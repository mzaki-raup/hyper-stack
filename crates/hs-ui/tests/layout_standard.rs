//! The layout standard (.claude/rules/ui.md, CR-0061): every text field is a
//! field's height, a dropdown in a raised row matches it, and an editable
//! table's rows are tall enough for those inputs with a header above them.
//! Measured headlessly with egui_kittest.

use egui_kittest::Harness;
use std::cell::Cell;

#[test]
fn a_line_edit_is_a_fields_height_and_a_bare_one_is_not() {
    let field = Cell::new(0.0f32);
    let line = Cell::new(0.0f32);
    let bare = Cell::new(0.0f32);
    let combo = Cell::new(0.0f32);
    let mut a = String::from("value");
    let mut b = String::from("value");
    let mut harness = Harness::new_ui(|ui| {
        field.set(hs_ui::form::field_height(ui));
        line.set(ui.add(hs_ui::form::line_edit(&mut a)).rect.height());
        bare.set(
            ui.add(hs_ui::egui::TextEdit::singleline(&mut b))
                .rect
                .height(),
        );
        hs_ui::form::centred_row(ui, |ui| {
            combo.set(
                hs_ui::egui::ComboBox::from_id_salt("cr-0061")
                    .selected_text("Auto")
                    .show_ui(ui, |_| {})
                    .response
                    .rect
                    .height(),
            );
        });
    });
    harness.run();
    let h = field.get();
    assert!(
        (line.get() - h).abs() < 0.5,
        "line_edit {} vs field {h}",
        line.get()
    );
    assert!(
        bare.get() < h - 4.0,
        "a bare field is visibly shorter ({})",
        bare.get()
    );
    assert!(
        (combo.get() - h).abs() < 0.5,
        "a raised dropdown {} vs field {h}",
        combo.get()
    );
}

/// The owner's report (2026-11-06): "main." beside the table name and
/// "Script" beside Apply sat high. A label in a centred row shares the
/// field's middle line, wrapped or not.
#[test]
fn a_label_in_a_centred_row_sits_on_the_fields_middle_line() {
    for wrapped in [false, true] {
        let label = Cell::new(0.0f32);
        let field = Cell::new(0.0f32);
        let mut v = String::from("bank");
        let mut harness = Harness::new_ui(|ui| {
            let row = |ui: &mut hs_ui::egui::Ui| {
                label.set(ui.label("main.").rect.center().y);
                field.set(ui.add(hs_ui::form::line_edit(&mut v)).rect.center().y);
            };
            if wrapped {
                hs_ui::form::centred_row_wrapped(ui, row);
            } else {
                hs_ui::form::centred_row(ui, row);
            }
        });
        harness.run();
        assert!(
            (label.get() - field.get()).abs() < 1.0,
            "wrapped {wrapped}: label centre {} vs field centre {}",
            label.get(),
            field.get()
        );
    }
}

#[test]
fn an_edit_table_row_holds_a_field_with_air_and_has_a_header() {
    let row_h = Cell::new(0.0f32);
    let field = Cell::new(0.0f32);
    let first_cell_top = Cell::new(0.0f32);
    let table_top = Cell::new(0.0f32);
    let theme = hs_ui::theme::Theme::dark();
    let mut values = [String::from("id"), String::from("name")];
    let mut harness = Harness::new_ui(|ui| {
        row_h.set(hs_ui::table::edit_row_height(ui));
        field.set(hs_ui::form::field_height(ui));
        table_top.set(ui.cursor().top());
        let cols = [
            hs_ui::table::EditColumn::new("Name", 200.0),
            hs_ui::table::EditColumn::new("", 0.0),
        ];
        hs_ui::table::edit_table(
            ui,
            &theme,
            "cr-0061-table",
            &cols,
            values.len(),
            None,
            |ui, r, c| {
                if c == 0 {
                    let rect = ui.add(hs_ui::form::line_edit(&mut values[r])).rect;
                    if r == 0 {
                        first_cell_top.set(rect.top());
                    }
                }
            },
        );
    });
    harness.run();
    assert!(row_h.get() > field.get(), "a row is taller than its field");
    assert!(
        first_cell_top.get() - table_top.get() >= hs_ui::table::HEADER_HEIGHT,
        "the first row starts below a header's height ({} from the top)",
        first_cell_top.get() - table_top.get()
    );
}
