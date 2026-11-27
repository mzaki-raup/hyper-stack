//! Metabase through its REST API (docs/tools/analytics.md §5, §8.3): an
//! API key in `x-api-key`, or a session from `POST /api/session` sent as
//! `X-Metabase-Session`. The session is kept in memory only and ended with
//! `DELETE /api/session` when the tab closes.

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, s};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::{Value, json};

use super::{
    API_KEYS_FROM_MINOR, BiProvider, Caps, Card, Column, ColumnKind, ExportFormat, Found, Kind,
    Node, Param, ParamKind, Result, SEARCH_PAGE, ServerInfo, Support, Table, Version,
};
use crate::profile::MetabaseConfig;

const SESSION_HEADER: &str = "X-Metabase-Session";

pub struct Metabase {
    cfg: MetabaseConfig,
    http: Http,
    /// The session id a username and password gave, while one is open.
    session: RwLock<Option<String>>,
}

impl Metabase {
    pub fn new(cfg: MetabaseConfig) -> Result<Metabase> {
        // The client carries no credential of its own: each request is
        // handed the current one (`request_as`), so a renewed session
        // needs no second client.
        let http = Http::with_tls(&cfg.url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?;
        Ok(Metabase {
            cfg,
            http,
            session: RwLock::new(None),
        })
    }

    /// What goes on the next request.
    fn auth(&self) -> Auth {
        match &self.cfg.auth {
            Auth::ApiKey { .. } => self.cfg.auth.clone(),
            Auth::Password { .. } => match self.session.read().clone() {
                Some(id) => Auth::ApiKey {
                    header: SESSION_HEADER.into(),
                    key: id,
                },
                None => Auth::None,
            },
            _ => Auth::None,
        }
    }

    /// `GET path` as the signed-in account.
    pub async fn get(&self, path: &str) -> Result<Value> {
        self.http
            .json(self.http.request_as(Method::GET, path, &self.auth()))
            .await
    }

    async fn sign_in(&self) -> Result<()> {
        let Auth::Password { user, password } = &self.cfg.auth else {
            return Ok(());
        };
        let rb = self
            .http
            .request_as(Method::POST, "/api/session", &Auth::None)
            .json(&json!({ "username": user, "password": password }));
        let v = self.http.json(rb).await.map_err(|e| {
            format!(
                "Metabase refused the sign-in: {e}. A server that signs people in through single sign-on takes an API key instead."
            )
        })?;
        let id = s(&v, "id");
        if id.is_empty() {
            return Err("Metabase signed in but gave no session.".into());
        }
        *self.session.write() = Some(id);
        Ok(())
    }
}

#[async_trait]
impl BiProvider for Metabase {
    async fn connect(&self) -> Result<ServerInfo> {
        // The version is public: read it before signing in, so an API key
        // sent to a server too old for one is said in words, not as a 401.
        let props = self.http.get_json("/api/session/properties").await?;
        let version_text = props
            .pointer("/version/tag")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let version = Version::parse(&version_text);
        if matches!(self.cfg.auth, Auth::ApiKey { .. })
            && version.is_some_and(|v| v.minor < API_KEYS_FROM_MINOR)
        {
            return Err(format!(
                "This Metabase ({version_text}) is older than 0.{API_KEYS_FROM_MINOR}, which is the first to take an API key. Use a username and password."
            ));
        }
        self.sign_in().await?;
        let me = self.get("/api/user/current").await?;
        let name = [s(&me, "common_name"), s(&me, "email")]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or_default();
        Ok(ServerInfo {
            product: "Metabase".into(),
            version_text,
            support: version.map(Version::support).unwrap_or(Support::Unknown),
            version,
            signed_in_as: name,
        })
    }

    fn capabilities(&self) -> Caps {
        // SQL arrives with FEAT-0130.
        Caps {
            browse: true,
            native_sql: false,
        }
    }

