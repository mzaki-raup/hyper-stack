//! Settings pages (docs/04 §9). Shown inside the Settings tab
//! (`tools::settings`): the category list in the sidebar, the page in the tab.
//! Every page is built from `hs_ui::form` cards and label/field grids so the
//! pages share one look.

use std::collections::HashMap;

use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::environments::Environment;
use hs_core::objstore::profile::S3Fields;
use hs_core::secrets::SecretRef;
use hs_ui::form;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use secrecy::SecretString;
use tool_db::profile::DbFields;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Accessibility,
    Workspace,
    Shortcuts,
    Environments,
    Connections,
    Containers,
    Security,
    Terminal,
    Editor,
    Git,
    Browser,
    Ai,
    About,
}

impl Page {
    pub fn all() -> [(Page, &'static str); 15] {
        [
            (Page::General, "settings-general"),
            (Page::Appearance, "settings-appearance"),
            (Page::Accessibility, "settings-accessibility"),
            (Page::Workspace, "settings-workspace"),
            (Page::Environments, "settings-environments"),
            (Page::Security, "settings-security"),
            (Page::Terminal, "settings-terminal"),
            (Page::Connections, "settings-connections"),
            (Page::Containers, "settings-containers"),
            (Page::Editor, "settings-editor"),
            (Page::Git, "settings-git"),
            (Page::Browser, "settings-browser"),
            (Page::Ai, "settings-ai"),
            (Page::Shortcuts, "settings-shortcuts"),
            (Page::About, "settings-about"),
        ]
    }

    fn icon(self) -> &'static str {
        use hs_ui::icons::ph;
        match self {
            Page::General => ph::SLIDERS_HORIZONTAL,
            Page::Appearance => ph::PALETTE,
            Page::Accessibility => ph::PERSON_ARMS_SPREAD,
            Page::Workspace => ph::FOLDERS,
            Page::Shortcuts => ph::KEYBOARD,
            Page::Environments => ph::GLOBE_HEMISPHERE_WEST,
            Page::Connections => ph::PLUGS_CONNECTED,
            Page::Containers => ph::SHIPPING_CONTAINER,
            Page::Security => ph::LOCK_KEY,
            Page::Terminal => ph::TERMINAL_WINDOW,
            Page::Editor => ph::CODE,
            Page::Git => ph::GIT_BRANCH,
            Page::Browser => ph::GLOBE,
            Page::Ai => ph::SPARKLE,
            Page::About => ph::INFO,
        }
    }

    /// Stable key (the Fluent key) used for layout persistence and commands.
    pub fn key(self) -> &'static str {
        Page::all()
            .iter()
            .find(|(p, _)| *p == self)
            .map(|(_, k)| *k)
            .unwrap_or("settings-general")
    }
    pub fn by_key(key: &str) -> Option<Page> {
        // The data folder moved from its own page to About (2026-10-12);
        // layouts saved before still name the old page.
        if key == "settings-data" {
            return Some(Page::About);
        }
        // Settings → Welcome moved to the Home tab (2026-10-15).
        if key == "settings-welcome" {
            return Some(Page::General);
        }
        Page::all().iter().find(|(_, k)| *k == key).map(|(p, _)| *p)
    }
}

/// Settings state plus the effects the pages produced this frame, shared
/// between the shell (which applies effects) and the Settings tab (which
/// draws the pages).
pub struct SettingsHub {
    pub state: SettingsState,
    pub effects: Vec<SettingsEffect>,
}

impl Default for SettingsHub {
    fn default() -> Self {
        Self {
            state: SettingsState {
                page: Page::General,
                ..Default::default()
            },
            effects: Vec::new(),
        }
    }
}

pub type SharedSettings = std::sync::Arc<parking_lot::Mutex<SettingsHub>>;

/// Which set of variables the Environments page's right column is
/// editing when it is not an environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VarSource {
    /// An API collection's own variables, by collection id, with its
    /// name for the heading.
    Collection(String, String),
    /// The application-wide set (`hs_core::globals`).
    Globals,
}

/// A `Kv` list as editor rows: the editor's row has a secret flag the
/// collection row has not, so the two are converted at the edges.
fn kv_to_rows(list: &[tool_api::model::Kv]) -> Vec<KvRow> {
    list.iter()
        .map(|v| KvRow {
            enabled: v.enabled,
            key: v.key.clone(),
            value: v.value.clone(),
            secret: false,
        })
        .collect()
}

fn rows_to_kv(rows: &[KvRow]) -> Vec<tool_api::model::Kv> {
    rows.iter()
        .filter(|r| !r.key.trim().is_empty())
        .map(|r| tool_api::model::Kv {
            enabled: r.enabled,
            key: r.key.trim().to_string(),
            value: r.value.clone(),
            description: String::new(),
        })
        .collect()
}

/// An answer on its way back from the runtime, read by the page that
/// asked for it.
type Slot<T> = std::sync::Arc<parking_lot::Mutex<Option<T>>>;

#[derive(Default)]
pub struct SettingsState {
    pub page: Page,
    pub env_selected: Option<String>,
    pub env_draft: Option<(Environment, Vec<KvRow>)>,
    pub env_copy: Option<EnvCopyState>,
    /// Settings -> Environments: a collection's variables or the
    /// application-wide globals being edited in the right column, in
    /// place of an environment (owner's requirement, 2026-10-29).
    pub var_source: Option<VarSource>,
    pub var_draft: Vec<KvRow>,
    /// The "Compare With…" dialog over a collection's variables.
    pub col_copy: Option<ColCopyState>,
    pub conn_selected: Option<String>,
    pub conn_draft: Option<(ConnectionProfile, String)>,
    /// Settings -> Editor -> Project List: which project is open for
    /// editing, and the copy being edited. The draft carries the Git
    /// identity read from the repository itself, so Save writes back only
    /// what was actually changed on screen.
    pub project_selected: Option<String>,
    pub project_draft: Option<ProjectDraft>,
    /// Settings -> Git -> Repositories: the entry open for editing and
    /// the copy being edited (docs/tools/git-activity.md §3).
    pub repo_selected: Option<String>,
    pub repo_draft: Option<RepoDraft>,
    /// Settings -> Git -> Git Credentials: the same pair for a credential.
    pub cred_selected: Option<String>,
    pub cred_draft: Option<hs_core::git_hosts::GitCredential>,
    /// Remove Credential was clicked once; the second click does it.
    pub cred_confirm_remove: bool,
    /// Fetch Project: the host's listing, the request in flight, and
    /// the search typed into the dropdown.
    pub repo_projects: Option<Vec<tool_git::hosts::Project>>,
    pub repo_projects_slot: Slot<Result<Vec<tool_git::hosts::Project>, String>>,
    pub repo_projects_busy: bool,
    pub repo_project_filter: String,
    /// A clone started by Save: the entry to record once it lands.
    pub repo_clone: Option<(hs_core::git_repos::RepoEntry, Slot<Result<(), String>>)>,
    /// The credential Test in flight, by credential id.
    pub cred_test: Option<(String, Slot<Result<String, String>>)>,
    pub shortcut_edit: HashMap<String, String>,
    pub shortcut_filter: String,
    pub confirm_delete: Option<(String, String, bool)>, // id, name, is_env
    pub ai: crate::settings_ai::AiPageState,
    /// Workspace page: the name being edited and which workspace it belongs
    /// to, so switching workspaces does not carry a stale draft over.
    pub ws_name: Option<(String, String)>,
    /// Workspace page: the other workspace whose environments are compared.
    pub ws_compare: String,
    /// Settings → Containers: the text being typed into a registry or
    /// chart-repository password before it is saved to the vault. One
    /// buffer, as the connection forms use.
    pub registry_scratch: String,
    /// Appearance page: (tool id, title) of every tool, for the rail order.
    pub rail_tools: Vec<(String, String)>,
    /// Appearance page: the workspace whose rail is being edited (`None`
    /// = the open one).
    pub rail_ws: Option<String>,
    /// The Test Connection in flight (or its answer), and the connection it
    /// belongs to, so a slow server cannot report onto another profile.
    pub conn_test: Option<crate::conn_test::Test>,
    /// Security page: the credentials row whose secrets are uncovered, and
    /// the `ui.input().time` they go back under. One row at a time, and it
    /// lapses on its own — an uncovered secret left on screen is the
    /// thing this page must not leave behind.
    pub reveal: Option<(String, f64)>,
    /// The master-password prompt that is open, and what it will do.
    pub ask: Option<Ask>,
    /// The Edit Secret dialog, once the master password has been accepted.
    pub edit_secret: Option<EditSecret>,
    /// Terminal page: the CLI profile being added or edited.
    pub cli_draft: Option<hs_core::settings::CliProfile>,
    /// Terminal page: the CLI profile whose removal is being confirmed.
    pub cli_confirm: Option<String>,
    /// Terminal page: commands found on PATH that are not in the list yet.
    pub cli_suggestions: Option<Vec<String>>,
}

/// What the master-password prompt is for.
pub struct Ask {
    pub row: String,
    /// `true` opens the edit dialog; `false` only reveals the row.
    pub edit: bool,
    pub typed: String,
    pub error: Option<String>,
}

/// The password and key of one row, being edited.
pub struct EditSecret {
    pub row: String,
    pub name: String,
    pub password: String,
    pub key: String,
    /// The row has a key secret slot (an SSH key host).
    pub key_editable: bool,
    pub error: Option<String>,
}

/// A workspace's logo, or `None` when it has none set.
fn ws_logo(
    s: &Services,
    registry: &hs_core::workspace::Registry,
    name: &str,
) -> Option<egui::Image<'static>> {
    registry
        .workspaces
        .iter()
        .find(|w| w.name == name)
        .map(|w| s.paths.resolve_path(&w.path))
        .and_then(|root| crate::app::ws_logo_image(&root))
}

/// Draw a workspace's logo (the folder glyph when it has none) at `size`.
/// Every place that names a workspace shows the same mark, at one of the
/// `crate::app::WS_MARK_*` sizes, centred on the name beside it.
fn ws_icon(
    ui: &mut egui::Ui,
    s: &Services,
    registry: &hs_core::workspace::Registry,
    name: &str,
    size: f32,
) {
    crate::app::ws_mark(ui, ws_logo(s, registry, name), size);
}

/// The "Copy Variables From…" dialog on the Environments page: which source
/// workspace and environment to copy from, and which of its keys are ticked.
#[derive(Default)]
pub struct EnvCopyState {
    pub source_ws: String,
    pub source_env: String,
    pub selected: std::collections::HashSet<String>,
    /// The (workspace, env) the `selected` set was seeded for, so a changed
    /// source re-seeds the ticks (missing keys on by default).
    pub seeded_for: Option<(String, String)>,
}

/// The compare/copy/sync dialog for a collection's variables: another
/// collection of this or any workspace is the source (owner's
/// requirement, 2026-10-29).
#[derive(Default)]
pub struct ColCopyState {
    pub source_ws: String,
    pub source_col: String,
    pub selected: std::collections::HashSet<String>,
    pub seeded_for: Option<(String, String)>,
}

pub enum SettingsEffect {
    SettingsChanged,
    CheckUpdates,
    DownloadUpdate,
    InstallUpdate,
    RevertUpdate,
    SkipUpdate(String),
    ThemeChanged,
    FontsChanged,
    LockVault,
    ChangeMasterPassword,
    EnvironmentsChanged,
    /// Import variables into the environment currently being edited.
    ImportEnvironment,
    /// Write the environment being edited to a file in this format.
    ExportEnvironment(tool_api::env_export::Format),
    ConnectionsChanged,
    /// Settings -> Editor -> Project List -> Remove: ask first, then
    /// drop the entry and close the tab that had it open. The
    /// **folder is never touched** - this is the list forgetting a
    /// reference, not a delete (owner's requirement, 2026-10-26),
    /// which is why the confirmation says so in as many words.
    ConfirmRemoveProject(String, String),
    /// Open this project folder in the Editor, in the tab it already has
    /// or in a new one.
    OpenProject(std::path::PathBuf),
    /// Settings -> Git -> Repositories -> Remove From List: ask first,
    /// then drop the entry and close the Git tabs that show it. The
    /// folder is never touched (docs/tools/git-activity.md §4).
    ConfirmRemoveRepo(String, String),
    /// Open this repository in the Git activity, in the tab it already
    /// has or in a new one.
    OpenRepo(std::path::PathBuf),
    /// Settings -> Connections -> Connect: open this saved profile in its
    /// own activity, in the tab it already has or in a new one.
    OpenConnection {
        tool: String,
        id: String,
    },
    /// Sign in to the registry at this index of
    /// `settings.containers.registries` (owner's request, 2026-10-24).
    RegistrySignIn(usize),
    /// Run `helm repo add` for the chart repository at this index.
    ChartRepoAdd(usize),
    /// Rename the open workspace (the shell owns the registry).
    RenameWorkspace(String),
    /// Delete the open workspace after the shell's confirmation.
    DeleteWorkspace,
    /// The workspace logo changed on disk: drop the cached image.
    WorkspaceLogoChanged,
    Toast(hs_core::tool::ToastLevel, String),
    /// Settings → General → Export: the shell confirms the master password
    /// and picks the file.
    ExportSettings,
    ImportSettings,
    /// Settings → Editor: install a language server or debug adapter by
    /// running its command in a new terminal tab.
    RunInTerminal(String),
    /// Settings → Editor: open a download page in the browser.
    OpenUrl(String),
    /// Accessibility → Read Aloud: read this text with the configured voice.
    SpeakSample(String),
    /// Put a secret on the clipboard and start the clearing timer
    /// (Settings → Security → Clear Clipboard After).
    CopySecret(String),
    /// Run one of the shell's own actions (`ws:<name>`, `ws.new`): the
    /// settings page has no `ToolContext` to push commands through.
    RunAction(String),
    StopSpeaking,
    /// Copy the environments `from` lacks into `to` (names that exist are
    /// skipped). Either may be the open workspace.
    SyncEnvironments {
        from: String,
        to: String,
    },
}

/// The category list (drawn in the sidebar of the Settings tab).
pub fn page_list(ui: &mut egui::Ui, theme: &Theme, services: &Services, state: &mut SettingsState) {
    form::header_bar(ui, theme, |ui| {
        ui.label(
            egui::RichText::new(services.t("settings"))
                .strong()
                .size(15.0),
        );
    });
    egui::ScrollArea::vertical()
        .id_salt("settings-pages")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (page, key) in Page::all() {
                if form::list_row(
                    ui,
                    theme,
                    state.page == page,
                    page.icon(),
                    &services.t(key),
                    None,
                )
                .clicked()
                {
                    state.page = page;
                }
                ui.add_space(2.0);
            }
        });
}

/// The selected page. Effects are appended for the shell to apply.
pub fn page(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    match state.page {
        Page::General => general(ui, theme, services, effects),
        Page::Appearance => appearance(ui, theme, services, state, effects),
        Page::Accessibility => accessibility(ui, theme, services, effects),
        Page::Workspace => workspace(ui, theme, services, state, effects),
        Page::Shortcuts => shortcuts(ui, theme, services, state, effects),
        Page::Environments => environments(ui, theme, services, state, effects),
        Page::Connections => connections(ui, theme, services, state, effects),
        Page::Containers => containers(ui, theme, services, state, effects),
        Page::Security => security(ui, theme, services, state, effects),
        Page::Terminal => terminal(ui, theme, services, state, effects),
        Page::Editor => editor(ui, theme, services, state, effects),
        Page::Git => git(ui, theme, services, state, effects),
        Page::Browser => browser(ui, theme, services, effects),
        Page::Ai => crate::settings_ai::page(ui, theme, services, &mut state.ai, effects),
        Page::About => about(ui, theme, services, effects),
    }
}

// ---------------------------------------------------------------------------

/// Name, logo and folder of the workspace that is open. The logo can be
/// changed here after creation, not only in the New Workspace dialog.
fn workspace(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-workspace"),
        Some(&s.t("settings-workspace-sub")),
    );
    let current = s.workspace.read().meta.name.clone();
    let root = s.workspace.read().root().to_path_buf();
    // Every workspace, not only the one that is open: this page is where
    // someone comes to see what there is. Owner's requirement, 2026-10-17.
    let registry = hs_core::workspace::Registry::load(&s.paths);
    form::section(ui, theme, &s.t("settings-workspace-section-list"), |ui| {
        for w in &registry.workspaces {
            let open = w.name == current;
            // A list to pick from, not a table: a larger logo and name
            // (owner's request, 2026-10-23).
            let row = ui.scope(|ui| {
                ui.add_space(3.0);
                ui.horizontal(|ui| {
                    ws_icon(ui, s, &registry, &w.name, crate::app::WS_MARK_LIST);
                    ui.add_space(4.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(&w.name).strong().size(15.0))
                            .selectable(false),
                    );
                    ui.add(
                        egui::Label::new(egui::RichText::new(&w.path).color(theme.text_muted))
                            .selectable(false)
                            .truncate(),
                    );
                    if open {
                        ui.label(
                            egui::RichText::new(s.t("settings-workspace-open-now"))
                                .small()
                                .color(theme.success),
                        );
                    }
                });
            });
            let r = ui.interact(
                row.response.rect,
                ui.id().with(("ws-row", &w.name)),
                egui::Sense::click(),
            );
            if !open && r.clicked() {
                effects.push(SettingsEffect::RunAction(format!("ws:{}", w.name)));
            }
            if !open {
                r.tip(s.t("settings-workspace-switch-hint"));
            }
            ui.add_space(2.0);
        }
        ui.add_space(4.0);
        if form::primary_button(ui, theme, &s.t("workspace-new")).clicked() {
            effects.push(SettingsEffect::RunAction("ws.new".into()));
        }
    });
    ui.add_space(8.0);
    // The draft follows the open workspace; switching discards an edit.
    let draft = match &mut state.ws_name {
        Some((ws, _)) if *ws == current => state.ws_name.as_mut().map(|(_, n)| n).unwrap(),
        _ => {
            state.ws_name = Some((current.clone(), current.clone()));
            state.ws_name.as_mut().map(|(_, n)| n).unwrap()
        }
    };

    form::section(
        ui,
        theme,
        &s.t("settings-workspace-section-identity"),
        |ui| {
            form::field_grid(ui, "ws-identity", |ui| {
                form::label(ui, &s.t("workspace-name"));
                ui.vertical(|ui| {
                    form::text_field(ui, draft, &s.t("workspace-name-hint"));
                    let changed = draft.trim() != current && !draft.trim().is_empty();
                    if ui
                        .add_enabled(
                            changed && current != "default",
                            egui::Button::new(s.t("settings-workspace-rename")),
                        )
                        .clicked()
                    {
                        effects.push(SettingsEffect::RenameWorkspace(draft.trim().to_string()));
                    }
                    if current == "default" {
                        form::hint(ui, theme, &s.t("settings-workspace-default-name-hint"));
                    }
                });
                ui.end_row();
                form::label(ui, &s.t("workspace-logo"));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(44.0, 44.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 6.0, theme.surface);
                        match hs_core::workspace::workspace_icon_file(&root)
                            .and_then(|p| crate::app::logo_image(&p))
                        {
                            Some(img) => img
                                .fit_to_exact_size(egui::vec2(40.0, 40.0))
                                .paint_at(ui, rect.shrink(2.0)),
                            None => {
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    hs_ui::icons::WORKSPACE,
                                    egui::FontId::proportional(22.0),
                                    theme.text_muted,
                                );
                            }
                        }
                        ui.add_space(8.0);
                        if ui.button(s.t("workspace-logo-choose")).clicked()
                            && let Some(p) = rfd::FileDialog::new()
                                .add_filter("Image", hs_core::workspace::ICON_EXTS)
                                .pick_file()
                        {
                            match hs_core::workspace::set_workspace_icon(&root, &p) {
                                Ok(_) => {
                                    effects.push(SettingsEffect::WorkspaceLogoChanged);
                                    effects.push(SettingsEffect::Toast(
                                        hs_core::tool::ToastLevel::Success,
                                        s.t("settings-workspace-logo-set"),
                                    ));
                                }
                                Err(e) => effects.push(SettingsEffect::Toast(
                                    hs_core::tool::ToastLevel::Error,
                                    e.to_string(),
                                )),
                            }
                        }
                        if let Some(existing) = hs_core::workspace::workspace_icon_file(&root)
                            && ui.button(s.t("workspace-logo-remove")).clicked()
                        {
                            match std::fs::remove_file(&existing) {
                                Ok(()) => {
                                    effects.push(SettingsEffect::WorkspaceLogoChanged);
                                    effects.push(SettingsEffect::Toast(
                                        hs_core::tool::ToastLevel::Success,
                                        s.t("settings-workspace-logo-removed"),
                                    ));
                                }
                                Err(e) => effects.push(SettingsEffect::Toast(
                                    hs_core::tool::ToastLevel::Error,
                                    e.to_string(),
                                )),
                            }
                        }
                    });
                    form::hint(ui, theme, &s.t("workspace-logo-hint"));
                });
                ui.end_row();
                form::label(ui, &s.t("settings-workspace-delete-label"));
                ui.vertical(|ui| {
                    let others = hs_core::workspace::Registry::load(&s.paths)
                        .names()
                        .into_iter()
                        .filter(|n| n != &current)
                        .count();
                    if ui
                        .add_enabled(
                            others > 0,
                            egui::Button::new(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    hs_ui::icons::TRASH,
                                    s.t("settings-workspace-delete")
                                ))
                                .color(theme.error),
                            ),
                        )
                        .clicked()
                    {
                        effects.push(SettingsEffect::DeleteWorkspace);
                    }
                    form::hint(ui, theme, &s.t("settings-workspace-delete-hint"));
                });
                ui.end_row();
            });
        },
    );

    form::section(ui, theme, &s.t("settings-workspace-section-envs"), |ui| {
        form::hint(ui, theme, &s.t("settings-workspace-envs-hint"));
        ui.add_space(6.0);
        let registry = hs_core::workspace::Registry::load(&s.paths);
        let others: Vec<String> = registry
            .names()
            .into_iter()
            .filter(|n| *n != current)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if others.is_empty() {
            form::hint(ui, theme, &s.t("settings-workspace-envs-none"));
        } else {
            if !others.contains(&state.ws_compare) {
                state.ws_compare = others[0].clone();
            }
            form::field_grid(ui, "ws-compare", |ui| {
                form::label(ui, &s.t("settings-workspace-compare-with"));
                ui.horizontal(|ui| {
                    ws_icon(ui, s, &registry, &state.ws_compare, crate::app::WS_MARK_ROW);
                    egui::ComboBox::from_id_salt("ws-compare-with")
                        .width(form::FIELD_MIN_WIDTH)
                        .selected_text(state.ws_compare.clone())
                        .show_ui(ui, |ui| {
                            for n in &others {
                                ui.horizontal(|ui| {
                                    ws_icon(ui, s, &registry, n, crate::app::WS_MARK_ROW);
                                    ui.selectable_value(&mut state.ws_compare, n.clone(), n);
                                });
                            }
                        });
                });
                ui.end_row();
            });
            ui.add_space(6.0);
            let here: Vec<(String, usize, bool)> = s
                .environments
                .read()
                .all()
                .iter()
                .map(|e| (e.name.clone(), e.variables.len(), e.protected))
                .collect();
            let there: Vec<(String, usize, bool)> = registry
                .peek(&s.paths, &state.ws_compare)
                .map(|ws| {
                    hs_core::environments::Environments::load(&ws.environments_dir())
                        .all()
                        .iter()
                        .map(|e| (e.name.clone(), e.variables.len(), e.protected))
                        .collect()
                })
                .unwrap_or_default();
            let mut names: Vec<String> = here
                .iter()
                .chain(there.iter())
                .map(|(n, _, _)| n.clone())
                .collect();
            names.sort();
            names.dedup();
            egui::Grid::new("ws-env-compare")
                .striped(true)
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(s.t("env-name")).strong());
                    // Each column is headed by its workspace, with the
                    // same mark the switcher and Home show.
                    for name in [&current, &state.ws_compare] {
                        ui.horizontal(|ui| {
                            ws_icon(ui, s, &registry, name, crate::app::WS_MARK_ROW);
                            ui.label(egui::RichText::new(name).strong());
                        });
                    }
                    ui.end_row();
                    let cell = |ui: &mut egui::Ui, e: Option<&(String, usize, bool)>| match e {
                        Some((_, vars, protected)) => {
                            ui.label(format!(
                                "{} {}{}",
                                hs_ui::icons::CHECK,
                                s.i18n.t_args(
                                    "settings-workspace-env-vars",
                                    &[("count", &vars.to_string())]
                                ),
                                if *protected { " 🔒" } else { "" }
                            ));
                        }
                        None => {
                            ui.colored_label(theme.text_muted, "—");
                        }
                    };
                    for n in &names {
                        ui.label(n);
                        cell(ui, here.iter().find(|(x, _, _)| x == n));
                        cell(ui, there.iter().find(|(x, _, _)| x == n));
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let missing_here = names
                    .iter()
                    .filter(|n| !here.iter().any(|(x, _, _)| x == *n))
                    .count();
                let missing_there = names
                    .iter()
                    .filter(|n| !there.iter().any(|(x, _, _)| x == *n))
                    .count();
                if ui
                    .add_enabled(
                        missing_here > 0,
                        egui::Button::new(format!(
                            "{} {}",
                            hs_ui::icons::ph::DOWNLOAD_SIMPLE,
                            s.i18n.t_args(
                                "settings-workspace-copy-here",
                                &[("count", &missing_here.to_string())]
                            )
                        )),
                    )
                    .clicked()
                {
                    effects.push(SettingsEffect::SyncEnvironments {
                        from: state.ws_compare.clone(),
                        to: current.clone(),
                    });
                }
                if ui
                    .add_enabled(
                        missing_there > 0,
                        egui::Button::new(format!(
                            "{} {}",
                            hs_ui::icons::ph::UPLOAD_SIMPLE,
                            s.i18n.t_args(
                                "settings-workspace-copy-there",
                                &[("count", &missing_there.to_string())]
                            )
                        )),
                    )
                    .clicked()
                {
                    effects.push(SettingsEffect::SyncEnvironments {
                        from: current.clone(),
                        to: state.ws_compare.clone(),
                    });
                }
            });
            form::hint(ui, theme, &s.t("settings-workspace-sync-hint"));
        }
    });

    form::section(ui, theme, &s.t("settings-workspace-section-folder"), |ui| {
        ui.add(
            egui::Label::new(egui::RichText::new(root.display().to_string()).monospace()).wrap(),
        );
        ui.add_space(6.0);
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::FOLDER_OPEN,
                s.t("settings-workspace-open-folder")
            ))
            .clicked()
        {
            open_folder(s, &root);
        }
        form::hint(ui, theme, &s.t("settings-workspace-folder-hint"));
    });
}

/// Window Size, at the foot of the Startup card (owner's request,
/// 2026-10-23). The width and height are the live inner size, so they follow
/// the window as it is dragged; typing in them resizes it. The picker is
/// Full Screen (maximise) or Custom (the size beside it). The floor is
/// `WindowState::MIN_SIZE`, strict: the viewport enforces it, this form
/// cannot ask for less, and a saved geometry below it is replaced on load.
fn window_size(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    use crate::window_state::{SizePreset, WindowState};

    let (inner, monitor, maximized) = ui.ctx().input(|i| {
        let vp = i.viewport();
        (
            vp.inner_rect.map(|r| r.size()),
            vp.monitor_size,
            vp.maximized.unwrap_or(false),
        )
    });
    let Some(inner) = inner else { return };
    let monitor = monitor.unwrap_or(egui::vec2(1920.0, 1080.0));
    // Never offer more than the screen holds, nor less than the window's own
    // size when the screen reports smaller than it.
    let ceiling = egui::vec2(
        monitor.x.max(inner.x).max(WindowState::MIN_SIZE[0]),
        monitor.y.max(inner.y).max(WindowState::MIN_SIZE[1]),
    );

    ui.label(egui::RichText::new(s.t("settings-window-size")).strong());
    ui.add_space(6.0);

    let mut apply: Option<SizePreset> = None;
    let mut resize: Option<egui::Vec2> = None;
    form::field_grid(ui, "window-size-grid", |ui| {
        form::label(ui, &s.t("settings-window-preset"));
        let current = SizePreset::of(maximized);
        egui::ComboBox::from_id_salt("window-preset")
            .width(form::FIELD_MIN_WIDTH)
            .selected_text(s.t(current.key()))
            .show_ui(ui, |ui| {
                for p in SizePreset::ALL {
                    if ui
                        .add(egui::Button::selectable(current == p, s.t(p.key())))
                        .clicked()
                    {
                        apply = Some(p);
                        ui.close();
                    }
                }
            });
        ui.end_row();

        form::label(ui, &s.t("settings-window-dimensions"));
        let mut w = inner.x.round();
        let mut h = inner.y.round();
        ui.horizontal(|ui| {
            let a = ui.add(
                egui::DragValue::new(&mut w)
                    .speed(1.0)
                    .range(WindowState::MIN_SIZE[0]..=ceiling.x)
                    .suffix(" px"),
            );
            ui.label("\u{d7}");
            let b = ui.add(
                egui::DragValue::new(&mut h)
                    .speed(1.0)
                    .range(WindowState::MIN_SIZE[1]..=ceiling.y)
                    .suffix(" px"),
            );
            if a.changed() || b.changed() {
                resize = Some(egui::vec2(w, h));
            }
        });
        ui.end_row();
    });

    let ctx = ui.ctx().clone();
    match apply {
        Some(SizePreset::Full) => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true)),
        // Custom restores the window to the size the fields show.
        Some(SizePreset::Custom) => ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false)),
        None => {}
    }
    if let Some(size) = resize {
        // Leaving a maximised window maximised would swallow the size.
        if maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    form::hint(
        ui,
        theme,
        &s.i18n.t_args(
            "settings-window-size-hint",
            &[
                ("w", &WindowState::MIN_SIZE[0].to_string()),
                ("h", &WindowState::MIN_SIZE[1].to_string()),
            ],
        ),
    );
}

