//! DataTable v1 (docs/04 §6): virtualised rows, resizable columns, click-to-sort,
//! inline filter, cell/row copy. Later milestones add multi-sort, cell viewers
//! and export.

use egui_extras::{Column, TableBuilder};

use crate::theme::Theme;

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnSpec {
    pub name: String,
    pub width: f32,
    pub numeric: bool,
}

impl ColumnSpec {
    pub fn new(name: impl Into<String>, width: f32) -> Self {
        Self {
            name: name.into(),
            width,
            numeric: false,
        }
    }
    pub fn numeric(mut self) -> Self {
        self.numeric = true;
        self
    }
}

#[derive(Debug, Default, Clone)]
pub struct TableState {
    pub sort: Option<(usize, bool)>, // column, ascending
    pub filter: String,
    pub selected: Option<usize>, // row index in the original data
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TableResponse {
    pub clicked: Option<usize>,
    pub double_clicked: Option<usize>,
    /// Text copied to the clipboard this frame (row as TSV).
    pub copied: Option<String>,
}

pub struct DataTable<'a> {
    pub theme: &'a Theme,
    pub columns: &'a [ColumnSpec],
    pub rows: &'a [Vec<String>],
    pub row_height: f32,
    pub show_filter: bool,
    /// Context-menu label for copying a row (Title Case, from the locale).
    pub copy_row_label: &'a str,
}

impl DataTable<'_> {
    /// Row indices in display order after filter and sort.
    pub fn order(&self, state: &TableState) -> Vec<usize> {
        let needle = state.filter.to_lowercase();
        let mut idx: Vec<usize> = (0..self.rows.len())
            .filter(|&i| {
                needle.is_empty()
                    || self.rows[i]
                        .iter()
                        .any(|c| c.to_lowercase().contains(&needle))
            })
            .collect();
        if let Some((col, asc)) = state.sort {
            let numeric = self.columns.get(col).is_some_and(|c| c.numeric);
            idx.sort_by(|&a, &b| {
                let (x, y) = (self.rows[a].get(col), self.rows[b].get(col));
                let ord = if numeric {
                    let px = x.and_then(|s| s.parse::<f64>().ok()).unwrap_or(f64::NAN);
                    let py = y.and_then(|s| s.parse::<f64>().ok()).unwrap_or(f64::NAN);
                    px.partial_cmp(&py).unwrap_or(std::cmp::Ordering::Equal)
                } else {
                    x.cmp(&y)
                };
                if asc { ord } else { ord.reverse() }
            });
        }
        idx
    }

    pub fn show(&self, ui: &mut egui::Ui, state: &mut TableState) -> TableResponse {
        let mut resp = TableResponse::default();
        if self.show_filter {
            ui.horizontal(|ui| {
                ui.label(crate::icons::SEARCH);
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .desired_width(220.0)
                        .hint_text("filter"),
                );
                if !state.filter.is_empty() && ui.small_button(crate::icons::TAB_CLOSE).clicked() {
                    state.filter.clear();
                }
                ui.colored_label(
                    self.theme.text_muted,
                    format!("{} / {}", self.order(state).len(), self.rows.len()),
                );
            });
        }
        let order = self.order(state);
        let mut builder = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click());
        for c in self.columns {
            builder = builder.column(Column::initial(c.width).at_least(40.0).clip(true));
        }
        builder = builder.column(Column::remainder());
        let mut new_sort = None;
        let mut copy_row = None;
        builder
            .header(22.0, |mut header| {
                for (i, c) in self.columns.iter().enumerate() {
                    header.col(|ui| {
                        let arrow = match state.sort {
                            Some((col, true)) if col == i => " ▲",
                            Some((col, false)) if col == i => " ▼",
                            _ => "",
                        };
                        if ui
                            .add(
                                egui::Label::new(
                                    egui::RichText::new(format!("{}{arrow}", c.name)).strong(),
                                )
                                .sense(egui::Sense::click()),
                            )
                            .clicked()
                        {
                            new_sort = Some(match state.sort {
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
                body.rows(self.row_height, order.len(), |mut row| {
                    let data_idx = order[row.index()];
                    row.set_selected(state.selected == Some(data_idx));
                    for (ci, c) in self.columns.iter().enumerate() {
                        row.col(|ui| {
                            let text = self.rows[data_idx]
                                .get(ci)
                                .map(String::as_str)
                                .unwrap_or("");
                            if c.numeric {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(egui::RichText::new(text).monospace())
                                                .truncate(),
                                        );
                                    },
                                );
                            } else {
                                ui.add(egui::Label::new(text).truncate());
                            }
                        });
                    }
                    row.col(|_| {});
                    let r = row
                        .response()
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        state.selected = Some(data_idx);
                        resp.clicked = Some(data_idx);
                    }
                    if r.double_clicked() {
                        resp.double_clicked = Some(data_idx);
                    }
                    r.context_menu(|ui| {
                        if ui.button(self.copy_row_label).clicked() {
                            copy_row = Some(data_idx);
                            ui.close();
                        }
                    });
                });
            });
        if let Some(s) = new_sort {
            state.sort = s;
        }
        if let Some(i) = copy_row {
            let tsv = self.rows[i].join("\t");
            ui.ctx().copy_text(tsv.clone());
            resp.copied = Some(tsv);
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_and_sort_order() {
        let theme = Theme::dark();
        let cols = [
            ColumnSpec::new("name", 100.0),
            ColumnSpec::new("size", 60.0).numeric(),
        ];
        let rows = vec![
            vec!["b.txt".to_string(), "10".to_string()],
            vec!["a.txt".to_string(), "9".to_string()],
            vec!["c.log".to_string(), "100".to_string()],
        ];
        let t = DataTable {
            theme: &theme,
            columns: &cols,
            rows: &rows,
            row_height: 18.0,
            show_filter: true,
            copy_row_label: "Copy Row",
        };
        let mut st = TableState::default();
        assert_eq!(t.order(&st), vec![0, 1, 2]);
        st.sort = Some((1, true));
        assert_eq!(t.order(&st), vec![1, 0, 2]); // numeric, not lexical
        st.sort = Some((0, false));
        assert_eq!(t.order(&st), vec![2, 0, 1]);
        st.filter = "txt".into();
        assert_eq!(t.order(&st), vec![0, 1]);
    }
}
