//! The egui editor widget: virtualised rendering with a gutter, highlighted
//! runs, selections and cursors; keyboard editing with multi-cursor support;
//! find/replace and go-to-line bars.

use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Vec2};
use hs_ui::tip::TipExt;

use crate::buffer::{Buffer, Edit, EditKind};
use crate::cursor::{Cursor, Cursors, Move, byte_to_char, char_to_byte};
use crate::highlight::LineHighlighter;
use crate::languages::Language;

#[derive(Debug, Clone, PartialEq)]
pub struct EditorTheme {
    pub background: Color32,
    pub gutter_bg: Color32,
    pub gutter_fg: Color32,
    pub text: Color32,
    pub current_line: Color32,
    pub selection: Color32,
    pub cursor: Color32,
    pub bracket: Color32,
    pub find_match: Color32,
    pub find_current: Color32,
    pub error: Color32,
    pub warning: Color32,
    pub info: Color32,
    pub dark: bool,
}

impl EditorTheme {
    /// Follow the app colour scheme (syntax colours stay per light/dark).
    pub fn for_theme(theme: &hs_ui::theme::Theme) -> Self {
        let mut t = if theme.dark {
            Self::dark()
        } else {
            Self::light()
        };
        t.background = theme.window_bg;
        t.gutter_bg = theme.panel_bg;
        t.gutter_fg = theme.text_muted;
        t.text = theme.text;
        t.cursor = theme.text;
        t.selection = theme.accent.linear_multiply(0.35);
        t.bracket = theme.secondary.linear_multiply(0.45);
        t.find_match = theme.warning.linear_multiply(0.35);
        t.find_current = theme.warning.linear_multiply(0.65);
        t.error = theme.error;
        t.warning = theme.warning;
        t.info = theme.secondary;
        t
    }

    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x15, 0x15, 0x1A),
            gutter_bg: Color32::from_rgb(0x1C, 0x1C, 0x23),
            gutter_fg: Color32::from_rgb(0x6A, 0x6A, 0x76),
            text: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            current_line: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 10),
            selection: Color32::from_rgba_unmultiplied(0x9A, 0x4D, 0xF0, 80),
            cursor: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            bracket: Color32::from_rgba_unmultiplied(0x3F, 0x9C, 0xEE, 110),
            find_match: Color32::from_rgba_unmultiplied(0xD9, 0xB0, 0x2A, 90),
            find_current: Color32::from_rgba_unmultiplied(0xD9, 0xB0, 0x2A, 170),
            error: Color32::from_rgb(0xF0, 0x5A, 0x5A),
            warning: Color32::from_rgb(0xD9, 0xB0, 0x2A),
            info: Color32::from_rgb(0x3F, 0x9C, 0xEE),
            dark: true,
        }
    }
    pub fn light() -> Self {
        Self {
            background: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            gutter_bg: Color32::from_rgb(0xF4, 0xF4, 0xF7),
            gutter_fg: Color32::from_rgb(0x9A, 0x9A, 0xA6),
            text: Color32::from_rgb(0x1E, 0x1E, 0x24),
            current_line: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            selection: Color32::from_rgba_unmultiplied(0x72, 0x00, 0xCA, 50),
            cursor: Color32::from_rgb(0x1E, 0x1E, 0x24),
            bracket: Color32::from_rgba_unmultiplied(0x00, 0x75, 0xD3, 90),
            find_match: Color32::from_rgba_unmultiplied(0xA7, 0x87, 0x00, 70),
            find_current: Color32::from_rgba_unmultiplied(0xA7, 0x87, 0x00, 150),
            error: Color32::from_rgb(0xC8, 0x1E, 0x1E),
            warning: Color32::from_rgb(0xA7, 0x87, 0x00),
            info: Color32::from_rgb(0x00, 0x75, 0xD3),
            dark: false,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct EditorResponse {
    pub changed: bool,
    pub save_requested: bool,
    pub focused: bool,
    pub copied: Option<String>,
    /// Text typed this frame (the last `Text` event), for completion
    /// triggers.
    pub typed: Option<String>,
    /// Keys the view left alone because [`EditorView::popup_open`] was set:
    /// arrows, Page Up/Down, Enter, Tab and Escape, in order.
    pub popup_keys: Vec<egui::Key>,
    /// The char under the mouse and where the mouse is, while it is over
    /// the text (hover requests).
    pub hover_char: Option<usize>,
    pub hover_pos: Option<Pos2>,
    /// A plain click in the text (closes popups).
    pub clicked: bool,
    /// Ctrl+click placed the cursor (go to definition).
    pub ctrl_clicked: bool,
    /// A click in the gutter (left of the line numbers): the 0-based line
    /// (toggles a breakpoint).
    pub gutter_clicked: Option<usize>,
}

/// A diagnostic underline: a char range and how serious it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Squiggle {
    pub start: usize,
    pub end: usize,
    pub level: SquiggleLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SquiggleLevel {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Default, Clone)]
pub struct FindState {
    pub open: bool,
    pub replace_open: bool,
    pub query: String,
    pub replacement: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub matches: Vec<(usize, usize)>,
    pub current: usize,
    pub focus_query: bool,
    last_key: (String, bool, bool, bool, u64),
}

/// How a line differs from the last commit, for the gutter marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineMark {
    Added,
    Modified,
    /// Lines were removed just above this one.
    Deleted,
}

impl LineMark {
    pub fn colour(self) -> Color32 {
        match self {
            LineMark::Added => Color32::from_rgb(0x3F, 0xB9, 0x50),
            LineMark::Modified => Color32::from_rgb(0x4A, 0x90, 0xD9),
            LineMark::Deleted => Color32::from_rgb(0xE0, 0x4F, 0x4F),
        }
    }
}

pub struct EditorView {
    pub id: egui::Id,
    pub font_size: f32,
    pub read_only: bool,
    pub show_gutter: bool,
    pub tab_inserts_spaces: bool,
    pub language: Language,
    pub find: FindState,
    pub goto_open: bool,
    pub goto_text: String,
    pub right_click_copy_paste: bool,
    /// Character ranges underlined as errors (SQL error position, …).
    pub error_ranges: Vec<(usize, usize)>,
    /// Diagnostics from a language server, underlined by severity.
    pub squiggles: Vec<Squiggle>,
    /// A popup (completion) owns the navigation keys: the view reports
    /// them in `EditorResponse::popup_keys` instead of acting on them.
    pub popup_open: bool,
    /// 0-based lines with a breakpoint, drawn as dots in the gutter; the
    /// bool says whether the adapter verified it.
    pub breakpoints: Vec<(usize, bool)>,
    /// The line the debugger stopped on (0-based), highlighted with an
    /// arrow in the gutter.
    pub exec_line: Option<usize>,
    /// Change markers at the gutter's left edge, indexed by line (the Git
    /// changes against the last commit). Lines past the end have none.
    pub line_marks: Vec<Option<LineMark>>,
    /// Screen rectangle of the primary cursor after the last `show`, for
    /// popups anchored at the caret (completion).
    pub cursor_rect: Option<Rect>,
    highlighter: LineHighlighter,
    scroll_to_cursor: bool,
    last_version: u64,
    dragging: bool,
    blink_start: f64,
}

