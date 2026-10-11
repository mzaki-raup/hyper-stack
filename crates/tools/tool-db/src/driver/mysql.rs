//! MySQL and MariaDB through `mysql_async` (text protocol, rustls with ring).
//!
//! Cancel is `KILL QUERY <id>` from a second, short-lived connection, sent
//! and awaited before the result is drained so it can never hit the next
//! statement. Certificate checks use the bundled web PKI roots plus the
//! profile's `ssl_ca`; `verify-ca` skips the host-name check (libpq meaning).

use std::sync::Arc;
use std::sync::Once;
use std::time::Instant;

use async_trait::async_trait;
use mysql_async::consts::ColumnType;
use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder, SslOpts};

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::sql::quote_literal;

/// ER_QUERY_INTERRUPTED (MySQL and MariaDB).
const QUERY_INTERRUPTED: u16 = 1317;

pub struct MySession {
    conn: Conn,
    opts: Opts,
    info: ServerInfo,
    runtime: tokio::runtime::Handle,
    in_tx: bool,
}

/// mysql_async builds its rustls config with `ClientConfig::builder()`, which
/// needs a process-wide provider when more than one is compiled in.
pub(crate) fn install_crypto_provider() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn my_err(e: mysql_async::Error) -> DbError {
    match &e {
        mysql_async::Error::Server(s) => DbError {
            message: s.message.clone(),
            position: None,
            code: Some(format!("{} / {}", s.code, s.state)),
        },
        other => DbError::msg(other.to_string()),
    }
}

fn is_interrupted(e: &mysql_async::Error) -> bool {
    matches!(e, mysql_async::Error::Server(s) if s.code == QUERY_INTERRUPTED)
}

fn is_number(t: ColumnType) -> bool {
    use ColumnType::*;
    matches!(
        t,
        MYSQL_TYPE_TINY
            | MYSQL_TYPE_SHORT
            | MYSQL_TYPE_LONG
            | MYSQL_TYPE_LONGLONG
            | MYSQL_TYPE_INT24
            | MYSQL_TYPE_FLOAT
            | MYSQL_TYPE_DOUBLE
            | MYSQL_TYPE_DECIMAL
            | MYSQL_TYPE_NEWDECIMAL
    )
}

fn is_binary(c: &mysql_async::Column) -> bool {
    use ColumnType::*;
    // charset 63 = binary
    c.character_set() == 63
        && matches!(
            c.column_type(),
            MYSQL_TYPE_BLOB
                | MYSQL_TYPE_TINY_BLOB
                | MYSQL_TYPE_MEDIUM_BLOB
                | MYSQL_TYPE_LONG_BLOB
                | MYSQL_TYPE_VAR_STRING
                | MYSQL_TYPE_STRING
                | MYSQL_TYPE_VARCHAR
                | MYSQL_TYPE_GEOMETRY
        )
}

fn type_name(t: ColumnType) -> String {
    format!("{t:?}")
        .trim_start_matches("MYSQL_TYPE_")
        .to_string()
}

fn convert(v: &mysql_async::Value, col: &mysql_async::Column) -> Value {
    use mysql_async::Value as V;
    match v {
        V::NULL => Value::Null,
        V::Bytes(b) if is_binary(col) => Value::Bytes(b.clone()),
        V::Bytes(b) => {
            let s = String::from_utf8_lossy(b).into_owned();
            if is_number(col.column_type()) {
                match s.parse::<i64>() {
                    Ok(i) => Value::Int(i),
                    Err(_) => Value::Number(s),
                }
            } else {
                Value::Text(s)
            }
        }
        V::Int(i) => Value::Int(*i),
        V::UInt(u) => Value::Number(u.to_string()),
        V::Float(f) => Value::Float(*f as f64),
        V::Double(d) => Value::Float(*d),
        other => Value::Text(other.as_sql(true).trim_matches('\'').to_string()),
    }
}

async fn kill_query(opts: Opts, id: u32) {
    if let Ok(mut c) = Conn::new(opts).await {
        let _ = c.query_drop(format!("KILL QUERY {id}")).await;
        let _ = c.disconnect().await;
    }
}

