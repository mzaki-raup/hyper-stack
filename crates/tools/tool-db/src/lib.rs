//! Database tool (docs/tools/database.md): one tab = one SQL editor bound to
//! one `kind = "db"` connection (and so to its environment), with its own
//! server session owned by a task on the runtime ([`runner`]). The sidebar
//! lists DB profiles and the selected one's schema tree, history and saved
//! queries; the main area is the editor over the result tabs.
//!
//! Safety (docs/06-security.md §3–4): every statement is classified before it
//! runs. Read-only profiles, and protected environments until writes are
//! allowed for the tab, refuse anything that writes; UPDATE/DELETE without
//! WHERE, DROP/TRUNCATE and admin statements ask first, and on a protected
//! environment ask for the object's name to be typed.

pub mod complete;
pub mod driver;
pub mod export;
pub mod grid;
pub mod history;
pub mod profile;
pub mod runner;
pub mod searcher;
pub mod sql;

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use hs_core::Services;
use hs_core::events::Event;
use hs_core::tool::{
    Accept, Action, CloseDecision, CommandDescriptor, OpenRequest, QuickOpenItem, ToastLevel, Tool,
    ToolCategory, ToolContext, ToolDescriptor, ToolFactory,
};
use hs_editor::buffer::EditKind;
use hs_editor::view::EditorTheme;
use hs_editor::{Buffer, Cursor, Cursors, EditorView, languages};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use hs_ui::tree::{self, TreeNode, TreeState};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::driver::{ConnectConfig, Engine, ObjectKind, Scope, Value};
use crate::export::Format;
use crate::grid::{GridMenu, GridState};
use crate::profile::{DbFields, ProfileError};
use crate::runner::{Cmd, Load, ResultSet, RunStmt, Shared, Status};
use crate::sql::Class;

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "db",
    title_key: "tool-db",
    icon: ph::DATABASE,
    category: ToolCategory::Data,
    accepts: &[Accept::Connection, Accept::LocalFile],
};

/// Separator inside tree node ids (never part of an identifier).
const SEP: char = '\u{1f}';
const LIMITS: &[usize] = &[100, 500, 1000, 5000, 0];
const TIMEOUTS: &[u64] = &[30, 300, 0];
/// Rows added by Fetch More.
const MORE: usize = 500;

pub struct DbFactory;

impl ToolFactory for DbFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(DbTool::new(services, state))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct State {
    conn_id: Option<String>,
    /// The editor text (restored with the tab; never holds secrets the user
    /// did not type).
    sql: String,
    /// Saved query file, relative to the workspace root. The active tab's,
    /// kept for the paths that still ask about "the" query.
    query_file: Option<String>,
    /// One entry per query tab: the saved query it holds, or `None` for a
    /// tab that has never been saved. Restoring a tab reopens them.
    queries: Vec<Option<String>>,
    /// Which query tab is in front.
    active_query: usize,
    /// Row limit; 0 = none.
    limit: usize,
    /// Statement timeout in seconds; 0 = none.
    timeout_secs: u64,
    manual_tx: bool,
    /// Editor share of the main area's height.
    editor_frac: f32,
}

impl Default for State {
    fn default() -> Self {
        Self {
            conn_id: None,
            sql: String::new(),
            query_file: None,
            queries: Vec::new(),
            active_query: 0,
            limit: 500,
            timeout_secs: 30,
            manual_tx: false,
            editor_frac: 0.42,
        }
    }
}

/// A saved query: `.sql` with a one-line JSON header (docs/05 §2.6).
#[derive(Debug, Clone, PartialEq)]
struct SavedQuery {
    path: PathBuf,
    name: String,
    connection: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct QueryHeader {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    connection: String,
}

const HEADER_PREFIX: &str = "-- hyperstack: ";

fn read_saved(path: &Path) -> Option<(QueryHeader, String)> {
    let text = std::fs::read_to_string(path).ok()?;
    let (first, rest) = text.split_once('\n').unwrap_or((&text, ""));
    let header = first
        .trim_end_matches('\r')
        .strip_prefix(HEADER_PREFIX)
        .and_then(|j| serde_json::from_str::<QueryHeader>(j).ok());
    match header {
        Some(h) => Some((h, rest.to_string())),
        None => Some((QueryHeader::default(), text)),
    }
}

/// The display name of a saved query: its header name, else the file stem.
fn saved_name(path: &Path, header: Option<QueryHeader>) -> String {
    header
        .map(|h| h.name)
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|x| x.to_string_lossy().to_string())
                .unwrap_or_default()
        })
}

/// Run what: the statement at the cursor (or the selection), the whole
/// script, or the current statement's plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Current,
    Script,
    Explain,
}

enum Dialog {
    /// Dangerous statements waiting for a yes (and the typed name on a
    /// protected environment).
    Confirm {
        stmts: Vec<RunStmt>,
        dangerous: Vec<(String, Class, Option<String>)>,
        need: Option<String>,
        typed: String,
        limit: Option<usize>,
    },
    AllowWrites,
    Cell {
        title: String,
        text: String,
        /// The raw value of a binary cell, for the picture/document preview
        /// and Save As. `None` for text cells.
        bytes: Option<Vec<u8>>,
        sniffed: Option<hs_core::sniff::Sniffed>,
        /// Image size when the bytes decode as a picture.
        image_size: Option<(u32, u32)>,
    },
    SaveQuery {
        name: String,
    },
}

struct Popup {
    completion: complete::Completion,
    /// Byte offset of the statement the completion was computed in.
    base: usize,
    selected: usize,
}

pub struct DbTool {
    state: State,
    /// Tool-level id for the popups and combos that belong to the tab
    /// rather than to one query, so they keep working across query tabs.
    id: egui::Id,
    /// The query tabs, in strip order.
    docs: Vec<QueryDoc>,
    shared: Arc<Mutex<Shared>>,
    cmd: Option<mpsc::Sender<Cmd>>,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    conn_name: String,
    engine: Engine,
    read_only_profile: bool,
    env_id: Option<String>,
    env_name: String,
    protected: bool,
    writes_unlocked: bool,
    /// Resolved config for exports on a second connection (memory only).
    export_cfg: Option<ConnectConfig>,
    tree: TreeState,
    tree_filter: String,
    menu: Option<(String, egui::Pos2)>,
    active_result: usize,
    grids: Vec<GridState>,
    seen_run: u64,
    popup: Option<Popup>,
    dialog: Option<Dialog>,
    history: Arc<history::History>,
    history_rows: Arc<Mutex<Vec<history::Entry>>>,
    history_search: String,
    saved: Vec<SavedQuery>,
    catalog_cached: bool,
    /// The saved query's name (from its header), for the tab title.
    query_name: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

fn char_to_byte(text: &str, ci: usize) -> usize {
    text.char_indices()
        .nth(ci)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

fn byte_to_char(text: &str, bi: usize) -> usize {
    text[..bi.min(text.len())].chars().count()
}

fn class_key(c: Class) -> &'static str {
    match c {
        Class::Read => "db-class-read",
        Class::Session => "db-class-session",
        Class::Write => "db-class-write",
        Class::WriteUnbounded => "db-class-write-unbounded",
        Class::Ddl => "db-class-ddl",
        Class::DdlDestructive => "db-class-ddl-destructive",
        Class::Admin => "db-class-admin",
    }
}

fn kind_key(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Schema => "db-kind-schemas",
        ObjectKind::Table => "db-kind-tables",
        ObjectKind::View => "db-kind-views",
        ObjectKind::Function => "db-kind-functions",
        ObjectKind::Sequence => "db-kind-sequences",
        ObjectKind::Column => "db-kind-columns",
        ObjectKind::Index => "db-kind-indexes",
    }
}

fn kind_id(k: ObjectKind) -> &'static str {
    match k {
        ObjectKind::Schema => "schema",
        ObjectKind::Table => "table",
        ObjectKind::View => "view",
        ObjectKind::Function => "function",
        ObjectKind::Sequence => "sequence",
        ObjectKind::Column => "column",
        ObjectKind::Index => "index",
    }
}

fn kind_from(s: &str) -> Option<ObjectKind> {
    Some(match s {
        "schema" => ObjectKind::Schema,
        "table" => ObjectKind::Table,
        "view" => ObjectKind::View,
        "function" => ObjectKind::Function,
        "sequence" => ObjectKind::Sequence,
        "column" => ObjectKind::Column,
        "index" => ObjectKind::Index,
        _ => return None,
    })
}

fn node_id(parts: &[&str]) -> String {
    parts.join(&SEP.to_string())
}

fn short_sql(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 {
        format!("{}…", one.chars().take(90).collect::<String>())
    } else {
        one
    }
}

/// Query tab identities, unique for the life of the process so a strip
/// never confuses two of them.
static NEXT_QUERY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// One query tab inside a connection's tab: its own buffer, cursors and
/// view, so moving between them keeps each caret and scroll where it was.
/// A connection's tab holds as many as the user opens; the shell's tab
/// holds exactly one connection. Owner's requirement, 2026-10-17.
struct QueryDoc {
    /// Identity for the tab strip, stable across reorders.
    uid: u64,
    buf: Buffer,
    cursors: Cursors,
    view: EditorView,
    /// The saved query this came from, relative to the workspace root.
    file: Option<String>,
    /// What the tab says: the saved query's name, or nothing for one that
    /// has never been saved.
    name: Option<String>,
}

impl QueryDoc {
    fn new(services: &Services, text: &str) -> Self {
        let st = services.settings.read();
        let dark = Theme::from_settings(&st.appearance).dark;
        let mut view = EditorView::new(
            egui::Id::new(("hs-db-editor", hs_core::new_id())),
            languages::detect("query.sql"),
            dark,
            st.editor.font_size,
        );
        view.right_click_copy_paste = st.editor.right_click_copy_paste;
        drop(st);
        let mut buf = Buffer::from_text(text);
        buf.mark_saved();
        Self {
            uid: NEXT_QUERY.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            buf,
            cursors: Cursors::single(0),
            view,
            file: None,
            name: None,
        }
    }

    /// What the tab says.
    fn label(&self, untitled: &str) -> String {
        self.name.clone().unwrap_or_else(|| untitled.to_string())
    }
}

/// Statements the tool writes for the user end in a semicolon. It is what
/// they would have typed, it survives being pasted into another client,
/// and the query tab is a script — Run Script and Prettify both read the
/// buffer as a sequence of statements, so an unterminated one is the odd
/// case rather than the normal one. Owner's requirement, 2026-10-17.
fn terminated(sql: &str) -> String {
    let sql = sql.trim_end();
    if sql.ends_with(';') {
        sql.to_string()
    } else {
        format!("{sql};")
    }
}

/// Which query tab a statement generated from the schema tree should land
/// in: the first tab that is unsaved *and* empty, so Open Data fills the
/// blank tab in front of the user rather than stacking another beside it.
/// A tab holding anything typed is never reused, saved or not — losing
/// someone's half-written query to a context-menu click would be worse
/// than an extra tab.
fn reusable_tab(tabs: impl Iterator<Item = (bool, String)>) -> Option<usize> {
    tabs.enumerate()
        .find(|(_, (unsaved, text))| *unsaved && text.trim().is_empty())
        .map(|(i, _)| i)
}

/// Lay a statement out again: one clause to a line, indented, keywords
/// as they were typed. `sqlformat` is dialect-agnostic, which suits a tool
/// that speaks three of them. Owner's requirement, 2026-10-17.
fn prettify_sql(sql: &str, indent: u8) -> String {
    let options = sqlformat::FormatOptions {
        indent: sqlformat::Indent::Spaces(indent),
        lines_between_queries: 1,
        // Keywords stay as the user typed them: this lays a statement out,
        // it does not have opinions about case.
        uppercase: None,
        ..Default::default()
    };
    sqlformat::format(sql, &sqlformat::QueryParams::None, &options)
}

impl DbTool {
    fn doc(&self) -> &QueryDoc {
        let i = self
            .state
            .active_query
            .min(self.docs.len().saturating_sub(1));
        &self.docs[i]
    }

    fn doc_mut(&mut self) -> &mut QueryDoc {
        let i = self
            .state
            .active_query
            .min(self.docs.len().saturating_sub(1));
        &mut self.docs[i]
    }

