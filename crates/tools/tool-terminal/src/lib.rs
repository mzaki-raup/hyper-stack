//! Terminal tool (docs/tools/terminal.md): one shell session per tab, shell
//! profiles in the sidebar, restart on exit, "open here" via `OpenRequest`.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_term::alacritty_terminal::grid::{Dimensions, Scroll};
use hs_term::links::LinkTarget;
use hs_term::search::{self, SearchMatch, SearchQuery};
use hs_term::session::{Session, TermEvent};
use hs_term::shells::ShellProfile;
use hs_term::widget::{TermTheme, TerminalView};
use hs_ui::tip::TipExt;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "terminal",
    title_key: "tool-terminal",
    icon: hs_ui::icons::ph::TERMINAL_WINDOW,
    category: ToolCategory::Code,
    accepts: &[Accept::LocalFile],
};

pub struct TerminalFactory;

impl ToolFactory for TerminalFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(TerminalTool::new(services, state))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct State {
    profile: Option<String>,
    cwd: Option<PathBuf>,
    /// Set when this tab runs a shell **inside a pod** rather than on
    /// this machine (phase 7). Saved with the tab, so a restored tab
    /// reconnects to the pod instead of silently opening PowerShell.
    #[serde(default)]
    cluster: Option<ClusterExec>,
    /// The icon key of the **CLI profile** this tab was opened for, when
    /// it was opened from one.
    ///
    /// A CLI row opens an ordinary shell running the CLI's command line,
    /// so the tab would otherwise wear the shell's glyph and three CLI
    /// tabs would look alike. Resolved once, when the tab is opened, and
    /// kept here because `tab_icon` is asked every frame and is handed
    /// no `Services` (owner's requirement, 2026-10-26).
    #[serde(default)]
    icon: Option<String>,
}

/// Which container a cluster tab runs its shell in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct ClusterExec {
    /// The `k8s` connection profile.
    conn_id: String,
    namespace: String,
    pod: String,
    /// Empty for a single-container pod.
    container: String,
}

impl ClusterExec {
    /// The handover address the Containers activity sends:
    /// `k8s://<conn id>/<namespace>/<pod>/<container>`.
    ///
    /// It travels in `OpenRequest::Terminal`'s existing `host` field -
    /// a pod's shell *is* the host this tab is on - so no new variant
    /// is needed and every other tool's match arms are untouched.
    fn parse(host: &str) -> Option<Self> {
        let rest = host.strip_prefix("k8s://")?;
        let mut parts = rest.splitn(4, '/');
        let conn_id = parts.next()?.to_string();
        let namespace = parts.next()?.to_string();
        let pod = parts.next()?.to_string();
        let container = parts.next().unwrap_or_default().to_string();
        if conn_id.is_empty() || pod.is_empty() {
            return None;
        }
        Some(Self {
            conn_id,
            namespace,
            pod,
            container,
        })
    }

    /// What the tab is called: the pod, and the container when the pod
    /// has more than one and the caller said which.
    fn label(&self) -> String {
        if self.container.is_empty() {
            self.pod.clone()
        } else {
            format!("{}/{}", self.pod, self.container)
        }
    }
}

/// A slot the cluster exec worker fills: the transport and reader for
/// a pod's shell, or why it could not be opened.
type ClusterPending = std::sync::Arc<
    parking_lot::Mutex<
        Option<
            Result<
                (
                    hs_container::exec::ExecTransport,
                    hs_container::exec::ExecReader,
                ),
                String,
            >,
        >,
    >,
>;

pub struct TerminalTool {
    state: State,
    /// Set when this tab is the activity's home tab: its name, which is the
    /// activity's rather than a shell's. A restored home tab carries a
    /// saved profile and would otherwise read "PowerShell 7".
    /// Owner's report, 2026-10-17.
    home_title: Option<String>,
    /// A command handed over with the open request, typed when the shell is up.
    pending_command: Option<String>,
    /// Where the cluster exec worker leaves its answer: connecting to a
    /// cluster and opening the stream are async, and the UI thread
    /// never waits (ADR-013).
    cluster_pending: Option<ClusterPending>,
    profiles: Arc<Vec<ShellProfile>>,
    session: Option<Session>,
    view: TerminalView,
    error: Option<String>,
    exit_code: Option<u32>,
    bell: bool,
    find: Find,
    /// Give the terminal keyboard focus on the next frame.
    pending_focus: Option<egui::Id>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
    /// Whether each CLI profile's command resolves on PATH, re-checked
    /// every few seconds: resolving stats every PATH entry per PATHEXT,
    /// which the sidebar used to do per profile per frame.
    cli_found: std::collections::HashMap<String, (std::time::Instant, bool)>,
}

