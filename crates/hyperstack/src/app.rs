//! The application shell (docs/04 §1): top bar, tab strip, environment banner,
//! activity rail + sidebar, main area (tab space with splits), bottom dock,
//! status bar, palette, settings, toasts and the vault dialogs.

use hs_ui::tip::TipExt;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::Color32;
use hs_core::Services;
use hs_core::events::Event;
use hs_core::logging::Logging;
use hs_core::paths::DataDir;
use hs_core::runtime::EguiWaker;
use hs_core::secrets::KdfParams;
use hs_core::tool::{Action, OpenRequest, ToastLevel, ToolContext, ToolFactory};
use hs_core::workspace::Registry;
use hs_ui::dialog::{self, DialogResult, PasswordDialogState, PasswordLabels};
use hs_ui::env_banner::EnvBanner;
use hs_ui::palette::{self, PaletteItem, PaletteLabels, PaletteMode, PaletteState};
use hs_ui::tabs::{TabMenuAction, TabStrip, TabStripLabels};
use hs_ui::theme::Theme;
use hs_ui::toast::Toasts;
use secrecy::SecretString;

use crate::bootstrap;
use crate::dock::{self, DockState};
use crate::launcher::{self, LauncherState};
use crate::settings_ui::{self, SettingsEffect, SharedSettings};
use crate::tabspace::TabSpace;
use crate::tools::home::{HomeRequest, SharedHome};

/// The workspace create/delete modal.
#[derive(Default)]
enum WorkspaceDialog {
    #[default]
    None,
    New {
        name: String,
        /// Workspace to seed environments and connections from, by name.
        /// `None` starts empty.
        copy_from: Option<String>,
        /// Logo file the user picked, copied into the workspace on create.
        icon: Option<std::path::PathBuf>,
    },
    ConfirmDelete {
        name: String,
        delete_files: bool,
    },
}

/// An `egui` image for a logo file, read as bytes and decoded by the installed
/// PNG/SVG loaders. The cache key carries the file's size and mtime so replacing the
/// logo shows the new image instead of the cached old one.
/// Returns `None` when the file is missing or unreadable.
pub(crate) fn logo_image(path: &std::path::Path) -> Option<egui::Image<'static>> {
    let bytes = std::fs::read(path).ok()?;
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let content_hash = hasher.finish();
    let stamp = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let uri = format!(
        "{}?v={stamp}-{}-{content_hash:x}",
        path.display().to_string().replace('\\', "/"),
        bytes.len()
    );
    Some(egui::Image::from_bytes(uri, bytes))
}

/// The workspace logo image for a workspace folder, if it has one.
pub(crate) fn ws_logo_image(root: &std::path::Path) -> Option<egui::Image<'static>> {
    logo_image(&hs_core::workspace::workspace_icon_file(root)?)
}

