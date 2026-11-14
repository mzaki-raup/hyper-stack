//! Users (docs/tools/idm.md §4.3): the searched, paged list in the middle
//! column and the selected user's page on the right - Details,
//! Attributes, Credentials, Groups, Role Mapping, Sessions.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use serde_json::{Value, json};

use super::common::{
    After, Cx, Load, WriteOk, WriteReq, ask, chip, paging, state_note, take, when, write_button,
};
use super::dialogs::{Create, Dialog};
use super::roles_pick::{RolePick, mapping_rows};
use crate::provider::{
    Change, CredentialInfo, Group, IdmProvider, NewUser, ObjRef, Page, Paged, RequiredAction,
    RoleMappings, Section, Session, Subject, UnmanagedPolicy, User, UserQuery, UserRow,
};

/// How long typing must pause before the search is sent (§4.3).
const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Details,
    Attributes,
    Credentials,
    Groups,
    Roles,
    Sessions,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::Details,
        Tab::Attributes,
        Tab::Credentials,
        Tab::Groups,
        Tab::Roles,
        Tab::Sessions,
    ];

    fn key(self) -> &'static str {
        match self {
            Tab::Details => "idm-tab-details",
            Tab::Attributes => "idm-tab-attributes",
            Tab::Credentials => "idm-tab-credentials",
            Tab::Groups => "idm-tab-groups",
            Tab::Roles => "idm-tab-role-mapping",
            Tab::Sessions => "idm-tab-sessions",
        }
    }
}

/// The Details page's fields while they are edited.
#[derive(Debug, Clone, Default, PartialEq)]
struct Draft {
    email: String,
    first_name: String,
    last_name: String,
    enabled: bool,
    email_verified: bool,
    required_actions: Vec<String>,
    /// Profile-managed attributes (24+) beyond the four core fields.
    profile: BTreeMap<String, String>,
    /// Unmanaged attributes, one row per value.
    attributes: Vec<KvRow>,
}

impl Draft {
    fn of(u: &User) -> Draft {
        let managed = |k: &str| u.profile.as_ref().is_some_and(|p| p.manages(k));
        let core = ["username", "email", "firstName", "lastName"];
        Draft {
            email: u.row.email.clone(),
            first_name: u.row.first_name.clone(),
            last_name: u.row.last_name.clone(),
            enabled: u.row.enabled,
            email_verified: u.row.email_verified,
            required_actions: u.required_actions.clone(),
            profile: u
                .attributes
                .iter()
                .filter(|(k, _)| managed(k) && !core.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.join(", ")))
                .collect(),
            attributes: u
                .attributes
                .iter()
                .filter(|(k, _)| !managed(k))
                .flat_map(|(k, vs)| {
                    vs.iter().map(move |v| KvRow {
                        enabled: true,
                        key: k.clone(),
                        value: v.clone(),
                        secret: false,
                    })
                })
                .collect(),
        }
    }

    /// The fields that differ from `u`, as the server names them.
    fn change(&self, u: &User) -> Change {
        let base = Draft::of(u);
        let mut c = Change::default();
        if self.email != base.email {
            c = c.set("email", self.email.trim());
        }
        if self.first_name != base.first_name {
            c = c.set("firstName", self.first_name.trim());
        }
        if self.last_name != base.last_name {
            c = c.set("lastName", self.last_name.trim());
        }
        if self.enabled != base.enabled {
            c = c.set("enabled", self.enabled);
        }
        if self.email_verified != base.email_verified {
            c = c.set("emailVerified", self.email_verified);
        }
        if self.required_actions != base.required_actions {
            c = c.set("requiredActions", json!(self.required_actions));
        }
        if self.profile != base.profile || self.attributes != base.attributes {
            // The whole map: attributes left out of a PUT are removed
            // (§4.3), so everything kept is sent.
            let mut map: BTreeMap<String, Vec<String>> = u.attributes.clone();
            for (k, v) in &self.profile {
                map.insert(k.clone(), split_values(v));
            }
            let managed = |k: &str| u.profile.as_ref().is_some_and(|p| p.manages(k));
            map.retain(|k, _| managed(k));
            for row in self.attributes.iter().filter(|r| r.enabled) {
                let key = row.key.trim();
                if key.is_empty() {
                    continue;
                }
                map.entry(key.to_string())
                    .or_default()
                    .push(row.value.clone());
            }
            c = c.set("attributes", json!(map));
        }
        c
    }
}

