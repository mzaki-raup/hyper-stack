//! Settings window (docs/04 §9): categories on the left, pages on the right.

use std::collections::HashMap;

use hs_core::Services;
use hs_core::connections::ConnectionProfile;
use hs_core::environments::Environment;
use hs_core::secrets::SecretRef;
use hs_ui::kv_editor::{KvEditor, KvRow};
use hs_ui::theme::Theme;
use secrecy::SecretString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    General,
    Updates,
    Appearance,
    Shortcuts,
    Environments,
    Connections,
    Security,
    Data,
    About,
}

impl Page {
    fn all() -> [(Page, &'static str); 9] {
        [
            (Page::General, "settings-general"),
            (Page::Updates, "settings-updates"),
            (Page::Appearance, "settings-appearance"),
            (Page::Shortcuts, "settings-shortcuts"),
            (Page::Environments, "settings-environments"),
            (Page::Connections, "settings-connections"),
            (Page::Security, "settings-security"),
            (Page::Data, "settings-data"),
            (Page::About, "settings-about"),
        ]
    }
}

#[derive(Default)]
pub struct SettingsState {
    pub open: bool,
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
    LockVault,
    ChangeMasterPassword,
    EnvironmentsChanged,
    ConnectionsChanged,
    Toast(hs_core::tool::ToastLevel, String),
}

pub fn show(
    ctx: &egui::Context,
    theme: &Theme,
    services: &Services,
    state: &mut SettingsState,
) -> Vec<SettingsEffect> {
    let mut effects = Vec::new();
    if !state.open {
        return effects;
    }
    let mut open = state.open;
    egui::Window::new(services.t("settings"))
        .open(&mut open)
        .default_size([900.0, 600.0])
        .min_size([700.0, 420.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    for (page, key) in Page::all() {
                        if ui
                            .selectable_label(state.page == page, services.t(key))
                            .clicked()
                        {
                            state.page = page;
                        }
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(600.0);
                    egui::ScrollArea::vertical().show(ui, |ui| match state.page {
                        Page::General => general(ui, services, &mut effects),
                        Page::Updates => updates(ui, theme, services, &mut effects),
                        Page::Appearance => appearance(ui, services, &mut effects),
                        Page::Shortcuts => shortcuts(ui, theme, services, state, &mut effects),
                        Page::Environments => {
                            environments(ui, theme, services, state, &mut effects)
                        }
                        Page::Connections => connections(ui, theme, services, state, &mut effects),
                        Page::Security => security(ui, theme, services, &mut effects),
                        Page::Data => data(ui, theme, services),
                        Page::About => about(ui, theme, services),
                    });
                });
            });
        });
    state.open = open;
    effects
}

