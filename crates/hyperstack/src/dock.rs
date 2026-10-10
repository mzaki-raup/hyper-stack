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
    pub terminal: Option<(hs_term::session::Session, hs_term::widget::TerminalView)>,
    pub terminal_error: Option<String>,
}

impl DockState {
    fn ensure_terminal(&mut self, services: &Services) {
        if self.terminal.is_some() || self.terminal_error.is_some() {
            return;
        }
        let profiles = hs_term::shells::detect();
        let (default, font, scrollback, copy) = {
            let st = services.settings.read();
            (
                st.terminal.default_profile.clone(),
                st.terminal.font_size,
                st.terminal.scrollback,
                st.terminal.copy_on_select,
            )
        };
        let Some(profile) = profiles
            .iter()
            .find(|p| p.id == default)
            .or(profiles.first())
            .cloned()
        else {
            self.terminal_error = Some(services.t("terminal-no-shell"));
            return;
        };
        let waker = services.runtime.waker();
        let waker: std::sync::Arc<dyn Fn() + Send + Sync> =
            std::sync::Arc::new(move || waker.request_repaint());
        let cwd = services.workspace.read().root().to_path_buf();
        match hs_term::session::Session::spawn(profile, Some(&cwd), 100, 12, scrollback, waker) {
            Ok(session) => {
                let mut view =
                    hs_term::widget::TerminalView::new(egui::Id::new("hs-dock-terminal"), font);
                view.copy_on_select = copy;
                self.terminal = Some((session, view));
            }
            Err(e) => self.terminal_error = Some(e),
        }
    }
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
            state.ensure_terminal(services);
            let dark = services.settings.read().appearance.theme != "light";
            let tt = if dark {
                hs_term::widget::TermTheme::dark()
            } else {
                hs_term::widget::TermTheme::light()
            };
            if let Some(err) = state.terminal_error.clone() {
                ui.colored_label(theme.error, err);
                if ui.button(services.t("terminal-restart")).clicked() {
                    state.terminal_error = None;
                    state.terminal = None;
                }
            } else if let Some((session, view)) = state.terminal.as_mut() {
                let _ = session.drain_events();
                if let Some(code) = session.exited() {
                    ui.horizontal(|ui| {
                        ui.label(
                            services
                                .i18n
                                .t_args("terminal-exited", &[("code", &code.to_string())]),
                        );
                        if ui.button(services.t("terminal-restart")).clicked() {
                            state.terminal = None;
                        }
                    });
                } else {
                    view.show(ui, session, &tt);
                }
            }
        }
    }
}
