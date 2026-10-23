//! The `[container]` fields a container connection profile carries.
//!
//! One struct covers all four kinds rather than four structs, because
//! they overlap heavily — a name, a transport, TLS, a tunnel — and the
//! kind decides which fields the form shows. `tool-nosql` takes the same
//! approach across MongoDB and CouchDB.
//!
//! **Secrets are never stored here in the clear.** Every field marked a
//! secret below holds a `secret:<ulid>` reference after the form has run
//! it through `store_typed_secrets`, as every other connection kind
//! does. The one deliberate exception is `kubeconfig_path`, which is a
//! *path* and not a secret: see `docs/tools/containers.md` §10.2 for why
//! a copy of a kubeconfig in the vault would be worse than a reference
//! to the real file.

use serde::{Deserialize, Serialize};

/// How the driver reaches the engine. Kubernetes ignores this and uses
/// its kubeconfig instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// The engine's local socket, or the Windows named pipe. The default
    /// because it is what a local Docker or Podman install gives you.
    #[default]
    Local,
    /// `tcp://host:port`, optionally with TLS.
    Tcp,
    /// The engine on another machine, through a saved `ssh` profile -
    /// what `docker -H ssh://` does.
    Ssh,
    /// The engine inside a WSL distribution, which on Windows is where
    /// it usually is (owner's report, 2026-10-23). Reached by prefixing
    /// `wsl.exe -d <distro> --`, as `tool_logs::services` already does.
    Wsl,
}

impl Transport {
    pub const ALL: [Transport; 4] = [
        Transport::Local,
        Transport::Tcp,
        Transport::Ssh,
        Transport::Wsl,
    ];

    /// The `en.ftl` key for this transport's label.
    pub fn key(self) -> &'static str {
        match self {
            Transport::Local => "container-transport-local",
            Transport::Tcp => "container-transport-tcp",
            Transport::Ssh => "container-transport-ssh",
            Transport::Wsl => "container-transport-wsl",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Transport::Local => "local",
            Transport::Tcp => "tcp",
            Transport::Ssh => "ssh",
            Transport::Wsl => "wsl",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "tcp" => Transport::Tcp,
            "ssh" => Transport::Ssh,
            "wsl" => Transport::Wsl,
            _ => Transport::Local,
        }
    }
}

/// Everything a container connection stores, for every kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContainerFields {
    // --- engines: docker, podman, wincontainer ----------------------
    pub transport: Transport,
    /// The socket or named pipe, when `transport` is Local. Empty means
    /// the platform default, which is what almost everyone wants.
    pub socket: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    /// Paths, not contents: a certificate is not a secret, and a key is
    /// held by reference so it is not copied into the vault twice.
    pub tls_ca: String,
    pub tls_cert: String,
    /// **Secret.** A `secret:<ulid>` after the form has stored it.
    pub tls_key: String,
    /// The saved `ssh` profile id, when `transport` is Ssh.
    pub tunnel_ssh: String,
    /// The WSL distribution, when `transport` is Wsl.
    pub distro: String,
    /// Pin the Engine API version when the server is older than the
    /// client expects.
    pub api_version: String,
    /// A registry to pull from, and its credentials.
    pub registry: String,
    pub registry_user: String,
    /// **Secret.**
    pub registry_password: String,

    // --- kubernetes --------------------------------------------------
    /// The kubeconfig to read, by path. Relative to the data folder when
    /// it sits inside it (ADR-017). **Not** a secret and **not** copied
    /// into the vault: a copy goes stale the moment a cluster is
    /// re-provisioned, duplicates a secret that already exists on disk,
    /// and breaks the `exec` credential plugins that resolve relative to
    /// the real file (§10.2).
    pub kubeconfig_path: String,
    /// A pasted kubeconfig, for a portable workspace or a static token.
    /// **Secret**, and the form warns that it will not follow rotation.
    pub kubeconfig_inline: String,
    pub context: String,
    pub namespace: String,
    /// **Secret.** A bearer token, for a cluster configured that way
    /// rather than through a kubeconfig.
    pub token: String,
    /// `--as`, for operators who audit through impersonation.
    pub impersonate_user: String,
    pub impersonate_group: String,
    pub proxy_url: String,
    /// Shell out to `kubectl` instead of talking to the API in process.
    /// The escape hatch of §10.3, for auth this app cannot satisfy
    /// itself.
    pub use_kubectl: bool,
    /// Skip TLS verification. Off, and the form says what it gives up.
    pub insecure_tls: bool,

    // --- common ------------------------------------------------------
    pub connect_timeout_secs: u32,
}

