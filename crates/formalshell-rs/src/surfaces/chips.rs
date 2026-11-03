//! The second bars' chip grid, the tray's (`panel::tray`) and the
//! chevron's (`wayland::Popout`): up to [`COLUMNS`] chips a row, the panel
//! gap between them, each column as wide as its widest chip and no chip
//! narrower than it is tall. A card held to a width (the sidebar's foot)
//! wraps its chips row by row at their own widths instead, each row
//! centred, since one wide chip would otherwise hold every row to one
//! column.

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

/// Rows filled in order while the next chip fits, each centred in `within`.
fn wrap(widths: &[f64], height: f64, gap: f64, within: f64) -> (Vec<Place>, (f64, f64)) {
    let widths: Vec<f64> = widths.iter().map(|w| w.max(height)).collect();
    let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
    let mut run = 0.0;
    for (i, w) in widths.iter().enumerate() {
        let row = rows.last_mut().expect("one row at least");
        let next = if row.is_empty() { *w } else { run + gap + w };
        if row.is_empty() || next <= within {
            row.push(i);
            run = next;
        } else {
            rows.push(vec![i]);
            run = *w;
        }
    }
    let mut at = vec![(0.0, 0.0, 0.0); widths.len()];
    for (r, row) in rows.iter().enumerate() {
        let span = row.iter().map(|&i| widths[i]).sum::<f64>() + gap * (row.len() - 1) as f64;
        let mut x = ((within - span) / 2.0).max(0.0);
        for &i in row {
            at[i] = (x, r as f64 * (height + gap), widths[i]);
            x += widths[i] + gap;
        }
    }
    let n = rows.len() as f64;
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
    fn a_held_width_wraps_row_by_row_with_each_row_centred() {
        // 20 20 | 50 | 20 20 in 50: a wide chip takes a row of its own.
        let (at, size) = grid(&[20.0, 20.0, 50.0, 20.0, 10.0], 20.0, 4.0, Some(50.0));
        assert_eq!(size, (50.0, 68.0));
        assert_eq!(at[0], (3.0, 0.0, 20.0));
        assert_eq!(at[1], (27.0, 0.0, 20.0));
        assert_eq!(at[2], (0.0, 24.0, 50.0));
        assert_eq!(at[4], (27.0, 48.0, 20.0));
        // A chip wider than the room still gets a row.
        assert_eq!(grid(&[60.0], 20.0, 4.0, Some(50.0)).1, (50.0, 20.0));
    }
}
