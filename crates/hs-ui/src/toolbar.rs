//! Tool toolbar helpers. The shell lays every tool's toolbar out in a
//! wrapping row, so a narrow window pushes buttons onto a second row
//! instead of clipping them off the right edge - and the hard-right
//! group stays in the corner while that happens.

use egui::{Align, Id, Layout, Rect, UiBuilder};

/// Where the shell leaves the toolbar's full rectangle for [`hard_right`],
/// and where that function leaves the width it needs for the next frame.
const OUTER: &str = "hs-toolbar-outer";
const WIDTH: &str = "hs-toolbar-right-width";

/// What the corner is assumed to need before it has ever been measured.
/// Every tool's group is the AI Assistant button, sometimes with a short
/// status beside it.
pub const RIGHT_RESERVE: f32 = 170.0;

/// How wide the hard-right corner wants to be, for the shell to hold back
/// before it hands the row to a tool. One frame behind the truth, which
/// only matters the first time a tool's group changes width.
pub fn right_reserve(ctx: &egui::Context, tab: Id) -> f32 {
    ctx.data(|d| d.get_temp::<f32>(tab.with(WIDTH)))
        .unwrap_or(RIGHT_RESERVE)
}

/// Tell [`hard_right`] which rectangle the whole toolbar occupies, so the
/// corner it draws in is the window's, not the narrowed row's. The shell
/// calls this before handing the row to a tool.
pub fn set_outer(ui: &mut egui::Ui, tab: Id, rect: Rect) {
    ui.data_mut(|d| d.insert_temp(tab.with(OUTER), rect));
    ui.data_mut(|d| d.insert_temp(Id::new(OUTER), tab));
}

/// Forget the toolbar's rectangle once the toolbar has been drawn.
///
/// Without this the marker outlives the toolbar, and a `hard_right`
/// called anywhere else - a settings form putting its Remove button on
/// the right, say - would draw itself into the corner of whatever
/// toolbar happened to be drawn last and leave the page it was actually
/// in laid out around a widget that is not there (2026-10-26).
pub fn clear_outer(ui: &mut egui::Ui) {
    ui.data_mut(|d| d.remove::<Id>(Id::new(OUTER)));
}

/// The toolbar's hard-right group: the AI Assistant control and whatever
/// status sits beside it.
///
/// This is pinned to the **top right of the first row** rather than laid
/// out with everything else. A right-to-left scope only ever gets the
/// width left on the current row, so when the buttons wrapped the control
/// went down to the second row with them - and the owner's rule is that
/// the assistant is always the hard right item of a tool toolbar, which
/// means the corner, not wherever the flow happened to end
/// (owner's requirement, 2026-10-26).
///
/// The shell holds that corner back from the wrapping row (see
/// [`right_reserve`]), so nothing is drawn underneath it.
pub fn hard_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let tab = ui.data(|d| d.get_temp::<Id>(Id::new(OUTER)));
    let outer = tab.and_then(|t| ui.data(|d| d.get_temp::<Rect>(t.with(OUTER))));
    // One row tall, whatever the strip grew to. `min_rect` is every row
    // the buttons took by the time the corner is drawn, and centring in
    // that put the assistant between the rows rather than on the first
    // one (owner's report on Logs, CR-0027).
    let Some((tab, outer)) = tab.zip(outer) else {
        // No shell around us (a test, or a tool drawn on its own): fall
        // back to the old behaviour rather than drawing nothing.
        ui.with_layout(Layout::right_to_left(Align::Center), add);
        return;
    };
    let height = ui.spacing().interact_size.y;
    let corner = Rect::from_min_max(
        egui::pos2(outer.left(), outer.top()),
        egui::pos2(outer.right(), outer.top() + height),
    );
    let used = ui
        .scope_builder(
            UiBuilder::new()
                .max_rect(corner)
                .layout(Layout::right_to_left(Align::Center)),
            add,
        )
        .response
        .rect
        .width();
    // Report what the corner actually took, so the next frame's row stops
    // short of it. `max` keeps a sizing pass from reporting zero.
    ui.data_mut(|d| {
        d.insert_temp(
            tab.with(WIDTH),
            (used + ui.spacing().item_spacing.x).max(ui.spacing().item_spacing.x),
        )
    });
}

/// Break a wrapping toolbar onto a new row, with a rule between them.
///
/// A `horizontal_wrapped` row has no "end row" of its own: it wraps when
/// a widget does not fit. Allocating the **full** width of the toolbar
/// therefore does three things at once - the current row ends, this
/// strip takes a row of its own, and whatever comes next starts a third
/// - which is exactly a row break with a rule in it.
///
/// Taking `max_rect().width()` rather than `available_width()` matters:
/// the latter is whatever is left of the current row, which on a nearly
/// full row is a stub that fits where it is and breaks nothing. The
/// width is the row's, which the shell has already narrowed to keep the
/// hard-right corner clear, so the rule stops short of the AI Assistant
/// rather than running under it.
pub fn row_break(ui: &mut egui::Ui, theme: &crate::theme::Theme) {
    const AIR: f32 = 5.0;
    let w = ui.max_rect().width().max(1.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, AIR * 2.0 + 1.0), egui::Sense::hover());
    // The rule runs the toolbar's whole width, not the row's: the row
    // stops short of the hard-right corner, and a rule that stopped with
    // it left a gap under the assistant (owner's report, CR-0027).
    let range = outer_rect(ui)
        .map(|o| o.x_range())
        .unwrap_or(rect.x_range());
    ui.painter()
        .hline(range, rect.center().y, egui::Stroke::new(1.0, theme.border));
}

