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
pub mod docker;

use std::borrow::Cow;

use hs_container::Project;
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

/// How wide a Scan Local finding's row is drawn. Bounded so its Add
/// button sits in a column beside the finding; left to the pane, the
/// row would be the full width of the window and the action would float
/// at the far edge, which the house rule on row actions forbids.
const SCAN_ROW_WIDTH: f32 = 620.0;

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
    /// Which list the pane is showing.
    #[serde(default)]
    pub resource: docker::Resource,
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
    /// The row the detail block is showing, by **identity** rather than
    /// index: the list is re-read on every refresh, and an index would
    /// then point at a different thing. Which list it belongs to is
    /// part of the selection, or switching the rail would show the
    /// detail of something that is no longer on screen.
    selected: Option<(docker::Resource, String)>,
    /// `ui.input(|i| i.time)` of the frame the selection last changed.
    ///
    /// egui lays a frame out **twice** when a panel's size changes
    /// (`request_discard`), and `Response::clicked()` is pointer state
    /// rather than an event, so the second pass sees the same click
    /// still held. The detail block does not exist during the first
    /// pass - selecting a row is what creates it - so in the second its
    /// freshly-made buttons are laid out under a pointer that is still
    /// "clicking", and one of them fires. Selecting a project ran
    /// `config`; on the container list the same click would have
    /// reached **Remove**. Both passes of one frame share a `time`, so
    /// comparing against it is exactly the guard needed (observed
    /// 2026-10-24).
    selected_at: f64,
    /// A Compose verb waiting to be confirmed, and which project.
    confirm_compose: Option<(hs_container::ComposeVerb, String)>,
    /// What the last Compose or Swarm verb printed, kept until the next
    /// one: `up` and `deploy` say what they did, and throwing that away
    /// would leave the user guessing whether anything happened.
    last_output: Option<String>,
    /// The table's own filter and sort.
    table: hs_ui::table::TableState,
    /// A destructive verb waiting to be confirmed: what, to which id,
    /// and the name to put in the question.
    confirm: Option<(hs_container::Verb, String, String)>,
    /// What Scan Local found, once it has run. `None` before that, so
    /// the dashboard can tell "not asked" from "asked, found nothing".
    scan: Option<Vec<hs_container::scan::Found>>,
    /// Where the scan worker leaves its answer.
    scan_pending: Option<ScanPending>,
    /// Where a Compose verb leaves what it said and the re-read that
    /// followed it.
    compose_pending: Option<ComposePending>,
}

/// A slot a Compose worker fills: what the verb printed (or why it
/// failed), and the inventory as it stands afterwards.
type ComposePending =
    std::sync::Arc<parking_lot::Mutex<Option<(Result<String, String>, Result<Inventory, String>)>>>;

/// A slot the scan worker fills. The scan stats files and runs
/// `wsl.exe`, so it never runs on the UI thread (ADR-013).
type ScanPending = std::sync::Arc<parking_lot::Mutex<Option<Vec<hs_container::scan::Found>>>>;

/// A slot a worker thread fills with one host's inventory. The UI drains
/// it at frame start, as every other background result here arrives
/// (ADR-013).
type Pending = std::sync::Arc<parking_lot::Mutex<Option<Result<Inventory, String>>>>;

/// What one read of a host returned.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inventory {
    pub version: String,
    pub containers: Vec<hs_container::Container>,
    pub image_list: Vec<hs_container::Image>,
    pub volume_list: Vec<hs_container::Volume>,
    pub network_list: Vec<hs_container::Network>,
    /// Compose projects, grouped from the labels and corrected by
    /// `docker compose ls` (phase 5).
    pub projects: Vec<hs_container::Project>,
    /// Where this engine stands in a swarm. Most are in none, and that
    /// is a state the pane shows rather than an error.
    pub swarm: hs_container::swarm::SwarmState,
    pub stacks: Vec<hs_container::swarm::Stack>,
    pub services: Vec<hs_container::swarm::Service>,
    pub nodes: Vec<hs_container::swarm::Node>,
}

