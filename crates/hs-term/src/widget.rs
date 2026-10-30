//! The egui terminal view: draws the grid, handles scrolling, selection and
//! keyboard input for a [`Session`].

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Vec2};

use crate::keys;
use crate::links::{self, LinkTarget};
use crate::search::SearchMatch;
use crate::session::Session;

/// Default xterm-ish palette (index 0..=15); 16..=255 are computed.
pub const ANSI_DARK: [Color32; 16] = [
    Color32::from_rgb(0x1E, 0x1E, 0x24),
    Color32::from_rgb(0xE5, 0x4D, 0x4D),
    Color32::from_rgb(0x3C, 0xB3, 0x71),
    Color32::from_rgb(0xD9, 0xB0, 0x2A),
    Color32::from_rgb(0x3F, 0x9C, 0xEE),
    Color32::from_rgb(0xB0, 0x6A, 0xE0),
    Color32::from_rgb(0x2E, 0xC4, 0xB6),
    Color32::from_rgb(0xD0, 0xD0, 0xD6),
    Color32::from_rgb(0x6A, 0x6A, 0x76),
    Color32::from_rgb(0xFF, 0x6E, 0x6E),
    Color32::from_rgb(0x5F, 0xD7, 0x8F),
    Color32::from_rgb(0xF0, 0xCF, 0x5A),
    Color32::from_rgb(0x6F, 0xB7, 0xFF),
    Color32::from_rgb(0xD0, 0x8C, 0xFF),
    Color32::from_rgb(0x5A, 0xE0, 0xD6),
    Color32::from_rgb(0xF2, 0xF2, 0xF5),
];

pub fn default_palette_rgb(index: usize) -> Rgb {
    let c = palette_colour(index as u8);
    Rgb {
        r: c.r(),
        g: c.g(),
        b: c.b(),
    }
}

