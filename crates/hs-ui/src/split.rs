//! Binary split layout for the main area (docs/04 §2.4). Leaves hold tab ids;
//! the shell draws each leaf through a callback.

use egui::{Rect, Sense, Vec2};

use crate::theme::Theme;

pub const HANDLE: f32 = 6.0;
pub const MIN_PANE: f32 = 120.0;
pub const MAX_DEPTH: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum SplitNode {
    Leaf(u64),
    Split {
        horizontal: bool,
        ratio: f32,
        a: Box<SplitNode>,
        b: Box<SplitNode>,
    },
}

impl SplitNode {
    pub fn leaves(&self) -> Vec<u64> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { a, b, .. } => {
                let mut v = a.leaves();
                v.extend(b.leaves());
                v
            }
        }
    }

    pub fn contains(&self, id: u64) -> bool {
        self.leaves().contains(&id)
    }

    pub fn depth(&self) -> usize {
        match self {
            Self::Leaf(_) => 0,
            Self::Split { a, b, .. } => 1 + a.depth().max(b.depth()),
        }
    }

    /// Replace leaf `target` with a split holding `target` and `new_id`.
    pub fn split_leaf(&mut self, target: u64, new_id: u64, horizontal: bool) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                *self = Self::Split {
                    horizontal,
                    ratio: 0.5,
                    a: Box::new(Self::Leaf(target)),
                    b: Box::new(Self::Leaf(new_id)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { a, b, .. } => {
                a.split_leaf(target, new_id, horizontal) || b.split_leaf(target, new_id, horizontal)
            }
        }
    }

    /// Remove a leaf; a split with one side gone collapses to the other side.
    /// Returns false when the tree would become empty.
    pub fn remove_leaf(&mut self, target: u64) -> bool {
        match self {
            Self::Leaf(id) => *id != target,
            Self::Split { a, b, .. } => {
                if matches!(**a, Self::Leaf(id) if id == target) {
                    let keep = std::mem::replace(&mut **b, Self::Leaf(0));
                    *self = keep;
                    return true;
                }
                if matches!(**b, Self::Leaf(id) if id == target) {
                    let keep = std::mem::replace(&mut **a, Self::Leaf(0));
                    *self = keep;
                    return true;
                }
                a.remove_leaf(target) && b.remove_leaf(target)
            }
        }
    }

    /// Replace one leaf id with another (switching the tab shown in a pane).
    pub fn replace_leaf(&mut self, from: u64, to: u64) -> bool {
        match self {
            Self::Leaf(id) if *id == from => {
                *id = to;
                true
            }
            Self::Leaf(_) => false,
            Self::Split { a, b, .. } => a.replace_leaf(from, to) || b.replace_leaf(from, to),
        }
    }

    /// Draw: `draw_leaf(ui, tab_id)` for each pane, with draggable handles.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        theme: &Theme,
        draw_leaf: &mut dyn FnMut(&mut egui::Ui, u64),
    ) {
        let rect = ui.available_rect_before_wrap();
        self.show_in(ui, rect, theme, draw_leaf, 0);
    }

    fn show_in(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        theme: &Theme,
        draw_leaf: &mut dyn FnMut(&mut egui::Ui, u64),
        depth: usize,
    ) {
        match self {
            Self::Leaf(id) => {
                let id = *id;
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(rect)
                        .id_salt(("pane", id, depth)),
                    |ui| {
                        draw_leaf(ui, id);
                    },
                );
            }
            Self::Split {
                horizontal,
                ratio,
                a,
                b,
            } => {
                let handle_id = ui.id().with((
                    "split-handle",
                    depth,
                    a.leaves().first().copied().unwrap_or(0),
                ));
                let (ra, rb, handle_rect) = if *horizontal {
                    let x = rect.left() + rect.width() * *ratio;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(x - HANDLE / 2.0, rect.max.y)),
                        Rect::from_min_max(egui::pos2(x + HANDLE / 2.0, rect.min.y), rect.max),
                        Rect::from_center_size(
                            egui::pos2(x, rect.center().y),
                            Vec2::new(HANDLE, rect.height()),
                        ),
                    )
                } else {
                    let y = rect.top() + rect.height() * *ratio;
                    (
                        Rect::from_min_max(rect.min, egui::pos2(rect.max.x, y - HANDLE / 2.0)),
                        Rect::from_min_max(egui::pos2(rect.min.x, y + HANDLE / 2.0), rect.max),
                        Rect::from_center_size(
                            egui::pos2(rect.center().x, y),
                            Vec2::new(rect.width(), HANDLE),
                        ),
                    )
                };
                a.show_in(ui, ra, theme, draw_leaf, depth + 1);
                b.show_in(ui, rb, theme, draw_leaf, depth + 1);
                let r = ui.interact(handle_rect, handle_id, Sense::click_and_drag());
                let colour = if r.hovered() || r.dragged() {
                    theme.accent
                } else {
                    theme.border
                };
                ui.painter().rect_filled(
                    handle_rect.shrink(if *horizontal { 2.0 } else { 0.0 }),
                    0.0,
                    colour,
                );
                if r.dragged() {
                    let d = r.drag_delta();
                    let size = if *horizontal {
                        rect.width()
                    } else {
                        rect.height()
                    };
                    // Below two minimum panes the divider stays put: relaxing
                    // the minimum to a share of a tiny pane let both halves
                    // shrink under every tool's own side-panel minimum.
                    if size >= MIN_PANE * 2.0 {
                        let delta = if *horizontal { d.x } else { d.y } / size;
                        let min = MIN_PANE / size;
                        *ratio = (*ratio + delta).clamp(min, 1.0 - min);
                    }
                }
                if r.hovered() || r.dragged() {
                    ui.ctx().set_cursor_icon(if *horizontal {
                        egui::CursorIcon::ResizeHorizontal
                    } else {
                        egui::CursorIcon::ResizeVertical
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_remove_and_collapse() {
        let mut t = SplitNode::Leaf(1);
        assert!(t.split_leaf(1, 2, true));
        assert!(t.split_leaf(2, 3, false));
        assert_eq!(t.leaves(), vec![1, 2, 3]);
        assert_eq!(t.depth(), 2);
        assert!(t.remove_leaf(2));
        assert_eq!(t.leaves(), vec![1, 3]);
        assert!(t.remove_leaf(1));
        assert_eq!(t, SplitNode::Leaf(3));
        assert!(!t.remove_leaf(3));
        assert!(!t.split_leaf(99, 4, true));
    }

    #[test]
    fn replace_leaf_switches_pane_content() {
        let mut t = SplitNode::Leaf(1);
        t.split_leaf(1, 2, true);
        assert!(t.replace_leaf(2, 7));
        assert_eq!(t.leaves(), vec![1, 7]);
        assert!(!t.replace_leaf(2, 8));
    }
}
