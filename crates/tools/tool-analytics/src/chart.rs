//! Turning a result and a card's chart settings into what is drawn
//! (docs/tools/analytics.md §4.4). Nothing here touches egui: a [`Plan`] is
//! worked out once per result and the screens draw from it every frame.

use serde_json::Value;

use crate::provider::{Card, Column, ColumnKind, Table};

/// A value as the table shows it.
pub fn cell_text(kind: ColumnKind, v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => match kind {
            ColumnKind::Decimal | ColumnKind::Other | ColumnKind::Integer | ColumnKind::Text => {
                match n.as_i64() {
                    Some(i) => i.to_string(),
                    None => decimal(n.as_f64().unwrap_or(f64::NAN)),
                }
            }
            _ => n.to_string(),
        },
        Value::String(t) => match kind {
            ColumnKind::Time => time_text(t),
            _ => t.clone(),
        },
        other => other.to_string(),
    }
}

/// Up to two decimals, none trailing: `3.5`, `2`, `1234.57`. No thousands
/// separator, so a column still sorts as numbers.
fn decimal(f: f64) -> String {
    if !f.is_finite() {
        return String::new();
    }
    let t = format!("{f:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `2026-01-01T00:00:00Z` is a date; a time of day keeps its time.
fn time_text(t: &str) -> String {
    let t = t.trim_end_matches('Z');
    let (date, time) = match t.split_once('T') {
        Some(p) => p,
        None => return t.to_string(),
    };
    let time = time.split(['+', '-']).next().unwrap_or(time);
    let time = time.split('.').next().unwrap_or(time);
    if time == "00:00:00" {
        date.to_string()
    } else {
        format!("{date} {time}")
    }
}

/// A cell as a number, where it is one.
pub fn number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(t) => t.trim().parse().ok(),
        _ => None,
    }
}

