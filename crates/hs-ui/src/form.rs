//! Form building blocks shared by settings pages and tool sidebars: card
//! sections, label/field grids, comfortable text fields, hint and note
//! panels, and padded header bars. Keeps every page looking the same.

use egui::{CornerRadius, Margin, RichText, Stroke, Vec2};

use crate::theme::Theme;

/// Minimum width of a text field; fields grow to the available width.
pub const FIELD_MIN_WIDTH: f32 = 260.0;
/// Widest a single-line field gets inside a section.
pub const FIELD_MAX_WIDTH: f32 = 560.0;
/// Inner padding of text fields (taller than egui's default).
pub const FIELD_MARGIN: Margin = Margin {
    left: 10,
    right: 10,
    top: 7,
    bottom: 7,
};

/// A page title with a short description under it.
pub fn page_title(ui: &mut egui::Ui, theme: &Theme, title: &str, subtitle: Option<&str>) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(22.0).strong());
    if let Some(sub) = subtitle {
        ui.label(RichText::new(sub).color(theme.text_muted));
    }
    ui.add_space(10.0);
}

/// A card: titled, framed, with consistent padding. Content width is capped
/// so wide windows don't stretch fields across the whole screen.
pub fn section<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let r = egui::Frame::new()
        .fill(theme.surface)
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(920.0));
            ui.label(RichText::new(title).size(15.0).strong());
            ui.add_space(2.0);
            ui.separator();
            ui.add_space(8.0);
            add_contents(ui)
        })
        .inner;
    ui.add_space(12.0);
    r
}

/// Two-column label/field grid for a section.
pub fn field_grid<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([18.0, 10.0])
        .min_col_width(170.0)
        .show(ui, add_contents)
        .inner
}

/// A row label inside [`field_grid`]: muted, vertically centred with the field.
pub fn label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [170.0, 32.0],
        egui::Label::new(RichText::new(text)).halign(egui::Align::LEFT),
    );
}

/// A comfortable single-line text field that fills the available width
/// (between [`FIELD_MIN_WIDTH`] and [`FIELD_MAX_WIDTH`]).
pub fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    let w = ui.available_width().clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH);
    text_field_width(ui, value, hint, w)
}

/// A comfortable single-line text field with an explicit width.
pub fn text_field_width(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .margin(FIELD_MARGIN)
            .desired_width(width),
    )
}

/// A password-style field (see [`text_field`]).
pub fn secret_field(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .password(true)
            .margin(FIELD_MARGIN)
            .desired_width(width),
    )
}

/// A multi-line code field that fills the width.
pub fn code_field(ui: &mut egui::Ui, value: &mut String, rows: usize) -> egui::Response {
    ui.add(
        egui::TextEdit::multiline(value)
            .code_editor()
            .margin(FIELD_MARGIN)
            .desired_rows(rows)
            .desired_width(ui.available_width().min(FIELD_MAX_WIDTH + 170.0)),
    )
}

/// Muted explanatory text under a control.
pub fn hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme.text_muted).size(12.5)).wrap());
}

/// A framed note panel with an info icon: instructions, tips, warnings.
pub fn note(ui: &mut egui::Ui, theme: &Theme, icon: &str, text: &str) {
    egui::Frame::new()
        .fill(theme.surface)
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.label(RichText::new(icon).size(16.0).color(theme.secondary));
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(text).color(theme.text_muted)).wrap());
            });
        });
}

/// A sidebar/toolbar header row with vertical padding, drawn on the panel
/// background with a bottom border.
pub fn header_bar<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let r = egui::Frame::new()
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 10,
            bottom: 10,
        })
        .show(ui, |ui| ui.horizontal(|ui| add_contents(ui)).inner)
        .inner;
    let rect = ui.max_rect();
    let y = ui.cursor().min.y;
    ui.painter().hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, theme.border),
    );
    ui.add_space(6.0);
    r
}

/// A selectable list row with an icon, hover highlight and selection colour
/// (shell profiles, connections, environments).
pub fn list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: &str,
    title: &str,
    subtitle: Option<&str>,
) -> egui::Response {
    let height = if subtitle.is_some() { 44.0 } else { 34.0 };
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        egui::Sense::click(),
    );
    let fill = if selected {
        theme.accent.linear_multiply(0.30)
    } else if resp.hovered() {
        theme.surface_hover
    } else {
        egui::Color32::TRANSPARENT
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    if resp.hovered() || selected {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.0, if selected { theme.accent } else { theme.border }),
            egui::StrokeKind::Inside,
        );
    }
    let icon_col = if selected || resp.hovered() {
        theme.accent
    } else {
        theme.text_muted
    };
    ui.painter().text(
        egui::pos2(rect.left() + 12.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(20.0),
        icon_col,
    );
    let text_x = rect.left() + 42.0;
    match subtitle {
        Some(sub) => {
            ui.painter().text(
                egui::pos2(text_x, rect.center().y - 8.0),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(14.0),
                theme.text,
            );
            ui.painter().text(
                egui::pos2(text_x, rect.center().y + 9.0),
                egui::Align2::LEFT_CENTER,
                sub,
                egui::FontId::proportional(11.5),
                theme.text_muted,
            );
        }
        None => {
            ui.painter().text(
                egui::pos2(text_x, rect.center().y),
                egui::Align2::LEFT_CENTER,
                title,
                egui::FontId::proportional(14.0),
                theme.text,
            );
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    resp
}

/// An icon-only button with a hover background, accent colour when hovered
/// or active, and a pointing-hand cursor so it reads as clickable.
pub fn icon_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &str,
    size: f32,
    active: bool,
) -> egui::Response {
    let pad = (size * 0.45).round();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size + pad * 2.0), egui::Sense::click());
    if resp.hovered() || active {
        let fill = if resp.hovered() {
            theme.surface_hover
        } else {
            theme.accent.linear_multiply(0.18)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    }
    let col = if active || resp.hovered() {
        theme.accent
    } else {
        theme.text_muted
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(size),
        col,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, icon));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A primary (accent-filled) button.
/// An [`icon_button`] that opens a menu: the same square and hover as the
/// plain icon buttons, so a row of them keeps an even rhythm.
pub fn icon_menu(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &str,
    size: f32,
    active: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let r = icon_button(ui, theme, icon, size, active);
    egui::Popup::menu(&r).show(add_contents);
    r
}

pub fn primary_button(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(egui::Color32::WHITE))
            .fill(theme.accent)
            .corner_radius(CornerRadius::same(5))
            .min_size(Vec2::new(0.0, 30.0)),
    )
}

/// A danger (error-coloured text) button.
pub fn danger_button(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(theme.error)).min_size(Vec2::new(0.0, 30.0)))
}