fn general(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-general"),
        Some(&s.t("settings-general-sub")),
    );
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-startup"), |ui| {
        form::field_grid(ui, "general-grid", |ui| {
            form::label(ui, &s.t("settings-startup-workspace"));
            let registry = hs_core::workspace::Registry::load(&s.paths);
            let names: Vec<String> = registry.workspaces.iter().map(|w| w.name.clone()).collect();
            ui.horizontal(|ui| {
                ws_icon(
                    ui,
                    s,
                    &registry,
                    &settings.general.startup_workspace,
                    crate::app::WS_MARK_ROW,
                );
                egui::ComboBox::from_id_salt("startup-ws")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(settings.general.startup_workspace.clone())
                    .show_ui(ui, |ui| {
                        for n in names {
                            ui.horizontal(|ui| {
                                ws_icon(ui, s, &registry, &n, crate::app::WS_MARK_ROW);
                                if ui
                                    .selectable_label(settings.general.startup_workspace == n, &n)
                                    .clicked()
                                {
                                    settings.general.startup_workspace = n.clone();
                                    changed = true;
                                }
                            });
                        }
                    });
            });
            ui.end_row();
            form::label(ui, &s.t("settings-language"));
            let mut lang = settings.general.language.clone();
            let auto = s.t("settings-language-auto");
            egui::ComboBox::from_id_salt("lang")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(if lang.is_empty() {
                    auto.clone()
                } else {
                    lang.clone()
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_value(&mut lang, String::new(), auto.clone())
                        .clicked()
                    {
                        changed = true;
                    }
                    for l in hs_core::i18n::I18n::available() {
                        if ui.selectable_value(&mut lang, l.clone(), l).clicked() {
                            changed = true;
                        }
                    }
                });
            settings.general.language = lang;
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-startup-workspace-hint"));
        ui.add_space(6.0);
        changed |= ui
            .checkbox(
                &mut settings.general.restore_tabs,
                s.t("settings-restore-tabs"),
            )
            .changed();
        ui.add_space(8.0);
        window_size(ui, theme, s);
    });
    form::section(ui, theme, &s.t("settings-section-behaviour"), |ui| {
        form::field_grid(ui, "behaviour-grid", |ui| {
            form::label(ui, &s.t("settings-undo-limit"));
            let mut limit = settings.general.undo_limit.clamp(1, 100);
            let response = form::slider(
                ui,
                theme,
                egui::Slider::new(&mut limit, 1..=100).suffix(" steps"),
            );
            if response.tip(s.t("settings-undo-limit-hint")).changed() {
                settings.general.undo_limit = limit;
                changed = true;
            }
            ui.end_row();
            form::label(ui, &s.t("settings-log-level"));
            let current = crate::dock::level_from_name(&settings.general.log_level);
            let mut chosen = current;
            egui::ComboBox::from_id_salt("settings-log-level")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(s.t(&format!(
                    "log-level-{}",
                    settings.general.log_level.trim().to_lowercase()
                )))
                .show_ui(ui, |ui| {
                    for (level, name) in crate::dock::LEVELS {
                        ui.selectable_value(&mut chosen, level, s.t(&format!("log-level-{name}")));
                    }
                });
            if chosen != current {
                settings.general.log_level = crate::dock::LEVELS
                    .iter()
                    .find(|(l, _)| *l == chosen)
                    .map(|(_, n)| n.to_string())
                    .unwrap_or_else(|| "info".into());
                changed = true;
            }
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-undo-limit-hint"));
        form::hint(ui, theme, &s.t("settings-log-level-hint"));
    });
    drop(settings);
    form::section(ui, theme, &s.t("settings-section-portable"), |ui| {
        form::hint(ui, theme, &s.t("settings-portable-hint"));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::EXPORT,
                    s.t("settings-export")
                ))
                .clicked()
            {
                effects.push(SettingsEffect::ExportSettings);
            }
            if ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::DOWNLOAD_SIMPLE,
                    s.t("settings-import")
                ))
                .clicked()
            {
                effects.push(SettingsEffect::ImportSettings);
            }
        });
        form::hint(ui, theme, &s.t("settings-portable-merge-hint"));
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// Settings → Accessibility: read aloud through the operating system's
/// voice, and reduced motion.
fn accessibility(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-accessibility"),
        Some(&s.t("settings-accessibility-sub")),
    );
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-tts"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.accessibility.tts_enabled,
                s.t("settings-tts-enabled"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-tts-enabled-hint"));
        ui.add_space(6.0);
        form::field_grid(ui, "tts-grid", |ui| {
            form::label(ui, &s.t("settings-tts-rate"));
            changed |= ui
                .add(egui::Slider::new(
                    &mut settings.accessibility.tts_rate,
                    -10..=10,
                ))
                .changed();
            ui.end_row();
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::SPEAKER_HIGH,
                    s.t("settings-tts-test")
                ))
                .clicked()
            {
                effects.push(SettingsEffect::SpeakSample(s.t("settings-tts-sample")));
            }
            if ui
                .button(format!("{} {}", hs_ui::icons::ph::STOP, s.t("tts-stop")))
                .clicked()
            {
                effects.push(SettingsEffect::StopSpeaking);
            }
        });
        if !hs_core::tts::Tts::available() {
            ui.add_space(6.0);
            form::note(
                ui,
                theme,
                hs_ui::icons::WARNING,
                &s.t("settings-tts-unavailable"),
            );
        }
        form::hint(ui, theme, &s.t("settings-tts-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-stt"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.accessibility.stt_enabled,
                s.t("settings-stt-enabled"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-stt-enabled-hint"));
        if !hs_core::stt::Stt::available() {
            form::note(
                ui,
                theme,
                hs_ui::icons::INFO,
                &s.t("settings-stt-unavailable"),
            );
        }
        ui.add_space(6.0);
        form::field_grid(ui, "stt-grid", |ui| {
            form::label(ui, &s.t("settings-stt-language"));
            changed |= form::text_field(
                ui,
                &mut settings.accessibility.stt_language,
                &s.t("settings-stt-language-hint"),
            )
            .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-stt-max"));
            changed |= ui
                .add(
                    egui::Slider::new(&mut settings.accessibility.stt_max_secs, 5..=600)
                        .suffix(" s"),
                )
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-stt-max-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-motion"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.accessibility.reduce_motion,
                s.t("settings-reduce-motion"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-reduce-motion-hint"));
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn updates(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    use hs_core::updater::UpdateStatus;
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-installed"), |ui| {
        form::field_grid(ui, "updates-info", |ui| {
            form::label(ui, &s.t("settings-about-version"));
            ui.monospace(s.version.to_string());
            ui.end_row();
            form::label(ui, &s.t("settings-data-folder"));
            ui.vertical(|ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(s.paths.root().display().to_string()).monospace(),
                    )
                    .wrap(),
                );
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FOLDER_OPEN,
                        s.t("settings-data-open-folder")
                    ))
                    .tip(s.t("settings-data-open-folder-tip"))
                    .clicked()
                {
                    open_folder(s, s.paths.root());
                }
                if let Some(r) = &s.paths.fallback_reason {
                    ui.colored_label(theme.warning, r);
                }
                form::hint(ui, theme, &s.t("settings-data-reset"));
            });
            ui.end_row();
            form::label(ui, &s.t("settings-updates-exe-folder"));
            match s.updater.exe_dir_writable() {
                Ok(dir) => {
                    ui.horizontal(|ui| {
                        ui.monospace(dir.display().to_string());
                        ui.colored_label(
                            theme.success,
                            format!("({})", s.t("settings-updates-writable")),
                        );
                    });
                }
                Err(_) => {
                    ui.colored_label(theme.warning, s.t("settings-updates-read-only"));
                }
            }
            ui.end_row();
        });
    });

    form::section(ui, theme, &s.t("settings-section-update-policy"), |ui| {
        let mut settings = s.settings.write();
        form::field_grid(ui, "updates-policy", |ui| {
            form::label(ui, &s.t("settings-updates-channel"));
            ui.horizontal(|ui| {
                for (ch, key) in [
                    ("stable", "settings-updates-channel-stable"),
                    ("beta", "settings-updates-channel-beta"),
                ] {
                    if ui.radio(settings.updates.channel == ch, s.t(key)).clicked() {
                        settings.updates.channel = ch.into();
                        changed = true;
                    }
                }
            });
            ui.end_row();
            form::label(ui, &s.t("settings-updates-when-found"));
            ui.horizontal(|ui| {
                for (val, key) in [
                    ("notify", "settings-updates-when-notify"),
                    ("install_on_exit", "settings-updates-when-install"),
                ] {
                    if ui
                        .radio(settings.updates.when_found == val, s.t(key))
                        .clicked()
                    {
                        settings.updates.when_found = val.into();
                        changed = true;
                    }
                }
            });
            ui.end_row();
        });
        ui.add_space(6.0);
        changed |= ui
            .checkbox(&mut settings.updates.enabled, s.t("settings-updates-auto"))
            .changed();
        form::hint(ui, theme, &s.t("settings-updates-auto-hint"));
    });

    let status = s.updater.status();
    let last = s.settings.read().updates.last_check.clone();
    form::section(ui, theme, &s.t("settings-section-check"), |ui| {
        ui.horizontal(|ui| {
            match &status {
                UpdateStatus::Checking => {
                    ui.spinner();
                    ui.label(s.t("settings-updates-checking"));
                }
                UpdateStatus::UpToDate { checked_at } => {
                    ui.colored_label(
                        theme.success,
                        format!(
                            "{} {}",
                            hs_ui::icons::CHECK,
                            s.t("settings-updates-up-to-date")
                        ),
                    );
                    ui.colored_label(
                        theme.text_muted,
                        s.i18n
                            .t_args("settings-updates-last-checked", &[("when", checked_at)]),
                    );
                }
                UpdateStatus::Error(e) => {
                    ui.colored_label(
                        theme.error,
                        s.i18n.t_args("settings-updates-error", &[("error", e)]),
                    );
                }
                _ => match &last {
                    Some(w) => {
                        ui.colored_label(
                            theme.text_muted,
                            s.i18n
                                .t_args("settings-updates-last-checked", &[("when", w)]),
                        );
                    }
                    None => {
                        ui.colored_label(theme.text_muted, s.t("settings-updates-never-checked"));
                    }
                },
            }
            let busy = matches!(
                status,
                UpdateStatus::Checking | UpdateStatus::Downloading { .. }
            );
            if !busy && ui.button(s.t("settings-updates-check-now")).clicked() {
                effects.push(SettingsEffect::CheckUpdates);
            }
        });
        ui.add_space(6.0);
        match &status {
            UpdateStatus::Available {
                candidate, asset, ..
            } => {
                let from = s.version.version.to_string();
                let to = candidate.version.to_string();
                ui.label(
                    egui::RichText::new(s.i18n.t_args(
                        "settings-updates-available",
                        &[("from", &from), ("to", &to)],
                    ))
                    .strong()
                    .color(theme.accent),
                );
                egui::CollapsingHeader::new(s.t("settings-updates-notes"))
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                ui.add(egui::Label::new(candidate.notes.as_str()).wrap());
                            });
                    });
                ui.horizontal(|ui| {
                    let size = format!("{:.1} MB", asset.size as f64 / 1_048_576.0);
                    if form::primary_button(
                        ui,
                        theme,
                        &s.i18n
                            .t_args("settings-updates-download", &[("size", &size)]),
                    )
                    .clicked()
                    {
                        effects.push(SettingsEffect::DownloadUpdate);
                    }
                    ui.hyperlink_to(s.t("settings-updates-view"), &candidate.html_url);
                    if ui.button(s.t("settings-updates-skip")).clicked() {
                        effects.push(SettingsEffect::SkipUpdate(candidate.tag.clone()));
                    }
                });
                if asset.name.ends_with(".zip") {
                    form::hint(ui, theme, &s.t("settings-updates-zip-hint"));
                }
            }
            UpdateStatus::Downloading { version } => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        s.i18n
                            .t_args("settings-updates-downloading", &[("version", version)]),
                    );
                });
                let jobs = s.jobs.snapshot();
                if let Some(j) = jobs.iter().rev().find(|j| j.tool == "updater") {
                    if let hs_core::jobs::Progress::Bytes { done, total } = &j.progress {
                        let f = total.map(|t| *done as f32 / t.max(1) as f32).unwrap_or(0.0);
                        ui.add(
                            egui::ProgressBar::new(f)
                                .desired_width(320.0)
                                .text(format!("{:.1} MB", *done as f64 / 1_048_576.0)),
                        );
                    }
                    if let Some(m) = &j.message {
                        ui.colored_label(theme.text_muted, m);
                    }
                }
            }
            UpdateStatus::Ready { version, .. } => {
                ui.colored_label(
                    theme.success,
                    format!(
                        "{} {}",
                        hs_ui::icons::CHECK,
                        s.i18n
                            .t_args("settings-updates-ready", &[("version", version)])
                    ),
                );
                if form::primary_button(ui, theme, &s.t("settings-updates-install")).clicked() {
                    effects.push(SettingsEffect::InstallUpdate);
                }
            }
            UpdateStatus::ManualOnly {
                version,
                reason,
                html_url,
            } => {
                ui.colored_label(
                    theme.warning,
                    s.i18n.t_args(
                        "settings-updates-manual",
                        &[("version", version), ("reason", reason)],
                    ),
                );
                ui.hyperlink_to(s.t("settings-updates-view"), html_url);
            }
            _ => {}
        }
    });

    if let Some((_, prev)) = s.updater.previous_exe() {
        form::section(ui, theme, &s.t("settings-updates-previous"), |ui| {
            ui.horizontal(|ui| {
                ui.monospace(&prev);
                if ui
                    .button(
                        s.i18n
                            .t_args("settings-updates-revert", &[("version", &prev)]),
                    )
                    .clicked()
                {
                    effects.push(SettingsEffect::RevertUpdate);
                }
            });
        });
    }
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn appearance(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let mut settings = s.settings.write();
    let mut changed = false;
    let mut theme_changed = false;
    form::page_title(
        ui,
        theme,
        &s.t("settings-appearance"),
        Some(&s.t("settings-appearance-sub")),
    );

    form::section(ui, theme, &s.t("settings-section-colors"), |ui| {
        form::field_grid(ui, "appearance-colors", |ui| {
            form::label(ui, &s.t("settings-theme-mode"));
            ui.horizontal(|ui| {
                let mode = settings.appearance.mode.clone();
                let legacy_dark = Theme::by_name(&settings.appearance.theme).dark;
                for (id, key) in [
                    ("system", "settings-theme-system"),
                    ("dark", "settings-theme-dark"),
                    ("light", "settings-theme-light"),
                ] {
                    // An empty mode (older settings) reads as the darkness of the scheme.
                    let selected = mode == id
                        || (mode.is_empty() && id == "dark" && legacy_dark)
                        || (mode.is_empty() && id == "light" && !legacy_dark);
                    if ui.selectable_label(selected, s.t(key)).clicked() && !selected {
                        if mode.is_empty() && !legacy_dark {
                            // Keep the light scheme the user had before modes existed.
                            settings.appearance.light_theme = settings.appearance.theme.clone();
                            settings.appearance.theme = "dark".into();
                        }
                        settings.appearance.mode = id.to_string();
                        theme_changed = true;
                    }
                }
            });
            ui.end_row();
            for (dark_slot, label_key, salt) in [
                (true, "settings-dark-scheme", "dark-scheme"),
                (false, "settings-light-scheme", "light-scheme"),
            ] {
                form::label(ui, &s.t(label_key));
                let slot = if dark_slot {
                    &mut settings.appearance.theme
                } else {
                    &mut settings.appearance.light_theme
                };
                let current = Theme::by_name(slot).name;
                egui::ComboBox::from_id_salt(salt)
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (id, name, dark) in Theme::schemes() {
                            if *dark != dark_slot {
                                continue;
                            }
                            if ui.selectable_label(slot == id, *name).clicked() {
                                *slot = (*id).to_string();
                                theme_changed = true;
                            }
                        }
                    });
                ui.end_row();
            }
        });
        form::hint(ui, theme, &s.t("settings-theme-mode-hint"));
        ui.add_space(10.0);
        // Every scheme as a clickable row of its own swatches. Clicking a
        // dark scheme selects it for dark mode (and switches to dark mode
        // unless the OS is in charge); a light one likewise for light mode.
        ui.label(
            egui::RichText::new(s.t("settings-color-palette"))
                .size(13.0)
                .color(theme.text_muted),
        );
        ui.add_space(6.0);
        let active_id = hs_ui::theme::scheme_id(&settings.appearance).to_string();
        for (id, name, dark) in Theme::schemes() {
            let scheme = Theme::by_name(id);
            let selected = active_id == *id;
            let in_slot = if *dark {
                settings.appearance.theme == *id
            } else {
                settings.appearance.light_theme == *id
            };
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().min(520.0), 30.0),
                egui::Sense::click(),
            );
            let fill = if selected {
                theme.accent.linear_multiply(0.30)
            } else if in_slot {
                theme.accent.linear_multiply(0.12)
            } else if resp.hovered() {
                theme.surface_hover
            } else {
                egui::Color32::TRANSPARENT
            };
            if fill != egui::Color32::TRANSPARENT {
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::same(6), fill);
            }
            let mut x = rect.left() + 8.0;
            for c in [
                scheme.window_bg,
                scheme.surface,
                scheme.accent,
                scheme.warning,
            ] {
                let sw = egui::Rect::from_min_size(
                    egui::pos2(x, rect.center().y - 7.0),
                    egui::vec2(16.0, 14.0),
                );
                ui.painter().rect_filled(sw, egui::CornerRadius::same(3), c);
                ui.painter().rect_stroke(
                    sw,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Inside,
                );
                x += 20.0;
            }
            ui.painter().text(
                egui::pos2(x + 8.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                format!(
                    "{}  {name}",
                    if *dark {
                        hs_ui::icons::ph::MOON
                    } else {
                        hs_ui::icons::ph::SUN
                    }
                ),
                egui::FontId::proportional(13.5),
                if selected { theme.accent } else { theme.text },
            );
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, *name)
            });
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !selected
            {
                if *dark {
                    settings.appearance.theme = (*id).to_string();
                    if settings.appearance.mode != "system" {
                        settings.appearance.mode = "dark".into();
                    }
                } else {
                    settings.appearance.light_theme = (*id).to_string();
                    if settings.appearance.mode != "system" {
                        settings.appearance.mode = "light".into();
                    }
                }
                theme_changed = true;
            }
            ui.add_space(2.0);
        }
        ui.add_space(4.0);
        form::hint(ui, theme, &s.t("settings-color-scheme-hint"));
    });

    form::section(ui, theme, &s.t("settings-section-interface-font"), |ui| {
        form::field_grid(ui, "appearance-font", |ui| {
            form::label(ui, &s.t("settings-ui-font"));
            if font_combo(
                ui,
                s,
                "ui-font",
                &mut settings.appearance.ui_font,
                false,
                &s.t("settings-ui-font-default"),
            ) {
                effects.push(SettingsEffect::FontsChanged);
            }
            ui.end_row();
            form::label(ui, &s.t("settings-font-size"));
            if ui
                .add(
                    egui::Slider::new(&mut settings.appearance.ui_font_size, 11.0..=22.0)
                        .step_by(0.5)
                        .suffix(" px"),
                )
                .changed()
            {
                theme_changed = true;
            }
            ui.end_row();
            form::label(ui, &s.t("settings-ui-zoom"));
            if ui
                .add(egui::Slider::new(&mut settings.appearance.ui_zoom, 0.75..=2.0).step_by(0.05))
                .changed()
            {
                theme_changed = true;
            }
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-font-size-hint"));
        form::hint(ui, theme, &s.t("settings-ui-zoom-hint"));
    });

    form::section(ui, theme, &s.t("settings-section-sidebar"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.appearance.rail_coloured,
                s.t("settings-rail-coloured"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-rail-coloured-hint"));
        ui.add_space(6.0);
        // Only meaningful with the colours on, so it says so rather than
        // sitting there doing nothing.
        let coloured = settings.appearance.rail_coloured;
        changed |= ui
            .add_enabled(
                coloured,
                egui::Checkbox::new(
                    &mut settings.appearance.rail_icon_outline,
                    s.t("settings-rail-icon-outline"),
                ),
            )
            .tip_disabled(s.t("settings-rail-icon-outline-needs-colour"))
            .changed();
        form::hint(ui, theme, &s.t("settings-rail-icon-outline-hint"));
    });

    form::section(ui, theme, &s.t("settings-section-tabs"), |ui| {
        form::field_grid(ui, "appearance-tabs", |ui| {
            form::label(ui, &s.t("settings-min-tab-width"));
            changed |= ui
                .add(
                    egui::Slider::new(&mut settings.appearance.min_tab_width, 60.0..=240.0)
                        .suffix(" px"),
                )
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-min-file-tab-width"));
            changed |= ui
                .add(
                    egui::Slider::new(&mut settings.appearance.min_file_tab_width, 80.0..=320.0)
                        .suffix(" px"),
                )
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-tab-gap"));
            changed |= ui
                .add(egui::Slider::new(&mut settings.appearance.tab_gap, 0.0..=24.0).suffix(" px"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-min-file-tab-width-hint"));
    });

    form::section(ui, theme, &s.t("settings-section-rail"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.appearance.rail_labels,
                s.t("settings-rail-labels"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-rail-labels-hint"));
        ui.add_space(8.0);
        // Which workspace's rail is being edited: the open one unless the
        // picker says otherwise. Order and visibility are per workspace
        // (owner's requirement, 2026-10-23); labels above stay global.
        let registry = hs_core::workspace::Registry::load(&s.paths);
        let open_name = s.workspace.read().meta.name.clone();
        let selected = state
            .rail_ws
            .clone()
            .filter(|n| registry.workspaces.iter().any(|w| &w.name == n))
            .unwrap_or_else(|| open_name.clone());
        form::field_grid(ui, "rail-ws-pick", |ui| {
            form::label(ui, &s.t("settings-rail-workspace"));
            ui.horizontal(|ui| {
                ws_icon(ui, s, &registry, &selected, crate::app::WS_MARK_ROW);
                egui::ComboBox::from_id_salt("rail-ws")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(selected.clone())
                    .show_ui(ui, |ui| {
                        for w in &registry.workspaces {
                            ui.horizontal(|ui| {
                                ws_icon(ui, s, &registry, &w.name, crate::app::WS_MARK_ROW);
                                if ui.selectable_label(selected == w.name, &w.name).clicked() {
                                    state.rail_ws = Some(w.name.clone());
                                }
                            });
                        }
                    });
            });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-rail-workspace-hint"));
        ui.add_space(8.0);
        let is_open = selected == open_name;
        // A workspace that is not open is edited through its folder.
        let mut other = if is_open {
            None
        } else {
            registry.peek(&s.paths, &selected).ok()
        };
        let mut prefs = if is_open {
            s.workspace.read().meta.rail_prefs(&settings.appearance)
        } else {
            other
                .as_ref()
                .map(|w| w.meta.rail_prefs(&settings.appearance))
                .unwrap_or_default()
        };
        ui.label(
            egui::RichText::new(s.t("settings-rail-order"))
                .size(13.0)
                .color(theme.text_muted),
        );
        ui.add_space(4.0);
        // The movable tools in their current rail order; Home and Settings
        // are fixed at the ends and not listed.
        let ids: Vec<&str> = state.rail_tools.iter().map(|(id, _)| id.as_str()).collect();
        let mut order: Vec<String> = crate::app::rail_order(&ids, &prefs.order)
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut moved = None;
        let mut toggled: Option<(String, bool)> = None;
        // Header, so the checkbox column says what it is.
        ui.horizontal(|ui| {
            ui.add_space(52.0);
            ui.label(
                egui::RichText::new(s.t("settings-rail-show"))
                    .size(12.0)
                    .color(theme.text_muted),
            );
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(s.t("settings-rail-tool"))
                    .size(12.0)
                    .color(theme.text_muted),
            );
        });
        for (i, id) in order.iter().enumerate() {
            let title = state
                .rail_tools
                .iter()
                .find(|(t, _)| t == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| id.clone());
            let mut shown = !prefs.hidden.iter().any(|h| h == id);
            ui.horizontal(|ui| {
                ui.set_min_width(300.0);
                ui.add_enabled_ui(i > 0, |ui| {
                    if ui
                        .small_button(hs_ui::icons::ph::ARROW_UP)
                        .tip(s.t("settings-rail-move-up"))
                        .clicked()
                    {
                        moved = Some((i, i - 1));
                    }
                });
                ui.add_enabled_ui(i + 1 < order.len(), |ui| {
                    if ui
                        .small_button(hs_ui::icons::ph::ARROW_DOWN)
                        .tip(s.t("settings-rail-move-down"))
                        .clicked()
                    {
                        moved = Some((i, i + 1));
                    }
                });
                if ui
                    .checkbox(&mut shown, "")
                    .tip(s.t("settings-rail-show-tip"))
                    .changed()
                {
                    toggled = Some((id.clone(), shown));
                }
                ui.label(title);
            });
        }
        let mut dirty = false;
        if let Some((from, to)) = moved {
            let t = order.remove(from);
            order.insert(to, t);
            prefs.order = order;
            dirty = true;
        }
        if let Some((id, shown)) = toggled {
            if shown {
                prefs.hidden.retain(|h| *h != id);
            } else if !prefs.hidden.contains(&id) {
                prefs.hidden.push(id);
            }
            dirty = true;
        }
        if dirty {
            let saved = if is_open {
                let mut ws = s.workspace.write();
                *ws.meta.rail_prefs_mut(&settings.appearance) = prefs;
                ws.save_meta()
            } else if let Some(w) = other.as_mut() {
                *w.meta.rail_prefs_mut(&settings.appearance) = prefs;
                w.save_meta()
            } else {
                Ok(())
            };
            if let Err(e) = saved {
                effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                ));
            }
        }
        form::hint(ui, theme, &s.t("settings-rail-order-hint"));
    });

    if theme_changed {
        effects.push(SettingsEffect::ThemeChanged);
    }
    if changed || theme_changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn shortcuts(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-shortcuts"),
        Some(&s.t("settings-shortcuts-sub")),
    );
    form::section(ui, theme, &s.t("settings-section-bindings"), |ui| {
        ui.horizontal(|ui| {
            ui.label(hs_ui::icons::SEARCH);
            form::text_field_width(
                ui,
                &mut state.shortcut_filter,
                &s.t("settings-shortcut-filter"),
                form::FIELD_MIN_WIDTH,
            );
            if ui.button(s.t("settings-shortcut-reset")).clicked() {
                let _ = std::fs::remove_file(s.paths.shortcuts_file());
                *s.shortcuts.write() = hs_core::shortcuts::Keymap::load(&s.paths.shortcuts_file());
                state.shortcut_edit.clear();
            }
        });
        let conflicts = s.shortcuts.read().conflicts();
        if !conflicts.is_empty() {
            ui.add_space(6.0);
            form::note(
                ui,
                theme,
                hs_ui::icons::WARNING,
                &format!(
                    "{}: {}",
                    s.t("settings-shortcut-conflicts"),
                    conflicts
                        .iter()
                        .map(|(c, a)| format!("{} → {}", c.display(), a.join(", ")))
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            );
        }
        ui.add_space(8.0);
        let bindings: Vec<(String, String)> = s
            .shortcuts
            .read()
            .all()
            .map(|(a, c)| (a.clone(), c.display()))
            .collect();
        egui::Grid::new("shortcuts")
            .num_columns(2)
            .striped(true)
            .spacing([18.0, 6.0])
            .min_col_width(220.0)
            .show(ui, |ui| {
                ui.label(egui::RichText::new(s.t("settings-shortcut-action")).strong());
                ui.label(egui::RichText::new(s.t("settings-shortcut-binding")).strong());
                ui.end_row();
                let f = state.shortcut_filter.to_lowercase();
                for (action, display) in bindings {
                    if !f.is_empty()
                        && !action.to_lowercase().contains(&f)
                        && !display.to_lowercase().contains(&f)
                    {
                        continue;
                    }
                    ui.monospace(&action);
                    let draft = state
                        .shortcut_edit
                        .entry(action.clone())
                        .or_insert_with(|| display.clone());
                    let r = form::text_field_width(ui, draft, "", 200.0);
                    let apply = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if apply && *draft != display {
                        let ok = s.shortcuts.write().set(&action, draft);
                        if ok {
                            let _ = s.shortcuts.read().save(&s.paths.shortcuts_file());
                            effects.push(SettingsEffect::Toast(
                                hs_core::tool::ToastLevel::Success,
                                format!("{action} = {draft}"),
                            ));
                        } else {
                            effects.push(SettingsEffect::Toast(
                                hs_core::tool::ToastLevel::Error,
                                s.i18n
                                    .t_args("settings-shortcut-invalid", &[("value", draft)]),
                            ));
                            *draft = display.clone();
                        }
                    }
                    ui.end_row();
                }
            });
        form::hint(ui, theme, &s.t("settings-shortcut-hint"));
    });
}

fn environments(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-environments"),
        Some(&s.t("settings-environments-sub")),
    );
    let mut open_copy = false;
    ui.horizontal_top(|ui| {
        // Left: the list.
        ui.vertical(|ui| {
            let mut list_width = 320.0_f32;
            let env_names: Vec<String> = s
                .environments
                .read()
                .all()
                .iter()
                .map(|env| env.name.clone())
                .collect();
            let font_id = egui::TextStyle::Body.resolve(ui.style());
            let painter = ui.painter();
            for name in &env_names {
                let text_width = painter
                    .layout_no_wrap(name.clone(), font_id.clone(), theme.text)
                    .size()
                    .x;
                list_width = list_width.max(text_width + 56.0);
            }
            list_width = list_width.min((ui.available_width() - 300.0).max(200.0));
            ui.set_min_width(320.0);
            ui.set_width(list_width);
            form::section(ui, theme, &s.t("settings-environments"), |ui| {
                if form::primary_button(
                    ui,
                    theme,
                    &format!("{} {}", hs_ui::icons::TAB_NEW, s.t("env-new")),
                )
                .clicked()
                {
                    let e = Environment {
                        name: "new".into(),
                        ..Default::default()
                    };
                    state.env_selected = Some(e.id.clone());
                    state.env_draft = Some((e, Vec::new()));
                    state.var_source = None;
                }
                ui.add_space(8.0);
                let envs: Vec<(String, String, Option<String>, bool)> = s
                    .environments
                    .read()
                    .all()
                    .iter()
                    .map(|e| (e.id.clone(), e.name.clone(), e.colour.clone(), e.protected))
                    .collect();
                if envs.is_empty() {
                    form::hint(ui, theme, &s.t("env-none-yet"));
                }
                for (id, name, colour, protected) in envs {
                    let col = theme.env_colour(&name, colour.as_deref());
                    let icon = if protected {
                        hs_ui::icons::LOCK
                    } else {
                        hs_ui::icons::ENVIRONMENT
                    };
                    let selected = state.env_selected.as_deref() == Some(&id);
                    let r = form::list_row(ui, theme, selected, icon, &name, None);
                    // Colour dot at the right edge.
                    let dot = egui::pos2(r.rect.right() - 14.0, r.rect.center().y);
                    ui.painter().circle_filled(dot, 5.0, col);
                    if r.clicked() {
                        state.env_selected = Some(id.clone());
                        state.var_source = None;
                        if let Some(e) = s.environments.read().by_id(&id) {
                            let mut vars: Vec<KvRow> = e
                                .variables
                                .iter()
                                .map(|(k, v)| KvRow {
                                    enabled: true,
                                    key: k.clone(),
                                    value: v.clone(),
                                    secret: SecretRef::parse(v).is_some(),
                                })
                                .collect();
                            vars.sort_by(|a, b| a.key.cmp(&b.key));
                            state.env_draft = Some((e.clone(), vars));
                        }
                    }
                    ui.add_space(2.0);
                }
            });
            // The variables an API collection carries: the same card
            // shape, listing this workspace's collections. Edited here
            // as well as in the API client's own Manage page, because
            // this is where someone looking for "where is {{host}} set"
            // comes first (owner's requirement, 2026-10-29).
            ui.add_space(8.0);
            form::section(ui, theme, &s.t("settings-collection-vars"), |ui| {
                let dir = s.workspace.read().root().join("api").join("collections");
                let mut cols: Vec<(String, String, usize)> =
                    tool_api::model::Collection::load_all(&dir)
                        .into_iter()
                        .map(|c| (c.id, c.name, c.variables.len()))
                        .collect();
                cols.sort_by(|a, b| a.1.cmp(&b.1));
                if cols.is_empty() {
                    form::hint(ui, theme, &s.t("settings-collection-vars-none"));
                }
                for (id, name, n) in cols {
                    let selected =
                        matches!(&state.var_source, Some(VarSource::Collection(c, _)) if *c == id);
                    let sub = s
                        .i18n
                        .t_args("settings-vars-count", &[("n", &n.to_string())]);
                    if form::list_row(
                        ui,
                        theme,
                        selected,
                        hs_ui::icons::ph::FOLDER,
                        &name,
                        Some(&sub),
                    )
                    .clicked()
                    {
                        let vars = tool_api::model::Collection::load_all(&dir)
                            .into_iter()
                            .find(|c| c.id == id)
                            .map(|c| kv_to_rows(&c.variables))
                            .unwrap_or_default();
                        state.var_source = Some(VarSource::Collection(id.clone(), name.clone()));
                        state.var_draft = vars;
                        state.env_selected = None;
                        state.env_draft = None;
                    }
                    ui.add_space(2.0);
                }
            });
            // The application-wide set: one row, because there is one
            // set - it reads the same in every workspace, under every
            // environment, from every tool.
            ui.add_space(8.0);
            form::section(ui, theme, &s.t("settings-global-vars"), |ui| {
                form::hint(ui, theme, &s.t("settings-global-vars-hint"));
                let g = hs_core::globals::Globals::load(&s.paths.globals_file());
                let selected = matches!(state.var_source, Some(VarSource::Globals));
                let sub = s
                    .i18n
                    .t_args("settings-vars-count", &[("n", &g.values.len().to_string())]);
                if form::list_row(
                    ui,
                    theme,
                    selected,
                    hs_ui::icons::ph::GLOBE,
                    &s.t("settings-global-vars-row"),
                    Some(&sub),
                )
                .clicked()
                {
                    state.var_source = Some(VarSource::Globals);
                    state.var_draft = kv_to_rows(&g.values);
                    state.env_selected = None;
                    state.env_draft = None;
                }
            });
        });
        ui.add_space(12.0);
        // Right: the form.
        ui.vertical(|ui| {
            if let Some(source) = state.var_source.clone() {
                variable_set_detail(ui, theme, s, state, effects, source);
                return;
            }
            let Some((draft, vars)) = &mut state.env_draft else {
                form::note(ui, theme, hs_ui::icons::INFO, &s.t("env-select-hint"));
                return;
            };
            form::section(ui, theme, &s.t("env-section-details"), |ui| {
                form::field_grid(ui, "env-form", |ui| {
                    form::label(ui, &s.t("env-name"));
                    form::text_field(ui, &mut draft.name, &s.t("env-name"));
                    ui.end_row();
                    form::label(ui, &s.t("env-colour"));
                    let mut col = draft
                        .colour
                        .as_deref()
                        .and_then(hs_ui::theme::parse_hex)
                        .unwrap_or(theme.env_colour(&draft.name, None));
                    if ui.color_edit_button_srgba(&mut col).changed() {
                        draft.colour = Some(hs_ui::theme::to_hex(col));
                    }
                    ui.end_row();
                    form::label(ui, &s.t("env-parent"));
                    let parents: Vec<(String, String)> = s
                        .environments
                        .read()
                        .all()
                        .iter()
                        .filter(|e| e.id != draft.id)
                        .map(|e| (e.id.clone(), e.name.clone()))
                        .collect();
                    let current = draft
                        .parent
                        .clone()
                        .and_then(|p| {
                            parents
                                .iter()
                                .find(|(id, _)| *id == p)
                                .map(|(_, n)| n.clone())
                        })
                        .unwrap_or_else(|| "—".into());
                    egui::ComboBox::from_id_salt("env-parent")
                        .width(form::FIELD_MIN_WIDTH)
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(draft.parent.is_none(), "—").clicked() {
                                draft.parent = None;
                            }
                            for (id, name) in parents {
                                if ui
                                    .selectable_label(draft.parent.as_deref() == Some(&id), name)
                                    .clicked()
                                {
                                    draft.parent = Some(id);
                                }
                            }
                        });
                    ui.end_row();
                    form::label(ui, "");
                    ui.checkbox(&mut draft.protected, s.t("env-protected-flag"))
                        .tip(s.t("env-protected-hint"));
                    ui.end_row();
                });
            });
            form::section(ui, theme, &s.t("env-variables"), |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(format!(
                            "{} {}",
                            hs_ui::icons::ph::DOWNLOAD_SIMPLE,
                            s.t("env-import")
                        ))
                        .clicked()
                    {
                        effects.push(SettingsEffect::ImportEnvironment);
                    }
                    ui.menu_button(
                        format!("{} {}", hs_ui::icons::ph::UPLOAD_SIMPLE, s.t("env-export")),
                        |ui| {
                            ui.set_min_width(160.0);
                            for format in tool_api::env_export::Format::ALL {
                                if ui.button(format.label()).clicked() {
                                    effects.push(SettingsEffect::ExportEnvironment(format));
                                    ui.close();
                                }
                            }
                        },
                    )
                    .response
                    .tip(s.t("env-export-hint"));
                    if ui
                        .button(format!("{} {}", hs_ui::icons::COPY, s.t("env-copy-from")))
                        .clicked()
                    {
                        open_copy = true;
                    }
                });
                ui.add_space(6.0);
                KvEditor {
                    theme,
                    key_hint: &s.t("env-var-name"),
                    value_hint: &s.t("env-var-value"),
                    add_label: &s.t("env-add-variable"),
                    allow_secret: true,
                }
                .show(ui, vars);
                form::hint(ui, theme, &s.t("env-variables-hint"));
            });
            ui.horizontal(|ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    let mut env = draft.clone();
                    env.variables.clear();
                    let vault_locked = s.secrets.is_locked();
                    let mut problem = None;
                    for row in vars
                        .iter_mut()
                        .filter(|r| r.enabled && !r.key.trim().is_empty())
                    {
                        let value = if row.secret && SecretRef::parse(&row.value).is_none() {
                            if vault_locked {
                                problem = Some(s.t("settings-vault-unlock-first"));
                                row.value.clone()
                            } else {
                                match s.secrets.create(
                                    &format!("{}: {}", env.name, row.key),
                                    &SecretString::from(row.value.clone()),
                                ) {
                                    Ok(r) => {
                                        row.value = r.to_string_ref();
                                        row.value.clone()
                                    }
                                    Err(e) => {
                                        problem = Some(e.to_string());
                                        row.value.clone()
                                    }
                                }
                            }
                        } else {
                            row.value.clone()
                        };
                        env.variables.insert(row.key.trim().to_string(), value);
                    }
                    if let Some(p) = problem {
                        effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Warning, p));
                    }
                    match s.environments.write().upsert(env) {
                        Ok(()) => effects.push(SettingsEffect::EnvironmentsChanged),
                        Err(e) => effects.push(SettingsEffect::Toast(
                            hs_core::tool::ToastLevel::Error,
                            e.to_string(),
                        )),
                    }
                }
                if form::danger_button(ui, theme, &s.t("delete")).clicked() {
                    state.confirm_delete = Some((draft.id.clone(), draft.name.clone(), true));
                }
            });
        });
    });
    if open_copy {
        state.env_copy = Some(EnvCopyState {
            source_ws: s.workspace.read().meta.name.clone(),
            ..Default::default()
        });
    }
    env_copy_dialog(ui, theme, s, state, effects);
}

