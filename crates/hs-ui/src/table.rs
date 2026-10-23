//! DataTable v1 (docs/04 §6): virtualised rows, resizable columns, click-to-sort,
//! inline filter, cell/row copy. Later milestones add multi-sort, cell viewers
//! and export.

use egui_extras::{Column, TableBuilder};

use crate::theme::Theme;

/// How narrow a column may be dragged, and the floor used when adding
/// the declared widths up.
const MIN_COLUMN: f32 = 40.0;

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
    /// Right-clicked row, for a caller that draws its own menu
    /// (`built_in_menu: false`).
    pub secondary: Option<usize>,
}

pub struct DataTable<'a> {
    pub theme: &'a Theme,
    pub columns: &'a [ColumnSpec],
    pub rows: &'a [Vec<String>],
    pub row_height: f32,
    pub show_filter: bool,
    /// Context-menu label for copying a row (Title Case, from the locale).
    pub copy_row_label: &'a str,
    /// Draw the built-in Copy Row menu. Set false when the caller has a
    /// menu of its own to show for `TableResponse::secondary`.
    pub built_in_menu: bool,
    /// An icon for the first column of each row, in `rows` order. A file
    /// listing reads far faster with its file-type icons than without.
    pub row_icons: Option<&'a [egui::ImageSource<'static>]>,
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

    /// What the data columns add up to, with the spacing between them.
    /// Never narrower than the pane: a table with two columns in a wide
    /// pane should fill it rather than huddle on the left, which is what
    /// the trailing remainder column is for.
    ///
    /// A column the user has dragged wider is not tracked here -
    /// `egui_extras` keeps that in its own state - so the scroll range
    /// follows the declared widths. `tool-db::grid` reads the real widths
    /// back through `TableBody::widths()` where that matters; this table
    /// does not need it, and guessing would be worse.
    pub fn content_width(&self, ui: &egui::Ui) -> f32 {
        let spacing = ui.spacing().item_spacing.x;
        let natural: f32 = self
            .columns
            .iter()
            .map(|c| c.width.max(MIN_COLUMN) + spacing)
            .sum();
        natural.max(ui.available_width())
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
        let mut new_sort = None;
        let mut copy_row = None;
        // The width the columns actually want. `TableBuilder` only ever
        // *shares out* the width it is handed, so a table asked to fit a
        // narrow pane shrinks its columns, clips them, and leaves nothing
        // to scroll to - the columns past the edge become unreachable.
        // Working the total out here, from the same numbers the columns
        // are built with, is what makes scrolling possible at all.
        let content_width = self.content_width(ui);
        let scroll_id = ui.next_auto_id();
        ui.skip_ahead_auto_ids(1);
        egui::ScrollArea::horizontal()
            .id_salt(scroll_id)
            .show(ui, |ui| {
                // Both bounds. The table reads `available_rect_before_wrap` to
                // share out its width, and inside a horizontal ScrollArea that is
                // unbounded, which would make a trailing remainder column
                // infinitely wide.
                ui.set_min_width(content_width);
                ui.set_max_width(content_width);
                let mut builder = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .sense(egui::Sense::click());
                for c in self.columns {
                    builder =
                        builder.column(Column::initial(c.width).at_least(MIN_COLUMN).clip(true));
                }
                // The remainder only soaks up slack when the columns do not fill
                // the pane; it is not counted in `content_width`, or the content
                // would wedge at whatever width it first had.
                builder = builder.column(Column::remainder());
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
                                            egui::RichText::new(format!("{}{arrow}", c.name))
                                                .strong(),
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
                                                    egui::Label::new(
                                                        egui::RichText::new(text).monospace(),
                                                    )
                                                    .truncate()
                                                    .selectable(false),
                                                );
                                            },
                                        );
                                    } else if ci == 0
                                        && let Some(icon) =
                                            self.row_icons.and_then(|icons| icons.get(data_idx))
                                    {
                                        // The icon and the name read as one thing.
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            ui.add(
                                                egui::Image::new(icon.clone())
                                                    .fit_to_exact_size(egui::vec2(16.0, 16.0)),
                                            );
                                            ui.add(
                                                egui::Label::new(text).truncate().selectable(false),
                                            );
                                        });
                                    } else {
                                        // Not selectable: a selectable label takes the
                                        // click, so the row would not select or open.
                                        ui.add(egui::Label::new(text).truncate().selectable(false));
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
                            if r.secondary_clicked() {
                                resp.secondary = Some(data_idx);
                            }
                            if self.built_in_menu {
                                r.context_menu(|ui| {
                                    if ui.button(self.copy_row_label).clicked() {
                                        copy_row = Some(data_idx);
                                        ui.close();
                                    }
                                });
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
            built_in_menu: true,
            row_icons: None,
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
