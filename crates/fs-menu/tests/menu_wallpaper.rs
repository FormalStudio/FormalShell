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
    Item, ItemKind, Mode, ProviderFn, Variant, apply_providers, catalog, image_basename, image_rows, items, title_of,
    wallpaper_listing, wallpaper_pick_mode, wallpaper_variants,
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

// The pairing model (specs/2026-11-04-wallpaper-picker.md §3).

fn listing(base: &str, names: &[&str]) -> Vec<String> {
    names.iter().map(|n| format!("{base}/{n}")).collect()
}

fn kinds(items: &[Item]) -> (usize, usize, usize, usize) {
    let mut n = (0, 0, 0, 0);
    for i in items {
        match &i.kind {
            ItemKind::Pair { .. } => n.0 += 1,
            ItemKind::Single { filed: None, .. } => n.1 += 1,
            ItemKind::Single { filed: Some(Mode::Dark), .. } => n.2 += 1,
            ItemKind::Single { filed: Some(Mode::Light), .. } => n.3 += 1,
            ItemKind::Sky => {}
        }
    }
    n
}

fn pair_of<'a>(items: &'a [Item], light: &str) -> Option<&'a str> {
    items.iter().find_map(|i| match &i.kind {
        ItemKind::Pair { light: l, dark } if l == light => Some(dark.as_str()),
        _ => None,
    })
}

// ~/Pictures/Wallpapers on the owner's mac, 2026-11-04.
const OWNER: &[&str] = &[
    "1.png",
    "2825711.png",
    "3840x2160_aesthetic_em-rofi.jpg",
    "3840x2160_mlfw_watchtower-mountains-and-forests.jpg",
    "Gang_We_In_Da_Carrrr_18052026 (1).png",
    "IMG_1178.PNG",
    "Villa_Nativo00002.jpg",
    "Villa_Nativo00020.jpg",
    "Villa_Nativo00094.jpg",
    "Villa_Nativo00132.jpg",
    "a6lmA5iOWS88N3ROEmzZRvzshiI.png",
    "dark/220742.jpg",
    "dark/3674x2449_nordic_ign_unsplash48.jpg",
    "dark/3840x1679_omarchy_golden-landscape-with-trees__01-golden-landscape-with-trees.jpg",
    "dark/3840x2160_aesthetic_guy_sleeping_colored.jpg",
    "dark/3840x2160_bonus_manga.png",
    "dark/3840x2160_omarchy_serene-anime-forest-landscape__01-serene-anime-forest-landscape.jpg",
    "dark/4000x2660_gruvbox-aj_elliott-engelmann-DjlKxYFJlTc.jpg",
    "dark/4658x2620_bonus_evening_sky.png",
    "dark/ARC - Towers.PNG",
    "dark/Desktop_1_Semaj_eb160ac6-0898-4e26-b8e7-89d121dcc77f.jpg",
    "dark/Moraine_Lake-flexoki.webp",
    "dark/Morants_Curve-flexoki.webp",
    "dark/clouds.61220450.png",
    "dark/death_valley_820b4aad-8ed2-4d4e-9bb9-0db23483c350.webp",
    "dark/earth.33dc76ef.png",
    "dark/endgame.69121815.png",
    "dark/field.5aa40e21.png",
    "dark/flexoki-dark-orb.png",
    "dark/galaxy.40a28fee.png",
    "dark/iss.bf7be4e6.png",
    "dark/kfc.jpg",
    "dark/nebula.4c3f9bc4.png",
    "dark/pylon.7a79ac39.png",
    "dark/river-city.jpg",
    "dark/saturn.35244eb4.png",
    "dark/storm.jpg",
    "dark/vincent-flexoki.jpg",
    "dark/wallhaven-rq67k1.jpg",
    "dark/wallhaven-yq2zwl-flexoki.png",
    "dark/wallhaven-yq2zwl-zenbones.png",
    "dark/water.2d2b7d25.png",
    "dark/wp3985916-tyler-the-creator-wallpapers.jpg",
    "dark/yosemite_ef80d4fc-df00-445c-a0ad-18fde4df3215.webp",
    "flexoki-dark-orb.png",
    "light/clouds.61220450.png",
    "light/emerald_bay_50ba6fb9-969d-4f10-a5c3-08f0d81457b3.webp",
    "light/field.5aa40e21.png",
    "light/flexoki-light-orb.png",
    "light/pylon.7a79ac39.png",
    "light/santa_barbara_7b8c547c-201e-4372-8389-4971ab84f866.webp",
    "light/water.2d2b7d25.png",
    "torii_black_and_white-wallpaper-5120x2880.jpg",
    "wallhaven-jee8ry.jpg",
    "wallhaven-jee8ry_rose-pine-Rose_Pine.png",
    "wallhaven-xee72o_rose-pine-Rose_Pine.png",
    "wallpaper2.png",
    "wp8968814-space-4k-firewatch-wallpapers.jpg",
    "wp9498191-firewatch-blue-wallpapers.png",
];