impl MySession {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Self> {
        install_crypto_provider();
        let base = OptsBuilder::default()
            .ip_or_hostname(cfg.host.clone())
            .tcp_port(cfg.port)
            .user(Some(cfg.user.clone()).filter(|u| !u.is_empty()))
            .pass(cfg.password.clone())
            .db_name(Some(cfg.database.clone()).filter(|d| !d.is_empty()))
            .prefer_socket(false);
        let ssl = |verify: bool, skip_domain: bool| -> Result<SslOpts> {
            let mut s = SslOpts::default()
                .with_danger_accept_invalid_certs(!verify)
                .with_danger_skip_domain_validation(skip_domain || !verify);
            if verify && let Some(ca) = cfg.ssl_ca.as_ref().filter(|c| !c.trim().is_empty()) {
                s = s.with_root_certs(vec![std::path::PathBuf::from(ca).into()]);
            }
            Ok(s)
        };
        let attempt = |o: OptsBuilder| async {
            tokio::time::timeout(cfg.connect_timeout, Conn::new(o))
                .await
                .map_err(|_| DbError::msg("timed out connecting"))?
                .map_err(my_err)
        };
        let (conn, opts) = match cfg.ssl.as_str() {
            "disable" => {
                let o = base.clone();
                (attempt(o.clone()).await?, Opts::from(o))
            }
            "prefer" => {
                let tls = base.clone().ssl_opts(Some(ssl(false, true)?));
                match attempt(tls.clone()).await {
                    Ok(c) => (c, Opts::from(tls)),
                    Err(_) => {
                        let o = base.clone();
                        (attempt(o.clone()).await?, Opts::from(o))
                    }
                }
            }
            mode => {
                let verify = super::tls::verifies(mode);
                let o = base
                    .clone()
                    .ssl_opts(Some(ssl(verify, mode == "verify-ca")?));
                (attempt(o.clone()).await?, Opts::from(o))
            }
        };
        let mut s = Self {
            conn,
            opts,
            info: ServerInfo::default(),
            runtime: tokio::runtime::Handle::current(),
            in_tx: false,
        };
        if cfg.read_only {
            s.conn
                .query_drop("SET SESSION TRANSACTION READ ONLY")
                .await
                .map_err(my_err)?;
        }
        let (version, db): (String, Option<String>) = s
            .conn
            .query_first("SELECT VERSION(), DATABASE()")
            .await
            .map_err(my_err)?
            .unwrap_or_default();
        let flavour = if version.to_ascii_lowercase().contains("mariadb") {
            "MariaDB"
        } else {
            "MySQL"
        };
        s.info = ServerInfo {
            version: format!(
                "{flavour} {}",
                version.split('-').next().unwrap_or(&version)
            ),
            database: db.unwrap_or_default(),
        };
        Ok(s)
    }

    async fn rows_of(&mut self, sql: &str) -> Result<Vec<Vec<String>>> {
        let rows: Vec<mysql_async::Row> = self.conn.query(sql).await.map_err(my_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (0..r.len())
                    .map(|i| match r.as_ref(i) {
                        Some(mysql_async::Value::Bytes(b)) => {
                            String::from_utf8_lossy(b).into_owned()
                        }
                        Some(mysql_async::Value::NULL) | None => String::new(),
                        Some(v) => v.as_sql(true).trim_matches('\'').to_string(),
                    })
                    .collect()
            })
            .collect())
    }
}

