//! HyperStack's own record of what it changed on an identity server
//! (docs/tools/idm.md §6.4): one JSON line per write in
//! `<workspace>/idm/audit.jsonl`, the **names** of the fields changed and
//! never their values. The server's admin events stay the authoritative
//! audit; this is what *this app* did.

use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// The most entries kept in memory for Recent Changes.
const TAIL: usize = 200;

/// One write, as the log stores it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AuditEntry {
    /// RFC 3339, UTC.
    pub at: String,
    /// The connection profile's id.
    pub connection: String,
    /// The server's base URL.
    pub server: String,
    pub realm: String,
    /// user, group, role, client, session, realm.
    pub object: String,
    pub id: String,
    pub name: String,
    /// create, update, delete, enable, disable, reset-password,
    /// join-group, leave-group, logout, end-session, assign-roles,
    /// unassign-roles, regenerate-secret, logout-all.
    pub verb: String,
    /// The names of what changed - never a value.
    pub fields: Vec<String>,
    /// The client id or username the token was issued to.
    pub by: String,
    pub os_user: String,
    /// `ok`, or the error in words.
    pub outcome: String,
}

/// Who and where, the same for every entry a tab writes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuditContext {
    pub connection: String,
    pub server: String,
}

/// The workspace's audit file. Appends are serialised; the last
/// [`TAIL`] entries are kept in memory once read, so Recent Changes does
/// not read the file every frame.
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    tail: Mutex<Option<Vec<AuditEntry>>>,
}

impl AuditLog {
    /// The log of the workspace rooted at `root`.
    pub fn in_workspace(root: &Path) -> AuditLog {
        AuditLog::at(root.join("idm").join("audit.jsonl"))
    }

    pub fn at(path: PathBuf) -> AuditLog {
        AuditLog {
            path,
            tail: Mutex::new(None),
        }
    }

    /// The one log of the workspace rooted at `root`, shared by its tabs
    /// and its dashboard, so Recent Changes shows a write the moment a tab
    /// appends it rather than what the file held when it was first read.
    pub fn shared(root: &Path) -> std::sync::Arc<AuditLog> {
        use std::collections::HashMap;
        use std::sync::{Arc, OnceLock};
        static LOGS: OnceLock<Mutex<HashMap<PathBuf, Arc<AuditLog>>>> = OnceLock::new();
        let log = AuditLog::in_workspace(root);
        LOGS.get_or_init(Default::default)
            .lock()
            .entry(log.path.clone())
            .or_insert_with(|| Arc::new(log))
            .clone()
    }

    /// The last `n` entries, newest first, if the file has been read -
    /// never reads it, so a frame can ask. `None` until [`AuditLog::recent`]
    /// has run once (off the UI thread).
    pub fn peek(&self, n: usize) -> Option<Vec<AuditEntry>> {
        let tail = self.tail.lock();
        tail.as_ref()
            .map(|t| t.iter().rev().take(n).cloned().collect())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one entry. `at` and `os_user` are filled in when empty.
    pub fn append(&self, mut e: AuditEntry) -> std::io::Result<()> {
        if e.at.is_empty() {
            e.at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        }
        if e.os_user.is_empty() {
            e.os_user = std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_default();
        }
        let mut tail = self.tail.lock();
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut line = serde_json::to_string(&e).map_err(std::io::Error::other)?;
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?
            .write_all(line.as_bytes())?;
        if let Some(t) = tail.as_mut() {
            t.push(e);
            let over = t.len().saturating_sub(TAIL);
            t.drain(..over);
        }
        Ok(())
    }

    fn read_all(&self) -> Vec<AuditEntry> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }

    /// The last `n` entries, newest first. The first call reads the file;
    /// later ones answer from memory.
    pub fn recent(&self, n: usize) -> Vec<AuditEntry> {
        let mut tail = self.tail.lock();
        let t = tail.get_or_insert_with(|| {
            let mut all = self.read_all();
            let over = all.len().saturating_sub(TAIL);
            all.drain(..over);
            all
        });
        t.iter().rev().take(n).cloned().collect()
    }

    /// Clear History: empties the file, not only the list (CR-0067).
    pub fn clear(&self) -> std::io::Result<()> {
        let mut tail = self.tail.lock();
        if self.path.exists() {
            std::fs::write(&self.path, b"")?;
        }
        *tail = Some(Vec::new());
        Ok(())
    }

    /// Remove one entry (the row's Remove), by equality.
    pub fn remove(&self, entry: &AuditEntry) -> std::io::Result<()> {
        let mut tail = self.tail.lock();
        let mut all = self.read_all();
        if let Some(i) = all.iter().rposition(|e| e == entry) {
            all.remove(i);
        }
        let mut text = String::new();
        for e in &all {
            text.push_str(&serde_json::to_string(e).map_err(std::io::Error::other)?);
            text.push('\n');
        }
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, text)?;
        let over = all.len().saturating_sub(TAIL);
        all.drain(..over);
        *tail = Some(all);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(verb: &str) -> AuditEntry {
        AuditEntry {
            connection: "01JC".into(),
            server: "http://127.0.0.1:8180".into(),
            realm: "hs-demo".into(),
            object: "user".into(),
            id: "5b1c".into(),
            name: "alice".into(),
            verb: verb.into(),
            fields: vec!["email".into()],
            by: "hs-admin".into(),
            outcome: "ok".into(),
            ..Default::default()
        }
    }

    #[test]
    fn entries_append_as_json_lines_and_read_back_newest_first() {
        let dir = tempfile::tempdir().expect("dir");
        let log = AuditLog::in_workspace(dir.path());
        assert!(log.recent(10).is_empty());
        log.append(entry("update")).expect("append");
        log.append(entry("delete")).expect("append");
        let text = std::fs::read_to_string(dir.path().join("idm/audit.jsonl")).expect("file");
        assert_eq!(text.lines().count(), 2);
        let back = log.recent(10);
        assert_eq!(back[0].verb, "delete");
        assert!(!back[0].at.is_empty());
        // A fresh reader sees the same file.
        assert_eq!(
            AuditLog::in_workspace(dir.path()).recent(1)[0].verb,
            "delete"
        );
    }

    #[test]
    fn clear_empties_the_file_and_remove_takes_one_row() {
        let dir = tempfile::tempdir().expect("dir");
        let log = AuditLog::in_workspace(dir.path());
        log.append(entry("update")).expect("append");
        log.append(entry("delete")).expect("append");
        let first = log.recent(10)[1].clone();
        log.remove(&first).expect("remove");
        assert_eq!(log.recent(10).len(), 1);
        assert_eq!(
            AuditLog::in_workspace(dir.path()).recent(10)[0].verb,
            "delete"
        );
        log.clear().expect("clear");
        assert!(log.recent(10).is_empty());
        assert_eq!(
            std::fs::read_to_string(log.path()).expect("file"),
            String::new()
        );
    }
}