const CLI_FOUND_TTL: std::time::Duration = std::time::Duration::from_secs(5);

/// The search bar (Ctrl+Shift+F): query, options and the hits in the grid.
#[derive(Default)]
struct Find {
    open: bool,
    focus: bool,
    query: SearchQuery,
    matches: Vec<SearchMatch>,
    current: Option<usize>,
    error: Option<String>,
}

impl TerminalTool {
    fn new(services: &Services, state: State) -> Self {
        let profiles = hs_term::shells::profiles();
        let font_size = services.settings.read().terminal.font_size;
        let mut t = Self {
            state,
            home_title: None,
            pending_command: None,
            cluster_pending: None,
            profiles,
            session: None,
            view: TerminalView::new(egui::Id::new(("hs-terminal", hs_core::new_id())), font_size),
            error: None,
            exit_code: None,
            bell: false,
            find: Find::default(),
            pending_focus: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            cli_found: std::collections::HashMap::new(),
        };
        t.apply_settings(services);
        // The shell starts on the first frame that actually draws a
        // terminal. Starting it here would give the home tab a live shell
        // nobody asked for, and the home tab is a dashboard.
        // Owner's requirement, 2026-10-17.
        t
    }

    fn profile(&self, services: &Services) -> Option<ShellProfile> {
        let wanted = self.state.profile.clone().or_else(|| {
            let d = services.settings.read().terminal.default_profile.clone();
            (!d.is_empty()).then_some(d)
        });
        wanted
            .and_then(|id| self.profiles.iter().find(|p| p.id == id).cloned())
            .or_else(|| self.profiles.first().cloned())
    }

    fn apply_settings(&mut self, services: &Services) {
        let st = services.settings.read();
        self.view.font_size = st.terminal.font_size;
        self.view.copy_on_select = st.terminal.copy_on_select;
        self.view.right_click_copy_paste = st.terminal.right_click_copy_paste;
        self.view.link_detection = st.terminal.link_detection;
    }

    /// Re-run the search over the current grid; keeps the current hit near
    /// where it was, or picks the newest one.
    fn run_search(&mut self) {
        let Some(sess) = &self.session else {
            return;
        };
        self.find.error = None;
        let re = match self.find.query.compile() {
            Ok(Some(re)) => re,
            Ok(None) => {
                self.find.matches.clear();
                self.find.current = None;
                self.sync_view();
                return;
            }
            Err(e) => {
                self.find.error = Some(e);
                self.find.matches.clear();
                self.find.current = None;
                self.sync_view();
                return;
            }
        };
        let previous = self
            .find
            .current
            .and_then(|i| self.find.matches.get(i))
            .copied();
        self.find.matches = search::search(&sess.term.lock(), &re);
        self.find.current = match previous {
            Some(p) => self
                .find
                .matches
                .iter()
                .rposition(|m| (m.line, m.start) <= (p.line, p.start))
                .or(self.find.matches.len().checked_sub(1)),
            None => self.find.matches.len().checked_sub(1),
        };
        self.sync_view();
        self.reveal_current();
    }

    /// Step through hits: `older` moves up the scrollback.
    fn step(&mut self, older: bool) {
        self.run_search();
        let n = self.find.matches.len();
        if n == 0 {
            return;
        }
        let i = self.find.current.unwrap_or(n - 1);
        self.find.current = Some(if older { (i + n - 1) % n } else { (i + 1) % n });
        self.sync_view();
        self.reveal_current();
    }

    fn sync_view(&mut self) {
        if self.find.open {
            self.view.search_matches = self.find.matches.clone();
            self.view.search_current = self.find.current;
        } else {
            self.view.search_matches.clear();
            self.view.search_current = None;
        }
    }

    /// Scroll so the current hit is on screen.
    fn reveal_current(&self) {
        let (Some(sess), Some(m)) = (
            &self.session,
            self.find.current.and_then(|i| self.find.matches.get(i)),
        ) else {
            return;
        };
        let mut t = sess.term.lock();
        let offset = t.grid().display_offset() as i32;
        let rows = t.grid().screen_lines() as i32;
        let visible = m.line >= -offset && m.line < rows - offset;
        if !visible {
            let target = search::offset_for_line(m.line, rows as usize, t.grid().history_size());
            t.scroll_display(Scroll::Delta(target as i32 - offset));
        }
    }

