//! The `[queue]` table of a broker's connection profile
//! (docs/tools/queue.md §5), and its resolution into a [`ConnectConfig`]:
//! `{{variables}}` from the profile's environment, the secret from the
//! vault.
//!
//! One table serves all five kinds. Where they differ is the address,
//! which is a URL for RabbitMQ and NATS and `host:port` for the others,
//! and which credential kinds make sense; the fields a kind has no use
//! for are simply not shown by its form.

use std::time::Duration;

use hs_core::secrets::{SecretRef, SecretStore};
use secrecy::ExposeSecret;

/// What a credential is (`.claude/rules/security.md`, "Credentials for
/// the newer activities"). `none` is the fourth answer: a broker on a
/// developer's machine usually has no login at all.
pub const AUTH_KINDS: &[&str] = &["none", "password", "token", "api-key"];

/// The auth kinds a broker kind can use. A kind that has no use for one
/// does not offer it, rather than offering a field that is ignored
/// (docs/tools/queue.md §5).
pub fn auth_kinds_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "rabbitmq" | "kafka" => AUTH_KINDS,
        _ => &["none", "password", "token"],
    }
}

/// Kafka's SASL mechanisms for a username and password.
pub const SASL_MECHANISMS: &[&str] = &["plain", "scram-sha-256", "scram-sha-512"];

pub fn sasl_label(m: &str) -> &str {
    match m {
        "plain" => "PLAIN",
        "scram-sha-256" => "SCRAM-SHA-256",
        "scram-sha-512" => "SCRAM-SHA-512",
        other => other,
    }
}

/// MQTT protocol versions.
pub const MQTT_VERSIONS: &[&str] = &["3.1.1", "5"];

/// Where a broker of `kind` is usually found, so a new profile opens on
/// something that would work against a local broker.
pub fn default_address(kind: &str) -> &'static str {
    match kind {
        "rabbitmq" => "http://127.0.0.1:15672",
        "kafka" => "127.0.0.1:9092",
        "redis-streams" => "127.0.0.1:6379",
        "nats" => "nats://127.0.0.1:4222",
        "mqtt" => "127.0.0.1:1883",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueFields {
    /// RabbitMQ: the management URL. Kafka: bootstrap servers, comma
    /// separated. NATS: server URLs, comma separated. Redis Streams and
    /// MQTT: `host:port`.
    pub address: String,
    pub tls: bool,
    /// none | password | token | api-key
    pub auth: String,
    /// The login (password), or the key's id (a Kafka API key).
    pub user: String,
    /// `secret:<id>`, `{{var}}`, or typed text until the form saves it:
    /// the password, the token or the key.
    pub secret: String,
    /// The header an API key travels in (RabbitMQ).
    pub header: String,
    /// RabbitMQ: the vhost the tab opens on; empty means every vhost.
    pub vhost: String,
    /// Kafka: the SASL mechanism a username and password use.
    pub sasl: String,
    /// Redis Streams: the database index.
    pub db: i64,
    /// MQTT: the client id; empty makes one up per session.
    pub client_id: String,
    /// MQTT: 3.1.1 or 5.
    pub protocol: String,
}

impl Default for QueueFields {
    fn default() -> Self {
        Self {
            address: String::new(),
            tls: false,
            auth: "none".into(),
            user: String::new(),
            secret: String::new(),
            header: String::new(),
            vhost: String::new(),
            sasl: "plain".into(),
            db: 0,
            client_id: String::new(),
            protocol: "3.1.1".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    VaultLocked,
    /// `{{names}}` the environment does not define.
    MissingVariables(Vec<String>),
    Secret(String),
    /// Something the form should not have let through.
    Invalid(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::VaultLocked => f.write_str("the vault is locked"),
            ProfileError::MissingVariables(v) => write!(f, "undefined variables: {}", v.join(", ")),
            ProfileError::Secret(e) | ProfileError::Invalid(e) => f.write_str(e),
        }
    }
}

/// A credential, resolved: what the driver hands the broker.
#[derive(Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    Password {
        user: String,
        password: String,
    },
    Token(String),
    /// `name` is the header (RabbitMQ) or the key's id (Kafka).
    ApiKey {
        name: String,
        key: String,
    },
}

/// Never prints the secret, so a config can be logged.
impl std::fmt::Debug for Auth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Auth::None => f.write_str("None"),
            Auth::Password { user, .. } => write!(f, "Password({user})"),
            Auth::Token(_) => f.write_str("Token"),
            Auth::ApiKey { name, .. } => write!(f, "ApiKey({name})"),
        }
    }
}

/// Everything a driver needs to connect, with variables and secrets
/// already resolved.
#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub kind: String,
    pub address: String,
    pub tls: bool,
    pub auth: Auth,
    pub vhost: String,
    pub sasl: String,
    pub db: i64,
    pub client_id: String,
    pub protocol: String,
    /// Writes are refused by the driver as well as the tab
    /// (docs/tools/queue.md §6).
    pub read_only: bool,
    pub timeout: Duration,
    /// MQTT: how long one look at the broker listens (Settings → Queue).
    pub mqtt_window: Duration,
}