impl Inventory {
    /// The counts beside each rail entry, in `Resource::ALL` order.
    fn counts(&self) -> Vec<usize> {
        docker::Resource::ALL
            .iter()
            .map(|r| match r {
                docker::Resource::Containers => self.containers.len(),
                docker::Resource::Projects => self.projects.len(),
                docker::Resource::Images => self.image_list.len(),
                docker::Resource::Volumes => self.volume_list.len(),
                docker::Resource::Networks => self.network_list.len(),
                docker::Resource::Stacks => self.stacks.len(),
                docker::Resource::Services => self.services.len(),
                docker::Resource::Nodes => self.nodes.len(),
            })
            .collect()
    }
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
            selected: None,
            selected_at: f64::NEG_INFINITY,
            confirm_compose: None,
            last_output: None,
            table: hs_ui::table::TableState::default(),
            confirm: None,
            scan: None,
            scan_pending: None,
            compose_pending: None,
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

    /// Start Scan Local, off the UI thread.
    fn start_scan(&mut self, s: &Services) {
        if self.scan_pending.is_some() {
            return;
        }
        let waker = s.runtime.waker();
        let slot: ScanPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.scan_pending = Some(slot);
        s.runtime.spawn_blocking(move || {
            *out.lock() = Some(hs_container::scan::scan());
            waker.request_repaint();
        });
    }

    /// Take the scan's answer when the worker has it.
    fn drain_scan(&mut self) {
        let Some(slot) = &self.scan_pending else {
            return;
        };
        let Some(found) = slot.lock().take() else {
            return;
        };
        self.scan = Some(found);
        self.scan_pending = None;
    }

    /// Save one finding as a host. It goes in as an ordinary profile,
    /// which is the whole point of hosts being profiles (§10.1): nothing
    /// here is a second store, and the user edits it in Settings like
    /// any other.
    fn add_found(&self, s: &Services, found: &hs_container::scan::Found) -> bool {
        let mut profile = ConnectionProfile {
            name: found.name.clone(),
            kind: found.kind.to_string(),
            ..Default::default()
        };
        found.fields().write_into(&mut profile.fields);
        let mut conns = s.connections.write();
        match conns.upsert(profile) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("could not save the scanned host: {e}");
                false
            }
        }
    }

    /// The keys of every container host already saved, so a scan offers
    /// what is missing rather than what is there.
    fn saved_keys(s: &Services) -> std::collections::HashSet<String> {
        hosts(s)
            .iter()
            .map(|p| hs_container::scan::key_of(&p.kind, &ContainerFields::from_table(&p.fields)))
            .collect()
    }

    /// Take the answer when the worker has it.
    fn drain(&mut self) {
        // The guard is taken in its own scope: a `let` chain holds the
        // borrow of `self.pending` across the body, and the body has to
        // clear it.
        let read = self.pending.as_ref().and_then(|s| s.lock().take());
        if let Some(result) = read {
            self.inventory = Some(result);
            self.loading = false;
            self.pending = None;
        }
        let ran = self.compose_pending.as_ref().and_then(|s| s.lock().take());
        if let Some((said, inv)) = ran {
            // A failure is shown in the same block as a success: the
            // engine's own words are what the reader needs either way.
            self.last_output = Some(match said {
                Ok(text) if text.trim().is_empty() => String::new(),
                Ok(text) => text,
                Err(e) => e,
            })
            .filter(|t| !t.trim().is_empty());
            self.inventory = Some(inv);
            self.loading = false;
            self.compose_pending = None;
        }
    }
}

