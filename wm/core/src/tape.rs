//! The tape: one workspace's columns of stacked windows, scrolled sideways.
//!
//! Widths are fractions of the strip, the usable area inset so that a window's
//! own gap on each side adds up to the outer gap at the output's edge. All
//! geometry here is in strip coordinates with the scroll left out; the scene
//! subtracts the scroll it is animating.

use crate::geom::Rect;

pub type WinId = String;

pub const MIN_WIDTH: f64 = 0.1;
pub const MAX_WIDTH: f64 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Prev,
    Next,
}

impl Side {
    fn step(self) -> isize {
        match self {
            Side::Prev => -1,
            Side::Next => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Big {
    /// To the edges of what the layers leave free.
    Maximized,
    /// The whole output, no chrome.
    Fullscreen,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub width: f64,
    pub windows: Vec<WinId>,
    /// The focused row.
    pub row: usize,
    /// Hyprland's scrolling layout keeps a maximized or fullscreen tile in
    /// the tape: its column, alone, widens to the view (or the output) and
    /// scrolls with the rest, and `restore` is the width it goes back to.
    pub big: Option<Big>,
    pub restore: f64,
}

impl Column {
    fn new(id: WinId, width: f64) -> Self {
        Self {
            width,
            windows: vec![id],
            row: 0,
            big: None,
            restore: width,
        }
    }

    fn unbig(&mut self) {
        if self.big.take().is_some() {
            self.width = self.restore;
        }
    }
}

/// One tiled window's place: the column and row it sits in and its rect in
/// strip coordinates, scroll left out.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: WinId,
    pub column: usize,
    pub row: usize,
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tape {
    pub columns: Vec<Column>,
    pub focus: usize,
    pub scroll: f64,
    /// The tile whose border is being dragged, until it is let go or leaves
    /// the tape. Meanwhile the view stays where the drag put it, before the
    /// start of the tape included, and a lone column is not recentred:
    /// Hyprland's camera does not move while a tile of its tape is dragged.
    pub held: Option<WinId>,
}

impl Tape {
    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }

    pub fn windows(&self) -> impl Iterator<Item = &WinId> {
        self.columns.iter().flat_map(|column| column.windows.iter())
    }

    pub fn find(&self, id: &str) -> Option<(usize, usize)> {
        self.columns.iter().enumerate().find_map(|(c, column)| {
            column
                .windows
                .iter()
                .position(|window| window == id)
                .map(|r| (c, r))
        })
    }

    pub fn focused(&self) -> Option<&WinId> {
        let column = self.columns.get(self.focus)?;
        column.windows.get(column.row)
    }

    /// A new column right of the focused one, focused.
    pub fn insert(&mut self, id: WinId, width: f64) {
        let at = if self.columns.is_empty() {
            0
        } else {
            self.focus + 1
        };
        self.insert_at(at, id, width);
    }

    /// A new column at `index` (clamped), focused.
    pub fn insert_at(&mut self, index: usize, id: WinId, width: f64) {
        let index = index.min(self.columns.len());
        self.columns
            .insert(index, Column::new(id, clamp_width(width)));
        self.focus = index;
    }

    /// Into the column at `column` as row `row` (both clamped), focused.
    pub fn insert_into(&mut self, column: usize, row: usize, id: WinId) {
        let Some(target) = self.columns.get_mut(column) else {
            self.insert_at(column, id, 0.5);
            return;
        };
        let row = row.min(target.windows.len());
        target.windows.insert(row, id);
        target.row = row;
        target.unbig();
        self.focus = column;
    }

    /// Takes the window out. The column right of an emptied column slides
    /// into its place and takes the focus it had.
    pub fn remove(&mut self, id: &str) -> Option<f64> {
        let (c, r) = self.find(id)?;
        if self.held.as_deref() == Some(id) {
            self.held = None;
        }
        let column = &mut self.columns[c];
        let width = column.width;
        column.windows.remove(r);
        if column.windows.is_empty() {
            self.columns.remove(c);
            if self.focus > c || self.focus >= self.columns.len() {
                self.focus = self.focus.saturating_sub(1);
            }
        } else {
            if column.row > r || column.row >= column.windows.len() {
                column.row = column.row.saturating_sub(1);
            }
        }
        Some(width)
    }

    pub fn focus_window(&mut self, id: &str) -> bool {
        let Some((c, r)) = self.find(id) else {
            return false;
        };
        self.focus = c;
        self.columns[c].row = r;
        true
    }

    /// Walks columns and wraps at the ends of the tape. In the column it
    /// lands on, Hyprland's `findBestNeighbor`: the row last focused there
    /// if it lies beside the window left, else the topmost that does.
    pub fn focus_column(&mut self, side: Side) -> bool {
        let count = self.columns.len() as isize;
        if count < 2 {
            return false;
        }
        let from = &self.columns[self.focus];
        let (row, rows) = (from.row, from.windows.len());
        self.focus = (self.focus as isize + side.step()).rem_euclid(count) as usize;
        let to = &mut self.columns[self.focus];
        let len = to.windows.len();
        // Rows split a column evenly: row j of m spans j/m to (j+1)/m.
        let beside = |j: usize| j * rows < (row + 1) * len && (j + 1) * rows > row * len;
        if !beside(to.row) {
            to.row = (0..len).find(|j| beside(*j)).unwrap_or(0);
        }
        true
    }

    pub fn focus_row(&mut self, side: Side) -> bool {
        let Some(column) = self.columns.get_mut(self.focus) else {
            return false;
        };
        let next = column.row as isize + side.step();
        if next < 0 || next >= column.windows.len() as isize {
            return false;
        }
        column.row = next as usize;
        true
    }

    /// Hyprland's `swapcol` under its default `wrap_swapcol`: at an end of
    /// the tape the column trades places with the one at the other end.
    pub fn swap_column(&mut self, side: Side) -> bool {
        let count = self.columns.len() as isize;
        if count < 2 {
            return false;
        }
        let next = (self.focus as isize + side.step()).rem_euclid(count) as usize;
        self.columns.swap(self.focus, next);
        self.focus = next;
        true
    }

    pub fn swap_row(&mut self, side: Side) -> bool {
        let Some(column) = self.columns.get_mut(self.focus) else {
            return false;
        };
        let next = column.row as isize + side.step();
        if next < 0 || next >= column.windows.len() as isize {
            return false;
        }
        column.windows.swap(column.row, next as usize);
        column.row = next as usize;
        true
    }

    /// Hyprland's `consume_or_expel`: a window alone in its column joins
    /// the neighbouring column, a stacked one leaves into a new column of
    /// the default `width` on that side.
    pub fn consume_or_expel(&mut self, side: Side, width: f64) -> bool {
        let Some(column) = self.columns.get(self.focus) else {
            return false;
        };
        if column.windows.len() > 1 {
            let row = column.row;
            let id = self.columns[self.focus].windows.remove(row);
            let source = &mut self.columns[self.focus];
            source.row = source.row.min(source.windows.len() - 1);
            let at = match side {
                Side::Prev => self.focus,
                Side::Next => self.focus + 1,
            };
            self.columns.insert(at, Column::new(id, width));
            self.focus = at;
            return true;
        }
        let neighbour = self.focus as isize + side.step();
        if neighbour < 0 || neighbour >= self.columns.len() as isize {
            return false;
        }
        let id = self.columns.remove(self.focus).windows.remove(0);
        let target = if neighbour as usize > self.focus {
            neighbour as usize - 1
        } else {
            neighbour as usize
        };
        let column = &mut self.columns[target];
        column.windows.push(id);
        column.row = column.windows.len() - 1;
        column.unbig();
        self.focus = target;
        true
    }

    /// Moves the column holding `id` to sit before the column holding
    /// `before`, or to the end of the tape.
    pub fn move_column(&mut self, id: &str, before: Option<&str>) -> bool {
        let Some((from, _)) = self.find(id) else {
            return false;
        };
        let focused = self.focused().cloned();
        let column = self.columns.remove(from);
        let to = before
            .and_then(|before| self.find(before))
            .map_or(self.columns.len(), |(c, _)| c);
        self.columns.insert(to, column);
        if let Some(focused) = focused {
            self.focus_window(&focused);
        }
        true
    }

    /// Swaps two windows' slots; the focus stays on the slot it was on.
    /// Trades two windows' places. Maximized and fullscreen belong to the
    /// window, as the client was told: each goes with its window, `wide`
    /// sizing a fullscreen one as `set_big` does.
    pub fn swap_windows(&mut self, a: &str, b: &str, wide: f64) -> bool {
        let (Some((ac, ar)), Some((bc, br))) = (self.find(a), self.find(b)) else {
            return false;
        };
        let focused = self.focused().cloned();
        let (big_a, big_b) = (self.columns[ac].big, self.columns[bc].big);
        self.columns[ac].unbig();
        self.columns[bc].unbig();
        let id_a = self.columns[ac].windows[ar].clone();
        let id_b = std::mem::replace(&mut self.columns[bc].windows[br], id_a);
        self.columns[ac].windows[ar] = id_b;
        for (id, big) in [(a, big_a), (b, big_b)] {
            if big.is_some() {
                self.set_big(id, big, wide);
            }
        }
        if let Some(id) = focused {
            self.focus_window(&id);
        }
        true
    }

    pub fn replace(&mut self, old: &str, new: WinId) -> bool {
        let Some((c, r)) = self.find(old) else {
            return false;
        };
        self.columns[c].windows[r] = new;
        true
    }

    pub fn set_width(&mut self, width: f64) -> bool {
        let Some(column) = self.columns.get_mut(self.focus) else {
            return false;
        };
        let width = clamp_width(width);
        let changed = column.big.is_some() || (column.width - width).abs() > f64::EPSILON;
        column.big = None;
        column.width = width;
        changed
    }

    /// The window's own column made big, or back to the width it had. A
    /// stacked window first leaves into a column of its own right of its
    /// stack, as Hyprland's `promote` does. `wide` is the fullscreen
    /// column's width, the output's over the strip's, so the next column
    /// starts past the output's edge.
    pub fn set_big(&mut self, id: &str, big: Option<Big>, wide: f64) -> bool {
        let Some((mut c, r)) = self.find(id) else {
            return false;
        };
        let Some(kind) = big else {
            let column = &mut self.columns[c];
            let was = column.big.is_some();
            column.unbig();
            return was;
        };
        if self.columns[c].big == Some(kind) {
            return false;
        }
        let restore = match self.columns[c].big {
            Some(_) => self.columns[c].restore,
            None => self.columns[c].width,
        };
        if self.columns[c].windows.len() > 1 {
            let source = &mut self.columns[c];
            let id = source.windows.remove(r);
            source.row = source.row.min(source.windows.len() - 1);
            c += 1;
            self.columns.insert(c, Column::new(id, restore));
        }
        let column = &mut self.columns[c];
        column.restore = restore;
        column.big = Some(kind);
        column.width = match kind {
            Big::Maximized => 1.0,
            Big::Fullscreen => wide.max(1.0),
        };
        column.row = 0;
        self.focus = c;
        true
    }

    /// A fullscreen column's width again for a strip that changed, `wide`
    /// being the output's width in strips; whether the focus is on a big
    /// column, which then wants fitting again.
    pub fn rewiden(&mut self, wide: f64) -> bool {
        for column in &mut self.columns {
            if column.big == Some(Big::Fullscreen) {
                column.width = wide.max(1.0);
            }
        }
        self.columns
            .get(self.focus)
            .is_some_and(|c| c.big.is_some())
    }

    pub fn big_of(&self, id: &str) -> Option<Big> {
        self.find(id).and_then(|(c, _)| self.columns[c].big)
    }

    /// `id`'s column dragged by an edge to `width`, as Hyprland's scrolling
    /// layout resizes a tile: by the right edge the left edge stays, by the
    /// left edge the view follows so the right edge stays, and neither edge
    /// leaves the view.
    pub fn resize_edge(&mut self, id: &str, left: bool, width: f64, strip_w: f64) -> bool {
        let Some((c, _)) = self.find(id) else {
            return false;
        };
        if !self.held.is_some() {
            // The centring of a lone column becomes the view's, so the drag
            // can keep its edges where they are.
            if self.columns.len() == 1 {
                self.scroll -= self.column_left(c, strip_w);
            }
            self.held = Some(id.to_owned());
        }
        let old = self.columns[c].width;
        let start = self.column_left(c, strip_w) - self.scroll;
        let end = start + old * strip_w;
        let room = if left { start } else { strip_w - end };
        let delta = (clamp_width(width) - old).min(room / strip_w);
        if delta.abs() < f64::EPSILON {
            return false;
        }
        let column = &mut self.columns[c];
        column.big = None;
        column.width = old + delta;
        if left {
            self.scroll += delta * strip_w;
        }
        true
    }

    /// Lets a border drag go: the view is clamped and a lone column centred
    /// again.
    pub fn release(&mut self, strip_w: f64) {
        if self.held.take().is_some() {
            self.clamp_scroll(strip_w);
        }
    }

    pub fn resize(&mut self, delta: f64) -> bool {
        let Some(column) = self.columns.get(self.focus) else {
            return false;
        };
        self.set_width(column.width + delta)
    }

    /// The next preset wider than the column, or the narrowest after the
    /// widest.
    pub fn cycle_width(&mut self, presets: &[f64]) -> bool {
        let Some(column) = self.columns.get(self.focus) else {
            return false;
        };
        let next = presets
            .iter()
            .copied()
            .filter(|preset| *preset > column.width + 0.005)
            .fold(None, |best: Option<f64>, preset| {
                Some(best.map_or(preset, |best| best.min(preset)))
            })
            .or_else(|| presets.iter().copied().reduce(f64::min));
        next.is_some_and(|width| self.set_width(width))
    }

    /// A column's left edge. A lone column sits centred at the width it has.
    pub fn column_left(&self, index: usize, strip_w: f64) -> f64 {
        if self.columns.len() == 1 && !self.held.is_some() {
            return ((1.0 - self.columns[0].width) * strip_w / 2.0).max(0.0);
        }
        self.columns[..index.min(self.columns.len())]
            .iter()
            .map(|column| column.width * strip_w)
            .sum()
    }

    pub fn total_width(&self, strip_w: f64) -> f64 {
        self.columns
            .iter()
            .map(|column| column.width * strip_w)
            .sum()
    }

    pub fn max_scroll(&self, strip_w: f64) -> f64 {
        if self.columns.len() < 2 {
            return 0.0;
        }
        (self.total_width(strip_w) - strip_w).max(0.0)
    }

    pub fn clamp_scroll(&mut self, strip_w: f64) {
        if self.held.is_some() {
            return;
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(strip_w));
    }

    /// Scrolls the least that brings the focused column fully into view.
    pub fn fit(&mut self, strip_w: f64) {
        if let Some(column) = self.columns.get(self.focus) {
            let left = self.column_left(self.focus, strip_w);
            let right = left + column.width * strip_w;
            if left < self.scroll {
                self.scroll = left;
            } else if right > self.scroll + strip_w {
                // A column wider than the view shows from its left edge.
                self.scroll = (right - strip_w).min(left);
            }
        }
        self.clamp_scroll(strip_w);
    }

    /// Where a scroll released at `projected` comes to rest: the nearest
    /// offset that has a column's edge on an edge of the view.
    pub fn snap(&self, projected: f64, strip_w: f64) -> f64 {
        let max = self.max_scroll(strip_w);
        let mut best = projected.clamp(0.0, max);
        let mut distance = f64::INFINITY;
        for (index, column) in self.columns.iter().enumerate() {
            let left = self.column_left(index, strip_w);
            for candidate in [left, left + column.width * strip_w - strip_w] {
                let candidate = candidate.clamp(0.0, max);
                let d = (candidate - projected).abs();
                if d < distance {
                    distance = d;
                    best = candidate;
                }
            }
        }
        best
    }

    /// After the tape was scrolled by hand: keeps the focus when its column
    /// is still wholly in view, else gives it to the nearest column that is.
    pub fn focus_in_view(&mut self, strip_w: f64) -> bool {
        let in_view = |tape: &Tape, index: usize| {
            let left = tape.column_left(index, strip_w);
            let right = left + tape.columns[index].width * strip_w;
            left >= tape.scroll - 0.5 && right <= tape.scroll + strip_w + 0.5
        };
        if self.columns.is_empty() || in_view(self, self.focus) {
            return false;
        }
        let nearest = (0..self.columns.len())
            .filter(|index| in_view(self, *index))
            .min_by_key(|index| index.abs_diff(self.focus));
        match nearest {
            Some(index) => {
                self.focus = index;
                true
            }
            None => false,
        }
    }

    /// The gap a column dropped at strip x `x` (scroll included by the
    /// caller) would open: the index it would take.
    pub fn gap_at(&self, x: f64, strip_w: f64) -> usize {
        self.columns
            .iter()
            .enumerate()
            .take_while(|(index, column)| {
                x > self.column_left(*index, strip_w) + column.width * strip_w / 2.0
            })
            .count()
    }

    pub fn cells(&self, strip: Rect, gap: f64) -> Vec<Cell> {
        let mut cells = Vec::new();
        for (c, column) in self.columns.iter().enumerate() {
            let left = self.column_left(c, strip.w);
            let width = column.width * strip.w;
            let height = strip.h / column.windows.len() as f64;
            for (r, id) in column.windows.iter().enumerate() {
                let cell = Rect::new(left, r as f64 * height, width, height);
                cells.push(Cell {
                    id: id.clone(),
                    column: c,
                    row: r,
                    rect: cell.snapped().inset(gap),
                });
            }
        }
        cells
    }
}

fn clamp_width(width: f64) -> f64 {
    if width.is_finite() {
        width.clamp(MIN_WIDTH, MAX_WIDTH)
    } else {
        0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: f64 = 1000.0;

    fn tape(ids: &[&str]) -> Tape {
        let mut tape = Tape::default();
        for id in ids {
            tape.insert((*id).to_owned(), 0.5);
        }
        tape
    }

    /// Columns left to right, a column's rows joined top to bottom.
    fn order(tape: &Tape) -> Vec<String> {
        tape.columns
            .iter()
            .map(|column| column.windows.join("+"))
            .collect()
    }

    #[test]
    fn a_new_window_opens_right_of_the_focused_column() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.focus_window("a");
        tape.insert("d".into(), 0.5);
        assert_eq!(order(&tape), ["a", "d", "b", "c"]);
        assert_eq!(tape.focused().unwrap(), "d");
    }

    #[test]
    fn a_lone_column_keeps_its_width_and_is_centred() {
        let tape = tape(&["a"]);
        let cells = tape.cells(Rect::new(0.0, 0.0, W, 600.0), 4.0);
        assert_eq!(cells[0].rect, Rect::new(254.0, 4.0, 492.0, 592.0));
        assert_eq!(tape.max_scroll(W), 0.0);
    }

    #[test]
    fn two_half_columns_fill_the_strip_with_one_gap_between() {
        let tape = tape(&["a", "b"]);
        let cells = tape.cells(Rect::new(0.0, 0.0, W, 600.0), 4.0);
        assert_eq!(cells[0].rect, Rect::new(4.0, 4.0, 492.0, 592.0));
        assert_eq!(cells[1].rect, Rect::new(504.0, 4.0, 492.0, 592.0));
        assert_eq!(cells[1].rect.x - cells[0].rect.right(), 8.0);
    }

    #[test]
    fn stacked_windows_split_the_column_evenly() {
        let mut tape = tape(&["a", "b"]);
        assert!(tape.consume_or_expel(Side::Prev, 0.5));
        assert_eq!(order(&tape), ["a+b"]);
        let cells = tape.cells(Rect::new(0.0, 0.0, W, 600.0), 4.0);
        assert_eq!(cells[0].rect.h, 292.0);
        assert_eq!(cells[1].rect.y, 304.0);
        assert_eq!((cells[1].column, cells[1].row), (0, 1));
    }

    #[test]
    fn focus_fits_by_the_least_scroll_and_does_not_recentre() {
        let mut tape = tape(&["a", "b", "c", "d"]);
        tape.focus_window("a");
        tape.fit(W);
        assert_eq!(tape.scroll, 0.0);
        tape.focus_window("c");
        tape.fit(W);
        assert_eq!(tape.scroll, 500.0);
        tape.focus_window("b");
        tape.fit(W);
        assert_eq!(tape.scroll, 500.0, "b is already wholly in view");
        tape.focus_window("a");
        tape.fit(W);
        assert_eq!(tape.scroll, 0.0);
    }

    #[test]
    fn focus_wraps_at_the_ends_of_the_tape() {
        let mut tape = tape(&["a", "b", "c"]);
        assert!(tape.focus_column(Side::Next));
        assert_eq!(tape.focused().unwrap(), "a");
        assert!(tape.focus_column(Side::Prev));
        assert_eq!(tape.focused().unwrap(), "c");
    }

    #[test]
    fn a_column_walk_lands_on_a_row_beside_the_window_it_left() {
        let mut tape = tape(&["a", "b", "c", "d"]);
        tape.focus_window("b");
        tape.consume_or_expel(Side::Prev, 0.5);
        tape.focus_window("d");
        tape.consume_or_expel(Side::Prev, 0.5);
        assert_eq!(order(&tape), ["a+b", "c+d"]);
        tape.focus_window("a");
        // d was focused last in its column, but lies below a.
        assert!(tape.focus_column(Side::Next));
        assert_eq!(tape.focused().unwrap(), "c");
        tape.focus_window("d");
        tape.focus_window("b");
        assert!(tape.focus_column(Side::Next));
        assert_eq!(tape.focused().unwrap(), "d", "last focused and beside b");
        tape.consume_or_expel(Side::Next, 0.5);
        tape.focus_window("a");
        // From the whole height every row is beside: the last focused.
        tape.focus_window("b");
        tape.focus_column(Side::Prev);
        tape.focus_window("c");
        tape.focus_column(Side::Next);
        assert_eq!(tape.focused().unwrap(), "d");
        assert!(tape.focus_column(Side::Prev));
        assert_eq!(
            tape.focused().unwrap(),
            "c",
            "the full-height d sees both; c was last"
        );
    }

    #[test]
    fn rows_do_not_wrap() {
        let mut tape = tape(&["a", "b"]);
        tape.consume_or_expel(Side::Prev, 0.5);
        assert!(!tape.focus_row(Side::Next));
        assert!(tape.focus_row(Side::Prev));
        assert_eq!(tape.focused().unwrap(), "a");
        assert!(tape.swap_row(Side::Next));
        assert_eq!(order(&tape), ["b+a"]);
        assert_eq!(tape.focused().unwrap(), "a");
    }

    #[test]
    fn swapping_columns_carries_the_focus() {
        let mut tape = tape(&["a", "b", "c"]);
        assert!(tape.swap_column(Side::Prev));
        assert_eq!(order(&tape), ["a", "c", "b"]);
        assert_eq!(tape.focused().unwrap(), "c");
        assert!(tape.swap_column(Side::Prev));
        assert_eq!(order(&tape), ["c", "a", "b"]);
    }

    #[test]
    fn a_swap_at_an_end_trades_with_the_other_end() {
        let mut tape = tape(&["a", "b", "c"]);
        assert!(tape.swap_column(Side::Next));
        assert_eq!(order(&tape), ["c", "b", "a"]);
        assert_eq!(tape.focused().unwrap(), "c");
        assert!(tape.swap_column(Side::Prev));
        assert_eq!(order(&tape), ["a", "b", "c"]);
        assert_eq!(tape.focused().unwrap(), "c");
        let mut lone = Tape::default();
        lone.insert("a".into(), 0.5);
        assert!(!lone.swap_column(Side::Next));
    }

    #[test]
    fn expel_leaves_on_the_asked_side_at_the_default_width() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.consume_or_expel(Side::Prev, 0.5);
        assert_eq!(order(&tape), ["a", "b+c"]);
        tape.set_width(0.667);
        assert!(tape.consume_or_expel(Side::Next, 0.5));
        assert_eq!(order(&tape), ["a", "b", "c"]);
        assert_eq!(tape.focused().unwrap(), "c");
        assert_eq!(
            tape.columns[2].width, 0.5,
            "a new column, as Hyprland's expel makes"
        );
        assert_eq!(tape.columns[1].width, 0.667, "the stack keeps its width");
        tape.consume_or_expel(Side::Prev, 0.5);
        assert!(tape.consume_or_expel(Side::Prev, 0.5));
        assert_eq!(order(&tape), ["a", "c", "b"]);
    }

    #[test]
    fn consume_at_the_edge_does_nothing() {
        let mut tape = tape(&["a", "b"]);
        assert!(!tape.consume_or_expel(Side::Next, 0.5));
        assert_eq!(order(&tape), ["a", "b"]);
    }

    #[test]
    fn removing_a_column_gives_its_place_and_focus_to_the_next() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.focus_window("b");
        tape.remove("b");
        assert_eq!(tape.focused().unwrap(), "c");
        tape.remove("c");
        assert_eq!(tape.focused().unwrap(), "a");
        tape.remove("a");
        assert!(tape.focused().is_none());
    }

