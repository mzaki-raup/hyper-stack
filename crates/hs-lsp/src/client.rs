//! One language server process: JSON-RPC 2.0 over stdio with
//! `Content-Length` framing. A reader thread parses responses and server
//! messages; writes go straight to the child's stdin under a lock. The
//! client is `Send + Sync` and cheap to share.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use serde_json::{Value, json};

use crate::text::Encoding;
use crate::types::{Diagnostic, Position, TextEdit};

/// Runs on the reader thread when the server answers (or the request
/// cannot be answered any more).
pub type Callback = Box<dyn FnOnce(Result<Value, RpcError>) + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

/// What the server tells the editor without being asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `initialize` answered: documents can be opened.
    Ready { name: String, version: String },
    Diagnostics {
        uri: String,
        version: Option<i64>,
        diagnostics: Vec<Diagnostic>,
    },
    /// `$/progress` (indexing, loading); `done` ends the token.
    Progress {
        token: String,
        title: String,
        message: Option<String>,
        percentage: Option<u32>,
        done: bool,
    },
    /// `window/logMessage`.
    Log { text: String },
    /// `window/showMessage`: 1 error, 2 warning, 3 info, 4 log.
    ShowMessage { kind: u8, text: String },
    /// `workspace/applyEdit`: edits per document URI, already accepted.
    ApplyEdit {
        label: Option<String>,
        edits: Vec<(String, Vec<TextEdit>)>,
    },
    /// The process is gone: it crashed, or shutdown finished.
    Exited { error: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Starting,
    Ready,
    Stopping,
    Dead,
}

/// What the server said it can do, reduced to what the editor asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub position_encoding: Encoding,
    pub completion: bool,
    pub completion_triggers: Vec<String>,
    pub hover: bool,
    pub definition: bool,
    pub references: bool,
    pub formatting: bool,
    pub rename: bool,
    /// 0 none, 1 full, 2 incremental (the client always sends full text).
    pub sync_kind: u8,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            position_encoding: Encoding::Utf16,
            completion: false,
            completion_triggers: Vec::new(),
            hover: false,
            definition: false,
            references: false,
            formatting: false,
            rename: false,
            sync_kind: 1,
        }
    }
}

