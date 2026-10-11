//! The tab's connection, owned by one task on the tokio runtime. The UI sends
//! [`Cmd`]s through a bounded channel and reads [`Shared`] at frame start;
//! nothing here ever runs on the UI thread (ADR-013). Rows land in the
//! shared result as each batch arrives, so a long query fills the grid
//! progressively and can be cancelled through the session's canceller.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use hs_core::runtime::UiWaker;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::driver::{
    self, Canceller, Catalog, Column, ConnectConfig, DbError, DbSession, ExecSummary, Flow,
    ResultSink, Row, SchemaObject, Scope, ServerInfo,
};
use crate::history::{History, NewEntry};
use crate::sql::Class;

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Load<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Connecting,
    Ready(ServerInfo),
    Failed(String),
    Closed,
}

/// One statement's result tab.
#[derive(Debug, Clone, Default)]
pub struct ResultSet {
    pub sql: String,
    pub class: Option<Class>,
    /// Where the statement sits in the editor text (bytes), for error marks.
    pub offset: usize,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub summary: Option<ExecSummary>,
    pub error: Option<DbError>,
    pub running: bool,
    /// Not run because an earlier statement failed.
    pub skipped: bool,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RunStmt {
    pub sql: String,
    pub class: Class,
    pub offset: usize,
}

pub enum Cmd {
    /// Run statements in order, stopping at the first error. `begin_first`
    /// opens a transaction when none is open (manual-commit mode).
    Run {
        stmts: Vec<RunStmt>,
        limit: Option<usize>,
        timeout: Option<Duration>,
        begin_first: bool,
    },
    /// A transaction-control statement (COMMIT, ROLLBACK) reported as a
    /// message, not a result tab.
    Control(String),
    Introspect {
        node: String,
        scope: Scope,
    },
    Catalog,
}

#[derive(Default)]
pub struct Shared {
    pub status: Option<Status>,
    pub canceller: Option<Canceller>,
    pub results: Vec<ResultSet>,
    /// Bumped whenever `results` is replaced by a new run.
    pub run_id: u64,
    pub running: bool,
    pub in_tx: bool,
    pub nodes: HashMap<String, Load<Vec<SchemaObject>>>,
    pub catalog: Option<Catalog>,
    /// Transaction-control outcomes and other messages, drained as toasts.
    pub messages: Vec<(bool, String)>,
    /// History changed; the UI reloads its list.
    pub history_changed: bool,
}

/// What the runner needs to record history.
#[derive(Clone)]
pub struct HistoryCtx {
    pub history: Arc<History>,
    pub workspace_id: String,
    pub connection_id: String,
    pub environment_id: Option<String>,
    /// Values redacted from stored statements (the password).
    pub secrets: Vec<String>,
}

struct TabSink<'a> {
    shared: &'a Mutex<Shared>,
    index: usize,
    limit: Option<usize>,
    waker: &'a Arc<dyn UiWaker>,
}

impl ResultSink for TabSink<'_> {
    fn columns(&mut self, cols: Vec<Column>) {
        if let Some(r) = self.shared.lock().results.get_mut(self.index) {
            r.columns = cols;
        }
        self.waker.request_repaint();
    }
    fn rows(&mut self, batch: Vec<Row>) -> Flow {
        let mut sh = self.shared.lock();
        let Some(r) = sh.results.get_mut(self.index) else {
            return Flow::Stop;
        };
        r.rows.extend(batch);
        let flow = match self.limit {
            Some(l) if r.rows.len() >= l => {
                r.rows.truncate(l);
                Flow::Stop
            }
            _ => Flow::Continue,
        };
        drop(sh);
        self.waker.request_repaint();
        flow
    }
}

/// Run one statement with an optional timeout; on timeout the server is
/// asked to cancel and the statement's own error (cancelled) is replaced.
async fn exec(
    session: &mut Box<dyn DbSession>,
    sql: &str,
    sink: &mut dyn ResultSink,
    timeout: Option<Duration>,
) -> Result<ExecSummary, DbError> {
    let cancel = session.canceller();
    let fut = session.execute(sql, sink);
    tokio::pin!(fut);
    match timeout {
        None => fut.await,
        Some(t) => {
            tokio::select! {
                r = &mut fut => r,
                _ = tokio::time::sleep(t) => {
                    cancel();
                    let _ = fut.await;
                    Err(DbError::msg(format!("timed out after {} s and was cancelled", t.as_secs())))
                }
            }
        }
    }
}

