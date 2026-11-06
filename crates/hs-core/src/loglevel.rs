//! The log level model every view that shows log lines shares
//! (CR-0077): the normalised [`Level`], its aliases, and where the level
//! word sits in a line. Moved here from the Logs activity so the other
//! activities colour their log lines the same way without depending on it.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Normalised severity, ordered so `level >= WARN` is a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Level {
    pub const ALL: [Level; 6] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
        Level::Fatal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Warn => "WARN",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }

    /// Many spellings into one. This is the single list: `Level::parse`
    /// reads it, and `LEVEL_TOKEN` - the regex that finds a level in a
    /// plain or syslog line - is built from it, so the two can never
    /// disagree about what a level looks like. They had: the parser knew
    /// `WRN`, `DBG`, `INF` and `TRC`, the scanner did not, so a log that
    /// used the short forms showed no level at all (owner's report,
    /// 2026-10-23).
    ///
    /// Syslog's numeric severities are deliberately absent: a bare digit
    /// in prose is not a level, so they are handled by `parse` alone,
    /// where the caller already knows the field is one.
    pub const ALIASES: &'static [(&'static str, Level)] = &[
        ("TRACE", Level::Trace),
        ("TRC", Level::Trace),
        ("VERBOSE", Level::Trace),
        ("FINEST", Level::Trace),
        ("DEBUG", Level::Debug),
        ("DBG", Level::Debug),
        ("FINER", Level::Debug),
        ("FINE", Level::Debug),
        ("INFORMATION", Level::Info),
        ("INFO", Level::Info),
        ("INF", Level::Info),
        ("NOTICE", Level::Info),
        ("WARNING", Level::Warn),
        ("WARN", Level::Warn),
        ("WRN", Level::Warn),
        ("ERROR", Level::Error),
        ("ERR", Level::Error),
        ("SEVERE", Level::Error),
        ("CRITICAL", Level::Fatal),
        ("CRIT", Level::Fatal),
        ("EMERGENCY", Level::Fatal),
        ("EMERG", Level::Fatal),
        ("FATAL", Level::Fatal),
        ("PANIC", Level::Fatal),
        ("ALERT", Level::Fatal),
    ];

    /// Many spellings into one: `WARNING`, `ERR`, `CRIT`, Go's `warn`,
    /// Python's `CRITICAL`, syslog's numeric severities (0-7).
    pub fn parse(token: &str) -> Option<Level> {
        let t = token.trim().trim_matches(|c: char| !c.is_alphanumeric());
        let upper = t.to_ascii_uppercase();
        if let Some((_, l)) = Self::ALIASES.iter().find(|(name, _)| *name == upper) {
            return Some(*l);
        }
        Some(match upper.as_str() {
            "7" => Level::Trace,
            "6" => Level::Debug,
            "5" => Level::Info,
            "4" => Level::Warn,
            "3" => Level::Error,
            "2" | "1" | "0" => Level::Fatal,
            _ => return None,
        })
    }
}

/// Where the level word sits in `text`, so the list can paint that word
/// in the level's colour whatever the format (owner's request,
/// 2026-10-23). `LEVEL_TOKEN` alone is not enough: it requires the word to
/// follow a space or a bracket, and the commonest shape of all is
/// `level=info`, where it follows an equals sign or a quote.
pub fn level_span(text: &str) -> Option<(std::ops::Range<usize>, Level)> {
    // `level=info`, `severity: "warn"`, `lvl='dbg'` and the like first:
    // the word after a level key is the level even when the same word
    // elsewhere in the line would not be.
    if let Some(c) = LEVEL_ASSIGN.captures(text)
        && let Some(m) = c.get(1)
        && let Some(l) = Level::parse(m.as_str())
    {
        return Some((m.range(), l));
    }
    let c = LEVEL_TOKEN.captures(text)?;
    let m = c.get(1)?;
    Level::parse(m.as_str()).map(|l| (m.range(), l))
}

/// `level=info`, `level: warn`, `"severity":"error"`: a level key, then
/// the word that is its value.
static LEVEL_ASSIGN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)(?:level|lvl|severity|loglevel|log_level|levelname)"?\s*[:=]\s*"?'?([A-Za-z]+)"#,
    )
    .expect("level assignment")
});

/// The regex that finds a level word in a plain or syslog line. Built
/// from [`Level::ALIASES`], longest spelling first so the alternation
/// prefers `WARNING` over `WARN` and `INFORMATION` over `INFO` rather than
/// stopping at the shorter prefix.
pub static LEVEL_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    let mut names: Vec<&str> = Level::ALIASES.iter().map(|(n, _)| *n).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    Regex::new(&format!(
        r"(?i)(?:^|[\s\[\(\|:])({})(?:$|[\s\]\)\|:])",
        names.join("|")
    ))
    .expect("level alternation")
});

#[cfg(test)]
mod tests {
    use super::*;

    /// The level word is painted in the level's colour wherever it sits,
    /// and `level=info` is the commonest place of all. Owner's report,
    /// 2026-10-23.
    #[test]
    fn the_level_word_is_located_in_every_shape_a_line_writes_it() {
        let cases = [
            (r#"time="..." level=info msg="up""#, "info", Level::Info),
            (
                r#"time="..." level=warning msg="x""#,
                "warning",
                Level::Warn,
            ),
            (r#"{"level":"error","msg":"x"}"#, "error", Level::Error),
            ("ts=1 lvl=dbg msg=x", "dbg", Level::Debug),
            ("severity: CRITICAL something", "CRITICAL", Level::Fatal),
            (
                "2026-10-23 09:00:01 WARN OrderSvc slow",
                "WARN",
                Level::Warn,
            ),
            ("09:00:01 [Trace] entering", "Trace", Level::Trace),
        ];
        for (line, word, want) in cases {
            let (span, got) = level_span(line).unwrap_or_else(|| panic!("no level in {line}"));
            assert_eq!(got, want, "{line}");
            assert_eq!(&line[span], word, "{line}");
        }
        // A line with no level has no span to paint.
        assert!(level_span("just some text about levels of water").is_none());
        // The level key wins over a level word elsewhere in the line.
        let line = "msg=\"ERROR was logged earlier\" level=info";
        let (span, got) = level_span(line).unwrap();
        assert_eq!(got, Level::Info);
        assert_eq!(&line[span], "info");
    }

    #[test]
    fn aliases_map_to_their_levels() {
        for (name, want) in Level::ALIASES {
            assert_eq!(Level::parse(name), Some(*want), "{name}");
        }
        assert_eq!(Level::parse("WARNING"), Some(Level::Warn));
        assert_eq!(Level::parse("err"), Some(Level::Error));
        assert_eq!(Level::parse("CRIT"), Some(Level::Fatal));
        assert_eq!(Level::parse("Notice"), Some(Level::Info));
        assert_eq!(Level::parse("verbose"), Some(Level::Trace));
        assert_eq!(Level::parse("3"), Some(Level::Error), "syslog severity");
        assert_eq!(Level::parse("water"), None);
    }
}
