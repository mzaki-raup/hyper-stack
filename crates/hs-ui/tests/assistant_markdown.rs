//! The assistant's Markdown rendering in a narrow panel (no GPU, no
//! window). A table used to be laid out at an unbounded width, because a
//! `ScrollArea::horizontal` leaves `available_width` unbounded and the
//! wrapping cells inside it therefore never wrapped: the table ran off
//! the side of the panel and only its left half was readable (owner's
//! report, 2026-10-23).

use egui_kittest::Harness;
use hs_ui::theme::Theme;

/// The assistant column is 370 px at its narrowest, so that is the width
/// a table has to be readable in.
const PANEL: f32 = 370.0;

/// Draw the Markdown at `width` and report the rectangle it used. The
/// closure the harness takes is `'static`, so the measurement comes back
/// through shared state rather than through a captured local.
fn draw(width: f32, text: &str) -> egui::Rect {
    use std::sync::{Arc, Mutex};
    let theme = Theme::dark();
    let text = text.to_string();
    let used: Arc<Mutex<egui::Rect>> = Arc::new(Mutex::new(egui::Rect::NOTHING));
    let sink = used.clone();
    let mut harness = Harness::new_ui(move |ui| {
        ui.set_width(width);
        let before = ui.cursor().min;
        hs_ui::ai_assistant::markdown_text(ui, &text, &theme);
        let after = ui.min_rect();
        *sink.lock().expect("measurement") = egui::Rect::from_min_max(before, after.max);
    });
    harness.run();
    *used.lock().expect("measurement")
}

const TABLE: &str = "\
Here is what each level means.

| Level | When it is used | What to do about it |
|---|---|---|
| ERROR | the request failed outright and the caller saw it | look now |
| WARN | something recovered but should not have happened | look today |
| INFO | the ordinary life of the service, start and stop | nothing |
";

#[test]
fn a_table_stays_inside_a_narrow_assistant_panel() {
    let used = draw(PANEL, TABLE);
    assert!(
        used.width() <= PANEL + 1.0,
        "the table drew {} px wide in a {PANEL} px panel",
        used.width()
    );
    // It is still tall enough to be a table rather than a clipped line:
    // a header and three rows, with the cells wrapped onto more lines.
    assert!(
        used.height() > 60.0,
        "the table collapsed to {} px tall",
        used.height()
    );
}

#[test]
fn a_wider_panel_gives_the_table_more_room_rather_than_the_same_cut() {
    let narrow = draw(PANEL, TABLE);
    let wide = draw(900.0, TABLE);
    // The same table in a wider panel needs fewer lines per cell, so it
    // is shorter. If the layout ignored the panel width both would be
    // identical, which is the bug this guards.
    assert!(
        wide.height() < narrow.height(),
        "narrow {} px tall, wide {} px tall: the width is being ignored",
        narrow.height(),
        wide.height()
    );
}

#[test]
fn ordinary_prose_still_wraps_to_the_panel() {
    let used = draw(
        PANEL,
        "The service refused the connection because the certificate had \
         expired three days ago, and nothing had renewed it since.",
    );
    assert!(used.width() <= PANEL + 1.0);
}
