//! A question, model or metric's page (docs/tools/analytics.md §4.3, §4.4):
//! its parameters, Run as a Job with cancel, and the result as a chart, a
//! table or the SQL behind it. Every read runs on the runtime and lands in
//! an inbox drained at frame start; the result is formatted and planned once
//! and drawn from that every frame.

use std::sync::Arc;

use egui_plot::{Bar, BarChart, Legend, Line, Plot, Points};
use hs_core::jobs::JobId;
use hs_core::tool::{OpenRequest, ToastLevel, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::palette8;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;
use hs_ui::tip::TipExt;
use parking_lot::Mutex;

use crate::chart::{self, Plan, Reason, Series, SeriesKind, SeriesPlan};
use crate::provider::{
    BiProvider, Card, ExportFormat, Kind, Node, Param, ParamKind, Result, Table,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Chart,
    Table,
    Sql,
}

enum Loaded<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// Where a run stands.
enum Run {
    Idle,
    Running(JobId),
    Failed(String),
}

/// A result made ready to draw: formatted once, planned once.
struct Done {
    table: Table,
    rows: Vec<Vec<String>>,
    specs: Vec<ColumnSpec>,
    plan: Plan,
}

impl Done {
    fn new(card: &Card, table: Table) -> Done {
        let rows = table
            .rows
            .iter()
            .map(|r| {
                table
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        r.get(i)
                            .map(|v| chart::cell_text(c.kind, v))
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .collect();
        let specs = table
            .columns
            .iter()
            .map(|c| {
                let spec = ColumnSpec::new(
                    c.title.clone(),
                    if c.kind.is_number() { 120.0 } else { 180.0 },
                );
                if c.kind.is_number() {
                    spec.numeric()
                } else {
                    spec
                }
            })
            .collect();
        let plan = chart::plan(card, &table);
        Done {
            table,
            rows,
            specs,
            plan,
        }
    }
}

#[derive(Default)]
struct Inbox {
    card: Option<Result<Card>>,
    /// A run's answer with the number of the run it answers.
    run: Option<(u64, Result<Table>)>,
    export: Option<(ExportFormat, Result<Vec<u8>>)>,
}

pub struct Question {
    node: Node,
    card: Loaded<Card>,
    /// Each parameter's text, by id; the card's defaults to begin with.
    values: Vec<(String, String)>,
    view: View,
    run: Run,
    run_no: u64,
    done: Option<Done>,
    table: TableState,
    inbox: Arc<Mutex<Inbox>>,
    exporting: bool,
}

impl Question {
    fn open(s: &hs_core::Services, provider: &Arc<dyn BiProvider>, node: &Node) -> Question {
        let inbox = Arc::new(Mutex::new(Inbox::default()));
        let (p, ib, id) = (provider.clone(), inbox.clone(), node.id.clone());
        s.runtime.spawn(async move {
            let r = p.card(&id).await;
            ib.lock().card = Some(r);
        });
        Question {
            node: node.clone(),
            card: Loaded::Loading,
            values: Vec::new(),
            view: View::Chart,
            run: Run::Idle,
            run_no: 0,
            done: None,
            table: TableState::default(),
            inbox,
            exporting: false,
        }
    }

    /// Run with the parameters as typed, as a Job that can be cancelled.
    pub fn run(&mut self, s: &hs_core::Services, provider: &Arc<dyn BiProvider>) {
        let Loaded::Ready(card) = &self.card else {
            return;
        };
        if matches!(self.run, Run::Running(_)) {
            return;
        }
        self.run_no += 1;
        let no = self.run_no;
        let limit = s.settings.read().analytics.rows_per_run as usize;
        let (p, card, values, inbox) = (
            provider.clone(),
            card.clone(),
            self.values.clone(),
            self.inbox.clone(),
        );
        let title = format!("Run {}", card.name);
        let handle = s.jobs.spawn(title, "analytics", move |h| async move {
            let token = h.cancel_token();
            tokio::select! {
                _ = token.cancelled() => Err("Cancelled".to_string()),
                r = p.run(&card, &values, limit) => {
                    let outcome = r.as_ref().map(|_| ()).map_err(Clone::clone);
                    inbox.lock().run = Some((no, r));
                    outcome
                }
            }
        });
        self.run = Run::Running(handle.id());
    }

    fn cancel(&mut self, s: &hs_core::Services) {
        if let Run::Running(id) = self.run {
            s.jobs.cancel(id);
        }
        // A late answer to the cancelled run is dropped.
        self.run_no += 1;
        self.run = Run::Idle;
    }

    fn export(&mut self, s: &hs_core::Services, provider: &Arc<dyn BiProvider>, f: ExportFormat) {
        let Loaded::Ready(card) = &self.card else {
            return;
        };
        self.exporting = true;
        let (p, card, values, inbox) = (
            provider.clone(),
            card.clone(),
            self.values.clone(),
            self.inbox.clone(),
        );
        s.runtime.spawn(async move {
            let r = p.export(&card, &values, f).await;
            inbox.lock().export = Some((f, r));
        });
    }

    /// Apply what the runtime delivered since the last frame.
    fn drain(&mut self, cx: &mut ToolContext, provider: &Arc<dyn BiProvider>) {
        let (card, run, export) = {
            let mut g = self.inbox.lock();
            (g.card.take(), g.run.take(), g.export.take())
        };
        if let Some(r) = card {
            match r {
                Ok(card) => {
                    self.values = card
                        .params
                        .iter()
                        .map(|p| (p.id.clone(), p.default.clone()))
                        .collect();
                    // Open on the card's own display; a table card opens on the table.
                    self.view = if card.display == "table" {
                        View::Table
                    } else {
                        View::Chart
                    };
                    self.card = Loaded::Ready(card);
                    // Metabase runs a question when it is opened.
                    self.run(cx.services, provider);
                }
                Err(e) => self.card = Loaded::Failed(e),
            }
        }
        if let Some((no, r)) = run
            && no == self.run_no
        {
            match (r, &self.card) {
                (Ok(table), Loaded::Ready(card)) => {
                    let done = Done::new(card, table);
                    // A card that cannot be drawn shows its table.
                    if self.view == View::Chart && !done.plan.is_chart() {
                        self.view = View::Table;
                    }
                    self.done = Some(done);
                    self.table = TableState::default();
                    self.run = Run::Idle;
                }
                (Err(e), _) => self.run = Run::Failed(e),
                _ => {}
            }
        }
        if let Some((f, r)) = export {
            self.exporting = false;
            let name = format!("{}.{}", file_stem(&self.node.name), f.ext());
            match r {
                Ok(bytes) => match hs_core::download::save_bytes(&name, &bytes) {
                    Ok(Some(path)) => cx.toast(
                        ToastLevel::Info,
                        cx.services.i18n.t_args(
                            "analytics-exported",
                            &[("path", &path.display().to_string())],
                        ),
                    ),
                    Ok(None) => {}
                    Err(e) => cx.toast(ToastLevel::Error, e),
                },
                Err(e) => cx.toast(ToastLevel::Error, e),
            }
        }
    }

    /// The SQL behind the result, and whether Metabase wrote it.
    fn sql(&self) -> Option<(String, bool)> {
        let Loaded::Ready(card) = &self.card else {
            return None;
        };
        match (
            &card.native_sql,
            self.done.as_ref().and_then(|d| d.table.sql.clone()),
        ) {
            (Some(own), _) => Some((own.clone(), false)),
            (None, Some(compiled)) => Some((compiled, true)),
            _ => None,
        }
    }
}

/// A file name from a question's name: no path characters.
fn file_stem(name: &str) -> String {
    let t: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim().to_string();
    if t.is_empty() { "question".into() } else { t }
}

/// Whether `node` opens as a question page.
pub fn is_card(node: &Node) -> bool {
    matches!(node.kind, Kind::Question | Kind::Model | Kind::Metric)
}

/// Draw the page for `node`, opening it first if it is not the one shown.
pub fn page(
    slot: &mut Option<Question>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    node: &Node,
) {
    if !slot
        .as_ref()
        .is_some_and(|q| q.node.kind == node.kind && q.node.id == node.id)
    {
        *slot = Some(Question::open(cx.services, provider, node));
    }
    let Some(q) = slot.as_mut() else {
        return;
    };
    q.drain(cx, provider);
    let s = cx.services;
    let url = provider.item_url(node.kind, &node.id);
    form::page_title(
        ui,
        theme,
        &node.name,
        Some(&s.t(&format!("analytics-kind-{}", node.kind.tag()))),
    );
    match &q.card {
        Loaded::Loading => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-loading"));
            });
            return;
        }
        Loaded::Failed(e) => {
            form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e));
            return;
        }
        Loaded::Ready(_) => {}
    }
    actions(q, ui, cx, theme, provider, &url);
    ui.add_space(form::ROW_GAP);
    params(q, ui, cx, theme);
    ui.add_space(form::PANE_GAP);
    views(q, ui, cx, theme);
}

