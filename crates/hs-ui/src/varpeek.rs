//! Two small companions for fields that may hold `{{variable}}` references:
//! a **peek** button that lists every reference in the text with its
//! resolved value (secrets masked until revealed) and copies it, and a
//! **Save to Environment** button that moves a typed secret into the vault
//! as a variable of the active environment and leaves `{{name}}` behind.

use hs_core::Services;
use hs_core::services::VarPeek;
use secrecy::SecretString;

use crate::icons::ph;
use crate::theme::Theme;
use crate::tip::TipExt;

/// `{{ name }}` references in `text`, in order, without duplicates.
pub fn references(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else { break };
        let name = after[..end].trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
        rest = &after[end + 2..];
    }
    out
}

#[derive(Clone, Default)]
struct PeekState {
    open: bool,
    revealed: bool,
}

/// An eye button, shown only when `text` references a variable; it opens
/// a popup with each reference's value and a Copy button. Secrets stay
/// masked until Reveal is clicked.
pub fn var_peek(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    id_salt: impl std::hash::Hash,
    text: &str,
) {
    let refs = references(text);
    if refs.is_empty() {
        return;
    }
    let id = egui::Id::new(("hs-var-peek", ui.id(), id_salt));
    let mut st = ui.data_mut(|d| d.get_temp::<PeekState>(id).unwrap_or_default());
    let button = ui
        .add(egui::Button::new(ph::EYE).frame(false))
        .tip(s.t("var-peek-tip"));
    if button.clicked() {
        st.open = !st.open;
    }
    if st.open {
        let pos = egui::pos2(button.rect.left(), button.rect.bottom() + 2.0);
        let area = egui::Area::new(id.with("popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.surface)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .show(ui, |ui| {
                        ui.set_min_width(320.0);
                        ui.set_max_width(520.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(s.t("var-peek-title"))
                                    .strong()
                                    .color(theme.text_muted),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button(ph::X).clicked() {
                                    st.open = false;
                                }
                                let label = if st.revealed {
                                    s.t("var-peek-hide")
                                } else {
                                    s.t("var-peek-reveal")
                                };
                                if ui.small_button(label).clicked() {
                                    st.revealed = !st.revealed;
                                }
                            });
                        });
                        egui::Grid::new(id.with("grid"))
                            .num_columns(3)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for name in &refs {
                                    ui.label(egui::RichText::new(format!("{{{{{name}}}}}")).monospace());
                                    let (shown, copy): (String, Option<String>) = match s.peek_var(name) {
                                        VarPeek::Unset => (s.t("var-peek-unset"), None),
                                        VarPeek::Plain(v) => (v.clone(), Some(v)),
                                        VarPeek::Locked => (s.t("var-peek-locked"), None),
                                        VarPeek::Secret(v) => {
                                            if st.revealed {
                                                (v.clone(), Some(v))
                                            } else {
                                                ("••••••••".into(), Some(v))
                                            }
                                        }
                                    };
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(shown).monospace())
                                            .truncate(),
                                    );
                                    match copy {
                                        Some(v) => {
                                            if ui.small_button(ph::COPY).tip(s.t("var-peek-copy")).clicked() {
                                                ui.ctx().copy_text(v);
                                            }
                                        }
                                        None => {
                                            ui.label("");
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            });
        if area.response.clicked_elsewhere() && !button.clicked() {
            st.open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, st));
}

#[derive(Clone, Default)]
struct SaveState {
    open: bool,
    name: String,
    error: Option<String>,
}

/// "Save to Environment…": moves the field's value into the vault as a
/// secret variable of the active environment and replaces the text with
/// `{{name}}`. Returns a message to show when something was saved.
pub fn save_to_env_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    id_salt: impl std::hash::Hash,
    value: &mut String,
) -> Option<String> {
    let id = egui::Id::new(("hs-save-env", ui.id(), id_salt));
    let mut st = ui.data_mut(|d| d.get_temp::<SaveState>(id).unwrap_or_default());
    let has_value = !value.trim().is_empty() && references(value).is_empty();
    let button = ui
        .add_enabled(has_value, egui::Button::new(format!("{} {}", ph::LOCK_KEY, s.t("var-save-env"))).small())
        .tip(s.t("var-save-env-tip"));
    if button.clicked() {
        st.open = !st.open;
        st.error = None;
    }
    let mut saved = None;
    if st.open {
        let pos = egui::pos2(button.rect.left(), button.rect.bottom() + 2.0);
        let area = egui::Area::new(id.with("popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.surface)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .show(ui, |ui| {
                        ui.set_min_width(300.0);
                        let env = s.environments.read().active_env().map(|e| e.name.clone());
                        match &env {
                            Some(e) => {
                                ui.label(s.i18n.t_args("var-save-env-into", &[("env", e)]));
                            }
                            None => {
                                ui.colored_label(theme.warning, s.t("var-save-env-no-env"));
                            }
                        }
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut st.name)
                                .hint_text(s.t("var-save-env-name"))
                                .desired_width(f32::INFINITY),
                        );
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            st.error = Some("__go".into());
                        }
                        if let Some(e) = st.error.as_deref().filter(|e| *e != "__go") {
                            ui.colored_label(theme.error, e);
                        }
                        ui.horizontal(|ui| {
                            if ui.button(s.t("save")).clicked() {
                                st.error = Some("__go".into());
                            }
                            if ui.button(s.t("cancel")).clicked() {
                                st.open = false;
                            }
                        });
                    });
            });
        if st.error.as_deref() == Some("__go") {
            st.error = None;
            let name = st.name.trim().to_string();
            let valid = !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.');
            if !valid {
                st.error = Some(s.t("var-save-env-bad-name"));
            } else if s.secrets.is_locked() {
                st.error = Some(s.t("settings-vault-unlock-first"));
            } else if s.environments.read().active.is_none() {
                st.error = Some(s.t("var-save-env-no-env"));
            } else {
                let label = format!("{}/{}", s.environments.read().active_env().map(|e| e.name.clone()).unwrap_or_default(), name);
                match s.secrets.create(&label, &SecretString::from(value.clone())) {
                    Ok(r) => match s.environments.write().set_var(None, &name, &r.to_string_ref()) {
                        Ok(()) => {
                            *value = format!("{{{{{name}}}}}");
                            s.events.publish(hs_core::events::Event::EnvironmentsChanged);
                            saved = Some(s.i18n.t_args("var-save-env-done", &[("name", &name)]));
                            st.open = false;
                            st.name.clear();
                        }
                        Err(e) => st.error = Some(e.to_string()),
                    },
                    Err(e) => st.error = Some(e.to_string()),
                }
            }
        }
        if area.response.clicked_elsewhere() && !button.clicked() {
            st.open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    saved
}

#[cfg(test)]
mod tests {
    use super::references;

    #[test]
    fn finds_references_once_each() {
        assert_eq!(
            references("{{a}} and {{ b }} then {{a}} {{"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(references("plain").is_empty());
    }
}
