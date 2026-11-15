//! The sidebar (§3.2): Add Identity Server, then the saved servers, each
//! with its product's mark, a grip to reorder, and the shared row menu.

use hs_core::tool::{Action, OpenRequest, ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

use super::IdmTool;
use crate::profile::IdmFields;

pub fn show(tool: &mut IdmTool, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
    let s = cx.services;
    let theme = Theme::from_settings(&s.settings.read().appearance);
    if form::sidebar_head(
        ui,
        &theme,
        &s.t("idm-servers"),
        &[form::SideAction {
            icon: ph::PLUS,
            label: &s.t("idm-add"),
            tip: "",
        }],
    )
    .is_some()
    {
        cx.actions.push(Action::RunCommand {
            id: "settings:identity:new".into(),
        });
    }
    let profiles: Vec<(String, String, String, bool, hs_ui::icons::ConnIcon)> = {
        let conns = s.connections.read();
        conns
            .of_idm_kinds()
            .map(|c| {
                (
                    c.id.clone(),
                    c.name.clone(),
                    IdmFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                    c.pinned,
                    hs_ui::icons::conn_icon(&c.kind, ""),
                )
            })
            .collect()
    };
    if profiles.is_empty() {
        form::note(ui, &theme, ph::LIGHTBULB, &s.t("idm-none"));
        return true;
    }
    egui::ScrollArea::vertical()
        .id_salt("idm-servers")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let mut reorder = hs_ui::reorder::Reorder::new("idm-servers");
            let mut open: Option<(String, bool)> = None;
            let mut pin: Option<(String, bool)> = None;
            let mut edit: Option<String> = None;
            let mut remove: Option<String> = None;
            for (i, (id, name, label, pinned, icon)) in profiles.iter().enumerate() {
                let selected = tool.state.conn_id.as_deref() == Some(id.as_str());
                reorder.row(ui, i, |ui| {
                    let row = form::connection_row(
                        ui,
                        &theme,
                        selected,
                        *icon,
                        name,
                        Some(label),
                        *pinned,
                        form::RowMenuLabels {
                            pin: &s.t("conn-pin-home"),
                            unpin: &s.t("conn-unpin-home"),
                            edit: &s.t("conn-edit"),
                            remove: &s.t("conn-remove"),
                        },
                    );
                    if row.response.tip(s.t("idm-open")).clicked() {
                        open = Some((id.clone(), selected));
                    }
                    if let Some(p) = row.pin {
                        pin = Some((id.clone(), p));
                    }
                    if row.edit {
                        edit = Some(id.clone());
                    }
                    if row.remove {
                        remove = Some(id.clone());
                    }
                });
                ui.add_space(2.0);
            }
            if let Some((id, p)) = pin
                && let Err(e) = s.connections.write().set_pinned(&id, p)
            {
                cx.toast(ToastLevel::Error, e.to_string());
            }
            if let Some(id) = edit {
                cx.actions.push(Action::RunCommand {
                    id: format!("settings:connection:{id}"),
                });
            }
            // Remove From List: the shell asks first (FEAT-0094).
            if let Some(id) = remove {
                cx.actions.push(Action::RunCommand {
                    id: format!("connection:remove:{id}"),
                });
            }
            if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                if let Err(e) =
                    s.connections
                        .write()
                        .reorder(hs_core::connections::IDM_KINDS, from, to)
                {
                    cx.toast(ToastLevel::Error, e.to_string());
                }
                ui.ctx().request_repaint();
            }
            // A server has at most one tab: the shell focuses it if open.
            if let Some((id, selected)) = open
                && !selected
            {
                cx.open("idm", Some(OpenRequest::Connection { id }));
            }
        });
    true
}