/// The right column of Settings -> Environments when a collection's
/// variables or the globals are being edited: the same editor the
/// environment form uses, over a plain key/value list, with Save.
fn variable_set_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
    source: VarSource,
) {
    let (title, hint) = match &source {
        VarSource::Collection(_, name) => (
            s.i18n
                .t_args("settings-collection-vars-title", &[("name", name)]),
            s.t("settings-collection-vars-detail-hint"),
        ),
        VarSource::Globals => (
            s.t("settings-global-vars-row"),
            s.t("settings-global-vars-detail-hint"),
        ),
    };
    let mut open_compare = false;
    form::section(ui, theme, &title, |ui| {
        form::hint(ui, theme, &hint);
        if matches!(source, VarSource::Collection(..))
            && ui
                .button(format!(
                    "{} {}",
                    hs_ui::icons::ph::ARROWS_LEFT_RIGHT,
                    s.t("col-compare")
                ))
                .tip(s.t("col-compare-hint"))
                .clicked()
        {
            open_compare = true;
        }
        ui.add_space(6.0);
        KvEditor {
            theme,
            key_hint: &s.t("env-var-name"),
            value_hint: &s.t("env-var-value"),
            add_label: &s.t("env-add-variable"),
            allow_secret: false,
        }
        .show(ui, &mut state.var_draft);
    });
    if open_compare {
        state.col_copy = Some(ColCopyState {
            source_ws: s.workspace.read().meta.name.clone(),
            ..Default::default()
        });
    }
    collection_copy_dialog(ui, theme, s, state, effects);
    ui.horizontal(|ui| {
        if form::primary_button(ui, theme, &s.t("save")).clicked() {
            let values = rows_to_kv(&state.var_draft);
            let result = match &source {
                VarSource::Collection(id, _) => {
                    let dir = s.workspace.read().root().join("api").join("collections");
                    match tool_api::model::Collection::load_all(&dir)
                        .into_iter()
                        .find(|c| c.id == *id)
                    {
                        Some(mut c) => {
                            c.variables = values;
                            c.save(&dir).map(|()| {
                                // Every API tab reloads its collections;
                                // the payload names no tab, so none skips it.
                                s.events.publish(hs_core::events::Event::Custom {
                                    topic: "api.collections".into(),
                                    payload: "settings".into(),
                                });
                            })
                        }
                        None => Err(hs_core::storage::StorageError::Message(
                            s.t("settings-collection-vars-gone"),
                        )),
                    }
                }
                VarSource::Globals => {
                    let g = hs_core::globals::Globals {
                        schema: hs_core::globals::SCHEMA,
                        values,
                    };
                    g.save(&s.paths.globals_file()).map(|()| {
                        // Live for every tool at once: the environments
                        // resolve `{{name}}` against this set last.
                        g.apply_to(&mut s.environments.write());
                    })
                }
            };
            match result {
                Ok(()) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Success,
                    s.t("toast-saved"),
                )),
                Err(e) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                )),
            }
        }
        if ui.button(s.t("close")).clicked() {
            state.var_source = None;
            state.var_draft.clear();
        }
    });
}

/// The collections of a workspace, by name, with their variables:
/// `(id, name, sorted vars)`. The current workspace is read from disk
/// too, which is what the API tabs write to, so what the dialog shows
/// is what a request would see.
fn collections_of(s: &Services, ws_name: &str) -> Vec<SourceEnv> {
    let current = s.workspace.read().meta.name.clone();
    let root = if ws_name == current {
        Some(s.workspace.read().root().to_path_buf())
    } else {
        let registry = hs_core::workspace::Registry::load(&s.paths);
        registry
            .workspaces
            .iter()
            .find(|w| w.name == ws_name)
            .map(|w| s.paths.resolve_path(&w.path))
    };
    let Some(root) = root else {
        return Vec::new();
    };
    let mut out: Vec<SourceEnv> =
        tool_api::model::Collection::load_all(&root.join("api").join("collections"))
            .into_iter()
            .map(|c| {
                let mut v: Vec<(String, String)> = c
                    .variables
                    .iter()
                    .filter(|k| k.enabled && !k.key.trim().is_empty())
                    .map(|k| (k.key.clone(), k.value.clone()))
                    .collect();
                v.sort();
                (c.id, c.name, v)
            })
            .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// "Compare With…" over a collection's variables: pick a source
/// collection in this or any workspace, see every key as New, Same or
/// Differs against the draft, and either copy the ticked keys or sync
/// the lot. Sync makes this collection's value match the source's for
/// every key the source has and keeps the keys only this one has - a
/// sync that deleted would be a surprise nobody asked for. Changes land
/// in the draft; Save writes them.
fn collection_copy_dialog(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    if state.col_copy.is_none() {
        return;
    }
    let Some(VarSource::Collection(target_id, target_name)) = state.var_source.clone() else {
        state.col_copy = None;
        return;
    };
    let target: std::collections::HashMap<String, String> = state
        .var_draft
        .iter()
        .filter(|r| !r.key.trim().is_empty())
        .map(|r| (r.key.trim().to_string(), r.value.clone()))
        .collect();

    let registry = hs_core::workspace::Registry::load(&s.paths);
    let ws_names = registry.names();
    let current_ws = s.workspace.read().meta.name.clone();
    let cp = state.col_copy.as_mut().unwrap();
    if cp.source_ws.is_empty() || !ws_names.contains(&cp.source_ws) {
        cp.source_ws = current_ws.clone();
    }
    // The collection being edited is not a source for itself.
    let sources: Vec<SourceEnv> = collections_of(s, &cp.source_ws)
        .into_iter()
        .filter(|(id, ..)| !(cp.source_ws == current_ws && *id == target_id))
        .collect();
    if cp.source_col.is_empty() || !sources.iter().any(|(id, ..)| *id == cp.source_col) {
        cp.source_col = sources
            .first()
            .map(|(id, ..)| id.clone())
            .unwrap_or_default();
    }
    let source_vars: Vec<(String, String)> = sources
        .iter()
        .find(|(id, ..)| *id == cp.source_col)
        .map(|(.., v)| v.clone())
        .unwrap_or_default();
    // Ticks default to everything the target lacks or has differently.
    let seed_key = (cp.source_ws.clone(), cp.source_col.clone());
    if cp.seeded_for.as_ref() != Some(&seed_key) {
        cp.selected = source_vars
            .iter()
            .filter(|(k, v)| target.get(k) != Some(v))
            .map(|(k, _)| k.clone())
            .collect();
        cp.seeded_for = Some(seed_key);
    }

    let mut apply: Option<Vec<(String, String)>> = None;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("hs-col-copy")).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        ui.heading(s.t("col-compare-title"));
        ui.add_space(6.0);
        ui.label(s.i18n.t_args("col-compare-into", &[("name", &target_name)]));
        ui.add_space(10.0);
        let cp = state.col_copy.as_mut().unwrap();
        egui::Grid::new("col-copy-src")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(s.t("env-copy-workspace"));
                egui::ComboBox::from_id_salt("col-copy-ws")
                    .selected_text(&cp.source_ws)
                    .show_ui(ui, |ui| {
                        for n in &ws_names {
                            ui.selectable_value(&mut cp.source_ws, n.clone(), n);
                        }
                    });
                ui.end_row();
                ui.label(s.t("col-compare-collection"));
                let sel_name = sources
                    .iter()
                    .find(|(id, ..)| *id == cp.source_col)
                    .map(|(_, n, _)| n.clone())
                    .unwrap_or_else(|| "—".into());
                egui::ComboBox::from_id_salt("col-copy-col")
                    .selected_text(sel_name)
                    .show_ui(ui, |ui| {
                        for (id, name, _) in &sources {
                            ui.selectable_value(&mut cp.source_col, id.clone(), name);
                        }
                    });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.separator();
        if source_vars.is_empty() {
            form::hint(ui, theme, &s.t("col-compare-empty"));
        } else {
            ui.horizontal(|ui| {
                if ui.button(s.t("env-copy-all")).clicked() {
                    cp.selected = source_vars.iter().map(|(k, _)| k.clone()).collect();
                }
                if ui.button(s.t("col-compare-changed")).clicked() {
                    cp.selected = source_vars
                        .iter()
                        .filter(|(k, v)| target.get(k) != Some(v))
                        .map(|(k, _)| k.clone())
                        .collect();
                }
                if ui.button(s.t("env-copy-none-btn")).clicked() {
                    cp.selected.clear();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (k, v) in &source_vars {
                        ui.horizontal(|ui| {
                            let mut on = cp.selected.contains(k);
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    cp.selected.insert(k.clone());
                                } else {
                                    cp.selected.remove(k);
                                }
                            }
                            // New: not here. Same: here with this value.
                            // Differs: here with another value - the
                            // case a compare exists for.
                            let tag = match target.get(k) {
                                None => egui::RichText::new(s.t("env-copy-new"))
                                    .small()
                                    .color(theme.success),
                                Some(t) if t == v => egui::RichText::new(s.t("col-compare-same"))
                                    .small()
                                    .color(theme.text_muted),
                                Some(_) => egui::RichText::new(s.t("col-compare-differs"))
                                    .small()
                                    .color(theme.warning),
                            };
                            ui.label(egui::RichText::new(k).strong());
                            ui.label(tag);
                            let preview: String = v.chars().take(40).collect();
                            ui.label(egui::RichText::new(preview).color(theme.text_muted));
                            if let Some(t) = target.get(k)
                                && t != v
                            {
                                let here: String = t.chars().take(24).collect();
                                ui.label(
                                    egui::RichText::new(
                                        s.i18n.t_args("col-compare-here", &[("value", &here)]),
                                    )
                                    .small()
                                    .color(theme.text_muted),
                                );
                            }
                        });
                    }
                });
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let n = cp.selected.len();
            let label = s.i18n.t_args("env-copy-apply", &[("n", &n.to_string())]);
            if ui.add_enabled(n > 0, egui::Button::new(label)).clicked() {
                apply = Some(
                    source_vars
                        .iter()
                        .filter(|(k, _)| cp.selected.contains(k))
                        .cloned()
                        .collect(),
                );
            }
            if ui
                .add_enabled(
                    !source_vars.is_empty(),
                    egui::Button::new(s.t("col-sync-all")),
                )
                .tip(s.t("col-sync-all-hint"))
                .clicked()
            {
                apply = Some(source_vars.clone());
            }
            if ui.button(s.t("cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if modal.should_close() {
        cancel = true;
    }

    if let Some(pairs) = apply {
        for (k, v) in &pairs {
            if let Some(row) = state.var_draft.iter_mut().find(|r| r.key.trim() == k) {
                row.value = v.clone();
                row.enabled = true;
            } else {
                state.var_draft.push(KvRow {
                    enabled: true,
                    key: k.clone(),
                    value: v.clone(),
                    secret: false,
                });
            }
        }
        state.var_draft.sort_by(|a, b| a.key.cmp(&b.key));
        effects.push(SettingsEffect::Toast(
            hs_core::tool::ToastLevel::Success,
            s.i18n
                .t_args("col-compare-done", &[("n", &pairs.len().to_string())]),
        ));
        state.col_copy = None;
    } else if cancel {
        state.col_copy = None;
    }
}

/// One source environment for the copy dialog: `(id, name, sorted vars)`.
type SourceEnv = (String, String, Vec<(String, String)>);

/// The "Copy Variables From…" modal: choose a source workspace and
/// environment (anywhere), compare its keys against the environment being
/// edited, tick which to copy, and merge them into the draft.
fn env_copy_dialog(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    if state.env_copy.is_none() || state.env_draft.is_none() {
        return;
    }
    // Keys already in the environment being edited.
    let target_keys: std::collections::HashSet<String> = state
        .env_draft
        .as_ref()
        .map(|(_, vars)| vars.iter().map(|r| r.key.trim().to_string()).collect())
        .unwrap_or_default();
    let target_name = state
        .env_draft
        .as_ref()
        .map(|(e, _)| e.name.clone())
        .unwrap_or_default();

    let registry = hs_core::workspace::Registry::load(&s.paths);
    let ws_names = registry.names();
    let current_ws = s.workspace.read().meta.name.clone();

    let cp = state.env_copy.as_mut().unwrap();
    if cp.source_ws.is_empty() || !ws_names.contains(&cp.source_ws) {
        cp.source_ws = current_ws.clone();
    }

    // Environments available in the chosen source workspace: live for the
    // current workspace, loaded from disk for any other.
    let source_envs: Vec<SourceEnv> = if cp.source_ws == current_ws {
        s.environments
            .read()
            .all()
            .iter()
            .map(|e| {
                let mut v: Vec<(String, String)> = e
                    .variables
                    .iter()
                    .map(|(k, val)| (k.clone(), val.clone()))
                    .collect();
                v.sort();
                (e.id.clone(), e.name.clone(), v)
            })
            .collect()
    } else if let Some(entry) = registry.workspaces.iter().find(|w| w.name == cp.source_ws) {
        let root = s.paths.resolve_path(&entry.path);
        match hs_core::workspace::Workspace::open_or_create(&root, &cp.source_ws) {
            Ok(ws) => hs_core::environments::Environments::load(&ws.environments_dir())
                .all()
                .iter()
                .map(|e| {
                    let mut v: Vec<(String, String)> = e
                        .variables
                        .iter()
                        .map(|(k, val)| (k.clone(), val.clone()))
                        .collect();
                    v.sort();
                    (e.id.clone(), e.name.clone(), v)
                })
                .collect(),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };

    if cp.source_env.is_empty() || !source_envs.iter().any(|(id, ..)| *id == cp.source_env) {
        cp.source_env = source_envs
            .first()
            .map(|(id, ..)| id.clone())
            .unwrap_or_default();
    }
    let source_vars: Vec<(String, String)> = source_envs
        .iter()
        .find(|(id, ..)| *id == cp.source_env)
        .map(|(.., v)| v.clone())
        .unwrap_or_default();

    // Re-seed the tick set when the source changes: default on for keys the
    // target does not already have.
    let seed_key = (cp.source_ws.clone(), cp.source_env.clone());
    if cp.seeded_for.as_ref() != Some(&seed_key) {
        cp.selected = source_vars
            .iter()
            .filter(|(k, _)| !target_keys.contains(k))
            .map(|(k, _)| k.clone())
            .collect();
        cp.seeded_for = Some(seed_key);
    }

    let mut apply: Option<Vec<(String, String)>> = None;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("hs-env-copy")).show(ui.ctx(), |ui| {
        ui.set_width(520.0);
        ui.heading(s.t("env-copy-title"));
        ui.add_space(6.0);
        ui.label(s.i18n.t_args("env-copy-into", &[("name", &target_name)]));
        ui.add_space(10.0);
        let cp = state.env_copy.as_mut().unwrap();
        egui::Grid::new("env-copy-src")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(s.t("env-copy-workspace"));
                egui::ComboBox::from_id_salt("env-copy-ws")
                    .selected_text(&cp.source_ws)
                    .show_ui(ui, |ui| {
                        for n in &ws_names {
                            ui.selectable_value(&mut cp.source_ws, n.clone(), n);
                        }
                    });
                ui.end_row();
                ui.label(s.t("env-copy-environment"));
                let sel_name = source_envs
                    .iter()
                    .find(|(id, ..)| *id == cp.source_env)
                    .map(|(_, n, _)| n.clone())
                    .unwrap_or_else(|| "—".into());
                egui::ComboBox::from_id_salt("env-copy-env")
                    .selected_text(sel_name)
                    .show_ui(ui, |ui| {
                        for (id, name, _) in &source_envs {
                            ui.selectable_value(&mut cp.source_env, id.clone(), name);
                        }
                    });
                ui.end_row();
            });
        ui.add_space(8.0);
        ui.separator();
        if source_vars.is_empty() {
            form::hint(ui, theme, &s.t("env-copy-empty"));
        } else {
            ui.horizontal(|ui| {
                if ui.button(s.t("env-copy-all")).clicked() {
                    cp.selected = source_vars.iter().map(|(k, _)| k.clone()).collect();
                }
                if ui.button(s.t("env-copy-missing")).clicked() {
                    cp.selected = source_vars
                        .iter()
                        .filter(|(k, _)| !target_keys.contains(k))
                        .map(|(k, _)| k.clone())
                        .collect();
                }
                if ui.button(s.t("env-copy-none-btn")).clicked() {
                    cp.selected.clear();
                }
            });
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .show(ui, |ui| {
                    for (k, v) in &source_vars {
                        ui.horizontal(|ui| {
                            let mut on = cp.selected.contains(k);
                            if ui.checkbox(&mut on, "").changed() {
                                if on {
                                    cp.selected.insert(k.clone());
                                } else {
                                    cp.selected.remove(k);
                                }
                            }
                            let exists = target_keys.contains(k);
                            let tag = if exists {
                                egui::RichText::new(s.t("env-copy-exists"))
                                    .small()
                                    .color(theme.warning)
                            } else {
                                egui::RichText::new(s.t("env-copy-new"))
                                    .small()
                                    .color(theme.success)
                            };
                            ui.label(egui::RichText::new(k).strong());
                            ui.label(tag);
                            let preview: String = v.chars().take(40).collect();
                            ui.label(egui::RichText::new(preview).color(theme.text_muted));
                        });
                    }
                });
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let n = cp.selected.len();
            let label = s.i18n.t_args("env-copy-apply", &[("n", &n.to_string())]);
            let enabled = n > 0;
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                apply = Some(
                    source_vars
                        .iter()
                        .filter(|(k, _)| cp.selected.contains(k))
                        .cloned()
                        .collect(),
                );
            }
            if ui.button(s.t("cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if modal.should_close() {
        cancel = true;
    }

    if let Some(pairs) = apply {
        if let Some((_, vars)) = state.env_draft.as_mut() {
            for (k, v) in &pairs {
                let secret = SecretRef::parse(v).is_some();
                if let Some(row) = vars.iter_mut().find(|r| r.key.trim() == k) {
                    row.value = v.clone();
                    row.secret = secret;
                    row.enabled = true;
                } else {
                    vars.push(KvRow {
                        enabled: true,
                        key: k.clone(),
                        value: v.clone(),
                        secret,
                    });
                }
            }
            vars.sort_by(|a, b| a.key.cmp(&b.key));
        }
        effects.push(SettingsEffect::Toast(
            hs_core::tool::ToastLevel::Success,
            s.i18n
                .t_args("env-copy-done", &[("n", &pairs.len().to_string())]),
        ));
        state.env_copy = None;
    } else if cancel {
        state.env_copy = None;
    }
}

/// Settings → Containers (ADR-026, `docs/tools/containers.md` §10.7).
///
/// The Container List is a **kind-filtered view of the connection
/// store**, not a second store: a container host is an ordinary
/// `ConnectionProfile` of kind `docker`, `podman`, `k8s` or
/// `wincontainer`. That is what makes the vault, per-environment hosts,
/// read-only handling, folders, tags, pinning and ordering work here
/// without being written again — and it is why this list and Settings →
/// Connections can never disagree about the same host.
fn containers(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-containers"),
        Some(&s.t("settings-containers-sub")),
    );
    container_list(ui, theme, s, state, effects);
    container_defaults(ui, theme, s, effects);
    container_registries(ui, theme, s, effects, state);
    container_tools(ui, theme, s);
}

/// The list, and the form for whichever host is chosen.
fn container_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    use hs_core::connections::{CONTAINER_KINDS, kind_label};
    form::section(ui, theme, &s.t("settings-container-list"), |ui| {
        form::hint(ui, theme, &s.t("settings-container-list-hint"));
        ui.add_space(8.0);
        // The right column needs a real width: inside a horizontal
        // layout `available_width` is unbounded and a wrapped row would
        // never wrap.
        let list_width = 260.0;
        let right_width = (ui.available_width() - list_width - 16.0).max(340.0);
        // The four kinds are one row across the **section**, not a
        // wrapped block inside the 260 px list column, where they broke
        // over three lines and read as a list of their own. Owner's
        // requirement, 2026-10-25.
        ui.horizontal(|ui| {
            for kind in CONTAINER_KINDS {
                let label = egui::RichText::new(format!(
                    "{} {}",
                    container_kind_icon(kind),
                    kind_label(kind)
                ))
                .size(14.0);
                if ui
                    .add(egui::Button::new(label).min_size(egui::vec2(0.0, 32.0)))
                    .tip(s.t("settings-container-add-hint"))
                    .clicked()
                {
                    let mut draft = hs_core::connections::ConnectionProfile {
                        name: format!("New {}", kind_label(kind)),
                        kind: (*kind).to_string(),
                        ..Default::default()
                    };
                    // The defaults for the kind, so the form opens on
                    // something that would work rather than on empty
                    // fields.
                    hs_container::profile::ContainerFields::default().write_into(&mut draft.fields);
                    state.conn_selected = Some(draft.id.clone());
                    let text = toml::to_string_pretty(&draft.fields).unwrap_or_default();
                    state.conn_draft = Some((draft, text));
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(list_width);
                let hosts: Vec<(String, String, String)> = s
                    .connections
                    .read()
                    .all()
                    .iter()
                    .filter(|c| CONTAINER_KINDS.contains(&c.kind.as_str()))
                    .map(|c| (c.id.clone(), c.kind.clone(), c.name.clone()))
                    .collect();
                if hosts.is_empty() {
                    form::hint(ui, theme, &s.t("settings-container-none"));
                }
                // Dragging here is the order the activity's sidebar
                // inherits, and `reorder` takes the same kind filter, so
                // moving a container never disturbs a database.
                let mut reorder = hs_ui::reorder::Reorder::new("container-list-reorder");
                let mut pick: Option<String> = None;
                for (i, (id, kind, name)) in hosts.iter().enumerate() {
                    let selected = state.conn_selected.as_deref() == Some(id.as_str());
                    let subtitle = container_subtitle(s, id, kind);
                    reorder.row(ui, i, |ui| {
                        if form::list_row(
                            ui,
                            theme,
                            selected,
                            container_kind_icon(kind),
                            name,
                            Some(&subtitle),
                        )
                        .clicked()
                        {
                            pick = Some(id.clone());
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent)
                    && let Err(e) = s.connections.write().reorder(CONTAINER_KINDS, from, to)
                {
                    effects.push(SettingsEffect::Toast(
                        hs_core::tool::ToastLevel::Error,
                        e.to_string(),
                    ));
                }
                if let Some(id) = pick
                    && let Some(p) = s.connections.read().by_id(&id).cloned()
                {
                    state.conn_selected = Some(id);
                    let text = toml::to_string_pretty(&p.fields).unwrap_or_default();
                    state.conn_draft = Some((p, text));
                }
            });
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.set_width(right_width);
                let is_container = state
                    .conn_draft
                    .as_ref()
                    .is_some_and(|(d, _)| hs_core::connections::is_container_kind(&d.kind));
                if !is_container {
                    form::note(
                        ui,
                        theme,
                        hs_ui::icons::INFO,
                        &s.t("settings-container-select-hint"),
                    );
                    return;
                }
                conn_detail(ui, theme, s, state, effects);
            });
        });
    });
}

/// What a row says under the name: the kind, and how it is reached.
fn container_subtitle(s: &Services, id: &str, kind: &str) -> String {
    let label = hs_core::connections::kind_label(kind);
    let Some(p) = s.connections.read().by_id(id).cloned() else {
        return label.to_string();
    };
    let f = hs_container::profile::ContainerFields::from_table(&p.fields);
    format!("{label} · {}", f.describe(kind))
}

/// The glyph for a container kind: what the thing **is**, as every other
/// list in the app now does.
/// The one table, shared with the Container activity so the two lists
/// of hosts can never show different icons for the same kind.
fn container_kind_icon(kind: &str) -> &'static str {
    hs_ui::icons::container_kind_icon(kind)
}

