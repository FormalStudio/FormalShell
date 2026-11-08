// The launcher's cursor: where it lands when the rows change under it, and
// where one key moves it on a row list, a grid and the app grid's two halves.

use fs_menu::nav::{Dir, rederive, step};

// --- Re-derivation ------------------------------------------------------------

// Clipboard or window churn inserts a row above a cursor the reader put
// somewhere: the cursor stays on its row rather than on its old slot.
#[test]
fn a_placed_cursor_keeps_its_row_when_rows_arrive_above_it() {
    assert_eq!(rederive("b", 1, &["x", "a", "b", "c"], false, true), 2);
}

#[test]
fn a_placed_cursor_keeps_its_row_when_rows_leave_above_it() {
    assert_eq!(rederive("c", 3, &["a", "c"], false, true), 1);
}

// The row it was on left: whatever took its slot, clamped to what is left, and
// never past the end, which is where Enter used to do nothing.
#[test]
fn a_row_that_left_hands_the_cursor_to_its_slot() {
    assert_eq!(rederive("b", 1, &["a", "c", "d"], false, true), 1);
    assert_eq!(rederive("z", 7, &["a", "c"], false, true), 1);
}

// The desktop entries reload one at a time, so the row the cursor was put on
// can leave and come back: it gets the cursor back rather than the cursor
// staying on whatever it was clamped to meanwhile.
#[test]
fn a_row_that_comes_back_gets_the_cursor_back() {
    assert_eq!(rederive("b", 1, &["c"], false, true), 0);
    assert_eq!(rederive("b", 0, &[], false, true), 0);
    assert_eq!(rederive("b", 0, &["a", "b", "c"], false, true), 1);
}

// A `when` condition resolving after the open inserts rows above the one the
// cursor started on: nobody put it there, so it stays on the head.
#[test]
fn an_unplaced_cursor_stays_on_the_first_row() {
    assert_eq!(rederive("b", 0, &["x", "y", "b"], false, false), 0);
}

// A new query, level or open always starts on the first row, even when the row
// the cursor was on survived it.
#[test]
fn a_fresh_list_starts_on_its_first_row() {
    assert_eq!(rederive("b", 1, &["a", "b"], true, true), 0);
}

#[test]
fn an_empty_list_puts_the_cursor_at_zero() {
    assert_eq!(rederive("b", 3, &[], false, true), 0);
    assert_eq!(rederive("", 0, &["a"], false, true), 0);
}

// --- Row list -----------------------------------------------------------------

#[test]
fn a_row_list_steps_and_wraps() {
    let down = |i| step(i, Dir::Down, 5, 0, 1, 3);
    assert_eq!(down(0).index, 1);
    assert!(down(0).travels);
    assert_eq!(down(4).index, 0);
    assert!(!down(4).travels);
    let up = step(0, Dir::Up, 5, 0, 1, 3);
    assert_eq!(up.index, 4);
    assert!(!up.travels);
}

#[test]
fn page_home_and_end_jump_without_wrapping() {
    assert_eq!(step(1, Dir::PageDown, 20, 0, 1, 6).index, 7);
    assert_eq!(step(17, Dir::PageDown, 20, 0, 1, 6).index, 19);
    assert_eq!(step(3, Dir::PageUp, 20, 0, 1, 6).index, 0);
    assert_eq!(step(9, Dir::Home, 20, 0, 1, 6).index, 0);
    assert_eq!(step(2, Dir::End, 20, 0, 1, 6).index, 19);
    assert!(!step(1, Dir::PageDown, 20, 0, 1, 6).travels);
}

#[test]
fn an_empty_list_goes_nowhere() {
    assert_eq!(step(0, Dir::Down, 0, 0, 1, 1).index, 0);
}

// --- Grid (picker, emoji) -------------------------------------------------------

// Ten cells over four columns: rows 0-3, 4-7 and a short 8-9.
#[test]
fn down_off_the_grid_wraps_to_the_same_column() {
    assert_eq!(step(9, Dir::Down, 10, 10, 4, 4).index, 1);
    assert_eq!(step(8, Dir::Down, 10, 10, 4, 4).index, 0);
}

// The old `raw % n` wrap lost the column whenever the cell count was not a
// multiple of the column count.
#[test]
fn up_off_the_grid_keeps_the_column_over_a_short_last_row() {
    assert_eq!(step(1, Dir::Up, 10, 10, 4, 4).index, 9);
    assert_eq!(step(3, Dir::Up, 10, 10, 4, 4).index, 7);
    assert_eq!(step(0, Dir::Up, 12, 12, 4, 4).index, 8);
}

// A column the short last row does not reach goes down onto the last cell
// rather than skipping out of the grid.
#[test]
fn down_into_a_short_last_row_lands_on_its_last_cell() {
    assert_eq!(step(7, Dir::Down, 10, 10, 4, 4).index, 9);
    assert_eq!(step(5, Dir::Down, 10, 10, 4, 4).index, 9);
    assert_eq!(step(4, Dir::Down, 10, 10, 4, 4).index, 8);
}

// Left on the first result used to wrap to the last one.
#[test]
fn left_and_right_walk_reading_order_and_stop_at_the_ends() {
    assert_eq!(step(3, Dir::Right, 10, 10, 4, 4).index, 4);
    assert_eq!(step(4, Dir::Left, 10, 10, 4, 4).index, 3);
    assert_eq!(step(0, Dir::Left, 10, 10, 4, 4).index, 0);
    assert_eq!(step(9, Dir::Right, 10, 10, 4, 4).index, 9);
}

// --- App grid: six cells over four columns, three rows under them --------------

#[test]
fn down_out_of_the_cells_lands_on_the_first_row_under_them() {
    assert_eq!(step(5, Dir::Down, 9, 6, 4, 4).index, 6);
    assert_eq!(step(4, Dir::Down, 9, 6, 4, 4).index, 6);
    assert_eq!(step(1, Dir::Down, 9, 6, 4, 4).index, 5);
}

#[test]
fn the_rows_under_the_cells_move_by_a_row() {
    assert_eq!(step(6, Dir::Down, 9, 6, 4, 4).index, 7);
    assert_eq!(step(7, Dir::Up, 9, 6, 4, 4).index, 6);
    assert_eq!(step(6, Dir::Up, 9, 6, 4, 4).index, 5);
}

#[test]
fn the_app_grid_wraps_through_its_rows() {
    assert_eq!(step(8, Dir::Down, 9, 6, 4, 4).index, 0);
    assert_eq!(step(2, Dir::Up, 9, 6, 4, 4).index, 8);
}

// A query no app matches leaves the grid with no cells, and the rows under it
// are then a plain list.
#[test]
fn an_app_grid_with_no_cells_is_a_row_list() {
    assert_eq!(step(0, Dir::Down, 3, 0, 4, 4).index, 1);
    assert_eq!(step(0, Dir::Up, 3, 0, 4, 4).index, 2);
}

#[test]
fn directions_parse_from_their_ipc_names() {
    assert_eq!(Dir::parse("pageDown"), Dir::PageDown);
    assert_eq!(Dir::parse("sideways"), Dir::Other);
    assert_eq!(step(3, Dir::Other, 10, 0, 1, 4).index, 3);
}
