//! The MongoDB driver of docs/tools/nosql.md §2: containers from the server's
//! own listings, queries as the user typed them, and documents that keep
//! their BSON types by travelling as Extended JSON.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::StreamExt;
use mongodb::bson::{Bson, Document as BsonDoc, doc};
use mongodb::options::{ClientOptions, Credential, ServerAddress};
use mongodb::{Client, Collection};

use crate::store::{
    BulkSummary, ContainerInfo, ContainerKind, ContainerRef, DocStore, Document, Error,
    FindSummary, IndexInfo, PutMode, Query, Result, Scope, StoreKind,
};

/// Everything needed to open a session, with secrets already resolved and any
/// SSH tunnel already in place.
#[derive(Clone, Default)]
pub struct ConnectConfig {
    /// A full `mongodb://` or `mongodb+srv://` URI. When set it wins.
    pub uri: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    /// The database that holds the user ("admin" by default).
    pub auth_source: String,
    /// The database the tree opens on.
    pub database: String,
    pub tls: bool,
    /// Talk to this one server rather than discovering a replica set.
    pub direct: bool,
    pub read_only: bool,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for ConnectConfig {
    /// Never prints the password, nor a URI that may carry one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("database", &self.database)
            .field("tls", &self.tls)
            .field("direct", &self.direct)
            .field("read_only", &self.read_only)
            .field("uri", &if self.uri.is_empty() { "" } else { "<set>" })
            .finish_non_exhaustive()
    }
}

impl From<mongodb::error::Error> for Error {
    fn from(e: mongodb::error::Error) -> Self {
        // The server's own label is more use than the whole chain.
        let code = match &*e.kind {
            mongodb::error::ErrorKind::Command(c) => Some(c.code_name.clone()),
            _ => None,
        };
        Error {
            message: e.to_string(),
            code: code.filter(|c| !c.is_empty()),
        }
    }
}

pub struct MongoStore {
    client: Client,
    version: String,
}

impl MongoStore {
    pub async fn connect(cfg: &ConnectConfig) -> Result<MongoStore> {
        let mut options = if cfg.uri.trim().is_empty() {
            let mut o = ClientOptions::default();
            o.hosts = vec![ServerAddress::Tcp {
                host: cfg.host.clone(),
                port: Some(cfg.port),
            }];
            if cfg.tls {
                o.tls = Some(mongodb::options::Tls::Enabled(
                    mongodb::options::TlsOptions::default(),
                ));
            }
            if !cfg.user.is_empty() {
                o.credential = Some(
                    Credential::builder()
                        .username(cfg.user.clone())
                        .password(cfg.password.clone())
                        .source(Some(
                            Some(cfg.auth_source.clone())
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| "admin".into()),
                        ))
                        .build(),
                );
            }
            o
        } else {
            ClientOptions::parse(&cfg.uri).await?
        };
        options.connect_timeout = Some(cfg.connect_timeout);
        options.server_selection_timeout = Some(cfg.connect_timeout);
        options.app_name = Some("HyperStack".into());
        if cfg.direct {
            options.direct_connection = Some(true);
        }
        let client = Client::with_options(options)?;
        // A ping proves the server is there before the UI says "ready".
        let reply = client
            .database("admin")
            .run_command(doc! { "buildInfo": 1 })
            .await?;
        let version = reply
            .get_str("version")
            .map(str::to_string)
            .unwrap_or_default();
        Ok(MongoStore { client, version })
    }

    fn collection(&self, c: &ContainerRef) -> Collection<BsonDoc> {
        self.client.database(&c.database).collection(&c.name)
    }
}

/// Parse a JSON object the user typed into BSON, reading Extended JSON so
/// `{"_id": {"$oid": "…"}}` addresses a document by its real type.
pub fn json_to_bson_doc(text: &str, what: &str) -> Result<BsonDoc> {
    let map = crate::store::parse_object(text, what)?;
    let value = serde_json::Value::Object(map);
    match Bson::try_from(value) {
        Ok(Bson::Document(d)) => Ok(d),
        Ok(_) => Err(Error::msg(format!("the {what} must be a JSON object"))),
        Err(e) => Err(Error::msg(format!("the {what} is not valid: {e}"))),
    }
}

/// A whole document the editor holds, as BSON.
pub fn value_to_bson_doc(value: &serde_json::Value) -> Result<BsonDoc> {
    match Bson::try_from(value.clone()) {
        Ok(Bson::Document(d)) => Ok(d),
        Ok(_) => Err(Error::msg("a document must be a JSON object")),
        Err(e) => Err(Error::msg(format!("this is not a valid document: {e}"))),
    }
}

/// BSON out to the Extended JSON the editor shows.
pub fn bson_doc_to_json(d: BsonDoc) -> serde_json::Value {
    Bson::Document(d).into_relaxed_extjson()
}