/// The application icon, shown beside the name in the top bar.
fn app_logo() -> egui::Image<'static> {
    egui::Image::from_bytes(
        "bytes://hyperstack-logo.png",
        include_bytes!("../../../assets/icon/hyperstack-64.png").as_slice(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaultDialog {
    None,
    Create,
    Unlock,
    ChangeMaster,
}

/// Settings export/import waits for the master password first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortableDialog {
    None,
    Export,
    Import,
}

pub struct HyperStackApp {
    services: Services,
    registry: Registry,
    factories: Vec<Box<dyn ToolFactory>>,
    theme: Theme,
    tabs: TabSpace,
    /// The component whose tabs the strip shows (a factory id); the rail switches it.
    group: Option<String>,
    window_state: crate::window_state::WindowState,
    window_poll: Instant,
    launcher: LauncherState,
    palette: PaletteState,
    toasts: Toasts,
    settings_ui: SharedSettings,
    dock: DockState,
    dock_open: bool,
    sidebar_open: bool,
    status: String,
    logging: Logging,
    vault_dialog: VaultDialog,
    password: PasswordDialogState,
    confirm_close: Option<u64>,
    last_input: Instant,
    /// When a secret copied from Settings → Security leaves the clipboard.
    clear_clipboard_at: Option<Instant>,
    last_layout_save: Instant,
    /// Hash of the tab states last written, so tool inputs that change
    /// without touching the strip (a devtools field, an API draft) are
    /// persisted too (polled every few seconds).
    layout_fingerprint: u64,
    layout_dirty_since: Option<Instant>,
    pending_actions: Vec<Action>,
    fallback_notice_open: bool,
    first_frame_done: bool,
    confirm_revert: bool,
    workspace_dialog: WorkspaceDialog,
    update_badge: Option<String>,
    notifications_open: bool,
    notifications_opened_this_frame: bool,
    home: SharedHome,
    tts: hs_core::tts::Tts,
    portable: PortableDialog,
    /// Where the active tab's content was drawn this frame, for Snip.
    main_rect: egui::Rect,
    /// A URL to open in the browser at the end of the frame (Print…).
    pending_open_url: Option<String>,
    browser_hub: tool_browser::SharedBrowser,
    browser_host: crate::browser_host::BrowserHost,
    /// A thumbnail of the active tab's pane was asked for this frame, and
    /// which tab it is of. The picture arrives on the next frame, by which
    /// time the active tab could have changed. Owner's requirement,
    /// 2026-10-17.
    thumb_wanted: Option<u64>,
    /// The last picture of each tab, as an egui image URI.
    thumbs: std::collections::HashMap<u64, String>,
    /// When the last thumbnail was taken, so this costs a screenshot every
    /// few seconds rather than every frame.
    thumb_at: Option<f64>,
    /// A screenshot was requested; crop to the tab (`false`) or keep the window.
    snip: Option<bool>,
}

impl HyperStackApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: DataDir,
        logging: Logging,
        open: Vec<std::path::PathBuf>,
    ) -> Self {
        hs_ui::fonts::prefetch_system_fonts();
        hs_term::shells::prefetch();
        hs_ui::icons::install_fonts(&cc.egui_ctx);
        hs_ui::icons::install_image_loaders(&cc.egui_ctx);
        let waker = Arc::new(EguiWaker(cc.egui_ctx.clone()));
        let boot = match bootstrap::services(paths, waker, KdfParams::DEFAULT) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("bootstrap failed: {e}");
                panic!("HyperStack could not start: {e}");
            }
        };
        let services = boot.services;
        hs_ui::theme::sync_system(&cc.egui_ctx);
        let theme = Theme::from_settings(&services.settings.read().appearance);
        hs_ui::theme::install(&cc.egui_ctx, &services.settings.read().appearance);
        hs_ui::fonts::install(&cc.egui_ctx, &font_choice(&services));
        hs_ui::theme::apply_font_size(
            &cc.egui_ctx,
            services.settings.read().appearance.ui_font_size,
        );
        cc.egui_ctx
            .set_zoom_factor(services.settings.read().appearance.ui_zoom);

        let settings_hub = SharedSettings::default();
        let home_hub = SharedHome::default();
        let browser_hub = crate::browser_host::new_hub();
        let factories =
            crate::tools::factories(settings_hub.clone(), home_hub.clone(), browser_hub.clone());
        home_hub.lock().tools = factories.iter().map(|f| f.descriptor().clone()).collect();
        settings_hub.lock().state.rail_tools = factories
            .iter()
            .map(|f| {
                (
                    f.descriptor().id.to_string(),
                    services.t(f.descriptor().title_key),
                )
            })
            .collect();
        let mut app = Self {
            thumb_wanted: None,
            thumb_at: None,
            thumbs: std::collections::HashMap::new(),
            fallback_notice_open: services.paths.fallback_reason.is_some()
                && services.settings.read().general.show_fallback_notice,
            dock_open: false,
            sidebar_open: true,
            services,
            registry: boot.registry,
            factories,
            theme,
            tabs: TabSpace::new(),
            group: None,
            window_state: crate::window_state::WindowState::default(),
            window_poll: Instant::now(),
            launcher: LauncherState::default(),
            palette: PaletteState::default(),
            toasts: Toasts::default(),
            settings_ui: settings_hub,
            dock: DockState::default(),
            status: String::new(),
            logging,
            vault_dialog: VaultDialog::None,
            password: PasswordDialogState::default(),
            confirm_close: None,
            last_input: Instant::now(),
            clear_clipboard_at: None,
            last_layout_save: Instant::now(),
            layout_fingerprint: 0,
            layout_dirty_since: None,
            pending_actions: Vec::new(),
            first_frame_done: false,
            confirm_revert: false,
            workspace_dialog: WorkspaceDialog::None,
            update_badge: None,
            notifications_open: false,
            notifications_opened_this_frame: false,
            home: home_hub,
            tts: hs_core::tts::Tts::new(),
            portable: PortableDialog::None,
            main_rect: egui::Rect::NOTHING,
            pending_open_url: None,
            browser_hub,
            browser_host: crate::browser_host::BrowserHost::default(),
            snip: None,
        };
        if let Some(e) = boot.settings_error {
            app.toasts.push(ToastLevel::Warning, e);
        }
        app.status = app.services.t("status-ready");
        app.restore_layout();
        app.vault_dialog = if app.services.secrets.exists() {
            VaultDialog::Unlock
        } else {
            VaultDialog::Create
        };
        app.password.reset();
        app.maybe_auto_check_updates();
        // Files and folders from the command line open in the editor.
        for p in open {
            let uri = p.to_string_lossy().to_string();
            app.open_tool("editor", Some(OpenRequest::File { uri, line: None }));
        }
        app
    }

    /// Daily automatic check (docs/09 §5.2), unless disabled.
    fn maybe_auto_check_updates(&mut self) {
        let (enabled, last, channel, source, skipped) = {
            let st = self.services.settings.read();
            (
                st.updates.enabled,
                st.updates.last_check.clone(),
                st.updates.channel.clone(),
                st.updates.source.clone(),
                st.updates.skipped_version.clone(),
            )
        };
        if !enabled {
            return;
        }
        let due = match last
            .as_deref()
            .and_then(|l| chrono::DateTime::parse_from_rfc3339(l).ok())
        {
            Some(t) => {
                chrono::Utc::now().signed_duration_since(t.with_timezone(&chrono::Utc))
                    > chrono::Duration::hours(24)
            }
            None => true,
        };
        if due {
            self.services.updater.check(
                hs_core::update::Channel::from_setting(&channel),
                Some(source),
                skipped,
            );
        }
    }

    fn start_update_check(&mut self) {
        let (channel, source, skipped) = {
            let st = self.services.settings.read();
            (
                st.updates.channel.clone(),
                st.updates.source.clone(),
                st.updates.skipped_version.clone(),
            )
        };
        self.services.updater.check(
            hs_core::update::Channel::from_setting(&channel),
            Some(source),
            skipped,
        );
    }

    /// Reacts to status changes of the updater once per frame.
    fn poll_updater(&mut self, ctx: &egui::Context) {
        use hs_core::updater::UpdateStatus;
        match self.services.updater.status() {
            UpdateStatus::Checking => ctx.request_repaint_after(Duration::from_millis(300)),
            UpdateStatus::UpToDate { checked_at } => {
                let stale = self.services.settings.read().updates.last_check.as_deref()
                    != Some(checked_at.as_str());
                if stale {
                    self.services.settings.write().updates.last_check = Some(checked_at);
                    let _ = self.services.save_settings();
                }
                self.update_badge = None;
            }
            UpdateStatus::Available { candidate, .. } => {
                let v = candidate.version.to_string();
                if self.update_badge.as_deref() != Some(v.as_str()) {
                    self.update_badge = Some(v.clone());
                    self.toasts.push(
                        ToastLevel::Info,
                        self.services
                            .i18n
                            .t_args("update-toast-available", &[("version", &v)]),
                    );
                    self.services.settings.write().updates.last_check =
                        Some(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
                    let _ = self.services.save_settings();
                }
            }
            UpdateStatus::Downloading { .. } => {
                ctx.request_repaint_after(Duration::from_millis(250))
            }
            UpdateStatus::Ready { .. } => {
                let on_exit = self.services.settings.read().updates.when_found == "install_on_exit";
                if on_exit && ctx.input(|i| i.viewport().close_requested()) {
                    self.install_update(ctx);
                }
            }
            _ => {}
        }
    }

    fn install_update(&mut self, ctx: &egui::Context) {
        use hs_core::updater::UpdateStatus;
        if let UpdateStatus::Ready { version, path } = self.services.updater.status() {
            self.save_layout();
            match self.services.updater.install(&path, &version) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.toasts.push(ToastLevel::Error, e),
            }
        }
    }

    /// Index into `self.factories`, so callers can borrow the factory and
    /// `self.tabs` at the same time (disjoint fields).
    fn factory_index(&self, id: &str) -> Option<usize> {
        self.factories.iter().position(|f| f.descriptor().id == id)
    }

    fn restore_layout(&mut self) {
        let mut layout = self.services.workspace.read().load_layout();
        // Home and Settings are pinned app surfaces, not workspace tabs.
        // Remove them and the legacy Welcome tab from saved layouts before
        // restoring; they are re-created below.
        layout
            .tabs
            .retain(|t| !is_mandatory(&t.tool) && t.tool != "hello");
        if layout
            .active
            .is_some_and(|a| !layout.tabs.iter().any(|t| t.id == a))
        {
            layout.active = layout.tabs.first().map(|t| t.id);
        }
        if let Some(node) = layout.split.take() {
            fn collect(node: &hs_core::workspace::LayoutNode, out: &mut Vec<u64>) {
                match node {
                    hs_core::workspace::LayoutNode::Leaf { tab } => out.push(*tab),
                    hs_core::workspace::LayoutNode::Split { a, b, .. } => {
                        collect(a, out);
                        collect(b, out);
                    }
                }
            }
            let mut leaves = Vec::new();
            collect(&node, &mut leaves);
            if leaves
                .iter()
                .all(|id| layout.tabs.iter().any(|t| t.id == *id))
            {
                layout.split = Some(node);
            }
        }
        let restore_tabs = self.services.settings.read().general.restore_tabs;
        if restore_tabs {
            let map: HashMap<String, &dyn ToolFactory> = self
                .factories
                .iter()
                .map(|f| (f.descriptor().id.to_string(), f.as_ref()))
                .collect();
            self.tabs.restore(&layout, &map, &self.services);
        }
        let restored_active = self.tabs.active;
        self.sidebar_open = layout.sidebar_open || layout.tabs.is_empty();
        self.dock_open = layout.dock_open;
        if let Some(env) = layout.active_environment {
            self.services.environments.write().active = Some(env);
        }
        self.ensure_mandatory_tabs(restored_active);
        self.group = self
            .active_factory()
            .as_deref()
            .map(|f| group_of(f).to_string());
    }

    /// Home and Settings are present in every workspace, pinned, once. They
    /// are app surfaces, not part of the workspace's saved layout. Opening
    /// them must not steal focus from a restored work tab; on an empty
    /// layout Home is the landing tab.
    fn ensure_mandatory_tabs(&mut self, restored_active: Option<u64>) {
        for fid in MANDATORY {
            let ids: Vec<u64> = self
                .tabs
                .tabs
                .iter()
                .filter(|t| t.factory_id == *fid)
                .map(|t| t.id)
                .collect();
            for id in ids.iter().skip(1) {
                self.tabs.close(*id);
            }
            let id = ids.first().copied().or_else(|| {
                let i = self.factory_index(fid)?;
                Some(
                    self.tabs
                        .open(self.factories[i].as_ref(), &self.services, None),
                )
            });
            if let Some(id) = id {
                self.tabs.set_pinned(id, true);
            }
        }
        // Home sits first in the strip, Settings right after it.
        for (want, fid) in MANDATORY.iter().enumerate() {
            if let Some(at) = self.tabs.tabs.iter().position(|t| t.factory_id == *fid)
                && at != want
                && want < self.tabs.tabs.len()
            {
                self.tabs.reorder(at, want);
            }
        }
        let landing = restored_active.or_else(|| {
            self.tabs
                .tabs
                .iter()
                .find(|t| t.factory_id == "home")
                .map(|t| t.id)
        });
        if let Some(active) = landing {
            self.tabs.activate(active);
        }
    }

    /// Factory id of the active tab.
    fn active_factory(&self) -> Option<String> {
        self.tabs
            .active
            .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
            .map(|t| t.factory_id.clone())
    }

    /// Ids of the tabs in the current group, in strip order.
    fn group_tab_ids(&self) -> Vec<u64> {
        self.tabs
            .tabs
            .iter()
            .filter(|t| {
                self.group
                    .as_deref()
                    .is_none_or(|g| group_of(&t.factory_id) == g)
            })
            .map(|t| t.id)
            .collect()
    }

    /// Show one component's tabs: keep the active tab if it belongs there,
    /// else activate the group's most recent tab, else open a fresh one.
    fn select_group(&mut self, factory_id: &str) {
        let factory_id = group_of(factory_id);
        self.group = Some(factory_id.to_string());
        if self.active_factory().as_deref().map(group_of) == Some(factory_id) {
            return;
        }
        match self.group_tab_ids().last().copied() {
            Some(id) => self.tabs.activate(id),
            None => self.open_tool(factory_id, None),
        }
    }

    /// After a close or a restore the active tab may sit outside the group.
    fn ensure_active_in_group(&mut self) {
        let Some(g) = self.group.clone() else { return };
        if self.active_factory().as_deref().map(group_of) == Some(g.as_str()) {
            return;
        }
        match self.group_tab_ids().last().copied() {
            Some(id) => self.tabs.activate(id),
            None => self.tabs.active = None,
        }
    }

    /// Ctrl+PageUp/PageDown cycle within the group.
    fn cycle_tab(&mut self, step: i32) {
        let ids = self.group_tab_ids();
        if ids.is_empty() {
            return;
        }
        let cur = self
            .tabs
            .active
            .and_then(|a| ids.iter().position(|i| *i == a))
            .unwrap_or(0) as i32;
        let n = ids.len() as i32;
        let idx = ((cur + step) % n + n) % n;
        self.tabs.activate(ids[idx as usize]);
    }

    /// Window geometry and panel sizes, saved with the layout (debounced, and on close).
    fn save_window_state(&mut self, ctx: &egui::Context) {
        if let Some(st) =
            ctx.input(|i| crate::window_state::WindowState::from_viewport(i.viewport()))
            && st != self.window_state
        {
            st.save(&self.services.paths.root().join("window.toml"));
            self.window_state = st;
        }
        let _ = self.services.save_settings();
    }

    fn save_layout(&mut self) {
        let mut layout = self
            .tabs
            .to_layout(self.services.environments.read().active.clone());
        layout
            .tabs
            .retain(|t| !is_mandatory(&t.tool) && t.tool != "hello");
        if layout.active.is_some_and(|active| {
            self.tabs
                .tabs
                .iter()
                .any(|t| t.id == active && is_mandatory(&t.factory_id))
        }) {
            layout.active = layout.tabs.first().map(|t| t.id);
        }
        if layout.split.as_ref().is_some_and(|node| {
            fn has_only_saved_leaves(
                node: &hs_core::workspace::LayoutNode,
                tabs: &[hs_core::workspace::LayoutTab],
            ) -> bool {
                match node {
                    hs_core::workspace::LayoutNode::Leaf { tab } => {
                        tabs.iter().any(|t| t.id == *tab)
                    }
                    hs_core::workspace::LayoutNode::Split { a, b, .. } => {
                        has_only_saved_leaves(a, tabs) && has_only_saved_leaves(b, tabs)
                    }
                }
            }
            !has_only_saved_leaves(node, &layout.tabs)
        }) {
            layout.split = None;
        }
        layout.sidebar_open = self.sidebar_open;
        layout.dock_open = self.dock_open;
        self.layout_fingerprint = layout_fingerprint(&layout);
        if let Err(e) = self.services.workspace.read().save_layout(&layout) {
            tracing::warn!("layout not saved: {e}");
        }
        self.tabs.dirty = false;
        self.layout_dirty_since = None;
        self.last_layout_save = Instant::now();
    }

    /// Settings live in one tab: focus it if open, else open it; optionally jump to a page.
    fn open_settings(&mut self, page: Option<settings_ui::Page>) {
        self.group = Some("settings".into());
        if let Some(p) = page {
            self.settings_ui.lock().state.page = p;
        }
        if let Some(id) = self
            .tabs
            .tabs
            .iter()
            .find(|t| t.factory_id == "settings")
            .map(|t| t.id)
        {
            self.tabs.activate(id);
        } else {
            self.open_tool("settings", None);
        }
    }

    /// A Ctrl+clicked link from the dock terminal: URLs go to the browser,
    /// file locations (relative to the workspace folder) to the editor.
    fn open_terminal_link(&mut self, link: hs_term::links::LinkTarget, ctx: &egui::Context) {
        match link {
            hs_term::links::LinkTarget::Url(url) => {
                if ctx.input(|i| i.modifiers.shift) {
                    self.open_tool("browser", Some(OpenRequest::Url { url }));
                } else {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
            hs_term::links::LinkTarget::Path { path, line, .. } => {
                let p = std::path::PathBuf::from(&path);
                let p = if p.is_absolute() {
                    p
                } else {
                    self.services.workspace.read().root().join(p)
                };
                if p.is_file() {
                    self.open_tool(
                        "editor",
                        Some(OpenRequest::File {
                            uri: p.to_string_lossy().to_string(),
                            line,
                        }),
                    );
                } else {
                    let msg = self
                        .services
                        .i18n
                        .t_args("terminal-link-missing", &[("path", &path)]);
                    self.toasts.push(ToastLevel::Warning, msg);
                }
            }
        }
    }

    /// Put a tool's rail entry back when something opens one of its tabs.
    /// Hiding an entry is a tidying choice, not a ban: a link Ctrl+clicked
    /// in the editor, a remote file opened from SFTP or a connection saved
    /// for a tool all bring its icon back rather than sending the user to
    /// Settings to wonder where the tab went.
    pub(crate) fn reveal_rail(&mut self, factory_id: &str) {
        let id = group_of(factory_id);
        let mut settings = self.services.settings.write();
        let before = settings.appearance.rail_hidden.len();
        settings.appearance.rail_hidden.retain(|h| h != id);
        if settings.appearance.rail_hidden.len() == before {
            return;
        }
        drop(settings);
        let _ = self.services.save_settings();
    }

    /// A tab of this tool that is not bound to any connection, the active
    /// one for preference.
    fn unbound_tab(&self, factory_id: &str) -> Option<u64> {
        let active = self.tabs.active;
        let mut candidates = self
            .tabs
            .tabs
            .iter()
            .filter(|t| {
                t.factory_id == factory_id
                    && t.tool.current_connection().is_none()
                    // A dashboard holds no connection and never will: it
                    // is the thing that lists them, so binding one to it
                    // would replace the dashboard with a connection.
                    && !(t.home && t.tool.home_is_dashboard())
            })
            .map(|t| t.id);
        let first = candidates.next()?;
        if candidates.clone().any(|id| Some(id) == active) {
            return active;
        }
        if Some(first) == active {
            return Some(first);
        }
        Some(first)
    }

    fn open_tool(&mut self, factory_id: &str, request: Option<OpenRequest>) {
        self.open_tool_in(factory_id, request, true);
    }

    /// Open a tool for `request`; with `reuse`, a tab that already shows the
    /// item (or would take it, `Tool::uri_affinity`) gets it instead.
    fn open_tool_in(&mut self, factory_id: &str, request: Option<OpenRequest>, reuse: bool) {
        self.reveal_rail(factory_id);
        // Home and Settings are single tabs: a second request focuses them.
        if is_mandatory(factory_id)
            && let Some(id) = self
                .tabs
                .tabs
                .iter()
                .find(|t| t.factory_id == factory_id)
                .map(|t| t.id)
        {
            self.group = Some(group_of(factory_id).to_string());
            self.tabs.activate(id);
            return;
        }
        // A tool that is one tab per workspace (the browser) focuses the
        // tab it already has and hands it the request, rather than opening
        // a second one.
        if let Some(id) = self
            .tabs
            .tabs
            .iter()
            .find(|t| t.factory_id == factory_id && t.tool.single_tab())
            .map(|t| t.id)
        {
            self.focus_and_open(id, factory_id, request);
            return;
        }
        // Project roots are unique in the main window even when an editor
        // action would normally force a new tab. File tabs may be duplicated
        // deliberately; project roots may not.
        if factory_id == "editor"
            && let Some(OpenRequest::File { uri, .. }) = &request
            && std::path::Path::new(uri.strip_prefix("file://").unwrap_or(uri)).is_dir()
            && let Some(id) = self
                .tabs
                .tabs
                .iter()
                .filter(|t| t.factory_id == "editor")
                .find(|t| t.tool.uri_affinity(uri) == Some(0))
                .map(|t| t.id)
        {
            self.focus_and_open(id, factory_id, request);
            return;
        }
        // A web address goes to the browser tab that exists (as a new page).
        if let Some(OpenRequest::Url { .. }) = &request
            && let Some(id) = self
                .tabs
                .tabs
                .iter()
                .find(|t| t.factory_id == "browser")
                .map(|t| t.id)
        {
            self.focus_and_open(id, factory_id, request);
            return;
        }
        // One tab per file: a request for something a tab of this tool already
        // shows focuses that tab (and lets it jump to the requested line).
        if reuse && let Some(OpenRequest::File { uri, .. }) = &request {
            let want = normalize_uri(uri);
            let active = self.tabs.active;
            let existing = self
                .tabs
                .tabs
                .iter()
                .find(|t| {
                    t.factory_id == factory_id
                        && t.tool
                            .current_uri()
                            .is_some_and(|u| normalize_uri(&u) == want)
                })
                .map(|t| t.id)
                .or_else(|| {
                    // A tab that holds the item elsewhere (an editor with
                    // the file in another row, or its project): best rank
                    // wins, the active tab on a tie.
                    self.tabs
                        .tabs
                        .iter()
                        .filter(|t| t.factory_id == factory_id)
                        .filter(|t| !(t.home && t.tool.home_is_dashboard()))
                        .filter_map(|t| t.tool.uri_affinity(uri).map(|r| (r, t.id)))
                        .min_by_key(|(r, id)| (*r, Some(*id) != active))
                        .map(|(_, id)| id)
                });
            if let Some(id) = existing {
                self.focus_and_open(id, factory_id, request);
                return;
            }
        }
        // How many tabs a connection may have, and what to do once it has
        // them: SSH gives a host up to three sessions, every other tool one.
        if reuse && let Some(OpenRequest::Connection { id: conn }) = &request {
            let bound: Vec<u64> = self
                .tabs
                .tabs
                .iter()
                .filter(|t| {
                    t.factory_id == factory_id && t.tool.current_connection() == Some(conn.as_str())
                })
                .map(|t| t.id)
                .collect();
            if bound.len() >= tabs_per_connection(factory_id) {
                // At the limit: focus one it already has (the active tab
                // when that is one of them), and let `open` reconnect it.
                let active = self.tabs.active;
                let id = bound
                    .iter()
                    .copied()
                    .find(|id| Some(*id) == active)
                    .or_else(|| bound.last().copied());
                if let Some(id) = id {
                    self.focus_and_open(id, factory_id, request);
                    return;
                }
            }
            // Room for another, so bind a tab of this tool that holds no
            // connection yet rather than leaving it blank beside the new
            // one — Home opens exactly such a tab.
            if let Some(id) = self.unbound_tab(factory_id) {
                self.focus_and_open(id, factory_id, request);
                return;
            }
        }
        let Some(i) = self.factory_index(factory_id) else {
            self.toasts
                .push(ToastLevel::Error, format!("No tool '{factory_id}'"));
            return;
        };
        let id = self
            .tabs
            .open(self.factories[i].as_ref(), &self.services, None);
        self.group = Some(group_of(factory_id).to_string());
        if let Some(req) = request {
            let mut actions = Vec::new();
            let env = self.services.environments.read().active.clone();
            if let Some(t) = self.tabs.get_mut(id) {
                let mut cx = ToolContext {
                    services: &self.services,
                    tab_id: id,
                    env_id: env.as_deref(),
                    actions: &mut actions,
                    is_home: false,
                };
                t.tool.open(req, &mut cx);
            }
            self.pending_actions.extend(actions);
        }
    }

    /// Focus an existing tab and hand it the request (jump to a line,
    /// reconnect a dropped connection).
    fn focus_and_open(&mut self, id: u64, factory_id: &str, request: Option<OpenRequest>) {
        self.group = Some(group_of(factory_id).to_string());
        self.tabs.activate(id);
        let mut actions = Vec::new();
        let env = self.services.environments.read().active.clone();
        if let (Some(t), Some(req)) = (self.tabs.get_mut(id), request) {
            let mut cx = ToolContext {
                services: &self.services,
                tab_id: id,
                env_id: env.as_deref(),
                actions: &mut actions,
                is_home: false,
            };
            t.tool.open(req, &mut cx);
        }
        self.pending_actions.extend(actions);
    }

    fn open_launcher_tab(&mut self) {
        // The launcher is the "no active tab" state: clear the active tab and focus search.
        self.tabs.active = None;
        self.launcher.focus_search = true;
        self.launcher.query.clear();
    }

    fn close_tab(&mut self, id: u64, force: bool) {
        // Mandatory shell tabs and an activity's permanent home tab both
        // stay. Checking here covers Close Others and Close to the Right
        // as well, which close through this.
        if self
            .tabs
            .tabs
            .iter()
            .any(|t| t.id == id && (is_mandatory(&t.factory_id) || t.home))
        {
            return;
        }
        let needs_confirm = self
            .tabs
            .get_mut(id)
            .is_some_and(|t| t.tool.on_close() == hs_core::tool::CloseDecision::AskUnsavedChanges);
        if needs_confirm && !force {
            self.confirm_close = Some(id);
            return;
        }
        self.tabs.close(id);
        self.ensure_active_in_group();
    }

    fn reopen_closed(&mut self, index: Option<usize>) {
        let idx = match index {
            Some(i) if i < self.tabs.closed.len() => i,
            Some(_) => return,
            None => {
                if self.tabs.closed.is_empty() {
                    return;
                }
                self.tabs.closed.len() - 1
            }
        };
        let closed = self.tabs.closed.remove(idx);
        if let Some(i) = self.factory_index(&closed.factory_id) {
            self.tabs.open(
                self.factories[i].as_ref(),
                &self.services,
                Some(closed.state),
            );
        }
    }

    fn run_action(&mut self, action: &str) {
        match action {
            "tab.new" => self.open_launcher_tab(),
            "tab.close" => {
                if let Some(a) = self.tabs.active {
                    self.close_tab(a, false);
                }
            }
            "tab.reopen" => self.reopen_closed(None),
            "tab.next" => self.cycle_tab(1),
            "tab.prev" => self.cycle_tab(-1),
            "tab.split_right" | "tab.split_down" => {
                if let Some(a) = self.tabs.active {
                    let horizontal = action == "tab.split_right";
                    if let Some(i) = self.factory_index("scratchpad") {
                        let new_id =
                            self.tabs
                                .open(self.factories[i].as_ref(), &self.services, None);
                        self.tabs.split_with(a, new_id, horizontal);
                    }
                }
            }
            "palette.commands" => self.palette.toggle(PaletteMode::Commands),
            "palette.quick_open" => self.palette.toggle(PaletteMode::QuickOpen),
            "view.sidebar" => self.sidebar_open = !self.sidebar_open,
            "view.dock" => self.dock_open = !self.dock_open,
            "view.terminal" => {
                self.dock_open = true;
                self.dock.tab = dock::DockTab::Terminal;
            }
            "terminal.new" => self.open_tool("terminal", None),
            "app.settings" => self.open_settings(None),
            "home.show" => self.select_group("home"),
            "tts.stop" => self.tts.stop(),
            "snip.tab" | "snip.window" => self.request_snip(action == "snip.window"),
            "print.tab" => self.print_active(),
            "settings:connections" => self.open_settings(Some(settings_ui::Page::Connections)),
            "ws.new" => {
                self.workspace_dialog = WorkspaceDialog::New {
                    name: String::new(),
                    copy_from: None,
                    icon: None,
                }
            }
            "app.shortcuts" => self.open_settings(Some(settings_ui::Page::Shortcuts)),
            "vault.lock" => {
                self.services.secrets.lock();
                self.services.events.publish(Event::VaultLocked);
                self.toasts
                    .push(ToastLevel::Info, self.services.t("vault-locked-toast"));
            }
            "vault.unlock" => {
                self.vault_dialog = if self.services.secrets.exists() {
                    VaultDialog::Unlock
                } else {
                    VaultDialog::Create
                };
                self.password.reset();
            }
            "layout.save" => {
                self.save_layout();
                self.toasts
                    .push(ToastLevel::Success, self.services.t("toast-layout-saved"));
            }
            "zoom.in" | "zoom.out" => {
                let mut s = self.services.settings.write();
                let z = s.appearance.ui_zoom + if action == "zoom.in" { 0.1 } else { -0.1 };
                s.appearance.ui_zoom = z.clamp(0.75, 2.0);
            }
            "env.switch" => self.palette.open(PaletteMode::Commands),
            other => {
                if let Some(env) = other.strip_prefix("env:") {
                    self.services.environments.write().active = if env.is_empty() {
                        None
                    } else {
                        Some(env.to_string())
                    };
                    self.services.events.publish(Event::EnvironmentSwitched {
                        env_id: self.services.environments.read().active.clone(),
                    });
                    self.tabs.dirty = true;
                } else if let Some(ws) = other.strip_prefix("ws.delete:") {
                    self.workspace_dialog = WorkspaceDialog::ConfirmDelete {
                        name: ws.to_string(),
                        delete_files: false,
                    };
                } else if let Some(ws) = other.strip_prefix("ws:") {
                    self.save_layout();
                    match bootstrap::switch_workspace(&self.services, &mut self.registry, ws) {
                        Ok(()) => {
                            self.tabs = TabSpace::new();
                            self.restore_layout();
                        }
                        Err(e) => self.toasts.push(ToastLevel::Error, e),
                    }
                } else if let Some(tool) = other.strip_prefix("open:") {
                    self.open_tool(tool, None);
                } else if let Some(rest) = other.strip_prefix("qo:")
                    && let Some((tab, item)) = rest.split_once(':')
                    && let Ok(tab) = tab.parse::<u64>()
                {
                    // A quick-open item: hand it back to the tab that offered it.
                    let env = self.services.environments.read().active.clone();
                    let mut actions = Vec::new();
                    if let Some(t) = self.tabs.get_mut(tab) {
                        let mut cx = ToolContext {
                            services: &self.services,
                            tab_id: tab,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                            is_home: false,
                        };
                        t.tool.open(
                            OpenRequest::File {
                                uri: item.to_string(),
                                line: None,
                            },
                            &mut cx,
                        );
                        self.tabs.activate(tab);
                    }
                    self.pending_actions.extend(actions);
                } else if let Some(a) = self.tabs.active {
                    // "tool:<cmd>" from the palette, or a bare action id from a
                    // shortcut (file.save, file.open, …): the active tool decides.
                    let cmd = other.strip_prefix("tool:").unwrap_or(other);
                    let env = self.services.environments.read().active.clone();
                    let mut actions = Vec::new();
                    if let Some(t) = self.tabs.get_mut(a) {
                        let mut cx = ToolContext {
                            services: &self.services,
                            tab_id: a,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                            is_home: false,
                        };
                        if !t.tool.run_command(cmd, &mut cx) && cmd == "file.new" {
                            cx.open("editor", None);
                        }
                    }
                    self.pending_actions.extend(actions);
                }
            }
        }
    }

    fn palette_items(&self) -> Vec<PaletteItem> {
        let s = &self.services;
        let km = s.shortcuts.read();
        let sc = |a: &str| km.get(a).map(|c| c.display());
        let mut items = Vec::new();
        match self.palette.mode {
            PaletteMode::Commands => {
                let shell = [
                    ("tab.new", "cmd-new-tab"),
                    ("tab.close", "cmd-close-tab"),
                    ("tab.reopen", "cmd-reopen-tab"),
                    ("tab.next", "cmd-next-tab"),
                    ("tab.prev", "cmd-prev-tab"),
                    ("tab.split_right", "cmd-split-right"),
                    ("tab.split_down", "cmd-split-down"),
                    ("view.sidebar", "cmd-toggle-sidebar"),
                    ("view.dock", "cmd-toggle-dock"),
                    ("app.settings", "cmd-settings"),
                    ("app.shortcuts", "cmd-shortcuts"),
                    ("layout.save", "cmd-save-layout"),
                    ("zoom.in", "cmd-zoom-in"),
                    ("zoom.out", "cmd-zoom-out"),
                ];
                for (id, key) in shell {
                    items.push(PaletteItem {
                        id: id.into(),
                        title: s.t(key),
                        subtitle: None,
                        icon: None,
                        shortcut: sc(id),
                    });
                }
                let vault_cmd = if s.secrets.is_locked() {
                    ("vault.unlock", "cmd-unlock-vault")
                } else {
                    ("vault.lock", "cmd-lock-vault")
                };
                items.push(PaletteItem {
                    id: vault_cmd.0.into(),
                    title: s.t(vault_cmd.1),
                    subtitle: None,
                    icon: Some(hs_ui::icons::LOCK.into()),
                    shortcut: sc("vault.lock"),
                });
                for f in &self.factories {
                    let d = f.descriptor();
                    items.push(PaletteItem {
                        id: format!("open:{}", d.id),
                        title: s
                            .i18n
                            .t_args("cmd-open-tool", &[("tool", &s.t(d.title_key))]),
                        subtitle: Some(s.t(d.category.label_key())),
                        icon: Some(d.icon.into()),
                        shortcut: None,
                    });
                }
                if let Some(a) = self.tabs.active
                    && let Some(t) = self.tabs.tabs.iter().find(|t| t.id == a)
                {
                    for c in t.tool.commands() {
                        items.push(PaletteItem {
                            id: format!("tool:{}", c.id),
                            title: c.title,
                            subtitle: Some(t.tool.title().to_string()),
                            icon: Some(t.tool.descriptor().icon.into()),
                            shortcut: c.shortcut_action.as_deref().and_then(sc),
                        });
                    }
                }
                items.push(PaletteItem {
                    id: "env:".into(),
                    title: s
                        .i18n
                        .t_args("cmd-switch-env", &[("name", &s.t("env-none"))]),
                    subtitle: None,
                    icon: Some(hs_ui::icons::ENVIRONMENT.into()),
                    shortcut: None,
                });
                for e in s.environments.read().all() {
                    items.push(PaletteItem {
                        id: format!("env:{}", e.id),
                        title: s.i18n.t_args("cmd-switch-env", &[("name", &e.name)]),
                        subtitle: e.protected.then(|| s.t("env-protected")),
                        icon: Some(hs_ui::icons::ENVIRONMENT.into()),
                        shortcut: None,
                    });
                }
                for w in &self.registry.workspaces {
                    items.push(PaletteItem {
                        id: format!("ws:{}", w.name),
                        title: s.i18n.t_args("cmd-switch-workspace", &[("name", &w.name)]),
                        subtitle: Some(w.path.clone()),
                        icon: Some(hs_ui::icons::WORKSPACE.into()),
                        shortcut: None,
                    });
                }
            }
            PaletteMode::QuickOpen => {
                for f in &self.factories {
                    let d = f.descriptor();
                    items.push(PaletteItem {
                        id: format!("open:{}", d.id),
                        title: s.t(d.title_key),
                        subtitle: Some(s.t(d.category.label_key())),
                        icon: Some(d.icon.into()),
                        shortcut: None,
                    });
                }
                for c in s.connections.read().all() {
                    items.push(PaletteItem {
                        id: format!("conn:{}", c.id),
                        title: c.name.clone(),
                        subtitle: Some(hs_core::connections::kind_label(&c.kind).to_string()),
                        icon: None,
                        shortcut: None,
                    });
                }
                for e in s.environments.read().all() {
                    items.push(PaletteItem {
                        id: format!("env:{}", e.id),
                        title: e.name.clone(),
                        subtitle: Some(s.t("settings-environments")),
                        icon: Some(hs_ui::icons::ENVIRONMENT.into()),
                        shortcut: None,
                    });
                }
                // Files and other items the open tools contribute (the active tab first,
                // then the others, without repeating an item).
                let mut seen = std::collections::HashSet::new();
                let mut order: Vec<&crate::tabspace::TabEntry> = self.tabs.tabs.iter().collect();
                if let Some(a) = self.tabs.active {
                    order.sort_by_key(|t| t.id != a);
                }
                for t in order {
                    for q in t.tool.quick_open_items() {
                        if !seen.insert(q.id.clone()) {
                            continue;
                        }
                        items.push(PaletteItem {
                            id: format!("qo:{}:{}", t.id, q.id),
                            title: q.title,
                            subtitle: Some(q.subtitle),
                            icon: Some(hs_ui::icons::ph::FILE.into()),
                            shortcut: None,
                        });
                    }
                }
            }
        }
        items
    }

    fn handle_pending_actions(&mut self) {
        let actions = std::mem::take(&mut self.pending_actions);
        for a in actions {
            match a {
                Action::OpenTool {
                    factory_id,
                    request,
                } => self.open_tool(&factory_id, request),
                Action::OpenToolNewTab {
                    factory_id,
                    request,
                } => self.open_tool_in(&factory_id, request, false),
                Action::FocusTab { tab_id } => self.tabs.activate(tab_id),
                Action::CloseSelf => {
                    if let Some(a) = self.tabs.active {
                        self.close_tab(a, true);
                    }
                }
                Action::Toast { level, text } => self.toasts.push(level, text),
                Action::SetStatus { text } => self.status = text,
                Action::SetTitle { .. } => {}
                Action::RunCommand { id } => self.run_action(&id),
            }
        }
    }

    fn apply_settings_effects(&mut self, effects: Vec<SettingsEffect>, ctx: &egui::Context) {
        for e in effects {
            match e {
                SettingsEffect::SettingsChanged => {
                    if let Err(err) = self.services.save_settings() {
                        self.toasts.push(ToastLevel::Error, err.to_string());
                    }
                }
                SettingsEffect::ThemeChanged => {
                    let (appearance, zoom) = {
                        let s = self.services.settings.read();
                        (s.appearance.clone(), s.appearance.ui_zoom)
                    };
                    self.theme = Theme::from_settings(&appearance);
                    hs_ui::theme::install(ctx, &appearance);
                    hs_ui::theme::apply_font_size(
                        ctx,
                        self.services.settings.read().appearance.ui_font_size,
                    );
                    ctx.set_zoom_factor(zoom);
                    self.services.events.publish(Event::ThemeChanged);
                    let _ = self.services.save_settings();
                }
                SettingsEffect::FontsChanged => {
                    hs_ui::fonts::install(ctx, &font_choice(&self.services));
                    let _ = self.services.save_settings();
                }
                SettingsEffect::CheckUpdates => self.start_update_check(),
                SettingsEffect::DownloadUpdate => {
                    if let hs_core::updater::UpdateStatus::Available {
                        candidate, asset, ..
                    } = self.services.updater.status()
                    {
                        self.services
                            .updater
                            .download(&self.services.jobs, candidate, asset);
                    }
                }
                SettingsEffect::InstallUpdate => self.install_update(ctx),
                SettingsEffect::RevertUpdate => self.confirm_revert = true,
                SettingsEffect::SkipUpdate(tag) => {
                    self.services.settings.write().updates.skipped_version = Some(tag);
                    let _ = self.services.save_settings();
                    self.services
                        .updater
                        .set_status(hs_core::updater::UpdateStatus::Idle);
                    self.update_badge = None;
                }
                SettingsEffect::LockVault => self.run_action("vault.lock"),
                SettingsEffect::ChangeMasterPassword => {
                    self.vault_dialog = VaultDialog::ChangeMaster;
                    self.password.reset();
                }
                SettingsEffect::EnvironmentsChanged => {
                    self.services.events.publish(Event::EnvironmentsChanged);
                    self.toasts
                        .push(ToastLevel::Success, self.services.t("toast-saved"));
                }
                SettingsEffect::ConnectionsChanged => {
                    self.services.events.publish(Event::ConnectionsChanged);
                    self.toasts
                        .push(ToastLevel::Success, self.services.t("toast-saved"));
                }
                SettingsEffect::DeleteWorkspace => {
                    let name = self.services.workspace.read().meta.name.clone();
                    self.workspace_dialog = WorkspaceDialog::ConfirmDelete {
                        name,
                        delete_files: true,
                    };
                }
                SettingsEffect::RenameWorkspace(name) => {
                    let old_name = self.services.workspace.read().meta.name.clone();
                    let result = self.registry.rename_open(
                        &self.services.paths,
                        &mut self.services.workspace.write(),
                        &name,
                    );
                    match result {
                        Ok(()) => {
                            if self.services.settings.read().general.startup_workspace == old_name {
                                self.services.settings.write().general.startup_workspace =
                                    name.clone();
                            }
                            self.settings_ui.lock().state.ws_name =
                                Some((name.clone(), name.clone()));
                            self.services
                                .events
                                .publish(Event::WorkspaceSwitched { name: name.clone() });
                            if let Err(e) = self.services.save_settings() {
                                self.toasts.push(ToastLevel::Error, e.to_string());
                            } else {
                                self.toasts.push(
                                    ToastLevel::Success,
                                    self.services
                                        .i18n
                                        .t_args("settings-workspace-renamed", &[("name", &name)]),
                                );
                            }
                        }
                        Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                    }
                }
                SettingsEffect::WorkspaceLogoChanged => ctx.request_repaint(),
                SettingsEffect::Toast(level, text) => self.toasts.push(level, text),
                SettingsEffect::ExportSettings => {
                    self.portable = PortableDialog::Export;
                    self.password.reset();
                }
                SettingsEffect::ImportSettings => {
                    self.portable = PortableDialog::Import;
                    self.password.reset();
                }
                SettingsEffect::SpeakSample(text) => {
                    let rate = self.services.settings.read().accessibility.tts_rate;
                    if let Err(e) = self.tts.speak(&text, rate) {
                        self.toasts.push(ToastLevel::Error, e);
                    }
                }
                SettingsEffect::StopSpeaking => self.tts.stop(),
                SettingsEffect::RunInTerminal(command) => {
                    self.open_tool(
                        "terminal",
                        Some(OpenRequest::Terminal {
                            cwd: None,
                            host: None,
                            profile: None,
                            command: Some(command.clone()),
                        }),
                    );
                    let msg = self
                        .services
                        .i18n
                        .t_args("settings-install-running", &[("command", &command)]);
                    self.toasts.push(ToastLevel::Info, msg);
                }
                SettingsEffect::OpenUrl(url) => self.pending_open_url = Some(url),
                SettingsEffect::RunAction(id) => self.run_action(&id),
                SettingsEffect::CopySecret(value) => {
                    ctx.copy_text(value);
                    let after = self
                        .services
                        .settings
                        .read()
                        .security
                        .clear_clipboard_after_seconds;
                    // 0 means "leave it alone", which is the only honest
                    // reading of a zero timer.
                    self.clear_clipboard_at =
                        (after > 0).then(|| Instant::now() + Duration::from_secs(u64::from(after)));
                    self.toasts.push(
                        hs_core::tool::ToastLevel::Info,
                        self.services.t("settings-cred-copied"),
                    );
                }
                SettingsEffect::SyncEnvironments { from, to } => {
                    let result = (|| {
                        let src = self.registry.peek(&self.services.paths, &from)?;
                        let dst = self.registry.peek(&self.services.paths, &to)?;
                        Registry::clone_environments(&src, &dst)
                    })();
                    match result {
                        Ok(n) => {
                            let current = self.services.workspace.read().meta.name.clone();
                            if to == current {
                                let dir = self.services.workspace.read().environments_dir();
                                *self.services.environments.write() =
                                    hs_core::environments::Environments::load(&dir);
                                self.services.events.publish(Event::EnvironmentsChanged);
                            }
                            self.toasts.push(
                                ToastLevel::Success,
                                self.services.i18n.t_args(
                                    "settings-workspace-envs-copied",
                                    &[("count", &n.to_string()), ("to", &to)],
                                ),
                            );
                        }
                        Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                    }
                }
            }
        }
    }

    /// New Workspace and Delete Workspace modals.
    fn workspace_dialogs(&mut self, ctx: &egui::Context) {
        let s = self.services.clone();
        let theme = self.theme.clone();
        match &mut self.workspace_dialog {
            WorkspaceDialog::None => {}
            WorkspaceDialog::New {
                name,
                copy_from,
                icon,
            } => {
                let mut create = false;
                let mut cancel = false;
                let ws_names = self.registry.names();
                let modal = egui::Modal::new(egui::Id::new("hs-ws-new")).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.heading(s.t("workspace-new-title"));
                    ui.add_space(6.0);
                    hs_ui::form::hint(ui, &theme, &s.t("workspace-new-hint"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(s.t("workspace-name"));
                        let r = hs_ui::form::text_field(ui, name, &s.t("workspace-name-hint"));
                        r.request_focus();
                        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            create = true;
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(s.t("workspace-logo"));
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 6.0, theme.surface);
                        if let Some(img) = icon.as_ref().and_then(|p| logo_image(p)) {
                            img.fit_to_exact_size(egui::vec2(36.0, 36.0))
                                .paint_at(ui, rect.shrink(2.0));
                        } else {
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                hs_ui::icons::WORKSPACE,
                                egui::FontId::proportional(20.0),
                                theme.text_muted,
                            );
                        }
                        ui.add_space(6.0);
                        if ui.button(s.t("workspace-logo-choose")).clicked()
                            && let Some(p) = rfd::FileDialog::new()
                                .add_filter("Image", hs_core::workspace::ICON_EXTS)
                                .pick_file()
                        {
                            *icon = Some(p);
                        }
                        if icon.is_some() && ui.button(s.t("workspace-logo-remove")).clicked() {
                            *icon = None;
                        }
                    });
                    ui.add_space(2.0);
                    hs_ui::form::hint(ui, &theme, &s.t("workspace-logo-hint"));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(s.t("workspace-copy-from"));
                        let none = s.t("workspace-copy-none");
                        let selected = copy_from.clone().unwrap_or_else(|| none.clone());
                        egui::ComboBox::from_id_salt("ws-copy-from")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(copy_from, None, &none);
                                for n in &ws_names {
                                    ui.selectable_value(copy_from, Some(n.clone()), n);
                                }
                            });
                    });
                    ui.add_space(2.0);
                    hs_ui::form::hint(ui, &theme, &s.t("workspace-copy-from-hint"));
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if hs_ui::form::primary_button(ui, &theme, &s.t("workspace-create"))
                            .clicked()
                        {
                            create = true;
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
                if modal.should_close() {
                    cancel = true;
                }
                if create && !name.trim().is_empty() {
                    let name = name.trim().to_string();
                    let copy_from = copy_from.clone();
                    let icon = icon.clone();
                    self.workspace_dialog = WorkspaceDialog::None;
                    self.create_workspace(&name, copy_from.as_deref(), icon.as_deref());
                } else if cancel {
                    self.workspace_dialog = WorkspaceDialog::None;
                }
            }
            WorkspaceDialog::ConfirmDelete { name, delete_files } => {
                let name = name.clone();
                let mut files = *delete_files;
                let mut decide: Option<bool> = None;
                let modal = egui::Modal::new(egui::Id::new("hs-ws-del")).show(ctx, |ui| {
                    ui.set_width(440.0);
                    ui.heading(s.t("workspace-delete-title"));
                    ui.add_space(6.0);
                    ui.label(s.i18n.t_args("workspace-delete-body", &[("name", &name)]));
                    ui.add_space(6.0);
                    ui.checkbox(&mut files, s.t("workspace-delete-files"));
                    ui.add_space(12.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if hs_ui::form::danger_button(ui, &theme, &s.t("delete")).clicked() {
                            decide = Some(true);
                        }
                        if ui.button(s.t("cancel")).clicked() {
                            decide = Some(false);
                        }
                    });
                });
                if let WorkspaceDialog::ConfirmDelete { delete_files, .. } =
                    &mut self.workspace_dialog
                {
                    *delete_files = files;
                }
                if modal.should_close() && decide.is_none() {
                    decide = Some(false);
                }
                match decide {
                    Some(true) => {
                        self.workspace_dialog = WorkspaceDialog::None;
                        self.delete_workspace(&name, files);
                    }
                    Some(false) => self.workspace_dialog = WorkspaceDialog::None,
                    None => {}
                }
            }
        }
    }

    fn create_workspace(
        &mut self,
        name: &str,
        copy_from: Option<&str>,
        icon: Option<&std::path::Path>,
    ) {
        // Load the chosen source workspace read-only (never through
        // `Registry::open`, which would move `last_opened`).
        let source = copy_from.and_then(|src| {
            if src == self.services.workspace.read().meta.name {
                return Some(self.services.workspace.read().clone());
            }
            let entry = self.registry.workspaces.iter().find(|w| w.name == src)?;
            let root = self.services.paths.resolve_path(&entry.path);
            hs_core::workspace::Workspace::open_or_create(&root, src).ok()
        });
        let result = self
            .registry
            .create(&self.services.paths, name, source.as_ref(), icon);
        match result {
            Ok(_) => {
                self.save_layout();
                match bootstrap::switch_workspace(&self.services, &mut self.registry, name) {
                    Ok(()) => {
                        self.tabs = TabSpace::new();
                        self.restore_layout();
                        self.toasts.push(
                            ToastLevel::Success,
                            self.services
                                .i18n
                                .t_args("workspace-created", &[("name", name)]),
                        );
                    }
                    Err(e) => self.toasts.push(ToastLevel::Error, e),
                }
            }
            Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
        }
    }

    fn delete_workspace(&mut self, name: &str, delete_files: bool) {
        let remaining: Vec<String> = self
            .registry
            .names()
            .into_iter()
            .filter(|n| n != name)
            .collect();
        let Some(next) = remaining.first().cloned() else {
            self.toasts
                .push(ToastLevel::Error, self.services.t("workspace-delete-last"));
            return;
        };
        let switch_away = self.services.workspace.read().meta.name == name;
        if switch_away {
            self.save_layout();
            if let Err(e) = bootstrap::switch_workspace(&self.services, &mut self.registry, &next) {
                self.toasts.push(ToastLevel::Error, e);
                return;
            }
            self.tabs = TabSpace::new();
            self.restore_layout();
        }
        // A startup preference pointing at the deleted workspace would
        // fail at the next launch; the registry's own fallback takes over.
        if self.services.settings.read().general.startup_workspace == name {
            self.services
                .settings
                .write()
                .general
                .startup_workspace
                .clear();
            let _ = self.services.save_settings();
        }
        match self
            .registry
            .remove(&self.services.paths, name, delete_files)
        {
            Ok(()) => self.toasts.push(
                ToastLevel::Success,
                self.services
                    .i18n
                    .t_args("workspace-deleted", &[("name", name)]),
            ),
            Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
        }
    }

    fn vault_dialogs(&mut self, ctx: &egui::Context) {
        let s = self.services.clone();
        match self.vault_dialog {
            VaultDialog::None => {}
            VaultDialog::Create => {
                let labels = PasswordLabels {
                    title: &s.t("vault-create-title"),
                    body: &s.t("vault-create-body"),
                    password: &s.t("vault-password"),
                    confirm: Some(&s.t("vault-password-confirm")),
                    ok: &s.t("vault-create"),
                    cancel: &s.t("vault-later"),
                    mismatch: &s.t("vault-mismatch"),
                };
                match dialog::password(ctx, &self.theme, &mut self.password, &labels) {
                    Some(Ok(pw)) => {
                        match s.secrets.initialize(&SecretString::from(pw)) {
                            Ok(()) => {
                                self.services.events.publish(Event::VaultUnlocked);
                                self.toasts
                                    .push(ToastLevel::Success, s.t("vault-unlocked-toast"));
                            }
                            Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                        }
                        self.password = PasswordDialogState::default();
                        self.vault_dialog = VaultDialog::None;
                    }
                    Some(Err(())) => {
                        self.password = PasswordDialogState::default();
                        self.vault_dialog = VaultDialog::None;
                    }
                    None => {}
                }
            }
            VaultDialog::Unlock => {
                let labels = PasswordLabels {
                    title: &s.t("vault-unlock-title"),
                    body: "",
                    password: &s.t("vault-password"),
                    confirm: None,
                    ok: &s.t("vault-unlock"),
                    cancel: &s.t("vault-later"),
                    mismatch: &s.t("vault-wrong"),
                };
                match dialog::password(ctx, &self.theme, &mut self.password, &labels) {
                    Some(Ok(pw)) => match s.secrets.unlock(&SecretString::from(pw)) {
                        Ok(()) => {
                            self.services.events.publish(Event::VaultUnlocked);
                            self.toasts
                                .push(ToastLevel::Success, s.t("vault-unlocked-toast"));
                            self.password = PasswordDialogState::default();
                            self.vault_dialog = VaultDialog::None;
                        }
                        Err(_) => {
                            self.password.error = Some(s.t("vault-wrong"));
                            self.password.password.clear();
                        }
                    },
                    Some(Err(())) => {
                        self.password = PasswordDialogState::default();
                        self.vault_dialog = VaultDialog::None;
                    }
                    None => {}
                }
            }
            VaultDialog::ChangeMaster => {
                let labels = PasswordLabels {
                    title: &s.t("settings-vault-change"),
                    body: &s.t("vault-create-body"),
                    password: &s.t("vault-password"),
                    confirm: Some(&s.t("vault-password-confirm")),
                    ok: &s.t("save"),
                    cancel: &s.t("cancel"),
                    mismatch: &s.t("vault-mismatch"),
                };
                match dialog::password(ctx, &self.theme, &mut self.password, &labels) {
                    Some(Ok(pw)) => {
                        match s.secrets.change_master(&SecretString::from(pw)) {
                            Ok(()) => self.toasts.push(ToastLevel::Success, s.t("toast-saved")),
                            Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                        }
                        self.password = PasswordDialogState::default();
                        self.vault_dialog = VaultDialog::None;
                    }
                    Some(Err(())) => {
                        self.password = PasswordDialogState::default();
                        self.vault_dialog = VaultDialog::None;
                    }
                    None => {}
                }
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 10,
                right: 10,
                top: 8,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    ui.add(app_logo().fit_to_exact_size(egui::vec2(22.0, 22.0)));
                    ui.label(
                        egui::RichText::new(s.t("app-name"))
                            .strong()
                            .size(16.0)
                            .color(self.theme.accent),
                    );
                    ui.add_space(8.0);
                    // Workspace switcher.
                    let ws_name = s.workspace.read().meta.name.clone();
                    let ws_root = s.workspace.read().root().to_path_buf();
                    let ws_logo = ws_logo_image(&ws_root);
                    let workspace_button = match ws_logo {
                        Some(image) => egui::Button::image_and_text(
                            image.fit_to_exact_size(egui::vec2(18.0, 18.0)),
                            ws_name.clone(),
                        ),
                        None => {
                            egui::Button::new(format!("{}  {ws_name}", hs_ui::icons::WORKSPACE))
                        }
                    }
                    .right_text(hs_ui::icons::CARET_DOWN);
                    egui::containers::menu::MenuButton::from_button(workspace_button).ui(
                        ui,
                        |ui| {
                            let rows: Vec<(String, std::path::PathBuf)> = self
                                .registry
                                .workspaces
                                .iter()
                                .map(|w| (w.name.clone(), s.paths.resolve_path(&w.path)))
                                .collect();
                            // One workspace must always remain.
                            let can_delete = rows.len() > 1;
                            for (n, root) in rows {
                                ui.horizontal(|ui| {
                                    match ws_logo_image(&root) {
                                        Some(img) => {
                                            ui.add(img.fit_to_exact_size(egui::vec2(16.0, 16.0)));
                                        }
                                        None => {
                                            ui.label(hs_ui::icons::WORKSPACE);
                                        }
                                    }
                                    if ui.selectable_label(n == ws_name, &n).clicked()
                                        && n != ws_name
                                    {
                                        self.pending_actions.push(Action::RunCommand {
                                            id: format!("ws:{n}"),
                                        });
                                    }
                                    if can_delete {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(hs_ui::icons::TRASH)
                                                            .frame(false),
                                                    )
                                                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                    .clicked()
                                                {
                                                    self.pending_actions.push(Action::RunCommand {
                                                        id: format!("ws.delete:{n}"),
                                                    });
                                                    ui.close();
                                                }
                                            },
                                        );
                                    }
                                });
                            }
                            ui.separator();
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} {}", hs_ui::icons::TAB_NEW, s.t("workspace-new")),
                                )
                                .clicked()
                            {
                                self.pending_actions.push(Action::RunCommand {
                                    id: "ws.new".into(),
                                });
                            }
                        },
                    );
                    // Environment switcher
                    let (active_name, active_col, active_protected) = {
                        let envs = s.environments.read();
                        match envs.active_env() {
                            Some(e) => (
                                e.name.clone(),
                                self.theme.env_colour(&e.name, e.colour.as_deref()),
                                e.protected,
                            ),
                            None => (s.t("env-none"), self.theme.text_muted, false),
                        }
                    };
                    let label = egui::RichText::new(format!(
                        "{} {}{}",
                        hs_ui::icons::ENVIRONMENT,
                        if active_protected { "🔒 " } else { "" },
                        active_name
                    ))
                    .color(active_col);
                    egui::ComboBox::from_id_salt("env")
                        .selected_text(label)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    s.environments.read().active.is_none(),
                                    s.t("env-none"),
                                )
                                .clicked()
                            {
                                self.pending_actions
                                    .push(Action::RunCommand { id: "env:".into() });
                            }
                            let list: Vec<(String, String, Option<String>, bool)> = s
                                .environments
                                .read()
                                .all()
                                .iter()
                                .map(|e| {
                                    (e.id.clone(), e.name.clone(), e.colour.clone(), e.protected)
                                })
                                .collect();
                            let active = s.environments.read().active.clone();
                            for (id, name, colour, protected) in list {
                                let col = self.theme.env_colour(&name, colour.as_deref());
                                let txt = egui::RichText::new(format!(
                                    "{}{}",
                                    if protected { "🔒 " } else { "" },
                                    name
                                ))
                                .color(col);
                                if ui
                                    .selectable_label(active.as_deref() == Some(&id), txt)
                                    .clicked()
                                {
                                    self.pending_actions.push(Action::RunCommand {
                                        id: format!("env:{id}"),
                                    });
                                }
                            }
                        });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let theme = self.theme.clone();
                        if hs_ui::form::icon_button(ui, &theme, hs_ui::icons::SETTINGS, 20.0, false)
                            .tip(s.t("settings"))
                            .clicked()
                        {
                            self.open_settings(None);
                        }
                        let locked = s.secrets.is_locked();
                        let lock_icon = if locked {
                            hs_ui::icons::LOCK
                        } else {
                            hs_ui::icons::UNLOCK
                        };
                        let r = hs_ui::form::icon_button(ui, &theme, lock_icon, 20.0, false);
                        // Vault state colour on top of the button glyph.
                        ui.painter().text(
                            r.rect.center(),
                            egui::Align2::CENTER_CENTER,
                            lock_icon,
                            egui::FontId::proportional(20.0),
                            if locked { theme.warning } else { theme.success },
                        );
                        if r.tip(if locked {
                            s.t("vault-tip-locked")
                        } else {
                            s.t("vault-tip-unlocked")
                        })
                        .clicked()
                        {
                            self.pending_actions.push(Action::RunCommand {
                                id: if locked {
                                    "vault.unlock".into()
                                } else {
                                    "vault.lock".into()
                                },
                            });
                        }
                        if s.settings.read().accessibility.tts_enabled {
                            self.tts_menu(ui);
                        }
                        self.snip_menu(ui);
                        if hs_ui::form::icon_button(ui, &theme, hs_ui::icons::BELL, 20.0, false)
                            .tip(s.t("notifications"))
                            .clicked()
                        {
                            self.notifications_open = !self.notifications_open;
                            self.notifications_opened_this_frame = self.notifications_open;
                        }
                        if hs_ui::form::icon_button(ui, &theme, hs_ui::icons::SEARCH, 20.0, false)
                            .tip(s.t("quick-open"))
                            .clicked()
                        {
                            self.palette.open(PaletteMode::QuickOpen);
                        }
                    });
                });
            });
    }

    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let (min_w, gap) = {
            let st = s.settings.read();
            (st.appearance.min_tab_width, st.appearance.tab_gap)
        };
        let labels = TabStripLabels {
            new_tab: s.t("tab-new"),
            close: s.t("tab-close"),
            close_others: s.t("tab-close-others"),
            close_left: s.t("tab-close-left"),
            close_right: s.t("tab-close-right"),
            pin: s.t("tab-pin"),
            unpin: s.t("tab-unpin"),
            duplicate: s.t("tab-duplicate"),
            duplicate_blocked: s.t("tab-duplicate-blocked"),
            copy_path: s.t("editor-copy-path"),
        };
        let theme = self.theme.clone();
        let env_colour = |env: Option<String>| -> Option<Color32> {
            let envs = s.environments.read();
            env.and_then(|id| {
                envs.by_id(&id)
                    .map(|e| theme.env_colour(&e.name, e.colour.as_deref()))
            })
        };
        let all = self.tabs.views(&env_colour);
        // Index map from the strip's slice back into the full tab list, so
        // reorder and close-left/right keep working on the filtered strip.
        let group = self.group.clone();
        let index_map: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                group
                    .as_deref()
                    .is_none_or(|g| group_of(&v.factory_id) == g)
            })
            .map(|(i, _)| i)
            .collect();
        let views: Vec<_> = index_map.iter().map(|&i| all[i].clone()).collect();
        let group_ids: Vec<u64> = views.iter().map(|v| v.id).collect();
        let visible = self.tabs.visible();
        let strip = TabStrip {
            theme: &self.theme,
            min_width: min_w,
            gap,
            active: self.tabs.active,
            visible: &visible,
            labels: &labels,
            compact: false,
            show_new_tab: shows_new_tab(self.group.as_deref()),
        };
        let r = strip.show(ui, &views);
        if let Some(id) = r.activated {
            self.tabs.activate(id);
        }
        if let Some(id) = r.close {
            self.close_tab(id, false);
        }
        if let Some((from, to)) = r.reorder
            && let (Some(&from), Some(&to)) = (index_map.get(from), index_map.get(to))
        {
            self.tabs.reorder(from, to);
        }
        if r.new_tab {
            match self.group.clone() {
                Some(g) => self.open_tool(&g, None),
                None => self.open_launcher_tab(),
            }
        }
        if let Some((id, action)) = r.menu {
            match action {
                TabMenuAction::Close => self.close_tab(id, false),
                TabMenuAction::CloseOthers => {
                    for other in self.tabs.close_others(id) {
                        if group_ids.contains(&other) {
                            self.close_tab(other, false);
                        }
                    }
                }
                TabMenuAction::CloseLeft => {
                    for other in self.tabs.close_side(id, true) {
                        if group_ids.contains(&other) {
                            self.close_tab(other, false);
                        }
                    }
                }
                TabMenuAction::CloseRight => {
                    for other in self.tabs.close_side(id, false) {
                        if group_ids.contains(&other) {
                            self.close_tab(other, false);
                        }
                    }
                }
                TabMenuAction::CopyPath => {}
                TabMenuAction::Pin => self.tabs.set_pinned(id, true),
                TabMenuAction::Unpin => {
                    let is_settings = self
                        .tabs
                        .tabs
                        .iter()
                        .any(|t| t.id == id && is_mandatory(&t.factory_id));
                    if !is_settings {
                        self.tabs.set_pinned(id, false);
                    }
                }
                TabMenuAction::Duplicate => {
                    // The tool decides what a duplicate *is*: a copy of
                    // the tab for a scratch document, a numbered copy of
                    // the saved document for the API client.
                    let mut actions = Vec::new();
                    let env = self.services.environments.read().active.clone();
                    let dup = self.tabs.get_mut(id).map(|t| {
                        let mut cx = ToolContext {
                            services: &self.services,
                            tab_id: id,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                            is_home: false,
                        };
                        (t.factory_id.clone(), t.tool.duplicate_state(&mut cx))
                    });
                    self.pending_actions.extend(actions);
                    if let Some((fid, state)) = dup
                        && !is_mandatory(&fid)
                        && let Some(i) = self.factory_index(&fid)
                    {
                        self.tabs
                            .open(self.factories[i].as_ref(), &self.services, Some(state));
                    }
                }
            }
        }
    }

    fn env_banner(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let envs = s.environments.read();
        let active = envs.active_env();
        let tab_env = self
            .tabs
            .active
            .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
            .and_then(|t| t.tool.environment());
        let shown = tab_env.as_deref().and_then(|id| envs.by_id(id)).or(active);
        let pinned_note = match (tab_env.as_deref(), envs.active.as_deref()) {
            (Some(t), a) if Some(t) != a => {
                shown.map(|e| s.i18n.t_args("env-banner-pinned", &[("name", &e.name)]))
            }
            _ => None,
        };
        let banner = EnvBanner {
            theme: &self.theme,
            name: shown.map(|e| e.name.as_str()),
            colour: shown
                .map(|e| self.theme.env_colour(&e.name, e.colour.as_deref()))
                .unwrap_or(self.theme.border),
            protected: shown.is_some_and(|e| e.protected),
            pinned_note: pinned_note.as_deref(),
            none_label: &s.t("env-none"),
            protected_label: &s.t("env-protected"),
        };
        banner.show(ui);
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.tabs.active else { return };
        let env = self.services.environments.read().active.clone();
        let mut actions = Vec::new();
        let services = self.services.clone();
        if let Some(t) = self.tabs.get_mut(a) {
            // A dashboard home tab holds no document and no connection, so
            // its activity's toolbar would aim Save, Run and Download at
            // nothing. Owner's report, 2026-10-17.
            if t.home && t.tool.home_is_dashboard() {
                return;
            }
            let is_home = t.home;
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: 8,
                    right: 8,
                    top: 8,
                    bottom: 8,
                })
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let mut cx = ToolContext {
                            services: &services,
                            tab_id: a,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                            is_home,
                        };
                        t.tool.toolbar(ui, &mut cx);
                    });
                });
        }
        self.pending_actions.extend(actions);
    }

    /// The activity rail (docs/04 §1): Home first, the tools in the order
    /// from Settings → Appearance → Activity Rail, then the sidebar toggle and
    /// Settings at the bottom. Labels under the icons are optional.
    fn rail(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let (labels, order, hidden) = {
            let st = s.settings.read();
            (
                st.appearance.rail_labels,
                st.appearance.rail_order.clone(),
                st.appearance.rail_hidden.clone(),
            )
        };
        let theme = self.theme.clone();
        let descs: Vec<(&'static str, &'static str, String)> = self
            .factories
            .iter()
            .map(|f| {
                (
                    f.descriptor().id,
                    f.descriptor().icon,
                    s.t(f.descriptor().title_key),
                )
            })
            .collect();
        let ids: Vec<&'static str> = descs.iter().map(|d| d.0).collect();
        let ordered: Vec<&'static str> = rail_order(&ids, &order)
            .into_iter()
            .filter(|id| !hidden.iter().any(|h| h == id))
            .collect();
        let item = |this: &mut Self, ui: &mut egui::Ui, id: &str| {
            let Some((_, icon, title)) = descs.iter().find(|d| d.0 == id) else {
                return;
            };
            let active = this.group.as_deref() == Some(group_of(id));
            let count = this.tabs.tabs.iter().filter(|t| t.factory_id == id).count();
            let tip = if count > 1 && !is_mandatory(id) {
                format!("{title} ({count})")
            } else {
                title.clone()
            };
            if rail_item(ui, &theme, icon, title, active, labels)
                .tip(tip)
                .clicked()
            {
                this.select_group(id);
                // The strip and main area change next frame; make sure it comes.
                ui.ctx().request_repaint();
            }
        };
        // Home stays at the top and the sidebar toggle and Settings at the
        // bottom; only the tools between them scroll, so a window too short
        // for every tool hides none of the three fixed entries.
        let entry_h = if labels { 50.0 } else { 24.0 + 10.0 };
        let gap = if labels { 2.0 } else { 4.0 };
        let bottom_h = 8.0 + entry_h + 4.0 + entry_h + 6.0;
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            item(self, ui, "home");
            ui.add_space(6.0);
            let tools_h = (ui.available_height() - bottom_h).max(entry_h);
            egui::ScrollArea::vertical()
                .id_salt("hs-rail-scroll")
                .max_height(tools_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        for id in ordered.iter().filter(|id| !is_mandatory(id)) {
                            item(self, ui, id);
                            ui.add_space(gap);
                        }
                    });
                });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                item(self, ui, "settings");
                ui.add_space(4.0);
                if rail_item(
                    ui,
                    &theme,
                    hs_ui::icons::SIDEBAR,
                    &s.t("sidebar"),
                    self.sidebar_open,
                    labels,
                )
                .tip(s.t("sidebar-toggle"))
                .clicked()
                {
                    self.sidebar_open = !self.sidebar_open;
                }
            });
        });
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.tabs.active else {
            ui.colored_label(self.theme.text_muted, self.services.t("launcher-title"));
            return;
        };
        let env = self.services.environments.read().active.clone();
        let mut actions = Vec::new();
        let services = self.services.clone();
        let mut drew = false;
        if let Some(t) = self.tabs.get_mut(a) {
            let is_home = t.home;
            let mut cx = ToolContext {
                services: &services,
                tab_id: a,
                env_id: env.as_deref(),
                actions: &mut actions,
                is_home,
            };
            drew = t.tool.sidebar(ui, &mut cx);
        }
        if !drew {
            ui.colored_label(self.theme.text_muted, self.services.t("app-tagline"));
        }
        self.pending_actions.extend(actions);
    }

    fn main_area(&mut self, ui: &mut egui::Ui) {
        if self.tabs.active.is_none() || self.tabs.tabs.is_empty() {
            let descs: Vec<&hs_core::tool::ToolDescriptor> =
                self.factories.iter().map(|f| f.descriptor()).collect();
            let closed: Vec<(usize, String, String)> = self
                .tabs
                .closed
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.factory_id.clone(), c.title.clone()))
                .collect();
            let mut r = launcher::show(ui, &self.theme, &self.services, &mut self.launcher, &descs);
            // With no tabs there is no Home sidebar either, so the column
            // that normally carries Pinned and Recent is drawn under the
            // page instead of being lost.
            let side = ui
                .vertical_centered(|ui| {
                    ui.set_width(ui.available_width().min(520.0));
                    ui.add_space(20.0);
                    launcher::side(ui, &self.theme, &self.services, &descs, &closed)
                })
                .inner;
            r.reopen_closed = r.reopen_closed.or(side.reopen_closed);
            r.open_connection = r.open_connection.or(side.open_connection);
            if let Some(id) = r.open_tool {
                self.open_tool(&id, None);
            }
            if let Some(i) = r.reopen_closed {
                self.reopen_closed(Some(i));
            }
            if let Some((tool, id)) = r.open_connection {
                self.open_tool(&tool, Some(OpenRequest::Connection { id }));
            }
            return;
        }
        let env = self.services.environments.read().active.clone();
        let services = self.services.clone();
        let mut actions = Vec::new();
        let protected = {
            let envs = services.environments.read();
            envs.active_env().is_some_and(|e| e.protected).then(|| {
                envs.active_env()
                    .map(|e| self.theme.env_colour(&e.name, e.colour.as_deref()))
                    .unwrap_or(self.theme.env_prod)
            })
        };
        let rect = ui.available_rect_before_wrap();
        self.main_rect = rect;
        let theme = self.theme.clone();
        let mut activate = None;
        if let Some(mut split) = self.tabs.split.take() {
            let tabs = &mut self.tabs;
            split.show(ui, &theme, &mut |ui, id| {
                let frame = egui::Frame::new()
                    .stroke(egui::Stroke::new(
                        1.0,
                        if tabs.active == Some(id) {
                            theme.accent
                        } else {
                            theme.border
                        },
                    ))
                    .inner_margin(egui::Margin::same(4));
                frame.show(ui, |ui| {
                    let r = ui.interact(
                        ui.max_rect(),
                        ui.id().with(("pane-focus", id)),
                        egui::Sense::click(),
                    );
                    if r.clicked() {
                        activate = Some(id);
                    }
                    if let Some(t) = tabs.tabs.iter_mut().find(|t| t.id == id) {
                        let is_home = t.home;
                        let mut cx = ToolContext {
                            services: &services,
                            tab_id: id,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                            is_home,
                        };
                        t.tool.ui(ui, &mut cx);
                    }
                });
            });
            self.tabs.split = Some(split);
        } else if let Some(a) = self.tabs.active
            && let Some(t) = self.tabs.get_mut(a)
        {
            let is_home = t.home;
            let mut cx = ToolContext {
                services: &services,
                tab_id: a,
                env_id: env.as_deref(),
                actions: &mut actions,
                is_home,
            };
            t.tool.ui(ui, &mut cx);
        }
        if let Some(id) = activate {
            self.tabs.activate(id);
        }
        if let Some(col) = protected {
            hs_ui::env_banner::protected_frame(ui, rect, col);
        }
        self.pending_actions.extend(actions);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let theme = self.theme.clone();
        let text = |t: String| egui::RichText::new(t).size(13.0);
        let muted = |t: String| egui::RichText::new(t).size(13.0).color(theme.text_muted);
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(8, 0))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    if hs_ui::form::icon_button(
                        ui,
                        &theme,
                        hs_ui::icons::DOCK,
                        15.0,
                        self.dock_open,
                    )
                    .tip(s.t("dock-toggle"))
                    .clicked()
                    {
                        self.dock_open = !self.dock_open;
                    }
                    ui.label(text(self.status.clone()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        ui.label(muted(format!("v{}", s.version.version)));
                        ui.separator();
                        ui.label(muted(s.i18n.locale.clone()));
                        let n = s.jobs.running_count();
                        let jobs = s.i18n.t_args("status-jobs", &[("n", &n.to_string())]);
                        if ui
                            .add(
                                egui::Button::new(text(format!("{} {jobs}", hs_ui::icons::JOBS)))
                                    .frame(false),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .tip(s.t("status-jobs-tip"))
                            .clicked()
                        {
                            self.dock_open = true;
                            self.dock.tab = dock::DockTab::Jobs;
                        }
                        ui.label(muted(
                            s.i18n.t_args(
                                "tab-count",
                                &[(
                                    "n",
                                    &self
                                        .tabs
                                        .tabs
                                        .iter()
                                        .filter(|t| !is_mandatory(&t.factory_id))
                                        .count()
                                        .to_string(),
                                )],
                            ),
                        ));
                        if let Some(v) = &self.update_badge {
                            let label = egui::RichText::new(format!(
                                "{} {} {v}",
                                hs_ui::icons::ARROW_UP,
                                s.t("update-badge")
                            ))
                            .size(13.0)
                            .color(egui::Color32::WHITE);
                            if ui
                                .add(
                                    egui::Button::new(label)
                                        .fill(theme.accent)
                                        .corner_radius(egui::CornerRadius::same(9)),
                                )
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.open_settings(Some(settings_ui::Page::About));
                            }
                        }
                    });
                });
            });
    }

    fn handle_home_request(&mut self, r: HomeRequest) {
        match r {
            HomeRequest::OpenTool(id) => self.open_tool(&id, None),
            HomeRequest::ReopenClosed(i) => self.reopen_closed(Some(i)),
            HomeRequest::OpenConnection { tool, id } => {
                self.open_tool(&tool, Some(OpenRequest::Connection { id }))
            }
            HomeRequest::OpenFolderAsProject => {
                self.open_tool("editor", None);
                self.run_action("file.open_folder");
            }
            HomeRequest::NewConnection => {
                self.open_settings(Some(settings_ui::Page::Connections));
                self.settings_ui.lock().state.conn_draft = None;
                self.settings_ui.lock().state.conn_selected = None;
            }
            HomeRequest::NewWorkspace => self.run_action("ws.new"),
            HomeRequest::SwitchWorkspace(name) => self.run_action(&format!("ws:{name}")),
            HomeRequest::OpenSettingsPage(key) => {
                self.open_settings(settings_ui::Page::by_key(key));
            }
            HomeRequest::RunAction(a) => self.run_action(a),
        }
    }

    /// The Read Aloud menu in the title bar: what the active tab offers,
    /// plus Stop while something is being read.
    fn tts_menu(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let theme = self.theme.clone();
        let speaking = self.tts.is_speaking();
        let items: Vec<hs_core::tool::Speakable> = self
            .tabs
            .active
            .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
            .map(|t| t.tool.speakable())
            .unwrap_or_default();
        let icon = if speaking {
            hs_ui::icons::ph::SPEAKER_HIGH
        } else {
            hs_ui::icons::ph::SPEAKER_NONE
        };
        let r = hs_ui::form::icon_menu(ui, &theme, icon, 20.0, speaking, |ui| {
            ui.set_min_width(240.0);
            ui.label(
                egui::RichText::new(s.t("tts-menu-title"))
                    .strong()
                    .color(theme.text_muted),
            );
            if items.is_empty() {
                ui.label(egui::RichText::new(s.t("tts-nothing")).color(theme.text_muted));
            }
            let rate = s.settings.read().accessibility.tts_rate;
            for it in &items {
                let words = it.text.split_whitespace().count();
                let label = s.i18n.t_args(
                    "tts-read-item",
                    &[("label", &it.label), ("words", &words.to_string())],
                );
                if ui.button(label).clicked() {
                    if let Err(e) = self.tts.speak(&it.text, rate) {
                        self.toasts.push(ToastLevel::Error, e);
                    }
                    ui.close();
                }
            }
            if speaking {
                ui.separator();
                if ui
                    .button(format!("{} {}", hs_ui::icons::ph::STOP, s.t("tts-stop")))
                    .clicked()
                {
                    self.tts.stop();
                    ui.close();
                }
            }
            ui.separator();
            if ui.button(s.t("tts-settings")).clicked() {
                self.open_settings(Some(settings_ui::Page::Accessibility));
                ui.close();
            }
        });
        r.tip(s.t("tts-tip"));
    }

    /// Settings export/import: confirm the master password, then pick the
    /// file. The bundle holds settings and shortcuts, never secrets.
    fn portable_dialogs(&mut self, ctx: &egui::Context) {
        if self.portable == PortableDialog::None {
            return;
        }
        let s = self.services.clone();
        let exporting = self.portable == PortableDialog::Export;
        let labels = PasswordLabels {
            title: &s.t(if exporting {
                "settings-export-title"
            } else {
                "settings-import-title"
            }),
            body: &s.t("settings-portable-password-body"),
            password: &s.t("vault-password"),
            confirm: None,
            ok: &s.t("ok"),
            cancel: &s.t("cancel"),
            mismatch: &s.t("vault-wrong"),
        };
        match dialog::password(ctx, &self.theme, &mut self.password, &labels) {
            Some(Ok(pw)) => {
                if !s.secrets.exists() {
                    self.toasts
                        .push(ToastLevel::Error, s.t("settings-portable-no-vault"));
                    self.password = PasswordDialogState::default();
                    self.portable = PortableDialog::None;
                    return;
                }
                if s.secrets.unlock(&SecretString::from(pw)).is_err() {
                    self.password.error = Some(s.t("vault-wrong"));
                    self.password.password.clear();
                    return;
                }
                self.services.events.publish(Event::VaultUnlocked);
                self.password = PasswordDialogState::default();
                self.portable = PortableDialog::None;
                if exporting {
                    self.export_settings();
                } else {
                    self.import_settings(ctx);
                }
            }
            Some(Err(())) => {
                self.password = PasswordDialogState::default();
                self.portable = PortableDialog::None;
            }
            None => {}
        }
    }

    fn export_settings(&mut self) {
        let s = self.services.clone();
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("hyperstack-settings.toml")
            .add_filter("HyperStack settings", &["toml"])
            .save_file()
        else {
            return;
        };
        let bundle = hs_core::portable::export(
            &s.settings.read(),
            &s.shortcuts.read(),
            &s.version.to_string(),
        );
        match hs_core::portable::write(&path, &bundle) {
            Ok(()) => self.toasts.push(
                ToastLevel::Success,
                s.i18n.t_args(
                    "settings-exported",
                    &[("path", &path.display().to_string())],
                ),
            ),
            Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
        }
    }

    fn import_settings(&mut self, ctx: &egui::Context) {
        let s = self.services.clone();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("HyperStack settings", &["toml"])
            .pick_file()
        else {
            return;
        };
        let bundle = match hs_core::portable::read(&path) {
            Ok(b) => b,
            Err(e) => {
                self.toasts.push(ToastLevel::Error, e.to_string());
                return;
            }
        };
        let report = {
            let mut settings = s.settings.write();
            let mut keymap = s.shortcuts.write();
            hs_core::portable::import(&bundle, &mut settings, &mut keymap)
        };
        if let Err(e) = s.save_settings() {
            self.toasts.push(ToastLevel::Error, e.to_string());
        }
        if let Err(e) = s.shortcuts.read().save(&s.paths.shortcuts_file()) {
            self.toasts.push(ToastLevel::Error, e.to_string());
        }
        self.apply_settings_effects(
            vec![SettingsEffect::ThemeChanged, SettingsEffect::FontsChanged],
            ctx,
        );
        self.toasts.push(
            ToastLevel::Success,
            s.i18n.t_args(
                "settings-imported",
                &[
                    ("added", &report.providers_added.to_string()),
                    ("updated", &report.providers_updated.to_string()),
                    ("keys", &report.shortcuts_set.to_string()),
                ],
            ),
        );
    }

    /// Tell every tool what the window has open, and keep a picture of
    /// the active tab so a dashboard has something to show for it. A tab
    /// that is not active is not drawn at all, so the only picture there
    /// can be is the last one taken while it was on screen.
    fn publish_tabs(&mut self, ctx: &egui::Context) {
        let active = self.tabs.active;
        let tabs: Vec<hs_core::open_tabs::TabInfo> = self
            .tabs
            .tabs
            .iter()
            .map(|t| hs_core::open_tabs::TabInfo {
                id: t.id,
                factory_id: t.factory_id.clone(),
                title: t.tool.title().to_string(),
                subtitle: t.tool.subtitle().unwrap_or_default(),
                // Filled in from the cache below: `set` replaces the list,
                // so the picture has to be put back each frame.
                thumbnail: None,
                home: t.home,
                active: Some(t.id) == active,
            })
            .collect();
        self.services.open_tabs.set(tabs);
        for (id, uri) in &self.thumbs {
            self.services.open_tabs.set_thumbnail(*id, uri.clone());
        }
        // One capture every few seconds, and only when nothing else wants
        // the screenshot machinery.
        let now = ctx.input(|i| i.time);
        let due = self.thumb_at.is_none_or(|at| now - at > THUMB_EVERY);
        if due && self.snip.is_none() && self.thumb_wanted.is_none() && active.is_some() {
            self.thumb_at = Some(now);
            self.thumb_wanted = active;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    /// Take the picture asked for by `publish_tabs` out of the frame's
    /// events, shrink it and keep it for that tab.
    fn finish_thumb(&mut self, ctx: &egui::Context) {
        let Some(id) = self.thumb_wanted else {
            return;
        };
        // Snip wants the same event; let it have it and try again later.
        if self.snip.is_some() {
            return;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = shot else {
            return;
        };
        self.thumb_wanted = None;
        if self.main_rect == egui::Rect::NOTHING {
            return;
        }
        let Some(cropped) = crop_pixels(&image, self.main_rect, ctx.pixels_per_point()) else {
            return;
        };
        let Some(small) = shrink(&cropped, THUMB_WIDTH) else {
            return;
        };
        let uri = format!("bytes://hs-thumb/{id}-{}.png", self.thumb_at.unwrap_or(0.0));
        let mut png = Vec::new();
        let [w, h] = small.size;
        let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, small.as_raw().to_vec())
        else {
            return;
        };
        if image::DynamicImage::ImageRgba8(buf)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .is_err()
        {
            return;
        }
        // A fresh URI each time, so egui shows the new picture rather than
        // the cached old one. The previous bytes are dropped with the key.
        if let Some(old) = self.thumbs.insert(id, uri.clone()) {
            ctx.forget_image(&old);
        }
        ctx.include_bytes(uri, png);
    }

    /// Snip: ask for a screenshot; `finish_snip` crops and delivers it on
    /// the next frame. Mandatory tabs have nothing worth snipping but
    /// snip fine anyway.
    fn request_snip(&mut self, whole_window: bool) {
        self.snip = Some(whole_window);
    }

    fn finish_snip(&mut self, ctx: &egui::Context) {
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let got_shot = shot.is_some();
        if let Some(image) = shot
            && let Some(whole) = self.snip.take()
        {
            let ppp = ctx.pixels_per_point();
            let cropped = if whole || self.main_rect == egui::Rect::NOTHING {
                (*image).clone()
            } else {
                // Same clamping as a thumbnail: the rectangle can reach
                // the edge and epaint's own crop does not check.
                crop_pixels(&image, self.main_rect, ppp).unwrap_or_else(|| (*image).clone())
            };
            ctx.copy_image(cropped.clone());
            let s = self.services.clone();
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let title = self
                .tabs
                .active
                .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
                .map(|t| hs_core::storage::slug(&t.tool.title()))
                .unwrap_or_else(|| "hyperstack".into());
            let picked = rfd::FileDialog::new()
                .set_file_name(format!("{title}-{stamp}.png"))
                .add_filter("PNG image", &["png"])
                .save_file();
            match picked {
                Some(path) => match save_png(&cropped, &path) {
                    Ok(()) => self.toasts.push(
                        ToastLevel::Success,
                        s.i18n
                            .t_args("snip-saved", &[("path", &path.display().to_string())]),
                    ),
                    Err(e) => self.toasts.push(ToastLevel::Error, e),
                },
                None => self.toasts.push(ToastLevel::Info, s.t("snip-copied")),
            }
        } else if self.snip.is_some() && !got_shot {
            // Request once; eframe answers with Event::Screenshot next frame.
            if !ctx.memory(|m| {
                m.data
                    .get_temp::<bool>(egui::Id::new("hs-snip-asked"))
                    .unwrap_or(false)
            }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new("hs-snip-asked"), true));
            }
            ctx.request_repaint();
        }
        if self.snip.is_none() {
            ctx.memory_mut(|m| m.data.remove::<bool>(egui::Id::new("hs-snip-asked")));
        }
    }

    /// Print…: the active tab's HTML export, written under `tmp/` and opened
    /// in the browser (which prints to paper or PDF).
    fn print_active(&mut self) {
        let s = self.services.clone();
        let printable = self
            .tabs
            .active
            .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
            .and_then(|t| t.tool.printable());
        let Some(p) = printable else {
            self.toasts.push(ToastLevel::Info, s.t("print-nothing"));
            return;
        };
        let dark = self.theme.dark;
        let page = hs_core::html::document(&p.title, &p.subtitle, &p.html, dark);
        let dir = s.paths.tmp();
        let _ = std::fs::create_dir_all(&dir);
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let path = dir.join(format!(
            "print-{}-{stamp}.html",
            hs_core::storage::slug(&p.title)
        ));
        if let Err(e) = std::fs::write(&path, page) {
            self.toasts.push(ToastLevel::Error, e.to_string());
            return;
        }
        let url = format!("file:///{}", path.display().to_string().replace('\\', "/"));
        self.pending_open_url = Some(url);
        self.toasts.push(ToastLevel::Success, s.t("print-opened"));
    }

    /// Snip and Print menu in the title bar.
    fn snip_menu(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        let theme = self.theme.clone();
        let r = hs_ui::form::icon_menu(ui, &theme, hs_ui::icons::ph::CAMERA, 20.0, false, |ui| {
            ui.set_min_width(220.0);
            let km = s.shortcuts.read();
            let key = |a: &str| {
                km.get(a)
                    .map(|c| format!("  {}", c.display()))
                    .unwrap_or_default()
            };
            if ui
                .button(format!(
                    "{} {}{}",
                    hs_ui::icons::ph::CROP,
                    s.t("snip-tab"),
                    key("snip.tab")
                ))
                .clicked()
            {
                self.request_snip(false);
                ui.close();
            }
            if ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::APP_WINDOW,
                    s.t("snip-window")
                ))
                .clicked()
            {
                self.request_snip(true);
                ui.close();
            }
            ui.separator();
            if ui
                .button(format!(
                    "{} {}{}",
                    hs_ui::icons::ph::PRINTER,
                    s.t("print-tab"),
                    key("print.tab")
                ))
                .clicked()
            {
                self.print_active();
                ui.close();
            }
        });
        r.tip(s.t("snip-tip"));
    }

    fn notification_popup(&mut self, ctx: &egui::Context) {
        if !self.notifications_open {
            return;
        }
        let theme = self.theme.clone();
        let mut close = false;
        let mut clear = false;
        let just_opened = std::mem::take(&mut self.notifications_opened_this_frame);
        let response = egui::Area::new(egui::Id::new("hs-notifications"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-12.0, 48.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.panel_bg)
                    .stroke(egui::Stroke::new(1.0, theme.border))
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 3],
                        blur: 12,
                        spread: 0,
                        color: Color32::from_black_alpha(100),
                    })
                    .show(ui, |ui| {
                        ui.set_width(360.0);
                        ui.horizontal(|ui| {
                            ui.heading(self.services.t("notifications-title"));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button(hs_ui::icons::TAB_CLOSE).clicked() {
                                        close = true;
                                    }
                                    if ui
                                        .small_button(self.services.t("notifications-clear"))
                                        .clicked()
                                    {
                                        clear = true;
                                    }
                                },
                            );
                        });
                        ui.separator();
                        let history = &self.toasts.history;
                        if history.is_empty() {
                            ui.colored_label(
                                theme.text_muted,
                                self.services.t("notifications-none"),
                            );
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(360.0)
                                .show(ui, |ui| {
                                    for item in history.iter().rev().take(100) {
                                        let (icon, colour) = match item.level {
                                            ToastLevel::Info => {
                                                (hs_ui::icons::INFO, theme.secondary)
                                            }
                                            ToastLevel::Success => {
                                                (hs_ui::icons::CHECK, theme.success)
                                            }
                                            ToastLevel::Warning => {
                                                (hs_ui::icons::WARNING, theme.warning)
                                            }
                                            ToastLevel::Error => (hs_ui::icons::ERROR, theme.error),
                                        };
                                        ui.horizontal_top(|ui| {
                                            ui.colored_label(colour, icon);
                                            ui.vertical(|ui| {
                                                ui.add(egui::Label::new(&item.text).wrap());
                                                let elapsed =
                                                    item.at.elapsed().as_secs().to_string();
                                                let relative = self.services.i18n.t_args(
                                                    "notifications-seconds-ago",
                                                    &[("n", &elapsed)],
                                                );
                                                ui.label(
                                                    egui::RichText::new(relative)
                                                        .small()
                                                        .color(theme.text_muted),
                                                );
                                            });
                                        });
                                        ui.separator();
                                    }
                                });
                        }
                    });
            });
        if notification_popup_should_close(just_opened, response.response.clicked_elsewhere()) {
            close = true;
        }
        if clear {
            self.toasts.clear();
        }
        if close {
            self.notifications_open = false;
        }
    }
}

