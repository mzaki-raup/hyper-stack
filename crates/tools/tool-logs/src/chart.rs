//! Records over time (docs/tools/logs.md §10 and §15.6): the histogram
//! the status line draws, and the arithmetic that makes it a navigator
//! rather than decoration.
//!
//! Two decisions shape everything here.
//!
//! It counts the **filtered** window, not the whole log, because the
//! question is "when did *this* happen", not "when was anything logged".
//!
//! And the bucket is **chosen from the span**, not fixed at a minute. A
//! minute is right for an incident and useless for a week: 10 080
//! buckets in a 300 px strip is one bar per thirty-three pixels of
//! nothing. [`Bucket::pick`] takes the span and returns a round unit a
//! person recognises - a second, a minute, an hour, a day - that leaves
//! a readable number of bars.

use chrono::{DateTime, DurationRound, TimeDelta, Utc};

use crate::parse::{Level, Record};

/// How wide one bar is. Only round units a reader recognises: a bucket
/// of "37 seconds" would make the axis unreadable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket(pub TimeDelta);

/// The units a bucket may take, smallest first.
const UNITS_SECS: &[i64] = &[
    1,
    5,
    15,
    30,
    60,
    5 * 60,
    15 * 60,
    30 * 60,
    3600,
    3 * 3600,
    6 * 3600,
    12 * 3600,
    86_400,
    7 * 86_400,
];

/// How many bars the strip aims for. Enough to show shape, few enough
/// that each is wide enough to hit with a pointer.
pub const TARGET_BARS: i64 = 120;

impl Bucket {
    /// The smallest round unit that keeps the bar count at or under
    /// [`TARGET_BARS`]. A span of nothing still gets a bucket, so the
    /// caller never divides by zero.
    pub fn pick(span: TimeDelta) -> Self {
        let secs = span.num_seconds().max(1);
        for &u in UNITS_SECS {
            if secs / u <= TARGET_BARS {
                return Self(TimeDelta::seconds(u));
            }
        }
        // Longer than two years of weekly bars: widen to fit rather than
        // return something that would make thousands of bars.
        let needed = (secs / TARGET_BARS).max(1);
        Self(TimeDelta::seconds(needed))
    }

    /// The label for this width, for the axis and the tooltip.
    pub fn label(self) -> String {
        let s = self.0.num_seconds();
        match s {
            s if s < 60 => format!("{s}s"),
            s if s < 3600 => format!("{}m", s / 60),
            s if s < 86_400 => format!("{}h", s / 3600),
            s => format!("{}d", s / 86_400),
        }
    }

    /// The start of the bucket `at` falls in: the timestamp floored to
    /// this width, so bars line up on round boundaries rather than on
    /// whenever the first record happened to arrive.
    pub fn floor(self, at: DateTime<Utc>) -> DateTime<Utc> {
        at.duration_trunc(self.0).unwrap_or(at)
    }
}

/// One bar: when it starts, and how many records of each level fall in
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bar {
    pub start: DateTime<Utc>,
    /// Counts by level, in `Level::ALL` order.
    pub levels: [u32; 6],
    /// Records with no level at all, which are most of a plain log.
    pub none: u32,
    /// The first filtered position in this bar, for the click-to-jump.
    pub first: usize,
}

impl Bar {
    pub fn total(&self) -> u32 {
        self.levels.iter().sum::<u32>() + self.none
    }
    /// The worst level in the bar, which is the colour the bar takes:
    /// one error in a minute of chatter is the thing worth seeing.
    pub fn worst(&self) -> Option<Level> {
        Level::ALL
            .iter()
            .enumerate()
            .rev()
            .find(|(i, _)| self.levels[*i] > 0)
            .map(|(_, l)| *l)
    }
}

/// The histogram of a filtered window.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Histogram {
    pub bars: Vec<Bar>,
    pub bucket_secs: i64,
    /// The span the bars cover, for mapping a pixel back to a time.
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    /// Filtered records that carry no timestamp, so they are in no bar.
    /// Reported rather than hidden: a chart that silently drops half a
    /// plain log would be a lie.
    pub undated: usize,
}

