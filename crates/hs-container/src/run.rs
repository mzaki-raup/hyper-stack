//! Running an [`Invocation`] and reading the answer
//! (`docs/tools/containers.md` §10.3).
//!
//! Blocking by design, like every other driver here: the caller runs it
//! as a Job, and the UI thread never waits (ADR-013). A local command
//! goes through `hs_core::paths::background_command`, so **no console
//! window ever appears** — the owner's rule of 2026-10-24.
//!
//! A remote engine is not run here. `Invocation::remote_ssh` names the
//! saved connection, and the caller runs `Invocation::to_line()` through
//! `hs_ssh::exec`: this crate must not depend on `hs-ssh`, or the
//! dependency would run the wrong way round.

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
    /// This invocation has to be run somewhere else.
    Remote(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotInstalled(p) => write!(f, "{p} was not found on PATH"),
            Error::Unreachable(m) => write!(f, "could not reach the engine: {m}"),
            Error::Refused(m) => write!(f, "{m}"),
            Error::Remote(c) => write!(f, "this host is reached through {c}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Run a local invocation and give back its stdout.
///
/// A non-zero exit is an error carrying **stderr**, because that is
/// where an engine says what was wrong; returning the empty stdout
/// instead is how a tool ends up reporting "no containers" for "no
/// permission on the socket".
pub fn output(inv: &Invocation, env: &[(String, String)]) -> Result<String> {
    if let Some(conn) = &inv.remote_ssh {
        return Err(Error::Remote(conn.clone()));
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
        match containers(&e) {
            Err(Error::Remote(conn)) => assert_eq!(conn, "web01"),
            other => panic!("expected a Remote error, got {other:?}"),
        }
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
        assert_eq!(
            Error::NotInstalled("docker".into()).to_string(),
            "docker was not found on PATH"
        );
        assert!(
            Error::Unreachable("no socket".into())
                .to_string()
                .starts_with("could not reach the engine")
        );
        // A refusal is the engine's own words, unwrapped.
        assert_eq!(
            Error::Refused("No such container: nope".into()).to_string(),
            "No such container: nope"
        );
    }
}
