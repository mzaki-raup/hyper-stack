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

pub mod action;
pub mod cluster;
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
    /// Which list a **cluster** tab is showing. Its own field: the two
    /// panes have different resources, and sharing one would mean a
    /// cluster tab restoring onto "Volumes".
    #[serde(default)]
    pub cluster_resource: cluster::Resource,
    /// The namespace a cluster tab is narrowed to. Empty is every
    /// namespace, which is what a cluster-wide search wants.
    #[serde(default)]
    pub namespace: String,
    /// Whether the reader has chosen a namespace in this tab.
    ///
    /// Without this, "All namespaces" is unreachable on any profile
    /// that names a default one: choosing it empties `namespace`, and
    /// the read that follows treats empty as "not set yet" and puts the
    /// profile's namespace straight back (seen on the bench,
    /// 2026-10-24). An empty string is a real choice, not a missing
    /// one, and only this flag can tell the two apart.
    #[serde(default)]
    pub namespace_set: bool,
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
    /// What the cluster driver last said, for a `k8s` host.
    cluster: Option<Result<cluster::Inventory, String>>,
    /// Where the cluster worker leaves its answer.
    cluster_pending: Option<ClusterPending>,
    /// The cluster row the detail block is showing, as a **position**
    /// in the inventory on screen rather than an identity.
    ///
    /// The Docker pane keeps an identity because it re-reads after
    /// every verb and the row must survive that; a cluster read happens
    /// only when the reader asks for one, and every read clears this.
    /// An event has no identity to keep anyway - no name, no object
    /// that need still exist - and it is exactly the row whose full
    /// message someone wants to see.
    cluster_selected: Option<(cluster::Resource, usize)>,
    /// `ui.input(|i| i.time)` of the frame that selection changed, for
    /// the same double-pass reason as `selected_at`.
    cluster_selected_at: f64,
    /// The YAML of the selected object, once asked for.
    yaml: Option<Result<String, String>>,
    /// Where the YAML worker leaves its answer.
    yaml_pending: Option<YamlPending>,
    /// The YAML as the user is editing it, once they have started.
    ///
    /// Separate from `yaml`: Reset has to have something to go back to,
    /// and a re-read while an edit is open must not silently take the
    /// edit away.
    yaml_edit: Option<String>,
    /// What the dry run said the apply would do, and whether it has
    /// been carried out for real yet.
    apply: Option<Result<Vec<hs_container::k8s::Applied>, String>>,
    /// Where the apply worker leaves its answer.
    apply_pending: Option<ApplyPending>,
    /// A cluster verb waiting to be confirmed, against which object.
    confirm_cluster: Option<(cluster::Verb, cluster::Target)>,
    /// The replica count typed into the Scale field, as text: a
    /// half-typed number is not an integer, and clearing the box must
    /// not become a zero.
    scale_to: String,
    /// Where a cluster verb leaves what it did and the re-read after it.
    cluster_act_pending: Option<ClusterActPending>,
    /// The objects of the selected custom resource, once asked for.
    custom: Option<Result<Vec<hs_container::k8s::CustomRow>, String>>,
    /// Where the custom-objects worker leaves its answer.
    custom_pending: Option<CustomPending>,
    /// The port typed into the Port Forward field.
    forward_port: String,
    /// The forwards this tab is holding open, newest first.
    forwards: Vec<Forwarding>,
    /// Where the forward worker leaves its answer.
    forward_pending: Option<ForwardPending>,
    /// An assistant action waiting to be confirmed (§10.8).
    confirm_action: Option<action::Action>,
    /// Whether what the last read found has been posted to the Problems
    /// inbox. Cleared by every read, so a pane posts once per read
    /// rather than once per frame (§10.9).
    posted: bool,
    /// The review-only assistant (ADR-019, §10.8). It is told what is on
    /// screen - the host, the list, the selected object and its YAML -
    /// and nothing it writes reaches the cluster without the same
    /// buttons a person would press.
    assistant: hs_ui::ai_assistant::AssistantPanel,
}

/// One port forward this tab is holding open.
struct Forwarding {
    /// `<namespace>/<pod>:<port>`, for the row.
    target: String,
    url: String,
    handle: hs_container::k8s::Forward,
}

/// A slot the custom-objects worker fills.
type CustomPending =
    std::sync::Arc<parking_lot::Mutex<Option<Result<Vec<hs_container::k8s::CustomRow>, String>>>>;

/// A slot the port-forward worker fills: the forward, or why not, and
/// what it was opened against.
type ForwardPending = std::sync::Arc<
    parking_lot::Mutex<Option<(String, Result<hs_container::k8s::Forward, String>)>>,
>;

/// A slot the YAML worker fills.
type YamlPending = std::sync::Arc<parking_lot::Mutex<Option<Result<String, String>>>>;

/// A slot the apply worker fills.
type ApplyPending =
    std::sync::Arc<parking_lot::Mutex<Option<Result<Vec<hs_container::k8s::Applied>, String>>>>;

/// A slot a cluster write fills: what it did, and the cluster as it
/// stands afterwards.
///
/// The `bool` says whether the write was an **apply** rather than a
/// verb, because the two report differently - "applied" names a
/// manifest that reached the cluster, "done" names an object a verb was
/// run against.
type ClusterActPending = std::sync::Arc<
    parking_lot::Mutex<
        Option<(
            Result<(String, bool), String>,
            Result<cluster::Inventory, String>,
        )>,
    >,
>;

