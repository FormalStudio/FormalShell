// The root "wallpaper" node and the rows its level shows. The picker is a menu
// ROUTE: an ordinary provider entry `default-menu.jsonc` declares itself, whose
// level the launcher renders as a grid of `image_rows`.
//
// Loads the real shipped default-menu.jsonc rather than a fixture mirroring it:
// the point is that the declaration in the file the package ships still builds
// the route the menu and the `picker` IPC target both resolve against.

mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::icons::icon_for;
use fs_menu::model::build_tree;
use fs_menu::node::{Entries, Kind, Node};
use fs_menu::providers::{
    ProviderFn, Variant, apply_providers, image_rows, wallpaper_listing, wallpaper_pick_mode, wallpaper_variants,
};
use serde_json::json;

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn shipped_default_declares_the_wallpaper_route() {
    let defaults = default_menu();
    let entry = defaults.get("wallpaper").expect("wallpaper entry");
    assert_eq!(entry.label.as_deref(), Some("Wallpaper"));
    assert_eq!(entry.provider.as_deref(), Some("wallpaper"));
    assert!(entry.aliases.as_ref().unwrap().contains(&"picker".to_string()));
    assert_eq!(icon_for(Some(&Node::new("wallpaper", "", Kind::Submenu))), "image");
}

// "provider" is what makes the node a descendable level rather than an action
// row: activation enters it, and the display rows swap the row list for the
// image grid once the current node is it.
#[test]
fn it_builds_as_a_root_provider_level() {
    let tree = build_tree(&default_menu(), &Entries::new());
    let node = &tree.nodes["wallpaper"];
    assert_eq!(node.parent_id, None);
    assert_eq!(node.kind, Kind::Provider);
    assert!(tree.root_ids.contains(&"wallpaper".to_string()));
}

// No provider fn is registered for it (same as emoji/calc/keybinds): the
// level's contents come from the directory scan, not the tree, so the node
// stays childless after apply_providers rather than silently acquiring rows
// from somewhere else.
#[test]
fn no_provider_fn_leaves_the_level_childless() {
    let mut tree = build_tree(&default_menu(), &Entries::new());
    apply_providers(&mut tree, &HashMap::<String, ProviderFn>::new());
    assert!(tree.nodes["wallpaper"].child_ids.is_empty());
}

#[test]
fn user_overlay_can_hide_it() {
    let defaults = default_menu();
    let mut only = Entries::new();
    only.insert("wallpaper".to_string(), defaults["wallpaper"].clone());
    let tree = build_tree(&only, &entries(json!({ "wallpaper": { "hidden": true } })));
    assert!(!tree.nodes.contains_key("wallpaper"));
    assert!(tree.root_ids.is_empty());
}

#[test]
fn image_rows_carry_the_path_and_a_basename_label() {
    let rows = image_rows(&strs(&["/pics/aurora.png", "/pics/dune.jpg"]), "");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].kind, Kind::Image);
    assert_eq!(rows[0].path, "/pics/aurora.png");
    assert_eq!(rows[0].label, "aurora.png");
    assert_eq!(rows[0].parent_id.as_deref(), Some("wallpaper"));
}

// Filtering is on the basename only: every row in a listing shares the same
// directory component, so matching it would make any query that happens to
// contain a directory name match the whole listing.
#[test]
fn image_rows_filter_on_the_basename_not_the_directory() {
    let paths = strs(&["/home/kyan/dune/aurora.png", "/home/kyan/dune/dune.jpg"]);
    assert_eq!(image_rows(&paths, "dune").len(), 1);
    assert_eq!(image_rows(&paths, "dune")[0].path, "/home/kyan/dune/dune.jpg");
    assert_eq!(image_rows(&paths, "AURORA").len(), 1);
    assert_eq!(image_rows(&paths, "").len(), 2);
}

#[test]
fn image_rows_tolerate_an_empty_listing() {
    assert_eq!(image_rows(&[], "").len(), 0);
    assert_eq!(image_rows(&[], "x").len(), 0);
}

// Dark/Light variants. The scan hands over the picker directory AND its
// Dark/Light subdirectories in one listing; this is the split the route's
// switcher reads.
#[test]
fn variants_split_the_listing_by_subdirectory() {
    let v = wallpaper_variants(&strs(&["/pics/Dark/night.png", "/pics/Light/day.png", "/pics/Light/noon.jpg"]), "/pics");
    assert!(v.has_variants);
    assert_eq!(v.dark.len(), 1);
    assert_eq!(v.light.len(), 2);
    assert_eq!(v.root.len(), 0);
    assert_eq!(v.dark[0], "/pics/Dark/night.png");
}

