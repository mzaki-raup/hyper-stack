//! PostgreSQL through `tokio-postgres` with rustls.
//!
//! Statements run over the simple-query protocol, so values arrive in the
//! server's own text form (exactly what psql shows, for every type including
//! arrays, ranges and extensions) and any statement works, DDL and
//! transaction control included. Column types come from a Describe of the
//! same statement (`prepare`) when the server accepts one, which is what
//! right-aligns numbers and keeps them unquoted on export.
//!
//! When the row limit is reached outside a transaction block the query is
//! cancelled server-side instead of streaming the rest of a large table.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures_util::StreamExt;
use parking_lot::Mutex;
use tokio_postgres::config::SslMode;
use tokio_postgres::types::Type;
use tokio_postgres::{AsyncMessage, CancelToken, Client, NoTls, SimpleQueryMessage};
use tokio_postgres_rustls::MakeRustlsConnect;

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::sql::quote_literal;

const QUERY_CANCELED: &str = "57014";

#[derive(Clone)]
enum CancelTls {
    Plain,
    Rustls(MakeRustlsConnect),
}

pub struct PgSession {
    client: Client,
    info: ServerInfo,
    cancel_token: CancelToken,
    tls: CancelTls,
    runtime: tokio::runtime::Handle,
    notices: Arc<Mutex<Vec<String>>>,
    in_tx: bool,
}

/// A server error with its position translated to a byte offset in `sql`.
fn pg_err(e: tokio_postgres::Error, sql: &str) -> DbError {
    match e.as_db_error() {
        Some(db) => {
            let position = match db.position() {
                Some(tokio_postgres::error::ErrorPosition::Original(p)) => sql
                    .char_indices()
                    .nth((*p as usize).saturating_sub(1))
                    .map(|(b, _)| b),
                _ => None,
            };
            let mut message = db.message().to_string();
            if let Some(d) = db.detail() {
                message.push('\n');
                message.push_str(d);
            }
            if let Some(h) = db.hint() {
                message.push_str("\nHint: ");
                message.push_str(h);
            }
            DbError {
                message,
                position,
                code: Some(db.code().code().to_string()),
            }
        }
        None => DbError::msg(e.to_string()),
    }
}

fn is_number(t: &Type) -> bool {
    matches!(
        *t,
        Type::INT2
            | Type::INT4
            | Type::INT8
            | Type::OID
            | Type::FLOAT4
            | Type::FLOAT8
            | Type::NUMERIC
    )
}

impl PgSession {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Self> {
        let mut pg = tokio_postgres::Config::new();
        pg.host(&cfg.host)
            .port(cfg.port)
            .user(&cfg.user)
            .application_name("HyperStack")
            .connect_timeout(cfg.connect_timeout);
        if !cfg.database.is_empty() {
            pg.dbname(&cfg.database);
        }
        if let Some(p) = &cfg.password {
            pg.password(p.as_bytes());
        }
        let tls = super::tls::client_config(&cfg.ssl, cfg.ssl_ca.as_deref())?;
        pg.ssl_mode(match cfg.ssl.as_str() {
            "disable" => SslMode::Disable,
            "prefer" => SslMode::Prefer,
            _ => SslMode::Require,
        });
        let notices = Arc::new(Mutex::new(Vec::new()));
        let n2 = notices.clone();
        let (client, cancel_tls) = match tls {
            None => {
                let (client, mut conn) = pg.connect(NoTls).await.map_err(|e| pg_err(e, ""))?;
                tokio::spawn(async move {
                    let mut s = futures_util::stream::poll_fn(move |cx| conn.poll_message(cx));
                    while let Some(Ok(m)) = s.next().await {
                        if let AsyncMessage::Notice(n) = m {
                            n2.lock().push(format!("{}: {}", n.severity(), n.message()));
                        }
                    }
                });
                (client, CancelTls::Plain)
            }
            Some(config) => {
                let make = MakeRustlsConnect::new((*config).clone());
                let (client, mut conn) =
                    pg.connect(make.clone()).await.map_err(|e| pg_err(e, ""))?;
                tokio::spawn(async move {
                    let mut s = futures_util::stream::poll_fn(move |cx| conn.poll_message(cx));
                    while let Some(Ok(m)) = s.next().await {
                        if let AsyncMessage::Notice(n) = m {
                            n2.lock().push(format!("{}: {}", n.severity(), n.message()));
                        }
                    }
                });
                (client, CancelTls::Rustls(make))
            }
        };
        if cfg.read_only {
            client
                .batch_execute("SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY")
                .await
                .map_err(|e| pg_err(e, ""))?;
        }
        let mut s = Self {
            cancel_token: client.cancel_token(),
            client,
            info: ServerInfo::default(),
            tls: cancel_tls,
            runtime: tokio::runtime::Handle::current(),
            notices,
            in_tx: false,
        };
        let r = s
            .rows_of("SELECT current_setting('server_version'), current_database()")
            .await?;
        if let Some(row) = r.first() {
            s.info = ServerInfo {
                // "16.4 (Ubuntu 16.4-1)" → "16.4"
                version: format!(
                    "PostgreSQL {}",
                    cell(row, 0).split_whitespace().next().unwrap_or_default()
                ),
                database: cell(row, 1),
            };
        }
        Ok(s)
    }

