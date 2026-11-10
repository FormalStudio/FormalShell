// Covers toggles.rs plus the drift guard that reads the shipped menu rather
// than a fixture: a `@state:` condition only means anything if the shipped
// toggle subtree names paths the snapshot carries.

mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::model::build_tree;
use fs_menu::node::{Entries, Kind, Node};
use fs_menu::toggles::{Checked, Snapshot, checked_for};

fn resolve(cond: &str, snap: &Snapshot) -> Option<bool> {
    Checked::parse(cond).resolve(Some(snap))
}

#[test]
fn a_condition_is_read_once() {
    assert_eq!(Checked::parse("@state:nightlight.active"), Checked::flag("nightlight.active"));
    assert_eq!(Checked::parse("@state:wifi.ssid=Home:5G"), Checked::value("wifi.ssid", "Home:5G"));
    let line = "command -v wlsunset >/dev/null 2>&1";
    assert_eq!(Checked::parse(line), Checked::Shell(line.into()));
}

// A path the snapshot has no field for answers false: a hostile menu.jsonc
// reaches nothing through this field.
#[test]
fn unknown_paths_are_off() {
    let empty = Snapshot::default();
    for cond in ["@state:Qt.quit", "@state:constructor", "@state:__proto__", "@state:hasOwnProperty", "@state:", "@state:bluetooth.powered"] {
        assert_eq!(resolve(cond, &empty), Some(false), "{cond}");
    }
}

#[test]
fn resolve_reads_the_snapshot() {
    let cond = "@state:notifications.dnd";
    assert_eq!(resolve(cond, &Snapshot { dnd: true, ..Snapshot::default() }), Some(true));
    assert_eq!(resolve(cond, &Snapshot::default()), Some(false));
    // No snapshot renders "off", never "unknown".
    assert_eq!(Checked::parse(cond).resolve(None), Some(false));
    assert_eq!(Checked::Always.resolve(None), Some(true));
}

// The tri-state that keeps shell-command `checked` fields resolving from the
// caller's own process cache.
#[test]
fn a_shell_condition_is_none() {
    let empty = Snapshot::default();
    assert_eq!(resolve("true", &empty), None);
    assert_eq!(resolve("test -n \"$HYPRLAND_INSTANCE_SIGNATURE\"", &empty), None);
}

fn checked_node(id: &str, checked: Option<&str>) -> Node {
    Node { checked: checked.map(Checked::parse), ..Node::new(id, "", Kind::Action) }
}

#[test]
fn checked_for_prefers_live_state_over_the_cache() {
    let node = checked_node("toggles.dnd", Some("@state:notifications.dnd"));
    let cache = HashMap::from([("toggles.dnd".to_string(), true)]);
    assert!(!checked_for(Some(&node), Some(&Snapshot::default()), Some(&cache)));
    let cache = HashMap::from([("toggles.dnd".to_string(), false)]);
    assert!(checked_for(Some(&node), Some(&Snapshot { dnd: true, ..Snapshot::default() }), Some(&cache)));
}

#[test]
fn checked_for_falls_back_to_the_shell_cache() {
    let node = checked_node("x", Some("test -f /nope"));
    let empty = Snapshot::default();
    assert!(checked_for(Some(&node), Some(&empty), Some(&HashMap::from([("x".to_string(), true)]))));
    assert!(!checked_for(Some(&node), Some(&empty), Some(&HashMap::new())));
    assert!(!checked_for(Some(&node), Some(&empty), None));
}

#[test]
fn checked_for_without_a_checked_field() {
    let cache = HashMap::from([("y".to_string(), true)]);
    assert!(!checked_for(Some(&checked_node("y", None)), Some(&Snapshot { dark: true, ..Snapshot::default() }), Some(&cache)));
    assert!(!checked_for(None, None, None));
}

#[test]
fn every_flag_reads_its_own_field() {
    let on = Snapshot {
        nightlight: true,
        overnight: true,
        caffeinate: true,
        dnd: true,
        dark: true,
        lights_on: true,
        hdr: true,
        ipad_active: true,
        ..Snapshot::default()
    };
    for path in ["lights.on", "nightlight.active", "overnight.active", "caffeinate.active", "notifications.dnd", "theme.dark", "hdr.active", "ipad.active"] {
        assert_eq!(on.flag(path), Some(true), "{path}");
        assert_eq!(Snapshot::default().flag(path), Some(false), "{path}");
    }
}

#[test]
fn a_value_condition_matches_only_its_own_value() {
    let s = Snapshot { lights_effect: "breathe".into(), lights_on: true, ..Snapshot::default() };
    assert_eq!(resolve("@state:lights.effect=breathe", &s), Some(true));
    assert_eq!(resolve("@state:lights.effect=static", &s), Some(false));
    // An empty value never matches, which is what leaves every colour preset
    // unchecked while the source is the wallpaper.
    assert_eq!(resolve("@state:lights.colour=", &s), Some(false));
    assert_eq!(resolve("@state:lights.bogus=breathe", &s), Some(false));
    // A flag path read as a value answers false rather than comparing a boolean.
    assert_eq!(resolve("@state:lights.on=true", &s), Some(false));
}

#[test]
fn a_list_condition_tests_membership() {
    let s = Snapshot { bluetooth_connected: vec!["AA:BB".into(), "CC:DD".into()], ..Snapshot::default() };
    assert_eq!(resolve("@state:bluetooth.connected=AA:BB", &s), Some(true));
    assert_eq!(resolve("@state:bluetooth.connected=EE:FF", &s), Some(false));
    assert_eq!(resolve("@state:bluetooth.connected=", &s), Some(false));
}

#[test]
fn device_values_match_their_value() {
    let s = Snapshot { wifi_ssid: "Home".into(), audio_sink: "alsa.out".into(), audio_source: "alsa.in".into(), radio_station: "u-1".into(), ..Snapshot::default() };
    assert_eq!(resolve("@state:wifi.ssid=Home", &s), Some(true));
    assert_eq!(resolve("@state:wifi.ssid=Other", &s), Some(false));
    assert_eq!(resolve("@state:audio.sink=alsa.out", &s), Some(true));
    assert_eq!(resolve("@state:audio.source=alsa.in", &s), Some(true));
    assert_eq!(resolve("@state:radio.station=u-1", &s), Some(true));
    assert_eq!(resolve("@state:nope.path=x", &s), Some(false));
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
        assert!(node.action.as_ref().map(ToString::to_string).as_deref().unwrap().starts_with("@ipc:"));
        assert_eq!(node.keep_open, Some(true));
        let Some(Checked::State { path, value: None }) = &node.checked else { panic!("{id}: {:?}", node.checked) };
        assert!(Snapshot::default().flag(path).is_some(), "{id}: {path}");
    }
    assert_eq!(tree.nodes["toggles"].child_ids.len(), 6);
    // Dark mode relocated into the hub; the old theme subtree is gone.
    assert!(!tree.nodes.contains_key("theme.mode-toggle"));
    assert!(!tree.nodes.contains_key("theme"));
}
