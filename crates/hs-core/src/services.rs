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
    pub environments: Arc<RwLock<Environments>>,
    pub secrets: Arc<dyn SecretStore>,
    pub shortcuts: Arc<RwLock<Keymap>>,
    pub i18n: Arc<I18n>,
    pub workspace: Arc<RwLock<Workspace>>,
    pub version: VersionInfo,
    pub updater: Updater,
    /// SSH local forwards for any tool (`hs_ssh::tunnels::TunnelManager`).
    pub tunnels: Arc<dyn Tunnels>,
}

impl Services {
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
