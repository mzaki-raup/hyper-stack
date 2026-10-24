//! Saved commands and configs: the second sidebar section
//! (`docs/tools/containers.md` §10.5).
//!
//! A Compose file, a stack file, a manifest or a one-off command worth
//! keeping. They live in `<workspace>/containers/*.toml`, each file
//! named `slug(name)` — a file under any other name is duplicated the
//! first time anything saves it (the connection-profile gotcha in
//! CLAUDE.md, which cost two hours once).

use std::path::{Path, PathBuf};

use hs_core::storage;
use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

/// Where these live, under the workspace.
pub fn dir(workspace_root: &Path) -> PathBuf {
    workspace_root.join("containers")
}

/// What kind of thing was saved. Decides the row's icon and what
/// opening it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A `docker-compose.yml` / `compose.yaml`.
    #[default]
    Compose,
    /// A Swarm stack file, deployed with `docker stack deploy`.
    Stack,
    /// A Kubernetes manifest.
    Manifest,
    /// A command to run against the host.
    Command,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Compose, Kind::Stack, Kind::Manifest, Kind::Command];

    pub fn key(self) -> &'static str {
        match self {
            Kind::Compose => "container-config-compose",
            Kind::Stack => "container-config-stack",
            Kind::Manifest => "container-config-manifest",
            Kind::Command => "container-config-command",
        }
    }

    /// What the thing **is**, as every list in this app now shows.
    pub fn icon(self) -> &'static str {
        use hs_ui::icons::ph;
        match self {
            Kind::Compose => ph::STACK,
            Kind::Stack => ph::SQUARES_FOUR,
            Kind::Manifest => ph::FILE_CODE,
            Kind::Command => ph::TERMINAL_WINDOW,
        }
    }

    /// A guess from a file name, so saving one does not start with a
    /// question the name already answers.
    pub fn of_path(path: &str) -> Self {
        let name = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.starts_with("docker-compose") || name.starts_with("compose.") {
            return Kind::Compose;
        }
        if name.contains("stack") {
            return Kind::Stack;
        }
        Kind::Manifest
    }
}

/// One saved command or config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedConfig {
    pub schema: u32,
    pub name: String,
    pub kind: Kind,
    /// The file, for a Compose, Stack or Manifest. Relative paths are
    /// resolved against the workspace.
    pub path: String,
    /// The command, for a Command. Never a secret: a command that needs
    /// one takes it from the environment, as every other tool here does.
    pub command: String,
    /// The host this belongs to, by profile id. Empty means "whichever
    /// host is open", which is what a manifest usually wants.
    pub host_id: String,
    /// Where it sits in the list.
    pub order: u32,
    pub note: String,
}

impl Default for SavedConfig {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            name: String::new(),
            kind: Kind::Compose,
            path: String::new(),
            command: String::new(),
            host_id: String::new(),
            order: 0,
            note: String::new(),
        }
    }
}

impl SavedConfig {
    pub fn file_for(&self, workspace_root: &Path) -> PathBuf {
        dir(workspace_root).join(format!("{}.toml", storage::slug(&self.name)))
    }

