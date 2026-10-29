//! Global **Git credentials**: the hosts a user can clone from and push
//! to, with the token to do it (docs/tools/git-activity.md §2.2).
//!
//! One set for the whole application, like [`crate::globals`]: a GitHub
//! token is the same token in every workspace. The file is
//! `HyperStackData/git-credentials.toml`; the **token never is** - it is
//! a `secret:<ulid>` reference into the vault, written by the Settings
//! form through `store_typed_secrets`, and this module never sees the
//! value.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

/// The host software, which decides the REST API shape **Fetch Project**
/// talks to and the default base URL.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostKind {
    #[default]
    GitHub,
    GitLab,
    Forgejo,
    Gitea,
    Bitbucket,
    Azure,
    /// Any other `https://` remote: no project listing, token for git only.
    Generic,
}

impl HostKind {
    pub fn all() -> [HostKind; 7] {
        [
            HostKind::GitHub,
            HostKind::GitLab,
            HostKind::Forgejo,
            HostKind::Gitea,
            HostKind::Bitbucket,
            HostKind::Azure,
            HostKind::Generic,
        ]
    }

    /// The display name - a product name, so it is not translated.
    pub fn name(self) -> &'static str {
        match self {
            HostKind::GitHub => "GitHub",
            HostKind::GitLab => "GitLab",
            HostKind::Forgejo => "Forgejo",
            HostKind::Gitea => "Gitea",
            HostKind::Bitbucket => "Bitbucket",
            HostKind::Azure => "Azure DevOps",
            HostKind::Generic => "Generic",
        }
    }

    /// The public instance's address, for the kinds that have one; a
    /// self-hosted Forgejo or Gitea has to be typed.
    pub fn default_url(self) -> &'static str {
        match self {
            HostKind::GitHub => "https://github.com",
            HostKind::GitLab => "https://gitlab.com",
            HostKind::Bitbucket => "https://bitbucket.org",
            HostKind::Azure => "https://dev.azure.com",
            HostKind::Forgejo | HostKind::Gitea | HostKind::Generic => "",
        }
    }

    /// Whether **Fetch Project** can list this host's projects.
    pub fn lists_projects(self) -> bool {
        !matches!(self, HostKind::Generic)
    }
}

/// One saved credential.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GitCredential {
    pub id: String,
    /// What the dropdowns show ("Work GitLab", "GitHub").
    pub name: String,
    pub kind: HostKind,
    /// `https://host[:port]`, no trailing slash.
    pub base_url: String,
    /// The login the token belongs to; what `GIT_ASKPASS` answers for
    /// "Username".
    pub username: String,
    /// A `secret:<ulid>` reference, or empty for anonymous.
    pub token: String,
}

impl Default for GitCredential {
    fn default() -> Self {
        Self {
            id: crate::new_id(),
            name: String::new(),
            kind: HostKind::GitHub,
            base_url: HostKind::GitHub.default_url().to_string(),
            username: String::new(),
            token: String::new(),
        }
    }
}

impl GitCredential {
    /// The name to show, falling back to the host so a row is never
    /// nameless.
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            if self.base_url.trim().is_empty() {
                self.kind.name().to_string()
            } else {
                self.base_url.clone()
            }
        } else {
            self.name.clone()
        }
    }

    /// `base_url` without a trailing slash or a stray path.
    pub fn host(&self) -> String {
        self.base_url.trim().trim_end_matches('/').to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
struct File {
    schema: u32,
    credential: Vec<GitCredential>,
}

impl Default for File {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            credential: Vec::new(),
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct GitCredentials {
    path: PathBuf,
    items: Vec<GitCredential>,
}

impl GitCredentials {
    pub fn load(path: &Path) -> Self {
        let mut items = match storage::read_toml::<File>(path) {
            Ok(f) if f.schema <= SCHEMA => f.credential,
            Ok(f) => {
                tracing::warn!(
                    "git-credentials.toml is schema {} and this build knows {SCHEMA}; ignoring it",
                    f.schema
                );
                Vec::new()
            }
            Err(_) => Vec::new(),
        };
        items.sort_by_key(|c| c.name.to_lowercase());
        Self {
            path: path.to_path_buf(),
            items,
        }
    }

    pub fn all(&self) -> &[GitCredential] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn by_id(&self, id: &str) -> Option<&GitCredential> {
        self.items.iter().find(|c| c.id == id)
    }

    pub fn upsert(&mut self, entry: GitCredential) -> storage::Result<()> {
        self.items.retain(|c| c.id != entry.id);
        self.items.push(entry);
        self.items.sort_by_key(|c| c.name.to_lowercase());
        self.save()
    }

    pub fn remove(&mut self, id: &str) -> storage::Result<()> {
        self.items.retain(|c| c.id != id);
        self.save()
    }

    fn save(&self) -> storage::Result<()> {
        storage::write_toml(
            &self.path,
            &File {
                schema: SCHEMA,
                credential: self.items.clone(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_round_trip_and_carry_only_a_reference() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("git-credentials.toml");
        let mut creds = GitCredentials::load(&file);
        let c = GitCredential {
            name: "Forge".into(),
            kind: HostKind::Forgejo,
            base_url: "https://git.example.test/".into(),
            username: "owner".into(),
            token: "secret:01J0000000000000000000TOKEN".into(),
            ..Default::default()
        };
        creds.upsert(c.clone()).unwrap();
        let again = GitCredentials::load(&file);
        let got = again.by_id(&c.id).unwrap();
        assert_eq!(got.kind, HostKind::Forgejo);
        assert_eq!(got.host(), "https://git.example.test");
        // The file carries the reference, which is the whole point.
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("secret:01J"));
        assert!(!text.contains("ghp_"));
    }

    #[test]
    fn kinds_have_names_and_sensible_defaults() {
        for k in HostKind::all() {
            assert!(!k.name().is_empty());
        }
        assert_eq!(HostKind::GitHub.default_url(), "https://github.com");
        assert_eq!(HostKind::Forgejo.default_url(), "");
        assert!(!HostKind::Generic.lists_projects());
    }
}