    async fn children(&self, collection: &str) -> Result<Vec<Node>> {
        let path = format!(
            "/api/collection/{}/items?archived=false&limit={ITEMS_LIMIT}",
            hs_core::service_http::seg(collection)
        );
        let mut nodes = nodes_from_items(&self.get(&path).await?);
        if collection == "root" {
            // The account's own collection is not among the root's items.
            let me = self.get("/api/user/current").await?;
            if let Some(id) = me.get("personal_collection_id").and_then(Value::as_u64) {
                let id = id.to_string();
                if !nodes.iter().any(|n| n.id == id) {
                    let c = self.get(&format!("/api/collection/{id}")).await?;
                    nodes.push(Node {
                        kind: Kind::Collection,
                        id,
                        name: s(&c, "name"),
                        description: s(&c, "description"),
                        collection: String::new(),
                        edited: String::new(),
                        editor: String::new(),
                    });
                    sort_nodes(&mut nodes);
                }
            }
        }
        Ok(nodes)
    }

    async fn search(&self, query: &str, kinds: &[Kind], offset: usize) -> Result<Found> {
        let mut path = format!(
            "/api/search?q={}&limit={SEARCH_PAGE}&offset={offset}",
            hs_core::service_http::seg(query)
        );
        for k in kinds {
            path.push_str("&models=");
            path.push_str(model_of(*k));
        }
        Ok(found_from_search(&self.get(&path).await?))
    }

    async fn card(&self, id: &str) -> Result<Card> {
        let v = self
            .get(&format!("/api/card/{}", hs_core::service_http::seg(id)))
            .await?;
        card_from(&v)
            .ok_or_else(|| "Metabase answered with something that is not a question.".into())
    }

    async fn run(&self, card: &Card, values: &[(String, String)], limit: usize) -> Result<Table> {
        let body = json!({ "parameters": param_values(card, values)?, "ignore_cache": false });
        let rb = self
            .http
            .request_as(
                Method::POST,
                &format!("/api/card/{}/query", hs_core::service_http::seg(&card.id)),
                &self.auth(),
            )
            .json(&body);
        let started = std::time::Instant::now();
        let v = self.http.json(rb).await?;
        let mut table = table_from(&v, limit)?;
        table.millis = started.elapsed().as_millis() as u64;
        Ok(table)
    }

    async fn export(
        &self,
        card: &Card,
        values: &[(String, String)],
        format: ExportFormat,
    ) -> Result<Vec<u8>> {
        let parameters = param_values(card, values)?.to_string();
        let rb = self
            .http
            .request_as(
                Method::POST,
                &format!(
                    "/api/card/{}/query/{}",
                    hs_core::service_http::seg(&card.id),
                    format.ext()
                ),
                &self.auth(),
            )
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "parameters={}",
                hs_core::service_http::seg(&parameters)
            ));
        let bytes = self
            .http
            .send(rb)
            .await?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }

    fn item_url(&self, kind: Kind, id: &str) -> String {
        let base = self.cfg.url.trim_end_matches('/');
        let page = match kind {
            Kind::Collection => "collection",
            Kind::Question => "question",
            Kind::Model => "model",
            Kind::Metric => "metric",
            Kind::Dashboard => "dashboard",
        };
        format!("{base}/{page}/{id}")
    }

    fn web_url(&self) -> String {
        self.cfg.url.clone()
    }

    async fn disconnect(&self) {
        let Some(id) = self.session.write().take() else {
            return;
        };
        let rb = self.http.request_as(
            Method::DELETE,
            &format!(
                "/api/session?metabase-session-id={}",
                hs_core::service_http::seg(&id)
            ),
            &Auth::ApiKey {
                header: SESSION_HEADER.into(),
                key: id.clone(),
            },
        );
        // Best effort: the session expires on its own if this fails.
        let _ = self.http.send(rb).await;
    }
}

/// The most entries read from one collection. A collection with more says
/// so through the server's own `total`, which the screens do not yet use.
const ITEMS_LIMIT: usize = 1000;

