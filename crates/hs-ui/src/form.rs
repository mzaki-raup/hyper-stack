//! Form building blocks shared by settings pages and tool sidebars: card
//! sections, label/field grids, comfortable text fields, hint and note
//! panels, and padded header bars. Keeps every page looking the same.

use egui::{CornerRadius, Margin, RichText, Stroke, Vec2};

use crate::theme::Theme;
use crate::tip::TipExt;
use crate::tree::TreeIcon;

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

/// The air between the parts of a work pane: under a toolbar or a
/// header rule, between an action row and a filter row, between a filter
/// row and its table, around a table and the status under it. One number
/// for every activity (owner's spacing pass, CR-0031), so the panes
/// breathe alike and a change is made once.
pub const PANE_GAP: f32 = 8.0;

/// How far a work pane's content sits in from the pane's edges (CR-0061).
pub const PANE_MARGIN: f32 = 12.0;

/// The vertical space between two form rows, the same as
/// [`field_grid`]'s row spacing (CR-0061).
pub const ROW_GAP: f32 = 10.0;
/// How far a sub-setting's label (one that only applies when its parent
/// is on) sits in from its parent's label in a [`field_grid`] (CR-0080).
pub const SUB_INDENT: f32 = 18.0;
/// The gap between buttons side by side in a form (CR-0080).
pub const BUTTON_GAP: f32 = 8.0;
/// The narrowest a [`field_grid`]'s label column is; it grows to fit its
/// longest label, so every control starts at one x (CR-0080).
const LABEL_MIN_WIDTH: f32 = 170.0;
/// The horizontal padding of every form button, so a primary and a plain
/// button side by side are one size (CR-0080).
const BUTTON_PADDING: Vec2 = Vec2::new(12.0, 4.0);
/// The gap between options sharing one row - checkboxes, radio buttons -
/// so each reads as its own choice rather than one run of text (owner,
/// 2026-11-06, CR-0078).
pub const OPTION_GAP: f32 = 18.0;

/// Draw a work pane's body [`PANE_MARGIN`] in from its left and right
/// edges and [`PANE_GAP`] below what is above it (CR-0061).
pub fn pane<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin {
            left: PANE_MARGIN as i8,
            right: PANE_MARGIN as i8,
            top: PANE_GAP as i8,
            bottom: PANE_GAP as i8,
        })
        .show(ui, add)
        .inner
}

/// How tall a text field drawn with [`FIELD_MARGIN`] comes out. A control
/// put beside one — a ComboBox, a button — has to be told this, or the two
/// frames sit at different heights and the row looks crooked even though
/// egui has centred them.
pub fn field_height(ui: &egui::Ui) -> f32 {
    ui.text_style_height(&egui::TextStyle::Body) + (FIELD_MARGIN.top + FIELD_MARGIN.bottom) as f32
}

/// Run `add` with controls sized to match a text field, so a ComboBox or a
/// button lines up with one beside it.
pub fn field_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let h = field_height(ui);
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = h;
        add(ui)
    })
    .inner
}

/// A slider in a settings grid: a visible rail (the dark theme paints
/// egui's inactive fill in the panel colour, so a bare slider at its
/// minimum is a lone box), a trailing fill and a fixed comfortable width.
pub fn slider(ui: &mut egui::Ui, theme: &Theme, slider: egui::Slider<'_>) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = 260.0;
        let widgets = &mut ui.visuals_mut().widgets;
        widgets.inactive.bg_fill = theme.border;
        widgets.hovered.bg_fill = theme.surface_hover;
        ui.add(slider.trailing_fill(true))
    })
    .inner
}

/// A page title with a short description under it.
pub fn page_title(ui: &mut egui::Ui, theme: &Theme, title: &str, subtitle: Option<&str>) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(22.0).strong());
    if let Some(sub) = subtitle {
        ui.label(RichText::new(sub).color(theme.text_muted));
    }
    ui.add_space(10.0);
}

/// [`page_title`] led by the page's own coloured mark (a Devtools tool's
/// picture, the icon its tab wears), on the title's middle line (owner,
/// 2026-11-07).
pub fn page_title_marked(
    ui: &mut egui::Ui,
    theme: &Theme,
    mark: Mark,
    title: &str,
    subtitle: Option<&str>,
) {
    ui.add_space(4.0);
    let text = RichText::new(title).size(22.0).strong();
    let h = ui
        .painter()
        .layout_no_wrap(
            title.to_string(),
            egui::FontId::proportional(22.0),
            egui::Color32::WHITE,
        )
        .size()
        .y
        .max(mark.size);
    // The row's height is set before it opens, so the mark and the title
    // share one middle line (see the "label sits high" gotcha).
    let saved = ui.spacing().interact_size.y;
    ui.spacing_mut().interact_size.y = h;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(mark.size, h), egui::Sense::hover());
        mark.paint(ui, rect.left_center(), theme.text_muted);
        ui.add_space(4.0);
        ui.label(text);
    });
    ui.spacing_mut().interact_size.y = saved;
    if let Some(sub) = subtitle {
        ui.label(RichText::new(sub).color(theme.text_muted));
    }
    ui.add_space(10.0);
}

/// A card: titled, framed, with consistent padding. Content width is capped
/// so wide windows don't stretch fields across the whole screen.
/// How wide a card - and anything that should line up with one - may
/// grow.
///
/// A settings or tool page on a wide monitor is a column of cards, not
/// a column of full-width bars: past about this width a line of text
/// becomes hard to track back to the next line. Anything drawn beside
/// or between cards uses this too, or it runs to the window edge while
/// the cards stop short and the page looks broken (owner's report,
/// 2026-10-27).
pub const CARD_MAX_WIDTH: f32 = 920.0;

pub fn section<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    section_max(ui, theme, title, CARD_MAX_WIDTH, add_contents)
}

/// A [`section`] with its own width cap: `f32::INFINITY` for a card that
/// holds a wide table and would otherwise scroll sideways inside a page
/// that has room (Settings → Security → Credentials, CR-0038).
pub fn section_max<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    max_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let r = egui::Frame::new()
        .fill(theme.surface)
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(18, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(max_width));
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

/// Two-column label/field grid for a section: labels left-aligned in a
/// first column as wide as its longest label, every control starting at
/// one x in the second (CR-0080).
pub fn field_grid<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let h = field_height(ui);
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([18.0, ROW_GAP])
        .min_col_width(LABEL_MIN_WIDTH)
        .show(ui, |ui| {
            // Every cell in a form row is a field's height tall, so a
            // checkbox or a ComboBox beside a label centres on the same
            // line as the label's text instead of hugging the row's top.
            ui.spacing_mut().interact_size.y = h;
            add_contents(ui)
        })
        .inner
}

/// A row label inside [`field_grid`]: left-aligned, vertically centred
/// with the field.
pub fn label(ui: &mut egui::Ui, text: &str) {
    grid_label(ui, text, 0.0);
}

