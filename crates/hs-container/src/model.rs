//! What the panes show: plain data, parsed from whatever the engine
//! said (`docs/tools/containers.md` §4).
//!
//! Docker and Podman answer the same questions in different shapes, and
//! neither is as tidy as its documentation suggests:
//!
//! - `docker ps --format '{{json .}}'` is **line-delimited** JSON, and
//!   its `Labels`, `Ports` and `Mounts` are *human strings* -
//!   `"a=1,b=2"`, `"8000/tcp, 0.0.0.0:9443->9443/tcp"` - not arrays.
//! - `podman ps --format json` is a **JSON array**, `Id` rather than
//!   `ID`, `Names` is an array, `Labels` an object and `Ports` an array
//!   of objects.
//!
//! So the parsers here are deliberately tolerant: either container
//! shape, either spelling, either representation of a list. That also
//! buys tolerance of version drift, which is the other thing that
//! breaks a parser written against one machine's output.

use serde::{Deserialize, Serialize};

/// What a container is doing. The engine's own word, normalised, because
/// a filter and a colour should mean the same thing on both engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Created,
    Running,
    Paused,
    Restarting,
    Removing,
    Exited,
    Dead,
    #[default]
    Unknown,
}

impl State {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "created" | "configured" => State::Created,
            "running" | "up" => State::Running,
            "paused" => State::Paused,
            "restarting" => State::Restarting,
            "removing" => State::Removing,
            "exited" | "stopped" => State::Exited,
            "dead" => State::Dead,
            _ => State::Unknown,
        }
    }

    pub fn is_running(self) -> bool {
        matches!(self, State::Running | State::Restarting)
    }

    /// Worth drawing attention to: a container that died or is stuck.
    /// `Exited` is included because an exited container in a list of
    /// running ones is usually the reason someone opened the pane.
    pub fn is_trouble(self) -> bool {
        matches!(self, State::Dead | State::Exited)
    }

    pub fn key(self) -> &'static str {
        match self {
            State::Created => "container-state-created",
            State::Running => "container-state-running",
            State::Paused => "container-state-paused",
            State::Restarting => "container-state-restarting",
            State::Removing => "container-state-removing",
            State::Exited => "container-state-exited",
            State::Dead => "container-state-dead",
            State::Unknown => "container-state-unknown",
        }
    }
}

/// A container's health, when it declares a healthcheck at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// No healthcheck declared, which is not the same as healthy.
    #[default]
    None,
    Starting,
    Healthy,
    Unhealthy,
}

impl Health {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "starting" => Health::Starting,
            "healthy" => Health::Healthy,
            "unhealthy" => Health::Unhealthy,
            _ => Health::None,
        }
    }
}

/// One published port.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Port {
    /// The port inside the container.
    pub container: u16,
    /// The port on the host, when it is published at all.
    pub host: Option<u16>,
    /// The host address it is published on, when there is one.
    pub host_ip: String,
    /// `tcp`, `udp`, `sctp`.
    pub protocol: String,
}

impl Port {
    /// `0.0.0.0:9443->9443/tcp` as it reads in the pane, or
    /// `8000/tcp` for a port that is exposed but not published.
    pub fn describe(&self) -> String {
        match self.host {
            Some(h) if !self.host_ip.is_empty() => {
                format!("{}:{h}->{}/{}", self.host_ip, self.container, self.protocol)
            }
            Some(h) => format!("{h}->{}/{}", self.container, self.protocol),
            None => format!("{}/{}", self.container, self.protocol),
        }
    }
}

/// One container, as a list row needs it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Container {
    pub id: String,
    /// The first name, which is the one people use. The engine can
    /// report several.
    pub name: String,
    pub names: Vec<String>,
    pub image: String,
    pub state: State,
    /// The engine's own sentence ("Up 13 hours"), which says more than
    /// the state alone and is worth showing verbatim.
    pub status: String,
    pub health: Health,
    pub ports: Vec<Port>,
    pub labels: Vec<(String, String)>,
    pub created: String,
    pub command: String,
}

impl Container {
    /// The Compose project this belongs to, from the label Compose sets.
    /// Stacks in the pane are grouped by this.
    pub fn compose_project(&self) -> Option<&str> {
        self.label("com.docker.compose.project")
    }

