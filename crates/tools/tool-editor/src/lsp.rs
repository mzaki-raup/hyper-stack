//! Language features through a language server (docs/tools/editor.md §11):
//! completion as you type, hover, diagnostics with a Problems panel, go to
//! definition, references, rename and formatting. One `Lsp` per editor tab
//! owns the servers its project needs (one process per server id and root)
//! and the state of the popups. Nothing here blocks: replies arrive on
//! channels drained once per frame.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use hs_core::Services;
use hs_core::tool::ToastLevel;
use hs_editor::Buffer;
use hs_editor::cursor::Cursors;
use hs_editor::view::{EditorResponse, EditorView, Squiggle, SquiggleLevel};
use hs_lsp::client::{Client, Event, RpcError, State};
use hs_lsp::registry::{Override, Resolved};
use hs_lsp::text::{self, Encoding, Lines};
use hs_lsp::types::{
    CompletionItem, CompletionResponse, Diagnostic, Hover, Position, Range, Severity, TextEdit,
};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use serde_json::Value;

/// Edits are sent to the server this long after the last keystroke.
const SYNC_DEBOUNCE: Duration = Duration::from_millis(150);
/// The mouse must rest this long on a symbol before hover is asked.
const HOVER_DWELL: Duration = Duration::from_millis(550);
/// Rows shown in the completion list at once.
const COMPLETION_ROWS: usize = 10;
const MAX_ITEMS: usize = 300;

/// A buffer as the protocol conversions see it.
pub struct BufLines<'a>(pub &'a Buffer);

impl Lines for BufLines<'_> {
    fn line_count(&self) -> usize {
        self.0.len_lines()
    }
    fn line(&self, n: usize) -> String {
        self.0
            .line_text(n)
            .trim_end_matches(['\r', '\n'])
            .to_string()
    }
    fn line_start_char(&self, n: usize) -> usize {
        self.0.line_start(n)
    }
}

/// What the tool must do after a frame of LSP work.
#[derive(Debug, Clone, PartialEq)]
pub enum LspAction {
    /// Open a file at a 0-based line and server column.
    GoTo {
        path: PathBuf,
        line: u32,
        col: u32,
        enc: Encoding,
    },
    /// Apply server edits to a file (open or on disk).
    ApplyEdits {
        path: PathBuf,
        edits: Vec<TextEdit>,
        enc: Encoding,
    },
    Toast(ToastLevel, String),
}

/// What the status bar says about the active file's language server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LspStatus {
    pub text: String,
    pub tip: String,
    /// Starting, indexing: the status bar keeps repainting.
    pub busy: bool,
    /// The language has a known server that is not installed: how to get
    /// it. `None` when a server is running or none is known.
    pub install: Option<Install>,
}

/// How to install a server that is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Install {
    pub server: String,
    /// A command to run in a terminal; empty when there is none that
    /// works on every platform.
    pub command: String,
    /// The download or documentation page; empty when there is none.
    pub url: String,
}

/// A location for the References list.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Loc {
    path: PathBuf,
    line: u32,
    col: u32,
    enc: Encoding,
}

struct Server {
    client: Client,
    spec: Resolved,
    rx: mpsc::Receiver<Event>,
    ready: bool,
    error: Option<String>,
    /// Running `$/progress` tokens and their last message.
    progress: BTreeMap<String, String>,
}

struct DocSync {
    server: String,
    /// The `textDocument` version sent to the server.
    version: i64,
    /// The buffer version the server has.
    sent_version: u64,
    dirty_since: Option<Instant>,
    /// Cached squiggles: (diagnostics generation, buffer version, list).
    squiggles: (u64, u64, Vec<Squiggle>),
}

struct Completion {
    path: PathBuf,
    /// Where the word being completed starts.
    anchor: usize,
    seq: u64,
    loading: bool,
    is_incomplete: bool,
    items: Vec<CompletionItem>,
    /// Indices into `items` after filtering, in display order.
    filtered: Vec<usize>,
    selected: usize,
    /// First visible row of the list.
    scroll: usize,
    enc: Encoding,
}

struct HoverPopup {
    path: PathBuf,
    /// Char range the hover is about: leaving it closes the popup.
    range: (usize, usize),
    pos: egui::Pos2,
    text: String,
    /// Opened from the keyboard: stays until a key or click.
    sticky: bool,
}

struct HoverPending {
    char: usize,
    since: Instant,
    requested: bool,
}

enum Reply {
    Completion {
        seq: u64,
        result: Result<Value, RpcError>,
    },
    Hover {
        seq: u64,
        path: PathBuf,
        char: usize,
        pos: egui::Pos2,
        sticky: bool,
        result: Result<Value, RpcError>,
    },
    Definition {
        enc: Encoding,
        result: Result<Value, RpcError>,
    },
    References {
        enc: Encoding,
        result: Result<Value, RpcError>,
    },
    Formatting {
        path: PathBuf,
        enc: Encoding,
        result: Result<Value, RpcError>,
    },
    Rename {
        enc: Encoding,
        result: Result<Value, RpcError>,
    },
}

/// The bottom panel: diagnostics, or a list of locations.
enum PanelMode {
    Problems,
    Locations { title: String, items: Vec<Loc> },
}

struct RenamePrompt {
    path: PathBuf,
    pos: usize,
    text: String,
    focus: bool,
}

pub struct Lsp {
    enabled: bool,
    complete_on_type: bool,
    overrides: Vec<Override>,
    servers: HashMap<String, Server>,
    docs: HashMap<PathBuf, DocSync>,
    /// Files `open_doc` was asked about, server or not, so the tool asks
    /// once per file instead of once per frame.
    attempted: std::collections::HashSet<PathBuf>,
    diagnostics: HashMap<PathBuf, (Encoding, Vec<Diagnostic>)>,
    diag_gen: u64,
    replies: mpsc::Receiver<Reply>,
    reply_tx: mpsc::Sender<Reply>,
    seq: u64,
    completion: Option<Completion>,
    /// A row clicked in the list, accepted on the next frame.
    pending_accept: Option<usize>,
    hover: Option<HoverPopup>,
    /// The server's hover range, resolved against the buffer when drawn.
    hover_range_pending: Option<Range>,
    hover_pending: Option<HoverPending>,
    rename: Option<RenamePrompt>,
    pub panel_open: bool,
    panel: PanelMode,
    actions: Vec<LspAction>,
    wake: Arc<dyn Fn() + Send + Sync>,
    id: egui::Id,
}