/// A sub-setting's label inside [`field_grid`] (one that only applies
/// when its parent is on): [`label`] indented [`SUB_INDENT`] under its
/// parent's, still left-aligned (CR-0080).
pub fn sub_label(ui: &mut egui::Ui, text: &str) {
    grid_label(ui, text, SUB_INDENT);
}

/// A grid label `indent` in from the column's left edge, a field's height
/// tall; returns the rect of the text itself. The cell is at least
/// page's label-column width ([`page_labels`]) and grows to fit the text,
/// so the grid's first column - and the x every control starts at - fits
/// its longest label. The indent is part of the cell rather than an
/// `add_space`, which a grid does not allow.
fn grid_label(ui: &mut egui::Ui, text: &str, indent: f32) -> egui::Rect {
    let h = field_height(ui);
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    note_label_width(ui, indent + galley.size().x);
    let w = (indent + galley.size().x).max(label_column_width(ui));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), egui::Sense::hover());
    let text_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + indent,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley.size(),
    );
    let colour = ui.visuals().text_color();
    ui.painter().galley(text_rect.min, galley, colour);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    text_rect
}

/// An empty first cell in a [`field_grid`] row, as wide as the page's
/// label column and `height` tall, so what follows starts at the control
/// column.
fn empty_label_cell(ui: &mut egui::Ui, height: f32) {
    let size = Vec2::new(label_column_width(ui), height);
    ui.allocate_exact_size(size, egui::Sense::hover());
}

/// What [`page_labels`] keeps in the context while a page is drawn: the
/// label-column width every grid uses this frame, and the widest label
/// measured so far, for the next frame.
#[derive(Clone, Copy, Default)]
struct PageLabels {
    width: f32,
    widest: f32,
}

fn page_labels_id() -> egui::Id {
    egui::Id::new("hs-form-page-labels")
}

/// Draw a page whose [`field_grid`]s - in every card - share one label
/// column, as wide as the page's widest label, so controls line up across
/// cards (owner, 2026-11-06). The width is measured while the page is
/// drawn and used from the next frame; a change asks for a repaint.
pub fn page_labels<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let key = egui::Id::new(("hs-form-page-label-width", id));
    let width = ui
        .data(|d| d.get_temp::<f32>(key))
        .unwrap_or(LABEL_MIN_WIDTH);
    let start = PageLabels {
        width,
        widest: LABEL_MIN_WIDTH,
    };
    let outer = ui.data_mut(|d| {
        let outer = d.get_temp::<PageLabels>(page_labels_id());
        d.insert_temp(page_labels_id(), start);
        outer
    });
    let r = add(ui);
    let done = ui.data_mut(|d| {
        let done = d.get_temp::<PageLabels>(page_labels_id());
        match outer {
            Some(o) => {
                d.insert_temp(page_labels_id(), o);
            }
            None => {
                d.remove_temp::<PageLabels>(page_labels_id());
            }
        }
        done
    });
    if let Some(p) = done
        && (p.widest - width).abs() > 0.5
    {
        ui.data_mut(|d| d.insert_temp(key, p.widest));
        ui.ctx().request_repaint();
    }
    r
}

/// The label column's width: the page's (see [`page_labels`]), or the
/// minimum outside one.
fn label_column_width(ui: &egui::Ui) -> f32 {
    ui.data(|d| d.get_temp::<PageLabels>(page_labels_id()))
        .map_or(LABEL_MIN_WIDTH, |p| p.width.max(LABEL_MIN_WIDTH))
}

/// Record a label's width for the page's next frame.
fn note_label_width(ui: &egui::Ui, w: f32) {
    ui.data_mut(|d| {
        if let Some(mut p) = d.get_temp::<PageLabels>(page_labels_id()) {
            p.widest = p.widest.max(w);
            d.insert_temp(page_labels_id(), p);
        }
    });
}

/// A sub-section heading inside a card (a group of rows under the card's
/// title): a step larger than body text and strong, smaller than the
/// card's title, with [`ROW_GAP`] above and half that below (CR-0080).
pub fn subsection(ui: &mut egui::Ui, theme: &Theme, title: &str) {
    let size = ui.text_style_height(&egui::TextStyle::Body) + 1.5;
    ui.add_space(ROW_GAP);
    ui.label(RichText::new(title).size(size).strong().color(theme.text));
    ui.add_space(ROW_GAP / 2.0);
}

/// A checkbox with its description under the label, indented to the
/// label's text, and [`ROW_GAP`] after the description rather than
/// between the box and its text (CR-0080). Returns the checkbox's
/// response.
pub fn checkbox_with_description(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    description: &str,
) -> egui::Response {
    let r = described_checkbox(ui, value, label, description);
    let rest = ROW_GAP - ui.spacing().item_spacing.y;
    if rest > 0.0 {
        ui.add_space(rest);
    }
    r
}

/// A checkbox and its description under the checkbox's text, stacked.
fn described_checkbox(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    description: &str,
) -> egui::Response {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let r = ui.checkbox(value, label);
        if !description.is_empty() {
            let indent = ui.spacing().icon_width + ui.spacing().icon_spacing;
            let weak = ui.visuals().weak_text_color();
            ui.horizontal(|ui| {
                ui.add_space(indent);
                ui.add(egui::Label::new(RichText::new(description).color(weak).size(12.5)).wrap());
            });
        }
        r
    })
    .inner
}

/// A checkbox row inside [`field_grid`]: an empty label cell, then the
/// checkbox at the control column, like every other control (owner,
/// 2026-11-06). Ends the row.
pub fn checkbox_row(ui: &mut egui::Ui, value: &mut bool, label: &str) -> egui::Response {
    let h = field_height(ui);
    empty_label_cell(ui, h);
    let r = ui.checkbox(value, label);
    ui.end_row();
    r
}

/// [`checkbox_with_description`] as a [`field_grid`] row: an empty label
/// cell, then the checkbox at the control column with its description
/// under the checkbox's text. Ends the row.
pub fn checkbox_row_with_description(
    ui: &mut egui::Ui,
    value: &mut bool,
    label: &str,
    description: &str,
) -> egui::Response {
    let h = field_height(ui);
    empty_label_cell(ui, h);
    let r = described_checkbox(ui, value, label, description);
    ui.end_row();
    r
}

/// A row inside [`field_grid`] whose control is whatever `add` draws - a
/// button row, a note, a list - at the control column under an empty
/// label cell. Ends the row.
pub fn control_row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let h = field_height(ui);
    empty_label_cell(ui, h);
    let r = ui.vertical(add).inner;
    ui.end_row();
    r
}

/// A single-line `TextEdit` with the standard [`FIELD_MARGIN`], for a
/// call site that sets its own width, hint or font. Every text field is
/// built from this or from [`text_field`], so a field is the same height
/// as a dropdown or a button beside it everywhere (CR-0026, CR-0061);
/// egui's own default margin draws a field about ten pixels shorter.
pub fn line_edit(value: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(value).margin(FIELD_MARGIN)
}

/// Give the controls of the rest of this `ui` - a dropdown, a checkbox,
/// a button - a text field's height, so a row that mixes them lines up
/// and sits on one middle line (CR-0026, CR-0061). Call it at the top of
/// the row's closure; it ends with the closure.
pub fn match_field_height(ui: &mut egui::Ui) {
    let h = field_height(ui);
    ui.spacing_mut().interact_size.y = h;
}

