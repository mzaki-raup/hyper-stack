//! The API client's answer to "have you seen this token?"
//! (docs/tools/logs.md §15.3).
//!
//! `history.jsonl` and the `Request` shape belong to this crate, so this
//! crate reads them. A correlation id reaches a request through its URL,
//! its headers, its query or its body, so all four are looked at - that
//! is usually where the id was *minted*, which makes this the most
//! useful of the searchers.

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::search::{Hit, Jump, Query, Searcher};
use hs_core::tool::OpenRequest;

use crate::model::{HistoryEntry, load_history};

/// How much history is read. The file is capped by the tool already;
/// this bounds the work of one search.
const SCAN: usize = 2000;

/// A whole `Request` as JSON, which an API tab can open and fill itself
/// from. Introduced for the cross-tool search: a history row has no
/// stable id to address it by, so the request itself is the address.
pub const REQUEST_MIME: &str = "application/x-hyperstack-api-request+json";

/// The history path is resolved **at search time**, not captured: a
/// searcher is registered once for the life of the app, and a workspace
/// switch would otherwise leave it reading the previous workspace's
/// requests for ever.
pub struct HistorySearcher {
    workspace: Arc<parking_lot::RwLock<hs_core::workspace::Workspace>>,
}

impl HistorySearcher {
    pub fn new(workspace: Arc<parking_lot::RwLock<hs_core::workspace::Workspace>>) -> Self {
        Self { workspace }
    }

    /// `<workspace>/api/history.jsonl`, now.
    fn path(&self) -> PathBuf {
        self.workspace
            .read()
            .root()
            .join("api")
            .join("history.jsonl")
    }
}

impl Searcher for HistorySearcher {
    fn id(&self) -> &str {
        "api"
    }

    fn label_key(&self) -> &str {
        "search-group-api"
    }

    fn search(&self, query: &Query) -> Result<Vec<Hit>, String> {
        search_file(&self.path(), query)
    }
}

/// The search itself, against a named history file. Separate from the
/// `Searcher` so it can be tested without standing up a workspace.
pub fn search_file(path: &std::path::Path, query: &Query) -> Result<Vec<Hit>, String> {
    {
        if query.token.is_empty() {
            return Ok(Vec::new());
        }
        // A workspace that has sent no requests has nothing to find,
        // which is not a failure.
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for e in load_history(path, SCAN) {
            let Some(where_found) = found_in(&e, query) else {
                continue;
            };
            out.push(Hit {
                source: "api".into(),
                preview: format!("{} {}", e.method, one_line(&e.url, 140)),
                at: chrono::DateTime::parse_from_rfc3339(&e.at)
                    .ok()
                    .map(|t| t.with_timezone(&chrono::Utc)),
                detail: format!("{} · in the {where_found}", e.status),
                // The request itself, so the tab can be filled from it:
                // `hs-api` is the action language the API assistant
                // already applies.
                open: serde_json::to_string(&e.request).ok().map(|body| Jump {
                    tool: "api".into(),
                    request: OpenRequest::Text {
                        mime: REQUEST_MIME.into(),
                        body,
                    },
                }),
            });
            if out.len() >= query.limit {
                break;
            }
        }
        Ok(out)
    }
}

/// Which part of the request carried the token, so the row can say.
/// Headers first, because that is where a correlation id usually lives.
fn found_in(e: &HistoryEntry, q: &Query) -> Option<&'static str> {
    let r = &e.request;
    if r.headers
        .iter()
        .any(|h| h.enabled && (q.matches(&h.key) || q.matches(&h.value)))
    {
        return Some("headers");
    }
    if q.matches(&e.url) || q.matches(&r.url) {
        return Some("URL");
    }
    if r.params
        .iter()
        .any(|p| p.enabled && (q.matches(&p.key) || q.matches(&p.value)))
    {
        return Some("query");
    }
    if q.matches(&r.body.text) {
        return Some("body");
    }
    None
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max).collect();
    format!("{cut}…")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Body, Kv, Request, append_history};

    const ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";

    fn entry(url: &str, headers: Vec<Kv>, body: &str) -> HistoryEntry {
        HistoryEntry {
            at: "2026-10-24T09:00:00Z".into(),
            method: "POST".into(),
            url: url.into(),
            status: 200,
            millis: 12,
            request: Request {
                url: url.into(),
                headers,
                body: Body {
                    text: body.into(),
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }

    fn header(name: &str, value: &str) -> Kv {
        Kv {
            key: name.into(),
            value: value.into(),
            enabled: true,
            ..Default::default()
        }
    }

    fn seeded(entries: Vec<HistoryEntry>) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.jsonl");
        for e in &entries {
            append_history(&path, e, 100);
        }
        (dir, path)
    }

    /// A correlation id is usually minted in a header, so that is the
    /// case that matters most.
    #[test]
    fn a_token_in_a_header_is_found_and_the_place_is_named() {
        let (_d, path) = seeded(vec![entry(
            "https://api.example.com/orders",
            vec![header("X-Request-Id", ID)],
            "{}",
        )]);
        let hits = search_file(&path, &Query::new(ID)).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].detail.contains("headers"), "{}", hits[0].detail);
        assert!(hits[0].preview.starts_with("POST https://api.example.com"));
        assert!(hits[0].open.is_some(), "the request can fill the tab");
        assert!(hits[0].at.is_some());
    }

    #[test]
    fn a_token_in_the_url_or_the_body_is_found_too() {
        let (_d, path) = seeded(vec![
            entry(&format!("https://x/t/{ID}"), vec![], "{}"),
            entry("https://x/u", vec![], &format!("{{\"trace\":\"{ID}\"}}")),
        ]);
        let hits = search_file(&path, &Query::new(ID)).unwrap();
        assert_eq!(hits.len(), 2);
        let places: Vec<&str> = hits
            .iter()
            .map(|h| {
                if h.detail.contains("URL") {
                    "URL"
                } else {
                    "body"
                }
            })
            .collect();
        assert!(places.contains(&"URL"));
        assert!(places.contains(&"body"));
    }

    /// A disabled header is not sent, so it did not carry the id.
    #[test]
    fn a_disabled_header_does_not_count_as_a_match() {
        let mut h = header("X-Request-Id", ID);
        h.enabled = false;
        let (_d, path) = seeded(vec![entry("https://x/v", vec![h], "{}")]);
        assert!(search_file(&path, &Query::new(ID)).unwrap().is_empty());
    }

    #[test]
    fn nothing_matching_is_an_empty_answer() {
        let (_d, path) = seeded(vec![entry("https://x/w", vec![], "{}")]);
        assert!(search_file(&path, &Query::new(ID)).unwrap().is_empty());
    }

    #[test]
    fn a_missing_history_file_finds_nothing_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("none.jsonl");
        assert!(search_file(&path, &Query::new(ID)).unwrap().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn the_limit_bounds_the_answer() {
        let many: Vec<HistoryEntry> = (0..10)
            .map(|n| entry(&format!("https://x/{n}/{ID}"), vec![], "{}"))
            .collect();
        let (_d, path) = seeded(many);
        let mut q = Query::new(ID);
        q.limit = 3;
        assert_eq!(search_file(&path, &q).unwrap().len(), 3);
    }

    #[test]
    fn matching_ignores_case() {
        let (_d, path) = seeded(vec![entry(
            "https://x/y",
            vec![header("X-Request-Id", &ID.to_uppercase())],
            "{}",
        )]);
        assert_eq!(search_file(&path, &Query::new(ID)).unwrap().len(), 1);
    }
}
