//! Cross-tool search: "who has seen this token?" (docs/tools/logs.md
//! §15.3, ADR-004).
//!
//! A request id in a log line is a join key into everything else in the
//! workspace, but **a tool may never depend on another tool crate**.
//! Logs cannot read the API client's history or the SQL history, because
//! those schemas belong to those tools, and reading them directly would
//! couple the tools through a file format instead of an interface.
//!
//! So each tool registers a [`Searcher`] here at start-up, capturing
//! whatever it needs - a path, a handle - and the asking tool queries
//! the registry without knowing who answers. The same registry serves
//! the other direction: any tool can ask.
//!
//! **Every searcher runs off the UI thread.** [`Searcher::search`] is
//! blocking by design - it opens files and databases - and the caller
//! runs it as a Job. A searcher that cannot answer returns an error
//! rather than an empty list, so "nothing found" and "could not look"
//! stay different answers.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::tool::OpenRequest;

/// What to look for. A token, not a pattern: this is for correlation
/// ids, and a regex across four stores would be a different feature
/// with different costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// Matched case-insensitively as a substring; ids differ in case
    /// between systems more often than anyone expects.
    pub token: String,
    /// At most this many hits per searcher, so one chatty store cannot
    /// bury the others.
    pub limit: usize,
    /// Sources a searcher should leave out, by whatever key it knows
    /// them by - a tab uri, a connection id. The asker uses this to
    /// exclude *itself*: the record it is looking at is already on
    /// screen, and offering to jump to it reads as a bug. Skipping a
    /// whole searcher is `ask_all`'s job; this skips one of its sources.
    pub exclude: Vec<String>,
}

impl Query {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            limit: 50,
            exclude: Vec::new(),
        }
    }
    /// Whether a source named `key` was asked to be left out.
    pub fn excluded(&self, key: &str) -> bool {
        self.exclude.iter().any(|e| e == key)
    }
    /// Whether `haystack` contains the token, the way every searcher
    /// should test it, so they all agree on what a match is.
    pub fn matches(&self, haystack: &str) -> bool {
        if self.token.is_empty() {
            return false;
        }
        haystack.to_lowercase().contains(&self.token.to_lowercase())
    }
}

/// One thing a searcher found.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// The searcher that found it, for grouping.
    pub source: String,
    /// A one-line preview, already redacted by whoever produced it.
    pub preview: String,
    /// When it happened, when that is known. Shown, and used to order
    /// within a group.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    /// Extra context for the row: a connection name, a status, a host.
    pub detail: String,
    /// How to get to it: the activity to open and what to ask of it.
    /// `None` for a hit that cannot be reached, which is honest - a
    /// pruned history row has nowhere to go.
    pub open: Option<Jump>,
}

/// Where a hit opens.
#[derive(Debug, Clone, PartialEq)]
pub struct Jump {
    /// The factory id of the activity (`logs`, `api`, `db`, `redis`).
    pub tool: String,
    pub request: OpenRequest,
}

/// Something that can answer a [`Query`]. Implemented by each tool and
/// registered once at start-up.
pub trait Searcher: Send + Sync {
    /// Stable id, used to dedupe registrations and to group hits.
    fn id(&self) -> &str;
    /// `en.ftl` key for the group heading, so it is translated like
    /// every other visible string.
    fn label_key(&self) -> &str;
    /// Look for the token. **Blocking**: the caller runs this on a
    /// worker thread. An error means "could not look", which is not the
    /// same as finding nothing.
    fn search(&self, query: &Query) -> Result<Vec<Hit>, String>;
}

/// What one searcher came back with.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub source: String,
    pub label_key: String,
    pub result: Result<Vec<Hit>, String>,
}

/// The searchers this app has. Cheap to clone; every clone is the same
/// registry.
#[derive(Clone, Default)]
pub struct SearchRegistry {
    inner: Arc<RwLock<Vec<Arc<dyn Searcher>>>>,
}

impl std::fmt::Debug for SearchRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchRegistry")
            .field("len", &self.inner.read().len())
            .finish()
    }
}

