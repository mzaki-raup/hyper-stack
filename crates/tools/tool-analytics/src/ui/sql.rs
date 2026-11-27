//! The SQL page (docs/tools/analytics.md §4.6): ad-hoc SQL on a database the
//! server queries, run as the signed-in account through `POST /api/dataset`.
//! Nothing is saved on the server. The result is a table, or a quick chart
//! of it. A protected environment disables the page; a database the account
//! has no native rights on is refused with a sentence.

use std::sync::Arc;

use hs_core::jobs::JobId;
use hs_core::tool::ToolContext;
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::table::TableState;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::browse::Browse;
use super::editor::SqlEditor;
use super::question::{Done, chart_view, table_view};
use crate::provider::{BiProvider, Card, Database, Kind, Node, Result, Table};

/// What the result is drawn as; the chart kinds are a quick look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Table,
    Line,
    Bar,
    Pie,
    Number,
}

impl View {
    const ALL: [View; 5] = [View::Table, View::Line, View::Bar, View::Pie, View::Number];

    /// The server's chart word, which the chart plan reads.
    fn display(self) -> &'static str {
        match self {
            View::Table => "table",
            View::Line => "line",
            View::Bar => "bar",
            View::Pie => "pie",
            View::Number => "scalar",
        }
    }

    fn key(self) -> &'static str {
        match self {
            View::Table => "analytics-view-table",
            View::Line => "analytics-view-line",
            View::Bar => "analytics-view-bar",
            View::Pie => "analytics-view-pie",
            View::Number => "analytics-view-number",
        }
    }
}

/// The editor's height, about nine lines.
const EDITOR_HEIGHT: f32 = 200.0;

/// A run's answer with the number of the run it answers.
type Answer = Option<(u64, Result<Table>)>;

enum Run {
    Idle,
    Running(JobId),
    Failed(String),
}

pub struct Sql {
    db_id: String,
    /// The statement; the editor writes here whenever it changes.
    text: String,
    /// The coloured editor with completion, made when the page is first drawn.
    editor: Option<SqlEditor>,
    /// The starter the page filled in, so picking another table may replace
    /// it but never what the user typed.
    starter: String,
    run: Run,
    run_no: u64,
    done: Option<Done>,
    view: View,
    table: TableState,
    inbox: Arc<Mutex<Answer>>,
}

/// A server error without the `-- Metabase::` comment line it echoes back
/// (user id and query hash, which tell the reader nothing).
fn tidy(error: &str) -> String {
    error
        .lines()
        .filter(|l| !l.trim_start().starts_with("-- Metabase::"))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
        .trim()
        .to_string()
}

/// `SELECT * FROM "schema"."table" LIMIT 100`, quoted the way the engine does.
pub fn starter(engine: &str, schema: &str, table: &str) -> String {
    let q = if matches!(
        engine,
        "mysql" | "mariadb" | "bigquery-cloud-sdk" | "sparksql" | "databricks" | "druid"
    ) {
        '`'
    } else {
        '"'
    };
    let name = if schema.is_empty() {
        format!("{q}{table}{q}")
    } else {
        format!("{q}{schema}{q}.{q}{table}{q}")
    };
    format!("SELECT * FROM {name} LIMIT 100")
}

/// The database a picked node belongs to: a database is its own, a table's
/// id is `<database>/<table>`.
pub fn database_of(node: &Node) -> Option<&str> {
    match node.kind {
        Kind::Database => Some(&node.id),
        Kind::Table => node.id.split_once('/').map(|(db, _)| db),
        _ => None,
    }
}

/// A stand-in for a saved question, so the chart plan can read the result.
fn card_for(view: View) -> Card {
    Card {
        id: "sql".into(),
        kind: Kind::Question,
        name: "SQL".into(),
        description: String::new(),
        display: view.display().into(),
        native_sql: None,
        database: String::new(),
        params: Vec::new(),
        viz: serde_json::Value::Null,
    }
}

impl Sql {
    fn new(db_id: &str) -> Sql {
        Sql {
            db_id: db_id.to_string(),
            text: String::new(),
            editor: None,
            starter: String::new(),
            run: Run::Idle,
            run_no: 0,
            done: None,
            view: View::Table,
            table: TableState::default(),
            inbox: Arc::new(Mutex::new(None)),
        }
    }

