//! Headless interaction test of the result grid with egui_kittest: a click
//! selects a cell (NULL included) and a header click sorts. (kittest's
//! synthetic clicks never form a double-click, so the cell viewer is
//! exercised in the app.)

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_ui::theme::Theme;
use tool_db::driver::{Column, Row, Value};
use tool_db::grid::{self, GridResponse, GridState, Labels};

struct State {
    columns: Vec<Column>,
    rows: Vec<Row>,
    grid: GridState,
    open: Option<(usize, usize)>,
}

fn harness<'a>() -> Harness<'a, State> {
    let col = |n: &str| Column {
        name: n.into(),
        type_name: String::new(),
    };
    let state = State {
        columns: vec![col("id"), col("status")],
        rows: vec![
            vec![Value::Int(2), Value::Text("paid".into())],
            vec![Value::Int(1), Value::Text("failed".into())],
            vec![Value::Int(3), Value::Null],
        ],
        grid: GridState::default(),
        open: None,
    };
    Harness::new_ui_state(
        |ui, s: &mut State| {
            ui.set_width(500.0);
            ui.set_height(240.0);
            let theme = Theme::dark();
            let labels = Labels {
                null: "NULL",
                copy_cell: "Copy Cell",
                copy_row: "Copy Row",
                view_cell: "View Cell",
                filter_value: "Filter by This Value",
            };
            let order = grid::order(&s.rows, &mut s.grid);
            let r: GridResponse = grid::show(
                ui,
                egui::Id::new("grid"),
                &theme,
                &s.columns,
                &s.rows,
                &mut s.grid,
                &labels,
                &order,
            );
            if r.open_cell.is_some() {
                s.open = r.open_cell;
            }
        },
        state,
    )
}

#[test]
fn click_selects_and_header_sorts() {
    let mut h = harness();
    h.run();
    h.get_by_label("failed").click();
    h.run();
    assert_eq!(h.state().grid.selected, Some((1, 1)));
    // NULL is drawn as a label too and is selectable like any cell.
    h.get_by_label("NULL").click();
    h.run();
    assert_eq!(h.state().grid.selected, Some((2, 1)));

    h.get_by_label("id").click();
    h.run();
    assert_eq!(h.state().grid.sort, Some((0, true)));
    let mut st = h.state().grid.clone();
    assert_eq!(grid::order(&h.state().rows, &mut st), vec![1, 0, 2]);
}
