//! A sidebar's header carries one title and labelled actions under it,
//! and the list starts below a rule (CR-0030). Headless, with
//! egui_kittest.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use hs_ui::form::{SideAction, sidebar_head};
use hs_ui::theme::Theme;
use std::cell::Cell;

#[test]
fn a_sidebar_header_has_one_title_and_labelled_actions_under_it() {
    let theme = Theme::dark();
    let clicked = Cell::new(None);
    let list_top = Cell::new(0.0f32);
    let mut harness = Harness::new_ui(|ui| {
        let r = sidebar_head(
            ui,
            &theme,
            "Brokers",
            &[
                SideAction {
                    icon: "+",
                    label: "Add Broker",
                    tip: "",
                },
                SideAction {
                    icon: "↓",
                    label: "Import",
                    tip: "",
                },
            ],
        );
        if r.is_some() {
            clicked.set(r);
        }
        list_top.set(ui.next_widget_position().y);
    });
    harness.run();

    // The words are on the buttons, not only in a tooltip.
    let add = harness.get_by_label_contains("Add Broker").rect();
    let import = harness.get_by_label_contains("Import").rect();
    let title = harness.get_by_label("Brokers").rect();
    assert_eq!(
        harness.query_all_by_label("Brokers").count(),
        1,
        "the title appears once"
    );
    assert!(
        add.top() > title.bottom(),
        "the actions sit under the title"
    );
    assert!(
        (import.center().y - add.center().y).abs() < 2.0,
        "both actions share one row"
    );
    assert!(
        list_top.get() > add.bottom(),
        "the list starts below the action row and its rule"
    );

    harness.get_by_label_contains("Import").click();
    harness.run();
    assert_eq!(
        clicked.get(),
        Some(1),
        "the second action reports its index"
    );
}
