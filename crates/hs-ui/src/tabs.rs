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
    /// Whether this tab may be unpinned from the strip (for mandatory app
    /// tabs such as Settings).
    fn can_unpin(&self) -> bool {
        true
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
pub fn compute_tab_layout(
    pinned: &[bool],
    permanent: &[bool],
    icon_only: &[bool],
    full_width: f32,
    min_width: f32,
    gap: f32,
) -> TabLayout {
    let is_permanent = |i: usize| permanent.get(i).copied().unwrap_or(false);
    // A permanent tab keeps its name even where a pinned one would be an
    // icon: it is the activity's front page, not a bookmark.
    let slot_width = |i: usize| {
        pinned_slot_width(icon_only.get(i).copied().unwrap_or(false) && !is_permanent(i))
    };
    let lead: Vec<usize> = (0..pinned.len())
        .filter(|&i| is_permanent(i))
        .chain((0..pinned.len()).filter(|&i| pinned[i] && !is_permanent(i)))
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
            lead,
        };
    }

    // Everything on one row at a comfortable width?
    let n = regular.len() as f32;
    let one_row_width = (first_row_avail + gap) / n - gap;
    if one_row_width >= min_width {
        return TabLayout {
            rows: vec![regular],
            tab_width: one_row_width.min(MAX_TAB_WIDTH),
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
    let widest_row = rows.iter().map(Vec::len).max().unwrap_or(1) as f32;
    let tab_width = ((other_rows_avail + gap) / widest_row - gap).clamp(min_width, MAX_TAB_WIDTH);
    TabLayout {
        rows,
        tab_width,
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
    compute_tab_layout(pinned, permanent, icon_only, full_width, min_width, gap).row_count()
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
    /// Tabs visible in a split (drawn with a thin underline).
    pub visible: &'a [u64],
    pub labels: &'a TabStripLabels,
    /// Lower rows and a menu of Close / Close Others / Close Left / Close
    /// Right / Copy Path (no pin, duplicate or split).
    pub compact: bool,
    /// Draw the add-tab button at the end of the last row. The window
    /// strip does not: a tab is opened from the activity's own sidebar or
    /// from Home, and a + there opened an untitled one nobody asked for.
    /// Owner's requirement, 2026-10-17.
    pub show_new_tab: bool,
}

impl TabStrip<'_> {
    fn row_h(&self) -> f32 {
        if self.compact {
            COMPACT_ROW_HEIGHT
        } else {
            ROW_HEIGHT
        }
    }

    pub fn show<T: TabItem>(&self, ui: &mut egui::Ui, tabs: &[T]) -> TabStripResponse {
        let mut resp = TabStripResponse::default();
        let full_width = ui.available_width();
        let pinned: Vec<bool> = tabs.iter().map(TabItem::pinned).collect();
        let permanent: Vec<bool> = tabs.iter().map(TabItem::permanent).collect();
        let icon_only: Vec<bool> = tabs.iter().map(TabItem::icon_only_when_pinned).collect();
        let layout = compute_tab_layout(
            &pinned,
            &permanent,
            &icon_only,
            full_width,
            self.min_width,
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
                for &i in &layout.lead {
                    let only_icon = icon_only[i] && !permanent[i];
                    let w = pinned_slot_width(only_icon);
                    let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, self.row_h()));
                    self.draw_tab(ui, &painter, rect, &tabs[i], only_icon, &mut resp, i);
                    rects.push((i, rect));
                    x += w + self.gap;
                }
            }
            for &i in row {
                let rect =
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(layout.tab_width, self.row_h()));
                self.draw_tab(ui, &painter, rect, &tabs[i], false, &mut resp, i);
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
        let is_visible = self.visible.contains(&tab.id());
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
        } else if is_visible {
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(rect.left(), rect.bottom() - 2.0),
                    Vec2::new(rect.width(), 2.0),
                ),
                CornerRadius::ZERO,
                colour.linear_multiply(0.6),
            );
        }

        let font = egui::FontId::proportional(if self.compact { 13.0 } else { 14.0 });
        let icon_pos = Pos2::new(rect.left() + 10.0, rect.center().y);
        // A favicon if the tab has one, the glyph otherwise. The image is
        // drawn at the glyph's size so the two cases line up and a tab does
        // not shift when its favicon arrives.
        match tab.icon_uri() {
            Some(uri) => {
                let size = Vec2::splat(14.0);
                let at =
                    Rect::from_center_size(Pos2::new(icon_pos.x + size.x / 2.0, icon_pos.y), size);
                egui::Image::from_uri(uri)
                    .fit_to_exact_size(size)
                    .paint_at(ui, at);
            }
            None => {
                painter.text(
                    icon_pos,
                    egui::Align2::LEFT_CENTER,
                    tab.icon(),
                    font.clone(),
                    self.theme.text_muted,
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
                Pos2::new(rect.right() - 14.0, rect.center().y),
                Vec2::splat(18.0),
            );
            let mut text_left = rect.left() + 30.0;
            let text_right = if is_pinned {
                rect.right() - 6.0
            } else {
                close_rect.left() - 4.0
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
            let title = tab.title();
            let galley = painter.layout_no_wrap(title.clone(), font.clone(), self.theme.text);
            let avail = (text_right - text_left).max(0.0);
            let shown = if galley.size().x <= avail {
                title
            } else {
                elide(&title, avail, galley.size().x)
            };
            painter.text(
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
                    let c = if cr.hovered() {
                        self.theme.text
                    } else {
                        self.theme.text_muted
                    };
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
                        egui::FontId::proportional(12.0),
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
            if !permanent {
                pick(ui, &labels.close, TabMenuAction::Close);
            }
            pick(ui, &labels.close_others, TabMenuAction::CloseOthers);
            if !permanent {
                pick(ui, &labels.close_left, TabMenuAction::CloseLeft);
            }
            pick(ui, &labels.close_right, TabMenuAction::CloseRight);
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
            // Split Right / Split Down used to sit here. They rearranged
            // the whole tab space from a menu meant for one tab, which is
            // not what someone right-clicking a tab is reaching for.
            // Removed at the owner's request, 2026-10-17.
            if tab.can_duplicate() {
                pick(ui, &labels.duplicate, TabMenuAction::Duplicate);
            } else {
                ui.add_enabled(false, egui::Button::new(&labels.duplicate))
                    .on_disabled_hover_text(&labels.duplicate_blocked);
            }
        });
    }
}