/// One read of a host, off the UI thread.
///
/// Only the version and the containers are allowed to fail the whole
/// read: everything else is extra, and losing the containers because
/// an engine cannot list volumes would be the wrong trade.
fn read_inventory(engine: &hs_container::Engine) -> Result<Inventory, String> {
    use hs_container::run;
    let version = run::probe(engine).map_err(|e| e.to_string())?;
    let containers = run::containers(engine).map_err(|e| e.to_string())?;
    let image_list = run::images(engine).unwrap_or_default();
    let volume_list = run::volumes(engine).unwrap_or_default();
    let network_list = run::networks(engine).unwrap_or_default();
    let projects = run::projects(engine).unwrap_or_default();
    // Swarm is read first, and the three listings are asked for **only**
    // if this engine is a manager: off a swarm they fail with "this node
    // is not a swarm manager", and three failed processes per refresh to
    // learn what `info` already said is waste.
    let swarm = run::swarm_state(engine).unwrap_or_default();
    let (stacks, services, nodes) = if swarm.can_manage() {
        (
            run::stacks(engine).unwrap_or_default(),
            run::services(engine).unwrap_or_default(),
            run::nodes(engine).unwrap_or_default(),
        )
    } else {
        Default::default()
    };
    Ok(Inventory {
        version,
        containers,
        image_list,
        volume_list,
        network_list,
        projects,
        swarm,
        stacks,
        services,
        nodes,
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
        self.drain_scan();
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
                    // Scan Local finds what is on this machine rather
                    // than asking the user to describe it.
                    let scanning = self.scan_pending.is_some();
                    if ui
                        .add_enabled(
                            !scanning,
                            egui::Button::new(format!(
                                "{} {}",
                                ph::MAGNIFYING_GLASS,
                                s.t("container-scan")
                            )),
                        )
                        .clicked()
                    {
                        self.start_scan(&s);
                    }
                    if scanning {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(s.t("container-scan-running"))
                                .small()
                                .color(theme.text_muted),
                        );
                    }
                });
                self.scan_findings(ui, &theme, &s);
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

    /// What Scan Local found, each offered as a host to add.
    fn scan_findings(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some(found) = self.scan.clone() else {
            return;
        };
        ui.add_space(6.0);
        if found.is_empty() {
            form::hint(ui, theme, &s.t("container-scan-none"));
            return;
        }
        form::hint(
            ui,
            theme,
            &s.i18n.t_args(
                "container-scan-found",
                &[("count", &found.len().to_string())],
            ),
        );
        let saved = Self::saved_keys(s);
        let mut added = false;
        // A bounded width, so Add sits in a column beside the finding
        // rather than floating at the far edge of the window (the house
        // rule on row actions, 2026-10-20).
        ui.scope(|ui| {
            ui.set_max_width(SCAN_ROW_WIDTH);
            for f in &found {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(kind_icon(f.kind)).size(16.0));
                    ui.label(&f.name);
                    ui.label(
                        egui::RichText::new(&f.detail)
                            .small()
                            .color(theme.text_muted),
                    );
                    hs_ui::toolbar::hard_right(ui, |ui| {
                        if saved.contains(&f.key()) {
                            ui.label(
                                egui::RichText::new(s.t("container-scan-have"))
                                    .small()
                                    .color(theme.text_muted),
                            );
                        } else if ui.small_button(s.t("container-scan-add")).clicked()
                            && self.add_found(s, f)
                        {
                            added = true;
                        }
                    });
                });
                ui.add_space(2.0);
            }
        });
        // The list is re-read from the store next frame, so a host just
        // added shows as added rather than being offered twice.
        if added {
            ui.ctx().request_repaint();
        }
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
                        &inv.image_list.len().to_string(),
                        &s.t("container-dash-images"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &inv.volume_list.len().to_string(),
                        &s.t("container-dash-volumes"),
                    );
                    hs_ui::dashboard::stat(
                        ui,
                        &theme,
                        &inv.network_list.len().to_string(),
                        &s.t("container-dash-networks"),
                    );
                    if !inv.projects.is_empty() {
                        hs_ui::dashboard::stat(
                            ui,
                            &theme,
                            &inv.projects.len().to_string(),
                            &s.t("container-dash-projects"),
                        );
                    }
                });
                form::hint(
                    ui,
                    &theme,
                    &s.i18n
                        .t_args("container-engine-version", &[("version", &inv.version)]),
                );
                ui.add_space(8.0);
                let inv = inv.clone();
                self.inventory(ui, &theme, &s, cx.actions, &profile, &inv);
            }
        }
        self.confirm_dialog(ui, &theme, &s);
        self.confirm_compose_dialog(ui, &theme, &s);
    }

    /// The Portainer-shaped pane (§10.6): a resource rail on the left,
    /// the chosen list beside it, and the detail block underneath.
    fn inventory(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        inv: &Inventory,
    ) {
        let counts = inv.counts();
        // The detail block takes its share off the top of the list, so
        // the two scroll independently rather than the list pushing the
        // detail off the bottom.
        let detail_h = if self.selected.is_some() { 230.0 } else { 0.0 };
        ui.horizontal_top(|ui| {
            if let Some(r) =
                docker::resource_rail(ui, theme, s, self.state.resource, &counts, inv.swarm)
            {
                self.state.resource = r;
                // The filter belonged to the list that is going away.
                self.table = hs_ui::table::TableState::default();
            }
            ui.separator();
            ui.vertical(|ui| {
                let full = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("container-list")
                    .max_height((full - detail_h).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.resource_table(ui, theme, s, inv);
                    });
                if self.selected.is_some() {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("container-detail")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.detail(ui, theme, s, cx_actions, profile, inv);
                        });
                }
            });
        });
    }

    /// The chosen resource, in the shared table so it scrolls both ways
    /// (§10.4).
    fn resource_table(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services, inv: &Inventory) {
        let res = self.state.resource;
        // A Swarm list off a swarm is not empty, it is unavailable, and
        // the two want different words.
        if res.is_swarm() && !inv.swarm.can_manage() {
            form::hint(ui, theme, &s.t(inv.swarm.key()));
            return;
        }
        let (columns, rows) = docker::table_data(res, s, inv);
        if rows.is_empty() {
            form::hint(ui, theme, &s.t(empty_key(res)));
            return;
        }
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
        let resp = table.show(ui, &mut self.table);
        // The table reports the index into the data it was given, not
        // into what the filter left showing.
        if let Some(i) = resp.clicked.or(resp.double_clicked)
            && let Some(id) = row_identity(res, inv, i)
        {
            let changed = self.selected.as_ref() != Some(&(res, id.clone()));
            self.selected = Some((res, id));
            // Read first, then store: taking the input lock inside a
            // `data_mut` is the other way to deadlock egui.
            self.selected_at = ui.input(|i| i.time);
            // What the last verb printed belongs to what it was run
            // against. Left standing, a project's `config` would appear
            // under the next project someone clicked, which reads as
            // that project's own output.
            if changed {
                self.last_output = None;
            }
        }
    }

    /// The detail block: what this container is, where its ports go, and
    /// the verbs.
    fn detail(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        inv: &Inventory,
    ) {
        let Some((res, id)) = self.selected.clone() else {
            return;
        };
        if res != self.state.resource {
            // The rail moved on; the detail belongs to the list it came
            // from, not to this one.
            return;
        }
        if res == docker::Resource::Projects {
            self.project_detail(ui, theme, s, cx_actions, profile, inv, &id);
            return;
        }
        if res != docker::Resource::Containers {
            // Images, volumes, networks and the Swarm lists have no
            // detail block yet; saying so beats an empty panel.
            form::hint(ui, theme, &s.t("container-detail-not-yet"));
            return;
        }
        let Some(c) = inv.containers.iter().find(|c| c.id == id).cloned() else {
            // It has gone since the last read, which is information.
            form::hint(ui, theme, &s.t("container-gone"));
            return;
        };
        let refusal = docker::guard(s, profile);
        // This block came into existence on the click that is still
        // being held; its buttons must not act on it.
        let fresh = ui.input(|i| i.time) == self.selected_at;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&c.name).strong());
            ui.label(
                egui::RichText::new(&c.image)
                    .small()
                    .color(theme.text_muted),
            );
            ui.label(
                egui::RichText::new(&c.status)
                    .small()
                    .color(if c.state.is_trouble() {
                        theme.error
                    } else {
                        theme.text_muted
                    }),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui.small_button(s.t("close")).clicked() {
                    self.selected = None;
                }
            });
        });
        ui.add_space(4.0);

        self.port_links(ui, theme, s, cx_actions, profile, &c);

        ui.horizontal_wrapped(|ui| {
            // Logs and a shell are handed to the tools that already do
            // them (§10.1); neither is rebuilt here, so the Logs tool's
            // filters, levels and assistant all come free.
            if ui
                .button(format!("{} {}", ph::SCROLL, s.t("container-view-logs")))
                .clicked()
                && !fresh
            {
                self.open_logs(s, cx_actions, profile, &c);
            }
            if ui
                .button(format!("{} {}", ph::TERMINAL_WINDOW, s.t("container-exec")))
                .clicked()
                && !fresh
            {
                self.open_exec(s, cx_actions, profile, &c);
            }
            ui.separator();
            for verb in docker::verbs_for(c.state) {
                let label = s.t(verb.key());
                // A refused verb is disabled and says why, not hidden:
                // hiding it reads as the tool lacking the feature rather
                // than the host being protected.
                let blocked = verb.destructive() && refusal.is_some();
                let r = ui.add_enabled(!blocked, egui::Button::new(label.as_str()));
                let r = match (&refusal, blocked) {
                    (Some(docker::Refusal::Protected(env)), true) => r.on_disabled_hover_text(
                        s.i18n
                            .t_args("container-refused-protected", &[("env", env)]),
                    ),
                    (Some(docker::Refusal::ReadOnly), true) => {
                        r.on_disabled_hover_text(s.t("container-refused-read-only"))
                    }
                    _ => r,
                };
                if r.clicked() && !fresh {
                    if verb.destructive() && s.settings.read().containers.confirm_stop {
                        self.confirm = Some((verb, c.id.clone(), c.name.clone()));
                    } else {
                        self.act(s, verb, &c.id);
                    }
                }
            }
        });
    }

    /// The published ports, as links into the built-in browser (owner's
    /// request, 2026-10-24).
    ///
    /// A port that cannot be reached is still shown, but as plain text:
    /// a link that goes nowhere is worse than no link.
    fn port_links(
        &self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        c: &hs_container::Container,
    ) {
        if c.ports.is_empty() {
            return;
        }
        let fields = ContainerFields::from_table(&profile.fields);
        let reach = hs_container::Reachable::of(&fields, self.ssh_host(s, profile).as_deref());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(s.t("container-ports"))
                    .small()
                    .color(theme.text_muted),
            );
            for p in &c.ports {
                let Some(url) = hs_container::port_url(p, &reach) else {
                    ui.label(
                        egui::RichText::new(p.describe())
                            .small()
                            .color(theme.text_muted),
                    )
                    .on_hover_text(s.t("container-port-not-open"));
                    continue;
                };
                let r = ui
                    .small_button(egui::RichText::new(p.describe()).color(theme.accent))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text(s.i18n.t_args("container-open-url", &[("url", &url)]));
                if r.clicked() {
                    open_url(cx_actions, &url);
                }
                // Right-click offers the same thing by name, and the
                // address itself for anything outside this app.
                let open_label = s.t("container-open-in-browser");
                let copy_label = s.t("container-copy-url");
                let menu_url = url.clone();
                r.context_menu(|ui| {
                    if ui.button(&open_label).clicked() {
                        open_url(cx_actions, &menu_url);
                        ui.close();
                    }
                    if ui.button(&copy_label).clicked() {
                        ui.ctx().copy_text(menu_url.clone());
                        ui.close();
                    }
                });
            }
        });
        ui.add_space(4.0);
    }

    /// The hostname of the SSH profile this host is reached through, so
    /// a published port is addressed on the right machine.
    ///
    /// `hs-container` cannot look this up itself: it must not depend on
    /// another tool's crate (ADR-004), so the tool resolves it and hands
    /// it over.
    fn ssh_host(&self, s: &Services, profile: &ConnectionProfile) -> Option<String> {
        let fields = ContainerFields::from_table(&profile.fields);
        if fields.tunnel_ssh.is_empty() {
            return None;
        }
        let conns = s.connections.read();
        let ssh = conns.by_id(&fields.tunnel_ssh)?;
        ssh.fields
            .get("ssh")
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("host"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }

    /// Follow this container's log in a Logs tab.
    fn open_logs(
        &self,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        c: &hs_container::Container,
    ) {
        let fields = ContainerFields::from_table(&profile.fields);
        let tail = s.settings.read().containers.log_tail_lines;
        let engine = hs_container::Engine::new(&profile.kind, fields.clone());
        let line = engine.follow_logs_line(&c.id, tail);
        // A remote engine's log is followed on that connection, which is
        // what the Logs tool's own command source does.
        let conn_id = (!fields.tunnel_ssh.is_empty()).then(|| fields.tunnel_ssh.clone());
        let source = serde_json::json!({
            "schema": 1,
            "name": format!("{} \u{b7} {}", profile.name, c.name),
            // `SourceSpec` is internally tagged: the variant is a `kind`
            // field beside the rest, not a wrapper around them.
            "spec": { "kind": "command", "conn_id": conn_id, "command": line },
            "format": "auto",
            "tail_lines": tail,
            "order": 0,
        });
        cx_actions.push(Action::OpenTool {
            factory_id: "logs".into(),
            request: Some(OpenRequest::Text {
                mime: LOG_SOURCE_MIME.to_string(),
                body: source.to_string(),
            }),
        });
    }

    /// Open a shell inside this container, in the Terminal activity.
    fn open_exec(
        &self,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        c: &hs_container::Container,
    ) {
        let fields = ContainerFields::from_table(&profile.fields);
        let engine = hs_container::Engine::new(&profile.kind, fields);
        // Which shell is the user's to choose (Settings → Containers):
        // a minimal image has no bash, and a distroless one has no
        // shell at all. The command is typed into a terminal they own,
        // so a wrong answer is visible and editable rather than silent.
        let shell = s.settings.read().containers.exec_shell.clone();
        let line = engine.exec_shell_line(&c.id, &shell);
        cx_actions.push(Action::OpenTool {
            factory_id: "terminal".into(),
            request: Some(OpenRequest::Terminal {
                cwd: None,
                host: None,
                profile: None,
                command: Some(line),
            }),
        });
    }

    /// Carry out a verb off the UI thread, then re-read the host.
    fn act(&mut self, s: &Services, verb: hs_container::Verb, id: &str) {
        let Some(profile) = self.host(s) else { return };
        let fields = ContainerFields::from_table(&profile.fields);
        let engine = hs_container::Engine::new(&profile.kind, fields);
        let id = id.to_string();
        let waker = s.runtime.waker();
        let slot: Pending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.pending = Some(slot);
        self.loading = true;
        s.runtime.spawn_blocking(move || {
            // The verb and the re-read on one thread, so what comes back
            // is the list *after* the verb rather than before it.
            let result = match hs_container::run::act(&engine, verb, &id) {
                Ok(()) => read_inventory(&engine),
                Err(e) => Err(e.to_string()),
            };
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// A Compose project's detail: what it is made of, where it came
    /// from, and the verbs.
    #[allow(clippy::too_many_arguments)]
    fn project_detail(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        inv: &Inventory,
        name: &str,
    ) {
        let Some(p) = inv.projects.iter().find(|p| p.name == name).cloned() else {
            form::hint(ui, theme, &s.t("container-gone"));
            return;
        };
        let refusal = docker::guard(s, profile);
        // Same guard as the container block: this one is created by the
        // very click its buttons would otherwise answer.
        let fresh = ui.input(|i| i.time) == self.selected_at;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&p.name).strong());
            ui.label(
                egui::RichText::new(s.i18n.t_args(
                    "container-compose-count",
                    &[
                        ("running", &p.running().to_string()),
                        ("total", &p.services.len().to_string()),
                    ],
                ))
                .small()
                .color(theme.text_muted),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui.small_button(s.t("close")).clicked() {
                    self.selected = None;
                }
            });
        });
        // Where it came from. Worth showing: a project is only as
        // reproducible as the file it was brought up from, and a
        // project with no file is one this pane cannot bring back up.
        if p.has_files() {
            ui.label(
                egui::RichText::new(p.config_files.join("  ·  "))
                    .small()
                    .color(theme.text_muted),
            );
        } else {
            form::hint(ui, theme, &s.t("container-compose-no-file"));
        }
        ui.add_space(4.0);

        ui.horizontal_wrapped(|ui| {
            for v in docker::compose_verbs_for(&p) {
                let label = s.t(v.key());
                let blocked = v.destructive() && refusal.is_some();
                let r = ui.add_enabled(!blocked, egui::Button::new(label.as_str()));
                let r = match (&refusal, blocked) {
                    (Some(docker::Refusal::Protected(env)), true) => r.on_disabled_hover_text(
                        s.i18n
                            .t_args("container-refused-protected", &[("env", env)]),
                    ),
                    (Some(docker::Refusal::ReadOnly), true) => {
                        r.on_disabled_hover_text(s.t("container-refused-read-only"))
                    }
                    _ => r,
                };
                if r.clicked() && !fresh {
                    // `down` removes containers and the network, so it
                    // asks first under the same setting that guards a
                    // container's own destructive verbs.
                    if v.destructive() && s.settings.read().containers.confirm_stop {
                        self.confirm_compose = Some((v, p.name.clone()));
                    } else {
                        self.run_compose(s, v, &p);
                    }
                }
            }
            ui.separator();
            // The services of a project are containers, and their logs
            // belong in the Logs tool like any other.
            if ui
                .button(format!("{} {}", ph::SCROLL, s.t("container-view-logs")))
                .clicked()
                && !fresh
                && let Some(first) = p.services.first()
                && let Some(c) = inv.containers.iter().find(|c| c.id == first.id)
            {
                self.open_logs(s, cx_actions, profile, c);
            }
        });

        if !p.services.is_empty() {
            ui.add_space(6.0);
            for sv in &p.services {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(if sv.state.is_running() {
                            hs_ui::icons::ph::CHECK_CIRCLE
                        } else {
                            hs_ui::icons::ph::X_CIRCLE
                        })
                        .color(if sv.state.is_running() {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                    );
                    ui.label(&sv.name);
                    ui.label(
                        egui::RichText::new(&sv.container)
                            .small()
                            .color(theme.text_muted),
                    );
                    ui.label(egui::RichText::new(&sv.status).small().color(
                        if sv.state.is_trouble() {
                            theme.error
                        } else {
                            theme.text_muted
                        },
                    ));
                });
            }
        }
        self.output_block(ui, theme, s);
    }

    /// What the last Compose or Swarm verb printed.
    ///
    /// Compose says what it did - "Container shop-web-1  Started" - on
    /// stderr, and dropping that would leave someone who pressed Up
    /// with no way to tell whether anything happened.
    fn output_block(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some(text) = self.last_output.clone() else {
            return;
        };
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("container-output"))
                    .small()
                    .strong()
                    .color(theme.text_muted),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui.small_button(s.t("close")).clicked() {
                    self.last_output = None;
                }
                if ui.small_button(s.t("container-copy-output")).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
            });
        });
        egui::ScrollArea::vertical()
            .id_salt("container-output")
            .max_height(120.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(text.trim()).monospace().small()).wrap(),
                );
            });
    }

    /// Run a Compose verb off the UI thread, then re-read the host.
    fn run_compose(&mut self, s: &Services, verb: hs_container::ComposeVerb, p: &Project) {
        let Some(profile) = self.host(s) else { return };
        let fields = ContainerFields::from_table(&profile.fields);
        let engine = hs_container::Engine::new(&profile.kind, fields);
        let name = p.name.clone();
        let files = p.config_files.clone();
        let waker = s.runtime.waker();
        let slot: ComposePending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.compose_pending = Some(slot);
        self.loading = true;
        s.runtime.spawn_blocking(move || {
            // The verb and the re-read on one thread, so what comes
            // back is the state *after* it rather than before.
            let said = hs_container::run::compose(&engine, &name, &files, verb);
            let inv = read_inventory(&engine);
            *out.lock() = Some((said.map_err(|e| e.to_string()), inv));
            waker.request_repaint();
        });
    }

    /// The question `down` asks first.
    fn confirm_compose_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some((verb, name)) = self.confirm_compose.clone() else {
            return;
        };
        let label = s.t(verb.key());
        match hs_ui::dialog::confirm(
            ui.ctx(),
            theme,
            "container-compose-verb",
            &label,
            &s.i18n
                .t_args("container-compose-confirm", &[("name", &name)]),
            &label,
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                self.confirm_compose = None;
                // Read the project again: the inventory may have been
                // refreshed while the question was on screen.
                let p = self
                    .inventory
                    .as_ref()
                    .and_then(|r| r.as_ref().ok())
                    .and_then(|inv| inv.projects.iter().find(|p| p.name == name).cloned());
                if let Some(p) = p {
                    self.run_compose(s, verb, &p);
                }
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm_compose = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }

    /// The question a destructive verb asks first.
    fn confirm_dialog(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some((verb, id, name)) = self.confirm.clone() else {
            return;
        };
        let label = s.t(verb.key());
        match hs_ui::dialog::confirm(
            ui.ctx(),
            theme,
            "container-verb",
            &label,
            &s.i18n.t_args(
                "container-confirm-verb",
                &[("verb", &label), ("name", &name)],
            ),
            &label,
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                self.confirm = None;
                self.act(s, verb, &id);
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }
}

