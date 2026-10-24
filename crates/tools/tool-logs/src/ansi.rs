//! Taking the terminal escapes out of a log line (owner's request,
//! 2026-10-24).
//!
//! A great many programs colour their output when they think a terminal
//! is watching, and a container's log keeps those bytes for ever:
//! portainer writes
//!
//! ```text
//! \u{1b}[90m2026/10/23 10:28AM\u{1b}[0m \u{1b}[32mINF\u{1b}[0m ...
//! ```
//!
//! which a log viewer shows literally as `[90m2026/10/23 ... [32mINF`.
//! Worse than ugly: the escapes sit between the timestamp and the level
//! where every parser looks for them, so a coloured line is also a line
//! that will not parse.
//!
//! This strips rather than renders. Reconstructing the colour would be
//! a different feature, and the one thing it must not do is guess: a
//! log that merely *contains* the text `[90m` with no escape byte in
//! front of it is data, and is left exactly as it is.

use std::borrow::Cow;

/// The escape that starts all of this.
const ESC: char = '\u{1b}';
/// A terminal's bell, which programs emit to end an OSC string and
/// which has no business in a log line either.
const BEL: char = '\u{7}';

/// A line with its terminal escapes removed.
///
/// Borrows when there is nothing to do, which is almost every line of
/// almost every log: the cost on a clean 4 k line is one scan for a
/// byte that is not there.
pub fn strip(line: &str) -> Cow<'_, str> {
    if !line.contains(ESC) && !line.contains(BEL) {
        return Cow::Borrowed(line);
    }
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            // A stray bell is noise wherever it appears.
            BEL => {}
            ESC => skip_escape(&mut chars),
            _ => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Consume one escape sequence, having just taken its `ESC`.
///
/// An unterminated sequence - a line cut mid-escape, which a tail can
/// hand us - consumes the rest of the line rather than printing its
/// innards, because half an escape is not text either.
fn skip_escape(chars: &mut std::str::Chars<'_>) {
    match chars.next() {
        // CSI: parameters and intermediates, then one final byte in
        // 0x40..=0x7E. This is colour, cursor movement, erase - the
        // overwhelming majority of what reaches a log.
        Some('[') => {
            for c in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c) {
                    return;
                }
            }
        }
        // OSC (window title, hyperlinks) runs to a BEL or to ST
        // (`ESC \`), and may carry anything in between.
        Some(']') => {
            let mut prev_esc = false;
            for c in chars.by_ref() {
                if c == BEL || (prev_esc && c == '\\') {
                    return;
                }
                prev_esc = c == ESC;
            }
        }
        // Every other escape is two bytes (`ESC c`, `ESC (B`, …); the
        // one already taken is the whole of it. The charset selectors
        // `ESC ( ` and `ESC ) ` take one more.
        Some('(') | Some(')') | Some('*') | Some('+') => {
            chars.next();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common case has to cost nothing: no allocation, same bytes.
    #[test]
    fn a_line_with_no_escapes_is_borrowed_unchanged() {
        let line = "2026-10-23T10:28:11Z INFO starting HTTPS server bind_address=:9443";
        match strip(line) {
            Cow::Borrowed(s) => assert_eq!(s, line),
            Cow::Owned(_) => panic!("a clean line must not be copied"),
        }
    }

    /// The line that prompted this, as portainer actually writes it.
    #[test]
    fn a_real_portainer_line_comes_out_readable() {
        let line = "\u{1b}[90m2026/10/23 10:28AM\u{1b}[0m \u{1b}[32mINF\u{1b}[0m \
                    \u{1b}[1mgithub.com/portainer/portainer/api/http/server.go:367\u{1b}[0m\
                    \u{1b}[36m >\u{1b}[0m starting HTTPS server | \u{1b}[36mbind_address=\u{1b}[0m:9443";
        assert_eq!(
            strip(line),
            "2026/10/23 10:28AM INF github.com/portainer/portainer/api/http/server.go:367 > \
             starting HTTPS server | bind_address=:9443"
        );
    }

    /// Colour is not the only thing that reaches a log: a progress bar
    /// leaves cursor movement and erase sequences behind.
    #[test]
    fn cursor_and_erase_sequences_go_too() {
        assert_eq!(strip("a\u{1b}[2Kb\u{1b}[3;10Hc\u{1b}[?25ld"), "abcd");
        // A long parameter list is still one sequence.
        assert_eq!(strip("\u{1b}[38;2;255;128;0mwarm\u{1b}[0m"), "warm");
    }

    /// OSC ends with a bell or with ST, and carries arbitrary text that
    /// must not leak into the line.
    #[test]
    fn osc_strings_are_taken_whole() {
        assert_eq!(strip("\u{1b}]0;a title\u{7}after"), "after");
        assert_eq!(strip("\u{1b}]8;;https://example.com\u{1b}\\link"), "link");
        // A lone bell is noise on its own.
        assert_eq!(strip("ding\u{7}dong"), "dingdong");
    }

    /// **The one thing this must never do is guess.** A log that
    /// contains the *text* of an escape, with no escape byte, is data.
    #[test]
    fn bracketed_text_without_an_escape_byte_is_left_alone() {
        let line = "the regex [90m] matched, see [0m and [1;32m";
        assert_eq!(strip(line), line);
        assert!(matches!(strip(line), Cow::Borrowed(_)));
    }

    /// A tail can hand us a line cut in the middle of an escape. Half an
    /// escape is not text, and it must not panic.
    #[test]
    fn an_unterminated_escape_does_not_panic_or_leak() {
        assert_eq!(strip("before\u{1b}"), "before");
        assert_eq!(strip("before\u{1b}[38;2;255"), "before");
        assert_eq!(strip("before\u{1b}]0;no terminator"), "before");
    }

    /// Multi-byte text around the escapes must survive intact - the
    /// stripper walks characters, not bytes.
    #[test]
    fn text_around_the_escapes_keeps_its_characters() {
        assert_eq!(strip("\u{1b}[31mérreur · 失败\u{1b}[0m"), "érreur · 失败");
    }

    /// Charset selectors take the byte after them; left in, the `B` of
    /// `ESC ( B` would appear in the text.
    #[test]
    fn charset_selectors_take_their_argument() {
        assert_eq!(strip("\u{1b}(Bplain"), "plain");
        assert_eq!(strip("\u{1b})0plain"), "plain");
    }
}
