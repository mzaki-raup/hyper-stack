//! The tab: sidebar key tree, and Keys / Console / Info in the main area
//! (docs/tools/redis.md §3). Everything the UI needs arrives through
//! [`crate::runner::Shared`]; nothing here talks to the server directly.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use hs_core::Services;
use hs_core::tool::{
    Accept, Action, CommandDescriptor, OpenRequest, Printable, ToastLevel, Tool, ToolCategory,
    ToolContext, ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::client::{ConnectConfig, KeyType, PAGE, STRING_PREVIEW, ValuePage};
use crate::console::{self, Class};
use crate::keytree;
use crate::profile::{ProfileError, RedisFields};
use crate::runner::{self, Cmd, ScanFlag, Shared, Status, Write};
use crate::value;

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "redis",
    title_key: "tool-redis",
    icon: ph::LIGHTNING,
    category: ToolCategory::Data,
    accepts: &[Accept::Connection],
};

pub struct RedisFactory;

impl ToolFactory for RedisFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = RedisTool::new(state);
        // A restored tab opens its connection itself; nothing calls `open`
        // for it, and without this it would sit on "Connecting…" for ever.
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores. Never a secret: the connection id is enough.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    pub pattern: String,
    pub tab: Tab,
    pub open_key: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Keys,
    Console,
    Info,
}

/// A dialog waiting for an answer.
enum Popup {
    /// Delete the keys listed, named by what the user picked.
    Delete {
        keys: Vec<String>,
        prefix: Option<String>,
        typed: String,
    },
    Rename {
        key: String,
        to: String,
    },
    Ttl {
        key: String,
        text: String,
    },
    NewKey {
        key: String,
        kind: KeyType,
        value: String,
    },
    /// A console command that needs a yes.
    Command {
        line: String,
        args: Vec<String>,
        typed: String,
    },
    AddMember {
        key: String,
        kind: KeyType,
        field: String,
        value: String,
        score: String,
    },
}

pub struct RedisTool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    cmd: Option<mpsc::Sender<Cmd>>,
    scan_flag: Option<ScanFlag>,
    conn_name: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    read_only_profile: bool,
    protected: bool,
    writes_unlocked: bool,
    env_id: Option<String>,
    env_name: String,
    fields: RedisFields,
    /// Expanded group paths in the key tree.
    open_groups: std::collections::HashSet<String>,
    /// The string editor's buffer, and whether it differs from the server.
    editor: String,
    editing: bool,
    popup: Option<Popup>,
    console_input: String,
    console_history: Vec<String>,
    history_at: Option<usize>,
    table: TableState,
    /// Rows of the value table, rebuilt when the page changes.
    rows: Vec<Vec<String>>,
    /// (key, when the TTL was read, what it was): the header counts down
    /// between reads rather than standing still.
    ttl_read: Option<(String, std::time::Instant, i64)>,
    /// Review-only assistant (ADR-019): it reads the open key and the
    /// console, and its commands can be inserted but never run by it.
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl RedisTool {
    fn new(state: State) -> RedisTool {
        RedisTool {
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            cmd: None,
            scan_flag: None,
            conn_name: String::new(),
            conn_error: None,
            waiting_for_vault: false,
            read_only_profile: false,
            protected: false,
            writes_unlocked: false,
            env_id: None,
            env_name: String::new(),
            fields: RedisFields::default(),
            open_groups: Default::default(),
            editor: String::new(),
            editing: false,
            popup: None,
            console_input: String::new(),
            console_history: Vec::new(),
            history_at: None,
            table: TableState::default(),
            rows: Vec::new(),
            ttl_read: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        }
    }

    /// Writes are refused on a read-only profile, and on a protected
    /// environment until the tab unlocks them.
    fn read_only(&self) -> bool {
        self.read_only_profile || (self.protected && !self.writes_unlocked)
    }

    fn connect(&mut self, s: &Services) {
        self.cmd = None; // closes the old session
        self.shared = Arc::new(Mutex::new(Shared::default()));
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.popup = None;
        self.open_groups.clear();
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("redis-no-profile"));
                return;
            };
            (
                RedisFields::from_table(&c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.read_only_profile = read_only;
        self.env_id = env_id.clone();
        self.fields = fields.clone();
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
        let lookup = {
            let envs = s.environments.read().clone();
            let env_id = env_id.clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let cfg = match fields.resolve(self.read_only(), s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault = e == ProfileError::VaultLocked;
                self.conn_error = Some(match e {
                    ProfileError::VaultLocked => s.t("redis-vault-locked"),
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("redis-missing-vars", &[("names", &v.join(", "))]),
                    ProfileError::Secret(e) => e,
                    ProfileError::UnsupportedMode(m) => {
                        s.i18n.t_args("redis-mode-unsupported", &[("mode", &m)])
                    }
                });
                return;
            }
        };
        // SSH tunnel: ask the Tunnels service for a local port first.
        let tunnel = (!fields.tunnel_ssh.is_empty()).then(|| {
            s.tunnels
                .ensure_local_forward(&fields.tunnel_ssh, &cfg.host, cfg.port)
        });
        let future = async move {
            let mut cfg: ConnectConfig = cfg;
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
        // One line per connection, so a support log says which server a tab
        // was on. Never the password: that is a vault reference here anyway.
        tracing::info!(
            "redis: connecting {} to {}:{}/{}",
            self.conn_name,
            fields.host,
            fields.port,
            fields.db
        );
        let (tx, flag) = runner::spawn(
            &s.runtime,
            future,
            self.shared.clone(),
            fields.delimiter.clone(),
        );
        self.cmd = Some(tx);
        self.scan_flag = Some(flag);
        self.rescan();
    }

    fn send(&mut self, cmd: Cmd) -> bool {
        match &self.cmd {
            Some(tx) => tx.try_send(cmd).is_ok(),
            None => false,
        }
    }

    fn rescan(&mut self) {
        let pattern = keytree::match_pattern(&self.state.pattern);
        // Stop whatever is running before starting another walk.
        if let Some(f) = &self.scan_flag {
            f.store(false, Ordering::Relaxed);
        }
        self.send(Cmd::Scan { pattern });
    }

    fn open_key(&mut self, key: &str) {
        self.state.open_key = Some(key.to_string());
        self.state.tab = Tab::Keys;
        self.editing = false;
        self.table = TableState::default();
        self.send(Cmd::Open {
            key: key.to_string(),
        });
    }

    /// Ask before a write the guards cover, or run it now.
    fn guarded_write(&mut self, w: Write, cx: &mut ToolContext) {
        if self.read_only() {
            let key = if self.read_only_profile {
                "redis-refused-read-only"
            } else {
                "redis-refused-protected"
            };
            cx.toast(ToastLevel::Error, cx.services.t(key));
            return;
        }
        self.send(Cmd::Run(w));
    }
}

/// A small icon button with the label as its tooltip: the shared helper takes
/// a size and an active flag, which no toolbar here needs.
fn tool_button(ui: &mut egui::Ui, theme: &Theme, icon: &str, label: &str) -> egui::Response {
    let r = form::icon_button(ui, theme, icon, 16.0, false);
    r.tip(label)
}

/// How wide the key list is beside the value pane.
const KEY_LIST_WIDTH: f32 = 300.0;

/// Rows for the table that shows a collection page.
fn page_rows(page: &ValuePage) -> (Vec<ColumnSpec>, Vec<Vec<String>>) {
    let short = |b: &[u8]| {
        let d = value::decode(b);
        let mut t = d.text.replace('\n', " ");
        if t.chars().count() > 300 {
            t = t.chars().take(300).collect::<String>() + "…";
        }
        t
    };
    match page {
        ValuePage::Pairs { items, .. } => (
            vec![
                ColumnSpec::new("field", 220.0),
                ColumnSpec::new("value", 520.0),
            ],
            items
                .iter()
                .map(|(f, v)| vec![f.clone(), short(v)])
                .collect(),
        ),
        ValuePage::Items { items, start, .. } => (
            vec![
                ColumnSpec::new("#", 80.0).numeric(),
                ColumnSpec::new("value", 660.0),
            ],
            items
                .iter()
                .enumerate()
                .map(|(i, v)| vec![(start + i as u64).to_string(), short(v)])
                .collect(),
        ),
        ValuePage::Scored { items, .. } => (
            vec![
                ColumnSpec::new("score", 120.0).numeric(),
                ColumnSpec::new("member", 620.0),
            ],
            items
                .iter()
                .map(|(m, s)| vec![format!("{s}"), short(m)])
                .collect(),
        ),
        ValuePage::Entries { items } => (
            vec![
                ColumnSpec::new("id", 200.0),
                ColumnSpec::new("fields", 560.0),
            ],
            items
                .iter()
                .map(|(id, fields)| {
                    let text = fields
                        .iter()
                        .map(|(f, v)| format!("{f}={}", short(v)))
                        .collect::<Vec<_>>()
                        .join("  ");
                    vec![id.clone(), text]
                })
                .collect(),
        ),
        _ => (Vec::new(), Vec::new()),
    }
}

/// Parse `INFO` into its sections for the dashboard.
pub fn info_sections(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim_end();
        if let Some(name) = line.strip_prefix("# ") {
            out.push((name.to_string(), Vec::new()));
        } else if let Some((k, v)) = line.split_once(':') {
            if out.is_empty() {
                out.push((String::from("server"), Vec::new()));
            }
            if let Some(last) = out.last_mut() {
                last.1.push((k.to_string(), v.to_string()));
            }
        }
    }
    out.retain(|(_, rows)| !rows.is_empty());
    out
}

