//! The `[bi]` table of a BI server's connection profile
//! (docs/tools/analytics.md §5.1), its credential through the shared
//! `hs_core::service_auth` (ADR-028), and its resolution into a
//! provider's config.

use std::path::PathBuf;
use std::time::Duration;

use hs_core::secrets::SecretStore;
use hs_core::service_auth::{Auth, AuthError, Credential};
use hs_core::service_http::Tls;

/// The credential kinds Metabase takes: an API key (Metabase 49 and later;
/// the way in when people sign in through single sign-on) or a username
/// and password.
pub const AUTH_KINDS: &[&str] = &["api-key", "password"];

/// The header Metabase reads an API key from.
pub const API_KEY_HEADER: &str = "x-api-key";

/// The request timeout when Settings gives none (§7).
pub const TIMEOUT: Duration = Duration::from_secs(60);

pub fn default_url(kind: &str) -> &'static str {
    match kind {
        "metabase" => "http://127.0.0.1:3030",
        _ => "",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyticsFields {
    /// The site root, no trailing path of the API.
    pub url: String,
    pub tls_verify: bool,
    /// A PEM file of extra roots (a company CA); empty for none.
    pub ca_file: String,
    pub credential: Credential,
}

impl Default for AnalyticsFields {
    fn default() -> Self {
        AnalyticsFields {
            url: String::new(),
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

/// A profile resolved for Metabase.
#[derive(Debug, Clone)]
pub struct MetabaseConfig {
    pub url: String,
    /// `Auth::ApiKey` (the key in its header) or `Auth::Password`.
    pub auth: Auth,
    pub tls: Tls,
    pub timeout: Duration,
}

impl AnalyticsFields {
    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get("bi")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let mut credential = Credential::from_table(&t);
        // `none` is no answer for a BI server: a profile that never chose
        // gets the default, an API key.
        if !AUTH_KINDS.contains(&credential.auth.as_str()) || !t.contains_key("auth") {
            credential.auth = AUTH_KINDS[0].into();
        }
        let url = s("url");
        AnalyticsFields {
            url: if url.is_empty() {
                default_url(kind).to_string()
            } else {
                url
            },
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
        for (k, v) in [("url", &self.url), ("ca_file", &self.ca_file)] {
            t.insert(k.into(), toml::Value::String(v.clone()));
        }
        t.insert("tls_verify".into(), toml::Value::Boolean(self.tls_verify));
        self.credential.write_into(&mut t);
        fields.insert("bi".into(), toml::Value::Table(t));
    }

    /// "Metabase · 127.0.0.1:3030", for a row's second line.
    pub fn label_for(&self, kind: &str) -> String {
        format!(
            "{} · {}",
            hs_core::connections::kind_label(kind),
            short(&self.url)
        )
    }

    /// Resolve variables and the vault secret into Metabase's config.
    pub fn resolve(
        &self,
        secrets: &dyn SecretStore,
        lookup: &dyn Fn(&str) -> Result<String, Vec<String>>,
    ) -> Result<MetabaseConfig, ProfileError> {
        let url = match lookup(self.url.trim()) {
            Ok(u) => u,
            Err(missing) => return Err(ProfileError::MissingVariables(missing)),
        };
        let url = url.trim().trim_end_matches('/').to_string();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(ProfileError::Invalid(
                "The server's address must start with http:// or https://.".into(),
            ));
        }
        let auth = self
            .credential
            .clone()
            .allowed(AUTH_KINDS)
            .resolve(secrets, lookup)
            .map_err(|e| match e {
                AuthError::MissingVariables(v) => ProfileError::MissingVariables(v),
                other => ProfileError::Auth(other),
            })?;
        let auth = match auth {
            Auth::ApiKey { header, key } => Auth::ApiKey {
                header: if header.trim().is_empty() {
                    API_KEY_HEADER.into()
                } else {
                    header
                },
                key,
            },
            Auth::None | Auth::Token { .. } => {
                return Err(ProfileError::Invalid(
                    "Choose how to sign in: an API key, or a username and password.".into(),
                ));
            }
            password => password,
        };
        Ok(MetabaseConfig {
            url,
            auth,
            tls: Tls {
                verify: self.tls_verify,
                ca_file: Some(self.ca_file.trim())
                    .filter(|p| !p.is_empty())
                    .map(PathBuf::from),
            },
            timeout: TIMEOUT,
        })
    }
}

/// The address without its scheme and trailing slash, for a row or a fact.
pub fn short(url: &str) -> &str {
    url.trim_end_matches('/')
        .trim_start_matches("https://")
        .trim_start_matches("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Plain values only: a test that reaches the vault is a mistake.
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
            unreachable!("these tests use plain values")
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

    fn no_vars(v: &str) -> Result<String, Vec<String>> {
        Ok(v.to_string())
    }

    #[test]
    fn a_profile_round_trips_and_defaults_to_an_api_key() {
        let mut table = toml::Table::new();
        let f = AnalyticsFields::from_table("metabase", &table);
        assert_eq!(f.url, "http://127.0.0.1:3030");
        assert_eq!(f.credential.auth, "api-key");
        assert!(f.tls_verify);

        let mut set = f.clone();
        set.url = "https://bi.example.com".into();
        set.tls_verify = false;
        set.credential = Credential {
            auth: "password".into(),
            user: "ada@example.com".into(),
            secret: "{{pw}}".into(),
            header: String::new(),
        };
        set.write_into(&mut table);
        assert_eq!(AnalyticsFields::from_table("metabase", &table), set);
    }

    #[test]
    fn the_credential_resolves_to_what_goes_on_the_wire() {
        let key = AnalyticsFields {
            url: "https://bi.example.com/".into(),
            credential: Credential {
                auth: "api-key".into(),
                secret: "mb_secret".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let cfg = key.resolve(&NoVault, &no_vars).expect("resolves");
        assert_eq!(cfg.url, "https://bi.example.com", "no trailing slash");
        assert_eq!(
            cfg.auth,
            Auth::ApiKey {
                header: "x-api-key".into(),
                key: "mb_secret".into()
            }
        );

        let pw = AnalyticsFields {
            credential: Credential {
                auth: "password".into(),
                user: "ada".into(),
                secret: "hunter2".into(),
                ..Default::default()
            },
            ..key.clone()
        };
        assert_eq!(
            pw.resolve(&NoVault, &no_vars).unwrap().auth,
            Auth::Password {
                user: "ada".into(),
                password: "hunter2".into()
            }
        );
    }

    #[test]
    fn a_bad_address_or_a_missing_variable_is_said_in_words() {
        let bad = AnalyticsFields {
            url: "bi.example.com".into(),
            ..Default::default()
        };
        assert!(matches!(
            bad.resolve(&NoVault, &no_vars),
            Err(ProfileError::Invalid(_))
        ));
        let missing = |_: &str| -> Result<String, Vec<String>> { Err(vec!["bi_url".into()]) };
        assert!(matches!(
            AnalyticsFields::default().resolve(&NoVault, &missing),
            Err(ProfileError::MissingVariables(v)) if v == ["bi_url"]
        ));
    }

    #[test]
    fn the_address_is_shown_without_its_scheme() {
        assert_eq!(short("https://bi.example.com/"), "bi.example.com");
        assert_eq!(short("http://127.0.0.1:3030"), "127.0.0.1:3030");
    }
}
