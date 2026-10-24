//! The Container activity (`docs/tools/containers.md` §10, ADR-026):
//! Docker, Podman, Kubernetes and Windows container hosts.
//!
//! Phase 3 is the shell — the rail entry, the dashboard, the
//! two-section sidebar and one tab per host. The inventory panes are
//! phase 4; a host tab here shows what the driver can already answer
//! and says plainly what is not built.
//!
//! A host is a `ConnectionProfile` of a container kind, never a store
//! of its own (§10.1), so everything about credentials, environments
//! and ordering is inherited.

pub mod configs;

use std::borrow::Cow;

use hs_container::profile::ContainerFields;
use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::tool::{
    Accept, Action, CommandDescriptor, OpenRequest, Tool, ToolCategory, ToolContext,
    ToolDescriptor, ToolFactory,
};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use serde::{Deserialize, Serialize};

pub static DESCRIPTOR: ToolDescriptor = ToolDescriptor {
    id: "containers",
    title_key: "tool-containers",
    icon: ph::SHIPPING_CONTAINER,
    category: ToolCategory::Infra,
    accepts: &[Accept::Connection],
};

/// A host travels to a new tab under this, carrying only its profile id:
/// the profile itself is in the store, and copying it into a tab would
/// be a second source of truth for something with secrets in it.
pub const HOST_MIME: &str = "application/x-hyperstack-container-host";

pub struct ContainersFactory;

impl ToolFactory for ContainersFactory {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }
    fn create(&self, services: &Services, restore: Option<serde_json::Value>) -> Box<dyn Tool> {
        let state: State = restore
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        Box::new(ContainersTool::new(services, state))
    }
}

/// What the workspace restores. The profile id only — never a secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// The host this tab shows. Empty for the dashboard.
    pub host_id: String,
    /// The host's name, so the tab strip can be labelled without a
    /// lookup. `title()` is asked every frame and is handed no
    /// `Services`, so an `Action::SetTitle` would be overwritten the
    /// moment it landed - the name has to live here.
    #[serde(default)]
    pub host_name: String,
}

pub struct ContainersTool {
    state: State,
    is_home: bool,
    /// The saved commands and configs, reloaded when another tab writes.
    configs: Vec<configs::SavedConfig>,
    configs_at: u64,
    /// What the driver last said about this host, and whether a read is
    /// in flight. Phase 4 turns this into the inventory pane; phase 3
    /// uses it to prove the whole path works end to end.
    inventory: Option<Result<Inventory, String>>,
    loading: bool,
    /// Where the worker leaves its answer. `None` when nothing is in
    /// flight.
    pending: Option<Pending>,
}

/// A slot a worker thread fills with one host's inventory. The UI drains
/// it at frame start, as every other background result here arrives
/// (ADR-013).
type Pending = std::sync::Arc<parking_lot::Mutex<Option<Result<Inventory, String>>>>;

/// What one read of a host returned.
#[derive(Debug, Clone, PartialEq)]
pub struct Inventory {
    pub version: String,
    pub containers: Vec<hs_container::Container>,
    pub images: usize,
    pub volumes: usize,
    pub networks: usize,
}

/// Bumped whenever a tab writes the saved configs, so every other tab
/// reloads rather than showing a stale list — the same mechanism the
/// Logs sidebar needed once two tabs could disagree.
static CONFIGS_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Say that the saved configs on disk have changed, so every other tab
/// reloads instead of showing a stale list.
///
/// Nothing in phase 3 writes a config - the list is read-only until
/// Compose arrives in phase 5 - so this exists for the writer to call
/// rather than being wired to a button that is not there yet. Reading
/// without it would be the bug the Logs sidebar had: two tabs
/// disagreeing about the same list.
pub fn configs_changed() {
    CONFIGS_VERSION.fetch_add(1, std::sync::atomic::Ordering::Release);
}

fn configs_version() -> u64 {
    CONFIGS_VERSION.load(std::sync::atomic::Ordering::Acquire)
}

/// The glyph for a container kind: what the thing **is** before where it
/// runs (§10.5).
pub fn kind_icon(kind: &str) -> &'static str {
    match kind {
        "k8s" => ph::POLYGON,
        "podman" => ph::HARD_DRIVES,
        "wincontainer" => ph::WINDOWS_LOGO,
        _ => ph::SHIPPING_CONTAINER,
    }
}

