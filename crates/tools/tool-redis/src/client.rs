//! The server session (docs/tools/redis.md §4, §5, §10): one multiplexed
//! connection per tab, `SCAN` rather than `KEYS`, and reads that page every
//! collection type so a million-element hash never arrives at once.
//!
//! Values travel as bytes: Redis strings are binary, and the viewer decides
//! how to show them ([`crate::value`]).

use std::time::Duration;

use redis::{AsyncCommands, RedisError, aio::MultiplexedConnection};

/// Everything needed to open a session, with secrets already resolved and any
/// SSH tunnel already in place (host/port then point at 127.0.0.1).
#[derive(Clone)]
pub struct ConnectConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub db: i64,
    pub tls: bool,
    /// PEM file with the CA that signed the server certificate.
    pub tls_ca: Option<String>,
    pub read_only: bool,
    pub connect_timeout: Duration,
}

impl std::fmt::Debug for ConnectConfig {
    /// Never prints the password.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("db", &self.db)
            .field("tls", &self.tls)
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl Default for ConnectConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6379,
            user: String::new(),
            password: None,
            db: 0,
            tls: false,
            tls_ca: None,
            read_only: false,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    /// The server's error class ("WRONGTYPE", "NOAUTH", "NOPERM").
    pub code: Option<String>,
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self {
            message: m.into(),
            code: None,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.code {
            Some(c) => write!(f, "{c}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl From<RedisError> for Error {
    fn from(e: RedisError) -> Self {
        Self {
            code: e.code().map(str::to_string),
            message: e
                .detail()
                .map(str::to_string)
                .unwrap_or_else(|| e.to_string()),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The value types the tool reads. `Other` keeps a module's type name
/// (ReJSON-RL, TSDB-TYPE) so the UI can at least say what it is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyType {
    String,
    List,
    Set,
    ZSet,
    Hash,
    Stream,
    Other(String),
    /// The key went away between the scan and the read.
    None,
}

impl KeyType {
    pub fn parse(s: &str) -> KeyType {
        match s {
            "string" => KeyType::String,
            "list" => KeyType::List,
            "set" => KeyType::Set,
            "zset" => KeyType::ZSet,
            "hash" => KeyType::Hash,
            "stream" => KeyType::Stream,
            "none" => KeyType::None,
            other => KeyType::Other(other.to_string()),
        }
    }

    /// Short badge in the key tree.
    pub fn badge(&self) -> &str {
        match self {
            KeyType::String => "string",
            KeyType::List => "list",
            KeyType::Set => "set",
            KeyType::ZSet => "zset",
            KeyType::Hash => "hash",
            KeyType::Stream => "stream",
            KeyType::Other(s) => s,
            KeyType::None => "gone",
        }
    }

    /// Whether the viewer pages through members rather than showing one value.
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            KeyType::List | KeyType::Set | KeyType::ZSet | KeyType::Hash | KeyType::Stream
        )
    }
}

/// What the header above a value shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyInfo {
    pub key: String,
    pub kind: Option<KeyType>,
    /// Milliseconds left, or `None` for a key that never expires.
    pub ttl_ms: Option<i64>,
    pub memory: Option<u64>,
    pub encoding: String,
    pub idle_secs: Option<u64>,
    /// Length, cardinality or field count, by type.
    pub len: Option<u64>,
}

/// One stream entry: its id and the field/value pairs it carries.
pub type StreamEntry = (String, Vec<(String, Vec<u8>)>);

/// One page of a value. Collections carry the cursor to continue from, `0`
/// meaning the iteration is complete.
#[derive(Debug, Clone, PartialEq)]
pub enum ValuePage {
    /// A string value, possibly cut at [`STRING_PREVIEW`].
    Text { bytes: Vec<u8>, total: u64 },
    /// Hash fields, stream entry fields.
    Pairs {
        items: Vec<(String, Vec<u8>)>,
        cursor: u64,
    },
    /// List or set members. Lists carry the index of the first item.
    Items {
        items: Vec<Vec<u8>>,
        cursor: u64,
        start: u64,
    },
    /// Sorted-set members with their scores.
    Scored {
        items: Vec<(Vec<u8>, f64)>,
        cursor: u64,
    },
    /// Stream entries: id and its field/value pairs.
    Entries { items: Vec<StreamEntry> },
    /// A type the tool has no viewer for.
    Unsupported(String),
}

/// How much of a large string is read before the user asks for the rest.
pub const STRING_PREVIEW: usize = 256 * 1024;
/// Members fetched per page.
pub const PAGE: usize = 200;
/// `COUNT` hint for a key scan.
pub const SCAN_COUNT: usize = 500;

/// One step of the key scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanPage {
    pub keys: Vec<String>,
    pub cursor: u64,
}

/// A live session. Every method borrows mutably: one command at a time per
/// tab, which is what the multiplexed connection wants.
pub struct Session {
    /// Kept to open the Pub/Sub connection beside the multiplexed one.
    client: redis::Client,
    conn: MultiplexedConnection,
    info: ServerInfo,
    db: i64,
    read_only: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerInfo {
    /// "7.2.4", or "" when INFO is not permitted.
    pub version: String,
    /// "redis", "valkey", "keydb".
    pub flavour: String,
    /// Databases the server has (`CONFIG GET databases`), 16 by default.
    pub databases: i64,
}

impl Session {
    /// Open the connection, authenticate and select the database.
    pub async fn connect(cfg: &ConnectConfig) -> Result<Session> {
        let client = open_client(cfg)?;
        let conn = tokio::time::timeout(
            cfg.connect_timeout,
            client.get_multiplexed_async_connection(),
        )
        .await
        .map_err(|_| Error::msg("the server did not answer in time"))??;
        let mut s = Session {
            client,
            conn,
            info: ServerInfo::default(),
            db: cfg.db,
            read_only: cfg.read_only,
        };
        s.info = s.read_server_info().await;
        Ok(s)
    }

