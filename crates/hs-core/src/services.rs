//! The shared handle every tool receives. Cheap to clone; each field is an
//! `Arc` or a `Clone` registry.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::connections::Connections;
use crate::environments::Environments;
use crate::events::EventBus;
use crate::i18n::I18n;
use crate::jobs::Jobs;
use crate::paths::DataDir;
use crate::runtime::AppRuntime;
use crate::secrets::SecretStore;
use crate::settings::Settings;
use crate::shortcuts::Keymap;
use crate::tunnels::Tunnels;
use crate::updater::Updater;
use crate::version::VersionInfo;
use crate::workspace::Workspace;

#[derive(Clone)]
pub struct Services {
    pub paths: Arc<DataDir>,
    pub settings: Arc<RwLock<Settings>>,
    pub runtime: AppRuntime,
    pub jobs: Jobs,
    pub events: EventBus,
    pub connections: Arc<RwLock<Connections>>,
    /// The workspace's saved project list, which the Editor activity's
    /// sidebar and Settings -> Editor both read and write.
    pub projects: Arc<RwLock<crate::projects::Projects>>,
    /// The workspace's saved repository list, which the Git activity's
    /// sidebar and Settings -> Git both read and write
    /// (docs/tools/git-activity.md §2.1).
    pub git_repos: Arc<RwLock<crate::git_repos::GitRepos>>,
    /// The application-wide Git hosts and credentials (§2.2).
    pub git_credentials: Arc<RwLock<crate::git_hosts::GitCredentials>>,
    pub environments: Arc<RwLock<Environments>>,
    pub secrets: Arc<dyn SecretStore>,
    pub shortcuts: Arc<RwLock<Keymap>>,
    pub i18n: Arc<I18n>,
    pub workspace: Arc<RwLock<Workspace>>,
    pub version: VersionInfo,
    pub updater: Updater,
    /// SSH local forwards for any tool (`hs_ssh::tunnels::TunnelManager`).
    pub tunnels: Arc<dyn Tunnels>,
    /// What every activity has open, published by the shell once a frame
    /// so a tool's home tab can list its own. Owner's requirement,
    /// 2026-10-17.
    pub open_tabs: crate::open_tabs::OpenTabs,
    /// The Problems inbox any tool may write to: editor diagnostics, SQL
    /// errors and log records that matched a reporting highlight rule
    /// all belong in one place (docs/tools/logs.md §15.2).
    pub problems: crate::problems::Problems,
    /// Who can answer "have you seen this token?". Each tool registers
    /// a searcher at start-up so the asking tool never depends on the
    /// answering one (ADR-004, docs/tools/logs.md §15.3).
    pub search: crate::search::SearchRegistry,
}

/// What a `{{name}}` resolves to right now (for the peek popups).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarPeek {
    Unset,
    Plain(String),
    /// A vault secret, resolved.
    Secret(String),
    /// A vault secret, but the vault is locked.
    Locked,
}

impl Services {
    /// Look a variable up through the active environment, its parents, the
    /// globals and the built-ins, resolving a `secret:` reference through
    /// the vault.
    pub fn peek_var(&self, name: &str) -> VarPeek {
        let Some(raw) = self.environments.read().lookup(name, None, &[]) else {
            return VarPeek::Unset;
        };
        match crate::secrets::SecretRef::parse(&raw) {
            None => VarPeek::Plain(raw),
            Some(r) => {
                if self.secrets.is_locked() {
                    return VarPeek::Locked;
                }
                match self.secrets.get(&r) {
                    Ok(v) => VarPeek::Secret(secrecy::ExposeSecret::expose_secret(&v).to_string()),
                    Err(_) => VarPeek::Unset,
                }
            }
        }
    }

    /// Substitute every `{{name}}` in `text` and resolve `secret:` references
    /// through the vault. Errors name what is missing or that the vault is
    /// locked, so a utility can refuse to run with half a key.
    pub fn expand(&self, text: &str) -> Result<String, String> {
        let out = match self.environments.read().substitute(text, None, &[]) {
            Ok(t) => t,
            Err(missing) => return Err(format!("unknown variable(s): {}", missing.join(", "))),
        };
        if !out.contains("secret:") {
            return Ok(out);
        }
        let mut result = String::with_capacity(out.len());
        let mut rest = out.as_str();
        while let Some(i) = rest.find("secret:") {
            result.push_str(&rest[..i]);
            let after = &rest[i..];
            let end = after[7..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map(|e| e + 7)
                .unwrap_or(after.len());
            let token = &after[..end];
            match crate::secrets::SecretRef::parse(token) {
                Some(r) => {
                    if self.secrets.is_locked() {
                        return Err(self.t("settings-vault-unlock-first"));
                    }
                    let v = self.secrets.get(&r).map_err(|e| e.to_string())?;
                    result.push_str(secrecy::ExposeSecret::expose_secret(&v));
                }
                None => result.push_str(token),
            }
            rest = &after[end..];
        }
        result.push_str(rest);
        Ok(result)
    }

    /// The credentials an AI provider needs, resolved through the vault.
    /// A field that holds no `secret:` reference is used as it is (a local
    /// server needs none); a locked vault is an error, not an empty key.
    pub fn ai_credentials(
        &self,
        p: &crate::settings::AiProvider,
    ) -> Result<crate::ai::Credentials, String> {
        let one = |field: &str| -> Result<String, String> {
            match crate::secrets::SecretRef::parse(field) {
                None => Ok(field.to_string()),
                Some(_) if self.secrets.is_locked() => Err(self.t("settings-vault-unlock-first")),
                Some(r) => self
                    .secrets
                    .get(&r)
                    .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
                    .map_err(|e| e.to_string()),
            }
        };
        Ok(crate::ai::Credentials {
            key: one(&p.api_key)?,
            secret: one(&p.secret_key)?,
        })
    }

    /// Translate a Fluent key with no arguments.
    pub fn t(&self, key: &str) -> String {
        self.i18n.t(key)
    }

    /// Persist settings and announce the change.
    pub fn save_settings(&self) -> crate::storage::Result<()> {
        let s = self.settings.read().clone();
        s.save(&self.paths.settings_file())?;
        self.events.publish(crate::events::Event::SettingsChanged);
        Ok(())
    }
}
