//! The write side of Git (docs/tools/git.md §5, §8, §10): stage, unstage,
//! discard, commit, stash, branches, and the network operations. Every
//! call runs the user's own `git`, so hooks, signing, credential helpers
//! and config all apply exactly as they do on the command line - and
//! every call blocks while `git` runs, so it belongs on the blocking pool,
//! never on the UI thread (ADR-013).
//!
//! Nothing here asks a question or forces anything: the guards that make
//! a destructive operation confirm, or refuse it on a protected remote,
//! live in the tool, because that is where the user is.

use std::path::Path;

use crate::{Result, git};

/// Add `paths` to the index (`git add --`). An empty list is a no-op.
pub fn stage(root: &Path, paths: &[&str]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add", "--"];
    args.extend(paths);
    git(root, &args).map(|_| ())
}

/// Stage everything, including new files (`git add -A`).
pub fn stage_all(root: &Path) -> Result<()> {
    git(root, &["add", "-A"]).map(|_| ())
}

/// Take `paths` out of the index, keeping the work tree as it is.
/// `git reset -- <paths>` rather than `restore --staged`: the latter
/// resolves HEAD and fails before the first commit, where `reset` treats
/// the unborn branch as an empty tree.
pub fn unstage(root: &Path, paths: &[&str]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["reset", "-q", "--"];
    args.extend(paths);
    git(root, &args).map(|_| ())
}

pub fn unstage_all(root: &Path) -> Result<()> {
    git(root, &["reset", "-q"]).map(|_| ())
}

/// Throw away the work-tree changes of tracked `paths`, so they match the
/// index again. Untracked files are not touched: see [`remove_untracked`].
pub fn discard(root: &Path, paths: &[&str]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["restore", "--worktree", "--"];
    args.extend(paths);
    git(root, &args).map(|_| ())
}

/// Delete untracked `paths` (`git clean -f --`), which is the only way an
/// untracked file is "discarded".
pub fn remove_untracked(root: &Path, paths: &[&str]) -> Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["clean", "-f", "--"];
    args.extend(paths);
    git(root, &args).map(|_| ())
}

/// What a commit is made of, beyond the staged changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitOptions {
    /// Replace the last commit instead of adding one.
    pub amend: bool,
    /// Add a `Signed-off-by` trailer.
    pub signoff: bool,
}

/// Commit the index with `message`. The user's `commit.gpgsign`, hooks and
/// template apply as on the command line; a signing prompt that cannot be
/// shown fails rather than hangs (`GIT_TERMINAL_PROMPT=0`).
pub fn commit(root: &Path, message: &str, opts: &CommitOptions) -> Result<()> {
    let mut args = vec!["commit", "-q", "-m", message];
    if opts.amend {
        args.push("--amend");
    }
    if opts.signoff {
        args.push("--signoff");
    }
    git(root, &args).map(|_| ())
}

/// One stash entry as `git stash list` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// `stash@{0}`
    pub id: String,
    pub message: String,
}

pub fn stash_list(root: &Path) -> Result<Vec<Stash>> {
    let out = git(root, &["stash", "list", "--format=%gd%x00%gs"])?;
    Ok(parse_stash_list(&String::from_utf8_lossy(&out)))
}

pub fn parse_stash_list(text: &str) -> Vec<Stash> {
    text.lines()
        .filter_map(|l| {
            let (id, message) = l.split_once('\0')?;
            Some(Stash {
                id: id.trim().to_string(),
                message: message.trim().to_string(),
            })
        })
        .collect()
}

/// Stash the work tree and index (`-u` takes untracked files too).
pub fn stash_push(root: &Path, message: &str, untracked: bool) -> Result<()> {
    let mut args = vec!["stash", "push", "-q"];
    if untracked {
        args.push("-u");
    }
    if !message.is_empty() {
        args.push("-m");
        args.push(message);
    }
    git(root, &args).map(|_| ())
}

pub fn stash_apply(root: &Path, id: &str) -> Result<()> {
    git(root, &["stash", "apply", "-q", id]).map(|_| ())
}

pub fn stash_pop(root: &Path, id: &str) -> Result<()> {
    git(root, &["stash", "pop", "-q", id]).map(|_| ())
}

pub fn stash_drop(root: &Path, id: &str) -> Result<()> {
    git(root, &["stash", "drop", "-q", id]).map(|_| ())
}

/// Switch to an existing branch.
pub fn checkout(root: &Path, branch: &str) -> Result<()> {
    git(root, &["switch", "-q", branch]).map(|_| ())
}

