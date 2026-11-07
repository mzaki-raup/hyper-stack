//! Two small companions for fields that may hold `{{variable}}` references:
//! a **peek** button that lists every reference in the text with its
//! resolved value (secrets masked until revealed) and copies it, and a
//! **Save to Environment** button that moves a typed secret into the vault
//! as a variable of the active environment and leaves `{{name}}` behind.
//! Plus a hover tooltip: the pointer over one `{{ name }}` shows that
//! reference's value (a secret always masked) - see [`hover_tip`].

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

/// A click this frame that landed outside `rect` (the popup). `Response::
/// clicked_elsewhere` on the Area closed the popup when one of its own
/// buttons was clicked, so the rect is tested directly.
fn clicked_outside(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        i.pointer.any_click()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !rect.expand(4.0).contains(p))
    })
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
    id_salt: impl std::hash::Hash + std::fmt::Debug,
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
                        ui.set_min_width(420.0);
                        ui.set_max_width(560.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(s.t("var-peek-title"))
                                    .strong()
                                    .color(theme.text_muted),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
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
                                },
                            );
                        });
                        egui::Grid::new(id.with("grid"))
                            .num_columns(3)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                for name in &refs {
                                    ui.label(
                                        egui::RichText::new(format!("{{{{{name}}}}}")).monospace(),
                                    );
                                    let (shown, copy): (String, Option<String>) =
                                        match s.peek_var(name) {
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
                                    // A truncating label inside a Grid shrinks to
                                    // whatever width the column had last frame, so
                                    // give it a fixed width instead.
                                    ui.scope(|ui| {
                                        ui.set_min_width(300.0);
                                        ui.set_max_width(300.0);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(shown).monospace(),
                                            )
                                            .truncate(),
                                        );
                                    });
                                    match copy {
                                        Some(v) => {
                                            if ui
                                                .small_button(ph::COPY)
                                                .tip(s.t("var-peek-copy"))
                                                .clicked()
                                            {
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
        if clicked_outside(ui, area.response.rect) && !button.clicked() {
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
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
) -> Option<String> {
    let id = egui::Id::new(("hs-save-env", ui.id(), id_salt));
    let mut st = ui.data_mut(|d| d.get_temp::<SaveState>(id).unwrap_or_default());
    let has_value = !value.trim().is_empty() && references(value).is_empty();
    let button = ui
        .add_enabled_ui(has_value, |ui| {
            crate::form::button(ui, theme, ph::LOCK_KEY, &s.t("var-save-env"))
        })
        .inner
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
                            crate::form::line_edit(&mut st.name)
                                .hint_text(s.t("var-save-env-name"))
                                .desired_width(f32::INFINITY),
                        );
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            st.error = Some("__go".into());
                        }
                        if let Some(e) = st.error.as_deref().filter(|e| *e != "__go") {
                            ui.colored_label(theme.error, e);
                        }
                        crate::form::centred_row_wrapped(ui, |ui| {
                            if crate::form::button(ui, theme, ph::FLOPPY_DISK, &s.t("save"))
                                .clicked()
                            {
                                st.error = Some("__go".into());
                            }
                            if crate::form::button(ui, theme, ph::X, &s.t("cancel")).clicked() {
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
                let label = format!(
                    "{}/{}",
                    s.environments
                        .read()
                        .active_env()
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                    name
                );
                match s.secrets.create(&label, &SecretString::from(value.clone())) {
                    Ok(r) => match s
                        .environments
                        .write()
                        .set_var(None, &name, &r.to_string_ref())
                    {
                        Ok(()) => {
                            *value = format!("{{{{{name}}}}}");
                            s.events
                                .publish(hs_core::events::Event::EnvironmentsChanged);
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
        if clicked_outside(ui, area.response.rect) && !button.clicked() {
            st.open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(id, st));
    saved
}

// ---- Hover tooltips over a `{{ name }}` reference ----

/// The masked stand-in for a secret's value in a tooltip.
const TIP_MASK: &str = "••••••••";
/// A plain value longer than this is cut, with an ellipsis, in a tooltip.
const TIP_MAX_CHARS: usize = 300;

/// The `{{ name }}` reference whose span (from its `{{` to its `}}`) holds
/// the byte at `byte`, as its trimmed name.
pub fn reference_at(text: &str, byte: usize) -> Option<&str> {
    let mut from = 0;
    while let Some(s) = text[from..].find("{{") {
        let start = from + s;
        let after = start + 2;
        let e = text[after..].find("}}")?;
        let end = after + e + 2;
        if byte < start {
            return None;
        }
        if byte < end {
            let name = text[after..after + e].trim();
            return (!name.is_empty()).then_some(name);
        }
        from = end;
    }
    None
}

/// What a tooltip says for a value it cannot show (already translated).
pub struct TipStrings<'a> {
    pub unset: &'a str,
    pub locked: &'a str,
    pub secret: &'a str,
}

/// The tooltip's two lines for `{{name}}`: the reference, then its value.
/// A secret is always masked; its plaintext never reaches the text.
pub fn tip_text(name: &str, peek: &VarPeek, t: &TipStrings) -> (String, String) {
    let body = match peek {
        VarPeek::Unset => t.unset.to_string(),
        VarPeek::Locked => t.locked.to_string(),
        VarPeek::Secret(_) => format!("{TIP_MASK}  {}", t.secret),
        VarPeek::Plain(v) if v.chars().count() > TIP_MAX_CHARS => {
            let cut: String = v.chars().take(TIP_MAX_CHARS).collect();
            format!("{cut}…")
        }
        VarPeek::Plain(v) => v.clone(),
    };
    (format!("{{{{{name}}}}}"), body)
}

/// What `name` resolves to, for a tooltip: like [`Services::peek_var`], but
/// a secret is never decrypted - the tooltip only shows it masked, so its
/// plaintext is never loaded on hover.
fn peek_masked(s: &Services, name: &str) -> VarPeek {
    let Some(raw) = s.environments.read().lookup(name, None, &[]) else {
        return VarPeek::Unset;
    };
    if hs_core::secrets::SecretRef::parse(&raw).is_none() {
        VarPeek::Plain(raw)
    } else if s.secrets.is_locked() {
        VarPeek::Locked
    } else {
        VarPeek::Secret(String::new())
    }
}

#[derive(Clone)]
struct Installed(std::sync::Arc<Services>);

fn installed_id() -> egui::Id {
    egui::Id::new("hs-var-tip-services")
}

/// Make the shared fields ([`crate::varfield::var_field`],
/// [`crate::kv_editor::KvEditor`], [`crate::form::text_field`] and the code
/// editor with `var_tips` on) show a `{{name}}` tooltip on hover. The
/// shell calls it once at start-up.
pub fn install(ctx: &egui::Context, s: &Services) {
    let v = Installed(std::sync::Arc::new(s.clone()));
    ctx.data_mut(|d| d.insert_temp(installed_id(), v));
}

fn installed(ctx: &egui::Context) -> Option<std::sync::Arc<Services>> {
    ctx.data(|d| d.get_temp::<Installed>(installed_id()))
        .map(|i| i.0)
}

/// Show the tooltip for the reference at `byte` of `text` at the pointer,
/// while `response` is hovered. Nothing when no reference holds `byte`.
pub fn reference_tip(response: &egui::Response, s: &Services, text: &str, byte: usize) {
    let Some(name) = reference_at(text, byte) else {
        return;
    };
    let (unset, locked, secret) = (
        s.t("var-peek-unset"),
        s.t("var-peek-locked"),
        s.t("var-tip-secret"),
    );
    let strings = TipStrings {
        unset: &unset,
        locked: &locked,
        secret: &secret,
    };
    let (head, body) = tip_text(name, &peek_masked(s, name), &strings);
    response.clone().on_hover_ui_at_pointer(|ui| {
        ui.label(egui::RichText::new(head).monospace().strong());
        ui.label(egui::RichText::new(body).monospace());
    });
}

/// [`reference_tip`] through the Services [`install`] stored; nothing
/// before it is called.
pub fn installed_reference_tip(response: &egui::Response, text: &str, byte: usize) {
    if let Some(s) = installed(&response.ctx) {
        reference_tip(response, &s, text, byte);
    }
}

/// The char of `out`'s galley under the pointer, while it is over the text.
fn hovered_char(out: &egui::text_edit::TextEditOutput) -> Option<usize> {
    let p = out.response.response.hover_pos()?;
    if !out.text_clip_rect.contains(p) {
        return None;
    }
    let rel = p - out.galley_pos;
    if !out.galley.rect.contains(rel.to_pos2()) {
        return None;
    }
    let c = out.galley.cursor_from_pos(rel);
    // The cursor snaps to the nearest gap; the char is the one to its
    // left when the pointer is left of that gap.
    let gap = out.galley.pos_from_cursor(c).min.x;
    Some(if rel.x < gap {
        c.index.0.saturating_sub(1)
    } else {
        c.index.0
    })
}

/// A tooltip over the `{{ name }}` under the pointer in a `TextEdit`
/// (`out` is what `TextEdit::show` returned for `text`). Cheap: it does
/// nothing unless the field is hovered and holds a `{{`.
pub fn hover_tip(s: &Services, out: &egui::text_edit::TextEditOutput, text: &str) {
    if !out.response.response.hovered() || !text.contains("{{") {
        return;
    }
    if let Some(ch) = hovered_char(out) {
        let byte = text.char_indices().nth(ch).map_or(text.len(), |(b, _)| b);
        reference_tip(&out.response.response, s, text, byte);
    }
}

/// [`hover_tip`] through the Services [`install`] stored.
pub fn text_edit_tip(out: &egui::text_edit::TextEditOutput, text: &str) {
    if !out.response.response.hovered() || !text.contains("{{") {
        return;
    }
    if let Some(s) = installed(&out.response.response.ctx) {
        hover_tip(&s, out, text);
    }
}

#[cfg(test)]
mod tests {
    use super::{TipStrings, VarPeek, reference_at, references, tip_text};

    #[test]
    fn finds_references_once_each() {
        assert_eq!(
            references("{{a}} and {{ b }} then {{a}} {{"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(references("plain").is_empty());
    }

    #[test]
    fn the_hovered_byte_maps_to_its_reference() {
        let t = "GET {{base}}/v1?k={{ api_key }}&x={{}} {{open";
        // Before, on each brace, inside, and after the first reference.
        assert_eq!(reference_at(t, 3), None);
        assert_eq!(reference_at(t, 4), Some("base"));
        assert_eq!(reference_at(t, 7), Some("base"));
        assert_eq!(reference_at(t, 11), Some("base"));
        assert_eq!(reference_at(t, 12), None);
        // The second, with spaces, trimmed.
        let k = t.find("{{ api").unwrap();
        assert_eq!(reference_at(t, k + 5), Some("api_key"));
        // An empty and an unclosed one are not references.
        let empty = t.find("{{}}").unwrap();
        assert_eq!(reference_at(t, empty + 1), None);
        assert_eq!(reference_at(t, t.len() - 2), None);
        // Multi-byte text before a reference.
        let u = "é→{{ü}}";
        assert_eq!(reference_at(u, u.find('ü').unwrap()), Some("ü"));
        assert_eq!(reference_at(u, 0), None);
    }

    #[test]
    fn a_secret_never_appears_in_the_tooltip() {
        let t = TipStrings {
            unset: "(Not set)",
            locked: "(Vault locked)",
            secret: "(Secret from the vault)",
        };
        let (head, body) = tip_text("token", &VarPeek::Secret("hunter2-plain".into()), &t);
        assert_eq!(head, "{{token}}");
        assert!(!head.contains("hunter2") && !body.contains("hunter2"));
        assert!(body.contains("••••••••") && body.contains("(Secret from the vault)"));
        assert_eq!(tip_text("a", &VarPeek::Unset, &t).1, "(Not set)");
        assert_eq!(tip_text("a", &VarPeek::Locked, &t).1, "(Vault locked)");
        assert_eq!(tip_text("a", &VarPeek::Plain("v1".into()), &t).1, "v1");
        let long = "x".repeat(1000);
        let body = tip_text("a", &VarPeek::Plain(long), &t).1;
        assert_eq!(body.chars().count(), 301);
        assert!(body.ends_with('…'));
    }
}
