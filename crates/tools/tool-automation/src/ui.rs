//! The tab (docs/tools/automation.md §3): the dashboard on the home tab,
//! and for a server its jobs on the left and the selected job's Runs, Log,
//! Run form and Queue on the right. Everything the UI shows arrives
//! through [`crate::runner::Shared`]; nothing here talks to a server.

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

use crate::driver::{self, Job, ParamKind, Run, RunState};
use crate::profile::{AutomationFields, ProfileError};
use crate::runner::{self, Cmd, Shared, Status};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "automation",
    title_key: "tool-automation",
    icon: ph::FLOW_ARROW,
    category: ToolCategory::Infra,
    accepts: &[Accept::Connection],
};

/// How often a running log is read again while it is on screen.
const LOG_POLL: Duration = Duration::from_secs(2);

/// What every open server tab last counted, by connection id: (running,
/// failed today). The dashboard's figures come from the servers open in
/// tabs only (docs/tools/automation.md §3).
static FIGURES: LazyLock<Mutex<HashMap<String, (usize, usize)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct AutomationFactory;

impl ToolFactory for AutomationFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = AutomationTool::new(state);
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores. Never a secret, never a parameter value.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    pub job: Option<String>,
    pub run: Option<String>,
    pub detail: Detail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Detail {
    #[default]
    Runs,
    Log,
    Run,
    Queue,
}

/// The Run form's values, for the job they were filled for.
#[derive(Default)]
struct RunValues {
    job: String,
    values: Vec<(String, String)>,
}

pub struct AutomationTool {
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
    jobs_table: TableState,
    runs_table: TableState,
    last_poll: Option<Instant>,
    last_log_poll: Option<Instant>,
    run_values: RunValues,
    confirm_cancel: Option<(Job, Run)>,
    /// The pane last asked for, and for which job, so a restored tab loads
    /// its detail once by itself.
    requested: Option<(Detail, String)>,
    /// The run whose end the log has reported and whose state has been
    /// read again for it, so it happens once.
    settled: Option<String>,
    /// The log laid out in steps (FEAT-0095), keyed by what it was laid
    /// out from - (run, length, next offset, cut) - so a frame that shows
    /// the same text does not lay it out again.
    log_lines: Option<(LogKey, Vec<crate::logfmt::Line>)>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl Drop for AutomationTool {
    fn drop(&mut self) {
        if let Some(id) = &self.state.conn_id {
            FIGURES.lock().remove(id);
        }
    }
}

/// The glyph and colour a run state wears.
/// One laid-out log line (FEAT-0095): a banner strong in the accent, a
/// group strong, a command in the accent with its prompt, the server's
/// bookkeeping muted, and the body indented by its depth.
fn log_line(ui: &mut egui::Ui, theme: &Theme, l: &crate::logfmt::Line) {
    use crate::logfmt::Kind;
    let indent = "  ".repeat(l.depth);
    let text = match l.kind {
        Kind::Blank => {
            ui.label("");
            return;
        }
        Kind::Header => egui::RichText::new(format!("{} {}", ph::BRIEFCASE, l.text))
            .monospace()
            .strong()
            .color(theme.accent),
        Kind::Group => egui::RichText::new(format!("{indent}{} {}", ph::CARET_RIGHT, l.text))
            .monospace()
            .strong(),
        Kind::Command => egui::RichText::new(format!("{indent}$ {}", l.text))
            .monospace()
            .color(theme.accent),
        Kind::Meta => egui::RichText::new(format!("{indent}{}", l.text))
            .monospace()
            .color(theme.text_muted),
        Kind::Plain => {
            // A line that names a level wears the Logs activity's colour
            // for it on the level word only (CR-0077); the span was read
            // when the log was laid out.
            let span = l
                .level
                .as_ref()
                .map(|(r, lv)| (r.start + indent.len()..r.end + indent.len(), *lv));
            let job = hs_ui::loglevel::line_job(
                theme,
                &format!("{indent}{}", l.text),
                span.as_ref(),
                egui::TextStyle::Monospace.resolve(ui.style()),
                theme.text,
            );
            ui.add(egui::Label::new(job).selectable(true).extend());
            return;
        }
    };
    ui.add(egui::Label::new(text).selectable(true).extend());
}

/// What a laid-out log was laid out from: (run, length, next offset, cut).
type LogKey = (String, usize, u64, bool);

fn state_glyph(s: &RunState, theme: &Theme) -> (&'static str, egui::Color32) {
    match s {
        RunState::Success => (ph::CHECK_CIRCLE, theme.success),
        RunState::Failure => (ph::X_CIRCLE, theme.error),
        RunState::Unstable => (ph::WARNING, theme.warning),
        RunState::Running => (ph::PLAY, theme.accent),
        RunState::Queued => (ph::HOURGLASS, theme.text_muted),
        RunState::Cancelled | RunState::Skipped | RunState::Unknown => {
            (ph::MINUS, theme.text_muted)
        }
    }
}

impl AutomationTool {
    fn new(state: State) -> AutomationTool {
        AutomationTool {
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
            jobs_table: TableState::default(),
            runs_table: TableState::default(),
            last_poll: None,
            last_log_poll: None,
            run_values: RunValues::default(),
            confirm_cancel: None,
            requested: None,
            settled: None,
            log_lines: None,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
        }
    }