impl eframe::App for HyperStackApp {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
        if !self.first_frame_done {
            self.first_frame_done = true;
            if let Some(st) = self.services.updater.mark_started_ok() {
                self.toasts.push(
                    ToastLevel::Success,
                    self.services
                        .i18n
                        .t_args("update-toast-installed", &[("version", &st.installed)]),
                );
            } else if let Some(st) = self.services.updater.failed_install() {
                self.toasts.push(
                    ToastLevel::Warning,
                    self.services
                        .i18n
                        .t_args("update-toast-failed-install", &[("version", &st.installed)]),
                );
            }
        }
        self.poll_updater(ctx);
        self.finish_snip(ctx);
        // System mode follows the OS: when its preference flips, our own
        // palette (tabs, sidebar, terminal colours) has to flip with egui's.
        if hs_ui::theme::sync_system(ctx)
            && self.services.settings.read().appearance.mode == "system"
        {
            let appearance = self.services.settings.read().appearance.clone();
            self.theme = Theme::from_settings(&appearance);
            hs_ui::theme::install(ctx, &appearance);
            self.services.events.publish(Event::ThemeChanged);
        }
        // Idle tracking for the vault relock.
        if ctx.input(|i| !i.events.is_empty() || i.pointer.any_down()) {
            self.last_input = Instant::now();
        }
        // A copied secret does not sit on the clipboard for ever.
        if let Some(at) = self.clear_clipboard_at
            && Instant::now() >= at
        {
            ctx.copy_text(String::new());
            self.clear_clipboard_at = None;
        }
        if self.clear_clipboard_at.is_some() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        let relock = self.services.settings.read().security.relock_after_minutes;
        if relock > 0
            && !self.services.secrets.is_locked()
            && self.last_input.elapsed() > Duration::from_secs(u64::from(relock) * 60)
        {
            self.run_action("vault.lock");
        }

