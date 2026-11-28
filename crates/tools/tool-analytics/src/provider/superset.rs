//! Apache Superset through its REST API v1 (docs/tools/analytics.md §4.11,
//! §5.5), read-only. `POST /api/v1/security/login` gives an access token and
//! a refresh token (the access token lives 15 minutes by default: a refused
//! request is retried once after a refresh), or an access token is pasted in.
//! Charts are drawn natively from `GET /chart/{id}/data/` through the same
//! chart code as Metabase's; a dashboard is laid out from its `position_json`
//! on Superset's 12-column grid (drawn on this tab's 24); SQL Lab runs a
//! statement that is a read.

use std::collections::HashMap;

use async_trait::async_trait;
use hs_core::service_auth::{Auth, TokenAs};
use hs_core::service_http::{Http, s, seg};
use parking_lot::RwLock;
use reqwest::Method;
use serde_json::{Value, json};

use super::{
    BiProvider, Caps, Card, Column, ColumnKind, Content, DashCard, DashParam, DashTab, Dashboard,
    Database, ExportFormat, FieldMeta, Found, Kind, Mapping, Node, ParamKind, Result, SEARCH_PAGE,
    ServerInfo, Support, Table, TableMeta,
};
use crate::profile::SupersetConfig;

/// A page of a list, and the most pages one list is read through.
const PAGE: usize = 100;
const MAX_PAGES: usize = 20;

/// Datasets whose columns are read when a database is opened.
const COLUMNS_FOR_TABLES: usize = 30;

/// The collection ids the root's two lists go by.
const DASHBOARDS: &str = "dashboards";
const CHARTS: &str = "charts";

#[derive(Default)]
struct Tokens {
    access: Option<String>,
    refresh: Option<String>,
}

pub struct Superset {
    cfg: SupersetConfig,
    http: Http,
    tokens: RwLock<Tokens>,
}

impl Superset {
    pub fn new(cfg: SupersetConfig) -> Result<Superset> {
        let http = Http::with_tls(&cfg.url, Auth::None, TokenAs::Bearer, cfg.timeout, &cfg.tls)?
            .with_header("Accept", "application/json");
        Ok(Superset {
            cfg,
            http,
            tokens: RwLock::new(Tokens::default()),
        })
    }

    fn bearer(&self) -> Auth {
        match self.tokens.read().access.clone() {
            Some(token) => Auth::Token {
                user: String::new(),
                token,
            },
            None => Auth::None,
        }
    }

    async fn login(&self) -> Result<()> {
        match &self.cfg.auth {
            Auth::Password { user, password } => {
                let rb = self
                    .http
                    .request_as(Method::POST, "/api/v1/security/login", &Auth::None)
                    .json(&json!({
                        "username": user, "password": password,
                        "provider": self.cfg.provider, "refresh": true }));
                let v = self.http.json(rb).await.map_err(|e| {
                    format!(
                        "Superset refused the sign-in: {e}. Check the provider (db or ldap); a server that signs people in through OAuth or SSO takes an access token instead."
                    )
                })?;
                let access = s(&v, "access_token");
                if access.is_empty() {
                    return Err("Superset signed in but gave no access token.".into());
                }
                let mut t = self.tokens.write();
                t.access = Some(access);
                t.refresh = Some(s(&v, "refresh_token")).filter(|r| !r.is_empty());
                Ok(())
            }
            Auth::Token { token, .. } => {
                self.tokens.write().access = Some(token.clone());
                Ok(())
            }
            _ => Err("Choose how to sign in.".into()),
        }
    }

    /// A new access token from the refresh token; false when there is none.
    async fn refresh(&self) -> bool {
        let Some(r) = self.tokens.read().refresh.clone() else {
            return false;
        };
        let rb = self
            .http
            .request_as(
                Method::POST,
                "/api/v1/security/refresh",
                &Auth::Token {
                    user: String::new(),
                    token: r,
                },
            )
            .body("");
        match self.http.json(rb).await {
            Ok(v) if !s(&v, "access_token").is_empty() => {
                self.tokens.write().access = Some(s(&v, "access_token"));
                true
            }
            _ => false,
        }
    }

    /// One request as the signed-in account; refused once, it is retried
    /// after the token is renewed.
    async fn call(&self, method: Method, path: &str, body: Option<&Value>) -> Result<Value> {
        for attempt in 0..2 {
            let mut rb = self.http.request_as(method.clone(), path, &self.bearer());
            if let Some(b) = body {
                rb = rb.json(b);
            }
            match self.http.json(rb).await {
                Err(e) if attempt == 0 && e.contains("(401)") && self.refresh().await => continue,
                other => return other,
            }
        }
        Err("Superset refused the renewed token.".into())
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.call(Method::GET, path, None).await
    }

    /// Every row of a `/api/v1/<list>/` endpoint, `PAGE` at a time. `filters`
    /// are Superset's `{col, opr, value}` objects.
    async fn list(&self, endpoint: &str, order: &str, filters: Vec<Value>) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for page in 0..MAX_PAGES {
            let q = json!({ "page": page, "page_size": PAGE, "order_column": order,
                "order_direction": "asc", "filters": filters });
            let v = self
                .get(&format!("/api/v1/{endpoint}/?q={}", seg(&q.to_string())))
                .await?;
            let items = v
                .get("result")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let got = items.len();
            out.extend(items);
            let count = v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
            if got == 0 || out.len() >= count {
                break;
            }
        }
        Ok(out)
    }

    /// A chart's saved query context, run with `filters` added (a read).
    async fn run_context(&self, context: &Value, filters: &[Value]) -> Result<Value> {
        let mut c = context.clone();
        if let Some(qs) = c.get_mut("queries").and_then(Value::as_array_mut) {
            for q in qs {
                let list = q
                    .as_object_mut()
                    .map(|o| o.entry("filters").or_insert_with(|| json!([])));
                if let Some(Value::Array(a)) = list {
                    a.extend(filters.iter().cloned());
                }
            }
        }
        self.call(Method::POST, "/api/v1/chart/data", Some(&c))
            .await
    }
}