/// Create `name` at HEAD (or at `from`) and switch to it.
pub fn create_branch(root: &Path, name: &str, from: Option<&str>) -> Result<()> {
    let mut args = vec!["switch", "-q", "-c", name];
    if let Some(f) = from {
        args.push(f);
    }
    git(root, &args).map(|_| ())
}

/// Rename a local branch without switching to it. Git refuses to rename the
/// branch currently checked out in another worktree.
pub fn rename_branch(root: &Path, old: &str, new: &str) -> Result<()> {
    git(root, &["branch", "-m", old, new]).map(|_| ())
}

/// Set the current branch's upstream (tracking) branch.
pub fn set_upstream(root: &Path, remote: &str, branch: &str) -> Result<()> {
    git(
        root,
        &["branch", "--set-upstream-to", &format!("{remote}/{branch}")],
    )
    .map(|_| ())
}

/// Delete a local branch. `force` is `-D`, which drops unmerged work; the
/// caller asks first (docs/tools/git.md §10).
pub fn delete_branch(root: &Path, name: &str, force: bool) -> Result<()> {
    git(root, &["branch", if force { "-D" } else { "-d" }, name]).map(|_| ())
}

/// A remote and where it pushes to, for the protected-remote guard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

pub fn remotes(root: &Path) -> Result<Vec<Remote>> {
    let out = git(root, &["remote", "-v"])?;
    Ok(parse_remotes(&String::from_utf8_lossy(&out)))
}

/// List local tags by their short names, in refname order.
pub fn tags(root: &Path) -> Result<Vec<String>> {
    let out = git(root, &["tag", "--list", "--sort=refname"])?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .map(str::to_string)
        .collect())
}

pub fn add_remote(root: &Path, name: &str, url: &str) -> Result<()> {
    git(root, &["remote", "add", name, url]).map(|_| ())
}

pub fn set_remote_url(root: &Path, name: &str, url: &str) -> Result<()> {
    git(root, &["remote", "set-url", name, url]).map(|_| ())
}

pub fn remove_remote(root: &Path, name: &str) -> Result<()> {
    git(root, &["remote", "remove", name]).map(|_| ())
}

pub fn create_tag(root: &Path, name: &str, target: Option<&str>) -> Result<()> {
    let mut args = vec!["tag"];
    args.push(name);
    if let Some(target) = target {
        args.push(target);
    }
    git(root, &args).map(|_| ())
}

pub fn delete_tag(root: &Path, name: &str) -> Result<()> {
    git(root, &["tag", "-d", name]).map(|_| ())
}

pub fn push_tag(root: &Path, remote: &str, tag: &str) -> Result<String> {
    git(root, &["push", remote, tag]).map(|out| String::from_utf8_lossy(&out).to_string())
}

