//! Microsoft SQL Server and Azure SQL through `tiberius` (TDS 8, rustls with
//! ring; ADR-030).
//!
//! `tiberius` has no way to send a TDS attention, so Cancel runs
//! `KILL <spid>` from a second connection. That ends the whole session, so
//! the next statement opens a new one first. A result cut short by the row
//! limit is drained when a transaction is open (killing it would roll the
//! transaction back) and ended the same way otherwise.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use async_trait::async_trait;
use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, ColumnData, Config, EncryptionLevel, FromSql, QueryItem};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use super::{
    BATCH, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, Engine, ExecSummary,
    Flow, ObjectKind, Result, ResultSink, Row, SchemaObject, Scope, ServerInfo, Value,
};
use crate::schema::{ColumnDef, ForeignKey, IndexDef, TableSchema};
use crate::sql::{Class, quote_literal};

type Conn = Client<Compat<TcpStream>>;

pub struct MssqlSession {
    conn: Option<Conn>,
    cfg: ConnectConfig,
    spid: i64,
    info: ServerInfo,
    runtime: tokio::runtime::Handle,
    in_tx: bool,
    /// Set by Cancel: the session was killed and must be opened again.
    killed: Arc<AtomicBool>,
    /// The spid Cancel kills, kept current across reconnects.
    spid_shared: Arc<parking_lot::Mutex<i64>>,
}

fn ms_err(e: tiberius::error::Error) -> DbError {
    match &e {
        tiberius::error::Error::Server(t) => DbError {
            message: t.message().to_string(),
            position: None,
            code: Some(t.code().to_string()),
        },
        other => DbError::msg(other.to_string()),
    }
}

fn config(cfg: &ConnectConfig) -> Config {
    let mut c = Config::new();
    c.host(&cfg.host);
    c.port(cfg.port);
    if !cfg.database.is_empty() {
        c.database(&cfg.database);
    }
    c.authentication(AuthMethod::sql_server(
        &cfg.user,
        cfg.password.as_deref().unwrap_or(""),
    ));
    c.application_name("HyperStack");
    // SQL Server always encrypts the login with TLS, even when the session
    // is not encrypted, and the certificate it generates for itself is an
    // X.509 v1 one rustls refuses. Only the verify modes check it, as
    // libpq's `require` does not either.
    match cfg.ssl.as_str() {
        // The login only, as SQL Server itself defaults to.
        "disable" => c.encryption(EncryptionLevel::Off),
        "prefer" => c.encryption(EncryptionLevel::On),
        _ => c.encryption(EncryptionLevel::Required),
    }
    if !super::tls::verifies(&cfg.ssl) {
        c.trust_cert();
    } else if let Some(ca) = cfg.ssl_ca.as_ref().filter(|x| !x.trim().is_empty()) {
        c.trust_cert_ca(ca);
    }
    c
}

async fn open(cfg: &ConnectConfig) -> Result<Conn> {
    super::mysql::install_crypto_provider();
    let mut c = config(cfg);
    let attempt = async {
        // Azure SQL may redirect the login to another node once.
        for _ in 0..2 {
            let tcp = TcpStream::connect(c.get_addr())
                .await
                .map_err(|e| DbError::msg(e.to_string()))?;
            tcp.set_nodelay(true).ok();
            match Client::connect(c.clone(), tcp.compat_write()).await {
                Ok(client) => return Ok(client),
                Err(tiberius::error::Error::Routing { host, port }) => {
                    c.host(&host);
                    c.port(port);
                }
                Err(e) => return Err(ms_err(e)),
            }
        }
        Err(DbError::msg(
            "the server redirected the login more than once",
        ))
    };
    tokio::time::timeout(cfg.connect_timeout, attempt)
        .await
        .map_err(|_| DbError::msg("timed out connecting"))?
}

