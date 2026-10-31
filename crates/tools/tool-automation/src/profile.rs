//! The `[automation]` table of a server's connection profile
//! (docs/tools/automation.md §5): where the server is, and its credential
//! through the shared `hs_core::service_auth` (ADR-028).

use std::time::Duration;

use hs_core::secrets::SecretStore;
use hs_core::service_auth::{Auth, AuthError, Credential, TokenAs};
use hs_core::service_http::Http;

/// The credential kinds a server kind can use (the spec's §5 table). A
/// kind that has no use for one does not offer it.
pub fn auth_kinds_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "github-actions" => &["none", "token"],
        "gitlab-ci" => &["none", "token", "api-key"],
        _ => &["none", "password", "token"],
    }
}

/// How the kind wants a token sent.
pub fn token_as(kind: &str) -> TokenAs {
    match kind {
        "jenkins" => TokenAs::Basic,
        "forgejo-actions" => TokenAs::TokenPrefix,
        _ => TokenAs::Bearer,
    }
}

/// The header an API key goes in when the form leaves it empty.
pub fn default_header(kind: &str) -> &'static str {
    match kind {
        "gitlab-ci" => "PRIVATE-TOKEN",
        _ => "",
    }
}

/// Where a server of `kind` is usually found, so a new profile opens on
/// something that would work.
pub fn default_url(kind: &str) -> &'static str {
    match kind {
        "jenkins" => "http://127.0.0.1:8080",
        "teamcity" => "http://127.0.0.1:8111",
        "github-actions" => "https://api.github.com",
        "gitlab-ci" => "https://gitlab.com",
        "forgejo-actions" => "http://127.0.0.1:3000",
        "awx" => "http://127.0.0.1:8043",
        _ => "",
    }
}

/// Whether a kind names a repository (`owner/name`) or a project.
pub fn wants_repo(kind: &str) -> bool {
    matches!(kind, "github-actions" | "forgejo-actions")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationFields {
    pub url: String,
    /// GitHub and Forgejo: `owner/name`.
    pub repo: String,
    /// GitLab: `group/name` or a numeric id.
    pub project: String,
    pub credential: Credential,
}

/// Everything a driver needs, resolved.
#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub kind: String,
    pub url: String,
    pub repo: String,
    pub project: String,
    pub auth: Auth,
    pub token_as: TokenAs,
    /// Writes are refused by the driver as well as the tab.
    pub read_only: bool,
    pub timeout: Duration,
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

impl ConnectConfig {
    /// The HTTP client for this server, rooted at `base`.
    pub fn http(&self, base: &str) -> Result<Http, String> {
        Http::new(base, self.auth.clone(), self.token_as, self.timeout)
    }
}

impl AutomationFields {
    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get("automation")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let url = s("url");
        Self {
            url: if url.is_empty() {
                default_url(kind).to_string()
            } else {
                url
            },
            repo: s("repo"),
            project: s("project"),
            credential: Credential::from_table(&t).allowed(auth_kinds_for(kind)),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        for (k, v) in [
            ("url", &self.url),
            ("repo", &self.repo),
            ("project", &self.project),
        ] {
            t.insert(k.into(), toml::Value::String(v.clone()));
        }
        self.credential.write_into(&mut t);
        fields.insert("automation".into(), toml::Value::Table(t));
    }

    /// The second line of a row: "Jenkins · ci.example.com", or the
    /// repository for the kinds that name one.
    pub fn label_for(&self, kind: &str) -> String {
        let place = match kind {
            "github-actions" | "forgejo-actions" if !self.repo.is_empty() => self.repo.clone(),
            "gitlab-ci" if !self.project.is_empty() => self.project.clone(),
            _ => short(&self.url).to_string(),
        };
        format!("{} · {place}", hs_core::connections::kind_label(kind))
    }

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
        let url = sub(&self.url);
        let repo = sub(&self.repo);
        let project = sub(&self.project);
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
        if wants_repo(kind) && repo.split('/').filter(|p| !p.is_empty()).count() != 2 {
            return Err(ProfileError::Invalid(
                "the repository is written owner/name".into(),
            ));
        }
        if kind == "gitlab-ci" && project.trim().is_empty() {
            return Err(ProfileError::Invalid(
                "the GitLab project is missing".into(),
            ));
        }
        let mut credential = self.credential.clone();
        if credential.auth == "api-key" && credential.header.trim().is_empty() {
            credential.header = default_header(kind).to_string();
        }
        let auth = credential.resolve(secrets, lookup).map_err(|e| match e {
            AuthError::MissingVariables(v) => ProfileError::MissingVariables(v),
            other => ProfileError::Auth(other),
        })?;
        Ok(ConnectConfig {
            kind: kind.to_string(),
            url,
            repo: repo.trim().trim_matches('/').to_string(),
            project: project.trim().to_string(),
            auth,
            token_as: token_as(kind),
            read_only,
            timeout: Duration::from_secs(15),
        })
    }
}

/// A URL without its scheme, for a row's second line.
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

    fn same(v: &str) -> Result<String, Vec<String>> {
        Ok(v.to_string())
    }

    #[test]
    fn a_new_profile_points_at_the_kinds_usual_address() {
        let f = AutomationFields::from_table("teamcity", &toml::Table::new());
        assert_eq!(f.url, "http://127.0.0.1:8111");
        assert_eq!(f.credential.auth, "none");
        assert_eq!(f.label_for("teamcity"), "TeamCity · 127.0.0.1:8111");
    }

    #[test]
    fn fields_round_trip_and_unusable_credentials_fall_back() {
        let f = AutomationFields {
            url: "https://gitlab.example.com".into(),
            project: "team/app".into(),
            credential: Credential {
                auth: "api-key".into(),
                secret: "glpat".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(AutomationFields::from_table("gitlab-ci", &t), f);
        // GitHub takes tokens only.
        assert_eq!(
            AutomationFields::from_table("github-actions", &t)
                .credential
                .auth,
            "none"
        );
    }

    #[test]
    fn a_repository_and_a_project_are_required_where_the_kind_needs_them() {
        let f = AutomationFields {
            url: "https://api.github.com".into(),
            repo: "just-a-name".into(),
            ..Default::default()
        };
        assert!(f.resolve("github-actions", false, &NoVault, &same).is_err());
        let g = AutomationFields {
            url: "https://gitlab.com".into(),
            ..Default::default()
        };
        assert!(g.resolve("gitlab-ci", false, &NoVault, &same).is_err());
    }

    #[test]
    fn an_api_key_without_a_header_takes_the_kinds_own() {
        let f = AutomationFields {
            url: "https://gitlab.com/".into(),
            project: "team/app".into(),
            credential: Credential {
                auth: "api-key".into(),
                secret: "k".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let c = f.resolve("gitlab-ci", false, &NoVault, &same).unwrap();
        assert_eq!(c.url, "https://gitlab.com");
        assert_eq!(
            c.auth,
            Auth::ApiKey {
                header: "PRIVATE-TOKEN".into(),
                key: "k".into()
            }
        );
        assert_eq!(c.token_as, TokenAs::Bearer);
        assert_eq!(token_as("jenkins"), TokenAs::Basic);
    }
}
