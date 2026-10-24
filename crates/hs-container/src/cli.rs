//! Turning a profile into a command to run
//! (`docs/tools/containers.md` §10.3).
//!
//! Every transport in §10.5 reaches the same `docker`/`podman` CLI; only
//! the way there differs. Building the argv is kept **pure** and
//! testable, because the mistakes here are silent: a `-H` left off talks
//! to the wrong engine, and a WSL prefix in the wrong place runs the
//! wrong program entirely.
//!
//! Running the command is the caller's job, through
//! `hs_core::paths::background_command` for a local invocation (no
//! console window ever appears — owner's rule) or `hs_ssh::exec` for a
//! remote one.

use crate::profile::{ContainerFields, Transport};

/// A command to run, as the pieces a runner needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The program to start. `wsl.exe` for a WSL engine, otherwise the
    /// engine's own binary.
    pub program: String,
    pub args: Vec<String>,
    /// When set, this is **not** a local command: run it on this saved
    /// `ssh` profile instead, as one shell line.
    pub remote_ssh: Option<String>,
}

impl Invocation {
    /// The command as one shell line, which is what an SSH exec channel
    /// takes and what the status line shows.
    ///
    /// Arguments containing spaces are quoted, because a container name
    /// or a Go template certainly will.
    pub fn to_line(&self) -> String {
        let mut out = String::new();
        for part in std::iter::once(&self.program).chain(self.args.iter()) {
            if !out.is_empty() {
                out.push(' ');
            }
            if part.is_empty() || part.contains([' ', '"', '\'', '\t']) {
                out.push('\'');
                out.push_str(&part.replace('\'', r"'\''"));
                out.push('\'');
            } else {
                out.push_str(part);
            }
        }
        out
    }
}

/// Builds invocations for one host.
#[derive(Debug, Clone, PartialEq)]
pub struct Engine {
    /// `docker` or `podman`. A Windows-container host is still `docker`:
    /// the engine is the same binary pointed at a Windows daemon.
    program: String,
    fields: ContainerFields,
}

impl Engine {
    /// The engine for a profile. `kind` picks the binary; everything
    /// else comes from the fields.
    pub fn new(kind: &str, fields: ContainerFields) -> Self {
        let program = match kind {
            "podman" => "podman",
            _ => "docker",
        };
        Self {
            program: program.to_string(),
            fields,
        }
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    pub fn fields(&self) -> &ContainerFields {
        &self.fields
    }

    /// Wrap engine arguments in whatever the transport needs.
    ///
    /// The order matters and is easy to get wrong: the WSL prefix comes
    /// **before** the program, and `-H`/TLS flags come **before** the
    /// subcommand, because the engine CLI only accepts them there.
    pub fn invoke<I, S>(&self, args: I) -> Invocation
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let tail: Vec<String> = args.into_iter().map(Into::into).collect();
        let mut global = self.global_args();
        global.extend(tail);
        match self.fields.transport {
            // `wsl.exe -d <distro> -- docker …`: the distribution is
            // addressed before the program, and `--` keeps wsl from
            // reading the engine's own flags.
            Transport::Wsl => {
                let mut args = vec!["-d".to_string()];
                args.push(self.fields.distro.clone());
                args.push("--".to_string());
                args.push(self.program.clone());
                args.extend(global);
                Invocation {
                    program: "wsl.exe".to_string(),
                    args,
                    remote_ssh: None,
                }
            }
            // The engine on another machine: the same command, run
            // there. No `-H`, because it is local from that side.
            Transport::Ssh => Invocation {
                program: self.program.clone(),
                args: global,
                remote_ssh: Some(self.fields.tunnel_ssh.clone()),
            },
            _ => Invocation {
                program: self.program.clone(),
                args: global,
                remote_ssh: None,
            },
        }
    }

    /// The flags that must precede any subcommand: where the engine is,
    /// and how to trust it.
    fn global_args(&self) -> Vec<String> {
        let mut out = Vec::new();
        match self.fields.transport {
            Transport::Local => {
                // Only when the socket was given: the default is right
                // on every normal install, and naming it wrongly is a
                // confusing way to fail.
                if !self.fields.socket.is_empty() {
                    out.push("-H".into());
                    out.push(host_arg(&self.fields.socket));
                }
            }
            Transport::Tcp => {
                out.push("-H".into());
                // Always `tcp://`: TLS is signalled by the flags below,
                // not by the scheme.
                out.push(format!("tcp://{}:{}", self.fields.host, self.fields.port));
                if self.fields.tls {
                    // `--tlsverify` *and* the three files: without the
                    // flag the certificates are ignored, which looks
                    // like TLS working when it is not.
                    if !self.fields.insecure_tls {
                        out.push("--tlsverify".into());
                    } else {
                        out.push("--tls".into());
                    }
                    for (flag, path) in [
                        ("--tlscacert", &self.fields.tls_ca),
                        ("--tlscert", &self.fields.tls_cert),
                        ("--tlskey", &self.fields.tls_key),
                    ] {
                        if !path.is_empty() {
                            out.push(flag.into());
                            out.push(path.clone());
                        }
                    }
                }
            }
            // Nothing: WSL and SSH run the engine where it already lives.
            Transport::Ssh | Transport::Wsl => {}
        }
        out
    }

