//! The dashboard on the activity's home tab (§3.1): the band, the
//! figures, Start Here, Saved Servers, Recent Changes and Open Here.

use std::collections::HashMap;
use std::sync::LazyLock;

use hs_core::tool::{Action, OpenRequest, ToolContext};
use hs_ui::dashboard as dash;
use hs_ui::dialog::DialogResult;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::IdmTool;
use crate::audit::{AuditEntry, AuditLog};
use crate::profile::IdmFields;

/// What each open tab last counted, by connection: realms, and the
/// current realm's users. The figures come from open tabs only; a saved
/// server is never connected to just to fill a number (§3.1).
/// Realms, and the current realm's users when counted.
pub type Figures = HashMap<String, (usize, Option<u64>)>;

pub static FIGURES: LazyLock<Mutex<Figures>> = LazyLock::new(Default::default);

/// A Clear History or Remove waiting for its confirmation.
pub enum Pending {
    Clear,
    Remove(Box<AuditEntry>),
}

/// The dashboard's own state, kept on the home tab.
#[derive(Default)]
pub struct DashState {
    pending: Option<Pending>,
    table: TableState,
    asked: bool,
    /// A row's right-click menu: the row and where it was opened.
    menu: Option<(usize, egui::Pos2)>,
}

pub fn show(tool: &mut IdmTool, ui: &mut egui::Ui, cx: &mut ToolContext) {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    let tabs = s.open_tabs.of("idm");
    let saved: Vec<(String, String, String, hs_ui::icons::ConnIcon)> = {
        let conns = s.connections.read();
        conns
            .of_idm_kinds()
            .map(|c| {
                let f = IdmFields::from_table(&c.kind, &c.fields);
                let realm = if f.default_realm.is_empty() {
                    f.auth_realm.clone()
                } else {
                    f.default_realm.clone()
                };
                (
                    c.id.clone(),
                    c.name.clone(),
                    format!("{} · {realm}", crate::profile::short(&f.url)),
                    hs_ui::icons::conn_icon(&c.kind, ""),
                )
            })
            .collect()
    };
    let (realms, users) = {
        let figures = FIGURES.lock();
        let realms: usize = figures.values().map(|(r, _)| r).sum();
        let users: Option<u64> = figures
            .values()
            .filter_map(|(_, u)| *u)
            .reduce(|a, b| a + b);
        (realms, users)
    };
    let log = AuditLog::shared(s.workspace.read().root());
    let dash_state = &mut tool.dash;
    let recent = match log.peek(10) {
        Some(r) => Some(r),
        None => {
            if !dash_state.asked {
                dash_state.asked = true;
                let l = log.clone();
                let waker = s.runtime.waker();
                s.runtime.spawn_blocking(move || {
                    l.recent(10);
                    waker.request_repaint();
                });
            }
            None
        }
    };
    let mut pick = None;
    let mut menu: Option<Pending> = None;
    egui::ScrollArea::vertical()
        .id_salt("idm-dashboard")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            dash::head(
                ui,
                &theme,
                super::DESCRIPTOR.icon,
                &s.t("tool-idm"),
                &s.t("idm-dashboard-sub"),
            );
            dash::rule(ui, &theme);
            let realms_text = realms.to_string();
            let users_text = users
                .map(super::common::group)
                .unwrap_or_else(|| "—".into());
            dash::stats(
                ui,
                &theme,
                &[
                    dash::Stat {
                        icon: ph::PLUGS_CONNECTED,
                        value: &saved.len().to_string(),
                        label: &s.t("idm-stat-servers"),
                    },
                    dash::Stat {
                        icon: ph::GLOBE_HEMISPHERE_WEST,
                        value: &realms_text,
                        label: &s.t("idm-stat-realms"),
                    },
                    dash::Stat {
                        icon: ph::USER,
                        value: &users_text,
                        label: &s.t("idm-stat-users"),
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
            if dash::actions(ui, &theme, &[format!("{} {}", ph::PLUS, s.t("idm-add"))]).is_some() {
                pick = Some(dash::Pick::Action(0));
            }
            dash::rule(ui, &theme);
            dash::section(ui, &theme, &s.t("idm-saved"));
            dash::grid(
                ui,
                &theme,
                "idm-saved",
                saved.len(),
                &s.t("idm-none"),
                |ui, i, w| {
                    let (id, name, detail, icon) = &saved[i];
                    if dash::item_button_icon(ui, &theme, *icon, name, detail, w).clicked() {
                        pick = Some(dash::Pick::Open(id.clone()));
                    }
                },
            );
            dash::rule(ui, &theme);
            let heading = ui
                .scope(|ui| dash::section(ui, &theme, &s.t("idm-recent-changes")))
                .response;
            let heading = ui.interact(
                heading.rect,
                ui.id().with("idm-recent-head"),
                egui::Sense::click(),
            );
            let empty = recent.as_ref().is_none_or(Vec::is_empty);
            heading.context_menu(|ui| {
                if ui
                    .add_enabled(
                        !empty,
                        egui::Button::new(format!("{} {}", ph::BROOM, s.t("idm-clear-history"))),
                    )
                    .clicked()
                {
                    menu = Some(Pending::Clear);
                    ui.close();
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(dash::HEAD_MARGIN);
                ui.vertical(|ui| {
                    ui.set_max_width(980.0);
                    match &recent {
                        None => {
                            ui.spinner();
                        }
                        Some(rows) => {
                            if let Some(i) =
                                recent_table(ui, &theme, s, rows, &mut dash_state.table)
                                && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                            {
                                dash_state.menu = Some((i, pos));
                            }
                        }
                    }
                });
            });
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
    // A row's menu: Remove this entry, or Clear History (CR-0067).
    if let Some((i, pos)) = dash_state.menu
        && let Some(entry) = recent.as_ref().and_then(|r| r.get(i))
    {
        let area = egui::Area::new(egui::Id::new("idm-recent-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    if ui
                        .button(format!("{} {}", ph::TRASH, s.t("idm-remove")))
                        .clicked()
                    {
                        menu = Some(Pending::Remove(Box::new(entry.clone())));
                    }
                    if ui
                        .button(format!("{} {}", ph::BROOM, s.t("idm-clear-history")))
                        .clicked()
                    {
                        menu = Some(Pending::Clear);
                    }
                });
            });
        // A left-click outside closes it; the right-click that opened it
        // must not.
        let clicked_elsewhere = ui.input(|i| i.pointer.primary_clicked())
            && !area
                .response
                .rect
                .contains(ui.input(|i| i.pointer.interact_pos()).unwrap_or(pos));
        if menu.is_some() || clicked_elsewhere {
            dash_state.menu = None;
        }
    }
    if let Some(p) = menu {
        dash_state.pending = Some(p);
    }
    if let Some(p) = dash_state.pending.take() {
        let (title, body) = match &p {
            Pending::Clear => (s.t("idm-clear-history"), s.t("idm-clear-history-confirm")),
            Pending::Remove(_) => (s.t("idm-remove"), s.t("idm-remove-entry-confirm")),
        };
        match hs_ui::dialog::confirm(
            ui.ctx(),
            &theme,
            "idm-recent-confirm",
            &title,
            &body,
            &title,
            &s.t("cancel"),
            true,
        ) {
            DialogResult::Ok => {
                let l = log.clone();
                let waker = s.runtime.waker();
                // The file is written off the UI thread; the shared tail
                // already shows the result.
                s.runtime.spawn_blocking(move || {
                    let r = match p {
                        Pending::Clear => l.clear(),
                        Pending::Remove(e) => l.remove(&e),
                    };
                    if let Err(e) = r {
                        tracing::warn!("idm: audit log: {e}");
                    }
                    waker.request_repaint();
                });
            }
            DialogResult::Cancel => {}
            DialogResult::Open => dash_state.pending = Some(p),
        }
    }
    match pick {
        Some(dash::Pick::Action(_)) => cx.actions.push(Action::RunCommand {
            id: "settings:identity:new".into(),
        }),
        Some(dash::Pick::Open(id)) => cx.open("idm", Some(OpenRequest::Connection { id })),
        Some(dash::Pick::Focus(tab_id)) => cx.actions.push(Action::FocusTab { tab_id }),
        _ => {}
    }
}

/// Recent Changes as a table: time, server, realm, verb, object, and the
/// outcome. Right-click a row for Remove or Clear History.
fn recent_table(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &hs_core::Services,
    rows: &[AuditEntry],
    state: &mut TableState,
) -> Option<usize> {
    let cols = [
        ColumnSpec::new(s.t("idm-time"), 150.0),
        ColumnSpec::new(s.t("idm-server"), 160.0),
        ColumnSpec::new(s.t("idm-realm"), 110.0),
        ColumnSpec::new(s.t("idm-verb"), 130.0),
        ColumnSpec::new(s.t("idm-object"), 200.0),
        ColumnSpec::new(s.t("idm-outcome"), 160.0),
    ];
    let data: Vec<Vec<String>> = rows
        .iter()
        .map(|e| {
            vec![
                local_time(&e.at),
                crate::profile::short(&e.server).to_string(),
                e.realm.clone(),
                e.verb.clone(),
                format!("{} {}", e.object, e.name).trim().to_string(),
                e.outcome.clone(),
            ]
        })
        .collect();
    let height =
        hs_ui::table::HEADER_HEIGHT + data.len().max(1) as f32 * hs_ui::table::ROW_HEIGHT + 8.0;
    let ok = theme.success;
    let bad = theme.error;
    let failed: Vec<bool> = rows.iter().map(|e| e.outcome != "ok").collect();
    let resp = ui
        .allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            DataTable {
                theme,
                columns: &cols,
                rows: &data,
                row_height: hs_ui::table::ROW_HEIGHT,
                show_filter: false,
                copy_row_label: &s.t("table-copy-row"),
                built_in_menu: false,
                row_icons: None,
                empty_hint: Some(&s.t("idm-recent-empty")),
            }
            .show_coloured(ui, state, &|row, col| {
                (col == 5).then(|| if failed[row] { bad } else { ok })
            })
        })
        .inner;
    resp.secondary.filter(|i| *i < rows.len())
}

/// An audit entry's RFC 3339 UTC time as local "2026-11-08 10:12:03".
fn local_time(at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(at)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| at.to_string())
}