    fn new(services: &Services, state: State) -> Self {
        let first = QueryDoc::new(services, "");
        let mut t = Self {
            id: egui::Id::new(("hs-db", hs_core::new_id())),
            docs: vec![first],
            shared: Arc::new(Mutex::new(Shared::default())),
            cmd: None,
            conn_error: None,
            waiting_for_vault: false,
            conn_name: String::new(),
            engine: Engine::Postgres,
            read_only_profile: false,
            env_id: None,
            env_name: String::new(),
            protected: false,
            writes_unlocked: false,
            export_cfg: None,
            tree: TreeState::default(),
            tree_filter: String::new(),
            menu: None,
            active_result: 0,
            grids: Vec::new(),
            seen_run: 0,
            popup: None,
            dialog: None,
            history: Arc::new(history::History::new(services.paths.history_db())),
            history_rows: Arc::new(Mutex::new(Vec::new())),
            history_search: String::new(),
            saved: Vec::new(),
            catalog_cached: false,
            query_name: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
            state,
        };
        // Reopen the query tabs this tab had. A saved one comes back with
        // the file's text; one that was never saved comes back empty,
        // because the layout keeps exactly one buffer's text — the tab
        // that was in front — and that is put back afterwards.
        let restored: Vec<Option<String>> = t.state.queries.clone();
        for (i, file) in restored.iter().enumerate() {
            if i > 0 {
                t.docs.push(QueryDoc::new(services, ""));
            }
            let Some(rel) = file.clone() else { continue };
            let path = services.workspace.read().root().join(&rel);
            let name = saved_name(&path, read_saved(&path).map(|h| h.0));
            if let Some((_, body)) = read_saved(&path) {
                t.docs[i].buf = Buffer::from_text(&body);
            }
            let d = &mut t.docs[i];
            d.buf.mark_saved();
            d.file = Some(rel);
            d.name = Some(name);
        }
        if t.state.queries.is_empty()
            && let Some(rel) = t.state.query_file.clone()
        {
            let path = services.workspace.read().root().join(&rel);
            let name = saved_name(&path, read_saved(&path).map(|h| h.0));
            let d = t.doc_mut();
            d.file = Some(rel);
            d.name = Some(name);
        }
        t.state.active_query = t.state.active_query.min(t.docs.len() - 1);
        // The one buffer the layout keeps is the front tab's, so it goes
        // back there rather than into the first tab.
        if !t.state.sql.is_empty() {
            let sql = t.state.sql.clone();
            t.doc_mut().buf = Buffer::from_text(&sql);
        }
        t.doc_mut().buf.mark_saved();
        t.query_name = t.doc().name.clone();
        if t.state.conn_id.is_some() {
            t.connect(services);
        }
        t
    }

    // ---- connection ---------------------------------------------------------

    fn catalog_cache_file(&self, s: &Services) -> Option<PathBuf> {
        let id = self.state.conn_id.as_ref()?;
        Some(
            s.paths
                .cache_dir()
                .join("db-catalog")
                .join(format!("{id}.json")),
        )
    }

