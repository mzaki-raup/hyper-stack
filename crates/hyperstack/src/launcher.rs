//! The new-tab launcher page (docs/04 §3).

use egui::{CornerRadius, Vec2};
use hs_core::Services;
use hs_core::tool::{ToolCategory, ToolDescriptor};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

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
        "s3" | "webdav" | "nexus" => Some("s3"),
        "db" => Some("db"),
        "redis" | "valkey" => Some("redis"),
        "mongodb" | "couchdb" | "qdrant" => Some("nosql"),
        // Every container kind opens in the Container activity, so a
        // host pinned to Home or saved from Settings has somewhere to
        // go and its rail entry is un-hidden on save (ADR-026).
        "docker" | "podman" | "k8s" | "wincontainer" => Some("containers"),
        "rabbitmq" | "kafka" | "redis-streams" | "nats" | "mqtt" => Some("queue"),
        "jenkins" | "teamcity" | "github-actions" | "gitlab-ci" | "forgejo-actions" | "awx" => {
            Some("automation")
        }
        _ => None,
    }
}

/// The page: what you can do here. Where you were is the sidebar's job
/// (`side`), which is why this no longer draws Pinned or Recent.
pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    state: &mut LauncherState,
    tools: &[&ToolDescriptor],
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
    });
    resp
}

/// How wide one Recent entry is.
const RECENT_WIDTH: f32 = 230.0;

/// Home's sidebar: where you were, rather than what you can do. The page
/// answers "what can I do" — Get Started, the tools, the shortcuts — and
/// this column answers "what was I doing", which is the commonest reason
/// to be on Home at all. Owner's requirement, 2026-10-17.
pub fn side(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    tools: &[&ToolDescriptor],
    closed: &[(usize, String, String)],
) -> LauncherResponse {
    let mut resp = LauncherResponse {
        open_tool: None,
        reopen_closed: None,
        open_connection: None,
    };
    let pinned: Vec<_> = services.connections.read().pinned().cloned().collect();
    let recent: Vec<_> = closed.iter().rev().take(12).collect();
    if pinned.is_empty() && recent.is_empty() {
        hs_ui::form::note(
            ui,
            theme,
            hs_ui::icons::INFO,
            &services.t("launcher-side-empty"),
        );
        return resp;
    }
    egui::ScrollArea::vertical()
        .id_salt("home-side")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !pinned.is_empty() {
                section_heading(ui, theme, &services.t("launcher-pinned"));
                ui.add_space(6.0);
                // One section per connection type, in the order the
                // activity rail shows the tools that open them, so the
                // column reads the same way down as the rail does. Pins
                // whose kind no tool claims go last. Owner's requirement,
                // 2026-10-17.
                for (gi, (kind, group)) in
                    pinned_by_kind(&pinned, tools, services).iter().enumerate()
                {
                    if gi > 0 {
                        ui.add_space(8.0);
                        ui.separator();
                        ui.add_space(4.0);
                    }
                    let fill = kind_colour(theme, kind);
                    kind_heading(ui, theme, hs_core::connections::kind_label(kind), fill);
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        for c in group {
                            // The section says the type, so the pill is
                            // just the name; the colour carries the kind.
                            let tool = tool_for_kind(&c.kind)
                                .and_then(|id| tools.iter().find(|t| t.id == id));
                            match tool {
                                Some(t) => {
                                    let tip = services.i18n.t_args(
                                        "launcher-open-connection",
                                        &[("tool", &services.t(t.title_key))],
                                    );
                                    if pill(ui, &c.name, fill, theme.text).tip(tip).clicked() {
                                        resp.open_connection =
                                            Some((t.id.to_string(), c.id.clone()));
                                    }
                                }
                                None => {
                                    let tip = services.i18n.t_args(
                                        "launcher-connection-no-tool",
                                        &[("kind", hs_core::connections::kind_label(&c.kind))],
                                    );
                                    pill(ui, &c.name, fill, theme.text_muted).tip(tip);
                                }
                            }
                        }
                    });
                }
            }
            if !recent.is_empty() {
                if !pinned.is_empty() {
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(6.0);
                }
                section_heading(ui, theme, &services.t("launcher-recent"));
                ui.add_space(4.0);
                for (idx, factory, title) in recent {
                    let icon = tools
                        .iter()
                        .find(|t| t.id == factory)
                        .map(|t| t.icon)
                        .unwrap_or("\u{2022}");
                    // `add_sized` centres what it is given, which left a
                    // ragged column of titles down the middle of the
                    // sidebar. A growing spacer after the text holds the
                    // row at full width with the label against the left
                    // edge. Owner's report, 2026-10-17.
                    let r = ui.add(
                        egui::Button::new((
                            egui::Atom::from(format!("{icon}  {title}")),
                            egui::Atom::grow(),
                        ))
                        .frame(false)
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .min_size(Vec2::new(RECENT_WIDTH, 26.0)),
                    );
                    if r.tip(services.t("launcher-reopen")).clicked() {
                        resp.reopen_closed = Some(*idx);
                    }
                }
            }
        });
    resp
}

