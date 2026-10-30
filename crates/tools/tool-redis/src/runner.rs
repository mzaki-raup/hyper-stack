//! The tab's connection, owned by one task on the tokio runtime. The UI
//! sends [`Cmd`]s through a bounded channel and reads [`Shared`] at frame
//! start; nothing here ever runs on the UI thread (ADR-013).
//!
//! A scan is a long job: it runs cursor by cursor inside the task, pushing
//! each page into the shared tree so the sidebar fills progressively, and it
//! stops when the UI clears the scan flag.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hs_core::runtime::UiWaker;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::client::{
    ClientRow, ConnectConfig, Error, KeyInfo, KeyType, SCAN_COUNT, ServerInfo, Session, SlowEntry,
    StreamView, ValuePage,
};
use crate::console;
use crate::keytree::KeyTree;

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready(ServerInfo),
    Failed(String),
}

/// A console entry: what was typed and what came back.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub command: String,
    pub lines: Vec<console::Line>,
    pub error: bool,
    pub elapsed_ms: u128,
}

/// One edit the UI asked for, named so a failure can say what failed.
#[derive(Debug, Clone, PartialEq)]
pub enum Write {
    SetString {
        key: String,
        value: Vec<u8>,
        keep_ttl: bool,
    },
    HashSet {
        key: String,
        field: String,
        value: Vec<u8>,
    },
    HashDelete {
        key: String,
        field: String,
    },
    ListSet {
        key: String,
        index: i64,
        value: Vec<u8>,
    },
    ListPush {
        key: String,
        value: Vec<u8>,
        front: bool,
    },
    ListRemove {
        key: String,
        value: Vec<u8>,
    },
    SetAdd {
        key: String,
        value: Vec<u8>,
    },
    SetRemove {
        key: String,
        value: Vec<u8>,
    },
    ZSetAdd {
        key: String,
        member: Vec<u8>,
        score: f64,
    },
    ZSetRemove {
        key: String,
        member: Vec<u8>,
    },
    StreamDelete {
        key: String,
        id: String,
    },
    Rename {
        key: String,
        to: String,
    },
    /// `None` persists the key.
    Ttl {
        key: String,
        ms: Option<i64>,
    },
}

impl Write {
    pub fn key(&self) -> &str {
        match self {
            Write::SetString { key, .. }
            | Write::HashSet { key, .. }
            | Write::HashDelete { key, .. }
            | Write::ListSet { key, .. }
            | Write::ListPush { key, .. }
            | Write::ListRemove { key, .. }
            | Write::SetAdd { key, .. }
            | Write::SetRemove { key, .. }
            | Write::ZSetAdd { key, .. }
            | Write::ZSetRemove { key, .. }
            | Write::StreamDelete { key, .. }
            | Write::Rename { key, .. }
            | Write::Ttl { key, .. } => key,
        }
    }
}

pub enum Cmd {
    /// Start a scan from scratch with this pattern.
    Scan {
        pattern: String,
    },
    /// Read a key's header and its first page.
    Open {
        key: String,
    },
    /// Another page of the open key.
    More {
        key: String,
        kind: KeyType,
        cursor: u64,
        start: u64,
    },
    /// Read the whole of a large string.
    Whole {
        key: String,
    },
    Run(Write),
    /// Delete keys outright; the UI has already confirmed.
    Delete {
        keys: Vec<String>,
    },
    /// Count what a pattern would delete, for the confirmation dialog.
    CountPrefix {
        pattern: String,
    },
    Console {
        line: String,
        args: Vec<String>,
    },
    Select {
        db: i64,
    },
    RefreshInfo,
    // ---- depth programme, phase A ----
    Slowlog,
    Clients,
    /// `CLIENT KILL ID`; the UI has already confirmed.
    KillClient {
        id: String,
    },
    /// The big-key finder: SCAN with `MEMORY USAGE`, top `top` by size.
    BigKeys {
        pattern: String,
        top: usize,
    },
    /// The dry run of a bulk operation: what a pattern matches.
    BulkPreview {
        pattern: String,
    },
    /// Apply a bulk operation to the keys the preview listed; confirmed.
    Bulk {
        action: BulkAction,
        keys: Vec<String>,
        pattern: String,
        /// Seconds for Expire, the new prefix for RenamePrefix.
        arg: String,
    },
    StreamInfo {
        key: String,
        count: usize,
    },
    Subscribe {
        channels: Vec<String>,
        patterns: Vec<String>,
    },
    Unsubscribe,
    Publish {
        channel: String,
        message: String,
    },
}

