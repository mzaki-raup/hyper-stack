//! The application shell (docs/04 §1): top bar, tab strip, environment banner,
//! activity rail + sidebar, main area (tab space with splits), bottom dock,
//! status bar, palette, settings, toasts and the vault dialogs.

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
use crate::settings_ui::{self, SettingsEffect, SettingsState};
use crate::tabspace::TabSpace;

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
    launcher: LauncherState,
    palette: PaletteState,
    toasts: Toasts,
    settings_ui: SettingsState,
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
}

impl HyperStackApp {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: DataDir, logging: Logging) -> Self {
        hs_ui::icons::install_fonts(&cc.egui_ctx);
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
        cc.egui_ctx
            .set_zoom_factor(services.settings.read().appearance.ui_zoom);

        let factories = crate::tools::factories();
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
            launcher: LauncherState::default(),
            palette: PaletteState::default(),
            toasts: Toasts::default(),
            settings_ui: SettingsState::default(),
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
        app
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

    fn open_tool(&mut self, factory_id: &str, request: Option<OpenRequest>) {
        let Some(i) = self.factory_index(factory_id) else {
            self.toasts
                .push(ToastLevel::Error, format!("No tool '{factory_id}'"));
            return;
        };
        let id = self
            .tabs
            .open(self.factories[i].as_ref(), &self.services, None);
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
            "tab.next" => self.tabs.next(1),
            "tab.prev" => self.tabs.next(-1),
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
            "view.dock" | "view.terminal" => self.dock_open = !self.dock_open,
            "app.settings" => self.settings_ui.open = true,
            "app.shortcuts" => {
                self.settings_ui.open = true;
                self.settings_ui.page = settings_ui::Page::Shortcuts;
            }
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
                } else if let Some(cmd) = other.strip_prefix("tool:")
                    && let Some(a) = self.tabs.active
                {
                    let env = self.services.environments.read().active.clone();
                    let mut actions = Vec::new();
                    if let Some(t) = self.tabs.get_mut(a) {
                        let mut cx = ToolContext {
                            services: &self.services,
                            tab_id: a,
                            env_id: env.as_deref(),
                            actions: &mut actions,
                        };
                        t.tool.run_command(cmd, &mut cx);
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
                        subtitle: Some(c.kind.clone()),
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
                    ctx.set_zoom_factor(zoom);
                    self.services.events.publish(Event::ThemeChanged);
                    let _ = self.services.save_settings();
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
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(s.t("app-name"))
                    .strong()
                    .color(self.theme.accent),
            );
            ui.add_space(12.0);
            // Workspace switcher
            let ws_name = s.workspace.read().meta.name.clone();
            egui::ComboBox::from_id_salt("ws")
                .selected_text(format!("{} {}", hs_ui::icons::WORKSPACE, ws_name))
                .show_ui(ui, |ui| {
                    let names: Vec<String> = self
                        .registry
                        .workspaces
                        .iter()
                        .map(|w| w.name.clone())
                        .collect();
                    for n in names {
                        if ui.selectable_label(n == ws_name, &n).clicked() && n != ws_name {
                            self.pending_actions.push(Action::RunCommand {
                                id: format!("ws:{n}"),
                            });
                        }
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
                        .selectable_label(s.environments.read().active.is_none(), s.t("env-none"))
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
                        .map(|e| (e.id.clone(), e.name.clone(), e.colour.clone(), e.protected))
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
                if ui
                    .add(egui::Button::new(hs_ui::icons::SETTINGS).frame(false))
                    .on_hover_text(s.t("settings"))
                    .clicked()
                {
                    self.settings_ui.open = true;
                }
                let lock_icon = if s.secrets.is_locked() {
                    hs_ui::icons::LOCK
                } else {
                    hs_ui::icons::UNLOCK
                };
                let lock_col = if s.secrets.is_locked() {
                    self.theme.warning
                } else {
                    self.theme.success
                };
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(lock_icon).color(lock_col))
                            .frame(false),
                    )
                    .on_hover_text(s.t("settings-vault-status"))
                    .clicked()
                {
                    self.pending_actions.push(Action::RunCommand {
                        id: if s.secrets.is_locked() {
                            "vault.unlock".into()
                        } else {
                            "vault.lock".into()
                        },
                    });
                }
                if ui
                    .add(egui::Button::new(hs_ui::icons::BELL).frame(false))
                    .on_hover_text(s.t("notifications"))
                    .clicked()
                {
                    self.dock_open = true;
                    self.dock.tab = dock::DockTab::Output;
                }
                ui.add(egui::Button::new(hs_ui::icons::SEARCH).frame(false))
                    .on_hover_text(s.t("quick-open-placeholder"))
                    .clicked()
                    .then(|| self.palette.open(PaletteMode::QuickOpen));
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
        let views = self.tabs.views(&env_colour);
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
        if let Some((from, to)) = r.reorder {
            self.tabs.reorder(from, to);
        }
        if r.new_tab {
            self.open_launcher_tab();
        }
        if let Some((id, action)) = r.menu {
            match action {
                TabMenuAction::Close => self.close_tab(id, false),
                TabMenuAction::CloseOthers => {
                    for other in self.tabs.close_others(id) {
                        self.close_tab(other, false);
                    }
                }
                TabMenuAction::CloseLeft => {
                    for other in self.tabs.close_side(id, true) {
                        self.close_tab(other, false);
                    }
                }
                TabMenuAction::CloseRight => {
                    for other in self.tabs.close_side(id, false) {
                        self.close_tab(other, false);
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
            ui.horizontal(|ui| {
                let mut cx = ToolContext {
                    services: &services,
                    tab_id: a,
                    env_id: env.as_deref(),
                    actions: &mut actions,
                };
                t.tool.toolbar(ui, &mut cx);
            });
        }
        self.pending_actions.extend(actions);
    }

    fn rail(&mut self, ui: &mut egui::Ui) {
        let s = self.services.clone();
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(hs_ui::icons::SIDEBAR).size(18.0))
                        .frame(false),
                )
                .on_hover_text(s.t("sidebar-toggle"))
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
                let active = self
                    .tabs
                    .active
                    .and_then(|a| self.tabs.tabs.iter().find(|t| t.id == a))
                    .is_some_and(|t| t.factory_id == id);
                let col = if active {
                    self.theme.accent
                } else {
                    self.theme.text_muted
                };
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(icon).size(20.0).color(col))
                            .frame(false),
                    )
                    .on_hover_text(title)
                    .clicked()
                {
                    self.open_tool(id, None);
                }
                ui.add_space(4.0);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(hs_ui::icons::DOCK).size(18.0))
                            .frame(false),
                    )
                    .on_hover_text(s.t("dock-toggle"))
                    .clicked()
                {
                    self.dock_open = !self.dock_open;
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
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(&self.status).size(12.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(s.i18n.locale.clone())
                        .size(12.0)
                        .color(self.theme.text_muted),
                );
                let n = s.jobs.running_count();
                let jobs = s.i18n.t_args("status-jobs", &[("n", &n.to_string())]);
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(format!("{} {jobs}", hs_ui::icons::JOBS))
                                .size(12.0),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    self.dock_open = true;
                    self.dock.tab = dock::DockTab::Jobs;
                }
                ui.label(
                    egui::RichText::new(
                        s.i18n
                            .t_args("tab-count", &[("n", &self.tabs.tabs.len().to_string())]),
                    )
                    .size(12.0)
                    .color(self.theme.text_muted),
                );
                ui.label(
                    egui::RichText::new(s.version.version.to_string())
                        .size(12.0)
                        .color(self.theme.text_muted),
                );
            });
        });
    }
}

