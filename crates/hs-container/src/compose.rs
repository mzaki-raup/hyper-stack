//! Compose projects, read back out of the engine
//! (`docs/tools/containers.md` §4, phase 5).
//!
//! A Compose project is not a thing the engine stores: it is a set of
//! containers wearing the same labels. `docker compose` writes
//! `com.docker.compose.project` on every container it creates, along
//! with the service name and - the part that makes this useful - the
//! **config files and working directory it was brought up from**. So a
//! project can be rebuilt from a plain `ps`, and its verbs can be run
//! against the same files Compose itself used, without the app keeping
//! a registry of its own that would go stale the moment someone ran
//! `docker compose` in a terminal.
//!
//! The verbs shell out to `docker compose` rather than being
//! reimplemented against the engine API: §4 asks for fidelity, and
//! dependency order, profiles, `depends_on` conditions and build
//! contexts are Compose's job, not ours.

use crate::cli::{Engine, Invocation};
use crate::model::{Container, Port, State};
// The label names live in `model`, beside the accessors that read them:
// a second copy here would be a typo waiting to make a project that
// silently never groups.
use crate::model::{L_CONFIG_FILES, L_NUMBER, L_WORKING_DIR};
#[cfg(test)]
use crate::model::{L_PROJECT, L_SERVICE};

/// One container of a project, as the pane lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    /// The service's name in the compose file (`web`, `db`).
    pub name: String,
    /// Which replica this is; 1 unless the service is scaled.
    pub number: u32,
    pub container: String,
    pub id: String,
    pub state: State,
    pub status: String,
    pub image: String,
    pub ports: Vec<Port>,
}

/// A Compose project: everything wearing one project label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    /// The compose files it was brought up from, in Compose's own
    /// order, as paths **on the engine's machine**. Empty when the
    /// containers predate the label or were made by hand.
    pub config_files: Vec<String>,
    pub working_dir: String,
    pub services: Vec<Service>,
}

impl Project {
    pub fn running(&self) -> usize {
        self.services
            .iter()
            .filter(|s| s.state.is_running())
            .count()
    }

    /// Whether the verbs can be run for this project.
    ///
    /// Without a config file `docker compose -p <name> up` would be run
    /// against whatever happens to be in the current directory, which
    /// is how a project gets recreated as something else entirely. The
    /// pane offers only `down`-shaped verbs in that case, and says why.
    pub fn has_files(&self) -> bool {
        !self.config_files.is_empty()
    }
}

/// Every Compose project among these containers, by their labels.
///
/// Sorted by name, and the services within a project by service name
/// then replica number, so a refresh does not reshuffle the list under
/// the pointer.
pub fn projects(containers: &[Container]) -> Vec<Project> {
    let mut out: Vec<Project> = Vec::new();
    for c in containers {
        let Some(name) = c.compose_project().filter(|n| !n.trim().is_empty()) else {
            continue;
        };
        let service = Service {
            name: c.compose_service().unwrap_or("").to_string(),
            number: c
                .label(L_NUMBER)
                .and_then(|n| n.trim().parse().ok())
                .unwrap_or(1),
            container: c.name.clone(),
            id: c.id.clone(),
            state: c.state,
            status: c.status.clone(),
            image: c.image.clone(),
            ports: c.ports.clone(),
        };
        match out.iter_mut().find(|p| p.name == name) {
            Some(p) => {
                // A project's files are taken from whichever container
                // carries them: one that was brought up by hand later
                // has no such label, and must not blank them.
                if p.config_files.is_empty() {
                    p.config_files = config_files(c);
                }
                if p.working_dir.is_empty() {
                    p.working_dir = c.label(L_WORKING_DIR).unwrap_or("").to_string();
                }
                p.services.push(service);
            }
            None => out.push(Project {
                name: name.to_string(),
                config_files: config_files(c),
                working_dir: c.label(L_WORKING_DIR).unwrap_or("").to_string(),
                services: vec![service],
            }),
        }
    }
    for p in &mut out {
        p.services
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.number.cmp(&b.number)));
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The compose files one container was brought up from.
///
/// **Only as good as the label can be.** Compose joins several files
/// with a comma, and `docker ps --format '{{json .}}'` joins the whole
/// *label set* with a comma too, so a project built from two files
/// cannot be told from two labels by reading that string - the second
/// file arrives looking like a label with no `=` in it and is dropped.
/// [`parse_list`] is the way out: `docker compose ls` reports the files
/// as their own JSON field, and [`merge`] prefers it. This is the
/// fallback for an engine too old to have `compose ls`, where one file
/// (the overwhelmingly common case) is still right.
fn config_files(c: &Container) -> Vec<String> {
    c.label(L_CONFIG_FILES)
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// One row of `docker compose ls`: the authoritative project list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Listed {
    pub name: String,
    /// Compose's own sentence, `running(2)` or `exited(1)`.
    pub status: String,
    pub config_files: Vec<String>,
}