/// What the UI reads each frame.
#[derive(Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub tree: KeyTree,
    /// The scan is still walking the keyspace.
    pub scanning: bool,
    /// Keys seen by the scan so far, including those a filter hid.
    pub scanned: u64,
    pub open_key: Option<KeyInfo>,
    pub page: Option<ValuePage>,
    /// Pages loaded after the first, appended to `page`.
    pub loading_page: bool,
    pub console: Vec<Entry>,
    pub info: Option<String>,
    pub keyspace: Vec<(i64, u64)>,
    pub db: i64,
    /// (ok, text) drained as toasts.
    pub messages: Vec<(bool, String)>,
    /// A prefix count the dialog is waiting for: (pattern, keys, capped).
    pub prefix_count: Option<(String, Vec<String>, bool)>,
    pub busy: bool,
    // ---- depth programme, phase A ----
    /// The last five minutes of INFO readings, newest last.
    pub samples: std::collections::VecDeque<Sample>,
    /// `ui.input().time` of the last reading the dashboard asked for.
    pub info_at: f64,
    pub slowlog: Vec<SlowEntry>,
    pub clients: Vec<ClientRow>,
    pub bigkeys: Vec<BigKey>,
    pub bigkeys_running: bool,
    pub bigkeys_scanned: u64,
    /// Set by the UI to stop the big-key scan at its next page.
    pub bigkeys_stop: bool,
    pub bulk: Option<BulkPreview>,
    pub stream: Option<StreamView>,
    pub pubsub_on: bool,
    pub pubsub_msgs: Vec<PubMsg>,
    /// Dropping it ends the subscriber task.
    pub pubsub_stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Shared {
    fn say(&mut self, ok: bool, text: impl Into<String>) {
        self.messages.push((ok, text.into()));
    }

    fn fail(&mut self, what: &str, e: &Error) {
        self.say(false, format!("{what}: {e}"));
    }
}

/// Set while a scan should keep going; cleared to stop it.
pub type ScanFlag = Arc<AtomicBool>;

/// Connect and serve commands until the channel closes (tab closed or
/// reconnecting). Returns the command sender and the scan flag.
pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    config: impl std::future::Future<Output = Result<ConnectConfig, String>> + Send + 'static,
    shared: Arc<Mutex<Shared>>,
    delimiter: String,
) -> (mpsc::Sender<Cmd>, ScanFlag) {
    let (tx, mut rx) = mpsc::channel::<Cmd>(16);
    let scanning: ScanFlag = Arc::new(AtomicBool::new(false));
    let flag = scanning.clone();
    let waker = runtime.waker();
    {
        let mut sh = shared.lock();
        sh.status = Some(Status::Connecting);
        sh.tree = KeyTree::new(&delimiter);
    }
    runtime.spawn(async move {
        let cfg = match config.await {
            Ok(c) => c,
            Err(e) => return set_failed(&shared, &waker, e),
        };
        let mut session = match Session::connect(&cfg).await {
            Ok(s) => s,
            Err(e) => return set_failed(&shared, &waker, e.to_string()),
        };
        {
            let mut sh = shared.lock();
            sh.status = Some(Status::Ready(session.server_info().clone()));
            sh.db = session.db();
        }
        let keyspace = session.keyspace().await;
        shared.lock().keyspace = keyspace;
        waker.request_repaint();
        while let Some(cmd) = rx.recv().await {
            serve(cmd, &mut session, &shared, &waker, &flag).await;
            shared.lock().busy = false;
            waker.request_repaint();
        }
    });
    (tx, scanning)
}

