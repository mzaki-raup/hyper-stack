//! Lane assignment for the commit graph (docs/tools/git-activity.md
//! §5.3): which column each commit sits in and which columns its
//! parents continue in, worked out from the parent list alone so the
//! History view can paint the lines itself. Pure, and tested without
//! git.
//!
//! Commits arrive newest first in topological order (`git log
//! --topo-order`). A lane holds the id of the commit it is waiting for;
//! a commit takes the lane that waits for it (or a new one), hands that
//! lane to its first parent and gives every other parent a lane of its
//! own - an existing one when something already waits for that parent,
//! which is where two lines join.

use crate::Commit;

/// Where one commit is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The column of the commit's dot.
    pub lane: usize,
    /// For each parent, the column its line continues in below this row.
    pub parents: Vec<usize>,
    /// Columns occupied *before* this row (lines passing through from
    /// above), including this commit's own.
    pub above: Vec<usize>,
    /// Columns occupied *after* this row (lines continuing below),
    /// including the parents'.
    pub below: Vec<usize>,
    /// Columns from above whose line ends in this commit's dot: a lane
    /// that was waiting for this commit's first parent and gives way to
    /// the commit's own column, so the main line stays where it is.
    pub joins: Vec<usize>,
}

impl Row {
    /// The widest column this row touches, for sizing the graph strip.
    pub fn width(&self) -> usize {
        self.above
            .iter()
            .chain(self.below.iter())
            .chain(std::iter::once(&self.lane))
            .copied()
            .max()
            .unwrap_or(0)
            + 1
    }
}

/// Assign lanes to `commits` (newest first, topological order).
pub fn lanes(commits: &[Commit]) -> Vec<Row> {
    // `lanes[i]` is the id the column waits for; `None` is a free column.
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    for c in commits {
        let above: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.as_ref().map(|_| i))
            .collect();
        let lane = match lanes.iter().position(|l| l.as_deref() == Some(&c.id)) {
            Some(i) => i,
            None => {
                let i = lanes
                    .iter()
                    .position(Option::is_none)
                    .unwrap_or(lanes.len());
                if i == lanes.len() {
                    lanes.push(None);
                }
                i
            }
        };
        // Any other column also waiting for this commit (a merge whose
        // second parent is this one) joins here and frees up.
        for l in lanes.iter_mut() {
            if l.as_deref() == Some(&c.id) {
                *l = None;
            }
        }
        let mut parents = Vec::with_capacity(c.parents.len());
        let mut joins = Vec::new();
        for (n, p) in c.parents.iter().enumerate() {
            let existing = lanes.iter().position(|l| l.as_deref() == Some(p.as_str()));
            let col = match (n, existing) {
                // The first parent keeps this column.
                (0, None) => {
                    lanes[lane] = Some(p.clone());
                    lane
                }
                // Already waited for in a column to the right: that line
                // joins here and this column carries the parent on, so
                // the main line never drifts right. To the left, this
                // line joins the other one instead.
                (0, Some(i)) if i > lane => {
                    lanes[i] = None;
                    lanes[lane] = Some(p.clone());
                    joins.push(i);
                    lane
                }
                (_, Some(i)) => i,
                (_, None) => {
                    let i = lanes
                        .iter()
                        .position(Option::is_none)
                        .unwrap_or(lanes.len());
                    if i == lanes.len() {
                        lanes.push(None);
                    }
                    lanes[i] = Some(p.clone());
                    i
                }
            };
            parents.push(col);
        }
        // Trim trailing free columns so the strip stays narrow.
        while lanes.last().is_some_and(Option::is_none) {
            lanes.pop();
        }
        let below: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter_map(|(i, l)| l.as_ref().map(|_| i))
            .collect();
        rows.push(Row {
            lane,
            parents,
            above,
            below,
            joins,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, parents: &[&str]) -> Commit {
        Commit {
            id: id.into(),
            short: id.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: Vec::new(),
            subject: String::new(),
        }
    }

    #[test]
    fn a_straight_line_stays_in_one_lane() {
        let rows = lanes(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(rows[0].parents, [0]);
        assert_eq!(rows[2].parents, Vec::<usize>::new());
        assert!(rows[2].below.is_empty(), "the root ends the line");
    }

    /// m merges f into main: f gets lane 1, and both lines join at a.
    #[test]
    fn a_merge_opens_a_second_lane_that_joins_at_the_base() {
        let rows = lanes(&[
            c("m", &["b", "f"]),
            c("f", &["a"]),
            c("b", &["a"]),
            c("a", &[]),
        ]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].parents, [0, 1]);
        assert_eq!(
            rows[1].lane, 1,
            "the feature commit sits in the lane the merge opened"
        );
        assert_eq!(rows[1].parents, [1], "its line carries on in its own lane");
        assert_eq!(rows[2].lane, 0, "the main line stays in lane 0");
        assert_eq!(rows[2].parents, [0]);
        assert_eq!(rows[2].joins, [1], "the feature line ends in b's dot");
        assert_eq!(rows[2].below, [0], "one line continues to the base");
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[0].width(), 2);
    }

    /// Two branch tips (git log --all): the second tip opens its own
    /// lane rather than taking the first's.
    #[test]
    fn unrelated_tips_get_their_own_lanes() {
        let rows = lanes(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].parents, [0], "both point at a, drawn in lane 0");
        assert_eq!(rows[2].lane, 0);
    }
}