#[async_trait]
impl BiProvider for Superset {
    async fn connect(&self) -> Result<ServerInfo> {
        self.login().await?;
        // One cheap read says whether the token works at all.
        self.get(&format!(
            "/api/v1/dashboard/?q={}",
            seg(r#"{"page_size":1}"#)
        ))
        .await?;
        let name = match &self.cfg.auth {
            Auth::Password { user, .. } => user.clone(),
            _ => "access token".to_string(),
        };
        Ok(ServerInfo {
            product: "Apache Superset".into(),
            version_text: String::new(),
            version: None,
            signed_in_as: name,
            support: Support::Unknown,
        })
    }

    fn capabilities(&self) -> Caps {
        Caps {
            browse: true,
            native_sql: true,
            views: false,
            extracts: false,
            search_kinds: &[Kind::Dashboard, Kind::Question],
            tables: true,
            data_section: "analytics-section-databases",
            jobs: false,
            view_data: false,
            view_files: &[],
        }
    }

    async fn children(&self, parent: &str) -> Result<Vec<Node>> {
        match parent {
            "root" => Ok(vec![
                section_node(DASHBOARDS, "Dashboards"),
                section_node(CHARTS, "Charts"),
            ]),
            DASHBOARDS => Ok(self
                .list("dashboard", "dashboard_title", Vec::new())
                .await?
                .iter()
                .filter_map(dashboard_node)
                .collect()),
            CHARTS => Ok(self
                .list("chart", "slice_name", Vec::new())
                .await?
                .iter()
                .filter_map(chart_node)
                .collect()),
            _ => Ok(Vec::new()),
        }
    }

    async fn search(&self, query: &str, kinds: &[Kind], offset: usize) -> Result<Found> {
        let wanted = |k: Kind| kinds.is_empty() || kinds.contains(&k);
        let page = offset / SEARCH_PAGE;
        let one = |col: &str| {
            let q = json!({ "page": page, "page_size": SEARCH_PAGE,
                "filters": [{ "col": col, "opr": "ct", "value": query }] });
            seg(&q.to_string())
        };
        let mut nodes = Vec::new();
        let mut total = 0;
        if wanted(Kind::Dashboard) {
            let v = self
                .get(&format!("/api/v1/dashboard/?q={}", one("dashboard_title")))
                .await?;
            total += v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
            nodes.extend(
                v.get("result")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(dashboard_node),
            );
        }
        if wanted(Kind::Question) {
            let v = self
                .get(&format!("/api/v1/chart/?q={}", one("slice_name")))
                .await?;
            total += v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
            nodes.extend(
                v.get("result")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(chart_node),
            );
        }
        Ok(Found { nodes, total })
    }

    fn item_url(&self, kind: Kind, id: &str) -> String {
        let base = &self.cfg.url;
        match kind {
            Kind::Dashboard => format!("{base}/superset/dashboard/{}/", seg(id)),
            Kind::Question => format!("{base}/explore/?slice_id={}", seg(id)),
            _ => base.clone(),
        }
    }

    fn web_url(&self) -> String {
        self.cfg.url.clone()
    }

    async fn card(&self, id: &str) -> Result<Card> {
        let v = self.get(&format!("/api/v1/chart/{}", seg(id))).await?;
        let mut card = card_from(&v["result"])
            .ok_or_else(|| "Superset answered with something that is not a chart.".to_string())?;
        // The database, for Open in Database: the chart's dataset knows it.
        if let Some(ds) = card.viz.get("dataset").and_then(Value::as_str)
            && let Ok(d) = self.get(&format!("/api/v1/dataset/{}", seg(ds))).await
        {
            card.database = d
                .pointer("/result/database/id")
                .and_then(Value::as_u64)
                .map(|n| n.to_string())
                .unwrap_or_default();
        }
        Ok(card)
    }

    async fn run(&self, card: &Card, _values: &[(String, String)], limit: usize) -> Result<Table> {
        let started = std::time::Instant::now();
        let v = self
            .get(&format!("/api/v1/chart/{}/data/", seg(&card.id)))
            .await
            .map_err(no_context_hint)?;
        let mut t = table_from(&v, limit)?;
        t.millis = started.elapsed().as_millis() as u64;
        Ok(t)
    }

    async fn databases(&self) -> Result<Vec<Database>> {
        let list = self.list("database", "database_name", Vec::new()).await?;
        let mut out = Vec::with_capacity(list.len());
        for d in &list {
            let id = s(d, "id");
            if id.is_empty() {
                continue;
            }
            // The address comes from the connection string, whose password
            // Superset masks and this never reads.
            let uri = self
                .get(&format!("/api/v1/database/{}/connection", seg(&id)))
                .await
                .map(|v| s(&v["result"], "sqlalchemy_uri"))
                .unwrap_or_default();
            let (host, port, dbname) = uri_parts(&uri);
            out.push(Database {
                id,
                name: s(d, "database_name"),
                engine: s(d, "backend"),
                native: d
                    .get("expose_in_sqllab")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                host,
                port,
                dbname,
            });
        }
        Ok(out)
    }

    async fn tables(&self, database: &str) -> Result<Vec<TableMeta>> {
        let datasets = self
            .list(
                "dataset",
                "table_name",
                vec![json!({ "col": "database", "opr": "rel_o_m", "value": database })],
            )
            .await?;
        let mut out = Vec::with_capacity(datasets.len());
        for (i, d) in datasets.iter().enumerate() {
            let id = s(d, "id");
            let fields = if i < COLUMNS_FOR_TABLES && !id.is_empty() {
                self.get(&format!("/api/v1/dataset/{}", seg(&id)))
                    .await
                    .map(|v| fields_from(&v["result"]))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
            out.push(TableMeta {
                id,
                schema: s(d, "schema"),
                name: s(d, "table_name"),
                fields,
            });
        }
        out.sort_by_key(|t| t.name.to_lowercase());
        Ok(out)
    }

    async fn run_sql(&self, database: &str, sql: &str, limit: usize) -> Result<Table> {
        read_only_sql(sql)?;
        let id: u64 = database
            .parse()
            .map_err(|_| "That is not a Superset database.".to_string())?;
        let body = json!({
            "database_id": id, "sql": sql, "runAsync": false, "queryLimit": limit,
            "select_as_cta": false, "tab": "HyperStack", "expand_data": true });
        let started = std::time::Instant::now();
        let v = self
            .call(Method::POST, "/api/v1/sqllab/execute/", Some(&body))
            .await?;
        let mut t = sql_table_from(&v, limit);
        t.millis = started.elapsed().as_millis() as u64;
        Ok(t)
    }

    async fn dashboard(&self, id: &str) -> Result<Dashboard> {
        let v = self.get(&format!("/api/v1/dashboard/{}", seg(id))).await?;
        let d = &v["result"];
        let position: Value = serde_json::from_str(&s(d, "position_json")).unwrap_or(Value::Null);
        let metadata: Value = serde_json::from_str(&s(d, "json_metadata")).unwrap_or(Value::Null);
        let (tabs, slots) = layout_of(&position);
        let filters = filters_of(&metadata);
        // Each chart on the dashboard, once.
        let mut charts: HashMap<String, Card> = HashMap::new();
        for slot in &slots {
            if let Item::Chart(cid) = &slot.item
                && !charts.contains_key(cid)
                && let Ok(c) = self.card(cid).await
            {
                charts.insert(cid.clone(), c);
            }
        }
        let cards = slots
            .into_iter()
            .filter_map(|slot| {
                let content = match slot.item {
                    Item::Chart(cid) => {
                        let card = charts.get(&cid)?.clone();
                        let dataset = card
                            .viz
                            .get("dataset")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        let mappings = filters
                            .iter()
                            .filter(|f| f.dataset == dataset)
                            .map(|f| Mapping {
                                param_id: f.param.id.clone(),
                                target: json!({ "column": f.column }),
                            })
                            .collect();
                        Content::Card {
                            card: Box::new(card),
                            mappings,
                        }
                    }
                    Item::Markdown(text) => Content::Text {
                        text,
                        heading: false,
                    },
                    Item::Header(text) => Content::Text {
                        text,
                        heading: true,
                    },
                };
                Some(DashCard {
                    id: slot.id,
                    tab: slot.tab,
                    row: slot.row,
                    col: slot.col,
                    size_x: slot.w,
                    size_y: slot.h,
                    content,
                })
            })
            .collect();
        Ok(Dashboard {
            id: id.to_string(),
            name: s(d, "dashboard_title"),
            description: String::new(),
            tabs,
            params: filters.into_iter().map(|f| f.param).collect(),
            cards,
        })
    }

    async fn run_dashcard(
        &self,
        _dashboard: &Dashboard,
        card: &DashCard,
        values: &[(String, String)],
        limit: usize,
    ) -> Result<Table> {
        let Content::Card {
            card: chart,
            mappings,
        } = &card.content
        else {
            return Err("This tile has nothing to run.".into());
        };
        let started = std::time::Instant::now();
        // The filters that reach this chart and have a value.
        let applied: Vec<Value> = mappings
            .iter()
            .filter_map(|m| {
                let v = values
                    .iter()
                    .find(|(id, v)| *id == m.param_id && !v.trim().is_empty())?
                    .1
                    .trim();
                let col = m.target.get("column").and_then(Value::as_str)?;
                Some(json!({ "col": col, "op": "IN", "val": [v] }))
            })
            .collect();
        let v = match (applied.is_empty(), chart.viz.get("query_context")) {
            (false, Some(ctx)) => self.run_context(ctx, &applied).await?,
            _ => self
                .get(&format!("/api/v1/chart/{}/data/", seg(&chart.id)))
                .await
                .map_err(no_context_hint)?,
        };
        let mut t = table_from(&v, limit)?;
        t.millis = started.elapsed().as_millis() as u64;
        Ok(t)
    }

    async fn filter_values(&self, dashboard: &str, param: &str) -> Result<Vec<String>> {
        let v = self
            .get(&format!("/api/v1/dashboard/{}", seg(dashboard)))
            .await?;
        let metadata: Value =
            serde_json::from_str(&s(&v["result"], "json_metadata")).unwrap_or(Value::Null);
        let f = filters_of(&metadata)
            .into_iter()
            .find(|f| f.param.id == param)
            .ok_or_else(|| "That filter is not on the dashboard.".to_string())?;
        let ds: u64 = f
            .dataset
            .parse()
            .map_err(|_| "That filter has no dataset.".to_string())?;
        // The distinct values: a group-by with a count, most common first.
        let count =
            json!({ "expressionType": "SQL", "sqlExpression": "COUNT(*)", "label": "count" });
        let ctx = json!({
            "datasource": { "id": ds, "type": "table" },
            "queries": [{ "columns": [f.column], "metrics": [count],
                "orderby": [[count, false]], "row_limit": 500, "filters": [] }],
            "result_format": "json", "result_type": "full" });
        let r = self
            .call(Method::POST, "/api/v1/chart/data", Some(&ctx))
            .await?;
        let t = table_from(&r, 500)?;
        Ok(t.rows
            .iter()
            .filter_map(|row| row.first())
            .map(|v| match v {
                Value::String(t) => t.clone(),
                other => other.to_string(),
            })
            .collect())
    }

    async fn export(
        &self,
        card: &Card,
        values: &[(String, String)],
        format: ExportFormat,
    ) -> Result<Vec<u8>> {
        let t = self.run(card, values, usize::MAX).await?;
        match format {
            ExportFormat::Csv => csv_of(&t),
            ExportFormat::Json => json_of(&t),
            ExportFormat::Xlsx => {
                Err("Excel export is not offered for Superset: save CSV or JSON.".into())
            }
        }
    }

    async fn disconnect(&self) {
        *self.tokens.write() = Tokens::default();
    }
}

// ---- parsers: JSON in, the screens' types out -------------------------------

fn section_node(id: &str, name: &str) -> Node {
    Node {
        kind: Kind::Collection,
        id: id.to_string(),
        name: name.to_string(),
        description: String::new(),
        collection: String::new(),
        edited: String::new(),
        editor: String::new(),
    }
}

fn dashboard_node(v: &Value) -> Option<Node> {
    let id = s(v, "id");
    (!id.is_empty()).then(|| Node {
        kind: Kind::Dashboard,
        id,
        name: s(v, "dashboard_title"),
        description: String::new(),
        collection: String::new(),
        edited: s(v, "changed_on_delta_humanized"),
        editor: s(&v["changed_by"], "last_name"),
    })
}

fn chart_node(v: &Value) -> Option<Node> {
    let id = s(v, "id");
    (!id.is_empty()).then(|| Node {
        kind: Kind::Question,
        id,
        name: s(v, "slice_name"),
        description: s(v, "datasource_name_text"),
        collection: String::new(),
        edited: s(v, "changed_on_delta_humanized"),
        editor: s(&v["changed_by"], "last_name"),
    })
}

/// The Metabase display word the chart code reads, for a Superset `viz_type`;
/// a kind with no drawing here is passed on as it is and shows its table.
fn display_of(viz_type: &str) -> String {
    match viz_type {
        "echarts_timeseries_line" | "line" | "echarts_timeseries_smooth" => "line",
        "echarts_area" | "area" | "echarts_timeseries_step" => "area",
        "echarts_timeseries_bar" | "bar" | "dist_bar" | "bar_v2" => "bar",
        "echarts_timeseries_scatter" | "scatter" => "scatter",
        "pie" => "pie",
        "big_number_total" => "scalar",
        "big_number" => "smartscalar",
        "gauge_chart" => "gauge",
        "table" | "pivot_table_v2" | "pivot_table" | "ag-grid-table" => "table",
        other => other,
    }
    .to_string()
}

/// A chart as a card. Its query context and dataset ride in `viz`, which the
/// chart code ignores for what it does not know.
fn card_from(v: &Value) -> Option<Card> {
    let id = s(v, "id");
    if id.is_empty() {
        return None;
    }
    let viz_type = s(v, "viz_type");
    let context: Value = serde_json::from_str(&s(v, "query_context")).unwrap_or(Value::Null);
    let mut viz = serde_json::Map::new();
    viz.insert("viz_type".into(), json!(viz_type));
    viz.insert("dataset".into(), json!(dataset_of(v, &context)));
    if !context.is_null() {
        viz.insert("query_context".into(), context);
    }
    Some(Card {
        id,
        kind: Kind::Question,
        name: s(v, "slice_name"),
        description: s(v, "description"),
        display: display_of(&viz_type),
        native_sql: None,
        database: String::new(),
        params: Vec::new(),
        viz: Value::Object(viz),
    })
}

/// The id of the dataset a chart reads: Superset 4.1 leaves `datasource_id`
/// null on `GET /chart/{id}`, so the query context's datasource, then the
/// chart's own params (`"datasource": "3__table"`), say it.
fn dataset_of(chart: &Value, context: &Value) -> String {
    let direct = s(chart, "datasource_id");
    if !direct.is_empty() {
        return direct;
    }
    if let Some(id) = context.pointer("/datasource/id").and_then(Value::as_u64) {
        return id.to_string();
    }
    let params: Value = serde_json::from_str(&s(chart, "params")).unwrap_or(Value::Null);
    s(&params, "datasource")
        .split("__")
        .next()
        .unwrap_or_default()
        .to_string()
}

/// What a failed chart read says when the chart was made over the API and
/// has no saved query context.
fn no_context_hint(e: String) -> String {
    if e.to_lowercase().contains("query context") {
        "This chart has no saved query context. Open it in Superset and save it once, then run it again.".to_string()
    } else {
        e
    }
}

/// Superset's generic types: 0 numeric, 1 string, 2 temporal, 3 boolean.
fn kind_of_generic(ty: i64, sample: Option<&Value>) -> ColumnKind {
    match ty {
        0 => match sample {
            Some(Value::Number(n)) if n.is_i64() || n.is_u64() => ColumnKind::Integer,
            _ => ColumnKind::Decimal,
        },
        1 => ColumnKind::Text,
        2 => ColumnKind::Time,
        3 => ColumnKind::Bool,
        _ => ColumnKind::Other,
    }
}

/// An ISO date and time from epoch milliseconds (UTC), which is how Superset
/// writes a temporal column in a chart's data.
pub fn iso_from_millis(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    let (h, mi, sec) = (rem / 3600, rem % 3600 / 60, rem % 60);
    if h == 0 && mi == 0 && sec == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}:{sec:02}")
    }
}

