//! Command palette and quick-open (docs/04 §4): one overlay, fuzzy-ranked
//! items from any provider, keyboard-driven.

use egui::{Align2, Color32, CornerRadius, Key, Vec2};

use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub struct PaletteItem {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub shortcut: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    Commands,
    QuickOpen,
}

#[derive(Debug, Clone)]
pub struct PaletteState {
    pub open: bool,
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
    just_opened: bool,
    /// Suppress the outside-click handler on the same frame that opened the
    /// popup (for example, a click on the Quick Open toolbar button).
    opened_this_frame: bool,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            open: false,
            mode: PaletteMode::Commands,
            query: String::new(),
            selected: 0,
            just_opened: false,
            opened_this_frame: false,
        }
    }
}

impl PaletteState {
    pub fn open(&mut self, mode: PaletteMode) {
        self.open = true;
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.just_opened = true;
        self.opened_this_frame = true;
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    pub fn toggle(&mut self, mode: PaletteMode) {
        if self.open && self.mode == mode {
            self.close()
        } else {
            self.open(mode)
        }
    }
}

pub struct PaletteLabels {
    pub placeholder_commands: String,
    pub placeholder_quick_open: String,
    pub no_results: String,
}

/// Draw the palette. Returns the id of the chosen item, if any, this frame.
pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    state: &mut PaletteState,
    items: &[PaletteItem],
    labels: &PaletteLabels,
) -> Option<String> {
    if !state.open {
        return None;
    }
    let mut chosen = None;
    let ranked: Vec<(usize, Vec<usize>)> = {
        let refs: Vec<(&str, Option<&str>)> = items
            .iter()
            .map(|i| (i.title.as_str(), i.subtitle.as_deref()))
            .collect();
        hs_core::fuzzy::rank(&state.query, &refs)
            .into_iter()
            .take(60)
            .map(|m| (m.index, m.positions))
            .collect()
    };
    if state.selected >= ranked.len() {
        state.selected = ranked.len().saturating_sub(1);
    }

    // Keyboard handling before drawing so the selection moves this frame.
    let (esc, enter, down, up) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, Key::Escape),
            i.consume_key(egui::Modifiers::NONE, Key::Enter),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
        )
    });
    if esc {
        state.close();
        return None;
    }
    if down && !ranked.is_empty() {
        state.selected = (state.selected + 1) % ranked.len();
    }
    if up && !ranked.is_empty() {
        state.selected = (state.selected + ranked.len() - 1) % ranked.len();
    }
    if enter {
        if let Some((idx, _)) = ranked.get(state.selected) {
            chosen = Some(items[*idx].id.clone());
        }
        state.close();
        return chosen;
    }

    let screen = ctx.content_rect();
    let width = (screen.width() * 0.6).clamp(360.0, 720.0);
    let area = egui::Area::new(egui::Id::new("hs-palette"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_TOP, Vec2::new(0.0, 80.0))
        .interactable(true);
    let response = area.show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme.panel_bg)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(egui::Margin::same(8))
            .shadow(egui::epaint::Shadow {
                offset: [0, 4],
                blur: 16,
                spread: 0,
                color: Color32::from_black_alpha(90),
            })
            .show(ui, |ui| {
                ui.set_width(width);
                let placeholder = match state.mode {
                    PaletteMode::Commands => &labels.placeholder_commands,
                    PaletteMode::QuickOpen => &labels.placeholder_quick_open,
                };
                let edit = egui::TextEdit::singleline(&mut state.query)
                    .hint_text(placeholder.as_str())
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Heading);
                let r = ui.add(edit);
                if state.just_opened {
                    r.request_focus();
                    state.just_opened = false;
                }
                if r.changed() {
                    state.selected = 0;
                }
                ui.add_space(6.0);
                if ranked.is_empty() {
                    ui.colored_label(theme.text_muted, &labels.no_results);
                    return;
                }
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        for (row, (idx, positions)) in ranked.iter().enumerate() {
                            let item = &items[*idx];
                            let selected = row == state.selected;
                            let (rect, resp) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), 30.0),
                                egui::Sense::click(),
                            );
                            if selected {
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(4),
                                    theme.accent.linear_multiply(0.3),
                                );
                                ui.scroll_to_rect(rect, None);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::same(4),
                                    theme.surface_hover,
                                );
                            }
                            let mut x = rect.left() + 8.0;
                            let font = egui::FontId::proportional(14.0);
                            if let Some(icon) = &item.icon {
                                ui.painter().text(
                                    egui::pos2(x, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    icon,
                                    font.clone(),
                                    theme.text_muted,
                                );
                                x += 24.0;
                            }
                            // Title with matched characters highlighted.
                            let mut job = egui::text::LayoutJob::default();
                            for (byte, ch) in item.title.char_indices() {
                                let hit = positions.contains(&byte);
                                job.append(
                                    &ch.to_string(),
                                    0.0,
                                    egui::TextFormat {
                                        font_id: font.clone(),
                                        color: if hit { theme.accent } else { theme.text },
                                        ..Default::default()
                                    },
                                );
                            }
                            let galley = ui.painter().layout_job(job);
                            let text_y = rect.center().y - galley.size().y / 2.0;
                            ui.painter()
                                .galley(egui::pos2(x, text_y), galley.clone(), theme.text);
                            x += galley.size().x + 10.0;
                            if let Some(sub) = &item.subtitle {
                                ui.painter().text(
                                    egui::pos2(x, rect.center().y),
                                    Align2::LEFT_CENTER,
                                    sub,
                                    egui::FontId::proportional(12.0),
                                    theme.text_muted,
                                );
                            }
                            if let Some(sc) = &item.shortcut {
                                ui.painter().text(
                                    egui::pos2(rect.right() - 8.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    sc,
                                    egui::FontId::monospace(12.0),
                                    theme.text_muted,
                                );
                            }
                            if resp.clicked() {
                                chosen = Some(item.id.clone());
                            }
                        }
                    });
            });
    });
    let just_opened = std::mem::take(&mut state.opened_this_frame);
    if !just_opened
        && response.response.clicked_elsewhere()
        && !ctx.input(|i| {
            i.pointer.any_pressed()
                && response
                    .response
                    .rect
                    .contains(i.pointer.interact_pos().unwrap_or_default())
        })
    {
        state.close();
    }
    if chosen.is_some() {
        state.close();
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[derive(Default)]
    struct State {
        palette: PaletteState,
    }

    #[test]
    fn opening_quick_open_from_a_button_does_not_close_on_the_same_click() {
        let theme = Theme::dark();
        let labels = PaletteLabels {
            placeholder_commands: "Type a command…".into(),
            placeholder_quick_open: "Quick Open".into(),
            no_results: "Nothing matches.".into(),
        };
        let items = [PaletteItem {
            id: "open:editor".into(),
            title: "Editor".into(),
            subtitle: None,
            icon: None,
            shortcut: None,
        }];
        let mut harness = Harness::new_ui_state(
            |ui, state: &mut State| {
                if ui.button("Quick Open").clicked() {
                    state.palette.open(PaletteMode::QuickOpen);
                }
                show(ui.ctx(), &theme, &mut state.palette, &items, &labels);
            },
            State::default(),
        );
        harness.run();
        harness.get_by_label("Quick Open").click();
        harness.run();
        assert!(harness.state().palette.open);
    }
}