fn actions(
    q: &mut Question,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    url: &str,
) {
    let s = cx.services;
    form::centred_row_wrapped(ui, |ui| {
        if let Run::Running(_) = q.run {
            if form::button(ui, theme, ph::X, &s.t("analytics-cancel")).clicked() {
                q.cancel(s);
            }
            ui.spinner();
        } else if form::primary_button(ui, theme, &format!("{} {}", ph::PLAY, s.t("analytics-run")))
            .on_hover_text(s.t("analytics-run-tip"))
            .clicked()
        {
            q.run(s, provider);
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
                    url: url.to_string(),
                }),
            );
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-copy-link")).clicked() {
            ui.ctx().copy_text(url.to_string());
        }
        let can_export = q.done.is_some() && !q.exporting;
        ui.add_enabled_ui(can_export, |ui| {
            ui.menu_button(
                format!("{} {}", ph::DOWNLOAD_SIMPLE, s.t("analytics-export")),
                |ui| {
                    for f in ExportFormat::ALL {
                        if ui
                            .button(s.t(&format!("analytics-export-{}", f.ext())))
                            .clicked()
                        {
                            q.export(s, provider, f);
                            ui.close();
                        }
                    }
                },
            );
        })
        .response
        .tip(s.t("analytics-export-tip"));
    });
}

