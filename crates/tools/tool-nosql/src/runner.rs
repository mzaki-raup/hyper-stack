//! The tab's session, owned by one task on the tokio runtime. The UI sends
//! [`Cmd`]s through a bounded channel and reads [`Shared`] at frame start;
//! nothing here ever runs on the UI thread (ADR-013).

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::store::{
    BulkSummary, ContainerInfo, ContainerRef, DocStore, Document, Error, FindSummary, IndexInfo,
    PutMode, Query, Scope, StoreKind,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready { kind: StoreKind, version: String },
    Failed(String),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

impl<T> Load<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Load::Ready(v) => Some(v),
            _ => None,
        }
    }
}

pub enum Cmd {
    Databases,
    Containers {
        database: String,
    },
    Find {
        container: ContainerRef,
        query: Query,
    },
    Count {
        container: ContainerRef,
        filter: String,
    },
    Save {
        container: ContainerRef,
        doc: Document,
        mode: PutMode,
    },
    Delete {
        container: ContainerRef,
        doc: Document,
    },
    DeleteMany {
        container: ContainerRef,
        filter: String,
        dry_run: bool,
    },
    Indexes {
        container: ContainerRef,
    },
    Aggregate {
        container: ContainerRef,
        pipeline: String,
        limit: i64,
    },
    Explain {
        container: ContainerRef,
        query: Query,
    },
}

/// What the UI reads each frame.
#[derive(Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub databases: Load<Vec<String>>,
    /// Containers per database, filled as the tree opens.
    pub containers: HashMap<String, Load<Vec<ContainerInfo>>>,
    pub docs: Vec<Document>,
    pub summary: Option<FindSummary>,
    pub finding: bool,
    /// An exact count the user asked for.
    pub count: Option<u64>,
    pub indexes: Load<Vec<IndexInfo>>,
    pub explain: Option<serde_json::Value>,
    /// A bulk delete's count, waiting for the dialog.
    pub bulk: Option<BulkSummary>,
    /// (ok, text) drained as toasts.
    pub messages: Vec<(bool, String)>,
    /// Bumped when `docs` is replaced, so the view resets its selection.
    pub run_id: u64,
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

/// Connect and serve commands until the channel closes.
pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    open: impl std::future::Future<Output = Result<Box<dyn DocStore>, String>> + Send + 'static,
    shared: Arc<Mutex<Shared>>,
) -> mpsc::Sender<Cmd> {
    let (tx, mut rx) = mpsc::channel::<Cmd>(16);
    let waker = runtime.waker();
    shared.lock().status = Some(Status::Connecting);
    runtime.spawn(async move {
        let mut store = match open.await {
            Ok(s) => s,
            Err(e) => {
                shared.lock().status = Some(Status::Failed(e));
                waker.request_repaint();
                return;
            }
        };
        {
            let mut sh = shared.lock();
            sh.status = Some(Status::Ready {
                kind: store.kind(),
                version: store.version().to_string(),
            });
            sh.databases = Load::Loading;
        }
        waker.request_repaint();
        match store.databases().await {
            Ok(mut dbs) => {
                dbs.sort();
                shared.lock().databases = Load::Ready(dbs);
            }
            Err(e) => shared.lock().databases = Load::Failed(e.to_string()),
        }
        waker.request_repaint();
        while let Some(cmd) = rx.recv().await {
            serve(cmd, store.as_mut(), &shared).await;
            shared.lock().busy = false;
            waker.request_repaint();
        }
    });
    tx
}

