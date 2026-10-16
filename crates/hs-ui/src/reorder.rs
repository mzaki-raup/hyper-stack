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

/// State for one list being drawn this frame.
pub struct Reorder {
    id: egui::Id,
    /// Which row is being dragged, as the payload says.
    dragged: Option<usize>,
    /// The boundary the pointer is nearest, and where to paint the line.
    target: Option<(usize, f32, egui::Rangef)>,
}

impl Reorder {
    pub fn new(id_salt: impl Hash + Debug) -> Self {
        Self {
            id: egui::Id::new("hs-reorder").with(id_salt),
            dragged: None,
            target: None,
        }
    }

    /// Whether a row of this list is being dragged right now, so the caller
    /// can dim it or suppress a hover effect.
    pub fn dragging(&self) -> bool {
        self.dragged.is_some()
    }

    /// Draw row `index`. The row becomes a drag handle and a drop target;
    /// `add` draws its contents as usual and its value is returned
    /// untouched, so an existing row body needs no changes.
    pub fn row<R>(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        add: impl FnOnce(&mut egui::Ui) -> R,
    ) -> R {
        let row_id = self.id.with(index);
        let egui::InnerResponse { inner, response } =
            ui.dnd_drag_source(row_id, DragIndex(index), add);
        if let Some(p) = response.dnd_hover_payload::<DragIndex>() {
            self.dragged = Some(p.0);
            // Above or below this row, whichever edge the pointer is nearer:
            // that is the boundary the row would land on.
            let rect = response.rect;
            let pointer = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
            let (boundary, y) = if pointer.y < rect.center().y {
                (index, rect.top())
            } else {
                (index + 1, rect.bottom())
            };
            self.target = Some((boundary, y, rect.x_range()));
        }
        inner
    }

    /// The move the user made, once every row has been drawn. Call this
    /// after the loop: it also paints the indicator, which must go on top
    /// of the rows.
    pub fn finish(&mut self, ui: &mut egui::Ui, colour: egui::Color32) -> Option<(usize, usize)> {
        let (to, y, x) = self.target?;
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
        let from = egui::DragAndDrop::take_payload::<DragIndex>(ui.ctx()).map(|p| p.0)?;
        if to == from || to == from + 1 {
            return None;
        }
        Some((from, to))
    }
}

/// The payload: which row left. Its own type so two lists on screen cannot
/// accept each other's rows.
#[derive(Clone, Copy, Debug)]
struct DragIndex(usize);

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
