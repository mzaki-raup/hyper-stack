//! Login Events and Admin Events (§4.8): filtered, newest first, paged
//! with More; the clicked event's details beside the list. Only the
//! level word wears the log-level colour (CR-0077).

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};

use super::common::{Cx, Load, ask, paging, state_note, take, when};
use crate::provider::{
    AdminEvent, AdminEventQuery, EventQuery, EventsConfig, IdmProvider, LoginEvent, Page, Paged,
};

const OPERATIONS: [&str; 4] = ["CREATE", "UPDATE", "DELETE", "ACTION"];

pub struct EventsView {
    admin: bool,
    config: Load<EventsConfig>,
    // Login filters.
    types: Vec<String>,
    type_text: String,
    client: String,
    user: String,
    ip: String,
    // Admin filters.
    operations: Vec<String>,
    resource_path: String,
    auth_user: String,
    date_from: String,
    date_to: String,
    /// The filters the list was read with; a change reads again.
    asked: Option<String>,
    page: Option<Page>,
    login: Load<Paged<LoginEvent>>,
    admin_list: Load<Paged<AdminEvent>>,
    table: TableState,
}

impl EventsView {
    pub fn new(admin: bool) -> EventsView {
        EventsView {
            admin,
            config: Load::Idle,
            types: Vec::new(),
            type_text: String::new(),
            client: String::new(),
            user: String::new(),
            ip: String::new(),
            operations: Vec::new(),
            resource_path: String::new(),
            auth_user: String::new(),
            date_from: String::new(),
            date_to: String::new(),
            asked: None,
            page: None,
            login: Load::Idle,
            admin_list: Load::Idle,
            table: TableState::default(),
        }
    }

    pub fn reset(&mut self) {
        *self = EventsView::new(self.admin);
    }

    pub fn refresh(&mut self) {
        self.asked = None;
        self.config = Load::Idle;
    }

    fn key(&self) -> String {
        format!(
            "{:?}|{}|{}|{}|{:?}|{}|{}|{}|{}|{:?}",
            self.types,
            self.client.trim(),
            self.user.trim(),
            self.ip.trim(),
            self.operations,
            self.resource_path.trim(),
            self.auth_user.trim(),
            self.date_from,
            self.date_to,
            self.page
        )
    }

