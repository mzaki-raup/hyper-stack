//! Debugging through a debug adapter (docs/tools/editor.md §12): launch
//! configurations from `.hyperstack/debug.json`, `.vscode/launch.json` or
//! generated for the project, breakpoints in the gutter, continue/step
//! controls, and a bottom panel with the console, variables, the call
//! stack and the breakpoint list. One `Debugger` per editor tab; at most
//! one session at a time. Nothing blocks: the adapter is started on a
//! thread and its replies and events are drained once per frame.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

use hs_core::Services;
use hs_core::tool::ToastLevel;
use hs_dap::adapters::{self, LaunchConfig, Origin, Override};
use hs_dap::client::{Client, Event, RequestError};
use hs_dap::types::{Evaluated, Scope, StackFrame, Thread, Variable, list};
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use serde_json::Value;

/// What the tool must do after a frame of debugger work.
#[derive(Debug, Clone, PartialEq)]
pub enum DebugAction {
    /// Show a file at a 1-based line (the stopped frame).
    GoTo {
        path: PathBuf,
        line: u32,
    },
    /// Open a file for editing (`debug.json`).
    Open(PathBuf),
    Toast(ToastLevel, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Building,
    Starting,
    Launching,
    Running,
    Stopped,
    Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelTab {
    Console,
    Variables,
    Stack,
    Breakpoints,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Line {
    Out(String),
    Err(String),
    Info(String),
    Input(String),
}

enum BuildMsg {
    Line(String),
    Done(Result<(), String>),
}

enum Reply {
    Initialized(Result<Value, RequestError>),
    Launched(Result<Value, RequestError>),
    Threads(Result<Value, RequestError>),
    Stack(Result<Value, RequestError>),
    Scopes(Result<Value, RequestError>),
    Variables(i64, Result<Value, RequestError>),
    Evaluate(String, Result<Value, RequestError>),
    Breakpoints(PathBuf, Result<Value, RequestError>),
    Control(&'static str, Result<Value, RequestError>),
}

struct Session {
    config: LaunchConfig,
    adapter_id: String,
    state: State,
    client: Option<Client>,
    pending_client: Option<mpsc::Receiver<Result<Client, String>>>,
    events: mpsc::Receiver<Event>,
    events_tx: mpsc::Sender<Event>,
    build: Option<mpsc::Receiver<BuildMsg>>,
    resolved: adapters::Resolved,
}

pub struct Debugger {
    /// 1-based lines per file.
    pub breakpoints: BTreeMap<PathBuf, BTreeSet<u32>>,
    verified: HashMap<(PathBuf, u32), bool>,
    overrides: Vec<Override>,
    session: Option<Session>,
    restart: Option<LaunchConfig>,
    last_config: Option<LaunchConfig>,
    configs: Vec<LaunchConfig>,
    pub picker_open: bool,
    pub panel_open: bool,
    tab: PanelTab,
    console: Vec<Line>,
    console_input: String,
    threads: Vec<Thread>,
    current_thread: Option<i64>,
    frames: Vec<StackFrame>,
    selected_frame: Option<usize>,
    scopes: Vec<Scope>,
    vars: HashMap<i64, Vec<Variable>>,
    expanded: HashSet<i64>,
    stopped_at: Option<(PathBuf, u32)>,
    actions: Vec<DebugAction>,
    replies: mpsc::Receiver<Reply>,
    reply_tx: mpsc::Sender<Reply>,
    wake: Arc<dyn Fn() + Send + Sync>,
    id: egui::Id,
}

const CONSOLE_CAP: usize = 5000;

impl Debugger {
    pub fn new(s: &Services, id: egui::Id) -> Self {
        let (reply_tx, replies) = mpsc::channel();
        let waker = s.runtime.waker();
        let mut me = Self {
            breakpoints: BTreeMap::new(),
            verified: HashMap::new(),
            overrides: Vec::new(),
            session: None,
            restart: None,
            last_config: None,
            configs: Vec::new(),
            picker_open: false,
            panel_open: false,
            tab: PanelTab::Console,
            console: Vec::new(),
            console_input: String::new(),
            threads: Vec::new(),
            current_thread: None,
            frames: Vec::new(),
            selected_frame: None,
            scopes: Vec::new(),
            vars: HashMap::new(),
            expanded: HashSet::new(),
            stopped_at: None,
            actions: Vec::new(),
            replies,
            reply_tx,
            wake: Arc::new(move || waker.request_repaint()),
            id: id.with("debug"),
        };
        me.read_settings(s);
        me
    }

    pub fn read_settings(&mut self, s: &Services) {
        let st = s.settings.read();
        self.overrides = st
            .editor
            .debug_adapters
            .iter()
            .map(|a| Override {
                id: a.id.clone(),
                command: a.command.clone(),
                args: a.args.split_whitespace().map(str::to_string).collect(),
                enabled: a.enabled,
            })
            .collect();
    }

    // ---- breakpoints ----------------------------------------------------

    /// Breakpoints as saved with the layout.
    pub fn saved_breakpoints(&self) -> Vec<(PathBuf, Vec<u32>)> {
        self.breakpoints
            .iter()
            .filter(|(_, l)| !l.is_empty())
            .map(|(p, l)| (p.clone(), l.iter().copied().collect()))
            .collect()
    }

    pub fn restore_breakpoints(&mut self, saved: &[(PathBuf, Vec<u32>)]) {
        for (p, lines) in saved {
            self.breakpoints
                .insert(p.clone(), lines.iter().copied().collect());
        }
    }

    /// Toggle the breakpoint on a 1-based line; the adapter is told at once.
    pub fn toggle_breakpoint(&mut self, path: &Path, line: u32) {
        let set = self.breakpoints.entry(path.to_path_buf()).or_default();
        if !set.remove(&line) {
            set.insert(line);
        }
        self.verified.retain(|(p, _), _| p != path);
        self.send_breakpoints(path);
    }

    fn send_breakpoints(&mut self, path: &Path) {
        let Some(client) = self.client() else { return };
        let lines: Vec<u32> = self
            .breakpoints
            .get(path)
            .map(|s| s.iter().copied().collect())
            .unwrap_or_default();
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        let p = path.to_path_buf();
        client.set_breakpoints(
            path,
            &lines,
            Box::new(move |r| {
                let _ = tx.send(Reply::Breakpoints(p, r));
                wake();
            }),
        );
    }

    fn send_all_breakpoints(&mut self) {
        let paths: Vec<PathBuf> = self.breakpoints.keys().cloned().collect();
        for p in paths {
            self.send_breakpoints(&p);
        }
    }

    /// Gutter marks for a file: (0-based line, verified) and the stopped
    /// line.
    pub fn marks(&self, path: &Path) -> (Vec<(usize, bool)>, Option<usize>) {
        let bps = self
            .breakpoints
            .get(path)
            .map(|set| {
                set.iter()
                    .map(|l| {
                        let v = self
                            .verified
                            .get(&(path.to_path_buf(), *l))
                            .copied()
                            .unwrap_or(self.session.is_none());
                        ((*l as usize).saturating_sub(1), v)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let exec = self
            .stopped_at
            .as_ref()
            .filter(|(p, _)| crate::same_path(p, path))
            .map(|(_, l)| (*l as usize).saturating_sub(1));
        (bps, exec)
    }

    // ---- sessions ----------------------------------------------------------

    pub fn state(&self) -> Option<State> {
        self.session.as_ref().map(|s| s.state)
    }

    pub fn active(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.state != State::Ended)
    }

    fn client(&self) -> Option<Client> {
        self.session
            .as_ref()
            .and_then(|s| s.client.clone())
            .filter(|c| !c.is_closed())
    }

    /// Reload the picker's configurations for a project root and file.
    pub fn refresh_configs(&mut self, root: Option<&Path>, file: Option<&Path>) {
        let root = root
            .map(Path::to_path_buf)
            .or_else(|| file.and_then(|f| f.parent().map(Path::to_path_buf)));
        self.configs = match root {
            Some(r) => adapters::configs(&r, file),
            None => Vec::new(),
        };
    }

    /// Start a session for a configuration (after a build step when it
    /// has one).
    pub fn start(&mut self, s: &Services, config: LaunchConfig) {
        if self.active() {
            self.actions.push(DebugAction::Toast(
                ToastLevel::Info,
                s.t("debug-already-running"),
            ));
            return;
        }
        let Some(resolved) = adapters::resolve(&config.adapter, &self.overrides) else {
            self.actions.push(DebugAction::Toast(
                ToastLevel::Error,
                s.i18n
                    .t_args("debug-no-adapter", &[("adapter", &config.adapter)]),
            ));
            return;
        };
        if resolved.path.is_none() {
            self.actions.push(DebugAction::Toast(
                ToastLevel::Warning,
                s.i18n.t_args(
                    "debug-adapter-missing",
                    &[
                        ("adapter", resolved.spec.id),
                        ("command", &resolved.command),
                        ("install", resolved.spec.install),
                    ],
                ),
            ));
            return;
        }
        self.console.clear();
        self.threads.clear();
        self.frames.clear();
        self.scopes.clear();
        self.vars.clear();
        self.expanded.clear();
        self.stopped_at = None;
        self.selected_frame = None;
        self.verified.clear();
        self.panel_open = true;
        self.tab = PanelTab::Console;
        self.last_config = Some(config.clone());
        let (events_tx, events) = mpsc::channel();
        let adapter_id = resolved.spec.id.to_string();
        let mut session = Session {
            config,
            adapter_id,
            state: State::Starting,
            client: None,
            pending_client: None,
            events,
            events_tx,
            build: None,
            resolved,
        };
        self.push(Line::Info(s.i18n.t_args(
            "debug-session-start",
            &[
                ("label", &session.config.label),
                ("adapter", &session.adapter_id),
            ],
        )));
        if let Some(step) = session.config.build.clone() {
            session.state = State::Building;
            self.push(Line::Info(format!(
                "$ {} {}",
                step.command,
                step.args.join(" ")
            )));
            session.build = Some(run_build(step, self.wake.clone()));
        } else {
            self.start_adapter(&mut session);
        }
        self.session = Some(session);
    }

    fn start_adapter(&mut self, session: &mut Session) {
        let exe = session
            .resolved
            .path
            .clone()
            .unwrap_or_else(|| PathBuf::from(&session.resolved.command));
        session.state = State::Starting;
        session.pending_client = Some(Client::start(
            &session.adapter_id,
            session.resolved.spec.transport,
            &exe,
            &session.resolved.args,
            &session.config.cwd,
            session.events_tx.clone(),
            self.wake.clone(),
        ));
    }

    /// Start the configuration used last (true), or nothing when there
    /// was none.
    pub fn start_last(&mut self, s: &Services) -> bool {
        match self.last_config.clone() {
            Some(cfg) => {
                self.start(s, cfg);
                true
            }
            None => false,
        }
    }

    pub fn stop(&mut self) {
        if let Some(c) = self.client() {
            c.disconnect(true);
        }
        if let Some(sess) = self.session.as_mut() {
            sess.state = State::Ended;
        }
        self.stopped_at = None;
    }

    pub fn restart(&mut self) {
        if let Some(cfg) = self.last_config.clone() {
            self.restart = Some(cfg);
            self.stop();
        }
    }

    fn control(&mut self, what: &'static str) {
        let Some(client) = self.client() else { return };
        let Some(thread) = self
            .current_thread
            .or_else(|| self.threads.first().map(|t| t.id))
        else {
            return;
        };
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        let cb: hs_dap::Callback = Box::new(move |r| {
            let _ = tx.send(Reply::Control(what, r));
            wake();
        });
        match what {
            "continue" => client.continue_(thread, cb),
            "next" => client.next(thread, cb),
            "stepIn" => client.step_in(thread, cb),
            "stepOut" => client.step_out(thread, cb),
            "pause" => client.pause(thread, cb),
            _ => {}
        }
        if what != "pause"
            && let Some(sess) = self.session.as_mut()
        {
            sess.state = State::Running;
            self.stopped_at = None;
        }
    }

    pub fn continue_(&mut self) {
        self.control("continue");
    }
    pub fn step_over(&mut self) {
        self.control("next");
    }
    pub fn step_into(&mut self) {
        self.control("stepIn");
    }
    pub fn step_out(&mut self) {
        self.control("stepOut");
    }
    pub fn pause(&mut self) {
        self.control("pause");
    }

    fn push(&mut self, line: Line) {
        self.console.push(line);
        if self.console.len() > CONSOLE_CAP {
            let extra = self.console.len() - CONSOLE_CAP;
            self.console.drain(0..extra);
        }
    }

    // ---- per frame ----------------------------------------------------------

    pub fn poll(&mut self, s: &Services) {
        // A restart waits for the previous session to end.
        if let Some(cfg) = self.restart.clone()
            && !self.active()
        {
            self.restart = None;
            self.session = None;
            self.start(s, cfg);
        }
        let Some(mut session) = self.session.take() else {
            return;
        };
        // Build output and completion.
        if let Some(rx) = &session.build {
            let mut done = None;
            while let Ok(m) = rx.try_recv() {
                match m {
                    BuildMsg::Line(l) => self.push(Line::Out(l)),
                    BuildMsg::Done(r) => done = Some(r),
                }
            }
            if let Some(r) = done {
                session.build = None;
                match r {
                    Ok(()) => self.start_adapter(&mut session),
                    Err(e) => {
                        self.push(Line::Err(e.clone()));
                        self.actions.push(DebugAction::Toast(ToastLevel::Error, e));
                        session.state = State::Ended;
                    }
                }
            }
        }
        // The adapter connected (or not).
        if let Some(rx) = &session.pending_client {
            match rx.try_recv() {
                Ok(Ok(client)) => {
                    session.pending_client = None;
                    session.state = State::Launching;
                    let tx = self.reply_tx.clone();
                    let wake = self.wake.clone();
                    client.initialize(
                        &session.adapter_id,
                        Box::new(move |r| {
                            let _ = tx.send(Reply::Initialized(r));
                            wake();
                        }),
                    );
                    session.client = Some(client);
                }
                Ok(Err(e)) => {
                    session.pending_client = None;
                    self.push(Line::Err(e.clone()));
                    self.actions.push(DebugAction::Toast(ToastLevel::Error, e));
                    session.state = State::Ended;
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    session.pending_client = None;
                    session.state = State::Ended;
                }
            }
        }
        // Adapter events.
        let mut events = Vec::new();
        while let Ok(ev) = session.events.try_recv() {
            events.push(ev);
        }
        self.session = Some(session);
        for ev in events {
            self.handle_event(s, ev);
        }
        while let Ok(r) = self.replies.try_recv() {
            self.handle_reply(s, r);
        }
    }

    fn handle_event(&mut self, s: &Services, ev: Event) {
        match ev {
            Event::Initialized => {
                self.send_all_breakpoints();
                if let Some(client) = self.client() {
                    let filters: Vec<String> = client
                        .capabilities()
                        .exception_breakpoint_filters
                        .iter()
                        .filter(|f| f.default)
                        .map(|f| f.filter.clone())
                        .collect();
                    client.set_exception_breakpoints(&filters, Box::new(|_| {}));
                    if client.capabilities().supports_configuration_done_request {
                        client.configuration_done(Box::new(|_| {}));
                    }
                }
                if let Some(sess) = self.session.as_mut()
                    && sess.state == State::Launching
                {
                    sess.state = State::Running;
                }
            }
            Event::Stopped(st) => {
                if let Some(sess) = self.session.as_mut() {
                    sess.state = State::Stopped;
                }
                self.current_thread = st.thread_id.or(self.current_thread);
                let mut why = st.reason.clone();
                if let Some(d) = &st.description {
                    why = d.clone();
                }
                if let Some(t) = &st.text {
                    why.push_str(": ");
                    why.push_str(t);
                }
                self.push(Line::Info(
                    s.i18n.t_args("debug-stopped", &[("reason", &why)]),
                ));
                if st.reason == "exception" {
                    self.actions
                        .push(DebugAction::Toast(ToastLevel::Warning, why));
                }
                self.vars.clear();
                self.scopes.clear();
                self.request_threads();
                if let Some(t) = self.current_thread {
                    self.request_stack(t);
                }
            }
            Event::Continued { .. } => {
                if let Some(sess) = self.session.as_mut() {
                    sess.state = State::Running;
                }
                self.stopped_at = None;
            }
            Event::Thread { reason, thread_id } => {
                if reason == "exited" {
                    self.threads.retain(|t| t.id != thread_id);
                }
            }
            Event::Output(o) => {
                let text = o.output.trim_end_matches(['\r', '\n']).to_string();
                if text.is_empty() {
                    return;
                }
                let line = match o.category.as_deref() {
                    Some("stderr") | Some("important") => Line::Err(text),
                    Some("console") => Line::Info(text),
                    _ => Line::Out(text),
                };
                self.push(line);
            }
            Event::Breakpoint { .. } => {}
            Event::Exited { code } => {
                self.push(Line::Info(s.i18n.t_args(
                    "debug-exited",
                    &[("code", &code.map(|c| c.to_string()).unwrap_or_default())],
                )));
            }
            Event::Terminated => {
                self.push(Line::Info(s.t("debug-terminated")));
                if let Some(sess) = self.session.as_mut() {
                    sess.state = State::Ended;
                }
                self.stopped_at = None;
                if let Some(c) = self.client() {
                    c.disconnect(false);
                }
            }
            Event::Unsupported(msg) => {
                self.actions
                    .push(DebugAction::Toast(ToastLevel::Warning, msg));
            }
            Event::Closed { error } => {
                let was_live = self.active();
                if let Some(sess) = self.session.as_mut() {
                    sess.state = State::Ended;
                }
                self.stopped_at = None;
                if let Some(e) = error.filter(|_| was_live) {
                    self.push(Line::Err(e.clone()));
                    self.actions
                        .push(DebugAction::Toast(ToastLevel::Warning, e));
                }
            }
        }
    }

    fn handle_reply(&mut self, s: &Services, r: Reply) {
        match r {
            Reply::Initialized(result) => match result {
                Ok(_) => {
                    let Some(sess) = self.session.as_ref() else {
                        return;
                    };
                    let Some(client) = self.client() else { return };
                    let args = adapters::launch_arguments(&sess.config);
                    let tx = self.reply_tx.clone();
                    let wake = self.wake.clone();
                    let cb: hs_dap::Callback = Box::new(move |r| {
                        let _ = tx.send(Reply::Launched(r));
                        wake();
                    });
                    if sess.config.request == "attach" {
                        client.attach(args, cb);
                    } else {
                        client.launch(args, cb);
                    }
                }
                Err(e) => self.fail(s, e.to_string()),
            },
            Reply::Launched(result) => {
                if let Err(e) = result {
                    self.fail(s, e.to_string());
                }
            }
            Reply::Threads(result) => {
                if let Ok(v) = result {
                    self.threads = list(&v, "threads");
                    if self.current_thread.is_none() {
                        self.current_thread = self.threads.first().map(|t| t.id);
                    }
                }
            }
            Reply::Stack(result) => {
                if let Ok(v) = result {
                    self.frames = list(&v, "stackFrames");
                    self.select_frame(0);
                }
            }
            Reply::Scopes(result) => {
                if let Ok(v) = result {
                    self.scopes = list(&v, "scopes");
                    let refs: Vec<i64> = self
                        .scopes
                        .iter()
                        .filter(|sc| !sc.expensive)
                        .map(|sc| sc.variables_reference)
                        .collect();
                    for r in refs {
                        self.expanded.insert(r);
                        self.request_variables(r);
                    }
                }
            }
            Reply::Variables(reference, result) => {
                if let Ok(v) = result {
                    self.vars.insert(reference, list(&v, "variables"));
                }
            }
            Reply::Evaluate(expr, result) => match result {
                Ok(v) => {
                    let e: Evaluated = serde_json::from_value(v).unwrap_or_default();
                    let _ = expr;
                    self.push(Line::Out(e.result));
                }
                Err(e) => self.push(Line::Err(e.message)),
            },
            Reply::Breakpoints(path, result) => {
                if let Ok(v) = result {
                    let bps: Vec<hs_dap::types::Breakpoint> = list(&v, "breakpoints");
                    let lines: Vec<u32> = self
                        .breakpoints
                        .get(&path)
                        .map(|s| s.iter().copied().collect())
                        .unwrap_or_default();
                    for (i, bp) in bps.iter().enumerate() {
                        let line = bp.line.or_else(|| lines.get(i).copied());
                        if let Some(l) = line {
                            self.verified.insert((path.clone(), l), bp.verified);
                        }
                    }
                }
            }
            Reply::Control(what, result) => {
                if let Err(e) = result {
                    self.push(Line::Err(format!("{what}: {}", e.message)));
                }
            }
        }
    }

    fn fail(&mut self, s: &Services, error: String) {
        self.push(Line::Err(error.clone()));
        self.actions.push(DebugAction::Toast(
            ToastLevel::Error,
            s.i18n.t_args("debug-failed", &[("error", &error)]),
        ));
        self.stop();
    }

    fn request_threads(&mut self) {
        let Some(client) = self.client() else { return };
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.threads(Box::new(move |r| {
            let _ = tx.send(Reply::Threads(r));
            wake();
        }));
    }

    fn request_stack(&mut self, thread: i64) {
        let Some(client) = self.client() else { return };
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.stack_trace(
            thread,
            Box::new(move |r| {
                let _ = tx.send(Reply::Stack(r));
                wake();
            }),
        );
    }

    fn request_variables(&mut self, reference: i64) {
        let Some(client) = self.client() else { return };
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.variables(
            reference,
            Box::new(move |r| {
                let _ = tx.send(Reply::Variables(reference, r));
                wake();
            }),
        );
    }

    /// Show a frame: its scopes and variables, and its source line.
    fn select_frame(&mut self, index: usize) {
        let Some(frame) = self.frames.get(index).cloned() else {
            return;
        };
        self.selected_frame = Some(index);
        self.scopes.clear();
        self.vars.clear();
        if let Some(p) = frame.path()
            && frame.line > 0
        {
            let path = crate::normalize_path(PathBuf::from(p));
            self.stopped_at = Some((path.clone(), frame.line));
            self.actions.push(DebugAction::GoTo {
                path,
                line: frame.line,
            });
        }
        let Some(client) = self.client() else { return };
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        client.scopes(
            frame.id,
            Box::new(move |r| {
                let _ = tx.send(Reply::Scopes(r));
                wake();
            }),
        );
    }

    fn evaluate(&mut self, expr: String) {
        let Some(client) = self.client() else {
            self.push(Line::Err("no session".into()));
            return;
        };
        let frame = self
            .selected_frame
            .and_then(|i| self.frames.get(i))
            .map(|f| f.id);
        self.push(Line::Input(expr.clone()));
        let tx = self.reply_tx.clone();
        let wake = self.wake.clone();
        let e = expr.clone();
        client.evaluate(
            &expr,
            frame,
            "repl",
            Box::new(move |r| {
                let _ = tx.send(Reply::Evaluate(e, r));
                wake();
            }),
        );
    }

    pub fn take_actions(&mut self) -> Vec<DebugAction> {
        std::mem::take(&mut self.actions)
    }

    // ---- UI -------------------------------------------------------------------

    /// The Debug button or, during a session, the controls. Draw it in the
    /// editor toolbar.
    pub fn toolbar(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        root: Option<&Path>,
        file: Option<&Path>,
    ) {
        let state = self.state().filter(|st| *st != State::Ended);
        match state {
            None => {
                let r = ui
                    .button(format!("{} {}", ph::BUG, s.t("debug-button")))
                    .tip(s.t("debug-button-tip"));
                if r.clicked() {
                    self.refresh_configs(root, file);
                    self.picker_open = !self.picker_open;
                }
                if self.picker_open {
                    self.picker(ui, s, theme, r.rect, root, file);
                }
            }
            Some(st) => {
                let stopped = st == State::Stopped;
                let running = st == State::Running;
                let label = match st {
                    State::Building => s.t("debug-state-building"),
                    State::Starting => s.t("debug-state-starting"),
                    State::Launching => s.t("debug-state-launching"),
                    State::Running => s.t("debug-state-running"),
                    State::Stopped => s.t("debug-state-stopped"),
                    State::Ended => String::new(),
                };
                ui.label(
                    egui::RichText::new(format!("{} {label}", ph::BUG)).color(if stopped {
                        theme.warning
                    } else {
                        theme.text_muted
                    }),
                );
                if ui
                    .add_enabled(
                        stopped,
                        egui::Button::new(format!("{} {}", ph::PLAY, s.t("debug-continue"))),
                    )
                    .tip(s.t("debug-continue-tip"))
                    .clicked()
                {
                    self.continue_();
                }
                if ui
                    .add_enabled(
                        running,
                        egui::Button::new(format!("{} {}", ph::PAUSE, s.t("debug-pause"))),
                    )
                    .tip(s.t("debug-pause-tip"))
                    .clicked()
                {
                    self.pause();
                }
                if ui
                    .add_enabled(
                        stopped,
                        egui::Button::new(format!(
                            "{} {}",
                            ph::ARROW_BEND_DOWN_RIGHT,
                            s.t("debug-step-over")
                        )),
                    )
                    .tip(s.t("debug-step-over-tip"))
                    .clicked()
                {
                    self.step_over();
                }
                if ui
                    .add_enabled(
                        stopped,
                        egui::Button::new(format!(
                            "{} {}",
                            ph::ARROW_LINE_DOWN,
                            s.t("debug-step-into")
                        )),
                    )
                    .tip(s.t("debug-step-into-tip"))
                    .clicked()
                {
                    self.step_into();
                }
                if ui
                    .add_enabled(
                        stopped,
                        egui::Button::new(format!(
                            "{} {}",
                            ph::ARROW_LINE_UP,
                            s.t("debug-step-out")
                        )),
                    )
                    .tip(s.t("debug-step-out-tip"))
                    .clicked()
                {
                    self.step_out();
                }
                if ui
                    .button(format!(
                        "{} {}",
                        ph::ARROW_COUNTER_CLOCKWISE,
                        s.t("debug-restart")
                    ))
                    .tip(s.t("debug-restart-tip"))
                    .clicked()
                {
                    self.restart();
                }
                if ui
                    .button(
                        egui::RichText::new(format!("{} {}", ph::STOP, s.t("debug-stop")))
                            .color(theme.error),
                    )
                    .tip(s.t("debug-stop-tip"))
                    .clicked()
                {
                    self.stop();
                }
                if !running && !stopped {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(300));
                }
            }
        }
        if ui
            .add(
                egui::Button::new(format!("{} {}", ph::TERMINAL_WINDOW, s.t("debug-panel")))
                    .selected(self.panel_open),
            )
            .tip(s.t("debug-panel-tip"))
            .clicked()
        {
            self.panel_open = !self.panel_open;
        }
    }

    fn picker(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        anchor: egui::Rect,
        root: Option<&Path>,
        file: Option<&Path>,
    ) {
        let id = self.id.with("picker");
        let mut start: Option<LaunchConfig> = None;
        let mut close = false;
        let mut edit = false;
        let area = egui::Area::new(id)
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(anchor.left(), anchor.bottom() + 4.0))
            .constrain(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.surface)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .show(ui, |ui| {
                        ui.set_min_width(360.0);
                        ui.set_max_width(520.0);
                        ui.label(
                            egui::RichText::new(s.t("debug-picker-title"))
                                .strong()
                                .color(theme.text_muted),
                        );
                        if self.configs.is_empty() {
                            ui.label(s.t("debug-picker-none"));
                        }
                        for cfg in &self.configs {
                            let origin = match cfg.origin {
                                Origin::Project => "debug.json",
                                Origin::VsCode => "launch.json",
                                Origin::Generated => "",
                            };
                            let text = if origin.is_empty() {
                                format!("{}  ·  {}", cfg.label, cfg.adapter)
                            } else {
                                format!("{}  ·  {}  ·  {origin}", cfg.label, cfg.adapter)
                            };
                            let found = adapters::resolve(&cfg.adapter, &self.overrides)
                                .is_some_and(|r| r.path.is_some());
                            let rich = if found {
                                egui::RichText::new(text)
                            } else {
                                egui::RichText::new(format!(
                                    "{text}  ({})",
                                    s.t("debug-adapter-not-installed")
                                ))
                                .color(theme.text_muted)
                            };
                            if ui.add(egui::Button::new(rich).frame(false)).clicked() {
                                start = Some(cfg.clone());
                            }
                        }
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::new(format!(
                                    "{} {}",
                                    ph::PENCIL_SIMPLE,
                                    s.t("debug-edit-config")
                                ))
                                .frame(false),
                            )
                            .tip(s.t("debug-edit-config-tip"))
                            .clicked()
                        {
                            edit = true;
                        }
                    });
            });
        let clicked_outside = ui.input(|i| i.pointer.any_click())
            && !ui
                .input(|i| i.pointer.interact_pos())
                .is_some_and(|p| area.response.rect.contains(p) || anchor.contains(p));
        if clicked_outside || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if let Some(cfg) = start {
            self.picker_open = false;
            self.start(s, cfg);
        } else if edit {
            self.picker_open = false;
            if let Some(r) = root
                .map(Path::to_path_buf)
                .or_else(|| file.and_then(|f| f.parent().map(Path::to_path_buf)))
            {
                let path = r.join(".hyperstack").join("debug.json");
                if !path.exists() {
                    let _ = std::fs::create_dir_all(r.join(".hyperstack"));
                    let _ = std::fs::write(&path, adapters::template(&r, file));
                }
                self.actions.push(DebugAction::Open(path));
            }
        } else if close {
            self.picker_open = false;
        }
    }

    /// The bottom panel: console, variables, call stack, breakpoints.
    pub fn panel_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        root: Option<&Path>,
    ) {
        let mut close = false;
        ui.horizontal(|ui| {
            for (tab, key) in [
                (PanelTab::Console, "debug-tab-console"),
                (PanelTab::Variables, "debug-tab-variables"),
                (PanelTab::Stack, "debug-tab-stack"),
                (PanelTab::Breakpoints, "debug-tab-breakpoints"),
            ] {
                if ui
                    .add(egui::Button::selectable(self.tab == tab, s.t(key)))
                    .clicked()
                {
                    self.tab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(ph::X).clicked() {
                    close = true;
                }
                if let Some(sess) = &self.session {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  ·  {}",
                            sess.config.label, sess.adapter_id
                        ))
                        .small()
                        .color(theme.text_muted),
                    );
                }
            });
        });
        ui.separator();
        match self.tab {
            PanelTab::Console => self.console_ui(ui, s, theme),
            PanelTab::Variables => self.variables_ui(ui, s, theme),
            PanelTab::Stack => self.stack_ui(ui, s, theme, root),
            PanelTab::Breakpoints => self.breakpoints_ui(ui, s, theme, root),
        }
        if close {
            self.panel_open = false;
        }
    }

    fn console_ui(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        let input_h = 26.0;
        let avail = ui.available_height() - input_h;
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("console"))
            .max_height(avail.max(40.0))
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                let font = egui::FontId::new(
                    s.settings.read().editor.font_size,
                    hs_ui::fonts::editor_family(),
                );
                for line in &self.console {
                    let (text, colour) = match line {
                        Line::Out(t) => (t.as_str(), theme.text),
                        Line::Err(t) => (t.as_str(), theme.error),
                        Line::Info(t) => (t.as_str(), theme.text_muted),
                        Line::Input(t) => (t.as_str(), theme.accent),
                    };
                    let prefix = if matches!(line, Line::Input(_)) {
                        "> "
                    } else {
                        ""
                    };
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{prefix}{text}"))
                                .font(font.clone())
                                .color(colour),
                        )
                        .wrap(),
                    );
                }
            });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(">").color(theme.accent));
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.console_input)
                    .hint_text(s.t("debug-console-hint"))
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !self.console_input.trim().is_empty()
            {
                let expr = std::mem::take(&mut self.console_input);
                self.evaluate(expr.trim().to_string());
                r.request_focus();
            }
        });
    }

    fn variables_ui(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme) {
        if self.scopes.is_empty() {
            ui.label(egui::RichText::new(s.t("debug-variables-none")).color(theme.text_muted));
            return;
        }
        let mut to_load: Vec<i64> = Vec::new();
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("vars"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let scopes = self.scopes.clone();
                for scope in scopes {
                    let open = self.expanded.contains(&scope.variables_reference);
                    let header = ui.add(
                        egui::Button::new(format!(
                            "{} {}",
                            if open {
                                ph::CARET_DOWN
                            } else {
                                ph::CARET_RIGHT
                            },
                            scope.name
                        ))
                        .frame(false),
                    );
                    if header.clicked() {
                        if open {
                            self.expanded.remove(&scope.variables_reference);
                        } else {
                            self.expanded.insert(scope.variables_reference);
                            if !self.vars.contains_key(&scope.variables_reference) {
                                to_load.push(scope.variables_reference);
                            }
                        }
                    }
                    if open {
                        self.variable_rows(ui, theme, scope.variables_reference, 1, &mut to_load);
                    }
                }
            });
        for r in to_load {
            self.request_variables(r);
        }
    }

    fn variable_rows(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        reference: i64,
        depth: usize,
        to_load: &mut Vec<i64>,
    ) {
        let Some(vars) = self.vars.get(&reference).cloned() else {
            ui.add_space(2.0);
            ui.label(egui::RichText::new("…").color(theme.text_muted));
            return;
        };
        for v in vars {
            let has_children = v.variables_reference > 0;
            let open = has_children && self.expanded.contains(&v.variables_reference);
            ui.horizontal(|ui| {
                ui.add_space(depth as f32 * 16.0);
                let caret = if !has_children {
                    "  "
                } else if open {
                    ph::CARET_DOWN
                } else {
                    ph::CARET_RIGHT
                };
                let r = ui.add(
                    egui::Button::new(
                        egui::RichText::new(format!("{caret} {}", v.name)).monospace(),
                    )
                    .frame(false),
                );
                if r.clicked() && has_children {
                    if open {
                        self.expanded.remove(&v.variables_reference);
                    } else {
                        self.expanded.insert(v.variables_reference);
                        if !self.vars.contains_key(&v.variables_reference) {
                            to_load.push(v.variables_reference);
                        }
                    }
                }
                ui.add(
                    egui::Label::new(egui::RichText::new(&v.value).monospace().color(theme.text))
                        .truncate()
                        .selectable(true),
                );
                if let Some(t) = &v.r#type {
                    ui.label(egui::RichText::new(t).small().color(theme.text_muted));
                }
            });
            if open {
                self.variable_rows(ui, theme, v.variables_reference, depth + 1, to_load);
            }
        }
    }

    fn stack_ui(&mut self, ui: &mut egui::Ui, s: &Services, theme: &Theme, root: Option<&Path>) {
        if self.frames.is_empty() {
            ui.label(egui::RichText::new(s.t("debug-stack-none")).color(theme.text_muted));
            return;
        }
        let mut pick: Option<usize> = None;
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("stack"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.threads.len() > 1
                    && let Some(t) = self
                        .current_thread
                        .and_then(|id| self.threads.iter().find(|t| t.id == id))
                {
                    ui.label(egui::RichText::new(&t.name).small().color(theme.text_muted));
                }
                for (i, f) in self.frames.iter().enumerate() {
                    let loc = f
                        .path()
                        .map(|p| {
                            let p = Path::new(p);
                            let shown = root
                                .and_then(|r| p.strip_prefix(r).ok())
                                .unwrap_or(p)
                                .to_string_lossy()
                                .to_string();
                            format!("{shown}:{}", f.line)
                        })
                        .unwrap_or_default();
                    let subtle = f.presentation_hint.as_deref() == Some("subtle");
                    let text = format!("{}    {loc}", f.name);
                    let rich = if subtle {
                        egui::RichText::new(text).color(theme.text_muted)
                    } else {
                        egui::RichText::new(text)
                    };
                    if ui
                        .add(egui::Button::selectable(
                            self.selected_frame == Some(i),
                            rich,
                        ))
                        .clicked()
                    {
                        pick = Some(i);
                    }
                }
            });
        if let Some(i) = pick {
            self.select_frame(i);
        }
    }

    fn breakpoints_ui(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        theme: &Theme,
        root: Option<&Path>,
    ) {
        let all: Vec<(PathBuf, u32)> = self
            .breakpoints
            .iter()
            .flat_map(|(p, lines)| lines.iter().map(move |l| (p.clone(), *l)))
            .collect();
        if all.is_empty() {
            ui.label(egui::RichText::new(s.t("debug-breakpoints-none")).color(theme.text_muted));
            return;
        }
        let mut remove: Option<(PathBuf, u32)> = None;
        let mut go: Option<(PathBuf, u32)> = None;
        egui::ScrollArea::vertical()
            .id_salt(self.id.with("bps"))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (p, l) in &all {
                    ui.horizontal(|ui| {
                        if ui
                            .small_button(ph::X)
                            .tip(s.t("debug-breakpoint-remove"))
                            .clicked()
                        {
                            remove = Some((p.clone(), *l));
                        }
                        let verified = self
                            .verified
                            .get(&(p.clone(), *l))
                            .copied()
                            .unwrap_or(self.session.is_none());
                        ui.colored_label(
                            if verified {
                                theme.error
                            } else {
                                theme.text_muted
                            },
                            ph::CIRCLE,
                        );
                        let shown = root
                            .and_then(|r| p.strip_prefix(r).ok())
                            .unwrap_or(p)
                            .to_string_lossy()
                            .to_string();
                        if ui
                            .add(egui::Button::new(format!("{shown}:{l}")).frame(false))
                            .clicked()
                        {
                            go = Some((p.clone(), *l));
                        }
                    });
                }
            });
        if let Some((p, l)) = remove {
            self.toggle_breakpoint(&p, l);
        }
        if let Some((path, line)) = go {
            self.actions.push(DebugAction::GoTo { path, line });
        }
    }
}

