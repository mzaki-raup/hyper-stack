//! The tab (docs/tools/queue.md §3): the dashboard on the home tab, and
//! for a broker its destinations on the left and the selected one's
//! Messages, Publish, Consumers and Bindings on the right. Everything the
//! UI shows arrives through [`crate::runner::Shared`]; nothing here talks
//! to a broker.

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use hs_core::Services;
use hs_core::tool::{
    Accept, Action, CommandDescriptor, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::driver::{self, Destination, PeekRequest, PublishRequest};
use crate::payload::{self, Body};
use crate::profile::{ProfileError, QueueFields};
use crate::runner::{self, Cmd, Shared, Status};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "queue",
    title_key: "tool-queue",
    icon: ph::QUEUE,
    category: ToolCategory::Infra,
    accepts: &[Accept::Connection],
};

/// What every open broker tab last counted, by connection id: the
/// dashboard's figures come from the brokers open in tabs, and a broker
/// that is not open contributes nothing rather than being connected to
/// just to fill a number (docs/tools/queue.md §3).
static FIGURES: LazyLock<Mutex<HashMap<String, (usize, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct QueueFactory;

impl ToolFactory for QueueFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = QueueTool::new(state);
        // A restored tab opens its broker itself; nothing calls `open`.
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores. Never a secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    /// The selected destination: (scope, name).
    pub selected: Option<(String, String)>,
    pub detail: Detail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    #[default]
    Messages,
    Publish,
    Consumers,
    Bindings,
}

/// A destructive action waiting for its answer.
enum Popup {
    Purge(Destination),
    /// Delete asks for the name to be typed back, as `DROP` does.
    Delete {
        dest: Destination,
        typed: String,
    },
}

/// The Publish form.
#[derive(Default)]
struct PublishForm {
    /// The destination it was filled for, so choosing another refills it.
    for_dest: String,
    target: String,
    key: String,
    partition: String,
    headers: Vec<(String, String)>,
    payload: String,
    qos: u8,
    retain: bool,
}

pub struct QueueTool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    cmd: Option<mpsc::Sender<Cmd>>,
    conn_name: String,
    kind: String,
    address: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    read_only_profile: bool,
    protected: bool,
    writes_unlocked: bool,
    env_name: String,
    table: TableState,
    messages_table: TableState,
    consumers_table: TableState,
    last_poll: Option<Instant>,
    peek_count: usize,
    peek_from: String,
    peek_partition: String,
    publish: PublishForm,
    popup: Option<Popup>,
    /// The detail pane last asked for, and for which destination, so a
    /// restored tab that opens on Consumers loads them once by itself.
    detail_requested: Option<(Detail, String)>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl Drop for QueueTool {
    fn drop(&mut self) {
        if let Some(id) = &self.state.conn_id {
            FIGURES.lock().remove(id);
        }
    }
}