/// A section heading on Home. Bigger than the body text, because these
/// separate the page into parts and were previously easy to miss.
/// Pinned connections grouped by kind, the groups ordered the way the
/// activity rail orders the tools that open them. A kind no tool claims
/// sorts last, and ties fall back to the kind's own name so the order is
/// stable rather than however the store happened to list them.
fn pinned_by_kind<'a>(
    pinned: &'a [hs_core::connections::ConnectionProfile],
    tools: &[&ToolDescriptor],
    services: &Services,
) -> Vec<(String, Vec<&'a hs_core::connections::ConnectionProfile>)> {
    let saved = {
        let st = services.settings.read();
        services
            .workspace
            .read()
            .meta
            .rail_prefs(&st.appearance)
            .order
    };
    let ids: Vec<&str> = tools.iter().map(|t| t.id).collect();
    let rail = crate::app::rail_order(&ids, &saved);
    let rank = |kind: &str| -> usize {
        tool_for_kind(kind)
            .and_then(|id| rail.iter().position(|r| *r == id))
            .unwrap_or(usize::MAX)
    };
    let mut groups: Vec<(String, Vec<&hs_core::connections::ConnectionProfile>)> = Vec::new();
    for c in pinned {
        match groups.iter_mut().find(|(k, _)| *k == c.kind) {
            Some((_, v)) => v.push(c),
            None => groups.push((c.kind.clone(), vec![c])),
        }
    }
    groups.sort_by(|a, b| rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0)));
    groups
}

/// A connection type's heading: its name beside a dot in the type's own
/// colour, so the section and the pills under it read as one group.
fn kind_heading(ui: &mut egui::Ui, theme: &Theme, text: &str, colour: egui::Color32) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
        ui.painter().circle_filled(rect.center(), 4.0, colour);
        ui.label(
            egui::RichText::new(text)
                .size(11.0)
                .strong()
                .color(theme.text_muted),
        );
    });
}

fn section_heading(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .strong()
            .size(15.0)
            .color(theme.text),
    );
}

