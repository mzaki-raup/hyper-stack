//! The multi-row tab strip (docs/04 §2.2). Tabs never scroll off-screen and
//! never shrink below the minimum width: when they do not fit on one row the
//! strip wraps onto more rows. Pinned tabs render icon-only at the start of the
//! first row.
//!
//! The same strip in `compact` form (lower rows, a file menu) is the editor's
//! row of file tabs inside a project tab.

use crate::tip::TipExt;
use egui::{Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::theme::Theme;

pub const ROW_HEIGHT: f32 = 32.0;
/// A pinned tab that shows only its icon (the browser, whose favicon says
/// which site it is far better than a truncated title).
pub const PINNED_WIDTH: f32 = 36.0;
/// A pinned tab that keeps its name. Pinning should mark a tab, not erase
/// what it says.
pub const PINNED_TEXT_WIDTH: f32 = 132.0;
pub const MAX_TAB_WIDTH: f32 = 240.0;
pub const NEW_TAB_WIDTH: f32 = 32.0;
/// Breathing room at the strip's right edge so the last close button is never
/// clipped by the window frame or the "+" slot.
pub const STRIP_RIGHT_PAD: f32 = 14.0;
/// Row height of a compact strip.
pub const COMPACT_ROW_HEIGHT: f32 = 26.0;

/// What the strip needs to know about each tab.
pub trait TabItem {
    fn id(&self) -> u64;
    fn title(&self) -> String;
    fn icon(&self) -> &str;
    /// An image to draw instead of [`TabItem::icon`], as an egui image URI
    /// (`bytes://…` for something already loaded). The browser's tabs use
    /// it for the page's favicon; everything else leaves it `None` and the
    /// glyph is drawn.
    fn icon_uri(&self) -> Option<String> {
        None
    }
    /// A short piece drawn before the title in a colour of its own: the
    /// API client's HTTP method, so the verb is picked out by colour.
    fn title_prefix(&self) -> Option<(String, Color32)> {
        None
    }
    fn pinned(&self) -> bool;
    /// Whether this tab shows only its icon once pinned. True for the
    /// browser, where the favicon identifies the page; false everywhere
    /// else, so a pinned tab keeps its name.
    fn icon_only_when_pinned(&self) -> bool {
        false
    }
    /// Whether the tab can be closed at all. A pinned Scratchpad note
    /// cannot: pinning a note is how you say it stays (owner's rule,
    /// CR-0041). The close cross and the menu's Close both go.
    fn can_close(&self) -> bool {
        true
    }
    /// Whether this tab may be unpinned from the strip (for mandatory app
    /// tabs such as Settings).
    fn can_unpin(&self) -> bool {
        true
    }
    /// A picture to draw instead of [`TabItem::icon`], already loaded as
    /// an [`egui::ImageSource`]. The shell hands over the activity's
    /// Material item icon this way, since the bytes come from
    /// `include_image!` and a bare `bytes://` uri would have nothing
    /// behind it (CR-0041).
    fn icon_image(&self) -> Option<egui::ImageSource<'static>> {
        None
    }
    /// The colour of this tab's glyph, when it has one of its own: an
    /// activity's fixed tab wears the activity's rail colour while
    /// Coloured Activity Icons is on. `None` draws it muted (CR-0043).
    fn icon_colour(&self) -> Option<Color32> {
        None
    }
    /// Draw this tab's icon as the pin, in the pinned colour, instead of
    /// its glyph or picture: a pinned Scratchpad note wears the mark so
    /// the strip says which notes are kept (owner's rule, CR-0041).
    fn pin_icon(&self) -> bool {
        false
    }
    /// Where this tab sits among the pinned ones, when the tool keeps
    /// its own pin order (the Scratchpad's notes). The strip sorts its
    /// pinned slots by this, so they read in that order rather than in
    /// the order the tabs happen to be in (CR-0041).
    fn pin_rank(&self) -> Option<usize> {
        None
    }
    /// Whether this tab is the activity's permanent home. It cannot be
    /// closed, closed-to-the-left past, pinned or duplicated, so its menu
    /// offers only Close Others and Close to the Right. Owner's
    /// requirement, 2026-10-17.
    fn permanent(&self) -> bool {
        false
    }
    /// Whether this tab has a context menu at all.
    fn has_menu(&self) -> bool {
        true
    }
    /// Whether Duplicate appears in the menu at all. False drops the
    /// entry; `can_duplicate` only greys it out. An activity whose tabs
    /// are all one document - the Scratchpad's notes - has nothing a
    /// second tab would show (owner's report, CR-0029).
    fn shows_duplicate(&self) -> bool {
        true
    }
    /// Whether Duplicate is offered. A tab bound to a connection is a view
    /// of a live session, so duplicating it would open a second session
    /// against the same server; the entry stays in the menu but disabled,
    /// because an item that silently vanishes reads as a bug rather than a
    /// rule. Owner's requirement, 2026-10-17.
    fn can_duplicate(&self) -> bool {
        true
    }
    fn dirty(&self) -> bool {
        false
    }
    /// Environment or user colour shown as a dot and the active underline.
    fn colour(&self) -> Option<Color32> {
        None
    }
    fn tooltip(&self) -> String {
        self.title()
    }
    /// The title is the app's own name, shown whole: the strip's tabs are
    /// made at least as wide as it needs (owner, 2026-11-07).
    fn whole_title(&self) -> bool {
        false
    }
}

/// How wide a pinned tab's slot is.
pub fn pinned_slot_width(icon_only: bool) -> f32 {
    if icon_only {
        PINNED_WIDTH
    } else {
        PINNED_TEXT_WIDTH
    }
}

/// Where each tab goes. Row 0 opens with the leading zone - the activity's
/// permanent tab, then the pinned ones - and the "+" button is placed after
/// the last tab of the last row.
#[derive(Debug, Clone, PartialEq)]
pub struct TabLayout {
    pub rows: Vec<Vec<usize>>, // indices into the tab list, per row
    pub tab_width: f32,
    /// Row 0's fixed slots, in the order they are drawn: the permanent tab
    /// first, then the pinned ones. Owner's requirement, 2026-10-17.
    pub lead: Vec<usize>,
    /// Each lead slot's width, in `lead`'s order: what its icon and name
    /// need, so a fixed tab never truncates its label (CR-0043).
    pub lead_widths: Vec<f32>,
}