fn set_failed(shared: &Arc<Mutex<Shared>>, waker: &Arc<dyn UiWaker>, e: String) {
    shared.lock().status = Some(Status::Failed(e));
    waker.request_repaint();
}

async fn serve(
    cmd: Cmd,
    session: &mut Session,
    shared: &Arc<Mutex<Shared>>,
    waker: &Arc<dyn UiWaker>,
    flag: &ScanFlag,
) {
    match cmd {
        Cmd::Scan { pattern } => {
            {
                let mut sh = shared.lock();
                sh.tree.clear();
                sh.scanned = 0;
                sh.scanning = true;
            }
            flag.store(true, Ordering::Relaxed);
            let mut cursor = 0u64;
            loop {
                if !flag.load(Ordering::Relaxed) {
                    break;
                }
                let page = match session.scan(cursor, &pattern, SCAN_COUNT).await {
                    Ok(p) => p,
                    Err(e) => {
                        shared.lock().fail("scan", &e);
                        break;
                    }
                };
                cursor = page.cursor;
                let types = session.types(&page.keys).await.unwrap_or_default();
                {
                    let mut sh = shared.lock();
                    for (i, k) in page.keys.iter().enumerate() {
                        sh.tree.insert(k, types.get(i).cloned());
                    }
                    sh.scanned += page.keys.len() as u64;
                }
                waker.request_repaint();
                if cursor == 0 {
                    break;
                }
            }
            flag.store(false, Ordering::Relaxed);
            shared.lock().scanning = false;
        }
        Cmd::Open { key } => match session.key_info(&key).await {
            Ok(info) => {
                let kind = info.kind.clone().unwrap_or(KeyType::None);
                let page = session.read(&key, &kind, 0, 0, false).await;
                let mut sh = shared.lock();
                match page {
                    Ok(p) => {
                        sh.page = Some(p);
                        sh.open_key = Some(info);
                    }
                    Err(e) => {
                        sh.page = None;
                        sh.fail("read", &e);
                    }
                }
            }
            Err(e) => {
                let mut sh = shared.lock();
                sh.open_key = None;
                sh.page = None;
                sh.fail("read", &e);
            }
        },
        Cmd::More {
            key,
            kind,
            cursor,
            start,
        } => {
            shared.lock().loading_page = true;
            match session.read(&key, &kind, cursor, start, false).await {
                Ok(p) => {
                    let mut sh = shared.lock();
                    sh.page = Some(merge(sh.page.take(), p));
                }
                Err(e) => shared.lock().fail("read", &e),
            }
            shared.lock().loading_page = false;
        }
        Cmd::Whole { key } => match session.read(&key, &KeyType::String, 0, 0, true).await {
            Ok(p) => shared.lock().page = Some(p),
            Err(e) => shared.lock().fail("read", &e),
        },
        Cmd::Run(w) => {
            let key = w.key().to_string();
            let r = apply(session, &w).await;
            // The guard is dropped before the next await: a parking_lot
            // guard is not Send, and the task must be.
            {
                let mut sh = shared.lock();
                match r {
                    Ok(()) => sh.say(true, format!("{key} updated")),
                    Err(e) => sh.fail("write", &e),
                }
            }
            // The header and the page both move after a write.
            if let Ok(info) = session.key_info(&key).await {
                let kind = info.kind.clone().unwrap_or(KeyType::None);
                if let Ok(p) = session.read(&key, &kind, 0, 0, false).await {
                    let mut sh = shared.lock();
                    sh.page = Some(p);
                    sh.open_key = Some(info);
                }
            }
        }
        Cmd::Delete { keys } => match session.delete(&keys).await {
            Ok(n) => {
                let mut sh = shared.lock();
                for k in &keys {
                    sh.tree.remove(k);
                }
                if sh.open_key.as_ref().is_some_and(|i| keys.contains(&i.key)) {
                    sh.open_key = None;
                    sh.page = None;
                }
                sh.say(true, format!("{n} deleted"));
            }
            Err(e) => shared.lock().fail("delete", &e),
        },
        Cmd::CountPrefix { pattern } => match session.count_prefix(&pattern, 10_000).await {
            Ok((keys, capped)) => {
                shared.lock().prefix_count = Some((pattern, keys, capped));
            }
            Err(e) => shared.lock().fail("scan", &e),
        },
        Cmd::Console { line, args } => {
            let start = std::time::Instant::now();
            let r = session.command(&args).await;
            let elapsed_ms = start.elapsed().as_millis();
            let entry = match r {
                Ok(v) => Entry {
                    command: line,
                    lines: console::render(&v),
                    error: false,
                    elapsed_ms,
                },
                Err(e) => Entry {
                    command: line,
                    lines: vec![console::Line {
                        depth: 0,
                        prefix: String::new(),
                        text: format!("(error) {e}"),
                        error: true,
                    }],
                    error: true,
                    elapsed_ms,
                },
            };
            let mut sh = shared.lock();
            sh.console.push(entry);
            sh.db = session.db();
        }
        Cmd::Select { db } => match session.select(db).await {
            Ok(()) => {
                let mut sh = shared.lock();
                sh.db = db;
                sh.tree.clear();
                sh.open_key = None;
                sh.page = None;
            }
            Err(e) => shared.lock().fail("select", &e),
        },
        Cmd::Slowlog => match session.slowlog(128).await {
            Ok(rows) => shared.lock().slowlog = rows,
            Err(e) => shared.lock().fail("slowlog", &e),
        },
        Cmd::Clients => match session.client_list().await {
            Ok(rows) => shared.lock().clients = rows,
            Err(e) => shared.lock().fail("client list", &e),
        },
        Cmd::KillClient { id } => {
            match session.client_kill(&id).await {
                Ok(()) => shared.lock().say(true, format!("client {id} closed")),
                Err(e) => shared.lock().fail("client kill", &e),
            }
            if let Ok(rows) = session.client_list().await {
                shared.lock().clients = rows;
            }
        }
        Cmd::BigKeys { pattern, top } => big_keys(session, shared, waker, &pattern, top).await,
        Cmd::BulkPreview { pattern } => match session.count_prefix(&pattern, 10_000).await {
            Ok((keys, capped)) => {
                shared.lock().bulk = Some(BulkPreview {
                    pattern,
                    keys,
                    capped,
                })
            }
            Err(e) => shared.lock().fail("scan", &e),
        },
        Cmd::Bulk {
            action,
            keys,
            pattern,
            arg,
        } => bulk(session, shared, action, keys, &pattern, &arg).await,
        Cmd::StreamInfo { key, count } => match session.stream_view(&key, count).await {
            Ok(v) => shared.lock().stream = Some(v),
            Err(e) => shared.lock().fail("xinfo", &e),
        },
        Cmd::Subscribe { channels, patterns } => {
            subscribe(session, shared, waker, channels, patterns).await
        }
        Cmd::Unsubscribe => {
            let mut sh = shared.lock();
            sh.pubsub_stop = None;
            sh.pubsub_on = false;
        }
        Cmd::Publish { channel, message } => match session.publish(&channel, &message).await {
            Ok(n) => shared
                .lock()
                .say(true, format!("published to {n} subscriber(s)")),
            Err(e) => shared.lock().fail("publish", &e),
        },
        Cmd::RefreshInfo => {
            let text = session.info("everything").await;
            let keyspace = session.keyspace().await;
            let mut sh = shared.lock();
            sh.keyspace = keyspace;
            match text {
                Ok(t) => {
                    sh.samples.push_back(parse_sample(&t));
                    while sh.samples.len() > SAMPLES {
                        sh.samples.pop_front();
                    }
                    sh.info = Some(t);
                }
                Err(e) => sh.fail("info", &e),
            }
        }
    }
}

