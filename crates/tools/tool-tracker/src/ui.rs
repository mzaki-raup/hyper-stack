//! The tab (docs/tools/tracker.md §3): the dashboard on the home tab, and
//! for a tracker its query and items on the left and the selected item on
//! the right - or, for Confluence, spaces and pages. Everything the UI
//! shows arrives through [`crate::runner::Shared`]; nothing here talks to
//! a server.

use std::collections::HashMap;
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

use crate::driver::{self, Detail, Edit, NewItem, Page, PageRef};
use crate::profile::{ProfileError, TrackerFields, wants_repo};
use crate::runner::{self, Cmd, Shared, Status};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "tracker",
    title_key: "tool-tracker",
    icon: ph::KANBAN,
    category: ToolCategory::Infra,
    accepts: &[Accept::Connection],
};

/// What every open tracker tab last counted, by connection id: (assigned
/// to the user, due within a week). The dashboard's figures come from the
/// trackers open in tabs only (docs/tools/tracker.md §3).
static FIGURES: LazyLock<Mutex<HashMap<String, (u64, usize)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct TrackerFactory;

impl ToolFactory for TrackerFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = TrackerTool::new(state);
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores. Never a secret, never a draft.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    pub query: String,
    pub item: Option<String>,
    /// Confluence: the space browsed, by key, and the page open, by id.
    pub space: Option<String>,
    pub page: Option<String>,
    /// Confluence: the left pane shows search results, not the tree.
    pub searching: bool,
}

/// What the right pane shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Pane {
    #[default]
    View,
    Status,
    Edit,
    New,
}

/// The Edit form: what the item had, and what the user has made of it.
#[derive(Debug, Clone, Default)]
struct EditDraft {
    key: String,
    was: Fields,
    now: Fields,
    lossy: Option<String>,
    review: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Fields {
    summary: String,
    description: String,
    assignee: String,
    priority: String,
    labels: String,
    due: String,
}

impl Fields {
    fn of(d: &Detail) -> Fields {
        Fields {
            summary: d.item.summary.clone(),
            description: d.description.clone(),
            assignee: d.item.assignee.clone(),
            priority: d.item.priority.clone(),
            labels: d.labels.join(", "),
            due: d.due.clone(),
        }
    }
}

/// Only what changed, so an edit never writes a field back as it was.
fn diff_edit(was: &Fields, now: &Fields) -> Edit {
    let changed = |a: &str, b: &str| (a.trim() != b.trim()).then(|| b.trim().to_string());
    Edit {
        summary: changed(&was.summary, &now.summary),
        description: (was.description.trim_end() != now.description.trim_end())
            .then(|| now.description.clone()),
        assignee: changed(&was.assignee, &now.assignee),
        priority: changed(&was.priority, &now.priority),
        labels: changed(&was.labels, &now.labels).map(|l| crate::github::split_list(&l)),
        due: changed(&was.due, &now.due),
    }
}

/// A page being edited.
#[derive(Debug, Clone, Default)]
struct PageDraft {
    page: Page,
    body: String,
    review: bool,
}

pub struct TrackerTool {
    state: State,
    shared: Arc<Mutex<Shared>>,
    cmd: Option<mpsc::Sender<Cmd>>,
    conn_name: String,
    kind: String,
    address: String,
    queries: Vec<String>,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    read_only_profile: bool,
    protected: bool,
    writes_unlocked: bool,
    env_name: String,
    items_table: TableState,
    last_poll: Option<Instant>,
    query_text: String,
    focus_query: bool,
    pane: Pane,
    edit: Option<EditDraft>,
    new: Option<NewItem>,
    page_edit: Option<PageDraft>,
    comment: String,
    focus_comment: bool,
    /// The item or page last asked for, so a restored tab loads it once.
    requested: Option<String>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl Drop for TrackerTool {
    fn drop(&mut self) {
        if let Some(id) = &self.state.conn_id {
            FIGURES.lock().remove(id);
        }
    }
}

/// A field's name as the item pane shows it.
fn field_key(name: &str) -> String {
    format!("tracker-field-{name}")
}

/// The page tree, depth first, with each page's depth.
fn tree_order(pages: &[PageRef]) -> Vec<(usize, &PageRef)> {
    let ids: std::collections::HashSet<&str> = pages.iter().map(|p| p.id.as_str()).collect();
    let mut children: HashMap<&str, Vec<&PageRef>> = HashMap::new();
    for p in pages {
        // A parent outside the listing (or none) puts the page at the top.
        let parent = if ids.contains(p.parent.as_str()) {
            p.parent.as_str()
        } else {
            ""
        };
        children.entry(parent).or_default().push(p);
    }
    for v in children.values_mut() {
        v.sort_by_key(|p| p.title.to_lowercase());
    }
    let mut out = Vec::with_capacity(pages.len());
    let mut stack: Vec<(usize, &PageRef)> = children
        .get("")
        .map(|v| v.iter().rev().map(|p| (0, *p)).collect())
        .unwrap_or_default();
    while let Some((depth, p)) = stack.pop() {
        out.push((depth, p));
        if let Some(kids) = children.get(p.id.as_str()) {
            stack.extend(kids.iter().rev().map(|k| (depth + 1, *k)));
        }
    }
    out
}

/// Markdown as the item pane draws it: prose through the shared
/// renderer, fenced code as code.
fn markdown(ui: &mut egui::Ui, text: &str, theme: &Theme) {
    for seg in hs_ui::ai_assistant::split_fences(text) {
        match seg {
            hs_ui::ai_assistant::Segment::Text(t) => {
                hs_ui::ai_assistant::markdown_text(ui, &t, theme)
            }
            hs_ui::ai_assistant::Segment::Code { body, .. } => {
                egui::Frame::new()
                    .fill(theme.panel_bg)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(body).monospace())
                                .selectable(true),
                        );
                    });
            }
        }
    }
}

/// A line diff, for Edit's description and a page's Publish.
fn line_diff(ui: &mut egui::Ui, theme: &Theme, old: &str, new: &str) {
    // A last line without its newline is not a change to that line.
    let end = |s: &str| format!("{}\n", s.trim_end_matches(['\r', '\n']));
    let (old, new) = (end(old), end(new));
    let diff = similar::TextDiff::from_lines(&old, &new);
    for op in diff.ops() {
        for change in diff.iter_changes(op) {
            let (sign, colour) = match change.tag() {
                similar::ChangeTag::Delete => ("-", theme.error),
                similar::ChangeTag::Insert => ("+", theme.success),
                similar::ChangeTag::Equal => continue,
            };
            ui.colored_label(
                colour,
                egui::RichText::new(format!("{sign} {}", change.value().trim_end())).monospace(),
            );
        }
    }
}

/// Today and a week from today, as `YYYY-MM-DD`, for "due this week".
fn week() -> (String, String) {
    let today = chrono::Local::now().date_naive();
    (
        today.format("%Y-%m-%d").to_string(),
        (today + chrono::Days::new(7))
            .format("%Y-%m-%d")
            .to_string(),
    )
}