/// A slot the cluster worker fills. `kube` is async rather than
/// blocking, so this is spawned on the runtime rather than on a
/// blocking thread - but it reaches the UI the same way (ADR-013).
type ClusterPending =
    std::sync::Arc<parking_lot::Mutex<Option<Result<cluster::Inventory, String>>>>;

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
            cluster: None,
            cluster_pending: None,
            cluster_selected: None,
            cluster_selected_at: 0.0,
            yaml: None,
            yaml_pending: None,
            yaml_edit: None,
            apply: None,
            apply_pending: None,
            confirm_cluster: None,
            scale_to: String::new(),
            cluster_act_pending: None,
            custom: None,
            custom_pending: None,
            forward_port: String::new(),
            forwards: Vec::new(),
            forward_pending: None,
            confirm_action: None,
            posted: false,
            assistant: hs_ui::ai_assistant::AssistantPanel::default(),
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
        // A cluster is read by the Kubernetes driver, not by running
        // `docker` at it.
        if hs_core::connections::is_cluster_kind(&profile.kind) {
            self.reload_cluster(s, &profile);
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

    /// Read a cluster through `kube`, off the UI thread.
    ///
    /// On the runtime rather than a blocking thread: `kube` is async,
    /// and a blocking thread would need its own reactor to drive it.
    fn reload_cluster(&mut self, s: &Services, profile: &ConnectionProfile) {
        let fields = ContainerFields::from_table(&profile.fields);
        // The profile's namespace is the starting point - until the
        // reader picks one, which includes picking "All namespaces".
        if !self.state.namespace_set && !fields.namespace.is_empty() {
            self.state.namespace = fields.namespace.clone();
            self.state.namespace_set = true;
        }
        let namespace = self.state.namespace.clone();
        // The detail block points into the inventory being replaced.
        self.cluster_selected = None;
        self.clear_detail();
        self.loading = true;
        let waker = s.runtime.waker();
        let slot: ClusterPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.cluster_pending = Some(slot);
        s.runtime.spawn(async move {
            let result = cluster::read(&fields, &namespace).await;
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// Fetch the selected object's YAML, off the UI thread.
    fn reload_yaml(&mut self, s: &Services, profile: &ConnectionProfile, target: cluster::Target) {
        let fields = ContainerFields::from_table(&profile.fields);
        self.yaml = None;
        let waker = s.runtime.waker();
        let slot: YamlPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.yaml_pending = Some(slot);
        s.runtime.spawn(async move {
            let result = cluster::yaml(&fields, &target).await;
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// Forget everything the detail block was showing.
    ///
    /// One place, because there are now four things to forget and a
    /// path that forgets three of them leaves an apply preview standing
    /// over an object nobody selected.
    fn clear_detail(&mut self) {
        self.yaml = None;
        self.yaml_pending = None;
        self.yaml_edit = None;
        self.apply = None;
        self.apply_pending = None;
        self.scale_to.clear();
        self.custom = None;
        self.custom_pending = None;
        self.forward_port.clear();
        // The forwards are **not** closed here: a selection change is
        // not a reason to drop a connection somebody is using. They
        // live until stopped, or until the tab goes.
    }

    /// List the objects of the selected custom resource.
    fn reload_custom(
        &mut self,
        s: &Services,
        profile: &ConnectionProfile,
        crd: hs_container::k8s::CrdRow,
    ) {
        let fields = ContainerFields::from_table(&profile.fields);
        let namespace = self.state.namespace.clone();
        self.custom = None;
        let waker = s.runtime.waker();
        let slot: CustomPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.custom_pending = Some(slot);
        s.runtime.spawn(async move {
            let result = cluster::custom_objects(&fields, &crd, &namespace).await;
            *out.lock() = Some(result);
            waker.request_repaint();
        });
    }

    /// Open a port forward to the selected pod.
    fn start_forward(
        &mut self,
        s: &Services,
        profile: &ConnectionProfile,
        target: cluster::Target,
        port: u16,
    ) {
        let fields = ContainerFields::from_table(&profile.fields);
        let label = format!("{}/{}:{port}", target.namespace, target.name);
        let waker = s.runtime.waker();
        let slot: ForwardPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.forward_pending = Some(slot);
        s.runtime.spawn(async move {
            // Local port 0: the OS picks a free one, which is the only
            // answer that cannot collide with something already
            // listening on this machine.
            let result = cluster::forward(&fields, &target, port, 0).await;
            *out.lock() = Some((label, result));
            waker.request_repaint();
        });
    }

    /// Run a cluster verb and re-read the cluster, off the UI thread.
    fn act_cluster(
        &mut self,
        s: &Services,
        profile: &ConnectionProfile,
        target: cluster::Target,
        verb: cluster::Verb,
    ) {
        let fields = ContainerFields::from_table(&profile.fields);
        let namespace = self.state.namespace.clone();
        self.loading = true;
        let waker = s.runtime.waker();
        let slot: ClusterActPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.cluster_act_pending = Some(slot);
        s.runtime.spawn(async move {
            // The verb and the re-read in one task, so what comes back
            // is the cluster *after* the verb rather than before it.
            let did = cluster::act(&fields, &target, verb)
                .await
                .map(|what| (what, false));
            let inv = cluster::read(&fields, &namespace).await;
            *out.lock() = Some((did, inv));
            waker.request_repaint();
        });
    }

    /// Dry-run or carry out an apply, off the UI thread.
    ///
    /// A dry run only fills the preview. A **real** apply goes through
    /// the same path as every other write in this pane: it re-reads the
    /// cluster on the same task and reports in the Output block, or the
    /// object just created would be missing from the list underneath it
    /// (seen on the bench, 2026-10-24).
    fn run_apply(&mut self, s: &Services, profile: &ConnectionProfile, yaml: String, dry: bool) {
        let fields = ContainerFields::from_table(&profile.fields);
        let namespace = self.state.namespace.clone();
        let waker = s.runtime.waker();
        if dry {
            self.apply = None;
            let slot: ApplyPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
            let out = slot.clone();
            self.apply_pending = Some(slot);
            s.runtime.spawn(async move {
                let result = cluster::apply(&fields, &yaml, &namespace, true).await;
                *out.lock() = Some(result);
                waker.request_repaint();
            });
            return;
        }
        self.loading = true;
        let slot: ClusterActPending = std::sync::Arc::new(parking_lot::Mutex::new(None));
        let out = slot.clone();
        self.cluster_act_pending = Some(slot);
        s.runtime.spawn(async move {
            let did = cluster::apply(&fields, &yaml, &namespace, false)
                .await
                .map(|applied| {
                    let what = applied
                        .iter()
                        .map(|a| a.what.clone())
                        .collect::<Vec<_>>()
                        .join(", ");
                    (what, true)
                });
            let inv = cluster::read(&fields, &namespace).await;
            *out.lock() = Some((did, inv));
            waker.request_repaint();
        });
    }

    /// Take the answer when the worker has it.
    fn drain(&mut self, s: &Services) {
        // The guard is taken in its own scope: a `let` chain holds the
        // borrow of `self.pending` across the body, and the body has to
        // clear it.
        let read = self.pending.as_ref().and_then(|s| s.lock().take());
        if let Some(result) = read {
            self.inventory = Some(result);
            self.loading = false;
            self.pending = None;
            self.posted = false;
        }
        if let Some(result) = self.yaml_pending.as_ref().and_then(|s| s.lock().take()) {
            self.yaml = Some(result);
            self.yaml_pending = None;
        }
        if let Some(result) = self.apply_pending.as_ref().and_then(|s| s.lock().take()) {
            self.apply = Some(result);
            self.apply_pending = None;
        }
        if let Some(result) = self.custom_pending.as_ref().and_then(|s| s.lock().take()) {
            self.custom = Some(result);
            self.custom_pending = None;
        }
        if let Some((target, result)) = self.forward_pending.as_ref().and_then(|s| s.lock().take())
        {
            match result {
                Ok(handle) => {
                    let url = format!("http://127.0.0.1:{}", handle.local_port);
                    self.last_output = Some(
                        s.i18n
                            .t_args("cluster-forward-on", &[("target", &target), ("url", &url)]),
                    );
                    self.forwards.insert(
                        0,
                        Forwarding {
                            target,
                            url,
                            handle,
                        },
                    );
                }
                Err(e) => self.last_output = Some(e),
            }
            self.forward_pending = None;
        }
        let acted = self
            .cluster_act_pending
            .as_ref()
            .and_then(|s| s.lock().take());
        if let Some((did, inv)) = acted {
            self.last_output = Some(match did {
                Ok((what, applied)) => {
                    let key = if applied {
                        "cluster-applied"
                    } else {
                        "cluster-verb-done"
                    };
                    s.i18n.t_args(key, &[("what", &what)])
                }
                Err(e) => e,
            });
            self.cluster = Some(inv);
            // The row the detail was showing may be gone, and the rest
            // have moved up: the only honest thing is to start again.
            self.cluster_selected = None;
            self.clear_detail();
            self.loading = false;
            self.cluster_act_pending = None;
        }
        let cluster = self.cluster_pending.as_ref().and_then(|s| s.lock().take());
        if let Some(result) = cluster {
            self.cluster = Some(result);
            self.loading = false;
            self.cluster_pending = None;
            self.posted = false;
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
        self.drain(&s);
        let Some(profile) = self.host(&s) else {
            form::hint(ui, &theme, &s.t("container-host-gone"));
            return;
        };
        let is_cluster = hs_core::connections::is_cluster_kind(&profile.kind);
        // The first read happens when the tab is first drawn, not when it
        // is created: a workspace that restores ten tabs must not probe
        // ten engines at start-up.
        let unread = if is_cluster {
            self.cluster.is_none()
        } else {
            self.inventory.is_none()
        };
        if unread && !self.loading {
            self.reload(&s);
        }
        // Once per read, not once per frame: what the read found goes
        // to the Problems inbox so a pod in CrashLoopBackOff is visible
        // in the dock without anybody watching this pane (§10.9).
        if !self.posted && !self.loading {
            self.posted = true;
            self.post_problems(&s, &profile);
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
        if is_cluster {
            self.cluster_pane(ui, &theme, &s, cx.actions, &profile);
            return;
        }
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

    /// The Lens-shaped cluster pane (§10.6): the namespace above a
    /// resource rail, the chosen list beside it.
    ///
    /// The namespace sits **above** the rail rather than beside the
    /// table, because it narrows every list below it: putting it in the
    /// table's own toolbar would read as filtering that one list.
    #[allow(clippy::too_many_arguments)]
    fn cluster_pane(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
    ) {
        let inv = match &self.cluster {
            None => {
                form::hint(ui, theme, &s.t("container-reading"));
                return;
            }
            Some(Err(e)) => {
                // The driver's own words: it tells a kubeconfig that
                // cannot be read from a cluster that refused the
                // credentials, and the remedies differ.
                form::note(ui, theme, hs_ui::icons::WARNING, e);
                return;
            }
            Some(Ok(inv)) => inv.clone(),
        };

        ui.horizontal(|ui| {
            hs_ui::dashboard::stat(
                ui,
                theme,
                &inv.nodes.len().to_string(),
                &s.t("cluster-dash-nodes"),
            );
            hs_ui::dashboard::stat(
                ui,
                theme,
                &inv.pods.len().to_string(),
                &s.t("cluster-dash-pods"),
            );
            hs_ui::dashboard::stat(
                ui,
                theme,
                &inv.workloads.len().to_string(),
                &s.t("cluster-dash-workloads"),
            );
            // The number that decides whether this pane was worth
            // opening, so it is a stat rather than a colour in a table
            // somebody has to scroll.
            hs_ui::dashboard::stat(
                ui,
                theme,
                &inv.trouble().to_string(),
                &s.t("cluster-dash-trouble"),
            );
            hs_ui::dashboard::stat(
                ui,
                theme,
                &inv.warnings().to_string(),
                &s.t("cluster-dash-warnings"),
            );
        });
        form::hint(
            ui,
            theme,
            &s.i18n
                .t_args("cluster-version", &[("version", &inv.version)]),
        );
        ui.add_space(6.0);

        // The namespace picker, above everything it narrows.
        let namespaced = self.state.cluster_resource.namespaced();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(s.t("cluster-namespace"))
                    .small()
                    .color(theme.text_muted),
            );
            let current = if self.state.namespace.is_empty() {
                s.t("cluster-all-namespaces")
            } else {
                self.state.namespace.clone()
            };
            let before = self.state.namespace.clone();
            ui.add_enabled_ui(namespaced, |ui| {
                egui::ComboBox::from_id_salt("cluster-namespace")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.state.namespace,
                            String::new(),
                            s.t("cluster-all-namespaces"),
                        );
                        for n in &inv.namespaces {
                            ui.selectable_value(&mut self.state.namespace, n.clone(), n);
                        }
                    });
            });
            if !namespaced {
                ui.label(
                    egui::RichText::new(s.t("cluster-not-namespaced"))
                        .small()
                        .color(theme.text_muted),
                );
            }
            if before != self.state.namespace {
                // Narrowing changes every list, so it is a re-read
                // rather than a filter over what is already here.
                self.state.namespace_set = true;
                self.cluster = None;
                self.reload_cluster(s, profile);
            }
        });
        ui.add_space(4.0);

        self.output_block(ui, theme, s);
        self.confirm_cluster_dialog(ui, theme, s, profile);
        self.confirm_action_dialog(ui, theme, s, profile);
        let counts = inv.counts();
        // The detail block takes its share off the top of the list, so
        // the two scroll independently.
        // A YAML document is why the block was opened at all, so it
        // takes the larger share once it is there; the facts above it
        // need a fraction of that.
        let detail_h = match (&self.cluster_selected, &self.yaml) {
            (None, _) => 0.0,
            (Some(_), Some(Ok(_))) => (ui.available_height() * 0.55).clamp(260.0, 520.0),
            (Some(_), _) => 260.0,
        };
        ui.horizontal_top(|ui| {
            if let Some(r) =
                cluster::resource_rail(ui, theme, s, self.state.cluster_resource, &counts)
            {
                self.state.cluster_resource = r;
                self.table = hs_ui::table::TableState::default();
                // The detail belonged to the list that is going away.
                self.cluster_selected = None;
                self.yaml = None;
            }
            ui.separator();
            ui.vertical(|ui| {
                let full = ui.available_height();
                egui::ScrollArea::vertical()
                    .id_salt("cluster-list")
                    .max_height((full - detail_h).max(120.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.cluster_table(ui, theme, s, &inv);
                    });
                if self.cluster_selected.is_some() {
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("cluster-detail")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            self.cluster_detail(ui, theme, s, cx_actions, profile, &inv);
                        });
                }
            });
        });
    }

    /// The chosen cluster resource, in the shared table.
    fn cluster_table(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        inv: &cluster::Inventory,
    ) {
        let res = self.state.cluster_resource;
        let (columns, rows) = cluster::table_data(res, s, inv);
        if rows.is_empty() {
            form::hint(ui, theme, &s.t(res.empty_key()));
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
        if let Some(i) = resp.clicked.or(resp.double_clicked) {
            let changed = self.cluster_selected != Some((res, i));
            self.cluster_selected = Some((res, i));
            self.cluster_selected_at = ui.input(|i| i.time);
            if changed {
                // What was fetched belongs to what it was fetched for.
                self.yaml = None;
                self.yaml_pending = None;
                // The forward field opens on the pod's first declared
                // port - **here**, once, rather than whenever the field
                // happens to be empty: re-filling an empty field every
                // frame means it cannot be cleared, which is the same
                // trap the namespace picker fell into.
                self.forward_port = cluster::pod_ports(inv, i)
                    .first()
                    .map(u16::to_string)
                    .unwrap_or_default();
            }
        }
    }

    /// The detail block for a cluster row: what it is, and its YAML on
    /// request.
    ///
    /// On request rather than with the list: one object's YAML is
    /// another round trip, and fetching every row's would make scrolling
    /// a list of pods a conversation with the API server.
    #[allow(clippy::too_many_arguments)]
    fn cluster_detail(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        inv: &cluster::Inventory,
    ) {
        let Some((res, i)) = self.cluster_selected else {
            return;
        };
        if res != self.state.cluster_resource {
            return;
        }
        let title = cluster::row_title(res, inv, i);
        if title.is_empty() {
            // The row went away with the last read.
            self.cluster_selected = None;
            return;
        }
        ui.add_space(4.0);
        // A `hard_right` item anchors to the Ui's own right edge, which
        // here is under the scroll bar; narrowing the row is what moves
        // it clear, not a space before it.
        let row_w = (ui.available_width() - 14.0).max(80.0);
        ui.scope(|ui| {
            ui.set_max_width(row_w);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(&title).strong());
                hs_ui::toolbar::hard_right(ui, |ui| {
                    if ui.button(s.t("cluster-detail-close")).clicked() {
                        self.cluster_selected = None;
                        self.clear_detail();
                    }
                });
            });
        });
        ui.add_space(2.0);
        egui::Grid::new(("cluster-detail-grid", res as u8, i))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (label, value) in cluster::summary(res, s, inv, i) {
                    ui.label(egui::RichText::new(label).small().color(theme.text_muted));
                    ui.add(egui::Label::new(value).selectable(true).truncate());
                    ui.end_row();
                }
            });

        let Some(target) = cluster::row_target(res, inv, i) else {
            // An event has nothing to fetch; its row above is all of it.
            return;
        };
        ui.add_space(6.0);
        // The same guard the Docker detail needs: this block did not
        // exist during the frame's first pass, so its buttons are laid
        // out under a pointer egui still reports as clicking.
        let fresh = ui.input(|i| i.time) <= self.cluster_selected_at;
        let refusal = docker::guard(s, profile);
        ui.horizontal_wrapped(|ui| {
            let asked = self.yaml.is_some() || self.yaml_pending.is_some();
            if ui
                .add_enabled(!fresh && !asked, egui::Button::new(s.t("cluster-yaml")))
                .clicked()
            {
                self.reload_yaml(s, profile, target.clone());
            }
            if asked
                && ui
                    .add_enabled(!fresh, egui::Button::new(s.t("cluster-yaml-hide")))
                    .clicked()
            {
                self.yaml = None;
                self.yaml_edit = None;
                self.yaml_pending = None;
                self.apply = None;
            }
            if let Some(Ok(yaml)) = &self.yaml
                && ui
                    .add_enabled(!fresh, egui::Button::new(s.t("cluster-yaml-copy")))
                    .clicked()
            {
                ui.ctx().copy_text(yaml.clone());
            }
            if res == cluster::Resource::Pods {
                // One button per container when there are several: a
                // sidecar's log is a different log, and picking the
                // first silently would hide that there is a choice.
                let containers = inv
                    .pods
                    .get(i)
                    .map(|p| p.containers.clone())
                    .unwrap_or_default();
                if containers.len() < 2 {
                    if ui
                        .add_enabled(!fresh, egui::Button::new(s.t("cluster-logs")))
                        .clicked()
                    {
                        self.open_pod_logs(s, cx_actions, profile, inv, i, "");
                    }
                } else {
                    for c in &containers {
                        let label = s.i18n.t_args("cluster-logs-of", &[("container", c)]);
                        if ui.add_enabled(!fresh, egui::Button::new(label)).clicked() {
                            self.open_pod_logs(s, cx_actions, profile, inv, i, c);
                        }
                    }
                }
                // Exec goes to the Terminal activity, as it does for a
                // container (§10.1). There is no command line to type
                // here - the API server's `exec` subresource is the
                // shell - so the address says which pod and the
                // Terminal tool builds the transport.
                // Guarded like a verb, unlike the Docker pane's Exec:
                // a shell inside a pod is unbounded write access, and
                // "protected environments default to read-only in
                // every tool" is the rule it has to answer to. The two
                // panes differing is noted in the spec; aligning the
                // Docker side belongs with its own change.
                let shell_blocked = refusal.is_some();
                let exec = ui.add_enabled(
                    !fresh && !shell_blocked,
                    egui::Button::new(s.t("cluster-exec")),
                );
                let exec = match &refusal {
                    Some(why) => exec.on_disabled_hover_text(docker::refusal_text(s, why)),
                    None => exec,
                };
                if exec.on_hover_text(s.t("cluster-exec-hint")).clicked() {
                    // Name the container only where there is a
                    // choice: with one, the API does not need it.
                    let container = match containers.len() {
                        0 | 1 => String::new(),
                        _ => containers.first().cloned().unwrap_or_default(),
                    };
                    cx_actions.push(Action::OpenTool {
                        factory_id: "terminal".into(),
                        request: Some(OpenRequest::Terminal {
                            cwd: None,
                            host: Some(format!(
                                "k8s://{}/{}/{}/{}",
                                profile.id, target.namespace, target.name, container
                            )),
                            profile: None,
                            command: None,
                        }),
                    });
                }
            }
            if res == cluster::Resource::Custom
                && ui
                    .add_enabled(!fresh, egui::Button::new(s.t("cluster-custom-list")))
                    .clicked()
                && let Some(crd) = inv.crds.get(i).cloned()
            {
                self.reload_custom(s, profile, crd);
            }
            if res == cluster::Resource::Pods {
                self.forward_controls(ui, s, profile, &target, fresh);
            }
            self.cluster_verbs(ui, s, profile, &target, fresh, refusal.as_ref());
        });
        self.custom_block(ui, theme, s, inv, i);
        self.forward_block(ui, theme, s, cx_actions);
        self.yaml_block(ui, theme, s, profile, &target, fresh);
    }

    /// The Port Forward field and button, for a pod.
    ///
    /// The field is filled in when the pod is selected, not here: see
    /// `cluster_table`.
    fn forward_controls(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        profile: &ConnectionProfile,
        target: &cluster::Target,
        fresh: bool,
    ) {
        ui.add(
            egui::TextEdit::singleline(&mut self.forward_port)
                .desired_width(72.0)
                .hint_text(s.t("cluster-forward-port")),
        );
        let port: Option<u16> = self.forward_port.trim().parse().ok().filter(|p| *p > 0);
        let starting = self.forward_pending.is_some();
        if ui
            .add_enabled(
                !fresh && port.is_some() && !starting,
                egui::Button::new(s.t("cluster-forward")),
            )
            .on_hover_text(s.t("cluster-forward-hint"))
            .clicked()
            && let Some(port) = port
        {
            self.start_forward(s, profile, target.clone(), port);
        }
    }

    /// The forwards this tab is holding open.
    fn forward_block(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        cx_actions: &mut Vec<Action>,
    ) {
        // A forward whose task has ended is not a forward any more.
        self.forwards.retain(|f| f.handle.running());
        if self.forwards.is_empty() {
            return;
        }
        ui.add_space(4.0);
        let mut stop: Option<usize> = None;
        for (i, f) in self.forwards.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&f.target)
                        .small()
                        .color(theme.text_muted),
                );
                if ui
                    .small_button(egui::RichText::new(&f.url).color(theme.accent))
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    open_url(cx_actions, &f.url);
                }
                if ui.small_button(s.t("cluster-forward-stop")).clicked() {
                    stop = Some(i);
                }
            });
        }
        if let Some(i) = stop {
            // Stop first, then forget: the task reads the flag.
            self.forwards[i].handle.stop();
            self.forwards.remove(i);
        }
    }

    /// The objects of the selected custom resource, once asked for.
    fn custom_block(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        inv: &cluster::Inventory,
        i: usize,
    ) {
        if self.state.cluster_resource != cluster::Resource::Custom {
            return;
        }
        match &self.custom {
            None if self.custom_pending.is_some() => {
                form::hint(ui, theme, &s.t("container-reading"));
            }
            None => {}
            Some(Err(e)) => form::note(ui, theme, hs_ui::icons::WARNING, e),
            Some(Ok(rows)) if rows.is_empty() => {
                form::hint(ui, theme, &s.t("cluster-no-objects"));
            }
            Some(Ok(rows)) => {
                ui.add_space(4.0);
                let kind = inv.crds.get(i).map(|c| c.kind.clone()).unwrap_or_default();
                ui.label(
                    egui::RichText::new(
                        s.i18n.t_args("cluster-custom-objects", &[("kind", &kind)]),
                    )
                    .small()
                    .color(theme.text_muted),
                );
                for r in rows {
                    let line = if r.namespace.is_empty() {
                        format!("{}  ·  {}", r.name, r.age)
                    } else {
                        format!("{}/{}  ·  {}", r.namespace, r.name, r.age)
                    };
                    ui.add(egui::Label::new(line).selectable(true).truncate());
                }
            }
        }
    }

    /// The verbs a cluster object offers, and the Scale field.
    ///
    /// A refused verb is **disabled and says why**, never hidden - the
    /// same rule the Docker pane follows next door, because a hidden
    /// button reads as a tool that cannot do the thing rather than a
    /// host that will not allow it.
    fn cluster_verbs(
        &mut self,
        ui: &mut egui::Ui,
        s: &Services,
        profile: &ConnectionProfile,
        target: &cluster::Target,
        fresh: bool,
        refusal: Option<&docker::Refusal>,
    ) {
        let blocked = refusal.is_some();
        let explain = |r: egui::Response| match refusal {
            Some(why) if blocked => r.on_disabled_hover_text(docker::refusal_text(s, why)),
            _ => r,
        };
        // A rollout restart is the one verb here that takes nothing
        // away: it replaces pods under the workload's own surge and
        // availability rules. It is still a write, so a read-only host
        // refuses it.
        if cluster::Verb::Restart.applies_to(&target.kind) {
            let r = explain(ui.add_enabled(
                !fresh && !blocked,
                egui::Button::new(s.t("cluster-verb-restart")),
            ));
            if r.on_hover_text(s.t("cluster-verb-restart-hint")).clicked() {
                self.act_cluster(s, profile, target.clone(), cluster::Verb::Restart);
            }
        }
        if cluster::Verb::Scale(0).applies_to(&target.kind) {
            ui.add_enabled(
                !blocked,
                egui::TextEdit::singleline(&mut self.scale_to)
                    // Wide enough for the word, not for a number: the
                    // hint is what tells a reader what the box is for,
                    // and at 48 px it clipped to "Rep…".
                    .desired_width(72.0)
                    .hint_text(s.t("cluster-replicas")),
            );
            let wanted: Option<i32> = self.scale_to.trim().parse().ok();
            let r = explain(ui.add_enabled(
                !fresh && !blocked && wanted.is_some(),
                egui::Button::new(s.t("cluster-verb-scale")),
            ));
            if r.on_hover_text(s.t("cluster-verb-scale-hint")).clicked()
                && let Some(n) = wanted
            {
                self.confirm_cluster = Some((cluster::Verb::Scale(n), target.clone()));
            }
        }
        let r = explain(ui.add_enabled(
            !fresh && !blocked,
            egui::Button::new(s.t("cluster-verb-delete")),
        ));
        if r.clicked() {
            self.confirm_cluster = Some((cluster::Verb::Delete, target.clone()));
        }
    }

    /// The YAML, editable, and what applying it would do.
    fn yaml_block(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        profile: &ConnectionProfile,
        target: &cluster::Target,
        fresh: bool,
    ) {
        match &self.yaml {
            None if self.yaml_pending.is_some() => {
                form::hint(ui, theme, &s.t("container-reading"));
                return;
            }
            None => return,
            Some(Err(e)) => {
                form::note(ui, theme, hs_ui::icons::WARNING, e);
                return;
            }
            Some(Ok(_)) => {}
        }
        let editable = docker::guard(s, profile).is_none();
        ui.add_space(4.0);
        if editable {
            // Edited in place rather than handed to the editor: the
            // round trip through a file would lose which cluster and
            // namespace the object came from, which is exactly what
            // Apply needs. The editor keeps the manifest workflow
            // (§10.7 item 6); this is the one-object case.
            let text = self.yaml_edit.get_or_insert_with(|| match &self.yaml {
                Some(Ok(y)) => y.clone(),
                _ => String::new(),
            });
            let before = text.clone();
            ui.add(
                egui::TextEdit::multiline(text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(12),
            );
            if *text != before {
                // The preview belongs to the text it was computed from.
                self.apply = None;
            }
            self.apply_row(ui, theme, s, profile, target, fresh);
        } else {
            let Some(Ok(yaml)) = &self.yaml else { return };
            ui.add(
                egui::Label::new(egui::RichText::new(yaml).monospace().size(12.0)).selectable(true),
            );
        }
    }

    /// Dry Run, Apply and what the server said they would do.
    fn apply_row(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        profile: &ConnectionProfile,
        target: &cluster::Target,
        fresh: bool,
    ) {
        let edited = match (&self.yaml, &self.yaml_edit) {
            (Some(Ok(original)), Some(text)) => original != text,
            _ => false,
        };
        let running = self.apply_pending.is_some();
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !fresh && edited && !running,
                    egui::Button::new(s.t("cluster-dry-run")),
                )
                .on_hover_text(s.t("cluster-dry-run-hint"))
                .clicked()
                && let Some(text) = self.yaml_edit.clone()
            {
                self.run_apply(s, profile, text, true);
            }
            // Apply is offered only **after** a dry run that came back
            // clean, which is the spec's rule (§5) and the reason the
            // preview exists: nothing reaches the cluster that the
            // server has not already described.
            let previewed = matches!(&self.apply, Some(Ok(a)) if a.iter().all(|x| x.dry_run));
            if ui
                .add_enabled(
                    !fresh && previewed && !running,
                    egui::Button::new(s.t("cluster-apply")),
                )
                .clicked()
                && let Some(text) = self.yaml_edit.clone()
            {
                self.run_apply(s, profile, text, false);
            }
            if ui
                .add_enabled(
                    !fresh && edited && !running,
                    egui::Button::new(s.t("cluster-reset")),
                )
                .clicked()
            {
                self.yaml_edit = None;
                self.apply = None;
            }
            if running {
                ui.label(
                    egui::RichText::new(s.t("container-reading"))
                        .small()
                        .color(theme.text_muted),
                );
            }
        });
        let _ = target;
        self.apply_preview(ui, theme, s);
    }

    /// What the dry run said, as a diff; or what the apply did.
    fn apply_preview(&mut self, ui: &mut egui::Ui, theme: &Theme, s: &Services) {
        let Some(result) = &self.apply else { return };
        ui.add_space(4.0);
        let applied = match result {
            Err(e) => {
                form::note(ui, theme, hs_ui::icons::WARNING, e);
                return;
            }
            Ok(a) => a,
        };
        for a in applied {
            let headline = if !a.dry_run {
                s.i18n.t_args("cluster-applied", &[("what", &a.what)])
            } else if a.creates() {
                s.i18n.t_args("cluster-would-create", &[("what", &a.what)])
            } else if a.changes() {
                s.i18n.t_args("cluster-would-change", &[("what", &a.what)])
            } else {
                s.i18n.t_args("cluster-no-change", &[("what", &a.what)])
            };
            ui.label(egui::RichText::new(headline).strong());
            if !a.dry_run || !a.changes() {
                continue;
            }
            for (sign, line) in cluster::diff(&a.before, &a.after) {
                let colour = match sign {
                    '+' => theme.success,
                    '-' => theme.error,
                    _ => theme.text_muted,
                };
                ui.label(
                    egui::RichText::new(format!("{sign} {line}"))
                        .monospace()
                        .size(12.0)
                        .color(colour),
                );
            }
        }
    }

    /// Follow one pod's log in a Logs tab.
    #[allow(clippy::too_many_arguments)]
    fn open_pod_logs(
        &self,
        s: &Services,
        cx_actions: &mut Vec<Action>,
        profile: &ConnectionProfile,
        inv: &cluster::Inventory,
        i: usize,
        container: &str,
    ) {
        let Some(pod) = inv.pods.get(i) else { return };
        let tail = s.settings.read().containers.log_tail_lines;
        // Empty for a single-container pod: naming it would be noise
        // the API does not need, and the caller passes the name only
        // where there was a choice to make.
        let container = container.to_string();
        let source = serde_json::json!({
            "schema": 1,
            "name": if container.is_empty() {
                format!("{} · {}", profile.name, pod.name)
            } else {
                format!("{} · {}/{container}", profile.name, pod.name)
            },
            "spec": {
                "kind": "k8s_pod",
                "conn_id": profile.id,
                "namespace": pod.namespace,
                "pod": pod.name,
                "container": container,
            },
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

    /// What the assistant is told, and the column it is drawn in
    /// (§10.8).
    ///
    /// Only what is on screen: the host, which list is open, the
    /// selected object and - when it has been fetched - its YAML. A
    /// pane that quietly read more than the user can see would be
    /// answering about things they cannot check.
    fn assistant_column(
        &mut self,
        outer: &mut egui::Ui,
        panel: Option<egui::Rect>,
        cx: &mut ToolContext,
    ) {
        if !self.assistant.open {
            return;
        }
        let s = cx.services.clone();
        let theme = Theme::from_settings(&s.settings.read().appearance);
        let profile = self.host(&s);
        self.assistant
            .set_context(s.t("container-assistant-context"), self.assistant_text(&s));
        if self.assistant.quick_actions.is_empty() {
            self.assistant.quick_actions =
                hs_ui::ai_assistant::QuickAction::for_activity(&s, "container");
        }
        // The action language, so an `hs-container` block is offered
        // **Apply** rather than Insert (the API client's arrangement).
        self.assistant.action_lang = Some(action::LANG.to_string());
        self.assistant.extra_system = action::system_prompt(&s, profile.as_ref());
        // Insert puts a manifest in the YAML pane, where Dry Run and
        // Apply already live: nothing reaches a cluster without the
        // same two presses a hand-written manifest needs.
        self.assistant.can_insert = true;
        // And nothing at all may be applied against a host that is
        // read-only or protected: the button is absent rather than
        // present and refusing (ADR-019).
        let allowed = profile
            .as_ref()
            .is_some_and(|p| docker::guard(&s, p).is_none());
        self.assistant.can_run = false;
        let Some(panel) = panel else {
            // Too narrow to split: the toggle stays on and the column
            // comes back when there is room for it.
            return;
        };
        let mut child = outer.new_child(
            egui::UiBuilder::new()
                .max_rect(panel)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(panel.intersect(outer.clip_rect()));
        let mut applied: Option<(String, String)> = None;
        egui::Frame::new()
            .fill(theme.panel_bg)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .show(&mut child, |ui| {
                ui.set_min_width(370.0);
                let out = self.assistant.ui(ui, &s, &theme, "Containers");
                if let Some(code) = out.use_block {
                    applied = Some((out.use_block_lang, code));
                }
            });
        if let Some((lang, code)) = applied {
            self.apply_block(&s, &lang, &code, allowed);
        }
    }

    /// Post what is unhealthy to the Problems inbox (§10.9).
    ///
    /// Called after a read, never per frame: a pane that posted on
    /// every repaint would fill the inbox with the same row sixty
    /// times a second, and the cooldown would be doing the work the
    /// caller should.
    ///
    /// **What counts as a problem is the same judgement the pane's own
    /// "needs attention" makes**, so a row in the inbox and a number in
    /// the header never disagree. The shape is the object rather than
    /// the message: a pod that flaps between `CrashLoopBackOff` and
    /// `Error` is one row that climbs, not two that interleave.
    fn post_problems(&self, s: &Services, profile: &ConnectionProfile) {
        let uri = format!("container://{}", profile.id);
        let post = |severity: hs_core::problems::Severity, message: String, shape: String| {
            s.problems.post(hs_core::problems::Report {
                severity,
                message: message.chars().take(300).collect(),
                shape,
                origin: hs_core::problems::Origin {
                    tool: "containers".into(),
                    uri: uri.clone(),
                    label: profile.name.clone(),
                    line: None,
                },
            });
        };
        if let Some(Ok(inv)) = &self.cluster {
            for p in inv.pods.iter().filter(|p| p.trouble) {
                post(
                    hs_core::problems::Severity::Error,
                    s.i18n.t_args(
                        "container-problem-pod",
                        &[
                            ("pod", &format!("{}/{}", p.namespace, p.name)),
                            ("status", &p.status),
                        ],
                    ),
                    format!("pod:{}/{}", p.namespace, p.name),
                );
            }
            for w in inv.workloads.iter().filter(|w| w.trouble) {
                post(
                    hs_core::problems::Severity::Warning,
                    s.i18n.t_args(
                        "container-problem-workload",
                        &[
                            ("what", &format!("{}/{}", w.kind, w.name)),
                            ("ready", &w.ready),
                        ],
                    ),
                    format!("workload:{}/{}", w.namespace, w.name),
                );
            }
            for n in inv.nodes.iter().filter(|n| !n.ready) {
                post(
                    hs_core::problems::Severity::Error,
                    s.i18n.t_args(
                        "container-problem-node",
                        &[("node", &n.name), ("status", &n.status)],
                    ),
                    format!("node:{}", n.name),
                );
            }
            return;
        }
        if let Some(Ok(inv)) = &self.inventory {
            for c in inv.containers.iter().filter(|c| c.state.is_trouble()) {
                post(
                    hs_core::problems::Severity::Error,
                    s.i18n.t_args(
                        "container-problem-container",
                        &[("name", &c.name), ("status", &c.status)],
                    ),
                    format!("container:{}", c.name),
                );
            }
            // An unhealthy container is still running, so it is not in
            // the list above - and it is exactly the case a pane is
            // opened for.
            for c in inv
                .containers
                .iter()
                .filter(|c| c.health == hs_container::Health::Unhealthy)
            {
                post(
                    hs_core::problems::Severity::Warning,
                    s.i18n
                        .t_args("container-problem-unhealthy", &[("name", &c.name)]),
                    format!("health:{}", c.name),
                );
            }
        }
    }

    /// Carry out what the user pressed on one of the assistant's code
    /// blocks (§10.8).
    ///
    /// Two languages do something here, and both end up where a
    /// hand-written one would:
    ///
    /// - **`yaml`** goes into the YAML pane, which is where Dry Run and
    ///   Apply already are. Nothing reaches the cluster that the server
    ///   has not described first, exactly as for a manifest the user
    ///   typed themselves.
    /// - **`hs-container`** is an action: it is read, and then **asked
    ///   about** before it runs, through the same confirmation a click
    ///   on the verb would raise.
    ///
    /// Anything else is copied by the panel and ignored here.
    fn apply_block(&mut self, s: &Services, lang: &str, code: &str, allowed: bool) {
        if lang.eq_ignore_ascii_case(action::LANG) {
            if !allowed {
                // Belt as well as braces: the block is not offered on a
                // protected host, and would be refused if it were.
                self.last_output = Some(s.t("container-action-not-allowed"));
                return;
            }
            match action::parse(code, s) {
                Ok(what) => self.confirm_action = Some(what),
                Err(why) => self.last_output = Some(why),
            }
            return;
        }
        if lang.eq_ignore_ascii_case("yaml") || lang.eq_ignore_ascii_case("yml") {
            // The pane shows the editor only once a YAML has been
            // fetched; a manifest the assistant wrote is a document in
            // its own right, so it opens the editor on itself.
            self.yaml = Some(Ok(code.to_string()));
            self.yaml_edit = Some(code.to_string());
            self.apply = None;
            self.last_output = Some(s.t("container-action-yaml-inserted"));
        }
    }

    /// The question an assistant action asks before it runs, and the
    /// run itself.
    fn confirm_action_dialog(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        profile: &ConnectionProfile,
    ) {
        let Some(what) = self.confirm_action.clone() else {
            return;
        };
        match hs_ui::dialog::confirm(
            ui.ctx(),
            theme,
            "container-action",
            &s.t("container-action-title"),
            &what.question(s),
            &s.t("container-action-run"),
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                self.confirm_action = None;
                self.run_action(s, profile, what);
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm_action = None,
            hs_ui::dialog::DialogResult::Open => {}
        }
    }

    /// Carry out a confirmed action through the ordinary paths.
    fn run_action(&mut self, s: &Services, profile: &ConnectionProfile, what: action::Action) {
        // The namespace the block named, or the one the tab is on: an
        // answer about the open list need not repeat it.
        let namespace = if what.namespace().is_empty() {
            self.state.namespace.clone()
        } else {
            what.namespace().to_string()
        };
        match what {
            action::Action::Scale {
                kind,
                name,
                replicas,
                ..
            } => {
                let target = cluster::Target {
                    kind,
                    namespace,
                    name,
                };
                self.act_cluster(s, profile, target, cluster::Verb::Scale(replicas));
            }
            action::Action::Restart { kind, name, .. } => {
                let target = cluster::Target {
                    kind,
                    namespace,
                    name,
                };
                self.act_cluster(s, profile, target, cluster::Verb::Restart);
            }
            action::Action::SetImage {
                kind,
                name,
                container,
                image,
                ..
            } => {
                // An image change is an apply, not a verb: it is a
                // change to the pod template, and going through apply
                // means the server describes it first like any other.
                let manifest =
                    action::set_image_patch(&kind, &namespace, &name, &container, &image);
                self.run_apply(s, profile, manifest, true);
                self.last_output = Some(s.t("container-action-image-dry-run"));
            }
        }
    }

    /// Everything the assistant is told about this tab.
    fn assistant_text(&self, s: &Services) -> String {
        let Some(profile) = self.host(s) else {
            return s.t("container-assistant-no-host");
        };
        let kind = hs_core::connections::kind_label(&profile.kind);
        let mut out = format!("Host: {} ({kind})\n", profile.name);
        if hs_core::connections::is_cluster_kind(&profile.kind) {
            let namespace = if self.state.namespace.is_empty() {
                "all namespaces".to_string()
            } else {
                self.state.namespace.clone()
            };
            out.push_str(&format!("Namespace: {namespace}\n"));
            match &self.cluster {
                Some(Ok(inv)) => {
                    out.push_str(&format!(
                        "Kubernetes {}\nNodes: {}, pods: {}, workloads: {}, needing attention: {}\n",
                        inv.version,
                        inv.nodes.len(),
                        inv.pods.len(),
                        inv.workloads.len(),
                        inv.trouble(),
                    ));
                    out.push_str(&format!(
                        "Open list: {}\n",
                        s.t(self.state.cluster_resource.key())
                    ));
                    if let Some((res, i)) = self.cluster_selected {
                        out.push_str(&format!(
                            "\nSelected: {}\n",
                            cluster::row_title(res, inv, i)
                        ));
                        for (label, value) in cluster::summary(res, s, inv, i) {
                            out.push_str(&format!("{label}: {value}\n"));
                        }
                        // The events are what an unhealthy pod is
                        // explained by, so they travel with it.
                        if !inv.events.is_empty() {
                            out.push_str("\nRecent events:\n");
                            for e in inv.events.iter().take(12) {
                                out.push_str(&format!(
                                    "{} {} {}: {}\n",
                                    e.kind, e.object, e.reason, e.message
                                ));
                            }
                        }
                    }
                }
                Some(Err(e)) => out.push_str(&format!("The cluster could not be read: {e}\n")),
                None => out.push_str("The cluster has not been read yet.\n"),
            }
            if let Some(Ok(yaml)) = &self.yaml {
                let shown = self.yaml_edit.as_deref().unwrap_or(yaml);
                out.push_str("\nYAML on screen:\n");
                out.push_str(shown);
                out.push('\n');
            }
        } else {
            match &self.inventory {
                Some(Ok(inv)) => {
                    out.push_str(&format!(
                        "Engine {}\nContainers: {} ({} running), images: {}, volumes: {}, networks: {}\n",
                        inv.version,
                        inv.containers.len(),
                        inv.containers.iter().filter(|c| c.state.is_running()).count(),
                        inv.image_list.len(),
                        inv.volume_list.len(),
                        inv.network_list.len(),
                    ));
                    out.push_str(&format!("Open list: {}\n", s.t(self.state.resource.key())));
                    if let Some((_, id)) = &self.selected
                        && let Some(c) = inv.containers.iter().find(|c| &c.id == id)
                    {
                        out.push_str(&format!(
                            "\nSelected container: {}\nImage: {}\nState: {}\nStatus: {}\nPorts: {}\n",
                            c.name,
                            c.image,
                            s.t(c.state.key()),
                            c.status,
                            c.ports
                                .iter()
                                .map(|p| p.describe())
                                .collect::<Vec<_>>()
                                .join(", "),
                        ));
                    }
                }
                Some(Err(e)) => out.push_str(&format!("The engine could not be read: {e}\n")),
                None => out.push_str("The host has not been read yet.\n"),
            }
        }
        if let Some(output) = &self.last_output {
            out.push_str(&format!("\nLast command output:\n{output}\n"));
        }
        out
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
            // Images, volumes, networks and the Swarm lists have no verbs
            // of their own yet, but they have facts, and the facts are
            // what the column widths cut off.
            self.facts_detail(ui, theme, s, inv, res, &id);
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
                    (Some(why), true) => r.on_disabled_hover_text(docker::refusal_text(s, why)),
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

    /// The detail block for a list with no verbs of its own: the row's
    /// own facts, in full, under a header that names it.
    ///
    /// One function for images, volumes, networks, stacks, services and
    /// nodes rather than six: what differs between them is the list of
    /// facts, and that lives in `docker::summary` beside the columns it
    /// is the long form of.
    fn facts_detail(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        inv: &Inventory,
        res: docker::Resource,
        id: &str,
    ) {
        let facts = docker::summary(res, s, inv, id);
        if facts.is_empty() {
            // It has gone since the last read, which is information.
            form::hint(ui, theme, &s.t("container-detail-gone"));
            return;
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&facts[0].1).strong());
            ui.label(
                egui::RichText::new(s.t(res.key()))
                    .small()
                    .color(theme.text_muted),
            );
            hs_ui::toolbar::hard_right(ui, |ui| {
                if ui.small_button(s.t("close")).clicked() {
                    self.selected = None;
                }
            });
        });
        ui.add_space(4.0);
        // Keyed by the row it shows, so a grid never keeps the tallest
        // height a previous selection gave it (the Logs lesson).
        egui::Grid::new(("container-facts", res as u8, id))
            .num_columns(2)
            .spacing([12.0, 4.0])
            .show(ui, |ui| {
                for (label, value) in &facts {
                    ui.label(egui::RichText::new(label).small().color(theme.text_muted));
                    ui.add(egui::Label::new(value.as_str()).selectable(true).truncate());
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        if ui.small_button(s.t("container-detail-copy")).clicked() {
            let text = facts
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            ui.ctx().copy_text(text);
        }
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
                    (Some(why), true) => r.on_disabled_hover_text(docker::refusal_text(s, why)),
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

    /// The question a destructive cluster verb asks first.
    ///
    /// Scale asks as well as Delete: scaling to zero takes a service
    /// down as surely as deleting it, and the question is the only
    /// place the number is shown back before it is sent.
    fn confirm_cluster_dialog(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        s: &Services,
        profile: &ConnectionProfile,
    ) {
        let Some((verb, target)) = self.confirm_cluster.clone() else {
            return;
        };
        if !verb.destructive() {
            self.confirm_cluster = None;
            return;
        }
        let what = format!("{}/{}", target.kind, target.name);
        let label = s.t(verb.key());
        let question = match &verb {
            cluster::Verb::Scale(n) => s.i18n.t_args(
                "cluster-confirm-scale",
                &[("what", &what), ("replicas", &n.to_string())],
            ),
            _ => s
                .i18n
                .t_args("cluster-confirm-verb", &[("verb", &label), ("what", &what)]),
        };
        match hs_ui::dialog::confirm(
            ui.ctx(),
            theme,
            "cluster-verb",
            &label,
            &question,
            &label,
            &s.t("cancel"),
            true,
        ) {
            hs_ui::dialog::DialogResult::Ok => {
                self.confirm_cluster = None;
                self.act_cluster(s, profile, target, verb);
            }
            hs_ui::dialog::DialogResult::Cancel => self.confirm_cluster = None,
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

/// Open a host, or focus the tab that already shows it.
///
/// **One tab per host**, and that is the shell's doing: it reuses a tab
/// for an `OpenRequest::Connection` whose `current_connection` matches,
/// and for an `OpenRequest::File` whose uri matches - but a
/// `Text` request has no such rule, so every click opened another tab
/// (owner's report, 2026-10-25). A container host *is* a saved
/// connection profile, so this asks for it as one.
fn open_host(cx: &mut ToolContext, host_id: &str) {
    cx.actions.push(Action::OpenTool {
        factory_id: "containers".into(),
        request: Some(OpenRequest::Connection {
            id: host_id.to_string(),
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

    /// Which host this tab holds. The shell reuses a tab bound to the
    /// same connection rather than opening a second one
    /// (`tabs_per_connection` is 1 for every activity but SSH), so
    /// leaving this at its default `None` is what let a host open twice.
    fn current_connection(&self) -> Option<&str> {
        if self.is_home || self.state.host_id.is_empty() {
            return None;
        }
        Some(&self.state.host_id)
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
        // The assistant control is the hard right item of every tool
        // toolbar, through `hard_right` rather than a bare right-to-left
        // scope: that scope only gets the width left on the current row,
        // so in a narrow window the control is the first thing to vanish.
        hs_ui::toolbar::hard_right(ui, |ui| {
            if ui
                .button(format!("{} {}", ph::SPARKLE, s.t("ai-assistant")))
                .clicked()
            {
                self.assistant.toggle("Containers");
            }
        });
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
        let mut pin: Option<(String, bool)> = None;
        // Edit This Host goes to Settings -> Containers with the host
        // already selected: one place manages a host, and the sidebar is
        // where the user is looking when they want to change one.
        let mut edit: Option<String> = None;
        egui::ScrollArea::vertical()
            .id_salt("container-hosts")
            .max_height(hosts_height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if all.is_empty() {
                    form::hint(ui, &theme, &s.t("container-none"));
                }
                // The order is the user's own, as in every other activity's
                // connection list: a row is dragged by the grip on its left.
                let mut reorder = hs_ui::reorder::Reorder::new("container-hosts");
                for (i, p) in all.iter().enumerate() {
                    let f = ContainerFields::from_table(&p.fields);
                    let live = current.as_deref() == Some(&format!("container://{}", p.id));
                    reorder.row(ui, i, |ui| {
                        let row = form::connection_row(
                            ui,
                            &theme,
                            live,
                            kind_icon(&p.kind),
                            &p.name,
                            Some(&f.describe(&p.kind)),
                            p.pinned,
                            form::RowMenuLabels {
                                pin: &s.t("conn-pin-home"),
                                unpin: &s.t("conn-unpin-home"),
                                edit: &s.t("container-edit-host"),
                            },
                        );
                        if row.response.clicked() {
                            open = Some(p.id.clone());
                        }
                        if let Some(v) = row.pin {
                            pin = Some((p.id.clone(), v));
                        }
                        if row.edit {
                            edit = Some(p.id.clone());
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent) {
                    if let Err(e) = s.connections.write().reorder(
                        hs_core::connections::CONTAINER_KINDS,
                        from,
                        to,
                    ) {
                        cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
                    }
                    ui.ctx().request_repaint();
                }
            });
        if let Some((id, v)) = pin
            && let Err(e) = s.connections.write().set_pinned(&id, v)
        {
            cx.toast(hs_core::tool::ToastLevel::Error, e.to_string());
        }
        if let Some(id) = edit {
            cx.actions.push(Action::RunCommand {
                id: format!("settings:container:{id}"),
            });
        }
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
            return;
        }
        // The assistant takes its column first, so the pane is narrower
        // while it is open rather than partly hidden behind it.
        let (mut content, panel) =
            hs_ui::ai_assistant::split_for_assistant(ui, self.assistant.open);
        self.host_pane(&mut content, cx);
        self.assistant_column(ui, panel, cx);
    }
}
