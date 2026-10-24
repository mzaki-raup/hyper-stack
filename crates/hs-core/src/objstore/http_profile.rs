//! The `[webdav]` and `[nexus]` tables of a connection profile
//! (owner's request, 2026-10-24).
//!
//! Both are HTTP stores reached with a user and a password, so they
//! share a shape - and deliberately **not** a table: the fields differ
//! enough (a DAV share is one URL, a Nexus server has repositories
//! under it) that one table would mean reading a key that means
//! nothing for half the profiles that carry it.
//!
//! The password is a `secret:<id>` like every other, so it is in the
//! vault and never in the TOML.

use crate::secrets::SecretStore;

use super::nexus::NexusConfig;
use super::profile::ProfileError;
use super::webdav::WebdavConfig;

/// A WebDAV share.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebdavFields {
    /// The collection to open at, e.g.
    /// `https://files.example.com/remote.php/dav/files/alice`.
    pub url: String,
    pub user: String,
    /// `secret:<id>`, or typed text until the form saves it.
    pub password: String,
    pub insecure_tls: bool,
}

/// A Nexus Repository server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NexusFields {
    /// The server root, not a repository: the tool lists them.
    pub url: String,
    pub user: String,
    pub password: String,
    /// Show only this repository. Empty lists everything the account
    /// can see, which is what most people want.
    pub repository: String,
    pub insecure_tls: bool,
}

fn read(fields: &toml::Table, table: &str) -> toml::Table {
    fields
        .get(table)
        .and_then(|v| v.as_table())
        .cloned()
        .unwrap_or_default()
}

fn text(t: &toml::Table, k: &str) -> String {
    t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string()
}

fn flag(t: &toml::Table, k: &str) -> bool {
    t.get(k).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Resolve a stored password: a vault reference, or text the form has
/// not saved yet.
fn reveal(value: &str, secrets: &dyn SecretStore) -> Result<String, ProfileError> {
    if value.is_empty() {
        return Ok(String::new());
    }
    match crate::secrets::SecretRef::parse(value) {
        Some(r) => {
            if secrets.is_locked() {
                return Err(ProfileError::VaultLocked);
            }
            secrets
                .get(&r)
                .map(|s| {
                    use secrecy::ExposeSecret;
                    s.expose_secret().to_string()
                })
                .map_err(|e| ProfileError::Secret(e.to_string()))
        }
        None => Ok(value.to_string()),
    }
}

impl WebdavFields {
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = read(fields, "webdav");
        Self {
            url: text(&t, "url"),
            user: text(&t, "user"),
            password: text(&t, "password"),
            insecure_tls: flag(&t, "insecure_tls"),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        t.insert("url".into(), toml::Value::String(self.url.clone()));
        t.insert("user".into(), toml::Value::String(self.user.clone()));
        t.insert(
            "password".into(),
            toml::Value::String(self.password.clone()),
        );
        t.insert(
            "insecure_tls".into(),
            toml::Value::Boolean(self.insecure_tls),
        );
        fields.insert("webdav".into(), toml::Value::Table(t));
    }

    /// Sidebar subtitle: the host and path, without the scheme, which
    /// is the part that identifies the share.
    pub fn label(&self) -> String {
        let bare = self
            .url
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.url);
        bare.trim_end_matches('/').to_string()
    }

    pub fn config(&self, secrets: &dyn SecretStore) -> Result<WebdavConfig, ProfileError> {
        Ok(WebdavConfig {
            base_url: self.url.clone(),
            user: self.user.clone(),
            password: reveal(&self.password, secrets)?,
            insecure_tls: self.insecure_tls,
        })
    }
}

impl NexusFields {
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = read(fields, "nexus");
        Self {
            url: text(&t, "url"),
            user: text(&t, "user"),
            password: text(&t, "password"),
            repository: text(&t, "repository"),
            insecure_tls: flag(&t, "insecure_tls"),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        t.insert("url".into(), toml::Value::String(self.url.clone()));
        t.insert("user".into(), toml::Value::String(self.user.clone()));
        t.insert(
            "password".into(),
            toml::Value::String(self.password.clone()),
        );
        t.insert(
            "repository".into(),
            toml::Value::String(self.repository.clone()),
        );
        t.insert(
            "insecure_tls".into(),
            toml::Value::Boolean(self.insecure_tls),
        );
        fields.insert("nexus".into(), toml::Value::Table(t));
    }