impl QueueTool {
    fn new(state: State) -> QueueTool {
        QueueTool {
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            cmd: None,
            conn_name: String::new(),
            kind: String::new(),
            address: String::new(),
            conn_error: None,
            waiting_for_vault: false,
            read_only_profile: false,
            protected: false,
            writes_unlocked: false,
            env_name: String::new(),
            table: TableState::default(),
            messages_table: TableState::default(),
            consumers_table: TableState::default(),
            last_poll: None,
            peek_count: 10,
            peek_from: String::new(),
            peek_partition: String::new(),
            publish: PublishForm::default(),
            popup: None,
            detail_requested: None,
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
        self.last_poll = None;
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (kind, fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("queue-no-profile"));
                return;
            };
            (
                c.kind.clone(),
                QueueFields::from_table(&c.kind, &c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.kind = kind.clone();
        self.address = fields.short_address().to_string();
        self.read_only_profile = read_only;
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
        let settings = s.settings.read().queue.clone();
        self.peek_count = (settings.peek_count as usize).clamp(1, driver::PEEK_MAX);
        let lookup = {
            let envs = s.environments.read().clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let mut cfg = match fields.resolve(&kind, self.read_only(), s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault = e == ProfileError::VaultLocked;
                self.conn_error = Some(match e {
                    ProfileError::VaultLocked => s.t("queue-vault-locked"),
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("queue-missing-vars", &[("names", &v.join(", "))]),
                    ProfileError::Secret(e) | ProfileError::Invalid(e) => e,
                });
                return;
            }
        };
        cfg.mqtt_window = Duration::from_secs(u64::from(settings.mqtt_window_secs.clamp(1, 60)));
        // One line per connection, so a support log says which broker a
        // tab was on. Never the credential.
        tracing::info!(
            "queue: connecting {} to {} {}",
            self.conn_name,
            kind,
            self.address
        );
        self.cmd = Some(runner::spawn(&s.runtime, cfg, self.shared.clone()));
        self.send(Cmd::Refresh);
    }

    fn send(&mut self, cmd: Cmd) -> bool {
        match &self.cmd {
            Some(tx) => tx.try_send(cmd).is_ok(),
            None => false,
        }
    }

    fn refresh(&mut self) {
        self.last_poll = Some(Instant::now());
        self.send(Cmd::Refresh);
    }

    /// The destination the user has selected, as last listed.
    fn selected(&self) -> Option<Destination> {
        let (scope, name) = self.state.selected.as_ref()?;
        let sh = self.shared.lock();
        sh.listing
            .as_ref()?
            .destinations
            .iter()
            .find(|d| &d.scope == scope && &d.name == name)
            .cloned()
    }

    fn peek(&mut self) {
        let Some(dest) = self.selected() else {
            return;
        };
        self.state.detail = Detail::Messages;
        self.messages_table = TableState::default();
        let req = PeekRequest {
            count: self.peek_count,
            from: self.peek_from.clone(),
            partition: self.peek_partition.trim().parse().ok(),
        };
        self.send(Cmd::Peek(dest, req));
    }

    fn load_detail(&mut self) {
        let Some(dest) = self.selected() else {
            return;
        };
        self.detail_requested = Some((self.state.detail, dest.name.clone()));
        let caps = self.shared.lock().caps;
        match self.state.detail {
            Detail::Consumers if caps.consumers => {
                self.consumers_table = TableState::default();
                self.send(Cmd::Consumers(dest));
            }
            Detail::Bindings if caps.bindings => {
                self.send(Cmd::Bindings(dest));
            }
            _ => {}
        }
    }

    /// Refuse a write here as well as in the driver, with the reason.
    fn guard(&self, cx: &mut ToolContext) -> bool {
        if self.read_only() {
            let key = if self.read_only_profile {
                "queue-refused-read-only"
            } else {
                "queue-refused-protected"
            };
            cx.toast(ToastLevel::Error, cx.services.t(key));
            return false;
        }
        true
    }

    fn ask_purge(&mut self, cx: &mut ToolContext) {
        let caps = self.shared.lock().caps;
        if !caps.purge || !self.guard(cx) {
            return;
        }
        if let Some(d) = self.selected() {
            self.popup = Some(Popup::Purge(d));
        }
    }

    fn ask_delete(&mut self, cx: &mut ToolContext) {
        let caps = self.shared.lock().caps;
        if !caps.delete || !self.guard(cx) {
            return;
        }
        if let Some(dest) = self.selected() {
            self.popup = Some(Popup::Delete {
                dest,
                typed: String::new(),
            });
        }
    }

    /// The Publish form, filled for the destination the user chose: what
    /// it is sent to is on the form before anything is sent
    /// (docs/tools/queue.md §6).
    fn fill_publish(&mut self, dest: &Destination) {
        let key = format!("{}/{}", dest.scope, dest.name);
        if self.publish.for_dest == key {
            return;
        }
        let headers = std::mem::take(&mut self.publish.headers);
        let payload = std::mem::take(&mut self.publish.payload);
        self.publish = PublishForm {
            for_dest: key,
            headers,
            payload,
            ..Default::default()
        };
        match self.kind.as_str() {
            // The default exchange routes by queue name.
            "rabbitmq" => self.publish.key = dest.name.clone(),
            // A stream is published to through one of its subjects.
            "nats" => {
                self.publish.target = dest
                    .figures
                    .get(3)
                    .and_then(|s| s.split(", ").next())
                    .filter(|s| !s.contains(['*', '>']))
                    .unwrap_or_default()
                    .to_string();
            }
            _ => self.publish.target = dest.name.clone(),
        }
    }

    fn publish_now(&mut self, cx: &mut ToolContext) {
        if !self.guard(cx) {
            return;
        }
        let Some(dest) = self.selected() else {
            return;
        };
        let f = &self.publish;
        let partition = match f.partition.trim() {
            "" => None,
            p => match p.parse() {
                Ok(n) => Some(n),
                Err(_) => {
                    cx.toast(
                        ToastLevel::Error,
                        cx.services
                            .i18n
                            .t_args("queue-bad-partition", &[("value", p)]),
                    );
                    return;
                }
            },
        };
        let req = PublishRequest {
            target: f.target.trim().to_string(),
            scope: dest.scope.clone(),
            key: f.key.clone(),
            headers: f
                .headers
                .iter()
                .filter(|(k, _)| !k.trim().is_empty())
                .cloned()
                .collect(),
            payload: f.payload.clone().into_bytes(),
            partition,
            qos: f.qos,
            retain: f.retain,
        };
        if req.target.is_empty() && self.kind != "rabbitmq" {
            cx.toast(ToastLevel::Error, cx.services.t("queue-publish-no-target"));
            return;
        }
        self.send(Cmd::Publish(req));
    }
}

/// A driver's message as a toast: sentence case, so "purged “orders”"
/// reads "Purged “orders”".
fn sentence(text: &str) -> String {
    let mut c = text.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// The Fluent key of what a kind calls its destinations.
fn destination_word(kind: &str) -> &'static str {
    match kind {
        "rabbitmq" => "queue-word-queues",
        "kafka" | "mqtt" => "queue-word-topics",
        "redis-streams" | "nats" => "queue-word-streams",
        _ => "queue-word-destinations",
    }
}

impl Tool for QueueTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    /// The kind and where it is: the dashboard's card already shows the
    /// name above this.
    fn subtitle(&self) -> Option<String> {
        (!self.kind.is_empty()).then(|| {
            format!(
                "{} · {}",
                hs_core::connections::kind_label(&self.kind),
                self.address
            )
        })
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> std::borrow::Cow<'_, str> {
        if self.conn_name.is_empty() {
            std::borrow::Cow::Borrowed("Queue")
        } else {
            std::borrow::Cow::Borrowed(&self.conn_name)
        }
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

    /// The broker, what it said it is, its version and where it is.
    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        if self.conn_name.is_empty() {
            return None;
        }
        let info = self.shared.lock().info.clone();
        Some(
            hs_core::tool::ToolHeader::new(
                hs_ui::icons::connection_icon(&self.kind, ""),
                self.conn_name.clone(),
            )
            .plain(if info.product.is_empty() {
                hs_core::connections::kind_label(&self.kind).to_string()
            } else {
                info.product
            })
            .fact("Version", info.version)
            .fact("Broker", self.address.clone()),
        )
    }

    fn tab_icon(&self) -> Option<&'static str> {
        if self.kind.is_empty() {
            return None;
        }
        Some(hs_ui::icons::connection_icon(&self.kind, ""))
    }