    pub fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    pub fn db(&self) -> i64 {
        self.db
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// `SELECT`, so one session can walk the keyspaces.
    pub async fn select(&mut self, db: i64) -> Result<()> {
        redis::cmd("SELECT")
            .arg(db)
            .exec_async(&mut self.conn)
            .await?;
        self.db = db;
        Ok(())
    }

    async fn read_server_info(&mut self) -> ServerInfo {
        let text: String = redis::cmd("INFO")
            .arg("server")
            .query_async(&mut self.conn)
            .await
            .unwrap_or_default();
        let field = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(':'))
                .map(|v| v.trim().to_string())
                .unwrap_or_default()
        };
        // Valkey reports **both**: `valkey_version` is what it is, and
        // `redis_version` is the Redis it claims compatibility with
        // (8.1.10 reports 7.2.4). Showing the compatibility number
        // beside the name "Valkey" states a version the server does not
        // have, so the flavour decides which field is the version.
        let mut flavour = String::from("redis");
        let valkey = field("valkey_version");
        if !valkey.is_empty() {
            flavour = "valkey".into();
        }
        let databases: i64 = redis::cmd("CONFIG")
            .arg("GET")
            .arg("databases")
            .query_async::<Vec<String>>(&mut self.conn)
            .await
            .ok()
            .and_then(|v| v.get(1)?.parse().ok())
            .unwrap_or(16);
        ServerInfo {
            version: if valkey.is_empty() {
                field("redis_version")
            } else {
                valkey
            },
            flavour,
            databases,
        }
    }

    /// `INFO <section>` as the server returns it.
    pub async fn info(&mut self, section: &str) -> Result<String> {
        Ok(redis::cmd("INFO")
            .arg(section)
            .query_async(&mut self.conn)
            .await?)
    }

    /// Keys per database, from `INFO keyspace`.
    pub async fn keyspace(&mut self) -> Vec<(i64, u64)> {
        let text = self.info("keyspace").await.unwrap_or_default();
        text.lines()
            .filter_map(|l| {
                let (db, rest) = l.split_once(':')?;
                let n = db.strip_prefix("db")?.parse().ok()?;
                let keys = rest
                    .split(',')
                    .find_map(|p| p.trim().strip_prefix("keys=")?.parse().ok())?;
                Some((n, keys))
            })
            .collect()
    }

    /// One `SCAN` step. `pattern` is a glob; empty means every key.
    pub async fn scan(&mut self, cursor: u64, pattern: &str, count: usize) -> Result<ScanPage> {
        let mut cmd = redis::cmd("SCAN");
        cmd.arg(cursor);
        if !pattern.is_empty() {
            cmd.arg("MATCH").arg(pattern);
        }
        cmd.arg("COUNT").arg(count);
        let (next, keys): (u64, Vec<String>) = cmd.query_async(&mut self.conn).await?;
        Ok(ScanPage { keys, cursor: next })
    }

