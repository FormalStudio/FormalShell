// The launcher-reachability guard: every panel the shell can open
// (fs-chrome's registry) is a Panels row, a page the launcher holds, or the
// tray route, and every Panels row is in the shipped tree and opens its panel.

mod common;

use std::collections::{HashMap, HashSet};

use common::*;
use fs_menu::node::Kind;
use fs_menu::providers::{
    ProviderFn, RadioState, RadioStation, TrayItem, apply_providers, capture_entries, panel_ids, panels_provider,
    radio_result_rows, radio_rows, tray_provider,
};
use fs_menu::search;

// Settings and Information are pages the launcher holds as routes of their
// own, not rows under Panels.
const LAUNCHER_PAGES: &[(&str, &str)] = &[("settings", "Settings"), ("info", "Information")];

// The headline guard: every panel name has a "panels.<name>" row whose action
// actually opens it, and the submenu has no extra/stale rows.
#[test]
fn shipped_tree_covers_every_panel() {
    let tree = real_tree();
    let panels = tree.nodes.get("panels").expect("panels node");
    let names: Vec<&str> = panel_ids().collect();
    assert_eq!(panels.child_ids, names.iter().map(|n| format!("panels.{n}")).collect::<Vec<_>>());
    for name in names {
        let node = tree.nodes.get(&format!("panels.{name}")).unwrap_or_else(|| panic!("missing launcher route for panel '{name}'"));
        assert_eq!(node.kind, Kind::Action);
        let action = node.action.as_deref().expect("action");
        assert!(action.contains(&format!("call panel open {name}")), "panels.{name} does not open panel '{name}'");
    }
}

#[test]
fn every_panel_reaches_the_launcher() {
    use fs_chrome::bar::panels::{PANELS, TRAY_OVERFLOW, known, launcher_route};
    let rows: Vec<&str> = panel_ids().collect();
    for name in PANELS {
        let listed = rows.contains(&name);
        let elsewhere = launcher_route(name).is_some() || name == TRAY_OVERFLOW;
        assert!(listed != elsewhere, "panel '{name}': a Panels row {listed}, held elsewhere {elsewhere}");
    }
    for row in &rows {
        assert!(known(row).is_some(), "Panels row '{row}' opens no panel");
    }
}

// Each page the launcher holds is a root route with its own provider, reached
// by typing its name at root, and never also a Panels row.
#[test]
fn launcher_pages_are_root_routes() {
    let tree = real_tree();
    for (id, label) in LAUNCHER_PAGES {
        let node = tree.nodes.get(*id).unwrap_or_else(|| panic!("missing launcher route '{id}'"));
        assert_eq!(node.provider.as_deref(), Some(*id), "{id} is not held by its own provider");
        assert!(!tree.nodes.contains_key(&format!("panels.{id}")), "{id} is still a Panels row");
        let ranked = search::rank(&tree.nodes, label, &no_conds(), None);
        assert!(ids(&ranked).contains(id), "root query '{label}' does not reach {id}");
    }
}

// A panel is reached by typing its name at root, not only from inside the
// Panels route.
#[test]
fn root_query_reaches_every_panel() {
    let tree = real_tree();
    for row in panels_provider("formalshell-ipc") {
        let ranked = search::rank(&tree.nodes, &row.label, &no_conds(), None);
        assert!(ids(&ranked).contains(&row.id.as_str()), "root query '{}' does not reach {}", row.label, row.id);
    }
}

#[test]
fn panels_provider_action_shape() {
    let rows = panels_provider("formalshell-ipc");
    let names: Vec<&str> = panel_ids().collect();
    assert_eq!(rows.len(), names.len());
    for (row, name) in rows.iter().zip(names) {
        assert_eq!(row.id, format!("panels.{name}"));
        assert_eq!(row.kind, Kind::Action);
        assert_eq!(row.action.as_deref(), Some(format!("formalshell-ipc call panel open {name}").as_str()));
    }
}

// Tray: an item row calls tray activate <id> self-targeted the same way, and
// an empty tray renders one dim note rather than nothing.
#[test]
fn tray_provider_empty_state() {
    let rows = tray_provider(&[], "formalshell-ipc");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, Kind::Note);
    assert_eq!(rows[0].dim, Some(true));
}