impl Default for ContainerFields {
    fn default() -> Self {
        Self {
            transport: Transport::Local,
            socket: String::new(),
            host: "127.0.0.1".into(),
            // The conventional Docker API port. Only used for Tcp.
            port: 2375,
            tls: false,
            tls_ca: String::new(),
            tls_cert: String::new(),
            tls_key: String::new(),
            tunnel_ssh: String::new(),
            distro: String::new(),
            api_version: String::new(),
            registry: String::new(),
            registry_user: String::new(),
            registry_password: String::new(),
            kubeconfig_path: String::new(),
            kubeconfig_inline: String::new(),
            context: String::new(),
            namespace: String::new(),
            token: String::new(),
            impersonate_user: String::new(),
            impersonate_group: String::new(),
            proxy_url: String::new(),
            use_kubectl: false,
            insecure_tls: false,
            connect_timeout_secs: 10,
        }
    }
}

impl ContainerFields {
    /// Read the `[container]` sub-table of a profile, falling back to the
    /// defaults for anything missing — a profile written by an older
    /// version must keep working.
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = fields
            .get("container")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let b = |k: &str| t.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
        let d = Self::default();
        let or = |v: String, fallback: String| if v.is_empty() { fallback } else { v };
        Self {
            transport: Transport::parse(&s("transport")),
            socket: s("socket"),
            host: or(s("host"), d.host),
            port: t
                .get("port")
                .and_then(|v| v.as_integer())
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(d.port),
            tls: b("tls"),
            tls_ca: s("tls_ca"),
            tls_cert: s("tls_cert"),
            tls_key: s("tls_key"),
            // The tunnel lives in its own `[tunnel]` table, as every
            // other kind's does, so one place answers "what is tunnelled".
            tunnel_ssh: fields
                .get("tunnel")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("ssh"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            distro: s("distro"),
            api_version: s("api_version"),
            registry: s("registry"),
            registry_user: s("registry_user"),
            registry_password: s("registry_password"),
            kubeconfig_path: s("kubeconfig_path"),
            kubeconfig_inline: s("kubeconfig_inline"),
            context: s("context"),
            namespace: s("namespace"),
            token: s("token"),
            impersonate_user: s("impersonate_user"),
            impersonate_group: s("impersonate_group"),
            proxy_url: s("proxy_url"),
            use_kubectl: b("use_kubectl"),
            insecure_tls: b("insecure_tls"),
            connect_timeout_secs: t
                .get("connect_timeout_secs")
                .and_then(|v| v.as_integer())
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(d.connect_timeout_secs),
        }
    }

