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
    let mut value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid .hdiag JSON: {e}"))?;
    let schema = value
        .get("schema")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(1) as u32;
    if schema > SCHEMA_VERSION {
        return Err(format!(
            "unsupported .hdiag schema {schema} (this build reads {SCHEMA_VERSION})"
        ));
    }
    // Older documents only ever lacked fields, which every model struct fills
    // with a serde default; bump the marker so the next save writes current.
    value["schema"] = serde_json::json!(SCHEMA_VERSION);
    serde_json::from_value(value).map_err(|e| format!("invalid .hdiag JSON: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DiagramKind;

    #[test]
    fn a_version_one_document_migrates_to_current() {
        let text = r#"{
            "schema": 1,
            "kind": "flow",
            "title": "Old",
            "nodes": [{"id": "a", "label": "A", "kind": "process"}],
            "edges": []
        }"#;
        let doc = from_json(text).expect("v1 loads");
        assert_eq!(doc.schema, SCHEMA_VERSION);
        assert_eq!(doc.kind, DiagramKind::Flow);
        assert_eq!(doc.nodes.len(), 1);
        assert!(to_json(&doc).contains("\"schema\": 2"));
    }

    #[test]
    fn a_newer_schema_is_rejected() {
        let text = format!(r#"{{"schema": {}, "kind": "flow"}}"#, SCHEMA_VERSION + 1);
        assert!(from_json(&text).is_err());
    }
}
