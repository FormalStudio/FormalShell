// Covers toggles.rs plus the drift guards that read shipped files rather than
// fixtures: the "@state:" allow-list only means anything if the condition
// evaluator's snapshot literal spells the same paths and the shipped toggle
// subtree names paths that exist.

mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::model::build_tree;
use fs_menu::node::{Entries, Kind, Node};
use fs_menu::toggles::{
    ENUM_PATHS, LIST_PATHS, PATHS, SnapValue, checked_for, is_known_list_path, is_known_path, is_state_condition,
    resolve_state, snapshot, state_path, unknown_keys, with_result,
};
use serde_json::{Value, json};

fn snap(live: Value) -> fs_menu::toggles::Snapshot {
    snapshot(Some(&live))
}

#[test]
fn prefix_detection() {
    assert!(is_state_condition("@state:nightlight.active"));
    assert!(!is_state_condition("command -v wlsunset >/dev/null 2>&1"));
    assert_eq!(state_path("@state:nightlight.active"), Some("nightlight.active"));
    assert_eq!(state_path("command -v wlsunset >/dev/null 2>&1"), None);
}

// Membership check, never an object lookup: a hostile menu.jsonc cannot reach
// the engine or a prototype member through this field.
#[test]
fn allow_list_rejects_unknown_and_prototype_paths() {
    let empty = snapshot(Some(&json!({})));
    for cond in ["@state:Qt.quit", "@state:constructor", "@state:__proto__", "@state:hasOwnProperty", "@state:"] {
        assert_eq!(resolve_state(cond, Some(&empty)), Some(false), "{cond}");
    }
}

#[test]
fn resolve_reads_the_snapshot() {
    let cond = "@state:notifications.dnd";
    assert_eq!(resolve_state(cond, Some(&snap(json!({ "notifications.dnd": true })))), Some(true));
    assert_eq!(resolve_state(cond, Some(&snap(json!({ "notifications.dnd": false })))), Some(false));
    // An unpopulated snapshot renders "off", never "unknown".
    assert_eq!(resolve_state(cond, Some(&snap(json!({})))), Some(false));
    assert_eq!(resolve_state(cond, None), Some(false));
}

// The tri-state that keeps existing shell-command `checked` fields resolving
// from the caller's own process cache.
#[test]
fn non_state_condition_is_none() {
    let empty = snap(json!({}));
    assert_eq!(resolve_state("true", Some(&empty)), None);
    assert_eq!(resolve_state("test -n \"$HYPRLAND_INSTANCE_SIGNATURE\"", Some(&empty)), None);
}

fn checked_node(id: &str, checked: Option<&str>) -> Node {
    Node { checked: checked.map(str::to_string), ..Node::new(id, "", Kind::Action) }
}

#[test]
fn checked_for_prefers_live_state_over_the_cache() {
    let node = checked_node("toggles.dnd", Some("@state:notifications.dnd"));
    let cache = HashMap::from([("toggles.dnd".to_string(), true)]);
    assert!(!checked_for(Some(&node), Some(&snap(json!({ "notifications.dnd": false }))), Some(&cache)));
    let cache = HashMap::from([("toggles.dnd".to_string(), false)]);
    assert!(checked_for(Some(&node), Some(&snap(json!({ "notifications.dnd": true }))), Some(&cache)));
}

#[test]
fn checked_for_falls_back_to_the_shell_cache() {
    let node = checked_node("x", Some("test -f /nope"));
    let empty = snap(json!({}));
    assert!(checked_for(Some(&node), Some(&empty), Some(&HashMap::from([("x".to_string(), true)]))));
    assert!(!checked_for(Some(&node), Some(&empty), Some(&HashMap::new())));
    assert!(!checked_for(Some(&node), Some(&empty), None));
}

#[test]
fn checked_for_without_a_checked_field() {
    let cache = HashMap::from([("y".to_string(), true)]);
    assert!(!checked_for(Some(&checked_node("y", None)), Some(&snap(json!({ "theme.dark": true }))), Some(&cache)));
    assert!(!checked_for(None, None, None));
}

#[test]
fn snapshot_normalizes_to_the_allow_list() {
    let s = snap(json!({ "nightlight.active": true, "bogus": true }));
    assert_eq!(s.len(), PATHS.len() + ENUM_PATHS.len() + LIST_PATHS.len());
    for path in PATHS {
        assert!(s.contains_key(*path));
    }
    assert!(!s.contains_key("bogus"));
    assert_eq!(s["nightlight.active"], SnapValue::Flag(true));
    assert_eq!(s["caffeinate.active"], SnapValue::Flag(false));
    assert_eq!(s["notifications.dnd"], SnapValue::Flag(false));
    assert_eq!(s["theme.dark"], SnapValue::Flag(false));
    // Strict `true` only, so a truthy non-boolean reads off.
    assert_eq!(snap(json!({ "theme.dark": 1 }))["theme.dark"], SnapValue::Flag(false));
    assert_eq!(snapshot(None)["theme.dark"], SnapValue::Flag(false));
    assert_eq!(unknown_keys(&json!({ "bogus": 1 })), ["bogus"]);
    assert!(unknown_keys(&json!({ "theme.dark": true })).is_empty());
}

