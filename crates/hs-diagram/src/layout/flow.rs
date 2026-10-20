//! Ranked flow layout: longest-path ranks, barycentre ordering and a
//! top-down placement. Cycles are tolerated by capping the rank sweep.

use crate::model::DiagramDoc;
use crate::text::{Measure, Rect};
use std::collections::VecDeque;

const NODE_GAP: f32 = 36.0;
const RANK_GAP: f32 = 72.0;

pub fn place(doc: &DiagramDoc, measure: Measure<'_>) -> Vec<Rect> {
    let n = doc.nodes.len();
    if n == 0 {
        return Vec::new();
    }
    let index = |id: &str| doc.nodes.iter().position(|node| node.id == id);
    let pairs: Vec<(usize, usize)> = doc
        .edges
        .iter()
        .filter_map(|e| {
            let a = index(&e.from)?;
            let b = index(&e.to)?;
            (a != b).then_some((a, b))
        })
        .collect();

    // Longest path from the sources; stop increasing after n passes so a
    // cycle cannot make the ranks unbounded.
    let mut rank = vec![0usize; n];
    for _ in 0..n {
        let mut changed = false;
        for &(a, b) in &pairs {
            if rank[b] < rank[a] + 1 && rank[a] + 1 < n {
                rank[b] = rank[a] + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let ranks = rank.iter().copied().max().unwrap_or(0) + 1;
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); ranks];
    for (i, &r) in rank.iter().enumerate() {
        rows[r].push(i);
    }

    // Barycentre ordering: two downward sweeps, deterministic ties by id.
    for _ in 0..2 {
        for r in 1..ranks {
            let prev: Vec<usize> = rows[r - 1].clone();
            let pos: Vec<(usize, f32)> = prev
                .iter()
                .enumerate()
                .map(|(i, &v)| (v, i as f32))
                .collect();
            let key = |v: usize| -> f32 {
                let mut total = 0.0;
                let mut count = 0usize;
                for &(a, b) in &pairs {
                    if b == v
                        && let Some((_, p)) = pos.iter().find(|(x, _)| *x == a)
                    {
                        total += *p;
                        count += 1;
                    }
                }
                if count == 0 {
                    rows[r].iter().position(|&x| x == v).unwrap_or(0) as f32
                } else {
                    total / count as f32
                }
            };
            let keys: Vec<f32> = rows[r].iter().map(|&v| key(v)).collect();
            let mut paired: Vec<(usize, f32)> = rows[r].iter().copied().zip(keys).collect();
            paired.sort_by(|(a, ka), (b, kb)| {
                ka.partial_cmp(kb)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| doc.nodes[*a].id.cmp(&doc.nodes[*b].id))
            });
            rows[r] = paired.into_iter().map(|(v, _)| v).collect();
        }
    }

    let sizes: Vec<_> = doc
        .nodes
        .iter()
        .map(|node| crate::layout::node_size(node, measure))
        .collect();
    let row_widths: Vec<f32> = rows
        .iter()
        .map(|row| {
            let sum: f32 = row.iter().map(|&i| sizes[i].w).sum();
            sum + NODE_GAP * (row.len().saturating_sub(1)) as f32
        })
        .collect();
    let widest = row_widths.iter().copied().fold(0.0_f32, f32::max);
    let mut rects = vec![Rect::default(); n];
    let mut y = 0.0f32;
    for (r, row) in rows.iter().enumerate() {
        let row_h = row.iter().map(|&i| sizes[i].h).fold(0.0_f32, f32::max);
        let mut x = (widest - row_widths[r]) / 2.0;
        for &i in row {
            rects[i] = Rect::new(x, y + (row_h - sizes[i].h) / 2.0, sizes[i].w, sizes[i].h);
            x += sizes[i].w + NODE_GAP;
        }
        y += row_h + RANK_GAP;
    }
    rects
}

/// Topological order used by the generator/Tests to explain the ranks.
pub fn order(doc: &DiagramDoc) -> Vec<String> {
    let n = doc.nodes.len();
    let mut indegree = vec![0usize; n];
    let index = |id: &str| doc.nodes.iter().position(|node| node.id == id);
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for edge in &doc.edges {
        if let (Some(a), Some(b)) = (index(&edge.from), index(&edge.to))
            && a != b
        {
            out[a].push(b);
            indegree[b] += 1;
        }
    }
    let mut queue: VecDeque<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut order = Vec::new();
    while let Some(i) = queue.pop_front() {
        order.push(doc.nodes[i].id.clone());
        for &b in &out[i] {
            indegree[b] -= 1;
            if indegree[b] == 0 {
                queue.push_back(b);
            }
        }
    }
    // A cycle leaves nodes out; append them in input order so the result is
    // still deterministic and complete.
    for node in &doc.nodes {
        if !order.contains(&node.id) {
            order.push(node.id.clone());
        }
    }
    order
}
