//! Logs answering "have you seen this token?" for the other tools, and
//! the registry of open tabs that makes it possible
//! (docs/tools/logs.md §15.3).
//!
//! A `Searcher` is registered once at start-up and lives as long as the
//! app, while a Logs tab comes and goes. So tabs register their record
//! window here under their uri, held **weakly**: a closed tab's window
//! is dropped as it should be, and the searcher skips the dead entry
//! rather than keeping a tab's worth of records alive for ever.

use std::sync::{Arc, Weak};

use hs_core::search::{Hit, Jump, Query, Searcher};
use hs_core::tool::OpenRequest;
use parking_lot::Mutex;

use crate::engine::Shared;

/// The open tabs, by uri. A `Weak` so a closed tab is not kept alive by
/// being searchable.
type Open = Vec<(String, String, Weak<Mutex<Shared>>)>;

static OPEN: std::sync::LazyLock<Mutex<Open>> = std::sync::LazyLock::new(|| Mutex::new(Vec::new()));

/// Make a tab's records searchable. Called whenever the tab's source
/// changes, so the uri it is registered under is the uri it reports.
pub fn register_tab(uri: &str, label: &str, shared: &Arc<Mutex<Shared>>) {
    let mut open = OPEN.lock();
    // Drop the dead while we are here: no other moment is better, and a
    // long session would otherwise accumulate empty slots.
    open.retain(|(_, _, w)| w.strong_count() > 0);
    open.retain(|(u, _, _)| u != uri);
    open.push((uri.to_string(), label.to_string(), Arc::downgrade(shared)));
}

/// Forget a tab, when it closes or changes source.
pub fn unregister_tab(uri: &str) {
    let mut open = OPEN.lock();
    open.retain(|(u, _, w)| u != uri && w.strong_count() > 0);
}

/// How many tabs are searchable, for the tests and the panel's hint.
pub fn open_tabs() -> usize {
    let mut open = OPEN.lock();
    open.retain(|(_, _, w)| w.strong_count() > 0);
    open.len()
}

/// Searches every open Logs tab except the one asking.
pub struct TabSearcher;

impl Searcher for TabSearcher {
    fn id(&self) -> &str {
        "logs"
    }

    fn label_key(&self) -> &str {
        "search-group-logs"
    }