impl Capabilities {
    fn parse(v: &Value) -> Self {
        let caps = v.get("capabilities").cloned().unwrap_or(Value::Null);
        let has = |k: &str| {
            !matches!(
                caps.get(k),
                None | Some(Value::Null) | Some(Value::Bool(false))
            )
        };
        let sync_kind = match caps.get("textDocumentSync") {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(1) as u8,
            Some(Value::Object(o)) => o.get("change").and_then(Value::as_u64).unwrap_or(1) as u8,
            _ => 1,
        };
        Self {
            position_encoding: caps
                .get("positionEncoding")
                .and_then(Value::as_str)
                .map(Encoding::parse)
                .unwrap_or_default(),
            completion: has("completionProvider"),
            completion_triggers: caps
                .get("completionProvider")
                .and_then(|c| c.get("triggerCharacters"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            hover: has("hoverProvider"),
            definition: has("definitionProvider"),
            references: has("referencesProvider"),
            formatting: has("documentFormattingProvider"),
            rename: has("renameProvider"),
            sync_kind,
        }
    }
}

struct Inner {
    name: String,
    child: Mutex<Option<Child>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, (String, Callback)>>,
    state: Mutex<State>,
    caps: Mutex<Capabilities>,
    /// Messages written before `initialize` was answered.
    queue: Mutex<Vec<Vec<u8>>>,
    /// The last lines the server wrote to stderr, for the exit report.
    stderr_tail: Mutex<Vec<String>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// A running language server.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    /// Start `command args…` for the project at `root`. Events go to
    /// `events`; `wake` is called after each so the UI repaints.
    pub fn spawn(
        name: &str,
        command: &Path,
        args: &[String],
        root: &Path,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> std::io::Result<Self> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take();
        let client = Self::from_streams(name, stdout, stdin, Some(child), root, events, wake);
        if let Some(err) = stderr {
            let inner = client.inner.clone();
            std::thread::Builder::new()
                .name(format!("lsp-{name}-stderr"))
                .spawn(move || {
                    for line in BufReader::new(err).lines().map_while(Result::ok) {
                        tracing::debug!(server = %inner.name, "{line}");
                        let mut tail = inner.stderr_tail.lock().unwrap();
                        tail.push(line);
                        if tail.len() > 20 {
                            tail.remove(0);
                        }
                    }
                })
                .ok();
        }
        Ok(client)
    }

    /// A client over any pair of streams (tests use an in-process pipe).
    /// `initialize` is sent at once.
    pub fn from_streams(
        name: &str,
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        child: Option<Child>,
        root: &Path,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let inner = Arc::new(Inner {
            name: name.to_string(),
            child: Mutex::new(child),
            writer: Mutex::new(Some(Box::new(writer))),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            state: Mutex::new(State::Starting),
            caps: Mutex::new(Capabilities::default()),
            queue: Mutex::new(Vec::new()),
            stderr_tail: Mutex::new(Vec::new()),
            events,
            wake,
        });
        let client = Self { inner };
        {
            let inner = client.inner.clone();
            std::thread::Builder::new()
                .name(format!("lsp-{name}"))
                .spawn(move || reader_loop(inner, reader))
                .expect("spawn reader thread");
        }
        client.initialize(root);
        client
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub fn state(&self) -> State {
        *self.inner.state.lock().unwrap()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.inner.caps.lock().unwrap().clone()
    }

    pub fn encoding(&self) -> Encoding {
        self.inner.caps.lock().unwrap().position_encoding
    }

    fn initialize(&self, root: &Path) {
        let root_uri = crate::uri::from_path(root);
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": {"name": "HyperStack", "version": env!("CARGO_PKG_VERSION")},
            "rootUri": root_uri,
            "workspaceFolders": [{"uri": root_uri, "name": name}],
            "capabilities": {
                "general": {"positionEncodings": ["utf-8", "utf-16"]},
                "workspace": {
                    "applyEdit": true,
                    "workspaceEdit": {"documentChanges": true},
                    "configuration": true,
                    "workspaceFolders": true,
                    "didChangeWatchedFiles": {"dynamicRegistration": false}
                },
                "textDocument": {
                    "synchronization": {"didSave": true, "dynamicRegistration": false},
                    "completion": {
                        "completionItem": {
                            "snippetSupport": true,
                            "documentationFormat": ["markdown", "plaintext"],
                            "insertReplaceSupport": true,
                            "deprecatedSupport": true
                        },
                        "completionItemKind": {"valueSet": (1..=25).collect::<Vec<u8>>()},
                        "contextSupport": true
                    },
                    "hover": {"contentFormat": ["markdown", "plaintext"]},
                    "definition": {"linkSupport": true},
                    "references": {},
                    "publishDiagnostics": {"relatedInformation": false},
                    "formatting": {},
                    "rename": {"prepareSupport": false}
                },
                "window": {"workDoneProgress": true}
            },
            "trace": "off"
        });
        let inner = self.inner.clone();
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.pending.lock().unwrap().insert(
            id,
            (
                "initialize".into(),
                Box::new(move |result| match result {
                    Ok(v) => {
                        *inner.caps.lock().unwrap() = Capabilities::parse(&v);
                        let (name, version) = v
                            .get("serverInfo")
                            .map(|s| {
                                (
                                    s.get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    s.get("version")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                )
                            })
                            .unwrap_or_default();
                        *inner.state.lock().unwrap() = State::Ready;
                        write_message(&inner, &notification("initialized", json!({})));
                        let queued: Vec<Vec<u8>> = std::mem::take(&mut inner.queue.lock().unwrap());
                        for m in queued {
                            write_message(&inner, &m);
                        }
                        emit(&inner, Event::Ready { name, version });
                    }
                    Err(e) => {
                        tracing::warn!(server = %inner.name, "initialize failed: {e}");
                        *inner.state.lock().unwrap() = State::Dead;
                        emit(
                            &inner,
                            Event::Exited {
                                error: Some(format!("initialize failed: {e}")),
                            },
                        );
                    }
                }),
            ),
        );
        write_message(&self.inner, &request(id, "initialize", params));
    }

    /// Send a request; `cb` runs on the reader thread with the result.
    pub fn request(&self, method: &str, params: Value, cb: Callback) {
        let state = self.state();
        if matches!(state, State::Dead | State::Stopping) {
            cb(Err(RpcError {
                code: -32099,
                message: format!("{} is not running", self.inner.name),
            }));
            return;
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(id, (method.to_string(), cb));
        let msg = request(id, method, params);
        if state == State::Starting {
            self.inner.queue.lock().unwrap().push(msg);
        } else {
            write_message(&self.inner, &msg);
        }
    }

    pub fn notify(&self, method: &str, params: Value) {
        match self.state() {
            State::Dead | State::Stopping => {}
            State::Starting => self
                .inner
                .queue
                .lock()
                .unwrap()
                .push(notification(method, params)),
            State::Ready => write_message(&self.inner, &notification(method, params)),
        }
    }

    // ---- document sync -------------------------------------------------

    pub fn did_open(&self, uri: &str, language_id: &str, version: i64, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {"uri": uri, "languageId": language_id, "version": version, "text": text}}),
        );
    }

    /// Full-text sync: the whole document after every (debounced) change.
    pub fn did_change_full(&self, uri: &str, version: i64, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]}),
        );
    }

    pub fn did_save(&self, uri: &str, text: Option<&str>) {
        let mut params = json!({"textDocument": {"uri": uri}});
        if let Some(t) = text {
            params["text"] = Value::String(t.to_string());
        }
        self.notify("textDocument/didSave", params);
    }

    pub fn did_close(&self, uri: &str) {
        self.notify(
            "textDocument/didClose",
            json!({"textDocument": {"uri": uri}}),
        );
    }

    // ---- requests --------------------------------------------------------

    pub fn completion(&self, uri: &str, pos: Position, trigger: Option<&str>, cb: Callback) {
        let context = match trigger {
            Some(t) => json!({"triggerKind": 2, "triggerCharacter": t}),
            None => json!({"triggerKind": 1}),
        };
        self.request(
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": pos, "context": context}),
            cb,
        );
    }

    pub fn hover(&self, uri: &str, pos: Position, cb: Callback) {
        self.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": pos}),
            cb,
        );
    }

    pub fn definition(&self, uri: &str, pos: Position, cb: Callback) {
        self.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": pos}),
            cb,
        );
    }

    pub fn references(&self, uri: &str, pos: Position, cb: Callback) {
        self.request(
            "textDocument/references",
            json!({"textDocument": {"uri": uri}, "position": pos, "context": {"includeDeclaration": true}}),
            cb,
        );
    }

    pub fn formatting(&self, uri: &str, tab_size: u32, insert_spaces: bool, cb: Callback) {
        self.request(
            "textDocument/formatting",
            json!({"textDocument": {"uri": uri}, "options": {"tabSize": tab_size, "insertSpaces": insert_spaces}}),
            cb,
        );
    }

    pub fn rename(&self, uri: &str, pos: Position, new_name: &str, cb: Callback) {
        self.request(
            "textDocument/rename",
            json!({"textDocument": {"uri": uri}, "position": pos, "newName": new_name}),
            cb,
        );
    }

    /// Ask the server to stop: `shutdown`, then `exit` once it answers;
    /// the process is killed if it is still there after a moment.
    pub fn shutdown(&self) {
        if matches!(self.state(), State::Dead | State::Stopping) {
            return;
        }
        let inner = self.inner.clone();
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        self.inner.pending.lock().unwrap().insert(
            id,
            (
                "shutdown".into(),
                Box::new(move |_| {
                    write_message(&inner, &notification("exit", Value::Null));
                }),
            ),
        );
        let was_ready = self.state() == State::Ready;
        *self.inner.state.lock().unwrap() = State::Stopping;
        if was_ready {
            write_message(&self.inner, &request(id, "shutdown", Value::Null));
        } else {
            // Never initialised: nothing to wait for.
            write_message(&self.inner, &notification("exit", Value::Null));
        }
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(Duration::from_millis(100));
                let mut child = inner.child.lock().unwrap();
                match child.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => continue,
                    _ => return,
                }
            }
            if let Some(c) = inner.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        });
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(c) = self.child.get_mut().unwrap().as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn request(id: i64, method: &str, params: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
        .unwrap_or_default()
}

