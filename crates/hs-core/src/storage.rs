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

#[cfg(test)]
mod tests {
    use super::*;
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