async fn apply(session: &mut Session, w: &Write) -> Result<(), Error> {
    match w {
        Write::SetString {
            key,
            value,
            keep_ttl,
        } => session.set_string(key, value, *keep_ttl).await,
        Write::HashSet { key, field, value } => session.hset(key, field, value).await,
        Write::HashDelete { key, field } => session.hdel(key, field).await,
        Write::ListSet { key, index, value } => session.lset(key, *index, value).await,
        Write::ListPush { key, value, front } => {
            if *front {
                session.lpush(key, value).await
            } else {
                session.rpush(key, value).await
            }
        }
        Write::ListRemove { key, value } => session.lrem(key, value).await.map(|_| ()),
        Write::SetAdd { key, value } => session.sadd(key, value).await,
        Write::SetRemove { key, value } => session.srem(key, value).await,
        Write::ZSetAdd { key, member, score } => session.zadd(key, member, *score).await,
        Write::ZSetRemove { key, member } => session.zrem(key, member).await,
        Write::StreamDelete { key, id } => session.xdel(key, id).await,
        Write::Rename { key, to } => session.rename(key, to).await,
        Write::Ttl { key, ms } => session.set_ttl(key, *ms).await,
    }
}

// ---- depth programme, phase A (docs/tools/redis.md §7, §8) -------------

