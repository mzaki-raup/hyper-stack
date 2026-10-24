//! The Kubernetes pane, Lens-shaped (`docs/tools/containers.md` §10.6,
//! phase 6).
//!
//! The same shape as the Docker pane rather than a second design: a
//! resource rail, a filterable table, a detail block. What differs is
//! the namespace, which sits above the rail because it narrows every
//! list below it, and the fact that a cluster's resources are **read
//! across all namespaces or within one** - a choice Docker has no
//! equivalent of.
//!
//! The detail block is the Lens detail column folded under the list
//! rather than beside it: at this window's width a third column would
//! leave the list too narrow to read, and the Docker pane next door
//! already puts its detail underneath. Logs, Exec, Scale and Restart
//! for a pod are the action phase's (phase 7) - they need a log stream
//! and an edit path, not a reader.

use hs_container::k8s;
use hs_core::Services;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use serde::{Deserialize, Serialize};

/// Which list the cluster pane is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resource {
    #[default]
    Pods,
    Workloads,
    Services,
    Configs,
    Claims,
    Nodes,
    Events,
    /// What this cluster defines beyond the built-in kinds.
    Custom,
}

impl Resource {
    pub const ALL: [Resource; 8] = [
        Resource::Pods,
        Resource::Workloads,
        Resource::Services,
        Resource::Configs,
        Resource::Claims,
        Resource::Nodes,
        Resource::Events,
        Resource::Custom,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Resource::Pods => "cluster-res-pods",
            Resource::Workloads => "cluster-res-workloads",
            Resource::Services => "cluster-res-services",
            Resource::Configs => "cluster-res-configs",
            Resource::Claims => "cluster-res-claims",
            Resource::Nodes => "cluster-res-nodes",
            Resource::Events => "cluster-res-events",
            Resource::Custom => "cluster-res-custom",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Resource::Pods => ph::SHIPPING_CONTAINER,
            Resource::Workloads => ph::STACK,
            Resource::Services => ph::TREE_STRUCTURE,
            Resource::Configs => ph::FILE_CODE,
            Resource::Claims => ph::HARD_DRIVES,
            Resource::Nodes => ph::DESKTOP,
            Resource::Events => ph::BELL,
            Resource::Custom => ph::PUZZLE_PIECE,
        }
    }

    /// Whether this resource lives in a namespace. Nodes do not, so the
    /// namespace picker is irrelevant to them and saying otherwise
    /// would imply a filter that does nothing.
    pub fn namespaced(self) -> bool {
        // A definition is cluster-wide; its *objects* may be
        // namespaced, which the list below handles per definition.
        !matches!(self, Resource::Nodes | Resource::Custom)
    }

    /// What an empty list should say. "None" and "not allowed" are
    /// different answers; this is the first, and the driver's own error
    /// carries the second.
    pub fn empty_key(self) -> &'static str {
        match self {
            Resource::Pods => "cluster-no-pods",
            Resource::Workloads => "cluster-no-workloads",
            Resource::Services => "cluster-no-services",
            Resource::Configs => "cluster-no-configs",
            Resource::Claims => "cluster-no-claims",
            Resource::Nodes => "cluster-no-nodes",
            Resource::Events => "cluster-no-events",
            Resource::Custom => "cluster-no-custom",
        }
    }
}

/// One read of a cluster.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inventory {
    pub version: String,
    pub namespaces: Vec<String>,
    pub pods: Vec<k8s::PodRow>,
    pub workloads: Vec<k8s::WorkloadRow>,
    pub services: Vec<k8s::ServiceRow>,
    pub configs: Vec<k8s::ConfigRow>,
    pub claims: Vec<k8s::ConfigRow>,
    pub nodes: Vec<k8s::NodeRow>,
    pub events: Vec<k8s::EventRow>,
    /// The custom resources this cluster defines (phase 7).
    pub crds: Vec<k8s::CrdRow>,
}

