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
use crate::provider::ldap::LdapConfig;

/// The credential kinds an identity server takes: `api-key` is a
/// confidential client's secret (Client Credentials), `password` an
/// admin's password (Admin Password), `token` an offline token.
pub const AUTH_KINDS: &[&str] = &["api-key", "password", "token"];

/// A directory binds with a DN and its password, or anonymously, which
/// reads only (§4.12).
pub const LDAP_AUTH_KINDS: &[&str] = &["password", "none"];

/// The credential kinds `kind` takes.
pub fn auth_kinds(kind: &str) -> &'static [&'static str] {
    match kind {
        "ldap" => LDAP_AUTH_KINDS,
        _ => AUTH_KINDS,
    }
}

/// A search's page size against a directory: OpenLDAP and AD both cap a
/// page at 1000 by default.
pub const LDAP_PAGE_SIZE: u32 = 500;

/// The request timeout (§7: 15 s by default).
pub const TIMEOUT: Duration = Duration::from_secs(15);

pub fn default_url(kind: &str) -> &'static str {
    match kind {
        "keycloak" => "http://127.0.0.1:8180",
        "ldap" => "ldap://127.0.0.1:1389",
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
    /// A master-realm account used only to read the version, when the
    /// main one is not shown it (CR-0099). Off by default.
    pub lookup: bool,
    pub lookup_client_id: String,
    pub lookup_credential: Credential,
    /// LDAP: StartTLS on an `ldap://` URL.
    pub starttls: bool,
    /// LDAP: where users and groups are searched and created; empty for
    /// the whole naming context.
    pub users_base: String,
    pub groups_base: String,
    /// LDAP: the filters that say what a user and a group are; empty for
    /// the provider's defaults.
    pub user_filter: String,
    pub group_filter: String,
    pub page_size: u32,
}

/// The sign-in kinds of the Version Lookup account: a master-realm
/// client's secret, or an admin's password through `admin-cli`.
pub const LOOKUP_AUTH_KINDS: &[&str] = &["api-key", "password"];

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
            lookup: false,
            lookup_client_id: String::new(),
            lookup_credential: Credential {
                auth: "password".into(),
                ..Default::default()
            },
            starttls: false,
            users_base: String::new(),
            groups_base: String::new(),
            user_filter: String::new(),
            group_filter: String::new(),
            page_size: LDAP_PAGE_SIZE,
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
    Ldap(LdapConfig),
}

