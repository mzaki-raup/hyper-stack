//! One repository's work pane (docs/tools/git-activity.md §5): the
//! SourceTree arrangement - a tree on the left (Workspace, Branches,
//! Remotes, Tags, Stashes), File Status or History in the centre, and
//! the diff, blame or conflict editor on the right - under a toolbar
//! with the branch, Refresh, Fetch, Pull, Push and Terminal.
//!
//! Every call into git runs on the blocking pool and lands in a slot the
//! next frame reads - the UI thread never waits on a process (ADR-013).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hs_core::services::Services;
use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_git::{ChangedFile, FileDiff, FileStatus, Kind, Side, Status};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use crate::blame::BlameView;
use crate::conflict::ConflictView;
use crate::diffview::{self, HunkPick};
use crate::history::{self, HistoryView};
use crate::tree::{self, Tree, View};
use crate::verbs::{Facts, Op, Verbs};

/// A result on its way back from the blocking pool.
type Slot<T> = Arc<Mutex<Option<T>>>;

/// How often the status is re-read while the tab is on screen and idle:
/// often enough that an edit in the editor shows up, rare enough that
/// `git status` is not what the machine spends its time on.
const REFRESH_SECONDS: f64 = 4.0;

/// Something the pane asks before doing.
enum Confirm {
    Discard(Vec<String>),
    /// A guarded operation: the question, then the verb.
    Run {
        text: String,
        op: Op,
    },
}

/// What the right column shows.
#[derive(Default)]
enum Right {
    #[default]
    Nothing,
    /// A work-tree or index diff of a file in File Status.
    Work {
        path: String,
        side: Side,
    },
    /// One file of a commit, from History.
    Commit {
        id: String,
        file: ChangedFile,
    },
    Blame,
    Conflict,
}

#[derive(Default)]
pub struct RepoPane {
    root: Option<PathBuf>,
    status: Option<Status>,
    /// A merge, rebase or cherry-pick that git has paused on.
    op_state: Option<hs_git::OperationState>,
    op_state_slot: Slot<Option<hs_git::OperationState>>,
    verbs: Verbs,
    /// The saved credential the network verbs hand to git, by id.
    credential: String,
    /// The AI Assistant column (docs/tools/git-activity.md §7).
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// What the assistant is told about the repository, refreshed with
    /// the status: short status, bounded staged diff, recent log.
    ai_context: String,
    ai_context_slot: Slot<String>,
    /// The commit box text an answer's Insert filled in, applied on the
    /// next frame the box is drawn.
    message_from_ai: Option<String>,
    status_error: Option<String>,
    status_slot: Slot<Result<Status, String>>,
    remote_slot: Slot<String>,
    remote_url: String,
    /// The operation in flight, by label; one at a time, as on the
    /// command line.
    busy: Option<String>,
    /// Whether the operation in flight deserves a toast when it ends:
    /// a push does, a stage does not - the list changing is the answer.
    notify: bool,
    op_slot: Slot<(String, Result<String, String>)>,
    /// What the last operation printed, shown under the toolbar until
    /// the next one.
    output: Option<String>,
    right: Right,
    diff: Option<FileDiff>,
    /// (path, Some(side) for a work diff / None for a commit's, result).
    diff_slot: Slot<(String, Option<Side>, Result<FileDiff, String>)>,
    message: String,
    amend: bool,
    push_after: bool,
    filter: String,
    last_refresh: f64,
    confirm: Option<Confirm>,
    tree: Tree,
    history: HistoryView,
    blame: BlameView,
    conflict: ConflictView,
}

impl RepoPane {
    pub fn set_root(&mut self, s: &Services, root: PathBuf) {
        if self.root.as_deref() == Some(root.as_path()) {
            return;
        }
        *self = Self::default();
        self.tree.set_root(root.clone());
        self.history.set_root(root.clone());
        self.verbs
            .set_root(root.clone(), s.settings.read().git.stash_untracked);
        self.root = Some(root);
        self.refresh(s);
        self.tree.refresh(s);
    }

    pub fn set_credential(&mut self, id: &str) {
        self.credential = id.to_string();
    }

    /// The credential for a network verb, out of the vault; a toast and
    /// `None` when the vault is locked or the reference is gone.
    fn auth(&self, cx: &mut ToolContext) -> Option<Option<crate::auth::Resolved>> {
        match crate::auth::resolve(cx.services, &self.credential) {
            Ok(r) => Some(r),
            Err(e) => {
                cx.toast(ToastLevel::Error, e);
                None
            }
        }
    }

    /// The `origin` URL, read once per root; what the header band shows.
    pub fn remote_url(&self) -> String {
        self.remote_url.clone()
    }