// Either case, since the owner's own directories may be either and a
// case-insensitive filesystem makes the distinction meaningless anyway.
#[test]
fn variant_directory_names_are_case_insensitive() {
    let v = wallpaper_variants(&strs(&["/pics/dark/night.png", "/pics/LIGHT/day.png"]), "/pics");
    assert_eq!(v.dark.len(), 1);
    assert_eq!(v.light.len(), 1);
}

// No subdirectory pair: one flat listing, no switcher, exactly the behavior
// every existing setup has today.
#[test]
fn a_directory_without_the_pair_lists_flat() {
    let v = wallpaper_variants(&strs(&["/pics/a.png", "/pics/b.png"]), "/pics");
    assert!(!v.has_variants);
    assert_eq!(v.root.len(), 2);
    assert_eq!(v.dark.len(), 0);
    assert_eq!(v.light.len(), 0);
}

// Classification is by position relative to the scanned directory, not by the
// parent directory's name: a picker directory that is itself called Dark must
// not turn its own root listing into a variant.
#[test]
fn a_base_directory_named_dark_is_still_the_root_listing() {
    let v = wallpaper_variants(&strs(&["/pics/Dark/night.png"]), "/pics/Dark");
    assert!(!v.has_variants);
    assert_eq!(v.root.len(), 1);
}

// Any other subdirectory is neither variant: only the two names mean anything,
// and a directory of, say, archived wallpapers must not become one mode's set.
// The scan's own -maxdepth 1 means neither case reaches here in practice; the
// rule is the first path segment either way, so something nested under Dark/
// would still be dark rather than unsorted.
#[test]
fn unrelated_subdirectories_are_not_variants() {
    let v = wallpaper_variants(&strs(&["/pics/archive/old.png"]), "/pics");
    assert!(!v.has_variants);
    assert_eq!(v.root.len(), 1);

    let nested = wallpaper_variants(&strs(&["/pics/Dark/deep/deeper.png"]), "/pics");
    assert_eq!(nested.dark.len(), 1);
}

// One variant present is still variant mode: the empty side reads honestly
// under its own header, where falling back to the root listing would look like
// the switcher did nothing.
#[test]
fn one_variant_alone_still_raises_the_switcher() {
    let v = wallpaper_variants(&strs(&["/pics/Dark/night.png"]), "/pics");
    assert!(v.has_variants);
    assert_eq!(wallpaper_listing(&v, Some(Variant::Light)).len(), 0);
    assert_eq!(wallpaper_listing(&v, Some(Variant::Dark)).len(), 1);
}

// The one function the grid, `picker choose`'s membership check and `picker
// status`'s count all read, so they cannot disagree about what is on screen. An
// unknown variant name resolves to dark rather than to nothing.
#[test]
fn listing_picks_the_variant_or_the_flat_root() {
    let pair = wallpaper_variants(&strs(&["/pics/Dark/n.png", "/pics/Light/d.png"]), "/pics");
    assert_eq!(wallpaper_listing(&pair, Some(Variant::Dark))[0], "/pics/Dark/n.png");
    assert_eq!(wallpaper_listing(&pair, Some(Variant::Light))[0], "/pics/Light/d.png");
    assert_eq!(wallpaper_listing(&pair, None)[0], "/pics/Dark/n.png");

    let flat = wallpaper_variants(&strs(&["/pics/a.png"]), "/pics");
    assert_eq!(wallpaper_listing(&flat, Some(Variant::Light))[0], "/pics/a.png");
}

#[test]
fn a_pick_out_of_a_set_commits_that_mode() {
    let pair = wallpaper_variants(&strs(&["/pics/Dark/n.png", "/pics/Light/d.png"]), "/pics");
    assert_eq!(wallpaper_pick_mode(&pair, Some(Variant::Dark)), Some(Variant::Dark));
    assert_eq!(wallpaper_pick_mode(&pair, Some(Variant::Light)), Some(Variant::Light));
    let flat = wallpaper_variants(&strs(&["/pics/a.png"]), "/pics");
    assert_eq!(wallpaper_pick_mode(&flat, Some(Variant::Dark)), None);
}

#[test]
fn variants_tolerate_an_empty_scan() {
    let v = wallpaper_variants(&[], "/pics");
    assert!(!v.has_variants);
    assert_eq!(wallpaper_listing(&v, Some(Variant::Dark)).len(), 0);
    assert_eq!(wallpaper_variants(&[], "").root.len(), 0);
}
