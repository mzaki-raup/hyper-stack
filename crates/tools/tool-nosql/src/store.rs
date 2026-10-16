//! The `DocStore` abstraction of docs/tools/nosql.md §5: one trait over the
//! document stores, so the UI shows containers, runs the store's own query
//! language and edits documents the same way whichever is behind it.
//!
//! Documents travel as JSON. MongoDB's types survive the trip as **Extended
//! JSON** (`{"$oid": …}`, `{"$date": …}`), which is what the editor shows and
//! parses back, so an ObjectId stays an ObjectId after a save.

use std::time::Duration;

use async_trait::async_trait;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StoreKind {
    Mongo,
    Couch,
}

impl StoreKind {
    /// The connection kind id this store reads.
    pub fn connection_kind(self) -> &'static str {
        match self {
            StoreKind::Mongo => "mongodb",
            StoreKind::Couch => "couchdb",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            StoreKind::Mongo => "MongoDB",
            StoreKind::Couch => "CouchDB",
        }
    }

    /// What the query box holds for this store.
    pub fn query_label(self) -> &'static str {
        match self {
            StoreKind::Mongo => "filter",
            StoreKind::Couch => "selector",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    /// The store's own code ("NamespaceNotFound", "not_found", "409").
    pub code: Option<String>,
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self {
            message: m.into(),
            code: None,
        }
    }

    pub fn coded(code: impl Into<String>, m: impl Into<String>) -> Self {
        Self {
            message: m.into(),
            code: Some(code.into()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.code {
            Some(c) => write!(f, "{c}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a container is called in the store it comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerKind {
    Collection,
    View,
    /// A CouchDB database: it holds documents directly.
    Database,
}

/// One database (Mongo) or one database's worth of documents (Couch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub database: String,
}

/// A container the UI can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub name: String,
    pub kind: ContainerKind,
    /// Documents, exact or estimated depending on the store.
    pub count: Option<u64>,
    pub size_bytes: Option<u64>,
}

/// Where a document lives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct ContainerRef {
    pub database: String,
    pub name: String,
}

impl ContainerRef {
    pub fn new(database: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            database: database.into(),
            name: name.into(),
        }
    }

    /// "shop.orders" for Mongo, "orders" for Couch (whose database is the
    /// container).
    pub fn display(&self) -> String {
        if self.database.is_empty() || self.database == self.name {
            self.name.clone()
        } else {
            format!("{}.{}", self.database, self.name)
        }
    }
}

/// One document, with the identity the store needs to write it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// Extended JSON for Mongo, plain JSON for Couch.
    pub json: serde_json::Value,
    /// `_id` rendered for display and for addressing the document again.
    pub id: String,
    /// CouchDB's `_rev`; `None` elsewhere.
    pub rev: Option<String>,
}

impl Document {
    pub fn new(json: serde_json::Value, id: impl Into<String>, rev: Option<String>) -> Self {
        Self {
            json,
            id: id.into(),
            rev,
        }
    }
}

/// The query a `Find` runs, as the user typed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Mongo filter or Couch Mango selector, as JSON text. Empty means all.
    pub filter: String,
    /// Mongo projection / Couch `fields`, as JSON text.
    pub projection: String,
    /// Mongo sort / Couch `sort`, as JSON text.
    pub sort: String,
    pub limit: i64,
    pub skip: i64,
}

impl Query {
    pub fn with_limit(limit: i64) -> Self {
        Self {
            limit,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FindSummary {
    pub returned: usize,
    pub elapsed: Duration,
    /// The limit cut the result short, so there is probably more.
    pub more: bool,
}

/// How a write is applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutMode {
    Insert,
    /// Replace the whole document; `rev` is CouchDB's conflict check.
    Replace {
        rev: Option<String>,
    },
}

/// What a bulk change would do, before it does it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BulkSummary {
    pub matched: u64,
    pub changed: u64,
    /// Nothing was written: this was the count that the dialog shows.
    pub dry_run: bool,
}

/// One index on a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    /// "{ createdAt: -1 }" or the Mango definition.
    pub definition: String,
    pub unique: bool,
}

/// A future that opens a store: the connection is made on the runtime, and
/// the tool hands this to the runner.
pub type Opening = std::pin::Pin<
    Box<dyn std::future::Future<Output = std::result::Result<Box<dyn DocStore>, String>> + Send>,
>;

#[async_trait]
pub trait DocStore: Send {
    fn kind(&self) -> StoreKind;

    /// Server version, for the toolbar.
    fn version(&self) -> &str;

    /// Databases, for the tree's first level.
    async fn databases(&mut self) -> Result<Vec<String>>;