impl TabLayout {
    pub fn row_count(&self) -> usize {
        self.rows.len().max(1)
    }
}

/// Pure layout: which tab lands on which row and how wide regular tabs are.
///
/// `pinned` and `permanent` are flags per tab; `full_width` is the strip
/// width; `min_width` and `gap` come from settings. The last row reserves
/// room for the "+" button. The permanent tab leads the strip and the
/// pinned tabs follow it, so pinning never puts a tab in front of the
/// activity's home. Owner's requirement, 2026-10-17.
// One slice per fact about each tab, read side by side; a struct of five
// slices would only rename them.
#[allow(clippy::too_many_arguments)]
pub fn compute_tab_layout(
    pinned: &[bool],
    permanent: &[bool],
    icon_only: &[bool],
    pin_rank: &[Option<usize>],
    fit_width: &[f32],
    full_width: f32,
    min_width: f32,
    gap: f32,
) -> TabLayout {
    let is_permanent = |i: usize| permanent.get(i).copied().unwrap_or(false);
    // A permanent tab keeps its name even where a pinned one would be an
    // icon: it is the activity's front page, not a bookmark. A slot that
    // shows a name is as wide as the name needs (`fit_width`, measured by
    // the strip), so it never truncates; a caller with nothing measured
    // gets the old fixed width (CR-0043).
    let slot_width = |i: usize| {
        if icon_only.get(i).copied().unwrap_or(false) && !is_permanent(i) {
            PINNED_WIDTH
        } else {
            fit_width.get(i).copied().unwrap_or(PINNED_TEXT_WIDTH)
        }
    };
    // The pinned slots follow the permanent tab, in the order the tool
    // gives them when it keeps one (the Scratchpad's sidebar), and in
    // tab order otherwise (CR-0041).
    let mut slots: Vec<usize> = (0..pinned.len())
        .filter(|&i| pinned[i] && !is_permanent(i))
        .collect();
    slots.sort_by_key(|&i| (pin_rank.get(i).copied().flatten().unwrap_or(usize::MAX), i));
    let lead: Vec<usize> = (0..pinned.len())
        .filter(|&i| is_permanent(i))
        .chain(slots)
        .collect();
    let regular: Vec<usize> = (0..pinned.len()).filter(|i| !lead.contains(i)).collect();
    let min_width = min_width.max(48.0);
    let gap = gap.max(0.0);
    let pinned_span: f32 = lead.iter().map(|&i| slot_width(i) + gap).sum();
    let first_row_avail = (full_width - pinned_span - NEW_TAB_WIDTH - gap).max(min_width);
    let other_rows_avail = (full_width - NEW_TAB_WIDTH - gap).max(min_width);

    if regular.is_empty() {
        return TabLayout {
            rows: vec![Vec::new()],
            tab_width: min_width,
            lead_widths: lead.iter().map(|&i| slot_width(i)).collect(),
            lead,
        };
    }

    // Everything on one row at a comfortable width?
    let n = regular.len() as f32;
    let one_row_width = (first_row_avail + gap) / n - gap;
    if one_row_width >= min_width {
        return TabLayout {
            rows: vec![regular],
            tab_width: one_row_width.min(MAX_TAB_WIDTH.max(min_width)),
            lead_widths: lead.iter().map(|&i| slot_width(i)).collect(),
            lead,
        };
    }

    // Wrap: as many tabs per row as fit at the minimum width, then widen them
    // to fill the row.
    let per_first = (((first_row_avail + gap) / (min_width + gap)).floor() as usize).max(1);
    let per_other = (((other_rows_avail + gap) / (min_width + gap)).floor() as usize).max(1);
    let mut rows = Vec::new();
    let mut rest: &[usize] = &regular;
    let first = per_first.min(rest.len());
    rows.push(rest[..first].to_vec());
    rest = &rest[first..];
    while !rest.is_empty() {
        let take = per_other.min(rest.len());
        rows.push(rest[..take].to_vec());
        rest = &rest[take..];
    }
    // One shared width for every row, so take the narrowest row's budget:
    // row 0 also carries the permanent/pinned lead, and sizing every row from
    // `other_rows_avail` overflowed it and hid its last tab.
    let mut fit = f32::MAX;
    for (i, row) in rows.iter().enumerate() {
        if row.is_empty() {
            continue;
        }
        let avail = if i == 0 {
            first_row_avail
        } else {
            other_rows_avail
        };
        fit = fit.min((avail + gap) / row.len() as f32 - gap);
    }
    let tab_width = fit.clamp(min_width, MAX_TAB_WIDTH.max(min_width));
    TabLayout {
        rows,
        tab_width,
        lead_widths: lead.iter().map(|&i| slot_width(i)).collect(),
        lead,
    }
}