    fn read_only(&self) -> bool {
        self.read_only_profile || (self.protected && !self.writes_unlocked)
    }

    fn connect(&mut self, s: &Services) {
        self.cmd = None;
        self.shared = Arc::new(Mutex::new(Shared::default()));
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.confirm_cancel = None;
        self.last_poll = None;
        self.requested = None;
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (kind, fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("automation-no-profile"));
                return;
            };
            (
                c.kind.clone(),
                AutomationFields::from_table(&c.kind, &c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.kind = kind.clone();
        self.address = fields.label_for(&kind);
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
                        s.t("automation-vault-locked")
                    }
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("automation-missing-vars", &[("names", &v.join(", "))]),
                    other => other.to_string(),
                });
                return;
            }
        };
        let log_max = (s.settings.read().automation.log_mb.clamp(1, 100) as usize) * 1024 * 1024;
        tracing::info!(
            "automation: connecting {} to {} {}",
            self.conn_name,
            kind,
            self.address
        );
        self.cmd = Some(runner::spawn(&s.runtime, cfg, self.shared.clone(), log_max));
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
        // The selected job's runs move on too.
        if self.state.detail == Detail::Runs && self.job().is_some() {
            self.load_detail();
        }
    }

    fn job(&self) -> Option<Job> {
        let id = self.state.job.as_ref()?;
        let sh = self.shared.lock();
        sh.listing
            .as_ref()?
            .jobs
            .iter()
            .find(|j| &j.id == id)
            .cloned()
    }

    fn run(&self) -> Option<Run> {
        let id = self.state.run.as_ref()?;
        let job = self.state.job.as_ref()?;
        let sh = self.shared.lock();
        let runs = sh.runs.as_ref().filter(|r| &r.job == job)?;
        runs.result
            .as_ref()
            .ok()?
            .iter()
            .find(|r| &r.id == id)
            .cloned()
    }

    /// Ask for whatever the current pane shows, for the current job.
    fn load_detail(&mut self) {
        let Some(job) = self.job() else {
            return;
        };
        self.requested = Some((self.state.detail, job.id.clone()));
        match self.state.detail {
            Detail::Runs => {
                self.send(Cmd::Runs(job));
            }
            Detail::Run => {
                self.send(Cmd::Form(job));
            }
            Detail::Log => {
                if let Some(run) = self.run() {
                    self.last_log_poll = Some(Instant::now());
                    self.send(Cmd::Log(job, run));
                }
            }
            Detail::Queue => {
                self.send(Cmd::Queue);
            }
        }
    }

    fn guard(&self, cx: &mut ToolContext) -> bool {
        if self.read_only() {
            let key = if self.read_only_profile {
                "automation-refused-read-only"
            } else {
                "automation-refused-protected"
            };
            cx.toast(ToastLevel::Error, cx.services.t(key));
            return false;
        }
        true
    }

    fn open_run_form(&mut self, cx: &mut ToolContext) {
        if !self.guard(cx) || self.job().is_none() {
            return;
        }
        self.state.detail = Detail::Run;
        self.load_detail();
    }

    fn ask_cancel(&mut self, cx: &mut ToolContext) {
        if !self.guard(cx) {
            return;
        }
        if let (Some(job), Some(run)) = (self.job(), self.run())
            && run.state.is_active()
        {
            self.confirm_cancel = Some((job, run));
        }
    }

    /// Hand the log read so far to the Logs activity, as a snapshot
    /// (docs/tools/automation.md §7).
    fn open_log(&mut self, cx: &mut ToolContext) {
        let text = self.shared.lock().log.as_ref().map(|b| b.text.clone());
        let (Some(text), Some(job), Some(run)) = (text, self.job(), self.run()) else {
            cx.toast(ToastLevel::Info, cx.services.t("automation-no-log"));
            return;
        };
        let body = format!("{} {}\n{text}", job.name, run.number);
        cx.open_new_tab(
            "logs",
            Some(OpenRequest::Text {
                mime: "text/plain".into(),
                body,
            }),
        );
    }

    fn open_browser(&mut self, cx: &mut ToolContext) {
        let url = self
            .run()
            .map(|r| r.url)
            .filter(|u| !u.is_empty())
            .or_else(|| self.job().map(|j| j.url).filter(|u| !u.is_empty()));
        if let Some(url) = url {
            cx.open("browser", Some(OpenRequest::Url { url }));
        }
    }
}

