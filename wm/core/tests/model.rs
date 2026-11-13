mod common;

use common::*;
use formalwm_core::{
    geom::Rect,
    model::{Big, Home, Layer, Options, Placement, WorkspaceSpec},
    tape::Side,
};

#[test]
fn eight_pixels_between_everything() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    let (a, b) = (place(&model, "a"), place(&model, "b"));
    assert_eq!(a.rect, Rect::new(8.0, 8.0, 948.0, 1064.0));
    assert_eq!(b.rect, Rect::new(964.0, 8.0, 948.0, 1064.0));
    assert_eq!(b.rect.x - a.rect.right(), 8.0);
    assert_eq!(1920.0 - b.rect.right(), 8.0);
    assert_eq!((b.column, b.row), (1, 0));
    assert!(a.scrolls && a.shown);
}

#[test]
fn the_tape_keeps_clear_of_what_the_layers_reserve() {
    let mut model = desk();
    let mut geo = output("A", 0.0, 1920.0, 1080.0);
    geo.usable = Rect::new(0.0, 40.0, 1920.0, 1040.0);
    model.set_outputs(vec![geo]);
    open(&mut model, "a");
    model.set_column_width(1.0);
    assert_eq!(
        place(&model, "a").rect,
        Rect::new(8.0, 48.0, 1904.0, 1024.0)
    );
}

#[test]
fn a_new_window_takes_the_focus_on_the_focused_workspace() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert_eq!(focused(&model), "b");
    assert_eq!(model.window("b").unwrap().home, Home::Workspace("1".into()));
    assert_eq!(model.window("b").unwrap().initial_title, "B");
}

#[test]
fn the_view_follows_a_window_to_the_workspace_its_rule_names() {
    let mut model = desk();
    open(&mut model, "a");
    let rule = Placement {
        workspace: Some("3".into()),
        ..Placement::default()
    };
    open_with(&mut model, "code", rule);
    assert_eq!(model.focused_workspace().unwrap().id, "3");
    assert_eq!(focused(&model), "code");
    assert!(!place(&model, "a").shown);
    assert!(model.focus_previous_workspace());
    assert_eq!(model.focused_workspace().unwrap().id, "1");
}

#[test]
fn workspaces_pinned_by_the_host_live_on_their_output() {
    let model = two_outputs();
    let stack = |output: &str| -> Vec<String> {
        model.stack(output).iter().map(|ws| ws.id.clone()).collect()
    };
    assert_eq!(stack("A"), ["1", "2", "3", "5", "6", "7", "9"]);
    assert_eq!(stack("B"), ["4", "8"]);
    assert_eq!(model.active_workspace("B").unwrap().id, "4");
    assert_eq!(model.stack_index("8"), Some(1));
}

#[test]
fn an_unplugged_outputs_workspaces_fall_to_the_first_and_return() {
    let mut model = two_outputs();
    model.focus_workspace("4");
    open(&mut model, "chat");
    assert_eq!(model.focused_output, "B");
    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0)]);
    assert_eq!(model.workspace("4").unwrap().output, "A");
    assert_eq!(model.focused_output, "A");
    assert_eq!(
        model.window("chat").unwrap().home,
        Home::Workspace("4".into())
    );
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        output("B", 1920.0, 1600.0, 1000.0),
    ]);
    assert_eq!(model.workspace("4").unwrap().output, "B");
    assert_eq!(place(&model, "chat").output, "B");
}

#[test]
fn an_output_that_returns_shows_what_it_showed_and_takes_its_workspaces_back() {
    let mut model = desk();
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        output("C", 1920.0, 1280.0, 720.0),
    ]);
    let own = model.active_workspace("C").unwrap().id.clone();
    model.focus_workspace(&own);
    open(&mut model, "note");
    model.focus_workspace("3");
    model.set_outputs(vec![output("C", 1920.0, 1280.0, 720.0)]);
    assert_eq!(model.workspace("3").unwrap().output, "C");
    model.set_outputs(Vec::new());
    assert_eq!(model.focused_workspace().map(|ws| ws.id.as_str()), None);
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        output("C", 1920.0, 1280.0, 720.0),
    ]);
    assert_eq!(model.active_workspace("A").unwrap().id, "3");
    assert_eq!(model.active_workspace("C").unwrap().id, own);
    assert_eq!(place(&model, "note").output, "C");
    assert_eq!(model.stack("A").len(), 9);
}

#[test]
fn an_output_with_no_workspace_of_its_own_gets_one() {
    let mut model = desk();
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        output("C", 1920.0, 1280.0, 720.0),
    ]);
    let ws = model.active_workspace("C").unwrap();
    assert!(!ws.named);
    assert_eq!(model.stack("A").len(), 9);
}

#[test]
fn focusing_a_window_goes_to_its_workspace_and_output() {
    let mut model = two_outputs();
    open(&mut model, "a");
    model.focus_workspace("8");
    open(&mut model, "music");
    model.focus_workspace("1");
    assert_eq!(focused(&model), "a");
    assert!(model.focus_window("music"));
    assert_eq!(model.focused_output, "B");
    assert_eq!(model.active_workspace("B").unwrap().id, "8");
    assert_eq!(focused(&model), "music");
}

#[test]
fn the_focused_column_is_fitted_into_view() {
    let mut model = desk();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    let scroll =
        |model: &formalwm_core::model::Model| model.focused_workspace().unwrap().tape.scroll;
    assert_eq!(scroll(&model), 956.0);
    model.focus_window("a");
    assert_eq!(scroll(&model), 0.0);
    assert!(model.focus_column(Side::Prev), "wraps to the last column");
    assert_eq!(focused(&model), "c");
    assert_eq!(scroll(&model), 956.0);
}

