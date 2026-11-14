//! Modal dialogs: confirm, and the master-password prompts for the vault.

use egui::CornerRadius;

use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogResult {
    Open,
    Ok,
    Cancel,
}

/// What the user chose in a [`three_way`] modal: the action that keeps the
/// work, the one that goes ahead without it, or neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeWay {
    Open,
    /// The primary action: save, keep, apply.
    Primary,
    /// Carry on without it: ignore, discard.
    Secondary,
    Cancel,
}

/// What [`confirm`] draws on its buttons. The dialog owns the glyphs: a
/// glyph the caller put before the OK label is the verb's own (a trash can
/// for Delete) and is kept, otherwise a warning for a danger and a check
/// for the rest; Cancel always wears an X. Any glyph a caller put before
/// Cancel is dropped, so no button ever shows two.
#[derive(Debug, PartialEq, Eq)]
struct ConfirmLabels<'a> {
    ok_glyph: &'a str,
    ok: &'a str,
    cancel: &'a str,
}

impl<'a> ConfirmLabels<'a> {
    fn new(ok_label: &'a str, cancel_label: &'a str, danger: bool) -> Self {
        let (given, ok) = crate::form::split_glyph(ok_label);
        let ok_glyph = given.unwrap_or(if danger {
            crate::icons::ph::WARNING
        } else {
            crate::icons::ph::CHECK
        });
        Self {
            ok_glyph,
            ok,
            cancel: crate::form::plain_label(cancel_label),
        }
    }
}

/// A yes/no modal. Returns `Ok`/`Cancel` on the frame the user decides.
/// Pass plain labels; the dialog adds the glyphs ([`ConfirmLabels`]).
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
            // Both buttons an icon and a label at the standard size; the
            // accessible name stays the label alone (CR-0063).
            let labels = ConfirmLabels::new(ok_label, cancel_label, danger);
            let ok = egui::Button::new(
                egui::RichText::new(format!("{} {}", labels.ok_glyph, labels.ok))
                    .color(egui::Color32::WHITE),
            )
            .fill(if danger { theme.error } else { theme.accent })
            .corner_radius(CornerRadius::same(4));
            if crate::form::sized_button(ui, ok, labels.ok).clicked() {
                result = DialogResult::Ok;
            }
            ui.add_space(crate::form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if crate::form::button(ui, theme, crate::icons::ph::X, labels.cancel).clicked() {
                result = DialogResult::Cancel;
            }
        });
    });
    if modal.should_close() && result == DialogResult::Open {
        result = DialogResult::Cancel;
    }
    result
}

/// A modal with three answers, for "you have unsaved work" moments: Save,
/// Ignore, Cancel. Escape or the close button means Cancel, never one of
/// the two actions, because guessing wrong there loses work or runs stale
/// code.
// Labelled arguments read better than a struct for a one-call helper.
#[allow(clippy::too_many_arguments)]
pub fn three_way(
    ctx: &egui::Context,
    theme: &Theme,
    id: &str,
    title: &str,
    body: &str,
    primary_label: &str,
    secondary_label: &str,
    cancel_label: &str,
) -> ThreeWay {
    let mut result = ThreeWay::Open;
    let modal = egui::Modal::new(egui::Id::new(("hs-three-way", id))).show(ctx, |ui| {
        ui.set_width(460.0);
        ui.heading(title);
        ui.add_space(6.0);
        ui.label(body);
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let primary =
                egui::Button::new(egui::RichText::new(primary_label).color(egui::Color32::WHITE))
                    .fill(theme.accent)
                    .corner_radius(CornerRadius::same(4));
            if ui.add(primary).clicked() {
                result = ThreeWay::Primary;
            }
            if ui.button(secondary_label).clicked() {
                result = ThreeWay::Secondary;
            }
            if ui.button(cancel_label).clicked() {
                result = ThreeWay::Cancel;
            }
        });
    });
    if modal.should_close() && result == ThreeWay::Open {
        result = ThreeWay::Cancel;
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
            crate::form::line_edit(&mut state.password)
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
                crate::form::line_edit(&mut state.confirm)
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

/// What a [`typed_confirm`] modal says: the title, the sentence of what
/// goes, the name to type back, and the two buttons.
pub struct TypedConfirm<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// What must be typed, exactly, before OK is enabled.
    pub name: &'a str,
    /// The danger verb, with its glyph ("🗑 Delete").
    pub ok_label: &'a str,
    pub cancel_label: &'a str,
}

