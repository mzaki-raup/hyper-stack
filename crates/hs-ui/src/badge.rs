//! Small pills: counts, environment names, "beta".

use egui::{Color32, CornerRadius, Vec2};

pub fn badge(ui: &mut egui::Ui, text: &str, fill: Color32, text_colour: Color32) -> egui::Response {
    let resp = pill(ui, text, egui::Sense::hover(), |_| fill, text_colour);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    resp
}

/// A badge that acts as a button: brighter on hover, pointer cursor, and an
/// AccessKit button node so screen readers and `egui_kittest` can click it.
pub fn badge_button(
    ui: &mut egui::Ui,
    text: &str,
    fill: Color32,
    text_colour: Color32,
) -> egui::Response {
    let resp = pill(
        ui,
        text,
        egui::Sense::click(),
        |r| {
            if r.hovered() {
                fill.gamma_multiply(1.6)
            } else {
                fill
            }
        },
        text_colour,
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, text));
    resp
}

fn pill(
    ui: &mut egui::Ui,
    text: &str,
    sense: egui::Sense,
    fill: impl FnOnce(&egui::Response) -> Color32,
    text_colour: Color32,
) -> egui::Response {
    let font = egui::FontId::proportional(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), font, text_colour);
    let size = Vec2::new(galley.size().x + 12.0, 18.0);
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(9), fill(&resp));
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