#[test]
fn floating_and_back() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.set_floating("b", None));
    let b = place(&model, "b");
    assert_eq!(b.layer, Layer::Float);
    assert_eq!((b.column, b.row), (-1, -1));
    // Around the middle of the tile it was, [964,8 948x1064].
    assert_eq!(b.rect, Rect::new(958.0, 216.0, 960.0, 648.0));
    assert_eq!(focused(&model), "b");
    assert!(model.place("b", Rect::new(100.0, 120.0, 640.0, 480.0)));
    assert!(model.set_floating("b", Some(false)));
    assert_eq!(place(&model, "b").layer, Layer::Tile);
    assert!(model.set_floating("b", Some(true)));
    assert_eq!(
        place(&model, "b").rect,
        Rect::new(1118.0, 300.0, 640.0, 480.0),
        "the size it last floated at"
    );
    // Wider than the room right of the tile's middle: pushed back inside.
    assert!(model.place("b", Rect::new(0.0, 0.0, 1400.0, 480.0)));
    assert!(model.set_floating("b", Some(false)));
    assert!(model.set_floating("b", Some(true)));
    assert_eq!(
        place(&model, "b").rect,
        Rect::new(520.0, 300.0, 1400.0, 480.0)
    );
    assert!(
        !model.place("a", Rect::new(0.0, 0.0, 10.0, 10.0)),
        "a is tiled"
    );
}

#[test]
fn a_column_bind_never_steps_onto_a_float_nor_off_one() {
    // Hyprland's `layout focus` asks the scrolling layout for the focused
    // window's column: a float has none, so the bind does nothing there,
    // and the layout knows no float to step onto.
    let mut model = desk();
    open(&mut model, "tile");
    open(&mut model, "mac");
    assert!(model.set_floating("mac", Some(true)));
    assert_eq!(focused(&model), "mac");
    assert!(!model.focus_column(Side::Next));
    assert_eq!(focused(&model), "mac");
    model.focus_window("tile");
    assert!(!model.focus_column(Side::Prev), "one tile and a float");
    assert_eq!(focused(&model), "tile");
    assert!(model.set_floating("mac", Some(false)));
    model.focus_window("tile");
    assert!(model.focus_column(Side::Prev));
    assert_eq!(focused(&model), "mac", "tiled, it is a column like any");
}

#[test]
fn fullscreen_covers_the_output_and_maximize_what_the_layers_leave() {
    let mut model = desk();
    let mut geo = output("A", 0.0, 1920.0, 1080.0);
    geo.usable = Rect::new(0.0, 40.0, 1920.0, 1040.0);
    model.set_outputs(vec![geo]);
    open(&mut model, "a");
    open(&mut model, "b");
    assert_eq!(model.fullscreen_on("A"), None);
    assert!(model.toggle_big("b", Big::Fullscreen));
    assert_eq!(model.fullscreen_on("A").map(String::as_str), Some("b"));
    let b = place(&model, "b");
    assert_eq!(
        (on_screen(&model, "b"), b.layer),
        (Rect::new(0.0, 0.0, 1920.0, 1080.0), Layer::Fullscreen)
    );
    assert!(model.toggle_big("b", Big::Maximized));
    assert_eq!(model.fullscreen_on("A"), None);
    assert_eq!(on_screen(&model, "b"), Rect::new(0.0, 40.0, 1920.0, 1040.0));
    assert!(model.toggle_big("b", Big::Maximized));
    assert_eq!(place(&model, "b").layer, Layer::Tile);
    assert_eq!(on_screen(&model, "b").w, 948.0, "back to its width");
}

#[test]
fn a_big_tile_stays_a_column_of_the_tape() {
    // Hyprland 0.56's scrolling layout: maximize widens the column to the
    // view and fullscreen to the output; the tape scrolls past it, the
    // neighbours stay a bind away and the mark outlives the focus.
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    open(&mut model, "c");
    model.focus_window("b");
    assert!(model.toggle_big("b", Big::Maximized));
    let b = place(&model, "b");
    assert!(b.scrolls, "it scrolls with the tape");
    assert_eq!(on_screen(&model, "b"), Rect::new(0.0, 0.0, 1920.0, 1080.0));

    assert!(model.focus_column(Side::Next));
    assert_eq!(focused(&model), "c");
    assert_eq!(
        model.big_of("b"),
        Some(Big::Maximized),
        "focus moving on keeps it"
    );
    assert_eq!(place(&model, "b").layer, Layer::Tile);
    assert_eq!(
        on_screen(&model, "c").right(),
        1912.0,
        "fitted by the least scroll"
    );
    assert_eq!(
        on_screen(&model, "b").right(),
        956.0,
        "b slid half out of view"
    );

    assert!(model.focus_column(Side::Prev));
    assert_eq!(place(&model, "b").layer, Layer::Maximized);
    assert_eq!(on_screen(&model, "b"), Rect::new(0.0, 0.0, 1920.0, 1080.0));

    // Fullscreen over it, and back to the width it had before either.
    assert!(model.toggle_big("b", Big::Fullscreen));
    assert!(model.toggle_big("b", Big::Fullscreen));
    assert_eq!(model.big_of("b"), None);
    assert_eq!(on_screen(&model, "b").w, 948.0);
}

#[test]
fn a_stacked_tile_made_big_leaves_its_stack_first() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.consume_or_expel(Side::Prev));
    assert!(model.toggle_big("b", Big::Fullscreen));
    let (a, b) = (place(&model, "a"), place(&model, "b"));
    assert_eq!((a.column, b.column), (0, 1), "promoted right of its stack");
    assert_eq!(on_screen(&model, "b"), Rect::new(0.0, 0.0, 1920.0, 1080.0));
    // Taking another window in ends it.
    model.focus_window("a");
    assert!(model.consume_or_expel(Side::Next));
    assert_eq!(model.big_of("b"), None);
}

