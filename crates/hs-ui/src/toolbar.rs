//! Tool toolbar helpers. The shell lays every tool's toolbar out in a
//! wrapping row, so a narrow window pushes buttons onto a second row
//! instead of clipping them off the right edge - and the hard-right
//! group stays in the corner while that happens.

use egui::{Align, Id, Layout, Rect, UiBuilder};

/// Where the shell leaves the toolbar's full rectangle for [`hard_right`],
/// and where that function leaves the width it needs for the next frame.
const OUTER: &str = "hs-toolbar-outer";
const WIDTH: &str = "hs-toolbar-right-width";

/// What the corner is assumed to need before it has ever been measured.
/// Every tool's group is the AI Assistant button, sometimes with a short
/// status beside it.
pub const RIGHT_RESERVE: f32 = 170.0;

/// How wide the hard-right corner wants to be, for the shell to hold back
/// before it hands the row to a tool. One frame behind the truth, which
/// only matters the first time a tool's group changes width.
pub fn right_reserve(ctx: &egui::Context, tab: Id) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(tab.with(WIDTH)))
        .unwrap_or(RIGHT_RESERVE)
}

/// Tell [`hard_right`] which rectangle the whole toolbar occupies, so the
/// corner it draws in is the window's, not the narrowed row's. The shell
/// calls this before handing the row to a tool.
pub fn set_outer(ui: &mut egui::Ui, tab: Id, rect: Rect) {
    ui.data_mut(|d| d.insert_temp(tab.with(OUTER), rect));
    ui.data_mut(|d| d.insert_temp(Id::new(OUTER), tab));
}

/// The toolbar's hard-right group: the AI Assistant control and whatever
/// status sits beside it.
///
/// This is pinned to the **top right of the first row** rather than laid
/// out with everything else. A right-to-left scope only ever gets the
/// width left on the current row, so when the buttons wrapped the control
/// went down to the second row with them - and the owner's rule is that
/// the assistant is always the hard right item of a tool toolbar, which
/// means the corner, not wherever the flow happened to end
/// (owner's requirement, 2026-10-26).
///
/// The shell holds that corner back from the wrapping row (see
/// [`right_reserve`]), so nothing is drawn underneath it.
pub fn hard_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let tab = ui.data(|d| d.get_temp::<Id>(Id::new(OUTER)));
    let outer = tab.and_then(|t| ui.data(|d| d.get_temp::<Rect>(t.with(OUTER))));
    let Some((tab, outer)) = tab.zip(outer) else {
        // No shell around us (a test, or a tool drawn on its own): fall
        // back to the old behaviour rather than drawing nothing.
        ui.with_layout(Layout::right_to_left(Align::Center), add);
        return;
    };
    let height = ui.min_rect().height().max(ui.spacing().interact_size.y);
    let corner = Rect::from_min_max(
        egui::pos2(outer.left(), outer.top()),
        egui::pos2(outer.right(), outer.top() + height),
    );
    let used = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(corner)
                .layout(Layout::right_to_left(Align::Center)),
            add,
        )
        .response
        .rect
        .width();
    // Report what the corner actually took, so the next frame's row stops
    // short of it. `max` keeps a sizing pass from reporting zero.
    ui.data_mut(|d| {
        d.insert_temp(
            tab.with(WIDTH),
            (used + ui.spacing().item_spacing.x).max(ui.spacing().item_spacing.x),
        )
    });
}
