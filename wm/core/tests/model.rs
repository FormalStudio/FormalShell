mod common;

use common::*;
use formalwm_core::{
    geom::Rect,
    model::{Big, Home, Layer, Placement, WorkspaceSpec},
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
    assert_eq!(b.rect, Rect::new(480.0, 216.0, 960.0, 648.0));
    assert_eq!(focused(&model), "b");
    assert!(model.place("b", Rect::new(100.0, 120.0, 640.0, 480.0)));
    assert!(model.set_floating("b", Some(false)));
    assert_eq!(place(&model, "b").layer, Layer::Tile);
    assert!(model.set_floating("b", Some(true)));
    assert_eq!(
        place(&model, "b").rect,
        Rect::new(100.0, 120.0, 640.0, 480.0)
    );
    assert!(
        !model.place("a", Rect::new(0.0, 0.0, 10.0, 10.0)),
        "a is tiled"
    );
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
        (b.rect, b.layer),
        (Rect::new(0.0, 0.0, 1920.0, 1080.0), Layer::Fullscreen)
    );
    assert!(!b.scrolls);
    assert!(model.toggle_big("b", Big::Maximized));
    assert_eq!(model.fullscreen_on("A"), None);
    assert_eq!(
        place(&model, "b").rect,
        Rect::new(0.0, 40.0, 1920.0, 1040.0)
    );
    assert!(model.toggle_big("b", Big::Maximized));
    assert_eq!(place(&model, "b").layer, Layer::Tile);
    model.toggle_big("b", Big::Fullscreen);
    model.focus_column(Side::Prev);
    assert_eq!(model.big_of("b"), None, "focusing another column leaves it");
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