#[test]
fn a_new_window_opens_beside_a_big_tile_and_leaves_it_big() {
    let mut model = desk();
    open(&mut model, "a");
    assert!(model.toggle_big("a", Big::Maximized));
    open(&mut model, "b");
    assert_eq!(model.big_of("a"), Some(Big::Maximized));
    assert_eq!(focused(&model), "b");
    assert_eq!(
        on_screen(&model, "b").right(),
        1912.0,
        "b is fitted into view"
    );
}

#[test]
fn a_tile_that_bounds_its_size_sits_in_the_middle_of_its_cell() {
    let mut model = desk();
    open(&mut model, "a");
    let cell = on_screen(&model, "a");
    model.set_max_size("a", Some((300.0, 200.0)));
    let held = on_screen(&model, "a");
    assert_eq!((held.w, held.h), (300.0, 200.0));
    assert_eq!(held.center(), cell.center());
    // A bound of 0 leaves that axis free.
    model.set_max_size("a", Some((300.0, 0.0)));
    let held = on_screen(&model, "a");
    assert_eq!((held.w, held.h), (300.0, cell.h));
    assert!(model.toggle_big("a", Big::Maximized));
    let big = on_screen(&model, "a");
    assert_eq!(big.w, 300.0);
    assert_eq!(big.center().0, 960.0, "in the middle of the maximized area");
    assert!(model.toggle_big("a", Big::Maximized));
    model.set_max_size("a", None);
    assert_eq!(on_screen(&model, "a"), cell);
}

#[test]
fn a_big_tile_covers_its_output_after_the_output_changes() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.set_big("b", Some(Big::Fullscreen)));
    model.set_outputs(vec![output("A", 0.0, 3840.0, 2160.0)]);
    assert_eq!(on_screen(&model, "b"), Rect::new(0.0, 0.0, 3840.0, 2160.0));
    assert!(model.set_big("b", Some(Big::Maximized)));
    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0)]);
    let border = Options::default().border;
    assert_eq!(
        on_screen(&model, "b"),
        Rect::new(0.0, 0.0, 1920.0, 1080.0).inset(border)
    );
    // Its output unplugged, the workspace moves to a bigger one.
    let mut model = desk();
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        output("B", 1920.0, 3840.0, 2160.0),
    ]);
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.set_big("b", Some(Big::Fullscreen)));
    model.set_outputs(vec![output("B", 1920.0, 3840.0, 2160.0)]);
    assert_eq!(
        on_screen(&model, "b"),
        Rect::new(1920.0, 0.0, 3840.0, 2160.0)
    );
}

#[test]
fn a_swap_keeps_big_state_with_its_window() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_window("a");
    assert!(model.set_big("a", Some(Big::Fullscreen)));
    assert!(model.swap("a", "b"));
    assert_eq!(model.big_of("a"), Some(Big::Fullscreen));
    assert_eq!(model.big_of("b"), None);
    assert_eq!(focused(&model), "a");
    assert_eq!(on_screen(&model, "a"), Rect::new(0.0, 0.0, 1920.0, 1080.0));
    open(&mut model, "c");
    assert!(model.set_big("c", Some(Big::Maximized)));
    assert!(model.swap("a", "c"));
    assert_eq!(model.big_of("a"), Some(Big::Fullscreen));
    assert_eq!(model.big_of("c"), Some(Big::Maximized));
    assert_eq!(model.big_of("b"), None);
}

#[test]
fn a_fullscreen_tile_is_not_the_outputs_while_a_float_has_the_focus() {
    let mut model = desk();
    open(&mut model, "a");
    assert!(model.set_big("a", Some(Big::Fullscreen)));
    assert_eq!(model.fullscreen_on("A").map(String::as_str), Some("a"));
    open_with(
        &mut model,
        "f",
        Placement {
            floating: true,
            ..Placement::default()
        },
    );
    assert_eq!(place(&model, "a").layer, Layer::Tile);
    assert_eq!(model.fullscreen_on("A"), None);
    model.focus_window("a");
    assert_eq!(model.fullscreen_on("A").map(String::as_str), Some("a"));
}

#[test]
fn a_tile_dragged_by_an_edge_moves_that_edge_only() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_window("a");
    let inset = 2.0 * Options::default().tile_inset();
    let column = |model: &formalwm_core::model::Model, id: &str| on_screen(model, id).w + inset;
    let (a, b) = (on_screen(&model, "a"), on_screen(&model, "b"));
    // By its right edge: its left edge stays.
    assert!(model.resize_tile_edge("a", false, column(&model, "a") + 100.0));
    let now = on_screen(&model, "a");
    assert_eq!((now.x, now.w), (a.x, a.w + 100.0));
    model.release_tile_edge("a");
    // By its left edge: its right edge stays, the view follows.
    let b_right = on_screen(&model, "b").right();
    assert!(model.resize_tile_edge("b", true, column(&model, "b") + 50.0));
    let now = on_screen(&model, "b");
    assert_eq!((now.right(), now.w), (b_right, b.w + 50.0));
    model.release_tile_edge("b");
    assert_eq!(
        on_screen(&model, "b").right(),
        b_right,
        "and after it is let go"
    );
    assert_eq!(focused(&model), "a", "a drag leaves the focus where it is");
    // Neither edge goes out of the view.
    assert!(model.resize_tile_edge("b", false, column(&model, "b") + 900.0));
    assert!(on_screen(&model, "b").right() <= 1920.0);
    assert!(model.resize_tile_edge("a", true, column(&model, "a") + 900.0));
    assert!(on_screen(&model, "a").x >= 0.0);
}

#[test]
fn a_left_border_shrunk_at_the_start_of_the_tape_keeps_the_right_edge() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_window("a");
    let inset = 2.0 * Options::default().tile_inset();
    let a = on_screen(&model, "a");
    assert!(model.resize_tile_edge("a", true, a.w + inset - 60.0));
    assert!(model.resize_tile_edge("a", true, a.w + inset - 100.0));
    let now = on_screen(&model, "a");
    assert_eq!((now.x, now.right()), (a.x + 100.0, a.right()));
    // Let go, the view comes back to the start of the tape.
    model.release_tile_edge("a");
    let now = on_screen(&model, "a");
    assert_eq!((now.x, now.w), (a.x, a.w - 100.0));
}

