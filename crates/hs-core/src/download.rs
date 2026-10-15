//! "Download" for anything a tool produces: a save dialog, then the bytes
//! or a copy of a file land where the user chose. Folders go out as a
//! ZIP written by the caller (`tool-editor::zip_dir`); this module only
//! covers single files so every tool saves the same way.

use std::path::{Path, PathBuf};

/// Ask where to save `suggested_name`, then write `bytes` there. Returns
/// the chosen path, `None` when the dialog was cancelled.
pub fn save_bytes(suggested_name: &str, bytes: &[u8]) -> Result<Option<PathBuf>, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_file_name(safe_name(suggested_name))
        .save_file()
    else {
        return Ok(None);
    };
    std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path))
}

/// Ask where to save a copy of `src` (its own file name suggested).
pub fn save_copy(src: &Path) -> Result<Option<PathBuf>, String> {
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let Some(path) = rfd::FileDialog::new().set_file_name(name).save_file() else {
        return Ok(None);
    };
    std::fs::copy(src, &path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path))
}

/// A file name a dialog will accept: path separators and control
/// characters replaced, empty names given a default.
pub fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if cleaned.is_empty() {
        "download".into()
    } else {
        cleaned.chars().take(120).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;

    #[test]
    fn names_are_made_safe() {
        assert_eq!(safe_name("a/b:c?.json"), "a_b_c_.json");
        assert_eq!(safe_name("   "), "download");
        assert_eq!(safe_name("note.md"), "note.md");
    }
}
