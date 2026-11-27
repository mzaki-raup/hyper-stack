//! The Extracts and Jobs page (docs/tools/analytics.md §4.8): the server's
//! extract refresh tasks with their schedule and failures, and its recent
//! background jobs with what they said. Read-only (owner, 2026-11-20): there
//! is no Refresh Now; Open in Browser leads to the server.

use std::sync::Arc;

use hs_core::tool::{OpenRequest, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use crate::provider::{BiProvider, ExtractTask, JobInfo, Result};

#[derive(Default)]
struct Inbox {
    tasks: Option<Result<Vec<ExtractTask>>>,
    jobs: Option<Result<Vec<JobInfo>>>,
}

enum Part<T> {
    Loading,
    Ready(T),
    Failed(String),
}

pub struct Extracts {
    tasks: Part<Vec<Vec<String>>>,
    jobs: Part<Vec<Vec<String>>>,
    task_table: TableState,
    job_table: TableState,
    inbox: Arc<Mutex<Inbox>>,
}

/// The result column of a job: still running, done, or failed.
fn result_text(job: &JobInfo) -> &'static str {
    match job.ok {
        None => "Running",
        Some(true) => "Succeeded",
        Some(false) => "Failed",
    }
}

fn task_row(t: &ExtractTask) -> Vec<String> {
    vec![
        t.target_kind.clone(),
        t.target.clone(),
        t.kind.clone(),
        t.schedule.clone(),
        t.frequency.clone(),
        t.failed.to_string(),
    ]
}

fn job_row(j: &JobInfo) -> Vec<String> {
    vec![
        j.kind.clone(),
        j.started.clone(),
        j.completed.clone(),
        format!("{}%", j.progress),
        result_text(j).to_string(),
        j.notes.clone(),
    ]
}

impl Extracts {
    pub fn open(s: &hs_core::Services, provider: &Arc<dyn BiProvider>) -> Extracts {
        let page = Extracts {
            tasks: Part::Loading,
            jobs: Part::Loading,
            task_table: TableState::default(),
            job_table: TableState::default(),
            inbox: Arc::new(Mutex::new(Inbox::default())),
        };
        let (p, inbox) = (provider.clone(), page.inbox.clone());
        s.runtime.spawn(async move {
            let r = p.extracts().await;
            inbox.lock().tasks = Some(r);
        });
        let (p, inbox) = (provider.clone(), page.inbox.clone());
        s.runtime.spawn(async move {
            let r = p.jobs().await;
            inbox.lock().jobs = Some(r);
        });
        page
    }

    fn drain(&mut self) {
        let (tasks, jobs) = {
            let mut g = self.inbox.lock();
            (g.tasks.take(), g.jobs.take())
        };
        if let Some(r) = tasks {
            self.tasks = match r {
                Ok(list) => Part::Ready(list.iter().map(task_row).collect()),
                Err(e) => Part::Failed(e),
            };
        }
        if let Some(r) = jobs {
            self.jobs = match r {
                Ok(list) => Part::Ready(list.iter().map(job_row).collect()),
                Err(e) => Part::Failed(e),
            };
        }
    }
}

/// One table of the page: its heading, what it holds, and what it says
/// when there is nothing.
struct Section<'a> {
    title: String,
    part: &'a Part<Vec<Vec<String>>>,
    columns: Vec<ColumnSpec>,
    empty: String,
    id: &'static str,
}

fn section(
    ui: &mut egui::Ui,
    s: &hs_core::Services,
    theme: &Theme,
    sec: &Section<'_>,
    state: &mut TableState,
) {
    form::subsection(ui, theme, &sec.title);
    ui.add_space(form::PANE_GAP);
    match sec.part {
        Part::Loading => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-loading"));
            });
        }
        Part::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
        Part::Ready(rows) => {
            ui.push_id(sec.id, |ui| {
                ui.allocate_ui(egui::vec2(ui.available_width(), 240.0), |ui| {
                    DataTable {
                        theme,
                        columns: &sec.columns,
                        rows,
                        row_height: hs_ui::table::ROW_HEIGHT,
                        show_filter: true,
                        copy_row_label: &s.t("table-copy-row"),
                        built_in_menu: true,
                        row_icons: None,
                        empty_hint: Some(&sec.empty),
                    }
                    .show(ui, state);
                });
            });
        }
    }
}