impl Histogram {
    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }
    /// The tallest bar, which the drawing scales to.
    pub fn peak(&self) -> u32 {
        self.bars.iter().map(Bar::total).max().unwrap_or(0)
    }
    pub fn bucket(&self) -> Bucket {
        Bucket(TimeDelta::seconds(self.bucket_secs.max(1)))
    }
}

/// Build the histogram of the records at `filtered` (indices into
/// `records`, in order). Empty buckets inside the span are kept, because
/// a gap is information: a log that stopped for ten minutes should look
/// like it stopped.
pub fn histogram(records: &[Record], filtered: &[usize]) -> Histogram {
    let mut first_ts = None;
    let mut last_ts = None;
    let mut undated = 0;
    for &i in filtered {
        match records.get(i).and_then(|r| r.ts) {
            Some(ts) => {
                if first_ts.is_none() {
                    first_ts = Some(ts);
                }
                // Not assumed sorted: a merged or out-of-order source
                // would otherwise give a negative span.
                first_ts = Some(first_ts.unwrap().min(ts));
                last_ts = Some(last_ts.map_or(ts, |l: DateTime<Utc>| l.max(ts)));
            }
            None => undated += 1,
        }
    }
    let (Some(from), Some(to)) = (first_ts, last_ts) else {
        return Histogram {
            undated,
            ..Default::default()
        };
    };
    let bucket = Bucket::pick(to - from);
    let start = bucket.floor(from);
    let span = (to - start).num_seconds().max(0);
    let count = (span / bucket.0.num_seconds()) as usize + 1;
    let mut bars: Vec<Bar> = (0..count)
        .map(|n| Bar {
            start: start + bucket.0 * (n as i32),
            first: usize::MAX,
            ..Default::default()
        })
        .collect();
    for (pos, &i) in filtered.iter().enumerate() {
        let Some(r) = records.get(i) else { continue };
        let Some(ts) = r.ts else { continue };
        let n = ((ts - start).num_seconds() / bucket.0.num_seconds()) as usize;
        let Some(bar) = bars.get_mut(n.min(count - 1)) else {
            continue;
        };
        match r.level {
            Some(l) => bar.levels[level_slot(l)] += 1,
            None => bar.none += 1,
        }
        bar.first = bar.first.min(pos);
    }
    Histogram {
        bars,
        bucket_secs: bucket.0.num_seconds(),
        from: Some(start),
        to: Some(to),
        undated,
    }
}

fn level_slot(l: Level) -> usize {
    Level::ALL.iter().position(|x| *x == l).unwrap_or(0)
}

/// A range the user dragged across the strip, as the times it means.
/// `from` is inclusive of the bucket it starts in and `to` is the end of
/// the bucket it ends in, so dragging across one bar selects that whole
/// bar rather than a zero-width instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

/// Turn two fractions of the strip's width (0.0 to 1.0, in any order)
/// into the time range they cover. `None` when the histogram has no
/// span to map onto.
pub fn range_at(h: &Histogram, a: f32, b: f32) -> Option<Range> {
    let (from_t, _) = (h.from?, h.to?);
    if h.bars.is_empty() {
        return None;
    }
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    let last = h.bars.len() - 1;
    let i = bar_at(h, lo)?;
    let j = bar_at(h, hi)?.min(last);
    let bucket = h.bucket().0;
    Some(Range {
        from: from_t + bucket * (i as i32),
        // The end of bar `j`, so a one-bar drag covers that bar.
        to: from_t + bucket * (j as i32) + bucket,
    })
}

/// Which bar a fraction of the width falls on.
pub fn bar_at(h: &Histogram, fraction: f32) -> Option<usize> {
    if h.bars.is_empty() {
        return None;
    }
    let n = (fraction.clamp(0.0, 1.0) * h.bars.len() as f32).floor() as usize;
    Some(n.min(h.bars.len() - 1))
}

