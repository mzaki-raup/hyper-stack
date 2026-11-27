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
    API_KEYS_FROM_MINOR, BiProvider, Caps, Card, Column, ColumnKind, Content, DashCard, DashParam,
    DashTab, Dashboard, Database, ExportFormat, FieldMeta, Found, Kind, Mapping, Node, Param,
    ParamKind, Result, SEARCH_PAGE, ServerInfo, Support, Table, TableMeta, Version,
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
        Caps {
            browse: true,
            native_sql: true,
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

    async fn databases(&self) -> Result<Vec<Database>> {
        Ok(databases_from(&self.get("/api/database").await?))
    }

    async fn tables(&self, database: &str) -> Result<Vec<TableMeta>> {
        let v = self
            .get(&format!(
                "/api/database/{}/metadata",
                hs_core::service_http::seg(database)
            ))
            .await?;
        Ok(tables_from(&v))
    }

    async fn run_sql(&self, database: &str, sql: &str, limit: usize) -> Result<Table> {
        let body = native_body(database, sql)?;
        let rb = self
            .http
            .request_as(Method::POST, "/api/dataset", &self.auth())
            .json(&body);
        let started = std::time::Instant::now();
        let v = self.http.json(rb).await?;
        let mut table = table_from(&v, limit)?;
        table.millis = started.elapsed().as_millis() as u64;
        Ok(table)
    }

    async fn dashboard(&self, id: &str) -> Result<Dashboard> {
        let v = self
            .get(&format!(
                "/api/dashboard/{}",
                hs_core::service_http::seg(id)
            ))
            .await?;
        dashboard_from(&v)
            .ok_or_else(|| "Metabase answered with something that is not a dashboard.".into())
    }

    async fn run_dashcard(
        &self,
        dashboard: &Dashboard,
        card: &DashCard,
        values: &[(String, String)],
        limit: usize,
    ) -> Result<Table> {
        let Content::Card { card: q, mappings } = &card.content else {
            return Err("This tile holds no question.".into());
        };
        let body = json!({ "parameters": dash_params(dashboard, mappings, values)?, "ignore_cache": false });
        let rb = self
            .http
            .request_as(
                Method::POST,
                &format!(
                    "/api/dashboard/{}/dashcard/{}/card/{}/query",
                    hs_core::service_http::seg(&dashboard.id),
                    hs_core::service_http::seg(&card.id),
                    hs_core::service_http::seg(&q.id)
                ),
                &self.auth(),
            )
            .json(&body);
        let started = std::time::Instant::now();
        let v = self.http.json(rb).await?;
        let mut table = table_from(&v, limit)?;
        table.millis = started.elapsed().as_millis() as u64;
        Ok(table)
    }

    async fn filter_values(&self, dashboard: &str, param: &str) -> Result<Vec<String>> {
        let v = self
            .get(&format!(
                "/api/dashboard/{}/params/{}/values",
                hs_core::service_http::seg(dashboard),
                hs_core::service_http::seg(param)
            ))
            .await?;
        Ok(values_from(&v))
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
            // A database's page; a table is `<database>/<table>`.
            Kind::Database => return format!("{base}/browse/databases/{id}"),
            Kind::Table => {
                let (db, table) = id.split_once('/').unwrap_or((id, id));
                return format!("{base}/browse/databases/{db}#table-{table}");
            }
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
        // Not items of a collection: never asked of the search.
        Kind::Database | Kind::Table => "",
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
        let value = filter_value(p.kind, &p.ty, &p.name, text)?;
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

fn id_text(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Number(n) => Some(n.to_string()),
        Value::String(t) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

fn count(v: &Value, k: &str) -> u32 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0) as u32
}

/// `GET /api/dashboard/:id`: the grid, the tabs, the filters and each
/// tile's card, with the dashboard's own overrides of the card's chart
/// settings merged over the card's.
fn dashboard_from(v: &Value) -> Option<Dashboard> {
    let id = id_of(v);
    if id.is_empty() {
        return None;
    }
    let mut tabs: Vec<(u64, DashTab)> = v
        .get("tabs")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    Some((
                        t.get("position").and_then(Value::as_u64).unwrap_or(0),
                        DashTab {
                            id: id_text(t.get("id"))?,
                            name: s(t, "name"),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    tabs.sort_by_key(|(position, _)| *position);
    let params = v
        .get("parameters")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|p| {
                    let ty = s(p, "type");
                    DashParam {
                        id: s(p, "id"),
                        name: [s(p, "name"), s(p, "slug")]
                            .into_iter()
                            .find(|n| !n.is_empty())
                            .unwrap_or_default(),
                        kind: param_kind(&ty),
                        ty,
                        default: default_text(p.get("default")),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let cards = v
        .get("dashcards")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(dashcard_from).collect())
        .unwrap_or_default();
    Some(Dashboard {
        id,
        name: s(v, "name"),
        description: s(v, "description"),
        tabs: tabs.into_iter().map(|(_, t)| t).collect(),
        params,
        cards,
    })
}

fn dashcard_from(v: &Value) -> Option<DashCard> {
    let id = id_text(v.get("id"))?;
    let viz = v
        .get("visualization_settings")
        .cloned()
        .unwrap_or(Value::Null);
    let content = match v
        .get("card")
        .filter(|c| c.get("id").is_some_and(|i| !i.is_null()))
    {
        Some(c) => {
            let mut card = card_from(c)?;
            // The dashboard's settings for this tile win over the card's.
            if let (Value::Object(base), Value::Object(over)) = (&mut card.viz, &viz) {
                for (k, x) in over {
                    base.insert(k.clone(), x.clone());
                }
            } else if card.viz.is_null() {
                card.viz = viz.clone();
            }
            let mappings = v
                .get("parameter_mappings")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|m| Mapping {
                            param_id: s(m, "parameter_id"),
                            target: m.get("target").cloned().unwrap_or(Value::Null),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Content::Card {
                card: Box::new(card),
                mappings,
            }
        }
        // A text, heading or link tile has no question behind it.
        None => {
            let display = viz
                .pointer("/virtual_card/display")
                .and_then(Value::as_str)
                .unwrap_or("text");
            match display {
                "link" => Content::Link {
                    title: viz
                        .pointer("/link/entity/name")
                        .and_then(Value::as_str)
                        .or_else(|| viz.pointer("/link/url").and_then(Value::as_str))
                        .unwrap_or_default()
                        .to_string(),
                    url: viz
                        .pointer("/link/url")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
                other => Content::Text {
                    text: s(&viz, "text"),
                    heading: other == "heading",
                },
            }
        }
    };
    Some(DashCard {
        id,
        tab: id_text(v.get("dashboard_tab_id")),
        row: count(v, "row"),
        col: count(v, "col"),
        size_x: count(v, "size_x").max(1),
        size_y: count(v, "size_y").max(1),
        content,
    })
}

/// One filter's value in the shape its type takes (as a question's).
fn filter_value(kind: ParamKind, ty: &str, name: &str, text: &str) -> Result<Value> {
    Ok(match kind {
        ParamKind::Date if ty == "date/single" => json!(text),
        ParamKind::Date => json!([text]),
        ParamKind::Number => {
            let n: f64 = text
                .parse()
                .map_err(|_| format!("{name} must be a number."))?;
            if n.fract() == 0.0 && n.abs() < 9e15 {
                json!([n as i64])
            } else {
                json!([n])
            }
        }
        ParamKind::Text => json!([text]),
    })
}

/// The `parameters` of one tile's request: each filter that has a value
/// and reaches this tile, with the field of the tile it writes to.
fn dash_params(
    dashboard: &Dashboard,
    mappings: &[Mapping],
    values: &[(String, String)],
) -> Result<Value> {
    let mut out = Vec::new();
    for p in &dashboard.params {
        let text = values
            .iter()
            .find(|(id, _)| *id == p.id)
            .map(|(_, v)| v.trim())
            .unwrap_or_default();
        let Some(m) = mappings.iter().find(|m| m.param_id == p.id) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        out.push(json!({
            "id": p.id, "type": p.ty, "target": m.target,
            "value": filter_value(p.kind, &p.ty, &p.name, text)?,
        }));
    }
    Ok(Value::Array(out))
}

/// `{ "values": [["a"], ["b"]] }`: the first cell of each row.
fn values_from(v: &Value) -> Vec<String> {
    v.get("values")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|r| match r {
                    Value::Array(cells) => cells.first(),
                    other => Some(other),
                })
                .filter_map(|c| match c {
                    Value::String(t) => Some(t.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GET /api/database`: `{ "data": [..] }` (0.40 and later) or a bare list.
/// A database the account has no native rights on says so.
fn databases_from(v: &Value) -> Vec<Database> {
    let list = v.get("data").unwrap_or(v);
    let mut out: Vec<Database> = list
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    Some(Database {
                        id: id_text(d.get("id"))?,
                        name: s(d, "name"),
                        engine: s(d, "engine"),
                        native: d.get("native_permissions").and_then(Value::as_str)
                            == Some("write"),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|d| d.name.to_lowercase());
    out
}

/// `GET /api/database/:id/metadata`: the tables and their fields. A hidden
/// table (`visibility_type` set) is left out, as Metabase's own browser does.
fn tables_from(v: &Value) -> Vec<TableMeta> {
    let mut out: Vec<TableMeta> = v
        .get("tables")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|t| t.get("visibility_type").is_none_or(Value::is_null))
                .filter_map(|t| {
                    Some(TableMeta {
                        id: id_text(t.get("id"))?,
                        schema: s(t, "schema"),
                        name: s(t, "name"),
                        fields: t
                            .get("fields")
                            .and_then(Value::as_array)
                            .map(|f| {
                                f.iter()
                                    .filter(|x| {
                                        x.get("visibility_type").and_then(Value::as_str)
                                            != Some("sensitive")
                                    })
                                    .map(|x| FieldMeta {
                                        name: s(x, "name"),
                                        ty: [s(x, "base_type"), s(x, "database_type")]
                                            .into_iter()
                                            .find(|t| !t.is_empty())
                                            .unwrap_or_default(),
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|t| (t.schema.to_lowercase(), t.name.to_lowercase()));
    out
}

/// The body of `POST /api/dataset` for ad-hoc SQL. An empty statement is
/// refused here, in words, rather than sent.
fn native_body(database: &str, sql: &str) -> Result<Value> {
    if sql.trim().is_empty() {
        return Err("Type a SQL statement first.".into());
    }
    let id: u64 = database
        .parse()
        .map_err(|_| "That is not a database of this server.".to_string())?;
    Ok(json!({ "database": id, "type": "native", "native": { "query": sql } }))
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

    /// A dashboard as `GET /api/dashboard/:id` answers it (trimmed).
    fn dashboard_json() -> Value {
        json!({
            "id": 2, "name": "Overview", "description": "Sales",
            "tabs": [ { "id": 5, "name": "Detail", "position": 1 },
                      { "id": 4, "name": "Overview", "position": 0 } ],
            "parameters": [ { "id": "category", "name": "Category", "slug": "category",
                              "type": "string/=", "default": ["Gadget"] } ],
            "dashcards": [
                { "id": 36, "card_id": 39, "dashboard_tab_id": 4, "row": 0, "col": 6,
                  "size_x": 18, "size_y": 6, "parameter_mappings": [],
                  "visualization_settings": { "graph.metrics": ["count"] },
                  "card": { "id": 39, "name": "Orders per month", "display": "line",
                            "visualization_settings": { "graph.dimensions": ["CREATED_AT"] },
                            "dataset_query": { "type": "query" } } },
                { "id": 37, "card_id": 41, "dashboard_tab_id": 4, "row": 6, "col": 0,
                  "size_x": 12, "size_y": 6,
                  "parameter_mappings": [ { "parameter_id": "category", "card_id": 41,
                      "target": ["dimension", ["field", 18, { "source-field": 14 }]] } ],
                  "visualization_settings": {},
                  "card": { "id": 41, "name": "By category", "display": "bar" } },
                { "id": 40, "card_id": null, "dashboard_tab_id": 4, "row": 12, "col": 0,
                  "size_x": 24, "size_y": 1, "card": null, "parameter_mappings": [],
                  "visualization_settings": { "virtual_card": { "display": "heading" },
                                              "text": "Sales" } },
                { "id": 41, "card_id": null, "dashboard_tab_id": 5, "row": 0, "col": 0,
                  "size_x": 12, "size_y": 2, "card": null,
                  "visualization_settings": { "virtual_card": { "display": "text" },
                                              "text": "Some **notes**" } },
                { "id": 42, "card_id": null, "dashboard_tab_id": 5, "row": 2, "col": 0,
                  "size_x": 6, "size_y": 1, "card": null,
                  "visualization_settings": { "virtual_card": { "display": "link" },
                                              "link": { "url": "https://example.com" } } }
            ]
        })
    }

    #[test]
    fn a_dashboard_has_its_tabs_in_order_its_filters_and_every_kind_of_tile() {
        let d = dashboard_from(&dashboard_json()).expect("a dashboard");
        assert_eq!(d.id, "2");
        assert_eq!(
            d.tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Overview", "Detail"],
            "by position, not as listed"
        );
        assert_eq!(d.params.len(), 1);
        assert_eq!(d.params[0].default, "Gadget");
        assert_eq!(d.cards.len(), 5);
        let kinds: Vec<&str> = d
            .cards
            .iter()
            .map(|c| match &c.content {
                Content::Card { .. } => "card",
                Content::Text { heading: true, .. } => "heading",
                Content::Text { .. } => "text",
                Content::Link { .. } => "link",
            })
            .collect();
        assert_eq!(kinds, ["card", "card", "heading", "text", "link"]);
        assert_eq!(d.cards[0].tab.as_deref(), Some("4"));
        assert_eq!((d.cards[0].col, d.cards[0].size_x), (6, 18));
        assert!(
            matches!(&d.cards[4].content, Content::Link { url, .. } if url == "https://example.com")
        );
    }

    #[test]
    fn a_tiles_own_settings_win_over_its_cards() {
        let d = dashboard_from(&dashboard_json()).expect("a dashboard");
        let Content::Card { card, .. } = &d.cards[0].content else {
            panic!("a card");
        };
        assert_eq!(
            card.viz["graph.dimensions"],
            json!(["CREATED_AT"]),
            "the card's stays"
        );
        assert_eq!(
            card.viz["graph.metrics"],
            json!(["count"]),
            "the dashboard's is added"
        );
    }

    #[test]
    fn a_filter_reaches_only_the_tiles_it_is_mapped_to() {
        let d = dashboard_from(&dashboard_json()).expect("a dashboard");
        let Content::Card { mappings: none, .. } = &d.cards[0].content else {
            panic!("a card");
        };
        let Content::Card { mappings, .. } = &d.cards[1].content else {
            panic!("a card");
        };
        let values = [("category".to_string(), "Gadget".to_string())];
        assert_eq!(dash_params(&d, none, &values).expect("params"), json!([]));
        let p = dash_params(&d, mappings, &values).expect("params");
        assert_eq!(p[0]["id"], "category");
        assert_eq!(p[0]["value"], json!(["Gadget"]));
        assert_eq!(
            p[0]["target"],
            json!(["dimension", ["field", 18, { "source-field": 14 }]])
        );
        // An empty filter is left out.
        let empty = [("category".to_string(), "  ".to_string())];
        assert_eq!(
            dash_params(&d, mappings, &empty).expect("params"),
            json!([])
        );
    }

    #[test]
    fn a_filters_values_are_the_first_cell_of_each_row() {
        let v = json!({ "values": [["Doohickey"], ["Gadget"], [7]], "has_more_values": false });
        assert_eq!(values_from(&v), ["Doohickey", "Gadget", "7"]);
        assert!(values_from(&json!({})).is_empty());
    }

    #[test]
    fn databases_say_whether_the_account_may_run_sql_and_sort_by_name() {
        let v = json!({ "data": [
            { "id": 3, "name": "warehouse", "engine": "postgres", "native_permissions": "none" },
            { "id": 1, "name": "Sample Database", "engine": "h2", "native_permissions": "write" } ] });
        let d = databases_from(&v);
        assert_eq!(
            d.iter()
                .map(|x| (x.name.as_str(), x.native))
                .collect::<Vec<_>>(),
            [("Sample Database", true), ("warehouse", false)]
        );
        assert_eq!(d[0].id, "1");
        // The older shape: a bare list.
        assert_eq!(databases_from(&json!([{ "id": 2, "name": "x" }])).len(), 1);
        assert!(databases_from(&json!({})).is_empty());
    }

    #[test]
    fn tables_come_with_their_fields_hidden_ones_left_out() {
        let v = json!({ "tables": [
            { "id": 9, "schema": "PUBLIC", "name": "ORDERS", "visibility_type": null,
              "fields": [ { "name": "ID", "base_type": "type/BigInteger" },
                          { "name": "SECRET", "base_type": "type/Text", "visibility_type": "sensitive" },
                          { "name": "TOTAL", "database_type": "DOUBLE PRECISION" } ] },
            { "id": 4, "schema": "PUBLIC", "name": "HIDDEN", "visibility_type": "hidden", "fields": [] },
            { "id": 2, "schema": "PUBLIC", "name": "ACCOUNTS", "fields": [] } ] });
        let t = tables_from(&v);
        assert_eq!(
            t.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["ACCOUNTS", "ORDERS"]
        );
        let orders = &t[1];
        assert_eq!(
            orders
                .fields
                .iter()
                .map(|f| (f.name.as_str(), f.ty.as_str()))
                .collect::<Vec<_>>(),
            [("ID", "type/BigInteger"), ("TOTAL", "DOUBLE PRECISION")]
        );
    }

    #[test]
    fn ad_hoc_sql_is_a_native_dataset_request() {
        let b = native_body("1", "SELECT 1").expect("a body");
        assert_eq!(
            b,
            json!({ "database": 1, "type": "native", "native": { "query": "SELECT 1" } })
        );
        assert!(
            native_body("1", "   \n ")
                .expect_err("empty")
                .contains("SQL")
        );
        assert!(
            native_body("sample", "SELECT 1").is_err(),
            "a database id is a number"
        );
    }
}