    #[test]
    fn removing_left_of_the_focus_keeps_the_focused_window() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.remove("a");
        assert_eq!(tape.focused().unwrap(), "c");
    }

    #[test]
    fn presets_cycle_upward_and_wrap() {
        let presets = [0.333, 0.5, 0.667, 1.0];
        let mut tape = tape(&["a"]);
        let mut seen = Vec::new();
        for _ in 0..4 {
            tape.cycle_width(&presets);
            seen.push(tape.columns[0].width);
        }
        assert_eq!(seen, [0.667, 1.0, 0.333, 0.5]);
    }

    #[test]
    fn widths_stay_between_a_tenth_and_the_whole_strip() {
        let mut tape = tape(&["a"]);
        for _ in 0..9 {
            tape.resize(0.1);
        }
        assert_eq!(tape.columns[0].width, 1.0);
        for _ in 0..12 {
            tape.resize(-0.1);
        }
        assert!((tape.columns[0].width - 0.1).abs() < 1e-9);
    }

    #[test]
    fn move_column_before_another_or_to_the_end() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.focus_window("b");
        assert!(tape.move_column("c", Some("a")));
        assert_eq!(order(&tape), ["c", "a", "b"]);
        assert_eq!(tape.focused().unwrap(), "b");
        assert!(tape.move_column("c", None));
        assert_eq!(order(&tape), ["a", "b", "c"]);
    }

    #[test]
    fn a_released_scroll_snaps_a_column_edge_to_a_view_edge() {
        let mut tape = tape(&["a", "b", "c", "d"]);
        tape.columns[1].width = 0.667;
        assert_eq!(tape.snap(480.0, W), 500.0);
        assert_eq!(tape.snap(140.0, W), 167.0);
        assert_eq!(tape.snap(-300.0, W), 0.0);
        assert_eq!(tape.snap(9000.0, W), tape.max_scroll(W));
    }

    #[test]
    fn a_hand_scroll_moves_the_focus_to_a_column_in_view() {
        let mut tape = tape(&["a", "b", "c", "d"]);
        tape.focus_window("a");
        tape.scroll = 1000.0;
        assert!(tape.focus_in_view(W));
        assert_eq!(tape.focused().unwrap(), "c");
        assert!(!tape.focus_in_view(W));
    }

    #[test]
    fn a_drop_lands_in_the_gap_under_the_pointer() {
        let tape = tape(&["a", "b", "c"]);
        assert_eq!(tape.gap_at(100.0, W), 0);
        assert_eq!(tape.gap_at(300.0, W), 1);
        assert_eq!(tape.gap_at(1100.0, W), 2);
        assert_eq!(tape.gap_at(5000.0, W), 3);
    }

    #[test]
    fn swapping_two_windows_trades_their_slots() {
        let mut tape = tape(&["a", "b", "c"]);
        tape.consume_or_expel(Side::Prev, 0.5);
        assert!(tape.swap_windows("a", "c", 1.0));
        assert_eq!(order(&tape), ["c", "b+a"]);
    }
}