/// Number of rows a strip needs (used to size the title-bar region before drawing).
pub fn tab_row_count(
    pinned: &[bool],
    permanent: &[bool],
    icon_only: &[bool],
    full_width: f32,
    min_width: f32,
    gap: f32,
) -> usize {
    compute_tab_layout(
        pinned,
        permanent,
        icon_only,
        &[],
        &[],
        full_width,
        min_width,
        gap,
    )
    .row_count()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabMenuAction {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    Pin,
    Unpin,
    Duplicate,
    /// Compact strips only: copy what the tab shows (a file path).
    CopyPath,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct TabStripResponse {
    pub activated: Option<u64>,
    pub close: Option<u64>,
    pub menu: Option<(u64, TabMenuAction)>,
    /// Drag-reorder: move tab `from` to index `to` (in display order).
    pub reorder: Option<(usize, usize)>,
    pub new_tab: bool,
}

pub struct TabStripLabels {
    pub new_tab: String,
    pub close: String,
    pub close_others: String,
    pub close_left: String,
    pub close_right: String,
    pub pin: String,
    pub unpin: String,
    pub duplicate: String,
    /// Why Duplicate is refused, shown on the disabled entry.
    pub duplicate_blocked: String,
    /// Compact strips only.
    pub copy_path: String,
}

pub struct TabStrip<'a> {
    pub theme: &'a Theme,
    pub min_width: f32,
    pub gap: f32,
    pub active: Option<u64>,
    pub labels: &'a TabStripLabels,
    /// Lower rows and a menu of Close / Close Others / Close Left / Close
    /// Right / Copy Path (no pin and no duplicate).
    pub compact: bool,
    /// Draw the add-tab button at the end of the last row. The window
    /// strip does not: a tab is opened from the activity's own sidebar or
    /// from Home, and a + there opened an untitled one nobody asked for.
    /// Owner's requirement, 2026-10-17.
    pub show_new_tab: bool,
    /// A row's height, from Settings -> Appearance -> Tabs. `None` takes
    /// the built-in height for this kind of strip, which is what the
    /// tests and a strip drawn outside the app use.
    pub height: Option<f32>,
}

/// Which of the "close the others" entries a tab's menu offers. An
/// entry that would close nothing is not drawn, and neither is one that
/// would close exactly what Close Others closes - a second way to do the
/// same thing reads as two different things (owner's report, CR-0029).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CloseScope {
    /// There is a closable tab before this one.
    pub left: bool,
    /// There is a closable tab after it.
    pub right: bool,
}

impl CloseScope {
    /// Close Others closes something.
    pub fn others(self) -> bool {
        self.left || self.right
    }
    /// Close to the Left closes something, and leaves something.
    pub fn close_left(self) -> bool {
        self.left && self.right
    }
    /// Close to the Right, likewise.
    pub fn close_right(self) -> bool {
        self.left && self.right
    }
}

/// Which tabs each one has closable neighbours on either side of.
/// `closable` is per tab, in the order the strip was given them.
///
/// A tab that is not closable is the permanent one or a pinned one, and
/// both are drawn in the lead zone at the strip's head - so every
/// closable tab is to its right whatever its place in the list. Taking
/// the list's order there would offer Close to the Left for tabs that
/// are drawn to its right (owner's report, CR-0029).
pub fn close_scopes(closable: &[bool]) -> Vec<CloseScope> {
    let any = closable.iter().any(|&c| c);
    (0..closable.len())
        .map(|i| {
            if closable[i] {
                CloseScope {
                    left: closable[..i].iter().any(|&c| c),
                    right: closable[i + 1..].iter().any(|&c| c),
                }
            } else {
                CloseScope {
                    left: false,
                    right: any,
                }
            }
        })
        .collect()
}

/// What a tab's parts measure at a given row height. Everything scales
/// with the row, so raising the height in Settings makes the icon, the
/// title and the close cross grow with it rather than leaving a taller
/// box around the same small tab (owner's requirement, CR-0029).
struct TabMetrics {
    font: f32,
    icon: f32,
    close_glyph: f32,
    close_box: f32,
}

