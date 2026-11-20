//! Directory (§4.12, FEAT-0117): a directory's tree, loaded a level at a
//! time from the realm's naming context; the selected entry's facts, its
//! attributes in the raw editor with a review before each save, Move and
//! Delete. Only a directory has it (`Caps::section`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::kv_editor::{KvEditor, KvRow};

use super::common::{After, Cx, Load, WriteOk, WriteReq, ask, state_note, take, write_button};
use super::dialogs::Dialog;
use super::groups::{map_of, rows_of};
use crate::provider::{DirEntry, DirKind, IdmProvider, ObjRef, Paged, Section};

#[derive(Default)]
pub struct DirectoryView {
    /// The nodes shown open, the realm's root among them once read.
    open: BTreeSet<String>,
    children: HashMap<String, Load<Paged<DirEntry>>>,
    /// The node whose children are being read (one at a time).
    children_for: Option<String>,
    /// The realm the tree was opened on.
    root: Option<String>,
    selected: Option<String>,
    item: Load<DirEntry>,
    rows: Option<Vec<KvRow>>,
    /// The fresh read a save is reviewed against (§4.3).
    saving: Option<Load<DirEntry>>,
    move_to: String,
}

/// The tree's picture and colour for an entry.
fn mark(kind: DirKind) -> (&'static str, egui::Color32) {
    match kind {
        DirKind::Container => (ph::FOLDER, egui::Color32::from_rgb(0xe8, 0xa3, 0x3d)),
        DirKind::Person => super::common::section_mark(Section::Users),
        DirKind::Group => super::common::section_mark(Section::Groups),
        DirKind::Other => (ph::FILE_TEXT, egui::Color32::from_rgb(0x90, 0xa4, 0xae)),
    }
}

/// An attribute's values as the review shows them.
fn joined(v: Option<&Vec<String>>) -> String {
    v.map(|v| v.join("; ")).unwrap_or_default()
}

impl DirectoryView {
    pub fn reset(&mut self) {
        *self = DirectoryView::default();
    }

    pub fn refresh(&mut self) {
        self.children.clear();
        self.children_for = None;
        if self.selected.is_some() {
            self.reload_item();
        }
    }

    fn reload_item(&mut self) {
        self.item = Load::Idle;
        self.rows = None;
        self.saving = None;
        self.move_to.clear();
    }

    pub fn select(&mut self, dn: String) {
        self.selected = Some(dn);
        self.reload_item();
    }

    pub fn forget_selected(&mut self) {
        self.selected = None;
        self.reload_item();
    }

    fn fetch(&mut self, cx: &mut Cx<'_>) {
        let realm = cx.realm.to_string();
        if self.root.as_deref() != Some(realm.as_str()) {
            self.reset();
            self.open.insert(realm.clone());
            self.root = Some(realm.clone());
        }
        if let Some(dn) = self.children_for.clone() {
            let slot = self.children.entry(dn).or_default();
            take(cx.session, "dir.children", slot);
            if !matches!(slot, Load::Loading) {
                self.children_for = None;
            }
        }
        if self.children_for.is_none()
            && let Some(dn) = self
                .open
                .iter()
                .find(|d| self.children.get(*d).is_none_or(Load::is_idle))
                .cloned()
        {
            let (r, page) = (realm.clone(), cx.page);
            let slot = self.children.entry(dn.clone()).or_default();
            let d = dn.clone();
            ask(cx.session, "dir.children", slot, move |p| async move {
                p.dir_children(&r, &d, page).await
            });
            self.children_for = Some(dn);
        }
        let Some(dn) = self.selected.clone() else {
            return;
        };
        take(cx.session, "dir.item", &mut self.item);
        if self.item.is_idle() {
            let r = realm.clone();
            ask(
                cx.session,
                "dir.item",
                &mut self.item,
                move |p| async move { p.dir_entry(&r, &dn).await },
            );
        }
        if let (None, Some(e)) = (&self.rows, self.item.ready()) {
            self.rows = Some(rows_of(&e.attributes));
        }
        if let Some(slot) = self.saving.as_mut() {
            take(cx.session, "dir.fresh", slot);
        }
    }

