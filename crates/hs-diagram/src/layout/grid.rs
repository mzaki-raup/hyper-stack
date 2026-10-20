//! Architecture grid: columns sized to the widest component, rows to the
//! tallest. Boundaries are painted by the shared scene builder.

use crate::model::DiagramDoc;
use crate::text::{Measure, Rect};

const GAP_X: f32 = 56.0;
const GAP_Y: f32 = 48.0;

pub fn place(doc: &DiagramDoc, measure: Measure<'_>) -> Vec<Rect> {
    let n = doc.nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let sizes: Vec<_> = doc
        .nodes
        .iter()
        .map(|node| crate::layout::node_size(node, measure))
        .collect();
    let cols = (n as f32).sqrt().ceil() as usize;
    let cols = cols.clamp(1, n);
    let rows = n.div_ceil(cols);
    let mut col_w = vec![0.0_f32; cols];
    let mut row_h = vec![0.0_f32; rows];
    for i in 0..n {
        col_w[i % cols] = col_w[i % cols].max(sizes[i].w);
        row_h[i / cols] = row_h[i / cols].max(sizes[i].h);
    }
    let mut x_at = vec![0.0_f32; cols];
    let mut x = 0.0;
    for (c, w) in col_w.iter().enumerate() {
        x_at[c] = x;
        x += w + GAP_X;
    }
    let mut y_at = vec![0.0_f32; rows];
    let mut y = 0.0;
    for (r, h) in row_h.iter().enumerate() {
        y_at[r] = y;
        y += h + GAP_Y;
    }
    let mut rects = Vec::with_capacity(n);
    for (i, size) in sizes.iter().enumerate() {
        let c = i % cols;
        let r = i / cols;
        let w = size.w;
        let h = size.h;
        rects.push(Rect::new(
            x_at[c] + (col_w[c] - w) / 2.0,
            y_at[r] + (row_h[r] - h) / 2.0,
            w,
            h,
        ));
    }
    rects
}