fn split_values(v: &str) -> Vec<String> {
    v.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// A JSON value as one line for the review.
fn show_value(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[derive(Default)]
pub struct UsersView {
    search: String,
    exact: bool,
    enabled: Option<bool>,
    typed_at: Option<Instant>,
    page: Option<Page>,
    list: Load<Paged<UserRow>>,
    table: TableState,
    pub selected: Option<String>,
    item: Load<User>,
    tab: Tab,
    draft: Option<Draft>,
    required: Load<Vec<RequiredAction>>,
    unmanaged: Load<UnmanagedPolicy>,
    add_action: String,
    creds: Load<Vec<CredentialInfo>>,
    groups: Load<Paged<Group>>,
    roles: Load<RoleMappings>,
    effective: bool,
    effective_realm: Load<Vec<crate::provider::Role>>,
    sessions: Load<Vec<Session>>,
    session_table: TableState,
    /// A Save waiting for the fresh read its review compares with.
    saving: Option<Load<User>>,
    join: Option<super::groups_pick::GroupPick>,
    assign: Option<RolePick>,
    focus: bool,
}

impl UsersView {
    /// Drop everything read from the realm (a realm switch, §3.3).
    pub fn reset(&mut self) {
        *self = UsersView::default();
    }

    /// The realm's user count, once read.
    pub fn total(&self) -> Option<u64> {
        self.list.ready().and_then(|l| l.total)
    }

    /// Identity: Search… puts the keyboard in the search field.
    pub fn focus_search(&mut self) {
        self.focus = true;
    }

    /// Read the list again, and the open user.
    pub fn refresh(&mut self) {
        self.list = Load::Idle;
        if self.selected.is_some() {
            self.reload_item();
        }
    }

    fn reload_item(&mut self) {
        self.item = Load::Idle;
        self.draft = None;
        self.creds = Load::Idle;
        self.groups = Load::Idle;
        self.roles = Load::Idle;
        self.effective_realm = Load::Idle;
        self.sessions = Load::Idle;
    }

    pub fn select(&mut self, id: String) {
        self.selected = Some(id);
        self.reload_item();
    }

    /// The selected user gone (a 404 or a delete): out of the list.
    pub fn forget_selected(&mut self) {
        if let (Some(id), Some(list)) = (self.selected.take(), self.list.ready_mut()) {
            list.rows.retain(|r| r.id != id);
        }
        self.reload_item();
    }

    fn query(&self) -> UserQuery {
        UserQuery {
            search: self.search.trim().to_string(),
            exact: self.exact,
            enabled: self.enabled,
        }
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        take(cx.session, "users.list", &mut self.list);
        if let Some(t) = self.typed_at
            && t.elapsed() >= DEBOUNCE
        {
            self.typed_at = None;
            self.page = None;
            self.list = Load::Idle;
        }
        if self.list.is_idle() && self.typed_at.is_none() {
            let realm = cx.realm.to_string();
            let q = self.query();
            let page = self.page.unwrap_or(cx.page);
            ask(
                cx.session,
                "users.list",
                &mut self.list,
                move |p| async move {
                    let mut rows = p.users(&realm, &q, page).await?;
                    // The count is a second request with the same filters;
                    // a server that refuses it still lists (§4.3).
                    if let Ok(Some(n)) = p.user_count(&realm, &q).await {
                        rows.total = Some(n);
                    }
                    Ok(rows)
                },
            );
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        take(cx.session, "users.item", &mut self.item);
        if let Load::Failed(crate::provider::IdmError::NotFound { .. }) = self.item {
            cx.out
                .toasts
                .push((hs_core::tool::ToastLevel::Warning, cx.t("idm-user-gone")));
            self.forget_selected();
            return;
        }
        if self.item.is_idle() {
            let realm = cx.realm.to_string();
            let id = id.clone();
            ask(
                cx.session,
                "users.item",
                &mut self.item,
                move |p| async move { p.user(&realm, &id).await },
            );
        }
        if self.draft.is_none()
            && let Some(u) = self.item.ready()
        {
            self.draft = Some(Draft::of(u));
        }
        take(cx.session, "users.required", &mut self.required);
        if self.required.is_idle() {
            let realm = cx.realm.to_string();
            ask(
                cx.session,
                "users.required",
                &mut self.required,
                move |p| async move { p.required_actions(&realm).await },
            );
        }
        take(cx.session, "users.unmanaged", &mut self.unmanaged);
        if self.unmanaged.is_idle() && cx.caps.user_profile {
            let realm = cx.realm.to_string();
            ask(
                cx.session,
                "users.unmanaged",
                &mut self.unmanaged,
                move |p| async move { p.unmanaged_policy(&realm).await },
            );
        }
        if let Some(saving) = self.saving.as_mut() {
            take(cx.session, "users.fresh", saving);
        }
        match self.tab {
            Tab::Credentials => {
                take(cx.session, "users.creds", &mut self.creds);
                if self.creds.is_idle() {
                    let (realm, id) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "users.creds",
                        &mut self.creds,
                        move |p| async move { p.user_credentials(&realm, &id).await },
                    );
                }
            }
            Tab::Groups => {
                take(cx.session, "users.groups", &mut self.groups);
                if self.groups.is_idle() {
                    let (realm, id, page) = (cx.realm.to_string(), id.clone(), cx.page);
                    ask(
                        cx.session,
                        "users.groups",
                        &mut self.groups,
                        move |p| async move { p.user_groups(&realm, &id, page).await },
                    );
                }
            }
            Tab::Roles => {
                take(cx.session, "users.roles", &mut self.roles);
                if self.roles.is_idle() {
                    let (realm, id) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "users.roles",
                        &mut self.roles,
                        move |p| async move { p.role_mappings(&realm, &Subject::User(id)).await },
                    );
                }
                take(cx.session, "users.effective", &mut self.effective_realm);
                if self.effective && self.effective_realm.is_idle() {
                    let (realm, id) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "users.effective",
                        &mut self.effective_realm,
                        move |p| async move {
                            p.realm_role_mappings(&realm, &Subject::User(id), true)
                                .await
                        },
                    );
                }
            }
            Tab::Sessions => {
                take(cx.session, "users.sessions", &mut self.sessions);
                if self.sessions.is_idle() {
                    let (realm, id) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "users.sessions",
                        &mut self.sessions,
                        move |p| async move { p.user_sessions(&realm, &id).await },
                    );
                }
            }
            Tab::Details | Tab::Attributes => {}
        }
    }

    /// The middle column: the search row, the table and the paging line.
    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let hint = cx.t("idm-search-users");
                let r = form::text_field_width(ui, &mut self.search, &hint, 240.0);
                if std::mem::take(&mut self.focus) {
                    r.request_focus();
                }
                if r.changed() {
                    self.typed_at = Some(Instant::now());
                }
                if form::toggle_button(ui, &mut self.exact, &cx.t("idm-exact"))
                    .on_hover_text(cx.t("idm-exact-tip"))
                    .changed()
                {
                    self.page = None;
                    self.list = Load::Idle;
                }
                let label = match self.enabled {
                    None => cx.t("idm-status-any"),
                    Some(true) => cx.t("idm-enabled"),
                    Some(false) => cx.t("idm-disabled"),
                };
                egui::ComboBox::from_id_salt("idm-users-enabled")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        for (v, k) in [
                            (None, "idm-status-any"),
                            (Some(true), "idm-enabled"),
                            (Some(false), "idm-disabled"),
                        ] {
                            if form::selectable_label(ui, self.enabled == v, cx.t(k)).clicked() {
                                self.enabled = v;
                                self.page = None;
                                self.list = Load::Idle;
                            }
                        }
                    });
                let may = cx.may_write("manage-users");
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::USER_PLUS,
                    &cx.t("idm-add-user"),
                    "",
                ) {
                    cx.out.dialog = Some(Dialog::Create(Create::User {
                        realm: cx.realm.to_string(),
                        form: NewUser {
                            enabled: true,
                            ..Default::default()
                        },
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
            ColumnSpec::new(cx.t("idm-username"), 140.0),
            ColumnSpec::new(cx.t("idm-email"), 170.0),
            ColumnSpec::new(cx.t("idm-name"), 120.0),
            ColumnSpec::new(cx.t("idm-status"), 70.0),
        ];
        let rows: Vec<Vec<String>> = list
            .rows
            .iter()
            .map(|r| {
                vec![
                    r.username.clone(),
                    r.email.clone(),
                    r.full_name(),
                    if r.enabled {
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
            cx.t("idm-users-empty")
        } else {
            cx.ta("idm-users-no-match", &[("term", self.search.trim())])
        };
        let ok = cx.theme.success;
        let off = cx.theme.text_muted;
        let enabled: Vec<bool> = list.rows.iter().map(|r| r.enabled).collect();
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
                .show_coloured(ui, &mut self.table, &|row, col| {
                    (col == 3).then(|| if enabled[row] { ok } else { off })
                })
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

    /// The right column: the selected user's page.
    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        if self.selected.is_none() {
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-user"));
            return;
        }
        let theme = cx.theme;
        if !state_note(ui, theme, cx.s, &self.item) {
            return;
        }
        let Some(user) = self.item.ready().cloned() else {
            return;
        };
        self.header(ui, cx, &user);
        ui.add_space(form::PANE_GAP);
        let labels: Vec<String> = Tab::ALL.iter().map(|t| cx.t(t.key())).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let at = Tab::ALL.iter().position(|t| *t == self.tab).unwrap_or(0);
        if let Some(i) = hs_ui::tabs::segmented(ui, theme, &refs, &[], &[], at) {
            self.tab = Tab::ALL[i];
        }
        ui.add_space(form::PANE_GAP);
        egui::ScrollArea::both()
            .id_salt("idm-user-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| match self.tab {
                    Tab::Details => self.details(ui, cx, &user),
                    Tab::Attributes => self.attributes(ui, cx, &user),
                    Tab::Credentials => self.credentials(ui, cx, &user),
                    Tab::Groups => self.groups(ui, cx, &user),
                    Tab::Roles => self.roles(ui, cx, &user),
                    Tab::Sessions => self.sessions(ui, cx, &user),
                });
            });
    }

    /// The item header: picture, name, state chips, and the user's verbs.
    fn header(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let (glyph, colour) = super::common::section_mark(Section::Users);
        form::pane(ui, |ui| {
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(20.0).color(colour));
                ui.label(egui::RichText::new(&user.row.username).strong().size(16.0));
                ui.add_space(form::BUTTON_GAP);
                if user.row.enabled {
                    chip(ui, &cx.t("idm-enabled"), theme.success);
                } else {
                    chip(ui, &cx.t("idm-disabled"), theme.text_muted);
                }
                if user.row.email_verified {
                    chip(ui, &cx.t("idm-email-verified"), theme.accent);
                }
                if !user.required_actions.is_empty() {
                    chip(ui, &cx.t("idm-actions-pending"), theme.warning);
                }
                if user.row.federation_link.is_some() {
                    chip(ui, &cx.t("idm-federated"), theme.text_muted);
                }
            });
            ui.add_space(form::PANE_GAP);
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                let may = cx.may_write("manage-users");
                let target = user.row.as_ref();
                let realm = cx.realm.to_string();
                let (icon, key, enable) = if user.row.enabled {
                    (ph::TOGGLE_LEFT, "idm-disable", false)
                } else {
                    (ph::TOGGLE_RIGHT, "idm-enable", true)
                };
                if write_button(ui, theme, &may, true, icon, &cx.t(key), "") {
                    let (t, r) = (target.clone(), realm.clone());
                    let done_key = if enable {
                        "idm-done-enabled"
                    } else {
                        "idm-done-disabled"
                    };
                    cx.out.dialog = Some(Dialog::Confirm {
                        title: cx.t(key),
                        body: cx.ta(
                            if enable {
                                "idm-enable-confirm"
                            } else {
                                "idm-disable-confirm"
                            },
                            &[("name", &target.name)],
                        ),
                        ok: cx.t(key),
                        danger: !enable,
                        then: WriteReq::new(
                            cx.ta(done_key, &[("name", &target.name)]),
                            After::Reload(Section::Users),
                            move |p| async move {
                                p.set_user_enabled(&r, &t, enable)
                                    .await
                                    .map(|()| WriteOk::Done)
                            },
                        ),
                    });
                }
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::PASSWORD,
                    &cx.t("idm-reset-password-menu"),
                    "",
                ) {
                    cx.out.dialog = Some(Dialog::ResetPassword {
                        realm: realm.clone(),
                        user: target.clone(),
                        first: String::new(),
                        second: String::new(),
                        temporary: true,
                        mismatch: false,
                    });
                }
                if write_button(ui, theme, &may, true, ph::SIGN_OUT, &cx.t("idm-log-out"), "") {
                    let (t, r) = (target.clone(), realm.clone());
                    cx.out.dialog = Some(Dialog::Confirm {
                        title: cx.t("idm-log-out"),
                        body: cx.ta("idm-log-out-confirm", &[("name", &target.name)]),
                        ok: format!("{} {}", ph::SIGN_OUT, cx.t("idm-log-out")),
                        danger: true,
                        then: WriteReq::new(
                            cx.ta("idm-done-logged-out", &[("name", &target.name)]),
                            After::Reload(Section::Users),
                            move |p| async move {
                                p.logout_user(&r, &t).await.map(|()| WriteOk::Done)
                            },
                        ),
                    });
                }
                if write_button(ui, theme, &may, true, ph::TRASH, &cx.t("idm-delete"), "") {
                    let (t, r) = (target.clone(), realm.clone());
                    let body = if user.row.federation_link.is_some() {
                        cx.ta("idm-delete-user-federated", &[("name", &target.name)])
                    } else {
                        cx.ta("idm-delete-user-confirm", &[("name", &target.name)])
                    };
                    cx.out.dialog = Some(Dialog::Typed {
                        title: cx.t("idm-delete-user"),
                        body,
                        name: target.name.clone(),
                        ok: format!("{} {}", ph::TRASH, cx.t("idm-delete")),
                        typed: String::new(),
                        then: WriteReq::new(
                            cx.ta("idm-done-deleted", &[("name", &target.name)]),
                            After::Gone(Section::Users),
                            move |p| async move {
                                p.delete_user(&r, &t).await.map(|()| WriteOk::Done)
                            },
                        ),
                    });
                }
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked()
                {
                    cx.out.console = Some(format!("users/{}/settings", user.row.id));
                }
            });
        });
    }

    fn details(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        let federated = user.row.federation_link.is_some();
        let actions: Vec<String> = self
            .required
            .ready()
            .map(|a| {
                a.iter()
                    .filter(|a| a.enabled)
                    .map(|a| a.alias.clone())
                    .collect()
            })
            .unwrap_or_default();
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        let editable = may.is_ok() && !federated;
        form::detail_table(
            ui,
            theme,
            "idm-user-facts",
            [
                (cx.t("idm-id").as_str(), user.row.id.as_str()),
                (
                    cx.t("idm-created").as_str(),
                    when(user.row.created.unwrap_or(0)).as_str(),
                ),
                (cx.t("idm-username").as_str(), user.row.username.as_str()),
            ]
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str())),
        );
        ui.add_space(form::ROW_GAP);
        ui.add_enabled_ui(editable, |ui| {
            form::field_grid(ui, "idm-user-details", |ui| {
                form::label(ui, &cx.t("idm-email"));
                form::text_field(ui, &mut draft.email, "alice@example.com");
                ui.end_row();
                form::label(ui, &cx.t("idm-email-verified"));
                ui.checkbox(&mut draft.email_verified, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-first-name"));
                form::text_field(ui, &mut draft.first_name, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-last-name"));
                form::text_field(ui, &mut draft.last_name, "");
                ui.end_row();
                form::label(ui, &cx.t("idm-enabled"));
                ui.checkbox(&mut draft.enabled, "");
                ui.end_row();
                // The profile's own attributes (24+), in its order, with
                // its display names; read-only ones as text.
                if let Some(profile) = &user.profile {
                    for a in profile.attributes.iter().filter(|a| {
                        !["username", "email", "firstName", "lastName"].contains(&a.name.as_str())
                    }) {
                        let mut name = if a.display_name.is_empty() {
                            a.name.clone()
                        } else {
                            a.display_name.clone()
                        };
                        if a.required {
                            name.push_str(" *");
                        }
                        form::label(ui, &name);
                        let value = draft.profile.entry(a.name.clone()).or_default();
                        if a.read_only {
                            ui.label(value.as_str());
                        } else {
                            form::text_field(ui, value, "");
                        }
                        ui.end_row();
                    }
                }
                form::label(ui, &cx.t("idm-required-actions"));
                ui.vertical(|ui| {
                    let mut remove = None;
                    for (i, a) in draft.required_actions.iter().enumerate() {
                        form::centred_row(ui, |ui| {
                            ui.label(a);
                            if ui
                                .small_button(ph::X)
                                .on_hover_text(cx.t("idm-remove"))
                                .clicked()
                            {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        draft.required_actions.remove(i);
                    }
                    let offered: Vec<String> = actions
                        .iter()
                        .filter(|a| !draft.required_actions.contains(a))
                        .cloned()
                        .collect();
                    let r = form::pick_field(
                        ui,
                        theme,
                        &mut self.add_action,
                        &offered,
                        &cx.t("idm-add-required-action"),
                    );
                    let chosen = self.add_action.trim().to_string();
                    if (r.lost_focus() || offered.contains(&chosen))
                        && !chosen.is_empty()
                        && offered.contains(&chosen)
                    {
                        draft.required_actions.push(chosen);
                        self.add_action.clear();
                    }
                });
                ui.end_row();
            });
        });
        if federated {
            ui.add_space(form::ROW_GAP);
            form::note(ui, theme, ph::LINK_BREAK, &cx.t("idm-federated-hint"));
        }
        ui.add_space(form::ROW_GAP);
        self.save_row(ui, cx, user, &may);
    }

    /// Save and Revert under an editable page: Save reads the user again
    /// and reviews the changes against it (§4.3, §6.2).
    fn save_row(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        user: &User,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let change = self
            .draft
            .as_ref()
            .map(|d| d.change(user))
            .unwrap_or_default();
        let dirty = !change.fields.is_empty();
        // The fresh read came back: open the review.
        if let Some(Load::Ready(fresh)) = &self.saving {
            let stale = fresh.raw != user.raw;
            let rows = change
                .review(&fresh.raw)
                .into_iter()
                .map(|(k, o, n)| (k, show_value(&o), show_value(&n)))
                .collect();
            let (realm, target) = (cx.realm.to_string(), user.row.as_ref());
            let name = target.name.clone();
            cx.out.dialog = Some(Dialog::Review {
                title: cx.ta("idm-review-title", &[("name", &name)]),
                rows,
                stale,
                then: WriteReq::new(
                    cx.ta("idm-done-saved", &[("name", &name)]),
                    After::Reload(Section::Users),
                    move |p| async move {
                        p.update_user(&realm, &target, &change)
                            .await
                            .map(|()| WriteOk::Done)
                    },
                ),
            });
            self.saving = None;
            return;
        }
        if let Some(Load::Failed(e)) = &self.saving {
            form::note(
                ui,
                theme,
                ph::WARNING,
                &hs_core::fmt::sentence(&e.to_string()),
            );
            ui.add_space(form::ROW_GAP);
        }
        form::centred_row(ui, |ui| {
            let busy = matches!(self.saving, Some(Load::Loading));
            if write_button(
                ui,
                theme,
                may,
                dirty && !busy,
                ph::FLOPPY_DISK,
                &cx.t("idm-save"),
                &cx.t("idm-nothing-changed"),
            ) {
                let (realm, id) = (cx.realm.to_string(), user.row.id.clone());
                let mut slot = Load::Idle;
                ask(cx.session, "users.fresh", &mut slot, move |p| async move {
                    p.user(&realm, &id).await
                });
                self.saving = Some(slot);
            }
            ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if ui
                .add_enabled_ui(dirty, |ui| {
                    form::button(ui, theme, ph::ARROW_COUNTER_CLOCKWISE, &cx.t("idm-revert"))
                })
                .inner
                .clicked()
            {
                self.draft = Some(Draft::of(user));
            }
            if busy {
                ui.spinner();
            }
        });
    }

    fn attributes(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        // 24+: only unmanaged attributes, as the realm's policy allows.
        let policy = if cx.caps.user_profile {
            match &self.unmanaged {
                Load::Ready(p) => *p,
                other => {
                    state_note(ui, theme, cx.s, other);
                    return;
                }
            }
        } else {
            UnmanagedPolicy::Enabled
        };
        if !policy.shows() {
            form::note(ui, theme, ph::INFO, &cx.t("idm-attributes-off"));
            return;
        }
        let editable = policy.edits() && may.is_ok() && user.row.federation_link.is_none();
        if !policy.edits() {
            form::note(ui, theme, ph::INFO, &cx.t("idm-attributes-view-only"));
            ui.add_space(form::ROW_GAP);
        }
        let Some(draft) = self.draft.as_mut() else {
            return;
        };
        if draft.attributes.is_empty() && !editable {
            form::hint(ui, theme, &cx.t("idm-attributes-none"));
            return;
        }
        ui.add_enabled_ui(editable, |ui| {
            KvEditor {
                theme,
                key_hint: &cx.t("idm-attribute"),
                value_hint: &cx.t("idm-value"),
                add_label: &cx.t("idm-add-attribute"),
                allow_secret: false,
            }
            .show(ui, &mut draft.attributes);
        });
        form::row_hint(ui, theme, &cx.t("idm-attributes-hint"));
        ui.add_space(form::ROW_GAP);
        self.save_row(ui, cx, user, &may);
    }

    fn credentials(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, _user: &User) {
        let theme = cx.theme;
        form::hint(ui, theme, &cx.t("idm-credentials-hint"));
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.creds) {
            return;
        }
        let Some(creds) = self.creds.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-type"), 140.0),
            ColumnSpec::new(cx.t("idm-label"), 200.0),
            ColumnSpec::new(cx.t("idm-created"), 160.0),
        ];
        let rows: Vec<Vec<String>> = creds
            .iter()
            .map(|c| {
                vec![
                    c.kind.clone(),
                    c.label.clone(),
                    when(c.created.unwrap_or(0)),
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
            &cx.t("idm-credentials-empty"),
            200.0,
        );
    }

    fn groups(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        let target = user.row.as_ref();
        if write_button(
            ui,
            theme,
            &may,
            self.join.is_none(),
            ph::USERS_THREE,
            &cx.t("idm-join-group"),
            "",
        ) {
            self.join = Some(super::groups_pick::GroupPick::default());
        }
        if let Some(pick) = self.join.as_mut() {
            ui.add_space(form::ROW_GAP);
            match pick.show(ui, cx) {
                Some(Some(group)) => {
                    let (r, t) = (cx.realm.to_string(), target.clone());
                    let name = group.name.clone();
                    cx.out.writes.push(WriteReq::new(
                        cx.ta(
                            "idm-done-joined",
                            &[("name", &target.name), ("group", &name)],
                        ),
                        After::Reload(Section::Users),
                        move |p| async move {
                            p.join_group(&r, &t, &group).await.map(|()| WriteOk::Done)
                        },
                    ));
                    self.join = None;
                }
                Some(None) => self.join = None,
                None => {}
            }
        }
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.groups) {
            return;
        }
        let Some(groups) = self.groups.ready() else {
            return;
        };
        if groups.rows.is_empty() {
            form::hint(ui, theme, &cx.t("idm-user-groups-empty"));
            return;
        }
        let mut leave = None;
        for g in &groups.rows {
            form::centred_row(ui, |ui| {
                let (glyph, colour) = super::common::section_mark(Section::Groups);
                ui.label(egui::RichText::new(glyph).color(colour));
                ui.label(&g.path);
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::USER_MINUS,
                    &cx.t("idm-leave"),
                    "",
                ) {
                    leave = Some(ObjRef::new(&g.id, &g.path));
                }
            });
            ui.add_space(form::ROW_GAP / 2.0);
        }
        if let Some(group) = leave {
            let (r, t) = (cx.realm.to_string(), target.clone());
            cx.out.writes.push(WriteReq::new(
                cx.ta(
                    "idm-done-left",
                    &[("name", &target.name), ("group", &group.name)],
                ),
                After::Reload(Section::Users),
                move |p| async move { p.leave_group(&r, &t, &group).await.map(|()| WriteOk::Done) },
            ));
        }
        if let Some(page) = paging(ui, theme, cx.s, groups) {
            let (realm, id) = (cx.realm.to_string(), user.row.id.clone());
            ask(
                cx.session,
                "users.groups",
                &mut self.groups,
                move |p| async move { p.user_groups(&realm, &id, page).await },
            );
        }
    }

    fn roles(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        let who = Subject::User(user.row.id.clone());
        form::centred_row(ui, |ui| {
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
            ui.add_space(form::OPTION_GAP);
            if ui
                .checkbox(&mut self.effective, cx.t("idm-show-effective"))
                .on_hover_text(cx.t("idm-show-effective-tip"))
                .changed()
            {
                self.effective_realm = Load::Idle;
            }
        });
        if let Some(pick) = self.assign.as_mut() {
            ui.add_space(form::ROW_GAP);
            match pick.show(ui, cx, &user.row.username) {
                Some(Some(req)) => {
                    cx.out.writes.push(req);
                    self.assign = None;
                }
                Some(None) => self.assign = None,
                None => {}
            }
        }
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.roles) {
            return;
        }
        let Some(mappings) = self.roles.ready().cloned() else {
            return;
        };
        let effective = if self.effective {
            self.effective_realm.ready().cloned()
        } else {
            None
        };
        if let Some(req) = mapping_rows(
            ui,
            cx,
            &who,
            &user.row.username,
            &mappings,
            effective.as_deref(),
            &may,
        ) {
            cx.out.writes.push(req);
        }
    }

    fn sessions(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>, user: &User) {
        let theme = cx.theme;
        let may = cx.may_write("manage-users");
        if !state_note(ui, theme, cx.s, &self.sessions) {
            return;
        }
        let Some(sessions) = self.sessions.ready() else {
            return;
        };
        if sessions.is_empty() {
            form::hint(ui, theme, &cx.t("idm-sessions-none"));
            return;
        }
        let cols = [
            ColumnSpec::new(cx.t("idm-ip"), 140.0),
            ColumnSpec::new(cx.t("idm-started"), 150.0),
            ColumnSpec::new(cx.t("idm-last-access"), 150.0),
            ColumnSpec::new(cx.t("idm-clients"), 200.0),
        ];
        let rows: Vec<Vec<String>> = sessions
            .iter()
            .map(|x| {
                vec![
                    x.ip_address.clone(),
                    when(x.started),
                    when(x.last_access),
                    x.clients.values().cloned().collect::<Vec<_>>().join(", "),
                ]
            })
            .collect();
        table(ui, cx, &cols, &rows, &mut self.session_table, "", 180.0);
        let picked = self
            .session_table
            .selected
            .and_then(|i| sessions.get(i))
            .cloned();
        ui.add_space(form::PANE_GAP);
        let end = write_button(
            ui,
            theme,
            &may,
            picked.is_some(),
            ph::SIGN_OUT,
            &cx.t("idm-end-session"),
            &cx.t("idm-select-session"),
        );
        if let Some(x) = picked.filter(|_| end) {
            let realm = cx.realm.to_string();
            let target = ObjRef::new(&x.id, &x.ip_address);
            cx.out.dialog = Some(Dialog::Confirm {
                title: cx.t("idm-end-session"),
                body: cx.ta(
                    "idm-end-session-confirm",
                    &[("name", &user.row.username), ("ip", &x.ip_address)],
                ),
                ok: format!("{} {}", ph::SIGN_OUT, cx.t("idm-end-session")),
                danger: true,
                then: WriteReq::new(
                    cx.t("idm-done-session-ended"),
                    After::Reload(Section::Users),
                    move |p| async move { p.end_session(&realm, &target).await.map(|()| WriteOk::Done) },
                ),
            });
        }
    }
}