    /// Run the statement as a Job that can be cancelled.
    pub fn run(&mut self, s: &hs_core::Services, provider: &Arc<dyn BiProvider>, db_name: &str) {
        if matches!(self.run, Run::Running(_)) {
            return;
        }
        self.run_no += 1;
        let no = self.run_no;
        let limit = s.settings.read().analytics.rows_per_run as usize;
        let (p, db, sql, inbox) = (
            provider.clone(),
            self.db_id.clone(),
            self.text.clone(),
            self.inbox.clone(),
        );
        let handle = s.jobs.spawn(
            format!("SQL on {db_name}"),
            "analytics",
            move |h| async move {
                let token = h.cancel_token();
                tokio::select! {
                    _ = token.cancelled() => Err("Cancelled".to_string()),
                    r = p.run_sql(&db, &sql, limit) => {
                        let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                        *inbox.lock() = Some((no, r));
                        outcome
                    }
                }
            },
        );
        self.run = Run::Running(handle.id());
    }

    fn cancel(&mut self, s: &hs_core::Services) {
        if let Run::Running(id) = self.run {
            s.jobs.cancel(id);
        }
        self.run_no += 1;
        self.run = Run::Idle;
    }

    fn drain(&mut self) {
        let Some((no, r)) = self.inbox.lock().take() else {
            return;
        };
        if no != self.run_no {
            return;
        }
        match r {
            Ok(table) => {
                self.done = Some(Done::new(&card_for(self.view), table));
                self.table = TableState::default();
                self.run = Run::Idle;
            }
            // A failed run clears the result: the old rows are not an answer.
            Err(e) => {
                self.done = None;
                self.run = Run::Failed(tidy(&e));
            }
        }
    }

    /// The chart kind changed: plan the same result again.
    fn replan(&mut self) {
        if let Some(d) = self.done.take() {
            self.done = Some(Done::new(&card_for(self.view), d.table));
        }
    }
}

/// Run the open SQL page, if one is shown (the palette's and `Ctrl+Enter`'s Run).
pub fn run_open(
    slot: &mut Option<Sql>,
    s: &hs_core::Services,
    provider: &Arc<dyn BiProvider>,
    db_name: &str,
) {
    if let Some(q) = slot.as_mut() {
        q.run(s, provider, db_name);
    }
}

/// What the page is shown for: the picked node, its database, the server
/// to run on, and the protected environment the connection is in, if any.
pub struct Target<'a> {
    pub node: &'a Node,
    pub db: &'a Database,
    pub provider: &'a Arc<dyn BiProvider>,
    pub protected: Option<&'a str>,
}