/// A row of mixed controls - a label, a text field, a dropdown, buttons -
/// on one middle line (CR-0061). The row's height is a field's height
/// *before* the row is laid out: egui fixes a horizontal row's height
/// when it opens, so raising it inside the closure (as
/// [`match_field_height`] alone does) leaves a label centred on the old,
/// shorter row - it sits high beside the field.
pub fn centred_row<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let saved = ui.spacing().interact_size.y;
    ui.spacing_mut().interact_size.y = field_height(ui);
    let r = ui.horizontal(add);
    ui.spacing_mut().interact_size.y = saved;
    r
}

/// [`centred_row`] that wraps to the next line when it runs out of width.
pub fn centred_row_wrapped<R>(
    ui: &mut egui::Ui,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let saved = ui.spacing().interact_size.y;
    ui.spacing_mut().interact_size.y = field_height(ui);
    let r = ui.horizontal_wrapped(add);
    ui.spacing_mut().interact_size.y = saved;
    r
}

/// A comfortable single-line text field that fills the available width
/// (between [`FIELD_MIN_WIDTH`] and [`FIELD_MAX_WIDTH`]).
pub fn text_field(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    let w = ui.available_width().clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH);
    text_field_width(ui, value, hint, w)
}

/// A comfortable single-line text field with an explicit width. Hovering
/// a `{{name}}` in it shows that variable's value
/// ([`crate::varpeek::text_edit_tip`]).
pub fn text_field_width(
    ui: &mut egui::Ui,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let out = egui::TextEdit::singleline(value)
        .hint_text(hint)
        .margin(FIELD_MARGIN)
        .desired_width(width)
        .show(ui);
    crate::varpeek::text_edit_tip(&out, value);
    out.response.response
}

/// A text field with the server's values behind a caret: typed or chosen
/// (FEAT-0095). A ref, an assignee or a priority is what the server knows,
/// so the list comes from it; but the list can be stale or the server out
/// of reach, so the field is never *only* the list. With no options it is
/// a plain text field, and the form still works. Returns the text field's
/// response; a choice from the list writes `value` and counts as a change.
pub fn pick_field(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &mut String,
    options: &[String],
    hint: &str,
) -> egui::Response {
    let w = ui.available_width().clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH);
    pick_field_width(ui, theme, value, options, hint, w)
}

/// [`pick_field`] at an exact outer width (the caret included), for a
/// table cell or a row that sets its own widths (CR-0061).
pub fn pick_field_width(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &mut String,
    options: &[String],
    hint: &str,
    width: f32,
) -> egui::Response {
    if options.is_empty() {
        return text_field_width(ui, value, hint, width);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (width - 32.0).max(80.0);
        let mut r = text_field_width(ui, value, hint, w);
        let caret = icon_button(ui, theme, crate::icons::CARET_DOWN, 14.0, false);
        let mut chosen = false;
        egui::Popup::menu(&caret).show(|ui| {
            ui.set_min_width(w);
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for o in options {
                        if selectable_label(ui, value == o, o).clicked() {
                            *value = o.clone();
                            chosen = true;
                            ui.close();
                        }
                    }
                });
        });
        if chosen {
            r.mark_changed();
        }
        r
    })
    .inner
}

/// A comma-separated list of names with the known names behind a caret,
/// each with a checkbox (CR-0072): an index's columns, a foreign key's
/// referenced columns. Ticking a name adds it at the end of the list,
/// unticking removes it, so the order is the order of picking; the field
/// can still be typed. With no options it is a plain text field. Returns
/// the text field's response; a tick counts as a change.
pub fn multi_pick_field_width(
    ui: &mut egui::Ui,
    theme: &Theme,
    value: &mut String,
    options: &[String],
    hint: &str,
    width: f32,
) -> egui::Response {
    if options.is_empty() {
        return text_field_width(ui, value, hint, width);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (width - 32.0).max(80.0);
        let mut r = text_field_width(ui, value, hint, w);
        let caret = icon_button(ui, theme, crate::icons::CARET_DOWN, 14.0, false);
        let mut changed = false;
        egui::Popup::menu(&caret)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_min_width(w);
                let picked: Vec<String> =
                    list_items(value).into_iter().map(str::to_string).collect();
                egui::ScrollArea::vertical()
                    .max_height(240.0)
                    .show(ui, |ui| {
                        for o in options {
                            let mut on = picked.iter().any(|p| p == o);
                            if ui.checkbox(&mut on, o.as_str()).changed() {
                                *value = toggle_list_item(value, o);
                                changed = true;
                            }
                        }
                    });
            });
        if changed {
            r.mark_changed();
        }
        r
    })
    .inner
}

/// The names in a comma-separated list, trimmed, empty ones dropped.
fn list_items(value: &str) -> Vec<&str> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// `value` with `name` removed when it is in the list, or appended at the
/// end when it is not; written back as `a, b, c` ([`multi_pick_field_width`]).
fn toggle_list_item(value: &str, name: &str) -> String {
    let mut items = list_items(value);
    if let Some(i) = items.iter().position(|s| *s == name) {
        items.remove(i);
    } else {
        items.push(name);
    }
    items.join(", ")
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
    let out = egui::TextEdit::multiline(value)
        .code_editor()
        .margin(FIELD_MARGIN)
        .desired_rows(rows)
        .desired_width(ui.available_width().min(FIELD_MAX_WIDTH + 170.0))
        .show(ui);
    crate::varpeek::text_edit_tip(&out, value);
    out.response.response
}

/// Muted explanatory text under a control.
pub fn hint(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(theme.text_muted).size(12.5)).wrap());
}

/// A [`hint`] directly under the row it explains, on a row of its own
/// inside [`field_grid`]: an empty label cell, the hint at the control's x
/// (owner, 2026-11-06). A grid cell is centred in its row, so a
/// description stacked under the control would make that row tall and
/// drag the label below the control's middle; on its own row the label
/// stays level with its control. Call it right after the row's `end_row`.
pub fn row_hint(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    empty_label_cell(ui, 0.0);
    let r = ui.add(egui::Label::new(RichText::new(text).color(theme.text_muted).size(12.5)).wrap());
    ui.end_row();
    r
}

/// A well for a scrollable area: the field background, a border and a
/// fixed height, so a list or a block of output looks like something you
/// can scroll rather than like text that happens to stop (CR-0028).
///
/// `height` is the well's inner height; pass `None` to let it take what
/// is left of the pane.
pub fn scroll_well<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    height: Option<f32>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    well(ui, theme, height, |ui| {
        let h = height.unwrap_or_else(|| ui.available_height());
        let mut area = egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false; 2]);
        if height.is_some() {
            area = area.max_height(h).min_scrolled_height(h);
        }
        area.show(ui, add).inner
    })
}

