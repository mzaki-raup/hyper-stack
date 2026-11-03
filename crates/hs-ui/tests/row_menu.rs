//! A connection row's context menu offers Remove From List under Edit,
//! and reports the choice to the caller, which asks before anything goes
//! (FEAT-0094). Measured headlessly with egui_kittest.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use std::cell::Cell;

#[test]
fn remove_from_list_sits_under_edit_and_reports_the_choice() {
    let removed = Cell::new(false);
    let edited = Cell::new(false);
    let theme = hs_ui::theme::Theme::dark();
    let mut harness = Harness::new_ui(|ui| {
        let row = hs_ui::form::connection_row(
            ui,
            &theme,
            false,
            "",
            "web-01",
            None,
            false,
            hs_ui::form::RowMenuLabels {
                pin: "Pin to Home",
                unpin: "Unpin From Home",
                edit: "Edit This Connection",
                remove: "Remove From List",
            },
        );
        removed.set(removed.get() || row.remove);
        edited.set(edited.get() || row.edit);
    });
    harness.run();
    harness.get_by_label("web-01").click_secondary();
    harness.run();

    let edit = harness.get_by_label("Edit This Connection").rect();
    let remove = harness.get_by_label("Remove From List").rect();
    assert!(remove.top() > edit.top(), "Remove From List is below Edit");

    harness.get_by_label("Remove From List").click();
    harness.run();
    assert!(removed.get(), "the row reported Remove From List");
    assert!(!edited.get(), "and nothing else");
}
