//! Bottom dock: Logs, Jobs and Problems.
//!
//! It had a terminal too, until the owner asked for it to go on
//! 2026-10-18: shells belong to the Terminal activity, and since the
//! editor grew one of its own in the project's folder a third place to
//! look was one too many. The Output tab became **Logs** on 2026-10-22
//! (owner's request): the app's own log with a level filter and colours,
//! where every error and warning a tool raises also lands.

use hs_core::Services;
use hs_core::jobs::{JobStatus, Progress};
use hs_core::logging::{LogBuffer, line_level};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockTab {
    #[default]
    Logs,
    Jobs,
    Problems,
}

pub struct DockState {
    pub tab: DockTab,
    /// The lowest level shown; starts from Settings → General → Behaviour.
    pub min_level: tracing::Level,
    pub level_synced: bool,
    pub search: String,
    /// How many buffer lines had been seen when the Logs tab was last
    /// shown, so errors since then count on the tab label.
    pub seen_lines: usize,
}

impl Default for DockState {
    fn default() -> Self {
        Self {
            tab: DockTab::Logs,
            min_level: tracing::Level::INFO,
            level_synced: false,
            search: String::new(),
            seen_lines: 0,
        }
    }
}

/// The levels a user picks from, highest first.
pub const LEVELS: [(tracing::Level, &str); 5] = [
    (tracing::Level::ERROR, "error"),
    (tracing::Level::WARN, "warn"),
    (tracing::Level::INFO, "info"),
    (tracing::Level::DEBUG, "debug"),
    (tracing::Level::TRACE, "trace"),
];

pub fn level_from_name(name: &str) -> tracing::Level {
    LEVELS
        .iter()
        .find(|(_, n)| n.eq_ignore_ascii_case(name.trim()))
        .map(|(l, _)| *l)
        .unwrap_or(tracing::Level::INFO)
}

pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    logs: &LogBuffer,
    state: &mut DockState,
) {
    if !state.level_synced {
        state.min_level = level_from_name(&services.settings.read().general.log_level);
        state.level_synced = true;
    }
    let lines = logs.snapshot();
    let unseen_errors = lines
        .iter()
        .skip(state.seen_lines.min(lines.len()))
        .filter(|l| line_level(l) == Some(tracing::Level::ERROR))
        .count();
    ui.horizontal(|ui| {
        let running = services.jobs.running_count();
        for (tab, key) in [
            (DockTab::Logs, "dock-logs"),
            (DockTab::Jobs, "dock-jobs"),
            (DockTab::Problems, "dock-problems"),
        ] {
            let label = match tab {
                DockTab::Jobs if running > 0 => format!("{} ({running})", services.t(key)),
                DockTab::Logs if unseen_errors > 0 && state.tab != DockTab::Logs => {
                    format!("{} ({unseen_errors})", services.t(key))
                }
                _ => services.t(key),
            };
            let text = if tab == DockTab::Logs && unseen_errors > 0 && state.tab != DockTab::Logs {
                egui::RichText::new(label).color(theme.error)
            } else {
                egui::RichText::new(label)
            };
            if ui.selectable_label(state.tab == tab, text).clicked() {
                state.tab = tab;
            }
        }
        if state.tab == DockTab::Logs {
            ui.separator();
            ui.label(services.t("dock-logs-level"));
            egui::ComboBox::from_id_salt("dock-log-level")
                .selected_text(services.t(&format!("log-level-{}", level_name(state.min_level))))
                .show_ui(ui, |ui| {
                    for (l, n) in LEVELS {
                        ui.selectable_value(
                            &mut state.min_level,
                            l,
                            services.t(&format!("log-level-{n}")),
                        );
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut state.search)
                    .hint_text(services.t("dock-logs-search"))
                    .desired_width(180.0),
            );
            if ui
                .small_button(services.t("dock-logs-copy"))
                .tip(services.t("dock-logs-copy-tip"))
                .clicked()
            {
                ui.ctx().copy_text(lines.join("\n"));
            }
            if ui.small_button(services.t("dock-logs-clear")).clicked() {
                logs.clear();
                state.seen_lines = 0;
            }
            form_hint(ui, theme, &services.t("dock-logs-hint"));
        }
    });
    ui.separator();
    match state.tab {
        DockTab::Logs => {
            state.seen_lines = lines.len();
            let needle = state.search.to_lowercase();
            let shown: Vec<&String> = lines
                .iter()
                .filter(|l| {
                    // A continuation line (no level of its own) follows the
                    // record before it; it is shown whenever that one is,
                    // which the filter approximates by letting it through.
                    line_level(l).is_none_or(|lv| lv <= state.min_level)
                })
                .filter(|l| needle.is_empty() || l.to_lowercase().contains(&needle))
                .collect();
            let shown: Vec<&String> = shown.into_iter().rev().take(2000).rev().collect();
            if shown.is_empty() {
                ui.colored_label(theme.text_muted, services.t("dock-logs-empty"));
            }
            egui::ScrollArea::both()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for line in shown {
                        let colour = match line_level(line) {
                            Some(tracing::Level::ERROR) => theme.error,
                            Some(tracing::Level::WARN) => theme.warning,
                            Some(tracing::Level::INFO) => theme.text,
                            Some(_) => theme.text_muted,
                            None => theme.text_muted,
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(line)
                                    .monospace()
                                    .size(12.0)
                                    .color(colour),
                            )
                            .wrap(),
                        );
                    }
                });
        }
        DockTab::Jobs => {
            let jobs = services.jobs.snapshot();
            if jobs.is_empty() {
                ui.colored_label(theme.text_muted, services.t("jobs-none"));
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for j in jobs.iter().rev() {
                    ui.horizontal(|ui| {
                        let (icon, col) = match j.status {
                            JobStatus::Running => (hs_ui::icons::JOBS, theme.secondary),
                            JobStatus::Succeeded => (hs_ui::icons::CHECK, theme.success),
                            JobStatus::Failed => (hs_ui::icons::ERROR, theme.error),
                            JobStatus::Cancelled => (hs_ui::icons::WARNING, theme.warning),
                        };
                        ui.colored_label(col, icon);
                        ui.label(&j.title);
                        ui.colored_label(theme.text_muted, &j.tool);
                        match &j.progress {
                            Progress::Fraction(f) if j.status == JobStatus::Running => {
                                ui.add(egui::ProgressBar::new(*f).desired_width(160.0));
                            }
                            Progress::Bytes { done, total } if j.status == JobStatus::Running => {
                                let f =
                                    total.map(|t| *done as f32 / t.max(1) as f32).unwrap_or(0.0);
                                ui.add(
                                    egui::ProgressBar::new(f)
                                        .desired_width(160.0)
                                        .text(format!("{done} B")),
                                );
                            }
                            _ => {}
                        }
                        if let Some(m) = &j.message {
                            ui.colored_label(theme.text_muted, m);
                        }
                        if j.status == JobStatus::Running
                            && ui.small_button(services.t("jobs-cancel")).clicked()
                        {
                            services.jobs.cancel(j.id);
                        }
                    });
                }
            });
        }
        DockTab::Problems => {
            ui.colored_label(theme.text_muted, "—");
        }
    }
}

fn level_name(l: tracing::Level) -> &'static str {
    LEVELS
        .iter()
        .find(|(x, _)| *x == l)
        .map(|(_, n)| *n)
        .unwrap_or("info")
}

fn form_hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(egui::RichText::new(text).small().color(theme.text_muted));
}