impl TabStrip<'_> {
    fn row_h(&self) -> f32 {
        self.height.unwrap_or(if self.compact {
            COMPACT_ROW_HEIGHT
        } else {
            ROW_HEIGHT
        })
    }

    /// The proportions the strip was drawn at before the height became a
    /// setting: a 32 px row with a 14 px title.
    fn metrics(&self) -> TabMetrics {
        let h = self.row_h();
        TabMetrics {
            font: (h * 0.4375).round().clamp(11.0, 20.0),
            icon: (h * 0.4375).round().clamp(11.0, 22.0),
            close_glyph: (h * 0.41).round().clamp(10.0, 19.0),
            close_box: (h * 0.56).round().clamp(14.0, 26.0),
        }
    }

    pub fn show<T: TabItem>(&self, ui: &mut egui::Ui, tabs: &[T]) -> TabStripResponse {
        let mut resp = TabStripResponse::default();
        let full_width = ui.available_width();
        let layout_width = (full_width - STRIP_RIGHT_PAD).max(1.0);
        let pinned: Vec<bool> = tabs.iter().map(TabItem::pinned).collect();
        let permanent: Vec<bool> = tabs.iter().map(TabItem::permanent).collect();
        let icon_only: Vec<bool> = tabs.iter().map(TabItem::icon_only_when_pinned).collect();
        // What Close Others, Close to the Left and Close to the Right
        // would actually close from each tab. A pinned tab and the
        // activity's permanent tab survive all three, so neither counts
        // as a neighbour worth closing.
        let closable: Vec<bool> = (0..tabs.len())
            .map(|i| !pinned[i] && !permanent[i])
            .collect();
        let scopes = close_scopes(&closable);
        let ranks: Vec<Option<usize>> = tabs.iter().map(TabItem::pin_rank).collect();
        // What a fixed or pinned tab's name needs: the icon, the gap after
        // it, the name, and the right edge the painter leaves. Measured
        // with the font the tab is drawn in, so a taller strip's bigger
        // text still fits (CR-0043).
        let m = self.metrics();
        let font = egui::FontId::proportional(m.font);
        let fit: Vec<f32> = tabs
            .iter()
            .map(|t| {
                if !(t.pinned() || t.permanent()) {
                    return 0.0;
                }
                let text = |s: String| {
                    ui.painter()
                        .layout_no_wrap(s, font.clone(), Color32::WHITE)
                        .size()
                        .x
                };
                let prefix = t.title_prefix().map(|(p, _)| text(p) + 6.0).unwrap_or(0.0);
                let w = 16.0 + m.icon + prefix + text(t.title()) + 12.0;
                // A pinned note's title can be long; a fixed tab's name
                // is an activity's or a workspace's, and is shown whole.
                if t.permanent() {
                    w
                } else {
                    w.min(MAX_TAB_WIDTH)
                }
            })
            .collect();
        // A strip whose titles are the app's own names (Devtools) makes
        // every tab as wide as the longest of them needs - the icon, the
        // name, the gap and the close button's slot - so none is elided
        // (owner, 2026-11-07).
        let whole = tabs
            .iter()
            .filter(|t| t.whole_title() && !(t.pinned() || t.permanent()))
            .map(|t| {
                let title = ui
                    .painter()
                    .layout_no_wrap(t.title(), font.clone(), Color32::WHITE)
                    .size()
                    .x;
                16.0 + m.icon + title + 8.0 + m.close_box * 1.28 + 4.0
            })
            .fold(0.0f32, f32::max);
        let layout = compute_tab_layout(
            &pinned,
            &permanent,
            &icon_only,
            &ranks,
            &fit,
            layout_width,
            self.min_width.max(whole),
            self.gap,
        );
        let total_height = layout.row_count() as f32 * (self.row_h() + self.gap);
        let (strip_rect, _) =
            ui.allocate_exact_size(Vec2::new(full_width, total_height), Sense::hover());
        let painter = ui.painter_at(strip_rect);

        let drag_id = ui.id().with("tab-drag");
        let dragging: Option<usize> = ui.ctx().data(|d| d.get_temp(drag_id));
        let mut rects: Vec<(usize, Rect)> = Vec::new();

        let mut y = strip_rect.top();
        for (row_i, row) in layout.rows.iter().enumerate() {
            let mut x = strip_rect.left();
            if row_i == 0 {
                for (k, &i) in layout.lead.iter().enumerate() {
                    let only_icon = icon_only[i] && !permanent[i];
                    let w = layout.lead_widths[k];
                    let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, self.row_h()));
                    self.draw_tab(
                        ui, &painter, rect, &tabs[i], only_icon, &mut resp, i, scopes[i],
                    );
                    rects.push((i, rect));
                    x += w + self.gap;
                }
            }
            for &i in row {
                let rect =
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(layout.tab_width, self.row_h()));
                self.draw_tab(ui, &painter, rect, &tabs[i], false, &mut resp, i, scopes[i]);
                rects.push((i, rect));
                x += layout.tab_width + self.gap;
            }
            if row_i + 1 == layout.rows.len() && self.show_new_tab {
                let rect =
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(NEW_TAB_WIDTH, self.row_h()));
                let r = ui.interact(rect, ui.id().with("new-tab"), Sense::click());
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &self.labels.new_tab)
                });
                let fill = if r.hovered() {
                    self.theme.surface_hover
                } else {
                    Color32::TRANSPARENT
                };
                painter.rect_filled(rect, CornerRadius::same(4), fill);
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    crate::icons::TAB_NEW,
                    egui::FontId::proportional(16.0),
                    self.theme.text_muted,
                );
                if r.tip(&self.labels.new_tab).clicked() {
                    resp.new_tab = true;
                }
            }
            y += self.row_h() + self.gap;
        }

        // Drag-and-drop reorder: drop onto another tab's rect.
        if let Some(from) = dragging {
            if let Some(pos) = ui.ctx().pointer_latest_pos()
                && let Some((to, rect)) = rects.iter().find(|(_, r)| r.contains(pos))
            {
                painter.rect_stroke(
                    *rect,
                    CornerRadius::same(4),
                    Stroke::new(2.0, self.theme.accent),
                    StrokeKind::Inside,
                );
                if ui.input(|i| i.pointer.any_released()) && *to != from {
                    resp.reorder = Some((from, *to));
                }
            }
            if ui.input(|i| i.pointer.any_released()) {
                ui.ctx().data_mut(|d| d.remove::<usize>(drag_id));
            }
        }
        resp
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tab<T: TabItem>(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        rect: Rect,
        tab: &T,
        icon_only: bool,
        resp: &mut TabStripResponse,
        index: usize,
        scope: CloseScope,
    ) {
        let id = ui.id().with(("tab", tab.id()));
        let r = ui.interact(rect, id, Sense::click_and_drag());
        r.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                true,
                self.active == Some(tab.id()),
                tab.title(),
            )
        });
        let is_active = self.active == Some(tab.id());
        let fill = if is_active {
            self.theme.tab_active
        } else if r.hovered() {
            self.theme.surface_hover
        } else {
            self.theme.tab_inactive
        };
        painter.rect_filled(
            rect,
            CornerRadius {
                nw: 6,
                ne: 6,
                sw: 0,
                se: 0,
            },
            fill,
        );
        let colour = tab.colour().unwrap_or(self.theme.accent);
        if is_active {
            painter.rect_filled(
                Rect::from_min_size(rect.left_top(), Vec2::new(rect.width(), 2.0)),
                CornerRadius::ZERO,
                colour,
            );
        }

        let m = self.metrics();
        let font = egui::FontId::proportional(m.font);
        let icon_pos = Pos2::new(rect.left() + 10.0, rect.center().y);
        // A favicon if the tab has one, the glyph otherwise. The image is
        // drawn at the glyph's size so the two cases line up and a tab does
        // not shift when its favicon arrives.
        let picture = (!tab.pin_icon())
            .then(|| {
                tab.icon_image()
                    .or_else(|| tab.icon_uri().map(egui::ImageSource::from))
            })
            .flatten();
        match picture {
            Some(src) => {
                let size = Vec2::splat(m.icon);
                let at =
                    Rect::from_center_size(Pos2::new(icon_pos.x + size.x / 2.0, icon_pos.y), size);
                egui::Image::new(src)
                    .fit_to_exact_size(size)
                    .paint_at(ui, at);
            }
            None => {
                let (glyph, colour) = if tab.pin_icon() {
                    (crate::icons::ph::PUSH_PIN, self.theme.pin)
                } else {
                    (
                        tab.icon(),
                        tab.icon_colour().unwrap_or(self.theme.text_muted),
                    )
                };
                painter.text(
                    icon_pos,
                    egui::Align2::LEFT_CENTER,
                    glyph,
                    font.clone(),
                    colour,
                );
            }
        }

        // The pinned marker: a bar down the leading edge in the tab's own
        // colour. A pinned tab keeps its name — losing it was the thing
        // that made pinning feel like the tab had been taken away.
        // A permanent tab wears no close cross for the same reason a
        // pinned one does not: it is not going anywhere, and the name
        // wants the room.
        let is_pinned = tab.pinned() || tab.permanent();
        if is_pinned {
            painter.rect_filled(
                Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
                CornerRadius::ZERO,
                colour,
            );
        }
        if !icon_only {
            // No close button on a pinned tab: pinning is for the tabs
            // somebody means to keep, and the name wants the room.
            let close_rect = Rect::from_center_size(
                Pos2::new(rect.right() - m.close_box * 0.78, rect.center().y),
                Vec2::splat(m.close_box),
            );
            let mut text_left = rect.left() + 16.0 + m.icon;
            let text_right = if is_pinned {
                rect.right() - 6.0
            } else {
                close_rect.left() - 8.0
            };
            // A coloured piece first, when the tool offers one: the API
            // client's HTTP method, so a strip of requests reads by
            // colour. Only the piece is coloured; the title is not.
            if let Some((prefix, colour)) = tab.title_prefix() {
                let g = painter.layout_no_wrap(prefix.clone(), font.clone(), colour);
                // Only when the name still has somewhere to go: a method
                // alone in a narrow tab says less than a truncated name.
                if text_left + g.size().x + 24.0 < text_right {
                    painter.text(
                        Pos2::new(text_left, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        prefix,
                        font.clone(),
                        colour,
                    );
                    text_left += g.size().x + 6.0;
                }
            }
            // The title stops before the close button's slot and its gap,
            // measured rather than estimated, and is clipped there too, so
            // it never runs under the cross (owner, 2026-11-06).
            let avail = (text_right - text_left).max(0.0);
            let shown = fit_title(painter, &tab.title(), &font, avail);
            let title_clip =
                Rect::from_x_y_ranges(text_left..=text_right.max(text_left), rect.y_range())
                    .intersect(painter.clip_rect());
            painter.with_clip_rect(title_clip).text(
                Pos2::new(text_left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                shown,
                font.clone(),
                self.theme.text,
            );

            if is_pinned {
                // A pinned tab shows its dirty mark where the close button
                // would be, and nothing else: the rest of draw_tab still
                // runs, because that is where the context menu lives — and
                // the context menu is how a tab gets unpinned.
                if tab.dirty() {
                    painter.circle_filled(close_rect.center(), 4.0, colour);
                }
            } else {
                let cr = ui.interact(close_rect, id.with("close"), Sense::click());
                cr.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        format!("{} {}", self.labels.close, tab.title()),
                    )
                });
                if tab.dirty() && !cr.hovered() {
                    painter.circle_filled(close_rect.center(), 4.0, colour);
                } else {
                    // The close cross is a real control, not a hover secret:
                    // keep it visible at rest so the last tab is never
                    // mistaken for an unclosable one.
                    let c = self.theme.text;
                    if cr.hovered() {
                        painter.rect_filled(
                            close_rect,
                            CornerRadius::same(4),
                            self.theme.surface_hover,
                        );
                    }
                    painter.text(
                        close_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        crate::icons::TAB_CLOSE,
                        egui::FontId::proportional(m.close_glyph),
                        c,
                    );
                }
                if cr.clicked() {
                    resp.close = Some(tab.id());
                }
            }
        } else if tab.dirty() {
            painter.circle_filled(Pos2::new(rect.right() - 7.0, rect.top() + 7.0), 3.0, colour);
        }

        if r.clicked() {
            resp.activated = Some(tab.id());
        }
        if r.middle_clicked() {
            resp.close = Some(tab.id());
        }
        if r.drag_started() {
            ui.ctx()
                .data_mut(|d| d.insert_temp(id.with("drag-root"), ()));
            ui.ctx()
                .data_mut(|d| d.insert_temp(ui.id().with("tab-drag"), index));
        }
        let r = r.tip(tab.tooltip());
        let labels = self.labels;
        let pinned = tab.pinned();
        let tid = tab.id();
        let compact = self.compact;
        if !tab.has_menu() {
            return;
        }
        let permanent = tab.permanent();
        // A permanent tab on its own has nothing to offer: every entry
        // acts on the tabs beside it, and there are none. An empty menu
        // reads as a broken one (CR-0029).
        if permanent && !scope.others() {
            return;
        }
        r.context_menu(|ui| {
            let mut pick = |ui: &mut egui::Ui, label: &str, action: TabMenuAction| {
                if !ui.button(label).clicked() {
                    return;
                }
                // egui lays the frame out twice when the strip's size
                // changes, and `clicked()` is pointer *state*, not an
                // event, so the second pass sees the same click. Opening
                // a tab changes the strip, so Duplicate acted twice and
                // made two copies. Both passes report the same time, so
                // that is what tells a repeat from a second click.
                let now = ui.input(|i| i.time);
                let key = egui::Id::new("tabstrip-last-menu");
                let seen: Option<(u64, TabMenuAction, f64)> = ui.ctx().data(|d| d.get_temp(key));
                if seen == Some((tid, action, now)) {
                    ui.close();
                    return;
                }
                ui.ctx()
                    .data_mut(|d| d.insert_temp(key, (tid, action, now)));
                resp.menu = Some((tid, action));
                ui.close();
            };
            // A permanent tab is the activity's home: closing it, closing
            // past it or pinning it are all meaningless, so only the two
            // that act on the tabs beside it are offered.
            if !permanent && tab.can_close() {
                pick(ui, &labels.close, TabMenuAction::Close);
            }
            // Only the entries that would do something, and no two that
            // would do the same thing (CR-0029).
            if pinned {
                // A pinned tab is not offered Close Others: that action
                // spares the other pinned tabs, so from a pinned tab it
                // does not mean what it says. Close to the Right does -
                // a pinned tab is drawn at the strip's head, so every
                // closable tab is to its right whatever its place in the
                // list, and nothing is ever to its left (owner's rule,
                // CR-0029).
                if scope.right {
                    pick(ui, &labels.close_right, TabMenuAction::CloseRight);
                }
            } else {
                if scope.others() {
                    pick(ui, &labels.close_others, TabMenuAction::CloseOthers);
                }
                if scope.close_left() {
                    pick(ui, &labels.close_left, TabMenuAction::CloseLeft);
                }
                if scope.close_right() {
                    pick(ui, &labels.close_right, TabMenuAction::CloseRight);
                }
            }
            if permanent {
                return;
            }
            ui.separator();
            if compact {
                pick(ui, &labels.copy_path, TabMenuAction::CopyPath);
                return;
            }
            if pinned {
                if tab.can_unpin() {
                    pick(ui, &labels.unpin, TabMenuAction::Unpin);
                }
            } else {
                pick(ui, &labels.pin, TabMenuAction::Pin);
            }
            // Split Right / Split Down used to sit here. They left this
            // menu in 2026-10-17 because they rearranged the whole tab
            // space from a menu meant for one tab, and splitting left the
            // app entirely on 2026-10-23 (docs/04 §2.4).
            if tab.shows_duplicate() {
                if tab.can_duplicate() {
                    pick(ui, &labels.duplicate, TabMenuAction::Duplicate);
                } else {
                    ui.add_enabled(false, egui::Button::new(&labels.duplicate))
                        .tip_disabled(&labels.duplicate_blocked);
                }
            }
        });
    }
}

