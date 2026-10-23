//! The SQL tool's answer to "have you seen this token?"
//! (docs/tools/logs.md §15.3).
//!
//! The `query_history` table belongs to this crate, so this crate is
//! what reads it. `tool-logs` asks through `Services::search` and never
//! learns the schema, which is the whole point of the registry.

use std::path::PathBuf;
use std::sync::Arc;

use hs_core::search::{Hit, Jump, Query, Searcher};
use hs_core::tool::OpenRequest;

/// Searches the shared history database for statements containing the
/// token.
///
/// The database path and the workspace id are resolved **at search
/// time**, not captured: a searcher is registered once for the life of
/// the app, and a workspace switch would otherwise leave it reading the
/// previous workspace's history for ever.
pub struct HistorySearcher {
    paths: Arc<hs_core::paths::DataDir>,
    workspace: Arc<parking_lot::RwLock<hs_core::workspace::Workspace>>,
}

impl HistorySearcher {
    pub fn new(
        paths: Arc<hs_core::paths::DataDir>,
        workspace: Arc<parking_lot::RwLock<hs_core::workspace::Workspace>>,
    ) -> Self {
        Self { paths, workspace }
    }

    /// The history database and the workspace whose rows count, now.
    fn target(&self) -> (PathBuf, String) {
        (
            self.paths.history_db(),
            self.workspace.read().meta.id.clone(),
        )
    }
}

impl Searcher for HistorySearcher {
    fn id(&self) -> &str {
        "db"
    }

    fn label_key(&self) -> &str {
        "search-group-sql"
    }

    fn search(&self, query: &Query) -> Result<Vec<Hit>, String> {
        let (db, workspace_id) = self.target();
        search_history(&db, &workspace_id, query)
    }
}