impl SearchRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a searcher, replacing any already registered under the same
    /// id. Registering twice is a mistake rather than a reason to search
    /// the same store twice, and start-up order should not decide which
    /// wins.
    pub fn register(&self, searcher: Arc<dyn Searcher>) {
        let mut all = self.inner.write();
        let id = searcher.id().to_string();
        all.retain(|s| s.id() != id);
        all.push(searcher);
    }

    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn ids(&self) -> Vec<String> {
        self.inner
            .read()
            .iter()
            .map(|s| s.id().to_string())
            .collect()
    }

    /// Every searcher but the ones named, which is how the asking tool
    /// leaves itself out: Logs searching Logs would find the record the
    /// user is already looking at.
    pub fn except(&self, skip: &[&str]) -> Vec<Arc<dyn Searcher>> {
        self.inner
            .read()
            .iter()
            .filter(|s| !skip.contains(&s.id()))
            .cloned()
            .collect()
    }

    /// Ask every searcher, in parallel, each on its own thread. Blocks
    /// until all have answered, so the **caller** must be a Job, not the
    /// UI thread.
    ///
    /// A searcher that panics is reported as a failed answer rather than
    /// taking the whole search down with it: one broken store must not
    /// hide the other three.
    pub fn ask_all(&self, query: &Query, skip: &[&str]) -> Vec<Answer> {
        let searchers = self.except(skip);
        let mut handles = Vec::new();
        for s in searchers {
            let q = query.clone();
            let id = s.id().to_string();
            let label = s.label_key().to_string();
            handles.push(std::thread::spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.search(&q)))
                        .unwrap_or_else(|_| Err("the search failed".to_string()));
                Answer {
                    source: id,
                    label_key: label,
                    result,
                }
            }));
        }
        let mut out: Vec<Answer> = handles.into_iter().filter_map(|h| h.join().ok()).collect();
        // A stable order, so the panel does not reshuffle between runs.
        out.sort_by(|a, b| a.source.cmp(&b.source));
        out
    }
}

/// Whether `token` looks like a correlation id worth searching for
/// (docs/tools/logs.md §15.3): a UUID, a 16-or-more-character hex
/// string, or a ULID.
///
/// Deliberately strict. The offer appears on a selection, and offering
/// to search four stores for the word "timeout" would be noise.
pub fn looks_like_id(token: &str) -> bool {
    let t = token.trim();
    if t.len() < 8 {
        return false;
    }
    // A UUID, with or without its dashes already counted by the hex arm.
    let uuid = t.len() == 36
        && t.as_bytes().iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    if uuid {
        return true;
    }
    // A long hex run: a trace id is 32 characters, a span id 16.
    if t.len() >= 16 && t.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // A ULID: 26 characters of Crockford base32, which excludes I, L, O
    // and U so it cannot be confused with a word.
    if t.len() == 26
        && t.chars().all(|c| {
            c.is_ascii_digit() || "ABCDEFGHJKMNPQRSTVWXYZ".contains(c.to_ascii_uppercase())
        })
        && t.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    false
}

/// The field names that carry a correlation id, compared ignoring case,
/// dashes and underscores - `X-Request-Id`, `request_id` and `requestid`
/// are the same field.
pub const ID_FIELDS: &[&str] = &[
    "traceid",
    "requestid",
    "correlationid",
    "spanid",
    "xrequestid",
    "xcorrelationid",
    "trace",
    "reqid",
];