impl Drop for Debugger {
    fn drop(&mut self) {
        if let Some(c) = self.client() {
            c.disconnect(true);
        }
    }
}

/// Run a build step on a thread, streaming its output.
fn run_build(
    step: adapters::BuildStep,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> mpsc::Receiver<BuildMsg> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("debug-build".into())
        .spawn(move || {
            let mut cmd = std::process::Command::new(&step.command);
            cmd.args(&step.args)
                .current_dir(&step.cwd)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x0800_0000);
            }
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(BuildMsg::Done(Err(format!("{}: {e}", step.command))));
                    wake();
                    return;
                }
            };
            let out = child.stdout.take();
            let err = child.stderr.take();
            let tx2 = tx.clone();
            let w2 = wake.clone();
            let t = std::thread::spawn(move || {
                if let Some(o) = out {
                    for line in BufReader::new(o).lines().map_while(Result::ok) {
                        let _ = tx2.send(BuildMsg::Line(line));
                        w2();
                    }
                }
            });
            if let Some(e) = err {
                for line in BufReader::new(e).lines().map_while(Result::ok) {
                    let _ = tx.send(BuildMsg::Line(line));
                    wake();
                }
            }
            let _ = t.join();
            let result = match child.wait() {
                Ok(st) if st.success() => Ok(()),
                Ok(st) => Err(format!("{} failed ({st})", step.command)),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(BuildMsg::Done(result));
            wake();
        })
        .expect("spawn build thread");
    rx
}
