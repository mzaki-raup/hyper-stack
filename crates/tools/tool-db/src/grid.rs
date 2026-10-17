//! The result grid (docs/tools/database.md §7): virtualised rows, resizable
//! columns, NULL drawn distinctly, numbers right-aligned, client-side sort
//! (numeric-aware) and quick filter, cell selection, a cell viewer on
//! double-click and copy as any export format from the context menu.

use egui_extras::{Column as TableColumn, TableBuilder};
use hs_ui::theme::Theme;

use crate::driver::{Column, Row, Value};

/// (row count, filter, sort) the cached order was computed for.
type OrderKey = (usize, String, Option<(usize, bool)>);

#[derive(Debug, Default, Clone)]
pub struct GridState {
    /// (column, ascending)
    pub sort: Option<(usize, bool)>,
    pub filter: String,
    /// (row index in the data, column)
    pub selected: Option<(usize, usize)>,
    /// Display order for (row count, filter, sort).
    cache: Option<(OrderKey, Vec<usize>)>,
    /// What the data columns measured last frame, so the next frame can
    /// make the scroll content that wide. Read back from the table rather
    /// than recomputed, so a column the user drags wider is included.
    wanted_width: f32,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct GridResponse {
    /// Double-clicked cell: open the cell viewer.
    pub open_cell: Option<(usize, usize)>,
    /// A context-menu choice: (what, row, column).
    pub menu: Option<(GridMenu, usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridMenu {
    CopyCell,
    CopyRow,
    ViewCell,
    FilterByValue,
}

pub struct Labels<'a> {
    pub null: &'a str,
    pub copy_cell: &'a str,
    pub copy_row: &'a str,
    pub view_cell: &'a str,
    pub filter_value: &'a str,
}

fn cmp_values(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    let num = |v: &Value| match v {
        Value::Int(i) => Some(*i as f64),
        Value::Float(f) => Some(*f),
        Value::Number(s) => s.parse::<f64>().ok(),
        _ => None,
    };
    match (a, b) {
        (Value::Null, Value::Null) => Equal,
        (Value::Null, _) => Greater,
        (_, Value::Null) => Less,
        _ => match (num(a), num(b)) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Equal),
            _ => a.display().cmp(&b.display()),
        },
    }
}

/// Row indices after the quick filter and sort.
pub fn order(rows: &[Row], state: &mut GridState) -> Vec<usize> {
    let key = (rows.len(), state.filter.clone(), state.sort);
    if let Some((k, o)) = &state.cache
        && *k == key
    {
        return o.clone();
    }
    let needle = state.filter.to_lowercase();
    let mut idx: Vec<usize> = (0..rows.len())
        .filter(|&i| {
            needle.is_empty()
                || rows[i]
                    .iter()
                    .any(|v| !v.is_null() && v.display().to_lowercase().contains(&needle))
                || (needle == "null" && rows[i].iter().any(Value::is_null))
        })
        .collect();
    if let Some((col, asc)) = state.sort {
        idx.sort_by(|&a, &b| {
            let o = cmp_values(
                rows[a].get(col).unwrap_or(&Value::Null),
                rows[b].get(col).unwrap_or(&Value::Null),
            );
            if asc { o } else { o.reverse() }
        });
    }
    state.cache = Some((key, idx.clone()));
    idx
}

