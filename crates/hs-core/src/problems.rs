//! The Problems inbox any tool can write to (docs/tools/logs.md §15.2).
//!
//! The bottom dock's Problems tab was a placeholder. Editor diagnostics,
//! SQL errors and - the reason this exists - log records matching a
//! highlight rule all belong in one inbox, so the sink lives on
//! `Services` rather than inside any one tool.
//!
//! Two rules keep it from becoming a nuisance, both learned from the
//! alerting in §10:
//!
//! **Deduplication by shape.** A storm is one row whose count climbs,
//! not a thousand rows. The caller supplies the `shape` - a normalised
//! message - because only the caller knows how to normalise its own
//! text.
//!
//! **A cooldown per source and shape.** Even deduplicated, a tight loop
//! would keep a row's timestamp and count churning; `post` refuses a
//! repeat that arrives inside the cooldown and says so, so the caller
//! can stop building the entry at all.
//!
//! Nothing here runs in the background: entries exist while something
//! holds the `Services` they were posted to, and a tool that closes
//! should [`Problems::clear_from`] its own.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// How bad a problem is. Ordered, so a filter can ask for "warning and
/// worse" the way the Logs tool's level chips do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn key(self) -> &'static str {
        match self {
            Severity::Info => "problem-info",
            Severity::Warning => "problem-warning",
            Severity::Error => "problem-error",
        }
    }
}

/// Where a problem came from, and how to get back to it. The shell turns
/// this into the jump; the sink only carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    /// The activity that posted it (`logs`, `editor`, `db`).
    pub tool: String,
    /// The `current_uri` of the tab that posted it, so the shell can
    /// focus that exact tab rather than any tab of the activity.
    pub uri: String,
    /// What to show as the place: a file name, a source label.
    pub label: String,
    /// A line number within that source, when there is one.
    pub line: Option<u64>,
}

/// One row of the inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub id: u64,
    pub severity: Severity,
    /// The text as it reads, already redacted by whoever posted it.
    pub message: String,
    /// The normalised form used to deduplicate. Never shown.
    pub shape: String,
    pub origin: Origin,
    /// How many times this shape has arrived.
    pub count: u32,
    /// When it last arrived, for the "x minutes ago" and the ordering.
    pub last: chrono::DateTime<chrono::Utc>,
}

/// What a tool posts. The sink fills in the id, the count and the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub severity: Severity,
    pub message: String,
    pub shape: String,
    pub origin: Origin,
}

/// How many rows the inbox keeps. Far more than anyone reads, few enough
/// that a runaway tool cannot exhaust memory.
pub const CAP: usize = 500;

/// The default pause between two reports of the same shape from the same
/// source.
pub const COOLDOWN: Duration = Duration::from_secs(10);

#[derive(Default)]
struct Inner {
    rows: Vec<Problem>,
    /// `(uri, shape) -> when it was last accepted`, for the cooldown.
    seen: std::collections::HashMap<(String, String), Instant>,
    next: u64,
    /// Bumped on every change, so a view can tell when to rebuild.
    version: u64,
}

/// The shared inbox. Cheap to clone; every clone is the same inbox.
#[derive(Clone, Default)]
pub struct Problems {
    inner: Arc<Mutex<Inner>>,
}

impl std::fmt::Debug for Problems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Problems")
            .field("len", &self.inner.lock().rows.len())
            .finish()
    }
}

impl Problems {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a report, or fold it into the matching row.
    ///
    /// Returns `false` when the report was refused by the cooldown,
    /// which the caller can use to skip the work of building the next
    /// one. A refused report is not lost information: the row it would
    /// have folded into is already there.
    pub fn post(&self, report: Report) -> bool {
        self.post_with_cooldown(report, COOLDOWN)
    }

    /// As [`Problems::post`], with the caller's own pause - a highlight
    /// rule carries one per rule.
    pub fn post_with_cooldown(&self, report: Report, cooldown: Duration) -> bool {
        let now = Instant::now();
        let mut inner = self.inner.lock();
        let key = (report.origin.uri.clone(), report.shape.clone());
        if let Some(at) = inner.seen.get(&key)
            && now.duration_since(*at) < cooldown
        {
            return false;
        }
        inner.seen.insert(key, now);
        // One row per shape per source, with a count that climbs: a
        // storm must not become a thousand rows.
        if let Some(row) = inner
            .rows
            .iter_mut()
            .find(|r| r.shape == report.shape && r.origin.uri == report.origin.uri)
        {
            row.count += 1;
            row.last = chrono::Utc::now();
            row.message = report.message;
            row.severity = report.severity;
            row.origin.line = report.origin.line;
            inner.version += 1;
            return true;
        }
        inner.next += 1;
        let id = inner.next;
        inner.rows.push(Problem {
            id,
            severity: report.severity,
            message: report.message,
            shape: report.shape,
            origin: report.origin,
            count: 1,
            last: chrono::Utc::now(),
        });
        // Oldest first out, so a long session keeps what just happened.
        if inner.rows.len() > CAP {
            let extra = inner.rows.len() - CAP;
            inner.rows.drain(..extra);
        }
        inner.version += 1;
        true
    }

    /// Every row, newest first.
    pub fn all(&self) -> Vec<Problem> {
        let mut rows = self.inner.lock().rows.clone();
        rows.sort_by(|a, b| b.last.cmp(&a.last).then(b.id.cmp(&a.id)));
        rows
    }