/// Merge a branch or ref into the current branch. Git may return an error
/// when conflicts are found; operation state and the index remain for the
/// user to resolve or abort.
pub fn merge(root: &Path, source: &str, no_commit: bool, squash: bool) -> Result<String> {
    let mut args = vec!["merge"];
    if no_commit {
        args.push("--no-commit");
    }
    if squash {
        args.push("--squash");
    }
    args.push(source);
    git(root, &args).map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn merge_continue(root: &Path) -> Result<String> {
    git(root, &["-c", "core.editor=true", "merge", "--continue"])
        .map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn merge_abort(root: &Path) -> Result<String> {
    git(root, &["merge", "--abort"]).map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn rebase(root: &Path, onto: &str) -> Result<String> {
    git(root, &["rebase", onto]).map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn rebase_continue(root: &Path) -> Result<String> {
    git(root, &["-c", "core.editor=true", "rebase", "--continue"])
        .map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn rebase_abort(root: &Path) -> Result<String> {
    git(root, &["rebase", "--abort"]).map(|out| String::from_utf8_lossy(&out).to_string())
}

/// Cherry-pick a commit (or range `A..B`) onto the current branch. Git may
/// return an error when conflicts are found; the operation state remains
/// for the user to resolve, continue or abort.
pub fn cherry_pick(root: &Path, commit: &str) -> Result<String> {
    git(root, &["cherry-pick", commit]).map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn cherry_pick_continue(root: &Path) -> Result<String> {
    git(
        root,
        &["-c", "core.editor=true", "cherry-pick", "--continue"],
    )
    .map(|out| String::from_utf8_lossy(&out).to_string())
}

pub fn cherry_pick_abort(root: &Path) -> Result<String> {
    git(root, &["cherry-pick", "--abort"]).map(|out| String::from_utf8_lossy(&out).to_string())
}

/// Skip the current commit during a cherry-pick and carry on with the rest.
pub fn cherry_pick_skip(root: &Path) -> Result<String> {
    git(root, &["cherry-pick", "--skip"]).map(|out| String::from_utf8_lossy(&out).to_string())
}

/// Which side of a conflicted file to keep when resolving it whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictSide {
    /// Stage 2, the version on the current branch (`HEAD`).
    Ours,
    /// Stage 3, the version from the commit being merged in.
    Theirs,
}

/// Resolve a conflicted file by keeping one whole side and staging it.
///
/// During a rebase Git's "ours" is the branch being rebased onto and
/// "theirs" is the commit being replayed; the labels follow Git's own
/// definitions rather than the user's mental model.
pub fn resolve_conflict(root: &Path, path: &str, side: ConflictSide) -> Result<()> {
    let flag = match side {
        ConflictSide::Ours => "--ours",
        ConflictSide::Theirs => "--theirs",
    };
    git(root, &["checkout", flag, "--", path])?;
    git(root, &["add", "--", path]).map(|_| ())
}

/// Parse `git remote -v`, keeping each remote's push URL once.
pub fn parse_remotes(text: &str) -> Vec<Remote> {
    let mut out: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(name), Some(url)) = (parts.next(), parts.next()) else {
            continue;
        };
        let kind = parts.next().unwrap_or("");
        match out.iter_mut().find(|r| r.name == name) {
            // The push URL wins when the two differ.
            Some(r) if kind == "(push)" => r.url = url.to_string(),
            Some(_) => {}
            None => out.push(Remote {
                name: name.to_string(),
                url: url.to_string(),
            }),
        }
    }
    out
}

/// `git fetch --prune` from every remote.
pub fn fetch(root: &Path) -> Result<String> {
    git(root, &["fetch", "--all", "--prune"]).map(|o| String::from_utf8_lossy(&o).to_string())
}

/// `git pull`, rebasing when asked; what git printed comes back for the
/// Output dock.
pub fn pull(root: &Path, rebase: bool) -> Result<String> {
    let mut args = vec!["pull"];
    if rebase {
        args.push("--rebase");
    }
    git(root, &args).map(|o| String::from_utf8_lossy(&o).to_string())
}

/// How a push is made.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PushOptions {
    /// `-u origin <branch>` for a branch with no upstream yet.
    pub set_upstream: Option<(String, String)>,
    /// `--force-with-lease`; the plain `--force` is never issued from here.
    pub force_with_lease: bool,
}

pub fn push(root: &Path, opts: &PushOptions) -> Result<String> {
    let mut args: Vec<&str> = vec!["push"];
    if opts.force_with_lease {
        args.push("--force-with-lease");
    }
    if let Some((remote, branch)) = &opts.set_upstream {
        args.push("-u");
        args.push(remote);
        args.push(branch);
    }
    // git writes its progress to stderr, which a success discards; the
    // stdout summary is what the dock shows.
    git(root, &args).map(|o| String::from_utf8_lossy(&o).to_string())
}

/// What to do with one hunk (docs/tools/git.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkOp {
    /// Add the hunk to the index.
    Stage,
    /// Take the hunk out of the index; the work tree keeps it.
    Unstage,
    /// Throw the hunk away in the work tree.
    Discard,
}

/// A patch holding just `hunk` of `path`, the shape `git apply` takes.
pub fn hunk_patch(path: &str, hunk: &crate::Hunk) -> String {
    let mut out = format!("--- a/{path}\n+++ b/{path}\n{}\n", hunk.header);
    for l in &hunk.lines {
        let prefix = match l.kind {
            crate::LineKind::Context => " ",
            crate::LineKind::Added => "+",
            crate::LineKind::Removed => "-",
            crate::LineKind::NoNewline => "",
        };
        if l.kind == crate::LineKind::NoNewline {
            out.push_str("\\ No newline at end of file");
        } else {
            out.push_str(prefix);
            out.push_str(&l.text);
        }
        out.push('\n');
    }
    out
}

/// Apply one hunk to the index or the work tree. The patch is fed on
/// stdin; `--recount` lets git fix up the line counts of a hunk that was
/// read back from a diff, and `--unidiff-zero` is not used so context is
/// still checked.
pub fn apply_hunk(root: &Path, path: &str, hunk: &crate::Hunk, op: HunkOp) -> Result<()> {
    use std::io::Write as _;
    use std::process::Stdio;
    let patch = hunk_patch(path, hunk);
    let mut args = vec!["apply", "--recount"];
    match op {
        HunkOp::Stage => args.push("--cached"),
        HunkOp::Unstage => {
            args.push("--cached");
            args.push("-R");
        }
        HunkOp::Discard => args.push("-R"),
    }
    args.push("-");
    let mut child = hs_core::paths::background_command("git")
        .arg("-C")
        .arg(root)
        .args(["-c", "core.quotepath=false"])
        .args(&args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                crate::GitError::NotInstalled
            } else {
                crate::GitError::Failed {
                    command: "apply".into(),
                    message: e.to_string(),
                }
            }
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(patch.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| crate::GitError::Failed {
            command: "apply".into(),
            message: e.to_string(),
        })?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(crate::GitError::Failed {
            command: "apply".into(),
            message: if msg.is_empty() {
                format!("exit status {}", out.status)
            } else {
                msg
            },
        })
    }
}

