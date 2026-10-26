//! The band above a tool's toolbar that says what the tab is looking at.
//!
//! Every activity answers the same questions - what is this, what kind of
//! thing is it, what version, where is it - and used to answer them in
//! whatever corner of its toolbar had room: a size readout here, a
//! connection name there, a path squeezed beside the assistant button.
//! One band, drawn by the shell above every toolbar, puts them in the
//! same place in every activity (owner's requirement, 2026-10-26).
//!
//! Tools do not draw this. They return [`hs_core::tool::ToolHeader`] and
//! the shell renders it, so the spacing, the rules and the emphasis
//! cannot drift apart between activities.

use egui::{Color32, CornerRadius, Margin, RichText, Stroke};

use crate::theme::Theme;

/// Space either side of every element and every rule.
const PAD: f32 = 10.0;
/// The name and its icon, which lead the band.
const NAME_SIZE: f32 = 15.0;
const ICON_SIZE: f32 = 18.0;
/// The facts that follow it.
const FACT_SIZE: f32 = 12.5;
/// The line under the band. Thick enough to read as a header rather
/// than as one more separator in a stack of panels.
const RULE: f32 = 2.0;

/// Draw the band for `header`.
///
/// Facts with an empty value are left out entirely, rules and all: a
/// terminal with no profile shows no "Profile :" gap, rather than a
/// label with nothing after it.
pub fn work_header(ui: &mut egui::Ui, theme: &Theme, header: &hs_core::tool::ToolHeader) {
    let fill = if theme.dark {
        theme.surface
    } else {
        theme.surface_hover
    };
    egui::Frame::new()
        .fill(fill)
        .inner_margin(Margin {
            left: PAD as i8,
            right: PAD as i8,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(PAD);
                if !header.icon.is_empty() {
                    ui.label(
                        RichText::new(&header.icon)
                            .size(ICON_SIZE)
                            .color(theme.accent),
                    );
                    ui.add_space(8.0);
                }
                ui.label(
                    RichText::new(&header.name)
                        .size(NAME_SIZE)
                        .strong()
                        .color(theme.text),
                );
                ui.add_space(PAD);
                rule(ui, theme);
                for (label, value) in header.facts.iter() {
                    if value.trim().is_empty() {
                        continue;
                    }
                    ui.add_space(PAD);
                    // A fact with no label is just its value - a path, or
                    // a method and a request name.
                    let text = if label.is_empty() {
                        value.clone()
                    } else {
                        format!("{label} : {value}")
                    };
                    ui.label(RichText::new(text).size(FACT_SIZE).color(theme.text_muted));
                    ui.add_space(PAD);
                    rule(ui, theme);
                }
            });
        });
    // The band's own underline, full width.
    let y = ui.cursor().top();
    let x = ui.max_rect().x_range();
    ui.painter()
        .hline(x, y, Stroke::new(RULE, theme.accent.linear_multiply(0.65)));
    ui.add_space(RULE);
}

/// A vertical rule between two parts of the band.
fn rule(ui: &mut egui::Ui, theme: &Theme) {
    let h = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, h), egui::Sense::hover());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(1.0, h)),
        CornerRadius::ZERO,
        separator_colour(theme),
    );
}

fn separator_colour(theme: &Theme) -> Color32 {
    theme.border
}
