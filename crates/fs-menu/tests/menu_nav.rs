// The launcher's cursor and key policy: where the cursor lands when the rows
// change under it, where one key moves it on a row list, a grid and the app
// grid's two halves, and which keys belong to the results rather than to the
// search field.

use fs_menu::nav::{Dir, Key, KeyAction, KeyCtx, KeyMode, Modifiers, key_action, rederive, step};

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

// --- Key policy -------------------------------------------------------------------

const NONE: Modifiers = Modifiers { ctrl: false, shift: false, alt: false };
const CTRL: Modifiers = Modifiers { ctrl: true, shift: false, alt: false };
const SHIFT: Modifiers = Modifiers { ctrl: false, shift: true, alt: false };

fn ctx() -> KeyCtx {
    KeyCtx { mode: KeyMode::Menu, query: false, grid: false, app_view: false, scrollable: false, variants: false, level: false, column: false, into: false }
}

fn act(key: Key, mods: Modifiers, repeat: bool, ctx: KeyCtx) -> &'static str {
    key_action(key, mods, repeat, &ctx).as_str()
}

#[test]
fn arrows_walk_the_results() {
    assert_eq!(act(Key::Up, NONE, false, ctx()), "up");
    assert_eq!(act(Key::Down, NONE, false, KeyCtx { query: true, ..ctx() }), "down");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { grid: true, ..ctx() }), "left");
    assert_eq!(act(Key::Right, NONE, false, KeyCtx { grid: true, query: true, ..ctx() }), "right");
}

// On a row list Left/Right are the caret's; on a grid, Ctrl+Left/Right still
// are, which is what keeps a typo fixable while the grid owns the plain arrows.
#[test]
fn the_caret_keeps_ctrl_arrows_and_plain_arrows_on_a_list() {
    let grid_query = KeyCtx { grid: true, query: true, ..ctx() };
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..ctx() }), "pass");
    assert_eq!(act(Key::Left, CTRL, false, grid_query), "pass");
    assert_eq!(act(Key::Right, CTRL, false, grid_query), "pass");
    assert_eq!(act(Key::Left, SHIFT, false, grid_query), "pass");
}

#[test]
fn ctrl_backspace_with_a_query_is_the_fields() {
    let query = KeyCtx { query: true, ..ctx() };
    assert_eq!(act(Key::Backspace, CTRL, false, query), "pass");
    assert_eq!(act(Key::Backspace, NONE, true, query), "pass");
}

// A held Backspace that outlived its text used to walk the whole tree and close
// the launcher.
#[test]
fn backspace_on_an_empty_field_pops_once_per_press() {
    assert_eq!(act(Key::Backspace, NONE, false, ctx()), "pop");
    assert_eq!(act(Key::Backspace, NONE, true, ctx()), "swallow");
    assert_eq!(act(Key::Backspace, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "pass");
}

// Left on an empty field inside a mode leaves it, once per press; with text,
// at the root or on a grid it stays the caret's or the grid's.
#[test]
fn left_on_an_empty_field_leaves_the_mode() {
    let mode = KeyCtx { level: true, ..ctx() };
    assert_eq!(act(Key::Left, NONE, false, mode), "pop");
    assert_eq!(act(Key::Left, NONE, true, mode), "swallow");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..mode }), "pass");
    assert_eq!(act(Key::Left, CTRL, false, mode), "pass");
    assert_eq!(act(Key::Left, NONE, false, ctx()), "pass");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { grid: true, ..mode }), "left");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { mode: KeyMode::Select, ..mode }), "pass");
}

#[test]
fn escape_clears_then_pops() {
    assert_eq!(act(Key::Escape, NONE, false, KeyCtx { query: true, ..ctx() }), "clear");
    assert_eq!(act(Key::Escape, NONE, false, ctx()), "pop");
    assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Select, query: true, ..ctx() }), "clear");
    assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "close");
    assert_eq!(act(Key::Escape, NONE, false, KeyCtx { mode: KeyMode::Input, query: true, ..ctx() }), "close");
}

