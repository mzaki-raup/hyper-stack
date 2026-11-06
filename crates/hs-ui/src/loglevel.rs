//! One set of log-level colours (CR-0077): every view that shows log
//! lines paints a level the way the Logs activity does, through
//! [`colour`], never its own palette.

use egui::Color32;
use hs_core::loglevel::{Level, level_span};

use crate::theme::Theme;

/// The colour of a level: errors and fatals in the error colour, warnings
/// in the warning colour, info in the accent, debug and trace muted.
pub fn colour(theme: &Theme, level: Level) -> Color32 {
    match level {
        Level::Trace => theme.text_muted,
        Level::Debug => theme.text_muted,
        Level::Info => theme.accent,
        Level::Warn => theme.warning,
        Level::Error | Level::Fatal => theme.error,
    }
}

/// The level colour for a whole line, when a level word (or a
/// `level=...` field) can be found in it; `None` for a line with no level.
pub fn line_colour(theme: &Theme, text: &str) -> Option<Color32> {
    level_span(text).map(|(_, l)| colour(theme, l))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_alias_has_its_levels_colour() {
        let t = Theme::dark();
        for (name, level) in Level::ALIASES {
            assert_eq!(
                line_colour(&t, &format!("12:00 {name} x")),
                Some(colour(&t, *level)),
                "{name}"
            );
        }
    }

    #[test]
    fn line_colour_finds_the_level_in_common_shapes() {
        let t = Theme::dark();
        assert_eq!(line_colour(&t, "2026-11-06 12:00 WARN x"), Some(t.warning));
        assert_eq!(line_colour(&t, "level=error"), Some(t.error));
        assert_eq!(line_colour(&t, "[DBG] starting"), Some(t.text_muted));
        assert_eq!(line_colour(&t, "INFO ready"), Some(t.accent));
        assert_eq!(line_colour(&t, "just some plain text"), None);
    }
}
