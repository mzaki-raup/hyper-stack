//! ClickHouse through its HTTP interface (ADR-030): one POST a statement,
//! rows streamed back a line at a time as
//! `JSONCompactEachRowWithNamesAndTypes` - the names, then the types, then
//! one JSON array per row.
//!
//! There are no transactions to track. Cancel is
//! `KILL QUERY WHERE query_id = …` from a second request; every statement
//! carries a query id of its own for that.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde_json::Value as Json;

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::schema::{ColumnDef, TableSchema};
use crate::sql::quote_literal;

const FORMAT: &str = "JSONCompactEachRowWithNamesAndTypes";

/// The databases ClickHouse keeps for itself.
const SYSTEM_DBS: &str = "('system', 'INFORMATION_SCHEMA', 'information_schema')";

pub struct ChSession {
    http: reqwest::Client,
    /// `http(s)://host:port/`
    base: String,
    database: String,
    user: String,
    password: Option<String>,
    read_only: bool,
    info: ServerInfo,
    /// The query id of the statement running now, for Cancel.
    running: Arc<Mutex<Option<String>>>,
    runtime: tokio::runtime::Handle,
}

/// `Code: 60. DB::Exception: Table default.x does not exist. (UNKNOWN_TABLE)`
/// as a message and a code.
fn ch_err(body: &str) -> DbError {
    let text = body.trim();
    let code = text
        .strip_prefix("Code: ")
        .and_then(|r| r.split('.').next())
        .map(str::to_string);
    let message = text
        .split_once("DB::Exception: ")
        .map(|(_, m)| m)
        .unwrap_or(text)
        .split(" (version ")
        .next()
        .unwrap_or(text)
        .trim()
        .to_string();
    DbError {
        message,
        position: None,
        code,
    }
}

/// The type a column holds once `Nullable(…)` and `LowCardinality(…)` are
/// taken off.
fn base_type(t: &str) -> &str {
    let mut t = t.trim();
    for w in ["Nullable(", "LowCardinality("] {
        while let Some(inner) = t.strip_prefix(w).and_then(|x| x.strip_suffix(')')) {
            t = inner;
        }
    }
    t
}

fn is_numeric(t: &str) -> bool {
    let b = base_type(t);
    ["Int", "UInt", "Float", "Decimal"]
        .iter()
        .any(|p| b.starts_with(p))
}

/// One cell of a row. 64-bit and wider integers and decimals come as JSON
/// strings so nothing is rounded; they stay text, right-aligned.
fn convert(v: &Json, ty: &str) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Text(b.to_string()),
        Json::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => match n.as_f64() {
                Some(f) if base_type(ty).starts_with("Float") => Value::Float(f),
                _ => Value::Number(n.to_string()),
            },
        },
        Json::String(s) if is_numeric(ty) => match s.parse::<i64>() {
            Ok(i) => Value::Int(i),
            Err(_) => Value::Number(s.clone()),
        },
        Json::String(s) => Value::Text(s.clone()),
        // Arrays, tuples and maps: their JSON is the readable form.
        other => Value::Text(other.to_string()),
    }
}

fn new_query_id() -> String {
    format!("hyperstack-{}", hs_core::new_id())
}

impl ChSession {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Self> {
        let tls = super::tls::verifies(&cfg.ssl) || cfg.ssl == "require";
        let scheme = if tls { "https" } else { "http" };
        let http = reqwest::Client::builder()
            .connect_timeout(cfg.connect_timeout)
            .danger_accept_invalid_certs(tls && !super::tls::verifies(&cfg.ssl))
            .build()
            .map_err(|e| DbError::msg(e.to_string()))?;
        let mut s = Self {
            http,
            base: format!("{scheme}://{}:{}/", cfg.host, cfg.port),
            database: cfg.database.clone(),
            user: cfg.user.clone(),
            password: cfg.password.clone(),
            read_only: cfg.read_only,
            info: ServerInfo::default(),
            running: Arc::new(Mutex::new(None)),
            runtime: tokio::runtime::Handle::current(),
        };
        let r = s
            .rows_of("SELECT version(), currentDatabase()")
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        s.info = ServerInfo {
            version: format!("ClickHouse {}", r.first().cloned().unwrap_or_default()),
            database: r.get(1).cloned().unwrap_or_default(),
        };
        Ok(s)
    }

