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
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        Cow::Borrowed("Home")
    }

    /// A row of activity icons: the quickest way from Home to anywhere.
    ///
    /// The same entries the rail shows for this workspace and in the same
    /// order (Settings → Appearance → Activity Rail is per workspace, so a
    /// hidden activity is hidden here too), minus Home itself — you are
    /// on it — and the sidebar toggle, which is a view switch rather than
    /// an activity. **Settings** joins them at the end, after a
    /// separator, as it does at the foot of the rail.
    ///
    /// Icons only, with the activity's name on hover: the names are right
    /// there under the rail's icons, and a second row of labels would be
    /// a worse version of the launcher further down the page. The colour
    /// follows Settings → Appearance → Sidebar → Coloured Activity Icons,
    /// so the two columns of icons never disagree. Owner's requirement,
    /// 2026-10-25.
    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (coloured, outlined, order, hidden) = {
            let st = s.settings.read();
            let prefs = s.workspace.read().meta.rail_prefs(&st.appearance);
            (
                st.appearance.rail_coloured,
                st.appearance.rail_icon_outline,
                prefs.order,
                prefs.hidden,
            )
        };
        // Room to breathe between the icons; the row is a set of targets,
        // not a word.
        ui.spacing_mut().item_spacing.x = 8.0;
        let mut hub = self.hub.lock();
        let hub = &mut *hub;
        let ids: Vec<&'static str> = hub.tools.iter().map(|t| t.id).collect();
        let ordered: Vec<&'static str> = crate::app::rail_order(&ids, &order)
            .into_iter()
            .filter(|id| !hidden.iter().any(|h| h == id))
            .collect();
        let mut open: Option<String> = None;
        let mut icon_button = |ui: &mut egui::Ui, id: &str| {
            let Some(d) = hub.tools.iter().find(|t| t.id == id) else {
                return;
            };
            let colour = if coloured {
                crate::app::activity_colour(id)
            } else {
                theme.text
            };
            let label = s.t(d.title_key);
            // Painted rather than a `Button` with a `RichText` label:
            // the outline is eight offset copies of the glyph under the
            // real one, and egui lays a button's text out itself.
            let (rect, r) = ui.allocate_exact_size(egui::vec2(34.0, 34.0), egui::Sense::click());
            // Hovering says "this is the one you are about to open": a
            // filled plate and an accent edge, which a bare colour change
            // on a coloured glyph could not say (owner's requirement,
            // 2026-10-26).
            if r.hovered() {
                let radius = egui::CornerRadius::same(8);
                ui.painter().rect_filled(rect, radius, theme.surface_hover);
                ui.painter().rect_stroke(
                    rect,
                    radius,
                    egui::Stroke::new(1.0, theme.accent),
                    egui::StrokeKind::Inside,
                );
            }
            hs_ui::icons::outlined_text(
                ui.painter(),
                rect.center(),
                egui::Align2::CENTER_CENTER,
                d.icon,
                egui::FontId::proportional(22.0),
                colour,
                // As in the rail: the ring is for a coloured glyph. A
                // plain one is the theme's text colour already.
                if coloured && outlined {
                    theme.icon_outline()
                } else {
                    egui::Color32::TRANSPARENT
                },
                crate::app::ICON_OUTLINE_WIDTH,
            );
            // A painted control has no AccessKit node of its own, so a
            // screen reader and `egui_kittest` would both miss it.
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            if r.on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&label)
                .clicked()
            {
                open = Some(id.to_string());
            }
        };
        for id in &ordered {
            icon_button(ui, id);
        }
        // Settings is not in the rail's own order (it is pinned to the
        // foot of the rail), so it is added here rather than filtered in.
        ui.separator();
        icon_button(ui, "settings");
        if let Some(id) = open {
            // No request with it: `open_tool` then focuses the activity's
            // dashboard home tab, which is where this is meant to land -
            // and for Settings, the page it was last left on.
            hub.requests.push(HomeRequest::OpenTool(id));
        }
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
                        let r = launcher::show(ui, &theme, s, &mut self.launcher, &descs);
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

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let mut hub = self.hub.lock();
        let hub = &mut *hub;
        let (ws, root) = {
            let w = s.workspace.read();
            (w.meta.name.clone(), w.root().to_path_buf())
        };
        hs_ui::form::sidebar_header_icon(
            ui,
            &theme,
            crate::app::ws_logo_image(&root),
            hs_ui::icons::WORKSPACE,
            &ws,
            &[],
        );
        ui.add_space(4.0);
        let descs: Vec<&ToolDescriptor> = hub.tools.iter().collect();
        let r = launcher::side(ui, &theme, s, &descs, &hub.closed);
        if let Some(i) = r.reopen_closed {
            hub.requests.push(HomeRequest::ReopenClosed(i));
        }
        if let Some((tool, id)) = r.open_connection {
            hub.requests.push(HomeRequest::OpenConnection { tool, id });
        }
        true
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
    // In the order someone sets a machine up: a workspace to put things
    // in, the environments they vary by, the connections that use them,
    // then the day's work. Owner's order, 2026-10-17.
    let cards: [(&str, &str, &str, HomeRequest); 6] = [
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
            ph::PLUGS_CONNECTED,
            "home-new-connection",
            "home-new-connection-hint",
            HomeRequest::NewConnection,
        ),
        (
            ph::FOLDER_OPEN,
            "home-open-folder",
            "home-open-folder-hint",
            HomeRequest::OpenFolderAsProject,
        ),
        (
            ph::TERMINAL_WINDOW,
            "home-new-terminal",
            "home-new-terminal-hint",
            HomeRequest::OpenTool("terminal".into()),
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
    let workspaces_dir = s.paths.workspaces_dir();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
        for name in hub.workspaces.clone() {
            let selected = name == current;
            let fill = if selected {
                theme.accent.linear_multiply(0.45)
            } else {
                theme.surface
            };
            // The workspace's own logo when it has one, else the folder.
            let logo = hs_core::workspace::workspace_icon_file(&workspaces_dir.join(&name))
                .and_then(|p| crate::app::logo_image(&p));
            let r = workspace_card(
                ui,
                theme,
                &name,
                logo,
                hs_ui::icons::WORKSPACE,
                fill,
                selected,
            )
            .on_hover_text(if selected {
                s.t("home-workspace-current")
            } else {
                s.t("home-workspace-switch")
            });
            if r.clicked() && !selected {
                hub.requests.push(HomeRequest::SwitchWorkspace(name));
            }
        }
        // A gap and a plus, so adding a workspace does not read as one
        // more workspace in the row.
        ui.add_space(14.0);
        if workspace_card(
            ui,
            theme,
            &s.t("workspace-new"),
            None,
            hs_ui::icons::ph::PLUS_CIRCLE,
            theme.panel_bg,
            false,
        )
        .on_hover_text(s.t("home-new-workspace-tip"))
        .clicked()
        {
            hub.requests.push(HomeRequest::NewWorkspace);
        }
    });
    ui.add_space(8.0);
}

