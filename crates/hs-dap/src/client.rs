//! One debug adapter: DAP messages (`request` / `response` / `event`) with
//! `Content-Length` framing over the adapter's stdio or a TCP connection.
//! A reader thread parses what the adapter sends; writes go to the
//! adapter under a lock.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::adapters::Transport;
use crate::types::{Breakpoint, Capabilities, Output, Stopped};

/// Runs on the reader thread with the response body, or the failure.
pub type Callback = Box<dyn FnOnce(Result<Value, RequestError>) + Send + 'static>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestError {
    pub command: String,
    pub message: String,
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.command, self.message)
    }
}

/// What the adapter tells the editor on its own.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The adapter accepts breakpoints now (`initialized`).
    Initialized,
    Stopped(Stopped),
    Continued {
        thread_id: Option<i64>,
        all_threads: bool,
    },
    Thread {
        reason: String,
        thread_id: i64,
    },
    Output(Output),
    /// A breakpoint changed (verified, moved).
    Breakpoint {
        reason: String,
        breakpoint: Breakpoint,
    },
    /// The debuggee ended with this exit code.
    Exited {
        code: Option<i64>,
    },
    /// The session is over (the adapter may still be running).
    Terminated,
    /// The adapter asked for something the client does not do
    /// (`runInTerminal`): shown to the user.
    Unsupported(String),
    /// The connection or the process is gone.
    Closed {
        error: Option<String>,
    },
}

