//! Cursors and selections. `head` is where the caret is, `anchor` where the
//! selection started; equal means no selection. The first cursor is primary.

use crate::buffer::Buffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub head: usize,
    pub anchor: usize,
    /// Column to aim for on vertical moves (sticky column).
    pub goal_col: Option<usize>,
}

impl Cursor {
    pub fn at(pos: usize) -> Self {
        Self {
            head: pos,
            anchor: pos,
            goal_col: None,
        }
    }
    pub fn range(&self) -> (usize, usize) {
        (self.head.min(self.anchor), self.head.max(self.anchor))
    }
    pub fn has_selection(&self) -> bool {
        self.head != self.anchor
    }
    pub fn collapse(&mut self) {
        self.anchor = self.head;
    }
    fn place(&mut self, pos: usize, select: bool) {
        self.head = pos;
        if !select {
            self.anchor = pos;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursors {
    pub list: Vec<Cursor>,
}

impl Default for Cursors {
    fn default() -> Self {
        Self {
            list: vec![Cursor::at(0)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
    PageUp(usize),
    PageDown(usize),
}

impl Cursors {
    pub fn single(pos: usize) -> Self {
        Self {
            list: vec![Cursor::at(pos)],
        }
    }

    pub fn primary(&self) -> Cursor {
        self.list[0]
    }

    pub fn set_single(&mut self, c: Cursor) {
        self.list = vec![c];
    }

    pub fn has_selection(&self) -> bool {
        self.list.iter().any(Cursor::has_selection)
    }

    /// `(head, anchor)` pairs for undo bookkeeping.
    pub fn snapshot(&self) -> Vec<(usize, usize)> {
        self.list.iter().map(|c| (c.head, c.anchor)).collect()
    }

    pub fn restore(&mut self, snap: &[(usize, usize)], len: usize) {
        if snap.is_empty() {
            self.list = vec![Cursor::at(0)];
            return;
        }
        self.list = snap
            .iter()
            .map(|&(h, a)| Cursor {
                head: h.min(len),
                anchor: a.min(len),
                goal_col: None,
            })
            .collect();
    }

    /// Clamp into the buffer and merge overlapping cursors.
    pub fn normalise(&mut self, len: usize) {
        for c in &mut self.list {
            c.head = c.head.min(len);
            c.anchor = c.anchor.min(len);
        }
        self.list.sort_by_key(|c| c.range().0);
        let mut merged: Vec<Cursor> = Vec::with_capacity(self.list.len());
        for c in self.list.drain(..) {
            if let Some(last) = merged.last_mut() {
                let (ls, le) = last.range();
                let (cs, ce) = c.range();
                let overlap = if last.has_selection() || c.has_selection() {
                    cs < le || (cs == le && cs == ls)
                } else {
                    cs == ls
                };
                if overlap || (cs == le && last.has_selection() && c.has_selection()) {
                    let s = ls.min(cs);
                    let e = le.max(ce);
                    let forward = last.head >= last.anchor;
                    *last = Cursor {
                        head: if forward { e } else { s },
                        anchor: if forward { s } else { e },
                        goal_col: None,
                    };
                    continue;
                }
            }
            merged.push(c);
        }
        self.list = merged;
        if self.list.is_empty() {
            self.list.push(Cursor::at(0));
        }
    }

    pub fn move_all(&mut self, buf: &Buffer, mv: Move, select: bool) {
        for c in &mut self.list {
            let (line, col) = buf.char_to_line_col(c.head);
            match mv {
                Move::Left => {
                    if !select && c.has_selection() {
                        let (s, _) = c.range();
                        c.place(s, false);
                    } else {
                        c.place(c.head.saturating_sub(1), select);
                    }
                    c.goal_col = None;
                }
                Move::Right => {
                    if !select && c.has_selection() {
                        let (_, e) = c.range();
                        c.place(e, false);
                    } else {
                        c.place((c.head + 1).min(buf.len_chars()), select);
                    }
                    c.goal_col = None;
                }
                Move::Up | Move::Down | Move::PageUp(_) | Move::PageDown(_) => {
                    let goal = c.goal_col.unwrap_or(col);
                    let target = match mv {
                        Move::Up => line.checked_sub(1),
                        Move::Down => (line + 1 < buf.len_lines()).then_some(line + 1),
                        Move::PageUp(n) => Some(line.saturating_sub(n.max(1))),
                        Move::PageDown(n) => {
                            Some((line + n.max(1)).min(buf.len_lines().saturating_sub(1)))
                        }
                        _ => None,
                    };
                    match target {
                        Some(t) => {
                            c.place(buf.line_col_to_char(t, goal), select);
                            c.goal_col = Some(goal);
                        }
                        None => {
                            let pos = if matches!(mv, Move::Up | Move::PageUp(_)) {
                                0
                            } else {
                                buf.len_chars()
                            };
                            c.place(pos, select);
                        }
                    }
                }
                Move::WordLeft => {
                    c.place(buf.prev_word_boundary(c.head), select);
                    c.goal_col = None;
                }
                Move::WordRight => {
                    c.place(buf.next_word_boundary(c.head), select);
                    c.goal_col = None;
                }
                Move::LineStart => {
                    // Smart Home: first non-blank, then column 0.
                    let start = buf.line_start(line);
                    let indent = buf.indent_of(line).chars().count();
                    let first = start + indent;
                    let pos = if c.head == first { start } else { first };
                    c.place(pos, select);
                    c.goal_col = None;
                }
                Move::LineEnd => {
                    c.place(buf.line_start(line) + buf.line_len(line), select);
                    c.goal_col = None;
                }
                Move::DocStart => {
                    c.place(0, select);
                    c.goal_col = None;
                }
                Move::DocEnd => {
                    c.place(buf.len_chars(), select);
                    c.goal_col = None;
                }
            }
        }
        self.normalise(buf.len_chars());
    }

    /// Select the word under each cursor, or add the next occurrence of the
    /// current selection as a new cursor (Ctrl+D).
    pub fn select_next_occurrence(&mut self, buf: &Buffer) {
        let primary = self.list[self.list.len() - 1];
        if !primary.has_selection() {
            for c in &mut self.list {
                if !c.has_selection() {
                    c.anchor = buf.word_start(c.head);
                    c.head = buf.word_end(c.head);
                }
            }
            self.normalise(buf.len_chars());
            return;
        }
        let (s, e) = primary.range();
        let needle = buf.slice(s, e);
        if needle.is_empty() {
            return;
        }
        let text = buf.text();
        let start_byte = char_to_byte(&text, e);
        let found = text[start_byte..]
            .find(&needle)
            .map(|b| byte_to_char(&text, start_byte + b))
            .or_else(|| {
                text[..start_byte]
                    .find(&needle)
                    .map(|b| byte_to_char(&text, b))
            });
        if let Some(pos) = found {
            let n = needle.chars().count();
            if !self.list.iter().any(|c| c.range() == (pos, pos + n)) {
                self.list.push(Cursor {
                    head: pos + n,
                    anchor: pos,
                    goal_col: None,
                });
            }
        }
        self.normalise(buf.len_chars());
    }

    /// One cursor per occurrence of the primary selection (Ctrl+Shift+L).
    pub fn select_all_occurrences(&mut self, buf: &Buffer) {
        let primary = self.list[0];
        if !primary.has_selection() {
            self.select_next_occurrence(buf);
            if !self.list[0].has_selection() {
                return;
            }
        }
        let (s, e) = self.list[0].range();
        let needle = buf.slice(s, e);
        if needle.is_empty() {
            return;
        }
        let text = buf.text();
        let n = needle.chars().count();
        let mut out = Vec::new();
        let mut from = 0;
        while let Some(b) = text[from..].find(&needle) {
            let pos = byte_to_char(&text, from + b);
            out.push(Cursor {
                head: pos + n,
                anchor: pos,
                goal_col: None,
            });
            from += b + needle.len();
        }
        if !out.is_empty() {
            self.list = out;
        }
        self.normalise(buf.len_chars());
    }

    /// Add a cursor on the line above/below the last one (Ctrl+Alt+Up/Down).
    pub fn add_cursor_vertical(&mut self, buf: &Buffer, down: bool) {
        let last = *self
            .list
            .iter()
            .max_by_key(|c| if down { c.head } else { usize::MAX - c.head })
            .unwrap();
        let (line, col) = buf.char_to_line_col(last.head);
        let target = if down { line + 1 } else { line.wrapping_sub(1) };
        if target < buf.len_lines() {
            let pos = buf.line_col_to_char(target, last.goal_col.unwrap_or(col));
            self.list.push(Cursor {
                head: pos,
                anchor: pos,
                goal_col: Some(last.goal_col.unwrap_or(col)),
            });
            self.normalise(buf.len_chars());
        }
    }

    pub fn collapse_to_primary(&mut self) {
        let mut p = self.list[0];
        p.collapse();
        self.list = vec![p];
    }
}

pub fn char_to_byte(text: &str, ch: usize) -> usize {
    text.char_indices()
        .nth(ch)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

pub fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_moves_keep_the_goal_column() {
        let b = Buffer::from_text("abcdef\nab\nabcdef");
        let mut c = Cursors::single(5);
        c.move_all(&b, Move::Down, false);
        assert_eq!(c.primary().head, 9); // clamped to "ab"
        c.move_all(&b, Move::Down, false);
        assert_eq!(c.primary().head, 15); // back to column 5
    }

    #[test]
    fn select_next_and_all_occurrences() {
        let b = Buffer::from_text("foo bar foo baz foo");
        let mut c = Cursors::single(1);
        c.select_next_occurrence(&b); // selects "foo"
        assert_eq!(c.primary().range(), (0, 3));
        c.select_next_occurrence(&b);
        assert_eq!(c.list.len(), 2);
        assert_eq!(c.list[1].range(), (8, 11));
        c.select_all_occurrences(&b);
        assert_eq!(c.list.len(), 3);
    }

    #[test]
    fn overlapping_cursors_merge() {
        let b = Buffer::from_text("hello");
        let mut c = Cursors {
            list: vec![
                Cursor::at(2),
                Cursor::at(2),
                Cursor {
                    head: 4,
                    anchor: 1,
                    goal_col: None,
                },
            ],
        };
        c.normalise(b.len_chars());
        assert_eq!(c.list.len(), 1);
        assert_eq!(c.list[0].range(), (1, 4));
    }

    #[test]
    fn smart_home_toggles_between_indent_and_column_zero() {
        let b = Buffer::from_text("    x = 1");
        let mut c = Cursors::single(7);
        c.move_all(&b, Move::LineStart, false);
        assert_eq!(c.primary().head, 4);
        c.move_all(&b, Move::LineStart, false);
        assert_eq!(c.primary().head, 0);
        c.move_all(&b, Move::LineStart, false);
        assert_eq!(c.primary().head, 4);
    }
}