/// Defaults and Safety: the guards that apply to every container host.
fn container_defaults(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    effects: &mut Vec<SettingsEffect>,
) {
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-container-safety"), |ui| {
        let mut st = s.settings.write();
        let c = &mut st.containers;
        form::field_grid(ui, "container-safety", |ui| {
            form::label(ui, &s.t("settings-container-confirm-stop"));
            changed |= ui.checkbox(&mut c.confirm_stop, "").changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-confirm-remove"));
            changed |= ui.checkbox(&mut c.confirm_remove_by_name, "").changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-protected-read-only"));
            changed |= ui
                .checkbox(&mut c.protected_read_only, "")
                .tip(s.t("settings-container-protected-read-only-hint"))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-mask-secrets"));
            changed |= ui.checkbox(&mut c.mask_secret_values, "").changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-allow-prune"));
            changed |= ui.checkbox(&mut c.allow_prune, "").changed();
            ui.end_row();
        });
        form::hint(
            ui,
            theme,
            &s.t("settings-container-protected-read-only-hint"),
        );
        ui.add_space(8.0);
        form::field_grid(ui, "container-limits", |ui| {
            form::label(ui, &s.t("settings-container-refresh"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut c.refresh_secs)
                        .range(1..=300)
                        .suffix("s"),
                )
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-stream-stats"));
            changed |= ui
                .checkbox(&mut c.stream_stats, "")
                .tip(s.t("settings-container-stream-stats-hint"))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-page-size"));
            changed |= ui
                .add(egui::DragValue::new(&mut c.page_size).range(20..=5000))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-log-tail"));
            changed |= ui
                .add(egui::DragValue::new(&mut c.log_tail_lines).range(50..=100_000))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-dry-run-first"));
            changed |= ui
                .checkbox(&mut c.apply_dry_run_first, "")
                .tip(s.t("settings-container-dry-run-first-hint"))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-manifest-dir"));
            changed |= form::text_field(ui, &mut c.manifest_dir, "").changed();
            ui.end_row();
            form::label(ui, &s.t("settings-container-exec-shell"));
            changed |= form::text_field(ui, &mut c.exec_shell, "/bin/sh")
                .tip(s.t("settings-container-exec-shell-hint"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-container-exec-shell-hint"));
        form::hint(ui, theme, &s.t("settings-container-dry-run-first-hint"));
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// Settings → Containers → Registries and Chart Repositories (owner's
/// request, 2026-10-24).
///
/// Credentials go to the vault on save like every other secret here,
/// and **Sign In** hands the password to `docker login` on stdin: an
/// argument list is readable by anything that can see the process
/// table, which is why `--password` is never used.
fn container_registries(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    effects: &mut Vec<SettingsEffect>,
    state: &mut SettingsState,
) {
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-container-registries"), |ui| {
        form::hint(ui, theme, &s.t("settings-container-registries-hint"));
        ui.add_space(6.0);
        let mut remove: Option<usize> = None;
        let mut sign_in: Option<usize> = None;
        {
            let mut st = s.settings.write();
            let list = &mut st.containers.registries;
            for (i, r) in list.iter_mut().enumerate() {
                ui.push_id(("registry", i), |ui| {
                    form::field_grid(ui, "registry-row", |ui| {
                        form::label(ui, &s.t("settings-container-registry-name"));
                        changed |= form::text_field(ui, &mut r.name, "Docker Hub").changed();
                        ui.end_row();
                        form::label(ui, &s.t("settings-container-registry-host"));
                        changed |= form::text_field(
                            ui,
                            &mut r.url,
                            &s.t("settings-container-registry-host-hint"),
                        )
                        .changed();
                        ui.end_row();
                        form::label(ui, &s.t("conn-user"));
                        changed |= form::text_field(ui, &mut r.user, "").changed();
                        ui.end_row();
                        form::label(ui, &s.t("conn-password"));
                        changed |= secret_field(
                            ui,
                            theme,
                            s,
                            "registry-pw",
                            &mut r.password,
                            &mut state.registry_scratch,
                            i,
                        );
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                r.is_usable(),
                                egui::Button::new(s.t("settings-container-sign-in")),
                            )
                            .tip_disabled(s.t("settings-container-sign-in-needs-user"))
                            .clicked()
                        {
                            sign_in = Some(i);
                        }
                        if ui.button(s.t("remove")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.add_space(4.0);
                    ui.separator();
                });
            }
        }
        if let Some(i) = remove {
            s.settings.write().containers.registries.remove(i);
            changed = true;
        }
        if let Some(i) = sign_in {
            effects.push(SettingsEffect::RegistrySignIn(i));
        }
        if ui.button(s.t("settings-container-add-registry")).clicked() {
            s.settings
                .write()
                .containers
                .registries
                .push(hs_core::settings::Registry::default());
            changed = true;
        }
    });

    form::section(ui, theme, &s.t("settings-container-charts"), |ui| {
        form::hint(ui, theme, &s.t("settings-container-charts-hint"));
        ui.add_space(6.0);
        let mut remove: Option<usize> = None;
        let mut add_repo: Option<usize> = None;
        {
            let mut st = s.settings.write();
            let list = &mut st.containers.chart_repos;
            for (i, r) in list.iter_mut().enumerate() {
                ui.push_id(("chart", i), |ui| {
                    form::field_grid(ui, "chart-row", |ui| {
                        form::label(ui, &s.t("settings-container-chart-name"));
                        changed |= form::text_field(ui, &mut r.name, "bitnami").changed();
                        ui.end_row();
                        form::label(ui, &s.t("settings-container-chart-url"));
                        changed |= form::text_field(ui, &mut r.url, "https://charts.example.com")
                            .changed();
                        ui.end_row();
                        form::label(ui, &s.t("conn-user"));
                        changed |= form::text_field(ui, &mut r.user, "").changed();
                        ui.end_row();
                        form::label(ui, &s.t("conn-password"));
                        changed |= secret_field(
                            ui,
                            theme,
                            s,
                            "chart-pw",
                            &mut r.password,
                            &mut state.registry_scratch,
                            100 + i,
                        );
                        ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !r.name.trim().is_empty() && !r.url.trim().is_empty(),
                                egui::Button::new(s.t("settings-container-add-repo")),
                            )
                            .tip_disabled(s.t("settings-container-add-repo-needs"))
                            .clicked()
                        {
                            add_repo = Some(i);
                        }
                        if ui.button(s.t("remove")).clicked() {
                            remove = Some(i);
                        }
                    });
                    ui.add_space(4.0);
                    ui.separator();
                });
            }
        }
        if let Some(i) = remove {
            s.settings.write().containers.chart_repos.remove(i);
            changed = true;
        }
        if let Some(i) = add_repo {
            effects.push(SettingsEffect::ChartRepoAdd(i));
        }
        if ui.button(s.t("settings-container-add-chart")).clicked() {
            s.settings
                .write()
                .containers
                .chart_repos
                .push(hs_core::settings::ChartRepo::default());
            changed = true;
        }
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// The command-line tools the activity can use, and whether they are
/// here. The same shape the Editor page uses for language servers,
/// including its note that a tool installed while the app is open is not
/// seen until Refresh Status.
fn container_tools(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    const TOOLS: &[(&str, &str)] = &[
        ("docker", "Docker"),
        ("podman", "Podman"),
        ("kubectl", "kubectl"),
        ("helm", "Helm"),
    ];
    form::section(ui, theme, &s.t("settings-container-tools"), |ui| {
        form::hint(ui, theme, &s.t("settings-container-tools-hint"));
        ui.add_space(8.0);
        form::field_grid(ui, "container-tools", |ui| {
            for (cmd, label) in TOOLS {
                form::label(ui, label);
                let found = hs_core::env_path::find(cmd).is_some();
                ui.label(if found {
                    egui::RichText::new(s.t("settings-editor-lsp-found")).color(theme.accent)
                } else {
                    egui::RichText::new(s.t("settings-editor-lsp-not-found"))
                        .color(theme.text_muted)
                });
                ui.end_row();
            }
        });
    });
}

fn connections(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-connections"),
        Some(&s.t("settings-connections-sub")),
    );
    // The list on the left and the whole form on the right. The form's
    // sections must be stacked inside a vertical of their own: drawn
    // straight into this horizontal layout, Details and the kind-specific
    // section became two columns side by side.
    let list_width = 260.0;
    let right_width = (ui.available_width() - list_width - 16.0).max(340.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(list_width);
            form::section(ui, theme, &s.t("settings-connections"), |ui| {
                if form::primary_button(
                    ui,
                    theme,
                    &format!("{} {}", hs_ui::icons::TAB_NEW, s.t("conn-new")),
                )
                .clicked()
                {
                    let c = ConnectionProfile {
                        name: "new".into(),
                        kind: "db".into(),
                        ..Default::default()
                    };
                    state.conn_selected = Some(c.id.clone());
                    state.conn_draft = Some((c, String::new()));
                }
                ui.add_space(8.0);
                // The engine travels with the kind: a `db` profile's
                // icon is PostgreSQL's or MySQL's, not a generic plug.
                let conns: Vec<(String, String, String, &'static str)> = s
                    .connections
                    .read()
                    .all()
                    .iter()
                    .map(|c| {
                        // Reads the engine for a database and the
                        // detected OS for an SSH host, so this list
                        // agrees with the activity sidebars.
                        let icon = hs_ui::icons::profile_icon(c);
                        (c.id.clone(), c.kind.clone(), c.name.clone(), icon)
                    })
                    .collect();
                if conns.is_empty() {
                    form::hint(ui, theme, &s.t("conn-none"));
                }
                // The whole list, every kind together: dragging here is the
                // order the tools' own sidebars inherit.
                let mut reorder = hs_ui::reorder::Reorder::new("conn-list-reorder");
                let mut pick: Option<String> = None;
                for (i, (id, kind, name, icon)) in conns.iter().enumerate() {
                    let selected = state.conn_selected.as_deref() == Some(id.as_str());
                    reorder.row(ui, i, |ui| {
                        if form::list_row(
                            ui,
                            theme,
                            selected,
                            icon,
                            name,
                            Some(hs_core::connections::kind_label(kind)),
                        )
                        .clicked()
                        {
                            pick = Some(id.clone());
                        }
                    });
                    ui.add_space(2.0);
                }
                if let Some((from, to)) = reorder.finish(ui, theme.accent)
                    && let Err(e) = s.connections.write().reorder(&[], from, to)
                {
                    effects.push(SettingsEffect::Toast(
                        hs_core::tool::ToastLevel::Error,
                        e.to_string(),
                    ));
                }
                if let Some(id) = pick {
                    state.conn_selected = Some(id.clone());
                    if let Some(c) = s.connections.read().by_id(&id) {
                        let fields = toml::to_string_pretty(&c.fields).unwrap_or_default();
                        state.conn_draft = Some((c.clone(), fields));
                    }
                }
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
            ui.set_width(right_width);
            conn_detail(ui, theme, s, state, effects);
        });
    });
}

/// The connection detail column: the shared form for one profile, with
/// its kind-specific section, Save, Test and Delete.
///
/// Extracted from `connections()` so Settings -> Containers can show the
/// same form for a container host (ADR-026). Duplicating a form that
/// moves secrets into the vault would be the worst thing in this file to
/// have two copies of.
/// Save the open draft: move its typed secrets into the vault, write the
/// profile, and un-hide the activity it belongs to.
///
/// A function rather than the body of the Save button, because Connect
/// saves too: what it opens has to be what is on disk, not a draft the
/// user never wrote. `false` means nothing was saved and the caller must
/// not act as though it was.
fn save_draft(
    s: &Services,
    draft: &mut ConnectionProfile,
    fields_text: &str,
    effects: &mut Vec<SettingsEffect>,
) -> bool {
    let is_ssh = draft.kind == "ssh";
    let is_s3 = draft.kind == "s3";
    let is_db = draft.kind == "db";
    let is_container = hs_core::connections::is_container_kind(&draft.kind);
    let mut saved = false;
    let is_http_store = matches!(draft.kind.as_str(), "webdav" | "nexus");
    let parsed = if is_ssh || is_s3 || is_db || is_container || is_http_store {
        // These forms write into `draft.fields` directly,
        // so the text box is a view rather than the truth.
        Ok(draft.fields.clone())
    } else {
        toml::from_str::<toml::Table>(fields_text)
    };
    match parsed {
        Ok(mut table) => {
            let mut secrets_ok = true;
            if is_ssh {
                let mut f = hs_ssh::SshFields::from_table(&table);
                secrets_ok = store_typed_secrets(
                    s,
                    &draft.name,
                    [
                        ("password", &mut f.password),
                        ("key passphrase", &mut f.key_passphrase),
                    ],
                    effects,
                );
                f.write_into(&mut table);
                draft.fields = table.clone();
            } else if is_s3 {
                let mut f = S3Fields::from_table(&table);
                secrets_ok = store_typed_secrets(
                    s,
                    &draft.name,
                    [
                        ("secret key", &mut f.secret_key),
                        ("session token", &mut f.session_token),
                    ],
                    effects,
                );
                f.write_into(&mut table);
                draft.fields = table.clone();
            } else if is_db {
                let mut f = DbFields::from_table(&table);
                secrets_ok =
                    store_typed_secrets(s, &draft.name, [("password", &mut f.password)], effects);
                f.write_into(&mut table);
                draft.fields = table.clone();
            } else if draft.kind == "webdav" {
                let mut f = hs_core::objstore::http_profile::WebdavFields::from_table(&table);
                secrets_ok =
                    store_typed_secrets(s, &draft.name, [("password", &mut f.password)], effects);
                f.write_into(&mut table);
                draft.fields = table.clone();
            } else if draft.kind == "nexus" {
                let mut f = hs_core::objstore::http_profile::NexusFields::from_table(&table);
                secrets_ok =
                    store_typed_secrets(s, &draft.name, [("password", &mut f.password)], effects);
                f.write_into(&mut table);
                draft.fields = table.clone();
            } else if is_container {
                // Every sensitive field at once, from the
                // one list that knows which they are, so a
                // field added later cannot be forgotten
                // here and written in the clear.
                let mut f = hs_container::profile::ContainerFields::from_table(&table);
                secrets_ok = store_typed_secrets(s, &draft.name, f.secret_fields(), effects);
                f.write_into(&mut table);
                draft.fields = table.clone();
            }
            if !secrets_ok {
                // Never write a typed secret to disk.
                return false;
            }
            let mut c = draft.clone();
            c.fields = table;
            // A connection for a tool whose rail entry is
            // hidden would otherwise have nowhere to open.
            let tool = crate::launcher::tool_for_kind(&c.kind);
            match s.connections.write().upsert(c) {
                Ok(()) => {
                    if let Some(tool) = tool {
                        let appearance = s.settings.read().appearance.clone();
                        let mut ws = s.workspace.write();
                        let prefs = ws.meta.rail_prefs_mut(&appearance);
                        let before = prefs.hidden.len();
                        prefs.hidden.retain(|h| h != tool);
                        if prefs.hidden.len() != before {
                            let _ = ws.save_meta();
                        }
                    }
                    saved = true;
                    effects.push(SettingsEffect::ConnectionsChanged);
                }
                Err(e) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                )),
            }
        }
        Err(e) => effects.push(SettingsEffect::Toast(
            hs_core::tool::ToastLevel::Error,
            s.i18n
                .t_args("conn-toml-error", &[("error", &e.to_string())]),
        )),
    }
    saved
}

fn conn_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let Some((draft, fields_text)) = &mut state.conn_draft else {
        form::note(ui, theme, hs_ui::icons::INFO, &s.t("conn-select-hint"));
        return;
    };
    form::section(ui, theme, &s.t("conn-section-details"), |ui| {
        form::field_grid(ui, "conn-form", |ui| {
            form::label(ui, &s.t("conn-name"));
            form::text_field(ui, &mut draft.name, &s.t("conn-name"));
            ui.end_row();
            form::label(ui, &s.t("conn-kind"));
            egui::ComboBox::from_id_salt("conn-kind")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(hs_core::connections::kind_label(&draft.kind))
                .show_ui(ui, |ui| {
                    for k in hs_core::connections::KINDS {
                        ui.selectable_value(
                            &mut draft.kind,
                            (*k).to_string(),
                            hs_core::connections::kind_label(k),
                        );
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("conn-folder"));
            form::text_field(ui, &mut draft.folder, &s.t("conn-folder-hint"));
            ui.end_row();
            form::label(ui, &s.t("conn-tags"));
            let mut tags = draft.tags.join(", ");
            if form::text_field(ui, &mut tags, &s.t("conn-tags-hint")).changed() {
                draft.tags = tags
                    .split(',')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect();
            }
            ui.end_row();
            form::label(ui, &s.t("conn-environment"));
            let envs: Vec<(String, String)> = s
                .environments
                .read()
                .all()
                .iter()
                .map(|e| (e.id.clone(), e.name.clone()))
                .collect();
            let cur = draft
                .environment
                .clone()
                .and_then(|id| envs.iter().find(|(i, _)| *i == id).map(|(_, n)| n.clone()))
                .unwrap_or_else(|| "—".into());
            egui::ComboBox::from_id_salt("conn-env")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(cur)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(draft.environment.is_none(), "—")
                        .clicked()
                    {
                        draft.environment = None;
                    }
                    for (id, name) in envs {
                        if ui
                            .selectable_label(draft.environment.as_deref() == Some(&id), name)
                            .clicked()
                        {
                            draft.environment = Some(id);
                        }
                    }
                });
            ui.end_row();
            form::label(ui, "");
            ui.checkbox(&mut draft.read_only, s.t("conn-read-only"));
            ui.end_row();
        });
    });
    let is_ssh = draft.kind == "ssh";
    let is_s3 = draft.kind == "s3";
    let is_db = draft.kind == "db";
    let is_redis = hs_core::connections::is_redis_kind(&draft.kind);
    if is_ssh {
        ssh_form(ui, theme, s, draft, fields_text, effects);
    } else if is_s3 {
        s3_form(ui, theme, s, draft, fields_text);
    } else if is_db {
        db_form(ui, theme, s, draft, fields_text);
    } else if is_redis {
        redis_form(ui, theme, s, draft, fields_text);
    } else if draft.kind == "webdav" {
        webdav_form(ui, theme, s, draft, fields_text);
    } else if draft.kind == "nexus" {
        nexus_form(ui, theme, s, draft, fields_text);
    } else if matches!(draft.kind.as_str(), "mongodb" | "couchdb" | "qdrant") {
        nosql_form(ui, theme, s, draft, fields_text);
    } else if hs_core::connections::is_container_kind(&draft.kind) {
        container_form(ui, theme, s, draft, fields_text);
    } else {
        form::section(ui, theme, &s.t("conn-fields"), |ui| {
            form::code_field(ui, fields_text, 8);
            form::hint(ui, theme, &s.t("conn-fields-hint"));
        });
    }
    // Testing is offered between Save and Delete: try it, read the
    // answer, then decide whether to keep the profile.
    let mut start_test = false;
    let mut do_save = false;
    let mut do_connect = false;
    ui.horizontal(|ui| {
        if form::primary_button(ui, theme, &s.t("save")).clicked() {
            do_save = true;
        }

        // A kind with no driver has nothing to open, so the button
        // is there but says why it cannot do anything.
        let can_test = crate::conn_test::testable(&draft.kind);
        let busy = state
            .conn_test
            .as_ref()
            .is_some_and(|t| t.profile_id == draft.id && t.running());
        let label = if busy {
            s.t("conn-test-running")
        } else {
            s.t("conn-test")
        };
        if ui
            .add_enabled(can_test && !busy, egui::Button::new(label))
            .tip_disabled(if can_test {
                s.t("conn-test-running")
            } else {
                s.t("conn-test-not-testable")
            })
            .clicked()
        {
            start_test = true;
        }
        // Connect saves first and then opens the activity's tab, so what
        // opens is the profile on the form rather than a stale copy of it.
        let can_open = crate::launcher::tool_for_kind(&draft.kind).is_some();
        if ui
            .add_enabled(can_open, egui::Button::new(s.t("conn-connect")))
            .tip(s.t("conn-connect-tip"))
            .tip_disabled(s.t("conn-connect-not-openable"))
            .clicked()
        {
            do_connect = true;
        }
        if form::danger_button(ui, theme, &s.t("delete")).clicked() {
            state.confirm_delete = Some((draft.id.clone(), draft.name.clone(), false));
        }
    });
    if do_save || do_connect {
        let saved = save_draft(s, draft, fields_text, effects);
        if do_connect
            && saved
            && let Some(tool) = crate::launcher::tool_for_kind(&draft.kind)
        {
            effects.push(SettingsEffect::OpenConnection {
                tool: tool.to_string(),
                id: draft.id.clone(),
            });
        }
    }
    if let Some(test) = &state.conn_test
        && test.profile_id == draft.id
    {
        ui.add_space(6.0);
        match test.outcome() {
            crate::conn_test::Outcome::Running => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.colored_label(theme.text_muted, s.t("conn-test-running"));
                });
            }
            crate::conn_test::Outcome::Ok(detail) => {
                let text = if detail.is_empty() {
                    s.t("conn-test-ok")
                } else {
                    s.i18n.t_args("conn-test-ok-detail", &[("detail", &detail)])
                };
                ui.colored_label(theme.success, format!("{} {text}", hs_ui::icons::CHECK));
            }
            crate::conn_test::Outcome::Failed(e) => {
                ui.colored_label(theme.error, format!("{} {e}", hs_ui::icons::WARNING));
            }
            crate::conn_test::Outcome::UnknownHostKey(fingerprint) => {
                ui.colored_label(
                    theme.warning,
                    s.i18n
                        .t_args("conn-test-host-key", &[("fingerprint", &fingerprint)]),
                );
            }
            crate::conn_test::Outcome::NotTestable => {
                form::hint(ui, theme, &s.t("conn-test-not-testable"));
            }
            // Nothing was tried: say what is missing, not what a
            // driver would have said.
            crate::conn_test::Outcome::VaultLocked => {
                ui.colored_label(theme.warning, s.t("conn-test-vault-locked"));
            }
            crate::conn_test::Outcome::MissingVariables(names) => {
                ui.colored_label(
                    theme.warning,
                    s.i18n.t_args("conn-test-missing", &[("names", &names)]),
                );
            }
            crate::conn_test::Outcome::NoPassword => {
                ui.colored_label(theme.warning, s.t("conn-test-no-password"));
            }
            crate::conn_test::Outcome::NoKeys => {
                ui.colored_label(theme.warning, s.t("conn-test-no-keys"));
            }
            crate::conn_test::Outcome::UnknownEngine(engine) => {
                ui.colored_label(
                    theme.error,
                    s.i18n.t_args("db-unknown-engine", &[("engine", &engine)]),
                );
            }
        }
    }
    if start_test {
        // Test what is on the form, not what was last saved.
        let mut probe = draft.clone();
        if !matches!(probe.kind.as_str(), "ssh" | "s3" | "db")
            && let Ok(table) = toml::from_str::<toml::Table>(fields_text)
        {
            probe.fields = table;
        }
        state.conn_test = Some(crate::conn_test::start(s, &probe));
    }
}

fn security(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-security"),
        Some(&s.t("settings-security-sub")),
    );
    form::section(ui, theme, &s.t("settings-vault-status"), |ui| {
        ui.horizontal(|ui| {
            if s.secrets.is_locked() {
                ui.colored_label(
                    theme.warning,
                    format!("{} {}", hs_ui::icons::LOCK, s.t("settings-vault-locked")),
                );
            } else {
                ui.colored_label(
                    theme.success,
                    format!(
                        "{} {}",
                        hs_ui::icons::UNLOCK,
                        s.t("settings-vault-unlocked")
                    ),
                );
                if ui.button(s.t("settings-vault-lock")).clicked() {
                    effects.push(SettingsEffect::LockVault);
                }
                if ui.button(s.t("settings-vault-change")).clicked() {
                    effects.push(SettingsEffect::ChangeMasterPassword);
                }
            }
        });
        form::hint(ui, theme, &s.t("settings-vault-hint"));
    });
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-timers"), |ui| {
        form::field_grid(ui, "security-timers", |ui| {
            form::label(ui, &s.t("settings-vault-relock"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.security.relock_after_minutes)
                        .range(0..=600),
                )
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-clipboard-clear"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.security.clear_clipboard_after_seconds)
                        .range(0..=600),
                )
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-timers-hint"));
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
    drop(settings);
    credentials_section(ui, theme, s, state, effects);
}

/// How long one revealed password stays uncovered. Short on purpose: the
/// reveal is for reading a value once, not for leaving it on the screen.
const REVEAL_SECONDS: f64 = 30.0;

fn credentials_section(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let now = ui.input(|i| i.time);
    if let Some((_, until)) = &state.reveal {
        if now >= *until {
            state.reveal = None;
        } else {
            // The lapse is a promise, so it cannot wait for the next
            // keystroke: ask for the frame that will cover it again.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }
    }
    let rows = crate::credentials::rows(&s.connections.read());
    form::section(ui, theme, &s.t("settings-credentials"), |ui| {
        form::hint(ui, theme, &s.t("settings-credentials-hint"));
        if rows.is_empty() {
            ui.add_space(4.0);
            form::note(ui, theme, hs_ui::icons::INFO, &s.t("conn-none"));
            return;
        }
        ui.add_space(6.0);
        // Username and Password share one width — the wider of the two —
        // so the two columns a reader compares line up instead of jumping
        // about with the length of each value. Every other column is left
        // to the grid, which sizes it to its own content.
        let pair_width = credentials_pair_width(ui, s, &rows, state, now);
        // Eight columns do not fit a settings page, and squeezing them
        // would hide the values this page exists to show.
        egui::ScrollArea::horizontal()
            .id_salt("sec-cred-scroll")
            .show(ui, |ui| {
                egui::Grid::new("sec-cred-grid")
                    .num_columns(8)
                    .spacing([18.0, 6.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for key in [
                            "settings-cred-name",
                            "settings-cred-type",
                            "settings-cred-host",
                            "settings-cred-port",
                            "settings-cred-user",
                            "settings-cred-password",
                            "settings-cred-key",
                            "settings-cred-actions",
                        ] {
                            ui.label(
                                egui::RichText::new(s.t(key))
                                    .color(theme.text_muted)
                                    .small(),
                            );
                        }
                        ui.end_row();
                        for r in &rows {
                            ui.label(&r.name);
                            ui.label(hs_core::connections::kind_label(&r.kind));
                            cred_cell(ui, s, theme, &r.host, None, effects);
                            ui.label(&r.port);
                            cred_cell(ui, s, theme, &r.user, Some(pair_width), effects);
                            secret_cell(
                                ui,
                                s,
                                theme,
                                state,
                                r,
                                &r.password,
                                now,
                                pair_width,
                                effects,
                            );
                            // A key-auth host shows its passphrase here and
                            // keeps the file path as the tooltip; every
                            // other row shows the path (or nothing).
                            if r.key_secret == crate::credentials::Password::None {
                                cred_cell(ui, s, theme, &r.key, None, effects);
                            } else {
                                secret_cell(
                                    ui,
                                    s,
                                    theme,
                                    state,
                                    r,
                                    &r.key_secret,
                                    now,
                                    pair_width,
                                    effects,
                                )
                                .tip(if r.key.is_empty() {
                                    s.t("settings-cred-key-passphrase")
                                } else {
                                    r.key.clone()
                                });
                            }
                            actions_cell(ui, s, state, r, now);
                            ui.end_row();
                        }
                    });
            });
    });
    reveal_dialog(ui, theme, s, state, now);
    edit_secret_dialog(ui, theme, s, state, effects);
}

/// How wide the Username and Password columns both are: whatever the
/// widest value in either needs. Measured rather than guessed, so a page of
/// short names does not leave two narrow columns beside a long one.
fn credentials_pair_width(
    ui: &egui::Ui,
    s: &Services,
    rows: &[crate::credentials::Row],
    state: &SettingsState,
    now: f64,
) -> f32 {
    use crate::credentials::Password;
    let body = egui::TextStyle::Body.resolve(ui.style());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    let painter = ui.painter();
    let width = |text: &str, font: &egui::FontId| {
        painter
            .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE)
            .rect
            .width()
    };
    let mut widest = width(&s.t("settings-cred-password"), &body);
    for r in rows {
        widest = widest.max(width(&r.user, &body));
        let revealed = state
            .reveal
            .as_ref()
            .is_some_and(|(id, until)| id == &r.id && now < *until);
        let shown = match (&r.password, revealed) {
            (Password::None, _) => "—".to_string(),
            (Password::Variable(v), _) => v.clone(),
            (Password::Plain(p), true) => p.clone(),
            (Password::Vault(_), true) => s
                .secrets
                .get(match &r.password {
                    Password::Vault(k) => k,
                    _ => unreachable!(),
                })
                .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
                .unwrap_or_else(|_| s.t("settings-cred-missing")),
            _ => MASK.to_string(),
        };
        widest = widest.max(width(&shown, &mono));
        // The Key column shares the same width, so a key passphrase is
        // measured too.
        let key_shown = match (&r.key_secret, revealed) {
            (Password::None, _) => String::new(),
            (Password::Variable(v), _) => v.clone(),
            (Password::Plain(p), true) => p.clone(),
            (Password::Vault(_), true) => s
                .secrets
                .get(match &r.key_secret {
                    Password::Vault(k) => k,
                    _ => unreachable!(),
                })
                .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
                .unwrap_or_else(|_| s.t("settings-cred-missing")),
            _ => MASK.to_string(),
        };
        if !key_shown.is_empty() {
            widest = widest.max(width(&key_shown, &mono));
        }
    }
    widest
}

/// The masked form of a stored password. Fixed width on purpose: its length
/// must say nothing about the password's.
const MASK: &str = "••••••••";

/// A plain cell: the value, and a button that copies it. `width` pins the
/// value's width when the column is one of the pair that share one.
fn cred_cell(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    value: &str,
    width: Option<f32>,
    effects: &mut Vec<SettingsEffect>,
) -> egui::Response {
    ui.horizontal(|ui| {
        let show = |ui: &mut egui::Ui| {
            if value.is_empty() {
                ui.label(egui::RichText::new("—").color(theme.text_muted));
            } else {
                ui.add(egui::Label::new(value).selectable(false).truncate());
            }
        };
        match width {
            Some(w) => {
                ui.scope(|ui| {
                    ui.set_min_width(w);
                    ui.set_max_width(w);
                    show(ui);
                });
            }
            None => show(ui),
        }
        if value.is_empty() {
            return;
        }
        if ui
            .small_button(hs_ui::icons::COPY)
            .tip(s.t("dt-copy"))
            .clicked()
        {
            ui.ctx().copy_text(value.to_string());
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Info,
                s.t("settings-cred-copied"),
            ));
        }
    })
    .response
}