/// Draw the page, reading both lists when it is first shown.
pub fn page(
    slot: &mut Option<Extracts>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
) {
    if slot.is_none() {
        *slot = Some(Extracts::open(cx.services, provider));
    }
    let Some(x) = slot.as_mut() else {
        return;
    };
    x.drain();
    let s = cx.services;
    form::page_title(
        ui,
        theme,
        &s.t("analytics-kind-extracts"),
        Some(&s.t("analytics-extracts-sub")),
    );
    form::centred_row_wrapped(ui, |ui| {
        if form::button(ui, theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-refresh")).clicked() {
            *slot = Some(Extracts::open(s, provider));
        }
        if form::button(
            ui,
            theme,
            ph::ARROW_SQUARE_OUT,
            &s.t("analytics-open-browser"),
        )
        .clicked()
        {
            cx.open(
                "browser",
                Some(OpenRequest::Url {
                    url: provider.web_url(),
                }),
            );
        }
    });
    let Some(x) = slot.as_mut() else {
        return;
    };
    ui.add_space(form::ROW_GAP);
    egui::ScrollArea::vertical()
        .id_salt("analytics-extracts")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let tasks = Section {
                title: s.t("analytics-extracts-tasks"),
                part: &x.tasks,
                columns: vec![
                    ColumnSpec::new(s.t("analytics-col-target-kind"), 120.0),
                    ColumnSpec::new(s.t("analytics-col-target"), 220.0),
                    ColumnSpec::new(s.t("analytics-col-type"), 160.0),
                    ColumnSpec::new(s.t("analytics-col-schedule"), 140.0),
                    ColumnSpec::new(s.t("analytics-col-frequency"), 110.0),
                    ColumnSpec::new(s.t("analytics-col-failed"), 100.0).numeric(),
                ],
                empty: s.t("analytics-extracts-tasks-empty"),
                id: "analytics-extract-tasks",
            };
            section(ui, s, theme, &tasks, &mut x.task_table);
            ui.add_space(form::PANE_GAP);
            ui.separator();
            ui.add_space(form::PANE_GAP);
            let jobs = Section {
                title: s.t("analytics-extracts-jobs"),
                part: &x.jobs,
                columns: vec![
                    ColumnSpec::new(s.t("analytics-col-type"), 160.0),
                    ColumnSpec::new(s.t("analytics-col-started"), 180.0),
                    ColumnSpec::new(s.t("analytics-col-completed"), 180.0),
                    ColumnSpec::new(s.t("analytics-col-progress"), 100.0).numeric(),
                    ColumnSpec::new(s.t("analytics-col-result"), 110.0),
                    ColumnSpec::new(s.t("analytics-col-notes"), 260.0),
                ],
                empty: s.t("analytics-extracts-jobs-empty"),
                id: "analytics-extract-jobs",
            };
            section(ui, s, theme, &jobs, &mut x.job_table);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_task_and_a_job_become_rows_of_text() {
        let t = ExtractTask {
            kind: "FullRefresh".into(),
            target_kind: "Workbook".into(),
            target: "Regional Sales".into(),
            schedule: "Nightly".into(),
            frequency: "Daily".into(),
            failed: 2,
            ..Default::default()
        };
        assert_eq!(
            task_row(&t),
            [
                "Workbook",
                "Regional Sales",
                "FullRefresh",
                "Nightly",
                "Daily",
                "2"
            ]
        );
        let j = JobInfo {
            kind: "RefreshExtracts".into(),
            progress: 100,
            ok: Some(false),
            notes: "Could not connect".into(),
            ..Default::default()
        };
        let r = job_row(&j);
        assert_eq!((r[3].as_str(), r[4].as_str()), ("100%", "Failed"));
        assert_eq!(
            result_text(&JobInfo {
                ok: None,
                ..j.clone()
            }),
            "Running"
        );
        assert_eq!(
            result_text(&JobInfo {
                ok: Some(true),
                ..j
            }),
            "Succeeded"
        );
    }
}
