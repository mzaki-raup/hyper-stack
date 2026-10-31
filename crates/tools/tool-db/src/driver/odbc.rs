//! Oracle and Db2 through ODBC (`odbc-api` over the vendor's driver, which
//! the user installs; ADR-030). One session type serves both: what differs
//! is the connection string and the catalogue queries, kept in [`Dialect`].
//!
//! ODBC calls block, so the connection lives on a thread of its own, as
//! SQLite's does. Statements run in autocommit, the ODBC default. Cancel
//! calls `SQLCancel` on the running statement from the UI side, which ODBC
//! allows from another thread.

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use async_trait::async_trait;
use odbc_api::buffers::TextRowSet;
use odbc_api::handles::{AsStatementRef, Statement};
use odbc_api::{ConnectionOptions, Cursor, DataType, Environment, ResultSetMetadata};
use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::schema::{ColumnDef, ForeignKey, IndexDef, TableSchema};
use crate::sql::quote_literal;

/// The longest text a cell is read as; longer values are cut by the driver.
const MAX_TEXT: usize = 64 * 1024;

type Conn = odbc_api::Connection<'static>;
type Job = Box<dyn FnOnce(&mut Conn) + Send>;

fn environment() -> std::result::Result<&'static Environment, DbError> {
    static ENV: OnceLock<std::result::Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| Environment::new().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| DbError::msg(format!("ODBC is not available: {e}")))
}

/// The ODBC drivers installed on this machine, by name, for the form.
pub fn installed_drivers() -> Vec<String> {
    environment()
        .ok()
        .and_then(|env| env.drivers().ok())
        .map(|d| d.into_iter().map(|d| d.description).collect())
        .unwrap_or_default()
}

/// The installed driver for `engine` when the profile names none.
pub fn default_driver(engine: Engine, installed: &[String]) -> Option<String> {
    let wanted: &[&str] = match engine {
        Engine::Oracle => &["oracle"],
        Engine::Db2 => &["db2", "ibm data server"],
        _ => &[],
    };
    installed
        .iter()
        .find(|d| {
            let l = d.to_ascii_lowercase();
            wanted.iter().any(|w| l.contains(w))
        })
        .cloned()
}

/// A connection-string value, braced when it would otherwise end the pair.
fn cs_value(v: &str) -> String {
    if v.contains([';', '{', '}', '=']) || v.trim() != v {
        format!("{{{}}}", v.replace('}', "}}"))
    } else {
        v.to_string()
    }
}

/// The connection string, never logged (it carries the password).
pub fn connection_string(cfg: &ConnectConfig, driver: &str) -> String {
    let pwd = cfg.password.as_deref().unwrap_or("");
    match cfg.engine {
        // Oracle's driver takes an Easy Connect string as DBQ.
        Engine::Oracle => format!(
            "DRIVER={};DBQ={};UID={};PWD={};",
            cs_value(driver),
            cs_value(&format!("{}:{}/{}", cfg.host, cfg.port, cfg.database)),
            cs_value(&cfg.user),
            cs_value(pwd)
        ),
        _ => format!(
            "DRIVER={};DATABASE={};HOSTNAME={};PORT={};PROTOCOL=TCPIP;UID={};PWD={};",
            cs_value(driver),
            cs_value(&cfg.database),
            cs_value(&cfg.host),
            cfg.port,
            cs_value(&cfg.user),
            cs_value(pwd)
        ),
    }
}

/// What differs between the ODBC engines: their catalogue.
struct Dialect {
    engine: Engine,
}