/// Text for one cell in the grid: the first line, at most 200 characters.
fn cell_preview(v: &Value) -> String {
    let s = v.display();
    let line = s.lines().next().unwrap_or("");
    let mut out: String = line.chars().take(200).collect();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

/// The fixed strip that carries the row number, beside the scrolling table.
const ROW_NUMBER_WIDTH: f32 = 48.0;
/// Shared by the table and the gutter, which have to line up.
const HEADER_HEIGHT: f32 = 24.0;
const ROW_HEIGHT: f32 = 22.0;

fn initial_width(col: &Column, rows: &[Row], i: usize) -> f32 {
    let sample = rows
        .iter()
        .take(50)
        .filter_map(|r| r.get(i))
        .map(|v| v.display().chars().count().min(60))
        .max()
        .unwrap_or(4);
    let chars = sample.max(col.name.chars().count() + 2) as f32;
    (chars * 7.5 + 16.0).clamp(60.0, 360.0)
}

#[allow(clippy::too_many_arguments)] // one call site; the pieces are all needed
pub fn show(
    ui: &mut egui::Ui,
    id: egui::Id,
    theme: &Theme,
    columns: &[Column],
    rows: &[Row],
    state: &mut GridState,
    labels: &Labels,
    order: &[usize],
) -> GridResponse {
    let mut resp = GridResponse::default();
    let mut new_sort = None;
    // egui_extras' table scrolls vertically only, so every column past
    // the pane's edge used to be unreachable — most visibly once the AI
    // assistant took its column and the grid lost 390 px. The table goes
    // in a horizontal ScrollArea whose content is as wide as the columns
    // want to be, so the pane's width decides how much is *visible*, never
    // how much exists. Owner's report, 2026-10-17.
    let spacing_x = ui.spacing().item_spacing.x;
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, egui::Sense::hover());
    // The row number stays put while the data scrolls under it, the way a
    // line-number gutter does: it is the thing you count *from*, so losing
    // it off the left edge is the one loss a wide result cannot afford.
    // That means it cannot live inside the table, which is what scrolls.
    let gutter = egui::Rect::from_min_max(
        full.min,
        egui::pos2(full.left() + ROW_NUMBER_WIDTH, full.bottom()),
    );
    let table_rect =
        egui::Rect::from_min_max(egui::pos2(gutter.right() + spacing_x, full.top()), full.max);
    // The width the data columns actually want. It has to be worked out
    // here, from the same measurements the columns are built with: inside
    // the table, sizing only ever *shares out* the width it is given, so a
    // table asked to fit a narrow pane shrinks its columns and then has
    // nothing to scroll. `wanted_width` carries a column the user has
    // dragged wider than its natural size.
    let natural = columns
        .iter()
        .enumerate()
        .map(|(i, c)| initial_width(c, rows, i) + spacing_x)
        .sum::<f32>();
    let content_width = natural.max(state.wanted_width).max(table_rect.width());
    let mut measured = state.wanted_width;
    let mut row_tops: Vec<(usize, f32)> = Vec::new();
    let mut table_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(table_rect)
            .layout(*ui.layout()),
    );
    egui::ScrollArea::horizontal()
        .id_salt(id.with("hscroll"))
        .show(&mut table_ui, |ui| {
            // Both bounds: the table reads `available_rect_before_wrap` to
            // share out its width, and inside a horizontal ScrollArea that
            // is unbounded, which would make the trailing remainder column
            // infinitely wide.
            ui.set_min_width(content_width);
            ui.set_max_width(content_width);
            grid_table(
                ui,
                id,
                theme,
                columns,
                rows,
                state,
                labels,
                order,
                &mut resp,
                &mut new_sort,
                &mut measured,
                &mut row_tops,
            );
        });
    state.wanted_width = measured;
    paint_gutter(ui, gutter, theme, state, &row_tops);
    if let Some(s) = new_sort {
        state.sort = s;
    }
    resp
}

