//! A group picker for Join Group (§4.3): a search over the realm's
//! groups, their paths listed, one clicked.

use hs_ui::form;
use hs_ui::icons::ph;

use super::common::{Cx, Load, ask, state_note, take};
use crate::provider::{Group, IdmProvider, ObjRef, Paged, Section};

#[derive(Default)]
pub struct GroupPick {
    search: String,
    asked: Option<String>,
    found: Load<Paged<Group>>,
}

impl GroupPick {
    /// `Some(Some(group))` once one is chosen, `Some(None)` on Cancel,
    /// `None` while it is open.
    pub fn show(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<Option<ObjRef>> {
        let theme = cx.theme;
        let mut out = None;
        take(cx.session, "pick.groups", &mut self.found);
        if self.asked.as_deref() != Some(self.search.trim()) {
            let term = self.search.trim().to_string();
            let (realm, page) = (cx.realm.to_string(), cx.page);
            let t = term.clone();
            ask(
                cx.session,
                "pick.groups",
                &mut self.found,
                move |p| async move { p.groups(&realm, &t, page).await },
            );
            self.asked = Some(term);
        }
        form::well(ui, theme, None, |ui| {
            form::centred_row(ui, |ui| {
                form::text_field_width(ui, &mut self.search, &cx.t("idm-search-groups"), 260.0);
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
            if found.rows.is_empty() {
                form::hint(ui, theme, &cx.t("idm-groups-none-found"));
                return;
            }
            let (glyph, colour) = super::common::section_mark(Section::Groups);
            egui::ScrollArea::vertical()
                .id_salt("idm-group-pick")
                .max_height(200.0)
                .show(ui, |ui| {
                    for g in found.rows.iter().flat_map(flatten) {
                        form::centred_row(ui, |ui| {
                            ui.label(egui::RichText::new(glyph).color(colour));
                            if form::selectable_label(ui, false, &g.path)
                                .on_hover_text(cx.t("idm-pick-this"))
                                .clicked()
                            {
                                out = Some(Some(ObjRef::new(&g.id, &g.path)));
                            }
                        });
                    }
                });
        });
        out
    }
}

/// A group and the sub-groups a search returned inside it, as one list.
fn flatten(g: &Group) -> Vec<&Group> {
    let mut out = vec![g];
    for c in &g.sub_groups {
        out.extend(flatten(c));
    }
    out
}