/// One workspace in the list: a 26 px logo (`glyph` when it has none)
/// and the name at 15 px on a pill that highlights the open workspace.
/// Every card is the same height, whether it carries an image or a glyph,
/// so a row of them lines up.
fn workspace_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    logo: Option<egui::Image<'static>>,
    glyph: &str,
    fill: egui::Color32,
    selected: bool,
) -> egui::Response {
    // The logo is what tells one workspace card from another.
    let icon_px = crate::app::WS_MARK_LIST;
    let resp = egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(
            1.0,
            if selected { theme.accent } else { theme.border },
        ))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(14, 9))
        .show(ui, |ui| {
            // A fixed height, and contents centred inside it: an image and
            // a glyph otherwise give the card two different sizes and the
            // row sits unevenly.
            ui.set_min_height(icon_px);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                match logo {
                    Some(img) => {
                        ui.add(
                            img.fit_to_exact_size(egui::vec2(icon_px, icon_px))
                                .corner_radius(4.0),
                        );
                    }
                    None => {
                        ui.label(egui::RichText::new(glyph).size(icon_px * 0.85).color(
                            if selected {
                                theme.text
                            } else {
                                theme.text_muted
                            },
                        ));
                    }
                }
                ui.label(
                    egui::RichText::new(name)
                        .size(15.0)
                        .strong()
                        .color(theme.text),
                );
            });
        })
        .response;
    let resp = resp.interact(egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(
            resp.rect,
            egui::CornerRadius::same(8),
            egui::Stroke::new(1.0, theme.accent),
            egui::StrokeKind::Outside,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
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