/// Metabase's `model` word for a kind (`card` is a question).
fn model_of(k: Kind) -> &'static str {
    match k {
        Kind::Collection => "collection",
        Kind::Question => "card",
        Kind::Model => "dataset",
        Kind::Metric => "metric",
        Kind::Dashboard => "dashboard",
    }
}

fn kind_of(model: &str) -> Option<Kind> {
    Kind::ALL.into_iter().find(|k| model_of(*k) == model)
}

/// Collections first, then the rest, each by name (the shared list rule).
fn sort_nodes(nodes: &mut [Node]) {
    nodes.sort_by(|a, b| {
        (a.kind != Kind::Collection)
            .cmp(&(b.kind != Kind::Collection))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// An id the server wrote as a number or as a word (`root`).
fn id_of(v: &Value) -> String {
    match v.get("id") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(t)) => t.clone(),
        _ => String::new(),
    }
}

fn node_from(v: &Value) -> Option<Node> {
    let kind = kind_of(v.get("model")?.as_str()?)?;
    let id = id_of(v);
    if id.is_empty() {
        return None;
    }
    // A collection's items carry `last_edit_info`; a search result carries
    // the editor and the time directly.
    let edit = v.get("last_edit_info");
    let from_edit = |k: &str| edit.map(|e| s(e, k)).unwrap_or_default();
    let editor = [
        from_edit("full_name"),
        s(v, "last_editor_common_name"),
        s(v, "creator_common_name"),
    ]
    .into_iter()
    .find(|t| !t.is_empty())
    .unwrap_or_default();
    let edited = [
        from_edit("timestamp"),
        s(v, "last_edited_at"),
        s(v, "updated_at"),
    ]
    .into_iter()
    .find(|t| !t.is_empty())
    .unwrap_or_default();
    Some(Node {
        kind,
        id,
        name: s(v, "name"),
        description: s(v, "description"),
        collection: v
            .get("collection")
            .map(|c| s(c, "name"))
            .unwrap_or_default(),
        edited,
        editor,
    })
}

/// `/api/collection/:id/items`: `{ "data": [ … ], "total": n }`. Entries
/// of a kind this tab does not show (a table, an action) are skipped.
fn nodes_from_items(v: &Value) -> Vec<Node> {
    let mut nodes: Vec<Node> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(node_from).collect())
        .unwrap_or_default();
    sort_nodes(&mut nodes);
    nodes
}

/// `/api/search`: the same shape, in the server's own relevance order.
fn found_from_search(v: &Value) -> Found {
    let nodes: Vec<Node> = v
        .get("data")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(node_from).collect())
        .unwrap_or_default();
    let total = v
        .get("total")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(nodes.len());
    Found { nodes, total }
}

/// The kind of a card: Metabase 0.50 names it in `type`; older servers
/// mark a model with `dataset: true`.
fn card_kind(v: &Value) -> Kind {
    match v.get("type").and_then(Value::as_str) {
        Some("model") => Kind::Model,
        Some("metric") => Kind::Metric,
        Some(_) => Kind::Question,
        None if v.get("dataset").and_then(Value::as_bool) == Some(true) => Kind::Model,
        None => Kind::Question,
    }
}

fn param_kind(ty: &str) -> ParamKind {
    if ty.starts_with("number") {
        ParamKind::Number
    } else if ty.starts_with("date") {
        ParamKind::Date
    } else {
        ParamKind::Text
    }
}

/// A default the server wrote as text, a number or a list.
fn default_text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(a)) => a.first().map(|x| default_text(Some(x))).unwrap_or_default(),
        _ => String::new(),
    }
}

