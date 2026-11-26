//! The dashboard on the activity's home tab (§3.1): the band, the figures,
//! Start Here, Saved Servers and Open Here. Recent (the items opened here)
//! comes with the items it records.

use std::collections::HashMap;
use std::sync::LazyLock;

use hs_core::tool::{Action, OpenRequest, ToolContext};
use hs_ui::dashboard as dash;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::AnalyticsTool;
use crate::profile::AnalyticsFields;

/// What each open tab last counted, by connection: dashboards and
/// questions. The figures come from open tabs only; a saved server is never
/// connected to just to fill a number (§3.1).
pub type Figure = (Option<u64>, Option<u64>);
pub type Figures = HashMap<String, Figure>;

pub static FIGURES: LazyLock<Mutex<Figures>> = LazyLock::new(Default::default);

pub fn show(_tool: &mut AnalyticsTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    let tabs = s.open_tabs.of("analytics");
    let saved: Vec<(String, String, String, hs_ui::icons::ConnIcon)> = {
        let conns = s.connections.read();
        conns
            .of_analytics_kinds()
            .map(|c| {
                let f = AnalyticsFields::from_table(&c.kind, &c.fields);
                (
                    c.id.clone(),
                    c.name.clone(),
                    f.label_for(&c.kind),
                    hs_ui::icons::conn_icon(&c.kind, ""),
                )
            })
            .collect()
    };
    let (boards, questions) = {
        let figures = FIGURES.lock();
        let sum = |pick: fn(&Figure) -> Option<u64>| {
            figures.values().filter_map(pick).reduce(|a, b| a + b)
        };
        (sum(|f| f.0), sum(|f| f.1))
    };
    let shown = |n: Option<u64>| n.map(|n| n.to_string()).unwrap_or_else(|| "—".into());
    let mut pick = None;
    egui::ScrollArea::vertical()
        .id_salt("analytics-dashboard")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            dash::head(
                ui,
                &theme,
                super::DESCRIPTOR.icon,
                &s.t("tool-analytics"),
                &s.t("analytics-dashboard-sub"),
            );
            dash::rule(ui, &theme);
            dash::stats(
                ui,
                &theme,
                &[
                    dash::Stat {
                        icon: ph::PLUGS_CONNECTED,
                        value: &saved.len().to_string(),
                        label: &s.t("analytics-stat-servers"),
                    },
                    dash::Stat {
                        icon: ph::SQUARES_FOUR,
                        value: &shown(boards),
                        label: &s.t("analytics-stat-dashboards"),
                    },
                    dash::Stat {
                        icon: ph::CHART_LINE,
                        value: &shown(questions),
                        label: &s.t("analytics-stat-questions"),
                    },
                    dash::Stat {
                        icon: ph::COLUMNS,
                        value: &tabs.iter().filter(|t| !t.home).count().to_string(),
                        label: &s.t("dash-open-tabs"),
                    },
                ],
            );
            dash::rule(ui, &theme);
            dash::section(ui, &theme, &s.t("dash-start-here"));
            if dash::actions(
                ui,
                &theme,
                &[format!("{} {}", ph::PLUS, s.t("analytics-add"))],
            )
            .is_some()
            {
                pick = Some(dash::Pick::Action(0));
            }
            dash::rule(ui, &theme);
            dash::section(ui, &theme, &s.t("analytics-saved"));
            dash::grid(
                ui,
                &theme,
                "analytics-saved",
                saved.len(),
                &s.t("analytics-none"),
                |ui, i, w| {
                    let (id, name, detail, icon) = &saved[i];
                    if dash::item_button_icon(ui, &theme, *icon, name, detail, w).clicked() {
                        pick = Some(dash::Pick::Open(id.clone()));
                    }
                },
            );
            dash::rule(ui, &theme);
            let labels = dash::DashboardLabels {
                open_heading: &s.t("dash-open"),
                nothing_open: &s.t("dash-nothing-open"),
                no_preview: &s.t("dash-no-preview"),
            };
            if let Some(id) = dash::open_tabs_section(ui, &theme, &tabs, &labels) {
                pick = Some(dash::Pick::Focus(id));
            }
            ui.add_space(8.0);
        });
    match pick {
        Some(dash::Pick::Action(_)) => cx.actions.push(Action::RunCommand {
            id: "settings:analytics:new".into(),
        }),
        Some(dash::Pick::Open(id)) => cx.open("analytics", Some(OpenRequest::Connection { id })),
        Some(dash::Pick::Focus(tab_id)) => cx.actions.push(Action::FocusTab { tab_id }),
        _ => {}
    }
}