    pub fn len(&self) -> usize {
        self.inner.lock().rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many rows are at `severity` or worse, for the dock's badge.
    pub fn count_at_least(&self, severity: Severity) -> usize {
        self.inner
            .lock()
            .rows
            .iter()
            .filter(|r| r.severity >= severity)
            .count()
    }

    pub fn version(&self) -> u64 {
        self.inner.lock().version
    }

    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.rows.clear();
        inner.seen.clear();
        inner.version += 1;
    }

    /// Drop everything a tab posted. A tab that closes takes its
    /// problems with it: a row that jumps to a tab which no longer
    /// exists is worse than no row.
    pub fn clear_from(&self, uri: &str) {
        let mut inner = self.inner.lock();
        inner.rows.retain(|r| r.origin.uri != uri);
        inner.seen.retain(|(u, _), _| u != uri);
        inner.version += 1;
    }

    pub fn remove(&self, id: u64) {
        let mut inner = self.inner.lock();
        inner.rows.retain(|r| r.id != id);
        inner.version += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(uri: &str) -> Origin {
        Origin {
            tool: "logs".into(),
            uri: uri.into(),
            label: "app.log".into(),
            line: Some(7),
        }
    }

    fn report(uri: &str, shape: &str) -> Report {
        Report {
            severity: Severity::Error,
            message: format!("something went wrong: {shape}"),
            shape: shape.into(),
            origin: origin(uri),
        }
    }

    /// A storm is one row whose count climbs, not a thousand rows.
    #[test]
    fn repeats_of_one_shape_fold_into_a_single_row() {
        let p = Problems::new();
        assert!(p.post(report("logs://a", "connection reset")));
        // Inside the cooldown the repeat is refused, and tells the
        // caller so.
        assert!(!p.post_with_cooldown(report("logs://a", "connection reset"), COOLDOWN));
        // With no cooldown it folds in and the count climbs.
        assert!(p.post_with_cooldown(report("logs://a", "connection reset"), Duration::ZERO));
        let rows = p.all();
        assert_eq!(rows.len(), 1, "one row, not three");
        assert_eq!(rows[0].count, 2, "refused reports do not count");
    }

    /// The same message from two different sources is two problems: they
    /// are different things to go and look at.
    #[test]
    fn the_same_shape_from_two_sources_stays_two_rows() {
        let p = Problems::new();
        p.post(report("logs://a", "disk full"));
        p.post(report("logs://b", "disk full"));
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn a_different_shape_is_its_own_row() {
        let p = Problems::new();
        p.post(report("logs://a", "one"));
        p.post(report("logs://a", "two"));
        assert_eq!(p.len(), 2);
    }

    /// A row that jumps to a tab which no longer exists is worse than no
    /// row, so a closing tab takes its problems with it.
    #[test]
    fn closing_a_tab_takes_its_problems_with_it() {
        let p = Problems::new();
        p.post(report("logs://a", "x"));
        p.post(report("logs://b", "y"));
        p.clear_from("logs://a");
        let rows = p.all();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].origin.uri, "logs://b");
        // And the cooldown for the gone tab is forgotten too, so
        // reopening it reports immediately rather than looking broken.
        assert!(p.post(report("logs://a", "x")));
    }

    #[test]
    fn the_newest_row_is_first() {
        let p = Problems::new();
        p.post(report("logs://a", "first"));
        std::thread::sleep(Duration::from_millis(2));
        p.post(report("logs://a", "second"));
        let rows = p.all();
        assert_eq!(rows[0].shape, "second");
    }

    /// A runaway tool must not exhaust memory, and what just happened
    /// matters more than what happened first.
    #[test]
    fn the_inbox_is_capped_and_keeps_the_newest() {
        let p = Problems::new();
        for i in 0..(CAP + 50) {
            p.post_with_cooldown(report("logs://a", &format!("shape {i}")), Duration::ZERO);
        }
        assert_eq!(p.len(), CAP);
        let rows = p.all();
        assert!(
            rows.iter()
                .any(|r| r.shape == format!("shape {}", CAP + 49)),
            "the newest survived"
        );
        assert!(
            !rows.iter().any(|r| r.shape == "shape 0"),
            "the oldest was dropped"
        );
    }

    #[test]
    fn the_badge_counts_by_severity() {
        let p = Problems::new();
        p.post(Report {
            severity: Severity::Info,
            ..report("logs://a", "a")
        });
        p.post(Report {
            severity: Severity::Warning,
            ..report("logs://a", "b")
        });
        p.post(Report {
            severity: Severity::Error,
            ..report("logs://a", "c")
        });
        assert_eq!(p.count_at_least(Severity::Error), 1);
        assert_eq!(p.count_at_least(Severity::Warning), 2);
        assert_eq!(p.count_at_least(Severity::Info), 3);
    }

    #[test]
    fn a_clone_is_the_same_inbox() {
        let p = Problems::new();
        let q = p.clone();
        p.post(report("logs://a", "x"));
        assert_eq!(q.len(), 1, "both handles see it");
        let before = q.version();
        q.clear();
        assert!(p.is_empty());
        assert!(q.version() > before, "a change moves the version");
    }
}