/// Connect and serve commands until the channel closes (tab closed or
/// reconnecting). Returns the command sender for the UI.
pub fn spawn(
    runtime: &hs_core::runtime::AppRuntime,
    config: impl std::future::Future<Output = Result<ConnectConfig, String>> + Send + 'static,
    shared: Arc<Mutex<Shared>>,
    history: HistoryCtx,
) -> mpsc::Sender<Cmd> {
    let (tx, mut rx) = mpsc::channel::<Cmd>(16);
    let waker = runtime.waker();
    shared.lock().status = Some(Status::Connecting);
    runtime.spawn(async move {
        let fail = |e: String| {
            let mut sh = shared.lock();
            sh.status = Some(Status::Failed(e));
            sh.running = false;
            drop(sh);
            waker.request_repaint();
        };
        let cfg = match config.await {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        let mut session = match driver::connect(&cfg).await {
            Ok(s) => s,
            Err(e) => return fail(e.to_string()),
        };
        {
            let mut sh = shared.lock();
            sh.status = Some(Status::Ready(session.server_info().clone()));
            sh.canceller = Some(session.canceller());
        }
        waker.request_repaint();
        while let Some(cmd) = rx.recv().await {
            match cmd {
                Cmd::Run {
                    stmts,
                    limit,
                    timeout,
                    begin_first,
                } => {
                    if begin_first && !session.in_transaction() {
                        let mut sink = driver::Collect::default();
                        if let Err(e) = session.execute("BEGIN", &mut sink).await {
                            shared.lock().messages.push((false, format!("BEGIN: {e}")));
                        }
                    }
                    let mut failed = false;
                    for (index, st) in stmts.iter().enumerate() {
                        if failed {
                            if let Some(r) = shared.lock().results.get_mut(index) {
                                r.skipped = true;
                                r.running = false;
                            }
                            continue;
                        }
                        if let Some(r) = shared.lock().results.get_mut(index) {
                            r.running = true;
                        }
                        waker.request_repaint();
                        let mut sink = TabSink {
                            shared: &shared,
                            index,
                            limit,
                            waker: &waker,
                        };
                        let r = exec(&mut session, &st.sql, &mut sink, timeout).await;
                        let (status, rows, error, ms) = match &r {
                            Ok(s) => (
                                "ok",
                                s.rows_affected.or(Some(s.rows_returned)).map(|n| n as i64),
                                None,
                                s.elapsed.as_millis() as i64,
                            ),
                            Err(e) => ("error", None, Some(e.message.clone()), 0),
                        };
                        {
                            let mut sh = shared.lock();
                            if let Some(res) = sh.results.get_mut(index) {
                                res.running = false;
                                match r {
                                    Ok(s) => res.summary = Some(s),
                                    Err(e) => {
                                        res.error = Some(e);
                                        failed = true;
                                    }
                                }
                            }
                            sh.in_tx = session.in_transaction();
                        }
                        waker.request_repaint();
                        let h = history.clone();
                        let sql = st.sql.clone();
                        let _ = tokio::task::spawn_blocking(move || {
                            h.history.add(
                                &NewEntry {
                                    workspace_id: &h.workspace_id,
                                    connection_id: &h.connection_id,
                                    environment_id: h.environment_id.as_deref(),
                                    sql: &sql,
                                    duration_ms: ms,
                                    row_count: rows,
                                    status,
                                    error: error.as_deref(),
                                },
                                &h.secrets,
                            )
                        })
                        .await;
                    }
                    let mut sh = shared.lock();
                    sh.running = false;
                    sh.history_changed = true;
                    sh.in_tx = session.in_transaction();
                    drop(sh);
                    waker.request_repaint();
                }
                Cmd::Control(sql) => {
                    let mut sink = driver::Collect::default();
                    let r = session.execute(&sql, &mut sink).await;
                    let mut sh = shared.lock();
                    sh.messages.push(match r {
                        Ok(_) => (true, sql.clone()),
                        Err(e) => (false, format!("{sql}: {e}")),
                    });
                    sh.in_tx = session.in_transaction();
                    sh.running = false;
                    drop(sh);
                    waker.request_repaint();
                }
                Cmd::Introspect { node, scope } => {
                    let r = session.introspect(&scope).await;
                    shared.lock().nodes.insert(
                        node,
                        match r {
                            Ok(v) => Load::Ready(v),
                            Err(e) => Load::Failed(e.to_string()),
                        },
                    );
                    waker.request_repaint();
                }
                Cmd::Catalog => {
                    if let Ok(c) = session.catalog().await {
                        shared.lock().catalog = Some(c);
                    }
                }
            }
        }
        shared.lock().status = Some(Status::Closed);
    });
    tx
}

/// Run one statement to a writer on a connection of its own (exports of a
/// whole result as a Job, while the tab's session stays free).
pub async fn export_all(
    cfg: ConnectConfig,
    sql: String,
    sink: &mut dyn ResultSink,
) -> Result<ExecSummary, DbError> {
    let mut s = driver::connect(&cfg).await?;
    s.execute(&sql, sink).await
}
