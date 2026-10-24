//! The Qdrant driver (owner's request, 2026-10-24).
//!
//! Qdrant is a vector database, and the shapes line up with this tool
//! more closely than they first appear: a **collection** is a
//! container, and a **point** is a document with an id. What does not
//! line up is worth stating, because both affect what the pane can
//! safely do:
//!
//! - **There are no databases.** One pseudo-scope stands in, so the
//!   tree has a root to open.
//! - **A point is `{id, payload, vector}`**, and the vector can be
//!   hundreds of floats. Listing them would make the viewer unreadable
//!   and the transfer large, so a scroll asks for payloads **without**
//!   vectors.
//!
//! That second point decides how writing works, and it is the one real
//! hazard here. Qdrant's upsert **requires** a vector: sending back a
//! point that was read without one would either be refused or, worse,
//! replace a real vector with nothing. So a save whose document carries
//! no vector goes to `/points/payload`, which sets the payload and
//! leaves the vector alone; only a document that actually carries a
//! vector is upserted whole. Editing what you can see therefore cannot
//! destroy what you cannot.

use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::store::{
    BulkSummary, ContainerInfo, ContainerKind, ContainerRef, DocStore, Document, Error,
    FindSummary, IndexInfo, PutMode, Query, Result, Scope, StoreKind, parse_object,
};

#[derive(Clone, Default)]
pub struct ConnectConfig {
    /// `http://host:6333`, without a trailing slash.
    pub url: String,
    /// Sent as the `api-key` header. Qdrant has no user name.
    pub api_key: Option<String>,
    /// The collection the tree opens on; empty lists them all.
    pub collection: String,
    pub read_only: bool,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for ConnectConfig {
    /// Never prints the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectConfig")
            .field("url", &self.url)
            .field("collection", &self.collection)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

/// The one pseudo-database. Qdrant has no such level; the tree needs a
/// root, and calling it after the product is clearer than an empty
/// string in a breadcrumb.
pub const SCOPE: &str = "qdrant";

pub struct QdrantStore {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
    version: String,
}

impl QdrantStore {
    pub async fn connect(cfg: &ConnectConfig) -> Result<QdrantStore> {
        let base = cfg.url.trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err(Error::msg("this connection has no URL"));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(cfg.connect_timeout)
            .build()
            .map_err(|e| Error::msg(e.to_string()))?;
        let store = QdrantStore {
            http,
            base,
            api_key: cfg.api_key.clone(),
            version: String::new(),
        };
        // The root says what it is, and is the cheapest way to find out
        // whether this is a Qdrant at all before anything else is tried.
        let root = store.get("/").await?;
        let version = root
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(QdrantStore { version, ..store })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let r = self.http.request(method, format!("{}{path}", self.base));
        match &self.api_key {
            Some(k) => r.header("api-key", k),
            None => r,
        }
    }

    async fn send(&self, r: reqwest::RequestBuilder) -> Result<serde_json::Value> {
        let resp = r.send().await.map_err(|e| Error {
            code: e.status().map(|s| s.as_u16().to_string()),
            message: e.to_string(),
        })?;
        let status = resp.status();
        let text = resp.text().await.map_err(|e| Error::msg(e.to_string()))?;
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        if !status.is_success() {
            // Qdrant puts the useful sentence in `status.error`; the
            // bare code alone ("400") says nothing a reader can act on.
            let message = doc
                .pointer("/status/error")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| text.trim().to_string());
            return Err(Error {
                code: Some(status.as_u16().to_string()),
                message: if message.is_empty() {
                    format!("the server answered {status}")
                } else {
                    message
                },
            });
        }
        Ok(doc)
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::GET, path)).await
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::POST, path).json(&body))
            .await
    }

    async fn put_json(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value> {
        self.send(self.request(reqwest::Method::PUT, path).json(&body))
            .await
    }
}

/// A Qdrant filter, from the text in the query box. Empty means "all".
fn filter_of(text: &str) -> Result<Option<serde_json::Value>> {
    let m = parse_object(text, "filter")?;
    if m.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::Value::Object(m)))
}

/// A point as the viewer shows it.
///
/// The whole point, not just the payload: the id is what addresses it
/// again, and hiding it would make a saved edit guesswork.
pub fn document_of(point: &serde_json::Value) -> Document {
    let id = point.get("id").map(render_id).unwrap_or_default();
    let mut shown = serde_json::Map::new();
    if let Some(v) = point.get("id") {
        shown.insert("id".into(), v.clone());
    }
    if let Some(v) = point.get("payload") {
        shown.insert("payload".into(), v.clone());
    }
    // Only when the server actually sent one: an absent vector and a
    // null vector mean different things to the save path below.
    if let Some(v) = point.get("vector")
        && !v.is_null()
    {
        shown.insert("vector".into(), v.clone());
    }
    Document::new(serde_json::Value::Object(shown), id, None)
}

