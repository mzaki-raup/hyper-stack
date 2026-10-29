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
    /// The activity rail and sidebar as this workspace wants them. `None`
    /// until the workspace is first shown, when it is seeded from the
    /// global Appearance settings (which keep the pre-2026-10-23 values).
    /// Owner's requirement: rail order, visibility and sidebar widths are
    /// per workspace, not global.
    #[serde(default)]
    pub rail: Option<RailPrefs>,
}

/// Per-workspace rail and sidebar preferences (`workspace.toml`, `[rail]`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct RailPrefs {
    /// Tool ids in the order the rail shows them; ids missing here follow
    /// in their default order. Home is always first and Settings last.
    pub order: Vec<String>,
    /// Tool ids kept out of the rail. A tool is never unreachable because
    /// of this: anything that opens one of its tabs puts its entry back.
    pub hidden: Vec<String>,
    /// Sidebar width per activity (tool id); an activity without an entry
    /// starts at the global `sidebar_width`.
    pub sidebar_widths: std::collections::HashMap<String, f32>,
}

impl WorkspaceMeta {
    /// The rail preferences in force: the workspace's own, or the global
    /// Appearance values as a seed for a workspace that has none yet.
    pub fn rail_prefs(&self, seed: &crate::settings::Appearance) -> RailPrefs {
        self.rail.clone().unwrap_or_else(|| RailPrefs {
            order: seed.rail_order.clone(),
            hidden: seed.rail_hidden.clone(),
            sidebar_widths: seed.sidebar_widths.clone(),
        })
    }

    /// Change the rail preferences (seeding them first when absent).
    pub fn rail_prefs_mut(&mut self, seed: &crate::settings::Appearance) -> &mut RailPrefs {
        if self.rail.is_none() {
            self.rail = Some(self.rail_prefs(seed));
        }
        self.rail.as_mut().expect("just seeded")
    }
}

/// Read a workspace's `workspace.toml` without opening the workspace (the
/// Settings page edits the rail of workspaces that are not open).
pub fn read_meta(root: &Path) -> storage::Result<WorkspaceMeta> {
    storage::read_toml_versioned::<WorkspaceMeta>(&root.join("workspace.toml"), SCHEMA)
}