/// A chart's data answer: the first result's columns and rows.
fn table_from(v: &Value, limit: usize) -> Result<Table> {
    let r = v
        .pointer("/result/0")
        .ok_or_else(|| "Superset answered with no result.".to_string())?;
    if let Some(e) = r
        .get("error")
        .and_then(Value::as_str)
        .filter(|e| !e.is_empty())
    {
        return Err(e.to_string());
    }
    let names: Vec<String> = r
        .get("colnames")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|n| n.as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default();
    let types: Vec<i64> = r
        .get("coltypes")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|t| t.as_i64().unwrap_or(-1)).collect())
        .unwrap_or_default();
    let data = r
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let truncated = data.len() > limit;
    let columns: Vec<Column> = names
        .iter()
        .enumerate()
        .map(|(i, n)| Column {
            name: n.clone(),
            title: n.clone(),
            kind: kind_of_generic(
                types.get(i).copied().unwrap_or(-1),
                data.first().and_then(|row| row.get(n)),
            ),
        })
        .collect();
    let rows = data
        .iter()
        .take(limit)
        .map(|row| {
            columns
                .iter()
                .map(|c| match (row.get(&c.name), c.kind) {
                    (Some(Value::Number(n)), ColumnKind::Time) => n
                        .as_i64()
                        .map_or(Value::Null, |ms| Value::String(iso_from_millis(ms))),
                    (Some(v), _) => v.clone(),
                    (None, _) => Value::Null,
                })
                .collect()
        })
        .collect();
    Ok(Table {
        columns,
        rows,
        sql: Some(s(r, "query")).filter(|q| !q.is_empty()),
        millis: 0,
        truncated,
    })
}