#[test]
fn a_lone_tile_dragged_by_a_border_moves_that_edge_and_recentres_on_release() {
    let mut model = desk();
    open(&mut model, "a");
    let inset = 2.0 * Options::default().tile_inset();
    let a = on_screen(&model, "a");
    assert!(model.resize_tile_edge("a", false, a.w + inset + 100.0));
    let now = on_screen(&model, "a");
    assert_eq!((now.x, now.w), (a.x, a.w + 100.0), "by its right border");
    assert!(model.resize_tile_edge("a", true, now.w + inset + 40.0));
    let held = on_screen(&model, "a");
    assert_eq!(
        (held.right(), held.w),
        (now.right(), now.w + 40.0),
        "by its left border"
    );
    model.release_tile_edge("a");
    let centred = on_screen(&model, "a");
    assert_eq!(centred.w, held.w);
    assert_eq!(centred.x - 0.0, 1920.0 - centred.right(), "centred again");
}

#[test]
fn a_tile_moved_away_mid_drag_leaves_no_hold_behind() {
    let mut model = desk();
    open(&mut model, "a");
    let inset = 2.0 * Options::default().tile_inset();
    let a = on_screen(&model, "a");
    assert!(model.resize_tile_edge("a", false, a.w + inset + 100.0));
    assert!(model.move_window("a", "2", None));
    model.release_tile_edge("a");
    assert!(model.move_window("a", "1", None));
    let back = on_screen(&model, "a");
    assert_eq!(
        back.x - 0.0,
        1920.0 - back.right(),
        "centred on its old tape"
    );
}

#[test]
fn a_window_sent_away_leaves_the_focus_here_unless_followed() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.move_to_workspace("b", "5", false));
    assert_eq!(model.focused_workspace().unwrap().id, "1");
    assert_eq!(focused(&model), "a");
    assert_eq!(model.window("b").unwrap().home, Home::Workspace("5".into()));
    assert!(model.move_to_workspace("a", "5", true));
    assert_eq!(model.focused_workspace().unwrap().id, "5");
    assert_eq!(focused(&model), "a");
    assert!(!model.move_to_workspace("a", "5", true));
}

#[test]
fn swap_and_move_column_cross_workspaces() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_workspace("2");
    open(&mut model, "c");
    assert!(model.swap("a", "c"));
    assert_eq!(model.window("a").unwrap().home, Home::Workspace("2".into()));
    assert_eq!(model.window("c").unwrap().home, Home::Workspace("1".into()));
    assert_eq!(place(&model, "c").column, 0);
    assert!(model.move_column("a", Some("b")));
    assert_eq!(model.window("a").unwrap().home, Home::Workspace("1".into()));
    let columns: Vec<i32> = ["c", "a", "b"]
        .iter()
        .map(|id| place(&model, id).column)
        .collect();
    assert_eq!(columns, [0, 1, 2]);
    assert!(model.workspace("2").unwrap().is_empty());
    assert!(model.move_column("c", None));
    assert_eq!(place(&model, "c").column, 2);
}

#[test]
fn the_console_maps_parked_and_shows_over_the_focused_output() {
    let mut model = two_outputs();
    open(&mut model, "a");
    let rule = Placement {
        overlay: Some("formalshell-console".into()),
        floating: true,
        ..Placement::default()
    };
    open_with(&mut model, "console", rule);
    assert_eq!(focused(&model), "a", "a parked window takes no focus");
    let parked = place(&model, "console");
    assert_eq!(
        (parked.layer, parked.shown, parked.workspace),
        (Layer::Overlay, false, None)
    );
    assert!(!model.focus_window("console"));

    model.focus_workspace("4");
    assert!(model.show_overlay("console"));
    assert_eq!(focused(&model), "console");
    let shown = place(&model, "console");
    assert!(shown.shown);
    assert_eq!(shown.output, "B");
    assert!(model.place("console", Rect::new(2000.0, 0.0, 1440.0, 400.0)));
    assert_eq!(
        place(&model, "console").rect,
        Rect::new(2000.0, 0.0, 1440.0, 400.0)
    );

    assert!(model.park("console", "formalshell-console"));
    assert!(!place(&model, "console").shown);
    assert_eq!(model.focused_workspace().unwrap().id, "4");
    assert!(!model.show_overlay("a"), "a is on no overlay");
}

#[test]
fn any_window_can_be_parked_and_brought_back_floating() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    assert!(model.park("b", "scratch"));
    assert_eq!(focused(&model), "a");
    assert_eq!(
        place(&model, "a").rect.w,
        948.0,
        "a is alone and keeps its width"
    );
    assert!(model.move_to_workspace("b", "1", true));
    assert!(model.window("b").unwrap().floating);
}

#[test]
fn zen_takes_the_centre_and_the_tape_slides_off_its_sides() {
    let mut model = desk();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    model.focus_window("b");
    let before = place(&model, "b").rect;
    assert!(model.set_zen(None, None));
    assert_eq!(model.focused_workspace().unwrap().zen.as_deref(), Some("b"));
    let zen = place(&model, "b");
    assert_eq!(zen.layer, Layer::Zen);
    assert_eq!(zen.rect, Rect::new(384.0, 65.0, 1152.0, 950.0));
    let scroll = model.focused_workspace().unwrap().tape.scroll;
    assert!(
        place(&model, "a").rect.right() - scroll <= 0.0,
        "a went off the left"
    );
    assert!(
        place(&model, "c").rect.x - scroll >= 1920.0,
        "c went off the right"
    );
    assert!(!model.scroll_by("1", 100.0), "the tape holds still");
    assert!(!model.focus_column(Side::Next));
    assert_eq!(focused(&model), "b");

    assert!(model.set_zen(None, None));
    assert_eq!(place(&model, "b").rect, before);
    assert_eq!(model.focused_workspace().unwrap().zen, None);
}