impl Lsp {
    pub fn new(s: &Services, id: egui::Id) -> Self {
        let (reply_tx, replies) = mpsc::channel();
        let waker = s.runtime.waker();
        let mut me = Self {
            enabled: true,
            complete_on_type: true,
            overrides: Vec::new(),
            servers: HashMap::new(),
            docs: HashMap::new(),
            attempted: std::collections::HashSet::new(),
            diagnostics: HashMap::new(),
            diag_gen: 0,
            replies,
            reply_tx,
            seq: 0,
            completion: None,
            pending_accept: None,
            hover: None,
            hover_range_pending: None,
            hover_pending: None,
            rename: None,
            panel_open: false,
            panel: PanelMode::Problems,
            actions: Vec::new(),
            wake: Arc::new(move || waker.request_repaint()),
            id: id.with("lsp"),
        };
        me.read_settings(s);
        me
    }

    fn read_settings(&mut self, s: &Services) -> bool {
        let st = s.settings.read();
        let overrides: Vec<Override> = st
            .editor
            .language_servers
            .iter()
            .map(|l| Override {
                language: l.language.clone(),
                command: l.command.clone(),
                args: l.args.split_whitespace().map(str::to_string).collect(),
                enabled: l.enabled,
            })
            .collect();
        let changed = overrides != self.overrides || st.editor.lsp_enabled != self.enabled;
        self.enabled = st.editor.lsp_enabled;
        self.complete_on_type = st.editor.completion_on_type;
        self.overrides = overrides;
        changed
    }

    /// Settings changed: stop every server when the table changed, so the
    /// tool reopens its documents against the new commands. Returns true
    /// when that reopen is needed.
    pub fn reconfigure(&mut self, s: &Services) -> bool {
        if !self.read_settings(s) {
            return false;
        }
        self.shutdown_all();
        true
    }

    pub fn shutdown_all(&mut self) {
        for (_, srv) in self.servers.drain() {
            srv.client.shutdown();
        }
        self.docs.clear();
        self.attempted.clear();
        self.diagnostics.clear();
        self.completion = None;
        self.hover = None;
    }

    /// Whether `open_doc` has seen this file (even without a server).
    pub fn knows(&self, path: &Path) -> bool {
        self.attempted.contains(path)
    }

    /// Close every file that is no longer open in the tool.
    pub fn retain_open(&mut self, open: &[PathBuf]) {
        let gone: Vec<PathBuf> = self
            .attempted
            .iter()
            .filter(|p| !open.iter().any(|o| o == *p))
            .cloned()
            .collect();
        for p in gone {
            self.close_doc(&p);
        }
    }

    // ---- documents -------------------------------------------------------

