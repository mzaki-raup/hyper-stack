//! Signing in to a registry, and adding a Helm chart repository
//! (owner's request, 2026-10-24).
//!
//! **The password never goes in the command line.** `docker login
//! --password <pw>` works and is a mistake: an argument list is
//! readable by anything that can see the process table, and the CLI
//! itself warns about it. Both commands here take the password on
//! **stdin** instead, which is what `--password-stdin` and Helm's
//! `--password-stdin` exist for.
//!
//! That rules out `run::output`, which closes stdin, so these have
//! their own runner.

use crate::cli::{Engine, Invocation};
use crate::run::{Error, Result};

/// `docker login`, with the password still to come on stdin.
///
/// An empty host is **left empty**: that is how the CLI is told Docker
/// Hub, and inventing a hostname for it would sign in somewhere else.
pub fn login(engine: &Engine, host: &str, user: &str) -> Invocation {
    let mut args: Vec<String> = vec![
        "login".into(),
        "--username".into(),
        user.to_string(),
        "--password-stdin".into(),
    ];
    let host = host.trim();
    if !host.is_empty() {
        args.push(host.to_string());
    }
    engine.invoke(args)
}

/// `docker logout`, which takes no password at all.
pub fn logout(engine: &Engine, host: &str) -> Invocation {
    let host = host.trim();
    if host.is_empty() {
        engine.invoke(["logout"])
    } else {
        engine.invoke(["logout", host])
    }
}

/// `helm repo add`, with the password to come on stdin when the
/// repository needs one.
///
/// Helm is its own binary rather than a subcommand of the engine, so
/// this does not go through `Engine`: a chart repository is a Helm
/// thing and has nothing to do with which container runtime is in use.
pub fn helm_repo_add(name: &str, url: &str, user: &str) -> Invocation {
    let mut args: Vec<String> = vec![
        "repo".into(),
        "add".into(),
        name.to_string(),
        url.to_string(),
    ];
    if !user.trim().is_empty() {
        args.push("--username".into());
        args.push(user.to_string());
        args.push("--password-stdin".into());
    }
    Invocation {
        program: "helm".into(),
        args,
        remote_ssh: None,
    }
}

/// Run an invocation that is handed a secret on stdin.
///
/// Blocking, like the rest of the driver: the caller runs it as a Job.
/// The secret is written and the pipe closed before the output is
/// read, or a command waiting on end-of-input would never return.
pub fn run_with_secret(inv: &Invocation, env: &[(String, String)], secret: &str) -> Result<String> {
    use std::io::Write;

    if let Some(conn) = &inv.remote_ssh {
        return Err(Error::Remote(conn.clone()));
    }
    let mut cmd = hs_core::paths::background_command(&inv.program);
    cmd.args(&inv.args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            Error::NotInstalled(inv.program.clone())
        } else {
            Error::Unreachable(e.to_string())
        }
    })?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Unreachable("could not write to the command".into()))?;
        stdin
            .write_all(secret.as_bytes())
            .map_err(|e| Error::Unreachable(e.to_string()))?;
        // Dropping the handle closes the pipe, which is what tells the
        // command the password has ended.
    }
    let out = child
        .wait_with_output()
        .map_err(|e| Error::Unreachable(e.to_string()))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let message = if stderr.trim().is_empty() {
            format!("{} exited with {}", inv.program, out.status)
        } else {
            stderr.trim().to_string()
        };
        return Err(Error::Refused(message));
    }
    // `docker login` says "Login Succeeded" on stdout and puts its
    // warnings on stderr; both are worth showing.
    let mut text = stdout;
    if !stderr.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(stderr.trim());
    }
    Ok(text)
}

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

    /// The password must never appear in the argument list: a process
    /// table is readable, and the CLI warns about exactly this.
    #[test]
    fn the_password_is_never_an_argument() {
        let line = login(&engine(), "ghcr.io", "ada").to_line();
        assert!(line.contains("--password-stdin"), "{line}");
        assert!(!line.contains("--password "), "{line}");
        assert!(line.contains("--username ada"), "{line}");

        let helm = helm_repo_add("charts", "https://charts.example.com", "ada").to_line();
        assert!(helm.contains("--password-stdin"), "{helm}");
        assert!(!helm.contains("--password "), "{helm}");
    }

    /// An empty host is how the CLI is told Docker Hub. Filling one in
    /// would sign in to a different registry than the one meant.
    #[test]
    fn an_empty_host_stays_empty_for_docker_hub() {
        let line = login(&engine(), "  ", "ada").to_line();
        assert!(line.ends_with("--password-stdin"), "{line}");
        assert!(logout(&engine(), "").to_line().ends_with("logout"));
        assert!(
            logout(&engine(), "ghcr.io")
                .to_line()
                .ends_with("logout ghcr.io")
        );
    }

    /// A public chart repository needs no credentials, and asking for
    /// them on stdin would hang waiting for a password nobody has.
    #[test]
    fn a_public_chart_repo_asks_for_no_password() {
        let line = helm_repo_add("stable", "https://charts.example.com", "").to_line();
        assert!(!line.contains("--password-stdin"), "{line}");
        assert!(!line.contains("--username"), "{line}");
        assert!(line.starts_with("helm repo add stable"), "{line}");
    }

    /// Helm is its own binary: a chart repository has nothing to do
    /// with which container runtime is configured.
    #[test]
    fn helm_is_not_the_container_engine() {
        let inv = helm_repo_add("a", "https://b", "");
        assert_eq!(inv.program, "helm");
        assert!(inv.remote_ssh.is_none());
    }
}
