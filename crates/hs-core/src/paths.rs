//! The portable data folder (ADR-017, docs/05 §1).
//!
//! Resolution order: explicit `--data-dir` → `HYPERSTACK_DATA` → `HyperStackData/`
//! beside the executable when that location is writable → per-user fallback.
//! Everything the app writes lives under the resolved directory; nothing else
//! computes paths.

use std::path::{Path, PathBuf};

pub const DATA_DIR_NAME: &str = "HyperStackData";
pub const ENV_VAR: &str = "HYPERSTACK_DATA";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    CommandLine,
    EnvVar,
    BesideExecutable,
    PerUserFallback,
}

#[derive(Debug, Clone)]
pub struct DataDir {
    root: PathBuf,
    pub source: Source,
    /// Set when the executable's folder was not writable; shown once in the UI.
    pub fallback_reason: Option<String>,
}

impl DataDir {
    /// Resolve the data directory and make sure the standard subfolders exist.
    pub fn resolve(explicit: Option<&Path>) -> std::io::Result<Self> {
        let (root, source, reason) = Self::pick(explicit);
        let dir = Self {
            root,
            source,
            fallback_reason: reason,
        };
        dir.ensure_layout()?;
        Ok(dir)
    }

    fn pick(explicit: Option<&Path>) -> (PathBuf, Source, Option<String>) {
        if let Some(p) = explicit {
            return (p.to_path_buf(), Source::CommandLine, None);
        }
        if let Some(p) = std::env::var_os(ENV_VAR).filter(|v| !v.is_empty()) {
            return (PathBuf::from(p), Source::EnvVar, None);
        }
        match exe_dir() {
            Some(exe_dir) => {
                let candidate = exe_dir.join(DATA_DIR_NAME);
                match probe_writable(&exe_dir, &candidate) {
                    Ok(()) => (candidate, Source::BesideExecutable, None),
                    Err(e) => (
                        per_user_dir(),
                        Source::PerUserFallback,
                        Some(format!(
                            "{} is not writable ({e}); using the per-user folder instead",
                            exe_dir.display()
                        )),
                    ),
                }
            }
            None => (
                per_user_dir(),
                Source::PerUserFallback,
                Some("could not determine the executable's folder".to_string()),
            ),
        }
    }

    fn ensure_layout(&self) -> std::io::Result<()> {
        for sub in [
            "workspaces",
            "themes",
            "cache",
            "tmp",
            "logs",
            "update",
            "keys",
        ] {
            std::fs::create_dir_all(self.root.join(sub))?;
        }
        // tmp/ is scratch space and is cleared on start.
        if let Ok(entries) = std::fs::read_dir(self.tmp()) {
            for e in entries.flatten() {
                let p = e.path();
                let _ = if p.is_dir() {
                    std::fs::remove_dir_all(&p)
                } else {
                    std::fs::remove_file(&p)
                };
            }
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.toml")
    }
    pub fn shortcuts_file(&self) -> PathBuf {
        self.root.join("shortcuts.toml")
    }
    pub fn vault_file(&self) -> PathBuf {
        self.root.join("vault.hsv")
    }
    pub fn known_hosts_file(&self) -> PathBuf {
        self.root.join("known_hosts")
    }
    pub fn workspaces_registry(&self) -> PathBuf {
        self.root.join("workspaces.toml")
    }
    pub fn workspaces_dir(&self) -> PathBuf {
        self.root.join("workspaces")
    }
    pub fn history_db(&self) -> PathBuf {
        self.root.join("history.sqlite")
    }
    pub fn themes_dir(&self) -> PathBuf {
        self.root.join("themes")
    }
    pub fn cache_dir(&self) -> PathBuf {
        self.root.join("cache")
    }
    pub fn tmp(&self) -> PathBuf {
        self.root.join("tmp")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn update_dir(&self) -> PathBuf {
        self.root.join("update")
    }
    pub fn instance_lock(&self) -> PathBuf {
        self.root.join("instance.lock")
    }

    /// Store a path relative to the data folder when it lies inside it, so the
    /// folder can be moved. Paths outside are kept as given.
    pub fn relativize(&self, path: &Path) -> PathBuf {
        path.strip_prefix(&self.root)
            .map(Path::to_path_buf)
            .unwrap_or_else(|_| path.to_path_buf())
    }

    /// [`relativize`](Self::relativize) as the string written to config files:
    /// forward slashes for paths inside the data folder, so the same file works
    /// on every platform.
    pub fn relativize_str(&self, path: &Path) -> String {
        match path.strip_prefix(&self.root) {
            Ok(rel) => rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            Err(_) => path.to_string_lossy().to_string(),
        }
    }

    /// Inverse of [`relativize`](Self::relativize); also expands `${DATA_DIR}`
    /// and `${EXE_DIR}` tokens.
    pub fn resolve_path(&self, stored: &str) -> PathBuf {
        let exe = exe_dir().unwrap_or_else(|| self.root.clone());
        let expanded = stored
            .replace("${DATA_DIR}", &self.root.to_string_lossy())
            .replace("${EXE_DIR}", &exe.to_string_lossy());
        let p = PathBuf::from(expanded);
        if p.is_absolute() {
            p
        } else {
            self.root.join(p)
        }
    }
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// Can we create our folder here and write into it? Probe with a real file.
fn probe_writable(parent: &Path, candidate: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(candidate)?;
    let probe = candidate.join(format!(".write-probe-{}", std::process::id()));
    std::fs::write(&probe, b"ok")?;
    std::fs::remove_file(&probe)?;
    let _ = parent;
    Ok(())
}

fn per_user_dir() -> PathBuf {
    let base = dirs::data_local_dir()
        .or_else(dirs::data_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir);
    base.join("HyperStack")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_dir_wins_and_layout_is_created() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDir::resolve(Some(tmp.path())).unwrap();
        assert_eq!(d.source, Source::CommandLine);
        assert!(d.workspaces_dir().is_dir());
        assert!(d.logs_dir().is_dir());
        assert!(d.tmp().is_dir());
    }

    #[test]
    fn relativize_and_resolve_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDir::resolve(Some(tmp.path())).unwrap();
        let inside = d.root().join("keys").join("id_ed25519");
        let rel = d.relativize(&inside);
        assert_eq!(rel, Path::new("keys").join("id_ed25519"));
        assert_eq!(d.resolve_path(&rel.to_string_lossy()), inside);
        let token = d.resolve_path("${DATA_DIR}/keys/x");
        assert!(token.starts_with(d.root()));
        let outside = Path::new(if cfg!(windows) {
            "C:\\elsewhere\\f"
        } else {
            "/elsewhere/f"
        });
        assert_eq!(d.relativize(outside), outside);
    }

    #[test]
    fn tmp_is_cleared_on_resolve() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDir::resolve(Some(tmp.path())).unwrap();
        std::fs::write(d.tmp().join("junk"), b"x").unwrap();
        let d2 = DataDir::resolve(Some(tmp.path())).unwrap();
        assert!(!d2.tmp().join("junk").exists());
    }
}
