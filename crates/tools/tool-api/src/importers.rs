//! Collections from other clients (docs/tools/api-client.md, M4): Insomnia's
//! v4 export, a Bruno collection folder, and an OpenAPI 3 document. Postman
//! lives in `postman.rs`; [`import_file`] tries each format in turn and says
//! so when none fits.
//!
//! Every importer maps into HyperStack's own model and nothing else: what a
//! client stores that the model has no place for (Insomnia's cookie jars,
//! Bruno's assertions, OpenAPI's schemas) is left behind rather than kept
//! as opaque text.

use std::path::Path;

use serde_json::Value;

use crate::model::{Auth, AuthKind, Body, BodyKind, Collection, Folder, Item, Kv, Request};

/// Import whatever `path` holds. Postman first, since that is what the
/// button has always taken, then the others by what the file looks like.
pub fn import_file(path: &Path) -> Result<Collection, String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    if name.ends_with(".bru") || name == "bruno.json" {
        let dir = path.parent().ok_or("no folder")?;
        return bruno::import_dir(dir);
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    import_text(&text)
}

/// Import a document: Postman, Insomnia or OpenAPI, whichever it is.
pub fn import_text(text: &str) -> Result<Collection, String> {
    if let Ok(c) = crate::postman::import_collection(text) {
        return Ok(c);
    }
    if let Ok(c) = insomnia::import(text) {
        return Ok(c);
    }
    if let Some(c) = openapi::import(text) {
        return c;
    }
    Err("unknown".into())
}

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

/// The body kind a MIME type maps to.
fn kind_of_mime(mime: &str) -> BodyKind {
    let m = mime.to_ascii_lowercase();
    if m.contains("json") {
        BodyKind::Json
    } else if m.contains("xml") {
        BodyKind::Xml
    } else if m.contains("x-www-form-urlencoded") {
        BodyKind::Form
    } else if m.contains("multipart") {
        BodyKind::Multipart
    } else if m.is_empty() {
        BodyKind::None
    } else {
        BodyKind::Text
    }
}

// --------------------------------------------------------------------- //
// Insomnia v4 export
// --------------------------------------------------------------------- //

pub mod insomnia {
    use super::*;

