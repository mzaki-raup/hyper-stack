//! Small widgets the utility pages share.

use hs_core::Services;
use hs_ui::theme::Theme;

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
        });
    });
    let mut shown = text.to_string();
    ui.add(
        egui::TextEdit::multiline(&mut shown)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    );
}

/// A multi-line input with a hint.
pub fn input(ui: &mut egui::Ui, value: &mut String, hint: &str, rows: usize) -> egui::Response {
    ui.add(
        egui::TextEdit::multiline(value)
            .hint_text(hint)
            .font(egui::TextStyle::Monospace)
            .desired_width(f32::INFINITY)
            .desired_rows(rows),
    )
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

/// Pretty JSON when the text parses, else the text unchanged.
pub fn pretty_json(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| text.to_string())
}