/// SQL Lab's answer: `columns` with names and types, `data` as objects.
fn sql_table_from(v: &Value, limit: usize) -> Table {
    let cols = v
        .get("columns")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let data = v
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let columns: Vec<Column> = cols
        .iter()
        .map(|c| {
            let name = [s(c, "column_name"), s(c, "name")]
                .into_iter()
                .find(|n| !n.is_empty())
                .unwrap_or_default();
            let ty = s(c, "type").to_uppercase();
            let kind = if c.get("is_dttm").and_then(Value::as_bool).unwrap_or(false) {
                ColumnKind::Time
            } else if ty.contains("INT") {
                ColumnKind::Integer
            } else if ["FLOAT", "DOUBLE", "NUMERIC", "DECIMAL", "REAL"]
                .iter()
                .any(|t| ty.contains(t))
            {
                ColumnKind::Decimal
            } else if ty.contains("BOOL") {
                ColumnKind::Bool
            } else if ty.is_empty() {
                ColumnKind::Other
            } else {
                ColumnKind::Text
            };
            Column {
                title: name.clone(),
                name,
                kind,
            }
        })
        .collect();
    let truncated = data.len() > limit;
    let rows = data
        .iter()
        .take(limit)
        .map(|row| {
            columns
                .iter()
                .map(|c| row.get(&c.name).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect();
    Table {
        columns,
        rows,
        sql: None,
        millis: 0,
        truncated,
    }
}

fn fields_from(dataset: &Value) -> Vec<FieldMeta> {
    dataset
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|c| FieldMeta {
                    name: s(c, "column_name"),
                    ty: s(c, "type"),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `(host, port, database)` from a SQLAlchemy URI such as
/// `postgresql://user:XXXXXXXXXX@db.example.com:5432/shop`; the credentials
/// are never kept. A file database (`sqlite:////path`) has no host.
fn uri_parts(uri: &str) -> (String, String, String) {
    let Some((_, rest)) = uri.split_once("://") else {
        return Default::default();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, db) = rest.split_once('/').unwrap_or((rest, ""));
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) => (h, p),
        _ => (hostport, ""),
    };
    (host.to_string(), port.to_string(), db.to_string())
}

/// SQL Lab is read-only here (§6): a statement that is not a read is refused
/// before it is sent. Comments and a trailing semicolon are ignored; more
/// than one statement is refused.
pub fn read_only_sql(sql: &str) -> Result<()> {
    let mut text = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '-' if chars.peek() == Some(&'-') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
                text.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                text.push(' ');
            }
            other => text.push(other),
        }
    }
    let body = text.trim().trim_end_matches(';').trim();
    if body.is_empty() {
        return Err("Write a statement to run.".into());
    }
    if body.contains(';') {
        return Err("Run one statement at a time.".into());
    }
    let first = body
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(
        first.as_str(),
        "select" | "with" | "show" | "describe" | "desc" | "explain" | "values"
    ) {
        Ok(())
    } else {
        Err(format!(
            "SQL Lab here only reads: a statement must start with SELECT, WITH, SHOW, DESCRIBE or EXPLAIN, not {}.",
            first.to_uppercase()
        ))
    }
}

