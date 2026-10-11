//! Build the `Services` handle from a data folder.

use std::sync::Arc;

use hs_core::Services;
use hs_core::connections::Connections;
use hs_core::environments::Environments;
use hs_core::events::EventBus;
use hs_core::i18n::I18n;
use hs_core::jobs::Jobs;
use hs_core::paths::DataDir;
use hs_core::runtime::{AppRuntime, UiWaker};
use hs_core::secrets::{KdfParams, VaultStore};
use hs_core::settings::Settings;
use hs_core::shortcuts::Keymap;
use hs_core::updater::Updater;
use hs_core::version::VersionInfo;
use hs_core::workspace::{Registry, Workspace};
use parking_lot::RwLock;

pub struct Boot {
    pub services: Services,
    pub registry: Registry,
    pub settings_error: Option<String>,
}

pub fn services(paths: DataDir, waker: Arc<dyn UiWaker>, kdf: KdfParams) -> Result<Boot, String> {
    let (settings, settings_error) = match Settings::load(&paths.settings_file()) {
        Ok(s) => (s, None),
        Err(e) => (Settings::default(), Some(e.to_string())),
    };
    let i18n = Arc::new(I18n::load(&settings.general.language));
    let keymap = Keymap::load(&paths.shortcuts_file());
    let runtime = AppRuntime::new(waker).map_err(|e| e.to_string())?;
    let jobs = Jobs::new(runtime.clone());
    let mut registry = Registry::load(&paths);
    let name = if settings.general.startup_workspace.is_empty() {
        "default".to_string()
    } else {
        settings.general.startup_workspace.clone()
    };
    let workspace: Workspace = registry.open(&paths, &name).map_err(|e| e.to_string())?;
    let connections = Connections::load(&workspace.connections_dir());
    let mut environments = Environments::load(&workspace.environments_dir());
    environments.active = workspace.meta.default_environment.clone();
    let secrets = Arc::new(VaultStore::new(&paths.vault_file(), kdf));
    let paths = Arc::new(paths);
    let version = VersionInfo::current();
    let updater = Updater::new(runtime.clone(), paths.clone(), version.clone());
    let connections = Arc::new(RwLock::new(connections));
    let tunnels = Arc::new(hs_ssh::tunnels::TunnelManager::new(
        runtime.clone(),
        connections.clone(),
        secrets.clone(),
        paths.known_hosts_file(),
    ));

    let services = Services {
        paths,
        settings: Arc::new(RwLock::new(settings)),
        runtime,
        jobs,
        events: EventBus::new(),
        connections,
        environments: Arc::new(RwLock::new(environments)),
        secrets,
        shortcuts: Arc::new(RwLock::new(keymap)),
        i18n,
        workspace: Arc::new(RwLock::new(workspace)),
        version,
        updater,
        tunnels,
    };
    Ok(Boot {
        services,
        registry,
        settings_error,
    })
}

/// Switch the live services to another workspace (reloads connections and environments).
pub fn switch_workspace(
    services: &Services,
    registry: &mut Registry,
    name: &str,
) -> Result<(), String> {
    let ws = registry
        .open(&services.paths, name)
        .map_err(|e| e.to_string())?;
    // Forwards go through the old workspace's SSH profiles.
    for t in services.tunnels.list() {
        services.tunnels.stop(t.id);
    }
    *services.connections.write() = Connections::load(&ws.connections_dir());
    let mut envs = Environments::load(&ws.environments_dir());
    envs.active = ws.meta.default_environment.clone();
    *services.environments.write() = envs;
    *services.workspace.write() = ws;
    services
        .events
        .publish(hs_core::events::Event::WorkspaceSwitched {
            name: name.to_string(),
        });
    Ok(())
}
