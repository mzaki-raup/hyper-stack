//! Documents as a table (docs/tools/nosql.md §7): every document flattened to
//! dotted paths so a page of them lines up in columns, with arrays shown as
//! their length rather than expanded into noise.

/// A cell's text, and whether the field was missing rather than null.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub text: String,
    pub missing: bool,
}

/// Flatten one document to `path -> cell`.
pub fn flatten(value: &serde_json::Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk("", value, &mut out);
    out
}

fn walk(prefix: &str, value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            // Extended JSON wrappers are values, not nesting: `{"$oid": …}`
            // is one cell, not a sub-document.
            if map.len() == 1
                && let Some((k, v)) = map.iter().next()
                && k.starts_with('$')
            {
                out.push((prefix.to_string(), scalar(v)));
                return;
            }
            if map.is_empty() {
                out.push((prefix.to_string(), "{}".into()));
                return;
            }
            for (k, v) in map {
                let path = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                walk(&path, v, out);
            }
        }
        serde_json::Value::Array(a) => {
            out.push((
                prefix.to_string(),
                if a.len() == 1 {
                    "[1 item]".to_string()
                } else {
                    format!("[{} items]", a.len())
                },
            ));
        }
        other => out.push((prefix.to_string(), scalar(other))),
    }
}

fn scalar(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "null".into(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The columns a page of documents needs: `_id` first, then every other path
/// in the order it was first seen, so the table is stable as you page.
pub fn columns(docs: &[serde_json::Value]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for d in docs {
        for (path, _) in flatten(d) {
            if !out.contains(&path) {
                out.push(path);
            }
        }
    }
    if let Some(i) = out.iter().position(|c| c == "_id")
        && i > 0
    {
        let id = out.remove(i);
        out.insert(0, id);
    }
    out
}

/// One row per document, in the column order, with missing fields marked.
pub fn rows(docs: &[serde_json::Value], columns: &[String]) -> Vec<Vec<Cell>> {
    docs.iter()
        .map(|d| {
            let flat = flatten(d);
            columns
                .iter()
                .map(|c| match flat.iter().find(|(p, _)| p == c) {
                    Some((_, v)) => Cell {
                        text: v.clone(),
                        missing: false,
                    },
                    None => Cell {
                        text: String::new(),
                        missing: true,
                    },
                })
                .collect()
        })
        .collect()
}

/// Lay a document out for the JSON view. Fields come out in name order:
/// `serde_json::Value` is a sorted map, so the order the store keeps them in
/// is already gone by the time a document reaches here.
pub fn pretty(value: &serde_json::Value) -> String {
    let compact = value.to_string();
    hs_core::fmt::pretty_json(&compact).unwrap_or(compact)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_fields_become_dotted_paths() {
        let d = serde_json::json!({
            "_id": "a",
            "customer": {"email": "ada@example.com", "vip": true},
            "total": 12.5
        });
        let flat = flatten(&d);
        assert!(flat.contains(&("customer.email".into(), "ada@example.com".into())));
        assert!(flat.contains(&("customer.vip".into(), "true".into())));
        assert!(flat.contains(&("total".into(), "12.5".into())));
    }

    #[test]
    fn an_extended_json_value_is_one_cell() {
        let d = serde_json::json!({"_id": {"$oid": "68e8a1b2c3d4e5f607182930"}});
        let flat = flatten(&d);
        assert_eq!(
            flat,
            vec![("_id".to_string(), "68e8a1b2c3d4e5f607182930".to_string())]
        );
    }

    #[test]
    fn an_array_shows_its_length_rather_than_its_contents() {
        let d = serde_json::json!({"items": [1, 2, 3], "one": ["x"], "none": []});
        let flat = flatten(&d);
        assert!(flat.contains(&("items".into(), "[3 items]".into())));
        assert!(flat.contains(&("one".into(), "[1 item]".into())));
        assert!(flat.contains(&("none".into(), "[0 items]".into())));
    }

    #[test]
    fn the_id_column_comes_first_and_missing_fields_are_marked() {
        let docs = vec![
            serde_json::json!({"total": 1, "_id": "a"}),
            serde_json::json!({"_id": "b", "status": "paid"}),
        ];
        let cols = columns(&docs);
        assert_eq!(cols[0], "_id");
        assert!(cols.contains(&"status".to_string()));
        let rows = rows(&docs, &cols);
        let status = cols.iter().position(|c| c == "status").unwrap();
        assert!(rows[0][status].missing, "the first document has no status");
        assert!(!rows[1][status].missing);
        assert_eq!(rows[1][status].text, "paid");
    }

    #[test]
    fn null_is_not_the_same_as_missing() {
        let docs = vec![serde_json::json!({"_id": "a", "note": null})];
        let cols = columns(&docs);
        let rows = rows(&docs, &cols);
        let note = cols.iter().position(|c| c == "note").unwrap();
        assert_eq!(rows[0][note].text, "null");
        assert!(!rows[0][note].missing);
    }

    #[test]
    fn a_document_lays_out_one_field_per_line() {
        let d = serde_json::json!({"b": 1, "a": 2});
        let text = pretty(&d);
        assert!(text.contains('\n'));
        assert!(text.contains("\"a\": 2"));
        // `serde_json::Value` is a sorted map, so a document's fields are
        // shown in name order, not the order the store keeps them in.
        assert!(text.find("\"a\"").unwrap() < text.find("\"b\"").unwrap());
    }
}