impl Inventory {
    /// The counts beside each rail entry, in `Resource::ALL` order.
    pub fn counts(&self) -> Vec<usize> {
        Resource::ALL
            .iter()
            .map(|r| match r {
                Resource::Pods => self.pods.len(),
                Resource::Workloads => self.workloads.len(),
                Resource::Services => self.services.len(),
                Resource::Configs => self.configs.len(),
                Resource::Claims => self.claims.len(),
                Resource::Nodes => self.nodes.len(),
                Resource::Events => self.events.len(),
                Resource::Custom => self.crds.len(),
            })
            .collect()
    }

    /// How many things are not well, for the header.
    ///
    /// Counted here rather than in the driver: "unhealthy" is a
    /// presentation question, and a pod that is `Pending` for three
    /// seconds while it schedules is not the same as one that has been
    /// `CrashLoopBackOff` for an hour - but both deserve the eye.
    pub fn trouble(&self) -> usize {
        self.pods.iter().filter(|p| p.trouble).count()
            + self.workloads.iter().filter(|w| w.trouble).count()
            + self.nodes.iter().filter(|n| !n.ready).count()
    }

    /// Warnings among the events, which is the number someone scans for.
    pub fn warnings(&self) -> usize {
        self.events.iter().filter(|e| e.warning).count()
    }
}

/// Read a whole cluster, off the UI thread.
///
/// Only the version and the namespaces are allowed to fail the read:
/// an account that may list pods but not nodes should still see its
/// pods, so each listing that is refused contributes nothing rather
/// than losing everything.
pub async fn read(
    fields: &hs_container::ContainerFields,
    namespace: &str,
) -> Result<Inventory, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    let version = k8s::version(&client).await.map_err(|e| e.to_string())?;
    let namespaces = k8s::namespaces(&client).await.unwrap_or_default();
    Ok(Inventory {
        version,
        namespaces,
        pods: k8s::pods(&client, namespace).await.unwrap_or_default(),
        workloads: k8s::workloads(&client, namespace).await.unwrap_or_default(),
        services: k8s::services(&client, namespace).await.unwrap_or_default(),
        configs: k8s::configs(&client, namespace).await.unwrap_or_default(),
        claims: k8s::claims(&client, namespace).await.unwrap_or_default(),
        nodes: k8s::nodes(&client).await.unwrap_or_default(),
        events: k8s::events(&client, namespace).await.unwrap_or_default(),
        // A cluster with none, and an account that may not list the
        // definitions, both answer with an empty rail entry rather
        // than failing the whole read.
        crds: k8s::crds(&client).await.unwrap_or_default(),
    })
}

