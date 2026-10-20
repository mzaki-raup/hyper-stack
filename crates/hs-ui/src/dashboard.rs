//! An activity's home tab: what it has open, as a grid of cards.
//!
//! One widget serves every activity, because the question is the same in
//! all of them — *what have I got open here, and take me to it*. A tool
//! supplies the summary above the grid, since only it knows what counts as
//! a summary of itself. Owner's requirement, 2026-10-17.

use hs_core::open_tabs::TabInfo;

use crate::theme::Theme;

/// A card is wide enough for a legible thumbnail and a two-line label.
const CARD_WIDTH: f32 = 220.0;
const THUMB_HEIGHT: f32 = 124.0;

/// Labels the grid needs, so nothing here reaches for Fluent itself.
pub struct DashboardLabels<'a> {
    pub open_heading: &'a str,
    /// Shown when the activity has nothing open but itself.
    pub nothing_open: &'a str,
    /// Shown on a card for a tab never looked at this session.
    pub no_preview: &'a str,
}

/// Draw the grid. Returns the tab the user asked to go to.
pub fn open_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[TabInfo],
    labels: &DashboardLabels<'_>,
) -> Option<u64> {
    let mut go = None;
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(labels.open_heading)
            .strong()
            .color(theme.text_muted)
            .size(12.0),
    );
    ui.add_space(6.0);
    if tabs.is_empty() {
        crate::form::note(ui, theme, crate::icons::INFO, labels.nothing_open);
        return None;
    }
    // Work the columns out rather than letting `horizontal_wrapped` try:
    // a frame reports its size only after it is drawn, so wrapping on it
    // never happens (see the CLAUDE.md gotcha).
    let spacing = ui.spacing().item_spacing.x;
    // Leave a little room for a scrollbar or panel edge: the last card used to
    // be clipped by the right edge on a 1400 px window.
    let avail = (ui.available_width() - 32.0).max(CARD_WIDTH);
    let columns = (((avail + spacing) / (CARD_WIDTH + spacing)).floor() as usize).max(1);
    egui::Grid::new("hs-dashboard-tabs")
        .num_columns(columns)
        .spacing([spacing, spacing])
        .show(ui, |ui| {
            for (i, t) in tabs.iter().enumerate() {
                if card(ui, theme, t, labels) {
                    go = Some(t.id);
                }
                if (i + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    go
}

/// One tab: its last picture, its name, and what it is bound to.
fn card(ui: &mut egui::Ui, theme: &Theme, t: &TabInfo, labels: &DashboardLabels<'_>) -> bool {
    let mut clicked = false;
    let height = THUMB_HEIGHT + 46.0;
    let (rect, resp) =
        ui.allocate_exact_size(egui::Vec2::new(CARD_WIDTH, height), egui::Sense::click());
    let hovered = resp.hovered();
    let stroke = if t.active {
        egui::Stroke::new(1.5, theme.accent)
    } else if hovered {
        egui::Stroke::new(1.0, theme.accent.gamma_multiply(0.6))
    } else {
        egui::Stroke::new(1.0, theme.border)
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, theme.panel_bg);
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(6),
        stroke,
        egui::StrokeKind::Inside,
    );
    let thumb = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(1.0, 1.0),
        egui::Vec2::new(rect.width() - 2.0, THUMB_HEIGHT),
    );
    match &t.thumbnail {
        Some(uri) => {
            egui::Image::from_uri(uri.clone())
                .maintain_aspect_ratio(true)
                .fit_to_exact_size(thumb.size())
                .paint_at(ui, thumb);
        }
        None => {
            // Never looked at this session, so there is no picture to show.
            // Saying so is better than an empty frame that reads as broken.
            painter.rect_filled(thumb, 6.0, theme.surface_hover);
            painter.text(
                thumb.center(),
                egui::Align2::CENTER_CENTER,
                labels.no_preview,
                egui::TextStyle::Small.resolve(ui.style()),
                theme.text_muted,
            );
        }
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    painter.text(
        egui::pos2(rect.left() + 8.0, thumb.bottom() + 14.0),
        egui::Align2::LEFT_CENTER,
        elide(ui, &t.title, &font, rect.width() - 16.0),
        font,
        theme.text,
    );
    if !t.subtitle.is_empty() {
        painter.text(
            egui::pos2(rect.left() + 8.0, thumb.bottom() + 32.0),
            egui::Align2::LEFT_CENTER,
            elide(ui, &t.subtitle, &small, rect.width() - 16.0),
            small,
            theme.text_muted,
        );
    }
    if resp.clicked() {
        clicked = true;
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &t.title));
    clicked
}

/// Cut a label to the width it has. A painter lays nothing out itself.
fn elide(ui: &egui::Ui, text: &str, font: &egui::FontId, width: f32) -> String {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), egui::Color32::WHITE);
    if full.size().x <= width || text.is_empty() {
        return text.to_string();
    }
    let keep = ((width / full.size().x) * text.chars().count() as f32).floor() as usize;
    let mut s: String = text.chars().take(keep.saturating_sub(1).max(1)).collect();
    s.push('…');
    s
}

/// A line of the summary above the grid: a count and what it counts.
pub fn stat(ui: &mut egui::Ui, theme: &Theme, value: &str, label: &str) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(value).size(22.0).strong());
        ui.label(
            egui::RichText::new(label)
                .size(11.0)
                .color(theme.text_muted),
        );
    });
    ui.add_space(18.0);
}