#[test]
fn closing_the_zen_window_brings_the_tape_back() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.set_zen(Some("b"), Some(true));
    model.close("b");
    assert_eq!(model.focused_workspace().unwrap().zen, None);
    assert_eq!(place(&model, "a").layer, Layer::Tile);
    assert_eq!(focused(&model), "a");
}

#[test]
fn a_floating_window_in_zen_returns_to_its_rect() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "f");
    model.set_floating("f", Some(true));
    let rect = place(&model, "f").rect;
    model.set_zen(Some("f"), Some(true));
    assert_eq!(
        place(&model, "f").rect,
        Rect::new(384.0, 65.0, 1152.0, 950.0)
    );
    assert!(place(&model, "a").rect.x >= 1920.0 || place(&model, "a").rect.right() <= 0.0);
    model.set_zen(Some("f"), Some(false));
    assert_eq!(place(&model, "f").rect, rect);
}

#[test]
fn a_pinned_float_shows_on_every_workspace_of_its_output() {
    let mut model = two_outputs();
    let rule = Placement {
        floating: true,
        pinned: true,
        float_rect: Some(Rect::new(1300.0, 700.0, 600.0, 360.0)),
        ..Placement::default()
    };
    open_with(&mut model, "pip", rule);
    model.focus_workspace("3");
    open(&mut model, "a");
    model.focus_workspace("1");
    model.focus_workspace("3");
    let pip = place(&model, "pip");
    assert_eq!((pip.layer, pip.shown), (Layer::Pinned, true));
    assert_eq!(pip.workspace.as_deref(), Some("3"));
    assert_eq!(
        focused(&model),
        "a",
        "it comes along without taking the focus"
    );
    model.focus_workspace("4");
    assert_eq!(place(&model, "pip").output, "A", "it stays on its output");
}

#[test]
fn a_workspace_opened_in_the_overview_goes_when_it_empties() {
    let mut model = desk();
    open(&mut model, "a");
    let id = model.insert_workspace("A", 1);
    assert_eq!(model.stack_index(&id), Some(1));
    assert_eq!(model.stack_index("2"), Some(2));
    assert!(model.move_to_workspace("a", &id, true));
    assert_eq!(model.focused_workspace().unwrap().id, id);
    model.focus_workspace("1");
    assert!(model.workspace(&id).is_some(), "it has a window");
    model.close("a");
    assert!(model.workspace(&id).is_none());
    assert_eq!(model.stack("A").len(), 9);
}

#[test]
fn workspaces_by_stack_position_and_by_neighbour() {
    let mut model = two_outputs();
    assert!(model.focus_workspace_at(2, Some("B")));
    assert_eq!(model.focused_workspace().unwrap().id, "8");
    assert!(model.focus_workspace_at(4, Some("A")));
    assert_eq!(model.focused_workspace().unwrap().id, "5");
    assert!(model.focus_workspace_relative(1));
    assert_eq!(model.focused_workspace().unwrap().id, "6");
    assert!(!model.focus_workspace_at(12, None));
    model.focus_workspace("1");
    assert!(!model.focus_workspace_relative(-1), "the stack ends");
}

#[test]
fn the_wheel_walks_the_workspaces_that_hold_a_window_and_wraps() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    open(&mut model, "c");
    model.move_to_workspace("b", "4", false);
    model.move_to_workspace("c", "8", false);
    let mut seen = Vec::new();
    for _ in 0..4 {
        model.focus_occupied_workspace(1);
        seen.push(model.focused_workspace().unwrap().id.clone());
    }
    assert_eq!(seen, ["4", "8", "1", "4"]);
    model.focus_occupied_workspace(-1);
    model.focus_occupied_workspace(-1);
    assert_eq!(model.focused_workspace().unwrap().id, "8");
    // An empty workspace in view is a stop of its own.
    model.focus_workspace("6");
    model.focus_occupied_workspace(1);
    assert_eq!(model.focused_workspace().unwrap().id, "8");
    model.focus_workspace("6");
    model.focus_occupied_workspace(-1);
    assert_eq!(model.focused_workspace().unwrap().id, "4");
}

#[test]
fn the_wheel_walks_every_outputs_workspaces_by_number() {
    // The g815: 4 and 8 on the portable, the panel with one of its own.
    let mut model = two_outputs();
    let mut portable = output("B", 1920.0, 1600.0, 1000.0);
    portable.default_workspace = Some("4".to_owned());
    model.set_outputs(vec![
        output("A", 0.0, 1920.0, 1080.0),
        portable,
        output("C", 3520.0, 1280.0, 720.0),
    ]);
    let panel = model.active_workspace("C").unwrap().id.clone();
    for (id, ws) in [("a", "1"), ("b", "2"), ("c", "3"), ("d", "8")] {
        open(&mut model, id);
        model.move_to_workspace(id, ws, false);
    }
    model.focus_workspace("1");
    let mut seen = Vec::new();
    for _ in 0..6 {
        model.focus_occupied_workspace(1);
        let ws = model.focused_workspace().unwrap();
        seen.push(format!("{}@{}", ws.id, ws.output));
    }
    let panel = format!("{panel}@C");
    assert_eq!(seen, ["2@A", "3@A", "4@B", "8@B", &panel, "1@A"]);
    model.focus_occupied_workspace(-1);
    assert_eq!(
        model.focused_workspace().unwrap().id,
        model.active_workspace("C").unwrap().id
    );
    // An empty workspace the desk shows is a stop; one it does not is not.
    model.focus_workspace("6");
    model.focus_occupied_workspace(1);
    assert_eq!(model.focused_workspace().unwrap().id, "8");
    model.focus_occupied_workspace(-1);
    model.focus_occupied_workspace(-1);
    assert_eq!(model.focused_workspace().unwrap().id, "3");
}