    fn search(&self, query: &Query) -> Result<Vec<Hit>, String> {
        if query.token.is_empty() {
            return Ok(Vec::new());
        }
        // The list is copied out first so the registry is not held while
        // each tab's own lock is taken: two locks held at once is how a
        // deadlock starts, and a tab's lock is contended by its reader.
        let tabs: Vec<(String, String, Arc<Mutex<Shared>>)> = {
            let open = OPEN.lock();
            open.iter()
                .filter_map(|(uri, label, weak)| {
                    weak.upgrade().map(|s| (uri.clone(), label.clone(), s))
                })
                .collect()
        };
        let mut out = Vec::new();
        for (uri, label, shared) in tabs {
            // The asking tab leaves itself out: the record is already on
            // screen there.
            if query.excluded(&uri) {
                continue;
            }
            let found: Vec<(u64, String, Option<chrono::DateTime<chrono::Utc>>)> = {
                let sh = shared.lock();
                sh.records
                    .iter()
                    .filter(|r| query.matches(&r.raw))
                    .take(query.limit)
                    .map(|r| (r.line, r.raw.clone(), r.ts))
                    .collect()
            };
            for (line, raw, at) in found {
                out.push(Hit {
                    source: "logs".into(),
                    preview: one_line(&raw, 160),
                    at,
                    detail: format!("{label} · line {line}"),
                    open: Some(Jump {
                        tool: "logs".into(),
                        request: OpenRequest::Text {
                            mime: crate::GOTO_MIME.into(),
                            body: line.to_string(),
                        },
                    }),
                });
                if out.len() >= query.limit {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}

fn one_line(text: &str, max: usize) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    if first.chars().count() <= max {
        return first.to_string();
    }
    let cut: String = first.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::{Format, parse};

    fn tab(lines: &[&str]) -> Arc<Mutex<Shared>> {
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let mut sh = shared.lock();
            for (i, l) in lines.iter().enumerate() {
                sh.records.push(parse(Format::Plain, i as u64 + 1, l));
            }
        }
        shared
    }

    /// The registry is process-wide, so these tests must not run
    /// beside each other: one registering a tab would be seen by
    /// another's assertions. Holding this guard for the body of a test
    /// serialises them properly, rather than relying on the runner
    /// being told `--test-threads=1`.
    static EXCLUSIVE: Mutex<()> = Mutex::new(());

    fn clean() -> parking_lot::MutexGuard<'static, ()> {
        let guard = EXCLUSIVE.lock();
        OPEN.lock().clear();
        guard
    }

    const ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

    #[test]
    fn a_token_in_another_tab_is_found_with_its_line() {
        let _guard = clean();
        let t = tab(&[
            "09:00:00 INFO starting",
            &format!("09:00:01 INFO handling trace={ID}"),
        ]);
        register_tab("logs://app.log", "app.log", &t);
        let hits = TabSearcher.search(&Query::new(ID)).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].detail.contains("app.log"));
        assert!(hits[0].detail.contains("line 2"));
        assert!(hits[0].preview.contains("handling"));
        // The jump is by line number, which is what GOTO_MIME takes.
        match &hits[0].open.as_ref().unwrap().request {
            OpenRequest::Text { mime, body } => {
                assert_eq!(mime, crate::GOTO_MIME);
                assert_eq!(body, "2");
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }

    /// A closed tab must not be kept alive by being searchable, and must
    /// not be searched either.
    #[test]
    fn a_closed_tab_drops_out_of_the_registry() {
        let _guard = clean();
        let t = tab(&[&format!("line with {ID}")]);
        register_tab("logs://gone.log", "gone.log", &t);
        assert_eq!(open_tabs(), 1);
        drop(t);
        assert_eq!(open_tabs(), 0, "the weak handle died with the tab");
        assert!(TabSearcher.search(&Query::new(ID)).unwrap().is_empty());
    }

    #[test]
    fn registering_the_same_uri_twice_replaces_it() {
        let _guard = clean();
        let a = tab(&[&format!("first {ID}")]);
        let b = tab(&[&format!("second {ID}"), &format!("third {ID}")]);
        register_tab("logs://same", "same", &a);
        register_tab("logs://same", "same", &b);
        assert_eq!(open_tabs(), 1);
        assert_eq!(
            TabSearcher.search(&Query::new(ID)).unwrap().len(),
            2,
            "the second registration is the live one"
        );
    }

    #[test]
    fn unregistering_removes_a_tab() {
        let _guard = clean();
        let t = tab(&[&format!("x {ID}")]);
        register_tab("logs://bye", "bye", &t);
        unregister_tab("logs://bye");
        assert_eq!(open_tabs(), 0);
        assert!(TabSearcher.search(&Query::new(ID)).unwrap().is_empty());
    }

    /// The asking tab leaves itself out, but the others still answer:
    /// that is the difference between excluding a source and skipping a
    /// searcher.
    #[test]
    fn the_asking_tab_is_excluded_while_the_others_answer() {
        let _guard = clean();
        let mine = tab(&[&format!("mine has {ID}")]);
        let other = tab(&[&format!("theirs has {ID}")]);
        register_tab("logs://mine", "mine", &mine);
        register_tab("logs://other", "other", &other);

        let mut q = Query::new(ID);
        q.exclude.push("logs://mine".into());
        let hits = TabSearcher.search(&q).unwrap();
        assert_eq!(hits.len(), 1, "only the other tab answers");
        assert!(hits[0].detail.contains("other"));
        // Without the exclusion, both answer.
        assert_eq!(TabSearcher.search(&Query::new(ID)).unwrap().len(), 2);
    }

    /// One chatty tab must not bury the rest, so the limit is a limit on
    /// the whole answer.
    #[test]
    fn the_limit_bounds_the_whole_answer() {
        let _guard = clean();
        let noisy: Vec<String> = (0..50).map(|n| format!("{n} has {ID}")).collect();
        let refs: Vec<&str> = noisy.iter().map(|s| s.as_str()).collect();
        let t = tab(&refs);
        register_tab("logs://noisy", "noisy", &t);
        let mut q = Query::new(ID);
        q.limit = 5;
        assert_eq!(TabSearcher.search(&q).unwrap().len(), 5);
    }

    #[test]
    fn nothing_matching_is_an_empty_answer() {
        let _guard = clean();
        let t = tab(&["nothing interesting here"]);
        register_tab("logs://quiet", "quiet", &t);
        assert!(TabSearcher.search(&Query::new(ID)).unwrap().is_empty());
        assert!(TabSearcher.search(&Query::new("")).unwrap().is_empty());
    }

    #[test]
    fn a_multi_line_record_previews_as_its_first_line() {
        let _guard = clean();
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let mut sh = shared.lock();
            let mut r = parse(Format::Plain, 1, &format!("ERROR failed {ID}"));
            r.raw.push_str("\n\tat com.example.Boot.main(Boot.java:12)");
            sh.records.push(r);
        }
        register_tab("logs://trace", "trace", &shared);
        let hits = TabSearcher.search(&Query::new(ID)).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].preview.contains("Boot.java"), "one line only");
    }
}
