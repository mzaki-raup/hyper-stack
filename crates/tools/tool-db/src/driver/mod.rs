//! The driver abstraction of docs/tools/database.md §4: one [`DbSession`] per
//! tab (its own server session, so one long query never blocks another tab
//! and can be cancelled), rows streamed in batches into a [`ResultSink`].
//!
//! Values travel as display-ready [`Value`]s: the grid shows text, so drivers
//! decode to the server's own text form (Postgres and MySQL text protocol)
//! and keep numbers, NULL and bytes distinct for alignment and export.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod tls;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Sqlite,
    Postgres,
    /// MySQL and MariaDB.
    Mysql,
}

impl Engine {
    pub const ALL: [Engine; 3] = [Engine::Postgres, Engine::Mysql, Engine::Sqlite];

    /// Profile id: postgres | mysql | mariadb | sqlite.
    pub fn parse(s: &str) -> Option<Engine> {
        match s.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" | "pg" => Some(Engine::Postgres),
            "mysql" | "mariadb" => Some(Engine::Mysql),
            "sqlite" | "sqlite3" => Some(Engine::Sqlite),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Engine::Sqlite => "sqlite",
            Engine::Postgres => "postgres",
            Engine::Mysql => "mysql",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Engine::Sqlite => "SQLite",
            Engine::Postgres => "PostgreSQL",
            Engine::Mysql => "MySQL / MariaDB",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Engine::Sqlite => 0,
            Engine::Postgres => 5432,
            Engine::Mysql => 3306,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Float(f64),
    /// Decimals, dates, JSON, UUIDs, … in the server's text form.
    Text(String),
    /// A number the server sent as text (NUMERIC, DECIMAL, BIGINT UNSIGNED):
    /// right-aligned and exported unquoted, never rounded.
    Number(String),
    Bytes(Vec<u8>),
}

impl Value {
    /// Text shown in a grid cell (NULL is drawn separately by the grid).
    pub fn display(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Int(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            Value::Text(s) | Value::Number(s) => s.clone(),
            Value::Bytes(b) => {
                let shown: String = b.iter().take(64).map(|x| format!("{x:02x}")).collect();
                if b.len() > 64 {
                    format!("\\x{shown}… ({} bytes)", b.len())
                } else {
                    format!("\\x{shown}")
                }
            }
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::Float(_) | Value::Number(_))
    }
}

pub type Row = Vec<Value>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    /// Server type name where the driver knows it ("int4", "VARCHAR", "").
    pub type_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Where a running statement sends its result.
pub trait ResultSink: Send {
    fn columns(&mut self, cols: Vec<Column>);
    /// A batch of rows; return [`Flow::Stop`] once enough have arrived.
    fn rows(&mut self, batch: Vec<Row>) -> Flow;
}

/// Rows per batch handed to the sink.
pub const BATCH: usize = 100;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExecSummary {
    /// INSERT/UPDATE/DELETE count, when the server reports one.
    pub rows_affected: Option<u64>,
    pub rows_returned: u64,
    /// The row limit cut the result short.
    pub limited: bool,
    pub elapsed: Duration,
    /// Server notices/warnings or the command tag ("CREATE TABLE").
    pub message: Option<String>,
    /// Whether the statement produced a result set (even an empty one).
    pub has_result: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbError {
    pub message: String,
    /// Byte offset into the statement, when the server reports a position.
    pub position: Option<usize>,
    /// SQLSTATE or engine error code.
    pub code: Option<String>,
}

impl DbError {
    pub fn msg(m: impl Into<String>) -> Self {
        Self {
            message: m.into(),
            position: None,
            code: None,
        }
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.code {
            Some(c) => write!(f, "{} ({c})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

pub type Result<T> = std::result::Result<T, DbError>;

/// Everything needed to open a session, with secrets already resolved and
/// any SSH tunnel already in place (host/port then point at 127.0.0.1).
#[derive(Clone)]
pub struct ConnectConfig {
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    /// disable | prefer | require | verify-ca | verify-full
    pub ssl: String,
    pub ssl_ca: Option<String>,
    /// SQLite: the database file.
    pub path: String,
    pub read_only: bool,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for ConnectConfig {
    /// Never prints the password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectConfig")
            .field("engine", &self.engine)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("ssl", &self.ssl)
            .field("path", &self.path)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            engine: Engine::Postgres,
            host: "127.0.0.1".into(),
            port: 5432,
            database: String::new(),
            user: String::new(),
            password: None,
            ssl: "prefer".into(),
            ssl_ca: None,
            path: String::new(),
            read_only: false,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "PostgreSQL 16.4", "10.11.6-MariaDB", "SQLite 3.46.0".
    pub version: String,
    /// Database (Postgres/SQLite) or default schema (MySQL) in use.
    pub database: String,
}

/// Object kinds the schema browser shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ObjectKind {
    Schema,
    Table,
    View,
    Function,
    Sequence,
    Column,
    Index,
}

impl ObjectKind {
    /// The groups listed under a schema, in order.
    pub fn groups(engine: Engine) -> &'static [ObjectKind] {
        match engine {
            Engine::Postgres => &[
                ObjectKind::Table,
                ObjectKind::View,
                ObjectKind::Function,
                ObjectKind::Sequence,
            ],
            Engine::Mysql => &[ObjectKind::Table, ObjectKind::View, ObjectKind::Function],
            Engine::Sqlite => &[ObjectKind::Table, ObjectKind::View],
        }
    }
}

/// One node returned by [`DbSession::introspect`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SchemaObject {
    pub kind: ObjectKind,
    pub name: String,
    /// Column type, function signature, index definition, ….
    pub detail: String,
    /// Badges: "PK", "NOT NULL", "UNIQUE", "FK → orders.id".
    pub badges: Vec<String>,
}

/// What to list. Each level is fetched lazily when its tree node opens.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    Schemas,
    Objects { schema: String, kind: ObjectKind },
    Columns { schema: String, table: String },
    Indexes { schema: String, table: String },
}

/// Tables and their columns for completion (one round trip per engine).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Catalog {
    /// (schema, table, columns)
    pub tables: Vec<(String, String, Vec<String>)>,
}

/// Cancels whatever the session is running right now; callable from the UI
/// thread while the session itself is busy.
pub type Canceller = Arc<dyn Fn() + Send + Sync>;

#[async_trait]
pub trait DbSession: Send {
    fn engine(&self) -> Engine;
    fn server_info(&self) -> &ServerInfo;

    /// Run one statement, streaming any result into `sink`; stops reading
    /// (and asks the server to stop sending) once the sink says so.
    async fn execute(&mut self, sql: &str, sink: &mut dyn ResultSink) -> Result<ExecSummary>;

    /// A handle that cancels the running statement (pg_cancel_backend,
    /// `KILL QUERY`, sqlite3_interrupt).
    fn canceller(&self) -> Canceller;

    async fn introspect(&mut self, scope: &Scope) -> Result<Vec<SchemaObject>>;

    async fn catalog(&mut self) -> Result<Catalog>;

    /// Whether the server is inside an open transaction block.
    fn in_transaction(&self) -> bool;
}

/// Open a session.
pub async fn connect(cfg: &ConnectConfig) -> Result<Box<dyn DbSession>> {
    match cfg.engine {
        Engine::Sqlite => Ok(Box::new(sqlite::SqliteSession::open(cfg).await?)),
        Engine::Postgres => Ok(Box::new(postgres::PgSession::connect(cfg).await?)),
        Engine::Mysql => Ok(Box::new(mysql::MySession::connect(cfg).await?)),
    }
}

/// Collects a whole result into memory (tests, introspection helpers).
#[derive(Debug, Default)]
pub struct Collect {
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub limit: Option<usize>,
}

impl ResultSink for Collect {
    fn columns(&mut self, cols: Vec<Column>) {
        self.columns = cols;
    }
    fn rows(&mut self, batch: Vec<Row>) -> Flow {
        self.rows.extend(batch);
        match self.limit {
            Some(l) if self.rows.len() >= l => {
                self.rows.truncate(l);
                Flow::Stop
            }
            _ => Flow::Continue,
        }
    }
}

/// Track BEGIN/COMMIT/ROLLBACK for engines whose protocol does not report the
/// transaction state (SQLite asks the connection; Postgres and MySQL use this
/// from the classifier's view of the statement).
pub(crate) fn tx_effect(sql: &str, engine: Engine) -> Option<bool> {
    let toks = crate::sql::tokenize(sql, engine);
    let words: Vec<String> = toks
        .iter()
        .filter_map(|t| match &t.tok {
            crate::sql::Tok::Word { upper, .. } => Some(upper.clone()),
            _ => None,
        })
        .take(3)
        .collect();
    let w = |i: usize| words.get(i).map(String::as_str).unwrap_or("");
    match w(0) {
        "BEGIN" if w(1) != "ATOMIC" => Some(true),
        "START" if w(1) == "TRANSACTION" => Some(true),
        "COMMIT" | "END" => Some(false),
        "ROLLBACK" if w(1) != "TO" && !(w(1) == "WORK" && w(2) == "TO") => Some(false),
        "ABORT" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engines_parse_profile_ids() {
        assert_eq!(Engine::parse("MariaDB"), Some(Engine::Mysql));
        assert_eq!(Engine::parse("postgresql"), Some(Engine::Postgres));
        assert_eq!(Engine::parse("mssql"), None);
    }

    #[test]
    fn transaction_effects() {
        let e = Engine::Postgres;
        assert_eq!(tx_effect("begin", e), Some(true));
        assert_eq!(tx_effect("START TRANSACTION READ ONLY", e), Some(true));
        assert_eq!(tx_effect("commit", e), Some(false));
        assert_eq!(tx_effect("rollback to savepoint a", e), None);
        assert_eq!(tx_effect("select 1", e), None);
    }

    #[test]
    fn bytes_display_as_hex() {
        assert_eq!(Value::Bytes(vec![0, 255]).display(), "\\x00ff");
        assert!(
            Value::Bytes(vec![1; 100])
                .display()
                .ends_with("(100 bytes)")
        );
    }
}
