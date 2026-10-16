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
    /// (tool factory id, connection profile id) from a pinned-connection chip.
    pub open_connection: Option<(String, String)>,
}

/// The tool that opens connection profiles of `kind`, or `None` while no
/// tool handles that kind yet (redis, mongodb, …).
pub fn tool_for_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "ssh" => Some("ssh"),
        "s3" => Some("s3"),
        "db" => Some("db"),
        "redis" => Some("redis"),
        _ => None,
    }
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
        open_connection: None,
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
                    let text = format!("{}  {}", hs_core::connections::kind_label(&c.kind), c.name);
                    let fill = col.linear_multiply(0.35);
                    let tool =
                        tool_for_kind(&c.kind).and_then(|id| tools.iter().find(|t| t.id == id));
                    match tool {
                        Some(t) => {
                            let tip = services.i18n.t_args(
                                "launcher-open-connection",
                                &[("tool", &services.t(t.title_key))],
                            );
                            if hs_ui::badge::badge_button(ui, &text, fill, theme.text)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                resp.open_connection = Some((t.id.to_string(), c.id.clone()));
                            }
                        }
                        None => {
                            let tip = services.i18n.t_args(
                                "launcher-connection-no-tool",
                                &[("kind", hs_core::connections::kind_label(&c.kind))],
                            );
                            hs_ui::badge::badge(ui, &text, fill, theme.text_muted)
                                .on_hover_text(tip);
                        }
                    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use hs_core::connections::ConnectionProfile;
    use hs_core::paths::DataDir;
    use hs_core::runtime::NoopWaker;
    use hs_core::secrets::KdfParams;
    use std::sync::Arc;

    const SSH: ToolDescriptor = ToolDescriptor {
        id: "ssh",
        title_key: "tool-ssh",
        icon: "S",
        category: ToolCategory::Infra,
        accepts: &[],
    };

    #[test]
    fn kinds_map_to_their_tools() {
        assert_eq!(tool_for_kind("ssh"), Some("ssh"));
        assert_eq!(tool_for_kind("s3"), Some("s3"));
        assert_eq!(tool_for_kind("db"), Some("db"));
        assert_eq!(tool_for_kind("redis"), Some("redis"));
        // No tool opens these kinds yet.
        assert_eq!(tool_for_kind("mongodb"), None);
        assert_eq!(tool_for_kind("cassandra"), None);
    }

    #[test]
    fn clicking_a_chip_opens_its_connection() {
        let dir = tempfile::tempdir().unwrap();
        let paths = DataDir::resolve(Some(dir.path())).unwrap();
        let boot = crate::bootstrap::services(paths, Arc::new(NoopWaker), KdfParams::FAST).unwrap();
        let services = boot.services;
        let ssh = ConnectionProfile {
            name: "web-01".into(),
            kind: "ssh".into(),
            ..Default::default()
        };
        let ssh_id = ssh.id.clone();
        let redis = ConnectionProfile {
            name: "cache".into(),
            kind: "redis".into(),
            ..Default::default()
        };
        {
            let mut c = services.connections.write();
            c.upsert(ssh).unwrap();
            c.upsert(redis).unwrap();
        }

        let theme = Theme::dark();
        let mut state = LauncherState::default();
        let mut opened = None;
        let mut harness = Harness::new_ui(|ui| {
            let r = show(ui, &theme, &services, &mut state, &[&SSH], &[]);
            if r.open_connection.is_some() {
                opened = r.open_connection;
            }
        });
        harness.run();
        // The redis chip is a plain label: clicking it opens nothing.
        harness.get_by_label("Redis  cache").click();
        harness.run();
        harness.get_by_label("SSH  web-01").click();
        harness.run();
        drop(harness);
        assert_eq!(opened, Some(("ssh".to_string(), ssh_id)));
    }
}
