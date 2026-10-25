//! The thin environment stripe under the tab strip (docs/04 §5).

use egui::{Color32, CornerRadius, Vec2};

use crate::theme::Theme;

pub struct EnvBanner<'a> {
    pub theme: &'a Theme,
    pub name: Option<&'a str>,
    pub colour: Color32,
    pub protected: bool,
    pub none_label: &'a str,
    pub protected_label: &'a str,
}

impl EnvBanner<'_> {
    pub fn show(&self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), egui::Sense::hover());
        let p = ui.painter_at(rect);
        let (fill, label_col) = match self.name {
            Some(_) => (
                self.colour
                    .linear_multiply(if self.theme.dark { 0.25 } else { 0.18 }),
                self.theme.text,
            ),
            None => (self.theme.surface, self.theme.text_muted),
        };
        p.rect_filled(rect, CornerRadius::ZERO, fill);
        p.rect_filled(
            egui::Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
            CornerRadius::ZERO,
            if self.name.is_some() {
                self.colour
            } else {
                self.theme.border
            },
        );
        let mut text = self.name.unwrap_or(self.none_label).to_string();
        if self.protected {
            text = format!(
                "{} {}  ·  {}",
                crate::icons::LOCK,
                text,
                self.protected_label
            );
        }
        p.text(
            egui::pos2(rect.left() + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(12.0),
            label_col,
        );
    }
}

/// The 2 px frame around the main area for protected environments.
pub fn protected_frame(ui: &egui::Ui, rect: egui::Rect, colour: Color32) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        egui::Stroke::new(2.0, colour),
        egui::StrokeKind::Inside,
    );
}
