//! Modal dialogs: confirm, and the master-password prompts for the vault.

use egui::CornerRadius;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResult {
    Open,
    Ok,
    Cancel,
}

/// A yes/no modal. Returns `Ok`/`Cancel` on the frame the user decides.
// Eight labelled arguments read better than a struct for a one-call helper.
#[allow(clippy::too_many_arguments)]
pub fn confirm(
    ctx: &egui::Context,
    theme: &Theme,
    id: &str,
    title: &str,
    body: &str,
    ok_label: &str,
    cancel_label: &str,
    danger: bool,
) -> DialogResult {
    let mut result = DialogResult::Open;
    let modal = egui::Modal::new(egui::Id::new(("hs-confirm", id))).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(title);
        ui.add_space(6.0);
        ui.label(body);
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = egui::Button::new(egui::RichText::new(ok_label).color(egui::Color32::WHITE))
                .fill(if danger { theme.error } else { theme.accent })
                .corner_radius(CornerRadius::same(4));
            if ui.add(ok).clicked() {
                result = DialogResult::Ok;
            }
            if ui.button(cancel_label).clicked() {
                result = DialogResult::Cancel;
            }
        });
    });
    if modal.should_close() && result == DialogResult::Open {
        result = DialogResult::Cancel;
    }
    result
}

#[derive(Default, Clone)]
pub struct PasswordDialogState {
    pub password: String,
    pub confirm: String,
    pub error: Option<String>,
    pub just_opened: bool,
}

impl PasswordDialogState {
    pub fn reset(&mut self) {
        self.password.clear();
        self.confirm.clear();
        self.error = None;
        self.just_opened = true;
    }
}

pub struct PasswordLabels<'a> {
    pub title: &'a str,
    pub body: &'a str,
    pub password: &'a str,
    pub confirm: Option<&'a str>,
    pub ok: &'a str,
    pub cancel: &'a str,
    pub mismatch: &'a str,
}

/// Master-password prompt. With `labels.confirm` set it asks twice (create).
/// Returns the password on OK; the caller clears `state` afterwards.
pub fn password(
    ctx: &egui::Context,
    theme: &Theme,
    state: &mut PasswordDialogState,
    labels: &PasswordLabels,
) -> Option<Result<String, ()>> {
    let mut out = None;
    let modal = egui::Modal::new(egui::Id::new(("hs-password", labels.title))).show(ctx, |ui| {
        ui.set_width(420.0);
        ui.heading(labels.title);
        ui.add_space(6.0);
        ui.label(labels.body);
        ui.add_space(10.0);
        ui.label(labels.password);
        let r = ui.add(
            egui::TextEdit::singleline(&mut state.password)
                .password(true)
                .desired_width(f32::INFINITY),
        );
        if state.just_opened {
            r.request_focus();
            state.just_opened = false;
        }
        let mut submit = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if let Some(c) = labels.confirm {
            ui.label(c);
            let r2 = ui.add(
                egui::TextEdit::singleline(&mut state.confirm)
                    .password(true)
                    .desired_width(f32::INFINITY),
            );
            submit = r2.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        }
        if let Some(e) = &state.error {
            ui.colored_label(theme.error, e);
        }
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let ok = egui::Button::new(egui::RichText::new(labels.ok).color(egui::Color32::WHITE))
                .fill(theme.accent)
                .corner_radius(CornerRadius::same(4));
            if ui.add(ok).clicked() {
                submit = true;
            }
            if ui.button(labels.cancel).clicked() {
                out = Some(Err(()));
            }
        });
        if submit {
            let mismatch = labels.confirm.is_some() && state.password != state.confirm;
            if mismatch || state.password.is_empty() {
                state.error = Some(labels.mismatch.to_string());
            } else {
                out = Some(Ok(state.password.clone()));
            }
        }
    });
    if modal.should_close() && out.is_none() {
        out = Some(Err(()));
    }
    out
}