    /// Environment variables the runner must set for this host.
    ///
    /// The API version is not a flag on either engine - it is
    /// `DOCKER_API_VERSION` - so it cannot live in the argv and has to
    /// be handed over separately. A runner that ignores this will talk
    /// the wrong API version to an older daemon and fail in a way that
    /// looks like a broken engine.
    pub fn env(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if !self.fields.api_version.is_empty() {
            out.push((
                "DOCKER_API_VERSION".to_string(),
                self.fields.api_version.clone(),
            ));
        }
        out
    }

    // --- the questions a pane asks -----------------------------------

    /// Every container, running or not. `--no-trunc` because a short id
    /// is for showing, not for addressing: the pane needs the full one
    /// to act on.
    pub fn list_containers(&self) -> Invocation {
        self.invoke(["ps", "--all", "--no-trunc", "--format", JSON_LINE])
    }

    pub fn list_images(&self) -> Invocation {
        self.invoke(["images", "--no-trunc", "--format", JSON_LINE])
    }

    pub fn list_volumes(&self) -> Invocation {
        self.invoke(["volume", "ls", "--format", JSON_LINE])
    }

    pub fn list_networks(&self) -> Invocation {
        self.invoke(["network", "ls", "--no-trunc", "--format", JSON_LINE])
    }

    /// `docker version`, used to tell "the engine is there" from "the
    /// engine refused us".
    pub fn version(&self) -> Invocation {
        self.invoke(["version", "--format", "{{json .}}"])
    }

    /// Start, stop, restart, pause, unpause or kill one container by id.
    pub fn verb(&self, verb: Verb, id: &str) -> Invocation {
        self.invoke([verb.as_str(), id])
    }

    /// The command that follows a container's log, for the Logs tab to
    /// run as its source (§10.1 — the log viewer is not rebuilt here).
    ///
    /// Returned as a **line** rather than an `Invocation`, because that
    /// is what a Logs command source takes.
    pub fn follow_logs_line(&self, id: &str, tail: u32) -> String {
        // The same line either way: a Logs source for a remote host
        // runs on that connection, so the line must not name the host
        // again. `Invocation::remote_ssh` is what says where to run it.
        let line = self
            .invoke([
                "logs",
                "--follow",
                "--tail",
                &tail.to_string(),
                "--timestamps",
                id,
            ])
            .to_line();
        // `docker logs` writes the container's stdout to its stdout and
        // the container's **stderr to its own stderr**, and a Logs
        // command source reads stdout. Most images log to stderr -
        // portainer, nginx, and most Go and Java applications do - so
        // without this the tab follows a live container and shows
        // nothing at all (measured against portainer, 2026-10-24).
        format!("{line} 2>&1")
    }

    /// An interactive shell inside a container, for `hs-term` to run.
    /// `sh` is the fallback because a minimal image has no bash.
    pub fn exec_shell_line(&self, id: &str, shell: &str) -> String {
        let shell = if shell.trim().is_empty() {
            "/bin/sh"
        } else {
            shell
        };
        self.invoke(["exec", "-it", id, shell]).to_line()
    }
}

/// `docker`'s per-line JSON, which both engines accept.
const JSON_LINE: &str = "{{json .}}";

/// What can be done to a container. Each is guarded by the caller
/// against a protected environment; this only names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Restart,
    Pause,
    Unpause,
    Kill,
    Remove,
}

impl Verb {
    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Stop => "stop",
            Verb::Restart => "restart",
            Verb::Pause => "pause",
            Verb::Unpause => "unpause",
            Verb::Kill => "kill",
            Verb::Remove => "rm",
        }
    }

    /// Whether this changes the world enough to need confirming. A
    /// start does not; everything else does.
    pub fn destructive(self) -> bool {
        !matches!(self, Verb::Start | Verb::Unpause)
    }

    pub fn key(self) -> &'static str {
        match self {
            Verb::Start => "container-verb-start",
            Verb::Stop => "container-verb-stop",
            Verb::Restart => "container-verb-restart",
            Verb::Pause => "container-verb-pause",
            Verb::Unpause => "container-verb-unpause",
            Verb::Kill => "container-verb-kill",
            Verb::Remove => "container-verb-remove",
        }
    }
}

