//! The tab (docs/tools/nosql.md §4): the sidebar lists the document-store
//! connections, exactly as the SSH, S3, database and Redis sidebars do, and
//! the tab holds the container tree, the query and the documents.

use std::sync::Arc;

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

use crate::flatten;
use crate::profile::{NoSqlFields, ProfileError};
use crate::runner::{self, Cmd, Load, Shared, Status};
use crate::store::{ContainerRef, DocStore, Document, PutMode, Query, StoreKind};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "nosql",
    title_key: "tool-nosql",
    icon: ph::TREE_STRUCTURE,
    category: ToolCategory::Data,
    accepts: &[Accept::Connection],
};

/// The connection kinds this tool opens.
/// The kinds this activity opens. Qdrant joined on 2026-10-24: a
/// collection is a container and a point is a document, so the whole
/// pane serves it unchanged.
pub const KINDS: &[&str] = &["mongodb", "couchdb", "qdrant"];

/// How wide the container tree is beside the documents.
const TREE_WIDTH: f32 = 230.0;
/// Documents per page unless the user changes it.
const DEFAULT_LIMIT: i64 = 50;

pub struct NoSqlFactory;

impl ToolFactory for NoSqlFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = NoSqlTool::new(state);
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
    pub database: Option<String>,
    pub container: Option<String>,
    pub tab: Tab,
    pub view: View,
    pub filter: String,
    pub projection: String,
    pub sort: String,
    pub limit: i64,
    pub skip: i64,
    pub pipeline: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Find,
    Aggregate,
    Indexes,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum View {
    #[default]
    Table,
    Tree,
    Json,
}

enum Popup {
    /// Delete one document.
    Delete { doc: Document },
    /// Delete everything the filter matches; the count arrives first.
    DeleteMany { filter: String, typed: String },
}