fn palette_colour(i: u8) -> Color32 {
    match i {
        0..=15 => ANSI_DARK[i as usize],
        16..=231 => {
            let i = i - 16;
            let r = i / 36;
            let g = (i % 36) / 6;
            let b = i % 6;
            let f = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Color32::from_rgb(f(r), f(g), f(b))
        }
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            Color32::from_rgb(v, v, v)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TermTheme {
    pub background: Color32,
    pub foreground: Color32,
    pub cursor: Color32,
    pub selection: Color32,
    pub palette: [Color32; 16],
}

impl TermTheme {
    /// Follow the app colour scheme: background, foreground, cursor and
    /// selection from the theme, ANSI palette by light/dark.
    pub fn for_theme(theme: &hs_ui::theme::Theme) -> Self {
        let mut t = if theme.dark {
            Self::dark()
        } else {
            Self::light()
        };
        t.background = theme.window_bg;
        t.foreground = theme.text;
        t.cursor = theme.text;
        // `linear_multiply` scales the alpha *and* the colour, which left
        // a selection over a dark background almost invisible. Keep the
        // accent at full strength and set the alpha on its own.
        t.selection = Color32::from_rgba_unmultiplied(
            theme.accent.r(),
            theme.accent.g(),
            theme.accent.b(),
            if theme.dark { 140 } else { 105 },
        );
        t
    }

    pub fn dark() -> Self {
        Self {
            background: Color32::from_rgb(0x15, 0x15, 0x1A),
            foreground: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            cursor: Color32::from_rgb(0xE6, 0xE6, 0xEA),
            selection: Color32::from_rgba_unmultiplied(0x9A, 0x4D, 0xF0, 140),
            palette: ANSI_DARK,
        }
    }
    pub fn light() -> Self {
        let mut p = ANSI_DARK;
        p[0] = Color32::from_rgb(0x1E, 0x1E, 0x24);
        p[7] = Color32::from_rgb(0x55, 0x55, 0x60);
        p[15] = Color32::from_rgb(0x10, 0x10, 0x14);
        Self {
            background: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            foreground: Color32::from_rgb(0x1E, 0x1E, 0x24),
            cursor: Color32::from_rgb(0x1E, 0x1E, 0x24),
            selection: Color32::from_rgba_unmultiplied(0x72, 0x00, 0xCA, 105),
            palette: p,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TerminalResponse {
    pub focused: bool,
    pub copied: Option<String>,
    pub clicked: bool,
    /// Ctrl+click on a URL or `path:line:col`.
    pub link: Option<LinkTarget>,
    /// Ctrl+Shift+F while focused (the tool opens its search bar).
    pub find_requested: bool,
    /// The bytes written to the session this frame (keys and pastes), for
    /// an owner that mirrors them elsewhere (the SSH tool's broadcast).
    pub input: Vec<u8>,
}

/// Per-view state (font size, selection drag) kept by the owner.
pub struct TerminalView {
    pub font_size: f32,
    pub copy_on_select: bool,
    /// PuTTY convention: right-click copies the selection, or pastes when nothing is selected.
    pub right_click_copy_paste: bool,
    pub id: egui::Id,
    /// Ctrl+hover underlines links and Ctrl+click opens them.
    pub link_detection: bool,
    /// Search hits to highlight (set by the owner) and the current one.
    pub search_matches: Vec<SearchMatch>,
    pub search_current: Option<usize>,
    dragging: bool,
    last_cell: Vec2,
}

impl TerminalView {
    pub fn new(id: egui::Id, font_size: f32) -> Self {
        Self {
            font_size,
            copy_on_select: false,
            right_click_copy_paste: true,
            id,
            link_detection: true,
            search_matches: Vec::new(),
            search_current: None,
            dragging: false,
            last_cell: Vec2::new(8.0, 16.0),
        }
    }

    pub fn cell_size(&self, ui: &egui::Ui) -> Vec2 {
        let font = egui::FontId::new(self.font_size, hs_ui::fonts::terminal_family());
        ui.fonts_mut(|f| {
            let w = f.glyph_width(&font, 'M');
            let h = f.row_height(&font);
            Vec2::new(w.max(1.0), (h * 1.15).max(1.0))
        })
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        session: &mut Session,
        theme: &TermTheme,
    ) -> TerminalResponse {
        let mut resp = TerminalResponse::default();
        let rect = ui.available_rect_before_wrap();
        // Claim that space. A terminal fills whatever it is given and used
        // to allocate nothing, which is invisible inside a central panel
        // but collapses any container that sizes itself to its content -
        // the editor's terminal panel and the bottom dock both shrank to
        // their minimum height. Owner's report, 2026-10-17.
        ui.allocate_rect(rect, Sense::hover());
        let cell = self.cell_size(ui);
        self.last_cell = cell;
        let cols = ((rect.width() - 8.0) / cell.x).floor().max(2.0) as u16;
        let rows = ((rect.height() - 4.0) / cell.y).floor().max(1.0) as u16;
        session.resize(cols, rows);

        let response = ui.interact(rect, self.id, Sense::click_and_drag());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, session.title.clone())
        });
        if response.clicked() {
            ui.memory_mut(|m| m.request_focus(self.id));
            resp.clicked = true;
        }
        let focused = ui.memory(|m| m.has_focus(self.id));
        resp.focused = focused;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, theme.background);
        let origin = Pos2::new(rect.left() + 4.0, rect.top() + 2.0);

        // Mouse: wheel scroll, drag selection.
        let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
        if response.hovered() && scroll_delta != 0.0 {
            let lines = (scroll_delta / cell.y).round() as i32;
            if lines != 0 {
                session.term.lock().scroll_display(Scroll::Delta(lines));
            }
        }
        let to_point = |pos: Pos2, display_offset: usize| -> (Point, Side) {
            let col = ((pos.x - origin.x) / cell.x).floor().max(0.0) as usize;
            let row = ((pos.y - origin.y) / cell.y).floor().max(0.0) as i32;
            let side = if (pos.x - origin.x) % cell.x > cell.x / 2.0 {
                Side::Right
            } else {
                Side::Left
            };
            let line = Line(row - display_offset as i32);
            (Point::new(line, Column(col.min(cols as usize - 1))), side)
        };
        if response.drag_started() {
            self.dragging = response.interact_pointer_pos().is_some();
        }
        if (response.drag_started() || response.dragged()) && self.dragging {
            // `drag_started` only fires once the pointer has passed egui's
            // threshold, and `interact_pointer_pos` is by then a few pixels
            // along: anchoring there loses the character the user pressed
            // on. Anchor at the press itself, and keep that cell inside the
            // selection by taking the anchor's side from the drag direction
            // (the moving end keeps the half-cell rule, which is what
            // picking a character with the pointer feels like).
            let press = ui.input(|i| i.pointer.press_origin());
            if let (Some(press), Some(pos)) = (press, response.interact_pointer_pos()) {
                let mut t = session.term.lock();
                let off = t.grid().display_offset();
                let (anchor, _) = to_point(press, off);
                let (head, head_side) = to_point(pos, off);
                let forward = (head.line, head.column) >= (anchor.line, anchor.column);
                let anchor_side = if forward { Side::Left } else { Side::Right };
                let ty = if ui.input(|i| i.modifiers.alt) {
                    SelectionType::Block
                } else {
                    SelectionType::Simple
                };
                let mut sel = Selection::new(ty, anchor, anchor_side);
                sel.update(head, head_side);
                t.selection = Some(sel);
            }
        } else if response.drag_stopped() && self.dragging {
            self.dragging = false;
            if self.copy_on_select
                && let Some(text) = session.term.lock().selection_to_string()
            {
                ui.ctx().copy_text(text.clone());
                resp.copied = Some(text);
            }
        }
        if response.double_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let mut t = session.term.lock();
            let off = t.grid().display_offset();
            let (p, side) = to_point(pos, off);
            t.selection = Some(Selection::new(SelectionType::Semantic, p, side));
        }
        if response.triple_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let mut t = session.term.lock();
            let off = t.grid().display_offset();
            let (p, side) = to_point(pos, off);
            t.selection = Some(Selection::new(SelectionType::Lines, p, side));
        }
        // Ctrl+hover: find the link under the pointer; Ctrl+click opens it.
        let mut hovered_link: Option<(i32, usize, usize)> = None;
        let ctrl = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if self.link_detection
            && ctrl
            && let Some(pos) = response.hover_pos()
        {
            let t = session.term.lock();
            let off = t.grid().display_offset();
            let (p, _) = to_point(pos, off);
            let (text, map) = crate::search::line_text(&t, p.line);
            if let Some(ci) = map.iter().rposition(|&c| c <= p.column.0)
                && let Some(link) = links::link_at(&text, ci)
            {
                let start = map[link.start];
                let end = map.get(link.end).copied().unwrap_or(cols as usize);
                hovered_link = Some((p.line.0, start, end));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if response.clicked() {
                    resp.link = Some(link.target);
                }
            }
        }
        if response.clicked() && !response.drag_started() {
            session.term.lock().selection = None;
        }
        if self.right_click_copy_paste && response.secondary_clicked() {
            ui.memory_mut(|m| m.request_focus(self.id));
            let selected = session.term.lock().selection_to_string();
            match selected {
                Some(text) if !text.is_empty() => {
                    ui.ctx().copy_text(text.clone());
                    session.term.lock().selection = None;
                    resp.copied = Some(text);
                }
                _ => {
                    // Delivered as Event::Paste on the next frame and written to the shell.
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                }
            }
        }

        // Keyboard input while focused.
        if focused {
            let app_cursor = session.app_cursor();
            let bracketed = session.bracketed_paste();
            let mut to_write: Vec<u8> = Vec::new();
            let mut scroll_to_bottom = false;
            let events = ui.input(|i| i.events.clone());
            for ev in events {
                match ev {
                    egui::Event::Text(t) => {
                        // Alt+key is sent as ESC+key by the Key event; skip its text.
                        let alt = ui.input(|i| i.modifiers.alt);
                        if !alt {
                            to_write.extend_from_slice(t.as_bytes());
                            scroll_to_bottom = true;
                        }
                    }
                    egui::Event::Paste(text) => {
                        to_write.extend_from_slice(&keys::paste_bytes(&text, bracketed));
                        scroll_to_bottom = true;
                    }
                    egui::Event::Copy => {
                        if let Some(text) = session.term.lock().selection_to_string() {
                            ui.ctx().copy_text(text.clone());
                            resp.copied = Some(text);
                        }
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        let ctrl_shift = (modifiers.ctrl || modifiers.command) && modifiers.shift;
                        if ctrl_shift && key == egui::Key::C {
                            if let Some(text) = session.term.lock().selection_to_string() {
                                ui.ctx().copy_text(text.clone());
                                resp.copied = Some(text);
                            }
                            continue;
                        }
                        if ctrl_shift && key == egui::Key::F {
                            resp.find_requested = true;
                            continue;
                        }
                        if ctrl_shift && key == egui::Key::V {
                            // Paste arrives as Event::Paste on the next frame.
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                            continue;
                        }
                        if modifiers.shift && key == egui::Key::PageUp {
                            session.term.lock().scroll_display(Scroll::PageUp);
                            continue;
                        }
                        if modifiers.shift && key == egui::Key::PageDown {
                            session.term.lock().scroll_display(Scroll::PageDown);
                            continue;
                        }
                        if let Some(bytes) = keys::encode(key, modifiers, app_cursor) {
                            to_write.extend_from_slice(&bytes);
                            scroll_to_bottom = true;
                        }
                    }
                    _ => {}
                }
            }
            if !to_write.is_empty() {
                resp.input.extend_from_slice(&to_write);
                session.write(&to_write);
            }
            if scroll_to_bottom {
                session.term.lock().scroll_display(Scroll::Bottom);
            }
            // Keep Tab inside the terminal instead of moving egui focus.
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    self.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                )
            });
        }

        // Render.
        let font = egui::FontId::new(self.font_size, hs_ui::fonts::terminal_family());
        let t = session.term.lock();
        let content = t.renderable_content();
        let colors = content.colors;
        let resolve = |c: Color, is_fg: bool| -> Color32 {
            match c {
                Color::Spec(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
                Color::Indexed(i) => match colors[i as usize] {
                    Some(rgb) => Color32::from_rgb(rgb.r, rgb.g, rgb.b),
                    None if (i as usize) < 16 => theme.palette[i as usize],
                    None => palette_colour(i),
                },
                Color::Named(n) => {
                    let idx = n as usize;
                    if let Some(rgb) = colors[n] {
                        return Color32::from_rgb(rgb.r, rgb.g, rgb.b);
                    }
                    match n {
                        NamedColor::Foreground | NamedColor::BrightForeground => theme.foreground,
                        NamedColor::DimForeground => theme.foreground.linear_multiply(0.7),
                        NamedColor::Background => theme.background,
                        NamedColor::Cursor => theme.cursor,
                        _ if idx < 16 => theme.palette[idx],
                        _ if (16..24).contains(&idx) => {
                            theme.palette[idx - 16].linear_multiply(0.7)
                        } // Dim*
                        _ => {
                            if is_fg {
                                theme.foreground
                            } else {
                                theme.background
                            }
                        }
                    }
                }
            }
        };
        let selection = content.selection;
        let cursor = content.cursor;
        let show_cursor = content
            .mode
            .contains(alacritty_terminal::term::TermMode::SHOW_CURSOR);

        // Search hits on screen, as (line, start, end, is_current).
        let hit_bg = Color32::from_rgba_unmultiplied(0xD9, 0xB0, 0x2A, 110);
        let current_bg = Color32::from_rgba_unmultiplied(0xF0, 0x8A, 0x24, 200);
        let first_line = -(content.display_offset as i32);
        let last_line = first_line + rows as i32;
        let hits: Vec<(i32, usize, usize, bool)> = self
            .search_matches
            .iter()
            .enumerate()
            .filter(|(_, m)| m.line >= first_line && m.line < last_line)
            .map(|(i, m)| (m.line, m.start, m.end, Some(i) == self.search_current))
            .collect();

        // Group cells into runs of equal style per line.
        let mut run_text = String::new();
        let mut run_start: Option<(Pos2, Color32, Color32, Flags)> = None;
        let mut run_len = 0usize;
        let flush = |painter: &egui::Painter,
                     start: Option<(Pos2, Color32, Color32, Flags)>,
                     text: &str,
                     len: usize| {
            if let Some((pos, fg, bg, flags)) = start {
                let size = Vec2::new(len as f32 * cell.x, cell.y);
                if bg != theme.background {
                    painter.rect_filled(Rect::from_min_size(pos, size), CornerRadius::ZERO, bg);
                }
                if !text.trim().is_empty() && !flags.contains(Flags::HIDDEN) {
                    painter.text(
                        Pos2::new(pos.x, pos.y + (cell.y - cell.y / 1.15) / 2.0),
                        egui::Align2::LEFT_TOP,
                        text,
                        font.clone(),
                        fg,
                    );
                    if flags.intersects(Flags::UNDERLINE | Flags::DOUBLE_UNDERLINE) {
                        let y = pos.y + cell.y - 1.5;
                        painter.line_segment(
                            [Pos2::new(pos.x, y), Pos2::new(pos.x + size.x, y)],
                            egui::Stroke::new(1.0, fg),
                        );
                    }
                    if flags.contains(Flags::STRIKEOUT) {
                        let y = pos.y + cell.y / 2.0;
                        painter.line_segment(
                            [Pos2::new(pos.x, y), Pos2::new(pos.x + size.x, y)],
                            egui::Stroke::new(1.0, fg),
                        );
                    }
                }
            }
        };
        let mut current_line = Line(i32::MIN);
        for indexed in content.display_iter {
            let point = indexed.point;
            let c = &indexed.cell;
            if c.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let screen_line = point.line.0 + content.display_offset as i32;
            if point.line != current_line {
                flush(&painter, run_start, &run_text, run_len);
                run_text.clear();
                run_start = None;
                run_len = 0;
                current_line = point.line;
            }
            let mut fg = resolve(c.fg, true);
            let mut bg = resolve(c.bg, false);
            if c.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if c.flags.contains(Flags::DIM) {
                fg = fg.linear_multiply(0.6);
            }
            let selected = selection.as_ref().is_some_and(|s| s.contains(point));
            if selected {
                bg = blend(bg, theme.selection);
                // Keep the cell's own colour when it still reads against
                // the highlight, otherwise switch to black or white.
                fg = readable(fg, bg);
            } else if let Some(&(_, _, _, cur)) = hits
                .iter()
                .find(|h| h.0 == point.line.0 && (h.1..h.2).contains(&point.column.0))
            {
                bg = blend(bg, if cur { current_bg } else { hit_bg });
            }
            let style_flags = c.flags
                & (Flags::UNDERLINE
                    | Flags::DOUBLE_UNDERLINE
                    | Flags::STRIKEOUT
                    | Flags::HIDDEN
                    | Flags::BOLD);
            let pos = Pos2::new(
                origin.x + point.column.0 as f32 * cell.x,
                origin.y + screen_line as f32 * cell.y,
            );
            let same = matches!(run_start, Some((_, rfg, rbg, rfl)) if rfg == fg && rbg == bg && rfl == style_flags);
            if !same {
                flush(&painter, run_start, &run_text, run_len);
                run_text.clear();
                run_start = Some((pos, fg, bg, style_flags));
                run_len = 0;
            }
            run_text.push(if c.c == '\0' { ' ' } else { c.c });
            run_len += if c.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            if c.flags.contains(Flags::WIDE_CHAR) {
                // wide glyph occupies two cells; keep the run width honest
            }
        }
        flush(&painter, run_start, &run_text, run_len);

        // Cursor.
        if show_cursor && cursor.shape != CursorShape::Hidden {
            let screen_line = cursor.point.line.0 + content.display_offset as i32;
            if screen_line >= 0 && screen_line < rows as i32 {
                let pos = Pos2::new(
                    origin.x + cursor.point.column.0 as f32 * cell.x,
                    origin.y + screen_line as f32 * cell.y,
                );
                let colour = if focused {
                    theme.cursor
                } else {
                    theme.cursor.linear_multiply(0.5)
                };
                match cursor.shape {
                    CursorShape::Block => {
                        if focused {
                            painter.rect_filled(
                                Rect::from_min_size(pos, cell),
                                CornerRadius::ZERO,
                                colour,
                            );
                            // Redraw the glyph under the block in background colour.
                            let ch = t.grid()[cursor.point].c;
                            if ch != ' ' && ch != '\0' {
                                painter.text(
                                    Pos2::new(pos.x, pos.y + (cell.y - cell.y / 1.15) / 2.0),
                                    egui::Align2::LEFT_TOP,
                                    ch,
                                    font.clone(),
                                    theme.background,
                                );
                            }
                        } else {
                            painter.rect_stroke(
                                Rect::from_min_size(pos, cell),
                                CornerRadius::ZERO,
                                egui::Stroke::new(1.0, colour),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                    CursorShape::Underline => {
                        painter.rect_filled(
                            Rect::from_min_size(
                                Pos2::new(pos.x, pos.y + cell.y - 2.0),
                                Vec2::new(cell.x, 2.0),
                            ),
                            CornerRadius::ZERO,
                            colour,
                        );
                    }
                    CursorShape::Beam => {
                        painter.rect_filled(
                            Rect::from_min_size(pos, Vec2::new(2.0, cell.y)),
                            CornerRadius::ZERO,
                            colour,
                        );
                    }
                    CursorShape::HollowBlock | CursorShape::Hidden => {
                        painter.rect_stroke(
                            Rect::from_min_size(pos, cell),
                            CornerRadius::ZERO,
                            egui::Stroke::new(1.0, colour),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
        // Underline the Ctrl+hovered link.
        if let Some((line, start, end)) = hovered_link {
            let screen_line = line + content.display_offset as i32;
            let y = origin.y + (screen_line + 1) as f32 * cell.y - 1.0;
            painter.line_segment(
                [
                    Pos2::new(origin.x + start as f32 * cell.x, y),
                    Pos2::new(origin.x + end as f32 * cell.x, y),
                ],
                egui::Stroke::new(1.0, theme.foreground),
            );
        }
        // Scrollback indicator.
        if content.display_offset > 0 {
            let total = t.grid().history_size();
            let label = format!("↑ {} / {}", content.display_offset, total);
            let galley =
                painter.layout_no_wrap(label, egui::FontId::proportional(11.0), theme.foreground);
            let r = Rect::from_min_size(
                Pos2::new(rect.right() - galley.size().x - 16.0, rect.top() + 4.0),
                galley.size() + Vec2::new(8.0, 4.0),
            );
            painter.rect_filled(
                r,
                CornerRadius::same(4),
                theme.background.linear_multiply(0.9),
            );
            painter.galley(r.min + Vec2::new(4.0, 2.0), galley, theme.foreground);
        }
        drop(t);
        resp
    }
}

/// Relative luminance (WCAG 2.1), for contrast decisions.
fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

/// `fg` when it is legible on `bg`, else black or white, whichever is.
/// 3.0 is the WCAG AA floor for large text, which is what a terminal
/// cell at a readable font size is.
fn readable(fg: Color32, bg: Color32) -> Color32 {
    if contrast(fg, bg) >= 3.0 {
        return fg;
    }
    if contrast(Color32::WHITE, bg) >= contrast(Color32::BLACK, bg) {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// `over` composited on an opaque `base`. `Color32` keeps its channels
/// **premultiplied** by the alpha, so the overlay is added as it is and
/// only the base is faded. Multiplying `over` by alpha again (what this
/// did before) dimmed every highlight twice and left the selection and
/// the search hits barely visible.
fn blend(base: Color32, over: Color32) -> Color32 {
    let a = over.a() as f32 / 255.0;
    let ch = |b: u8, o: u8| ((b as f32) * (1.0 - a) + o as f32).min(255.0) as u8;
    Color32::from_rgb(
        ch(base.r(), over.r()),
        ch(base.g(), over.g()),
        ch(base.b(), over.b()),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn blend_treats_the_overlay_as_premultiplied() {
        let base = Color32::from_rgb(0x15, 0x15, 0x1A);
        let over = Color32::from_rgba_unmultiplied(0x9A, 0x4D, 0xF0, 140);
        let out = blend(base, over);
        // Half-opaque purple over near-black: about half the purple.
        assert!(out.r() > 0x50 && out.r() < 0x6A, "{out:?}");
        assert!(out.b() > 0x80, "{out:?}");
        // The ends stay exact.
        assert_eq!(blend(base, Color32::TRANSPARENT), base);
        let solid = Color32::from_rgb(0x20, 0x30, 0x40);
        assert_eq!(blend(base, solid), solid);
    }

    #[test]
    fn selection_is_visible_and_keeps_text_readable() {
        for dark in [true, false] {
            let app = if dark {
                hs_ui::theme::Theme::dark()
            } else {
                hs_ui::theme::Theme::light()
            };
            let t = TermTheme::for_theme(&app);
            let bg = blend(t.background, t.selection);
            // The highlight has to be obvious against the unselected
            // background: the old `linear_multiply` wash was not.
            assert!(
                contrast(bg, t.background) >= 1.35,
                "dark={dark}: selection barely differs from the background ({:?} vs {:?})",
                bg,
                t.background
            );
            // And the text on it stays legible, whatever colour the cell had.
            for fg in [
                t.foreground,
                t.palette[4],
                t.palette[0],
                Color32::from_rgb(0x30, 0x30, 0x40),
            ] {
                assert!(
                    contrast(readable(fg, bg), bg) >= 3.0,
                    "dark={dark}: {fg:?} is unreadable on the selection"
                );
            }
        }
    }

    use super::*;

    #[test]
    fn palette_indices_cover_the_cube_and_greys() {
        assert_eq!(palette_colour(16), Color32::from_rgb(0, 0, 0));
        assert_eq!(palette_colour(231), Color32::from_rgb(255, 255, 255));
        assert_eq!(palette_colour(232), Color32::from_rgb(8, 8, 8));
        assert_eq!(palette_colour(255), Color32::from_rgb(238, 238, 238));
        assert_eq!(default_palette_rgb(1).r, 0xE5);
    }
}
