//! The second bars' chip grid, the tray's (`panel::tray`) and the
//! chevron's (`wayland::Popout`): up to [`COLUMNS`] chips a row, the
//! panel gap between them, each column as wide as its widest chip and no
//! chip narrower than it is tall.

/// Chips to a row before the card wraps.
pub const COLUMNS: usize = 8;

pub fn columns(n: usize) -> usize {
    n.clamp(1, COLUMNS)
}

/// A chip's place on the grid: along its row, across, and its column's width.
pub type Place = (f64, f64, f64);

/// Each chip's place inside the grid, and the grid's size, for chips
/// `widths` wide and `height` tall, `gap` apart.
pub fn grid(widths: &[f64], height: f64, gap: f64) -> (Vec<Place>, (f64, f64)) {
    if widths.is_empty() {
        return (Vec::new(), (0.0, 0.0));
    }
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
        let (at, size) = grid(&widths, 20.0, 4.0);
        assert_eq!(at[0], (0.0, 0.0, 30.0));
        assert_eq!(at[1], (34.0, 0.0, 50.0));
        assert_eq!(at[2], (88.0, 0.0, 20.0));
        assert_eq!(at[8], (0.0, 24.0, 30.0));
        assert_eq!(size, (30.0 + 50.0 + 6.0 * 20.0 + 7.0 * 4.0, 44.0));
    }

    #[test]
    fn one_row_for_a_few() {
        let (at, size) = grid(&[20.0, 20.0, 20.0], 20.0, 4.0);
        assert_eq!(at.iter().map(|a| a.1).collect::<Vec<_>>(), vec![0.0; 3]);
        assert_eq!(size, (68.0, 20.0));
        assert_eq!(grid(&[], 20.0, 4.0).1, (0.0, 0.0));
    }
}
