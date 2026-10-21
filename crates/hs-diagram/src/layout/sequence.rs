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
            edge_id: None,
        });
    }
    for fragment in &doc.fragments {
        if doc.edges.is_empty() || rects.is_empty() {
            continue;
        }
        let from = fragment.from.min(doc.edges.len() - 1);
        if fragment.kind == crate::model::FragmentKind::Note {
            // A note sits in the gap after message `from`, across the
            // lifelines it is about; the whole width keeps it readable
            // without anchoring it to one participant.
            let first = &rects[0];
            let last = &rects[rects.len() - 1];
            let y = FIRST_MESSAGE + from as f32 * STEP + STEP / 2.0 - 14.0;
            scene.push(Prim::Shape {
                rect: Rect {
                    x: first.x + first.w / 2.0,
                    y,
                    w: (last.x + last.w / 2.0) - (first.x + first.w / 2.0),
                    h: 28.0,
                },
                kind: crate::model::NodeKind::Note,
                label: fragment.label.clone(),
                style: crate::model::Style {
                    fill: Some("#fff8c5".into()),
                    stroke: Some("#d4a72c".into()),
                    ..Default::default()
                },
            });
            continue;
        }
        let to = fragment.to.max(from).min(doc.edges.len() - 1);
        let y = FIRST_MESSAGE + from as f32 * STEP - 30.0;
        let bottom = FIRST_MESSAGE + to as f32 * STEP + 20.0;
        let first = &rects[0];
        let last = &rects[rects.len() - 1];
        scene.push(Prim::Frame {
            rect: Rect {
                x: first.x - 12.0,
                y,
                w: last.x + last.w - first.x + 24.0,
                h: bottom - y,
            },
            label: format!(
                "{} {}",
                match fragment.kind {
                    crate::model::FragmentKind::Alt => "alt",
                    crate::model::FragmentKind::Loop => "loop",
                    crate::model::FragmentKind::Opt => "opt",
                    crate::model::FragmentKind::Note => "note",
                },
                fragment.label
            ),
            style: crate::model::Style::default(),
            label_placement: crate::model::LabelPlacement::Top,
            label_align: crate::model::LabelAlign::Left,
            group_id: None,
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
        scene.push(Prim::Shape {
            rect: Rect {
                x: to.x - 4.0,
                y: y - 10.0,
                w: 8.0,
                h: 26.0,
            },
            kind: crate::model::NodeKind::Process,
            label: String::new(),
            style: crate::model::Style {
                fill: Some("#e8f0ff".into()),
                ..Default::default()
            },
        });
        scene.push(Prim::Line {
            points: vec![from, to],
            dashed: edge.dashed,
            dotted: edge.dotted,
            arrow_start: false,
            arrow_end: true,
            style: edge.style.clone(),
            edge_id: Some(edge.id.clone()),
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
                edge_id: Some(edge.id.clone()),
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
    fn a_fragment_draws_a_frame_and_messages_draw_activations() {
        let mut doc = crate::template::sequence();
        doc.fragments.push(crate::model::Fragment {
            kind: crate::model::FragmentKind::Opt,
            label: "when signed in".into(),
            from: 0,
            to: 1,
        });
        let scene = layout::layout(&doc, &approx_measure);
        assert!(scene.prims.iter().any(|p| match p {
            Prim::Frame { label, .. } => label.contains("opt when signed in"),
            _ => false,
        }));
        let activations = scene
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Shape { label, .. } if label.is_empty()))
            .count();
        assert!(activations >= 2);
    }

    #[test]
    fn a_note_fragment_draws_a_note_strip_between_messages() {
        let mut doc = crate::template::sequence();
        doc.fragments.push(crate::model::Fragment {
            kind: crate::model::FragmentKind::Note,
            label: "retries once".into(),
            from: 0,
            to: 0,
        });
        let scene = layout::layout(&doc, &approx_measure);
        let note = scene.prims.iter().find_map(|p| match p {
            Prim::Shape {
                rect,
                kind: crate::model::NodeKind::Note,
                label,
                ..
            } if label == "retries once" => Some(*rect),
            _ => None,
        });
        let note = note.expect("the note is drawn as a Note shape");
        // Between the first and the second message, not on either.
        let first = super::FIRST_MESSAGE;
        assert!(note.y > first && note.y + note.h < first + super::STEP);
        // No frame is drawn for a note.
        assert!(
            !scene
                .prims
                .iter()
                .any(|p| matches!(p, Prim::Frame { label, .. } if label.contains("note")))
        );
    }

    #[test]
    fn participants_sit_on_one_row_and_messages_are_horizontal() {
        let doc = crate::template::sequence();
        let scene = layout::layout(&doc, &approx_measure);
        let boxes: Vec<_> = scene
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Shape { rect, label, .. } if !label.is_empty() => Some(*rect),
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