    fn connect(&mut self, s: &Services) {
        self.cmd = None; // closes the old session
        self.shared = Arc::new(Mutex::new(Shared::default()));
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.popup = None;
        self.grids.clear();
        self.seen_run = 0;
        self.tree = TreeState::default();
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("db-no-profile"));
                return;
            };
            (
                DbFields::from_table(&c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.read_only_profile = read_only;
        self.env_id = env_id.clone();
        {
            // A profile without an environment follows the active one, as
            // the banner does (guards and {{variables}} alike).
            let envs = s.environments.read();
            let env = env_id
                .as_deref()
                .or(envs.active.as_deref())
                .and_then(|e| envs.by_id(e));
            self.protected = env.is_some_and(|e| e.protected);
            self.env_name = env.map(|e| e.name.clone()).unwrap_or_default();
        }
        let Some(engine) = fields.engine() else {
            self.conn_error = Some(
                s.i18n
                    .t_args("db-unknown-engine", &[("engine", &fields.engine)]),
            );
            return;
        };
        self.engine = engine;
        self.doc_mut().view.error_ranges.clear();
        let session_read_only = read_only || (self.protected && !self.writes_unlocked);
        let lookup = {
            let envs = s.environments.read().clone();
            let env_id = env_id.clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let cfg = match fields.resolve(session_read_only, s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault = e == ProfileError::VaultLocked;
                self.conn_error = Some(match e {
                    ProfileError::VaultLocked => s.t("db-vault-locked"),
                    ProfileError::UnknownEngine(x) => {
                        s.i18n.t_args("db-unknown-engine", &[("engine", &x)])
                    }
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("db-missing-vars", &[("names", &v.join(", "))]),
                    ProfileError::Secret(e) => e,
                });
                return;
            }
        };
        let secrets: Vec<String> = cfg.password.iter().cloned().collect();
        // Cached catalog: completion works before the server answers.
        if let Some(f) = self.catalog_cache_file(s)
            && let Ok(text) = std::fs::read_to_string(&f)
            && let Ok(c) = serde_json::from_str(&text)
        {
            self.shared.lock().catalog = Some(c);
        }
        self.catalog_cached = false;
        // SSH tunnel: ask the Tunnels service for a local port first.
        let tunnel = (!fields.tunnel_ssh.is_empty() && engine != Engine::Sqlite).then(|| {
            s.tunnels
                .ensure_local_forward(&fields.tunnel_ssh, &cfg.host, cfg.port)
        });
        let mut cfg_for_export = cfg.clone();
        let future = async move {
            let mut cfg = cfg;
            if let Some(rx) = tunnel {
                let port = rx
                    .await
                    .map_err(|_| "the SSH tunnel stopped".to_string())?
                    .map_err(|e| format!("SSH tunnel: {e}"))?;
                cfg.host = "127.0.0.1".into();
                cfg.port = port;
            }
            Ok(cfg)
        };
        if !fields.tunnel_ssh.is_empty() {
            // Exports reuse the same tunnel; it is shared per host and port.
            cfg_for_export.host = String::new();
        }
        self.export_cfg = Some(cfg_for_export);
        let ws_id = s.workspace.read().meta.id.clone();
        let ctx = runner::HistoryCtx {
            history: self.history.clone(),
            workspace_id: ws_id,
            connection_id: id,
            environment_id: env_id,
            secrets,
        };
        let tx = runner::spawn(&s.runtime, future, self.shared.clone(), ctx);
        let _ = tx.try_send(Cmd::Introspect {
            node: "root".into(),
            scope: Scope::Schemas,
        });
        let _ = tx.try_send(Cmd::Catalog);
        self.cmd = Some(tx);
        self.reload_history(s);
        self.reload_saved(s);
    }

    fn send(&mut self, cmd: Cmd, s: &Services) -> bool {
        match &self.cmd {
            Some(tx) => match tx.try_send(cmd) {
                Ok(()) => true,
                Err(_) => {
                    self.shared.lock().messages.push((false, s.t("db-busy")));
                    false
                }
            },
            None => false,
        }
    }

    fn ready(&self) -> bool {
        matches!(self.shared.lock().status, Some(Status::Ready(_)))
    }

    fn reload_history(&self, s: &Services) {
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let h = self.history.clone();
        let out = self.history_rows.clone();
        let search = self.history_search.clone();
        let waker = s.runtime.waker();
        s.runtime.spawn_blocking(move || {
            let _ = h.prune();
            if let Ok(rows) = h.recent(&id, &search, 100) {
                *out.lock() = rows;
                waker.request_repaint();
            }
        });
    }

    fn queries_dir(&self, s: &Services) -> PathBuf {
        let slug = hs_core::storage::slug(if self.conn_name.is_empty() {
            "queries"
        } else {
            &self.conn_name
        });
        s.workspace.read().root().join("queries").join(slug)
    }

    fn reload_saved(&mut self, s: &Services) {
        let mut out = Vec::new();
        let dir = s.workspace.read().root().join("queries");
        let conn = self.state.conn_id.clone().unwrap_or_default();
        if let Ok(folders) = std::fs::read_dir(&dir) {
            for folder in folders.flatten().filter(|e| e.path().is_dir()) {
                let Ok(files) = std::fs::read_dir(folder.path()) else {
                    continue;
                };
                for f in files.flatten() {
                    let p = f.path();
                    if p.extension().is_some_and(|e| e == "sql")
                        && let Some((h, _)) = read_saved(&p)
                        && (h.connection.is_empty() || h.connection == conn)
                    {
                        let name = if h.name.is_empty() {
                            p.file_stem()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default()
                        } else {
                            h.name
                        };
                        out.push(SavedQuery {
                            path: p,
                            name,
                            connection: h.connection,
                        });
                    }
                }
            }
        }
        out.sort_by_key(|q| q.name.to_lowercase());
        self.saved = out;
    }

    fn open_saved(&mut self, s: &Services, path: &Path) -> bool {
        let Some((h, body)) = read_saved(path) else {
            return false;
        };
        let name = saved_name(path, Some(h.clone()));
        self.query_name = Some(name.clone());
        let root = s.workspace.read().root().to_path_buf();
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        let d = self.doc_mut();
        d.buf = Buffer::from_text(&body);
        d.buf.mark_saved();
        d.cursors = Cursors::single(0);
        d.file = Some(rel.clone());
        d.name = Some(name);
        self.state.query_file = Some(rel);
        if !h.connection.is_empty() && self.state.conn_id.as_deref() != Some(&h.connection) {
            self.state.conn_id = Some(h.connection);
            self.connect(s);
        }
        true
    }

    fn save_query(&mut self, s: &Services, name: Option<String>) -> Result<(), String> {
        let root = s.workspace.read().root().to_path_buf();
        let (path, name, id) = match (&self.state.query_file, name) {
            (Some(rel), None) => {
                let p = root.join(rel);
                let h = read_saved(&p).map(|(h, _)| h).unwrap_or_default();
                let n = if h.name.is_empty() {
                    p.file_stem()
                        .map(|x| x.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    h.name
                };
                let id = if h.id.is_empty() {
                    hs_core::new_id()
                } else {
                    h.id
                };
                (p, n, id)
            }
            (_, Some(n)) => {
                let dir = self.queries_dir(s);
                (
                    dir.join(format!("{}.sql", hs_core::storage::slug(&n))),
                    n,
                    hs_core::new_id(),
                )
            }
            (None, None) => return Err(s.t("db-save-needs-name")),
        };
        let header = QueryHeader {
            id,
            name,
            connection: self.state.conn_id.clone().unwrap_or_default(),
        };
        let text = format!(
            "{HEADER_PREFIX}{}\n{}",
            serde_json::to_string(&header).map_err(|e| e.to_string())?,
            self.doc().buf.text()
        );
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        self.query_name = Some(header.name.clone());
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let name = header.name.clone();
        let d = self.doc_mut();
        d.buf.mark_saved();
        d.file = Some(rel.clone());
        d.name = Some(name);
        self.state.query_file = Some(rel);
        self.reload_saved(s);
        Ok(())
    }

    // ---- running ------------------------------------------------------------

    /// The text to run for `mode`: (text, its byte offset in the editor).
    fn pick_text(&self, mode: RunMode) -> Option<(String, usize)> {
        let text = self.doc().buf.text();
        let c = self.doc().cursors.primary();
        if c.has_selection() {
            let (a, b) = c.range();
            let (ba, bb) = (char_to_byte(&text, a), char_to_byte(&text, b));
            return Some((text[ba..bb].to_string(), ba));
        }
        if mode == RunMode::Script {
            return Some((text, 0));
        }
        let stmts = sql::split(&text, self.engine);
        let cursor = char_to_byte(&text, c.head);
        sql::statement_at(&stmts, cursor).map(|s| (s.text.clone(), s.start))
    }

    fn run(&mut self, mode: RunMode, s: &Services) {
        if self.shared.lock().running {
            self.shared
                .lock()
                .messages
                .push((false, s.t("db-still-running")));
            return;
        }
        if !self.ready() {
            return;
        }
        let Some((text, base)) = self.pick_text(mode) else {
            return;
        };
        self.run_text(&text, base, mode == RunMode::Explain, None, s);
    }

    /// Classify, guard and send. `base` is the text's byte offset in the
    /// editor (for error marks); `usize::MAX` for statements not from it.
    fn run_text(
        &mut self,
        text: &str,
        base: usize,
        explain: bool,
        limit: Option<usize>,
        s: &Services,
    ) {
        let substituted = match s
            .environments
            .read()
            .substitute(text, self.env_id.as_deref(), &[])
        {
            Ok(t) => t,
            Err(missing) => {
                self.shared.lock().messages.push((
                    false,
                    s.i18n
                        .t_args("db-missing-vars", &[("names", &missing.join(", "))]),
                ));
                return;
            }
        };
        // Offsets are only meaningful while no variable changed the text.
        let base = if substituted == text {
            base
        } else {
            usize::MAX
        };
        let explain_prefix = match self.engine {
            Engine::Sqlite => "EXPLAIN QUERY PLAN ",
            _ => "EXPLAIN ",
        };
        let stmts: Vec<RunStmt> = sql::split(&substituted, self.engine)
            .into_iter()
            .map(|st| {
                let sql = if explain {
                    format!("{explain_prefix}{}", st.text)
                } else {
                    st.text.clone()
                };
                RunStmt {
                    class: sql::classify(&sql, self.engine).class,
                    sql,
                    offset: base.saturating_add(st.start),
                }
            })
            .collect();
        self.popup = None;
        if stmts.is_empty() {
            return;
        }
        let read_only = self.read_only_profile || (self.protected && !self.writes_unlocked);
        if read_only && let Some(bad) = stmts.iter().find(|st| !st.class.is_read_only()) {
            let why = if self.read_only_profile {
                "db-refused-read-only"
            } else {
                "db-refused-protected"
            };
            let msg = s.i18n.t_args(
                why,
                &[
                    ("class", &s.t(class_key(bad.class))),
                    ("env", &self.env_name),
                ],
            );
            let mut sh = self.shared.lock();
            sh.run_id += 1;
            sh.results = vec![ResultSet {
                sql: bad.sql.clone(),
                class: Some(bad.class),
                offset: bad.offset,
                error: Some(driver::DbError::msg(msg)),
                ..Default::default()
            }];
            return;
        }
        let dangerous: Vec<(String, Class, Option<String>)> = stmts
            .iter()
            .filter(|st| st.class.is_dangerous() || (self.protected && st.class == Class::Ddl))
            .map(|st| {
                (
                    short_sql(&st.sql),
                    st.class,
                    sql::classify(&st.sql, self.engine).target,
                )
            })
            .collect();
        if !dangerous.is_empty() {
            let need = self.protected.then(|| {
                dangerous
                    .iter()
                    .find_map(|d| d.2.clone())
                    .unwrap_or_else(|| self.conn_name.clone())
            });
            self.dialog = Some(Dialog::Confirm {
                stmts,
                dangerous,
                need,
                typed: String::new(),
                limit,
            });
            return;
        }
        self.start_run(stmts, limit, s);
    }

    fn start_run(&mut self, stmts: Vec<RunStmt>, limit: Option<usize>, s: &Services) {
        let limit = limit.or((self.state.limit > 0).then_some(self.state.limit));
        let timeout =
            (self.state.timeout_secs > 0).then(|| Duration::from_secs(self.state.timeout_secs));
        {
            let mut sh = self.shared.lock();
            sh.run_id += 1;
            sh.running = true;
            sh.results = stmts
                .iter()
                .map(|st| ResultSet {
                    sql: st.sql.clone(),
                    class: Some(st.class),
                    offset: st.offset,
                    limit,
                    ..Default::default()
                })
                .collect();
        }
        self.doc_mut().view.error_ranges.clear();
        let begin_first = self.state.manual_tx;
        if !self.send(
            Cmd::Run {
                stmts,
                limit,
                timeout,
                begin_first,
            },
            s,
        ) {
            self.shared.lock().running = false;
        }
    }

    fn cancel(&self) {
        let c = self.shared.lock().canceller.clone();
        if let Some(c) = c {
            c();
        }
    }

    fn control(&mut self, sql: &str, s: &Services) {
        if self.shared.lock().running {
            return;
        }
        self.shared.lock().running = true;
        if !self.send(Cmd::Control(sql.into()), s) {
            self.shared.lock().running = false;
        }
    }

    fn fetch_more(&mut self, index: usize, s: &Services) {
        let (sql, class, offset, have) = {
            let sh = self.shared.lock();
            let Some(r) = sh.results.get(index) else {
                return;
            };
            (r.sql.clone(), r.class, r.offset, r.rows.len())
        };
        if class != Some(Class::Read) {
            return;
        }
        self.start_run(
            vec![RunStmt {
                sql,
                class: Class::Read,
                offset,
            }],
            Some(have + MORE),
            s,
        );
    }

    /// Qualified, quoted name of a tree object.
    fn qualified(&self, schema: &str, name: &str) -> String {
        let q = |x: &str| sql::quote_ident(x, self.engine);
        let default = match self.engine {
            Engine::Postgres => schema == "public",
            Engine::Sqlite => schema == "main",
            Engine::Mysql => false,
        };
        if default || schema.is_empty() {
            q(name)
        } else {
            format!("{}.{}", q(schema), q(name))
        }
    }

    fn insert_text(&mut self, text: &str) {
        let at = self.doc().buf.len_chars();
        let prefix = if self.doc().buf.text().trim().is_empty() {
            ""
        } else {
            "\n\n"
        };
        let d = self.doc_mut();
        d.cursors.set_single(Cursor::at(at));
        d.view.insert_at_cursors(
            &mut d.buf,
            &mut d.cursors,
            &format!("{prefix}{text}"),
            EditKind::Other,
        );
        d.view.scroll_to_cursor();
    }

    fn export(&mut self, index: usize, format: Format, s: &Services) {
        let (sql, columns) = {
            let sh = self.shared.lock();
            let Some(r) = sh.results.get(index) else {
                return;
            };
            (r.sql.clone(), r.columns.clone())
        };
        let Some(mut cfg) = self.export_cfg.clone() else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("result.{}", format.extension()))
            .save_file()
        else {
            return;
        };
        if cfg.host.is_empty() {
            // Tunnelled: reuse the tab's resolved address is not possible
            // without the tunnel; ask the Tunnels service again.
            let fields = self.state.conn_id.as_ref().and_then(|id| {
                s.connections
                    .read()
                    .by_id(id)
                    .map(|c| DbFields::from_table(&c.fields))
            });
            let Some(f) = fields else { return };
            let rx = s
                .tunnels
                .ensure_local_forward(&f.tunnel_ssh, &f.host, f.port);
            let engine = self.engine;
            let table = export::source_table(&sql, engine).unwrap_or_else(|| "exported".into());
            let _ = columns;
            self.export_job(s, path, format, sql, table, async move {
                let port = rx
                    .await
                    .map_err(|_| "the SSH tunnel stopped".to_string())?
                    .map_err(|e| e.to_string())?;
                cfg.host = "127.0.0.1".into();
                cfg.port = port;
                Ok(cfg)
            });
            return;
        }
        let table = export::source_table(&sql, self.engine).unwrap_or_else(|| "exported".into());
        self.export_job(s, path, format, sql, table, async move { Ok(cfg) });
    }

    fn export_job(
        &self,
        s: &Services,
        path: PathBuf,
        format: Format,
        sql: String,
        table: String,
        cfg: impl std::future::Future<Output = Result<ConnectConfig, String>> + Send + 'static,
    ) {
        let engine = self.engine;
        let shared = self.shared.clone();
        let done_msg = s
            .i18n
            .t_args("db-exported", &[("path", &path.to_string_lossy())]);
        let title = format!(
            "Export {}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        s.jobs.spawn(title, "db", move |h| async move {
            let cfg = cfg.await?;
            let file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
            let h2 = h.clone();
            let mut sink = export::FileSink {
                out: std::io::BufWriter::new(file),
                format,
                engine,
                table,
                columns: Vec::new(),
                written: 0,
                error: None,
                progress: Box::new(move |n| {
                    h2.set_message(format!("{n} rows"));
                    !h2.is_cancelled()
                }),
            };
            let r = runner::export_all(cfg, sql, &mut sink).await;
            let n = sink.finish().map_err(|e| e.to_string())?;
            r.map_err(|e| e.to_string())?;
            shared
                .lock()
                .messages
                .push((true, format!("{done_msg} ({n} rows)")));
            Ok(())
        });
    }

    // ---- completion ---------------------------------------------------------

    fn update_completion(&mut self, force: bool) {
        let text = self.doc().buf.text();
        let cursor = char_to_byte(&text, self.doc().cursors.primary().head);
        let before = &text[..cursor];
        let last = before.chars().last();
        let word_len = before
            .chars()
            .rev()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .count();
        if !force && last != Some('.') && word_len < 2 {
            self.popup = None;
            return;
        }
        let catalog = self.shared.lock().catalog.clone().unwrap_or_default();
        let stmts = sql::split(&text, self.engine);
        let (stmt_text, base) = match stmts.iter().find(|s| cursor >= s.start && cursor <= s.end) {
            Some(s) => (s.text.as_str(), s.start),
            None => {
                // Between statements: complete against the text since the
                // last statement ended.
                let start = stmts
                    .iter()
                    .rev()
                    .find(|s| s.end <= cursor)
                    .map(|s| s.end)
                    .unwrap_or(0);
                (&text[start..cursor], start)
            }
        };
        let c = complete::complete(stmt_text, cursor - base, self.engine, &catalog);
        if c.items.is_empty() {
            self.popup = None;
        } else {
            let selected = self
                .popup
                .as_ref()
                .map(|p| p.selected.min(c.items.len() - 1))
                .unwrap_or(0);
            self.popup = Some(Popup {
                completion: c,
                base,
                selected,
            });
        }
    }

    fn accept_completion(&mut self) {
        let Some(p) = self.popup.take() else { return };
        let Some(item) = p.completion.items.get(p.selected) else {
            return;
        };
        let text = self.doc().buf.text();
        let a = byte_to_char(&text, p.base + p.completion.start);
        let b = byte_to_char(&text, p.base + p.completion.end);
        let d = self.doc_mut();
        d.cursors.set_single(Cursor {
            head: b,
            anchor: a,
            goal_col: None,
        });
        d.view
            .insert_at_cursors(&mut d.buf, &mut d.cursors, &item.insert, EditKind::Other);
    }

    /// Keys the open completion popup takes before the editor sees them.
    fn completion_keys(&mut self, ui: &mut egui::Ui) {
        if self.popup.is_none() {
            return;
        }
        let n = self
            .popup
            .as_ref()
            .map(|p| p.completion.items.len())
            .unwrap_or(0);
        let (down, up, accept, close) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if let Some(p) = &mut self.popup {
            if down {
                p.selected = (p.selected + 1) % n.max(1);
            }
            if up {
                p.selected = (p.selected + n.max(1) - 1) % n.max(1);
            }
        }
        if accept {
            self.accept_completion();
        }
        if close {
            self.popup = None;
            // egui drops focus on Escape before the tool sees the key; the
            // Escape only meant "close the list", so keep typing here.
            ui.memory_mut(|m| m.request_focus(self.doc().view.id));
        }
    }

    fn completion_popup(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        let (Some(p), Some(rect)) = (&self.popup, self.doc().view.cursor_rect) else {
            return;
        };
        let mut clicked = None;
        egui::Area::new(self.doc().view.id.with("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_bottom() + egui::vec2(0.0, 2.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(260.0);
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            for (i, it) in p.completion.items.iter().enumerate() {
                                let icon = match it.kind {
                                    complete::Kind::Column => ph::COLUMNS,
                                    complete::Kind::Table => ph::TABLE,
                                    complete::Kind::Keyword => ph::TEXT_AA,
                                };
                                let r = ui.horizontal(|ui| {
                                    let r = ui.selectable_label(
                                        i == p.selected,
                                        format!("{icon} {}", it.label),
                                    );
                                    if !it.detail.is_empty() {
                                        ui.label(
                                            egui::RichText::new(&it.detail)
                                                .small()
                                                .color(theme.text_muted),
                                        );
                                    }
                                    r
                                });
                                if i == p.selected {
                                    r.inner.scroll_to_me(None);
                                }
                                if r.inner.clicked() {
                                    clicked = Some(i);
                                }
                            }
                        });
                });
            });
        if let Some(i) = clicked {
            if let Some(p) = &mut self.popup {
                p.selected = i;
            }
            self.accept_completion();
        }
    }

    // ---- frame pieces ---------------------------------------------------------

    fn drain(&mut self, cx: &mut ToolContext) {
        let s = cx.services;
        let (messages, history_changed, run_id, catalog) = {
            let mut sh = self.shared.lock();
            (
                std::mem::take(&mut sh.messages),
                std::mem::take(&mut sh.history_changed),
                sh.run_id,
                (!self.catalog_cached).then(|| sh.catalog.clone()).flatten(),
            )
        };
        for (ok, m) in messages {
            let text = match (ok, m.as_str()) {
                (true, "COMMIT") => s.t("db-committed"),
                (true, "ROLLBACK") => s.t("db-rolled-back"),
                _ => m,
            };
            cx.toast(
                if ok {
                    ToastLevel::Success
                } else {
                    ToastLevel::Error
                },
                text,
            );
        }
        if run_id != self.seen_run {
            self.seen_run = run_id;
            self.active_result = 0;
            self.grids.clear();
        }
        if history_changed {
            self.reload_history(s);
            self.mark_errors();
            // A finished run shows the statement that failed, if one did.
            if let Some(i) = self
                .shared
                .lock()
                .results
                .iter()
                .position(|r| r.error.is_some())
            {
                self.active_result = i;
            }
        }
        if let Some(c) = catalog
            && self.ready()
            && let Some(f) = self.catalog_cache_file(s)
        {
            self.catalog_cached = true;
            s.runtime.spawn_blocking(move || {
                if let Some(d) = f.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                let _ = std::fs::write(f, serde_json::to_string(&c).unwrap_or_default());
            });
        }
    }

    /// Underline the failing statement's error position in the editor.
    fn mark_errors(&mut self) {
        let text = self.doc().buf.text();
        // Worked out first and written after: the results are behind a lock
        // and the editor is behind `self`, and holding both at once is a
        // borrow the compiler is right to refuse.
        let mut ranges = Vec::new();
        let sh = self.shared.lock();
        for r in &sh.results {
            // Only where the server says where (a cancel or a lost
            // connection is not the text's fault).
            let Some(e) = r.error.as_ref().filter(|e| e.position.is_some()) else {
                continue;
            };
            if r.offset == usize::MAX || r.offset >= text.len() {
                continue;
            }
            let start = r.offset + e.position.unwrap_or(0);
            let end = match e.position {
                Some(_) => text[start.min(text.len())..]
                    .char_indices()
                    .find(|(_, c)| c.is_whitespace() || *c == ',' || *c == ';' || *c == ')')
                    .map(|(i, _)| start + i.max(1))
                    .unwrap_or(text.len()),
                None => r.offset + r.sql.len(),
            };
            ranges.push((
                byte_to_char(&text, start),
                byte_to_char(&text, end.min(text.len())),
            ));
        }
        drop(sh);
        self.doc_mut().view.error_ranges = ranges;
    }

    fn results_panel(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let (count, running) = {
            let sh = self.shared.lock();
            (sh.results.len(), sh.running)
        };
        if count == 0 {
            ui.add_space(8.0);
            let hint = if self.ready() {
                s.t("db-results-empty")
            } else {
                String::new()
            };
            if !hint.is_empty() {
                form::note(ui, theme, ph::LIGHTBULB, &hint);
            }
            return;
        }
        if self.grids.len() != count {
            self.grids.resize_with(count, GridState::default);
        }
        // Tabs: one per statement, then Messages.
        let messages_tab = count;
        ui.horizontal_wrapped(|ui| {
            let sh = self.shared.lock();
            for (i, r) in sh.results.iter().enumerate() {
                let (icon, color) = if r.running {
                    (ph::SPINNER_GAP, theme.accent)
                } else if r.error.is_some() {
                    (ph::X_CIRCLE, theme.error)
                } else if r.skipped || r.summary.is_none() {
                    (ph::MINUS_CIRCLE, theme.text_muted)
                } else {
                    (ph::CHECK_CIRCLE, theme.success)
                };
                let detail = match &r.summary {
                    Some(sm) if sm.has_result => s.i18n.t_args(
                        "db-tab-rows",
                        &[
                            ("rows", &r.rows.len().to_string()),
                            ("ms", &sm.elapsed.as_millis().to_string()),
                        ],
                    ),
                    Some(sm) => s.i18n.t_args(
                        "db-tab-affected",
                        &[
                            ("rows", &sm.rows_affected.unwrap_or(0).to_string()),
                            ("ms", &sm.elapsed.as_millis().to_string()),
                        ],
                    ),
                    None if r.running && !r.rows.is_empty() => format!("{} …", r.rows.len()),
                    None if r.running => "…".into(),
                    None => String::new(),
                };
                let name = s.i18n.t_args("db-result-n", &[("n", &(i + 1).to_string())]);
                let label = egui::RichText::new(if detail.is_empty() {
                    format!("{icon} {name}")
                } else {
                    format!("{icon} {name} · {detail}")
                })
                .color(if i == self.active_result {
                    theme.text
                } else {
                    color
                });
                let r = ui
                    .selectable_label(i == self.active_result, label)
                    .tip(short_sql(&r.sql));
                if r.clicked() {
                    self.active_result = i;
                }
            }
            if ui
                .selectable_label(
                    self.active_result == messages_tab,
                    format!("{} {}", ph::CHAT_TEXT, s.t("db-messages")),
                )
                .clicked()
            {
                self.active_result = messages_tab;
            }
            if running {
                ui.add_space(8.0);
                ui.spinner();
            }
        });
        ui.separator();
        if self.active_result >= count {
            self.messages(ui, s, theme);
            return;
        }
        let i = self.active_result;
        let (error, has_result, skipped, running_this) = {
            let sh = self.shared.lock();
            let r = &sh.results[i];
            (
                r.error.clone(),
                r.summary.as_ref().is_some_and(|x| x.has_result) || !r.columns.is_empty(),
                r.skipped,
                r.running,
            )
        };
        if let Some(e) = error {
            ui.add_space(4.0);
            ui.colored_label(theme.error, format!("{} {}", ph::X_CIRCLE, e));
            ui.add_space(4.0);
            let sql = self.shared.lock().results[i].sql.clone();
            ui.label(
                egui::RichText::new(short_sql(&sql))
                    .monospace()
                    .color(theme.text_muted),
            );
            return;
        }
        if skipped {
            form::note(ui, theme, ph::INFO, &s.t("db-skipped"));
            return;
        }
        if !has_result {
            let sh = self.shared.lock();
            let r = &sh.results[i];
            match &r.summary {
                Some(sm) => {
                    ui.add_space(6.0);
                    ui.label(s.i18n.t_args(
                        "db-affected",
                        &[
                            ("rows", &sm.rows_affected.unwrap_or(0).to_string()),
                            ("ms", &sm.elapsed.as_millis().to_string()),
                        ],
                    ));
                    if let Some(m) = &sm.message {
                        ui.label(egui::RichText::new(m).color(theme.text_muted));
                    }
                }
                None if running_this => {
                    ui.spinner();
                }
                None => {}
            }
            return;
        }
        // Footer first (bottom-up) so the grid takes the rest.
        let footer_h = 30.0;
        let grid_h = (ui.available_height() - footer_h).max(60.0);
        let labels = grid::Labels {
            null: "NULL",
            copy_cell: &s.t("db-copy-cell"),
            copy_row: &s.t("db-copy-row"),
            view_cell: &s.t("db-view-cell"),
            filter_value: &s.t("db-filter-value"),
        };
        let mut menu = None;
        let mut open_cell = None;
        let shown;
        let total;
        {
            let sh = self.shared.lock();
            let r = &sh.results[i];
            let state = &mut self.grids[i];
            let order = grid::order(&r.rows, state);
            shown = order.len();
            total = r.rows.len();
            ui.allocate_ui(egui::vec2(ui.available_width(), grid_h), |ui| {
                let resp = grid::show(
                    ui,
                    egui::Id::new(("db-grid", self.seen_run, i)),
                    theme,
                    &r.columns,
                    &r.rows,
                    state,
                    &labels,
                    &order,
                );
                menu = resp.menu;
                open_cell = resp.open_cell;
            });
        }
        if let Some((row, col)) = open_cell {
            self.open_cell(i, row, col, s);
        }
        if let Some((what, row, col)) = menu {
            self.grid_menu(ui, what, i, row, col, s);
        }
        // Footer. Laid out from the right, so a pane too narrow for the
        // whole row gives up the row count — which the grid also shows —
        // rather than slicing Export in half against the clip edge, which
        // is what a plain left-to-right row did once the AI assistant took
        // its 390 px. Wrapping is not the answer either: `footer_h` below
        // reserves one row, so a wrapped second row falls off the bottom.
        // Owner's report, 2026-10-17.
        let (limited, class) = {
            let sh = self.shared.lock();
            let r = &sh.results[i];
            (r.summary.as_ref().is_some_and(|x| x.limited), r.class)
        };
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(format!("{} {}", ph::EXPORT, s.t("db-export")), |ui| {
                    for f in [
                        Format::Csv,
                        Format::Json,
                        Format::Insert,
                        Format::Markdown,
                        Format::Tsv,
                    ] {
                        if ui.button(s.t(f.label_key())).clicked() {
                            ui.close();
                            self.export(i, f, s);
                        }
                    }
                });
                ui.menu_button(format!("{} {}", ph::COPY, s.t("db-copy-as")), |ui| {
                    for f in Format::ALL {
                        if ui.button(s.t(f.label_key())).clicked() {
                            let text = self.render_result(i, f);
                            ui.ctx().copy_text(text);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                // The filter gives up its width before anything else,
                // down to a box still worth typing in.
                let filter_w = (ui.available_width() - 140.0).clamp(70.0, 160.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.grids[i].filter)
                        .hint_text(s.t("db-grid-filter"))
                        .desired_width(filter_w),
                );
                ui.label(hs_ui::icons::SEARCH);
                ui.separator();
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let mut text = s
                        .i18n
                        .t_args("db-rows-count", &[("rows", &total.to_string())]);
                    if shown != total {
                        text = s.i18n.t_args(
                            "db-rows-filtered",
                            &[("shown", &shown.to_string()), ("rows", &total.to_string())],
                        );
                    }
                    if limited {
                        text.push_str(&format!(" · {}", s.t("db-limited")));
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).color(theme.text_muted))
                            .truncate(),
                    );
                    if limited
                        && class == Some(Class::Read)
                        && !running
                        && ui.button(s.t("db-fetch-more")).clicked()
                    {
                        self.fetch_more(i, s);
                    }
                });
            });
        });
    }

    fn render_result(&self, i: usize, f: Format) -> String {
        let sh = self.shared.lock();
        let r = &sh.results[i];
        let mut st = self.grids.get(i).cloned().unwrap_or_default();
        let order = grid::order(&r.rows, &mut st);
        let rows: Vec<&driver::Row> = order.iter().map(|&k| &r.rows[k]).collect();
        let table = export::source_table(&r.sql, self.engine).unwrap_or_else(|| "exported".into());
        export::render(f, &r.columns, &rows, self.engine, &table)
    }

    fn open_cell(&mut self, i: usize, row: usize, col: usize, s: &Services) {
        let sh = self.shared.lock();
        let r = &sh.results[i];
        let (Some(v), Some(c)) = (r.rows.get(row).and_then(|x| x.get(col)), r.columns.get(col))
        else {
            return;
        };
        let mut bytes = None;
        let mut sniffed = None;
        let mut image_size = None;
        let text = match v {
            Value::Null => "NULL".to_string(),
            // A JSON or XML column is laid out to be read. The text is
            // re-indented, never re-serialised, so the column's key order
            // and its numbers survive.
            Value::Text(t) => hs_core::fmt::pretty_auto(t).unwrap_or_else(|| t.clone()),
            Value::Bytes(b) => {
                // A picture is shown as a picture, text as text, a document
                // is named and offered as a file; the rest is a hex dump.
                let sn = hs_core::sniff::sniff(b);
                let shown = match sn.kind {
                    hs_core::sniff::Kind::Text => {
                        let text = String::from_utf8_lossy(b).into_owned();
                        hs_core::fmt::pretty_auto(&text).unwrap_or(text)
                    }
                    _ => hs_core::sniff::hex_dump(b, 4096),
                };
                if sn.kind == hs_core::sniff::Kind::Image && b.len() <= 64 * 1024 * 1024 {
                    image_size = image::load_from_memory(b)
                        .ok()
                        .map(|img| (img.width(), img.height()));
                }
                sniffed = Some(sn);
                bytes = Some(b.clone());
                shown
            }
            other => other.display(),
        };
        let title = if c.type_name.is_empty() {
            c.name.clone()
        } else {
            format!("{} · {}", c.name, c.type_name)
        };
        let _ = s;
        drop(sh);
        self.dialog = Some(Dialog::Cell {
            title,
            text,
            bytes,
            sniffed,
            image_size,
        });
    }

    fn grid_menu(
        &mut self,
        ui: &mut egui::Ui,
        what: GridMenu,
        i: usize,
        row: usize,
        col: usize,
        s: &Services,
    ) {
        match what {
            GridMenu::CopyCell => {
                let sh = self.shared.lock();
                if let Some(v) = sh.results[i].rows.get(row).and_then(|x| x.get(col)) {
                    ui.ctx().copy_text(match v {
                        Value::Null => String::new(),
                        other => other.display(),
                    });
                }
            }
            GridMenu::CopyRow => {
                let sh = self.shared.lock();
                let r = &sh.results[i];
                if let Some(x) = r.rows.get(row) {
                    let text = export::render(Format::Tsv, &r.columns, &[x], self.engine, "");
                    // Without the header line.
                    ui.ctx()
                        .copy_text(text.split_once('\n').map(|x| x.1).unwrap_or("").to_string());
                }
            }
            GridMenu::ViewCell => self.open_cell(i, row, col, s),
            GridMenu::FilterByValue => {
                let v = self.shared.lock().results[i]
                    .rows
                    .get(row)
                    .and_then(|x| x.get(col))
                    .map(|v| {
                        if v.is_null() {
                            "null".into()
                        } else {
                            v.display()
                        }
                    });
                if let Some(v) = v
                    && let Some(g) = self.grids.get_mut(i)
                {
                    g.filter = v;
                }
            }
        }
    }

    fn messages(&self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let sh = self.shared.lock();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (i, r) in sh.results.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}.", i + 1)).color(theme.text_muted),
                        );
                        ui.label(egui::RichText::new(short_sql(&r.sql)).monospace());
                    });
                    let line = if let Some(e) = &r.error {
                        egui::RichText::new(e.to_string()).color(theme.error)
                    } else if r.skipped {
                        egui::RichText::new(s.t("db-skipped")).color(theme.text_muted)
                    } else if let Some(sm) = &r.summary {
                        let mut t = if sm.has_result {
                            s.i18n.t_args(
                                "db-tab-rows",
                                &[
                                    ("rows", &r.rows.len().to_string()),
                                    ("ms", &sm.elapsed.as_millis().to_string()),
                                ],
                            )
                        } else {
                            s.i18n.t_args(
                                "db-affected",
                                &[
                                    ("rows", &sm.rows_affected.unwrap_or(0).to_string()),
                                    ("ms", &sm.elapsed.as_millis().to_string()),
                                ],
                            )
                        };
                        if let Some(m) = &sm.message {
                            t.push_str(&format!("\n{m}"));
                        }
                        egui::RichText::new(t).color(theme.text_muted)
                    } else {
                        egui::RichText::new("…").color(theme.text_muted)
                    };
                    ui.label(line);
                    ui.add_space(6.0);
                }
            });
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        let Some(dialog) = &mut self.dialog else {
            return;
        };
        let mut close = false;
        type Then = Box<dyn FnOnce(&mut DbTool, &Services)>;
        let mut then: Option<Then> = None;
        let conn = self.conn_name.clone();
        let env = if self.env_name.is_empty() {
            "—".to_string()
        } else {
            self.env_name.clone()
        };
        let protected = self.protected;
        let modal = egui::Modal::new(egui::Id::new(("db-dialog", self.id))).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            match dialog {
                Dialog::Confirm {
                    stmts,
                    dangerous,
                    need,
                    typed,
                    limit,
                } => {
                    ui.heading(s.t("db-confirm-title"));
                    ui.add_space(4.0);
                    ui.label(
                        s.i18n
                            .t_args("db-confirm-body", &[("conn", &conn), ("env", &env)]),
                    );
                    if protected {
                        ui.colored_label(
                            theme.env_prod,
                            format!(
                                "{} {}",
                                ph::WARNING,
                                s.i18n.t_args("db-confirm-protected", &[("env", &env)])
                            ),
                        );
                    }
                    ui.add_space(6.0);
                    for (sql, class, _) in dangerous.iter() {
                        ui.horizontal(|ui| {
                            ui.colored_label(theme.warning, s.t(class_key(*class)));
                            ui.label(egui::RichText::new(sql.as_str()).monospace());
                        });
                    }
                    if let Some(name) = need.as_ref() {
                        ui.add_space(6.0);
                        ui.label(s.i18n.t_args("db-type-name", &[("name", name)]));
                        ui.add(
                            egui::TextEdit::singleline(typed)
                                .hint_text(name.as_str())
                                .desired_width(f32::INFINITY),
                        );
                    }
                    ui.add_space(10.0);
                    let allowed = need.as_ref().is_none_or(|n| typed.trim() == n);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                allowed,
                                egui::Button::new(
                                    egui::RichText::new(s.t("db-run-anyway"))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(theme.error),
                            )
                            .clicked()
                        {
                            let stmts = std::mem::take(stmts);
                            let limit = *limit;
                            then = Some(Box::new(move |t, s| t.start_run(stmts, limit, s)));
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::AllowWrites => {
                    ui.heading(s.t("db-allow-writes-title"));
                    ui.add_space(4.0);
                    ui.label(
                        s.i18n
                            .t_args("db-allow-writes-body", &[("conn", &conn), ("env", &env)]),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if form::danger_button(ui, theme, &s.t("db-allow-writes")).clicked() {
                            then = Some(Box::new(|t, s| {
                                t.writes_unlocked = true;
                                t.connect(s);
                            }));
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Cell {
                    title,
                    text,
                    bytes,
                    sniffed,
                    image_size,
                } => {
                    ui.heading(title.as_str());
                    ui.add_space(4.0);
                    if let (Some(b), Some(sn)) = (bytes.as_ref(), sniffed.as_ref()) {
                        let size = match image_size {
                            Some((w, h)) => format!(" · {w} × {h}"),
                            None => String::new(),
                        };
                        ui.label(
                            egui::RichText::new(format!(
                                "{} · {}{size}",
                                sn.label,
                                human_bytes(b.len())
                            ))
                            .color(theme.text_muted),
                        );
                        ui.add_space(4.0);
                    }
                    let picture = matches!(
                        sniffed.as_ref().map(|sn| sn.kind),
                        Some(hs_core::sniff::Kind::Image | hs_core::sniff::Kind::Svg)
                    );
                    egui::ScrollArea::vertical()
                        .max_height(420.0)
                        .show(ui, |ui| {
                            if picture && let Some(b) = bytes.as_ref() {
                                let ext = sniffed.as_ref().map(|sn| sn.ext).unwrap_or("bin");
                                let digest = {
                                    use std::hash::{Hash, Hasher};
                                    let mut h = std::collections::hash_map::DefaultHasher::new();
                                    b.hash(&mut h);
                                    h.finish()
                                };
                                let uri = format!("bytes://db-cell-{digest:x}.{ext}");
                                ui.add(
                                    egui::Image::from_bytes(uri, b.clone())
                                        .max_size(egui::vec2(620.0, 400.0))
                                        .corner_radius(4.0),
                                );
                                ui.add_space(6.0);
                                egui::CollapsingHeader::new(s.t("db-cell-hex"))
                                    .default_open(false)
                                    .show(ui, |ui| {
                                        let mut shown = text.clone();
                                        ui.add(
                                            egui::TextEdit::multiline(&mut shown)
                                                .font(egui::TextStyle::Monospace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(8),
                                        );
                                    });
                            } else {
                                let mut shown = text.clone();
                                ui.add(
                                    egui::TextEdit::multiline(&mut shown)
                                        .font(egui::TextStyle::Monospace)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(12),
                                );
                            }
                        });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(format!("{} {}", ph::COPY, s.t("db-copy")))
                            .clicked()
                        {
                            ui.ctx().copy_text(text.clone());
                        }
                        if let (Some(b), Some(sn)) = (bytes.as_ref(), sniffed.as_ref())
                            && ui
                                .button(format!("{} {}", ph::FLOPPY_DISK, s.t("db-cell-save")))
                                .tip(s.t("db-cell-save-tip"))
                                .clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!("cell.{}", sn.ext))
                                .add_filter(sn.label, &[sn.ext])
                                .save_file()
                        {
                            match std::fs::write(&path, b) {
                                Ok(()) => cx.toast(
                                    ToastLevel::Success,
                                    s.i18n.t_args(
                                        "db-cell-saved",
                                        &[("path", &path.display().to_string())],
                                    ),
                                ),
                                Err(e) => cx.toast(ToastLevel::Error, e.to_string()),
                            }
                        }
                        if ui.button(s.t("close")).clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::SaveQuery { name } => {
                    ui.heading(s.t("db-save-query"));
                    ui.add_space(6.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(name)
                            .hint_text(s.t("db-query-name"))
                            .desired_width(f32::INFINITY),
                    );
                    if name.is_empty() && !r.has_focus() {
                        r.request_focus();
                    }
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if (form::primary_button(ui, theme, &s.t("save")).clicked() || enter)
                            && !name.trim().is_empty()
                        {
                            let n = name.trim().to_string();
                            then = Some(Box::new(move |t, s| {
                                if let Err(e) = t.save_query(s, Some(n)) {
                                    t.shared.lock().messages.push((false, e));
                                }
                            }));
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                }
            }
        });
        if modal.should_close() {
            close = true;
        }
        if let Some(f) = then {
            self.dialog = None;
            f(self, s);
        } else if close {
            self.dialog = None;
        }
    }

    // ---- sidebar pieces -------------------------------------------------------

    fn schema_nodes(&self, s: &Services) -> Vec<TreeNode> {
        let sh = self.shared.lock();
        let filter = self.tree_filter.to_lowercase();
        let loaded = |id: &str| -> Option<Vec<driver::SchemaObject>> {
            match sh.nodes.get(id) {
                Some(Load::Ready(v)) => Some(v.clone()),
                _ => None,
            }
        };
        let failed_or_loading = |id: &str| -> Option<TreeNode> {
            match sh.nodes.get(id) {
                Some(Load::Failed(e)) => Some(TreeNode::leaf(
                    format!("{id}{SEP}err"),
                    e.clone(),
                    ph::WARNING,
                )),
                Some(Load::Loading) => Some(TreeNode::leaf(
                    format!("{id}{SEP}loading"),
                    s.t("db-loading"),
                    ph::SPINNER_GAP,
                )),
                _ => None,
            }
        };
        let Some(schemas) = loaded("root") else {
            return failed_or_loading("root").into_iter().collect();
        };
        schemas
            .iter()
            .map(|sc| {
                let sid = node_id(&["s", &sc.name]);
                let groups = ObjectKind::groups(self.engine)
                    .iter()
                    .map(|&k| {
                        let gid = node_id(&["g", &sc.name, kind_id(k)]);
                        let children = loaded(&gid).map(|objs| {
                            objs.iter()
                                .filter(|o| {
                                    filter.is_empty() || o.name.to_lowercase().contains(&filter)
                                })
                                .map(|o| {
                                    let oid = node_id(&["o", &sc.name, kind_id(k), &o.name]);
                                    let icon = match k {
                                        ObjectKind::Table => ph::TABLE,
                                        ObjectKind::View => ph::EYE,
                                        ObjectKind::Function => ph::FUNCTION,
                                        _ => ph::LIST_NUMBERS,
                                    };
                                    if matches!(k, ObjectKind::Table | ObjectKind::View) {
                                        let mut kids: Vec<TreeNode> = loaded(&oid)
                                            .unwrap_or_default()
                                            .iter()
                                            .map(|c| {
                                                let pk = c.badges.iter().any(|b| b == "PK");
                                                let fk =
                                                    c.badges.iter().any(|b| b.starts_with("FK"));
                                                let icon = if pk {
                                                    ph::KEY
                                                } else if fk {
                                                    ph::LINK
                                                } else {
                                                    ph::COLUMNS
                                                };
                                                let mut n = TreeNode::leaf(
                                                    node_id(&["c", &sc.name, &o.name, &c.name]),
                                                    c.name.clone(),
                                                    icon,
                                                );
                                                n.badge = Some(c.detail.clone());
                                                n
                                            })
                                            .collect();
                                        if let Some(n) = failed_or_loading(&oid) {
                                            kids.push(n);
                                        }
                                        if k == ObjectKind::Table && loaded(&oid).is_some() {
                                            let xid = node_id(&["x", &sc.name, &o.name]);
                                            let mut x = TreeNode::branch(
                                                xid.clone(),
                                                s.t("db-kind-indexes"),
                                                ph::LIGHTNING,
                                                Vec::new(),
                                            );
                                            x.children = loaded(&xid).map(|ix| {
                                                ix.iter()
                                                    .map(|i| {
                                                        let mut n = TreeNode::leaf(
                                                            node_id(&[
                                                                "i", &sc.name, &o.name, &i.name,
                                                            ]),
                                                            i.name.clone(),
                                                            if i.badges.is_empty() {
                                                                ph::LIGHTNING
                                                            } else {
                                                                ph::KEY
                                                            },
                                                        );
                                                        n.badge = Some(i.detail.clone());
                                                        n
                                                    })
                                                    .collect()
                                            });
                                            kids.push(x);
                                        }
                                        let mut n = TreeNode::branch(
                                            oid.clone(),
                                            o.name.clone(),
                                            icon,
                                            kids,
                                        );
                                        if loaded(&oid).is_none()
                                            && failed_or_loading(&oid).is_none()
                                        {
                                            n.children = None;
                                        }
                                        n.badge = Some(o.detail.clone()).filter(|d| !d.is_empty());
                                        n
                                    } else {
                                        let mut n = TreeNode::leaf(oid, o.name.clone(), icon);
                                        n.badge = Some(o.detail.clone()).filter(|d| !d.is_empty());
                                        n
                                    }
                                })
                                .collect::<Vec<_>>()
                        });
                        let count = loaded(&gid).map(|v| v.len());
                        let mut g = TreeNode::branch(
                            gid.clone(),
                            s.t(kind_key(k)),
                            ph::FOLDER_SIMPLE,
                            Vec::new(),
                        );
                        g.children = match (children, failed_or_loading(&gid)) {
                            (Some(c), _) => Some(c),
                            (None, Some(n)) => Some(vec![n]),
                            (None, None) => None,
                        };
                        g.badge = count.map(|c| c.to_string());
                        g
                    })
                    .collect();
                TreeNode::branch(sid, sc.name.clone(), ph::DATABASE, groups)
            })
            .collect()
    }

    fn load_node(&mut self, id: &str, s: &Services) {
        let parts: Vec<&str> = id.split(SEP).collect();
        let scope = match parts.as_slice() {
            ["g", schema, kind] => kind_from(kind).map(|k| Scope::Objects {
                schema: schema.to_string(),
                kind: k,
            }),
            ["o", schema, _, name] => Some(Scope::Columns {
                schema: schema.to_string(),
                table: name.to_string(),
            }),
            ["x", schema, name] => Some(Scope::Indexes {
                schema: schema.to_string(),
                table: name.to_string(),
            }),
            _ => None,
        };
        if let Some(scope) = scope {
            self.shared
                .lock()
                .nodes
                .insert(id.to_string(), Load::Loading);
            self.send(
                Cmd::Introspect {
                    node: id.to_string(),
                    scope,
                },
                s,
            );
        }
    }

    fn refresh_schema(&mut self, s: &Services) {
        let expanded: Vec<String> = self.tree.expanded.iter().cloned().collect();
        self.shared.lock().nodes.clear();
        self.send(
            Cmd::Introspect {
                node: "root".into(),
                scope: Scope::Schemas,
            },
            s,
        );
        for id in expanded {
            self.load_node(&id, s);
        }
        self.catalog_cached = false;
        self.send(Cmd::Catalog, s);
    }

    fn tree_menu(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let Some((id, pos)) = self.menu.clone() else {
            return;
        };
        let s = cx.services;
        let parts: Vec<String> = id.split(SEP).map(str::to_string).collect();
        let mut close = false;
        let mut run: Option<String> = None;
        let mut insert: Option<String> = None;
        let mut copy: Option<String> = None;
        let mut refresh = false;
        let area = egui::Area::new(self.id.with("tree-menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    let item = |ui: &mut egui::Ui, label: String| {
                        ui.add(
                            egui::Button::new(label)
                                .frame(false)
                                .min_size(egui::vec2(ui.available_width(), 0.0)),
                        )
                        .clicked()
                    };
                    match parts
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>()
                        .as_slice()
                    {
                        ["o", schema, kind, name] => {
                            let q = self.qualified(schema, name);
                            let table_or_view = *kind == "table" || *kind == "view";
                            if table_or_view && item(ui, s.t("db-open-data")) {
                                run = Some(format!("SELECT * FROM {q}"));
                            }
                            if table_or_view && item(ui, s.t("db-select-template")) {
                                insert = Some(format!("SELECT *\nFROM {q}\nWHERE ;"));
                            }
                            if *kind == "table" && item(ui, s.t("db-count-rows")) {
                                run = Some(format!("SELECT COUNT(*) FROM {q}"));
                            }
                            if item(ui, s.t("db-copy-name")) {
                                copy = Some(q.clone());
                            }
                            if *kind == "table" {
                                ui.separator();
                                if item(ui, s.t("db-truncate")) {
                                    insert = Some(format!("TRUNCATE TABLE {q};"));
                                }
                                if item(ui, s.t("db-drop")) {
                                    insert = Some(format!("DROP TABLE {q};"));
                                }
                            } else if *kind == "view" {
                                ui.separator();
                                if item(ui, s.t("db-drop")) {
                                    insert = Some(format!("DROP VIEW {q};"));
                                }
                            }
                        }
                        ["c", _, _, name] | ["i", _, _, name] => {
                            if item(ui, s.t("db-copy-name")) {
                                copy = Some(sql::quote_ident(name, self.engine));
                            }
                        }
                        _ => {
                            if item(ui, s.t("db-refresh-schema")) {
                                refresh = true;
                            }
                        }
                    }
                    if item(ui, s.t("close")) {
                        close = true;
                    }
                });
            });
        if area.response.clicked_elsewhere() {
            close = true;
        }
        if let Some(sql) = run {
            self.run_in_new_tab(&sql, s);
            close = true;
        }
        if let Some(t) = insert {
            // Destructive templates go into the editor; running them goes
            // through the guard like anything else.
            self.insert_text(&t);
            close = true;
        }
        if let Some(t) = copy {
            ui.ctx().copy_text(t);
            close = true;
        }
        if refresh {
            self.refresh_schema(s);
            close = true;
        }
        if close {
            self.menu = None;
        }
    }
}

