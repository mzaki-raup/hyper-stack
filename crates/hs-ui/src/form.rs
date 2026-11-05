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

/// Two-column label/field grid for a section.
pub fn field_grid<R>(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let h = field_height(ui);
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([18.0, 10.0])
        .min_col_width(170.0)
        .show(ui, |ui| {
            // Every cell in a form row is a field's height tall, so a
            // checkbox or a ComboBox beside a label centres on the same
            // line as the label's text instead of hugging the row's top.
            ui.spacing_mut().interact_size.y = h;
            add_contents(ui)
        })
        .inner
}

/// A row label inside [`field_grid`]: muted, vertically centred with the field.
pub fn label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized(
        [170.0, field_height(ui)],
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
    if options.is_empty() {
        return text_field(ui, value, hint);
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width().clamp(FIELD_MIN_WIDTH, FIELD_MAX_WIDTH) - 32.0).max(80.0);
        let mut r = text_field_width(ui, value, hint, w);
        let caret = icon_button(ui, theme, crate::icons::CARET_DOWN, 14.0, false);
        let mut chosen = false;
        egui::Popup::menu(&caret).show(|ui| {
            ui.set_min_width(w);
            egui::ScrollArea::vertical()
                .max_height(240.0)
                .show(ui, |ui| {
                    for o in options {
                        if ui.selectable_label(value == o, o).clicked() {
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

/// A [`hint`] under a form row, on a row of its own inside [`field_grid`].
/// A grid cell is centred in its row, so a description stacked under the
/// control makes that row tall and drags the label below the control's
/// middle; on its own row the control's label stays level with it.
pub fn field_hint_row(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label("");
    hint(ui, theme, text);
    ui.end_row();
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
/// workspace's own logo on Home, falling back to a glyph when none is
/// set, so the column says which workspace these pins belong to rather
/// than only naming it. Owner's requirement, 2026-10-17.
pub fn sidebar_header_icon(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<egui::Image<'_>>,
    fallback: &str,
    title: &str,
    actions: &[(&str, &str)],
) -> Option<usize> {
    sidebar_header_inner(ui, theme, Some((icon, fallback)), title, actions)
}

fn sidebar_header_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    leading: Option<(Option<egui::Image<'_>>, &str)>,
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
                if let Some((icon, fallback)) = leading {
                    // Home's header names the open workspace, and its
                    // logo is the mark that says which project this is.
                    const SIZE: f32 = 22.0;
                    match icon {
                        Some(img) => {
                            ui.add(img.fit_to_exact_size(egui::Vec2::splat(SIZE)));
                        }
                        None => {
                            ui.label(
                                egui::RichText::new(fallback)
                                    .size(SIZE - 2.0)
                                    .color(theme.text_muted),
                            );
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
        if ui.button(labels.edit).clicked() {
            out.edit = true;
            ui.close();
        }
        if ui.button(labels.remove).clicked() {
            out.remove = true;
            ui.close();
        }
        ui.separator();
        if pinned {
            if ui.button(labels.unpin).clicked() {
                out.pin = Some(false);
                ui.close();
            }
        } else if ui.button(labels.pin).clicked() {
            out.pin = Some(true);
            ui.close();
        }
    });
    out
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
