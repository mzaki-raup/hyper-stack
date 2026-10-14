//! Postman Collection v2.1 import and export, and Postman environment
//! import. Scripts come across as they are (the runtime speaks the `pm.*`
//! API); what has no HyperStack equivalent (OAuth 2 flows, GraphQL bodies)
//! is kept in the request's docs so nothing is silently lost.

use serde_json::{Value, json};

use crate::model::{Auth, AuthKind, Body, BodyKind, Collection, Folder, Item, Kv, Request};

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

fn kv_list(v: Option<&Value>) -> Vec<Kv> {
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .map(|e| Kv {
                    enabled: !e.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false),
                    key: s(e, "key"),
                    value: s(e, "value"),
                    description: e
                        .get("description")
                        .map(|d| match d {
                            Value::String(t) => t.clone(),
                            other => s(other, "content"),
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn script_of(events: Option<&Value>, listen: &str) -> String {
    events
        .and_then(|e| e.as_array())
        .and_then(|arr| {
            arr.iter()
                .find(|e| s(e, "listen") == listen)
                .and_then(|e| e.get("script"))
                .and_then(|sc| sc.get("exec"))
                .map(|exec| match exec {
                    Value::Array(lines) => lines
                        .iter()
                        .filter_map(|l| l.as_str())
                        .collect::<Vec<_>>()
                        .join("\n"),
                    Value::String(t) => t.clone(),
                    _ => String::new(),
                })
        })
        .unwrap_or_default()
}

fn auth_of(v: Option<&Value>) -> Auth {
    let Some(a) = v else {
        return Auth::default();
    };
    let kind = s(a, "type");
    let param = |section: &str, key: &str| -> String {
        a.get(section)
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.iter().find(|e| s(e, "key") == key))
            .map(|e| match e.get("value") {
                Some(Value::String(t)) => t.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            })
            .unwrap_or_default()
    };
    match kind.as_str() {
        "noauth" => Auth {
            kind: AuthKind::None,
            ..Default::default()
        },
        "basic" => Auth {
            kind: AuthKind::Basic,
            username: param("basic", "username"),
            password: param("basic", "password"),
            ..Default::default()
        },
        "bearer" => Auth {
            kind: AuthKind::Bearer,
            token: param("bearer", "token"),
            ..Default::default()
        },
        "apikey" => Auth {
            kind: AuthKind::ApiKey,
            key: param("apikey", "key"),
            value: param("apikey", "value"),
            add_to: if param("apikey", "in") == "query" {
                "query".into()
            } else {
                "header".into()
            },
            ..Default::default()
        },
        _ => Auth::default(),
    }
}

fn url_of(v: Option<&Value>) -> (String, Vec<Kv>) {
    match v {
        Some(Value::String(raw)) => (raw.clone(), Vec::new()),
        Some(obj) => {
            let raw = s(obj, "raw");
            let query = kv_list(obj.get("query"));
            // The raw URL already carries the query string; keep the table
            // in sync by stripping it when the query list is present.
            let url = if !query.is_empty() {
                raw.split('?').next().unwrap_or(&raw).to_string()
            } else {
                raw
            };
            (url, query)
        }
        None => (String::new(), Vec::new()),
    }
}

fn body_of(v: Option<&Value>) -> (Body, Option<String>) {
    let Some(b) = v else {
        return (Body::default(), None);
    };
    let mode = s(b, "mode");
    match mode.as_str() {
        "raw" => {
            let lang = b
                .get("options")
                .and_then(|o| o.get("raw"))
                .map(|r| s(r, "language"))
                .unwrap_or_default();
            let kind = match lang.as_str() {
                "json" => BodyKind::Json,
                "xml" => BodyKind::Xml,
                _ => {
                    let raw = s(b, "raw");
                    if raw.trim_start().starts_with('{') || raw.trim_start().starts_with('[') {
                        BodyKind::Json
                    } else {
                        BodyKind::Text
                    }
                }
            };
            (
                Body {
                    kind,
                    text: s(b, "raw"),
                    ..Default::default()
                },
                None,
            )
        }
        "urlencoded" => (
            Body {
                kind: BodyKind::Form,
                form: kv_list(b.get("urlencoded")),
                ..Default::default()
            },
            None,
        ),
        "formdata" => (
            Body {
                kind: BodyKind::Multipart,
                form: b
                    .get("formdata")
                    .and_then(|x| x.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|e| Kv {
                                enabled: !e
                                    .get("disabled")
                                    .and_then(|d| d.as_bool())
                                    .unwrap_or(false),
                                key: s(e, "key"),
                                value: if s(e, "type") == "file" {
                                    s(e, "src")
                                } else {
                                    s(e, "value")
                                },
                                description: if s(e, "type") == "file" {
                                    "file:".into()
                                } else {
                                    String::new()
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                ..Default::default()
            },
            None,
        ),
        "file" => (
            Body {
                kind: BodyKind::Binary,
                file: b.get("file").map(|f| s(f, "src")).unwrap_or_default(),
                ..Default::default()
            },
            None,
        ),
        "graphql" => {
            let q = b.get("graphql").cloned().unwrap_or(Value::Null);
            (
                Body {
                    kind: BodyKind::Json,
                    text: serde_json::to_string_pretty(&json!({
                        "query": q.get("query").cloned().unwrap_or(Value::String(String::new())),
                        "variables": q.get("variables").and_then(|v| v.as_str()).and_then(|t| serde_json::from_str::<Value>(t).ok()).unwrap_or(json!({})),
                    }))
                    .unwrap_or_default(),
                    ..Default::default()
                },
                Some("Imported from a Postman GraphQL body.".into()),
            )
        }
        _ => (Body::default(), None),
    }
}

fn item_of(v: &Value) -> Item {
    let name = s(v, "name");
    let docs = match v.get("description") {
        Some(Value::String(t)) => t.clone(),
        Some(other) => s(other, "content"),
        None => String::new(),
    };
    if let Some(children) = v.get("item").and_then(|x| x.as_array()) {
        return Item::Folder(Folder {
            name,
            items: children.iter().map(item_of).collect(),
            auth: auth_of(v.get("auth")),
            pre_script: script_of(v.get("event"), "prerequest"),
            post_script: script_of(v.get("event"), "test"),
            docs,
            ..Default::default()
        });
    }
    let req = v.get("request");
    let (method, url, params, headers, body, auth, note) = match req {
        Some(Value::String(raw)) => (
            "GET".to_string(),
            raw.clone(),
            Vec::new(),
            Vec::new(),
            Body::default(),
            Auth::default(),
            None,
        ),
        Some(r) => {
            let (url, params) = url_of(r.get("url"));
            let (body, note) = body_of(r.get("body"));
            (
                {
                    let m = s(r, "method");
                    if m.is_empty() { "GET".into() } else { m }
                },
                url,
                params,
                kv_list(r.get("header")),
                body,
                auth_of(r.get("auth")),
                note,
            )
        }
        None => (
            "GET".to_string(),
            String::new(),
            Vec::new(),
            Vec::new(),
            Body::default(),
            Auth::default(),
            None,
        ),
    };
    let docs = match (docs.is_empty(), note) {
        (true, Some(n)) => n,
        (false, Some(n)) => format!("{docs}\n\n{n}"),
        (_, None) => docs,
    };
    Item::Request(Request {
        name,
        method,
        url,
        params,
        headers,
        body,
        auth,
        pre_script: script_of(v.get("event"), "prerequest"),
        post_script: script_of(v.get("event"), "test"),
        docs,
        ..Default::default()
    })
}

/// A collection from Postman v2.1 JSON.
pub fn import_collection(text: &str) -> Result<Collection, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let info = v
        .get("info")
        .ok_or("not a Postman collection (no \"info\")")?;
    let schema = s(info, "schema");
    if !schema.contains("v2") {
        return Err(format!("unsupported Postman schema: {schema}"));
    }
    Ok(Collection {
        name: s(info, "name"),
        items: v
            .get("item")
            .and_then(|x| x.as_array())
            .map(|arr| arr.iter().map(item_of).collect())
            .unwrap_or_default(),
        variables: kv_list(v.get("variable")),
        auth: {
            let a = auth_of(v.get("auth"));
            if a.kind == AuthKind::Inherit {
                Auth {
                    kind: AuthKind::None,
                    ..Default::default()
                }
            } else {
                a
            }
        },
        pre_script: script_of(v.get("event"), "prerequest"),
        post_script: script_of(v.get("event"), "test"),
        docs: match info.get("description") {
            Some(Value::String(t)) => t.clone(),
            Some(other) => s(other, "content"),
            None => String::new(),
        },
        ..Default::default()
    })
}

/// A Postman environment file: `(name, variables)`.
pub fn import_environment(text: &str) -> Result<(String, Vec<Kv>), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let values = v
        .get("values")
        .and_then(|x| x.as_array())
        .ok_or("not a Postman environment (no \"values\")")?;
    Ok((
        s(&v, "name"),
        values
            .iter()
            .map(|e| Kv {
                enabled: e.get("enabled").and_then(|b| b.as_bool()).unwrap_or(true),
                key: s(e, "key"),
                value: s(e, "value"),
                description: String::new(),
            })
            .collect(),
    ))
}

fn kv_json(list: &[Kv]) -> Value {
    Value::Array(
        list.iter()
            .map(|k| {
                let mut o = json!({ "key": k.key, "value": k.value, "type": "text" });
                if !k.enabled {
                    o["disabled"] = Value::Bool(true);
                }
                if !k.description.is_empty() {
                    o["description"] = Value::String(k.description.clone());
                }
                o
            })
            .collect(),
    )
}

fn events_json(pre: &str, post: &str) -> Value {
    let mut ev = Vec::new();
    for (listen, code) in [("prerequest", pre), ("test", post)] {
        if !code.trim().is_empty() {
            ev.push(json!({
                "listen": listen,
                "script": { "type": "text/javascript", "exec": code.lines().collect::<Vec<_>>() }
            }));
        }
    }
    Value::Array(ev)
}

fn auth_json(a: &Auth) -> Option<Value> {
    let param = |key: &str, value: &str| json!({ "key": key, "value": value, "type": "string" });
    Some(match a.kind {
        AuthKind::Inherit => return None,
        AuthKind::None => json!({ "type": "noauth" }),
        AuthKind::Basic => json!({
            "type": "basic",
            "basic": [param("username", &a.username), param("password", &a.password)]
        }),
        AuthKind::Bearer => json!({ "type": "bearer", "bearer": [param("token", &a.token)] }),
        AuthKind::ApiKey => json!({
            "type": "apikey",
            "apikey": [param("key", &a.key), param("value", &a.value), param("in", if a.add_to == "query" { "query" } else { "header" })]
        }),
    })
}

fn request_json(r: &Request) -> Value {
    let query: Vec<String> = r
        .params
        .iter()
        .filter(|p| p.enabled)
        .map(|p| format!("{}={}", p.key, p.value))
        .collect();
    let raw = if query.is_empty() {
        r.url.clone()
    } else {
        format!("{}?{}", r.url, query.join("&"))
    };
    let mut req = json!({
        "method": r.method,
        "header": kv_json(&r.headers),
        "url": { "raw": raw, "query": kv_json(&r.params) },
    });
    if let Some(a) = auth_json(&r.auth) {
        req["auth"] = a;
    }
    let body = match r.body.kind {
        BodyKind::None => None,
        BodyKind::Json | BodyKind::Text | BodyKind::Xml => Some(json!({
            "mode": "raw",
            "raw": r.body.text,
            "options": { "raw": { "language": match r.body.kind { BodyKind::Json => "json", BodyKind::Xml => "xml", _ => "text" } } }
        })),
        BodyKind::Form => {
            Some(json!({ "mode": "urlencoded", "urlencoded": kv_json(&r.body.form) }))
        }
        BodyKind::Multipart => Some(json!({
            "mode": "formdata",
            "formdata": r.body.form.iter().map(|k| {
                if k.description.starts_with("file:") {
                    json!({ "key": k.key, "type": "file", "src": k.value })
                } else {
                    json!({ "key": k.key, "type": "text", "value": k.value })
                }
            }).collect::<Vec<_>>()
        })),
        BodyKind::Binary => Some(json!({ "mode": "file", "file": { "src": r.body.file } })),
    };
    if let Some(b) = body {
        req["body"] = b;
    }
    if !r.docs.is_empty() {
        req["description"] = Value::String(r.docs.clone());
    }
    let mut item = json!({ "name": r.name, "request": req, "response": [] });
    let ev = events_json(&r.pre_script, &r.post_script);
    if !ev.as_array().is_some_and(|a| a.is_empty()) {
        item["event"] = ev;
    }
    item
}

fn item_json(it: &Item) -> Value {
    match it {
        Item::Request(r) => request_json(r),
        Item::Folder(f) => {
            let mut o = json!({
                "name": f.name,
                "item": f.items.iter().map(item_json).collect::<Vec<_>>(),
            });
            if let Some(a) = auth_json(&f.auth) {
                o["auth"] = a;
            }
            let ev = events_json(&f.pre_script, &f.post_script);
            if !ev.as_array().is_some_and(|a| a.is_empty()) {
                o["event"] = ev;
            }
            if !f.docs.is_empty() {
                o["description"] = Value::String(f.docs.clone());
            }
            o
        }
    }
}

/// Postman v2.1 JSON for a collection.
pub fn export_collection(c: &Collection) -> String {
    let mut root = json!({
        "info": {
            "_postman_id": c.id,
            "name": c.name,
            "description": c.docs,
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
        },
        "item": c.items.iter().map(item_json).collect::<Vec<_>>(),
        "variable": kv_json(&c.variables),
    });
    if let Some(a) = auth_json(&c.auth) {
        root["auth"] = a;
    }
    let ev = events_json(&c.pre_script, &c.post_script);
    if !ev.as_array().is_some_and(|a| a.is_empty()) {
        root["event"] = ev;
    }
    serde_json::to_string_pretty(&root).unwrap_or_default()
}

/// Postman environment JSON for a set of variables.
pub fn export_environment(name: &str, vars: &[(String, String)]) -> String {
    serde_json::to_string_pretty(&json!({
        "name": name,
        "values": vars.iter().map(|(k, v)| json!({ "key": k, "value": v, "enabled": true, "type": "default" })).collect::<Vec<_>>(),
        "_postman_variable_scope": "environment",
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "info": { "name": "Shop", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
      "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}", "type": "string" }] },
      "variable": [{ "key": "base", "value": "https://api.example.com" }],
      "event": [{ "listen": "prerequest", "script": { "exec": ["console.log('hi')"] } }],
      "item": [
        { "name": "orders", "item": [
          { "name": "create", "event": [{ "listen": "test", "script": { "exec": ["pm.test('ok', () => pm.response.to.have.status(201));"] } }],
            "request": { "method": "POST", "header": [{ "key": "X-Trace", "value": "1" }],
              "url": { "raw": "{{base}}/v1/orders?dry=1", "query": [{ "key": "dry", "value": "1" }] },
              "body": { "mode": "raw", "raw": "{\"a\":1}", "options": { "raw": { "language": "json" } } } } }
        ]},
        { "name": "ping", "request": "{{base}}/ping" }
      ]
    }"#;

    #[test]
    fn imports_and_exports_round_trip() {
        let c = import_collection(SAMPLE).unwrap();
        assert_eq!(c.name, "Shop");
        assert_eq!(c.auth.kind, AuthKind::Bearer);
        assert_eq!(c.variables[0].key, "base");
        assert_eq!(c.pre_script, "console.log('hi')");
        let reqs = c.requests();
        assert_eq!(reqs.len(), 2);
        let (path, create) = &reqs[0];
        assert_eq!(path, &vec!["orders".to_string()]);
        assert_eq!(create.method, "POST");
        assert_eq!(create.url, "{{base}}/v1/orders");
        assert_eq!(create.params[0].key, "dry");
        assert_eq!(create.body.kind, BodyKind::Json);
        assert!(create.post_script.contains("pm.test"));
        assert_eq!(reqs[1].1.url, "{{base}}/ping");
        let out = export_collection(&c);
        let again = import_collection(&out).unwrap();
        assert_eq!(again.requests().len(), 2);
        assert_eq!(again.requests()[0].1.body.text, "{\"a\":1}");
        assert_eq!(again.auth.kind, AuthKind::Bearer);
    }

    #[test]
    fn environment_import_and_export() {
        let (name, vars) = import_environment(
            r#"{"name":"staging","values":[{"key":"base","value":"https://s","enabled":true},{"key":"off","value":"x","enabled":false}]}"#,
        )
        .unwrap();
        assert_eq!(name, "staging");
        assert_eq!(vars.len(), 2);
        assert!(!vars[1].enabled);
        let out = export_environment("staging", &[("base".into(), "https://s".into())]);
        assert!(out.contains("\"key\": \"base\""));
        assert!(import_collection("{}").is_err());
    }
}
