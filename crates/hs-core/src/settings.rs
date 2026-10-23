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
    pub git: Git,
    pub containers: Containers,
}

/// Settings → Containers: the guards and limits that apply to every
/// container host (`docs/tools/containers.md` §10.7, ADR-026). The hosts
/// themselves are connection profiles, not settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Containers {
    /// Confirm before stopping, restarting or killing something.
    pub confirm_stop: bool,
    /// Require the name to be typed before a remove or a namespace
    /// delete - the same bar the other destructive actions use.
    pub confirm_remove_by_name: bool,
    /// A protected environment's hosts are read-only however the
    /// profile is marked. On, because a production cluster is not a
    /// place to discover a mis-click.
    pub protected_read_only: bool,
    /// Mask Kubernetes Secret values until revealed.
    pub mask_secret_values: bool,
    /// Offer `system prune` at all. Off, and never offered on a
    /// protected environment whatever this says.
    pub allow_prune: bool,
    /// How often a live pane re-reads its inventory.
    pub refresh_secs: u32,
    /// Stream `/events` and `/stats`. Worth turning off on a slow link,
    /// where the streams cost more than they tell.
    pub stream_stats: bool,
    pub page_size: u32,
    /// How much history a Logs tab opened from here starts with.
    pub log_tail_lines: u32,
    /// Always dry-run an apply and show the diff first. On: this is the
    /// guard that makes an assistant-written manifest safe to apply.
    pub apply_dry_run_first: bool,
    /// Where Save to Manifests writes, relative to the workspace when
    /// it is not absolute. Empty means `manifests/`.
    pub manifest_dir: String,
}

impl Default for Containers {
    fn default() -> Self {
        Self {
            confirm_stop: true,
            confirm_remove_by_name: true,
            protected_read_only: true,
            mask_secret_values: true,
            allow_prune: false,
            refresh_secs: 5,
            stream_stats: true,
            page_size: 500,
            log_tail_lines: 2000,
            apply_dry_run_first: true,
            manifest_dir: String::new(),
        }
    }
}

/// Settings → Editor → Git: how commits are made and what is protected
/// (docs/tools/git.md §5, §10).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Git {
    /// A remote whose URL, or a branch whose name, contains one of these
    /// is protected: a force push is refused there and a branch delete or
    /// a discard asks twice.
    pub protected_patterns: Vec<String>,
    /// `git pull --rebase` rather than a merge.
    pub pull_rebase: bool,
    /// Add a Signed-off-by trailer to every commit.
    pub signoff: bool,
    /// Stash untracked files too (`git stash -u`).
    pub stash_untracked: bool,
}

impl Default for Git {
    fn default() -> Self {
        Self {
            protected_patterns: vec!["prod".into(), "release".into()],
            pull_rebase: true,
            signoff: false,
            stash_untracked: true,
        }
    }
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
    /// Show the favourites bar under the address bar, as Chrome shows its
    /// bookmarks bar. Owner's requirement, 2026-10-18.
    pub favorites_bar: bool,
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
            favorites_bar: true,
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
    /// Maximum number of undo steps kept by tools that implement their own
    /// history (image/icon edits today). Clamped to 1..=100.
    pub undo_limit: u32,
    /// The lowest level the app's log records and the Logs dock shows:
    /// `error`, `warn`, `info`, `debug` or `trace`.
    pub log_level: String,
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
    /// Per-activity sidebar widths, keyed by tool id; an activity without
    /// an entry starts at `sidebar_width`. Each activity's sidebar lists
    /// different things, so one width rarely suits them all.
    #[serde(default)]
    pub sidebar_widths: std::collections::HashMap<String, f32>,
    pub dock_height: f32,
    pub ui_zoom: f32,
    /// Show each tool's name under its icon in the activity rail.
    pub rail_labels: bool,
    /// Each activity's icon in its own colour; off, the icons take the
    /// theme's text colour (the default). Owner's rule, 2026-10-23.
    #[serde(default)]
    pub rail_coloured: bool,
    /// Tool ids in the order the rail shows them; ids missing here follow
    /// in their default order. Home is always first and Settings last.
    /// Since 2026-10-23 only the seed for a workspace that has no `[rail]`
    /// of its own (`workspace::RailPrefs`); the rail reads the workspace.
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
            git: Git::default(),
            containers: Containers::default(),
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
    /// CLI tools the Terminal activity lists below its shells, in order.
    /// Owner's request, 2026-10-19.
    pub cli: Vec<CliProfile>,
}

