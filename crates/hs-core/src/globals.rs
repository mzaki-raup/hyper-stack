//! Global variables: one set, for the whole application.
//!
//! An environment belongs to a workspace and one of them is active at a
//! time. A **global** belongs to nobody: it reads the same in every
//! workspace, under every environment, from every tool - the API
//! client's `pm.globals`, a `{{name}}` in the Scratchpad, the Browser,
//! a connection form (owner's requirement, 2026-10-29). It lives in one
//! file beside the executable (`HyperStackData/globals.json`) rather than
//! under a workspace, and every `Environments` the app loads is given a
//! copy to resolve `{{name}}` against last, after the environment chain.
//!
//! Until 2026-10-29 the API client kept these per workspace in
//! `<workspace>/api/globals.json`; the first start after that takes the
//! current workspace's file as the seed.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::environments::Environments;

pub const SCHEMA: u32 = 1;

/// One variable. The same shape the API client's collections use, so a
/// row moves between the two without translation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Kv {
    pub enabled: bool,
    pub key: String,
    pub value: String,
    pub description: String,
}

impl Default for Kv {
    fn default() -> Self {
        Self {
            enabled: true,
            key: String::new(),
            value: String::new(),
            description: String::new(),
        }
    }
}

impl Kv {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ..Default::default()
        }
    }
}

/// `globals.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Globals {
    pub schema: u32,
    pub values: Vec<Kv>,
}

/// Bumped on every save, so a tool holding a copy knows to read the
/// file again - Settings and the API client both edit the same set.
static VERSION: AtomicU64 = AtomicU64::new(1);

/// The current edition of the global set. Compare against a remembered
/// value; equality means "nothing has been saved since".
pub fn version() -> u64 {
    VERSION.load(Ordering::Relaxed)
}

impl Globals {
    pub fn load(path: &Path) -> Self {
        if path.exists() {
            crate::storage::read_json(path).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, path: &Path) -> crate::storage::Result<()> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let mut g = self.clone();
        g.schema = SCHEMA;
        crate::storage::write_json(path, &g)?;
        VERSION.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|v| v.enabled && v.key == key)
            .map(|v| v.value.as_str())
    }

    pub fn set(&mut self, key: &str, value: &str) {
        match self.values.iter_mut().find(|v| v.key == key) {
            Some(v) => {
                v.value = value.to_string();
                v.enabled = true;
            }
            None => self.values.push(Kv::new(key, value)),
        }
    }

    pub fn unset(&mut self, key: &str) {
        self.values.retain(|v| v.key != key);
    }

    /// The enabled variables as a map - what `{{name}}` resolves against.
    pub fn map(&self) -> HashMap<String, String> {
        self.values
            .iter()
            .filter(|v| v.enabled && !v.key.trim().is_empty())
            .map(|v| (v.key.clone(), v.value.clone()))
            .collect()
    }

    /// Hand the set to an `Environments`, which resolves `{{name}}`
    /// against it after the environment chain. Called whenever either
    /// side changes: a workspace switch loads fresh environments, a save
    /// here changes the set.
    pub fn apply_to(&self, envs: &mut Environments) {
        envs.globals = self.map();
    }

    /// Seed the app-wide file from a workspace's old per-workspace copy
    /// the first time the app runs with this store, so nothing anyone
    /// saved is lost in the move. Does nothing once the file exists.
    pub fn migrate_from(app_file: &Path, legacy: &Path) {
        if app_file.exists() || !legacy.exists() {
            return;
        }
        let old = Self::load(legacy);
        if old.values.is_empty() {
            return;
        }
        if let Err(e) = old.save(app_file) {
            tracing::warn!("globals not migrated from {}: {e}", legacy.display());
        } else {
            tracing::info!(
                "globals moved from {} to {}",
                legacy.display(),
                app_file.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A global reads through an `Environments` after the environment
    /// chain, whatever environment is active - and a disabled one does
    /// not read at all.
    #[test]
    fn globals_resolve_last_and_everywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let mut envs = Environments::load(&tmp.path().join("envs"));
        let mut g = Globals::default();
        g.set("org", "acme");
        g.set("off", "x");
        g.values
            .iter_mut()
            .find(|v| v.key == "off")
            .unwrap()
            .enabled = false;
        g.apply_to(&mut envs);
        assert_eq!(envs.substitute("{{org}}", None, &[]).unwrap(), "acme");
        assert!(envs.substitute("{{off}}", None, &[]).is_err());
    }

    /// Saving bumps the edition, and a legacy per-workspace file seeds
    /// the app-wide one exactly once.
    #[test]
    fn a_save_is_versioned_and_the_old_file_is_taken_over_once() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("globals.json");
        let legacy = tmp.path().join("ws/api/globals.json");
        let mut old = Globals::default();
        old.set("token", "t");
        old.save(&legacy).unwrap();
        let before = version();
        Globals::migrate_from(&app, &legacy);
        assert_eq!(Globals::load(&app).get("token"), Some("t"));
        assert!(version() > before);
        // A second migration must not overwrite a newer app file.
        let mut newer = Globals::load(&app);
        newer.set("token", "new");
        newer.save(&app).unwrap();
        Globals::migrate_from(&app, &legacy);
        assert_eq!(Globals::load(&app).get("token"), Some("new"));
    }
}
