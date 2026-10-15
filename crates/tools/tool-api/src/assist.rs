//! The assistant's hands in the API client. An answer may carry one fenced
//! ```hs-api block holding JSON actions (new collection, new request,
//! method, URL, params, headers, body, auth, scripts, docs, variables).
//! The user reads the block and clicks **Apply**; this module then edits
//! the open request and the collections exactly as a click would. Nothing
//! here sends a request: Send stays a user action.

use hs_core::tool::{ToastLevel, ToolContext};
use serde::Deserialize;

use crate::ApiTool;
use crate::model::METHODS;
use crate::model::{AuthKind, BodyKind, Collection, Folder, Item, Kv, Request};

/// The fence language the assistant uses for actions.
pub const LANG: &str = "hs-api";

/// Extra system instructions: the action schema and what is open right
/// now, so the model names collections and requests the client has.
pub fn system_prompt(collections: &[Collection], request: &Request, location: &str) -> String {
    let mut s = String::from(
        "You can also change the API client's open request and its collections. \
         When the user asks you to create, fill in or change something, answer \
         with one short sentence and then exactly one fenced code block tagged \
         `hs-api` containing JSON of the form {\"actions\":[...]}. The user \
         reviews the block and clicks Apply; no request is ever sent by you. \
         Use {{variable}} references for hosts and secrets instead of literal \
         values when an environment variable is likely to exist. Supported \
         actions (\"op\" plus fields):\n\
         - new_collection {name}\n\
         - new_folder {collection?, name}\n\
         - new_request {collection?, folder?, name, method?, url?} (creates and opens it; later actions edit it)\n\
         - open_request {name, collection?}\n\
         - set_name {name}; set_method {method}; set_url {url}\n\
         - set_param {key, value, enabled?}; remove_param {key}\n\
         - set_header {key, value, enabled?}; remove_header {key}\n\
         - set_body {kind: none|json|text|xml|form|multipart|binary, text?, form?: [{key, value}]}\n\
         - set_auth {kind: inherit|none|basic|bearer|api_key, username?, password?, token?, key?, value?, add_to?: header|query}\n\
         - set_script {phase: pre|post, code, append?} (JavaScript with the `pm` API)\n\
         - set_docs {text} (Markdown)\n\
         - set_variable {scope: collection|global, key, value}\n\
         - save\n\
         Omit a collection name to use the one the open request belongs to, or the last one created.\n",
    );
    s.push_str("\nCollections and their requests:\n");
    if collections.is_empty() {
        s.push_str("(none yet)\n");
    }
    for c in collections {
        let names: Vec<String> = c
            .requests()
            .iter()
            .map(|(path, r)| {
                if path.is_empty() {
                    format!("{} {}", r.method, r.name)
                } else {
                    format!("{}/{} {}", path.join("/"), r.method, r.name)
                }
            })
            .collect();
        s.push_str(&format!(
            "- {}: {}\n",
            c.name,
            if names.is_empty() {
                "(empty)".to_string()
            } else {
                names.join("; ")
            }
        ));
    }
    s.push_str(&format!(
        "\nOpen request: {} {} — \"{}\" ({location}). Body kind: {}. Auth: {}.\n",
        request.method,
        if request.url.is_empty() {
            "(no URL)"
        } else {
            &request.url
        },
        request.name,
        request.body.kind.label(),
        request.auth.kind.label()
    ));
    s
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct KvSpec {
    pub key: String,
    pub value: String,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    NewCollection {
        name: String,
    },
    NewFolder {
        #[serde(default)]
        collection: Option<String>,
        name: String,
    },
    NewRequest {
        #[serde(default)]
        collection: Option<String>,
        #[serde(default)]
        folder: Option<String>,
        name: String,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        url: Option<String>,
    },
    OpenRequest {
        name: String,
        #[serde(default)]
        collection: Option<String>,
    },
    SetName {
        name: String,
    },
    SetMethod {
        method: String,
    },
    SetUrl {
        url: String,
    },
    SetParam {
        key: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        enabled: Option<bool>,
    },
    RemoveParam {
        key: String,
    },
    SetHeader {
        key: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        enabled: Option<bool>,
    },
    RemoveHeader {
        key: String,
    },
    SetBody {
        kind: String,
        #[serde(default)]
        text: String,
        #[serde(default)]
        form: Vec<KvSpec>,
    },
    SetAuth {
        kind: String,
        #[serde(default)]
        username: Option<String>,
        #[serde(default)]
        password: Option<String>,
        #[serde(default)]
        token: Option<String>,
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        value: Option<String>,
        #[serde(default)]
        add_to: Option<String>,
    },
    SetScript {
        phase: String,
        code: String,
        #[serde(default)]
        append: bool,
    },
    SetDocs {
        text: String,
    },
    SetVariable {
        scope: String,
        key: String,
        #[serde(default)]
        value: String,
    },
    Save,
}

/// Parse a block body: `{"actions":[...]}` or a bare array.
pub fn parse(body: &str) -> Result<Vec<Action>, String> {
    let v: serde_json::Value = serde_json::from_str(body.trim()).map_err(|e| e.to_string())?;
    let list = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(mut o) => match o.remove("actions") {
            Some(serde_json::Value::Array(a)) => a,
            Some(_) => return Err("\"actions\" must be an array".into()),
            None => vec![serde_json::Value::Object(o)],
        },
        _ => return Err("expected an object with \"actions\" or an array".into()),
    };
    list.into_iter()
        .enumerate()
        .map(|(i, a)| {
            serde_json::from_value::<Action>(a).map_err(|e| format!("action {}: {e}", i + 1))
        })
        .collect()
}

