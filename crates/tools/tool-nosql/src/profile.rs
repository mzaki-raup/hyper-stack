//! The `[mongodb]` and `[couchdb]` tables of a connection profile
//! (docs/05-data-model-and-storage.md §2.2), their URL import, and the
//! resolution into a driver config: `{{variables}}` from the profile's
//! environment, the password from the vault.

use std::time::Duration;

use hs_core::secrets::{SecretRef, SecretStore};
use secrecy::ExposeSecret;

use crate::store::StoreKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    VaultLocked,
    /// `{{names}}` the environment does not define.
    MissingVariables(Vec<String>),
    Secret(String),
    UnknownKind(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::VaultLocked => f.write_str("the vault is locked"),
            ProfileError::MissingVariables(v) => write!(f, "undefined variables: {}", v.join(", ")),
            ProfileError::Secret(e) => f.write_str(e),
            ProfileError::UnknownKind(k) => write!(f, "{k} is not a document store"),
        }
    }
}

/// The fields both stores share, plus the ones only one of them uses. One
/// struct keeps the connection form and the resolution in one place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoSqlFields {
    /// mongodb | couchdb
    pub kind: String,
    /// A full connection URI; when set it wins over host and port.
    pub uri: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    /// `secret:<id>`, `{{var}}`, or typed text until the form saves it.
    pub password: String,
    /// MongoDB: the database that holds the user. CouchDB ignores it.
    pub auth_source: String,
    /// The database the tree opens on; empty shows them all.
    pub database: String,
    pub tls: bool,
    /// MongoDB: talk to this server rather than discovering a replica set.
    pub direct: bool,
    /// Id of an SSH connection to tunnel through (`[tunnel] ssh`).
    pub tunnel_ssh: String,
}

impl Default for NoSqlFields {
    fn default() -> Self {
        Self {
            kind: "mongodb".into(),
            uri: String::new(),
            host: "127.0.0.1".into(),
            port: 27017,
            user: String::new(),
            password: String::new(),
            auth_source: "admin".into(),
            database: String::new(),
            tls: false,
            direct: false,
            tunnel_ssh: String::new(),
        }
    }
}

impl NoSqlFields {
    pub fn store_kind(&self) -> Option<StoreKind> {
        match self.kind.as_str() {
            "mongodb" => Some(StoreKind::Mongo),
            "couchdb" => Some(StoreKind::Couch),
            _ => None,
        }
    }

    /// The default port for a kind, so switching kind in the form moves it.
    pub fn default_port(kind: &str) -> u16 {
        match kind {
            "couchdb" => 5984,
            _ => 27017,
        }
    }

    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get(kind)
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d = Self::default();
        Self {
            kind: kind.to_string(),
            uri: s("uri"),
            host: Some(s("host")).filter(|x| !x.is_empty()).unwrap_or(d.host),
            port: t
                .get("port")
                .and_then(|v| v.as_integer())
                .and_then(|p| u16::try_from(p).ok())
                .unwrap_or_else(|| Self::default_port(kind)),
            user: s("user"),
            password: s("password"),
            auth_source: Some(s("auth_source"))
                .filter(|x| !x.is_empty())
                .unwrap_or(d.auth_source),
            database: s("database"),
            tls: t.get("tls").and_then(|v| v.as_bool()).unwrap_or(false),
            direct: t.get("direct").and_then(|v| v.as_bool()).unwrap_or(false),
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
        for (k, v) in [
            ("uri", &self.uri),
            ("host", &self.host),
            ("user", &self.user),
            ("password", &self.password),
            ("database", &self.database),
        ] {
            t.insert(k.into(), toml::Value::String(v.clone()));
        }
        t.insert("port".into(), toml::Value::Integer(self.port as i64));
        t.insert("tls".into(), toml::Value::Boolean(self.tls));
        if self.kind == "mongodb" {
            t.insert(
                "auth_source".into(),
                toml::Value::String(self.auth_source.clone()),
            );
            t.insert("direct".into(), toml::Value::Boolean(self.direct));
        }
        // Only this kind's table: a profile is one kind.
        fields.remove("mongodb");
        fields.remove("couchdb");
        fields.insert(self.kind.clone(), toml::Value::Table(t));
        if self.tunnel_ssh.is_empty() {
            fields.remove("tunnel");
        } else {
            let mut tun = toml::Table::new();
            tun.insert("ssh".into(), toml::Value::String(self.tunnel_ssh.clone()));
            fields.insert("tunnel".into(), toml::Value::Table(tun));
        }
    }

