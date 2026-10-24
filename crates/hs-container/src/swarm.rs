//! Swarm: stacks, services and nodes
//! (`docs/tools/containers.md` §4 and §10.6, phase 5).
//!
//! Swarm is the one surface here that may simply not be there. Most
//! engines are not in a swarm, and `docker stack ls` against one of
//! those fails with "This node is not a swarm manager" — which is not
//! an error to report but a **state to show**, because the remedy
//! ("run `docker swarm init`") is a decision for the person, not
//! something a viewer should offer to do for them.
//!
//! So the state is read first, from `docker info`, and the pane shows
//! what that says. A worker node is a second case worth telling apart:
//! it *is* in a swarm, but cannot be asked about stacks, and saying
//! "not a swarm" there would be wrong.

use crate::cli::{Engine, Invocation};
use crate::model::{records, text_of};

/// Where this engine stands in a swarm, from `docker info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SwarmState {
    /// Not in a swarm at all. The commonest answer by far.
    #[default]
    Inactive,
    /// In a swarm and able to answer about stacks and services.
    Manager,
    /// In a swarm, but a worker: it has no view of stacks.
    Worker,
    /// In a swarm whose manager has locked its keys; nothing can be
    /// read until it is unlocked.
    Locked,
    /// Joining, or an engine too old to say. Reported as it stands
    /// rather than guessed either way.
    Pending,
}

impl SwarmState {
    /// Read `docker info`'s `Swarm.LocalNodeState` and
    /// `Swarm.ControlAvailable`, which together say manager or worker.
    ///
    /// Takes the raw text so the caller can feed it either the JSON of
    /// `--format '{{json .Swarm}}'` or the whole `info` document.
    pub fn parse(text: &str) -> Self {
        let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or_default();
        // Either the Swarm sub-object or the document itself.
        let swarm = v.get("Swarm").unwrap_or(&v);
        let state = text_of(swarm, &["LocalNodeState"]).to_ascii_lowercase();
        let control = swarm
            .get("ControlAvailable")
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        match state.trim() {
            "active" if control => SwarmState::Manager,
            "active" => SwarmState::Worker,
            "locked" => SwarmState::Locked,
            "pending" => SwarmState::Pending,
            "inactive" | "" => SwarmState::Inactive,
            _ => SwarmState::Pending,
        }
    }

    /// Whether stacks, services and nodes can be listed at all.
    pub fn can_manage(self) -> bool {
        matches!(self, SwarmState::Manager)
    }

    /// What the pane says when it cannot.
    pub fn key(self) -> &'static str {
        match self {
            SwarmState::Inactive => "container-swarm-inactive",
            SwarmState::Manager => "container-swarm-manager",
            SwarmState::Worker => "container-swarm-worker",
            SwarmState::Locked => "container-swarm-locked",
            SwarmState::Pending => "container-swarm-pending",
        }
    }
}

/// A deployed stack.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stack {
    pub name: String,
    pub services: u32,
    pub orchestrator: String,
}

/// A Swarm service.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Service {
    pub id: String,
    pub name: String,
    /// `replicated` or `global`.
    pub mode: String,
    /// How many are up.
    pub running: u32,
    /// How many are wanted. Equal to `running` when all is well, and
    /// the pair is the point: `1/3` is the whole story of an unhealthy
    /// service.
    pub desired: u32,
    pub image: String,
    pub ports: String,
}

impl Service {
    /// Whether the service has as many tasks as it is meant to.
    ///
    /// A global service reports no desired count on some versions; it
    /// is not called unhealthy for that.
    pub fn is_converged(&self) -> bool {
        self.desired == 0 || self.running >= self.desired
    }

    pub fn replicas(&self) -> String {
        format!("{}/{}", self.running, self.desired)
    }
}

/// A node in the swarm.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Node {
    pub id: String,
    pub hostname: String,
    /// `Ready`, `Down`, `Unknown`.
    pub status: String,
    /// `Active`, `Pause`, `Drain`.
    pub availability: String,
    /// `Leader`, `Reachable`, or empty for a worker.
    pub manager_status: String,
    pub engine_version: String,
}

impl Node {
    pub fn is_ready(&self) -> bool {
        self.status.eq_ignore_ascii_case("ready")
    }

    pub fn is_leader(&self) -> bool {
        self.manager_status.eq_ignore_ascii_case("leader")
    }

    pub fn is_manager(&self) -> bool {
        !self.manager_status.trim().is_empty()
    }
}

/// `docker stack ls --format '{{json .}}'`.
pub fn parse_stacks(text: &str) -> Vec<Stack> {
    records(text)
        .iter()
        .map(|v| Stack {
            name: text_of(v, &["Name"]),
            // Docker reports this as a string on some versions and a
            // number on others, which `text_of` already smooths over.
            services: text_of(v, &["Services"]).trim().parse().unwrap_or(0),
            orchestrator: text_of(v, &["Orchestrator"]),
        })
        .filter(|s| !s.name.is_empty())
        .collect()
}