/// The title as it fits in `avail` pixels: whole when it fits, else
/// middle-elided and measured until it really fits. [`elide`]'s guess
/// assumes every glyph is as wide as the average, so a title with wide
/// letters at its ends overran into the close button.
fn fit_title(painter: &egui::Painter, title: &str, font: &egui::FontId, avail: f32) -> String {
    let width = |s: &str| {
        painter
            .layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
    };
    let full = width(title);
    if full <= avail {
        return title.to_string();
    }
    let mut shown = elide(title, avail, full);
    let mut keep = shown.chars().count().saturating_sub(1);
    while keep > 2 && width(&shown) > avail {
        keep -= 1;
        shown = elide_to(title, keep);
    }
    shown
}

/// Middle-elide a title to roughly `avail` pixels given its full width.
fn elide(title: &str, avail: f32, full: f32) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() < 4 || full <= 0.0 {
        return title.to_string();
    }
    let keep = ((avail / full) * chars.len() as f32).floor() as usize;
    elide_to(title, keep.saturating_sub(1).max(2))
}

/// `title` cut to `keep` characters, head and tail either side of `…`.
fn elide_to(title: &str, keep: usize) -> String {
    let chars: Vec<char> = title.chars().collect();
    let head = keep.div_ceil(2);
    let tail = keep / 2;
    let mut s: String = chars[..head.min(chars.len())].iter().collect();
    s.push('…');
    s.extend(chars[chars.len().saturating_sub(tail)..].iter());
    s
}