    pub fn list(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.fetch(cx);
        let theme = cx.theme;
        form::hint(ui, theme, &cx.t("idm-directory-hint"));
        ui.add_space(form::PANE_GAP);
        let root = cx.realm.to_string();
        let mut pick = None;
        let mut toggle = None;
        // As wide as the tree, so its scrollbar stays beside it rather than
        // at the edge of a list wider than its room (under the entry).
        egui::ScrollArea::vertical()
            .id_salt("idm-directory-tree")
            .auto_shrink([true, false])
            .show(ui, |ui| {
                // The naming context itself, always open.
                form::centred_row(ui, |ui| {
                    ui.add_space(22.0);
                    let (g, c) = mark(DirKind::Container);
                    ui.label(egui::RichText::new(g).color(c));
                    let selected = self.selected.as_deref() == Some(root.as_str());
                    if form::selectable_label(ui, selected, root.as_str()).clicked() {
                        pick = Some(root.clone());
                    }
                });
                self.level(ui, cx, &root, 1, &mut pick, &mut toggle);
            });
        if let Some(dn) = toggle
            && !self.open.remove(&dn)
        {
            self.open.insert(dn);
        }
        if let Some(dn) = pick
            && self.selected.as_deref() != Some(dn.as_str())
        {
            self.select(dn);
        }
    }

    /// The children of `dn`, each a middle-line row (caret, picture,
    /// name), then their open children.
    fn level(
        &self,
        ui: &mut egui::Ui,
        cx: &Cx<'_>,
        dn: &str,
        depth: usize,
        pick: &mut Option<String>,
        toggle: &mut Option<String>,
    ) {
        let theme = cx.theme;
        let indent = depth as f32 * 16.0;
        match self.children.get(dn) {
            Some(Load::Ready(kids)) => {
                if kids.rows.is_empty() {
                    form::centred_row(ui, |ui| {
                        ui.add_space(indent + 22.0);
                        ui.label(
                            egui::RichText::new(cx.t("idm-directory-empty"))
                                .color(theme.text_muted),
                        );
                    });
                }
                for e in &kids.rows {
                    let open = self.open.contains(&e.dn);
                    form::centred_row(ui, |ui| {
                        ui.add_space(indent);
                        if e.has_children {
                            let caret = if open {
                                ph::CARET_DOWN
                            } else {
                                ph::CARET_RIGHT
                            };
                            if ui.small_button(caret).clicked() {
                                *toggle = Some(e.dn.clone());
                            }
                        } else {
                            // A caret's room, so a leaf's picture lines up
                            // with its siblings'.
                            ui.add_visible(false, egui::Button::new(ph::CARET_RIGHT).small());
                        }
                        let (g, c) = mark(e.kind());
                        ui.label(egui::RichText::new(g).color(c));
                        let selected = self.selected.as_deref() == Some(e.dn.as_str());
                        if form::selectable_label(ui, selected, e.rdn.as_str()).clicked() {
                            *pick = Some(e.dn.clone());
                        }
                    });
                    if open {
                        self.level(ui, cx, &e.dn, depth + 1, pick, toggle);
                    }
                }
                if kids.more {
                    form::centred_row(ui, |ui| {
                        ui.add_space(indent + 22.0);
                        ui.label(
                            egui::RichText::new(cx.t("idm-directory-more")).color(theme.text_muted),
                        );
                    });
                }
            }
            Some(Load::Failed(e)) => {
                form::centred_row(ui, |ui| {
                    ui.add_space(indent + 22.0);
                    ui.colored_label(theme.error, hs_core::fmt::sentence(&e.to_string()));
                });
            }
            _ => {
                form::centred_row(ui, |ui| {
                    ui.add_space(indent + 22.0);
                    ui.spinner();
                });
            }
        }
    }