/// Middle-elide a title to roughly `avail` pixels given its full width.
fn elide(title: &str, avail: f32, full: f32) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() < 4 || full <= 0.0 {
        return title.to_string();
    }
    let keep = ((avail / full) * chars.len() as f32).floor() as usize;
    let keep = keep.saturating_sub(1).max(2);
    let head = keep.div_ceil(2);
    let tail = keep / 2;
    let mut s: String = chars[..head.min(chars.len())].iter().collect();
    s.push('…');
    s.extend(chars[chars.len().saturating_sub(tail)..].iter());
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_row_when_tabs_fit_and_width_is_capped() {
        let l = compute_tab_layout(&[false; 3], &[false; 3], &[false; 3], 1000.0, 140.0, 4.0);
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
        let l = compute_tab_layout(&[false; 6], &[false; 6], &[false; 6], 600.0, 140.0, 4.0);
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
        let with_icon = compute_tab_layout(&pinned, &[false; 4], &icons, 700.0, 140.0, 4.0);
        let with_name = compute_tab_layout(&pinned, &[false; 4], &names, 700.0, 140.0, 4.0);
        assert!(
            with_name.tab_width < with_icon.tab_width,
            "a named pinned tab should take more of the row"
        );
        assert_eq!(pinned_slot_width(true), PINNED_WIDTH);
        assert_eq!(pinned_slot_width(false), PINNED_TEXT_WIDTH);
    }

    #[test]
    fn pinned_tabs_take_first_row_space_and_are_not_counted_as_regular() {
        let pinned = [true, false, true, false, false];
        let l = compute_tab_layout(&pinned, &[false; 5], &[true; 5], 800.0, 140.0, 4.0);
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
        let l = compute_tab_layout(&pinned, &permanent, &[true; 5], 800.0, 140.0, 4.0);
        assert_eq!(l.lead, vec![2, 0, 3]);
        assert_eq!(l.rows.concat(), vec![1, 4]);
    }

    #[test]
    fn minimum_width_setting_changes_row_count() {
        let narrow = tab_row_count(&[false; 10], &[false; 10], &[false; 10], 900.0, 80.0, 4.0);
        let wide = tab_row_count(&[false; 10], &[false; 10], &[false; 10], 900.0, 200.0, 4.0);
        assert!(wide > narrow);
    }

    #[test]
    fn no_regular_tabs_is_one_empty_row() {
        let l = compute_tab_layout(
            &[true, true],
            &[false, false],
            &[true, true],
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
}
