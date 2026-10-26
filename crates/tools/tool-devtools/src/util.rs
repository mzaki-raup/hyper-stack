//! Small widgets the utility pages share.

use hs_core::Services;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

/// A labelled read-only monospace box with a Copy button.
pub fn output(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    label: &str,
    text: &str,
    rows: usize,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).strong().color(theme.text_muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(format!("{} {}", hs_ui::icons::COPY, s.t("dt-copy")))
                .clicked()
            {
                ui.ctx().copy_text(text.to_string());
            }
            if ui
                .small_button(format!(
                    "{} {}",
                    hs_ui::icons::ph::DOWNLOAD_SIMPLE,
                    s.t("download")
                ))
                .tip(s.t("dt-download-tip"))
                .clicked()
            {
                let ext = match label.to_ascii_lowercase().as_str() {
                    "json" => "json",
                    "xml" => "xml",
                    _ => "txt",
                };
                let name = format!("{}.{ext}", hs_core::download::safe_name(label));
                let msg = match hs_core::download::save_bytes(&name, text.as_bytes()) {
                    Ok(Some(p)) => Some(
                        s.i18n
                            .t_args("download-saved", &[("path", &p.display().to_string())]),
                    ),
                    Ok(None) => None,
                    Err(e) => Some(e),
                };
                if let Some(m) = msg {
                    let id = ui.id().with("dt-download-flash");
                    let now = ui.input(|i| i.time);
                    ui.data_mut(|d| d.insert_temp(id, (m, now)));
                }
            }
        });
    });
    {
        let id = ui.id().with("dt-download-flash");
        let flash = ui.data(|d| d.get_temp::<(String, f64)>(id));
        if let Some((m, at)) = flash {
            if ui.input(|i| i.time) - at < 6.0 {
                ui.label(egui::RichText::new(m).small().color(theme.text_muted));
            } else {
                ui.data_mut(|d| d.remove::<(String, f64)>(id));
            }
        }
    }
    let mut shown = text.to_string();
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    );
}

/// A multi-line input with a hint; `{{variables}}` in it get a peek button.
pub fn input(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    value: &mut String,
    hint: &str,
    rows: usize,
) -> egui::Response {
    let r = ui.add(
        egui::TextEdit::multiline(value)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    );
    if hs_ui::varpeek::references(value).is_empty() {
        return r;
    }
    ui.horizontal(|ui| {
        hs_ui::varpeek::var_peek(ui, theme, s, r.id, value);
        ui.label(
            egui::RichText::new(s.t("dt-var-hint"))
                .small()
                .color(theme.text_muted),
        );
    });
    r
}

/// A multi-line input for a key or other secret: peek for `{{variables}}`
/// and **Save to Environment…** to move a typed value into the vault.
pub fn secret_input(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    value: &mut String,
    hint: &str,
    rows: usize,
) -> egui::Response {
    let r = ui.add(
        egui::TextEdit::multiline(value)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    );
    secret_tools(ui, s, theme, r.id, value);
    r
}

/// A single-line text field with the peek button.
pub fn line(ui: &mut egui::Ui, s: &Services, theme: &Theme, value: &mut String, hint: &str) {
    ui.horizontal(|ui| {
        let r = hs_ui::form::text_field_width(ui, value, hint, 320.0);
        hs_ui::varpeek::var_peek(ui, theme, s, r.id, value);
    });
}

/// A single-line password field with peek and Save to Environment….
pub fn secret_line(ui: &mut egui::Ui, s: &Services, theme: &Theme, value: &mut String, hint: &str) {
    ui.vertical(|ui| {
        let password = hs_ui::varpeek::references(value).is_empty();
        let r = ui.add(
            egui::TextEdit::singleline(value)
                .hint_text(hint)
                .password(password)
                .margin(hs_ui::form::FIELD_MARGIN)
                .desired_width(320.0),
        );
        secret_tools(ui, s, theme, r.id, value);
    });
}

fn secret_tools(ui: &mut egui::Ui, s: &Services, theme: &Theme, id: egui::Id, value: &mut String) {
    ui.horizontal(|ui| {
        hs_ui::varpeek::var_peek(ui, theme, s, id, value);
        if let Some(msg) = hs_ui::varpeek::save_to_env_button(ui, theme, s, id, value) {
            // Read the clock first: `input` and `data_mut` both lock the
            // context, and nesting them deadlocks.
            let now = ui.input(|i| i.time);
            ui.data_mut(|d| d.insert_temp(id.with("flash"), (msg, now)));
        }
        let flash = ui.data(|d| d.get_temp::<(String, f64)>(id.with("flash")));
        if let Some((msg, at)) = flash {
            if ui.input(|i| i.time) - at < 6.0 {
                ui.colored_label(theme.success, msg);
            } else {
                ui.data_mut(|d| d.remove::<(String, f64)>(id.with("flash")));
            }
        }
    });
}

/// `{{variables}}` and `secret:` references resolved, or the reason not.
pub fn expand(s: &Services, text: &str) -> Result<String, String> {
    s.expand(text)
}

pub fn error(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.colored_label(theme.error, format!("{} {text}", hs_ui::icons::ERROR));
}

pub fn ok(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.colored_label(theme.success, format!("{} {text}", hs_ui::icons::CHECK));
}

/// Either an error or an output box.
pub fn result(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    label: &str,
    r: &Option<Result<String, String>>,
    rows: usize,
) {
    match r {
        Some(Ok(text)) => output(ui, s, theme, label, text, rows),
        Some(Err(e)) => error(ui, theme, e),
        None => {}
    }
}

/// Pretty JSON when the text parses, else the text unchanged. The keys
/// keep the order the server sent them in (`hs_core::fmt` re-indents the
/// text instead of round-tripping it through a sorted map).
pub fn pretty_json(text: &str) -> String {
    hs_core::fmt::pretty_json(text).unwrap_or_else(|| text.to_string())
}