#[async_trait]
impl DbSession for MySession {
    fn engine(&self) -> Engine {
        Engine::Mysql
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let id = self.conn.id();
        let opts = self.opts.clone();
        let mut summary = ExecSummary::default();
        let mut result = self.conn.query_iter(sql).await.map_err(my_err)?;
        let cols = result.columns();
        let mut stopped = false;
        if let Some(cols) = cols.filter(|c| !c.is_empty()) {
            summary.has_result = true;
            sink.columns(
                cols.iter()
                    .map(|c| Column {
                        name: c.name_str().to_string(),
                        type_name: type_name(c.column_type()),
                    })
                    .collect(),
            );
            let mut batch: Vec<Row> = Vec::with_capacity(BATCH);
            loop {
                let row = match result.next().await {
                    Ok(Some(r)) => r,
                    Ok(None) => break,
                    Err(e) => return Err(my_err(e)),
                };
                batch.push(
                    (0..row.len())
                        .map(|i| {
                            row.as_ref(i)
                                .map(|v| convert(v, &cols[i]))
                                .unwrap_or(Value::Null)
                        })
                        .collect(),
                );
                summary.rows_returned += 1;
                if batch.len() >= BATCH && sink.rows(std::mem::take(&mut batch)) == Flow::Stop {
                    stopped = true;
                    break;
                }
            }
            if !batch.is_empty() && !stopped && sink.rows(batch) == Flow::Stop {
                stopped = true;
            }
        } else {
            summary.rows_affected = Some(result.affected_rows());
            let info = result.info().to_string();
            if !info.is_empty() {
                summary.message = Some(info);
            }
        }
        let warnings = result.warnings();
        if stopped {
            summary.limited = true;
            // KILL QUERY ends the statement, not the transaction; wait for it
            // so it cannot land on the next statement.
            kill_query(opts, id).await;
        }
        match result.drop_result().await {
            Ok(()) => {}
            Err(e) if stopped && is_interrupted(&e) => {}
            Err(e) => return Err(my_err(e)),
        }
        if warnings > 0 {
            let w = self
                .rows_of("SHOW WARNINGS")
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|r| r.join(": "))
                .collect::<Vec<_>>()
                .join("\n");
            if !w.is_empty() {
                summary.message = Some(match summary.message.take() {
                    Some(m) => format!("{m}\n{w}"),
                    None => w,
                });
            }
        }
        if let Some(t) = super::tx_effect(sql, Engine::Mysql) {
            self.in_tx = t;
        }
        summary.elapsed = started.elapsed();
        Ok(summary)
    }

    fn canceller(&self) -> Canceller {
        let opts = self.opts.clone();
        let id = self.conn.id();
        let rt = self.runtime.clone();
        Arc::new(move || {
            rt.spawn(kill_query(opts.clone(), id));
        })
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        let e = Engine::Mysql;
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(
                    "SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN \
                     ('information_schema','performance_schema','mysql','sys') ORDER BY schema_name = DATABASE() DESC, schema_name",
                )
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Schema, r[0].clone(), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let s = quote_literal(schema, e);
                let sql = match kind {
                    ObjectKind::Table => format!(
                        "SELECT table_name, CONCAT('~', COALESCE(table_rows, 0), ' rows') FROM information_schema.tables \
                         WHERE table_schema = {s} AND table_type = 'BASE TABLE' ORDER BY table_name"
                    ),
                    ObjectKind::View => format!(
                        "SELECT table_name, '' FROM information_schema.tables \
                         WHERE table_schema = {s} AND table_type = 'VIEW' ORDER BY table_name"
                    ),
                    ObjectKind::Function => format!(
                        "SELECT routine_name, LOWER(routine_type) FROM information_schema.routines \
                         WHERE routine_schema = {s} ORDER BY routine_name"
                    ),
                    _ => return Ok(Vec::new()),
                };
                self.rows_of(&sql)
                    .await?
                    .into_iter()
                    .map(|r| o(*kind, r[0].clone(), r[1].clone(), vec![]))
                    .collect()
            }
            Scope::Columns { schema, table } => {
                let sql = format!(
                    "SELECT c.column_name, c.column_type, c.is_nullable, c.column_key, \
                       (SELECT CONCAT(k.referenced_table_name, '.', k.referenced_column_name) \
                          FROM information_schema.key_column_usage k \
                         WHERE k.table_schema = c.table_schema AND k.table_name = c.table_name \
                           AND k.column_name = c.column_name AND k.referenced_table_name IS NOT NULL LIMIT 1) \
                     FROM information_schema.columns c WHERE c.table_schema = {} AND c.table_name = {} \
                     ORDER BY c.ordinal_position",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                );
                self.rows_of(&sql)
                    .await?
                    .into_iter()
                    .map(|r| {
                        let mut badges = Vec::new();
                        match r[3].as_str() {
                            "PRI" => badges.push("PK".to_string()),
                            "UNI" => badges.push("UNIQUE".to_string()),
                            _ => {}
                        }
                        if r[2] == "NO" {
                            badges.push("NOT NULL".to_string());
                        }
                        if !r[4].is_empty() {
                            badges.push(format!("FK → {}", r[4]));
                        }
                        o(ObjectKind::Column, r[0].clone(), r[1].clone(), badges)
                    })
                    .collect()
            }
            Scope::Indexes { schema, table } => {
                let sql = format!(
                    "SELECT index_name, GROUP_CONCAT(column_name ORDER BY seq_in_index SEPARATOR ', '), MIN(non_unique), MIN(index_type) \
                     FROM information_schema.statistics WHERE table_schema = {} AND table_name = {} \
                     GROUP BY index_name ORDER BY index_name",
                    quote_literal(schema, e),
                    quote_literal(table, e)
                );
                self.rows_of(&sql)
                    .await?
                    .into_iter()
                    .map(|r| {
                        let mut badges = Vec::new();
                        if r[0] == "PRIMARY" {
                            badges.push("PK".to_string());
                        } else if r[2] == "0" {
                            badges.push("UNIQUE".to_string());
                        }
                        o(
                            ObjectKind::Index,
                            r[0].clone(),
                            format!("{} ({})", r[3].to_lowercase(), r[1]),
                            badges,
                        )
                    })
                    .collect()
            }
        })
    }

    async fn catalog(&mut self) -> Result<Catalog> {
        let rows = self
            .rows_of(
                "SELECT table_schema, table_name, column_name FROM information_schema.columns \
                 WHERE table_schema NOT IN ('information_schema','performance_schema','mysql','sys') \
                 ORDER BY table_schema, table_name, ordinal_position",
            )
            .await?;
        Ok(super::postgres::group_catalog(
            rows.into_iter()
                .map(|r| (r[0].clone(), r[1].clone(), r[2].clone())),
        ))
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }
}
