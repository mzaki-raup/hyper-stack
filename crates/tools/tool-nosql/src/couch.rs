//! The CouchDB driver of docs/tools/nosql.md §2. CouchDB has no maintained
//! Rust client, so this talks to its HTTP API directly: `_all_dbs`,
//! `_all_docs`, Mango `_find`, and documents addressed by `_id` with `_rev`
//! as the conflict check, which is what makes a save safe here.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use base64::Engine as _;

use crate::store::{
    BulkSummary, ContainerInfo, ContainerKind, ContainerRef, DocStore, Document, Error,
    FindSummary, IndexInfo, PutMode, Query, Result, Scope, StoreKind, parse_object,
};

#[derive(Clone, Default)]
pub struct ConnectConfig {
    /// `http://host:5984`, without a trailing slash.
    pub url: String,
    pub user: String,
    pub password: Option<String>,
    /// The database the tree opens on; empty lists them all.
    pub database: String,
    pub read_only: bool,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for ConnectConfig {
    /// Never prints the password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectConfig")
            .field("url", &self.url)
            .field("user", &self.user)
            .field("database", &self.database)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error {
            code: e.status().map(|s| s.as_u16().to_string()),
            message: e.to_string(),
        }
    }
}

pub struct CouchStore {
    http: reqwest::Client,
    base: String,
    auth: Option<String>,
    version: String,
}

impl CouchStore {
    pub async fn connect(cfg: &ConnectConfig) -> Result<CouchStore> {
        let base = cfg.url.trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(Error::msg("this connection has no URL"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(cfg.connect_timeout)
            .build()
            .map_err(Error::from)?;
        let auth = (!cfg.user.is_empty()).then(|| {
            let raw = format!("{}:{}", cfg.user, cfg.password.clone().unwrap_or_default());
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(raw)
            )
        });
        let mut store = CouchStore {
            http,
            base,
            auth,
            version: String::new(),
        };
        // The welcome document proves the server is there and names it.
        let root: serde_json::Value = store.get("/").await?;
        store.version = root
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        Ok(store)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let b = self.http.request(method, format!("{}{path}", self.base));
        match &self.auth {
            Some(a) => b.header(reqwest::header::AUTHORIZATION, a),
            None => b,
        }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<serde_json::Value> {
        let resp = req.send().await?;
        let status = resp.status();
        let body = resp.text().await?;
        let value: serde_json::Value =
            serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        if status.is_success() {
            return Ok(value);
        }
        // CouchDB answers `{"error": "...", "reason": "..."}`.
        let reason = value
            .get("reason")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| body.chars().take(200).collect());
        let code = value
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| status.as_u16().to_string());
        Err(Error::coded(code, reason))
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::GET, path)).await
    }

    async fn post(&self, path: &str, body: &serde_json::Value) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::POST, path).json(body))
            .await
    }
}

/// `_id` and `_rev` out of a document the server returned.
fn id_and_rev(v: &serde_json::Value) -> (String, Option<String>) {
    let id = v
        .get("_id")
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string();
    let rev = v.get("_rev").and_then(|x| x.as_str()).map(str::to_string);
    (id, rev)
}

