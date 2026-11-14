//! Clients (§4.6): the realm's clients, searched and paged with More;
//! the client's page - Settings, Credentials, Roles, Service Account
//! Roles, Sessions.

use std::time::{Duration, Instant};

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use serde_json::{Value, json};

use super::common::{
    After, Cx, Load, WriteOk, WriteReq, ask, chip, paging, state_note, take, write_button,
};
use super::dialogs::{Create, Dialog, lines};
use super::roles_pick::{RolePick, mapping_rows};
use super::users::table;
use crate::provider::{
    Access, Change, Client, IdmProvider, NewClient, Page, Paged, Role, RoleMappings, RoleScope,
    Secret, Section, Session, Subject, UserRow,
};

/// How long Show leaves a secret visible (§4.6).
const SHOW_FOR: Duration = Duration::from_secs(30);
/// When a copied secret is cleared from the clipboard (§4.6).
const CLEAR_AFTER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Settings,
    Credentials,
    Roles,
    ServiceAccount,
    Sessions,
}

impl Tab {
    const ALL: [Tab; 5] = [
        Tab::Settings,
        Tab::Credentials,
        Tab::Roles,
        Tab::ServiceAccount,
        Tab::Sessions,
    ];
    fn key(self) -> &'static str {
        match self {
            Tab::Settings => "idm-tab-settings",
            Tab::Credentials => "idm-tab-credentials",
            Tab::Roles => "idm-tab-roles",
            Tab::ServiceAccount => "idm-tab-service-account",
            Tab::Sessions => "idm-tab-sessions",
        }
    }
}