/// The Logs tool's source MIME. Named here rather than depended on: a
/// tool may not depend on another tool's crate (ADR-004), and this
/// string is the published contract between the two.
const LOG_SOURCE_MIME: &str = "application/x-hyperstack-log-source+json";

/// What an empty list of each resource should say. "None" and "not
/// built yet" are different answers and must not share a message.
fn empty_key(r: docker::Resource) -> &'static str {
    match r {
        docker::Resource::Containers => "container-no-containers",
        docker::Resource::Projects => "container-no-projects",
        docker::Resource::Images => "container-no-images",
        docker::Resource::Volumes => "container-no-volumes",
        docker::Resource::Networks => "container-no-networks",
        docker::Resource::Stacks => "container-no-stacks",
        docker::Resource::Services => "container-no-services",
        docker::Resource::Nodes => "container-no-nodes",
    }
}

/// What identifies the row the user clicked, for the list it is in.
///
/// An id or a name rather than the row index: every list here is
/// re-read on each refresh, and an index would point at a different
/// thing a moment later.
fn row_identity(r: docker::Resource, inv: &Inventory, i: usize) -> Option<String> {
    match r {
        docker::Resource::Containers => inv.containers.get(i).map(|c| c.id.clone()),
        docker::Resource::Projects => inv.projects.get(i).map(|p| p.name.clone()),
        docker::Resource::Images => inv.image_list.get(i).map(|x| x.id.clone()),
        docker::Resource::Volumes => inv.volume_list.get(i).map(|v| v.name.clone()),
        docker::Resource::Networks => inv.network_list.get(i).map(|n| n.id.clone()),
        docker::Resource::Stacks => inv.stacks.get(i).map(|x| x.name.clone()),
        docker::Resource::Services => inv.services.get(i).map(|x| x.name.clone()),
        docker::Resource::Nodes => inv.nodes.get(i).map(|n| n.hostname.clone()),
    }
}

/// Open an address in the built-in Browser activity, in its own tab.
fn open_url(actions: &mut Vec<Action>, url: &str) {
    actions.push(Action::OpenTool {
        factory_id: "browser".into(),
        request: Some(OpenRequest::Url {
            url: url.to_string(),
        }),
    });
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