const GUTTER_PAD: f32 = 12.0;

impl EditorView {
    pub fn new(id: egui::Id, language: Language, dark: bool, font_size: f32) -> Self {
        let highlighter = LineHighlighter::new(&language, dark);
        Self {
            id,
            font_size,
            read_only: false,
            show_gutter: true,
            tab_inserts_spaces: !language.use_tabs,
            language,
            find: FindState::default(),
            goto_open: false,
            goto_text: String::new(),
            right_click_copy_paste: true,
            error_ranges: Vec::new(),
            squiggles: Vec::new(),
            popup_open: false,
            breakpoints: Vec::new(),
            exec_line: None,
            line_marks: Vec::new(),
            cursor_rect: None,
            highlighter,
            scroll_to_cursor: false,
            last_version: u64::MAX,
            dragging: false,
            blink_start: 0.0,
        }
    }

    pub fn set_language(&mut self, lang: Language) {
        self.tab_inserts_spaces = !lang.use_tabs;
        self.highlighter.set_language(&lang);
        self.language = lang;
    }

    pub fn set_dark(&mut self, dark: bool) {
        self.highlighter.set_dark(dark);
    }

    pub fn scroll_to_cursor(&mut self) {
        self.scroll_to_cursor = true;
    }

    pub fn open_find(&mut self, replace: bool, seed: Option<String>) {
        self.find.open = true;
        self.find.replace_open = replace;
        if let Some(s) = seed.filter(|s| !s.is_empty() && !s.contains('\n')) {
            self.find.query = s;
        }
        self.find.focus_query = true;
    }

    fn cell(&self, ui: &egui::Ui) -> (egui::FontId, Vec2) {
        let font = egui::FontId::new(self.font_size, hs_ui::fonts::editor_family());
        let size = ui.fonts_mut(|f| {
            Vec2::new(
                f.glyph_width(&font, 'M').max(1.0),
                (f.row_height(&font) * 1.25).max(1.0),
            )
        });
        (font, size)
    }