fn notification(method: &str, params: Value) -> Vec<u8> {
    let mut m = json!({"jsonrpc": "2.0", "method": method});
    if !params.is_null() {
        m["params"] = params;
    }
    serde_json::to_vec(&m).unwrap_or_default()
}

fn response(id: &Value, result: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc": "2.0", "id": id, "result": result})).unwrap_or_default()
}

fn error_response(id: &Value, code: i64, message: &str) -> Vec<u8> {
    serde_json::to_vec(
        &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
    )
    .unwrap_or_default()
}

fn write_message(inner: &Inner, body: &[u8]) {
    let mut guard = inner.writer.lock().unwrap();
    let Some(w) = guard.as_mut() else { return };
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let ok = w
        .write_all(header.as_bytes())
        .and_then(|_| w.write_all(body))
        .and_then(|_| w.flush());
    if let Err(e) = ok {
        tracing::warn!(server = %inner.name, "write failed: {e}");
        *guard = None;
    }
}

fn emit(inner: &Inner, ev: Event) {
    let _ = inner.events.send(ev);
    (inner.wake)();
}

fn reader_loop(inner: Arc<Inner>, reader: impl Read) {
    let mut reader = BufReader::new(reader);
    while let Some(body) = read_frame(&mut reader) {
        let msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(server = %inner.name, "bad JSON from server: {e}");
                continue;
            }
        };
        handle_message(&inner, msg);
    }
    // The stream closed: fail what is still pending and report.
    let was_stopping = matches!(*inner.state.lock().unwrap(), State::Stopping);
    *inner.state.lock().unwrap() = State::Dead;
    *inner.writer.lock().unwrap() = None;
    let pending: Vec<(i64, (String, Callback))> = inner.pending.lock().unwrap().drain().collect();
    for (_, (_, cb)) in pending {
        cb(Err(RpcError {
            code: -32099,
            message: "server exited".into(),
        }));
    }
    let error = if was_stopping {
        None
    } else {
        let tail = inner.stderr_tail.lock().unwrap();
        Some(
            tail.iter()
                .rev()
                .find(|l| !l.trim().is_empty())
                .cloned()
                .unwrap_or_else(|| "server exited".into()),
        )
    };
    emit(&inner, Event::Exited { error });
}