#[allow(clippy::too_many_arguments)] // one cell, one argument each.
fn secret_cell(
    ui: &mut egui::Ui,
    s: &Services,
    theme: &Theme,
    state: &mut SettingsState,
    row: &crate::credentials::Row,
    secret: &crate::credentials::Password,
    now: f64,
    width: f32,
    effects: &mut Vec<SettingsEffect>,
) -> egui::Response {
    use crate::credentials::Password;
    match secret {
        Password::None => return cred_cell(ui, s, theme, "", Some(width), effects),
        // A variable names an environment value; masking it would hide
        // which one, which is the only thing worth reading here.
        Password::Variable(v) => return cred_cell(ui, s, theme, v, Some(width), effects),
        _ => {}
    }
    let revealed = state
        .reveal
        .as_ref()
        .is_some_and(|(id, until)| id == &row.id && now < *until);
    // Read the value out of the vault for this frame only; nothing keeps
    // plaintext while the page is open.
    let shown = revealed.then(|| match secret {
        // A reference the vault does not hold says so; an empty cell
        // would read as "the secret is blank".
        Password::Vault(r) => s
            .secrets
            .get(r)
            .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
            .map_err(|_| s.t("settings-cred-missing")),
        Password::Plain(p) => Ok(p.clone()),
        _ => Ok(String::new()),
    });
    ui.horizontal(|ui| {
        ui.scope(|ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            match &shown {
                Some(Ok(v)) => {
                    ui.add(
                        egui::Label::new(egui::RichText::new(v).monospace())
                            .selectable(false)
                            .truncate(),
                    );
                }
                Some(Err(e)) => {
                    ui.colored_label(theme.error, e);
                }
                None => {
                    ui.label(
                        egui::RichText::new(MASK)
                            .monospace()
                            .color(theme.text_muted),
                    );
                }
            }
        });
        if ui
            .small_button(hs_ui::icons::COPY)
            .tip(s.t("dt-copy"))
            .clicked()
        {
            match &shown {
                // Copying is a reveal by another route, so it is gated the
                // same way.
                None => {
                    state.ask = Some(Ask {
                        row: row.id.clone(),
                        edit: false,
                        typed: String::new(),
                        error: None,
                    });
                }
                Some(Ok(v)) => effects.push(SettingsEffect::CopySecret(v.clone())),
                // Nothing to copy when the vault does not have it.
                Some(Err(_)) => {}
            }
        }
    })
    .response
}

/// The Action column: Reveal Secret and Edit Secret, icons only, with space
/// between them (owner's request, 2026-10-19).
fn actions_cell(
    ui: &mut egui::Ui,
    s: &Services,
    state: &mut SettingsState,
    row: &crate::credentials::Row,
    now: f64,
) {
    let revealed = state
        .reveal
        .as_ref()
        .is_some_and(|(id, until)| id == &row.id && now < *until);
    ui.horizontal(|ui| {
        let eye = if revealed {
            hs_ui::icons::EYE_SLASH
        } else {
            hs_ui::icons::EYE
        };
        if ui
            .small_button(eye)
            .tip(if revealed {
                s.t("settings-cred-hide")
            } else {
                s.t("settings-cred-reveal")
            })
            .clicked()
        {
            if revealed {
                state.reveal = None;
            } else {
                state.ask = Some(Ask {
                    row: row.id.clone(),
                    edit: false,
                    typed: String::new(),
                    error: None,
                });
            }
        }
        ui.add_space(8.0);
        if ui
            .small_button(hs_ui::icons::EDIT)
            .tip(s.t("settings-cred-edit"))
            .clicked()
        {
            state.ask = Some(Ask {
                row: row.id.clone(),
                edit: true,
                typed: String::new(),
                error: None,
            });
        }
    });
}

/// The value of a secret as text, for the edit dialog only: the master
/// password has just been accepted, so reading it is the point.
fn read_secret(s: &Services, secret: &crate::credentials::Password) -> String {
    use crate::credentials::Password;
    match secret {
        Password::None => String::new(),
        Password::Variable(v) => v.clone(),
        Password::Plain(p) => p.clone(),
        Password::Vault(r) => s
            .secrets
            .get(r)
            .map(|v| secrecy::ExposeSecret::expose_secret(&v).to_string())
            .unwrap_or_default(),
    }
}

/// The master-password prompt. Revealing or editing a stored secret asks for
/// it every time: the vault being unlocked is not the same as the person at
/// the keyboard being the owner.
fn reveal_dialog(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    now: f64,
) {
    let (id, edit, unlocked, close) = {
        let Some(ask) = &mut state.ask else {
            return;
        };
        let mut close = false;
        let mut accept = false;
        let modal = egui::Modal::new(egui::Id::new("hs-cred-reveal")).show(ui.ctx(), |ui| {
            ui.set_width(380.0);
            ui.heading(s.t(if ask.edit {
                "settings-cred-edit"
            } else {
                "settings-cred-reveal"
            }));
            ui.add_space(4.0);
            let hint = if ask.edit {
                "settings-cred-edit-ask-hint"
            } else {
                "settings-cred-reveal-hint"
            };
            form::hint(ui, theme, &s.t(hint));
            ui.add_space(6.0);
            let field = form::secret_field(
                ui,
                &mut ask.typed,
                &s.t("settings-cred-master-password"),
                340.0,
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                accept = true;
            }
            if ui.memory(|m| m.focused().is_none()) {
                field.request_focus();
            }
            if let Some(e) = &ask.error {
                ui.add_space(4.0);
                ui.colored_label(theme.error, e);
            }
            ui.add_space(10.0);
            // Right-aligned with the action outermost, as every other dialog
            // in the app lays its buttons out.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let go = if ask.edit {
                    "settings-cred-edit-go"
                } else {
                    "settings-cred-reveal-go"
                };
                if form::primary_button(ui, theme, &s.t(go)).clicked() {
                    accept = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    close = true;
                }
            });
        });
        if modal.should_close() {
            close = true;
        }
        let mut unlocked = false;
        if accept {
            let master = secrecy::SecretString::from(ask.typed.clone());
            match s.secrets.unlock(&master) {
                Ok(()) => {
                    unlocked = true;
                    close = true;
                }
                Err(_) => ask.error = Some(s.t("settings-cred-wrong-password")),
            }
        }
        (ask.row.clone(), ask.edit, unlocked, close)
    };
    if unlocked {
        let row = crate::credentials::rows(&s.connections.read())
            .into_iter()
            .find(|r| r.id == id);
        if let Some(r) = row {
            if edit {
                state.edit_secret = Some(EditSecret {
                    row: r.id.clone(),
                    name: r.name.clone(),
                    password: read_secret(s, &r.password),
                    key: read_secret(s, &r.key_secret),
                    key_editable: r.key_secret != crate::credentials::Password::None,
                    error: None,
                });
                state.reveal = None;
            } else {
                state.reveal = Some((r.id.clone(), now + REVEAL_SECONDS));
            }
        }
    }
    if close {
        state.ask = None;
    }
}

/// Edit the password and key of one row. The master password has already
/// been accepted; typed text moves into the vault on Save.
fn edit_secret_dialog(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let Some(edit) = &mut state.edit_secret else {
        return;
    };
    let id = edit.row.clone();
    let name = edit.name.clone();
    let key_editable = edit.key_editable;
    let mut close = false;
    let mut save = false;
    let modal = egui::Modal::new(egui::Id::new("hs-cred-edit")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.heading(
            s.i18n
                .t_args("settings-cred-edit-title", &[("name", &name)]),
        );
        ui.add_space(4.0);
        form::hint(ui, theme, &s.t("settings-cred-edit-hint"));
        ui.add_space(6.0);
        form::field_grid(ui, "cred-edit-fields", |ui| {
            // A key-auth host has no password of its own; the passphrase is
            // the secret, so only that field is shown.
            if !key_editable {
                form::label(ui, &s.t("settings-cred-password"));
                form::secret_field(
                    ui,
                    &mut edit.password,
                    &s.t("settings-cred-password"),
                    320.0,
                );
                ui.end_row();
            }
            if key_editable {
                form::label(ui, &s.t("settings-cred-key-passphrase"));
                form::secret_field(
                    ui,
                    &mut edit.key,
                    &s.t("settings-cred-key-passphrase"),
                    320.0,
                );
                ui.end_row();
            }
        });
        if let Some(e) = &edit.error {
            ui.add_space(4.0);
            ui.colored_label(theme.error, e);
        }
        ui.add_space(10.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if form::primary_button(ui, theme, &s.t("save")).clicked() {
                save = true;
            }
            if ui.button(s.t("cancel")).clicked() {
                close = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if save {
        let (mut password, mut key) = (edit.password.clone(), edit.key.clone());
        if apply_secret_edit(s, &id, &mut password, &mut key, key_editable, effects) {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Success,
                s.t("settings-cred-saved"),
            ));
            close = true;
        } else {
            edit.error = Some(s.t("settings-cred-save-failed"));
        }
    }
    if close {
        state.edit_secret = None;
    }
}

/// Write the edited secret into the profile and its vault. Only the field
/// the dialog showed is touched, so editing a password cannot clear a key
/// passphrase. Returns false when the vault refused the value, in which
/// case nothing is saved.
fn apply_secret_edit(
    s: &Services,
    id: &str,
    password: &mut String,
    key: &mut String,
    key_editable: bool,
    effects: &mut Vec<SettingsEffect>,
) -> bool {
    let Some(profile) = s.connections.read().by_id(id).cloned() else {
        return false;
    };
    let mut table = profile.fields.clone();
    let ok = match profile.kind.as_str() {
        "ssh" => {
            let mut f = hs_ssh::SshFields::from_table(&table);
            let ok = if key_editable {
                f.key_passphrase = std::mem::take(key);
                store_typed_secrets(
                    s,
                    &profile.name,
                    [("key passphrase", &mut f.key_passphrase)],
                    effects,
                )
            } else {
                f.password = std::mem::take(password);
                store_typed_secrets(s, &profile.name, [("password", &mut f.password)], effects)
            };
            f.write_into(&mut table);
            ok
        }
        "s3" => {
            let mut f = S3Fields::from_table(&table);
            f.secret_key = std::mem::take(password);
            let ok = store_typed_secrets(
                s,
                &profile.name,
                [("secret key", &mut f.secret_key)],
                effects,
            );
            f.write_into(&mut table);
            ok
        }
        "db" => {
            let mut f = DbFields::from_table(&table);
            f.password = std::mem::take(password);
            let ok =
                store_typed_secrets(s, &profile.name, [("password", &mut f.password)], effects);
            f.write_into(&mut table);
            ok
        }
        "redis" => {
            let mut f = tool_redis::profile::RedisFields::from_table(&table);
            f.password = std::mem::take(password);
            let ok =
                store_typed_secrets(s, &profile.name, [("password", &mut f.password)], effects);
            f.write_into(&mut table);
            ok
        }
        "mongodb" | "couchdb" | "qdrant" => {
            let mut f = tool_nosql::profile::NoSqlFields::from_table(&profile.kind, &table);
            f.password = std::mem::take(password);
            let ok =
                store_typed_secrets(s, &profile.name, [("password", &mut f.password)], effects);
            f.write_into(&mut table);
            ok
        }
        _ => return false,
    };
    if !ok {
        return false;
    }
    let mut updated = profile;
    updated.fields = table;
    match s.connections.write().upsert(updated) {
        Ok(()) => {
            effects.push(SettingsEffect::ConnectionsChanged);
            true
        }
        Err(e) => {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            ));
            false
        }
    }
}

/// Commands the Terminal page offers when they are found on PATH:
/// `(command, display name, icon key)`.
const CLI_CANDIDATES: &[(&str, &str, &str)] = &[
    ("mysql", "MySQL CLI", "database"),
    ("psql", "PostgreSQL CLI", "database"),
    ("sqlite3", "SQLite CLI", "database"),
    ("mongosh", "MongoDB Shell", "database"),
    ("redis-cli", "Redis CLI", "database"),
    ("aws", "AWS CLI", "cloud"),
    ("kubectl", "kubectl", "cloud"),
    ("docker", "Docker", "cloud"),
    ("claude", "Claude Code", "sparkle"),
    ("opencode", "OpenCode", "sparkle"),
    ("gemini", "Gemini CLI", "sparkle"),
    ("aider", "Aider", "sparkle"),
    ("ollama", "Ollama", "sparkle"),
];

/// The Phosphor icon for a stored icon key.
fn cli_icon(key: &str) -> &'static str {
    use hs_ui::icons::ph;
    match key {
        "database" => ph::DATABASE,
        "cloud" => ph::CLOUD,
        "sparkle" => ph::SPARKLE,
        _ => ph::TERMINAL_WINDOW,
    }
}

fn cli_icon_label(key: &str, s: &Services) -> String {
    let text = match key {
        "database" => s.t("settings-terminal-icon-database"),
        "cloud" => s.t("settings-terminal-icon-cloud"),
        "sparkle" => s.t("settings-terminal-icon-ai"),
        _ => s.t("settings-terminal-icon-terminal"),
    };
    format!("{} {text}", cli_icon(key))
}

/// The detected shells in the stored order, then any newly detected ones.
fn ordered_shell_ids(profiles: &[hs_term::shells::ShellProfile], saved: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for id in saved {
        if profiles.iter().any(|p| p.id == *id) && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for p in profiles {
        if !ids.contains(&p.id) {
            ids.push(p.id.clone());
        }
    }
    ids
}

/// Candidate commands found on PATH and not already in the list.
fn cli_suggestions(settings: &hs_core::settings::Settings) -> Vec<String> {
    CLI_CANDIDATES
        .iter()
        .filter(|(command, _, _)| {
            !settings
                .terminal
                .cli
                .iter()
                .any(|c| c.command.eq_ignore_ascii_case(command))
        })
        .filter(|(command, _, _)| hs_core::env_path::find(command).is_some())
        .map(|(command, _, _)| (*command).to_string())
        .collect()
}

fn terminal(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-terminal"),
        Some(&s.t("settings-terminal-sub")),
    );
    let profiles = hs_term::shells::profiles();
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-shell"), |ui| {
        form::field_grid(ui, "terminal-shell", |ui| {
            form::label(ui, &s.t("settings-terminal-default-profile"));
            let current = profiles
                .iter()
                .find(|p| p.id == settings.terminal.default_profile)
                .or(profiles.first())
                .map(|p| p.name.clone())
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("term-default")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for p in profiles.iter() {
                        if ui
                            .selectable_label(settings.terminal.default_profile == p.id, &p.name)
                            .tip(&p.exe)
                            .clicked()
                        {
                            settings.terminal.default_profile = p.id.clone();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("settings-terminal-scrollback"));
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.terminal.scrollback)
                        .range(100..=200_000)
                        .speed(100),
                )
                .changed();
            ui.end_row();
        });
    });
    form::section(ui, theme, &s.t("settings-section-terminal-font"), |ui| {
        form::field_grid(ui, "terminal-font", |ui| {
            form::label(ui, &s.t("settings-terminal-font"));
            if font_combo(
                ui,
                s,
                "terminal-font",
                &mut settings.terminal.font_family,
                true,
                "",
            ) {
                effects.push(SettingsEffect::FontsChanged);
            }
            ui.end_row();
            form::label(ui, &s.t("settings-terminal-font-size"));
            changed |= ui
                .add(egui::Slider::new(&mut settings.terminal.font_size, 9.0..=24.0).suffix(" px"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-terminal-font-size-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-behaviour"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.terminal.copy_on_select,
                s.t("settings-terminal-copy-on-select"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.terminal.right_click_copy_paste,
                s.t("settings-terminal-right-click"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-terminal-right-click-hint"));
        changed |= ui
            .checkbox(
                &mut settings.terminal.link_detection,
                s.t("settings-terminal-links"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-terminal-links-hint"));
    });
    form::section(ui, theme, &s.t("settings-terminal-list"), |ui| {
        form::hint(ui, theme, &s.t("settings-terminal-list-hint"));
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(s.t("settings-terminal-shells"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        let ids = ordered_shell_ids(&profiles, &settings.terminal.shell_order);
        let mut move_shell: Option<(usize, usize)> = None;
        for (i, id) in ids.iter().enumerate() {
            let Some(profile) = profiles.iter().find(|p| p.id == *id) else {
                continue;
            };
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(i > 0, egui::Button::new(hs_ui::icons::ph::ARROW_UP))
                    .clicked()
                {
                    move_shell = Some((i, i - 1));
                }
                if ui
                    .add_enabled(
                        i + 1 < ids.len(),
                        egui::Button::new(hs_ui::icons::ph::ARROW_DOWN),
                    )
                    .clicked()
                {
                    move_shell = Some((i, i + 1));
                }
                ui.label(&profile.name);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&profile.exe)
                            .small()
                            .color(theme.text_muted),
                    )
                    .truncate()
                    .selectable(false),
                );
            });
        }
        if let Some((a, b)) = move_shell {
            let mut order = ids;
            order.swap(a, b);
            settings.terminal.shell_order = order;
            changed = true;
        }

        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(s.t("settings-terminal-cli"))
                .small()
                .strong()
                .color(theme.text_muted),
        );
        if settings.terminal.cli.is_empty() {
            form::hint(ui, theme, &s.t("settings-terminal-cli-empty"));
        }
        let count = settings.terminal.cli.len();
        let mut move_cli: Option<(usize, usize)> = None;
        let mut edit_cli: Option<String> = None;
        let mut remove_cli: Option<String> = None;
        for i in 0..count {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(i > 0, egui::Button::new(hs_ui::icons::ph::ARROW_UP))
                    .clicked()
                {
                    move_cli = Some((i, i - 1));
                }
                if ui
                    .add_enabled(
                        i + 1 < count,
                        egui::Button::new(hs_ui::icons::ph::ARROW_DOWN),
                    )
                    .clicked()
                {
                    move_cli = Some((i, i + 1));
                }
                changed |= ui
                    .checkbox(&mut settings.terminal.cli[i].enabled, "")
                    .changed();
                ui.label(&settings.terminal.cli[i].name);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(settings.terminal.cli[i].command_line())
                            .small()
                            .monospace()
                            .color(theme.text_muted),
                    )
                    .truncate()
                    .selectable(false),
                );
                let id = settings.terminal.cli[i].id.clone();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(hs_ui::icons::ph::TRASH).clicked() {
                        remove_cli = Some(id.clone());
                    }
                    if ui.small_button(hs_ui::icons::ph::PENCIL_SIMPLE).clicked() {
                        edit_cli = Some(id.clone());
                    }
                });
            });
        }
        if let Some((a, b)) = move_cli {
            settings.terminal.cli.swap(a, b);
            changed = true;
        }
        if let Some(id) = edit_cli
            && let Some(profile) = settings.terminal.cli.iter().find(|c| c.id == id)
        {
            state.cli_draft = Some(profile.clone());
        }
        if let Some(id) = remove_cli {
            state.cli_confirm = Some(id);
        }
        if let Some(id) = state.cli_confirm.clone() {
            let name = settings
                .terminal
                .cli
                .iter()
                .find(|c| c.id == id)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.label(
                    s.i18n
                        .t_args("settings-terminal-remove-cli", &[("name", &name)]),
                );
                if ui.button(s.t("delete")).clicked() {
                    settings.terminal.cli.retain(|c| c.id != id);
                    state.cli_confirm = None;
                    changed = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    state.cli_confirm = None;
                }
            });
        }
        ui.add_space(4.0);
        if ui
            .button(format!(
                "{} {}",
                hs_ui::icons::ph::PLUS,
                s.t("settings-terminal-add-cli")
            ))
            .clicked()
        {
            state.cli_draft = Some(hs_core::settings::CliProfile::default());
        }
        let mut save = false;
        let mut cancel = false;
        if let Some(draft) = &mut state.cli_draft {
            ui.separator();
            ui.label(egui::RichText::new(s.t("settings-terminal-cli-edit")).strong());
            form::field_grid(ui, "cli-editor", |ui| {
                form::label(ui, &s.t("settings-terminal-cli-name"));
                form::text_field(ui, &mut draft.name, &s.t("settings-terminal-cli-name"));
                ui.end_row();
                form::label(ui, &s.t("settings-terminal-cli-command"));
                form::text_field(ui, &mut draft.command, "mysql");
                ui.end_row();
                form::label(ui, &s.t("settings-terminal-cli-args"));
                form::text_field(ui, &mut draft.args, "-u root");
                ui.end_row();
                form::label(ui, &s.t("settings-terminal-cli-cwd"));
                form::text_field(ui, &mut draft.cwd, "{{workspace}}");
                ui.end_row();
                form::label(ui, &s.t("settings-terminal-cli-icon"));
                egui::ComboBox::from_id_salt("cli-icon")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(cli_icon_label(&draft.icon, s))
                    .show_ui(ui, |ui| {
                        for key in ["terminal", "database", "cloud", "sparkle"] {
                            if ui
                                .selectable_label(draft.icon == key, cli_icon_label(key, s))
                                .clicked()
                            {
                                draft.icon = key.to_string();
                            }
                        }
                    });
                ui.end_row();
            });
            changed |= ui
                .checkbox(&mut draft.enabled, s.t("settings-terminal-cli-enabled"))
                .changed();
            ui.horizontal(|ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    save = true;
                }
                if ui.button(s.t("cancel")).clicked() {
                    cancel = true;
                }
            });
        }
        if save && let Some(mut draft) = state.cli_draft.take() {
            if draft.id.is_empty() {
                draft.id = hs_core::new_id();
                settings.terminal.cli.push(draft);
            } else if let Some(existing) =
                settings.terminal.cli.iter_mut().find(|c| c.id == draft.id)
            {
                *existing = draft;
            }
            changed = true;
        }
        if cancel {
            state.cli_draft = None;
        }
    });
    form::section(ui, theme, &s.t("settings-terminal-suggestions"), |ui| {
        form::hint(ui, theme, &s.t("settings-terminal-suggestions-hint"));
        if state.cli_suggestions.is_none() {
            state.cli_suggestions = Some(cli_suggestions(&settings));
        }
        let suggestions = state.cli_suggestions.clone().unwrap_or_default();
        if suggestions.is_empty() {
            form::hint(ui, theme, &s.t("settings-terminal-suggestions-none"));
        }
        let mut add: Option<String> = None;
        for command in &suggestions {
            let (label, icon) = CLI_CANDIDATES
                .iter()
                .find(|(c, _, _)| c == command)
                .map(|(_, l, i)| (*l, *i))
                .unwrap_or((command.as_str(), "terminal"));
            ui.horizontal(|ui| {
                ui.label(cli_icon(icon));
                ui.label(label);
                ui.label(
                    egui::RichText::new(command)
                        .monospace()
                        .small()
                        .color(theme.text_muted),
                );
                if ui.button(s.t("settings-terminal-add")).clicked() {
                    add = Some(command.clone());
                }
            });
        }
        if let Some(command) = add {
            let (label, icon) = CLI_CANDIDATES
                .iter()
                .find(|(c, _, _)| *c == command)
                .map(|(_, l, i)| (*l, *i))
                .unwrap_or((command.as_str(), "terminal"));
            settings.terminal.cli.push(hs_core::settings::CliProfile {
                id: hs_core::new_id(),
                name: label.to_string(),
                command,
                icon: icon.to_string(),
                enabled: false,
                ..Default::default()
            });
            state.cli_suggestions = None;
            changed = true;
        }
        if ui.button(s.t("settings-terminal-refresh")).clicked() {
            // A CLI installed while the app is open is not in the inherited
            // PATH; re-read the system value before scanning again.
            hs_core::env_path::refresh();
            state.cli_suggestions = None;
        }
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// A project being edited in Settings -> Editor -> Project List.
///
/// The Git identity is read from the repository when the row is opened
/// and written back on Save, so this struct - not the stored entry - is
/// what the form binds to.
#[derive(Debug, Clone)]
pub struct ProjectDraft {
    pub entry: hs_core::projects::ProjectEntry,
    /// The repository's own `--local` configuration, as it reads right
    /// now. Saved back key by key; an empty field unsets rather than
    /// blanks, so the repository inherits the global value again.
    pub git: hs_git::write::RepoConfig,
    /// Whether the folder is a git repository at all; when it is not,
    /// the identity fields are shown disabled rather than hidden, so the
    /// reason is visible.
    pub is_repo: bool,
}

impl ProjectDraft {
    /// A draft for a project being **added**: an empty form in the
    /// detail column, exactly as New Connection gives one.
    ///
    /// Nothing is written until Save, so a folder picked and then
    /// thought better of leaves no entry behind.
    pub fn blank() -> Self {
        Self {
            entry: hs_core::projects::ProjectEntry::default(),
            git: hs_git::write::RepoConfig::default(),
            is_repo: false,
        }
    }

    /// Public so the shell can open this form from the Editor's sidebar
    /// (Edit This Project Settings).
    pub fn open(entry: hs_core::projects::ProjectEntry) -> Self {
        let is_repo = entry.root.join(".git").exists();
        // Reading two config keys is two short `git` runs. That is fine
        // on a click; it would not be fine every frame, which is why it
        // happens here and not in the form.
        let git = if is_repo {
            hs_git::write::repo_config(&entry.root)
        } else {
            hs_git::write::RepoConfig::default()
        };
        Self {
            entry,
            git,
            is_repo,
        }
    }
}

/// Settings -> Editor -> **Project List**: every folder the Editor
/// activity offers, in the order its sidebar shows them.
///
/// This is the same list the activity's sidebar draws, from the same
/// store, so adding or reordering here is immediately what the sidebar
/// does (owner's requirement, 2026-10-26).
fn project_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::section(ui, theme, &s.t("settings-section-project-list"), |ui| {
        // Add leads the card, as New Connection leads the connections
        // list, and opens an empty form in the detail column rather than
        // a file dialog: the folder is one field of several, and picking
        // it first put the cart before the horse (owner's requirement,
        // 2026-10-27).
        if form::primary_button(
            ui,
            theme,
            &format!("{} {}", hs_ui::icons::ph::PLUS, s.t("settings-project-add")),
        )
        .clicked()
        {
            state.project_selected = None;
            state.project_draft = Some(ProjectDraft::blank());
        }
        ui.add_space(8.0);
        form::hint(ui, theme, &s.t("settings-project-list-hint"));
        let rows: Vec<(String, String, String, &'static str)> = s
            .projects
            .read()
            .all()
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    p.label(),
                    p.root.display().to_string(),
                    hs_ui::icons::language_icon(&p.language),
                )
            })
            .collect();
        if rows.is_empty() {
            form::hint(ui, theme, &s.t("settings-project-list-none"));
        }
        let mut reorder = hs_ui::reorder::Reorder::new("project-list-reorder");
        let mut pick: Option<String> = None;
        for (i, (id, name, root, icon)) in rows.iter().enumerate() {
            let selected = state.project_selected.as_deref() == Some(id.as_str());
            reorder.row(ui, i, |ui| {
                if form::list_row(ui, theme, selected, icon, name, Some(root)).clicked() {
                    pick = Some(id.clone());
                }
            });
            ui.add_space(2.0);
        }
        if let Some((from, to)) = reorder.finish(ui, theme.accent)
            && let Err(e) = s.projects.write().reorder(from, to)
        {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            ));
        }
        if let Some(id) = pick {
            state.project_selected = Some(id.clone());
            if let Some(p) = s.projects.read().by_id(&id) {
                state.project_draft = Some(ProjectDraft::open(p.clone()));
            }
        }
    });
    if state.project_draft.is_some() {
        ui.add_space(8.0);
        project_detail(ui, theme, s, state, effects);
    }
    ui.add_space(12.0);
}

