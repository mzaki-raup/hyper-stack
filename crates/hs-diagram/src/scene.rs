//! Resolved geometry shared by the egui painter, SVG, PNG and PDF. Layout
//! stores final text lines here; no consumer re-wraps.

use crate::model::{NodeKind, Point, Style};
use crate::text::{Rect, Size};

#[derive(Debug, Clone, PartialEq)]
pub enum Prim {
    Shape {
        rect: Rect,
        kind: NodeKind,
        label: String,
        style: Style,
    },
    Frame {
        rect: Rect,
        label: String,
    },
    Line {
        points: Vec<Point>,
        dashed: bool,
        arrow_start: bool,
        arrow_end: bool,
        style: Style,
    },
    Text {
        at: Point,
        label: String,
        boxed: bool,
        style: Style,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub size: Size,
    pub prims: Vec<Prim>,
}

impl Scene {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            prims: Vec::new(),
        }
    }

    pub fn push(&mut self, prim: Prim) {
        self.prims.push(prim);
    }

    /// Bounds of every primitive, used to fit a page and to translate a
    /// scene so it starts at the margin.
    pub fn bounds(&self) -> Rect {
        let mut bounds: Option<Rect> = None;
        for prim in &self.prims {
            let r = match prim {
                Prim::Shape { rect, .. } | Prim::Frame { rect, .. } => *rect,
                Prim::Line { points, .. } => {
                    let mut min = Point {
                        x: f32::INFINITY,
                        y: f32::INFINITY,
                    };
                    let mut max = Point {
                        x: f32::NEG_INFINITY,
                        y: f32::NEG_INFINITY,
                    };
                    for p in points {
                        min.x = min.x.min(p.x);
                        min.y = min.y.min(p.y);
                        max.x = max.x.max(p.x);
                        max.y = max.y.max(p.y);
                    }
                    if points.is_empty() {
                        Rect::new(0.0, 0.0, 0.0, 0.0)
                    } else {
                        Rect::new(min.x, min.y, max.x - min.x, max.y - min.y)
                    }
                }
                Prim::Text { at, label, .. } => {
                    let lines = label.lines().count().max(1) as f32;
                    Rect::new(at.x - 40.0, at.y - lines * 9.0, 80.0, lines * 18.0)
                }
            };
            bounds = Some(match bounds {
                Some(b) => b.union(&r),
                None => r,
            });
        }
        bounds.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0))
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        for prim in &mut self.prims {
            match prim {
                Prim::Shape { rect, .. } | Prim::Frame { rect, .. } => {
                    rect.x += dx;
                    rect.y += dy;
                }
                Prim::Line { points, .. } => {
                    for p in points {
                        p.x += dx;
                        p.y += dy;
                    }
                }
                Prim::Text { at, .. } => {
                    at.x += dx;
                    at.y += dy;
                }
            }
        }
    }
}

/// Default fill and stroke per shape kind. Kept here so the painter and the
/// SVG writer agree and a style override is a simple replacement.
pub fn default_fill(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Process => "#f5f7fb",
        NodeKind::RoundedRectangle => "#f5f7fb",
        NodeKind::Ellipse => "#eef4ff",
        NodeKind::Decision => "#fff4e5",
        NodeKind::StartEnd => "#e8f7ee",
        NodeKind::InputOutput => "#eef4ff",
        NodeKind::Database => "#eef0ff",
        NodeKind::Note => "#fffbe6",
        NodeKind::Text => "none",
        NodeKind::Container => "none",
        NodeKind::Frontend => "#e8f0ff",
        NodeKind::Backend => "#e8f7ee",
        NodeKind::Store => "#f3e8ff",
        NodeKind::Queue => "#fff0e6",
        NodeKind::External => "#f1f3f5",
    }
}

pub fn default_stroke(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Note => "#b58b00",
        NodeKind::Container => "#8a94a6",
        NodeKind::Text => "none",
        _ => "#5b6472",
    }
}