/// Why a result is shown as a table instead of a drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The card is a table.
    IsTable,
    /// Metabase draws this kind (a map, a pivot, a funnel); this tab does not.
    NotDrawn,
    NoRows,
    /// The columns hold nothing a chart could plot.
    NoNumbers,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scalar {
    pub title: String,
    pub value: String,
    /// A smart scalar's change against the period before.
    pub change: Option<Change>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// `+12.5%`
    pub text: String,
    pub up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeriesKind {
    Line,
    Area,
    Bar,
    Scatter,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub kind: SeriesKind,
    pub points: Vec<[f64; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SeriesPlan {
    pub series: Vec<Series>,
    /// One label per whole x position, when X is not a number.
    pub x_labels: Option<Vec<String>>,
    pub x_title: String,
    pub stacked: bool,
    /// A row chart: bars lie down.
    pub horizontal: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    pub label: String,
    pub value: f64,
    pub percent: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    Scalar(Scalar),
    /// A progress bar or a gauge: the value against its goal.
    Progress {
        title: String,
        value: f64,
        goal: f64,
    },
    Series(SeriesPlan),
    Pie(Vec<Slice>),
    Table(Reason),
}

impl Plan {
    /// Whether the Chart view has a drawing to show.
    pub fn is_chart(&self) -> bool {
        !matches!(self, Plan::Table(_))
    }
}

/// The index of the column called `name`.
fn column_index(columns: &[Column], name: &str) -> Option<usize> {
    columns.iter().position(|c| c.name == name)
}

/// The column names a setting lists (`graph.dimensions`, `graph.metrics`).
fn setting_columns(card: &Card, key: &str, columns: &[Column]) -> Vec<usize> {
    card.viz
        .get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter_map(|n| column_index(columns, n))
                .collect()
        })
        .unwrap_or_default()
}

/// What a card's display and a result make: the drawing, or the table with
/// the reason there is none.
pub fn plan(card: &Card, table: &Table) -> Plan {
    let display = card.display.as_str();
    if display == "table" {
        return Plan::Table(Reason::IsTable);
    }
    if !matches!(
        display,
        "scalar"
            | "smartscalar"
            | "progress"
            | "gauge"
            | "line"
            | "area"
            | "bar"
            | "row"
            | "combo"
            | "scatter"
            | "waterfall"
            | "pie"
    ) || display == "waterfall"
    {
        return Plan::Table(Reason::NotDrawn);
    }
    if table.rows.is_empty() {
        return Plan::Table(Reason::NoRows);
    }
    match display {
        "scalar" | "smartscalar" | "progress" | "gauge" => single(card, table),
        "pie" => pie(card, table),
        _ => series(card, table),
    }
}

/// The first numeric column, or the first column.
fn value_column(card: &Card, table: &Table) -> usize {
    let named = card
        .viz
        .get("scalar.field")
        .and_then(Value::as_str)
        .and_then(|n| column_index(&table.columns, n));
    named
        .or_else(|| table.columns.iter().position(|c| c.kind.is_number()))
        .unwrap_or(0)
}

fn single(card: &Card, table: &Table) -> Plan {
    let col = value_column(card, table);
    let Some(column) = table.columns.get(col) else {
        return Plan::Table(Reason::NoNumbers);
    };
    let last = table.rows.len() - 1;
    let cell = |row: usize| table.rows.get(row).and_then(|r| r.get(col));
    let value_text = cell(last)
        .map(|v| cell_text(column.kind, v))
        .unwrap_or_default();
    match card.display.as_str() {
        "smartscalar" if last >= 1 => {
            let (now, before) = (cell(last).and_then(number), cell(last - 1).and_then(number));
            let change = match (now, before) {
                (Some(n), Some(b)) if b != 0.0 => {
                    let pct = (n - b) / b.abs() * 100.0;
                    Some(Change {
                        text: format!("{}{}%", if pct >= 0.0 { "+" } else { "" }, decimal(pct)),
                        up: pct >= 0.0,
                    })
                }
                _ => None,
            };
            Plan::Scalar(Scalar {
                title: column.title.clone(),
                value: value_text,
                change,
            })
        }
        "progress" | "gauge" => {
            let goal = card.viz.get("progress.goal").and_then(number).or_else(|| {
                card.viz
                    .get("gauge.segments")
                    .and_then(Value::as_array)
                    .and_then(|a| a.last())
                    .and_then(|s| s.get("max"))
                    .and_then(number)
            });
            match (cell(last).and_then(number), goal) {
                (Some(value), Some(goal)) if goal > 0.0 => Plan::Progress {
                    title: column.title.clone(),
                    value,
                    goal,
                },
                _ => Plan::Scalar(Scalar {
                    title: column.title.clone(),
                    value: value_text,
                    change: None,
                }),
            }
        }
        _ => Plan::Scalar(Scalar {
            title: column.title.clone(),
            value: value_text,
            change: None,
        }),
    }
}

fn pie(card: &Card, table: &Table) -> Plan {
    let dim = card
        .viz
        .get("pie.dimension")
        .and_then(Value::as_str)
        .and_then(|n| column_index(&table.columns, n))
        .unwrap_or(0);
    let metric = card
        .viz
        .get("pie.metric")
        .and_then(Value::as_str)
        .and_then(|n| column_index(&table.columns, n))
        .or_else(|| {
            table
                .columns
                .iter()
                .enumerate()
                .position(|(i, c)| i != dim && c.kind.is_number())
        });
    let Some(metric) = metric else {
        return Plan::Table(Reason::NoNumbers);
    };
    let kind = table.columns[dim].kind;
    let mut slices: Vec<Slice> = table
        .rows
        .iter()
        .filter_map(|r| {
            let value = r.get(metric).and_then(number)?;
            (value > 0.0).then(|| Slice {
                label: r.get(dim).map(|v| cell_text(kind, v)).unwrap_or_default(),
                value,
                percent: 0.0,
            })
        })
        .collect();
    let total: f64 = slices.iter().map(|s| s.value).sum();
    if slices.is_empty() || total <= 0.0 {
        return Plan::Table(Reason::NoNumbers);
    }
    for s in &mut slices {
        s.percent = s.value / total * 100.0;
    }
    Plan::Pie(slices)
}

fn series(card: &Card, table: &Table) -> Plan {
    let display = card.display.as_str();
    let cols = &table.columns;
    let mut dims = setting_columns(card, "graph.dimensions", cols);
    if dims.is_empty() {
        dims.push(0);
    }
    let mut metrics = setting_columns(card, "graph.metrics", cols);
    if metrics.is_empty() {
        metrics = cols
            .iter()
            .enumerate()
            .filter(|(i, c)| !dims.contains(i) && c.kind.is_number())
            .map(|(i, _)| i)
            .collect();
    }
    if metrics.is_empty() {
        return Plan::Table(Reason::NoNumbers);
    }
    let x_col = dims[0];
    let numeric_x = cols[x_col].kind.is_number() && matches!(display, "scatter");
    let x_title = cols[x_col].title.clone();
    // A second dimension with one metric splits it into a series per value.
    let split = (dims.len() >= 2 && metrics.len() == 1).then(|| dims[1]);

    let mut labels: Vec<String> = Vec::new();
    let mut x_of = |row: &[Value]| -> Option<f64> {
        let cell = row.get(x_col)?;
        if numeric_x {
            return number(cell);
        }
        let label = cell_text(cols[x_col].kind, cell);
        let at = labels.iter().position(|l| *l == label).unwrap_or_else(|| {
            labels.push(label);
            labels.len() - 1
        });
        Some(at as f64)
    };
    let kind_of = |name: &str, first: bool| match display {
        "area" => SeriesKind::Area,
        "bar" | "row" => SeriesKind::Bar,
        "scatter" => SeriesKind::Scatter,
        "combo" => match card
            .viz
            .pointer(&format!("/series_settings/{name}/display"))
            .and_then(Value::as_str)
        {
            Some("bar") => SeriesKind::Bar,
            Some("area") => SeriesKind::Area,
            Some("line") => SeriesKind::Line,
            // A combo with no choice made: the first series bars, the rest lines.
            _ if first => SeriesKind::Bar,
            _ => SeriesKind::Line,
        },
        _ => SeriesKind::Line,
    };

    let mut out: Vec<Series> = Vec::new();
    if let Some(split_col) = split {
        let metric = metrics[0];
        for row in &table.rows {
            let (Some(x), Some(y)) = (x_of(row), row.get(metric).and_then(number)) else {
                continue;
            };
            let name = row
                .get(split_col)
                .map(|v| cell_text(cols[split_col].kind, v))
                .unwrap_or_default();
            let at = out.iter().position(|s| s.name == name).unwrap_or_else(|| {
                out.push(Series {
                    kind: kind_of(&name, out.is_empty()),
                    name,
                    points: Vec::new(),
                });
                out.len() - 1
            });
            out[at].points.push([x, y]);
        }
    } else {
        for (n, &metric) in metrics.iter().enumerate() {
            let name = cols[metric].title.clone();
            let mut points = Vec::with_capacity(table.rows.len());
            for row in &table.rows {
                if let (Some(x), Some(y)) = (x_of(row), row.get(metric).and_then(number)) {
                    points.push([x, y]);
                }
            }
            out.push(Series {
                kind: kind_of(&cols[metric].name, n == 0),
                name,
                points,
            });
        }
    }
    out.retain(|s| !s.points.is_empty());
    if out.is_empty() {
        return Plan::Table(Reason::NoNumbers);
    }
    // A row chart lists the first category at the top, as Metabase does.
    if display == "row" {
        let last = labels.len().saturating_sub(1) as f64;
        for s in &mut out {
            for p in &mut s.points {
                p[0] = last - p[0];
            }
        }
        labels.reverse();
    }
    let stacked = matches!(
        card.viz.get("stackable.stack_type").and_then(Value::as_str),
        Some("stacked" | "normalized")
    );
    Plan::Series(SeriesPlan {
        series: out,
        x_labels: (!numeric_x).then_some(labels),
        x_title,
        stacked,
        horizontal: display == "row",
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::provider::Kind;

    fn col(name: &str, kind: ColumnKind) -> Column {
        Column {
            name: name.into(),
            title: name.to_uppercase(),
            kind,
        }
    }

    fn card(display: &str, viz: Value) -> Card {
        Card {
            id: "1".into(),
            kind: Kind::Question,
            name: "q".into(),
            description: String::new(),
            display: display.into(),
            native_sql: None,
            database: String::new(),
            params: Vec::new(),
            viz,
        }
    }

    /// month, count, revenue
    fn by_month() -> Table {
        Table {
            columns: vec![
                col("month", ColumnKind::Time),
                col("count", ColumnKind::Integer),
                col("revenue", ColumnKind::Decimal),
            ],
            rows: vec![
                vec![json!("2026-01-01T00:00:00Z"), json!(12), json!(100.5)],
                vec![json!("2026-02-01T00:00:00Z"), json!(15), json!(120.25)],
                vec![json!("2026-03-01T00:00:00Z"), json!(9), json!(80)],
            ],
            ..Table::default()
        }
    }

    #[test]
    fn a_cell_is_formatted_by_its_columns_type() {
        assert_eq!(cell_text(ColumnKind::Integer, &json!(1200)), "1200");
        assert_eq!(cell_text(ColumnKind::Decimal, &json!(3.5)), "3.5");
        assert_eq!(cell_text(ColumnKind::Decimal, &json!(2.0)), "2");
        assert_eq!(cell_text(ColumnKind::Decimal, &json!(1234.5678)), "1234.57");
        assert_eq!(
            cell_text(ColumnKind::Time, &json!("2026-01-01T00:00:00Z")),
            "2026-01-01"
        );
        assert_eq!(
            cell_text(ColumnKind::Time, &json!("2026-01-01T08:30:15.000+02:00")),
            "2026-01-01 08:30:15"
        );
        assert_eq!(cell_text(ColumnKind::Bool, &json!(true)), "true");
        assert_eq!(cell_text(ColumnKind::Text, &Value::Null), "");
        assert_eq!(cell_text(ColumnKind::Text, &json!("hi")), "hi");
    }

    #[test]
    fn a_line_has_a_point_per_row_and_the_months_as_labels() {
        let Plan::Series(p) = plan(&card("line", Value::Null), &by_month()) else {
            panic!("a series plan");
        };
        assert_eq!(p.series.len(), 2, "count and revenue");
        assert_eq!(p.series[0].name, "COUNT");
        assert_eq!(p.series[0].kind, SeriesKind::Line);
        assert_eq!(p.series[0].points, [[0.0, 12.0], [1.0, 15.0], [2.0, 9.0]]);
        assert_eq!(
            p.x_labels.as_deref(),
            Some(
                &[
                    "2026-01-01".to_string(),
                    "2026-02-01".into(),
                    "2026-03-01".into()
                ][..]
            )
        );
        assert!(!p.stacked && !p.horizontal);
    }

    #[test]
    fn the_cards_own_dimensions_and_metrics_choose_the_columns() {
        let viz = json!({ "graph.dimensions": ["month"], "graph.metrics": ["revenue"] });
        let Plan::Series(p) = plan(&card("bar", viz), &by_month()) else {
            panic!("a series plan");
        };
        assert_eq!(p.series.len(), 1);
        assert_eq!(p.series[0].name, "REVENUE");
        assert_eq!(p.series[0].kind, SeriesKind::Bar);
        assert_eq!(p.series[0].points[1], [1.0, 120.25]);
    }

    #[test]
    fn a_second_dimension_splits_one_metric_into_series_and_stacks_on_request() {
        let t = Table {
            columns: vec![
                col("cat", ColumnKind::Text),
                col("region", ColumnKind::Text),
                col("n", ColumnKind::Integer),
            ],
            rows: vec![
                vec![json!("a"), json!("east"), json!(1)],
                vec![json!("a"), json!("west"), json!(2)],
                vec![json!("b"), json!("east"), json!(3)],
            ],
            ..Table::default()
        };
        let viz = json!({ "graph.dimensions": ["cat", "region"], "graph.metrics": ["n"],
                          "stackable.stack_type": "stacked" });
        let Plan::Series(p) = plan(&card("bar", viz), &t) else {
            panic!("a series plan");
        };
        let names: Vec<&str> = p.series.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["east", "west"]);
        assert_eq!(p.series[0].points, [[0.0, 1.0], [1.0, 3.0]]);
        assert_eq!(p.series[1].points, [[0.0, 2.0]]);
        assert!(p.stacked);
        // A row chart lies down, the first category at the top.
        let Plan::Series(r) = plan(&card("row", Value::Null), &t) else {
            panic!("a series plan");
        };
        assert!(r.horizontal);
        assert_eq!(
            r.x_labels.as_deref(),
            Some(&["b".to_string(), "a".into()][..])
        );
        let east = r.series.iter().find(|s| s.name == "N").expect("a series");
        assert_eq!(
            east.points[0],
            [1.0, 1.0],
            "a is last in the list, so lowest"
        );
    }

    #[test]
    fn a_scatter_with_numeric_x_has_no_labels() {
        let t = Table {
            columns: vec![col("x", ColumnKind::Decimal), col("y", ColumnKind::Decimal)],
            rows: vec![vec![json!(1.5), json!(2.5)], vec![json!(3), json!(1)]],
            ..Table::default()
        };
        let Plan::Series(p) = plan(&card("scatter", Value::Null), &t) else {
            panic!("a series plan");
        };
        assert!(p.x_labels.is_none());
        assert_eq!(p.series[0].kind, SeriesKind::Scatter);
        assert_eq!(p.series[0].points, [[1.5, 2.5], [3.0, 1.0]]);
    }

    #[test]
    fn a_combos_series_follow_their_settings_else_bars_then_lines() {
        let viz = json!({ "series_settings": { "revenue": { "display": "line" } } });
        let Plan::Series(p) = plan(&card("combo", viz), &by_month()) else {
            panic!("a series plan");
        };
        assert_eq!(
            p.series[0].kind,
            SeriesKind::Bar,
            "no choice, first is bars"
        );
        assert_eq!(p.series[1].kind, SeriesKind::Line);
    }

    #[test]
    fn a_pie_has_slices_that_add_to_a_hundred_percent() {
        let t = Table {
            columns: vec![col("cat", ColumnKind::Text), col("n", ColumnKind::Integer)],
            rows: vec![
                vec![json!("a"), json!(30)],
                vec![json!("b"), json!(10)],
                vec![json!("c"), json!(0)],
                vec![json!("d"), Value::Null],
            ],
            ..Table::default()
        };
        let Plan::Pie(s) = plan(&card("pie", Value::Null), &t) else {
            panic!("a pie");
        };
        assert_eq!(s.len(), 2, "zero and empty are not slices");
        assert_eq!(s[0].percent, 75.0);
        assert!((s.iter().map(|x| x.percent).sum::<f64>() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_scalar_shows_the_last_value_and_a_smart_scalar_its_change() {
        let t = Table {
            columns: vec![col("total", ColumnKind::Integer)],
            rows: vec![vec![json!(80)], vec![json!(100)]],
            ..Table::default()
        };
        let Plan::Scalar(s) = plan(&card("scalar", Value::Null), &t) else {
            panic!("a scalar");
        };
        assert_eq!((s.title.as_str(), s.value.as_str()), ("TOTAL", "100"));
        assert!(s.change.is_none());
        let Plan::Scalar(m) = plan(&card("smartscalar", Value::Null), &t) else {
            panic!("a scalar");
        };
        assert_eq!(
            m.change,
            Some(Change {
                text: "+25%".into(),
                up: true
            })
        );
    }

    #[test]
    fn a_progress_bar_needs_its_goal_else_it_is_a_scalar() {
        let t = Table {
            columns: vec![col("n", ColumnKind::Integer)],
            rows: vec![vec![json!(40)]],
            ..Table::default()
        };
        let with = card("progress", json!({ "progress.goal": 100 }));
        assert!(matches!(
            plan(&with, &t),
            Plan::Progress { value, goal, .. } if value == 40.0 && goal == 100.0
        ));
        assert!(matches!(
            plan(&card("progress", Value::Null), &t),
            Plan::Scalar(_)
        ));
    }

    #[test]
    fn what_is_not_drawn_is_the_table_with_its_reason() {
        let t = by_month();
        assert_eq!(
            plan(&card("table", Value::Null), &t),
            Plan::Table(Reason::IsTable)
        );
        assert_eq!(
            plan(&card("map", Value::Null), &t),
            Plan::Table(Reason::NotDrawn)
        );
        assert_eq!(
            plan(&card("pivot", Value::Null), &t),
            Plan::Table(Reason::NotDrawn)
        );
        assert_eq!(
            plan(&card("line", Value::Null), &Table::default()),
            Plan::Table(Reason::NoRows)
        );
        let words = Table {
            columns: vec![col("a", ColumnKind::Text), col("b", ColumnKind::Text)],
            rows: vec![vec![json!("x"), json!("y")]],
            ..Table::default()
        };
        assert_eq!(
            plan(&card("bar", Value::Null), &words),
            Plan::Table(Reason::NoNumbers)
        );
        assert!(!plan(&card("map", Value::Null), &t).is_chart());
    }
}
