//! Clickable things in terminal output (docs/tools/terminal.md §3,
//! "URL/path detection"): URLs and `path:line:col` locations such as the
//! ones in compiler errors and panic messages. Detection works on one grid
//! line at a time and reports character columns, so the widget can map a
//! hovered cell to a link without knowing anything about the text.

use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// A file path, as written (relative paths are resolved by the caller
    /// against the shell's folder), with an optional 1-based line and column.
    Path {
        path: String,
        line: Option<u32>,
        col: Option<u32>,
    },
}

/// A link on one line: `start..end` are character (cell) columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub target: LinkTarget,
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)\b(?:https?|ftp|file)://[^\s<>"'`]+"#).expect("url regex"));

// A path needs a separator or an extension so plain words never match:
// `./a`, `../a`, `~/a`, `/abs/a`, `C:\a`, `C:/a`, `dir/file.rs`, `file.rs`
// (the last only when followed by `:line`). Optional `:line[:col]` or
// `(line[,col])` suffix.
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?x)
        (?P<path>
            (?:[A-Za-z]:[\\/]|\.{1,2}[\\/]|~[\\/]|[\\/])?   # anchor
            (?:[\w.@+-]+[\\/])*                              # folders
            [\w.@+-]*\.?[\w@+-]+                             # file name
        )
        (?:
            :(?P<line>\d+)(?::(?P<col>\d+))?
          | \((?P<pline>\d+)(?:,\s*(?P<pcol>\d+))?\)
        )?
        "#,
    )
    .expect("path regex")
});

fn char_col(s: &str, byte: usize) -> usize {
    s[..byte].chars().count()
}

/// Trailing punctuation that ends a sentence rather than a URL or path.
fn trim_trailing(s: &str) -> &str {
    let mut end = s.len();
    let bytes = s.as_bytes();
    while end > 0 {
        let c = bytes[end - 1];
        let closing_unbalanced = |open: u8, close: u8| {
            c == close
                && s[..end].bytes().filter(|&b| b == close).count()
                    > s[..end].bytes().filter(|&b| b == open).count()
        };
        if matches!(c, b'.' | b',' | b';' | b':' | b'!' | b'?' | b'\'' | b'"')
            || closing_unbalanced(b'(', b')')
            || closing_unbalanced(b'[', b']')
        {
            end -= 1;
        } else {
            break;
        }
    }
    &s[..end]
}

fn looks_like_path(p: &str, has_position: bool) -> bool {
    if p.len() < 2 || p.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return false;
    }
    let has_sep = p.contains('/') || p.contains('\\');
    let has_ext = p
        .rsplit_once('.')
        .is_some_and(|(stem, ext)| !stem.is_empty() && (1..=8).contains(&ext.len()));
    if has_sep {
        // `a/b` alone is too often prose ("and/or"); require an anchor, an
        // extension or a position.
        let anchored = p.starts_with('/')
            || p.starts_with("./")
            || p.starts_with("../")
            || p.starts_with("~/")
            || p.starts_with(".\\")
            || p.starts_with("..\\")
            || p.get(1..3).is_some_and(|s| s == ":\\" || s == ":/");
        anchored || has_ext || has_position
    } else {
        has_ext && has_position
    }
}

