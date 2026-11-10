// Row icon names. The drift guard reads the shipped default-menu.jsonc rather
// than a fixture: a route added without an `iconName` renders with no icon
// at all. Resolving the names in the Lucide and Nerd sets belongs to the
// theme icon table's port.

mod common;

use common::*;
use fs_menu::icons::{fallback_for, icon_for};
use fs_menu::model::build_tree;
use fs_menu::node::{Entries, Entry, Kind, Node};

// Every route the shell ships names its icon. A route added without one
// still renders, just bare, so only this test catches the omission.
#[test]
fn every_shipped_route_names_its_icon() {
    let tree = build_tree(&default_menu(), &Entries::new());
    assert!(!tree.nodes.is_empty());
    for node in tree.nodes.values() {
        assert!(!icon_for(Some(node)).is_empty(), "{} has no icon name", node.id);
    }
    assert_eq!(icon_for(Some(&tree.nodes["clipboard"])), "clipboard");
    assert_eq!(icon_for(Some(&tree.nodes["toggles.dnd"])), "bell-off");
    assert_eq!(icon_for(Some(&real_tree().nodes["panels.network"])), "wifi");
}

#[test]
fn a_row_with_no_name_keeps_its_own_glyph() {
    // An emoji row's icon IS the emoji, and a user menu.jsonc route names no
    // icon of the set.
    assert_eq!(icon_for(Some(&Node { icon: "a".into(), ..Node::new("emoji.a", "", Kind::Action) })), "");
    assert_eq!(icon_for(Some(&Node::default())), "");
    assert_eq!(icon_for(None), "");
}

// A user entry overriding a shipped route keeps the route's icon and chord
// unless it names its own.
#[test]
fn a_user_override_keeps_the_shipped_icon_and_chord() {
    let user = Entries::from([("clipboard".to_owned(), Entry { prompt: Some("Mine".into()), ..Entry::labelled("Clips") })]);
    let tree = build_tree(&default_menu(), &user);
    let clip = &tree.nodes["clipboard"];
    assert_eq!((clip.label.as_str(), clip.icon_name.as_str(), clip.chord.as_str()), ("Clips", "clipboard", "Super+Ctrl+V"));
    let own = Entries::from([("clipboard".to_owned(), Entry { chord: Some("Super+V".into()), ..Entry::labelled("Clips").with_icon_name("copy") })]);
    let clip = &build_tree(&default_menu(), &own).nodes["clipboard"];
    assert_eq!((clip.icon_name.as_str(), clip.chord.as_str()), ("copy", "Super+V"));
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
    assert_eq!(named(Kind::Window), "app-window");
    assert_eq!(named(Kind::Note), "circle-help");
    assert_eq!(named(Kind::Option), "circle-help");
    assert_eq!(fallback_for(None), "circle-help");
}