    async fn rows_of(&self, sql: &str) -> Result<Vec<Vec<Option<String>>>> {
        let msgs = self
            .client
            .simple_query(sql)
            .await
            .map_err(|e| pg_err(e, sql))?;
        Ok(msgs
            .into_iter()
            .filter_map(|m| match m {
                SimpleQueryMessage::Row(r) => {
                    Some((0..r.len()).map(|i| r.get(i).map(str::to_string)).collect())
                }
                _ => None,
            })
            .collect())
    }

    /// Send a cancel request and wait until it is delivered, so it cannot
    /// land on the next statement.
    async fn cancel_now(&self) {
        let _ = match self.tls.clone() {
            CancelTls::Plain => self.cancel_token.cancel_query(NoTls).await,
            CancelTls::Rustls(m) => self.cancel_token.cancel_query(m).await,
        };
    }
}

fn cell(row: &[Option<String>], i: usize) -> String {
    row.get(i).cloned().flatten().unwrap_or_default()
}

#[async_trait]
impl DbSession for PgSession {
    fn engine(&self) -> Engine {
        Engine::Postgres
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        self.notices.lock().clear();
        // Column types from a Describe; statements the server will not
        // prepare (parameters, some utility commands) just show untyped.
        let types: Vec<Type> = match self.client.prepare(sql).await {
            Ok(st) => st.columns().iter().map(|c| c.type_().clone()).collect(),
            Err(_) => Vec::new(),
        };
        let stream = self
            .client
            .simple_query_raw(sql)
            .await
            .map_err(|e| pg_err(e, sql))?;
        futures_util::pin_mut!(stream);
        let mut summary = ExecSummary::default();
        let mut batch: Vec<Row> = Vec::with_capacity(BATCH);
        let mut stopped = false;
        let mut cancel_sent = false;
        let mut failure = None;
        while let Some(msg) = stream.next().await {
            match msg {
                Err(e) => {
                    let err = pg_err(e, sql);
                    if !(cancel_sent && err.code.as_deref() == Some(QUERY_CANCELED)) {
                        failure = Some(err);
                    }
                    break;
                }
                Ok(SimpleQueryMessage::RowDescription(cols)) => {
                    summary.has_result = true;
                    sink.columns(
                        cols.iter()
                            .enumerate()
                            .map(|(i, c)| Column {
                                name: c.name().to_string(),
                                type_name: types
                                    .get(i)
                                    .map(|t| t.name().to_string())
                                    .unwrap_or_default(),
                            })
                            .collect(),
                    );
                }
                Ok(SimpleQueryMessage::Row(r)) => {
                    if stopped {
                        continue;
                    }
                    let row: Row = (0..r.len())
                        .map(|i| match r.get(i) {
                            None => Value::Null,
                            Some(s) if types.get(i).is_some_and(is_number) => {
                                match s.parse::<i64>() {
                                    Ok(n) => Value::Int(n),
                                    Err(_) => Value::Number(s.to_string()),
                                }
                            }
                            Some(s) => Value::Text(s.to_string()),
                        })
                        .collect();
                    batch.push(row);
                    summary.rows_returned += 1;
                    if batch.len() >= BATCH && sink.rows(std::mem::take(&mut batch)) == Flow::Stop {
                        stopped = true;
                        summary.limited = true;
                        // Cancelling inside a transaction block would abort it;
                        // there the rest is read and dropped instead.
                        if !self.in_tx {
                            self.cancel_now().await;
                            cancel_sent = true;
                        }
                    }
                }
                Ok(SimpleQueryMessage::CommandComplete(n)) => {
                    if !batch.is_empty()
                        && !stopped
                        && sink.rows(std::mem::take(&mut batch)) == Flow::Stop
                    {
                        stopped = true;
                    }
                    if !summary.has_result {
                        summary.rows_affected = Some(n);
                    }
                }
                Ok(_) => {}
            }
        }
        if !batch.is_empty() && !stopped {
            sink.rows(batch);
        }
        if stopped {
            summary.limited = true;
        }
        if let Some(e) = failure {
            return Err(e);
        }
        if let Some(t) = super::tx_effect(sql, Engine::Postgres) {
            self.in_tx = t;
        }
        // The statement text is only a guess (a function may BEGIN, a
        // savepoint may ROLLBACK TO, a failed statement aborts the block);
        // the server knows, so ask it.
        if let Some(t) = self.probe_in_tx().await {
            self.in_tx = t;
        }
        let notices = std::mem::take(&mut *self.notices.lock());
        if !notices.is_empty() {
            summary.message = Some(notices.join("\n"));
        }
        summary.elapsed = started.elapsed();
        Ok(summary)
    }

