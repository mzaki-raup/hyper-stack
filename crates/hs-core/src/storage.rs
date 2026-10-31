//! Atomic file IO and TOML/JSON helpers. Every write goes to a temp file in
//! the same directory, is flushed, then renamed over the target, so a crash or
//! a yanked USB stick leaves either the old or the new file, never half of one.

use std::path::Path;

use serde::{Serialize, de::DeserializeOwned};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("{0}")]
    Message(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("could not parse {path}: {message}")]
    Parse { path: String, message: String },
    #[error("could not serialise {path}: {message}")]
    Serialize { path: String, message: String },
    #[error(
        "{path} was written by a newer HyperStack (schema {found}, this build reads {supported})"
    )]
    NewerSchema {
        path: String,
        found: u32,
        supported: u32,
    },
}

pub type Result<T> = std::result::Result<T, StorageError>;

fn io(path: &Path, source: std::io::Error) -> StorageError {
    StorageError::Io {
        path: path.display().to_string(),
        source,
    }
}

/// Write bytes atomically (temp file + fsync + rename).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io(path, std::io::Error::other("path has no parent directory")))?;
    std::fs::create_dir_all(dir).map_err(|e| io(path, e))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        std::process::id()
    ));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&tmp).map_err(|e| io(&tmp, e))?;
        f.write_all(bytes).map_err(|e| io(&tmp, e))?;
        f.sync_all().map_err(|e| io(&tmp, e))?;
    }
    // On Windows, rename over an existing file needs the target removed first
    // for some filesystems; `rename` handles it on NTFS, but be explicit.
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| io(path, e))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| io(path, e))?;
    Ok(())
}

pub fn read_toml<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    toml::from_str(&text).map_err(|e| StorageError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

pub fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = toml::to_string_pretty(value).map_err(|e| StorageError::Serialize {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    write_atomic(path, text.as_bytes())
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    serde_json::from_str(&text).map_err(|e| StorageError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(value).map_err(|e| StorageError::Serialize {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    write_atomic(path, text.as_bytes())
}

/// Read a TOML file that carries a top-level `schema = N`, refusing files from
/// a newer build instead of silently misreading them (docs/05 §5).
pub fn read_toml_versioned<T: DeserializeOwned>(path: &Path, supported: u32) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|e| io(path, e))?;
    let probe: toml::Value = toml::from_str(&text).map_err(|e| StorageError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let found = probe
        .get("schema")
        .and_then(|v| v.as_integer())
        .unwrap_or(1) as u32;
    if found > supported {
        return Err(StorageError::NewerSchema {
            path: path.display().to_string(),
            found,
            supported,
        });
    }
    toml::from_str(&text).map_err(|e| StorageError::Parse {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

/// A file-system-safe name derived from a human name (`orders — staging` → `orders-staging`).
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-').to_string();
    if trimmed.is_empty() {
        "item".to_string()
    } else {
        trimmed
    }
}

/// A name not already in `taken`: `wanted` itself, then `wanted 001`,
/// `wanted 002`, … - the suffix the rest of the app gives a clash.
///
/// Compared without regard to case: two names that differ only in case are
/// the same file or folder on Windows and macOS, so treating them as
/// different would hand back a name that collides on disk.
pub fn unique_name<'a>(wanted: &str, taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: Vec<String> = taken.into_iter().map(str::to_lowercase).collect();
    let free = |name: &str| !taken.contains(&name.to_lowercase());
    let wanted = wanted.trim();
    if free(wanted) {
        return wanted.to_string();
    }
    for n in 1..=999 {
        let candidate = format!("{wanted} {n:03}");
        if free(&candidate) {
            return candidate;
        }
    }
    // Past 999 the sequence is no help; fall back to something unique.
    format!("{wanted} {}", crate::new_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_free_name_is_kept_and_a_clash_is_numbered() {
        assert_eq!(unique_name("Work", ["Home"]), "Work");
        assert_eq!(unique_name("Work", ["Work"]), "Work 001");
        assert_eq!(unique_name("Work", ["Work", "Work 001"]), "Work 002");
        assert_eq!(unique_name("  Work ", ["Home"]), "Work", "trimmed");
    }

    #[test]
    fn a_name_differing_only_in_case_is_a_clash() {
        // `work` and `Work` are one folder on Windows and macOS.
        assert_eq!(unique_name("work", ["Work"]), "work 001");
        assert_eq!(unique_name("Work", ["WORK", "work 001"]), "Work 002");
    }
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Doc {
        schema: u32,
        name: String,
    }

    #[test]
    fn atomic_write_replaces_existing_file_and_leaves_no_temp() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("a.toml");
        write_toml(
            &p,
            &Doc {
                schema: 1,
                name: "one".into(),
            },
        )
        .unwrap();
        write_toml(
            &p,
            &Doc {
                schema: 1,
                name: "two".into(),
            },
        )
        .unwrap();
        let d: Doc = read_toml(&p).unwrap();
        assert_eq!(d.name, "two");
        let leftovers: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn newer_schema_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("b.toml");
        write_toml(
            &p,
            &Doc {
                schema: 9,
                name: "future".into(),
            },
        )
        .unwrap();
        let err = read_toml_versioned::<Doc>(&p, 1).unwrap_err();
        assert!(matches!(
            err,
            StorageError::NewerSchema {
                found: 9,
                supported: 1,
                ..
            }
        ));
        assert!(read_toml_versioned::<Doc>(&p, 9).is_ok());
    }

    #[test]
    fn slugs_are_safe_and_stable() {
        assert_eq!(slug("orders — staging"), "orders-staging");
        assert_eq!(slug("  Web 01 / prod  "), "web-01-prod");
        assert_eq!(slug("***"), "item");
    }
}
