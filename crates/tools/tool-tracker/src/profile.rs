//! The `[tracker]` table of a tracker's connection profile
//! (docs/tools/tracker.md §5), its credential through the shared
//! `hs_core::service_auth` (ADR-028), and its resolution into a
//! [`ConnectConfig`].

use std::time::Duration;

use hs_core::secrets::SecretStore;
use hs_core::service_auth::{Auth, AuthError, Credential, TokenAs};
use hs_core::service_http::Http;

/// The credential kinds a tracker kind can use (the spec's §5 table).
pub fn auth_kinds_for(kind: &str) -> &'static [&'static str] {
    match kind {
        "jira" | "confluence" => &["none", "password", "token"],
        "redmine" => &["none", "password", "api-key"],
        "github-issues" => &["none", "token"],
        "gitlab-issues" => &["none", "token", "api-key"],
        _ => &["none", "password", "token"],
    }
}

/// How a token is sent. Atlassian Cloud takes an email and an API token
/// as basic; a Data Center personal access token, and the forges' tokens,
/// are bearer (Forgejo's has its own prefix).
pub fn token_as(kind: &str, user: &str) -> TokenAs {
    match kind {
        "jira" | "confluence" if !user.is_empty() => TokenAs::Basic,
        "forgejo-issues" => TokenAs::TokenPrefix,
        _ => TokenAs::Bearer,
    }
}

pub fn default_header(kind: &str) -> &'static str {
    match kind {
        "redmine" => "X-Redmine-API-Key",
        "gitlab-issues" => "PRIVATE-TOKEN",
        _ => "",
    }
}

pub fn default_url(kind: &str) -> &'static str {
    match kind {
        "jira" | "confluence" => "https://your-site.atlassian.net",
        "redmine" => "http://127.0.0.1:3010",
        "github-issues" => "https://api.github.com",
        "gitlab-issues" => "https://gitlab.com",
        "forgejo-issues" => "http://127.0.0.1:3000",
        _ => "",
    }
}

/// Whether a kind names a repository (`owner/name`).
pub fn wants_repo(kind: &str) -> bool {
    matches!(kind, "github-issues" | "forgejo-issues")
}

/// Whether a kind is Atlassian's, with a Cloud / Data Center choice.
pub fn is_atlassian(kind: &str) -> bool {
    matches!(kind, "jira" | "confluence")
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackerFields {
    pub url: String,
    /// GitHub and Forgejo: `owner/name`.
    pub repo: String,
    /// GitLab: the project; Jira: the default project key; Redmine: the
    /// default project identifier; Confluence: the default space key.
    pub project: String,
    /// Jira and Confluence: Cloud rather than Data Center.
    pub cloud: bool,
    /// Redmine: the site writes Textile rather than Markdown (Redmine
    /// 6's default is CommonMark; older sites often Textile).
    pub textile: bool,
    /// Saved queries, in the tracker's own language.
    pub queries: Vec<String>,
    pub credential: Credential,
}

#[derive(Debug, Clone)]
pub struct ConnectConfig {
    pub kind: String,
    pub url: String,
    pub repo: String,
    pub project: String,
    pub cloud: bool,
    pub textile: bool,
    pub auth: Auth,
    pub token_as: TokenAs,
    /// Writes are refused by the driver as well as the tab.
    pub read_only: bool,
    pub timeout: Duration,
}

impl ConnectConfig {
    /// The HTTP client for this tracker, rooted at `base`.
    pub fn http(&self, base: &str) -> Result<Http, String> {
        Http::new(base, self.auth.clone(), self.token_as, self.timeout)
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

impl TrackerFields {
    pub fn from_table(kind: &str, fields: &toml::Table) -> Self {
        let t = fields
            .get("tracker")
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
            cloud: t
                .get("cloud")
                .and_then(|v| v.as_bool())
                .unwrap_or(is_atlassian(kind)),
            textile: t.get("textile").and_then(|v| v.as_bool()).unwrap_or(false),
            queries: t
                .get("queries")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .filter_map(|q| q.as_str().map(String::from))
                .collect(),
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
        t.insert("cloud".into(), toml::Value::Boolean(self.cloud));
        t.insert("textile".into(), toml::Value::Boolean(self.textile));
        t.insert(
            "queries".into(),
            toml::Value::Array(
                self.queries
                    .iter()
                    .map(|q| toml::Value::String(q.clone()))
                    .collect(),
            ),
        );
        self.credential.write_into(&mut t);
        fields.insert("tracker".into(), toml::Value::Table(t));
    }

    /// "Jira · acme.atlassian.net", or the repository or project.
    pub fn label_for(&self, kind: &str) -> String {
        let place = match kind {
            "github-issues" | "forgejo-issues" if !self.repo.is_empty() => self.repo.clone(),
            "gitlab-issues" if !self.project.is_empty() => self.project.clone(),
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
        if kind == "gitlab-issues" && project.trim().is_empty() {
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
        let user = match &auth {
            Auth::Token { user, .. } => user.clone(),
            _ => String::new(),
        };
        Ok(ConnectConfig {
            kind: kind.to_string(),
            url,
            repo: repo.trim().trim_matches('/').to_string(),
            project: project.trim().to_string(),
            cloud: self.cloud,
            textile: self.textile,
            auth,
            token_as: token_as(kind, &user),
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

    #[test]
    fn fields_round_trip_and_atlassian_defaults_to_cloud() {
        let f = TrackerFields::from_table("jira", &toml::Table::new());
        assert!(f.cloud);
        assert!(!TrackerFields::from_table("redmine", &toml::Table::new()).cloud);
        let g = TrackerFields {
            url: "https://redmine.example.com".into(),
            project: "ops".into(),
            textile: true,
            queries: vec!["status:closed".into()],
            credential: Credential {
                auth: "api-key".into(),
                secret: "k".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut t = toml::Table::new();
        g.write_into(&mut t);
        assert_eq!(TrackerFields::from_table("redmine", &t), g);
    }

    #[test]
    fn an_atlassian_token_with_an_email_goes_as_basic_and_without_as_bearer() {
        let cloud = TrackerFields {
            url: "https://acme.atlassian.net".into(),
            credential: Credential {
                auth: "token".into(),
                user: "ada@example.com".into(),
                secret: "t".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            cloud
                .resolve("jira", false, &NoVault, &same)
                .unwrap()
                .token_as,
            TokenAs::Basic
        );
        let dc = TrackerFields {
            credential: Credential {
                user: String::new(),
                ..cloud.credential.clone()
            },
            ..cloud
        };
        assert_eq!(
            dc.resolve("jira", false, &NoVault, &same).unwrap().token_as,
            TokenAs::Bearer
        );
    }

    #[test]
    fn redmine_key_defaults_to_its_header() {
        let f = TrackerFields {
            url: "http://127.0.0.1:3010".into(),
            credential: Credential {
                auth: "api-key".into(),
                secret: "k".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            f.resolve("redmine", false, &NoVault, &same).unwrap().auth,
            Auth::ApiKey {
                header: "X-Redmine-API-Key".into(),
                key: "k".into()
            }
        );
    }
}
