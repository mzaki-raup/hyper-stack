//! A toolbar item carries its words, and a sub-toolbar row lands under
//! the main row rather than inside it (CR-0027). Headless, with
//! egui_kittest — no image snapshots, the runner has no graphics device.

use egui_kittest::Harness;
use hs_ui::theme::Theme;
use std::cell::Cell;

#[test]
fn a_toolbar_item_carries_its_label_and_colours_the_icon() {
    let theme = Theme::dark();
    let text = Cell::new(String::new());
    let colours = Cell::new((egui::Color32::BLACK, egui::Color32::BLACK));
    let mut harness = Harness::new_ui(|ui| {
        let job = hs_ui::toolbar::item_text(ui, &theme, hs_ui::icons::ph::PLAY, "Run");
        text.set(job.text.clone());
        colours.set((job.sections[0].format.color, job.sections[1].format.color));
    });
    harness.run();
    drop(harness);

    let got = text.take();
    assert!(
        got.ends_with("Run"),
        "the label follows the icon, got {got:?}"
    );
    let (icon, label) = colours.get();
    assert_eq!(icon, theme.accent, "a toolbar icon is drawn in the accent");
    assert_eq!(label, theme.text, "its label stays the ordinary text");
}

#[test]
fn a_row_break_puts_what_follows_on_the_next_row() {
    let theme = Theme::dark();
    let first = Cell::new(egui::Rect::NOTHING);
    let second = Cell::new(egui::Rect::NOTHING);
    let mut harness = Harness::new_ui(|ui| {
        ui.horizontal_wrapped(|ui| {
            hs_ui::toolbar::strip_style(ui);
            first.set(hs_ui::toolbar::btn(ui, &theme, "", "Follow").rect);
            hs_ui::toolbar::row_break(ui, &theme);
            second.set(hs_ui::toolbar::btn(ui, &theme, "", "Time Zone").rect);
        });
    });
    harness.run();

    let (a, b) = (first.get(), second.get());
    assert!(
        b.top() >= a.bottom(),
        "the second row starts below the first ({:?} then {:?})",
        a,
        b
    );
    assert!(
        b.left() <= a.left() + 1.0,
        "and back at the row's left edge ({:?} then {:?})",
        a,
        b
    );
}
