//! Terminal tool (docs/tools/terminal.md): one shell session per tab, shell
//! profiles in the sidebar, restart on exit, "open here" via `OpenRequest`.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_term::session::{Session, TermEvent};
use hs_term::shells::ShellProfile;
use hs_term::widget::{TermTheme, TerminalView};
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
}

impl TerminalTool {
    fn new(services: &Services, state: State) -> Self {
        let profiles = Arc::new(hs_term::shells::detect());
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
        };
        t.view.copy_on_select = services.settings.read().terminal.copy_on_select;
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
        match &self.session {
            Some(s) => Cow::Borrowed(s.title.as_str()),
            None => Cow::Borrowed("Terminal"),
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
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        ui.label(egui::RichText::new(s.t("terminal-profiles")).strong());
        ui.add_space(4.0);
        let profiles = self.profiles.clone();
        for p in profiles.iter() {
            if ui
                .add(
                    egui::Button::new(format!("{}  {}", hs_ui::icons::ph::TERMINAL, p.name))
                        .frame(false),
                )
                .on_hover_text(&p.exe)
                .clicked()
            {
                cx.open(
                    "terminal",
                    Some(OpenRequest::Terminal {
                        cwd: None,
                        host: None,
                        profile: Some(p.id.clone()),
                    }),
                );
            }
        }
        ui.add_space(8.0);
        ui.label(egui::RichText::new(s.t("terminal-hint")).weak());
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
        let dark = s.settings.read().appearance.theme != "light";
        let theme = if dark {
            TermTheme::dark()
        } else {
            TermTheme::light()
        };
        match (&mut self.session, &self.error) {
            (Some(sess), _) => {
                let r = self.view.show(ui, sess, &theme);
                if r.clicked {
                    self.bell = false;
                }
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
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
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
                id: "terminal.new".into(),
                title: "Terminal: New Terminal".into(),
                shortcut_action: Some("terminal.new".into()),
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "terminal.restart" => {
                self.session = None;
                self.start(cx.services);
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