impl eframe::App for HyperStackApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        let ctx = &ctx;
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
            .exact_size(36.0)
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
            .exact_size(44.0)
            .resizable(false)
            .show(root, |ui| self.rail(ui));
        if self.sidebar_open {
            let w = self.services.settings.read().appearance.sidebar_width;
            egui::Panel::left("hs-sidebar")
                .default_size(w)
                .size_range(180.0..=640.0)
                .resizable(true)
                .show(root, |ui| self.sidebar(ui));
        }
        egui::Panel::bottom("hs-status")
            .exact_size(24.0)
            .resizable(false)
            .show(root, |ui| self.status_bar(ui));
        if self.dock_open {
            let h = self.services.settings.read().appearance.dock_height;
            let logs = self.logging.buffer.clone();
            egui::Panel::bottom("hs-dock")
                .default_size(h)
                .size_range(100.0..=600.0)
                .resizable(true)
                .show(root, |ui| {
                    dock::show(ui, &self.theme, &self.services, &logs, &mut self.dock);
                });
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
                    self.toasts.push(
                        ToastLevel::Info,
                        format!("Connection {c}: tools arrive in M2/M3"),
                    );
                } else {
                    self.run_action(&chosen);
                }
            }
        }
        let effects = settings_ui::show(ctx, &self.theme, &self.services, &mut self.settings_ui);
        self.apply_settings_effects(effects, ctx);
        if let Some((id, name, is_env)) = self.settings_ui.confirm_delete.clone() {
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
                            if is_env {
                                self.settings_ui.env_draft = None;
                                self.settings_ui.env_selected = None;
                            } else {
                                self.settings_ui.conn_draft = None;
                                self.settings_ui.conn_selected = None;
                            }
                        }
                        Err(e) => self.toasts.push(ToastLevel::Error, e.to_string()),
                    }
                    self.settings_ui.confirm_delete = None;
                }
                DialogResult::Cancel => self.settings_ui.confirm_delete = None,
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
        }
        if self.tabs.dirty {
            ctx.request_repaint_after(Duration::from_millis(1100));
        }
        if self.services.jobs.running_count() > 0 {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
}
