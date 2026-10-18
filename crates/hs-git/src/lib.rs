//! The read side of Git for HyperStack (docs/tools/git.md, ADR-006 as
//! amended 2026-10-12): find the repository a folder belongs to, read its
//! status with the branch and upstream, diff a file (staged, unstaged,
//! untracked, or as changed by a commit), list history and branches.
//!
//! Everything runs the user's own `git` with porcelain output, so the
//! answers match the command line (config, `.gitignore`, attributes,
//! worktrees). Every call blocks while `git` runs: call it from
//! `AppRuntime::spawn_blocking`, never from the UI thread (ADR-013).
//! Reads never take the index lock (`--no-optional-locks`), so a refresh
//! cannot get in the way of the user's own `git` in a terminal.

use std::path::{Path, PathBuf};
use std::process::Output;

pub mod diff;
pub mod write;

pub use diff::{DiffLine, FileDiff, Hunk, LineKind};
pub use write::{CommitOptions, PushOptions, Remote, Stash};

#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum GitError {
    #[error("git is not installed or not on PATH")]
    NotInstalled,
    #[error("git {command}: {message}")]
    Failed { command: String, message: String },
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Keep diffs and logs of a pathological repository from swamping the UI.
const MAX_OUTPUT: usize = 16 * 1024 * 1024;

/// Run `git -C root <args>` and return its stdout.
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let out: Output = hs_core::paths::background_command("git")
        .arg("-C")
        .arg(root)
        .args([
            "--no-optional-locks",
            "-c",
            "core.quotepath=false",
            "-c",
            "color.ui=false",
        ])
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GitError::NotInstalled
            } else {
                GitError::Failed {
                    command: args.first().unwrap_or(&"").to_string(),
                    message: e.to_string(),
                }
            }
        })?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(GitError::Failed {
            command: args.first().unwrap_or(&"").to_string(),
            message: if msg.is_empty() {
                format!("exit status {}", out.status)
            } else {
                msg
            },
        });
    }
    let mut stdout = out.stdout;
    stdout.truncate(MAX_OUTPUT);
    Ok(stdout)
}

/// The installed git's version line ("git version 2.43.0"), or why not.
pub fn version() -> Result<String> {
    let out = hs_core::paths::background_command("git")
        .arg("--version")
        .output()
        .map_err(|_| GitError::NotInstalled)?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The work tree root of the repository `path` lies in, by looking for a
/// `.git` folder (or the `.git` file of a worktree or submodule) in it and
/// its parents. No process is started.
pub fn discover(path: &Path) -> Option<PathBuf> {
    let start = if path.is_file() { path.parent()? } else { path };
    start
        .ancestors()
        .find(|d| d.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Whether a file-system event at `path` (inside the work tree `root`)
/// can change what `status` reports. Inside `.git` only the files that
/// move HEAD, the index or a ref count; object and lock churn do not.
pub fn event_matters(root: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    let mut parts = rel.components();
    match parts.next() {
        Some(c) if c.as_os_str() == ".git" => {
            let rest: Vec<String> = parts
                .map(|c| c.as_os_str().to_string_lossy().to_string())
                .collect();
            match rest.first().map(String::as_str) {
                Some("HEAD" | "index" | "ORIG_HEAD" | "MERGE_HEAD" | "FETCH_HEAD") => true,
                Some("refs" | "packed-refs") => !rest.last().is_some_and(|l| l.ends_with(".lock")),
                _ => false,
            }
        }
        Some(_) => true,
        None => false,
    }
}

// ---- status ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Tracked and changed in the index and/or the work tree.
    Changed,
    Untracked,
    /// Unmerged: a merge, rebase or cherry-pick left a conflict.
    Conflict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    /// Relative to the work tree root, with `/`.
    pub path: String,
    /// The old path of a rename or copy.
    pub orig_path: Option<String>,
    /// Porcelain status letters: `.` unchanged, `M`, `A`, `D`, `R`, `C`,
    /// `T` (type change), `U` (unmerged), `?` (untracked).
    pub index: char,
    pub worktree: char,
    pub kind: Kind,
}

impl FileStatus {
    pub fn staged(&self) -> bool {
        self.kind == Kind::Changed && self.index != '.'
    }
    pub fn unstaged(&self) -> bool {
        self.kind == Kind::Changed && self.worktree != '.'
    }
    /// The one letter a file tree shows: the work tree change first, then
    /// the staged one.
    pub fn letter(&self) -> char {
        match self.kind {
            Kind::Untracked => 'U',
            Kind::Conflict => '!',
            Kind::Changed if self.worktree != '.' => self.worktree,
            Kind::Changed => self.index,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// `None` on a detached HEAD.
    pub branch: Option<String>,
    /// Short id of HEAD; `None` before the first commit.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileStatus>,
}

/// An in-progress Git sequencer operation that may need continuation or
/// abort (merge, rebase or cherry-pick).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationState {
    Merge,
    Rebase,
    CherryPick,
}

/// Detect an operation from Git's metadata files. Handles linked worktrees,
/// whose per-worktree metadata is under `.git/worktrees/<name>`.
pub fn operation_state(root: &Path) -> Option<OperationState> {
    let git_dir = git_dir(root).unwrap_or_else(|| root.join(".git"));
    if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
        Some(OperationState::Rebase)
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        Some(OperationState::CherryPick)
    } else if git_dir.join("MERGE_HEAD").exists() {
        Some(OperationState::Merge)
    } else {
        None
    }
}

fn git_dir(root: &Path) -> Option<PathBuf> {
    let marker = std::fs::read_to_string(root.join(".git")).ok()?;
    let git_dir = marker.trim().strip_prefix("gitdir: ")?;
    let path = Path::new(git_dir);
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    })
}

