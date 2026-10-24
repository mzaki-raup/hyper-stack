//! Scan Local: find the engines and clusters on this machine without
//! being told where they are (`docs/tools/containers.md` §10.5).
//!
//! Three rules, borrowed from the Logs service scan which learned them
//! the hard way:
//!
//! - **It never starts, configures or changes anything.** Every probe is
//!   a read, and a runtime that is not installed contributes nothing.
//! - **A guess is checked before it is offered.** A socket path that
//!   does not exist is not a finding.
//! - **It is a Job.** Everything here blocks - it stats files and runs
//!   `wsl.exe` - so the caller runs it off the UI thread.

use crate::profile::{ContainerFields, Transport};

/// Something worth offering as a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// The connection kind this would become.
    pub kind: &'static str,
    /// A name to suggest, which the user can change.
    pub name: String,
    /// What was found, for the row's subtitle.
    pub detail: String,
    /// How to reach it.
    pub transport: Transport,
    /// The socket, for a local engine.
    pub socket: String,
    /// The distribution, for a WSL engine.
    pub distro: String,
    /// The kubeconfig path, for a cluster.
    pub kubeconfig: String,
    /// The context, for a cluster.
    pub context: String,
}

impl Found {
    /// The fields a new profile would carry. The caller still saves it
    /// through the normal form, so nothing is created behind the user's
    /// back.
    pub fn fields(&self) -> ContainerFields {
        ContainerFields {
            transport: self.transport,
            socket: self.socket.clone(),
            distro: self.distro.clone(),
            kubeconfig_path: self.kubeconfig.clone(),
            context: self.context.clone(),
            ..Default::default()
        }
    }

    /// A stable key, so a second scan does not offer what is already
    /// saved and two scans do not offer the same thing twice.
    pub fn key(&self) -> String {
        key_of(self.kind, &self.fields())
    }
}

/// The same key for a host that is already saved, so a scan can say
/// "already added" instead of offering a duplicate.
///
/// One definition for both sides on purpose: two functions that must
/// agree about identity are two functions that will stop agreeing.
pub fn key_of(kind: &str, fields: &ContainerFields) -> String {
    match fields.transport {
        Transport::Wsl => format!("{kind}:wsl:{}", fields.distro),
        _ if kind == "k8s" => {
            format!("k8s:{}:{}", fields.kubeconfig_path, fields.context)
        }
        _ => format!("{kind}:local:{}", fields.socket),
    }
}

/// Where a local engine's socket lives on each platform. Checked for
/// existence before being offered.
#[cfg(windows)]
const LOCAL_SOCKETS: &[(&str, &str)] = &[
    ("docker", r"\\.\pipe\docker_engine"),
    ("podman", r"\\.\pipe\podman-machine-default"),
];
#[cfg(not(windows))]
const LOCAL_SOCKETS: &[(&str, &str)] = &[
    ("docker", "/var/run/docker.sock"),
    ("podman", "/run/podman/podman.sock"),
];

/// Everything this machine offers. Blocking; run it as a Job.
pub fn scan() -> Vec<Found> {
    let mut out = Vec::new();
    out.extend(local_engines());
    out.extend(wsl_engines());
    out.extend(clusters());
    // A stable order, so a second scan does not reshuffle the list under
    // the pointer.
    out.sort_by_key(|f| f.key());
    out.dedup_by(|a, b| a.key() == b.key());
    out
}

/// An engine on this machine, by its socket.
fn local_engines() -> Vec<Found> {
    LOCAL_SOCKETS
        .iter()
        .filter(|(_, path)| socket_exists(path))
        .map(|(kind, path)| Found {
            kind,
            name: format!("Local {}", label(kind)),
            detail: (*path).to_string(),
            transport: Transport::Local,
            socket: (*path).to_string(),
            distro: String::new(),
            kubeconfig: String::new(),
            context: String::new(),
        })
        .collect()
}

/// A Windows named pipe is not a file `exists()` can see in every case,
/// so the two are checked differently.
fn socket_exists(path: &str) -> bool {
    #[cfg(windows)]
    {
        if let Some(name) = path
            .strip_prefix(r"\\.\pipe\")
            .or_else(|| path.strip_prefix("//./pipe/"))
        {
            // The pipe directory can be listed, which is the only
            // reliable way to ask whether a named pipe is there.
            return std::fs::read_dir(r"\\.\pipe\")
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name))
                })
                .unwrap_or(false);
        }
    }
    std::path::Path::new(path).exists()
}

