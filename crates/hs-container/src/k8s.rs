//! The Kubernetes driver (`docs/tools/containers.md` §5 and §10.3,
//! phase 6).
//!
//! In process through `kube` rather than by shelling out to `kubectl`,
//! for the two reasons §10.3 records: typed **watches** are what make a
//! live pane possible later, and needing no external binary keeps the
//! portable promise. `kube` also runs kubeconfig `exec` credential
//! plugins - the cloud CLIs - which is the part nobody should
//! reimplement.
//!
//! What a pane shows is **not** what the API returns. A `Pod` carries
//! a dozen status fields and no answer to "is it all right?"; the
//! column someone actually reads is a sentence Kubernetes never sends
//! (`Running`, `CrashLoopBackOff`, `ErrImagePull`), derived from the
//! container statuses underneath. That derivation is the bulk of this
//! file, and it is pure so it can be tested without a cluster - which
//! matters, because the interesting cases are the broken ones and a
//! healthy test cluster will not produce them on demand.

use std::collections::BTreeMap;
use std::time::Duration;

use k8s_openapi::api::apps::v1::{DaemonSet, Deployment, StatefulSet};
use k8s_openapi::api::batch::v1::{CronJob, Job};
use k8s_openapi::api::core::v1::{
    ConfigMap, Event, Namespace, Node, PersistentVolumeClaim, Pod, Secret, Service,
};
use kube::api::{Api, ListParams, ResourceExt};
use kube::config::{KubeConfigOptions, Kubeconfig};
use kube::{Client, Config};

use crate::profile::ContainerFields;

/// What went wrong, in terms a pane can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The kubeconfig could not be read or has no such context.
    Config(String),
    /// The cluster would not answer: unreachable, or the credentials
    /// were refused.
    Unreachable(String),
    /// It answered and said no - usually RBAC.
    Refused(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Config(m) => write!(f, "{m}"),
            Error::Unreachable(m) => write!(f, "could not reach the cluster: {m}"),
            Error::Refused(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Turn a `kube` error into one of ours.
///
/// The distinction matters to the reader: "your token is wrong",
/// "this account may not list pods" and "nothing is listening" have
/// three different remedies, and the API says which.
pub(crate) fn classify(e: kube::Error) -> Error {
    // A request that could not even be built never left this process,
    // so "could not reach the cluster" would be a lie - and a confusing
    // one, because the remedy is in the code or the arguments rather
    // than in the network. A bad `PatchParams` combination surfaced
    // exactly this way (2026-10-24).
    if let kube::Error::BuildRequest(inner) = &e {
        return Error::Refused(format!("the request could not be built: {inner}"));
    }
    if let kube::Error::SerdeError(inner) = &e {
        return Error::Refused(format!("the cluster's answer could not be read: {inner}"));
    }
    if let kube::Error::Api(resp) = &e {
        return match resp.code {
            401 => Error::Refused(format!(
                "the cluster refused these credentials ({})",
                resp.message
            )),
            403 => Error::Refused(format!(
                "this account may not do that here ({})",
                resp.message
            )),
            404 => Error::Refused(format!("there is no such resource ({})", resp.message)),
            _ => Error::Refused(resp.message.clone()),
        };
    }
    Error::Unreachable(e.to_string())
}

/// Connect to the cluster a profile names.
///
/// `kubeconfig_inline` wins over the path when it is set: a profile may
/// carry its own config out of the vault (§10.2) rather than pointing
/// at a file on disk.
pub async fn connect(fields: &ContainerFields) -> Result<Client> {
    install_crypto_provider();
    let options = KubeConfigOptions {
        context: Some(fields.context.clone()).filter(|c| !c.trim().is_empty()),
        ..Default::default()
    };
    let kubeconfig = if !fields.kubeconfig_inline.trim().is_empty() {
        Kubeconfig::from_yaml(&fields.kubeconfig_inline)
            .map_err(|e| Error::Config(format!("the stored kubeconfig could not be read: {e}")))?
    } else if !fields.kubeconfig_path.trim().is_empty() {
        let path = std::path::Path::new(fields.kubeconfig_path.trim());
        Kubeconfig::read_from(path)
            .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?
    } else {
        // No path at all: the conventional location, which is what
        // every other Kubernetes tool falls back to.
        Kubeconfig::read()
            .map_err(|e| Error::Config(format!("no kubeconfig could be read: {e}")))?
    };
    let mut config = Config::from_custom_kubeconfig(kubeconfig, &options)
        .await
        .map_err(|e| Error::Config(e.to_string()))?;
    if fields.insecure_tls {
        config.accept_invalid_certs = true;
    }
    config.connect_timeout = Some(std::time::Duration::from_secs(
        fields.connect_timeout_secs.max(1) as u64,
    ));
    Client::try_from(config).map_err(|e| Error::Unreachable(e.to_string()))
}

/// Pick **ring** as the process-wide rustls provider.
///
/// More than one provider is compiled into this process (reqwest
/// brings aws-lc-rs, the rest of the app uses ring), and rustls then
/// refuses to choose: the first TLS handshake panics with "could not
/// automatically determine the process-level CryptoProvider". The SQL
/// driver already learned this (CLAUDE.md); a cluster opened before any
/// database would have hit it first here. `Once`, and the result is
/// ignored, because whichever caller gets there first is right and a
/// second install is not an error worth reporting.
fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// What the cluster says it is, for the toolbar.
pub async fn version(client: &Client) -> Result<String> {
    let info = client.apiserver_version().await.map_err(classify)?;
    Ok(format!("{}.{}", info.major, info.minor))
}

// --- what the panes show ---------------------------------------------

/// A pod as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PodRow {
    pub name: String,
    pub namespace: String,
    /// The sentence a reader wants: `Running`, `CrashLoopBackOff`,
    /// `ErrImagePull`, `Completed`, `Terminating`.
    pub status: String,
    /// Containers ready out of containers wanted, as `1/2`.
    pub ready: String,
    pub restarts: i32,
    pub node: String,
    pub age: String,
    /// Whether this is worth drawing attention to.
    pub trouble: bool,
    pub containers: Vec<String>,
    /// The container ports this pod declares, for the port forward to
    /// open on. Declared, not published - a pod may listen on a port it
    /// never mentioned - so these are a starting point, not a list of
    /// what is forwardable.
    pub ports: Vec<u16>,
}

/// A workload (deployment, statefulset, daemonset) as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WorkloadRow {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    /// `2/2` - ready over desired.
    pub ready: String,
    pub up_to_date: i32,
    pub available: i32,
    pub age: String,
    pub images: Vec<String>,
    pub trouble: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServiceRow {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub cluster_ip: String,
    pub external: String,
    pub ports: String,
    pub age: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NodeRow {
    pub name: String,
    pub status: String,
    pub roles: String,
    pub version: String,
    pub age: String,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventRow {
    pub kind: String,
    pub reason: String,
    pub object: String,
    pub message: String,
    pub count: i32,
    pub age: String,
    pub warning: bool,
}

/// A config map or secret. The **values of a secret are not carried**
/// unless asked for: a list that holds every secret in memory is a list
/// that leaks them into a screenshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConfigRow {
    pub name: String,
    pub namespace: String,
    pub kind: String,
    pub keys: Vec<String>,
    pub age: String,
}

/// How a pod is doing, from its status rather than from a single field.
///
/// Kubernetes has no "status" string for a pod; `kubectl` builds the
/// one everybody reads out of the container statuses, and so does this.
/// The order matters: a pod being deleted is `Terminating` whatever
/// its containers say, and a container that is waiting with a reason
/// (`ErrImagePull`, `CrashLoopBackOff`) is the most useful sentence
/// there is.
pub fn pod_status(pod: &Pod) -> (String, bool) {
    if pod.metadata.deletion_timestamp.is_some() {
        return ("Terminating".into(), true);
    }
    let status = pod.status.as_ref();
    let phase = status
        .and_then(|s| s.phase.clone())
        .unwrap_or_else(|| "Unknown".into());

    // A waiting or terminated container with a reason beats the phase:
    // `Running` over a CrashLoopBackOff is the single most misleading
    // thing this column could say.
    if let Some(statuses) = status.and_then(|s| s.container_statuses.as_ref()) {
        for c in statuses {
            if let Some(state) = &c.state {
                if let Some(w) = &state.waiting
                    && let Some(reason) = &w.reason
                    && reason != "ContainerCreating"
                    && reason != "PodInitializing"
                {
                    return (reason.clone(), true);
                }
                if let Some(t) = &state.terminated
                    && let Some(reason) = &t.reason
                    && reason != "Completed"
                {
                    return (reason.clone(), true);
                }
            }
        }
    }
    let trouble = matches!(phase.as_str(), "Failed" | "Unknown" | "Pending");
    (phase, trouble)
}

/// Containers ready out of containers wanted.
pub fn pod_ready(pod: &Pod) -> String {
    let statuses = pod
        .status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref());
    let total = pod
        .spec
        .as_ref()
        .map(|s| s.containers.len())
        .unwrap_or_else(|| statuses.map(|s| s.len()).unwrap_or(0));
    let ready = statuses
        .map(|s| s.iter().filter(|c| c.ready).count())
        .unwrap_or(0);
    format!("{ready}/{total}")
}

/// Total restarts across every container, which is what the column
/// shows and what a reader is actually asking about.
pub fn pod_restarts(pod: &Pod) -> i32 {
    pod.status
        .as_ref()
        .and_then(|s| s.container_statuses.as_ref())
        .map(|cs| cs.iter().map(|c| c.restart_count).sum())
        .unwrap_or(0)
}

/// How long ago, in the short form every Kubernetes tool uses: `3d`,
/// `4h12m`, `47s`. Never "0s": something that has just appeared reads
/// better as `1s`.
pub fn age_of(created: Option<&k8s_openapi::apimachinery::pkg::apis::meta::v1::Time>) -> String {
    let Some(t) = created else {
        return String::new();
    };
    short_age(seconds_since(t))
}

/// Seconds since a Kubernetes timestamp.
///
/// Through epoch seconds rather than the timestamp type's own
/// arithmetic: `k8s-openapi` carries a `jiff::Timestamp` (it moved off
/// chrono), and comparing epoch seconds works whichever clock crate
/// the next version brings.
fn seconds_since(t: &k8s_openapi::apimachinery::pkg::apis::meta::v1::Time) -> i64 {
    let now = k8s_openapi::jiff::Timestamp::now().as_second();
    (now - t.0.as_second()).max(1)
}

/// The age arithmetic on its own, so it can be tested without a clock.
pub fn short_age(secs: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    if secs < MIN {
        return format!("{secs}s");
    }
    if secs < HOUR {
        let m = secs / MIN;
        let s = secs % MIN;
        return if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m{s}s")
        };
    }
    if secs < DAY {
        let h = secs / HOUR;
        let m = (secs % HOUR) / MIN;
        return if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{m}m")
        };
    }
    let d = secs / DAY;
    let h = (secs % DAY) / HOUR;
    // Past a day, minutes are noise.
    if h == 0 {
        format!("{d}d")
    } else {
        format!("{d}d{h}h")
    }
}