// The five byte-identical copies: four twins across dark/ and light/, and the
// orb in the root and dark/.
fn owner_identical(a: &str, b: &str) -> bool {
    let twins = ["clouds.61220450.png", "field.5aa40e21.png", "pylon.7a79ac39.png", "water.2d2b7d25.png", "flexoki-dark-orb.png"];
    image_basename(a) == image_basename(b) && twins.contains(&image_basename(a))
}

#[test]
fn the_owners_directory_reads_as_53_tiles() {
    let base = "/home/kyan/Pictures/Wallpapers";
    let got = items(&listing(base, OWNER), base, owner_identical);
    assert_eq!(got.len(), 53);
    assert_eq!(kinds(&got), (1, 4 + 18, 28, 2));
    assert_eq!(
        pair_of(&got, &format!("{base}/light/flexoki-light-orb.png")),
        Some(format!("{base}/dark/flexoki-dark-orb.png").as_str())
    );
    let twins = got
        .iter()
        .filter(|i| matches!(&i.kind, ItemKind::Single { path, filed: None } if path.contains("/dark/") || path.contains("/light/")))
        .count();
    assert_eq!(twins, 4);
    assert!(got.iter().all(|i| i.id != format!("{base}/flexoki-dark-orb.png")));
    assert!(got.iter().any(|i| i.title == "Flexoki orb" && matches!(i.kind, ItemKind::Pair { .. })));
}

#[test]
fn same_name_across_dark_and_light() {
    let p = |n: &str| format!("/w/{n}");
    let same = |a: &str, b: &str| image_basename(a) == "twin.png" && image_basename(b) == "twin.png";
    let got = items(&listing("/w", &["Dark/pair.png", "Light/pair.png", "Dark/twin.png", "Light/twin.png", "twin.png"]), "/w", same);
    assert_eq!(pair_of(&got, &p("Light/pair.png")), Some(p("Dark/pair.png").as_str()));
    assert_eq!(kinds(&got), (1, 1, 0, 0));

    // A root copy with other bytes is a picture of its own.
    let got = items(&listing("/w", &["dark/x.png", "x.png"]), "/w", |_, _| false);
    assert_eq!(kinds(&got), (0, 1, 1, 0));
}

#[test]
fn gnome_letter_tokens_pair() {
    let got = items(&listing("/w", &["fold-d.jxl", "fold-l.png", "plan-d.png"]), "/w", |_, _| false);
    assert_eq!(pair_of(&got, "/w/fold-l.png"), Some("/w/fold-d.jxl"));
    assert_eq!(got.iter().find(|i| i.id == "/w/fold-l.png").unwrap().title, "Fold");
    // A letter with no sibling is just a word, filed by its directory alone.
    assert_eq!(kinds(&got), (1, 1, 0, 0));
    // A whole word never pairs with a letter.
    let got = items(&listing("/w", &["x-dark.png", "x-l.png"]), "/w", |_, _| false);
    assert_eq!(kinds(&got), (0, 2, 0, 0));
}

#[test]
fn the_token_names_the_half_whatever_the_directory() {
    let got = items(&listing("/w", &["Light/sea-dark.png", "sea-Light.png"]), "/w", |_, _| false);
    assert_eq!(pair_of(&got, "/w/sea-Light.png"), Some("/w/Light/sea-dark.png"));
    assert_eq!(got[0].title, "Sea");
    // With no sibling the token is a word and the directory files it.
    let got = items(&listing("/w", &["Light/moon-dark.png"]), "/w", |_, _| false);
    assert_eq!(kinds(&got), (0, 0, 0, 1));
}