/// Write a workspace's `workspace.toml` in place.
pub fn write_meta(root: &Path, meta: &WorkspaceMeta) -> storage::Result<()> {
    storage::write_toml(&root.join("workspace.toml"), meta)
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
    pub sidebar_width: Option<f32>,
    pub dock_height: Option<f32>,
    pub dock_open: bool,
    pub sidebar_open: bool,
    pub active_environment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LayoutTab {
    pub id: u64,
    pub tool: String,
    pub pinned: bool,
    pub colour: Option<[u8; 3]>,
    /// This tab is its activity's permanent home tab. Remembered, because
    /// working out which tab it is from the order would turn whichever
    /// connection happened to be first into a dashboard on the next start.
    /// Owner's report, 2026-10-17.
    pub home: bool,
    pub state: serde_json::Value,
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
    /// The saved project list the Editor activity shows
    /// (`hs_core::projects`).
    pub fn projects_file(&self) -> PathBuf {
        self.root.join("projects.toml")
    }
    /// The saved repository list the Git activity shows
    /// (`hs_core::git_repos`).
    pub fn git_repos_file(&self) -> PathBuf {
        self.root.join("git-repos.toml")
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
                rail: None,
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
        self.workspaces.iter().any(|w| w.name == name)
    }

    /// The workspace to open at startup when none is configured: the one
    /// opened last, else the first registered, else the built-in default
    /// (created on demand).
    pub fn startup_name(&self) -> String {
        if !self.last_opened.is_empty() && self.exists(&self.last_opened) {
            return self.last_opened.clone();
        }
        self.workspaces
            .first()
            .map(|w| w.name.clone())
            .unwrap_or_else(|| "default".into())
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
        // Any workspace can be deleted, the built-in default included, as
        // long as one remains: the app always has a workspace open.
        if !self.workspaces.iter().any(|w| w.name == name) {
            return Err(storage::StorageError::Message(format!(
                "no workspace named \"{name}\""
            )));
        }
        if self.workspaces.len() <= 1 {
            return Err(storage::StorageError::Message(
                "the last workspace cannot be deleted; create another one first".into(),
            ));
        }
        let entry = self.workspaces.iter().find(|w| w.name == name).cloned();
        self.workspaces.retain(|w| w.name != name);
        if self.last_opened == name {
            self.last_opened = self
                .workspaces
                .first()
                .map(|w| w.name.clone())
                .unwrap_or_default();
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
    fn rail_prefs_are_seeded_from_appearance_then_kept_per_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("ws");
        let mut ws = Workspace::open_or_create(&root, "ws").unwrap();
        assert!(ws.meta.rail.is_none());
        let mut seed = crate::settings::Appearance {
            rail_order: vec!["logs".into(), "editor".into()],
            rail_hidden: vec!["browser".into()],
            sidebar_widths: [("db".to_string(), 333.0)].into_iter().collect(),
            ..Default::default()
        };
        // Reading seeds nothing on disk; it only answers with the seed.
        let read = ws.meta.rail_prefs(&seed);
        assert_eq!(read.order, seed.rail_order);
        assert!(ws.meta.rail.is_none());
        // Writing seeds first, then changes, and the file carries it.
        ws.meta.rail_prefs_mut(&seed).hidden.clear();
        ws.meta
            .rail_prefs_mut(&seed)
            .sidebar_widths
            .insert("db".into(), 240.0);
        ws.save_meta().unwrap();
        let back = read_meta(&root).unwrap();
        let prefs = back.rail.clone().unwrap();
        assert_eq!(prefs.order, vec!["logs".to_string(), "editor".to_string()]);
        assert!(prefs.hidden.is_empty());
        assert_eq!(prefs.sidebar_widths["db"], 240.0);
        // A later change to the global seed no longer reaches it.
        seed.rail_order = vec!["ssh".into()];
        assert_eq!(
            back.rail_prefs(&seed).order,
            vec!["logs".to_string(), "editor".to_string()]
        );
        // write_meta on a workspace that is not open.
        let mut other = back.clone();
        other.rail_prefs_mut(&seed).order = vec!["s3".into()];
        write_meta(&root, &other).unwrap();
        assert_eq!(
            read_meta(&root).unwrap().rail.unwrap().order,
            vec!["s3".to_string()]
        );
    }

    #[test]
    fn any_workspace_but_the_last_can_be_deleted() {
        let dir = std::env::temp_dir().join(format!("hs-ws-del-{}", crate::new_id()));
        let paths = crate::paths::DataDir::resolve(Some(&dir)).unwrap();
        let mut reg = Registry::load(&paths);
        reg.open(&paths, "default").unwrap();
        assert!(
            reg.remove(&paths, "default", true).is_err(),
            "the last one stays"
        );
        reg.open(&paths, "shop").unwrap();
        assert!(reg.remove(&paths, "default", true).is_ok());
        assert!(!reg.exists("default"));
        assert_eq!(reg.startup_name(), "shop");
        assert!(!paths.workspaces_dir().join("default").exists());
        assert!(reg.remove(&paths, "shop", false).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

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
                home: false,
                state: serde_json::json!({"text": "hi"}),
            }],
            active: Some(1),
            sidebar_open: true,
            ..Default::default()
        };
        ws.save_layout(&layout).unwrap();
        let back = ws.load_layout();
        assert_eq!(back.tabs, layout.tabs);
        assert_eq!(back.active, layout.active);
        assert!(ws.root().join(".gitignore").exists());
    }

    /// Split panes were removed on 2026-10-23 and `Layout::split` with them.
    /// Every installed copy has a `layout.json` carrying a `split` tree, and
    /// a parse error there would silently throw the user's tabs away, so the
    /// old key has to be ignored rather than rejected.
    #[test]
    fn a_layout_saved_with_split_panes_still_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = DataDir::resolve(Some(tmp.path())).unwrap();
        let mut reg = Registry::load(&paths);
        let ws = reg.open(&paths, "default").unwrap();
        let json = r#"{
          "schema": 1,
          "tabs": [{"id": 7, "tool": "scratchpad", "pinned": false,
                    "colour": null, "home": true, "state": {"text": "kept"}}],
          "active": 7,
          "split": {"kind": "split", "horizontal": true, "ratio": 0.5,
                    "a": {"kind": "leaf", "tab": 7},
                    "b": {"kind": "leaf", "tab": 8}},
          "sidebar_open": true
        }"#;
        std::fs::write(ws.layout_file(), json).unwrap();

        let back = ws.load_layout();
        assert_eq!(back.tabs.len(), 1, "the tabs survive the dropped field");
        assert_eq!(back.tabs[0].id, 7);
        assert_eq!(back.active, Some(7));
        assert!(back.sidebar_open);
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