    fn request(&self, sql: &str, query_id: &str) -> reqwest::RequestBuilder {
        let mut q: Vec<(&str, &str)> = vec![("query_id", query_id), ("default_format", FORMAT)];
        if !self.database.is_empty() {
            q.push(("database", &self.database));
        }
        if self.read_only {
            // Reads only; settings may still change (2, not 1, or the
            // format above would be refused).
            q.push(("readonly", "2"));
        }
        let query: Vec<String> = q
            .iter()
            .map(|(k, v)| format!("{k}={}", hs_core::service_http::seg(v)))
            .collect();
        let mut rb = self
            .http
            .post(format!("{}?{}", self.base, query.join("&")))
            .body(sql.to_string());
        if !self.user.is_empty() {
            rb = rb.header("X-ClickHouse-User", &self.user);
        }
        if let Some(p) = &self.password {
            rb = rb.header("X-ClickHouse-Key", p);
        }
        rb
    }

    async fn send(&self, sql: &str, query_id: &str) -> Result<reqwest::Response> {
        let resp = self
            .request(sql, query_id)
            .send()
            .await
            .map_err(|e| DbError::msg(hs_core::service_http::transport_error(&e)))?;
        if resp.status().is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        Err(ch_err(&body))
    }

    /// A whole small result as text, for the catalogue queries.
    async fn rows_of(&self, sql: &str) -> Result<Vec<Vec<String>>> {
        let body = self
            .send(sql, &new_query_id())
            .await?
            .text()
            .await
            .map_err(|e| DbError::msg(e.to_string()))?;
        // Skip the names and the types.
        Ok(body
            .lines()
            .skip(2)
            .filter_map(|l| serde_json::from_str::<Vec<Json>>(l).ok())
            .map(|r| {
                r.into_iter()
                    .map(|v| match v {
                        Json::String(s) => s,
                        Json::Null => String::new(),
                        other => other.to_string(),
                    })
                    .collect()
            })
            .collect())
    }
}

async fn kill(
    http: reqwest::Client,
    base: String,
    user: String,
    password: Option<String>,
    id: String,
) {
    let sql = format!(
        "KILL QUERY WHERE query_id = {} ASYNC",
        quote_literal(&id, Engine::ClickHouse)
    );
    let mut rb = http.post(&base).body(sql);
    if !user.is_empty() {
        rb = rb.header("X-ClickHouse-User", user);
    }
    if let Some(p) = password {
        rb = rb.header("X-ClickHouse-Key", p);
    }
    let _ = rb.send().await;
}