/// Percent-encode a document id for a URL path (`a/b` is a legal id).
fn encode_id(id: &str) -> String {
    let mut out = String::with_capacity(id.len());
    for b in id.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            // A design document keeps its slash: `_design/name` is one id.
            b'/' if id.starts_with("_design/") || id.starts_with("_local/") => out.push('/'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[async_trait]
impl DocStore for CouchStore {
    fn kind(&self) -> StoreKind {
        StoreKind::Couch
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn databases(&mut self) -> Result<Vec<String>> {
        let v = self.get("/_all_dbs").await?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// A CouchDB database holds documents directly, so it is its own
    /// container; the tree shows one child under each database.
    async fn containers(&mut self, scope: &Scope) -> Result<Vec<ContainerInfo>> {
        let info = self.get(&format!("/{}", scope.database)).await?;
        Ok(vec![ContainerInfo {
            name: scope.database.clone(),
            kind: ContainerKind::Database,
            count: info.get("doc_count").and_then(|v| v.as_u64()),
            size_bytes: info
                .pointer("/sizes/active")
                .or_else(|| info.get("data_size"))
                .and_then(|v| v.as_u64()),
        }])
    }

    async fn find(&mut self, c: &ContainerRef, q: &Query) -> Result<(Vec<Document>, FindSummary)> {
        let started = Instant::now();
        let selector = parse_object(&q.filter, "selector")?;
        let limit = q.limit.max(1);
        let docs: Vec<serde_json::Value> =
            if selector.is_empty() && q.sort.trim().is_empty() && q.projection.trim().is_empty() {
                // No selector: `_all_docs` is cheaper and needs no index.
                let path = format!(
                    "/{}/_all_docs?include_docs=true&limit={}&skip={}",
                    c.name,
                    limit + 1,
                    q.skip.max(0)
                );
                let v = self.get(&path).await?;
                v.get("rows")
                    .and_then(|r| r.as_array())
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|r| r.get("doc").cloned())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            } else {
                let mut body = serde_json::json!({
                    "selector": serde_json::Value::Object(selector),
                    "limit": limit + 1,
                    "skip": q.skip.max(0),
                });
                if !q.projection.trim().is_empty() {
                    // Mango takes a list of field names, not a projection map.
                    let fields: Vec<String> = parse_object(&q.projection, "fields")?
                        .keys()
                        .cloned()
                        .collect();
                    body["fields"] = serde_json::json!(fields);
                }
                if !q.sort.trim().is_empty() {
                    let sort = parse_object(&q.sort, "sort")?;
                    let list: Vec<serde_json::Value> = sort
                        .into_iter()
                        .map(|(k, v)| {
                            let dir = if v.as_i64().unwrap_or(1) < 0 {
                                "desc"
                            } else {
                                "asc"
                            };
                            serde_json::json!({ k: dir })
                        })
                        .collect();
                    body["sort"] = serde_json::Value::Array(list);
                }
                let v = self.post(&format!("/{}/_find", c.name), &body).await?;
                v.get("docs")
                    .and_then(|d| d.as_array())
                    .cloned()
                    .unwrap_or_default()
            };
        let more = docs.len() as i64 > limit;
        let out: Vec<Document> = docs
            .into_iter()
            .take(limit as usize)
            .map(|json| {
                let (id, rev) = id_and_rev(&json);
                Document::new(json, id, rev)
            })
            .collect();
        Ok((
            out.clone(),
            FindSummary {
                returned: out.len(),
                elapsed: started.elapsed(),
                more,
            },
        ))
    }

    async fn count(&mut self, c: &ContainerRef, filter: &str) -> Result<u64> {
        let selector = parse_object(filter, "selector")?;
        if selector.is_empty() {
            let info = self.get(&format!("/{}", c.name)).await?;
            return Ok(info.get("doc_count").and_then(|v| v.as_u64()).unwrap_or(0));
        }
        // Mango has no count: ask for ids only and count what comes back.
        let body = serde_json::json!({
            "selector": serde_json::Value::Object(selector),
            "fields": ["_id"],
            "limit": 100_000,
        });
        let v = self.post(&format!("/{}/_find", c.name), &body).await?;
        Ok(v.get("docs")
            .and_then(|d| d.as_array())
            .map(|a| a.len())
            .unwrap_or(0) as u64)
    }

    async fn put(&mut self, c: &ContainerRef, doc: &Document, mode: PutMode) -> Result<Document> {
        let mut json = doc.json.clone();
        let obj = json
            .as_object_mut()
            .ok_or_else(|| Error::msg("a document must be a JSON object"))?;
        let reply = match mode {
            PutMode::Insert => {
                // Let CouchDB assign the id unless the document names one.
                obj.remove("_rev");
                self.post(&format!("/{}", c.name), &json).await?
            }
            PutMode::Replace { rev } => {
                let id = obj
                    .get("_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
                    .ok_or_else(|| Error::msg("this document has no _id to replace"))?;
                if let Some(r) = rev.or_else(|| doc.rev.clone()) {
                    obj.insert("_rev".into(), serde_json::Value::String(r));
                }
                self.send(
                    self.request(
                        reqwest::Method::PUT,
                        &format!("/{}/{}", c.name, encode_id(&id)),
                    )
                    .json(&json),
                )
                .await?
            }
        };
        let id = reply
            .get("id")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| doc.id.clone());
        let rev = reply
            .get("rev")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        // Show the document as the server now has it.
        if let Some(obj) = json.as_object_mut() {
            obj.insert("_id".into(), serde_json::Value::String(id.clone()));
            if let Some(r) = &rev {
                obj.insert("_rev".into(), serde_json::Value::String(r.clone()));
            }
        }
        Ok(Document::new(json, id, rev))
    }

    async fn delete(&mut self, c: &ContainerRef, doc: &Document) -> Result<()> {
        let rev = doc
            .rev
            .clone()
            .or_else(|| {
                doc.json
                    .get("_rev")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| Error::msg("this document has no _rev, so it cannot be deleted"))?;
        self.send(self.request(
            reqwest::Method::DELETE,
            &format!("/{}/{}?rev={rev}", c.name, encode_id(&doc.id)),
        ))
        .await?;
        Ok(())
    }

    async fn delete_many(
        &mut self,
        c: &ContainerRef,
        filter: &str,
        dry_run: bool,
    ) -> Result<BulkSummary> {
        let selector = parse_object(filter, "selector")?;
        if selector.is_empty() {
            return Err(Error::msg(
                "deleting every document needs a selector, even an empty-looking one",
            ));
        }
        let body = serde_json::json!({
            "selector": serde_json::Value::Object(selector),
            "fields": ["_id", "_rev"],
            "limit": 100_000,
        });
        let v = self.post(&format!("/{}/_find", c.name), &body).await?;
        let docs = v
            .get("docs")
            .and_then(|d| d.as_array())
            .cloned()
            .unwrap_or_default();
        let matched = docs.len() as u64;
        if dry_run {
            return Ok(BulkSummary {
                matched,
                changed: 0,
                dry_run: true,
            });
        }
        // `_bulk_docs` with `_deleted` is CouchDB's delete-many.
        let marked: Vec<serde_json::Value> = docs
            .into_iter()
            .map(|mut d| {
                if let Some(o) = d.as_object_mut() {
                    o.insert("_deleted".into(), serde_json::Value::Bool(true));
                }
                d
            })
            .collect();
        let reply = self
            .post(
                &format!("/{}/_bulk_docs", c.name),
                &serde_json::json!({ "docs": marked }),
            )
            .await?;
        let changed = reply
            .as_array()
            .map(|a| a.iter().filter(|r| r.get("ok").is_some()).count() as u64)
            .unwrap_or(0);
        Ok(BulkSummary {
            matched,
            changed,
            dry_run: false,
        })
    }

    async fn indexes(&mut self, c: &ContainerRef) -> Result<Vec<IndexInfo>> {
        let v = self.get(&format!("/{}/_index", c.name)).await?;
        Ok(v.get("indexes")
            .and_then(|i| i.as_array())
            .map(|a| {
                a.iter()
                    .map(|i| IndexInfo {
                        name: i
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .to_string(),
                        definition: i.get("def").map(|d| d.to_string()).unwrap_or_default(),
                        unique: false,
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn explain(&mut self, c: &ContainerRef, q: &Query) -> Result<serde_json::Value> {
        let selector = parse_object(&q.filter, "selector")?;
        let body = serde_json::json!({
            "selector": serde_json::Value::Object(selector),
            "limit": q.limit.max(1),
        });
        self.post(&format!("/{}/_explain", c.name), &body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_never_prints_its_password() {
        let cfg = ConnectConfig {
            url: "http://localhost:5984".into(),
            user: "admin".into(),
            password: Some("hunter2".into()),
            ..Default::default()
        };
        assert!(!format!("{cfg:?}").contains("hunter2"));
    }

    #[test]
    fn a_document_id_is_escaped_for_the_url() {
        assert_eq!(encode_id("order-1"), "order-1");
        assert_eq!(encode_id("a b"), "a%20b");
        assert_eq!(encode_id("a/b"), "a%2Fb");
        // A design document keeps the slash that makes it one.
        assert_eq!(encode_id("_design/views"), "_design/views");
    }

    #[test]
    fn the_id_and_rev_come_off_a_returned_document() {
        let v = serde_json::json!({"_id": "a", "_rev": "1-xyz", "n": 1});
        assert_eq!(id_and_rev(&v), ("a".into(), Some("1-xyz".into())));
        let v = serde_json::json!({"n": 1});
        assert_eq!(id_and_rev(&v), (String::new(), None));
    }
}