    /// `{"_type":"export","__export_format":4,"resources":[...]}`: a flat
    /// list of workspace, request groups and requests linked by
    /// `parentId`. Groups become folders; the workspace's name is the
    /// collection's.
    pub fn import(text: &str) -> Result<Collection, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if s(&v, "_type") != "export" {
            return Err("not an Insomnia export".into());
        }
        let resources = v
            .get("resources")
            .and_then(|r| r.as_array())
            .ok_or("no resources")?;
        let workspace = resources
            .iter()
            .find(|r| s(r, "_type") == "workspace")
            .ok_or("no workspace in the export")?;
        let ws_id = s(workspace, "_id");
        let mut c = Collection {
            name: {
                let n = s(workspace, "name");
                if n.is_empty() { "Insomnia".into() } else { n }
            },
            ..Default::default()
        };
        // Variables come from the base environment of the workspace.
        for r in resources
            .iter()
            .filter(|r| s(r, "_type") == "environment" && s(r, "parentId") == ws_id)
        {
            if let Some(data) = r.get("data").and_then(|d| d.as_object()) {
                for (k, val) in data {
                    let value = match val {
                        Value::String(t) => t.clone(),
                        other => other.to_string(),
                    };
                    c.variables.push(Kv::new(k, &value));
                }
            }
        }
        c.items = children(resources, &ws_id);
        Ok(c)
    }

    fn children(resources: &[Value], parent: &str) -> Vec<Item> {
        let mut out: Vec<(i64, Item)> = Vec::new();
        for r in resources.iter().filter(|r| s(r, "parentId") == parent) {
            let seq = r.get("metaSortKey").and_then(|x| x.as_i64()).unwrap_or(0);
            match s(r, "_type").as_str() {
                "request_group" => out.push((
                    seq,
                    Item::Folder(Folder {
                        name: s(r, "name"),
                        items: children(resources, &s(r, "_id")),
                        auth: auth_of(r.get("authentication")),
                        ..Default::default()
                    }),
                )),
                "request" => out.push((seq, Item::Request(request_of(r)))),
                _ => {}
            }
        }
        out.sort_by_key(|(seq, _)| *seq);
        out.into_iter().map(|(_, i)| i).collect()
    }

    fn request_of(r: &Value) -> Request {
        let body = r.get("body").cloned().unwrap_or(Value::Null);
        let mime = s(&body, "mimeType");
        let kind = kind_of_mime(&mime);
        let form = body
            .get("params")
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().map(kv_of).collect())
            .unwrap_or_default();
        Request {
            name: s(r, "name"),
            method: {
                let m = s(r, "method").to_ascii_uppercase();
                if m.is_empty() { "GET".into() } else { m }
            },
            url: s(r, "url"),
            params: r
                .get("parameters")
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().map(kv_of).collect())
                .unwrap_or_default(),
            headers: r
                .get("headers")
                .and_then(|p| p.as_array())
                .map(|arr| arr.iter().map(kv_of).collect())
                .unwrap_or_default(),
            body: Body {
                kind,
                text: s(&body, "text"),
                form,
                ..Default::default()
            },
            auth: auth_of(r.get("authentication")),
            docs: s(r, "description"),
            ..Default::default()
        }
    }

    fn kv_of(v: &Value) -> Kv {
        Kv {
            key: s(v, "name"),
            value: s(v, "value"),
            enabled: !v.get("disabled").and_then(|d| d.as_bool()).unwrap_or(false),
            description: s(v, "description"),
        }
    }

    fn auth_of(v: Option<&Value>) -> Auth {
        let Some(v) = v else {
            return Auth::default();
        };
        match s(v, "type").as_str() {
            "bearer" => Auth {
                kind: AuthKind::Bearer,
                token: s(v, "token"),
                ..Default::default()
            },
            "basic" => Auth {
                kind: AuthKind::Basic,
                username: s(v, "username"),
                password: s(v, "password"),
                ..Default::default()
            },
            "apikey" => Auth {
                kind: AuthKind::ApiKey,
                key: s(v, "key"),
                value: s(v, "value"),
                add_to: if s(v, "addTo") == "queryParams" {
                    "query".into()
                } else {
                    "header".into()
                },
                ..Default::default()
            },
            "none" => Auth {
                kind: AuthKind::None,
                ..Default::default()
            },
            "oauth2" => Auth {
                kind: AuthKind::OAuth2,
                token_url: s(v, "accessTokenUrl"),
                grant: match s(v, "grantType").as_str() {
                    "password" => "password".into(),
                    "refresh_token" => "refresh_token".into(),
                    _ => "client_credentials".into(),
                },
                client_id: s(v, "clientId"),
                client_secret: s(v, "clientSecret"),
                scope: s(v, "scope"),
                audience: s(v, "audience"),
                username: s(v, "username"),
                password: s(v, "password"),
                ..Default::default()
            },
            "iam" => Auth {
                kind: AuthKind::AwsSigV4,
                access_key: s(v, "accessKeyId"),
                secret_key: s(v, "secretAccessKey"),
                session_token: s(v, "sessionToken"),
                region: s(v, "region"),
                service: s(v, "service"),
                ..Default::default()
            },
            _ => Auth::default(),
        }
    }
}

// --------------------------------------------------------------------- //
// Bruno: a folder of .bru files
// --------------------------------------------------------------------- //

pub mod bruno {
    use super::*;

