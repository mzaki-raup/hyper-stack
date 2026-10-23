//! Services running on this machine, and where each one writes its log
//! (docs/tools/logs.md §16, owner's request 2026-10-23).
//!
//! The sidebar lists what is running so a log can be opened without
//! knowing the path by heart. Detection is deliberately shallow and
//! read-only: it asks the process list, Docker and systemd what they are
//! running, matches the executable against a table of well-known servers,
//! and offers the paths that server conventionally writes to. It never
//! starts, stops or configures anything.
//!
//! A guess is a guess. Every candidate path is checked for existence
//! before it is offered, and the configure form lets the user correct it,
//! so a wrong guess costs a glance rather than an empty tab.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a service was found, which decides how its log is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Host {
    /// A process on this machine.
    Local,
    /// A Docker or Podman container; the log comes from the runtime.
    Container,
    /// A systemd unit, read through `journalctl`.
    Systemd,
    /// A unit inside a WSL distribution.
    Wsl,
    /// A Kubernetes pod, read through `kubectl logs`.
    Kubernetes,
}

/// One running thing the sidebar can offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Service {
    /// What to call it in the list: the container name, the unit, or the
    /// server's own name with its process id.
    pub name: String,
    /// The well-known server this looks like ("nginx", "tomcat"), or the
    /// executable's own name when it matches nothing in the table.
    pub kind: String,
    pub host: Host,
    /// The container id, unit name or pod this needs to read its log.
    pub handle: String,
    /// Log files that exist and look like this service's.
    pub files: Vec<String>,
    /// A command that follows this service's log, for the hosts that have
    /// no file to read.
    pub command: Option<String>,
}

impl Service {
    /// What the sidebar row says under the name.
    pub fn describe(&self) -> String {
        match self.host {
            Host::Local => match self.files.first() {
                Some(f) => f.clone(),
                None => self.kind.clone(),
            },
            Host::Container => format!("container · {}", self.handle),
            Host::Systemd => format!("systemd · {}", self.handle),
            Host::Wsl => format!("wsl · {}", self.handle),
            Host::Kubernetes => format!("kubernetes · {}", self.handle),
        }
    }

    /// A stable key, so a service that has already been saved can be left
    /// out of the list (owner's request: once configured it belongs to
    /// Saved Logs, not here).
    pub fn key(&self) -> String {
        match self.host {
            Host::Local => self
                .files
                .first()
                .cloned()
                .unwrap_or_else(|| format!("local:{}", self.kind)),
            _ => format!("{:?}:{}", self.host, self.handle).to_lowercase(),
        }
    }
}

/// A well-known server: how its process is named, and where it writes.
struct Known {
    kind: &'static str,
    /// Lower-case fragments of the executable or container image.
    matches: &'static [&'static str],
    /// Paths it conventionally logs to. `{}` is not substituted; these
    /// are checked as they are, and only existing ones are offered.
    paths: &'static [&'static str],
}

