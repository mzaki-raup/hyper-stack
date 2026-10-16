//! The `[redis]` table of a `kind = "redis"` connection profile
//! (docs/tools/redis.md §9), `redis://` URL import, and the resolution of a
//! profile into a [`ConnectConfig`]: `{{variables}}` from the profile's
//! environment, the password from the vault.

use std::time::Duration;

use hs_core::secrets::{SecretRef, SecretStore};
use secrecy::ExposeSecret;

use crate::client::ConnectConfig;

/// How the client reaches the server. Sentinel and cluster are described by
/// the spec and stored here; only standalone opens a session so far.
pub const MODES: &[&str] = &["standalone", "sentinel", "cluster"];

/// How a mode is written on screen. The stored value stays lower case;
/// a list item is a name, so it is shown in Title Case.
pub fn mode_label(mode: &str) -> &str {
    match mode {
        "standalone" => "Standalone",
        "sentinel" => "Sentinel",
        "cluster" => "Cluster",
        other => other,
    }
}

/// Default separator the key tree groups on.
pub const DEFAULT_DELIMITER: &str = ":";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisFields {
    pub host: String,
    pub port: u16,
    /// ACL user; empty means the implicit `default` user.
    pub user: String,
    /// `secret:<id>`, `{{var}}`, or typed text until the form saves it.
    pub password: String,
    pub db: i64,
    pub tls: bool,
    pub tls_ca: String,
    /// standalone | sentinel | cluster
    pub mode: String,
    /// Sentinel master name.
    pub master_name: String,
    /// Sentinel/cluster seed nodes, `host:port` separated by commas.
    pub nodes: String,
    /// Id of an SSH connection to tunnel through (`[tunnel] ssh`).
    pub tunnel_ssh: String,
    /// What the key tree groups on.
    pub delimiter: String,
}

impl Default for RedisFields {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6379,
            user: String::new(),
            password: String::new(),
            db: 0,
            tls: false,
            tls_ca: String::new(),
            mode: "standalone".into(),
            master_name: String::new(),
            nodes: String::new(),
            tunnel_ssh: String::new(),
            delimiter: DEFAULT_DELIMITER.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    VaultLocked,
    /// `{{names}}` the environment does not define.
    MissingVariables(Vec<String>),
    Secret(String),
    /// sentinel/cluster: stored, but not yet a session.
    UnsupportedMode(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::VaultLocked => f.write_str("the vault is locked"),
            ProfileError::MissingVariables(v) => write!(f, "undefined variables: {}", v.join(", ")),
            ProfileError::Secret(e) => f.write_str(e),
            ProfileError::UnsupportedMode(m) => write!(f, "{m} connections are not supported yet"),
        }
    }
}

