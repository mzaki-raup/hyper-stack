//! The `[s3]` table of a `kind = "s3"` connection profile
//! (docs/05-data-model-and-storage.md §2.2), the provider presets, and the
//! import of AWS CLI profiles from `~/.aws/credentials` and `~/.aws/config`.

use std::collections::BTreeMap;
use std::path::Path;

use secrecy::ExposeSecret;

use super::s3::S3Config;
use super::sigv4::Credentials;
use crate::secrets::{SecretRef, SecretStore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Fields {
    /// aws | minio | r2 | b2 | spaces | wasabi | other
    pub provider: String,
    /// Empty for AWS (derived from the region).
    pub endpoint: String,
    pub region: String,
    pub access_key: String,
    /// `secret:<id>` (or typed text until the form saves it to the vault).
    pub secret_key: String,
    pub session_token: String,
    pub path_style: bool,
    pub default_bucket: String,
}

impl Default for S3Fields {
    fn default() -> Self {
        Self {
            provider: "aws".into(),
            endpoint: String::new(),
            region: "us-east-1".into(),
            access_key: String::new(),
            secret_key: String::new(),
            session_token: String::new(),
            path_style: false,
            default_bucket: String::new(),
        }
    }
}

pub const PROVIDERS: &[&str] = &["aws", "minio", "r2", "b2", "spaces", "wasabi", "other"];

pub fn provider_label(p: &str) -> &str {
    match p {
        "aws" => "Amazon S3",
        "minio" => "MinIO",
        "r2" => "Cloudflare R2",
        "b2" => "Backblaze B2",
        "spaces" => "DigitalOcean Spaces",
        "wasabi" => "Wasabi",
        _ => "Other S3-Compatible",
    }
}

/// Endpoint template and path-style default for a provider; `{region}` and
/// `{account}` are placeholders the user completes.
pub fn provider_preset(p: &str) -> (&'static str, bool) {
    match p {
        "minio" => ("http://localhost:9000", true),
        "r2" => ("https://{account}.r2.cloudflarestorage.com", true),
        "b2" => ("https://s3.{region}.backblazeb2.com", false),
        "spaces" => ("https://{region}.digitaloceanspaces.com", false),
        "wasabi" => ("https://s3.{region}.wasabisys.com", false),
        _ => ("", false),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    VaultLocked,
    NoKeys,
    Secret(String),
}

impl S3Fields {
    pub fn from_table(fields: &toml::Table) -> Self {
        let t = fields
            .get("s3")
            .and_then(|v| v.as_table())
            .cloned()
            .unwrap_or_default();
        let s = |k: &str| t.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let d = Self::default();
        Self {
            provider: Some(s("provider"))
                .filter(|p| !p.is_empty())
                .unwrap_or(d.provider),
            endpoint: s("endpoint"),
            region: Some(s("region"))
                .filter(|r| !r.is_empty())
                .unwrap_or(d.region),
            access_key: s("access_key"),
            secret_key: s("secret_key"),
            session_token: s("session_token"),
            path_style: t
                .get("path_style")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            default_bucket: s("default_bucket"),
        }
    }

    pub fn write_into(&self, fields: &mut toml::Table) {
        let mut t = toml::Table::new();
        let mut put = |k: &str, v: &str| {
            t.insert(k.into(), toml::Value::String(v.to_string()));
        };
        put("provider", &self.provider);
        put("endpoint", &self.endpoint);
        put("region", &self.region);
        put("access_key", &self.access_key);
        put("secret_key", &self.secret_key);
        put("session_token", &self.session_token);
        put("default_bucket", &self.default_bucket);
        t.insert("path_style".into(), toml::Value::Boolean(self.path_style));
        fields.insert("s3".into(), toml::Value::Table(t));
    }

    /// Endpoint and region for the sidebar subtitle.
    pub fn label(&self) -> String {
        if self.endpoint.is_empty() {
            format!("{} · {}", provider_label(&self.provider), self.region)
        } else {
            self.endpoint.clone()
        }
    }

    /// Client configuration with the secrets fetched from the vault.
    pub fn config(&self, secrets: &dyn SecretStore) -> Result<S3Config, ProfileError> {
        let reveal = |v: &str| -> Result<Option<String>, ProfileError> {
            if v.is_empty() {
                return Ok(None);
            }
            match SecretRef::parse(v) {
                Some(r) => {
                    if secrets.is_locked() {
                        return Err(ProfileError::VaultLocked);
                    }
                    secrets
                        .get(&r)
                        .map(|s| Some(s.expose_secret().to_string()))
                        .map_err(|e| ProfileError::Secret(e.to_string()))
                }
                None => Ok(Some(v.to_string())),
            }
        };
        let secret_key = reveal(&self.secret_key)?.ok_or(ProfileError::NoKeys)?;
        if self.access_key.is_empty() {
            return Err(ProfileError::NoKeys);
        }
        Ok(S3Config {
            endpoint: self.endpoint.clone(),
            region: self.region.clone(),
            path_style: self.path_style,
            credentials: Credentials {
                access_key: self.access_key.clone(),
                secret_key,
                session_token: reveal(&self.session_token)?,
            },
        })
    }
}

/// A profile found in the AWS CLI files.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct AwsProfile {
    pub name: String,
    pub access_key: String,
    pub secret_key: String,
    pub session_token: String,
    pub region: String,
    pub endpoint: String,
}

impl std::fmt::Debug for AwsProfile {
    /// Never prints the keys.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsProfile")
            .field("name", &self.name)
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

/// INI sections → key/value maps (`#` and `;` comments, `[profile x]` in
/// `config` normalised to `x`).
fn parse_ini(text: &str, config_file: bool) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = name.trim();
            let name = if config_file {
                name.strip_prefix("profile ").unwrap_or(name).trim()
            } else {
                name
            };
            current = Some(name.to_string());
            out.entry(name.to_string()).or_default();
            continue;
        }
        if let (Some(sec), Some((k, v))) = (&current, line.split_once('=')) {
            out.entry(sec.clone())
                .or_default()
                .insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    out
}

/// Profiles with static keys from the two AWS CLI files (either may be
/// missing). Profiles without keys (SSO, role chains) are skipped.
pub fn parse_aws_files(credentials: &str, config: &str) -> Vec<AwsProfile> {
    let creds = parse_ini(credentials, false);
    let conf = parse_ini(config, true);
    let mut names: Vec<&String> = creds.keys().chain(conf.keys()).collect();
    names.sort();
    names.dedup();
    let mut out = Vec::new();
    for name in names {
        let get = |k: &str| {
            creds
                .get(name)
                .and_then(|m| m.get(k))
                .or_else(|| conf.get(name).and_then(|m| m.get(k)))
                .cloned()
                .unwrap_or_default()
        };
        let p = AwsProfile {
            name: name.clone(),
            access_key: get("aws_access_key_id"),
            secret_key: get("aws_secret_access_key"),
            session_token: get("aws_session_token"),
            region: get("region"),
            endpoint: get("endpoint_url"),
        };
        if !p.access_key.is_empty() && !p.secret_key.is_empty() {
            out.push(p);
        }
    }
    out
}

/// Read `~/.aws/credentials` and `~/.aws/config` (or `AWS_SHARED_CREDENTIALS_FILE`
/// / `AWS_CONFIG_FILE`).
pub fn load_aws_profiles() -> Vec<AwsProfile> {
    let home = dirs::home_dir().unwrap_or_default();
    let path = |var: &str, default: &str| {
        std::env::var_os(var)
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| home.join(".aws").join(default))
    };
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    parse_aws_files(
        &read(&path("AWS_SHARED_CREDENTIALS_FILE", "credentials")),
        &read(&path("AWS_CONFIG_FILE", "config")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_round_trip_through_the_profile_table() {
        let f = S3Fields {
            provider: "minio".into(),
            endpoint: "http://127.0.0.1:9000".into(),
            region: "eu-central-1".into(),
            access_key: "AK".into(),
            secret_key: "secret:01ABC".into(),
            session_token: String::new(),
            path_style: true,
            default_bucket: "dev".into(),
        };
        let mut t = toml::Table::new();
        f.write_into(&mut t);
        assert_eq!(S3Fields::from_table(&t), f);
        assert_eq!(
            S3Fields::from_table(&toml::Table::new()).region,
            "us-east-1"
        );
    }

    #[test]
    fn aws_files_merge_credentials_and_config() {
        let creds = "[default]\naws_access_key_id = AKIA1\naws_secret_access_key = s1\n\n# comment\n[shop]\naws_access_key_id=AKIA2\naws_secret_access_key=s2\naws_session_token=tok\n";
        let conf = "[default]\nregion = eu-west-1\n[profile shop]\nregion=us-west-2\nendpoint_url = http://localhost:9000\n[profile sso-only]\nsso_start_url = https://x\n";
        let p = parse_aws_files(creds, conf);
        assert_eq!(p.len(), 2, "the SSO profile has no keys");
        assert_eq!(p[0].name, "default");
        assert_eq!(p[0].region, "eu-west-1");
        assert_eq!(p[1].name, "shop");
        assert_eq!(p[1].session_token, "tok");
        assert_eq!(p[1].region, "us-west-2");
        assert_eq!(p[1].endpoint, "http://localhost:9000");
    }

    #[test]
    fn config_needs_keys() {
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
            fn get(&self, _: &SecretRef) -> crate::secrets::Result<secrecy::SecretString> {
                unreachable!()
            }
            fn set(
                &self,
                _: &SecretRef,
                _: &str,
                _: &secrecy::SecretString,
            ) -> crate::secrets::Result<()> {
                Ok(())
            }
            fn delete(&self, _: &SecretRef) -> crate::secrets::Result<()> {
                Ok(())
            }
            fn list(&self) -> crate::secrets::Result<Vec<(SecretRef, String)>> {
                Ok(Vec::new())
            }
        }
        let mut f = S3Fields::default();
        assert_eq!(f.config(&Locked).unwrap_err(), ProfileError::NoKeys);
        f.access_key = "AK".into();
        f.secret_key = "secret:01X".into();
        assert_eq!(f.config(&Locked).unwrap_err(), ProfileError::VaultLocked);
        f.secret_key = "plain".into();
        assert_eq!(f.config(&Locked).unwrap().credentials.secret_key, "plain");
    }
}