#[test]
fn palette_siblings_stay_apart() {
    let got = items(&listing("/w", &["dark/wallhaven-yq2zwl-flexoki.png", "dark/wallhaven-yq2zwl-zenbones.png"]), "/w", |_, _| false);
    assert_eq!(kinds(&got), (0, 0, 2, 0));
}

#[test]
fn two_dark_candidates_for_one_light() {
    // The one in the light half's directory wins.
    let got = items(&listing("/w", &["Dark/sea-dark.png", "Light/sea-dark.jpg", "Light/sea-light.png"]), "/w", |_, _| false);
    assert_eq!(pair_of(&got, "/w/Light/sea-light.png"), Some("/w/Light/sea-dark.jpg"));
    assert_eq!(kinds(&got), (1, 0, 1, 0));
    // Neither there: the first by name.
    let got = items(&listing("/w", &["a/sea-dark.webp", "b/sea-dark.jpg", "sea-light.png"]), "/w", |_, _| false);
    assert_eq!(pair_of(&got, "/w/sea-light.png"), Some("/w/b/sea-dark.jpg"));
    assert_eq!(kinds(&got), (1, 1, 0, 0));
}

#[test]
fn titles() {
    let cases = [
        ("1.png", "1"),
        ("2825711.png", "2825711.png"),
        ("3840x2160_aesthetic_em-rofi.jpg", "Aesthetic em rofi"),
        ("3840x2160_mlfw_watchtower-mountains-and-forests.jpg", "Mlfw watchtower mountains and forests"),
        ("Gang_We_In_Da_Carrrr_18052026 (1).png", "Gang We In Da Carrrr 1"),
        ("IMG_1178.PNG", "IMG 1178"),
        ("Villa_Nativo00002.jpg", "Villa Nativo00002"),
        ("Villa_Nativo00020.jpg", "Villa Nativo00020"),
        ("Villa_Nativo00094.jpg", "Villa Nativo00094"),
        ("Villa_Nativo00132.jpg", "Villa Nativo00132"),
        ("a6lmA5iOWS88N3ROEmzZRvzshiI.png", "A6lmA5iOWS88N3ROEmzZRvzshiI"),
        ("220742.jpg", "220742.jpg"),
        ("3674x2449_nordic_ign_unsplash48.jpg", "Nordic ign unsplash48"),
        (
            "3840x1679_omarchy_golden-landscape-with-trees__01-golden-landscape-with-trees.jpg",
            "Omarchy golden landscape with trees 01 golden landscape with trees",
        ),
        ("3840x2160_aesthetic_guy_sleeping_colored.jpg", "Aesthetic guy sleeping colored"),
        ("3840x2160_bonus_manga.png", "Bonus manga"),
        (
            "3840x2160_omarchy_serene-anime-forest-landscape__01-serene-anime-forest-landscape.jpg",
            "Omarchy serene anime forest landscape 01 serene anime forest landscape",
        ),
        ("4000x2660_gruvbox-aj_elliott-engelmann-DjlKxYFJlTc.jpg", "Gruvbox aj elliott engelmann DjlKxYFJlTc"),
        ("4658x2620_bonus_evening_sky.png", "Bonus evening sky"),
        ("ARC - Towers.PNG", "ARC Towers"),
        ("Desktop_1_Semaj_eb160ac6-0898-4e26-b8e7-89d121dcc77f.jpg", "Desktop 1 Semaj eb160ac6 0898 4e26 b8e7 89d121dcc77f"),
        ("Moraine_Lake-flexoki.webp", "Moraine Lake flexoki"),
        ("Morants_Curve-flexoki.webp", "Morants Curve flexoki"),
        ("clouds.61220450.png", "Clouds"),
        ("death_valley_820b4aad-8ed2-4d4e-9bb9-0db23483c350.webp", "Death valley 820b4aad 8ed2 4d4e 9bb9 0db23483c350"),
        ("earth.33dc76ef.png", "Earth 33dc76ef"),
        ("endgame.69121815.png", "Endgame"),
        ("field.5aa40e21.png", "Field 5aa40e21"),
        ("flexoki-dark-orb.png", "Flexoki orb"),
        ("flexoki-light-orb.png", "Flexoki orb"),
        ("galaxy.40a28fee.png", "Galaxy 40a28fee"),
        ("iss.bf7be4e6.png", "Iss bf7be4e6"),
        ("kfc.jpg", "Kfc"),
        ("nebula.4c3f9bc4.png", "Nebula 4c3f9bc4"),
        ("pylon.7a79ac39.png", "Pylon 7a79ac39"),
        ("river-city.jpg", "River city"),
        ("saturn.35244eb4.png", "Saturn 35244eb4"),
        ("storm.jpg", "Storm"),
        ("vincent-flexoki.jpg", "Vincent flexoki"),
        ("wallhaven-rq67k1.jpg", "Wallhaven rq67k1"),
        ("wallhaven-yq2zwl-flexoki.png", "Wallhaven yq2zwl flexoki"),
        ("wallhaven-yq2zwl-zenbones.png", "Wallhaven yq2zwl zenbones"),
        ("water.2d2b7d25.png", "Water 2d2b7d25"),
        ("wp3985916-tyler-the-creator-wallpapers.jpg", "Wp3985916 tyler the creator wallpapers"),
        ("yosemite_ef80d4fc-df00-445c-a0ad-18fde4df3215.webp", "Yosemite ef80d4fc df00 445c a0ad 18fde4df3215"),
        ("emerald_bay_50ba6fb9-969d-4f10-a5c3-08f0d81457b3.webp", "Emerald bay 50ba6fb9 969d 4f10 a5c3 08f0d81457b3"),
        ("santa_barbara_7b8c547c-201e-4372-8389-4971ab84f866.webp", "Santa barbara 7b8c547c 201e 4372 8389 4971ab84f866"),
        ("torii_black_and_white-wallpaper-5120x2880.jpg", "Torii black and white wallpaper"),
        ("wallhaven-jee8ry.jpg", "Wallhaven jee8ry"),
        ("wallhaven-jee8ry_rose-pine-Rose_Pine.png", "Wallhaven jee8ry rose pine Rose Pine"),
        ("wallhaven-xee72o_rose-pine-Rose_Pine.png", "Wallhaven xee72o rose pine Rose Pine"),
        ("wallpaper2.png", "Wallpaper2"),
        ("wp8968814-space-4k-firewatch-wallpapers.jpg", "Wp8968814 space 4k firewatch wallpapers"),
        ("wp9498191-firewatch-blue-wallpapers.png", "Wp9498191 firewatch blue wallpapers"),
    ];
    for (name, title) in cases {
        assert_eq!(title_of(name), title, "{name}");
    }
    let names: Vec<&str> = OWNER.iter().map(|p| image_basename(p)).collect();
    for name in names {
        assert!(cases.iter().any(|(n, _)| *n == name), "no title case for {name}");
    }
}