/// Every container host in this workspace, in the user's order.
fn hosts(s: &Services) -> Vec<ConnectionProfile> {
    s.connections
        .read()
        .all()
        .iter()
        .filter(|p| hs_core::connections::is_container_kind(&p.kind))
        .cloned()
        .collect()
}

impl ContainersTool {
    fn new(services: &Services, state: State) -> Self {
        let root = services.workspace.read().root().to_path_buf();
        Self {
            state,
            is_home: false,
            configs: configs::SavedConfig::load_all(&root),
            configs_at: configs_version(),
            inventory: None,
            loading: false,
            pending: None,
        }
    }

    /// Pick up another tab's save or delete.
    fn sync_configs(&mut self, s: &Services) {
        let now = configs_version();
        if self.configs_at == now {
            return;
        }
        self.configs_at = now;
        let root = s.workspace.read().root().to_path_buf();
        self.configs = configs::SavedConfig::load_all(&root);
    }

    fn host(&self, s: &Services) -> Option<ConnectionProfile> {
        if self.state.host_id.is_empty() {
            return None;
        }
        s.connections.read().by_id(&self.state.host_id).cloned()
    }

    /// Read the host's inventory on a worker thread. The driver shells
    /// out, so it must never run on the UI thread (ADR-013).
    fn reload(&mut self, s: &Services) {
        let Some(profile) = self.host(s) else { return };
        if self.loading {
            return;
        }
        // A Kubernetes cluster has no engine driver yet; say so rather
        // than running `docker` against it and reporting a confusing
        // failure.
        if hs_core::connections::is_cluster_kind(&profile.kind) {
            self.inventory = Some(Err(s.t("container-cluster-not-yet")));
            return;
        }
        let fields = ContainerFields::from_table(&profile.fields);
        let engine = hs_container::Engine::new(&profile.kind, fields);
        self.loading = true;
        let waker = s.runtime.waker();
        let slot: Pending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.pending = Some(slot);
        s.runtime.spawn_blocking(move || {
            let result = read_inventory(&engine);
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// Take the answer when the worker has it.
    fn drain(&mut self) {
        let Some(slot) = &self.pending else { return };
        let Some(result) = slot.lock().take() else {
            return;
        };
        self.inventory = Some(result);
        self.loading = false;
        self.pending = None;
    }
}

/// One read of a host, off the UI thread.
fn read_inventory(engine: &hs_container::Engine) -> Result<Inventory, String> {
    use hs_container::run;
    let version = run::probe(engine).map_err(|e| e.to_string())?;
    let containers = run::containers(engine).map_err(|e| e.to_string())?;
    // The counts are cheap and the dashboard wants them; a failure here
    // is not worth losing the containers over, so they default to zero.
    let images = run::images(engine).map(|v| v.len()).unwrap_or(0);
    let volumes = run::volumes(engine).map(|v| v.len()).unwrap_or(0);
    let networks = run::networks(engine).map(|v| v.len()).unwrap_or(0);
    Ok(Inventory {
        version,
        containers,
        images,
        volumes,
        networks,
    })
}

// --- the UI ----------------------------------------------------------

impl ContainersTool {
    /// The dashboard (§10.5): what this workspace has, what to do next,
    /// and the hosts as rows that open them.
    ///
    /// The counts here are only what is already known - a dashboard that
    /// probed every configured host on every frame would spawn a process
    /// per host per frame, which is the mistake the Logs service scan
    /// made once and paid for.
    fn dashboard(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.sync_configs(&s);
        form::page_title(
            ui,
            &theme,
            &s.t("tool-containers"),
            Some(&s.t("container-dashboard-sub")),
        );
        let all = hosts(&s);
        let engines = all
            .iter()
            .filter(|p| !hs_core::connections::is_cluster_kind(&p.kind))
            .count();
        let clusters = all.len() - engines;
        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &engines.to_string(),
                &s.t("container-dash-engines"),
            );
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &clusters.to_string(),
                &s.t("container-dash-clusters"),
            );
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &self.configs.len().to_string(),
                &s.t("container-dash-configs"),
            );
            hs_ui::dashboard::stat(
                ui,
                &theme,
                &s.open_tabs.of("containers").len().to_string(),
                &s.t("dash-open-tabs"),
            );
        });
        egui::ScrollArea::vertical()
            .id_salt("container-dashboard")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(s.t("container-dash-start"))
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(format!("{} {}", ph::PLUS, s.t("container-add")))
                        .clicked()
                    {
                        cx.actions.push(Action::RunCommand {
                            id: "settings:containers".into(),
                        });
                    }
                });
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(s.t("container-dash-hosts"))
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(6.0);
                if all.is_empty() {
                    form::hint(ui, &theme, &s.t("container-none"));
                }
                let mut open: Option<String> = None;
                for p in &all {
                    let f = ContainerFields::from_table(&p.fields);
                    let subtitle = format!(
                        "{} · {}",
                        hs_core::connections::kind_label(&p.kind),
                        f.describe(&p.kind)
                    );
                    if form::list_row(
                        ui,
                        &theme,
                        false,
                        kind_icon(&p.kind),
                        &p.name,
                        Some(&subtitle),
                    )
                    .clicked()
                    {
                        open = Some(p.id.clone());
                    }
                    ui.add_space(2.0);
                }
                if let Some(id) = open {
                    open_host(cx, &id);
                }
                // The shared Open Here grid, as every other activity's
                // dashboard shows it: the question "what have I got open
                // here, and take me to it" is the same everywhere, and an
                // activity that answered it differently would be the odd
                // one out (owner's rule on one UI language, 2026-10-20).
                let tabs = s.open_tabs.of("containers");
                let labels = hs_ui::dashboard::DashboardLabels {
                    open_heading: &s.t("dash-open"),
                    nothing_open: &s.t("dash-nothing-open"),
                    no_preview: &s.t("dash-no-preview"),
                };
                if let Some(id) = hs_ui::dashboard::open_tabs(ui, &theme, &tabs, &labels) {
                    cx.actions.push(Action::FocusTab { tab_id: id });
                }
            });
    }

    /// One host's tab. Phase 3 proves the path end to end and says what
    /// is not built; phase 4 replaces this with the Portainer-shaped
    /// pane of §10.6.
    fn host_pane(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.drain();
        let Some(profile) = self.host(&s) else {
            form::hint(ui, &theme, &s.t("container-host-gone"));
            return;
        };
        // The first read happens when the tab is first drawn, not when it
        // is created: a workspace that restores ten tabs must not probe
        // ten engines at start-up.
        if self.inventory.is_none() && !self.loading {
            self.reload(&s);
        }
        let f = ContainerFields::from_table(&profile.fields);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(kind_icon(&profile.kind)).size(16.0));
            ui.label(egui::RichText::new(&profile.name).strong().size(15.0));
            ui.label(
                egui::RichText::new(f.describe(&profile.kind))
                    .small()
                    .color(theme.text_muted),
            );
            if self.loading {
                ui.spinner();
            }
        });
        ui.separator();
        match &self.inventory {
            None => {
                form::hint(ui, &theme, &s.t("container-reading"));
            }
            Some(Err(e)) => {
                // The driver's own words: it already tells an
                // unreachable daemon from a refusal, and the remedies
                // differ.
                form::note(ui, &theme, hs_ui::icons::WARNING, e);
            }
            Some(Ok(inv)) => {
                let summary = hs_container::Summary::of(&inv.containers);
                ui.horizontal(|ui| {
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &summary.running.to_string(),
                        &s.t("container-dash-running"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &summary.stopped.to_string(),
                        &s.t("container-dash-stopped"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &inv.images.to_string(),
                        &s.t("container-dash-images"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &inv.volumes.to_string(),
                        &s.t("container-dash-volumes"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &inv.networks.to_string(),
                        &s.t("container-dash-networks"),
                    );
                });
                form::hint(
                    ui,
                    &theme,
                    &s.i18n
                        .t_args("container-engine-version", &[("version", &inv.version)]),
                );
                ui.add_space(8.0);
                self.container_table(ui, &theme, &s, &inv.containers.clone());
            }
        }
    }

    /// The containers, in the shared table so it scrolls both ways
    /// (§10.4). Phase 4 adds the verbs and the detail block.
    fn container_table(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        containers: &[hs_container::Container],
    ) {
        if containers.is_empty() {
            form::hint(ui, theme, &s.t("container-no-containers"));
            return;
        }
        let columns = [
            hs_ui::table::ColumnSpec::new(s.t("container-col-name"), 220.0),
            hs_ui::table::ColumnSpec::new(s.t("container-col-image"), 240.0),
            hs_ui::table::ColumnSpec::new(s.t("container-col-state"), 110.0),
            hs_ui::table::ColumnSpec::new(s.t("container-col-status"), 180.0),
            hs_ui::table::ColumnSpec::new(s.t("container-col-ports"), 220.0),
        ];
        let rows: Vec<Vec<String>> = containers
            .iter()
            .map(|c| {
                vec![
                    c.name.clone(),
                    c.image.clone(),
                    s.t(c.state.key()),
                    c.status.clone(),
                    c.ports
                        .iter()
                        .map(|p| p.describe())
                        .collect::<Vec<_>>()
                        .join(", "),
                ]
            })
            .collect();
        let table = hs_ui::table::DataTable {
            theme,
            columns: &columns,
            rows: &rows,
            row_height: 20.0,
            show_filter: true,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
        };
        let mut state = hs_ui::table::TableState::default();
        table.show(ui, &mut state);
    }
}

/// Open a host, or focus the tab that already shows it. One tab per
/// host: `current_uri` is what the shell matches on (§10.9).
fn open_host(cx: &mut ToolContext, host_id: &str) {
    cx.actions.push(Action::OpenTool {
        factory_id: "containers".into(),
        request: Some(OpenRequest::Text {
            mime: HOST_MIME.into(),
            body: host_id.to_string(),
        }),
    });
}

impl Tool for ContainersTool {
    fn descriptor(&self) -> &ToolDescriptor {
        &DESCRIPTOR
    }

    fn title(&self) -> Cow<'_, str> {
        if self.is_home || self.state.host_id.is_empty() {
            return Cow::Borrowed("Containers");
        }
        if self.state.host_name.is_empty() {
            // A profile deleted since this tab was saved: its id is not
            // a useful label, and the activity's name is honest.
            return Cow::Borrowed("Containers");
        }
        Cow::Borrowed(&self.state.host_name)
    }

    /// A dashboard reports **no** uri, or it takes every request for the
    /// activity and clicking a host in the sidebar reads as a dead
    /// click (the Devtools lesson, §10.5).
    fn current_uri(&self) -> Option<String> {
        if self.is_home || self.state.host_id.is_empty() {
            return None;
        }
        Some(format!("container://{}", self.state.host_id))
    }

    /// The activity has a permanent home tab, and that home is a
    /// dashboard. Without `wants_home_tab` the shell never creates one,
    /// which is how the first run of this activity opened a host tab
    /// with no host in it.
    fn wants_home_tab(&self) -> bool {
        true
    }

    fn home_is_dashboard(&self) -> bool {
        true
    }

    fn wants_dashboard_home(&self) -> bool {
        true
    }

    fn set_home_tab(&mut self, home: bool, _services: &Services) {
        self.is_home = home;
    }

    fn save_state(&self) -> serde_json::Value {
        serde_json::to_value(&self.state).unwrap_or(serde_json::Value::Null)
    }

    fn open(&mut self, item: OpenRequest, cx: &mut ToolContext) -> bool {
        let wanted = match item {
            // The sidebar's own route.
            OpenRequest::Text { mime, body } if mime == HOST_MIME => Some(body),
            // How Home's pins, the launcher and Settings open a host.
            OpenRequest::Connection { id } => Some(id),
            _ => None,
        };
        match wanted {
            Some(body) => {
                let id = body.trim().to_string();
                // Only a container host: a request for a database must
                // not open an empty tab here.
                let is_container = cx
                    .services
                    .connections
                    .read()
                    .by_id(&id)
                    .is_some_and(|p| hs_core::connections::is_container_kind(&p.kind));
                if id.is_empty() || !is_container {
                    return false;
                }
                if self.state.host_id != id {
                    self.state.host_id = id;
                    // A different host: everything known about the last
                    // one is wrong now.
                    self.inventory = None;
                    self.pending = None;
                    self.loading = false;
                }
                if let Some(p) = self.host(cx.services) {
                    self.state.host_name = p.name;
                }
                true
            }
            None => false,
        }
    }

    /// How well this tab matches a uri, so the shell focuses the tab
    /// that already shows a host rather than opening a second
    /// (§10.9). 0 is an exact match; a dashboard never matches.
    fn uri_affinity(&self, uri: &str) -> Option<u8> {
        if self.is_home {
            return None;
        }
        (self.current_uri().as_deref() == Some(uri)).then_some(0)
    }

    fn commands(&self) -> Vec<CommandDescriptor> {
        vec![
            CommandDescriptor {
                id: "containers.add".into(),
                title: "Containers: Add Container…".into(),
                shortcut_action: None,
            },
            CommandDescriptor {
                id: "containers.refresh".into(),
                title: "Containers: Refresh".into(),
                shortcut_action: None,
            },
        ]
    }

    fn run_command(&mut self, id: &str, cx: &mut ToolContext) -> bool {
        match id {
            "containers.add" => {
                cx.actions.push(Action::RunCommand {
                    id: "settings:containers".into(),
                });
                true
            }
            "containers.refresh" => {
                self.inventory = None;
                self.reload(cx.services);
                true
            }
            _ => false,
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        if self.is_home || self.state.host_id.is_empty() {
            return;
        }
        if form::icon_button(ui, &theme, ph::ARROW_CLOCKWISE, 16.0, self.loading)
            .tip(s.t("container-refresh"))
            .clicked()
        {
            self.run_command("containers.refresh", cx);
        }
    }

    /// The sidebar: hosts above, saved commands and configs below, each
    /// scrolling on its own so a long list never pushes the other off
    /// (§10.5).
    fn sidebar(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) -> bool {
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        self.sync_configs(&s);
        form::header_bar(ui, &theme, |ui| {
            ui.label(
                egui::RichText::new(s.t("tool-containers"))
                    .strong()
                    .size(15.0),
            );
        });
        // Add goes to Settings, as Add Connection does from every other
        // activity: one place manages a host, not two.
        if form::primary_button(
            ui,
            &theme,
            &format!("{} {}", ph::PLUS, s.t("container-add")),
        )
        .clicked()
        {
            cx.actions.push(Action::RunCommand {
                id: "settings:containers".into(),
            });
        }
        ui.add_space(6.0);

        let full = ui.available_height();
        let hosts_height = (full * 0.40).max(90.0);
        ui.label(
            egui::RichText::new(s.t("container-section-hosts"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        let all = hosts(&s);
        let current = self.current_uri();
        let mut open: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("container-hosts")
            .max_height(hosts_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if all.is_empty() {
                    form::hint(ui, &theme, &s.t("container-none"));
                }
                for p in &all {
                    let f = ContainerFields::from_table(&p.fields);
                    let live = current.as_deref() == Some(&format!("container://{}", p.id));
                    if form::list_row(
                        ui,
                        &theme,
                        live,
                        kind_icon(&p.kind),
                        &p.name,
                        Some(&f.describe(&p.kind)),
                    )
                    .clicked()
                    {
                        open = Some(p.id.clone());
                    }
                    ui.add_space(2.0);
                }
            });
        if let Some(id) = open {
            open_host(cx, &id);
        }
        ui.separator();
        ui.label(
            egui::RichText::new(s.t("container-section-configs"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        egui::ScrollArea::vertical()
            .id_salt("container-configs")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if self.configs.is_empty() {
                    form::hint(ui, &theme, &s.t("container-no-configs"));
                }
                for c in &self.configs {
                    let r = form::list_row(
                        ui,
                        &theme,
                        false,
                        c.kind.icon(),
                        &c.name,
                        Some(&c.describe()),
                    );
                    // Opening one is phase 5 (Compose) and phase 6
                    // (manifests); saying so beats a dead click.
                    if r.clicked() {
                        cx.toast(
                            hs_core::tool::ToastLevel::Info,
                            s.t("container-config-not-yet"),
                        );
                    }
                    ui.add_space(2.0);
                }
            });
        true
    }

    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut ToolContext) {
        // No host is the dashboard, whatever the home flag says. Keying
        // this on `is_home` alone showed "this host has been deleted" in
        // a tab that had simply never been given one.
        if self.is_home || self.state.host_id.is_empty() {
            self.dashboard(ui, cx);
        } else {
            self.host_pane(ui, cx);
        }
    }
}