impl Status {
    /// "main", "main ↑2 ↓1", or "(detached 1a2b3c4)".
    pub fn branch_label(&self) -> String {
        let mut s = match (&self.branch, &self.head) {
            (Some(b), _) => b.clone(),
            (None, Some(h)) => format!("(detached {h})"),
            (None, None) => "(no branch)".into(),
        };
        if self.ahead > 0 {
            s.push_str(&format!(" \u{2191}{}", self.ahead));
        }
        if self.behind > 0 {
            s.push_str(&format!(" \u{2193}{}", self.behind));
        }
        s
    }
}

/// `git status` of the work tree at `root`, untracked files listed one by one.
pub fn status(root: &Path) -> Result<Status> {
    let out = git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=all",
        ],
    )?;
    Ok(parse_status(&String::from_utf8_lossy(&out)))
}

/// Parse `git status --porcelain=v2 --branch -z`.
pub fn parse_status(text: &str) -> Status {
    let mut st = Status::default();
    let mut fields = text.split('\0').peekable();
    while let Some(rec) = fields.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(h) = rec.strip_prefix("# ") {
            let (key, val) = h.split_once(' ').unwrap_or((h, ""));
            match key {
                "branch.oid" if val != "(initial)" => st.head = Some(val.chars().take(7).collect()),
                "branch.head" if val != "(detached)" => st.branch = Some(val.to_string()),
                "branch.upstream" => st.upstream = Some(val.to_string()),
                "branch.ab" => {
                    for part in val.split_whitespace() {
                        if let Some(n) = part.strip_prefix('+') {
                            st.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            st.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let xy = |s: &str| {
            let mut c = s.chars();
            (c.next().unwrap_or('.'), c.next().unwrap_or('.'))
        };
        match rec.as_bytes()[0] {
            // 1 XY sub mH mI mW hH hI path
            b'1' => {
                let parts: Vec<&str> = rec.splitn(9, ' ').collect();
                if parts.len() == 9 {
                    let (x, y) = xy(parts[1]);
                    st.files.push(FileStatus {
                        path: parts[8].to_string(),
                        orig_path: None,
                        index: x,
                        worktree: y,
                        kind: Kind::Changed,
                    });
                }
            }
            // 2 XY sub mH mI mW hH hI Xscore path NUL origPath
            b'2' => {
                let parts: Vec<&str> = rec.splitn(10, ' ').collect();
                if parts.len() == 10 {
                    let (x, y) = xy(parts[1]);
                    let orig = fields.next().map(str::to_string);
                    st.files.push(FileStatus {
                        path: parts[9].to_string(),
                        orig_path: orig,
                        index: x,
                        worktree: y,
                        kind: Kind::Changed,
                    });
                }
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            b'u' => {
                let parts: Vec<&str> = rec.splitn(11, ' ').collect();
                if parts.len() == 11 {
                    let (x, y) = xy(parts[1]);
                    st.files.push(FileStatus {
                        path: parts[10].to_string(),
                        orig_path: None,
                        index: x,
                        worktree: y,
                        kind: Kind::Conflict,
                    });
                }
            }
            b'?' => st.files.push(FileStatus {
                path: rec[2..].to_string(),
                orig_path: None,
                index: '?',
                worktree: '?',
                kind: Kind::Untracked,
            }),
            _ => {}
        }
    }
    st.files.sort_by(|a, b| a.path.cmp(&b.path));
    st
}

// ---- diffs ------------------------------------------------------------------

/// Which side of a file's change to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Index against HEAD (`git diff --cached`).
    Staged,
    /// Work tree against the index (`git diff`).
    Unstaged,
    /// A file git does not track yet: all of it is new.
    Untracked,
}

/// The diff of one file (repository-relative `path`).
pub fn file_diff(root: &Path, path: &str, side: Side) -> Result<FileDiff> {
    match side {
        Side::Untracked => Ok(diff::all_added(path, &root.join(path))),
        Side::Staged | Side::Unstaged => {
            let mut args = vec!["diff", "--no-ext-diff", "-M"];
            if side == Side::Staged {
                args.push("--cached");
            }
            args.extend(["--", path]);
            let out = git(root, &args)?;
            Ok(diff::parse(&String::from_utf8_lossy(&out))
                .into_iter()
                .next()
                .unwrap_or_else(|| FileDiff::empty(path)))
        }
    }
}

/// The text of `path` (repository-relative) in the last commit; `None`
/// when HEAD does not have it (a new file, or no commit yet) or it is binary.
pub fn head_text(root: &Path, path: &str) -> Result<Option<String>> {
    let spec = format!("HEAD:{path}");
    match git(root, &["show", "--no-textconv", &spec]) {
        Ok(bytes) if bytes.contains(&0) => Ok(None),
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(GitError::Failed { message, .. })
            if message.contains("does not exist")
                || message.contains("not in 'HEAD'")
                || message.contains("invalid object name")
                || message.contains("bad revision") =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// How one line of the current text differs from the committed one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineChange {
    Added,
    Modified,
    /// Lines were removed just above this one.
    Deleted,
}

/// Per-line changes of `new` against `old` (`None` for an unchanged
/// line), indexed by line of `new`; `old = None` marks every line added.
/// Gives up after a short time on pathological input and returns what it
/// has, so it is safe to call while the user types.
pub fn line_changes(old: Option<&str>, new: &str) -> Vec<Option<LineChange>> {
    let lines = new.split('\n').count();
    let mut out = vec![None; lines];
    let Some(old) = old else {
        if new.is_empty() {
            return out;
        }
        let real = if new.ends_with('\n') {
            lines - 1
        } else {
            lines
        };
        out[..real].fill(Some(LineChange::Added));
        return out;
    };
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(150))
        .diff_lines(old, new);
    for op in diff.ops() {
        match *op {
            similar::DiffOp::Equal { .. } => {}
            similar::DiffOp::Delete { new_index, .. } => {
                let at = new_index.min(lines.saturating_sub(1));
                if out[at].is_none() {
                    out[at] = Some(LineChange::Deleted);
                }
            }
            similar::DiffOp::Insert {
                new_index, new_len, ..
            } => out[new_index..new_index + new_len].fill(Some(LineChange::Added)),
            similar::DiffOp::Replace {
                new_index, new_len, ..
            } => out[new_index..new_index + new_len].fill(Some(LineChange::Modified)),
        }
    }
    out
}

// ---- history ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Unix seconds (author date).
    pub time: i64,
    /// Branches and tags pointing here ("HEAD -> main", "tag: v1.0", "origin/main").
    pub refs: Vec<String>,
    pub subject: String,
}

const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%P%x1f%an%x1f%ae%x1f%at%x1f%D%x1f%s%x1e";

/// Up to `limit` commits of the current branch, newest first, after
/// skipping `skip`. A repository without commits has an empty history.
pub fn log(root: &Path, skip: usize, limit: usize) -> Result<Vec<Commit>> {
    let skip = format!("--skip={skip}");
    let limit = format!("--max-count={limit}");
    match git(root, &["log", LOG_FORMAT, &skip, &limit]) {
        Ok(out) => Ok(parse_log(&String::from_utf8_lossy(&out))),
        // "does not have any commits yet"
        Err(GitError::Failed { message, .. }) if message.contains("any commits") => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            if rec.is_empty() {
                return None;
            }
            let f: Vec<&str> = rec.split('\x1f').collect();
            if f.len() < 8 {
                return None;
            }
            Some(Commit {
                id: f[0].to_string(),
                short: f[1].to_string(),
                parents: f[2].split_whitespace().map(str::to_string).collect(),
                author: f[3].to_string(),
                email: f[4].to_string(),
                time: f[5].parse().unwrap_or(0),
                refs: f[6]
                    .split(", ")
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect(),
                subject: f[7].to_string(),
            })
        })
        .collect()
}

/// A file a commit changed: status letter (`A`, `M`, `D`, `R`, …), path
/// and, for renames and copies, the old path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub status: char,
    pub path: String,
    pub orig_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    /// The whole message (subject and body).
    pub message: String,
    pub committer: String,
    pub commit_time: i64,
    /// Against the first parent (a merge shows what it brought in).
    pub files: Vec<ChangedFile>,
}

pub fn commit_details(root: &Path, id: &str, first_parent: Option<&str>) -> Result<CommitDetails> {
    let meta = git(root, &["show", "-s", "--format=%cn%x1f%ct%x1f%B", id])?;
    let meta = String::from_utf8_lossy(&meta).to_string();
    let mut f = meta.splitn(3, '\x1f');
    let committer = f.next().unwrap_or_default().to_string();
    let commit_time = f.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    let message = f.next().unwrap_or_default().trim_end().to_string();
    let names = match first_parent {
        Some(p) => git(
            root,
            &["diff-tree", "-r", "-M", "--name-status", "-z", p, id],
        )?,
        None => git(
            root,
            &[
                "diff-tree",
                "-r",
                "-M",
                "--root",
                "--name-status",
                "-z",
                "--no-commit-id",
                id,
            ],
        )?,
    };
    Ok(CommitDetails {
        message,
        committer,
        commit_time,
        files: parse_name_status(&String::from_utf8_lossy(&names)),
    })
}

/// Parse `--name-status -z`: `M\0path\0`, `R100\0old\0new\0`.
pub fn parse_name_status(text: &str) -> Vec<ChangedFile> {
    let mut out = Vec::new();
    let mut it = text.split('\0').filter(|s| !s.is_empty());
    while let Some(code) = it.next() {
        let status = code.chars().next().unwrap_or('M');
        if matches!(status, 'R' | 'C') {
            let (Some(old), Some(new)) = (it.next(), it.next()) else {
                break;
            };
            out.push(ChangedFile {
                status,
                path: new.to_string(),
                orig_path: Some(old.to_string()),
            });
        } else if let Some(p) = it.next() {
            out.push(ChangedFile {
                status,
                path: p.to_string(),
                orig_path: None,
            });
        }
    }
    out
}

/// How a commit changed one file (against its first parent).
pub fn commit_file_diff(
    root: &Path,
    id: &str,
    first_parent: Option<&str>,
    file: &ChangedFile,
) -> Result<FileDiff> {
    let mut paths = vec![file.path.as_str()];
    if let Some(o) = &file.orig_path {
        paths.push(o);
    }
    let out = match first_parent {
        Some(p) => {
            let mut args = vec!["diff", "--no-ext-diff", "-M", p, id, "--"];
            args.extend(paths);
            git(root, &args)?
        }
        None => {
            let mut args = vec!["show", "--no-ext-diff", "--format=", "-M", id, "--"];
            args.extend(paths);
            git(root, &args)?
        }
    };
    Ok(diff::parse(&String::from_utf8_lossy(&out))
        .into_iter()
        .find(|d| d.path == file.path)
        .unwrap_or_else(|| FileDiff::empty(&file.path)))
}

// ---- branches ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// "main", "origin/main".
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Unix seconds of the tip's commit.
    pub time: i64,
    pub subject: String,
}