    /// The types of a batch of keys, in the order asked. One pipeline rather
    /// than a round trip per key.
    pub async fn types(&mut self, keys: &[String]) -> Result<Vec<KeyType>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut pipe = redis::pipe();
        for k in keys {
            pipe.cmd("TYPE").arg(k);
        }
        let names: Vec<String> = pipe.query_async(&mut self.conn).await?;
        Ok(names.iter().map(|n| KeyType::parse(n)).collect())
    }

    /// Everything the value header shows. `MEMORY USAGE` is sampled, and is
    /// missing on servers that do not allow it.
    pub async fn key_info(&mut self, key: &str) -> Result<KeyInfo> {
        let kind = KeyType::parse(
            &redis::cmd("TYPE")
                .arg(key)
                .query_async::<String>(&mut self.conn)
                .await?,
        );
        let ttl: i64 = redis::cmd("PTTL")
            .arg(key)
            .query_async(&mut self.conn)
            .await?;
        let memory: Option<u64> = redis::cmd("MEMORY")
            .arg("USAGE")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .ok()
            .flatten();
        let encoding: String = redis::cmd("OBJECT")
            .arg("ENCODING")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .unwrap_or_default();
        let idle: Option<u64> = redis::cmd("OBJECT")
            .arg("IDLETIME")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .ok();
        let len = self.length(key, &kind).await;
        Ok(KeyInfo {
            key: key.to_string(),
            ttl_ms: (ttl >= 0).then_some(ttl),
            memory,
            encoding,
            idle_secs: idle,
            len,
            kind: Some(kind),
        })
    }

    async fn length(&mut self, key: &str, kind: &KeyType) -> Option<u64> {
        let cmd = match kind {
            KeyType::String => "STRLEN",
            KeyType::List => "LLEN",
            KeyType::Set => "SCARD",
            KeyType::ZSet => "ZCARD",
            KeyType::Hash => "HLEN",
            KeyType::Stream => "XLEN",
            _ => return None,
        };
        redis::cmd(cmd)
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .ok()
    }