/// One reading of INFO for the dashboard.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sample {
    pub used_memory: u64,
    pub peak_memory: u64,
    pub clients: u64,
    pub ops: u64,
    pub hits: u64,
    pub misses: u64,
    pub uptime: u64,
    pub evicted: u64,
    pub expired: u64,
    pub fragmentation: f64,
    pub role: String,
    pub persistence: String,
}

/// Five minutes at one reading every two seconds.
pub const SAMPLES: usize = 150;

/// The figures the dashboard shows, out of an `INFO everything` text.
pub fn parse_sample(text: &str) -> Sample {
    let mut s = Sample::default();
    let mut aof = false;
    let mut replicas = 0u64;
    let mut role = String::new();
    for line in text.lines() {
        let Some((k, v)) = line.trim_end().split_once(':') else {
            continue;
        };
        let n = || v.parse::<u64>().unwrap_or(0);
        match k {
            "used_memory" => s.used_memory = n(),
            "used_memory_peak" => s.peak_memory = n(),
            "connected_clients" => s.clients = n(),
            "instantaneous_ops_per_sec" => s.ops = n(),
            "keyspace_hits" => s.hits = n(),
            "keyspace_misses" => s.misses = n(),
            "uptime_in_seconds" => s.uptime = n(),
            "evicted_keys" => s.evicted = n(),
            "expired_keys" => s.expired = n(),
            "mem_fragmentation_ratio" => s.fragmentation = v.parse().unwrap_or(0.0),
            "role" => role = v.to_string(),
            "connected_slaves" => replicas = n(),
            "aof_enabled" => aof = v == "1",
            _ => {}
        }
    }
    s.role = if replicas > 0 {
        format!("{role}, {replicas} replica(s)")
    } else {
        role
    };
    s.persistence = if aof { "AOF + RDB" } else { "RDB" }.to_string();
    s
}

/// One of the big-key finder's results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BigKey {
    pub key: String,
    pub bytes: u64,
    pub kind: String,
    pub len: u64,
    /// Seconds, or negative for a key that never expires.
    pub ttl: i64,
}

/// The dry run a bulk operation shows before it asks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkPreview {
    pub pattern: String,
    pub keys: Vec<String>,
    /// More than the cap matched; the operation refuses until narrowed.
    pub capped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    Delete,
    Expire,
    Persist,
    RenamePrefix,
}

