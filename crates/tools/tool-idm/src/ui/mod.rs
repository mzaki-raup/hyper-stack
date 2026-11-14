//! The Identity activity's screens (docs/tools/idm.md §3): the dashboard
//! on the home tab, the saved servers in the sidebar, and one tab per
//! server with its browse column, the section's list and the item page.

mod clients;
mod common;
mod dashboard;
mod dialogs;
mod events;
mod export;
mod groups;
mod groups_pick;
mod realm;
mod roles;
mod roles_pick;
mod sessions;
mod sidebar;
mod tab;
mod users;

use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::{
    Accept, CommandDescriptor, OpenRequest, Tool, ToolCategory, ToolContext, ToolDescriptor,
    ToolFactory,
};
use hs_ui::icons::ph;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditContext, AuditLog};
use crate::guarded::{Guarded, WriteGate};
use crate::profile::{ConnectConfig, IdmFields, ProfileError};
use crate::provider::keycloak::Keycloak;
use crate::provider::{IdmProvider, Section, Support};
use crate::runner::Session;

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "idm",
    title_key: "tool-idm",
    icon: ph::IDENTIFICATION_BADGE,
    category: ToolCategory::Infra,
    accepts: &[Accept::Connection],
};

pub struct IdmFactory;

impl ToolFactory for IdmFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        let mut tool = IdmTool {
            state,
            ..Default::default()
        };
        if tool.state.conn_id.is_some() {
            tool.connect(services);
        }
        Box::new(tool)
    }
}

/// What the workspace restores (§8.2). Never a secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    pub realm: String,
    pub section: SectionId,
    /// The browse column is shown (§3.3: hideable).
    pub browse: bool,
    /// The selected user, kept so a restored tab opens on it.
    pub user: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        State {
            conn_id: None,
            realm: String::new(),
            section: SectionId::Users,
            browse: true,
            user: None,
        }
    }
}

/// [`Section`] as the layout stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SectionId {
    #[default]
    Users,
    Groups,
    RealmRoles,
    ClientRoles,
    Clients,
    Sessions,
    LoginEvents,
    AdminEvents,
    RealmSettings,
}

impl From<SectionId> for Section {
    fn from(s: SectionId) -> Section {
        match s {
            SectionId::Users => Section::Users,
            SectionId::Groups => Section::Groups,
            SectionId::RealmRoles => Section::RealmRoles,
            SectionId::ClientRoles => Section::ClientRoles,
            SectionId::Clients => Section::Clients,
            SectionId::Sessions => Section::Sessions,
            SectionId::LoginEvents => Section::LoginEvents,
            SectionId::AdminEvents => Section::AdminEvents,
            SectionId::RealmSettings => Section::RealmSettings,
        }
    }
}

impl From<Section> for SectionId {
    fn from(s: Section) -> SectionId {
        match s {
            Section::Users => SectionId::Users,
            Section::Groups => SectionId::Groups,
            Section::RealmRoles => SectionId::RealmRoles,
            Section::ClientRoles => SectionId::ClientRoles,
            Section::Clients => SectionId::Clients,
            Section::Sessions => SectionId::Sessions,
            Section::LoginEvents => SectionId::LoginEvents,
            Section::AdminEvents => SectionId::AdminEvents,
            Section::RealmSettings => SectionId::RealmSettings,
        }
    }
}

#[derive(Default)]
pub struct IdmTool {
    state: State,
    conn_name: String,
    kind: String,
    address: String,
    /// The base URL, for the header's Server fact.
    url: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    /// The profile is marked read-only (it cannot be unlocked, §6.1).
    read_only_profile: bool,
    /// The profile's environment is protected.
    protected: bool,
    /// The profile's environment as connected, to notice it change.
    env_id: Option<String>,
    env_name: String,
    tls_off: bool,
    session: Option<Session>,
    audit: Option<Arc<AuditLog>>,
    view: tab::TabView,
    /// The home tab's dashboard.
    dash: dashboard::DashState,
}