/// One cell. Dates and times are read through chrono, so they show the
/// way the server writes them.
fn convert(d: ColumnData<'static>) -> Value {
    use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
    let date_text = |d: &ColumnData<'static>| -> Option<String> {
        if let Ok(Some(v)) = NaiveDateTime::from_sql(d) {
            return Some(v.format("%Y-%m-%d %H:%M:%S%.f").to_string());
        }
        if let Ok(Some(v)) = DateTime::<FixedOffset>::from_sql(d) {
            return Some(v.format("%Y-%m-%d %H:%M:%S%.f %:z").to_string());
        }
        if let Ok(Some(v)) = NaiveDate::from_sql(d) {
            return Some(v.to_string());
        }
        if let Ok(Some(v)) = NaiveTime::from_sql(d) {
            return Some(v.to_string());
        }
        None
    };
    match d {
        ColumnData::U8(Some(v)) => Value::Int(v as i64),
        ColumnData::I16(Some(v)) => Value::Int(v as i64),
        ColumnData::I32(Some(v)) => Value::Int(v as i64),
        ColumnData::I64(Some(v)) => Value::Int(v),
        ColumnData::F32(Some(v)) => Value::Float(v as f64),
        ColumnData::F64(Some(v)) => Value::Float(v),
        ColumnData::Bit(Some(v)) => Value::Int(v as i64),
        ColumnData::String(Some(s)) => Value::Text(s.into_owned()),
        ColumnData::Guid(Some(g)) => Value::Text(g.to_string().to_uppercase()),
        ColumnData::Binary(Some(b)) => Value::Bytes(b.into_owned()),
        ColumnData::Numeric(Some(n)) => Value::Number(n.to_string()),
        ColumnData::Xml(Some(x)) => Value::Text(x.to_string()),
        other @ (ColumnData::DateTime(Some(_))
        | ColumnData::SmallDateTime(Some(_))
        | ColumnData::Time(Some(_))
        | ColumnData::Date(Some(_))
        | ColumnData::DateTime2(Some(_))
        | ColumnData::DateTimeOffset(Some(_))) => {
            date_text(&other).map(Value::Text).unwrap_or(Value::Null)
        }
        _ => Value::Null,
    }
}

/// A statement whose result sets are the point: reads, and procedure calls.
fn returns_rows(sql: &str) -> bool {
    let c = crate::sql::classify(sql, Engine::Mssql);
    if c.class == Class::Read {
        return true;
    }
    let first = sql
        .trim_start()
        .split(|ch: char| ch.is_whitespace() || ch == '(')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(
        first.as_str(),
        "EXEC" | "EXECUTE" | "SP_HELP" | "SP_WHO" | "SP_WHO2"
    )
}

async fn kill(cfg: ConnectConfig, spid: i64) {
    if let Ok(mut c) = open(&cfg).await {
        let _ = c.simple_query(format!("KILL {spid}")).await;
    }
}

impl MssqlSession {
    pub async fn connect(cfg: &ConnectConfig) -> Result<Self> {
        let conn = open(cfg).await?;
        let mut s = Self {
            conn: Some(conn),
            cfg: cfg.clone(),
            spid: 0,
            info: ServerInfo::default(),
            runtime: tokio::runtime::Handle::current(),
            in_tx: false,
            killed: Arc::new(AtomicBool::new(false)),
            spid_shared: Arc::new(parking_lot::Mutex::new(0)),
        };
        s.after_open().await?;
        Ok(s)
    }

    async fn after_open(&mut self) -> Result<()> {
        let r = self
            .rows_of(
                "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(64)), \
                 CAST(SERVERPROPERTY('Edition') AS nvarchar(128)), DB_NAME(), @@SPID",
            )
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        let edition = r.get(1).cloned().unwrap_or_default();
        let product = if edition.contains("Azure") {
            "Azure SQL"
        } else {
            "SQL Server"
        };
        self.info = ServerInfo {
            version: format!("{product} {}", r.first().cloned().unwrap_or_default()),
            database: r.get(2).cloned().unwrap_or_default(),
        };
        self.spid = r.get(3).and_then(|v| v.parse().ok()).unwrap_or(0);
        *self.spid_shared.lock() = self.spid;
        self.in_tx = false;
        Ok(())
    }

    /// The connection, opened again when Cancel killed the last one.
    async fn conn(&mut self) -> Result<&mut Conn> {
        if self.killed.swap(false, Ordering::SeqCst) || self.conn.is_none() {
            self.conn = None;
            let c = open(&self.cfg).await?;
            self.conn = Some(c);
            self.after_open().await?;
        }
        self.conn
            .as_mut()
            .ok_or_else(|| DbError::msg("not connected"))
    }

