//! Home: the pinned first tab of every workspace (docs/04 §3). A dashboard
//! with the things people start with — open a folder as a project, add a
//! connection, create or switch a workspace, open a tool — plus the recent
//! tabs, the pinned connections and the keys worth knowing. It replaced the
//! standalone Welcome tab and Settings → Welcome (owner's request,
//! 2026-10-15).
//!
//! The page talks to the shell through `SharedHome`: the shell fills in what
//! only it knows (tool descriptors, recently closed tabs) and drains the
//! requests the page produced.

use std::borrow::Cow;

use hs_core::Services;
use hs_core::tool::{
    CommandDescriptor, Tool, ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::theme::Theme;

use crate::launcher::{self, LauncherState};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "home",
    title_key: "tool-home",
    icon: hs_ui::icons::ph::HOUSE,
    category: ToolCategory::Utilities,
    accepts: &[],
};

/// What the Home page asks the shell to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HomeRequest {
    OpenTool(String),
    ReopenClosed(usize),
    OpenConnection { tool: String, id: String },
    OpenFolderAsProject,
    NewConnection,
    NewWorkspace,
    SwitchWorkspace(String),
    OpenSettingsPage(&'static str),
    RunAction(&'static str),
}

#[derive(Default)]
pub struct HomeHub {
    /// Filled by the shell once: every registered tool.
    pub tools: Vec<ToolDescriptor>,
    /// Filled by the shell each frame: (index, factory id, title).
    pub closed: Vec<(usize, String, String)>,
    /// Workspace names from the registry, current first.
    pub workspaces: Vec<String>,
    pub requests: Vec<HomeRequest>,
}

pub type SharedHome = std::sync::Arc<parking_lot::Mutex<HomeHub>>;

pub struct HomeFactory {
    pub hub: SharedHome,
}

impl ToolFactory for HomeFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, _services: &Services, _restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        Box::new(HomeTool {
            hub: self.hub.clone(),
            launcher: LauncherState::default(),
        })
    }
}

pub struct HomeTool {
    hub: SharedHome,
    launcher: LauncherState,
}

impl Tool for HomeTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        Cow::Borrowed("Home")
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let mut hub = self.hub.lock();
        let hub = &mut *hub;
        egui::ScrollArea::vertical()
            .id_salt("hs-home")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(24, 16))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        header(ui, &theme, s);
                        get_started(ui, &theme, s, hub);
                        workspaces(ui, &theme, s, hub);
                        ui.add_space(12.0);
                        let descs: Vec<&ToolDescriptor> = hub
                            .tools
                            .iter()
                            .filter(|t| t.id != "home" && t.id != "settings")
                            .collect();
                        let r =
                            launcher::show(ui, &theme, s, &mut self.launcher, &descs, &hub.closed);
                        if let Some(id) = r.open_tool {
                            hub.requests.push(HomeRequest::OpenTool(id));
                        }
                        if let Some(i) = r.reopen_closed {
                            hub.requests.push(HomeRequest::ReopenClosed(i));
                        }
                        if let Some((tool, id)) = r.open_connection {
                            hub.requests.push(HomeRequest::OpenConnection { tool, id });
                        }
                        keys(ui, &theme, s);
                    });
            });
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![CommandDescriptor {
            id: "home.show".into(),
            title: "Home: Show Home".into(),
            shortcut_action: None,
        }]
    }

    fn run_command(&mut self, id: &str, _cx: &mut ToolContext) -> bool {
        id == "home.show"
    }
}

fn header(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    ui.horizontal(|ui| {
        ui.add(
            egui::Image::from_bytes(
                "bytes://hyperstack-logo-home.png",
                include_bytes!("../../../../assets/icon/hyperstack-64.png").as_slice(),
            )
            .fit_to_exact_size(egui::vec2(40.0, 40.0)),
        );
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(s.t("app-name"))
                    .size(24.0)
                    .strong()
                    .color(theme.accent),
            );
            ui.label(egui::RichText::new(s.t("app-tagline")).color(theme.text_muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("v{}", s.version))
                    .monospace()
                    .color(theme.text_muted),
            );
        });
    });
    ui.add_space(14.0);
}

fn section_title(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(13.5)
            .color(theme.text_muted),
    );
    ui.add_space(6.0);
}

