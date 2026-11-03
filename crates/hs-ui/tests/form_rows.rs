//! A form row's label and its control have to sit on the same line
//! (CR-0026). Measured headlessly with egui_kittest — no image snapshots,
//! the runner has no graphics device.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::cell::Cell;

#[test]
fn a_control_in_a_form_row_is_a_fields_height_tall() {
    let field = Cell::new(0.0f32);
    let in_grid = Cell::new(0.0f32);
    let loose = Cell::new(0.0f32);
    let mut on = false;
    {
        let mut harness = Harness::new_ui(|ui| {
            field.set(hs_ui::form::field_height(ui));
            loose.set(ui.checkbox(&mut on, "Loose").rect.height());
            hs_ui::form::field_grid(ui, "cr-0026", |ui| {
                hs_ui::form::label(ui, "Dry Run");
                in_grid.set(ui.checkbox(&mut on, "").rect.height());
                ui.end_row();
            });
        });
        harness.run();
    }

    let h = field.get();
    assert!(h > 20.0, "a field should be comfortably tall, got {h}");
    assert_eq!(
        in_grid.get(),
        h,
        "a checkbox in a form row gets the field height, so it centres on the label"
    );
    assert!(
        loose.get() < h,
        "outside a form row the control keeps egui's own height ({} < {h})",
        loose.get()
    );
}

#[test]
fn a_toggle_button_keeps_its_rect_when_hovered() {
    let size = Cell::new(hs_ui::egui::Vec2::ZERO);
    let mut on = false;
    let mut harness = Harness::new_ui(|ui| {
        size.set(hs_ui::form::toggle_button(ui, &mut on, "Aa").rect.size());
    });
    harness.run();
    let cold = size.get();
    harness.get_by_label("Aa").hover();
    harness.run();
    assert_eq!(
        size.get(),
        cold,
        "a framed toggle has the same box hovered as it does cold"
    );
}