impl TrackerTool {
    fn new(state: State) -> TrackerTool {
        TrackerTool {
            query_text: state.query.clone(),
            state,
            shared: Arc::new(Mutex::new(Shared::default())),
            cmd: None,
            conn_name: String::new(),
            kind: String::new(),
            address: String::new(),
            queries: Vec::new(),
            conn_error: None,
            waiting_for_vault: false,
            read_only_profile: false,
            protected: false,
            writes_unlocked: false,
            env_name: String::new(),
            items_table: TableState::default(),
            last_poll: None,
            focus_query: false,
            pane: Pane::View,
            edit: None,
            new: None,
            page_edit: None,
            comment: String::new(),
            focus_comment: false,
            requested: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        }
    }

    fn read_only(&self) -> bool {
        self.read_only_profile || (self.protected && !self.writes_unlocked)
    }

    fn is_pages(&self) -> bool {
        self.kind == "confluence"
    }

    fn connect(&mut self, s: &Services) {
        self.cmd = None;
        self.shared = Arc::new(Mutex::new(Shared::default()));
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.last_poll = None;
        self.requested = None;
        self.pane = Pane::View;
        self.edit = None;
        self.new = None;
        self.page_edit = None;
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (kind, fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("tracker-no-profile"));
                return;
            };
            (
                c.kind.clone(),
                TrackerFields::from_table(&c.kind, &c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.kind = kind.clone();
        self.address = fields.label_for(&kind);
        self.queries = fields.queries.clone();
        self.read_only_profile = read_only;
        {
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
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let cfg = match fields.resolve(&kind, self.read_only(), s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault =
                    e == ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked);
                self.conn_error = Some(match e {
                    ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked) => {
                        s.t("tracker-vault-locked")
                    }
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("tracker-missing-vars", &[("names", &v.join(", "))]),
                    other => other.to_string(),
                });
                return;
            }
        };
        tracing::info!(
            "tracker: connecting {} to {} {}",
            self.conn_name,
            kind,
            self.address
        );
        self.cmd = Some(runner::spawn(&s.runtime, cfg, self.shared.clone()));
        // The first search waits for the driver's default query, unless
        // the tab was restored with one of its own.
        if !self.state.query.is_empty() {
            let q = self.state.query.clone();
            self.send(Cmd::Search(q));
            self.last_poll = Some(Instant::now());
        }
    }

    fn send(&mut self, cmd: Cmd) -> bool {
        match &self.cmd {
            Some(tx) => tx.try_send(cmd).is_ok(),
            None => false,
        }
    }

    fn search(&mut self) {
        self.state.query = self.query_text.trim().to_string();
        self.last_poll = Some(Instant::now());
        let q = self.state.query.clone();
        self.send(Cmd::Search(q));
    }

    fn refresh(&mut self) {
        self.last_poll = Some(Instant::now());
        self.send(Cmd::Refresh);
        if let Some(key) = self.state.item.clone()
            && self.pane == Pane::View
            && !self.is_pages()
        {
            self.send(Cmd::Item(key));
        }
    }

    fn detail(&self) -> Option<Detail> {
        let key = self.state.item.as_ref()?;
        let sh = self.shared.lock();
        let d = sh.detail.as_ref().filter(|d| &d.key == key)?;
        d.result.as_ref().ok().cloned()
    }

    fn page(&self) -> Option<Page> {
        let id = self.state.page.as_ref()?;
        let sh = self.shared.lock();
        let p = sh.page.as_ref().filter(|p| &p.key == id)?;
        p.result.as_ref().ok().cloned()
    }

    fn select(&mut self, key: String) {
        if self.state.item.as_ref() != Some(&key) {
            self.comment.clear();
        }
        self.state.item = Some(key.clone());
        self.pane = Pane::View;
        self.edit = None;
        self.requested = Some(key.clone());
        self.send(Cmd::Item(key));
    }

    fn open_page(&mut self, id: String) {
        if self.state.page.as_ref() != Some(&id) {
            self.comment.clear();
        }
        self.state.page = Some(id.clone());
        self.page_edit = None;
        self.requested = Some(id.clone());
        self.send(Cmd::Page(id));
    }

    fn refusal(&self, s: &Services) -> String {
        if self.read_only_profile {
            s.t("tracker-refused-read-only")
        } else {
            s.t("tracker-refused-protected")
        }
    }

    fn guard(&self, cx: &mut ToolContext) -> bool {
        if self.read_only() {
            cx.toast(ToastLevel::Error, self.refusal(cx.services));
            return false;
        }
        true
    }

    fn open_status(&mut self, cx: &mut ToolContext) {
        if !self.guard(cx) || !self.shared.lock().caps.transition {
            return;
        }
        let Some(key) = self.state.item.clone() else {
            cx.toast(ToastLevel::Info, cx.services.t("tracker-select-first"));
            return;
        };
        self.pane = Pane::Status;
        self.send(Cmd::Transitions(key));
    }

    fn open_edit(&mut self, cx: &mut ToolContext) {
        if !self.guard(cx) {
            return;
        }
        if self.is_pages() {
            if let Some(p) = self.page()
                && p.read_only.is_none()
            {
                self.page_edit = Some(PageDraft {
                    body: p.body.clone(),
                    page: p,
                    review: false,
                });
            }
            return;
        }
        let Some(d) = self.detail() else {
            cx.toast(ToastLevel::Info, cx.services.t("tracker-select-first"));
            return;
        };
        self.edit = Some(EditDraft {
            key: d.item.key.clone(),
            was: Fields::of(&d),
            now: Fields::of(&d),
            lossy: d.lossy.clone(),
            review: false,
        });
        self.pane = Pane::Edit;
    }

    fn open_new(&mut self, cx: &mut ToolContext) {
        let caps = self.shared.lock().caps;
        if !caps.create || !self.guard(cx) {
            return;
        }
        let project = self
            .state
            .conn_id
            .as_ref()
            .and_then(|id| {
                cx.services
                    .connections
                    .read()
                    .by_id(id)
                    .map(|c| TrackerFields::from_table(&c.kind, &c.fields).project)
            })
            .unwrap_or_default();
        self.new = Some(NewItem {
            project: if wants_repo(&self.kind) || self.kind == "gitlab-issues" {
                String::new()
            } else {
                project.clone()
            },
            ..Default::default()
        });
        self.shared.lock().form = None;
        self.send(Cmd::Fields(project));
        self.pane = Pane::New;
    }

    fn open_browser(&mut self, cx: &mut ToolContext) {
        let url = if self.is_pages() {
            self.page().map(|p| p.url)
        } else {
            self.detail().map(|d| d.item.url).or_else(|| {
                let key = self.state.item.as_ref()?;
                let sh = self.shared.lock();
                sh.found
                    .as_ref()?
                    .items
                    .iter()
                    .find(|i| &i.key == key)
                    .map(|i| i.url.clone())
            })
        };
        if let Some(url) = url.filter(|u| !u.is_empty()) {
            cx.open("browser", Some(OpenRequest::Url { url }));
        }
    }

    /// Keep the saved queries in the profile, where every tab of this
    /// tracker finds them.
    fn save_queries(&mut self, s: &Services) -> Result<(), String> {
        let Some(id) = self.state.conn_id.clone() else {
            return Ok(());
        };
        let mut c = s
            .connections
            .read()
            .by_id(&id)
            .cloned()
            .ok_or_else(|| s.t("tracker-no-profile"))?;
        let mut f = TrackerFields::from_table(&c.kind, &c.fields);
        f.queries = self.queries.clone();
        f.write_into(&mut c.fields);
        s.connections.write().upsert(c).map_err(|e| e.to_string())
    }
}