impl Tool for DbTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    /// What this tab is: the engine, and the query being edited.
    fn subtitle(&self) -> Option<String> {
        let engine = self.engine.label().to_string();
        match self.query_name.as_deref() {
            Some(q) if !q.is_empty() => Some(format!("{engine} · {q}")),
            _ => Some(engine),
        }
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        let file = self
            .state
            .query_file
            .as_ref()
            .and_then(|_| self.query_name.clone());
        match (file, self.conn_name.is_empty()) {
            (Some(f), false) => Cow::Owned(format!("{f} — {}", self.conn_name)),
            (Some(f), true) => Cow::Owned(f),
            (None, false) => Cow::Owned(self.conn_name.clone()),
            (None, true) => Cow::Borrowed("Database"),
        }
    }

    /// The engine's own mark, so a strip of database tabs says which
    /// server each one is.
    fn tab_icon(&self) -> Option<&'static str> {
        if self.conn_name.is_empty() {
            return None;
        }
        Some(hs_ui::icons::connection_icon("db", self.engine.id()))
    }

    fn dirty(&self) -> bool {
        self.shared.lock().in_tx || (self.state.query_file.is_some() && self.doc().buf.dirty())
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let ready = self.ready();
        let (running, in_tx) = {
            let sh = self.shared.lock();
            (sh.running, sh.in_tx)
        };
        let km = s.shortcuts.read();
        let sc = |a: &str| km.get(a).map(|c| c.display()).unwrap_or_default();
        let btn = |ui: &mut egui::Ui, enabled: bool, icon: &str, key: &str, tip: String| {
            ui.add_enabled(enabled, egui::Button::new(format!("{icon} {}", s.t(key))))
                .tip(tip)
                .clicked()
        };
        if btn(
            ui,
            ready && !running,
            ph::PLAY,
            "db-run",
            s.i18n.t_args("db-run-tip", &[("key", &sc("run"))]),
        ) {
            self.run(RunMode::Current, s);
        }
        if btn(
            ui,
            ready && !running,
            ph::FAST_FORWARD,
            "db-run-script",
            s.i18n
                .t_args("db-run-script-tip", &[("key", &sc("db.run_script"))]),
        ) {
            self.run(RunMode::Script, s);
        }
        if btn(ui, running, ph::STOP, "db-cancel", s.t("db-cancel-tip")) {
            self.cancel();
        }
        if btn(
            ui,
            ready && !running,
            ph::TREE_STRUCTURE,
            "db-explain",
            s.i18n
                .t_args("db-explain-tip", &[("key", &sc("db.explain"))]),
        ) {
            self.run(RunMode::Explain, s);
        }
        if btn(
            ui,
            !running,
            ph::TEXT_ALIGN_LEFT,
            "db-prettify",
            s.t("db-prettify-tip"),
        ) {
            self.prettify();
        }
        drop(km);
        ui.separator();
        let tx_label = if self.state.manual_tx {
            s.t("db-tx-manual")
        } else {
            s.t("db-tx-auto")
        };
        egui::ComboBox::from_id_salt(self.id.with("tx"))
            .selected_text(tx_label)
            .width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.state.manual_tx, false, s.t("db-tx-auto"));
                ui.selectable_value(&mut self.state.manual_tx, true, s.t("db-tx-manual"));
            });
        if self.state.manual_tx || in_tx {
            if btn(
                ui,
                in_tx && !running,
                ph::CHECK,
                "db-commit",
                s.t("db-commit-tip"),
            ) {
                self.control("COMMIT", s);
            }
            if btn(
                ui,
                in_tx && !running,
                ph::ARROW_COUNTER_CLOCKWISE,
                "db-rollback",
                s.t("db-rollback-tip"),
            ) {
                self.control("ROLLBACK", s);
            }
            if in_tx {
                ui.colored_label(
                    theme.warning,
                    format!("{} {}", ph::HOURGLASS, s.t("db-tx-pending")),
                );
            }
        }
        ui.separator();
        let limit_text = |l: usize| {
            if l == 0 {
                s.t("db-no-limit")
            } else {
                s.i18n.t_args("db-limit-n", &[("n", &l.to_string())])
            }
        };
        egui::ComboBox::from_id_salt(self.id.with("limit"))
            .selected_text(limit_text(self.state.limit))
            .width(100.0)
            .show_ui(ui, |ui| {
                for &l in LIMITS {
                    ui.selectable_value(&mut self.state.limit, l, limit_text(l));
                }
            });
        let timeout_text = |t: u64| {
            if t == 0 {
                s.t("db-no-timeout")
            } else if t.is_multiple_of(60) {
                s.i18n
                    .t_args("db-timeout-min", &[("n", &(t / 60).to_string())])
            } else {
                s.i18n.t_args("db-timeout-sec", &[("n", &t.to_string())])
            }
        };
        egui::ComboBox::from_id_salt(self.id.with("timeout"))
            .selected_text(timeout_text(self.state.timeout_secs))
            .width(100.0)
            .show_ui(ui, |ui| {
                for &t in TIMEOUTS {
                    ui.selectable_value(&mut self.state.timeout_secs, t, timeout_text(t));
                }
            });
        ui.separator();
        if self.read_only_profile {
            ui.colored_label(
                theme.text_muted,
                format!("{} {}", ph::LOCK, s.t("db-read-only")),
            )
            .tip(s.t("db-read-only-tip"));
        } else if self.protected {
            if self.writes_unlocked {
                ui.colored_label(
                    theme.env_prod,
                    format!("{} {}", ph::LOCK_OPEN, s.t("db-writes-allowed")),
                );
            } else if ui
                .button(format!("{} {}", ph::LOCK, s.t("db-allow-writes")))
                .tip(
                    s.i18n
                        .t_args("db-allow-writes-tip", &[("env", &self.env_name)]),
                )
                .clicked()
            {
                self.dialog = Some(Dialog::AllowWrites);
            }
        }
        if btn(
            ui,
            true,
            ph::FLOPPY_DISK,
            "db-save-query",
            s.t("db-save-query-tip"),
        ) {
            self.run_command("file.save", cx);
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("SQL");
            }
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let mut actions: Vec<(&str, String)> = vec![(ph::PLUS, s.t("db-new-profile"))];
        if self.cmd.is_some() {
            actions.push((ph::ARROW_CLOCKWISE, s.t("db-refresh-schema")));
        }
        let actions: Vec<(&str, &str)> = actions.iter().map(|(i, t)| (*i, t.as_str())).collect();
        match form::sidebar_header(ui, &theme, &s.t("db-connections"), &actions) {
            Some(0) => cx.actions.push(Action::RunCommand {
                id: "settings:connections".into(),
            }),
            Some(1) => self.refresh_schema(s),
            _ => {}
        }
        // The engine's own mark, so PostgreSQL and MySQL are told apart
        // in the list as well as in the subtitle.
        let profiles: Vec<(String, String, String, bool, &'static str)> = s
            .connections
            .read()
            .of_kind("db")
            .map(|c| {
                let f = DbFields::from_table(&c.fields);
                let icon = hs_ui::icons::connection_icon("db", &f.engine);
                (c.id.clone(), c.name.clone(), f.label(), c.pinned, icon)
            })
            .collect();
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("db-no-profiles"));
            return true;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("db-profiles");
                let mut open_profile: Option<(String, bool)> = None;
                let mut pin: Option<(String, bool)> = None;
                // Edit This Connection takes the user to the profile's own
                // form in Settings, selected and ready: one place manages a
                // connection, and the sidebar is where they are looking.
                let mut edit: Option<String> = None;
                for (i, (id, name, label, is_pinned, icon)) in profiles.iter().enumerate() {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            selected,
                            *icon,
                            name,
                            Some(label),
                            *is_pinned,
                            form::RowMenuLabels {
                                pin: &s.t("conn-pin-home"),
                                unpin: &s.t("conn-unpin-home"),
                                edit: &s.t("conn-edit"),
                            },
                        );
                        if row.response.tip(s.t("db-open-profile")).clicked() {
                            open_profile = Some((id.clone(), selected));
                        }
                        if let Some(p) = row.pin {
                            pin = Some((id.clone(), p));
                        }
                        if row.edit {
                            edit = Some(id.clone());
                        }
                    });
                    ui.add_space(2.0);
                    if !selected {
                        continue;
                    }
                    // The schema itself is a column of its own beside the
                    // tab's content, not a block nested under this row: it
                    // is what the connection is *for*, and it has to stay
                    // when the sidebar is hidden.
                }
                if let Some((id, p)) = pin
                    && let Err(e) = s.connections.write().set_pinned(&id, p)
                {
                    cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                }
                if let Some(id) = edit {
                    cx.actions.push(hs_core::tool::Action::RunCommand {
                        id: format!("settings:connection:{id}"),
                    });
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    if let Err(e) = s.connections.write().reorder(&["db"], from, to) {
                        cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
                // The shell decides which tab this lands in: the one
                // already bound to the profile, this one if it holds no
                // connection, or a new one.
                if let Some((id, selected)) = open_profile
                    && !selected
                {
                    cx.open("db", Some(OpenRequest::Connection { id }));
                }
            });
        self.tree_menu(ui, cx);
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // Keep the editor and the grid clear of the pane's focus border.
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(4, 2))
            .show(ui, |ui| self.body(ui, cx));
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        let mut out = Vec::new();
        if let Some(Dialog::Cell { title, text, .. }) = &self.dialog {
            out.push(hs_core::tool::Speakable::new(
                format!("Cell: {title}"),
                text.clone(),
            ));
        }
        let sel = hs_editor::view::selected_text(&self.doc().buf, &self.doc().cursors);
        if !sel.trim().is_empty() {
            out.push(hs_core::tool::Speakable::new("Selected SQL", sel));
        }
        let query = self.doc().buf.text();
        if !query.trim().is_empty() {
            out.push(hs_core::tool::Speakable::new("Query", query));
        }
        if let Some(a) = self.assistant.last_answer() {
            out.push(hs_core::tool::Speakable::new("Assistant's last answer", a));
        }
        out
    }

    fn printable(&self) -> Option<hs_core::tool::Printable> {
        let sh = self.shared.lock();
        let mut html = String::new();
        let query = self.doc().buf.text();
        if !query.trim().is_empty() {
            html.push_str("<h2>Query</h2>");
            html.push_str(&hs_core::html::pre(&query));
        }
        for r in &sh.results {
            if r.columns.is_empty() && r.error.is_none() {
                continue;
            }
            html.push_str("<h2>Result</h2>");
            html.push_str(&hs_core::html::pre(&r.sql));
            if let Some(e) = &r.error {
                html.push_str(&format!(
                    "<p style=\"color:#c62a2a\">{}</p>",
                    hs_core::html::escape(&e.to_string())
                ));
            }
            if !r.columns.is_empty() {
                let headers: Vec<String> = r.columns.iter().map(|c| c.name.clone()).collect();
                let rows: Vec<Vec<String>> = r
                    .rows
                    .iter()
                    .take(5000)
                    .map(|row| row.iter().map(|v| v.display()).collect())
                    .collect();
                html.push_str(&hs_core::html::table(&headers, &rows));
                if r.rows.len() > 5000 {
                    html.push_str("<p>… first 5000 rows</p>");
                }
            }
        }
        if html.is_empty() {
            return None;
        }
        Some(hs_core::tool::Printable {
            title: format!("SQL · {}", self.conn_name),
            subtitle: self.engine.label().to_string(),
            html,
        })
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        self.command_list()
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        self.command(id, cx)
    }

    fn save_state(&self) -> serde_json::Value {
        let mut st = self.state.clone();
        st.sql = self.doc().buf.text();
        serde_json::to_value(&st).unwrap_or_default()
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

    fn current_uri(&self) -> Option<String> {
        self.state
            .query_file
            .as_ref()
            .map(|f| format!("db-query://{f}"))
    }

    fn quick_open_items(&self) -> Vec<QuickOpenItem> {
        self.saved
            .iter()
            .map(|q| QuickOpenItem {
                id: q.path.to_string_lossy().to_string(),
                title: format!("# {}", q.name),
                subtitle: self.conn_name.clone(),
            })
            .collect()
    }

    fn on_close(&mut self) -> CloseDecision {
        if self.dirty() {
            CloseDecision::AskUnsavedChanges
        } else {
            CloseDecision::Close
        }
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                if self.state.conn_id.as_deref() == Some(&id) {
                    if self.cmd.is_none() {
                        self.connect(cx.services);
                    }
                } else if self.state.conn_id.is_none() {
                    self.state.conn_id = Some(id);
                    self.connect(cx.services);
                } else {
                    cx.open("db", Some(OpenRequest::Connection { id }));
                }
                true
            }
            OpenRequest::File { uri, .. } if uri.ends_with(".sql") => {
                let path = PathBuf::from(uri.strip_prefix("file://").unwrap_or(&uri));
                if self.state.conn_id.is_none() && self.doc().buf.text().trim().is_empty()
                    || self.state.query_file.is_none() && self.doc().buf.text().trim().is_empty()
                {
                    self.open_saved(cx.services, &path)
                } else {
                    cx.open("db", Some(OpenRequest::File { uri, line: None }));
                    true
                }
            }
            _ => false,
        }
    }

    fn on_event(&mut self, ev: &Event, cx: &mut ToolContext) {
        match ev {
            Event::SettingsChanged => {
                let st = cx.services.settings.read();
                for d in &mut self.docs {
                    d.view.font_size = st.editor.font_size;
                    d.view.right_click_copy_paste = st.editor.right_click_copy_paste;
                }
            }
            Event::ConnectionsChanged
                if self.state.conn_id.is_some() && !self.shared.lock().running =>
            {
                // A changed profile (host, password, read-only) applies on
                // the next connect; reconnect now unless a transaction is open.
                if !self.shared.lock().in_tx {
                    self.connect(cx.services);
                }
            }
            Event::VaultUnlocked if self.waiting_for_vault => self.connect(cx.services),
            Event::EnvironmentSwitched { .. }
                if self.state.conn_id.is_some()
                    && self.env_id.is_none()
                    && !self.shared.lock().in_tx
                    && !self.shared.lock().running =>
            {
                // Unbound profile: the active environment decides the guards.
                self.writes_unlocked = false;
                self.connect(cx.services);
            }
            _ => {}
        }
    }
}

