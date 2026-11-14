//! Role mapping (§4.3, §4.4): the roles a user or group holds, with
//! Unassign per assigned row, and the Assign Role picker - the realm's
//! available roles, or a client then its available roles.

use std::collections::BTreeSet;

use hs_ui::form;
use hs_ui::icons::ph;

use super::common::{After, Cx, Load, WriteOk, WriteReq, ask, state_note, take, write_button};
use crate::provider::{Client, IdmProvider, ObjRef, Paged, Role, RoleMappings, Section, Subject};

fn after(who: &Subject) -> After {
    match who {
        Subject::User(_) => After::Reload(Section::Users),
        Subject::Group(_) => After::Reload(Section::Groups),
    }
}

/// The subject's mappings as rows: realm roles, then each client's, each
/// with Unassign; with `effective`, the inherited realm roles too, marked
/// and without Unassign. Returns the Unassign asked for.
pub fn mapping_rows(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    who: &Subject,
    name: &str,
    m: &RoleMappings,
    effective: Option<&[Role]>,
    may: &Result<(), String>,
) -> Option<WriteReq> {
    let theme = cx.theme;
    let mut unassign: Option<(Option<ObjRef>, Role)> = None;
    let direct: BTreeSet<&str> = m.realm.iter().map(|r| r.name.as_str()).collect();
    let inherited: Vec<&Role> = effective
        .map(|e| {
            e.iter()
                .filter(|r| !direct.contains(r.name.as_str()))
                .collect()
        })
        .unwrap_or_default();
    if m.realm.is_empty() && m.clients.is_empty() && inherited.is_empty() {
        form::hint(ui, theme, &cx.t("idm-roles-none"));
        return None;
    }
    let (realm_glyph, realm_colour) = super::common::section_mark(Section::RealmRoles);
    let (client_glyph, client_colour) = super::common::section_mark(Section::ClientRoles);
    form::subsection(ui, theme, &cx.t("idm-realm-roles"));
    ui.add_space(form::ROW_GAP / 2.0);
    for r in &m.realm {
        form::centred_row(ui, |ui| {
            ui.label(egui::RichText::new(realm_glyph).color(realm_colour));
            ui.label(&r.name);
            if write_button(ui, theme, may, true, ph::X, &cx.t("idm-unassign"), "") {
                unassign = Some((None, r.clone()));
            }
        });
        ui.add_space(form::ROW_GAP / 2.0);
    }
    for r in &inherited {
        form::centred_row(ui, |ui| {
            ui.label(egui::RichText::new(realm_glyph).color(theme.text_muted));
            ui.label(egui::RichText::new(&r.name).color(theme.text_muted));
            super::common::chip(ui, &cx.t("idm-inherited"), theme.text_muted);
        })
        .response
        .on_hover_text(cx.t("idm-inherited-tip"));
        ui.add_space(form::ROW_GAP / 2.0);
    }
    for c in &m.clients {
        ui.add_space(form::ROW_GAP);
        form::subsection(ui, theme, &c.client);
        ui.add_space(form::ROW_GAP / 2.0);
        for r in &c.roles {
            form::centred_row(ui, |ui| {
                ui.label(egui::RichText::new(client_glyph).color(client_colour));
                ui.label(&r.name);
                if write_button(ui, theme, may, true, ph::X, &cx.t("idm-unassign"), "") {
                    unassign = Some((Some(ObjRef::new(&c.id, &c.client)), r.clone()));
                }
            });
            ui.add_space(form::ROW_GAP / 2.0);
        }
    }
    let (client, role) = unassign?;
    let (realm, who2) = (cx.realm.to_string(), who.clone());
    let done = cx.ta(
        "idm-done-unassigned",
        &[("role", &role.name), ("name", name)],
    );
    Some(WriteReq::new(done, after(who), move |p| async move {
        match client {
            None => p.remove_realm_roles(&realm, &who2, &[role]).await,
            Some(c) => p.remove_client_roles(&realm, &who2, &c, &[role]).await,
        }
        .map(|()| WriteOk::Done)
    }))
}

/// Assign Role: realm roles, or a client's.
pub struct RolePick {
    who: Subject,
    client_mode: bool,
    client_search: String,
    client_asked: Option<String>,
    clients: Load<Paged<Client>>,
    client: Option<ObjRef>,
    roles_for: Option<Option<String>>,
    roles: Load<Vec<Role>>,
    chosen: BTreeSet<String>,
}

impl RolePick {
    pub fn new(who: Subject) -> RolePick {
        RolePick {
            who,
            client_mode: false,
            client_search: String::new(),
            client_asked: None,
            clients: Load::Idle,
            client: None,
            roles_for: None,
            roles: Load::Idle,
            chosen: BTreeSet::new(),
        }
    }