    async fn rows_of(&mut self, sql: &str) -> Result<Vec<Vec<String>>> {
        let conn = self
            .conn
            .as_mut()
            .ok_or_else(|| DbError::msg("not connected"))?;
        let rows = conn
            .simple_query(sql)
            .await
            .map_err(ms_err)?
            .into_first_result()
            .await
            .map_err(ms_err)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                r.into_iter()
                    .map(|d| match convert(d) {
                        Value::Null => String::new(),
                        v => v.display(),
                    })
                    .collect()
            })
            .collect())
    }

    async fn probe_in_tx(&mut self) {
        if let Ok(r) = self.rows_of("SELECT @@TRANCOUNT").await {
            self.in_tx = r.first().and_then(|r| r.first()).is_some_and(|v| v != "0");
        }
    }
}

#[async_trait]
impl DbSession for MssqlSession {
    fn engine(&self) -> Engine {
        Engine::Mssql
    }

    fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary> {
        let started = Instant::now();
        let mut summary = ExecSummary::default();
        let in_tx = self.in_tx;
        let killed = self.killed.clone();
        let conn = self.conn().await?;
        if !returns_rows(sql) {
            let r = conn.execute(sql, &[]).await;
            let r = match r {
                Ok(r) => r,
                Err(e) if killed.load(Ordering::SeqCst) => {
                    return Err(DbError::msg(format!("cancelled ({})", ms_err(e).message)));
                }
                Err(e) => {
                    let err = ms_err(e);
                    self.probe_in_tx().await;
                    return Err(err);
                }
            };
            summary.rows_affected = Some(r.total());
            self.probe_in_tx().await;
            summary.elapsed = started.elapsed();
            return Ok(summary);
        }
        let mut stream = conn.simple_query(sql).await.map_err(ms_err)?;
        let mut batch: Vec<Row> = Vec::with_capacity(BATCH);
        let mut stopped = false;
        // The first result set is shown; later ones are read and dropped.
        let mut first_done = false;
        loop {
            let item = match stream.try_next().await {
                Ok(Some(i)) => i,
                Ok(None) => break,
                Err(e) if killed.load(Ordering::SeqCst) => {
                    return Err(DbError::msg(format!("cancelled ({})", ms_err(e).message)));
                }
                Err(e) => return Err(ms_err(e)),
            };
            match item {
                QueryItem::Metadata(m) => {
                    if summary.has_result {
                        first_done = true;
                        continue;
                    }
                    summary.has_result = true;
                    sink.columns(
                        m.columns()
                            .iter()
                            .map(|c| Column {
                                name: c.name().to_string(),
                                type_name: format!("{:?}", c.column_type()),
                            })
                            .collect(),
                    );
                }
                QueryItem::Row(r) => {
                    if first_done || stopped {
                        continue;
                    }
                    batch.push(r.into_iter().map(convert).collect());
                    summary.rows_returned += 1;
                    if batch.len() >= BATCH && sink.rows(std::mem::take(&mut batch)) == Flow::Stop {
                        stopped = true;
                        if !in_tx {
                            // End it rather than reading the rest; the
                            // session is opened again for the next one.
                            break;
                        }
                    }
                }
            }
        }
        drop(stream);
        if !stopped && !batch.is_empty() && sink.rows(batch) == Flow::Stop {
            stopped = true;
        }
        if stopped {
            summary.limited = true;
            if !in_tx {
                let spid = self.spid;
                self.killed.store(true, Ordering::SeqCst);
                kill(self.cfg.clone(), spid).await;
                self.conn = None;
                summary.elapsed = started.elapsed();
                return Ok(summary);
            }
        }
        self.probe_in_tx().await;
        summary.elapsed = started.elapsed();
        Ok(summary)
    }