impl QueueFields {
    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get("queue")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d = Self::default();
        let or = |v: String, d: String| if v.is_empty() { d } else { v };
        let auth = or(s("auth"), d.auth);
        Self {
            address: or(s("address"), default_address(kind).to_string()),
            tls: t.get("tls").and_then(|v| v.as_bool()).unwrap_or(false),
            // A kind that cannot use the stored credential kind falls
            // back to none rather than sending something meaningless.
            auth: if auth_kinds_for(kind).contains(&auth.as_str()) {
                auth
            } else {
                "none".into()
            },
            user: s("user"),
            secret: s("secret"),
            header: s("header"),
            vhost: s("vhost"),
            sasl: or(s("sasl"), d.sasl),
            db: t.get("db").and_then(|v| v.as_integer()).unwrap_or(0),
            client_id: s("client_id"),
            protocol: or(s("protocol"), d.protocol),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        let mut put = |k: &str, v: &str| {
            t.insert(k.into(), toml::Value::String(v.to_string()));
        };
        put("address", &self.address);
        put("auth", &self.auth);
        put("user", &self.user);
        put("secret", &self.secret);
        put("header", &self.header);
        put("vhost", &self.vhost);
        put("sasl", &self.sasl);
        put("client_id", &self.client_id);
        put("protocol", &self.protocol);
        t.insert("tls".into(), toml::Value::Boolean(self.tls));
        t.insert("db".into(), toml::Value::Integer(self.db));
        fields.insert("queue".into(), toml::Value::Table(t));
    }

    /// Sidebar subtitle: "Kafka · 127.0.0.1:9092".
    pub fn label_for(&self, kind: &str) -> String {
        format!(
            "{} · {}",
            hs_core::connections::kind_label(kind),
            self.short_address()
        )
    }

    /// The address without its scheme, for a row's second line.
    pub fn short_address(&self) -> &str {
        self.address
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.address)
            .trim_end_matches('/')
    }

    /// Resolve variables and the secret. `lookup` substitutes `{{names}}`
    /// from the profile's environment and returns the names it could not
    /// find.
    pub fn resolve(
        &self,
        kind: &str,
        read_only: bool,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<ConnectConfig, ProfileError> {
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s,
            Err(m) => {
                missing.extend(m);
                v.to_string()
            }
        };
        let address = sub(&self.address);
        let user = sub(&self.user);
        let header = sub(&self.header);
        let vhost = sub(&self.vhost);
        let client_id = sub(&self.client_id);
        let secret_raw = sub(&self.secret);
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(ProfileError::MissingVariables(missing));
        }
        if address.trim().is_empty() {
            return Err(ProfileError::Invalid("the broker has no address".into()));
        }
        let auth = match self.auth.as_str() {
            "none" => Auth::None,
            _ => {
                let secret = match SecretRef::parse(&secret_raw) {
                    Some(r) => {
                        if secrets.is_locked() {
                            return Err(ProfileError::VaultLocked);
                        }
                        secrets
                            .get(&r)
                            .map_err(|e| ProfileError::Secret(e.to_string()))?
                            .expose_secret()
                            .to_string()
                    }
                    None => secret_raw,
                };
                match self.auth.as_str() {
                    "token" => Auth::Token(secret),
                    "api-key" => Auth::ApiKey {
                        // RabbitMQ sends the key in a header; Kafka takes
                        // a key id and its secret (Confluent Cloud's form).
                        name: if kind == "kafka" { user } else { header },
                        key: secret,
                    },
                    _ => Auth::Password {
                        user,
                        password: secret,
                    },
                }
            }
        };
        Ok(ConnectConfig {
            kind: kind.to_string(),
            address: address.trim().to_string(),
            tls: self.tls,
            auth,
            vhost,
            sasl: self.sasl.clone(),
            db: self.db,
            client_id,
            protocol: self.protocol.clone(),
            read_only,
            timeout: Duration::from_secs(10),
            mqtt_window: Duration::from_secs(3),
        })
    }

    /// The secret this profile holds, with the label the vault files it
    /// under, for the form's save to move it into the vault.
    pub fn secret_fields(&mut self) -> [(&'static str, &mut String); 1] {
        let label = match self.auth.as_str() {
            "token" => "token",
            "api-key" => "API key",
            _ => "password",
        };
        [(label, &mut self.secret)]
    }
}

