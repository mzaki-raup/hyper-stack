//! Sessions (§4.7): the realm's clients with their active and offline
//! counts; a client's sessions with End Session; Log Out All Sessions.

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};

use super::common::{
    After, Cx, Load, WriteOk, WriteReq, ask, paging, state_note, take, when, write_button,
};
use super::dialogs::Dialog;
use super::users::table;
use crate::provider::{ClientSessionStats, IdmProvider, ObjRef, Page, Paged, Section, Session};

#[derive(Default)]
pub struct SessionsView {
    stats: Load<Vec<ClientSessionStats>>,
    table: TableState,
    selected: Option<ClientSessionStats>,
    page: Option<Page>,
    sessions: Load<Paged<Session>>,
    session_table: TableState,
    logout_all: bool,
}

impl SessionsView {
    pub fn reset(&mut self) {
        *self = SessionsView::default();
    }

    pub fn refresh(&mut self) {
        self.stats = Load::Idle;
        self.sessions = Load::Idle;
    }

    /// The palette's Log Out All Sessions…: ask on the next frame.
    pub fn ask_logout_all(&mut self) {
        self.logout_all = true;
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        take(cx.session, "sessions.stats", &mut self.stats);
        if self.stats.is_idle() {
            let realm = cx.realm.to_string();
            ask(
                cx.session,
                "sessions.stats",
                &mut self.stats,
                move |p| async move { p.client_session_stats(&realm).await },
            );
        }
        take(cx.session, "sessions.client", &mut self.sessions);
        if let Some(c) = &self.selected
            && self.sessions.is_idle()
        {
            let (realm, id, page) = (
                cx.realm.to_string(),
                c.id.clone(),
                self.page.unwrap_or(cx.page),
            );
            ask(
                cx.session,
                "sessions.client",
                &mut self.sessions,
                move |p| async move { p.client_sessions(&realm, &id, page).await },
            );
        }
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::SIGN_OUT,
                    &cx.t("idm-logout-all"),
                    "",
                ) {
                    self.logout_all = true;
                }
            });
        });
        if std::mem::take(&mut self.logout_all) && may.is_ok() {
            let realm = cx.realm.to_string();
            let r = realm.clone();
            cx.out.dialog = Some(Dialog::Typed {
                title: cx.t("idm-logout-all"),
                body: cx.ta("idm-logout-all-confirm", &[("realm", &realm)]),
                name: realm.clone(),
                ok: format!("{} {}", ph::SIGN_OUT, cx.t("idm-logout-all-ok")),
                typed: String::new(),
                then: WriteReq::new(
                    cx.ta("idm-done-logout-all", &[("realm", &realm)]),
                    After::Reload(Section::Sessions),
                    move |p| async move { p.logout_all(&r).await.map(WriteOk::LogoutAll) },
                ),
            });
        }
        ui.add_space(form::PANE_GAP);
        if !state_note(ui, theme, cx.s, &self.stats) {
            return;
        }
        let Some(stats) = self.stats.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-client-id"), 220.0),
            ColumnSpec::new(cx.t("idm-active"), 100.0).numeric(),
            ColumnSpec::new(cx.t("idm-offline"), 100.0).numeric(),
        ];
        let rows: Vec<Vec<String>> = stats
            .iter()
            .map(|c| {
                vec![
                    c.client_id.clone(),
                    c.active.to_string(),
                    c.offline.to_string(),
                ]
            })
            .collect();
        let height = (ui.available_height() - 8.0).max(120.0);
        let resp = ui
            .allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
                DataTable {
                    theme,
                    columns: &cols,
                    rows: &rows,
                    row_height: hs_ui::table::ROW_HEIGHT,
                    show_filter: false,
                    copy_row_label: &cx.t("table-copy-row"),
                    built_in_menu: true,
                    row_icons: None,
                    empty_hint: Some(&cx.t("idm-sessions-realm-empty")),
                }
                .show(ui, &mut self.table)
            })
            .inner;
        if let Some(c) = resp.clicked.and_then(|i| stats.get(i)).cloned()
            && self.selected.as_ref().map(|s| &s.id) != Some(&c.id)
        {
            self.selected = Some(c);
            self.page = None;
            self.sessions = Load::Idle;
            self.session_table = TableState::default();
        }
    }

    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let Some(client) = self.selected.clone() else {
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-client-sessions"));
            return;
        };
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        form::pane(ui, |ui| {
            let (glyph, colour) = super::common::section_mark(Section::Sessions);
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(20.0).color(colour));
                ui.label(egui::RichText::new(&client.client_id).strong().size(16.0));
            });
            ui.add_space(form::ROW_GAP);
            if !state_note(ui, theme, cx.s, &self.sessions) {
                return;
            }
            let Some(sessions) = self.sessions.ready().cloned() else {
                return;
            };
            let cols = [
                ColumnSpec::new(cx.t("idm-username"), 150.0),
                ColumnSpec::new(cx.t("idm-ip"), 130.0),
                ColumnSpec::new(cx.t("idm-started"), 140.0),
                ColumnSpec::new(cx.t("idm-last-access"), 140.0),
            ];
            let rows: Vec<Vec<String>> = sessions
                .rows
                .iter()
                .map(|x| {
                    vec![
                        x.username.clone(),
                        x.ip_address.clone(),
                        when(x.started),
                        when(x.last_access),
                    ]
                })
                .collect();
            table(
                ui,
                cx,
                &cols,
                &rows,
                &mut self.session_table,
                &cx.t("idm-sessions-none"),
                420.0,
            );
            let picked = self
                .session_table
                .selected
                .and_then(|i| sessions.rows.get(i))
                .cloned();
            ui.add_space(form::PANE_GAP);
            if write_button(
                ui,
                theme,
                &may,
                picked.is_some(),
                ph::SIGN_OUT,
                &cx.t("idm-end-session"),
                &cx.t("idm-select-session"),
            ) && let Some(x) = picked
            {
                let realm = cx.realm.to_string();
                let target = ObjRef::new(&x.id, &x.username);
                cx.out.dialog = Some(Dialog::Confirm {
                    title: cx.t("idm-end-session"),
                    body: cx.ta(
                        "idm-end-session-confirm",
                        &[("name", &x.username), ("ip", &x.ip_address)],
                    ),
                    ok: format!("{} {}", ph::SIGN_OUT, cx.t("idm-end-session")),
                    danger: true,
                    then: WriteReq::new(
                        cx.t("idm-done-session-ended"),
                        After::Reload(Section::Sessions),
                        move |p| async move {
                            p.end_session(&realm, &target).await.map(|()| WriteOk::Done)
                        },
                    ),
                });
            }
            if let Some(page) = paging(ui, theme, cx.s, &sessions) {
                self.page = Some(page);
                self.sessions = Load::Idle;
            }
        });
    }
}
