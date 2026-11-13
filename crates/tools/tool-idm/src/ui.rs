//! The tab (docs/tools/idm.md §3). This is the foundation's placeholder:
//! the dashboard on the home tab, the saved servers in the sidebar, and a
//! server's tab that connects through [`Session`] and says what it
//! reached. The sections of §4 are drawn on top of the same session.

use std::sync::Arc;

use hs_core::Services;
use hs_core::tool::{
    Accept, Action, OpenRequest, ToastLevel, Tool, ToolCategory, ToolContext, ToolDescriptor,
    ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditContext, AuditLog};
use crate::guarded::{Guarded, WriteGate};
use crate::profile::{ConnectConfig, IdmFields, ProfileError};
use crate::provider::keycloak::Keycloak;
use crate::provider::{IdmProvider, Support};
use crate::runner::{Session, Status};

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

/// What the workspace restores. Never a secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub conn_id: Option<String>,
    pub realm: String,
}

#[derive(Default)]
pub struct IdmTool {
    state: State,
    conn_name: String,
    kind: String,
    address: String,
    conn_error: Option<String>,
    waiting_for_vault: bool,
    session: Option<Session>,
}

impl IdmTool {
    /// Open (or reopen) the tab's session from its profile.
    fn connect(&mut self, s: &Services) {
        self.session = None;
        self.conn_error = None;
        self.waiting_for_vault = false;
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
        let inner: Arc<dyn IdmProvider> = match cfg {
            ConnectConfig::Keycloak(k) => match Keycloak::new(k) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    self.conn_error = Some(e);
                    return;
                }
            },
        };
        let audit = Arc::new(AuditLog::in_workspace(s.workspace.read().root()));
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

    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let tabs = s.open_tabs.of("idm");
        let saved: Vec<hs_ui::dashboard::Entry> = {
            let conns = s.connections.read();
            conns
                .of_idm_kinds()
                .map(|c| hs_ui::dashboard::Entry {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    detail: IdmFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                    icon: hs_ui::icons::conn_icon(&c.kind, "").glyph.to_string(),
                    image: hs_ui::icons::conn_icon(&c.kind, "").uri(),
                    tint: hs_ui::icons::conn_icon(&c.kind, "").fixed_colour(&theme),
                })
                .collect()
        };
        let labels = hs_ui::dashboard::DashboardLabels {
            open_heading: &s.t("dash-open"),
            nothing_open: &s.t("dash-nothing-open"),
            no_preview: &s.t("dash-no-preview"),
        };
        let page = hs_ui::dashboard::Page {
            icon: DESCRIPTOR.icon,
            title: &s.t("tool-idm"),
            subtitle: &s.t("idm-dashboard-sub"),
            saved: &s.t("idm-saved"),
            empty: &s.t("idm-none"),
            start_here: Some(&s.t("dash-start-here")),
            salt: "idm-dashboard",
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
                    label: &s.t("idm-stat-servers"),
                },
                hs_ui::dashboard::Stat {
                    icon: ph::COLUMNS,
                    value: &tabs.len().to_string(),
                    label: &s.t("dash-open-tabs"),
                },
            ],
            &[format!("{} {}", ph::PLUS, s.t("idm-add"))],
            &saved,
        );
        match pick {
            // Settings → Identity is the next step (FEAT-0109); until then a
            // Keycloak profile is added on the Connections page.
            Some(hs_ui::dashboard::Pick::Action(_)) => cx.actions.push(Action::RunCommand {
                id: "settings:connections".into(),
            }),
            Some(hs_ui::dashboard::Pick::Open(id)) => {
                cx.open("idm", Some(OpenRequest::Connection { id }));
            }
            Some(hs_ui::dashboard::Pick::Focus(tab_id)) => {
                cx.actions.push(Action::FocusTab { tab_id });
            }
            Some(hs_ui::dashboard::Pick::OpenExtra(_)) | Some(hs_ui::dashboard::Pick::Menu(_)) => {}
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
        form::pane(ui, |ui| {
            if self.state.conn_id.is_none() {
                form::note(ui, &theme, ph::PLUGS_CONNECTED, &s.t("idm-pick-hint"));
                return;
            }
            if let Some(e) = self.conn_error.clone() {
                form::note(ui, &theme, ph::WARNING, &hs_core::fmt::sentence(&e));
                ui.add_space(form::ROW_GAP);
                if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("idm-retry")).clicked() {
                    self.connect(s);
                }
                return;
            }
            let Some(session) = self.session.as_ref() else {
                return;
            };
            let (status, info, realms) = {
                let sh = session.shared();
                (
                    sh.status.clone(),
                    sh.info.clone(),
                    sh.realms
                        .as_ref()
                        .and_then(|r| r.as_ref().ok())
                        .map(Vec::len)
                        .unwrap_or(0),
                )
            };
            match status {
                Status::Connecting => {
                    form::note(
                        ui,
                        &theme,
                        ph::HOURGLASS,
                        &s.i18n
                            .t_args("idm-connecting", &[("server", &self.address)]),
                    );
                }
                Status::Failed(e) => {
                    form::note(
                        ui,
                        &theme,
                        ph::WARNING,
                        &hs_core::fmt::sentence(&e.to_string()),
                    );
                    ui.add_space(form::ROW_GAP);
                    if form::button(ui, &theme, ph::ARROWS_CLOCKWISE, &s.t("idm-retry")).clicked() {
                        self.connect(s);
                    }
                }
                Status::Ready => {
                    let version = if info.version_text.is_empty() {
                        s.t("idm-version-unknown")
                    } else {
                        info.version_text.clone()
                    };
                    form::note(
                        ui,
                        &theme,
                        ph::CHECK_CIRCLE,
                        &s.i18n.t_args(
                            "idm-connected",
                            &[
                                ("product", &info.product),
                                ("version", &version),
                                ("who", &info.signed_in_as),
                                ("realms", &realms.to_string()),
                            ],
                        ),
                    );
                }
            }
        });
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
            .fact("Server", crate::profile::short(&self.address).to_string());
        if !info.version_text.is_empty() {
            h = h.fact("Version", info.version_text.clone());
        }
        if !info.signed_in_as.is_empty() {
            h = h.fact(
                "Signed In As",
                format!("{} ({})", info.signed_in_as, info.auth_realm),
            );
        }
        if matches!(info.support, Support::TooOld | Support::Newer) {
            h = h.fact("Support", format!("{:?}", info.support));
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

    fn on_event(&mut self, ev: &hs_core::events::Event, cx: &mut ToolContext) {
        match ev {
            hs_core::events::Event::ConnectionsChanged => {
                if let Some(id) = self.state.conn_id.clone()
                    && let Some(c) = cx.services.connections.read().by_id(&id)
                {
                    self.conn_name = c.name.clone();
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
                let is_idm = cx
                    .services
                    .connections
                    .read()
                    .by_id(&id)
                    .is_some_and(|c| hs_core::connections::is_idm_kind(&c.kind));
                if !is_idm {
                    return false;
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
        let s = cx.services;
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if form::sidebar_head(
            ui,
            &theme,
            &s.t("idm-servers"),
            &[hs_ui::form::SideAction {
                icon: ph::PLUS,
                label: &s.t("idm-add"),
                tip: "",
            }],
        )
        .is_some()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:connections".into(),
            });
        }
        let profiles: Vec<(String, String, String, bool, hs_ui::icons::ConnIcon)> = {
            let conns = s.connections.read();
            conns
                .of_idm_kinds()
                .map(|c| {
                    (
                        c.id.clone(),
                        c.name.clone(),
                        IdmFields::from_table(&c.kind, &c.fields).label_for(&c.kind),
                        c.pinned,
                        hs_ui::icons::conn_icon(&c.kind, ""),
                    )
                })
                .collect()
        };
        if profiles.is_empty() {
            form::note(ui, &theme, ph::LIGHTBULB, &s.t("idm-none"));
            return true;
        }
        let mut open: Option<String> = None;
        let mut edit: Option<String> = None;
        let mut remove: Option<String> = None;
        let mut pin: Option<(String, bool)> = None;
        egui::ScrollArea::vertical()
            .id_salt("idm-servers")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (id, name, label, pinned, icon) in &profiles {
                    let selected = self.state.conn_id.as_deref() == Some(id.as_str());
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
                    if row.response.tip(s.t("idm-open")).clicked() && !selected {
                        open = Some(id.clone());
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
                    ui.add_space(2.0);
                }
            });
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
        if let Some(id) = open {
            cx.open("idm", Some(OpenRequest::Connection { id }));
        }
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        self.body(ui, cx);
    }
}
