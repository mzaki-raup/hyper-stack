//! DuckDB through the `duckdb` crate, the engine bundled into the exe
//! (ADR-030). Like SQLite, the database is a file (or `:memory:`), the
//! connection is blocking and lives on its own thread, and statements send
//! row batches back over a bounded channel so the async side can stop the
//! stream; Cancel interrupts the running statement.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use duckdb::types::{TimeUnit, Value as Dv};
use duckdb::{AccessMode, Config, Connection, InterruptHandle};
use tokio::sync::{mpsc, oneshot};

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::schema::{ColumnDef, ForeignKey, IndexDef, TableSchema};
use crate::sql::{Class, quote_literal};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

pub struct DuckSession {
    jobs: std::sync::mpsc::Sender<Job>,
    interrupt: Arc<InterruptHandle>,
    info: ServerInfo,
    in_tx: Arc<AtomicBool>,
}

enum Msg {
    Columns(Vec<Column>),
    Rows(Vec<Row>),
}

fn err(e: duckdb::Error) -> DbError {
    DbError::msg(e.to_string())
}

fn micros_text(us: i64) -> String {
    chrono::DateTime::from_timestamp_micros(us)
        .map(|t| t.naive_utc().format("%Y-%m-%d %H:%M:%S%.f").to_string())
        .unwrap_or_else(|| us.to_string())
}

fn to_micros(unit: TimeUnit, v: i64) -> i64 {
    match unit {
        TimeUnit::Second => v.saturating_mul(1_000_000),
        TimeUnit::Millisecond => v.saturating_mul(1_000),
        TimeUnit::Microsecond => v,
        TimeUnit::Nanosecond => v / 1_000,
    }
}

/// One cell, in the form DuckDB's own shell would print it.
fn convert(v: Dv) -> Value {
    match v {
        Dv::Null => Value::Null,
        Dv::Boolean(b) => Value::Text(b.to_string()),
        Dv::TinyInt(i) => Value::Int(i as i64),
        Dv::SmallInt(i) => Value::Int(i as i64),
        Dv::Int(i) => Value::Int(i as i64),
        Dv::BigInt(i) => Value::Int(i),
        Dv::UTinyInt(i) => Value::Int(i as i64),
        Dv::USmallInt(i) => Value::Int(i as i64),
        Dv::UInt(i) => Value::Int(i as i64),
        Dv::UBigInt(i) => Value::Number(i.to_string()),
        Dv::HugeInt(i) => Value::Number(i.to_string()),
        Dv::Float(f) => Value::Float(f as f64),
        Dv::Double(f) => Value::Float(f),
        Dv::Decimal(d) => Value::Number(d.to_string()),
        Dv::Text(s) | Dv::Enum(s) => Value::Text(s),
        Dv::Blob(b) => Value::Bytes(b),
        Dv::Timestamp(unit, v) => Value::Text(micros_text(to_micros(unit, v))),
        Dv::Date32(days) => Value::Text(
            chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
                .and_then(|d| d.checked_add_signed(chrono::Duration::days(days as i64)))
                .map(|d| d.to_string())
                .unwrap_or_else(|| days.to_string()),
        ),
        Dv::Time64(unit, v) => {
            let us = to_micros(unit, v);
            Value::Text(
                chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                    (us / 1_000_000) as u32,
                    ((us % 1_000_000) * 1_000) as u32,
                )
                .map(|t| t.to_string())
                .unwrap_or_else(|| us.to_string()),
            )
        }
        // Lists, structs, maps, intervals: their debug form is readable.
        other => Value::Text(format!("{other:?}")),
    }
}

/// Every value as text, for the catalogue queries.
fn text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        other => other.display(),
    }
}

