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
    ConnectConfig, Error, KeyInfo, KeyType, SCAN_COUNT, ServerInfo, Session, ValuePage,
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
        Cmd::RefreshInfo => {
            let text = session.info("everything").await;
            let keyspace = session.keyspace().await;
            let mut sh = shared.lock();
            sh.keyspace = keyspace;
            match text {
                Ok(t) => sh.info = Some(t),
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