impl Tool for RedisTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> std::borrow::Cow<'_, str> {
        if self.conn_name.is_empty() {
            std::borrow::Cow::Borrowed("Redis")
        } else {
            std::borrow::Cow::Borrowed(&self.conn_name)
        }
    }

    fn environment(&self) -> Option<String> {
        self.env_id.clone()
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                self.state.conn_id = Some(id);
                self.connect(cx.services);
                true
            }
            _ => false,
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: None,
        };
        vec![
            c("redis.scan", "Redis: Scan Keys"),
            c("redis.stop", "Redis: Stop the Scan"),
            c("redis.console", "Redis: Console"),
            c("redis.info", "Redis: Server Info"),
            c("redis.new_key", "Redis: New Key"),
            c("redis.reconnect", "Redis: Reconnect"),
            c("redis.ai", "Redis: AI Assistant"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "redis.scan" => {
                self.rescan();
                true
            }
            "redis.stop" => {
                if let Some(f) = &self.scan_flag {
                    f.store(false, Ordering::Relaxed);
                }
                true
            }
            "redis.console" => {
                self.state.tab = Tab::Console;
                true
            }
            "redis.ai" => {
                self.assistant.toggle("Redis");
                true
            }
            "redis.info" => {
                self.state.tab = Tab::Info;
                self.send(Cmd::RefreshInfo);
                true
            }
            "redis.new_key" => {
                self.popup = Some(Popup::NewKey {
                    key: String::new(),
                    kind: KeyType::String,
                    value: String::new(),
                });
                true
            }
            "redis.reconnect" => {
                self.connect(cx.services);
                true
            }
            _ => false,
        }
    }

    fn printable(&self) -> Option<Printable> {
        let sh = self.shared.lock();
        let info = sh.open_key.as_ref()?;
        let (title, body) = match &sh.page {
            Some(ValuePage::Text { bytes, .. }) => (
                info.key.clone(),
                hs_core::html::pre(&value::decode(bytes).text),
            ),
            Some(p) => {
                let (columns, rows) = page_rows(p);
                let headers: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
                (info.key.clone(), hs_core::html::table(&headers, &rows))
            }
            None => return None,
        };
        Some(Printable {
            title,
            subtitle: format!("Redis {}", self.conn_name),
            html: body,
        })
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        // The same header, list rows and add button as the SSH, S3 and
        // database sidebars; the Sidebar toggle hides all of it.
        if form::sidebar_header(
            ui,
            &theme,
            &s.t("redis-connections"),
            &[(ph::PLUS, &s.t("redis-new-profile"))],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:connections".into(),
            });
        }
        let profiles: Vec<(String, String, String, bool)> = {
            let conns = s.connections.read();
            conns
                .of_kind("redis")
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        RedisFields::from_table(&c.fields).label(),
                        c.pinned,
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("redis-no-profiles"));
            return true;
        }
        egui::ScrollArea::vertical()
            .id_salt("redis-connections")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("redis-profiles");
                let mut open_profile: Option<(String, bool)> = None;
                let mut pin: Option<(String, bool)> = None;
                for (i, (id, name, label, is_pinned)) in profiles.iter().enumerate() {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            selected,
                            ph::LIGHTNING,
                            name,
                            Some(label),
                            *is_pinned,
                            &s.t("conn-pin-home"),
                            &s.t("conn-unpin-home"),
                        );
                        if row.response.tip(s.t("redis-open-profile")).clicked() {
                            open_profile = Some((id.clone(), selected));
                        }
                        if let Some(p) = row.pin {
                            pin = Some((id.clone(), p));
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((id, p)) = pin
                    && let Err(e) = s.connections.write().set_pinned(&id, p)
                {
                    cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    if let Err(e) = s.connections.write().reorder(&["redis"], from, to) {
                        cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
                if let Some((id, selected)) = open_profile
                    && !selected
                {
                    // The shell decides which tab this lands in: the one
                    // already bound to the profile, this one if it holds no
                    // connection, or a new one.
                    cx.open("redis", Some(OpenRequest::Connection { id }));
                }
            });
        true
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        self.assistant
            .last_answer()
            .map(|a| vec![hs_core::tool::Speakable::new("Assistant's last answer", a)])
            .unwrap_or_default()
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        // Icon plus label, the way the other tools' toolbars read.
        for (tab, icon, key) in [
            (Tab::Keys, ph::KEY, "redis-tab-keys"),
            (Tab::Console, ph::TERMINAL_WINDOW, "redis-tab-console"),
            (Tab::Info, ph::INFO, "redis-tab-info"),
        ] {
            let label = format!("{icon} {}", s.t(key));
            if ui
                .add(egui::Button::selectable(self.state.tab == tab, label))
                .clicked()
            {
                self.state.tab = tab;
                if tab == Tab::Info {
                    self.send(Cmd::RefreshInfo);
                }
            }
        }
        if self.protected && !self.writes_unlocked {
            ui.separator();
            let label = s.i18n.t_args("redis-protected", &[("env", &self.env_name)]);
            if ui
                .button(label)
                .on_hover_text(s.t("redis-unlock-hint"))
                .clicked()
            {
                self.writes_unlocked = true;
            }
        }
        let status = self.shared.lock().status.clone();
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .on_hover_text(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("Redis");
            }
            if let Some(Status::Ready(info)) = status {
                // "Redis 8.6.3": the server's name is a name.
                let mut flavour = info.flavour.clone();
                if let Some(first) = flavour.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                ui.label(
                    egui::RichText::new(format!("{flavour} {}", info.version))
                        .small()
                        .color(theme.text_muted),
                );
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // The assistant takes its column first, so the tool lays out in
        // what is left rather than into whatever happens to be spare.
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        self.body(&mut content, cx);
        self.assistant_column(ui, assistant_rect, cx);
    }
}