impl DbTool {
    /// The schema column: what server this is, a filter, and the tree of
    /// its objects. It lives beside the editor rather than inside the
    /// sidebar, so hiding the connections list does not take the schema
    /// with it — the sidebar is for choosing a server, this is for working
    /// in one. Owner's requirement, 2026-10-17.
    fn schema_column(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        match self.shared.lock().status.clone() {
            Some(Status::Connecting) => {
                ui.label(egui::RichText::new(s.t("db-connecting")).weak());
            }
            Some(Status::Ready(info)) => {
                ui.label(
                    egui::RichText::new(format!("{} · {}", info.version, info.database))
                        .small()
                        .color(theme.text_muted),
                );
            }
            Some(Status::Failed(e)) => {
                ui.colored_label(theme.error, e);
            }
            _ => {}
        }
        ui.add_space(4.0);
        ui.add(
            egui::TextEdit::singleline(&mut self.tree_filter)
                .hint_text(s.t("db-filter-objects"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(2.0);
        // The tree takes the upper half; saved queries and history share
        // the rest, so neither pushes the other off the column.
        let tree_h = (ui.available_height() * 0.55).max(120.0);
        egui::ScrollArea::vertical()
            .id_salt("db-schema-column")
            .max_height(tree_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let nodes = self.schema_nodes(s);
                let resp = tree::show(ui, theme, &mut self.tree, &nodes);
                for id in resp.load {
                    self.load_node(&id, s);
                }
                if let Some(id) = resp.double_clicked {
                    let parts: Vec<&str> = id.split(SEP).collect();
                    if let ["o", schema, kind, name] = parts.as_slice()
                        && (*kind == "table" || *kind == "view")
                    {
                        let q = self.qualified(schema, name);
                        self.run_in_new_tab(&format!("SELECT * FROM {q}"), s);
                    }
                }
                if let Some(id) = resp.secondary
                    && let Some(pos) = ui.input(|i| i.pointer.interact_pos())
                {
                    self.menu = Some((id, pos));
                }
            });
        self.queries_section(ui, theme, cx);
    }

    /// Saved queries and history, under the schema in the same column.
    /// They belong to the connection, not to the list of connections, and
    /// a reader reaches for a saved query far more often than for a line
    /// of history — so Saved Queries goes first. Owner's requirement,
    /// 2026-10-17.
    fn queries_section(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("db-queries-section")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::CollapsingHeader::new(format!(
                    "{} {}",
                    ph::BOOKMARK_SIMPLE,
                    s.t("db-saved-queries")
                ))
                .id_salt("db-saved")
                .default_open(true)
                .show(ui, |ui| {
                    if self.saved.is_empty() {
                        ui.label(egui::RichText::new(s.t("db-saved-empty")).weak());
                    }
                    let mut open = None;
                    for q in &self.saved {
                        let current = self
                            .state
                            .query_file
                            .as_ref()
                            .is_some_and(|f| q.path.ends_with(f));
                        if ui
                            .selectable_label(current, format!("{} {}", ph::FILE_SQL, q.name))
                            .clicked()
                        {
                            open = Some(q.path.clone());
                        }
                    }
                    if let Some(p) = open {
                        // A saved query opens as another query tab on this
                        // connection, unless the one in front is empty and
                        // unsaved -- no reason to leave a blank tab behind.
                        let busy =
                            self.doc().file.is_some() || !self.doc().buf.text().trim().is_empty();
                        if busy {
                            self.new_query(s);
                        }
                        self.open_saved(s, &p);
                    }
                });

                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);
                egui::CollapsingHeader::new(format!(
                    "{} {}",
                    ph::CLOCK_COUNTER_CLOCKWISE,
                    s.t("db-history")
                ))
                .id_salt("db-history")
                .default_open(false)
                .show(ui, |ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut self.history_search)
                                .hint_text(s.t("db-history-search"))
                                .desired_width(f32::INFINITY),
                        )
                        .changed()
                    {
                        self.reload_history(s);
                    }
                    let rows = self.history_rows.lock().clone();
                    if rows.is_empty() {
                        ui.label(egui::RichText::new(s.t("db-history-empty")).weak());
                    }
                    for h in rows.iter().take(50) {
                        let icon = if h.status == "ok" {
                            egui::RichText::new(ph::CHECK).color(theme.success)
                        } else {
                            egui::RichText::new(ph::X).color(theme.error)
                        };
                        let when = h
                            .created_at
                            .get(..16)
                            .unwrap_or(&h.created_at)
                            .replace('T', " ");
                        let r = ui
                            .add(
                                egui::Button::new({
                                    let mut job = egui::text::LayoutJob::default();
                                    icon.append_to(
                                        &mut job,
                                        ui.style(),
                                        egui::FontSelection::Default,
                                        egui::Align::Center,
                                    );
                                    egui::RichText::new(format!(" {}", short_sql(&h.sql)))
                                        .append_to(
                                            &mut job,
                                            ui.style(),
                                            egui::FontSelection::Default,
                                            egui::Align::Center,
                                        );
                                    job
                                })
                                .frame(false)
                                .truncate(),
                            )
                            .tip(format!("{when} · {} ms\n{}", h.duration_ms, h.sql));
                        if r.clicked() {
                            let sql = if h.sql.trim_end().ends_with(';') {
                                h.sql.clone()
                            } else {
                                format!("{};", h.sql)
                            };
                            self.insert_text(&sql);
                        }
                    }
                });
            });
        ui.add_space(6.0);
    }

    /// The query tabs for this connection: a multi-row strip above the
    /// editor, one tab per query, named by the saved query it holds or
    /// Untitled. The shell's tab is one connection; the queries inside it
    /// are these. Owner's requirement, 2026-10-17.
    fn query_tabs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
        let s = cx.services;
        struct QueryTab {
            id: u64,
            title: String,
            dirty: bool,
        }
        impl hs_ui::tabs::TabItem for QueryTab {
            fn id(&self) -> u64 {
                self.id
            }
            fn title(&self) -> String {
                self.title.clone()
            }
            fn icon(&self) -> &str {
                ph::FILE_SQL
            }
            fn pinned(&self) -> bool {
                false
            }
            fn dirty(&self) -> bool {
                self.dirty
            }
        }
        let untitled = s.t("db-untitled");
        let tabs: Vec<QueryTab> = self
            .docs
            .iter()
            .map(|d| QueryTab {
                id: d.uid,
                title: d.label(&untitled),
                dirty: d.buf.dirty(),
            })
            .collect();
        let active = self.doc().uid;
        let labels = hs_ui::tabs::TabStripLabels {
            new_tab: s.t("db-new-query"),
            close: s.t("db-close-query"),
            close_others: s.t("editor-close-others"),
            close_left: s.t("tab-close-left"),
            close_right: s.t("tab-close-right"),
            pin: String::new(),
            unpin: String::new(),
            duplicate: String::new(),
            duplicate_blocked: String::new(),
            copy_path: String::new(),
        };
        let min_width = s.settings.read().appearance.min_file_tab_width;
        let r = ui
            .push_id(self.id.with("query-tabs"), |ui| {
                hs_ui::tabs::TabStrip {
                    theme,
                    min_width,
                    gap: 2.0,
                    active: Some(active),
                    labels: &labels,
                    compact: true,
                    show_new_tab: true,
                }
                .show(ui, &tabs)
            })
            .inner;
        if let Some(uid) = r.activated
            && let Some(i) = self.docs.iter().position(|d| d.uid == uid)
        {
            self.state.active_query = i;
        }
        if let Some((from, to)) = r.reorder
            && from < self.docs.len()
            && to <= self.docs.len()
        {
            let d = self.docs.remove(from);
            let to = to.min(self.docs.len());
            self.docs.insert(to, d);
            self.state.active_query = to;
        }
        if let Some(uid) = r.close
            && let Some(i) = self.docs.iter().position(|d| d.uid == uid)
        {
            // The last one is emptied rather than removed: a connection
            // tab with no query in it has nothing to show.
            if self.docs.len() == 1 {
                self.docs[0] = QueryDoc::new(s, "");
            } else {
                self.docs.remove(i);
            }
            self.state.active_query = self.state.active_query.min(self.docs.len() - 1);
        }
        if r.new_tab {
            self.new_query(s);
        }
        self.sync_query_state();
    }

    /// Open another query tab on this connection, and put it in front.
    fn new_query(&mut self, services: &Services) {
        self.docs.push(QueryDoc::new(services, ""));
        self.state.active_query = self.docs.len() - 1;
    }

    /// Run a statement the user asked for from the schema tree in a query
    /// tab of its own, so the text that produced the grid is on screen and
    /// can be edited and run again. It used to run from nowhere, leaving
    /// whatever was in the front tab looking like the cause of the result.
    /// An Untitled tab that is still empty is filled rather than added to,
    /// the way the API client fills an empty tab before opening another.
    /// Owner's requirement, 2026-10-17.
    fn run_in_new_tab(&mut self, sql: &str, s: &Services) {
        let sql = &terminated(sql);
        let reusable = reusable_tab(self.docs.iter().map(|d| (d.file.is_none(), d.buf.text())));
        match reusable {
            Some(i) => {
                // A whole new document, not a new buffer under the old
                // view: `EditorView` caches per-line spans and only drops
                // them when the buffer version moves, and a buffer built
                // with `from_text` starts its versions again — so the view
                // kept the empty line's layout and drew one character of
                // the statement. Verified live, 2026-10-17.
                let uid = self.docs[i].uid;
                let mut doc = QueryDoc::new(s, sql);
                doc.uid = uid;
                self.docs[i] = doc;
                self.state.active_query = i;
            }
            None => {
                self.docs.push(QueryDoc::new(s, sql));
                self.state.active_query = self.docs.len() - 1;
            }
        }
        self.sync_query_state();
        self.run_text(sql, usize::MAX, false, None, s);
    }

    /// Keep the single-query fields the rest of the tool still reads in
    /// step with whichever tab is in front.
    fn sync_query_state(&mut self) {
        self.state.query_file = self.doc().file.clone();
        self.query_name = self.doc().name.clone();
        self.state.queries = self.docs.iter().map(|d| d.file.clone()).collect();
    }

    /// Replace the query tab's text with a laid-out version of itself.
    /// The whole buffer, not the statement under the caret: a script is
    /// formatted as a script, and `sqlformat` keeps the statements apart.
    fn prettify(&mut self) {
        let indent = if self.doc().view.tab_inserts_spaces {
            2
        } else {
            4
        };
        let text = self.doc().buf.text();
        if text.trim().is_empty() {
            return;
        }
        let out = prettify_sql(&text, indent);
        if out.trim() == text.trim() {
            return;
        }
        let d = self.doc_mut();
        d.buf = Buffer::from_text(&out);
        d.cursors = Cursors::single(0);
        d.view.error_ranges.clear();
    }

    /// The home tab: how many connections this workspace has, how many
    /// are open, and a card for each with the last picture of its pane.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let profiles = s.connections.read().of_kind("db").count();
        let tabs = s.open_tabs.of("db");
        hs_ui::form::page_title(ui, &theme, &s.t("tool-db"), Some(&s.t("db-dashboard-sub")));
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(ui, &theme, &profiles.to_string(), &s.t("dash-connections"));
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        egui::ScrollArea::vertical()
            .id_salt("db-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

    fn body(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // The activity's home tab lists what is open rather than being one
        // of them. Owner's requirement, 2026-10-17.
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        // The assistant takes its column before anything else is laid out,
        // so the editor and the results share what is left instead of the
        // assistant being drawn over the top of them.
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        let outer = ui;
        let ui = &mut content;
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.waiting_for_vault && !s.secrets.is_locked() {
            self.connect(s);
        }
        self.drain(cx);
        if let Some(e) = self.conn_error.clone() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.colored_label(theme.error, e);
                if self.state.conn_id.is_some() && ui.button(s.t("db-retry")).clicked() {
                    self.connect(s);
                }
            });
            return;
        }
        if self.state.conn_id.is_none() {
            ui.vertical_centered(|ui| {
                ui.add_space(30.0);
                ui.label(egui::RichText::new(s.t("db-pick-profile")).weak());
            });
            return;
        }
        let status = self.shared.lock().status.clone();
        if let Some(Status::Failed(e)) = status {
            ui.add_space(6.0);
            ui.colored_label(theme.error, format!("{} {e}", ph::WARNING));
            if ui.button(s.t("db-retry")).clicked() {
                self.connect(s);
            }
            ui.add_space(6.0);
        }
        // Escape cancels a running statement (when no popup takes it).
        if self.popup.is_none()
            && self.shared.lock().running
            && ui.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.cancel();
        }
        // Ctrl+Space opens completion.
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Space)) {
            self.update_completion(true);
        }
        self.completion_keys(ui);

        // The schema column, beside the editor rather than inside the
        // sidebar: hiding the connections list is about choosing a server,
        // and should not take the schema of the one you are in with it.
        if self.cmd.is_some() {
            egui::Panel::left(self.id.with("db-schema"))
                .resizable(true)
                .default_size(260.0)
                .size_range(160.0..=460.0)
                .show(ui, |ui| self.schema_column(ui, &theme, cx));
        }
        self.query_tabs(ui, cx, &theme);
        let total_h = ui.available_height();
        let editor_h = (total_h * self.state.editor_frac).clamp(80.0, (total_h - 120.0).max(80.0));
        let ed_theme = EditorTheme::for_theme(&theme);
        let version_before = self.doc().buf.version();
        let head_before = self.doc().cursors.primary().head;
        let resp = ui
            .allocate_ui(egui::vec2(ui.available_width(), editor_h), |ui| {
                let d = self.doc_mut();
                d.view.show(ui, &mut d.buf, &mut d.cursors, &ed_theme)
            })
            .inner;
        if self.doc().buf.version() != version_before {
            self.doc_mut().view.error_ranges.clear();
            // Typing keeps an open list in step; it closes once the word
            // ends (space, `;`, operator) like it opened.
            self.update_completion(false);
        } else if self.doc().cursors.primary().head != head_before {
            self.popup = None;
        }
        if !resp.focused {
            // Clicking elsewhere closes the popup.
            if ui.input(|i| i.pointer.any_pressed()) {
                self.popup = None;
            }
        }
        // Splitter.
        let (rect, r) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), egui::Sense::drag());
        let r = r.on_hover_cursor(egui::CursorIcon::ResizeVertical);
        ui.painter().line_segment(
            [rect.left_center(), rect.right_center()],
            egui::Stroke::new(1.0, theme.border),
        );
        if r.dragged() && total_h > 0.0 {
            self.state.editor_frac = ((editor_h + r.drag_delta().y) / total_h).clamp(0.15, 0.85);
        }
        self.results_panel(ui, cx, &theme);
        self.completion_popup(ui, &theme);
        self.dialogs(ui, cx, &theme);
        let mut run_block: Option<String> = None;
        if self.assistant.open {
            let query = self.doc().buf.text();
            let last_error = self
                .shared
                .lock()
                .results
                .last()
                .and_then(|r| r.error.as_ref().map(ToString::to_string));
            let context = format!(
                "Connection: {}\nEngine: {:?}\nQuery:\n{}\n\nLast query error: {}",
                self.conn_name,
                self.engine,
                query,
                last_error.unwrap_or_else(|| "(none)".into())
            );
            self.assistant.set_context("SQL workspace", context);
            if self.assistant.quick_actions.is_empty() {
                self.assistant.quick_actions =
                    hs_ui::ai_assistant::QuickAction::for_activity(s, "db");
            }
            let Some(panel) = assistant_rect else {
                // Too narrow to split: the toggle stays on, and the panel
                // comes back when there is room for it.
                return;
            };
            let mut child = outer.new_child(
                egui::UiBuilder::new()
                    .max_rect(panel)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            child.set_clip_rect(panel.intersect(outer.clip_rect()));
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .show(&mut child, |ui| {
                    ui.set_min_width(370.0);
                    self.assistant.can_insert = true;
                    self.assistant.can_run = self.ready();
                    let out = self.assistant.ui(ui, s, &theme, "SQL");
                    if let Some(block) = out.use_block {
                        // Into the SQL editor at the cursor; Insert and Run
                        // then sends exactly that text through the same
                        // guards as the Run button (ADR-019).
                        let d = self.doc_mut();
                        d.view.insert_at_cursors(
                            &mut d.buf,
                            &mut d.cursors,
                            &block,
                            EditKind::Other,
                        );
                        self.doc_mut().view.scroll_to_cursor();
                        if out.run {
                            run_block = Some(block);
                        }
                    }
                });
        }
        if let Some(block) = run_block {
            // Exactly the inserted text, through the classify-and-confirm
            // path the Run button uses: a protected environment, a
            // dangerous statement or a read-only profile still stops it.
            self.run_text(&block, usize::MAX, false, None, s);
        }
    }

    fn command_list(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str, sc: Option<&str>| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: sc.map(str::to_string),
        };
        vec![
            c("db.ai", "AI: Toggle Assistant", None),
            c("run", "Database: Run Statement", Some("run")),
            c(
                "db.run_script",
                "Database: Run Script",
                Some("db.run_script"),
            ),
            c("db.explain", "Database: Explain", Some("db.explain")),
            c("db.cancel", "Database: Cancel", None),
            c("db.commit", "Database: Commit", Some("db.commit")),
            c("db.rollback", "Database: Rollback", Some("db.rollback")),
            c(
                "db.refresh_schema",
                "Database: Refresh Schema",
                Some("db.refresh_schema"),
            ),
            c("db.complete", "Database: Complete", None),
            c("file.save", "Database: Save Query", Some("file.save")),
            c("db.save_as", "Database: Save Query As", None),
            c("db.new_query", "Database: New Query Tab", None),
            c(
                "db.export",
                "Database: Export Result as CSV",
                Some("db.export"),
            ),
        ]
    }

    fn command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        match id {
            "db.ai" => self.assistant.toggle("SQL"),
            "run" => self.run(RunMode::Current, s),
            "db.run_script" | "run.alt" => self.run(RunMode::Script, s),
            "db.explain" => self.run(RunMode::Explain, s),
            "db.cancel" => self.cancel(),
            "db.commit" => self.control("COMMIT", s),
            "db.rollback" => self.control("ROLLBACK", s),
            "db.refresh_schema" => self.refresh_schema(s),
            "db.complete" => self.update_completion(true),
            "file.save" => {
                if self.state.query_file.is_some() {
                    match self.save_query(s, None) {
                        Ok(()) => cx.toast(ToastLevel::Success, s.t("db-saved")),
                        Err(e) => cx.toast(ToastLevel::Error, e),
                    }
                } else {
                    self.dialog = Some(Dialog::SaveQuery {
                        name: String::new(),
                    });
                }
            }
            "db.save_as" | "file.save_as" => {
                self.dialog = Some(Dialog::SaveQuery {
                    name: String::new(),
                })
            }
            "db.new_query" => {
                if let Some(id) = self.state.conn_id.clone() {
                    cx.open("db", Some(OpenRequest::Connection { id }));
                }
            }
            "db.export" => {
                let i = self.active_result;
                if i < self.shared.lock().results.len() {
                    self.export(i, Format::Csv, s);
                }
            }
            _ => return false,
        }
        true
    }
}

