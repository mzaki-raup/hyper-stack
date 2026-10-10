//! Settings pages (docs/04 §9). Shown inside the Settings tab
//! (`tools::settings`): the category list in the sidebar, the page in the tab.
//! Every page is built from `hs_ui::form` cards and label/field grids so the
//! pages share one look.

use std::collections::HashMap;

use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::environments::Environment;
use hs_core::secrets::SecretRef;
use hs_ui::form;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use secrecy::SecretString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Appearance,
    Shortcuts,
    Environments,
    Connections,
    Security,
    Terminal,
    Editor,
    Data,
    About,
}

impl Page {
    pub fn all() -> [(Page, &'static str); 10] {
        [
            (Page::General, "settings-general"),
            (Page::Appearance, "settings-appearance"),
            (Page::Environments, "settings-environments"),
            (Page::Security, "settings-security"),
            (Page::Connections, "settings-connections"),
            (Page::Terminal, "settings-terminal"),
            (Page::Editor, "settings-editor"),
            (Page::Data, "settings-data"),
            (Page::Shortcuts, "settings-shortcuts"),
            (Page::About, "settings-about"),
        ]
    }

    fn icon(self) -> &'static str {
        use hs_ui::icons::ph;
        match self {
            Page::General => ph::SLIDERS_HORIZONTAL,
            Page::Appearance => ph::PALETTE,
            Page::Shortcuts => ph::KEYBOARD,
            Page::Environments => ph::GLOBE_HEMISPHERE_WEST,
            Page::Connections => ph::PLUGS_CONNECTED,
            Page::Security => ph::LOCK_KEY,
            Page::Terminal => ph::TERMINAL_WINDOW,
            Page::Editor => ph::CODE,
            Page::Data => ph::DATABASE,
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
        Page::all().iter().find(|(_, k)| *k == key).map(|(p, _)| *p)
    }
}

/// Settings state plus the effects the pages produced this frame, shared
/// between the shell (which applies effects) and the Settings tab (which
/// draws the pages).
#[derive(Default)]
pub struct SettingsHub {
    pub state: SettingsState,
    pub effects: Vec<SettingsEffect>,
}

pub type SharedSettings = std::sync::Arc<parking_lot::Mutex<SettingsHub>>;

#[derive(Default)]
pub struct SettingsState {
    pub page: Page,
    pub env_selected: Option<String>,
    pub env_draft: Option<(Environment, Vec<KvRow>)>,
    pub conn_selected: Option<String>,
    pub conn_draft: Option<(ConnectionProfile, String)>,
    pub shortcut_edit: HashMap<String, String>,
    pub shortcut_filter: String,
    pub confirm_delete: Option<(String, String, bool)>, // id, name, is_env
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
    ConnectionsChanged,
    Toast(hs_core::tool::ToastLevel, String),
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
        Page::Appearance => appearance(ui, theme, services, effects),
        Page::Shortcuts => shortcuts(ui, theme, services, state, effects),
        Page::Environments => environments(ui, theme, services, state, effects),
        Page::Connections => connections(ui, theme, services, state, effects),
        Page::Security => security(ui, theme, services, effects),
        Page::Terminal => terminal(ui, theme, services, effects),
        Page::Editor => editor(ui, theme, services, effects),
        Page::Data => data(ui, theme, services),
        Page::About => about(ui, theme, services, effects),
    }
}

