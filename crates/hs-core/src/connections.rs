//! Connection profiles shared by every tool (docs/05 §2.2). One TOML file per
//! profile under `<workspace>/connections/<kind>/<slug>.toml`; kind-specific
//! fields stay a free-form table the owning tool interprets.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ConnectionProfile {
    pub schema: u32,
    pub id: String,
    pub name: String,
    /// db | ssh | s3 | api | docker | k8s | redis | mongodb | couchdb | elasticsearch | dynamodb | cassandra
    pub kind: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub colour: Option<String>,
    pub environment: Option<String>,
    pub read_only: bool,
    /// Kind-specific fields (`[db]`, `[ssh]`, …) plus optional `[tunnel]`.
    #[serde(flatten)]
    pub fields: toml::Table,
}

impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            id: crate::new_id(),
            name: String::new(),
            kind: String::new(),
            folder: String::new(),
            tags: Vec::new(),
            colour: None,
            environment: None,
            read_only: false,
            fields: toml::Table::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct Connections {
    dir: PathBuf,
    profiles: Vec<ConnectionProfile>,
}

impl Connections {
    pub fn load(dir: &Path) -> Self {
        let mut profiles = Vec::new();
        if let Ok(kinds) = std::fs::read_dir(dir) {
            for kind in kinds.flatten().filter(|e| e.path().is_dir()) {
                if let Ok(files) = std::fs::read_dir(kind.path()) {
                    for f in files.flatten() {
                        let p = f.path();
                        if p.extension().is_some_and(|e| e == "toml") {
                            match storage::read_toml_versioned::<ConnectionProfile>(&p, SCHEMA) {
                                Ok(c) => profiles.push(c),
                                Err(e) => {
                                    tracing::warn!("skipping connection {}: {e}", p.display())
                                }
                            }
                        }
                    }
                }
            }
        }
        profiles.sort_by_key(|a| a.name.to_lowercase());
        Self {
            dir: dir.to_path_buf(),
            profiles,
        }
    }

    pub fn all(&self) -> &[ConnectionProfile] {
        &self.profiles
    }

    pub fn by_id(&self, id: &str) -> Option<&ConnectionProfile> {
        self.profiles.iter().find(|c| c.id == id)
    }

    pub fn of_kind<'a>(
        &'a self,
        kind: &'a str,
    ) -> impl Iterator<Item = &'a ConnectionProfile> + 'a {
        self.profiles.iter().filter(move |c| c.kind == kind)
    }

    fn file_for(&self, c: &ConnectionProfile) -> PathBuf {
        self.dir
            .join(&c.kind)
            .join(format!("{}.toml", storage::slug(&c.name)))
    }

    /// Insert or replace by id and write the file. A renamed profile's old file
    /// is removed.
    pub fn upsert(&mut self, mut c: ConnectionProfile) -> storage::Result<()> {
        c.schema = SCHEMA;
        if let Some(old) = self.profiles.iter().find(|p| p.id == c.id) {
            let old_file = self.file_for(old);
            if old_file != self.file_for(&c) && old_file.exists() {
                let _ = std::fs::remove_file(old_file);
            }
        }
        storage::write_toml(&self.file_for(&c), &c)?;
        self.profiles.retain(|p| p.id != c.id);
        self.profiles.push(c);
        self.profiles.sort_by_key(|a| a.name.to_lowercase());
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        if let Some(c) = self.profiles.iter().find(|p| p.id == id) {
            let f = self.file_for(c);
            if f.exists() {
                std::fs::remove_file(&f).map_err(|e| storage::StorageError::Io {
                    path: f.display().to_string(),
                    source: e,
                })?;
            }
        }
        self.profiles.retain(|p| p.id != id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_persist_one_file_per_kind_and_survive_rename() {
        let tmp = tempfile::tempdir().unwrap();
        let mut conns = Connections::load(tmp.path());
        let mut fields = toml::Table::new();
        let mut db = toml::Table::new();
        db.insert("engine".into(), toml::Value::String("postgres".into()));
        db.insert("host".into(), toml::Value::String("{{db_host}}".into()));
        fields.insert("db".into(), toml::Value::Table(db));
        let c = ConnectionProfile {
            name: "orders — staging".into(),
            kind: "db".into(),
            fields,
            ..Default::default()
        };
        let id = c.id.clone();
        conns.upsert(c).unwrap();
        assert!(tmp.path().join("db").join("orders-staging.toml").exists());

        let mut c2 = conns.by_id(&id).unwrap().clone();
        c2.name = "orders — prod".into();
        conns.upsert(c2).unwrap();
        assert!(!tmp.path().join("db").join("orders-staging.toml").exists());
        assert!(tmp.path().join("db").join("orders-prod.toml").exists());

        let reloaded = Connections::load(tmp.path());
        let back = reloaded.by_id(&id).unwrap();
        assert_eq!(back.fields["db"]["engine"].as_str(), Some("postgres"));
        assert_eq!(reloaded.of_kind("db").count(), 1);
    }
}
