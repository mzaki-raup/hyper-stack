//! A single-line text field with `{{ }}` environment-variable autocomplete.
//!
//! When the caret sits inside an unclosed `{{` the field drops a popup that
//! fuzzy-matches the available variable keys. Picking one inserts
//! `name}}`; an "Add variable" row lets the user define a brand-new key and
//! value on the spot, which the caller persists to the active environment.

use crate::form;
use crate::icons::ph;
use crate::theme::Theme;

/// What [`var_field`] produced this frame.
pub struct VarFieldOutput {
    pub response: egui::Response,
    pub changed: bool,
    /// A new `(key, value)` the user asked to create. The caller writes it to
    /// the active environment; the reference is already inserted in the text.
    pub add_var: Option<(String, String)>,
}

#[derive(Clone, Default)]
struct PopupState {
    selected: usize,
    adding: bool,
    add_value: String,
}

/// Byte index of the caret from a char index into `text`.
fn byte_of_char(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// If the caret is inside an unclosed `{{`, return `(brace_start_byte,
/// partial)` where `partial` is the text typed after the braces.
fn open_token(text: &str, caret_byte: usize) -> Option<(usize, String)> {
    let before = &text[..caret_byte];
    let start = before.rfind("{{")?;
    let partial = &before[start + 2..];
    // A `}` (or a later `{`) means the token is already closed or restarted.
    if partial.contains('}') || partial.contains('{') || partial.contains('\n') {
        return None;
    }
    Some((start, partial.to_string()))
}

/// A `{{ }}`-aware single-line field. `vars` is the list of `(key, value)`
/// pairs visible in the active environment. The field auto-sizes between
/// [`form::FIELD_MIN_WIDTH`] and [`form::FIELD_MAX_WIDTH`].
pub fn var_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    hint: &str,
    vars: &[(String, String)],
) -> VarFieldOutput {
    let width = ui
        .available_width()
        .clamp(form::FIELD_MIN_WIDTH, form::FIELD_MAX_WIDTH);
    var_field_width(ui, theme, id_salt, value, hint, vars, width)
}