#[test]
fn tray_provider_rows_activate_by_id() {
    let item = TrayItem { id: "spotify".into(), title: "Spotify".into(), tooltip_title: String::new() };
    let rows = tray_provider(&[item], "formalshell-ipc");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "tray.spotify");
    assert_eq!(rows[0].label, "Spotify");
    // The id is shell-quoted: tray ids are opaque strings that can carry
    // spaces and quotes, so the provider must never interpolate one bare.
    assert_eq!(rows[0].action.as_deref(), Some("formalshell-ipc call tray activate 'spotify'"));
}

// The rest of the sweep (console, plain screenshots, screensaver, plugins,
// notification bulk actions, retheme/explicit mode) all injected the same
// self-targeted way capture_entries already injects "capture". One assertion
// per IPC target/function actually referenced.
#[test]
fn capture_entries_covers_the_rest_of_the_sweep() {
    let call = "formalshell-ipc call ";
    let entries = capture_entries("formalshell-ipc");
    let action = |id: &str| entries[id].action.clone().unwrap_or_default();
    assert_eq!(action("capture.screenshot"), format!("{call}screenshot full"));
    assert_eq!(action("capture.region"), format!("{call}screenshot region"));
    assert_eq!(action("system.console"), format!("{call}console toggle"));
    assert_eq!(action("system.screensaver"), format!("{call}screensaver start"));
    assert_eq!(action("system.plugins.list"), format!("{call}plugins list"));
    assert_eq!(action("system.plugins.reload"), format!("{call}plugins reload"));
    assert_eq!(action("notifications.clear"), format!("{call}notifications clear"));
    assert_eq!(action("notifications.markAllSeen"), format!("{call}notifications markAllSeen"));
    assert_eq!(action("notifications.dismissAll"), format!("{call}notifications dismissAll"));
    assert_eq!(action("theme.retheme"), format!("{call}theme retheme"));
    assert_eq!(action("theme.mode-dark"), format!("{call}theme mode dark"));
    assert_eq!(action("theme.mode-light"), format!("{call}theme mode light"));
}

// In-process through logind's Lock, not a spawned `loginctl lock-session`:
// a child of a shell running under the user manager is in no session, and
// logind refuses its empty session name.
#[test]
fn system_lock_asks_logind_in_process() {
    let node = &real_tree().nodes["system.lock"];
    assert_eq!(node.when, None);
    assert_eq!(node.action.as_deref(), Some("@ipc:lock.session"));
}

#[test]
fn wifi_is_a_root_provider_node() {
    let tree = real_tree();
    let node = &tree.nodes["wifi"];
    assert_eq!(node.kind, Kind::Provider);
    assert_eq!(node.provider.as_deref(), Some("wifi"));
    assert_eq!(node.parent_id, None);
}

fn device_tree() -> fs_menu::node::Tree {
    let mut tree = real_tree();
    let mut fns: HashMap<String, ProviderFn> = HashMap::new();
    fns.insert(
        "radio".into(),
        Box::new(|| {
            let favorite = RadioStation { uuid: "u-1".into(), name: "Groove Salad".into(), ..RadioStation::default() };
            let result = RadioStation { uuid: "u-2".into(), name: "Zebra Jazz".into(), ..RadioStation::default() };
            let mut rows = radio_rows(&RadioState { running: false, favorites: vec![favorite] });
            rows.extend(radio_result_rows(&[result], &HashSet::new()));
            rows
        }),
    );
    apply_providers(&mut tree, &fns);
    tree
}

#[test]
fn device_routes_exist_at_root() {
    let tree = device_tree();
    for (id, provider) in [("wifi", "wifi"), ("bluetooth", "bluetooth"), ("audio", "audio"), ("radio", "radio")] {
        let node = tree.nodes.get(id).unwrap_or_else(|| panic!("missing route {id}"));
        assert_eq!(node.provider.as_deref(), Some(provider));
        assert_eq!(node.parent_id, None);
    }
    assert_eq!(tree.nodes["radio.search"].provider.as_deref(), Some("radioSearch"));
    assert_eq!(tree.nodes["radio.search"].parent_id.as_deref(), Some("radio"));
}

#[test]
fn root_query_finds_a_favorite_but_not_a_search_result() {
    let tree = device_tree();
    let fav = search::rank(&tree.nodes, "Groove Salad", &no_conds(), None);
    assert!(ids(&fav).contains(&"radio.fav.u-1"));
    let res = search::rank(&tree.nodes, "Zebra Jazz", &no_conds(), None);
    assert!(!ids(&res).contains(&"radio.result.u-2"));
}
