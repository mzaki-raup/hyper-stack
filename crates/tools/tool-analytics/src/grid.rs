//! Where a dashboard's tiles go (docs/tools/analytics.md §4.5): Metabase's
//! 24-column grid at the pane's width, or one column when the pane is
//! narrow, as Metabase stacks a dashboard on a phone. Pure arithmetic: the
//! screens ask once per width and draw from the answer.

/// Columns of Metabase's grid.
pub const COLUMNS: u32 = 24;
/// The height of one grid row, in points.
pub const ROW: f32 = 56.0;
/// Space between tiles.
pub const GAP: f32 = 8.0;
/// Below this width a dashboard is one column.
pub const STACK_BELOW: f32 = 720.0;
/// The shortest a stacked tile is drawn.
pub const STACKED_MIN: f32 = 150.0;

/// A tile's grid cell, as the server gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
    pub size_x: u32,
    pub size_y: u32,
}

/// Where one tile is drawn, relative to the top left of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// The tile's index in the list handed to [`place`].
    pub index: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Every tile's place at `width`, and the height of the whole grid.
pub fn place(cells: &[Cell], width: f32) -> (Vec<Place>, f32) {
    if cells.is_empty() || width <= 0.0 {
        return (Vec::new(), 0.0);
    }
    if width < STACK_BELOW {
        return stacked(cells, width);
    }
    let col_w = width / COLUMNS as f32;
    let mut out = Vec::with_capacity(cells.len());
    let mut bottom = 0.0_f32;
    for (index, c) in cells.iter().enumerate() {
        let col = c.col.min(COLUMNS - 1);
        let span = c.size_x.clamp(1, COLUMNS - col);
        let p = Place {
            index,
            x: col as f32 * col_w,
            y: c.row as f32 * ROW,
            w: span as f32 * col_w - GAP,
            h: c.size_y.max(1) as f32 * ROW - GAP,
        };
        bottom = bottom.max(p.y + p.h);
        out.push(p);
    }
    (out, bottom)
}

/// One column, tiles in reading order (row, then column).
fn stacked(cells: &[Cell], width: f32) -> (Vec<Place>, f32) {
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by_key(|&i| (cells[i].row, cells[i].col));
    let mut y = 0.0_f32;
    let mut out = Vec::with_capacity(cells.len());
    for index in order {
        let h = (cells[index].size_y.max(1) as f32 * ROW - GAP).max(STACKED_MIN);
        out.push(Place {
            index,
            x: 0.0,
            y,
            w: width,
            h,
        });
        y += h + GAP;
    }
    (out, (y - GAP).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(row: u32, col: u32, size_x: u32, size_y: u32) -> Cell {
        Cell {
            row,
            col,
            size_x,
            size_y,
        }
    }

    #[test]
    fn at_a_wide_pane_a_tile_sits_at_its_column_and_row() {
        // 1200 wide: a column is 50.
        let cells = [cell(0, 0, 6, 3), cell(0, 6, 18, 6), cell(6, 12, 12, 6)];
        let (p, height) = place(&cells, 1200.0);
        assert_eq!((p[0].x, p[0].y), (0.0, 0.0));
        assert_eq!(p[0].w, 6.0 * 50.0 - GAP);
        assert_eq!(p[0].h, 3.0 * ROW - GAP);
        assert_eq!(p[1].x, 6.0 * 50.0);
        assert_eq!(p[1].w, 18.0 * 50.0 - GAP);
        assert_eq!((p[2].x, p[2].y), (600.0, 6.0 * ROW));
        assert_eq!(
            height,
            6.0 * ROW + 6.0 * ROW - GAP,
            "to the bottom of the lowest tile"
        );
        // The tiles do not overlap: the first row ends where the second begins.
        assert!(p[0].x + p[0].w <= p[1].x);
    }

    #[test]
    fn the_grid_scales_with_the_width() {
        let cells = [cell(0, 12, 12, 2)];
        let (narrow, _) = place(&cells, 960.0);
        let (wide, _) = place(&cells, 1920.0);
        assert_eq!(narrow[0].x, 480.0);
        assert_eq!(wide[0].x, 960.0);
        assert!(wide[0].w > narrow[0].w);
        assert_eq!(wide[0].y, narrow[0].y, "rows are a fixed height");
    }

    #[test]
    fn a_narrow_pane_stacks_the_tiles_in_reading_order() {
        // Handed out of order: the second row's tile first.
        let cells = [cell(6, 0, 12, 4), cell(0, 12, 12, 2), cell(0, 0, 12, 2)];
        let (p, height) = place(&cells, 500.0);
        let order: Vec<usize> = p.iter().map(|x| x.index).collect();
        assert_eq!(order, [2, 1, 0], "row, then column");
        assert!(p.iter().all(|x| x.x == 0.0 && x.w == 500.0));
        assert!(p[1].y > p[0].y && p[2].y > p[1].y);
        assert_eq!(p[0].h, STACKED_MIN, "a short tile is raised to the minimum");
        let last = p.last().expect("a tile");
        assert_eq!(height, last.y + last.h);
    }

    #[test]
    fn a_tile_past_the_edge_is_kept_inside_the_grid() {
        let (p, _) = place(&[cell(0, 20, 12, 2)], 1200.0);
        assert!(p[0].x + p[0].w + GAP <= 1200.0 + 0.001);
        let (p, _) = place(&[cell(0, 99, 3, 2)], 1200.0);
        assert!(p[0].x < 1200.0);
    }

    #[test]
    fn nothing_to_place_is_an_empty_grid() {
        assert_eq!(place(&[], 1200.0), (Vec::new(), 0.0));
        assert_eq!(place(&[cell(0, 0, 4, 2)], 0.0), (Vec::new(), 0.0));
    }
}