        // Events published since the last frame go to every tab.
        let events = self.services.events.drain();
        if !events.is_empty() {
            let env = self.services.environments.read().active.clone();
            let services = self.services.clone();
            let mut actions = Vec::new();
            for t in &mut self.tabs.tabs {
                let mut cx = ToolContext {
                    services: &services,
                    tab_id: t.id,
                    env_id: env.as_deref(),
                    actions: &mut actions,
                    is_home: false,
                };
                for e in &events {
                    t.tool.on_event(e, &mut cx);
                }
            }
            self.pending_actions.extend(actions);
            if events.iter().any(|e| matches!(e, Event::SettingsChanged)) {
                let zoom = self.services.settings.read().appearance.ui_zoom;
                ctx.set_zoom_factor(zoom);
            }
        }

        // Global shortcuts (not while typing in the palette).
        if !self.palette.open {
            let triggered = ctx.input(|i| {
                self.services
                    .shortcuts
                    .read()
                    .triggered(i)
                    .map(str::to_string)
            });
            if let Some(a) = triggered {
                self.run_action(&a);
            }
        }

        egui::Panel::top("hs-top")
            .exact_size(46.0)
            .resizable(false)
            .show(root, |ui| self.top_bar(ui));
        egui::Panel::top("hs-tabs")
            .resizable(false)
            .show(root, |ui| {
                ui.add_space(4.0);
                self.tab_strip(ui);
            });
        egui::Panel::top("hs-banner")
            .resizable(false)
            .show(root, |ui| {
                self.env_banner(ui);
                self.toolbar(ui);
            });
        let rail_w = if self.services.settings.read().appearance.rail_labels {
            RAIL_WIDTH_LABELS
        } else {
            RAIL_WIDTH
        };
        egui::Panel::left("hs-rail")
            .exact_size(rail_w)
            .resizable(false)
            .show(root, |ui| self.rail(ui));
        if self.sidebar_open {
            let w = self.services.settings.read().appearance.sidebar_width;
            let r = egui::Panel::left("hs-sidebar")
                .default_size(w)
                .size_range(180.0..=640.0)
                .resizable(true)
                .show(root, |ui| self.sidebar(ui));
            let _ = &r;
            let width =
                egui::containers::panel::PanelState::load(root.ctx(), egui::Id::new("hs-sidebar"))
                    .map(|st| st.outer_rect.width().round())
                    .unwrap_or(w);
            if (width - w).abs() >= 1.0 && width >= 180.0 {
                self.services.settings.write().appearance.sidebar_width = width;
                self.layout_dirty_since.get_or_insert(Instant::now());
                self.tabs.dirty = true;
            }
        }
        egui::Panel::bottom("hs-status")
            .exact_size(32.0)
            .resizable(false)
            .show(root, |ui| self.status_bar(ui));
        if self.dock_open {
            let h = self.services.settings.read().appearance.dock_height;
            let logs = self.logging.buffer.clone();
            let r = egui::Panel::bottom("hs-dock")
                .default_size(h)
                .size_range(100.0..=600.0)
                .resizable(true)
                .show(root, |ui| {
                    dock::show(ui, &self.theme, &self.services, &logs, &mut self.dock);
                });
            if let Some(link) = self.dock.pending_link.take() {
                self.open_terminal_link(link, root.ctx());
            }
            let _ = &r;
            let height =
                egui::containers::panel::PanelState::load(root.ctx(), egui::Id::new("hs-dock"))
                    .map(|st| st.outer_rect.height().round())
                    .unwrap_or(h);
            if (height - h).abs() >= 1.0 && height >= 100.0 {
                self.services.settings.write().appearance.dock_height = height;
                self.layout_dirty_since.get_or_insert(Instant::now());
                self.tabs.dirty = true;
            }
        }
        {
            // What only the shell knows and the Home page shows.
            let mut hub = self.home.lock();
            hub.closed = self
                .tabs
                .closed
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.factory_id.clone(), c.title.clone()))
                .collect();
            hub.workspaces = self.registry.names();
        }
        egui::CentralPanel::default().show(root, |ui| self.main_area(ui));
        let home_requests = std::mem::take(&mut self.home.lock().requests);
        for r in home_requests {
            self.handle_home_request(r);
        }
        self.portable_dialogs(ctx);
        if let Some(url) = self.pending_open_url.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        self.finish_thumb(ctx);
        self.publish_tabs(ctx);
        // Native web views sit above egui: hide them under any modal,
        // including one a tool opened for itself.
        let tool_modal = self.browser_hub.lock().tabs.values().any(|t| t.overlay);
        let overlay = tool_modal
            || self.palette.open
            || self.vault_dialog != VaultDialog::None
            || !matches!(self.workspace_dialog, WorkspaceDialog::None)
            || self.portable != PortableDialog::None
            || self.confirm_close.is_some()
            || self.confirm_revert
            || self.notifications_open
            || self.fallback_notice_open
            || self.settings_ui.lock().state.confirm_delete.is_some();
        let (blocklist, devtools_panel) = {
            let st = self.services.settings.read();
            (st.browser.blocklist.clone(), st.browser.devtools_panel)
        };
        self.browser_host.set_devtools_panel(devtools_panel);
        self.browser_host
            .sync(frame, ctx, &self.browser_hub, overlay, &blocklist);

        // Overlays.
        let labels = PaletteLabels {
            placeholder_commands: self.services.t("palette-placeholder"),
            placeholder_quick_open: self.services.t("quick-open-placeholder"),
            no_results: self.services.t("palette-no-results"),
        };
        if self.palette.open {
            let items = self.palette_items();
            if let Some(chosen) =
                palette::show(ctx, &self.theme, &mut self.palette, &items, &labels)
            {
                if let Some(c) = chosen.strip_prefix("conn:") {
                    let kind = self
                        .services
                        .connections
                        .read()
                        .by_id(c)
                        .map(|p| p.kind.clone());
                    match kind.as_deref().and_then(launcher::tool_for_kind) {
                        Some(tool) => self
                            .open_tool(tool, Some(OpenRequest::Connection { id: c.to_string() })),
                        None => self.toasts.push(
                            ToastLevel::Info,
                            format!("Connection {c}: this tool arrives in a later milestone"),
                        ),
                    }
                } else {
                    self.run_action(&chosen);
                }
            }
        }
        let effects = std::mem::take(&mut self.settings_ui.lock().effects);
        self.apply_settings_effects(effects, ctx);
        let confirm_delete = self.settings_ui.lock().state.confirm_delete.clone();
        if let Some((id, name, is_env)) = confirm_delete {
            let s = self.services.clone();
            let body = if is_env {
                s.i18n.t_args("env-delete-confirm", &[("name", &name)])
            } else {
                s.i18n.t_args("conn-delete-confirm", &[("name", &name)])
            };
            match dialog::confirm(
                ctx,
                &self.theme,
                "delete",
                &s.t("delete"),
                &body,
                &s.t("delete"),
                &s.t("cancel"),
                true,
            ) {
                DialogResult::Ok => {
                    let res = if is_env {
                        s.environments.write().remove(&id)
                    } else {
                        s.connections.write().remove(&id)
                    };
                    match res {
                        Ok(()) => {
                            let mut hub = self.settings_ui.lock();
                            if is_env {
                                hub.state.env_draft = None;
                                hub.state.env_selected = None;
                            } else {
                                hub.state.conn_draft = None;
                                hub.state.conn_selected = None;
                            }
                        }
                        Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                    }
                    self.settings_ui.lock().state.confirm_delete = None;
                }
                DialogResult::Cancel => self.settings_ui.lock().state.confirm_delete = None,
                DialogResult::Open => {}
            }
        }
        if let Some(id) = self.confirm_close {
            let s = self.services.clone();
            let title = self
                .tabs
                .tabs
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.tool.title().to_string())
                .unwrap_or_default();
            let body = s.i18n.t_args("tab-unsaved-body", &[("title", &title)]);
            match dialog::confirm(
                ctx,
                &self.theme,
                "close-tab",
                &s.t("tab-unsaved-title"),
                &body,
                &s.t("tab-unsaved-discard"),
                &s.t("tab-unsaved-keep"),
                true,
            ) {
                DialogResult::Ok => {
                    self.tabs.close(id);
                    self.confirm_close = None;
                }
                DialogResult::Cancel => self.confirm_close = None,
                DialogResult::Open => {}
            }
        }
        if self.confirm_revert {
            let s = self.services.clone();
            let prev = s.updater.previous_exe().map(|(_, v)| v).unwrap_or_default();
            let body = s
                .i18n
                .t_args("settings-updates-revert-confirm", &[("version", &prev)]);
            let ok_label = s
                .i18n
                .t_args("settings-updates-revert", &[("version", &prev)]);
            match dialog::confirm(
                ctx,
                &self.theme,
                "revert",
                &s.t("settings-updates-previous"),
                &body,
                &ok_label,
                &s.t("cancel"),
                true,
            ) {
                DialogResult::Ok => {
                    self.confirm_revert = false;
                    self.save_layout();
                    match s.updater.revert() {
                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.toasts.push(ToastLevel::Error, e),
                    }
                }
                DialogResult::Cancel => self.confirm_revert = false,
                DialogResult::Open => {}
            }
        }
        if self.fallback_notice_open {
            let s = self.services.clone();
            let body = s.i18n.t_args(
                "fallback-body",
                &[("path", &s.paths.root().display().to_string())],
            );
            if dialog::confirm(
                ctx,
                &self.theme,
                "fallback",
                &s.t("fallback-title"),
                &body,
                &s.t("ok"),
                &s.t("close"),
                false,
            ) != DialogResult::Open
            {
                self.fallback_notice_open = false;
                self.services.settings.write().general.show_fallback_notice = false;
                let _ = self.services.save_settings();
            }
        }
        self.vault_dialogs(ctx);
        self.workspace_dialogs(ctx);
        self.handle_pending_actions();
        self.notification_popup(ctx);
        self.toasts.show(ctx, &self.theme);

        // Layout persistence: debounced, and on close.
        if self.tabs.dirty && self.layout_dirty_since.is_none() {
            self.layout_dirty_since = Some(Instant::now());
        }
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested
            || self
                .layout_dirty_since
                .is_some_and(|t| t.elapsed() > Duration::from_secs(1))
        {
            self.save_layout();
            self.save_window_state(ctx);
        }
        // Tool inputs change without touching the strip (a devtools field,
        // an API draft): every few seconds compare the saved states with
        // what was last written, so a crash or a kill loses little.
        if self.layout_dirty_since.is_none()
            && self.last_layout_save.elapsed() > Duration::from_secs(5)
        {
            let layout = self
                .tabs
                .to_layout(self.services.environments.read().active.clone());
            if layout_fingerprint(&layout) != self.layout_fingerprint {
                self.save_layout();
            } else {
                self.last_layout_save = Instant::now();
            }
        }
        // Window moves/resizes do not dirty the layout; poll them once a second.
        if self.window_poll.elapsed() > Duration::from_secs(1) {
            self.window_poll = Instant::now();
            if let Some(st) =
                ctx.input(|i| crate::window_state::WindowState::from_viewport(i.viewport()))
                && st != self.window_state
            {
                st.save(&self.services.paths.root().join("window.toml"));
                self.window_state = st;
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }
        if self.tabs.dirty {
            ctx.request_repaint_after(Duration::from_millis(1100));
        }
        if self.services.jobs.running_count() > 0 {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}

/// The three font choices from settings (interface, terminal, editor).
fn font_choice(s: &Services) -> hs_ui::fonts::FontChoice {
    let st = s.settings.read();
    hs_ui::fonts::FontChoice {
        ui: st.appearance.ui_font.clone(),
        terminal: st.terminal.font_family.clone(),
        editor: st.editor.font_family.clone(),
    }
}

/// Comparable form of a file URI/path: no `file://`, one separator style,
/// no trailing separator, case-folded on Windows.
fn normalize_uri(uri: &str) -> String {
    let p = uri.strip_prefix("file://").unwrap_or(uri);
    let p = p.replace('\\', "/");
    let p = p.trim_end_matches('/');
    if cfg!(windows) {
        p.to_ascii_lowercase()
    } else {
        p.to_string()
    }
}

fn notification_popup_should_close(just_opened: bool, clicked_elsewhere: bool) -> bool {
    !just_opened && clicked_elsewhere
}

/// Tabs every workspace always has, pinned, in this strip order: they never
/// open twice and cannot be closed or unpinned.
const MANDATORY: &[&str] = &["home", "settings"];

pub(crate) fn is_mandatory(factory_id: &str) -> bool {
    MANDATORY.contains(&factory_id)
}

/// The rail group a tool's tabs show in.
fn group_of(factory_id: &str) -> &str {
    factory_id
}

/// Whether this activity's window strip offers an add-tab button.
///
/// Not every activity: a tab is opened from the sidebar in the ones where
/// a tab *is* a connection, a shell or a saved page, and a + there made an
/// untitled tab nobody asked for. Where a tab is a document the user
/// creates — the editor's files, an API request, a Graphics image, a
/// scratchpad note — the button stays. Owner's requirement, 2026-10-17.
fn shows_new_tab(group: Option<&str>) -> bool {
    !matches!(
        group,
        Some(
            "terminal" | "ssh" | "db" | "nosql" | "redis" | "s3" | "browser" | "home" | "devtools"
        )
    )
}

/// Write an egui image as PNG.
fn save_png(img: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    let [w, h] = img.size;
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, img.as_raw().to_vec())
        .ok_or_else(|| "image buffer size mismatch".to_string())?;
    buf.save(path).map_err(|e| e.to_string())
}

