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

// ---- opening folders ------------------------------------------------------

/// What opened a folder for [`open_folder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderOpener {
    /// Eden Explorer at this path; a running instance opens it as a new tab.
    EdenExplorer(PathBuf),
    /// The system file manager (Explorer, Finder, xdg-open).
    System,
}

pub const EDEN_EXE: &str = "EdenExplorer.exe";

/// Where to look for Eden Explorer besides a running instance and `PATH`:
/// next to HyperStack (both portable tools in one folder or side by side)
/// and the usual per-user and machine-wide program folders.
#[cfg(any(windows, test))]
fn eden_candidates(exe_dir: Option<&Path>, env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = exe_dir {
        out.push(d.join(EDEN_EXE));
        out.push(d.join("EdenExplorer").join(EDEN_EXE));
        if let Some(parent) = d.parent() {
            out.push(parent.join("EdenExplorer").join(EDEN_EXE));
            out.push(parent.join(EDEN_EXE));
        }
    }
    if let Some(local) = env("LOCALAPPDATA") {
        out.push(
            Path::new(&local)
                .join("Programs")
                .join("EdenExplorer")
                .join(EDEN_EXE),
        );
        out.push(Path::new(&local).join("EdenExplorer").join(EDEN_EXE));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(pf) = env(var) {
            out.push(Path::new(&pf).join("EdenExplorer").join(EDEN_EXE));
        }
    }
    if let Some(sys) = env("SystemDrive") {
        out.push(
            Path::new(&format!("{sys}\\"))
                .join("Apps")
                .join("EdenExplorer")
                .join(EDEN_EXE),
        );
    }
    out
}

/// Eden Explorer's executable on this machine, if there is one: a running
/// instance first (asked through PowerShell, so this blocks for a moment —
/// call it off the UI thread), then `PATH`, then [`eden_candidates`].
/// Eden Explorer is Windows-only; elsewhere this is always `None`.
pub fn find_eden_explorer() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let running = std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-Process -Name EdenExplorer -ErrorAction SilentlyContinue | Select-Object -First 1).Path",
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_file());
        if running.is_some() {
            return running;
        }
        let on_path = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|d| d.join(EDEN_EXE))
                .find(|p| p.is_file())
        });
        if on_path.is_some() {
            return on_path;
        }
        eden_candidates(exe_dir().as_deref(), &|k| std::env::var(k).ok())
            .into_iter()
            .find(|p| p.is_file())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Open `path` in Eden Explorer when it is installed (as a new tab of the
/// running window: Eden is single-instance and forwards the folder), else in
/// the system file manager. Blocking (see [`find_eden_explorer`]).
pub fn open_folder(path: &Path) -> std::io::Result<FolderOpener> {
    if let Some(eden) = find_eden_explorer() {
        // `--` so a folder name starting with `-` is never read as an option.
        if std::process::Command::new(&eden)
            .arg("--")
            .arg(path)
            .spawn()
            .is_ok()
        {
            return Ok(FolderOpener::EdenExplorer(eden));
        }
    }
    #[cfg(windows)]
    std::process::Command::new("explorer").arg(path).spawn()?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(path).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    std::process::Command::new("xdg-open").arg(path).spawn()?;
    Ok(FolderOpener::System)
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
    fn eden_is_looked_for_beside_hyperstack_and_in_program_folders() {
        let env = |k: &str| match k {
            "LOCALAPPDATA" => Some("L".to_string()),
            "ProgramFiles" => Some("P".to_string()),
            _ => None,
        };
        let c = eden_candidates(Some(Path::new("D/Tools/HyperStack")), &env);
        assert_eq!(c[0], Path::new("D/Tools/HyperStack").join(EDEN_EXE));
        assert!(c.contains(&Path::new("D/Tools/EdenExplorer").join(EDEN_EXE)));
        assert!(c.contains(&Path::new("L/Programs/EdenExplorer").join(EDEN_EXE)));
        assert!(c.contains(&Path::new("P/EdenExplorer").join(EDEN_EXE)));
        assert!(eden_candidates(None, &|_| None).is_empty());
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