/// A node's roles, from the labels Kubernetes marks them with.
pub fn node_roles(labels: &BTreeMap<String, String>) -> String {
    let mut roles: Vec<String> = labels
        .keys()
        .filter_map(|k| k.strip_prefix("node-role.kubernetes.io/"))
        .filter(|r| !r.is_empty())
        .map(str::to_string)
        .collect();
    roles.sort();
    roles.dedup();
    if roles.is_empty() {
        "<none>".into()
    } else {
        roles.join(",")
    }
}

/// A service's ports, as `80:30080/TCP, 443/TCP`.
pub fn service_ports(svc: &Service) -> String {
    let Some(spec) = &svc.spec else {
        return String::new();
    };
    let Some(ports) = &spec.ports else {
        return String::new();
    };
    ports
        .iter()
        .map(|p| {
            let proto = p.protocol.clone().unwrap_or_else(|| "TCP".into());
            match p.node_port {
                Some(np) => format!("{}:{np}/{proto}", p.port),
                None => format!("{}/{proto}", p.port),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// --- the listings ----------------------------------------------------

/// Every namespace, sorted, so the tree does not reshuffle on refresh.
pub async fn namespaces(client: &Client) -> Result<Vec<String>> {
    let api: Api<Namespace> = Api::all(client.clone());
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<String> = list.iter().map(|n| n.name_any()).collect();
    out.sort();
    Ok(out)
}

/// One namespace's pods, or every namespace when `ns` is empty.
pub async fn pods(client: &Client, ns: &str) -> Result<Vec<PodRow>> {
    let api: Api<Pod> = scoped(client, ns);
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<PodRow> = list
        .iter()
        .map(|p| {
            let (status, trouble) = pod_status(p);
            PodRow {
                name: p.name_any(),
                namespace: p.namespace().unwrap_or_default(),
                status,
                ready: pod_ready(p),
                restarts: pod_restarts(p),
                node: p
                    .spec
                    .as_ref()
                    .and_then(|s| s.node_name.clone())
                    .unwrap_or_default(),
                age: age_of(p.metadata.creation_timestamp.as_ref()),
                trouble,
                containers: p
                    .spec
                    .as_ref()
                    .map(|s| s.containers.iter().map(|c| c.name.clone()).collect())
                    .unwrap_or_default(),
                ports: pod_ports(p),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Deployments, statefulsets and daemonsets in one list: they answer
/// the same question and a pane that split them would make someone
/// look in three places for "what is running here".
pub async fn workloads(client: &Client, ns: &str) -> Result<Vec<WorkloadRow>> {
    let mut out = Vec::new();

    let deployments: Api<Deployment> = scoped(client, ns);
    for d in deployments
        .list(&ListParams::default())
        .await
        .map_err(classify)?
    {
        let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(0);
        let status = d.status.clone().unwrap_or_default();
        let ready = status.ready_replicas.unwrap_or(0);
        out.push(WorkloadRow {
            name: d.name_any(),
            namespace: d.namespace().unwrap_or_default(),
            kind: "Deployment".into(),
            ready: format!("{ready}/{desired}"),
            up_to_date: status.updated_replicas.unwrap_or(0),
            available: status.available_replicas.unwrap_or(0),
            age: age_of(d.metadata.creation_timestamp.as_ref()),
            images: images_of(d.spec.as_ref().and_then(|s| s.template.spec.as_ref())),
            trouble: ready < desired,
        });
    }

    let statefulsets: Api<StatefulSet> = scoped(client, ns);
    for s in statefulsets
        .list(&ListParams::default())
        .await
        .map_err(classify)?
    {
        let desired = s.spec.as_ref().and_then(|x| x.replicas).unwrap_or(0);
        let status = s.status.clone().unwrap_or_default();
        let ready = status.ready_replicas.unwrap_or(0);
        out.push(WorkloadRow {
            name: s.name_any(),
            namespace: s.namespace().unwrap_or_default(),
            kind: "StatefulSet".into(),
            ready: format!("{ready}/{desired}"),
            up_to_date: status.updated_replicas.unwrap_or(0),
            available: ready,
            age: age_of(s.metadata.creation_timestamp.as_ref()),
            images: images_of(s.spec.as_ref().and_then(|x| x.template.spec.as_ref())),
            trouble: ready < desired,
        });
    }

    let daemonsets: Api<DaemonSet> = scoped(client, ns);
    for d in daemonsets
        .list(&ListParams::default())
        .await
        .map_err(classify)?
    {
        let status = d.status.clone().unwrap_or_default();
        let desired = status.desired_number_scheduled;
        let ready = status.number_ready;
        out.push(WorkloadRow {
            name: d.name_any(),
            namespace: d.namespace().unwrap_or_default(),
            kind: "DaemonSet".into(),
            ready: format!("{ready}/{desired}"),
            up_to_date: status.updated_number_scheduled.unwrap_or(0),
            available: status.number_available.unwrap_or(0),
            age: age_of(d.metadata.creation_timestamp.as_ref()),
            images: images_of(d.spec.as_ref().and_then(|x| x.template.spec.as_ref())),
            trouble: ready < desired,
        });
    }

    let jobs: Api<Job> = scoped(client, ns);
    for j in jobs.list(&ListParams::default()).await.map_err(classify)? {
        let status = j.status.clone().unwrap_or_default();
        let succeeded = status.succeeded.unwrap_or(0);
        let desired = j.spec.as_ref().and_then(|s| s.completions).unwrap_or(1);
        out.push(WorkloadRow {
            name: j.name_any(),
            namespace: j.namespace().unwrap_or_default(),
            kind: "Job".into(),
            ready: format!("{succeeded}/{desired}"),
            up_to_date: succeeded,
            available: succeeded,
            age: age_of(j.metadata.creation_timestamp.as_ref()),
            images: images_of(j.spec.as_ref().and_then(|s| s.template.spec.as_ref())),
            trouble: status.failed.unwrap_or(0) > 0,
        });
    }

    let cronjobs: Api<CronJob> = scoped(client, ns);
    for c in cronjobs
        .list(&ListParams::default())
        .await
        .map_err(classify)?
    {
        let schedule = c
            .spec
            .as_ref()
            .map(|s| s.schedule.clone())
            .unwrap_or_default();
        let suspended = c.spec.as_ref().and_then(|s| s.suspend).unwrap_or(false);
        out.push(WorkloadRow {
            name: c.name_any(),
            namespace: c.namespace().unwrap_or_default(),
            kind: "CronJob".into(),
            // A schedule is what someone wants to see for a cron job,
            // and there is no "ready" for one that has never run.
            ready: schedule,
            up_to_date: 0,
            available: 0,
            age: age_of(c.metadata.creation_timestamp.as_ref()),
            images: Vec::new(),
            trouble: suspended,
        });
    }

    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub async fn services(client: &Client, ns: &str) -> Result<Vec<ServiceRow>> {
    let api: Api<Service> = scoped(client, ns);
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<ServiceRow> = list
        .iter()
        .map(|s| {
            let spec = s.spec.clone().unwrap_or_default();
            ServiceRow {
                name: s.name_any(),
                namespace: s.namespace().unwrap_or_default(),
                kind: spec.type_.clone().unwrap_or_else(|| "ClusterIP".into()),
                cluster_ip: spec.cluster_ip.clone().unwrap_or_default(),
                external: spec.external_ips.clone().unwrap_or_default().join(", "),
                ports: service_ports(s),
                age: age_of(s.metadata.creation_timestamp.as_ref()),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub async fn nodes(client: &Client) -> Result<Vec<NodeRow>> {
    let api: Api<Node> = Api::all(client.clone());
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<NodeRow> = list
        .iter()
        .map(|n| {
            let ready = n
                .status
                .as_ref()
                .and_then(|s| s.conditions.as_ref())
                .map(|cs| cs.iter().any(|c| c.type_ == "Ready" && c.status == "True"))
                .unwrap_or(false);
            NodeRow {
                name: n.name_any(),
                status: if ready {
                    "Ready".into()
                } else {
                    "NotReady".into()
                },
                roles: node_roles(n.labels()),
                version: n
                    .status
                    .as_ref()
                    .and_then(|s| s.node_info.as_ref())
                    .map(|i| i.kubelet_version.clone())
                    .unwrap_or_default(),
                age: age_of(n.metadata.creation_timestamp.as_ref()),
                ready,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Recent events, newest first: an event list in any other order is
/// nearly useless, because the reason anyone opens it is "what just
/// happened".
pub async fn events(client: &Client, ns: &str) -> Result<Vec<EventRow>> {
    let api: Api<Event> = scoped(client, ns);
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut rows: Vec<(i64, EventRow)> = list
        .iter()
        .map(|e| {
            let when = e
                .last_timestamp
                .as_ref()
                .or(e.first_timestamp.as_ref())
                .or(e.metadata.creation_timestamp.as_ref());
            let secs = when.map(seconds_since).unwrap_or(i64::MAX);
            let kind = e.type_.clone().unwrap_or_default();
            (
                secs,
                EventRow {
                    warning: kind == "Warning",
                    kind,
                    reason: e.reason.clone().unwrap_or_default(),
                    object: format!(
                        "{}/{}",
                        e.involved_object.kind.clone().unwrap_or_default(),
                        e.involved_object.name.clone().unwrap_or_default()
                    ),
                    message: e.message.clone().unwrap_or_default(),
                    count: e.count.unwrap_or(1),
                    age: when.map(|_| short_age(secs)).unwrap_or_default(),
                },
            )
        })
        .collect();
    rows.sort_by_key(|(secs, _)| *secs);
    Ok(rows.into_iter().map(|(_, r)| r).collect())
}

/// Config maps and secrets. **Only the keys of a secret**, never the
/// values: a list that carries every secret is one that leaks them
/// into a screenshot. The detail view fetches one on request, which is
/// where the reveal guard belongs.
pub async fn configs(client: &Client, ns: &str) -> Result<Vec<ConfigRow>> {
    let mut out = Vec::new();
    let maps: Api<ConfigMap> = scoped(client, ns);
    for c in maps.list(&ListParams::default()).await.map_err(classify)? {
        let mut keys: Vec<String> = c.data.clone().unwrap_or_default().into_keys().collect();
        keys.sort();
        out.push(ConfigRow {
            name: c.name_any(),
            namespace: c.namespace().unwrap_or_default(),
            kind: "ConfigMap".into(),
            keys,
            age: age_of(c.metadata.creation_timestamp.as_ref()),
        });
    }
    let secrets: Api<Secret> = scoped(client, ns);
    for s in secrets
        .list(&ListParams::default())
        .await
        .map_err(classify)?
    {
        let mut keys: Vec<String> = s.data.clone().unwrap_or_default().into_keys().collect();
        keys.sort();
        out.push(ConfigRow {
            name: s.name_any(),
            namespace: s.namespace().unwrap_or_default(),
            kind: "Secret".into(),
            keys,
            age: age_of(s.metadata.creation_timestamp.as_ref()),
        });
    }
    out.sort_by(|a, b| a.kind.cmp(&b.kind).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub async fn claims(client: &Client, ns: &str) -> Result<Vec<ConfigRow>> {
    let api: Api<PersistentVolumeClaim> = scoped(client, ns);
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<ConfigRow> = list
        .iter()
        .map(|c| ConfigRow {
            name: c.name_any(),
            namespace: c.namespace().unwrap_or_default(),
            kind: c
                .status
                .as_ref()
                .and_then(|s| s.phase.clone())
                .unwrap_or_else(|| "Unknown".into()),
            keys: Vec::new(),
            age: age_of(c.metadata.creation_timestamp.as_ref()),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// One object as YAML, which is what Describe shows and what an edit
/// will start from in phase 7.
///
/// The kind is the one the list put in the row, so a reader never picks
/// it: a workload row already knows whether it is a Deployment or a
/// DaemonSet, and asking would be asking them to repeat the screen.
pub async fn yaml(client: &Client, kind: &str, ns: &str, name: &str) -> Result<String> {
    match kind {
        "Pod" => one::<Pod>(client, ns, name).await,
        "Deployment" => one::<Deployment>(client, ns, name).await,
        "StatefulSet" => one::<StatefulSet>(client, ns, name).await,
        "DaemonSet" => one::<DaemonSet>(client, ns, name).await,
        "Job" => one::<Job>(client, ns, name).await,
        "CronJob" => one::<CronJob>(client, ns, name).await,
        "Service" => one::<Service>(client, ns, name).await,
        "ConfigMap" => one::<ConfigMap>(client, ns, name).await,
        "PersistentVolumeClaim" => one::<PersistentVolumeClaim>(client, ns, name).await,
        "Node" => {
            let api: Api<Node> = Api::all(client.clone());
            let node = api.get(name).await.map_err(classify)?;
            render(node)
        }
        // A secret's YAML is the one listing that would put credentials
        // on the screen, so the values are replaced before it is
        // rendered. The keys stay: which keys exist is the question a
        // reader actually has.
        "Secret" => {
            let api: Api<Secret> = scoped(client, ns);
            let secret = api.get(name).await.map_err(classify)?;
            render(redact(secret))
        }
        other => Err(Error::Refused(format!("{other} has no YAML view"))),
    }
}

/// What a secret's values are replaced with before it is rendered.
pub const REDACTED: &str = "<redacted by HyperStack>";

/// Replace a secret's values, keeping its keys.
///
/// Which keys exist is the question a reader has; the values are the
/// one thing in this whole pane that must never land in a screenshot,
/// and `data` is only base64 - not encryption. Reveal belongs behind
/// the vault's own guard, not in a view that opens on a click.
fn redact(mut secret: Secret) -> Secret {
    secret.data = secret.data.map(|d| {
        d.into_keys()
            .map(|k| (k, k8s_openapi::ByteString(REDACTED.as_bytes().to_vec())))
            .collect()
    });
    secret.string_data = secret
        .string_data
        .map(|d| d.into_keys().map(|k| (k, REDACTED.to_string())).collect());
    secret
}

/// Fetch one namespaced object and render it.
async fn one<K>(client: &Client, ns: &str, name: &str) -> Result<String>
where
    K: kube::Resource<Scope = k8s_openapi::NamespaceResourceScope>
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + serde::de::DeserializeOwned,
    <K as kube::Resource>::DynamicType: Default,
{
    let api: Api<K> = scoped(client, ns);
    let object = api.get(name).await.map_err(classify)?;
    render(object)
}

/// Serialise an object the way a reader wants it.
///
/// Two pieces of bookkeeping are dropped first, and the second is a
/// **leak**, not a tidy-up:
///
/// - `managedFields` is server-side-apply accounting, routinely longer
///   than the spec it describes, and never the reason anybody opened
///   this view. `kubectl get -o yaml` keeps it; every cluster UI hides
///   it.
/// - `kubectl.kubernetes.io/last-applied-configuration` is a **verbatim
///   copy of the object as it was applied**, and for a secret applied
///   with `kubectl apply` that copy holds the values in the clear -
///   `"DB_PASSWORD":"hunter2"` sat in it while `data` above read
///   `<redacted>` (seen on the k3s bench, 2026-10-24). Redacting the
///   fields and leaving their duplicate is no redaction at all, so the
///   annotation goes for every kind rather than for secrets alone: one
///   rule has no hole for the next kind to fall through.
fn render<K: kube::Resource + serde::Serialize>(mut object: K) -> Result<String> {
    let meta = object.meta_mut();
    meta.managed_fields = None;
    if let Some(annotations) = meta.annotations.as_mut() {
        annotations.remove(LAST_APPLIED);
        if annotations.is_empty() {
            meta.annotations = None;
        }
    }
    serde_norway::to_string(&object).map_err(|e| Error::Refused(e.to_string()))
}

/// The annotation `kubectl apply` leaves behind: the whole object as it
/// was applied, values and all.
const LAST_APPLIED: &str = "kubectl.kubernetes.io/last-applied-configuration";

/// Follow one pod's log, as a stream of chunks (phase 7).
///
/// The Logs activity drives this rather than the pane: a log viewer
/// inside this activity would be a worse copy of the one next door
/// (§10.1). There is no `kubectl logs` subprocess behind it - the API
/// server streams it, which is also why a pod that restarts mid-follow
/// ends the stream rather than silently tailing the old container.
///
/// `container` may be empty for a single-container pod; naming it is
/// required the moment there are two, and the API says so itself.
pub async fn log_stream(
    client: &Client,
    ns: &str,
    pod: &str,
    container: &str,
    tail: i64,
    follow: bool,
) -> Result<impl futures_util::AsyncBufRead> {
    let api: Api<Pod> = scoped(client, ns);
    let params = kube::api::LogParams {
        container: Some(container.to_string()).filter(|c| !c.trim().is_empty()),
        follow,
        tail_lines: Some(tail.max(0)),
        // Timestamps on, because a log line that carries none leaves
        // the Logs tool's time strip and Jump to Time with nothing to
        // work from, and the API server knows the time for certain.
        timestamps: true,
        ..Default::default()
    };
    api.log_stream(pod, &params).await.map_err(classify)
}

/// Scale a workload. `kind` is the row's own kind.
///
/// A daemon set has no replicas - one pod per node is what it means -
/// so scaling one is refused in its own words rather than failing with
/// the API's.
pub async fn scale(client: &Client, kind: &str, ns: &str, name: &str, replicas: i32) -> Result<()> {
    let patch = serde_json::json!({ "spec": { "replicas": replicas.max(0) } });
    // A **merge** patch on the `scale` subresource, exactly as
    // `kubectl scale` does it: the replica count is the only field
    // being set, and server-side apply would make this app the owner of
    // a field the deployment's own manifest also sets. `force()` is
    // refused outright here - "PatchParams::force only works with
    // Patch::Apply" - and the request never leaves the process
    // (measured against k3s 1.34, 2026-10-24).
    let params = kube::api::PatchParams::default();
    match kind {
        "Deployment" => {
            let api: Api<Deployment> = scoped(client, ns);
            api.patch_scale(name, &params, &kube::api::Patch::Merge(&patch))
                .await
                .map_err(classify)?;
        }
        "StatefulSet" => {
            let api: Api<StatefulSet> = scoped(client, ns);
            api.patch_scale(name, &params, &kube::api::Patch::Merge(&patch))
                .await
                .map_err(classify)?;
        }
        other => return Err(Error::Refused(format!("{other} has no replica count"))),
    }
    Ok(())
}

/// Roll a workload's pods, the way `kubectl rollout restart` does it.
///
/// There is no "restart" verb in the API: the controller only reacts to
/// a change in the pod template, so the annotation **is** the restart.
/// Writing anything else (deleting the pods, scaling to zero and back)
/// loses the rollout's own surge and availability guarantees.
pub async fn restart(client: &Client, kind: &str, ns: &str, name: &str) -> Result<()> {
    let now = k8s_openapi::jiff::Timestamp::now().to_string();
    let patch = serde_json::json!({
        "spec": { "template": { "metadata": { "annotations": {
            "kubectl.kubernetes.io/restartedAt": now
        } } } }
    });
    let params = kube::api::PatchParams::default();
    let patch = kube::api::Patch::Merge(&patch);
    match kind {
        "Deployment" => {
            let api: Api<Deployment> = scoped(client, ns);
            api.patch(name, &params, &patch).await.map_err(classify)?;
        }
        "StatefulSet" => {
            let api: Api<StatefulSet> = scoped(client, ns);
            api.patch(name, &params, &patch).await.map_err(classify)?;
        }
        "DaemonSet" => {
            let api: Api<DaemonSet> = scoped(client, ns);
            api.patch(name, &params, &patch).await.map_err(classify)?;
        }
        other => {
            return Err(Error::Refused(format!(
                "{other} has no pod template to roll"
            )));
        }
    }
    Ok(())
}

/// Delete one object. Guarded by the caller, never here: the driver
/// does what it is told and the pane decides whether to ask.
pub async fn delete(client: &Client, kind: &str, ns: &str, name: &str) -> Result<()> {
    match kind {
        "Pod" => delete_one::<Pod>(client, ns, name).await,
        "Deployment" => delete_one::<Deployment>(client, ns, name).await,
        "StatefulSet" => delete_one::<StatefulSet>(client, ns, name).await,
        "DaemonSet" => delete_one::<DaemonSet>(client, ns, name).await,
        "Job" => delete_one::<Job>(client, ns, name).await,
        "CronJob" => delete_one::<CronJob>(client, ns, name).await,
        "Service" => delete_one::<Service>(client, ns, name).await,
        "ConfigMap" => delete_one::<ConfigMap>(client, ns, name).await,
        "Secret" => delete_one::<Secret>(client, ns, name).await,
        "PersistentVolumeClaim" => delete_one::<PersistentVolumeClaim>(client, ns, name).await,
        other => Err(Error::Refused(format!("{other} cannot be deleted here"))),
    }
}

/// Delete one namespaced object of a known type.
async fn delete_one<K>(client: &Client, ns: &str, name: &str) -> Result<()>
where
    K: kube::Resource<Scope = k8s_openapi::NamespaceResourceScope>
        + Clone
        + std::fmt::Debug
        + serde::de::DeserializeOwned,
    <K as kube::Resource>::DynamicType: Default,
{
    let api: Api<K> = scoped(client, ns);
    // The API answers either the object or a Status, and which it is
    // says only whether the delete finished or was accepted - both mean
    // "taken", and the pane re-reads either way.
    api.delete(name, &kube::api::DeleteParams::default())
        .await
        .map_err(classify)?;
    Ok(())
}

/// A custom resource this cluster defines (phase 7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CrdRow {
    /// `widgets.example.com`, which is what a reader recognises.
    pub name: String,
    pub group: String,
    /// The version the cluster serves as storage.
    pub version: String,
    pub kind: String,
    pub plural: String,
    pub namespaced: bool,
    pub age: String,
}

/// One object of a custom resource, as the list shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CustomRow {
    pub name: String,
    pub namespace: String,
    pub age: String,
}

/// The custom resources this cluster defines.
///
/// Read from the definitions rather than from a full discovery sweep:
/// discovery walks **every** group including the built-ins, which is a
/// dozen round trips to answer a question about the handful of kinds
/// somebody installed. A cluster with no CRDs answers with an empty
/// list rather than an error.
pub async fn crds(client: &Client) -> Result<Vec<CrdRow>> {
    use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
    let api: Api<CustomResourceDefinition> = Api::all(client.clone());
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<CrdRow> = list
        .iter()
        .map(|c| {
            // The storage version is the one to list objects at: a CRD
            // may serve several, and only one is what the cluster keeps.
            let version = c
                .spec
                .versions
                .iter()
                .find(|v| v.storage)
                .or_else(|| c.spec.versions.first())
                .map(|v| v.name.clone())
                .unwrap_or_default();
            CrdRow {
                name: c.name_any(),
                group: c.spec.group.clone(),
                version,
                kind: c.spec.names.kind.clone(),
                plural: c.spec.names.plural.clone(),
                namespaced: c.spec.scope == "Namespaced",
                age: age_of(c.metadata.creation_timestamp.as_ref()),
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// The objects of one custom resource.
///
/// Name, namespace and age only: a custom resource's own fields are
/// whatever its author chose, so there is no column set this can know -
/// the YAML view is where the content is read.
pub async fn custom(client: &Client, crd: &CrdRow, ns: &str) -> Result<Vec<CustomRow>> {
    let resource = kube::api::ApiResource::from_gvk(&kube::api::GroupVersionKind::gvk(
        &crd.group,
        &crd.version,
        &crd.kind,
    ));
    let api: Api<kube::api::DynamicObject> = if crd.namespaced && !ns.trim().is_empty() {
        Api::namespaced_with(client.clone(), ns.trim(), &resource)
    } else {
        Api::all_with(client.clone(), &resource)
    };
    let list = api.list(&ListParams::default()).await.map_err(classify)?;
    let mut out: Vec<CustomRow> = list
        .iter()
        .map(|o| CustomRow {
            name: o.name_any(),
            namespace: o.namespace().unwrap_or_default(),
            age: age_of(o.metadata.creation_timestamp.as_ref()),
        })
        .collect();
    out.sort_by(|a, b| a.namespace.cmp(&b.namespace).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// What an apply would do, and then what it did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// `<kind>/<name>` as the server names it back.
    pub what: String,
    /// The object as it is now, empty when it does not exist yet.
    pub before: String,
    /// The object as the server says it would be.
    pub after: String,
    /// Whether this was the dry run or the real thing.
    pub dry_run: bool,
}

impl Applied {
    /// Whether the apply would change anything at all.
    pub fn changes(&self) -> bool {
        self.before != self.after
    }

    /// Whether the object is being created rather than changed.
    pub fn creates(&self) -> bool {
        self.before.is_empty()
    }
}

/// Apply a manifest, through **server-side apply**.
///
/// Dry run first is the spec's rule (§5, §10.1) and the reason this
/// returns `before` beside `after`: a diff the server itself computed
/// beats one this app guesses at, because defaulting, admission
/// webhooks and other controllers' fields all land in the answer. The
/// field manager is `hyperstack`, so what this app owns is recorded and
/// a later apply from `kubectl` conflicts loudly rather than silently
/// reverting someone's work.
///
/// One document, not a stream: a manifest with several objects is split
/// by the caller, so a failure names which document failed instead of
/// leaving the rest in doubt.
pub async fn apply(
    client: &Client,
    yaml: &str,
    default_ns: &str,
    dry_run: bool,
) -> Result<Applied> {
    let object: kube::api::DynamicObject = serde_norway::from_str(yaml)
        .map_err(|e| Error::Config(format!("this is not a Kubernetes manifest: {e}")))?;
    let types = object
        .types
        .clone()
        .ok_or_else(|| Error::Config("the manifest has no apiVersion and kind".into()))?;
    let gvk = kube::api::GroupVersionKind::try_from(&types)
        .map_err(|e| Error::Config(format!("{} is not a known kind: {e}", types.kind)))?;
    // Discovery rather than a hard-coded table: this is the path a CRD
    // reaches the cluster by, and a built-in kind costs the same lookup.
    let (resource, caps) = kube::discovery::pinned_kind(client, &gvk)
        .await
        .map_err(classify)?;
    let name = object
        .metadata
        .name
        .clone()
        .ok_or_else(|| Error::Config("the manifest has no metadata.name".into()))?;
    let namespaced = caps.scope == kube::discovery::Scope::Namespaced;
    let ns = object
        .metadata
        .namespace
        .clone()
        .unwrap_or_else(|| default_ns.trim().to_string());
    let api: Api<kube::api::DynamicObject> = if !namespaced {
        Api::all_with(client.clone(), &resource)
    } else if ns.is_empty() {
        return Err(Error::Config(format!(
            "{} is namespaced and the manifest names no namespace",
            types.kind
        )));
    } else {
        Api::namespaced_with(client.clone(), &ns, &resource)
    };

    // "Not there yet" is a normal answer here, not a failure: it is
    // what makes this apply a creation.
    let before = match api.get_opt(&name).await.map_err(classify)? {
        Some(current) => render(current)?,
        None => String::new(),
    };
    let mut params = kube::api::PatchParams::apply("hyperstack");
    if dry_run {
        params = params.dry_run();
    }
    let result = api
        .patch(&name, &params, &kube::api::Patch::Apply(&object))
        .await
        .map_err(classify)?;
    Ok(Applied {
        what: format!("{}/{name}", types.kind),
        before,
        after: render(result)?,
        dry_run,
    })
}

/// Split a manifest into its documents, dropping the empty ones.
///
/// A file of several objects separated by `---` is the normal shape of
/// a manifest, and applying it as one document fails with a parse error
/// that says nothing useful.
pub fn documents(yaml: &str) -> Vec<String> {
    yaml.split("\n---")
        .map(|d| d.trim_start_matches("---").trim().to_string())
        .filter(|d| {
            !d.is_empty()
                && d.lines()
                    .any(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        })
        .collect()
}

/// A port forward the pane is holding open (phase 7).
///
/// **Not** a `Tunnels` forward: that service is SSH-shaped - its
/// `ForwardSpec` carries an `ssh_id` and its implementation is
/// `hs_ssh::tunnels::TunnelManager` - and a Kubernetes forward goes
/// through the API server's `portforward` subresource instead. Putting
/// one in there would mean inventing an SSH connection that does not
/// exist (`docs/tools/containers.md` §10.6).
pub struct Forward {
    /// The port on this machine, known once the listener is bound.
    pub local_port: u16,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Forward {
    /// Stop accepting and drop the connections.
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        !self.stop.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Closing the tab closes its forwards.
///
/// The listener lives in a spawned task that only ends when the flag is
/// set, so without this a closed tab would leave a port on this machine
/// listening - and forwarding - for the rest of the session, with
/// nothing on screen to stop it.
impl Drop for Forward {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Forward `127.0.0.1:<local>` to `<pod>:<remote>` through the API
/// server.
///
/// The listener is bound **before** this returns, so the caller is told
/// the real port (0 asks the OS for a free one) and a failure to bind
/// is reported where the user asked for it rather than later in a task
/// nobody is watching. Each accepted connection opens its own
/// portforward stream: the API multiplexes them and a browser opens
/// several at once.
pub async fn forward(
    client: &Client,
    ns: &str,
    pod: &str,
    remote_port: u16,
    local_port: u16,
) -> Result<Forward> {
    use tokio::io::AsyncWriteExt;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", local_port))
        .await
        .map_err(|e| Error::Refused(format!("could not listen on port {local_port}: {e}")))?;
    let bound = listener
        .local_addr()
        .map_err(|e| Error::Refused(e.to_string()))?
        .port();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let api: Api<Pod> = scoped(client, ns);
    let (pod, flag) = (pod.to_string(), stop.clone());
    tokio::spawn(async move {
        loop {
            if flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            // A timeout rather than a bare accept: a forward nobody
            // connects to must still notice that it has been stopped.
            let accepted =
                tokio::time::timeout(Duration::from_millis(500), listener.accept()).await;
            let Ok(accepted) = accepted else { continue };
            let Ok((mut socket, _)) = accepted else { break };
            let (api, pod, flag) = (api.clone(), pod.clone(), flag.clone());
            tokio::spawn(async move {
                let mut pf = match api.portforward(&pod, &[remote_port]).await {
                    Ok(pf) => pf,
                    Err(e) => {
                        tracing::warn!("port forward to {pod}:{remote_port} failed: {e}");
                        let _ = socket.shutdown().await;
                        return;
                    }
                };
                let Some(mut upstream) = pf.take_stream(remote_port) else {
                    let _ = socket.shutdown().await;
                    return;
                };
                // Both directions until either end closes, or until the
                // forward is stopped from the pane.
                let copy = tokio::io::copy_bidirectional(&mut socket, &mut upstream);
                tokio::select! {
                    _ = copy => {}
                    _ = async {
                        while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                            tokio::time::sleep(Duration::from_millis(250)).await;
                        }
                    } => {}
                }
                let _ = socket.shutdown().await;
                let _ = pf.join().await;
            });
        }
    });
    Ok(Forward {
        local_port: bound,
        stop,
    })
}

/// Scoped to one namespace, or across all of them when `ns` is empty.
fn scoped<K>(client: &Client, ns: &str) -> Api<K>
where
    K: kube::Resource<Scope = k8s_openapi::NamespaceResourceScope>,
    <K as kube::Resource>::DynamicType: Default,
{
    if ns.trim().is_empty() {
        Api::all(client.clone())
    } else {
        Api::namespaced(client.clone(), ns.trim())
    }
}

/// The container ports a pod declares, de-duplicated and in order.
///
/// Init containers are left out: they have finished by the time anyone
/// forwards a port, and their ports are never what is wanted.
fn pod_ports(pod: &Pod) -> Vec<u16> {
    let mut out: Vec<u16> = pod
        .spec
        .iter()
        .flat_map(|s| s.containers.iter())
        .flat_map(|c| c.ports.iter().flatten())
        .map(|p| p.container_port as u16)
        .collect();
    out.dedup();
    out
}

/// Every image a pod template runs, init containers included: "which
/// image is this" is the second question after "is it up", and an init
/// container that cannot pull keeps the whole pod down.
fn images_of(spec: Option<&k8s_openapi::api::core::v1::PodSpec>) -> Vec<String> {
    let Some(spec) = spec else {
        return Vec::new();
    };
    let mut out: Vec<String> = spec
        .init_containers
        .iter()
        .flatten()
        .chain(spec.containers.iter())
        .filter_map(|c| c.image.clone())
        .collect();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateTerminated, ContainerStateWaiting, ContainerStatus, PodSpec,
        PodStatus,
    };

    fn container_status(
        name: &str,
        ready: bool,
        restarts: i32,
        state: ContainerState,
    ) -> ContainerStatus {
        ContainerStatus {
            name: name.into(),
            ready,
            restart_count: restarts,
            state: Some(state),
            ..Default::default()
        }
    }

    fn pod_with(phase: &str, statuses: Vec<ContainerStatus>, containers: usize) -> Pod {
        Pod {
            spec: Some(PodSpec {
                containers: (0..containers)
                    .map(|i| k8s_openapi::api::core::v1::Container {
                        name: format!("c{i}"),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
            status: Some(PodStatus {
                phase: Some(phase.into()),
                container_statuses: Some(statuses),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// **The whole reason this derivation exists.** Kubernetes reports
    /// the phase of a crash-looping pod as `Running`; the sentence a
    /// reader needs is the container's waiting reason. Showing
    /// `Running` over a CrashLoopBackOff is the single most misleading
    /// thing this column could say.
    #[test]
    fn a_waiting_reason_beats_the_phase() {
        let pod = pod_with(
            "Running",
            vec![container_status(
                "app",
                false,
                7,
                ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("CrashLoopBackOff".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )],
            1,
        );
        let (status, trouble) = pod_status(&pod);
        assert_eq!(status, "CrashLoopBackOff");
        assert!(trouble);
        assert_eq!(pod_restarts(&pod), 7);
        assert_eq!(pod_ready(&pod), "0/1");
    }

    /// The bench's broken deployment: an image that does not exist.
    #[test]
    fn an_image_that_will_not_pull_says_so() {
        let pod = pod_with(
            "Pending",
            vec![container_status(
                "nope",
                false,
                0,
                ContainerState {
                    waiting: Some(ContainerStateWaiting {
                        reason: Some("ErrImagePull".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )],
            1,
        );
        assert_eq!(pod_status(&pod).0, "ErrImagePull");
    }

    /// `ContainerCreating` and `PodInitializing` are **normal**: a pod
    /// that has just been scheduled is not in trouble, and reporting
    /// those as the status would make every new pod look broken.
    #[test]
    fn the_ordinary_waiting_reasons_do_not_mask_the_phase() {
        for reason in ["ContainerCreating", "PodInitializing"] {
            let pod = pod_with(
                "Pending",
                vec![container_status(
                    "app",
                    false,
                    0,
                    ContainerState {
                        waiting: Some(ContainerStateWaiting {
                            reason: Some(reason.into()),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                )],
                1,
            );
            assert_eq!(pod_status(&pod).0, "Pending", "{reason} is not a status");
        }
    }

    /// A finished job's pod is `Completed`, which is not trouble; any
    /// other terminated reason is.
    #[test]
    fn a_completed_container_is_not_trouble_but_a_failed_one_is() {
        let done = pod_with(
            "Succeeded",
            vec![container_status(
                "report",
                false,
                0,
                ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        reason: Some("Completed".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )],
            1,
        );
        let (status, trouble) = pod_status(&done);
        assert_eq!(status, "Succeeded");
        assert!(!trouble);

        let oom = pod_with(
            "Running",
            vec![container_status(
                "app",
                false,
                1,
                ContainerState {
                    terminated: Some(ContainerStateTerminated {
                        reason: Some("OOMKilled".into()),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            )],
            1,
        );
        assert_eq!(oom.clone().status.unwrap().phase.unwrap(), "Running");
        assert_eq!(pod_status(&oom), ("OOMKilled".into(), true));
    }

    /// A pod being deleted is `Terminating` whatever its containers
    /// say - it is on its way out and the list should show that.
    #[test]
    fn a_deleting_pod_is_terminating() {
        let mut pod = pod_with(
            "Running",
            vec![container_status("a", true, 0, ContainerState::default())],
            1,
        );
        pod.metadata.deletion_timestamp =
            Some(k8s_openapi::apimachinery::pkg::apis::meta::v1::Time(
                k8s_openapi::jiff::Timestamp::now(),
            ));
        assert_eq!(pod_status(&pod), ("Terminating".into(), true));
    }

    /// Ready counts containers, not pods: `1/2` is the case that
    /// matters and the one a naive implementation gets wrong.
    #[test]
    fn ready_counts_containers_out_of_the_spec() {
        let pod = pod_with(
            "Running",
            vec![
                container_status("a", true, 0, ContainerState::default()),
                container_status("b", false, 0, ContainerState::default()),
            ],
            2,
        );
        assert_eq!(pod_ready(&pod), "1/2");
        // A pod whose statuses have not arrived yet still knows how
        // many containers it wants.
        let fresh = Pod {
            spec: pod.spec.clone(),
            status: Some(PodStatus::default()),
            ..Default::default()
        };
        assert_eq!(pod_ready(&fresh), "0/2");
    }

    /// The short form every Kubernetes tool uses, and never "0s".
    #[test]
    fn the_age_reads_the_way_kubectl_writes_it() {
        assert_eq!(short_age(1), "1s");
        assert_eq!(short_age(47), "47s");
        assert_eq!(short_age(60), "1m");
        assert_eq!(short_age(95), "1m35s");
        assert_eq!(short_age(3600), "1h");
        assert_eq!(short_age(3600 + 12 * 60), "1h12m");
        assert_eq!(short_age(86_400), "1d");
        assert_eq!(short_age(86_400 * 3 + 3600 * 5), "3d5h");
        // Past a day the minutes are noise.
        assert_eq!(short_age(86_400 * 2 + 61), "2d");
    }

    #[test]
    fn node_roles_come_from_the_labels() {
        let mut labels = BTreeMap::new();
        labels.insert(
            "node-role.kubernetes.io/control-plane".into(),
            "true".into(),
        );
        labels.insert("node-role.kubernetes.io/master".into(), "true".into());
        labels.insert("kubernetes.io/hostname".into(), "n1".into());
        assert_eq!(node_roles(&labels), "control-plane,master");
        assert_eq!(node_roles(&BTreeMap::new()), "<none>");
    }

    /// A node port is the number someone needs to reach the service
    /// from outside, so it has to be in the column.
    #[test]
    fn service_ports_show_the_node_port_when_there_is_one() {
        use k8s_openapi::api::core::v1::{ServicePort, ServiceSpec};
        let svc = Service {
            spec: Some(ServiceSpec {
                ports: Some(vec![
                    ServicePort {
                        port: 80,
                        node_port: Some(30080),
                        protocol: Some("TCP".into()),
                        ..Default::default()
                    },
                    ServicePort {
                        port: 443,
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(service_ports(&svc), "80:30080/TCP, 443/TCP");
        assert_eq!(service_ports(&Service::default()), "");
    }

    /// An init container that cannot pull keeps the whole pod down, so
    /// its image belongs in the list too.
    #[test]
    fn images_include_the_init_containers() {
        use k8s_openapi::api::core::v1::Container;
        let spec = PodSpec {
            init_containers: Some(vec![Container {
                name: "wait".into(),
                image: Some("busybox:stable".into()),
                ..Default::default()
            }]),
            containers: vec![Container {
                name: "app".into(),
                image: Some("nginx:alpine".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(images_of(Some(&spec)), ["busybox:stable", "nginx:alpine"]);
        assert!(images_of(None).is_empty());
    }

    /// A secret's YAML is the one view in this pane that could put
    /// credentials on the screen. The keys survive, the values do not -
    /// and `data` is base64, which looks encrypted and is not.
    #[test]
    fn a_secrets_values_never_reach_the_yaml_view() {
        let secret = Secret {
            data: Some(BTreeMap::from([(
                "password".to_string(),
                k8s_openapi::ByteString(b"hunter2".to_vec()),
            )])),
            string_data: Some(BTreeMap::from([(
                "token".to_string(),
                "s3cret".to_string(),
            )])),
            // What `kubectl apply` leaves behind: the applied object in
            // full, values included. Redacting `data` and leaving this
            // is no redaction at all (seen on the k3s bench).
            metadata: kube::api::ObjectMeta {
                annotations: Some(BTreeMap::from([(
                    LAST_APPLIED.to_string(),
                    r#"{"kind":"Secret","stringData":{"token":"s3cret"}}"#.to_string(),
                )])),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = render(redact(secret)).expect("renders");
        assert!(out.contains("password"), "the keys are the useful part");
        assert!(out.contains("token"));
        assert!(!out.contains("hunter2"), "{out}");
        assert!(!out.contains("s3cret"), "{out}");
        // Base64 of "hunter2", which is how `data` would be serialised
        // had the value survived.
        assert!(!out.contains("aHVudGVyMg=="), "{out}");
        assert!(!out.contains("last-applied-configuration"), "{out}");
    }

    /// The applied-configuration annotation is dropped for every kind,
    /// not only for secrets: one rule has no hole for the next kind
    /// that turns out to carry something sensitive in it.
    #[test]
    fn the_applied_configuration_annotation_is_dropped_for_every_kind() {
        let pod = Pod {
            metadata: kube::api::ObjectMeta {
                name: Some("web".into()),
                annotations: Some(BTreeMap::from([
                    (LAST_APPLIED.to_string(), "{\"kind\":\"Pod\"}".to_string()),
                    ("team".to_string(), "shop".to_string()),
                ])),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = render(pod).expect("renders");
        assert!(!out.contains("last-applied-configuration"), "{out}");
        assert!(out.contains("team: shop"), "the others stay: {out}");
    }

    /// A manifest of several objects is the normal shape, and applying
    /// it as one document fails with a parse error that says nothing.
    #[test]
    fn a_manifest_is_split_into_its_documents() {
        let manifest = "\
apiVersion: v1
kind: Service
metadata:
  name: web
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: web
";
        let docs = documents(manifest);
        assert_eq!(docs.len(), 2);
        assert!(docs[0].starts_with("apiVersion: v1"));
        assert!(docs[1].contains("kind: Deployment"));
        assert!(!docs[1].starts_with("---"), "the separator is not content");
    }

    /// A leading separator, a trailing one and a comment-only document
    /// are all things `kubectl` accepts and none of them is an object.
    #[test]
    fn empty_and_comment_only_documents_are_not_applied() {
        let manifest = "\
---
# nothing here yet
---
apiVersion: v1
kind: ConfigMap
metadata:
  name: shop
---
";
        let docs = documents(manifest);
        assert_eq!(docs.len(), 1, "{docs:?}");
        assert!(docs[0].contains("ConfigMap"));
        assert!(documents("").is_empty());
        assert!(documents("\n\n---\n\n").is_empty());
    }

    /// `changes` and `creates` are what the preview's wording turns on,
    /// and "nothing to do" must not read as "created".
    #[test]
    fn an_apply_knows_whether_it_creates_changes_or_does_nothing() {
        let created = Applied {
            before: String::new(),
            after: "a: 1".into(),
            ..Default::default()
        };
        assert!(created.creates() && created.changes());

        let changed = Applied {
            before: "a: 1".into(),
            after: "a: 2".into(),
            ..Default::default()
        };
        assert!(!changed.creates() && changed.changes());

        let same = Applied {
            before: "a: 1".into(),
            after: "a: 1".into(),
            ..Default::default()
        };
        assert!(!same.creates() && !same.changes());
    }

    /// `managedFields` is server-side-apply bookkeeping, routinely
    /// longer than the spec it describes and never the reason anybody
    /// opened the view.
    #[test]
    fn the_yaml_view_drops_the_managed_fields() {
        use k8s_openapi::apimachinery::pkg::apis::meta::v1::ManagedFieldsEntry;
        let mut pod = Pod {
            metadata: kube::api::ObjectMeta {
                name: Some("web".into()),
                managed_fields: Some(vec![ManagedFieldsEntry {
                    manager: Some("kubectl-client-side-apply".into()),
                    ..Default::default()
                }]),
                ..Default::default()
            },
            ..Default::default()
        };
        let out = render(std::mem::take(&mut pod)).expect("renders");
        assert!(out.contains("name: web"));
        assert!(!out.contains("managedFields"), "{out}");
        assert!(!out.contains("kubectl-client-side-apply"), "{out}");
    }
}