    /// A file was opened: start its server when needed and tell it.
    pub fn open_doc(&mut self, path: &Path, language: &str, text: &str, root: Option<&Path>) {
        if !self.enabled || self.docs.contains_key(path) {
            return;
        }
        self.attempted.insert(path.to_path_buf());
        let Some(spec) = hs_lsp::registry::resolve(language, &self.overrides) else {
            return;
        };
        let Some(exe) = spec.path.clone() else {
            // Known but not installed: the status line says how to get it.
            return;
        };
        let root = root
            .map(Path::to_path_buf)
            .or_else(|| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let key = format!("{}@{}", spec.id, root.display());
        if !self.servers.contains_key(&key) {
            let (tx, rx) = mpsc::channel();
            match Client::spawn(&spec.id, &exe, &spec.args, &root, tx, self.wake.clone()) {
                Ok(client) => {
                    tracing::info!("started {} for {}", spec.id, root.display());
                    self.servers.insert(
                        key.clone(),
                        Server {
                            client,
                            spec: spec.clone(),
                            rx,
                            ready: false,
                            error: None,
                            progress: BTreeMap::new(),
                        },
                    );
                }
                Err(e) => {
                    tracing::warn!("could not start {}: {e}", spec.id);
                    self.actions.push(LspAction::Toast(
                        ToastLevel::Warning,
                        format!("{}: {e}", spec.id),
                    ));
                    return;
                }
            }
        }
        self.servers[&key].client.did_open(
            &hs_lsp::uri::from_path(path),
            &spec.language_id,
            1,
            text,
        );
        self.docs.insert(
            path.to_path_buf(),
            DocSync {
                server: key,
                version: 1,
                sent_version: u64::MAX,
                dirty_since: None,
                squiggles: (u64::MAX, u64::MAX, Vec::new()),
            },
        );
    }

    pub fn close_doc(&mut self, path: &Path) {
        self.attempted.remove(path);
        if let Some(d) = self.docs.remove(path)
            && let Some(srv) = self.servers.get(&d.server)
        {
            srv.client.did_close(&hs_lsp::uri::from_path(path));
        }
        self.diagnostics.remove(path);
        if self.completion.as_ref().is_some_and(|c| c.path == path) {
            self.completion = None;
        }
    }

    /// Keep the server's copy of a document current (debounced).
    pub fn sync(&mut self, path: &Path, buf: &Buffer) {
        let Some(d) = self.docs.get_mut(path) else {
            return;
        };
        if d.sent_version == u64::MAX {
            // Just opened: the server has this text.
            d.sent_version = buf.version();
            return;
        }
        if buf.version() == d.sent_version {
            d.dirty_since = None;
            return;
        }
        let since = *d.dirty_since.get_or_insert_with(Instant::now);
        if since.elapsed() < SYNC_DEBOUNCE {
            return;
        }
        self.flush(path, buf);
    }

    /// Send the text now (before a request that depends on it).
    fn flush(&mut self, path: &Path, buf: &Buffer) {
        let Some(d) = self.docs.get_mut(path) else {
            return;
        };
        if d.sent_version == u64::MAX {
            d.sent_version = buf.version();
            return;
        }
        if buf.version() == d.sent_version {
            return;
        }
        d.version += 1;
        d.sent_version = buf.version();
        d.dirty_since = None;
        if let Some(srv) = self.servers.get(&d.server) {
            srv.client
                .did_change_full(&hs_lsp::uri::from_path(path), d.version, &buf.text());
        }
    }

    pub fn saved(&mut self, path: &Path, buf: &Buffer) {
        self.flush(path, buf);
        if let Some(d) = self.docs.get(path)
            && let Some(srv) = self.servers.get(&d.server)
        {
            srv.client
                .did_save(&hs_lsp::uri::from_path(path), Some(&buf.text()));
        }
    }

    /// Whether any server will answer for this file.
    pub fn has_server(&self, path: &Path) -> bool {
        self.docs.contains_key(path)
    }

    /// The ready client for a file.
    fn client_for(&self, path: &Path) -> Option<Client> {
        self.docs
            .get(path)
            .and_then(|d| self.servers.get(&d.server))
            .filter(|s| s.ready && s.client.state() == State::Ready)
            .map(|s| s.client.clone())
    }

    fn position(buf: &Buffer, at: usize, enc: Encoding) -> Position {
        let (l, c) = buf.char_to_line_col(at);
        text::position_from_line_col(&BufLines(buf).line(l), l, c, enc)
    }

    // ---- per-frame work --------------------------------------------------

    /// Drain server events and request replies. Call once per frame
    /// before drawing.
    pub fn poll(&mut self, s: &Services) {
        let mut events: Vec<(String, Event)> = Vec::new();
        for (key, srv) in &self.servers {
            while let Ok(ev) = srv.rx.try_recv() {
                events.push((key.clone(), ev));
            }
        }
        for (key, ev) in events {
            self.handle_event(s, &key, ev);
        }
        while let Ok(r) = self.replies.try_recv() {
            self.handle_reply(s, r);
        }
    }

    fn handle_event(&mut self, s: &Services, key: &str, ev: Event) {
        let Some(srv) = self.servers.get_mut(key) else {
            return;
        };
        match ev {
            Event::Ready { name, version } => {
                srv.ready = true;
                tracing::info!("{} ready ({name} {version})", srv.spec.id);
            }
            Event::Diagnostics {
                uri, diagnostics, ..
            } => {
                if let Some(path) = hs_lsp::uri::to_path(&uri) {
                    let path = crate::normalize_path(path);
                    let enc = srv.client.encoding();
                    if diagnostics.is_empty() {
                        self.diagnostics.remove(&path);
                    } else {
                        self.diagnostics.insert(path, (enc, diagnostics));
                    }
                    self.diag_gen += 1;
                }
            }
            Event::Progress {
                token,
                title,
                message,
                percentage,
                done,
            } => {
                if done {
                    srv.progress.remove(&token);
                } else {
                    let mut text = title;
                    if let Some(m) = message {
                        if !text.is_empty() {
                            text.push_str(": ");
                        }
                        text.push_str(&m);
                    }
                    if let Some(p) = percentage {
                        text.push_str(&format!(" {p}%"));
                    }
                    if text.is_empty() {
                        text = s.t("editor-lsp-working");
                    }
                    srv.progress.insert(token, text);
                }
            }
            Event::Log { .. } => {}
            Event::ShowMessage { kind, text } => {
                let level = match kind {
                    1 => ToastLevel::Error,
                    2 => ToastLevel::Warning,
                    _ => ToastLevel::Info,
                };
                if kind <= 2 {
                    self.actions
                        .push(LspAction::Toast(level, format!("{}: {text}", srv.spec.id)));
                }
            }
            Event::ApplyEdit { edits, .. } => {
                let enc = srv.client.encoding();
                for (uri, edits) in edits {
                    if let Some(path) = hs_lsp::uri::to_path(&uri) {
                        self.actions.push(LspAction::ApplyEdits {
                            path: crate::normalize_path(path),
                            edits,
                            enc,
                        });
                    }
                }
            }
            Event::Exited { error } => {
                srv.ready = false;
                srv.progress.clear();
                if let Some(e) = &error {
                    tracing::warn!("{} exited: {e}", srv.spec.id);
                    srv.error = Some(e.clone());
                    self.actions.push(LspAction::Toast(
                        ToastLevel::Warning,
                        s.i18n.t_args(
                            "editor-lsp-failed",
                            &[("server", &srv.spec.id), ("error", e)],
                        ),
                    ));
                }
                let key = key.to_string();
                self.docs.retain(|_, d| d.server != key);
            }
        }
    }

    fn handle_reply(&mut self, s: &Services, r: Reply) {
        match r {
            Reply::Completion { seq, result } => {
                if !self.completion.as_ref().is_some_and(|c| c.seq == seq) {
                    return;
                }
                match result {
                    Ok(v) => {
                        let (mut items, incomplete) =
                            serde_json::from_value::<CompletionResponse>(v)
                                .map(CompletionResponse::into_items)
                                .unwrap_or_default();
                        items.truncate(MAX_ITEMS * 4);
                        let c = self.completion.as_mut().expect("checked above");
                        c.loading = false;
                        c.items = items;
                        c.is_incomplete = incomplete;
                        c.selected = 0;
                        c.scroll = 0;
                    }
                    Err(e) => {
                        tracing::debug!("completion: {e}");
                        self.completion = None;
                    }
                }
            }
            Reply::Hover {
                seq,
                path,
                char,
                pos,
                sticky,
                result,
            } => {
                if seq != self.seq {
                    return;
                }
                if let Ok(v) = result
                    && !v.is_null()
                    && let Ok(h) = serde_json::from_value::<Hover>(v)
                {
                    let text = h.text();
                    if text.trim().is_empty() {
                        return;
                    }
                    self.hover = Some(HoverPopup {
                        path,
                        range: (char, char + 1),
                        pos,
                        text,
                        sticky,
                    });
                    self.hover_range_pending = h.range;
                }
            }
            Reply::Definition { enc, result } => match result {
                Ok(v) => {
                    let locs = hs_lsp::types::locations(&v);
                    match locs.first() {
                        Some(l) => {
                            if let Some(path) = hs_lsp::uri::to_path(&l.uri) {
                                self.actions.push(LspAction::GoTo {
                                    path: crate::normalize_path(path),
                                    line: l.range.start.line,
                                    col: l.range.start.character,
                                    enc,
                                });
                            }
                        }
                        None => self.actions.push(LspAction::Toast(
                            ToastLevel::Info,
                            s.t("editor-definition-none"),
                        )),
                    }
                }
                Err(e) => self
                    .actions
                    .push(LspAction::Toast(ToastLevel::Warning, e.message)),
            },
            Reply::References { enc, result } => match result {
                Ok(v) => {
                    let items: Vec<Loc> = hs_lsp::types::locations(&v)
                        .into_iter()
                        .filter_map(|l| {
                            Some(Loc {
                                path: crate::normalize_path(hs_lsp::uri::to_path(&l.uri)?),
                                line: l.range.start.line,
                                col: l.range.start.character,
                                enc,
                            })
                        })
                        .collect();
                    if items.is_empty() {
                        self.actions.push(LspAction::Toast(
                            ToastLevel::Info,
                            s.t("editor-references-none"),
                        ));
                    } else {
                        self.panel = PanelMode::Locations {
                            title: s.i18n.t_args(
                                "editor-references-title",
                                &[("n", &items.len().to_string())],
                            ),
                            items,
                        };
                        self.panel_open = true;
                    }
                }
                Err(e) => self
                    .actions
                    .push(LspAction::Toast(ToastLevel::Warning, e.message)),
            },
            Reply::Formatting { path, enc, result } => match result {
                Ok(v) => {
                    let edits: Vec<TextEdit> = serde_json::from_value(v).unwrap_or_default();
                    if edits.is_empty() {
                        self.actions.push(LspAction::Toast(
                            ToastLevel::Info,
                            s.t("editor-format-nothing"),
                        ));
                    } else {
                        self.actions
                            .push(LspAction::ApplyEdits { path, edits, enc });
                    }
                }
                Err(e) => self
                    .actions
                    .push(LspAction::Toast(ToastLevel::Warning, e.message)),
            },
            Reply::Rename { enc, result } => match result {
                Ok(v) => {
                    let per_file = hs_lsp::types::workspace_edits(&v);
                    let n = per_file.len();
                    for (uri, edits) in per_file {
                        if let Some(path) = hs_lsp::uri::to_path(&uri) {
                            self.actions.push(LspAction::ApplyEdits {
                                path: crate::normalize_path(path),
                                edits,
                                enc,
                            });
                        }
                    }
                    self.actions.push(LspAction::Toast(
                        ToastLevel::Success,
                        s.i18n
                            .t_args("editor-rename-done", &[("count", &n.to_string())]),
                    ));
                }
                Err(e) => self.actions.push(LspAction::Toast(
                    ToastLevel::Error,
                    s.i18n
                        .t_args("editor-rename-failed", &[("error", &e.message)]),
                )),
            },
        }
    }

    /// What the tool has to do (opened files, edits, toasts).
    pub fn take_actions(&mut self) -> Vec<LspAction> {
        std::mem::take(&mut self.actions)
    }

    /// Diagnostics of a file as underlines, cached per buffer version.
    pub fn squiggles(&mut self, path: &Path, buf: &Buffer) -> Vec<Squiggle> {
        let Some(d) = self.docs.get_mut(path) else {
            return Vec::new();
        };
        if d.squiggles.0 == self.diag_gen && d.squiggles.1 == buf.version() {
            return d.squiggles.2.clone();
        }
        let lines = BufLines(buf);
        let out: Vec<Squiggle> = self
            .diagnostics
            .get(path)
            .map(|(enc, list)| {
                list.iter()
                    .map(|diag| {
                        let (start, end) = text::range_to_chars(&lines, diag.range, *enc);
                        Squiggle {
                            start,
                            end: end.max(start + 1),
                            level: match diag.severity() {
                                Severity::Error => SquiggleLevel::Error,
                                Severity::Warning => SquiggleLevel::Warning,
                                Severity::Information => SquiggleLevel::Info,
                                Severity::Hint => SquiggleLevel::Hint,
                            },
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        d.squiggles = (self.diag_gen, buf.version(), out.clone());
        out
    }

    /// Error and warning counts over every file with diagnostics.
    pub fn counts(&self) -> (usize, usize) {
        let mut e = 0;
        let mut w = 0;
        for (_, list) in self.diagnostics.values() {
            for d in list {
                match d.severity() {
                    Severity::Error => e += 1,
                    Severity::Warning => w += 1,
                    _ => {}
                }
            }
        }
        (e, w)
    }

    /// The diagnostic messages under a char of a file (for the tooltip).
    fn messages_at(&self, path: &Path, buf: &Buffer, at: usize) -> Vec<String> {
        let Some((enc, list)) = self.diagnostics.get(path) else {
            return Vec::new();
        };
        let lines = BufLines(buf);
        list.iter()
            .filter(|d| {
                let (s, e) = text::range_to_chars(&lines, d.range, *enc);
                at >= s && at < e.max(s + 1)
            })
            .map(|d| match (&d.source, d.code_text()) {
                (Some(src), Some(code)) => format!("{} [{src} {code}]", d.message),
                (Some(src), None) => format!("{} [{src}]", d.message),
                (None, Some(code)) => format!("{} [{code}]", d.message),
                (None, None) => d.message.clone(),
            })
            .collect()
    }

    // ---- requests ---------------------------------------------------------

    /// Open the completion list at the cursor (Ctrl+Space, or typing).
    pub fn request_completion(
        &mut self,
        path: &Path,
        buf: &Buffer,
        cursors: &Cursors,
        trigger: Option<&str>,
    ) {
        if cursors.list.len() != 1 || cursors.primary().has_selection() {
            return;
        }
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return;
        };
        if !client.capabilities().completion {
            return;
        }
        let enc = client.encoding();
        let head = cursors.primary().head;
        let anchor = match trigger {
            Some(_) => head,
            None => word_start(buf, head),
        };
        self.seq += 1;
        let seq = self.seq;
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.completion(
            &hs_lsp::uri::from_path(path),
            Self::position(buf, head, enc),
            trigger,
            Box::new(move |result| {
                let _ = tx.send(Reply::Completion { seq, result });
                wake();
            }),
        );
        let keep_items = self
            .completion
            .take()
            .filter(|c| c.path == path && c.anchor == anchor)
            .map(|c| c.items)
            .unwrap_or_default();
        self.completion = Some(Completion {
            path: path.to_path_buf(),
            anchor,
            seq,
            loading: true,
            is_incomplete: false,
            items: keep_items,
            filtered: Vec::new(),
            selected: 0,
            scroll: 0,
            enc,
        });
    }

    /// Hover for the symbol at the cursor (keyboard): stays until a key
    /// or a click.
    pub fn request_hover_at_cursor(
        &mut self,
        path: &Path,
        buf: &Buffer,
        cursors: &Cursors,
        view: &EditorView,
    ) {
        let at = cursors.primary().head;
        let pos = view
            .cursor_rect
            .map(|r| r.left_bottom() + egui::vec2(0.0, 2.0))
            .unwrap_or(egui::Pos2::ZERO);
        self.request_hover(path, buf, at, pos, true);
    }

    fn request_hover(
        &mut self,
        path: &Path,
        buf: &Buffer,
        at: usize,
        pos: egui::Pos2,
        sticky: bool,
    ) {
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return;
        };
        if !client.capabilities().hover {
            return;
        }
        let enc = client.encoding();
        self.seq += 1;
        let seq = self.seq;
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        let p = path.to_path_buf();
        client.hover(
            &hs_lsp::uri::from_path(path),
            Self::position(buf, at, enc),
            Box::new(move |result| {
                let _ = tx.send(Reply::Hover {
                    seq,
                    path: p,
                    char: at,
                    pos,
                    sticky,
                    result,
                });
                wake();
            }),
        );
    }

    pub fn request_definition(&mut self, path: &Path, buf: &Buffer, at: usize) {
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return;
        };
        if !client.capabilities().definition {
            return;
        }
        let enc = client.encoding();
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.definition(
            &hs_lsp::uri::from_path(path),
            Self::position(buf, at, enc),
            Box::new(move |result| {
                let _ = tx.send(Reply::Definition { enc, result });
                wake();
            }),
        );
    }

    pub fn request_references(&mut self, path: &Path, buf: &Buffer, at: usize) {
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return;
        };
        if !client.capabilities().references {
            return;
        }
        let enc = client.encoding();
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.references(
            &hs_lsp::uri::from_path(path),
            Self::position(buf, at, enc),
            Box::new(move |result| {
                let _ = tx.send(Reply::References { enc, result });
                wake();
            }),
        );
    }

    /// Format the whole document through the server. Returns false when
    /// the server has no formatter.
    pub fn request_formatting(&mut self, path: &Path, buf: &Buffer, view: &EditorView) -> bool {
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return false;
        };
        if !client.capabilities().formatting {
            return false;
        }
        let enc = client.encoding();
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        let p = path.to_path_buf();
        client.formatting(
            &hs_lsp::uri::from_path(path),
            view.language.indent.max(1) as u32,
            view.tab_inserts_spaces,
            Box::new(move |result| {
                let _ = tx.send(Reply::Formatting {
                    path: p,
                    enc,
                    result,
                });
                wake();
            }),
        );
        true
    }

    /// Open the rename prompt for the symbol at the cursor.
    pub fn start_rename(&mut self, path: &Path, buf: &Buffer, cursors: &Cursors) -> bool {
        let Some(client) = self.client_for(path) else {
            return false;
        };
        if !client.capabilities().rename {
            return false;
        }
        let at = cursors.primary().head;
        let text = buf.slice(word_start(buf, at), buf.word_end(at));
        self.rename = Some(RenamePrompt {
            path: path.to_path_buf(),
            pos: at,
            text,
            focus: true,
        });
        true
    }

    fn request_rename(&mut self, path: &Path, buf: &Buffer, at: usize, new_name: &str) {
        self.flush(path, buf);
        let Some(client) = self.client_for(path) else {
            return;
        };
        let enc = client.encoding();
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.rename(
            &hs_lsp::uri::from_path(path),
            Self::position(buf, at, enc),
            new_name,
            Box::new(move |result| {
                let _ = tx.send(Reply::Rename { enc, result });
                wake();
            }),
        );
    }

    // ---- UI ----------------------------------------------------------------

    /// Everything that follows the editor widget: completion triggers and
    /// keys, the completion list, hover, Ctrl+click, the rename prompt.
    #[allow(clippy::too_many_arguments)]
    pub fn after_show(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        path: Option<&Path>,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        view: &mut EditorView,
        resp: &EditorResponse,
    ) {
        let Some(path) = path.filter(|p| self.has_server(p)) else {
            view.popup_open = false;
            self.completion = None;
            self.hover = None;
            self.hover_pending = None;
            if let Some(p) = path {
                self.diagnostic_tooltip(ui, p, buf, resp);
            }
            return;
        };
        // Typing: trigger or refine completion; any edit closes hover.
        if let Some(t) = &resp.typed {
            self.hover = None;
            self.hover_pending = None;
            let triggers = self
                .client_for(path)
                .map(|c| c.capabilities().completion_triggers)
                .unwrap_or_default();
            if triggers.iter().any(|tr| tr == t) {
                self.request_completion(path, buf, cursors, Some(t.as_str()));
            } else if t.chars().all(Buffer::is_word_char) {
                let open = self.completion.as_ref().is_some_and(|c| c.path == path);
                if open {
                    if self.completion.as_ref().is_some_and(|c| c.is_incomplete) {
                        self.request_completion(path, buf, cursors, None);
                    }
                } else if self.complete_on_type {
                    self.request_completion(path, buf, cursors, None);
                }
            } else {
                self.completion = None;
            }
        } else if resp.changed {
            // Backspace and the like: the list stays while the word is
            // still being edited and goes when the cursor left it.
            if let Some(c) = &self.completion
                && cursors.primary().head < c.anchor
            {
                self.completion = None;
            }
            self.hover = None;
        }
        if resp.clicked {
            self.completion = None;
            self.hover = None;
        }
        if resp.ctrl_clicked {
            self.request_definition(path, buf, cursors.primary().head);
        }
        // A cursor that left the completion's line closes it.
        if let Some(c) = &self.completion {
            let head = cursors.primary().head;
            if c.path != path
                || head < c.anchor
                || buf.char_to_line_col(head).0 != buf.char_to_line_col(c.anchor).0
            {
                self.completion = None;
            }
        }
        // Keys the view handed over while the list was open.
        let mut accept = self.pending_accept.take();
        let mut close = false;
        if let Some(c) = self.completion.as_mut() {
            for k in &resp.popup_keys {
                let n = c.filtered.len();
                match k {
                    egui::Key::Escape => close = true,
                    egui::Key::ArrowDown if n > 0 => c.selected = (c.selected + 1) % n,
                    egui::Key::ArrowUp if n > 0 => c.selected = (c.selected + n - 1) % n,
                    egui::Key::PageDown if n > 0 => {
                        c.selected = (c.selected + COMPLETION_ROWS).min(n - 1)
                    }
                    egui::Key::PageUp if n > 0 => {
                        c.selected = c.selected.saturating_sub(COMPLETION_ROWS)
                    }
                    egui::Key::Enter | egui::Key::Tab => {
                        if n > 0 {
                            accept = Some(c.filtered[c.selected]);
                        } else {
                            close = true;
                        }
                    }
                    _ => {}
                }
            }
        }
        if close {
            self.completion = None;
        }
        if let Some(i) = accept {
            self.accept_completion(i, buf, cursors, view);
        }
        if self.hover.as_ref().is_some_and(|h| h.sticky)
            && (resp.popup_keys.contains(&egui::Key::Escape)
                || ui.input(|i| i.key_pressed(egui::Key::Escape)))
        {
            self.hover = None;
        }
        self.refilter(buf, cursors);
        view.popup_open = self
            .completion
            .as_ref()
            .is_some_and(|c| !c.filtered.is_empty() || c.loading);
        self.completion_popup(ui, s, theme, view);
        self.hover_logic(ui, s, theme, path, buf, resp);
        self.diagnostic_tooltip(ui, path, buf, resp);
        self.rename_modal(ui, s, buf);
    }

    fn refilter(&mut self, buf: &Buffer, cursors: &Cursors) {
        let Some(c) = self.completion.as_mut() else {
            return;
        };
        let head = cursors.primary().head;
        let prefix = buf.slice(c.anchor, head).to_lowercase();
        let mut scored: Vec<(u8, &str, usize)> = c
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| {
                let key = item.filter_key().to_lowercase();
                let tier = if prefix.is_empty() {
                    u8::from(item.preselect != Some(true))
                } else if key.starts_with(&prefix) {
                    0
                } else if key.contains(&prefix) {
                    1
                } else if is_subsequence(&prefix, &key) {
                    2
                } else {
                    return None;
                };
                Some((tier, item.sort_text.as_deref().unwrap_or(&item.label), i))
            })
            .collect();
        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        scored.truncate(MAX_ITEMS);
        let before = c.filtered.get(c.selected).copied();
        let filtered: Vec<usize> = scored.into_iter().map(|(_, _, i)| i).collect();
        c.filtered = filtered;
        c.selected = before
            .and_then(|b| c.filtered.iter().position(|&i| i == b))
            .unwrap_or(0);
        if c.selected < c.scroll || c.selected >= c.scroll + COMPLETION_ROWS {
            c.scroll = c.selected.saturating_sub(COMPLETION_ROWS / 2);
        }
    }