impl ConnectConfig {
    pub fn url(&self) -> &str {
        match self {
            ConnectConfig::Keycloak(k) => &k.url,
            ConnectConfig::Ldap(l) => &l.url,
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
        // Client Credentials. A directory's is a bind DN and password - a
        // new profile's, not one saved as anonymous.
        if !auth_kinds(kind).contains(&credential.auth.as_str()) || !t.contains_key("auth") {
            credential.auth = auth_kinds(kind)[0].into();
        }
        let url = s("url");
        let auth_realm = s("auth_realm");
        let lt = t
            .get("version_lookup")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let mut lookup_credential = Credential::from_table(&lt);
        if !LOOKUP_AUTH_KINDS.contains(&lookup_credential.auth.as_str()) {
            lookup_credential.auth = "password".into();
        }
        IdmFields {
            url: if url.is_empty() {
                default_url(kind).to_string()
            } else {
                url
            },
            // A directory has no auth realm: its tab opens on the default
            // base, else the server's first naming context.
            auth_realm: if auth_realm.is_empty() && kind != "ldap" {
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
            lookup: lt.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false),
            lookup_client_id: lt
                .get("client_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            lookup_credential,
            starttls: t.get("starttls").and_then(|v| v.as_bool()).unwrap_or(false),
            users_base: s("users_base"),
            groups_base: s("groups_base"),
            user_filter: s("user_filter"),
            group_filter: s("group_filter"),
            page_size: t
                .get("page_size")
                .and_then(|v| v.as_integer())
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
                .unwrap_or(LDAP_PAGE_SIZE),
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
            ("users_base", &self.users_base),
            ("groups_base", &self.groups_base),
            ("user_filter", &self.user_filter),
            ("group_filter", &self.group_filter),
        ] {
            t.insert(k.into(), toml::Value::String(v.clone()));
        }
        t.insert("tls_verify".into(), toml::Value::Boolean(self.tls_verify));
        t.insert("starttls".into(), toml::Value::Boolean(self.starttls));
        t.insert(
            "page_size".into(),
            toml::Value::Integer(i64::from(self.page_size)),
        );
        self.credential.write_into(&mut t);
        let mut lt = toml::Table::new();
        lt.insert("enabled".into(), toml::Value::Boolean(self.lookup));
        lt.insert(
            "client_id".into(),
            toml::Value::String(self.lookup_client_id.clone()),
        );
        self.lookup_credential.write_into(&mut lt);
        t.insert("version_lookup".into(), toml::Value::Table(lt));
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
        if kind == "ldap" {
            return self.resolve_ldap(url, ca_file, secrets, lookup);
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
        let auth_error = |e| match e {
            AuthError::MissingVariables(v) => ProfileError::MissingVariables(v),
            other => ProfileError::Auth(other),
        };
        let auth = self
            .credential
            .resolve(secrets, lookup)
            .map_err(auth_error)?;
        let grant = grant_of(auth, client_id.trim().to_string())?;
        let version_lookup = if self.lookup {
            let auth = self
                .lookup_credential
                .resolve(secrets, lookup)
                .map_err(auth_error)?;
            let client_id =
                lookup(&self.lookup_client_id).map_err(ProfileError::MissingVariables)?;
            Some(grant_of(auth, client_id.trim().to_string())?)
        } else {
            None
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
                version_lookup,
            })),
            other => Err(ProfileError::Invalid(format!(
                "{other} is not an identity server this version knows"
            ))),
        }
    }
}

impl IdmFields {
    /// A directory's profile (§4.12): an `ldap://` or `ldaps://` URL, the
    /// bind DN and its password, or an anonymous bind.
    fn resolve_ldap(
        &self,
        url: String,
        ca_file: String,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<ConnectConfig, ProfileError> {
        let url = url.trim().trim_end_matches('/').to_string();
        if !url.starts_with("ldap://") && !url.starts_with("ldaps://") {
            return Err(ProfileError::Invalid(format!(
                "“{url}” is not an ldap:// or ldaps:// URL"
            )));
        }
        let mut missing = Vec::new();
        let mut sub = |v: &str| match lookup(v) {
            Ok(s) => s.trim().to_string(),
            Err(m) => {
                missing.extend(m);
                String::new()
            }
        };
        let default_base = sub(&self.default_realm);
        let users_base = sub(&self.users_base);
        let groups_base = sub(&self.groups_base);
        let user_filter = sub(&self.user_filter);
        let group_filter = sub(&self.group_filter);
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            return Err(ProfileError::MissingVariables(missing));
        }
        let auth = self
            .credential
            .resolve(secrets, lookup)
            .map_err(|e| match e {
                AuthError::MissingVariables(v) => ProfileError::MissingVariables(v),
                other => ProfileError::Auth(other),
            })?;
        let (bind_dn, password) = match auth {
            Auth::Password { user, password } => {
                if user.trim().is_empty() {
                    return Err(ProfileError::Invalid("a bind needs its DN".into()));
                }
                (user.trim().to_string(), Some(SecretString::from(password)))
            }
            Auth::None => (String::new(), None),
            _ => {
                return Err(ProfileError::Invalid(
                    "a directory binds with a DN and password, or anonymously".into(),
                ));
            }
        };
        Ok(ConnectConfig::Ldap(LdapConfig {
            url,
            starttls: self.starttls,
            tls_verify: self.tls_verify,
            ca_file: ca_file.trim().to_string(),
            bind_dn,
            password,
            default_base,
            users_base,
            groups_base,
            user_filter,
            group_filter,
            page_size: self.page_size,
            timeout: TIMEOUT,
        }))
    }
}

