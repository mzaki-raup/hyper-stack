//! Running an [`Invocation`] and reading the answer
//! (`docs/tools/containers.md` §10.3).
//!
//! Blocking by design, like every other driver here: the caller runs it
//! as a Job, and the UI thread never waits (ADR-013). A local command
//! goes through `hs_core::paths::background_command`, so **no console
//! window ever appears** — the owner's rule of 2026-10-24.
//!
//! A remote engine is not run here either. `Invocation::remote_ssh` names
//! the saved connection, and `Invocation::to_line()` goes to the runner
//! the binary installs with [`set_remote`] (the SSH service's exec): this
//! crate must not depend on `hs-ssh`, or the dependency would run the
//! wrong way round. With no runner installed, a remote invocation is
//! refused rather than run against the local engine (BUG-0070: before
//! the runner, every SSH host failed this way).

use crate::cli::{Engine, Invocation};
use crate::model;

/// What went wrong, in terms a pane can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The engine's binary is not on PATH. The commonest failure by
    /// far, and the one with the clearest remedy, so it is its own case.
    NotInstalled(String),
    /// The engine is there but would not answer: not running, no
    /// permission on the socket, the wrong host.
    Unreachable(String),
    /// It answered, and said no.
    Refused(String),
    /// Running it on its SSH host failed before the engine answered (no
    /// runner, no connection, the vault locked), in these words.
    Remote(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Sentences, as every pane shows them (BUG-0070).
        match self {
            Error::NotInstalled(p) => write!(
                f,
                "The {p} command was not found on this computer's PATH. Install the {p} client: a remote engine over TCP needs it here too, or reach the host over SSH instead."
            ),
            Error::Unreachable(m) => write!(f, "Could not reach the engine: {m}"),
            Error::Refused(m) => write!(f, "{m}"),
            Error::Remote(m) => write!(f, "Could not run the command over SSH: {m}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// What a remote command answered: stdout, stderr and its exit status.
pub type RemoteAnswer = std::result::Result<(String, String, u32), String>;

/// Runs one shell line on a saved SSH connection: `(connection id, line)`.
pub type RemoteRunner = dyn Fn(&str, &str) -> RemoteAnswer + Send + Sync;

static REMOTE: std::sync::RwLock<Option<std::sync::Arc<RemoteRunner>>> =
    std::sync::RwLock::new(None);

/// Install the runner for SSH hosts. The binary does this once at start-up
/// with the SSH service; tests and `--self-test` leave it unset.
pub fn set_remote(runner: std::sync::Arc<RemoteRunner>) {
    if let Ok(mut slot) = REMOTE.write() {
        *slot = Some(runner);
    }
}

fn remote_runner() -> Option<std::sync::Arc<RemoteRunner>> {
    REMOTE.read().ok().and_then(|r| r.clone())
}

/// The line an SSH host runs: the engine's environment as assignments
/// before the command, which is how a POSIX shell takes them.
fn remote_line(inv: &Invocation, env: &[(String, String)]) -> String {
    let mut line = String::new();
    for (k, v) in env {
        line.push_str(&format!("{k}={} ", crate::cli::shell_quote(v)));
    }
    line.push_str(&inv.to_line());
    line
}

/// A remote invocation through `runner`, its answer read as a local one is.
fn remote_output<F: Fn(&str, &str) -> RemoteAnswer + ?Sized>(
    runner: &F,
    conn: &str,
    inv: &Invocation,
    env: &[(String, String)],
    with_stderr: bool,
) -> Result<String> {
    let (stdout, stderr, status) = runner(conn, &remote_line(inv, env)).map_err(Error::Remote)?;
    if status != 0 {
        let stderr = stderr.trim();
        // 127 is a POSIX shell's "command not found": the engine is not
        // installed on that host, which deserves its own words.
        if status == 127 {
            return Err(Error::Refused(format!(
                "The {} command was not found on the SSH host: install the engine there, or check its PATH for a non-interactive login.",
                inv.program
            )));
        }
        let message = if stderr.is_empty() {
            format!("{} exited with {status}", inv.program)
        } else {
            stderr.to_string()
        };
        return Err(if looks_unreachable(&message.to_ascii_lowercase()) {
            Error::Unreachable(message)
        } else {
            Error::Refused(message)
        });
    }
    let mut text = stdout;
    if with_stderr && !stderr.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(text)
}

/// A remote invocation through the installed runner, or refused when
/// there is none (never run against the local engine instead).
fn remote(
    conn: &str,
    inv: &Invocation,
    env: &[(String, String)],
    with_stderr: bool,
) -> Result<String> {
    match remote_runner() {
        Some(r) => remote_output(r.as_ref(), conn, inv, env, with_stderr),
        None => Err(Error::Remote(
            "SSH is not available in this process".to_string(),
        )),
    }
}

/// Run an invocation and give back its stdout.
///
/// A non-zero exit is an error carrying **stderr**, because that is
/// where an engine says what was wrong; returning the empty stdout
/// instead is how a tool ends up reporting "no containers" for "no
/// permission on the socket".
pub fn output(inv: &Invocation, env: &[(String, String)]) -> Result<String> {
    if let Some(conn) = &inv.remote_ssh {
        return remote(conn, inv, env, false);
    }
    let mut cmd = hs_core::paths::background_command(&inv.program);
    cmd.args(&inv.args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null());
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotInstalled(inv.program.clone())
        } else {
            Error::Unreachable(e.to_string())
        }
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("{} exited with {}", inv.program, out.status)
        } else {
            stderr
        };
        // The engine's own words decide which failure this is: the
        // remedies are different and the pane should say the right one.
        let lower = message.to_ascii_lowercase();
        return Err(if looks_unreachable(&lower) {
            Error::Unreachable(message)
        } else {
            Error::Refused(message)
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run an invocation whose **useful output is on stderr**, and return
/// both streams.
///
/// `docker compose` and `docker stack deploy` write their progress -
/// "Container shop-db-1  Created", "Creating service shop_web" - to
/// stderr, keeping stdout for machine-readable output they mostly do
/// not produce. A caller that read only stdout would report a
/// successful deploy as an empty string and look like it had done
/// nothing. The listings keep using [`output`], because they parse
/// stdout as JSON and must not have progress text mixed into it.
pub fn output_with_stderr(inv: &Invocation, env: &[(String, String)]) -> Result<String> {
    if let Some(conn) = &inv.remote_ssh {
        return remote(conn, inv, env, true);
    }
    let mut cmd = hs_core::paths::background_command(&inv.program);
    cmd.args(&inv.args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::null());
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotInstalled(inv.program.clone())
        } else {
            Error::Unreachable(e.to_string())
        }
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let message = if stderr.trim().is_empty() {
            format!("{} exited with {}", inv.program, out.status)
        } else {
            stderr.trim().to_string()
        };
        let lower = message.to_ascii_lowercase();
        return Err(if looks_unreachable(&lower) {
            Error::Unreachable(message)
        } else {
            Error::Refused(message)
        });
    }
    let mut text = stdout.into_owned();
    if !stderr.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(text)
}

/// Whether the engine's complaint means "I could not talk to a daemon"
/// rather than "I understood and refused".
fn looks_unreachable(lower: &str) -> bool {
    const SIGNS: &[&str] = &[
        "cannot connect to the docker daemon",
        "is the docker daemon running",
        "permission denied while trying to connect",
        "connection refused",
        "no such host",
        "dial unix",
        "open //./pipe/docker_engine",
        "cannot connect to podman",
        "unable to connect to podman",
        "timeout",
    ];
    SIGNS.iter().any(|s| lower.contains(s))
}

/// The containers on a host.
pub fn containers(engine: &Engine) -> Result<Vec<model::Container>> {
    let text = output(&engine.list_containers(), &engine.env())?;
    Ok(model::parse_containers(&text))
}

pub fn images(engine: &Engine) -> Result<Vec<model::Image>> {
    let text = output(&engine.list_images(), &engine.env())?;
    Ok(model::parse_images(&text))
}

pub fn volumes(engine: &Engine) -> Result<Vec<model::Volume>> {
    let text = output(&engine.list_volumes(), &engine.env())?;
    Ok(model::parse_volumes(&text))
}

pub fn networks(engine: &Engine) -> Result<Vec<model::Network>> {
    let text = output(&engine.list_networks(), &engine.env())?;
    Ok(model::parse_networks(&text))
}

/// Whether the engine answers at all, and what it says it is. Used by
/// Test on the connection form and by the dashboard's reachable count.
pub fn probe(engine: &Engine) -> Result<String> {
    let text = output(&engine.version(), &engine.env())?;
    // The whole version document is more than anyone wants; the server's
    // version is the useful part.
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap_or_default();
    let server = parsed
        .pointer("/Server/Version")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.pointer("/Client/Version").and_then(|v| v.as_str()))
        .or_else(|| parsed.get("Version").and_then(|v| v.as_str()));
    Ok(match server {
        Some(v) => v.to_string(),
        // It answered with something unparseable, which still means it
        // is there - better to say so than to call it unreachable.
        None => text.lines().next().unwrap_or("").trim().to_string(),
    })
}

/// Carry out a verb on one container.
pub fn act(engine: &Engine, verb: crate::cli::Verb, id: &str) -> Result<()> {
    output(&engine.verb(verb, id), &engine.env())?;
    Ok(())
}

// --- Swarm (phase 5) -------------------------------------------------

/// Where this engine stands in a swarm.
///
/// An engine that cannot be reached at all is an error; one that
/// answers "not in a swarm" is not, and that distinction is the whole
/// reason this returns a `Result<SwarmState>` rather than swallowing
/// the failure into `Inactive`.
pub fn swarm_state(engine: &Engine) -> Result<crate::swarm::SwarmState> {
    let text = output(&crate::swarm::info(engine), &engine.env())?;
    Ok(crate::swarm::SwarmState::parse(&text))
}

pub fn stacks(engine: &Engine) -> Result<Vec<crate::swarm::Stack>> {
    let text = output(&crate::swarm::list_stacks(engine), &engine.env())?;
    Ok(crate::swarm::parse_stacks(&text))
}

pub fn services(engine: &Engine) -> Result<Vec<crate::swarm::Service>> {
    let text = output(&crate::swarm::list_services(engine), &engine.env())?;
    Ok(crate::swarm::parse_services(&text))
}

pub fn nodes(engine: &Engine) -> Result<Vec<crate::swarm::Node>> {
    let text = output(&crate::swarm::list_nodes(engine), &engine.env())?;
    Ok(crate::swarm::parse_nodes(&text))
}

/// The Compose projects on a host.
///
/// Two sources, because neither is enough on its own: `docker compose
/// ls` knows which projects exist and which files they came from, the
/// containers' labels know what is inside them. An engine too old for
/// `compose ls` still gets the projects from the labels alone, which
/// is why its failure is not an error here.
pub fn projects(engine: &Engine) -> Result<Vec<crate::compose::Project>> {
    let containers = containers(engine)?;
    let listed = match output(&crate::compose::list(engine), &engine.env()) {
        Ok(text) => crate::compose::parse_list(&text),
        Err(e) => {
            tracing::debug!("compose ls did not answer ({e}); reading the labels alone");
            Vec::new()
        }
    };
    Ok(crate::compose::merge(&listed, &containers))
}

/// Run one Compose verb for a project, returning what it printed.
///
/// `output_with_stderr`, because Compose's progress is on stderr and
/// stdout is usually empty - except for `config`, whose rendered YAML
/// *is* on stdout and which therefore comes back with any warnings
/// appended, which is what someone reading it wants anyway.
pub fn compose(
    engine: &Engine,
    project: &str,
    files: &[String],
    verb: crate::compose::ComposeVerb,
) -> Result<String> {
    output_with_stderr(
        &crate::compose::invocation(engine, project, files, verb),
        &engine.env(),
    )
}

/// Deploy a stack from its file(s).
pub fn deploy_stack(engine: &Engine, files: &[String], stack: &str) -> Result<String> {
    output_with_stderr(&crate::swarm::deploy(engine, files, stack), &engine.env())
}

/// Remove a stack and every service in it.
pub fn remove_stack(engine: &Engine, stack: &str) -> Result<String> {
    output_with_stderr(&crate::swarm::remove_stack(engine, stack), &engine.env())
}

/// Scale a Swarm service.
pub fn scale_service(engine: &Engine, service: &str, replicas: u32) -> Result<String> {
    output_with_stderr(
        &crate::swarm::scale(engine, service, replicas),
        &engine.env(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{ContainerFields, Transport};

    /// A remote invocation must be refused here rather than silently run
    /// against the *local* engine, which would be the worst possible
    /// outcome: the right-looking answer from the wrong machine.
    #[test]
    fn a_remote_invocation_is_refused_not_run_locally() {
        let e = Engine::new(
            "docker",
            ContainerFields {
                transport: Transport::Ssh,
                tunnel_ssh: "web01".into(),
                ..Default::default()
            },
        );
        // No test installs a runner, so this is the "no SSH here" case.
        match containers(&e) {
            Err(Error::Remote(_)) => {}
            other => panic!("expected a Remote error, got {other:?}"),
        }
    }

    /// An SSH host's commands go to the runner as one line, with the
    /// engine's environment before it, and its answer reads as a local
    /// one does (BUG-0070: before, every SSH host failed with the
    /// connection's id).
    #[test]
    fn a_remote_invocation_runs_through_the_runner() {
        let e = Engine::new(
            "docker",
            ContainerFields {
                transport: Transport::Ssh,
                tunnel_ssh: "web01".into(),
                api_version: "1.41".into(),
                ..Default::default()
            },
        );
        let seen = std::sync::Mutex::new(Vec::<(String, String)>::new());
        let runner = |conn: &str, line: &str| -> RemoteAnswer {
            seen.lock().unwrap().push((conn.into(), line.into()));
            Ok(("27.3.1\n".into(), String::new(), 0))
        };
        let inv = e.version();
        let out = remote_output(&runner, "web01", &inv, &e.env(), false).unwrap();
        assert_eq!(out, "27.3.1\n");
        let (conn, line) = seen.lock().unwrap()[0].clone();
        assert_eq!(conn, "web01");
        assert_eq!(
            line,
            "DOCKER_API_VERSION=1.41 docker version --format '{{json .}}'"
        );
    }

    #[test]
    fn a_remote_failure_reads_as_a_local_one() {
        let inv = Invocation {
            program: "docker".into(),
            args: vec!["ps".into()],
            remote_ssh: Some("web01".into()),
        };
        let missing = |_: &str, _: &str| -> RemoteAnswer {
            Ok((String::new(), "sh: docker: not found".into(), 127))
        };
        let err = remote_output(&missing, "web01", &inv, &[], false).unwrap_err();
        assert!(
            err.to_string()
                .starts_with("The docker command was not found on the SSH host")
        );
        let down = |_: &str, _: &str| -> RemoteAnswer {
            Ok((
                String::new(),
                "Cannot connect to the Docker daemon at unix:///var/run/docker.sock.".into(),
                1,
            ))
        };
        assert!(matches!(
            remote_output(&down, "web01", &inv, &[], false),
            Err(Error::Unreachable(_))
        ));
        let no_ssh = |_: &str, _: &str| -> RemoteAnswer { Err("the vault is locked".into()) };
        assert_eq!(
            remote_output(&no_ssh, "web01", &inv, &[], false)
                .unwrap_err()
                .to_string(),
            "Could not run the command over SSH: the vault is locked"
        );
    }

    /// A missing binary is its own case, because its remedy is its own.
    #[test]
    fn a_missing_binary_says_so() {
        let inv = Invocation {
            program: "definitely-not-a-real-engine-binary".into(),
            args: vec!["ps".into()],
            remote_ssh: None,
        };
        match output(&inv, &[]) {
            Err(Error::NotInstalled(p)) => {
                assert_eq!(p, "definitely-not-a-real-engine-binary");
                assert!(
                    Error::NotInstalled(p).to_string().contains("PATH"),
                    "the message names the remedy"
                );
            }
            other => panic!("expected NotInstalled, got {other:?}"),
        }
    }

    /// "I could not reach a daemon" and "I understood and refused" need
    /// different remedies, so they are different errors.
    #[test]
    fn an_unreachable_daemon_is_told_from_a_refusal() {
        for message in [
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
            "permission denied while trying to connect to the Docker daemon socket",
            "dial unix /var/run/docker.sock: connect: connection refused",
            "error during connect: Get \"http://%2F%2F.%2Fpipe%2Fdocker_engine/v1.47/version\": open //./pipe/docker_engine: The system cannot find the file specified.",
        ] {
            assert!(
                looks_unreachable(&message.to_ascii_lowercase()),
                "should read as unreachable: {message}"
            );
        }
        for message in [
            "Error response from daemon: No such container: nope",
            "invalid reference format",
            "You cannot remove a running container",
        ] {
            assert!(
                !looks_unreachable(&message.to_ascii_lowercase()),
                "should read as a refusal: {message}"
            );
        }
    }

    #[test]
    fn the_errors_read_as_sentences() {
        // Each starts with a capital (BUG-0070) and the missing client
        // names both remedies.
        let missing = Error::NotInstalled("docker".into()).to_string();
        assert!(missing.starts_with("The docker command was not found on this computer's PATH."));
        assert!(missing.contains("over TCP") && missing.contains("over SSH"));
        assert!(
            Error::Unreachable("no socket".into())
                .to_string()
                .starts_with("Could not reach the engine")
        );
        // A refusal is the engine's own words, unwrapped.
        assert_eq!(
            Error::Refused("No such container: nope".into()).to_string(),
            "No such container: nope"
        );
    }
}