/// A read-only item-and-value list drawn as a table with no header:
/// striped rows the height of a table's, the name muted in a column of
/// its own, the value wrapping beside it. For the facts a detail pane
/// lists - Type / Bug, Author / Redmine Admin (owner's report, CR-0028).
pub fn detail_table<'a>(
    ui: &mut egui::Ui,
    theme: &Theme,
    id: impl std::hash::Hash + std::fmt::Debug,
    rows: impl IntoIterator<Item = (&'a str, &'a str)>,
) {
    egui::Grid::new(id)
        .num_columns(2)
        .striped(true)
        .spacing([16.0, 4.0])
        .min_col_width(150.0)
        .min_row_height(crate::table::ROW_HEIGHT)
        .show(ui, |ui| {
            for (name, value) in rows {
                ui.label(RichText::new(name).color(theme.text_muted));
                ui.add(egui::Label::new(value).wrap());
                ui.end_row();
            }
        });
}

/// The well itself, for content that scrolls on its own (a `DataTable`
/// keeps its own scroll areas) or does not scroll at all.
pub fn well<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    height: Option<f32>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(theme.field_bg())
        .stroke(Stroke::new(1.0, theme.border))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(h) = height {
                ui.set_min_height(h);
                ui.set_max_height(h);
            }
            add(ui)
        })
        .inner
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

/// How tall a sidebar header's row is, so the title and its buttons share a
/// centre line.
pub const SIDEBAR_HEADER_HEIGHT: f32 = 28.0;

/// The header of a sidebar list: the title on the left, small icon actions on
/// the right. Every tool draws its connections list with this, so the titles
/// sit on the same baseline and the buttons in the same place.
///
/// Returns the index of the action that was clicked.
/// A sidebar header with something drawn before the title: the
/// workspace's own logo on Home, falling back to the title's lettered tile
/// (CR-0082) when none is set, so the column says which workspace these
/// pins belong to rather than only naming it. Owner's requirement,
/// 2026-10-17.
pub fn sidebar_header_icon(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<egui::Image<'_>>,
    title: &str,
    actions: &[(&str, &str)],
) -> Option<usize> {
    sidebar_header_inner(ui, theme, Some(icon), title, actions)
}

fn sidebar_header_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    leading: Option<Option<egui::Image<'_>>>,
    title: &str,
    actions: &[(&str, &str)],
) -> Option<usize> {
    let mut clicked = None;
    header_bar(ui, theme, |ui| {
        // One row of a fixed height, both halves centred in it: letting each
        // half size itself left the title and the buttons a few pixels apart.
        let height = SIDEBAR_HEADER_HEIGHT;
        ui.set_min_height(height);
        let width = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::Vec2::new(width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                if let Some(icon) = leading {
                    // Home's header names the open workspace, and its
                    // logo is the mark that says which project this is.
                    const SIZE: f32 = 22.0;
                    match icon {
                        Some(img) => {
                            ui.add(img.fit_to_exact_size(egui::Vec2::splat(SIZE)));
                        }
                        None => {
                            crate::lettered::tile(ui, title, SIZE);
                        }
                    }
                    ui.add_space(6.0);
                }
                ui.label(egui::RichText::new(title).strong().size(15.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Right to left, so the first action ends up rightmost and
                    // the add button sits in the same place in every tool.
                    for (i, (icon, tip)) in actions.iter().enumerate() {
                        if icon_button(ui, theme, icon, 16.0, false)
                            .tip(*tip)
                            .clicked()
                        {
                            clicked = Some(i);
                        }
                    }
                });
            },
        );
    });
    clicked
}

/// One action in a sidebar header's button row: drawn as `icon label`,
/// with `tip` on hover when it has more to say than the label.
pub struct SideAction<'a> {
    pub icon: &'a str,
    pub label: &'a str,
    pub tip: &'a str,
}

/// A sidebar's header, in the shape the Containers sidebar set: the
/// title in its bar, then a row of labelled actions with air above and
/// below, then a rule before the list. The first action is the primary
/// one; any others sit beside it as framed buttons. Returns the index
/// of the action clicked (owner's standard, CR-0030).
///
/// Icon-only header buttons left the reader to hover each one to learn
/// what it did; every action here carries its words.
pub fn sidebar_head(
    ui: &mut egui::Ui,
    theme: &Theme,
    title: &str,
    actions: &[SideAction<'_>],
) -> Option<usize> {
    header_bar(ui, theme, |ui| {
        ui.set_min_height(SIDEBAR_HEADER_HEIGHT);
        ui.horizontal(|ui| {
            ui.set_min_height(SIDEBAR_HEADER_HEIGHT);
            ui.label(RichText::new(title).strong().size(15.0));
        });
    });
    if actions.is_empty() {
        return None;
    }
    let mut clicked = None;
    sidebar_action_row(ui, theme, |ui| {
        for (i, a) in actions.iter().enumerate() {
            let r = if i == 0 {
                primary_button(ui, theme, &format!("{} {}", a.icon, a.label))
            } else {
                crate::toolbar::btn(ui, theme, a.icon, a.label)
            };
            let r = if a.tip.is_empty() { r } else { r.tip(a.tip) };
            if r.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// The band under a sidebar's title where its actions sit: air above, a
/// wrapping row, air below and a rule across the column. Separate from
/// [`sidebar_head`] so a sidebar can put something else in the same
/// place - the Scratchpad's folder-name form takes the buttons' row
/// while it is open (CR-0030).
pub fn sidebar_action_row<R>(
    ui: &mut egui::Ui,
    theme: &Theme,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let r = egui::Frame::new()
        .inner_margin(Margin {
            left: 8,
            right: 8,
            top: 8,
            bottom: 8,
        })
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                add(ui)
            })
            .inner
        })
        .inner;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 1.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme.border),
    );
    ui.add_space(6.0);
    r
}

/// A selectable list row with an icon, hover highlight and selection colour
/// (shell profiles, connections, environments).
/// The right-click menu every connection row carries: pin it to Home, or
/// take it off again. Returns the new state when the user chose one.
///
/// Home lists the connections someone pinned, which is only meaningful if
/// pinning is something they can do from where the connections are.
/// What a connection row reported this frame.
pub struct ConnectionRow {
    pub response: egui::Response,
    /// `Some(true)` to pin the connection to Home, `Some(false)` to unpin.
    pub pin: Option<bool>,
    /// Edit This Connection was chosen: take the user to the profile's
    /// own form in Settings.
    pub edit: bool,
    /// Remove From List was chosen (FEAT-0094).
    pub remove: bool,
}

/// A row in a tool's connection sidebar: the row itself, plus the
/// Pin to Home / Unpin From Home menu on it.
///
/// Every connection list goes through this rather than calling
/// [`list_row`] directly, so the menu cannot be left off one of them —
/// which is exactly what had happened to Database, Redis, NoSQL and S3,
/// where only SSH offered it. A new activity type gets the menu by using
/// this helper. Owner's requirement, 2026-10-17.
#[allow(clippy::too_many_arguments)]
pub fn connection_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: crate::icons::ConnIcon,
    name: &str,
    subtitle: Option<&str>,
    pinned: bool,
    labels: RowMenuLabels<'_>,
) -> ConnectionRow {
    let response = conn_list_row(ui, theme, selected, icon, name, subtitle);
    let menu = row_menu(&response, pinned, labels);
    ConnectionRow {
        response,
        pin: menu.pin,
        edit: menu.edit,
        remove: menu.remove,
    }
}

