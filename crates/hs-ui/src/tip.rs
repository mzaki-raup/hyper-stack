//! Tooltips **above the mouse cursor**, never under it, where the cursor
//! covers the first words. Use `response.tip(text)` instead of
//! `on_hover_text`, and `tip_disabled` instead of `on_disabled_hover_text`;
//! owner's rule, 2026-10-11, amended 2026-10-26.

use egui::{RectAlign, Response, Tooltip, WidgetText};

/// How far above the pointer the tooltip's bottom edge sits.
///
/// A cursor is drawn from its hotspot: the arrow's hotspot is its tip, so
/// anything above it is already clear, but the I-beam and the hand are
/// drawn around theirs and reach roughly ten points up. Sixteen clears
/// every standard Windows cursor with a little air left, which is what
/// "do not overlap the cursor" has to mean in practice - the cursor is
/// not a rectangle egui can measure.
const GAP: f32 = 16.0;

/// The alignments to try, in order: above and to the right of the
/// pointer, then above and to the left when the window's right edge is
/// close.
///
/// **Below is deliberately not an option.** It used to be the last
/// resort, which meant the one row where there is no room above - the
/// title bar's own icons, a few pixels from the top of the window -
/// fell back to a tooltip under the cursor, the single place the rule
/// was meant to fix (owner's report, 2026-10-26, after asking twice).
/// With no alternative that fits, egui clamps the tooltip into view
/// instead, which keeps it above the pointer and merely closer to the
/// top edge.
const ALIGNS: [RectAlign; 2] = [RectAlign::TOP_START, RectAlign::TOP_END];

pub trait TipExt {
    /// Show `text` above the pointer.
    fn tip(self, text: impl Into<WidgetText>) -> Self;

    /// The same, for a widget that is **disabled** - where the tooltip is
    /// usually the only thing that says why.
    fn tip_disabled(self, text: impl Into<WidgetText>) -> Self;
}

/// Anchor to the pointer rather than to the widget, and put the tooltip
/// above it.
///
/// The widget is the wrong anchor for a big one: above a tall list row or
/// a wide tab, the tooltip can land a long way from what the user is
/// actually pointing at. `at_pointer` anchors to a zero-size rect at the
/// cursor, so `TOP_START` puts the tooltip's bottom-left corner there and
/// the gap lifts it clear.
fn show(tooltip: Tooltip<'_>, text: WidgetText) {
    let mut tooltip = tooltip.at_pointer().gap(GAP);
    tooltip.popup = tooltip
        .popup
        .align(RectAlign::TOP_START)
        .align_alternatives(&ALIGNS);
    tooltip.show(|ui| {
        ui.set_max_width(ui.spacing().tooltip_width);
        ui.add(egui::Label::new(text));
    });
}

impl TipExt for Response {
    fn tip(self, text: impl Into<WidgetText>) -> Self {
        show(Tooltip::for_enabled(&self), text.into());
        self
    }

    fn tip_disabled(self, text: impl Into<WidgetText>) -> Self {
        show(Tooltip::for_disabled(&self), text.into());
        self
    }
}
