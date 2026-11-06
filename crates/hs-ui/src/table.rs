//! DataTable v1 (docs/04 §6): virtualised rows, resizable columns, click-to-sort,
//! inline filter, cell/row copy. Later milestones add multi-sort, cell viewers
//! and export.

use egui_extras::{Column, TableBuilder};

use crate::theme::Theme;

/// How narrow a column may be dragged, and the floor used when adding
/// the declared widths up.
const MIN_COLUMN: f32 = 40.0;

/// One data row's height. Every table in the app uses this, so a row is
/// the same to read and the same to hit wherever it is drawn (CR-0028).
pub const ROW_HEIGHT: f32 = 26.0;

/// The header row's height: taller than a data row, which is part of
/// what says it is a header, with air above and below the text (CR-0046).
pub const HEADER_HEIGHT: f32 = 34.0;

/// Header text, larger than a cell's and always bold (CR-0046).
pub const HEADER_TEXT: f32 = 14.5;

/// How far a header's title sits in from its column's left edge, and a
/// left-aligned cell's text with it, so the two start at the same x in
/// every table.
pub const HEADER_INDENT: f32 = 8.0;

/// Every cell lays its content out from the left, centred on the row's
/// height, so text sits in the middle of its row in every table (CR-0046).
pub fn cell_layout() -> egui::Layout {
    egui::Layout::left_to_right(egui::Align::Center)
}

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
    /// One sentence-case line drawn under the header when there are no
    /// rows, so an empty table says so (CR-0061). `None` for a caller
    /// that shows its empty state elsewhere.
    pub empty_hint: Option<&'a str>,
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
            crate::form::centred_row(ui, |ui| {
                ui.label(crate::icons::SEARCH);
                ui.add(
                    crate::form::line_edit(&mut state.filter)
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
            // Air between the filter and the header band, in every
            // activity's table at once (CR-0031).
            ui.add_space(crate::form::PANE_GAP);
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
                let band = begin_header_band(ui);
                let mut builder = TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(cell_layout())
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
                    .header(HEADER_HEIGHT, |mut header| {
                        for (i, c) in self.columns.iter().enumerate() {
                            header.col(|ui| {
                                let arrow = match state.sort {
                                    Some((col, true)) if col == i => " ▲",
                                    Some((col, false)) if col == i => " ▼",
                                    _ => "",
                                };
                                ui.add_space(HEADER_INDENT);
                                if ui
                                    .add(
                                        egui::Label::new(
                                            egui::RichText::new(format!("{}{arrow}", c.name))
                                                .strong()
                                                .size(HEADER_TEXT),
                                        )
                                        .selectable(false)
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
                        body.rows(self.row_height.max(ROW_HEIGHT), order.len(), |mut row| {
                            let data_idx = order[row.index()];
                            row.set_selected(state.selected == Some(data_idx));
                            for (ci, c) in self.columns.iter().enumerate() {
                                row.col(|ui| {
                                    if !c.numeric {
                                        ui.add_space(HEADER_INDENT);
                                    }
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
                end_header_band(ui, self.theme, band);
            });
        if self.rows.is_empty()
            && let Some(text) = self.empty_hint
        {
            draw_empty_hint(ui, self.theme, text);
        }
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

/// The header row's background, reserved before the table is laid out
/// and painted once it is: one band the full width of the table's row,
/// behind every header cell and the gaps between them, so it never
/// stops short at the last column (owner, 2026-11-06).
pub struct HeaderBand {
    idx: egui::layers::ShapeIdx,
    left: f32,
    right: f32,
    top: f32,
}

/// Reserve the header band. Call before `TableBuilder::new`, and
/// [`end_header_band`] after the table.
pub fn begin_header_band(ui: &egui::Ui) -> HeaderBand {
    let r = ui.available_rect_before_wrap();
    HeaderBand {
        idx: ui.painter().add(egui::Shape::Noop),
        left: r.left(),
        right: r.right().max(ui.clip_rect().right()),
        top: ui.cursor().top(),
    }
}

/// The header's fill: the accent, light enough to keep the bold titles
/// readable, so the header row stands apart from the data cells.
pub fn header_fill(theme: &Theme) -> egui::Color32 {
    theme.accent.gamma_multiply(0.22)
}

/// Paint the band reserved by [`begin_header_band`]: the accent fill
/// across the full row, with an accent rule under it.
pub fn end_header_band(ui: &egui::Ui, theme: &Theme, band: HeaderBand) {
    let rect = egui::Rect::from_min_max(
        egui::pos2(band.left, band.top),
        egui::pos2(band.right, band.top + HEADER_HEIGHT),
    );
    ui.painter().set(
        band.idx,
        egui::Shape::rect_filled(rect, 0.0, header_fill(theme)),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, theme.accent.gamma_multiply(0.6)),
    );
}

/// The muted line an empty table shows under its header, indented as
/// the header's titles are.
fn draw_empty_hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(HEADER_INDENT);
        crate::form::hint(ui, theme, text);
    });
}

/// One column of an [`edit_table`]: its header and its narrowest width.
/// Spare width is shared over the columns in proportion to it; a column
/// with `width` 0 (row actions) keeps its content's width.
pub struct EditColumn<'a> {
    pub name: &'a str,
    pub width: f32,
}

impl<'a> EditColumn<'a> {
    pub fn new(name: &'a str, width: f32) -> Self {
        Self { name, width }
    }
}

/// How tall an [`edit_table`] row is: a field's height and air above and
/// below it, so the inputs in a row sit in its middle with room between
/// rows (CR-0061).
pub fn edit_row_height(ui: &egui::Ui) -> f32 {
    crate::form::field_height(ui) + 8.0
}

/// A table whose cells are inputs - a form laid out in rows (a table's
/// columns, a list of rules). It follows the table standard every
/// `DataTable` does: the header band, bold header text at [`HEADER_TEXT`]
/// on a [`HEADER_HEIGHT`] row, striped rows, every cell laid out from the
/// left and centred on the row ([`cell_layout`]); and every control in a
/// cell - field, dropdown, checkbox, button - is a field's height, so the
/// inputs of a row line up (CR-0061). `cell(ui, row, column)` draws one
/// cell. With no rows, `empty_hint` is drawn under the header.
pub fn edit_table(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    columns: &[EditColumn<'_>],
    rows: usize,
    empty_hint: Option<&str>,
    mut cell: impl FnMut(&mut egui::Ui, usize, usize),
) {
    let field_h = crate::form::field_height(ui);
    let row_h = edit_row_height(ui);
    let avail = ui.available_width();
    let spacing = ui.spacing().item_spacing.x * columns.len() as f32;
    let band = begin_header_band(ui);
    let mut builder = TableBuilder::new(ui)
        .id_salt(id)
        .striped(true)
        .resizable(false)
        .vscroll(false)
        .cell_layout(cell_layout());
    // A width is the column's floor; the spare width of the pane is
    // shared out over the columns in proportion, so a field shows its
    // value instead of a sliver of it (CR-0061). A width of 0 marks a
    // column (row actions) that keeps its content's width.
    let floor: f32 = columns.iter().map(|c| c.width).sum();
    let grow = if floor > 0.0 {
        ((avail - spacing - 90.0) / floor).max(1.0)
    } else {
        1.0
    };
    for c in columns {
        builder = builder.column(if c.width > 0.0 {
            Column::exact((c.width * grow).floor())
        } else {
            Column::auto().at_least(MIN_COLUMN)
        });
    }
    builder
        .header(HEADER_HEIGHT, |mut header| {
            for c in columns {
                header.col(|ui| {
                    ui.add_space(HEADER_INDENT);
                    ui.add(
                        egui::Label::new(egui::RichText::new(c.name).strong().size(HEADER_TEXT))
                            .selectable(false),
                    );
                });
            }
        })
        .body(|body| {
            body.rows(row_h, rows, |mut row| {
                let r = row.index();
                for c in 0..columns.len() {
                    row.col(|ui| {
                        ui.spacing_mut().interact_size.y = field_h;
                        // The field's edge under the header's title.
                        ui.add_space(HEADER_INDENT);
                        cell(ui, r, c);
                    });
                }
            });
        });
    end_header_band(ui, theme, band);
    if rows == 0
        && let Some(text) = empty_hint
    {
        draw_empty_hint(ui, theme, text);
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
            empty_hint: None,
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