/// What the row's context menu calls its items, in the caller's locale.
#[derive(Clone, Copy)]
pub struct RowMenuLabels<'a> {
    pub pin: &'a str,
    pub unpin: &'a str,
    pub edit: &'a str,
    pub remove: &'a str,
}

/// What was chosen in a connection row's context menu.
#[derive(Default)]
pub struct RowMenu {
    pub pin: Option<bool>,
    pub edit: bool,
    /// Remove From List was chosen; the caller asks before anything goes.
    pub remove: bool,
}

/// The context menu every connection row carries: pin to Home, and edit
/// the profile where it is kept. Both live here so no list can be left
/// without them.
pub fn row_menu(row: &egui::Response, pinned: bool, labels: RowMenuLabels<'_>) -> RowMenu {
    let mut out = RowMenu::default();
    row.context_menu(|ui| {
        // Every item a glyph, the same verb's glyph as everywhere else
        // (ui.md "Menu items carry a glyph").
        if menu_item(ui, crate::icons::ph::PENCIL_SIMPLE, labels.edit).clicked() {
            out.edit = true;
            ui.close();
        }
        if menu_item(ui, crate::icons::ph::TRASH, labels.remove).clicked() {
            out.remove = true;
            ui.close();
        }
        ui.separator();
        if pinned {
            if menu_item(ui, crate::icons::ph::PUSH_PIN_SLASH, labels.unpin).clicked() {
                out.pin = Some(false);
                ui.close();
            }
        } else if menu_item(ui, crate::icons::ph::PUSH_PIN, labels.pin).clicked() {
            out.pin = Some(true);
            ui.close();
        }
    });
    out
}

/// Another widget has the keyboard - a tab's rename field in the strip,
/// say. A pane that hands its own widget `own` the focus when it comes to
/// the front leaves it where it is.
pub fn focus_is_elsewhere(ctx: &egui::Context, own: egui::Id) -> bool {
    ctx.memory(|m| m.focused()).is_some_and(|id| id != own)
}

/// A context-menu item: `icon` drawn before `label`, while the accessible
/// name a screen reader and a test read is the label alone.
pub fn menu_item(ui: &mut egui::Ui, icon: &str, label: &str) -> egui::Response {
    let r = ui.button(format!("{icon} {label}"));
    let enabled = r.enabled();
    let name = label.to_owned();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    r
}

/// [`list_row`] led by a connection kind's icon: its product logo, or its
/// glyph in the product's colour (CR-0036).
pub fn conn_list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: crate::icons::ConnIcon,
    title: &str,
    subtitle: Option<&str>,
) -> egui::Response {
    use crate::icons::ConnTint;
    let art = match (icon.image, icon.tint) {
        (Some(uri), _) => RowArt::Uri(uri),
        (None, ConnTint::Brand(c)) => RowArt::Tinted(icon.glyph, c),
        (None, ConnTint::Text) => RowArt::Tinted(icon.glyph, theme.text),
        (None, ConnTint::Default) => RowArt::Glyph(icon.glyph),
    };
    list_row_art(ui, theme, selected, art, title, subtitle)
}

pub fn list_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: &str,
    title: &str,
    subtitle: Option<&str>,
) -> egui::Response {
    list_row_image(ui, theme, selected, None, icon, title, subtitle)
}

/// [`list_row`] whose icon may be a picture: `image` is an egui image
/// URI drawn at the glyph's size, with `icon` the glyph when it is
/// `None` (an uploaded repository icon, a workspace logo).
pub fn list_row_image(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    image: Option<&str>,
    icon: &str,
    title: &str,
    subtitle: Option<&str>,
) -> egui::Response {
    let art = match image {
        Some(uri) => RowArt::Uri(uri),
        None => RowArt::Glyph(icon),
    };
    list_row_art(ui, theme, selected, art, title, subtitle)
}

/// [`list_row`] drawn with a [`TreeIcon`], so a caller can hand over
/// whatever [`crate::fileicons::for_kind`] gave it without caring which
/// of the two it is (FEAT-0084).
///
/// This exists rather than reusing [`list_row_image`]'s URI because an
/// `include_image!` source registers its bytes with the context the first
/// time the **source** is drawn; a bare URI has nothing to register and
/// renders as a broken image.
pub fn list_row_icon(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    icon: &TreeIcon,
    title: &str,
    subtitle: Option<&str>,
) -> egui::Response {
    let art = match icon {
        TreeIcon::Glyph(g) => RowArt::Glyph(g),
        TreeIcon::Image(src) => RowArt::Source(src),
    };
    list_row_art(ui, theme, selected, art, title, subtitle)
}