    fn canceller(&self) -> Canceller {
        let token = self.cancel_token.clone();
        let tls = self.tls.clone();
        let rt = self.runtime.clone();
        Arc::new(move || {
            let token = token.clone();
            let tls = tls.clone();
            rt.spawn(async move {
                let _ = match tls {
                    CancelTls::Plain => token.cancel_query(NoTls).await,
                    CancelTls::Rustls(m) => token.cancel_query(m).await,
                };
            });
        })
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        let e = Engine::Postgres;
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(
                    "SELECT nspname FROM pg_namespace WHERE nspname NOT LIKE 'pg\\_%' \
                     AND nspname <> 'information_schema' ORDER BY nspname = 'public' DESC, nspname",
                )
                .await?
                .iter()
                .map(|r| o(ObjectKind::Schema, cell(r, 0), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let s = quote_literal(schema, e);
                let sql = match kind {
                    ObjectKind::Table => format!(
                        "SELECT c.relname, CASE WHEN c.reltuples >= 0 THEN '~' || c.reltuples::bigint || ' rows' ELSE '' END \
                         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = {s} AND c.relkind IN ('r','p','f') ORDER BY 1"
                    ),
                    ObjectKind::View => format!(
                        "SELECT c.relname, CASE c.relkind WHEN 'm' THEN 'materialized' ELSE '' END \
                         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = {s} AND c.relkind IN ('v','m') ORDER BY 1"
                    ),
                    ObjectKind::Function => format!(
                        "SELECT p.proname || '(' || pg_get_function_identity_arguments(p.oid) || ')', \
                         coalesce(pg_get_function_result(p.oid), 'procedure') \
                         FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
                         WHERE n.nspname = {s} AND p.prokind IN ('f','p','w') ORDER BY 1"
                    ),
                    ObjectKind::Sequence => format!(
                        "SELECT c.relname, '' FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                         WHERE n.nspname = {s} AND c.relkind = 'S' ORDER BY 1"
                    ),
                    _ => return Ok(Vec::new()),
                };
                self.rows_of(&sql)
                    .await?
                    .iter()
                    .map(|r| o(*kind, cell(r, 0), cell(r, 1), vec![]))
                    .collect()
            }
            Scope::Columns { schema, table } => {
                let sql = format!(
                    "SELECT a.attname, format_type(a.atttypid, a.atttypmod), a.attnotnull, \
                       EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid AND i.indisprimary AND a.attnum = ANY(i.indkey)), \
                       (SELECT fc.relname || '.' || fa.attname FROM pg_constraint k \
                          JOIN pg_class fc ON fc.oid = k.confrelid \
                          JOIN pg_attribute fa ON fa.attrelid = k.confrelid AND fa.attnum = k.confkey[1] \
                         WHERE k.conrelid = c.oid AND k.contype = 'f' AND k.conkey[1] = a.attnum LIMIT 1) \
                     FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = {} AND c.relname = {} AND a.attnum > 0 AND NOT a.attisdropped \
                     ORDER BY a.attnum",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                );
                self.rows_of(&sql)
                    .await?
                    .iter()
                    .map(|r| {
                        let mut badges = Vec::new();
                        if cell(r, 3) == "t" {
                            badges.push("PK".to_string());
                        }
                        if cell(r, 2) == "t" {
                            badges.push("NOT NULL".to_string());
                        }
                        let fk = cell(r, 4);
                        if !fk.is_empty() {
                            badges.push(format!("FK → {fk}"));
                        }
                        o(ObjectKind::Column, cell(r, 0), cell(r, 1), badges)
                    })
                    .collect()
            }
            Scope::Indexes { schema, table } => {
                let sql = format!(
                    "SELECT i.relname, pg_get_indexdef(i.oid), ix.indisunique, ix.indisprimary \
                     FROM pg_index ix JOIN pg_class i ON i.oid = ix.indexrelid \
                     JOIN pg_class c ON c.oid = ix.indrelid JOIN pg_namespace n ON n.oid = c.relnamespace \
                     WHERE n.nspname = {} AND c.relname = {} ORDER BY 1",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                );
                self.rows_of(&sql)
                    .await?
                    .iter()
                    .map(|r| {
                        let mut badges = Vec::new();
                        if cell(r, 3) == "t" {
                            badges.push("PK".to_string());
                        } else if cell(r, 2) == "t" {
                            badges.push("UNIQUE".to_string());
                        }
                        // "CREATE INDEX x ON s.t USING btree (a, b)" → "btree (a, b)"
                        let def = cell(r, 1);
                        let short = def
                            .split_once(" USING ")
                            .map(|(_, rest)| rest.to_string())
                            .unwrap_or(def);
                        o(ObjectKind::Index, cell(r, 0), short, badges)
                    })
                    .collect()
            }
        })
    }

    async fn catalog(&mut self) -> Result<Catalog> {
        let rows = self
            .rows_of(
                "SELECT table_schema, table_name, column_name FROM information_schema.columns \
                 WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
                 ORDER BY table_schema, table_name, ordinal_position",
            )
            .await?;
        Ok(group_catalog(
            rows.iter().map(|r| (cell(r, 0), cell(r, 1), cell(r, 2))),
        ))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }
}

/// (schema, table, column) rows in order → one entry per table.
pub(crate) fn group_catalog(rows: impl Iterator<Item = (String, String, String)>) -> Catalog {
    let mut cat = Catalog::default();
    for (s, t, c) in rows {
        match cat.tables.last_mut() {
            Some((ls, lt, cols)) if *ls == s && *lt == t => cols.push(c),
            _ => cat.tables.push((s, t, vec![c])),
        }
    }
    cat
}

impl PgSession {
    /// Ask the server whether a transaction block is open. `now()` is the
    /// block's start time and `statement_timestamp()` this statement's, so
    /// they differ only inside a block; an aborted block answers with
    /// error 25P02, which is a block too. `None` when the probe itself
    /// failed for another reason (keep the guess).
    async fn probe_in_tx(&self) -> Option<bool> {
        match self
            .client
            .simple_query("SELECT now() <> statement_timestamp()")
            .await
        {
            Ok(msgs) => msgs.iter().find_map(|m| match m {
                SimpleQueryMessage::Row(r) => r.get(0).map(|v| v == "t"),
                _ => None,
            }),
            Err(e) => match e.as_db_error().map(|d| d.code().code()) {
                Some("25P02") => Some(true),
                _ => None,
            },
        }
    }
}