    fn accept_completion(
        &mut self,
        index: usize,
        buf: &mut Buffer,
        cursors: &mut Cursors,
        view: &mut EditorView,
    ) {
        let Some(c) = self.completion.take() else {
            return;
        };
        let Some(item) = c.items.get(index) else {
            return;
        };
        let head = cursors.primary().head;
        let lines = BufLines(buf);
        let edit = item.edit();
        let raw = match &edit {
            Some(te) => te.new_text.clone(),
            None => item.insert_text().to_string(),
        };
        let (text, cursor_off) = if item.is_snippet() {
            text::strip_snippet(&raw)
        } else {
            (raw, None)
        };
        let start = match &edit {
            Some(te) => text::range_to_chars(&lines, te.range, c.enc).0.min(head),
            None => c.anchor.min(head),
        };
        let mut edits: Vec<(usize, usize, String)> = item
            .additional_text_edits
            .iter()
            .map(|e| {
                let (s, en) = text::range_to_chars(&lines, e.range, c.enc);
                (s, en, e.new_text.clone())
            })
            .collect();
        let mut cursor = start + cursor_off.unwrap_or_else(|| text.chars().count());
        for (s, e, t) in &edits {
            if *e <= start {
                cursor = cursor + t.chars().count() - (e - s);
            }
        }
        edits.push((start, head, text));
        edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
        view.apply_edits(buf, cursors, &edits, Some(cursor));
    }

