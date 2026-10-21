//! Deterministic layout: flow ranks and architecture grids, both driven by a
//! measure callback so the core has no fonts and no window.

pub mod flow;
pub mod grid;
mod sequence;

use crate::model::{DiagramDoc, DiagramKind, Node, Point};
use crate::scene::{Prim, Scene};
use crate::text::{Measure, Rect, Size, label_lines};

/// Node size for automatic layout: wrapped text plus padding, with the shapes
/// that need extra room (decision, stadium, database) accounted for.
pub fn node_size(node: &Node, measure: Measure<'_>) -> Size {
    let (_, text) = label_lines(&node.label, 160.0, measure);
    let mut w = (text.w + 28.0).clamp(90.0, 220.0);
    let mut h = (text.h + 20.0).clamp(48.0, 160.0);
    match node.kind {
        crate::model::NodeKind::Decision => {
            w *= 1.35;
            h *= 1.2;
        }
        crate::model::NodeKind::StartEnd => h = h.max(44.0),
        crate::model::NodeKind::Database => h += 8.0,
        crate::model::NodeKind::Container => {
            w = w.max(140.0);
            h = h.max(90.0);
        }
        _ => {}
    }
    Size::new(w, h)
}

pub fn layout(doc: &DiagramDoc, measure: Measure<'_>) -> Scene {
    let rects = if doc.auto_layout || doc.kind == DiagramKind::Sequence {
        match doc.kind {
            DiagramKind::Flow => flow::place(doc, measure),
            DiagramKind::Sequence => sequence::place(doc),
            DiagramKind::Architecture | DiagramKind::Erd => grid::place(doc, measure),
        }
    } else {
        geometry(doc, measure)
    };
    build(doc, &rects, measure)
}

fn geometry(doc: &DiagramDoc, measure: Measure<'_>) -> Vec<Rect> {
    doc.nodes
        .iter()
        .map(|n| {
            let auto = node_size(n, measure);
            Rect::new(
                n.x,
                n.y,
                if n.w > 0.0 { n.w } else { auto.w },
                if n.h > 0.0 { n.h } else { auto.h },
            )
        })
        .collect()
}

/// Paint groups, nodes and edges in that order, then fit the page.
fn build(doc: &DiagramDoc, rects: &[Rect], measure: Measure<'_>) -> Scene {
    let index = |id: &str| doc.nodes.iter().position(|n| n.id == id);
    let mut scene = Scene::default();
    for group in &doc.groups {
        let mut frame: Option<Rect> = None;
        for id in &group.nodes {
            if let Some(i) = index(id) {
                frame = Some(match frame {
                    Some(r) => r.union(&rects[i]),
                    None => rects[i],
                });
            }
        }
        if let Some(r) = frame {
            scene.push(Prim::Frame {
                rect: r.inflate(18.0),
                label: group.label.clone(),
                style: group.style.clone(),
                label_placement: group.label_placement,
                label_align: group.label_align,
            });
        }
    }
    for (i, node) in doc.nodes.iter().enumerate() {
        let (label, _) = label_lines(&node.label, (rects[i].w - 24.0).max(40.0), measure);
        scene.push(Prim::Shape {
            rect: rects[i],
            kind: node.kind,
            label,
            style: node.style.clone(),
        });
    }
    if doc.kind == DiagramKind::Sequence {
        sequence::decorate(&mut scene, doc, rects);
    } else {
        for edge in &doc.edges {
            let (Some(a), Some(b)) = (index(&edge.from), index(&edge.to)) else {
                continue;
            };
            let points = crate::route::route(rects[a], rects[b], edge.route, &edge.via);
            let (arrow_start, arrow_end) = match edge.arrow {
                crate::model::Arrow::None => (false, false),
                crate::model::Arrow::End => (false, true),
                crate::model::Arrow::Both => (true, true),
            };
            scene.push(Prim::Line {
                points: points.clone(),
                dashed: edge.dashed,
                dotted: edge.dotted,
                arrow_start,
                arrow_end,
                style: edge.style.clone(),
            });
            if !edge.label.is_empty()
                && let Some(at) = edge.label_at.or_else(|| midpoint(&points))
            {
                scene.push(Prim::Text {
                    at,
                    label: edge.label.clone(),
                    boxed: true,
                    style: crate::model::Style {
                        font_size: Some(12.0),
                        ..Default::default()
                    },
                });
            }
        }
    }
    if doc.show_title && !doc.nodes.is_empty() {
        let b = scene.bounds();
        scene.push(Prim::Frame {
            rect: b.inflate(24.0),
            label: doc.title.clone(),
            style: crate::model::Style {
                dashed: true,
                ..Default::default()
            },
            label_placement: doc.title_placement,
            label_align: doc.title_align,
        });
    }
    let margin = doc.page.margin.clamp(8.0, 160.0);
    let bounds = scene.bounds();
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        scene.size = Size::new(400.0, 300.0);
        return scene;
    }
    scene.translate(margin - bounds.x, margin - bounds.y);
    scene.size = Size::new(
        (bounds.w + margin * 2.0).max(120.0),
        (bounds.h + margin * 2.0).max(90.0),
    );
    scene
}

fn midpoint(points: &[Point]) -> Option<Point> {
    if points.len() < 2 {
        return points.first().copied();
    }
    let i = (points.len() - 1) / 2;
    let a = points[i];
    let b = points[i + 1];
    Some(Point {
        x: (a.x + b.x) / 2.0,
        y: (a.y + b.y) / 2.0,
    })
}