impl Dialect {
    fn version(&self) -> &'static str {
        match self.engine {
            Engine::Oracle => {
                "SELECT banner, SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM v$version WHERE ROWNUM = 1"
            }
            _ => "SELECT service_level, CURRENT SCHEMA FROM TABLE(sysproc.env_get_inst_info())",
        }
    }

    fn schemas(&self) -> &'static str {
        match self.engine {
            Engine::Oracle => {
                "SELECT username FROM all_users WHERE oracle_maintained = 'N' \
                 ORDER BY CASE WHEN username = SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') THEN 0 ELSE 1 END, username"
            }
            _ => {
                "SELECT RTRIM(schemaname) FROM syscat.schemata \
                 WHERE schemaname NOT LIKE 'SYS%' AND schemaname NOT IN ('NULLID', 'SQLJ') \
                 ORDER BY CASE WHEN schemaname = CURRENT SCHEMA THEN 0 ELSE 1 END, schemaname"
            }
        }
    }

    fn objects(&self, schema: &str, kind: ObjectKind) -> Option<String> {
        let s = quote_literal(schema, self.engine);
        Some(match (self.engine, kind) {
            (Engine::Oracle, ObjectKind::Table) => format!(
                "SELECT table_name, '~' || NVL(num_rows, 0) || ' rows' FROM all_tables \
                 WHERE owner = {s} ORDER BY table_name"
            ),
            (Engine::Oracle, ObjectKind::View) => {
                format!("SELECT view_name, '' FROM all_views WHERE owner = {s} ORDER BY view_name")
            }
            (Engine::Oracle, ObjectKind::Function) => format!(
                "SELECT object_name, LOWER(object_type) FROM all_objects WHERE owner = {s} \
                 AND object_type IN ('FUNCTION', 'PROCEDURE', 'PACKAGE') ORDER BY object_name"
            ),
            (Engine::Oracle, ObjectKind::Sequence) => format!(
                "SELECT sequence_name, '' FROM all_sequences WHERE sequence_owner = {s} ORDER BY sequence_name"
            ),
            (Engine::Db2, ObjectKind::Table) => format!(
                "SELECT RTRIM(tabname), '~' || CAST(MAX(card, 0) AS VARCHAR(20)) || ' rows' FROM syscat.tables \
                 WHERE tabschema = {s} AND type = 'T' ORDER BY tabname"
            ),
            (Engine::Db2, ObjectKind::View) => format!(
                "SELECT RTRIM(tabname), '' FROM syscat.tables WHERE tabschema = {s} AND type = 'V' ORDER BY tabname"
            ),
            (Engine::Db2, ObjectKind::Function) => format!(
                "SELECT RTRIM(routinename), CASE routinetype WHEN 'P' THEN 'procedure' ELSE 'function' END \
                 FROM syscat.routines WHERE routineschema = {s} ORDER BY routinename"
            ),
            _ => return None,
        })
    }

    fn catalog(&self) -> &'static str {
        match self.engine {
            Engine::Oracle => {
                "SELECT c.owner, c.table_name, c.column_name FROM all_tab_columns c \
                 JOIN all_users u ON u.username = c.owner AND u.oracle_maintained = 'N' \
                 ORDER BY c.owner, c.table_name, c.column_id"
            }
            _ => {
                "SELECT RTRIM(tabschema), RTRIM(tabname), RTRIM(colname) FROM syscat.columns \
                 WHERE tabschema NOT LIKE 'SYS%' ORDER BY tabschema, tabname, colno"
            }
        }
    }

    /// name, type, nullable (Y/N), default, identity (Y/N)
    fn columns(&self, s: &str, t: &str) -> String {
        match self.engine {
            Engine::Oracle => format!(
                "SELECT column_name, \
                   CASE WHEN data_type IN ('VARCHAR2', 'NVARCHAR2', 'CHAR', 'NCHAR', 'RAW') THEN data_type || '(' || char_length || ')' \
                        WHEN data_type = 'NUMBER' AND data_precision IS NOT NULL THEN 'NUMBER(' || data_precision || ',' || data_scale || ')' \
                        ELSE data_type END, \
                   nullable, data_default, identity_column \
                 FROM all_tab_columns WHERE owner = {s} AND table_name = {t} ORDER BY column_id"
            ),
            _ => format!(
                "SELECT RTRIM(colname), \
                   CASE WHEN typename IN ('VARCHAR', 'CHARACTER', 'VARGRAPHIC', 'GRAPHIC') THEN RTRIM(typename) || '(' || length || ')' \
                        WHEN typename = 'DECIMAL' THEN 'DECIMAL(' || length || ',' || scale || ')' \
                        ELSE RTRIM(typename) END, \
                   nulls, default, identity \
                 FROM syscat.columns WHERE tabschema = {s} AND tabname = {t} ORDER BY colno"
            ),
        }
    }

    /// name, unique (Y/N), primary (Y/N), column - one row per column, in order
    fn indexes(&self, s: &str, t: &str) -> String {
        match self.engine {
            Engine::Oracle => format!(
                "SELECT i.index_name, CASE WHEN i.uniqueness = 'UNIQUE' THEN 'Y' ELSE 'N' END, \
                   CASE WHEN EXISTS (SELECT 1 FROM all_constraints c WHERE c.owner = i.table_owner \
                     AND c.index_name = i.index_name AND c.constraint_type = 'P') THEN 'Y' ELSE 'N' END, \
                   ic.column_name \
                 FROM all_indexes i JOIN all_ind_columns ic ON ic.index_owner = i.owner AND ic.index_name = i.index_name \
                 WHERE i.table_owner = {s} AND i.table_name = {t} ORDER BY i.index_name, ic.column_position"
            ),
            _ => format!(
                "SELECT RTRIM(i.indname), CASE WHEN i.uniquerule IN ('U', 'P') THEN 'Y' ELSE 'N' END, \
                   CASE WHEN i.uniquerule = 'P' THEN 'Y' ELSE 'N' END, RTRIM(c.colname) \
                 FROM syscat.indexes i JOIN syscat.indexcoluse c ON c.indschema = i.indschema AND c.indname = i.indname \
                 WHERE i.tabschema = {s} AND i.tabname = {t} ORDER BY i.indname, c.colseq"
            ),
        }
    }

    /// name, ref schema, ref table, column, ref column, delete rule
    fn foreign_keys(&self, s: &str, t: &str) -> String {
        match self.engine {
            Engine::Oracle => format!(
                "SELECT c.constraint_name, r.owner, r.table_name, cc.column_name, rc.column_name, c.delete_rule \
                 FROM all_constraints c \
                 JOIN all_constraints r ON r.owner = c.r_owner AND r.constraint_name = c.r_constraint_name \
                 JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
                 JOIN all_cons_columns rc ON rc.owner = r.owner AND rc.constraint_name = r.constraint_name \
                   AND rc.position = cc.position \
                 WHERE c.owner = {s} AND c.table_name = {t} AND c.constraint_type = 'R' \
                 ORDER BY c.constraint_name, cc.position"
            ),
            _ => format!(
                "SELECT RTRIM(r.constname), RTRIM(r.reftabschema), RTRIM(r.reftabname), RTRIM(k.colname), RTRIM(rk.colname), \
                   CASE r.deleterule WHEN 'C' THEN 'CASCADE' WHEN 'N' THEN 'SET NULL' WHEN 'R' THEN 'RESTRICT' ELSE 'NO ACTION' END \
                 FROM syscat.references r \
                 JOIN syscat.keycoluse k ON k.constname = r.constname AND k.tabschema = r.tabschema AND k.tabname = r.tabname \
                 JOIN syscat.keycoluse rk ON rk.constname = r.refkeyname AND rk.tabschema = r.reftabschema \
                   AND rk.tabname = r.reftabname AND rk.colseq = k.colseq \
                 WHERE r.tabschema = {s} AND r.tabname = {t} ORDER BY r.constname, k.colseq"
            ),
        }
    }
}

