//! The slice of LSP 3.17 the editor uses, typed just enough to be safe.
//! Positions are what the server sent (line and a column in the
//! negotiated encoding, see [`crate::text`]); the editor converts them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: Range,
}

/// `DiagnosticSeverity`: 1 error, 2 warning, 3 information, 4 hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    Information,
    Hint,
}

impl Severity {
    pub fn from_lsp(n: Option<u8>) -> Self {
        match n {
            Some(2) => Severity::Warning,
            Some(3) => Severity::Information,
            Some(4) => Severity::Hint,
            _ => Severity::Error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub range: Range,
    #[serde(default)]
    pub severity: Option<u8>,
    #[serde(default)]
    pub code: Option<Value>,
    #[serde(default)]
    pub source: Option<String>,
    pub message: String,
}

impl Diagnostic {
    pub fn severity(&self) -> Severity {
        Severity::from_lsp(self.severity)
    }

    /// The code as text (`E0425`, `unused-variable`), if any.
    pub fn code_text(&self) -> Option<String> {
        match &self.code {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    pub range: Range,
    pub new_text: String,
}

/// `CompletionItemKind` names, indexed by the protocol's 1-based value.
pub const COMPLETION_KINDS: [&str; 25] = [
    "text",
    "method",
    "function",
    "constructor",
    "field",
    "variable",
    "class",
    "interface",
    "module",
    "property",
    "unit",
    "value",
    "enum",
    "keyword",
    "snippet",
    "color",
    "file",
    "reference",
    "folder",
    "enum member",
    "constant",
    "struct",
    "event",
    "operator",
    "type parameter",
];

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    pub label: String,
    #[serde(default)]
    pub kind: Option<u8>,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub documentation: Option<Value>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    #[serde(default)]
    pub preselect: Option<bool>,
    #[serde(default)]
    pub sort_text: Option<String>,
    #[serde(default)]
    pub filter_text: Option<String>,
    #[serde(default)]
    pub insert_text: Option<String>,
    /// 1 plain text, 2 snippet.
    #[serde(default)]
    pub insert_text_format: Option<u8>,
    /// A `TextEdit` or an `InsertReplaceEdit` (`newText`, `insert`, `replace`).
    #[serde(default)]
    pub text_edit: Option<Value>,
    #[serde(default)]
    pub additional_text_edits: Vec<TextEdit>,
    #[serde(default)]
    pub commit_characters: Vec<String>,
    /// Servers may return a bare item and resolve the rest on demand.
    #[serde(default)]
    pub data: Option<Value>,
}

impl CompletionItem {
    pub fn kind_name(&self) -> &'static str {
        self.kind
            .and_then(|k| COMPLETION_KINDS.get(usize::from(k).saturating_sub(1)))
            .copied()
            .unwrap_or("")
    }

    pub fn is_snippet(&self) -> bool {
        self.insert_text_format == Some(2)
    }

    /// What typing filters against.
    pub fn filter_key(&self) -> &str {
        self.filter_text.as_deref().unwrap_or(&self.label)
    }

    /// The text edit to apply, when the server gave one: its range (the
    /// `replace` range of an insert/replace edit) and the new text.
    pub fn edit(&self) -> Option<TextEdit> {
        let v = self.text_edit.as_ref()?;
        if let Ok(e) = serde_json::from_value::<TextEdit>(v.clone()) {
            return Some(e);
        }
        let new_text = v.get("newText")?.as_str()?.to_string();
        let range = v
            .get("replace")
            .or_else(|| v.get("insert"))
            .and_then(|r| serde_json::from_value::<Range>(r.clone()).ok())?;
        Some(TextEdit { range, new_text })
    }

    /// The text to insert when there is no text edit.
    pub fn insert_text(&self) -> &str {
        self.insert_text.as_deref().unwrap_or(&self.label)
    }

    pub fn documentation_text(&self) -> Option<String> {
        self.documentation.as_ref().map(markup_text)
    }
}

/// A completion response: a list or an array of items.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum CompletionResponse {
    List {
        #[serde(rename = "isIncomplete", default)]
        is_incomplete: bool,
        items: Vec<CompletionItem>,
    },
    Items(Vec<CompletionItem>),
}

impl CompletionResponse {
    pub fn into_items(self) -> (Vec<CompletionItem>, bool) {
        match self {
            CompletionResponse::List {
                is_incomplete,
                items,
            } => (items, is_incomplete),
            CompletionResponse::Items(items) => (items, false),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Hover {
    pub contents: Value,
    #[serde(default)]
    pub range: Option<Range>,
}

impl Hover {
    /// The hover as Markdown-ish text: `MarkupContent`, a `MarkedString`
    /// or an array of them, fenced code for the `{language, value}` form.
    pub fn text(&self) -> String {
        marked_text(&self.contents)
    }
}

/// `MarkupContent` or a plain string to text.
pub fn markup_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Object(o) => o
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn marked_text(v: &Value) -> String {
    match v {
        Value::Array(items) => items
            .iter()
            .map(marked_text)
            .filter(|s| !s.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        Value::Object(o) if o.contains_key("language") => {
            let lang = o.get("language").and_then(Value::as_str).unwrap_or("");
            let value = o.get("value").and_then(Value::as_str).unwrap_or("");
            format!("```{lang}\n{value}\n```")
        }
        other => markup_text(other),
    }
}

/// `Location | Location[] | LocationLink[]` to locations.
pub fn locations(v: &Value) -> Vec<Location> {
    let one = |v: &Value| -> Option<Location> {
        if let Ok(l) = serde_json::from_value::<Location>(v.clone()) {
            return Some(l);
        }
        let uri = v.get("targetUri")?.as_str()?.to_string();
        let range = v
            .get("targetSelectionRange")
            .or_else(|| v.get("targetRange"))
            .and_then(|r| serde_json::from_value::<Range>(r.clone()).ok())?;
        Some(Location { uri, range })
    };
    match v {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Null => Vec::new(),
        other => one(other).into_iter().collect(),
    }
}

/// A `WorkspaceEdit` flattened to edits per document URI (`changes` and
/// `documentChanges` with text document edits; file operations are ignored).
pub fn workspace_edits(v: &Value) -> Vec<(String, Vec<TextEdit>)> {
    let mut out: Vec<(String, Vec<TextEdit>)> = Vec::new();
    if let Some(Value::Object(changes)) = v.get("changes") {
        for (uri, edits) in changes {
            let edits: Vec<TextEdit> = serde_json::from_value(edits.clone()).unwrap_or_default();
            out.push((uri.clone(), edits));
        }
    }
    if let Some(Value::Array(docs)) = v.get("documentChanges") {
        for d in docs {
            let Some(uri) = d
                .get("textDocument")
                .and_then(|t| t.get("uri"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            let edits: Vec<TextEdit> = d
                .get("edits")
                .and_then(|e| serde_json::from_value(e.clone()).ok())
                .unwrap_or_default();
            match out.iter_mut().find(|(u, _)| u == uri) {
                Some((_, list)) => list.extend(edits),
                None => out.push((uri.to_string(), edits)),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn completion_edit_accepts_both_edit_shapes() {
        let plain: CompletionItem = serde_json::from_value(json!({
            "label": "foo",
            "textEdit": {"range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 3}}, "newText": "foo"}
        }))
        .unwrap();
        assert_eq!(plain.edit().unwrap().range.end.character, 3);
        let ir: CompletionItem = serde_json::from_value(json!({
            "label": "bar",
            "kind": 3,
            "textEdit": {"newText": "bar()", "insert": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 2}},
                         "replace": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 5}}}
        }))
        .unwrap();
        let e = ir.edit().unwrap();
        assert_eq!((e.range.end.character, e.new_text.as_str()), (5, "bar()"));
        assert_eq!(ir.kind_name(), "function");
    }

    #[test]
    fn hover_text_joins_marked_strings() {
        let h: Hover = serde_json::from_value(json!({
            "contents": [{"language": "rust", "value": "fn main()"}, "Entry point"]
        }))
        .unwrap();
        assert_eq!(h.text(), "```rust\nfn main()\n```\n\nEntry point");
        let m: Hover = serde_json::from_value(json!({
            "contents": {"kind": "markdown", "value": "**bold**"}
        }))
        .unwrap();
        assert_eq!(m.text(), "**bold**");
    }

    #[test]
    fn definition_results_come_in_three_shapes() {
        let loc = json!({"uri": "file:///a.rs", "range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 3}}});
        assert_eq!(locations(&loc).len(), 1);
        assert_eq!(locations(&json!([loc.clone(), loc.clone()])).len(), 2);
        let link = json!([{"targetUri": "file:///b.rs",
            "targetRange": {"start": {"line": 0, "character": 0}, "end": {"line": 9, "character": 0}},
            "targetSelectionRange": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 8}}}]);
        let l = locations(&link);
        assert_eq!(l[0].range.start.line, 2);
        assert!(locations(&Value::Null).is_empty());
    }

    #[test]
    fn workspace_edit_flattens_both_forms() {
        let v = json!({
            "changes": {"file:///a.rs": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "x"}]},
            "documentChanges": [{"textDocument": {"uri": "file:///a.rs", "version": 3},
                "edits": [{"range": {"start": {"line": 1, "character": 0}, "end": {"line": 1, "character": 0}}, "newText": "y"}]}]
        });
        let e = workspace_edits(&v);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].1.len(), 2);
    }
}