/// The servers this recognises. Adding one is a row here, not code
/// elsewhere.
const KNOWN: &[Known] = &[
    Known {
        kind: "nginx",
        matches: &["nginx"],
        paths: &[
            "/var/log/nginx/access.log",
            "/var/log/nginx/error.log",
            "C:/nginx/logs/access.log",
            "C:/nginx/logs/error.log",
        ],
    },
    Known {
        kind: "apache",
        matches: &["httpd", "apache2"],
        paths: &[
            "/var/log/apache2/access.log",
            "/var/log/apache2/error.log",
            "/var/log/httpd/access_log",
            "/var/log/httpd/error_log",
        ],
    },
    Known {
        kind: "caddy",
        matches: &["caddy"],
        paths: &["/var/log/caddy/access.log"],
    },
    Known {
        kind: "tomcat",
        matches: &["tomcat", "catalina"],
        paths: &[
            "/var/log/tomcat/catalina.out",
            "/opt/tomcat/logs/catalina.out",
            "/usr/local/tomcat/logs/catalina.out",
        ],
    },
    Known {
        kind: "keycloak",
        // Keycloak runs as `java -jar quarkus-run.jar` or `kc.sh start`,
        // so the executable alone says only "java": the arguments are
        // what identify it (owner's report, 2026-10-23).
        matches: &["keycloak", "kc.sh", "kc.bat", "quarkus-run.jar"],
        paths: &[
            "/opt/keycloak/data/log/keycloak.log",
            "/var/log/keycloak/keycloak.log",
            "C:/keycloak/data/log/keycloak.log",
        ],
    },
    Known {
        kind: "elasticsearch",
        matches: &["elasticsearch"],
        paths: &["/var/log/elasticsearch/elasticsearch.log"],
    },
    Known {
        kind: "kafka",
        matches: &["kafka.Kafka", "kafka-server"],
        paths: &["/var/log/kafka/server.log", "/opt/kafka/logs/server.log"],
    },
    Known {
        kind: "rabbitmq",
        matches: &["rabbitmq", "beam.smp"],
        paths: &["/var/log/rabbitmq"],
    },
    Known {
        kind: "mongodb",
        matches: &["mongod"],
        paths: &["/var/log/mongodb/mongod.log"],
    },
    Known {
        kind: "haproxy",
        matches: &["haproxy"],
        paths: &["/var/log/haproxy.log"],
    },
    Known {
        kind: "traefik",
        matches: &["traefik"],
        paths: &["/var/log/traefik/traefik.log"],
    },
    Known {
        kind: "jboss",
        matches: &["jboss", "standalone.sh"],
        paths: &["/opt/jboss/standalone/log/server.log"],
    },
    Known {
        kind: "wildfly",
        matches: &["wildfly"],
        paths: &["/opt/wildfly/standalone/log/server.log"],
    },
    Known {
        kind: "jetty",
        matches: &["jetty"],
        paths: &["/var/log/jetty/jetty.log"],
    },
    Known {
        kind: "quarkus",
        matches: &["quarkus"],
        paths: &["/var/log/quarkus/application.log"],
    },
    Known {
        kind: "iis",
        matches: &["w3wp", "inetinfo"],
        paths: &["C:/inetpub/logs/LogFiles"],
    },
    Known {
        kind: "kestrel",
        matches: &["dotnet"],
        paths: &[],
    },
    Known {
        kind: "node",
        matches: &["node", "express", "fastify"],
        paths: &[],
    },
    Known {
        kind: "gunicorn",
        matches: &["gunicorn", "uwsgi", "uvicorn", "hypercorn"],
        paths: &["/var/log/gunicorn/error.log", "/var/log/uwsgi/app.log"],
    },
    Known {
        kind: "postgres",
        matches: &["postgres"],
        paths: &[
            "/var/log/postgresql",
            "C:/Program Files/PostgreSQL/17/data/log",
            "C:/Program Files/PostgreSQL/16/data/log",
            "C:/Program Files/PostgreSQL/15/data/log",
        ],
    },
    Known {
        kind: "mysql",
        matches: &["mysqld", "mariadbd"],
        paths: &[
            "/var/log/mysql/error.log",
            "/var/log/mariadb/mariadb.log",
            "C:/ProgramData/MySQL/MySQL Server 8.4/Data",
            "C:/ProgramData/MySQL/MySQL Server 8.0/Data",
        ],
    },
    Known {
        kind: "redis",
        matches: &["redis-server"],
        paths: &["/var/log/redis/redis-server.log"],
    },
];

/// The well-known server an executable or image name looks like.
pub fn classify(name: &str) -> Option<&'static str> {
    let n = name.to_lowercase();
    KNOWN
        .iter()
        .find(|k| k.matches.iter().any(|m| n.contains(m)))
        .map(|k| k.kind)
}

