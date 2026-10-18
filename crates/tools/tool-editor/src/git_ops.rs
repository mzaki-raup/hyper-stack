//! The write side of Source Control (docs/tools/git.md §5, §8, §10): the
//! commit box, stage/unstage/discard, stashes, branches, and fetch / pull /
//! push. Every git call runs on the blocking pool through `hs_git::write`;
//! the network ones are also Jobs, so the dock lists them and their output.
//!
//! The guards live here: a discard or a branch delete always confirms with
//! what it would lose, and a push to a protected remote or branch (Settings
//! → Editor → Git) confirms once more. A plain `--force` is never issued.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use hs_core::Services;
use hs_core::settings::Git as GitSettings;
use hs_core::tool::ToastLevel;
use hs_git::write;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

/// What an operation came back with: a line for the toast or the log.
pub(crate) type OpResult = Result<String, String>;

/// The lists a repository shows beside its status.
#[derive(Debug, Clone, Default)]
pub(crate) struct Lists {
    pub stashes: Vec<write::Stash>,
    pub branches: Vec<hs_git::Branch>,
    pub remotes: Vec<write::Remote>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OperationInput {
    pub source: String,
    pub no_commit: bool,
    pub squash: bool,
}

/// A question asked before something is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// Throw away work-tree changes of these tracked files.
    Discard(Vec<String>),
    /// Delete these untracked files.
    Delete(Vec<String>),
    /// Delete a branch; `force` when it is not merged.
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// Push to a protected remote or branch.
    ProtectedPush,
    /// Drop a stash.
    DropStash(write::Stash),
    /// Throw one hunk away in the work tree.
    DiscardHunk {
        path: String,
        hunk: hs_git::Hunk,
    },
    RemoveRemote(String),
    DeleteTag(String),
    PushTag {
        remote: String,
        tag: String,
    },
}

/// Per-repository state of the write side.
pub(crate) struct Ops {
    pub message: String,
    pub amend: bool,
    /// The operation running now, as a label, so the box is disabled.
    pub busy: Option<String>,
    result: Option<mpsc::Receiver<(String, OpResult)>>,
    pub lists: Lists,
    lists_rx: Option<mpsc::Receiver<Lists>>,
    /// The lists are stale: fetched again after the next status.
    pub lists_due: bool,
    pub operation: Option<hs_git::OperationState>,
    operation_rx: Option<mpsc::Receiver<Option<hs_git::OperationState>>>,
    operation_check_due: bool,
    pub confirm: Option<Confirm>,
    /// The New Branch field, while it is open.
    pub new_branch: Option<String>,
    /// The branch being renamed and the new name being edited.
    pub rename_branch: Option<(String, String)>,
    pub remote_name: String,
    pub remote_url: String,
    pub editing_remote: Option<String>,
    /// The New Tag panel, while it is open: name and message.
    pub new_tag: Option<(String, String)>,
    pub merge_input: OperationInput,
    pub rebase_onto: String,
    pub cherry_pick_commit: String,
    pub stash_message: String,
    /// The interactive rebase panel, while it is open.
    pub rebase_ui: Option<RebaseUi>,
}

/// The state of the Interactive Rebase panel: the base the user typed, the
/// commits loaded for it, and the action chosen for each.
pub(crate) struct RebaseUi {
    pub base: String,
    pub commits: Vec<hs_git::write::RebaseCommit>,
    /// Parallel to `commits`.
    pub actions: Vec<hs_git::write::RebaseAction>,
    loading: bool,
    rx: Option<mpsc::Receiver<Result<Vec<hs_git::write::RebaseCommit>, String>>>,
    pub error: Option<String>,
}

impl RebaseUi {
    fn new() -> Self {
        Self {
            base: String::new(),
            commits: Vec::new(),
            actions: Vec::new(),
            loading: false,
            rx: None,
            error: None,
        }
    }
}

/// Conventional Commit prefixes offered next to the commit box
/// (docs/tools/git.md §8). Selecting one prepends it to the message when
/// the message does not already start with a prefix.
const COMMIT_PREFIXES: &[(&str, &str)] = &[
    ("feat", "git-prefix-feat"),
    ("fix", "git-prefix-fix"),
    ("docs", "git-prefix-docs"),
    ("style", "git-prefix-style"),
    ("refactor", "git-prefix-refactor"),
    ("perf", "git-prefix-perf"),
    ("test", "git-prefix-test"),
    ("build", "git-prefix-build"),
    ("ci", "git-prefix-ci"),
    ("chore", "git-prefix-chore"),
];

/// Whether the message already starts with a Conventional Commit type
/// (`feat:`, `fix!:`, …), so a picked prefix is not stacked on top.
fn has_commit_prefix(message: &str) -> bool {
    let text = message.trim_start();
    COMMIT_PREFIXES.iter().any(|(prefix, _)| {
        text.starts_with(&format!("{prefix}:")) || text.starts_with(&format!("{prefix}!:"))
    })
}

impl Default for Ops {
    fn default() -> Self {
        Self {
            message: String::new(),
            amend: false,
            busy: None,
            result: None,
            lists: Lists::default(),
            lists_rx: None,
            lists_due: true,
            operation: None,
            operation_rx: None,
            operation_check_due: true,
            confirm: None,
            new_branch: None,
            rename_branch: None,
            remote_name: String::new(),
            remote_url: String::new(),
            editing_remote: None,
            new_tag: None,
            merge_input: OperationInput::default(),
            rebase_onto: String::new(),
            cherry_pick_commit: String::new(),
            stash_message: String::new(),
            rebase_ui: None,
        }
    }
}

/// What the sidebar asked for, applied by the view after the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Request {
    Stage(Vec<String>),
    StageAll,
    Unstage(Vec<String>),
    UnstageAll,
    /// After the confirmation.
    Discard(Vec<String>),
    Delete(Vec<String>),
    Commit {
        push: bool,
    },
    Fetch,
    Pull,
    Push,
    StashPush,
    StashApply(String),
    StashPop(String),
    StashDrop(String),
    Checkout(String),
    CreateBranch(String),
    RenameBranch {
        old: String,
        new: String,
    },
    SetUpstream {
        remote: String,
        branch: String,
    },
    AddRemote {
        name: String,
        url: String,
    },
    SetRemoteUrl {
        name: String,
        url: String,
    },
    RemoveRemote(String),
    CreateTag {
        name: String,
        /// Empty makes a lightweight tag; otherwise annotated.
        message: String,
    },
    DeleteTag(String),
    PushTag {
        remote: String,
        tag: String,
    },
    Merge {
        source: String,
        no_commit: bool,
        squash: bool,
    },
    MergeContinue,
    MergeAbort,
    Rebase(String),
    RebaseContinue,
    RebaseAbort,
    CherryPick(String),
    CherryPickContinue,
    CherryPickAbort,
    CherryPickSkip,
    /// `git rebase -i` with the todo the panel built.
    RebaseInteractive {
        base: String,
        todo: Vec<(String, hs_git::write::RebaseAction)>,
    },
    /// Resolve one conflicted file by keeping a whole side.
    ResolveConflict {
        path: String,
        side: write::ConflictSide,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    /// One hunk of a file, to the index, out of it, or gone.
    Hunk {
        path: String,
        hunk: hs_git::Hunk,
        op: write::HunkOp,
    },
}