// The fresh-value contract the caller's change detection depends on.
#[test]
fn with_result_returns_a_fresh_map() {
    let source = HashMap::from([("a".to_string(), true)]);
    let out = with_result(&source, "b", false);
    assert!(out["a"]);
    assert!(!out["b"]);
    assert_eq!(source.len(), 1);
    assert!(!source.contains_key("b"));
}

// Breaks loudly if a path is added without updating the caller and the docs.
#[test]
fn allow_list_is_exactly_the_documented_paths() {
    assert_eq!(PATHS.len(), 8);
    for path in ["lights.on", "nightlight.active", "overnight.active", "caffeinate.active", "notifications.dnd", "theme.dark", "hdr.active", "ipad.active"] {
        assert!(is_known_path(path), "{path}");
    }
    assert!(!is_known_path("bluetooth.powered"));
}

#[test]
fn enum_condition_matches_only_its_own_value() {
    let s = snap(json!({ "lights.effect": "breathe", "lights.colour": "" }));
    assert_eq!(resolve_state("@state:lights.effect=breathe", Some(&s)), Some(true));
    assert_eq!(resolve_state("@state:lights.effect=static", Some(&s)), Some(false));
    // An empty value never matches, which is what leaves every colour preset
    // unchecked while the source is the wallpaper.
    assert_eq!(resolve_state("@state:lights.colour=", Some(&s)), Some(false));
    assert_eq!(resolve_state("@state:lights.bogus=breathe", Some(&s)), Some(false));
    // A flag path read as an enum answers false rather than comparing a boolean.
    assert_eq!(resolve_state("@state:lights.on=true", Some(&snap(json!({ "lights.on": true })))), Some(false));
    // A non-string enum value normalizes to "".
    assert_eq!(snap(json!({ "lights.brightness": 3 }))["lights.brightness"], SnapValue::Text(String::new()));
}

#[test]
fn list_condition_tests_membership() {
    let s = snap(json!({ "bluetooth.connected": ["AA:BB", "CC:DD"] }));
    assert_eq!(resolve_state("@state:bluetooth.connected=AA:BB", Some(&s)), Some(true));
    assert_eq!(resolve_state("@state:bluetooth.connected=EE:FF", Some(&s)), Some(false));
    assert_eq!(resolve_state("@state:bluetooth.connected=", Some(&s)), Some(false));
    assert!(is_known_list_path("bluetooth.connected"));
    assert!(!is_known_list_path("lights.effect"));
}

#[test]
fn list_snapshot_normalizes() {
    let list = |v: Value| snap(v)["bluetooth.connected"].clone();
    assert_eq!(list(json!({ "bluetooth.connected": "AA:BB" })), SnapValue::List(vec![]));
    assert_eq!(list(json!({})), SnapValue::List(vec![]));
    assert_eq!(
        list(json!({ "bluetooth.connected": ["a", 3, null, "b"] })),
        SnapValue::List(vec!["a".into(), "b".into()])
    );
}

#[test]
fn device_enum_paths_match_their_value() {
    let s = snap(json!({ "wifi.ssid": "Home", "audio.sink": "alsa.out", "audio.source": "alsa.in", "radio.station": "u-1" }));
    assert_eq!(resolve_state("@state:wifi.ssid=Home", Some(&s)), Some(true));
    assert_eq!(resolve_state("@state:wifi.ssid=Other", Some(&s)), Some(false));
    assert_eq!(resolve_state("@state:audio.sink=alsa.out", Some(&s)), Some(true));
    assert_eq!(resolve_state("@state:audio.source=alsa.in", Some(&s)), Some(true));
    assert_eq!(resolve_state("@state:radio.station=u-1", Some(&s)), Some(true));
}

#[test]
fn unknown_path_with_value_is_false() {
    assert_eq!(resolve_state("@state:nope.path=x", Some(&snap(json!({ "nope.path": "x" })))), Some(false));
    assert!(unknown_keys(&json!({ "wifi.ssid": "", "audio.sink": "", "audio.source": "", "radio.station": "", "bluetooth.connected": [] })).is_empty());
}

// Contract test against the real shipped tree, not a fixture: this is what
// catches a mistyped @state: path or a missing keepOpen in default-menu.jsonc.
#[test]
fn shipped_toggle_subtree_contract() {
    let tree = build_tree(&default_menu(), &Entries::new());
    let toggle_ids = [
        "toggles.nightlight",
        "toggles.overnight",
        "toggles.caffeinate",
        "toggles.hdr",
        "toggles.dnd",
        "toggles.dark-mode",
    ];
    for id in toggle_ids {
        let node = tree.nodes.get(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(node.parent_id.as_deref(), Some("toggles"));
        assert_eq!(node.kind, Kind::Action);
        assert!(node.action.as_deref().unwrap().starts_with("@ipc:"));
        assert_eq!(node.keep_open, Some(true));
        let checked = node.checked.as_deref().unwrap();
        assert!(is_state_condition(checked));
        assert!(is_known_path(state_path(checked).unwrap()));
    }
    assert_eq!(tree.nodes["toggles"].child_ids.len(), 6);
    // Dark mode relocated into the hub; the old theme subtree is gone.
    assert!(!tree.nodes.contains_key("theme.mode-toggle"));
    assert!(!tree.nodes.contains_key("theme"));
}
