//! A table's header is a band, not a first row in bold, and an
//! item-and-value list is a two-column table with no header at all
//! (CR-0028). Headless, with egui_kittest.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;

#[test]
fn a_tables_header_stands_a_band_above_its_first_row() {
    let theme = Theme::dark();
    let cols = [
        ColumnSpec::new("Name", 100.0),
        ColumnSpec::new("Size", 60.0),
    ];
    let rows = vec![vec!["a.txt".to_string(), "9".to_string()]];
    let mut state = TableState::default();
    let mut harness = Harness::new_ui(|ui| {
        DataTable {
            theme: &theme,
            columns: &cols,
            rows: &rows,
            row_height: hs_ui::table::ROW_HEIGHT,
            show_filter: false,
            copy_row_label: "Copy Row",
            built_in_menu: true,
            row_icons: None,
            empty_hint: None,
        }
        .show(ui, &mut state);
    });
    harness.run();

    let header = harness.get_by_label("Name").rect();
    let cell = harness.get_by_label("a.txt").rect();
    assert!(
        cell.top() - header.top() >= hs_ui::table::HEADER_HEIGHT - 1.0,
        "the first row starts a header's height below the column name \
         (header {header:?}, cell {cell:?})"
    );
}

#[test]
fn an_item_and_value_list_is_two_columns_and_no_header() {
    let theme = Theme::dark();
    let rows = [("Type", "Bug"), ("Author", "Redmine Admin")];
    let mut harness = Harness::new_ui(|ui| {
        hs_ui::form::detail_table(ui, &theme, "detail", rows.iter().copied());
    });
    harness.run();

    let name = harness.get_by_label("Type").rect();
    let value = harness.get_by_label("Bug").rect();
    assert!(
        value.left() > name.left(),
        "the value sits in a column beside its name ({name:?} then {value:?})"
    );
    let next = harness.get_by_label("Author").rect();
    assert!(
        (next.left() - name.left()).abs() < 1.0,
        "the names line up in one column ({name:?} then {next:?})"
    );
    assert!(
        next.top() - name.top() >= hs_ui::table::ROW_HEIGHT - 1.0,
        "each pair gets a table row's height ({name:?} then {next:?})"
    );
}