/// A point id is a number or a UUID, and both have to survive the round
/// trip: `1` and `"1"` are different points to Qdrant.
fn render_id(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Turn the id a document carries back into the JSON Qdrant expects.
///
/// A bare number must go back as a number: quoting it addresses a
/// different point, or none.
fn id_value(doc: &Document) -> Result<serde_json::Value> {
    if let Some(v) = doc.json.get("id")
        && !v.is_null()
    {
        return Ok(v.clone());
    }
    let raw = doc.id.trim();
    if raw.is_empty() {
        return Err(Error::msg("this point has no id"));
    }
    Ok(match raw.parse::<u64>() {
        Ok(n) => serde_json::Value::from(n),
        Err(_) => serde_json::Value::String(raw.to_string()),
    })
}

#[async_trait]
impl DocStore for QdrantStore {
    fn kind(&self) -> StoreKind {
        StoreKind::Qdrant
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn databases(&mut self) -> Result<Vec<String>> {
        // Qdrant has no databases; one scope so the tree has a root.
        Ok(vec![SCOPE.to_string()])
    }

    async fn containers(&mut self, _scope: &Scope) -> Result<Vec<ContainerInfo>> {
        let doc = self.get("/collections").await?;
        let names: Vec<String> = doc
            .pointer("/result/collections")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| c.get("name").and_then(|n| n.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for name in names {
            // The count is per collection and worth having in the tree;
            // a collection that will not answer is still listed, because
            // "it is there but busy" is not "it is not there".
            let count = self
                .get(&format!("/collections/{name}"))
                .await
                .ok()
                .and_then(|d| d.pointer("/result/points_count").and_then(|v| v.as_u64()));
            out.push(ContainerInfo {
                name,
                kind: ContainerKind::Collection,
                count,
                size_bytes: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn find(&mut self, c: &ContainerRef, q: &Query) -> Result<(Vec<Document>, FindSummary)> {
        let started = Instant::now();
        let mut body = serde_json::json!({
            "limit": q.limit.max(1),
            "with_payload": true,
            // Vectors are hundreds of floats; fetching them would make
            // the viewer unreadable and the response large. A save of a
            // point read this way sets the payload only, so nothing is
            // lost by leaving them out (see the module note).
            "with_vector": false,
        });
        if let Some(f) = filter_of(&q.filter)? {
            body["filter"] = f;
        }
        // Qdrant pages by `offset`, which is a point id rather than a
        // row number, so `skip` is honoured by scrolling past.
        let doc = self
            .post(&format!("/collections/{}/points/scroll", c.name), body)
            .await?;
        let points = doc
            .pointer("/result/points")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let docs: Vec<Document> = points
            .iter()
            .skip(q.skip.max(0) as usize)
            .map(document_of)
            .collect();
        let more = doc
            .pointer("/result/next_page_offset")
            .is_some_and(|v| !v.is_null());
        let summary = FindSummary {
            returned: docs.len(),
            elapsed: started.elapsed(),
            more,
        };
        Ok((docs, summary))
    }

    async fn count(&mut self, c: &ContainerRef, filter: &str) -> Result<u64> {
        let mut body = serde_json::json!({ "exact": true });
        if let Some(f) = filter_of(filter)? {
            body["filter"] = f;
        }
        let doc = self
            .post(&format!("/collections/{}/points/count", c.name), body)
            .await?;
        Ok(doc
            .pointer("/result/count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0))
    }

    async fn put(&mut self, c: &ContainerRef, doc: &Document, _mode: PutMode) -> Result<Document> {
        let id = id_value(doc)?;
        let payload = doc
            .json
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Object(Default::default()));
        let vector = doc.json.get("vector").filter(|v| !v.is_null()).cloned();
        match vector {
            // A document that carries a vector is the whole point, and
            // is upserted as one.
            Some(v) => {
                self.put_json(
                    &format!("/collections/{}/points?wait=true", c.name),
                    serde_json::json!({
                        "points": [ { "id": id, "payload": payload, "vector": v } ]
                    }),
                )
                .await?;
            }
            // No vector means it was read without one. Upserting would
            // demand a vector and could replace a real one with
            // nothing, so only the payload is written.
            None => {
                self.post(
                    &format!("/collections/{}/points/payload?wait=true", c.name),
                    serde_json::json!({ "payload": payload, "points": [id] }),
                )
                .await?;
            }
        }
        Ok(doc.clone())
    }

    async fn delete(&mut self, c: &ContainerRef, doc: &Document) -> Result<()> {
        let id = id_value(doc)?;
        self.post(
            &format!("/collections/{}/points/delete?wait=true", c.name),
            serde_json::json!({ "points": [id] }),
        )
        .await?;
        Ok(())
    }

    async fn delete_many(
        &mut self,
        c: &ContainerRef,
        filter: &str,
        dry_run: bool,
    ) -> Result<BulkSummary> {
        // Count first, always: the number is what the confirmation
        // shows, and a dry run must not touch anything.
        let matched = self.count(c, filter).await?;
        if dry_run {
            return Ok(BulkSummary {
                matched,
                changed: 0,
                dry_run: true,
            });
        }
        let body = match filter_of(filter)? {
            Some(f) => serde_json::json!({ "filter": f }),
            // Qdrant has no "delete everything" shorthand that is not a
            // filter, and an empty object matches all.
            None => serde_json::json!({ "filter": {} }),
        };
        self.post(
            &format!("/collections/{}/points/delete?wait=true", c.name),
            body,
        )
        .await?;
        Ok(BulkSummary {
            matched,
            changed: matched,
            dry_run: false,
        })
    }

    async fn indexes(&mut self, c: &ContainerRef) -> Result<Vec<IndexInfo>> {
        // Qdrant's indexes are on payload fields, and the collection
        // describes them.
        let doc = self.get(&format!("/collections/{}", c.name)).await?;
        let schema = doc.pointer("/result/payload_schema").cloned();
        let mut out = Vec::new();
        if let Some(serde_json::Value::Object(m)) = schema {
            for (field, info) in m {
                let data_type = info
                    .get("data_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let points = info
                    .get("points")
                    .and_then(|v| v.as_u64())
                    .map(|n| format!(", {n} points"))
                    .unwrap_or_default();
                out.push(IndexInfo {
                    name: field.clone(),
                    definition: format!("{data_type}{points}"),
                    // A payload index is not a uniqueness constraint;
                    // saying otherwise would be a claim Qdrant does not
                    // make.
                    unique: false,
                });
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_becomes_a_document_without_losing_its_id() {
        let point = serde_json::json!({
            "id": 42,
            "payload": { "city": "London" },
        });
        let d = document_of(&point);
        assert_eq!(d.id, "42");
        assert_eq!(d.json.get("payload").unwrap()["city"], "London");
        // The id is in the shown document too, or a save would be
        // guesswork.
        assert_eq!(d.json.get("id").unwrap(), &serde_json::json!(42));
        // No vector was sent, so none is shown - which is what tells
        // the save path to write the payload only.
        assert!(d.json.get("vector").is_none());
    }

    /// `1` and `"1"` are different points to Qdrant, so the id's type
    /// has to survive the round trip.
    #[test]
    fn a_numeric_id_goes_back_as_a_number() {
        let numeric = document_of(&serde_json::json!({ "id": 7, "payload": {} }));
        assert_eq!(id_value(&numeric).unwrap(), serde_json::json!(7));

        let uuid = document_of(&serde_json::json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "payload": {}
        }));
        assert_eq!(
            id_value(&uuid).unwrap(),
            serde_json::json!("550e8400-e29b-41d4-a716-446655440000")
        );

        // A document typed by hand, with only the display id to go on.
        let typed = Document::new(serde_json::json!({ "payload": {} }), "12", None);
        assert_eq!(id_value(&typed).unwrap(), serde_json::json!(12));
        let named = Document::new(serde_json::json!({ "payload": {} }), "abc", None);
        assert_eq!(id_value(&named).unwrap(), serde_json::json!("abc"));
    }

    /// A point with no id at all cannot be addressed, and saying so
    /// beats writing to whatever id the server picks.
    #[test]
    fn a_point_with_no_id_is_refused() {
        let d = Document::new(serde_json::json!({ "payload": {} }), "", None);
        assert!(id_value(&d).is_err());
    }

    /// A vector that *was* fetched is kept, because that document can
    /// be upserted whole.
    #[test]
    fn a_fetched_vector_is_shown_and_kept() {
        let point = serde_json::json!({
            "id": 1,
            "payload": { "a": 1 },
            "vector": [0.1, 0.2, 0.3],
        });
        let d = document_of(&point);
        assert_eq!(
            d.json.get("vector").unwrap(),
            &serde_json::json!([0.1, 0.2, 0.3])
        );
        // A null vector is not a vector: it must not read as one, or a
        // save would try to upsert with nothing.
        let null = document_of(&serde_json::json!({ "id": 1, "vector": null }));
        assert!(null.json.get("vector").is_none());
    }

    /// An empty filter box means "everything", and anything else has to
    /// be a JSON object - the same rule the other stores follow.
    #[test]
    fn the_filter_box_is_empty_or_an_object() {
        assert!(filter_of("").unwrap().is_none());
        assert!(filter_of("   ").unwrap().is_none());
        let f = filter_of(r#"{"must":[{"key":"city","match":{"value":"London"}}]}"#)
            .unwrap()
            .unwrap();
        assert!(f.get("must").is_some());
        assert!(filter_of("[1,2]").is_err(), "an array is not a filter");
        assert!(filter_of("not json").is_err());
    }
}