struct Inner {
    name: String,
    child: Mutex<Option<Child>>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    next_seq: AtomicI64,
    pending: Mutex<HashMap<i64, (String, Callback)>>,
    closed: Mutex<bool>,
    caps: Mutex<Capabilities>,
    stderr_tail: Mutex<Vec<String>>,
    events: mpsc::Sender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

/// A connected debug adapter.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

/// How long to wait for a TCP adapter to open its port.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

impl Client {
    /// Start an adapter and connect to it, on a thread: the result arrives
    /// on the returned receiver (the UI polls it). For a TCP adapter the
    /// `{port}` placeholder in `args` is replaced by a free local port.
    pub fn start(
        name: &str,
        transport: Transport,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> mpsc::Receiver<Result<Client, String>> {
        let (tx, rx) = mpsc::channel();
        let name = name.to_string();
        let port = match transport {
            Transport::Tcp => free_port(),
            Transport::Stdio => 0,
        };
        let args: Vec<String> = args
            .iter()
            .map(|a| a.replace("{port}", &port.to_string()))
            .collect();
        let exe = exe.to_path_buf();
        let cwd = cwd.to_path_buf();
        std::thread::Builder::new()
            .name(format!("dap-{name}-start"))
            .spawn(move || {
                let result = Self::start_blocking(
                    &name,
                    &exe,
                    &args,
                    &cwd,
                    transport,
                    port,
                    events,
                    wake.clone(),
                );
                let _ = tx.send(result);
                wake();
            })
            .expect("spawn start thread");
        rx
    }

    #[allow(clippy::too_many_arguments)]
    fn start_blocking(
        name: &str,
        exe: &Path,
        args: &[String],
        cwd: &Path,
        transport: Transport,
        port: u16,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Client, String> {
        let mut cmd = Command::new(exe);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
        let stderr = child.stderr.take();
        let stderr_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        if let Some(err) = stderr {
            let tail = stderr_tail.clone();
            let n = name.to_string();
            std::thread::spawn(move || {
                for line in BufReader::new(err).lines().map_while(Result::ok) {
                    tracing::debug!(adapter = %n, "{line}");
                    let mut t = tail.lock().unwrap();
                    t.push(line);
                    if t.len() > 30 {
                        t.remove(0);
                    }
                }
            });
        }
        match transport {
            Transport::Stdio => {
                let stdin = child.stdin.take().expect("piped stdin");
                let stdout = child.stdout.take().expect("piped stdout");
                Ok(Self::from_streams(
                    name,
                    stdout,
                    stdin,
                    Some(child),
                    stderr_tail,
                    events,
                    wake,
                ))
            }
            Transport::Tcp => {
                // The adapter's own stdout is only chatter in this mode.
                if let Some(out) = child.stdout.take() {
                    let n = name.to_string();
                    std::thread::spawn(move || {
                        for line in BufReader::new(out).lines().map_while(Result::ok) {
                            tracing::debug!(adapter = %n, "{line}");
                        }
                    });
                }
                let addr = format!("127.0.0.1:{port}");
                let start = Instant::now();
                loop {
                    match TcpStream::connect(&addr) {
                        Ok(stream) => {
                            let _ = stream.set_nodelay(true);
                            let reader = stream.try_clone().map_err(|e| e.to_string())?;
                            return Ok(Self::from_streams(
                                name,
                                reader,
                                stream,
                                Some(child),
                                stderr_tail,
                                events,
                                wake,
                            ));
                        }
                        Err(_) if start.elapsed() < CONNECT_TIMEOUT => {
                            if let Ok(Some(status)) = child.try_wait() {
                                let tail = stderr_tail.lock().unwrap().join("\n");
                                return Err(format!(
                                    "{name} exited ({status}) before listening on {addr}\n{tail}"
                                ));
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        Err(e) => {
                            let _ = child.kill();
                            return Err(format!("{name}: no connection on {addr}: {e}"));
                        }
                    }
                }
            }
        }
    }

    /// A client over any pair of streams (tests use an in-process pipe).
    pub fn from_streams(
        name: &str,
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        child: Option<Child>,
        stderr_tail: Arc<Mutex<Vec<String>>>,
        events: mpsc::Sender<Event>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let inner = Arc::new(Inner {
            name: name.to_string(),
            child: Mutex::new(child),
            writer: Mutex::new(Some(Box::new(writer))),
            next_seq: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            closed: Mutex::new(false),
            caps: Mutex::new(Capabilities::default()),
            stderr_tail: Mutex::new(Vec::new()),
            events,
            wake,
        });
        // Share the tail the start thread already collects.
        *inner.stderr_tail.lock().unwrap() = stderr_tail.lock().unwrap().clone();
        let client = Self { inner };
        let reader_inner = client.inner.clone();
        std::thread::Builder::new()
            .name(format!("dap-{name}"))
            .spawn(move || reader_loop(reader_inner, reader, stderr_tail))
            .expect("spawn reader thread");
        client
    }

    pub fn name(&self) -> &str {
        &self.inner.name
    }

    pub fn is_closed(&self) -> bool {
        *self.inner.closed.lock().unwrap()
    }

    pub fn capabilities(&self) -> Capabilities {
        self.inner.caps.lock().unwrap().clone()
    }

    /// Send a request; `cb` runs on the reader thread with the response
    /// body (`Value::Null` when there is none).
    pub fn request(&self, command: &str, arguments: Value, cb: Callback) {
        if self.is_closed() {
            cb(Err(RequestError {
                command: command.into(),
                message: format!("{} is not connected", self.inner.name),
            }));
            return;
        }
        let seq = self.inner.next_seq.fetch_add(1, Ordering::Relaxed);
        self.inner
            .pending
            .lock()
            .unwrap()
            .insert(seq, (command.to_string(), cb));
        let mut msg = json!({"seq": seq, "type": "request", "command": command});
        if !arguments.is_null() {
            msg["arguments"] = arguments;
        }
        write_message(&self.inner, &serde_json::to_vec(&msg).unwrap_or_default());
    }

    /// Fire a request whose answer nobody waits for.
    pub fn send(&self, command: &str, arguments: Value) {
        self.request(command, arguments, Box::new(|_| {}));
    }

    // ---- typed requests ---------------------------------------------------

    /// `initialize`; the capabilities are kept on the client.
    pub fn initialize(&self, adapter_id: &str, cb: Callback) {
        let inner = self.inner.clone();
        self.request(
            "initialize",
            json!({
                "clientID": "hyperstack",
                "clientName": "HyperStack",
                "adapterID": adapter_id,
                "locale": "en",
                "linesStartAt1": true,
                "columnsStartAt1": true,
                "pathFormat": "path",
                "supportsVariableType": true,
                "supportsRunInTerminalRequest": false,
                "supportsProgressReporting": false,
                "supportsInvalidatedEvent": false
            }),
            Box::new(move |r| {
                if let Ok(v) = &r {
                    *inner.caps.lock().unwrap() =
                        serde_json::from_value(v.clone()).unwrap_or_default();
                }
                cb(r);
            }),
        );
    }

    pub fn launch(&self, arguments: Value, cb: Callback) {
        self.request("launch", arguments, cb);
    }

    pub fn attach(&self, arguments: Value, cb: Callback) {
        self.request("attach", arguments, cb);
    }

    /// Replace the breakpoints of one file (1-based lines).
    pub fn set_breakpoints(&self, path: &Path, lines: &[u32], cb: Callback) {
        let bps: Vec<Value> = lines.iter().map(|l| json!({"line": l})).collect();
        self.request(
            "setBreakpoints",
            json!({
                "source": {"path": path.to_string_lossy(), "name": path.file_name().map(|n| n.to_string_lossy().to_string())},
                "breakpoints": bps,
                "lines": lines,
                "sourceModified": false
            }),
            cb,
        );
    }

    pub fn set_exception_breakpoints(&self, filters: &[String], cb: Callback) {
        self.request("setExceptionBreakpoints", json!({"filters": filters}), cb);
    }

    pub fn configuration_done(&self, cb: Callback) {
        self.request("configurationDone", Value::Null, cb);
    }

    pub fn threads(&self, cb: Callback) {
        self.request("threads", Value::Null, cb);
    }

    pub fn stack_trace(&self, thread_id: i64, cb: Callback) {
        self.request(
            "stackTrace",
            json!({"threadId": thread_id, "startFrame": 0, "levels": 50}),
            cb,
        );
    }

    pub fn scopes(&self, frame_id: i64, cb: Callback) {
        self.request("scopes", json!({"frameId": frame_id}), cb);
    }

    pub fn variables(&self, reference: i64, cb: Callback) {
        self.request("variables", json!({"variablesReference": reference}), cb);
    }

    pub fn continue_(&self, thread_id: i64, cb: Callback) {
        self.request("continue", json!({"threadId": thread_id}), cb);
    }

    pub fn next(&self, thread_id: i64, cb: Callback) {
        self.request(
            "next",
            json!({"threadId": thread_id, "granularity": "line"}),
            cb,
        );
    }

    pub fn step_in(&self, thread_id: i64, cb: Callback) {
        self.request(
            "stepIn",
            json!({"threadId": thread_id, "granularity": "line"}),
            cb,
        );
    }

    pub fn step_out(&self, thread_id: i64, cb: Callback) {
        self.request(
            "stepOut",
            json!({"threadId": thread_id, "granularity": "line"}),
            cb,
        );
    }

    pub fn pause(&self, thread_id: i64, cb: Callback) {
        self.request("pause", json!({"threadId": thread_id}), cb);
    }

    pub fn evaluate(&self, expression: &str, frame_id: Option<i64>, context: &str, cb: Callback) {
        let mut args = json!({"expression": expression, "context": context});
        if let Some(f) = frame_id {
            args["frameId"] = json!(f);
        }
        self.request("evaluate", args, cb);
    }

    /// Stop the debuggee (when the adapter supports `terminate`) and the
    /// session; the adapter process is killed if it lingers.
    pub fn disconnect(&self, terminate_debuggee: bool) {
        if self.is_closed() {
            return;
        }
        let supports_terminate = self.capabilities().supports_terminate_request;
        let inner = self.inner.clone();
        let finish = move || {
            inner.clone().request_raw(
                "disconnect",
                json!({"terminateDebuggee": terminate_debuggee}),
                Box::new(|_| {}),
            );
            let inner2 = inner.clone();
            std::thread::spawn(move || {
                for _ in 0..30 {
                    std::thread::sleep(Duration::from_millis(100));
                    let mut child = inner2.child.lock().unwrap();
                    match child.as_mut().map(|c| c.try_wait()) {
                        Some(Ok(None)) => continue,
                        _ => return,
                    }
                }
                if let Some(c) = inner2.child.lock().unwrap().as_mut() {
                    let _ = c.kill();
                }
            });
        };
        if terminate_debuggee && supports_terminate {
            self.request("terminate", json!({}), Box::new(move |_| finish()));
        } else {
            finish();
        }
    }
}

impl Inner {
    fn request_raw(self: Arc<Self>, command: &str, arguments: Value, cb: Callback) {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        self.pending
            .lock()
            .unwrap()
            .insert(seq, (command.to_string(), cb));
        let msg =
            json!({"seq": seq, "type": "request", "command": command, "arguments": arguments});
        write_message(&self, &serde_json::to_vec(&msg).unwrap_or_default());
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

/// An unused local TCP port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .unwrap_or(4711)
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
        tracing::warn!(adapter = %inner.name, "write failed: {e}");
        *guard = None;
    }
}

fn emit(inner: &Inner, ev: Event) {
    let _ = inner.events.send(ev);
    (inner.wake)();
}

fn reader_loop(inner: Arc<Inner>, reader: impl Read, stderr_tail: Arc<Mutex<Vec<String>>>) {
    let mut reader = BufReader::new(reader);
    while let Some(body) = read_frame(&mut reader) {
        let msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(adapter = %inner.name, "bad JSON from adapter: {e}");
                continue;
            }
        };
        handle_message(&inner, msg);
    }
    *inner.closed.lock().unwrap() = true;
    *inner.writer.lock().unwrap() = None;
    let pending: Vec<(i64, (String, Callback))> = inner.pending.lock().unwrap().drain().collect();
    for (_, (command, cb)) in pending {
        cb(Err(RequestError {
            command,
            message: "adapter closed the connection".into(),
        }));
    }
    let tail = stderr_tail.lock().unwrap();
    let error = tail
        .iter()
        .rev()
        .find(|l| l.to_ascii_lowercase().contains("error"))
        .cloned();
    emit(&inner, Event::Closed { error });
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
    match msg.get("type").and_then(Value::as_str) {
        Some("response") => {
            let Some(seq) = msg.get("request_seq").and_then(Value::as_i64) else {
                return;
            };
            let Some((command, cb)) = inner.pending.lock().unwrap().remove(&seq) else {
                return;
            };
            if msg.get("success").and_then(Value::as_bool).unwrap_or(false) {
                cb(Ok(msg.get("body").cloned().unwrap_or(Value::Null)));
            } else {
                let message = msg
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        msg.get("body")
                            .and_then(|b| b.get("error"))
                            .and_then(|e| e.get("format"))
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| "request failed".into());
                tracing::debug!(adapter = %inner.name, "{command} failed: {message}");
                cb(Err(RequestError { command, message }));
            }
        }
        Some("event") => {
            let body = msg.get("body").cloned().unwrap_or(Value::Null);
            let event = msg.get("event").and_then(Value::as_str).unwrap_or("");
            adapter_event(inner, event, body);
        }
        Some("request") => {
            // Reverse requests: nothing is supported, say so politely.
            let seq = msg.get("seq").cloned().unwrap_or(Value::Null);
            let command = msg
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let reply = json!({
                "seq": inner.next_seq.fetch_add(1, Ordering::Relaxed),
                "type": "response",
                "request_seq": seq,
                "success": false,
                "command": command,
                "message": "not supported by this client"
            });
            write_message(inner, &serde_json::to_vec(&reply).unwrap_or_default());
            if command == "runInTerminal" {
                emit(
                    inner,
                    Event::Unsupported(
                        "the adapter asked for a terminal; set the console to the internal one"
                            .into(),
                    ),
                );
            }
        }
        _ => {}
    }
}

fn adapter_event(inner: &Arc<Inner>, event: &str, body: Value) {
    match event {
        "initialized" => emit(inner, Event::Initialized),
        "stopped" => {
            let s: Stopped = serde_json::from_value(body).unwrap_or_default();
            emit(inner, Event::Stopped(s));
        }
        "continued" => emit(
            inner,
            Event::Continued {
                thread_id: body.get("threadId").and_then(Value::as_i64),
                all_threads: body
                    .get("allThreadsContinued")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            },
        ),
        "thread" => emit(
            inner,
            Event::Thread {
                reason: body
                    .get("reason")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                thread_id: body.get("threadId").and_then(Value::as_i64).unwrap_or(0),
            },
        ),
        "output" => {
            let o: Output = serde_json::from_value(body).unwrap_or_default();
            if o.category.as_deref() != Some("telemetry") {
                emit(inner, Event::Output(o));
            }
        }
        "breakpoint" => {
            if let Some(bp) = body
                .get("breakpoint")
                .and_then(|b| serde_json::from_value::<Breakpoint>(b.clone()).ok())
            {
                emit(
                    inner,
                    Event::Breakpoint {
                        reason: body
                            .get("reason")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        breakpoint: bp,
                    },
                );
            }
        }
        "exited" => emit(
            inner,
            Event::Exited {
                code: body.get("exitCode").and_then(Value::as_i64),
            },
        ),
        "terminated" => emit(inner, Event::Terminated),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::PipeWriter;
    use std::sync::mpsc::channel;

    /// A fake adapter: answers initialize with capabilities, sends
    /// `initialized`, stops at the first breakpoint after
    /// `configurationDone`, answers threads/stackTrace, and asks for a
    /// terminal (which the client refuses).
    fn fake_adapter(mut from_client: impl Read + Send + 'static, mut to_client: PipeWriter) {
        std::thread::spawn(move || {
            let mut reader = BufReader::new(&mut from_client);
            let mut seq = 100;
            let mut send = |w: &mut PipeWriter, mut v: Value| {
                seq += 1;
                v["seq"] = json!(seq);
                let body = serde_json::to_vec(&v).unwrap();
                write!(w, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
                w.write_all(&body).unwrap();
                w.flush().unwrap();
            };
            while let Some(body) = read_frame(&mut reader) {
                let msg: Value = serde_json::from_slice(&body).unwrap();
                let command = msg.get("command").and_then(Value::as_str).unwrap_or("");
                let rs = msg.get("seq").cloned().unwrap_or(Value::Null);
                let ok = |command: &str, body: Value| json!({"type": "response", "request_seq": rs, "success": true, "command": command, "body": body});
                match command {
                    "initialize" => {
                        send(
                            &mut to_client,
                            ok(
                                "initialize",
                                json!({"supportsConfigurationDoneRequest": true, "supportsTerminateRequest": true}),
                            ),
                        );
                        send(
                            &mut to_client,
                            json!({"type": "event", "event": "initialized"}),
                        );
                    }
                    "launch" => {
                        send(
                            &mut to_client,
                            json!({"type": "request", "command": "runInTerminal", "arguments": {"args": ["x"]}}),
                        );
                        send(&mut to_client, ok("launch", Value::Null));
                    }
                    "setBreakpoints" => send(
                        &mut to_client,
                        ok(
                            "setBreakpoints",
                            json!({"breakpoints": [{"verified": true, "line": 3}]}),
                        ),
                    ),
                    "configurationDone" => {
                        send(&mut to_client, ok("configurationDone", Value::Null));
                        send(
                            &mut to_client,
                            json!({"type": "event", "event": "output", "body": {"category": "stdout", "output": "hello\n"}}),
                        );
                        send(
                            &mut to_client,
                            json!({"type": "event", "event": "stopped", "body": {"reason": "breakpoint", "threadId": 1, "allThreadsStopped": true}}),
                        );
                    }
                    "threads" => send(
                        &mut to_client,
                        ok("threads", json!({"threads": [{"id": 1, "name": "main"}]})),
                    ),
                    "stackTrace" => send(
                        &mut to_client,
                        ok(
                            "stackTrace",
                            json!({"stackFrames": [{"id": 7, "name": "main", "line": 3, "column": 1, "source": {"path": "/a.py"}}], "totalFrames": 1}),
                        ),
                    ),
                    "evaluate" => send(
                        &mut to_client,
                        json!({"type": "response", "request_seq": rs, "success": false, "command": "evaluate", "message": "name 'x' is not defined"}),
                    ),
                    "terminate" => {
                        send(&mut to_client, ok("terminate", Value::Null));
                        send(
                            &mut to_client,
                            json!({"type": "event", "event": "terminated"}),
                        );
                    }
                    "disconnect" => {
                        send(&mut to_client, ok("disconnect", Value::Null));
                        return;
                    }
                    _ => {
                        if msg.get("type").and_then(Value::as_str) == Some("response") {
                            assert_eq!(msg["success"], json!(false), "runInTerminal is refused");
                        }
                    }
                }
            }
        });
    }

    #[test]
    fn a_session_runs_through_initialize_launch_breakpoint_and_stop() {
        let (adapter_reads, client_writes) = std::io::pipe().unwrap();
        let (client_reads, adapter_writes) = std::io::pipe().unwrap();
        fake_adapter(adapter_reads, adapter_writes);
        let (tx, rx) = channel();
        let client = Client::from_streams(
            "fake",
            client_reads,
            client_writes,
            None,
            Arc::new(Mutex::new(Vec::new())),
            tx,
            Arc::new(|| {}),
        );
        let (itx, irx) = channel();
        client.initialize(
            "fake",
            Box::new(move |r| {
                let _ = itx.send(r.is_ok());
            }),
        );
        assert!(irx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(client.capabilities().supports_configuration_done_request);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Initialized
        );
        client.launch(json!({"program": "a.py"}), Box::new(|_| {}));
        let unsupported = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(unsupported, Event::Unsupported(_)));
        let (btx, brx) = channel();
        client.set_breakpoints(
            Path::new("/a.py"),
            &[3],
            Box::new(move |r| {
                let _ = btx.send(r);
            }),
        );
        let bps: Vec<Breakpoint> = crate::types::list(
            &brx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            "breakpoints",
        );
        assert!(bps[0].verified);
        client.configuration_done(Box::new(|_| {}));
        let out = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            out,
            Event::Output(Output {
                category: Some("stdout".into()),
                output: "hello\n".into()
            })
        );
        match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
            Event::Stopped(s) => {
                assert_eq!((s.reason.as_str(), s.thread_id), ("breakpoint", Some(1)))
            }
            other => panic!("unexpected {other:?}"),
        }
        let (stx, srx) = channel();
        client.stack_trace(
            1,
            Box::new(move |r| {
                let _ = stx.send(r);
            }),
        );
        let frames: Vec<crate::types::StackFrame> = crate::types::list(
            &srx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap(),
            "stackFrames",
        );
        assert_eq!((frames[0].line, frames[0].path()), (3, Some("/a.py")));
        let (etx, erx) = channel();
        client.evaluate(
            "x",
            Some(7),
            "repl",
            Box::new(move |r| {
                let _ = etx.send(r);
            }),
        );
        let err = erx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert!(err.message.contains("not defined"));
        client.disconnect(true);
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Event::Terminated
        );
        let closed = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(closed, Event::Closed { .. }));
        assert!(client.is_closed());
    }
}