    pub fn item(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        if self.selected.is_none() {
            super::common::nothing_selected(ui, cx.theme, &cx.t("idm-select-entry"));
            return;
        }
        let theme = cx.theme;
        if let Load::Failed(crate::provider::IdmError::NotFound { .. }) = self.item {
            cx.out
                .toasts
                .push((hs_core::tool::ToastLevel::Warning, cx.t("idm-entry-gone")));
            self.forget_selected();
            self.refresh();
            return;
        }
        if !state_note(ui, theme, cx.s, &self.item) {
            return;
        }
        let Some(e) = self.item.ready().cloned() else {
            return;
        };
        let may = cx.may_write("manage-users");
        let root = cx.realm == e.dn;
        let target = ObjRef::new(&e.dn, &e.rdn);
        let column = ui.available_width();
        // Both ways, as the user page: content wider than the column
        // scrolls inside it instead of widening the column past its edge.
        egui::ScrollArea::both()
            .id_salt("idm-directory-item")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                form::pane(ui, |ui| {
                    let (g, c) = mark(e.kind());
                    form::centred_row(ui, |ui| {
                        ui.label(egui::RichText::new(g).size(20.0).color(c));
                        ui.label(egui::RichText::new(&e.rdn).strong().size(16.0));
                    });
                    ui.add_space(form::PANE_GAP);
                    let children = if e.has_children {
                        cx.t("idm-yes")
                    } else {
                        cx.t("idm-no")
                    };
                    let classes = e.classes.join(", ");
                    let (dn_l, cl_l, ch_l) = (
                        cx.t("idm-dn"),
                        cx.t("idm-object-classes"),
                        cx.t("idm-has-children"),
                    );
                    form::detail_table(
                        ui,
                        theme,
                        "idm-dir-facts",
                        [
                            (dn_l.as_str(), e.dn.as_str()),
                            (cl_l.as_str(), classes.as_str()),
                            (ch_l.as_str(), children.as_str()),
                        ],
                    );
                    ui.add_space(form::PANE_GAP);
                    ui.separator();
                    ui.add_space(form::PANE_GAP);
                    form::subsection(ui, theme, &cx.t("idm-tab-attributes"));
                    ui.add_space(form::ROW_GAP);
                    if let Some(rows) = self.rows.as_mut() {
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
                    }
                    // The editor's rows may be wider than the column; the
                    // hint wraps at the column, not at them.
                    ui.scope(|ui| {
                        ui.set_max_width(column - 2.0 * form::PANE_MARGIN);
                        form::row_hint(ui, theme, &cx.t("idm-directory-attributes-hint"));
                    });
                    ui.add_space(form::ROW_GAP);
                    self.save_row(ui, cx, &e, &may);
                    ui.add_space(form::PANE_GAP);
                    ui.separator();
                    ui.add_space(form::PANE_GAP);
                    self.move_and_delete(ui, cx, &e, &target, &may, root);
                });
            });
    }

    /// Save and Revert: Save reads the entry again and reviews each changed
    /// attribute against it, saying so when the server's copy changed.
    fn save_row(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        e: &DirEntry,
        may: &Result<(), String>,
    ) {
        let theme = cx.theme;
        let after = self.rows.as_deref().map(map_of).unwrap_or_default();
        let dirty = after != e.attributes;
        if let Some(Load::Ready(fresh)) = &self.saving {
            let names: BTreeSet<&String> = fresh.attributes.keys().chain(after.keys()).collect();
            let rows: Vec<(String, String, String)> = names
                .into_iter()
                .filter(|n| fresh.attributes.get(*n) != after.get(*n))
                .map(|n| {
                    (
                        n.clone(),
                        joined(fresh.attributes.get(n)),
                        joined(after.get(n)),
                    )
                })
                .collect();
            let stale = fresh.attributes != e.attributes;
            let (realm, target) = (cx.realm.to_string(), ObjRef::new(&e.dn, &e.rdn));
            let name = e.rdn.clone();
            let attrs: BTreeMap<String, Vec<String>> = after;
            cx.out.dialog = Some(Dialog::Review {
                title: cx.ta("idm-review-title", &[("name", &name)]),
                rows,
                stale,
                then: WriteReq::new(
                    cx.ta("idm-done-saved", &[("name", &name)]),
                    After::Reload(Section::Directory),
                    move |p| async move {
                        p.dir_modify(&realm, &target, &attrs)
                            .await
                            .map(|()| WriteOk::Done)
                    },
                ),
            });
            self.saving = None;
            return;
        }
        if let Some(Load::Failed(err)) = &self.saving {
            form::note(
                ui,
                theme,
                ph::WARNING,
                &hs_core::fmt::sentence(&err.to_string()),
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
                let (realm, dn) = (cx.realm.to_string(), e.dn.clone());
                let mut slot = Load::Idle;
                ask(cx.session, "dir.fresh", &mut slot, move |p| async move {
                    p.dir_entry(&realm, &dn).await
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
                self.rows = Some(rows_of(&e.attributes));
            }
            if busy {
                ui.spinner();
            }
        });
    }

    /// Move To (a container's DN, the open containers offered) and Delete;
    /// the naming context itself is neither moved nor deleted.
    fn move_and_delete(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        e: &DirEntry,
        target: &ObjRef,
        may: &Result<(), String>,
        root: bool,
    ) {
        let theme = cx.theme;
        let not_root = cx.t("idm-directory-root-fixed");
        let parent = crate::provider::ldap::map::parent(&e.dn);
        // The containers the tree has read, as the places to move to.
        let places: Vec<String> = std::iter::once(cx.realm.to_string())
            .chain(
                self.children
                    .values()
                    .filter_map(Load::ready)
                    .flat_map(|p| p.rows.iter())
                    .filter(|c| c.kind() == DirKind::Container && c.dn != e.dn)
                    .map(|c| c.dn.clone()),
            )
            .filter(|d| d != &parent && !crate::provider::ldap::map::is_under(d, &e.dn))
            .collect();
        form::field_grid(ui, "idm-dir-move", |ui| {
            form::label(ui, &cx.t("idm-move-to"));
            form::pick_field(ui, theme, &mut self.move_to, &places, &parent);
            ui.end_row();
        });
        ui.add_space(form::ROW_GAP);
        ui.horizontal_wrapped(|ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let to = self.move_to.trim().to_string();
            if write_button(
                ui,
                theme,
                may,
                !root && !to.is_empty() && to != parent,
                ph::ARROW_BEND_UP_RIGHT,
                &cx.t("idm-move"),
                if root { &not_root } else { "" },
            ) {
                let (r, t, dest) = (cx.realm.to_string(), target.clone(), to.clone());
                let then = WriteReq::new(
                    cx.ta("idm-done-moved", &[("name", &e.rdn), ("to", &to)]),
                    // The moved entry is selected at its new DN.
                    After::Select(Section::Directory),
                    move |p| async move { p.dir_move(&r, &t, &dest).await.map(WriteOk::Created) },
                );
                let body = cx.ta("idm-move-confirm", &[("name", &e.dn), ("to", &to)]);
                // Moving a subtree is typed back, as deleting is (§4.12).
                cx.out.dialog = Some(if e.has_children {
                    Dialog::Typed {
                        title: cx.t("idm-move"),
                        body: format!("{body} {}", cx.t("idm-move-subtree")),
                        name: e.rdn.clone(),
                        ok: format!("{} {}", ph::ARROW_BEND_UP_RIGHT, cx.t("idm-move")),
                        typed: String::new(),
                        then,
                    }
                } else {
                    Dialog::Confirm {
                        title: cx.t("idm-move"),
                        body,
                        ok: format!("{} {}", ph::ARROW_BEND_UP_RIGHT, cx.t("idm-move")),
                        danger: false,
                        then,
                    }
                });
            }
            let delete_why = if root {
                not_root.clone()
            } else {
                cx.t("idm-directory-delete-leaf")
            };
            if write_button(
                ui,
                theme,
                may,
                !root && !e.has_children,
                ph::TRASH,
                &cx.t("idm-delete"),
                &delete_why,
            ) {
                let (r, t) = (cx.realm.to_string(), target.clone());
                cx.out.dialog = Some(Dialog::Typed {
                    title: cx.t("idm-delete-entry"),
                    body: cx.ta("idm-delete-entry-confirm", &[("name", &e.dn)]),
                    name: e.rdn.clone(),
                    ok: format!("{} {}", ph::TRASH, cx.t("idm-delete")),
                    typed: String::new(),
                    then: WriteReq::new(
                        cx.ta("idm-done-deleted", &[("name", &e.rdn)]),
                        After::Gone(Section::Directory),
                        move |p| async move { p.dir_delete(&r, &t).await.map(|()| WriteOk::Done) },
                    ),
                });
            }
        });
    }
}
