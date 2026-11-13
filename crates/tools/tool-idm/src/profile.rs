//! The `[idm]` table of an identity server's connection profile
//! (docs/tools/idm.md §5.1), its credential through the shared
//! `hs_core::service_auth` (ADR-028), and its resolution into a
//! provider's config.

use std::path::PathBuf;
use std::time::Duration;

use hs_core::secrets::SecretStore;
use hs_core::service_auth::{Auth, AuthError, Credential};
use hs_core::service_http::Tls;
use secrecy::SecretString;

use crate::provider::keycloak::KeycloakConfig;
use crate::provider::keycloak::auth::{ADMIN_CLI, Grant};

/// The credential kinds an identity server takes: `api-key` is a
/// confidential client's secret (Client Credentials), `password` an
/// admin's password (Admin Password), `token` an offline token.
pub const AUTH_KINDS: &[&str] = &["api-key", "password", "token"];

/// The request timeout (§7: 15 s by default).
pub const TIMEOUT: Duration = Duration::from_secs(15);

pub fn default_url(kind: &str) -> &'static str {
    match kind {
        "keycloak" => "http://127.0.0.1:8180",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdmFields {
    /// The base URL, no `/admin`; `/auth` included for a pre-17 server.
    pub url: String,
    /// The realm the token is requested from.
    pub auth_realm: String,
    /// The realm the tab opens on; empty means `auth_realm`.
    pub default_realm: String,
    /// The confidential client (Client Credentials), or `admin-cli`.
    pub client_id: String,
    pub tls_verify: bool,
    /// A PEM file of extra roots (a company CA); empty for none.
    pub ca_file: String,
    pub credential: Credential,
}

impl Default for IdmFields {
    fn default() -> Self {
        IdmFields {
            url: String::new(),
            auth_realm: "master".into(),
            default_realm: String::new(),
            client_id: String::new(),
            tls_verify: true,
            ca_file: String::new(),
            credential: Credential {
                auth: "api-key".into(),
                ..Default::default()
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    Auth(AuthError),
    MissingVariables(Vec<String>),
    Invalid(String),
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProfileError::Auth(e) => e.fmt(f),
            ProfileError::MissingVariables(v) => write!(f, "undefined variables: {}", v.join(", ")),
            ProfileError::Invalid(e) => f.write_str(e),
        }
    }
}

/// A profile resolved for one provider.
#[derive(Debug, Clone)]
pub enum ConnectConfig {
    Keycloak(KeycloakConfig),
}

impl ConnectConfig {
    pub fn url(&self) -> &str {
        match self {
            ConnectConfig::Keycloak(k) => &k.url,
        }
    }
}

impl IdmFields {
    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get("idm")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut credential = Credential::from_table(&t);
        // `none` is no answer for an admin API; the form's default is
        // Client Credentials.
        if !AUTH_KINDS.contains(&credential.auth.as_str()) {
            credential.auth = "api-key".into();
        }
        let url = s("url");
        let auth_realm = s("auth_realm");
        IdmFields {
            url: if url.is_empty() {
                default_url(kind).to_string()
            } else {
                url
            },
            auth_realm: if auth_realm.is_empty() {
                "master".into()
            } else {
                auth_realm
            },
            default_realm: s("default_realm"),
            client_id: s("client_id"),
            tls_verify: t
                .get("tls_verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            ca_file: s("ca_file"),
            credential,
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        for (k, v) in [
            ("url", &self.url),
            ("auth_realm", &self.auth_realm),
            ("default_realm", &self.default_realm),
            ("client_id", &self.client_id),
            ("ca_file", &self.ca_file),
        ] {
            t.insert(k.into(), toml::Value::String(v.clone()));
        }
        t.insert("tls_verify".into(), toml::Value::Boolean(self.tls_verify));
        self.credential.write_into(&mut t);
        fields.insert("idm".into(), toml::Value::Table(t));
    }

    /// The realm a tab opens on.
    pub fn opening_realm(&self) -> &str {
        if self.default_realm.trim().is_empty() {
            &self.auth_realm
        } else {
            &self.default_realm
        }
    }

    /// "Keycloak · 127.0.0.1:8180", for a row's second line.
    pub fn label_for(&self, kind: &str) -> String {
        format!(
            "{} · {}",
            hs_core::connections::kind_label(kind),
            short(&self.url)
        )
    }

    /// Resolve variables and the vault secret into a provider config.
    pub fn resolve(
        &self,
        kind: &str,
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
        let url = sub(&self.url);
        let auth_realm = sub(&self.auth_realm);
        let client_id = sub(&self.client_id);
        let ca_file = sub(&self.ca_file);
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(ProfileError::MissingVariables(missing));
        }
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ProfileError::Invalid(format!(
                "“{url}” is not an http(s) URL"
            )));
        }
        if url.ends_with("/admin") {
            return Err(ProfileError::Invalid(
                "the base URL ends before /admin: leave /admin off".into(),
            ));
        }
        let auth_realm = auth_realm.trim().to_string();
        if auth_realm.is_empty() {
            return Err(ProfileError::Invalid("the auth realm is missing".into()));
        }
        let auth = self
            .credential
            .resolve(secrets, lookup)
            .map_err(|e| match e {
                AuthError::MissingVariables(v) => ProfileError::MissingVariables(v),
                other => ProfileError::Auth(other),
            })?;
        let client_id = client_id.trim().to_string();
        let grant = match auth {
            Auth::ApiKey { key, .. } => {
                if client_id.is_empty() {
                    return Err(ProfileError::Invalid(
                        "Client Credentials needs the client id".into(),
                    ));
                }
                Grant::ClientCredentials {
                    client_id,
                    secret: SecretString::from(key),
                }
            }
            Auth::Password { user, password } => {
                if user.trim().is_empty() {
                    return Err(ProfileError::Invalid(
                        "Admin Password needs the username".into(),
                    ));
                }
                Grant::Password {
                    client_id: if client_id.is_empty() {
                        ADMIN_CLI.into()
                    } else {
                        client_id
                    },
                    username: user.trim().to_string(),
                    password: SecretString::from(password),
                }
            }
            Auth::Token { token, .. } => Grant::OfflineToken {
                client_id: if client_id.is_empty() {
                    ADMIN_CLI.into()
                } else {
                    client_id
                },
                token: SecretString::from(token),
            },
            Auth::None => {
                return Err(ProfileError::Invalid(
                    "an identity server needs a credential".into(),
                ));
            }
        };
        let tls = Tls {
            verify: self.tls_verify,
            ca_file: (!ca_file.trim().is_empty()).then(|| PathBuf::from(ca_file.trim())),
        };
        match kind {
            "keycloak" => Ok(ConnectConfig::Keycloak(KeycloakConfig {
                url,
                auth_realm,
                grant,
                tls,
                timeout: TIMEOUT,
            })),
            other => Err(ProfileError::Invalid(format!(
                "{other} is not an identity server this version knows"
            ))),
        }
    }
}

