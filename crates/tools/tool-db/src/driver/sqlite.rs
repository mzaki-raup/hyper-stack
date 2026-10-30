//! SQLite through `rusqlite` (bundled). The connection lives on its own
//! thread (rusqlite is blocking); statements run there and send row batches
//! back over a bounded channel, so the async side can stop the stream and
//! `sqlite3_interrupt` can cancel it.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, InterruptHandle, OpenFlags};
use tokio::sync::{mpsc, oneshot};

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::schema::{ColumnDef, ForeignKey, IndexDef, TableSchema};
use crate::sql::quote_ident;

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

pub struct SqliteSession {
    jobs: std::sync::mpsc::Sender<Job>,
    interrupt: Arc<InterruptHandle>,
    info: ServerInfo,
    in_tx: Arc<AtomicBool>,
}

enum Msg {
    Columns(Vec<Column>),
    Rows(Vec<Row>),
}

fn err(e: rusqlite::Error) -> DbError {
    let code = match &e {
        rusqlite::Error::SqliteFailure(f, _) => Some(format!("{:?}", f.code)),
        _ => None,
    };
    let message = match &e {
        rusqlite::Error::SqliteFailure(_, Some(m)) => m.clone(),
        other => other.to_string(),
    };
    DbError {
        message,
        position: None,
        code,
    }
}

fn value(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => Value::Int(i),
        ValueRef::Real(f) => Value::Float(f),
        ValueRef::Text(t) => Value::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Value::Bytes(b.to_vec()),
    }
}

