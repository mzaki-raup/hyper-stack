//! Drag a row to a new place in a list, with a line showing where it would
//! land.
//!
//! Every sidebar list in HyperStack is the user's own collection — SSH
//! hosts, database and Redis and NoSQL connections, S3 profiles, shell
//! profiles, API collections — and the order they are kept in is part of
//! how someone works. One helper does it everywhere so the gesture, the
//! indicator and the keyboard story are the same in all of them.
//!
//! The list itself is never touched here: [`Reorder::finish`] reports the
//! move and the caller applies it with [`move_item`] and saves whatever it
//! keeps the order in.

use std::fmt::Debug;
use std::hash::Hash;

/// Apply a move from `from` to the boundary `to` (`0..=len`), the form
/// [`Reorder::finish`] reports.
///
/// A boundary is between rows, not a row index: dropping just below row 2
/// of five is `to == 3`, and that is a different thing from `to == 2`.
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) {
    if from >= items.len() || to > items.len() {
        return;
    }
    // Dropping a row immediately above or below itself changes nothing.
    if to == from || to == from + 1 {
        return;
    }
    let item = items.remove(from);
    let to = if to > from { to - 1 } else { to };
    items.insert(to, item);
}

/// Where a dragged row ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    /// Dropped in the list it came from.
    Within { from: usize, to: usize },
    /// Dropped in a different list — one level of a tree into another.
    /// `scope` is what the source list was named with [`Reorder::scoped`].
    From {
        scope: String,
        from: usize,
        to: usize,
    },
}

/// State for one list being drawn this frame.
pub struct Reorder {
    id: egui::Id,
    /// What this list is called, when it is one level of a tree and a row
    /// may be dragged out of it into another level.
    scope: Option<String>,
    /// The row being dragged, as the payload says — from this list or
    /// another one.
    dragged: Option<DragIndex>,
    /// The boundary the pointer is nearest, and where to paint the line.
    target: Option<(usize, f32, egui::Rangef)>,
}

impl Reorder {
    pub fn new(id_salt: impl Hash + Debug) -> Self {
        Self {
            id: egui::Id::new("hs-reorder").with(id_salt),
            scope: None,
            dragged: None,
            target: None,
        }
    }

    /// A list that is one level of a tree. `scope` names it, so a drop in
    /// another level can say where the row came from
    /// ([`Reorder::finish_move`]).
    pub fn scoped(id_salt: impl Hash + Debug, scope: impl Into<String>) -> Self {
        Self {
            scope: Some(scope.into()),
            ..Self::new(id_salt)
        }
    }

    /// Forget the drop boundary worked out this frame. A tree calls this
    /// when the pointer is over a folder's header, where the drop means
    /// "into this folder" rather than "between these two rows", so only
    /// one indicator is painted.
    pub fn suppress_target(&mut self) {
        self.target = None;
    }

    /// Whether a row of this list is being dragged right now, so the caller
    /// can dim it or suppress a hover effect.
    pub fn dragging(&self) -> bool {
        self.dragged.as_ref().is_some_and(|d| d.list == self.id)
    }