    fn refresh(&mut self, s: &Services) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let slot = self.status_slot.clone();
        let remote = self.remote_slot.clone();
        let op_state = self.op_state_slot.clone();
        let ai_context = self.assistant.open.then(|| self.ai_context_slot.clone());
        s.runtime.spawn_blocking(move || {
            let st = hs_git::status(&root).map_err(|e| e.to_string());
            *slot.lock() = Some(st);
            *op_state.lock() = Some(hs_git::operation_state(&root));
            // What the assistant sees, only while its column is open: a
            // bounded picture, never the whole tree.
            if let Some(ctx) = ai_context {
                *ctx.lock() = Some(assistant_context(&root));
            }
            if let Ok(rs) = hs_git::write::remotes(&root) {
                let url = rs
                    .iter()
                    .find(|r| r.name == "origin")
                    .or(rs.first())
                    .map(|r| r.url.clone())
                    .unwrap_or_default();
                *remote.lock() = Some(url);
            }
        });
    }

    /// Everything that can have changed after an operation: the status,
    /// the refs in the tree, and the graph.
    fn refresh_all(&mut self, s: &Services) {
        self.refresh(s);
        self.tree.refresh(s);
        self.history.stale = true;
    }

    /// Start `f` on the blocking pool; `label` names it while it runs
    /// and in the toast at the end.
    fn run(
        &mut self,
        s: &Services,
        label: String,
        f: impl FnOnce(&Path) -> Result<String, String> + Send + 'static,
    ) {
        self.start(s, label, true, f);
    }

    /// [`run`] for the quick verbs (stage, unstage): no toast, the list
    /// changing under the pointer is the confirmation.
    fn run_quiet(
        &mut self,
        s: &Services,
        label: String,
        f: impl FnOnce(&Path) -> Result<String, String> + Send + 'static,
    ) {
        self.start(s, label, false, f);
    }

    fn start(
        &mut self,
        s: &Services,
        label: String,
        notify: bool,
        f: impl FnOnce(&Path) -> Result<String, String> + Send + 'static,
    ) {
        if self.busy.is_some() {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        self.busy = Some(label.clone());
        self.notify = notify;
        let slot = self.op_slot.clone();
        s.runtime.spawn_blocking(move || {
            let out = f(&root);
            *slot.lock() = Some((label, out));
        });
    }

    /// Run a verb's operation.
    fn exec(&mut self, s: &Services, op: Op) {
        let Op { label, notify, run } = op;
        self.start(s, label, notify, run);
    }

    /// Run `op`, or ask `question` first when there is one (a delete, a
    /// push onto a protected branch).
    fn guarded(&mut self, s: &Services, question: Option<String>, op: Op) {
        match question {
            Some(text) => self.confirm = Some(Confirm::Run { text, op }),
            None => self.exec(s, op),
        }
    }

    /// Whether the current branch or the remote is protected
    /// (Settings -> Git -> Protected Patterns).
    fn protected(&self, s: &Services, branch: &str) -> bool {
        let patterns = s.settings.read().git.protected_patterns.clone();
        hs_git::write::is_protected(&patterns, &self.remote_url, branch)
    }

    fn load_work_diff(&mut self, s: &Services, path: String, side: Side) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.right = Right::Work {
            path: path.clone(),
            side,
        };
        let slot = self.diff_slot.clone();
        s.runtime.spawn_blocking(move || {
            let d = hs_git::file_diff(&root, &path, side).map_err(|e| e.to_string());
            *slot.lock() = Some((path, Some(side), d));
        });
    }

    fn load_commit_diff(
        &mut self,
        s: &Services,
        id: String,
        first_parent: Option<String>,
        file: ChangedFile,
    ) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.right = Right::Commit {
            id: id.clone(),
            file: file.clone(),
        };
        self.diff = None;
        let slot = self.diff_slot.clone();
        s.runtime.spawn_blocking(move || {
            let d = hs_git::commit_file_diff(&root, &id, first_parent.as_deref(), &file)
                .map_err(|e| e.to_string());
            *slot.lock() = Some((file.path, None, d));
        });
    }

    /// The right column's current work-diff selection, if that is what
    /// it shows.
    fn work_selection(&self) -> Option<(String, Side)> {
        match &self.right {
            Right::Work { path, side } => Some((path.clone(), *side)),
            _ => None,
        }
    }

    fn poll(&mut self, cx: &mut ToolContext, now: f64) {
        let st = self.status_slot.lock().take();
        if let Some(st) = st {
            match st {
                Ok(st) => {
                    // The selected file may have left the list (staged,
                    // discarded, committed); the diff goes with it.
                    if let Some((p, side)) = self.work_selection()
                        && !st
                            .files
                            .iter()
                            .any(|f| f.path == p && side_of(f).contains(&side))
                    {
                        self.right = Right::Nothing;
                        self.diff = None;
                    }
                    self.status = Some(st);
                    self.status_error = None;
                }
                Err(e) => self.status_error = Some(e),
            }
            self.last_refresh = now;
        }
        if let Some(url) = self.remote_slot.lock().take() {
            self.remote_url = url;
        }
        if let Some(st) = self.op_state_slot.lock().take() {
            self.op_state = st;
        }
        if let Some(c) = self.ai_context_slot.lock().take() {
            self.ai_context = c;
        }
        let d = self.diff_slot.lock().take();
        if let Some((path, side, d)) = d {
            let wanted = match (&self.right, side) {
                (Right::Work { path: p, side: s }, Some(side)) => *p == path && *s == side,
                (Right::Commit { file, .. }, None) => file.path == path,
                _ => false,
            };
            if wanted {
                match d {
                    Ok(d) => self.diff = Some(d),
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
            }
        }
        let finished = self.op_slot.lock().take();
        if let Some((label, out)) = finished {
            self.busy = None;
            match out {
                Ok(text) => {
                    let text = text.trim().to_string();
                    self.output = (!text.is_empty()).then_some(text);
                    if self.notify {
                        cx.toast(
                            ToastLevel::Success,
                            cx.services
                                .i18n
                                .t_args("git-op-done", &[("op", label.as_str())]),
                        );
                    }
                }
                Err(e) => {
                    self.output = Some(e.clone());
                    cx.toast(ToastLevel::Error, e);
                }
            }
            self.refresh_all(cx.services);
            if let Some((p, side)) = self.work_selection() {
                self.load_work_diff(cx.services, p, side);
            }
        }
        if self.busy.is_none()
            && self.status_slot.lock().is_none()
            && now - self.last_refresh > REFRESH_SECONDS
        {
            self.last_refresh = now;
            self.refresh(cx.services);
        }
    }

    pub fn toolbar(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let busy = self.busy.is_some();
        // The branch, first: it is the one fact a person checks before
        // any of the verbs beside it.
        if let Some(st) = &self.status {
            ui.label(
                egui::RichText::new(format!("{} {}", ph::GIT_BRANCH, st.branch_label()))
                    .strong()
                    .color(theme.accent),
            );
            ui.separator();
        }
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(format!(
                    "{} {}",
                    ph::ARROWS_CLOCKWISE,
                    s.t("git-refresh-button")
                )),
            )
            .clicked()
        {
            self.refresh_all(s);
        }
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(format!(
                    "{} {}",
                    ph::CLOUD_ARROW_DOWN,
                    s.t("git-fetch-button")
                )),
            )
            .clicked()
            && let Some(auth) = self.auth(cx)
        {
            self.run(s, s.t("git-fetch-button"), move |r| {
                crate::auth::with_env(auth.as_ref(), |env| {
                    hs_git::write::fetch_env(r, env).map_err(|e| e.to_string())
                })?
            });
        }
        let rebase = s.settings.read().git.pull_rebase;
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("git-pull"))),
            )
            .clicked()
            && let Some(auth) = self.auth(cx)
        {
            self.run(s, s.t("git-pull"), move |r| {
                crate::auth::with_env(auth.as_ref(), |env| {
                    hs_git::write::pull_env(r, rebase, env).map_err(|e| e.to_string())
                })?
            });
        }
        let upstream = self.status.as_ref().and_then(|st| st.upstream.clone());
        let branch = self.status.as_ref().and_then(|st| st.branch.clone());
        let push_label = if upstream.is_none() && branch.is_some() {
            s.t("git-push-set-upstream")
        } else {
            s.t("git-push")
        };
        if ui
            .add_enabled(
                !busy,
                egui::Button::new(format!("{} {}", ph::UPLOAD_SIMPLE, push_label)),
            )
            .clicked()
        {
            let set_upstream = match (&upstream, &branch) {
                (None, Some(b)) => Some(("origin".to_string(), b.clone())),
                _ => None,
            };
            let question = branch.as_deref().filter(|b| self.protected(s, b)).map(|b| {
                s.i18n
                    .t_args("git-confirm-protected-push", &[("branch", b)])
            });
            if let Some(auth) = self.auth(cx) {
                let op = Op::new(s.t("git-push"), move |r| {
                    crate::auth::with_env(auth.as_ref(), |env| {
                        hs_git::write::push_env(
                            r,
                            &hs_git::write::PushOptions {
                                set_upstream,
                                force_with_lease: false,
                            },
                            env,
                        )
                        .map_err(|e| e.to_string())
                    })?
                });
                self.guarded(s, question, op);
            }
        }
        ui.separator();
        self.verbs.toolbar(ui, s, busy);
        ui.separator();
        if ui
            .button(format!("{} {}", ph::TERMINAL_WINDOW, s.t("git-terminal")))
            .clicked()
            && let Some(root) = &self.root
        {
            let profile = s.settings.read().git.terminal_profile.clone();
            cx.open_new_tab(
                "terminal",
                Some(OpenRequest::Terminal {
                    cwd: Some(root.to_string_lossy().to_string()),
                    host: None,
                    profile: (!profile.is_empty()).then_some(profile),
                    command: None,
                }),
            );
        }
        if let Some(label) = &self.busy {
            ui.separator();
            ui.spinner();
            ui.label(egui::RichText::new(label).color(theme.text_muted));
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The same control in every tool, in the same place.
            if ui
                .selectable_label(
                    self.assistant.open,
                    format!("{} {}", ph::SPARKLE, s.t("ai-assistant")),
                )
                .on_hover_text(s.t("git-ai-tip"))
                .clicked()
            {
                self.assistant.toggle("git");
                if self.assistant.open {
                    self.refresh(s);
                }
            }
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let now = ui.input(|i| i.time);
        self.poll(cx, now);
        if self.busy.is_some() || self.status_slot.lock().is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SECONDS));
        }
        if let Some(e) = &self.status_error {
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                hs_ui::form::note(ui, theme, ph::WARNING, e);
            });
            return;
        }
        if let Some(out) = self.output.clone() {
            egui::Frame::new()
                .fill(theme.surface)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&out)
                                .monospace()
                                .size(12.0)
                                .color(theme.text_muted),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button(ph::X).clicked() {
                                self.output = None;
                            }
                        });
                    });
                });
        }
        self.confirm_bar(ui, theme, cx);
        self.operation_bar(ui, theme, cx);
        let facts_branches = self.tree.refs.branches.clone();
        let facts_remotes = self.tree.refs.remotes.clone();
        let current = self.status.as_ref().and_then(|st| st.branch.clone());
        let facts = Facts {
            branches: &facts_branches,
            remotes: &facts_remotes,
            current: current.as_deref(),
        };
        let busy = self.busy.is_some();
        if let Some(op) = self.verbs.panel_ui(ui, theme, cx.services, &facts, busy) {
            self.exec(cx.services, op);
        }
        let (mut main, panel) = hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        // The column is drawn into the *outer* ui: the carved-off child's
        // clip rectangle excludes it, and a child made from that child
        // clipped the whole assistant away (seen on screen).
        let outer = ui;
        let ui = &mut main;
        let full = ui.available_width();
        // The tree, narrow and resizable.
        let mut tree_pick: Option<tree::Pick> = None;
        egui::Panel::left(ui.id().with("git-tree-col"))
            .resizable(true)
            .default_size(220.0)
            .size_range(160.0..=360.0)
            .show(ui, |ui| {
                tree_pick = self.tree.ui(ui, theme, cx);
            });
        if let Some(p) = tree_pick {
            self.tree_pick(cx.services, p);
        }
        let rest = (full - 220.0).max(400.0);
        let centre_w = match self.tree.view {
            View::Status => (rest * 0.40).clamp(360.0, 620.0),
            View::History => (rest * 0.55).clamp(420.0, 900.0),
        };
        egui::Panel::left(
            ui.id()
                .with(("git-centre-col", self.tree.view == View::History)),
        )
        .resizable(true)
        .default_size(centre_w)
        .size_range(320.0..=(rest - 240.0).max(320.0))
        .show(ui, |ui| match self.tree.view {
            View::Status => self.status_column(ui, theme, cx),
            View::History => self.history_column(ui, theme, cx),
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme.panel_bg))
            .show(ui, |ui| {
                self.right_column(ui, theme, cx);
            });
        if let Some(panel) = panel {
            self.assistant_column(outer, panel, theme, cx);
        }
    }

    /// The assistant's column (docs/tools/git-activity.md §7): the
    /// repository as context, git prompt buttons, and answers whose
    /// command blocks can be run in a terminal in the repository root.
    fn assistant_column(
        &mut self,
        ui: &mut egui::Ui,
        panel: egui::Rect,
        theme: &Theme,
        cx: &mut ToolContext,
    ) {
        let s = cx.services;
        let name = self
            .root
            .as_ref()
            .and_then(|r| r.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        let branch = self
            .status
            .as_ref()
            .map(|st| st.branch_label())
            .unwrap_or_default();
        let upstream = self
            .status
            .as_ref()
            .and_then(|st| st.upstream.clone())
            .unwrap_or_default();
        let context = format!(
            "Repository: {name}\nBranch: {branch}\nUpstream: {upstream}\nRemote: {}\n\n{}",
            self.remote_url, self.ai_context
        );
        self.assistant.set_context(s.t("git-ai-context"), context);
        self.assistant.extra_system = s.t("git-ai-system");
        self.assistant.quick_actions = hs_ui::ai_assistant::QuickAction::for_activity(s, "git");
        self.assistant.can_insert = true;
        self.assistant.can_run = self.root.is_some();
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(panel)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(panel.intersect(ui.clip_rect()));
        let mut out = hs_ui::ai_assistant::AssistantOutput::default();
        egui::Frame::new()
            .fill(theme.panel_bg)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(&mut child, |ui| {
                ui.set_min_width(370.0);
                out = self.assistant.ui(ui, s, theme, "git");
            });
        let Some(block) = out.use_block else {
            return;
        };
        let lang = out.use_block_lang.trim().to_ascii_lowercase();
        let is_command = matches!(
            lang.as_str(),
            "bash" | "sh" | "shell" | "git" | "console" | "zsh" | "powershell" | "pwsh" | "cmd"
        );
        if is_command {
            let command = block.trim().to_string();
            if out.run {
                // Insert and Run: a terminal in the repository root
                // runs it - the activity never spawns a shell of its own.
                if let Some(root) = &self.root {
                    let profile = s.settings.read().git.terminal_profile.clone();
                    cx.open_new_tab(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd: Some(root.to_string_lossy().to_string()),
                            host: None,
                            profile: (!profile.is_empty()).then_some(profile),
                            command: Some(command),
                        }),
                    );
                }
            } else {
                // Insert is review-only: the command goes to the
                // clipboard and a terminal opens in the root for it.
                ui.ctx().copy_text(command);
                if let Some(root) = &self.root {
                    let profile = s.settings.read().git.terminal_profile.clone();
                    cx.open_new_tab(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd: Some(root.to_string_lossy().to_string()),
                            host: None,
                            profile: (!profile.is_empty()).then_some(profile),
                            command: None,
                        }),
                    );
                }
                cx.toast(ToastLevel::Info, s.t("git-ai-copied"));
            }
        } else {
            // Prose or a ```commit block: the commit message.
            self.message_from_ai = Some(block.trim().to_string());
            self.tree.view = View::Status;
            cx.toast(ToastLevel::Success, s.t("git-ai-message-set"));
        }
    }

    /// A merge, rebase or cherry-pick git has paused on: Continue, Skip
    /// (cherry-pick) and Abort.
    fn operation_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let Some(state) = self.op_state else {
            return;
        };
        let s = cx.services;
        let busy = self.busy.is_some();
        let key = match state {
            hs_git::OperationState::Merge => "git-op-merge",
            hs_git::OperationState::Rebase => "git-op-rebase",
            hs_git::OperationState::CherryPick => "git-op-cherry-pick",
        };
        let mut op: Option<Op> = None;
        egui::Frame::new()
            .fill(theme.warning.linear_multiply(0.12))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{} {}", ph::WARNING, s.t(key))).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(!busy, egui::Button::new(s.t("git-abort")))
                            .clicked()
                        {
                            op = Some(Op::new(s.t("git-abort"), move |r| {
                                let out = match state {
                                    hs_git::OperationState::Merge => hs_git::write::merge_abort(r),
                                    hs_git::OperationState::Rebase => {
                                        hs_git::write::rebase_abort(r)
                                    }
                                    hs_git::OperationState::CherryPick => {
                                        hs_git::write::cherry_pick_abort(r)
                                    }
                                };
                                out.map_err(|e| e.to_string())
                            }));
                        }
                        if state == hs_git::OperationState::CherryPick
                            && ui
                                .add_enabled(!busy, egui::Button::new(s.t("git-skip")))
                                .clicked()
                        {
                            op = Some(Op::new(s.t("git-skip"), |r| {
                                hs_git::write::cherry_pick_skip(r).map_err(|e| e.to_string())
                            }));
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new(s.t("git-continue")))
                            .clicked()
                        {
                            op = Some(Op::new(s.t("git-continue"), move |r| {
                                let out = match state {
                                    hs_git::OperationState::Merge => {
                                        hs_git::write::merge_continue(r)
                                    }
                                    hs_git::OperationState::Rebase => {
                                        hs_git::write::rebase_continue(r)
                                    }
                                    hs_git::OperationState::CherryPick => {
                                        hs_git::write::cherry_pick_continue(r)
                                    }
                                };
                                out.map_err(|e| e.to_string())
                            }));
                        }
                    });
                });
            });
        if let Some(op) = op {
            self.exec(s, op);
        }
    }

    /// What a row of the tree asked for.
    fn tree_pick(&mut self, s: &Services, p: tree::Pick) {
        use tree::Pick as P;
        let first_remote = self
            .tree
            .refs
            .remotes
            .first()
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "origin".into());
        match p {
            P::View(v) => self.tree.view = v,
            P::Checkout(b) => {
                let op = Op::new(s.t("git-menu-checkout"), move |r| {
                    hs_git::write::checkout(r, &b)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::CheckoutRemote(full) => {
                // `origin/topic` -> a local `topic` tracking it.
                let local = full
                    .split_once('/')
                    .map(|(_, b)| b.to_string())
                    .unwrap_or(full.clone());
                let op = Op::new(s.t("git-menu-checkout"), move |r| {
                    hs_git::write::create_branch(r, &local, Some(&full))
                        .map_err(|e| e.to_string())?;
                    hs_git::write::checkout(r, &local)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::MergeInto(b) => self.verbs.merge_from(&b),
            P::RebaseOnto(b) => self.verbs.rebase_on(&b),
            P::Rename(b) => self.verbs.rename(&b),
            P::Delete(b) => {
                let text = s
                    .i18n
                    .t_args("git-confirm-delete-branch", &[("branch", &b)]);
                let op = Op::new(s.t("git-menu-delete"), move |r| {
                    hs_git::write::delete_branch(r, &b, true)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.guarded(s, Some(text), op);
            }
            P::SetUpstream(b) => {
                let remote = first_remote;
                let op = Op::new(s.t("git-menu-set-upstream"), move |r| {
                    hs_git::write::set_upstream(r, &remote, &b)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::PushBranch(b) => {
                let question = self.protected(s, &b).then(|| {
                    s.i18n
                        .t_args("git-confirm-protected-push", &[("branch", &b)])
                });
                let remote = first_remote;
                let op = Op::new(s.t("git-push"), move |r| {
                    hs_git::write::push_branch(r, &remote, &b, true).map_err(|e| e.to_string())
                });
                self.guarded(s, question, op);
            }
            P::DeleteRemoteBranch { remote, branch } => {
                let text = s.i18n.t_args(
                    "git-confirm-delete-remote-branch",
                    &[("branch", &format!("{remote}/{branch}"))],
                );
                let op = Op::new(s.t("git-menu-delete-remote-branch"), move |r| {
                    hs_git::write::delete_remote_branch(r, &remote, &branch)
                        .map_err(|e| e.to_string())
                });
                self.guarded(s, Some(text), op);
            }
            P::FetchRemote(name) => {
                let op = Op::new(s.t("git-fetch-button"), move |r| {
                    hs_git::write::fetch_remote(r, &name, true).map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::EditRemote(name) => {
                let existing = self
                    .tree
                    .refs
                    .remotes
                    .iter()
                    .find(|r| r.name == name)
                    .cloned();
                self.verbs.remote(existing.as_ref());
            }
            P::AddRemote => self.verbs.remote(None),
            P::RemoveRemote(name) => {
                let text = s
                    .i18n
                    .t_args("git-confirm-remove-remote", &[("remote", &name)]);
                let op = Op::new(s.t("git-menu-remove-remote"), move |r| {
                    hs_git::write::remove_remote(r, &name)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.guarded(s, Some(text), op);
            }
            P::PushTag(t) => {
                let remote = first_remote;
                let op = Op::new(s.t("git-menu-push-tag"), move |r| {
                    hs_git::write::push_tag(r, &remote, &t).map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::DeleteTag(t) => {
                let text = s.i18n.t_args("git-confirm-delete-tag", &[("tag", &t)]);
                let op = Op::new(s.t("git-menu-delete-tag"), move |r| {
                    hs_git::write::delete_tag(r, &t)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.guarded(s, Some(text), op);
            }
            P::StashApply(id) => {
                let op = Op::new(s.t("git-menu-stash-apply"), move |r| {
                    hs_git::write::stash_apply(r, &id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::StashPop(id) => {
                let op = Op::new(s.t("git-menu-stash-pop"), move |r| {
                    hs_git::write::stash_pop(r, &id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            P::StashDrop(id) => {
                let text = s.i18n.t_args("git-confirm-drop-stash", &[("stash", &id)]);
                let op = Op::new(s.t("git-menu-stash-drop"), move |r| {
                    hs_git::write::stash_drop(r, &id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.guarded(s, Some(text), op);
            }
        }
    }

    fn history_column(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let pick = self.history.ui(ui, theme, cx);
        match pick {
            Some(history::Pick::FileDiff {
                id,
                first_parent,
                file,
            }) => self.load_commit_diff(cx.services, id, first_parent, file),
            Some(history::Pick::Blame { id, path }) => {
                if let Some(root) = self.root.clone() {
                    self.blame.load(cx.services, root, path, Some(id));
                    self.right = Right::Blame;
                }
            }
            Some(history::Pick::Checkout(id)) => {
                let s = cx.services;
                let op = Op::new(s.t("git-menu-checkout-commit"), move |r| {
                    hs_git::write::checkout(r, &id)
                        .map(|_| String::new())
                        .map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            Some(history::Pick::Cherry(id)) => {
                let s = cx.services;
                let op = Op::new(s.t("git-menu-cherry-pick"), move |r| {
                    hs_git::write::cherry_pick(r, &id).map_err(|e| e.to_string())
                });
                self.exec(s, op);
            }
            Some(history::Pick::BranchAt(id)) => self.verbs.branch_at(&id),
            Some(history::Pick::TagAt(id)) => self.verbs.tag_at(&id),
            None => {}
        }
    }

    fn right_column(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        match &self.right {
            Right::Nothing => {
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    ui.add_space(24.0);
                    hs_ui::form::note(ui, theme, ph::GIT_DIFF, &s.t("git-no-file-selected"));
                });
            }
            Right::Blame => {
                if let Some(id) = self.blame.ui(ui, theme, cx) {
                    // A line's commit opens in History.
                    self.tree.view = View::History;
                    self.history.search = id;
                }
            }
            Right::Conflict => {
                if let Some(root) = self.root.clone()
                    && self.conflict.ui(ui, theme, cx, &root)
                {
                    self.right = Right::Nothing;
                    self.refresh_all(s);
                }
            }
            Right::Work { path, side } => {
                let (path, side) = (path.clone(), *side);
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(ph::FILE).color(theme.text_muted));
                            ui.label(egui::RichText::new(&path).strong());
                            let what = match side {
                                Side::Staged => s.t("git-staged"),
                                Side::Unstaged => s.t("git-unstaged"),
                                Side::Untracked => s.t("git-untracked"),
                            };
                            ui.label(egui::RichText::new(what).color(theme.text_muted));
                            if let Some(d) = &self.diff {
                                diffview::counts(ui, theme, d);
                            }
                        });
                    });
                ui.separator();
                let Some(d) = self.diff.clone() else {
                    ui.spinner();
                    return;
                };
                let busy = self.busy.is_some();
                let verbs = (side != Side::Untracked).then_some(side);
                if let Some(pick) = diffview::show(ui, theme, s, &d, verbs, busy) {
                    let (hi, op) = match pick {
                        HunkPick::Stage(i) => (i, hs_git::write::HunkOp::Stage),
                        HunkPick::Unstage(i) => (i, hs_git::write::HunkOp::Unstage),
                        HunkPick::Discard(i) => (i, hs_git::write::HunkOp::Discard),
                    };
                    if let Some(hunk) = d.hunks.get(hi).cloned() {
                        let p = path.clone();
                        let label = match op {
                            hs_git::write::HunkOp::Discard => s.t("git-discard-button"),
                            hs_git::write::HunkOp::Stage => s.t("git-stage"),
                            hs_git::write::HunkOp::Unstage => s.t("git-unstage"),
                        };
                        self.run_quiet(s, label, move |r| {
                            hs_git::write::apply_hunk(r, &p, &hunk, op)
                                .map(|_| String::new())
                                .map_err(|e| e.to_string())
                        });
                    }
                }
            }
            Right::Commit { id, file } => {
                let (id, file) = (id.clone(), file.clone());
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(ph::FILE).color(theme.text_muted));
                            ui.label(egui::RichText::new(&file.path).strong());
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    ph::GIT_COMMIT,
                                    &id[..id.len().min(7)]
                                ))
                                .monospace()
                                .color(theme.text_muted),
                            );
                            if let Some(d) = &self.diff {
                                diffview::counts(ui, theme, d);
                            }
                        });
                    });
                ui.separator();
                let Some(d) = self.diff.clone() else {
                    ui.spinner();
                    return;
                };
                diffview::show(ui, theme, s, &d, None, true);
            }
        }
    }

    fn confirm_bar(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let Some(c) = &self.confirm else {
            return;
        };
        let s = cx.services;
        let mut go = false;
        let mut cancel = false;
        egui::Frame::new()
            .fill(theme.error.linear_multiply(0.12))
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let verb = match c {
                        Confirm::Discard(paths) => {
                            ui.label(s.i18n.t_args(
                                "git-discard-confirm",
                                &[("count", &paths.len().to_string())],
                            ));
                            s.t("git-discard-button")
                        }
                        Confirm::Run { text, op } => {
                            ui.label(text);
                            op.label.clone()
                        }
                    };
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(s.t("cancel")).clicked() {
                            cancel = true;
                        }
                        if hs_ui::form::danger_button(ui, theme, &verb).clicked() {
                            go = true;
                        }
                    });
                });
            });
        if cancel {
            self.confirm = None;
        }
        if go
            && matches!(self.confirm, Some(Confirm::Run { .. }))
            && let Some(Confirm::Run { op, .. }) = self.confirm.take()
        {
            self.exec(s, op);
            return;
        }
        if go && let Some(Confirm::Discard(paths)) = self.confirm.take() {
            let untracked: Vec<String> = self
                .status
                .as_ref()
                .map(|st| {
                    st.files
                        .iter()
                        .filter(|f| f.kind == Kind::Untracked && paths.contains(&f.path))
                        .map(|f| f.path.clone())
                        .collect()
                })
                .unwrap_or_default();
            let tracked: Vec<String> = paths
                .into_iter()
                .filter(|p| !untracked.contains(p))
                .collect();
            self.run(s, s.t("git-discard-button"), move |r| {
                let t: Vec<&str> = tracked.iter().map(String::as_str).collect();
                let u: Vec<&str> = untracked.iter().map(String::as_str).collect();
                if !t.is_empty() {
                    hs_git::write::discard(r, &t).map_err(|e| e.to_string())?;
                }
                if !u.is_empty() {
                    hs_git::write::remove_untracked(r, &u).map_err(|e| e.to_string())?;
                }
                Ok(String::new())
            });
        }
    }

    fn status_column(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(st) = self.status.clone() else {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.spinner();
            });
            return;
        };
        let filter = self.filter.to_lowercase();
        let shown = |f: &FileStatus| filter.is_empty() || f.path.to_lowercase().contains(&filter);
        let unstaged: Vec<FileStatus> = st
            .files
            .iter()
            .filter(|f| (f.unstaged() || f.kind != Kind::Changed) && shown(f))
            .cloned()
            .collect();
        let staged: Vec<FileStatus> = st
            .files
            .iter()
            .filter(|f| f.staged() && shown(f))
            .cloned()
            .collect();
        let busy = self.busy.is_some();

        // The commit box takes the bottom; the lists share what is left.
        let commit_h = 172.0;
        let lists_h = (ui.available_height() - commit_h).max(120.0);
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(ph::MAGNIFYING_GLASS).color(theme.text_muted));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filter)
                            .hint_text(s.t("git-filter-hint"))
                            .desired_width(f32::INFINITY),
                    );
                });
            });
        let mut stage: Vec<String> = Vec::new();
        let mut unstage: Vec<String> = Vec::new();
        let mut discard: Vec<String> = Vec::new();
        let mut stage_all = false;
        let mut unstage_all = false;
        let mut pick: Option<(String, Side)> = None;
        let mut conflict: Option<String> = None;
        let mut blame: Option<String> = None;
        let mut file_history: Option<String> = None;
        let selected = self.work_selection();
        let tips = RowTips {
            stage: s.t("git-stage"),
            unstage: s.t("git-unstage"),
            discard: s.t("git-discard-button"),
            blame: s.t("git-blame"),
            history: s.t("git-file-history"),
        };
        egui::ScrollArea::vertical()
            .id_salt(ui.id().with("git-lists"))
            .max_height(lists_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Unstaged
                list_header(ui, theme, &s.t("git-unstaged"), unstaged.len(), |ui| {
                    if ui
                        .add_enabled(
                            !busy && !unstaged.is_empty(),
                            egui::Button::new(s.t("git-stage-all")).small(),
                        )
                        .clicked()
                    {
                        stage_all = true;
                    }
                });
                if unstaged.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(s.t("git-clean")).color(theme.text_muted));
                    });
                }
                for f in &unstaged {
                    let side = if f.kind == Kind::Untracked {
                        Side::Untracked
                    } else {
                        Side::Unstaged
                    };
                    let is_sel = selected.as_ref() == Some(&(f.path.clone(), side));
                    let act = file_row(ui, theme, f, is_sel, busy, RowVerbs::Unstaged, &tips);
                    match act {
                        RowAction::Pick if f.kind == Kind::Conflict => {
                            conflict = Some(f.path.clone());
                        }
                        RowAction::Pick => pick = Some((f.path.clone(), side)),
                        RowAction::Stage => stage.push(f.path.clone()),
                        RowAction::Discard => discard.push(f.path.clone()),
                        RowAction::Blame => blame = Some(f.path.clone()),
                        RowAction::History => file_history = Some(f.path.clone()),
                        RowAction::Unstage | RowAction::None => {}
                    }
                }
                ui.add_space(8.0);
                // Staged
                list_header(ui, theme, &s.t("git-staged"), staged.len(), |ui| {
                    if ui
                        .add_enabled(
                            !busy && !staged.is_empty(),
                            egui::Button::new(s.t("git-unstage-all")).small(),
                        )
                        .clicked()
                    {
                        unstage_all = true;
                    }
                });
                if staged.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(s.t("git-nothing-staged")).color(theme.text_muted),
                        );
                    });
                }
                for f in &staged {
                    let is_sel = selected.as_ref() == Some(&(f.path.clone(), Side::Staged));
                    let act = file_row(ui, theme, f, is_sel, busy, RowVerbs::Staged, &tips);
                    match act {
                        RowAction::Pick => pick = Some((f.path.clone(), Side::Staged)),
                        RowAction::Unstage => unstage.push(f.path.clone()),
                        RowAction::Blame => blame = Some(f.path.clone()),
                        RowAction::History => file_history = Some(f.path.clone()),
                        _ => {}
                    }
                }
            });
        ui.separator();
        self.commit_box(ui, theme, cx, &st, staged.len());

        if let Some((p, side)) = pick {
            self.load_work_diff(s, p, side);
        }
        if let Some(p) = conflict
            && let Some(root) = self.root.clone()
        {
            self.conflict.load(s, root, p);
            self.right = Right::Conflict;
        }
        if let Some(p) = blame
            && let Some(root) = self.root.clone()
        {
            self.blame.load(s, root, p, None);
            self.right = Right::Blame;
        }
        if let Some(p) = file_history {
            self.history.path = Some(p);
            self.history.stale = true;
            self.tree.view = View::History;
        }
        if stage_all {
            self.run_quiet(s, s.t("git-stage"), |r| {
                hs_git::write::stage_all(r)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string())
            });
        } else if unstage_all {
            self.run_quiet(s, s.t("git-unstage"), |r| {
                hs_git::write::unstage_all(r)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string())
            });
        } else if !stage.is_empty() {
            self.run_quiet(s, s.t("git-stage"), move |r| {
                let v: Vec<&str> = stage.iter().map(String::as_str).collect();
                hs_git::write::stage(r, &v)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string())
            });
        } else if !unstage.is_empty() {
            self.run_quiet(s, s.t("git-unstage"), move |r| {
                let v: Vec<&str> = unstage.iter().map(String::as_str).collect();
                hs_git::write::unstage(r, &v)
                    .map(|_| String::new())
                    .map_err(|e| e.to_string())
            });
        } else if !discard.is_empty() {
            // Always asked, never guarded away (docs/tools/git-activity.md §5.4).
            self.confirm = Some(Confirm::Discard(discard));
        }
    }

    fn commit_box(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        cx: &mut ToolContext,
        st: &Status,
        staged: usize,
    ) {
        let s = cx.services;
        let busy = self.busy.is_some();
        if let Some(m) = self.message_from_ai.take() {
            self.message = m;
        }
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.message)
                        .hint_text(s.t("git-commit-message-hint"))
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.add_space(4.0);
                // The options on one row and the button on its own
                // below: a `right_to_left` scope beside two checkboxes
                // takes whatever width is left, and in a narrow column
                // that is less than the button needs (seen on screen).
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.amend, s.t("git-amend-check"));
                    ui.checkbox(&mut self.push_after, s.t("git-push-after"));
                });
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can =
                            !busy && (staged > 0 || self.amend) && !self.message.trim().is_empty();
                        let label = if self.amend {
                            s.t("git-commit-amend")
                        } else {
                            s.i18n
                                .t_args("git-commit-n", &[("count", &staged.to_string())])
                        };
                        let b = ui.add_enabled(
                            can,
                            egui::Button::new(
                                egui::RichText::new(format!("{} {label}", ph::GIT_COMMIT))
                                    .color(if can { theme.text } else { theme.text_muted }),
                            )
                            .fill(if can {
                                theme.accent
                            } else {
                                theme.surface
                            }),
                        );
                        if b.clicked() {
                            let message = self.message.clone();
                            let opts = hs_git::write::CommitOptions {
                                amend: self.amend,
                                signoff: s.settings.read().git.signoff,
                            };
                            let push_after = self.push_after;
                            let set_upstream = match (&st.upstream, &st.branch) {
                                (None, Some(b)) => Some(("origin".to_string(), b.clone())),
                                _ => None,
                            };
                            self.message.clear();
                            self.amend = false;
                            self.run(s, s.t("git-commit"), move |r| {
                                hs_git::write::commit(r, &message, &opts)
                                    .map_err(|e| e.to_string())?;
                                if push_after {
                                    return hs_git::write::push(
                                        r,
                                        &hs_git::write::PushOptions {
                                            set_upstream,
                                            force_with_lease: false,
                                        },
                                    )
                                    .map_err(|e| e.to_string());
                                }
                                Ok(String::new())
                            });
                        }
                    });
                });
            });
    }
}