fn params(q: &mut Question, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let Loaded::Ready(card) = &q.card else {
        return;
    };
    if card.params.is_empty() {
        return;
    }
    let months: Vec<String> = (1..=12).map(|m| s.t(&format!("date-month-{m}"))).collect();
    let weekdays: Vec<String> = (1..=7).map(|d| s.t(&format!("date-day-{d}"))).collect();
    let labels = hs_ui::datefield::DateLabels {
        open: &s.t("date-open"),
        today: &s.t("date-today"),
        clear: &s.t("date-clear"),
        months: &months,
        weekdays: &weekdays,
    };
    let list: Vec<Param> = card.params.clone();
    form::field_grid(ui, "analytics-params", |ui| {
        for p in &list {
            let Some(slot) = q.values.iter_mut().find(|(id, _)| *id == p.id) else {
                continue;
            };
            form::label(ui, &p.name);
            match p.kind {
                ParamKind::Date => {
                    hs_ui::datefield::date_field(ui, theme, &p.id, &mut slot.1, &labels);
                }
                ParamKind::Number => {
                    form::text_field_width(
                        ui,
                        &mut slot.1,
                        &s.t("analytics-param-number"),
                        form::FIELD_MIN_WIDTH,
                    );
                }
                ParamKind::Text => {
                    form::text_field_width(
                        ui,
                        &mut slot.1,
                        &s.t("analytics-param-text"),
                        form::FIELD_MIN_WIDTH,
                    );
                }
            }
            ui.end_row();
        }
    });
}

fn views(q: &mut Question, ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme) {
    let s = cx.services;
    let labels = [
        s.t("analytics-view-chart"),
        s.t("analytics-view-table"),
        s.t("analytics-view-sql"),
    ];
    let names: Vec<&str> = labels.iter().map(String::as_str).collect();
    let none: [Option<egui::ImageSource<'static>>; 3] = [None, None, None];
    let glyphs = [
        Some((ph::CHART_LINE, theme.text_muted)),
        Some((ph::TABLE, theme.text_muted)),
        Some((ph::CODE, theme.text_muted)),
    ];
    let current = match q.view {
        View::Chart => 0,
        View::Table => 1,
        View::Sql => 2,
    };
    ui.allocate_ui(egui::vec2(ui.available_width().min(420.0), 30.0), |ui| {
        if let Some(i) = hs_ui::tabs::segmented(ui, theme, &names, &none, &glyphs, current) {
            q.view = [View::Chart, View::Table, View::Sql][i];
        }
    });
    ui.add_space(form::PANE_GAP);
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
        form::hint(ui, theme, &s.t("analytics-not-run"));
        return;
    };
    // The status line: how much was read, how long it took.
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
    match q.view {
        View::Table => table_view(ui, cx, theme, done, &mut q.table),
        View::Chart => chart_view(ui, cx, theme, done),
        View::Sql => sql_view(ui, cx, theme, q.sql()),
    }
}

fn table_view(
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    done: &Done,
    state: &mut TableState,
) {
    let s = cx.services;
    let height = ui.available_height().max(160.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), height), |ui| {
        DataTable {
            theme,
            columns: &done.specs,
            rows: &done.rows,
            row_height: hs_ui::table::ROW_HEIGHT,
            show_filter: true,
            copy_row_label: &s.t("table-copy-row"),
            built_in_menu: true,
            row_icons: None,
            empty_hint: Some(&s.t("analytics-result-empty")),
        }
        .show(ui, state);
    });
}

