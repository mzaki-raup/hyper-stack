//! Groups (§4.4): the realm's group tree, loaded a level at a time, with
//! search; the group's page - Members, Role Mapping, Attributes, Details.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::table::{ColumnSpec, TableState};
use serde_json::json;

use super::common::{
    After, Cx, Load, WriteOk, WriteReq, ask, paging, state_note, take, write_button,
};
use super::dialogs::{Create, Dialog};
use super::roles_pick::{RolePick, mapping_rows};
use super::users::table;
use crate::provider::{
    Change, Group, IdmProvider, ObjRef, Page, Paged, RoleMappings, Section, Subject, UserQuery,
    UserRow,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Members,
    Roles,
    Attributes,
    Details,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Members, Tab::Roles, Tab::Attributes, Tab::Details];
    fn key(self) -> &'static str {
        match self {
            Tab::Members => "idm-tab-members",
            Tab::Roles => "idm-tab-role-mapping",
            Tab::Attributes => "idm-tab-attributes",
            Tab::Details => "idm-tab-details",
        }
    }
}

#[derive(Default)]
pub struct GroupsView {
    search: String,
    asked: Option<String>,
    page: Option<Page>,
    top: Load<Paged<Group>>,
    open: BTreeSet<String>,
    children: HashMap<String, Load<Paged<Group>>>,
    /// The group whose children are being read (one at a time).
    children_for: Option<String>,
    selected: Option<String>,
    item: Load<Group>,
    tab: Tab,
    members: Load<Paged<UserRow>>,
    members_page: Option<Page>,
    member_table: TableState,
    roles: Load<RoleMappings>,
    assign: Option<RolePick>,
    attributes: Option<Vec<KvRow>>,
    name: Option<String>,
    add_member: Option<UserPick>,
}

impl GroupsView {
    pub fn reset(&mut self) {
        *self = GroupsView::default();
    }

    pub fn refresh(&mut self) {
        self.top = Load::Idle;
        self.asked = None;
        self.children.clear();
        if self.selected.is_some() {
            self.reload_item();
        }
    }

    fn reload_item(&mut self) {
        self.item = Load::Idle;
        self.members = Load::Idle;
        self.roles = Load::Idle;
        self.attributes = None;
        self.name = None;
    }

    pub fn select(&mut self, id: String) {
        self.selected = Some(id);
        self.reload_item();
    }