impl RedisTool {
    fn body(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        // Drain what the runner wants to say.
        let messages: Vec<(bool, String)> = {
            let mut sh = self.shared.lock();
            std::mem::take(&mut sh.messages)
        };
        for (ok, text) in messages {
            cx.toast(
                if ok {
                    ToastLevel::Success
                } else {
                    ToastLevel::Error
                },
                text,
            );
        }
        if self.state.conn_id.is_none() {
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("redis-pick-hint"));
            return;
        }
        // Read the status into a local first: the guard would otherwise
        // live across the `self.connect` call in the Failed arm.
        let status = self.shared.lock().status.clone();
        match status {
            Some(Status::Connecting) | None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("redis-connecting"));
                });
                return;
            }
            Some(Status::Failed(e)) => {
                form::note(ui, &theme, ph::WARNING, &e);
                if ui.button(s.t("redis-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            Some(Status::Ready(_)) => {}
        }
        self.dialogs(ui, cx);
        match self.state.tab {
            Tab::Keys => self.keys_tab(ui, cx),
            Tab::Console => self.console_tab(ui, cx),
            Tab::Info => self.info_tab(ui, cx),
        }
    }

    /// Everything the assistant is told about this tab, and the column it
    /// is drawn in. The context is the open key and the last console
    /// command: what is on screen, nothing more.
    fn assistant_column(
        &mut self,
        outer: &mut egui::Ui,
        panel: Option<egui::Rect>,
        cx: &mut ToolContext,
    ) {
        if !self.assistant.open {
            return;
        }
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (status, open_key, last) = {
            let sh = self.shared.lock();
            (
                sh.status.clone(),
                sh.open_key.clone(),
                sh.console.last().map(|e| {
                    let body: Vec<String> =
                        e.lines.iter().take(20).map(|l| l.text.clone()).collect();
                    format!("{}\n{}", e.command, body.join("\n"))
                }),
            )
        };
        let server = match &status {
            Some(Status::Ready(info)) => format!("{} {}", info.flavour, info.version),
            _ => "(unknown)".into(),
        };
        let key = open_key
            .map(|k| {
                format!(
                    "Key: {}\nType: {}\nTTL: {}\nEncoding: {}\nSize: {}",
                    k.key,
                    k.kind.map(|t| t.badge().to_string()).unwrap_or_default(),
                    k.ttl_ms
                        .map(|ms| format!("{} ms", ms))
                        .unwrap_or_else(|| "none".into()),
                    k.encoding,
                    k.len.map(|n| n.to_string()).unwrap_or_default(),
                )
            })
            .unwrap_or_else(|| "(no key open)".into());
        let context = format!(
            "Connection: {}\nServer: {server}\nDatabase: {}\nKey pattern: {}\n\n{key}\n\nLast console command:\n{}",
            self.conn_name,
            self.shared.lock().db,
            self.state.pattern,
            last.unwrap_or_else(|| "(none)".into()),
        );
        self.assistant.set_context("Redis key", context);
        if self.assistant.quick_actions.is_empty() {
            use hs_ui::ai_assistant::QuickAction;
            self.assistant.quick_actions = vec![
                QuickAction::new(
                    s,
                    "ai-qa-redis-explain-key",
                    "ai-qa-redis-explain-key-prompt",
                    "",
                ),
                QuickAction::new(s, "ai-qa-redis-command", "ai-qa-redis-command-prompt", ""),
                QuickAction::new(s, "ai-qa-redis-memory", "ai-qa-redis-memory-prompt", ""),
            ];
        }
        let Some(panel) = panel else {
            // Too narrow to split: the toggle stays on and the column
            // returns when there is room for it.
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
                // Review only (ADR-019): a command it wrote can be put in
                // the console's input, but the user is the one who sends it.
                self.assistant.can_insert = true;
                let out = self.assistant.ui(ui, s, &theme, "Redis");
                if let Some(code) = out.use_block {
                    self.state.tab = Tab::Console;
                    self.console_input = code.trim().to_string();
                }
            });
    }

    /// The row across the top of the Keys tab: which database, what to match,
    /// scan or stop, and New Key.
    fn keys_controls(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let theme = theme.clone();
        let (db, keyspace, scanning, found) = {
            let sh = self.shared.lock();
            (sh.db, sh.keyspace.clone(), sh.scanning, sh.tree.len())
        };
        let databases = match &self.shared.lock().status {
            Some(Status::Ready(info)) => info.databases.max(1),
            _ => 16,
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(s.t("redis-database"));
            let keys_here = keyspace
                .iter()
                .find(|(n, _)| *n == db)
                .map(|(_, k)| *k)
                .unwrap_or(0);
            egui::ComboBox::from_id_salt("redis-db")
                .width(96.0)
                .selected_text(format!("{db} ({keys_here})"))
                .show_ui(ui, |ui| {
                    for n in 0..databases {
                        let count = keyspace
                            .iter()
                            .find(|(x, _)| *x == n)
                            .map(|(_, k)| *k)
                            .unwrap_or(0);
                        if ui
                            .selectable_label(n == db, format!("{n} ({count})"))
                            .clicked()
                            && n != db
                        {
                            self.send(Cmd::Select { db: n });
                            self.rescan();
                        }
                    }
                });
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.state.pattern)
                    .hint_text(s.t("redis-pattern-hint"))
                    .desired_width(ui.available_width() - 60.0),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.rescan();
            }
            if scanning {
                if tool_button(ui, &theme, ph::STOP, &s.t("redis-stop")).clicked()
                    && let Some(f) = &self.scan_flag
                {
                    f.store(false, Ordering::Relaxed);
                }
            } else if tool_button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("redis-scan")).clicked() {
                self.rescan();
            }
            if tool_button(ui, &theme, ph::PLUS, &s.t("redis-new-key")).clicked() {
                self.popup = Some(Popup::NewKey {
                    key: String::new(),
                    kind: KeyType::String,
                    value: String::new(),
                });
            }
        });
        let count_text = if scanning {
            s.i18n
                .t_args("redis-scanning", &[("n", &found.to_string())])
        } else {
            s.i18n
                .t_args("redis-key-count", &[("n", &found.to_string())])
        };
        form::hint(ui, &theme, &count_text);
    }

    /// The key tree itself.
    fn key_list(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let theme = theme.clone();
        let delimiter = self.fields.delimiter.clone();
        let scanning = self.shared.lock().scanning;
        if let Some(e) = &self.conn_error {
            form::note(ui, &theme, ph::WARNING, e);
            if self.waiting_for_vault && ui.button(s.t("redis-retry")).clicked() {
                self.connect(s);
            }
            return;
        }
        // The tree itself.
        let rows = {
            let sh = self.shared.lock();
            sh.tree.rows(&|p| self.open_groups.contains(p))
        };
        let selected = self.state.open_key.clone();
        let mut to_open: Option<String> = None;
        let mut toggle: Option<String> = None;
        let mut popup: Option<Popup> = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &rows {
                    // One step per level, from the column's own left edge.
                    let indent = 14.0 * row.depth as f32;
                    let is_selected = !row.is_group && selected.as_deref() == Some(&row.path);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(indent);
                        let icon = if row.is_group {
                            if row.expanded {
                                ph::CARET_DOWN
                            } else {
                                ph::CARET_RIGHT
                            }
                        } else {
                            ph::KEY
                        };
                        let label = if row.is_group {
                            format!("{icon} {} ({})", row.label, row.count)
                        } else {
                            format!("{icon} {}", row.label)
                        };
                        // A selectable label sizes to its text, so every row
                        // starts at the same left edge; a stretched button
                        // centres its text and makes the list look ragged.
                        let r = ui.selectable_label(is_selected, label);
                        if r.clicked() {
                            if row.is_group {
                                toggle = Some(row.path.clone());
                            } else {
                                to_open = Some(row.path.clone());
                            }
                        }
                        if let Some(k) = &row.kind {
                            ui.label(
                                egui::RichText::new(k.badge())
                                    .small()
                                    .color(theme.text_muted),
                            );
                        }
                        r.context_menu(|ui| {
                            if !row.is_group && ui.button(s.t("redis-open")).clicked() {
                                to_open = Some(row.path.clone());
                                ui.close();
                            }
                            if ui.button(s.t("redis-copy-key")).clicked() {
                                ui.ctx().copy_text(row.path.clone());
                                ui.close();
                            }
                            if !row.is_group && ui.button(s.t("redis-rename")).clicked() {
                                popup = Some(Popup::Rename {
                                    key: row.path.clone(),
                                    to: row.path.clone(),
                                });
                                ui.close();
                            }
                            if !row.is_group && ui.button(s.t("redis-set-ttl")).clicked() {
                                popup = Some(Popup::Ttl {
                                    key: row.path.clone(),
                                    text: String::new(),
                                });
                                ui.close();
                            }
                            ui.separator();
                            if !row.is_group && ui.button(s.t("redis-delete")).clicked() {
                                popup = Some(Popup::Delete {
                                    keys: vec![row.path.clone()],
                                    prefix: None,
                                    typed: String::new(),
                                });
                                ui.close();
                            }
                            if row.is_group && ui.button(s.t("redis-delete-prefix")).clicked() {
                                let pattern = format!("{}{delimiter}*", row.path);
                                popup = Some(Popup::Delete {
                                    keys: Vec::new(),
                                    prefix: Some(row.path.clone()),
                                    typed: String::new(),
                                });
                                self.send(Cmd::CountPrefix { pattern });
                                ui.close();
                            }
                        });
                    });
                }
                if rows.is_empty() && !scanning {
                    form::hint(ui, &theme, &s.t("redis-no-keys"));
                }
            });
        if let Some(p) = toggle
            && !self.open_groups.remove(&p)
        {
            self.open_groups.insert(p);
        }
        if let Some(k) = to_open {
            self.open_key(&k);
        }
        if popup.is_some() {
            self.popup = popup;
        }
    }
}

