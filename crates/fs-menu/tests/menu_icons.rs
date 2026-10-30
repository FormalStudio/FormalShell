// The map from the shipped route ids onto icon names. The drift guard reads the
// shipped default-menu.jsonc rather than a fixture: a route added without a
// mapping renders with no icon at all. Resolving the names in the Lucide and
// Nerd sets belongs to the theme icon table's port.

mod common;

use common::*;
use fs_menu::icons::{ROUTE_ICONS, ROUTE_ICON_PREFIXES, fallback_for, icon_for};
use fs_menu::model::build_tree;
use fs_menu::node::{Entries, Kind, Node};

fn id(id: &str) -> Node {
    Node { id: id.into(), ..Node::default() }
}

#[test]
fn every_route_name_is_non_empty_and_unique_by_id() {
    let mut seen = std::collections::HashSet::new();
    for (route, name) in ROUTE_ICONS {
        assert!(!name.is_empty(), "{route}");
        assert!(seen.insert(*route), "{route} is mapped twice");
    }
    assert!(!ROUTE_ICON_PREFIXES.is_empty());
}

#[test]
fn a_device_row_resolves_by_prefix() {
    assert_eq!(icon_for(Some(&id("wifi.net.x"))), "wifi");
    assert_eq!(icon_for(Some(&id("audio.input.a%2Eb"))), "mic");
    assert_eq!(icon_for(Some(&id("wifi"))), "wifi");
}

// The point of the map: every route the shell ships names its icon here. A
// route added without one still renders, just bare, so only this test catches
// the omission.
#[test]
fn every_shipped_route_is_mapped() {
    let tree = build_tree(&default_menu(), &Entries::new());
    assert!(!tree.nodes.is_empty());
    let mut mapped = 0;
    for node in tree.nodes.values() {
        if node.id.is_empty() {
            continue;
        }
        assert!(!icon_for(Some(node)).is_empty(), "{} has no mapped icon name", node.id);
        mapped += 1;
    }
    assert!(mapped > 0);
}

#[test]
fn an_unmapped_row_keeps_its_own_glyph() {
    // An emoji row's icon IS the emoji, and a user menu.jsonc route names an id
    // this map has never heard of.
    assert_eq!(icon_for(Some(&Node { icon: "a".into(), ..id("emoji.a") })), "");
    assert_eq!(icon_for(Some(&id("my.own.route"))), "");
    assert_eq!(icon_for(Some(&Node::default())), "");
    assert_eq!(icon_for(None), "");
}

#[test]
fn a_mapped_row_answers_with_its_name() {
    assert_eq!(icon_for(Some(&id("clipboard"))), "clipboard");
    assert_eq!(icon_for(Some(&id("toggles.dnd"))), "bell-off");
    assert_eq!(icon_for(Some(&id("panels.network"))), "wifi");
}

// A row whose own icon is a raw codepoint draws a named icon for what it is
// instead.
#[test]
fn the_fallback_is_named() {
    let named = |kind: Kind| fallback_for(Some(&Node::new("x", "", kind)));
    assert_eq!(named(Kind::App), "app-window");
    assert_eq!(named(Kind::Submenu), "folder");
    assert_eq!(named(Kind::Provider), "folder");
    assert_eq!(named(Kind::Link), "folder");
    assert_eq!(named(Kind::Action), "terminal");
    assert_eq!(named(Kind::Note), "circle-help");
    assert_eq!(named(Kind::Other("option".into())), "circle-help");
    assert_eq!(fallback_for(None), "circle-help");
}