impl Ops {
    /// Start `f` on the blocking pool; `label` names it while it runs.
    pub fn start(
        &mut self,
        s: &Services,
        label: String,
        f: impl FnOnce() -> OpResult + Send + 'static,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.operation_check_due = true;
        let (tx, rx) = mpsc::channel();
        self.busy = Some(label.clone());
        self.result = Some(rx);
        s.runtime.spawn_blocking(move || {
            let _ = tx.send((label, f()));
        });
    }

    /// Start a network operation as a Job as well, so the dock shows it.
    pub fn start_job(
        &mut self,
        s: &Services,
        label: String,
        tool_label: String,
        f: impl FnOnce() -> OpResult + Send + 'static,
    ) {
        if self.busy.is_some() {
            return;
        }
        self.operation_check_due = true;
        let (tx, rx) = mpsc::channel();
        self.busy = Some(label.clone());
        self.result = Some(rx);
        let waker = s.runtime.waker();
        s.jobs.spawn(tool_label, "git", move |h| async move {
            let out = tokio::task::spawn_blocking(f)
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
            match &out {
                Ok(text) => {
                    for line in text.lines() {
                        h.log(line.to_string());
                    }
                }
                Err(e) => h.log(e.clone()),
            }
            let _ = tx.send((label, out.clone()));
            waker.request_repaint();
            out.map(|_| ())
        });
    }

    /// A finished operation, once.
    pub fn take_result(&mut self) -> Option<(String, OpResult)> {
        let rx = self.result.as_ref()?;
        match rx.try_recv() {
            Ok(r) => {
                self.result = None;
                self.busy = None;
                Some(r)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.result = None;
                self.busy = None;
                None
            }
        }
    }

    /// Load the stash, branch and remote lists in the background.
    pub fn refresh_lists(&mut self, s: &Services, root: &Path) {
        if self.lists_rx.is_some() {
            return;
        }
        self.lists_due = false;
        let (tx, rx) = mpsc::channel();
        self.lists_rx = Some(rx);
        let root = root.to_path_buf();
        s.runtime.spawn_blocking(move || {
            let lists = Lists {
                stashes: write::stash_list(&root).unwrap_or_default(),
                branches: hs_git::branches(&root).unwrap_or_default(),
                remotes: write::remotes(&root).unwrap_or_default(),
                tags: write::tags(&root).unwrap_or_default(),
            };
            let _ = tx.send(lists);
        });
    }