/// What a row draws in its icon column.
enum RowArt<'a> {
    Glyph(&'a str),
    /// An egui image URI whose bytes the context already holds.
    Uri(&'a str),
    /// An embedded source, which registers its own bytes when drawn.
    Source(&'a egui::ImageSource<'static>),
    /// A glyph in a colour of its own, hovered or not: a product's.
    Tinted(&'a str, egui::Color32),
}

fn list_row_art(
    ui: &mut egui::Ui,
    theme: &Theme,
    selected: bool,
    art: RowArt<'_>,
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
    let picture_at = || {
        let size = Vec2::splat(22.0);
        (
            size,
            egui::Rect::from_center_size(
                egui::pos2(rect.left() + 12.0 + size.x / 2.0, rect.center().y),
                size,
            ),
        )
    };
    match art {
        RowArt::Uri(uri) => {
            let (size, at) = picture_at();
            egui::Image::from_uri(uri.to_string())
                .fit_to_exact_size(size)
                .paint_at(ui, at);
        }
        RowArt::Source(src) => {
            let (size, at) = picture_at();
            egui::Image::new(src.clone())
                .fit_to_exact_size(size)
                .paint_at(ui, at);
        }
        RowArt::Glyph(glyph) => {
            ui.painter().text(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                glyph,
                egui::FontId::proportional(20.0),
                icon_col,
            );
        }
        RowArt::Tinted(glyph, colour) => {
            ui.painter().text(
                egui::pos2(rect.left() + 12.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                glyph,
                egui::FontId::proportional(20.0),
                colour,
            );
        }
    }
    let text_x = rect.left() + 42.0;
    // Title and subtitle are cut to the row with an ellipsis rather than
    // painted past its right edge into the next column (BUG-0045).
    let max_w = (rect.right() - 8.0 - text_x).max(8.0);
    let line = |ui: &mut egui::Ui, text: &str, size: f32, colour: egui::Color32, y: f32| {
        let mut job = egui::text::LayoutJob::simple(
            text.to_string(),
            egui::FontId::proportional(size),
            colour,
            f32::INFINITY,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let pos = egui::pos2(text_x, y - galley.size().y / 2.0);
        ui.painter().galley(pos, galley, colour);
    };
    match subtitle {
        Some(sub) => {
            line(ui, title, 14.0, theme.text, rect.center().y - 8.0);
            line(ui, sub, 11.5, theme.text_muted, rect.center().y + 9.0);
        }
        None => line(ui, title, 14.0, theme.text, rect.center().y),
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

/// A toggle that is always visibly a button.
///
/// `egui::Button::selectable` draws **no frame while unselected**
/// (`frame_when_inactive(selected)`), so the control is invisible until the
/// pointer touches it and then sprouts a box around its text - which reads
/// as the button growing on hover, and leaves someone with no way to tell
/// it was a button at all. This keeps the frame in every state, so hovering
/// only changes the highlight (CR-0026).
///
/// Use it for a toggle that sits in a toolbar or a button row. A row in a
/// list is not one: a frame per row turns a list into a wall of buttons.
pub fn toggle_button(ui: &mut egui::Ui, on: &mut bool, text: &str) -> egui::Response {
    let r = selectable_button(ui, *on, text);
    if r.clicked() {
        *on = !*on;
    }
    r
}

/// [`toggle_button`] for a choice among several - a result tab, a view
/// picker - where the caller sets the value itself rather than flipping a
/// `bool`. Framed in every state for the same reason (CR-0026).
pub fn selectable_button(
    ui: &mut egui::Ui,
    selected: bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .selected(selected)
            .frame_when_inactive(true)
            .frame(true),
    )
}

/// The one form button: a field's height with the standard padding, an
/// optional `icon` (empty for none) in the accent colour before `label`
/// (CR-0080). [`primary_button`] and [`danger_button`] are the same size;
/// buttons side by side keep [`BUTTON_GAP`] between them.
pub fn button(ui: &mut egui::Ui, theme: &Theme, icon: &str, label: &str) -> egui::Response {
    let text = crate::toolbar::item_text(ui, theme, icon, label);
    sized_button(ui, egui::Button::new(text), label)
}

/// A form button at a field's height with [`BUTTON_PADDING`].
/// `name` is what a screen reader and a test read: the label alone, never
/// the icon glyph drawn before it.
pub(crate) fn sized_button(
    ui: &mut egui::Ui,
    button: egui::Button<'_>,
    name: &str,
) -> egui::Response {
    let h = field_height(ui);
    let r = ui
        .scope(|ui| {
            ui.spacing_mut().button_padding = BUTTON_PADDING;
            ui.add(button.min_size(Vec2::new(0.0, h)))
        })
        .inner;
    let enabled = r.enabled();
    let name = name.to_owned();
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    r
}

/// The words of a button's text without the icon glyph it may lead with:
/// icon fonts live in the Private Use Areas.
pub(crate) fn plain_label(text: &str) -> &str {
    split_glyph(text).1
}

/// A button's text split into the icon glyph it leads with (if any) and
/// its words. Icon fonts live in the Private Use Areas.
pub(crate) fn split_glyph(text: &str) -> (Option<&str>, &str) {
    let mut chars = text.char_indices();
    match chars.next() {
        Some((_, c))
            if ('\u{E000}'..='\u{F8FF}').contains(&c)
                || ('\u{F0000}'..='\u{10FFFD}').contains(&c) =>
        {
            match chars.next() {
                Some((i, _)) => (Some(&text[..i]), text[i..].trim_start()),
                None => (Some(text), ""),
            }
        }
        _ => (None, text),
    }
}

/// A primary (accent-filled) button, the same size as [`button`].
pub fn primary_button(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    sized_button(
        ui,
        egui::Button::new(RichText::new(text).color(egui::Color32::WHITE))
            .fill(theme.accent)
            .corner_radius(CornerRadius::same(5)),
        plain_label(text),
    )
}

/// A danger (error-coloured text) button, the same size as [`button`].
pub fn danger_button(ui: &mut egui::Ui, theme: &Theme, text: &str) -> egui::Response {
    sized_button(
        ui,
        egui::Button::new(RichText::new(text).color(theme.error)),
        plain_label(text),
    )
}

/// `Ui::selectable_label` that keeps its size under the pointer - for list
/// and tree rows, where a frame per row is wrong (see [`toggle_button`]).
///
/// egui 0.35 sizes a button's inner margin as `button_padding - stroke` of
/// the current state, and draws an unselected selectable button with no
/// frame (so no stroke) while inactive. The theme gives inactive widgets a
/// 1 px outline (fields need it), so an inactive row came out a pixel
/// short on every side and grew to full size on hover - rows, links and
/// tabs "magnifying" in Containers, Redis and NoSQL (owner, 2026-11-07).
/// With no inactive stroke the margin is the same in every state; hover
/// only fills and outlines.
pub fn selectable_label<'a>(
    ui: &mut egui::Ui,
    selected: bool,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    add_selectable(ui, egui::Button::selectable(selected, atoms))
}

/// `Ui::selectable_value` through [`selectable_label`].
pub fn selectable_value<'a, V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let mut r = selectable_label(ui, *current == value, atoms);
    if r.clicked() && *current != value {
        *current = value;
        r.mark_changed();
    }
    r
}

/// `Ui::toggle_value` through [`selectable_label`].
pub fn toggle_value<'a>(
    ui: &mut egui::Ui,
    on: &mut bool,
    atoms: impl egui::IntoAtoms<'a>,
) -> egui::Response {
    let mut r = selectable_label(ui, *on, atoms);
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    r
}

/// `ui.add(button)` for an `egui::Button::selectable(..)` with extra
/// styling, with the same fixed size as [`selectable_label`].
pub fn add_selectable(ui: &mut egui::Ui, button: egui::Button<'_>) -> egui::Response {
    let saved = ui.visuals().widgets.inactive.bg_stroke;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    let r = ui.add(button);
    ui.visuals_mut().widgets.inactive.bg_stroke = saved;
    r
}

/// The size of a [`Mark`] in a dropdown row or a closed dropdown.
pub const MARK_SIZE: f32 = 16.0;
/// The gap between a [`Mark`] and the name after it.
const MARK_GAP: f32 = 6.0;

/// What a dropdown entry wears before its name (CR-0081): its kind's
/// picture - a product logo, a workspace's logo, a Material item picture -
/// or a glyph in its kind's colour (a brand, an environment, an activity),
/// or a workspace's lettered tile when it has no logo (CR-0082).
/// `None` colour is the control's own text colour.
#[derive(Clone)]
pub struct Mark {
    image: Option<egui::Image<'static>>,
    tile: Option<String>,
    glyph: String,
    colour: Option<egui::Color32>,
    size: f32,
}

impl Mark {
    /// A glyph, in `colour` or the control's own colour.
    pub fn glyph(glyph: impl Into<String>, colour: Option<egui::Color32>) -> Self {
        Self {
            image: None,
            tile: None,
            glyph: glyph.into(),
            colour,
            size: MARK_SIZE,
        }
    }

    /// A picture (a workspace's logo, an embedded image).
    pub fn image(image: egui::Image<'static>) -> Self {
        Self {
            image: Some(image),
            tile: None,
            glyph: String::new(),
            colour: None,
            size: MARK_SIZE,
        }
    }

    /// `name`'s lettered tile ([`crate::lettered`]): what a workspace
    /// without a logo wears (CR-0082).
    pub fn tile(name: impl Into<String>) -> Self {
        Self {
            image: None,
            tile: Some(name.into()),
            glyph: String::new(),
            colour: None,
            size: MARK_SIZE,
        }
    }

    /// A connection kind, a shell or a CLI as it is drawn everywhere else:
    /// its logo, or its glyph in its brand colour.
    pub fn conn(icon: crate::icons::ConnIcon, theme: &Theme) -> Self {
        match icon.image {
            Some(uri) => Self::image(egui::Image::from_uri(uri)),
            None => Self::glyph(icon.glyph, icon.fixed_colour(theme)),
        }
    }

    /// An item or file kind from [`crate::fileicons`]: its Material
    /// picture, or the plain glyph when Material icons are off.
    pub fn tree(icon: TreeIcon) -> Self {
        match icon {
            TreeIcon::Image(src) => Self::image(egui::Image::new(src)),
            TreeIcon::Glyph(g) => Self::glyph(g, None),
        }
    }

    /// The same mark at another size (a workspace's logo is drawn larger).
    pub fn sized(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    fn atom(self) -> egui::Atom<'static> {
        let size = egui::vec2(self.size, self.size);
        match self.image {
            Some(img) => img.fit_to_exact_size(size).into(),
            None => {
                let text = RichText::new(self.glyph).size(self.size * 0.9);
                match self.colour {
                    Some(c) => text.color(c).into(),
                    None => text.into(),
                }
            }
        }
    }

    fn paint(&self, ui: &egui::Ui, left_centre: egui::Pos2, fallback: egui::Color32) {
        let rect = egui::Rect::from_min_size(
            egui::pos2(left_centre.x, left_centre.y - self.size / 2.0),
            egui::vec2(self.size, self.size),
        );
        if let Some(name) = &self.tile {
            crate::lettered::paint(ui.painter(), rect, name);
            return;
        }
        match &self.image {
            Some(img) => img
                .clone()
                .fit_to_exact_size(rect.size())
                .paint_at(ui, rect),
            None => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &self.glyph,
                    egui::FontId::proportional(self.size * 0.9),
                    self.colour.unwrap_or(fallback),
                );
            }
        }
    }
}

