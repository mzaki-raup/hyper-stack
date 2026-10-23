//! `DataTable` in a pane narrower than its columns (no GPU, no window).
//!
//! `egui_extras::TableBuilder` only ever *shares out* the width it is
//! handed, and it has a `vscroll` but no horizontal equivalent. A table
//! wider than its pane therefore squeezed its columns, clipped them, and
//! left the ones past the edge unreachable - with nothing to scroll. It
//! was a known fault against the S3 object list and the SFTP pane, and
//! the Container activity's panes are wide tables, so it had to be fixed
//! in the shared table rather than worked around twice more
//! (docs/tools/containers.md §10.4).

use egui_kittest::Harness;
use hs_ui::table::{ColumnSpec, DataTable, TableState};
use hs_ui::theme::Theme;

/// Eight columns of 160 px cannot fit a 400 px pane by any sharing out.
fn wide_columns() -> Vec<ColumnSpec> {
    [
        "name", "image", "state", "ports", "created", "cpu", "memory", "id",
    ]
    .into_iter()
    .map(|n| ColumnSpec::new(n, 160.0))
    .collect()
}

fn rows(n: usize, cols: usize) -> Vec<Vec<String>> {
    (0..n)
        .map(|r| (0..cols).map(|c| format!("r{r}c{c}")).collect())
        .collect()
}

/// Draw the table in a pane `width` wide and report the content width it
/// asked for. The harness closure is `'static`, so the measurement comes
/// back through shared state.
fn content_width(pane: f32, columns: Vec<ColumnSpec>) -> f32 {
    use std::sync::{Arc, Mutex};
    let data = rows(20, columns.len());
    let measured: Arc<Mutex<f32>> = Arc::new(Mutex::new(0.0));
    let sink = measured.clone();
    let mut harness = Harness::new_ui(move |ui| {
        let theme = Theme::dark();
        ui.set_width(pane);
        ui.set_height(300.0);
        let table = DataTable {
            theme: &theme,
            columns: &columns,
            rows: &data,
            row_height: 18.0,
            show_filter: false,
            copy_row_label: "Copy Row",
            built_in_menu: true,
            row_icons: None,
        };
        // The width the inner `Ui` is bounded to, which is what the
        // scroll area scrolls over. Measuring `ui.min_rect()` from out
        // here would only ever report the pane: a ScrollArea clamps
        // itself to the width it is given, which is exactly why the
        // clipping was invisible for so long.
        *sink.lock().expect("measurement") = table.content_width(ui);
        let mut state = TableState::default();
        table.show(ui, &mut state);
    });
    harness.run();
    *measured.lock().expect("measurement")
}

/// The regression itself. Eight 160 px columns want about 1 300 px; in a
/// 400 px pane the old table squeezed them to about 50 px each and
/// clipped, so the measured content was the pane width and there was
/// nothing to scroll. It must now ask for its real width.
#[test]
fn a_table_wider_than_its_pane_has_something_to_scroll() {
    const PANE: f32 = 400.0;
    let got = content_width(PANE, wide_columns());
    assert!(
        got > PANE * 1.5,
        "the table fitted itself into the pane instead of scrolling: \
         asked for {got} px in a {PANE} px pane"
    );
}

/// The columns must keep their declared widths rather than being shared
/// down to nothing, or the text is clipped even while scrolling.
#[test]
fn the_columns_keep_their_declared_width() {
    let columns = wide_columns();
    let declared: f32 = columns.iter().map(|c| c.width).sum();
    let got = content_width(400.0, columns);
    assert!(
        got >= declared,
        "columns were squeezed: {got} px for {declared} px of columns"
    );
}

/// A table that fits must still fill its pane. Two narrow columns in a
/// wide pane should not huddle on the left - that is what the trailing
/// remainder column is for, and it must not become a scroll range.
#[test]
fn a_table_that_fits_fills_the_pane_without_scrolling() {
    const PANE: f32 = 900.0;
    let columns = vec![
        ColumnSpec::new("key", 120.0),
        ColumnSpec::new("value", 160.0),
    ];
    let got = content_width(PANE, columns);
    assert!(
        (got - PANE).abs() < 40.0,
        "a table that fits should fill the pane, not scroll: {got} px in \
         a {PANE} px pane"
    );
}

/// The ordering logic is independent of the scrolling, and must survive
/// being wrapped in a scroll area.
#[test]
fn filter_and_sort_still_work_inside_the_scroll_area() {
    let theme = Theme::dark();
    let cols = [
        ColumnSpec::new("name", 100.0),
        ColumnSpec::new("size", 60.0).numeric(),
    ];
    let data = vec![
        vec!["b.txt".to_string(), "10".to_string()],
        vec!["a.txt".to_string(), "9".to_string()],
        vec!["c.log".to_string(), "100".to_string()],
    ];
    let table = DataTable {
        theme: &theme,
        columns: &cols,
        rows: &data,
        row_height: 18.0,
        show_filter: true,
        copy_row_label: "Copy Row",
        built_in_menu: true,
        row_icons: None,
    };
    let mut state = TableState::default();
    assert_eq!(table.order(&state), vec![0, 1, 2]);
    state.sort = Some((1, true));
    assert_eq!(table.order(&state), vec![1, 0, 2], "numeric, not lexical");
    state.filter = "txt".into();
    assert_eq!(table.order(&state), vec![1, 0]);
}