/// How many records of each shape fall in a range (docs/tools/logs.md
/// §15.8). The key is `shape::key` of the message, so `took 31ms` and
/// `took 904ms` are one shape.
pub fn shapes_in(
    records: &[Record],
    filtered: &[usize],
    range: Option<Range>,
) -> std::collections::BTreeMap<String, u32> {
    let mut out = std::collections::BTreeMap::new();
    for &i in filtered {
        let Some(r) = records.get(i) else { continue };
        if let Some(range) = range {
            // A record with no timestamp cannot be placed in a window,
            // so it belongs to neither side of the comparison.
            let Some(ts) = r.ts else { continue };
            if ts < range.from || ts >= range.to {
                continue;
            }
        }
        *out.entry(crate::shape::key(&r.message)).or_insert(0) += 1;
    }
    out
}

/// One line of the window comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeDelta {
    pub shape: String,
    pub a: u32,
    pub b: u32,
}

impl ShapeDelta {
    /// B minus A, which is what the table sorts by.
    pub fn change(&self) -> i64 {
        i64::from(self.b) - i64::from(self.a)
    }
    pub fn only_in_b(&self) -> bool {
        self.a == 0 && self.b > 0
    }
    pub fn only_in_a(&self) -> bool {
        self.b == 0 && self.a > 0
    }
}