/// The statement running now, as a raw handle, so Cancel can reach it.
/// `usize` because the handle is a pointer, which is not `Send`.
type Running = Arc<Mutex<Option<usize>>>;

pub struct OdbcSession {
    engine: Engine,
    jobs: std::sync::mpsc::Sender<Job>,
    running: Running,
    info: ServerInfo,
}

enum Msg {
    Columns(Vec<Column>),
    Rows(Vec<Row>),
}

fn odbc_err(e: odbc_api::Error) -> DbError {
    match &e {
        odbc_api::Error::Diagnostics { record, .. } => DbError {
            message: record.to_string(),
            position: None,
            code: Some(String::from_utf8_lossy(&record.state.0).to_string()),
        },
        other => DbError::msg(other.to_string()),
    }
}

fn is_number(t: &DataType) -> bool {
    matches!(
        t,
        DataType::Integer
            | DataType::SmallInt
            | DataType::TinyInt
            | DataType::BigInt
            | DataType::Decimal { .. }
            | DataType::Numeric { .. }
            | DataType::Float { .. }
            | DataType::Real
            | DataType::Double
    )
}

fn cell(text: Option<&str>, number: bool) -> Value {
    match text {
        None => Value::Null,
        Some(t) if number => match t.parse::<i64>() {
            Ok(i) => Value::Int(i),
            Err(_) => Value::Number(t.to_string()),
        },
        Some(t) => Value::Text(t.to_string()),
    }
}

