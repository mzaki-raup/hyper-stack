//! A table's row preview (docs/tools/analytics.md §4.9): the first rows of a
//! CAS table in a `DataTable`, read as a job that can be cancelled. Rows per
//! Run (Settings → Analytics) sets how many; a result says when it was cut.

use std::sync::Arc;

use hs_core::jobs::JobId;
use hs_core::tool::ToolContext;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::TableState;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::question::{Done, table_view};
use crate::provider::{BiProvider, Card, Kind, Node, Result, Table};

/// A run's answer, with the number of the run it answers.
type Inbox = Arc<Mutex<Option<(u64, Result<Table>)>>>;

enum Load {
    Running(JobId),
    Ready(Box<Done>),
    Failed(String),
}

pub struct Preview {
    node_id: String,
    /// Bumped by every run and cancel; an answer to an older one is dropped.
    run_no: u64,
    load: Load,
    table: TableState,
    inbox: Inbox,
}

/// `<database>/<table>`, as the browse tree names a table's node.
fn split(id: &str) -> (&str, &str) {
    id.split_once('/').unwrap_or((id, id))
}

/// A stand-in card, so the rows are made ready the way a question's are.
fn card_for(node: &Node) -> Card {
    Card {
        id: node.id.clone(),
        kind: Kind::Question,
        name: node.name.clone(),
        description: String::new(),
        display: "table".into(),
        native_sql: None,
        database: String::new(),
        params: Vec::new(),
        viz: serde_json::Value::Null,
    }
}

impl Preview {
    fn start(
        s: &hs_core::Services,
        provider: &Arc<dyn BiProvider>,
        node: &Node,
        run_no: u64,
        inbox: Inbox,
    ) -> Load {
        let (db, table) = split(&node.id);
        let (db, table) = (db.to_string(), table.to_string());
        let limit = s.settings.read().analytics.rows_per_run as usize;
        let p = provider.clone();
        let handle = s.jobs.spawn(
            format!("Rows of {}", node.name),
            "analytics",
            move |h| async move {
                let token = h.cancel_token();
                tokio::select! {
                    _ = token.cancelled() => Err("Cancelled".to_string()),
                    r = p.preview(&db, &table, limit) => {
                        let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                        *inbox.lock() = Some((run_no, r));
                        outcome
                    }
                }
            },
        );
        Load::Running(handle.id())
    }

    fn drain(&mut self, node: &Node) {
        let Some((no, r)) = self.inbox.lock().take() else {
            return;
        };
        if no != self.run_no {
            return;
        }
        self.table = TableState::default();
        self.load = match r {
            Ok(t) => Load::Ready(Box::new(Done::new(&card_for(node), t))),
            Err(e) => Load::Failed(e),
        };
    }
}

/// Draw the page for the table `node`, reading it when it is first shown.
pub fn page(
    slot: &mut Option<Preview>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    node: &Node,
) {
    let s = cx.services;
    if !slot.as_ref().is_some_and(|p| p.node_id == node.id) {
        let inbox = Arc::new(Mutex::new(None));
        *slot = Some(Preview {
            node_id: node.id.clone(),
            run_no: 1,
            load: Load::Failed(String::new()),
            table: TableState::default(),
            inbox: inbox.clone(),
        });
        let load = Preview::start(s, provider, node, 1, inbox);
        if let Some(p) = slot.as_mut() {
            p.load = load;
        }
    }
    let Some(p) = slot.as_mut() else {
        return;
    };
    p.drain(node);
    let sub = if node.description.is_empty() {
        s.t("analytics-kind-table")
    } else {
        format!("{} · {}", s.t("analytics-kind-table"), node.description)
    };
    form::page_title(ui, theme, &node.name, Some(&sub));
    form::hint(ui, theme, &s.t("analytics-preview-sub"));
    ui.add_space(form::PANE_GAP);
    form::centred_row_wrapped(ui, |ui| {
        if let Load::Running(id) = p.load {
            if form::button(ui, theme, ph::X, &s.t("analytics-cancel")).clicked() {
                s.jobs.cancel(id);
                p.run_no += 1;
                p.load = Load::Failed(s.t("analytics-cancelled"));
            }
            ui.spinner();
        } else if form::button(ui, theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-refresh")).clicked()
        {
            p.run_no += 1;
            p.load = Preview::start(s, provider, node, p.run_no, p.inbox.clone());
        }
    });
    ui.add_space(form::ROW_GAP);
    match &p.load {
        Load::Running(_) => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-running"));
            });
        }
        Load::Failed(e) if e.is_empty() => {}
        Load::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
        Load::Ready(done) => {
            let status = s.i18n.t_args(
                "analytics-result-status",
                &[
                    ("rows", &done.table.rows.len().to_string()),
                    ("ms", &done.table.millis.to_string()),
                ],
            );
            ui.label(egui::RichText::new(status).color(theme.text_muted));
            if done.table.truncated {
                ui.label(
                    egui::RichText::new(s.i18n.t_args(
                        "analytics-truncated",
                        &[("rows", &done.table.rows.len().to_string())],
                    ))
                    .color(theme.warning),
                );
            }
            ui.add_space(form::PANE_GAP);
            table_view(ui, cx, theme, done, &mut p.table);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tables_node_id_names_its_library_and_table() {
        assert_eq!(split("Public/ORDERS"), ("Public", "ORDERS"));
        assert_eq!(split("ORDERS"), ("ORDERS", "ORDERS"));
    }
}
