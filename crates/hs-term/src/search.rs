//! Scrollback search (docs/tools/terminal.md §3 "Search"): plain or regex,
//! with or without case, over every line of history and the screen.
//! Matches are reported in grid coordinates (`Line` as alacritty numbers it:
//! negative in history, `0..rows` on screen) and cell columns.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;
use regex::{Regex, RegexBuilder};

/// Searching stops after this many hits; the counter shows "1000+".
pub const MAX_MATCHES: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub regex: bool,
    pub case_sensitive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub line: i32,
    /// First cell column of the match.
    pub start: usize,
    /// One past the last cell column.
    pub end: usize,
}

impl SearchQuery {
    pub fn compile(&self) -> Result<Option<Regex>, String> {
        if self.text.is_empty() {
            return Ok(None);
        }
        let pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.case_sensitive)
            .size_limit(1 << 20)
            .build()
            .map(Some)
            .map_err(|e| e.to_string())
    }
}

/// The text of one grid line, and for each character the cell column it
/// starts at (wide characters take two cells, their spacer is skipped).
pub fn line_text<T>(term: &Term<T>, line: Line) -> (String, Vec<usize>) {
    let grid = term.grid();
    let row = &grid[line];
    let cols = grid.columns();
    let mut text = String::with_capacity(cols);
    let mut map = Vec::with_capacity(cols);
    for c in 0..cols {
        let cell = &row[Column(c)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        text.push(if cell.c == '\0' { ' ' } else { cell.c });
        map.push(c);
    }
    (text, map)
}

/// Every match from the oldest history line to the bottom of the screen.
pub fn search<T>(term: &Term<T>, re: &Regex) -> Vec<SearchMatch> {
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    let bottom = grid.screen_lines() as i32;
    let mut out = Vec::new();
    for l in top..bottom {
        let (text, map) = line_text(term, Line(l));
        if text.trim().is_empty() {
            continue;
        }
        for m in re.find_iter(&text) {
            if m.start() == m.end() {
                continue;
            }
            let first = text[..m.start()].chars().count();
            let last = text[..m.end()].chars().count();
            let start = map[first];
            // Column after the last character (a wide one ends a cell later).
            let end = if last < map.len() {
                map[last]
            } else {
                grid.columns()
            };
            out.push(SearchMatch {
                line: l,
                start,
                end,
            });
            if out.len() >= MAX_MATCHES {
                return out;
            }
        }
    }
    out
}

/// `display_offset` that brings `line` into view, roughly centred.
pub fn offset_for_line(line: i32, screen_lines: usize, history: usize) -> usize {
    if line >= 0 {
        return 0;
    }
    let want = (-line) as usize + screen_lines / 2;
    want.min(history)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    fn term_with(text: &str, rows: usize) -> Term<VoidListener> {
        let mut t = Term::new(
            Config {
                scrolling_history: 100,
                ..Config::default()
            },
            &TermSize::new(40, rows),
            VoidListener,
        );
        let mut p: Processor<StdSyncHandler> = Processor::new();
        p.advance(&mut t, text.replace('\n', "\r\n").as_bytes());
        t
    }

    fn q(text: &str, regex: bool, case: bool) -> Regex {
        SearchQuery {
            text: text.into(),
            regex,
            case_sensitive: case,
        }
        .compile()
        .unwrap()
        .unwrap()
    }

    #[test]
    fn finds_plain_text_in_history_and_screen() {
        let mut s = String::new();
        for i in 0..20 {
            s.push_str(&format!("line {i} Error here\n"));
        }
        let t = term_with(&s, 5);
        let hits = search(&t, &q("error", false, false));
        assert_eq!(hits.len(), 20);
        assert!(hits[0].line < 0, "oldest hit is in history");
        assert_eq!(hits[0].start, 7);
        assert_eq!(hits[0].end, 12);
        assert!(search(&t, &q("error", false, true)).is_empty());
    }

    #[test]
    fn regex_and_escaping() {
        let t = term_with("a.b axb\n", 4);
        assert_eq!(search(&t, &q("a.b", false, false)).len(), 1);
        assert_eq!(search(&t, &q("a.b", true, false)).len(), 2);
        assert!(
            SearchQuery {
                text: "(".into(),
                regex: true,
                case_sensitive: false
            }
            .compile()
            .is_err()
        );
    }

    #[test]
    fn wide_characters_map_to_cells() {
        let t = term_with("日本 ok\n", 3);
        let hits = search(&t, &q("ok", false, false));
        assert_eq!(hits[0].start, 5, "two wide chars + space = 5 cells");
        assert_eq!(hits[0].end, 7);
    }

    #[test]
    fn offset_centres_history_lines() {
        assert_eq!(offset_for_line(3, 10, 50), 0);
        assert_eq!(offset_for_line(-10, 10, 50), 15);
        assert_eq!(offset_for_line(-48, 10, 50), 50);
    }
}