impl RedisFields {
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = fields
            .get("redis")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d = Self::default();
        Self {
            host: Some(s("host")).filter(|x| !x.is_empty()).unwrap_or(d.host),
            port: t
                .get("port")
                .and_then(|v| v.as_integer())
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or(d.port),
            user: s("user"),
            password: s("password"),
            db: t.get("db").and_then(|v| v.as_integer()).unwrap_or(0),
            tls: t.get("tls").and_then(|v| v.as_bool()).unwrap_or(false),
            tls_ca: s("tls_ca"),
            mode: Some(s("mode")).filter(|x| !x.is_empty()).unwrap_or(d.mode),
            master_name: s("master_name"),
            nodes: s("nodes"),
            delimiter: Some(s("delimiter"))
                .filter(|x| !x.is_empty())
                .unwrap_or(d.delimiter),
            tunnel_ssh: fields
                .get("tunnel")
                .and_then(|v| v.as_table())
                .and_then(|t| t.get("ssh"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        let mut put = |k: &str, v: &str| {
            t.insert(k.into(), toml::Value::String(v.to_string()));
        };
        put("host", &self.host);
        put("user", &self.user);
        put("password", &self.password);
        put("tls_ca", &self.tls_ca);
        put("mode", &self.mode);
        put("master_name", &self.master_name);
        put("nodes", &self.nodes);
        put("delimiter", &self.delimiter);
        t.insert("port".into(), toml::Value::Integer(self.port as i64));
        t.insert("db".into(), toml::Value::Integer(self.db));
        t.insert("tls".into(), toml::Value::Boolean(self.tls));
        fields.insert("redis".into(), toml::Value::Table(t));
        if self.tunnel_ssh.is_empty() {
            fields.remove("tunnel");
        } else {
            let mut tun = toml::Table::new();
            tun.insert("ssh".into(), toml::Value::String(self.tunnel_ssh.clone()));
            fields.insert("tunnel".into(), toml::Value::Table(tun));
        }
    }

    /// Sidebar subtitle: "Redis · cache.example.com:6379/0".
    pub fn label(&self) -> String {
        let scheme = if self.tls { "Rediss" } else { "Redis" };
        format!("{scheme} · {}:{}/{}", self.host, self.port, self.db)
    }

    /// Resolve variables and secrets. `lookup` substitutes `{{names}}` from
    /// the profile's environment (it returns the names it could not find).
    pub fn resolve(
        &self,
        read_only: bool,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<ConnectConfig, ProfileError> {
        if self.mode != "standalone" {
            return Err(ProfileError::UnsupportedMode(self.mode.clone()));
        }
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s,
            Err(m) => {
                missing.extend(m);
                v.to_string()
            }
        };
        let host = sub(&self.host);
        let user = sub(&self.user);
        let tls_ca = sub(&self.tls_ca);
        let password_raw = sub(&self.password);
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(ProfileError::MissingVariables(missing));
        }
        let password = if password_raw.is_empty() {
            None
        } else {
            match SecretRef::parse(&password_raw) {
                Some(r) => {
                    if secrets.is_locked() {
                        return Err(ProfileError::VaultLocked);
                    }
                    Some(
                        secrets
                            .get(&r)
                            .map_err(|e| ProfileError::Secret(e.to_string()))?
                            .expose_secret()
                            .to_string(),
                    )
                }
                None => Some(password_raw),
            }
        };
        Ok(ConnectConfig {
            host,
            port: self.port,
            user,
            password,
            db: self.db,
            tls: self.tls,
            tls_ca: Some(tls_ca).filter(|c| !c.is_empty()),
            read_only,
            connect_timeout: Duration::from_secs(10),
        })
    }
}

/// Read a `redis://` or `rediss://` URI into form fields, the way the
/// connection form's **Import** does. Anything the URI omits keeps its
/// default, so a bare host is enough.
pub fn parse_url(url: &str) -> Option<RedisFields> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    let tls = match scheme.to_ascii_lowercase().as_str() {
        "redis" => false,
        "rediss" => true,
        _ => return None,
    };
    let mut f = RedisFields {
        tls,
        port: 6379,
        ..Default::default()
    };
    // userinfo@host:port/db
    let (authority, path) = match rest.split_once('/') {
        Some((a, p)) => (a, p),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    if let Some(u) = userinfo {
        match u.split_once(':') {
            // `redis://:password@host` is the password-only form.
            Some((user, pass)) => {
                f.user = percent_decode(user);
                f.password = percent_decode(pass);
            }
            None => f.user = percent_decode(u),
        }
    }
    if hostport.is_empty() {
        return None;
    }
    match hostport.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => {
            f.host = h.trim_matches(['[', ']']).to_string();
            f.port = p.parse().ok()?;
        }
        _ => f.host = hostport.trim_matches(['[', ']']).to_string(),
    }
    let db = path.split('?').next().unwrap_or("");
    if !db.is_empty() {
        f.db = db.parse().ok()?;
    }
    Some(f)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
            if let Ok(v) = u8::from_str_radix(hex, 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with nothing in it: these tests never resolve a `secret:` ref.
    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn is_locked(&self) -> bool {
            false
        }
        fn exists(&self) -> bool {
            false
        }
        fn unlock(&self, _: &secrecy::SecretString) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn lock(&self) {}
        fn get(&self, _: &SecretRef) -> hs_core::secrets::Result<secrecy::SecretString> {
            unreachable!("these tests use plain passwords")
        }
        fn set(
            &self,
            _: &SecretRef,
            _: &str,
            _: &secrecy::SecretString,
        ) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn delete(&self, _: &SecretRef) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn list(&self) -> hs_core::secrets::Result<Vec<(SecretRef, String)>> {
            Ok(Vec::new())
        }
    }