    fn topics(&self) -> (&'static str, &'static str) {
        if self.admin {
            ("events.admin", "events.admin-config")
        } else {
            ("events.login", "events.login-config")
        }
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        let (list_topic, config_topic) = self.topics();
        take(cx.session, config_topic, &mut self.config);
        if self.config.is_idle() {
            let realm = cx.realm.to_string();
            ask(
                cx.session,
                config_topic,
                &mut self.config,
                move |p| async move { p.events_config(&realm).await },
            );
        }
        if self.admin {
            take(cx.session, list_topic, &mut self.admin_list);
        } else {
            take(cx.session, list_topic, &mut self.login);
        }
        let key = self.key();
        if self.asked.as_ref() == Some(&key) {
            return;
        }
        let (realm, page) = (cx.realm.to_string(), self.page.unwrap_or(cx.page));
        if self.admin {
            let q = AdminEventQuery {
                operation_types: self.operations.clone(),
                resource_path: self.resource_path.trim().to_string(),
                auth_user: self.auth_user.trim().to_string(),
                date_from: self.date_from.clone(),
                date_to: self.date_to.clone(),
                ..Default::default()
            };
            ask(
                cx.session,
                list_topic,
                &mut self.admin_list,
                move |p| async move { p.admin_events(&realm, &q, page).await },
            );
        } else {
            let q = EventQuery {
                types: self.types.clone(),
                client: self.client.trim().to_string(),
                user: self.user.trim().to_string(),
                ip: self.ip.trim().to_string(),
                date_from: self.date_from.clone(),
                date_to: self.date_to.clone(),
            };
            ask(
                cx.session,
                list_topic,
                &mut self.login,
                move |p| async move { p.login_events(&realm, &q, page).await },
            );
        }
        self.asked = Some(key);
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        let s = cx.s;
        let months: Vec<String> = (1..=12).map(|m| s.t(&format!("date-month-{m}"))).collect();
        let weekdays: Vec<String> = (1..=7).map(|d| s.t(&format!("date-day-{d}"))).collect();
        let labels = hs_ui::datefield::DateLabels {
            open: &s.t("date-open"),
            today: &s.t("date-today"),
            clear: &s.t("date-clear"),
            months: &months,
            weekdays: &weekdays,
        };
        let event_types = cx.session.shared().info.event_types.clone();
        let mut changed = false;
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if self.admin {
                    for op in OPERATIONS {
                        let mut on = self.operations.iter().any(|o| o == op);
                        if ui.checkbox(&mut on, op).changed() {
                            if on {
                                self.operations.push(op.to_string());
                            } else {
                                self.operations.retain(|o| o != op);
                            }
                            changed = true;
                        }
                        ui.add_space(form::OPTION_GAP);
                    }
                    changed |= form::text_field_width(
                        ui,
                        &mut self.resource_path,
                        &cx.t("idm-resource-path"),
                        180.0,
                    )
                    .lost_focus();
                    changed |= form::text_field_width(
                        ui,
                        &mut self.auth_user,
                        &cx.t("idm-by-user"),
                        160.0,
                    )
                    .lost_focus();
                } else {
                    let options: Vec<String> = event_types
                        .iter()
                        .filter(|t| !self.types.contains(t))
                        .cloned()
                        .collect();
                    let r = form::pick_field_width(
                        ui,
                        theme,
                        &mut self.type_text,
                        &options,
                        &cx.t("idm-event-type"),
                        200.0,
                    );
                    let typed = self.type_text.trim().to_string();
                    if (r.lost_focus() || options.contains(&typed)) && options.contains(&typed) {
                        self.types.push(typed);
                        self.type_text.clear();
                        changed = true;
                    }
                    changed |=
                        form::text_field_width(ui, &mut self.client, &cx.t("idm-client"), 140.0)
                            .lost_focus();
                    changed |= form::text_field_width(ui, &mut self.user, &cx.t("idm-user"), 140.0)
                        .lost_focus();
                    changed |= form::text_field_width(ui, &mut self.ip, &cx.t("idm-ip"), 120.0)
                        .lost_focus();
                }
                ui.label(cx.t("idm-from"));
                changed |= ui
                    .scope(|ui| {
                        ui.set_max_width(form::FIELD_MIN_WIDTH);
                        hs_ui::datefield::date_field(
                            ui,
                            theme,
                            "idm-ev-from",
                            &mut self.date_from,
                            &labels,
                        )
                    })
                    .inner
                    .changed();
                ui.label(cx.t("idm-to"));
                changed |= ui
                    .scope(|ui| {
                        ui.set_max_width(form::FIELD_MIN_WIDTH);
                        hs_ui::datefield::date_field(
                            ui,
                            theme,
                            "idm-ev-to",
                            &mut self.date_to,
                            &labels,
                        )
                    })
                    .inner
                    .changed();
            });
        });
        if !self.admin && !self.types.is_empty() {
            ui.add_space(form::PANE_GAP);
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                for (i, t) in self.types.iter().enumerate() {
                    if ui
                        .small_button(format!("{t}  {}", ph::X))
                        .on_hover_text(cx.t("idm-remove"))
                        .clicked()
                    {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                self.types.remove(i);
                changed = true;
            }
        }
        if changed {
            self.page = None;
        }
        ui.add_space(form::PANE_GAP);
        // A realm that does not store this kind of event says so (§4.8).
        if let Some(c) = self.config.ready() {
            let stored = if self.admin {
                c.admin_events_enabled
            } else {
                c.events_enabled
            };
            if !stored {
                form::note(
                    ui,
                    theme,
                    ph::INFO,
                    &cx.t(if self.admin {
                        "idm-admin-events-off"
                    } else {
                        "idm-login-events-off"
                    }),
                );
                ui.add_space(form::ROW_GAP);
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked() {
                    cx.out.console = Some("realm-settings/user-events".into());
                }
                return;
            }
        }
        let go = if self.admin {
            self.admin_table(ui, cx)
        } else {
            self.login_table(ui, cx)
        };
        if let Some(page) = go {
            self.page = Some(page);
        }
    }

    fn login_table(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<Page> {
        let theme = cx.theme;
        if !state_note(ui, theme, cx.s, &self.login) {
            return None;
        }
        let list = self.login.ready()?;
        let cols = [
            ColumnSpec::new(cx.t("idm-time"), 130.0),
            ColumnSpec::new(cx.t("idm-level"), 60.0),
            ColumnSpec::new(cx.t("idm-type"), 150.0),
            ColumnSpec::new(cx.t("idm-user"), 120.0),
            ColumnSpec::new(cx.t("idm-client"), 100.0),
            ColumnSpec::new(cx.t("idm-ip"), 100.0),
            ColumnSpec::new(cx.t("idm-error"), 120.0),
        ];
        let rows: Vec<Vec<String>> = list
            .rows
            .iter()
            .map(|e| {
                vec![
                    when(e.time),
                    e.level().to_string(),
                    e.kind.clone(),
                    e.details
                        .get("username")
                        .cloned()
                        .unwrap_or_else(|| e.user_id.clone()),
                    e.client_id.clone(),
                    e.ip_address.clone(),
                    e.error.clone(),
                ]
            })
            .collect();
        let error = hs_ui::loglevel::colour(theme, hs_core::loglevel::Level::Error);
        let info = hs_ui::loglevel::colour(theme, hs_core::loglevel::Level::Info);
        let errors: Vec<bool> = list.rows.iter().map(|e| e.level() == "ERROR").collect();
        let height = (ui.available_height() - 52.0).max(120.0);
        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            DataTable {
                theme,
                columns: &cols,
                rows: &rows,
                row_height: hs_ui::table::ROW_HEIGHT,
                show_filter: false,
                copy_row_label: &cx.t("table-copy-row"),
                built_in_menu: true,
                row_icons: None,
                empty_hint: Some(&cx.t("idm-events-empty")),
            }
            .show_coloured(ui, &mut self.table, &|row, col| {
                (col == 1).then(|| if errors[row] { error } else { info })
            })
        });
        paging(ui, theme, cx.s, list)
    }

    fn admin_table(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<Page> {
        let theme = cx.theme;
        if !state_note(ui, theme, cx.s, &self.admin_list) {
            return None;
        }
        let list = self.admin_list.ready()?;
        let cols = [
            ColumnSpec::new(cx.t("idm-time"), 130.0),
            ColumnSpec::new(cx.t("idm-operation"), 90.0),
            ColumnSpec::new(cx.t("idm-resource-type"), 120.0),
            ColumnSpec::new(cx.t("idm-resource-path"), 200.0),
            ColumnSpec::new(cx.t("idm-by"), 120.0),
            ColumnSpec::new(cx.t("idm-ip"), 100.0),
        ];
        let rows: Vec<Vec<String>> = list
            .rows
            .iter()
            .map(|e| {
                let by = if e.auth_user.is_empty() {
                    e.auth_client.clone()
                } else {
                    e.auth_user.clone()
                };
                vec![
                    when(e.time),
                    e.operation.clone(),
                    e.resource_type.clone(),
                    e.resource_path.clone(),
                    by,
                    e.auth_ip.clone(),
                ]
            })
            .collect();
        let height = (ui.available_height() - 52.0).max(120.0);
        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            DataTable {
                theme,
                columns: &cols,
                rows: &rows,
                row_height: hs_ui::table::ROW_HEIGHT,
                show_filter: false,
                copy_row_label: &cx.t("table-copy-row"),
                built_in_menu: true,
                row_icons: None,
                empty_hint: Some(&cx.t("idm-events-empty")),
            }
            .show(ui, &mut self.table)
        });
        paging(ui, theme, cx.s, list)
    }

    /// The clicked event: its details, or the representation the admin
    /// event carries (already masked, §6.5).
    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        let Some(i) = self.table.selected else {
            super::common::nothing_selected(ui, theme, &cx.t("idm-select-event"));
            return;
        };
        form::pane(ui, |ui| {
            if self.admin {
                let Some(e) = self.admin_list.ready().and_then(|l| l.rows.get(i)) else {
                    return;
                };
                form::subsection(ui, theme, &format!("{} {}", e.operation, e.resource_type));
                ui.add_space(form::ROW_GAP);
                let rows = [
                    (cx.t("idm-time"), when(e.time)),
                    (cx.t("idm-resource-path"), e.resource_path.clone()),
                    (cx.t("idm-auth-realm"), e.auth_realm.clone()),
                    (cx.t("idm-client"), e.auth_client.clone()),
                    (cx.t("idm-user"), e.auth_user.clone()),
                    (cx.t("idm-ip"), e.auth_ip.clone()),
                    (cx.t("idm-error"), e.error.clone()),
                ];
                form::detail_table(
                    ui,
                    theme,
                    "idm-admin-event",
                    rows.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                );
                ui.add_space(form::ROW_GAP);
                match &e.representation {
                    Some(json) => {
                        form::subsection(ui, theme, &cx.t("idm-representation"));
                        ui.add_space(form::PANE_GAP);
                        form::scroll_well(ui, theme, "idm-admin-repr", Some(300.0), |ui| {
                            ui.add(egui::Label::new(egui::RichText::new(json).monospace()).wrap());
                        });
                    }
                    None => form::hint(ui, theme, &cx.t("idm-no-representation")),
                }
            } else {
                let Some(e) = self.login.ready().and_then(|l| l.rows.get(i)) else {
                    return;
                };
                form::subsection(ui, theme, &e.kind);
                ui.add_space(form::ROW_GAP);
                let mut rows = vec![
                    (cx.t("idm-time"), when(e.time)),
                    (cx.t("idm-user"), e.user_id.clone()),
                    (cx.t("idm-client"), e.client_id.clone()),
                    (cx.t("idm-ip"), e.ip_address.clone()),
                    (cx.t("idm-session"), e.session_id.clone()),
                    (cx.t("idm-error"), e.error.clone()),
                ];
                rows.extend(e.details.iter().map(|(k, v)| (k.clone(), v.clone())));
                form::detail_table(
                    ui,
                    theme,
                    "idm-login-event",
                    rows.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                );
            }
        });
    }
}
