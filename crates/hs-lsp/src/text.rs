//! Columns, edits and snippets: the glue between the editor's char
//! offsets and what the protocol sends.

use crate::types::{Position, Range, TextEdit};

/// How the server counts columns (`positionEncoding`, LSP 3.17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf16,
    Utf8,
    Utf32,
}

impl Encoding {
    pub fn parse(s: &str) -> Self {
        match s {
            "utf-8" => Encoding::Utf8,
            "utf-32" => Encoding::Utf32,
            _ => Encoding::Utf16,
        }
    }

    fn units(self, c: char) -> u32 {
        match self {
            Encoding::Utf16 => c.len_utf16() as u32,
            Encoding::Utf8 => c.len_utf8() as u32,
            Encoding::Utf32 => 1,
        }
    }
}

/// A char column of `line` as the server counts it.
pub fn col_to_lsp(line: &str, char_col: usize, enc: Encoding) -> u32 {
    let mut n = 0u32;
    let mut seen = 0usize;
    for c in line.chars().take(char_col) {
        n += enc.units(c);
        seen += 1;
    }
    // Past the end (the position after a trailing newline): clamp.
    n + (char_col - seen) as u32
}

/// The char column of `line` for a server column. Past the end clamps
/// to the end of the line.
pub fn col_from_lsp(line: &str, lsp_col: u32, enc: Encoding) -> usize {
    let mut units = 0u32;
    for (i, c) in line.chars().enumerate() {
        if units >= lsp_col {
            return i;
        }
        units += enc.units(c);
    }
    line.chars().count()
}

/// The lines of a document as the conversions need them (without their
/// line breaks).
pub trait Lines {
    fn line_count(&self) -> usize;
    /// Text of line `n` without the newline; empty past the end.
    fn line(&self, n: usize) -> String;
    /// Char offset of the start of line `n`.
    fn line_start_char(&self, n: usize) -> usize;
}

impl Lines for str {
    fn line_count(&self) -> usize {
        self.split('\n').count()
    }
    fn line(&self, n: usize) -> String {
        self.split('\n')
            .nth(n)
            .map(|l| l.trim_end_matches('\r').to_string())
            .unwrap_or_default()
    }
    fn line_start_char(&self, n: usize) -> usize {
        self.split('\n')
            .take(n)
            .map(|l| l.chars().count() + 1)
            .sum()
    }
}

/// A server position as a char offset into the document.
pub fn position_to_char(doc: &(impl Lines + ?Sized), pos: Position, enc: Encoding) -> usize {
    let line = (pos.line as usize).min(doc.line_count().saturating_sub(1));
    let text = doc.line(line);
    doc.line_start_char(line) + col_from_lsp(&text, pos.character, enc)
}

/// A char offset as a server position (`line`/`col` already split by the
/// caller, since the editor's buffer answers that directly).
pub fn position_from_line_col(
    line_text: &str,
    line: usize,
    char_col: usize,
    enc: Encoding,
) -> Position {
    Position::new(line as u32, col_to_lsp(line_text, char_col, enc))
}

/// A server range as char offsets.
pub fn range_to_chars(doc: &(impl Lines + ?Sized), range: Range, enc: Encoding) -> (usize, usize) {
    let s = position_to_char(doc, range.start, enc);
    let e = position_to_char(doc, range.end, enc).max(s);
    (s, e)
}

