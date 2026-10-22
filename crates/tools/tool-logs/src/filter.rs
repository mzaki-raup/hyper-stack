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

    /// The chip as it reads on screen: `level ≥ WARN`, `!regex: timeout`.
    pub fn label(&self) -> String {
        let body = match &self.kind {
            ChipKind::Contains(t) => format!("\"{t}\""),
            ChipKind::Regex(r) => format!("/{r}/"),
            ChipKind::LevelAtLeast(l) => format!("level ≥ {}", l.label()),
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
pub struct Highlight {
    pub pattern: String,
    pub colour: [u8; 3],
    pub bold: bool,
    pub enabled: bool,
}

/// Highlights compiled for the frame.
pub struct Highlights {
    rules: Vec<(Regex, [u8; 3], bool)>,
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
                        .map(|re| (re, h.colour, h.bold))
                })
                .collect(),
        }
    }

    /// The first rule that matches the record's raw text.
    pub fn paint(&self, r: &Record) -> Option<([u8; 3], bool)> {
        self.rules
            .iter()
            .find(|(re, _, _)| re.is_match(&r.raw))
            .map(|(_, c, b)| (*c, *b))
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

    #[test]
    fn highlights_pick_the_first_matching_rule() {
        let h = Highlights::compile(&[
            Highlight {
                pattern: "duration: (\\d{3,}) ms".into(),
                colour: [255, 140, 0],
                bold: true,
                enabled: true,
            },
            Highlight {
                pattern: "timeout".into(),
                colour: [255, 0, 0],
                bold: false,
                enabled: false,
            },
        ]);
        let slow = parse(Format::Plain, 1, "LOG: duration: 812 ms");
        assert_eq!(h.paint(&slow), Some(([255, 140, 0], true)));
        let t = parse(Format::Plain, 1, "redis timeout");
        assert_eq!(h.paint(&t), None, "a disabled rule paints nothing");
    }
}