fn sql_view(ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme, sql: Option<(String, bool)>) {
    let s = cx.services;
    let Some((text, generated)) = sql else {
        form::note(ui, theme, ph::LIGHTBULB, &s.t("analytics-sql-none"));
        return;
    };
    if generated {
        form::hint(ui, theme, &s.t("analytics-sql-generated"));
        ui.add_space(form::PANE_GAP);
    }
    form::centred_row_wrapped(ui, |ui| {
        if form::button(ui, theme, ph::COPY, &s.t("analytics-sql-copy")).clicked() {
            ui.ctx().copy_text(text.clone());
        }
        ui.add_enabled_ui(false, |ui| {
            form::button(ui, theme, ph::DATABASE, &s.t("analytics-sql-open-db"))
        })
        .inner
        .tip_disabled(s.t("analytics-section-soon"));
    });
    ui.add_space(form::PANE_GAP);
    egui::ScrollArea::both()
        .id_salt("analytics-sql")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).monospace())
                    .selectable(true)
                    .wrap_mode(egui::TextWrapMode::Extend),
            );
        });
}

fn chart_view(ui: &mut egui::Ui, cx: &mut ToolContext, theme: &Theme, done: &Done) {
    let s = cx.services;
    match &done.plan {
        Plan::Table(reason) => {
            let key = match reason {
                Reason::NotDrawn => "analytics-chart-not-drawn",
                Reason::NoRows => "analytics-result-empty",
                Reason::NoNumbers => "analytics-chart-no-numbers",
                Reason::IsTable => "analytics-chart-is-table",
            };
            form::note(ui, theme, ph::LIGHTBULB, &s.t(key));
        }
        Plan::Scalar(sc) => scalar(ui, theme, sc),
        Plan::Progress { title, value, goal } => progress(ui, theme, title, *value, *goal),
        Plan::Pie(slices) => pie(ui, theme, slices),
        Plan::Series(p) => series(ui, theme, p),
    }
}

fn scalar(ui: &mut egui::Ui, theme: &Theme, sc: &chart::Scalar) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(&sc.value).size(56.0).strong());
        ui.label(egui::RichText::new(&sc.title).color(theme.text_muted));
        if let Some(c) = &sc.change {
            let colour = if c.up { theme.success } else { theme.error };
            let arrow = if c.up { ph::ARROW_UP } else { ph::ARROW_DOWN };
            ui.label(egui::RichText::new(format!("{arrow} {}", c.text)).color(colour));
        }
    });
}

fn progress(ui: &mut egui::Ui, theme: &Theme, title: &str, value: f64, goal: f64) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(format!("{} / {}", fmt_num(value), fmt_num(goal)))
                .size(40.0)
                .strong(),
        );
        ui.label(egui::RichText::new(title).color(theme.text_muted));
        ui.add_space(form::ROW_GAP);
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(520.0), 22.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 4.0, theme.surface);
        let frac = (value / goal).clamp(0.0, 1.0) as f32;
        let mut filled = rect;
        filled.set_right(rect.left() + rect.width() * frac);
        ui.painter().rect_filled(
            filled,
            4.0,
            if value >= goal {
                theme.success
            } else {
                theme.accent
            },
        );
    });
}

fn fmt_num(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

fn pie(ui: &mut egui::Ui, theme: &Theme, slices: &[chart::Slice]) {
    let side = ui.available_height().clamp(180.0, 360.0);
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let centre = rect.center();
        let radius = side / 2.0 - 4.0;
        let mut mesh = egui::Mesh::default();
        let mut from = -std::f32::consts::FRAC_PI_2;
        for (i, sl) in slices.iter().enumerate() {
            let sweep = (sl.percent / 100.0) as f32 * std::f32::consts::TAU;
            let steps = ((sweep / 0.08).ceil() as usize).max(1);
            let colour = palette8::by_index(i);
            for k in 0..steps {
                let a0 = from + sweep * k as f32 / steps as f32;
                let a1 = from + sweep * (k + 1) as f32 / steps as f32;
                let base = mesh.vertices.len() as u32;
                for p in [
                    centre,
                    centre + radius * egui::vec2(a0.cos(), a0.sin()),
                    centre + radius * egui::vec2(a1.cos(), a1.sin()),
                ] {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: p,
                        uv: egui::epaint::WHITE_UV,
                        color: colour,
                    });
                }
                mesh.indices.extend([base, base + 1, base + 2]);
            }
            from += sweep;
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.add_space(form::ROW_GAP * 2.0);
        ui.vertical(|ui| {
            for (i, sl) in slices.iter().enumerate() {
                form::centred_row(ui, |ui| {
                    let (sw, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    ui.painter().rect_filled(sw, 2.0, palette8::by_index(i));
                    ui.label(format!("{}  {:.1}%", sl.label, sl.percent));
                });
            }
        });
    });
    let _ = theme;
}