/// How an `_id` is shown: the bare string or number where that is what it is,
/// and the Extended JSON form otherwise.
pub fn id_display(id: &Bson) -> String {
    match id {
        Bson::String(s) => s.clone(),
        Bson::ObjectId(o) => o.to_hex(),
        Bson::Int32(i) => i.to_string(),
        Bson::Int64(i) => i.to_string(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

fn to_document(d: BsonDoc) -> Document {
    let id = d.get("_id").map(id_display).unwrap_or_default();
    Document::new(bson_doc_to_json(d), id, None)
}

#[async_trait]
impl DocStore for MongoStore {
    fn kind(&self) -> StoreKind {
        StoreKind::Mongo
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn databases(&mut self) -> Result<Vec<String>> {
        Ok(self.client.list_database_names().await?)
    }

    async fn containers(&mut self, scope: &Scope) -> Result<Vec<ContainerInfo>> {
        let db = self.client.database(&scope.database);
        let mut cursor = db.list_collections().await?;
        let mut out = Vec::new();
        while let Some(spec) = cursor.next().await {
            let spec = spec?;
            let kind = match spec.collection_type {
                mongodb::results::CollectionType::View => ContainerKind::View,
                _ => ContainerKind::Collection,
            };
            // An estimate: an exact count of a large collection is a scan.
            let count = db
                .collection::<BsonDoc>(&spec.name)
                .estimated_document_count()
                .await
                .ok();
            out.push(ContainerInfo {
                name: spec.name,
                kind,
                count,
                size_bytes: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    async fn find(&mut self, c: &ContainerRef, q: &Query) -> Result<(Vec<Document>, FindSummary)> {
        let filter = json_to_bson_doc(&q.filter, "filter")?;
        let projection = json_to_bson_doc(&q.projection, "projection")?;
        let sort = json_to_bson_doc(&q.sort, "sort")?;
        let started = Instant::now();
        let limit = q.limit.max(1);
        // The collection is bound first: the builder borrows it.
        let coll = self.collection(c);
        let mut find = coll.find(filter);
        if !projection.is_empty() {
            find = find.projection(projection);
        }
        if !sort.is_empty() {
            find = find.sort(sort);
        }
        if q.skip > 0 {
            find = find.skip(q.skip as u64);
        }
        // One more than asked, so the view can say there is another page.
        let mut cursor = find.limit(limit + 1).await?;
        let mut docs = Vec::new();
        while let Some(d) = cursor.next().await {
            docs.push(to_document(d?));
            if docs.len() as i64 > limit {
                break;
            }
        }
        let more = docs.len() as i64 > limit;
        docs.truncate(limit as usize);
        Ok((
            docs.clone(),
            FindSummary {
                returned: docs.len(),
                elapsed: started.elapsed(),
                more,
            },
        ))
    }

    async fn count(&mut self, c: &ContainerRef, filter: &str) -> Result<u64> {
        let filter = json_to_bson_doc(filter, "filter")?;
        Ok(self.collection(c).count_documents(filter).await?)
    }

    async fn put(&mut self, c: &ContainerRef, doc: &Document, mode: PutMode) -> Result<Document> {
        let bson = value_to_bson_doc(&doc.json)?;
        let coll = self.collection(c);
        match mode {
            PutMode::Insert => {
                let r = coll.insert_one(&bson).await?;
                let mut saved = bson.clone();
                saved.insert("_id", r.inserted_id.clone());
                Ok(to_document(saved))
            }
            PutMode::Replace { .. } => {
                let id = bson
                    .get("_id")
                    .cloned()
                    .ok_or_else(|| Error::msg("this document has no _id to replace"))?;
                coll.replace_one(doc! { "_id": id }, &bson).await?;
                Ok(to_document(bson))
            }
        }
    }

    async fn delete(&mut self, c: &ContainerRef, doc: &Document) -> Result<()> {
        let bson = value_to_bson_doc(&doc.json)?;
        let id = bson
            .get("_id")
            .cloned()
            .ok_or_else(|| Error::msg("this document has no _id to delete"))?;
        self.collection(c).delete_one(doc! { "_id": id }).await?;
        Ok(())
    }

    async fn delete_many(
        &mut self,
        c: &ContainerRef,
        filter: &str,
        dry_run: bool,
    ) -> Result<BulkSummary> {
        let f = json_to_bson_doc(filter, "filter")?;
        let coll = self.collection(c);
        let matched = coll.count_documents(f.clone()).await?;
        if dry_run {
            return Ok(BulkSummary {
                matched,
                changed: 0,
                dry_run: true,
            });
        }
        let r = coll.delete_many(f).await?;
        Ok(BulkSummary {
            matched,
            changed: r.deleted_count,
            dry_run: false,
        })
    }

    async fn indexes(&mut self, c: &ContainerRef) -> Result<Vec<IndexInfo>> {
        let mut cursor = self.collection(c).list_indexes().await?;
        let mut out = Vec::new();
        while let Some(i) = cursor.next().await {
            let i = i?;
            let name = i
                .options
                .as_ref()
                .and_then(|o| o.name.clone())
                .unwrap_or_default();
            let unique = i.options.as_ref().and_then(|o| o.unique).unwrap_or(false);
            out.push(IndexInfo {
                name,
                definition: Bson::Document(i.keys).into_relaxed_extjson().to_string(),
                unique,
            });
        }
        Ok(out)
    }

    async fn aggregate(
        &mut self,
        c: &ContainerRef,
        pipeline: &str,
        limit: i64,
    ) -> Result<Vec<Document>> {
        let stages = crate::store::parse_array(pipeline, "pipeline")?;
        let mut docs: Vec<BsonDoc> = Vec::new();
        for s in stages {
            match Bson::try_from(s) {
                Ok(Bson::Document(d)) => docs.push(d),
                _ => return Err(Error::msg("every pipeline stage must be a JSON object")),
            }
        }
        // The preview never asks the server for more than it will show.
        docs.push(doc! { "$limit": limit.max(1) });
        let mut cursor = self.collection(c).aggregate(docs).await?;
        let mut out = Vec::new();
        while let Some(d) = cursor.next().await {
            out.push(to_document(d?));
        }
        Ok(out)
    }

    async fn explain(&mut self, c: &ContainerRef, q: &Query) -> Result<serde_json::Value> {
        let filter = json_to_bson_doc(&q.filter, "filter")?;
        let sort = json_to_bson_doc(&q.sort, "sort")?;
        let mut find = doc! { "find": c.name.clone(), "filter": filter };
        if !sort.is_empty() {
            find.insert("sort", sort);
        }
        if q.limit > 0 {
            find.insert("limit", q.limit);
        }
        let reply = self
            .client
            .database(&c.database)
            .run_command(doc! { "explain": find, "verbosity": "executionStats" })
            .await?;
        Ok(bson_doc_to_json(reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_never_prints_its_password_or_uri() {
        let cfg = ConnectConfig {
            password: Some("hunter2".into()),
            uri: "mongodb://user:hunter2@host".into(),
            ..Default::default()
        };
        let shown = format!("{cfg:?}");
        assert!(!shown.contains("hunter2"));
        assert!(shown.contains("<set>"));
    }

    #[test]
    fn extended_json_round_trips_through_bson() {
        // An ObjectId survives the trip as an ObjectId, not as a sub-document.
        let text = r#"{"_id": {"$oid": "68e8a1b2c3d4e5f607182930"}, "n": 2}"#;
        let d = json_to_bson_doc(text, "document").unwrap();
        assert!(matches!(d.get("_id"), Some(Bson::ObjectId(_))));
        let back = bson_doc_to_json(d);
        assert_eq!(
            back.pointer("/_id/$oid").and_then(|v| v.as_str()),
            Some("68e8a1b2c3d4e5f607182930")
        );
        assert_eq!(back.pointer("/n").and_then(|v| v.as_i64()), Some(2));
    }

    #[test]
    fn a_filter_that_is_not_an_object_is_refused() {
        assert!(json_to_bson_doc("[1,2]", "filter").is_err());
        assert!(json_to_bson_doc("{oops", "filter").is_err());
        // An empty box means no filter at all.
        assert!(json_to_bson_doc("  ", "filter").unwrap().is_empty());
    }

    #[test]
    fn an_id_is_shown_the_way_it_is_written() {
        assert_eq!(id_display(&Bson::String("abc".into())), "abc");
        assert_eq!(id_display(&Bson::Int32(7)), "7");
        let oid = mongodb::bson::oid::ObjectId::parse_str("68e8a1b2c3d4e5f607182930").unwrap();
        assert_eq!(id_display(&Bson::ObjectId(oid)), "68e8a1b2c3d4e5f607182930");
        // Anything else keeps its Extended JSON form rather than guessing.
        assert!(id_display(&Bson::Boolean(true)).contains("true"));
    }

    #[test]
    fn a_document_takes_its_id_from_the_underscore_field() {
        let d = doc! { "_id": "order-1", "total": 12.5 };
        let out = to_document(d);
        assert_eq!(out.id, "order-1");
        assert_eq!(out.rev, None);
        assert_eq!(
            out.json.pointer("/total").and_then(|v| v.as_f64()),
            Some(12.5)
        );
        // A document without one still renders.
        assert_eq!(to_document(doc! { "x": 1 }).id, "");
    }
}
