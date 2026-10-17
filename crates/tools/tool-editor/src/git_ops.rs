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
}

/// A question asked before something is lost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Confirm {
    /// Throw away work-tree changes of these tracked files.
    Discard(Vec<String>),
    /// Delete these untracked files.
    Delete(Vec<String>),
    /// Delete a branch; `force` when it is not merged.
    DeleteBranch { name: String, force: bool },
    /// Push to a protected remote or branch.
    ProtectedPush,
    /// Drop a stash.
    DropStash(write::Stash),
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
    pub confirm: Option<Confirm>,
    /// The New Branch field, while it is open.
    pub new_branch: Option<String>,
    pub stash_message: String,
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
            confirm: None,
            new_branch: None,
            stash_message: String::new(),
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
    DeleteBranch {
        name: String,
        force: bool,
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
        if ui.button(s.t("git-new-branch")).clicked() {
            ops.new_branch = Some(String::new());
            ui.close();
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
                if !is_current {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
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
                    });
                }
            });
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
    })
}

/// The toast level a result reads as.
pub(crate) fn level(r: &OpResult) -> ToastLevel {
    match r {
        Ok(_) => ToastLevel::Success,
        Err(_) => ToastLevel::Error,
    }
}
