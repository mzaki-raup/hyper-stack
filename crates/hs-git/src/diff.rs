//! Unified diffs as git prints them, parsed into hunks and numbered lines
//! for a diff view.

use std::path::Path;

/// Larger untracked files are not read for an "everything added" view.
const MAX_UNTRACKED_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
    /// "\ No newline at end of file".
    NoNewline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line number in the old file (context and removed lines).
    pub old: Option<u32>,
    /// Line number in the new file (context and added lines).
    pub new: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The `@@ -a,b +c,d @@ context` line.
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// The new path (the old one for a deletion).
    pub path: String,
    pub orig_path: Option<String>,
    pub binary: bool,
    /// "new file mode 100644", "deleted file mode", "rename from …", "old mode …".
    pub notes: Vec<String>,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    pub fn empty(path: &str) -> Self {
        Self {
            path: path.to_string(),
            ..Default::default()
        }
    }

    pub fn added(&self) -> usize {
        self.count(LineKind::Added)
    }

    pub fn removed(&self) -> usize {
        self.count(LineKind::Removed)
    }

    fn count(&self, k: LineKind) -> usize {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == k)
            .count()
    }
}

fn unquote(p: &str) -> String {
    // core.quotepath=false leaves non-ASCII alone; names with quotes, tabs
    // or backslashes still arrive C-quoted.
    let p = p.trim();
    let p = p
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .map_or_else(
            || p.to_string(),
            |s| {
                s.replace("\\\"", "\"")
                    .replace("\\t", "\t")
                    .replace("\\\\", "\\")
            },
        );
    p.strip_prefix("a/")
        .or_else(|| p.strip_prefix("b/"))
        .unwrap_or(&p)
        .to_string()
}

fn hunk_starts(header: &str) -> (u32, u32) {
    // @@ -12,7 +12,8 @@ …
    let mut old = 0;
    let mut new = 0;
    for part in header.split_whitespace().skip(1).take(2) {
        let num = |s: &str| {
            s.split(',')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        if let Some(o) = part.strip_prefix('-') {
            old = num(o);
        } else if let Some(n) = part.strip_prefix('+') {
            new = num(n);
        }
    }
    (old, new)
}

/// Parse the output of `git diff` / `git show` (one or more files).
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            // "a/x b/x": the b side is the new path; renames fix it below.
            let path = rest
                .rfind(" b/")
                .map(|i| unquote(&rest[i + 1..]))
                .unwrap_or_else(|| unquote(rest));
            files.push(FileDiff::empty(&path));
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if let Some(h) = line.strip_prefix("@@") {
            let header = format!("@@{h}");
            (old_no, new_no) = hunk_starts(&header);
            f.hunks.push(Hunk {
                header,
                lines: Vec::new(),
            });
            continue;
        }
        if let Some(hunk) = f.hunks.last_mut() {
            let (kind, text) = match line.as_bytes().first() {
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'\\') => (LineKind::NoNewline, line),
                None => (LineKind::Context, ""),
                _ => continue,
            };
            let (old, new) = match kind {
                LineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                    (Some(old_no - 1), Some(new_no - 1))
                }
                LineKind::Added => {
                    new_no += 1;
                    (None, Some(new_no - 1))
                }
                LineKind::Removed => {
                    old_no += 1;
                    (Some(old_no - 1), None)
                }
                LineKind::NoNewline => (None, None),
            };
            hunk.lines.push(DiffLine {
                kind,
                old,
                new,
                text: text.to_string(),
            });
            continue;
        }
        // Extended header lines before the first hunk.
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            f.binary = true;
        } else if let Some(p) = line.strip_prefix("rename from ") {
            f.orig_path = Some(unquote(p));
            f.notes.push(line.to_string());
        } else if let Some(p) = line.strip_prefix("rename to ") {
            f.path = unquote(p);
            f.notes.push(line.to_string());
        } else if line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
            || line.starts_with("old mode")
            || line.starts_with("new mode")
            || line.starts_with("similarity index")
        {
            f.notes.push(line.to_string());
        }
    }
    files
}

/// An untracked file shown as wholly added.
pub fn all_added(path: &str, full: &Path) -> FileDiff {
    let mut d = FileDiff::empty(path);
    d.notes.push("untracked".into());
    let Ok(meta) = std::fs::metadata(full) else {
        return d;
    };
    if meta.len() > MAX_UNTRACKED_BYTES {
        d.notes.push(format!("{} bytes, not shown", meta.len()));
        return d;
    }
    let Ok(bytes) = std::fs::read(full) else {
        return d;
    };
    if bytes.contains(&0) {
        d.binary = true;
        return d;
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<DiffLine> = text
        .lines()
        .enumerate()
        .map(|(i, l)| DiffLine {
            kind: LineKind::Added,
            old: None,
            new: Some(i as u32 + 1),
            text: l.to_string(),
        })
        .collect();
    if !lines.is_empty() {
        d.hunks.push(Hunk {
            header: format!("@@ -0,0 +1,{} @@", lines.len()),
            lines,
        });
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_number_their_lines() {
        let text = "diff --git a/src/a.rs b/src/a.rs\nindex 1..2 100644\n--- a/src/a.rs\n+++ b/src/a.rs\n\
@@ -10,4 +10,5 @@ fn main() {\n ctx\n-old\n+new\n+more\n ctx2\n\\ No newline at end of file\n\
diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n\
diff --git a/old.md b/new.md\nsimilarity index 90%\nrename from old.md\nrename to new.md\n";
        let f = parse(text);
        assert_eq!(f.len(), 3);
        let a = &f[0];
        assert_eq!(a.path, "src/a.rs");
        assert_eq!((a.added(), a.removed()), (2, 1));
        let l = &a.hunks[0].lines;
        assert_eq!((l[0].old, l[0].new), (Some(10), Some(10)));
        assert_eq!(
            (l[1].kind, l[1].old, l[1].new),
            (LineKind::Removed, Some(11), None)
        );
        assert_eq!((l[2].kind, l[2].new), (LineKind::Added, Some(11)));
        assert_eq!((l[4].old, l[4].new), (Some(12), Some(13)));
        assert_eq!(l[5].kind, LineKind::NoNewline);
        assert!(f[1].binary);
        assert_eq!(f[2].path, "new.md");
        assert_eq!(f[2].orig_path.as_deref(), Some("old.md"));
    }
}