#[test]
fn tab_opens_the_actions_and_never_leaves_the_field() {
    assert_eq!(act(Key::Tab, NONE, false, ctx()), "actions");
    assert_eq!(act(Key::Tab, NONE, false, KeyCtx { variants: true, ..ctx() }), "actions");
    assert_eq!(act(Key::Tab, CTRL, false, KeyCtx { variants: true, ..ctx() }), "variant");
    assert_eq!(act(Key::Backtab, SHIFT, false, ctx()), "swallow");
    assert_eq!(act(Key::Tab, NONE, false, KeyCtx { mode: KeyMode::Select, ..ctx() }), "swallow");
}

#[test]
fn the_column_has_the_keys_while_open() {
    let column = KeyCtx { column: true, ..ctx() };
    assert_eq!(act(Key::Down, NONE, false, column), "down");
    assert_eq!(act(Key::Return, SHIFT, false, column), "activate");
    assert_eq!(act(Key::Tab, NONE, false, column), "swallow");
    assert_eq!(act(Key::Backtab, SHIFT, false, column), "back");
    assert_eq!(act(Key::Escape, NONE, false, KeyCtx { query: true, ..column }), "clear");
    assert_eq!(act(Key::Escape, NONE, false, column), "back");
    assert_eq!(act(Key::Left, NONE, false, column), "back");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { query: true, ..column }), "pass");
    assert_eq!(act(Key::Backspace, NONE, false, column), "back");
    assert_eq!(act(Key::Backspace, NONE, true, column), "swallow");
    assert_eq!(act(Key::Backspace, NONE, false, KeyCtx { query: true, ..column }), "pass");
}

#[test]
fn right_steps_into_a_subject_only_when_it_can() {
    let into = KeyCtx { into: true, ..ctx() };
    assert_eq!(act(Key::Right, NONE, false, into), "into");
    assert_eq!(act(Key::Right, CTRL, false, into), "pass");
    assert_eq!(act(Key::Right, NONE, false, ctx()), "pass");
    assert_eq!(act(Key::Right, NONE, false, KeyCtx { mode: KeyMode::Select, ..into }), "pass");
    assert_eq!(act(Key::Right, NONE, false, KeyCtx { grid: true, ..into }), "right");
}

#[test]
fn page_home_and_end_go_to_the_results() {
    assert_eq!(act(Key::PageDown, NONE, false, ctx()), "pageDown");
    assert_eq!(act(Key::PageUp, NONE, false, KeyCtx { grid: true, ..ctx() }), "pageUp");
    assert_eq!(act(Key::Home, NONE, false, KeyCtx { query: true, ..ctx() }), "home");
    assert_eq!(act(Key::End, NONE, false, KeyCtx { query: true, ..ctx() }), "end");
    assert_eq!(act(Key::Home, SHIFT, false, KeyCtx { query: true, ..ctx() }), "pass");
    assert_eq!(act(Key::End, NONE, false, KeyCtx { mode: KeyMode::Input, query: true, ..ctx() }), "pass");
}

#[test]
fn an_app_view_scrolls_instead() {
    let view = KeyCtx { app_view: true, ..ctx() };
    let scrolling = KeyCtx { app_view: true, scrollable: true, ..ctx() };
    assert_eq!(act(Key::Down, NONE, false, view), "scrollDown");
    assert_eq!(act(Key::PageDown, NONE, false, scrolling), "scrollPageDown");
    assert_eq!(act(Key::PageDown, NONE, false, view), "pass");
    assert_eq!(act(Key::Home, NONE, false, scrolling), "scrollHome");
    assert_eq!(act(Key::Left, NONE, false, KeyCtx { app_view: true, grid: true, ..ctx() }), "pass");
}

#[test]
fn enter_activates_and_shift_enter_is_the_alternate() {
    assert_eq!(act(Key::Return, NONE, false, ctx()), "activate");
    assert_eq!(act(Key::Enter, SHIFT, false, ctx()), "activateAlternate");
    assert_eq!(act(Key::Return, NONE, false, KeyCtx { mode: KeyMode::Input, ..ctx() }), "submit");
}

#[test]
fn printable_keys_are_the_fields() {
    assert_eq!(key_action(Key::Other, NONE, false, &KeyCtx { grid: true, ..ctx() }), KeyAction::Pass);
}