fn get_started(ui: &mut egui::Ui, theme: &Theme, s: &Services, hub: &mut HomeHub) {
    use hs_ui::icons::ph;
    section_title(ui, theme, &s.t("home-get-started"));
    let cards: [(&str, &str, &str, HomeRequest); 6] = [
        (
            ph::FOLDER_OPEN,
            "home-open-folder",
            "home-open-folder-hint",
            HomeRequest::OpenFolderAsProject,
        ),
        (
            ph::PLUGS_CONNECTED,
            "home-new-connection",
            "home-new-connection-hint",
            HomeRequest::NewConnection,
        ),
        (
            ph::TERMINAL_WINDOW,
            "home-new-terminal",
            "home-new-terminal-hint",
            HomeRequest::OpenTool("terminal".into()),
        ),
        (
            ph::FOLDERS,
            "home-new-workspace",
            "home-new-workspace-hint",
            HomeRequest::NewWorkspace,
        ),
        (
            ph::GLOBE_HEMISPHERE_WEST,
            "home-environments",
            "home-environments-hint",
            HomeRequest::OpenSettingsPage("settings-environments"),
        ),
        (
            ph::MAGNIFYING_GLASS,
            "home-quick-open",
            "home-quick-open-hint",
            HomeRequest::RunAction("palette.quick_open"),
        ),
    ];
    let avail = ui.available_width();
    let cols = if avail > 900.0 {
        3
    } else if avail > 560.0 {
        2
    } else {
        1
    };
    let gap = 10.0;
    let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(200.0);
    let mut i = 0;
    while i < cards.len() {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (icon, title, hint, req) in cards.iter().skip(i).take(cols) {
                if action_card(ui, theme, w, icon, &s.t(title), &s.t(hint)).clicked() {
                    hub.requests.push(req.clone());
                }
            }
        });
        ui.add_space(gap);
        i += cols;
    }
    ui.add_space(8.0);
}

fn action_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    icon: &str,
    title: &str,
    hint: &str,
) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(width, 64.0), egui::Sense::click());
    let fill = if r.hovered() {
        theme.surface_hover
    } else {
        theme.surface
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(8), fill);
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(8),
        egui::Stroke::new(
            1.0,
            if r.hovered() {
                theme.accent
            } else {
                theme.border
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 16.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(24.0),
        theme.accent,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 52.0, rect.center().y - 9.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.5),
        theme.text,
    );
    let hint_galley = ui.painter().layout(
        hint.to_string(),
        egui::FontId::proportional(11.5),
        theme.text_muted,
        width - 64.0,
    );
    ui.painter().galley(
        egui::pos2(rect.left() + 52.0, rect.center().y + 2.0),
        hint_galley,
        theme.text_muted,
    );
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn workspaces(ui: &mut egui::Ui, theme: &Theme, s: &Services, hub: &mut HomeHub) {
    section_title(ui, theme, &s.t("home-workspaces"));
    let current = s.workspace.read().meta.name.clone();
    ui.horizontal_wrapped(|ui| {
        for name in hub.workspaces.clone() {
            let selected = name == current;
            let fill = if selected {
                theme.accent.linear_multiply(0.45)
            } else {
                theme.surface
            };
            let text = format!("{}  {name}", hs_ui::icons::WORKSPACE);
            if hs_ui::badge::badge_button(ui, &text, fill, theme.text)
                .on_hover_text(if selected {
                    s.t("home-workspace-current")
                } else {
                    s.t("home-workspace-switch")
                })
                .clicked()
                && !selected
            {
                hub.requests.push(HomeRequest::SwitchWorkspace(name));
            }
        }
        if hs_ui::badge::badge_button(
            ui,
            &format!("{} {}", hs_ui::icons::TAB_NEW, s.t("workspace-new")),
            theme.surface,
            theme.text_muted,
        )
        .clicked()
        {
            hub.requests.push(HomeRequest::NewWorkspace);
        }
    });
    ui.add_space(8.0);
}

fn keys(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    ui.add_space(20.0);
    ui.separator();
    section_title(ui, theme, &s.t("home-keys"));
    let km = s.shortcuts.read();
    ui.horizontal_wrapped(|ui| {
        for (action, label) in [
            ("palette.quick_open", "home-key-quick-open"),
            ("palette.commands", "home-key-commands"),
            ("tab.new", "home-key-new-tab"),
            ("view.dock", "home-key-dock"),
            ("app.settings", "home-key-settings"),
        ] {
            let combo = km.get(action).map(|c| c.display()).unwrap_or("—".into());
            egui::Frame::new()
                .fill(theme.surface)
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(combo).monospace().strong());
                    ui.label(egui::RichText::new(s.t(label)).color(theme.text_muted));
                });
        }
    });
}
