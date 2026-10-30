//! Filter chips and highlight rules (docs/tools/logs.md §7). Chips combine
//! with AND; each can be negated. Highlights are independent of filters:
//! a regex paints the rows it matches.

use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::parse::{Level, Record};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChipKind {
    /// Case-insensitive substring of the raw record.
    Contains(String),
    Regex(String),
    LevelAtLeast(Level),
    /// A named field equals the value (structured formats).
    Field(String, String),
    After(DateTime<Utc>),
    Before(DateTime<Utc>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    pub kind: ChipKind,
    pub negate: bool,
}

impl Chip {
    pub fn new(kind: ChipKind) -> Self {
        Self {
            kind,
            negate: false,
        }
    }

    /// The chip as it reads on screen, in the canonical spelling:
    /// `level ≥ WARN`, `!regex: timeout`. Prefer [`Chip::label_with`]
    /// where the source's own spelling is known.
    pub fn label(&self) -> String {
        self.label_with(&crate::parse::LevelAliases::default())
    }

    /// The chip as it reads on screen, naming a level the way this source
    /// spells it: `level ≥ warning` for a log that writes `level=warning`,
    /// `level ≥ WARN` for one that writes WARN (owner's request,
    /// 2026-10-23). Only the label changes; the comparison is the same
    /// ordered one either way, so a preset still means what it meant.
    pub fn label_with(&self, aliases: &crate::parse::LevelAliases) -> String {
        let body = match &self.kind {
            ChipKind::Contains(t) => format!("\"{t}\""),
            ChipKind::Regex(r) => format!("/{r}/"),
            ChipKind::LevelAtLeast(l) => format!("level ≥ {}", aliases.name(*l)),
            ChipKind::Field(k, v) => format!("{k} = {v}"),
            ChipKind::After(t) => format!("after {}", t.format("%H:%M:%S")),
            ChipKind::Before(t) => format!("before {}", t.format("%H:%M:%S")),
        };
        if self.negate {
            format!("NOT {body}")
        } else {
            body
        }
    }
}

/// A compiled chip set.
#[derive(Default)]
pub struct Filter {
    chips: Vec<(Chip, Option<Regex>, String)>,
}

impl Filter {
    pub fn compile(chips: &[Chip]) -> Self {
        Self {
            chips: chips
                .iter()
                .map(|c| {
                    let re = match &c.kind {
                        ChipKind::Regex(r) => Regex::new(&format!("(?i){r}")).ok(),
                        _ => None,
                    };
                    let lower = match &c.kind {
                        ChipKind::Contains(t) => t.to_lowercase(),
                        _ => String::new(),
                    };
                    (c.clone(), re, lower)
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chips.is_empty()
    }

    pub fn matches(&self, r: &Record) -> bool {
        self.chips.iter().all(|(chip, re, lower)| {
            let hit = match &chip.kind {
                ChipKind::Contains(_) => r.raw.to_lowercase().contains(lower.as_str()),
                ChipKind::Regex(_) => re.as_ref().is_some_and(|re| re.is_match(&r.raw)),
                ChipKind::LevelAtLeast(l) => r.level.is_some_and(|rl| rl >= *l),
                ChipKind::Field(k, v) => r.field(k).is_some_and(|fv| fv == v),
                ChipKind::After(t) => r.ts.is_some_and(|ts| ts >= *t),
                ChipKind::Before(t) => r.ts.is_some_and(|ts| ts <= *t),
            };
            hit != chip.negate
        })
    }
}

/// A regex that paints matching records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Highlight {
    pub pattern: String,
    pub colour: [u8; 3],
    pub bold: bool,
    pub enabled: bool,
    /// Post a matching record to the Problems dock tab
    /// (docs/tools/logs.md §15.2). Off by default: a highlight is a way
    /// of seeing, and turning every one of them into an inbox entry
    /// would make the inbox useless.
    pub report: bool,
    /// Seconds between two reports of the same shape from this rule.
    /// Zero uses `hs_core::problems::COOLDOWN`.
    pub cooldown_secs: u32,
    /// Raise an alert on a match: a toast, and a count on the tab while
    /// it is not in front (docs/tools/logs.md §10). Same cooldown.
    #[serde(default)]
    pub alert: bool,
}

impl Default for Highlight {
    fn default() -> Self {
        Self {
            pattern: String::new(),
            colour: [220, 80, 80],
            bold: false,
            enabled: true,
            report: false,
            cooldown_secs: 0,
            alert: false,
        }
    }
}

/// Highlights compiled for the frame.
pub struct Highlights {
    rules: Vec<Compiled>,
}

struct Compiled {
    re: Regex,
    colour: [u8; 3],
    bold: bool,
    report: bool,
    cooldown_secs: u32,
    alert: bool,
}

impl Highlights {
    pub fn compile(rules: &[Highlight]) -> Self {
        Self {
            rules: rules
                .iter()
                .filter(|h| h.enabled)
                .filter_map(|h| {
                    Regex::new(&format!("(?i){}", h.pattern))
                        .ok()
                        .map(|re| Compiled {
                            re,
                            colour: h.colour,
                            bold: h.bold,
                            report: h.report,
                            cooldown_secs: h.cooldown_secs,
                            alert: h.alert,
                        })
                })
                .collect(),
        }
    }

    /// The first rule that matches the record's raw text.
    pub fn paint(&self, r: &Record) -> Option<([u8; 3], bool)> {
        self.rules
            .iter()
            .find(|c| c.re.is_match(&r.raw))
            .map(|c| (c.colour, c.bold))
    }

    /// Whether any **reporting** rule matches, and its cooldown in
    /// seconds. Separate from `paint` because a record can be painted by
    /// a rule that does not report, and the first match is not
    /// necessarily the reporting one.
    pub fn reports(&self, r: &Record) -> Option<u32> {
        self.rules
            .iter()
            .find(|c| c.report && c.re.is_match(&r.raw))
            .map(|c| c.cooldown_secs)
    }

    /// Whether any rule reports at all, so a tab with none can skip the
    /// scan entirely.
    pub fn any_reporting(&self) -> bool {
        self.rules.iter().any(|c| c.report)
    }

    /// The first alerting rule that matches: its index and cooldown.
    pub fn alerts(&self, r: &Record) -> Option<(usize, u32)> {
        self.rules
            .iter()
            .enumerate()
            .find(|(_, c)| c.alert && c.re.is_match(&r.raw))
            .map(|(i, c)| (i, c.cooldown_secs))
    }

    pub fn any_alerting(&self) -> bool {
        self.rules.iter().any(|c| c.alert)
    }
}

/// A saved set of chips, under a name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub chips: Vec<Chip>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Format, parse};

    fn rec(line: &str) -> Record {
        parse(Format::Logfmt, 1, line)
    }

    #[test]
    fn chips_combine_with_and_and_negate() {
        let a = rec(r#"time=2026-10-22T14:32:01Z level=warn msg="redis timeout" svc=cache"#);
        let b = rec(r#"time=2026-10-22T14:32:05Z level=info msg="ok" svc=orders"#);
        let f = Filter::compile(&[
            Chip::new(ChipKind::LevelAtLeast(Level::Warn)),
            Chip::new(ChipKind::Contains("TIMEOUT".into())),
        ]);
        assert!(f.matches(&a) && !f.matches(&b));
        let f = Filter::compile(&[Chip {
            kind: ChipKind::Field("svc".into(), "cache".into()),
            negate: true,
        }]);
        assert!(!f.matches(&a) && f.matches(&b));
        let f = Filter::compile(&[Chip::new(ChipKind::Regex("re.is".into()))]);
        assert!(f.matches(&a) && !f.matches(&b));
        let f = Filter::compile(&[Chip::new(ChipKind::After(
            crate::parse::parse_timestamp("2026-10-22T14:32:03Z").unwrap(),
        ))]);
        assert!(!f.matches(&a) && f.matches(&b));
        assert_eq!(
            Chip {
                kind: ChipKind::LevelAtLeast(Level::Error),
                negate: true
            }
            .label(),
            "NOT level ≥ ERROR"
        );
    }

    /// `level >= WARN` is an ordered comparison, not an equality, and it
    /// has to work on a level that came out of a journalctl payload's
    /// `level=warning` rather than out of a level column. Owner's point,
    /// 2026-10-23.
    #[test]
    fn level_at_least_is_ordered_and_reads_a_journalctl_level() {
        let line = |lvl: &str| {
            format!(
                r#"Oct 23 09:00:01 web-01 dockerd[1234]: time="2026-10-23T09:00:01Z" level={lvl} msg="x""#
            )
        };
        let rec = |lvl: &str| parse(Format::Syslog, 1, &line(lvl));

        let info = rec("info");
        let warning = rec("warning");
        let err = rec("error");
        assert_eq!(warning.level, Some(Level::Warn), "warning is WARN");

        let at_least_warn = Filter::compile(&[Chip::new(ChipKind::LevelAtLeast(Level::Warn))]);
        assert!(!at_least_warn.matches(&info), "info is below warning");
        assert!(at_least_warn.matches(&warning), "warning is included");
        assert!(at_least_warn.matches(&err), "error is above warning");

        let at_least_error = Filter::compile(&[Chip::new(ChipKind::LevelAtLeast(Level::Error))]);
        assert!(!at_least_error.matches(&warning));
        assert!(at_least_error.matches(&err));
    }

    #[test]
    fn highlights_pick_the_first_matching_rule() {
        let h = Highlights::compile(&[
            Highlight {
                pattern: "duration: (\\d{3,}) ms".into(),
                colour: [255, 140, 0],
                bold: true,
                enabled: true,
                ..Default::default()
            },
            Highlight {
                pattern: "timeout".into(),
                colour: [255, 0, 0],
                bold: false,
                enabled: false,
                ..Default::default()
            },
        ]);
        let slow = parse(Format::Plain, 1, "LOG: duration: 812 ms");
        assert_eq!(h.paint(&slow), Some(([255, 140, 0], true)));
        let t = parse(Format::Plain, 1, "redis timeout");
        assert_eq!(h.paint(&t), None, "a disabled rule paints nothing");
    }

    /// §15.2: reporting is a separate question from painting. A record
    /// can be painted by a rule that does not report, and the first
    /// rule that matches is not necessarily the reporting one.
    #[test]
    fn reporting_is_asked_separately_from_painting() {
        let h = Highlights::compile(&[
            Highlight {
                pattern: "timeout".into(),
                colour: [1, 2, 3],
                report: false,
                ..Default::default()
            },
            Highlight {
                pattern: "timeout".into(),
                colour: [9, 9, 9],
                report: true,
                cooldown_secs: 30,
                ..Default::default()
            },
        ]);
        let r = parse(Format::Plain, 1, "redis timeout talking to cache");
        assert_eq!(
            h.paint(&r),
            Some(([1, 2, 3], false)),
            "the first match paints"
        );
        assert_eq!(
            h.reports(&r),
            Some(30),
            "but the reporting rule is the one found for reporting"
        );
        assert!(h.any_reporting());
    }

    /// A tab whose rules never report must be able to skip the scan
    /// entirely, so the common case costs nothing.
    #[test]
    fn a_tab_with_no_reporting_rules_says_so() {
        let h = Highlights::compile(&[Highlight {
            pattern: "warn".into(),
            ..Default::default()
        }]);
        assert!(!h.any_reporting(), "off by default on every rule");
        let r = parse(Format::Plain, 1, "a warn line");
        assert!(h.paint(&r).is_some(), "it still paints");
        assert_eq!(h.reports(&r), None);
    }

    /// A disabled rule does nothing at all, reporting included.
    #[test]
    fn a_disabled_rule_does_not_report() {
        let h = Highlights::compile(&[Highlight {
            pattern: "boom".into(),
            enabled: false,
            report: true,
            ..Default::default()
        }]);
        assert!(!h.any_reporting());
        let r = parse(Format::Plain, 1, "boom");
        assert_eq!(h.reports(&r), None);
    }
}
