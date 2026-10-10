//! Small pills: counts, environment names, "beta".

use egui::{Color32, CornerRadius, Vec2};

pub fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, text_colour: Color32) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), text_colour);
    let size = Vec2::new(galley.size().x + 12.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter().galley(
        egui::pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0),
        galley,
        text_colour,
    );
    resp
}

pub fn count_badge(ui: &mut egui::Ui, n: usize, fill: Color32) -> egui::Response {
    badge(ui, &n.to_string(), fill, Color32::WHITE)
}
