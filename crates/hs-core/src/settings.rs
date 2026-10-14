//! Typed settings with defaults, stored as `settings.toml` in the data folder.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage;

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub schema: u32,
    pub general: General,
    pub appearance: Appearance,
    pub updates: Updates,
    pub security: Security,
    pub terminal: Terminal,
    pub editor: Editor,
    pub ai: Ai,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct General {
    /// Name of the workspace to open at startup (folder name under workspaces/).
    pub startup_workspace: String,
    pub restore_tabs: bool,
    /// Fluent locale, e.g. "en"; empty = detect from the OS.
    pub language: String,
    pub show_fallback_notice: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Appearance {
    /// "dark" | "light" | "system"
    pub theme: String,
    pub ui_font_size: f32,
    /// Interface font id (`hs_ui::fonts`): "" = default, "system:<family>" for an installed font.
    pub ui_font: String,
    pub min_tab_width: f32,
    /// Minimum width of a file tab inside an editor project tab.
    pub min_file_tab_width: f32,
    pub tab_gap: f32,
    pub sidebar_width: f32,
    pub dock_height: f32,
    pub ui_zoom: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Updates {
    pub enabled: bool,
    /// "stable" | "beta"
    pub channel: String,
    /// "notify" | "install_on_exit"
    pub when_found: String,
    /// Override of the forge URL (mirror); empty = default.
    pub source: String,
    pub last_check: Option<String>,
    pub skipped_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Security {
    /// "vault" | "keyring"
    pub secret_store: String,
    pub relock_after_minutes: u32,
    pub clear_clipboard_after_seconds: u32,
    pub remember_master_password_on_this_pc: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            general: General::default(),
            appearance: Appearance::default(),
            updates: Updates::default(),
            security: Security::default(),
            terminal: Terminal::default(),
            editor: Editor::default(),
            ai: Ai::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Terminal {
    /// Shell profile id ("pwsh", "cmd", "git-bash", "wsl:Ubuntu", …); empty = first detected.
    pub default_profile: String,
    pub font_size: f32,
    /// Font id (`hs_ui::fonts`): a bundled id or "system:<family>".
    pub font_family: String,
    pub scrollback: usize,
    pub copy_on_select: bool,
    /// Right-click copies the selection or pastes the clipboard (PuTTY style).
    pub right_click_copy_paste: bool,
    /// Ctrl+click opens URLs and `path:line:col` locations.
    pub link_detection: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Editor {
    pub font_size: f32,
    /// Font id (`hs_ui::fonts`): a bundled id or "system:<family>".
    pub font_family: String,
    /// Right-click copies the selection or pastes the clipboard (PuTTY style).
    pub right_click_copy_paste: bool,
    pub trim_trailing_whitespace: bool,
    pub insert_final_newline: bool,
    /// Reload an unmodified buffer silently when the file changes on disk.
    pub auto_reload: bool,
}

/// The optional AI assistant (ADR-019): the services it may call. Nothing
/// is sent anywhere until the user adds a provider and asks something.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ai {
    pub providers: Vec<AiProvider>,
    /// `AiProvider::id` used for a new chat; empty = the first one.
    pub default_provider: String,
    /// How many rounds Cascade may run on its own before it stops and asks.
    pub max_rounds: u32,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            default_provider: String::new(),
            max_rounds: 4,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    pub kind: crate::ai::Provider,
    /// Empty = the kind's default endpoint.
    pub base_url: String,
    /// `secret:<ulid>` in the vault, or empty for a local server without one.
    pub api_key: String,
    /// The model a new chat starts with.
    pub model: String,
}

impl Ai {
    /// The provider a new chat uses.
    pub fn chosen(&self) -> Option<&AiProvider> {
        self.providers
            .iter()
            .find(|p| p.id == self.default_provider)
            .or(self.providers.first())
    }
}

impl AiProvider {
    pub fn config(&self, model: &str) -> crate::ai::AiConfig {
        crate::ai::AiConfig {
            provider: self.kind,
            base_url: self.base_url.clone(),
            model: if model.is_empty() {
                self.model.clone()
            } else {
                model.to_string()
            },
        }
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            font_size: 13.0,
            font_family: "jetbrains-mono-nf".into(),
            right_click_copy_paste: true,
            trim_trailing_whitespace: false,
            insert_final_newline: false,
            auto_reload: true,
        }
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self {
            default_profile: String::new(),
            font_size: 13.0,
            font_family: "jetbrains-mono-nf".into(),
            scrollback: 10_000,
            copy_on_select: false,
            right_click_copy_paste: true,
            link_detection: true,
        }
    }
}

impl Default for General {
    fn default() -> Self {
        Self {
            startup_workspace: "default".into(),
            restore_tabs: true,
            language: String::new(),
            show_fallback_notice: true,
        }
    }
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            ui_font_size: 14.0,
            ui_font: String::new(),
            min_tab_width: 140.0,
            min_file_tab_width: 160.0,
            tab_gap: 4.0,
            sidebar_width: 260.0,
            dock_height: 220.0,
            ui_zoom: 1.0,
        }
    }
}

impl Default for Updates {
    fn default() -> Self {
        Self {
            enabled: true,
            channel: "stable".into(),
            when_found: "notify".into(),
            source: String::new(),
            last_check: None,
            skipped_version: None,
        }
    }
}

impl Default for Security {
    fn default() -> Self {
        Self {
            secret_store: "vault".into(),
            relock_after_minutes: 15,
            clear_clipboard_after_seconds: 30,
            remember_master_password_on_this_pc: false,
        }
    }
}

impl Settings {
    /// Load, or return defaults when the file does not exist yet. A file from a
    /// newer build is an error the caller must show.
    pub fn load(path: &Path) -> storage::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let mut s: Settings = storage::read_toml_versioned(path, SCHEMA)?;
        s.schema = SCHEMA;
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> storage::Result<()> {
        storage::write_toml(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_and_missing_fields_fill_in() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.toml");
        let s = Settings::default();
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), s);
        std::fs::write(&p, "schema = 1\n[appearance]\nmin_tab_width = 200.0\n").unwrap();
        let s2 = Settings::load(&p).unwrap();
        assert_eq!(s2.appearance.min_tab_width, 200.0);
        assert_eq!(s2.appearance.min_file_tab_width, 160.0);
        assert_eq!(s2.appearance.tab_gap, 4.0);
        assert_eq!(s2.general.startup_workspace, "default");
    }
}
