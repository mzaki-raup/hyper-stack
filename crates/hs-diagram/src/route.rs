//! Port selection and edge routing.

use crate::model::{EdgeRoute, Point};
use crate::text::Rect;

/// Points for an edge. `via` wins when present; otherwise the route picks the
/// ports facing the other box and inserts elbow points.
pub fn route(from: Rect, to: Rect, route: EdgeRoute, via: &[Point]) -> Vec<Point> {
    if !via.is_empty() {
        let mut points = vec![port(from, to, true)];
        points.extend_from_slice(via);
        points.push(port(to, from, false));
        return dedup(points);
    }
    match route {
        EdgeRoute::Straight => dedup(vec![port(from, to, true), port(to, from, false)]),
        EdgeRoute::Curved => {
            let a = port(from, to, true);
            let b = port(to, from, false);
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let ctrl = Point {
                x: (a.x + b.x) / 2.0 - dy / len * len.min(80.0) * 0.25,
                y: (a.y + b.y) / 2.0 + dx / len * len.min(80.0) * 0.25,
            };
            let mut points = Vec::with_capacity(14);
            for i in 0..=12 {
                let t = i as f32 / 12.0;
                let u = 1.0 - t;
                points.push(Point {
                    x: u * u * a.x + 2.0 * u * t * ctrl.x + t * t * b.x,
                    y: u * u * a.y + 2.0 * u * t * ctrl.y + t * t * b.y,
                });
            }
            dedup(points)
        }
        EdgeRoute::Orthogonal => {
            let a = port(from, to, true);
            let b = port(to, from, false);
            let horizontal = (b.x - a.x).abs() >= (b.y - a.y).abs();
            let mut points = if horizontal {
                let mid = (a.x + b.x) / 2.0;
                vec![a, Point { x: mid, y: a.y }, Point { x: mid, y: b.y }, b]
            } else {
                let mid = (a.y + b.y) / 2.0;
                vec![a, Point { x: a.x, y: mid }, Point { x: b.x, y: mid }, b]
            };
            points.dedup();
            dedup(points)
        }
    }
}

/// The port on `from` facing `to` (or, when `outgoing` is false, the port on
/// `from` facing `to` from the other side).
fn port(from: Rect, to: Rect, _outgoing: bool) -> Point {
    let fc = from.center();
    let tc = to.center();
    let dx = tc.x - fc.x;
    let dy = tc.y - fc.y;
    let overlap_x = from.x < to.right() && to.x < from.right();
    let overlap_y = from.y < to.bottom() && to.y < from.bottom();
    // Boxes that overlap on one axis should leave/enter on the other axis:
    // a moved node above another otherwise routes a segment through it.
    let vertical = (overlap_x && !overlap_y) || (!overlap_x && !overlap_y && dy.abs() > dx.abs());
    if vertical {
        Point {
            x: fc.x,
            y: if dy >= 0.0 { from.bottom() } else { from.y },
        }
    } else {
        Point {
            x: if dx >= 0.0 { from.right() } else { from.x },
            y: fc.y,
        }
    }
}

fn dedup(mut points: Vec<Point>) -> Vec<Point> {
    points.dedup_by(|a, b| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01);
    points
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orthogonal_has_elbows() {
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(300.0, 120.0, 100.0, 40.0);
        let p = route(a, b, EdgeRoute::Orthogonal, &[]);
        assert!(p.len() >= 3);
        assert_eq!(p.first().unwrap().x, 100.0);
        assert_eq!(p.last().unwrap().x, 300.0);
    }

    #[test]
    fn via_is_respected() {
        let a = Rect::new(0.0, 0.0, 100.0, 40.0);
        let b = Rect::new(300.0, 120.0, 100.0, 40.0);
        let via = [Point { x: 150.0, y: 200.0 }];
        let p = route(a, b, EdgeRoute::Straight, &via);
        assert!(p.iter().any(|q| q.y == 200.0));
    }
}