/// What the assistant is told: `git status --short`, the staged diff
/// (cut at a bound - a model is given a slice, never a tree), and the
/// last commits. Read on the blocking pool.
fn assistant_context(root: &Path) -> String {
    const DIFF_BOUND: usize = 12_000;
    let run = |args: &[&str]| {
        hs_git::git_with_env(root, args, &[])
            .map(|o| String::from_utf8_lossy(&o).to_string())
            .unwrap_or_default()
    };
    let status = run(&["status", "--short"]);
    let mut diff = run(&["diff", "--cached", "--no-color"]);
    if diff.is_empty() {
        diff = run(&["diff", "--no-color"]);
    }
    if diff.len() > DIFF_BOUND {
        let cut = diff
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i < DIFF_BOUND)
            .last()
            .unwrap_or(0);
        diff.truncate(cut);
        diff.push_str("\n… (diff truncated)\n");
    }
    let log = run(&["log", "--oneline", "-8", "--no-color"]);
    format!("git status --short:\n{status}\nstaged diff:\n{diff}\nrecent commits:\n{log}")
}

/// Which lists a file appears in, as the sides a selection may name.
fn side_of(f: &FileStatus) -> Vec<Side> {
    let mut v = Vec::new();
    match f.kind {
        Kind::Untracked => v.push(Side::Untracked),
        Kind::Conflict => v.push(Side::Unstaged),
        Kind::Changed => {
            if f.unstaged() {
                v.push(Side::Unstaged);
            }
            if f.staged() {
                v.push(Side::Staged);
            }
        }
    }
    v
}