/// A URL without its scheme.
pub fn short(url: &str) -> &str {
    url.split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(url)
        .trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoVault;
    impl SecretStore for NoVault {
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
        fn get(
            &self,
            _: &hs_core::secrets::SecretRef,
        ) -> hs_core::secrets::Result<secrecy::SecretString> {
            unreachable!("plain values only")
        }
        fn set(
            &self,
            _: &hs_core::secrets::SecretRef,
            _: &str,
            _: &secrecy::SecretString,
        ) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn delete(&self, _: &hs_core::secrets::SecretRef) -> hs_core::secrets::Result<()> {
            Ok(())
        }
        fn list(&self) -> hs_core::secrets::Result<Vec<(hs_core::secrets::SecretRef, String)>> {
            Ok(Vec::new())
        }
    }

    fn same(v: &str) -> Result<String, Vec<String>> {
        Ok(v.to_string())
    }

    fn fields(auth: &str, user: &str, client_id: &str) -> IdmFields {
        IdmFields {
            url: "http://127.0.0.1:8180/".into(),
            auth_realm: "hs-demo".into(),
            client_id: client_id.into(),
            credential: Credential {
                auth: auth.into(),
                user: user.into(),
                secret: "s".into(),
                header: String::new(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn fields_round_trip_through_the_idm_table() {
        let f = IdmFields {
            url: "https://id.example.test".into(),
            auth_realm: "master".into(),
            default_realm: "hs-demo".into(),
            client_id: "hs-admin".into(),
            tls_verify: false,
            ca_file: "C:/certs/corp.pem".into(),
            credential: Credential {
                auth: "api-key".into(),
                secret: "secret:01J0000000000000000000000".into(),
                ..Default::default()
            },
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(IdmFields::from_table("keycloak", &t), f);
        assert!(t["idm"].as_table().unwrap().contains_key("auth"));
        let empty = IdmFields::from_table("keycloak", &toml::Table::new());
        assert_eq!(empty.url, "http://127.0.0.1:8180");
        assert_eq!(empty.auth_realm, "master");
        assert_eq!(empty.credential.auth, "api-key");
        assert!(empty.tls_verify);
        assert_eq!(empty.opening_realm(), "master");
        assert_eq!(f.opening_realm(), "hs-demo");
    }

    #[test]
    fn each_kind_resolves_to_its_grant() {
        let r = |f: IdmFields| match f.resolve("keycloak", &NoVault, &same) {
            Ok(ConnectConfig::Keycloak(k)) => Ok(k),
            Err(e) => Err(e),
        };
        let k = r(fields("api-key", "", "hs-admin")).unwrap();
        assert_eq!(k.url, "http://127.0.0.1:8180");
        assert!(
            matches!(k.grant, Grant::ClientCredentials { ref client_id, .. } if client_id == "hs-admin")
        );
        let k = r(fields("password", "admin", "")).unwrap();
        assert!(
            matches!(k.grant, Grant::Password { ref client_id, ref username, .. } if client_id == "admin-cli" && username == "admin")
        );
        let k = r(fields("token", "", "")).unwrap();
        assert!(matches!(k.grant, Grant::OfflineToken { .. }));
        assert!(matches!(
            r(fields("api-key", "", "")),
            Err(ProfileError::Invalid(_))
        ));
        assert!(matches!(
            r(fields("password", "", "")),
            Err(ProfileError::Invalid(_))
        ));
        let mut bad = fields("api-key", "", "hs-admin");
        bad.url = "127.0.0.1:8180".into();
        assert!(matches!(r(bad), Err(ProfileError::Invalid(_))));
        let mut ca = fields("api-key", "", "hs-admin");
        ca.ca_file = "C:/certs/corp.pem".into();
        assert_eq!(
            r(ca).unwrap().tls.ca_file,
            Some(PathBuf::from("C:/certs/corp.pem"))
        );
    }
}