/// The table itself, inside the scroll area. `measured` comes back as the
/// width the data columns asked for, the trailing remainder excluded — it
/// only exists to soak up slack when the columns do not fill the pane, and
/// counting it would wedge the content at whatever width it first had.
#[allow(clippy::too_many_arguments)]
fn grid_table(
    ui: &mut egui::Ui,
    id: egui::Id,
    theme: &Theme,
    columns: &[Column],
    rows: &[Row],
    state: &mut GridState,
    labels: &Labels,
    order: &[usize],
    resp: &mut GridResponse,
    new_sort: &mut Option<Option<(usize, bool)>>,
    measured: &mut f32,
    row_tops: &mut Vec<(usize, f32)>,
) {
    let mono = egui::TextStyle::Monospace;
    let row_h = ROW_HEIGHT;
    let spacing_x = ui.spacing().item_spacing.x;
    // No row sense: a row-wide click area is registered after the cells
    // and would take their clicks (selection, double-click).
    let mut builder = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(true)
        .auto_shrink([false, false]);
    for (i, c) in columns.iter().enumerate() {
        builder = builder.column(
            TableColumn::initial(initial_width(c, rows, i))
                .at_least(40.0)
                .clip(true),
        );
    }
    builder = builder.column(TableColumn::remainder());
    builder
        .header(HEADER_HEIGHT, |mut header| {
            for (i, c) in columns.iter().enumerate() {
                header.col(|ui| {
                    let arrow = match state.sort {
                        Some((col, true)) if col == i => " ▲",
                        Some((col, false)) if col == i => " ▼",
                        _ => "",
                    };
                    let r = ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{}{arrow}", c.name)).strong(),
                        )
                        .selectable(false)
                        .sense(egui::Sense::click()),
                    );
                    let r = if c.type_name.is_empty() {
                        r
                    } else {
                        r.on_hover_text(&c.type_name)
                    };
                    if r.clicked() {
                        *new_sort = Some(match state.sort {
                            Some((col, true)) if col == i => Some((i, false)),
                            Some((col, false)) if col == i => None,
                            _ => Some((i, true)),
                        });
                    }
                });
            }
            header.col(|_| {});
        })
        .body(|body| {
            // Columns are: the row-number column, one per data column, then
            // the remainder. Everything but the remainder is real content.
            let w = body.widths();
            let n = w.len().saturating_sub(1);
            *measured = w.iter().take(n).sum::<f32>() + spacing_x * n as f32;
            body.rows(row_h, order.len(), |mut row| {
                let data_idx = order[row.index()];
                let r_data = &rows[data_idx];
                row.set_selected(state.selected.is_some_and(|(r, _)| r == data_idx));
                for ci in 0..columns.len() {
                    let v = r_data.get(ci).unwrap_or(&Value::Null);
                    let selected = state.selected == Some((data_idx, ci));
                    let mut cell = None;
                    row.col(|ui| {
                        // The click area goes in first with a stable id; the
                        // label on top only senses hover, so clicks reach it.
                        cell = Some(ui.interact(
                            ui.max_rect(),
                            id.with(("cell", data_idx, ci)),
                            egui::Sense::click(),
                        ));
                        if selected {
                            ui.painter().rect_stroke(
                                ui.max_rect().shrink(1.0),
                                egui::CornerRadius::same(2),
                                egui::Stroke::new(1.5, theme.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        let label = match v {
                            Value::Null => egui::Label::new(
                                egui::RichText::new(labels.null)
                                    .italics()
                                    .color(theme.text_muted),
                            ),
                            _ => egui::Label::new(
                                egui::RichText::new(cell_preview(v)).text_style(mono.clone()),
                            ),
                        }
                        .truncate()
                        .selectable(false);
                        if v.is_numeric() {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| ui.add(label),
                            );
                        } else {
                            ui.add(label);
                        }
                    });
                    let Some(cell) = cell else { continue };
                    if cell.clicked() || cell.secondary_clicked() {
                        state.selected = Some((data_idx, ci));
                    }
                    if cell.double_clicked() {
                        resp.open_cell = Some((data_idx, ci));
                    }
                    cell.context_menu(|ui| {
                        for (what, label) in [
                            (GridMenu::CopyCell, labels.copy_cell),
                            (GridMenu::CopyRow, labels.copy_row),
                            (GridMenu::ViewCell, labels.view_cell),
                            (GridMenu::FilterByValue, labels.filter_value),
                        ] {
                            if ui.button(label).clicked() {
                                resp.menu = Some((what, data_idx, ci));
                                ui.close();
                            }
                        }
                    });
                }
                // Only the rows egui actually drew report back, which is
                // exactly the set the gutter has to number.
                row.col(|ui| row_tops.push((data_idx, ui.max_rect().top())));
            });
        });
}

/// Number the rows the table drew, in a strip the horizontal scroll never
/// touches. Painted rather than laid out, because the rows it lines up with
/// live inside the table's own virtualised scroll area.
fn paint_gutter(
    ui: &egui::Ui,
    rect: egui::Rect,
    theme: &Theme,
    state: &GridState,
    row_tops: &[(usize, f32)],
) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, theme.panel_bg);
    painter.line_segment(
        [rect.right_top(), rect.right_bottom()],
        egui::Stroke::new(1.0, theme.border),
    );
    let font = egui::TextStyle::Monospace.resolve(ui.style());
    painter.text(
        egui::pos2(rect.right() - 6.0, rect.top() + HEADER_HEIGHT / 2.0),
        egui::Align2::RIGHT_CENTER,
        "#",
        font.clone(),
        theme.text_muted,
    );
    for &(data_idx, top) in row_tops {
        let row = egui::Rect::from_min_max(
            egui::pos2(rect.left(), top),
            egui::pos2(rect.right(), top + ROW_HEIGHT),
        );
        if !rect.intersects(row) {
            continue;
        }
        if state.selected.is_some_and(|(r, _)| r == data_idx) {
            painter.rect_filled(row, 0.0, theme.accent.gamma_multiply(0.25));
        }
        painter.text(
            egui::pos2(rect.right() - 6.0, row.center().y),
            egui::Align2::RIGHT_CENTER,
            (data_idx + 1).to_string(),
            font.clone(),
            theme.text_muted,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_numbers_numerically_and_nulls_last_then_filters() {
        let rows: Vec<Row> = vec![
            vec![Value::Number("10.5".into()), Value::Text("b".into())],
            vec![Value::Null, Value::Text("a".into())],
            vec![Value::Int(9), Value::Text("c".into())],
        ];
        let mut st = GridState {
            sort: Some((0, true)),
            ..Default::default()
        };
        assert_eq!(order(&rows, &mut st), vec![2, 0, 1]);
        st.sort = Some((1, false));
        assert_eq!(order(&rows, &mut st), vec![2, 0, 1]);
        st.filter = "B".into();
        assert_eq!(order(&rows, &mut st), vec![0]);
        st.filter = "null".into();
        assert_eq!(order(&rows, &mut st), vec![1]);
    }
}