/// Height of a [`segmented`] control.
pub const SEGMENT_HEIGHT: f32 = 26.0;
/// A Material picture's side in a segment, and the gap after it.
const SEGMENT_ART: f32 = 15.0;
const SEGMENT_ART_GAP: f32 = 5.0;
/// Room left and right of a segment's content, together.
const SEGMENT_PAD: f32 = 16.0;

fn segment_font() -> egui::FontId {
    egui::FontId::proportional(13.0)
}

/// A segment's text: its glyph in a colour of its own (CR-0048), when it
/// has one, then the label.
fn segment_job(
    label: &str,
    glyph: Option<(&str, Color32)>,
    colour: Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    if let Some((g, c)) = glyph {
        job.append(g, 0.0, egui::TextFormat::simple(segment_font(), c));
        job.append(label, 5.0, egui::TextFormat::simple(segment_font(), colour));
    } else {
        job.append(label, 0.0, egui::TextFormat::simple(segment_font(), colour));
    }
    job
}

/// The narrowest a [`segmented`] control can be with every segment's
/// picture or glyph and label shown whole: the widest segment's content
/// plus its padding, times the number of segments (they are equal width).
/// A pane holding one uses this as its minimum (CR-0069). `arts` and
/// `glyphs` are index-aligned with `labels`, as for [`segmented`].
pub fn segmented_min_width(
    ui: &egui::Ui,
    labels: &[&str],
    arts: &[Option<egui::ImageSource<'static>>],
    glyphs: &[Option<(&str, Color32)>],
) -> f32 {
    let widest = labels
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let glyph = glyphs.get(i).copied().flatten();
            let text = ui
                .painter()
                .layout_job(segment_job(label, glyph, Color32::WHITE))
                .size()
                .x;
            let art = if arts.get(i).is_some_and(Option::is_some) {
                SEGMENT_ART + SEGMENT_ART_GAP
            } else {
                0.0
            };
            art + text
        })
        .fold(0.0_f32, f32::max);
    labels.len() as f32 * (widest + SEGMENT_PAD)
}