    /// Containers inside one database.
    async fn containers(&mut self, scope: &Scope) -> Result<Vec<ContainerInfo>>;

    /// Run a query and return the documents it matched.
    async fn find(&mut self, c: &ContainerRef, q: &Query) -> Result<(Vec<Document>, FindSummary)>;

    /// Count what a filter matches, which can be expensive and is asked for
    /// separately.
    async fn count(&mut self, c: &ContainerRef, filter: &str) -> Result<u64>;

    /// Write one document.
    async fn put(&mut self, c: &ContainerRef, doc: &Document, mode: PutMode) -> Result<Document>;

    /// Delete one document.
    async fn delete(&mut self, c: &ContainerRef, doc: &Document) -> Result<()>;

    /// Delete everything a filter matches; `dry_run` only counts.
    async fn delete_many(
        &mut self,
        c: &ContainerRef,
        filter: &str,
        dry_run: bool,
    ) -> Result<BulkSummary>;

    /// The container's indexes.
    async fn indexes(&mut self, c: &ContainerRef) -> Result<Vec<IndexInfo>>;

    /// An aggregation pipeline (Mongo) or a Mango explain (Couch), as JSON
    /// text. `None` when the store has no such thing.
    async fn aggregate(
        &mut self,
        _c: &ContainerRef,
        _pipeline: &str,
        _limit: i64,
    ) -> Result<Vec<Document>> {
        Err(Error::msg("this store has no aggregation pipeline"))
    }

    /// The plan for a query, rendered as JSON.
    async fn explain(&mut self, _c: &ContainerRef, _q: &Query) -> Result<serde_json::Value> {
        Err(Error::msg("this store cannot explain a query"))
    }
}

/// Parse a JSON object the user typed, with a clear message when it is not
/// one. An empty box is an empty object, which every store reads as "all".
pub fn parse_object(text: &str, what: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(serde_json::Map::new());
    }
    match serde_json::from_str::<serde_json::Value>(t) {
        Ok(serde_json::Value::Object(m)) => Ok(m),
        Ok(_) => Err(Error::msg(format!("the {what} must be a JSON object"))),
        Err(e) => Err(Error::msg(format!("the {what} is not valid JSON: {e}"))),
    }
}

/// Parse a JSON array (an aggregation pipeline).
pub fn parse_array(text: &str, what: &str) -> Result<Vec<serde_json::Value>> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(Vec::new());
    }
    match serde_json::from_str::<serde_json::Value>(t) {
        Ok(serde_json::Value::Array(a)) => Ok(a),
        Ok(_) => Err(Error::msg(format!("the {what} must be a JSON array"))),
        Err(e) => Err(Error::msg(format!("the {what} is not valid JSON: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_container_reads_as_database_dot_name_where_that_makes_sense() {
        assert_eq!(ContainerRef::new("shop", "orders").display(), "shop.orders");
        // CouchDB's database is the container, so it is not repeated.
        assert_eq!(ContainerRef::new("orders", "orders").display(), "orders");
        assert_eq!(ContainerRef::new("", "orders").display(), "orders");
    }

    #[test]
    fn an_empty_query_box_means_everything() {
        assert!(parse_object("", "filter").unwrap().is_empty());
        assert!(parse_object("   ", "filter").unwrap().is_empty());
        assert!(parse_array("", "pipeline").unwrap().is_empty());
    }

    #[test]
    fn a_broken_filter_says_what_is_wrong_with_it() {
        let e = parse_object("{oops", "filter").unwrap_err();
        assert!(e.message.contains("not valid JSON"), "{}", e.message);
        let e = parse_object("[1, 2]", "filter").unwrap_err();
        assert!(e.message.contains("must be a JSON object"), "{}", e.message);
        let e = parse_array("{}", "pipeline").unwrap_err();
        assert!(e.message.contains("must be a JSON array"), "{}", e.message);
        assert_eq!(
            parse_object(r#"{"status":"failed"}"#, "filter")
                .unwrap()
                .get("status")
                .and_then(|v| v.as_str()),
            Some("failed")
        );
    }

    #[test]
    fn store_kinds_name_their_connection_and_their_query_box() {
        assert_eq!(StoreKind::Mongo.connection_kind(), "mongodb");
        assert_eq!(StoreKind::Couch.connection_kind(), "couchdb");
        assert_eq!(StoreKind::Mongo.query_label(), "filter");
        assert_eq!(StoreKind::Couch.query_label(), "selector");
        assert_eq!(StoreKind::Couch.label(), "CouchDB");
    }
}