/// "1.2 KB" style size for the cell viewer.
fn human_bytes(n: usize) -> String {
    let n = n as f64;
    if n < 1024.0 {
        format!("{n} B")
    } else if n < 1024.0 * 1024.0 {
        format!("{:.1} KB", n / 1024.0)
    } else {
        format!("{:.1} MB", n / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tabs(v: &[(bool, &str)]) -> Vec<(bool, String)> {
        v.iter().map(|(u, t)| (*u, (*t).to_string())).collect()
    }

    #[test]
    fn open_data_fills_the_blank_tab_in_front_of_the_user() {
        let t = tabs(&[(false, "SELECT 1"), (true, "   \n ")]);
        assert_eq!(reusable_tab(t.into_iter()), Some(1));
    }

    #[test]
    fn a_tab_with_anything_typed_in_it_is_never_reused() {
        let t = tabs(&[(true, "SELECT half_written"), (false, "")]);
        // Index 1 is empty but saved: a saved query's tab is its own thing.
        assert_eq!(reusable_tab(t.into_iter()), None);
    }

    #[test]
    fn with_no_blank_tab_open_data_gets_one_of_its_own() {
        let t = tabs(&[(true, "SELECT a"), (true, "SELECT b")]);
        assert_eq!(reusable_tab(t.into_iter()), None);
    }

    #[test]
    fn a_generated_statement_is_terminated() {
        assert_eq!(terminated("SELECT * FROM t"), "SELECT * FROM t;");
        assert_eq!(
            terminated("SELECT COUNT(*) FROM t"),
            "SELECT COUNT(*) FROM t;"
        );
    }

    #[test]
    fn a_statement_that_already_ends_in_one_is_left_alone() {
        assert_eq!(terminated("SELECT 1;"), "SELECT 1;");
        // Trailing whitespace must not earn a second semicolon.
        assert_eq!(
            terminated(
                "SELECT 1;  
"
            ),
            "SELECT 1;"
        );
    }

    #[test]
    fn prettify_lays_a_one_line_statement_out() {
        let out = prettify_sql("select a, b from t where a = 1", 2);
        assert!(out.lines().count() > 1, "{out}");
        assert!(out.contains("select"), "keywords keep their case: {out}");
    }
}