/// One `Content-Length`-framed message, `None` at end of stream.
fn read_frame(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("content-length:"))
        {
            len = v.trim().parse().ok();
        }
    }
    let len = len?;
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

fn handle_message(inner: &Arc<Inner>, msg: Value) {
    let id = msg.get("id").cloned();
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .map(str::to_string);
    match (id, method) {
        (Some(id), None) => {
            // A response to one of ours.
            let Some(id_num) = id.as_i64() else { return };
            let Some((method, cb)) = inner.pending.lock().unwrap().remove(&id_num) else {
                return;
            };
            if let Some(err) = msg.get("error") {
                let e = RpcError {
                    code: err.get("code").and_then(Value::as_i64).unwrap_or(0),
                    message: err
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                        .to_string(),
                };
                tracing::debug!(server = %inner.name, "{method} failed: {e}");
                cb(Err(e));
            } else {
                cb(Ok(msg.get("result").cloned().unwrap_or(Value::Null)));
            }
        }
        (Some(id), Some(method)) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            server_request(inner, &id, &method, params);
        }
        (None, Some(method)) => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            server_notification(inner, &method, params);
        }
        _ => {}
    }
}

fn server_request(inner: &Arc<Inner>, id: &Value, method: &str, params: Value) {
    let reply = match method {
        "workspace/configuration" => {
            let n = params
                .get("items")
                .and_then(Value::as_array)
                .map(Vec::len)
                .unwrap_or(1);
            response(id, Value::Array(vec![Value::Null; n]))
        }
        "client/registerCapability"
        | "client/unregisterCapability"
        | "window/workDoneProgress/create"
        | "workspace/semanticTokens/refresh"
        | "workspace/inlayHint/refresh"
        | "workspace/codeLens/refresh"
        | "workspace/diagnostic/refresh"
        | "workspace/inlineValue/refresh"
        | "workspace/foldingRange/refresh" => response(id, Value::Null),
        "workspace/workspaceFolders" => response(id, Value::Null),
        "workspace/applyEdit" => {
            let label = params
                .get("label")
                .and_then(Value::as_str)
                .map(str::to_string);
            let edits = params
                .get("edit")
                .map(crate::types::workspace_edits)
                .unwrap_or_default();
            emit(inner, Event::ApplyEdit { label, edits });
            response(id, json!({"applied": true}))
        }
        "window/showMessageRequest" => {
            let text = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let kind = params.get("type").and_then(Value::as_u64).unwrap_or(3) as u8;
            emit(inner, Event::ShowMessage { kind, text });
            response(id, Value::Null)
        }
        "window/showDocument" => response(id, json!({"success": false})),
        _ => error_response(id, -32601, "method not found"),
    };
    write_message(inner, &reply);
}