/// Draw the page for the picked database or table.
pub fn page(
    slot: &mut Option<Sql>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    target: &Target<'_>,
    browse: &mut Browse,
) {
    let Target {
        node,
        db,
        provider,
        protected,
    } = *target;
    let s = cx.services;
    if !slot.as_ref().is_some_and(|q| q.db_id == db.id) {
        *slot = Some(Sql::new(&db.id));
    }
    let Some(q) = slot.as_mut() else {
        return;
    };
    if q.editor.is_none() {
        q.editor = Some(SqlEditor::new(s));
    }
    // Picking a table fills in a starter query, unless the user has typed.
    if node.kind == Kind::Table {
        let next = starter(&db.engine, &node.collection, &node.name);
        if q.text.trim().is_empty() || q.text == q.starter {
            q.text = next.clone();
            q.starter = next;
            if let Some(e) = q.editor.as_mut() {
                e.set_text(&q.text);
            }
        }
    }
    q.drain();
    form::page_title(
        ui,
        theme,
        &db.name,
        Some(&format!(
            "{} · {}",
            s.t("analytics-kind-database"),
            db.engine
        )),
    );
    if let Some(env) = protected {
        form::note(
            ui,
            theme,
            ph::SHIELD_CHECK,
            &s.i18n.t_args("analytics-sql-protected", &[("env", env)]),
        );
        return;
    }
    if !db.native {
        form::note(ui, theme, ph::LOCK, &s.t("analytics-sql-no-native"));
        return;
    }
    form::hint(
        ui,
        theme,
        &s.i18n.t_args("analytics-sql-notice", &[("db", &db.name)]),
    );
    ui.add_space(form::PANE_GAP);
    if let Some(e) = q.editor.as_mut()
        && e.show(ui, theme, EDITOR_HEIGHT, &mut || {
            browse.tables(s, provider, &db.id)
        })
    {
        q.text = e.text();
    }
    ui.add_space(form::PANE_GAP);
    form::centred_row_wrapped(ui, |ui| {
        if matches!(q.run, Run::Running(_)) {
            if form::button(ui, theme, ph::X, &s.t("analytics-cancel")).clicked() {
                q.cancel(s);
            }
            ui.spinner();
        } else if form::primary_button(ui, theme, &format!("{} {}", ph::PLAY, s.t("analytics-run")))
            .on_hover_text(s.t("analytics-sql-run-tip"))
            .clicked()
        {
            q.run(s, provider, &db.name);
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-sql-copy")).clicked() {
            ui.ctx().copy_text(q.text.clone());
        }
        super::open_db::button(ui, cx, theme, Some(db), &q.text);
        ui.label(s.t("analytics-sql-view"));
        let before = q.view;
        egui::ComboBox::from_id_salt("analytics-sql-view")
            .selected_text(s.t(q.view.key()))
            .show_ui(ui, |ui| {
                for v in View::ALL {
                    form::selectable_value(ui, &mut q.view, v, s.t(v.key()));
                }
            });
        if q.view != before {
            q.replan();
        }
    });
    ui.add_space(form::ROW_GAP);
    match &q.run {
        Run::Running(_) if q.done.is_none() => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-running"));
            });
            return;
        }
        Run::Failed(e) => {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
            ui.add_space(form::ROW_GAP);
        }
        _ => {}
    }
    let Some(done) = &q.done else {
        form::hint(ui, theme, &s.t("analytics-sql-not-run"));
        return;
    };
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
    if q.view == View::Table {
        table_view(ui, cx, theme, done, &mut q.table);
    } else {
        chart_view(ui, cx, theme, done);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: Kind, id: &str) -> Node {
        Node {
            kind,
            id: id.into(),
            name: "ORDERS".into(),
            description: String::new(),
            collection: "PUBLIC".into(),
            edited: String::new(),
            editor: String::new(),
        }
    }

    #[test]
    fn a_starter_quotes_names_the_way_the_engine_does() {
        assert_eq!(
            starter("postgres", "public", "orders"),
            r#"SELECT * FROM "public"."orders" LIMIT 100"#
        );
        assert_eq!(
            starter("mysql", "shop", "orders"),
            "SELECT * FROM `shop`.`orders` LIMIT 100"
        );
        assert_eq!(
            starter("h2", "", "ORDERS"),
            r#"SELECT * FROM "ORDERS" LIMIT 100"#,
            "no schema, no dot"
        );
    }

    #[test]
    fn an_error_loses_the_comment_metabase_echoes_back() {
        let e = "Table \"X\" not found; SQL statement:
-- Metabase:: userID: 2 queryType: native queryHash: ab12
SELECT * FROM X [42102-214]";
        assert_eq!(
            tidy(e),
            "Table \"X\" not found; SQL statement:
SELECT * FROM X [42102-214]"
        );
        assert_eq!(tidy("plain"), "plain");
    }

    #[test]
    fn a_failed_run_clears_the_old_result() {
        let mut q = Sql::new("1");
        q.run_no = 1;
        *q.inbox.lock() = Some((1, Ok(Table::default())));
        q.drain();
        assert!(q.done.is_some());
        q.run_no = 2;
        *q.inbox.lock() = Some((2, Err("boom".into())));
        q.drain();
        assert!(q.done.is_none(), "the failed run leaves no stale rows");
        assert!(matches!(q.run, Run::Failed(ref e) if e == "boom"));
    }

    #[test]
    fn a_picked_node_belongs_to_its_database() {
        assert_eq!(database_of(&node(Kind::Database, "3")), Some("3"));
        assert_eq!(database_of(&node(Kind::Table, "3/17")), Some("3"));
        assert_eq!(database_of(&node(Kind::Question, "3")), None);
    }

    #[test]
    fn the_chart_kinds_map_to_the_servers_words() {
        assert_eq!(View::Number.display(), "scalar");
        assert_eq!(View::Table.display(), "table");
        assert_eq!(card_for(View::Pie).display, "pie");
    }

    #[test]
    fn an_answer_to_a_cancelled_run_is_dropped() {
        let mut q = Sql::new("1");
        q.run_no = 2;
        *q.inbox.lock() = Some((1, Ok(Table::default())));
        q.drain();
        assert!(q.done.is_none(), "the answer to run 1 is stale");
        *q.inbox.lock() = Some((2, Ok(Table::default())));
        q.drain();
        assert!(q.done.is_some());
    }
}
