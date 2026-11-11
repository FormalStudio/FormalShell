mod common;

use common::*;
use formalwm_core::{
    model::{Big, Placement},
    view::{Extras, state},
};

#[test]
fn the_state_the_socket_publishes() {
    let mut model = two_outputs();
    for id in ["a", "b", "c"] {
        open(&mut model, id);
    }
    model.focus_workspace("8");
    open(&mut model, "music");
    let console = Placement {
        overlay: Some("formalshell-console".into()),
        ..Placement::default()
    };
    open_with(&mut model, "console", console);
    let extras = Extras {
        keyboard_layouts: vec!["es".into()],
        active_border: "#4385be".into(),
        blur: true,
        ..Extras::default()
    };
    let state = state(&model, &extras);

    assert_eq!(state.version, 1);
    assert_eq!(state.outputs.len(), 2);
    assert_eq!((state.outputs[1].x, state.outputs[1].width), (1920, 1600));
    assert_eq!(state.focused_output, "B");
    assert_eq!(state.focused_workspace_id, "8");
    assert_eq!(state.focused_window_id, "music");
    assert_eq!(state.chrome.gaps_out.left, 8);
    assert_eq!(state.keyboard.layouts, ["es"]);

    let ws = |id: &str| state.workspaces.iter().find(|ws| ws.id == id).unwrap();
    assert_eq!(state.workspaces.len(), 9);
    assert_eq!(
        (ws("1").idx, ws("5").idx, ws("4").idx, ws("8").idx),
        (1, 4, 1, 2)
    );
    assert!(ws("1").active && !ws("1").focused);
    assert!(ws("8").active && ws("8").focused);
    assert!(!ws("4").active);
    assert_eq!(ws("1").name, "ws1");
    assert!(ws("1").columns);

    let ids: Vec<&str> = state.windows.iter().map(|w| w.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "c", "music", "console"], "first-seen order");
    let win = |id: &str| state.windows.iter().find(|w| w.id == id).unwrap();
    assert_eq!((win("c").column, win("c").row), (2, 0));
    assert_eq!(win("c").rect.unwrap().x, 964, "the scroll is in the rect");
    assert_eq!(win("a").rect.unwrap().x, 8 - 956);
    assert_eq!(win("a").app_id, "app.a");
    assert_eq!(win("a").initial_title, "A");
    assert!(win("music").focused && !win("c").focused);
    assert_eq!(win("console").workspace_id, "");
    assert_eq!(
        win("console").overlay.as_deref(),
        Some("formalshell-console")
    );
    assert!(!win("console").shown && win("console").floating);
    assert!(
        state
            .windows
            .iter()
            .filter(|w| w.overlay.is_none())
            .all(|w| w.shown)
    );
    assert_eq!(win("a").overlay, None);
}

#[test]
fn zen_fullscreen_urgency_and_the_shown_overlay() {
    let mut model = desk();
    open(&mut model, "a");
    open(&mut model, "b");
    model.set_urgent("a", true);
    model.toggle_big("b", Big::Fullscreen);
    let console = Placement {
        overlay: Some("formalshell-console".into()),
        ..Placement::default()
    };
    open_with(&mut model, "console", console);
    model.show_overlay("console");
    let published = state(&model, &Extras::default());
    let win = |id: &str| published.windows.iter().find(|w| w.id == id).unwrap();
    assert!(win("b").fullscreen);
    assert!(win("a").urgent && published.workspaces[0].urgent);
    assert!(win("console").shown && win("console").focused);
    assert_eq!(published.focused_window_id, "console");

    model.park("console", "formalshell-console");
    model.set_zen(Some("a"), Some(true));
    let published = state(&model, &Extras::default());
    assert_eq!(published.workspaces[0].zen.as_deref(), Some("a"));
    assert!(
        !published.windows.iter().any(|w| w.fullscreen),
        "zen ends fullscreen"
    );
}

#[test]
fn the_same_model_publishes_the_same_json() {
    let mut model = desk();
    open(&mut model, "a");
    let first = serde_json::to_string(&state(&model, &Extras::default())).unwrap();
    let second = serde_json::to_string(&state(&model, &Extras::default())).unwrap();
    assert_eq!(first, second);
    assert!(first.contains("\"focusedWindowId\":\"a\""));
}