/// A dropdown row led by its kind's [`Mark`] (CR-0081), through
/// [`selectable_label`].
pub fn mark_selectable(
    ui: &mut egui::Ui,
    selected: bool,
    mark: Mark,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let text: egui::WidgetText = text.into();
    let Some(name) = &mark.tile else {
        return selectable_label(ui, selected, (mark.atom(), text));
    };
    // A tile is painted, not an atom: keep its room, then paint into it.
    let id = egui::Id::new("hs_ui_mark_tile");
    let room = egui::Atom::custom(id, Vec2::splat(mark.size));
    let saved = ui.visuals().widgets.inactive.bg_stroke;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::NONE;
    let r = egui::Button::selectable(selected, (room, text)).atom_ui(ui);
    ui.visuals_mut().widgets.inactive.bg_stroke = saved;
    if let Some(rect) = r.rect(id) {
        crate::lettered::paint(ui.painter(), rect, name);
    }
    r.response
}

/// [`selectable_value`] led by its kind's [`Mark`] (CR-0081).
pub fn mark_selectable_value<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    mark: Mark,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    let mut r = mark_selectable(ui, *current == value, mark, text);
    if r.clicked() && *current != value {
        *current = value;
        r.mark_changed();
    }
    r
}

/// A dropdown whose closed box shows the selected value with its kind's
/// [`Mark`] before the name (CR-0081), the same mark its rows wear.
/// `combo` carries the id and width; its selected text is `text`.
pub fn mark_combo<R>(
    ui: &mut egui::Ui,
    combo: egui::ComboBox,
    mark: Mark,
    text: impl Into<egui::WidgetText>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    let text = match text.into() {
        egui::WidgetText::Text(t) => RichText::new(t).into(),
        t => t,
    };
    let mut job = std::sync::Arc::unwrap_or_clone(text.into_layout_job(
        ui.style(),
        egui::FontSelection::Style(egui::TextStyle::Button),
        egui::Align::Center,
    ));
    let room = mark.size + MARK_GAP;
    match job.sections.first_mut() {
        Some(first) => first.leading_space += room,
        None => job.append(
            " ",
            room,
            egui::TextFormat {
                font_id: egui::TextStyle::Button.resolve(ui.style()),
                color: egui::Color32::PLACEHOLDER,
                ..Default::default()
            },
        ),
    }
    let r = combo.selected_text(job).show_ui(ui, add_contents);
    let at = egui::pos2(
        r.response.rect.left() + ui.spacing().button_padding.x,
        r.response.rect.center().y,
    );
    let fallback = ui.style().interact(&r.response).text_color();
    mark.paint(ui, at, fallback);
    r
}

#[cfg(test)]
mod tests {