/// Whether a field name is one of [`ID_FIELDS`].
pub fn is_id_field(name: &str) -> bool {
    let flat: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    ID_FIELDS.contains(&flat.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        id: &'static str,
        hits: usize,
        fail: bool,
    }

    impl Searcher for Fake {
        fn id(&self) -> &str {
            self.id
        }
        fn label_key(&self) -> &str {
            "x"
        }
        fn search(&self, _q: &Query) -> Result<Vec<Hit>, String> {
            if self.fail {
                return Err("no".into());
            }
            Ok((0..self.hits)
                .map(|n| Hit {
                    source: self.id.to_string(),
                    preview: format!("hit {n}"),
                    at: None,
                    detail: String::new(),
                    open: None,
                })
                .collect())
        }
    }

    fn fake(id: &'static str, hits: usize) -> Arc<dyn Searcher> {
        Arc::new(Fake {
            id,
            hits,
            fail: false,
        })
    }

    #[test]
    fn registering_twice_under_one_id_replaces_rather_than_duplicates() {
        let r = SearchRegistry::new();
        r.register(fake("api", 1));
        r.register(fake("api", 5));
        assert_eq!(r.len(), 1, "one searcher per store");
        let answers = r.ask_all(&Query::new("x"), &[]);
        assert_eq!(
            answers[0].result.as_ref().unwrap().len(),
            5,
            "the later one"
        );
    }

    /// The asking tool leaves itself out: Logs searching Logs would
    /// find the record the user is already looking at.
    #[test]
    fn a_searcher_can_be_skipped_by_id() {
        let r = SearchRegistry::new();
        r.register(fake("logs", 1));
        r.register(fake("api", 1));
        let answers = r.ask_all(&Query::new("x"), &["logs"]);
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].source, "api");
    }

    /// "Could not look" and "found nothing" are different answers, and
    /// one broken store must not hide the others.
    #[test]
    fn a_failing_searcher_is_reported_without_losing_the_rest() {
        let r = SearchRegistry::new();
        r.register(Arc::new(Fake {
            id: "db",
            hits: 0,
            fail: true,
        }));
        r.register(fake("api", 2));
        let answers = r.ask_all(&Query::new("x"), &[]);
        assert_eq!(answers.len(), 2);
        let db = answers.iter().find(|a| a.source == "db").unwrap();
        assert!(db.result.is_err(), "the failure is reported as one");
        let api = answers.iter().find(|a| a.source == "api").unwrap();
        assert_eq!(api.result.as_ref().unwrap().len(), 2);
    }

    /// A searcher that panics must not take the search down with it.
    #[test]
    fn a_panicking_searcher_becomes_a_failed_answer() {
        struct Boom;
        impl Searcher for Boom {
            fn id(&self) -> &str {
                "boom"
            }
            fn label_key(&self) -> &str {
                "x"
            }
            fn search(&self, _q: &Query) -> Result<Vec<Hit>, String> {
                panic!("in a searcher");
            }
        }
        let r = SearchRegistry::new();
        r.register(Arc::new(Boom));
        r.register(fake("api", 1));
        let answers = r.ask_all(&Query::new("x"), &[]);
        assert_eq!(answers.len(), 2);
        assert!(
            answers
                .iter()
                .find(|a| a.source == "boom")
                .unwrap()
                .result
                .is_err()
        );
        assert!(
            answers
                .iter()
                .find(|a| a.source == "api")
                .unwrap()
                .result
                .is_ok()
        );
    }

    #[test]
    fn the_answers_come_back_in_a_stable_order() {
        let r = SearchRegistry::new();
        for id in ["redis", "api", "db"] {
            r.register(fake(id, 1));
        }
        let first: Vec<String> = r
            .ask_all(&Query::new("x"), &[])
            .into_iter()
            .map(|a| a.source)
            .collect();
        let again: Vec<String> = r
            .ask_all(&Query::new("x"), &[])
            .into_iter()
            .map(|a| a.source)
            .collect();
        assert_eq!(first, again);
        assert_eq!(first, ["api", "db", "redis"]);
    }

    /// Every searcher must agree on what a match is, so the test lives
    /// on the query rather than in each of them.
    #[test]
    fn matching_ignores_case_and_refuses_an_empty_token() {
        let q = Query::new("AbC-123");
        assert!(q.matches("prefix abc-123 suffix"));
        assert!(q.matches("ABC-123"));
        assert!(!q.matches("abc-124"));
        assert!(
            !Query::new("").matches("anything"),
            "an empty token matches nothing"
        );
    }

    /// The offer appears on a selection, so it has to be strict:
    /// offering to search four stores for "timeout" would be noise.
    #[test]
    fn only_things_that_look_like_ids_are_offered() {
        for id in [
            "550e8400-e29b-41d4-a716-446655440000",
            "4bf92f3577b34da6a3ce929d0e0e4736",
            "00f067aa0ba902b7",
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        ] {
            assert!(looks_like_id(id), "{id} is an id");
        }
        for not in [
            "timeout",
            "",
            "abc",
            "deadbeef",
            "connection reset by peer",
            "2026-10-24",
            "ERROR",
        ] {
            assert!(!looks_like_id(not), "{not:?} is not an id");
        }
    }

    /// The asker excludes itself by source, not by searcher: the other
    /// Logs tabs must still be searched.
    #[test]
    fn a_single_source_can_be_excluded_without_skipping_its_searcher() {
        let mut q = Query::new("x");
        q.exclude.push("logs://this.log".into());
        assert!(q.excluded("logs://this.log"));
        assert!(!q.excluded("logs://other.log"));
        assert!(!Query::new("x").excluded("logs://this.log"));
    }

    /// `X-Request-Id`, `request_id` and `requestid` are one field.
    #[test]
    fn an_id_field_is_recognised_whatever_its_spelling() {
        for name in [
            "trace_id",
            "traceId",
            "TRACE-ID",
            "x-request-id",
            "X_Request_Id",
            "requestid",
            "correlation_id",
            "span_id",
        ] {
            assert!(is_id_field(name), "{name} carries an id");
        }
        for name in ["message", "level", "id", "user_id", "idx"] {
            assert!(!is_id_field(name), "{name} does not");
        }
    }
}