fn server_notification(inner: &Arc<Inner>, method: &str, params: Value) {
    match method {
        "textDocument/publishDiagnostics" => {
            let uri = params
                .get("uri")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let version = params.get("version").and_then(Value::as_i64);
            let diagnostics: Vec<Diagnostic> = params
                .get("diagnostics")
                .and_then(|d| serde_json::from_value(d.clone()).ok())
                .unwrap_or_default();
            emit(
                inner,
                Event::Diagnostics {
                    uri,
                    version,
                    diagnostics,
                },
            );
        }
        "$/progress" => {
            let token = match params.get("token") {
                Some(Value::String(s)) => s.clone(),
                Some(v) => v.to_string(),
                None => String::new(),
            };
            let value = params.get("value").cloned().unwrap_or(Value::Null);
            let kind = value.get("kind").and_then(Value::as_str).unwrap_or("");
            let title = value
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let message = value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string);
            let percentage = value
                .get("percentage")
                .and_then(Value::as_u64)
                .map(|p| p as u32);
            emit(
                inner,
                Event::Progress {
                    token,
                    title,
                    message,
                    percentage,
                    done: kind == "end",
                },
            );
        }
        "window/logMessage" => {
            let text = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            tracing::debug!(server = %inner.name, "{text}");
            emit(inner, Event::Log { text });
        }
        "window/showMessage" => {
            let text = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let kind = params.get("type").and_then(Value::as_u64).unwrap_or(3) as u8;
            emit(inner, Event::ShowMessage { kind, text });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::PipeWriter;
    use std::sync::mpsc::channel;

    /// A fake server on the other end of two pipes: answers `initialize`,
    /// echoes a completion, publishes a diagnostic on open and asks for
    /// configuration.
    fn fake_server(mut from_client: impl Read + Send + 'static, mut to_client: PipeWriter) {
        std::thread::spawn(move || {
            let mut reader = BufReader::new(&mut from_client);
            let send = |w: &mut PipeWriter, v: Value| {
                let body = serde_json::to_vec(&v).unwrap();
                write!(w, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
                w.write_all(&body).unwrap();
                w.flush().unwrap();
            };
            while let Some(body) = read_frame(&mut reader) {
                let msg: Value = serde_json::from_slice(&body).unwrap();
                let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
                let id = msg.get("id").cloned();
                match method {
                    "initialize" => send(
                        &mut to_client,
                        json!({"jsonrpc": "2.0", "id": id, "result": {
                            "capabilities": {"completionProvider": {"triggerCharacters": [".", "::"]}, "hoverProvider": true,
                                             "positionEncoding": "utf-8", "textDocumentSync": 1},
                            "serverInfo": {"name": "fake", "version": "0.1"}}}),
                    ),
                    "initialized" => send(
                        &mut to_client,
                        json!({"jsonrpc": "2.0", "id": 77, "method": "workspace/configuration", "params": {"items": [{"section": "x"}, {"section": "y"}]}}),
                    ),
                    "textDocument/didOpen" => send(
                        &mut to_client,
                        json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {
                            "uri": msg["params"]["textDocument"]["uri"], "version": 1,
                            "diagnostics": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 2}}, "severity": 2, "message": "unused"}]}}),
                    ),
                    "textDocument/completion" => send(
                        &mut to_client,
                        json!({"jsonrpc": "2.0", "id": id, "result": {"isIncomplete": false, "items": [{"label": "alpha", "kind": 6}]}}),
                    ),
                    "textDocument/hover" => send(
                        &mut to_client,
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "boom"}}),
                    ),
                    "shutdown" => {
                        send(
                            &mut to_client,
                            json!({"jsonrpc": "2.0", "id": id, "result": null}),
                        );
                    }
                    "exit" => return,
                    _ => {
                        // The configuration answer arrives here as a response.
                        if msg.get("id").is_some() && msg.get("result").is_some() {
                            assert_eq!(msg["result"], json!([null, null]));
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn client_initialises_syncs_and_answers_requests() {
        let (server_reads, client_writes) = std::io::pipe().unwrap();
        let (client_reads, server_writes) = std::io::pipe().unwrap();
        fake_server(server_reads, server_writes);
        let (tx, rx) = channel();
        let client = Client::from_streams(
            "fake",
            client_reads,
            client_writes,
            None,
            Path::new("."),
            tx,
            Arc::new(|| {}),
        );
        // Queued before Ready, sent after.
        client.did_open("file:///a.rs", "rust", 1, "ab\n");
        let (ctx, crx) = channel();
        client.completion(
            "file:///a.rs",
            Position::new(0, 1),
            Some("."),
            Box::new(move |r| {
                let _ = ctx.send(r);
            }),
        );
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            first,
            Event::Ready {
                name: "fake".into(),
                version: "0.1".into()
            }
        );
        let caps = client.capabilities();
        assert!(caps.completion && caps.hover && !caps.definition);
        assert_eq!(caps.position_encoding, Encoding::Utf8);
        assert_eq!(caps.completion_triggers, vec![".", "::"]);
        let diag = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        match diag {
            Event::Diagnostics {
                uri, diagnostics, ..
            } => {
                assert_eq!(uri, "file:///a.rs");
                assert_eq!(diagnostics[0].message, "unused");
                assert_eq!(diagnostics[0].severity(), crate::types::Severity::Warning);
            }
            other => panic!("unexpected {other:?}"),
        }
        let completion = crx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        let (items, incomplete) =
            serde_json::from_value::<crate::types::CompletionResponse>(completion)
                .unwrap()
                .into_items();
        assert_eq!((items[0].label.as_str(), incomplete), ("alpha", false));
        let (htx, hrx) = channel();
        client.hover(
            "file:///a.rs",
            Position::new(0, 0),
            Box::new(move |r| {
                let _ = htx.send(r);
            }),
        );
        let err = hrx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert_eq!(err.message, "boom");
        client.shutdown();
        let exited = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(exited, Event::Exited { error: None });
        assert_eq!(client.state(), State::Dead);
        // Requests after the end fail at once.
        let (ltx, lrx) = channel();
        client.definition(
            "file:///a.rs",
            Position::new(0, 0),
            Box::new(move |r| {
                let _ = ltx.send(r);
            }),
        );
        assert!(lrx.recv_timeout(Duration::from_secs(1)).unwrap().is_err());
    }

    #[test]
    fn frames_are_read_with_any_header_case() {
        let data = b"content-length: 2\r\nOther: x\r\n\r\n{}Content-Length: 4\r\n\r\nnull";
        let mut r = BufReader::new(&data[..]);
        assert_eq!(read_frame(&mut r).unwrap(), b"{}");
        assert_eq!(read_frame(&mut r).unwrap(), b"null");
        assert!(read_frame(&mut r).is_none());
    }
}
