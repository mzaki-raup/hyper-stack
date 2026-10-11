//! Query history in `HyperStackData/history.sqlite`, table `query_history`
//! (docs/05-data-model-and-storage.md §3). Statements are stored with the
//! connection's secrets redacted; retention keeps 90 days and at most 50,000
//! rows. Calls are blocking and small; the tool runs them on the runtime's
//! blocking pool.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

const KEEP_DAYS: i64 = 90;
const KEEP_ROWS: i64 = 50_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: i64,
    pub created_at: String,
    pub connection_id: String,
    pub environment_id: Option<String>,
    pub sql: String,
    pub duration_ms: i64,
    pub row_count: Option<i64>,
    /// "ok" | "error" | "cancelled"
    pub status: String,
    pub error: Option<String>,
}

pub struct NewEntry<'a> {
    pub workspace_id: &'a str,
    pub connection_id: &'a str,
    pub environment_id: Option<&'a str>,
    pub sql: &'a str,
    pub duration_ms: i64,
    pub row_count: Option<i64>,
    pub status: &'a str,
    pub error: Option<&'a str>,
}

pub struct History {
    path: PathBuf,
}

fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let c = Connection::open(path)?;
    c.busy_timeout(std::time::Duration::from_secs(3))?;
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS query_history (
           id INTEGER PRIMARY KEY,
           workspace_id TEXT NOT NULL,
           created_at TEXT NOT NULL,
           environment_id TEXT,
           connection_id TEXT NOT NULL,
           sql TEXT NOT NULL,
           duration_ms INTEGER NOT NULL,
           row_count INTEGER,
           status TEXT NOT NULL,
           error TEXT
         );
         CREATE INDEX IF NOT EXISTS query_history_conn ON query_history (connection_id, id);",
    )?;
    Ok(c)
}

impl History {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Record one statement. `secrets` are replaced by `***` first.
    pub fn add(&self, e: &NewEntry, secrets: &[String]) -> rusqlite::Result<()> {
        let c = open(&self.path)?;
        let sql = hs_core::secrets::redact(e.sql, secrets);
        let error = e.error.map(|m| hs_core::secrets::redact(m, secrets));
        c.execute(
            "INSERT INTO query_history (workspace_id, created_at, environment_id, connection_id, sql, duration_ms, row_count, status, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                e.workspace_id,
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                e.environment_id,
                e.connection_id,
                sql,
                e.duration_ms,
                e.row_count,
                e.status,
                error,
            ],
        )?;
        Ok(())
    }

    /// Most recent first; `search` matches anywhere in the statement.
    pub fn recent(
        &self,
        connection_id: &str,
        search: &str,
        limit: usize,
    ) -> rusqlite::Result<Vec<Entry>> {
        let c = open(&self.path)?;
        let like = format!(
            "%{}%",
            search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut st = c.prepare(
            "SELECT id, created_at, connection_id, environment_id, sql, duration_ms, row_count, status, error
               FROM query_history WHERE connection_id = ?1 AND sql LIKE ?2 ESCAPE '\\'
              ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = st.query_map(params![connection_id, like, limit as i64], |r| {
            Ok(Entry {
                id: r.get(0)?,
                created_at: r.get(1)?,
                connection_id: r.get(2)?,
                environment_id: r.get(3)?,
                sql: r.get(4)?,
                duration_ms: r.get(5)?,
                row_count: r.get(6)?,
                status: r.get(7)?,
                error: r.get(8)?,
            })
        })?;
        rows.collect()
    }

    /// Drop entries older than the retention window or beyond the row cap.
    pub fn prune(&self) -> rusqlite::Result<usize> {
        let c = open(&self.path)?;
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(KEEP_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let mut n = c.execute(
            "DELETE FROM query_history WHERE created_at < ?1",
            params![cutoff],
        )?;
        n += c.execute(
            "DELETE FROM query_history WHERE id <= (SELECT id FROM query_history ORDER BY id DESC LIMIT 1 OFFSET ?1)",
            params![KEEP_ROWS],
        )?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_redacts_searches_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path().join("history.sqlite"));
        let add = |sql: &str, status: &str| {
            h.add(
                &NewEntry {
                    workspace_id: "ws",
                    connection_id: "c1",
                    environment_id: Some("prod"),
                    sql,
                    duration_ms: 12,
                    row_count: Some(3),
                    status,
                    error: None,
                },
                &["hunter2secret".to_string()],
            )
            .unwrap()
        };
        add("select 1", "ok");
        add("alter user app password 'hunter2secret'", "ok");
        add("select 100%_done", "error");
        let all = h.recent("c1", "", 10).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].sql, "select 100%_done", "newest first");
        assert_eq!(all[1].sql, "alter user app password '***'");
        assert_eq!(h.recent("c1", "100%_", 10).unwrap().len(), 1);
        assert_eq!(h.recent("c1", "%", 10).unwrap().len(), 1, "% is literal");
        assert!(h.recent("other", "", 10).unwrap().is_empty());
        assert_eq!(h.prune().unwrap(), 0);
        // The secret never reached the file.
        let bytes = std::fs::read(dir.path().join("history.sqlite")).unwrap();
        assert!(!bytes.windows(13).any(|w| w == b"hunter2secret"));
    }
}