    /// Draw row `index` with a drag grip on its left. `add` draws the row
    /// as usual and its value is returned untouched, so an existing row
    /// body needs no changes — and keeps all of its own interaction.
    ///
    /// Only the grip is a drag source. Making the whole row one puts a
    /// `Sense::drag()` interaction over it (that is what
    /// `Ui::dnd_drag_source` does), which sits on top of whatever the row
    /// drew and swallows its clicks and hovers: the list becomes pretty
    /// and unusable. The grip also says the list can be reordered, which
    /// nothing else does.
    pub fn row<R>(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let mut inner = None;
        let row = ui.horizontal(|ui| {
            // An explicit rectangle, not a Label: the glyph's own size
            // depends on whether the icon font has it, and a grip that is
            // a few pixels wide is one nobody can grab. The slot is
            // reserved first; the dots are painted after the row body so
            // they sit at the row's vertical centre whatever its height
            // (a profile row with a subtitle is 44 px, a bare one 34 px).
            let (slot, _) =
                ui.allocate_exact_size(egui::vec2(GRIP_WIDTH, GRIP_HEIGHT), egui::Sense::hover());
            inner = Some(add(ui));
            let body = ui.min_rect();
            let grip_rect = egui::Rect::from_min_max(
                egui::pos2(slot.left(), body.top()),
                egui::pos2(slot.right(), body.bottom()),
            );
            // An interaction with an id of our own, so the grip's drag
            // state is remembered per row rather than per position.
            let grip = ui.interact(grip_rect, self.id.with(index), egui::Sense::drag());
            paint_grip(
                ui.painter(),
                grip_rect,
                if grip.hovered() {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().weak_text_color()
                },
            );
            if grip.drag_started() || grip.dragged() {
                egui::DragAndDrop::set_payload(
                    ui.ctx(),
                    DragIndex {
                        list: self.id,
                        scope: self.scope.clone(),
                        index,
                    },
                );
            }
            grip.on_hover_cursor(egui::CursorIcon::Grab);
        });
        // The drop boundary is judged from the whole row, not the grip.
        let rect = row.response.rect;
        // interact_pos, not hover_pos: egui reports no hover while a mouse
        // button is held, which is exactly when a drag is in flight.
        let pointer_now = ui.input(|i| i.pointer.interact_pos());
        if let Some(p) = egui::DragAndDrop::payload::<DragIndex>(ui.ctx())
            && pointer_now.is_some_and(|pos| rect.y_range().contains(pos.y))
        {
            self.dragged = Some((*p).clone());
            let pointer = pointer_now.unwrap_or(rect.center());
            let (boundary, y) = if pointer.y < rect.center().y {
                (index, rect.top())
            } else {
                (index + 1, rect.bottom())
            };
            self.target = Some((boundary, y, rect.x_range()));
        }
        inner.expect("the row body runs exactly once")
    }

    /// The move the user made, once every row has been drawn. Call this
    /// after the loop: it also paints the indicator, which must go on top
    /// of the rows.
    pub fn finish(&mut self, ui: &mut egui::Ui, colour: egui::Color32) -> Option<(usize, usize)> {
        match self.finish_move(ui, colour)? {
            Move::Within { from, to } => Some((from, to)),
            // A list that never names a scope cannot be dropped into from
            // elsewhere, so this arm is unreachable for those callers.
            Move::From { .. } => None,
        }
    }

    /// The move the user made, saying which list the row came from. A tree
    /// uses this to let a row leave one level for another; a flat list can
    /// use [`Reorder::finish`] and ignore the difference.
    pub fn finish_move(&mut self, ui: &mut egui::Ui, colour: egui::Color32) -> Option<Move> {
        let (to, y, x) = self.target?;
        // Only a drag that could land here paints an indicator: a row from
        // an unnamed list belongs to that list alone.
        let dragged = self.dragged.clone()?;
        let foreign = dragged.list != self.id;
        if foreign && (self.scope.is_none() || dragged.scope.is_none()) {
            return None;
        }
        let painter = ui.painter();
        painter.hline(x, y, egui::Stroke::new(2.0, colour));
        // A small nub at the left end, so a thin line is readable against a
        // striped list.
        painter.circle_filled(egui::pos2(x.min + 2.0, y), 3.0, colour);
        let released = ui.input(|i| i.pointer.any_released());
        if !released {
            ui.ctx().request_repaint();
            return None;
        }
        let p = egui::DragAndDrop::take_payload::<DragIndex>(ui.ctx())?;
        if p.list == self.id {
            if to == p.index || to == p.index + 1 {
                return None;
            }
            return Some(Move::Within { from: p.index, to });
        }
        Some(Move::From {
            scope: p.scope.clone()?,
            from: p.index,
            to,
        })
    }
}

/// How wide the grab slot is, whatever the glyph measures; its height
/// follows the row. A list that draws a non-draggable row beside draggable
/// ones (a pinned note) uses the same width for its own leading slot so the
/// rows line up.
pub const GRIP_WIDTH: f32 = 14.0;
const GRIP_HEIGHT: f32 = 22.0;

/// Six dots in two columns, the usual "drag me" mark.
///
/// Painted rather than taken from the icon font: the font HyperStack
/// bundles does not carry every Phosphor codepoint, and a missing one
/// renders as a tofu box — which is what the dots-six glyph did. Six
/// circles cannot go missing.
fn paint_grip(painter: &egui::Painter, rect: egui::Rect, colour: egui::Color32) {
    const RADIUS: f32 = 1.4;
    const GAP_X: f32 = 4.0;
    const GAP_Y: f32 = 4.5;
    let c = rect.center();
    for row in -1..=1 {
        for col in [-1.0f32, 1.0] {
            painter.circle_filled(
                egui::pos2(c.x + col * GAP_X / 2.0, c.y + row as f32 * GAP_Y),
                RADIUS,
                colour,
            );
        }
    }
}