/// The whole toolbar's rectangle, as the shell reported it with
/// [`set_outer`]; `None` outside a shell.
pub fn outer_rect(ui: &egui::Ui) -> Option<Rect> {
    let tab = ui.data(|d| d.get_temp::<Id>(Id::new(OUTER)))?;
    ui.data(|d| d.get_temp::<Rect>(tab.with(OUTER)))
}

/// How much air a toolbar button has around its label. Bigger than
/// egui's default (4, 2), so a toolbar reads as a row of buttons rather
/// than as text that happens to be clickable (owner's report, CR-0027).
pub const BUTTON_PADDING: egui::Vec2 = egui::vec2(10.0, 6.0);

/// The toolbar's own metrics, applied once to the strip: medium buttons,
/// and every control in the row as tall as a form field so buttons,
/// dropdowns and status chips share one line.
pub fn strip_style(ui: &mut egui::Ui) {
    let h = crate::form::field_height(ui);
    let s = ui.spacing_mut();
    s.item_spacing.x = 8.0;
    s.button_padding = BUTTON_PADDING;
    s.interact_size.y = h;
}

/// A toolbar item's text: the icon in the accent, the label in the
/// ordinary text colour, so the row reads as a toolbar without the
/// labels shouting (owner's report, CR-0027).
pub fn item_text(
    ui: &egui::Ui,
    theme: &crate::theme::Theme,
    icon: &str,
    label: &str,
) -> egui::text::LayoutJob {
    item_text_in(ui, theme.accent, theme.text, icon, label)
}

/// [`item_text`] with the colours spelled out: a disabled button needs
/// both of them muted, since a `LayoutJob`'s own colours survive egui's
/// dimming of the widget around them.
fn item_text_in(
    ui: &egui::Ui,
    icon_colour: egui::Color32,
    text_colour: egui::Color32,
    icon: &str,
    label: &str,
) -> egui::text::LayoutJob {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    if !icon.is_empty() {
        job.append(
            icon,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: icon_colour,
                ..Default::default()
            },
        );
    }
    if !label.is_empty() {
        job.append(
            label,
            if icon.is_empty() { 0.0 } else { 6.0 },
            egui::TextFormat {
                font_id: font,
                color: text_colour,
                ..Default::default()
            },
        );
    }
    job
}

/// A toolbar button: [`item_text`] in a frame of the strip's own size.
pub fn btn(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    icon: &str,
    label: &str,
) -> egui::Response {
    let text = item_text(ui, theme, icon, label);
    ui.button(text)
}

/// A toolbar button that is disabled with a reason on it.
pub fn btn_enabled(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    enabled: bool,
    icon: &str,
    label: &str,
) -> egui::Response {
    let text = if enabled {
        item_text(ui, theme, icon, label)
    } else {
        item_text_in(ui, theme.text_muted, theme.text_muted, icon, label)
    };
    ui.add_enabled(enabled, egui::Button::new(text))
}

/// A toolbar's own tab or toggle: [`item_text`] in a frame that stays in
/// every state, so an unselected tab still reads as a button (CR-0026's
/// rule, with CR-0027's colours).
pub fn tab(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    selected: bool,
    icon: &str,
    label: &str,
) -> egui::Response {
    let text = item_text(ui, theme, icon, label);
    crate::form::selectable_button(ui, selected, text)
}

/// [`tab`] led by a Material picture in place of the glyph when `image`
/// is `Some`: an inner tab wears the picture its list does (CR-0065).
pub fn tab_image(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    selected: bool,
    image: Option<egui::ImageSource<'static>>,
    icon: &str,
    label: &str,
) -> egui::Response {
    let Some(src) = image else {
        return tab(ui, theme, selected, icon, label);
    };
    let size = egui::TextStyle::Button.resolve(ui.style()).size;
    let text = item_text(ui, theme, "", label);
    ui.add(
        egui::Button::new((
            egui::Image::new(src).fit_to_exact_size(egui::Vec2::splat(size)),
            text,
        ))
        .selected(selected)
        .frame_when_inactive(true)
        .frame(true),
    )
}

/// [`tab`] for a toggle that owns its own flag: flips `on` when clicked,
/// the way `Ui::toggle_value` does, but framed and coloured.
pub fn toggle(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    on: &mut bool,
    icon: &str,
    label: &str,
) -> egui::Response {
    let r = tab(ui, theme, *on, icon, label);
    if r.clicked() {
        *on = !*on;
    }
    r
}

/// A toolbar's status readout - Connected, Disconnected, 12 of 40 rows -
/// as a chip the size of a button beside it: the mark in the state's own
/// colour, the text in the ordinary one, and the strip's padding around
/// both, so it sits level with the buttons instead of reading as loose
/// text (owner's report, CR-0027).
pub fn status(
    ui: &mut egui::Ui,
    theme: &crate::theme::Theme,
    colour: egui::Color32,
    mark: &str,
    text: &str,
) -> egui::Response {
    let h = ui.spacing().interact_size.y;
    egui::Frame::new()
        .fill(theme.surface)
        .stroke(egui::Stroke::new(1.0, theme.border))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin {
            left: BUTTON_PADDING.x as i8,
            right: BUTTON_PADDING.x as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_min_height(h);
            ui.spacing_mut().item_spacing.x = 6.0;
            if !mark.is_empty() {
                ui.colored_label(colour, mark);
            }
            ui.label(text);
        })
        .response
}