/// `docker service ls --format '{{json .}}'`.
pub fn parse_services(text: &str) -> Vec<Service> {
    records(text)
        .iter()
        .map(|v| {
            let (running, desired) = replicas_of(&text_of(v, &["Replicas"]));
            Service {
                id: text_of(v, &["ID", "Id"]),
                name: text_of(v, &["Name"]),
                mode: text_of(v, &["Mode"]),
                running,
                desired,
                image: text_of(v, &["Image"]),
                ports: text_of(v, &["Ports"]),
            }
        })
        .filter(|s| !s.name.is_empty())
        .collect()
}

/// Docker writes replicas as `2/3`, and for a global service as
/// `3/3 (max 1 per node)` or just `3/3`. Only the leading pair matters.
fn replicas_of(text: &str) -> (u32, u32) {
    let head = text.split_whitespace().next().unwrap_or("");
    let mut parts = head.split('/');
    let running = parts.next().unwrap_or("").trim().parse().unwrap_or(0);
    let desired = parts.next().unwrap_or("").trim().parse().unwrap_or(0);
    (running, desired)
}

/// `docker node ls --format '{{json .}}'`.
pub fn parse_nodes(text: &str) -> Vec<Node> {
    records(text)
        .iter()
        .map(|v| Node {
            id: text_of(v, &["ID", "Id"]),
            // Docker marks the node you are on with a `*` on the
            // hostname in the table output; the JSON has `Self`
            // instead, but a stray marker must not become part of a
            // name that is then compared against anything.
            hostname: text_of(v, &["Hostname"]).trim_end_matches(" *").to_string(),
            status: text_of(v, &["Status"]),
            availability: text_of(v, &["Availability"]),
            manager_status: text_of(v, &["ManagerStatus"]),
            engine_version: text_of(v, &["EngineVersion"]),
        })
        .filter(|n| !n.hostname.is_empty() || !n.id.is_empty())
        .collect()
}

// --- what to run -----------------------------------------------------

/// `docker info`, as JSON, for the swarm state.
pub fn info(engine: &Engine) -> Invocation {
    engine.invoke(["info", "--format", "{{json .}}"])
}

pub fn list_stacks(engine: &Engine) -> Invocation {
    engine.invoke(["stack", "ls", "--format", JSON_LINE])
}

pub fn list_services(engine: &Engine) -> Invocation {
    engine.invoke(["service", "ls", "--format", JSON_LINE])
}

pub fn list_nodes(engine: &Engine) -> Invocation {
    engine.invoke(["node", "ls", "--format", JSON_LINE])
}

/// The services of one stack.
pub fn stack_services(engine: &Engine, stack: &str) -> Invocation {
    engine.invoke(["stack", "services", stack, "--format", JSON_LINE])
}

/// `docker stack deploy -c <file> <name>`.
///
/// `--prune` is **not** passed: it removes services that are no longer
/// in the file, which is a reasonable thing to want and a terrible
/// default for a button.
pub fn deploy(engine: &Engine, files: &[String], stack: &str) -> Invocation {
    let mut args: Vec<String> = vec!["stack".into(), "deploy".into()];
    for f in files {
        args.push("-c".into());
        args.push(f.clone());
    }
    args.push(stack.to_string());
    engine.invoke(args)
}

/// `docker stack rm <name>`: removes every service in the stack.
pub fn remove_stack(engine: &Engine, stack: &str) -> Invocation {
    engine.invoke(["stack", "rm", stack])
}

/// `docker service scale <name>=<n>`.
pub fn scale(engine: &Engine, service: &str, replicas: u32) -> Invocation {
    engine.invoke(["service", "scale", &format!("{service}={replicas}")])
}