impl BulkAction {
    pub fn verb(self) -> &'static str {
        match self {
            BulkAction::Delete => "delete",
            BulkAction::Expire => "expire",
            BulkAction::Persist => "persist",
            BulkAction::RenamePrefix => "rename",
        }
    }
}

/// A message a subscription received.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PubMsg {
    /// Unix seconds.
    pub at: i64,
    pub channel: String,
    pub payload: String,
}

/// The part of a glob pattern before its first wildcard: what a rename by
/// prefix strips from each key.
pub fn literal_prefix(pattern: &str) -> &str {
    let end = pattern.find(['*', '?', '[']).unwrap_or(pattern.len());
    &pattern[..end]
}

/// `(from, to)` for every key the pattern matched: the literal prefix is
/// replaced by `to`. Keys that do not start with it (a `*` in the middle)
/// are left alone.
pub fn rename_pairs(keys: &[String], pattern: &str, to: &str) -> Vec<(String, String)> {
    let prefix = literal_prefix(pattern);
    keys.iter()
        .filter_map(|k| {
            k.strip_prefix(prefix)
                .map(|rest| (k.clone(), format!("{to}{rest}")))
        })
        .collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// SCAN with `MEMORY USAGE` per page, keeping the `top` largest; type,
/// length and TTL are read for those only, at the end. Stops at the next
/// page when the UI sets `bigkeys_stop`.
async fn big_keys(
    session: &mut Session,
    shared: &Arc<Mutex<Shared>>,
    waker: &Arc<dyn UiWaker>,
    pattern: &str,
    top: usize,
) {
    {
        let mut sh = shared.lock();
        sh.bigkeys.clear();
        sh.bigkeys_running = true;
        sh.bigkeys_scanned = 0;
        sh.bigkeys_stop = false;
        sh.busy = true;
    }
    let mut best: Vec<(u64, String)> = Vec::new();
    let mut cursor = 0u64;
    let mut failed: Option<Error> = None;
    loop {
        if shared.lock().bigkeys_stop {
            break;
        }
        let page = match session.scan(cursor, pattern, SCAN_COUNT).await {
            Ok(p) => p,
            Err(e) => {
                failed = Some(e);
                break;
            }
        };
        cursor = page.cursor;
        let n = page.keys.len() as u64;
        match session.memory_usages(&page.keys).await {
            Ok(sizes) => {
                for (k, size) in page.keys.into_iter().zip(sizes) {
                    if let Some(b) = size {
                        best.push((b, k));
                    }
                }
                best.sort_by_key(|a| std::cmp::Reverse(a.0));
                best.truncate(top);
            }
            Err(e) => {
                failed = Some(e);
                break;
            }
        }
        shared.lock().bigkeys_scanned += n;
        waker.request_repaint();
        if cursor == 0 {
            break;
        }
    }
    let mut out = Vec::with_capacity(best.len());
    for (bytes, key) in best {
        let info = session.key_info(&key).await.ok();
        out.push(BigKey {
            kind: info
                .as_ref()
                .and_then(|i| i.kind.as_ref())
                .map(|k| k.badge().to_string())
                .unwrap_or_default(),
            len: info.as_ref().and_then(|i| i.len).unwrap_or(0),
            ttl: info
                .as_ref()
                .and_then(|i| i.ttl_ms)
                .map(|ms| ms / 1000)
                .unwrap_or(-1),
            key,
            bytes,
        });
    }
    let mut sh = shared.lock();
    sh.bigkeys = out;
    sh.bigkeys_running = false;
    if let Some(e) = failed {
        sh.fail("big keys", &e);
    }
}

/// Apply a confirmed bulk operation and fix the tree up afterwards.
async fn bulk(
    session: &mut Session,
    shared: &Arc<Mutex<Shared>>,
    action: BulkAction,
    keys: Vec<String>,
    pattern: &str,
    arg: &str,
) {
    let mut renamed: Vec<(String, String)> = Vec::new();
    let res = match action {
        BulkAction::Delete => session.delete(&keys).await,
        BulkAction::Expire => match arg.trim().parse::<i64>() {
            Ok(s) if s > 0 => session.expire_many(&keys, s).await,
            _ => Err(Error::msg("seconds must be a positive number")),
        },
        BulkAction::Persist => session.persist_many(&keys).await,
        BulkAction::RenamePrefix => {
            renamed = rename_pairs(&keys, pattern, arg.trim());
            session.rename_many(&renamed).await
        }
    };
    let mut sh = shared.lock();
    sh.bulk = None;
    match res {
        Ok(n) => {
            sh.say(true, format!("{}: {n} key(s)", action.verb()));
            match action {
                BulkAction::Delete => {
                    for k in &keys {
                        sh.tree.remove(k);
                    }
                }
                BulkAction::RenamePrefix => {
                    for (from, to) in &renamed {
                        sh.tree.remove(from);
                        sh.tree.insert(to, None);
                    }
                }
                _ => {}
            }
        }
        Err(e) => sh.fail(action.verb(), &e),
    }
}

/// Open the Pub/Sub connection, subscribe, and read it on its own task
/// until the UI drops the stop handle or the server closes it.
async fn subscribe(
    session: &mut Session,
    shared: &Arc<Mutex<Shared>>,
    waker: &Arc<dyn UiWaker>,
    channels: Vec<String>,
    patterns: Vec<String>,
) {
    let mut ps = match session.pubsub().await {
        Ok(p) => p,
        Err(e) => return shared.lock().fail("subscribe", &e),
    };
    for c in &channels {
        if let Err(e) = ps.subscribe(c).await {
            return shared.lock().fail("subscribe", &e.into());
        }
    }
    for p in &patterns {
        if let Err(e) = ps.psubscribe(p).await {
            return shared.lock().fail("psubscribe", &e.into());
        }
    }
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();
    {
        let mut sh = shared.lock();
        sh.pubsub_stop = Some(stop_tx);
        sh.pubsub_on = true;
    }
    let shared = shared.clone();
    let waker = waker.clone();
    tokio::spawn(async move {
        use futures_util::StreamExt;
        let mut stream = ps.into_on_message();
        let mut stopped = false;
        loop {
            tokio::select! {
                _ = &mut stop_rx => {
                    stopped = true;
                    break;
                }
                msg = stream.next() => {
                    let Some(msg) = msg else { break };
                    let payload = String::from_utf8_lossy(msg.get_payload_bytes()).into_owned();
                    let mut sh = shared.lock();
                    sh.pubsub_msgs.push(PubMsg {
                        at: now_secs(),
                        channel: msg.get_channel_name().to_string(),
                        payload,
                    });
                    if sh.pubsub_msgs.len() > 2_000 {
                        sh.pubsub_msgs.drain(..1_000);
                    }
                    drop(sh);
                    waker.request_repaint();
                }
            }
        }
        if !stopped {
            let mut sh = shared.lock();
            sh.pubsub_on = false;
            sh.pubsub_stop = None;
            sh.say(false, "the subscription was closed by the server");
            drop(sh);
            waker.request_repaint();
        }
    });
}

/// Append a page to the one already on screen, keeping the earlier items.
fn merge(old: Option<ValuePage>, new: ValuePage) -> ValuePage {
    match (old, new) {
        (Some(ValuePage::Pairs { items: mut a, .. }), ValuePage::Pairs { items: b, cursor }) => {
            a.extend(b);
            ValuePage::Pairs { items: a, cursor }
        }
        (
            Some(ValuePage::Items {
                items: mut a,
                start,
                ..
            }),
            ValuePage::Items {
                items: b, cursor, ..
            },
        ) => {
            a.extend(b);
            ValuePage::Items {
                items: a,
                cursor,
                start,
            }
        }
        (Some(ValuePage::Scored { items: mut a, .. }), ValuePage::Scored { items: b, cursor }) => {
            a.extend(b);
            ValuePage::Scored { items: a, cursor }
        }
        (Some(ValuePage::Entries { items: mut a }), ValuePage::Entries { items: b }) => {
            a.extend(b);
            ValuePage::Entries { items: a }
        }
        (_, new) => new,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_appends_to_the_one_on_screen() {
        let first = ValuePage::Items {
            items: vec![b"a".to_vec()],
            cursor: 200,
            start: 0,
        };
        let second = ValuePage::Items {
            items: vec![b"b".to_vec()],
            cursor: 0,
            start: 200,
        };
        match merge(Some(first), second) {
            ValuePage::Items {
                items,
                cursor,
                start,
            } => {
                assert_eq!(items.len(), 2);
                // The new cursor wins; the start stays where the page began.
                assert_eq!((cursor, start), (0, 0));
            }
            other => panic!("wrong page: {other:?}"),
        }
    }

    #[test]
    fn a_page_of_another_type_replaces_rather_than_appends() {
        let text = ValuePage::Text {
            bytes: b"x".to_vec(),
            total: 1,
        };
        let pairs = ValuePage::Pairs {
            items: vec![("f".into(), b"v".to_vec())],
            cursor: 0,
        };
        assert!(matches!(merge(Some(text), pairs), ValuePage::Pairs { .. }));
    }

    #[test]
    fn every_write_names_the_key_it_touches() {
        let w = Write::HashSet {
            key: "cart:77".into(),
            field: "sku".into(),
            value: b"2".to_vec(),
        };
        assert_eq!(w.key(), "cart:77");
        assert_eq!(
            Write::Ttl {
                key: "s:1".into(),
                ms: None
            }
            .key(),
            "s:1"
        );
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;

    #[test]
    fn a_sample_reads_the_dashboard_figures() {
        let text = "# Server\r\nuptime_in_seconds:91\r\n# Clients\r\nconnected_clients:3\r\n# Memory\r\nused_memory:1024\r\nused_memory_peak:2048\r\nmem_fragmentation_ratio:1.25\r\n# Persistence\r\naof_enabled:1\r\n# Stats\r\ninstantaneous_ops_per_sec:7\r\nkeyspace_hits:90\r\nkeyspace_misses:10\r\nevicted_keys:1\r\nexpired_keys:2\r\n# Replication\r\nrole:master\r\nconnected_slaves:2\r\n";
        let s = parse_sample(text);
        assert_eq!(s.uptime, 91);
        assert_eq!(s.clients, 3);
        assert_eq!(s.used_memory, 1024);
        assert_eq!(s.peak_memory, 2048);
        assert!((s.fragmentation - 1.25).abs() < 1e-9);
        assert_eq!(s.ops, 7);
        assert_eq!((s.hits, s.misses), (90, 10));
        assert_eq!((s.evicted, s.expired), (1, 2));
        assert_eq!(s.role, "master, 2 replica(s)");
        assert_eq!(s.persistence, "AOF + RDB");
        assert_eq!(parse_sample("role:slave\r\n").role, "slave");
        assert_eq!(parse_sample("").persistence, "RDB");
    }

    #[test]
    fn a_rename_by_prefix_swaps_the_literal_part() {
        assert_eq!(literal_prefix("session:*"), "session:");
        assert_eq!(literal_prefix("a?b"), "a");
        assert_eq!(literal_prefix("plain"), "plain");
        let keys = vec![
            "session:1".to_string(),
            "session:2".to_string(),
            "other:3".to_string(),
        ];
        let pairs = rename_pairs(&keys, "session:*", "archive:session:");
        assert_eq!(
            pairs,
            vec![
                ("session:1".to_string(), "archive:session:1".to_string()),
                ("session:2".to_string(), "archive:session:2".to_string()),
            ]
        );
    }
}
