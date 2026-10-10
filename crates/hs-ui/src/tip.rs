//! Tooltips above the widget (never under the pointer, where the cursor
//! hides the first words). Use `response.tip(text)` instead of
//! `on_hover_text`; owner's rule, 2026-10-11.

use egui::{RectAlign, Response, Tooltip, WidgetText};

pub trait TipExt {
    /// Show `text` above the hovered widget (falls back to below when there is
    /// no room above).
    fn tip(self, text: impl Into<WidgetText>) -> Self;
}

impl TipExt for Response {
    fn tip(self, text: impl Into<WidgetText>) -> Self {
        let text = text.into();
        let mut tooltip = Tooltip::for_enabled(&self).gap(8.0);
        tooltip.popup = tooltip
            .popup
            .align(RectAlign::TOP_START)
            .align_alternatives(&[
                RectAlign::TOP_START,
                RectAlign::TOP_END,
                RectAlign::BOTTOM_START,
                RectAlign::BOTTOM_END,
            ]);
        tooltip.show(|ui| {
            ui.set_max_width(ui.spacing().tooltip_width);
            ui.add(egui::Label::new(text));
        });
        self
    }
}
