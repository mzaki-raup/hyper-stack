//! Environment export (docs/05 §2.3): Postman v2.1, Insomnia v4, Bruno and a
//! flat name/value JSON — the formats [`crate::postman::import_environment_document`]
//! already reads.
//!
//! Secrets are never written. A row flagged secret, or one holding a
//! `secret:` reference, exports as an empty value so a file handed to
//! another client cannot leak the vault; Postman keeps `"type": "secret"`
//! so the entry stays marked there too.

use hs_core::secrets::SecretRef;
use serde_json::{Map, Value, json};

/// The file shapes the Environments page can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Postman,
    Insomnia,
    Bruno,
    Flat,
}

impl Format {
    pub const ALL: [Format; 4] = [
        Format::Postman,
        Format::Insomnia,
        Format::Bruno,
        Format::Flat,
    ];

    /// Stable id for the file name and the effect that carries it.
    pub fn id(self) -> &'static str {
        match self {
            Format::Postman => "postman",
            Format::Insomnia => "insomnia",
            Format::Bruno => "bruno",
            Format::Flat => "flat",
        }
    }

    /// A human name; not translated because these are product names.
    pub fn label(self) -> &'static str {
        match self {
            Format::Postman => "Postman",
            Format::Insomnia => "Insomnia",
            Format::Bruno => "Bruno",
            Format::Flat => "Flat JSON",
        }
    }
}

/// One variable as the page knows it. `secret` is the row's toggle: a
/// typed secret that has not reached the vault yet is still a secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub key: String,
    pub value: String,
    pub secret: bool,
}

impl Row {
    fn is_secret(&self) -> bool {
        self.secret || SecretRef::parse(&self.value).is_some()
    }

    /// What may leave the app: an empty string for a secret.
    fn export_value(&self) -> &str {
        if self.is_secret() { "" } else { &self.value }
    }
}

/// Render `name` and `rows` in `format`. The result is pretty JSON, because
/// these files are read by people as well as by clients.
pub fn render(name: &str, rows: &[Row], format: Format) -> String {
    let value = match format {
        Format::Postman => json!({
            "name": name,
            "values": rows
                .iter()
                .map(|row| json!({
                    "key": row.key,
                    "value": row.export_value(),
                    "type": if row.is_secret() { "secret" } else { "default" },
                    "enabled": true,
                }))
                .collect::<Vec<_>>(),
            "_postman_variable_scope": "environment",
            "_postman_exported_using": "HyperStack",
        }),
        Format::Insomnia => {
            let mut data = Map::new();
            for row in rows {
                data.insert(row.key.clone(), Value::String(row.export_value().into()));
            }
            json!({ "_type": "environment", "name": name, "data": data })
        }
        Format::Bruno => json!({
            "name": name,
            "vars": rows
                .iter()
                .map(|row| json!({
                    "name": row.key,
                    "value": row.export_value(),
                    "enabled": true,
                    "secret": row.is_secret(),
                }))
                .collect::<Vec<_>>(),
        }),
        Format::Flat => {
            let mut values = Map::new();
            for row in rows {
                values.insert(row.key.clone(), Value::String(row.export_value().into()));
            }
            json!({ "name": name, "values": values })
        }
    };
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())
}

/// How many rows a file will leave empty because they are secret.
pub fn omitted_secrets(rows: &[Row]) -> usize {
    rows.iter().filter(|row| row.is_secret()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Row> {
        vec![
            Row {
                key: "host".into(),
                value: "db.example".into(),
                secret: false,
            },
            Row {
                key: "db_password".into(),
                value: "secret:01HQ".into(),
                secret: false,
            },
            Row {
                key: "api_token".into(),
                value: "typed-but-unsaved".into(),
                secret: true,
            },
        ]
    }

    #[test]
    fn every_format_keeps_secrets_out() {
        let rows = rows();
        for format in Format::ALL {
            let text = render("staging", &rows, format);
            assert!(
                !text.contains("01HQ") && !text.contains("typed-but-unsaved"),
                "{} leaked a secret: {text}",
                format.id()
            );
            assert!(
                text.contains("db.example"),
                "{} lost a plain value",
                format.id()
            );
            let value: Value = serde_json::from_str(&text).expect("valid JSON");
            match format {
                Format::Postman => {
                    let values = value["values"].as_array().unwrap();
                    assert_eq!(values[0]["value"], "db.example");
                    assert_eq!(values[1]["type"], "secret");
                    assert_eq!(values[1]["value"], "");
                    assert_eq!(values[2]["value"], "");
                }
                Format::Insomnia => {
                    assert_eq!(value["_type"], "environment");
                    assert_eq!(value["data"]["host"], "db.example");
                    assert_eq!(value["data"]["db_password"], "");
                }
                Format::Bruno => {
                    let vars = value["vars"].as_array().unwrap();
                    assert_eq!(vars[0]["name"], "host");
                    assert_eq!(vars[1]["secret"], true);
                    assert_eq!(vars[1]["value"], "");
                }
                Format::Flat => {
                    assert_eq!(value["values"]["host"], "db.example");
                    assert_eq!(value["values"]["api_token"], "");
                    assert_eq!(value["name"], "staging");
                }
            }
        }
        assert_eq!(omitted_secrets(&rows), 2);
    }

    #[test]
    fn imports_of_our_own_exports_read_back() {
        // The Postman writer and the importer are two sides of one contract.
        let text = render(
            "dev",
            &[Row {
                key: "base".into(),
                value: "http://localhost".into(),
                secret: false,
            }],
            Format::Postman,
        );
        let (name, vars) = crate::postman::import_environment_document(&text).unwrap();
        assert_eq!(name, "dev");
        assert_eq!(vars[0].key, "base");
        assert_eq!(vars[0].value, "http://localhost");
    }
}
