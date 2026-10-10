//! Key/value rows with enable checkboxes (headers, params, variables).

use crate::theme::Theme;

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
        egui::Grid::new(ui.id().with("kv"))
            .num_columns(5)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                for (i, row) in rows.iter_mut().enumerate() {
                    if ui.checkbox(&mut row.enabled, "").changed() {
                        resp.changed = true;
                    }
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut row.key)
                                .hint_text(self.key_hint)
                                .desired_width(160.0),
                        )
                        .changed()
                    {
                        resp.changed = true;
                    }
                    let edit = egui::TextEdit::singleline(&mut row.value)
                        .hint_text(self.value_hint)
                        .desired_width(260.0)
                        .password(row.secret);
                    if ui.add(edit).changed() {
                        resp.changed = true;
                    }
                    if self.allow_secret {
                        let icon = if row.secret {
                            crate::icons::LOCK
                        } else {
                            crate::icons::UNLOCK
                        };
                        if ui
                            .add(egui::Button::new(icon).frame(false))
                            .on_hover_text("secret")
                            .clicked()
                        {
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
