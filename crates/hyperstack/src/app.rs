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
/// PNG/SVG loaders (keyed by the path so it is cached after the first frame).
/// Returns `None` when the file is missing or unreadable.
fn logo_image(path: &std::path::Path) -> Option<egui::Image<'static>> {
    let bytes = std::fs::read(path).ok()?;
    let uri = path.display().to_string().replace('\\', "/");
    Some(egui::Image::from_bytes(uri, bytes))
}

/// The workspace logo image for a workspace folder, if it has one.
fn ws_logo_image(root: &std::path::Path) -> Option<egui::Image<'static>> {
    logo_image(&hs_core::workspace::workspace_icon_file(root)?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VaultDialog {
    None,
    Create,
    Unlock,
    ChangeMaster,
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
    last_layout_save: Instant,
    layout_dirty_since: Option<Instant>,
    pending_actions: Vec<Action>,
    fallback_notice_open: bool,
    first_frame_done: bool,
    confirm_revert: bool,
    workspace_dialog: WorkspaceDialog,
    update_badge: Option<String>,
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
        let theme = Theme::by_name(&services.settings.read().appearance.theme);
        theme.apply(&cc.egui_ctx);
        hs_ui::fonts::install(&cc.egui_ctx, &font_choice(&services));
        hs_ui::theme::apply_font_size(
            &cc.egui_ctx,
            services.settings.read().appearance.ui_font_size,
        );
        cc.egui_ctx
            .set_zoom_factor(services.settings.read().appearance.ui_zoom);

        let settings_hub = SharedSettings::default();
        let factories = crate::tools::factories(settings_hub.clone());
        let mut app = Self {
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
            last_layout_save: Instant::now(),
            layout_dirty_since: None,
            pending_actions: Vec::new(),
            first_frame_done: false,
            confirm_revert: false,
            workspace_dialog: WorkspaceDialog::None,
            update_badge: None,
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
        let layout = self.services.workspace.read().load_layout();
        let restore_tabs = self.services.settings.read().general.restore_tabs;
        if restore_tabs {
            let map: HashMap<String, &dyn ToolFactory> = self
                .factories
                .iter()
                .map(|f| (f.descriptor().id.to_string(), f.as_ref()))
                .collect();
            self.tabs.restore(&layout, &map, &self.services);
        }
        self.sidebar_open = layout.sidebar_open || layout.tabs.is_empty();
        self.dock_open = layout.dock_open;
        if let Some(env) = layout.active_environment {
            self.services.environments.write().active = Some(env);
        }
        if self.tabs.tabs.is_empty()
            && let Some(i) = self.factory_index("hello")
        {
            self.tabs
                .open(self.factories[i].as_ref(), &self.services, None);
        }
        // Only one Settings / Welcome tab survives a restore.
        for fid in SINGLE_INSTANCE {
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
        }
        self.group = self
            .active_factory()
            .as_deref()
            .map(|f| group_of(f).to_string());
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
        layout.sidebar_open = self.sidebar_open;
        layout.dock_open = self.dock_open;
        if let Err(e) = self.services.workspace.read().save_layout(&layout) {
            tracing::warn!("layout not saved: {e}");
        }
        self.tabs.dirty = false;
        self.layout_dirty_since = None;
        self.last_layout_save = Instant::now();
    }

    /// Settings live in one tab: focus it if open, else open it; optionally jump to a page.
    fn open_settings(&mut self, page: Option<settings_ui::Page>) {
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

    fn open_tool(&mut self, factory_id: &str, request: Option<OpenRequest>) {
        // Settings and Welcome are single tabs: a second request focuses the existing one.
        if SINGLE_INSTANCE.contains(&factory_id)
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
        // One tab per file: a request for something a tab of this tool already
        // shows focuses that tab (and lets it jump to the requested line).
        if let Some(OpenRequest::File { uri, .. }) = &request {
            let want = normalize_uri(uri);
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
                .map(|t| t.id);
            if let Some(id) = existing {
                self.group = Some(factory_id.to_string());
                self.tabs.activate(id);
                let mut actions = Vec::new();
                let env = self.services.environments.read().active.clone();
                if let (Some(t), Some(req)) = (self.tabs.get_mut(id), request) {
                    let mut cx = ToolContext {
                        services: &self.services,
                        tab_id: id,
                        env_id: env.as_deref(),
                        actions: &mut actions,
                    };
                    t.tool.open(req, &mut cx);
                }
                self.pending_actions.extend(actions);
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
                };
                t.tool.open(req, &mut cx);
            }
            self.pending_actions.extend(actions);
        }
    }

    fn open_launcher_tab(&mut self) {
        // The launcher is the "no active tab" state: clear the active tab and focus search.
        self.tabs.active = None;
        self.launcher.focus_search = true;
        self.launcher.query.clear();
    }

    fn close_tab(&mut self, id: u64, force: bool) {
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
                    let (name, zoom) = {
                        let s = self.services.settings.read();
                        (s.appearance.theme.clone(), s.appearance.ui_zoom)
                    };
                    self.theme = Theme::by_name(&name);
                    self.theme.apply(ctx);
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
                SettingsEffect::Toast(level, text) => self.toasts.push(level, text),
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
        let switch_away = self.services.workspace.read().meta.name == name;
        if switch_away {
            self.save_layout();
            let _ = bootstrap::switch_workspace(&self.services, &mut self.registry, "default");
            self.tabs = TabSpace::new();
            self.restore_layout();
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
                    ui.label(
                        egui::RichText::new(s.t("app-name"))
                            .strong()
                            .size(16.0)
                            .color(self.theme.accent),
                    );
                    ui.add_space(8.0);
                    // Workspace switcher
                    let ws_name = s.workspace.read().meta.name.clone();
                    let ws_root = s.workspace.read().root().to_path_buf();
                    if let Some(img) = ws_logo_image(&ws_root) {
                        ui.add(img.fit_to_exact_size(egui::vec2(18.0, 18.0)));
                    }
                    egui::ComboBox::from_id_salt("ws")
                        .selected_text(format!("{} {}", hs_ui::icons::WORKSPACE, ws_name))
                        .show_ui(ui, |ui| {
                            let rows: Vec<(String, std::path::PathBuf)> = self
                                .registry
                                .workspaces
                                .iter()
                                .map(|w| (w.name.clone(), s.paths.resolve_path(&w.path)))
                                .collect();
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
                                    if n != "default" {
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
                        });
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
                        if hs_ui::form::icon_button(ui, &theme, hs_ui::icons::BELL, 20.0, false)
                            .tip(s.t("notifications"))
                            .clicked()
                        {
                            self.dock_open = true;
                            self.dock.tab = dock::DockTab::Output;
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
            split_right: s.t("tab-split-right"),
            split_down: s.t("tab-split-down"),
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
                TabMenuAction::Pin => self.tabs.set_pinned(id, true),
                TabMenuAction::Unpin => self.tabs.set_pinned(id, false),
                TabMenuAction::Duplicate => {
                    let dup = self
                        .tabs
                        .get_mut(id)
                        .map(|t| (t.factory_id.clone(), t.tool.save_state()));
                    if let Some((fid, state)) = dup
                        && !SINGLE_INSTANCE.contains(&fid.as_str())
                        && let Some(i) = self.factory_index(&fid)
                    {
                        self.tabs
                            .open(self.factories[i].as_ref(), &self.services, Some(state));
                    }
                }
                TabMenuAction::SplitRight | TabMenuAction::SplitDown => {
                    let horizontal = action == TabMenuAction::SplitRight;
                    if let Some(i) = self.factory_index("scratchpad") {
                        let new_id =
                            self.tabs
                                .open(self.factories[i].as_ref(), &self.services, None);
                        self.tabs.split_with(id, new_id, horizontal);
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
                        };
                        t.tool.toolbar(ui, &mut cx);
                    });
                });
        }
        self.pending_actions.extend(actions);
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            let theme = self.theme.clone();
            if hs_ui::form::icon_button(ui, &theme, hs_ui::icons::SIDEBAR, 20.0, self.sidebar_open)
                .tip(s.t("sidebar-toggle"))
                .clicked()
            {
                self.sidebar_open = !self.sidebar_open;
            }
            ui.add_space(6.0);
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
            for (id, icon, title) in descs {
                if id == "hello" {
                    continue; // Welcome shares the Settings group
                }
                let active = self.group.as_deref() == Some(group_of(id));
                let count = self.tabs.tabs.iter().filter(|t| t.factory_id == id).count();
                let title = if count > 0 {
                    format!("{title} ({count})")
                } else {
                    title
                };
                if hs_ui::form::icon_button(ui, &theme, icon, 24.0, active)
                    .tip(title)
                    .clicked()
                {
                    self.select_group(id);
                }
                ui.add_space(4.0);
            }
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
            let mut cx = ToolContext {
                services: &services,
                tab_id: a,
                env_id: env.as_deref(),
                actions: &mut actions,
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
            let r = launcher::show(
                ui,
                &self.theme,
                &self.services,
                &mut self.launcher,
                &descs,
                &closed,
            );
            if let Some(id) = r.open_tool {
                self.open_tool(&id, None);
            }
            if let Some(i) = r.reopen_closed {
                self.reopen_closed(Some(i));
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
                        let mut cx = ToolContext {
                            services: &services,
                            tab_id: id,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                        };
                        t.tool.ui(ui, &mut cx);
                    }
                });
            });
            self.tabs.split = Some(split);
        } else if let Some(a) = self.tabs.active
            && let Some(t) = self.tabs.get_mut(a)
        {
            let mut cx = ToolContext {
                services: &services,
                tab_id: a,
                env_id: env.as_deref(),
                actions: &mut actions,
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
                        ui.label(muted(s.i18n.t_args(
                            "tab-count",
                            &[("n", &self.tabs.tabs.len().to_string())],
                        )));
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
}

impl eframe::App for HyperStackApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
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
        // Idle tracking for the vault relock.
        if ctx.input(|i| !i.events.is_empty() || i.pointer.any_down()) {
            self.last_input = Instant::now();
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
        egui::Panel::left("hs-rail")
            .exact_size(56.0)
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
        egui::CentralPanel::default().show(root, |ui| self.main_area(ui));

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
                    match kind.as_deref() {
                        Some("ssh") => self
                            .open_tool("ssh", Some(OpenRequest::Connection { id: c.to_string() })),
                        _ => self.toasts.push(
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

/// Tools that never open twice: a second request focuses the existing tab.
const SINGLE_INSTANCE: &[&str] = &["settings", "hello"];

/// The rail group a tool's tabs show in (Welcome shares the Settings group).
fn group_of(factory_id: &str) -> &str {
    match factory_id {
        "hello" => "settings",
        other => other,
    }
}