    fn on_event(&mut self, ev: &hs_core::events::Event, cx: &mut ToolContext) {
        match ev {
            // A renamed profile renames its tab, and nothing else: a
            // rename must not disturb a live session.
            hs_core::events::Event::ConnectionsChanged => {
                if let Some(id) = self.state.conn_id.clone()
                    && let Some(name) = cx
                        .services
                        .connections
                        .read()
                        .by_id(&id)
                        .map(|c| c.name.clone())
                {
                    self.conn_name = name;
                }
            }
            hs_core::events::Event::VaultUnlocked if self.waiting_for_vault => {
                self.connect(cx.services);
            }
            _ => {}
        }
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                if let Some(old) = self.state.conn_id.take() {
                    FIGURES.lock().remove(&old);
                }
                self.state = State {
                    conn_id: Some(id),
                    ..Default::default()
                };
                self.connect(cx.services);
                true
            }
            _ => false,
        }
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        let c = |id: &str, title: &str, key: Option<&str>| CommandDescriptor {
            id: id.into(),
            title: title.into(),
            shortcut_action: key.map(String::from),
        };
        vec![
            c("queue.refresh", "Queue: Refresh", Some("run.alt")),
            c("queue.peek", "Queue: Peek Messages", Some("queue.peek")),
            c(
                "queue.publish",
                "Queue: Publish Message",
                Some("queue.publish"),
            ),
            c("queue.purge", "Queue: Purge…", None),
            c("queue.delete", "Queue: Delete…", None),
            c("queue.add", "Queue: Add Broker", None),
            c("queue.reconnect", "Queue: Reconnect", None),
            c("queue.ai", "Queue: AI Assistant", None),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            // F5 is `run.alt` everywhere; here it reads the broker again.
            "queue.refresh" | "run.alt" => {
                self.refresh();
                true
            }
            "queue.peek" => {
                self.peek();
                true
            }
            "queue.publish" => {
                self.state.detail = Detail::Publish;
                true
            }
            "queue.purge" => {
                self.ask_purge(cx);
                true
            }
            "queue.delete" => {
                self.ask_delete(cx);
                true
            }
            "queue.add" => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:queue".into(),
                });
                true
            }
            "queue.reconnect" => {
                self.connect(cx.services);
                true
            }
            "queue.ai" => {
                self.assistant.toggle("Queue");
                true
            }
            _ => false,
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if form::sidebar_header(
            ui,
            &theme,
            &s.t("queue-brokers"),
            &[(ph::PLUS, &s.t("queue-add-broker"))],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:queue".into(),
            });
        }
        let profiles: Vec<(String, String, String, bool, &'static str)> = {
            let conns = s.connections.read();
            conns
                .of_queue_kinds()
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        QueueFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                        c.pinned,
                        hs_ui::icons::connection_icon(&c.kind, ""),
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("queue-no-brokers"));
            return true;
        }
        egui::ScrollArea::vertical()
            .id_salt("queue-brokers")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("queue-brokers");
                let mut open: Option<(String, bool)> = None;
                let mut pin: Option<(String, bool)> = None;
                let mut edit: Option<String> = None;
                for (i, (id, name, label, pinned, icon)) in profiles.iter().enumerate() {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            selected,
                            icon,
                            name,
                            Some(label),
                            *pinned,
                            form::RowMenuLabels {
                                pin: &s.t("conn-pin-home"),
                                unpin: &s.t("conn-unpin-home"),
                                edit: &s.t("conn-edit"),
                            },
                        );
                        if row.response.tip(s.t("queue-open-broker")).clicked() {
                            open = Some((id.clone(), selected));
                        }
                        if let Some(p) = row.pin {
                            pin = Some((id.clone(), p));
                        }
                        if row.edit {
                            edit = Some(id.clone());
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((id, p)) = pin
                    && let Err(e) = s.connections.write().set_pinned(&id, p)
                {
                    cx.toast(ToastLevel::Error, e.to_string());
                }
                if let Some(id) = edit {
                    cx.actions.push(Action::RunCommand {
                        id: format!("settings:connection:{id}"),
                    });
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    if let Err(e) =
                        s.connections
                            .write()
                            .reorder(hs_core::connections::QUEUE_KINDS, from, to)
                    {
                        cx.toast(ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
                if let Some((id, selected)) = open
                    && !selected
                {
                    cx.open("queue", Some(OpenRequest::Connection { id }));
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
        if self.state.conn_id.is_some() && !cx.is_home {
            let (caps, busy) = {
                let sh = self.shared.lock();
                (sh.caps, sh.busy.is_some())
            };
            let ready = matches!(self.shared.lock().status, Some(Status::Ready));
            let has_dest = self.state.selected.is_some();
            if ui
                .add_enabled(
                    ready && !busy,
                    egui::Button::new(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("queue-refresh"))),
                )
                .tip(s.t("queue-refresh-tip"))
                .clicked()
            {
                self.refresh();
            }
            ui.separator();
            if ui
                .add_enabled(
                    ready && has_dest,
                    egui::Button::new(format!("{} {}", ph::ENVELOPE_OPEN, s.t("queue-peek"))),
                )
                .tip(s.t("queue-peek-tip"))
                .clicked()
            {
                self.peek();
            }
            let reason = if self.read_only_profile {
                s.t("queue-refused-read-only")
            } else {
                s.t("queue-refused-protected")
            };
            let writable = ready && has_dest && !self.read_only();
            if caps.purge
                && ui
                    .add_enabled(
                        writable,
                        egui::Button::new(format!("{} {}", ph::BROOM, s.t("queue-purge"))),
                    )
                    .tip(s.t("queue-purge-tip"))
                    .tip_disabled(if self.read_only() {
                        reason.clone()
                    } else {
                        s.t("queue-select-first")
                    })
                    .clicked()
            {
                self.ask_purge(cx);
            }
            if caps.delete
                && ui
                    .add_enabled(
                        writable,
                        egui::Button::new(format!("{} {}", ph::TRASH, s.t("queue-delete"))),
                    )
                    .tip_disabled(if self.read_only() {
                        reason.clone()
                    } else {
                        s.t("queue-select-first")
                    })
                    .clicked()
            {
                self.ask_delete(cx);
            }
            if self.protected && !self.writes_unlocked && !self.read_only_profile {
                ui.separator();
                let label = s.i18n.t_args("queue-protected", &[("env", &self.env_name)]);
                if ui.button(label).tip(s.t("queue-unlock-hint")).clicked() {
                    // The runner was started read-only; writes need a
                    // session that is not.
                    self.writes_unlocked = true;
                    self.connect(s);
                }
            }
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("Queue");
            }
        });
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        self.body(&mut content, cx);
        self.assistant_column(ui, assistant_rect, cx);
    }
}