impl DuckSession {
    pub async fn open(cfg: &ConnectConfig) -> Result<Self> {
        let path = cfg.path.trim().to_string();
        if path.is_empty() {
            return Err(DbError::msg("no database file is set"));
        }
        let read_only = cfg.read_only;
        let (ready_tx, ready_rx) = oneshot::channel();
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("hs-duckdb".into())
            .spawn(move || {
                let opened = (|| -> duckdb::Result<(Connection, String)> {
                    let config = if read_only {
                        Config::default().access_mode(AccessMode::ReadOnly)?
                    } else {
                        Config::default()
                    };
                    let c = if path == ":memory:" {
                        Connection::open_in_memory_with_flags(config)?
                    } else {
                        Connection::open_with_flags(&path, config)?
                    };
                    let v: String = c.query_row("SELECT version()", [], |r| r.get(0))?;
                    Ok((c, v))
                })();
                let mut conn = match opened {
                    Ok((c, v)) => {
                        let _ = ready_tx.send(Ok((c.interrupt_handle(), v)));
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(err(e)));
                        return;
                    }
                };
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })
            .map_err(|e| DbError::msg(e.to_string()))?;
        let (interrupt, version) = ready_rx
            .await
            .map_err(|_| DbError::msg("the DuckDB thread stopped"))??;
        let database = std::path::Path::new(&cfg.path)
            .file_stem()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| cfg.path.clone());
        Ok(Self {
            jobs,
            interrupt,
            info: ServerInfo {
                version: format!("DuckDB {version}"),
                database,
            },
            in_tx: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Run `f` on the connection thread.
    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |c| {
                let _ = tx.send(f(c));
            }))
            .map_err(|_| DbError::msg("the DuckDB connection is closed"))?;
        rx.await
            .map_err(|_| DbError::msg("the DuckDB connection is closed"))?
    }

    async fn rows_of(&self, sql: String) -> Result<Vec<Vec<String>>> {
        self.call(move |c| {
            let mut st = c.prepare(&sql).map_err(err)?;
            let mut rows = st.query([]).map_err(err)?;
            let mut out = Vec::new();
            while let Some(r) = rows.next().map_err(err)? {
                let n = r.as_ref().column_count();
                out.push(
                    (0..n)
                        .map(|i| text(&convert(r.get::<_, Dv>(i).unwrap_or(Dv::Null))))
                        .collect(),
                );
            }
            Ok(out)
        })
        .await
    }
}

