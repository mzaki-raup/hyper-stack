//! Realm Roles and Client Roles (§4.5): the roles, searched and paged
//! with More; the role's page - Details, Composite Roles, Users in Role,
//! Groups in Role. Client Roles first picks the client.

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};

use super::common::{
    After, Cx, Load, WriteOk, WriteReq, ask, chip, paging, state_note, take, write_button,
};
use super::dialogs::{Create, Dialog};
use super::users::table;
use crate::provider::{
    Change, Client, Group, IdmProvider, NewRole, Page, Paged, Role, RoleScope, Section, UserRow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Details,
    Composites,
    Users,
    Groups,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Details, Tab::Composites, Tab::Users, Tab::Groups];
    fn key(self) -> &'static str {
        match self {
            Tab::Details => "idm-tab-details",
            Tab::Composites => "idm-tab-composites",
            Tab::Users => "idm-tab-users-in-role",
            Tab::Groups => "idm-tab-groups-in-role",
        }
    }
}

pub struct RolesView {
    /// Client Roles (a client is picked first) rather than Realm Roles.
    client_mode: bool,
    client_text: String,
    client_asked: Option<String>,
    clients: Load<Paged<Client>>,
    /// The picked client: internal id and client id.
    client: Option<(String, String)>,
    search: String,
    asked: Option<(String, Option<String>)>,
    page: Option<Page>,
    list: Load<Paged<Role>>,
    table: TableState,
    selected: Option<String>,
    item: Load<Role>,
    tab: Tab,
    description: Option<String>,
    composites: Load<Vec<Role>>,
    users: Load<Paged<UserRow>>,
    users_page: Option<Page>,
    groups: Load<Paged<Group>>,
}

impl RolesView {
    pub fn new(client_mode: bool) -> RolesView {
        RolesView {
            client_mode,
            client_text: String::new(),
            client_asked: None,
            clients: Load::Idle,
            client: None,
            search: String::new(),
            asked: None,
            page: None,
            list: Load::Idle,
            table: TableState::default(),
            selected: None,
            item: Load::Idle,
            tab: Tab::Details,
            description: None,
            composites: Load::Idle,
            users: Load::Idle,
            users_page: None,
            groups: Load::Idle,
        }
    }

    fn section(&self) -> Section {
        if self.client_mode {
            Section::ClientRoles
        } else {
            Section::RealmRoles
        }
    }

    fn scope(&self) -> Option<RoleScope> {
        if self.client_mode {
            self.client
                .as_ref()
                .map(|(id, _)| RoleScope::Client(id.clone()))
        } else {
            Some(RoleScope::Realm)
        }
    }

    pub fn reset(&mut self) {
        *self = RolesView::new(self.client_mode);
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
        self.description = None;
        self.composites = Load::Idle;
        self.users = Load::Idle;
        self.users_page = None;
        self.groups = Load::Idle;
    }

    pub fn select(&mut self, id: String) {
        self.selected = Some(id);
        self.reload_item();
    }