    #[test]
    fn a_uri_fills_the_form() {
        let f = parse_url("redis://cache.example.com:6380/3").unwrap();
        assert_eq!(f.host, "cache.example.com");
        assert_eq!(f.port, 6380);
        assert_eq!(f.db, 3);
        assert!(!f.tls);
        // A bare host keeps the defaults.
        let f = parse_url("redis://localhost").unwrap();
        assert_eq!((f.host.as_str(), f.port, f.db), ("localhost", 6379, 0));
        // rediss is TLS, and the ACL user and password are decoded.
        let f = parse_url("rediss://alice:p%40ss@h:6379/1").unwrap();
        assert!(f.tls);
        assert_eq!(f.user, "alice");
        assert_eq!(f.password, "p@ss");
        // The password-only form has no user.
        let f = parse_url("redis://:secret@h").unwrap();
        assert_eq!(f.user, "");
        assert_eq!(f.password, "secret");
        assert_eq!(parse_url("postgres://h/db"), None);
        assert_eq!(parse_url("not a url"), None);
    }

    #[test]
    fn fields_round_trip_through_the_profile_table() {
        let f = RedisFields {
            host: "cache".into(),
            port: 6380,
            user: "alice".into(),
            password: "secret:01J".into(),
            db: 2,
            tls: true,
            delimiter: "|".into(),
            tunnel_ssh: "ssh-1".into(),
            ..Default::default()
        };
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        assert_eq!(RedisFields::from_table(&table), f);
        // Dropping the tunnel removes the table rather than storing "".
        let mut f2 = f.clone();
        f2.tunnel_ssh.clear();
        f2.write_into(&mut table);
        assert!(!table.contains_key("tunnel"));
        assert_eq!(RedisFields::from_table(&table), f2);
        // An empty table is all defaults.
        assert_eq!(
            RedisFields::from_table(&toml::Table::new()),
            RedisFields::default()
        );
    }

    #[test]
    fn the_label_says_scheme_host_and_database() {
        let mut f = RedisFields::default();
        assert_eq!(f.label(), "Redis · 127.0.0.1:6379/0");
        f.tls = true;
        f.db = 4;
        assert_eq!(f.label(), "Rediss · 127.0.0.1:6379/4");
    }

    #[test]
    fn resolving_reports_missing_variables_and_refuses_other_modes() {
        let secrets = NoSecrets;
        let f = RedisFields {
            host: "{{cache_host}}".into(),
            ..Default::default()
        };
        let lookup = |v: &str| {
            if v.contains("{{") {
                Err(vec!["cache_host".to_string()])
            } else {
                Ok(v.to_string())
            }
        };
        assert_eq!(
            f.resolve(false, &secrets, &lookup).err(),
            Some(ProfileError::MissingVariables(vec!["cache_host".into()]))
        );
        let f = RedisFields {
            mode: "cluster".into(),
            ..Default::default()
        };
        assert_eq!(
            f.resolve(false, &secrets, &lookup).err(),
            Some(ProfileError::UnsupportedMode("cluster".into()))
        );
    }

    #[test]
    fn a_plain_password_is_used_as_typed_and_read_only_carries_through() {
        let secrets = NoSecrets;
        let f = RedisFields {
            password: "hunter2".into(),
            ..Default::default()
        };
        let cfg = f
            .resolve(true, &secrets, &|v: &str| Ok(v.to_string()))
            .unwrap();
        assert_eq!(cfg.password.as_deref(), Some("hunter2"));
        assert!(cfg.read_only);
    }
}