impl IdmTool {
    /// Open (or reopen) the tab's session from its profile. Reconnecting
    /// locks writes again (§6.1).
    fn connect(&mut self, s: &Services) {
        self.session = None;
        self.conn_error = None;
        self.waiting_for_vault = false;
        self.view.reset();
        let Some(id) = self.state.conn_id.clone() else {
            return;
        };
        let (kind, fields, read_only, env_id, name) = {
            let conns = s.connections.read();
            let Some(c) = conns.by_id(&id) else {
                self.conn_error = Some(s.t("idm-no-profile"));
                return;
            };
            (
                c.kind.clone(),
                IdmFields::from_table(&c.kind, &c.fields),
                c.read_only,
                c.environment.clone(),
                c.name.clone(),
            )
        };
        self.conn_name = name;
        self.kind = kind.clone();
        self.address = fields.label_for(&kind);
        self.url = fields.url.clone();
        self.read_only_profile = read_only;
        self.env_id = env_id.clone();
        self.tls_off = !fields.tls_verify;
        if self.state.realm.is_empty() {
            self.state.realm = fields.opening_realm().to_string();
        }
        let (protected, env_name) = {
            let envs = s.environments.read();
            let env = env_id
                .as_deref()
                .or(envs.active.as_deref())
                .and_then(|e| envs.by_id(e));
            (
                env.is_some_and(|e| e.protected),
                env.map(|e| e.name.clone()).unwrap_or_default(),
            )
        };
        self.protected = protected;
        self.env_name = env_name.clone();
        let lookup = {
            let envs = s.environments.read().clone();
            move |v: &str| envs.substitute(v, env_id.as_deref(), &[])
        };
        let cfg = match fields.resolve(&kind, s.secrets.as_ref(), &lookup) {
            Ok(c) => c,
            Err(e) => {
                self.waiting_for_vault =
                    e == ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked);
                self.conn_error = Some(match e {
                    ProfileError::Auth(hs_core::service_auth::AuthError::VaultLocked) => {
                        s.t("idm-vault-locked")
                    }
                    ProfileError::MissingVariables(v) => s
                        .i18n
                        .t_args("idm-missing-vars", &[("names", &v.join(", "))]),
                    other => other.to_string(),
                });
                return;
            }
        };
        let server = cfg.url().to_string();
        let timeout =
            std::time::Duration::from_secs(u64::from(s.settings.read().idm.timeout_secs.max(5)));
        let inner: Arc<dyn IdmProvider> = match cfg {
            ConnectConfig::Keycloak(mut k) => {
                k.timeout = timeout;
                match Keycloak::new(k) {
                    Ok(p) => Arc::new(p),
                    Err(e) => {
                        self.conn_error = Some(e);
                        return;
                    }
                }
            }
        };
        let audit = AuditLog::shared(s.workspace.read().root());
        self.audit = Some(audit.clone());
        let provider = Arc::new(Guarded::new(
            inner,
            WriteGate::new(read_only, protected, &env_name),
            Some(audit),
            AuditContext {
                connection: id,
                server,
            },
        ));
        tracing::info!("idm: connecting {} to {}", self.conn_name, self.address);
        self.session = Some(Session::spawn(&s.runtime, provider));
    }

    /// Why this tab refuses writes now, in words, or `None`.
    fn refusal(&self, s: &Services) -> Option<String> {
        let gate = self.session.as_ref()?.gate();
        if !gate.read_only() {
            return None;
        }
        Some(if self.read_only_profile {
            s.t("idm-refused-read-only")
        } else {
            s.i18n
                .t_args("idm-refused-protected", &[("env", &self.env_name)])
        })
    }
}

impl Tool for IdmTool {
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
            std::borrow::Cow::Borrowed("Identity")
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
        let info = self
            .session
            .as_ref()
            .map(|s| s.shared().info.clone())
            .unwrap_or_default();
        let mut h = hs_ui::icons::conn_icon(&self.kind, "")
            .header(self.conn_name.clone())
            .plain(hs_core::connections::kind_label(&self.kind).to_string())
            .fact("Server", crate::profile::short(&self.url).to_string());
        h = h.fact(
            "Version",
            if info.version_text.is_empty() {
                "not shown to this account".to_string()
            } else {
                info.version_text.clone()
            },
        );
        if !info.signed_in_as.is_empty() {
            h = h.fact(
                "Signed In As",
                format!("{} ({})", info.signed_in_as, info.auth_realm),
            );
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
        if cx.is_home || self.state.conn_id.is_none() {
            return;
        }
        tab::toolbar(self, ui, cx);
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
                    c.environment != self.env_id || c.read_only != self.read_only_profile
                };
                // A protection change reaches an open tab at once: it
                // reconnects, which also locks Allow Writes again (§6.1).
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
        let mut st = self.state.clone();
        st.user = self.view.selected_user();
        serde_json::to_value(&st).unwrap_or(serde_json::Value::Null)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        match item {
            OpenRequest::Connection { id } => {
                let is_idm = cx
                    .services
                    .connections
                    .read()
                    .by_id(&id)
                    .is_some_and(|c| hs_core::connections::is_idm_kind(&c.kind));
                if !is_idm {
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
        } else {
            tab::body(self, ui, cx);
        }
    }
}