/// What applying a block did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub applied: usize,
    pub errors: Vec<String>,
    /// The request tab that best shows the last change.
    pub focus_tab: Option<usize>,
}

fn upsert(list: &mut Vec<Kv>, key: &str, value: &str, enabled: Option<bool>, ci: bool) {
    let same = |k: &str| {
        if ci {
            k.eq_ignore_ascii_case(key)
        } else {
            k == key
        }
    };
    match list.iter_mut().find(|kv| same(&kv.key)) {
        Some(kv) => {
            kv.value = value.to_string();
            if let Some(e) = enabled {
                kv.enabled = e;
            }
        }
        None => {
            let mut kv = Kv::new(key, value);
            kv.enabled = enabled.unwrap_or(true);
            list.push(kv);
        }
    }
}

fn body_kind(s: &str) -> Option<BodyKind> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "none" | "" => BodyKind::None,
        "json" => BodyKind::Json,
        "text" | "raw" => BodyKind::Text,
        "xml" => BodyKind::Xml,
        "form" | "urlencoded" | "x-www-form-urlencoded" => BodyKind::Form,
        "multipart" | "form-data" => BodyKind::Multipart,
        "binary" | "file" => BodyKind::Binary,
        _ => return None,
    })
}

fn auth_kind(s: &str) -> Option<AuthKind> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "inherit" => AuthKind::Inherit,
        "none" | "no_auth" | "noauth" => AuthKind::None,
        "basic" => AuthKind::Basic,
        "bearer" | "bearer_token" | "token" => AuthKind::Bearer,
        "api_key" | "apikey" | "api-key" => AuthKind::ApiKey,
        _ => return None,
    })
}

impl ApiTool {
    /// The collection an action refers to: by name, else the open
    /// request's, else the most recently created one.
    fn collection_for(&self, name: Option<&str>) -> Option<String> {
        if let Some(n) = name.map(str::trim).filter(|n| !n.is_empty()) {
            return self
                .collections
                .iter()
                .find(|c| c.name.eq_ignore_ascii_case(n))
                .map(|c| c.id.clone());
        }
        self.state
            .collection
            .clone()
            .or_else(|| self.collections.last().map(|c| c.id.clone()))
    }

    /// Apply a parsed action list. Every action is attempted; failures are
    /// collected and the rest still land.
    pub(crate) fn apply_actions(&mut self, actions: &[Action], cx: &mut ToolContext) -> Report {
        let mut rep = Report::default();
        for a in actions {
            match self.apply_action(a, cx) {
                Ok(tab) => {
                    rep.applied += 1;
                    if tab.is_some() {
                        rep.focus_tab = tab;
                    }
                }
                Err(e) => rep.errors.push(e),
            }
        }
        if let Some(tab) = rep.focus_tab {
            self.set_req_tab(tab);
        }
        rep
    }

