//! A KPI's page (docs/tools/analytics.md §4.10): its value against its goal,
//! how it stands, and its trend, read-only. The trend is drawn by the shared
//! chart code as a line.

use std::sync::Arc;

use hs_core::tool::{OpenRequest, ToolContext};
use hs_ui::form;
use hs_ui::icons::ph;
use hs_ui::theme::Theme;
use parking_lot::Mutex;

use super::question::{Done, chart_view};
use crate::provider::{BiProvider, Card, Column, ColumnKind, Kind, Kpi, Node, Result, Table};

type Inbox = Arc<Mutex<Option<Result<Kpi>>>>;

enum Load {
    Loading,
    Ready(Box<Kpi>, Box<Done>),
    Failed(String),
}

pub struct KpiPage {
    node_id: String,
    load: Load,
    inbox: Inbox,
}

/// 1284000 as `1,284,000`.
fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}

/// A KPI's number as its format writes it: a whole number with thousands
/// separators, a currency with a leading sign, a percentage, or two decimals.
pub fn format_value(v: f64, format: &str) -> String {
    match format.to_lowercase().as_str() {
        "integer" => thousands(v.round() as i64),
        "currency" => format!("${}", thousands(v.round() as i64)),
        "percentage" | "percent" => format!("{:.1}%", v * 100.0),
        _ if v.fract() == 0.0 => thousands(v as i64),
        _ => format!("{v:.2}"),
    }
}

/// How the value stands against its goal, as a fraction (0 with no goal).
pub fn progress(value: f64, goal: f64) -> f32 {
    if goal > 0.0 {
        (value / goal).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}

/// The trend as a one-column table the chart code draws as a line.
fn trend_table(k: &Kpi) -> Table {
    Table {
        columns: vec![Column {
            name: "Trend".into(),
            title: "Trend".into(),
            kind: ColumnKind::Decimal,
        }],
        rows: k.trend.iter().map(|v| vec![serde_json::json!(v)]).collect(),
        ..Default::default()
    }
}

fn card_for(node: &Node) -> Card {
    Card {
        id: node.id.clone(),
        kind: Kind::Question,
        name: node.name.clone(),
        description: String::new(),
        display: "line".into(),
        native_sql: None,
        database: String::new(),
        params: Vec::new(),
        viz: serde_json::Value::Null,
    }
}

impl KpiPage {
    fn start(s: &hs_core::Services, provider: &Arc<dyn BiProvider>, id: &str, inbox: Inbox) {
        let (p, id) = (provider.clone(), id.to_string());
        s.runtime.spawn(async move {
            let r = p.kpi(&id).await;
            *inbox.lock() = Some(r);
        });
    }

    fn drain(&mut self, node: &Node) {
        let Some(r) = self.inbox.lock().take() else {
            return;
        };
        self.load = match r {
            Ok(k) => {
                let done = Done::new(&card_for(node), trend_table(&k));
                Load::Ready(Box::new(k), Box::new(done))
            }
            Err(e) => Load::Failed(e),
        };
    }
}

/// Draw the page for the KPI `node`, reading it when it is first shown.
pub fn page(
    slot: &mut Option<KpiPage>,
    ui: &mut egui::Ui,
    cx: &mut ToolContext,
    theme: &Theme,
    provider: &Arc<dyn BiProvider>,
    node: &Node,
) {
    let s = cx.services;
    if !slot.as_ref().is_some_and(|p| p.node_id == node.id) {
        let inbox: Inbox = Arc::new(Mutex::new(None));
        KpiPage::start(s, provider, &node.id, inbox.clone());
        *slot = Some(KpiPage {
            node_id: node.id.clone(),
            load: Load::Loading,
            inbox,
        });
    }
    let Some(p) = slot.as_mut() else {
        return;
    };
    p.drain(node);
    let url = provider.item_url(Kind::Kpi, &node.id);
    form::page_title(ui, theme, &node.name, Some(&s.t("analytics-kind-kpi")));
    form::centred_row_wrapped(ui, |ui| {
        if form::button(ui, theme, ph::ARROWS_CLOCKWISE, &s.t("analytics-refresh")).clicked() {
            p.load = Load::Loading;
            KpiPage::start(s, provider, &node.id, p.inbox.clone());
        }
        if form::button(
            ui,
            theme,
            ph::ARROW_SQUARE_OUT,
            &s.t("analytics-open-browser"),
        )
        .clicked()
        {
            cx.open("browser", Some(OpenRequest::Url { url: url.clone() }));
        }
        if form::button(ui, theme, ph::COPY, &s.t("analytics-copy-link")).clicked() {
            ui.ctx().copy_text(url.clone());
        }
    });
    ui.add_space(form::ROW_GAP);
    match &p.load {
        Load::Loading => {
            form::centred_row(ui, |ui| {
                ui.spinner();
                ui.label(s.t("analytics-loading"));
            });
        }
        Load::Failed(e) => form::note(ui, theme, ph::WARNING, &hs_core::fmt::sentence(e)),
        Load::Ready(k, done) => {
            let (word, colour) = match k.status {
                1 => (s.t("analytics-kpi-good"), theme.success),
                -1 => (s.t("analytics-kpi-bad"), theme.error),
                _ => (s.t("analytics-kpi-neutral"), theme.warning),
            };
            let value = format_value(k.value, &k.format);
            let goal = format_value(k.goal, &k.format);
            ui.label(egui::RichText::new(&value).size(34.0).strong());
            ui.add_space(form::PANE_GAP);
            form::detail_table(
                ui,
                theme,
                "analytics-kpi-facts",
                [
                    (s.t("analytics-kpi-value").as_str(), value.as_str()),
                    (s.t("analytics-kpi-goal").as_str(), goal.as_str()),
                    (s.t("analytics-kpi-status").as_str(), word.as_str()),
                ],
            );
            ui.add_space(form::PANE_GAP);
            if k.goal > 0.0 {
                ui.add(
                    egui::ProgressBar::new(progress(k.value, k.goal))
                        .fill(colour)
                        .show_percentage()
                        .desired_width(form::FIELD_MAX_WIDTH),
                );
                ui.add_space(form::ROW_GAP);
            }
            if k.trend.len() > 1 {
                form::subsection(ui, theme, &s.t("analytics-kpi-trend"));
                ui.add_space(form::PANE_GAP);
                chart_view(ui, cx, theme, done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kpis_numbers_are_written_in_its_format() {
        assert_eq!(format_value(1_284_000.0, "Currency"), "$1,284,000");
        assert_eq!(format_value(121.0, "Integer"), "121");
        assert_eq!(format_value(-1_500.4, "Integer"), "-1,500");
        assert_eq!(format_value(0.256, "Percentage"), "25.6%");
        assert_eq!(format_value(12.0, ""), "12");
        assert_eq!(format_value(12.345, ""), "12.35");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
    }

    #[test]
    fn progress_is_the_value_over_the_goal_kept_between_zero_and_one() {
        assert!((progress(75.0, 100.0) - 0.75).abs() < 1e-6);
        assert_eq!(progress(150.0, 100.0), 1.0);
        assert_eq!(progress(5.0, 0.0), 0.0);
        assert_eq!(progress(-5.0, 10.0), 0.0);
    }

    #[test]
    fn the_trend_is_one_column_of_numbers() {
        let k = Kpi {
            trend: vec![1.0, 2.5],
            ..Default::default()
        };
        let t = trend_table(&k);
        assert_eq!((t.columns.len(), t.rows.len()), (1, 2));
        assert_eq!(t.rows[1][0], serde_json::json!(2.5));
    }
}