/// The search itself, against a named database and workspace. Separate
/// from the `Searcher` so it can be tested against a seeded file without
/// standing up a data directory and a workspace.
pub fn search_history(
    db: &std::path::Path,
    workspace_id: &str,
    query: &Query,
) -> Result<Vec<Hit>, String> {
    if query.token.is_empty() {
        return Ok(Vec::new());
    }
    // Read-only: a search must never create the database or write to it.
    // A missing file is "nothing to find", not an error.
    if !db.exists() {
        return Ok(Vec::new());
    }
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    // LIKE with an escaped pattern, never string interpolation: a token
    // is user text and goes in as a parameter.
    let pattern = format!("%{}%", escape_like(&query.token));
    let mut stmt = conn
        .prepare(
            r"SELECT created_at, connection_id, sql, status, error
                FROM query_history
               WHERE workspace_id = ?1
                 AND (sql LIKE ?2 ESCAPE '\' OR IFNULL(error, '') LIKE ?2 ESCAPE '\')
            ORDER BY id DESC
               LIMIT ?3",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(
            rusqlite::params![workspace_id, pattern, query.limit as i64],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (at, connection_id, sql, status, error) = row.map_err(|e| e.to_string())?;
        let mut detail = connection_id.clone();
        if status != "ok" {
            detail.push_str(" · ");
            detail.push_str(&status);
        }
        if let Some(e) = &error
            && query.matches(e)
        {
            detail.push_str(" · ");
            detail.push_str(&one_line(e, 80));
        }
        out.push(Hit {
            source: "db".into(),
            preview: one_line(&sql, 160),
            at: chrono::DateTime::parse_from_rfc3339(&at)
                .ok()
                .map(|t| t.with_timezone(&chrono::Utc)),
            detail,
            // Opened as text the SQL tab puts in its editor; a history
            // row has no stable id to address it by.
            open: Some(Jump {
                tool: "db".into(),
                request: OpenRequest::Text {
                    mime: "text/x-sql".into(),
                    body: sql,
                },
            }),
        });
    }
    Ok(out)
}

/// `%`, `_` and the escape character itself have to be escaped, or a
/// token containing one would match far more than it should.
fn escape_like(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        if matches!(c, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A statement as one line, cut to `max` characters: the panel shows a
/// preview, and a formatted query is twenty lines long.
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
    use crate::history::{History, NewEntry};

    fn seeded() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("history.sqlite");
        let h = History::new(db.clone());
        for (sql, status) in [
            (
                "SELECT * FROM orders WHERE trace = '4bf92f3577b34da6a3ce929d0e0e4736'",
                "ok",
            ),
            ("SELECT 1", "ok"),
            ("UPDATE orders SET paid = 1", "error"),
        ] {
            h.add(
                &NewEntry {
                    workspace_id: "ws1",
                    connection_id: "shop",
                    environment_id: None,
                    sql,
                    duration_ms: 3,
                    row_count: Some(1),
                    status,
                    error: if status == "error" {
                        Some("deadlock on 4bf92f3577b34da6a3ce929d0e0e4736")
                    } else {
                        None
                    },
                },
                &[],
            )
            .unwrap();
        }
        // Another workspace's run must not be found.
        h.add(
            &NewEntry {
                workspace_id: "ws2",
                connection_id: "other",
                environment_id: None,
                sql: "SELECT '4bf92f3577b34da6a3ce929d0e0e4736'",
                duration_ms: 1,
                row_count: None,
                status: "ok",
                error: None,
            },
            &[],
        )
        .unwrap();
        (dir, db)
    }

    #[test]
    fn a_statement_carrying_the_token_is_found() {
        let (_d, db) = seeded();
        let hits =
            search_history(&db, "ws1", &Query::new("4bf92f3577b34da6a3ce929d0e0e4736")).unwrap();
        assert_eq!(hits.len(), 2, "the statement and the error");
        assert!(hits.iter().any(|h| h.preview.contains("FROM orders")));
        // An error that carries the token is reported with it.
        assert!(hits.iter().any(|h| h.detail.contains("deadlock")));
        assert!(hits[0].open.is_some(), "a hit can be opened");
    }

    /// A token from another workspace's run is not the same incident.
    #[test]
    fn another_workspaces_history_is_not_searched() {
        let (_d, db) = seeded();
        let hits =
            search_history(&db, "ws1", &Query::new("4bf92f3577b34da6a3ce929d0e0e4736")).unwrap();
        assert!(!hits.iter().any(|h| h.detail.contains("other")));
    }

    #[test]
    fn nothing_matching_is_an_empty_answer_not_an_error() {
        let (_d, db) = seeded();
        assert!(
            search_history(&db, "ws1", &Query::new("nope-nothing"))
                .unwrap()
                .is_empty()
        );
    }

    /// A search must never create the database: a workspace that has run
    /// no SQL has nothing to find, which is not a failure.
    #[test]
    fn a_missing_database_finds_nothing_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("none.sqlite");
        assert!(
            search_history(&db, "ws1", &Query::new("abc12345"))
                .unwrap()
                .is_empty()
        );
        assert!(!db.exists(), "the search did not create it");
    }

    /// A token with a LIKE wildcard in it must match literally, or it
    /// would quietly match everything.
    #[test]
    fn like_wildcards_in_a_token_are_escaped() {
        let (_d, db) = seeded();
        assert!(
            search_history(&db, "ws1", &Query::new("%"))
                .unwrap()
                .is_empty(),
            "a bare percent is a literal, not every row"
        );
        assert_eq!(escape_like("a_b%c"), r"a\_b\%c");
    }

    #[test]
    fn the_limit_is_honoured() {
        let (_d, db) = seeded();
        let mut q = Query::new("SELECT");
        q.limit = 1;
        assert_eq!(search_history(&db, "ws1", &q).unwrap().len(), 1);
    }

    #[test]
    fn a_formatted_statement_previews_as_one_line() {
        assert_eq!(one_line("SELECT\n  *\nFROM  t", 80), "SELECT * FROM t");
        assert!(one_line(&"x".repeat(200), 20).ends_with('…'));
        assert_eq!(one_line(&"x".repeat(200), 20).chars().count(), 21);
    }
}