    /// Draw and handle input. `buf` and `cursors` are owned by the caller.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        theme: &EditorTheme,
    ) -> EditorResponse {
        let mut resp = EditorResponse::default();
        if buf.version() != self.last_version {
            // Changed outside the view (a new file, the AI assistant, a
            // reload): the cached highlight spans no longer match the lines.
            self.highlighter.invalidate_all();
            self.last_version = buf.version();
        }
        self.highlighter.set_dark(theme.dark);

        // Bars above the text.
        self.find_bar(ui, buf, cursors, theme, &mut resp);
        self.goto_bar(ui, buf, cursors);

        let (font, cell) = self.cell(ui);
        let gutter_w = if self.show_gutter {
            (buf.len_lines().max(1).to_string().len() as f32 + 2.5) * cell.x + GUTTER_PAD
        } else {
            4.0
        };
        let total_lines = buf.len_lines();
        let max_chars = buf.max_line_chars();
        let content_size = Vec2::new(
            gutter_w + (max_chars as f32 + 4.0) * cell.x,
            total_lines as f32 * cell.y + cell.y,
        );

        let outer = ui.available_rect_before_wrap();
        ui.painter()
            .rect_filled(outer, CornerRadius::ZERO, theme.background);

        let scroll = egui::ScrollArea::both()
            .id_salt(self.id.with("scroll"))
            .auto_shrink([false, false]);
        let id = self.id;
        let scroll_out = scroll.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(content_size);
            let origin = ui.min_rect().min;
            let text_x = origin.x + gutter_w;
            let first =
                ((viewport.min.y / cell.y).floor() as usize).min(total_lines.saturating_sub(1));
            let last = (((viewport.max.y / cell.y).ceil() as usize) + 1).min(total_lines);
            let visible_rows = ((viewport.height() / cell.y).floor() as usize).max(1);

            // Interaction over the whole content.
            let content_rect = Rect::from_min_size(origin, content_size.max(viewport.size()));
            let response = ui.interact(content_rect, id, Sense::click_and_drag());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "editor")
            });
            if response.clicked() || response.drag_started() {
                ui.memory_mut(|m| m.request_focus(id));
            }
            let focused = ui.memory(|m| m.has_focus(id));
            resp.focused = focused;

            let pos_to_char = |pos: Pos2| -> usize {
                let line = ((pos.y - origin.y) / cell.y).floor().max(0.0) as usize;
                let line = line.min(total_lines.saturating_sub(1));
                let col = (((pos.x - text_x) / cell.x) + 0.5).floor().max(0.0) as usize;
                buf.line_col_to_char(line, col)
            };

            // Mouse.
            if response.drag_started() {
                if let Some(p) = response.interact_pointer_pos() {
                    let at = pos_to_char(p);
                    let mods = ui.input(|i| i.modifiers);
                    if mods.alt {
                        cursors.list.push(Cursor::at(at));
                    } else if mods.shift {
                        let c = cursors.list.last_mut().unwrap();
                        c.head = at;
                    } else {
                        cursors.set_single(Cursor::at(at));
                    }
                    cursors.normalise(buf.len_chars());
                    self.dragging = true;
                }
            } else if response.dragged() && self.dragging {
                if let Some(p) = response.interact_pointer_pos() {
                    let at = pos_to_char(p);
                    if let Some(c) = cursors.list.last_mut() {
                        c.head = at;
                        c.goal_col = None;
                    }
                }
            } else if response.drag_stopped() {
                self.dragging = false;
                cursors.normalise(buf.len_chars());
            }
            let gutter_click = response.clicked()
                && self.show_gutter
                && response
                    .interact_pointer_pos()
                    .is_some_and(|p| p.x < text_x - GUTTER_PAD / 2.0);
            if gutter_click && let Some(p) = response.interact_pointer_pos() {
                let line = ((p.y - origin.y) / cell.y).floor().max(0.0) as usize;
                if line < total_lines {
                    resp.gutter_clicked = Some(line);
                }
            }
            if response.clicked()
                && !self.dragging
                && !gutter_click
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = pos_to_char(p);
                let mods = ui.input(|i| i.modifiers);
                if mods.alt {
                    cursors.list.push(Cursor::at(at));
                } else if mods.shift {
                    cursors.list.last_mut().unwrap().head = at;
                } else {
                    cursors.set_single(Cursor::at(at));
                    resp.ctrl_clicked = mods.command;
                }
                resp.clicked = true;
                cursors.normalise(buf.len_chars());
                self.blink_start = ui.input(|i| i.time);
            }
            if response.double_clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = pos_to_char(p);
                cursors.set_single(Cursor {
                    head: buf.word_end(at),
                    anchor: buf.word_start(at),
                    goal_col: None,
                });
            }
            if response.triple_clicked()
                && let Some(p) = response.interact_pointer_pos()
            {
                let at = pos_to_char(p);
                let (line, _) = buf.char_to_line_col(at);
                let s = buf.line_start(line);
                let e = if line + 1 < total_lines {
                    buf.line_start(line + 1)
                } else {
                    buf.len_chars()
                };
                cursors.set_single(Cursor {
                    head: e,
                    anchor: s,
                    goal_col: None,
                });
            }
            if self.right_click_copy_paste && response.secondary_clicked() {
                ui.memory_mut(|m| m.request_focus(id));
                let selected = selected_text(buf, cursors);
                if selected.is_empty() {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                } else {
                    ui.ctx().copy_text(selected.clone());
                    for c in &mut cursors.list {
                        c.collapse();
                    }
                    resp.copied = Some(selected);
                }
            }
            if ui.input(|i| i.modifiers.command) && response.hovered() {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy.abs() > 0.0 {
                    self.font_size = (self.font_size + dy.signum() * 1.0).clamp(8.0, 40.0);
                }
            }
            if response.hovered()
                && let Some(p) = ui.input(|i| i.pointer.hover_pos())
                && p.x >= text_x
            {
                resp.hover_char = Some(pos_to_char(p));
                resp.hover_pos = Some(p);
            }

            // Keyboard.
            if focused {
                let changed = self.handle_keys(ui, buf, cursors, visible_rows, &mut resp);
                if changed {
                    resp.changed = true;
                    self.scroll_to_cursor = true;
                }
                ui.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        id,
                        egui::EventFilter {
                            tab: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                            escape: false,
                        },
                    )
                });
            }

            // Render visible lines.
            let painter = ui.painter_at(
                viewport
                    .translate(origin.to_vec2())
                    .union(Rect::from_min_size(origin, content_size)),
            );
            let painter = painter.with_clip_rect(Rect::from_min_size(
                origin + viewport.min.to_vec2(),
                viewport.size(),
            ));
            let primary_line = buf.char_to_line_col(cursors.primary().head).0;
            if self.show_gutter {
                let gutter_rect = Rect::from_min_size(
                    Pos2::new(origin.x, origin.y + viewport.min.y),
                    Vec2::new(gutter_w - 4.0, viewport.height()),
                );
                painter.rect_filled(gutter_rect, CornerRadius::ZERO, theme.gutter_bg);
            }
            let bracket = if cursors.list.len() == 1 && !cursors.primary().has_selection() {
                buf.matching_bracket(cursors.primary().head, 20_000)
            } else {
                None
            };
            let blink_on = {
                let t = ui.input(|i| i.time) - self.blink_start;
                !focused || (t % 1.0) < 0.55
            };
            for line in first..last {
                let y = origin.y + line as f32 * cell.y;
                let line_start = buf.line_start(line);
                let line_len = buf.line_len(line);
                let text = buf.line_text(line);
                if line == primary_line && focused {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(text_x - 2.0, y),
                            Vec2::new(viewport.width().max(content_size.x), cell.y),
                        ),
                        CornerRadius::ZERO,
                        theme.current_line,
                    );
                }
                if self.exec_line == Some(line) {
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(text_x - 2.0, y),
                            Vec2::new(viewport.width().max(content_size.x), cell.y),
                        ),
                        CornerRadius::ZERO,
                        theme.warning.linear_multiply(0.22),
                    );
                }
                // Selection.
                for c in &cursors.list {
                    if !c.has_selection() {
                        continue;
                    }
                    let (s, e) = c.range();
                    let ls = line_start;
                    let le = line_start + line_len;
                    if e < ls || s > le {
                        continue;
                    }
                    let a = s.max(ls) - ls;
                    let b = e.min(le) - ls;
                    let extra = if e > le { cell.x * 0.5 } else { 0.0 };
                    painter.rect_filled(
                        Rect::from_min_size(
                            Pos2::new(text_x + a as f32 * cell.x, y),
                            Vec2::new((b - a) as f32 * cell.x + extra, cell.y),
                        ),
                        CornerRadius::ZERO,
                        theme.selection,
                    );
                }
                // Find matches.
                if self.find.open {
                    for (i, (s, e)) in self.find.matches.iter().enumerate() {
                        if *e < line_start || *s > line_start + line_len {
                            continue;
                        }
                        let a = s.max(&line_start) - line_start;
                        let b = e.min(&(line_start + line_len)) - line_start;
                        let col = if i == self.find.current {
                            theme.find_current
                        } else {
                            theme.find_match
                        };
                        painter.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(text_x + a as f32 * cell.x, y),
                                Vec2::new((b - a).max(1) as f32 * cell.x, cell.y),
                            ),
                            CornerRadius::same(2),
                            col,
                        );
                    }
                }
                // Bracket highlight.
                if let Some((bs, be)) = bracket {
                    for b in [bs, be] {
                        if b >= line_start && b < line_start + line_len {
                            painter.rect_filled(
                                Rect::from_min_size(
                                    Pos2::new(text_x + (b - line_start) as f32 * cell.x, y),
                                    cell,
                                ),
                                CornerRadius::same(2),
                                theme.bracket,
                            );
                        }
                    }
                }
                // Change marker.
                if self.show_gutter
                    && let Some(Some(m)) = self.line_marks.get(line)
                {
                    let x = origin.x + 1.0;
                    match m {
                        LineMark::Deleted => {
                            // A wedge on the line where text was removed.
                            painter.add(egui::Shape::convex_polygon(
                                vec![
                                    Pos2::new(x, y - 3.0),
                                    Pos2::new(x + 5.0, y),
                                    Pos2::new(x, y + 3.0),
                                ],
                                m.colour(),
                                egui::Stroke::NONE,
                            ));
                        }
                        _ => {
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x, y), Vec2::new(3.0, cell.y)),
                                CornerRadius::ZERO,
                                m.colour(),
                            );
                        }
                    }
                }
                // Breakpoint dot and the execution arrow, left of the number.
                if self.show_gutter {
                    let cx = origin.x + 6.0 + cell.x * 0.5;
                    let cy = y + cell.y / 2.0;
                    if let Some((_, verified)) = self.breakpoints.iter().find(|(l, _)| *l == line) {
                        let r = (cell.y * 0.28).clamp(3.0, 6.0);
                        if *verified {
                            painter.circle_filled(Pos2::new(cx, cy), r, theme.error);
                        } else {
                            painter.circle_stroke(
                                Pos2::new(cx, cy),
                                r,
                                egui::Stroke::new(1.5, theme.error),
                            );
                        }
                    }
                    if self.exec_line == Some(line) {
                        let w = (cell.y * 0.35).clamp(4.0, 7.0);
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                Pos2::new(cx - w * 0.6, cy - w),
                                Pos2::new(cx + w, cy),
                                Pos2::new(cx - w * 0.6, cy + w),
                            ],
                            theme.warning,
                            egui::Stroke::NONE,
                        ));
                    }
                }
                // Gutter number.
                if self.show_gutter {
                    let n = (line + 1).to_string();
                    let col = if line == primary_line {
                        theme.text
                    } else {
                        theme.gutter_fg
                    };
                    painter.text(
                        Pos2::new(
                            origin.x + gutter_w - GUTTER_PAD,
                            y + (cell.y - self.font_size) / 2.0 - 1.0,
                        ),
                        egui::Align2::RIGHT_TOP,
                        n,
                        font.clone(),
                        col,
                    );
                }
                // Text runs.
                let spans = self.highlighter.spans(buf, line);
                let text_y = y + (cell.y - self.font_size) / 2.0 - 1.0;
                if spans.is_empty() {
                    if !text.is_empty() {
                        painter.text(
                            Pos2::new(text_x, text_y),
                            egui::Align2::LEFT_TOP,
                            &text,
                            font.clone(),
                            theme.text,
                        );
                    }
                } else {
                    let chars: Vec<char> = text.chars().collect();
                    for s in spans {
                        let a = s.start.min(chars.len());
                        let b = s.end.min(chars.len());
                        if a >= b {
                            continue;
                        }
                        let piece: String = chars[a..b].iter().collect();
                        if piece.trim().is_empty() {
                            continue;
                        }
                        painter.text(
                            Pos2::new(text_x + a as f32 * cell.x, text_y),
                            egui::Align2::LEFT_TOP,
                            piece,
                            font.clone(),
                            s.fg,
                        );
                    }
                }
                // Error underlines.
                for (s, e) in &self.error_ranges {
                    if *e < line_start || *s > line_start + line_len {
                        continue;
                    }
                    let a = s.max(&line_start) - line_start;
                    let b = e.min(&(line_start + line_len)) - line_start;
                    let x0 = text_x + a as f32 * cell.x;
                    let x1 = text_x + b.max(a + 1) as f32 * cell.x;
                    let base = y + cell.y - 2.0;
                    let mut pts = Vec::new();
                    let mut x = x0;
                    let mut up = false;
                    while x < x1 {
                        pts.push(Pos2::new(x, if up { base - 2.0 } else { base }));
                        x += 2.0;
                        up = !up;
                    }
                    pts.push(Pos2::new(x1, base));
                    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2, theme.error)));
                }
                // Diagnostics.
                for sq in &self.squiggles {
                    if sq.end < line_start || sq.start > line_start + line_len {
                        continue;
                    }
                    let a = sq.start.max(line_start) - line_start;
                    let b = sq.end.min(line_start + line_len) - line_start;
                    let x0 = text_x + a as f32 * cell.x;
                    let x1 = text_x + b.max(a + 1) as f32 * cell.x;
                    let colour = match sq.level {
                        SquiggleLevel::Error => theme.error,
                        SquiggleLevel::Warning => theme.warning,
                        SquiggleLevel::Info => theme.info,
                        SquiggleLevel::Hint => theme.gutter_fg,
                    };
                    wave(&painter, x0, x1, y + cell.y - 2.0, colour);
                }
                // Cursors.
                if focused && blink_on {
                    for c in &cursors.list {
                        if c.head >= line_start && c.head <= line_start + line_len {
                            let x = text_x + (c.head - line_start) as f32 * cell.x;
                            painter.rect_filled(
                                Rect::from_min_size(Pos2::new(x - 1.0, y), Vec2::new(2.0, cell.y)),
                                CornerRadius::ZERO,
                                theme.cursor,
                            );
                        }
                    }
                }
            }
            if focused {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
            {
                let (l, c) = buf.char_to_line_col(cursors.primary().head);
                self.cursor_rect = Some(Rect::from_min_size(
                    Pos2::new(text_x + c as f32 * cell.x, origin.y + l as f32 * cell.y),
                    Vec2::new(2.0, cell.y),
                ));
            }
            // Keep the primary cursor in view after edits/moves.
            if self.scroll_to_cursor {
                self.scroll_to_cursor = false;
                let (l, c) = buf.char_to_line_col(cursors.primary().head);
                let r = Rect::from_min_size(
                    Pos2::new(
                        text_x + c as f32 * cell.x - cell.x,
                        origin.y + l as f32 * cell.y,
                    ),
                    Vec2::new(cell.x * 3.0, cell.y),
                );
                ui.scroll_to_rect(r, None);
            }
        });
        let _ = scroll_out;
        resp
    }

    // ---- editing ------------------------------------------------------------

    /// Insert `text` at every cursor, replacing selections.
    pub fn insert_at_cursors(
        &mut self,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        text: &str,
        kind: EditKind,
    ) {
        if self.read_only {
            return;
        }
        let before = cursors.snapshot();
        // Apply from the highest cursor down so earlier positions stay valid.
        let mut order: Vec<usize> = (0..cursors.list.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(cursors.list[i].range().0));
        let mut edits = Vec::new();
        let n = text.chars().count();
        let mut min_line = usize::MAX;
        for i in order {
            let c = cursors.list[i];
            let (s, e) = c.range();
            let removed = buf.slice(s, e);
            min_line = min_line.min(buf.char_to_line_col(s).0);
            edits.push((
                i,
                Edit {
                    pos: s,
                    removed,
                    inserted: text.to_string(),
                },
            ));
        }
        // Apply and move cursors; compute the shift for cursors after each edit.
        let mut applied: Vec<Edit> = Vec::new();
        for (i, e) in edits {
            let (s, _) = cursors.list[i].range();
            let removed_n = e.removed.chars().count();
            // Shift every cursor positioned after this edit.
            for (j, c) in cursors.list.iter_mut().enumerate() {
                if j == i {
                    continue;
                }
                if c.head > s {
                    c.head = c.head + n - removed_n.min(c.head - s);
                }
                if c.anchor > s {
                    c.anchor = c.anchor + n - removed_n.min(c.anchor - s);
                }
            }
            cursors.list[i] = Cursor::at(s + n);
            applied.push(e);
        }
        // Edits were built highest-first, which is the order they must be applied in.
        buf.apply(applied, kind, before, cursors.snapshot());
        cursors.normalise(buf.len_chars());
        if min_line != usize::MAX {
            self.highlighter.invalidate_from(min_line);
        }
        self.last_version = buf.version();
    }

    /// Delete the selection, or `count` chars before/after each cursor.
    fn delete_at_cursors(
        &mut self,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        forward: bool,
        word: bool,
    ) {
        if self.read_only {
            return;
        }
        let before = cursors.snapshot();
        let mut order: Vec<usize> = (0..cursors.list.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(cursors.list[i].range().0));
        let mut edits = Vec::new();
        let mut min_line = usize::MAX;
        for i in order {
            let c = cursors.list[i];
            let (s, e) = if c.has_selection() {
                c.range()
            } else if forward {
                let e = if word {
                    buf.next_word_boundary(c.head)
                } else {
                    (c.head + 1).min(buf.len_chars())
                };
                (c.head, e)
            } else {
                let s = if word {
                    buf.prev_word_boundary(c.head)
                } else {
                    c.head.saturating_sub(1)
                };
                (s, c.head)
            };
            if s == e {
                continue;
            }
            let removed = buf.slice(s, e);
            let removed_n = e - s;
            min_line = min_line.min(buf.char_to_line_col(s).0);
            for (j, cc) in cursors.list.iter_mut().enumerate() {
                if j == i {
                    continue;
                }
                if cc.head > s {
                    cc.head -= removed_n.min(cc.head - s);
                }
                if cc.anchor > s {
                    cc.anchor -= removed_n.min(cc.anchor - s);
                }
            }
            cursors.list[i] = Cursor::at(s);
            edits.push(Edit {
                pos: s,
                removed,
                inserted: String::new(),
            });
        }
        if edits.is_empty() {
            return;
        }
        buf.apply(edits, EditKind::Deleting, before, cursors.snapshot());
        cursors.normalise(buf.len_chars());
        if min_line != usize::MAX {
            self.highlighter.invalidate_from(min_line);
        }
        self.last_version = buf.version();
    }

    /// Replace the whole buffer text (used by replace-all and reload).
    pub fn replace_all_text(&mut self, buf: &mut Buffer, cursors: &mut Cursors, new_text: &str) {
        let old = buf.text();
        if old == new_text {
            return;
        }
        let before = cursors.snapshot();
        buf.apply(
            vec![Edit {
                pos: 0,
                removed: old,
                inserted: new_text.to_string(),
            }],
            EditKind::Other,
            before,
            vec![(0, 0)],
        );
        cursors.set_single(Cursor::at(0));
        self.highlighter.invalidate_all();
        self.last_version = buf.version();
    }

    /// Apply char-range edits (`start`, `end`, replacement), given
    /// last-first so earlier offsets stay valid, as one undo step; the
    /// cursor lands at `cursor` when given (a completion's final position),
    /// else stays where it was, shifted by the edits.
    pub fn apply_edits(
        &mut self,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        edits: &[(usize, usize, String)],
        cursor: Option<usize>,
    ) {
        if self.read_only || edits.is_empty() {
            return;
        }
        let before = cursors.snapshot();
        let mut min_line = usize::MAX;
        let mut list = Vec::with_capacity(edits.len());
        let mut head = cursors.primary().head;
        for (s, e, text) in edits {
            let s = (*s).min(buf.len_chars());
            let e = (*e).min(buf.len_chars()).max(s);
            min_line = min_line.min(buf.char_to_line_col(s).0);
            let n = text.chars().count();
            if head >= e {
                head = head + n - (e - s);
            } else if head > s {
                head = s + n;
            }
            list.push(Edit {
                pos: s,
                removed: buf.slice(s, e),
                inserted: text.clone(),
            });
        }
        let after = cursor.unwrap_or(head);
        buf.apply(list, EditKind::Other, before, vec![(after, after)]);
        cursors.set_single(Cursor::at(after.min(buf.len_chars())));
        if min_line != usize::MAX {
            self.highlighter.invalidate_from(min_line);
        }
        self.last_version = buf.version();
        self.scroll_to_cursor = true;
    }

    fn lines_of_selection(buf: &Buffer, c: &Cursor) -> (usize, usize) {
        let (s, e) = c.range();
        let (ls, _) = buf.char_to_line_col(s);
        let (mut le, col) = buf.char_to_line_col(e);
        if le > ls && col == 0 {
            le -= 1;
        }
        (ls, le)
    }

    /// Apply a per-line transform over the selected lines of the primary cursor.
    fn transform_lines(
        &mut self,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        f: impl Fn(&str) -> String,
    ) {
        if self.read_only {
            return;
        }
        let c = cursors.primary();
        let (ls, le) = Self::lines_of_selection(buf, &c);
        let start = buf.line_start(ls);
        let end = buf.line_start(le) + buf.line_len(le);
        let old = buf.slice(start, end);
        let new: Vec<String> = old.split('\n').map(f).collect();
        let new = new.join("\n");
        if new == old {
            return;
        }
        let before = cursors.snapshot();
        buf.apply(
            vec![Edit {
                pos: start,
                removed: old,
                inserted: new.clone(),
            }],
            EditKind::Other,
            before,
            vec![],
        );
        let new_end = start + new.chars().count();
        cursors.set_single(Cursor {
            head: new_end,
            anchor: start,
            goal_col: None,
        });
        self.highlighter.invalidate_from(ls);
        self.last_version = buf.version();
    }

    fn toggle_comment(&mut self, buf: &mut Buffer, cursors: &mut Cursors) {
        let Some(token) = self.language.line_comment else {
            return;
        };
        let c = cursors.primary();
        let (ls, le) = Self::lines_of_selection(buf, &c);
        let all_commented = (ls..=le).all(|l| {
            let t = buf.line_text(l);
            t.trim().is_empty() || t.trim_start().starts_with(token)
        });
        let token = token.to_string();
        self.transform_lines(buf, cursors, |line| {
            if line.trim().is_empty() {
                return line.to_string();
            }
            let indent_len = line.len() - line.trim_start().len();
            let (indent, rest) = line.split_at(indent_len);
            if all_commented {
                let rest = rest.strip_prefix(token.as_str()).unwrap_or(rest);
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                format!("{indent}{rest}")
            } else {
                format!("{indent}{token} {rest}")
            }
        });
    }

    fn indent_unit(&self) -> String {
        if self.tab_inserts_spaces {
            " ".repeat(self.language.indent.max(1))
        } else {
            "\t".into()
        }
    }

    fn handle_keys(
        &mut self,
        ui: &mut egui::Ui,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        visible_rows: usize,
        resp: &mut EditorResponse,
    ) -> bool {
        let events = ui.input(|i| i.events.clone());
        let mut changed = false;
        for ev in events {
            match ev {
                egui::Event::Text(t) => {
                    if ui.input(|i| i.modifiers.alt) {
                        continue;
                    }
                    resp.typed = Some(t.clone());
                    let open = match t.as_str() {
                        "(" => Some(")"),
                        "[" => Some("]"),
                        "{" => Some("}"),
                        _ => None,
                    };
                    let closers = [")", "]", "}"];
                    // Overtype a closing bracket that is already there.
                    if closers.contains(&t.as_str())
                        && cursors.list.len() == 1
                        && !cursors.primary().has_selection()
                        && buf.char_at(cursors.primary().head).map(|c| c.to_string())
                            == Some(t.clone())
                    {
                        cursors.move_all(buf, Move::Right, false);
                        changed = true;
                        continue;
                    }
                    if let Some(close) = open
                        .filter(|_| cursors.list.len() == 1 && !cursors.primary().has_selection())
                    {
                        let next = buf.char_at(cursors.primary().head);
                        if next.is_none_or(|c| {
                            c.is_whitespace() || closers.iter().any(|x| x.starts_with(c))
                        }) {
                            self.insert_at_cursors(
                                buf,
                                cursors,
                                &format!("{t}{close}"),
                                EditKind::Typing,
                            );
                            cursors.move_all(buf, Move::Left, false);
                            changed = true;
                            continue;
                        }
                    }
                    self.insert_at_cursors(buf, cursors, &t, EditKind::Typing);
                    changed = true;
                }
                egui::Event::Paste(text) => {
                    let text = text.replace("\r\n", "\n");
                    self.insert_at_cursors(buf, cursors, &text, EditKind::Other);
                    changed = true;
                }
                egui::Event::Copy | egui::Event::Cut => {
                    let text = selected_text(buf, cursors);
                    if !text.is_empty() {
                        ui.ctx().copy_text(text.clone());
                        resp.copied = Some(text);
                    }
                    if matches!(ev, egui::Event::Cut) && cursors.has_selection() {
                        self.delete_at_cursors(buf, cursors, false, false);
                        changed = true;
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers: m,
                    ..
                } => {
                    use egui::Key as K;
                    let ctrl = m.command;
                    let shift = m.shift;
                    let alt = m.alt;
                    if self.popup_open
                        && !ctrl
                        && !alt
                        && matches!(
                            key,
                            K::ArrowUp
                                | K::ArrowDown
                                | K::PageUp
                                | K::PageDown
                                | K::Enter
                                | K::Tab
                                | K::Escape
                        )
                    {
                        resp.popup_keys.push(key);
                        continue;
                    }
                    match key {
                        K::ArrowLeft => cursors.move_all(
                            buf,
                            if ctrl { Move::WordLeft } else { Move::Left },
                            shift,
                        ),
                        K::ArrowRight => cursors.move_all(
                            buf,
                            if ctrl { Move::WordRight } else { Move::Right },
                            shift,
                        ),
                        K::ArrowUp if ctrl && alt => cursors.add_cursor_vertical(buf, false),
                        K::ArrowDown if ctrl && alt => cursors.add_cursor_vertical(buf, true),
                        K::ArrowUp if alt => {
                            self.move_lines(buf, cursors, -1);
                            changed = true;
                        }
                        K::ArrowDown if alt && !shift => {
                            self.move_lines(buf, cursors, 1);
                            changed = true;
                        }
                        K::ArrowDown if alt && shift => {
                            self.duplicate_lines(buf, cursors);
                            changed = true;
                        }
                        K::ArrowUp => cursors.move_all(buf, Move::Up, shift),
                        K::ArrowDown => cursors.move_all(buf, Move::Down, shift),
                        K::Home => cursors.move_all(
                            buf,
                            if ctrl {
                                Move::DocStart
                            } else {
                                Move::LineStart
                            },
                            shift,
                        ),
                        K::End => cursors.move_all(
                            buf,
                            if ctrl { Move::DocEnd } else { Move::LineEnd },
                            shift,
                        ),
                        K::PageUp => cursors.move_all(buf, Move::PageUp(visible_rows), shift),
                        K::PageDown => cursors.move_all(buf, Move::PageDown(visible_rows), shift),
                        K::Backspace => {
                            self.delete_at_cursors(buf, cursors, false, ctrl);
                            changed = true;
                        }
                        K::Delete => {
                            self.delete_at_cursors(buf, cursors, true, ctrl);
                            changed = true;
                        }
                        K::Enter if !ctrl => {
                            // Auto-indent: copy the current line's indentation; extra level after an opener.
                            let (line, col) = buf.char_to_line_col(cursors.primary().head);
                            let indent = buf.indent_of(line);
                            let before_cursor =
                                buf.line_text(line).chars().take(col).collect::<String>();
                            let opener = before_cursor.trim_end().ends_with(['{', '(', '[', ':']);
                            let mut text = format!("\n{indent}");
                            if opener {
                                text.push_str(&self.indent_unit());
                            }
                            self.insert_at_cursors(buf, cursors, &text, EditKind::Typing);
                            changed = true;
                        }
                        K::Tab => {
                            if shift {
                                let unit = self.indent_unit();
                                self.transform_lines(buf, cursors, |l| {
                                    l.strip_prefix(unit.as_str())
                                        .or_else(|| l.strip_prefix('\t'))
                                        .unwrap_or(l)
                                        .to_string()
                                });
                                changed = true;
                            } else if cursors.has_selection()
                                && selected_text(buf, cursors).contains('\n')
                            {
                                let unit = self.indent_unit();
                                self.transform_lines(buf, cursors, |l| {
                                    if l.is_empty() {
                                        String::new()
                                    } else {
                                        format!("{unit}{l}")
                                    }
                                });
                                changed = true;
                            } else {
                                let unit = if self.tab_inserts_spaces {
                                    let (_, col) = buf.char_to_line_col(cursors.primary().head);
                                    let w = self.language.indent.max(1);
                                    " ".repeat(w - (col % w))
                                } else {
                                    "\t".into()
                                };
                                self.insert_at_cursors(buf, cursors, &unit, EditKind::Typing);
                                changed = true;
                            }
                        }
                        K::Escape => {
                            if self.find.open || self.goto_open {
                                self.find.open = false;
                                self.goto_open = false;
                            } else {
                                cursors.collapse_to_primary();
                            }
                        }
                        K::A if ctrl => cursors.set_single(Cursor {
                            head: buf.len_chars(),
                            anchor: 0,
                            goal_col: None,
                        }),
                        K::Z if ctrl && !shift => {
                            if let Some(snap) = buf.undo() {
                                cursors.restore(&snap, buf.len_chars());
                                self.highlighter.invalidate_all();
                                changed = true;
                            }
                        }
                        K::Y if ctrl => {
                            if let Some(snap) = buf.redo() {
                                cursors.restore(&snap, buf.len_chars());
                                self.highlighter.invalidate_all();
                                changed = true;
                            }
                        }
                        K::Z if ctrl && shift => {
                            if let Some(snap) = buf.redo() {
                                cursors.restore(&snap, buf.len_chars());
                                self.highlighter.invalidate_all();
                                changed = true;
                            }
                        }
                        K::D if ctrl => cursors.select_next_occurrence(buf),
                        K::F3 if alt => cursors.select_all_occurrences(buf),
                        K::K if ctrl && shift => {
                            self.delete_lines(buf, cursors);
                            changed = true;
                        }
                        K::Slash if ctrl => {
                            self.toggle_comment(buf, cursors);
                            changed = true;
                        }
                        K::F if ctrl => self.open_find(false, Some(selected_text(buf, cursors))),
                        K::H if ctrl => self.open_find(true, Some(selected_text(buf, cursors))),
                        K::G if ctrl => {
                            self.goto_open = true;
                            self.goto_text.clear();
                        }
                        K::F3 => self.find_step(buf, cursors, !shift),
                        _ => {}
                    }
                    if matches!(
                        key,
                        K::ArrowLeft
                            | K::ArrowRight
                            | K::ArrowUp
                            | K::ArrowDown
                            | K::Home
                            | K::End
                            | K::PageUp
                            | K::PageDown
                    ) {
                        buf.break_undo_group();
                        self.scroll_to_cursor = true;
                        self.blink_start = ui.input(|i| i.time);
                    }
                }
                _ => {}
            }
        }
        changed
    }

    fn move_lines(&mut self, buf: &mut Buffer, cursors: &mut Cursors, dir: i32) {
        if self.read_only {
            return;
        }
        let c = cursors.primary();
        let (ls, le) = Self::lines_of_selection(buf, &c);
        let total = buf.len_lines();
        if (dir < 0 && ls == 0) || (dir > 0 && le + 1 >= total) {
            return;
        }
        let (block_s, block_e) = (buf.line_start(ls), buf.line_start(le) + buf.line_len(le));
        let block = buf.slice(block_s, block_e);
        let (other_line, new_first_line) = if dir < 0 {
            (ls - 1, ls - 1)
        } else {
            (le + 1, ls + 1)
        };
        let other = buf.line_text(other_line);
        let (region_s, region_e) = if dir < 0 {
            (buf.line_start(other_line), block_e)
        } else {
            (
                block_s,
                buf.line_start(other_line) + buf.line_len(other_line),
            )
        };
        let new = if dir < 0 {
            format!("{block}\n{other}")
        } else {
            format!("{other}\n{block}")
        };
        let before = cursors.snapshot();
        let old = buf.slice(region_s, region_e);
        buf.apply(
            vec![Edit {
                pos: region_s,
                removed: old,
                inserted: new,
            }],
            EditKind::Other,
            before,
            vec![],
        );
        let head_off = c.head - block_s;
        let anchor_off = c.anchor - block_s;
        let new_block_start = buf.line_start(new_first_line);
        cursors.set_single(Cursor {
            head: new_block_start + head_off,
            anchor: new_block_start + anchor_off,
            goal_col: None,
        });
        self.highlighter.invalidate_from(ls.min(other_line));
        self.last_version = buf.version();
    }

    fn duplicate_lines(&mut self, buf: &mut Buffer, cursors: &mut Cursors) {
        if self.read_only {
            return;
        }
        let c = cursors.primary();
        let (ls, le) = Self::lines_of_selection(buf, &c);
        let s = buf.line_start(ls);
        let e = buf.line_start(le) + buf.line_len(le);
        let block = buf.slice(s, e);
        let before = cursors.snapshot();
        buf.apply(
            vec![Edit {
                pos: e,
                removed: String::new(),
                inserted: format!("\n{block}"),
            }],
            EditKind::Other,
            before,
            vec![],
        );
        let shift = block.chars().count() + 1;
        cursors.set_single(Cursor {
            head: c.head + shift,
            anchor: c.anchor + shift,
            goal_col: None,
        });
        self.highlighter.invalidate_from(ls);
        self.last_version = buf.version();
    }

    fn delete_lines(&mut self, buf: &mut Buffer, cursors: &mut Cursors) {
        if self.read_only {
            return;
        }
        let c = cursors.primary();
        let (ls, le) = Self::lines_of_selection(buf, &c);
        let s = buf.line_start(ls);
        let e = if le + 1 < buf.len_lines() {
            buf.line_start(le + 1)
        } else {
            buf.len_chars()
        };
        let s = if e == buf.len_chars() && s > 0 {
            s - 1
        } else {
            s
        };
        let before = cursors.snapshot();
        let removed = buf.slice(s, e);
        buf.apply(
            vec![Edit {
                pos: s,
                removed,
                inserted: String::new(),
            }],
            EditKind::Other,
            before,
            vec![(s, s)],
        );
        cursors.set_single(Cursor::at(s.min(buf.len_chars())));
        self.highlighter.invalidate_from(ls.saturating_sub(1));
        self.last_version = buf.version();
    }

    // ---- find / replace / go to ---------------------------------------------

    fn recompute_matches(&mut self, buf: &Buffer) {
        let key = (
            self.find.query.clone(),
            self.find.regex,
            self.find.case_sensitive,
            self.find.whole_word,
            buf.version(),
        );
        if key == self.find.last_key {
            return;
        }
        self.find.last_key = key;
        self.find.matches.clear();
        if self.find.query.is_empty() {
            return;
        }
        let text = buf.text();
        let pattern = if self.find.regex {
            self.find.query.clone()
        } else {
            regex::escape(&self.find.query)
        };
        let pattern = if self.find.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        let pattern = if self.find.case_sensitive {
            pattern
        } else {
            format!("(?i){pattern}")
        };
        let Ok(re) = regex::Regex::new(&pattern) else {
            return;
        };
        for m in re.find_iter(&text).take(20_000) {
            if m.start() == m.end() {
                continue;
            }
            self.find
                .matches
                .push((byte_to_char(&text, m.start()), byte_to_char(&text, m.end())));
        }
        if self.find.current >= self.find.matches.len() {
            self.find.current = 0;
        }
    }

    fn find_step(&mut self, buf: &Buffer, cursors: &mut Cursors, forward: bool) {
        self.recompute_matches(buf);
        if self.find.matches.is_empty() {
            return;
        }
        let head = cursors.primary().head;
        let n = self.find.matches.len();
        let idx = if forward {
            self.find
                .matches
                .iter()
                .position(|(s, _)| *s >= head.max(cursors.primary().range().1))
                .unwrap_or(0)
        } else {
            self.find
                .matches
                .iter()
                .rposition(|(s, _)| *s < cursors.primary().range().0)
                .unwrap_or(n - 1)
        };
        self.find.current = idx;
        let (s, e) = self.find.matches[idx];
        cursors.set_single(Cursor {
            head: e,
            anchor: s,
            goal_col: None,
        });
        self.scroll_to_cursor = true;
    }

    fn find_bar(
        &mut self,
        ui: &mut egui::Ui,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        theme: &EditorTheme,
        resp: &mut EditorResponse,
    ) {
        if !self.find.open {
            return;
        }
        self.recompute_matches(buf);
        let mut step: Option<bool> = None;
        let mut replace_one = false;
        let mut replace_all = false;
        egui::Frame::new()
            .fill(theme.gutter_bg)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.find.query)
                            .desired_width(220.0)
                            .hint_text("Find"),
                    );
                    if self.find.focus_query {
                        r.request_focus();
                        self.find.focus_query = false;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        step = Some(!ui.input(|i| i.modifiers.shift));
                        r.request_focus();
                    }
                    ui.toggle_value(&mut self.find.case_sensitive, "Aa")
                        .tip("Match Case");
                    ui.toggle_value(&mut self.find.whole_word, "ab")
                        .tip("Whole Word");
                    ui.toggle_value(&mut self.find.regex, ".*")
                        .tip("Regular Expression");
                    let n = self.find.matches.len();
                    let label = if n == 0 {
                        "No results".to_string()
                    } else {
                        format!("{} of {n}", (self.find.current + 1).min(n))
                    };
                    ui.label(egui::RichText::new(label).weak());
                    if ui.small_button("▲").tip("Previous (Shift+F3)").clicked() {
                        step = Some(false);
                    }
                    if ui.small_button("▼").tip("Next (F3)").clicked() {
                        step = Some(true);
                    }
                    if ui.small_button("✕").clicked() {
                        self.find.open = false;
                    }
                });
                if self.find.replace_open && !self.read_only {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.find.replacement)
                                .desired_width(220.0)
                                .hint_text("Replace"),
                        );
                        if ui.button("Replace").clicked() {
                            replace_one = true;
                        }
                        if ui.button("Replace All").clicked() {
                            replace_all = true;
                        }
                    });
                }
            });
        if let Some(forward) = step {
            self.find_step(buf, cursors, forward);
        }
        if replace_one && !self.find.matches.is_empty() {
            let (s, e) = self.find.matches[self.find.current.min(self.find.matches.len() - 1)];
            cursors.set_single(Cursor {
                head: e,
                anchor: s,
                goal_col: None,
            });
            let rep = self.find.replacement.clone();
            self.insert_at_cursors(buf, cursors, &rep, EditKind::Other);
            resp.changed = true;
            self.find_step(buf, cursors, true);
        }
        if replace_all && !self.find.matches.is_empty() {
            let text = buf.text();
            let mut out = String::with_capacity(text.len());
            let mut last = 0usize;
            for &(s, e) in &self.find.matches {
                let (bs, be) = (char_to_byte(&text, s), char_to_byte(&text, e));
                out.push_str(&text[last..bs]);
                out.push_str(&self.find.replacement);
                last = be;
            }
            out.push_str(&text[last..]);
            self.replace_all_text(buf, cursors, &out);
            resp.changed = true;
        }
    }

    fn goto_bar(&mut self, ui: &mut egui::Ui, buf: &Buffer, cursors: &mut Cursors) {
        if !self.goto_open {
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Go to Line");
            let r = ui.add(egui::TextEdit::singleline(&mut self.goto_text).desired_width(80.0));
            if self.goto_text.is_empty() {
                r.request_focus();
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let mut parts = self.goto_text.split(':');
                let line: usize = parts
                    .next()
                    .and_then(|p| p.trim().parse().ok())
                    .unwrap_or(1);
                let col: usize = parts
                    .next()
                    .and_then(|p| p.trim().parse().ok())
                    .unwrap_or(1);
                let pos = buf.line_col_to_char(line.saturating_sub(1), col.saturating_sub(1));
                cursors.set_single(Cursor::at(pos));
                self.goto_open = false;
                self.scroll_to_cursor = true;
            }
            if ui.small_button("✕").clicked() {
                self.goto_open = false;
            }
        });
    }
}