impl QueueTool {
    /// The home tab: the brokers, what the open ones hold, and the tabs.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("queue");
        let saved: Vec<hs_ui::dashboard::Entry> = {
            let conns = s.connections.read();
            conns
                .of_queue_kinds()
                .map(|c| {
                    let f = QueueFields::from_table(&c.kind, &c.fields);
                    hs_ui::dashboard::Entry {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        detail: f.label_for(&c.kind),
                        icon: hs_ui::icons::connection_icon(&c.kind, "").to_string(),
                        image: None,
                    }
                })
                .collect()
        };
        let (destinations, ready): (usize, u64) = FIGURES
            .lock()
            .values()
            .fold((0, 0), |(d, r), (n, m)| (d + n, r + m));
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-queue"),
            subtitle: &s.t("queue-dashboard-sub"),
            saved: &s.t("queue-saved-brokers"),
            empty: &s.t("queue-no-brokers"),
            start_here: Some(&s.t("dash-start-here")),
            salt: "queue-dashboard",
            tabs: &tabs,
            labels: &labels,
        };
        let pick = hs_ui::dashboard::saved_dashboard(
            ui,
            &theme,
            &page,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::PLUGS_CONNECTED,
                    value: &saved.len().to_string(),
                    label: &s.t("queue-stat-brokers"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::QUEUE,
                    value: &destinations.to_string(),
                    label: &s.t("queue-stat-destinations"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::ENVELOPE_SIMPLE,
                    value: &ready.to_string(),
                    label: &s.t("queue-stat-ready"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[format!("{} {}", ph::PLUS, s.t("queue-add-broker"))],
            &saved,
        );
        match pick {
            Some(hs_ui::dashboard::Pick::Action(_)) => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:queue".into(),
                });
            }
            Some(hs_ui::dashboard::Pick::Open(id)) => {
                cx.open("queue", Some(OpenRequest::Connection { id }));
            }
            Some(hs_ui::dashboard::Pick::Focus(tab_id)) => {
                cx.actions.push(Action::FocusTab { tab_id });
            }
            None => {}
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let toasts: Vec<(bool, String)> = std::mem::take(&mut self.shared.lock().toasts);
        for (ok, text) in toasts {
            cx.toast(
                if ok {
                    ToastLevel::Success
                } else {
                    ToastLevel::Error
                },
                sentence(&text),
            );
        }
        if self.state.conn_id.is_none() {
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("queue-pick-hint"));
            return;
        }
        if let Some(e) = self.conn_error.clone() {
            form::note(ui, &theme, ph::WARNING, &e);
            if ui.button(s.t("queue-retry")).clicked() {
                self.connect(s);
            }
            return;
        }
        let status = self.shared.lock().status.clone();
        match status {
            Some(Status::Connecting) | None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("queue-connecting"));
                });
                return;
            }
            Some(Status::Failed(e)) => {
                form::note(ui, &theme, ph::WARNING, &e);
                if ui.button(s.t("queue-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            Some(Status::Ready) => {}
        }
        // Poll while the tab is drawn, never while it is not
        // (docs/tools/queue.md §7): `ui` runs only for a visible tab.
        let poll = Duration::from_secs(u64::from(s.settings.read().queue.poll_secs.max(1)));
        if self.last_poll.is_none_or(|t| t.elapsed() >= poll) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(poll);
        // The dashboard's figures follow what this tab last counted.
        if let Some(id) = &self.state.conn_id
            && let Some(l) = &self.shared.lock().listing
        {
            let ready: u64 = l.destinations.iter().filter_map(|d| d.depth).sum();
            FIGURES
                .lock()
                .insert(id.clone(), (l.destinations.len(), ready));
        }
        self.dialogs(ui, cx);
        let left = (ui.available_width() * 0.5).clamp(320.0, 720.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left);
                self.destinations(ui, &theme, s);
            });
            ui.separator();
            ui.vertical(|ui| {
                self.detail(ui, &theme, cx);
            });
        });
    }

    /// The destinations of the broker: what the kind calls them, with the
    /// kind's columns.
    fn destinations(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let (listing, error, busy) = {
            let sh = self.shared.lock();
            (sh.listing.clone(), sh.listing_error.clone(), sh.busy)
        };
        ui.horizontal(|ui| {
            ui.strong(s.t(destination_word(&self.kind)));
            if let Some(b) = busy {
                ui.spinner();
                ui.weak(s.t(b));
            }
        });
        if let Some(e) = &error {
            form::note(ui, theme, ph::WARNING, e);
        }
        let Some(listing) = listing else {
            if error.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("queue-listing"));
                });
            }
            return;
        };
        if let Some(n) = &listing.note {
            form::hint(ui, theme, &sentence(n));
        }
        if listing.destinations.is_empty() {
            form::note(
                ui,
                theme,
                ph::INFO,
                &s.i18n.t_args(
                    "queue-none-found",
                    &[("what", &s.t(destination_word(&self.kind)))],
                ),
            );
            return;
        }
        let mut columns = vec![ColumnSpec::new(s.t("queue-col-name"), 200.0)];
        for key in driver::columns(&self.kind) {
            let numeric = !matches!(
                *key,
                "queue-col-vhost"
                    | "queue-col-state"
                    | "queue-col-first-id"
                    | "queue-col-last-id"
                    | "queue-col-subjects"
                    | "queue-col-retained"
                    | "queue-col-last-seen"
                    | "queue-col-publish-rate"
                    | "queue-col-deliver-rate"
            );
            let c = ColumnSpec::new(s.t(key), 90.0);
            columns.push(if numeric { c.numeric() } else { c });
        }
        let rows: Vec<Vec<String>> = listing
            .destinations
            .iter()
            .map(|d| {
                let mut r = Vec::with_capacity(d.figures.len() + 1);
                r.push(d.name.clone());
                r.extend(d.figures.iter().cloned());
                r
            })
            .collect();
        self.table.selected = self.state.selected.as_ref().and_then(|(scope, name)| {
            listing
                .destinations
                .iter()
                .position(|d| &d.scope == scope && &d.name == name)
        });
        let table = DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: 22.0,
            show_filter: true,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
        };
        // Each table in the tab needs its own id scope, or egui sees the
        // destinations and the consumers as one table drawn twice.
        let r = ui
            .push_id("queue-destinations", |ui| table.show(ui, &mut self.table))
            .inner;
        if let Some(i) = r.clicked.or(r.double_clicked)
            && let Some(d) = listing.destinations.get(i)
        {
            let pick = (d.scope.clone(), d.name.clone());
            if self.state.selected.as_ref() != Some(&pick) {
                self.state.selected = Some(pick);
                self.shared.lock().messages = None;
                self.fill_publish(d);
                self.load_detail();
            }
            if r.double_clicked.is_some() {
                self.peek();
            }
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(dest) = self.selected() else {
            form::note(ui, theme, ph::HAND_POINTING, &s.t("queue-select-hint"));
            return;
        };
        let caps = self.shared.lock().caps;
        // A pane that has not been asked for this destination yet (a tab
        // restored on Consumers, say) asks now, once.
        if matches!(self.state.detail, Detail::Consumers | Detail::Bindings)
            && self.detail_requested.as_ref() != Some(&(self.state.detail, dest.name.clone()))
        {
            self.load_detail();
        }
        ui.horizontal(|ui| {
            for (d, icon, key, shown) in [
                (
                    Detail::Messages,
                    ph::ENVELOPE_OPEN,
                    "queue-tab-messages",
                    true,
                ),
                (
                    Detail::Publish,
                    ph::PAPER_PLANE_RIGHT,
                    "queue-tab-publish",
                    true,
                ),
                (
                    Detail::Consumers,
                    ph::USERS_THREE,
                    "queue-tab-consumers",
                    caps.consumers,
                ),
                (
                    Detail::Bindings,
                    ph::LINK,
                    "queue-tab-bindings",
                    caps.bindings,
                ),
            ] {
                if !shown {
                    continue;
                }
                if ui
                    .add(egui::Button::selectable(
                        self.state.detail == d,
                        format!("{icon} {}", s.t(key)),
                    ))
                    .clicked()
                    && self.state.detail != d
                {
                    self.state.detail = d;
                    self.load_detail();
                }
            }
        });
        ui.add_space(6.0);
        ui.strong(&dest.name);
        ui.add_space(4.0);
        match self.state.detail {
            Detail::Messages => self.messages_pane(ui, theme, s, &dest),
            Detail::Publish => self.publish_pane(ui, theme, cx, &dest),
            Detail::Consumers => self.consumers_pane(ui, theme, s, &dest),
            Detail::Bindings => self.bindings_pane(ui, theme, s, &dest),
        }
    }

    fn messages_pane(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        dest: &Destination,
    ) {
        let caps = self.shared.lock().caps;
        // RabbitMQ cannot read a queue without requeueing what it read;
        // say so before the first peek (docs/tools/queue.md §6).
        match self.kind.as_str() {
            "rabbitmq" => form::note(ui, theme, ph::WARNING, &s.t("queue-peek-requeue")),
            "mqtt" => form::hint(ui, theme, &s.t("queue-peek-mqtt")),
            _ => form::hint(ui, theme, &s.t("queue-peek-safe")),
        }
        ui.horizontal(|ui| {
            ui.label(s.t("queue-peek-count"));
            ui.add(egui::DragValue::new(&mut self.peek_count).range(1..=driver::PEEK_MAX));
            if caps.peek_from {
                ui.label(s.t(match self.kind.as_str() {
                    "kafka" => "queue-peek-from-offset",
                    "redis-streams" => "queue-peek-from-id",
                    _ => "queue-peek-from-sequence",
                }));
                ui.add(
                    egui::TextEdit::singleline(&mut self.peek_from)
                        .hint_text(s.t("queue-peek-from-hint"))
                        .desired_width(120.0),
                );
            }
            if self.kind == "kafka" {
                ui.label(s.t("queue-partition"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.peek_partition)
                        .hint_text(s.t("queue-partition-all"))
                        .desired_width(60.0),
                );
            }
            if ui
                .button(format!("{} {}", ph::ENVELOPE_OPEN, s.t("queue-peek")))
                .clicked()
            {
                self.peek();
            }
        });
        ui.add_space(4.0);
        let got = self.shared.lock().messages.clone();
        let Some(got) = got.filter(|g| g.dest == dest.name) else {
            form::hint(ui, theme, &s.t("queue-peek-empty-hint"));
            return;
        };
        let messages = match got.result {
            Ok(m) => m,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &e);
                return;
            }
        };
        if messages.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("queue-no-messages"));
            return;
        }
        let mut columns = vec![ColumnSpec::new(s.t("queue-col-position"), 110.0)];
        if self.kind == "kafka" {
            columns.push(ColumnSpec::new(s.t("queue-partition"), 70.0).numeric());
            columns.push(ColumnSpec::new(s.t("queue-col-key"), 110.0));
        }
        columns.push(ColumnSpec::new(s.t("queue-col-time"), 170.0));
        columns.push(ColumnSpec::new(s.t("queue-col-payload"), 260.0));
        let rows: Vec<Vec<String>> = messages
            .iter()
            .map(|m| {
                let mut r = vec![m.position.clone()];
                if self.kind == "kafka" {
                    r.push(m.partition.map(|p| p.to_string()).unwrap_or_default());
                    r.push(
                        m.key
                            .as_deref()
                            .map(|k| payload::preview(k, 40))
                            .unwrap_or_default(),
                    );
                }
                r.push(m.timestamp.clone());
                r.push(payload::preview(&m.payload, 80));
                r
            })
            .collect();
        let half = (ui.available_height() * 0.45).max(120.0);
        egui::Frame::new().show(ui, |ui| {
            ui.set_max_height(half);
            let table = DataTable {
                theme,
                columns: &columns,
                rows: &rows,
                row_height: 22.0,
                show_filter: false,
                copy_row_label: &s.t("table-copy-row"),
                built_in_menu: true,
                row_icons: None,
            };
            let r = ui
                .push_id("queue-messages", |ui| {
                    table.show(ui, &mut self.messages_table)
                })
                .inner;
            if let Some(i) = r.clicked {
                self.messages_table.selected = Some(i);
            }
        });
        let pick = self.messages_table.selected.unwrap_or(0);
        let Some(m) = messages.get(pick) else {
            return;
        };
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("queue-message")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !m.headers.is_empty() {
                    egui::Grid::new("queue-message-headers")
                        .num_columns(2)
                        .striped(true)
                        .show(ui, |ui| {
                            for (k, v) in &m.headers {
                                ui.weak(k);
                                ui.label(v);
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                }
                let mono = |t: &str| egui::RichText::new(t).monospace();
                match payload::render(&m.payload) {
                    Body::Json(t) | Body::Text(t) => {
                        ui.add(egui::Label::new(mono(&t)).selectable(true));
                    }
                    Body::Binary { label, len, hex } => {
                        ui.weak(s.i18n.t_args(
                            "queue-binary",
                            &[("what", &label), ("bytes", &len.to_string())],
                        ));
                        ui.add(egui::Label::new(mono(&hex)).selectable(true));
                    }
                    Body::Empty => {
                        ui.weak(s.t("queue-empty-payload"));
                    }
                }
            });
    }

    fn publish_pane(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        cx: &mut ToolContext,
        dest: &Destination,
    ) {
        let s = cx.services;
        self.fill_publish(dest);
        let kind = self.kind.clone();
        let mut send = false;
        form::field_grid(ui, "queue-publish", |ui| {
            let target_key = match kind.as_str() {
                "rabbitmq" => "queue-exchange",
                "nats" => "queue-subject",
                "redis-streams" => "queue-stream",
                _ => "queue-topic",
            };
            form::label(ui, &s.t(target_key));
            let hint = if kind == "rabbitmq" {
                s.t("queue-default-exchange")
            } else {
                String::new()
            };
            form::text_field(ui, &mut self.publish.target, &hint);
            ui.end_row();
            match kind.as_str() {
                "rabbitmq" | "kafka" => {
                    form::label(
                        ui,
                        &s.t(if kind == "rabbitmq" {
                            "queue-routing-key"
                        } else {
                            "queue-col-key"
                        }),
                    );
                    form::text_field(ui, &mut self.publish.key, "");
                    ui.end_row();
                }
                _ => {}
            }
            if kind == "kafka" {
                form::label(ui, &s.t("queue-partition"));
                form::text_field(
                    ui,
                    &mut self.publish.partition,
                    &s.t("queue-partition-by-key"),
                );
                ui.end_row();
            }
            if kind == "mqtt" {
                form::label(ui, &s.t("queue-qos"));
                ui.horizontal(|ui| {
                    for q in 0..=2u8 {
                        ui.radio_value(&mut self.publish.qos, q, q.to_string());
                    }
                });
                ui.end_row();
                form::label(ui, "");
                ui.checkbox(&mut self.publish.retain, s.t("queue-retain"));
                ui.end_row();
            }
        });
        // MQTT 3.1.1 has no headers; every other kind (and MQTT 5) does.
        let headers_label = if kind == "redis-streams" {
            "queue-fields"
        } else {
            "queue-headers"
        };
        let mqtt311 = kind == "mqtt" && self.shared.lock().info.version != "5";
        if !mqtt311 {
            ui.add_space(6.0);
            ui.strong(s.t(headers_label));
            let mut remove = None;
            for (i, (k, v)) in self.publish.headers.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(k)
                            .hint_text(s.t("queue-name"))
                            .desired_width(160.0),
                    );
                    ui.add(
                        egui::TextEdit::singleline(v)
                            .hint_text(s.t("queue-value"))
                            .desired_width(220.0),
                    );
                    if ui.button(ph::X).tip(s.t("queue-remove")).clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                self.publish.headers.remove(i);
            }
            let add = if kind == "redis-streams" {
                "queue-add-field"
            } else {
                "queue-add-header"
            };
            if ui.button(format!("{} {}", ph::PLUS, s.t(add))).clicked() {
                self.publish.headers.push((String::new(), String::new()));
            }
        }
        ui.add_space(6.0);
        ui.strong(s.t("queue-payload"));
        if kind == "redis-streams" {
            form::hint(ui, theme, &s.t("queue-stream-payload-hint"));
        }
        ui.add(
            egui::TextEdit::multiline(&mut self.publish.payload)
                .code_editor()
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        let reason = if self.read_only_profile {
            s.t("queue-refused-read-only")
        } else {
            s.t("queue-refused-protected")
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.read_only(),
                    egui::Button::new(format!(
                        "{} {}",
                        ph::PAPER_PLANE_RIGHT,
                        s.t("queue-publish")
                    )),
                )
                .tip_disabled(reason)
                .clicked()
            {
                send = true;
            }
            if hs_core::fmt::pretty_json(&self.publish.payload).is_some()
                && ui.button(s.t("queue-format-json")).clicked()
                && let Some(p) = hs_core::fmt::pretty_json(&self.publish.payload)
            {
                self.publish.payload = p;
            }
        });
        if send {
            self.publish_now(cx);
        }
    }

    fn consumers_pane(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        dest: &Destination,
    ) {
        let (got, busy, progress) = {
            let sh = self.shared.lock();
            (sh.consumers.clone(), sh.busy, sh.progress.clone())
        };
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    busy.is_none(),
                    egui::Button::new(format!("{} {}", ph::ARROWS_CLOCKWISE, s.t("queue-load"))),
                )
                .clicked()
            {
                self.load_detail();
            }
            if busy == Some("queue-busy-consumers") {
                ui.spinner();
                let total = progress.total.load(Ordering::Relaxed);
                if total > 0 {
                    ui.label(s.i18n.t_args(
                        "queue-group-progress",
                        &[
                            ("done", &progress.done.load(Ordering::Relaxed).to_string()),
                            ("total", &total.to_string()),
                        ],
                    ));
                }
                if ui.button(s.t("queue-stop")).clicked() {
                    progress.cancel.store(true, Ordering::Relaxed);
                }
            }
        });
        let Some(got) = got.filter(|g| g.dest == dest.name) else {
            return;
        };
        let groups = match got.result {
            Ok(g) => g,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &e);
                return;
            }
        };
        if groups.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("queue-no-consumers"));
            return;
        }
        let columns = [
            ColumnSpec::new(s.t("queue-col-name"), 200.0),
            ColumnSpec::new(s.t("queue-col-state"), 160.0),
            ColumnSpec::new(s.t("queue-col-members"), 80.0).numeric(),
            ColumnSpec::new(s.t("queue-col-pending"), 80.0).numeric(),
            ColumnSpec::new(s.t("queue-col-lag"), 80.0).numeric(),
        ];
        let dash = |v: Option<String>| v.unwrap_or_else(|| "—".into());
        let rows: Vec<Vec<String>> = groups
            .iter()
            .map(|g| {
                vec![
                    g.name.clone(),
                    g.state.clone(),
                    g.members.to_string(),
                    dash(g.pending.map(|p| p.to_string())),
                    dash(g.lag.map(|l| l.to_string())),
                ]
            })
            .collect();
        let table = DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: 22.0,
            show_filter: groups.len() > 10,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
        };
        let r = ui
            .push_id("queue-consumers", |ui| {
                table.show(ui, &mut self.consumers_table)
            })
            .inner;
        if let Some(i) = r.clicked {
            self.consumers_table.selected = Some(i);
        }
        // Lag per partition, for the group picked.
        if let Some(g) = self.consumers_table.selected.and_then(|i| groups.get(i))
            && !g.detail.is_empty()
        {
            ui.separator();
            egui::Grid::new("queue-lag")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    for key in [
                        "queue-partition",
                        "queue-col-committed",
                        "queue-col-latest",
                        "queue-col-lag",
                    ] {
                        ui.strong(s.t(key));
                    }
                    ui.end_row();
                    for l in &g.detail {
                        ui.label(l.partition.to_string());
                        ui.label(dash(l.committed.map(|v| v.to_string())));
                        ui.label(dash(l.latest.map(|v| v.to_string())));
                        ui.label(dash(l.lag.map(|v| v.to_string())));
                        ui.end_row();
                    }
                });
        }
    }

    fn bindings_pane(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        dest: &Destination,
    ) {
        let got = self.shared.lock().bindings.clone();
        let Some(got) = got.filter(|g| g.dest == dest.name) else {
            ui.spinner();
            return;
        };
        let bindings = match got.result {
            Ok(b) => b,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &e);
                return;
            }
        };
        egui::Grid::new("queue-bindings")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "queue-col-exchange",
                    "queue-routing-key",
                    "queue-col-arguments",
                ] {
                    ui.strong(s.t(key));
                }
                ui.end_row();
                for b in &bindings {
                    if b.source.is_empty() {
                        ui.label(s.t("queue-default-exchange"));
                    } else {
                        ui.label(&b.source);
                    }
                    ui.label(&b.routing_key);
                    ui.label(&b.arguments);
                    ui.end_row();
                }
            });
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let Some(popup) = self.popup.take() else {
            return;
        };
        match popup {
            Popup::Purge(dest) => {
                let count = dest
                    .depth
                    .map(|d| d.to_string())
                    .unwrap_or_else(|| s.t("queue-unknown-count"));
                let body = s.i18n.t_args(
                    "queue-purge-confirm",
                    &[("name", &dest.name), ("count", &count)],
                );
                match hs_ui::dialog::confirm(
                    ui.ctx(),
                    &theme,
                    "queue-purge",
                    &s.t("queue-purge-title"),
                    &body,
                    &s.t("queue-purge"),
                    &s.t("cancel"),
                    true,
                ) {
                    hs_ui::dialog::DialogResult::Ok => {
                        if self.guard(cx) {
                            self.send(Cmd::Purge(dest));
                        }
                    }
                    hs_ui::dialog::DialogResult::Cancel => {}
                    hs_ui::dialog::DialogResult::Open => self.popup = Some(Popup::Purge(dest)),
                }
            }
            Popup::Delete { dest, mut typed } => {
                let mut answer = None;
                let modal = egui::Modal::new(egui::Id::new("queue-delete")).show(ui.ctx(), |ui| {
                    ui.set_width(440.0);
                    ui.heading(s.t("queue-delete-title"));
                    ui.add_space(6.0);
                    let count = dest
                        .depth
                        .map(|d| d.to_string())
                        .unwrap_or_else(|| s.t("queue-unknown-count"));
                    ui.label(s.i18n.t_args(
                        "queue-delete-confirm",
                        &[("name", &dest.name), ("count", &count)],
                    ));
                    ui.add_space(8.0);
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut typed)
                            .hint_text(&dest.name)
                            .desired_width(f32::INFINITY),
                    );
                    r.request_focus();
                    ui.add_space(12.0);
                    let matches = typed == dest.name;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                matches,
                                egui::Button::new(
                                    egui::RichText::new(s.t("queue-delete"))
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(theme.error),
                            )
                            .clicked()
                        {
                            answer = Some(true);
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            answer = Some(false);
                        }
                    });
                });
                if modal.should_close() && answer.is_none() {
                    answer = Some(false);
                }
                match answer {
                    Some(true) => {
                        if self.guard(cx) {
                            self.state.selected = None;
                            self.send(Cmd::Delete(dest));
                        }
                    }
                    Some(false) => {}
                    None => self.popup = Some(Popup::Delete { dest, typed }),
                }
            }
        }
    }

    /// The review-only assistant (ADR-019), told about the destination on
    /// screen and its figures - for "explain this backlog".
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
        let dest = self.selected();
        let columns: Vec<String> = driver::columns(&self.kind).iter().map(|k| s.t(k)).collect();
        let figures = dest
            .as_ref()
            .map(|d| {
                columns
                    .iter()
                    .zip(&d.figures)
                    .map(|(c, v)| format!("{c}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| "(nothing selected)".into());
        let context = format!(
            "Broker: {} ({})\nDestination: {}\n{figures}",
            self.conn_name,
            hs_core::connections::kind_label(&self.kind),
            dest.map(|d| d.name).unwrap_or_default(),
        );
        self.assistant.set_context("Queue", context);
        if self.assistant.quick_actions.is_empty() {
            self.assistant.quick_actions =
                hs_ui::ai_assistant::QuickAction::for_activity(s, "queue");
        }
        let Some(panel) = panel else {
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
                self.assistant.can_insert = false;
                let _ = self.assistant.ui(ui, s, &theme, "Queue");
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dest(name: &str, figures: &[&str]) -> Destination {
        Destination {
            name: name.into(),
            figures: figures.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn publish_is_filled_for_the_chosen_destination_only() {
        let mut t = QueueTool::new(State::default());
        t.kind = "rabbitmq".into();
        t.fill_publish(&dest("orders", &[]));
        // The default exchange, routed by the queue's name.
        assert_eq!(t.publish.target, "");
        assert_eq!(t.publish.key, "orders");
        // What the user typed survives picking another destination;
        // where it goes does not.
        t.publish.payload = "{}".into();
        t.fill_publish(&dest("billing", &[]));
        assert_eq!(t.publish.key, "billing");
        assert_eq!(t.publish.payload, "{}");
    }

    #[test]
    fn a_nats_stream_is_published_to_through_a_plain_subject() {
        let mut t = QueueTool::new(State::default());
        t.kind = "nats".into();
        t.fill_publish(&dest(
            "ORDERS",
            &["1", "2", "0", "orders.created, orders.*"],
        ));
        assert_eq!(t.publish.target, "orders.created");
        t.fill_publish(&dest("ALL", &["1", "2", "0", "events.>"]));
        assert_eq!(t.publish.target, "");
    }

    #[test]
    fn toasts_start_with_a_capital() {
        assert_eq!(sentence("purged “q”"), "Purged “q”");
        assert_eq!(sentence(""), "");
    }

    #[test]
    fn a_protected_tab_is_read_only_until_unlocked() {
        let mut t = QueueTool::new(State::default());
        t.protected = true;
        assert!(t.read_only());
        t.writes_unlocked = true;
        assert!(!t.read_only());
        t.read_only_profile = true;
        assert!(t.read_only());
    }
}