/// Every link on `line`, left to right, never overlapping.
pub fn find_links(line: &str) -> Vec<Link> {
    let mut out: Vec<Link> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    for m in URL.find_iter(line) {
        let text = trim_trailing(m.as_str());
        let (s, e) = (m.start(), m.start() + text.len());
        taken.push((s, e));
        out.push(Link {
            start: char_col(line, s),
            end: char_col(line, e),
            target: LinkTarget::Url(text.to_string()),
        });
    }
    for caps in PATH.captures_iter(line) {
        let whole = caps.get(0).expect("match");
        if taken
            .iter()
            .any(|&(s, e)| whole.start() < e && s < whole.end())
        {
            continue;
        }
        let path_m = caps.name("path").expect("path group");
        // A path must start at a word boundary (not mid-identifier).
        if path_m.start() > 0 {
            let prev = line[..path_m.start()].chars().next_back().unwrap_or(' ');
            if prev.is_alphanumeric() || prev == '_' {
                continue;
            }
        }
        let num = |a: &str, b: &str| {
            caps.name(a)
                .or_else(|| caps.name(b))
                .and_then(|m| m.as_str().parse::<u32>().ok())
        };
        let lnum = num("line", "pline");
        let col = num("col", "pcol");
        let path = trim_trailing(path_m.as_str());
        // `name.method(1)` is a call, not a location: the `(line,col)` form
        // needs a folder or a column (MSBuild writes `File.cs(12,3)`).
        if caps.name("pline").is_some()
            && col.is_none()
            && !path.contains('/')
            && !path.contains('\\')
        {
            continue;
        }
        if path.is_empty() || !looks_like_path(path, lnum.is_some()) {
            continue;
        }
        let end = if lnum.is_some() {
            whole.end()
        } else {
            path_m.start() + path.len()
        };
        out.push(Link {
            start: char_col(line, path_m.start()),
            end: char_col(line, end),
            target: LinkTarget::Path {
                path: path.to_string(),
                line: lnum,
                col,
            },
        });
    }
    out.sort_by_key(|l| l.start);
    out
}

/// The link under character column `col`, if any.
pub fn link_at(line: &str, col: usize) -> Option<Link> {
    find_links(line)
        .into_iter()
        .find(|l| (l.start..l.end).contains(&col))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(line: &str) -> Vec<LinkTarget> {
        find_links(line).into_iter().map(|l| l.target).collect()
    }

    fn path(p: &str, line: Option<u32>, col: Option<u32>) -> LinkTarget {
        LinkTarget::Path {
            path: p.into(),
            line,
            col,
        }
    }

    #[test]
    fn urls_are_found_and_trailing_punctuation_dropped() {
        assert_eq!(
            targets("see https://example.com/a?b=1, then (http://x.io/y)."),
            vec![
                LinkTarget::Url("https://example.com/a?b=1".into()),
                LinkTarget::Url("http://x.io/y".into()),
            ]
        );
        assert_eq!(
            targets("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
            vec![LinkTarget::Url(
                "https://en.wikipedia.org/wiki/Rust_(programming_language)".into()
            )]
        );
    }

    #[test]
    fn compiler_and_panic_locations_carry_line_and_column() {
        assert_eq!(
            targets("  --> crates/hs-term/src/links.rs:42:7"),
            vec![path("crates/hs-term/src/links.rs", Some(42), Some(7))]
        );
        assert_eq!(
            targets("thread 'main' panicked at src/main.rs:10:5:"),
            vec![path("src/main.rs", Some(10), Some(5))]
        );
        assert_eq!(
            targets(r"C:\work\app\Program.cs(12,3): error CS1002"),
            vec![path(r"C:\work\app\Program.cs", Some(12), Some(3))]
        );
        assert_eq!(
            targets("main.go:7 undefined: x"),
            vec![path("main.go", Some(7), None)]
        );
    }

    #[test]
    fn plain_paths_need_an_anchor_or_extension() {
        assert_eq!(
            targets("wrote ./target/out.txt and /etc/hosts"),
            vec![
                path("./target/out.txt", None, None),
                path("/etc/hosts", None, None)
            ]
        );
        assert_eq!(
            targets("edit docs/README.md now"),
            vec![path("docs/README.md", None, None)]
        );
    }

    #[test]
    fn prose_and_numbers_are_not_links() {
        assert!(targets("and/or either way, version 1.2.3 is out").is_empty());
        assert!(targets("ratio 3:4 at 10:30").is_empty());
        assert!(
            targets("Cargo.toml").is_empty(),
            "bare file name without position"
        );
        assert!(targets("call foo.bar(1) here").is_empty());
    }

    #[test]
    fn columns_are_character_columns() {
        let line = "é → src/a.rs:3";
        let l = &find_links(line)[0];
        assert_eq!(l.start, 4);
        assert_eq!(l.end, line.chars().count());
        assert!(link_at(line, 5).is_some());
        assert!(link_at(line, 1).is_none());
    }
}