    pub fn poll_lists(&mut self) {
        if let Some(rx) = &self.lists_rx {
            match rx.try_recv() {
                Ok(l) => {
                    self.lists = l;
                    self.lists_rx = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => self.lists_rx = None,
            }
        }
    }

    pub fn ensure_lists(&mut self, s: &Services, root: &Path) {
        if self.lists_due && self.lists_rx.is_none() {
            self.refresh_lists(s, root);
        }
    }

    pub fn ensure_operation(&mut self, s: &Services, root: &Path) {
        if !self.operation_check_due && self.operation_rx.is_none() {
            return;
        }
        if self.operation_rx.is_none() {
            let root = root.to_path_buf();
            let (tx, rx) = mpsc::channel();
            self.operation_rx = Some(rx);
            s.runtime.spawn_blocking(move || {
                let _ = tx.send(hs_git::operation_state(&root));
            });
        }
        if let Some(rx) = &self.operation_rx {
            match rx.try_recv() {
                Ok(state) => {
                    self.operation = state;
                    self.operation_rx = None;
                    self.operation_check_due = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.operation_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    /// Whether the current branch, or any remote, matches the protected
    /// patterns (docs/tools/git.md §10).
    pub fn protected(&self, settings: &GitSettings, branch: Option<&str>) -> bool {
        let branch = branch.unwrap_or("");
        self.lists
            .remotes
            .iter()
            .any(|r| write::is_protected(&settings.protected_patterns, &r.url, branch))
            || write::is_protected(&settings.protected_patterns, "", branch)
    }
}

/// Run `req` for the repository at `root`. The result reaches
/// [`Ops::take_result`]; the caller refreshes the status after it.
pub(crate) fn run(ops: &mut Ops, s: &Services, root: &Path, status: &hs_git::Status, req: Request) {
    let root: PathBuf = root.to_path_buf();
    let settings = s.settings.read().git.clone();
    let done = |key: &str| s.t(key);
    match req {
        Request::Stage(paths) => {
            let msg = done("git-op-staged");
            ops.start(s, s.t("git-op-staging"), move || {
                let p: Vec<&str> = paths.iter().map(String::as_str).collect();
                write::stage(&root, &p)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::StageAll => {
            let msg = done("git-op-staged");
            ops.start(s, s.t("git-op-staging"), move || {
                write::stage_all(&root)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Unstage(paths) => {
            let msg = done("git-op-unstaged");
            ops.start(s, s.t("git-op-unstaging"), move || {
                let p: Vec<&str> = paths.iter().map(String::as_str).collect();
                write::unstage(&root, &p)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::UnstageAll => {
            let msg = done("git-op-unstaged");
            ops.start(s, s.t("git-op-unstaging"), move || {
                write::unstage_all(&root)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Discard(paths) => {
            let msg = done("git-op-discarded");
            ops.start(s, s.t("git-op-discarding"), move || {
                let p: Vec<&str> = paths.iter().map(String::as_str).collect();
                write::discard(&root, &p)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Delete(paths) => {
            let msg = done("git-op-deleted");
            ops.start(s, s.t("git-op-deleting"), move || {
                let p: Vec<&str> = paths.iter().map(String::as_str).collect();
                write::remove_untracked(&root, &p)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Commit { push } => {
            let message = ops.message.trim().to_string();
            if message.is_empty() {
                return;
            }
            let opts = write::CommitOptions {
                amend: ops.amend,
                signoff: settings.signoff,
            };
            let committed = done("git-op-committed");
            let pushed = done("git-op-pushed");
            let upstream = push_upstream(status, &ops.lists);
            ops.start(s, s.t("git-op-committing"), move || {
                write::commit(&root, &message, &opts).map_err(|e| e.to_string())?;
                if push {
                    write::push(&root, &push_options(upstream)).map_err(|e| e.to_string())?;
                    return Ok(pushed);
                }
                Ok(committed)
            });
            ops.message.clear();
            ops.amend = false;
        }
        Request::Fetch => {
            let msg = done("git-op-fetched");
            ops.start_job(
                s,
                s.t("git-op-fetching"),
                s.t("git-op-fetching"),
                move || {
                    write::fetch(&root)
                        .map(|o| if o.trim().is_empty() { msg } else { o })
                        .map_err(|e| e.to_string())
                },
            );
        }
        Request::Pull => {
            let rebase = settings.pull_rebase;
            ops.start_job(s, s.t("git-op-pulling"), s.t("git-op-pulling"), move || {
                write::pull(&root, rebase).map_err(|e| e.to_string())
            });
        }
        Request::Push => {
            let upstream = push_upstream(status, &ops.lists);
            let msg = done("git-op-pushed");
            ops.start_job(s, s.t("git-op-pushing"), s.t("git-op-pushing"), move || {
                write::push(&root, &push_options(upstream))
                    .map(|o| if o.trim().is_empty() { msg } else { o })
                    .map_err(|e| e.to_string())
            });
        }
        Request::StashPush => {
            let message = ops.stash_message.trim().to_string();
            let untracked = settings.stash_untracked;
            let msg = done("git-op-stashed");
            ops.stash_message.clear();
            ops.start(s, s.t("git-op-stashing"), move || {
                write::stash_push(&root, &message, untracked)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::StashApply(id) => {
            let msg = done("git-op-stash-applied");
            ops.start(s, s.t("git-op-stash-applying"), move || {
                write::stash_apply(&root, &id)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::StashPop(id) => {
            let msg = done("git-op-stash-applied");
            ops.start(s, s.t("git-op-stash-applying"), move || {
                write::stash_pop(&root, &id)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::StashDrop(id) => {
            let msg = done("git-op-stash-dropped");
            ops.start(s, s.t("git-op-stash-dropping"), move || {
                write::stash_drop(&root, &id)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Checkout(name) => {
            let msg = s.i18n.t_args("git-op-switched", &[("branch", &name)]);
            ops.start(s, s.t("git-op-switching"), move || {
                write::checkout(&root, &name)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::CreateBranch(name) => {
            let msg = s.i18n.t_args("git-op-switched", &[("branch", &name)]);
            ops.start(s, s.t("git-op-switching"), move || {
                write::create_branch(&root, &name, None)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::RenameBranch { old, new } => {
            let msg = s.i18n.t_args("git-op-branch-renamed", &[("branch", &new)]);
            ops.start(s, s.t("git-op-branch-renaming"), move || {
                write::rename_branch(&root, &old, &new)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::SetUpstream { remote, branch } => {
            let upstream = format!("{remote}/{branch}");
            let msg = s
                .i18n
                .t_args("git-op-upstream-set", &[("upstream", &upstream)]);
            ops.start(s, s.t("git-op-upstream-setting"), move || {
                write::set_upstream(&root, &remote, &branch)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::AddRemote { name, url } => {
            let msg = s.i18n.t_args("git-op-remote-added", &[("remote", &name)]);
            ops.start(s, s.t("git-op-remote-adding"), move || {
                write::add_remote(&root, &name, &url)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
            ops.lists_due = true;
        }
        Request::SetRemoteUrl { name, url } => {
            let msg = s.i18n.t_args("git-op-remote-updated", &[("remote", &name)]);
            ops.start(s, s.t("git-op-remote-updating"), move || {
                write::set_remote_url(&root, &name, &url)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
            ops.lists_due = true;
        }
        Request::RemoveRemote(name) => {
            let msg = s.i18n.t_args("git-op-remote-removed", &[("remote", &name)]);
            ops.start(s, s.t("git-op-remote-removing"), move || {
                write::remove_remote(&root, &name)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
            ops.lists_due = true;
        }
        Request::CreateTag { name, message } => {
            let msg = s.i18n.t_args("git-op-tag-created", &[("tag", &name)]);
            ops.start(s, s.t("git-op-tag-creating"), move || {
                write::create_tag(&root, &name, None, &message)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
            ops.lists_due = true;
        }
        Request::DeleteTag(name) => {
            let msg = s.i18n.t_args("git-op-tag-deleted", &[("tag", &name)]);
            ops.start(s, s.t("git-op-tag-deleting"), move || {
                write::delete_tag(&root, &name)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
            ops.lists_due = true;
        }
        Request::PushTag { remote, tag } => {
            let remote_url = ops
                .lists
                .remotes
                .iter()
                .find(|r| r.name == remote)
                .map(|r| r.url.clone())
                .unwrap_or_default();
            if write::is_protected(&settings.protected_patterns, &remote_url, &tag) {
                ops.confirm = Some(Confirm::PushTag { remote, tag });
                return;
            }
            let msg = s
                .i18n
                .t_args("git-op-tag-pushed", &[("tag", &tag), ("remote", &remote)]);
            ops.start_job(
                s,
                s.t("git-op-tag-pushing"),
                s.t("git-op-tag-pushing"),
                move || {
                    write::push_tag(&root, &remote, &tag)
                        .map(|output| {
                            if output.trim().is_empty() {
                                msg
                            } else {
                                output
                            }
                        })
                        .map_err(|e| e.to_string())
                },
            );
            ops.lists_due = true;
        }
        Request::Merge {
            source,
            no_commit,
            squash,
        } => {
            ops.operation_check_due = true;
            ops.start_job(s, s.t("git-op-merging"), s.t("git-op-merging"), move || {
                write::merge(&root, &source, no_commit, squash).map_err(|e| e.to_string())
            });
        }
        Request::MergeContinue => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-continuing"), move || {
                write::merge_continue(&root).map_err(|e| e.to_string())
            });
        }
        Request::MergeAbort => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-aborting"), move || {
                write::merge_abort(&root).map_err(|e| e.to_string())
            });
        }
        Request::Rebase(onto) => {
            ops.operation_check_due = true;
            ops.start_job(
                s,
                s.t("git-op-rebasing"),
                s.t("git-op-rebasing"),
                move || write::rebase(&root, &onto).map_err(|e| e.to_string()),
            );
        }
        Request::RebaseContinue => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-continuing"), move || {
                write::rebase_continue(&root).map_err(|e| e.to_string())
            });
        }
        Request::RebaseAbort => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-aborting"), move || {
                write::rebase_abort(&root).map_err(|e| e.to_string())
            });
        }
        Request::CherryPick(commit) => {
            ops.operation_check_due = true;
            ops.start_job(
                s,
                s.t("git-op-cherry-picking"),
                s.t("git-op-cherry-picking"),
                move || write::cherry_pick(&root, &commit).map_err(|e| e.to_string()),
            );
        }
        Request::CherryPickContinue => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-continuing"), move || {
                write::cherry_pick_continue(&root).map_err(|e| e.to_string())
            });
        }
        Request::CherryPickAbort => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-aborting"), move || {
                write::cherry_pick_abort(&root).map_err(|e| e.to_string())
            });
        }
        Request::CherryPickSkip => {
            ops.operation_check_due = true;
            ops.start(s, s.t("git-op-skipping"), move || {
                write::cherry_pick_skip(&root).map_err(|e| e.to_string())
            });
        }
        Request::RebaseInteractive { base, todo } => {
            ops.operation_check_due = true;
            ops.rebase_ui = None;
            ops.lists_due = true;
            ops.start_job(
                s,
                s.t("git-op-rebasing"),
                s.t("git-op-rebasing"),
                move || write::rebase_interactive(&root, &base, &todo).map_err(|e| e.to_string()),
            );
        }
        Request::ResolveConflict { path, side } => {
            let msg = s
                .i18n
                .t_args("git-op-conflict-resolved", &[("path", &path)]);
            ops.start(s, s.t("git-op-resolving"), move || {
                write::resolve_conflict(&root, &path, side)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::Hunk { path, hunk, op } => {
            let msg = done(match op {
                write::HunkOp::Stage => "git-op-staged",
                write::HunkOp::Unstage => "git-op-unstaged",
                write::HunkOp::Discard => "git-op-discarded",
            });
            ops.start(s, s.t("git-op-staging"), move || {
                write::apply_hunk(&root, &path, &hunk, op)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
        Request::DeleteBranch { name, force } => {
            let msg = s.i18n.t_args("git-op-branch-deleted", &[("branch", &name)]);
            ops.start(s, s.t("git-op-deleting"), move || {
                write::delete_branch(&root, &name, force)
                    .map(|_| msg)
                    .map_err(|e| e.to_string())
            });
        }
    }
    ops.lists_due = true;
}

/// `-u <remote> <branch>` for a branch that has no upstream yet, so the
/// first push of a new branch just works; `None` otherwise.
fn push_upstream(status: &hs_git::Status, lists: &Lists) -> Option<(String, String)> {
    if status.upstream.is_some() {
        return None;
    }
    let branch = status.branch.clone()?;
    let remote = lists
        .remotes
        .iter()
        .find(|r| r.name == "origin")
        .or(lists.remotes.first())?;
    Some((remote.name.clone(), branch))
}

fn push_options(upstream: Option<(String, String)>) -> write::PushOptions {
    write::PushOptions {
        set_upstream: upstream,
        force_with_lease: false,
    }
}

/// The commit box and the operation buttons, under a repository's header.
/// Returns what was asked for; the caller runs it (or asks first).
pub(crate) fn box_ui(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    root: &Path,
    status: &hs_git::Status,
    protected: bool,
) -> Vec<Request> {
    let mut out = Vec::new();
    let enabled = ops.busy.is_none();
    // Fetch / pull / push, stash and branch menus.
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui
                .small_button(ph::ARROWS_CLOCKWISE)
                .tip(s.t("git-fetch"))
                .clicked()
            {
                out.push(Request::Fetch);
            }
            let pull = if status.behind > 0 {
                format!("{} {}", ph::ARROW_DOWN, status.behind)
            } else {
                ph::ARROW_DOWN.to_string()
            };
            if ui.small_button(pull).tip(s.t("git-pull")).clicked() {
                out.push(Request::Pull);
            }
            let push = if status.ahead > 0 {
                format!("{} {}", ph::ARROW_UP, status.ahead)
            } else {
                ph::ARROW_UP.to_string()
            };
            let push_tip = if protected {
                s.t("git-push-protected-tip")
            } else {
                s.t("git-push")
            };
            let r = ui.small_button(push).tip(push_tip);
            if r.clicked() {
                out.push(Request::Push);
            }
            if protected {
                ui.label(egui::RichText::new(ph::SHIELD_WARNING).color(theme.warning))
                    .on_hover_text(s.t("git-protected-tip"));
            }
            stash_menu(ops, ui, s, theme, &mut out);
            branch_menu(ops, ui, s, theme, status, &mut out);
            tag_menu(ops, ui, s, &mut out);
            if let Some(label) = &ops.busy {
                ui.spinner();
                ui.label(egui::RichText::new(label).small().color(theme.text_muted));
            }
        });
    });
    // The New Branch field, when open.
    if let Some(name) = &mut ops.new_branch {
        let mut done = None;
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text(s.t("git-new-branch-hint"))
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
        });
        if let Some(create) = done {
            let name = ops.new_branch.take().unwrap_or_default();
            let name = name.trim().to_string();
            if create && !name.is_empty() {
                out.push(Request::CreateBranch(name));
            }
        }
    }
    if let Some((old, new)) = &mut ops.rename_branch {
        let mut done = None;
        ui.horizontal(|ui| {
            ui.label(
                s.i18n
                    .t_args("git-rename-branch-prompt", &[("branch", old)]),
            );
            let r = ui.add(
                egui::TextEdit::singleline(new)
                    .hint_text(s.t("git-rename-branch-hint"))
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = Some(true);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                done = Some(false);
            }
            if ui.button(s.t("git-rename-branch-apply")).clicked() {
                done = Some(true);
            }
        });
        if let Some(apply) = done {
            let (old, new) = ops.rename_branch.take().unwrap_or_default();
            let new = new.trim().to_string();
            if apply && !new.is_empty() && new != old {
                out.push(Request::RenameBranch { old, new });
            }
        }
    }
    rebase_panel(ops, ui, s, theme, root, &mut out);
    tag_panel(ops, ui, s, theme, &mut out);
    // The commit box.
    let staged = status.files.iter().filter(|f| f.staged()).count();
    ui.add_enabled_ui(enabled, |ui| {
        let r = ui.add(
            egui::TextEdit::multiline(&mut ops.message)
                .hint_text(s.t("git-commit-hint"))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        // Ctrl+Enter commits from the message, as in every git client.
        if r.has_focus()
            && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter))
            && staged > 0
            && !ops.message.trim().is_empty()
        {
            out.push(Request::Commit { push: false });
        }
        // A prefix picker for Conventional Commits, next to the message.
        // Selecting an entry prepends `type:` when the message does not
        // already begin with one (docs/tools/git.md §8).
        ui.horizontal(|ui| {
            let mr = ui.menu_button(
                egui::RichText::new(format!("{} {}", ph::TAG, s.t("git-prefix"))).small(),
                |ui| {
                    ui.set_min_width(180.0);
                    for (prefix, key) in COMMIT_PREFIXES {
                        if ui.button(format!("{prefix}:  {}", s.t(key))).clicked() {
                            if !has_commit_prefix(&ops.message) {
                                ops.message = format!("{prefix}: {}", ops.message.trim_start());
                            }
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button(s.t("git-prefix-clear")).clicked() {
                        if has_commit_prefix(&ops.message) {
                            let text = ops.message.trim_start();
                            if let Some(i) = text.find(':') {
                                ops.message = text[i + 1..].trim_start().to_string();
                            }
                        }
                        ui.close();
                    }
                },
            );
            mr.response.on_hover_text(s.t("git-prefix-tip"));
        });
        ui.horizontal(|ui| {
            let can = staged > 0 && !ops.message.trim().is_empty();
            if ui
                .add_enabled(can, egui::Button::new(s.t("git-commit")))
                .tip(s.t("git-commit-tip"))
                .clicked()
            {
                out.push(Request::Commit { push: false });
            }
            if ui
                .add_enabled(can, egui::Button::new(s.t("git-commit-push")))
                .clicked()
            {
                out.push(Request::Commit { push: true });
            }
            ui.checkbox(&mut ops.amend, s.t("git-amend"));
        });
        if staged == 0 && !status.files.is_empty() {
            ui.label(
                egui::RichText::new(s.t("git-nothing-staged"))
                    .small()
                    .color(theme.text_muted),
            );
        }
    });
    out
}

/// The Interactive Rebase panel: a base, the commits loaded for it, and an
/// action and position per commit. Nothing opens an editor: the engine
/// writes the todo list Git would have asked for.
fn rebase_panel(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    root: &Path,
    out: &mut Vec<Request>,
) {
    let mut close = false;
    let mut start = false;
    {
        let Some(panel) = &mut ops.rebase_ui else {
            return;
        };
        panel.poll();
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("git-rebase-interactive-heading"))
                    .strong()
                    .color(theme.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(s.t("cancel")).clicked() {
                    close = true;
                }
            });
        });
        form_hint_line(ui, theme, &s.t("git-rebase-interactive-hint"));
        // The field takes the row on its own: an infinite width beside a
        // button in a narrow sidebar pushes the button out of sight.
        ui.add(
            egui::TextEdit::singleline(&mut panel.base)
                .hint_text(s.t("git-rebase-base-hint"))
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(!panel.loading, egui::Button::new(s.t("git-rebase-load")))
            .clicked()
        {
            panel.load(s, root);
        }
        if panel.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(s.t("git-rebase-loading"));
            });
        }
        if let Some(error) = &panel.error {
            ui.colored_label(theme.error, error);
        }
        if !panel.commits.is_empty() {
            let mut up: Option<usize> = None;
            let mut down: Option<usize> = None;
            egui::ScrollArea::vertical()
                .id_salt("rebase-commits")
                .max_height(220.0)
                .show(ui, |ui| {
                    for i in 0..panel.commits.len() {
                        let commit = &panel.commits[i];
                        ui.horizontal(|ui| {
                            let first = i == 0;
                            let selected = action_label(panel.actions[i], s);
                            egui::ComboBox::from_id_salt(("rebase-action", i))
                                .width(78.0)
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    for action in [
                                        hs_git::write::RebaseAction::Pick,
                                        hs_git::write::RebaseAction::Squash,
                                        hs_git::write::RebaseAction::Fixup,
                                        hs_git::write::RebaseAction::Drop,
                                    ] {
                                        // Git cannot squash or fix up the
                                        // first entry: there is nothing to
                                        // fold it into.
                                        if first
                                            && matches!(
                                                action,
                                                hs_git::write::RebaseAction::Squash
                                                    | hs_git::write::RebaseAction::Fixup
                                            )
                                        {
                                            continue;
                                        }
                                        ui.selectable_value(
                                            &mut panel.actions[i],
                                            action,
                                            action_label(action, s),
                                        );
                                    }
                                });
                            ui.monospace(&commit.short);
                            ui.add(
                                egui::Label::new(&commit.subject)
                                    .truncate()
                                    .selectable(false),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if i + 1 < panel.commits.len()
                                        && ui
                                            .small_button(ph::ARROW_DOWN)
                                            .tip(s.t("git-rebase-move-down"))
                                            .clicked()
                                    {
                                        down = Some(i);
                                    }
                                    if i > 0
                                        && ui
                                            .small_button(ph::ARROW_UP)
                                            .tip(s.t("git-rebase-move-up"))
                                            .clicked()
                                    {
                                        up = Some(i);
                                    }
                                },
                            );
                        });
                    }
                });
            // Move the clicked row; the actions travel with their commits.
            let swap = |panel: &mut RebaseUi, a: usize, b: usize| {
                panel.commits.swap(a, b);
                panel.actions.swap(a, b);
                if matches!(
                    panel.actions[0],
                    hs_git::write::RebaseAction::Squash | hs_git::write::RebaseAction::Fixup
                ) {
                    panel.actions[0] = hs_git::write::RebaseAction::Pick;
                }
            };
            if let Some(i) = up {
                swap(panel, i, i - 1);
            }
            if let Some(i) = down {
                swap(panel, i, i + 1);
            }
            ui.add_space(4.0);
            let runnable = panel
                .actions
                .iter()
                .any(|a| *a != hs_git::write::RebaseAction::Drop);
            if ui
                .add_enabled(runnable, egui::Button::new(s.t("git-rebase-start")))
                .clicked()
            {
                start = true;
            }
        }
    }
    if start {
        if let Some(panel) = &ops.rebase_ui {
            let base = panel.base.trim().to_string();
            let todo: Vec<(String, hs_git::write::RebaseAction)> = panel
                .commits
                .iter()
                .zip(panel.actions.iter())
                .map(|(commit, action)| (commit.sha.clone(), *action))
                .collect();
            out.push(Request::RebaseInteractive { base, todo });
        }
    } else if close {
        ops.rebase_ui = None;
    }
}

fn action_label(action: hs_git::write::RebaseAction, s: &Services) -> String {
    match action {
        hs_git::write::RebaseAction::Pick => s.t("git-rebase-action-pick"),
        hs_git::write::RebaseAction::Squash => s.t("git-rebase-action-squash"),
        hs_git::write::RebaseAction::Fixup => s.t("git-rebase-action-fixup"),
        hs_git::write::RebaseAction::Drop => s.t("git-rebase-action-drop"),
    }
}

fn form_hint_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme.text_muted));
}

impl RebaseUi {
    /// Load `base..HEAD` on the blocking pool; the answer arrives through
    /// `poll` on a later frame.
    fn load(&mut self, s: &Services, root: &Path) {
        if self.rx.is_some() {
            return;
        }
        let base = self.base.trim().to_string();
        if base.is_empty() {
            self.error = Some(s.t("git-rebase-base-required"));
            return;
        }
        let root = root.to_path_buf();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.loading = true;
        self.error = None;
        self.commits.clear();
        self.actions.clear();
        s.runtime.spawn_blocking(move || {
            let _ = tx.send(hs_git::write::commits_since(&root, &base).map_err(|e| e.to_string()));
        });
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(commits)) => {
                self.actions = vec![hs_git::write::RebaseAction::Pick; commits.len()];
                self.commits = commits;
                self.rx = None;
                self.loading = false;
            }
            Ok(Err(error)) => {
                self.error = Some(error);
                self.rx = None;
                self.loading = false;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                self.loading = false;
            }
        }
    }
}

/// The New Tag panel: a name and an optional message. Creating a tag needs
/// text input, and a text field inside a menu popup closed the menu on the
/// first click, so this follows the inline-panel pattern of rename/rebase.
fn tag_panel(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    out: &mut Vec<Request>,
) {
    let mut close = false;
    let mut create = false;
    {
        let Some((name, message)) = &mut ops.new_tag else {
            return;
        };
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("git-tag-new"))
                    .strong()
                    .color(theme.accent),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(s.t("cancel")).clicked() {
                    close = true;
                }
            });
        });
        ui.add(
            egui::TextEdit::singleline(name)
                .hint_text(s.t("git-tag-name"))
                .desired_width(f32::INFINITY),
        );
        ui.add(
            egui::TextEdit::singleline(message)
                .hint_text(s.t("git-tag-message-hint"))
                .desired_width(f32::INFINITY),
        );
        form_hint_line(ui, theme, &s.t("git-tag-message-tip"));
        if ui
            .add_enabled(
                !name.trim().is_empty(),
                egui::Button::new(s.t("git-tag-create")),
            )
            .clicked()
        {
            create = true;
        }
    }
    if create {
        if let Some((name, message)) = ops.new_tag.take() {
            out.push(Request::CreateTag {
                name: name.trim().to_string(),
                message: message.trim().to_string(),
            });
        }
    } else if close {
        ops.new_tag = None;
    }
}

/// Create a tag (annotated when a message is given) and manage the local
/// tags. A top-level menu rather than a submenu of the branch menu: a nested
/// popup tall enough for the fields was drawn under its parent.
fn tag_menu(ops: &mut Ops, ui: &mut egui::Ui, s: &Services, out: &mut Vec<Request>) {
    let label = if ops.lists.tags.is_empty() {
        ph::TAG.to_string()
    } else {
        format!("{} {}", ph::TAG, ops.lists.tags.len())
    };
    ui.menu_button(label, |ui| {
        ui.set_min_width(280.0);
        ui.set_max_width(320.0);
        if ui
            .button(s.t("git-tag-new"))
            .tip(s.t("git-tag-new-tip"))
            .clicked()
        {
            ops.new_tag = Some((String::new(), String::new()));
            ui.close();
        }
        ui.separator();
        let tags = ops.lists.tags.clone();
        for tag in tags {
            ui.horizontal(|ui| {
                ui.label(&tag);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some(remote) = ops.lists.remotes.first()
                        && ui
                            .small_button(ph::ARROW_UP)
                            .tip(
                                if write::is_protected(
                                    &s.settings.read().git.protected_patterns,
                                    &remote.url,
                                    &tag,
                                ) {
                                    s.t("git-tag-push-protected-tip")
                                } else {
                                    s.t("git-tag-push")
                                },
                            )
                            .clicked()
                    {
                        out.push(Request::PushTag {
                            remote: remote.name.clone(),
                            tag: tag.clone(),
                        });
                        ui.close();
                    }
                    if ui
                        .small_button(ph::TRASH)
                        .tip(s.t("git-tag-delete"))
                        .clicked()
                    {
                        ops.confirm = Some(Confirm::DeleteTag(tag.clone()));
                        ui.close();
                    }
                });
            });
        }
    });
}

fn stash_menu(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    out: &mut Vec<Request>,
) {
    let label = if ops.lists.stashes.is_empty() {
        ph::ARCHIVE.to_string()
    } else {
        format!("{} {}", ph::ARCHIVE, ops.lists.stashes.len())
    };
    ui.menu_button(label, |ui| {
        ui.set_min_width(240.0);
        ui.label(egui::RichText::new(s.t("git-stash-new")).strong());
        let r = ui.add(
            egui::TextEdit::singleline(&mut ops.stash_message)
                .hint_text(s.t("git-stash-hint"))
                .desired_width(f32::INFINITY),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button(s.t("git-stash-push")).clicked() || enter {
            out.push(Request::StashPush);
            ui.close();
        }
        if !ops.lists.stashes.is_empty() {
            ui.separator();
        }
        let stashes = ops.lists.stashes.clone();
        for st in stashes {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&st.message).small())
                        .truncate()
                        .selectable(false),
                )
                .on_hover_text(&st.id);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(ph::TRASH)
                        .tip(s.t("git-stash-drop"))
                        .clicked()
                    {
                        ops.confirm = Some(Confirm::DropStash(st.clone()));
                        ui.close();
                    }
                    if ui
                        .small_button(s.t("git-stash-pop"))
                        .tip(s.t("git-stash-pop-tip"))
                        .clicked()
                    {
                        out.push(Request::StashPop(st.id.clone()));
                        ui.close();
                    }
                    if ui
                        .small_button(s.t("git-stash-apply"))
                        .tip(s.t("git-stash-apply-tip"))
                        .clicked()
                    {
                        out.push(Request::StashApply(st.id.clone()));
                        ui.close();
                    }
                });
            });
        }
        let _ = theme;
    })
    .response
    .tip(s.t("git-stash"));
}

fn branch_menu(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    status: &hs_git::Status,
    out: &mut Vec<Request>,
) {
    ui.menu_button(ph::GIT_BRANCH, |ui| {
        ui.set_min_width(220.0);
        ui.horizontal(|ui| {
            if ui.button(s.t("git-new-branch")).clicked() {
                ops.new_branch = Some(String::new());
                ui.close();
            }
            ui.menu_button(s.t("git-remotes"), |ui| {
                ui.set_min_width(320.0);
                let remotes = ops.lists.remotes.clone();
                for remote in remotes {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&remote.name).strong());
                        let url = if write::is_protected(
                            &s.settings.read().git.protected_patterns,
                            &remote.url,
                            "",
                        ) {
                            s.i18n
                                .t_args("git-remote-url-protected", &[("url", &remote.url)])
                        } else {
                            remote.url.clone()
                        };
                        ui.add(
                            egui::Label::new(egui::RichText::new(url).small())
                                .truncate()
                                .selectable(false),
                        );
                        if ui
                            .small_button(ph::PENCIL_SIMPLE)
                            .tip(s.t("git-remote-edit"))
                            .clicked()
                        {
                            ops.editing_remote = Some(remote.name.clone());
                            ops.remote_name = remote.name.clone();
                            ops.remote_url = remote.url.clone();
                            ui.close();
                        }
                        if ui
                            .small_button(ph::TRASH)
                            .tip(s.t("git-remote-remove"))
                            .clicked()
                        {
                            ops.confirm = Some(Confirm::RemoveRemote(remote.name.clone()));
                            ui.close();
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut ops.remote_name)
                            .hint_text(s.t("git-remote-name")),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut ops.remote_url)
                            .hint_text(s.t("git-remote-url")),
                    );
                    if ui
                        .button(if ops.editing_remote.is_some() {
                            s.t("save")
                        } else {
                            s.t("git-remote-add")
                        })
                        .clicked()
                        && !ops.remote_name.trim().is_empty()
                        && !ops.remote_url.trim().is_empty()
                    {
                        if ops.editing_remote.is_some() {
                            out.push(Request::SetRemoteUrl {
                                name: ops.remote_name.trim().to_string(),
                                url: ops.remote_url.trim().to_string(),
                            });
                        } else {
                            out.push(Request::AddRemote {
                                name: ops.remote_name.trim().to_string(),
                                url: ops.remote_url.trim().to_string(),
                            });
                        }
                        ops.editing_remote = None;
                        ops.remote_name.clear();
                        ops.remote_url.clear();
                        ui.close();
                    }
                    if ops.editing_remote.is_some() && ui.small_button(s.t("cancel")).clicked() {
                        ops.editing_remote = None;
                        ops.remote_name.clear();
                        ops.remote_url.clear();
                        ui.close();
                    }
                });
            });
            ui.menu_button(s.t("git-merge-rebase"), |ui| {
                ui.set_min_width(260.0);
                if let Some(operation) = ops.operation {
                    let label = match operation {
                        hs_git::OperationState::Merge => s.t("git-operation-merge-conflict"),
                        hs_git::OperationState::Rebase => s.t("git-operation-rebase-conflict"),
                        hs_git::OperationState::CherryPick => {
                            s.t("git-operation-cherry-pick-conflict")
                        }
                    };
                    ui.label(egui::RichText::new(label).color(theme.warning));
                    match operation {
                        hs_git::OperationState::Merge => {
                            if ui.button(s.t("git-continue")).clicked() {
                                out.push(Request::MergeContinue);
                                ui.close();
                            }
                            if ui.button(s.t("git-abort")).clicked() {
                                out.push(Request::MergeAbort);
                                ui.close();
                            }
                        }
                        hs_git::OperationState::Rebase => {
                            if ui.button(s.t("git-continue")).clicked() {
                                out.push(Request::RebaseContinue);
                                ui.close();
                            }
                            if ui.button(s.t("git-abort")).clicked() {
                                out.push(Request::RebaseAbort);
                                ui.close();
                            }
                        }
                        hs_git::OperationState::CherryPick => {
                            if ui.button(s.t("git-continue")).clicked() {
                                out.push(Request::CherryPickContinue);
                                ui.close();
                            }
                            if ui.button(s.t("git-skip")).clicked() {
                                out.push(Request::CherryPickSkip);
                                ui.close();
                            }
                            if ui.button(s.t("git-abort")).clicked() {
                                out.push(Request::CherryPickAbort);
                                ui.close();
                            }
                        }
                    }
                    ui.separator();
                }
                if ops.operation.is_none() {
                    ui.label(egui::RichText::new(s.t("git-merge-heading")).strong());
                }
                ui.add(
                    egui::TextEdit::singleline(&mut ops.merge_input.source)
                        .hint_text(s.t("git-merge-source")),
                );
                ui.checkbox(&mut ops.merge_input.no_commit, s.t("git-merge-no-commit"));
                ui.checkbox(&mut ops.merge_input.squash, s.t("git-merge-squash"));
                if ui.button(s.t("git-merge-run")).clicked()
                    && !ops.merge_input.source.trim().is_empty()
                {
                    out.push(Request::Merge {
                        source: ops.merge_input.source.trim().to_string(),
                        no_commit: ops.merge_input.no_commit,
                        squash: ops.merge_input.squash,
                    });
                    ops.merge_input = OperationInput::default();
                    ui.close();
                }
                ui.separator();
                ui.label(egui::RichText::new(s.t("git-rebase-heading")).strong());
                ui.add(
                    egui::TextEdit::singleline(&mut ops.rebase_onto)
                        .hint_text(s.t("git-rebase-onto")),
                );
                if ui.button(s.t("git-rebase-run")).clicked() && !ops.rebase_onto.trim().is_empty()
                {
                    out.push(Request::Rebase(ops.rebase_onto.trim().to_string()));
                    ops.rebase_onto.clear();
                    ui.close();
                }
                if ui
                    .button(s.t("git-rebase-interactive-open"))
                    .tip(s.t("git-rebase-interactive-tip"))
                    .clicked()
                {
                    let mut panel = RebaseUi::new();
                    // The upstream is the usual base; otherwise the last
                    // commit's parent. The user can always change it.
                    panel.base = status
                        .upstream
                        .clone()
                        .unwrap_or_else(|| "HEAD~1".to_string());
                    ops.rebase_ui = Some(panel);
                    ui.close();
                }
                ui.separator();
                ui.label(egui::RichText::new(s.t("git-cherry-pick-heading")).strong());
                ui.add(
                    egui::TextEdit::singleline(&mut ops.cherry_pick_commit)
                        .hint_text(s.t("git-cherry-pick-commit")),
                );
                if ui.button(s.t("git-cherry-pick-run")).clicked()
                    && !ops.cherry_pick_commit.trim().is_empty()
                {
                    out.push(Request::CherryPick(
                        ops.cherry_pick_commit.trim().to_string(),
                    ));
                    ops.cherry_pick_commit.clear();
                    ui.close();
                }
            });
        });
        if let Some(operation) = ops.operation {
            let label = match operation {
                hs_git::OperationState::Merge => s.t("git-operation-merge-conflict"),
                hs_git::OperationState::Rebase => s.t("git-operation-rebase-conflict"),
                hs_git::OperationState::CherryPick => s.t("git-operation-cherry-pick-conflict"),
            };
            ui.colored_label(theme.warning, label);
        }
        ui.separator();
        let current = status.branch.clone();
        let branches = ops.lists.branches.clone();
        for b in branches.iter().filter(|b| !b.remote) {
            let is_current = current.as_deref() == Some(b.name.as_str());
            ui.horizontal(|ui| {
                let text = if is_current {
                    egui::RichText::new(format!("{} {}", ph::CHECK, b.name)).strong()
                } else {
                    egui::RichText::new(format!("   {}", b.name))
                };
                if ui.add(egui::Button::new(text).frame(false)).clicked() && !is_current {
                    out.push(Request::Checkout(b.name.clone()));
                    ui.close();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !is_current
                        && ui
                            .small_button(ph::TRASH)
                            .tip(s.t("git-delete-branch"))
                            .clicked()
                    {
                        ops.confirm = Some(Confirm::DeleteBranch {
                            name: b.name.clone(),
                            force: false,
                        });
                        ui.close();
                    }
                    if ui
                        .small_button(ph::PENCIL_SIMPLE)
                        .tip(s.t("git-rename-branch"))
                        .clicked()
                    {
                        ops.rename_branch = Some((b.name.clone(), b.name.clone()));
                        ui.close();
                    }
                });
            });
        }
        if let Some(current) = current
            && let Some(branch) = branches.iter().find(|b| !b.remote && b.name == current)
            && !branch.remote
            && !branches
                .iter()
                .any(|b| b.remote && b.name.ends_with(&format!("/{current}")))
        {
            ui.separator();
            ui.label(
                egui::RichText::new(s.t("git-no-upstream"))
                    .small()
                    .color(theme.text_muted),
            );
            for remote in &ops.lists.remotes {
                if ui
                    .button(format!("{} {}/{}", ph::LINK, remote.name, current))
                    .clicked()
                {
                    out.push(Request::SetUpstream {
                        remote: remote.name.clone(),
                        branch: current.clone(),
                    });
                    ui.close();
                }
            }
        }
        let remotes: Vec<&hs_git::Branch> = branches.iter().filter(|b| b.remote).collect();
        if !remotes.is_empty() {
            ui.separator();
            ui.label(
                egui::RichText::new(s.t("git-remote-branches"))
                    .small()
                    .color(theme.text_muted),
            );
            for b in remotes {
                ui.label(egui::RichText::new(&b.name).small());
            }
        }
    })
    .response
    .tip(s.t("git-branches"));
}

/// The confirmation modal. Returns the request to run once confirmed.
pub(crate) fn confirm_ui(
    ops: &mut Ops,
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    pending_push: &mut Option<Request>,
) -> Option<Request> {
    let confirm = ops.confirm.clone()?;
    let mut answer: Option<bool> = None;
    let (title, body, danger) = match &confirm {
        Confirm::Discard(paths) => (
            s.t("git-discard-title"),
            s.i18n
                .t_args("git-discard-body", &[("n", &paths.len().to_string())]),
            true,
        ),
        Confirm::Delete(paths) => (
            s.t("git-delete-title"),
            s.i18n
                .t_args("git-delete-body", &[("n", &paths.len().to_string())]),
            true,
        ),
        Confirm::DeleteBranch { name, force } => (
            s.t("git-delete-branch-title"),
            s.i18n.t_args(
                if *force {
                    "git-delete-branch-force-body"
                } else {
                    "git-delete-branch-body"
                },
                &[("branch", name)],
            ),
            true,
        ),
        Confirm::ProtectedPush => (
            s.t("git-protected-push-title"),
            s.t("git-protected-push-body"),
            false,
        ),
        Confirm::DropStash(st) => (
            s.t("git-stash-drop-title"),
            s.i18n
                .t_args("git-stash-drop-body", &[("message", &st.message)]),
            true,
        ),
        Confirm::RemoveRemote(remote) => (
            s.t("git-remote-remove-title"),
            s.i18n
                .t_args("git-remote-remove-body", &[("remote", remote)]),
            true,
        ),
        Confirm::DeleteTag(tag) => (
            s.t("git-tag-delete-title"),
            s.i18n.t_args("git-tag-delete-body", &[("tag", tag)]),
            true,
        ),
        Confirm::PushTag { remote, tag } => (
            s.t("git-protected-push-title"),
            s.i18n.t_args(
                "git-protected-tag-push-body",
                &[("tag", tag), ("remote", remote)],
            ),
            false,
        ),
        Confirm::DiscardHunk { hunk, .. } => (
            s.t("git-discard-hunk-title"),
            s.i18n
                .t_args("git-discard-hunk-body", &[("hunk", &hunk.header)]),
            true,
        ),
    };
    egui::Modal::new(egui::Id::new("git-confirm")).show(ui.ctx(), |ui| {
        ui.set_min_width(360.0);
        ui.label(egui::RichText::new(title).strong().size(16.0));
        ui.add_space(6.0);
        ui.label(body);
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let yes = if danger {
                hs_ui::form::danger_button(ui, theme, &s.t("git-confirm-yes"))
            } else {
                hs_ui::form::primary_button(ui, theme, &s.t("git-confirm-yes"))
            };
            if yes.clicked() {
                answer = Some(true);
            }
            if ui.button(s.t("git-confirm-no")).clicked() {
                answer = Some(false);
            }
        });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            answer = Some(false);
        }
    });
    let yes = answer?;
    ops.confirm = None;
    if !yes {
        pending_push.take();
        return None;
    }
    Some(match confirm {
        Confirm::Discard(paths) => Request::Discard(paths),
        Confirm::Delete(paths) => Request::Delete(paths),
        Confirm::DeleteBranch { name, force } => Request::DeleteBranch { name, force },
        Confirm::ProtectedPush => pending_push.take()?,
        Confirm::DropStash(st) => Request::StashDrop(st.id),
        Confirm::RemoveRemote(remote) => Request::RemoveRemote(remote),
        Confirm::DeleteTag(tag) => Request::DeleteTag(tag),
        Confirm::PushTag { remote, tag } => Request::PushTag { remote, tag },
        Confirm::DiscardHunk { path, hunk } => Request::Hunk {
            path,
            hunk,
            op: write::HunkOp::Discard,
        },
    })
}

/// The toast level a result reads as.
pub(crate) fn level(r: &OpResult) -> ToastLevel {
    match r {
        Ok(_) => ToastLevel::Success,
        Err(_) => ToastLevel::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conventional_prefix_is_recognised_once() {
        assert!(has_commit_prefix("feat: add a thing"));
        assert!(has_commit_prefix("fix!: breaking fix"));
        assert!(has_commit_prefix("  chore: tidy"));
        assert!(!has_commit_prefix("add a thing"));
        // A word that merely starts with a type is not a prefix.
        assert!(!has_commit_prefix("feature request"));
        assert!(!has_commit_prefix("fixture: not a type"));
    }
}