#[test]
fn a_settings_reload_keeps_every_window_where_it_is() {
    let mut model = desk();
    open(&mut model, "a");
    model.focus_workspace("9");
    open(&mut model, "z");
    let before = model.layout();
    let specs: Vec<WorkspaceSpec> = (1..=7)
        .map(|n| WorkspaceSpec {
            name: format!("new{n}"),
            pin: None,
        })
        .collect();
    model.set_workspaces(&specs);
    assert_eq!(model.layout(), before);
    assert_eq!(model.workspace("1").unwrap().name, "new1");
    assert!(!model.workspace("9").unwrap().named);
    assert!(model.workspace("8").is_none(), "empty and no longer named");
    model.close("z");
    model.focus_workspace("1");
    assert!(model.workspace("9").is_none());
}

#[test]
fn a_dragged_tape_snaps_and_hands_the_focus_to_a_column_in_view() {
    let mut model = desk();
    for id in ["a", "b", "c", "d"] {
        open(&mut model, id);
    }
    model.focus_window("a");
    assert!(model.scroll_by("1", 700.0));
    assert_eq!(model.workspace("1").unwrap().tape.scroll, 700.0);
    model.scroll_release("1", 1000.0);
    assert_eq!(model.workspace("1").unwrap().tape.scroll, 956.0);
    assert_eq!(focused(&model), "b");
    assert!(!model.scroll_by("1", 99999.0) || model.workspace("1").unwrap().tape.scroll == 1912.0);
}

#[test]
fn a_lifted_window_drops_into_a_gap_or_goes_back() {
    let mut model = desk();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    let lifted = model.lift("c").unwrap();
    assert_eq!(model.workspace("1").unwrap().tape.columns.len(), 2);
    model.drop_column(&lifted, "1", 0);
    assert_eq!(place(&model, "c").column, 0);
    assert_eq!(focused(&model), "c");

    let lifted = model.lift("c").unwrap();
    model.drop_column(&lifted, "2", 0);
    assert_eq!(model.focused_workspace().unwrap().id, "2");
    assert_eq!(model.window("c").unwrap().home, Home::Workspace("2".into()));

    let lifted = model.lift("a").unwrap();
    model.drop_back(&lifted);
    assert_eq!(place(&model, "a").column, 0);
    assert!(model.lift("nope").is_none());
}

#[test]
fn closing_the_last_window_leaves_no_focus() {
    let mut model = desk();
    open(&mut model, "a");
    model.close("a");
    assert_eq!(focused(&model), "");
    assert!(model.layout().is_empty());
}

#[test]
fn a_zen_or_big_window_that_changes_workspace_leaves_no_mark_behind() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_workspace("2");
    open(&mut model, "c");
    model.focus_workspace("1");

    assert!(model.set_zen(Some("b"), Some(true)));
    assert!(model.swap("b", "c"));
    let one = model.workspace("1").unwrap();
    assert_eq!(one.zen, None);
    assert_eq!(focused(&model), "c", "the focus is a window that is here");
    assert_eq!(place(&model, "a").rect.x, 8.0, "the tape is back in view");

    assert!(model.set_zen(Some("c"), Some(true)));
    assert!(model.move_column("c", Some("b")));
    assert_eq!(model.workspace("1").unwrap().zen, None);
    assert_eq!(focused(&model), "a");

    assert!(model.set_big("a", Some(Big::Fullscreen)));
    assert!(model.move_column("a", Some("b")));
    assert_eq!(model.workspace("1").unwrap().big, None);
    model.close("a");
    assert_eq!(model.workspace("1").unwrap().focused(), None);
}

#[test]
fn one_window_moved_out_of_a_stack_leaves_the_rest_of_its_column() {
    let mut model = desk();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    assert!(model.consume_or_expel(Side::Prev));
    assert_eq!(
        (place(&model, "b").column, place(&model, "c").column),
        (1, 1)
    );

    assert!(model.move_window("c", "1", Some("a")));
    let columns = |model: &formalwm_core::model::Model| {
        ["a", "b", "c"].map(|id| (place(model, id).column, place(model, id).row))
    };
    assert_eq!(columns(&model), [(1, 0), (2, 0), (0, 0)]);
    assert_eq!(focused(&model), "c");

    assert!(model.consume_or_expel(Side::Next));
    assert_eq!(columns(&model), [(0, 0), (1, 0), (0, 1)]);
    assert!(model.move_window("c", "2", None));
    assert_eq!(model.window("c").unwrap().home, Home::Workspace("2".into()));
    assert_eq!(model.window("a").unwrap().home, Home::Workspace("1".into()));
    assert_eq!(model.focused_workspace().unwrap().id, "1", "the view stays");
    assert_eq!(model.workspace("2").unwrap().focused().unwrap(), "c");

    assert!(!model.move_window("a", "1", Some("a")));
    assert!(!model.move_window("a", "nope", None));
    assert!(!model.move_window("a", "1", Some("nope")));
    assert_eq!(columns(&model)[0], (0, 0), "a refused move changes nothing");
}

#[test]
fn an_overlay_shown_on_an_output_that_goes_is_parked() {
    let mut model = two_outputs();
    let rule = Placement {
        overlay: Some("formalshell-console".into()),
        floating: true,
        ..Placement::default()
    };
    open_with(&mut model, "console", rule);
    model.focus_workspace("4");
    assert!(model.show_overlay("console"));
    assert_eq!(place(&model, "console").output, "B");

    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0)]);
    let parked = place(&model, "console");
    assert_eq!((parked.shown, parked.output.as_str()), (false, "A"));
    assert!(!model.focus_window("console"));
    assert_eq!(model.focused_output, "A");
    assert!(model.show_overlay("console"));
    assert_eq!(focused(&model), "console");
}