/// `host:port` out of an address in any of the forms a profile holds,
/// with `default_port` when the port is left out.
pub fn host_port(address: &str, default_port: u16) -> Result<(String, u16), String> {
    let a = address
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(address)
        .trim()
        .trim_end_matches('/');
    if a.is_empty() {
        return Err("the broker has no address".into());
    }
    // A bracketed IPv6 address keeps its colons.
    if let Some(rest) = a.strip_prefix('[') {
        let (host, tail) = rest
            .split_once(']')
            .ok_or_else(|| format!("“{a}” is not an address"))?;
        let port = match tail.strip_prefix(':') {
            Some(p) => p.parse().map_err(|_| format!("“{p}” is not a port"))?,
            None => default_port,
        };
        return Ok((host.to_string(), port));
    }
    match a.rsplit_once(':') {
        Some((h, p)) => Ok((
            h.to_string(),
            p.parse().map_err(|_| format!("“{p}” is not a port"))?,
        )),
        None => Ok((a.to_string(), default_port)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A locked vault: resolving a `secret:` reference must stop there.
    struct Locked;
    impl SecretStore for Locked {
        fn is_locked(&self) -> bool {
            true
        }
        fn exists(&self) -> bool {
            true
        }
        fn unlock(&self, _: &secrecy::SecretString) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn lock(&self) {}
        fn get(&self, _: &SecretRef) -> hs_core::secrets::Result<secrecy::SecretString> {
            Err(hs_core::secrets::SecretError::Locked)
        }
        fn set(
            &self,
            _: &SecretRef,
            _: &str,
            _: &secrecy::SecretString,
        ) -> hs_core::secrets::Result<()> {
            Err(hs_core::secrets::SecretError::Locked)
        }
        fn delete(&self, _: &SecretRef) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn list(&self) -> hs_core::secrets::Result<Vec<(SecretRef, String)>> {
            Ok(Vec::new())
        }
    }

    fn same(v: &str) -> Result<String, Vec<String>> {
        Ok(v.to_string())
    }

    #[test]
    fn a_new_profile_points_at_the_kinds_usual_local_address() {
        let f = QueueFields::from_table("kafka", &toml::Table::new());
        assert_eq!(f.address, "127.0.0.1:9092");
        assert_eq!(f.auth, "none");
        let f = QueueFields::from_table("rabbitmq", &toml::Table::new());
        assert_eq!(f.label_for("rabbitmq"), "RabbitMQ · 127.0.0.1:15672");
    }

    #[test]
    fn fields_survive_a_round_trip_through_the_table() {
        let f = QueueFields {
            address: "nats://a:4222,nats://b:4222".into(),
            tls: true,
            auth: "token".into(),
            secret: "secret:01J0000000000000000000000".into(),
            ..Default::default()
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(QueueFields::from_table("nats", &t), f);
    }

    #[test]
    fn a_kind_never_offers_a_credential_it_would_ignore() {
        // NATS has no API keys: a stored one falls back to none.
        let mut t = toml::Table::new();
        QueueFields {
            auth: "api-key".into(),
            ..Default::default()
        }
        .write_into(&mut t);
        assert_eq!(QueueFields::from_table("nats", &t).auth, "none");
        assert_eq!(QueueFields::from_table("rabbitmq", &t).auth, "api-key");
        assert!(!auth_kinds_for("mqtt").contains(&"api-key"));
    }

    #[test]
    fn a_vault_secret_needs_the_vault_open() {
        let f = QueueFields {
            address: "127.0.0.1:1883".into(),
            auth: "password".into(),
            user: "dev".into(),
            secret: "secret:01J0000000000000000000000".into(),
            ..Default::default()
        };
        assert_eq!(
            f.resolve("mqtt", false, &Locked, &same).unwrap_err(),
            ProfileError::VaultLocked
        );
        // No credential, no vault needed.
        let open = QueueFields {
            auth: "none".into(),
            ..f
        };
        assert_eq!(
            open.resolve("mqtt", false, &Locked, &same).unwrap().auth,
            Auth::None
        );
    }

    #[test]
    fn a_kafka_api_key_is_its_id_and_secret() {
        let f = QueueFields {
            address: "broker:9092".into(),
            auth: "api-key".into(),
            user: "KEYID".into(),
            header: "ignored".into(),
            secret: "s3cr3t".into(),
            ..Default::default()
        };
        let c = f.resolve("kafka", false, &Locked, &same).unwrap();
        assert_eq!(
            c.auth,
            Auth::ApiKey {
                name: "KEYID".into(),
                key: "s3cr3t".into()
            }
        );
        // And the debug form of a config never carries it.
        assert!(!format!("{c:?}").contains("s3cr3t"));
    }

    #[test]
    fn undefined_variables_are_named() {
        let f = QueueFields {
            address: "{{BROKER}}".into(),
            ..Default::default()
        };
        let missing = |v: &str| {
            if v.contains("{{") {
                Err(vec!["BROKER".to_string()])
            } else {
                Ok(v.to_string())
            }
        };
        assert_eq!(
            f.resolve("kafka", false, &Locked, &missing).unwrap_err(),
            ProfileError::MissingVariables(vec!["BROKER".into()])
        );
    }

    #[test]
    fn host_and_port_come_from_any_address_form() {
        assert_eq!(
            host_port("127.0.0.1:1883", 1).unwrap(),
            ("127.0.0.1".into(), 1883)
        );
        assert_eq!(
            host_port("mqtt://broker", 1883).unwrap(),
            ("broker".into(), 1883)
        );
        assert_eq!(host_port("[::1]:6379", 1).unwrap(), ("::1".into(), 6379));
        assert!(host_port("", 1).is_err());
        assert!(host_port("h:port", 1).is_err());
    }
}