fn series(ui: &mut egui::Ui, _theme: &Theme, p: &SeriesPlan) {
    let height = ui.available_height().max(240.0);
    let labels = p.x_labels.clone();
    let at = |v: f64| -> String {
        let r = v.round();
        if (v - r).abs() > 1e-6 || r < 0.0 {
            return String::new();
        }
        match &labels {
            // A tick past the last category has no label.
            Some(l) => l.get(r as usize).cloned().unwrap_or_default(),
            None => fmt_num(v),
        }
    };
    let horizontal = p.horizontal;
    let mut plot = Plot::new("analytics-chart")
        .height(height)
        .legend(Legend::default())
        .allow_scroll(false)
        .label_formatter({
            let labels = p.x_labels.clone();
            let horizontal = p.horizontal;
            move |hover| {
                use egui_plot::HoverPosition;
                let (name, x, y) = match hover {
                    HoverPosition::NearDataPoint {
                        plot_name,
                        position,
                        ..
                    } => (Some(*plot_name), position.x, position.y),
                    HoverPosition::Elsewhere { position } => (None, position.x, position.y),
                };
                let (cat, val) = if horizontal { (y, x) } else { (x, y) };
                let cat = match &labels {
                    Some(l) if (cat - cat.round()).abs() < 1e-6 && cat >= 0.0 => {
                        l.get(cat.round() as usize).cloned().unwrap_or_default()
                    }
                    _ => fmt_num(cat),
                };
                Some(match name {
                    Some(n) => format!("{n}\n{cat}: {}", fmt_num(val)),
                    None => format!("{cat}: {}", fmt_num(val)),
                })
            }
        });
    if horizontal {
        plot = plot.y_axis_formatter(move |m, _| at(m.value));
        plot = plot.x_axis_label(egui::RichText::new(""));
    } else {
        plot = plot.x_axis_formatter(move |m, _| at(m.value));
        plot = plot.x_axis_label(p.x_title.clone());
    }
    let bars: Vec<&Series> = p
        .series
        .iter()
        .filter(|s| s.kind == SeriesKind::Bar)
        .collect();
    let n_bars = bars.len().max(1);
    let group_w = 0.7 / if p.stacked { 1.0 } else { n_bars as f64 };
    plot.show(ui, |plot_ui| {
        // Stacked bars sit on the sum so far at each position.
        let mut totals: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
        let mut bar_no = 0usize;
        for (i, s) in p.series.iter().enumerate() {
            let colour = palette8::by_index(i);
            match s.kind {
                SeriesKind::Line | SeriesKind::Area => {
                    let mut line = Line::new(s.name.clone(), s.points.clone())
                        .color(colour)
                        .width(2.0);
                    if s.kind == SeriesKind::Area {
                        line = line.fill(0.0).fill_alpha(0.25);
                    }
                    plot_ui.line(line);
                    plot_ui.points(
                        Points::new(s.name.clone(), s.points.clone())
                            .color(colour)
                            .radius(3.0),
                    );
                }
                SeriesKind::Scatter => {
                    plot_ui.points(
                        Points::new(s.name.clone(), s.points.clone())
                            .color(colour)
                            .radius(4.0),
                    );
                }
                SeriesKind::Bar => {
                    let shift = if p.stacked {
                        0.0
                    } else {
                        (bar_no as f64 - (n_bars as f64 - 1.0) / 2.0) * group_w
                    };
                    bar_no += 1;
                    let items: Vec<Bar> = s
                        .points
                        .iter()
                        .map(|[x, y]| {
                            let mut b = Bar::new(x + shift, *y).width(group_w).fill(colour);
                            if p.stacked {
                                let key = x.round() as i64;
                                let base = totals.entry(key).or_insert(0.0);
                                b = b.base_offset(*base);
                                *base += *y;
                            }
                            if horizontal {
                                b = b.horizontal();
                            }
                            b
                        })
                        .collect();
                    let mut chart = BarChart::new(s.name.clone(), items).color(colour);
                    if horizontal {
                        chart = chart.horizontal();
                    }
                    plot_ui.bar_chart(chart);
                }
            }
        }
    });
}