    pub fn label(&self) -> String {
        let bare = self
            .url
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.url)
            .trim_end_matches('/');
        if self.repository.is_empty() {
            bare.to_string()
        } else {
            format!("{bare} · {}", self.repository)
        }
    }

    pub fn config(&self, secrets: &dyn SecretStore) -> Result<NexusConfig, ProfileError> {
        Ok(NexusConfig {
            base_url: self.url.clone(),
            user: self.user.clone(),
            password: reveal(&self.password, secrets)?,
            repository: self.repository.clone(),
            insecure_tls: self.insecure_tls,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webdav_fields_round_trip_through_the_profile_table() {
        let f = WebdavFields {
            url: "https://files.example.com/remote.php/dav/files/alice".into(),
            user: "alice".into(),
            password: "secret:01J".into(),
            insecure_tls: true,
        };
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        assert_eq!(WebdavFields::from_table(&table), f);
        // Its own table, not the S3 one.
        assert!(table.contains_key("webdav") && !table.contains_key("s3"));
    }

    #[test]
    fn nexus_fields_round_trip_through_the_profile_table() {
        let f = NexusFields {
            url: "https://nexus.example.com".into(),
            user: "admin".into(),
            password: "secret:01J".into(),
            repository: "hs-raw".into(),
            insecure_tls: false,
        };
        let mut table = toml::Table::new();
        f.write_into(&mut table);
        assert_eq!(NexusFields::from_table(&table), f);
    }

    /// A profile that has never been saved reads as empty rather than
    /// panicking, which is what a form opening on a new connection does.
    #[test]
    fn an_empty_table_reads_as_defaults() {
        let empty = toml::Table::new();
        assert_eq!(WebdavFields::from_table(&empty), WebdavFields::default());
        assert_eq!(NexusFields::from_table(&empty), NexusFields::default());
    }

    /// The subtitle identifies the share without the scheme, which is
    /// noise in a narrow sidebar.
    #[test]
    fn the_subtitle_is_the_part_that_identifies_the_server() {
        let dav = WebdavFields {
            url: "https://files.example.com/dav/".into(),
            ..Default::default()
        };
        assert_eq!(dav.label(), "files.example.com/dav");

        let nexus = NexusFields {
            url: "https://nexus.example.com/".into(),
            ..Default::default()
        };
        assert_eq!(nexus.label(), "nexus.example.com");
        let one = NexusFields {
            repository: "hs-raw".into(),
            ..nexus.clone()
        };
        assert_eq!(one.label(), "nexus.example.com · hs-raw");
    }

    /// A typed password is used as it stands; a `secret:` reference
    /// needs the vault, and a locked one is an error rather than an
    /// empty password sent to the server.
    #[test]
    fn a_locked_vault_is_an_error_not_an_empty_password() {
        struct Locked;
        impl SecretStore for Locked {
            fn is_locked(&self) -> bool {
                true
            }
            fn exists(&self) -> bool {
                true
            }
            fn unlock(&self, _: &secrecy::SecretString) -> crate::secrets::Result<()> {
                Ok(())
            }
            fn lock(&self) {}
            fn get(
                &self,
                _: &crate::secrets::SecretRef,
            ) -> crate::secrets::Result<secrecy::SecretString> {
                unreachable!("locked")
            }
            fn set(
                &self,
                _: &crate::secrets::SecretRef,
                _: &str,
                _: &secrecy::SecretString,
            ) -> crate::secrets::Result<()> {
                Ok(())
            }
            fn delete(&self, _: &crate::secrets::SecretRef) -> crate::secrets::Result<()> {
                Ok(())
            }
            fn list(&self) -> crate::secrets::Result<Vec<(crate::secrets::SecretRef, String)>> {
                Ok(Vec::new())
            }
        }
        let f = WebdavFields {
            password: "secret:01JABC".into(),
            ..Default::default()
        };
        assert!(matches!(f.config(&Locked), Err(ProfileError::VaultLocked)));

        // Text the form has not saved yet goes through as it stands.
        let typed = WebdavFields {
            password: "hunter2".into(),
            ..Default::default()
        };
        assert_eq!(typed.config(&Locked).unwrap().password, "hunter2");
    }
}