    /// One page of a value. `cursor` continues a hash/set/zset scan;
    /// `start` is the list index or the stream offset to read from.
    pub async fn read(
        &mut self,
        key: &str,
        kind: &KeyType,
        cursor: u64,
        start: u64,
        whole: bool,
    ) -> Result<ValuePage> {
        match kind {
            KeyType::String => {
                let total: u64 = redis::cmd("STRLEN")
                    .arg(key)
                    .query_async(&mut self.conn)
                    .await
                    .unwrap_or(0);
                let bytes: Vec<u8> = if whole || total as usize <= STRING_PREVIEW {
                    redis::cmd("GET")
                        .arg(key)
                        .query_async(&mut self.conn)
                        .await?
                } else {
                    redis::cmd("GETRANGE")
                        .arg(key)
                        .arg(0)
                        .arg(STRING_PREVIEW as i64 - 1)
                        .query_async(&mut self.conn)
                        .await?
                };
                Ok(ValuePage::Text { bytes, total })
            }
            KeyType::Hash => {
                let (next, flat): (u64, Vec<Vec<u8>>) = redis::cmd("HSCAN")
                    .arg(key)
                    .arg(cursor)
                    .arg("COUNT")
                    .arg(PAGE)
                    .query_async(&mut self.conn)
                    .await?;
                let items = flat
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| (String::from_utf8_lossy(&c[0]).into_owned(), c[1].clone()))
                    .collect();
                Ok(ValuePage::Pairs {
                    items,
                    cursor: next,
                })
            }
            KeyType::Set => {
                let (next, items): (u64, Vec<Vec<u8>>) = redis::cmd("SSCAN")
                    .arg(key)
                    .arg(cursor)
                    .arg("COUNT")
                    .arg(PAGE)
                    .query_async(&mut self.conn)
                    .await?;
                Ok(ValuePage::Items {
                    items,
                    cursor: next,
                    start: 0,
                })
            }
            KeyType::List => {
                let end = start + PAGE as u64 - 1;
                let items: Vec<Vec<u8>> = redis::cmd("LRANGE")
                    .arg(key)
                    .arg(start)
                    .arg(end)
                    .query_async(&mut self.conn)
                    .await?;
                // A short page means the end of the list.
                let next = if items.len() < PAGE { 0 } else { end + 1 };
                Ok(ValuePage::Items {
                    items,
                    cursor: next,
                    start,
                })
            }
            KeyType::ZSet => {
                let end = start + PAGE as u64 - 1;
                let flat: Vec<Vec<u8>> = redis::cmd("ZRANGE")
                    .arg(key)
                    .arg(start)
                    .arg(end)
                    .arg("WITHSCORES")
                    .query_async(&mut self.conn)
                    .await?;
                let items: Vec<(Vec<u8>, f64)> = flat
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| {
                        let score = String::from_utf8_lossy(&c[1]).parse().unwrap_or(f64::NAN);
                        (c[0].clone(), score)
                    })
                    .collect();
                let next = if items.len() < PAGE { 0 } else { end + 1 };
                Ok(ValuePage::Scored {
                    items,
                    cursor: next,
                })
            }
            KeyType::Stream => {
                let raw: redis::Value = redis::cmd("XRANGE")
                    .arg(key)
                    .arg("-")
                    .arg("+")
                    .arg("COUNT")
                    .arg(PAGE)
                    .query_async(&mut self.conn)
                    .await?;
                Ok(ValuePage::Entries {
                    items: stream_entries(&raw),
                })
            }
            KeyType::Other(t) => Ok(ValuePage::Unsupported(t.clone())),
            KeyType::None => Ok(ValuePage::Unsupported("none".into())),
        }
    }

    // ---- depth programme, phase A --------------------------------------

    /// `SLOWLOG GET n`, newest first as the server returns them.
    pub async fn slowlog(&mut self, n: usize) -> Result<Vec<SlowEntry>> {
        let v: redis::Value = redis::cmd("SLOWLOG")
            .arg("GET")
            .arg(n)
            .query_async(&mut self.conn)
            .await?;
        Ok(parse_slowlog(&v))
    }

    pub async fn client_list(&mut self) -> Result<Vec<ClientRow>> {
        let text: String = redis::cmd("CLIENT")
            .arg("LIST")
            .query_async(&mut self.conn)
            .await?;
        Ok(parse_client_list(&text))
    }

    pub async fn client_kill(&mut self, id: &str) -> Result<()> {
        redis::cmd("CLIENT")
            .arg("KILL")
            .arg("ID")
            .arg(id)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    /// `MEMORY USAGE` for a page of keys in one pipeline; `None` where the
    /// key went away between the scan and the read.
    pub async fn memory_usages(&mut self, keys: &[String]) -> Result<Vec<Option<u64>>> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let mut pipe = redis::pipe();
        for k in keys {
            pipe.cmd("MEMORY").arg("USAGE").arg(k).arg("SAMPLES").arg(0);
        }
        Ok(pipe.query_async(&mut self.conn).await?)
    }

    /// `EXPIRE` every key in one pipeline; how many were set.
    pub async fn expire_many(&mut self, keys: &[String], secs: i64) -> Result<u64> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut pipe = redis::pipe();
        for k in keys {
            pipe.cmd("EXPIRE").arg(k).arg(secs);
        }
        let v: Vec<i64> = pipe.query_async(&mut self.conn).await?;
        Ok(v.into_iter().filter(|n| *n == 1).count() as u64)
    }

    /// `PERSIST` every key in one pipeline; how many had a TTL to drop.
    pub async fn persist_many(&mut self, keys: &[String]) -> Result<u64> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut pipe = redis::pipe();
        for k in keys {
            pipe.cmd("PERSIST").arg(k);
        }
        let v: Vec<i64> = pipe.query_async(&mut self.conn).await?;
        Ok(v.into_iter().filter(|n| *n == 1).count() as u64)
    }

    /// `RENAME` each pair in turn; a failure says how many went before it.
    pub async fn rename_many(&mut self, pairs: &[(String, String)]) -> Result<u64> {
        let mut n = 0u64;
        for (from, to) in pairs {
            if let Err(e) = self.rename(from, to).await {
                return Err(Error::msg(format!("renamed {n}, then {from}: {e}")));
            }
            n += 1;
        }
        Ok(n)
    }

    /// `XINFO STREAM`, `XINFO GROUPS` and the newest `count` entries.
    pub async fn stream_view(&mut self, key: &str, count: usize) -> Result<StreamView> {
        let info: redis::Value = redis::cmd("XINFO")
            .arg("STREAM")
            .arg(key)
            .query_async(&mut self.conn)
            .await?;
        let info = kv_pairs(&info);
        // A stream without groups answers an empty array; an older server
        // without XINFO GROUPS answers an error, which is no reason to hide
        // the rest.
        let groups: redis::Value = redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(key)
            .query_async(&mut self.conn)
            .await
            .unwrap_or(redis::Value::Array(Vec::new()));
        let raw: redis::Value = redis::cmd("XREVRANGE")
            .arg(key)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut self.conn)
            .await?;
        let mut entries: Vec<(String, Vec<(String, String)>)> = stream_entries(&raw)
            .into_iter()
            .map(|(id, fields)| {
                let fields = fields
                    .into_iter()
                    .map(|(k, v)| (k, String::from_utf8_lossy(&v).into_owned()))
                    .collect();
                (id, fields)
            })
            .collect();
        entries.reverse();
        let groups = match groups {
            redis::Value::Array(gs) => gs
                .iter()
                .map(|g| {
                    let kv = kv_pairs(g);
                    GroupRow {
                        name: field_string(&kv, "name"),
                        consumers: field_i64(&kv, "consumers"),
                        pending: field_i64(&kv, "pending"),
                        last_delivered: field_string(&kv, "last-delivered-id"),
                    }
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(StreamView {
            key: key.to_string(),
            length: field_i64(&info, "length"),
            last_id: field_string(&info, "last-generated-id"),
            groups,
            entries,
        })
    }

    /// `PUBLISH`; how many subscribers received it.
    pub async fn publish(&mut self, channel: &str, message: &str) -> Result<i64> {
        Ok(redis::cmd("PUBLISH")
            .arg(channel)
            .arg(message)
            .query_async(&mut self.conn)
            .await?)
    }

    /// A second connection for Pub/Sub: a subscribed connection can do
    /// nothing else, so the session keeps its own.
    pub async fn pubsub(&self) -> Result<redis::aio::PubSub> {
        Ok(self.client.get_async_pubsub().await?)
    }

    /// Run one already-parsed command, for the console.
    pub async fn command(&mut self, args: &[String]) -> Result<redis::Value> {
        let mut parts = args.iter();
        let name = parts.next().ok_or_else(|| Error::msg("no command"))?;
        let mut cmd = redis::cmd(name);
        for a in parts {
            cmd.arg(a.as_str());
        }
        let v = cmd.query_async(&mut self.conn).await?;
        // SELECT through the console moves the whole session.
        if name.eq_ignore_ascii_case("select")
            && let Some(n) = args.get(1).and_then(|d| d.parse().ok())
        {
            self.db = n;
        }
        Ok(v)
    }

    // ---- writes ---------------------------------------------------------

    pub async fn set_string(&mut self, key: &str, value: &[u8], keep_ttl: bool) -> Result<()> {
        let mut cmd = redis::cmd("SET");
        cmd.arg(key).arg(value);
        if keep_ttl {
            cmd.arg("KEEPTTL");
        }
        cmd.exec_async(&mut self.conn).await?;
        Ok(())
    }

    pub async fn hset(&mut self, key: &str, field: &str, value: &[u8]) -> Result<()> {
        redis::cmd("HSET")
            .arg(key)
            .arg(field)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn hdel(&mut self, key: &str, field: &str) -> Result<()> {
        redis::cmd("HDEL")
            .arg(key)
            .arg(field)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn lset(&mut self, key: &str, index: i64, value: &[u8]) -> Result<()> {
        redis::cmd("LSET")
            .arg(key)
            .arg(index)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn rpush(&mut self, key: &str, value: &[u8]) -> Result<()> {
        redis::cmd("RPUSH")
            .arg(key)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn lpush(&mut self, key: &str, value: &[u8]) -> Result<()> {
        redis::cmd("LPUSH")
            .arg(key)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    /// Remove every occurrence of a list element.
    pub async fn lrem(&mut self, key: &str, value: &[u8]) -> Result<u64> {
        Ok(redis::cmd("LREM")
            .arg(key)
            .arg(0)
            .arg(value)
            .query_async(&mut self.conn)
            .await?)
    }

    pub async fn sadd(&mut self, key: &str, value: &[u8]) -> Result<()> {
        redis::cmd("SADD")
            .arg(key)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn srem(&mut self, key: &str, value: &[u8]) -> Result<()> {
        redis::cmd("SREM")
            .arg(key)
            .arg(value)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn zadd(&mut self, key: &str, member: &[u8], score: f64) -> Result<()> {
        redis::cmd("ZADD")
            .arg(key)
            .arg(score)
            .arg(member)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn zrem(&mut self, key: &str, member: &[u8]) -> Result<()> {
        redis::cmd("ZREM")
            .arg(key)
            .arg(member)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn xdel(&mut self, key: &str, id: &str) -> Result<()> {
        redis::cmd("XDEL")
            .arg(key)
            .arg(id)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    pub async fn rename(&mut self, key: &str, to: &str) -> Result<()> {
        redis::cmd("RENAME")
            .arg(key)
            .arg(to)
            .exec_async(&mut self.conn)
            .await?;
        Ok(())
    }

    /// `PEXPIRE`, or `PERSIST` when `ms` is `None`.
    pub async fn set_ttl(&mut self, key: &str, ms: Option<i64>) -> Result<()> {
        match ms {
            Some(ms) => {
                redis::cmd("PEXPIRE")
                    .arg(key)
                    .arg(ms)
                    .exec_async(&mut self.conn)
                    .await?;
            }
            None => {
                redis::cmd("PERSIST")
                    .arg(key)
                    .exec_async(&mut self.conn)
                    .await?;
            }
        }
        Ok(())
    }

    /// Delete keys, returning how many existed.
    pub async fn delete(&mut self, keys: &[String]) -> Result<u64> {
        if keys.is_empty() {
            return Ok(0);
        }
        let mut cmd = redis::cmd("DEL");
        for k in keys {
            cmd.arg(k);
        }
        Ok(cmd.query_async(&mut self.conn).await?)
    }

    /// Count the keys a prefix would delete, scanning rather than `KEYS`.
    /// Returns the matches found and whether the scan was cut short.
    pub async fn count_prefix(&mut self, pattern: &str, cap: usize) -> Result<(Vec<String>, bool)> {
        let mut cursor = 0u64;
        let mut found = Vec::new();
        loop {
            let page = self.scan(cursor, pattern, SCAN_COUNT).await?;
            cursor = page.cursor;
            found.extend(page.keys);
            if found.len() >= cap {
                found.truncate(cap);
                return Ok((found, true));
            }
            if cursor == 0 {
                return Ok((found, false));
            }
        }
    }

    pub async fn ping(&mut self) -> Result<()> {
        redis::cmd("PING").exec_async(&mut self.conn).await?;
        Ok(())
    }

    /// `TYPE` for one key, so a console result can offer "open as key".
    pub async fn exists(&mut self, key: &str) -> bool {
        self.conn.exists(key).await.unwrap_or(false)
    }
}

/// Build the driver's connection info, including TLS and ACL. The crate's
/// fields are private, so this goes through its setters rather than a
/// struct literal.
fn connection_info(cfg: &ConnectConfig) -> Result<redis::ConnectionInfo> {
    use redis::IntoConnectionInfo;
    let addr = if cfg.tls {
        redis::ConnectionAddr::TcpTls {
            host: cfg.host.clone(),
            port: cfg.port,
            insecure: false,
            tls_params: None,
        }
    } else {
        redis::ConnectionAddr::Tcp(cfg.host.clone(), cfg.port)
    };
    let mut redis_info = redis::RedisConnectionInfo::default().set_db(cfg.db);
    if !cfg.user.is_empty() {
        redis_info = redis_info.set_username(&cfg.user);
    }
    if let Some(p) = &cfg.password {
        redis_info = redis_info.set_password(p);
    }
    Ok(addr.into_connection_info()?.set_redis_settings(redis_info))
}

// ---- depth programme, phase A (docs/tools/redis.md §7, §8) -------------

/// One `SLOWLOG GET` entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlowEntry {
    pub id: i64,
    /// Unix seconds.
    pub at: i64,
    pub micros: i64,
    pub command: String,
    pub client: String,
}

/// One `CLIENT LIST` row: the fields the table shows, as the server
/// printed them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientRow {
    pub id: String,
    pub addr: String,
    pub name: String,
    pub age: String,
    pub idle: String,
    pub db: String,
    pub cmd: String,
}

/// A consumer group of a stream (`XINFO GROUPS`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupRow {
    pub name: String,
    pub consumers: i64,
    pub pending: i64,
    pub last_delivered: String,
}

/// A stream's shape and its newest entries, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamView {
    pub key: String,
    pub length: i64,
    pub last_id: String,
    pub groups: Vec<GroupRow>,
    pub entries: Vec<(String, Vec<(String, String)>)>,
}

/// Alternating key/value arrays (RESP2) or maps (RESP3) as pairs.
pub fn kv_pairs(v: &redis::Value) -> Vec<(String, redis::Value)> {
    match v {
        redis::Value::Map(m) => m.iter().map(|(k, v)| (as_string(k), v.clone())).collect(),
        redis::Value::Array(a) => a
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| (as_string(&c[0]), c[1].clone()))
            .collect(),
        _ => Vec::new(),
    }
}

fn field_string(kv: &[(String, redis::Value)], name: &str) -> String {
    kv.iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| as_string(v))
        .unwrap_or_default()
}

fn field_i64(kv: &[(String, redis::Value)], name: &str) -> i64 {
    kv.iter()
        .find(|(k, _)| k == name)
        .and_then(|(_, v)| match v {
            redis::Value::Int(i) => Some(*i),
            other => as_string(other).parse().ok(),
        })
        .unwrap_or(0)
}

/// `SLOWLOG GET` rows: id, unix time, microseconds, argv, client address.
pub fn parse_slowlog(v: &redis::Value) -> Vec<SlowEntry> {
    let redis::Value::Array(rows) = v else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|r| {
            let redis::Value::Array(f) = r else {
                return None;
            };
            let int = |i: usize| match f.get(i) {
                Some(redis::Value::Int(n)) => *n,
                _ => 0,
            };
            let command = match f.get(3) {
                Some(redis::Value::Array(args)) => {
                    args.iter().map(as_string).collect::<Vec<_>>().join(" ")
                }
                _ => String::new(),
            };
            Some(SlowEntry {
                id: int(0),
                at: int(1),
                micros: int(2),
                command,
                client: f.get(4).map(as_string).unwrap_or_default(),
            })
        })
        .collect()
}

/// `CLIENT LIST` text: one client per line, `key=value` fields.
pub fn parse_client_list(text: &str) -> Vec<ClientRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut row = ClientRow::default();
            for part in l.split(' ') {
                let Some((k, v)) = part.split_once('=') else {
                    continue;
                };
                let v = v.to_string();
                match k {
                    "id" => row.id = v,
                    "addr" => row.addr = v,
                    "name" => row.name = v,
                    "age" => row.age = v,
                    "idle" => row.idle = v,
                    "db" => row.db = v,
                    "cmd" => row.cmd = v,
                    _ => {}
                }
            }
            row
        })
        .collect()
}

/// Open the client, handing rustls a private CA when the profile names one.
fn open_client(cfg: &ConnectConfig) -> Result<redis::Client> {
    let info = connection_info(cfg)?;
    match (&cfg.tls_ca, cfg.tls) {
        (Some(path), true) => {
            let root_cert = std::fs::read(path)
                .map_err(|e| Error::msg(format!("cannot read the CA file {path}: {e}")))?;
            Ok(redis::Client::build_with_tls(
                info,
                redis::TlsCertificates {
                    client_tls: None,
                    root_cert: Some(root_cert),
                },
            )?)
        }
        _ => Ok(redis::Client::open(info)?),
    }
}

/// `XRANGE` returns `[[id, [field, value, …]], …]`.
fn stream_entries(v: &redis::Value) -> Vec<StreamEntry> {
    let redis::Value::Array(entries) = v else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|e| {
            let redis::Value::Array(pair) = e else {
                return None;
            };
            let id = as_string(pair.first()?);
            let redis::Value::Array(flat) = pair.get(1)? else {
                return Some((id, Vec::new()));
            };
            let fields = flat
                .chunks(2)
                .filter(|c| c.len() == 2)
                .map(|c| (as_string(&c[0]), as_bytes(&c[1])))
                .collect();
            Some((id, fields))
        })
        .collect()
}