#[test]
fn a_window_is_in_view_on_a_shown_workspace_or_a_shown_overlay() {
    let mut model = two_outputs();
    open(&mut model, "a");
    let rule = Placement {
        overlay: Some("formalshell-console".into()),
        floating: true,
        ..Placement::default()
    };
    open_with(&mut model, "console", rule);
    assert!(model.in_view("a"));
    assert!(!model.in_view("console"), "parked");
    assert!(!model.in_view("nope"));

    assert!(model.move_to_workspace("a", "5", false));
    assert!(!model.in_view("a"), "its workspace is not the one shown");
    assert_eq!(model.focused_window(), None);
    assert!(model.show_overlay("console"));
    assert!(model.in_view("console"));
    model.focus_workspace("5");
    assert!(model.in_view("a"));
}

#[test]
fn the_outer_gap_lies_inside_what_a_layer_reserves_at_the_side() {
    let mut model = desk();
    let side = |x: f64, w: f64| {
        let mut geo = output("A", 0.0, 1920.0, 1080.0);
        geo.usable = Rect::new(x, 0.0, w, 1080.0);
        vec![geo]
    };
    model.set_outputs(side(154.0, 1766.0));
    open(&mut model, "a");
    model.set_column_width(1.0);
    assert_eq!(
        place(&model, "a").rect,
        Rect::new(162.0, 8.0, 1750.0, 1064.0)
    );
    // A lone column narrower than the strip is centred in what is left.
    model.set_column_width(0.5);
    assert_eq!(
        place(&model, "a").rect,
        Rect::new(602.0, 8.0, 871.0, 1064.0)
    );

    model.set_outputs(side(0.0, 1766.0));
    assert_eq!(
        place(&model, "a").rect,
        Rect::new(448.0, 8.0, 871.0, 1064.0)
    );
    model.set_column_width(1.0);
    assert_eq!(place(&model, "a").rect, Rect::new(8.0, 8.0, 1750.0, 1064.0));
}

#[test]
fn gaps_are_measured_to_the_outer_edge_of_a_window_s_border() {
    for border in [1.0, 2.0] {
        let mut model = desk();
        model.set_options(Options {
            border,
            ..Options::default()
        });
        let mut geo = output("A", 0.0, 1920.0, 1080.0);
        geo.usable = Rect::new(154.0, 0.0, 1766.0, 1080.0);
        model.set_outputs(vec![geo]);
        open(&mut model, "a");
        open(&mut model, "b");
        let (a, b) = (place(&model, "a"), place(&model, "b"));
        // The outer gap, from what the layers leave to the border.
        assert_eq!(a.rect.x - border, 154.0 + 8.0);
        assert_eq!(a.rect.y - border, 8.0);
        assert_eq!(1080.0 - (a.rect.bottom() + border), 8.0);
        assert_eq!(1920.0 - (b.rect.right() + border), 8.0);
        // The inner gap, from border to border.
        assert_eq!(b.rect.x - a.rect.right(), 8.0 + 2.0 * border);

        assert!(model.consume_or_expel(Side::Prev));
        let (a, b) = (place(&model, "a"), place(&model, "b"));
        assert_eq!((a.column, b.column), (0, 0));
        assert_eq!(b.rect.y - a.rect.bottom(), 8.0 + 2.0 * border);
        assert_eq!(1080.0 - (b.rect.bottom() + border), 8.0);

        // A maximised window's border lies inside what the layers leave.
        assert!(model.toggle_big("b", Big::Maximized));
        assert_eq!(
            on_screen(&model, "b"),
            Rect::new(
                154.0 + border,
                border,
                1766.0 - 2.0 * border,
                1080.0 - 2.0 * border
            )
        );
    }
}

#[test]
fn the_pointer_takes_the_outputs_focus_and_leaves_the_keyboard() {
    let mut model = two_outputs();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_workspace("4");
    open(&mut model, "chat");
    assert!(model.focus_window("b"));
    assert_eq!((model.focused_output.as_str(), focused(&model)), ("A", "b"));

    // The pointer crosses to B: the output and its workspace are the
    // focused ones, the keyboard is still b's, and the tape's binds are
    // for the window typed into.
    assert!(model.focus_output("B"));
    assert!(!model.focus_output("B"));
    assert_eq!(model.focused_output, "B");
    assert_eq!(model.focused_workspace().unwrap().id, "4");
    assert_eq!(focused(&model), "b");
    assert!(model.focus_column(Side::Prev));
    assert_eq!((model.focused_output.as_str(), focused(&model)), ("B", "a"));

    // Back where the keyboard is, the two are one again.
    assert!(model.focus_output("A"));
    assert_eq!((model.focused_output.as_str(), focused(&model)), ("A", "a"));

    // A window opens where the pointer is and takes the keyboard there.
    model.focus_output("B");
    open(&mut model, "mail");
    assert_eq!(
        model.window("mail").unwrap().home,
        Home::Workspace("4".into())
    );
    assert_eq!(
        (model.focused_output.as_str(), focused(&model)),
        ("B", "mail")
    );

    // So does a click on a window, and a workspace shown.
    model.focus_output("A");
    assert_eq!(focused(&model), "mail");
    assert!(model.focus_window("a"));
    assert_eq!((model.focused_output.as_str(), focused(&model)), ("A", "a"));
    model.focus_output("B");
    assert!(model.focus_workspace("8"));
    assert_eq!(model.focused_window(), None);
    assert_eq!(model.focused_output, "B");

    // With no window typed into, the tape's binds are for the pointer's
    // output.
    model.focus_workspace("4");
    model.focus_workspace("2");
    assert_eq!(model.focused_window(), None);
    model.focus_output("B");
    assert_eq!(model.focused_window(), None);
    assert!(model.focus_column(Side::Prev));
    assert_eq!(focused(&model), "chat");

    // The output the keyboard was left on goes: it is the pointer's.
    model.focus_window("mail");
    model.focus_output("A");
    assert_eq!(focused(&model), "mail");
    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0)]);
    assert_eq!(model.focused_output, "A");
}

