//! The card's layout as numbers: items in order flowing into two columns,
//! a full-width item taking a row of its own, and an open menu laid under
//! one row, moving every row after it down.

use vello_cpu::kurbo::Rect;

/// GNOME's `N_QUICK_SETTINGS_COLUMNS`.
pub const COLUMNS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    /// Columns taken, 1 or [`COLUMNS`].
    pub span: usize,
    pub height: f64,
}

/// A menu open under row `under`, `height` tall as drawn now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Menu {
    pub under: usize,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Each item's box, in the order given, from the grid's top left.
    pub rects: Vec<Rect>,
    /// Each item's row.
    pub rows: Vec<usize>,
    pub menu: Option<Rect>,
}

/// Lays `items` out `width` wide with `gap` between rows and columns. A
/// row is as tall as its tallest item. A menu of no height takes no room.
pub fn place(items: &[Item], width: f64, gap: f64, menu: Option<Menu>) -> Placed {
    let column = (width - gap * (COLUMNS - 1) as f64) / COLUMNS as f64;
    let mut rows = Vec::with_capacity(items.len());
    let mut heights: Vec<f64> = Vec::new();
    let mut cols = Vec::with_capacity(items.len());
    let mut at = COLUMNS;
    for item in items {
        let span = item.span.clamp(1, COLUMNS);
        if at + span > COLUMNS {
            heights.push(0.0);
            at = 0;
        }
        let row = heights.len() - 1;
        heights[row] = heights[row].max(item.height);
        rows.push(row);
        cols.push((at, span));
        at += span;
    }
    let menu = menu.filter(|m| m.height > 0.0 && m.under < heights.len());
    let mut tops = Vec::with_capacity(heights.len());
    let mut y = 0.0;
    let mut menu_rect = None;
    for (r, h) in heights.iter().enumerate() {
        if r > 0 {
            y += gap;
        }
        tops.push(y);
        y += h;
        if let Some(m) = menu.filter(|m| m.under == r) {
            y += gap;
            menu_rect = Some(Rect::new(0.0, y, width, y + m.height));
            y += m.height;
        }
    }
    let rects = rows
        .iter()
        .zip(&cols)
        .zip(items)
        .map(|((&r, &(col, span)), item)| {
            let x = col as f64 * (column + gap);
            let w = column * span as f64 + gap * (span - 1) as f64;
            Rect::new(x, tops[r], x + w, tops[r] + item.height)
        })
        .collect();
    Placed { rects, rows, menu: menu_rect }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: f64 = 400.0;
    const INSET: f64 = 16.0;
    const GAP: f64 = 12.0;

    fn tiles(n: usize) -> Vec<Item> {
        vec![Item { span: 1, height: 48.0 }; n]
    }

    #[test]
    fn two_tiles_share_a_row_178_wide_in_a_400_card() {
        let p = place(&tiles(3), CARD - INSET * 2.0, GAP, None);
        assert_eq!(p.rects[0], Rect::new(0.0, 0.0, 178.0, 48.0));
        assert_eq!(p.rects[1], Rect::new(190.0, 0.0, 368.0, 48.0));
        assert_eq!(p.rects[2], Rect::new(0.0, 60.0, 178.0, 108.0));
        assert_eq!(p.rows, vec![0, 0, 1]);
    }

    #[test]
    fn a_span_2_item_takes_a_row_of_its_own() {
        let mut items = tiles(1);
        items.push(Item { span: 2, height: 40.0 });
        items.push(Item { span: 1, height: 48.0 });
        let p = place(&items, 368.0, GAP, None);
        assert_eq!(p.rows, vec![0, 1, 2]);
        assert_eq!(p.rects[1], Rect::new(0.0, 60.0, 368.0, 100.0));
        assert_eq!(p.rects[2], Rect::new(0.0, 112.0, 178.0, 160.0));
    }

    #[test]
    fn a_menu_moves_the_rows_after_its_own_by_its_height_and_a_gap() {
        let h = 150.0;
        let shut = place(&tiles(6), 368.0, GAP, None);
        let open = place(&tiles(6), 368.0, GAP, Some(Menu { under: 1, height: h }));
        assert_eq!(open.menu, Some(Rect::new(0.0, 108.0 + GAP, 368.0, 108.0 + GAP + h)));
        for (i, (a, b)) in shut.rects.iter().zip(&open.rects).enumerate() {
            let moved = if shut.rows[i] > 1 { h + GAP } else { 0.0 };
            assert_eq!(b.y0 - a.y0, moved, "item {i}");
        }
    }

    #[test]
    fn a_menu_of_no_height_or_past_the_last_row_takes_no_room() {
        let shut = place(&tiles(4), 368.0, GAP, None);
        assert_eq!(place(&tiles(4), 368.0, GAP, Some(Menu { under: 0, height: 0.0 })), shut);
        assert_eq!(place(&tiles(4), 368.0, GAP, Some(Menu { under: 5, height: 80.0 })), shut);
    }
}
