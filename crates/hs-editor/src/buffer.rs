//! The text buffer: a rope, the file's encoding and line endings, and an
//! undo/redo stack with typing groups. Positions are char indices into the
//! rope; the text is always stored with `\n` line endings.

use std::time::{Duration, Instant};

use ropey::Rope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16LeBom,
    Utf16Be,
    Utf16BeBom,
    Windows1252,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16LeBom => "UTF-16 LE BOM",
            Self::Utf16Be => "UTF-16 BE",
            Self::Utf16BeBom => "UTF-16 BE BOM",
            Self::Windows1252 => "Windows-1252",
        }
    }
    pub fn all() -> [Encoding; 7] {
        [
            Self::Utf8,
            Self::Utf8Bom,
            Self::Utf16Le,
            Self::Utf16LeBom,
            Self::Utf16Be,
            Self::Utf16BeBom,
            Self::Windows1252,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// One reversible change. `pos` is a char index; applying = delete `removed`
/// at `pos` then insert `inserted` there.
#[derive(Debug, Clone, PartialEq)]
pub struct Edit {
    pub pos: usize,
    pub removed: String,
    pub inserted: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Typing,
    Deleting,
    Other,
}

#[derive(Debug, Clone)]
struct UndoGroup {
    edits: Vec<Edit>,
    cursors_before: Vec<(usize, usize)>,
    cursors_after: Vec<(usize, usize)>,
    kind: EditKind,
    at: Instant,
}

pub struct Buffer {
    rope: Rope,
    pub encoding: Encoding,
    pub eol: Eol,
    undo: Vec<UndoGroup>,
    redo: Vec<UndoGroup>,
    version: u64,
    saved_version: u64,
    max_line_chars: Option<usize>,
}

const GROUP_WINDOW: Duration = Duration::from_millis(700);

impl Default for Buffer {
    fn default() -> Self {
        Self::from_text("")
    }
}

impl Buffer {
    pub fn from_text(text: &str) -> Self {
        let eol = if text.contains("\r\n") {
            Eol::CrLf
        } else {
            Eol::Lf
        };
        let normalised = if eol == Eol::CrLf {
            text.replace("\r\n", "\n")
        } else {
            text.to_string()
        };
        Self {
            rope: Rope::from_str(&normalised),
            encoding: Encoding::Utf8,
            eol,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            saved_version: 0,
            max_line_chars: None,
        }
    }

    /// Decode bytes: BOMs first, then valid UTF-8, then a UTF-16 LE heuristic,
    /// else Windows-1252 (never fails, never loses bytes silently).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let (text, encoding) = decode(bytes);
        let mut b = Self::from_text(&text);
        b.encoding = encoding;
        b
    }

    /// Decode bytes using the explicitly selected encoding rather than BOM
    /// detection/heuristics. A matching BOM is consumed; a different BOM is
    /// treated as content, as expected when forcing an encoding.
    pub fn from_bytes_with_encoding(bytes: &[u8], encoding: Encoding) -> Self {
        let body = match encoding {
            Encoding::Utf8 | Encoding::Utf8Bom if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) => {
                &bytes[3..]
            }
            Encoding::Utf16LeBom if bytes.starts_with(&[0xFF, 0xFE]) => &bytes[2..],
            Encoding::Utf16BeBom if bytes.starts_with(&[0xFE, 0xFF]) => &bytes[2..],
            _ => bytes,
        };
        let text = match encoding {
            Encoding::Utf8 | Encoding::Utf8Bom => String::from_utf8_lossy(body).into_owned(),
            Encoding::Utf16Le | Encoding::Utf16LeBom => encoding_rs::UTF_16LE
                .decode_without_bom_handling(body)
                .0
                .into_owned(),
            Encoding::Utf16Be | Encoding::Utf16BeBom => encoding_rs::UTF_16BE
                .decode_without_bom_handling(body)
                .0
                .into_owned(),
            Encoding::Windows1252 => encoding_rs::WINDOWS_1252
                .decode_without_bom_handling(body)
                .0
                .into_owned(),
        };
        let mut buffer = Self::from_text(&text);
        buffer.encoding = encoding;
        buffer
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = self.rope.to_string();
        if self.eol == Eol::CrLf {
            text = text.replace('\n', "\r\n");
        }
        encode(&text, self.encoding)
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }
    pub fn rope(&self) -> &Rope {
        &self.rope
    }
    pub fn len_chars(&self) -> usize {
        self.rope.len_chars()
    }
    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn dirty(&self) -> bool {
        self.version != self.saved_version
    }
    pub fn mark_saved(&mut self) {
        self.saved_version = self.version;
    }
    pub fn mark_modified_externally(&mut self) {
        self.saved_version = u64::MAX;
    }

    /// Line text without its trailing newline.
    pub fn line_text(&self, line: usize) -> String {
        if line >= self.rope.len_lines() {
            return String::new();
        }
        let s = self.rope.line(line);
        let mut t = s.to_string();
        if t.ends_with('\n') {
            t.pop();
        }
        t
    }

    /// Chars in the line, excluding the newline.
    pub fn line_len(&self, line: usize) -> usize {
        if line >= self.rope.len_lines() {
            return 0;
        }
        let s = self.rope.line(line);
        let n = s.len_chars();
        if n > 0 && s.char(n - 1) == '\n' {
            n - 1
        } else {
            n
        }
    }

    pub fn line_start(&self, line: usize) -> usize {
        self.rope
            .line_to_char(line.min(self.rope.len_lines().saturating_sub(1)))
    }

    pub fn char_to_line_col(&self, idx: usize) -> (usize, usize) {
        let idx = idx.min(self.rope.len_chars());
        let line = self.rope.char_to_line(idx);
        (line, idx - self.rope.line_to_char(line))
    }

    pub fn line_col_to_char(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.rope.len_lines().saturating_sub(1));
        self.rope.line_to_char(line) + col.min(self.line_len(line))
    }

    pub fn char_at(&self, idx: usize) -> Option<char> {
        (idx < self.rope.len_chars()).then(|| self.rope.char(idx))
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        let end = end.min(self.rope.len_chars());
        let start = start.min(end);
        self.rope.slice(start..end).to_string()
    }

    /// Longest line in chars (cached; recomputed after edits, bounded for huge files).
    pub fn max_line_chars(&mut self) -> usize {
        if let Some(m) = self.max_line_chars {
            return m;
        }
        let lines = self.rope.len_lines();
        let m = if lines > 200_000 {
            200
        } else {
            (0..lines).map(|l| self.line_len(l)).max().unwrap_or(0)
        };
        self.max_line_chars = Some(m);
        m
    }

    // ---- editing ----------------------------------------------------------

    /// Apply edits as one undo group. Edits are applied in the given order and
    /// must already account for each other's shifts (callers apply from the
    /// highest position down so earlier positions stay valid).
    pub fn apply(
        &mut self,
        edits: Vec<Edit>,
        kind: EditKind,
        cursors_before: Vec<(usize, usize)>,
        cursors_after: Vec<(usize, usize)>,
    ) {
        if edits.is_empty() {
            return;
        }
        for e in &edits {
            self.apply_raw(e);
        }
        self.version += 1;
        self.redo.clear();
        self.max_line_chars = None;
        let now = Instant::now();
        let merge = matches!(kind, EditKind::Typing | EditKind::Deleting)
            && self.undo.last().is_some_and(|g| {
                g.kind == kind
                    && now.duration_since(g.at) < GROUP_WINDOW
                    && g.cursors_after.len() == cursors_after.len()
            });
        if merge {
            let g = self.undo.last_mut().unwrap();
            g.edits.extend(edits);
            g.cursors_after = cursors_after;
            g.at = now;
        } else {
            self.undo.push(UndoGroup {
                edits,
                cursors_before,
                cursors_after,
                kind,
                at: now,
            });
            if self.undo.len() > 10_000 {
                self.undo.remove(0);
            }
        }
    }

    fn apply_raw(&mut self, e: &Edit) {
        let pos = e.pos.min(self.rope.len_chars());
        if !e.removed.is_empty() {
            let end = (pos + e.removed.chars().count()).min(self.rope.len_chars());
            self.rope.remove(pos..end);
        }
        if !e.inserted.is_empty() {
            self.rope.insert(pos, &e.inserted);
        }
    }

    fn revert_raw(&mut self, e: &Edit) {
        let pos = e.pos.min(self.rope.len_chars());
        if !e.inserted.is_empty() {
            let end = (pos + e.inserted.chars().count()).min(self.rope.len_chars());
            self.rope.remove(pos..end);
        }
        if !e.removed.is_empty() {
            self.rope.insert(pos, &e.removed);
        }
    }

    /// Undo the last group; returns the cursors from before it.
    pub fn undo(&mut self) -> Option<Vec<(usize, usize)>> {
        let g = self.undo.pop()?;
        for e in g.edits.iter().rev() {
            self.revert_raw(e);
        }
        self.version += 1;
        self.max_line_chars = None;
        let cursors = g.cursors_before.clone();
        self.redo.push(g);
        Some(cursors)
    }

    pub fn redo(&mut self) -> Option<Vec<(usize, usize)>> {
        let g = self.redo.pop()?;
        for e in &g.edits {
            self.apply_raw(e);
        }
        self.version += 1;
        self.max_line_chars = None;
        let cursors = g.cursors_after.clone();
        self.undo.push(g);
        Some(cursors)
    }

    /// Break the typing group so the next keystroke starts a new undo step.
    pub fn break_undo_group(&mut self) {
        if let Some(g) = self.undo.last_mut() {
            g.at = Instant::now() - GROUP_WINDOW * 2;
        }
    }

    // ---- navigation helpers ----------------------------------------------

    pub fn is_word_char(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    pub fn word_start(&self, idx: usize) -> usize {
        let mut i = idx.min(self.rope.len_chars());
        while i > 0 && Self::is_word_char(self.rope.char(i - 1)) {
            i -= 1;
        }
        i
    }

    pub fn word_end(&self, idx: usize) -> usize {
        let n = self.rope.len_chars();
        let mut i = idx.min(n);
        while i < n && Self::is_word_char(self.rope.char(i)) {
            i += 1;
        }
        i
    }

    /// Previous word boundary (Ctrl+Left semantics).
    pub fn prev_word_boundary(&self, idx: usize) -> usize {
        let mut i = idx.min(self.rope.len_chars());
        while i > 0 && self.rope.char(i - 1).is_whitespace() && self.rope.char(i - 1) != '\n' {
            i -= 1;
        }
        if i > 0 && self.rope.char(i - 1) == '\n' {
            return i - 1;
        }
        let word = i > 0 && Self::is_word_char(self.rope.char(i - 1));
        while i > 0 {
            let c = self.rope.char(i - 1);
            if c == '\n' || c.is_whitespace() || Self::is_word_char(c) != word {
                break;
            }
            i -= 1;
        }
        i
    }

    /// Next word boundary (Ctrl+Right semantics).
    pub fn next_word_boundary(&self, idx: usize) -> usize {
        let n = self.rope.len_chars();
        let mut i = idx.min(n);
        if i < n && self.rope.char(i) == '\n' {
            return i + 1;
        }
        let word = i < n && Self::is_word_char(self.rope.char(i));
        while i < n {
            let c = self.rope.char(i);
            if c == '\n' || c.is_whitespace() || Self::is_word_char(c) != word {
                break;
            }
            i += 1;
        }
        while i < n && self.rope.char(i).is_whitespace() && self.rope.char(i) != '\n' {
            i += 1;
        }
        i
    }

    /// The bracket matching the one at `idx` (or just before it), searching
    /// at most `limit` chars.
    pub fn matching_bracket(&self, idx: usize, limit: usize) -> Option<(usize, usize)> {
        let n = self.rope.len_chars();
        let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
        let candidates = [idx, idx.wrapping_sub(1)];
        for &at in &candidates {
            if at >= n {
                continue;
            }
            let c = self.rope.char(at);
            if let Some(&(open, close)) = pairs.iter().find(|(o, cl)| *o == c || *cl == c) {
                let forward = c == open;
                let mut depth = 0i32;
                let mut i = at;
                let mut steps = 0;
                loop {
                    let ch = self.rope.char(i);
                    if ch == open {
                        depth += if forward { 1 } else { -1 };
                    } else if ch == close {
                        depth += if forward { -1 } else { 1 };
                    }
                    if depth == 0 {
                        return Some((at.min(i), at.max(i)));
                    }
                    steps += 1;
                    if steps > limit {
                        return None;
                    }
                    if forward {
                        i += 1;
                        if i >= n {
                            return None;
                        }
                    } else {
                        if i == 0 {
                            return None;
                        }
                        i -= 1;
                    }
                }
            }
        }
        None
    }

    /// Leading whitespace of a line.
    pub fn indent_of(&self, line: usize) -> String {
        self.line_text(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }
}

fn decode(bytes: &[u8]) -> (String, Encoding) {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(&bytes[3..]).to_string(),
            Encoding::Utf8Bom,
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (s, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(&bytes[2..]);
        return (s.to_string(), Encoding::Utf16LeBom);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (s, _) = encoding_rs::UTF_16BE.decode_without_bom_handling(&bytes[2..]);
        return (s.to_string(), Encoding::Utf16BeBom);
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return (s.to_string(), Encoding::Utf8);
    }
    // UTF-16 LE without BOM: every other byte zero in ASCII-heavy text.
    let sample = &bytes[..bytes.len().min(4096)];
    if sample.len() >= 4 {
        let zeros_odd = sample
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|b| **b == 0)
            .count();
        if zeros_odd * 10 > sample.len() / 2 * 7 {
            let (s, _) = encoding_rs::UTF_16LE.decode_without_bom_handling(bytes);
            return (s.to_string(), Encoding::Utf16Le);
        }
    }
    let (s, _) = encoding_rs::WINDOWS_1252.decode_without_bom_handling(bytes);
    (s.to_string(), Encoding::Windows1252)
}

fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(text.as_bytes());
            v
        }
        Encoding::Utf16Le => {
            let mut v = Vec::new();
            v.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
            v
        }
        Encoding::Utf16LeBom => {
            let mut v = vec![0xFF, 0xFE];
            v.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
            v
        }
        Encoding::Utf16Be => {
            let mut v = Vec::new();
            v.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
            v
        }
        Encoding::Utf16BeBom => {
            let mut v = vec![0xFE, 0xFF];
            v.extend(text.encode_utf16().flat_map(|u| u.to_be_bytes()));
            v
        }
        Encoding::Windows1252 => {
            let (cow, _, _) = encoding_rs::WINDOWS_1252.encode(text);
            cow.to_vec()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crlf_is_detected_normalised_and_restored() {
        let b = Buffer::from_bytes(b"a\r\nb\r\n");
        assert_eq!(b.eol, Eol::CrLf);
        assert_eq!(b.text(), "a\nb\n");
        assert_eq!(b.to_bytes(), b"a\r\nb\r\n");
        assert_eq!(b.len_lines(), 3);
        assert_eq!(b.line_text(1), "b");
        assert_eq!(b.line_len(0), 1);
    }

    #[test]
    fn encodings_round_trip() {
        for enc in Encoding::all() {
            let mut b = Buffer::from_text("héllo\nwörld");
            b.encoding = enc;
            let bytes = b.to_bytes();
            let back = if matches!(enc, Encoding::Utf16Le | Encoding::Utf16Be) {
                Buffer::from_bytes_with_encoding(&bytes, enc)
            } else {
                Buffer::from_bytes(&bytes)
            };
            assert_eq!(back.text(), "héllo\nwörld", "{enc:?}");
            assert_eq!(back.encoding, enc, "{enc:?}");
        }
        let latin = encoding_rs::WINDOWS_1252.encode("caf\u{e9}").0.to_vec();
        let b = Buffer::from_bytes(&latin);
        assert_eq!(b.encoding, Encoding::Windows1252);
        assert_eq!(b.text(), "caf\u{e9}");
    }

    #[test]
    fn explicitly_selected_encoding_redecodes_bytes() {
        let bytes = encode("café\n", Encoding::Utf16BeBom);
        let b = Buffer::from_bytes_with_encoding(&bytes, Encoding::Utf16BeBom);
        assert_eq!(b.text(), "café\n");
        assert_eq!(b.encoding, Encoding::Utf16BeBom);
        assert_eq!(b.to_bytes(), bytes);
    }

    #[test]
    fn edits_undo_and_redo_with_typing_groups() {
        let mut b = Buffer::from_text("hello");
        b.apply(
            vec![Edit {
                pos: 5,
                removed: String::new(),
                inserted: " w".into(),
            }],
            EditKind::Typing,
            vec![(5, 5)],
            vec![(7, 7)],
        );
        b.apply(
            vec![Edit {
                pos: 7,
                removed: String::new(),
                inserted: "orld".into(),
            }],
            EditKind::Typing,
            vec![(7, 7)],
            vec![(11, 11)],
        );
        assert_eq!(b.text(), "hello world");
        assert!(b.dirty());
        // Both typing edits merged into one group.
        assert_eq!(b.undo().unwrap(), vec![(5, 5)]);
        assert_eq!(b.text(), "hello");
        assert_eq!(b.redo().unwrap(), vec![(11, 11)]);
        assert_eq!(b.text(), "hello world");
        b.break_undo_group();
        b.apply(
            vec![Edit {
                pos: 0,
                removed: "hello".into(),
                inserted: "HELLO".into(),
            }],
            EditKind::Other,
            vec![(0, 5)],
            vec![(0, 5)],
        );
        assert_eq!(b.text(), "HELLO world");
        b.undo();
        assert_eq!(b.text(), "hello world");
        b.undo();
        assert_eq!(b.text(), "hello");
        assert!(b.undo().is_none());
    }

    #[test]
    fn positions_words_and_brackets() {
        let b = Buffer::from_text("fn main() {\n    let x = (1 + 2);\n}\n");
        assert_eq!(b.char_to_line_col(13), (1, 1));
        assert_eq!(b.line_col_to_char(1, 4), 16);
        assert_eq!(b.word_start(5), 3);
        assert_eq!(b.word_end(3), 7);
        assert_eq!(b.next_word_boundary(0), 3);
        assert_eq!(b.prev_word_boundary(7), 3);
        let open = b.text().find('{').unwrap();
        let close = b.text().rfind('}').unwrap();
        assert_eq!(b.matching_bracket(open, 10_000), Some((open, close)));
        assert_eq!(b.matching_bracket(close + 1, 10_000), Some((open, close)));
        assert_eq!(b.indent_of(1), "    ");
    }
}