/// `GET /api/card/:id`.
fn card_from(v: &Value) -> Option<Card> {
    let id = id_of(v);
    if id.is_empty() {
        return None;
    }
    let native = v.pointer("/dataset_query/native/query");
    let native_sql = native.and_then(Value::as_str).map(str::to_string);
    let mut params: Vec<Param> = Vec::new();
    for p in v
        .get("parameters")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let ty = s(p, "type");
        params.push(Param {
            id: s(p, "id"),
            name: [s(p, "name"), s(p, "slug")]
                .into_iter()
                .find(|n| !n.is_empty())
                .unwrap_or_default(),
            kind: param_kind(&ty),
            ty,
            default: default_text(p.get("default")),
            target: p.get("target").cloned().unwrap_or(Value::Null),
        });
    }
    Some(Card {
        id,
        kind: card_kind(v),
        name: s(v, "name"),
        description: s(v, "description"),
        display: s(v, "display"),
        native_sql,
        params,
        viz: v
            .get("visualization_settings")
            .cloned()
            .unwrap_or(Value::Null),
    })
}

/// The request's `parameters`: each parameter that has a value, in the
/// shape its type takes (a list for most, a string for a single date).
fn param_values(card: &Card, values: &[(String, String)]) -> Result<Value> {
    let mut out = Vec::new();
    for p in &card.params {
        let text = values
            .iter()
            .find(|(id, _)| *id == p.id)
            .map(|(_, v)| v.trim())
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        let value = match p.kind {
            ParamKind::Date if p.ty == "date/single" => json!(text),
            ParamKind::Date => json!([text]),
            ParamKind::Number => {
                let n: f64 = text
                    .parse()
                    .map_err(|_| format!("{} must be a number.", p.name))?;
                if n.fract() == 0.0 && n.abs() < 9e15 {
                    json!([n as i64])
                } else {
                    json!([n])
                }
            }
            ParamKind::Text => json!([text]),
        };
        out.push(json!({ "id": p.id, "type": p.ty, "target": p.target, "value": value }));
    }
    Ok(Value::Array(out))
}

fn column_kind(base: &str) -> ColumnKind {
    match base {
        "type/Integer" | "type/BigInteger" => ColumnKind::Integer,
        "type/Float" | "type/Decimal" | "type/Number" => ColumnKind::Decimal,
        "type/Text" | "type/UUID" | "type/URL" | "type/Email" => ColumnKind::Text,
        "type/Date"
        | "type/DateTime"
        | "type/DateTimeWithTZ"
        | "type/DateTimeWithLocalTZ"
        | "type/DateTimeWithZoneOffset"
        | "type/Time"
        | "type/TimeWithTZ" => ColumnKind::Time,
        "type/Boolean" => ColumnKind::Bool,
        other if other.starts_with("type/Text") => ColumnKind::Text,
        _ => ColumnKind::Other,
    }
}