/// The paths a known server writes to that actually exist here.
pub fn known_paths(kind: &str) -> Vec<String> {
    KNOWN
        .iter()
        .find(|k| k.kind == kind)
        .map(|k| {
            k.paths
                .iter()
                .filter(|p| PathBuf::from(p).exists())
                .map(|p| (*p).to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// The parser a known server's log wants, so the configure form opens on
/// the right one rather than on Auto (owner's request, 2026-10-23).
/// Anything not listed stays Auto, which now settles on its own.
pub fn format_for(kind: &str) -> crate::parse::Format {
    use crate::parse::Format;
    match kind {
        // Both write the combined access log; the error log is plain and
        // Auto settles that one correctly on its own.
        "nginx" | "apache" | "caddy" => Format::Nginx,
        "postgres" => Format::Postgres,
        // A container runtime prefixes its own timestamp and the payload
        // is usually JSON or logfmt; Auto reads both and the prefix is
        // stripped either way.
        _ => Format::Auto,
    }
}

/// Every path a known server might write to, whether or not it is here.
/// The sidebar only ever offers paths that exist, but the configure form
/// would rather start from a likely path the user can correct than from
/// an empty field (owner's report, 2026-10-23).
pub fn candidate_paths(kind: &str) -> Vec<String> {
    KNOWN
        .iter()
        .find(|k| k.kind == kind)
        .map(|k| k.paths.iter().map(|p| (*p).to_string()).collect())
        .unwrap_or_default()
}

/// Every kind this knows about, for the configure form's picker.
pub fn kinds() -> Vec<&'static str> {
    KNOWN.iter().map(|k| k.kind).collect()
}

/// How long a scan's answer is reused. Services do not come and go fast
/// enough to be worth asking more often, and each ask spawns five
/// processes.
const CACHE_FOR: std::time::Duration = std::time::Duration::from_secs(60);

static CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<Service>)>> =
    std::sync::Mutex::new(None);

/// The running services, from the cache when it is fresh. Every Logs tab
/// draws the sidebar, so without this a workspace with six log tabs ran
/// five processes per tab on open, which the owner saw as a flicker
/// (2026-10-23). `refresh` on the sidebar calls [`rescan`] instead.
pub fn detect() -> Vec<Service> {
    if let Ok(guard) = CACHE.lock()
        && let Some((at, found)) = guard.as_ref()
        && at.elapsed() < CACHE_FOR
    {
        return found.clone();
    }
    rescan()
}

/// Ask again, whatever the cache says, and remember the answer.
/// Blocking: the caller puts it on a thread, never on the UI.
pub fn rescan() -> Vec<Service> {
    let found = scan();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((std::time::Instant::now(), found.clone()));
    }
    found
}

/// Each probe is independent, and one whose tool is not installed simply
/// contributes nothing.
fn scan() -> Vec<Service> {
    let mut out = Vec::new();
    out.extend(processes());
    out.extend(containers(&[], "docker"));
    out.extend(containers(&[], "podman"));
    out.extend(systemd_units(&[]));
    out.extend(kube_pods());
    // Docker on Windows is very often inside WSL rather than on the host,
    // where `docker ps` finds nothing (owner's report, 2026-10-23). Each
    // distribution is asked the same questions through `wsl -d <name> --`.
    for distro in wsl_distros() {
        let via: Vec<String> = vec!["-d".into(), distro.clone(), "--".into()];
        out.extend(containers(&via, "docker"));
        out.extend(containers(&via, "podman"));
        out.extend(systemd_units(&via));
    }
    // One row per thing: a server found both as a process and as a unit
    // is the same server.
    out.sort_by_key(|s| s.name.to_lowercase());
    out.dedup_by(|a, b| a.key() == b.key());
    out
}

/// Run a command and give back its stdout, or nothing when it is not
/// installed or fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    run_owned(
        program,
        &args.iter().map(|a| (*a).to_string()).collect::<Vec<_>>(),
    )
}

/// As [`run`], with arguments that are owned: the WSL probes build theirs.
fn run_owned(program: &str, args: &[String]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(decode(&out.stdout))
}