/// A resolved credential and client id as the grant Keycloak takes.
fn grant_of(auth: Auth, client_id: String) -> Result<Grant, ProfileError> {
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
    Ok(grant)
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
            lookup: true,
            lookup_client_id: String::new(),
            lookup_credential: Credential {
                auth: "password".into(),
                user: "admin".into(),
                secret: "secret:01J0000000000000000000001".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(IdmFields::from_table("keycloak", &t), f);
        assert!(t["idm"].as_table().unwrap().contains_key("auth"));
        assert!(
            t["idm"]["version_lookup"]
                .as_table()
                .unwrap()
                .contains_key("secret")
        );
        let empty = IdmFields::from_table("keycloak", &toml::Table::new());
        assert_eq!(empty.url, "http://127.0.0.1:8180");
        assert_eq!(empty.auth_realm, "master");
        assert_eq!(empty.credential.auth, "api-key");
        assert!(!empty.lookup, "the version lookup is off by default");
        assert_eq!(empty.lookup_credential.auth, "password");
        assert!(empty.tls_verify);
        assert_eq!(empty.opening_realm(), "master");
        assert_eq!(f.opening_realm(), "hs-demo");
    }

    #[test]
    fn each_kind_resolves_to_its_grant() {
        let r = |f: IdmFields| match f.resolve("keycloak", &NoVault, &same) {
            Ok(ConnectConfig::Keycloak(k)) => Ok(k),
            Ok(other) => panic!("a keycloak profile resolved to {other:?}"),
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
        // CR-0099: no lookup unless it is on; on, the admin password goes
        // through admin-cli.
        assert!(
            r(fields("api-key", "", "hs-admin"))
                .unwrap()
                .version_lookup
                .is_none()
        );
        let mut on = fields("api-key", "", "hs-admin");
        on.lookup = true;
        on.lookup_credential = Credential {
            auth: "password".into(),
            user: "admin".into(),
            secret: "pw".into(),
            ..Default::default()
        };
        assert!(matches!(
            r(on.clone()).unwrap().version_lookup,
            Some(Grant::Password { ref client_id, ref username, .. }) if client_id == "admin-cli" && username == "admin"
        ));
        on.lookup_credential.user.clear();
        assert!(matches!(r(on), Err(ProfileError::Invalid(_))));
        let mut ca = fields("api-key", "", "hs-admin");
        ca.ca_file = "C:/certs/corp.pem".into();
        assert_eq!(
            r(ca).unwrap().tls.ca_file,
            Some(PathBuf::from("C:/certs/corp.pem"))
        );
    }

    /// FEAT-0117: a directory's profile keeps its own fields and resolves
    /// a bind DN, or an anonymous bind, into the LDAP provider's config.
    #[test]
    fn a_directory_profile_round_trips_and_binds_by_dn_or_anonymously() {
        let mut f = IdmFields::from_table("ldap", &toml::Table::new());
        assert_eq!(f.url, "ldap://127.0.0.1:1389");
        assert_eq!(
            f.credential.auth, "password",
            "a bind DN and password by default"
        );
        assert_eq!(f.opening_realm(), "", "no master: the server's first base");
        f.starttls = true;
        f.users_base = "ou=people,dc=x".into();
        f.group_filter = "(objectClass=groupOfNames)".into();
        f.page_size = 250;
        f.credential.user = "cn=admin,dc=x".into();
        f.credential.secret = "pw".into();
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(IdmFields::from_table("ldap", &t), f);
        let Ok(ConnectConfig::Ldap(l)) = f.resolve("ldap", &NoVault, &same) else {
            panic!("not an LDAP config");
        };
        assert_eq!(l.bind_dn, "cn=admin,dc=x");
        assert!(l.password.is_some() && l.starttls && l.page_size == 250);
        f.credential.auth = "none".into();
        let Ok(ConnectConfig::Ldap(anon)) = f.resolve("ldap", &NoVault, &same) else {
            panic!("not an LDAP config");
        };
        assert!(anon.bind_dn.is_empty() && anon.password.is_none());
        f.url = "https://x".into();
        assert!(matches!(
            f.resolve("ldap", &NoVault, &same),
            Err(ProfileError::Invalid(_))
        ));
    }
}
