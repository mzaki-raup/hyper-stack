//! One set of log-level colours (CR-0077): every view that shows log
//! lines paints a level the way the Logs activity does, through
//! [`colour`], never its own palette.

use egui::Color32;
use egui::text::{LayoutJob, TextFormat};
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

/// Where a line's level word sits and which level it names, as
/// [`level_span`] finds it. Callers read it once per line and keep it.
pub type Span = (std::ops::Range<usize>, Level);

/// A line laid out the way the Logs activity paints it: `base` for the
/// whole line except the level word at `span`, which alone wears its
/// level's colour (owner, 2026-11-07). `span` is precomputed by the
/// caller, so a cached line never re-runs the regex; one that does not
/// fit `text` is ignored and the line is drawn plain.
pub fn line_job(
    theme: &Theme,
    text: &str,
    span: Option<&Span>,
    font: egui::FontId,
    base: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let plain = TextFormat::simple(font.clone(), base);
    let parts = span.and_then(|(r, l)| {
        Some((
            text.get(..r.start)?,
            text.get(r.clone())?,
            text.get(r.end..)?,
            *l,
        ))
    });
    match parts {
        Some((before, word, after, l)) => {
            job.append(before, 0.0, plain.clone());
            job.append(word, 0.0, TextFormat::simple(font, colour(theme, l)));
            job.append(after, 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job
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

    #[test]
    fn line_job_colours_only_the_level_word() {
        let t = Theme::dark();
        let line = "2026-11-07 12:00 WARN disk usage 81%";
        let span = level_span(line);
        let job = line_job(
            &t,
            line,
            span.as_ref(),
            egui::FontId::monospace(12.0),
            t.text,
        );
        assert_eq!(job.text, line);
        let coloured: Vec<&str> = job
            .sections
            .iter()
            .filter(|s| s.format.color == t.warning)
            .map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0])
            .collect();
        assert_eq!(coloured, ["WARN"]);
        assert!(
            job.sections
                .iter()
                .filter(|s| !s.byte_range.is_empty())
                .all(|s| s.format.color == t.warning || s.format.color == t.text)
        );
        // No level word: the whole line in the base colour.
        let plain = line_job(
            &t,
            "just text",
            None,
            egui::FontId::monospace(12.0),
            t.error,
        );
        assert!(plain.sections.iter().all(|s| s.format.color == t.error));
    }
}
