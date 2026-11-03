//! The second bars' chip grid, the tray's (`panel::tray`) and the
//! chevron's (`wayland::Popout`): up to [`COLUMNS`] chips a row, the panel
//! gap between them, each column as wide as its widest chip and no chip
//! narrower than it is tall. A card held to a width (the sidebar's foot)
//! lays its chips on a grid of equal cells that fills that width, a chip
//! spanning as many cells as its own width needs and taking the first run
//! of free cells, so every edge lines up with the ones above it.

/// Chips to a row before the card wraps.
pub const COLUMNS: usize = 8;

pub fn columns(n: usize) -> usize {
    n.clamp(1, COLUMNS)
}

/// A chip's place on the grid: along its row, across, and its column's width.
pub type Place = (f64, f64, f64);

/// Each chip's place inside the grid, and the grid's size, for chips
/// `widths` wide and `height` tall, `gap` apart; held `within` a width,
/// wrapped to it.
pub fn grid(widths: &[f64], height: f64, gap: f64, within: Option<f64>) -> (Vec<Place>, (f64, f64)) {
    if widths.is_empty() {
        return (Vec::new(), (0.0, 0.0));
    }
    match within {
        Some(within) => wrap(widths, height, gap, within),
        None => columns_of(widths, height, gap),
    }
}

/// Equal cells `height` tall, as many to a row as `within` holds and
/// stretched to fill it; each chip, in order, in the first free run of the
/// cells it spans, rows left open behind a wide chip filled by the next
/// narrow one.
fn wrap(widths: &[f64], height: f64, gap: f64, within: f64) -> (Vec<Place>, (f64, f64)) {
    let cols = (((within + gap) / (height + gap)).floor() as usize).max(1);
    let cell = (within - gap * (cols - 1) as f64) / cols as f64;
    let mut taken: Vec<Vec<bool>> = Vec::new();
    let mut at = Vec::with_capacity(widths.len());
    for w in widths {
        let span = (((w + gap) / (cell + gap)).ceil() as usize).clamp(1, cols);
        let (row, col) = (0..)
            .find_map(|r| {
                if r == taken.len() {
                    taken.push(vec![false; cols]);
                }
                (0..=cols - span).find(|&c| !taken[r][c..c + span].iter().any(|&t| t)).map(|c| (r, c))
            })
            .expect("a fresh row always has room");
        taken[row][col..col + span].fill(true);
        at.push((col as f64 * (cell + gap), row as f64 * (height + gap), span as f64 * cell + (span - 1) as f64 * gap));
    }
    let n = taken.len() as f64;
    (at, (within, n * height + gap * (n - 1.0)))
}

fn columns_of(widths: &[f64], height: f64, gap: f64) -> (Vec<Place>, (f64, f64)) {
    let cols = columns(widths.len());
    let mut col_w = vec![0.0_f64; cols];
    for (i, w) in widths.iter().enumerate() {
        col_w[i % cols] = col_w[i % cols].max(w.max(height));
    }
    let at = (0..widths.len())
        .map(|i| {
            let (row, col) = (i / cols, i % cols);
            let x = col_w[..col].iter().sum::<f64>() + gap * col as f64;
            (x, row as f64 * (height + gap), col_w[col])
        })
        .collect();
    let rows = widths.len().div_ceil(cols);
    let size = (col_w.iter().sum::<f64>() + gap * (cols - 1) as f64, rows as f64 * height + gap * (rows - 1) as f64);
    (at, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_after_eight_with_columns_as_wide_as_their_widest() {
        let mut widths = vec![10.0; 9];
        widths[1] = 50.0;
        widths[8] = 30.0;
        let (at, size) = grid(&widths, 20.0, 4.0, None);
        assert_eq!(at[0], (0.0, 0.0, 30.0));
        assert_eq!(at[1], (34.0, 0.0, 50.0));
        assert_eq!(at[2], (88.0, 0.0, 20.0));
        assert_eq!(at[8], (0.0, 24.0, 30.0));
        assert_eq!(size, (30.0 + 50.0 + 6.0 * 20.0 + 7.0 * 4.0, 44.0));
    }

    #[test]
    fn one_row_for_a_few() {
        let (at, size) = grid(&[20.0, 20.0, 20.0], 20.0, 4.0, None);
        assert_eq!(at.iter().map(|a| a.1).collect::<Vec<_>>(), vec![0.0; 3]);
        assert_eq!(size, (68.0, 20.0));
        assert_eq!(grid(&[], 20.0, 4.0, None).1, (0.0, 0.0));
    }

    #[test]
    fn a_held_width_lays_equal_cells_and_fills_the_gaps_a_wide_chip_leaves() {
        // 68 holds three 20 cells; 40 spans two, so the second 40 fits only
        // under the first and the 10 fills the cell the first left on its row.
        let (at, size) = grid(&[20.0, 20.0, 20.0, 40.0, 40.0, 10.0], 20.0, 4.0, Some(68.0));
        assert_eq!(at[0], (0.0, 0.0, 20.0));
        assert_eq!(at[2], (48.0, 0.0, 20.0));
        assert_eq!(at[3], (0.0, 24.0, 44.0));
        assert_eq!(at[4], (0.0, 48.0, 44.0));
        assert_eq!(at[5], (48.0, 24.0, 20.0));
        assert_eq!(size, (68.0, 68.0));
        // The cells stretch to the width, and a chip wider than it spans the row.
        assert_eq!(grid(&[10.0], 20.0, 4.0, Some(50.0)).0[0], (0.0, 0.0, 23.0));
        assert_eq!(grid(&[90.0], 20.0, 4.0, Some(50.0)), (vec![(0.0, 0.0, 50.0)], (50.0, 20.0)));
    }
}
