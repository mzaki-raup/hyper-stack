//! Cached syntect highlighting. Parse/highlight state is checkpointed every
//! `CHECKPOINT_EVERY` lines so an edit only re-highlights from the nearest
//! checkpoint before it, and only the visible lines are ever computed.

use std::sync::LazyLock;

use egui::Color32;
use syntect::highlighting::{
    FontStyle, HighlightIterator, HighlightState, Highlighter, Theme, ThemeSet,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};

use crate::buffer::Buffer;
use crate::languages::Language;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(SyntaxSet::load_defaults_newlines);
static THEMES: LazyLock<ThemeSet> = LazyLock::new(ThemeSet::load_defaults);

const CHECKPOINT_EVERY: usize = 64;
/// Lines longer than this are drawn without highlighting (minified files).
const MAX_HIGHLIGHT_LINE: usize = 4_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    /// Char offsets within the line.
    pub start: usize,
    pub end: usize,
    pub fg: Color32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

pub fn syntax_set() -> &'static SyntaxSet {
    &SYNTAXES
}

pub fn theme(dark: bool) -> &'static Theme {
    let name = if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    };
    THEMES.themes.get(name).unwrap_or_else(|| {
        THEMES
            .themes
            .values()
            .next()
            .expect("syntect default themes")
    })
}

pub struct LineHighlighter {
    syntax: Option<&'static SyntaxReference>,
    dark: bool,
    /// (first line covered, parse state, highlight state) at line starts.
    checkpoints: Vec<(usize, ParseState, HighlightState)>,
    cache: Vec<Option<Vec<Span>>>,
    version: u64,
}

impl LineHighlighter {
    pub fn new(lang: &Language, dark: bool) -> Self {
        let syntax = SYNTAXES
            .find_syntax_by_token(lang.syntax_token)
            .filter(|s| s.name != "Plain Text");
        Self {
            syntax,
            dark,
            checkpoints: Vec::new(),
            cache: Vec::new(),
            version: u64::MAX,
        }
    }

    pub fn set_language(&mut self, lang: &Language) {
        self.syntax = SYNTAXES
            .find_syntax_by_token(lang.syntax_token)
            .filter(|s| s.name != "Plain Text");
        self.invalidate_all();
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.invalidate_all();
        }
    }

    pub fn has_syntax(&self) -> bool {
        self.syntax.is_some()
    }

    pub fn invalidate_all(&mut self) {
        self.checkpoints.clear();
        self.cache.clear();
    }

    /// Drop everything from `line` on (an edit happened there).
    pub fn invalidate_from(&mut self, line: usize) {
        self.cache.truncate(line);
        self.checkpoints.retain(|(l, _, _)| *l <= line);
        // The checkpoint *at* `line` was computed from the previous line's end,
        // which is still valid; checkpoints after it are gone.
    }

    /// Spans for `line`, computing forward from the nearest checkpoint when needed.
    pub fn spans(&mut self, buf: &Buffer, line: usize) -> Vec<Span> {
        if buf.version() != self.version {
            // The owner is expected to call invalidate_from; this is the safety net.
            self.version = buf.version();
        }
        let Some(syntax) = self.syntax else {
            return Vec::new();
        };
        if line < self.cache.len()
            && let Some(s) = &self.cache[line]
        {
            return s.clone();
        }
        let theme = theme(self.dark);
        let highlighter = Highlighter::new(theme);
        // Start from the last checkpoint at or before `line`.
        let (mut cur_line, mut parse_state, mut hl_state) =
            match self.checkpoints.iter().rev().find(|(l, _, _)| *l <= line) {
                Some((l, p, h)) => (*l, p.clone(), h.clone()),
                None => (
                    0,
                    ParseState::new(syntax),
                    HighlightState::new(&highlighter, ScopeStack::new()),
                ),
            };
        if self.checkpoints.is_empty() {
            self.checkpoints
                .push((0, parse_state.clone(), hl_state.clone()));
        }
        while cur_line <= line && cur_line < buf.len_lines() {
            let text = buf.line_text(cur_line);
            let spans = if text.chars().count() > MAX_HIGHLIGHT_LINE {
                Vec::new()
            } else {
                let with_nl = format!("{text}\n");
                match parse_state.parse_line(&with_nl, &SYNTAXES) {
                    Ok(ops) => {
                        let iter =
                            HighlightIterator::new(&mut hl_state, &ops, &with_nl, &highlighter);
                        let mut out = Vec::new();
                        let mut offset = 0usize;
                        for (style, piece) in iter {
                            let n = piece.chars().count();
                            if n == 0 {
                                continue;
                            }
                            let fg = Color32::from_rgb(
                                style.foreground.r,
                                style.foreground.g,
                                style.foreground.b,
                            );
                            out.push(Span {
                                start: offset,
                                end: offset + n,
                                fg,
                                bold: style.font_style.contains(FontStyle::BOLD),
                                italic: style.font_style.contains(FontStyle::ITALIC),
                                underline: style.font_style.contains(FontStyle::UNDERLINE),
                            });
                            offset += n;
                        }
                        out
                    }
                    Err(_) => Vec::new(),
                }
            };
            if self.cache.len() <= cur_line {
                self.cache.resize(cur_line + 1, None);
            }
            self.cache[cur_line] = Some(spans);
            cur_line += 1;
            if cur_line % CHECKPOINT_EVERY == 0
                && !self.checkpoints.iter().any(|(l, _, _)| *l == cur_line)
            {
                self.checkpoints
                    .push((cur_line, parse_state.clone(), hl_state.clone()));
            }
        }
        self.cache.get(line).cloned().flatten().unwrap_or_default()
    }

    /// Theme colours the view needs around the code (gutter, current line).
    pub fn background(&self) -> Option<Color32> {
        theme(self.dark)
            .settings
            .background
            .map(|c| Color32::from_rgb(c.r, c.g, c.b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::languages;

    #[test]
    fn rust_keywords_get_a_distinct_colour_and_cache_survives_edits() {
        let mut buf = Buffer::from_text("fn main() {\n    let x = 1;\n}\n");
        let mut h = LineHighlighter::new(&languages::detect("a.rs"), true);
        assert!(h.has_syntax());
        let spans0 = h.spans(&buf, 0);
        assert!(!spans0.is_empty());
        let fn_colour = spans0[0].fg;
        let spans1 = h.spans(&buf, 1);
        let plain = spans1
            .iter()
            .find(|s| {
                buf.line_text(1)[..]
                    .chars()
                    .skip(s.start)
                    .take(s.end - s.start)
                    .collect::<String>()
                    == " = "
            })
            .map(|s| s.fg);
        assert_ne!(Some(fn_colour), plain);
        // Edit line 1: only lines >= 1 are recomputed; line 0 stays cached.
        buf.apply(
            vec![crate::buffer::Edit {
                pos: buf.line_start(1),
                removed: String::new(),
                inserted: "// ".into(),
            }],
            crate::buffer::EditKind::Other,
            vec![],
            vec![],
        );
        h.invalidate_from(1);
        assert_eq!(h.spans(&buf, 0), spans0);
        let commented = h.spans(&buf, 1);
        assert!(
            commented.iter().all(|s| s.fg == commented[0].fg),
            "a comment line is one colour"
        );
    }

    #[test]
    fn plain_text_has_no_spans() {
        let buf = Buffer::from_text("hello");
        let mut h = LineHighlighter::new(&languages::plain(), true);
        assert!(!h.has_syntax());
        assert!(h.spans(&buf, 0).is_empty());
    }
}