/// Engines inside WSL distributions, which on Windows is where the
/// engine usually is (owner's report, 2026-10-23).
fn wsl_engines() -> Vec<Found> {
    let mut out = Vec::new();
    for distro in wsl_distros() {
        for kind in ["docker", "podman"] {
            // Ask the distribution whether the binary is there, rather
            // than assuming: a distro without Docker must contribute
            // nothing.
            if !wsl_has(&distro, kind) {
                continue;
            }
            out.push(Found {
                kind: if kind == "podman" { "podman" } else { "docker" },
                name: format!("{} in {distro}", label(kind)),
                detail: format!("WSL · {distro}"),
                transport: Transport::Wsl,
                socket: String::new(),
                distro: distro.clone(),
                kubeconfig: String::new(),
                context: String::new(),
            });
        }
    }
    out
}

/// The installed WSL distributions, or nothing when WSL is absent.
pub fn wsl_distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let Some(text) = run("wsl.exe", &["-l", "-q"]) else {
        return Vec::new();
    };
    text.lines()
        .map(|l| l.trim().trim_matches('\u{0}').trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn wsl_has(distro: &str, program: &str) -> bool {
    run(
        "wsl.exe",
        &[
            "-d",
            distro,
            "--",
            "sh",
            "-lc",
            &format!("command -v {program}"),
        ],
    )
    .is_some_and(|out| !out.trim().is_empty())
}

/// Kubernetes contexts from the kubeconfig, if there is one.
fn clusters() -> Vec<Found> {
    let Some(path) = kubeconfig_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let shown = path.to_string_lossy().to_string();
    contexts_in(&text)
        .into_iter()
        .map(|context| Found {
            kind: "k8s",
            name: context.clone(),
            detail: format!("{shown} · {context}"),
            transport: Transport::Local,
            socket: String::new(),
            distro: String::new(),
            kubeconfig: shown.clone(),
            context,
        })
        .collect()
}

/// `KUBECONFIG` first, as every Kubernetes tool reads it, then the
/// conventional location. Only the first entry of a `KUBECONFIG` list is
/// taken: merging several is the cluster tooling's job, not a scan's.
pub fn kubeconfig_path() -> Option<std::path::PathBuf> {
    if let Some(from_env) = std::env::var_os("KUBECONFIG") {
        let first = std::env::split_paths(&from_env).find(|p| p.is_file());
        if let Some(p) = first {
            return Some(p);
        }
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let p = std::path::Path::new(&home).join(".kube").join("config");
    p.is_file().then_some(p)
}

/// The context names in a kubeconfig.
///
/// Read line by line rather than with a YAML parser: this crate has no
/// YAML dependency, the shape is fixed by the kubeconfig schema, and a
/// scan that offers one context too few is better than a dependency
/// added for a list of names. Anything the user disagrees with is
/// corrected in the form.
pub fn contexts_in(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_contexts = false;
    for line in yaml.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        // A top-level key ends the contexts block.
        if !trimmed.starts_with([' ', '\t', '-']) {
            in_contexts = trimmed.starts_with("contexts:");
            continue;
        }
        if !in_contexts {
            continue;
        }
        // `- name: staging`, or a `name:` under a `- context:` entry.
        let body = trimmed.trim_start().trim_start_matches("- ").trim_start();
        if let Some(rest) = body.strip_prefix("name:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !name.is_empty() && !out.iter().any(|x| x == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn label(kind: &str) -> &'static str {
    match kind {
        "podman" => "Podman",
        _ => "Docker",
    }
}

/// Run a read-only probe. Windowless, so no console ever appears.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = hs_core::paths::background_command(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(decode(&out.stdout))
}

/// `wsl.exe` writes its *own* output as UTF-16 while the commands it
/// runs write UTF-8, so reading the first as UTF-8 gives a string of
/// NULs. The encoding is detected rather than assumed (the same lesson
/// `tool_logs::services` learned).
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

#[cfg(test)]
mod tests {
    use super::*;

    const KUBECONFIG: &str = r#"apiVersion: v1
kind: Config
current-context: staging
clusters:
- cluster:
    server: https://one.example.com
  name: one-cluster
contexts:
- context:
    cluster: one-cluster
    namespace: shop
  name: staging
- context:
    cluster: two-cluster
  name: "production"
users:
- name: not-a-context
"#;

    /// The context names, and nothing from the neighbouring blocks: a
    /// cluster's name and a user's name are both `name:` too.
    #[test]
    fn only_the_context_names_are_read() {
        let got = contexts_in(KUBECONFIG);
        assert_eq!(got, ["staging", "production"]);
        assert!(
            !got.iter().any(|c| c == "one-cluster"),
            "a cluster is not a context"
        );
        assert!(
            !got.iter().any(|c| c == "not-a-context"),
            "a user is not a context"
        );
    }

    #[test]
    fn a_quoted_name_loses_its_quotes() {
        let got = contexts_in("contexts:\n- name: \"with spaces\"\n");
        assert_eq!(got, ["with spaces"]);
        let single = contexts_in("contexts:\n- name: 'quoted'\n");
        assert_eq!(single, ["quoted"]);
    }

    #[test]
    fn a_kubeconfig_with_no_contexts_yields_nothing() {
        assert!(contexts_in("apiVersion: v1\nkind: Config\n").is_empty());
        assert!(contexts_in("").is_empty());
        assert!(contexts_in("contexts: []\n").is_empty());
    }

    /// A comment must not be read as a context, and a repeated name must
    /// not be offered twice.
    #[test]
    fn comments_are_skipped_and_names_are_not_repeated() {
        let text = "contexts:\n# - name: commented-out\n- name: real\n- name: real\n";
        assert_eq!(contexts_in(text), ["real"]);
    }

    /// Each finding has to turn into a profile that would actually
    /// connect, or the scan is a list of things that do not work.
    #[test]
    fn a_wsl_finding_becomes_a_wsl_profile() {
        let f = Found {
            kind: "docker",
            name: "Docker in Ubuntu-24.04".into(),
            detail: "WSL · Ubuntu-24.04".into(),
            transport: Transport::Wsl,
            socket: String::new(),
            distro: "Ubuntu-24.04".into(),
            kubeconfig: String::new(),
            context: String::new(),
        };
        let fields = f.fields();
        assert_eq!(fields.transport, Transport::Wsl);
        assert_eq!(fields.distro, "Ubuntu-24.04");
        // And the command it would build actually names the distro.
        let engine = crate::cli::Engine::new(f.kind, fields);
        let inv = engine.list_containers();
        assert_eq!(inv.program, "wsl.exe");
        assert!(inv.args.contains(&"Ubuntu-24.04".to_string()));
    }

    #[test]
    fn a_cluster_finding_keeps_its_path_and_context() {
        let f = Found {
            kind: "k8s",
            name: "staging".into(),
            detail: "~/.kube/config · staging".into(),
            transport: Transport::Local,
            socket: String::new(),
            distro: String::new(),
            kubeconfig: "/home/me/.kube/config".into(),
            context: "staging".into(),
        };
        let fields = f.fields();
        assert_eq!(fields.kubeconfig_path, "/home/me/.kube/config");
        assert_eq!(fields.context, "staging");
        assert!(
            fields.kubeconfig_inline.is_empty(),
            "a scan references the file, it does not copy it into the vault"
        );
    }

    /// Two scans must not offer the same host twice, and the keys have
    /// to tell different things apart.
    #[test]
    fn the_key_identifies_a_finding_without_colliding() {
        let wsl_a = Found {
            kind: "docker",
            distro: "Ubuntu-24.04".into(),
            transport: Transport::Wsl,
            ..blank()
        };
        let wsl_b = Found {
            distro: "Debian".into(),
            ..wsl_a.clone()
        };
        let podman = Found {
            kind: "podman",
            ..wsl_a.clone()
        };
        let keys = [wsl_a.key(), wsl_b.key(), podman.key()];
        let mut uniq = keys.clone().to_vec();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 3, "keys collided: {keys:?}");
        // The same thing found twice is one key.
        assert_eq!(wsl_a.key(), wsl_a.clone().key());
    }

    fn blank() -> Found {
        Found {
            kind: "docker",
            name: String::new(),
            detail: String::new(),
            transport: Transport::Local,
            socket: String::new(),
            distro: String::new(),
            kubeconfig: String::new(),
            context: String::new(),
        }
    }

    /// A scan must never invent a host: a socket that is not there is
    /// not a finding. This runs on whatever machine the tests are on,
    /// so it asserts the invariant rather than a count.
    #[test]
    fn nothing_offered_points_at_something_absent() {
        for f in local_engines() {
            assert!(
                socket_exists(&f.socket),
                "offered a socket that is not there: {}",
                f.socket
            );
        }
    }

    /// Whatever this machine has, the result must be ordered and free of
    /// duplicates, or the list jumps about between scans.
    #[test]
    fn the_result_is_stable_and_has_no_duplicates() {
        let first = scan();
        let again = scan();
        assert_eq!(
            first.iter().map(Found::key).collect::<Vec<_>>(),
            again.iter().map(Found::key).collect::<Vec<_>>(),
            "two scans disagreed"
        );
        let mut keys: Vec<String> = first.iter().map(Found::key).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), before, "the same host was offered twice");
    }
}