/// Where a row came from, reported by [`drop_onto`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dropped {
    /// What the source list called itself.
    pub scope: String,
    /// The row's index in that list.
    pub index: usize,
}

/// What the row in flight calls its list, without consuming the drag.
/// A tree uses it to decide whether a target should light up at all —
/// a folder does not accept a row it already holds.
pub fn dragged_scope(ctx: &egui::Context) -> Option<String> {
    egui::DragAndDrop::payload::<DragIndex>(ctx)?.scope.clone()
}

/// A drop *onto* something rather than between two rows: a tree's folder,
/// which may be collapsed or empty and so has no rows of its own to aim
/// between. Paints `rect` while a dragged row is over it and reports the
/// source once the pointer is released. Owner's requirement, 2026-10-17.
pub fn drop_onto(ui: &mut egui::Ui, rect: egui::Rect, colour: egui::Color32) -> Option<Dropped> {
    // A row from a list that never named itself (a flat list, such as the
    // collections themselves) cannot be dropped *into* anything: it has no
    // scope to report. Ignoring it here matters - taking the payload to
    // find that out would swallow that list's own reorder.
    let payload = egui::DragAndDrop::payload::<DragIndex>(ui.ctx())?;
    payload.scope.as_ref()?;
    let pointer = ui.input(|i| i.pointer.interact_pos())?;
    if !rect.contains(pointer) {
        return None;
    }
    // A filled wash plus an outline: the line used between rows would say
    // the wrong thing here, because this drop has no position in a list.
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, colour.gamma_multiply(0.20));
    painter.rect_stroke(
        rect,
        egui::CornerRadius::same(4),
        egui::Stroke::new(1.5, colour),
        egui::StrokeKind::Inside,
    );
    if !ui.input(|i| i.pointer.any_released()) {
        ui.ctx().request_repaint();
        return None;
    }
    let p = egui::DragAndDrop::take_payload::<DragIndex>(ui.ctx())?;
    Some(Dropped {
        scope: p.scope.clone()?,
        index: p.index,
    })
}

/// The payload: which row left. Its own type so two lists on screen cannot
/// accept each other's rows.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DragIndex {
    /// The list the row was picked up from.
    list: egui::Id,
    /// What that list calls itself, when it is one level of a tree.
    scope: Option<String>,
    index: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Vec<&'static str> {
        vec!["a", "b", "c", "d"]
    }

    #[test]
    fn a_row_moves_to_the_boundary_it_was_dropped_on() {
        // "a" dropped between c and d.
        let mut v = list();
        move_item(&mut v, 0, 3);
        assert_eq!(v, ["b", "c", "a", "d"]);

        // "d" dropped at the very top.
        let mut v = list();
        move_item(&mut v, 3, 0);
        assert_eq!(v, ["d", "a", "b", "c"]);

        // "b" dropped at the very bottom.
        let mut v = list();
        move_item(&mut v, 1, 4);
        assert_eq!(v, ["a", "c", "d", "b"]);
    }

    #[test]
    fn dropping_a_row_back_where_it_was_changes_nothing() {
        for (from, to) in [(1usize, 1usize), (1, 2), (0, 0), (3, 3), (3, 4)] {
            let mut v = list();
            move_item(&mut v, from, to);
            assert_eq!(v, list(), "{from} -> {to} should be a no-op");
        }
    }

    #[test]
    fn an_impossible_move_is_ignored_rather_than_panicking() {
        let mut v = list();
        move_item(&mut v, 9, 1);
        assert_eq!(v, list());
        move_item(&mut v, 1, 9);
        assert_eq!(v, list());
        let mut empty: Vec<&str> = Vec::new();
        move_item(&mut empty, 0, 0);
        assert!(empty.is_empty());
    }

    #[test]
    fn every_move_keeps_every_item() {
        for from in 0..4 {
            for to in 0..=4 {
                let mut v = list();
                move_item(&mut v, from, to);
                let mut sorted = v.clone();
                sorted.sort_unstable();
                assert_eq!(sorted, ["a", "b", "c", "d"], "{from} -> {to} lost an item");
            }
        }
    }
}