/// [`var_field`] with an explicit width (for fields that share a row).
pub fn var_field_width(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    hint: &str,
    vars: &[(String, String)],
    width: f32,
) -> VarFieldOutput {
    let id = egui::Id::new((ui.id(), id_salt));
    let before = value.clone();
    let out = egui::TextEdit::singleline(value)
        .hint_text(hint)
        .margin(form::FIELD_MARGIN)
        .desired_width(width)
        .id(id)
        .show(ui);
    let response = out.response.response.clone();
    let changed = *value != before;

    let mut add_var = None;

    // The caret (primary cursor) as a byte offset into the current text.
    let caret_byte = out
        .cursor_range
        .map(|r| byte_of_char(value, r.primary.index.0))
        .unwrap_or_else(|| value.len());

    let mut state = ui.data_mut(|d| d.get_temp::<PopupState>(id).unwrap_or_default());

    // Keep the popup alive on the frame focus leaves (Enter pressed, or a row
    // clicked), and for as long as the user is typing a new variable's value
    // in the popup (which steals focus from this field).
    let active = response.has_focus() || response.lost_focus() || state.adding;
    let token = if active {
        open_token(value, caret_byte)
    } else {
        None
    };

    let Some((brace_start, partial)) = token else {
        ui.data_mut(|d| d.remove::<PopupState>(id));
        return VarFieldOutput {
            response,
            changed,
            add_var,
        };
    };

    // Rank the keys against the partial (everything when the partial is empty).
    let ranked: Vec<usize> = if partial.is_empty() {
        (0..vars.len()).collect()
    } else {
        let refs: Vec<(&str, Option<&str>)> =
            vars.iter().map(|(k, _)| (k.as_str(), None)).collect();
        hs_core::fuzzy::rank(&partial, &refs)
            .into_iter()
            .map(|m| m.index)
            .collect()
    };
    let exact = vars.iter().any(|(k, _)| k == &partial);
    let can_add = !partial.is_empty() && !exact;

    if ranked.is_empty() && !can_add {
        return VarFieldOutput {
            response,
            changed,
            add_var,
        };
    }

    let option_count = ranked.len() + usize::from(can_add);
    state.selected = state.selected.min(option_count.saturating_sub(1));

    // Keyboard navigation while the field keeps focus.
    let (mut accept, mut dismiss) = (false, false);
    ui.input(|i| {
        if i.key_pressed(egui::Key::ArrowDown) {
            state.selected = (state.selected + 1) % option_count;
        }
        if i.key_pressed(egui::Key::ArrowUp) {
            state.selected = (state.selected + option_count - 1) % option_count;
        }
        if i.key_pressed(egui::Key::Enter) {
            accept = true;
        }
        if i.key_pressed(egui::Key::Escape) {
            dismiss = true;
        }
    });

    let mut chosen: Option<String> = None;
    let add_index = ranked.len();

    let popup_pos = egui::pos2(response.rect.left(), response.rect.bottom() + 2.0);
    egui::Area::new(id.with("popup"))
        .order(egui::Order::Foreground)
        .fixed_pos(popup_pos)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.surface)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(ui, |ui| {
                    ui.set_min_width(width.max(220.0));
                    ui.set_max_width(width.max(220.0));
                    ui.set_max_height(240.0);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for (row, &vi) in ranked.iter().enumerate() {
                            let (key, val) = &vars[vi];
                            let selected = row == state.selected;
                            let r = row_widget(ui, theme, selected, ph::BRACKETS_CURLY, key, val);
                            if r.clicked() {
                                chosen = Some(key.clone());
                            }
                            if r.hovered() {
                                state.selected = row;
                            }
                        }
                        if can_add {
                            if !ranked.is_empty() {
                                ui.separator();
                            }
                            let selected = state.selected == add_index;
                            let label = format!("{}  Add \"{partial}\"", ph::PLUS_CIRCLE);
                            let r = row_label(ui, theme, selected, &label, theme.accent);
                            if r.hovered() {
                                state.selected = add_index;
                            }
                            if r.clicked() {
                                state.adding = true;
                            }
                            if state.adding {
                                ui.horizontal(|ui| {
                                    ui.label(format!("{partial} ="));
                                    let vr = ui.add(
                                        egui::TextEdit::singleline(&mut state.add_value)
                                            .hint_text("value")
                                            .desired_width(width - 130.0),
                                    );
                                    vr.request_focus();
                                    let enter = vr.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    if ui.button(ph::CHECK).clicked() || enter {
                                        add_var = Some((partial.clone(), state.add_value.clone()));
                                        chosen = Some(partial.clone());
                                    }
                                    if ui.button(ph::X).clicked() {
                                        state.adding = false;
                                        state.add_value.clear();
                                    }
                                });
                            }
                        }
                    });
                });
        });

    // Enter accepts the highlighted row (unless the add-value box ate it).
    if accept && chosen.is_none() && !state.adding {
        if state.selected < ranked.len() {
            chosen = Some(vars[ranked[state.selected]].0.clone());
        } else if can_add {
            state.adding = true;
        }
    }

    if let Some(key) = chosen {
        // Replace the partial (brace_start+2 .. caret) with `key}}`.
        let insert = format!("{key}}}}}");
        value.replace_range(brace_start + 2..caret_byte, &insert);
        // Move the caret just past the inserted `}}`.
        let new_byte = brace_start + 2 + insert.len();
        let new_char = value[..new_byte.min(value.len())].chars().count();
        let mut tstate = out.state.clone();
        let ccursor = egui::text::CCursor::new(new_char);
        tstate
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
        tstate.store(ui.ctx(), id);
        ui.data_mut(|d| d.remove::<PopupState>(id));
        response.request_focus();
        return VarFieldOutput {
            response,
            changed: true,
            add_var,
        };
    }

    if dismiss {
        ui.data_mut(|d| d.remove::<PopupState>(id));
    } else {
        ui.data_mut(|d| d.insert_temp(id, state));
    }

    VarFieldOutput {
        response,
        changed,
        add_var,
    }
}

fn row_widget(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: &str,
    key: &str,
    value: &str,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width(), 22.0);
    let (rect, r) = ui.allocate_exact_size(desired, egui::Sense::click());
    if selected || r.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.surface_hover);
    }
    let p = ui.painter();
    p.text(
        egui::pos2(rect.left() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(14.0),
        theme.accent,
    );
    p.text(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        key,
        egui::FontId::proportional(13.0),
        theme.text,
    );
    let preview: String = value.chars().take(28).collect();
    p.text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        preview,
        egui::FontId::proportional(12.0),
        theme.text_muted,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn row_label(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    label: &str,
    colour: egui::Color32,
) -> egui::Response {
    let desired = egui::vec2(ui.available_width(), 22.0);
    let (rect, r) = ui.allocate_exact_size(desired, egui::Sense::click());
    if selected || r.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme.surface_hover);
    }
    ui.painter().text(
        egui::pos2(rect.left() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        colour,
    );
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}