fn list_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    count: usize,
    actions: impl FnOnce(&mut egui::Ui),
) {
    egui::Frame::new()
        .fill(theme.surface)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).strong());
                ui.label(egui::RichText::new(count.to_string()).color(theme.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), actions);
            });
        });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RowVerbs {
    Unstaged,
    Staged,
}

enum RowAction {
    None,
    Pick,
    Stage,
    Unstage,
    Discard,
    Blame,
    History,
}

/// The row verbs' tooltips and menu labels, read once per frame rather
/// than per row.
struct RowTips {
    stage: String,
    unstage: String,
    discard: String,
    blame: String,
    history: String,
}

/// One file in a list: its status letter, its path, and the verbs for
/// its side in a fixed column (owner's rule on aligned row actions).
/// Right-click offers Blame and File History.
fn file_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    f: &FileStatus,
    selected: bool,
    busy: bool,
    verbs: RowVerbs,
    tips: &RowTips,
) -> RowAction {
    let mut action = RowAction::None;
    let height = 26.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    let fill = if selected {
        theme.accent.linear_multiply(0.30)
    } else if resp.hovered() {
        theme.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    let letter = f.letter();
    let colour = match letter {
        'A' | 'U' => theme.success,
        'D' => theme.error,
        'M' | 'T' => theme.warning,
        'R' | 'C' => theme.accent,
        '!' => theme.error,
        _ => theme.text_muted,
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.painter().text(
        rect.left_center() + egui::vec2(12.0, 0.0),
        egui::Align2::LEFT_CENTER,
        letter.to_string(),
        egui::FontId::monospace(font.size),
        colour,
    );
    let verbs_w = 96.0;
    let text_rect = egui::Rect::from_min_max(
        rect.left_top() + egui::vec2(30.0, 0.0),
        rect.right_bottom() - egui::vec2(verbs_w, 0.0),
    );
    let label = match &f.orig_path {
        Some(o) => format!("{o} \u{2192} {}", f.path),
        None => f.path.clone(),
    };
    let galley = ui
        .painter()
        .layout(label, font, theme.text, text_rect.width().max(10.0));
    // One line, clipped at the verbs rather than wrapped under them.
    let clip = ui.painter().with_clip_rect(text_rect);
    clip.galley(
        egui::pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        theme.text,
    );
    if resp.clicked() {
        action = RowAction::Pick;
    }
    resp.context_menu(|ui| {
        if ui.button(&tips.blame).clicked() {
            action = RowAction::Blame;
            ui.close();
        }
        if ui.button(&tips.history).clicked() {
            action = RowAction::History;
            ui.close();
        }
    });
    let verb_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - verbs_w, rect.top()),
        rect.right_bottom(),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(verb_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.add_space(4.0);
    match verbs {
        RowVerbs::Unstaged => {
            if child
                .add_enabled(!busy, egui::Button::new(ph::PLUS).small())
                .on_hover_text(&tips.stage)
                .clicked()
            {
                action = RowAction::Stage;
            }
            if child
                .add_enabled(
                    !busy,
                    egui::Button::new(ph::ARROW_COUNTER_CLOCKWISE).small(),
                )
                .on_hover_text(&tips.discard)
                .clicked()
            {
                action = RowAction::Discard;
            }
        }
        RowVerbs::Staged => {
            if child
                .add_enabled(!busy, egui::Button::new(ph::MINUS).small())
                .on_hover_text(&tips.unstage)
                .clicked()
            {
                action = RowAction::Unstage;
            }
        }
    }
    action
}