    fn apply_action(&mut self, a: &Action, cx: &mut ToolContext) -> Result<Option<usize>, String> {
        match a {
            Action::NewCollection { name } => {
                if name.trim().is_empty() {
                    return Err("new_collection: name is empty".into());
                }
                // Applying a block twice (or asking for a collection that
                // already exists) reuses it rather than making a twin.
                let id = match self
                    .collections
                    .iter()
                    .find(|c| c.name.eq_ignore_ascii_case(name.trim()))
                {
                    Some(c) => c.id.clone(),
                    None => self.add_collection(name, cx),
                };
                if self.state.request_id.is_none() {
                    self.state.collection = Some(id);
                }
                Ok(None)
            }
            Action::NewFolder { collection, name } => {
                let cid = self
                    .collection_for(collection.as_deref())
                    .ok_or_else(|| format!("new_folder: no collection for \"{name}\""))?;
                let c = self
                    .collections
                    .iter_mut()
                    .find(|c| c.id == cid)
                    .ok_or("new_folder: collection not found")?;
                c.items.push(Item::Folder(Folder {
                    name: name.trim().to_string(),
                    ..Default::default()
                }));
                self.save_collection(&cid, cx);
                Ok(None)
            }
            Action::NewRequest {
                collection,
                folder,
                name,
                method,
                url,
            } => {
                let cid = match self.collection_for(collection.as_deref()) {
                    Some(id) => id,
                    None => {
                        let cname = collection.clone().unwrap_or_else(|| "Requests".into());
                        self.add_collection(&cname, cx)
                    }
                };
                let method = method.as_deref().map(check_method).transpose()?;
                let url = url.as_deref().map(|u| u.trim().to_string());
                let c = self
                    .collections
                    .iter_mut()
                    .find(|c| c.id == cid)
                    .ok_or("new_request: collection not found")?;
                // The same name in the same collection is that request:
                // open it and update what was given, no twin.
                let existing = c
                    .requests()
                    .iter()
                    .find(|(path, r)| {
                        r.name.eq_ignore_ascii_case(name.trim())
                            && match folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                                Some(f) => path.last().is_some_and(|p| p.eq_ignore_ascii_case(f)),
                                None => path.is_empty(),
                            }
                    })
                    .map(|(_, r)| r.id.clone());
                if let Some(rid) = existing {
                    if let Some(slot) = c.find_mut(&rid) {
                        if let Some(m) = &method {
                            slot.method = m.clone();
                        }
                        if let Some(u) = &url {
                            slot.url = u.clone();
                        }
                    }
                    self.save_collection(&cid, cx);
                    self.open_request(&cid, &rid);
                    return Ok(None);
                }
                let mut req = Request {
                    name: name.trim().to_string(),
                    ..Default::default()
                };
                if let Some(m) = method {
                    req.method = m;
                }
                if let Some(u) = url {
                    req.url = u;
                }
                let rid = req.id.clone();
                let c = self
                    .collections
                    .iter_mut()
                    .find(|c| c.id == cid)
                    .ok_or("new_request: collection not found")?;
                let folder_id = match folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                    Some(fname) => Some(match find_folder(&c.items, fname) {
                        Some(id) => id,
                        None => {
                            let f = Folder {
                                name: fname.to_string(),
                                ..Default::default()
                            };
                            let id = f.id.clone();
                            c.items.push(Item::Folder(f));
                            id
                        }
                    }),
                    None => None,
                };
                c.items_mut(folder_id.as_deref())
                    .ok_or("new_request: folder not found")?
                    .push(Item::Request(req));
                self.save_collection(&cid, cx);
                self.open_request(&cid, &rid);
                Ok(None)
            }
            Action::OpenRequest { name, collection } => {
                let want = name.trim().to_ascii_lowercase();
                let cid = self.collection_for(collection.as_deref());
                let found = self
                    .collections
                    .iter()
                    .filter(|c| cid.as_deref().is_none_or(|id| c.id == id))
                    .find_map(|c| {
                        c.requests()
                            .iter()
                            .find(|(_, r)| r.name.to_ascii_lowercase() == want)
                            .map(|(_, r)| (c.id.clone(), r.id.clone()))
                    })
                    .ok_or_else(|| format!("open_request: no request named \"{name}\""))?;
                self.open_request(&found.0, &found.1);
                Ok(None)
            }
            Action::SetName { name } => {
                self.request.name = name.trim().to_string();
                Ok(None)
            }
            Action::SetMethod { method } => {
                self.request.method = check_method(method)?;
                Ok(None)
            }
            Action::SetUrl { url } => {
                self.request.url = url.trim().to_string();
                Ok(None)
            }
            Action::SetParam {
                key,
                value,
                enabled,
            } => {
                upsert(&mut self.request.params, key.trim(), value, *enabled, false);
                Ok(Some(0))
            }
            Action::RemoveParam { key } => {
                self.request.params.retain(|k| k.key != key.trim());
                Ok(Some(0))
            }
            Action::SetHeader {
                key,
                value,
                enabled,
            } => {
                upsert(&mut self.request.headers, key.trim(), value, *enabled, true);
                Ok(Some(1))
            }
            Action::RemoveHeader { key } => {
                self.request
                    .headers
                    .retain(|k| !k.key.eq_ignore_ascii_case(key.trim()));
                Ok(Some(1))
            }
            Action::SetBody { kind, text, form } => {
                let k =
                    body_kind(kind).ok_or_else(|| format!("set_body: unknown kind \"{kind}\""))?;
                self.request.body.kind = k;
                match k {
                    BodyKind::Form | BodyKind::Multipart => {
                        self.request.body.form = form
                            .iter()
                            .map(|f| {
                                let mut kv = Kv::new(&f.key, &f.value);
                                kv.enabled = f.enabled.unwrap_or(true);
                                kv
                            })
                            .collect();
                    }
                    BodyKind::Binary => self.request.body.file = text.trim().to_string(),
                    _ => {
                        // Lay the body out, then keep exactly what the
                        // box shows: the saved request and the editor
                        // never disagree.
                        self.body_view.set_text_pretty(text, k);
                        self.request.body.text = self.body_view.text();
                    }
                }
                Ok(Some(2))
            }
            Action::SetAuth {
                kind,
                username,
                password,
                token,
                key,
                value,
                add_to,
            } => {
                let k =
                    auth_kind(kind).ok_or_else(|| format!("set_auth: unknown kind \"{kind}\""))?;
                let auth = &mut self.request.auth;
                auth.kind = k;
                if let Some(v) = username {
                    auth.username = v.clone();
                }
                if let Some(v) = password {
                    auth.password = v.clone();
                }
                if let Some(v) = token {
                    auth.token = v.clone();
                }
                if let Some(v) = key {
                    auth.key = v.clone();
                }
                if let Some(v) = value {
                    auth.value = v.clone();
                }
                if let Some(v) = add_to {
                    auth.add_to = v.trim().to_ascii_lowercase();
                }
                Ok(Some(3))
            }
            Action::SetScript {
                phase,
                code,
                append,
            } => {
                let target = match phase.trim().to_ascii_lowercase().as_str() {
                    "pre" | "pre-request" | "prerequest" => &mut self.request.pre_script,
                    "post" | "post-response" | "test" | "tests" => &mut self.request.post_script,
                    other => return Err(format!("set_script: unknown phase \"{other}\"")),
                };
                if *append && !target.is_empty() {
                    if !target.ends_with('\n') {
                        target.push('\n');
                    }
                    target.push_str(code);
                } else {
                    *target = code.clone();
                }
                Ok(Some(4))
            }
            Action::SetDocs { text } => {
                self.request.docs = text.clone();
                Ok(Some(5))
            }
            Action::SetVariable { scope, key, value } => {
                match scope.trim().to_ascii_lowercase().as_str() {
                    "global" | "globals" => {
                        self.globals.set(key.trim(), value);
                        if let Err(e) = self.globals.save(&self.globals_path()) {
                            return Err(format!("set_variable: {e}"));
                        }
                    }
                    "collection" => {
                        let cid = self
                            .collection_for(None)
                            .ok_or("set_variable: no collection is open")?;
                        let c = self
                            .collections
                            .iter_mut()
                            .find(|c| c.id == cid)
                            .ok_or("set_variable: collection not found")?;
                        upsert(&mut c.variables, key.trim(), value, Some(true), false);
                        self.save_collection(&cid, cx);
                    }
                    other => return Err(format!("set_variable: unknown scope \"{other}\"")),
                }
                Ok(None)
            }
            Action::Save => {
                self.save(cx);
                Ok(None)
            }
        }
    }
}

