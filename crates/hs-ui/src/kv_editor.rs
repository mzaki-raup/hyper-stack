//! Key/value rows with enable checkboxes (headers, params, variables).

use crate::theme::Theme;
use crate::tip::TipExt;

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct KvRow {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct KvResponse {
    pub changed: bool,
}

pub struct KvEditor<'a> {
    pub theme: &'a Theme,
    pub key_hint: &'a str,
    pub value_hint: &'a str,
    pub add_label: &'a str,
    pub allow_secret: bool,
}

impl KvEditor<'_> {
    pub fn show(&self, ui: &mut egui::Ui, rows: &mut Vec<KvRow>) -> KvResponse {
        let mut resp = KvResponse::default();
        let mut remove = None;
        // Key gets a third of the width, value the rest (both comfortably tall).
        let total = ui.available_width().clamp(420.0, 900.0);
        let key_w = (total * 0.32).clamp(140.0, 260.0);
        let value_w = (total - key_w - 110.0).max(200.0);
        egui::Grid::new(ui.id().with("kv"))
            .num_columns(5)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                for (i, row) in rows.iter_mut().enumerate() {
                    if ui.checkbox(&mut row.enabled, "").changed() {
                        resp.changed = true;
                    }
                    // Fixed sizes: inside a grid the leftover width is tiny on
                    // the first frame and the fields would collapse.
                    let key = show_sized(
                        ui,
                        [key_w, 32.0],
                        crate::form::line_edit(&mut row.key).hint_text(self.key_hint),
                    );
                    crate::varpeek::text_edit_tip(&key, &row.key);
                    if key.response.response.changed() {
                        resp.changed = true;
                    }
                    let edit = crate::form::line_edit(&mut row.value)
                        .hint_text(self.value_hint)
                        .password(row.secret);
                    let value = show_sized(ui, [value_w, 32.0], edit);
                    crate::varpeek::text_edit_tip(&value, &row.value);
                    if value.response.response.changed() {
                        resp.changed = true;
                    }
                    if self.allow_secret {
                        // Locked = warning colour with a filled lock; plain = muted open lock.
                        let (icon, tip) = if row.secret {
                            (crate::icons::LOCK, "Secret (Stored in the Vault)")
                        } else {
                            (crate::icons::UNLOCK, "Mark as Secret")
                        };
                        let r = crate::form::icon_button(ui, self.theme, icon, 16.0, row.secret);
                        if row.secret {
                            ui.painter().text(
                                r.rect.center(),
                                egui::Align2::CENTER_CENTER,
                                icon,
                                egui::FontId::proportional(16.0),
                                self.theme.warning,
                            );
                        }
                        if r.tip(tip).clicked() {
                            row.secret = !row.secret;
                            resp.changed = true;
                        }
                    } else {
                        ui.label("");
                    }
                    if ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(crate::icons::TRASH)
                                    .color(self.theme.text_muted),
                            )
                            .frame(false),
                        )
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            rows.remove(i);
            resp.changed = true;
        }
        if ui
            .button(format!("{} {}", crate::icons::TAB_NEW, self.add_label))
            .clicked()
        {
            rows.push(KvRow {
                enabled: true,
                ..Default::default()
            });
            resp.changed = true;
        }
        resp
    }
}

/// `ui.add_sized` for a text field, keeping its output (the galley the
/// `{{name}}` hover tooltip reads).
fn show_sized(
    ui: &mut egui::Ui,
    size: [f32; 2],
    edit: egui::TextEdit<'_>,
) -> egui::text_edit::TextEditOutput {
    let layout = egui::Layout::centered_and_justified(ui.layout().main_dir());
    ui.allocate_ui_with_layout(size.into(), layout, |ui| edit.show(ui))
        .inner
}