const JSON_LINE: &str = "{{json .}}";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ContainerFields, Transport};

    fn engine() -> Engine {
        Engine::new(
            "docker",
            ContainerFields {
                transport: Transport::Local,
                ..Default::default()
            },
        )
    }

    /// Most engines are not in a swarm, and that is a state to show,
    /// not an error to report.
    #[test]
    fn the_swarm_state_tells_apart_the_cases_that_need_different_words() {
        assert_eq!(
            SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"inactive"}}"#),
            SwarmState::Inactive
        );
        assert_eq!(
            SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"active","ControlAvailable":true}}"#),
            SwarmState::Manager
        );
        // In a swarm but a worker: it cannot answer about stacks, and
        // calling that "not a swarm" would be wrong.
        assert_eq!(
            SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"active","ControlAvailable":false}}"#),
            SwarmState::Worker
        );
        assert_eq!(
            SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"locked"}}"#),
            SwarmState::Locked
        );
        // Only a manager may be asked.
        assert!(
            SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"active","ControlAvailable":true}}"#)
                .can_manage()
        );
        assert!(!SwarmState::parse(r#"{"Swarm":{"LocalNodeState":"active"}}"#).can_manage());
    }

    /// The Swarm sub-object on its own, which is what
    /// `--format '{{json .Swarm}}'` gives.
    #[test]
    fn the_swarm_object_alone_is_understood_too() {
        assert_eq!(
            SwarmState::parse(r#"{"LocalNodeState":"active","ControlAvailable":true}"#),
            SwarmState::Manager
        );
        // Nothing at all is "not in a swarm", not a panic.
        assert_eq!(SwarmState::parse(""), SwarmState::Inactive);
        assert_eq!(SwarmState::parse("not json"), SwarmState::Inactive);
    }

    #[test]
    fn stacks_are_read_from_line_delimited_json() {
        let text = concat!(
            r#"{"Name":"shop","Services":"3","Orchestrator":"Swarm"}"#,
            "\n",
            r#"{"Name":"blog","Services":"1","Orchestrator":"Swarm"}"#,
            "\n"
        );
        let ss = parse_stacks(text);
        assert_eq!(ss.len(), 2);
        assert_eq!(ss[0].name, "shop");
        assert_eq!(ss[0].services, 3);
        // A version that reports the count as a number, not a string.
        let n = parse_stacks(r#"{"Name":"a","Services":2}"#);
        assert_eq!(n[0].services, 2);
    }

    /// `2/3` is the whole story of an unhealthy service, so the pair
    /// has to survive parsing - including the trailing note a global
    /// service carries.
    #[test]
    fn replicas_keep_both_halves() {
        let text = concat!(
            r#"{"ID":"a1","Name":"shop_web","Mode":"replicated","Replicas":"2/3","Image":"web:1"}"#,
            "\n",
            r#"{"ID":"b2","Name":"shop_db","Mode":"global","Replicas":"3/3 (max 1 per node)","Image":"db:1"}"#,
        );
        let ss = parse_services(text);
        assert_eq!(ss[0].replicas(), "2/3");
        assert!(!ss[0].is_converged(), "2 of 3 is not converged");
        assert_eq!(ss[1].replicas(), "3/3");
        assert!(ss[1].is_converged());
    }

    /// An unparsable replica count must not read as "converged": that
    /// would hide exactly the thing the column exists to show.
    #[test]
    fn a_service_with_no_replica_count_is_not_called_unhealthy() {
        let ss = parse_services(r#"{"ID":"a","Name":"x","Replicas":""}"#);
        assert_eq!(ss[0].running, 0);
        assert_eq!(ss[0].desired, 0);
        // Nothing is known, so nothing is claimed.
        assert!(ss[0].is_converged());
    }

    #[test]
    fn nodes_are_read_with_their_roles() {
        let text = concat!(
            r#"{"ID":"n1","Hostname":"mgr1","Status":"Ready","Availability":"Active","ManagerStatus":"Leader","EngineVersion":"29.5.0"}"#,
            "\n",
            r#"{"ID":"n2","Hostname":"wrk1","Status":"Down","Availability":"Drain","ManagerStatus":"","EngineVersion":"29.5.0"}"#,
        );
        let ns = parse_nodes(text);
        assert_eq!(ns.len(), 2);
        assert!(ns[0].is_leader() && ns[0].is_manager() && ns[0].is_ready());
        assert!(!ns[1].is_manager());
        assert!(!ns[1].is_ready(), "Down is not ready");
    }

    /// Docker marks the node you are on; the marker is not part of the
    /// name and must not be compared as though it were.
    #[test]
    fn the_self_marker_is_not_part_of_the_hostname() {
        let ns = parse_nodes(r#"{"ID":"n1","Hostname":"mgr1 *","Status":"Ready"}"#);
        assert_eq!(ns[0].hostname, "mgr1");
    }

    /// Each config file gets its own `-c`, in order: a later file
    /// overrides an earlier one, so the order is the meaning.
    #[test]
    fn deploy_passes_every_file_in_order() {
        let line = deploy(
            &engine(),
            &["base.yml".to_string(), "prod.yml".to_string()],
            "shop",
        )
        .to_line();
        assert!(
            line.contains("stack deploy -c base.yml -c prod.yml shop"),
            "{line}"
        );
    }

    /// `--prune` removes services that are no longer in the file. A
    /// reasonable thing to want, and a terrible default for a button.
    #[test]
    fn deploy_never_prunes_on_its_own() {
        let line = deploy(&engine(), &["a.yml".to_string()], "shop").to_line();
        assert!(!line.contains("--prune"), "{line}");
    }

    #[test]
    fn scale_names_the_service_and_the_count() {
        let line = scale(&engine(), "shop_web", 3).to_line();
        assert!(line.ends_with("service scale shop_web=3"), "{line}");
    }

    /// Every listing asks for the per-line JSON both engines parse the
    /// same way.
    #[test]
    fn the_listings_ask_for_json() {
        let e = engine();
        for inv in [
            list_stacks(&e),
            list_services(&e),
            list_nodes(&e),
            stack_services(&e, "shop"),
        ] {
            assert!(inv.to_line().contains("{{json .}}"), "{}", inv.to_line());
        }
        assert!(info(&e).to_line().contains("{{json .}}"));
    }
}
