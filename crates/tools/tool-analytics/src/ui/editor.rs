//! The SQL page's editor: the shared code editor with SQL colours, and a
//! completion list drawn from the database's tables and columns (docs/
//! tools/analytics.md §4.6). Typing a word of two characters, or a dot after
//! a table, offers the list; Ctrl+Space forces it; Up and Down move, Enter or
//! Tab take, Escape closes.

use hs_core::Services;
use hs_editor::buffer::EditKind;
use hs_editor::cursor::{byte_to_char, char_to_byte};
use hs_editor::view::EditorTheme;
use hs_editor::{Buffer, Cursor, Cursors, EditorView, languages};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;

use crate::complete::{self, Completion, Kind};
use crate::provider::TableMeta;

struct Popup {
    completion: Completion,
    selected: usize,
}

pub struct SqlEditor {
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    popup: Option<Popup>,
}

impl SqlEditor {
    pub fn new(s: &Services) -> SqlEditor {
        let st = s.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
        let mut view = EditorView::new(
            egui::Id::new(("analytics-sql-editor", hs_core::new_id())),
            languages::detect("query.sql"),
            dark,
            st.editor.font_size,
        );
        view.right_click_copy_paste = st.editor.right_click_copy_paste;
        SqlEditor {
            buf: Buffer::from_text(""),
            cursors: Cursors::single(0),
            view,
            popup: None,
        }
    }

    pub fn text(&self) -> String {
        self.buf.text()
    }

    pub fn set_text(&mut self, text: &str) {
        self.buf = Buffer::from_text(text);
        self.cursors = Cursors::single(0);
        self.popup = None;
    }

    /// Draw the editor `height` tall; true when the text changed. `tables`
    /// is what completion draws on, asked for only when the list is updated.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        height: f32,
        tables: &mut dyn FnMut() -> Vec<TableMeta>,
    ) -> bool {
        // Ctrl+Space asks for the list; the popup's own keys come first.
        let force = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Space));
        self.popup_keys(ui);
        let ed_theme = EditorTheme::for_theme(theme);
        let before = (self.buf.version(), self.cursors.primary().head);
        ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
            self.view
                .show(ui, &mut self.buf, &mut self.cursors, &ed_theme)
        });
        let changed = self.buf.version() != before.0;
        if changed || force {
            self.update(tables(), force);
        } else if self.cursors.primary().head != before.1 {
            self.popup = None;
        }
        self.popup_ui(ui, theme);
        changed
    }

    fn update(&mut self, tables: Vec<TableMeta>, force: bool) {
        let text = self.buf.text();
        let caret = char_to_byte(&text, self.cursors.primary().head);
        let c = complete::complete(&text, caret, &tables, force);
        self.popup = if c.items.is_empty() {
            None
        } else {
            let selected = self
                .popup
                .as_ref()
                .map_or(0, |p| p.selected.min(c.items.len() - 1));
            Some(Popup {
                completion: c,
                selected,
            })
        };
    }

    fn accept(&mut self) {
        let Some(p) = self.popup.take() else { return };
        let Some(item) = p.completion.items.get(p.selected) else {
            return;
        };
        let text = self.buf.text();
        let (a, b) = (
            byte_to_char(&text, p.completion.start),
            byte_to_char(&text, p.completion.end),
        );
        self.cursors.set_single(Cursor {
            head: b,
            anchor: a,
            goal_col: None,
        });
        self.view.insert_at_cursors(
            &mut self.buf,
            &mut self.cursors,
            &item.label,
            EditKind::Other,
        );
    }

    /// Keys the open list takes before the editor sees them.
    fn popup_keys(&mut self, ui: &mut egui::Ui) {
        let Some(n) = self.popup.as_ref().map(|p| p.completion.items.len()) else {
            return;
        };
        let (down, up, accept, close) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if let Some(p) = &mut self.popup {
            if down {
                p.selected = (p.selected + 1) % n.max(1);
            }
            if up {
                p.selected = (p.selected + n.max(1) - 1) % n.max(1);
            }
        }
        if accept {
            self.accept();
        }
        if close {
            self.popup = None;
            // egui drops focus on Escape; the key only meant "close the list".
            ui.memory_mut(|m| m.request_focus(self.view.id));
        }
    }

    fn popup_ui(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let (Some(p), Some(rect)) = (&self.popup, self.view.cursor_rect) else {
            return;
        };
        let mut clicked = None;
        egui::Area::new(self.view.id.with("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_bottom() + egui::vec2(0.0, 2.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(260.0);
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for (i, it) in p.completion.items.iter().enumerate() {
                                let icon = match it.kind {
                                    Kind::Column => ph::COLUMNS,
                                    Kind::Table => ph::TABLE,
                                    Kind::Keyword => ph::TEXT_AA,
                                };
                                let r = ui.horizontal(|ui| {
                                    let r = hs_ui::form::selectable_label(
                                        ui,
                                        i == p.selected,
                                        format!("{icon} {}", it.label),
                                    );
                                    if !it.detail.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&it.detail)
                                                .small()
                                                .color(theme.text_muted),
                                        );
                                    }
                                    r
                                });
                                if i == p.selected {
                                    r.inner.scroll_to_me(None);
                                }
                                if r.inner.clicked() {
                                    clicked = Some(i);
                                }
                            }
                        });
                });
            });
        if let Some(i) = clicked {
            if let Some(p) = &mut self.popup {
                p.selected = i;
            }
            self.accept();
        }
    }
}