    pub fn compose_service(&self) -> Option<&str> {
        self.label("com.docker.compose.service")
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A short id, as every container tool shows it.
    pub fn short_id(&self) -> &str {
        let n = self.id.len().min(12);
        &self.id[..n]
    }
}

/// One image.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Image {
    pub id: String,
    pub repository: String,
    pub tag: String,
    /// The engine's own size string; bytes are not always reported.
    pub size: String,
    pub created: String,
}

impl Image {
    /// `repo:tag`, or `<none>` for a dangling layer.
    pub fn reference(&self) -> String {
        if self.repository.is_empty() || self.repository == "<none>" {
            return "<none>".into();
        }
        if self.tag.is_empty() || self.tag == "<none>" {
            return self.repository.clone();
        }
        format!("{}:{}", self.repository, self.tag)
    }

    pub fn dangling(&self) -> bool {
        self.reference() == "<none>"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Volume {
    pub name: String,
    pub driver: String,
    pub mountpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Network {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub scope: String,
}

/// What a host adds up to, for the dashboard.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    pub running: usize,
    pub stopped: usize,
    pub unhealthy: usize,
    pub images: usize,
    pub volumes: usize,
    pub networks: usize,
}

impl Summary {
    pub fn of(containers: &[Container]) -> Self {
        Self {
            running: containers.iter().filter(|c| c.state.is_running()).count(),
            stopped: containers.iter().filter(|c| !c.state.is_running()).count(),
            unhealthy: containers
                .iter()
                .filter(|c| c.health == Health::Unhealthy)
                .count(),
            ..Default::default()
        }
    }
}

// --- parsing ---------------------------------------------------------

/// Read whatever the engine printed into values.
///
/// Accepts both container shapes: a JSON array (Podman), or
/// line-delimited objects (Docker). A line that cannot be parsed is
/// skipped rather than failing the whole list - one odd container must
/// not hide the other twenty.
fn records(text: &str) -> Vec<serde_json::Value> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('[') {
        return serde_json::from_str::<Vec<serde_json::Value>>(trimmed).unwrap_or_default();
    }
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// A string field under any of several spellings, because the two
/// engines disagree about capitalisation (`ID` and `Id`).
fn text_of(v: &serde_json::Value, keys: &[&str]) -> String {
    for k in keys {
        if let Some(s) = v.get(k).and_then(|x| x.as_str()) {
            return s.to_string();
        }
        // Podman reports some numbers where Docker reports strings.
        if let Some(n) = v.get(k).and_then(|x| x.as_i64()) {
            return n.to_string();
        }
    }
    String::new()
}

/// A list of names, which Docker gives as a comma-separated string and
/// Podman as an array.
fn names_of(v: &serde_json::Value) -> Vec<String> {
    for k in ["Names", "Name"] {
        match v.get(k) {
            Some(serde_json::Value::Array(a)) => {
                return a
                    .iter()
                    .filter_map(|x| x.as_str())
                    .map(|s| s.trim_start_matches('/').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            Some(serde_json::Value::String(s)) => {
                return s
                    .split(',')
                    .map(|p| p.trim().trim_start_matches('/'))
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            _ => {}
        }
    }
    Vec::new()
}

/// Labels: Docker gives `"a=1,b=2"`, Podman gives an object.
fn labels_of(v: &serde_json::Value) -> Vec<(String, String)> {
    match v.get("Labels") {
        Some(serde_json::Value::Object(map)) => map
            .iter()
            .map(|(k, val)| {
                (
                    k.clone(),
                    val.as_str().map(str::to_string).unwrap_or_default(),
                )
            })
            .collect(),
        Some(serde_json::Value::String(s)) => s
            .split(',')
            .filter_map(|pair| {
                let pair = pair.trim();
                if pair.is_empty() {
                    return None;
                }
                // Only the first `=` splits: a label's value very often
                // contains more of them (a URL, a JSON blob).
                match pair.split_once('=') {
                    Some((k, val)) => Some((k.trim().to_string(), val.to_string())),
                    None => Some((pair.to_string(), String::new())),
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Ports. Docker gives one human string; Podman an array of objects.
fn ports_of(v: &serde_json::Value) -> Vec<Port> {
    match v.get("Ports") {
        Some(serde_json::Value::String(s)) => parse_port_string(s),
        Some(serde_json::Value::Array(a)) => a
            .iter()
            .map(|p| Port {
                container: p
                    .get("container_port")
                    .or_else(|| p.get("containerPort"))
                    .and_then(|x| x.as_u64())
                    .and_then(|n| u16::try_from(n).ok())
                    .unwrap_or(0),
                host: p
                    .get("host_port")
                    .or_else(|| p.get("hostPort"))
                    .and_then(|x| x.as_u64())
                    .and_then(|n| u16::try_from(n).ok())
                    .filter(|n| *n != 0),
                host_ip: p
                    .get("host_ip")
                    .or_else(|| p.get("hostIP"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                protocol: p
                    .get("protocol")
                    .and_then(|x| x.as_str())
                    .unwrap_or("tcp")
                    .to_string(),
            })
            .filter(|p| p.container != 0)
            .collect(),
        _ => Vec::new(),
    }
}

/// `"8000/tcp, 0.0.0.0:9443->9443/tcp, [::]:9443->9443/tcp"`.
///
/// The IPv6 form has colons inside brackets, so the address cannot be
/// found by splitting on `:` - the bracket has to be respected or every
/// IPv6 mapping is mangled.
pub fn parse_port_string(s: &str) -> Vec<Port> {
    let mut out = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (left, right) = match part.split_once("->") {
            Some((l, r)) => (Some(l.trim()), r.trim()),
            None => (None, part),
        };
        // `9443/tcp`
        let (port_text, protocol) = match right.rsplit_once('/') {
            Some((p, proto)) => (p, proto.to_string()),
            None => (right, "tcp".to_string()),
        };
        let Ok(container) = port_text.trim().parse::<u16>() else {
            continue;
        };
        let (host_ip, host) = match left {
            None => (String::new(), None),
            Some(l) => {
                // `[::]:9443`, `0.0.0.0:9443`, or a bare port.
                let (addr, port) = if let Some(rest) = l.strip_prefix('[') {
                    match rest.split_once("]:") {
                        Some((a, p)) => (format!("[{a}]"), p),
                        None => (String::new(), l),
                    }
                } else {
                    match l.rsplit_once(':') {
                        Some((a, p)) => (a.to_string(), p),
                        None => (String::new(), l),
                    }
                };
                (addr, port.trim().parse::<u16>().ok())
            }
        };
        out.push(Port {
            container,
            host,
            host_ip,
            protocol,
        });
    }
    out
}

/// Containers from `docker ps --format '{{json .}}'` or
/// `podman ps --format json`.
pub fn parse_containers(text: &str) -> Vec<Container> {
    records(text)
        .iter()
        .map(|v| {
            let names = names_of(v);
            let state_text = text_of(v, &["State", "Status"]);
            Container {
                id: text_of(v, &["ID", "Id", "ContainerID"]),
                name: names.first().cloned().unwrap_or_default(),
                names,
                image: text_of(v, &["Image"]),
                // Docker's `State` is the bare word; Podman's too. When
                // only `Status` is there ("Up 13 hours"), its first word
                // is the state.
                state: State::parse(state_text.split_whitespace().next().unwrap_or(&state_text)),
                status: text_of(v, &["Status"]),
                health: Health::parse(&text_of(v, &["HealthStatus", "Health"])),
                ports: ports_of(v),
                labels: labels_of(v),
                created: text_of(v, &["CreatedAt", "Created"]),
                command: text_of(v, &["Command"]),
            }
        })
        .filter(|c| !c.id.is_empty())
        .collect()
}

/// Images from `docker images --format '{{json .}}'` or
/// `podman images --format json`.
pub fn parse_images(text: &str) -> Vec<Image> {
    records(text)
        .iter()
        .flat_map(|v| {
            let id = text_of(v, &["ID", "Id"]);
            let size = text_of(v, &["Size", "VirtualSize"]);
            let created = text_of(v, &["CreatedAt", "CreatedSince", "Created"]);
            // Podman reports every tag of one image in a `Names` array,
            // which is one image per tag to a reader.
            match v.get("Names").and_then(|x| x.as_array()) {
                Some(tags) if !tags.is_empty() => tags
                    .iter()
                    .filter_map(|t| t.as_str())
                    .map(|t| {
                        let (repository, tag) = split_reference(t);
                        Image {
                            id: id.clone(),
                            repository,
                            tag,
                            size: size.clone(),
                            created: created.clone(),
                        }
                    })
                    .collect::<Vec<_>>(),
                _ => vec![Image {
                    id,
                    repository: text_of(v, &["Repository"]),
                    tag: text_of(v, &["Tag"]),
                    size,
                    created,
                }],
            }
        })
        .filter(|i| !i.id.is_empty())
        .collect()
}

/// `registry/repo:tag` into its repository and tag. The registry may
/// carry a port, so the last colon only counts when it comes after the
/// last slash.
fn split_reference(reference: &str) -> (String, String) {
    match reference.rsplit_once(':') {
        Some((repo, tag)) if !tag.contains('/') => (repo.to_string(), tag.to_string()),
        _ => (reference.to_string(), String::new()),
    }
}

pub fn parse_volumes(text: &str) -> Vec<Volume> {
    records(text)
        .iter()
        .map(|v| Volume {
            name: text_of(v, &["Name"]),
            driver: text_of(v, &["Driver"]),
            mountpoint: text_of(v, &["Mountpoint"]),
        })
        .filter(|v| !v.name.is_empty())
        .collect()
}

pub fn parse_networks(text: &str) -> Vec<Network> {
    records(text)
        .iter()
        .map(|v| Network {
            id: text_of(v, &["ID", "Id"]),
            name: text_of(v, &["Name"]),
            driver: text_of(v, &["Driver"]),
            scope: text_of(v, &["Scope"]),
        })
        .filter(|n| !n.name.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the real output of `docker ps --format '{{json .}}'`
    /// on this machine's engine (a Portainer container in WSL). The
    /// label blob is shortened but keeps the shapes that matter: an `=`
    /// inside a value, and a URL.
    const DOCKER_PS: &str = r#"{"Command":"\"/portainer\"","CreatedAt":"2026-05-29 22:35:51 +0800 +08","HealthStatus":"none","ID":"af48259fe8da","Image":"portainer/portainer-ce:lts","Labels":"org.opencontainers.image.title=Portainer CE,io.portainer.server=true,com.docker.extension.additional-urls=[{\"url\": \"https://portainer.io?a=1\"}]","LocalVolumes":"1","Mounts":"portainer_data","Names":"portainer","Networks":"bridge","Platform":null,"Ports":"8000/tcp, 9000/tcp, 0.0.0.0:9443->9443/tcp, [::]:9443->9443/tcp","RunningFor":"4 months ago","Size":"0B (virtual 182MB)","State":"running","Status":"Up 13 hours"}
{"Command":"\"nginx -g\"","CreatedAt":"2026-10-01 09:00:00 +0800 +08","HealthStatus":"unhealthy","ID":"bb11cc22dd33","Image":"nginx:1.27","Labels":"com.docker.compose.project=shop,com.docker.compose.service=web","Names":"shop-web-1","Ports":"","State":"exited","Status":"Exited (1) 2 hours ago"}"#;

    /// `podman ps --format json`: an array, `Id`, `Names` as an array,
    /// `Labels` as an object, `Ports` as objects.
    const PODMAN_PS: &str = r#"[
      {
        "Id": "1111222233334444",
        "Names": ["api"],
        "Image": "docker.io/library/node:22",
        "State": "running",
        "Status": "Up 3 minutes",
        "Created": "2026-10-24T09:00:00Z",
        "Command": ["node", "server.js"],
        "Labels": {"com.docker.compose.project": "shop", "team": "payments"},
        "Ports": [{"host_ip": "0.0.0.0", "host_port": 8080, "container_port": 3000, "protocol": "tcp"}]
      }
    ]"#;

    #[test]
    fn docker_line_delimited_output_is_read() {
        let got = parse_containers(DOCKER_PS);
        assert_eq!(got.len(), 2);
        let p = &got[0];
        assert_eq!(p.id, "af48259fe8da");
        assert_eq!(p.name, "portainer");
        assert_eq!(p.image, "portainer/portainer-ce:lts");
        assert_eq!(p.state, State::Running);
        assert_eq!(p.status, "Up 13 hours");
        assert_eq!(p.health, Health::None, "\"none\" is not healthy");
        assert!(p.state.is_running());
    }

    #[test]
    fn podman_array_output_is_read_despite_every_difference() {
        let got = parse_containers(PODMAN_PS);
        assert_eq!(got.len(), 1);
        let c = &got[0];
        assert_eq!(c.id, "1111222233334444", "`Id`, not `ID`");
        assert_eq!(c.name, "api", "a names array, not a string");
        assert_eq!(c.state, State::Running);
        assert_eq!(
            c.compose_project(),
            Some("shop"),
            "labels as an object, not a string"
        );
        assert_eq!(c.ports.len(), 1);
        assert_eq!(c.ports[0].host, Some(8080));
        assert_eq!(c.ports[0].container, 3000);
    }

    /// The label blob is a comma-separated string whose **values**
    /// contain `=` and commas of their own. Splitting on every `=` would
    /// mangle them.
    #[test]
    fn a_label_value_may_contain_equals_signs() {
        let got = parse_containers(DOCKER_PS);
        let p = &got[0];
        assert_eq!(
            p.label("org.opencontainers.image.title"),
            Some("Portainer CE")
        );
        assert_eq!(p.label("io.portainer.server"), Some("true"));
        let urls = p.label("com.docker.extension.additional-urls").unwrap();
        assert!(urls.contains("a=1"), "the value kept its own = : {urls}");
    }

    /// The IPv6 form has colons inside brackets, so the address cannot
    /// be split off at the last colon.
    #[test]
    fn an_ipv6_port_mapping_is_not_mangled() {
        let ports = parse_port_string("8000/tcp, 0.0.0.0:9443->9443/tcp, [::]:9443->9443/tcp");
        assert_eq!(ports.len(), 3);
        // Exposed but not published.
        assert_eq!(ports[0].container, 8000);
        assert_eq!(ports[0].host, None);
        assert_eq!(ports[0].describe(), "8000/tcp");
        // IPv4.
        assert_eq!(ports[1].host_ip, "0.0.0.0");
        assert_eq!(ports[1].host, Some(9443));
        assert_eq!(ports[1].describe(), "0.0.0.0:9443->9443/tcp");
        // IPv6: the address is the whole bracketed form.
        assert_eq!(ports[2].host_ip, "[::]", "not `[:`");
        assert_eq!(ports[2].host, Some(9443));
        assert_eq!(ports[2].container, 9443);
    }

    #[test]
    fn an_empty_or_odd_port_string_yields_nothing_rather_than_a_wrong_port() {
        assert!(parse_port_string("").is_empty());
        assert!(parse_port_string("   ").is_empty());
        assert!(parse_port_string("not a port").is_empty());
        // A protocol-less port is still a port.
        let p = parse_port_string("5432");
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].protocol, "tcp", "assumed, as the engines do");
    }

    #[test]
    fn compose_grouping_comes_from_the_labels_compose_sets() {
        let got = parse_containers(DOCKER_PS);
        let web = got.iter().find(|c| c.name == "shop-web-1").unwrap();
        assert_eq!(web.compose_project(), Some("shop"));
        assert_eq!(web.compose_service(), Some("web"));
        assert_eq!(got[0].compose_project(), None, "not part of a stack");
    }

    #[test]
    fn an_unhealthy_container_is_recognised_and_counted() {
        let got = parse_containers(DOCKER_PS);
        let web = got.iter().find(|c| c.name == "shop-web-1").unwrap();
        assert_eq!(web.health, Health::Unhealthy);
        assert_eq!(web.state, State::Exited);
        assert!(web.state.is_trouble());
        let s = Summary::of(&got);
        assert_eq!(s.running, 1);
        assert_eq!(s.stopped, 1);
        assert_eq!(s.unhealthy, 1);
    }

    /// One malformed line must not hide the rest of the list.
    #[test]
    fn a_line_that_cannot_be_parsed_is_skipped_not_fatal() {
        let text = format!("{{not json\n{}\n", DOCKER_PS.lines().next().unwrap());
        let got = parse_containers(&text);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "portainer");
    }

    #[test]
    fn nothing_running_is_an_empty_list_not_an_error() {
        assert!(parse_containers("").is_empty());
        assert!(parse_containers("[]").is_empty());
        assert!(parse_containers("\n\n").is_empty());
    }

    /// A name may arrive with the leading slash the API uses.
    #[test]
    fn a_leading_slash_is_stripped_from_a_name() {
        let got = parse_containers(r#"{"Id":"a1","Names":["/api","/api-alias"]}"#);
        assert_eq!(got[0].name, "api");
        assert_eq!(got[0].names, ["api", "api-alias"]);
    }

    #[test]
    fn images_are_read_from_either_engine() {
        let docker = r#"{"ID":"sha256:aaa","Repository":"nginx","Tag":"1.27","Size":"187MB","CreatedAt":"2026-10-01 09:00:00"}
{"ID":"sha256:bbb","Repository":"<none>","Tag":"<none>","Size":"9MB","CreatedAt":"2026-09-01 09:00:00"}"#;
        let got = parse_images(docker);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].reference(), "nginx:1.27");
        assert!(!got[0].dangling());
        assert_eq!(got[1].reference(), "<none>");
        assert!(got[1].dangling(), "a layer with no tag is dangling");

        // Podman reports every tag of one image together.
        let podman = r#"[{"Id":"ccc","Names":["docker.io/library/nginx:1.27","docker.io/library/nginx:latest"],"Size":187000000,"Created":1730000000}]"#;
        let got = parse_images(podman);
        assert_eq!(got.len(), 2, "one row per tag, which is what a reader sees");
        assert_eq!(got[0].reference(), "docker.io/library/nginx:1.27");
        assert_eq!(got[1].tag, "latest");
        assert_eq!(got[0].id, got[1].id, "the same image underneath");
    }

    /// A registry with a port has a colon that is not the tag's.
    #[test]
    fn a_registry_port_is_not_mistaken_for_a_tag() {
        assert_eq!(
            split_reference("registry.example.com:5000/team/app"),
            (
                "registry.example.com:5000/team/app".to_string(),
                String::new()
            )
        );
        assert_eq!(
            split_reference("registry.example.com:5000/team/app:v2"),
            (
                "registry.example.com:5000/team/app".to_string(),
                "v2".to_string()
            )
        );
    }

    #[test]
    fn volumes_and_networks_are_read() {
        let v = parse_volumes(
            r#"{"Name":"portainer_data","Driver":"local","Mountpoint":"/var/lib/docker/volumes/portainer_data/_data"}"#,
        );
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "portainer_data");
        assert_eq!(v[0].driver, "local");

        let n =
            parse_networks(r#"[{"Id":"abc","Name":"bridge","Driver":"bridge","Scope":"local"}]"#);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].name, "bridge");
    }

    #[test]
    fn a_state_word_is_normalised_across_engines() {
        for (text, want) in [
            ("running", State::Running),
            ("Up", State::Running),
            ("exited", State::Exited),
            ("stopped", State::Exited),
            ("configured", State::Created),
            ("paused", State::Paused),
            ("dead", State::Dead),
            ("something new", State::Unknown),
        ] {
            assert_eq!(State::parse(text), want, "{text}");
        }
        assert!(!State::Unknown.is_running());
        assert!(!State::Unknown.is_trouble(), "unknown is not trouble");
    }

    #[test]
    fn a_short_id_is_twelve_characters_and_never_panics() {
        let c = Container {
            id: "af48259fe8da1234".into(),
            ..Default::default()
        };
        assert_eq!(c.short_id(), "af48259fe8da");
        let tiny = Container {
            id: "ab".into(),
            ..Default::default()
        };
        assert_eq!(tiny.short_id(), "ab");
        let empty = Container::default();
        assert_eq!(empty.short_id(), "");
    }
}