async fn serve(cmd: Cmd, store: &mut dyn DocStore, shared: &Arc<Mutex<Shared>>) {
    match cmd {
        Cmd::Databases => {
            shared.lock().databases = Load::Loading;
            match store.databases().await {
                Ok(mut dbs) => {
                    dbs.sort();
                    shared.lock().databases = Load::Ready(dbs);
                }
                Err(e) => shared.lock().databases = Load::Failed(e.to_string()),
            }
        }
        Cmd::Containers { database } => {
            shared
                .lock()
                .containers
                .insert(database.clone(), Load::Loading);
            let r = store
                .containers(&Scope {
                    database: database.clone(),
                })
                .await;
            let load = match r {
                Ok(c) => Load::Ready(c),
                Err(e) => Load::Failed(e.to_string()),
            };
            shared.lock().containers.insert(database, load);
        }
        Cmd::Find { container, query } => {
            shared.lock().finding = true;
            let r = store.find(&container, &query).await;
            let mut sh = shared.lock();
            sh.finding = false;
            sh.count = None;
            sh.run_id += 1;
            match r {
                Ok((docs, summary)) => {
                    sh.docs = docs;
                    sh.summary = Some(summary);
                }
                Err(e) => {
                    sh.docs.clear();
                    sh.summary = None;
                    sh.fail("find", &e);
                }
            }
        }
        Cmd::Count { container, filter } => match store.count(&container, &filter).await {
            Ok(n) => shared.lock().count = Some(n),
            Err(e) => shared.lock().fail("count", &e),
        },
        Cmd::Save {
            container,
            doc,
            mode,
        } => {
            let inserting = mode == PutMode::Insert;
            match store.put(&container, &doc, mode).await {
                Ok(saved) => {
                    let mut sh = shared.lock();
                    sh.say(
                        true,
                        if inserting {
                            format!("{} inserted", saved.id)
                        } else {
                            format!("{} saved", saved.id)
                        },
                    );
                    // Keep the list in step with what was written.
                    if let Some(slot) = sh.docs.iter_mut().find(|d| d.id == saved.id) {
                        *slot = saved;
                    } else {
                        sh.docs.insert(0, saved);
                    }
                }
                Err(e) => shared.lock().fail("save", &e),
            }
        }
        Cmd::Delete { container, doc } => match store.delete(&container, &doc).await {
            Ok(()) => {
                let mut sh = shared.lock();
                sh.docs.retain(|d| d.id != doc.id);
                sh.say(true, format!("{} deleted", doc.id));
            }
            Err(e) => shared.lock().fail("delete", &e),
        },
        Cmd::DeleteMany {
            container,
            filter,
            dry_run,
        } => match store.delete_many(&container, &filter, dry_run).await {
            Ok(summary) => {
                let mut sh = shared.lock();
                if !summary.dry_run {
                    sh.say(true, format!("{} deleted", summary.changed));
                }
                sh.bulk = Some(summary);
            }
            Err(e) => shared.lock().fail("delete", &e),
        },
        Cmd::Indexes { container } => {
            shared.lock().indexes = Load::Loading;
            let load = match store.indexes(&container).await {
                Ok(i) => Load::Ready(i),
                Err(e) => Load::Failed(e.to_string()),
            };
            shared.lock().indexes = load;
        }
        Cmd::Aggregate {
            container,
            pipeline,
            limit,
        } => {
            shared.lock().finding = true;
            let r = store.aggregate(&container, &pipeline, limit).await;
            let mut sh = shared.lock();
            sh.finding = false;
            sh.run_id += 1;
            match r {
                Ok(docs) => {
                    sh.summary = Some(FindSummary {
                        returned: docs.len(),
                        elapsed: std::time::Duration::ZERO,
                        more: false,
                    });
                    sh.docs = docs;
                }
                Err(e) => {
                    sh.docs.clear();
                    sh.fail("aggregate", &e);
                }
            }
        }
        Cmd::Explain { container, query } => match store.explain(&container, &query).await {
            Ok(v) => shared.lock().explain = Some(v),
            Err(e) => shared.lock().fail("explain", &e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_load_only_yields_what_is_ready() {
        let l: Load<Vec<String>> = Load::Loading;
        assert!(l.ready().is_none());
        let l = Load::Ready(vec!["shop".to_string()]);
        assert_eq!(l.ready().map(|v| v.len()), Some(1));
        let l: Load<Vec<String>> = Load::Failed("nope".into());
        assert!(l.ready().is_none());
    }

    #[test]
    fn messages_carry_whether_they_went_well() {
        let mut sh = Shared::default();
        sh.say(true, "saved");
        sh.fail("find", &Error::msg("broken"));
        assert_eq!(sh.messages[0], (true, "saved".to_string()));
        assert_eq!(sh.messages[1], (false, "find: broken".to_string()));
    }
}