/// A wavy underline from `x0` to `x1` with its baseline at `base`.
fn wave(painter: &egui::Painter, x0: f32, x1: f32, base: f32, colour: Color32) {
    let mut pts = Vec::new();
    let mut x = x0;
    let mut up = false;
    while x < x1 {
        pts.push(Pos2::new(x, if up { base - 2.0 } else { base }));
        x += 2.0;
        up = !up;
    }
    pts.push(Pos2::new(x1, base));
    painter.add(egui::Shape::line(pts, egui::Stroke::new(1.2, colour)));
}

/// Selected text across cursors, joined with newlines.
pub fn selected_text(buf: &Buffer, cursors: &Cursors) -> String {
    let mut parts: Vec<String> = cursors
        .list
        .iter()
        .filter(|c| c.has_selection())
        .map(|c| {
            let (s, e) = c.range();
            buf.slice(s, e)
        })
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    if parts.len() == 1 {
        return parts.remove(0);
    }
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages;

    fn view() -> EditorView {
        EditorView::new(egui::Id::new("t"), languages::detect("a.rs"), true, 13.0)
    }

    #[test]
    fn multi_cursor_insert_and_delete_keep_positions_consistent() {
        let mut buf = Buffer::from_text("ab\ncd\nef");
        let mut cursors = Cursors {
            list: vec![Cursor::at(0), Cursor::at(3), Cursor::at(6)],
        };
        let mut v = view();
        v.insert_at_cursors(&mut buf, &mut cursors, "X", EditKind::Typing);
        assert_eq!(buf.text(), "Xab\nXcd\nXef");
        assert_eq!(cursors.snapshot(), vec![(1, 1), (5, 5), (9, 9)]);
        v.delete_at_cursors(&mut buf, &mut cursors, false, false);
        assert_eq!(buf.text(), "ab\ncd\nef");
        assert_eq!(cursors.snapshot(), vec![(0, 0), (3, 3), (6, 6)]);
    }

    #[test]
    fn comment_toggle_and_line_moves() {
        let mut buf = Buffer::from_text("a\n    b\nc");
        let mut cursors = Cursors::single(0);
        let mut v = view();
        cursors.set_single(Cursor {
            head: buf.len_chars(),
            anchor: 0,
            goal_col: None,
        });
        v.toggle_comment(&mut buf, &mut cursors);
        assert_eq!(buf.text(), "// a\n    // b\n// c");
        v.toggle_comment(&mut buf, &mut cursors);
        assert_eq!(buf.text(), "a\n    b\nc");
        cursors.set_single(Cursor::at(0));
        v.move_lines(&mut buf, &mut cursors, 1);
        assert_eq!(buf.text(), "    b\na\nc");
        assert_eq!(buf.char_to_line_col(cursors.primary().head).0, 1);
        v.duplicate_lines(&mut buf, &mut cursors);
        assert_eq!(buf.text(), "    b\na\na\nc");
        v.delete_lines(&mut buf, &mut cursors);
        assert_eq!(buf.text(), "    b\na\nc");
    }

    #[test]
    fn find_matches_and_replace_all() {
        let mut buf = Buffer::from_text("foo Foo foobar");
        let mut cursors = Cursors::single(0);
        let mut v = view();
        v.find.open = true;
        v.find.query = "foo".into();
        v.recompute_matches(&buf);
        assert_eq!(v.find.matches.len(), 3);
        v.find.case_sensitive = true;
        v.recompute_matches(&buf);
        assert_eq!(v.find.matches.len(), 2);
        v.find.whole_word = true;
        v.recompute_matches(&buf);
        assert_eq!(v.find.matches, vec![(0, 3)]);
        v.find.whole_word = false;
        v.find.case_sensitive = false;
        v.find.replacement = "x".into();
        v.recompute_matches(&buf);
        let text = buf.text();
        let mut out = String::new();
        let mut last = 0;
        for &(s, e) in &v.find.matches {
            out.push_str(&text[char_to_byte(&text, last)..char_to_byte(&text, s)]);
            out.push_str(&v.find.replacement);
            last = e;
        }
        out.push_str(&text[char_to_byte(&text, last)..]);
        v.replace_all_text(&mut buf, &mut cursors, &out);
        assert_eq!(buf.text(), "x x xbar");
        assert!(buf.undo().is_some());
        assert_eq!(buf.text(), "foo Foo foobar");
    }
}