    pub fn forget_selected(&mut self) {
        if let (Some(id), Some(list)) = (self.selected.take(), self.list.ready_mut()) {
            list.rows.retain(|r| r.id != id);
        }
        self.reload_item();
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        take(cx.session, "roles.clients", &mut self.clients);
        if self.client_mode && self.client_asked.as_deref() != Some(self.client_text.trim()) {
            let term = self.client_text.trim().to_string();
            let (realm, page) = (cx.realm.to_string(), cx.page);
            let t = term.clone();
            ask(
                cx.session,
                "roles.clients",
                &mut self.clients,
                move |p| async move { p.clients(&realm, &t, page).await },
            );
            self.client_asked = Some(term);
        }
        take(cx.session, "roles.list", &mut self.list);
        let Some(scope) = self.scope() else {
            return;
        };
        let key = (
            self.search.trim().to_string(),
            self.client.as_ref().map(|c| c.0.clone()),
        );
        if self.asked.as_ref() != Some(&key) || self.list.is_idle() {
            let (realm, page) = (cx.realm.to_string(), self.page.unwrap_or(cx.page));
            let (term, sc) = (key.0.clone(), scope.clone());
            ask(
                cx.session,
                "roles.list",
                &mut self.list,
                move |p| async move { p.roles(&realm, &sc, &term, page).await },
            );
            self.asked = Some(key);
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        take(cx.session, "roles.item", &mut self.item);
        if self.item.is_idle() {
            let (realm, i) = (cx.realm.to_string(), id.clone());
            ask(
                cx.session,
                "roles.item",
                &mut self.item,
                move |p| async move { p.role(&realm, &i).await },
            );
        }
        if self.description.is_none()
            && let Some(r) = self.item.ready()
        {
            self.description = Some(r.description.clone());
        }
        let name = self.item.ready().map(|r| r.name.clone());
        match (self.tab, name) {
            (Tab::Composites, _) => {
                take(cx.session, "roles.composites", &mut self.composites);
                if self.composites.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "roles.composites",
                        &mut self.composites,
                        move |p| async move { p.role_composites(&realm, &i).await },
                    );
                }
            }
            (Tab::Users, Some(name)) => {
                take(cx.session, "roles.users", &mut self.users);
                if self.users.is_idle() {
                    let (realm, page, sc) = (
                        cx.realm.to_string(),
                        self.users_page.unwrap_or(cx.page),
                        scope.clone(),
                    );
                    ask(
                        cx.session,
                        "roles.users",
                        &mut self.users,
                        move |p| async move { p.role_users(&realm, &sc, &name, page).await },
                    );
                }
            }
            (Tab::Groups, Some(name)) => {
                take(cx.session, "roles.groups", &mut self.groups);
                if self.groups.is_idle() {
                    let (realm, page, sc) = (cx.realm.to_string(), cx.page, scope.clone());
                    ask(
                        cx.session,
                        "roles.groups",
                        &mut self.groups,
                        move |p| async move { p.role_groups(&realm, &sc, &name, page).await },
                    );
                }
            }
            _ => {}
        }
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        if self.client_mode {
            let options: Vec<String> = self
                .clients
                .ready()
                .map(|c| c.rows.iter().map(|c| c.client_id.clone()).collect())
                .unwrap_or_default();
            form::centred_row(ui, |ui| {
                form::label(ui, &cx.t("idm-client"));
                let r = form::pick_field_width(
                    ui,
                    theme,
                    &mut self.client_text,
                    &options,
                    &cx.t("idm-pick-client"),
                    260.0,
                );
                let typed = self.client_text.trim().to_string();
                if (r.lost_focus() || options.contains(&typed))
                    && let Some(c) = self
                        .clients
                        .ready()
                        .and_then(|l| l.rows.iter().find(|c| c.client_id == typed))
                    && self.client.as_ref().map(|x| &x.1) != Some(&typed)
                {
                    self.client = Some((c.id.clone(), c.client_id.clone()));
                    self.selected = None;
                    self.reload_item();
                    self.list = Load::Idle;
                }
            });
            ui.add_space(form::PANE_GAP);
            if self.client.is_none() {
                form::note(ui, theme, ph::INFO, &cx.t("idm-pick-client-hint"));
                return;
            }
        }
        let builtin_client = self
            .client
            .as_ref()
            .is_some_and(|(_, cid)| crate::provider::is_builtin_client(cid, cx.realm));
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if form::text_field_width(ui, &mut self.search, &cx.t("idm-search-roles"), 240.0)
                    .changed()
                {
                    self.page = None;
                }
                let role = if self.client_mode {
                    "manage-clients"
                } else {
                    "manage-realm"
                };
                let may = cx.may_write(role);
                if write_button(
                    ui,
                    theme,
                    &may,
                    !builtin_client,
                    ph::PLUS,
                    &cx.t("idm-create-role"),
                    &cx.t("idm-builtin-client"),
                ) && let Some(scope) = self.scope()
                {
                    cx.out.dialog = Some(Dialog::Create(Create::Role {
                        realm: cx.realm.to_string(),
                        scope,
                        form: NewRole::default(),
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
            ColumnSpec::new(cx.t("idm-name"), 220.0),
            ColumnSpec::new(cx.t("idm-composite"), 100.0),
            ColumnSpec::new(cx.t("idm-description"), 260.0),
        ];
        let rows: Vec<Vec<String>> = list
            .rows
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    if r.composite {
                        cx.t("idm-yes")
                    } else {
                        cx.t("idm-no")
                    },
                    r.description.clone(),
                ]
            })
            .collect();
        self.table.selected = self
            .selected
            .as_deref()
            .and_then(|id| list.rows.iter().position(|r| r.id == id));
        let empty = cx.t("idm-roles-empty");
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
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-role"));
            return;
        }
        let theme = cx.theme;
        if let Load::Failed(crate::provider::IdmError::NotFound { .. }) = self.item {
            cx.out
                .toasts
                .push((hs_core::tool::ToastLevel::Warning, cx.t("idm-role-gone")));
            self.forget_selected();
            return;
        }
        if !state_note(ui, theme, cx.s, &self.item) {
            return;
        }
        let Some(role) = self.item.ready().cloned() else {
            return;
        };
        let client_id = self.client.as_ref().map(|c| c.1.clone());
        let builtin = role.is_builtin(cx.realm, client_id.as_deref());
        let may = cx.may_write(if self.client_mode {
            "manage-clients"
        } else {
            "manage-realm"
        });
        let section = self.section();
        form::pane(ui, |ui| {
            let (glyph, colour) = super::common::section_mark(section);
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(20.0).color(colour));
                ui.label(egui::RichText::new(&role.name).strong().size(16.0));
                if role.composite {
                    chip(ui, &cx.t("idm-composite"), theme.accent);
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
                    &cx.t("idm-builtin-role"),
                ) {
                    let (r, t) = (cx.realm.to_string(), role.as_ref());
                    let warn = if role.composite {
                        cx.t("idm-delete-role-composite")
                    } else {
                        String::new()
                    };
                    cx.out.dialog = Some(Dialog::Typed {
                        title: cx.t("idm-delete-role"),
                        body: format!(
                            "{} {warn}",
                            cx.ta("idm-delete-role-confirm", &[("name", &role.name)])
                        )
                        .trim()
                        .to_string(),
                        name: role.name.clone(),
                        ok: format!("{} {}", ph::TRASH, cx.t("idm-delete")),
                        typed: String::new(),
                        then: WriteReq::new(
                            cx.ta("idm-done-deleted", &[("name", &role.name)]),
                            After::Gone(section),
                            move |p| async move { p.delete_role(&r, &t).await.map(|()| WriteOk::Done) },
                        ),
                    });
                }
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked() {
                    cx.out.console = Some(format!("roles/{}/details", role.id));
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
            .id_salt("idm-role-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| match self.tab {
                    Tab::Details => self.details(ui, cx, &role, &may, builtin),
                    Tab::Composites => self.composites_tab(ui, cx),
                    Tab::Users => self.users_tab(ui, cx),
                    Tab::Groups => self.groups_tab(ui, cx),
                });
            });
    }

    fn details(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        role: &Role,
        may: &Result<(), String>,
        builtin: bool,
    ) {
        let theme = cx.theme;
        let Some(description) = self.description.as_mut() else {
            return;
        };
        form::detail_table(
            ui,
            theme,
            "idm-role-facts",
            [
                (cx.t("idm-id"), role.id.clone()),
                (cx.t("idm-name"), role.name.clone()),
            ]
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect::<Vec<_>>(),
        );
        ui.add_space(form::ROW_GAP);
        let editable = may.is_ok() && !builtin;
        ui.add_enabled_ui(editable, |ui| {
            form::field_grid(ui, "idm-role-details", |ui| {
                form::label(ui, &cx.t("idm-description"));
                form::text_field(ui, description, "");
                ui.end_row();
            });
        });
        let change = if description.trim() != role.description {
            Change::default().set("description", description.trim())
        } else {
            Change::default()
        };
        ui.add_space(form::ROW_GAP);
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
                .review(&role.raw)
                .into_iter()
                .map(|(k, o, n)| (k, o.to_string(), n.to_string()))
                .collect();
            let (realm, target, section) = (cx.realm.to_string(), role.as_ref(), self.section());
            cx.out.dialog = Some(Dialog::Review {
                title: cx.ta("idm-review-title", &[("name", &role.name)]),
                rows,
                stale: false,
                then: WriteReq::new(
                    cx.ta("idm-done-saved", &[("name", &role.name)]),
                    After::Reload(section),
                    move |p| async move {
                        p.update_role(&realm, &target, &change)
                            .await
                            .map(|()| WriteOk::Done)
                    },
                ),
            });
        }
    }

    fn composites_tab(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        form::hint(ui, theme, &cx.t("idm-composites-hint"));
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.composites) {
            return;
        }
        let Some(roles) = self.composites.ready() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-name"), 200.0),
            ColumnSpec::new(cx.t("idm-kind"), 120.0),
            ColumnSpec::new(cx.t("idm-description"), 220.0),
        ];
        let rows: Vec<Vec<String>> = roles
            .iter()
            .map(|r| {
                vec![
                    r.name.clone(),
                    if r.client_role {
                        cx.t("idm-client-role")
                    } else {
                        cx.t("idm-realm-role")
                    },
                    r.description.clone(),
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
            &cx.t("idm-composites-empty"),
            320.0,
        );
    }

    fn users_tab(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        form::hint(ui, theme, &cx.t("idm-users-in-role-hint"));
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.users) {
            return;
        }
        let Some(users) = self.users.ready().cloned() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-username"), 180.0),
            ColumnSpec::new(cx.t("idm-email"), 200.0),
        ];
        let rows: Vec<Vec<String>> = users
            .rows
            .iter()
            .map(|u| vec![u.username.clone(), u.email.clone()])
            .collect();
        let mut state = TableState::default();
        table(
            ui,
            cx,
            &cols,
            &rows,
            &mut state,
            &cx.t("idm-users-in-role-empty"),
            320.0,
        );
        if let Some(page) = paging(ui, theme, cx.s, &users) {
            self.users_page = Some(page);
            self.users = Load::Idle;
        }
    }

    fn groups_tab(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let theme = cx.theme;
        if !state_note(ui, theme, cx.s, &self.groups) {
            return;
        }
        let Some(groups) = self.groups.ready() else {
            return;
        };
        if groups.rows.is_empty() {
            form::hint(ui, theme, &cx.t("idm-groups-in-role-empty"));
            return;
        }
        let (glyph, colour) = super::common::section_mark(Section::Groups);
        for g in &groups.rows {
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).color(colour));
                ui.label(&g.path);
            });
            ui.add_space(form::ROW_GAP / 2.0);
        }
    }
}