/// Compare two windows by shape, keeping only what moved by at least
/// `threshold`, biggest move first (docs/tools/logs.md §15.8). A shape
/// that appeared or vanished is always kept, whatever the threshold: it
/// is the most interesting row there is.
pub fn diff_windows(
    records: &[Record],
    filtered: &[usize],
    a: Range,
    b: Range,
    threshold: u32,
) -> Vec<ShapeDelta> {
    let left = shapes_in(records, filtered, Some(a));
    let right = shapes_in(records, filtered, Some(b));
    let mut out: Vec<ShapeDelta> = left
        .keys()
        .chain(right.keys())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|shape| ShapeDelta {
            shape: shape.clone(),
            a: left.get(shape).copied().unwrap_or(0),
            b: right.get(shape).copied().unwrap_or(0),
        })
        .filter(|d| {
            d.only_in_a() || d.only_in_b() || d.change().unsigned_abs() >= u64::from(threshold)
        })
        .collect();
    // Biggest move first, and a stable order for equal moves so the
    // table does not reshuffle itself between frames.
    out.sort_by(|x, y| {
        y.change()
            .abs()
            .cmp(&x.change().abs())
            .then(x.shape.cmp(&y.shape))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + secs, 0).unwrap()
    }

    fn rec(secs: i64, level: Option<Level>, message: &str) -> Record {
        Record {
            line: 1,
            raw: message.to_string(),
            ts: Some(at(secs)),
            level,
            message: message.to_string(),
            fields: Vec::new(),
        }
    }

    fn all(n: usize) -> Vec<usize> {
        (0..n).collect()
    }

    /// A minute is right for an incident and useless for a week: the
    /// bucket has to follow the span, and only to units a reader
    /// recognises.
    #[test]
    fn the_bucket_follows_the_span_in_round_units() {
        assert_eq!(Bucket::pick(TimeDelta::seconds(30)).label(), "1s");
        assert_eq!(Bucket::pick(TimeDelta::minutes(30)).label(), "15s");
        assert_eq!(Bucket::pick(TimeDelta::hours(2)).label(), "1m");
        assert_eq!(Bucket::pick(TimeDelta::days(1)).label(), "15m");
        assert_eq!(Bucket::pick(TimeDelta::days(7)).label(), "3h");
        assert_eq!(Bucket::pick(TimeDelta::days(365)).label(), "7d");
        // Never zero, whatever it is handed.
        assert!(Bucket::pick(TimeDelta::zero()).0.num_seconds() >= 1);
    }

    #[test]
    fn every_span_stays_under_the_target_bar_count() {
        for secs in [1, 59, 61, 3599, 86_401, 7 * 86_400, 400 * 86_400] {
            let b = Bucket::pick(TimeDelta::seconds(secs));
            let bars = secs / b.0.num_seconds();
            assert!(
                bars <= TARGET_BARS,
                "{secs}s gave {bars} bars of {}",
                b.label()
            );
        }
    }

    /// A gap is information: a log that stopped for ten minutes must
    /// look like it stopped, so empty buckets inside the span are kept.
    #[test]
    fn empty_buckets_inside_the_span_are_kept() {
        let records = vec![rec(0, None, "a"), rec(600, None, "b")];
        let h = histogram(&records, &all(2));
        assert_eq!(h.bucket().label(), "5s", "600 s / 5 s is the target 120");
        assert_eq!(h.bars.len(), 121);
        assert_eq!(h.bars[0].total(), 1);
        assert_eq!(h.bars[120].total(), 1);
        assert!(h.bars[1..120].iter().all(|b| b.total() == 0), "the gap");
    }

    #[test]
    fn a_bar_takes_the_colour_of_its_worst_level() {
        // An hour's span buckets at 30 s, so the three land together in
        // the first bar.
        let records = vec![
            rec(0, Some(Level::Info), "a"),
            rec(1, Some(Level::Error), "b"),
            rec(2, Some(Level::Warn), "c"),
            rec(3600, Some(Level::Info), "much later"),
        ];
        let h = histogram(&records, &all(4));
        assert_eq!(h.bucket().label(), "30s");
        let bar = &h.bars[0];
        assert_eq!(bar.total(), 3);
        assert_eq!(
            bar.worst(),
            Some(Level::Error),
            "one error among chatter is the thing to see"
        );
        assert_eq!(h.bars[0].levels[level_slot(Level::Info)], 1);
        // A bar with no level at all has no colour to take.
        let plain = Bar::default();
        assert_eq!(plain.worst(), None);
    }

    /// The chart counts the filtered window, not the log: it answers
    /// "when did this happen", not "when was anything logged".
    #[test]
    fn only_the_filtered_records_are_counted() {
        let records = vec![rec(0, None, "a"), rec(1, None, "b"), rec(2, None, "c")];
        let h = histogram(&records, &[0, 2]);
        assert_eq!(h.bars.iter().map(Bar::total).sum::<u32>(), 2);
    }

    /// Records with no timestamp cannot be placed, and must be reported
    /// rather than silently dropped.
    #[test]
    fn undated_records_are_counted_not_hidden() {
        let mut records = vec![rec(0, None, "a"), rec(5, None, "b")];
        records.push(Record {
            ts: None,
            message: "no time".into(),
            ..Default::default()
        });
        let h = histogram(&records, &all(3));
        assert_eq!(h.undated, 1);
        assert_eq!(h.bars.iter().map(Bar::total).sum::<u32>(), 2);
    }

    #[test]
    fn a_window_with_no_timestamps_has_no_bars() {
        let records = vec![Record {
            ts: None,
            ..Default::default()
        }];
        let h = histogram(&records, &all(1));
        assert!(h.is_empty());
        assert_eq!(h.undated, 1);
        assert_eq!(h.peak(), 0);
    }

    /// Clicking a bar jumps to its first record, so the bar has to know
    /// which filtered position that is.
    #[test]
    fn a_bar_remembers_the_first_record_in_it() {
        let records = vec![rec(0, None, "a"), rec(100, None, "b"), rec(101, None, "c")];
        let h = histogram(&records, &all(3));
        let bucket = h.bucket().0.num_seconds();
        let n = (100 / bucket) as usize;
        assert_eq!(h.bars[n].first, 1, "the second filtered record");
        assert_eq!(h.bars[0].first, 0);
    }

    /// Dragging across one bar must select that whole bar, not a
    /// zero-width instant that would match nothing.
    #[test]
    fn dragging_one_bar_selects_that_whole_bar() {
        let records = vec![rec(0, None, "a"), rec(300, None, "b")];
        let h = histogram(&records, &all(2));
        let r = range_at(&h, 0.0, 0.0).unwrap();
        assert_eq!(r.from, h.bars[0].start);
        assert_eq!(r.to - r.from, h.bucket().0, "one bucket wide");
        assert!(r.to > r.from, "never empty");
    }

    #[test]
    fn a_drag_backwards_means_the_same_range() {
        let records = vec![rec(0, None, "a"), rec(600, None, "b")];
        let h = histogram(&records, &all(2));
        assert_eq!(
            range_at(&h, 0.2, 0.8).unwrap(),
            range_at(&h, 0.8, 0.2).unwrap(),
            "a drag is a range, not a direction"
        );
    }

    #[test]
    fn a_fraction_outside_the_strip_is_clamped() {
        let records = vec![rec(0, None, "a"), rec(600, None, "b")];
        let h = histogram(&records, &all(2));
        assert_eq!(bar_at(&h, -5.0), Some(0));
        assert_eq!(bar_at(&h, 9.0), Some(h.bars.len() - 1));
    }

    // --- §15.8 ---------------------------------------------------------

    fn deploy_log() -> Vec<Record> {
        vec![
            // Window A: healthy.
            rec(0, Some(Level::Info), "served GET /orders in 31ms"),
            rec(1, Some(Level::Info), "served GET /orders in 904ms"),
            rec(2, Some(Level::Info), "cache hit for 12"),
            // Window B: after the deploy.
            rec(600, Some(Level::Info), "served GET /orders in 55ms"),
            rec(601, Some(Level::Error), "connection reset by peer"),
            rec(602, Some(Level::Error), "connection reset by peer"),
            rec(603, Some(Level::Error), "connection reset by peer"),
        ]
    }

    /// Numbers must not make two readings of the same line into two
    /// shapes, or every row would be unique and the table useless.
    #[test]
    fn records_differing_only_in_numbers_are_one_shape() {
        let records = deploy_log();
        let counts = shapes_in(&records, &all(records.len()), None);
        let served = counts
            .iter()
            .find(|(k, _)| k.contains("served"))
            .expect("a served shape");
        assert_eq!(*served.1, 3, "31ms, 904ms and 55ms are one shape");
    }

    #[test]
    fn the_diff_finds_what_the_deploy_introduced() {
        let records = deploy_log();
        let f = all(records.len());
        let a = Range {
            from: at(0),
            to: at(300),
        };
        let b = Range {
            from: at(600),
            to: at(900),
        };
        let d = diff_windows(&records, &f, a, b, 1);
        // Biggest move first: three new errors.
        assert!(d[0].shape.contains("connection reset"), "{:?}", d[0].shape);
        assert_eq!((d[0].a, d[0].b), (0, 3));
        assert!(d[0].only_in_b(), "it is new in B");
        assert_eq!(d[0].change(), 3);
        // And what went away is reported too.
        let gone = d.iter().find(|x| x.shape.contains("cache hit")).unwrap();
        assert!(gone.only_in_a());
        assert_eq!(gone.change(), -1);
    }

    /// A shape that appeared or vanished is the most interesting row
    /// there is, so the threshold must never filter it out.
    #[test]
    fn appearing_and_vanishing_survive_any_threshold() {
        let records = deploy_log();
        let f = all(records.len());
        let a = Range {
            from: at(0),
            to: at(300),
        };
        let b = Range {
            from: at(600),
            to: at(900),
        };
        let d = diff_windows(&records, &f, a, b, 1000);
        assert!(
            d.iter().any(|x| x.only_in_b()),
            "a new shape survives a huge threshold"
        );
        assert!(d.iter().any(|x| x.only_in_a()));
        // But a shape that merely wobbled does not.
        assert!(
            !d.iter().any(|x| x.a > 0 && x.b > 0),
            "nothing that only moved a little: {d:?}"
        );
    }

    #[test]
    fn a_record_with_no_timestamp_is_in_neither_window() {
        let mut records = deploy_log();
        records.push(Record {
            ts: None,
            message: "no time".into(),
            ..Default::default()
        });
        let f = all(records.len());
        let a = Range {
            from: at(0),
            to: at(300),
        };
        let b = Range {
            from: at(600),
            to: at(900),
        };
        let d = diff_windows(&records, &f, a, b, 1);
        assert!(!d.iter().any(|x| x.shape.contains("no time")));
    }

    /// The order must not change between frames, or the table
    /// reshuffles under the pointer.
    #[test]
    fn equal_moves_keep_a_stable_order() {
        let records = vec![
            rec(600, Some(Level::Error), "alpha failed"),
            rec(601, Some(Level::Error), "beta failed"),
        ];
        let f = all(2);
        let a = Range {
            from: at(0),
            to: at(300),
        };
        let b = Range {
            from: at(600),
            to: at(900),
        };
        let first = diff_windows(&records, &f, a, b, 1);
        let again = diff_windows(&records, &f, a, b, 1);
        assert_eq!(first, again);
        assert!(first[0].shape < first[1].shape, "ties break by shape");
    }
}