/// The common fields of §4.6, edited.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    name: String,
    description: String,
    enabled: bool,
    root_url: String,
    base_url: String,
    admin_url: String,
    redirect_uris: String,
    post_logout: String,
    web_origins: String,
    confidential: bool,
    standard_flow: bool,
    implicit_flow: bool,
    direct_access: bool,
    service_accounts: bool,
    device_grant: bool,
    front_channel_logout: bool,
    consent_required: bool,
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}
fn b(v: &Value, k: &str) -> bool {
    v.get(k).and_then(Value::as_bool).unwrap_or(false)
}
fn list(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}
fn attr(v: &Value, k: &str) -> String {
    v.get("attributes")
        .and_then(|a| a.get(k))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// The fields the Settings page edits; everything else is More Settings.
const EDITED: &[&str] = &[
    "name",
    "description",
    "enabled",
    "rootUrl",
    "baseUrl",
    "adminUrl",
    "redirectUris",
    "webOrigins",
    "publicClient",
    "standardFlowEnabled",
    "implicitFlowEnabled",
    "directAccessGrantsEnabled",
    "serviceAccountsEnabled",
    "frontchannelLogout",
    "consentRequired",
    "attributes",
];

impl Draft {
    fn of(c: &Client) -> Draft {
        let v = &c.raw;
        Draft {
            name: s(v, "name"),
            description: s(v, "description"),
            enabled: b(v, "enabled"),
            root_url: s(v, "rootUrl"),
            base_url: s(v, "baseUrl"),
            admin_url: s(v, "adminUrl"),
            redirect_uris: list(v, "redirectUris"),
            post_logout: attr(v, "post.logout.redirect.uris").replace("##", "\n"),
            web_origins: list(v, "webOrigins"),
            confidential: !b(v, "publicClient"),
            standard_flow: b(v, "standardFlowEnabled"),
            implicit_flow: b(v, "implicitFlowEnabled"),
            direct_access: b(v, "directAccessGrantsEnabled"),
            service_accounts: b(v, "serviceAccountsEnabled"),
            device_grant: attr(v, "oauth2.device.authorization.grant.enabled") == "true",
            front_channel_logout: b(v, "frontchannelLogout"),
            consent_required: b(v, "consentRequired"),
        }
    }

    fn change(&self, c: &Client) -> Change {
        let base = Draft::of(c);
        let mut ch = Change::default();
        let mut text = |k: &str, new: &str, old: &str| {
            if new.trim() != old.trim() {
                ch = std::mem::take(&mut ch).set(k, new.trim());
            }
        };
        text("name", &self.name, &base.name);
        text("description", &self.description, &base.description);
        text("rootUrl", &self.root_url, &base.root_url);
        text("baseUrl", &self.base_url, &base.base_url);
        text("adminUrl", &self.admin_url, &base.admin_url);
        let mut flag = |k: &str, new: bool, old: bool| {
            if new != old {
                ch = std::mem::take(&mut ch).set(k, new);
            }
        };
        flag("enabled", self.enabled, base.enabled);
        flag("publicClient", !self.confidential, !base.confidential);
        flag(
            "standardFlowEnabled",
            self.standard_flow,
            base.standard_flow,
        );
        flag(
            "implicitFlowEnabled",
            self.implicit_flow,
            base.implicit_flow,
        );
        flag(
            "directAccessGrantsEnabled",
            self.direct_access,
            base.direct_access,
        );
        flag(
            "serviceAccountsEnabled",
            self.service_accounts,
            base.service_accounts,
        );
        flag(
            "frontchannelLogout",
            self.front_channel_logout,
            base.front_channel_logout,
        );
        flag(
            "consentRequired",
            self.consent_required,
            base.consent_required,
        );
        if lines(&self.redirect_uris) != lines(&base.redirect_uris) {
            ch = ch.set("redirectUris", json!(lines(&self.redirect_uris)));
        }
        if lines(&self.web_origins) != lines(&base.web_origins) {
            ch = ch.set("webOrigins", json!(lines(&self.web_origins)));
        }
        // The two attribute-backed settings go in the whole attribute map,
        // so the client's other attributes are kept.
        if lines(&self.post_logout) != lines(&base.post_logout)
            || self.device_grant != base.device_grant
        {
            let mut attrs = c
                .raw
                .get("attributes")
                .cloned()
                .unwrap_or_else(|| json!({}));
            if let Value::Object(m) = &mut attrs {
                m.insert(
                    "post.logout.redirect.uris".into(),
                    json!(lines(&self.post_logout).join("##")),
                );
                m.insert(
                    "oauth2.device.authorization.grant.enabled".into(),
                    json!(self.device_grant.to_string()),
                );
            }
            ch = ch.set("attributes", attrs);
        }
        ch
    }
}

/// A client secret on screen: masked unless Show, for 30 s.
struct Shown {
    secret: Secret,
    shown_at: Option<Instant>,
}

#[derive(Default)]
pub struct ClientsView {
    search: String,
    asked: Option<String>,
    page: Option<Page>,
    list: Load<Paged<Client>>,
    table: TableState,
    selected: Option<String>,
    item: Load<Client>,
    tab: Tab,
    draft: Option<Draft>,
    secret: Load<Secret>,
    shown: Option<Shown>,
    roles: Load<Paged<Role>>,
    sa_user: Load<UserRow>,
    sa_roles: Load<RoleMappings>,
    assign: Option<RolePick>,
    sessions: Load<Paged<Session>>,
    session_count: Load<u64>,
}

impl ClientsView {
    pub fn reset(&mut self) {
        *self = ClientsView::default();
    }

    pub fn refresh(&mut self) {
        self.list = Load::Idle;
        self.asked = None;
        if self.selected.is_some() {
            self.reload_item();
        }
    }

    fn reload_item(&mut self) {
        self.item = Load::Idle;
        self.draft = None;
        self.secret = Load::Idle;
        self.shown = None;
        self.roles = Load::Idle;
        self.sa_user = Load::Idle;
        self.sa_roles = Load::Idle;
        self.sessions = Load::Idle;
        self.session_count = Load::Idle;
    }

    pub fn select(&mut self, id: String) {
        self.selected = Some(id);
        self.reload_item();
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    pub fn forget_selected(&mut self) {
        if let (Some(id), Some(list)) = (self.selected.take(), self.list.ready_mut()) {
            list.rows.retain(|r| r.id != id);
        }
        self.reload_item();
    }

    /// A regenerated secret, shown masked like the old one (§4.6).
    pub fn show_secret(&mut self, secret: Secret) {
        self.secret = Load::Ready(secret);
        self.shown = None;
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        take(cx.session, "clients.list", &mut self.list);
        let term = self.search.trim().to_string();
        if self.asked.as_deref() != Some(term.as_str()) || self.list.is_idle() {
            let (realm, page) = (cx.realm.to_string(), self.page.unwrap_or(cx.page));
            let t = term.clone();
            ask(
                cx.session,
                "clients.list",
                &mut self.list,
                move |p| async move { p.clients(&realm, &t, page).await },
            );
            self.asked = Some(term);
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        take(cx.session, "clients.item", &mut self.item);
        if self.item.is_idle() {
            let (realm, i) = (cx.realm.to_string(), id.clone());
            ask(
                cx.session,
                "clients.item",
                &mut self.item,
                move |p| async move { p.client(&realm, &i).await },
            );
        }
        if self.draft.is_none()
            && let Some(c) = self.item.ready()
        {
            self.draft = Some(Draft::of(c));
        }
        let confidential = self
            .item
            .ready()
            .is_some_and(|c| c.access() == Access::Confidential);
        let service = self.item.ready().is_some_and(|c| c.service_accounts);
        match self.tab {
            Tab::Credentials if confidential => {
                take(cx.session, "clients.secret", &mut self.secret);
                if self.secret.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "clients.secret",
                        &mut self.secret,
                        move |p| async move { p.client_secret(&realm, &i).await },
                    );
                }
            }
            Tab::Roles => {
                take(cx.session, "clients.roles", &mut self.roles);
                if self.roles.is_idle() {
                    let (realm, i, page) = (cx.realm.to_string(), id.clone(), cx.page);
                    ask(
                        cx.session,
                        "clients.roles",
                        &mut self.roles,
                        move |p| async move { p.roles(&realm, &RoleScope::Client(i), "", page).await },
                    );
                }
            }
            Tab::ServiceAccount if service => {
                take(cx.session, "clients.sa-user", &mut self.sa_user);
                if self.sa_user.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "clients.sa-user",
                        &mut self.sa_user,
                        move |p| async move { p.service_account_user(&realm, &i).await },
                    );
                }
                take(cx.session, "clients.sa-roles", &mut self.sa_roles);
                if self.sa_roles.is_idle()
                    && let Some(u) = self.sa_user.ready()
                {
                    let (realm, uid) = (cx.realm.to_string(), u.id.clone());
                    ask(
                        cx.session,
                        "clients.sa-roles",
                        &mut self.sa_roles,
                        move |p| async move { p.role_mappings(&realm, &Subject::User(uid)).await },
                    );
                }
            }
            Tab::Sessions => {
                take(cx.session, "clients.sessions", &mut self.sessions);
                take(cx.session, "clients.session-count", &mut self.session_count);
                if self.sessions.is_idle() {
                    let (realm, i, page) = (cx.realm.to_string(), id.clone(), cx.page);
                    ask(
                        cx.session,
                        "clients.sessions",
                        &mut self.sessions,
                        move |p| async move { p.client_sessions(&realm, &i, page).await },
                    );
                }
                if self.session_count.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "clients.session-count",
                        &mut self.session_count,
                        move |p| async move { p.client_session_count(&realm, &i).await },
                    );
                }
            }
            _ => {}
        }
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if form::text_field_width(ui, &mut self.search, &cx.t("idm-search-clients"), 240.0)
                    .changed()
                {
                    self.page = None;
                }
                let may = cx.may_write("manage-clients");
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::PLUS,
                    &cx.t("idm-create-client"),
                    "",
                ) {
                    cx.out.dialog = Some(Dialog::Create(Create::Client {
                        realm: cx.realm.to_string(),
                        form: NewClient::default(),
                    }));
                }
            });
        });
        ui.add_space(form::PANE_GAP);
        if !state_note(ui, theme, cx.s, &self.list) {
            return;
        }
        let Some(list) = self.list.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-client-id"), 160.0),
            ColumnSpec::new(cx.t("idm-name"), 130.0),
            ColumnSpec::new(cx.t("idm-type"), 120.0),
            ColumnSpec::new(cx.t("idm-access"), 100.0),
            ColumnSpec::new(cx.t("idm-status"), 80.0),
        ];
        let rows: Vec<Vec<String>> = list
            .rows
            .iter()
            .map(|c| {
                vec![
                    c.client_id.clone(),
                    c.name.clone(),
                    if c.protocol == "saml" {
                        "SAML".into()
                    } else {
                        "OpenID Connect".into()
                    },
                    match c.access() {
                        Access::Confidential => cx.t("idm-confidential"),
                        Access::Public => cx.t("idm-public"),
                        Access::BearerOnly => cx.t("idm-bearer-only"),
                    },
                    if c.enabled {
                        cx.t("idm-enabled")
                    } else {
                        cx.t("idm-disabled")
                    },
                ]
            })
            .collect();
        self.table.selected = self
            .selected
            .as_deref()
            .and_then(|id| list.rows.iter().position(|r| r.id == id));
        let empty = if self.search.trim().is_empty() {
            cx.t("idm-clients-empty")
        } else {
            cx.ta("idm-clients-no-match", &[("term", self.search.trim())])
        };
        let height = (ui.available_height() - 52.0).max(120.0);
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
                    empty_hint: Some(&empty),
                }
                .show(ui, &mut self.table)
            })
            .inner;
        let pick = resp
            .clicked
            .and_then(|i| list.rows.get(i))
            .map(|r| r.id.clone());
        let go = paging(ui, theme, cx.s, list);
        if let Some(id) = pick
            && self.selected.as_deref() != Some(id.as_str())
        {
            self.select(id);
        }
        if let Some(page) = go {
            self.page = Some(page);
            self.list = Load::Idle;
        }
    }

    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        if self.selected.is_none() {
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-client"));
            return;
        }
        let theme = cx.theme;
        if let Load::Failed(crate::provider::IdmError::NotFound { .. }) = self.item {
            cx.out
                .toasts
                .push((hs_core::tool::ToastLevel::Warning, cx.t("idm-client-gone")));
            self.forget_selected();
            return;
        }
        if !state_note(ui, theme, cx.s, &self.item) {
            return;
        }
        let Some(client) = self.item.ready().cloned() else {
            return;
        };
        let builtin = client.is_builtin(cx.realm);
        let may = cx.may_write("manage-clients");
        form::pane(ui, |ui| {
            let (glyph, colour) = super::common::section_mark(Section::Clients);
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(20.0).color(colour));
                ui.label(egui::RichText::new(&client.client_id).strong().size(16.0));
                if client.enabled {
                    chip(ui, &cx.t("idm-enabled"), theme.success);
                } else {
                    chip(ui, &cx.t("idm-disabled"), theme.text_muted);
                }
                if builtin {
                    chip(ui, &cx.t("idm-built-in"), theme.text_muted);
                }
            });
            ui.add_space(form::PANE_GAP);
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if write_button(
                    ui,
                    theme,
                    &may,
                    !builtin,
                    ph::TRASH,
                    &cx.t("idm-delete"),
                    &cx.t("idm-builtin-client"),
                ) {
                    let (r, t) = (cx.realm.to_string(), client.as_ref());
                    cx.out.dialog = Some(Dialog::Typed {
                        title: cx.t("idm-delete-client"),
                        body: cx.ta("idm-delete-client-confirm", &[("name", &client.client_id)]),
                        name: client.client_id.clone(),
                        ok: format!("{} {}", ph::TRASH, cx.t("idm-delete")),
                        typed: String::new(),
                        then: WriteReq::new(
                            cx.ta("idm-done-deleted", &[("name", &client.client_id)]),
                            After::Gone(Section::Clients),
                            move |p| async move {
                                p.delete_client(&r, &t).await.map(|()| WriteOk::Done)
                            },
                        ),
                    });
                }
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked() {
                    cx.out.console = Some(format!("clients/{}/settings", client.id));
                }
            });
        });
        ui.add_space(form::PANE_GAP);
        let labels: Vec<String> = Tab::ALL.iter().map(|t| cx.t(t.key())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        if let Some(i) = hs_ui::tabs::segmented(ui, theme, &refs, &[], &[], at) {
            self.tab = Tab::ALL[i];
        }
        ui.add_space(form::PANE_GAP);
        egui::ScrollArea::both()
            .id_salt("idm-client-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| match self.tab {
                    Tab::Settings => self.settings(ui, cx, &client, &may, builtin),
                    Tab::Credentials => self.credentials(ui, cx, &client, &may),
                    Tab::Roles => self.roles_tab(ui, cx),
                    Tab::ServiceAccount => self.service_account(ui, cx, &client, &may),
                    Tab::Sessions => self.sessions_tab(ui, cx),
                });
            });
    }

    fn settings(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        c: &Client,
        may: &Result<(), String>,
        builtin: bool,
    ) {
        let theme = cx.theme;
        let saml = c.protocol == "saml";
        let editable = may.is_ok() && !builtin;
        if builtin {
            form::note(ui, theme, ph::LOCK, &cx.t("idm-builtin-client-read-only"));
            ui.add_space(form::ROW_GAP);
        }
        let Some(d) = self.draft.as_mut() else {
            return;
        };
        ui.add_enabled_ui(editable, |ui| {
            form::field_grid(ui, "idm-client-settings", |ui| {
                form::label(ui, &cx.t("idm-name"));
                form::text_field(ui, &mut d.name, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-description"));
                form::text_field(ui, &mut d.description, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-enabled"));
                ui.checkbox(&mut d.enabled, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-root-url"));
                form::text_field(ui, &mut d.root_url, "https://app.example.com");
                ui.end_row();
                form::label(ui, &cx.t("idm-home-url"));
                form::text_field(ui, &mut d.base_url, "/");
                ui.end_row();
                form::label(ui, &cx.t("idm-redirect-uris"));
                form::code_field(ui, &mut d.redirect_uris, 2);
                ui.end_row();
                form::label(ui, &cx.t("idm-post-logout-uris"));
                form::code_field(ui, &mut d.post_logout, 2);
                ui.end_row();
                form::label(ui, &cx.t("idm-web-origins"));
                form::code_field(ui, &mut d.web_origins, 2);
                ui.end_row();
                form::row_hint(ui, theme, &cx.t("idm-one-per-line"));
                form::label(ui, &cx.t("idm-admin-url"));
                form::text_field(ui, &mut d.admin_url, "");
                ui.end_row();
                if !saml {
                    form::label(ui, &cx.t("idm-client-authentication"));
                    ui.checkbox(&mut d.confidential, "");
                    ui.end_row();
                    form::label(ui, &cx.t("idm-standard-flow"));
                    ui.checkbox(&mut d.standard_flow, "");
                    ui.end_row();
                    form::label(ui, &cx.t("idm-implicit-flow"));
                    ui.checkbox(&mut d.implicit_flow, "");
                    ui.end_row();
                    form::label(ui, &cx.t("idm-direct-access"));
                    ui.checkbox(&mut d.direct_access, "");
                    ui.end_row();
                    form::label(ui, &cx.t("idm-service-accounts"));
                    ui.add_enabled(
                        d.confidential,
                        egui::Checkbox::new(&mut d.service_accounts, ""),
                    );
                    ui.end_row();
                    form::label(ui, &cx.t("idm-device-grant"));
                    ui.checkbox(&mut d.device_grant, "");
                    ui.end_row();
                }
                form::label(ui, &cx.t("idm-front-channel-logout"));
                ui.checkbox(&mut d.front_channel_logout, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-consent-required"));
                ui.checkbox(&mut d.consent_required, "");
                ui.end_row();
            });
        });
        let change = d.change(c);
        ui.add_space(form::ROW_GAP);
        form::centred_row(ui, |ui| {
            if write_button(
                ui,
                theme,
                may,
                !change.fields.is_empty() && !builtin,
                ph::FLOPPY_DISK,
                &cx.t("idm-save"),
                &cx.t("idm-nothing-changed"),
            ) {
                let rows = change
                    .review(&c.raw)
                    .into_iter()
                    .map(|(k, o, n)| (k, short(&o), short(&n)))
                    .collect();
                let (realm, target, ch) = (cx.realm.to_string(), c.as_ref(), change.clone());
                cx.out.dialog = Some(Dialog::Review {
                    title: cx.ta("idm-review-title", &[("name", &c.client_id)]),
                    rows,
                    stale: false,
                    then: WriteReq::new(
                        cx.ta("idm-done-saved", &[("name", &c.client_id)]),
                        After::Reload(Section::Clients),
                        move |p| async move {
                            p.update_client(&realm, &target, &ch)
                                .await
                                .map(|()| WriteOk::Done)
                        },
                    ),
                });
            }
            ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if ui
                .add_enabled_ui(!change.fields.is_empty(), |ui| {
                    form::button(ui, theme, ph::ARROW_COUNTER_CLOCKWISE, &cx.t("idm-revert"))
                })
                .inner
                .clicked()
            {
                self.draft = Some(Draft::of(c));
            }
        });
        // Everything the page does not edit, read-only and masked (§6.5).
        let mut more = c.raw.clone();
        crate::mask::mask(&mut more);
        let rest: Vec<(String, String)> = more
            .as_object()
            .map(|m| {
                m.iter()
                    .filter(|(k, _)| {
                        !EDITED.contains(&k.as_str()) && *k != "id" && *k != "clientId"
                    })
                    .map(|(k, v)| (k.clone(), short(v)))
                    .collect()
            })
            .unwrap_or_default();
        ui.add_space(form::ROW_GAP);
        egui::CollapsingHeader::new(cx.t("idm-more-settings"))
            .id_salt("idm-client-more")
            .show(ui, |ui| {
                form::detail_table(
                    ui,
                    theme,
                    "idm-client-more-table",
                    rest.iter().map(|(a, b)| (a.as_str(), b.as_str())),
                );
            });
    }

    fn credentials(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        c: &Client,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        if c.access() != Access::Confidential {
            form::note(ui, theme, ph::INFO, &cx.t("idm-no-secret-public"));
            return;
        }
        let authenticator = c
            .raw
            .get("clientAuthenticatorType")
            .and_then(Value::as_str)
            .unwrap_or("client-secret")
            .to_string();
        form::detail_table(
            ui,
            theme,
            "idm-client-cred",
            [(cx.t("idm-authenticator"), authenticator)]
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
        );
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.secret) {
            return;
        }
        let Some(secret) = self.secret.ready().cloned() else {
            return;
        };
        let shown = self.shown.get_or_insert_with(|| Shown {
            secret: secret.clone(),
            shown_at: None,
        });
        if shown.shown_at.is_some_and(|t| t.elapsed() >= SHOW_FOR) {
            shown.shown_at = None;
        }
        let visible = shown.shown_at.is_some();
        if visible {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        form::centred_row(ui, |ui| {
            form::label(ui, &cx.t("idm-client-secret"));
            let text = if visible {
                shown.secret.expose().to_string()
            } else {
                "••••••••".to_string()
            };
            ui.label(egui::RichText::new(text).monospace());
            let (icon, key) = if visible {
                (ph::EYE_SLASH, "idm-hide")
            } else {
                (ph::EYE, "idm-show")
            };
            if form::button(ui, theme, icon, &cx.t(key))
                .on_hover_text(cx.t("idm-show-tip"))
                .clicked()
            {
                shown.shown_at = if visible { None } else { Some(Instant::now()) };
            }
            if form::button(ui, theme, ph::COPY, &cx.t("idm-copy"))
                .on_hover_text(cx.t("idm-copy-secret-tip"))
                .clicked()
            {
                copy_and_clear(ui.ctx(), cx, shown.secret.expose());
                cx.out
                    .toasts
                    .push((hs_core::tool::ToastLevel::Info, cx.t("idm-secret-copied")));
            }
        });
        ui.add_space(form::ROW_GAP);
        if write_button(
            ui,
            theme,
            may,
            !c.is_builtin(cx.realm),
            ph::ARROWS_CLOCKWISE,
            &cx.t("idm-regenerate-secret"),
            &cx.t("idm-builtin-client"),
        ) {
            let (r, t) = (cx.realm.to_string(), c.as_ref());
            cx.out.dialog = Some(Dialog::Typed {
                title: cx.t("idm-regenerate-secret"),
                body: cx.ta("idm-regenerate-confirm", &[("name", &c.client_id)]),
                name: c.client_id.clone(),
                ok: format!("{} {}", ph::ARROWS_CLOCKWISE, cx.t("idm-regenerate")),
                typed: String::new(),
                then: WriteReq::new(
                    cx.ta("idm-done-regenerated", &[("name", &c.client_id)]),
                    After::Reload(Section::Clients),
                    move |p| async move { p.regenerate_secret(&r, &t).await.map(WriteOk::Secret) },
                ),
            });
        }
    }

    fn roles_tab(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        form::hint(ui, theme, &cx.t("idm-client-roles-hint"));
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.roles) {
            return;
        }
        let Some(roles) = self.roles.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-name"), 200.0),
            ColumnSpec::new(cx.t("idm-description"), 240.0),
        ];
        let rows: Vec<Vec<String>> = roles
            .rows
            .iter()
            .map(|r| vec![r.name.clone(), r.description.clone()])
            .collect();
        let mut state = TableState::default();
        table(
            ui,
            cx,
            &cols,
            &rows,
            &mut state,
            &cx.t("idm-roles-empty"),
            320.0,
        );
    }

    fn service_account(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        c: &Client,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        if !c.service_accounts {
            form::note(ui, theme, ph::INFO, &cx.t("idm-no-service-account"));
            return;
        }
        if !state_note(ui, theme, cx.s, &self.sa_user) {
            return;
        }
        let Some(user) = self.sa_user.ready().cloned() else {
            return;
        };
        let may_users = cx.may_write("manage-users");
        let may = if may.is_err() { may.clone() } else { may_users };
        let who = Subject::User(user.id.clone());
        if write_button(
            ui,
            theme,
            &may,
            self.assign.is_none(),
            ph::SHIELD_STAR,
            &cx.t("idm-assign-role"),
            "",
        ) {
            self.assign = Some(RolePick::new(who.clone()));
        }
        if let Some(pick) = self.assign.as_mut() {
            ui.add_space(form::ROW_GAP);
            match pick.show(ui, cx, &user.username) {
                Some(Some(req)) => {
                    cx.out.writes.push(retarget(req));
                    self.assign = None;
                    self.sa_roles = Load::Idle;
                }
                Some(None) => self.assign = None,
                None => {}
            }
        }
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.sa_roles) {
            return;
        }
        let Some(m) = self.sa_roles.ready().cloned() else {
            return;
        };
        if let Some(req) = mapping_rows(ui, cx, &who, &user.username, &m, None, &may) {
            cx.out.writes.push(retarget(req));
            self.sa_roles = Load::Idle;
        }
    }

    fn sessions_tab(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        if let Some(n) = self.session_count.ready() {
            ui.label(cx.ta("idm-client-session-count", &[("count", &n.to_string())]));
            ui.add_space(form::ROW_GAP);
        }
        if !state_note(ui, theme, cx.s, &self.sessions) {
            return;
        }
        let Some(sessions) = self.sessions.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-username"), 160.0),
            ColumnSpec::new(cx.t("idm-ip"), 130.0),
            ColumnSpec::new(cx.t("idm-started"), 150.0),
            ColumnSpec::new(cx.t("idm-last-access"), 150.0),
        ];
        let rows: Vec<Vec<String>> = sessions
            .rows
            .iter()
            .map(|x| {
                vec![
                    x.username.clone(),
                    x.ip_address.clone(),
                    super::common::when(x.started),
                    super::common::when(x.last_access),
                ]
            })
            .collect();
        let mut state = TableState::default();
        table(
            ui,
            cx,
            &cols,
            &rows,
            &mut state,
            &cx.t("idm-sessions-none"),
            320.0,
        );
    }
}