/// `POST /api/card/:id/query`: `{ "data": { "cols": [..], "rows": [..] } }`,
/// or an error as a sentence. At most `limit` rows are kept.
fn table_from(v: &Value, limit: usize) -> Result<Table> {
    if let Some(e) = v.get("error").and_then(Value::as_str)
        && !e.is_empty()
    {
        return Err(e.to_string());
    }
    let data = v.get("data").ok_or("Metabase answered without any data.")?;
    let columns: Vec<Column> = data
        .get("cols")
        .and_then(Value::as_array)
        .map(|cols| {
            cols.iter()
                .map(|c| Column {
                    name: s(c, "name"),
                    title: [s(c, "display_name"), s(c, "name")]
                        .into_iter()
                        .find(|t| !t.is_empty())
                        .unwrap_or_default(),
                    kind: column_kind(&s(c, "base_type")),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut rows: Vec<Vec<Value>> = data
        .get("rows")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .map(|r| r.as_array().cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default();
    let truncated = rows.len() > limit;
    rows.truncate(limit);
    let sql = data
        .pointer("/native_form/query")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Table {
        columns,
        rows,
        sql,
        millis: 0,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A collection's items as Metabase 0.52 answers them (trimmed).
    fn items() -> Value {
        json!({
            "total": 5,
            "data": [
                { "id": 7, "model": "card", "name": "orders per month",
                  "description": "Line", "last_edit_info": { "full_name": "Ada Lovelace",
                  "timestamp": "2026-11-02T10:15:00Z" } },
                { "id": 3, "model": "collection", "name": "Archive stuff" },
                { "id": 9, "model": "dashboard", "name": "Overview" },
                { "id": 11, "model": "table", "name": "ORDERS" },
                { "id": 4, "model": "collection", "name": "demo" },
                { "id": 8, "model": "dataset", "name": "A model" }
            ]
        })
    }

    #[test]
    fn a_collections_items_list_folders_first_then_by_name() {
        let nodes = nodes_from_items(&items());
        let names: Vec<(&str, Kind)> = nodes.iter().map(|n| (n.name.as_str(), n.kind)).collect();
        assert_eq!(
            names,
            [
                ("Archive stuff", Kind::Collection),
                ("demo", Kind::Collection),
                ("A model", Kind::Model),
                ("orders per month", Kind::Question),
                ("Overview", Kind::Dashboard),
            ]
        );
    }

    #[test]
    fn an_item_carries_its_editor_and_time() {
        let nodes = nodes_from_items(&items());
        let q = nodes.iter().find(|n| n.id == "7").expect("the question");
        assert_eq!(q.editor, "Ada Lovelace");
        assert_eq!(q.edited, "2026-11-02T10:15:00Z");
        assert_eq!(q.description, "Line");
    }

    #[test]
    fn a_search_result_keeps_the_servers_order_and_its_collection() {
        let v = json!({
            "total": 120, "limit": 50, "offset": 0,
            "data": [
                { "id": 9, "model": "dashboard", "name": "Overview",
                  "collection": { "id": 4, "name": "HyperStack Demo" },
                  "last_editor_common_name": "Bench Admin", "updated_at": "2026-11-03T08:00:00Z" },
                { "id": 7, "model": "card", "name": "Orders",
                  "collection": { "id": 4, "name": "HyperStack Demo" } },
                { "id": 2, "model": "database", "name": "Sample Database" }
            ]
        });
        let found = found_from_search(&v);
        assert_eq!(found.total, 120);
        assert_eq!(
            found
                .nodes
                .iter()
                .map(|n| n.id.as_str())
                .collect::<Vec<_>>(),
            ["9", "7"]
        );
        assert_eq!(found.nodes[0].collection, "HyperStack Demo");
        assert_eq!(found.nodes[0].editor, "Bench Admin");
    }

    #[test]
    fn an_answer_without_data_is_an_empty_list() {
        assert!(nodes_from_items(&json!({})).is_empty());
        assert_eq!(found_from_search(&json!({ "total": 0 })), Found::default());
    }

    #[test]
    fn every_kind_has_a_model_word_that_maps_back() {
        for k in Kind::ALL {
            assert_eq!(kind_of(model_of(k)), Some(k));
            assert_eq!(Kind::from_tag(k.tag()), Some(k));
        }
    }

    /// A native question as `GET /api/card/:id` answers it (trimmed).
    fn native_card() -> Value {
        json!({
            "id": 39, "name": "Big orders (native)", "description": "Totals over a minimum",
            "display": "table", "type": "question",
            "visualization_settings": { "table.pivot": false },
            "dataset_query": { "type": "native", "database": 1, "native": {
                "query": "SELECT ID, TOTAL FROM ORDERS WHERE TOTAL > {{min_total}}",
                "template-tags": { "min_total": { "name": "min_total", "type": "number" } } } },
            "parameters": [ { "id": "abc-1", "name": "Min total", "slug": "min_total",
                "type": "number/=", "target": ["variable", ["template-tag", "min_total"]],
                "default": 100 } ]
        })
    }

    #[test]
    fn a_card_keeps_its_display_sql_and_parameters() {
        let c = card_from(&native_card()).expect("a card");
        assert_eq!((c.id.as_str(), c.kind), ("39", Kind::Question));
        assert_eq!(c.display, "table");
        assert!(
            c.native_sql
                .as_deref()
                .is_some_and(|q| q.contains("{{min_total}}"))
        );
        assert_eq!(c.params.len(), 1);
        assert_eq!(c.params[0].kind, ParamKind::Number);
        assert_eq!(c.params[0].default, "100");
        assert_eq!(c.params[0].name, "Min total");
    }

    #[test]
    fn a_model_and_an_older_models_flag_are_models() {
        assert_eq!(card_kind(&json!({ "type": "model" })), Kind::Model);
        assert_eq!(card_kind(&json!({ "type": "metric" })), Kind::Metric);
        assert_eq!(card_kind(&json!({ "dataset": true })), Kind::Model);
        assert_eq!(card_kind(&json!({})), Kind::Question);
    }

    #[test]
    fn parameter_values_go_into_the_request_in_their_types_shape() {
        let mut c = card_from(&native_card()).expect("a card");
        let v = param_values(&c, &[("abc-1".into(), " 250 ".into())]).expect("values");
        assert_eq!(v[0]["value"], json!([250]));
        assert_eq!(v[0]["type"], "number/=");
        assert_eq!(
            v[0]["target"],
            json!(["variable", ["template-tag", "min_total"]])
        );
        let half = param_values(&c, &[("abc-1".into(), "2.5".into())]).expect("values");
        assert_eq!(half[0]["value"], json!([2.5]));
        // An empty field is left out; a word where a number belongs is said.
        assert_eq!(
            param_values(&c, &[("abc-1".into(), String::new())]).expect("values"),
            json!([])
        );
        let err = param_values(&c, &[("abc-1".into(), "lots".into())]).expect_err("not a number");
        assert!(err.contains("Min total"), "{err}");
        // A single date is a string, a date range a list.
        c.params[0].kind = ParamKind::Date;
        c.params[0].ty = "date/single".into();
        let d = param_values(&c, &[("abc-1".into(), "2026-01-31".into())]).expect("values");
        assert_eq!(d[0]["value"], json!("2026-01-31"));
    }

    /// A result as the server answers it (trimmed).
    fn result() -> Value {
        json!({ "status": "completed", "data": {
            "cols": [
                { "name": "CREATED_AT", "display_name": "Created At", "base_type": "type/DateTime" },
                { "name": "count", "display_name": "Count", "base_type": "type/BigInteger" },
                { "name": "avg", "display_name": "Average", "base_type": "type/Float" } ],
            "rows": [ ["2026-01-01T00:00:00Z", 12, 3.5], ["2026-02-01T00:00:00Z", 15, null],
                      ["2026-03-01T00:00:00Z", 9, 2.0] ],
            "native_form": { "query": "SELECT 1" } } })
    }

    #[test]
    fn a_result_has_titled_typed_columns_and_its_rows() {
        let t = table_from(&result(), 100).expect("a table");
        let titles: Vec<&str> = t.columns.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(titles, ["Created At", "Count", "Average"]);
        assert_eq!(
            t.columns.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [ColumnKind::Time, ColumnKind::Integer, ColumnKind::Decimal]
        );
        assert_eq!(t.rows.len(), 3);
        assert!(!t.truncated);
        assert_eq!(t.sql.as_deref(), Some("SELECT 1"));
    }

    #[test]
    fn rows_over_the_limit_are_cut_and_said() {
        let t = table_from(&result(), 2).expect("a table");
        assert_eq!(t.rows.len(), 2);
        assert!(t.truncated);
    }

    #[test]
    fn an_error_answer_is_a_sentence_not_an_empty_table() {
        let v = json!({ "status": "failed", "error": "Column \"X\" not found" });
        assert_eq!(
            table_from(&v, 10),
            Err("Column \"X\" not found".to_string())
        );
        assert!(table_from(&json!({}), 10).is_err());
    }
}
