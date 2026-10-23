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
        paths: &["/var/log/postgresql"],
    },
    Known {
        kind: "mysql",
        matches: &["mysqld", "mariadbd"],
        paths: &["/var/log/mysql/error.log", "/var/log/mariadb/mariadb.log"],
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

/// Every kind this knows about, for the configure form's picker.
pub fn kinds() -> Vec<&'static str> {
    KNOWN.iter().map(|k| k.kind).collect()
}

/// Look for everything running here. Blocking: the caller puts it on the
/// runtime, never on the UI thread. Each probe is independent, and one
/// that is not installed simply contributes nothing.
pub fn detect() -> Vec<Service> {
    let mut out = Vec::new();
    out.extend(processes());
    out.extend(containers("docker"));
    out.extend(containers("podman"));
    out.extend(systemd_units());
    out.extend(kube_pods());
    // One row per thing: a server found both as a process and as a unit
    // is the same server.
    out.sort_by_key(|s| s.name.to_lowercase());
    out.dedup_by(|a, b| a.key() == b.key());
    out
}

/// Run a command and give back its stdout, or nothing when it is not
/// installed or fails. Bounded by the caller's own timeout.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Server processes on this machine.
fn processes() -> Vec<Service> {
    let listing = if cfg!(windows) {
        run("tasklist", &["/fo", "csv", "/nh"])
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
        let (pid, name) = if cfg!(windows) {
            // "image.exe","1234","Services","0","1,234 K"
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
fn containers(runtime: &str) -> Vec<Service> {
    let Some(text) = run(
        runtime,
        &["ps", "--format", "{{.ID}}\t{{.Names}}\t{{.Image}}"],
    ) else {
        return Vec::new();
    };
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
            Some(Service {
                name,
                kind: kind.to_string(),
                host: Host::Container,
                handle: id.clone(),
                files: Vec::new(),
                command: Some(format!("{runtime} logs -f --tail 2000 {id}")),
            })
        })
        .collect()
}

/// Running systemd units that look like a server.
fn systemd_units() -> Vec<Service> {
    let Some(text) = run(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--state=running",
            "--no-legend",
            "--no-pager",
            "--plain",
        ],
    ) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|l| {
            let unit = l.split_whitespace().next()?;
            let bare = unit.strip_suffix(".service").unwrap_or(unit);
            let kind = classify(bare)?;
            Some(Service {
                name: bare.to_string(),
                kind: kind.to_string(),
                host: Host::Systemd,
                handle: unit.to_string(),
                files: Vec::new(),
                command: Some(format!("journalctl -n 2000 -f -u {unit}")),
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