/// Whether a remote URL or a branch name matches one of the protected
/// patterns (docs/tools/git.md §10): a plain substring match, case
/// insensitive, so `prod` catches `origin-prod`, `production` and
/// `git@host:team/prod-api.git` alike.
pub fn is_protected(patterns: &[String], url: &str, branch: &str) -> bool {
    let url = url.to_ascii_lowercase();
    let branch = branch.to_ascii_lowercase();
    patterns
        .iter()
        .map(|p| p.trim().to_ascii_lowercase())
        .filter(|p| !p.is_empty())
        .any(|p| url.contains(&p) || branch.contains(&p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_keep_the_push_url() {
        let text = "origin\thttps://a.test/x.git (fetch)\norigin\tgit@a.test:x.git (push)\nmirror\thttps://b.test/y.git (fetch)\nmirror\thttps://b.test/y.git (push)\n";
        let r = parse_remotes(text);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "origin");
        assert_eq!(r[0].url, "git@a.test:x.git");
        assert_eq!(r[1].url, "https://b.test/y.git");
    }

    #[test]
    fn stash_list_is_id_and_message() {
        let s =
            parse_stash_list("stash@{0}\0WIP on main: 1a2b3c4 first\nstash@{1}\0On main: saved\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, "stash@{0}");
        assert_eq!(s[1].message, "On main: saved");
    }

    #[test]
    fn a_hunk_patch_is_a_unified_diff_of_that_hunk_alone() {
        let d = crate::diff::parse(
            "diff --git a/x.rs b/x.rs\n--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n one\n-two\n+deux\n",
        );
        let p = hunk_patch("x.rs", &d[0].hunks[0]);
        assert_eq!(
            p,
            "--- a/x.rs\n+++ b/x.rs\n@@ -1,2 +1,2 @@\n one\n-two\n+deux\n"
        );
    }

    #[test]
    fn protected_patterns_match_urls_and_branches() {
        let p = vec!["prod".to_string(), "release".to_string(), "".to_string()];
        assert!(is_protected(&p, "git@host:team/Prod-api.git", "main"));
        assert!(is_protected(&p, "https://host/x.git", "release/2.0"));
        assert!(!is_protected(&p, "https://host/x.git", "feature/one"));
        assert!(!is_protected(&[], "https://host/prod.git", "prod"));
    }

    #[test]
    fn renames_branch_and_sets_tracking_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "--initial-branch=main"]).unwrap();
        git(root, &["config", "user.name", "HyperStack Test"]).unwrap();
        git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
        // The owner's global config signs every commit; a fixture repository
        // must not depend on a warm GPG agent (see CLAUDE.md gotchas).
        git(root, &["config", "commit.gpgsign", "false"]).unwrap();
        git(root, &["config", "tag.gpgSign", "false"]).unwrap();
        std::fs::write(root.join("tracked.txt"), "tracked\n").unwrap();
        git(root, &["add", "tracked.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "initial"]).unwrap();
        git(root, &["branch", "feature/old"]).unwrap();

        rename_branch(root, "feature/old", "feature/new").unwrap();
        assert!(git(root, &["show-ref", "--verify", "refs/heads/feature/new"]).is_ok());
        assert!(git(root, &["show-ref", "--verify", "refs/heads/feature/old"]).is_err());

        git(
            root,
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/team/project.git",
            ],
        )
        .unwrap();
        git(
            root,
            &["update-ref", "refs/remotes/origin/feature/new", "HEAD"],
        )
        .unwrap();
        set_upstream(root, "origin", "feature/new").unwrap();
        let status = crate::status(root).unwrap();
        assert_eq!(status.upstream.as_deref(), Some("origin/feature/new"));
    }

    /// Cherry-pick a commit from one branch onto another, and detect the
    /// operation state while it is paused on a conflict.
    #[test]
    fn cherry_picks_a_commit_and_reports_operation_state() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "--initial-branch=main"]).unwrap();
        git(root, &["config", "user.name", "HyperStack Test"]).unwrap();
        git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(root, &["config", "commit.gpgsign", "false"]).unwrap();

        std::fs::write(root.join("base.txt"), "line\n").unwrap();
        git(root, &["add", "base.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "base"]).unwrap();

        // A side branch adds a file and commits it.
        git(root, &["checkout", "-q", "-b", "side"]).unwrap();
        std::fs::write(root.join("side.txt"), "side\n").unwrap();
        git(root, &["add", "side.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "side work"]).unwrap();
        let side_commit = String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_string();

        // Back on main, cherry-pick the side commit.
        git(root, &["checkout", "-q", "main"]).unwrap();
        cherry_pick(root, &side_commit).unwrap();
        assert!(root.join("side.txt").exists());
        assert_eq!(crate::operation_state(root), None);

        // A conflicting cherry-pick leaves the operation state set.
        git(root, &["checkout", "-q", "-b", "conflict"]).unwrap();
        std::fs::write(root.join("shared.txt"), "main version\n").unwrap();
        git(root, &["add", "shared.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "main shared"]).unwrap();
        let main_commit = String::from_utf8(git(root, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_string();

        git(root, &["checkout", "-q", "main"]).unwrap();
        std::fs::write(root.join("shared.txt"), "side version\n").unwrap();
        git(root, &["add", "shared.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "side shared"]).unwrap();

        // Cherry-picking the conflicting commit fails; the operation stays.
        assert!(cherry_pick(root, &main_commit).is_err());
        assert_eq!(
            crate::operation_state(root),
            Some(crate::OperationState::CherryPick)
        );
        cherry_pick_abort(root).unwrap();
        assert_eq!(crate::operation_state(root), None);
    }

    /// A conflicted file can be resolved whole: keeping one side rewrites
    /// the work tree and stages it, so the conflict leaves the status.
    #[test]
    fn resolves_a_conflict_by_keeping_one_side() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q", "--initial-branch=main"]).unwrap();
        git(root, &["config", "user.name", "HyperStack Test"]).unwrap();
        git(root, &["config", "user.email", "test@example.invalid"]).unwrap();
        git(root, &["config", "commit.gpgsign", "false"]).unwrap();

        std::fs::write(root.join("shared.txt"), "base\n").unwrap();
        git(root, &["add", "shared.txt"]).unwrap();
        git(root, &["commit", "-q", "-m", "base"]).unwrap();

        git(root, &["checkout", "-q", "-b", "other"]).unwrap();
        std::fs::write(root.join("shared.txt"), "theirs\n").unwrap();
        git(root, &["commit", "-q", "-am", "other side"]).unwrap();

        git(root, &["checkout", "-q", "main"]).unwrap();
        std::fs::write(root.join("shared.txt"), "ours\n").unwrap();
        git(root, &["commit", "-q", "-am", "main side"]).unwrap();

        // The merge stops with a conflict.
        assert!(merge(root, "other", false, false).is_err());
        assert_eq!(
            crate::operation_state(root),
            Some(crate::OperationState::Merge)
        );
        let conflicted = crate::status(root)
            .unwrap()
            .files
            .iter()
            .any(|f| f.kind == crate::Kind::Conflict && f.path == "shared.txt");
        assert!(conflicted);

        // Keeping ours rewrites the file and stages it.
        resolve_conflict(root, "shared.txt", ConflictSide::Ours).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("shared.txt"))
                .unwrap()
                .trim_end(),
            "ours"
        );
        let status = crate::status(root).unwrap();
        assert!(
            !status.files.iter().any(|f| f.kind == crate::Kind::Conflict),
            "the conflict is gone: {:?}",
            status.files
        );

        merge_abort(root).unwrap();
        assert_eq!(crate::operation_state(root), None);
    }
}