/// Whether the text typed into a [`typed_confirm`] allows its OK.
pub fn typed_matches(typed: &str, name: &str) -> bool {
    !name.is_empty() && typed == name
}

/// A typed confirmation for a delete-style verb (ui.md: "a typed
/// confirmation for purge- and delete-style verbs"): the name of what goes
/// must be typed back before the danger button is enabled. `typed` is the
/// caller's, kept between frames. Escape or the close button is Cancel.
pub fn typed_confirm(
    ctx: &egui::Context,
    theme: &Theme,
    id: &str,
    text: &TypedConfirm<'_>,
    typed: &mut String,
) -> DialogResult {
    let mut result = DialogResult::Open;
    let modal = egui::Modal::new(egui::Id::new(("hs-typed-confirm", id))).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(text.title);
        ui.add_space(6.0);
        ui.label(text.body);
        ui.add_space(8.0);
        let r = ui.add(
            crate::form::line_edit(typed)
                .hint_text(text.name)
                .desired_width(f32::INFINITY),
        );
        r.request_focus();
        ui.add_space(12.0);
        let matches = typed_matches(typed, text.name);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let labels = ConfirmLabels::new(text.ok_label, text.cancel_label, true);
            let ok = egui::Button::new(
                egui::RichText::new(format!("{} {}", labels.ok_glyph, labels.ok))
                    .color(egui::Color32::WHITE),
            )
            .fill(theme.error)
            .corner_radius(CornerRadius::same(4));
            let ok = ui.add_enabled_ui(matches, |ui| crate::form::sized_button(ui, ok, labels.ok));
            if ok.inner.clicked()
                || (matches && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                result = DialogResult::Ok;
            }
            ui.add_space(crate::form::BUTTON_GAP - ui.spacing().item_spacing.x);
            if crate::form::button(ui, theme, crate::icons::ph::X, labels.cancel).clicked() {
                result = DialogResult::Cancel;
            }
        });
    });
    if modal.should_close() && result == DialogResult::Open {
        result = DialogResult::Cancel;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{ConfirmLabels, typed_matches};

    #[test]
    fn a_typed_confirmation_needs_the_exact_name() {
        assert!(typed_matches("alice", "alice"));
        assert!(!typed_matches("Alice", "alice"));
        assert!(!typed_matches("alic", "alice"));
        assert!(!typed_matches("alice ", "alice"));
        assert!(!typed_matches("", ""), "an empty name never allows OK");
    }
    use crate::icons::ph;

    #[test]
    fn confirm_labels_never_carry_two_glyphs() {
        let ok = format!("{} Delete", ph::TRASH);
        let cancel = format!("{} Cancel", ph::X);
        let l = ConfirmLabels::new(&ok, &cancel, true);
        assert_eq!(l.ok_glyph, ph::TRASH, "the verb's own glyph is kept");
        assert_eq!(l.ok, "Delete");
        assert_eq!(l.cancel, "Cancel");
    }

    #[test]
    fn plain_confirm_labels_get_the_dialog_glyph() {
        let l = ConfirmLabels::new("Remove", "Cancel", true);
        assert_eq!(l.ok_glyph, ph::WARNING);
        assert_eq!((l.ok, l.cancel), ("Remove", "Cancel"));
        let l = ConfirmLabels::new("Apply", "Cancel", false);
        assert_eq!(l.ok_glyph, ph::CHECK);
        assert_eq!(l.ok, "Apply");
    }
}