/// `docker compose ls -a --format json`.
///
/// `-a` so a project whose containers are all stopped is still listed;
/// without it a `down`-but-not-removed project vanishes from the pane
/// and looks deleted.
pub fn list(engine: &Engine) -> Invocation {
    engine.invoke(["compose", "ls", "-a", "--format", "json"])
}

/// Read `docker compose ls`, which answers as a JSON array.
pub fn parse_list(text: &str) -> Vec<Listed> {
    let v: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or_default();
    let rows = match v {
        serde_json::Value::Array(a) => a,
        _ => return Vec::new(),
    };
    rows.iter()
        .map(|r| Listed {
            name: r
                .get("Name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            status: r
                .get("Status")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            config_files: r
                .get("ConfigFiles")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        })
        .filter(|l| !l.name.is_empty())
        .collect()
}

/// The projects to show: `docker compose ls` for the truth about which
/// projects exist and which files they came from, the containers'
/// labels for the services inside them.
///
/// A project Compose lists but that has no containers is kept, with no
/// services - that is a project brought down and is worth seeing. A
/// project the containers show but Compose does not is kept too, since
/// the containers are plainly there.
pub fn merge(listed: &[Listed], containers: &[Container]) -> Vec<Project> {
    let mut out = projects(containers);
    for l in listed {
        match out.iter_mut().find(|p| p.name == l.name) {
            Some(p) => {
                // Compose's answer wins: the label string cannot
                // represent several files unambiguously.
                if !l.config_files.is_empty() {
                    p.config_files = l.config_files.clone();
                }
            }
            None => out.push(Project {
                name: l.name.clone(),
                config_files: l.config_files.clone(),
                working_dir: String::new(),
                services: Vec::new(),
            }),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// What can be done to a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeVerb {
    /// `up -d`: create and start what is missing, leave the rest.
    Up,
    /// `down`: stop and **remove** the containers and the default
    /// network. Destructive, and named so.
    Down,
    Start,
    Stop,
    Restart,
    Pull,
    Build,
    /// `config`: the rendered, merged YAML. A read, and the honest
    /// answer to "what will this actually do".
    Config,
}

impl ComposeVerb {
    pub fn as_str(self) -> &'static str {
        match self {
            ComposeVerb::Up => "up",
            ComposeVerb::Down => "down",
            ComposeVerb::Start => "start",
            ComposeVerb::Stop => "stop",
            ComposeVerb::Restart => "restart",
            ComposeVerb::Pull => "pull",
            ComposeVerb::Build => "build",
            ComposeVerb::Config => "config",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ComposeVerb::Up => "container-compose-up",
            ComposeVerb::Down => "container-compose-down",
            ComposeVerb::Start => "container-compose-start",
            ComposeVerb::Stop => "container-compose-stop",
            ComposeVerb::Restart => "container-compose-restart",
            ComposeVerb::Pull => "container-compose-pull",
            ComposeVerb::Build => "container-compose-build",
            ComposeVerb::Config => "container-compose-config",
        }
    }

    /// `down` removes containers and networks; the rest do not.
    pub fn destructive(self) -> bool {
        matches!(self, ComposeVerb::Down)
    }

    /// Whether this needs the compose files to be known. `down`,
    /// `start`, `stop` and `restart` work from the project name alone,
    /// because the engine already knows which containers wear the
    /// label; `up`, `pull`, `build` and `config` have to read the file.
    pub fn needs_files(self) -> bool {
        matches!(
            self,
            ComposeVerb::Up | ComposeVerb::Pull | ComposeVerb::Build | ComposeVerb::Config
        )
    }

    /// The flags that belong after the verb.
    fn flags(self) -> &'static [&'static str] {
        match self {
            // Detached, or the command never returns and the Job hangs
            // waiting for a process that is tailing logs.
            ComposeVerb::Up => &["-d"],
            _ => &[],
        }
    }
}

/// How to run one Compose verb for a project.
///
/// `-f` for each config file and `-p` for the project, both **before**
/// the subcommand: `docker compose` takes its own options there, and
/// after the verb they are the verb's options and mean something else
/// or nothing at all.
pub fn invocation(
    engine: &Engine,
    project: &str,
    files: &[String],
    verb: ComposeVerb,
) -> Invocation {
    let mut args: Vec<String> = vec!["compose".into()];
    for f in files {
        args.push("-f".into());
        args.push(f.clone());
    }
    if !project.trim().is_empty() {
        args.push("-p".into());
        args.push(project.trim().to_string());
    }
    args.push(verb.as_str().to_string());
    for f in verb.flags() {
        args.push((*f).to_string());
    }
    engine.invoke(args)
}

/// The same thing as a command line, for a Job or a terminal.
pub fn line(engine: &Engine, project: &str, files: &[String], verb: ComposeVerb) -> String {
    invocation(engine, project, files, verb).to_line()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ContainerFields, Transport};

    fn container(name: &str, labels: &[(&str, &str)], state: State) -> Container {
        Container {
            id: format!("id-{name}"),
            name: name.to_string(),
            names: vec![name.to_string()],
            image: "img:1".into(),
            state,
            status: "Up 2 hours".into(),
            health: Default::default(),
            ports: Vec::new(),
            labels: labels
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
            created: String::new(),
            command: String::new(),
        }
    }

    fn engine() -> Engine {
        Engine::new(
            "docker",
            ContainerFields {
                transport: Transport::Wsl,
                distro: "Ubuntu-24.04".into(),
                ..Default::default()
            },
        )
    }

    /// A project is a set of labels, not a thing the engine stores, so
    /// the grouping is the whole feature.
    #[test]
    fn containers_group_into_projects_by_their_labels() {
        let cs = vec![
            container(
                "shop-web-1",
                &[
                    (L_PROJECT, "shop"),
                    (L_SERVICE, "web"),
                    (L_CONFIG_FILES, "/srv/shop/compose.yaml"),
                    (L_WORKING_DIR, "/srv/shop"),
                ],
                State::Running,
            ),
            container(
                "shop-db-1",
                &[(L_PROJECT, "shop"), (L_SERVICE, "db")],
                State::Running,
            ),
            container(
                "blog-web-1",
                &[(L_PROJECT, "blog"), (L_SERVICE, "web")],
                State::Exited,
            ),
            // Not Compose's: it must not join a project.
            container("stray", &[], State::Running),
        ];
        let ps = projects(&cs);
        assert_eq!(ps.len(), 2, "two projects, and the stray is not one");
        assert_eq!(ps[0].name, "blog", "sorted by name");
        assert_eq!(ps[1].name, "shop");
        assert_eq!(ps[1].services.len(), 2);
        assert_eq!(ps[1].services[0].name, "db", "services sorted by name");
        assert_eq!(ps[1].config_files, ["/srv/shop/compose.yaml"]);
        assert_eq!(ps[1].working_dir, "/srv/shop");
        assert_eq!(ps[1].running(), 2);
        assert_eq!(ps[0].running(), 0);
    }

    /// The files come from whichever container carries them. One
    /// started by hand afterwards has no such label and must not blank
    /// what the others know.
    #[test]
    fn a_container_without_the_file_labels_does_not_erase_them() {
        let cs = vec![
            container(
                "shop-db-1",
                &[(L_PROJECT, "shop"), (L_SERVICE, "db")],
                State::Running,
            ),
            container(
                "shop-web-1",
                &[
                    (L_PROJECT, "shop"),
                    (L_SERVICE, "web"),
                    (L_CONFIG_FILES, "/srv/shop/compose.yaml"),
                ],
                State::Running,
            ),
        ];
        let ps = projects(&cs);
        assert_eq!(ps[0].config_files, ["/srv/shop/compose.yaml"]);
        assert!(ps[0].has_files());
    }

    /// Compose joins several files with a comma, and a single file has
    /// no separator at all.
    #[test]
    fn several_config_files_are_split_in_order() {
        let cs = vec![container(
            "a-web-1",
            &[
                (L_PROJECT, "a"),
                (
                    L_CONFIG_FILES,
                    "/srv/a/compose.yaml,/srv/a/compose.prod.yaml",
                ),
            ],
            State::Running,
        )];
        assert_eq!(
            projects(&cs)[0].config_files,
            ["/srv/a/compose.yaml", "/srv/a/compose.prod.yaml"]
        );
    }

    /// A scaled service is several containers of one name, which have
    /// to stay in replica order rather than whatever `ps` returned.
    #[test]
    fn replicas_keep_their_order() {
        let cs = vec![
            container(
                "a-web-3",
                &[(L_PROJECT, "a"), (L_SERVICE, "web"), (L_NUMBER, "3")],
                State::Running,
            ),
            container(
                "a-web-1",
                &[(L_PROJECT, "a"), (L_SERVICE, "web"), (L_NUMBER, "1")],
                State::Running,
            ),
            container(
                "a-web-2",
                &[(L_PROJECT, "a"), (L_SERVICE, "web"), (L_NUMBER, "2")],
                State::Running,
            ),
        ];
        let numbers: Vec<u32> = projects(&cs)[0].services.iter().map(|s| s.number).collect();
        assert_eq!(numbers, [1, 2, 3]);
    }

    /// `-f` and `-p` are **`docker compose`'s** options and belong
    /// before the subcommand; after it they are the verb's options and
    /// mean something else or nothing.
    #[test]
    fn the_options_go_before_the_subcommand() {
        let e = engine();
        let line = line(
            &e,
            "shop",
            &["/srv/shop/compose.yaml".to_string()],
            ComposeVerb::Up,
        );
        assert_eq!(
            line,
            "wsl.exe -d Ubuntu-24.04 -- docker compose -f /srv/shop/compose.yaml -p shop up -d"
        );
    }

    /// `up` without `-d` never returns: it tails the logs, and a Job
    /// would wait for a process that is not going to end.
    #[test]
    fn up_is_detached() {
        let e = engine();
        assert!(line(&e, "a", &[], ComposeVerb::Up).ends_with(" up -d"));
        // And nothing else gains a stray flag.
        assert!(line(&e, "a", &[], ComposeVerb::Down).ends_with(" -p a down"));
    }

    /// Every file is passed, each with its own `-f`, in order: the
    /// later file overrides the earlier, so the order is the meaning.
    #[test]
    fn each_file_gets_its_own_flag_in_order() {
        let e = engine();
        let line = line(
            &e,
            "a",
            &["base.yaml".to_string(), "prod.yaml".to_string()],
            ComposeVerb::Config,
        );
        assert!(
            line.contains("-f base.yaml -f prod.yaml"),
            "both files, in order: {line}"
        );
    }

    /// Which verbs need the file is not a detail: running `up` without
    /// one takes whatever is in the current directory, which is how a
    /// project gets recreated as something else.
    #[test]
    fn the_verbs_that_need_a_file_say_so() {
        for v in [
            ComposeVerb::Up,
            ComposeVerb::Pull,
            ComposeVerb::Build,
            ComposeVerb::Config,
        ] {
            assert!(v.needs_files(), "{v:?} reads the compose file");
        }
        for v in [
            ComposeVerb::Down,
            ComposeVerb::Start,
            ComposeVerb::Stop,
            ComposeVerb::Restart,
        ] {
            assert!(!v.needs_files(), "{v:?} works from the project label");
        }
    }

    /// `down` removes containers and networks; nothing else here does.
    #[test]
    fn only_down_is_destructive() {
        assert!(ComposeVerb::Down.destructive());
        for v in [
            ComposeVerb::Up,
            ComposeVerb::Start,
            ComposeVerb::Stop,
            ComposeVerb::Restart,
            ComposeVerb::Pull,
            ComposeVerb::Build,
            ComposeVerb::Config,
        ] {
            assert!(!v.destructive(), "{v:?} is not destructive");
        }
    }

    /// `docker compose ls` answers as a JSON array, and it is the only
    /// unambiguous source for a project's files.
    #[test]
    fn the_project_list_is_read_from_compose_ls() {
        let text =
            r#"[{"Name":"hsdemo","Status":"running(2)","ConfigFiles":"/srv/a/compose.yaml"}]"#;
        let ls = parse_list(text);
        assert_eq!(ls.len(), 1);
        assert_eq!(ls[0].name, "hsdemo");
        assert_eq!(ls[0].status, "running(2)");
        assert_eq!(ls[0].config_files, ["/srv/a/compose.yaml"]);
        // Several files are a comma-separated field of its own here,
        // which is exactly what the label string cannot express.
        let two = parse_list(
            r#"[{"Name":"a","Status":"running(1)","ConfigFiles":"/srv/a/base.yaml,/srv/a/prod.yaml"}]"#,
        );
        assert_eq!(
            two[0].config_files,
            ["/srv/a/base.yaml", "/srv/a/prod.yaml"]
        );
        // An engine with no projects, and one too old to answer.
        assert!(parse_list("[]").is_empty());
        assert!(parse_list("").is_empty());
        assert!(parse_list("Name Status").is_empty());
    }

    /// Compose's answer about the files wins over the label, which
    /// cannot represent two files without ambiguity.
    #[test]
    fn merging_prefers_compose_ls_for_the_files() {
        let cs = vec![container(
            "a-web-1",
            &[
                (L_PROJECT, "a"),
                (L_SERVICE, "web"),
                // What the label string would have yielded: the first
                // file only, the second lost to the separator.
                (L_CONFIG_FILES, "/srv/a/base.yaml"),
            ],
            State::Running,
        )];
        let listed = vec![Listed {
            name: "a".into(),
            status: "running(1)".into(),
            config_files: vec!["/srv/a/base.yaml".into(), "/srv/a/prod.yaml".into()],
        }];
        let ps = merge(&listed, &cs);
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].config_files, ["/srv/a/base.yaml", "/srv/a/prod.yaml"]);
        assert_eq!(
            ps[0].services.len(),
            1,
            "the services still come from labels"
        );
    }

    /// A project brought down still exists and is worth seeing; one
    /// whose containers are plainly there is kept even if Compose does
    /// not list it.
    #[test]
    fn a_project_from_either_side_is_kept() {
        let cs = vec![container(
            "live-web-1",
            &[(L_PROJECT, "live"), (L_SERVICE, "web")],
            State::Running,
        )];
        let listed = vec![Listed {
            name: "stopped".into(),
            status: "exited(2)".into(),
            config_files: vec!["/srv/s/compose.yaml".into()],
        }];
        let ps = merge(&listed, &cs);
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["live", "stopped"], "both, sorted");
        assert!(ps[1].services.is_empty(), "a down project has no services");
        assert!(ps[1].has_files(), "but it can still be brought up");
    }

    /// `-a`, or a project whose containers are all stopped vanishes
    /// from the pane and looks deleted.
    #[test]
    fn the_listing_asks_for_stopped_projects_too() {
        let line = list(&engine()).to_line();
        assert!(line.contains("compose ls -a"), "{line}");
        assert!(line.contains("--format json"), "{line}");
    }

    #[test]
    fn a_project_with_no_files_knows_it() {
        let cs = vec![container(
            "a-web-1",
            &[(L_PROJECT, "a"), (L_SERVICE, "web")],
            State::Running,
        )];
        assert!(!projects(&cs)[0].has_files());
    }
}