    /// A marked page title puts its mark on the title's middle line, in
    /// the mark's colour (owner, 2026-11-07).
    #[test]
    fn a_marked_title_shares_the_titles_middle_line() {
        let theme = Theme::dark();
        let red = egui::Color32::from_rgb(220, 40, 40);
        let ctx = egui::Context::default();
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            page_title_marked(
                ui,
                &theme,
                Mark::glyph("M", Some(red)).sized(26.0),
                "Hashes",
                Some("Every digest"),
            );
        });
        let mut found = Vec::new();
        fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect, egui::Color32)>) {
            match shape {
                egui::Shape::Text(t) => out.push((
                    t.galley.text().to_string(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                    t.override_text_color.unwrap_or(t.fallback_color),
                )),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut found));
        let get = |s: &str| found.iter().find(|(x, _, _)| x == s).cloned().expect(s);
        let (_, mark, colour) = get("M");
        let (_, title, _) = get("Hashes");
        assert_eq!(colour, red);
        assert!(mark.right() < title.left());
        assert!((mark.center().y - title.center().y).abs() < 3.0);
    }

    #[test]
    fn a_buttons_accessible_name_drops_its_icon_glyph() {
        assert_eq!(super::plain_label("\u{E0A0} Save"), "Save");
        assert_eq!(super::plain_label("\u{F0001} Close"), "Close");
        assert_eq!(super::plain_label("Save"), "Save");
        assert_eq!(super::plain_label("Check Now"), "Check Now");
    }
    use super::*;

    #[test]
    fn ticking_appends_in_pick_order_and_unticking_removes() {
        let v = toggle_list_item("", "id");
        assert_eq!(v, "id");
        let v = toggle_list_item(&v, "name");
        assert_eq!(v, "id, name");
        let v = toggle_list_item(&v, "age");
        assert_eq!(v, "id, name, age");
        let v = toggle_list_item(&v, "name");
        assert_eq!(v, "id, age");
        let v = toggle_list_item(&v, "id");
        assert_eq!(v, "age");
        assert_eq!(toggle_list_item(&v, "age"), "");
    }

    /// Run `add` for three frames - a grid sizes its columns from the frame
    /// before, and a page's label column from the frame before that - and
    /// return what the last one recorded.
    fn settled<T: Default>(mut add: impl FnMut(&mut egui::Ui, &mut T)) -> T {
        let ctx = egui::Context::default();
        let mut out = T::default();
        for _ in 0..3 {
            out = T::default();
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| add(ui, &mut out));
        }
        out
    }

    #[test]
    fn a_marked_dropdown_leaves_room_for_its_mark_before_the_name() {
        // CR-0081: the closed box draws the mark where the name used to
        // start, so the box grows by exactly the mark and its gap.
        let (plain, marked) = settled(|ui, out: &mut (f32, f32)| {
            out.0 = egui::ComboBox::from_id_salt("plain")
                .width(1.0)
                .selected_text("Production")
                .show_ui(ui, |_| {})
                .response
                .rect
                .width();
            out.1 = mark_combo(
                ui,
                egui::ComboBox::from_id_salt("marked").width(1.0),
                Mark::glyph(crate::icons::ENVIRONMENT, Some(egui::Color32::RED)),
                "Production",
                |_| {},
            )
            .response
            .rect
            .width();
        });
        assert!(
            (marked - plain - (MARK_SIZE + MARK_GAP)).abs() < 0.5,
            "plain {plain}, marked {marked}"
        );
    }

    #[test]
    fn labels_start_left_sub_labels_indent_and_controls_share_one_x() {
        let (label, sub, control_a, control_b) = settled(|ui, out: &mut (f32, f32, f32, f32)| {
            field_grid(ui, "t", |ui| {
                out.0 = grid_label(ui, "Short", 0.0).left();
                out.2 = ui.button("A").rect.left();
                ui.end_row();
                out.1 = grid_label(ui, "A much longer sub-setting label", SUB_INDENT).left();
                out.3 = ui.button("B").rect.left();
                ui.end_row();
            });
        });
        assert!(
            (sub - label - SUB_INDENT).abs() < 0.5,
            "label at {label}, sub-label at {sub}"
        );
        assert!(
            (control_a - control_b).abs() < 0.5,
            "controls at {control_a} and {control_b}"
        );
        assert!(control_a > sub, "the controls start past the labels");
    }

    #[test]
    fn a_row_hint_sits_under_its_control_at_the_control_x() {
        let theme = Theme::dark();
        let (control, hint, control_bottom, hint_top) =
            settled(|ui, out: &mut (f32, f32, f32, f32)| {
                field_grid(ui, "t", |ui| {
                    label(ui, "Undo limit");
                    let c = ui.button("Control").rect;
                    ui.end_row();
                    let h = row_hint(ui, &theme, "What the control does.").rect;
                    label(ui, "Log level");
                    let _ = ui.button("Next");
                    ui.end_row();
                    *out = (c.left(), h.left(), c.bottom(), h.top());
                });
            });
        assert!(
            (control - hint).abs() < 0.5,
            "control at {control}, hint at {hint}"
        );
        assert!(hint_top >= control_bottom, "the hint is under its control");
    }

    #[test]
    fn a_grid_checkbox_starts_at_the_control_column() {
        let (control, plain, described) = settled(|ui, out: &mut (f32, f32, f32)| {
            field_grid(ui, "t", |ui| {
                label(ui, "A label");
                out.0 = ui.button("Control").rect.left();
                ui.end_row();
                let mut on = false;
                out.1 = checkbox_row(ui, &mut on, "Copy on select").rect.left();
                out.2 = checkbox_row_with_description(ui, &mut on, "Detect links", "Underlined.")
                    .rect
                    .left();
            });
        });
        assert!(
            (control - plain).abs() < 0.5,
            "control at {control}, checkbox at {plain}"
        );
        assert!(
            (control - described).abs() < 0.5,
            "control at {control}, described checkbox at {described}"
        );
    }

    #[test]
    fn grids_on_one_page_share_the_label_column() {
        let (short_card, long_card) = settled(|ui, out: &mut (f32, f32)| {
            page_labels(ui, "page", |ui| {
                field_grid(ui, "a", |ui| {
                    label(ui, "Short");
                    out.0 = ui.button("A").rect.left();
                    ui.end_row();
                });
                field_grid(ui, "b", |ui| {
                    label(ui, "A label much longer than the minimum column width");
                    out.1 = ui.button("B").rect.left();
                    ui.end_row();
                });
            });
        });
        assert!(
            (short_card - long_card).abs() < 0.5,
            "controls at {short_card} and {long_card}"
        );
    }

    #[test]
    fn button_primary_and_danger_are_one_height() {
        let theme = Theme::dark();
        let (plain, primary, danger) = settled(|ui, out: &mut (f32, f32, f32)| {
            ui.horizontal(|ui| {
                out.0 = button(ui, &theme, "", "Plain").rect.height();
                out.1 = primary_button(ui, &theme, "Primary").rect.height();
                out.2 = danger_button(ui, &theme, "Danger").rect.height();
            });
        });
        assert!((plain - primary).abs() < 0.5, "{plain} vs {primary}");
        assert!((plain - danger).abs() < 0.5, "{plain} vs {danger}");
    }

    /// One frame with the pointer at `pointer` (if any); the row's rect
    /// and whether egui saw it hovered.
    fn row_frame(
        ctx: &egui::Context,
        pointer: Option<egui::Pos2>,
        selected: bool,
    ) -> (egui::Rect, bool) {
        let mut input = egui::RawInput::default();
        input.events.extend(pointer.map(egui::Event::PointerMoved));
        let mut out = (egui::Rect::NOTHING, false);
        let _ = ctx.run_ui(input, |ui| {
            let r = selectable_label(ui, selected, "A list row");
            out = (r.rect, r.hovered());
        });
        out
    }

    /// Owner, 2026-11-07: rows must not magnify on hover - a fill or a
    /// border only. The hovered rect is the resting rect, selected or not.
    #[test]
    fn a_selectable_row_keeps_its_rect_under_the_pointer() {
        for selected in [false, true] {
            let ctx = egui::Context::default();
            Theme::dark().apply(&ctx);
            let mut resting = egui::Rect::NOTHING;
            for _ in 0..3 {
                resting = row_frame(&ctx, None, selected).0;
            }
            let mut hovered = (egui::Rect::NOTHING, false);
            for _ in 0..3 {
                hovered = row_frame(&ctx, Some(resting.center()), selected);
            }
            assert!(
                hovered.1,
                "the pointer is over the row (selected {selected})"
            );
            assert_eq!(hovered.0, resting, "selected {selected}");
        }
    }

    #[test]
    fn typed_lists_are_normalised_on_toggle() {
        assert_eq!(toggle_list_item(" a ,, b,", "c"), "a, b, c");
        assert_eq!(toggle_list_item("a,b", "a"), "b");
        assert_eq!(list_items(" x , ,y"), vec!["x", "y"]);
    }
}
