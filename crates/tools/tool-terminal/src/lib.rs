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
}

pub struct TerminalTool {
    state: State,
    profiles: Arc<Vec<ShellProfile>>,
    session: Option<Session>,
    view: TerminalView,
    error: Option<String>,
    exit_code: Option<u32>,
    bell: bool,
    tab_id_hint: u64,
    find: Find,
    /// Give the terminal keyboard focus on the next frame.
    pending_focus: Option<egui::Id>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

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
            profiles,
            session: None,
            view: TerminalView::new(egui::Id::new(("hs-terminal", hs_core::new_id())), font_size),
            error: None,
            exit_code: None,
            bell: false,
            tab_id_hint: 0,
            find: Find::default(),
            pending_focus: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        };
        t.apply_settings(services);
        t.start(services);
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
            label.on_hover_text(e.clone());
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
            LinkTarget::Url(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
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

    fn start(&mut self, services: &Services) {
        self.error = None;
        self.exit_code = None;
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
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
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
        let current = self.state.profile.clone().unwrap_or_default();
        let current_name = self
            .profiles
            .iter()
            .find(|p| p.id == current)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| s.t("terminal-profile"));
        let profiles = self.profiles.clone();
        let mut picked: Option<String> = None;
        egui::ComboBox::from_id_salt(("term-profile", self.tab_id_hint))
            .selected_text(current_name)
            .show_ui(ui, |ui| {
                for p in profiles.iter() {
                    if ui.selectable_label(p.id == current, &p.name).clicked() {
                        picked = Some(p.id.clone());
                    }
                }
            });
        if let Some(id) = picked {
            // A new profile opens a new tab; the current session stays.
            cx.open(
                "terminal",
                Some(OpenRequest::Terminal {
                    cwd: self
                        .state
                        .cwd
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    host: None,
                    profile: Some(id),
                }),
            );
        }
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
        if ui.button(s.t("terminal-clear")).clicked()
            && let Some(sess) = &self.session
        {
            // Clear screen and scrollback (xterm: ESC c would reset; use CSI 3J + 2J + H).
            sess.write(b"\x1b[3J\x1b[2J\x1b[H");
        }
        if let Some(sess) = &self.session {
            let (c, r) = sess.size();
            ui.label(egui::RichText::new(format!("{c}×{r}")).weak());
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
        if ui
            .button(format!("{} AI", hs_ui::icons::ph::SPARKLE))
            .tip(s.t("ai-shared-button-tip"))
            .clicked()
        {
            self.assistant.toggle("terminal");
        }
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
        let profiles = self.profiles.clone();
        let current = self.state.profile.clone().unwrap_or_default();
        for p in profiles.iter() {
            let icon = shell_icon(&p.id);
            let r = hs_ui::form::list_row(ui, &theme, p.id == current, icon, &p.name, Some(&p.exe));
            if r.tip(s.t("terminal-open-in-new-tab")).clicked() {
                cx.open(
                    "terminal",
                    Some(OpenRequest::Terminal {
                        cwd: None,
                        host: None,
                        profile: Some(p.id.clone()),
                    }),
                );
            }
            ui.add_space(2.0);
        }
        ui.add_space(10.0);
        hs_ui::form::note(
            ui,
            &theme,
            hs_ui::icons::ph::LIGHTBULB,
            &s.t("terminal-hint"),
        );
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
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
            let rect = ui.available_rect_before_wrap();
            let panel = egui::Rect::from_min_max(
                egui::pos2(rect.right() - 390.0, rect.top()),
                rect.right_bottom(),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(ui.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = self.session.is_some();
                    let out = self.assistant.ui(ui, s, &theme, "terminal");
                    if let Some(block) = out.use_block
                        && let Some(session) = &self.session
                    {
                        // Typed, not run: the user presses Enter after review.
                        session.write(block.trim_end().as_bytes());
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
            OpenRequest::Terminal { cwd, profile, .. } => {
                if let Some(p) = profile {
                    self.state.profile = Some(p);
                }
                if let Some(c) = cwd {
                    self.state.cwd = Some(PathBuf::from(c));
                }
                self.session = None;
                self.start(cx.services);
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