/// A row of equal-width toggle segments across the available width (never
/// wider, so a resizable side panel does not grow). Returns the clicked one.
///
/// A segment may show a Material picture beside its text instead of a
/// leading glyph (FEAT-0084): `arts` is index-aligned with `labels`, and a
/// missing or `None` entry draws the label alone, which is what an
/// activity with Material icons off gets. `glyphs`, also index-aligned,
/// gives a segment a leading glyph in its own colour (the Browser's gold
/// star, CR-0048). Nothing is drawn outside its segment: when the content
/// does not fit, the picture or glyph is drawn alone, and the text is
/// clipped to the segment (CR-0069). Keep the pane at least
/// [`segmented_min_width`] wide and that never happens.
pub fn segmented(
    ui: &mut egui::Ui,
    theme: &Theme,
    labels: &[&str],
    arts: &[Option<egui::ImageSource<'static>>],
    glyphs: &[Option<(&str, Color32)>],
    selected: usize,
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), SEGMENT_HEIGHT),
        Sense::hover(),
    );
    let w = rect.width() / labels.len().max(1) as f32;
    let mut clicked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * w, rect.top()),
            egui::vec2(w, rect.height()),
        )
        .shrink2(egui::vec2(1.0, 0.0));
        let resp = ui.interact(r, ui.id().with(("segment", i)), Sense::click());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                i == selected,
                *label,
            )
        });
        let on = i == selected;
        let fill = if on {
            theme.accent.linear_multiply(0.3)
        } else if resp.hovered() {
            theme.surface_hover
        } else {
            theme.surface
        };
        ui.painter().rect_filled(r, CornerRadius::same(4), fill);
        let p = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
        let colour = if on { theme.text } else { theme.text_muted };
        let glyph = glyphs.get(i).copied().flatten();
        let galley = p.layout_job(segment_job(label, glyph, colour));
        let room = r.width() - 4.0;
        match arts.get(i).and_then(|a| a.clone()) {
            // Picture and text are centred as one unit, so the segment
            // reads the same as the glyph-and-text one beside it.
            Some(src) => {
                let full = SEGMENT_ART + SEGMENT_ART_GAP + galley.size().x;
                let fits = full <= room;
                let total = if fits { full } else { SEGMENT_ART };
                let left = r.center().x - total / 2.0;
                let at = Rect::from_min_size(
                    egui::pos2(left, r.center().y - SEGMENT_ART / 2.0),
                    Vec2::splat(SEGMENT_ART),
                );
                // The picture only where it is whole inside the segment.
                if r.contains_rect(at) {
                    egui::Image::new(src)
                        .fit_to_exact_size(Vec2::splat(SEGMENT_ART))
                        .paint_at(ui, at);
                }
                if fits {
                    p.galley(
                        egui::pos2(
                            left + SEGMENT_ART + SEGMENT_ART_GAP,
                            r.center().y - galley.size().y / 2.0,
                        ),
                        galley,
                        colour,
                    );
                }
            }
            None => match glyph {
                Some((g, c)) if galley.size().x > room => {
                    p.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        g,
                        segment_font(),
                        c,
                    );
                }
                _ => {
                    p.galley(r.center() - galley.size() / 2.0, galley, colour);
                }
            },
        }
        if resp.clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_row_when_tabs_fit_and_width_is_capped() {
        let l = compute_tab_layout(
            &[false; 3],
            &[false; 3],
            &[false; 3],
            &[],
            &[],
            1000.0,
            140.0,
            4.0,
        );
        assert_eq!(l.rows.len(), 1);
        assert_eq!(l.tab_width, MAX_TAB_WIDTH);
        assert_eq!(
            tab_row_count(&[false; 3], &[false; 3], &[false; 3], 1000.0, 140.0, 4.0),
            1
        );
    }

    #[test]
    fn tabs_shrink_to_minimum_then_wrap_instead_of_going_thinner() {
        // 6 tabs × 140 + gaps + "+" needs more than 600 px → wraps.
        let l = compute_tab_layout(
            &[false; 6],
            &[false; 6],
            &[false; 6],
            &[],
            &[],
            600.0,
            140.0,
            4.0,
        );
        assert!(l.rows.len() >= 2, "expected wrapping, got {:?}", l.rows);
        assert!(l.tab_width >= 140.0);
        let total: usize = l.rows.iter().map(Vec::len).sum();
        assert_eq!(total, 6);
        // Every row fits the width at the computed tab width.
        for row in &l.rows {
            let used = row.len() as f32 * (l.tab_width + 4.0) + NEW_TAB_WIDTH;
            assert!(used <= 600.0 + 4.0 + 1.0, "row too wide: {used}");
        }
    }

    #[test]
    fn a_pinned_tab_with_a_name_reserves_more_room_than_an_icon_only_one() {
        // The browser's pinned tab is a favicon; everything else keeps its
        // name, so it needs a wider slot and the regular tabs get less.
        let pinned = [true, false, false, false];
        let icons = [true, false, false, false];
        let names = [false, false, false, false];
        // Narrow enough that the tabs are not already at MAX_TAB_WIDTH,
        // or both answers are the cap and the test proves nothing.
        let with_icon =
            compute_tab_layout(&pinned, &[false; 4], &icons, &[], &[], 700.0, 140.0, 4.0);
        let with_name =
            compute_tab_layout(&pinned, &[false; 4], &names, &[], &[], 700.0, 140.0, 4.0);
        assert!(
            with_name.tab_width < with_icon.tab_width,
            "a named pinned tab should take more of the row"
        );
        assert_eq!(pinned_slot_width(true), PINNED_WIDTH);
        assert_eq!(pinned_slot_width(false), PINNED_TEXT_WIDTH);
    }

    #[test]
    fn every_row_fits_inside_the_strip() {
        let pinned = vec![false; 20];
        let mut permanent = vec![false; 20];
        permanent[0] = true;
        let icon_only = vec![false; 20];
        let width = 900.0;
        let gap = 4.0;
        let l = compute_tab_layout(&pinned, &permanent, &icon_only, &[], &[], width, 120.0, gap);
        let lead_span: f32 = l.lead.iter().map(|_| pinned_slot_width(false) + gap).sum();
        for (i, row) in l.rows.iter().enumerate() {
            let used =
                if i == 0 { lead_span } else { 0.0 } + row.len() as f32 * (l.tab_width + gap);
            assert!(
                used <= width + 0.5,
                "row {i} uses {used} of {width} ({} tabs, {})",
                row.len(),
                l.tab_width
            );
        }
    }

    #[test]
    fn pinned_tabs_take_first_row_space_and_are_not_counted_as_regular() {
        let pinned = [true, false, true, false, false];
        let l = compute_tab_layout(
            &pinned,
            &[false; 5],
            &[true; 5],
            &[],
            &[],
            800.0,
            140.0,
            4.0,
        );
        assert_eq!(l.lead, vec![0, 2]);
        let regular: Vec<usize> = l.rows.concat();
        assert_eq!(regular, vec![1, 3, 4]);
    }

    #[test]
    fn the_permanent_tab_leads_and_pinned_tabs_follow_it() {
        // Tab 2 is the activity's home; 0 and 3 are pinned. The home tab is
        // drawn first whatever the strip order is.
        let pinned = [true, false, false, true, false];
        let permanent = [false, false, true, false, false];
        let l = compute_tab_layout(&pinned, &permanent, &[true; 5], &[], &[], 800.0, 140.0, 4.0);
        assert_eq!(l.lead, vec![2, 0, 3]);
        assert_eq!(l.rows.concat(), vec![1, 4]);
    }

    #[test]
    fn minimum_width_setting_changes_row_count() {
        let narrow = tab_row_count(&[false; 10], &[false; 10], &[false; 10], 900.0, 80.0, 4.0);
        let wide = tab_row_count(&[false; 10], &[false; 10], &[false; 10], 900.0, 200.0, 4.0);
        assert!(wide > narrow);
    }

    /// A fixed or pinned slot that shows a name is as wide as the name
    /// needs - wider than the old fixed slot for a long one, narrower for
    /// a short one - so it never truncates (CR-0043). An icon-only pinned
    /// slot keeps its small fixed width.
    #[test]
    fn a_named_lead_slot_is_as_wide_as_its_name() {
        // Permanent, a pinned name, an icon-only pin, then two regular tabs.
        let pinned = [false, true, true, false, false];
        let permanent = [true, false, false, false, false];
        let icon_only = [false, false, true, false, false];
        let fit = [190.0, 80.0, 999.0, 0.0, 0.0];
        let l = compute_tab_layout(
            &pinned,
            &permanent,
            &icon_only,
            &[],
            &fit,
            1400.0,
            140.0,
            4.0,
        );
        assert_eq!(l.lead, vec![0, 1, 2]);
        assert_eq!(l.lead_widths, vec![190.0, 80.0, PINNED_WIDTH]);
        // Measured or not, a slot is the same width everywhere: with no
        // measurement it falls back to the old fixed width.
        let fixed = compute_tab_layout(
            &pinned,
            &permanent,
            &icon_only,
            &[],
            &[],
            1400.0,
            140.0,
            4.0,
        );
        assert_eq!(
            fixed.lead_widths,
            vec![PINNED_TEXT_WIDTH, PINNED_TEXT_WIDTH, PINNED_WIDTH]
        );
    }

    #[test]
    fn no_regular_tabs_is_one_empty_row() {
        let l = compute_tab_layout(
            &[true, true],
            &[false, false],
            &[true, true],
            &[],
            &[],
            500.0,
            140.0,
            4.0,
        );
        assert_eq!(l.rows, vec![Vec::<usize>::new()]);
        assert_eq!(l.row_count(), 1);
    }

    #[test]
    fn elide_keeps_head_and_tail() {
        let s = elide("docker-compose.staging.yml", 60.0, 200.0);
        assert!(s.contains('…'));
        assert!(s.starts_with('d'));
        assert!(s.ends_with('l'));
    }

    /// A truncated title is measured to its room, so it never runs under
    /// the close button - even with wide letters at both ends, where the
    /// average-width guess overran (owner, 2026-11-06).
    #[test]
    fn a_fitted_title_never_exceeds_its_room() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::FontId::proportional(13.0);
            let painter = ui.painter();
            let title = "WWWWWWWWMMiiiiiiiiiiiiiiiiiiiiiiiillllllllllWWWWWWWWMM";
            for avail in [60.0, 90.0, 120.0, 160.0] {
                let shown = fit_title(painter, title, &font, avail);
                assert!(shown.contains('…'), "{avail}: {shown}");
                let w = painter
                    .layout_no_wrap(shown.clone(), font.clone(), Color32::WHITE)
                    .size()
                    .x;
                assert!(w <= avail, "{shown} is {w} wide in {avail}");
            }
            assert_eq!(fit_title(painter, "Short", &font, 200.0), "Short");
        });
    }

    #[test]
    fn the_segmented_minimum_fits_every_label_and_nothing_paints_outside() {
        let ctx = egui::Context::default();
        let theme = Theme::dark();
        let labels = ["Project", "Source Control (12)", "Favorites"];
        let arts = [
            Some(egui::ImageSource::Uri("bytes://art".into())),
            None,
            None,
        ];
        let glyphs = [None, None, Some(("*", Color32::GOLD))];
        let mut narrow = Rect::NOTHING;
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let min = segmented_min_width(ui, &labels, &arts, &glyphs);
            let seg = min / labels.len() as f32 - 2.0 - 4.0;
            for (i, label) in labels.iter().enumerate() {
                let text = ui
                    .painter()
                    .layout_job(segment_job(label, glyphs[i], Color32::WHITE))
                    .size()
                    .x;
                let art = if arts[i].is_some() {
                    SEGMENT_ART + SEGMENT_ART_GAP
                } else {
                    0.0
                };
                assert!(text > 0.0, "{label} was not measured");
                assert!(art + text <= seg, "{label} does not fit {seg}");
            }
            // A longer label needs more room.
            assert!(
                segmented_min_width(ui, &["Project", "A much longer label"], &[], &[])
                    > segmented_min_width(ui, &["Project", "Short"], &[], &[])
            );
            // Far narrower than the minimum: the text stays in its segment.
            // No picture here - with no image loader in a test it would
            // draw its own error mark, which is not the segment's text.
            narrow = ui
                .allocate_ui(egui::vec2(60.0, SEGMENT_HEIGHT), |ui| {
                    segmented(ui, &theme, &labels, &[], &glyphs, 0);
                })
                .response
                .rect;
        });
        for clipped in &out.shapes {
            if matches!(clipped.shape, egui::Shape::Text(_)) {
                assert!(
                    narrow.expand(0.5).contains_rect(clipped.clip_rect),
                    "text clipped to {:?}, outside {narrow:?}",
                    clipped.clip_rect
                );
            }
        }
    }

    /// Devtools raises the minimum to its longest name; a minimum above
    /// the usual cap still lays out (no clamp panic) and every tab gets it.
    #[test]
    fn a_whole_title_minimum_above_the_cap_is_honoured() {
        let n = 6;
        let l = compute_tab_layout(
            &vec![false; n],
            &vec![false; n],
            &vec![false; n],
            &vec![None; n],
            &vec![0.0; n],
            900.0,
            MAX_TAB_WIDTH + 30.0,
            4.0,
        );
        assert!(l.tab_width >= MAX_TAB_WIDTH + 30.0);
        assert!(l.row_count() >= 2);
    }
}