fn as_string(v: &redis::Value) -> String {
    String::from_utf8_lossy(&as_bytes(v)).into_owned()
}

fn as_bytes(v: &redis::Value) -> Vec<u8> {
    match v {
        redis::Value::BulkString(b) => b.clone(),
        redis::Value::SimpleString(s) => s.as_bytes().to_vec(),
        redis::Value::Int(i) => i.to_string().into_bytes(),
        other => format!("{other:?}").into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_names_map_to_viewers() {
        assert_eq!(KeyType::parse("zset"), KeyType::ZSet);
        assert!(KeyType::parse("hash").is_collection());
        assert!(!KeyType::parse("string").is_collection());
        assert_eq!(
            KeyType::parse("ReJSON-RL"),
            KeyType::Other("ReJSON-RL".into())
        );
        assert_eq!(KeyType::parse("ReJSON-RL").badge(), "ReJSON-RL");
        assert_eq!(KeyType::parse("none"), KeyType::None);
    }

    #[test]
    fn a_connect_config_never_prints_its_password() {
        let cfg = ConnectConfig {
            password: Some("hunter2".into()),
            ..Default::default()
        };
        assert!(!format!("{cfg:?}").contains("hunter2"));
    }

    #[test]
    fn tls_picks_the_tls_address_and_acl_the_username() {
        let info = connection_info(&ConnectConfig {
            tls: true,
            user: "alice".into(),
            db: 3,
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(info.addr(), redis::ConnectionAddr::TcpTls { .. }));
        assert_eq!(info.redis_settings().username(), Some("alice"));
        assert_eq!(info.redis_settings().db(), 3);
        let plain = connection_info(&ConnectConfig::default()).unwrap();
        assert!(matches!(plain.addr(), redis::ConnectionAddr::Tcp(..)));
        // No ACL user means the implicit default user, not an empty one.
        assert_eq!(plain.redis_settings().username(), None);
    }

    #[test]
    fn stream_entries_keep_their_ids_and_fields() {
        let v = redis::Value::Array(vec![redis::Value::Array(vec![
            redis::Value::BulkString(b"1700-0".to_vec()),
            redis::Value::Array(vec![
                redis::Value::BulkString(b"sensor".to_vec()),
                redis::Value::BulkString(b"12".to_vec()),
            ]),
        ])]);
        let out = stream_entries(&v);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].0, "1700-0");
        assert_eq!(out[0].1[0].0, "sensor");
        assert_eq!(out[0].1[0].1, b"12");
        assert!(stream_entries(&redis::Value::Nil).is_empty());
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    fn bulk(s: &str) -> redis::Value {
        redis::Value::BulkString(s.as_bytes().to_vec())
    }

    #[test]
    fn slowlog_rows_parse() {
        let v = redis::Value::Array(vec![redis::Value::Array(vec![
            redis::Value::Int(14),
            redis::Value::Int(1_700_000_000),
            redis::Value::Int(15_000),
            redis::Value::Array(vec![bulk("KEYS"), bulk("*")]),
            bulk("127.0.0.1:50000"),
            bulk(""),
        ])]);
        let rows = parse_slowlog(&v);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, 14);
        assert_eq!(rows[0].micros, 15_000);
        assert_eq!(rows[0].command, "KEYS *");
        assert_eq!(rows[0].client, "127.0.0.1:50000");
    }

    #[test]
    fn client_list_rows_parse() {
        let text = "id=3 addr=127.0.0.1:1 name= age=10 idle=0 db=0 cmd=client|list\nid=4 addr=127.0.0.1:2 name=app age=99 idle=5 db=2 cmd=get\n";
        let rows = parse_client_list(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].id, "4");
        assert_eq!(rows[1].name, "app");
        assert_eq!(rows[1].db, "2");
        assert_eq!(rows[0].cmd, "client|list");
    }

    #[test]
    fn kv_pairs_read_arrays_and_maps() {
        let arr = redis::Value::Array(vec![
            bulk("length"),
            redis::Value::Int(3),
            bulk("last-generated-id"),
            bulk("1-0"),
        ]);
        let kv = kv_pairs(&arr);
        assert_eq!(field_i64(&kv, "length"), 3);
        assert_eq!(field_string(&kv, "last-generated-id"), "1-0");
        let map = redis::Value::Map(vec![(bulk("pending"), bulk("7"))]);
        assert_eq!(field_i64(&kv_pairs(&map), "pending"), 7);
        assert_eq!(field_i64(&kv, "missing"), 0);
    }
}