/// A role write from the service-account tab re-reads Clients, not Users.
fn retarget(mut req: WriteReq) -> WriteReq {
    req.after = After::Reload(Section::Clients);
    req
}

/// A JSON value as one short line for a table cell.
fn short(v: &Value) -> String {
    let text = match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if text.chars().count() > 120 {
        format!("{}…", text.chars().take(120).collect::<String>())
    } else {
        text
    }
}

/// Copy a secret as an explicit click, and clear the clipboard after
/// [`CLEAR_AFTER`] (§4.6), from a timer on the runtime so it happens
/// whichever tab is in front.
fn copy_and_clear(ctx: &egui::Context, cx: &Cx<'_>, secret: &str) {
    ctx.copy_text(secret.to_string());
    let ctx = ctx.clone();
    cx.s.runtime.spawn(async move {
        tokio::time::sleep(CLEAR_AFTER).await;
        ctx.copy_text(String::new());
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> Client {
        Client {
            id: "c1".into(),
            client_id: "web-app".into(),
            protocol: "openid-connect".into(),
            raw: json!({
                "id": "c1", "clientId": "web-app", "name": "Web", "enabled": true,
                "publicClient": false, "standardFlowEnabled": true,
                "redirectUris": ["https://a/*"], "webOrigins": ["+"],
                "attributes": {"pkce.code.challenge.method": "S256"}
            }),
            ..Default::default()
        }
    }

    #[test]
    fn an_unedited_client_changes_nothing() {
        let c = client();
        assert!(Draft::of(&c).change(&c).fields.is_empty());
    }

    #[test]
    fn a_redirect_list_change_sends_the_list_and_keeps_other_attributes() {
        let c = client();
        let mut d = Draft::of(&c);
        d.redirect_uris.push_str("\nhttps://b/cb");
        let ch = d.change(&c);
        assert_eq!(ch.names(), vec!["redirectUris".to_string()]);
        assert_eq!(
            ch.fields["redirectUris"],
            json!(["https://a/*", "https://b/cb"])
        );
        // An attribute-backed setting sends the whole attribute map.
        d.device_grant = true;
        let ch = d.change(&c);
        assert_eq!(
            ch.fields["attributes"]["pkce.code.challenge.method"],
            json!("S256")
        );
        assert_eq!(
            ch.fields["attributes"]["oauth2.device.authorization.grant.enabled"],
            json!("true")
        );
    }
}