// ---------------------------------------------------------------------------

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
            let names: Vec<String> = hs_core::workspace::Registry::load(&s.paths)
                .workspaces
                .iter()
                .map(|w| w.name.clone())
                .collect();
            egui::ComboBox::from_id_salt("startup-ws")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(settings.general.startup_workspace.clone())
                .show_ui(ui, |ui| {
                    for n in names {
                        if ui
                            .selectable_label(settings.general.startup_workspace == n, &n)
                            .clicked()
                        {
                            settings.general.startup_workspace = n;
                            changed = true;
                        }
                    }
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
            ui.monospace(s.paths.root().display().to_string());
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

fn appearance(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
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
            form::label(ui, &s.t("settings-color-scheme"));
            let current = Theme::by_name(&settings.appearance.theme).name;
            egui::ComboBox::from_id_salt("color-scheme")
                .width(form::FIELD_MIN_WIDTH)
                .selected_text(current.clone())
                .show_ui(ui, |ui| {
                    for (id, name, dark) in Theme::schemes() {
                        let label = format!(
                            "{}  {}",
                            if *dark {
                                hs_ui::icons::ph::MOON
                            } else {
                                hs_ui::icons::ph::SUN
                            },
                            name
                        );
                        if ui
                            .selectable_label(settings.appearance.theme == *id, label)
                            .clicked()
                        {
                            settings.appearance.theme = (*id).to_string();
                            theme_changed = true;
                        }
                    }
                });
            ui.end_row();
        });
        ui.add_space(6.0);
        // Swatches of the selected scheme.
        ui.horizontal(|ui| {
            for c in [
                theme.window_bg,
                theme.panel_bg,
                theme.surface,
                theme.accent,
                theme.secondary,
                theme.success,
                theme.warning,
                theme.error,
            ] {
                let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(r, egui::CornerRadius::same(3), c);
                ui.painter().rect_stroke(
                    r,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(1.0, theme.border),
                    egui::StrokeKind::Inside,
                );
            }
        });
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
            form::label(ui, &s.t("settings-tab-gap"));
            changed |= ui
                .add(egui::Slider::new(&mut settings.appearance.tab_gap, 0.0..=24.0).suffix(" px"))
                .changed();
            ui.end_row();
        });
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
    ui.horizontal_top(|ui| {
        // Left: the list.
        ui.vertical(|ui| {
            ui.set_width(240.0);
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
        });
        ui.add_space(12.0);
        // Right: the form.
        ui.vertical(|ui| {
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
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(260.0);
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
                let conns: Vec<(String, String, String)> = s
                    .connections
                    .read()
                    .all()
                    .iter()
                    .map(|c| (c.id.clone(), c.kind.clone(), c.name.clone()))
                    .collect();
                if conns.is_empty() {
                    form::hint(ui, theme, &s.t("conn-none"));
                }
                for (id, kind, name) in conns {
                    let selected = state.conn_selected.as_deref() == Some(&id);
                    if form::list_row(
                        ui,
                        theme,
                        selected,
                        hs_ui::icons::ph::PLUGS_CONNECTED,
                        &name,
                        Some(&kind),
                    )
                    .clicked()
                    {
                        state.conn_selected = Some(id.clone());
                        if let Some(c) = s.connections.read().by_id(&id) {
                            let fields = toml::to_string_pretty(&c.fields).unwrap_or_default();
                            state.conn_draft = Some((c.clone(), fields));
                        }
                    }
                    ui.add_space(2.0);
                }
            });
        });
        ui.add_space(12.0);
        ui.vertical(|ui| {
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
                        .selected_text(draft.kind.clone())
                        .show_ui(ui, |ui| {
                            for k in [
                                "db",
                                "ssh",
                                "s3",
                                "api",
                                "docker",
                                "k8s",
                                "redis",
                                "mongodb",
                                "couchdb",
                                "elasticsearch",
                                "dynamodb",
                                "cassandra",
                            ] {
                                ui.selectable_value(&mut draft.kind, k.to_string(), k);
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
                                    .selectable_label(
                                        draft.environment.as_deref() == Some(&id),
                                        name,
                                    )
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
            form::section(ui, theme, &s.t("conn-fields"), |ui| {
                form::code_field(ui, fields_text, 8);
                form::hint(ui, theme, &s.t("conn-fields-hint"));
            });
            ui.horizontal(|ui| {
                if form::primary_button(ui, theme, &s.t("save")).clicked() {
                    match toml::from_str::<toml::Table>(fields_text) {
                        Ok(table) => {
                            let mut c = draft.clone();
                            c.fields = table;
                            match s.connections.write().upsert(c) {
                                Ok(()) => effects.push(SettingsEffect::ConnectionsChanged),
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
                }
                if form::danger_button(ui, theme, &s.t("delete")).clicked() {
                    state.confirm_delete = Some((draft.id.clone(), draft.name.clone(), false));
                }
            });
        });
    });
}

fn security(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
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
}

fn terminal(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
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
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn editor(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-editor"),
        Some(&s.t("settings-editor-sub")),
    );
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
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn data(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    form::page_title(
        ui,
        theme,
        &s.t("settings-data"),
        Some(&s.t("settings-data-sub")),
    );
    form::section(ui, theme, &s.t("settings-data-folder"), |ui| {
        ui.horizontal(|ui| {
            ui.monospace(s.paths.root().display().to_string());
            if ui.button(s.t("settings-data-open-folder")).clicked() {
                open_folder(s.paths.root());
            }
        });
        form::hint(ui, theme, &format!("{:?}", s.paths.source));
        if let Some(r) = &s.paths.fallback_reason {
            ui.colored_label(theme.warning, r);
        }
        ui.add_space(6.0);
        form::note(
            ui,
            theme,
            hs_ui::icons::WARNING,
            &s.t("settings-data-reset"),
        );
    });
}

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
            ui.hyperlink("https://git.zacx.net/owner/hyper-stack");
            ui.end_row();
        });
        ui.add_space(6.0);
        form::hint(ui, theme, &s.t("settings-about-license"));
        form::hint(ui, theme, &s.t("settings-about-fonts"));
        form::hint(ui, theme, &s.t("settings-about-icons"));
    });
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

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
