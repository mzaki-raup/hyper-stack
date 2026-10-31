//! The `[db]` table of a `kind = "db"` connection profile
//! (docs/05-data-model-and-storage.md §2.2), connection-URL import, and the
//! resolution of a profile into a [`ConnectConfig`]: `{{variables}}` from the
//! profile's environment, the password from the vault.

use std::time::Duration;

use hs_core::secrets::{SecretRef, SecretStore};
use secrecy::ExposeSecret;

use crate::driver::{ConnectConfig, Engine};

pub const SSL_MODES: &[&str] = &["disable", "prefer", "require", "verify-ca", "verify-full"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DbFields {
    /// postgres | mysql | mariadb | sqlite | mssql | clickhouse | duckdb |
    /// oracle | db2
    pub engine: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    /// `secret:<id>`, `{{var}}`, or typed text until the form saves it.
    pub password: String,
    pub ssl: String,
    pub ssl_ca: String,
    /// SQLite and DuckDB database file.
    pub path: String,
    /// Oracle and Db2: the ODBC driver, as ODBC names it; empty picks the
    /// installed one for the engine.
    pub odbc_driver: String,
    /// Id of an SSH connection to tunnel through (`[tunnel] ssh`).
    pub tunnel_ssh: String,
}

impl Default for DbFields {
    fn default() -> Self {
        Self {
            engine: "postgres".into(),
            host: "localhost".into(),
            port: 5432,
            database: String::new(),
            user: String::new(),
            password: String::new(),
            ssl: "prefer".into(),
            ssl_ca: String::new(),
            path: String::new(),
            odbc_driver: String::new(),
            tunnel_ssh: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    VaultLocked,
    UnknownEngine(String),
    /// `{{names}}` the environment does not define.
    MissingVariables(Vec<String>),
    Secret(String),
}

impl DbFields {
    pub fn engine(&self) -> Option<Engine> {
        Engine::parse(&self.engine)
    }

    pub fn from_table(fields: &toml::Table) -> Self {
        let t = fields
            .get("db")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d = Self::default();
        let engine = Some(s("engine"))
            .filter(|e| !e.is_empty())
            .unwrap_or(d.engine);
        let port = t
            .get("port")
            .and_then(|v| v.as_integer())
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or_else(|| {
                Engine::parse(&engine)
                    .map(Engine::default_port)
                    .unwrap_or(0)
            });
        Self {
            engine,
            host: s("host"),
            port,
            database: s("database"),
            user: s("user"),
            password: s("password"),
            ssl: Some(s("ssl")).filter(|x| !x.is_empty()).unwrap_or(d.ssl),
            ssl_ca: s("ssl_ca"),
            path: s("path"),
            odbc_driver: s("odbc_driver"),
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
        put("engine", &self.engine);
        if self.engine().is_some_and(Engine::is_file) {
            put("path", &self.path);
        } else {
            if self.engine().is_some_and(Engine::is_odbc) {
                put("odbc_driver", &self.odbc_driver);
            }
            put("host", &self.host);
            put("database", &self.database);
            put("user", &self.user);
            put("password", &self.password);
            put("ssl", &self.ssl);
            put("ssl_ca", &self.ssl_ca);
            t.insert("port".into(), toml::Value::Integer(self.port as i64));
        }
        fields.insert("db".into(), toml::Value::Table(t));
        if self.tunnel_ssh.is_empty() || self.engine().is_some_and(Engine::is_file) {
            fields.remove("tunnel");
        } else {
            let mut tun = toml::Table::new();
            tun.insert("ssh".into(), toml::Value::String(self.tunnel_ssh.clone()));
            fields.insert("tunnel".into(), toml::Value::Table(tun));
        }
    }

    /// Sidebar subtitle: "PostgreSQL · db.example.com/orders".
    pub fn label(&self) -> String {
        match self.engine() {
            Some(e) if e.is_file() => {
                let file = std::path::Path::new(&self.path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| self.path.clone());
                format!("{} · {file}", e.label())
            }
            Some(e) => {
                let short = match e {
                    Engine::Mysql if self.engine.eq_ignore_ascii_case("mariadb") => "MariaDB",
                    Engine::Mysql => "MySQL",
                    other => other.label(),
                };
                if self.database.is_empty() {
                    format!("{short} · {}", self.host)
                } else {
                    format!("{short} · {}/{}", self.host, self.database)
                }
            }
            None => self.engine.clone(),
        }
    }

    /// Resolve variables and secrets. `lookup` substitutes `{{names}}` from
    /// the profile's environment (it returns the names it could not find).
    pub fn resolve(
        &self,
        read_only: bool,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<ConnectConfig, ProfileError> {
        let engine = self
            .engine()
            .ok_or_else(|| ProfileError::UnknownEngine(self.engine.clone()))?;
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s,
            Err(m) => {
                missing.extend(m);
                v.to_string()
            }
        };
        let host = sub(&self.host);
        let database = sub(&self.database);
        let user = sub(&self.user);
        let path = sub(&self.path);
        let ssl_ca = sub(&self.ssl_ca);
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
            engine,
            host: if host.is_empty() {
                "localhost".into()
            } else {
                host
            },
            port: if self.port == 0 {
                engine.default_port()
            } else {
                self.port
            },
            database,
            user,
            password,
            ssl: self.ssl.clone(),
            ssl_ca: Some(ssl_ca).filter(|c| !c.is_empty()),
            path,
            odbc_driver: self.odbc_driver.clone(),
            read_only,
            connect_timeout: Duration::from_secs(10),
        })
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            let v = (h * 16 + l) as u8;
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parse a pasted connection string: `postgres://user:pass@host:5432/db?sslmode=require`,
/// `mysql://…`, `mariadb://…`, `jdbc:postgresql://…`, `sqlite:///path/file.db`
/// or `sqlite:file.db`. The password comes back separately so the caller can
/// put it into the vault.
pub fn parse_url(url: &str) -> Option<(DbFields, Option<String>)> {
    let url = url.trim();
    let url = url.strip_prefix("jdbc:").unwrap_or(url);
    let (scheme, rest) = url.split_once(':')?;
    let engine = match scheme.to_ascii_lowercase().as_str() {
        "postgres" | "postgresql" => "postgres",
        "mysql" => "mysql",
        "mariadb" => "mariadb",
        "sqlite" | "sqlite3" | "file" => "sqlite",
        _ => return None,
    };
    if engine == "sqlite" {
        let path = rest.strip_prefix("//").unwrap_or(rest);
        // sqlite:///C:/x.db → C:/x.db ; sqlite:///home/x.db → /home/x.db
        let path = match path.strip_prefix('/') {
            Some(p) if p.len() > 1 && p.as_bytes()[1] == b':' => p,
            _ => path,
        };
        let path = path.split('?').next().unwrap_or(path);
        return Some((
            DbFields {
                engine: engine.into(),
                path: percent_decode(path),
                ..Default::default()
            },
            None,
        ));
    }
    let rest = rest.strip_prefix("//")?;
    let (rest, query) = match rest.split_once('?') {
        Some((a, q)) => (a, Some(q)),
        None => (rest, None),
    };
    let (authority, database) = match rest.split_once('/') {
        Some((a, d)) => (a, d.trim_end_matches('/')),
        None => (rest, ""),
    };
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, authority),
    };
    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((a, b)) => (percent_decode(a), Some(percent_decode(b))),
            None => (percent_decode(u), None),
        },
        None => (String::new(), None),
    };
    let default_port = Engine::parse(engine).map(Engine::default_port).unwrap_or(0);
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        // [::1]:5432
        let (h, p) = h.split_once(']')?;
        (
            h.to_string(),
            p.strip_prefix(':').and_then(|p| p.parse().ok()),
        )
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()),
            None => (hostport.to_string(), None),
        }
    };
    let mut f = DbFields {
        engine: engine.into(),
        host: if host.is_empty() {
            "localhost".into()
        } else {
            host
        },
        port: port.unwrap_or(default_port),
        database: percent_decode(database),
        user,
        ..Default::default()
    };
    for pair in query.unwrap_or("").split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k.to_ascii_lowercase().as_str() {
            "sslmode" | "ssl-mode" => {
                let v = v.to_ascii_lowercase().replace('_', "-");
                f.ssl = match v.as_str() {
                    "required" => "require".into(),
                    "preferred" => "prefer".into(),
                    "disabled" => "disable".into(),
                    "verify-identity" => "verify-full".into(),
                    other => other.to_string(),
                };
            }
            "sslrootcert" => f.ssl_ca = percent_decode(v),
            "user" if f.user.is_empty() => f.user = percent_decode(v),
            _ => {}
        }
    }
    if !SSL_MODES.contains(&f.ssl.as_str()) {
        f.ssl = "prefer".into();
    }
    Some((f, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip_through_the_profile_table() {
        let f = DbFields {
            engine: "postgres".into(),
            host: "{{db_host}}".into(),
            port: 6543,
            database: "orders".into(),
            user: "app".into(),
            password: "secret:01ABC".into(),
            ssl: "require".into(),
            ssl_ca: String::new(),
            path: String::new(),
            odbc_driver: String::new(),
            tunnel_ssh: "01SSH".into(),
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(DbFields::from_table(&t), f);
        // ADR-030: an ODBC engine keeps its driver; DuckDB is a file.
        let oracle = DbFields {
            engine: "oracle".into(),
            port: 1521,
            odbc_driver: "Oracle in instantclient_23_9".into(),
            ..f.clone()
        };
        let mut t = toml::Table::new();
        oracle.write_into(&mut t);
        assert_eq!(DbFields::from_table(&t).odbc_driver, oracle.odbc_driver);
        let duck = DbFields {
            engine: "duckdb".into(),
            path: "C:/data/a.duckdb".into(),
            ..Default::default()
        };
        let mut t = toml::Table::new();
        duck.write_into(&mut t);
        assert_eq!(DbFields::from_table(&t).path, duck.path);
        assert_eq!(duck.label(), "DuckDB · a.duckdb");
        let mut no_tunnel = f.clone();
        no_tunnel.tunnel_ssh.clear();
        no_tunnel.write_into(&mut t);
        assert!(!t.contains_key("tunnel"));
        assert_eq!(DbFields::from_table(&toml::Table::new()).port, 5432);
    }

    #[test]
    fn parses_connection_urls() {
        let (f, pw) =
            parse_url("postgres://app:p%40ss@db.example.com:6543/orders?sslmode=verify-full")
                .unwrap();
        assert_eq!(f.engine, "postgres");
        assert_eq!(f.host, "db.example.com");
        assert_eq!(f.port, 6543);
        assert_eq!(f.database, "orders");
        assert_eq!(f.user, "app");
        assert_eq!(pw.as_deref(), Some("p@ss"));
        assert_eq!(f.ssl, "verify-full");

        let (f, pw) = parse_url("jdbc:mysql://localhost/shop?sslMode=REQUIRED").unwrap();
        assert_eq!(
            (f.engine.as_str(), f.port, f.ssl.as_str()),
            ("mysql", 3306, "require")
        );
        assert!(pw.is_none());

        let (f, _) = parse_url("mariadb://u@[::1]:3307").unwrap();
        assert_eq!((f.host.as_str(), f.port), ("::1", 3307));

        assert_eq!(
            parse_url("sqlite:///C:/data/app.db").unwrap().0.path,
            "C:/data/app.db"
        );
        assert_eq!(
            parse_url("sqlite:///home/a/app.db").unwrap().0.path,
            "/home/a/app.db"
        );
        assert_eq!(parse_url("sqlite:app.db").unwrap().0.path, "app.db");
        assert!(parse_url("redis://x").is_none());
    }

    #[test]
    fn resolve_substitutes_and_reads_the_vault() {
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
                unreachable!()
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
        let lookup = |v: &str| -> Result<String, Vec<String>> {
            if v.contains("{{missing}}") {
                Err(vec!["missing".into()])
            } else {
                Ok(v.replace("{{db_host}}", "10.0.0.5"))
            }
        };
        let mut f = DbFields {
            host: "{{db_host}}".into(),
            password: "plain".into(),
            ..Default::default()
        };
        let c = f.resolve(true, &Locked, &lookup).unwrap();
        assert_eq!(c.host, "10.0.0.5");
        assert_eq!(c.password.as_deref(), Some("plain"));
        assert!(c.read_only);
        f.password = "secret:01X".into();
        assert_eq!(
            f.resolve(false, &Locked, &lookup).unwrap_err(),
            ProfileError::VaultLocked
        );
        f.user = "{{missing}}".into();
        assert_eq!(
            f.resolve(false, &Locked, &lookup).unwrap_err(),
            ProfileError::MissingVariables(vec!["missing".into()])
        );
        // Oracle was the example once; since ADR-030 it is an engine.
        f.engine = "informix".into();
        assert!(matches!(
            f.resolve(false, &Locked, &lookup),
            Err(ProfileError::UnknownEngine(_))
        ));
    }
}