    /// A collection folder: `bruno.json` (or any `.bru` file's folder, which
    /// is walked up to the one holding `bruno.json`, or taken as the root
    /// when there is none). Sub-folders become folders; each `.bru` file
    /// a request, ordered by its `seq`.
    pub fn import_dir(dir: &Path) -> Result<Collection, String> {
        let root = ancestors_with_bruno_json(dir).unwrap_or(dir.to_path_buf());
        let name = std::fs::read_to_string(root.join("bruno.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .map(|v| s(&v, "name"))
            .filter(|n| !n.is_empty())
            .or_else(|| root.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Bruno".into());
        let items = items_in(&root)?;
        if items.is_empty() {
            return Err("no .bru files found".into());
        }
        Ok(Collection {
            name,
            items,
            ..Default::default()
        })
    }

    fn ancestors_with_bruno_json(dir: &Path) -> Option<std::path::PathBuf> {
        let mut at = Some(dir);
        while let Some(d) = at {
            if d.join("bruno.json").is_file() {
                return Some(d.to_path_buf());
            }
            at = d.parent();
        }
        None
    }

    fn items_in(dir: &Path) -> Result<Vec<Item>, String> {
        let mut out: Vec<(i64, Item)> = Vec::new();
        let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
        for entry in rd.flatten() {
            let p = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if name.starts_with('.') || name == "node_modules" || name == "environments" {
                    continue;
                }
                let items = items_in(&p)?;
                if !items.is_empty() {
                    out.push((
                        i64::MAX,
                        Item::Folder(Folder {
                            name,
                            items,
                            ..Default::default()
                        }),
                    ));
                }
            } else if name.ends_with(".bru") && name != "collection.bru" && name != "folder.bru" {
                let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
                let (seq, req) = parse(&text, name.trim_end_matches(".bru"));
                out.push((seq, Item::Request(req)));
            }
        }
        out.sort_by_key(|(seq, _)| *seq);
        Ok(out.into_iter().map(|(_, i)| i).collect())
    }

    /// One `.bru` file: blocks of `name { ... }`, with `key: value` lines in
    /// most of them and raw text in `body:json`, `body:text` and the like.
    pub fn parse(text: &str, fallback_name: &str) -> (i64, Request) {
        let blocks = blocks(text);
        let get = |name: &str| {
            blocks
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, b)| b.as_str())
        };
        let meta = get("meta").map(pairs).unwrap_or_default();
        let name = meta
            .iter()
            .find(|(k, _)| k == "name")
            .map(|(_, v)| v.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback_name.to_string());
        let seq = meta
            .iter()
            .find(|(k, _)| k == "seq")
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(i64::MAX - 1);
        let mut req = Request {
            name,
            ..Default::default()
        };
        for method in [
            "get", "post", "put", "patch", "delete", "head", "options", "trace",
        ] {
            if let Some(b) = get(method) {
                req.method = method.to_ascii_uppercase();
                let p = pairs(b);
                req.url = p
                    .iter()
                    .find(|(k, _)| k == "url")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                let body_kind = p
                    .iter()
                    .find(|(k, _)| k == "body")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                req.body.kind = match body_kind.as_str() {
                    "json" => BodyKind::Json,
                    "xml" => BodyKind::Xml,
                    "text" => BodyKind::Text,
                    "formUrlEncoded" => BodyKind::Form,
                    "multipartForm" => BodyKind::Multipart,
                    _ => BodyKind::None,
                };
                let auth = p
                    .iter()
                    .find(|(k, _)| k == "auth")
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default();
                req.auth.kind = match auth.as_str() {
                    "none" => AuthKind::None,
                    "bearer" => AuthKind::Bearer,
                    "basic" => AuthKind::Basic,
                    "apikey" => AuthKind::ApiKey,
                    _ => AuthKind::Inherit,
                };
            }
        }
        if let Some(b) = get("query") {
            req.params = kvs(b);
        }
        if let Some(b) = get("headers") {
            req.headers = kvs(b);
        }
        for (block, kind) in [
            ("body:json", BodyKind::Json),
            ("body:text", BodyKind::Text),
            ("body:xml", BodyKind::Xml),
        ] {
            if let Some(b) = get(block) {
                req.body.text = unindent(b);
                if req.body.kind == BodyKind::None {
                    req.body.kind = kind;
                }
            }
        }
        if let Some(b) = get("body:form-urlencoded") {
            req.body.form = kvs(b);
            req.body.kind = BodyKind::Form;
        }
        if let Some(b) = get("auth:bearer") {
            let p = pairs(b);
            req.auth.kind = AuthKind::Bearer;
            req.auth.token = p
                .iter()
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
        }
        if let Some(b) = get("auth:basic") {
            let p = pairs(b);
            req.auth.kind = AuthKind::Basic;
            for (k, v) in p {
                match k.as_str() {
                    "username" => req.auth.username = v,
                    "password" => req.auth.password = v,
                    _ => {}
                }
            }
        }
        if let Some(b) = get("docs") {
            req.docs = unindent(b);
        }
        (seq, req)
    }