impl RedisTool {
    /// The Keys tab: the database picker and filter across the top, the key
    /// list on the left and the open key's value on the right. Both live in
    /// the tab, not the sidebar, so the sidebar stays the connection list.
    fn keys_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.keys_controls(ui, &theme, s);
        ui.separator();
        let total = ui.available_width();
        let list_width = KEY_LIST_WIDTH.min(total * 0.45);
        let value_width = (total - list_width - 26.0).max(160.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(list_width);
                self.key_list(ui, &theme, s);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(value_width);
                self.value_pane(ui, cx);
            });
        });
    }

    fn value_pane(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let (info, page) = {
            let sh = self.shared.lock();
            (sh.open_key.clone(), sh.page.clone())
        };
        let Some(info) = info else {
            form::note(ui, &theme, ph::KEY, &s.t("redis-no-key-open"));
            return;
        };
        match (&self.ttl_read, info.ttl_ms) {
            (Some((k, _, ms)), Some(now)) if *k == info.key && *ms == now => {}
            (_, Some(now)) => {
                self.ttl_read = Some((info.key.clone(), std::time::Instant::now(), now))
            }
            (_, None) => self.ttl_read = None,
        }
        let kind = info.kind.clone().unwrap_or(KeyType::None);
        // Header: what this key is.
        form::header_bar(ui, &theme, |ui| {
            ui.label(egui::RichText::new(&info.key).strong());
            ui.label(
                egui::RichText::new(kind.badge())
                    .small()
                    .color(theme.accent),
            );
            ui.separator();
            // The TTL is read with the key and counted down here, so it
            // runs rather than standing still until the next reload.
            let fresh = match (&self.ttl_read, info.ttl_ms) {
                (Some((k, at, ms)), Some(_)) if *k == info.key => {
                    Some(ms - at.elapsed().as_millis() as i64)
                }
                _ => info.ttl_ms,
            };
            ui.label(
                egui::RichText::new(format!("TTL {}", value::format_ttl(fresh)))
                    .small()
                    .color(theme.text_muted),
            )
            .tip(s.t("redis-ttl-tip"));
            if fresh.is_some() {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs(1));
            }
            if let Some(m) = info.memory {
                ui.label(
                    egui::RichText::new(value::format_bytes(m))
                        .small()
                        .color(theme.text_muted),
                );
            }
            if !info.encoding.is_empty() {
                ui.label(
                    egui::RichText::new(&info.encoding)
                        .small()
                        .color(theme.text_muted),
                );
            }
            if let Some(n) = info.len {
                // A string's length is bytes; a collection's is members.
                let text = if kind == KeyType::String {
                    s.i18n
                        .t_args("redis-length-bytes", &[("n", &n.to_string())])
                } else {
                    s.i18n.t_args("redis-length", &[("n", &n.to_string())])
                };
                ui.label(egui::RichText::new(text).small().color(theme.text_muted));
            }
        });
        ui.horizontal_wrapped(|ui| {
            if tool_button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("redis-reload")).clicked() {
                let k = info.key.clone();
                self.open_key(&k);
            }
            if tool_button(ui, &theme, ph::PENCIL, &s.t("redis-rename")).clicked() {
                self.popup = Some(Popup::Rename {
                    key: info.key.clone(),
                    to: info.key.clone(),
                });
            }
            if tool_button(ui, &theme, ph::TIMER, &s.t("redis-set-ttl")).clicked() {
                self.popup = Some(Popup::Ttl {
                    key: info.key.clone(),
                    text: String::new(),
                });
            }
            if info.ttl_ms.is_some()
                && tool_button(ui, &theme, ph::INFINITY, &s.t("redis-persist")).clicked()
            {
                let w = Write::Ttl {
                    key: info.key.clone(),
                    ms: None,
                };
                self.guarded_write(w, cx);
            }
            if tool_button(ui, &theme, ph::TRASH, &s.t("redis-delete")).clicked() {
                self.popup = Some(Popup::Delete {
                    keys: vec![info.key.clone()],
                    prefix: None,
                    typed: String::new(),
                });
            }
            if kind.is_collection()
                && tool_button(ui, &theme, ph::PLUS, &s.t("redis-add-member")).clicked()
            {
                self.popup = Some(Popup::AddMember {
                    key: info.key.clone(),
                    kind: kind.clone(),
                    field: String::new(),
                    value: String::new(),
                    score: "0".into(),
                });
            }
        });
        ui.add_space(6.0);
        match &page {
            Some(ValuePage::Text { bytes, total }) => {
                let decoded = value::decode(bytes);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:?}", decoded.shape))
                            .small()
                            .color(theme.text_muted),
                    );
                    if decoded.gzip {
                        ui.label(
                            egui::RichText::new(s.t("redis-gzip"))
                                .small()
                                .color(theme.accent),
                        );
                    }
                    if *total as usize > STRING_PREVIEW && bytes.len() < *total as usize {
                        ui.label(
                            egui::RichText::new(s.i18n.t_args(
                                "redis-truncated",
                                &[("shown", &value::format_bytes(bytes.len() as u64))],
                            ))
                            .small()
                            .color(theme.warning),
                        );
                        if ui.button(s.t("redis-load-full")).clicked() {
                            self.send(Cmd::Whole {
                                key: info.key.clone(),
                            });
                        }
                    }
                    if !self.editing {
                        if ui.button(s.t("redis-edit")).clicked() {
                            self.editor = decoded.text.clone();
                            self.editing = true;
                        }
                    } else {
                        if form::primary_button(ui, &theme, &s.t("save")).clicked() {
                            let w = Write::SetString {
                                key: info.key.clone(),
                                value: self.editor.clone().into_bytes(),
                                keep_ttl: true,
                            };
                            self.guarded_write(w, cx);
                            self.editing = false;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            self.editing = false;
                        }
                    }
                });
                ui.add_space(4.0);
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if self.editing {
                            ui.add(
                                egui::TextEdit::multiline(&mut self.editor)
                                    .code_editor()
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20),
                            );
                        } else {
                            let mut shown = decoded.text.clone();
                            ui.add(
                                egui::TextEdit::multiline(&mut shown)
                                    .code_editor()
                                    .interactive(false)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20),
                            );
                        }
                    });
            }
            Some(
                p @ (ValuePage::Pairs { .. }
                | ValuePage::Items { .. }
                | ValuePage::Scored { .. }
                | ValuePage::Entries { .. }),
            ) => {
                let (columns, rows) = page_rows(p);
                self.rows = rows;
                let table = DataTable {
                    theme: &theme,
                    columns: &columns,
                    rows: &self.rows,
                    row_height: 22.0,
                    show_filter: true,
                    copy_row_label: &s.t("redis-copy-row"),
                    built_in_menu: true,
                    row_icons: None,
                };
                let resp = table.show(ui, &mut self.table);
                if let Some(text) = resp.copied {
                    ui.ctx().copy_text(text);
                }
                if let Some(i) = resp.double_clicked {
                    self.member_editor(i, p, &info.key, kind.clone());
                }
                // More of this collection.
                let (cursor, start) = match p {
                    ValuePage::Pairs { cursor, .. } | ValuePage::Scored { cursor, .. } => {
                        (*cursor, 0)
                    }
                    ValuePage::Items {
                        cursor,
                        items,
                        start,
                    } => (*cursor, start + items.len() as u64),
                    _ => (0, 0),
                };
                let more = match p {
                    ValuePage::Entries { items } => items.len() >= PAGE,
                    _ => cursor != 0,
                };
                if more {
                    ui.add_space(4.0);
                    if ui.button(s.t("redis-load-more")).clicked() {
                        self.send(Cmd::More {
                            key: info.key.clone(),
                            kind: kind.clone(),
                            cursor,
                            start,
                        });
                    }
                }
            }
            Some(ValuePage::Unsupported(t)) => {
                form::note(
                    ui,
                    &theme,
                    ph::WARNING,
                    &s.i18n.t_args("redis-unsupported-type", &[("type", t)]),
                );
            }
            None => {
                ui.spinner();
            }
        }
    }

    /// Open the dialog that edits one member of a collection.
    fn member_editor(&mut self, row: usize, page: &ValuePage, key: &str, kind: KeyType) {
        let (field, value, score) = match page {
            ValuePage::Pairs { items, .. } => match items.get(row) {
                Some((f, v)) => (
                    f.clone(),
                    String::from_utf8_lossy(v).into_owned(),
                    String::new(),
                ),
                None => return,
            },
            ValuePage::Items { items, start, .. } => match items.get(row) {
                Some(v) => (
                    (start + row as u64).to_string(),
                    String::from_utf8_lossy(v).into_owned(),
                    String::new(),
                ),
                None => return,
            },
            ValuePage::Scored { items, .. } => match items.get(row) {
                Some((m, sc)) => (
                    String::new(),
                    String::from_utf8_lossy(m).into_owned(),
                    sc.to_string(),
                ),
                None => return,
            },
            _ => return,
        };
        self.popup = Some(Popup::AddMember {
            key: key.to_string(),
            kind,
            field,
            value,
            score,
        });
    }

    fn console_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let entries = self.shared.lock().console.clone();
        let input_height = 36.0;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(ui.available_height() - input_height)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for e in &entries {
                    ui.label(
                        egui::RichText::new(format!("> {}", e.command))
                            .monospace()
                            .color(theme.accent),
                    );
                    for line in &e.lines {
                        let text = if line.prefix.is_empty() {
                            line.text.clone()
                        } else {
                            format!("{} {}", line.prefix, line.text)
                        };
                        ui.horizontal(|ui| {
                            ui.add_space(12.0 * line.depth as f32);
                            ui.label(egui::RichText::new(text).monospace().color(if line.error {
                                theme.error
                            } else {
                                theme.text
                            }));
                        });
                    }
                }
                if entries.is_empty() {
                    form::hint(ui, &theme, &s.t("redis-console-hint"));
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.console_input)
                    .hint_text(s.t("redis-console-input"))
                    .font(egui::TextStyle::Monospace)
                    .desired_width(ui.available_width() - 80.0),
            );
            // ↑/↓ walk the history, the way a shell does.
            if r.has_focus() {
                let (up, down) = ui.input(|i| {
                    (
                        i.key_pressed(egui::Key::ArrowUp),
                        i.key_pressed(egui::Key::ArrowDown),
                    )
                });
                if up && !self.console_history.is_empty() {
                    let at = match self.history_at {
                        Some(0) | None if self.history_at.is_none() => {
                            self.console_history.len() - 1
                        }
                        Some(n) => n.saturating_sub(1),
                        None => self.console_history.len() - 1,
                    };
                    self.history_at = Some(at);
                    self.console_input = self.console_history[at].clone();
                }
                if down {
                    match self.history_at {
                        Some(n) if n + 1 < self.console_history.len() => {
                            self.history_at = Some(n + 1);
                            self.console_input = self.console_history[n + 1].clone();
                        }
                        Some(_) => {
                            self.history_at = None;
                            self.console_input.clear();
                        }
                        None => {}
                    }
                }
            }
            let submit = (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                || form::primary_button(ui, &theme, &s.t("redis-run")).clicked();
            if submit && !self.console_input.trim().is_empty() {
                let line = self.console_input.clone();
                match console::split(&line) {
                    Ok(args) if !args.is_empty() => {
                        self.console_history.push(line.clone());
                        self.history_at = None;
                        self.console_input.clear();
                        r.request_focus();
                        self.run_console(line, args, cx);
                    }
                    Ok(_) => {}
                    Err(e) => cx.toast(ToastLevel::Error, e),
                }
            }
        });
    }

    /// Guards for a typed command, then run it.
    fn run_console(&mut self, line: String, args: Vec<String>, cx: &mut ToolContext) {
        let class = console::classify(&args);
        if class.writes() && self.read_only() {
            let key = if self.read_only_profile {
                "redis-refused-read-only"
            } else {
                "redis-refused-protected"
            };
            cx.toast(ToastLevel::Error, cx.services.t(key));
            return;
        }
        match class {
            Class::Dangerous | Class::Blocking => {
                self.popup = Some(Popup::Command {
                    line,
                    args,
                    typed: String::new(),
                });
            }
            _ => {
                self.send(Cmd::Console { line, args });
            }
        }
    }

    fn info_tab(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        ui.horizontal(|ui| {
            if tool_button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("redis-reload")).clicked() {
                self.send(Cmd::RefreshInfo);
            }
        });
        let text = self.shared.lock().info.clone();
        let Some(text) = text else {
            form::hint(ui, &theme, &s.t("redis-info-hint"));
            self.send(Cmd::RefreshInfo);
            return;
        };
        let sections = info_sections(&text);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (name, rows) in sections {
                    form::section(ui, &theme, &name, |ui| {
                        egui::Grid::new(format!("redis-info-{name}"))
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for (k, v) in rows {
                                    ui.label(egui::RichText::new(k).small());
                                    ui.label(egui::RichText::new(v).small().monospace());
                                    ui.end_row();
                                }
                            });
                    });
                }
            });
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let Some(popup) = self.popup.take() else {
            return;
        };
        let mut keep = Some(popup);
        match keep.as_mut().expect("just set") {
            Popup::Delete {
                keys,
                prefix,
                typed,
            } => {
                // A prefix delete waits for the count before it can say what
                // it would remove.
                if prefix.is_some() && keys.is_empty() {
                    let counted = self.shared.lock().prefix_count.clone();
                    if let Some((_, found, _)) = counted {
                        *keys = found;
                    }
                }
                let n = keys.len();
                let mut close = false;
                let mut go = false;
                egui::Modal::new(egui::Id::new("redis-delete")).show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.heading(s.t("redis-delete-title"));
                    ui.add_space(6.0);
                    match prefix.as_deref() {
                        Some(p) if n == 0 => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(s.i18n.t_args("redis-counting", &[("prefix", p)]));
                            });
                        }
                        Some(p) => {
                            ui.label(s.i18n.t_args(
                                "redis-delete-prefix-body",
                                &[
                                    ("n", &n.to_string()),
                                    ("prefix", p),
                                    ("env", &self.env_name),
                                ],
                            ));
                        }
                        None => {
                            ui.label(s.i18n.t_args(
                                "redis-delete-body",
                                &[("key", keys.first().map(String::as_str).unwrap_or(""))],
                            ));
                        }
                    }
                    // A prefix delete on a protected environment asks for the
                    // prefix to be typed back, as the SQL guard does.
                    let need_typed = prefix.is_some() && self.protected;
                    if need_typed {
                        ui.add_space(8.0);
                        ui.label(s.t("redis-type-prefix"));
                        ui.text_edit_singleline(typed);
                    }
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let allowed = n > 0
                            && (!need_typed || typed.as_str() == prefix.as_deref().unwrap_or(""));
                        if ui
                            .add_enabled(allowed, egui::Button::new(s.t("redis-delete")))
                            .clicked()
                        {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go {
                    let keys = std::mem::take(keys);
                    self.shared.lock().prefix_count = None;
                    if self.read_only() {
                        cx.toast(ToastLevel::Error, s.t("redis-refused-protected"));
                    } else {
                        self.send(Cmd::Delete { keys });
                    }
                    close = true;
                }
                if close {
                    self.shared.lock().prefix_count = None;
                    keep = None;
                }
            }
            Popup::Rename { key, to } => {
                let mut close = false;
                let mut go = false;
                egui::Modal::new(egui::Id::new("redis-rename")).show(ui.ctx(), |ui| {
                    ui.set_width(420.0);
                    ui.heading(s.t("redis-rename"));
                    ui.add_space(6.0);
                    ui.text_edit_singleline(to);
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::primary_button(ui, &theme, &s.t("save")).clicked() {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go {
                    let w = Write::Rename {
                        key: key.clone(),
                        to: to.clone(),
                    };
                    self.guarded_write(w, cx);
                    self.state.open_key = Some(to.clone());
                    close = true;
                }
                if close {
                    keep = None;
                }
            }
            Popup::Ttl { key, text } => {
                let mut close = false;
                let mut go = false;
                egui::Modal::new(egui::Id::new("redis-ttl")).show(ui.ctx(), |ui| {
                    ui.set_width(420.0);
                    ui.heading(s.t("redis-set-ttl"));
                    ui.add_space(6.0);
                    form::hint(ui, &theme, &s.t("redis-ttl-body"));
                    ui.text_edit_singleline(text);
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::primary_button(ui, &theme, &s.t("save")).clicked() {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go {
                    match text.trim().parse::<i64>() {
                        Ok(secs) if secs > 0 => {
                            let w = Write::Ttl {
                                key: key.clone(),
                                ms: Some(secs * 1000),
                            };
                            self.guarded_write(w, cx);
                            close = true;
                        }
                        _ => cx.toast(ToastLevel::Error, s.t("redis-ttl-invalid")),
                    }
                }
                if close {
                    keep = None;
                }
            }
            Popup::NewKey { key, kind, value } => {
                let mut close = false;
                let mut go = false;
                egui::Modal::new(egui::Id::new("redis-new")).show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.heading(s.t("redis-new-key"));
                    ui.add_space(6.0);
                    form::field_grid(ui, "redis-new-grid", |ui| {
                        form::label(ui, &s.t("redis-key"));
                        form::text_field(ui, key, "session:1042:profile");
                        ui.end_row();
                        form::label(ui, &s.t("redis-type"));
                        egui::ComboBox::from_id_salt("redis-new-kind")
                            .selected_text(kind.badge().to_string())
                            .show_ui(ui, |ui| {
                                for k in [
                                    KeyType::String,
                                    KeyType::List,
                                    KeyType::Set,
                                    KeyType::ZSet,
                                    KeyType::Hash,
                                ] {
                                    let label = k.badge().to_string();
                                    ui.selectable_value(kind, k, label);
                                }
                            });
                        ui.end_row();
                        form::label(ui, &s.t("redis-value"));
                        form::text_field(ui, value, "");
                        ui.end_row();
                    });
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::primary_button(ui, &theme, &s.t("redis-create")).clicked() {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go && !key.trim().is_empty() {
                    let bytes = value.clone().into_bytes();
                    let w = match kind {
                        KeyType::List => Write::ListPush {
                            key: key.clone(),
                            value: bytes,
                            front: false,
                        },
                        KeyType::Set => Write::SetAdd {
                            key: key.clone(),
                            value: bytes,
                        },
                        KeyType::ZSet => Write::ZSetAdd {
                            key: key.clone(),
                            member: bytes,
                            score: 0.0,
                        },
                        KeyType::Hash => Write::HashSet {
                            key: key.clone(),
                            field: "field".into(),
                            value: bytes,
                        },
                        _ => Write::SetString {
                            key: key.clone(),
                            value: bytes,
                            keep_ttl: false,
                        },
                    };
                    let k = key.clone();
                    self.guarded_write(w, cx);
                    self.open_key(&k);
                    close = true;
                }
                if close {
                    keep = None;
                }
            }
            Popup::AddMember {
                key,
                kind,
                field,
                value,
                score,
            } => {
                let mut close = false;
                let mut go = false;
                let mut remove = false;
                egui::Modal::new(egui::Id::new("redis-member")).show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.heading(s.t("redis-member"));
                    ui.add_space(6.0);
                    form::field_grid(ui, "redis-member-grid", |ui| {
                        match kind {
                            KeyType::Hash => {
                                form::label(ui, &s.t("redis-field"));
                                form::text_field(ui, field, "");
                                ui.end_row();
                            }
                            KeyType::List => {
                                form::label(ui, &s.t("redis-index"));
                                form::text_field(ui, field, "0");
                                ui.end_row();
                            }
                            KeyType::ZSet => {
                                form::label(ui, &s.t("redis-score"));
                                form::text_field(ui, score, "0");
                                ui.end_row();
                            }
                            _ => {}
                        }
                        form::label(ui, &s.t("redis-value"));
                        form::text_field(ui, value, "");
                        ui.end_row();
                    });
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::primary_button(ui, &theme, &s.t("save")).clicked() {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                        if form::danger_button(ui, &theme, &s.t("redis-remove")).clicked() {
                            remove = true;
                        }
                    });
                });
                if go || remove {
                    let bytes = value.clone().into_bytes();
                    let w = match (kind.clone(), remove) {
                        (KeyType::Hash, false) => Write::HashSet {
                            key: key.clone(),
                            field: field.clone(),
                            value: bytes,
                        },
                        (KeyType::Hash, true) => Write::HashDelete {
                            key: key.clone(),
                            field: field.clone(),
                        },
                        (KeyType::List, false) => match field.trim().parse::<i64>() {
                            Ok(i) => Write::ListSet {
                                key: key.clone(),
                                index: i,
                                value: bytes,
                            },
                            Err(_) => Write::ListPush {
                                key: key.clone(),
                                value: bytes,
                                front: false,
                            },
                        },
                        (KeyType::List, true) => Write::ListRemove {
                            key: key.clone(),
                            value: bytes,
                        },
                        (KeyType::Set, false) => Write::SetAdd {
                            key: key.clone(),
                            value: bytes,
                        },
                        (KeyType::Set, true) => Write::SetRemove {
                            key: key.clone(),
                            value: bytes,
                        },
                        (KeyType::ZSet, false) => Write::ZSetAdd {
                            key: key.clone(),
                            member: bytes,
                            score: score.trim().parse().unwrap_or(0.0),
                        },
                        (KeyType::ZSet, true) => Write::ZSetRemove {
                            key: key.clone(),
                            member: bytes,
                        },
                        (KeyType::Stream, true) => Write::StreamDelete {
                            key: key.clone(),
                            id: field.clone(),
                        },
                        _ => Write::SetString {
                            key: key.clone(),
                            value: bytes,
                            keep_ttl: true,
                        },
                    };
                    self.guarded_write(w, cx);
                    close = true;
                }
                if close {
                    keep = None;
                }
            }
            Popup::Command { line, args, typed } => {
                let mut close = false;
                let mut go = false;
                let class = console::classify(args);
                egui::Modal::new(egui::Id::new("redis-command")).show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.heading(s.t("redis-confirm-command"));
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(line.as_str()).monospace());
                    ui.add_space(6.0);
                    form::note(
                        ui,
                        &theme,
                        ph::WARNING,
                        &s.t(if class == Class::Blocking {
                            "redis-blocking-body"
                        } else {
                            "redis-dangerous-body"
                        }),
                    );
                    if self.protected {
                        ui.add_space(8.0);
                        ui.label(s.i18n.t_args("redis-type-env", &[("env", &self.env_name)]));
                        ui.text_edit_singleline(typed);
                    }
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let allowed = !self.protected || typed.as_str() == self.env_name;
                        if ui
                            .add_enabled(allowed, egui::Button::new(s.t("redis-run")))
                            .clicked()
                        {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go {
                    self.send(Cmd::Console {
                        line: line.clone(),
                        args: args.clone(),
                    });
                    close = true;
                }
                if close {
                    keep = None;
                }
            }
        }
        self.popup = keep;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_splits_into_its_sections() {
        let text = "# Server\r\nredis_version:7.2.4\r\nuptime_in_seconds:91\r\n\r\n# Clients\r\nconnected_clients:3\r\n";
        let s = info_sections(text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].0, "Server");
        assert_eq!(s[0].1[0], ("redis_version".into(), "7.2.4".into()));
        assert_eq!(s[1].0, "Clients");
        assert_eq!(s[1].1.len(), 1);
        // A section with no rows is dropped rather than drawn empty.
        assert!(info_sections("# Empty\n").is_empty());
    }

    #[test]
    fn a_hash_page_becomes_field_and_value_columns() {
        let page = ValuePage::Pairs {
            items: vec![("sku:A1".into(), b"2".to_vec())],
            cursor: 0,
        };
        let (cols, rows) = page_rows(&page);
        assert_eq!(cols.len(), 2);
        assert_eq!(rows[0], vec!["sku:A1".to_string(), "2".to_string()]);
    }

    #[test]
    fn a_list_page_numbers_rows_from_where_the_page_began() {
        let page = ValuePage::Items {
            items: vec![b"a".to_vec(), b"b".to_vec()],
            cursor: 0,
            start: 200,
        };
        let (_, rows) = page_rows(&page);
        assert_eq!(rows[0][0], "200");
        assert_eq!(rows[1][0], "201");
    }

    #[test]
    fn a_stream_page_shows_its_entry_fields_on_one_row() {
        let page = ValuePage::Entries {
            items: vec![("1700-0".into(), vec![("sensor".into(), b"12".to_vec())])],
        };
        let (_, rows) = page_rows(&page);
        assert_eq!(rows[0][0], "1700-0");
        assert_eq!(rows[0][1], "sensor=12");
    }
}
