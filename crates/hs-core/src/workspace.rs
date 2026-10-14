//! Workspaces: a folder holding connections, environments, collections, saved
//! queries and the last layout (docs/05 §1). The registry of known workspace
//! folders lives in `workspaces.toml` in the data folder.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::paths::DataDir;
use crate::storage;

pub const SCHEMA: u32 = 1;

/// Image extensions accepted for a workspace logo. The file is always stored
/// as `icon.<ext>` in the workspace folder, so it travels with the workspace
/// when the folder is copied to another machine.
pub const ICON_EXTS: &[&str] = &["png", "ico", "jpg", "jpeg", "bmp", "gif", "webp", "svg"];

/// The workspace logo file (`icon.<ext>`) if one exists in `root`.
pub fn workspace_icon_file(root: &Path) -> Option<PathBuf> {
    ICON_EXTS
        .iter()
        .map(|ext| root.join(format!("icon.{ext}")))
        .find(|p| p.is_file())
}

/// Copy an image at `src` into the workspace folder as `icon.<ext>`, replacing
/// any existing logo. Returns the stored file name. Rejects unknown
/// extensions.
pub fn set_workspace_icon(root: &Path, src: &Path) -> storage::Result<String> {
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| ICON_EXTS.contains(&e.as_str()))
        .ok_or_else(|| {
            storage::StorageError::Message("choose a PNG, ICO, JPG, BMP, GIF, WEBP or SVG".into())
        })?;
    let io_err = |path: &Path| {
        let p = path.display().to_string();
        move |source| storage::StorageError::Io {
            path: p.clone(),
            source,
        }
    };
    std::fs::create_dir_all(root).map_err(io_err(root))?;
    // Drop any logo in another format first so only one remains.
    if let Some(old) = workspace_icon_file(root) {
        let _ = std::fs::remove_file(old);
    }
    let dst = root.join(format!("icon.{ext}"));
    std::fs::copy(src, &dst).map_err(io_err(src))?;
    Ok(format!("icon.{ext}"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkspaceMeta {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub created: String,
    /// Environment id selected when the workspace opens.
    pub default_environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Registry {
    pub schema: u32,
    pub last_opened: String,
    /// name → path (relative to the data folder when inside it).
    pub workspaces: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegistryEntry {
    pub name: String,
    pub path: String,
}

/// The persisted shape of the tab space (docs/04 §2.5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Layout {
    pub schema: u32,
    pub tabs: Vec<LayoutTab>,
    pub active: Option<u64>,
    pub split: Option<LayoutNode>,
    pub sidebar_width: Option<f32>,
    pub dock_height: Option<f32>,
    pub dock_open: bool,
    pub sidebar_open: bool,
    pub active_environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LayoutTab {
    pub id: u64,
    pub tool: String,
    pub pinned: bool,
    pub colour: Option<[u8; 3]>,
    pub state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LayoutNode {
    Leaf {
        tab: u64,
    },
    Split {
        horizontal: bool,
        ratio: f32,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

#[derive(Debug, Clone)]
pub struct Workspace {
    pub meta: WorkspaceMeta,
    root: PathBuf,
}

impl Workspace {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn environments_dir(&self) -> PathBuf {
        self.root.join("environments")
    }
    pub fn connections_dir(&self) -> PathBuf {
        self.root.join("connections")
    }
    pub fn layout_file(&self) -> PathBuf {
        self.root.join("layout.json")
    }
    pub fn tab_groups_dir(&self) -> PathBuf {
        self.root.join("tab-groups")
    }
    /// The workspace logo file (`icon.<ext>`) if one has been set.
    pub fn icon_file(&self) -> Option<PathBuf> {
        workspace_icon_file(&self.root)
    }
    /// Store `src` as this workspace's logo (`icon.<ext>`), replacing any
    /// existing one.
    pub fn set_icon(&self, src: &Path) -> storage::Result<String> {
        set_workspace_icon(&self.root, src)
    }

    /// Open an existing workspace folder or create a fresh one.
    pub fn open_or_create(root: &Path, name: &str) -> storage::Result<Self> {
        let meta_path = root.join("workspace.toml");
        let meta = if meta_path.exists() {
            storage::read_toml_versioned::<WorkspaceMeta>(&meta_path, SCHEMA)?
        } else {
            for sub in [
                "environments",
                "connections",
                "api",
                "queries",
                "snippets",
                "tab-groups",
            ] {
                std::fs::create_dir_all(root.join(sub)).map_err(|e| storage::StorageError::Io {
                    path: root.join(sub).display().to_string(),
                    source: e,
                })?;
            }
            let meta = WorkspaceMeta {
                schema: SCHEMA,
                id: crate::new_id(),
                name: name.to_string(),
                created: chrono::Utc::now().to_rfc3339(),
                default_environment: None,
            };
            storage::write_toml(&meta_path, &meta)?;
            let gitignore = root.join(".gitignore");
            if !gitignore.exists() {
                storage::write_atomic(&gitignore, b"layout.json\n*.local.toml\n")?;
            }
            meta
        };
        Ok(Self {
            meta,
            root: root.to_path_buf(),
        })
    }

    pub fn save_meta(&self) -> storage::Result<()> {
        storage::write_toml(&self.root.join("workspace.toml"), &self.meta)
    }

    pub fn load_layout(&self) -> Layout {
        let p = self.layout_file();
        if p.exists() {
            storage::read_json(&p).unwrap_or_default()
        } else {
            Layout::default()
        }
    }

    pub fn save_layout(&self, layout: &Layout) -> storage::Result<()> {
        let mut l = layout.clone();
        l.schema = SCHEMA;
        storage::write_json(&self.layout_file(), &l)
    }
}

impl Registry {
    pub fn load(paths: &DataDir) -> Self {
        let p = paths.workspaces_registry();
        if p.exists() {
            storage::read_toml(&p).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self, paths: &DataDir) -> storage::Result<()> {
        let mut r = self.clone();
        r.schema = SCHEMA;
        storage::write_toml(&paths.workspaces_registry(), &r)
    }

    /// Resolve a workspace by name; the built-in `default` is created on demand.
    pub fn open(&mut self, paths: &DataDir, name: &str) -> storage::Result<Workspace> {
        let root = match self.workspaces.iter().find(|w| w.name == name) {
            Some(e) => paths.resolve_path(&e.path),
            None => paths.workspaces_dir().join(storage::slug(name)),
        };
        let ws = Workspace::open_or_create(&root, name)?;
        if !self.workspaces.iter().any(|w| w.name == name) {
            self.workspaces.push(RegistryEntry {
                name: name.to_string(),
                path: paths.relativize_str(&root),
            });
        }
        self.last_opened = name.to_string();
        self.save(paths)?;
        Ok(ws)
    }

    /// Load a registered workspace without opening it (no last-opened
    /// change, no registry write). `None` when the name is unknown.
    pub fn peek(&self, paths: &DataDir, name: &str) -> storage::Result<Workspace> {
        let root = match self.workspaces.iter().find(|w| w.name == name) {
            Some(e) => paths.resolve_path(&e.path),
            None if name == "default" => paths.workspaces_dir().join("default"),
            None => {
                return Err(storage::StorageError::Message(format!(
                    "no workspace named \"{name}\""
                )));
            }
        };
        Workspace::open_or_create(&root, name)
    }

    pub fn exists(&self, name: &str) -> bool {
        name == "default" || self.workspaces.iter().any(|w| w.name == name)
    }

    /// Rename a registered workspace without moving its folder. The path is
    /// stable, so external project folders and relative registry entries keep
    /// working. Also updates the metadata in the currently open workspace.
    pub fn rename_open(
        &mut self,
        paths: &DataDir,
        workspace: &mut Workspace,
        new_name: &str,
    ) -> storage::Result<()> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(storage::StorageError::Message(
                "the workspace needs a name".into(),
            ));
        }
        let old_name = workspace.meta.name.clone();
        if new_name == old_name {
            return Ok(());
        }
        if old_name == "default" {
            return Err(storage::StorageError::Message(
                "the built-in default workspace cannot be renamed".into(),
            ));
        }
        if self.workspaces.iter().any(|w| w.name == new_name) {
            return Err(storage::StorageError::Message(format!(
                "a workspace named \"{new_name}\" already exists"
            )));
        }
        let previous_last_opened = self.last_opened.clone();
        let previous = workspace.meta.name.clone();
        workspace.meta.name = new_name.to_string();
        if let Err(e) = workspace.save_meta() {
            workspace.meta.name = previous;
            return Err(e);
        }
        if let Some(entry) = self.workspaces.iter_mut().find(|w| w.name == old_name) {
            entry.name = new_name.to_string();
        }
        // This workspace is the one currently being renamed, so the registry
        // should point its last-opened marker at its new name even if the old
        // marker was stale (startup preferences can open a different entry).
        self.last_opened = new_name.to_string();
        if let Err(e) = self.save(paths) {
            // Best-effort rollback of the file and in-memory state.
            workspace.meta.name = old_name.clone();
            let _ = workspace.save_meta();
            if let Some(entry) = self.workspaces.iter_mut().find(|w| w.name == new_name) {
                entry.name = old_name.clone();
            }
            self.last_opened = previous_last_opened;
            return Err(e);
        }
        Ok(())
    }

    pub fn names(&self) -> Vec<String> {
        self.workspaces.iter().map(|w| w.name.clone()).collect()
    }

    /// Create a new workspace, optionally seeding its environments and
    /// connections from `copy_from`. Errors if the name already exists.
    pub fn create(
        &mut self,
        paths: &DataDir,
        name: &str,
        copy_from: Option<&Workspace>,
        icon: Option<&Path>,
    ) -> storage::Result<Workspace> {
        let name = name.trim();
        if name.is_empty() {
            return Err(storage::StorageError::Message(
                "the workspace needs a name".into(),
            ));
        }
        if self.exists(name) {
            return Err(storage::StorageError::Message(format!(
                "a workspace named \"{name}\" already exists"
            )));
        }
        let root = paths.workspaces_dir().join(storage::slug(name));
        let ws = Workspace::open_or_create(&root, name)?;
        if let Some(src) = copy_from {
            copy_dir_contents(&src.environments_dir(), &ws.environments_dir())?;
            copy_dir_contents(&src.connections_dir(), &ws.connections_dir())?;
        }
        if let Some(src) = icon {
            ws.set_icon(src)?;
        }
        self.workspaces.push(RegistryEntry {
            name: name.to_string(),
            path: paths.relativize_str(&root),
        });
        self.save(paths)?;
        Ok(ws)
    }

    /// Copy every environment from another workspace into `target`, skipping
    /// names that already exist there. Returns how many were copied.
    pub fn clone_environments(from: &Workspace, target: &Workspace) -> storage::Result<usize> {
        let src = crate::environments::Environments::load(&from.environments_dir());
        let mut dst = crate::environments::Environments::load(&target.environments_dir());
        let existing: std::collections::HashSet<String> =
            dst.all().iter().map(|e| e.name.clone()).collect();
        let mut n = 0;
        for e in src.all() {
            if existing.contains(&e.name) {
                continue;
            }
            let mut copy = e.clone();
            copy.id = crate::new_id();
            dst.upsert(copy)?;
            n += 1;
        }
        Ok(n)
    }

    /// Remove a workspace from the registry. The built-in `default` cannot be
    /// removed. When `delete_files` is set and the folder lives inside the
    /// portable data dir, its files are deleted too.
    pub fn remove(
        &mut self,
        paths: &DataDir,
        name: &str,
        delete_files: bool,
    ) -> storage::Result<()> {
        if name == "default" {
            return Err(storage::StorageError::Message(
                "the default workspace cannot be removed".into(),
            ));
        }
        let entry = self.workspaces.iter().find(|w| w.name == name).cloned();
        self.workspaces.retain(|w| w.name != name);
        if self.last_opened == name {
            self.last_opened = "default".into();
        }
        if delete_files && let Some(e) = entry {
            let root = paths.resolve_path(&e.path);
            // Only delete inside the managed workspaces folder (never an
            // external repo folder the user registered).
            if root.starts_with(paths.workspaces_dir()) && root.is_dir() {
                let _ = std::fs::remove_dir_all(&root);
            }
        }
        self.save(paths)
    }

    /// Register an external folder (for example a repo's `.hyperstack/`).
    pub fn add_external(
        &mut self,
        paths: &DataDir,
        name: &str,
        root: &Path,
    ) -> storage::Result<Workspace> {
        let ws = Workspace::open_or_create(root, name)?;
        self.workspaces.retain(|w| w.name != name);
        self.workspaces.push(RegistryEntry {
            name: name.to_string(),
            path: paths.relativize_str(root),
        });
        self.save(paths)?;
        Ok(ws)
    }
}

/// Copy every file in `from` into `to` (one level deep, by kind subfolders),
/// creating `to` as needed. Used to seed a new workspace.
fn copy_dir_contents(from: &Path, to: &Path) -> storage::Result<()> {
    if !from.is_dir() {
        return Ok(());
    }
    let io_err = |path: &Path| {
        let p = path.display().to_string();
        move |source| storage::StorageError::Io {
            path: p.clone(),
            source,
        }
    };
    std::fs::create_dir_all(to).map_err(io_err(to))?;
    for entry in std::fs::read_dir(from).map_err(io_err(from))?.flatten() {
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if src.is_dir() {
            copy_dir_contents(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(io_err(&src))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workspace_is_created_registered_and_layout_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DataDir::resolve(Some(tmp.path())).unwrap();
        let mut reg = Registry::load(&paths);
        let ws = reg.open(&paths, "default").unwrap();
        assert!(ws.root().join("workspace.toml").exists());
        assert!(ws.environments_dir().is_dir());
        let reg2 = Registry::load(&paths);
        assert_eq!(reg2.last_opened, "default");
        assert_eq!(reg2.workspaces[0].path, "workspaces/default");

        let layout = Layout {
            tabs: vec![LayoutTab {
                id: 1,
                tool: "scratchpad".into(),
                pinned: true,
                colour: None,
                state: serde_json::json!({"text": "hi"}),
            }],
            active: Some(1),
            split: Some(LayoutNode::Leaf { tab: 1 }),
            sidebar_open: true,
            ..Default::default()
        };
        ws.save_layout(&layout).unwrap();
        let back = ws.load_layout();
        assert_eq!(back.tabs, layout.tabs);
        assert_eq!(back.split, layout.split);
        assert!(ws.root().join(".gitignore").exists());
    }

    #[test]
    fn renaming_workspace_updates_registry_and_metadata_without_moving_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DataDir::resolve(Some(tmp.path())).unwrap();
        let mut registry = Registry::load(&paths);
        let default = registry.open(&paths, "default").unwrap();
        let mut ws = registry
            .create(&paths, "Shop API", Some(&default), None)
            .unwrap();
        registry.last_opened = "some stale entry".into();
        let folder = ws.root().to_path_buf();
        registry.rename_open(&paths, &mut ws, "Orders API").unwrap();
        assert_eq!(ws.meta.name, "Orders API");
        assert_eq!(ws.root(), folder);
        let loaded = Registry::load(&paths);
        assert_eq!(loaded.last_opened, "Orders API");
        assert!(
            loaded
                .workspaces
                .iter()
                .any(|w| { w.name == "Orders API" && paths.resolve_path(&w.path) == folder })
        );
        assert!(!loaded.workspaces.iter().any(|w| w.name == "Shop API"));
        let metadata: WorkspaceMeta =
            storage::read_toml_versioned(&folder.join("workspace.toml"), SCHEMA).unwrap();
        assert_eq!(metadata.name, "Orders API");
    }
}