/// How often a thumbnail of the active tab is taken. A screenshot is not
/// free, and a dashboard is read, not watched.
const THUMB_EVERY: f64 = 3.0;
/// How wide a thumbnail is kept. Enough to recognise a pane, small enough
/// that a dozen of them cost nothing.
const THUMB_WIDTH: usize = 320;

/// Cut a rectangle of points out of a captured image, clamped to what the
/// image actually holds.
///
/// `ColorImage::region` converts points to pixels with its own rounding
/// and then indexes without checking, so a rectangle that reaches the edge
/// can run a row past the buffer and panic - which it did, the first time
/// a thumbnail was taken (`range end index ... out of range`). Doing the
/// conversion here means the bounds are checked before anything is read.
fn crop_pixels(img: &egui::ColorImage, rect: egui::Rect, ppp: f32) -> Option<egui::ColorImage> {
    let [w, h] = img.size;
    let px = |v: f32| (v * ppp).round().max(0.0) as usize;
    let x0 = px(rect.min.x).min(w);
    let y0 = px(rect.min.y).min(h);
    let x1 = px(rect.max.x).min(w);
    let y1 = px(rect.max.y).min(h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity(cw * ch);
    for y in y0..y1 {
        let row = y * w;
        out.extend_from_slice(&img.pixels[row + x0..row + x1]);
    }
    Some(egui::ColorImage {
        size: [cw, ch],
        pixels: out,
        source_size: egui::Vec2::new(cw as f32, ch as f32),
    })
}

/// Shrink an image to `width`, keeping its shape. `None` when there is
/// nothing to shrink.
fn shrink(img: &egui::ColorImage, width: usize) -> Option<egui::ColorImage> {
    let [w, h] = img.size;
    if w == 0 || h == 0 {
        return None;
    }
    if w <= width {
        return Some(img.clone());
    }
    let height = (h * width / w).max(1);
    let buf = image::RgbaImage::from_raw(w as u32, h as u32, img.as_raw().to_vec())?;
    let small = image::imageops::resize(
        &buf,
        width as u32,
        height as u32,
        image::imageops::FilterType::Triangle,
    );
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [width, height],
        small.as_raw(),
    ))
}