/// Text edits as `(start, end, new_text)` in char offsets, ordered so that
/// applying them last-first keeps earlier offsets valid.
pub fn edits_to_chars(
    doc: &(impl Lines + ?Sized),
    edits: &[TextEdit],
    enc: Encoding,
) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = edits
        .iter()
        .map(|e| {
            let (s, en) = range_to_chars(doc, e.range, enc);
            (s, en, e.new_text.clone())
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    out
}

/// Apply edits to a string (tests and files that are not open).
pub fn apply_edits(text: &str, edits: &[TextEdit], enc: Encoding) -> String {
    let mut out: Vec<char> = text.chars().collect();
    for (s, e, new) in edits_to_chars(text, edits, enc) {
        let s = s.min(out.len());
        let e = e.min(out.len()).max(s);
        out.splice(s..e, new.chars());
    }
    out.into_iter().collect()
}

/// A snippet (`insertTextFormat` 2) reduced to plain text: tab stops
/// vanish, placeholders keep their default, choices keep the first. The
/// second value is the char offset where `$0` (the final cursor) was, or
/// where the first tab stop was.
pub fn strip_snippet(s: &str) -> (String, Option<usize>) {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut out_len = 0usize;
    let mut cursor: Option<usize> = None;
    let mut first_stop: Option<usize> = None;
    let mut i = 0;
    let note_stop =
        |name: &str, at: usize, cursor: &mut Option<usize>, first: &mut Option<usize>| {
            if name == "0" {
                *cursor = Some(at);
            } else if name.chars().all(|c| c.is_ascii_digit()) && first.is_none() {
                *first = Some(at);
            }
        };
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            out.push(chars[i + 1]);
            out_len += 1;
            i += 2;
            continue;
        }
        if c != '$' {
            out.push(c);
            out_len += 1;
            i += 1;
            continue;
        }
        // `$0`, `$12`, `$name`
        if i + 1 < chars.len() && chars[i + 1] != '{' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                j += 1;
            }
            if j == i + 1 {
                out.push('$');
                out_len += 1;
                i += 1;
                continue;
            }
            let name: String = chars[i + 1..j].iter().collect();
            note_stop(&name, out_len, &mut cursor, &mut first_stop);
            i = j;
            continue;
        }
        // `${1:default}`, `${1|a,b|}`, `${name}`
        if i + 1 < chars.len() && chars[i + 1] == '{' {
            let mut depth = 0;
            let mut j = i + 1;
            while j < chars.len() {
                match chars[j] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    '\\' => j += 1,
                    _ => {}
                }
                j += 1;
            }
            let inner: String = chars[i + 2..j.min(chars.len())].iter().collect();
            let (name, rest) = match inner.find([':', '|']) {
                Some(k) => (&inner[..k], &inner[k..]),
                None => (inner.as_str(), ""),
            };
            note_stop(name, out_len, &mut cursor, &mut first_stop);
            if let Some(default) = rest.strip_prefix(':') {
                let (inner_text, _) = strip_snippet(default);
                out_len += inner_text.chars().count();
                out.push_str(&inner_text);
            } else if let Some(choices) = rest.strip_prefix('|') {
                let first = choices
                    .trim_end_matches('|')
                    .split(',')
                    .next()
                    .unwrap_or("");
                out_len += first.chars().count();
                out.push_str(first);
            }
            i = j + 1;
            continue;
        }
        out.push('$');
        out_len += 1;
        i += 1;
    }
    (out, cursor.or(first_stop))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_convert_in_each_encoding() {
        let line = "a€𝄞b";
        assert_eq!(col_to_lsp(line, 3, Encoding::Utf16), 4);
        assert_eq!(col_to_lsp(line, 3, Encoding::Utf8), 8);
        assert_eq!(col_to_lsp(line, 3, Encoding::Utf32), 3);
        assert_eq!(col_from_lsp(line, 4, Encoding::Utf16), 3);
        assert_eq!(col_from_lsp(line, 8, Encoding::Utf8), 3);
        assert_eq!(col_from_lsp(line, 99, Encoding::Utf16), 4);
        assert_eq!(col_to_lsp("ab", 5, Encoding::Utf16), 5);
    }

    #[test]
    fn positions_map_to_char_offsets() {
        let doc = "ab\ncd€f\n";
        assert_eq!(doc.line(1), "cd€f");
        assert_eq!(
            position_to_char(doc, Position::new(1, 3), Encoding::Utf16),
            6
        );
        assert_eq!(
            position_to_char(doc, Position::new(9, 0), Encoding::Utf16),
            8
        );
    }

    #[test]
    fn edits_apply_last_first() {
        let doc = "hello world";
        let edits = vec![
            TextEdit {
                range: Range {
                    start: Position::new(0, 0),
                    end: Position::new(0, 5),
                },
                new_text: "HELLO".into(),
            },
            TextEdit {
                range: Range {
                    start: Position::new(0, 6),
                    end: Position::new(0, 11),
                },
                new_text: "there".into(),
            },
        ];
        assert_eq!(apply_edits(doc, &edits, Encoding::Utf16), "HELLO there");
    }

    #[test]
    fn snippets_reduce_to_text_with_a_cursor() {
        assert_eq!(strip_snippet("foo($1)$0"), ("foo()".into(), Some(5)));
        assert_eq!(strip_snippet("foo(${1:x})"), ("foo(x)".into(), Some(4)));
        assert_eq!(strip_snippet("${1|a,b|}"), ("a".into(), Some(0)));
        assert_eq!(strip_snippet("plain \\$x"), ("plain $x".into(), None));
        assert_eq!(
            strip_snippet("if ${1:cond} {\n\t$0\n}"),
            ("if cond {\n\t\n}".into(), Some(11))
        );
        assert_eq!(strip_snippet("cost: 5$"), ("cost: 5$".into(), None));
    }
}