impl Tool for TrackerTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn subtitle(&self) -> Option<String> {
        Some(self.address.clone()).filter(|s| !s.is_empty())
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> std::borrow::Cow<'_, str> {
        if self.conn_name.is_empty() {
            std::borrow::Cow::Borrowed("Tracker")
        } else {
            std::borrow::Cow::Borrowed(&self.conn_name)
        }
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

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
            // The kind is already in the band; the place is what is left.
            .fact(
                "Server",
                self.address
                    .split_once(" · ")
                    .map(|(_, place)| place)
                    .unwrap_or(&self.address)
                    .to_string(),
            ),
        )
    }

    fn tab_icon(&self) -> Option<&'static str> {
        (!self.kind.is_empty()).then(|| hs_ui::icons::connection_icon(&self.kind, ""))
    }

    fn on_event(&mut self, ev: &hs_core::events::Event, cx: &mut ToolContext) {
        match ev {
            hs_core::events::Event::ConnectionsChanged => {
                if let Some(id) = self.state.conn_id.clone()
                    && let Some(c) = cx.services.connections.read().by_id(&id)
                {
                    self.conn_name = c.name.clone();
                    self.queries = TrackerFields::from_table(&c.kind, &c.fields).queries;
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
                self.query_text.clear();
                self.comment.clear();
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
            c("tracker.refresh", "Tracker: Refresh", Some("run.alt")),
            c("tracker.search", "Tracker: Search…", Some("tracker.search")),
            c("tracker.new", "Tracker: New Item…", Some("tracker.new")),
            c(
                "tracker.transition",
                "Tracker: Change Status…",
                Some("tracker.transition"),
            ),
            c("tracker.edit", "Tracker: Edit…", None),
            c("tracker.comment", "Tracker: Comment…", None),
            c("tracker.browser", "Tracker: Open in Browser", None),
            c("tracker.add", "Tracker: Add Tracker", None),
            c("tracker.reconnect", "Tracker: Reconnect", None),
            c("tracker.ai", "Tracker: AI Assistant", None),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "tracker.refresh" | "run.alt" => {
                self.refresh();
                true
            }
            "tracker.search" => {
                if self.is_pages() {
                    self.state.searching = true;
                }
                self.focus_query = true;
                true
            }
            "tracker.new" => {
                self.open_new(cx);
                true
            }
            "tracker.transition" => {
                self.open_status(cx);
                true
            }
            "tracker.edit" => {
                self.open_edit(cx);
                true
            }
            "tracker.comment" => {
                if self.guard(cx) {
                    self.pane = Pane::View;
                    self.focus_comment = true;
                }
                true
            }
            "tracker.browser" => {
                self.open_browser(cx);
                true
            }
            "tracker.add" => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:tracker".into(),
                });
                true
            }
            "tracker.reconnect" => {
                self.connect(cx.services);
                true
            }
            "tracker.ai" => {
                self.assistant.toggle("Tracker");
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
            &s.t("tracker-trackers"),
            &[(ph::PLUS, &s.t("tracker-add"))],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:tracker".into(),
            });
        }
        let profiles: Vec<(String, String, String, bool, &'static str)> = {
            let conns = s.connections.read();
            conns
                .of_tracker_kinds()
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        TrackerFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                        c.pinned,
                        hs_ui::icons::connection_icon(&c.kind, ""),
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("tracker-none"));
            return true;
        }
        egui::ScrollArea::vertical()
            .id_salt("tracker-trackers")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("tracker-trackers");
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
                        if row.response.tip(s.t("tracker-open")).clicked() {
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
                            .reorder(hs_core::connections::TRACKER_KINDS, from, to)
                    {
                        cx.toast(ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
                if let Some((id, selected)) = open
                    && !selected
                {
                    cx.open("tracker", Some(OpenRequest::Connection { id }));
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
            let (caps, busy, ready) = {
                let sh = self.shared.lock();
                (
                    sh.caps,
                    sh.busy.is_some(),
                    matches!(sh.status, Some(Status::Ready)),
                )
            };
            if ui
                .add_enabled(
                    ready && !busy,
                    egui::Button::new(format!(
                        "{} {}",
                        ph::ARROWS_CLOCKWISE,
                        s.t("tracker-refresh")
                    )),
                )
                .tip(s.t("tracker-refresh-tip"))
                .clicked()
            {
                self.refresh();
            }
            ui.separator();
            let reason = self.refusal(s);
            let ro = self.read_only();
            if self.is_pages() {
                let page = self.page();
                let editable = page.as_ref().is_some_and(|p| p.read_only.is_none());
                if ui
                    .add_enabled(
                        ready && editable && !ro && self.page_edit.is_none(),
                        egui::Button::new(format!("{} {}", ph::PENCIL_SIMPLE, s.t("tracker-edit"))),
                    )
                    .tip(s.t("tracker-edit-page-tip"))
                    .tip_disabled(if ro {
                        reason.clone()
                    } else if page.is_some() {
                        s.t("tracker-page-read-only")
                    } else {
                        s.t("tracker-select-page")
                    })
                    .clicked()
                {
                    self.open_edit(cx);
                }
            } else {
                let has_item = self.detail().is_some();
                let blocked = if ro {
                    reason.clone()
                } else {
                    s.t("tracker-select-first")
                };
                if caps.transition
                    && ui
                        .add_enabled(
                            ready && has_item && !ro,
                            egui::Button::new(format!(
                                "{} {}",
                                ph::ARROW_FAT_LINES_RIGHT,
                                s.t("tracker-status")
                            )),
                        )
                        .tip(s.t("tracker-status-tip"))
                        .tip_disabled(blocked.clone())
                        .clicked()
                {
                    self.open_status(cx);
                }
                if caps.edit
                    && ui
                        .add_enabled(
                            ready && has_item && !ro,
                            egui::Button::new(format!(
                                "{} {}",
                                ph::PENCIL_SIMPLE,
                                s.t("tracker-edit")
                            )),
                        )
                        .tip(s.t("tracker-edit-tip"))
                        .tip_disabled(blocked)
                        .clicked()
                {
                    self.open_edit(cx);
                }
                if caps.create
                    && ui
                        .add_enabled(
                            ready && !ro,
                            egui::Button::new(format!("{} {}", ph::PLUS, s.t("tracker-new"))),
                        )
                        .tip(s.t("tracker-new-tip"))
                        .tip_disabled(reason)
                        .clicked()
                {
                    self.open_new(cx);
                }
            }
            ui.separator();
            if ui
                .add_enabled(
                    self.state.item.is_some() || self.state.page.is_some(),
                    egui::Button::new(format!(
                        "{} {}",
                        ph::ARROW_SQUARE_OUT,
                        s.t("tracker-browser")
                    )),
                )
                .tip(s.t("tracker-browser-tip"))
                .clicked()
            {
                self.open_browser(cx);
            }
            if self.protected && !self.writes_unlocked && !self.read_only_profile {
                ui.separator();
                let label = s
                    .i18n
                    .t_args("tracker-protected", &[("env", &self.env_name)]);
                if ui.button(label).tip(s.t("tracker-unlock-hint")).clicked() {
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
                self.assistant.toggle("Tracker");
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

impl TrackerTool {
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("tracker");
        let saved: Vec<hs_ui::dashboard::Entry> = {
            let conns = s.connections.read();
            conns
                .of_tracker_kinds()
                .map(|c| hs_ui::dashboard::Entry {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    detail: TrackerFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                    icon: hs_ui::icons::connection_icon(&c.kind, "").to_string(),
                    image: None,
                })
                .collect()
        };
        let (mine, due) = FIGURES
            .lock()
            .values()
            .fold((0, 0), |(m, d), (a, b)| (m + a, d + b));
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-tracker"),
            subtitle: &s.t("tracker-dashboard-sub"),
            saved: &s.t("tracker-saved"),
            empty: &s.t("tracker-none"),
            start_here: Some(&s.t("dash-start-here")),
            salt: "tracker-dashboard",
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
                    label: &s.t("tracker-stat-trackers"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::USER,
                    value: &mine.to_string(),
                    label: &s.t("tracker-stat-mine"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::CALENDAR,
                    value: &due.to_string(),
                    label: &s.t("tracker-stat-due"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[format!("{} {}", ph::PLUS, s.t("tracker-add"))],
            &saved,
        );
        match pick {
            Some(hs_ui::dashboard::Pick::Action(_)) => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:tracker".into(),
                });
            }
            Some(hs_ui::dashboard::Pick::Open(id)) => {
                cx.open("tracker", Some(OpenRequest::Connection { id }));
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
                hs_core::fmt::sentence(&text),
            );
        }
        if self.state.conn_id.is_none() {
            form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("tracker-pick-hint"));
            return;
        }
        if let Some(e) = self.conn_error.clone() {
            form::note(ui, &theme, ph::WARNING, &e);
            if ui.button(s.t("tracker-retry")).clicked() {
                self.connect(s);
            }
            return;
        }
        let status = self.shared.lock().status.clone();
        match status {
            Some(Status::Connecting) | None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("tracker-connecting"));
                });
                return;
            }
            Some(Status::Failed(e)) => {
                form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                if ui.button(s.t("tracker-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            Some(Status::Ready) => {}
        }
        // The first query, once the driver has said what it is.
        if self.state.query.is_empty() && self.last_poll.is_none() {
            self.query_text = self.shared.lock().default_query.clone();
            self.search();
        }
        if self.is_pages() && self.shared.lock().spaces.is_none() && !self.state.searching {
            self.shared.lock().spaces = Some(Ok(Vec::new()));
            self.send(Cmd::Spaces);
        }
        // Re-read while the tab is drawn, never while it is not
        // (docs/tools/tracker.md §7).
        let poll = Duration::from_secs(u64::from(s.settings.read().tracker.poll_secs.max(10)));
        if self.last_poll.is_none_or(|t| t.elapsed() >= poll) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(poll);
        // The dashboard's figures follow what this tab last counted.
        if let Some(id) = &self.state.conn_id {
            let sh = self.shared.lock();
            let (today, week_end) = week();
            let due = sh
                .found
                .as_ref()
                .map(|f| {
                    f.items
                        .iter()
                        .filter(|i| !i.due.is_empty() && i.due >= today && i.due <= week_end)
                        .count()
                })
                .unwrap_or(0);
            FIGURES
                .lock()
                .insert(id.clone(), (sh.mine.unwrap_or(0), due));
        }
        // A restored tab reads its item or page once by itself.
        let want = if self.is_pages() {
            self.state.page.clone()
        } else {
            self.state.item.clone()
        };
        if let Some(k) = want
            && self.requested.as_ref() != Some(&k)
        {
            if self.is_pages() {
                self.open_page(k);
            } else {
                self.select(k);
            }
        }
        // A new item, once created, is the one selected.
        if let Some(key) = self.shared.lock().created.take() {
            self.state.item = Some(key.clone());
            self.requested = Some(key);
            self.pane = Pane::View;
            self.new = None;
        }
        // Wide enough for the six columns; the item takes the rest.
        let left = (ui.available_width() * 0.5).clamp(320.0, 720.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left);
                if self.is_pages() && !self.state.searching {
                    self.page_tree(ui, &theme, s);
                } else {
                    self.items(ui, &theme, cx);
                }
            });
            ui.separator();
            ui.vertical(|ui| {
                if self.is_pages() {
                    self.page_pane(ui, &theme, cx);
                } else {
                    match self.pane {
                        Pane::View => self.item_pane(ui, &theme, cx),
                        Pane::Status => self.status_pane(ui, &theme, cx),
                        Pane::Edit => self.edit_pane(ui, &theme, cx),
                        Pane::New => self.new_pane(ui, &theme, cx),
                    }
                }
            });
        });
    }

    /// Browse and Search, for Confluence's left pane.
    fn pages_mode(&mut self, ui: &mut egui::Ui, s: &Services) {
        ui.horizontal(|ui| {
            for (searching, icon, key) in [
                (false, ph::TREE_STRUCTURE, "tracker-browse"),
                (true, ph::MAGNIFYING_GLASS, "tracker-search"),
            ] {
                if ui
                    .add(egui::Button::selectable(
                        self.state.searching == searching,
                        format!("{icon} {}", s.t(key)),
                    ))
                    .clicked()
                {
                    self.state.searching = searching;
                }
            }
        });
        ui.add_space(4.0);
    }

    fn query_bar(&mut self, ui: &mut egui::Ui, s: &Services, cx: &mut ToolContext) {
        let mut go = false;
        let mut save = false;
        let mut remove: Option<usize> = None;
        let mut pick: Option<String> = None;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 120.0).max(120.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query_text)
                    .hint_text(s.t(match self.kind.as_str() {
                        "jira" => "tracker-query-hint-jql",
                        "confluence" => "tracker-query-hint-cql",
                        "redmine" => "tracker-query-hint-redmine",
                        _ => "tracker-query-hint-forge",
                    }))
                    .desired_width(width)
                    .font(egui::TextStyle::Monospace),
            );
            if self.focus_query {
                r.request_focus();
                self.focus_query = false;
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            if ui
                .button(ph::MAGNIFYING_GLASS)
                .tip(s.t("tracker-search-tip"))
                .clicked()
            {
                go = true;
            }
            ui.menu_button(ph::BOOKMARK_SIMPLE, |ui| {
                if self.queries.is_empty() {
                    ui.weak(s.t("tracker-no-saved-queries"));
                }
                for (i, q) in self.queries.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(q).clicked() {
                            pick = Some(q.clone());
                            ui.close();
                        }
                        if ui
                            .small_button(ph::X)
                            .tip(s.t("tracker-remove-query"))
                            .clicked()
                        {
                            remove = Some(i);
                        }
                    });
                }
                ui.separator();
                if ui
                    .add_enabled(
                        !self.query_text.trim().is_empty()
                            && !self.queries.contains(&self.query_text.trim().to_string()),
                        egui::Button::new(format!("{} {}", ph::PLUS, s.t("tracker-save-query"))),
                    )
                    .clicked()
                {
                    save = true;
                    ui.close();
                }
            })
            .response
            .tip(s.t("tracker-saved-queries"));
        });
        if let Some(q) = pick {
            self.query_text = q;
            go = true;
        }
        if save || remove.is_some() {
            if save {
                self.queries.push(self.query_text.trim().to_string());
            }
            if let Some(i) = remove {
                self.queries.remove(i);
            }
            if let Err(e) = self.save_queries(s) {
                cx.toast(ToastLevel::Error, e);
            }
        }
        if go {
            self.search();
        }
    }

    fn items(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        if self.is_pages() {
            self.pages_mode(ui, s);
        }
        self.query_bar(ui, s, cx);
        let (found, error, busy) = {
            let sh = self.shared.lock();
            (sh.found.clone(), sh.found_error.clone(), sh.busy)
        };
        ui.horizontal(|ui| {
            ui.strong(s.t(if self.is_pages() {
                "tracker-pages"
            } else {
                "tracker-items"
            }));
            if let Some(f) = &found {
                let count = match f.total {
                    Some(t) => s.i18n.t_args(
                        "tracker-count-of",
                        &[
                            ("shown", &f.items.len().to_string()),
                            ("total", &t.to_string()),
                        ],
                    ),
                    None => f.items.len().to_string(),
                };
                ui.weak(count);
            }
            if let Some(b) = busy {
                ui.spinner();
                ui.weak(s.t(b));
            }
        });
        if let Some(e) = &error {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
        }
        let Some(found) = found else {
            if error.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("tracker-searching"));
                });
            }
            return;
        };
        if found.items.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("tracker-no-items"));
            return;
        }
        let columns: Vec<ColumnSpec> = if self.is_pages() {
            vec![
                ColumnSpec::new(s.t("tracker-col-title"), 220.0),
                ColumnSpec::new(s.t("tracker-col-space"), 70.0),
                ColumnSpec::new(s.t("tracker-col-by"), 110.0),
                ColumnSpec::new(s.t("tracker-col-updated"), 120.0),
            ]
        } else {
            vec![
                ColumnSpec::new(s.t("tracker-col-key"), 70.0),
                ColumnSpec::new(s.t("tracker-col-summary"), 220.0),
                ColumnSpec::new(s.t("tracker-col-status"), 90.0),
                ColumnSpec::new(s.t("tracker-col-assignee"), 110.0),
                ColumnSpec::new(s.t("tracker-col-priority"), 70.0),
                ColumnSpec::new(s.t("tracker-col-updated"), 120.0),
            ]
        };
        let rows: Vec<Vec<String>> = found
            .items
            .iter()
            .map(|i| {
                let updated = i.updated_ms.map(driver::time_ms).unwrap_or_default();
                if self.is_pages() {
                    vec![
                        i.summary.clone(),
                        i.status.clone(),
                        i.assignee.clone(),
                        updated,
                    ]
                } else {
                    vec![
                        i.key.clone(),
                        i.summary.clone(),
                        i.status.clone(),
                        i.assignee.clone(),
                        i.priority.clone(),
                        updated,
                    ]
                }
            })
            .collect();
        let current = if self.is_pages() {
            self.state.page.as_ref()
        } else {
            self.state.item.as_ref()
        };
        self.items_table.selected =
            current.and_then(|k| found.items.iter().position(|i| &i.key == k));
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
        let more = found.next.is_some();
        let r = ui
            .push_id("tracker-items", |ui| {
                // Leave room for More under the table.
                ui.set_max_height(ui.available_height() - if more { 32.0 } else { 0.0 });
                table.show(ui, &mut self.items_table)
            })
            .inner;
        if let Some(i) = r.clicked.or(r.double_clicked)
            && let Some(it) = found.items.get(i)
        {
            if self.is_pages() {
                if self.state.page.as_ref() != Some(&it.key) {
                    self.open_page(it.key.clone());
                }
            } else if self.state.item.as_ref() != Some(&it.key) || self.pane != Pane::View {
                self.select(it.key.clone());
            }
        }
        if more
            && ui
                .add_enabled(
                    busy.is_none(),
                    egui::Button::new(format!("{} {}", ph::CARET_DOWN, s.t("tracker-more"))),
                )
                .tip(s.t("tracker-more-tip"))
                .clicked()
        {
            self.send(Cmd::More);
        }
    }

    fn page_tree(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        self.pages_mode(ui, s);
        let (spaces, pages, busy) = {
            let sh = self.shared.lock();
            (sh.spaces.clone(), sh.pages.clone(), sh.busy)
        };
        let spaces = match spaces {
            Some(Ok(v)) => v,
            Some(Err(e)) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
            None => Vec::new(),
        };
        // The profile's default space, until the user picks one.
        if self.state.space.is_none()
            && let Some(id) = &self.state.conn_id
        {
            let default = s
                .connections
                .read()
                .by_id(id)
                .map(|c| TrackerFields::from_table(&c.kind, &c.fields).project)
                .unwrap_or_default();
            if let Some(sp) = spaces.iter().find(|x| x.key == default) {
                self.state.space = Some(sp.key.clone());
            }
        }
        let current = self
            .state
            .space
            .as_ref()
            .and_then(|k| spaces.iter().find(|x| &x.key == k))
            .cloned();
        let mut picked = None;
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("tracker-space")
                .width(ui.available_width() - 30.0)
                .selected_text(
                    current
                        .as_ref()
                        .map(|x| format!("{} ({})", x.name, x.key))
                        .unwrap_or_else(|| s.t("tracker-pick-space")),
                )
                .show_ui(ui, |ui| {
                    for sp in &spaces {
                        if ui
                            .selectable_label(
                                current.as_ref() == Some(sp),
                                format!("{} ({})", sp.name, sp.key),
                            )
                            .clicked()
                        {
                            picked = Some(sp.clone());
                        }
                    }
                });
            if busy.is_some() {
                ui.spinner();
            }
        });
        if let Some(sp) = picked {
            self.state.space = Some(sp.key.clone());
            self.send(Cmd::Pages(sp));
        }
        let Some(space) = current else {
            if spaces.is_empty() && busy.is_none() {
                form::note(ui, theme, ph::INFO, &s.t("tracker-no-spaces"));
            } else {
                form::hint(ui, theme, &s.t("tracker-pick-space-hint"));
            }
            return;
        };
        let Some(pages) = pages.filter(|p| p.key == space.key) else {
            if busy.is_none() {
                self.send(Cmd::Pages(space));
            }
            ui.spinner();
            return;
        };
        let pages = match pages.result {
            Ok(p) => p,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        if pages.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("tracker-no-pages"));
            return;
        }
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("tracker-page-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (depth, p) in tree_order(&pages) {
                    ui.horizontal(|ui| {
                        ui.add_space(depth as f32 * 14.0);
                        let selected = self.state.page.as_ref() == Some(&p.id);
                        if ui
                            .selectable_label(selected, format!("{} {}", ph::FILE_TEXT, p.title))
                            .clicked()
                            && !selected
                        {
                            open = Some(p.id.clone());
                        }
                    });
                }
            });
        if let Some(id) = open {
            self.open_page(id);
        }
    }

    fn item_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(key) = self.state.item.clone() else {
            form::note(ui, theme, ph::HAND_POINTING, &s.t("tracker-select-hint"));
            return;
        };
        let got = self.shared.lock().detail.clone();
        let Some(got) = got.filter(|g| g.key == key) else {
            ui.spinner();
            return;
        };
        let d = match got.result {
            Ok(d) => d,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong(&d.item.key);
            ui.heading(&d.item.summary);
        });
        ui.horizontal_wrapped(|ui| {
            let chip = |ui: &mut egui::Ui, icon: &str, text: &str| {
                if !text.is_empty() {
                    ui.label(format!("{icon} {text}"));
                    ui.add_space(8.0);
                }
            };
            chip(ui, ph::CIRCLE_HALF, &d.item.status);
            chip(ui, ph::USER, &d.item.assignee);
            chip(ui, ph::FLAG, &d.item.priority);
            chip(ui, ph::CALENDAR, &d.due);
            for l in &d.labels {
                ui.label(egui::RichText::new(format!("{} {l}", ph::TAG)).color(theme.accent));
            }
        });
        ui.add_space(4.0);
        let read_only = self.read_only();
        let can_comment = self.shared.lock().caps.comment;
        egui::ScrollArea::vertical()
            .id_salt("tracker-item")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if !d.fields.is_empty() {
                    egui::Grid::new("tracker-fields")
                        .num_columns(2)
                        .spacing([12.0, 2.0])
                        .show(ui, |ui| {
                            for (name, value) in &d.fields {
                                if value.is_empty() {
                                    continue;
                                }
                                let k = field_key(name);
                                let label = s.t(&k);
                                ui.weak(if label == k { name.clone() } else { label });
                                ui.label(value);
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                }
                if let Some(l) = &d.lossy {
                    form::hint(ui, theme, &s.i18n.t_args("tracker-lossy", &[("what", l)]));
                }
                if d.description.trim().is_empty() {
                    ui.weak(s.t("tracker-no-description"));
                } else {
                    markdown(ui, &d.description, theme);
                }
                if !d.attachments.is_empty() || !d.links.is_empty() {
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for (name, url) in &d.attachments {
                            if ui
                                .link(format!("{} {name}", ph::PAPERCLIP))
                                .tip(s.t("tracker-attachment-tip"))
                                .clicked()
                            {
                                cx.open("browser", Some(OpenRequest::Url { url: url.clone() }));
                            }
                        }
                    });
                    let mut go = None;
                    ui.horizontal_wrapped(|ui| {
                        for (how, other) in &d.links {
                            ui.weak(how);
                            if ui.link(other).clicked() {
                                go = Some(other.clone());
                            }
                            ui.add_space(8.0);
                        }
                    });
                    if let Some(k) = go {
                        self.select(k);
                    }
                }
                ui.add_space(10.0);
                ui.strong(
                    s.i18n
                        .t_args("tracker-comments", &[("n", &d.comments.len().to_string())]),
                );
                if let Some(n) = &d.comments_note {
                    form::hint(ui, theme, &hs_core::fmt::sentence(n));
                }
                for c in &d.comments {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .inner_margin(6.0)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.strong(&c.author);
                                ui.weak(c.created_ms.map(driver::time_ms).unwrap_or_default());
                            });
                            markdown(ui, &c.body, theme);
                        });
                }
                if can_comment {
                    ui.add_space(8.0);
                    self.comment_box(ui, theme, cx, &key, read_only);
                }
            });
    }

    fn comment_box(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        cx: &mut ToolContext,
        key: &str,
        read_only: bool,
    ) {
        let s = cx.services;
        let r = ui.add_enabled(
            !read_only,
            egui::TextEdit::multiline(&mut self.comment)
                .hint_text(s.t("tracker-comment-hint"))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        if self.focus_comment {
            r.request_focus();
            self.focus_comment = false;
        }
        if read_only {
            form::hint(ui, theme, &self.refusal(s));
            return;
        }
        if ui
            .add_enabled(
                !self.comment.trim().is_empty(),
                egui::Button::new(format!("{} {}", ph::CHAT_TEXT, s.t("tracker-comment"))),
            )
            .tip(s.t("tracker-comment-tip"))
            .clicked()
            && self.guard(cx)
        {
            let body = std::mem::take(&mut self.comment);
            self.send(Cmd::Comment(key.to_string(), body));
        }
    }

    fn status_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(key) = self.state.item.clone() else {
            self.pane = Pane::View;
            return;
        };
        ui.strong(s.i18n.t_args("tracker-status-for", &[("key", &key)]));
        ui.add_space(6.0);
        let got = self.shared.lock().transitions.clone();
        let mut go = None;
        match got.filter(|g| g.key == key).map(|g| g.result) {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e)),
            Some(Ok(list)) if list.is_empty() => {
                form::note(ui, theme, ph::INFO, &s.t("tracker-no-transitions"))
            }
            Some(Ok(list)) => {
                for t in list {
                    if ui
                        .add_enabled(
                            !self.read_only(),
                            egui::Button::new(format!("{} {}", ph::ARROW_RIGHT, t.name)),
                        )
                        .clicked()
                    {
                        go = Some(t);
                    }
                }
            }
        }
        ui.add_space(8.0);
        if ui.button(s.t("cancel")).clicked() {
            self.pane = Pane::View;
        }
        if let Some(t) = go
            && self.guard(cx)
        {
            self.send(Cmd::Transition(key, t));
            self.pane = Pane::View;
        }
    }

    fn edit_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let caps_labels = !matches!(self.kind.as_str(), "redmine");
        let has_priority = matches!(self.kind.as_str(), "jira" | "redmine");
        let has_due = !matches!(self.kind.as_str(), "github-issues");
        let Some(draft) = self.edit.as_mut() else {
            self.pane = Pane::View;
            return;
        };
        ui.strong(s.i18n.t_args("tracker-edit-title", &[("key", &draft.key)]));
        ui.add_space(6.0);
        if let Some(l) = &draft.lossy {
            form::note(
                ui,
                theme,
                ph::WARNING,
                &s.i18n.t_args("tracker-lossy-edit", &[("what", l)]),
            );
        }
        let edit = diff_edit(&draft.was, &draft.now);
        let mut save = false;
        let mut back = false;
        if draft.review {
            ui.label(s.t("tracker-review-hint"));
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("tracker-edit-review")
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, key: &str, old: &str, new: &str| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(s.t(key));
                            ui.colored_label(theme.error, old);
                            ui.label(ph::ARROW_RIGHT);
                            ui.colored_label(theme.success, new);
                        });
                    };
                    let w = &draft.was;
                    let n = &draft.now;
                    if edit.summary.is_some() {
                        row(ui, "tracker-f-summary", &w.summary, &n.summary);
                    }
                    if edit.assignee.is_some() {
                        row(ui, "tracker-f-assignee", &w.assignee, &n.assignee);
                    }
                    if edit.priority.is_some() {
                        row(ui, "tracker-f-priority", &w.priority, &n.priority);
                    }
                    if edit.labels.is_some() {
                        row(ui, "tracker-f-labels", &w.labels, &n.labels);
                    }
                    if edit.due.is_some() {
                        row(ui, "tracker-f-due", &w.due, &n.due);
                    }
                    if edit.description.is_some() {
                        ui.strong(s.t("tracker-f-description"));
                        line_diff(ui, theme, &w.description, &n.description);
                    }
                });
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} {}", ph::FLOPPY_DISK, s.t("tracker-save")))
                    .clicked()
                {
                    save = true;
                }
                if ui.button(s.t("tracker-back")).clicked() {
                    draft.review = false;
                }
            });
        } else {
            form::field_grid(ui, "tracker-edit", |ui| {
                form::label(ui, &s.t("tracker-f-summary"));
                form::text_field(ui, &mut draft.now.summary, "");
                ui.end_row();
                form::label(ui, &s.t("tracker-f-assignee"));
                form::text_field(ui, &mut draft.now.assignee, &s.t("tracker-assignee-hint"));
                ui.end_row();
                if has_priority {
                    form::label(ui, &s.t("tracker-f-priority"));
                    form::text_field(ui, &mut draft.now.priority, "");
                    ui.end_row();
                }
                if caps_labels {
                    form::label(ui, &s.t("tracker-f-labels"));
                    form::text_field(ui, &mut draft.now.labels, &s.t("tracker-labels-hint"));
                    ui.end_row();
                }
                if has_due {
                    form::label(ui, &s.t("tracker-f-due"));
                    form::text_field(ui, &mut draft.now.due, "YYYY-MM-DD");
                    ui.end_row();
                }
            });
            ui.label(s.t("tracker-f-description"));
            ui.add(
                egui::TextEdit::multiline(&mut draft.now.description)
                    .code_editor()
                    .desired_rows(12)
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !edit.is_empty() && !draft.now.summary.trim().is_empty(),
                        egui::Button::new(format!("{} {}", ph::LIST_CHECKS, s.t("tracker-review"))),
                    )
                    .tip(s.t("tracker-review-tip"))
                    .tip_disabled(s.t("tracker-nothing-changed"))
                    .clicked()
                {
                    draft.review = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    back = true;
                }
            });
        }
        let key = draft.key.clone();
        if back {
            self.edit = None;
            self.pane = Pane::View;
        }
        if save && self.guard(cx) {
            self.send(Cmd::Edit(key, edit));
            self.edit = None;
            self.pane = Pane::View;
        }
    }

    fn new_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let form_spec = self.shared.lock().form.clone();
        let needs_project = matches!(self.kind.as_str(), "jira" | "redmine");
        let has_labels = !matches!(self.kind.as_str(), "redmine");
        let Some(item) = self.new.as_mut() else {
            self.pane = Pane::View;
            return;
        };
        ui.strong(s.t("tracker-new-title"));
        ui.add_space(6.0);
        let spec = match form_spec {
            None => {
                ui.spinner();
                return;
            }
            Some(Err(e)) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                Default::default()
            }
            Some(Ok(f)) => f,
        };
        if item.kind.is_empty()
            && let Some(k) = spec.kinds.first()
        {
            item.kind = k.clone();
        }
        let mut reload = false;
        form::field_grid(ui, "tracker-new", |ui| {
            if needs_project {
                form::label(ui, &s.t("tracker-f-project"));
                if form::text_field(ui, &mut item.project, "").lost_focus() {
                    reload = true;
                }
                ui.end_row();
            }
            if !spec.kinds.is_empty() {
                form::label(ui, &s.t("tracker-f-type"));
                egui::ComboBox::from_id_salt("tracker-new-kind")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(item.kind.clone())
                    .show_ui(ui, |ui| {
                        for k in &spec.kinds {
                            ui.selectable_value(&mut item.kind, k.clone(), k);
                        }
                    });
                ui.end_row();
            }
            if !spec.priorities.is_empty() {
                form::label(ui, &s.t("tracker-f-priority"));
                egui::ComboBox::from_id_salt("tracker-new-priority")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(item.priority.clone())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut item.priority, String::new(), "—");
                        for p in &spec.priorities {
                            ui.selectable_value(&mut item.priority, p.clone(), p);
                        }
                    });
                ui.end_row();
            }
            form::label(ui, &format!("{} *", s.t("tracker-f-summary")));
            form::text_field(ui, &mut item.summary, "");
            ui.end_row();
            if has_labels {
                form::label(ui, &s.t("tracker-f-labels"));
                let mut labels = item.labels.join(", ");
                if form::text_field(ui, &mut labels, &s.t("tracker-labels-hint")).changed() {
                    item.labels = crate::github::split_list(&labels);
                }
                ui.end_row();
            }
        });
        ui.label(s.t("tracker-f-description"));
        ui.add(
            egui::TextEdit::multiline(&mut item.description)
                .code_editor()
                .desired_rows(10)
                .desired_width(f32::INFINITY),
        );
        let ready = !item.summary.trim().is_empty() && (!needs_project || !item.project.is_empty());
        let mut create = false;
        let mut back = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    ready,
                    egui::Button::new(format!("{} {}", ph::PLUS, s.t("tracker-create"))),
                )
                .tip_disabled(s.t("tracker-new-needs"))
                .clicked()
            {
                create = true;
            }
            if ui.button(s.t("cancel")).clicked() {
                back = true;
            }
        });
        let project = item.project.clone();
        let item = item.clone();
        if reload {
            self.shared.lock().form = None;
            self.send(Cmd::Fields(project));
        }
        if back {
            self.new = None;
            self.pane = Pane::View;
        }
        if create && self.guard(cx) {
            self.send(Cmd::Create(item));
        }
    }

    fn page_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(id) = self.state.page.clone() else {
            form::note(ui, theme, ph::HAND_POINTING, &s.t("tracker-select-page"));
            return;
        };
        let got = self.shared.lock().page.clone();
        let Some(got) = got.filter(|g| g.key == id) else {
            ui.spinner();
            return;
        };
        let p = match got.result {
            Ok(p) => p,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        ui.horizontal_wrapped(|ui| {
            ui.heading(&p.title);
            ui.weak(
                s.i18n
                    .t_args("tracker-page-version", &[("n", &p.version.to_string())]),
            );
        });
        if let Some(why) = &p.read_only {
            form::hint(ui, theme, &hs_core::fmt::sentence(why));
        }
        ui.add_space(4.0);
        let read_only = self.read_only();
        if let Some(draft) = self.page_edit.as_mut()
            && draft.page.id == p.id
        {
            let mut publish = false;
            let mut back = false;
            if draft.review {
                ui.label(s.t("tracker-publish-hint"));
                egui::ScrollArea::vertical()
                    .id_salt("tracker-page-diff")
                    .max_height(ui.available_height() - 40.0)
                    .show(ui, |ui| line_diff(ui, theme, &draft.page.body, &draft.body));
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{} {}", ph::UPLOAD_SIMPLE, s.t("tracker-publish")))
                        .clicked()
                    {
                        publish = true;
                    }
                    if ui.button(s.t("tracker-back")).clicked() {
                        draft.review = false;
                    }
                });
            } else {
                ui.add(
                    egui::TextEdit::multiline(&mut draft.body)
                        .code_editor()
                        .desired_rows(20)
                        .desired_width(f32::INFINITY),
                );
                ui.horizontal(|ui| {
                    let changed = draft.body.trim_end() != draft.page.body.trim_end();
                    if ui
                        .add_enabled(
                            changed,
                            egui::Button::new(format!(
                                "{} {}",
                                ph::LIST_CHECKS,
                                s.t("tracker-review")
                            )),
                        )
                        .tip_disabled(s.t("tracker-nothing-changed"))
                        .clicked()
                    {
                        draft.review = true;
                    }
                    if ui.button(s.t("cancel")).clicked() {
                        back = true;
                    }
                });
            }
            let (page, body) = (draft.page.clone(), draft.body.clone());
            if back {
                self.page_edit = None;
            }
            if publish && self.guard(cx) {
                self.send(Cmd::Publish(page, body));
                self.page_edit = None;
            }
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("tracker-page")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                markdown(ui, &p.body, theme);
                ui.add_space(10.0);
                ui.strong(
                    s.i18n
                        .t_args("tracker-comments", &[("n", &p.comments.len().to_string())]),
                );
                for c in &p.comments {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .stroke(egui::Stroke::new(1.0, theme.border))
                        .inner_margin(6.0)
                        .corner_radius(4.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.strong(&c.author);
                                ui.weak(c.created_ms.map(driver::time_ms).unwrap_or_default());
                            });
                            markdown(ui, &c.body, theme);
                        });
                }
                ui.add_space(8.0);
                self.comment_box(ui, theme, cx, &id, read_only);
            });
    }

    /// The review-only assistant (ADR-019), told about the selected item
    /// and its comments. A reply it drafts goes into the comment box with
    /// Insert; it never sends anything.
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
        let context = if self.is_pages() {
            self.page()
                .map(|p| {
                    let comments: String = p
                        .comments
                        .iter()
                        .map(|c| format!("{}: {}\n", c.author, c.body))
                        .collect();
                    format!("Page: {}\n\n{}\n\nComments:\n{comments}", p.title, p.body)
                })
                .unwrap_or_default()
        } else {
            self.detail()
                .map(|d| {
                    let comments: String = d
                        .comments
                        .iter()
                        .rev()
                        .take(30)
                        .rev()
                        .map(|c| format!("{}: {}\n", c.author, c.body))
                        .collect();
                    format!(
                        "Item: {} {}\nStatus: {}\nAssignee: {}\nPriority: {}\n\nDescription:\n{}\n\nComments:\n{comments}",
                        d.item.key,
                        d.item.summary,
                        d.item.status,
                        d.item.assignee,
                        d.item.priority,
                        d.description
                    )
                })
                .unwrap_or_default()
        };
        let context = format!(
            "Tracker: {} ({})\n{context}",
            self.conn_name,
            hs_core::connections::kind_label(&self.kind),
        );
        self.assistant.set_context("Tracker", context);
        if self.assistant.quick_actions.is_empty() {
            self.assistant.quick_actions =
                hs_ui::ai_assistant::QuickAction::for_activity(s, "tracker");
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
        let mut insert = None;
        egui::Frame::new()
            .fill(theme.panel_bg)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(&mut child, |ui| {
                ui.set_min_width(370.0);
                self.assistant.can_insert = !self.read_only();
                insert = self.assistant.ui(ui, s, &theme, "Tracker").use_block;
            });
        // Insert puts a drafted reply in the comment box; the user sends it.
        if let Some(text) = insert {
            self.comment = text.trim().to_string();
            self.pane = Pane::View;
            self.page_edit = None;
            self.focus_comment = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_protected_tab_is_read_only_until_unlocked() {
        let mut t = TrackerTool::new(State::default());
        t.protected = true;
        assert!(t.read_only());
        t.writes_unlocked = true;
        assert!(!t.read_only());
        t.read_only_profile = true;
        assert!(t.read_only());
    }

    #[test]
    fn an_edit_carries_only_the_fields_that_changed() {
        let was = Fields {
            summary: "Crash".into(),
            description: "Steps\n".into(),
            labels: "bug, ui".into(),
            ..Default::default()
        };
        let mut now = was.clone();
        assert!(diff_edit(&was, &now).is_empty());
        now.labels = "bug".into();
        now.description = "Steps".into();
        let e = diff_edit(&was, &now);
        assert_eq!(e.labels, Some(vec!["bug".to_string()]));
        // A trailing newline is not a change.
        assert_eq!(e.description, None);
        assert_eq!(e.summary, None);
    }

    #[test]
    fn the_page_tree_nests_children_under_parents_and_orphans_at_the_top() {
        let p = |id: &str, title: &str, parent: &str| PageRef {
            id: id.into(),
            title: title.into(),
            parent: parent.into(),
        };
        let pages = vec![
            p("3", "Child B", "1"),
            p("1", "Home", ""),
            p("2", "Child A", "1"),
            p("4", "Grandchild", "2"),
            p("5", "Orphan", "99"),
        ];
        let order: Vec<(usize, &str)> = tree_order(&pages)
            .into_iter()
            .map(|(d, p)| (d, p.title.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                (0, "Home"),
                (1, "Child A"),
                (2, "Grandchild"),
                (1, "Child B"),
                (0, "Orphan"),
            ]
        );
    }
}
