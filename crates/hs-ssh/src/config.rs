//! From a `kind = "ssh"` connection profile to an [`SshConfig`]: the auth
//! method, with passwords and passphrases fetched from the vault. Shared by
//! the SSH tool and the tunnel manager so both read profiles the same way.

use std::path::PathBuf;

use hs_core::secrets::{SecretRef, SecretStore};
use secrecy::SecretString;

use crate::{Auth, SshConfig, SshFields};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    VaultLocked,
    /// Password auth without a stored password.
    NoPassword,
    /// The vault could not return the secret.
    Secret(String),
}

/// `secret:<id>` from the vault, a plain value as is (tolerated, never
/// written by the form), empty as `None`.
pub fn resolve_secret(
    secrets: &dyn SecretStore,
    value: &str,
) -> Result<Option<SecretString>, ConfigError> {
    if value.is_empty() {
        return Ok(None);
    }
    match SecretRef::parse(value) {
        Some(r) => {
            if secrets.is_locked() {
                return Err(ConfigError::VaultLocked);
            }
            secrets
                .get(&r)
                .map(Some)
                .map_err(|e| ConfigError::Secret(e.to_string()))
        }
        None => Ok(Some(SecretString::from(value.to_string()))),
    }
}

/// `~/…` against the user's home folder (USERPROFILE on Windows).
pub fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        return std::path::Path::new(&home)
            .join(rest)
            .to_string_lossy()
            .to_string();
    }
    p.to_string()
}

pub fn config_from_fields(
    f: &SshFields,
    secrets: &dyn SecretStore,
) -> Result<SshConfig, ConfigError> {
    let auth = match f.auth.as_str() {
        "key" => Auth::KeyFile {
            path: PathBuf::from(expand_home(&f.key_path)),
            passphrase: resolve_secret(secrets, &f.key_passphrase)?,
        },
        "agent" => Auth::Agent,
        _ => match resolve_secret(secrets, &f.password)? {
            Some(p) => Auth::Password(p),
            None => return Err(ConfigError::NoPassword),
        },
    };
    Ok(SshConfig {
        host: f.host.clone(),
        port: f.port,
        user: f.user.clone(),
        auth,
        keepalive_secs: f.keepalive_secs,
        connect_timeout_secs: f.connect_timeout_secs,
    })
}