const RAIL_WIDTH: f32 = 56.0;
const RAIL_WIDTH_LABELS: f32 = 78.0;

/// Tool ids in rail order: the saved order first (ids it names that still
/// exist), then every other tool in its default order. Home and Settings
/// are placed by the rail itself, so they are left out here.
/// How many tabs one connection may occupy in a tool.
///
/// SSH is the exception the owner asked for: a host often wants more than
/// one shell — a build running in one, a log followed in another — so it
/// gets up to three, each its own session. Everywhere else a second tab on
/// the same connection is a duplicate nobody asked for, and clicking the
/// connection again should land on the tab that is already there.
pub(crate) fn tabs_per_connection(factory_id: &str) -> usize {
    match group_of(factory_id) {
        "ssh" => 3,
        _ => 1,
    }
}

pub(crate) fn rail_order<'a>(defaults: &[&'a str], saved: &[String]) -> Vec<&'a str> {
    let mut out: Vec<&str> = saved
        .iter()
        .filter_map(|id| defaults.iter().copied().find(|d| *d == id))
        .filter(|id| !is_mandatory(id))
        .collect();
    for d in defaults {
        if !is_mandatory(d) && !out.contains(d) {
            out.push(d);
        }
    }
    out
}

/// One rail entry: a 24 px icon, optionally with the tool's name under it.
/// What `save_layout` would write for the tabs, hashed: the probe for
/// tool state that changed without a strip change.
fn layout_fingerprint(layout: &hs_core::workspace::Layout) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(&layout.tabs)
        .unwrap_or_default()
        .hash(&mut h);
    layout.active.hash(&mut h);
    h.finish()
}

