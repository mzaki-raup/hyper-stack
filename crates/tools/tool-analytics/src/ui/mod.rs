//! The Analytics activity's screens (docs/tools/analytics.md §3): the
//! dashboard on the home tab, the saved servers in the sidebar, and one tab
//! per server with its header band, toolbar, browse column and item page.

mod dashboard;
mod sidebar;
mod tab;

use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Tool, ToolCategory, ToolContext, ToolDescriptor,
    ToolFactory,
};
use hs_ui::icons::ph;
use serde::{Deserialize, Serialize};

use crate::profile::{AnalyticsFields, ProfileError};
use crate::provider::metabase::Metabase;
use crate::provider::{BiProvider, Support};
use crate::session::Session;

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "analytics",
    title_key: "tool-analytics",
    icon: ph::CHART_BAR,
    category: ToolCategory::Data,
    accepts: &[Accept::Connection],
};

pub struct AnalyticsFactory;

impl ToolFactory for AnalyticsFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = AnalyticsTool {
            state,
            ..Default::default()
        };
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores. Never a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    /// The browse column is shown (§3.3: hideable).
    pub browse: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            conn_id: None,
            browse: true,
        }
    }
}

#[derive(Default)]
pub struct AnalyticsTool {
    state: State,
    conn_name: String,
    kind: String,
    address: String,
    /// The base URL, for the header's Server fact and Open in Browser.
    url: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    /// The profile as connected, so an edit of it reconnects.
    fields: Option<AnalyticsFields>,
    env_id: Option<String>,
    tls_off: bool,
    session: Option<Session>,
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

impl AnalyticsTool {
    /// Open (or reopen) the tab's session from its profile.
    fn connect(&mut self, s: &Services) {
        self.session = None;
        self.conn_error = None;
        self.waiting_for_vault = false;
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (kind, fields, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("analytics-no-profile"));
                return;
            };
            (
                c.kind.clone(),
                AnalyticsFields::from_table(&c.kind, &c.fields),
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.kind = kind.clone();
        self.address = fields.label_for(&kind);
        self.url = fields.url.clone();
        self.env_id = env_id.clone();
        self.fields = Some(fields.clone());
        self.tls_off = !fields.tls_verify;
        let lookup = {
            let envs = s.environments.read().clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let mut cfg = match fields.resolve(s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault =
                    e == ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked);
                self.conn_error = Some(match e {
                    ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked) => {
                        s.t("analytics-vault-locked")
                    }
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("analytics-missing-vars", &[("names", &v.join(", "))]),
                    other => other.to_string(),
                });
                return;
            }
        };
        cfg.timeout = std::time::Duration::from_secs(u64::from(
            s.settings.read().analytics.timeout_secs.max(5),
        ));
        let provider: Arc<dyn BiProvider> = match Metabase::new(cfg) {
            Ok(p) => Arc::new(p),
            Err(e) => {
                self.conn_error = Some(e);
                return;
            }
        };
        tracing::info!(
            "analytics: connecting {} to {}",
            self.conn_name,
            self.address
        );
        self.session = Some(Session::spawn(&s.runtime, provider));
    }
}

impl Tool for AnalyticsTool {
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> std::borrow::Cow<'_, str> {
        if self.conn_name.is_empty() {
            std::borrow::Cow::Borrowed("Analytics")
        } else {
            std::borrow::Cow::Borrowed(&self.conn_name)
        }
    }

    fn subtitle(&self) -> Option<String> {
        Some(self.address.clone()).filter(|s| !s.is_empty())
    }

    fn current_connection(&self) -> Option<&str> {
        self.state.conn_id.as_deref()
    }

    fn header(&self) -> Option<hs_core::tool::ToolHeader> {
        if self.conn_name.is_empty() {
            return None;
        }
        let info = self.session.as_ref().map(|s| s.info()).unwrap_or_default();
        let mut h = hs_ui::icons::conn_icon(&self.kind, "")
            .header(self.conn_name.clone())
            .plain(hs_core::connections::kind_label(&self.kind).to_string())
            .fact("Server", crate::profile::short(&self.url).to_string());
        if !info.version_text.is_empty() {
            h = h.fact("Version", info.version_text.clone());
        }
        if !info.signed_in_as.is_empty() {
            h = h.fact("Signed In As", info.signed_in_as.clone());
        }
        match info.support {
            Support::TooOld => h = h.fact("Support", "Version not supported".to_string()),
            Support::Newer => h = h.fact("Support", "Newer than tested".to_string()),
            _ => {}
        }
        if self.tls_off {
            h = h.fact("TLS", "Not verified".to_string());
        }
        Some(h)
    }

    fn tab_icon(&self) -> Option<&'static str> {
        (!self.kind.is_empty()).then(|| hs_ui::icons::connection_icon(&self.kind, ""))
    }

    fn tab_icon_uri(&self) -> Option<String> {
        self.tab_icon()?;
        hs_ui::icons::conn_icon(&self.kind, "").uri()
    }

    fn tab_icon_colour(&self) -> Option<egui::Color32> {
        self.tab_icon()?;
        hs_ui::icons::conn_icon(&self.kind, "").brand()
    }

    fn draws_toolbar(&self) -> bool {
        true
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if !cx.is_home && self.state.conn_id.is_some() {
            tab::toolbar(self, ui, cx);
        }
        let s = cx.services;
        let theme = hs_ui::theme::Theme::from_settings(&s.settings.read().appearance);
        hs_ui::toolbar::hard_right(ui, |ui| {
            if hs_ui::toolbar::btn(ui, &theme, ph::SPARKLE, &s.t("ai-assistant"))
                .on_hover_text(s.t("ai-shared-button-tip"))
                .clicked()
            {
                self.assistant.toggle("Analytics");
            }
        });
    }

    fn speakable(&self) -> Vec<hs_core::tool::Speakable> {
        self.assistant
            .last_answer()
            .map(|a| vec![hs_core::tool::Speakable::new("Assistant's last answer", a)])
            .unwrap_or_default()
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        tab::commands()
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        tab::run_command(self, id, cx)
    }

    fn on_event(&mut self, ev: &hs_core::events::Event, cx: &mut ToolContext) {
        match ev {
            hs_core::events::Event::ConnectionsChanged => {
                let Some(id) = self.state.conn_id.clone() else {
                    return;
                };
                let changed = {
                    let conns = cx.services.connections.read();
                    let Some(c) = conns.by_id(&id) else {
                        return;
                    };
                    self.conn_name = c.name.clone();
                    c.environment != self.env_id
                        || self.fields.as_ref()
                            != Some(&AnalyticsFields::from_table(&c.kind, &c.fields))
                };
                // An edit of the profile (its address or account) or of its
                // environment reaches an open tab at once: it reconnects.
                if changed && self.session.is_some() {
                    self.connect(cx.services);
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
                let ours = cx
                    .services
                    .connections
                    .read()
                    .by_id(&id)
                    .is_some_and(|c| hs_core::connections::is_analytics_kind(&c.kind));
                if !ours {
                    return false;
                }
                if self.state.conn_id.as_deref() == Some(id.as_str()) {
                    return true;
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

    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        sidebar::show(self, ui, cx)
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        if cx.is_home {
            dashboard::show(self, ui, cx);
            return;
        }
        let (mut content, assistant_rect) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        tab::body(self, &mut content, cx);
        tab::assistant_column(self, ui, assistant_rect, cx);
    }
}
