//! Handing a saved credential to git for one command
//! (docs/tools/git-activity.md §2.2): the token comes out of the vault,
//! goes into a file under `HyperStackData/tmp/` that git's credential
//! helper reads, and the file is deleted when the command is done. The
//! token never reaches a command line, a URL or a config file.

use std::path::{Path, PathBuf};

use hs_core::services::Services;
use secrecy::ExposeSecret as _;

/// A credential resolved for use: the login and the token in the clear,
/// held only for the duration of one operation.
#[derive(Clone)]
pub struct Resolved {
    pub username: String,
    pub token: String,
    tmp: PathBuf,
}

/// The token of credential `id`, out of the vault. `None` when the
/// entry has no credential or no token (anonymous), `Err` when the
/// vault is locked or the reference is gone.
pub fn resolve(s: &Services, id: &str) -> Result<Option<Resolved>, String> {
    if id.is_empty() {
        return Ok(None);
    }
    let cred = s
        .git_credentials
        .read()
        .by_id(id)
        .cloned()
        .ok_or_else(|| s.t("git-cred-missing"))?;
    resolve_cred(s, &cred)
}

/// [`resolve`] for a credential already in hand.
pub fn resolve_cred(
    s: &Services,
    cred: &hs_core::git_hosts::GitCredential,
) -> Result<Option<Resolved>, String> {
    if cred.token.trim().is_empty() {
        return Ok(None);
    }
    let Some(r) = hs_core::secrets::SecretRef::parse(&cred.token) else {
        // Typed but never saved through the form: treat the text as the
        // token itself rather than refusing the operation.
        return Ok(Some(Resolved {
            username: cred.username.clone(),
            token: cred.token.clone(),
            tmp: s.paths.tmp(),
        }));
    };
    if s.secrets.is_locked() {
        return Err(s.t("settings-vault-unlock-first"));
    }
    let token = s.secrets.get(&r).map_err(|e| e.to_string())?;
    Ok(Some(Resolved {
        username: cred.username.clone(),
        token: token.expose_secret().to_string(),
        tmp: s.paths.tmp(),
    }))
}

impl Resolved {
    /// Write the helper file and return its path and git's environment.
    /// The caller runs its command, then calls [`Resolved::forget`].
    pub fn arm(&self) -> Result<(PathBuf, Vec<(String, String)>), String> {
        std::fs::create_dir_all(&self.tmp).map_err(|e| e.to_string())?;
        let file = self.tmp.join(format!("git-cred-{}", hs_core::new_id()));
        write_private(
            &file,
            &hs_git::write::credential_file_text(&self.username, &self.token),
        )?;
        let env = hs_git::write::credential_env(&file);
        Ok((file, env))
    }

    /// Delete the helper file. Best effort: a file that is already gone
    /// is the outcome wanted.
    pub fn forget(file: &Path) {
        let _ = std::fs::remove_file(file);
    }
}

/// Run `f` with git's credential environment armed, then clean up -
/// whatever `f` returns. With no credential, `f` gets an empty
/// environment.
pub fn with_env<T>(
    resolved: Option<&Resolved>,
    f: impl FnOnce(&[(String, String)]) -> T,
) -> Result<T, String> {
    match resolved {
        None => Ok(f(&[])),
        Some(r) => {
            let (file, env) = r.arm()?;
            let out = f(&env);
            Resolved::forget(&file);
            Ok(out)
        }
    }
}

#[cfg(unix)]
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    // The data folder is the user's own on Windows; the file lives for
    // one command and is deleted after.
    std::fs::write(path, text).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_helper_file_exists_only_while_armed() {
        let dir = tempfile::tempdir().unwrap();
        let r = Resolved {
            username: "owner".into(),
            token: "ghp_secret".into(),
            tmp: dir.path().to_path_buf(),
        };
        let seen = with_env(Some(&r), |env| {
            let file = env
                .iter()
                .find(|(k, _)| k == "GIT_CONFIG_VALUE_0")
                .map(|(_, v)| v.clone())
                .unwrap();
            assert!(file.contains("git-cred-"));
            let on_disk: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
            assert_eq!(on_disk.len(), 1);
            std::fs::read_to_string(on_disk[0].path()).unwrap()
        })
        .unwrap();
        assert_eq!(seen, "username=owner\npassword=ghp_secret\n");
        let after: Vec<_> = std::fs::read_dir(dir.path()).unwrap().flatten().collect();
        assert!(
            after.is_empty(),
            "the file is gone once the command is done"
        );
    }
}
