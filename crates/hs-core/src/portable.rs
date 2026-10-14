//! Settings export and import (Settings → General → Import / Export). A
//! bundle is one TOML file holding `settings.toml` and `shortcuts.toml`. It
//! never contains secrets: the AI provider keys and every other secret are
//! `secret:<id>` references into the vault, which stays on the machine.
//! The shell asks for the master password before either direction runs.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::settings::Settings;
use crate::shortcuts::Keymap;
use crate::storage;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bundle {
    pub schema: u32,
    pub exported: String,
    pub app_version: String,
    pub settings: Settings,
    /// action → combo, as in `shortcuts.toml`.
    pub shortcuts: BTreeMap<String, String>,
}

/// What an import changed, for the toast.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub providers_updated: usize,
    pub providers_added: usize,
    pub shortcuts_set: usize,
}

pub fn export(settings: &Settings, keymap: &Keymap, app_version: &str) -> Bundle {
    let mut settings = settings.clone();
    // Keys travel as vault references only; make sure nothing else slipped in.
    for p in &mut settings.ai.providers {
        if crate::secrets::SecretRef::parse(&p.api_key).is_none() {
            p.api_key.clear();
        }
    }
    Bundle {
        schema: SCHEMA,
        exported: chrono::Utc::now().to_rfc3339(),
        app_version: app_version.to_string(),
        settings,
        shortcuts: keymap
            .all()
            .map(|(a, c)| (a.clone(), c.display()))
            .collect(),
    }
}

pub fn write(path: &Path, bundle: &Bundle) -> storage::Result<()> {
    storage::write_toml(path, bundle)
}

pub fn read(path: &Path) -> storage::Result<Bundle> {
    let b: Bundle = storage::read_toml(path)?;
    if b.schema > SCHEMA {
        return Err(storage::StorageError::Message(format!(
            "this bundle was written by a newer HyperStack ({})",
            b.app_version
        )));
    }
    Ok(b)
}

/// Apply a bundle: every settings section is overwritten by the bundle's,
/// except that AI providers merge by id (existing ones updated, new ones
/// appended) and shortcuts are set one by one, so bindings the bundle does
/// not mention survive.
pub fn import(bundle: &Bundle, settings: &mut Settings, keymap: &mut Keymap) -> ImportReport {
    let mut report = ImportReport::default();
    let mut merged = bundle.settings.clone();
    merged.schema = crate::settings::SCHEMA;
    // Providers: merge rather than replace.
    let mut providers = settings.ai.providers.clone();
    for p in &bundle.settings.ai.providers {
        match providers.iter_mut().find(|q| q.id == p.id) {
            Some(existing) => {
                let keep_key = if p.api_key.is_empty() {
                    existing.api_key.clone()
                } else {
                    p.api_key.clone()
                };
                *existing = p.clone();
                existing.api_key = keep_key;
                report.providers_updated += 1;
            }
            None => {
                providers.push(p.clone());
                report.providers_added += 1;
            }
        }
    }
    merged.ai.providers = providers;
    if merged.ai.default_provider.is_empty() {
        merged.ai.default_provider = settings.ai.default_provider.clone();
    }
    // Machine-specific values stay: the window and panels of this PC.
    merged.appearance.sidebar_width = settings.appearance.sidebar_width;
    merged.appearance.dock_height = settings.appearance.dock_height;
    merged.updates.last_check = settings.updates.last_check.clone();
    *settings = merged;
    for (action, combo) in &bundle.shortcuts {
        if keymap.set(action, combo) {
            report.shortcuts_set += 1;
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::AiProvider;

    #[test]
    fn export_import_round_trips_and_merges_providers() {
        let mut src = Settings::default();
        src.appearance.theme = "nord".into();
        src.ai.providers.push(AiProvider {
            id: "p1".into(),
            name: "Local".into(),
            api_key: "secret:01ABC".into(),
            ..Default::default()
        });
        src.ai.providers.push(AiProvider {
            id: "p2".into(),
            name: "Typed key must not export".into(),
            api_key: "sk-plain".into(),
            ..Default::default()
        });
        let mut km = Keymap::default();
        km.set("tab.new", "Ctrl+Shift+N");
        let bundle = export(&src, &km, "1.2.3");
        assert_eq!(bundle.settings.ai.providers[1].api_key, "");
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("bundle.toml");
        write(&p, &bundle).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back.settings.appearance.theme, "nord");

        let mut dst = Settings::default();
        dst.appearance.sidebar_width = 333.0;
        dst.ai.providers.push(AiProvider {
            id: "p1".into(),
            name: "Old name".into(),
            api_key: "secret:KEEP".into(),
            ..Default::default()
        });
        dst.ai.providers.push(AiProvider {
            id: "mine".into(),
            name: "Only here".into(),
            ..Default::default()
        });
        let mut km2 = Keymap::default();
        let report = import(&back, &mut dst, &mut km2);
        assert_eq!(dst.appearance.theme, "nord");
        assert_eq!(dst.appearance.sidebar_width, 333.0);
        assert_eq!(dst.ai.providers.len(), 3);
        assert_eq!(dst.ai.providers[0].name, "Local");
        assert_eq!(dst.ai.providers[0].api_key, "secret:01ABC");
        assert_eq!(dst.ai.providers[1].name, "Only here");
        assert_eq!(report.providers_updated, 1);
        assert_eq!(report.providers_added, 1);
        assert!(report.shortcuts_set >= 1);
        assert_eq!(km2.get("tab.new").unwrap().display(), "Ctrl+Shift+N");
    }
}