#[async_trait]
impl DbSession for DuckSession {
    fn engine(&self) -> Engine {
        Engine::DuckDb
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let sql = sql.to_string();
        let read = crate::sql::classify(&sql, Engine::DuckDb).class == Class::Read
            || sql.trim_start().to_ascii_uppercase().starts_with("FROM ");
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let in_tx = self.in_tx.clone();
        let (tx, mut rx) = mpsc::channel::<Msg>(4);
        let done = self.call(move |c| {
            let run = || -> Result<ExecSummary> {
                if !read {
                    let n = c.execute(&sql, []).map_err(err)?;
                    return Ok(ExecSummary {
                        rows_affected: Some(n as u64),
                        ..Default::default()
                    });
                }
                let mut st = c.prepare(&sql).map_err(err)?;
                let mut rows = st.query([]).map_err(err)?;
                let cols: Vec<Column> = {
                    let s = rows.as_ref().ok_or_else(|| DbError::msg("no statement"))?;
                    (0..s.column_count())
                        .map(|i| Column {
                            name: s.column_name(i).map(|n| n.to_string()).unwrap_or_default(),
                            type_name: format!("{:?}", s.column_type(i)),
                        })
                        .collect()
                };
                let n = cols.len();
                let _ = tx.blocking_send(Msg::Columns(cols));
                let mut batch = Vec::with_capacity(BATCH);
                let mut total = 0u64;
                let mut limited = false;
                while let Some(r) = rows.next().map_err(err)? {
                    if stop2.load(Ordering::Relaxed) {
                        limited = true;
                        break;
                    }
                    batch.push(
                        (0..n)
                            .map(|i| convert(r.get::<_, Dv>(i).unwrap_or(Dv::Null)))
                            .collect(),
                    );
                    total += 1;
                    if batch.len() == BATCH
                        && tx
                            .blocking_send(Msg::Rows(std::mem::take(&mut batch)))
                            .is_err()
                    {
                        break;
                    }
                }
                if !batch.is_empty() {
                    let _ = tx.blocking_send(Msg::Rows(batch));
                }
                Ok(ExecSummary {
                    rows_returned: total,
                    limited,
                    has_result: true,
                    ..Default::default()
                })
            };
            let r = run();
            in_tx.store(!c.is_autocommit(), Ordering::Relaxed);
            drop(tx);
            r
        });
        let forward = async {
            let mut stopped = false;
            while let Some(m) = rx.recv().await {
                match m {
                    Msg::Columns(c) => sink.columns(c),
                    Msg::Rows(r) if !stopped => {
                        if sink.rows(r) == Flow::Stop {
                            stopped = true;
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                    Msg::Rows(_) => {}
                }
            }
            stopped
        };
        let (result, stopped) = tokio::join!(done, forward);
        let mut s = result?;
        s.limited |= stopped;
        s.elapsed = started.elapsed();
        Ok(s)
    }

    fn canceller(&self) -> Canceller {
        let h = self.interrupt.clone();
        Arc::new(move || h.interrupt())
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        let e = Engine::DuckDb;
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(
                    "SELECT schema_name FROM information_schema.schemata \
                     WHERE catalog_name = current_database() \
                       AND schema_name NOT IN ('information_schema', 'pg_catalog') \
                     ORDER BY schema_name = 'main' DESC, schema_name"
                        .into(),
                )
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Schema, r[0].clone(), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let s = quote_literal(schema, e);
                let sql = match kind {
                    ObjectKind::Table => format!(
                        "SELECT table_name, '~' || estimated_size || ' rows' FROM duckdb_tables() \
                         WHERE database_name = current_database() AND schema_name = {s} ORDER BY table_name"
                    ),
                    ObjectKind::View => format!(
                        "SELECT view_name, '' FROM duckdb_views() WHERE NOT internal \
                         AND database_name = current_database() AND schema_name = {s} ORDER BY view_name"
                    ),
                    ObjectKind::Sequence => format!(
                        "SELECT sequence_name, '' FROM duckdb_sequences() \
                         WHERE database_name = current_database() AND schema_name = {s} ORDER BY sequence_name"
                    ),
                    _ => return Ok(Vec::new()),
                };
                self.rows_of(sql)
                    .await?
                    .into_iter()
                    .map(|r| o(*kind, r[0].clone(), r[1].clone(), vec![]))
                    .collect()
            }
            Scope::Columns { schema, table } => {
                let t = self.table_schema(schema, table).await?;
                t.columns
                    .iter()
                    .map(|c| {
                        let mut badges = Vec::new();
                        if t.primary_key.contains(&c.name) {
                            badges.push("PK".to_string());
                        }
                        if !c.nullable {
                            badges.push("NOT NULL".to_string());
                        }
                        for fk in &t.foreign_keys {
                            if let Some(i) = fk.columns.iter().position(|x| x == &c.name) {
                                badges.push(format!(
                                    "FK → {}.{}",
                                    fk.ref_table,
                                    fk.ref_columns.get(i).cloned().unwrap_or_default()
                                ));
                            }
                        }
                        o(
                            ObjectKind::Column,
                            c.name.clone(),
                            c.data_type.clone(),
                            badges,
                        )
                    })
                    .collect()
            }
            Scope::Indexes { schema, table } => {
                let t = self.table_schema(schema, table).await?;
                t.indexes
                    .iter()
                    .map(|ix| {
                        let badges = if ix.unique {
                            vec!["UNIQUE".to_string()]
                        } else {
                            vec![]
                        };
                        o(
                            ObjectKind::Index,
                            ix.name.clone(),
                            format!("({})", ix.columns.join(", ")),
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
                "SELECT schema_name, table_name, column_name FROM duckdb_columns() \
                 WHERE database_name = current_database() AND NOT internal \
                 ORDER BY schema_name, table_name, column_index"
                    .into(),
            )
            .await?;
        Ok(super::postgres::group_catalog(
            rows.into_iter()
                .map(|r| (r[0].clone(), r[1].clone(), r[2].clone())),
        ))
    }

    async fn table_schema(&mut self, schema: &str, table: &str) -> Result<TableSchema> {
        let e = Engine::DuckDb;
        let (s, t) = (quote_literal(schema, e), quote_literal(table, e));
        let scope = format!(
            "database_name = current_database() AND schema_name = {s} AND table_name = {t}"
        );
        let cols = self
            .rows_of(format!(
                "SELECT column_name, data_type, is_nullable, column_default FROM duckdb_columns() \
                 WHERE {scope} ORDER BY column_index"
            ))
            .await?;
        let columns = cols
            .iter()
            .map(|r| ColumnDef {
                name: r[0].clone(),
                data_type: r[1].clone(),
                nullable: r[2] == "true",
                default: Some(r[3].clone()).filter(|d| !d.is_empty()),
                auto_increment: r[3].starts_with("nextval("),
            })
            .collect();
        let cons = self
            .rows_of(format!(
                "SELECT constraint_type, array_to_string(constraint_column_names, chr(31)), \
                   coalesce(referenced_table, ''), array_to_string(coalesce(referenced_column_names, []), chr(31)), \
                   constraint_name \
                 FROM duckdb_constraints() WHERE {scope} \
                   AND constraint_type IN ('PRIMARY KEY', 'FOREIGN KEY', 'UNIQUE') \
                 ORDER BY constraint_index"
            ))
            .await?;
        let split = |x: &str| -> Vec<String> {
            x.split('\u{1f}')
                .filter(|p| !p.is_empty())
                .map(String::from)
                .collect()
        };
        let mut primary_key = Vec::new();
        let mut foreign_keys = Vec::new();
        let mut indexes = Vec::new();
        for r in &cons {
            match r[0].as_str() {
                "PRIMARY KEY" => primary_key = split(&r[1]),
                "FOREIGN KEY" => foreign_keys.push(ForeignKey {
                    name: r[4].clone(),
                    columns: split(&r[1]),
                    ref_schema: schema.to_string(),
                    ref_table: r[2].clone(),
                    ref_columns: split(&r[3]),
                    on_delete: None,
                    on_update: None,
                }),
                _ => indexes.push(IndexDef {
                    name: r[4].clone(),
                    columns: split(&r[1]),
                    unique: true,
                    primary: false,
                }),
            }
        }
        // Explicit CREATE INDEX statements; their column list is the
        // expression list DuckDB keeps as text ("[a, b]").
        let ixs = self
            .rows_of(format!(
                "SELECT index_name, is_unique, expressions FROM duckdb_indexes() WHERE {scope} ORDER BY index_name"
            ))
            .await
            .unwrap_or_default();
        for r in &ixs {
            indexes.push(IndexDef {
                name: r[0].clone(),
                columns: r[2]
                    .trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|c| c.trim().trim_matches('\'').trim_matches('"').to_string())
                    .filter(|c| !c.is_empty())
                    .collect(),
                unique: r[1] == "true",
                primary: false,
            });
        }
        Ok(TableSchema {
            schema: schema.to_string(),
            name: table.to_string(),
            columns,
            primary_key,
            indexes,
            foreign_keys,
        })
    }

    fn in_transaction(&self) -> bool {
        self.in_tx.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Collect, DbSession};

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    /// DuckDB runs in-process, so its driver is tested for real here, the
    /// way SQLite's is (ADR-030).
    #[test]
    fn duckdb_runs_reads_writes_and_reads_its_own_catalogue() {
        rt().block_on(async {
            let dir = tempfile::tempdir().expect("dir");
            let cfg = ConnectConfig {
                engine: Engine::DuckDb,
                path: dir.path().join("shop.duckdb").to_string_lossy().to_string(),
                ..Default::default()
            };
            let mut s = DuckSession::open(&cfg).await.expect("open");
            assert!(s.server_info().version.starts_with("DuckDB"));
            let mut sink = Collect::default();
            for sql in [
                "CREATE TABLE customers (id INTEGER PRIMARY KEY, name VARCHAR NOT NULL)",
                "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers (id), total DECIMAL(10, 2), placed DATE)",
                "CREATE INDEX orders_placed ON orders (placed)",
            ] {
                s.execute(sql, &mut sink).await.expect(sql);
            }
            let w = s
                .execute("INSERT INTO customers VALUES (1, 'Ada'), (2, 'Bob')", &mut sink)
                .await
                .expect("insert");
            assert_eq!(w.rows_affected, Some(2));
            s.execute(
                "INSERT INTO orders VALUES (10, 1, 12.50, DATE '2026-10-31')",
                &mut sink,
            )
            .await
            .expect("insert order");
            let mut rows = Collect::default();
            let r = s
                .execute("SELECT id, total, placed FROM orders", &mut rows)
                .await
                .expect("select");
            assert!(r.has_result);
            assert_eq!(rows.columns.len(), 3);
            assert_eq!(rows.rows[0][0], Value::Int(10));
            assert_eq!(rows.rows[0][1], Value::Number("12.50".into()));
            assert_eq!(rows.rows[0][2], Value::Text("2026-10-31".into()));

            let t = s.table_schema("main", "orders").await.expect("schema");
            assert_eq!(t.primary_key, vec!["id"]);
            assert_eq!(t.foreign_keys[0].ref_table, "customers");
            assert_eq!(t.foreign_keys[0].columns, vec!["customer_id"]);
            assert!(t.indexes.iter().any(|ix| ix.name == "orders_placed" && ix.columns == vec!["placed"]));
            let cat = s.catalog().await.expect("catalog");
            assert!(cat.tables.iter().any(|(sc, t, cols)| sc == "main" && t == "customers" && cols.len() == 2));
            let tables = s
                .introspect(&Scope::Objects {
                    schema: "main".into(),
                    kind: ObjectKind::Table,
                })
                .await
                .expect("tables");
            assert_eq!(tables.len(), 2);
        });
    }
}