    /// `Some(Some(write))` on Assign, `Some(None)` on Cancel, `None`
    /// while open. `name` is the subject's name for the toast.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        name: &str,
    ) -> Option<Option<WriteReq>> {
        let theme = cx.theme;
        let mut out = None;
        take(cx.session, "pick.roles", &mut self.roles);
        take(cx.session, "pick.clients", &mut self.clients);
        let want = self
            .client_mode
            .then(|| self.client.as_ref().map(|c| c.id.clone()))
            .flatten();
        let ready_for = if self.client_mode {
            self.client.is_some()
        } else {
            true
        };
        if ready_for && self.roles_for.as_ref() != Some(&want) {
            let (realm, who) = (cx.realm.to_string(), self.who.clone());
            let w = want.clone();
            ask(
                cx.session,
                "pick.roles",
                &mut self.roles,
                move |p| async move {
                    match w {
                        None => p.available_realm_roles(&realm, &who).await,
                        Some(c) => p.available_client_roles(&realm, &who, &c).await,
                    }
                },
            );
            self.roles_for = Some(want);
            self.chosen.clear();
        }
        if self.client_mode
            && self.client.is_none()
            && self.client_asked.as_deref() != Some(self.client_search.trim())
        {
            let term = self.client_search.trim().to_string();
            let (realm, page) = (cx.realm.to_string(), cx.page);
            let t = term.clone();
            ask(
                cx.session,
                "pick.clients",
                &mut self.clients,
                move |p| async move { p.clients(&realm, &t, page).await },
            );
            self.client_asked = Some(term);
        }
        form::well(ui, theme, None, |ui| {
            form::centred_row(ui, |ui| {
                if ui
                    .radio(!self.client_mode, cx.t("idm-realm-roles"))
                    .clicked()
                    && self.client_mode
                {
                    self.client_mode = false;
                    self.client = None;
                }
                ui.add_space(form::OPTION_GAP);
                if ui
                    .radio(self.client_mode, cx.t("idm-client-roles"))
                    .clicked()
                    && !self.client_mode
                {
                    self.client_mode = true;
                    self.client = None;
                    self.client_asked = None;
                }
            });
            ui.add_space(form::PANE_GAP);
            if self.client_mode && self.client.is_none() {
                form::text_field_width(
                    ui,
                    &mut self.client_search,
                    &cx.t("idm-search-clients"),
                    260.0,
                );
                ui.add_space(form::PANE_GAP);
                if state_note(ui, theme, cx.s, &self.clients)
                    && let Some(found) = self.clients.ready()
                {
                    let (glyph, colour) = super::common::section_mark(Section::Clients);
                    egui::ScrollArea::vertical()
                        .id_salt("idm-role-pick-clients")
                        .max_height(180.0)
                        .show(ui, |ui| {
                            for c in &found.rows {
                                form::centred_row(ui, |ui| {
                                    ui.label(egui::RichText::new(glyph).color(colour));
                                    if form::selectable_label(ui, false, &c.client_id).clicked() {
                                        self.client = Some(ObjRef::new(&c.id, &c.client_id));
                                    }
                                });
                            }
                        });
                }
            } else {
                if let Some(name) = self.client.as_ref().map(|c| c.name.clone()) {
                    form::centred_row(ui, |ui| {
                        ui.label(egui::RichText::new(name).strong());
                        if ui.small_button(cx.t("idm-change")).clicked() {
                            self.client = None;
                            self.client_asked = None;
                        }
                    });
                    ui.add_space(form::PANE_GAP);
                }
                if state_note(ui, theme, cx.s, &self.roles)
                    && let Some(roles) = self.roles.ready()
                {
                    if roles.is_empty() {
                        form::hint(ui, theme, &cx.t("idm-roles-none-available"));
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("idm-role-pick")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for r in roles {
                                let mut on = self.chosen.contains(&r.name);
                                if ui
                                    .checkbox(&mut on, &r.name)
                                    .on_hover_text(&r.description)
                                    .changed()
                                {
                                    if on {
                                        self.chosen.insert(r.name.clone());
                                    } else {
                                        self.chosen.remove(&r.name);
                                    }
                                }
                                ui.add_space(form::ROW_GAP / 2.0);
                            }
                        });
                }
            }
            ui.add_space(form::ROW_GAP);
            form::centred_row(ui, |ui| {
                let n = self.chosen.len();
                if ui
                    .add_enabled_ui(n > 0, |ui| {
                        form::primary_button(
                            ui,
                            theme,
                            &format!("{} {}", ph::CHECK, cx.t("idm-assign")),
                        )
                    })
                    .inner
                    .clicked()
                {
                    let roles: Vec<Role> = self
                        .roles
                        .ready()
                        .map(|rs| {
                            rs.iter()
                                .filter(|r| self.chosen.contains(&r.name))
                                .cloned()
                                .collect()
                        })
                        .unwrap_or_default();
                    let names = roles
                        .iter()
                        .map(|r| r.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let (realm, who, client) =
                        (cx.realm.to_string(), self.who.clone(), self.client.clone());
                    let done = cx.ta("idm-done-assigned", &[("role", &names), ("name", name)]);
                    out = Some(Some(WriteReq::new(
                        done,
                        after(&self.who),
                        move |p| async move {
                            match client {
                                None => p.add_realm_roles(&realm, &who, &roles).await,
                                Some(c) => p.add_client_roles(&realm, &who, &c, &roles).await,
                            }
                            .map(|()| WriteOk::Done)
                        },
                    )));
                }
                ui.add_space(form::BUTTON_GAP - ui.spacing().item_spacing.x);
                if form::button(ui, theme, ph::X, &cx.t("cancel")).clicked() {
                    out = Some(None);
                }
            });
        });
        out
    }
}