impl OdbcSession {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Self> {
        let env = environment()?;
        let driver = if cfg.odbc_driver.trim().is_empty() {
            default_driver(cfg.engine, &installed_drivers()).ok_or_else(|| {
                DbError::msg(format!(
                    "no {} ODBC driver is installed on this machine",
                    cfg.engine.label()
                ))
            })?
        } else {
            cfg.odbc_driver.trim().to_string()
        };
        let cs = connection_string(cfg, &driver);
        let timeout = cfg.connect_timeout.as_secs().max(1) as u32;
        let (ready_tx, ready_rx) = oneshot::channel();
        let (jobs, rx) = std::sync::mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("hs-odbc".into())
            .spawn(move || {
                let opts = ConnectionOptions {
                    login_timeout_sec: Some(timeout),
                    ..Default::default()
                };
                let mut conn = match env.connect_with_connection_string(&cs, opts) {
                    Ok(c) => {
                        let _ = ready_tx.send(Ok(()));
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(odbc_err(e)));
                        return;
                    }
                };
                while let Ok(job) = rx.recv() {
                    job(&mut conn);
                }
            })
            .map_err(|e| DbError::msg(e.to_string()))?;
        ready_rx
            .await
            .map_err(|_| DbError::msg("the ODBC thread stopped"))??;
        let mut s = Self {
            engine: cfg.engine,
            jobs,
            running: Arc::new(Mutex::new(None)),
            info: ServerInfo::default(),
        };
        let d = Dialect { engine: s.engine };
        let r = s
            .rows_of(d.version().into())
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        let banner = r.first().cloned().unwrap_or_default();
        s.info = ServerInfo {
            version: if banner.is_empty() {
                cfg.engine.label().to_string()
            } else if banner.starts_with(cfg.engine.label()) || banner.contains("Oracle") {
                banner
            } else {
                format!("{} {banner}", cfg.engine.label())
            },
            database: r.get(1).cloned().unwrap_or_default(),
        };
        Ok(s)
    }

    async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Conn) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |c| {
                let _ = tx.send(f(c));
            }))
            .map_err(|_| DbError::msg("the ODBC connection is closed"))?;
        rx.await
            .map_err(|_| DbError::msg("the ODBC connection is closed"))?
    }

    async fn rows_of(&self, sql: String) -> Result<Vec<Vec<String>>> {
        self.call(move |c| {
            let mut out = Vec::new();
            let Some(mut cursor) = c.execute(&sql, (), None).map_err(odbc_err)? else {
                return Ok(out);
            };
            let mut buf =
                TextRowSet::for_cursor(BATCH, &mut cursor, Some(MAX_TEXT)).map_err(odbc_err)?;
            let mut block = cursor.bind_buffer(&mut buf).map_err(odbc_err)?;
            while let Some(b) = block.fetch().map_err(odbc_err)? {
                for row in 0..b.num_rows() {
                    out.push(
                        (0..b.num_cols())
                            .map(|col| {
                                b.at_as_str(col, row)
                                    .ok()
                                    .flatten()
                                    .unwrap_or("")
                                    .trim_end()
                                    .to_string()
                            })
                            .collect(),
                    );
                }
            }
            Ok(out)
        })
        .await
    }
}

