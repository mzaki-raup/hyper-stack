//! Bottom dock: Output, Jobs, Problems, Terminal (placeholder until M1).

use hs_core::Services;
use hs_core::jobs::{JobStatus, Progress};
use hs_core::logging::LogBuffer;
use hs_ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DockTab {
    #[default]
    Output,
    Jobs,
    Problems,
    Terminal,
}

#[derive(Default)]
pub struct DockState {
    pub tab: DockTab,
}

pub fn show(
    ui: &mut egui::Ui,
    theme: &Theme,
    services: &Services,
    logs: &LogBuffer,
    state: &mut DockState,
) {
    ui.horizontal(|ui| {
        let running = services.jobs.running_count();
        for (tab, key) in [
            (DockTab::Output, "dock-output"),
            (DockTab::Jobs, "dock-jobs"),
            (DockTab::Problems, "dock-problems"),
            (DockTab::Terminal, "dock-terminal"),
        ] {
            let label = if tab == DockTab::Jobs && running > 0 {
                format!("{} ({running})", services.t(key))
            } else {
                services.t(key)
            };
            if ui.selectable_label(state.tab == tab, label).clicked() {
                state.tab = tab;
            }
        }
    });
    ui.separator();
    match state.tab {
        DockTab::Output => {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in logs.snapshot().iter().rev().take(500).rev() {
                        ui.add(
                            egui::Label::new(egui::RichText::new(line).monospace().size(12.0))
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
        DockTab::Terminal => {
            ui.colored_label(
                theme.text_muted,
                "The terminal arrives in milestone M1 (see docs/07-roadmap.md).",
            );
        }
    }
}
