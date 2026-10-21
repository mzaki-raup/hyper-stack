//! Sequence layout: participants across the top, dashed lifelines and a
//! horizontal message arrow per edge.

use crate::model::DiagramDoc;
use crate::scene::{Prim, Scene};
use crate::text::Rect;

const LEFT: f32 = 40.0;
const TOP: f32 = 40.0;
const BOX_H: f32 = 52.0;
const GAP: f32 = 72.0;
const FIRST_MESSAGE: f32 = 140.0;
const STEP: f32 = 64.0;

pub fn place(doc: &DiagramDoc) -> Vec<Rect> {
    let mut x = LEFT;
    doc.nodes
        .iter()
        .map(|node| {
            let chars = node
                .label
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .count() as f32;
            let w = (chars * 7.5 + 32.0).clamp(120.0, 220.0);
            let rect = Rect {
                x,
                y: TOP,
                w,
                h: BOX_H,
            };
            x += w + GAP;
            rect
        })
        .collect()
}

/// Draw the lifelines, message arrows and message labels; the generic edge
/// router is skipped for this kind.
pub fn decorate(scene: &mut Scene, doc: &DiagramDoc, rects: &[Rect]) {
    let last = FIRST_MESSAGE + (doc.edges.len().saturating_sub(1)) as f32 * STEP + 60.0;
    for rect in rects {
        let x = rect.x + rect.w / 2.0;
        scene.push(Prim::Line {
            points: vec![
                crate::model::Point {
                    x,
                    y: rect.y + rect.h,
                },
                crate::model::Point { x, y: last },
            ],
            dashed: true,
            dotted: false,
            arrow_start: false,
            arrow_end: false,
            style: crate::model::Style::default(),
        });
    }
    for (i, edge) in doc.edges.iter().enumerate() {
        let (Some(a), Some(b)) = (
            doc.nodes.iter().position(|n| n.id == edge.from),
            doc.nodes.iter().position(|n| n.id == edge.to),
        ) else {
            continue;
        };
        let (Some(a), Some(b)) = (rects.get(a), rects.get(b)) else {
            continue;
        };
        let y = FIRST_MESSAGE + i as f32 * STEP;
        let from = crate::model::Point {
            x: a.x + a.w / 2.0,
            y,
        };
        let to = crate::model::Point {
            x: b.x + b.w / 2.0,
            y,
        };
        scene.push(Prim::Line {
            points: vec![from, to],
            dashed: edge.dashed,
            dotted: edge.dotted,
            arrow_start: false,
            arrow_end: true,
            style: edge.style.clone(),
        });
        if !edge.label.is_empty() {
            scene.push(Prim::Text {
                at: crate::model::Point {
                    x: (from.x + to.x) / 2.0,
                    y: y - 14.0,
                },
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

#[cfg(test)]
mod tests {
    use crate::layout;
    use crate::scene::Prim;
    use crate::text::approx_measure;

    #[test]
    fn participants_sit_on_one_row_and_messages_are_horizontal() {
        let doc = crate::template::sequence();
        let scene = layout::layout(&doc, &approx_measure);
        let boxes: Vec<_> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert_eq!(boxes.len(), 3);
        assert!(boxes.windows(2).all(|w| w[0].y == w[1].y));
        let messages: Vec<_> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Line { points, .. } if !points.is_empty() => Some(points.clone()),
                _ => None,
            })
            .collect();
        assert!(messages.iter().any(|p| p.len() == 2 && p[0].y == p[1].y));
    }
}