    pub fn load_all(workspace_root: &Path) -> Vec<SavedConfig> {
        let mut out = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir(workspace_root)) else {
            return out;
        };
        for e in entries.flatten() {
            let p = e.path();
            if !p.extension().is_some_and(|x| x == "toml") {
                continue;
            }
            match storage::read_toml::<SavedConfig>(&p) {
                Ok(c) if c.schema <= SCHEMA => out.push(c),
                Ok(_) => tracing::warn!("skipping container config {}: newer schema", p.display()),
                Err(err) => tracing::warn!("skipping container config {}: {err}", p.display()),
            }
        }
        out.sort_by(|a, b| {
            a.order
                .cmp(&b.order)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        out
    }

    pub fn save(&self, workspace_root: &Path) -> storage::Result<()> {
        let _ = std::fs::create_dir_all(dir(workspace_root));
        storage::write_toml(&self.file_for(workspace_root), self)
    }

    pub fn delete(&self, workspace_root: &Path) -> std::io::Result<()> {
        let p = self.file_for(workspace_root);
        if p.exists() {
            std::fs::remove_file(p)?;
        }
        Ok(())
    }

    /// What the row says under the name.
    pub fn describe(&self) -> String {
        if !self.note.trim().is_empty() {
            return self.note.trim().to_string();
        }
        match self.kind {
            Kind::Command => self.command.clone(),
            _ => self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn sample() -> SavedConfig {
        SavedConfig {
            name: "Shop Stack".into(),
            kind: Kind::Compose,
            path: "deploy/docker-compose.yml".into(),
            host_id: "host-1".into(),
            note: "the whole shop".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_config_round_trips_through_its_file() {
        let d = tmp();
        let c = sample();
        c.save(d.path()).expect("saved");
        let back = SavedConfig::load_all(d.path());
        assert_eq!(back.len(), 1);
        assert_eq!(back[0], c);
    }

    /// The file name is the slug of the name, as every other store here
    /// does it: anything else is duplicated on the first save.
    #[test]
    fn the_file_name_is_the_slug_and_saving_twice_is_one_file() {
        let d = tmp();
        let c = sample();
        assert_eq!(c.file_for(d.path()).file_name().unwrap(), "shop-stack.toml");
        c.save(d.path()).unwrap();
        c.save(d.path()).unwrap();
        assert_eq!(SavedConfig::load_all(d.path()).len(), 1);
    }

    #[test]
    fn a_file_from_the_future_is_skipped() {
        let d = tmp();
        let mut c = sample();
        c.schema = SCHEMA + 1;
        c.save(d.path()).unwrap();
        assert!(SavedConfig::load_all(d.path()).is_empty());
    }

    #[test]
    fn no_folder_means_no_configs() {
        assert!(SavedConfig::load_all(tmp().path()).is_empty());
    }

    #[test]
    fn the_list_follows_the_users_order_then_the_name() {
        let d = tmp();
        for (name, order) in [("Zeta", 0u32), ("Alpha", 1), ("Beta", 0)] {
            SavedConfig {
                name: name.into(),
                ..Default::default()
            }
            .save(d.path())
            .unwrap();
            // Saved separately so each file exists before the order is set.
            let mut c = SavedConfig::load_all(d.path())
                .into_iter()
                .find(|c| c.name == name)
                .unwrap();
            c.order = order;
            c.save(d.path()).unwrap();
        }
        let names: Vec<String> = SavedConfig::load_all(d.path())
            .into_iter()
            .map(|c| c.name)
            .collect();
        assert_eq!(names, ["Beta", "Zeta", "Alpha"]);
    }

    /// The name usually says what the file is, so saving one should not
    /// start by asking.
    #[test]
    fn a_kind_is_guessed_from_the_file_name() {
        assert_eq!(Kind::of_path("deploy/docker-compose.yml"), Kind::Compose);
        assert_eq!(Kind::of_path("compose.yaml"), Kind::Compose);
        assert_eq!(Kind::of_path("deploy/stack.yml"), Kind::Stack);
        assert_eq!(Kind::of_path("k8s/deployment.yaml"), Kind::Manifest);
        // Nothing to go on is a manifest, which is the commonest.
        assert_eq!(Kind::of_path(""), Kind::Manifest);
    }

    #[test]
    fn the_row_prefers_the_note_then_says_what_it_points_at() {
        assert_eq!(sample().describe(), "the whole shop");
        let bare = SavedConfig {
            note: "  ".into(),
            ..sample()
        };
        assert_eq!(bare.describe(), "deploy/docker-compose.yml");
        let cmd = SavedConfig {
            kind: Kind::Command,
            command: "docker compose up -d".into(),
            note: String::new(),
            ..Default::default()
        };
        assert_eq!(cmd.describe(), "docker compose up -d");
    }

    #[test]
    fn every_kind_has_its_own_icon_and_label() {
        let mut icons: Vec<&str> = Kind::ALL.iter().map(|k| k.icon()).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), Kind::ALL.len(), "no two kinds share a glyph");
        for k in Kind::ALL {
            assert!(k.key().starts_with("container-config-"));
        }
    }
}