    fn open_find(&mut self) {
        self.find.open = true;
        self.find.focus = true;
        // Seed with the selection, like most terminals.
        if let Some(sess) = &self.session
            && let Some(sel) = sess.term.lock().selection_to_string()
            && !sel.is_empty()
            && !sel.contains('\n')
        {
            self.find.query.text = sel;
        }
        self.run_search();
    }

    fn close_find(&mut self) {
        self.find.open = false;
        self.sync_view();
        if let Some(sess) = &self.session {
            sess.term.lock().scroll_display(Scroll::Bottom);
        }
        let id = self.view.id;
        self.pending_focus = Some(id);
    }

    fn find_bar(&mut self, ui: &mut egui::Ui, s: &Services) {
        use hs_ui::icons::ph;
        ui.separator();
        let edit = egui::TextEdit::singleline(&mut self.find.query.text)
            .hint_text(s.t("terminal-find-placeholder"))
            .desired_width(220.0);
        let r = ui.add(edit);
        if self.find.focus {
            r.request_focus();
            self.find.focus = false;
        }
        let mut changed = r.changed();
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            let shift = ui.input(|i| i.modifiers.shift);
            self.step(!shift);
            r.request_focus();
        }
        if (r.has_focus() || r.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_find();
            return;
        }
        changed |= ui
            .selectable_label(self.find.query.case_sensitive, "Aa")
            .tip(s.t("terminal-find-case"))
            .clicked()
            .then(|| self.find.query.case_sensitive = !self.find.query.case_sensitive)
            .is_some();
        changed |= ui
            .selectable_label(self.find.query.regex, ".*")
            .tip(s.t("terminal-find-regex"))
            .clicked()
            .then(|| self.find.query.regex = !self.find.query.regex)
            .is_some();
        if changed {
            self.find.current = None;
            self.run_search();
        }
        let status = if self.find.error.is_some() {
            s.t("terminal-find-bad-regex")
        } else if self.find.query.text.is_empty() {
            String::new()
        } else if self.find.matches.is_empty() {
            s.t("terminal-find-none")
        } else {
            let total = if self.find.matches.len() >= search::MAX_MATCHES {
                format!("{}+", search::MAX_MATCHES)
            } else {
                self.find.matches.len().to_string()
            };
            let current = self.find.current.map(|i| i + 1).unwrap_or(0).to_string();
            s.i18n.t_args(
                "terminal-find-count",
                &[("current", &current), ("total", &total)],
            )
        };
        let label = ui.label(egui::RichText::new(status).weak());
        if let Some(e) = &self.find.error {
            label.tip(e.clone());
        }
        if ui
            .button(ph::ARROW_UP)
            .tip(s.t("terminal-find-prev"))
            .clicked()
        {
            self.step(true);
        }
        if ui
            .button(ph::ARROW_DOWN)
            .tip(s.t("terminal-find-next"))
            .clicked()
        {
            self.step(false);
        }
        if ui.button(ph::X).tip(s.t("terminal-find-close")).clicked() {
            self.close_find();
        }
    }

    /// The shell's folder: from a `user@host: dir` title when the prompt
    /// sets one, else the folder the tab was opened in.
    fn shell_dirs(&self, services: &Services) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(sess) = &self.session
            && let Some((_, dir)) = sess.title.split_once(": ")
        {
            let dir = dir.trim();
            if let Some(rest) = dir.strip_prefix('~') {
                if let Some(home) = home_dir() {
                    out.push(home.join(rest.trim_start_matches(['/', '\\'])));
                }
            } else if !dir.is_empty() {
                out.push(PathBuf::from(dir));
            }
        }
        if let Some(c) = &self.state.cwd {
            out.push(c.clone());
        }
        out.push(services.workspace.read().root().to_path_buf());
        out
    }

    fn open_link(&self, target: LinkTarget, ui: &egui::Ui, cx: &mut ToolContext) {
        match target {
            LinkTarget::Url(url) => {
                // Shift+click opens the link in HyperStack's own browser.
                if ui.input(|i| i.modifiers.shift) {
                    cx.open("browser", Some(OpenRequest::Url { url }));
                } else {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
            LinkTarget::Path { path, line, .. } => {
                let p = if let Some(rest) = path.strip_prefix("~/") {
                    home_dir()
                        .map(|h| h.join(rest))
                        .unwrap_or_else(|| PathBuf::from(&path))
                } else {
                    PathBuf::from(&path)
                };
                let found = if p.is_absolute() {
                    p.exists().then_some(p)
                } else {
                    self.shell_dirs(cx.services)
                        .into_iter()
                        .map(|d| d.join(&p))
                        .find(|c| c.exists())
                };
                match found {
                    Some(f) if f.is_dir() => cx.open(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd: Some(f.to_string_lossy().to_string()),
                            host: None,
                            profile: self.state.profile.clone(),
                            command: None,
                        }),
                    ),
                    Some(f) => cx.open(
                        "editor",
                        Some(OpenRequest::File {
                            uri: f.to_string_lossy().to_string(),
                            line,
                        }),
                    ),
                    None => {
                        let msg = cx
                            .services
                            .i18n
                            .t_args("terminal-link-missing", &[("path", &path)]);
                        cx.toast(ToastLevel::Warning, msg);
                    }
                }
            }
        }
    }

    /// The home tab: the shells this machine offers, what is open, and
    /// the last picture of each terminal. Owner's requirement, 2026-10-17.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("terminal");
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-terminal"),
            Some(&s.t("terminal-dashboard-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &self.profiles.len().to_string(),
                &s.t("dash-shells"),
            );
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        egui::ScrollArea::vertical()
            .id_salt("terminal-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

    /// Start a shell **inside a pod**, through the cluster's API
    /// server.
    ///
    /// The Containers activity hands this over exactly as it hands a
    /// Docker exec over (§10.1): the difference is that there is no
    /// local command to run, so the transport is built here instead of
    /// being typed into a shell. `hs-container` is a driver library
    /// rather than a tool crate, so depending on it is the same
    /// arrangement the Logs activity already has (ADR-004).
    fn start_cluster(&mut self, services: &Services, target: ClusterExec) {
        self.error = None;
        self.exit_code = None;
        self.session = None;
        let Some(profile) = services.connections.read().by_id(&target.conn_id).cloned() else {
            self.error = Some(services.t("terminal-no-cluster"));
            return;
        };
        let fields = hs_container::ContainerFields::from_table(&profile.fields);
        let shell = services.settings.read().containers.exec_shell.clone();
        // The shell is the user's to choose: a minimal image has no
        // bash and a distroless one has no shell at all, so a failure
        // here has to be visible and editable rather than guessed at.
        let line: Vec<String> = shell
            .split_whitespace()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let line = if line.is_empty() {
            vec!["/bin/sh".to_string()]
        } else {
            line
        };
        let waker = services.runtime.waker();
        let slot: ClusterPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.cluster_pending = Some(slot);
        services.runtime.spawn(async move {
            let result = async {
                let client = hs_container::k8s::connect(&fields)
                    .await
                    .map_err(|e| e.to_string())?;
                hs_container::exec::exec(
                    &client,
                    &target.namespace,
                    &target.pod,
                    &target.container,
                    &line,
                )
                .await
                .map_err(|e| e.to_string())
            }
            .await;
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// Build the session once the cluster has answered.
    fn drain_cluster(&mut self, services: &Services) {
        let Some(result) = self.cluster_pending.as_ref().and_then(|s| s.lock().take()) else {
            return;
        };
        self.cluster_pending = None;
        let (transport, reader) = match result {
            Ok(pair) => pair,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let waker = services.runtime.waker();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
        let scrollback = services.settings.read().terminal.scrollback;
        let name = self
            .state
            .cluster
            .as_ref()
            .map(|c| c.pod.clone())
            .unwrap_or_default();
        let profile = ShellProfile::new(&format!("k8s:{name}"), &name, "", "k8s", &[]);
        match Session::with_transport(
            profile,
            Box::new(transport),
            reader,
            100,
            30,
            scrollback,
            waker,
        ) {
            Ok(sess) => self.session = Some(sess),
            Err(e) => self.error = Some(e),
        }
    }

    fn start(&mut self, services: &Services) {
        self.error = None;
        self.exit_code = None;
        // A tab opened against a pod runs that pod's shell, not a local
        // one: `profile` would otherwise start a PowerShell beside it.
        if let Some(target) = self.state.cluster.clone() {
            self.start_cluster(services, target);
            return;
        }
        let Some(profile) = self.profile(services) else {
            self.error = Some(services.t("terminal-no-shell"));
            return;
        };
        self.state.profile = Some(profile.id.clone());
        let waker = services.runtime.waker();
        let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || waker.request_repaint());
        let scrollback = services.settings.read().terminal.scrollback;
        let cwd = self
            .state
            .cwd
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| Some(services.workspace.read().root().to_path_buf()));
        match Session::spawn(profile, cwd.as_deref(), 100, 30, scrollback, waker) {
            Ok(s) => self.session = Some(s),
            Err(e) => self.error = Some(e),
        }
    }
}

impl Tool for TerminalTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    /// Which shell this tab is running.
    fn subtitle(&self) -> Option<String> {
        self.state.profile.clone()
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn set_home_tab(&mut self, home: bool, services: &Services) {
        self.home_title = home.then(|| services.t("tool-terminal"));
    }

    /// The glyph this tab's own row wears in the sidebar: the CLI's when
    /// it was opened from one, the pod's for a shell inside a pod, and
    /// otherwise the shell profile's - so PowerShell, cmd, Git Bash and
    /// WSL are told apart in the strip (owner's requirement,
    /// 2026-10-26).
    fn tab_icon(&self) -> Option<&'static str> {
        if self.home_title.is_some() {
            return None;
        }
        if self.state.cluster.is_some() {
            return Some(hs_ui::icons::ph::POLYGON);
        }
        if let Some(key) = &self.state.icon {
            return Some(cli_icon(key));
        }
        self.state.profile.as_deref().map(shell_icon)
    }

    fn title(&self) -> Cow<'_, str> {
        // The home tab is the activity's; it has no shell of its own.
        if let Some(name) = &self.home_title {
            return Cow::Borrowed(name);
        }
        // A pod's shell is named after the pod: the shell inside it
        // sets a title of its own ("sh") that says nothing about where
        // it is running.
        if let Some(c) = &self.state.cluster {
            return Cow::Owned(c.label());
        }
        // The profile name until the shell sets a real title (a bare exe path
        // or an empty string is not one).
        let profile_name = self
            .state
            .profile
            .as_ref()
            .and_then(|id| self.profiles.iter().find(|p| p.id == *id))
            .map(|p| p.name.clone());
        match &self.session {
            Some(s) => {
                let t = s.title.trim();
                let looks_like_path = t.is_empty()
                    || t.to_ascii_lowercase().ends_with(".exe")
                    || t.contains('\\')
                    || t.starts_with('/');
                if looks_like_path {
                    Cow::Owned(profile_name.unwrap_or_else(|| "Terminal".into()))
                } else {
                    Cow::Borrowed(s.title.as_str())
                }
            }
            None => Cow::Owned(profile_name.unwrap_or_else(|| "Terminal".into())),
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        // No profile picker and no size readout. The tab *is* its shell -
        // its name and icon say which - and another one is opened from
        // the sidebar, which is the only place that lists them; the
        // columns and rows are a detail of the moment, not an action
        // (owner's requirement, 2026-10-26).
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::ARROW_CLOCKWISE,
                s.t("terminal-restart")
            ))
            .clicked()
        {
            self.session = None;
            self.start(s);
        }
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::ERASER,
                s.t("terminal-clear")
            ))
            .clicked()
            && let Some(sess) = &self.session
        {
            // Clear screen and scrollback (xterm: ESC c would reset; use CSI 3J + 2J + H).
            sess.write(b"\x1b[3J\x1b[2J\x1b[H");
        }
        if self.bell {
            ui.colored_label(
                egui::Color32::from_rgb(0xD9, 0xB0, 0x2A),
                hs_ui::icons::BELL,
            );
        }
        if self.find.open {
            self.find_bar(ui, s);
        } else if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::MAGNIFYING_GLASS,
                s.t("terminal-find")
            ))
            .tip("Ctrl+Shift+F")
            .clicked()
        {
            self.open_find();
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::SPARKLE,
                    s.t("ai-assistant")
                ))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("terminal");
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        hs_ui::form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("terminal-profiles"))
                    .strong()
                    .size(15.0),
            );
        });
        let current = self.state.profile.clone().unwrap_or_default();
        // Detected shells, in the order the user put them in.
        let saved = s.settings.read().terminal.shell_order.clone();
        let profiles = ordered_shells(&self.profiles, &saved);
        egui::ScrollArea::vertical()
            .id_salt("terminal-shells-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("terminal-shells");
                let mut open_id: Option<String> = None;
                for (i, p) in profiles.iter().enumerate() {
                    let icon = shell_icon(&p.id);
                    reorder.row(ui, i, |ui| {
                        if hs_ui::form::list_row(
                            ui,
                            &theme,
                            p.id == current,
                            icon,
                            &p.name,
                            Some(&p.exe),
                        )
                        .tip(s.t("terminal-open-in-new-tab"))
                        .clicked()
                        {
                            open_id = Some(p.id.clone());
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    let mut ids: Vec<String> = profiles.iter().map(|p| p.id.clone()).collect();
                    hs_ui::reorder::move_item(&mut ids, from, to);
                    s.settings.write().terminal.shell_order = ids;
                    let _ = s.save_settings();
                    ui.ctx().request_repaint();
                }
                if let Some(id) = open_id {
                    cx.open(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd: None,
                            host: None,
                            profile: Some(id),
                            command: None,
                        }),
                    );
                }
                // CLI tools from Settings -> Terminal, below the shells.
                let cli: Vec<hs_core::settings::CliProfile> = s
                    .settings
                    .read()
                    .terminal
                    .cli
                    .iter()
                    .filter(|c| c.enabled)
                    .cloned()
                    .collect();
                if !cli.is_empty() {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(s.t("settings-terminal-cli"))
                            .small()
                            .strong()
                            .color(theme.text_muted),
                    );
                }
                let mut open_cli: Option<(String, Option<String>)> = None;
                let now = std::time::Instant::now();
                for c in &cli {
                    let found = match self.cli_found.get(&c.command) {
                        Some((at, found)) if now.duration_since(*at) < CLI_FOUND_TTL => *found,
                        _ => {
                            let found = hs_core::env_path::find(&c.command).is_some();
                            self.cli_found.insert(c.command.clone(), (now, found));
                            found
                        }
                    };
                    let line = if found {
                        c.command_line()
                    } else {
                        format!("{} - {}", c.command_line(), s.t("terminal-cli-not-found"))
                    };
                    let row = hs_ui::form::list_row(
                        ui,
                        &theme,
                        false,
                        cli_icon(&c.icon),
                        &c.name,
                        Some(&line),
                    );
                    if found && row.clicked() {
                        let cwd = if c.cwd.trim().is_empty() {
                            None
                        } else if c.cwd.trim() == "{{workspace}}" {
                            Some(s.workspace.read().root().to_string_lossy().to_string())
                        } else {
                            Some(c.cwd.trim().to_string())
                        };
                        open_cli = Some((c.command_line(), cwd));
                    }
                    ui.add_space(2.0);
                }
                if let Some((command, cwd)) = open_cli {
                    let shell = self
                        .state
                        .profile
                        .clone()
                        .or_else(|| {
                            let d = s.settings.read().terminal.default_profile.clone();
                            (!d.is_empty()).then_some(d)
                        })
                        .or_else(|| self.profiles.first().map(|p| p.id.clone()));
                    cx.open(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd,
                            host: None,
                            profile: shell,
                            command: Some(command),
                        }),
                    );
                }
                ui.add_space(10.0);
                hs_ui::form::note(
                    ui,
                    &theme,
                    hs_ui::icons::ph::LIGHTBULB,
                    &s.t("terminal-hint"),
                );
            });
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        // A cluster shell is opened on the runtime; take the answer at
        // frame start, as every background result arrives (ADR-013).
        self.drain_cluster(cx.services);
        // First draw of a real terminal tab: this is where the shell comes
        // up, since the tool is created before anyone knows which tab it
        // is.
        if self.session.is_none() && self.error.is_none() && self.cluster_pending.is_none() {
            self.start(cx.services);
        }
        // The assistant takes its column first, so the terminal is narrower
        // while it is open rather than partly hidden behind it.
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let outer = ui;
        let ui = &mut content;
        if let Some(cmd) = self.pending_command.take() {
            match &self.session {
                Some(sess) => sess.write(format!("{cmd}\r").as_bytes()),
                None => self.pending_command = Some(cmd),
            }
        }
        let s = cx.services;
        if let Some(sess) = &mut self.session {
            for ev in sess.drain_events() {
                match ev {
                    TermEvent::Bell => self.bell = true,
                    TermEvent::Exit => self.exit_code = sess.exited(),
                    TermEvent::ClipboardStore(text) => ui.ctx().copy_text(text),
                    TermEvent::Title(_) => {}
                }
            }
        }
        let theme = TermTheme::for_theme(&hs_ui::theme::Theme::from_settings(
            &s.settings.read().appearance,
        ));
        let mut find_requested = false;
        let mut link = None;
        match (&mut self.session, &self.error) {
            (Some(sess), _) => {
                if let Some(id) = self.pending_focus.take() {
                    ui.memory_mut(|m| m.request_focus(id));
                }
                let r = self.view.show(ui, sess, &theme);
                if r.clicked {
                    self.bell = false;
                }
                find_requested = r.find_requested;
                link = r.link;
                if let Some(code) = sess.exited().or(self.exit_code) {
                    let rect = ui.max_rect();
                    let overlay =
                        egui::Rect::from_center_size(rect.center(), egui::vec2(320.0, 80.0));
                    ui.painter().rect_filled(
                        overlay,
                        egui::CornerRadius::same(8),
                        egui::Color32::from_black_alpha(200),
                    );
                    ui.scope_builder(
                        egui::UiBuilder::new().max_rect(overlay.shrink(12.0)),
                        |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    s.i18n
                                        .t_args("terminal-exited", &[("code", &code.to_string())]),
                                );
                                if ui.button(s.t("terminal-restart")).clicked() {
                                    self.session = None;
                                    self.start(s);
                                }
                            });
                        },
                    );
                }
            }
            (None, Some(err)) => {
                ui.colored_label(egui::Color32::from_rgb(0xE5, 0x4D, 0x4D), err);
                if ui.button(s.t("terminal-restart")).clicked() {
                    self.start(s);
                }
            }
            (None, None) => {
                ui.spinner();
            }
        }
        if find_requested {
            self.open_find();
        }
        if let Some(link) = link {
            self.open_link(link, ui, cx);
        }
        if self.assistant.open {
            let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
            let ctx = self
                .session
                .as_ref()
                .map(|session| {
                    format!(
                        "Recent visible terminal output:\n{}",
                        session.visible_text(80)
                    )
                })
                .unwrap_or_else(|| {
                    format!(
                        "Terminal session title: {}\nExit code: {:?}",
                        self.title(),
                        self.exit_code
                    )
                });
            self.assistant.set_context("terminal state", ctx);
            if self.assistant.quick_actions.is_empty() {
                self.assistant.quick_actions =
                    hs_ui::ai_assistant::QuickAction::for_activity(s, "terminal");
            }
            let Some(panel) = assistant_rect else {
                // Too narrow to split: the toggle stays on and the column
                // returns when there is room for it.
                return;
            };
            let mut child = outer.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(outer.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = self.session.is_some();
                    self.assistant.can_run = self.session.is_some();
                    let out = self.assistant.ui(ui, s, &theme, "terminal");
                    if let Some(block) = out.use_block
                        && let Some(session) = &self.session
                    {
                        // Insert types it for review; Insert and Run adds the
                        // Return the user would press (ADR-019).
                        session.write(block.trim_end().as_bytes());
                        if out.run {
                            session.write(b"\r");
                        }
                    }
                });
        }
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        let mut out = Vec::new();
        if let Some(session) = &self.session {
            let screen = session.visible_text(200);
            if !screen.trim().is_empty() {
                out.push(hs_core::tool::Speakable::new("Terminal screen", screen));
            }
        }
        if let Some(a) = self.assistant.last_answer() {
            out.push(hs_core::tool::Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<hs_core::tool::Printable> {
        let session = self.session.as_ref()?;
        let screen = session.visible_text(10_000);
        Some(hs_core::tool::Printable {
            title: self.title().to_string(),
            subtitle: "Terminal".into(),
            html: hs_core::html::pre(&screen),
        })
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "terminal.ai".into(),
                title: "AI: Toggle Assistant".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "terminal.restart".into(),
                title: "Terminal: Restart".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "terminal.clear".into(),
                title: "Terminal: Clear".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "terminal.find".into(),
                title: "Terminal: Find".into(),
                shortcut_action: Some("terminal.find".into()),
            },
            CommandDescriptor {
                id: "terminal.new".into(),
                title: "Terminal: New Terminal".into(),
                shortcut_action: Some("terminal.new".into()),
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "terminal.ai" => {
                self.assistant.toggle("terminal");
                true
            }
            "terminal.restart" => {
                self.session = None;
                self.start(cx.services);
                true
            }
            "terminal.find" => {
                self.open_find();
                true
            }
            "terminal.clear" => {
                if let Some(s) = &self.session {
                    s.write(b"\x1b[3J\x1b[2J\x1b[H");
                }
                true
            }
            "terminal.new" => {
                cx.open(
                    "terminal",
                    Some(OpenRequest::Terminal {
                        cwd: self
                            .state
                            .cwd
                            .as_ref()
                            .map(|p| p.to_string_lossy().to_string()),
                        host: None,
                        profile: self.state.profile.clone(),
                        command: None,
                    }),
                );
                true
            }
            _ => false,
        }
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or_default()
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        if let Event::SettingsChanged = ev {
            self.apply_settings(cx.services);
        }
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Terminal {
                cwd,
                host,
                profile,
                command,
            } => {
                // A `k8s://` host means this tab runs a shell inside a
                // pod; anything else is a shell on this machine.
                self.state.cluster = host.as_deref().and_then(ClusterExec::parse);
                if let Some(p) = profile {
                    self.state.profile = Some(p);
                }
                if let Some(c) = cwd {
                    self.state.cwd = Some(PathBuf::from(c));
                }
                // Which CLI, if this is one: the command line is what
                // the sidebar's CLI rows are built from, so matching on
                // it needs no new field in `OpenRequest::Terminal` and
                // leaves every other tool's match arms alone.
                if let Some(cmd) = command.as_deref() {
                    let st = cx.services.settings.read();
                    self.state.icon = st
                        .terminal
                        .cli
                        .iter()
                        .find(|c| c.command_line() == cmd)
                        .map(|c| c.icon.clone());
                }
                self.session = None;
                self.start(cx.services);
                self.pending_command = command;
                true
            }
            OpenRequest::File { uri, .. } => {
                // "Open terminal here" with a file: use its folder.
                let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                let dir = if path.is_dir() {
                    Some(path)
                } else {
                    path.parent().map(|p| p.to_path_buf())
                };
                if dir.is_some() {
                    self.state.cwd = dir;
                    self.session = None;
                    self.start(cx.services);
                    true
                } else {
                    cx.toast(ToastLevel::Warning, "No folder to open a terminal in.");
                    false
                }
            }
            _ => false,
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// A glyph per shell family for the sidebar list.
/// Detected shells in the saved order: the ones named in `saved` first, in
/// that order, then anything else as detected — so installing a new shell
/// shows it without the saved list having to be rewritten, and uninstalling
/// one leaves no gap.
fn ordered_shells(detected: &[ShellProfile], saved: &[String]) -> Vec<ShellProfile> {
    let mut out: Vec<ShellProfile> = saved
        .iter()
        .filter_map(|id| detected.iter().find(|p| p.id == *id).cloned())
        .collect();
    for p in detected {
        if !out.iter().any(|q| q.id == p.id) {
            out.push(p.clone());
        }
    }
    out
}

/// The sidebar icon for a CLI profile's stored key.
fn cli_icon(key: &str) -> &'static str {
    use hs_ui::icons::ph;
    match key {
        "database" => ph::DATABASE,
        "cloud" => ph::CLOUD,
        "sparkle" => ph::SPARKLE,
        _ => ph::TERMINAL_WINDOW,
    }
}

fn shell_icon(id: &str) -> &'static str {
    use hs_ui::icons::ph;
    if id.starts_with("wsl:") {
        ph::LINUX_LOGO
    } else if id == "pwsh" || id == "powershell" {
        ph::TERMINAL_WINDOW
    } else if id == "cmd" {
        ph::WINDOWS_LOGO
    } else if id == "git-bash" {
        ph::GIT_BRANCH
    } else {
        ph::TERMINAL
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    fn p(id: &str) -> ShellProfile {
        ShellProfile::new(id, id, "", id, &[])
    }

    #[test]
    fn a_saved_shell_order_is_followed_and_new_shells_still_appear() {
        let detected = [p("pwsh"), p("cmd"), p("nu")];
        let ids = |v: Vec<ShellProfile>| v.into_iter().map(|s| s.id).collect::<Vec<_>>();

        // Nothing saved: detection order.
        assert_eq!(ids(ordered_shells(&detected, &[])), ["pwsh", "cmd", "nu"]);

        // Saved order wins, and a shell detected since then follows.
        let saved = ["nu".to_string(), "pwsh".to_string()];
        assert_eq!(
            ids(ordered_shells(&detected, &saved)),
            ["nu", "pwsh", "cmd"]
        );

        // A saved shell that is no longer installed leaves no gap.
        let saved = ["gone".to_string(), "cmd".to_string()];
        assert_eq!(
            ids(ordered_shells(&detected, &saved)),
            ["cmd", "pwsh", "nu"]
        );
    }
}