/// The columns and rows for one resource, as the shared table wants
/// them. Pure, so the shapes are testable without a cluster.
pub fn table_data(
    resource: Resource,
    s: &Services,
    inv: &Inventory,
) -> (Vec<hs_ui::table::ColumnSpec>, Vec<Vec<String>>) {
    use hs_ui::table::ColumnSpec;
    match resource {
        Resource::Pods => (
            vec![
                ColumnSpec::new(s.t("cluster-col-name"), 280.0),
                ColumnSpec::new(s.t("cluster-col-ready"), 80.0),
                ColumnSpec::new(s.t("cluster-col-status"), 160.0),
                ColumnSpec::new(s.t("cluster-col-restarts"), 90.0),
                ColumnSpec::new(s.t("cluster-col-node"), 160.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.pods
                .iter()
                .map(|p| {
                    vec![
                        p.name.clone(),
                        p.ready.clone(),
                        p.status.clone(),
                        p.restarts.to_string(),
                        p.node.clone(),
                        p.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Workloads => (
            vec![
                ColumnSpec::new(s.t("cluster-col-kind"), 120.0),
                ColumnSpec::new(s.t("cluster-col-name"), 240.0),
                ColumnSpec::new(s.t("cluster-col-ready"), 110.0),
                ColumnSpec::new(s.t("cluster-col-image"), 280.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.workloads
                .iter()
                .map(|w| {
                    vec![
                        w.kind.clone(),
                        w.name.clone(),
                        w.ready.clone(),
                        w.images.join(", "),
                        w.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Services => (
            vec![
                ColumnSpec::new(s.t("cluster-col-name"), 220.0),
                ColumnSpec::new(s.t("cluster-col-kind"), 120.0),
                ColumnSpec::new(s.t("cluster-col-cluster-ip"), 160.0),
                ColumnSpec::new(s.t("cluster-col-ports"), 220.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.services
                .iter()
                .map(|v| {
                    vec![
                        v.name.clone(),
                        v.kind.clone(),
                        v.cluster_ip.clone(),
                        v.ports.clone(),
                        v.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Configs => (
            vec![
                ColumnSpec::new(s.t("cluster-col-kind"), 120.0),
                ColumnSpec::new(s.t("cluster-col-name"), 240.0),
                ColumnSpec::new(s.t("cluster-col-keys"), 320.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.configs
                .iter()
                .map(|c| {
                    vec![
                        c.kind.clone(),
                        c.name.clone(),
                        // Keys, never values: a list that carries every
                        // secret is one that leaks them into a
                        // screenshot.
                        c.keys.join(", "),
                        c.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Claims => (
            vec![
                ColumnSpec::new(s.t("cluster-col-name"), 280.0),
                ColumnSpec::new(s.t("cluster-col-status"), 140.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.claims
                .iter()
                .map(|c| vec![c.name.clone(), c.kind.clone(), c.age.clone()])
                .collect(),
        ),
        Resource::Nodes => (
            vec![
                ColumnSpec::new(s.t("cluster-col-name"), 240.0),
                ColumnSpec::new(s.t("cluster-col-status"), 120.0),
                ColumnSpec::new(s.t("cluster-col-roles"), 160.0),
                ColumnSpec::new(s.t("cluster-col-version"), 160.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.nodes
                .iter()
                .map(|n| {
                    vec![
                        n.name.clone(),
                        n.status.clone(),
                        n.roles.clone(),
                        n.version.clone(),
                        n.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Custom => (
            vec![
                ColumnSpec::new(s.t("cluster-col-kind"), 160.0),
                ColumnSpec::new(s.t("cluster-col-name"), 300.0),
                ColumnSpec::new(s.t("cluster-col-group"), 220.0),
                ColumnSpec::new(s.t("cluster-col-version"), 100.0),
                ColumnSpec::new(s.t("cluster-col-scope"), 120.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
            ],
            inv.crds
                .iter()
                .map(|c| {
                    vec![
                        c.kind.clone(),
                        c.name.clone(),
                        c.group.clone(),
                        c.version.clone(),
                        s.t(if c.namespaced {
                            "cluster-scope-namespaced"
                        } else {
                            "cluster-scope-cluster"
                        }),
                        c.age.clone(),
                    ]
                })
                .collect(),
        ),
        Resource::Events => (
            vec![
                ColumnSpec::new(s.t("cluster-col-type"), 100.0),
                ColumnSpec::new(s.t("cluster-col-reason"), 160.0),
                ColumnSpec::new(s.t("cluster-col-object"), 240.0),
                ColumnSpec::new(s.t("cluster-col-count"), 70.0),
                ColumnSpec::new(s.t("cluster-col-age"), 80.0),
                ColumnSpec::new(s.t("cluster-col-message"), 520.0),
            ],
            inv.events
                .iter()
                .map(|e| {
                    vec![
                        e.kind.clone(),
                        e.reason.clone(),
                        e.object.clone(),
                        e.count.to_string(),
                        e.age.clone(),
                        e.message.clone(),
                    ]
                })
                .collect(),
        ),
    }
}

/// The object a row points at, as the YAML view needs to ask for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The Kubernetes kind, which the row already knows: a workload row
    /// carries whether it is a Deployment or a DaemonSet, so nobody is
    /// asked to repeat what is on the screen.
    pub kind: String,
    /// Empty for a cluster-scoped object.
    pub namespace: String,
    pub name: String,
}

/// What row `i` of `resource` points at.
///
/// An **event** points at nothing to fetch: its row already carries
/// everything the event knows, and the object it names is often gone -
/// which is frequently why the event exists.
pub fn row_target(resource: Resource, inv: &Inventory, i: usize) -> Option<Target> {
    let target = |kind: &str, namespace: &str, name: &str| Target {
        kind: kind.to_string(),
        namespace: namespace.to_string(),
        name: name.to_string(),
    };
    match resource {
        Resource::Pods => inv
            .pods
            .get(i)
            .map(|p| target("Pod", &p.namespace, &p.name)),
        Resource::Workloads => inv
            .workloads
            .get(i)
            .map(|w| target(&w.kind, &w.namespace, &w.name)),
        Resource::Services => inv
            .services
            .get(i)
            .map(|v| target("Service", &v.namespace, &v.name)),
        // The row's own kind: this list holds both config maps and
        // secrets, and they are different objects to fetch.
        Resource::Configs => inv
            .configs
            .get(i)
            .map(|c| target(&c.kind, &c.namespace, &c.name)),
        // A claim row's `kind` field is its phase (`Bound`), not its
        // kind - every row here is the same object.
        Resource::Claims => inv
            .claims
            .get(i)
            .map(|c| target("PersistentVolumeClaim", &c.namespace, &c.name)),
        Resource::Nodes => inv.nodes.get(i).map(|n| target("Node", "", &n.name)),
        // A definition is itself an object - of kind
        // `CustomResourceDefinition` - so its YAML is the schema, which
        // is exactly what someone selecting one wants to read.
        Resource::Custom => inv
            .crds
            .get(i)
            .map(|c| target("CustomResourceDefinition", "", &c.name)),
        Resource::Events => None,
    }
}

/// The facts the detail block lists for row `i`, as label/value pairs.
///
/// The wide fields the table has to truncate - a pod's containers, a
/// workload's images, an event's message - are the point of this block,
/// so they are here in full rather than repeated short.
pub fn summary(
    resource: Resource,
    s: &Services,
    inv: &Inventory,
    i: usize,
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut add = |label: String, value: String| {
        if !value.is_empty() {
            out.push((label, value));
        }
    };
    match resource {
        Resource::Pods => {
            let Some(p) = inv.pods.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-namespace"), p.namespace.clone());
            add(s.t("cluster-col-status"), p.status.clone());
            add(s.t("cluster-col-ready"), p.ready.clone());
            add(s.t("cluster-col-restarts"), p.restarts.to_string());
            add(s.t("cluster-col-node"), p.node.clone());
            add(s.t("cluster-col-age"), p.age.clone());
            add(s.t("cluster-detail-containers"), p.containers.join(", "));
        }
        Resource::Workloads => {
            let Some(w) = inv.workloads.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-kind"), w.kind.clone());
            add(s.t("cluster-namespace"), w.namespace.clone());
            add(s.t("cluster-col-ready"), w.ready.clone());
            add(s.t("cluster-detail-up-to-date"), w.up_to_date.to_string());
            add(s.t("cluster-detail-available"), w.available.to_string());
            add(s.t("cluster-col-age"), w.age.clone());
            add(s.t("cluster-col-image"), w.images.join(", "));
        }
        Resource::Services => {
            let Some(v) = inv.services.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-kind"), v.kind.clone());
            add(s.t("cluster-namespace"), v.namespace.clone());
            add(s.t("cluster-col-cluster-ip"), v.cluster_ip.clone());
            add(s.t("cluster-detail-external"), v.external.clone());
            add(s.t("cluster-col-ports"), v.ports.clone());
            add(s.t("cluster-col-age"), v.age.clone());
        }
        Resource::Configs => {
            let Some(c) = inv.configs.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-kind"), c.kind.clone());
            add(s.t("cluster-namespace"), c.namespace.clone());
            add(s.t("cluster-col-age"), c.age.clone());
            // Keys, never values - the same rule the list follows.
            add(s.t("cluster-col-keys"), c.keys.join(", "));
        }
        Resource::Claims => {
            let Some(c) = inv.claims.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-namespace"), c.namespace.clone());
            add(s.t("cluster-col-status"), c.kind.clone());
            add(s.t("cluster-col-age"), c.age.clone());
        }
        Resource::Nodes => {
            let Some(n) = inv.nodes.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-status"), n.status.clone());
            add(s.t("cluster-col-roles"), n.roles.clone());
            add(s.t("cluster-col-version"), n.version.clone());
            add(s.t("cluster-col-age"), n.age.clone());
        }
        Resource::Custom => {
            let Some(c) = inv.crds.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-kind"), c.kind.clone());
            add(s.t("cluster-col-group"), c.group.clone());
            add(s.t("cluster-col-version"), c.version.clone());
            add(s.t("cluster-detail-plural"), c.plural.clone());
            add(
                s.t("cluster-col-scope"),
                s.t(if c.namespaced {
                    "cluster-scope-namespaced"
                } else {
                    "cluster-scope-cluster"
                }),
            );
            add(s.t("cluster-col-age"), c.age.clone());
        }
        Resource::Events => {
            let Some(e) = inv.events.get(i) else {
                return Vec::new();
            };
            add(s.t("cluster-col-type"), e.kind.clone());
            add(s.t("cluster-col-reason"), e.reason.clone());
            add(s.t("cluster-col-object"), e.object.clone());
            add(s.t("cluster-col-count"), e.count.to_string());
            add(s.t("cluster-col-age"), e.age.clone());
            add(s.t("cluster-col-message"), e.message.clone());
        }
    }
    out
}

/// What the detail block's heading says for row `i`.
pub fn row_title(resource: Resource, inv: &Inventory, i: usize) -> String {
    if let Some(t) = row_target(resource, inv, i) {
        return if t.namespace.is_empty() {
            format!("{} · {}", t.kind, t.name)
        } else {
            format!("{} · {}/{}", t.kind, t.namespace, t.name)
        };
    }
    inv.events
        .get(i)
        .map(|e| format!("{} · {}", e.reason, e.object))
        .unwrap_or_default()
}

/// A write the cluster pane can ask for (phase 7).
///
/// Scale carries its number rather than reading it back from the pane
/// when the answer arrives: the field is editable while the request is
/// in flight, and a verb must do what was confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verb {
    Restart,
    Scale(i32),
    Delete,
}

impl Verb {
    pub fn key(&self) -> &'static str {
        match self {
            Verb::Restart => "cluster-verb-restart",
            Verb::Scale(_) => "cluster-verb-scale",
            Verb::Delete => "cluster-verb-delete",
        }
    }

    /// Whether this needs to be confirmed first.
    ///
    /// Scale is in here with Delete on purpose: scaling to zero takes a
    /// service down as surely as deleting it, and scaling *up* can cost
    /// real money on a cluster that autoscales its nodes.
    pub fn destructive(&self) -> bool {
        !matches!(self, Verb::Restart)
    }

    /// Which kinds this verb means anything for.
    pub fn applies_to(&self, kind: &str) -> bool {
        match self {
            // A daemon set runs one pod per node; there is no count to
            // set, and the API would refuse with its own words.
            Verb::Scale(_) => matches!(kind, "Deployment" | "StatefulSet"),
            Verb::Restart => matches!(kind, "Deployment" | "StatefulSet" | "DaemonSet"),
            Verb::Delete => true,
        }
    }
}

/// Carry out a verb, off the UI thread.
pub async fn act(
    fields: &hs_container::ContainerFields,
    target: &Target,
    verb: Verb,
) -> Result<String, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    let (kind, ns, name) = (&target.kind, &target.namespace, &target.name);
    match verb {
        Verb::Restart => k8s::restart(&client, kind, ns, name).await,
        Verb::Scale(n) => k8s::scale(&client, kind, ns, name, n).await,
        Verb::Delete => k8s::delete(&client, kind, ns, name).await,
    }
    .map_err(|e| e.to_string())?;
    Ok(format!("{kind}/{name}"))
}

/// Apply a manifest, dry run or for real, off the UI thread.
pub async fn apply(
    fields: &hs_container::ContainerFields,
    yaml: &str,
    namespace: &str,
    dry_run: bool,
) -> Result<Vec<k8s::Applied>, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    let docs = k8s::documents(yaml);
    if docs.is_empty() {
        return Err("there is nothing to apply".into());
    }
    let mut out = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        // Which document failed, rather than "the apply failed": a
        // manifest of six objects is the normal case.
        let applied = k8s::apply(&client, doc, namespace, dry_run)
            .await
            .map_err(|e| format!("document {}: {e}", i + 1))?;
        out.push(applied);
    }
    Ok(out)
}

/// The line-by-line difference between two YAML documents, as the
/// confirmation shows it.
///
/// Only the changed lines with a little context: a pod's YAML is two
/// hundred lines and an apply usually touches three of them, so a full
/// side-by-side would bury the answer the reader came for.
pub fn diff(before: &str, after: &str) -> Vec<(char, String)> {
    use similar::{ChangeTag, TextDiff};
    const CONTEXT: usize = 2;
    let diff = TextDiff::from_lines(before, after);
    let mut out = Vec::new();
    for (i, group) in diff.grouped_ops(CONTEXT).iter().enumerate() {
        if i > 0 {
            out.push(('@', "…".to_string()));
        }
        for op in group {
            for change in diff.iter_changes(op) {
                let sign = match change.tag() {
                    ChangeTag::Delete => '-',
                    ChangeTag::Insert => '+',
                    ChangeTag::Equal => ' ',
                };
                out.push((sign, change.value().trim_end().to_string()));
            }
        }
    }
    out
}

/// List the objects of one custom resource, off the UI thread.
pub async fn custom_objects(
    fields: &hs_container::ContainerFields,
    crd: &k8s::CrdRow,
    namespace: &str,
) -> Result<Vec<k8s::CustomRow>, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    k8s::custom(&client, crd, namespace)
        .await
        .map_err(|e| e.to_string())
}

/// Open a port forward to a pod, off the UI thread.
pub async fn forward(
    fields: &hs_container::ContainerFields,
    target: &Target,
    remote_port: u16,
    local_port: u16,
) -> Result<k8s::Forward, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    k8s::forward(
        &client,
        &target.namespace,
        &target.name,
        remote_port,
        local_port,
    )
    .await
    .map_err(|e| e.to_string())
}

/// The ports a pod declares, so the forward field opens on a useful
/// number instead of an empty box.
///
/// A container port is **declared**, not published: it is documentation
/// the image's author wrote, and a pod that listens on a port it never
/// declared still forwards fine. So this is a starting point, never a
/// restriction.
pub fn pod_ports(inv: &Inventory, i: usize) -> Vec<u16> {
    inv.pods.get(i).map(|p| p.ports.clone()).unwrap_or_default()
}

/// Fetch one object's YAML, off the UI thread.
pub async fn yaml(
    fields: &hs_container::ContainerFields,
    target: &Target,
) -> Result<String, String> {
    let client = k8s::connect(fields).await.map_err(|e| e.to_string())?;
    k8s::yaml(&client, &target.kind, &target.namespace, &target.name)
        .await
        .map_err(|e| e.to_string())
}

/// Draw the resource rail. Returns the resource clicked, if any.
pub fn resource_rail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    current: Resource,
    counts: &[usize],
) -> Option<Resource> {
    let mut picked = None;
    ui.vertical(|ui| {
        ui.set_width(150.0);
        for (i, r) in Resource::ALL.into_iter().enumerate() {
            let mut label = egui::text::LayoutJob::default();
            let font = egui::FontId::proportional(14.0);
            label.append(
                &format!("{}  {}  ", r.icon(), s.t(r.key())),
                0.0,
                egui::TextFormat::simple(font.clone(), theme.text),
            );
            if let Some(n) = counts.get(i) {
                label.append(
                    &n.to_string(),
                    0.0,
                    egui::TextFormat::simple(font, theme.text_muted),
                );
            }
            if ui
                .add(egui::Button::selectable(current == r, label).min_size(egui::vec2(140.0, 0.0)))
                .clicked()
            {
                picked = Some(r);
            }
            ui.add_space(2.0);
        }
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inv() -> Inventory {
        Inventory {
            version: "1.34".into(),
            namespaces: vec!["default".into(), "shop".into()],
            pods: vec![
                k8s::PodRow {
                    name: "web-1".into(),
                    status: "Running".into(),
                    ready: "1/1".into(),
                    trouble: false,
                    ..Default::default()
                },
                k8s::PodRow {
                    name: "broken-1".into(),
                    status: "ImagePullBackOff".into(),
                    ready: "0/1".into(),
                    trouble: true,
                    ..Default::default()
                },
            ],
            workloads: vec![k8s::WorkloadRow {
                name: "broken".into(),
                kind: "Deployment".into(),
                ready: "0/1".into(),
                trouble: true,
                ..Default::default()
            }],
            nodes: vec![k8s::NodeRow {
                name: "n1".into(),
                ready: true,
                ..Default::default()
            }],
            events: vec![
                k8s::EventRow {
                    kind: "Warning".into(),
                    warning: true,
                    ..Default::default()
                },
                k8s::EventRow {
                    kind: "Normal".into(),
                    warning: false,
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    /// The header's "needs attention" number is what makes the pane
    /// worth opening at a glance.
    #[test]
    fn trouble_counts_pods_workloads_and_unready_nodes() {
        let i = inv();
        assert_eq!(i.trouble(), 2, "one pod and one deployment");
        assert_eq!(i.warnings(), 1);

        let mut bad_node = inv();
        bad_node.nodes[0].ready = false;
        assert_eq!(bad_node.trouble(), 3, "an unready node counts too");
    }

    /// The counts line up with the rail, in the rail's own order; a
    /// mismatch would put a count beside the wrong row.
    #[test]
    fn the_counts_follow_the_rail_order() {
        let c = inv().counts();
        assert_eq!(c.len(), Resource::ALL.len());
        assert_eq!(c[0], 2, "pods first");
        assert_eq!(c[1], 1, "then workloads");
        let events = Resource::ALL
            .iter()
            .position(|r| *r == Resource::Events)
            .expect("events are on the rail");
        assert_eq!(c[events], 2, "beside Events, not beside its neighbour");
        let custom = Resource::ALL
            .iter()
            .position(|r| *r == Resource::Custom)
            .expect("custom resources are on the rail");
        assert_eq!(c[custom], 0, "this cluster defines none");
    }

    /// A node and a custom-resource **definition** are cluster-wide, so
    /// narrowing by namespace would be a filter that does nothing. (The
    /// *objects* of a custom resource may well be namespaced; that is
    /// the definition's own business, not the rail's.)
    #[test]
    fn only_the_namespaced_resources_are_narrowed() {
        let cluster_wide = [Resource::Nodes, Resource::Custom];
        for r in cluster_wide {
            assert!(!r.namespaced(), "{r:?} is cluster-wide");
        }
        for r in Resource::ALL
            .into_iter()
            .filter(|r| !cluster_wide.contains(r))
        {
            assert!(r.namespaced(), "{r:?} lives in a namespace");
        }
    }

    /// The kind the YAML view asks for comes from the row, and each
    /// list has its own trap: a config row may be either kind, and a
    /// claim row's `kind` field is its phase rather than its kind.
    #[test]
    fn a_row_names_the_object_the_yaml_view_must_fetch() {
        let mut i = inv();
        i.configs = vec![
            k8s::ConfigRow {
                name: "app".into(),
                namespace: "shop".into(),
                kind: "ConfigMap".into(),
                ..Default::default()
            },
            k8s::ConfigRow {
                name: "creds".into(),
                namespace: "shop".into(),
                kind: "Secret".into(),
                ..Default::default()
            },
        ];
        i.claims = vec![k8s::ConfigRow {
            name: "data".into(),
            namespace: "shop".into(),
            kind: "Bound".into(),
            ..Default::default()
        }];

        assert_eq!(row_target(Resource::Pods, &i, 0).unwrap().kind, "Pod");
        assert_eq!(
            row_target(Resource::Workloads, &i, 0).unwrap().kind,
            "Deployment",
            "the workload's own kind, not a guess"
        );
        assert_eq!(row_target(Resource::Configs, &i, 1).unwrap().kind, "Secret");
        assert_eq!(
            row_target(Resource::Claims, &i, 0).unwrap().kind,
            "PersistentVolumeClaim",
            "Bound is the phase, not the kind"
        );
        let node = row_target(Resource::Nodes, &i, 0).unwrap();
        assert!(node.namespace.is_empty(), "a node is cluster-scoped");
        assert_eq!(
            row_target(Resource::Events, &i, 0),
            None,
            "an event has no object of its own to fetch"
        );
        assert_eq!(
            row_target(Resource::Pods, &i, 99),
            None,
            "a row that is no longer there"
        );
    }

    /// Which verbs a kind offers, and which of them ask first.
    #[test]
    fn a_verb_is_offered_only_where_it_means_something() {
        assert!(Verb::Scale(1).applies_to("Deployment"));
        assert!(Verb::Scale(1).applies_to("StatefulSet"));
        assert!(
            !Verb::Scale(1).applies_to("DaemonSet"),
            "one pod per node is what a daemon set means"
        );
        assert!(!Verb::Scale(1).applies_to("Pod"));
        assert!(Verb::Restart.applies_to("DaemonSet"));
        assert!(!Verb::Restart.applies_to("Pod"), "a pod has no template");
        assert!(Verb::Delete.applies_to("ConfigMap"));

        // Scaling to zero takes a service down as surely as deleting
        // it, so it asks first; a rollout restart takes nothing away.
        assert!(Verb::Scale(0).destructive());
        assert!(Verb::Delete.destructive());
        assert!(!Verb::Restart.destructive());
    }

    /// The preview shows the changed lines with a little context: a
    /// pod's YAML is two hundred lines and an apply touches three.
    #[test]
    fn the_diff_shows_what_changed_and_not_the_rest() {
        let before = (1..=20)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let after = before.replace("line 10", "line ten");
        let d = diff(&before, &after);
        assert!(d.iter().any(|(s, l)| *s == '-' && l == "line 10"), "{d:?}");
        assert!(d.iter().any(|(s, l)| *s == '+' && l == "line ten"), "{d:?}");
        assert!(
            d.len() < 10,
            "only the neighbourhood of the change: {}",
            d.len()
        );
        assert!(
            diff(&before, &before).is_empty(),
            "nothing changed, nothing shown"
        );
    }

    #[test]
    fn every_resource_has_its_own_icon_label_and_empty_message() {
        let mut icons: Vec<&str> = Resource::ALL.iter().map(|r| r.icon()).collect();
        icons.sort_unstable();
        icons.dedup();
        assert_eq!(icons.len(), Resource::ALL.len(), "no two share a glyph");
        for r in Resource::ALL {
            assert!(r.key().starts_with("cluster-res-"));
            assert!(r.empty_key().starts_with("cluster-no-"));
        }
    }
}
