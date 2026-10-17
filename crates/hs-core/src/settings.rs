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
    pub accessibility: Accessibility,
    pub browser: Browser,
}

/// Settings → Browser: defaults for new pages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Browser {
    pub home_url: String,
    pub adblock: bool,
    pub javascript: bool,
    /// Force a dark rendering on every page by default.
    pub dark: bool,
    /// Extra hosts to refuse, one per entry, in addition to the bundled list.
    pub blocklist: Vec<String>,
    /// Inspect docks the inspector under the page instead of opening
    /// WebView2's own window. The web view then listens for the Chrome
    /// DevTools Protocol on a local port, which any process on this machine
    /// can drive, so this is off until it is asked for
    /// (docs/06-security.md).
    pub devtools_panel: bool,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            home_url: "https://duckduckgo.com".into(),
            adblock: true,
            javascript: true,
            dark: false,
            blocklist: Vec::new(),
            devtools_panel: false,
        }
    }
}

/// Settings → Accessibility. Read Aloud and reduced motion are off by
/// default; the assistants' microphone is on wherever an engine exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Accessibility {
    /// Show the Read Aloud button in the title bar.
    pub tts_enabled: bool,
    /// Speech rate, -10 (slowest) … 10 (fastest), 0 = the voice's default.
    pub tts_rate: i32,
    /// Skip toast slide and spinner animation.
    pub reduce_motion: bool,
    /// Show the microphone toggle in the AI assistants (speech to text
    /// through the operating system's dictation engine).
    pub stt_enabled: bool,
    /// Recogniser culture such as `en-US`; empty = the engine's default.
    pub stt_language: String,
    /// A listening session ends on its own after this many seconds
    /// (5–600), so a forgotten microphone does not stay open.
    pub stt_max_secs: u32,
}

impl Default for Accessibility {
    fn default() -> Self {
        Self {
            tts_enabled: false,
            tts_rate: 0,
            reduce_motion: false,
            stt_enabled: true,
            stt_language: String::new(),
            stt_max_secs: 60,
        }
    }
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
    /// "dark" | "light" | "system". Empty (settings written before the mode
    /// existed) means "whatever `theme` is", so an old `theme = "github-light"`
    /// keeps showing that light scheme.
    pub mode: String,
    /// The colour scheme used in dark mode (`hs_ui::theme::Theme::schemes`).
    pub theme: String,
    /// The colour scheme used in light mode.
    pub light_theme: String,
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
    /// Show each tool's name under its icon in the activity rail.
    pub rail_labels: bool,
    /// Tool ids in the order the rail shows them; ids missing here follow
    /// in their default order. Home is always first and Settings last.
    pub rail_order: Vec<String>,
    /// Tool ids kept out of the rail. A tool is never unreachable because
    /// of this: anything that opens one of its tabs (saving a connection
    /// for it, Ctrl+clicking a link, opening a remote file in the editor)
    /// puts its entry back.
    pub rail_hidden: Vec<String>,
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
            accessibility: Accessibility::default(),
            browser: Browser::default(),
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
    /// Shell profile ids in the order the sidebar lists them. Shells that
    /// are not named here follow, in the order they were detected, so a
    /// newly installed shell appears without the list being rewritten.
    pub shell_order: Vec<String>,
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
    /// Lay out a JSON file that arrives on one line (a source map, a
    /// minified payload) when it is opened.
    pub format_minified_on_open: bool,
    /// Start language servers for the languages that have one on `PATH`.
    pub lsp_enabled: bool,
    /// Open the completion list while typing (Ctrl+Space always works).
    pub completion_on_type: bool,
    /// Per-language changes to the built-in server table.
    pub language_servers: Vec<LanguageServer>,
    /// Per-adapter changes to the built-in debug adapter table.
    pub debug_adapters: Vec<DebugAdapter>,
    /// Shell profile id for the editor's own terminal ("pwsh", "cmd",
    /// "git-bash", "wsl:Ubuntu", …). Empty means PowerShell 7 where it is
    /// installed, else the Terminal tool's default, else the first shell
    /// detected. Owner's requirement, 2026-10-17.
    pub terminal_profile: String,
}

/// One debug adapter as the user set it (Settings → Editor → Debug
/// Adapters): an empty `command` keeps the built-in one, `args` are split
/// on whitespace (`{port}` stands for the port of a TCP adapter).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct DebugAdapter {
    pub id: String,
    pub command: String,
    pub args: String,
    pub enabled: bool,
}

/// One language's server as the user set it (Settings → Editor →
/// Language Servers): an empty `command` keeps the built-in one, `args`
/// are split on whitespace.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct LanguageServer {
    pub language: String,
    pub command: String,
    pub args: String,
    pub enabled: bool,
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
    /// AWS region (Bedrock only).
    pub region: String,
    /// Azure OpenAI API version (Azure only); empty uses the default.
    pub api_version: String,
    /// `secret:<ulid>` of the AWS secret access key (Bedrock only).
    pub secret_key: String,
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
            region: self.region.clone(),
            api_version: self.api_version.clone(),
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
            format_minified_on_open: true,
            lsp_enabled: true,
            completion_on_type: true,
            language_servers: Vec::new(),
            debug_adapters: Vec::new(),
            terminal_profile: String::new(),
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
            shell_order: Vec::new(),
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
            mode: "dark".into(),
            theme: "dark".into(),
            light_theme: "light".into(),
            ui_font_size: 14.0,
            ui_font: String::new(),
            min_tab_width: 140.0,
            min_file_tab_width: 160.0,
            tab_gap: 4.0,
            sidebar_width: 260.0,
            dock_height: 220.0,
            ui_zoom: 1.0,
            rail_labels: true,
            rail_order: Vec::new(),
            rail_hidden: Vec::new(),
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