    fn completion_popup(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        view: &EditorView,
    ) {
        let id = self.id;
        let Some(c) = self.completion.as_ref() else {
            return;
        };
        if c.filtered.is_empty() && !c.loading {
            return;
        }
        let Some(rect) = view.cursor_rect else {
            return;
        };
        let font = egui::FontId::new(view.font_size, hs_ui::fonts::editor_family());
        let row_h = ui.fonts_mut(|fonts| fonts.row_height(&font)) + 6.0;
        let screen = ui.ctx().content_rect();
        let width = 460.0_f32.min(screen.width() - 16.0);
        let rows = c.filtered.len().clamp(1, COMPLETION_ROWS);
        let height = rows as f32 * row_h + 8.0;
        let below = rect.bottom() + height + 4.0 <= screen.bottom();
        let x =
            (rect.left() - 8.0).clamp(screen.left(), (screen.right() - width).max(screen.left()));
        let y = if below {
            rect.bottom() + 2.0
        } else {
            (rect.top() - height - 2.0).max(screen.top())
        };
        let mut clicked: Option<usize> = None;
        let selected_doc = c
            .filtered
            .get(c.selected)
            .and_then(|&i| c.items.get(i))
            .and_then(|it| {
                let mut parts = Vec::new();
                if let Some(d) = &it.detail {
                    parts.push(d.trim().to_string());
                }
                if let Some(d) = it.documentation_text() {
                    parts.push(d.trim().to_string());
                }
                (!parts.is_empty()).then(|| parts.join("\n\n"))
            });
        let small = egui::FontId::proportional((view.font_size - 1.0).max(9.0));
        egui::Area::new(id.with("completion"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(x, y))
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.surface)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        if c.filtered.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(s.t("editor-completion-loading"))
                                        .color(theme.text_muted),
                                )
                                .selectable(false),
                            );
                            return;
                        }
                        let end = (c.scroll + COMPLETION_ROWS).min(c.filtered.len());
                        for row in c.scroll..end {
                            let i = c.filtered[row];
                            let item = &c.items[i];
                            let (r, resp) = ui.allocate_exact_size(
                                egui::vec2(width, row_h),
                                egui::Sense::click(),
                            );
                            let selected = row == c.selected;
                            if selected || resp.hovered() {
                                ui.painter().rect_filled(
                                    r,
                                    egui::CornerRadius::same(3),
                                    if selected {
                                        theme.accent.linear_multiply(0.35)
                                    } else {
                                        theme.surface_hover
                                    },
                                );
                            }
                            let kind = item.kind_name();
                            let ty = r.center().y;
                            let painter = ui.painter();
                            painter.text(
                                egui::pos2(r.left() + 6.0, ty),
                                egui::Align2::LEFT_CENTER,
                                kind_icon(kind),
                                egui::FontId::proportional(view.font_size),
                                kind_colour(kind, theme),
                            );
                            let label_colour = if item.deprecated == Some(true) {
                                theme.text_muted
                            } else {
                                theme.text
                            };
                            let label_w = (width - 36.0) * 0.6;
                            let label = truncate_to(&item.label, &font, label_w, ui);
                            painter.text(
                                egui::pos2(r.left() + 28.0, ty),
                                egui::Align2::LEFT_CENTER,
                                label,
                                font.clone(),
                                label_colour,
                            );
                            if let Some(d) = &item.detail {
                                let detail = truncate_to(
                                    d.lines().next().unwrap_or("").trim(),
                                    &small,
                                    width - 36.0 - label_w - 8.0,
                                    ui,
                                );
                                painter.text(
                                    egui::pos2(r.right() - 6.0, ty),
                                    egui::Align2::RIGHT_CENTER,
                                    detail,
                                    small.clone(),
                                    theme.text_muted,
                                );
                            }
                            if resp.clicked() {
                                clicked = Some(i);
                            }
                        }
                        if c.filtered.len() > COMPLETION_ROWS {
                            ui.add_space(2.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "{}/{}",
                                        c.selected + 1,
                                        c.filtered.len()
                                    ))
                                    .small()
                                    .color(theme.text_muted),
                                )
                                .selectable(false),
                            );
                        }
                    });
            });
        // The selected item's documentation beside the list.
        if let Some(doc) = selected_doc {
            let dx = x + width + 6.0;
            let dw = 360.0_f32.min((screen.right() - dx - 8.0).max(120.0));
            let pos = if dx + 120.0 < screen.right() {
                egui::pos2(dx, y)
            } else {
                egui::pos2((x - 366.0).max(screen.left()), y)
            };
            let font_size = view.font_size;
            egui::Area::new(id.with("completion-doc"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(theme.surface)
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .show(ui, |ui| {
                            ui.set_width(dw);
                            ui.set_max_height(260.0);
                            egui::ScrollArea::vertical()
                                .id_salt(id.with("completion-doc-scroll"))
                                .show(ui, |ui| {
                                    markdown(ui, theme, &doc, font_size);
                                });
                        });
                });
        }
        if clicked.is_some() {
            // A click accepts, like Enter, on the next frame.
            self.pending_accept = clicked;
        }
    }

    fn hover_logic(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        path: &Path,
        buf: &Buffer,
        resp: &EditorResponse,
    ) {
        let enc = self
            .client_for(path)
            .map(|c| c.encoding())
            .unwrap_or_default();
        // Dwell on a symbol asks the server once.
        if let (Some(ch), Some(pos)) = (resp.hover_char, resp.hover_pos)
            && self.completion.is_none()
        {
            let word = (word_start(buf, ch), buf.word_end(ch));
            if word.0 == word.1 {
                self.hover_pending = None;
            } else {
                let mut pending = self.hover_pending.take().unwrap_or(HoverPending {
                    char: ch,
                    since: Instant::now(),
                    requested: false,
                });
                if !(word.0 <= pending.char && pending.char < word.1) {
                    pending = HoverPending {
                        char: ch,
                        since: Instant::now(),
                        requested: false,
                    };
                }
                let already = self
                    .hover
                    .as_ref()
                    .is_some_and(|h| h.path == path && h.range.0 <= ch && ch < h.range.1);
                let ask = !pending.requested && !already && pending.since.elapsed() >= HOVER_DWELL;
                if ask {
                    pending.requested = true;
                } else if !pending.requested && !already {
                    ui.ctx().request_repaint_after(HOVER_DWELL);
                }
                self.hover_pending = Some(pending);
                if ask {
                    self.request_hover(path, buf, ch, egui::pos2(pos.x, pos.y + 18.0), false);
                }
            }
        }
        // Resolve the server's range once against the buffer.
        if let Some(r) = self.hover_range_pending.take()
            && let Some(h) = self.hover.as_mut()
        {
            h.range = text::range_to_chars(&BufLines(buf), r, enc);
            if h.range.1 <= h.range.0 {
                h.range.1 = h.range.0 + 1;
            }
        } else if let Some(h) = self.hover.as_mut()
            && h.range.1 == h.range.0 + 1
        {
            let ch = h.range.0;
            h.range = (word_start(buf, ch), buf.word_end(ch).max(ch + 1));
        }
        let Some(h) = &self.hover else {
            return;
        };
        if h.path != path {
            self.hover = None;
            return;
        }
        let popup_id = self.id.with("hover");
        let pointer_over_popup = ui
            .ctx()
            .read_response(popup_id)
            .is_some_and(|r| r.contains_pointer());
        let over_range = resp
            .hover_char
            .is_some_and(|c| c >= h.range.0 && c < h.range.1);
        if !h.sticky && !over_range && !pointer_over_popup {
            self.hover = None;
            return;
        }
        let text = h.text.clone();
        let pos = h.pos;
        let width = 520.0_f32.min(ui.ctx().content_rect().width() - 16.0);
        let font_size = s.settings.read().editor.font_size;
        egui::Area::new(popup_id)
            .order(egui::Order::Tooltip)
            .fixed_pos(pos)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.surface)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .show(ui, |ui| {
                        ui.set_max_width(width);
                        ui.set_max_height(320.0);
                        egui::ScrollArea::vertical()
                            .id_salt(popup_id.with("scroll"))
                            .show(ui, |ui| {
                                markdown(ui, theme, &text, font_size);
                            });
                    });
            });
    }

    /// The diagnostic under the mouse as a tooltip.
    fn diagnostic_tooltip(
        &self,
        ui: &mut egui::Ui,
        path: &Path,
        buf: &Buffer,
        resp: &EditorResponse,
    ) {
        let (Some(ch), Some(pos)) = (resp.hover_char, resp.hover_pos) else {
            return;
        };
        if self.hover.is_some() || self.completion.is_some() {
            return;
        }
        let msgs = self.messages_at(path, buf, ch);
        if msgs.is_empty() {
            return;
        }
        egui::Area::new(self.id.with("diag-tip"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos + egui::vec2(0.0, 18.0))
            .constrain(true)
            .interactable(false)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(480.0);
                    for m in msgs {
                        ui.label(m);
                    }
                });
            });
    }

    fn rename_modal(&mut self, ui: &mut egui::Ui, s: &Services, buf: &Buffer) {
        let Some(mut prompt) = self.rename.take() else {
            return;
        };
        let mut done = false;
        let mut cancel = false;
        let modal = egui::Modal::new(self.id.with("rename")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.heading(s.t("editor-rename-symbol"));
            ui.add_space(6.0);
            let r =
                ui.add(egui::TextEdit::singleline(&mut prompt.text).desired_width(f32::INFINITY));
            if prompt.focus {
                r.request_focus();
                prompt.focus = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                done = true;
            }
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(s.t("editor-rename-symbol-hint"))
                    .small()
                    .weak(),
            );
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(s.t("ok")).clicked() {
                    done = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if modal.should_close() {
            cancel = true;
        }
        if done {
            let name = prompt.text.trim().to_string();
            if !name.is_empty() {
                self.request_rename(&prompt.path, buf, prompt.pos, &name);
            }
        } else if !cancel {
            self.rename = Some(prompt);
        }
    }

    /// The Problems / References panel. Draw it as a bottom panel of the
    /// editor area when `panel_open`.
    pub fn panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        root: Option<&Path>,
    ) {
        let mut go: Option<(PathBuf, u32, u32, Encoding)> = None;
        let mut close = false;
        let mut show_problems = false;
        ui.horizontal(|ui| {
            let title = match &self.panel {
                PanelMode::Problems => {
                    let (e, w) = self.counts();
                    format!(
                        "{}   {} {e}   {} {w}",
                        s.t("editor-problems"),
                        ph::X_CIRCLE,
                        ph::WARNING
                    )
                }
                PanelMode::Locations { title, .. } => title.clone(),
            };
            ui.label(egui::RichText::new(title).strong());
            if matches!(self.panel, PanelMode::Locations { .. })
                && ui
                    .small_button(s.t("editor-problems"))
                    .tip(s.t("editor-problems-tip"))
                    .clicked()
            {
                show_problems = true;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(ph::X).clicked() {
                    close = true;
                }
            });
        });
        ui.separator();
        let rel = |p: &Path| -> String {
            root.and_then(|r| p.strip_prefix(r).ok())
                .map(|r| r.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        };
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("panel-scroll"))
            .auto_shrink([false, false])
            .show(ui, |ui| match &self.panel {
                PanelMode::Problems => {
                    let mut rows: Vec<(&PathBuf, Encoding, &Diagnostic)> = self
                        .diagnostics
                        .iter()
                        .flat_map(|(p, (enc, list))| list.iter().map(move |d| (p, *enc, d)))
                        .collect();
                    rows.sort_by(|a, b| a.0.cmp(b.0).then(a.2.range.start.cmp(&b.2.range.start)));
                    if rows.is_empty() {
                        ui.label(
                            egui::RichText::new(s.t("editor-problems-none"))
                                .color(theme.text_muted),
                        );
                    }
                    for (p, enc, d) in rows {
                        let (icon, colour) = match d.severity() {
                            Severity::Error => (ph::X_CIRCLE, theme.error),
                            Severity::Warning => (ph::WARNING, theme.warning),
                            _ => (ph::INFO, theme.secondary),
                        };
                        let text = format!(
                            "{}   {}:{}:{}",
                            d.message.lines().next().unwrap_or(""),
                            rel(p),
                            d.range.start.line + 1,
                            d.range.start.character + 1
                        );
                        let r = ui.horizontal(|ui| {
                            ui.colored_label(colour, icon);
                            ui.add(egui::Label::new(text).truncate().selectable(false))
                        });
                        let r = r.inner.union(r.response);
                        if r.interact(egui::Sense::click()).clicked() {
                            go =
                                Some((p.clone(), d.range.start.line, d.range.start.character, enc));
                        }
                    }
                }
                PanelMode::Locations { items, .. } => {
                    for l in items {
                        let text = format!("{}:{}:{}", rel(&l.path), l.line + 1, l.col + 1);
                        if ui
                            .add(
                                egui::Label::new(text)
                                    .truncate()
                                    .selectable(false)
                                    .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            go = Some((l.path.clone(), l.line, l.col, l.enc));
                        }
                    }
                }
            });
        if let Some((path, line, col, enc)) = go {
            self.actions.push(LspAction::GoTo {
                path,
                line,
                col,
                enc,
            });
        }
        if show_problems {
            self.panel = PanelMode::Problems;
        }
        if close {
            self.panel_open = false;
        }
    }

    pub fn toggle_problems(&mut self) {
        if self.panel_open && matches!(self.panel, PanelMode::Problems) {
            self.panel_open = false;
        } else {
            self.panel = PanelMode::Problems;
            self.panel_open = true;
        }
    }

    /// The status-bar segment for a file: the server's state, or how to
    /// install one: `(text, tooltip, busy)`. `None` when there is nothing
    /// to say (servers off, a language without one).
    pub fn status(&self, s: &Services, path: Option<&Path>, language: &str) -> Option<LspStatus> {
        if !self.enabled {
            return None;
        }
        let path = path?;
        if let Some(d) = self.docs.get(path)
            && let Some(srv) = self.servers.get(&d.server)
        {
            let busy = !srv.progress.is_empty() || !srv.ready;
            let text = if let Some(e) = &srv.error {
                format!("{} {}: {e}", ph::WARNING, srv.spec.id)
            } else if let Some(p) = srv.progress.values().next() {
                format!("{} {}: {p}", ph::CIRCLE_NOTCH, srv.spec.id)
            } else if !srv.ready {
                s.i18n
                    .t_args("editor-lsp-starting", &[("server", &srv.spec.id)])
            } else {
                format!("{} {}", ph::CHECK, srv.spec.id)
            };
            let tip = s.i18n.t_args(
                "editor-lsp-tip",
                &[("server", &srv.spec.id), ("command", &srv.spec.command)],
            );
            return Some(LspStatus {
                text,
                tip,
                busy,
                install: None,
            });
        }
        let spec = hs_lsp::registry::resolve(language, &self.overrides)?;
        if spec.path.is_some() {
            // Installed, but this file has no server session (no project).
            return None;
        }
        Some(LspStatus {
            text: format!("{} {}", ph::WARNING, spec.id),
            tip: s.i18n.t_args(
                "editor-lsp-missing-tip",
                &[
                    ("language", language),
                    ("server", &spec.id),
                    ("install", &spec.install),
                ],
            ),
            busy: false,
            install: Some(Install {
                server: spec.id.clone(),
                command: spec.install_command.clone(),
                url: spec.install_url.clone(),
            }),
        })
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

/// Start of the identifier `at` is in or right after.
fn word_start(buf: &Buffer, at: usize) -> usize {
    if at > 0 && buf.char_at(at - 1).is_some_and(Buffer::is_word_char) {
        buf.word_start(at - 1)
    } else if buf.char_at(at).is_some_and(Buffer::is_word_char) {
        buf.word_start(at)
    } else {
        at
    }
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "method" | "function" | "constructor" => ph::FUNCTION,
        "field" | "property" => ph::TAG,
        "variable" | "constant" | "value" => ph::CUBE,
        "class" | "struct" | "interface" | "type parameter" => ph::SHAPES,
        "module" | "folder" | "file" => ph::FOLDER,
        "enum" | "enum member" => ph::LIST_BULLETS,
        "keyword" => ph::KEY,
        "snippet" => ph::TEXT_T,
        "text" => ph::TEXT_AA,
        _ => ph::DOT,
    }
}