/// One project's settings: its name, the folder it refers to, and the
/// identity its commits carry.
fn project_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let Some(draft) = state.project_draft.as_mut() else {
        return;
    };
    let mut save = false;
    let mut remove = false;
    form::section(ui, theme, &draft.entry.label(), |ui| {
        form::field_grid(ui, "project-detail", |ui| {
            form::label(ui, &s.t("settings-project-name"));
            form::text_field(
                ui,
                &mut draft.entry.name,
                &s.t("settings-project-name-hint"),
            );
            ui.end_row();

            form::label(ui, &s.t("settings-project-root"));
            ui.horizontal(|ui| {
                let mut root = draft.entry.root.display().to_string();
                if form::text_field(ui, &mut root, "").changed() {
                    draft.entry.root = std::path::PathBuf::from(root);
                }
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FOLDER_OPEN,
                        s.t("browse")
                    ))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    draft.entry.root = p;
                    // A different folder is a different project: its
                    // language and its repository identity both have to
                    // be read again rather than carried over.
                    draft.entry.language =
                        hs_core::projects::detect_language(&draft.entry.root).to_string();
                    let re = ProjectDraft::open(draft.entry.clone());
                    draft.git = re.git;
                    draft.is_repo = re.is_repo;
                }
            });
            ui.end_row();

            // Detection reads the manifest, which is right most of the
            // time and wrong for a folder that carries none or carries
            // two. The picker starts on whatever was detected and the
            // choice sticks: it is what the tab icon, the sidebar row
            // and the header band all read (owner's requirement,
            // 2026-10-27). "Detect" puts it back.
            form::label(ui, &s.t("settings-project-language"));
            let current = if draft.entry.language.is_empty() {
                s.t("settings-project-language-auto")
            } else {
                let name = hs_core::projects::language_name(&draft.entry.language);
                if name.is_empty() {
                    draft.entry.language.clone()
                } else {
                    name.to_string()
                }
            };
            egui::ComboBox::from_id_salt("project-language")
                .selected_text(format!(
                    "{} {current}",
                    hs_ui::icons::language_icon(&draft.entry.language)
                ))
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut draft.entry.language,
                        String::new(),
                        s.t("settings-project-language-auto"),
                    );
                    for (id, name) in hs_core::projects::LANGUAGES {
                        ui.selectable_value(
                            &mut draft.entry.language,
                            (*id).to_string(),
                            format!("{} {name}", hs_ui::icons::language_icon(id)),
                        );
                    }
                });
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(s.t("settings-project-git"))
                .strong()
                .color(theme.text),
        );
        if !draft.is_repo {
            form::hint(ui, theme, &s.t("settings-project-not-a-repo"));
        }
        let is_repo = draft.is_repo;
        ui.add_enabled_ui(is_repo, |ui| {
            form::field_grid(ui, "project-git", |ui| {
                form::label(ui, &s.t("settings-project-git-name"));
                form::text_field(
                    ui,
                    &mut draft.git.name,
                    &s.t("settings-project-git-inherit-hint"),
                );
                ui.end_row();
                form::label(ui, &s.t("settings-project-git-email"));
                form::text_field(
                    ui,
                    &mut draft.git.email,
                    &s.t("settings-project-git-inherit-hint"),
                );
                ui.end_row();
                form::label(ui, &s.t("settings-project-git-signing-key"));
                form::text_field(
                    ui,
                    &mut draft.git.signing_key,
                    &s.t("settings-project-git-inherit-hint"),
                );
                ui.end_row();
                // Three states, because "not set here" is a real answer
                // and a checkbox cannot say it: an unset key inherits,
                // and saving `false` over it is a different instruction.
                form::label(ui, &s.t("settings-project-git-sign"));
                let label = match draft.git.sign_commits {
                    None => s.t("settings-project-git-inherit"),
                    Some(true) => s.t("yes"),
                    Some(false) => s.t("no"),
                };
                egui::ComboBox::from_id_salt("project-git-sign")
                    .selected_text(label)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut draft.git.sign_commits,
                            None,
                            s.t("settings-project-git-inherit"),
                        );
                        ui.selectable_value(&mut draft.git.sign_commits, Some(true), s.t("yes"));
                        ui.selectable_value(&mut draft.git.sign_commits, Some(false), s.t("no"));
                    });
                ui.end_row();
                form::label(ui, &s.t("settings-project-git-branch"));
                form::text_field(
                    ui,
                    &mut draft.git.default_branch,
                    &s.t("settings-project-git-branch-hint"),
                );
                ui.end_row();
                form::label(ui, &s.t("settings-project-git-remote"));
                form::text_field(
                    ui,
                    &mut draft.git.remote_url,
                    &s.t("settings-project-git-remote-hint"),
                );
                ui.end_row();
            });
        });
        form::hint(ui, theme, &s.t("settings-project-git-hint"));
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(s.t("settings-project-run"))
                .strong()
                .color(theme.text),
        );
        form::hint(ui, theme, &s.t("settings-project-run-hint"));
        form::field_grid(ui, "project-run", |ui| {
            form::label(ui, &s.t("settings-project-run-spring"));
            form::text_field(
                ui,
                &mut draft.entry.run.spring_profiles,
                &s.t("settings-project-run-spring-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-profiles"));
            form::text_field(
                ui,
                &mut draft.entry.run.build_profiles,
                &s.t("settings-project-run-profiles-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-goal"));
            form::text_field(
                ui,
                &mut draft.entry.run.goal,
                &s.t("settings-project-run-goal-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-args"));
            form::text_field(
                ui,
                &mut draft.entry.run.args,
                &s.t("settings-project-run-args-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-jvm"));
            form::text_field(
                ui,
                &mut draft.entry.run.jvm_args,
                &s.t("settings-project-run-jvm-hint"),
            );
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-skip-tests"));
            ui.checkbox(&mut draft.entry.run.skip_tests, "");
            ui.end_row();
            form::label(ui, &s.t("settings-project-run-env"));
            form::code_field(ui, &mut draft.entry.run.env, 4);
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-project-run-env-hint"));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if form::primary_button(ui, theme, &s.t("save")).clicked() {
                save = true;
            }
            if ui.button(s.t("close")).clicked() {
                state.project_selected = None;
            }
            // A plain right-aligned scope: `toolbar::hard_right` is for a
            // tool's toolbar, where the shell reserves it a corner.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if form::danger_button(ui, theme, &s.t("settings-project-remove")).clicked() {
                    remove = true;
                }
            });
        });
    });

    if save && let Some(draft) = state.project_draft.clone() {
        let mut entry = draft.entry.clone();
        // A project with no folder is not a project: saving one would
        // put a row in the list that points at nothing.
        if entry.root.as_os_str().is_empty() {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                s.t("settings-project-needs-folder"),
            ));
            return;
        }
        // Name it after the folder when the user did not, so a row is
        // never blank.
        if entry.name.trim().is_empty() {
            entry.name = entry
                .root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
        }
        // The language the user chose wins; detection only fills a blank.
        if entry.language.trim().is_empty() {
            entry.language = hs_core::projects::detect_language(&entry.root).to_string();
        }
        // Whether this is a project being added rather than edited,
        // decided before the upsert puts it in the list.
        let is_new = s.projects.read().by_id(&entry.id).is_none();
        let mut failed = None;
        if draft.is_repo {
            if let Err(e) = hs_git::write::set_repo_config(&entry.root, &draft.git) {
                failed = Some(e.to_string());
            } else {
                // The entry keeps a copy of the identity only so a list
                // can show it without running git; the repository is
                // where it actually lives.
                entry.git_name = draft.git.name.clone();
                entry.git_email = draft.git.email.clone();
            }
        }
        match failed {
            Some(e) => effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, e)),
            None => match s.projects.write().upsert(entry.clone()) {
                Ok(()) => {
                    // Back to the list, so the next project is one click
                    // away rather than behind a Close (owner's
                    // requirement, 2026-10-26).
                    state.project_draft = None;
                    state.project_selected = None;
                    effects.push(SettingsEffect::Toast(
                        hs_core::tool::ToastLevel::Success,
                        s.t("toast-saved"),
                    ));
                    // A project added here opens, as one added from the
                    // activity does.
                    if is_new {
                        effects.push(SettingsEffect::OpenProject(entry.root.clone()));
                    }
                }
                Err(e) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                )),
            },
        }
    }
    if remove && let Some(draft) = state.project_draft.clone() {
        // Removing is a list operation: the folder on disk is never
        // touched (owner's requirement, 2026-10-26). The confirmation
        // says so, because "Remove" next to a folder path reads as
        // something much worse than it is.
        effects.push(SettingsEffect::ConfirmRemoveProject(
            draft.entry.id.clone(),
            draft.entry.label(),
        ));
    }
}

// ---------------------------------------------------------------------------
// Settings -> Git (docs/tools/git-activity.md §3)

/// A repository being added or edited in Settings -> Git.
#[derive(Debug, Clone)]
pub struct RepoDraft {
    pub entry: hs_core::git_repos::RepoEntry,
}

impl RepoDraft {
    /// An empty form in the detail column, as Add Project gives one.
    pub fn blank(source: hs_core::git_repos::Source) -> Self {
        Self {
            entry: hs_core::git_repos::RepoEntry {
                source,
                ..Default::default()
            },
        }
    }
    pub fn open(entry: hs_core::git_repos::RepoEntry) -> Self {
        Self { entry }
    }
}

fn git(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-git"),
        Some(&s.t("settings-git-sub")),
    );
    repo_list(ui, theme, s, state, effects);
    credential_list(ui, theme, s, state, effects);
    git_general(ui, theme, s, effects);
}

/// How the Git activity pulls, pushes, protects and branches; until
/// 2026-10-29 the first four lived on the Editor page.
fn git_general(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-git-general"), |ui| {
        form::field_grid(ui, "git-general", |ui| {
            form::label(ui, &s.t("settings-git-protected"));
            let mut text = settings.git.protected_patterns.join(", ");
            if form::text_field(ui, &mut text, "prod, release").changed() {
                settings.git.protected_patterns = text
                    .split(',')
                    .map(|p| p.trim().to_string())
                    .filter(|p| !p.is_empty())
                    .collect();
                changed = true;
            }
            ui.end_row();

            form::label(ui, &s.t("settings-git-clone-folder"));
            ui.horizontal(|ui| {
                changed |= form::text_field(ui, &mut settings.git.clone_folder, "").changed();
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FOLDER_OPEN,
                        s.t("browse")
                    ))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    settings.git.clone_folder = p.display().to_string();
                    changed = true;
                }
            });
            ui.end_row();

            form::label(ui, &s.t("settings-git-terminal-profile"));
            let clis: Vec<(String, String)> = settings
                .terminal
                .cli
                .iter()
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect();
            let current = clis
                .iter()
                .find(|(id, _)| *id == settings.git.terminal_profile)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| s.t("settings-git-terminal-default"));
            egui::ComboBox::from_id_salt("git-terminal-profile")
                .selected_text(current)
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            settings.git.terminal_profile.is_empty(),
                            s.t("settings-git-terminal-default"),
                        )
                        .clicked()
                    {
                        settings.git.terminal_profile.clear();
                        changed = true;
                    }
                    for (id, name) in &clis {
                        if ui
                            .selectable_label(settings.git.terminal_profile == *id, name)
                            .clicked()
                        {
                            settings.git.terminal_profile = id.clone();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-git-protected-hint"));
        form::hint(ui, theme, &s.t("settings-git-clone-folder-hint"));
        form::hint(ui, theme, &s.t("settings-git-terminal-profile-hint"));
        ui.add_space(6.0);
        changed |= ui
            .checkbox(
                &mut settings.git.pull_rebase,
                s.t("settings-git-pull-rebase"),
            )
            .changed();
        changed |= ui
            .checkbox(&mut settings.git.signoff, s.t("settings-git-signoff"))
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.git.stash_untracked,
                s.t("settings-git-stash-untracked"),
            )
            .changed();
    });
    form::section(ui, theme, &s.t("settings-git-flow"), |ui| {
        form::hint(ui, theme, &s.t("settings-git-flow-hint"));
        ui.add_space(4.0);
        let flow = &mut settings.git.flow;
        form::field_grid(ui, "git-flow", |ui| {
            for (key, value) in [
                ("settings-git-flow-production", &mut flow.production),
                ("settings-git-flow-develop", &mut flow.develop),
                ("settings-git-flow-feature", &mut flow.feature),
                ("settings-git-flow-release", &mut flow.release),
                ("settings-git-flow-hotfix", &mut flow.hotfix),
                ("settings-git-flow-tag", &mut flow.version_tag_prefix),
            ] {
                form::label(ui, &s.t(key));
                changed |= form::text_field(ui, value, "").changed();
                ui.end_row();
            }
        });
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn credential_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::section(ui, theme, &s.t("settings-section-git-credentials"), |ui| {
        if form::primary_button(
            ui,
            theme,
            &format!(
                "{} {}",
                hs_ui::icons::ph::PLUS,
                s.t("settings-git-cred-add")
            ),
        )
        .clicked()
        {
            state.cred_selected = None;
            state.cred_draft = Some(hs_core::git_hosts::GitCredential::default());
            state.cred_confirm_remove = false;
        }
        ui.add_space(8.0);
        form::hint(ui, theme, &s.t("settings-git-cred-hint"));
        let rows: Vec<(String, String, String)> = s
            .git_credentials
            .read()
            .all()
            .iter()
            .map(|c| {
                (
                    c.id.clone(),
                    c.label(),
                    format!("{} · {}", c.kind.name(), c.host()),
                )
            })
            .collect();
        if rows.is_empty() {
            form::hint(ui, theme, &s.t("settings-git-cred-none"));
        }
        let mut pick: Option<String> = None;
        for (id, name, detail) in &rows {
            let selected = state.cred_selected.as_deref() == Some(id.as_str());
            if form::list_row(
                ui,
                theme,
                selected,
                hs_ui::icons::ph::KEY,
                name,
                Some(detail),
            )
            .clicked()
            {
                pick = Some(id.clone());
            }
            ui.add_space(2.0);
        }
        if let Some(id) = pick {
            state.cred_selected = Some(id.clone());
            state.cred_draft = s.git_credentials.read().by_id(&id).cloned();
            state.cred_confirm_remove = false;
        }
    });
    if state.cred_draft.is_some() {
        ui.add_space(8.0);
        credential_detail(ui, theme, s, state, effects);
    }
    ui.add_space(12.0);
}

fn credential_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    let tested = state
        .cred_test
        .as_ref()
        .and_then(|(_, slot)| slot.lock().take());
    if let Some(r) = tested {
        state.cred_test = None;
        match r {
            Ok(login) => effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Success,
                s.i18n
                    .t_args("settings-git-cred-test-ok", &[("login", &login)]),
            )),
            Err(e) => effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, e)),
        }
    }
    let Some(cred) = state.cred_draft.as_mut() else {
        return;
    };
    let is_new = s.git_credentials.read().by_id(&cred.id).is_none();
    let mut save = false;
    let mut close = false;
    let mut remove = false;
    let title = if cred.name.trim().is_empty() {
        s.t("settings-git-cred-add")
    } else {
        cred.label()
    };
    form::section(ui, theme, &title, |ui| {
        form::field_grid(ui, "git-cred-detail", |ui| {
            form::label(ui, &s.t("settings-git-cred-name"));
            form::text_field(ui, &mut cred.name, &s.t("settings-git-cred-name-hint"));
            ui.end_row();

            form::label(ui, &s.t("settings-git-cred-kind"));
            egui::ComboBox::from_id_salt("git-cred-kind")
                .selected_text(cred.kind.name())
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    for k in hs_core::git_hosts::HostKind::all() {
                        if ui.selectable_label(cred.kind == k, k.name()).clicked() {
                            // The public host's address follows the kind
                            // until the user has typed one of their own.
                            let was_default = cred.base_url.is_empty()
                                || cred.base_url == cred.kind.default_url();
                            cred.kind = k;
                            if was_default {
                                cred.base_url = k.default_url().to_string();
                            }
                        }
                    }
                });
            ui.end_row();

            form::label(ui, &s.t("settings-git-cred-url"));
            form::text_field(ui, &mut cred.base_url, &s.t("settings-git-cred-url-hint"));
            ui.end_row();

            form::label(ui, &s.t("settings-git-cred-user"));
            form::text_field(ui, &mut cred.username, "");
            ui.end_row();

            form::label(ui, &s.t("settings-git-cred-token"));
            let mut scratch = String::new();
            secret_field(
                ui,
                theme,
                s,
                "git-cred-token",
                &mut cred.token,
                &mut scratch,
                0,
            );
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-git-cred-token-hint"));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let can = !cred.base_url.trim().is_empty() || cred.kind.default_url().is_empty();
            if ui
                .add_enabled(can, egui::Button::new(s.t("save")).fill(theme.accent))
                .clicked()
            {
                save = true;
            }
            if ui.button(s.t("close")).clicked() {
                close = true;
            }
            // Test asks the host who the token belongs to.
            let testing = state
                .cred_test
                .as_ref()
                .is_some_and(|(id, _)| *id == cred.id);
            if ui
                .add_enabled(
                    cred.kind.lists_projects() && !testing,
                    egui::Button::new(s.t("settings-git-cred-test")),
                )
                .clicked()
            {
                match tool_git::auth::resolve_cred(s, cred) {
                    Err(err) => {
                        effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, err))
                    }
                    Ok(resolved) => {
                        let token = resolved.map(|r| r.token).unwrap_or_default();
                        let slot: std::sync::Arc<
                            parking_lot::Mutex<Option<Result<String, String>>>,
                        > = Default::default();
                        let c = cred.clone();
                        let done = slot.clone();
                        s.runtime.spawn(async move {
                            let http = hs_core::ai::http_client();
                            *done.lock() = Some(tool_git::hosts::whoami(&http, &c, &token).await);
                        });
                        state.cred_test = Some((cred.id.clone(), slot));
                    }
                }
            }
            if testing {
                ui.spinner();
            }
            if !is_new {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if state.cred_confirm_remove {
                        if form::danger_button(ui, theme, &s.t("settings-git-cred-remove"))
                            .clicked()
                        {
                            remove = true;
                        }
                        ui.label(
                            egui::RichText::new(s.i18n.t_args(
                                "settings-git-cred-remove-confirm",
                                &[("name", &cred.label())],
                            ))
                            .color(theme.warning),
                        );
                    } else if ui.button(s.t("settings-git-cred-remove")).clicked() {
                        state.cred_confirm_remove = true;
                    }
                });
            }
        });
    });
    if close {
        state.cred_draft = None;
        state.cred_selected = None;
        state.cred_confirm_remove = false;
    }
    if save && let Some(mut cred) = state.cred_draft.clone() {
        if cred.name.trim().is_empty() {
            cred.name = cred.label();
        }
        cred.base_url = cred.host();
        // The typed token goes to the vault and the reference stays here;
        // a locked vault refuses the save rather than writing the token.
        let label = cred.label();
        if !store_typed_secrets(s, &label, [("Token", &mut cred.token)], effects) {
            return;
        }
        match s.git_credentials.write().upsert(cred) {
            Ok(()) => {
                state.cred_draft = None;
                state.cred_selected = None;
                effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Success,
                    s.t("toast-saved"),
                ));
            }
            Err(e) => effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            )),
        }
    }
    if remove && let Some(cred) = state.cred_draft.clone() {
        match s.git_credentials.write().remove(&cred.id) {
            Ok(()) => {
                state.cred_draft = None;
                state.cred_selected = None;
                state.cred_confirm_remove = false;
            }
            Err(e) => effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            )),
        }
    }
}

fn repo_list(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::section(ui, theme, &s.t("settings-section-git-repos"), |ui| {
        if form::primary_button(
            ui,
            theme,
            &format!(
                "{} {}",
                hs_ui::icons::ph::PLUS,
                s.t("settings-git-repo-add")
            ),
        )
        .clicked()
        {
            state.repo_selected = None;
            state.repo_draft = Some(RepoDraft::blank(hs_core::git_repos::Source::Local));
        }
        ui.add_space(8.0);
        form::hint(ui, theme, &s.t("settings-git-repos-hint"));
        let rows: Vec<(String, String, String, &'static str, Option<String>)> = s
            .git_repos
            .read()
            .all()
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    r.label(),
                    r.path.display().to_string(),
                    repo_icon(r),
                    repo_image(ui.ctx(), s, &r.id),
                )
            })
            .collect();
        if rows.is_empty() {
            form::hint(ui, theme, &s.t("settings-git-repos-none"));
        }
        let mut reorder = hs_ui::reorder::Reorder::new("git-repo-list-reorder");
        let mut pick: Option<String> = None;
        for (i, (id, name, path, icon, image)) in rows.iter().enumerate() {
            let selected = state.repo_selected.as_deref() == Some(id.as_str());
            reorder.row(ui, i, |ui| {
                if form::list_row_image(
                    ui,
                    theme,
                    selected,
                    image.as_deref(),
                    icon,
                    name,
                    Some(path),
                )
                .clicked()
                {
                    pick = Some(id.clone());
                }
            });
            ui.add_space(2.0);
        }
        if let Some((from, to)) = reorder.finish(ui, theme.accent)
            && let Err(e) = s.git_repos.write().reorder(from, to)
        {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            ));
        }
        if let Some(id) = pick {
            state.repo_selected = Some(id.clone());
            if let Some(r) = s.git_repos.read().by_id(&id) {
                state.repo_draft = Some(RepoDraft::open(r.clone()));
            }
        }
    });
    if state.repo_draft.is_some() {
        ui.add_space(8.0);
        repo_detail(ui, theme, s, state, effects);
    }
    ui.add_space(12.0);
}

/// A saved repository's uploaded icon as an image URI, if it has one.
pub(crate) fn repo_image(ctx: &egui::Context, s: &Services, id: &str) -> Option<String> {
    let file = s.git_repos.read().icon_file(id)?;
    hs_ui::icons::image_uri(ctx, &file)
}

/// The glyph a saved repository draws: its chosen icon, else its
/// language's, else git's branch mark.
pub(crate) fn repo_icon(r: &hs_core::git_repos::RepoEntry) -> &'static str {
    hs_ui::icons::named(&r.icon).unwrap_or_else(|| {
        if r.language.is_empty() {
            hs_ui::icons::ph::GIT_BRANCH
        } else {
            hs_ui::icons::language_icon(&r.language)
        }
    })
}

fn repo_detail(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    use hs_core::git_repos::Source;
    // Answers from the blocking pool, read before anything borrows the
    // draft: the host's project listing, and a clone Save started.
    let listed = state.repo_projects_slot.lock().take();
    if let Some(r) = listed {
        state.repo_projects_busy = false;
        match r {
            Ok(list) => state.repo_projects = Some(list),
            Err(e) => effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, e)),
        }
    }
    let cloned = state
        .repo_clone
        .as_ref()
        .and_then(|(_, slot)| slot.lock().take());
    if let Some(r) = cloned
        && let Some((mut entry, _)) = state.repo_clone.take()
    {
        // The folder exists now, so its language can be read.
        if entry.language.trim().is_empty() {
            entry.language = hs_core::projects::detect_language(&entry.path).to_string();
        }
        match r {
            Ok(()) => match s.git_repos.write().upsert(entry.clone()) {
                Ok(()) => {
                    state.repo_draft = None;
                    state.repo_selected = None;
                    effects.push(SettingsEffect::Toast(
                        hs_core::tool::ToastLevel::Success,
                        s.i18n
                            .t_args("settings-git-repo-cloned", &[("name", &entry.label())]),
                    ));
                    effects.push(SettingsEffect::OpenRepo(entry.path.clone()));
                }
                Err(e) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                )),
            },
            Err(e) => effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, e)),
        }
    }
    let Some(draft) = state.repo_draft.as_mut() else {
        return;
    };
    let cloning = state.repo_clone.is_some();
    let is_new = s.git_repos.read().by_id(&draft.entry.id).is_none();
    let mut save = false;
    let mut close = false;
    let mut remove = false;
    let title = if draft.entry.name.trim().is_empty() && draft.entry.path.as_os_str().is_empty() {
        s.t("settings-git-repo-add")
    } else {
        draft.entry.label()
    };
    form::section(ui, theme, &title, |ui| {
        let e = &mut draft.entry;
        form::field_grid(ui, "git-repo-detail", |ui| {
            form::label(ui, &s.t("settings-git-repo-source"));
            egui::ComboBox::from_id_salt("git-repo-source")
                .selected_text(s.t(e.source.label_key()))
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    for src in Source::all() {
                        if ui
                            .selectable_label(e.source == src, s.t(src.label_key()))
                            .clicked()
                        {
                            e.source = src;
                        }
                    }
                });
            ui.end_row();

            form::label(ui, &s.t("settings-git-repo-name"));
            form::text_field(ui, &mut e.name, &s.t("settings-git-repo-name-hint"));
            ui.end_row();

            form::label(ui, &s.t("settings-git-repo-icon"));
            // An uploaded picture is the icon the tab, the sidebar and
            // the dashboard wear; it is kept beside the list under
            // <workspace>/git/icons/<id>.<ext> (owner's requirement,
            // 2026-10-29). The glyph picker below stays the fallback.
            ui.horizontal(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 6.0, theme.surface);
                let image = s.git_repos.read().icon_file(&e.id);
                match &image {
                    Some(file) => {
                        if let Some(uri) = hs_ui::icons::image_uri(ui.ctx(), file) {
                            egui::Image::from_uri(uri)
                                .fit_to_exact_size(egui::vec2(32.0, 32.0))
                                .paint_at(ui, rect.shrink(2.0));
                        }
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            repo_icon(e),
                            egui::FontId::proportional(20.0),
                            theme.text_muted,
                        );
                    }
                }
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::UPLOAD_SIMPLE,
                        s.t("settings-git-repo-icon-upload")
                    ))
                    .clicked()
                    && let Some(src) = rfd::FileDialog::new()
                        .add_filter("Image", hs_core::workspace::ICON_EXTS)
                        .pick_file()
                {
                    match s.git_repos.read().set_icon(&e.id, &src) {
                        Ok(_) => effects.push(SettingsEffect::Toast(
                            hs_core::tool::ToastLevel::Success,
                            s.t("settings-git-repo-icon-set"),
                        )),
                        Err(err) => effects.push(SettingsEffect::Toast(
                            hs_core::tool::ToastLevel::Error,
                            err.to_string(),
                        )),
                    }
                }
                if image.is_some() && ui.button(s.t("settings-git-repo-icon-remove")).clicked() {
                    s.git_repos.read().remove_icon(&e.id);
                }
            });
            ui.end_row();

            form::label(ui, "");
            let current = hs_ui::icons::named(&e.icon)
                .map(|g| format!("{g}  {}", e.icon))
                .unwrap_or_else(|| s.t("settings-git-repo-icon-auto"));
            egui::ComboBox::from_id_salt("git-repo-icon")
                .selected_text(current)
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(e.icon.is_empty(), s.t("settings-git-repo-icon-auto"))
                        .clicked()
                    {
                        e.icon.clear();
                    }
                    for (key, glyph) in hs_ui::icons::REPO_ICONS {
                        if ui
                            .selectable_label(e.icon == *key, format!("{glyph}  {key}"))
                            .clicked()
                        {
                            e.icon = key.to_string();
                        }
                    }
                });
            ui.end_row();

            if e.source != Source::Local {
                form::label(ui, &s.t("settings-git-repo-url"));
                form::text_field(ui, &mut e.url, &s.t("settings-git-repo-url-hint"));
                ui.end_row();

                form::label(ui, &s.t("settings-git-repo-credential"));
                let creds: Vec<(String, String)> = s
                    .git_credentials
                    .read()
                    .all()
                    .iter()
                    .map(|c| (c.id.clone(), c.label()))
                    .collect();
                let current = creds
                    .iter()
                    .find(|(id, _)| *id == e.credential)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| s.t("settings-git-repo-credential-none"));
                egui::ComboBox::from_id_salt("git-repo-credential")
                    .selected_text(current)
                    .width(form::FIELD_MIN_WIDTH)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                e.credential.is_empty(),
                                s.t("settings-git-repo-credential-none"),
                            )
                            .clicked()
                        {
                            e.credential.clear();
                        }
                        for (id, name) in &creds {
                            if ui.selectable_label(e.credential == *id, name).clicked() {
                                e.credential = id.clone();
                                if let Some(c) = s.git_credentials.read().by_id(id) {
                                    e.host_url = c.host();
                                }
                            }
                        }
                    });
                ui.end_row();

                form::label(ui, &s.t("settings-git-repo-project"));
                ui.horizontal(|ui| {
                    // Fetch Project asks the host for everything the
                    // credential can see; the dropdown is searched with
                    // the fuzzy matcher the palette uses.
                    let cred = s.git_credentials.read().by_id(&e.credential).cloned();
                    let lists = cred.as_ref().is_some_and(|c| c.kind.lists_projects());
                    let fetch = ui
                        .add_enabled(
                            lists && !state.repo_projects_busy,
                            egui::Button::new(format!(
                                "{} {}",
                                hs_ui::icons::ph::CLOUD_ARROW_DOWN,
                                s.t("settings-git-repo-fetch")
                            )),
                        )
                        .on_disabled_hover_text(s.t("settings-git-repo-fetch-first"))
                        .clicked();
                    if state.repo_projects_busy {
                        ui.spinner();
                    }
                    if let Some(list) = &state.repo_projects {
                        let shown = if e.project.is_empty() {
                            s.i18n.t_args(
                                "settings-git-repo-project-pick",
                                &[("count", &list.len().to_string())],
                            )
                        } else {
                            e.project.clone()
                        };
                        let mut chosen: Option<tool_git::hosts::Project> = None;
                        egui::ComboBox::from_id_salt("git-repo-project")
                            .selected_text(shown)
                            .width(360.0)
                            .show_ui(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut state.repo_project_filter)
                                        .hint_text(s.t("settings-git-repo-project-hint"))
                                        .desired_width(f32::INFINITY),
                                );
                                let needle = state.repo_project_filter.trim();
                                let order: Vec<usize> = if needle.is_empty() {
                                    (0..list.len()).collect()
                                } else {
                                    let items: Vec<(&str, Option<&str>)> = list
                                        .iter()
                                        .map(|p| {
                                            (p.full_name.as_str(), Some(p.description.as_str()))
                                        })
                                        .collect();
                                    hs_core::fuzzy::rank(needle, &items)
                                        .into_iter()
                                        .map(|m| m.index)
                                        .collect()
                                };
                                egui::ScrollArea::vertical()
                                    .max_height(320.0)
                                    .show(ui, |ui| {
                                        for i in order.into_iter().take(400) {
                                            let p = &list[i];
                                            let label = if p.private {
                                                format!(
                                                    "{} {}",
                                                    hs_ui::icons::ph::LOCK,
                                                    p.full_name
                                                )
                                            } else {
                                                p.full_name.clone()
                                            };
                                            let r = ui
                                                .selectable_label(e.project == p.full_name, label);
                                            let r = if p.description.is_empty() {
                                                r
                                            } else {
                                                r.on_hover_text(&p.description)
                                            };
                                            if r.clicked() {
                                                chosen = Some(p.clone());
                                            }
                                        }
                                    });
                            });
                        if let Some(p) = chosen {
                            // The project fills what the form would ask
                            // for next: the URL, a name, and the folder
                            // under the default clone folder.
                            let name = p
                                .full_name
                                .rsplit('/')
                                .next()
                                .unwrap_or(&p.full_name)
                                .to_string();
                            if e.name.trim().is_empty()
                                || e.name == e.project.rsplit('/').next().unwrap_or("")
                            {
                                e.name = name.clone();
                            }
                            e.project = p.full_name.clone();
                            e.url = p.clone_url.clone();
                            // The host's picture becomes the icon, unless
                            // one was uploaded already (phase 6).
                            if !p.avatar_url.is_empty()
                                && s.git_repos.read().icon_file(&e.id).is_none()
                            {
                                let id = e.id.clone();
                                let url = p.avatar_url.clone();
                                let repos = s.git_repos.clone();
                                s.runtime.spawn(async move {
                                    let http = hs_core::ai::http_client();
                                    if let Ok((bytes, ext)) =
                                        tool_git::hosts::fetch_avatar(&http, &url).await
                                        && let Err(e) =
                                            repos.read().set_icon_bytes(&id, &ext, &bytes)
                                    {
                                        tracing::warn!("repository avatar not saved: {e}");
                                    }
                                });
                            }
                            let base = s.settings.read().git.clone_folder.clone();
                            if e.path.as_os_str().is_empty() && !base.trim().is_empty() {
                                e.path = std::path::PathBuf::from(base.trim()).join(&name);
                            }
                        }
                    }
                    if fetch && let Some(cred) = cred {
                        match tool_git::auth::resolve_cred(s, &cred) {
                            Err(err) => effects
                                .push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, err)),
                            Ok(resolved) => {
                                let token = resolved.map(|r| r.token).unwrap_or_default();
                                let slot = state.repo_projects_slot.clone();
                                state.repo_projects_busy = true;
                                state.repo_project_filter.clear();
                                s.runtime.spawn(async move {
                                    let http = hs_core::ai::http_client();
                                    let r =
                                        tool_git::hosts::list_projects(&http, &cred, &token).await;
                                    *slot.lock() = Some(r);
                                });
                            }
                        }
                    }
                });
                ui.end_row();
            }

            form::label(ui, &s.t("settings-git-repo-path"));
            ui.horizontal(|ui| {
                let mut path = e.path.display().to_string();
                if form::text_field(ui, &mut path, "").changed() {
                    e.path = std::path::PathBuf::from(path);
                }
                if ui
                    .button(format!(
                        "{} {}",
                        hs_ui::icons::ph::FOLDER_OPEN,
                        s.t("browse")
                    ))
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new().pick_folder()
                {
                    // A different folder is a different repository: its
                    // name and language are read again.
                    let fresh = hs_core::git_repos::RepoEntry::for_path(&p);
                    if e.name.trim().is_empty() || e.name == fresh.name {
                        e.name = fresh.name;
                    }
                    e.language = fresh.language;
                    e.path = p;
                }
            });
            ui.end_row();

            form::label(ui, &s.t("settings-git-repo-language"));
            let current = if e.language.is_empty() {
                s.t("settings-project-language-auto")
            } else {
                let name = hs_core::projects::language_name(&e.language);
                if name.is_empty() {
                    e.language.clone()
                } else {
                    name.to_string()
                }
            };
            egui::ComboBox::from_id_salt("git-repo-language")
                .selected_text(format!(
                    "{} {current}",
                    hs_ui::icons::language_icon(&e.language)
                ))
                .width(form::FIELD_MIN_WIDTH)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            e.language.is_empty(),
                            s.t("settings-project-language-auto"),
                        )
                        .clicked()
                    {
                        e.language = hs_core::projects::detect_language(&e.path).to_string();
                    }
                    for (id, name) in hs_core::projects::LANGUAGES {
                        if ui
                            .selectable_label(
                                e.language == *id,
                                format!("{} {name}", hs_ui::icons::language_icon(id)),
                            )
                            .clicked()
                        {
                            e.language = id.to_string();
                        }
                    }
                });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-git-repo-icon-hint"));
        let has_path = !e.path.as_os_str().is_empty();
        // A folder that is not a repository: for a host source Save
        // clones into it (it must not exist yet, as git requires); for
        // a local source it is simply not a repository.
        let will_clone = e.source != Source::Local && has_path && !e.is_cloned();
        if will_clone {
            form::hint(ui, theme, &s.t("settings-git-repo-will-clone"));
        } else if has_path && !e.is_cloned() {
            form::hint(ui, theme, &s.t("settings-git-repo-not-a-repo"));
        }
        // Save is disabled until the mandatory fields hold, and the hint
        // under it names what is missing (owner's requirement).
        let missing: Vec<String> = e.missing().iter().map(|k| s.t(k)).collect();
        let can_save = missing.is_empty() && (e.is_cloned() || will_clone) && !cloning;
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let save_label = if will_clone {
                s.t("settings-git-repo-clone-and-save")
            } else {
                s.t("save")
            };
            if ui
                .add_enabled(can_save, egui::Button::new(save_label).fill(theme.accent))
                .clicked()
            {
                save = true;
            }
            if cloning {
                ui.spinner();
                ui.label(
                    egui::RichText::new(s.t("settings-git-repo-cloning")).color(theme.text_muted),
                );
            }
            if ui.button(s.t("close")).clicked() {
                close = true;
            }
            if !is_new {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if form::danger_button(ui, theme, &s.t("git-repo-remove")).clicked() {
                        remove = true;
                    }
                });
            }
        });
        if !missing.is_empty() {
            form::hint(
                ui,
                theme,
                &s.i18n.t_args(
                    "settings-git-repo-missing",
                    &[("what", &missing.join(", "))],
                ),
            );
        }
    });
    if close {
        state.repo_draft = None;
        state.repo_selected = None;
    }
    if save && let Some(draft) = state.repo_draft.clone() {
        let mut entry = draft.entry;
        if entry.name.trim().is_empty() {
            entry.name = entry.label();
        }
        // A host source whose folder holds nothing yet: clone first, as a
        // Job with git's progress lines and cancel; the entry is recorded
        // when the clone lands (polled at the top of this function).
        if entry.source != Source::Local && !entry.is_cloned() {
            let resolved = match tool_git::auth::resolve(s, &entry.credential) {
                Ok(r) => r,
                Err(e) => {
                    effects.push(SettingsEffect::Toast(hs_core::tool::ToastLevel::Error, e));
                    return;
                }
            };
            let slot: Slot<Result<(), String>> = Default::default();
            let (url, dest) = (entry.url.clone(), entry.path.clone());
            let done = slot.clone();
            let title = s.i18n.t_args(
                "settings-git-repo-cloning-title",
                &[("name", &entry.label())],
            );
            s.jobs.spawn(title, "git", move |h| async move {
                let cancel = h.clone();
                let progress = h.clone();
                let r = tokio::task::spawn_blocking(move || {
                    tool_git::auth::with_env(resolved.as_ref(), |env| {
                        hs_git::write::clone(
                            &url,
                            &dest,
                            env,
                            &|| cancel.is_cancelled(),
                            &mut |line| progress.set_message(line.to_string()),
                        )
                        .map_err(|e| e.to_string())
                    })
                    .and_then(|r| r)
                })
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| r);
                *done.lock() = Some(r.clone());
                r
            });
            state.repo_clone = Some((entry, slot));
            return;
        }
        if entry.language.trim().is_empty() {
            entry.language = hs_core::projects::detect_language(&entry.path).to_string();
        }
        let is_new = s.git_repos.read().by_id(&entry.id).is_none();
        match s.git_repos.write().upsert(entry.clone()) {
            Ok(()) => {
                state.repo_draft = None;
                state.repo_selected = None;
                effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Success,
                    s.t("toast-saved"),
                ));
                if is_new {
                    effects.push(SettingsEffect::OpenRepo(entry.path.clone()));
                }
            }
            Err(e) => effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Error,
                e.to_string(),
            )),
        }
    }
    if remove && let Some(draft) = state.repo_draft.clone() {
        effects.push(SettingsEffect::ConfirmRemoveRepo(
            draft.entry.id.clone(),
            draft.entry.label(),
        ));
    }
}