    pub fn forget_selected(&mut self) {
        self.selected = None;
        self.reload_item();
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        take(cx.session, "groups.top", &mut self.top);
        let term = self.search.trim().to_string();
        if self.asked.as_deref() != Some(term.as_str()) || self.top.is_idle() {
            let (realm, page) = (cx.realm.to_string(), self.page.unwrap_or(cx.page));
            let t = term.clone();
            ask(
                cx.session,
                "groups.top",
                &mut self.top,
                move |p| async move {
                    let mut rows = p.groups(&realm, &t, page).await?;
                    if let Ok(Some(n)) = p.group_count(&realm, &t).await {
                        rows.total = Some(n);
                    }
                    Ok(rows)
                },
            );
            self.asked = Some(term);
        }
        if let Some(id) = self.children_for.clone() {
            let slot = self.children.entry(id).or_default();
            take(cx.session, "groups.children", slot);
            if !matches!(slot, Load::Loading) {
                self.children_for = None;
            }
        }
        if self.children_for.is_none()
            && let Some(id) = self
                .open
                .iter()
                .find(|id| self.children.get(*id).is_none_or(Load::is_idle))
                .cloned()
        {
            let (realm, page) = (cx.realm.to_string(), cx.page);
            let slot = self.children.entry(id.clone()).or_default();
            let i = id.clone();
            ask(cx.session, "groups.children", slot, move |p| async move {
                p.group_children(&realm, &i, page).await
            });
            self.children_for = Some(id);
        }
        let Some(id) = self.selected.clone() else {
            return;
        };
        take(cx.session, "groups.item", &mut self.item);
        if self.item.is_idle() {
            let (realm, i) = (cx.realm.to_string(), id.clone());
            ask(
                cx.session,
                "groups.item",
                &mut self.item,
                move |p| async move { p.group(&realm, &i).await },
            );
        }
        if let Some(g) = self.item.ready() {
            if self.attributes.is_none() {
                self.attributes = Some(rows_of(&g.attributes));
            }
            if self.name.is_none() {
                self.name = Some(g.name.clone());
            }
        }
        match self.tab {
            Tab::Members => {
                take(cx.session, "groups.members", &mut self.members);
                if self.members.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    let page = self.members_page.unwrap_or(cx.page);
                    ask(
                        cx.session,
                        "groups.members",
                        &mut self.members,
                        move |p| async move { p.group_members(&realm, &i, page).await },
                    );
                }
            }
            Tab::Roles => {
                take(cx.session, "groups.roles", &mut self.roles);
                if self.roles.is_idle() {
                    let (realm, i) = (cx.realm.to_string(), id.clone());
                    ask(
                        cx.session,
                        "groups.roles",
                        &mut self.roles,
                        move |p| async move { p.role_mappings(&realm, &Subject::Group(i)).await },
                    );
                }
            }
            Tab::Attributes | Tab::Details => {}
        }
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        hs_ui::toolbar::sub_strip(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if form::text_field_width(ui, &mut self.search, &cx.t("idm-search-groups"), 240.0)
                    .changed()
                {
                    self.page = None;
                }
                let may = cx.may_write("manage-users");
                if write_button(
                    ui,
                    theme,
                    &may,
                    true,
                    ph::PLUS,
                    &cx.t("idm-create-group"),
                    "",
                ) {
                    cx.out.dialog = Some(Dialog::Create(Create::Group {
                        realm: cx.realm.to_string(),
                        parent: None,
                        name: String::new(),
                    }));
                }
            });
        });
        ui.add_space(form::PANE_GAP);
        if !state_note(ui, theme, cx.s, &self.top) {
            return;
        }
        let Some(top) = self.top.ready().cloned() else {
            return;
        };
        if top.rows.is_empty() {
            let text = if self.search.trim().is_empty() {
                cx.t("idm-groups-empty")
            } else {
                cx.ta("idm-groups-no-match", &[("term", self.search.trim())])
            };
            form::hint(ui, theme, &text);
            return;
        }
        let searching = !self.search.trim().is_empty();
        let mut pick = None;
        let mut toggle = None;
        egui::ScrollArea::vertical()
            .id_salt("idm-groups-tree")
            .max_height((ui.available_height() - 52.0).max(120.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for g in &top.rows {
                    self.node(ui, cx, g, 0, searching, &mut pick, &mut toggle);
                }
            });
        if let Some(id) = toggle
            && !self.open.remove(&id)
        {
            self.open.insert(id);
        }
        if let Some(id) = pick
            && self.selected.as_deref() != Some(id.as_str())
        {
            self.select(id);
        }
        if let Some(page) = paging(ui, theme, cx.s, &top) {
            self.page = Some(page);
            self.top = Load::Idle;
        }
    }

    /// One tree row (a list row is one middle line: caret, picture, name,
    /// count), then its open children.
    #[allow(clippy::too_many_arguments)] // reason: one row's inputs and its two outcomes
    fn node(
        &self,
        ui: &mut egui::Ui,
        cx: &Cx<'_>,
        g: &Group,
        depth: usize,
        searching: bool,
        pick: &mut Option<String>,
        toggle: &mut Option<String>,
    ) {
        let theme = cx.theme;
        let (glyph, colour) = super::common::section_mark(Section::Groups);
        let has_children = g.sub_group_count.unwrap_or(g.sub_groups.len() as u64) > 0;
        let open = searching || self.open.contains(&g.id);
        form::centred_row(ui, |ui| {
            ui.add_space(depth as f32 * 16.0);
            if has_children && !searching {
                let caret = if open {
                    ph::CARET_DOWN
                } else {
                    ph::CARET_RIGHT
                };
                if ui.small_button(caret).clicked() {
                    *toggle = Some(g.id.clone());
                }
            } else {
                ui.add_space(22.0);
            }
            ui.label(egui::RichText::new(glyph).color(colour));
            let selected = self.selected.as_deref() == Some(g.id.as_str());
            let text = if searching { &g.path } else { &g.name };
            if form::selectable_label(ui, selected, text.as_str()).clicked() {
                *pick = Some(g.id.clone());
            }
            if let Some(n) = g.sub_group_count.filter(|n| *n > 0) {
                ui.label(egui::RichText::new(n.to_string()).color(theme.text_muted));
            }
        });
        if !open {
            return;
        }
        if searching {
            for c in &g.sub_groups {
                self.node(ui, cx, c, depth + 1, searching, pick, toggle);
            }
            return;
        }
        match self.children.get(&g.id) {
            Some(Load::Ready(kids)) => {
                for c in &kids.rows {
                    self.node(ui, cx, c, depth + 1, searching, pick, toggle);
                }
                if kids.more {
                    form::centred_row(ui, |ui| {
                        ui.add_space((depth + 1) as f32 * 16.0 + 22.0);
                        ui.label(
                            egui::RichText::new(cx.t("idm-more-children")).color(theme.text_muted),
                        );
                    });
                }
            }
            Some(Load::Failed(e)) => {
                ui.colored_label(theme.error, hs_core::fmt::sentence(&e.to_string()));
            }
            _ => {
                form::centred_row(ui, |ui| {
                    ui.add_space((depth + 1) as f32 * 16.0 + 22.0);
                    ui.spinner();
                });
            }
        }
    }

    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        if self.selected.is_none() {
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-group"));
            return;
        }
        let theme = cx.theme;
        if let Load::Failed(crate::provider::IdmError::NotFound { .. }) = self.item {
            cx.out
                .toasts
                .push((hs_core::tool::ToastLevel::Warning, cx.t("idm-group-gone")));
            self.forget_selected();
            self.refresh();
            return;
        }
        if !state_note(ui, theme, cx.s, &self.item) {
            return;
        }
        let Some(g) = self.item.ready().cloned() else {
            return;
        };
        let may = cx.may_write("manage-users");
        let target = ObjRef::new(&g.id, &g.name);
        form::pane(ui, |ui| {
            let (glyph, colour) = super::common::section_mark(Section::Groups);
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(glyph).size(20.0).color(colour));
                ui.label(egui::RichText::new(&g.path).strong().size(16.0));
            });
            ui.add_space(form::PANE_GAP);
            ui.horizontal_wrapped(|ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if write_button(ui, theme, &may, true, ph::PLUS, &cx.t("idm-create-sub-group"), "") {
                    cx.out.dialog = Some(Dialog::Create(Create::Group {
                        realm: cx.realm.to_string(),
                        parent: Some(target.clone()),
                        name: String::new(),
                    }));
                }
                if write_button(ui, theme, &may, true, ph::TRASH, &cx.t("idm-delete"), "") {
                    let subs = g.sub_group_count.unwrap_or(g.sub_groups.len() as u64);
                    let (r, t) = (cx.realm.to_string(), target.clone());
                    cx.out.dialog = Some(Dialog::Typed {
                        title: cx.t("idm-delete-group"),
                        body: cx.ta(
                            "idm-delete-group-confirm",
                            &[("name", &g.path), ("subs", &subs.to_string())],
                        ),
                        name: g.name.clone(),
                        ok: format!("{} {}", ph::TRASH, cx.t("idm-delete")),
                        typed: String::new(),
                        then: WriteReq::new(
                            cx.ta("idm-done-deleted", &[("name", &g.name)]),
                            After::Gone(Section::Groups),
                            move |p| async move { p.delete_group(&r, &t).await.map(|()| WriteOk::Done) },
                        ),
                    });
                }
                if form::button(ui, theme, ph::ARROW_SQUARE_OUT, &cx.t("idm-console")).clicked() {
                    cx.out.console = Some(format!("groups/{}", g.id));
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
            .id_salt("idm-group-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| match self.tab {
                    Tab::Members => self.members_tab(ui, cx, &g, &may),
                    Tab::Roles => self.roles_tab(ui, cx, &g, &may),
                    Tab::Attributes => self.attributes_tab(ui, cx, &g, &may),
                    Tab::Details => self.details_tab(ui, cx, &g, &may),
                });
            });
    }

    fn members_tab(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        g: &Group,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let group = ObjRef::new(&g.id, &g.path);
        if write_button(
            ui,
            theme,
            may,
            self.add_member.is_none(),
            ph::USER_PLUS,
            &cx.t("idm-add-member"),
            "",
        ) {
            self.add_member = Some(UserPick::default());
        }
        if let Some(pick) = self.add_member.as_mut() {
            ui.add_space(form::ROW_GAP);
            match pick.show(ui, cx) {
                Some(Some(user)) => {
                    let (r, grp) = (cx.realm.to_string(), group.clone());
                    cx.out.writes.push(WriteReq::new(
                        cx.ta(
                            "idm-done-joined",
                            &[("name", &user.name), ("group", &group.name)],
                        ),
                        After::Reload(Section::Groups),
                        move |p| async move {
                            p.join_group(&r, &user, &grp).await.map(|()| WriteOk::Done)
                        },
                    ));
                    self.add_member = None;
                }
                Some(None) => self.add_member = None,
                None => {}
            }
        }
        ui.add_space(form::ROW_GAP);
        form::hint(ui, theme, &cx.t("idm-members-direct-hint"));
        ui.add_space(form::ROW_GAP);
        if !state_note(ui, theme, cx.s, &self.members) {
            return;
        }
        let Some(members) = self.members.ready().cloned() else {
            return;
        };
        let cols = [
            ColumnSpec::new(cx.t("idm-username"), 180.0),
            ColumnSpec::new(cx.t("idm-email"), 200.0),
            ColumnSpec::new(cx.t("idm-name"), 160.0),
        ];
        let rows: Vec<Vec<String>> = members
            .rows
            .iter()
            .map(|u| vec![u.username.clone(), u.email.clone(), u.full_name()])
            .collect();
        table(
            ui,
            cx,
            &cols,
            &rows,
            &mut self.member_table,
            &cx.t("idm-members-empty"),
            320.0,
        );
        let picked = self
            .member_table
            .selected
            .and_then(|i| members.rows.get(i))
            .cloned();
        ui.add_space(form::PANE_GAP);
        form::centred_row(ui, |ui| {
            if write_button(
                ui,
                theme,
                may,
                picked.is_some(),
                ph::USER_MINUS,
                &cx.t("idm-remove-member"),
                &cx.t("idm-select-member"),
            ) && let Some(u) = &picked
            {
                let (r, grp, user) = (cx.realm.to_string(), group.clone(), u.as_ref());
                cx.out.writes.push(WriteReq::new(
                    cx.ta("idm-done-left", &[("name", &u.username), ("group", &group.name)]),
                    After::Reload(Section::Groups),
                    move |p| async move { p.leave_group(&r, &user, &grp).await.map(|()| WriteOk::Done) },
                ));
            }
        });
        if let Some(page) = paging(ui, theme, cx.s, &members) {
            self.members_page = Some(page);
            self.members = Load::Idle;
        }
    }

    fn roles_tab(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        g: &Group,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let who = Subject::Group(g.id.clone());
        if write_button(
            ui,
            theme,
            may,
            self.assign.is_none(),
            ph::SHIELD_STAR,
            &cx.t("idm-assign-role"),
            "",
        ) {
            self.assign = Some(RolePick::new(who.clone()));
        }
        if let Some(pick) = self.assign.as_mut() {
            ui.add_space(form::ROW_GAP);
            match pick.show(ui, cx, &g.path) {
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
        let Some(m) = self.roles.ready().cloned() else {
            return;
        };
        if let Some(req) = mapping_rows(ui, cx, &who, &g.path, &m, None, may) {
            cx.out.writes.push(req);
        }
    }

    fn attributes_tab(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        g: &Group,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let Some(rows) = self.attributes.as_mut() else {
            return;
        };
        ui.add_enabled_ui(may.is_ok(), |ui| {
            KvEditor {
                theme,
                key_hint: &cx.t("idm-attribute"),
                value_hint: &cx.t("idm-value"),
                add_label: &cx.t("idm-add-attribute"),
                allow_secret: false,
            }
            .show(ui, rows);
        });
        let map = map_of(rows);
        let change = if map != g.attributes {
            Change::default().set("attributes", json!(map))
        } else {
            Change::default()
        };
        ui.add_space(form::ROW_GAP);
        if save_row(ui, cx, may, &change) {
            self.review(cx, g, change);
        }
        if form::button(ui, theme, ph::ARROW_COUNTER_CLOCKWISE, &cx.t("idm-revert")).clicked() {
            self.attributes = Some(rows_of(&g.attributes));
        }
    }

    fn details_tab(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        g: &Group,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let Some(name) = self.name.as_mut() else {
            return;
        };
        form::detail_table(
            ui,
            theme,
            "idm-group-facts",
            [
                (cx.t("idm-id"), g.id.clone()),
                (cx.t("idm-path"), g.path.clone()),
            ]
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect::<Vec<_>>(),
        );
        ui.add_space(form::ROW_GAP);
        ui.add_enabled_ui(may.is_ok(), |ui| {
            form::field_grid(ui, "idm-group-details", |ui| {
                form::label(ui, &cx.t("idm-name"));
                form::text_field(ui, name, "");
                ui.end_row();
            });
        });
        let change = if name.trim() != g.name {
            Change::default().set("name", name.trim())
        } else {
            Change::default()
        };
        ui.add_space(form::ROW_GAP);
        if save_row(ui, cx, may, &change) {
            self.review(cx, g, change);
        }
    }

    /// The review before a group's save (§6.2).
    fn review(&mut self, cx: &mut Cx<'_>, g: &Group, change: Change) {
        let rows = change
            .review(&g.raw)
            .into_iter()
            .map(|(k, o, n)| (k, o.to_string(), n.to_string()))
            .collect();
        let (realm, target) = (cx.realm.to_string(), ObjRef::new(&g.id, &g.name));
        cx.out.dialog = Some(Dialog::Review {
            title: cx.ta("idm-review-title", &[("name", &g.path)]),
            rows,
            stale: false,
            then: WriteReq::new(
                cx.ta("idm-done-saved", &[("name", &g.path)]),
                After::Reload(Section::Groups),
                move |p| async move {
                    p.update_group(&realm, &target, &change)
                        .await
                        .map(|()| WriteOk::Done)
                },
            ),
        });
    }
}

/// Save under an editable page; true when it was pressed.
fn save_row(ui: &mut egui::Ui, cx: &Cx<'_>, may: &Result<(), String>, change: &Change) -> bool {
    write_button(
        ui,
        cx.theme,
        may,
        !change.fields.is_empty(),
        ph::FLOPPY_DISK,
        &cx.t("idm-save"),
        &cx.t("idm-nothing-changed"),
    )
}

fn rows_of(map: &BTreeMap<String, Vec<String>>) -> Vec<KvRow> {
    map.iter()
        .flat_map(|(k, vs)| {
            vs.iter().map(move |v| KvRow {
                enabled: true,
                key: k.clone(),
                value: v.clone(),
                secret: false,
            })
        })
        .collect()
}

fn map_of(rows: &[KvRow]) -> BTreeMap<String, Vec<String>> {
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in rows
        .iter()
        .filter(|r| r.enabled && !r.key.trim().is_empty())
    {
        map.entry(r.key.trim().to_string())
            .or_default()
            .push(r.value.clone());
    }
    map
}

/// A user search for Add Member.
#[derive(Default)]
pub struct UserPick {
    search: String,
    asked: Option<String>,
    found: Load<Paged<UserRow>>,
}

impl UserPick {
    pub fn show(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<Option<ObjRef>> {
        let theme = cx.theme;
        let mut out = None;
        take(cx.session, "pick.users", &mut self.found);
        let term = self.search.trim().to_string();
        if self.asked.as_deref() != Some(term.as_str()) {
            let (realm, page) = (cx.realm.to_string(), cx.page);
            let q = UserQuery {
                search: term.clone(),
                ..Default::default()
            };
            ask(
                cx.session,
                "pick.users",
                &mut self.found,
                move |p| async move { p.users(&realm, &q, page).await },
            );
            self.asked = Some(term);
        }
        form::well(ui, theme, None, |ui| {
            form::centred_row(ui, |ui| {
                form::text_field_width(ui, &mut self.search, &cx.t("idm-search-users"), 260.0);
                if form::button(ui, theme, ph::X, &cx.t("cancel")).clicked() {
                    out = Some(None);
                }
            });
            ui.add_space(form::PANE_GAP);
            if !state_note(ui, theme, cx.s, &self.found) {
                return;
            }
            let Some(found) = self.found.ready() else {
                return;
            };
            let (glyph, colour) = super::common::section_mark(Section::Users);
            egui::ScrollArea::vertical()
                .id_salt("idm-user-pick")
                .max_height(200.0)
                .show(ui, |ui| {
                    for u in &found.rows {
                        form::centred_row(ui, |ui| {
                            ui.label(egui::RichText::new(glyph).color(colour));
                            let label = if u.email.is_empty() {
                                u.username.clone()
                            } else {
                                format!("{}  ·  {}", u.username, u.email)
                            };
                            if form::selectable_label(ui, false, label).clicked() {
                                out = Some(Some(u.as_ref()));
                            }
                        });
                    }
                });
        });
        out
    }
}