fn general(ui: &mut egui::Ui, s: &Services, effects: &mut Vec<SettingsEffect>) {
    let mut settings = s.settings.write();
    let mut changed = false;
    ui.heading(s.t("settings-general"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(s.t("settings-startup-workspace"));
        changed |= ui
            .text_edit_singleline(&mut settings.general.startup_workspace)
            .changed();
    });
    changed |= ui
        .checkbox(
            &mut settings.general.restore_tabs,
            s.t("settings-restore-tabs"),
        )
        .changed();
    ui.horizontal(|ui| {
        ui.label(s.t("settings-language"));
        let mut lang = settings.general.language.clone();
        let auto = s.t("settings-language-auto");
        egui::ComboBox::from_id_salt("lang")
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
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn updates(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    use hs_core::updater::UpdateStatus;
    let mut changed = false;
    ui.heading(s.t("settings-updates"));
    ui.add_space(8.0);
    egui::Grid::new("updates-info")
        .num_columns(2)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.label(s.t("settings-about-version"));
            ui.monospace(s.version.to_string());
            ui.end_row();
            ui.label(s.t("settings-data-folder"));
            ui.monospace(s.paths.root().display().to_string());
            ui.end_row();
            ui.label(s.t("settings-updates-exe-folder"));
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
    ui.add_space(10.0);
    {
        let mut settings = s.settings.write();
        ui.horizontal(|ui| {
            ui.label(s.t("settings-updates-channel"));
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
        changed |= ui
            .checkbox(&mut settings.updates.enabled, s.t("settings-updates-auto"))
            .changed();
        ui.colored_label(theme.text_muted, s.t("settings-updates-auto-hint"));
        ui.horizontal(|ui| {
            ui.label(s.t("settings-updates-when-found"));
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
    }
    ui.add_space(10.0);
    ui.separator();
    let status = s.updater.status();
    let last = s.settings.read().updates.last_check.clone();
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
                if ui
                    .button(
                        s.i18n
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
            let btn = egui::Button::new(
                egui::RichText::new(s.t("settings-updates-install")).color(egui::Color32::WHITE),
            )
            .fill(theme.accent);
            if ui.add(btn).clicked() {
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
    if let Some((_, prev)) = s.updater.previous_exe() {
        ui.add_space(10.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(s.t("settings-updates-previous"));
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
    }
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn appearance(ui: &mut egui::Ui, s: &Services, effects: &mut Vec<SettingsEffect>) {
    let mut settings = s.settings.write();
    let mut changed = false;
    let mut theme_changed = false;
    ui.heading(s.t("settings-appearance"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(s.t("settings-theme"));
        for (val, key) in [
            ("dark", "settings-theme-dark"),
            ("light", "settings-theme-light"),
        ] {
            if ui
                .radio(settings.appearance.theme == val, s.t(key))
                .clicked()
            {
                settings.appearance.theme = val.into();
                theme_changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(s.t("settings-min-tab-width"));
        changed |= ui
            .add(
                egui::Slider::new(&mut settings.appearance.min_tab_width, 60.0..=240.0)
                    .suffix(" px"),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(s.t("settings-tab-gap"));
        changed |= ui
            .add(egui::Slider::new(&mut settings.appearance.tab_gap, 0.0..=24.0).suffix(" px"))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(s.t("settings-ui-zoom"));
        if ui
            .add(egui::Slider::new(&mut settings.appearance.ui_zoom, 0.75..=2.0).step_by(0.05))
            .changed()
        {
            theme_changed = true;
        }
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
    ui.heading(s.t("settings-shortcuts"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(hs_ui::icons::SEARCH);
        ui.text_edit_singleline(&mut state.shortcut_filter);
        if ui.button(s.t("settings-shortcut-reset")).clicked() {
            let _ = std::fs::remove_file(s.paths.shortcuts_file());
            *s.shortcuts.write() = hs_core::shortcuts::Keymap::load(&s.paths.shortcuts_file());
            state.shortcut_edit.clear();
        }
    });
    let conflicts = s.shortcuts.read().conflicts();
    if !conflicts.is_empty() {
        ui.colored_label(
            theme.warning,
            format!(
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
    let bindings: Vec<(String, String)> = s
        .shortcuts
        .read()
        .all()
        .map(|(a, c)| (a.clone(), c.display()))
        .collect();
    egui::Grid::new("shortcuts")
        .num_columns(3)
        .striped(true)
        .show(ui, |ui| {
            ui.label(egui::RichText::new(s.t("settings-shortcut-action")).strong());
            ui.label(egui::RichText::new(s.t("settings-shortcut-binding")).strong());
            ui.label("");
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
                let r = ui.add(egui::TextEdit::singleline(draft).desired_width(160.0));
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
                ui.label("");
                ui.end_row();
            }
        });
}

fn environments(
    ui: &mut egui::Ui,
    theme: &Theme,
    s: &Services,
    state: &mut SettingsState,
    effects: &mut Vec<SettingsEffect>,
) {
    ui.heading(s.t("settings-environments"));
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(180.0);
            if ui
                .button(format!("{} {}", hs_ui::icons::TAB_NEW, s.t("env-new")))
                .clicked()
            {
                let e = Environment {
                    name: "new".into(),
                    ..Default::default()
                };
                state.env_selected = Some(e.id.clone());
                state.env_draft = Some((e, Vec::new()));
            }
            let envs: Vec<(String, String, Option<String>, bool)> = s
                .environments
                .read()
                .all()
                .iter()
                .map(|e| (e.id.clone(), e.name.clone(), e.colour.clone(), e.protected))
                .collect();
            for (id, name, colour, protected) in envs {
                let col = theme.env_colour(&name, colour.as_deref());
                let label =
                    egui::RichText::new(format!("{}{}", if protected { "🔒 " } else { "" }, name))
                        .color(col);
                if ui
                    .selectable_label(state.env_selected.as_deref() == Some(&id), label)
                    .clicked()
                {
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
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some((draft, vars)) = &mut state.env_draft else {
                return;
            };
            egui::Grid::new("env-form").num_columns(2).show(ui, |ui| {
                ui.label(s.t("env-name"));
                ui.text_edit_singleline(&mut draft.name);
                ui.end_row();
                ui.label(s.t("env-colour"));
                let mut col = draft
                    .colour
                    .as_deref()
                    .and_then(hs_ui::theme::parse_hex)
                    .unwrap_or(theme.env_colour(&draft.name, None));
                if ui.color_edit_button_srgba(&mut col).changed() {
                    draft.colour = Some(hs_ui::theme::to_hex(col));
                }
                ui.end_row();
                ui.label("");
                ui.checkbox(&mut draft.protected, s.t("env-protected-flag"))
                    .on_hover_text(s.t("env-protected-hint"));
                ui.end_row();
                ui.label(s.t("env-parent"));
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
            });
            ui.add_space(8.0);
            ui.label(egui::RichText::new(s.t("env-variables")).strong());
            KvEditor {
                theme,
                key_hint: "name",
                value_hint: "value",
                add_label: &s.t("env-add-variable"),
                allow_secret: true,
            }
            .show(ui, vars);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(s.t("save")).clicked() {
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
                if ui
                    .button(egui::RichText::new(s.t("delete")).color(theme.error))
                    .clicked()
                {
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
    ui.heading(s.t("settings-connections"));
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(200.0);
            if ui
                .button(format!("{} {}", hs_ui::icons::TAB_NEW, s.t("conn-new")))
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
            let conns: Vec<(String, String, String)> = s
                .connections
                .read()
                .all()
                .iter()
                .map(|c| (c.id.clone(), c.kind.clone(), c.name.clone()))
                .collect();
            if conns.is_empty() {
                ui.colored_label(theme.text_muted, s.t("conn-none"));
            }
            for (id, kind, name) in conns {
                if ui
                    .selectable_label(
                        state.conn_selected.as_deref() == Some(&id),
                        format!("{kind}  {name}"),
                    )
                    .clicked()
                {
                    state.conn_selected = Some(id.clone());
                    if let Some(c) = s.connections.read().by_id(&id) {
                        let fields = toml::to_string_pretty(&c.fields).unwrap_or_default();
                        state.conn_draft = Some((c.clone(), fields));
                    }
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let Some((draft, fields_text)) = &mut state.conn_draft else {
                return;
            };
            egui::Grid::new("conn-form").num_columns(2).show(ui, |ui| {
                ui.label(s.t("conn-name"));
                ui.text_edit_singleline(&mut draft.name);
                ui.end_row();
                ui.label(s.t("conn-kind"));
                egui::ComboBox::from_id_salt("conn-kind")
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
                ui.label(s.t("conn-folder"));
                ui.text_edit_singleline(&mut draft.folder);
                ui.end_row();
                ui.label(s.t("conn-tags"));
                let mut tags = draft.tags.join(", ");
                if ui.text_edit_singleline(&mut tags).changed() {
                    draft.tags = tags
                        .split(',')
                        .map(|t| t.trim().to_string())
                        .filter(|t| !t.is_empty())
                        .collect();
                }
                ui.end_row();
                ui.label(s.t("conn-environment"));
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
                ui.label("");
                ui.checkbox(&mut draft.read_only, s.t("conn-read-only"));
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.label(egui::RichText::new(s.t("conn-fields")).strong());
            ui.add(
                egui::TextEdit::multiline(fields_text)
                    .code_editor()
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(s.t("save")).clicked() {
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
                if ui
                    .button(egui::RichText::new(s.t("delete")).color(theme.error))
                    .clicked()
                {
                    state.confirm_delete = Some((draft.id.clone(), draft.name.clone(), false));
                }
            });
        });
    });
}

fn security(ui: &mut egui::Ui, theme: &Theme, s: &Services, effects: &mut Vec<SettingsEffect>) {
    ui.heading(s.t("settings-security"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(s.t("settings-vault-status"));
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
    let mut settings = s.settings.write();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(s.t("settings-vault-relock"));
        changed |= ui
            .add(egui::DragValue::new(&mut settings.security.relock_after_minutes).range(0..=600))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label(s.t("settings-clipboard-clear"));
        changed |= ui
            .add(
                egui::DragValue::new(&mut settings.security.clear_clipboard_after_seconds)
                    .range(0..=600),
            )
            .changed();
    });
    if changed {
        effects.push(SettingsEffect::SettingsChanged);
    }
}

fn data(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    ui.heading(s.t("settings-data"));
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(s.t("settings-data-folder"));
        ui.monospace(s.paths.root().display().to_string());
        if ui.button(s.t("settings-data-open-folder")).clicked() {
            open_folder(s.paths.root());
        }
    });
    ui.colored_label(theme.text_muted, format!("{:?}", s.paths.source));
    if let Some(r) = &s.paths.fallback_reason {
        ui.colored_label(theme.warning, r);
    }
    ui.add_space(8.0);
    ui.colored_label(theme.text_muted, s.t("settings-data-reset"));
}

fn about(ui: &mut egui::Ui, theme: &Theme, s: &Services) {
    ui.heading(s.t("app-name"));
    ui.label(s.t("app-tagline"));
    ui.add_space(8.0);
    egui::Grid::new("about").num_columns(2).show(ui, |ui| {
        ui.label(s.t("settings-about-version"));
        ui.monospace(s.version.to_string());
        ui.end_row();
        ui.label(s.t("settings-about-signing-key"));
        ui.vertical(|ui| {
            for k in hs_core::update::SIGNING_KEYS {
                ui.monospace(*k);
            }
        });
        ui.end_row();
        ui.label(s.t("settings-about-project"));
        ui.hyperlink("https://git.zacx.net/owner/hyper-stack");
        ui.end_row();
    });
    ui.add_space(8.0);
    ui.colored_label(theme.text_muted, s.t("settings-about-license"));
}

fn open_folder(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(path).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