fn rail_item(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &str,
    label: &str,
    active: bool,
    show_label: bool,
) -> egui::Response {
    if !show_label {
        return hs_ui::form::icon_button(ui, theme, icon, 24.0, active);
    }
    let size = egui::vec2(RAIL_WIDTH_LABELS - 8.0, 50.0);
    let (rect, r) = ui.allocate_exact_size(size, egui::Sense::click());
    let fill = if active {
        theme.accent.linear_multiply(0.30)
    } else if r.hovered() {
        theme.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(6), fill);
    }
    let col = if active || r.hovered() {
        theme.accent
    } else {
        theme.text_muted
    };
    ui.painter().text(
        egui::pos2(rect.center().x, rect.top() + 17.0),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(22.0),
        col,
    );
    let galley = ui.painter().layout(
        label.to_string(),
        egui::FontId::proportional(10.0),
        if active { theme.text } else { theme.text_muted },
        size.x - 4.0,
    );
    let text_rect = egui::Align2::CENTER_TOP.anchor_size(
        egui::pos2(rect.center().x, rect.top() + 31.0),
        galley.size(),
    );
    ui.painter().galley(text_rect.min, galley, theme.text);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, active, label));
    r.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::{notification_popup_should_close, rail_order};

    #[test]
    fn the_add_tab_button_is_hidden_only_where_a_tab_is_not_a_document() {
        use super::shows_new_tab;
        // A tab here is a connection, a shell or the workspace's browser,
        // opened from the sidebar.
        for g in [
            "terminal", "ssh", "db", "nosql", "redis", "s3", "browser", "home", "devtools",
        ] {
            assert!(
                !shows_new_tab(Some(g)),
                "{g} should hide the add-tab button"
            );
        }
        // A tab here is a document somebody makes.
        for g in ["editor", "api", "graphics", "scratchpad", "settings"] {
            assert!(shows_new_tab(Some(g)), "{g} should keep the add-tab button");
        }
    }

    #[test]
    fn rail_order_keeps_saved_order_then_defaults_and_skips_mandatory() {
        let defaults = ["home", "editor", "terminal", "ssh", "settings"];
        let saved = vec!["ssh".to_string(), "gone".to_string(), "home".to_string()];
        assert_eq!(
            rail_order(&defaults, &saved),
            vec!["ssh", "editor", "terminal"]
        );
        assert_eq!(
            rail_order(&defaults, &[]),
            vec!["editor", "terminal", "ssh"]
        );
    }

    #[test]
    fn notification_popup_ignores_its_opening_click_but_dismisses_outside_clicks() {
        assert!(!notification_popup_should_close(true, true));
        assert!(notification_popup_should_close(false, true));
        assert!(!notification_popup_should_close(false, false));
    }
}