/// A socket path becomes a URL the CLI understands; something that is
/// already a URL is left alone.
fn host_arg(socket: &str) -> String {
    let s = socket.trim();
    if s.contains("://") {
        return s.to_string();
    }
    // A Windows named pipe has its own scheme.
    if s.starts_with(r"\\.\pipe\") || s.starts_with("//./pipe/") {
        return format!("npipe://{}", s.replace('\\', "/"));
    }
    format!("unix://{s}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(kind: &str, fields: ContainerFields) -> Engine {
        Engine::new(kind, fields)
    }

    #[test]
    fn a_local_engine_is_called_with_no_host_flag() {
        let e = engine("docker", ContainerFields::default());
        let inv = e.list_containers();
        assert_eq!(inv.program, "docker");
        assert_eq!(
            inv.args,
            ["ps", "--all", "--no-trunc", "--format", "{{json .}}"]
        );
        assert!(
            inv.remote_ssh.is_none(),
            "a local engine is not run over SSH"
        );
    }

    #[test]
    fn podman_is_a_different_binary_and_nothing_else() {
        let e = engine("podman", ContainerFields::default());
        assert_eq!(e.list_containers().program, "podman");
        // A Windows-container host is still the docker binary.
        assert_eq!(
            engine("wincontainer", ContainerFields::default())
                .list_containers()
                .program,
            "docker"
        );
    }

    /// A named socket becomes a URL; a path and a Windows pipe differ.
    #[test]
    fn a_socket_becomes_a_host_url() {
        let e = engine(
            "docker",
            ContainerFields {
                socket: "/var/run/docker.sock".into(),
                ..Default::default()
            },
        );
        let inv = e.list_containers();
        assert_eq!(inv.args[0], "-H");
        assert_eq!(inv.args[1], "unix:///var/run/docker.sock");

        assert_eq!(
            host_arg(r"\\.\pipe\docker_engine"),
            "npipe:////./pipe/docker_engine"
        );
        assert_eq!(host_arg("tcp://10.0.0.5:2376"), "tcp://10.0.0.5:2376");
    }

    /// The flags have to come **before** the subcommand: the engine CLI
    /// refuses them anywhere else, and the failure reads as a bad
    /// subcommand rather than a misplaced flag.
    #[test]
    fn the_host_flag_precedes_the_subcommand() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Tcp,
                host: "10.0.0.5".into(),
                port: 2376,
                ..Default::default()
            },
        );
        let inv = e.list_containers();
        assert_eq!(inv.args[0], "-H");
        assert_eq!(inv.args[1], "tcp://10.0.0.5:2376");
        assert_eq!(inv.args[2], "ps", "the subcommand comes after");
    }

    /// Without `--tlsverify` the certificates are ignored, which looks
    /// exactly like TLS working when it is not.
    #[test]
    fn tls_passes_the_verify_flag_and_all_three_files() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Tcp,
                host: "h".into(),
                port: 2376,
                tls: true,
                tls_ca: "ca.pem".into(),
                tls_cert: "cert.pem".into(),
                tls_key: "key.pem".into(),
                ..Default::default()
            },
        );
        let args = e.list_containers().args;
        assert!(args.contains(&"--tlsverify".to_string()));
        assert!(args.contains(&"--tlscacert".to_string()));
        assert!(args.contains(&"ca.pem".to_string()));
        assert!(args.contains(&"--tlskey".to_string()));
        assert!(args.contains(&"key.pem".to_string()));
    }

    /// Skipping verification must use `--tls`, not `--tlsverify`, or
    /// the setting would do the opposite of what it says.
    #[test]
    fn skipping_verification_does_not_pass_tlsverify() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Tcp,
                tls: true,
                insecure_tls: true,
                ..Default::default()
            },
        );
        let args = e.list_containers().args;
        assert!(args.contains(&"--tls".to_string()));
        assert!(!args.contains(&"--tlsverify".to_string()));
    }

    /// `wsl.exe -d <distro> -- docker …`. The prefix comes before the
    /// program, and `--` stops wsl from reading the engine's flags as
    /// its own.
    #[test]
    fn a_wsl_engine_is_prefixed_before_the_program() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Wsl,
                distro: "Ubuntu-24.04".into(),
                ..Default::default()
            },
        );
        let inv = e.list_containers();
        assert_eq!(inv.program, "wsl.exe");
        assert_eq!(inv.args[0], "-d");
        assert_eq!(inv.args[1], "Ubuntu-24.04");
        assert_eq!(inv.args[2], "--", "or wsl eats the engine's flags");
        assert_eq!(inv.args[3], "docker");
        assert_eq!(inv.args[4], "ps");
        assert!(inv.remote_ssh.is_none());
    }

    /// Over SSH the engine is local *to the other machine*, so no `-H`
    /// is passed and the runner is told which connection to use.
    #[test]
    fn an_ssh_engine_carries_the_connection_and_no_host_flag() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Ssh,
                tunnel_ssh: "web01".into(),
                ..Default::default()
            },
        );
        let inv = e.list_containers();
        assert_eq!(inv.remote_ssh.as_deref(), Some("web01"));
        assert_eq!(inv.program, "docker");
        assert!(
            !inv.args.contains(&"-H".to_string()),
            "it is local from there"
        );
    }

    /// The Go template contains a space, so an unquoted line would
    /// split it into two arguments and the engine would reject it.
    #[test]
    fn the_shell_line_quotes_arguments_that_need_it() {
        let e = engine("docker", ContainerFields::default());
        let line = e.list_containers().to_line();
        assert!(
            line.contains("'{{json .}}'"),
            "the template must survive the shell: {line}"
        );
        assert!(line.starts_with("docker ps --all --no-trunc --format "));
    }

    /// A name with a quote in it must not be able to end the quoting.
    #[test]
    fn a_quote_inside_an_argument_cannot_escape_the_quoting() {
        let inv = Invocation {
            program: "docker".into(),
            args: vec!["logs".into(), "odd'name".into()],
            remote_ssh: None,
        };
        let line = inv.to_line();
        assert_eq!(line, r"docker logs 'odd'\''name'");
    }

    #[test]
    fn the_log_follow_line_is_what_a_logs_source_runs() {
        let e = engine(
            "docker",
            ContainerFields {
                transport: Transport::Wsl,
                distro: "Ubuntu-24.04".into(),
                ..Default::default()
            },
        );
        let line = e.follow_logs_line("af48259fe8da", 2000);
        assert!(line.starts_with("wsl.exe -d Ubuntu-24.04 -- docker logs"));
        assert!(line.contains("--follow"));
        assert!(line.contains("--tail 2000"));
        assert!(line.contains("af48259fe8da"));
        // A container that logs to stderr - portainer, nginx, most Go
        // and Java images - produces nothing on stdout, and a Logs
        // command source reads stdout. Without the redirect the tab
        // follows a live container and stays empty.
        assert!(line.ends_with(" 2>&1"), "stderr has to be carried too");
    }

    /// A minimal image has no bash, so the fallback has to be `sh`.
    #[test]
    fn an_exec_shell_defaults_to_sh() {
        let e = engine("docker", ContainerFields::default());
        assert!(e.exec_shell_line("abc", "").contains("/bin/sh"));
        assert!(e.exec_shell_line("abc", "/bin/bash").contains("/bin/bash"));
        assert!(e.exec_shell_line("abc", "  ").contains("/bin/sh"));
        assert!(e.exec_shell_line("abc", "").contains("exec -it abc"));
    }

    #[test]
    fn a_verb_names_itself_and_says_whether_it_needs_confirming() {
        let e = engine("docker", ContainerFields::default());
        let inv = e.verb(Verb::Stop, "abc");
        assert_eq!(inv.args, ["stop", "abc"]);
        assert!(Verb::Stop.destructive());
        assert!(Verb::Remove.destructive());
        assert!(Verb::Kill.destructive());
        assert!(!Verb::Start.destructive(), "starting breaks nothing");
        assert!(!Verb::Unpause.destructive());
    }

    /// The API version is an environment variable on both engines, not
    /// a flag, so it cannot be smuggled into the argv.
    #[test]
    fn a_pinned_api_version_is_handed_over_as_an_environment_variable() {
        let e = engine(
            "docker",
            ContainerFields {
                api_version: "1.44".into(),
                ..Default::default()
            },
        );
        assert_eq!(
            e.env(),
            [("DOCKER_API_VERSION".to_string(), "1.44".to_string())]
        );
        assert!(
            !e.list_containers().args.contains(&"1.44".to_string()),
            "it is not a flag"
        );
        assert!(
            engine("docker", ContainerFields::default())
                .env()
                .is_empty(),
            "nothing to set when it is not pinned"
        );
    }

    #[test]
    fn every_list_asks_for_json_and_full_ids() {
        let e = engine("docker", ContainerFields::default());
        for inv in [e.list_containers(), e.list_images(), e.list_networks()] {
            assert!(
                inv.args.contains(&"--no-trunc".to_string()),
                "a short id is for showing, not addressing: {:?}",
                inv.args
            );
            assert!(inv.args.contains(&"{{json .}}".to_string()));
        }
        // `volume ls` has no --no-trunc, and a volume is named anyway.
        assert!(e.list_volumes().args.contains(&"{{json .}}".to_string()));
    }
}