fn check_method(m: &str) -> Result<String, String> {
    let up = m.trim().to_ascii_uppercase();
    if METHODS.contains(&up.as_str()) {
        Ok(up)
    } else {
        Err(format!("unknown method \"{m}\""))
    }
}

fn find_folder(items: &[Item], name: &str) -> Option<String> {
    for it in items {
        if let Item::Folder(f) = it {
            if f.name.eq_ignore_ascii_case(name) {
                return Some(f.id.clone());
            }
            if let Some(id) = find_folder(&f.items, name) {
                return Some(id);
            }
        }
    }
    None
}

/// A one-line summary for the toast after Apply.
pub fn summary(cx: &ToolContext, rep: &Report) -> (ToastLevel, String) {
    if rep.errors.is_empty() {
        (
            ToastLevel::Success,
            cx.services
                .i18n
                .t_args("api-assist-applied", &[("count", &rep.applied.to_string())]),
        )
    } else {
        (
            ToastLevel::Warning,
            cx.services.i18n.t_args(
                "api-assist-partial",
                &[
                    ("count", &rep.applied.to_string()),
                    ("errors", &rep.errors.join("; ")),
                ],
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_object_with_actions_or_a_bare_array() {
        let a =
            parse(r#"{"actions":[{"op":"set_method","method":"post"},{"op":"save"}]}"#).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(
            a[0],
            Action::SetMethod {
                method: "post".into()
            }
        );
        let b = parse(r#"[{"op":"set_url","url":"{{host}}/x"}]"#).unwrap();
        assert_eq!(
            b,
            vec![Action::SetUrl {
                url: "{{host}}/x".into()
            }]
        );
        assert!(parse("not json").is_err());
        assert!(parse(r#"[{"op":"fly"}]"#).is_err());
    }

    #[test]
    fn kinds_and_methods_are_normalised() {
        assert_eq!(body_kind("JSON"), Some(BodyKind::Json));
        assert_eq!(body_kind("x-www-form-urlencoded"), Some(BodyKind::Form));
        assert_eq!(body_kind("pdf"), None);
        assert_eq!(auth_kind("Bearer"), Some(AuthKind::Bearer));
        assert_eq!(auth_kind("api-key"), Some(AuthKind::ApiKey));
        assert_eq!(check_method("patch").as_deref(), Ok("PATCH"));
        assert!(check_method("FETCH").is_err());
    }

    #[test]
    fn upsert_replaces_headers_case_insensitively_and_keeps_params_exact() {
        let mut h = vec![Kv::new("Content-Type", "text/plain")];
        upsert(&mut h, "content-type", "application/json", None, true);
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].value, "application/json");
        let mut p = vec![Kv::new("Page", "1")];
        upsert(&mut p, "page", "2", Some(false), false);
        assert_eq!(p.len(), 2);
        assert!(!p[1].enabled);
    }

    #[test]
    fn system_prompt_lists_collections_and_the_open_request() {
        let mut c = Collection {
            name: "Orders".into(),
            ..Default::default()
        };
        c.items.push(Item::Request(Request {
            name: "List".into(),
            ..Default::default()
        }));
        let r = Request {
            method: "POST".into(),
            url: "{{host}}/orders".into(),
            ..Default::default()
        };
        let p = system_prompt(&[c], &r, "unsaved");
        assert!(p.contains("- Orders: GET List"));
        assert!(p.contains("Open request: POST {{host}}/orders"));
        assert!(p.contains("`hs-api`"));
    }
}