#[async_trait]
impl DbSession for ChSession {
    fn engine(&self) -> Engine {
        Engine::ClickHouse
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let id = new_query_id();
        *self.running.lock() = Some(id.clone());
        let resp = self.send(sql, &id).await;
        let resp = match resp {
            Ok(r) => r,
            Err(e) => {
                *self.running.lock() = None;
                return Err(e);
            }
        };
        let written = resp
            .headers()
            .get("x-clickhouse-summary")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| serde_json::from_str::<Json>(h).ok())
            .and_then(|j| {
                j.get("written_rows")
                    .and_then(|w| w.as_str().map(str::to_string))
            })
            .and_then(|w| w.parse::<u64>().ok());
        let mut summary = ExecSummary::default();
        let mut stream = resp.bytes_stream();
        let mut pending: Vec<u8> = Vec::new();
        let mut names: Option<Vec<String>> = None;
        let mut types: Option<Vec<String>> = None;
        let mut batch: Vec<Row> = Vec::with_capacity(BATCH);
        let mut stopped = false;
        'read: while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| DbError::msg(e.to_string()))?;
            pending.extend_from_slice(&chunk);
            while let Some(nl) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line);
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let Ok(cells) = serde_json::from_str::<Vec<Json>>(line) else {
                    // An error raised after rows had started arrives as text.
                    *self.running.lock() = None;
                    return Err(ch_err(line));
                };
                if names.is_none() {
                    names = Some(
                        cells
                            .iter()
                            .map(|c| c.as_str().unwrap_or("").to_string())
                            .collect(),
                    );
                    continue;
                }
                if types.is_none() {
                    let t: Vec<String> = cells
                        .iter()
                        .map(|c| c.as_str().unwrap_or("").to_string())
                        .collect();
                    summary.has_result = true;
                    sink.columns(
                        names
                            .clone()
                            .unwrap_or_default()
                            .into_iter()
                            .zip(&t)
                            .map(|(name, ty)| Column {
                                name,
                                type_name: ty.clone(),
                            })
                            .collect(),
                    );
                    types = Some(t);
                    continue;
                }
                let t = types.as_deref().unwrap_or(&[]);
                batch.push(
                    cells
                        .iter()
                        .enumerate()
                        .map(|(i, v)| convert(v, t.get(i).map(String::as_str).unwrap_or("")))
                        .collect(),
                );
                summary.rows_returned += 1;
                if batch.len() >= BATCH && sink.rows(std::mem::take(&mut batch)) == Flow::Stop {
                    stopped = true;
                    break 'read;
                }
            }
        }
        if !stopped && !batch.is_empty() && sink.rows(batch) == Flow::Stop {
            stopped = true;
        }
        *self.running.lock() = None;
        if stopped {
            summary.limited = true;
            // Dropping the stream closes the connection; ClickHouse cancels
            // a query whose client went away, and KILL makes sure.
            kill(
                self.http.clone(),
                self.base.clone(),
                self.user.clone(),
                self.password.clone(),
                id,
            )
            .await;
        }
        if !summary.has_result {
            summary.rows_affected = written.filter(|w| *w > 0);
        }
        summary.elapsed = started.elapsed();
        Ok(summary)
    }

    fn canceller(&self) -> Canceller {
        let running = self.running.clone();
        let (http, base, user, password) = (
            self.http.clone(),
            self.base.clone(),
            self.user.clone(),
            self.password.clone(),
        );
        let rt = self.runtime.clone();
        Arc::new(move || {
            if let Some(id) = running.lock().clone() {
                rt.spawn(kill(
                    http.clone(),
                    base.clone(),
                    user.clone(),
                    password.clone(),
                    id,
                ));
            }
        })
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        let e = Engine::ClickHouse;
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(&format!(
                    "SELECT name FROM system.databases WHERE name NOT IN {SYSTEM_DBS} \
                     ORDER BY name = currentDatabase() DESC, name"
                ))
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Schema, r[0].clone(), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let s = quote_literal(schema, e);
                let sql = match kind {
                    ObjectKind::Table => format!(
                        "SELECT name, concat(engine, ' · ~', toString(ifNull(total_rows, 0)), ' rows') \
                         FROM system.tables WHERE database = {s} AND NOT is_temporary \
                         AND engine NOT IN ('View', 'MaterializedView') ORDER BY name"
                    ),
                    ObjectKind::View => format!(
                        "SELECT name, lower(engine) FROM system.tables WHERE database = {s} \
                         AND engine IN ('View', 'MaterializedView') ORDER BY name"
                    ),
                    _ => return Ok(Vec::new()),
                };
                self.rows_of(&sql)
                    .await?
                    .into_iter()
                    .map(|r| o(*kind, r[0].clone(), r[1].clone(), vec![]))
                    .collect()
            }
            Scope::Columns { schema, table } => self
                .rows_of(&format!(
                    "SELECT name, type, is_in_primary_key, is_in_sorting_key FROM system.columns \
                     WHERE database = {} AND table = {} ORDER BY position",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                ))
                .await?
                .into_iter()
                .map(|r| {
                    let mut badges = Vec::new();
                    if r[2] == "1" {
                        badges.push("PK".to_string());
                    } else if r[3] == "1" {
                        badges.push("ORDER BY".to_string());
                    }
                    if !r[1].starts_with("Nullable(") {
                        badges.push("NOT NULL".to_string());
                    }
                    o(ObjectKind::Column, r[0].clone(), r[1].clone(), badges)
                })
                .collect(),
            // Data-skipping indexes: what ClickHouse has instead of B-trees.
            Scope::Indexes { schema, table } => self
                .rows_of(&format!(
                    "SELECT name, concat(type, ' (', expr, ')') FROM system.data_skipping_indices \
                     WHERE database = {} AND table = {} ORDER BY name",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                ))
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Index, r[0].clone(), r[1].clone(), vec![]))
                .collect(),
        })
    }

    async fn catalog(&mut self) -> Result<Catalog> {
        let rows = self
            .rows_of(&format!(
                "SELECT database, table, name FROM system.columns \
                 WHERE database NOT IN {SYSTEM_DBS} ORDER BY database, table, position"
            ))
            .await?;
        Ok(super::postgres::group_catalog(
            rows.into_iter()
                .map(|r| (r[0].clone(), r[1].clone(), r[2].clone())),
        ))
    }

    async fn table_schema(&mut self, schema: &str, table: &str) -> Result<TableSchema> {
        let e = Engine::ClickHouse;
        let (s, t) = (quote_literal(schema, e), quote_literal(table, e));
        let cols = self
            .rows_of(&format!(
                "SELECT name, type, default_kind, default_expression FROM system.columns \
                 WHERE database = {s} AND table = {t} ORDER BY position"
            ))
            .await?;
        let columns = cols
            .iter()
            .map(|r| {
                let ty = r.get(1).cloned().unwrap_or_default();
                // ClickHouse spells nullability in the type; the DDL keeps
                // the type as it is and does not add NOT NULL on top.
                let nullable = ty.starts_with("Nullable(");
                ColumnDef {
                    name: r.first().cloned().unwrap_or_default(),
                    data_type: ty,
                    nullable,
                    default: Some(r.get(3).cloned().unwrap_or_default())
                        .filter(|d| !d.is_empty() && r.get(2).is_some_and(|k| k == "DEFAULT")),
                    auto_increment: false,
                }
            })
            .collect();
        // The primary key is an expression list ("a, b"); simple columns
        // are what a key usually is, and anything else is kept whole.
        let key = self
            .rows_of(&format!(
                "SELECT primary_key FROM system.tables WHERE database = {s} AND name = {t}"
            ))
            .await?
            .into_iter()
            .next()
            .and_then(|r| r.into_iter().next())
            .unwrap_or_default();
        let primary_key = key
            .split(',')
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty())
            .collect();
        Ok(TableSchema {
            schema: schema.to_string(),
            name: table.to_string(),
            columns,
            primary_key,
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
        })
    }

    fn in_transaction(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_error_reads_as_a_message_and_a_code() {
        let e = ch_err(
            "Code: 60. DB::Exception: Table default.nope does not exist. (UNKNOWN_TABLE) (version 24.8.4.13 (official build))\n",
        );
        assert_eq!(e.code.as_deref(), Some("60"));
        assert_eq!(
            e.message,
            "Table default.nope does not exist. (UNKNOWN_TABLE)"
        );
    }

    #[test]
    fn wide_integers_stay_exact_and_nullable_types_are_seen_through() {
        assert_eq!(
            convert(&Json::String("18446744073709551615".into()), "UInt64"),
            Value::Number("18446744073709551615".into())
        );
        assert_eq!(
            convert(&Json::String("42".into()), "Nullable(Int64)"),
            Value::Int(42)
        );
        assert_eq!(
            convert(&Json::String("1.50".into()), "Decimal(10, 2)"),
            Value::Number("1.50".into())
        );
        assert_eq!(
            convert(&serde_json::json!(1.5), "Float64"),
            Value::Float(1.5)
        );
        assert_eq!(
            convert(&Json::String("x".into()), "LowCardinality(String)"),
            Value::Text("x".into())
        );
        assert_eq!(
            convert(&serde_json::json!([1, 2]), "Array(UInt8)"),
            Value::Text("[1,2]".into())
        );
        assert_eq!(base_type("Nullable(LowCardinality(String))"), "String");
    }
}