fn editor(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-editor"),
        Some(&s.t("settings-editor-sub")),
    );
    project_list(ui, theme, s, state, effects);
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-section-editor-font"), |ui| {
        form::field_grid(ui, "editor-font", |ui| {
            form::label(ui, &s.t("settings-editor-font"));
            if font_combo(
                ui,
                s,
                "editor-font",
                &mut settings.editor.font_family,
                true,
                "",
            ) {
                effects.push(SettingsEffect::FontsChanged);
            }
            ui.end_row();
            form::label(ui, &s.t("settings-editor-font-size"));
            changed |= ui
                .add(egui::Slider::new(&mut settings.editor.font_size, 9.0..=28.0).suffix(" px"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-editor-font-size-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-behaviour"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.editor.right_click_copy_paste,
                s.t("settings-editor-right-click"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.editor.auto_reload,
                s.t("settings-editor-auto-reload"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.editor.format_minified_on_open,
                s.t("settings-editor-format-minified"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-editor-format-minified-hint"));
        form::hint(ui, theme, &s.t("settings-terminal-right-click-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-on-save"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.editor.trim_trailing_whitespace,
                s.t("settings-editor-trim"),
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.editor.insert_final_newline,
                s.t("settings-editor-final-newline"),
            )
            .changed();
    });
    form::section(ui, theme, &s.t("settings-section-editor-terminal"), |ui| {
        form::field_grid(ui, "editor-terminal-shell", |ui| {
            form::label(ui, &s.t("settings-editor-terminal-shell"));
            let profiles = hs_term::shells::profiles();
            let current = profiles
                .iter()
                .find(|p| p.id == settings.editor.terminal_profile)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| s.t("settings-editor-terminal-shell-default"));
            egui::ComboBox::from_id_salt("editor-terminal-shell")
                .selected_text(current)
                .width(260.0)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(
                            &mut settings.editor.terminal_profile,
                            String::new(),
                            s.t("settings-editor-terminal-shell-default"),
                        )
                        .changed();
                    for p in profiles.iter() {
                        changed |= ui
                            .selectable_value(
                                &mut settings.editor.terminal_profile,
                                p.id.clone(),
                                &p.name,
                            )
                            .changed();
                    }
                });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("settings-editor-terminal-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-editor-build"), |ui| {
        form::field_grid(ui, "editor-build-tools", |ui| {
            form::label(ui, &s.t("settings-editor-maven"));
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut settings.editor.maven_command)
                        .hint_text("mvn")
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.end_row();
            form::label(ui, &s.t("settings-editor-gradle"));
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut settings.editor.gradle_command)
                        .hint_text("gradle")
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.end_row();
        });
        changed |= ui
            .checkbox(
                &mut settings.editor.prefer_wrapper,
                s.t("settings-editor-prefer-wrapper"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-editor-build-hint"));
    });
    form::section(ui, theme, &s.t("settings-section-language-servers"), |ui| {
        changed |= ui
            .checkbox(&mut settings.editor.lsp_enabled, s.t("settings-editor-lsp"))
            .changed();
        changed |= ui
            .checkbox(
                &mut settings.editor.completion_on_type,
                s.t("settings-editor-complete-on-type"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-editor-lsp-hint"));
        // PATH is read once at start, but it can be read again: the button
        // does that and forgets what the table decided about each command.
        form::hint(ui, theme, &s.t("settings-editor-lsp-path-hint"));
        ui.add_space(2.0);
        refresh_button(ui, theme, s);
        ui.add_space(4.0);
        changed |=
            language_server_table(ui, theme, s, &mut settings.editor.language_servers, effects);
    });
    form::section(ui, theme, &s.t("settings-section-debug-adapters"), |ui| {
        form::hint(ui, theme, &s.t("settings-editor-dap-hint"));
        form::hint(ui, theme, &s.t("settings-editor-dap-path-hint"));
        ui.add_space(2.0);
        refresh_button(ui, theme, s);
        ui.add_space(4.0);
        changed |= debug_adapter_table(ui, theme, s, &mut settings.editor.debug_adapters, effects);
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// The button beside a "Not found" row: run the install command in a
/// terminal tab, or open the download page when no single command works
/// on every platform.
/// How wide the Command and Arguments fields may be in an extension table.
///
/// A command is often a full path once the default is replaced (a jdtls
/// launcher, a virtual environment's python) and the arguments can be a line
/// of them, so both want all the room the page can spare — but never more
/// than it has: a cell that asks for more than `available_width` makes the
/// panel grow every frame, and egui saves that runaway width with the
/// layout. Every future extension table should use this.
fn extension_field_widths(ui: &egui::Ui) -> (f32, f32) {
    // What the other three columns and the grid spacing need.
    const OTHER: f32 = 440.0;
    let spare = (ui.available_width() - OTHER).clamp(220.0, 700.0);
    (spare * 0.56, spare * 0.44)
}

/// **Refresh Status**: read `PATH` from the system again and forget what the
/// page decided about each command. Installing a server from a terminal does
/// not change this process's environment, so without this the table would go
/// on saying "Not found" until the next start.
fn refresh_extension_status(ui: &mut egui::Ui) {
    let changed = hs_core::env_path::refresh();
    for id in ["settings-lsp-path-cache", "settings-dap-path-cache"] {
        ui.ctx()
            .data_mut(|d| d.remove::<std::collections::HashMap<String, bool>>(egui::Id::new(id)));
    }
    let now = ui.input(|i| i.time);
    ui.ctx()
        .data_mut(|d| d.insert_temp(egui::Id::new(REFRESH_RESULT), (now, changed)));
    tracing::info!(path_changed = changed, "extension status refreshed");
}

/// When the last refresh happened and whether `PATH` had moved, so the
/// button can say what it did instead of appearing to do nothing.
const REFRESH_RESULT: &str = "settings-extensions-refresh-result";
/// How long that answer stays on screen.
const REFRESH_NOTE_SECS: f64 = 8.0;

/// The button that does it, for one extension section.
fn refresh_button(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    ui.horizontal(|ui| {
        if ui
            .small_button(format!(
                "{} {}",
                hs_ui::icons::ph::ARROWS_CLOCKWISE,
                s.t("settings-extensions-refresh")
            ))
            .tip(s.t("settings-extensions-refresh-tip"))
            .clicked()
        {
            refresh_extension_status(ui);
        }
        let last: Option<(f64, bool)> = ui
            .ctx()
            .data_mut(|d| d.get_temp(egui::Id::new(REFRESH_RESULT)));
        let now = ui.input(|i| i.time);
        if let Some((at, changed)) = last
            && now - at < REFRESH_NOTE_SECS
        {
            let key = if changed {
                "settings-extensions-refresh-changed"
            } else {
                "settings-extensions-refresh-same"
            };
            ui.colored_label(
                if changed {
                    theme.success
                } else {
                    theme.text_muted
                },
                s.t(key),
            );
            // The note goes away on its own, so keep painting until it has.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
    });
}

fn install_button(
    ui: &mut egui::Ui,
    s: &Services,
    command: &str,
    url: &str,
    effects: &mut Vec<SettingsEffect>,
) {
    if command.is_empty() && url.is_empty() {
        return;
    }
    let tip = if command.is_empty() {
        s.i18n
            .t_args("editor-lsp-install-page-tip", &[("url", url)])
    } else {
        s.i18n
            .t_args("editor-lsp-install-run-tip", &[("command", command)])
    };
    if ui
        .small_button(format!(
            "{} {}",
            hs_ui::icons::ph::DOWNLOAD_SIMPLE,
            s.t("editor-lsp-install")
        ))
        .tip(tip)
        .clicked()
    {
        if command.is_empty() {
            effects.push(SettingsEffect::OpenUrl(url.to_string()));
        } else {
            effects.push(SettingsEffect::RunInTerminal(command.to_string()));
        }
    }
}

/// One row per debug adapter: command and arguments (empty keeps the
/// built-in ones), on/off, and whether the command is on PATH.
fn debug_adapter_table(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    rows: &mut Vec<hs_core::settings::DebugAdapter>,
    effects: &mut Vec<SettingsEffect>,
) -> bool {
    use hs_core::settings::DebugAdapter;
    let cache_id = egui::Id::new("settings-dap-path-cache");
    let mut cache: std::collections::HashMap<String, bool> = ui
        .ctx()
        .data_mut(|d| d.get_temp(cache_id))
        .unwrap_or_default();
    let mut changed = false;
    let (command_w, args_w) = extension_field_widths(ui);
    egui::Grid::new("settings-dap-table")
        .num_columns(5)
        .spacing([12.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            for key in [
                "settings-editor-dap-adapter",
                "settings-editor-lsp-command",
                "settings-editor-lsp-args",
                "settings-editor-lsp-enabled",
                "settings-editor-lsp-status",
            ] {
                ui.label(
                    egui::RichText::new(s.t(key))
                        .strong()
                        .color(theme.text_muted),
                );
            }
            ui.end_row();
            for spec in hs_dap::adapters::ADAPTERS {
                let idx = rows.iter().position(|a| a.id == spec.id);
                let mut row = idx
                    .map(|i| rows[i].clone())
                    .unwrap_or_else(|| DebugAdapter {
                        id: spec.id.to_string(),
                        command: String::new(),
                        args: String::new(),
                        enabled: true,
                    });
                ui.label(format!("{}  ({})", spec.id, spec.languages.join(", ")));
                // A grid cell is as wide as its column was last frame, so a
                // desired_width alone is clamped to the old, narrow value and
                // never grows. The scope gives the cell the width instead.
                let r1 = ui
                    .scope(|ui| {
                        ui.set_min_width(command_w);
                        ui.add(
                            egui::TextEdit::singleline(&mut row.command)
                                .hint_text(spec.command)
                                .desired_width(command_w),
                        )
                    })
                    .inner;
                let r2 = ui
                    .scope(|ui| {
                        ui.set_min_width(args_w);
                        ui.add(
                            egui::TextEdit::singleline(&mut row.args)
                                .hint_text(spec.args.join(" "))
                                .desired_width(args_w),
                        )
                    })
                    .inner;
                let r3 = ui.checkbox(&mut row.enabled, "");
                let command = if row.command.trim().is_empty() {
                    spec.command.to_string()
                } else {
                    row.command.trim().to_string()
                };
                if !row.enabled {
                    ui.label(
                        egui::RichText::new(s.t("settings-editor-lsp-off")).color(theme.text_muted),
                    );
                } else {
                    let found = *cache
                        .entry(command.clone())
                        .or_insert_with(|| hs_dap::adapters::on_path(&command).is_some());
                    if found {
                        ui.colored_label(
                            theme.success,
                            format!(
                                "{} {}",
                                hs_ui::icons::CHECK,
                                s.t("settings-editor-lsp-found")
                            ),
                        );
                    } else {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                theme.warning,
                                format!(
                                    "{} {}",
                                    hs_ui::icons::WARNING,
                                    s.t("settings-editor-lsp-not-found")
                                ),
                            )
                            .tip(s.i18n.t_args(
                                "settings-editor-lsp-install-tip",
                                &[("command", spec.install)],
                            ));
                            install_button(ui, s, spec.install_command, spec.install_url, effects);
                        });
                    }
                }
                ui.end_row();
                if r1.changed() || r2.changed() || r3.changed() {
                    changed = true;
                    cache.clear();
                    let is_default =
                        row.command.trim().is_empty() && row.args.trim().is_empty() && row.enabled;
                    match (idx, is_default) {
                        (Some(i), true) => {
                            rows.remove(i);
                        }
                        (Some(i), false) => rows[i] = row,
                        (None, true) => {}
                        (None, false) => rows.push(row),
                    }
                }
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(cache_id, cache));
    changed
}

/// One row per language the editor knows a server for: command and
/// arguments (empty keeps the built-in ones), on/off, and whether the
/// command is on PATH. Returns true when a row changed.
fn language_server_table(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    rows: &mut Vec<hs_core::settings::LanguageServer>,
    effects: &mut Vec<SettingsEffect>,
) -> bool {
    use hs_core::settings::LanguageServer;
    // PATH lookups are not free: remember them per command for the page.
    let cache_id = egui::Id::new("settings-lsp-path-cache");
    let mut cache: std::collections::HashMap<String, bool> = ui
        .ctx()
        .data_mut(|d| d.get_temp(cache_id))
        .unwrap_or_default();
    let mut changed = false;
    let mut seen: Vec<&str> = Vec::new();
    let (command_w, args_w) = extension_field_widths(ui);
    egui::Grid::new("settings-lsp-table")
        .num_columns(5)
        .spacing([12.0, 6.0])
        .striped(true)
        .show(ui, |ui| {
            for key in [
                "settings-editor-lsp-language",
                "settings-editor-lsp-command",
                "settings-editor-lsp-args",
                "settings-editor-lsp-enabled",
                "settings-editor-lsp-status",
            ] {
                ui.label(
                    egui::RichText::new(s.t(key))
                        .strong()
                        .color(theme.text_muted),
                );
            }
            ui.end_row();
            for spec in hs_lsp::registry::DEFAULTS {
                if seen.contains(&spec.language) {
                    continue;
                }
                seen.push(spec.language);
                let idx = rows.iter().position(|l| l.language == spec.language);
                let mut row = idx
                    .map(|i| rows[i].clone())
                    .unwrap_or_else(|| LanguageServer {
                        language: spec.language.to_string(),
                        command: String::new(),
                        args: String::new(),
                        enabled: true,
                    });
                ui.label(spec.language);
                // A grid cell is as wide as its column was last frame, so a
                // desired_width alone is clamped to the old, narrow value and
                // never grows. The scope gives the cell the width instead.
                let r1 = ui
                    .scope(|ui| {
                        ui.set_min_width(command_w);
                        ui.add(
                            egui::TextEdit::singleline(&mut row.command)
                                .hint_text(spec.command)
                                .desired_width(command_w),
                        )
                    })
                    .inner;
                let r2 = ui
                    .scope(|ui| {
                        ui.set_min_width(args_w);
                        ui.add(
                            egui::TextEdit::singleline(&mut row.args)
                                .hint_text(spec.args.join(" "))
                                .desired_width(args_w),
                        )
                    })
                    .inner;
                let r3 = ui.checkbox(&mut row.enabled, "");
                let command = if row.command.trim().is_empty() {
                    spec.command.to_string()
                } else {
                    row.command.trim().to_string()
                };
                if !row.enabled || command.is_empty() {
                    ui.label(
                        egui::RichText::new(s.t("settings-editor-lsp-off")).color(theme.text_muted),
                    );
                } else {
                    let found = *cache
                        .entry(command.clone())
                        .or_insert_with(|| hs_lsp::registry::on_path(&command).is_some());
                    if found {
                        ui.colored_label(
                            theme.success,
                            format!(
                                "{} {}",
                                hs_ui::icons::CHECK,
                                s.t("settings-editor-lsp-found")
                            ),
                        );
                    } else {
                        ui.horizontal(|ui| {
                            ui.colored_label(
                                theme.warning,
                                format!(
                                    "{} {}",
                                    hs_ui::icons::WARNING,
                                    s.t("settings-editor-lsp-not-found")
                                ),
                            )
                            .tip(s.i18n.t_args(
                                "settings-editor-lsp-install-tip",
                                &[("command", spec.install)],
                            ));
                            install_button(ui, s, spec.install_command, spec.install_url, effects);
                        });
                    }
                }
                ui.end_row();
                if r1.changed() || r2.changed() || r3.changed() {
                    changed = true;
                    cache.clear();
                    let is_default =
                        row.command.trim().is_empty() && row.args.trim().is_empty() && row.enabled;
                    match (idx, is_default) {
                        (Some(i), true) => {
                            rows.remove(i);
                        }
                        (Some(i), false) => rows[i] = row,
                        (None, true) => {}
                        (None, false) => rows.push(row),
                    }
                }
            }
        });
    ui.ctx().data_mut(|d| d.insert_temp(cache_id, cache));
    changed
}

/// Settings → Browser: defaults for new pages and the extra blocklist.
fn browser(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-browser"),
        Some(&s.t("settings-browser-sub")),
    );
    let mut settings = s.settings.write();
    let mut changed = false;
    form::section(ui, theme, &s.t("settings-browser-section-pages"), |ui| {
        form::field_grid(ui, "browser-grid", |ui| {
            form::label(ui, &s.t("settings-browser-home"));
            changed |= form::text_field(ui, &mut settings.browser.home_url, "https://…").changed();
            ui.end_row();
        });
        changed |= ui
            .checkbox(
                &mut settings.browser.adblock,
                s.t("settings-browser-adblock"),
            )
            .changed();
        changed |= ui
            .checkbox(&mut settings.browser.javascript, s.t("settings-browser-js"))
            .changed();
        changed |= ui
            .checkbox(&mut settings.browser.dark, s.t("settings-browser-dark"))
            .changed();
        form::hint(ui, theme, &s.t("settings-browser-pages-hint"));
        changed |= ui
            .checkbox(
                &mut settings.browser.devtools_panel,
                s.t("settings-browser-devtools-panel"),
            )
            .changed();
        form::note(
            ui,
            theme,
            hs_ui::icons::ph::WARNING,
            &s.t("settings-browser-devtools-panel-hint"),
        );
    });
    form::section(ui, theme, &s.t("settings-browser-section-favbar"), |ui| {
        changed |= ui
            .checkbox(
                &mut settings.browser.favorites_bar,
                s.t("settings-browser-favbar"),
            )
            .changed();
        form::hint(ui, theme, &s.t("settings-browser-favbar-hint"));
    });
    form::section(
        ui,
        theme,
        &s.t("settings-browser-section-blocklist"),
        |ui| {
            let mut text = settings.browser.blocklist.join("\n");
            if ui
                .add(
                    egui::TextEdit::multiline(&mut text)
                        .hint_text("ads.example.com")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(6),
                )
                .changed()
            {
                settings.browser.blocklist = text
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                changed = true;
            }
            form::hint(ui, theme, &s.t("settings-browser-blocklist-hint"));
        },
    );
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

/// The third-party inventory shown on About. It lives at the repository root
/// and is compiled in so an installed copy can show it without the source
/// tree (docs/03 §7, `THIRD_PARTY_NOTICES.md`).
const THIRD_PARTY_NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");

fn about(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    form::page_title(
        ui,
        theme,
        &s.t("app-name"),
        Some(&s.t("settings-about-sub")),
    );
    updates(ui, theme, s, effects);
    form::section(ui, theme, &s.t("settings-about-credits"), |ui| {
        form::field_grid(ui, "about", |ui| {
            form::label(ui, &s.t("settings-about-signing-key"));
            ui.vertical(|ui| {
                for k in hs_core::update::SIGNING_KEYS {
                    ui.monospace(*k);
                }
            });
            ui.end_row();
            form::label(ui, &s.t("settings-about-project"));
            ui.hyperlink(hs_core::update::FORGE_URL);
            ui.end_row();
        });
        ui.add_space(6.0);
        form::hint(ui, theme, &s.t("settings-about-license"));
        form::hint(ui, theme, &s.t("settings-about-fonts"));
        form::hint(ui, theme, &s.t("settings-about-icons"));
    });
    egui::CollapsingHeader::new(s.t("settings-about-notices"))
        .id_salt("about-notices")
        .show(ui, |ui| {
            let mut text = THIRD_PARTY_NOTICES;
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .desired_width(f32::INFINITY)
                    .interactive(false),
            );
        });
}

/// SSH host form (docs/tools/ssh-sftp.md §4) bound to the profile's `[ssh]`
/// table. Secrets typed here go to the vault on save and are kept as refs.
fn ssh_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
    effects: &mut Vec<SettingsEffect>,
) {
    use hs_ssh::SshFields;
    use hs_ui::varfield;
    let mut f = SshFields::from_table(&draft.fields);
    let mut changed = false;
    // Variables for `{{ }}` autocomplete, from the connection's environment
    // (or the active one). New keys the user adds inline are collected and
    // written back afterwards.
    let vars = s
        .environments
        .read()
        .available_vars(draft.environment.as_deref());
    let mut new_vars: Vec<(String, String)> = Vec::new();
    form::section(ui, theme, &s.t("conn-ssh-section-host"), |ui| {
        form::field_grid(ui, "ssh-host", |ui| {
            form::label(ui, &s.t("conn-ssh-host"));
            ui.horizontal(|ui| {
                let o = varfield::var_field_width(
                    ui,
                    theme,
                    "ssh-host-var",
                    &mut f.host,
                    "web-01.example.com",
                    &vars,
                    320.0,
                );
                changed |= o.changed;
                if let Some(kv) = o.add_var {
                    new_vars.push(kv);
                }
                ui.label(s.t("conn-ssh-port"));
                let mut port = f.port as i64;
                if ui
                    .add(egui::DragValue::new(&mut port).range(1..=65535))
                    .changed()
                {
                    f.port = port as u16;
                    changed = true;
                }
            });
            ui.end_row();
            form::label(ui, &s.t("conn-ssh-user"));
            let o = varfield::var_field(ui, theme, "ssh-user-var", &mut f.user, "ubuntu", &vars);
            changed |= o.changed;
            if let Some(kv) = o.add_var {
                new_vars.push(kv);
            }
            ui.end_row();
            form::label(ui, &s.t("conn-ssh-remote-dir"));
            let o = varfield::var_field(
                ui,
                theme,
                "ssh-remote-var",
                &mut f.remote_dir,
                "/var/www",
                &vars,
            );
            changed |= o.changed;
            if let Some(kv) = o.add_var {
                new_vars.push(kv);
            }
            ui.end_row();
            form::label(ui, &s.t("conn-ssh-startup"));
            let o = varfield::var_field(
                ui,
                theme,
                "ssh-startup-var",
                &mut f.startup_command,
                "",
                &vars,
            );
            changed |= o.changed;
            if let Some(kv) = o.add_var {
                new_vars.push(kv);
            }
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-ssh-startup-hint"));
    });
    if !new_vars.is_empty() {
        let mut envs = s.environments.write();
        for (k, v) in &new_vars {
            match envs.set_var(draft.environment.as_deref(), k, v) {
                Ok(()) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Success,
                    s.i18n.t_args("env-var-added", &[("key", k)]),
                )),
                Err(e) => effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                )),
            }
        }
        drop(envs);
        effects.push(SettingsEffect::EnvironmentsChanged);
    }
    form::section(ui, theme, &s.t("conn-ssh-section-auth"), |ui| {
        form::field_grid(ui, "ssh-auth", |ui| {
            form::label(ui, &s.t("conn-ssh-auth"));
            egui::ComboBox::from_id_salt("ssh-auth-kind")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(s.t(match f.auth.as_str() {
                    "key" => "conn-ssh-auth-key",
                    "agent" => "conn-ssh-auth-agent",
                    _ => "conn-ssh-auth-password",
                }))
                .show_ui(ui, |ui| {
                    for (id, key) in [
                        ("password", "conn-ssh-auth-password"),
                        ("key", "conn-ssh-auth-key"),
                        ("agent", "conn-ssh-auth-agent"),
                    ] {
                        if ui.selectable_label(f.auth == id, s.t(key)).clicked() {
                            f.auth = id.into();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            match f.auth.as_str() {
                "key" => {
                    form::label(ui, &s.t("conn-ssh-key-path"));
                    ui.horizontal(|ui| {
                        changed |=
                            form::text_field_width(ui, &mut f.key_path, "~/.ssh/id_ed25519", 320.0)
                                .changed();
                        if ui.button(s.t("conn-ssh-browse")).clicked()
                            && let Some(p) = rfd::FileDialog::new().pick_file()
                        {
                            f.key_path = p.to_string_lossy().to_string();
                            changed = true;
                        }
                    });
                    ui.end_row();
                    form::label(ui, &s.t("conn-ssh-key-passphrase"));
                    changed |= secret_field(
                        ui,
                        theme,
                        s,
                        "ssh-passphrase",
                        &mut f.key_passphrase,
                        scratch,
                        1,
                    );
                    ui.end_row();
                }
                "agent" => {
                    form::label(ui, "");
                    form::hint(ui, theme, &s.t("conn-ssh-agent-hint"));
                    ui.end_row();
                }
                _ => {
                    form::label(ui, &s.t("conn-ssh-password"));
                    changed |=
                        secret_field(ui, theme, s, "ssh-password", &mut f.password, scratch, 0);
                    ui.end_row();
                }
            }
        });
        form::hint(ui, theme, &s.t("conn-ssh-secret-hint"));
    });
    if changed {
        f.write_into(&mut draft.fields);
    }
    let _ = effects;
}

/// Move secrets the user typed into a profile form into the vault, leaving
/// `secret:<id>` references in the profile.
/// Move typed secrets into the vault. Returns false when one could not be
/// stored (vault locked, write failed): the caller must not save the
/// profile then, or the typed text would reach the TOML file in plaintext.
/// `{{variables}}` are references, not secrets, and stay as typed.
/// Takes any iterable of `(what it is, the value)`: an array at a call
/// site that knows its fields, or a `Vec` from a type that owns the list
/// of which of its own fields are sensitive (`ContainerFields`). The
/// second form matters - a field added to such a type later is covered
/// without anyone remembering to edit this call.
pub(crate) fn store_typed_secrets<'a>(
    s: &Services,
    profile_name: &str,
    fields: impl IntoIterator<Item = (&'a str, &'a mut String)>,
    effects: &mut Vec<SettingsEffect>,
) -> bool {
    let mut ok = true;
    for (label, v) in fields {
        if v.is_empty()
            || hs_core::secrets::SecretRef::parse(v).is_some()
            || (v.trim().starts_with("{{") && v.trim().ends_with("}}"))
        {
            continue;
        }
        if s.secrets.is_locked() {
            effects.push(SettingsEffect::Toast(
                hs_core::tool::ToastLevel::Warning,
                s.t("settings-vault-unlock-first"),
            ));
            ok = false;
            continue;
        }
        match s.secrets.create(
            &format!("{profile_name}: {label}"),
            &SecretString::from(v.clone()),
        ) {
            Ok(r) => *v = r.to_string_ref(),
            Err(e) => {
                effects.push(SettingsEffect::Toast(
                    hs_core::tool::ToastLevel::Error,
                    e.to_string(),
                ));
                ok = false;
            }
        }
    }
    ok
}

/// The `kind = "s3"` form: provider preset, endpoint, region, keys (into
/// the vault on save), path style, default bucket.
fn s3_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use hs_core::objstore::profile::{PROVIDERS, provider_label, provider_preset};
    let mut f = S3Fields::from_table(&draft.fields);
    let mut changed = false;
    form::section(ui, theme, &s.t("conn-s3-section-endpoint"), |ui| {
        form::field_grid(ui, "s3-endpoint", |ui| {
            form::label(ui, &s.t("conn-s3-provider"));
            egui::ComboBox::from_id_salt("s3-provider")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(provider_label(&f.provider))
                .show_ui(ui, |ui| {
                    for p in PROVIDERS {
                        if ui
                            .selectable_label(f.provider == *p, provider_label(p))
                            .clicked()
                            && f.provider != *p
                        {
                            f.provider = (*p).to_string();
                            let (endpoint, path_style) = provider_preset(p);
                            f.endpoint = endpoint.to_string();
                            f.path_style = path_style;
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("conn-s3-endpoint"));
            changed |=
                form::text_field(ui, &mut f.endpoint, &s.t("conn-s3-endpoint-hint")).changed();
            ui.end_row();
            form::label(ui, &s.t("conn-s3-region"));
            changed |= form::text_field(ui, &mut f.region, "us-east-1").changed();
            ui.end_row();
            form::label(ui, "");
            changed |= ui
                .checkbox(&mut f.path_style, s.t("conn-s3-path-style"))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("conn-s3-default-bucket"));
            changed |= form::text_field(ui, &mut f.default_bucket, "").changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-s3-endpoint-help"));
    });
    form::section(ui, theme, &s.t("conn-ssh-section-auth"), |ui| {
        form::field_grid(ui, "s3-auth", |ui| {
            form::label(ui, &s.t("conn-s3-access-key"));
            changed |= form::text_field(ui, &mut f.access_key, "AKIA…").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-s3-secret-key"));
            changed |= secret_field(ui, theme, s, "s3-secret", &mut f.secret_key, scratch, 0);
            ui.end_row();
            form::label(ui, &s.t("conn-s3-session-token"));
            changed |= secret_field(ui, theme, s, "s3-token", &mut f.session_token, scratch, 1);
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-ssh-secret-hint"));
    });
    if changed {
        f.write_into(&mut draft.fields);
    }
}

/// The WebDAV form. One URL, a user and a password: a DAV share has no
/// bucket level and nothing else to configure.
fn webdav_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use hs_core::objstore::http_profile::WebdavFields;
    let mut f = WebdavFields::from_table(&draft.fields);
    let mut changed = false;
    form::section(ui, theme, &s.t("conn-webdav-section"), |ui| {
        form::field_grid(ui, "webdav-fields", |ui| {
            form::label(ui, &s.t("conn-webdav-url"));
            changed |= form::text_field(ui, &mut f.url, &s.t("conn-webdav-url-hint")).changed();
            ui.end_row();
            form::label(ui, &s.t("conn-user"));
            changed |= form::text_field(ui, &mut f.user, "").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-password"));
            changed |= secret_field(ui, theme, s, "webdav-pw", &mut f.password, scratch, 0);
            ui.end_row();
            form::label(ui, "");
            changed |= ui
                .checkbox(&mut f.insecure_tls, s.t("conn-insecure-tls"))
                .tip(s.t("conn-insecure-tls-hint"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-webdav-url-help"));
    });
    if changed {
        f.write_into(&mut draft.fields);
    }
}

/// The Nexus form: the **server**, not a repository - the tool lists
/// them. One repository can be named to narrow the list for an account
/// that cannot see them all.
fn nexus_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use hs_core::objstore::http_profile::NexusFields;
    let mut f = NexusFields::from_table(&draft.fields);
    let mut changed = false;
    form::section(ui, theme, &s.t("conn-nexus-section"), |ui| {
        form::field_grid(ui, "nexus-fields", |ui| {
            form::label(ui, &s.t("conn-nexus-url"));
            changed |= form::text_field(ui, &mut f.url, "https://nexus.example.com").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-user"));
            changed |= form::text_field(ui, &mut f.user, "").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-nexus-token"));
            changed |= secret_field(ui, theme, s, "nexus-pw", &mut f.password, scratch, 0);
            ui.end_row();
            form::label(ui, &s.t("conn-nexus-repository"));
            changed |= form::text_field(ui, &mut f.repository, &s.t("conn-nexus-repository-hint"))
                .changed();
            ui.end_row();
            form::label(ui, "");
            changed |= ui
                .checkbox(&mut f.insecure_tls, s.t("conn-insecure-tls"))
                .tip(s.t("conn-insecure-tls-hint"))
                .changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-nexus-help"));
    });
    if changed {
        f.write_into(&mut draft.fields);
    }
}

/// The container form (`docker`, `podman`, `k8s`, `wincontainer` —
/// ADR-026, `docs/tools/containers.md` §10.7). One form for all four
/// kinds, because they overlap heavily and the kind decides which
/// sections are shown.
///
/// Secrets are typed in the clear here and moved into the vault on save
/// by `container_secrets`, exactly as every other kind does. The
/// exception is the kubeconfig **path**, which is a path and stays one
/// (§10.2).
fn container_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use hs_container::profile::{ContainerFields, Transport};
    let cluster = hs_core::connections::is_cluster_kind(&draft.kind);
    let mut f = ContainerFields::from_table(&draft.fields);
    let mut changed = false;

    if cluster {
        form::section(ui, theme, &s.t("conn-container-section-cluster"), |ui| {
            form::field_grid(ui, "container-cluster", |ui| {
                form::label(ui, &s.t("conn-container-kubeconfig"));
                ui.horizontal(|ui| {
                    changed |=
                        form::text_field(ui, &mut f.kubeconfig_path, "~/.kube/config").changed();
                    if ui.button(s.t("settings-container-browse")).clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_file()
                    {
                        f.kubeconfig_path = p.to_string_lossy().to_string();
                        changed = true;
                    }
                });
                ui.end_row();
                form::label(ui, &s.t("conn-container-context"));
                changed |= form::text_field(ui, &mut f.context, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-container-namespace"));
                changed |= form::text_field(ui, &mut f.namespace, "default").changed();
                ui.end_row();
            });
            // Why a path and not a copy, said where the choice is made.
            form::hint(ui, theme, &s.t("conn-container-kubeconfig-hint"));
            ui.add_space(6.0);
            form::field_grid(ui, "container-cluster-auth", |ui| {
                form::label(ui, &s.t("conn-container-paste-kubeconfig"));
                changed |= form::code_field(ui, &mut f.kubeconfig_inline, 4).changed();
                ui.end_row();
                form::label(ui, &s.t("conn-container-token"));
                changed |=
                    form::secret_field(ui, &mut f.token, "", form::FIELD_MIN_WIDTH).changed();
                ui.end_row();
            });
            form::note(
                ui,
                theme,
                hs_ui::icons::INFO,
                &s.t("conn-container-paste-kubeconfig-note"),
            );
            ui.add_space(6.0);
            form::field_grid(ui, "container-cluster-extra", |ui| {
                form::label(ui, &s.t("conn-container-impersonate-user"));
                changed |= form::text_field(ui, &mut f.impersonate_user, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-container-impersonate-group"));
                changed |= form::text_field(ui, &mut f.impersonate_group, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-container-proxy"));
                changed |= form::text_field(ui, &mut f.proxy_url, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-container-use-kubectl"));
                changed |= ui
                    .checkbox(&mut f.use_kubectl, "")
                    .tip(s.t("conn-container-use-kubectl-hint"))
                    .changed();
                ui.end_row();
            });
            form::hint(ui, theme, &s.t("conn-container-use-kubectl-hint"));
        });
    } else {
        form::section(ui, theme, &s.t("conn-container-section-engine"), |ui| {
            form::field_grid(ui, "container-engine", |ui| {
                form::label(ui, &s.t("conn-container-transport"));
                egui::ComboBox::from_id_salt("container-transport")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(s.t(f.transport.key()))
                    .show_ui(ui, |ui| {
                        for t in Transport::ALL {
                            if ui
                                .selectable_label(f.transport == t, s.t(t.key()))
                                .clicked()
                            {
                                f.transport = t;
                                changed = true;
                            }
                        }
                    });
                ui.end_row();
                // Only the fields the chosen transport actually uses, so
                // the form says what matters rather than everything.
                match f.transport {
                    Transport::Local => {
                        form::label(ui, &s.t("conn-container-socket"));
                        changed |=
                            form::text_field(ui, &mut f.socket, "/var/run/docker.sock").changed();
                        ui.end_row();
                    }
                    Transport::Tcp => {
                        form::label(ui, &s.t("conn-host"));
                        changed |= form::text_field(ui, &mut f.host, "127.0.0.1").changed();
                        ui.end_row();
                        form::label(ui, &s.t("conn-port"));
                        changed |= ui
                            .add(egui::DragValue::new(&mut f.port).range(1..=65535))
                            .changed();
                        ui.end_row();
                    }
                    Transport::Ssh => {
                        form::label(ui, &s.t("conn-container-over-ssh"));
                        changed |= ssh_profile_picker(ui, s, &mut f.tunnel_ssh);
                        ui.end_row();
                    }
                    Transport::Wsl => {
                        form::label(ui, &s.t("conn-container-distro"));
                        changed |= form::text_field(ui, &mut f.distro, "Ubuntu-24.04").changed();
                        ui.end_row();
                    }
                }
                form::label(ui, &s.t("conn-container-api-version"));
                changed |= form::text_field(ui, &mut f.api_version, "").changed();
                ui.end_row();
            });
            form::hint(ui, theme, &s.t("conn-container-transport-hint"));
        });
        if f.transport == Transport::Tcp {
            form::section(ui, theme, &s.t("conn-container-section-tls"), |ui| {
                form::field_grid(ui, "container-tls", |ui| {
                    form::label(ui, &s.t("conn-container-tls"));
                    changed |= ui.checkbox(&mut f.tls, "").changed();
                    ui.end_row();
                    form::label(ui, &s.t("conn-container-tls-ca"));
                    changed |= form::text_field(ui, &mut f.tls_ca, "").changed();
                    ui.end_row();
                    form::label(ui, &s.t("conn-container-tls-cert"));
                    changed |= form::text_field(ui, &mut f.tls_cert, "").changed();
                    ui.end_row();
                    form::label(ui, &s.t("conn-container-tls-key"));
                    changed |=
                        form::secret_field(ui, &mut f.tls_key, "", form::FIELD_MIN_WIDTH).changed();
                    ui.end_row();
                });
                form::hint(ui, theme, &s.t("conn-container-tls-hint"));
            });
        }
        form::section(ui, theme, &s.t("conn-container-section-registry"), |ui| {
            form::field_grid(ui, "container-registry", |ui| {
                form::label(ui, &s.t("conn-container-registry"));
                changed |= form::text_field(ui, &mut f.registry, "registry.example.com").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-user"));
                changed |= form::text_field(ui, &mut f.registry_user, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-password"));
                changed |=
                    form::secret_field(ui, &mut f.registry_password, "", form::FIELD_MIN_WIDTH)
                        .changed();
                ui.end_row();
            });
        });
    }

    form::section(ui, theme, &s.t("conn-container-section-common"), |ui| {
        form::field_grid(ui, "container-common", |ui| {
            form::label(ui, &s.t("conn-container-timeout"));
            changed |= ui
                .add(egui::DragValue::new(&mut f.connect_timeout_secs).range(1..=300))
                .changed();
            ui.end_row();
            form::label(ui, &s.t("conn-container-insecure-tls"));
            changed |= ui.checkbox(&mut f.insecure_tls, "").changed();
            ui.end_row();
        });
        if f.insecure_tls {
            // Say what it gives up, rather than only that it is on.
            form::note(
                ui,
                theme,
                hs_ui::icons::WARNING,
                &s.t("conn-container-insecure-tls-note"),
            );
        }
    });

    if changed {
        f.write_into(&mut draft.fields);
        *scratch = toml::to_string_pretty(&draft.fields).unwrap_or_default();
    }
}

/// A picker over the saved `ssh` profiles, for reaching an engine on
/// another machine. Returns whether the choice changed.
fn ssh_profile_picker(ui: &mut egui::Ui, s: &Services, chosen: &mut String) -> bool {
    let profiles: Vec<(String, String)> = s
        .connections
        .read()
        .all()
        .iter()
        .filter(|p| p.kind == "ssh")
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();
    let mut changed = false;
    let label = profiles
        .iter()
        .find(|(id, _)| id == chosen)
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| s.t("conn-container-no-ssh"));
    egui::ComboBox::from_id_salt("container-ssh")
        .width(form::FIELD_MIN_WIDTH)
        .selected_text(label)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(chosen.is_empty(), s.t("conn-container-no-ssh"))
                .clicked()
            {
                chosen.clear();
                changed = true;
            }
            for (id, name) in &profiles {
                if ui.selectable_label(chosen == id, name).clicked() {
                    *chosen = id.clone();
                    changed = true;
                }
            }
        });
    changed
}

/// The `kind = "db"` form: engine, server or file, credentials (password
/// into the vault on save), TLS mode, SSH tunnel, and a pasted connection URL.
fn db_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use tool_db::driver::Engine;
    use tool_db::profile::{SSL_MODES, parse_url};
    let mut f = DbFields::from_table(&draft.fields);
    let mut changed = false;
    let url_id = egui::Id::new(("db-url", draft.id.clone()));
    let mut url: String = ui.data_mut(|d| d.get_temp(url_id).unwrap_or_default());
    form::section(ui, theme, &s.t("conn-db-section-server"), |ui| {
        form::field_grid(ui, "db-server", |ui| {
            form::label(ui, &s.t("conn-db-url"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut url)
                        .hint_text("postgres://user@host:5432/db")
                        .desired_width(form::FIELD_MIN_WIDTH - 70.0),
                );
                if ui
                    .add_enabled(
                        !url.trim().is_empty(),
                        egui::Button::new(s.t("conn-db-apply-url")),
                    )
                    .clicked()
                {
                    if let Some((parsed, password)) = parse_url(&url) {
                        let tunnel = std::mem::take(&mut f.tunnel_ssh);
                        f = parsed;
                        f.tunnel_ssh = tunnel;
                        if let Some(p) = password {
                            // Typed text until save moves it into the vault.
                            f.password = p;
                        }
                        url.clear();
                        changed = true;
                    } else {
                        url.clear();
                    }
                }
            });
            ui.end_row();
            form::label(ui, &s.t("conn-db-engine"));
            let engines = [
                ("postgres", "PostgreSQL"),
                ("mysql", "MySQL"),
                ("mariadb", "MariaDB"),
                ("sqlite", "SQLite"),
            ];
            let current = engines
                .iter()
                .find(|(id, _)| f.engine.eq_ignore_ascii_case(id))
                .map(|(_, l)| *l)
                .unwrap_or("?");
            egui::ComboBox::from_id_salt("db-engine")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (id, label) in engines {
                        if ui.selectable_label(f.engine == id, label).clicked() && f.engine != id {
                            let old_default = f.engine().map(Engine::default_port);
                            f.engine = id.to_string();
                            if old_default == Some(f.port) || f.port == 0 {
                                f.port = f.engine().map(Engine::default_port).unwrap_or(0);
                            }
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            if f.engine() == Some(Engine::Sqlite) {
                form::label(ui, &s.t("conn-db-file"));
                ui.horizontal(|ui| {
                    changed |= ui
                        .add(
                            egui::TextEdit::singleline(&mut f.path)
                                .hint_text("C:/data/app.db")
                                .desired_width(form::FIELD_MIN_WIDTH - 70.0),
                        )
                        .changed();
                    if ui.button(s.t("conn-db-browse")).clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_file()
                    {
                        f.path = p.to_string_lossy().to_string();
                        changed = true;
                    }
                });
                ui.end_row();
            } else {
                form::label(ui, &s.t("conn-db-host"));
                changed |= form::text_field(ui, &mut f.host, "localhost").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-db-port"));
                let mut port = f.port.to_string();
                if form::text_field(ui, &mut port, "5432").changed() {
                    f.port = port.trim().parse().unwrap_or(f.port);
                    changed = true;
                }
                ui.end_row();
                form::label(ui, &s.t("conn-db-database"));
                changed |= form::text_field(ui, &mut f.database, "").changed();
                ui.end_row();
            }
        });
        form::hint(ui, theme, &s.t("conn-db-server-hint"));
    });
    if f.engine() != Some(Engine::Sqlite) {
        form::section(ui, theme, &s.t("conn-ssh-section-auth"), |ui| {
            form::field_grid(ui, "db-auth", |ui| {
                form::label(ui, &s.t("conn-db-user"));
                changed |= form::text_field(ui, &mut f.user, "").changed();
                ui.end_row();
                form::label(ui, &s.t("conn-db-password"));
                changed |= secret_field(ui, theme, s, "db-password", &mut f.password, scratch, 0);
                ui.end_row();
                form::label(ui, &s.t("conn-db-ssl"));
                egui::ComboBox::from_id_salt("db-ssl")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(f.ssl.clone())
                    .show_ui(ui, |ui| {
                        for m in SSL_MODES {
                            if ui.selectable_label(f.ssl == *m, *m).clicked() {
                                f.ssl = (*m).to_string();
                                changed = true;
                            }
                        }
                    });
                ui.end_row();
                if f.ssl.starts_with("verify") {
                    form::label(ui, &s.t("conn-db-ssl-ca"));
                    changed |=
                        form::text_field(ui, &mut f.ssl_ca, &s.t("conn-db-ssl-ca-hint")).changed();
                    ui.end_row();
                }
                form::label(ui, &s.t("conn-db-tunnel"));
                let hosts: Vec<(String, String)> = s
                    .connections
                    .read()
                    .of_kind("ssh")
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect();
                let none = s.t("conn-db-no-tunnel");
                let current = hosts
                    .iter()
                    .find(|(id, _)| *id == f.tunnel_ssh)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| none.clone());
                egui::ComboBox::from_id_salt("db-tunnel")
                    .width(form::FIELD_MIN_WIDTH)
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(f.tunnel_ssh.is_empty(), none.clone())
                            .clicked()
                        {
                            f.tunnel_ssh.clear();
                            changed = true;
                        }
                        for (id, name) in hosts {
                            if ui.selectable_label(f.tunnel_ssh == id, name).clicked() {
                                f.tunnel_ssh = id;
                                changed = true;
                            }
                        }
                    });
                ui.end_row();
            });
            form::hint(ui, theme, &s.t("conn-db-auth-hint"));
        });
    }
    ui.data_mut(|d| d.insert_temp(url_id, url));
    if changed {
        f.write_into(&mut draft.fields);
    }
}

/// The `kind = "redis"` form: server, ACL credentials (password into the
/// vault on save), TLS, SSH tunnel, the key separator, and a pasted
/// `redis://` URL.
fn redis_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use tool_redis::profile::{MODES, RedisFields, mode_label, parse_url};
    let mut f = RedisFields::from_table(&draft.fields);
    let mut changed = false;
    let url_id = egui::Id::new(("redis-url", draft.id.clone()));
    let mut url: String = ui.data_mut(|d| d.get_temp(url_id).unwrap_or_default());
    form::section(ui, theme, &s.t("conn-redis-section-server"), |ui| {
        form::field_grid(ui, "redis-server", |ui| {
            form::label(ui, &s.t("conn-redis-url"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut url)
                        .hint_text("redis://user@host:6379/0")
                        .desired_width(form::FIELD_MIN_WIDTH - 70.0),
                );
                if ui
                    .add_enabled(
                        !url.trim().is_empty(),
                        egui::Button::new(s.t("conn-redis-import")),
                    )
                    .clicked()
                {
                    if let Some(parsed) = parse_url(&url) {
                        // The tunnel and the separator are the user's choice,
                        // not the URL's.
                        let tunnel = std::mem::take(&mut f.tunnel_ssh);
                        let delimiter = std::mem::take(&mut f.delimiter);
                        f = parsed;
                        f.tunnel_ssh = tunnel;
                        f.delimiter = delimiter;
                        changed = true;
                    }
                    url.clear();
                }
            });
            ui.end_row();
            form::label(ui, &s.t("conn-redis-host"));
            changed |= form::text_field(ui, &mut f.host, "127.0.0.1").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-redis-port"));
            let mut port = f.port.to_string();
            if form::text_field(ui, &mut port, "6379").changed() {
                f.port = port.trim().parse().unwrap_or(f.port);
                changed = true;
            }
            ui.end_row();
            form::label(ui, &s.t("conn-redis-db"));
            let mut db = f.db.to_string();
            if form::text_field(ui, &mut db, "0").changed() {
                f.db = db.trim().parse().unwrap_or(f.db);
                changed = true;
            }
            ui.end_row();
            form::label(ui, &s.t("conn-redis-mode"));
            egui::ComboBox::from_id_salt("redis-mode")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(mode_label(&f.mode))
                .show_ui(ui, |ui| {
                    for m in MODES {
                        if ui.selectable_label(f.mode == *m, mode_label(m)).clicked() {
                            f.mode = (*m).to_string();
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-redis-url-hint"));
        if f.mode != "standalone" {
            form::note(
                ui,
                theme,
                hs_ui::icons::ph::WARNING,
                &s.t("conn-redis-mode-hint"),
            );
        }
    });
    form::section(ui, theme, &s.t("conn-ssh-section-auth"), |ui| {
        form::field_grid(ui, "redis-auth", |ui| {
            form::label(ui, &s.t("conn-redis-user"));
            changed |= form::text_field(ui, &mut f.user, "default").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-redis-password"));
            changed |= secret_field(ui, theme, s, "redis-pass", &mut f.password, scratch, 0);
            ui.end_row();
            form::label(ui, &s.t("conn-redis-tls"));
            changed |= ui.checkbox(&mut f.tls, "").changed();
            ui.end_row();
            if f.tls {
                form::label(ui, &s.t("conn-redis-tls-ca"));
                changed |= form::text_field(ui, &mut f.tls_ca, "ca.pem").changed();
                ui.end_row();
            }
            form::label(ui, &s.t("conn-db-tunnel"));
            let hosts: Vec<(String, String)> = s
                .connections
                .read()
                .of_kind("ssh")
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect();
            let none = s.t("conn-db-no-tunnel");
            let current = hosts
                .iter()
                .find(|(id, _)| *id == f.tunnel_ssh)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| none.clone());
            egui::ComboBox::from_id_salt("redis-tunnel")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(f.tunnel_ssh.is_empty(), none.clone())
                        .clicked()
                    {
                        f.tunnel_ssh.clear();
                        changed = true;
                    }
                    for (id, name) in hosts {
                        if ui.selectable_label(f.tunnel_ssh == id, name).clicked() {
                            f.tunnel_ssh = id;
                            changed = true;
                        }
                    }
                });
            ui.end_row();
            form::label(ui, &s.t("conn-redis-delimiter"));
            changed |= form::text_field(ui, &mut f.delimiter, ":").changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-redis-delimiter-hint"));
    });
    ui.data_mut(|d| d.insert_temp(url_id, url));
    if changed {
        f.write_into(&mut draft.fields);
    }
}

/// The `kind = "mongodb"` and `kind = "couchdb"` form: server or URI,
/// credentials (password into the vault on save), TLS, the database the tree
/// opens on, and an SSH tunnel.
fn nosql_form(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    draft: &mut ConnectionProfile,
    scratch: &mut String,
) {
    use tool_nosql::profile::{NoSqlFields, parse_url};
    let mut f = NoSqlFields::from_table(&draft.kind, &draft.fields);
    let mut changed = false;
    let is_mongo = draft.kind == "mongodb";
    let url_id = egui::Id::new(("nosql-url", draft.id.clone()));
    let mut url: String = ui.data_mut(|d| d.get_temp(url_id).unwrap_or_default());
    form::section(ui, theme, &s.t("conn-nosql-section-server"), |ui| {
        form::field_grid(ui, "nosql-server", |ui| {
            form::label(ui, &s.t("conn-nosql-url"));
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut url)
                        .hint_text(if is_mongo {
                            "mongodb://user@host:27017/shop"
                        } else {
                            "http://admin@host:5984/orders"
                        })
                        .desired_width(form::FIELD_MIN_WIDTH - 70.0),
                );
                if ui
                    .add_enabled(
                        !url.trim().is_empty(),
                        egui::Button::new(s.t("conn-nosql-import")),
                    )
                    .clicked()
                {
                    if let Some(parsed) = parse_url(&url) {
                        let tunnel = std::mem::take(&mut f.tunnel_ssh);
                        f = parsed;
                        f.tunnel_ssh = tunnel;
                        // The profile's kind follows the URL that was pasted.
                        draft.kind = f.kind.clone();
                        changed = true;
                    }
                    url.clear();
                }
            });
            ui.end_row();
            form::label(ui, &s.t("conn-nosql-host"));
            changed |= form::text_field(ui, &mut f.host, "127.0.0.1").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-nosql-port"));
            let mut port = f.port.to_string();
            if form::text_field(ui, &mut port, "27017").changed() {
                f.port = port.trim().parse().unwrap_or(f.port);
                changed = true;
            }
            ui.end_row();
            form::label(ui, &s.t("conn-nosql-database"));
            changed |= form::text_field(ui, &mut f.database, "shop").changed();
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-nosql-url-hint"));
    });
    form::section(ui, theme, &s.t("conn-ssh-section-auth"), |ui| {
        form::field_grid(ui, "nosql-auth", |ui| {
            form::label(ui, &s.t("conn-nosql-user"));
            changed |= form::text_field(ui, &mut f.user, "").changed();
            ui.end_row();
            form::label(ui, &s.t("conn-nosql-password"));
            changed |= secret_field(ui, theme, s, "nosql-pass", &mut f.password, scratch, 0);
            ui.end_row();
            if is_mongo {
                form::label(ui, &s.t("conn-nosql-auth-source"));
                changed |= form::text_field(ui, &mut f.auth_source, "admin").changed();
                ui.end_row();
            }
            form::label(ui, &s.t("conn-nosql-tls"));
            changed |= ui.checkbox(&mut f.tls, "").changed();
            ui.end_row();
            if is_mongo {
                form::label(ui, &s.t("conn-nosql-direct"));
                changed |= ui
                    .checkbox(&mut f.direct, s.t("conn-nosql-direct-hint"))
                    .changed();
                ui.end_row();
            }
            form::label(ui, &s.t("conn-db-tunnel"));
            let hosts: Vec<(String, String)> = s
                .connections
                .read()
                .of_kind("ssh")
                .map(|c| (c.id.clone(), c.name.clone()))
                .collect();
            let none = s.t("conn-db-no-tunnel");
            let current = hosts
                .iter()
                .find(|(id, _)| *id == f.tunnel_ssh)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| none.clone());
            egui::ComboBox::from_id_salt("nosql-tunnel")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(f.tunnel_ssh.is_empty(), none.clone())
                        .clicked()
                    {
                        f.tunnel_ssh.clear();
                        changed = true;
                    }
                    for (id, name) in hosts {
                        if ui.selectable_label(f.tunnel_ssh == id, name).clicked() {
                            f.tunnel_ssh = id;
                            changed = true;
                        }
                    }
                });
            ui.end_row();
        });
        form::hint(ui, theme, &s.t("conn-ssh-secret-hint"));
    });
    ui.data_mut(|d| d.insert_temp(url_id, url));
    if changed {
        // The kind may have moved with a pasted URL.
        f.kind = draft.kind.clone();
        f.write_into(&mut draft.fields);
    }
}

/// A vault-backed secret field: shows "stored" when the value is a secret
/// reference, otherwise a password box whose text is moved into the vault on
/// save. `scratch` carries typed-but-unsaved text between frames; `slot`
/// distinguishes several fields sharing one scratch string.
fn secret_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    salt: &str,
    value: &mut String,
    scratch: &mut String,
    slot: usize,
) -> bool {
    use hs_core::secrets::SecretRef;
    let mut changed = false;
    if SecretRef::parse(value).is_some() {
        ui.horizontal(|ui| {
            ui.colored_label(
                theme.success,
                format!("{} {}", hs_ui::icons::LOCK, s.t("conn-ssh-secret-stored")),
            );
            if ui.button(s.t("conn-ssh-change")).clicked() {
                value.clear();
                changed = true;
            }
        });
        return changed;
    }
    // Plain text until saved: keep it in the profile value and store on save.
    let _ = (salt, scratch, slot);
    let r = form::secret_field(
        ui,
        value,
        &s.t("conn-ssh-type-secret"),
        form::FIELD_MIN_WIDTH,
    );
    if r.changed() {
        changed = true;
    }
    if s.secrets.is_locked() && !value.is_empty() {
        ui.colored_label(theme.warning, s.t("settings-vault-unlock-first"));
    }
    changed
}

/// Font picker: bundled fonts first, then the fonts installed on this
/// machine (monospace only when `mono`). Returns true when the choice changed.
fn font_combo(
    ui: &mut egui::Ui,
    s: &Services,
    salt: &str,
    value: &mut String,
    mono: bool,
    default_label: &str,
) -> bool {
    use hs_ui::fonts;
    let mut changed = false;
    let current = fonts::label(value, default_label);
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current)
        .width(form::FIELD_MIN_WIDTH)
        .show_ui(ui, |ui| {
            if !mono
                && ui
                    .selectable_label(value.is_empty(), default_label)
                    .clicked()
            {
                value.clear();
                changed = true;
            }
            for (id, label, _) in fonts::EMBEDDED {
                if ui.selectable_label(value == id, *label).clicked() {
                    *value = (*id).to_string();
                    changed = true;
                }
            }
            ui.separator();
            match fonts::system_fonts() {
                None => {
                    ui.label(egui::RichText::new(s.t("settings-fonts-scanning")).weak());
                }
                Some(list) => {
                    let mut any = false;
                    for f in list.iter().filter(|f| !mono || f.monospace) {
                        any = true;
                        let id = format!("{}{}", fonts::SYSTEM_PREFIX, f.family);
                        if ui.selectable_label(*value == id, &f.family).clicked() {
                            *value = id;
                            changed = true;
                        }
                    }
                    if !any {
                        ui.label(egui::RichText::new(s.t("settings-fonts-none")).weak());
                    }
                }
            }
        });
    changed
}

/// Open a folder in Eden Explorer when it is installed (a new tab of the
/// running window), else in the system file manager. Finding Eden can take a
/// moment, so it runs on the runtime's blocking pool, never the UI thread.
fn open_folder(s: &Services, path: &std::path::Path) {
    let path = path.to_path_buf();
    s.runtime
        .spawn_blocking(move || match hs_core::paths::open_folder(&path) {
            Ok(opener) => tracing::info!("opened {} with {opener:?}", path.display()),
            Err(e) => tracing::warn!("could not open {}: {e}", path.display()),
        });
}

#[cfg(test)]
mod tests {
    #[test]
    fn third_party_notices_name_archify_and_mit() {
        let text = super::THIRD_PARTY_NOTICES;
        assert!(text.contains("Archify"), "Archify notice missing");
        assert!(text.contains("MIT"), "MIT licence text missing");
        assert!(text.contains("208acba8"), "pinned commit missing");
    }
}