/// A small read-only table inside an item page, `height` tall at most.
pub fn table(
    ui: &mut egui::Ui,
    cx: &Cx<'_>,
    cols: &[ColumnSpec],
    rows: &[Vec<String>],
    state: &mut TableState,
    empty: &str,
    height: f32,
) -> hs_ui::table::TableResponse {
    let h =
        (hs_ui::table::HEADER_HEIGHT + rows.len().max(1) as f32 * hs_ui::table::ROW_HEIGHT + 8.0)
            .min(height);
    ui.allocate_ui(egui::vec2(ui.available_width(), h), |ui| {
        DataTable {
            theme: cx.theme,
            columns: cols,
            rows,
            row_height: hs_ui::table::ROW_HEIGHT,
            show_filter: false,
            copy_row_label: &cx.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
            empty_hint: (!empty.is_empty()).then_some(empty),
        }
        .show(ui, state)
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        let mut attributes = BTreeMap::new();
        attributes.insert("department".to_string(), vec!["Engineering".to_string()]);
        attributes.insert("phone".to_string(), vec!["1".to_string(), "2".to_string()]);
        User {
            row: UserRow {
                id: "u1".into(),
                username: "alice".into(),
                email: "alice@example.com".into(),
                enabled: true,
                ..Default::default()
            },
            attributes,
            ..Default::default()
        }
    }

    #[test]
    fn an_unedited_draft_changes_nothing() {
        let u = user();
        assert!(Draft::of(&u).change(&u).fields.is_empty());
    }

    #[test]
    fn a_save_sends_only_what_was_edited_but_the_whole_attribute_map() {
        let u = user();
        let mut d = Draft::of(&u);
        d.email = "a.li@example.com".into();
        assert_eq!(d.change(&u).names(), vec!["email".to_string()]);
        // Editing one attribute sends every attribute kept, so the server
        // does not drop the others (§4.3).
        let row = d
            .attributes
            .iter_mut()
            .find(|r| r.key == "department")
            .unwrap();
        row.value = "Sales".into();
        let c = d.change(&u);
        let attrs = c.fields.get("attributes").unwrap();
        assert_eq!(attrs["department"], json!(["Sales"]));
        assert_eq!(attrs["phone"], json!(["1", "2"]));
    }

    #[test]
    fn a_disabled_attribute_row_is_left_out() {
        let u = user();
        let mut d = Draft::of(&u);
        for r in d.attributes.iter_mut().filter(|r| r.key == "phone") {
            r.enabled = false;
        }
        let c = d.change(&u);
        let attrs = c.fields.get("attributes").unwrap();
        assert!(attrs.get("phone").is_none());
        assert_eq!(attrs["department"], json!(["Engineering"]));
    }
}
