//! The new-tab launcher page (docs/04 §3).

use egui::{CornerRadius, Vec2};
use hs_core::Services;
use hs_core::tool::{ToolCategory, ToolDescriptor};
use hs_ui::theme::Theme;

#[derive(Default)]
pub struct LauncherState {
    pub query: String,
    pub focus_search: bool,
}

pub struct LauncherResponse {
    pub open_tool: Option<String>,
    pub reopen_closed: Option<usize>,
}

pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    state: &mut LauncherState,
    tools: &[&ToolDescriptor],
    closed: &[(usize, String, String)], // index, factory id, title
) -> LauncherResponse {
    let mut resp = LauncherResponse {
        open_tool: None,
        reopen_closed: None,
    };
    let width = ui.available_width().min(960.0);
    ui.vertical_centered(|ui| {
        ui.set_width(width);
        ui.add_space(24.0);
        ui.heading(services.t("launcher-title"));
        ui.add_space(8.0);
        let edit = egui::TextEdit::singleline(&mut state.query)
            .hint_text(services.t("launcher-search"))
            .desired_width(width.min(520.0));
        let r = ui.add(edit);
        if state.focus_search {
            r.request_focus();
            state.focus_search = false;
        }
        ui.add_space(16.0);

        let q = state.query.trim();
        let matching: Vec<&ToolDescriptor> = if q.is_empty() {
            tools.to_vec()
        } else {
            let titles: Vec<(String, Option<String>)> = tools
                .iter()
                .map(|t| (services.t(t.title_key), Some(t.id.to_string())))
                .collect();
            let refs: Vec<(&str, Option<&str>)> = titles
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_deref()))
                .collect();
            hs_core::fuzzy::rank(q, &refs)
                .into_iter()
                .map(|m| tools[m.index])
                .collect()
        };
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(first) = matching.first()
        {
            resp.open_tool = Some(first.id.to_string());
        }
        if matching.is_empty() {
            ui.colored_label(theme.text_muted, services.t("launcher-no-tools"));
        }

        ui.horizontal_top(|ui| {
            for cat in ToolCategory::all() {
                let in_cat: Vec<&&ToolDescriptor> =
                    matching.iter().filter(|t| t.category == cat).collect();
                if in_cat.is_empty() {
                    continue;
                }
                ui.vertical(|ui| {
                    ui.set_width((width / 5.0).max(150.0) - 8.0);
                    ui.label(
                        egui::RichText::new(services.t(cat.label_key()))
                            .strong()
                            .color(theme.text_muted),
                    );
                    ui.add_space(4.0);
                    for t in in_cat {
                        if tool_card(ui, theme, t.icon, &services.t(t.title_key)).clicked() {
                            resp.open_tool = Some(t.id.to_string());
                        }
                    }
                });
            }
        });

        if !closed.is_empty() {
            ui.add_space(20.0);
            ui.separator();
            ui.label(
                egui::RichText::new(services.t("launcher-recent"))
                    .strong()
                    .color(theme.text_muted),
            );
            ui.add_space(4.0);
            for (idx, factory, title) in closed.iter().rev().take(8) {
                let icon = tools
                    .iter()
                    .find(|t| t.id == factory)
                    .map(|t| t.icon)
                    .unwrap_or("•");
                if ui
                    .add(egui::Button::new(format!("{icon}  {title}")).frame(false))
                    .clicked()
                {
                    resp.reopen_closed = Some(*idx);
                }
            }
        }

        let conns = services.connections.read();
        if !conns.all().is_empty() {
            ui.add_space(20.0);
            ui.separator();
            ui.label(
                egui::RichText::new(services.t("launcher-pinned"))
                    .strong()
                    .color(theme.text_muted),
            );
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for c in conns.all() {
                    let col = c
                        .colour
                        .as_deref()
                        .and_then(hs_ui::theme::parse_hex)
                        .unwrap_or(theme.secondary);
                    hs_ui::badge::badge(
                        ui,
                        &format!("{}  {}", hs_core::connections::kind_label(&c.kind), c.name),
                        col.linear_multiply(0.35),
                        theme.text,
                    );
                }
            });
        }
    });
    resp
}

fn tool_card(ui: &mut egui::Ui, theme: &Theme, icon: &str, title: &str) -> egui::Response {
    let (rect, r) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), egui::Sense::click());
    let fill = if r.hovered() {
        theme.surface_hover
    } else {
        theme.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(18.0),
        theme.accent,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 40.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.0),
        theme.text,
    );
    ui.add_space(4.0);
    r
}
