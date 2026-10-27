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

/// The band at the top of a dashboard: the activity's icon and name,
/// with its subtitle under them, on a filled and bordered strip.
///
/// It stops short of the pane's edges rather than running into them -
/// the dashboard is a page, and a page has margins (owner's
/// requirement, 2026-10-27). The work pane's own `header` band sits
/// above every *working* tab; this is the dashboard's answer to it, and
/// the two deliberately look alike.
pub fn head(ui: &mut egui::Ui, theme: &Theme, icon: &str, title: &str, subtitle: &str) {
    let fill = if theme.dark {
        theme.surface
    } else {
        theme.surface_hover
    };
    ui.add_space(HEAD_MARGIN);
    // The band is inset on both sides, so `horizontal` is given the
    // width that is left rather than the whole pane.
    ui.horizontal(|ui| {
        ui.add_space(HEAD_MARGIN);
        let width = (ui.available_width() - HEAD_MARGIN).max(120.0);
        egui::Frame::new()
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, theme.border))
            .corner_radius(egui::CornerRadius::same(8))
            .inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 10,
                bottom: 10,
            })
            .show(ui, |ui| {
                // A `Frame` sizes to its content, so the content has to
                // claim the row or the band stops where the words do.
                ui.set_min_width(width - 28.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        if !icon.is_empty() {
                            ui.label(egui::RichText::new(icon).size(22.0).color(theme.accent));
                            ui.add_space(4.0);
                        }
                        ui.label(egui::RichText::new(title).size(20.0).strong());
                    });
                    if !subtitle.is_empty() {
                        ui.label(
                            egui::RichText::new(subtitle)
                                .size(12.5)
                                .color(theme.text_muted),
                        );
                    }
                });
            });
    });
}

/// How far the band and the rules stay clear of the pane's edges.
const HEAD_MARGIN: f32 = 8.0;

/// A full-width rule with room above and below it.
///
/// Dashboards are a stack of sections and this is what separates them,
/// so the spacing lives here rather than being retyped at every call
/// site and drifting apart.
pub fn rule(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(10.0);
    let width = (ui.available_width() - HEAD_MARGIN * 2.0).max(40.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    let y = rect.center().y;
    let x0 = rect.left() + HEAD_MARGIN;
    ui.painter()
        .hline(x0..=(x0 + width), y, egui::Stroke::new(1.0, theme.border));
    ui.add_space(10.0);
}

/// A section heading inside a dashboard - big enough to read as one.
pub fn section(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    ui.horizontal(|ui| {
        ui.add_space(HEAD_MARGIN);
        ui.label(
            egui::RichText::new(title)
                .size(15.0)
                .strong()
                .color(theme.text),
        );
    });
    ui.add_space(6.0);
}

/// One figure on a dashboard: how many of something, and what.
pub struct Stat<'a> {
    pub icon: &'a str,
    pub value: &'a str,
    pub label: &'a str,
}

/// The summary row: each figure in its own rounded box, every box the
/// same size.
///
/// The width is measured from the widest `icon + label` and the widest
/// value, so the boxes line up as a row of equals rather than hugging
/// their own text - a count of 7 beside a count of 1204 looked like two
/// unrelated controls (owner's requirement, 2026-10-27).
pub fn stats(ui: &mut egui::Ui, theme: &Theme, stats: &[Stat<'_>]) {
    if stats.is_empty() {
        return;
    }
    const VALUE: f32 = 26.0;
    const LABEL: f32 = 13.0;
    const ICON: f32 = 14.0;
    const PAD: f32 = 16.0;
    // Read the spacing before taking the font lock: `ui.fonts_mut`
    // holds the context, and asking it for anything else inside the
    // closure deadlocks (CLAUDE.md).
    let gap = ui.spacing().item_spacing.x;
    // Measure what is actually drawn. Laying the icon and the label out
    // as one string at one size is not the same thing - the icon is a
    // point bigger and a real gap sits between them - so a label
    // measured that way overflowed its box and the frame grew to fit,
    // which is how two boxes meant to be equal ended up different.
    let measured: Vec<(f32, f32)> = ui.fonts_mut(|f| {
        let value_font = egui::FontId::proportional(VALUE);
        let label_font = egui::FontId::proportional(LABEL);
        let icon_font = egui::FontId::proportional(ICON);
        stats
            .iter()
            .map(|s| {
                let value = f
                    .layout_no_wrap(s.value.to_string(), value_font.clone(), theme.text)
                    .size()
                    .x;
                let icon = f
                    .layout_no_wrap(s.icon.to_string(), icon_font.clone(), theme.text)
                    .size()
                    .x;
                let label = f
                    .layout_no_wrap(s.label.to_string(), label_font.clone(), theme.text)
                    .size()
                    .x;
                let row = if s.icon.is_empty() {
                    label
                } else {
                    icon + gap + label
                };
                (value, row)
            })
            .collect()
    });
    // One width for every box, taken from the widest thing any of them
    // has to hold: a row of figures reads as a row only if the boxes
    // are equals (owner's requirement, 2026-10-27).
    let content = measured
        .iter()
        .map(|(v, r)| v.max(*r))
        .fold(0.0_f32, f32::max);
    let row_height = LABEL + 6.0;
    ui.horizontal(|ui| {
        ui.add_space(HEAD_MARGIN);
        for (s, (_, row_width)) in stats.iter().zip(measured.iter()) {
            egui::Frame::new()
                .fill(theme.panel_bg)
                .stroke(egui::Stroke::new(1.0, theme.border))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin {
                    left: PAD as i8,
                    right: PAD as i8,
                    top: 12,
                    bottom: 12,
                })
                .show(ui, |ui| {
                    // `set_width` and not `set_min_width`: the latter
                    // does not cap, and the first box then took the
                    // whole row and pushed the second off it.
                    ui.set_width(content);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(s.value)
                                .size(VALUE)
                                .strong()
                                .color(theme.text),
                        );
                        ui.add_space(2.0);
                        // A row of known width, so the centring has
                        // something to centre: a bare `horizontal`
                        // claims everything and sits flush left.
                        ui.allocate_ui_with_layout(
                            egui::vec2(*row_width, row_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if !s.icon.is_empty() {
                                    ui.label(
                                        egui::RichText::new(s.icon).size(ICON).color(theme.accent),
                                    );
                                }
                                ui.label(
                                    egui::RichText::new(s.label)
                                        .size(LABEL)
                                        .color(theme.text_muted),
                                );
                            },
                        );
                    });
                });
            ui.add_space(10.0);
        }
    });
}

/// The Open Here grid under its own section heading, for a dashboard
/// built from the pieces above. Returns the tab the user asked to go to.
pub fn open_tabs_section(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[TabInfo],
    labels: &DashboardLabels<'_>,
) -> Option<u64> {
    section(ui, theme, labels.open_heading);
    tab_grid(ui, theme, tabs, labels)
}

/// Draw the grid. Returns the tab the user asked to go to.
pub fn open_tabs(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[TabInfo],
    labels: &DashboardLabels<'_>,
) -> Option<u64> {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(labels.open_heading)
            .strong()
            .color(theme.text_muted)
            .size(12.0),
    );
    ui.add_space(6.0);
    tab_grid(ui, theme, tabs, labels)
}

/// The cards themselves, with no heading of their own.
fn tab_grid(
    ui: &mut egui::Ui,
    theme: &Theme,
    tabs: &[TabInfo],
    labels: &DashboardLabels<'_>,
) -> Option<u64> {
    let mut go = None;
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