#[test]
fn a_workspace_in_view_under_the_pointer_takes_the_keyboard_when_asked_for() {
    let mut model = two_outputs();
    open(&mut model, "a");
    model.focus_workspace("4");
    open(&mut model, "chat");

    // The keyboard on a, the pointer on B, which shows 4. Asked for, 4
    // changes nothing that is shown and has the keyboard all the same,
    // which the caller has to hear of.
    let leave = |model: &mut formalwm_core::model::Model| {
        assert!(model.focus_window("a"));
        assert!(model.focus_output("B"));
        assert_eq!((model.focused_output.as_str(), focused(model)), ("B", "a"));
    };
    leave(&mut model);
    assert!(model.focus_workspace("4"));
    assert_eq!(focused(&model), "chat");
    assert!(!model.focus_workspace("4"));
    // By its place in the stack, and as the one before.
    leave(&mut model);
    assert!(model.focus_workspace_at(1, None));
    assert_eq!(focused(&model), "chat");
    model.focus_workspace("8");
    model.focus_workspace("4");
    model.focus_workspace("8");
    leave(&mut model);
    assert!(model.focus_previous_workspace());
    assert_eq!(model.focused_window(), None);

    // With nothing on it the keyboard is nobody's.
    leave(&mut model);
    assert_eq!(model.focused_workspace().unwrap().id, "8");
    assert!(model.focus_workspace("8"));
    assert_eq!(model.focused_window(), None);

    // A bind on the tape that moves nothing still takes the keyboard to
    // the pointer's output when no window was typed into.
    model.focus_workspace("4");
    model.focus_workspace("2");
    model.focus_output("B");
    assert_eq!(model.focused_window(), None);
    assert!(model.focus_column(Side::Prev));
    assert_eq!(focused(&model), "chat");
    assert!(!model.focus_column(Side::Prev));
}

#[test]
fn a_focused_float_leaves_the_column_binds_alone() {
    // Hyprland's `layout` messages act on the focused window's column, and
    // a float has none: focus, swap, widths, consume and fit do nothing.
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    open_with(
        &mut model,
        "f",
        Placement {
            floating: true,
            ..Placement::default()
        },
    );
    let tape = model.workspace("1").unwrap().tape.clone();
    assert!(!model.focus_column(Side::Prev));
    assert!(!model.focus_column(Side::Next));
    assert!(!model.swap_column(Side::Prev));
    assert!(!model.cycle_column_width());
    assert!(!model.set_column_width(1.0));
    assert!(!model.resize_column(0.1));
    assert!(!model.consume_or_expel(Side::Prev));
    assert!(!model.fit_into_view());
    assert_eq!(focused(&model), "f");
    assert_eq!(model.workspace("1").unwrap().tape, tape);
    // The row binds are Hyprland's `movefocus`, which does leave a float.
    assert!(model.focus_row(Side::Prev));
    assert_eq!(focused(&model), "b");
}

#[test]
fn a_float_tiled_again_goes_to_the_end_of_the_tape() {
    let mut model = desk();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    model.focus_window("a");
    assert!(model.set_floating("a", Some(true)));
    model.focus_window("b");
    model.focus_window("a");
    assert!(model.set_floating("a", Some(false)));
    assert_eq!(place(&model, "a").column, 2, "the float had the focus");
    assert!(model.set_floating("c", Some(true)));
    model.focus_window("b");
    assert!(model.set_floating("c", Some(false)));
    assert_eq!(place(&model, "c").column, 1, "right of b, the focused tile");
    assert_eq!(place(&model, "a").column, 2);
}

#[test]
fn a_window_opens_right_of_a_focused_tile_and_at_the_end_else() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.focus_window("a");
    open(&mut model, "c");
    let column = |model: &formalwm_core::model::Model, id: &str| place(model, id).column;
    assert_eq!(column(&model, "c"), 1, "right of a, the focused tile");
    open_with(
        &mut model,
        "f",
        Placement {
            floating: true,
            ..Placement::default()
        },
    );
    model.focus_window("c");
    model.focus_window("f");
    open(&mut model, "d");
    assert_eq!(column(&model, "d"), 3, "a float has the focus: at the end");
    model.focus_window("a");
    open_with(
        &mut model,
        "e",
        Placement {
            workspace: Some("2".into()),
            ..Placement::default()
        },
    );
    open(&mut model, "g");
    model.focus_window("e");
    model.focus_window("a");
    assert!(model.move_to_workspace("a", "2", false));
    assert_eq!(column(&model, "a"), 2, "sent to a workspace: its end");
}

#[test]
fn a_float_sent_to_a_smaller_output_is_fitted_into_it() {
    // B is 1600x1000 to A's 1920x1080. Hyprland keeps a float's place on
    // the output and pushes it back inside the work area.
    let mut model = two_outputs();
    open_with(
        &mut model,
        "f",
        Placement {
            floating: true,
            float_rect: Some(Rect::new(1500.0, 900.0, 400.0, 150.0)),
            ..Placement::default()
        },
    );
    assert!(model.move_to_workspace("f", "4", false));
    assert_eq!(
        on_screen(&model, "f"),
        Rect::new(1920.0 + 1200.0, 850.0, 400.0, 150.0)
    );
    open_with(
        &mut model,
        "g",
        Placement {
            floating: true,
            float_rect: Some(Rect::new(100.0, 100.0, 400.0, 150.0)),
            ..Placement::default()
        },
    );
    assert!(model.move_to_workspace("g", "4", false));
    assert_eq!(
        on_screen(&model, "g"),
        Rect::new(1920.0 + 100.0, 100.0, 400.0, 150.0)
    );
}