    /// Write these fields back into a profile's table.
    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        let mut put = |k: &str, v: &str| {
            t.insert(k.into(), toml::Value::String(v.to_string()));
        };
        put("transport", self.transport.as_str());
        put("socket", &self.socket);
        put("host", &self.host);
        put("tls_ca", &self.tls_ca);
        put("tls_cert", &self.tls_cert);
        put("tls_key", &self.tls_key);
        put("distro", &self.distro);
        put("api_version", &self.api_version);
        put("registry", &self.registry);
        put("registry_user", &self.registry_user);
        put("registry_password", &self.registry_password);
        put("kubeconfig_path", &self.kubeconfig_path);
        put("kubeconfig_inline", &self.kubeconfig_inline);
        put("context", &self.context);
        put("namespace", &self.namespace);
        put("token", &self.token);
        put("impersonate_user", &self.impersonate_user);
        put("impersonate_group", &self.impersonate_group);
        put("proxy_url", &self.proxy_url);
        t.insert("port".into(), toml::Value::Integer(i64::from(self.port)));
        t.insert("tls".into(), toml::Value::Boolean(self.tls));
        t.insert("use_kubectl".into(), toml::Value::Boolean(self.use_kubectl));
        t.insert(
            "insecure_tls".into(),
            toml::Value::Boolean(self.insecure_tls),
        );
        t.insert(
            "connect_timeout_secs".into(),
            toml::Value::Integer(i64::from(self.connect_timeout_secs)),
        );
        fields.insert("container".into(), toml::Value::Table(t));
        if self.tunnel_ssh.is_empty() {
            fields.remove("tunnel");
        } else {
            let mut tun = toml::Table::new();
            tun.insert("ssh".into(), toml::Value::String(self.tunnel_ssh.clone()));
            fields.insert("tunnel".into(), toml::Value::Table(tun));
        }
    }

    /// The fields that must go through the vault, as
    /// `(what it is, the value)` pairs for `store_typed_secrets`. The
    /// caller takes them, stores them and writes the returned
    /// `secret:<ulid>` back.
    ///
    /// `kubeconfig_path` is deliberately absent: it is a path, not a
    /// secret (§10.2).
    pub fn secret_fields(&mut self) -> Vec<(&'static str, &mut String)> {
        vec![
            ("TLS key", &mut self.tls_key),
            ("registry password", &mut self.registry_password),
            ("kubeconfig", &mut self.kubeconfig_inline),
            ("token", &mut self.token),
        ]
    }

    /// What the list row says under the name: enough to tell two hosts
    /// apart without opening either.
    pub fn describe(&self, kind: &str) -> String {
        if hs_core::connections::is_cluster_kind(kind) {
            let mut parts = Vec::new();
            if !self.context.is_empty() {
                parts.push(self.context.clone());
            }
            if !self.namespace.is_empty() {
                parts.push(format!("ns {}", self.namespace));
            }
            if self.use_kubectl {
                parts.push("kubectl".to_string());
            }
            if parts.is_empty() {
                parts.push("default context".to_string());
            }
            return parts.join(" · ");
        }
        match self.transport {
            Transport::Local if self.socket.is_empty() => "local engine".into(),
            Transport::Local => self.socket.clone(),
            Transport::Tcp => format!(
                "{}{}:{}",
                if self.tls { "tcps://" } else { "tcp://" },
                self.host,
                self.port
            ),
            Transport::Ssh if self.tunnel_ssh.is_empty() => "over SSH".into(),
            Transport::Ssh => format!("over SSH · {}", self.tunnel_ssh),
            Transport::Wsl if self.distro.is_empty() => "in WSL".into(),
            Transport::Wsl => format!("WSL · {}", self.distro),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(f: &ContainerFields) -> ContainerFields {
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        // Through text, as a saved profile really goes, so a field that
        // cannot be serialised is caught here rather than on someone's
        // machine.
        let text = toml::to_string(&table).expect("serialises");
        let back: toml::Table = toml::from_str(&text).expect("parses");
        ContainerFields::from_table(&back)
    }

    #[test]
    fn every_field_survives_a_round_trip() {
        let f = ContainerFields {
            transport: Transport::Tcp,
            socket: "/var/run/docker.sock".into(),
            host: "10.0.0.5".into(),
            port: 2376,
            tls: true,
            tls_ca: "certs/ca.pem".into(),
            tls_cert: "certs/cert.pem".into(),
            tls_key: "secret:01J0".into(),
            tunnel_ssh: "ssh-web01".into(),
            distro: "Ubuntu-24.04".into(),
            api_version: "1.44".into(),
            registry: "registry.example.com".into(),
            registry_user: "ci".into(),
            registry_password: "secret:01J1".into(),
            kubeconfig_path: "~/.kube/config".into(),
            kubeconfig_inline: "secret:01J2".into(),
            context: "staging".into(),
            namespace: "shop".into(),
            token: "secret:01J3".into(),
            impersonate_user: "ops".into(),
            impersonate_group: "sre".into(),
            proxy_url: "http://proxy:3128".into(),
            use_kubectl: true,
            insecure_tls: true,
            connect_timeout_secs: 30,
        };
        assert_eq!(roundtrip(&f), f);
    }

    /// A profile written before this kind existed, or by an older
    /// version, must read back with working defaults rather than zeroes.
    #[test]
    fn an_empty_table_reads_back_as_the_defaults() {
        let got = ContainerFields::from_table(&toml::Table::new());
        assert_eq!(got, ContainerFields::default());
        assert_eq!(got.transport, Transport::Local);
        assert_eq!(got.connect_timeout_secs, 10, "not zero");
        assert_eq!(got.host, "127.0.0.1", "not empty");
        assert_eq!(got.port, 2375);
    }

    /// The tunnel belongs in `[tunnel]`, as every other kind's does, so
    /// one place answers "what is tunnelled".
    #[test]
    fn the_tunnel_lives_in_its_own_table_and_is_removed_when_cleared() {
        let mut table = toml::Table::new();
        ContainerFields {
            tunnel_ssh: "ssh-web01".into(),
            ..Default::default()
        }
        .write_into(&mut table);
        assert!(table.contains_key("tunnel"));
        assert_eq!(ContainerFields::from_table(&table).tunnel_ssh, "ssh-web01");

        ContainerFields::default().write_into(&mut table);
        assert!(!table.contains_key("tunnel"), "cleared, not left behind");
    }

    /// The vault list is what the form iterates, so a secret missing
    /// from it would be written to disk in the clear. `kubeconfig_path`
    /// must **not** be in it: it is a path, not a secret (§10.2).
    #[test]
    fn the_secret_fields_are_exactly_the_sensitive_ones() {
        let mut f = ContainerFields::default();
        let names: Vec<&str> = f.secret_fields().into_iter().map(|(n, _)| n).collect();
        assert_eq!(
            names,
            ["TLS key", "registry password", "kubeconfig", "token"]
        );
        assert!(
            !names.contains(&"kubeconfig path"),
            "a path is not a secret, and a vault copy would go stale"
        );
    }

    #[test]
    fn a_cluster_describes_itself_by_context_and_namespace() {
        let f = ContainerFields {
            context: "staging".into(),
            namespace: "shop".into(),
            ..Default::default()
        };
        assert_eq!(f.describe("k8s"), "staging · ns shop");
        let bare = ContainerFields::default();
        assert_eq!(bare.describe("k8s"), "default context");
        let via = ContainerFields {
            use_kubectl: true,
            context: "prod".into(),
            ..Default::default()
        };
        assert_eq!(via.describe("k8s"), "prod · kubectl");
    }

    #[test]
    fn an_engine_describes_itself_by_transport() {
        let local = ContainerFields::default();
        assert_eq!(local.describe("docker"), "local engine");
        let tcp = ContainerFields {
            transport: Transport::Tcp,
            host: "10.0.0.5".into(),
            port: 2376,
            tls: true,
            ..Default::default()
        };
        assert_eq!(tcp.describe("podman"), "tcps://10.0.0.5:2376");
        let wsl = ContainerFields {
            transport: Transport::Wsl,
            distro: "Ubuntu-24.04".into(),
            ..Default::default()
        };
        assert_eq!(wsl.describe("docker"), "WSL · Ubuntu-24.04");
        let ssh = ContainerFields {
            transport: Transport::Ssh,
            tunnel_ssh: "web01".into(),
            ..Default::default()
        };
        assert_eq!(ssh.describe("docker"), "over SSH · web01");
    }

    /// An unknown transport string must not panic or invent a kind; the
    /// local engine is the safe reading.
    #[test]
    fn an_unknown_transport_falls_back_to_local() {
        let mut t = toml::Table::new();
        let mut inner = toml::Table::new();
        inner.insert("transport".into(), toml::Value::String("quantum".into()));
        t.insert("container".into(), toml::Value::Table(inner));
        assert_eq!(ContainerFields::from_table(&t).transport, Transport::Local);
    }
}