    /// Sidebar subtitle: "MongoDB · 127.0.0.1:27017/shop".
    pub fn label(&self) -> String {
        let name = self
            .store_kind()
            .map(|k| k.label())
            .unwrap_or(self.kind.as_str());
        if !self.uri.is_empty() {
            // Never show a URI: it may carry a password.
            return format!("{name} · {}", self.database);
        }
        if self.database.is_empty() {
            format!("{name} · {}:{}", self.host, self.port)
        } else {
            format!("{name} · {}:{}/{}", self.host, self.port, self.database)
        }
    }

    fn resolve_common(
        &self,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<Resolved, ProfileError> {
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s,
            Err(m) => {
                missing.extend(m);
                v.to_string()
            }
        };
        let uri = sub(&self.uri);
        let host = sub(&self.host);
        let user = sub(&self.user);
        let database = sub(&self.database);
        let auth_source = sub(&self.auth_source);
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
        Ok(Resolved {
            uri,
            host,
            user,
            database,
            auth_source,
            password,
        })
    }

    /// Resolve into the MongoDB driver's config.
    pub fn resolve_mongo(
        &self,
        read_only: bool,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<crate::mongo::ConnectConfig, ProfileError> {
        let r = self.resolve_common(secrets, lookup)?;
        Ok(crate::mongo::ConnectConfig {
            uri: r.uri,
            host: r.host,
            port: self.port,
            user: r.user,
            password: r.password,
            auth_source: r.auth_source,
            database: r.database,
            tls: self.tls,
            direct: self.direct,
            read_only,
            connect_timeout: Duration::from_secs(10),
        })
    }

    /// Resolve into the CouchDB driver's config.
    pub fn resolve_couch(
        &self,
        read_only: bool,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<crate::couch::ConnectConfig, ProfileError> {
        let r = self.resolve_common(secrets, lookup)?;
        let url = if r.uri.is_empty() {
            let scheme = if self.tls { "https" } else { "http" };
            format!("{scheme}://{}:{}", r.host, self.port)
        } else {
            r.uri
        };
        Ok(crate::couch::ConnectConfig {
            url,
            user: r.user,
            password: r.password,
            database: r.database,
            read_only,
            connect_timeout: Duration::from_secs(10),
        })
    }
}

struct Resolved {
    uri: String,
    host: String,
    user: String,
    database: String,
    auth_source: String,
    password: Option<String>,
}

/// Read a `mongodb://`, `mongodb+srv://`, `http://` or `https://` URI into
/// form fields. A Mongo SRV URI keeps its URI form, since the host list comes
/// from DNS.
pub fn parse_url(url: &str) -> Option<NoSqlFields> {
    let url = url.trim();
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    let mut f = NoSqlFields::default();
    match scheme.as_str() {
        "mongodb" => f.kind = "mongodb".into(),
        "mongodb+srv" => {
            // The driver resolves the seed list itself.
            f.kind = "mongodb".into();
            f.uri = url.to_string();
            return Some(f);
        }
        "http" | "https" => {
            f.kind = "couchdb".into();
            f.tls = scheme == "https";
            f.port = 5984;
        }
        _ => return None,
    }
    if scheme == "mongodb" {
        f.port = 27017;
    }
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
    // A Mongo URI may list several hosts; the form shows the first.
    let first = hostport.split(',').next().unwrap_or(hostport);
    match first.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() => {
            f.host = h.trim_matches(['[', ']']).to_string();
            f.port = p.parse().ok()?;
        }
        _ => f.host = first.trim_matches(['[', ']']).to_string(),
    }
    let db = path.split('?').next().unwrap_or("");
    if !db.is_empty() {
        f.database = percent_decode(db);
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
    fn a_mongo_uri_fills_the_form() {
        let f = parse_url("mongodb://alice:p%40ss@db.example.com:27018/shop").unwrap();
        assert_eq!(f.kind, "mongodb");
        assert_eq!(f.host, "db.example.com");
        assert_eq!(f.port, 27018);
        assert_eq!(f.user, "alice");
        assert_eq!(f.password, "p@ss");
        assert_eq!(f.database, "shop");
        // A replica set lists several hosts; the form shows the first.
        let f = parse_url("mongodb://a.example:27017,b.example:27017/shop").unwrap();
        assert_eq!(f.host, "a.example");
        // SRV keeps the URI, because DNS holds the seed list.
        let f = parse_url("mongodb+srv://user@cluster.example.net/shop").unwrap();
        assert!(f.uri.starts_with("mongodb+srv://"));
    }

    #[test]
    fn a_couch_url_fills_the_form() {
        let f = parse_url("http://admin:secret@localhost:5984/orders").unwrap();
        assert_eq!(f.kind, "couchdb");
        assert!(!f.tls);
        assert_eq!(f.port, 5984);
        assert_eq!(f.database, "orders");
        let f = parse_url("https://couch.example.com").unwrap();
        assert!(f.tls);
        assert_eq!(f.port, 5984);
        assert_eq!(parse_url("redis://x"), None);
    }

    #[test]
    fn fields_round_trip_and_keep_one_kind_only() {
        let f = NoSqlFields {
            kind: "mongodb".into(),
            host: "db".into(),
            port: 27018,
            user: "alice".into(),
            password: "secret:01J".into(),
            database: "shop".into(),
            tunnel_ssh: "ssh-1".into(),
            ..Default::default()
        };
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        assert_eq!(NoSqlFields::from_table("mongodb", &table), f);
        // Switching kind replaces the table rather than leaving both.
        let c = NoSqlFields {
            kind: "couchdb".into(),
            port: 5984,
            ..Default::default()
        };
        c.write_into(&mut table);
        assert!(!table.contains_key("mongodb"));
        assert_eq!(NoSqlFields::from_table("couchdb", &table).kind, "couchdb");
    }

    #[test]
    fn resolving_reports_missing_variables() {
        let f = NoSqlFields {
            host: "{{db_host}}".into(),
            ..Default::default()
        };
        let lookup = |v: &str| {
            if v.contains("{{") {
                Err(vec!["db_host".to_string()])
            } else {
                Ok(v.to_string())
            }
        };
        assert_eq!(
            f.resolve_mongo(false, &NoSecrets, &lookup).err(),
            Some(ProfileError::MissingVariables(vec!["db_host".into()]))
        );
    }

    #[test]
    fn couch_builds_its_url_from_the_host_and_tls_flag() {
        let f = NoSqlFields {
            kind: "couchdb".into(),
            host: "couch".into(),
            port: 5984,
            ..Default::default()
        };
        let cfg = f
            .resolve_couch(true, &NoSecrets, &|v: &str| Ok(v.to_string()))
            .unwrap();
        assert_eq!(cfg.url, "http://couch:5984");
        assert!(cfg.read_only);
        let f = NoSqlFields { tls: true, ..f };
        assert_eq!(
            f.resolve_couch(false, &NoSecrets, &|v: &str| Ok(v.to_string()))
                .unwrap()
                .url,
            "https://couch:5984"
        );
    }

    #[test]
    fn the_label_never_shows_a_uri() {
        let f = NoSqlFields {
            uri: "mongodb://user:secret@host/shop".into(),
            database: "shop".into(),
            ..Default::default()
        };
        assert!(!f.label().contains("secret"));
        assert_eq!(NoSqlFields::default().label(), "MongoDB · 127.0.0.1:27017");
    }

    #[test]
    fn each_kind_has_its_own_default_port() {
        assert_eq!(NoSqlFields::default_port("mongodb"), 27017);
        assert_eq!(NoSqlFields::default_port("couchdb"), 5984);
        assert_eq!(NoSqlFields::default().store_kind(), Some(StoreKind::Mongo));
        assert_eq!(
            NoSqlFields {
                kind: "cassandra".into(),
                ..Default::default()
            }
            .store_kind(),
            None
        );
    }
}