/// `wsl.exe` writes its own output as UTF-16, while the commands it runs
/// write UTF-8. Reading the first as UTF-8 gives a string of NULs, so the
/// encoding is detected rather than assumed.
fn decode(bytes: &[u8]) -> String {
    let nuls = bytes.iter().filter(|b| **b == 0).count();
    if bytes.len() >= 4 && nuls * 3 >= bytes.len() {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// A command on the host, or the same command inside a WSL
/// distribution: `wsl.exe -d <distro> -- <program> …`.
fn prefixed(via: &[String], program: &str) -> (String, Vec<String>) {
    if via.is_empty() {
        return (program.to_string(), Vec::new());
    }
    let mut args = via.to_vec();
    args.push(program.to_string());
    ("wsl.exe".to_string(), args)
}

/// The installed WSL distributions, or nothing when WSL is absent.
fn wsl_distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let Some(text) = run("wsl.exe", &["-l", "-q"]) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.trim().trim_matches('\u{0}').to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Server processes on this machine.
fn processes() -> Vec<Service> {
    // The executable name alone is not enough: Keycloak, Tomcat, Kafka
    // and Elasticsearch are all "java", and what tells them apart is the
    // command line. `tasklist` does not print one, so Windows asks CIM
    // instead and falls back to `tasklist` where PowerShell is absent.
    let listing = if cfg!(windows) {
        run(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Get-CimInstance Win32_Process | ForEach-Object { \"$($_.ProcessId) $($_.Name) $($_.CommandLine)\" }",
            ],
        )
        .or_else(|| run("tasklist", &["/fo", "csv", "/nh"]))
    } else {
        run("ps", &["-eo", "pid=,comm=,args="])
    };
    let Some(text) = listing else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Both shapes are "<pid> <the rest>", except `tasklist`'s CSV,
        // which is recognised by its leading quote.
        let (pid, name) = if line.starts_with('"') {
            let cols: Vec<&str> = line.split("\",\"").collect();
            let Some(first) = cols.first() else { continue };
            let name = first.trim_matches('"').to_string();
            let pid = cols.get(1).unwrap_or(&"").trim_matches('"').to_string();
            (pid, name)
        } else {
            let mut it = line.splitn(2, char::is_whitespace);
            let pid = it.next().unwrap_or("").to_string();
            (pid, it.next().unwrap_or("").trim().to_string())
        };
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Some(kind) = classify(&name) else {
            continue;
        };
        let files = known_paths(kind);
        out.push(Service {
            name: format!("{kind} ({pid})"),
            kind: kind.to_string(),
            host: Host::Local,
            handle: pid,
            files,
            command: None,
        });
    }
    out
}

/// Containers under a runtime that speaks the Docker CLI.
fn containers(via: &[String], runtime: &str) -> Vec<Service> {
    // `via` is empty on the host, or `-d <distro> --` to ask inside a WSL
    // distribution; the runtime's own arguments follow either way.
    let (program, mut args) = prefixed(via, runtime);
    args.extend(
        ["ps", "--format", "{{.ID}}\t{{.Names}}\t{{.Image}}"]
            .iter()
            .map(|a| (*a).to_string()),
    );
    let Some(text) = run_owned(&program, &args) else {
        return Vec::new();
    };
    let inside = (!via.is_empty()).then(|| via[1].clone());
    text.lines()
        .filter_map(|l| {
            let mut c = l.split('\t');
            let id = c.next()?.trim().to_string();
            let name = c.next()?.trim().to_string();
            let image = c.next().unwrap_or("").trim();
            if id.is_empty() {
                return None;
            }
            let kind = classify(image)
                .or_else(|| classify(&name))
                .unwrap_or("container");
            let (host, handle, command) = match &inside {
                Some(distro) => (
                    Host::Wsl,
                    format!("{distro}/{id}"),
                    format!("wsl.exe -d {distro} -- {runtime} logs -f --tail 2000 {id}"),
                ),
                None => (
                    Host::Container,
                    id.clone(),
                    format!("{runtime} logs -f --tail 2000 {id}"),
                ),
            };
            Some(Service {
                name: match &inside {
                    Some(distro) => format!("{name} ({distro})"),
                    None => name,
                },
                kind: kind.to_string(),
                host,
                handle,
                files: Vec::new(),
                command: Some(command),
            })
        })
        .collect()
}

/// Running systemd units that look like a server.
fn systemd_units(via: &[String]) -> Vec<Service> {
    let (program, mut args) = prefixed(via, "systemctl");
    args.extend(
        [
            "list-units",
            "--type=service",
            "--state=running",
            "--no-legend",
            "--no-pager",
            "--plain",
        ]
        .iter()
        .map(|a| (*a).to_string()),
    );
    let Some(text) = run_owned(&program, &args) else {
        return Vec::new();
    };
    let inside = (!via.is_empty()).then(|| via[1].clone());
    text.lines()
        .filter_map(|l| {
            let unit = l.split_whitespace().next()?;
            let bare = unit.strip_suffix(".service").unwrap_or(unit);
            let kind = classify(bare)?;
            let (host, handle, command) = match &inside {
                Some(distro) => (
                    Host::Wsl,
                    format!("{distro}/{unit}"),
                    format!("wsl.exe -d {distro} -- journalctl -n 2000 -f -u {unit}"),
                ),
                None => (
                    Host::Systemd,
                    unit.to_string(),
                    format!("journalctl -n 2000 -f -u {unit}"),
                ),
            };
            Some(Service {
                name: match &inside {
                    Some(distro) => format!("{bare} ({distro})"),
                    None => bare.to_string(),
                },
                kind: kind.to_string(),
                host,
                handle,
                files: Vec::new(),
                command: Some(command),
            })
        })
        .collect()
}

/// Pods in the current kubectl context.
fn kube_pods() -> Vec<Service> {
    let Some(text) = run(
        "kubectl",
        &[
            "get",
            "pods",
            "--field-selector=status.phase=Running",
            "-o",
            "custom-columns=:metadata.name",
            "--no-headers",
        ],
    ) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let pod = l.trim();
            if pod.is_empty() {
                return None;
            }
            Some(Service {
                name: pod.to_string(),
                kind: classify(pod).unwrap_or("pod").to_string(),
                host: Host::Kubernetes,
                handle: pod.to_string(),
                files: Vec::new(),
                command: Some(format!("kubectl logs -f --tail 2000 {pod}")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executables_are_matched_to_the_server_they_are() {
        assert_eq!(classify("nginx: master process"), Some("nginx"));
        assert_eq!(classify("/usr/sbin/apache2"), Some("apache"));
        assert_eq!(classify("httpd.exe"), Some("apache"));
        assert_eq!(classify("java -jar ... catalina ..."), Some("tomcat"));
        assert_eq!(classify("w3wp.exe"), Some("iis"));
        assert_eq!(classify("mariadbd"), Some("mysql"));
        assert_eq!(classify("some-random-thing"), None);
    }

    #[test]
    fn a_service_says_where_it_came_from() {
        let c = Service {
            name: "shop-api".into(),
            kind: "node".into(),
            host: Host::Container,
            handle: "9f2ab3".into(),
            files: Vec::new(),
            command: Some("docker logs -f 9f2ab3".into()),
        };
        assert_eq!(c.describe(), "container · 9f2ab3");
        assert_eq!(c.key(), "container:9f2ab3");

        let local = Service {
            name: "nginx".into(),
            kind: "nginx".into(),
            host: Host::Local,
            handle: "1234".into(),
            files: vec!["/var/log/nginx/error.log".into()],
            command: None,
        };
        assert_eq!(local.describe(), "/var/log/nginx/error.log");
        assert_eq!(local.key(), "/var/log/nginx/error.log");
    }

    #[test]
    fn only_paths_that_exist_are_offered() {
        // Nothing in the table is expected to exist on a build machine,
        // and a guess that does not exist must never reach the form.
        for kind in kinds() {
            for p in known_paths(kind) {
                assert!(PathBuf::from(&p).exists(), "{p} was offered but is absent");
            }
        }
    }
}