/// One CLI tool that opens as a terminal: `mysql`, `psql`, `claude`,
/// `opencode` or anything the user adds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CliProfile {
    pub id: String,
    pub name: String,
    /// The executable, resolved through PATH/PATHEXT when it starts.
    pub command: String,
    /// Passed to the shell as a command line, so quoting stays the user's.
    pub args: String,
    /// Empty starts in the activity's default folder; `{{workspace}}` is the
    /// workspace folder.
    pub cwd: String,
    /// A sidebar icon key: `terminal`, `database`, `cloud` or `sparkle`.
    pub icon: String,
    pub enabled: bool,
}

impl Default for CliProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            command: String::new(),
            args: String::new(),
            cwd: String::new(),
            icon: "terminal".into(),
            enabled: true,
        }
    }
}

impl CliProfile {
    /// The line typed into the shell when the profile opens.
    pub fn command_line(&self) -> String {
        let args = self.args.trim();
        if args.is_empty() {
            self.command.trim().to_string()
        } else {
            format!("{} {}", self.command.trim(), args)
        }
    }
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
    /// How to start Maven for a project that uses it; empty = `mvn` from
    /// PATH. A full path is accepted (quoted when it has spaces).
    /// Owner's requirement, 2026-10-17.
    pub maven_command: String,
    /// How to start Gradle; empty = `gradle` from PATH.
    pub gradle_command: String,
    /// Prefer the project's own `mvnw`/`gradlew` over either of those: the
    /// wrapper pins the version a project expects, so it wins by default.
    pub prefer_wrapper: bool,
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
    /// The user's changes to the assistant's prompt buttons: their order,
    /// their names, what they ask, and which are hidden. Global rather
    /// than per workspace (owner's rule, 2026-10-23): a prompt is how
    /// someone likes to ask a question, not a property of a project.
    pub prompts: crate::prompts::Prompts,
}

impl Default for Ai {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            default_provider: String::new(),
            max_rounds: 4,
            prompts: crate::prompts::Prompts::default(),
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
    /// How a self-hosted image server is driven (image kinds only).
    pub image: crate::ai::ImageWorkflow,
    /// Whether this provider's model can be sent a picture (chat kinds
    /// only). Auto reads the model's name; see [`crate::ai::VisionSupport`].
    pub vision: crate::ai::VisionSupport,
}

impl AiProvider {
    /// Whether a chat request to this provider, using `model`, may carry a
    /// picture. `model` is the chat's own choice, which may differ from
    /// the provider's default.
    pub fn sends_images(&self, model: &str) -> bool {
        let model = if model.trim().is_empty() {
            self.model.as_str()
        } else {
            model
        };
        self.kind.accepts_images() && self.vision.allows(model)
    }
}

impl Ai {
    /// The provider a new chat uses: the default when it chats, else the
    /// first chat provider.
    pub fn chosen(&self) -> Option<&AiProvider> {
        self.providers
            .iter()
            .find(|p| p.id == self.default_provider && p.kind.chats())
            .or_else(|| self.providers.iter().find(|p| p.kind.chats()))
    }
    /// The providers an assistant may talk to.
    pub fn chat_providers(&self) -> Vec<AiProvider> {
        self.providers
            .iter()
            .filter(|p| p.kind.chats())
            .cloned()
            .collect()
    }
    /// The providers that make pictures (1min.AI and the self-hosted
    /// image servers).
    pub fn image_providers(&self) -> Vec<AiProvider> {
        self.providers
            .iter()
            .filter(|p| p.kind.generates_images())
            .cloned()
            .collect()
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
            image: self.image.clone(),
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
            maven_command: String::new(),
            gradle_command: String::new(),
            prefer_wrapper: true,
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
            cli: Vec::new(),
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
            undo_limit: 20,
            log_level: "info".into(),
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
            sidebar_widths: std::collections::HashMap::new(),
            dock_height: 220.0,
            ui_zoom: 1.0,
            rail_labels: true,
            rail_coloured: false,
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
    #[test]
    fn a_cli_profile_builds_its_command_line() {
        let mut profile = CliProfile {
            command: "mysql".into(),
            ..Default::default()
        };
        assert_eq!(profile.command_line(), "mysql");
        profile.args = " -u root -p shop ".into();
        assert_eq!(profile.command_line(), "mysql -u root -p shop");
        assert!(
            CliProfile::default().enabled,
            "a new profile is shown unless it is switched off"
        );
    }
}