impl SqliteSession {
    pub async fn open(cfg: &ConnectConfig) -> Result<Self> {
        let path = cfg.path.trim().to_string();
        if path.is_empty() {
            return Err(DbError::msg("no database file is set"));
        }
        let read_only = cfg.read_only;
        let (ready_tx, ready_rx) = oneshot::channel();
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("hs-sqlite".into())
            .spawn(move || {
                let flags = if read_only {
                    OpenFlags::SQLITE_OPEN_READ_ONLY
                        | OpenFlags::SQLITE_OPEN_URI
                        | OpenFlags::SQLITE_OPEN_NO_MUTEX
                } else {
                    OpenFlags::default()
                };
                let opened = Connection::open_with_flags(&path, flags).and_then(|c| {
                    c.busy_timeout(std::time::Duration::from_secs(5))?;
                    if read_only {
                        c.pragma_update(None, "query_only", true)?;
                    }
                    let v: String = c.query_row("select sqlite_version()", [], |r| r.get(0))?;
                    Ok((c, v))
                });
                let mut conn = match opened {
                    Ok((c, v)) => {
                        let _ = ready_tx.send(Ok((Arc::new(c.get_interrupt_handle()), v)));
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
            .map_err(|_| DbError::msg("the SQLite thread stopped"))??;
        let database = std::path::Path::new(&cfg.path)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| cfg.path.clone());
        Ok(Self {
            jobs,
            interrupt,
            info: ServerInfo {
                version: format!("SQLite {version}"),
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
            .map_err(|_| DbError::msg("the SQLite connection is closed"))?;
        rx.await
            .map_err(|_| DbError::msg("the SQLite connection is closed"))?
    }

    async fn rows_of(&self, sql: String) -> Result<Vec<Vec<Value>>> {
        self.call(move |c| {
            let mut st = c.prepare(&sql).map_err(err)?;
            let n = st.column_count();
            let mut rows = st.query([]).map_err(err)?;
            let mut out = Vec::new();
            while let Some(r) = rows.next().map_err(err)? {
                out.push((0..n).map(|i| value(r.get_ref_unwrap(i))).collect());
            }
            Ok(out)
        })
        .await
    }
}

fn text(v: &Value) -> String {
    v.display()
}

#[async_trait]
impl DbSession for SqliteSession {
    fn engine(&self) -> Engine {
        Engine::Sqlite
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let sql = sql.to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let in_tx = self.in_tx.clone();
        let (tx, mut rx) = mpsc::channel::<Msg>(4);
        let done = self.call(move |c| {
            let run = || -> Result<ExecSummary> {
                let mut st = c.prepare(&sql).map_err(err)?;
                let n = st.column_count();
                if n == 0 {
                    st.raw_execute().map_err(err)?;
                    return Ok(ExecSummary {
                        rows_affected: Some(c.changes()),
                        ..Default::default()
                    });
                }
                let cols = st
                    .columns()
                    .iter()
                    .map(|col| Column {
                        name: col.name().to_string(),
                        type_name: col.decl_type().unwrap_or("").to_string(),
                    })
                    .collect();
                let _ = tx.blocking_send(Msg::Columns(cols));
                let mut rows = st.query([]).map_err(err)?;
                let mut batch = Vec::with_capacity(BATCH);
                let mut total = 0u64;
                let mut limited = false;
                while let Some(r) = rows.next().map_err(err)? {
                    if stop2.load(Ordering::Relaxed) {
                        limited = true;
                        break;
                    }
                    batch.push((0..n).map(|i| value(r.get_ref_unwrap(i))).collect());
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
        // Forward batches while the statement runs.
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
        let e = Engine::Sqlite;
        let obj = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        match scope {
            Scope::Schemas => {
                let rows = self.rows_of("PRAGMA database_list".into()).await?;
                Ok(rows
                    .iter()
                    .map(|r| text(&r[1]))
                    .filter(|n| n != "temp")
                    .map(|n| obj(ObjectKind::Schema, n, String::new(), vec![]))
                    .collect())
            }
            Scope::Objects { schema, kind } => {
                let ty = match kind {
                    ObjectKind::Table => "table",
                    ObjectKind::View => "view",
                    _ => return Ok(Vec::new()),
                };
                let sql = format!(
                    "SELECT name FROM {}.sqlite_master WHERE type = '{ty}' AND name NOT LIKE 'sqlite_%' ORDER BY name",
                    quote_ident(schema, e)
                );
                Ok(self
                    .rows_of(sql)
                    .await?
                    .iter()
                    .map(|r| obj(*kind, text(&r[0]), String::new(), vec![]))
                    .collect())
            }
            Scope::Columns { schema, table } => {
                let s = quote_ident(schema, e);
                let t = crate::sql::quote_literal(table, e);
                let fks = self
                    .rows_of(format!("SELECT * FROM {s}.pragma_foreign_key_list({t})"))
                    .await
                    .unwrap_or_default();
                let cols = self
                    .rows_of(format!(
                        "SELECT name, type, \"notnull\", pk FROM {s}.pragma_table_info({t})"
                    ))
                    .await?;
                Ok(cols
                    .iter()
                    .map(|r| {
                        let name = text(&r[0]);
                        let mut badges = Vec::new();
                        if matches!(r[3], Value::Int(p) if p > 0) {
                            badges.push("PK".to_string());
                        }
                        if matches!(r[2], Value::Int(1)) {
                            badges.push("NOT NULL".to_string());
                        }
                        // foreign_key_list: id, seq, table, from, to, …
                        for fk in &fks {
                            if text(&fk[3]) == name {
                                badges.push(format!("FK → {}.{}", text(&fk[2]), text(&fk[4])));
                            }
                        }
                        obj(ObjectKind::Column, name, text(&r[1]), badges)
                    })
                    .collect())
            }
            Scope::Indexes { schema, table } => {
                let s = quote_ident(schema, e);
                let t = crate::sql::quote_literal(table, e);
                let list = self
                    .rows_of(format!(
                        "SELECT name, \"unique\" FROM {s}.pragma_index_list({t})"
                    ))
                    .await?;
                let mut out = Vec::new();
                for r in list {
                    let name = text(&r[0]);
                    let cols = self
                        .rows_of(format!(
                            "SELECT name FROM {s}.pragma_index_info({})",
                            crate::sql::quote_literal(&name, e)
                        ))
                        .await?
                        .iter()
                        .map(|c| text(&c[0]))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let badges = if matches!(r[1], Value::Int(1)) {
                        vec!["UNIQUE".to_string()]
                    } else {
                        vec![]
                    };
                    out.push(obj(ObjectKind::Index, name, format!("({cols})"), badges));
                }
                Ok(out)
            }
        }
    }

    async fn catalog(&mut self) -> Result<Catalog> {
        let rows = self
            .rows_of(
                "SELECT m.name, p.name FROM sqlite_master m JOIN pragma_table_info(m.name) p \
                 WHERE m.type IN ('table','view') AND m.name NOT LIKE 'sqlite_%' ORDER BY m.name, p.cid"
                    .into(),
            )
            .await?;
        let mut cat = Catalog::default();
        for r in rows {
            let (t, c) = (text(&r[0]), text(&r[1]));
            match cat.tables.last_mut() {
                Some((_, name, cols)) if *name == t => cols.push(c),
                _ => cat.tables.push(("main".into(), t, vec![c])),
            }
        }
        Ok(cat)
    }

    async fn table_schema(&mut self, schema: &str, table: &str) -> Result<TableSchema> {
        let e = Engine::Sqlite;
        let s = quote_ident(schema, e);
        let t = crate::sql::quote_literal(table, e);

        // `AUTOINCREMENT` is not a pragma: it only shows in the stored DDL.
        let created = self
            .rows_of(format!(
                "SELECT sql FROM {s}.sqlite_master WHERE type = 'table' AND name = {t}"
            ))
            .await
            .unwrap_or_default();
        let declared_autoinc = created
            .first()
            .map(|r| text(&r[0]).to_ascii_uppercase().contains("AUTOINCREMENT"))
            .unwrap_or(false);

        // cid, name, type, notnull, dflt_value, pk
        let rows = self
            .rows_of(format!("SELECT * FROM {s}.pragma_table_info({t})"))
            .await?;
        let mut columns = Vec::with_capacity(rows.len());
        // pk holds the 1-based position in the key, so the key keeps its order.
        let mut key: Vec<(i64, String)> = Vec::new();
        for r in &rows {
            let name = text(&r[1]);
            let pk = match r[5] {
                Value::Int(i) => i,
                _ => 0,
            };
            if pk > 0 {
                key.push((pk, name.clone()));
            }
            let data_type = text(&r[2]);
            // Only an INTEGER PRIMARY KEY can carry AUTOINCREMENT.
            let auto_increment =
                declared_autoinc && pk == 1 && data_type.eq_ignore_ascii_case("INTEGER");
            columns.push(ColumnDef {
                name,
                data_type,
                // SQLite leaves `notnull` clear on an INTEGER PRIMARY KEY,
                // because the column is a rowid alias rather than a declared
                // NOT NULL. No engine allows a null in a key, and Postgres
                // and MySQL both report one as NOT NULL, so a key column is
                // recorded that way here too: otherwise every primary key
                // would read as a difference in a cross-engine compare.
                nullable: !matches!(r[3], Value::Int(1)) && pk == 0,
                default: match &r[4] {
                    Value::Null => None,
                    v => Some(text(v)),
                },
                auto_increment,
            });
        }
        key.sort_by_key(|(pos, _)| *pos);
        let primary_key: Vec<String> = key.into_iter().map(|(_, n)| n).collect();

        // seq, name, unique, origin, partial. origin "pk" is the key's own index.
        let listed = self
            .rows_of(format!("SELECT * FROM {s}.pragma_index_list({t})"))
            .await
            .unwrap_or_default();
        let mut indexes = Vec::with_capacity(listed.len());
        for r in &listed {
            let name = text(&r[1]);
            let info = self
                .rows_of(format!(
                    "SELECT * FROM {s}.pragma_index_info({})",
                    crate::sql::quote_literal(&name, e)
                ))
                .await
                .unwrap_or_default();
            // seqno, cid, name - already in index order.
            let cols = info
                .iter()
                .map(|i| text(&i[2]))
                .filter(|n| !n.is_empty())
                .collect();
            indexes.push(IndexDef {
                name,
                columns: cols,
                unique: matches!(r[2], Value::Int(1)),
                primary: text(&r[3]) == "pk",
            });
        }

        // id, seq, table, from, to, on_update, on_delete, match
        let fk_rows = self
            .rows_of(format!("SELECT * FROM {s}.pragma_foreign_key_list({t})"))
            .await
            .unwrap_or_default();
        let mut foreign_keys: Vec<ForeignKey> = Vec::new();
        for r in &fk_rows {
            let id = match r[0] {
                Value::Int(i) => i,
                _ => 0,
            };
            let from = text(&r[3]);
            let to = text(&r[4]);
            // One row per column: rows sharing an id are one composite key.
            match foreign_keys.last_mut() {
                Some(fk) if fk.name == format!("fk_{id}") => {
                    fk.columns.push(from);
                    fk.ref_columns.push(to);
                }
                _ => foreign_keys.push(ForeignKey {
                    // SQLite does not name its keys; the id keeps them apart.
                    name: format!("fk_{id}"),
                    columns: vec![from],
                    ref_schema: String::new(),
                    ref_table: text(&r[2]),
                    ref_columns: vec![to],
                    on_delete: crate::schema::referential_action(&text(&r[6])),
                    on_update: crate::schema::referential_action(&text(&r[5])),
                }),
            }
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
