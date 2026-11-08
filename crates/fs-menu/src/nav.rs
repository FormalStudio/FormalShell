//! The launcher's cursor as pure functions over the committed row list:
//! where it lands when the rows change and where one key press moves it.
//!
//! The cursor is a row id, and an index only as a consequence of where that
//! id sits in the rows on screen. Rows change for more reasons than typing,
//! and an index held across such a change points at whatever row slid into
//! its slot.

/// Where the cursor goes when the row list becomes `ids`. `fresh` is a new
/// query, level or open, which always starts on the first row; `placed` is
/// whether a key or the pointer has moved the cursor since. Until one has,
/// the cursor is the list's head rather than any particular row. Once
/// placed, the row it was put on (`want_id`) keeps it wherever that row now
/// sits, and while that row is gone the cursor holds `prev_index`, clamped
/// to the list.
pub fn rederive(want_id: &str, prev_index: i64, ids: &[&str], fresh: bool, placed: bool) -> usize {
    let n = ids.len();
    if n == 0 || fresh || !placed {
        return 0;
    }
    if !want_id.is_empty()
        && let Some(at) = ids.iter().position(|id| *id == want_id)
    {
        return at;
    }
    prev_index.clamp(0, n as i64 - 1) as usize
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    Home,
    End,
    PageDown,
    PageUp,
    Left,
    Right,
    Down,
    Up,
    /// A direction nothing handles: the index stays where it is.
    Other,
}

impl Dir {
    pub fn parse(name: &str) -> Dir {
        match name {
            "home" => Dir::Home,
            "end" => Dir::End,
            "pageDown" => Dir::PageDown,
            "pageUp" => Dir::PageUp,
            "left" => Dir::Left,
            "right" => Dir::Right,
            "down" => Dir::Down,
            "up" => Dir::Up,
            _ => Dir::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub index: usize,
    /// A step travels; a wrap, a page or an end snaps. The cursor fill
    /// animates on this.
    pub travels: bool,
}

const fn snap(index: usize) -> Step {
    Step { index, travels: false }
}

const fn travel(index: usize) -> Step {
    Step { index, travels: true }
}

/// One key's move over a list whose first `cells` entries are a grid of
/// `columns` and whose rest are rows under it: `cells` is the whole list for
/// the picker and emoji grids, 0 for a row list, the app count for the app
/// grid.
///
/// Vertical moves keep the column: off the bottom of the grid onto the top
/// row of the same column, off the top onto the last row that has one, so a
/// short last row never swallows the column. Out of the grid's last row the
/// cursor goes to the first row under it, and up out of that row back to the
/// last cell. Horizontal moves walk the list in reading order and stop at
/// either end.
pub fn step(index: usize, dir: Dir, count: usize, cells: usize, columns: usize, page: usize) -> Step {
    let n = count;
    if n == 0 {
        return snap(0);
    }
    let i = index.min(n - 1);
    let g = cells.min(n);
    let c = if g > 0 { columns.max(1) } else { 1 };
    let tail = g < n;

    match dir {
        Dir::Home => snap(0),
        Dir::End => snap(n - 1),
        Dir::PageDown => snap((i + page.max(1)).min(n - 1)),
        Dir::PageUp => snap(i.saturating_sub(page.max(1))),
        Dir::Left => if i > 0 { travel(i - 1) } else { snap(i) },
        Dir::Right => if i < n - 1 { travel(i + 1) } else { snap(i) },
        Dir::Down => {
            if i < g {
                let last_row_start = (g - 1) / c * c;
                if i < last_row_start {
                    return travel((i + c).min(g - 1));
                }
                if tail {
                    return travel(g);
                }
                return snap(i % c);
            }
            if i + 1 < n {
                return travel(i + 1);
            }
            snap(0)
        }
        Dir::Up => {
            if i < g {
                if i >= c {
                    return travel(i - c);
                }
                if tail {
                    return snap(n - 1);
                }
                return snap(last_in_column(i % c, g, c));
            }
            if i > g {
                return travel(i - 1);
            }
            if g > 0 {
                return travel(g - 1);
            }
            snap(n - 1)
        }
        Dir::Other => snap(i),
    }
}

/// The lowest cell in `column` of a grid of `cells`: the last row's own cell
/// when that row reaches the column, the row above it when it is short.
pub fn last_in_column(column: usize, cells: usize, columns: usize) -> usize {
    let at = (cells - 1) / columns * columns + column;
    if at > cells - 1 { at - columns } else { at }
}