pub fn branches(root: &Path) -> Result<Vec<Branch>> {
    let out = git(
        root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%1f%(HEAD)%1f%(upstream:short)%1f%(upstream:track,nobracket)%1f%(committerdate:unix)%1f%(subject)%1e",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(parse_branches(&String::from_utf8_lossy(&out)))
}

pub fn parse_branches(text: &str) -> Vec<Branch> {
    text.split('\x1e')
        .filter_map(|rec| {
            let f: Vec<&str> = rec.trim_start_matches('\n').split('\x1f').collect();
            if f.len() < 6 {
                return None;
            }
            let (name, remote) = match f[0].strip_prefix("refs/heads/") {
                Some(n) => (n, false),
                None => (f[0].strip_prefix("refs/remotes/")?, true),
            };
            // origin/HEAD is an alias, not a branch.
            if remote && name.ends_with("/HEAD") {
                return None;
            }
            let (mut ahead, mut behind) = (0, 0);
            for part in f[3].split(", ") {
                if let Some(n) = part.strip_prefix("ahead ") {
                    ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix("behind ") {
                    behind = n.parse().unwrap_or(0);
                }
            }
            Some(Branch {
                name: name.to_string(),
                remote,
                current: f[1] == "*",
                upstream: Some(f[2]).filter(|u| !u.is_empty()).map(str::to_string),
                ahead,
                behind,
                time: f[4].parse().unwrap_or(0),
                subject: f[5].to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_v2_records() {
        let text = "# branch.oid 1234567890abcdef\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/main.rs\0\
1 A. N... 000000 100644 100644 000 ccc new file.txt\0\
2 R. N... 100644 100644 100644 ddd ddd R100 docs/b.md\0docs/a.md\0\
u UU N... 100644 100644 100644 100644 e1 e2 e3 conflict.rs\0\
? notes/todo.txt\0";
        let st = parse_status(text);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.head.as_deref(), Some("1234567"));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.branch_label(), "main \u{2191}2 \u{2193}1");
        let paths: Vec<&str> = st.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "conflict.rs",
                "docs/b.md",
                "new file.txt",
                "notes/todo.txt",
                "src/main.rs"
            ]
        );
        let get = |p: &str| st.files.iter().find(|f| f.path == p).unwrap();
        assert!(get("src/main.rs").unstaged() && !get("src/main.rs").staged());
        assert_eq!(get("src/main.rs").letter(), 'M');
        assert!(get("new file.txt").staged());
        assert_eq!(get("docs/b.md").orig_path.as_deref(), Some("docs/a.md"));
        assert_eq!(get("conflict.rs").kind, Kind::Conflict);
        assert_eq!(get("notes/todo.txt").letter(), 'U');
    }

    #[test]
    fn detached_and_initial() {
        let st = parse_status("# branch.oid (initial)\0# branch.head main\0");
        assert_eq!(st.head, None);
        assert_eq!(st.branch_label(), "main");
        let st = parse_status("# branch.oid abcdef0123\0# branch.head (detached)\0");
        assert_eq!(st.branch_label(), "(detached abcdef0)");
    }

    #[test]
    fn log_and_names() {
        let text = "aaaa\x1faa\x1fpp qq\x1fAda\x1fada@x\x1f1700000000\x1fHEAD -> main, tag: v1\x1fMerge it\x1e\n\
bbbb\x1fbb\x1f\x1fAda\x1fada@x\x1f1690000000\x1f\x1fFirst\x1e";
        let c = parse_log(text);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].parents, ["pp", "qq"]);
        assert_eq!(c[0].refs, ["HEAD -> main", "tag: v1"]);
        assert!(c[1].parents.is_empty() && c[1].refs.is_empty());
        let f = parse_name_status("M\0a.rs\0R087\0old.md\0new.md\0D\0gone\0");
        assert_eq!(f.len(), 3);
        assert_eq!(f[1].orig_path.as_deref(), Some("old.md"));
        assert_eq!(f[2].status, 'D');
    }

    #[test]
    fn branch_records() {
        let text = "refs/heads/main\x1f*\x1forigin/main\x1fahead 1, behind 2\x1f1700000000\x1ftip\x1e\n\
refs/remotes/origin/HEAD\x1f \x1f\x1f\x1f1700000000\x1ftip\x1e\n\
refs/remotes/origin/main\x1f \x1f\x1f\x1f1700000000\x1ftip\x1e";
        let b = parse_branches(text);
        assert_eq!(b.len(), 2);
        assert!(b[0].current && !b[0].remote);
        assert_eq!((b[0].ahead, b[0].behind), (1, 2));
        assert!(b[1].remote && b[1].name == "origin/main");
    }

    #[test]
    fn line_changes_mark_added_modified_and_deleted() {
        use LineChange::*;
        let old = "a\nb\nc\nd\n";
        let new = "a\nB\nc\nnew\n";
        // b -> B modified; d -> new modified; trailing empty line clean.
        assert_eq!(
            line_changes(Some(old), new),
            [None, Some(Modified), None, Some(Modified), None]
        );
        let ch = line_changes(Some(old), "a\nb\nx\ny\nc\nd\n");
        assert_eq!(ch[2..4], [Some(Added), Some(Added)]);
        let ch = line_changes(Some(old), "a\nd\n");
        assert_eq!(ch[1], Some(Deleted));
        assert_eq!(
            line_changes(None, "x\ny\n"),
            [Some(Added), Some(Added), None]
        );
        assert!(line_changes(Some(old), old).iter().all(Option::is_none));
    }

    #[test]
    fn events_inside_git_dir() {
        let root = Path::new("/r");
        assert!(event_matters(root, Path::new("/r/src/a.rs")));
        assert!(event_matters(root, Path::new("/r/.git/index")));
        assert!(event_matters(root, Path::new("/r/.git/refs/heads/main")));
        assert!(!event_matters(
            root,
            Path::new("/r/.git/refs/heads/main.lock")
        ));
        assert!(!event_matters(root, Path::new("/r/.git/objects/ab/cdef")));
        assert!(!event_matters(root, Path::new("/r/.git/index.lock")));
        assert!(!event_matters(root, Path::new("/elsewhere/a")));
    }
}
