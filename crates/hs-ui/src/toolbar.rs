//! Tool toolbar helpers. The shell lays every tool's toolbar out in a
//! wrapping row, so a narrow window pushes buttons onto a second row
//! instead of clipping them off the right edge.

/// The toolbar's hard-right group (the AI Assistant control and whatever
/// status sits beside it). A right-to-left scope only ever gets the width
/// left on the current row; when that is too little for the group, start a
/// fresh row first so the control stays reachable instead of overlapping
/// the last left-hand button or vanishing past the edge.
pub fn hard_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    const NEEDED: f32 = 170.0;
    if ui.available_width() < NEEDED && ui.cursor().left() > ui.max_rect().left() + 1.0 {
        ui.end_row();
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), add);
}