fn csv_of(t: &Table) -> Result<Vec<u8>> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(t.columns.iter().map(|c| c.title.as_str()))
        .map_err(|e| e.to_string())?;
    for row in &t.rows {
        w.write_record(row.iter().map(|v| match v {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }))
        .map_err(|e| e.to_string())?;
    }
    w.into_inner().map_err(|e| e.to_string())
}

fn json_of(t: &Table) -> Result<Vec<u8>> {
    let rows: Vec<Value> = t
        .rows
        .iter()
        .map(|row| {
            Value::Object(
                t.columns
                    .iter()
                    .zip(row)
                    .map(|(c, v)| (c.title.clone(), v.clone()))
                    .collect(),
            )
        })
        .collect();
    serde_json::to_vec_pretty(&rows).map_err(|e| e.to_string())
}

// ---- a dashboard's layout ---------------------------------------------------

/// What a tile holds, before the charts are fetched.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Chart(String),
    Markdown(String),
    Header(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Slot {
    id: String,
    tab: Option<String>,
    row: u32,
    col: u32,
    w: u32,
    h: u32,
    item: Item,
}

/// This tab's row is 56 points, a Superset grid unit 8: pixels per grid row.
const UNIT: u32 = 8;
const ROW_PX: u32 = 56;

/// Superset's default tile: 4 of 12 columns, 50 units high.
const DEFAULT_WIDTH: u32 = 4;
const DEFAULT_HEIGHT: u32 = 50;

/// Tabs and tiles from a dashboard's `position_json`: rows of charts, text
/// and headers, directly under the grid or under a tab. Superset's 12 columns
/// become this tab's 24, and its height units become grid rows.
fn layout_of(position: &Value) -> (Vec<DashTab>, Vec<Slot>) {
    let mut tabs = Vec::new();
    let mut slots = Vec::new();
    let mut cursor: HashMap<Option<String>, u32> = HashMap::new();
    walk(
        position,
        "ROOT_ID",
        None,
        &mut tabs,
        &mut slots,
        &mut cursor,
    );
    (tabs, slots)
}

fn walk(
    position: &Value,
    id: &str,
    tab: Option<&str>,
    tabs: &mut Vec<DashTab>,
    slots: &mut Vec<Slot>,
    cursor: &mut HashMap<Option<String>, u32>,
) {
    let Some(node) = position.get(id) else {
        return;
    };
    let children: Vec<String> = node
        .get("children")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    match node.get("type").and_then(Value::as_str).unwrap_or_default() {
        "TAB" => {
            let name = node
                .pointer("/meta/text")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string();
            tabs.push(DashTab {
                id: id.to_string(),
                name,
            });
            for c in &children {
                walk(position, c, Some(id), tabs, slots, cursor);
            }
        }
        "ROW" => {
            let key = tab.map(str::to_string);
            let top = *cursor.get(&key).unwrap_or(&0);
            let mut col = 0;
            let mut tallest = 0;
            for c in &children {
                let Some(child) = position.get(c.as_str()) else {
                    continue;
                };
                let meta = child.get("meta");
                let width = meta
                    .and_then(|m| m.get("width"))
                    .and_then(Value::as_u64)
                    .map_or(DEFAULT_WIDTH, |w| w as u32)
                    .clamp(1, 12);
                let height = meta
                    .and_then(|m| m.get("height"))
                    .and_then(Value::as_u64)
                    .map_or(DEFAULT_HEIGHT, |h| h as u32);
                let item = match child
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                {
                    "CHART" => meta
                        .and_then(|m| m.get("chartId"))
                        .and_then(|v| {
                            v.as_u64()
                                .map(|n| n.to_string())
                                .or_else(|| v.as_str().map(str::to_string))
                        })
                        .map(Item::Chart),
                    "MARKDOWN" => Some(Item::Markdown(
                        meta.and_then(|m| m.get("code"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )),
                    "HEADER" => Some(Item::Header(
                        meta.and_then(|m| m.get("text"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    )),
                    _ => None,
                };
                let rows = (height * UNIT).div_ceil(ROW_PX).max(1);
                if let Some(item) = item {
                    slots.push(Slot {
                        id: c.clone(),
                        tab: key.clone(),
                        row: top,
                        col: (col * 2).min(23),
                        w: width * 2,
                        h: rows,
                        item,
                    });
                }
                col += width;
                tallest = tallest.max(rows);
            }
            cursor.insert(key, top + tallest);
        }
        // ROOT, GRID, TABS and COLUMN just hold what is below them.
        _ => {
            for c in &children {
                walk(position, c, tab, tabs, slots, cursor);
            }
        }
    }
}

/// A native filter, with where it reaches.
#[derive(Debug, Clone, PartialEq)]
struct NativeFilter {
    param: DashParam,
    dataset: String,
    column: String,
}

/// The select filters of `json_metadata`; a time or range filter is left out.
fn filters_of(metadata: &Value) -> Vec<NativeFilter> {
    metadata
        .get("native_filter_configuration")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter(|f| s(f, "filterType") == "filter_select")
                .filter_map(|f| {
                    let target = f.pointer("/targets/0")?;
                    let column = target.pointer("/column/name")?.as_str()?.to_string();
                    let dataset = s(target, "datasetId");
                    let default = f
                        .pointer("/defaultDataMask/filterState/value")
                        .and_then(Value::as_array)
                        .and_then(|a| a.first())
                        .map(|v| match v {
                            Value::String(t) => t.clone(),
                            other => other.to_string(),
                        })
                        .unwrap_or_default();
                    Some(NativeFilter {
                        param: DashParam {
                            id: s(f, "id"),
                            name: s(f, "name"),
                            ty: "filter_select".into(),
                            kind: ParamKind::Text,
                            default,
                        },
                        dataset,
                        column,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viz_types_map_to_the_chart_codes_words() {
        assert_eq!(display_of("echarts_timeseries_line"), "line");
        assert_eq!(display_of("echarts_timeseries_bar"), "bar");
        assert_eq!(display_of("pie"), "pie");
        assert_eq!(display_of("big_number_total"), "scalar");
        assert_eq!(display_of("big_number"), "smartscalar");
        assert_eq!(display_of("pivot_table_v2"), "table");
        assert_eq!(
            display_of("funnel"),
            "funnel",
            "no drawing: the table shows"
        );
    }

    #[test]
    fn a_chart_is_a_card_with_its_query_context_and_dataset() {
        let v = json!({ "id": 7, "slice_name": "Revenue", "viz_type": "echarts_timeseries_line",
            "datasource_id": 3, "query_context": "{\"queries\":[{}]}" });
        let c = card_from(&v).expect("a card");
        assert_eq!((c.id.as_str(), c.display.as_str()), ("7", "line"));
        assert_eq!(c.viz["dataset"], "3");
        assert!(c.viz["query_context"]["queries"].is_array());
        let none = card_from(
            &json!({ "id": 8, "slice_name": "x", "viz_type": "table", "query_context": null }),
        );
        assert!(none.expect("a card").viz.get("query_context").is_none());
        // Superset 4.1 leaves datasource_id null: the context or the params say.
        let by_context = json!({ "id": 9, "viz_type": "pie", "datasource_id": null,
            "query_context": "{\"datasource\":{\"id\":4,\"type\":\"table\"}}" });
        assert_eq!(card_from(&by_context).unwrap().viz["dataset"], "4");
        let by_params =
            json!({ "id": 10, "viz_type": "pie", "params": "{\"datasource\":\"6__table\"}" });
        assert_eq!(card_from(&by_params).unwrap().viz["dataset"], "6");
        assert!(card_from(&json!({})).is_none());
    }

    #[test]
    fn chart_data_becomes_a_typed_table_with_dates_written_out() {
        let v = json!({ "result": [{
            "colnames": ["month", "region", "Revenue", "n"],
            "coltypes": [2, 1, 0, 0],
            "query": "SELECT 1",
            "data": [
                { "month": 1_767_225_600_000_i64, "region": "East", "Revenue": 120.5, "n": 3 },
                { "month": 1_769_904_000_000_i64, "region": "West", "Revenue": 98.0, "n": 4 }] }] });
        let t = table_from(&v, 100).expect("table");
        assert_eq!(
            t.columns.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [
                ColumnKind::Time,
                ColumnKind::Text,
                ColumnKind::Decimal,
                ColumnKind::Integer
            ]
        );
        assert_eq!(t.rows[0][0], json!("2026-01-01"));
        assert_eq!(t.rows[1][0], json!("2026-02-01"));
        assert_eq!(t.rows[0][2], json!(120.5));
        assert_eq!(t.sql.as_deref(), Some("SELECT 1"));
        assert!(!t.truncated);
        let cut = table_from(&v, 1).expect("table");
        assert_eq!((cut.rows.len(), cut.truncated), (1, true));
        let failed = json!({ "result": [{ "error": "Chart has no query context saved." }] });
        assert!(table_from(&failed, 10).is_err());
        assert!(table_from(&json!({}), 10).is_err());
        assert!(
            no_context_hint("x: Chart has no query context saved".into()).contains("save it once")
        );
    }

    #[test]
    fn milliseconds_are_written_as_dates_and_times() {
        assert_eq!(iso_from_millis(0), "1970-01-01");
        assert_eq!(iso_from_millis(86_399_000), "1970-01-01 23:59:59");
        assert_eq!(iso_from_millis(1_782_864_000_000), "2026-07-01");
        assert_eq!(iso_from_millis(-86_400_000), "1969-12-31");
    }

    #[test]
    fn sql_labs_answer_is_a_table() {
        let v = json!({ "columns": [
            { "column_name": "id", "type": "INTEGER" }, { "column_name": "amount", "type": "FLOAT" },
            { "column_name": "day", "type": "TEXT" }],
            "data": [{ "id": 1, "amount": 5.5, "day": "2026-01-01" }, { "id": 2, "amount": 6.0, "day": "x" }] });
        let t = sql_table_from(&v, 1);
        assert_eq!(
            t.columns.iter().map(|c| c.kind).collect::<Vec<_>>(),
            [ColumnKind::Integer, ColumnKind::Decimal, ColumnKind::Text]
        );
        assert_eq!((t.rows.len(), t.truncated), (1, true));
        assert_eq!(t.rows[0][1], json!(5.5));
    }

    #[test]
    fn sql_lab_refuses_what_is_not_a_read() {
        for ok in [
            "SELECT * FROM sales",
            "  select 1;",
            "WITH a AS (SELECT 1) SELECT * FROM a",
            "-- a comment\nSELECT 1",
            "/* hi */ SHOW TABLES",
            "EXPLAIN SELECT 1",
            "describe sales",
        ] {
            assert!(read_only_sql(ok).is_ok(), "{ok}");
        }
        for bad in [
            "DROP TABLE sales",
            "delete from sales",
            "UPDATE sales SET qty = 0",
            "INSERT INTO sales VALUES (1)",
            "SELECT 1; DROP TABLE sales",
            "-- SELECT\nDROP TABLE sales",
            "/* SELECT */ delete from sales",
            "",
            "   ;  ",
        ] {
            assert!(read_only_sql(bad).is_err(), "{bad}");
        }
        assert!(
            read_only_sql("TRUNCATE sales")
                .unwrap_err()
                .contains("TRUNCATE")
        );
    }

    #[test]
    fn a_connection_string_gives_its_host_port_and_database_and_no_credential() {
        assert_eq!(
            uri_parts("postgresql://user:XXXXXXXXXX@db.example.com:5432/shop?sslmode=require"),
            ("db.example.com".into(), "5432".into(), "shop".into())
        );
        assert_eq!(
            uri_parts("mysql+pymysql://u:XXXXXXXXXX@10.0.0.5/orders"),
            ("10.0.0.5".into(), String::new(), "orders".into())
        );
        assert_eq!(
            uri_parts("sqlite:////app/superset_home/bench.db"),
            (
                String::new(),
                String::new(),
                "/app/superset_home/bench.db".into()
            )
        );
        assert_eq!(uri_parts("nonsense"), Default::default());
    }

    fn position() -> Value {
        json!({
            "ROOT_ID": { "type": "ROOT", "id": "ROOT_ID", "children": ["GRID_ID"] },
            "GRID_ID": { "type": "GRID", "id": "GRID_ID", "children": ["ROW-1", "ROW-2"] },
            "ROW-1": { "type": "ROW", "id": "ROW-1", "children": ["CHART-n", "CHART-l"] },
            "ROW-2": { "type": "ROW", "id": "ROW-2", "children": ["HEAD-1", "MD-1"] },
            "CHART-n": { "type": "CHART", "meta": { "chartId": 5, "width": 3, "height": 20 } },
            "CHART-l": { "type": "CHART", "meta": { "chartId": 6, "width": 9, "height": 40 } },
            "HEAD-1": { "type": "HEADER", "meta": { "text": "Notes", "width": 12 } },
            "MD-1": { "type": "MARKDOWN", "meta": { "code": "hello" } } })
    }

    #[test]
    fn a_dashboard_grid_is_laid_out_on_twenty_four_columns() {
        let (tabs, slots) = layout_of(&position());
        assert!(tabs.is_empty());
        let by = |id: &str| slots.iter().find(|s| s.id == id).expect(id).clone();
        let n = by("CHART-n");
        assert_eq!((n.row, n.col, n.w), (0, 0, 6), "3 of 12 is 6 of 24");
        assert_eq!(n.item, Item::Chart("5".into()));
        let l = by("CHART-l");
        assert_eq!((l.col, l.w), (6, 18));
        assert_eq!(l.h, 6, "40 units of 8 px over 56 px rows");
        // The second row starts under the tallest tile of the first.
        let head = by("HEAD-1");
        assert_eq!(head.row, 6);
        assert_eq!(head.item, Item::Header("Notes".into()));
        let md = by("MD-1");
        assert_eq!(
            (md.col, md.w),
            (23, 8),
            "a tile past the edge is kept inside"
        );
        assert_eq!(md.item, Item::Markdown("hello".into()));
    }

    #[test]
    fn tabs_hold_their_own_rows_from_the_top() {
        let pos = json!({
            "ROOT_ID": { "type": "ROOT", "children": ["TABS-1"] },
            "TABS-1": { "type": "TABS", "children": ["TAB-a", "TAB-b"] },
            "TAB-a": { "type": "TAB", "meta": { "text": "Overview" }, "children": ["ROW-1"] },
            "TAB-b": { "type": "TAB", "meta": { "text": "Detail" }, "children": ["ROW-2"] },
            "ROW-1": { "type": "ROW", "children": ["CHART-1"] },
            "ROW-2": { "type": "ROW", "children": ["CHART-2"] },
            "CHART-1": { "type": "CHART", "meta": { "chartId": 1 } },
            "CHART-2": { "type": "CHART", "meta": { "chartId": 2 } } });
        let (tabs, slots) = layout_of(&pos);
        assert_eq!(
            tabs.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            ["Overview", "Detail"]
        );
        assert_eq!(slots[0].tab.as_deref(), Some("TAB-a"));
        assert_eq!(slots[1].tab.as_deref(), Some("TAB-b"));
        assert_eq!(
            (slots[0].row, slots[1].row),
            (0, 0),
            "each tab starts at the top"
        );
    }

    #[test]
    fn native_select_filters_become_dashboard_filters_with_their_target() {
        let meta = json!({ "native_filter_configuration": [
            { "id": "NATIVE_FILTER-1", "name": "Region", "filterType": "filter_select",
              "targets": [{ "datasetId": 3, "column": { "name": "region" } }],
              "defaultDataMask": { "filterState": { "value": ["East"] } } },
            { "id": "NATIVE_FILTER-2", "name": "Day", "filterType": "filter_time",
              "targets": [{}] }] });
        let f = filters_of(&meta);
        assert_eq!(f.len(), 1, "a time filter is left out");
        assert_eq!(
            (f[0].column.as_str(), f[0].dataset.as_str()),
            ("region", "3")
        );
        assert_eq!(f[0].param.default, "East");
        assert_eq!(f[0].param.name, "Region");
        assert!(filters_of(&json!({})).is_empty());
    }

    #[test]
    fn the_exports_are_csv_and_json_of_the_table() {
        let t = Table {
            columns: vec![
                Column {
                    name: "a".into(),
                    title: "A".into(),
                    kind: ColumnKind::Text,
                },
                Column {
                    name: "n".into(),
                    title: "N".into(),
                    kind: ColumnKind::Integer,
                },
            ],
            rows: vec![vec![json!("x, y"), json!(1)], vec![Value::Null, json!(2)]],
            ..Default::default()
        };
        assert_eq!(
            String::from_utf8(csv_of(&t).unwrap()).unwrap(),
            "A,N\n\"x, y\",1\n,2\n"
        );
        let j: Value = serde_json::from_slice(&json_of(&t).unwrap()).unwrap();
        assert_eq!(j[0]["A"], "x, y");
        assert_eq!(j[1]["N"], 2);
    }
}