fn kind_colour(kind: &str, theme: &Theme) -> egui::Color32 {
    match kind {
        "method" | "function" | "constructor" => theme.secondary,
        "class" | "struct" | "interface" | "enum" | "type parameter" => theme.warning,
        "keyword" | "snippet" => theme.accent,
        _ => theme.text_muted,
    }
}

fn truncate_to(text: &str, font: &egui::FontId, max_w: f32, ui: &egui::Ui) -> String {
    let w = |t: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(t.to_string(), font.clone(), egui::Color32::WHITE)
                .size()
                .x
        })
    };
    if w(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut n = chars.len();
    while n > 1 {
        n -= 1;
        let candidate: String = chars[..n].iter().collect::<String>() + "…";
        if w(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".into()
}

/// Hover and documentation text: Markdown with fenced code in monospace.
fn markdown(ui: &mut egui::Ui, theme: &Theme, text: &str, font_size: f32) {
    let style = hs_editor::markdown::MarkdownStyle {
        dark: theme.dark,
        text: theme.text,
        muted: theme.text_muted,
        link: theme.accent,
        code_bg: theme.panel_bg,
        quote_bar: theme.border,
        base_size: font_size,
    };
    hs_editor::markdown::render(ui, text, &style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_start_finds_the_identifier_under_or_before_the_cursor() {
        let buf = Buffer::from_text("let foo_bar = baz;");
        assert_eq!(word_start(&buf, 11), 4, "inside foo_bar");
        assert_eq!(word_start(&buf, 4), 4, "at its start");
        assert_eq!(word_start(&buf, 12), 12, "after a space");
        assert_eq!(word_start(&buf, 0), 0);
    }

    #[test]
    fn subsequence_matching_is_loose_but_ordered() {
        assert!(is_subsequence("fb", "foo_bar"));
        assert!(!is_subsequence("bf", "foo_bar"));
        assert!(is_subsequence("", "x"));
    }

    #[test]
    fn buffer_lines_match_the_protocol_view() {
        let buf = Buffer::from_text("ab\r\ncd\n");
        let l = BufLines(&buf);
        assert_eq!(l.line(0), "ab");
        assert_eq!(l.line(1), "cd");
        // The buffer keeps LF inside and remembers CRLF for saving.
        assert_eq!(l.line_start_char(1), 3);
        assert_eq!(l.line_count(), 3);
    }
}