#[test]
fn the_catalog() {
    let json = r#"[
        {"id": "lake", "title": "Lake", "author": "Jingwei Ke", "license": "CC0-1.0",
         "licenseUrl": "https://creativecommons.org/publicdomain/zero/1.0/",
         "source": "https://commons.wikimedia.org/wiki/File:Lake.jpg", "origin": "https://unsplash.com/photos/x", "file": "lake.jpg"},
        {"id": "gone", "title": "Gone", "file": "gone.jpg"},
        {"id": "fold", "light": "fold-l.jpg", "dark": "fold-d.jpg"},
        {"id": "half", "light": "half-l.jpg", "dark": "half-gone.jpg"}
    ]"#;
    let exists = |p: &str| !p.contains("gone");
    let got = catalog(json, "/share/wallpapers/", exists);
    assert_eq!(got.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["lake", "fold"]);
    assert_eq!(got[0].kind, ItemKind::Single { path: "/share/wallpapers/lake.jpg".into(), filed: None });
    let credit = got[0].credit.as_ref().unwrap();
    assert_eq!((credit.author.as_str(), credit.license.as_str()), ("Jingwei Ke", "CC0-1.0"));
    assert_eq!(got[1].kind, ItemKind::Pair { light: "/share/wallpapers/fold-l.jpg".into(), dark: "/share/wallpapers/fold-d.jpg".into() });
    assert_eq!(got[1].title, "Fold");
    assert!(catalog("not json", "/share", |_| true).is_empty());
}