pub struct NoSqlTool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    cmd: Option<mpsc::Sender<Cmd>>,
    conn_name: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    read_only_profile: bool,
    protected: bool,
    writes_unlocked: bool,
    env_id: Option<String>,
    env_name: String,
    kind: Option<StoreKind>,
    /// Databases whose containers are shown.
    open_dbs: std::collections::HashSet<String>,
    table: TableState,
    /// The document being edited, and its text.
    editing: Option<(Document, String)>,
    /// `true` while the editor holds a new document.
    inserting: bool,
    popup: Option<Popup>,
    seen_run: u64,
    /// Review-only assistant (ADR-019): it reads the open container and the
    /// query on screen, and a filter it writes can be inserted but not run
    /// by it.
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl NoSqlTool {
    fn new(state: State) -> NoSqlTool {
        NoSqlTool {
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            cmd: None,
            conn_name: String::new(),
            conn_error: None,
            waiting_for_vault: false,
            read_only_profile: false,
            protected: false,
            writes_unlocked: false,
            env_id: None,
            env_name: String::new(),
            kind: None,
            open_dbs: Default::default(),
            table: TableState::default(),
            editing: None,
            inserting: false,
            popup: None,
            seen_run: 0,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        }
    }

    /// Writes are refused on a read-only profile, and on a protected
    /// environment until the tab unlocks them.
    fn read_only(&self) -> bool {
        self.read_only_profile || (self.protected && !self.writes_unlocked)
    }

    fn container(&self) -> Option<ContainerRef> {
        Some(ContainerRef::new(
            self.state.database.clone()?,
            self.state.container.clone()?,
        ))
    }

    fn query(&self) -> Query {
        Query {
            filter: self.state.filter.clone(),
            projection: self.state.projection.clone(),
            sort: self.state.sort.clone(),
            limit: if self.state.limit > 0 {
                self.state.limit
            } else {
                DEFAULT_LIMIT
            },
            skip: self.state.skip.max(0),
        }
    }

    fn connect(&mut self, s: &Services) {
        self.cmd = None;
        self.shared = Arc::new(Mutex::new(Shared::default()));
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.popup = None;
        self.editing = None;
        self.open_dbs.clear();
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("nosql-no-profile"));
                return;
            };
            (
                NoSqlFields::from_table(&c.kind, &c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.read_only_profile = read_only;
        self.env_id = env_id.clone();
        self.kind = fields.store_kind();
        {
            let envs = s.environments.read();
            let env = env_id
                .as_deref()
                .or(envs.active.as_deref())
                .and_then(|e| envs.by_id(e));
            self.protected = env.is_some_and(|e| e.protected);
            self.env_name = env.map(|e| e.name.clone()).unwrap_or_default();
        }
        let Some(kind) = self.kind else {
            self.conn_error = Some(
                s.i18n
                    .t_args("nosql-unknown-kind", &[("kind", &fields.kind)]),
            );
            return;
        };
        let lookup = {
            let envs = s.environments.read().clone();
            let env_id = env_id.clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let read_only_now = self.read_only();
        let say = |e: ProfileError| match e {
            ProfileError::VaultLocked => s.t("nosql-vault-locked"),
            ProfileError::MissingVariables(v) => s
                .i18n
                .t_args("nosql-missing-vars", &[("names", &v.join(", "))]),
            ProfileError::Secret(e) => e,
            ProfileError::UnknownKind(k) => s.i18n.t_args("nosql-unknown-kind", &[("kind", &k)]),
        };
        // The tunnel, when the profile names one, moves the host to a local
        // port before the driver ever sees it.
        let tunnel_id = fields.tunnel_ssh.clone();
        let opener: crate::store::Opening = match kind {
            StoreKind::Mongo => {
                let cfg = match fields.resolve_mongo(read_only_now, s.secrets.as_ref(), &lookup) {
                    Ok(c) => c,
                    Err(e) => {
                        self.waiting_for_vault = e == ProfileError::VaultLocked;
                        self.conn_error = Some(say(e));
                        return;
                    }
                };
                let tunnel = (!tunnel_id.is_empty()).then(|| {
                    s.tunnels
                        .ensure_local_forward(&tunnel_id, &cfg.host, cfg.port)
                });
                Box::pin(async move {
                    let mut cfg = cfg;
                    if let Some(rx) = tunnel {
                        let port = rx
                            .await
                            .map_err(|_| "the SSH tunnel stopped".to_string())?
                            .map_err(|e| format!("SSH tunnel: {e}"))?;
                        cfg.host = "127.0.0.1".into();
                        cfg.port = port;
                    }
                    crate::mongo::MongoStore::connect(&cfg)
                        .await
                        .map(|s| Box::new(s) as Box<dyn DocStore>)
                        .map_err(|e| e.to_string())
                })
            }
            StoreKind::Couch => {
                let cfg = match fields.resolve_couch(read_only_now, s.secrets.as_ref(), &lookup) {
                    Ok(c) => c,
                    Err(e) => {
                        self.waiting_for_vault = e == ProfileError::VaultLocked;
                        self.conn_error = Some(say(e));
                        return;
                    }
                };
                Box::pin(async move {
                    crate::couch::CouchStore::connect(&cfg)
                        .await
                        .map(|s| Box::new(s) as Box<dyn DocStore>)
                        .map_err(|e| e.to_string())
                })
            }
            StoreKind::Qdrant => {
                let cfg = match fields.resolve_qdrant(read_only_now, s.secrets.as_ref(), &lookup) {
                    Ok(c) => c,
                    Err(e) => {
                        self.waiting_for_vault = e == ProfileError::VaultLocked;
                        self.conn_error = Some(say(e));
                        return;
                    }
                };
                Box::pin(async move {
                    crate::qdrant::QdrantStore::connect(&cfg)
                        .await
                        .map(|s| Box::new(s) as Box<dyn DocStore>)
                        .map_err(|e| e.to_string())
                })
            }
        };
        tracing::info!("nosql: connecting {} ({})", self.conn_name, fields.kind);
        self.cmd = Some(runner::spawn(&s.runtime, opener, self.shared.clone()));
    }

    fn send(&self, cmd: Cmd) -> bool {
        match &self.cmd {
            Some(tx) => tx.try_send(cmd).is_ok(),
            None => false,
        }
    }

    fn run_find(&mut self) {
        if let Some(c) = self.container() {
            self.editing = None;
            self.send(Cmd::Find {
                container: c,
                query: self.query(),
            });
        }
    }

    /// Ask before a write the guards cover, or run it now.
    fn guarded(&mut self, cx: &mut ToolContext, cmd: Cmd) {
        if self.read_only() {
            let key = if self.read_only_profile {
                "nosql-refused-read-only"
            } else {
                "nosql-refused-protected"
            };
            cx.toast(ToastLevel::Error, cx.services.t(key));
            return;
        }
        self.send(cmd);
    }
}

impl Tool for NoSqlTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }
    fn subtitle(&self) -> Option<String> {
        let kind = self.kind.map(|k| k.label().to_string()).unwrap_or_default();
        match (kind.is_empty(), self.conn_name.is_empty()) {
            (true, true) => None,
            (true, false) => Some(self.conn_name.clone()),
            (false, true) => Some(kind),
            (false, false) => Some(format!("{kind} · {}", self.conn_name)),
        }
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> std::borrow::Cow<'_, str> {
        if self.conn_name.is_empty() {
            std::borrow::Cow::Borrowed("NoSQL")
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
                if self.state.conn_id.as_deref() != Some(&id) || self.cmd.is_none() {
                    self.state.conn_id = Some(id);
                    self.state.database = None;
                    self.state.container = None;
                    self.connect(cx.services);
                }
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
            c("nosql.run", "NoSQL: Run the Query"),
            c("nosql.count", "NoSQL: Count Matches"),
            c("nosql.insert", "NoSQL: New Document"),
            c("nosql.refresh", "NoSQL: Refresh Containers"),
            c("nosql.reconnect", "NoSQL: Reconnect"),
            c("nosql.ai", "NoSQL: AI Assistant"),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "nosql.ai" => {
                self.assistant.toggle("NoSQL");
                true
            }
            "nosql.run" => {
                self.run_find();
                true
            }
            "nosql.count" => {
                if let Some(c) = self.container() {
                    self.send(Cmd::Count {
                        container: c,
                        filter: self.state.filter.clone(),
                    });
                }
                true
            }
            "nosql.insert" => {
                self.editing = Some((
                    Document::new(serde_json::json!({}), "", None),
                    "{\n  \n}".to_string(),
                ));
                self.inserting = true;
                true
            }
            "nosql.refresh" => {
                self.send(Cmd::Databases);
                true
            }
            "nosql.reconnect" => {
                self.connect(cx.services);
                true
            }
            _ => false,
        }
    }

    fn printable(&self) -> Option<Printable> {
        let sh = self.shared.lock();
        if sh.docs.is_empty() {
            return None;
        }
        let values: Vec<serde_json::Value> = sh.docs.iter().map(|d| d.json.clone()).collect();
        let columns = flatten::columns(&values);
        let rows: Vec<Vec<String>> = flatten::rows(&values, &columns)
            .into_iter()
            .map(|r| r.into_iter().map(|c| c.text).collect())
            .collect();
        Some(Printable {
            title: self
                .container()
                .map(|c| c.display())
                .unwrap_or_else(|| self.conn_name.clone()),
            subtitle: self.kind.map(|k| k.label().to_string()).unwrap_or_default(),
            html: hs_core::html::table(&columns, &rows),
        })
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if form::sidebar_header(
            ui,
            &theme,
            &s.t("nosql-connections"),
            &[(ph::PLUS, &s.t("nosql-new-profile"))],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:connections".into(),
            });
        }
        // One pass over the store, not one per kind, so the list is in the
        // store's own order - which is what a dragged row renumbers.
        let profiles: Vec<(String, String, String, bool)> = {
            let conns = s.connections.read();
            conns
                .all()
                .iter()
                .filter(|c| KINDS.contains(&c.kind.as_str()))
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        NoSqlFields::from_table(&c.kind, &c.fields).label(),
                        c.pinned,
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("nosql-no-profiles"));
            return true;
        }
        egui::ScrollArea::vertical()
            .id_salt("nosql-connections")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("nosql-profiles");
                let mut open_profile: Option<(String, bool)> = None;
                let mut pin: Option<(String, bool)> = None;
                // Edit This Connection takes the user to the profile's own
                // form in Settings, selected and ready: one place manages a
                // connection, and the sidebar is where they are looking.
                let mut edit: Option<String> = None;
                for (i, (id, name, label, is_pinned)) in profiles.iter().enumerate() {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            selected,
                            ph::TREE_STRUCTURE,
                            name,
                            Some(label),
                            *is_pinned,
                            form::RowMenuLabels {
                                pin: &s.t("conn-pin-home"),
                                unpin: &s.t("conn-unpin-home"),
                                edit: &s.t("conn-edit"),
                            },
                        );
                        if row.response.tip(s.t("nosql-open-profile")).clicked() {
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
                    if let Err(e) = s.connections.write().reorder(KINDS, from, to) {
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
                    cx.open("nosql", Some(OpenRequest::Connection { id }));
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
        for (tab, icon, key) in [
            (Tab::Find, ph::MAGNIFYING_GLASS, "nosql-tab-find"),
            (Tab::Aggregate, ph::FUNNEL, "nosql-tab-aggregate"),
            (Tab::Indexes, ph::LIST_BULLETS, "nosql-tab-indexes"),
        ] {
            let label = format!("{icon} {}", s.t(key));
            if ui
                .add(egui::Button::selectable(self.state.tab == tab, label))
                .clicked()
            {
                self.state.tab = tab;
                if tab == Tab::Indexes
                    && let Some(c) = self.container()
                {
                    self.send(Cmd::Indexes { container: c });
                }
            }
        }
        if let Some(c) = self.container() {
            ui.separator();
            ui.label(
                egui::RichText::new(c.display())
                    .small()
                    .color(theme.text_muted),
            );
        }
        if self.protected && !self.writes_unlocked {
            ui.separator();
            let label = s.i18n.t_args("nosql-protected", &[("env", &self.env_name)]);
            if ui
                .button(label)
                .on_hover_text(s.t("nosql-unlock-hint"))
                .clicked()
            {
                self.writes_unlocked = true;
            }
        }
        let status = self.shared.lock().status.clone();
        hs_ui::toolbar::hard_right(ui, |ui| {
            // The assistant is the same control in every tool, so it sits in
            // the same place: hard right of the toolbar.
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .on_hover_text(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("NoSQL");
            }
            if let Some(Status::Ready { kind, version }) = status {
                ui.label(
                    egui::RichText::new(format!("{} {version}", kind.label()))
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

impl NoSqlTool {
    /// The home tab: what this activity has open, with the last picture of
    /// each pane. Owner's requirement, 2026-10-17.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let count = s
            .connections
            .read()
            .all()
            .iter()
            .filter(|c| KINDS.contains(&c.kind.as_str()))
            .count();
        let tabs = s.open_tabs.of("nosql");
        hs_ui::form::page_title(
            ui,
            &theme,
            &s.t("tool-nosql"),
            Some(&s.t("nosql-dashboard-sub")),
        );
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(ui, &theme, &count.to_string(), &s.t("dash-connections"));
            hs_ui::dashboard::stat(ui, &theme, &tabs.len().to_string(), &s.t("dash-open-tabs"));
        });
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        egui::ScrollArea::vertical()
            .id_salt("nosql-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions
                        .push(hs_core::tool::Action::FocusTab { tab_id: id });
                }
            });
    }

    fn body(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // The activity's home tab lists what is open rather than being
        // one of them.
        if cx.is_home {
            self.dashboard(ui, cx);
            return;
        }
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
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
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("nosql-pick-hint"));
            return;
        }
        if let Some(e) = self.conn_error.clone() {
            form::note(ui, &theme, ph::WARNING, &e);
            if ui.button(s.t("nosql-retry")).clicked() {
                self.connect(s);
            }
            return;
        }
        let status = self.shared.lock().status.clone();
        match status {
            Some(Status::Connecting) | None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("nosql-connecting"));
                });
                return;
            }
            Some(Status::Failed(e)) => {
                form::note(ui, &theme, ph::WARNING, &e);
                if ui.button(s.t("nosql-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            Some(Status::Ready { .. }) => {}
        }
        self.dialogs(ui, cx);
        let total = ui.available_width();
        let tree_width = TREE_WIDTH.min(total * 0.4);
        let main_width = (total - tree_width - 40.0).max(240.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(tree_width);
                self.container_tree(ui, &theme, s);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(main_width);
                match self.state.tab {
                    Tab::Find => self.find_tab(ui, &theme, cx),
                    Tab::Aggregate => self.aggregate_tab(ui, &theme, cx),
                    Tab::Indexes => self.indexes_tab(ui, &theme, s),
                }
            });
        });
    }

    /// Everything the assistant is told about this tab, and the column it
    /// is drawn in: the store, the open container and the query on screen.
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
        let (status, summary) = {
            let sh = self.shared.lock();
            (sh.status.clone(), sh.summary.clone())
        };
        let store = match &status {
            Some(Status::Ready { kind, version }) => format!("{} {version}", kind.label()),
            _ => "(unknown)".into(),
        };
        let query = match self.state.tab {
            Tab::Aggregate => format!("Pipeline:\n{}", self.state.pipeline),
            _ => format!(
                "Filter: {}\nProjection: {}\nSort: {}\nLimit: {}  Skip: {}",
                self.state.filter,
                self.state.projection,
                self.state.sort,
                self.state.limit,
                self.state.skip
            ),
        };
        let context = format!(
            "Connection: {}\nStore: {store}\nDatabase: {}\nContainer: {}\n\n{query}\n\nLast result: {}",
            self.conn_name,
            self.state.database.as_deref().unwrap_or("(none)"),
            self.state.container.as_deref().unwrap_or("(none)"),
            summary
                .map(|f| format!("{} documents in {} ms", f.returned, f.elapsed.as_millis()))
                .unwrap_or_else(|| "(nothing run yet)".into()),
        );
        self.assistant.set_context("NoSQL query", context);
        if self.assistant.quick_actions.is_empty() {
            self.assistant.quick_actions =
                hs_ui::ai_assistant::QuickAction::for_activity(s, "nosql");
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
                // Review only (ADR-019): a filter or pipeline it wrote lands
                // in the field, and the user presses Run.
                self.assistant.can_insert = true;
                let out = self.assistant.ui(ui, s, &theme, "NoSQL");
                if let Some(block) = out.use_block {
                    match self.state.tab {
                        Tab::Aggregate => self.state.pipeline = block.trim().to_string(),
                        _ => self.state.filter = block.trim().to_string(),
                    }
                }
            });
    }

    /// Databases, and the containers inside the open ones.
    fn container_tree(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let (databases, containers) = {
            let sh = self.shared.lock();
            (sh.databases.clone(), sh.containers.clone())
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("nosql-containers"))
                    .small()
                    .color(theme.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::icon_button(ui, theme, ph::ARROWS_CLOCKWISE, 14.0, false)
                    .tip(s.t("nosql-refresh"))
                    .clicked()
                {
                    self.send(Cmd::Databases);
                }
            });
        });
        let dbs = match &databases {
            Load::Ready(d) => d.clone(),
            Load::Failed(e) => {
                form::note(ui, theme, ph::WARNING, e);
                return;
            }
            _ => {
                ui.spinner();
                return;
            }
        };
        let mut open_db: Option<String> = None;
        let mut pick: Option<(String, String)> = None;
        egui::ScrollArea::vertical()
            .id_salt("nosql-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for db in &dbs {
                    let expanded = self.open_dbs.contains(db);
                    let icon = if expanded {
                        ph::CARET_DOWN
                    } else {
                        ph::CARET_RIGHT
                    };
                    if ui
                        .add(egui::Button::selectable(
                            self.state.database.as_deref() == Some(db.as_str()),
                            format!("{icon} {db}"),
                        ))
                        .clicked()
                    {
                        open_db = Some(db.clone());
                    }
                    if !expanded {
                        continue;
                    }
                    match containers.get(db) {
                        Some(Load::Ready(list)) => {
                            for c in list {
                                let selected = self.state.database.as_deref() == Some(db.as_str())
                                    && self.state.container.as_deref() == Some(c.name.as_str());
                                ui.horizontal(|ui| {
                                    ui.add_space(14.0);
                                    let label = match c.count {
                                        Some(n) => format!("{} {} ({n})", ph::TABLE, c.name),
                                        None => format!("{} {}", ph::TABLE, c.name),
                                    };
                                    if ui.add(egui::Button::selectable(selected, label)).clicked() {
                                        pick = Some((db.clone(), c.name.clone()));
                                    }
                                });
                            }
                            if list.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.add_space(14.0);
                                    form::hint(ui, theme, &s.t("nosql-no-containers"));
                                });
                            }
                        }
                        Some(Load::Failed(e)) => {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                form::hint(ui, theme, e);
                            });
                        }
                        _ => {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                ui.spinner();
                            });
                        }
                    }
                }
            });
        if let Some(db) = open_db {
            if self.open_dbs.contains(&db) {
                self.open_dbs.remove(&db);
            } else {
                self.open_dbs.insert(db.clone());
                if !containers.contains_key(&db) {
                    self.send(Cmd::Containers { database: db });
                }
            }
        }
        if let Some((db, name)) = pick {
            self.state.database = Some(db);
            self.state.container = Some(name);
            self.state.skip = 0;
            self.run_find();
        }
    }

    /// Filter, projection, sort, paging and Run.
    fn find_tab(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        if self.container().is_none() {
            form::note(ui, theme, ph::TABLE, &s.t("nosql-pick-container"));
            return;
        }
        let label = self
            .kind
            .map(|k| k.query_label())
            .unwrap_or("filter")
            .to_string();
        // Each store's filter box takes its own language, so the
        // example follows the store rather than showing Mongo's to
        // everyone.
        let example = self
            .kind
            .map(|k| k.filter_example())
            .unwrap_or(r#"{"status": "failed"}"#);
        let extras = self.kind.is_none_or(|k| k.has_projection_and_sort());
        egui::Grid::new("nosql-query")
            .num_columns(2)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                ui.label(label);
                ui.add(
                    egui::TextEdit::singleline(&mut self.state.filter)
                        .hint_text(example)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                ui.end_row();
                // Qdrant has neither: its scroll returns whole points
                // in id order, and a box the store ignores is an
                // invitation to type something that does nothing.
                if extras {
                    ui.label(s.t("nosql-projection"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.state.projection)
                            .hint_text(r#"{"total": 1}"#)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.end_row();
                    ui.label(s.t("nosql-sort"));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.state.sort)
                            .hint_text(r#"{"createdAt": -1}"#)
                            .desired_width(f32::INFINITY)
                            .font(egui::TextStyle::Monospace),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if form::primary_button(ui, theme, &s.t("nosql-run")).clicked() {
                self.run_find();
            }
            if ui.button(s.t("nosql-count")).clicked()
                && let Some(c) = self.container()
            {
                self.send(Cmd::Count {
                    container: c,
                    filter: self.state.filter.clone(),
                });
            }
            ui.label(s.t("nosql-limit"));
            let mut limit = if self.state.limit > 0 {
                self.state.limit
            } else {
                DEFAULT_LIMIT
            };
            if ui
                .add(egui::DragValue::new(&mut limit).range(1..=1000))
                .changed()
            {
                self.state.limit = limit;
            }
            if ui
                .button("‹")
                .on_hover_text(s.t("nosql-previous"))
                .clicked()
            {
                self.state.skip = (self.state.skip - limit).max(0);
                self.run_find();
            }
            ui.label(format!("{}", self.state.skip));
            if ui.button("›").on_hover_text(s.t("nosql-next")).clicked() {
                self.state.skip += limit;
                self.run_find();
            }
            if ui.button(s.t("nosql-new-document")).clicked() {
                self.editing = Some((
                    Document::new(serde_json::json!({}), "", None),
                    "{\n  \n}".to_string(),
                ));
                self.inserting = true;
            }
            if ui.button(s.t("nosql-delete-matching")).clicked() {
                self.popup = Some(Popup::DeleteMany {
                    filter: self.state.filter.clone(),
                    typed: String::new(),
                });
                if let Some(c) = self.container() {
                    self.send(Cmd::DeleteMany {
                        container: c,
                        filter: self.state.filter.clone(),
                        dry_run: true,
                    });
                }
            }
        });
        ui.add_space(4.0);
        self.results(ui, theme, cx);
    }

    /// The documents a run returned, and the editor under them.
    fn results(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let (docs, summary, finding, count, run_id) = {
            let sh = self.shared.lock();
            (
                sh.docs.clone(),
                sh.summary.clone(),
                sh.finding,
                sh.count,
                sh.run_id,
            )
        };
        if run_id != self.seen_run {
            self.seen_run = run_id;
            self.table = TableState::default();
        }
        ui.horizontal(|ui| {
            for (view, key) in [
                (View::Table, "nosql-view-table"),
                (View::Tree, "nosql-view-tree"),
                (View::Json, "nosql-view-json"),
            ] {
                if ui
                    .add(egui::Button::selectable(self.state.view == view, s.t(key)))
                    .clicked()
                {
                    self.state.view = view;
                }
            }
            if finding {
                ui.spinner();
            }
            if let Some(sum) = &summary {
                let text = s.i18n.t_args(
                    "nosql-summary",
                    &[
                        ("n", &sum.returned.to_string()),
                        ("ms", &sum.elapsed.as_millis().to_string()),
                    ],
                );
                ui.label(egui::RichText::new(text).small().color(theme.text_muted));
                if sum.more {
                    ui.label(
                        egui::RichText::new(s.t("nosql-more"))
                            .small()
                            .color(theme.warning),
                    );
                }
            }
            if let Some(n) = count {
                ui.label(
                    egui::RichText::new(s.i18n.t_args("nosql-count-is", &[("n", &n.to_string())]))
                        .small()
                        .color(theme.text_muted),
                );
            }
        });
        if docs.is_empty() {
            form::hint(ui, theme, &s.t("nosql-no-documents"));
        }
        let values: Vec<serde_json::Value> = docs.iter().map(|d| d.json.clone()).collect();
        let height = (ui.available_height() * 0.55).max(120.0);
        egui::ScrollArea::both()
            .id_salt("nosql-results")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| match self.state.view {
                View::Table => {
                    let columns = flatten::columns(&values);
                    let specs: Vec<ColumnSpec> = columns
                        .iter()
                        .map(|c| ColumnSpec::new(c.clone(), 160.0))
                        .collect();
                    let rows: Vec<Vec<String>> = flatten::rows(&values, &columns)
                        .into_iter()
                        .map(|r| {
                            r.into_iter()
                                .map(|c| if c.missing { String::new() } else { c.text })
                                .collect()
                        })
                        .collect();
                    let table = DataTable {
                        theme,
                        columns: &specs,
                        rows: &rows,
                        row_height: 22.0,
                        show_filter: false,
                        copy_row_label: &s.t("nosql-copy-row"),
                        built_in_menu: true,
                        row_icons: None,
                    };
                    let resp = table.show(ui, &mut self.table);
                    if let Some(text) = resp.copied {
                        ui.ctx().copy_text(text);
                    }
                    if let Some(i) = resp.clicked.or(resp.double_clicked)
                        && let Some(d) = docs.get(i)
                    {
                        self.editing = Some((d.clone(), flatten::pretty(&d.json)));
                        self.inserting = false;
                    }
                }
                View::Tree => {
                    for (i, d) in docs.iter().enumerate() {
                        let title = if d.id.is_empty() {
                            format!("#{}", i + 1)
                        } else {
                            d.id.clone()
                        };
                        egui::CollapsingHeader::new(title)
                            .id_salt(("nosql-doc", i))
                            .show(ui, |ui| {
                                for (path, value) in flatten::flatten(&d.json) {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(path)
                                                .small()
                                                .color(theme.text_muted),
                                        );
                                        ui.label(egui::RichText::new(value).small().monospace());
                                    });
                                }
                                if ui.button(s.t("nosql-edit")).clicked() {
                                    self.editing = Some((d.clone(), flatten::pretty(&d.json)));
                                    self.inserting = false;
                                }
                            });
                    }
                }
                View::Json => {
                    let mut text = docs
                        .iter()
                        .map(|d| flatten::pretty(&d.json))
                        .collect::<Vec<_>>()
                        .join(",\n");
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .code_editor()
                            .interactive(false)
                            .desired_width(f32::INFINITY),
                    );
                }
            });
        // The editor for whichever document is open.
        let Some((doc, text)) = &mut self.editing else {
            return;
        };
        ui.separator();
        let doc = doc.clone();
        ui.horizontal(|ui| {
            let title = if self.inserting {
                s.t("nosql-new-document")
            } else {
                doc.id.clone()
            };
            ui.label(egui::RichText::new(title).strong());
            if let Some(rev) = &doc.rev {
                ui.label(egui::RichText::new(rev).small().color(theme.text_muted));
            }
        });
        let mut save = false;
        let mut delete = false;
        let mut close = false;
        egui::ScrollArea::vertical()
            .id_salt("nosql-editor")
            .max_height(ui.available_height() - 36.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(8),
                );
            });
        ui.horizontal(|ui| {
            if form::primary_button(ui, theme, &s.t("save")).clicked() {
                save = true;
            }
            if !self.inserting && form::danger_button(ui, theme, &s.t("nosql-delete")).clicked() {
                delete = true;
            }
            if ui.button(s.t("cancel")).clicked() {
                close = true;
            }
        });
        if save {
            let text = self
                .editing
                .as_ref()
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(json) => {
                    let mode = if self.inserting {
                        PutMode::Insert
                    } else {
                        PutMode::Replace {
                            rev: doc.rev.clone(),
                        }
                    };
                    if let Some(c) = self.container() {
                        let doc = Document::new(json, doc.id.clone(), doc.rev.clone());
                        self.guarded(
                            cx,
                            Cmd::Save {
                                container: c,
                                doc,
                                mode,
                            },
                        );
                        self.editing = None;
                        self.inserting = false;
                    }
                }
                Err(e) => cx.toast(
                    ToastLevel::Error,
                    cx.services
                        .i18n
                        .t_args("nosql-bad-json", &[("error", &e.to_string())]),
                ),
            }
        }
        if delete {
            self.popup = Some(Popup::Delete { doc });
        }
        if close {
            self.editing = None;
            self.inserting = false;
        }
    }

    fn aggregate_tab(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        if self.container().is_none() {
            form::note(ui, theme, ph::TABLE, &s.t("nosql-pick-container"));
            return;
        }
        if self.kind != Some(StoreKind::Mongo) {
            form::note(ui, theme, ph::INFO, &s.t("nosql-aggregate-mongo-only"));
            return;
        }
        form::hint(ui, theme, &s.t("nosql-pipeline-hint"));
        ui.add(
            egui::TextEdit::multiline(&mut self.state.pipeline)
                .code_editor()
                .hint_text(
                    r#"[{"$match": {"status": "failed"}}, {"$group": {"_id": "$customer"}}]"#,
                )
                .desired_width(f32::INFINITY)
                .desired_rows(6),
        );
        ui.add_space(4.0);
        if form::primary_button(ui, theme, &s.t("nosql-run")).clicked()
            && let Some(c) = self.container()
        {
            self.editing = None;
            self.send(Cmd::Aggregate {
                container: c,
                pipeline: self.state.pipeline.clone(),
                limit: if self.state.limit > 0 {
                    self.state.limit
                } else {
                    DEFAULT_LIMIT
                },
            });
        }
        ui.add_space(4.0);
        self.results(ui, theme, cx);
    }

    fn indexes_tab(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        if self.container().is_none() {
            form::note(ui, theme, ph::TABLE, &s.t("nosql-pick-container"));
            return;
        }
        let indexes = self.shared.lock().indexes.clone();
        match indexes {
            Load::Ready(list) => {
                egui::Grid::new("nosql-indexes")
                    .num_columns(3)
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(s.t("nosql-index-name")).strong());
                        ui.label(egui::RichText::new(s.t("nosql-index-definition")).strong());
                        ui.label(egui::RichText::new(s.t("nosql-index-unique")).strong());
                        ui.end_row();
                        for i in list {
                            ui.label(i.name);
                            ui.label(egui::RichText::new(i.definition).monospace().small());
                            ui.label(if i.unique { "✓" } else { "" });
                            ui.end_row();
                        }
                    });
            }
            Load::Failed(e) => form::note(ui, theme, ph::WARNING, &e),
            _ => {
                ui.spinner();
                if let Some(c) = self.container() {
                    self.send(Cmd::Indexes { container: c });
                }
            }
        }
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let Some(popup) = self.popup.take() else {
            return;
        };
        let mut keep = Some(popup);
        match keep.as_mut().expect("just set") {
            Popup::Delete { doc } => {
                let doc = doc.clone();
                let mut go = false;
                let mut close = false;
                egui::Modal::new(egui::Id::new("nosql-delete")).show(ui.ctx(), |ui| {
                    ui.set_width(420.0);
                    ui.heading(s.t("nosql-delete"));
                    ui.add_space(6.0);
                    ui.label(s.i18n.t_args("nosql-delete-body", &[("id", &doc.id)]));
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if form::danger_button(ui, &theme, &s.t("nosql-delete")).clicked() {
                            go = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            close = true;
                        }
                    });
                });
                if go {
                    if let Some(c) = self.container() {
                        self.guarded(cx, Cmd::Delete { container: c, doc });
                    }
                    self.editing = None;
                    close = true;
                }
                if close {
                    keep = None;
                }
            }
            Popup::DeleteMany { filter, typed } => {
                let bulk = self.shared.lock().bulk.clone();
                let mut go = false;
                let mut close = false;
                let filter = filter.clone();
                egui::Modal::new(egui::Id::new("nosql-delete-many")).show(ui.ctx(), |ui| {
                    ui.set_width(460.0);
                    ui.heading(s.t("nosql-delete-matching"));
                    ui.add_space(6.0);
                    match &bulk {
                        Some(b) => {
                            ui.label(s.i18n.t_args(
                                "nosql-delete-many-body",
                                &[
                                    ("n", &b.matched.to_string()),
                                    (
                                        "container",
                                        &self.container().map(|c| c.display()).unwrap_or_default(),
                                    ),
                                    ("env", &self.env_name),
                                ],
                            ));
                        }
                        None => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(s.t("nosql-counting"));
                            });
                        }
                    }
                    if self.protected {
                        ui.add_space(8.0);
                        ui.label(s.i18n.t_args("nosql-type-env", &[("env", &self.env_name)]));
                        ui.text_edit_singleline(typed);
                    }
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let allowed = bulk.as_ref().is_some_and(|b| b.matched > 0)
                            && (!self.protected || typed.as_str() == self.env_name);
                        if ui
                            .add_enabled(allowed, egui::Button::new(s.t("nosql-delete")))
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
                    if let Some(c) = self.container() {
                        self.guarded(
                            cx,
                            Cmd::DeleteMany {
                                container: c,
                                filter,
                                dry_run: false,
                            },
                        );
                    }
                    close = true;
                }
                if close {
                    self.shared.lock().bulk = None;
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

    /// Every kind this activity claims must map to a store it can
    /// actually open, or the sidebar lists a connection that goes
    /// nowhere when clicked.
    #[test]
    fn every_kind_the_tool_lists_is_one_it_can_open() {
        assert_eq!(KINDS, &["mongodb", "couchdb", "qdrant"]);
        assert_eq!(DESCRIPTOR.id, "nosql");
        for k in KINDS {
            let f = crate::profile::NoSqlFields {
                kind: (*k).to_string(),
                ..Default::default()
            };
            let store = f.store_kind().unwrap_or_else(|| panic!("{k} has no store"));
            assert_eq!(
                store.connection_kind(),
                *k,
                "the kind and the store must agree about the name"
            );
        }
    }

    /// Each store's filter box takes its own language; showing Mongo's
    /// example over a Qdrant box is an instruction that does not work.
    #[test]
    fn each_store_shows_its_own_filter_example() {
        use crate::store::StoreKind;
        let mut seen: Vec<&str> = [StoreKind::Mongo, StoreKind::Couch, StoreKind::Qdrant]
            .iter()
            .map(|k| k.filter_example())
            .collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 3, "no two stores share an example");
        assert!(StoreKind::Qdrant.filter_example().contains("must"));
        // Qdrant has neither a projection nor a sort.
        assert!(!StoreKind::Qdrant.has_projection_and_sort());
        assert!(StoreKind::Mongo.has_projection_and_sort());
    }

    #[test]
    fn a_query_falls_back_to_the_default_page_size() {
        let mut t = NoSqlTool::new(State::default());
        assert_eq!(t.query().limit, DEFAULT_LIMIT);
        t.state.limit = 10;
        t.state.skip = -5;
        let q = t.query();
        assert_eq!(q.limit, 10);
        assert_eq!(q.skip, 0, "a negative skip is nonsense");
    }

    #[test]
    fn a_container_needs_both_a_database_and_a_name() {
        let mut t = NoSqlTool::new(State::default());
        assert!(t.container().is_none());
        t.state.database = Some("shop".into());
        assert!(t.container().is_none());
        t.state.container = Some("orders".into());
        assert_eq!(t.container().unwrap().display(), "shop.orders");
    }

    #[test]
    fn writes_are_refused_while_the_profile_is_read_only() {
        let mut t = NoSqlTool::new(State::default());
        assert!(!t.read_only());
        t.read_only_profile = true;
        assert!(t.read_only());
        t.read_only_profile = false;
        t.protected = true;
        assert!(t.read_only(), "a protected environment locks writes");
        t.writes_unlocked = true;
        assert!(!t.read_only());
    }
}