    /// Top-level `name {` … `}` blocks, with their inner text.
    fn blocks(text: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut name: Option<String> = None;
        let mut body = String::new();
        let mut depth = 0usize;
        for line in text.lines() {
            let t = line.trim_end();
            if name.is_none() {
                if let Some(head) = t.strip_suffix('{') {
                    name = Some(head.trim().to_string());
                    depth = 1;
                    body.clear();
                }
                continue;
            }
            // A nested brace (a JSON body) keeps the block open.
            let opens = t.matches('{').count();
            let closes = t.matches('}').count();
            if t.trim() == "}" && depth == 1 {
                out.push((name.take().unwrap_or_default(), body.clone()));
                continue;
            }
            depth = (depth + opens).saturating_sub(closes).max(1);
            body.push_str(line);
            body.push('\n');
        }
        out
    }

    /// `key: value` lines; a `~` prefix means disabled (kept, flagged).
    fn pairs(block: &str) -> Vec<(String, String)> {
        block
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let (k, v) = l.split_once(':')?;
                Some((
                    k.trim().trim_start_matches('~').to_string(),
                    v.trim().to_string(),
                ))
            })
            .collect()
    }

    fn kvs(block: &str) -> Vec<Kv> {
        block
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                let (k, v) = l.split_once(':')?;
                let disabled = k.trim().starts_with('~');
                Some(Kv {
                    key: k.trim().trim_start_matches('~').to_string(),
                    value: v.trim().to_string(),
                    enabled: !disabled,
                    description: String::new(),
                })
            })
            .collect()
    }

    /// Strip the common leading indentation of a raw block.
    fn unindent(block: &str) -> String {
        let indent = block
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        block
            .lines()
            .map(|l| {
                if l.len() >= indent {
                    &l[indent..]
                } else {
                    l.trim_start()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string()
    }
}

// --------------------------------------------------------------------- //
// OpenAPI 3
// --------------------------------------------------------------------- //

pub mod openapi {
    use super::*;

    /// `None` when the text is not an OpenAPI 3 document at all; `Some(Err)`
    /// when it is one that could not be read.
    pub fn import(text: &str) -> Option<Result<Collection, String>> {
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => super::yaml::to_json(text)?,
        };
        if !s(&v, "openapi").starts_with('3') {
            return None;
        }
        Some(build(&v))
    }

    fn build(v: &Value) -> Result<Collection, String> {
        let info = v.get("info").cloned().unwrap_or(Value::Null);
        let server = v
            .get("servers")
            .and_then(|a| a.as_array())
            .and_then(|a| a.first())
            .map(|srv| s(srv, "url"))
            .unwrap_or_default();
        let mut c = Collection {
            name: {
                let n = s(&info, "title");
                if n.is_empty() { "OpenAPI".into() } else { n }
            },
            docs: s(&info, "description"),
            ..Default::default()
        };
        if !server.is_empty() {
            c.variables.push(Kv::new("baseUrl", &server));
        }
        let paths = v
            .get("paths")
            .and_then(|p| p.as_object())
            .ok_or("no paths")?;
        // One folder per first tag, requests without a tag at the root.
        let mut folders: Vec<Folder> = Vec::new();
        let mut loose: Vec<Item> = Vec::new();
        for (path, ops) in paths {
            let Some(ops) = ops.as_object() else { continue };
            for (method, op) in ops {
                let m = method.to_ascii_uppercase();
                if ![
                    "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
                ]
                .contains(&m.as_str())
                {
                    continue;
                }
                let req = request_of(path, &m, op, !server.is_empty());
                let tag = op
                    .get("tags")
                    .and_then(|t| t.as_array())
                    .and_then(|t| t.first())
                    .and_then(|t| t.as_str())
                    .map(str::to_string);
                match tag {
                    Some(tag) => match folders.iter_mut().find(|f| f.name == tag) {
                        Some(f) => f.items.push(Item::Request(req)),
                        None => folders.push(Folder {
                            name: tag,
                            items: vec![Item::Request(req)],
                            ..Default::default()
                        }),
                    },
                    None => loose.push(Item::Request(req)),
                }
            }
        }
        c.items = folders.into_iter().map(Item::Folder).collect();
        c.items.extend(loose);
        Ok(c)
    }

    fn request_of(path: &str, method: &str, op: &Value, has_server: bool) -> Request {
        let name = {
            let summary = s(op, "summary");
            if !summary.is_empty() {
                summary
            } else {
                let id = s(op, "operationId");
                if id.is_empty() {
                    format!("{method} {path}")
                } else {
                    id
                }
            }
        };
        let mut req = Request {
            name,
            method: method.to_string(),
            url: if has_server {
                format!("{{{{baseUrl}}}}{path}")
            } else {
                path.to_string()
            },
            docs: s(op, "description"),
            ..Default::default()
        };
        if let Some(params) = op.get("parameters").and_then(|p| p.as_array()) {
            for p in params {
                let name = s(p, "name");
                let example = p
                    .get("example")
                    .map(|e| match e {
                        Value::String(t) => t.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                match s(p, "in").as_str() {
                    "query" => req.params.push(Kv {
                        key: name,
                        value: example,
                        enabled: p.get("required").and_then(|r| r.as_bool()).unwrap_or(false),
                        description: s(p, "description"),
                    }),
                    "header" => req.headers.push(Kv {
                        key: name,
                        value: example,
                        enabled: true,
                        description: s(p, "description"),
                    }),
                    _ => {}
                }
            }
        }
        if let Some(content) = op
            .get("requestBody")
            .and_then(|b| b.get("content"))
            .and_then(|c| c.as_object())
            && let Some((mime, media)) = content.iter().next()
        {
            {
                req.body.kind = kind_of_mime(mime);
                let example = media.get("example").cloned().or_else(|| {
                    media
                        .get("examples")
                        .and_then(|e| e.as_object())
                        .and_then(|e| e.values().next())
                        .and_then(|e| e.get("value").cloned())
                });
                if let Some(ex) = example {
                    req.body.text = match ex {
                        Value::String(t) => t,
                        other => serde_json::to_string_pretty(&other).unwrap_or_default(),
                    };
                }
            }
        }
        req
    }
}

/// A small YAML reader for OpenAPI documents: enough for the shapes those
/// use (mappings, sequences, scalars, block scalars), turned into JSON.
mod yaml {
    use serde_json::Value;

    pub fn to_json(text: &str) -> Option<Value> {
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
            .collect();
        if lines.is_empty() {
            return None;
        }
        let mut i = 0;
        let v = parse_block(&lines, &mut i, 0)?;
        Some(v)
    }

    fn indent_of(l: &str) -> usize {
        l.len() - l.trim_start().len()
    }

    fn scalar(t: &str) -> Value {
        let t = t.trim();
        if let Some(q) = t
            .strip_prefix('"')
            .and_then(|r| r.strip_suffix('"'))
            .or_else(|| t.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
        {
            return Value::String(q.to_string());
        }
        match t {
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            "null" | "~" => return Value::Null,
            _ => {}
        }
        if let Ok(n) = t.parse::<i64>() {
            return Value::Number(n.into());
        }
        if let Ok(f) = t.parse::<f64>()
            && let Some(n) = serde_json::Number::from_f64(f)
        {
            return Value::Number(n);
        }
        Value::String(t.to_string())
    }

    /// A mapping or a sequence at `indent`, read until the indentation
    /// drops below it.
    fn parse_block(lines: &[&str], i: &mut usize, indent: usize) -> Option<Value> {
        let first = lines.get(*i)?;
        if first.trim_start().starts_with("- ") || first.trim() == "-" {
            let mut arr = Vec::new();
            while *i < lines.len() {
                let l = lines[*i];
                if indent_of(l) != indent || !(l.trim_start().starts_with("- ") || l.trim() == "-")
                {
                    break;
                }
                let rest = l.trim_start()[1..].trim_start();
                *i += 1;
                if rest.is_empty() {
                    arr.push(
                        parse_block(lines, i, next_indent(lines, *i, indent))
                            .unwrap_or(Value::Null),
                    );
                } else if rest.contains(": ") || rest.ends_with(':') {
                    // A mapping that starts on the dash line: re-read it as
                    // if it were indented past the dash.
                    let inner_indent = indent + 2;
                    let mut map = serde_json::Map::new();
                    insert_pair(&mut map, rest, lines, i, inner_indent);
                    while *i < lines.len() && indent_of(lines[*i]) == inner_indent {
                        let l = lines[*i].trim_start();
                        *i += 1;
                        insert_pair(&mut map, l, lines, i, inner_indent);
                    }
                    arr.push(Value::Object(map));
                } else {
                    arr.push(scalar(rest));
                }
            }
            return Some(Value::Array(arr));
        }
        let mut map = serde_json::Map::new();
        while *i < lines.len() {
            let l = lines[*i];
            if indent_of(l) != indent {
                if indent_of(l) < indent {
                    break;
                }
                *i += 1;
                continue;
            }
            *i += 1;
            insert_pair(&mut map, l.trim_start(), lines, i, indent);
        }
        Some(Value::Object(map))
    }

    fn next_indent(lines: &[&str], i: usize, at_least: usize) -> usize {
        lines.get(i).map(|l| indent_of(l)).unwrap_or(at_least)
    }

    /// `key: value` or `key:` followed by a nested block or a block scalar.
    fn insert_pair(
        map: &mut serde_json::Map<String, Value>,
        line: &str,
        lines: &[&str],
        i: &mut usize,
        indent: usize,
    ) {
        let Some((k, v)) = line.split_once(':') else {
            return;
        };
        let key = k.trim().trim_matches('"').trim_matches('\'').to_string();
        let v = v.trim();
        if v.is_empty() {
            if *i < lines.len() && indent_of(lines[*i]) > indent {
                let child = next_indent(lines, *i, indent + 1);
                let val = parse_block(lines, i, child).unwrap_or(Value::Null);
                map.insert(key, val);
            } else {
                map.insert(key, Value::Null);
            }
        } else if v == "|" || v == ">" || v == "|-" || v == ">-" {
            let mut text = String::new();
            let block_indent = next_indent(lines, *i, indent + 1);
            while *i < lines.len() && indent_of(lines[*i]) >= block_indent {
                text.push_str(&lines[*i][block_indent..]);
                text.push(if v.starts_with('>') { ' ' } else { '\n' });
                *i += 1;
            }
            map.insert(key, Value::String(text.trim_end().to_string()));
        } else if v == "[]" {
            map.insert(key, Value::Array(Vec::new()));
        } else if v == "{}" {
            map.insert(key, Value::Object(serde_json::Map::new()));
        } else if let Some(inner) = v.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            map.insert(
                key,
                Value::Array(inner.split(',').map(|t| scalar(t.trim())).collect()),
            );
        } else {
            map.insert(key, scalar(v));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_insomnia_export_becomes_folders_and_requests() {
        let text = r#"{"_type":"export","__export_format":4,"resources":[
          {"_type":"workspace","_id":"wrk_1","name":"Shop"},
          {"_type":"environment","_id":"env_1","parentId":"wrk_1","data":{"host":"https://api.test"}},
          {"_type":"request_group","_id":"fld_1","parentId":"wrk_1","name":"Orders","metaSortKey":2},
          {"_type":"request","_id":"req_1","parentId":"fld_1","name":"Create","method":"post","url":"{{ _.host }}/orders",
           "body":{"mimeType":"application/json","text":"{\"sku\":1}"},
           "headers":[{"name":"X-Trace","value":"1"}],"parameters":[{"name":"dry","value":"true","disabled":true}],
           "authentication":{"type":"bearer","token":"{{ _.token }}"},"metaSortKey":1},
          {"_type":"request","_id":"req_2","parentId":"wrk_1","name":"Ping","method":"get","url":"{{ _.host }}/ping","metaSortKey":1}
        ]}"#;
        let c = insomnia::import(text).unwrap();
        assert_eq!(c.name, "Shop");
        assert_eq!(c.variables[0].key, "host");
        assert_eq!(c.items.len(), 2);
        let Item::Request(ping) = &c.items[0] else {
            panic!("ping first by sort key")
        };
        assert_eq!(ping.method, "GET");
        let Item::Folder(f) = &c.items[1] else {
            panic!("then the folder")
        };
        let Item::Request(create) = &f.items[0] else {
            panic!()
        };
        assert_eq!(create.method, "POST");
        assert_eq!(create.body.kind, BodyKind::Json);
        assert_eq!(create.auth.kind, AuthKind::Bearer);
        assert!(!create.params[0].enabled);
        assert_eq!(create.headers[0].key, "X-Trace");
    }

    #[test]
    fn a_bru_file_is_one_request() {
        let text = "meta {\n  name: Create order\n  type: http\n  seq: 2\n}\n\npost {\n  url: {{host}}/orders\n  body: json\n  auth: bearer\n}\n\nheaders {\n  X-Trace: 1\n  ~X-Off: 0\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\nbody:json {\n  {\n    \"sku\": 1\n  }\n}\n";
        let (seq, r) = bruno::parse(text, "fallback");
        assert_eq!(seq, 2);
        assert_eq!(r.name, "Create order");
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "{{host}}/orders");
        assert_eq!(r.body.kind, BodyKind::Json);
        assert_eq!(r.body.text, "{\n  \"sku\": 1\n}");
        assert_eq!(r.headers.len(), 2);
        assert!(r.headers[0].enabled && !r.headers[1].enabled);
        assert_eq!(r.auth.kind, AuthKind::Bearer);
        assert_eq!(r.auth.token, "{{token}}");
    }

    #[test]
    fn a_bruno_folder_is_walked_in_seq_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        std::fs::write(root.join("bruno.json"), r#"{"version":"1","name":"Pets"}"#).unwrap();
        std::fs::write(
            root.join("list.bru"),
            "meta {\n  name: List\n  seq: 2\n}\nget {\n  url: /pets\n}\n",
        )
        .unwrap();
        std::fs::create_dir(root.join("admin")).unwrap();
        std::fs::write(
            root.join("admin/purge.bru"),
            "meta {\n  name: Purge\n  seq: 1\n}\ndelete {\n  url: /pets\n}\n",
        )
        .unwrap();
        let c = bruno::import_dir(&root.join("admin")).unwrap();
        assert_eq!(c.name, "Pets", "walked up to bruno.json");
        assert_eq!(c.items.len(), 2);
        assert!(matches!(&c.items[0], Item::Request(r) if r.name == "List"));
        assert!(matches!(&c.items[1], Item::Folder(f) if f.name == "admin"));
    }

    #[test]
    fn an_openapi_document_in_yaml_becomes_requests_by_tag() {
        let text = "openapi: 3.0.3\ninfo:\n  title: Pet Store\n  description: |\n    Pets.\n    More pets.\nservers:\n  - url: https://api.test/v1\npaths:\n  /pets:\n    get:\n      summary: List pets\n      tags: [pets]\n      parameters:\n        - name: limit\n          in: query\n          required: true\n          example: 10\n    post:\n      operationId: createPet\n      tags:\n        - pets\n      requestBody:\n        content:\n          application/json:\n            example:\n              name: Rex\n  /health:\n    get:\n      summary: Health\n";
        let c = openapi::import(text).unwrap().unwrap();
        assert_eq!(c.name, "Pet Store");
        assert_eq!(c.docs, "Pets.\nMore pets.");
        assert_eq!(c.variables[0].value, "https://api.test/v1");
        let Item::Folder(pets) = &c.items[0] else {
            panic!("the tag folder first")
        };
        assert_eq!(pets.items.len(), 2);
        let Item::Request(list) = &pets.items[0] else {
            panic!()
        };
        assert_eq!(list.url, "{{baseUrl}}/pets");
        assert_eq!(list.params[0].key, "limit");
        assert_eq!(list.params[0].value, "10");
        assert!(list.params[0].enabled);
        let Item::Request(create) = &pets.items[1] else {
            panic!()
        };
        assert_eq!(create.name, "createPet");
        assert_eq!(create.body.kind, BodyKind::Json);
        assert!(create.body.text.contains("\"name\": \"Rex\""));
        assert!(matches!(&c.items[1], Item::Request(r) if r.name == "Health"));
    }

    #[test]
    fn unknown_text_is_refused_by_name() {
        assert_eq!(import_text("hello").unwrap_err(), "unknown");
        assert!(import_text("{\"swagger\":\"2.0\"}").is_err());
    }
}
