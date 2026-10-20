//! `.hdiag` I/O: pretty JSON, a size cap and an atomic replace.

use crate::model::{DiagramDoc, SCHEMA_VERSION};
use crate::validate::{Issue, MAX_FILE_BYTES, validate};
use std::path::Path;

pub fn to_json(doc: &DiagramDoc) -> String {
    let mut text = serde_json::to_string_pretty(doc).unwrap_or_else(|_| "{}".into());
    text.push('\n');
    text
}

pub fn from_json(text: &str) -> Result<DiagramDoc, String> {
    let doc: DiagramDoc =
        serde_json::from_str(text).map_err(|e| format!("invalid .hdiag JSON: {e}"))?;
    if doc.schema != SCHEMA_VERSION {
        return Err(format!(
            "unsupported .hdiag schema {} (this build reads {SCHEMA_VERSION})",
            doc.schema
        ));
    }
    Ok(doc)
}

/// Load a document and return its issues too; lenient `.hdiag` parsing keeps
/// warnings, but a malformed file is an error.
pub fn load(path: &Path) -> Result<(DiagramDoc, Vec<Issue>), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.len() as usize > MAX_FILE_BYTES {
        return Err(format!(
            "the file is larger than {} MB",
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    let doc = from_json(&text)?;
    let issues = validate(&doc);
    Ok((doc, issues))
}

pub fn save(path: &Path, doc: &DiagramDoc) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = to_json(doc);
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}