/// The colour a connection kind is shown in, so a glance at Home sorts the
/// databases from the shells without reading a word.
fn kind_colour(theme: &Theme, kind: &str) -> egui::Color32 {
    let c = match kind {
        "ssh" => egui::Color32::from_rgb(0x4A, 0x90, 0xD9),
        "db" => egui::Color32::from_rgb(0x8B, 0x5C, 0xF6),
        "redis" => egui::Color32::from_rgb(0xDC, 0x38, 0x2D),
        // Its own colour rather than Redis's: the pill exists to be
        // told apart at a glance, and "which of these two is the
        // Valkey" is exactly the question someone running both asks.
        "valkey" => egui::Color32::from_rgb(0x17, 0xA2, 0xB8),
        "mongodb" | "couchdb" => egui::Color32::from_rgb(0x10, 0xA3, 0x7B),
        "qdrant" => egui::Color32::from_rgb(0xD8, 0x3A, 0x7D),
        "s3" => egui::Color32::from_rgb(0xF5, 0x9E, 0x0B),
        // The object browser's other two stores, each its own colour:
        // the pill exists to be told apart at a glance.
        "webdav" => egui::Color32::from_rgb(0x64, 0x74, 0x8B),
        "nexus" => egui::Color32::from_rgb(0x1B, 0xA0, 0xE1),
        "api" => egui::Color32::from_rgb(0xEC, 0x48, 0x99),
        // The brokers share one colour, the Queue activity's: the pill
        // says "a message broker", and the icon says which.
        "rabbitmq" | "kafka" | "redis-streams" | "nats" | "mqtt" => {
            egui::Color32::from_rgb(0xFF, 0x66, 0x00)
        }
        // The build servers share the Automation activity's colour.
        "jenkins" | "teamcity" | "github-actions" | "gitlab-ci" | "forgejo-actions" | "awx" => {
            egui::Color32::from_rgb(0x22, 0xC5, 0x5E)
        }
        _ => theme.secondary,
    };
    // The pill is a tint of that colour, not the colour itself, so the
    // label stays readable on it.
    c.linear_multiply(0.45)
}

/// A connection pill: bigger than a badge, because these are the main
/// thing on Home and were previously the smallest.
fn pill(
    ui: &mut egui::Ui,
    text: &str,
    fill: egui::Color32,
    colour: egui::Color32,
) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), colour);
    let size = Vec2::new(galley.size().x + 22.0, 28.0);
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    let bg = if r.hovered() {
        fill.linear_multiply(1.4)
    } else {
        fill
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(14), bg);
    ui.painter().galley(
        egui::Pos2::new(rect.left() + 11.0, rect.center().y - galley.size().y / 2.0),
        galley,
        colour,
    );
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    r
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
        // One tool opens both document stores.
        assert_eq!(tool_for_kind("mongodb"), Some("nosql"));
        assert_eq!(tool_for_kind("couchdb"), Some("nosql"));
        // Every broker kind opens in the Queue activity.
        for k in hs_core::connections::QUEUE_KINDS {
            assert_eq!(tool_for_kind(k), Some("queue"), "{k}");
        }
        for k in hs_core::connections::AUTOMATION_KINDS {
            assert_eq!(tool_for_kind(k), Some("automation"), "{k}");
        }
        // No tool opens these kinds yet.
        assert_eq!(tool_for_kind("cassandra"), None);
        assert_eq!(tool_for_kind("elasticsearch"), None);
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
        let redis_id = redis.id.clone();
        // A third, left unpinned: Home shows what was pinned, not
        // everything that exists.
        let other = ConnectionProfile {
            name: "not-pinned".into(),
            kind: "redis".into(),
            ..Default::default()
        };
        {
            let mut c = services.connections.write();
            c.upsert(ssh).unwrap();
            c.upsert(redis).unwrap();
            c.upsert(other).unwrap();
            c.set_pinned(&ssh_id, true).unwrap();
            c.set_pinned(&redis_id, true).unwrap();
        }

        let theme = Theme::dark();
        let mut opened = None;
        let mut harness = Harness::new_ui(|ui| {
            let r = side(ui, &theme, &services, &[&SSH], &[]);
            if r.open_connection.is_some() {
                opened = r.open_connection;
            }
        });
        harness.run();
        // Only pinned connections are on Home.
        assert!(
            harness.query_by_label("not-pinned").is_none(),
            "an unpinned connection should not be on Home"
        );
        // The pill is the connection's name: its type is said once, by the
        // heading of the group it sits in.
        assert!(
            harness.query_by_label("SSH").is_some(),
            "each group is headed by its connection type"
        );
        // The redis chip has no tool here, so clicking it opens nothing.
        harness.get_by_label("cache").click();
        harness.run();
        harness.get_by_label("web-01").click();
        harness.run();
        drop(harness);
        assert_eq!(opened, Some(("ssh".to_string(), ssh_id)));
    }
}