    fn canceller(&self) -> Canceller {
        let cfg = self.cfg.clone();
        let spid = self.spid_shared.clone();
        let killed = self.killed.clone();
        let rt = self.runtime.clone();
        Arc::new(move || {
            killed.store(true, Ordering::SeqCst);
            rt.spawn(kill(cfg.clone(), *spid.lock()));
        })
    }

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>> {
        self.conn().await?;
        let e = Engine::Mssql;
        let o = |kind, name: String, detail: String, badges: Vec<String>| SchemaObject {
            kind,
            name,
            detail,
            badges,
        };
        Ok(match scope {
            Scope::Schemas => self
                .rows_of(
                    "SELECT s.name FROM sys.schemas s WHERE s.schema_id < 16384 \
                     AND s.name NOT IN ('sys', 'INFORMATION_SCHEMA', 'guest') \
                     AND (s.name = 'dbo' OR EXISTS (SELECT 1 FROM sys.objects o WHERE o.schema_id = s.schema_id)) \
                     ORDER BY CASE WHEN s.name = 'dbo' THEN 0 ELSE 1 END, s.name",
                )
                .await?
                .into_iter()
                .map(|r| o(ObjectKind::Schema, r[0].clone(), String::new(), vec![]))
                .collect(),
            Scope::Objects { schema, kind } => {
                let s = quote_literal(schema, e);
                let sql = match kind {
                    ObjectKind::Table => format!(
                        "SELECT t.name, CONCAT('~', SUM(p.rows), ' rows') FROM sys.tables t \
                         JOIN sys.schemas s ON s.schema_id = t.schema_id \
                         LEFT JOIN sys.partitions p ON p.object_id = t.object_id AND p.index_id IN (0, 1) \
                         WHERE s.name = {s} GROUP BY t.name ORDER BY t.name"
                    ),
                    ObjectKind::View => format!(
                        "SELECT v.name, '' FROM sys.views v JOIN sys.schemas s ON s.schema_id = v.schema_id \
                         WHERE s.name = {s} ORDER BY v.name"
                    ),
                    ObjectKind::Function => format!(
                        "SELECT o.name, LOWER(REPLACE(o.type_desc, '_', ' ')) FROM sys.objects o \
                         JOIN sys.schemas s ON s.schema_id = o.schema_id \
                         WHERE s.name = {s} AND o.type IN ('FN', 'IF', 'TF', 'P') ORDER BY o.name"
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
                let t = table_schema_of(self, schema, table).await?;
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
                        if c.auto_increment {
                            badges.push("IDENTITY".to_string());
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
                        o(ObjectKind::Column, c.name.clone(), c.data_type.clone(), badges)
                    })
                    .collect()
            }
            Scope::Indexes { schema, table } => {
                let t = table_schema_of(self, schema, table).await?;
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
                        o(ObjectKind::Index, ix.name.clone(), format!("({})", ix.columns.join(", ")), badges)
                    })
                    .collect()
            }
        })
    }

    async fn catalog(&mut self) -> Result<Catalog> {
        self.conn().await?;
        let rows = self
            .rows_of(
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS \
                 ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION",
            )
            .await?;
        Ok(super::postgres::group_catalog(
            rows.into_iter()
                .map(|r| (r[0].clone(), r[1].clone(), r[2].clone())),
        ))
    }

    async fn table_schema(&mut self, schema: &str, table: &str) -> Result<TableSchema> {
        self.conn().await?;
        table_schema_of(self, schema, table).await
    }

    fn in_transaction(&self) -> bool {
        self.in_tx
    }
}

/// `nvarchar(64)`, `decimal(10,2)`, `varbinary(max)` from sys.columns.
fn type_sql(name: &str, max_length: i64, precision: i64, scale: i64) -> String {
    match name {
        "varchar" | "char" | "varbinary" | "binary" => {
            if max_length == -1 {
                format!("{name}(max)")
            } else {
                format!("{name}({max_length})")
            }
        }
        // Stored in bytes, declared in characters.
        "nvarchar" | "nchar" => {
            if max_length == -1 {
                format!("{name}(max)")
            } else {
                format!("{name}({})", max_length / 2)
            }
        }
        "decimal" | "numeric" => format!("{name}({precision},{scale})"),
        "datetime2" | "time" | "datetimeoffset" => format!("{name}({scale})"),
        _ => name.to_string(),
    }
}

