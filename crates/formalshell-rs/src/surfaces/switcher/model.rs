//! The switcher's list, its cursor and its grid, pure.

use fs_types::{IRect, Rect, Window, WindowId, WorkspaceId};

/// Gala's order: the most recently focused first, then the compositor's
/// own order for windows nothing has focused this session. Special
/// workspaces are dropped, and with `workspace` set, everything off it.
pub fn entries<'a>(windows: &'a [Window], history: &[WindowId], workspace: &WorkspaceId) -> Vec<&'a Window> {
    let offered: Vec<&Window> = windows
        .iter()
        .filter(|w| !w.id.is_empty() && !w.special)
        .filter(|w| workspace.is_empty() || w.workspace_id == *workspace)
        .collect();
    let mut out: Vec<&Window> = Vec::with_capacity(offered.len());
    for id in history {
        if let Some(w) = offered.iter().find(|w| w.id == *id)
            && !out.iter().any(|o| o.id == w.id)
        {
            out.push(w);
        }
    }
    for w in &offered {
        if !out.iter().any(|o| o.id == w.id) {
            out.push(w);
        }
    }
    out
}

/// The cursor after a step, wrapping both ways; 0 on an empty list.
pub fn advance(index: usize, count: usize, step: i32) -> usize {
    if count == 0 {
        return 0;
    }
    (index as i64 + step as i64).rem_euclid(count as i64) as usize
}

/// A thumbnail's width at `height` off its window's aspect (16:9 with no
/// rect), held between `min` and `max`.
pub fn thumb_width(rect: Option<&IRect>, height: f64, min: f64, max: f64) -> f64 {
    let ratio = match rect {
        Some(r) if !r.is_empty() => f64::from(r.w) / f64::from(r.h),
        _ => 16.0 / 9.0,
    };
    (height * ratio).round().min(max).max(min)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub first: usize,
    pub count: usize,
    pub width: f64,
    pub widths: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub width: f64,
    pub gap: f64,
}

fn pack(widths: &[f64], gap: f64, limit: f64) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut first = 0;
    let mut used = 0.0;
    for (i, w) in widths.iter().enumerate() {
        let mut next = if i == first { *w } else { used + gap + w };
        if i > first && next > limit {
            rows.push(Row { first, count: i - first, width: used, widths: widths[first..i].to_vec() });
            first = i;
            next = *w;
        }
        used = next;
    }
    if !widths.is_empty() {
        rows.push(Row { first, count: widths.len() - first, width: used, widths: widths[first..].to_vec() });
    }
    rows
}

/// The cells wrapped into rows no wider than `max`, the limit pulled in as
/// far as the greedy row count allows so the rows come out even.
pub fn layout(widths: &[f64], gap: f64, max: f64) -> Grid {
    if widths.is_empty() {
        return Grid { rows: Vec::new(), width: 0.0, gap };
    }
    let widest = widths.iter().copied().fold(f64::MIN, f64::max);
    let limit = widest.max(max.floor());
    let count = pack(widths, gap, limit).len();
    let (mut low, mut high) = (widest, limit);
    while low < high {
        let mid = ((low + high) / 2.0).floor();
        if pack(widths, gap, mid).len() <= count {
            high = mid;
        } else {
            low = mid + 1.0;
        }
    }
    let rows = pack(widths, gap, high);
    let width = rows.iter().map(|r| r.width).fold(0.0, f64::max);
    Grid { rows, width, gap }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    pub rect: Rect,
    pub row: usize,
}

/// Each cell from the grid's top left, rows centred on the widest.
pub fn cells(grid: &Grid, cell_height: f64) -> Vec<Place> {
    let mut out = Vec::new();
    for (r, row) in grid.rows.iter().enumerate() {
        let mut x = (grid.width - row.width) / 2.0;
        for w in &row.widths {
            out.push(Place { rect: Rect::new(x, r as f64 * cell_height, *w, cell_height), row: r });
            x += w + grid.gap;
        }
    }
    out
}

/// Which of its app's windows each tile is, `(n, of)`; an empty key is
/// never grouped.
pub fn ordinals(keys: &[String]) -> Vec<(usize, usize)> {
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            if k.is_empty() {
                return (1, 1);
            }
            let n = keys[..=i].iter().filter(|o| *o == k).count();
            (n, keys.iter().filter(|o| *o == k).count())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, ws: &str) -> Window {
        Window { id: id.into(), workspace_id: ws.into(), ..Window::default() }
    }

    #[test]
    fn history_first_then_compositor_order_and_no_special() {
        let special = Window { special: true, ..win("c", "-98") };
        let ws = [win("a", "1"), win("b", "1"), special, win("d", "2")];
        let ids = |v: Vec<&Window>| v.iter().map(|w| w.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(entries(&ws, &["b".into(), "x".into()], &"".into())), ["b", "a", "d"]);
        assert_eq!(ids(entries(&ws, &["d".into()], &"1".into())), ["a", "b"]);
    }

    #[test]
    fn advance_wraps() {
        assert_eq!(advance(0, 3, -1), 2);
        assert_eq!(advance(2, 3, 1), 0);
        assert_eq!(advance(0, 0, 1), 0);
    }

    #[test]
    fn layout_evens_rows() {
        let g = layout(&[100.0; 11], 0.0, 600.0);
        assert_eq!(g.rows.iter().map(|r| r.count).collect::<Vec<_>>(), [6, 5]);
        let c = cells(&g, 50.0);
        assert_eq!(c[6].rect.x, 50.0);
        assert_eq!(c[6].rect.y, 50.0);
    }

    #[test]
    fn ordinals_count_per_app() {
        let keys: Vec<String> = ["a", "b", "a", ""].iter().map(|s| s.to_string()).collect();
        assert_eq!(ordinals(&keys), [(1, 2), (1, 1), (2, 2), (1, 1)]);
    }
}
