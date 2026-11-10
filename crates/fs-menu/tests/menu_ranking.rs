// The launcher's ranked ids for a spread of queries over the shipped tree,
// a few apps, a configured abbreviation, a learned one, picks, a file row
// and clipboard rows, asserted against the ids the tree produced before the
// launcher's query pipeline moved into fs-menu.

mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::abbrev::{self, Book};
use fs_menu::clipboard::entry::Entry;
use fs_menu::frecency::Record;
use fs_menu::model;
use fs_menu::node::{DesktopEntry, Node};
use fs_menu::providers::{apps_provider, attach_children, clipboard_provider, clipboard_search, ClipMode, recent_subject};

const NOW: f64 = 1_800_000_000_000.0;

fn world() -> fs_menu::node::Tree {
    with_apps(real_tree())
}

fn with_apps(mut tree: fs_menu::node::Tree) -> fs_menu::node::Tree {
    let apps: Vec<DesktopEntry> = [("firefox", "Firefox"), ("foxtrot", "Foxtrot"), ("notebook", "Notebook"), ("notes", "Notes Pad"), ("fx-editor", "Fx Editor"), ("mpv", "mpv Media Player")]
        .into_iter()
        .map(|(id, name)| desktop_entry(id, name))
        .collect();
    attach_children(&mut tree, "apps", apps_provider(&apps, None, &[], NOW));
    tree
}

fn record(id: &str, count: u32) -> Record {
    Record { id: id.into(), count: count.into(), last_ms: NOW }
}

fn rows(tree: &fs_menu::node::Tree, q: &str, level: Option<&str>) -> Vec<Node> {
    let mut configured = HashMap::new();
    let sid = |id: &str| fs_menu::actions::subject_id(&tree.nodes[id]).to_string();
    configured.insert("fx".to_string(), sid("apps.foxtrot"));
    let book = Book::default();
    let lookup = abbrev::lookup(&configured, &book, q, NOW, |id| model::subject_node(&tree.nodes, id).is_some());
    let picks = vec![record(&sid("apps.notes"), 3), record(&sid("apps.notebook"), 1)];
    rows_under_test(tree, q, level, &lookup, &picks)
}

fn rows_under_test(tree: &fs_menu::node::Tree, q: &str, level: Option<&str>, lookup: &abbrev::Lookup, picks: &[Record]) -> Vec<Node> {
    let boost = fs_menu::search::Boost { abbrev: Some(lookup), picks, now: NOW };
    fs_menu::search::rows_for_query(&tree.nodes, q, &no_conds(), level, boost)
}

fn line<'a>(tag: &str, rows: impl IntoIterator<Item = &'a Node>) -> String {
    format!("{tag}: {}", rows.into_iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(" "))
}

fn with_book(tree: &fs_menu::node::Tree, q: &str, book: &Book) -> Vec<Node> {
    let lookup = abbrev::lookup(&HashMap::new(), book, q, NOW, |id| model::subject_node(&tree.nodes, id).is_some());
    rows_under_test(tree, q, None, &lookup, &[])
}

fn snapshot() -> String {
    let tree = world();
    let sid = |id: &str| fs_menu::actions::subject_id(&tree.nodes[id]).to_string();
    let mut out = Vec::new();
    for q in ["", "fx", "f", "notebook", "noteboo", "notebo", "note", "no", "app", "firefox", "mpv", "2+2", "sound", "dark", "wifi", "clip", "Foxtrot", "x"] {
        out.push(line(&format!("root {q:?}"), &rows(&tree, q, None)));
    }
    for (level, q) in [("apps", "no"), ("apps", ""), ("apps", "fx")] {
        out.push(line(&format!("{level} {q:?}"), &rows(&tree, q, Some(level))));
    }

    let file = recent_subject("/home/u/notebook.txt", false, "/home/u");
    out.push(format!("file row: {} {}", file.id, file.label));

    let pinned = abbrev::pin(&Book::default(), "nb", &sid("apps.notebook"));
    out.push(line("pinned nb", &with_book(&tree, "nb", &pinned)));
    let learned = (0..2).fold(Book::default(), |b, _| abbrev::learn(&b, "nt", &sid("apps.notes"), NOW));
    out.push(line("learned nt", &with_book(&tree, "nt", &learned)));

    let plugin_tree = with_apps(fs_menu::base::base_tree(
        fs_menu::base::defaults().expect("defaults"),
        &fs_menu::base::BaseInputs { plugin_modes: vec![("plugin.zeta".into(), "Zeta Tool".into())], ..Default::default() },
        &fs_menu::node::Entries::new(),
    ));
    out.push(line("plugin zeta", &rows(&plugin_tree, "zeta", None)));

    let dataset = fs_menu::model::parse_headered_json(&read_repo("crates/fs-menu/data/emoji.json")).expect("emoji.json parses");
    let emoji = fs_menu::providers::EmojiIndex::new(serde_json::from_value(dataset).expect("emoji entries"));
    let sob = emoji.rows("sob", true, &[], 0.0);
    out.push(format!("emoji sob: {} {}", sob[0].id, sob[0].icon));

    let clips: Vec<Node> = clipboard_provider(&[Entry::text("a", "notebook entry"), Entry::text("b", "other"), Entry::image("c", "/tmp/x.png")], ClipMode::Copy, true);
    for q in ["", "note", "zzz"] {
        out.push(line(&format!("clip {q:?}"), clipboard_search(&clips, q).into_iter().map(|n| n as &Node)));
    }
    out.join("\n")
}

const EXPECTED: &str = r#"root "": 
root "fx": apps.foxtrot apps.fx-editor apps.firefox
root "f": files apps.firefox apps.foxtrot apps.fx-editor info wifi system.notifications toggles.caffeinate mirror settings radio install
root "notebook": apps.notebook
root "noteboo": apps.notebook
root "notebo": apps.notebook
root "note": apps.notes apps.notebook
root "no": system.notifications apps.notes apps.notebook toggles.dnd monitor info panels.network panels.monitor
root "app": apps panels.appmenu install wallpaper
root "firefox": apps.firefox
root "mpv": apps.mpv
root "2+2": calc.result
root "sound": audio
root "dark": toggles.dark-mode
root "wifi": wifi
root "clip": clipboard
root "Foxtrot": apps.foxtrot
root "x": apps.firefox apps.foxtrot apps.fx-editor
apps "no": apps.notes apps.notebook
apps "": 
apps "fx": apps.foxtrot apps.fx-editor apps.firefox
file row: files.b1004f719a3b1497 notebook.txt
pinned nb: apps.notebook toggles.dnd
learned nt: apps.notes files wifi panels.quicksettings monitor info install panels.network panels.monitor system.notifications toggles.overnight toggles.caffeinate toggles.dnd apps.notebook
plugin zeta: plugin.zeta
emoji sob: emoji.😭 😭
clip "": clipboard.a clipboard.b clipboard.c
clip "note": clipboard.a
clip "zzz": "#;

#[test]
fn ranking_snapshot() {
    assert_eq!(snapshot(), EXPECTED);
}