#[async_trait]
impl DbSession for OdbcSession {
    fn engine(&self) -> Engine {
        self.engine
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let sql = sql.to_string();
        let running = self.running.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let (tx, mut rx) = mpsc::channel::<Msg>(4);
        let done = self.call(move |c| {
            let mut stmt = c.preallocate().map_err(odbc_err)?;
            *running.lock() = Some(stmt.as_stmt_ref().as_sys().0 as usize);
            let run = (|| -> Result<ExecSummary> {
                let mut summary = ExecSummary::default();
                let Some(mut cursor) = stmt.execute(&sql, ()).map_err(odbc_err)? else {
                    return Ok(summary);
                };
                let n = cursor.num_result_cols().map_err(odbc_err)?.max(0) as u16;
                let mut cols = Vec::with_capacity(n as usize);
                let mut numbers = Vec::with_capacity(n as usize);
                for i in 1..=n {
                    let ty = cursor.col_data_type(i).map_err(odbc_err)?;
                    numbers.push(is_number(&ty));
                    cols.push(Column {
                        name: cursor.col_name(i).map_err(odbc_err)?,
                        type_name: format!("{ty:?}"),
                    });
                }
                summary.has_result = true;
                let _ = tx.blocking_send(Msg::Columns(cols));
                let mut buf =
                    TextRowSet::for_cursor(BATCH, &mut cursor, Some(MAX_TEXT)).map_err(odbc_err)?;
                let mut block = cursor.bind_buffer(&mut buf).map_err(odbc_err)?;
                while let Some(b) = block.fetch().map_err(odbc_err)? {
                    let rows: Vec<Row> = (0..b.num_rows())
                        .map(|row| {
                            (0..b.num_cols())
                                .map(|col| cell(b.at_as_str(col, row).ok().flatten(), numbers[col]))
                                .collect()
                        })
                        .collect();
                    summary.rows_returned += rows.len() as u64;
                    if tx.blocking_send(Msg::Rows(rows)).is_err()
                        || stop2.load(std::sync::atomic::Ordering::Relaxed)
                    {
                        summary.limited = true;
                        break;
                    }
                }
                Ok(summary)
            })();
            // The handle is about to be freed: Cancel must not reach it.
            *running.lock() = None;
            let mut summary = run?;
            if !summary.has_result {
                summary.rows_affected = stmt.row_count().ok().flatten().map(|n| n as u64);
            }
            drop(tx);
            Ok(summary)
        });
        let forward = async {
            let mut stopped = false;
            while let Some(m) = rx.recv().await {
                match m {
                    Msg::Columns(c) => sink.columns(c),
                    Msg::Rows(r) if !stopped => {
                        if sink.rows(r) == Flow::Stop {
                            stopped = true;
                            stop.store(true, std::sync::atomic::Ordering::Relaxed);
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
        let running = self.running.clone();
        Arc::new(move || {
            let guard = running.lock();
            if let Some(h) = *guard {
                // SAFETY: `h` is the handle of the statement executing on
                // the session thread. That thread clears it under this same
                // lock before the statement is freed, so while the lock is
                // held the handle is alive; SQLCancel is the one ODBC call
                // meant to be made from another thread during execution.
                unsafe {
                    let _ =
                        odbc_api::sys::SQLCancel(odbc_api::sys::HStmt(h as *mut std::ffi::c_void));
                }
            }
        })
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        let d = Dialect {
            engine: self.engine,
        };
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(d.schemas().into())
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Schema, r[0].clone(), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let Some(sql) = d.objects(schema, *kind) else {
                    return Ok(Vec::new());
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
                        let badges = if ix.primary {
                            vec!["PK".to_string()]
                        } else if ix.unique {
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
        let d = Dialect {
            engine: self.engine,
        };
        let rows = self.rows_of(d.catalog().into()).await?;
        Ok(super::postgres::group_catalog(
            rows.into_iter()
                .map(|r| (r[0].clone(), r[1].clone(), r[2].clone())),
        ))
    }

    async fn table_schema(&mut self, schema: &str, table: &str) -> Result<TableSchema> {
        let d = Dialect {
            engine: self.engine,
        };
        let (s, t) = (
            quote_literal(schema, self.engine),
            quote_literal(table, self.engine),
        );
        let columns = self
            .rows_of(d.columns(&s, &t))
            .await?
            .into_iter()
            .map(|r| ColumnDef {
                name: r[0].clone(),
                data_type: r[1].clone(),
                nullable: r[2] == "Y",
                default: Some(r[3].trim().to_string()).filter(|x| !x.is_empty()),
                auto_increment: r[4] == "Y" || r[4] == "YES",
            })
            .collect();
        let mut indexes: Vec<IndexDef> = Vec::new();
        for r in self.rows_of(d.indexes(&s, &t)).await? {
            match indexes.last_mut() {
                Some(ix) if ix.name == r[0] => ix.columns.push(r[3].clone()),
                _ => indexes.push(IndexDef {
                    name: r[0].clone(),
                    columns: vec![r[3].clone()],
                    unique: r[1] == "Y",
                    primary: r[2] == "Y",
                }),
            }
        }
        let primary_key = indexes
            .iter()
            .find(|ix| ix.primary)
            .map(|ix| ix.columns.clone())
            .unwrap_or_default();
        let mut foreign_keys: Vec<ForeignKey> = Vec::new();
        for r in self.rows_of(d.foreign_keys(&s, &t)).await? {
            match foreign_keys.last_mut() {
                Some(fk) if fk.name == r[0] => {
                    fk.columns.push(r[3].clone());
                    fk.ref_columns.push(r[4].clone());
                }
                _ => foreign_keys.push(ForeignKey {
                    name: r[0].clone(),
                    columns: vec![r[3].clone()],
                    ref_schema: r[1].clone(),
                    ref_table: r[2].clone(),
                    ref_columns: vec![r[4].clone()],
                    on_delete: crate::schema::referential_action(&r[5]),
                    on_update: None,
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
        // Autocommit: every statement is its own transaction.
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_strings_name_the_driver_and_brace_awkward_values() {
        let cfg = ConnectConfig {
            engine: Engine::Oracle,
            host: "db".into(),
            port: 1521,
            database: "FREEPDB1".into(),
            user: "app".into(),
            password: Some("p;w}".into()),
            ..Default::default()
        };
        assert_eq!(
            connection_string(&cfg, "Oracle in instantclient_23_9"),
            "DRIVER=Oracle in instantclient_23_9;DBQ=db:1521/FREEPDB1;UID=app;PWD={p;w}}};"
        );
        let db2 = ConnectConfig {
            engine: Engine::Db2,
            port: 50000,
            database: "SAMPLE".into(),
            password: None,
            ..cfg
        };
        assert_eq!(
            connection_string(&db2, "IBM DB2 ODBC DRIVER"),
            "DRIVER=IBM DB2 ODBC DRIVER;DATABASE=SAMPLE;HOSTNAME=db;PORT=50000;PROTOCOL=TCPIP;UID=app;PWD=;"
        );
    }

    #[test]
    fn the_engines_driver_is_found_among_the_installed_ones() {
        let installed = vec![
            "SQL Server".to_string(),
            "Oracle in instantclient_23_9".to_string(),
            "IBM DB2 ODBC DRIVER - clidriver".to_string(),
        ];
        assert_eq!(
            default_driver(Engine::Oracle, &installed).as_deref(),
            Some("Oracle in instantclient_23_9")
        );
        assert_eq!(
            default_driver(Engine::Db2, &installed).as_deref(),
            Some("IBM DB2 ODBC DRIVER - clidriver")
        );
        assert_eq!(default_driver(Engine::Oracle, &installed[..1]), None);
    }
}