async fn table_schema_of(s: &mut MssqlSession, schema: &str, table: &str) -> Result<TableSchema> {
    let e = Engine::Mssql;
    let obj = format!(
        "OBJECT_ID({})",
        quote_literal(
            &format!(
                "{}.{}",
                crate::sql::quote_ident(schema, Engine::Mssql),
                crate::sql::quote_ident(table, Engine::Mssql)
            ),
            e
        )
    );
    let cols = s
        .rows_of(&format!(
            "SELECT c.name, ty.name, c.max_length, c.precision, c.scale, c.is_nullable, c.is_identity, \
               dc.definition \
             FROM sys.columns c JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
             LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
             WHERE c.object_id = {obj} ORDER BY c.column_id"
        ))
        .await?;
    let int = |v: &str| v.parse::<i64>().unwrap_or(0);
    let columns = cols
        .iter()
        .map(|r| ColumnDef {
            name: r[0].clone(),
            data_type: type_sql(&r[1], int(&r[2]), int(&r[3]), int(&r[4])),
            nullable: r[5] == "1",
            default: Some(r[7].clone()).filter(|d| !d.is_empty()),
            auto_increment: r[6] == "1",
        })
        .collect();
    let ix_rows = s
        .rows_of(&format!(
            "SELECT i.name, i.is_unique, i.is_primary_key, col.name \
             FROM sys.indexes i \
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             JOIN sys.columns col ON col.object_id = ic.object_id AND col.column_id = ic.column_id \
             WHERE i.object_id = {obj} AND i.name IS NOT NULL AND ic.is_included_column = 0 \
             ORDER BY i.name, ic.key_ordinal"
        ))
        .await?;
    let mut indexes: Vec<IndexDef> = Vec::new();
    for r in &ix_rows {
        match indexes.last_mut() {
            Some(ix) if ix.name == r[0] => ix.columns.push(r[3].clone()),
            _ => indexes.push(IndexDef {
                name: r[0].clone(),
                columns: vec![r[3].clone()],
                unique: r[1] == "1",
                primary: r[2] == "1",
            }),
        }
    }
    let primary_key = indexes
        .iter()
        .find(|ix| ix.primary)
        .map(|ix| ix.columns.clone())
        .unwrap_or_default();
    let fk_rows = s
        .rows_of(&format!(
            "SELECT fk.name, rs.name, rt.name, pc.name, rc.name, \
               fk.delete_referential_action_desc, fk.update_referential_action_desc \
             FROM sys.foreign_keys fk \
             JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.columns pc ON pc.object_id = fkc.parent_object_id AND pc.column_id = fkc.parent_column_id \
             JOIN sys.columns rc ON rc.object_id = fkc.referenced_object_id AND rc.column_id = fkc.referenced_column_id \
             JOIN sys.tables rt ON rt.object_id = fk.referenced_object_id \
             JOIN sys.schemas rs ON rs.schema_id = rt.schema_id \
             WHERE fk.parent_object_id = {obj} ORDER BY fk.name, fkc.constraint_column_id"
        ))
        .await?;
    let mut foreign_keys: Vec<ForeignKey> = Vec::new();
    for r in &fk_rows {
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
                on_delete: crate::schema::referential_action(&r[5].replace('_', " ")),
                on_update: crate::schema::referential_action(&r[6].replace('_', " ")),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_written_the_way_they_are_declared() {
        assert_eq!(type_sql("nvarchar", 64, 0, 0), "nvarchar(32)");
        assert_eq!(type_sql("varbinary", -1, 0, 0), "varbinary(max)");
        assert_eq!(type_sql("decimal", 9, 10, 2), "decimal(10,2)");
        assert_eq!(type_sql("int", 4, 10, 0), "int");
    }

    #[test]
    fn reads_and_procedures_stream_rows_and_writes_count_them() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("WITH a AS (SELECT 1 x) SELECT * FROM a"));
        assert!(returns_rows("EXEC sp_who"));
        assert!(!returns_rows("UPDATE t SET a = 1"));
        assert!(!returns_rows("CREATE TABLE t (a int)"));
    }

    #[test]
    fn plain_values_convert() {
        assert_eq!(convert(ColumnData::I32(Some(7))), Value::Int(7));
        assert_eq!(convert(ColumnData::I32(None)), Value::Null);
        assert_eq!(
            convert(ColumnData::String(Some("x".into()))),
            Value::Text("x".into())
        );
        assert_eq!(convert(ColumnData::Bit(Some(true))), Value::Int(1));
    }
}