impl Tool for AutomationTool {
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
            std::borrow::Cow::Borrowed("Automation")
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
            hs_ui::icons::conn_icon(&self.kind, "")
                .header(self.conn_name.clone())
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

    /// The product's logo on the tab, where it has one (CR-0036).
    fn tab_icon_uri(&self) -> Option<String> {
        self.tab_icon()?;
        hs_ui::icons::conn_icon(&self.kind, "").uri()
    }

    /// The product's colour for the tab's glyph, where it has no logo.
    fn tab_icon_colour(&self) -> Option<egui::Color32> {
        self.tab_icon()?;
        hs_ui::icons::conn_icon(&self.kind, "").brand()
    }

    fn on_event(&mut self, ev: &hs_core::events::Event, cx: &mut ToolContext) {
        match ev {
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
            c("automation.refresh", "Automation: Refresh", Some("run.alt")),
            c("automation.run", "Automation: Run…", Some("automation.run")),
            c("automation.cancel", "Automation: Cancel Run…", None),
            c(
                "automation.open_log",
                "Automation: Open Log in Logs",
                Some("automation.open_log"),
            ),
            c("automation.browser", "Automation: Open in Browser", None),
            c("automation.add", "Automation: Add Server", None),
            c("automation.reconnect", "Automation: Reconnect", None),
            c("automation.ai", "Automation: AI Assistant", None),
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "automation.refresh" | "run.alt" => {
                self.refresh();
                true
            }
            "automation.run" => {
                self.open_run_form(cx);
                true
            }
            "automation.cancel" => {
                self.ask_cancel(cx);
                true
            }
            "automation.open_log" => {
                self.open_log(cx);
                true
            }
            "automation.browser" => {
                self.open_browser(cx);
                true
            }
            "automation.add" => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:automation:new".into(),
                });
                true
            }
            "automation.reconnect" => {
                self.connect(cx.services);
                true
            }
            "automation.ai" => {
                self.assistant.toggle("Automation");
                true
            }
            _ => false,
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if form::sidebar_head(
            ui,
            &theme,
            &s.t("tool-automation"),
            &[hs_ui::form::SideAction {
                icon: ph::PLUS,
                label: &s.t("automation-add-server"),
                tip: "",
            }],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:automation:new".into(),
            });
        }
        let profiles: Vec<(String, String, String, bool, hs_ui::icons::ConnIcon)> = {
            let conns = s.connections.read();
            conns
                .of_automation_kinds()
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        AutomationFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                        c.pinned,
                        hs_ui::icons::conn_icon(&c.kind, ""),
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("automation-no-servers"));
            return true;
        }
        egui::ScrollArea::vertical()
            .id_salt("automation-servers")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let mut reorder = hs_ui::reorder::Reorder::new("automation-servers");
                let mut open: Option<(String, bool)> = None;
                let mut pin: Option<(String, bool)> = None;
                let mut edit: Option<String> = None;
                let mut remove: Option<String> = None;
                for (i, (id, name, label, pinned, icon)) in profiles.iter().enumerate() {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            selected,
                            *icon,
                            name,
                            Some(label),
                            *pinned,
                            form::RowMenuLabels {
                                pin: &s.t("conn-pin-home"),
                                unpin: &s.t("conn-unpin-home"),
                                edit: &s.t("conn-edit"),
                                remove: &s.t("conn-remove"),
                            },
                        );
                        if row.response.tip(s.t("automation-open-server")).clicked() {
                            open = Some((id.clone(), selected));
                        }
                        if let Some(p) = row.pin {
                            pin = Some((id.clone(), p));
                        }
                        if row.edit {
                            edit = Some(id.clone());
                        }
                        if row.remove {
                            remove = Some(id.clone());
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
                // Remove From List: the shell asks first (FEAT-0094).
                if let Some(id) = remove {
                    cx.actions.push(Action::RunCommand {
                        id: format!("connection:remove:{id}"),
                    });
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    if let Err(e) = s.connections.write().reorder(
                        hs_core::connections::AUTOMATION_KINDS,
                        from,
                        to,
                    ) {
                        cx.toast(ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
                if let Some((id, selected)) = open
                    && !selected
                {
                    cx.open("automation", Some(OpenRequest::Connection { id }));
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
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        if self.state.conn_id.is_some() && !cx.is_home {
            let (caps, busy, ready) = {
                let sh = self.shared.lock();
                (
                    sh.caps,
                    sh.busy.is_some(),
                    matches!(sh.status, Some(Status::Ready)),
                )
            };
            let has_job = self.job().is_some();
            let run = self.run();
            if hs_ui::toolbar::btn_enabled(
                ui,
                &theme,
                ready && !busy,
                ph::ARROWS_CLOCKWISE,
                &s.t("automation-refresh"),
            )
            .tip(s.t("automation-refresh-tip"))
            .clicked()
            {
                self.refresh();
            }
            ui.separator();
            let reason = if self.read_only_profile {
                s.t("automation-refused-read-only")
            } else {
                s.t("automation-refused-protected")
            };
            let blocked = |enabled: bool| {
                if !enabled && self.read_only() {
                    reason.clone()
                } else {
                    s.t("automation-select-first")
                }
            };
            if caps.run
                && hs_ui::toolbar::btn_enabled(
                    ui,
                    &theme,
                    ready && has_job && !self.read_only(),
                    ph::PLAY,
                    &s.t("automation-run"),
                )
                .tip(s.t("automation-run-tip"))
                .tip_disabled(blocked(false))
                .clicked()
            {
                self.open_run_form(cx);
            }
            let can_cancel = run.as_ref().is_some_and(|r| r.state.is_active());
            if caps.cancel
                && hs_ui::toolbar::btn_enabled(
                    ui,
                    &theme,
                    ready && can_cancel && !self.read_only(),
                    ph::STOP,
                    &s.t("automation-cancel"),
                )
                .tip_disabled(if self.read_only() {
                    reason.clone()
                } else {
                    s.t("automation-cancel-needs-active")
                })
                .clicked()
            {
                self.ask_cancel(cx);
            }
            ui.separator();
            if caps.log
                && hs_ui::toolbar::btn_enabled(
                    ui,
                    &theme,
                    run.is_some(),
                    ph::FILE_TEXT,
                    &s.t("automation-open-log"),
                )
                .tip(s.t("automation-open-log-tip"))
                .clicked()
            {
                self.open_log(cx);
            }
            if hs_ui::toolbar::btn_enabled(
                ui,
                &theme,
                has_job,
                ph::ARROW_SQUARE_OUT,
                &s.t("automation-browser"),
            )
            .tip(s.t("automation-browser-tip"))
            .clicked()
            {
                self.open_browser(cx);
            }
            if self.protected && !self.writes_unlocked && !self.read_only_profile {
                ui.separator();
                let label = s
                    .i18n
                    .t_args("automation-protected", &[("env", &self.env_name)]);
                if ui
                    .button(label)
                    .tip(s.t("automation-unlock-hint"))
                    .clicked()
                {
                    self.writes_unlocked = true;
                    self.connect(s);
                }
            }
        }
        hs_ui::toolbar::hard_right(ui, |ui| {
            if hs_ui::toolbar::btn(ui, &theme, ph::SPARKLE, &s.t("ai-assistant"))
                .tip(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("Automation");
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

impl AutomationTool {
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("automation");
        let saved: Vec<hs_ui::dashboard::Entry> = {
            let conns = s.connections.read();
            conns
                .of_automation_kinds()
                .map(|c| hs_ui::dashboard::Entry {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    detail: AutomationFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                    icon: hs_ui::icons::conn_icon(&c.kind, "").glyph.to_string(),
                    image: hs_ui::icons::conn_icon(&c.kind, "").uri(),
                    tint: hs_ui::icons::conn_icon(&c.kind, "").fixed_colour(&theme),
                })
                .collect()
        };
        let (running, failed) = FIGURES
            .lock()
            .values()
            .fold((0, 0), |(r, f), (a, b)| (r + a, f + b));
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-automation"),
            subtitle: &s.t("automation-dashboard-sub"),
            saved: &s.t("automation-saved-servers"),
            empty: &s.t("automation-no-servers"),
            start_here: Some(&s.t("dash-start-here")),
            salt: "automation-dashboard",
            tabs: &tabs,
            labels: &labels,
            extra: None,
            entry_menu: None,
        };
        let pick = hs_ui::dashboard::saved_dashboard(
            ui,
            &theme,
            &page,
            &[
                hs_ui::dashboard::Stat {
                    icon: ph::PLUGS_CONNECTED,
                    value: &saved.len().to_string(),
                    label: &s.t("automation-stat-servers"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::PLAY,
                    value: &running.to_string(),
                    label: &s.t("automation-stat-running"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::X_CIRCLE,
                    value: &failed.to_string(),
                    label: &s.t("automation-stat-failed"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[format!("{} {}", ph::PLUS, s.t("automation-add-server"))],
            &saved,
        );
        match pick {
            Some(hs_ui::dashboard::Pick::Action(_)) => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:automation:new".into(),
                });
            }
            Some(hs_ui::dashboard::Pick::Open(id)) => {
                cx.open("automation", Some(OpenRequest::Connection { id }));
            }
            // No second grid on this dashboard (CR-0056).
            Some(hs_ui::dashboard::Pick::OpenExtra(_)) => {}
            Some(hs_ui::dashboard::Pick::Menu(_)) => {}
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
            form::note(
                ui,
                &theme,
                ph::PLUGS_CONNECTED,
                &s.t("automation-pick-hint"),
            );
            return;
        }
        if let Some(e) = self.conn_error.clone() {
            form::note(ui, &theme, ph::WARNING, &e);
            if ui.button(s.t("automation-retry")).clicked() {
                self.connect(s);
            }
            return;
        }
        let status = self.shared.lock().status.clone();
        match status {
            Some(Status::Connecting) | None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("automation-connecting"));
                });
                return;
            }
            Some(Status::Failed(e)) => {
                form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                if ui.button(s.t("automation-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            Some(Status::Ready) => {}
        }
        // Poll while the tab is drawn, never while it is not
        // (docs/tools/automation.md §7).
        let poll = Duration::from_secs(u64::from(s.settings.read().automation.poll_secs.max(2)));
        if self.last_poll.is_none_or(|t| t.elapsed() >= poll) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(poll.min(LOG_POLL));
        // The dashboard's figures follow what this tab last counted.
        if let Some(id) = &self.state.conn_id
            && let Some(l) = &self.shared.lock().listing
        {
            let today = chrono::Local::now()
                .date_naive()
                .and_hms_opt(0, 0, 0)
                .and_then(|t| t.and_local_timezone(chrono::Local).single())
                .map(|t| t.timestamp_millis())
                .unwrap_or(0);
            let running = l
                .jobs
                .iter()
                .filter(|j| j.last.as_ref().is_some_and(|r| r.state.is_active()))
                .count();
            let failed = l
                .jobs
                .iter()
                .filter(|j| {
                    j.last.as_ref().is_some_and(|r| {
                        r.state == RunState::Failure && r.started_ms.unwrap_or(0) >= today
                    })
                })
                .count();
            FIGURES.lock().insert(id.clone(), (running, failed));
        }
        self.dialogs(ui, cx);
        let left = (ui.available_width() * 0.45).clamp(300.0, 640.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left);
                self.jobs(ui, &theme, s);
            });
            ui.separator();
            ui.vertical(|ui| {
                self.detail(ui, &theme, cx);
            });
        });
    }

    fn jobs(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let (listing, error, busy) = {
            let sh = self.shared.lock();
            (sh.listing.clone(), sh.listing_error.clone(), sh.busy)
        };
        ui.horizontal(|ui| {
            ui.strong(s.t("automation-jobs"));
            if let Some(b) = busy {
                ui.spinner();
                ui.weak(s.t(b));
            }
        });
        ui.add_space(form::PANE_GAP);
        if let Some(e) = &error {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
        }
        let Some(listing) = listing else {
            if error.is_none() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(s.t("automation-listing"));
                });
            }
            return;
        };
        if let Some(n) = &listing.note {
            form::hint(ui, theme, &hs_core::fmt::sentence(n));
        }
        if listing.jobs.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("automation-no-jobs"));
            return;
        }
        let columns = [
            ColumnSpec::new(s.t("automation-col-name"), 200.0),
            ColumnSpec::new(s.t("automation-col-status"), 90.0),
            ColumnSpec::new(s.t("automation-col-last"), 120.0),
            ColumnSpec::new(s.t("automation-col-duration"), 80.0),
        ];
        let rows: Vec<Vec<String>> = listing
            .jobs
            .iter()
            .map(|j| {
                let last = j.last.as_ref();
                vec![
                    j.name.clone(),
                    last.map(|r| s.t(r.state.key())).unwrap_or_default(),
                    last.and_then(|r| r.started_ms)
                        .map(driver::time_ms)
                        .unwrap_or_default(),
                    last.and_then(|r| r.duration_ms)
                        .map(driver::duration)
                        .unwrap_or_default(),
                ]
            })
            .collect();
        self.jobs_table.selected = self
            .state
            .job
            .as_ref()
            .and_then(|id| listing.jobs.iter().position(|j| &j.id == id));
        let table = DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: hs_ui::table::ROW_HEIGHT,
            show_filter: true,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
            empty_hint: None,
        };
        let r = ui
            .push_id("automation-jobs", |ui| table.show(ui, &mut self.jobs_table))
            .inner;
        if let Some(i) = r.clicked.or(r.double_clicked)
            && let Some(j) = listing.jobs.get(i)
            && self.state.job.as_ref() != Some(&j.id)
        {
            self.state.job = Some(j.id.clone());
            self.state.run = None;
            if self.state.detail == Detail::Log {
                self.state.detail = Detail::Runs;
            }
            self.load_detail();
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext) {
        let s = cx.services;
        let Some(job) = self.job() else {
            form::note(ui, theme, ph::HAND_POINTING, &s.t("automation-select-hint"));
            return;
        };
        let caps = self.shared.lock().caps;
        if self.requested.as_ref() != Some(&(self.state.detail, job.id.clone())) {
            self.load_detail();
        }
        hs_ui::toolbar::sub_strip(ui, |ui| {
            for (d, icon, key, shown) in [
                (Detail::Runs, ph::LIST_CHECKS, "automation-tab-runs", true),
                (Detail::Log, ph::FILE_TEXT, "automation-tab-log", caps.log),
                (Detail::Run, ph::PLAY, "automation-tab-run", caps.run),
                (
                    Detail::Queue,
                    ph::HOURGLASS,
                    "automation-tab-queue",
                    caps.queue,
                ),
            ] {
                if !shown {
                    continue;
                }
                if hs_ui::form::add_selectable(
                    ui,
                    egui::Button::selectable(
                        self.state.detail == d,
                        format!("{icon} {}", s.t(key)),
                    ),
                )
                .clicked()
                    && self.state.detail != d
                {
                    self.state.detail = d;
                    self.load_detail();
                }
            }
        });
        ui.add_space(form::PANE_GAP);
        ui.strong(&job.name);
        ui.add_space(form::PANE_GAP);
        match self.state.detail {
            Detail::Runs => self.runs_pane(ui, theme, cx, &job),
            Detail::Log => self.log_pane(ui, theme, s, &job),
            Detail::Run => self.run_pane(ui, theme, cx, &job),
            Detail::Queue => self.queue_pane(ui, theme, s),
        }
    }

    fn runs_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext, job: &Job) {
        let s = cx.services;
        let got = self.shared.lock().runs.clone();
        let Some(got) = got.filter(|g| g.job == job.id) else {
            ui.spinner();
            return;
        };
        let runs = match got.result {
            Ok(r) => r,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        if runs.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("automation-no-runs"));
            return;
        }
        let columns = [
            ColumnSpec::new(s.t("automation-col-run"), 70.0),
            ColumnSpec::new(s.t("automation-col-status"), 90.0),
            ColumnSpec::new(s.t("automation-col-ref"), 110.0),
            ColumnSpec::new(s.t("automation-col-started"), 130.0),
            ColumnSpec::new(s.t("automation-col-duration"), 80.0),
            ColumnSpec::new(s.t("automation-col-who"), 110.0),
        ];
        let rows: Vec<Vec<String>> = runs
            .iter()
            .map(|r| {
                vec![
                    r.number.clone(),
                    s.t(r.state.key()),
                    r.reference.clone(),
                    r.started_ms.map(driver::time_ms).unwrap_or_default(),
                    r.duration_ms.map(driver::duration).unwrap_or_default(),
                    r.who.clone(),
                ]
            })
            .collect();
        self.runs_table.selected = self
            .state
            .run
            .as_ref()
            .and_then(|id| runs.iter().position(|r| &r.id == id));
        let table = DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: hs_ui::table::ROW_HEIGHT,
            show_filter: false,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
            empty_hint: None,
        };
        let r = ui
            .push_id("automation-runs", |ui| table.show(ui, &mut self.runs_table))
            .inner;
        if let Some(i) = r.clicked.or(r.double_clicked)
            && let Some(run) = runs.get(i)
        {
            self.state.run = Some(run.id.clone());
            if r.double_clicked.is_some() {
                if self.shared.lock().caps.log {
                    self.state.detail = Detail::Log;
                    self.load_detail();
                } else {
                    // A kind with no log API (Forgejo 11 has neither a
                    // run log nor a cancel) used to answer a double-click
                    // with nothing at all (BUG-0037). The run's own page
                    // shows the log, so send the gesture there.
                    self.open_browser(cx);
                }
            }
        }
        if let Some(run) = self
            .state
            .run
            .as_ref()
            .and_then(|id| runs.iter().find(|r| &r.id == id))
        {
            let (glyph, colour) = state_glyph(&run.state, theme);
            ui.add_space(form::PANE_GAP);
            ui.horizontal(|ui| {
                ui.colored_label(colour, glyph);
                ui.label(format!("{} · {}", run.number, s.t(run.state.key())));
            });
            let hint = if self.shared.lock().caps.log {
                "automation-runs-hint"
            } else {
                "automation-runs-hint-web"
            };
            form::hint(ui, theme, &s.t(hint));
        }
    }

    fn log_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services, job: &Job) {
        let Some(run) = self.run() else {
            form::note(ui, theme, ph::INFO, &s.t("automation-pick-run"));
            return;
        };
        let buf = self.shared.lock().log.clone();
        let buf = buf.filter(|b| b.job == job.id && b.run == run.id);
        // Follow a run that is still writing.
        if let Some(b) = &buf
            && b.more
            && self.last_log_poll.is_none_or(|t| t.elapsed() >= LOG_POLL)
        {
            self.last_log_poll = Some(Instant::now());
            self.send(Cmd::Log(job.clone(), run.clone()));
        }
        // The log says the run has ended while the runs read last still
        // say it is going: read them again, once, so its state (and
        // whether Cancel applies) catches up.
        if let Some(b) = &buf
            && !b.more
            && b.error.is_none()
            && run.state.is_active()
            && self.settled.as_ref() != Some(&run.id)
        {
            self.settled = Some(run.id.clone());
            self.send(Cmd::Runs(job.clone()));
        }
        ui.horizontal(|ui| {
            let (glyph, colour) = state_glyph(&run.state, theme);
            ui.colored_label(colour, glyph);
            ui.label(format!("{} · {}", run.number, s.t(run.state.key())));
            if buf.as_ref().is_some_and(|b| b.more) {
                ui.spinner();
                ui.weak(s.t("automation-following"));
            }
        });
        let Some(b) = buf else {
            ui.spinner();
            return;
        };
        if let Some(e) = &b.error {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
        }
        if b.cut {
            form::hint(ui, theme, &s.t("automation-log-cut"));
        }
        ui.add_space(form::PANE_GAP);
        // Laid out in steps (FEAT-0095): job banners, groups, commands and
        // indented output, with air before each step. Laid out once per
        // change of the text, never per frame.
        let key = (run.id.clone(), b.text.len(), b.next, b.cut);
        if self.log_lines.as_ref().is_none_or(|(k, _)| *k != key) {
            self.log_lines = Some((key, crate::logfmt::lines(&b.text)));
        }
        let lines = self
            .log_lines
            .as_ref()
            .map(|(_, l)| l.as_slice())
            .unwrap_or(&[]);
        // In a well, so the output reads as something you scroll rather
        // than as text that happens to stop (CR-0028).
        form::well(ui, theme, None, |ui| {
            let mono = egui::TextStyle::Monospace.resolve(ui.style());
            let row_h = ui.fonts_mut(|f| f.row_height(&mono)) + 2.0;
            egui::ScrollArea::both()
                .id_salt("automation-log")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show_rows(ui, row_h, lines.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for l in &lines[range] {
                        log_line(ui, theme, l);
                    }
                });
        });
    }

    fn run_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, cx: &mut ToolContext, job: &Job) {
        let s = cx.services;
        let got = self.shared.lock().form.clone();
        let Some(got) = got.filter(|g| g.job == job.id) else {
            ui.spinner();
            return;
        };
        let form_spec = match got.result {
            Ok(f) => f,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        if !form_spec.allowed {
            form::note(ui, theme, ph::INFO, &hs_core::fmt::sentence(&form_spec.why));
            return;
        }
        // Fill the values from the defaults the first time this job's
        // form is shown.
        if self.run_values.job != job.id {
            self.run_values = RunValues {
                job: job.id.clone(),
                values: form_spec
                    .params
                    .iter()
                    .map(|p| {
                        let v = if p.kind == ParamKind::Secret {
                            String::new()
                        } else {
                            p.default.clone()
                        };
                        (p.name.clone(), v)
                    })
                    .collect(),
            };
        }
        if form_spec.params.is_empty() {
            form::hint(ui, theme, &s.t("automation-no-params"));
        }
        form::field_grid(ui, "automation-run-form", |ui| {
            for (i, p) in form_spec.params.iter().enumerate() {
                let Some((_, value)) = self.run_values.values.get_mut(i) else {
                    continue;
                };
                // The ref is ours, not the server's: it gets a label.
                let name = if p.name == crate::github::REF {
                    s.t("automation-col-ref")
                } else {
                    p.name.clone()
                };
                let label = if p.required {
                    format!("{name} *")
                } else {
                    name
                };
                form::label(ui, &label);
                match &p.kind {
                    ParamKind::Bool => {
                        let mut on = value == "true";
                        if ui.checkbox(&mut on, "").changed() {
                            *value = on.to_string();
                        }
                    }
                    ParamKind::Choice(options) => {
                        egui::ComboBox::from_id_salt(("automation-choice", i))
                            .width(form::FIELD_MIN_WIDTH)
                            .selected_text(value.clone())
                            .show_ui(ui, |ui| {
                                for o in options {
                                    hs_ui::form::selectable_value(ui, value, o.clone(), o);
                                }
                            });
                    }
                    ParamKind::LongText => {
                        ui.add(
                            egui::TextEdit::multiline(value)
                                .code_editor()
                                .desired_rows(4)
                                .desired_width(form::FIELD_MIN_WIDTH * 1.5),
                        );
                    }
                    ParamKind::Secret => {
                        form::secret_field(ui, value, "", form::FIELD_MIN_WIDTH);
                    }
                    ParamKind::Text => {
                        form::text_field(ui, value, "");
                    }
                    ParamKind::Pick(options) => {
                        form::pick_field(ui, theme, value, options, "");
                    }
                }
                ui.end_row();
                if !p.description.is_empty() {
                    form::row_hint(ui, theme, &p.description);
                }
            }
        });
        let missing: Vec<String> = form_spec
            .params
            .iter()
            .zip(&self.run_values.values)
            .filter(|(p, (_, v))| p.required && v.trim().is_empty())
            .map(|(p, _)| p.name.clone())
            .collect();
        ui.add_space(6.0);
        let reason = if self.read_only_profile {
            s.t("automation-refused-read-only")
        } else {
            s.t("automation-refused-protected")
        };
        let mut go = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !self.read_only() && missing.is_empty(),
                    egui::Button::new(format!("{} {}", ph::PLAY, s.t("automation-run"))),
                )
                .tip_disabled(if self.read_only() {
                    reason
                } else {
                    s.i18n
                        .t_args("automation-required", &[("names", &missing.join(", "))])
                })
                .clicked()
            {
                go = true;
            }
        });
        if go && self.guard(cx) {
            let values: Vec<(String, String)> = self
                .run_values
                .values
                .iter()
                // An empty optional value is left to the server's default.
                .filter(|(_, v)| !v.is_empty())
                .cloned()
                .collect();
            self.send(Cmd::Trigger(job.clone(), values));
            // A secret is typed for one run and not kept.
            for (p, (_, v)) in form_spec
                .params
                .iter()
                .zip(self.run_values.values.iter_mut())
            {
                if p.kind == ParamKind::Secret {
                    v.clear();
                }
            }
            self.state.detail = Detail::Runs;
        }
    }

    fn queue_pane(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let got = self.shared.lock().queue.clone();
        let Some(got) = got else {
            ui.spinner();
            return;
        };
        let items = match got {
            Ok(i) => i,
            Err(e) => {
                form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                return;
            }
        };
        if items.is_empty() {
            form::note(ui, theme, ph::INFO, &s.t("automation-queue-empty"));
            return;
        }
        egui::Grid::new("automation-queue")
            .num_columns(3)
            .striped(true)
            .show(ui, |ui| {
                for key in [
                    "automation-col-name",
                    "automation-col-why",
                    "automation-col-since",
                ] {
                    ui.strong(s.t(key));
                }
                ui.end_row();
                for q in &items {
                    ui.label(&q.job);
                    ui.label(&q.why);
                    ui.label(q.since_ms.map(driver::time_ms).unwrap_or_default());
                    ui.end_row();
                }
            });
    }

    fn dialogs(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let Some((job, run)) = self.confirm_cancel.take() else {
            return;
        };
        let body = s.i18n.t_args(
            "automation-cancel-confirm",
            &[("job", &job.name), ("run", &run.number)],
        );
        match hs_ui::dialog::confirm(
            ui.ctx(),
            &theme,
            "automation-cancel",
            &s.t("automation-cancel-title"),
            &body,
            &s.t("automation-cancel"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                if self.guard(cx) {
                    self.send(Cmd::Cancel(job, run));
                }
            }
            hs_ui::dialog::DialogResult::Cancel => {}
            hs_ui::dialog::DialogResult::Open => self.confirm_cancel = Some((job, run)),
        }
    }

    /// The review-only assistant (ADR-019), told about the job, its last
    /// runs and the end of the selected log - for "explain this failure".
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
        let job = self.job();
        let runs: String = {
            let sh = self.shared.lock();
            sh.runs
                .as_ref()
                .filter(|r| Some(&r.job) == job.as_ref().map(|j| &j.id))
                .and_then(|r| r.result.as_ref().ok())
                .map(|runs| {
                    runs.iter()
                        .take(10)
                        .map(|r| format!("{} {:?} {}", r.number, r.state, r.reference))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default()
        };
        let tail: String = {
            let sh = self.shared.lock();
            sh.log
                .as_ref()
                .map(|b| {
                    let start = b.text.len().saturating_sub(4000);
                    let start = (start..=b.text.len())
                        .find(|i| b.text.is_char_boundary(*i))
                        .unwrap_or(b.text.len());
                    b.text[start..].to_string()
                })
                .unwrap_or_default()
        };
        let context = format!(
            "Server: {} ({})\nJob: {}\nRecent runs:\n{runs}\n\nEnd of the selected run's log:\n{tail}",
            self.conn_name,
            hs_core::connections::kind_label(&self.kind),
            job.map(|j| j.name).unwrap_or_default(),
        );
        self.assistant.set_context("Automation", context);
        if self.assistant.quick_actions.is_empty() {
            self.assistant.quick_actions =
                hs_ui::ai_assistant::QuickAction::for_activity(s, "automation");
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
                let _ = self.assistant.ui(ui, s, &theme, "Automation");
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_protected_tab_is_read_only_until_unlocked() {
        let mut t = AutomationTool::new(State::default());
        t.protected = true;
        assert!(t.read_only());
        t.writes_unlocked = true;
        assert!(!t.read_only());
        t.read_only_profile = true;
        assert!(t.read_only());
    }
}
