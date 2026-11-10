// The launcher's ranked ids for a spread of queries over the shipped tree,
// a few apps, a configured abbreviation, a learned one, picks, a file row
// and clipboard rows. Printed in a stable shape so a refactor can diff it.

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
    let mut tree = real_tree();
    let apps: Vec<DesktopEntry> = [("firefox", "Firefox"), ("foxtrot", "Foxtrot"), ("notebook", "Notebook"), ("notes", "Notes Pad"), ("fx-editor", "Fx Editor"), ("mpv", "mpv Media Player")]
        .into_iter()
        .map(|(id, name)| desktop_entry(id, name))
        .collect();
    attach_children(&mut tree, "apps", apps_provider(&apps, None, &[], Some(NOW)));
    tree
}

fn record(id: &str, count: u32) -> Record {
    Record { id: id.into(), count: count.into(), last_ms: NOW }
}

fn dump(tag: &str, rows: &[Node]) {
    println!("RANK {tag}: {}", rows.iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(" "));
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

#[test]
fn ranking_snapshot() {
    let tree = world();
    for q in ["", "fx", "f", "notebook", "noteboo", "notebo", "note", "no", "app", "firefox", "mpv", "2+2", "sound", "dark", "wifi", "clip", "Foxtrot", "x"] {
        dump(&format!("root {q:?}"), &rows(&tree, q, None));
    }
    for (level, q) in [("apps", "no"), ("apps", ""), ("apps", "fx")] {
        dump(&format!("{level} {q:?}"), &rows(&tree, q, Some(level)));
    }
    let file = recent_subject("/home/u/notebook.txt", false, "/home/u");
    println!("RANK file row: {} {}", file.id, file.label);
    let clips: Vec<Node> = clipboard_provider(&[Entry::text("a", "notebook entry"), Entry::text("b", "other"), Entry::image("c", "/tmp/x.png")], ClipMode::Copy, true);
    for q in ["", "note", "zzz"] {
        println!("RANK clip {q:?}: {}", clipboard_search(&clips, q).iter().map(|n| n.id.as_str()).collect::<Vec<_>>().join(" "));
    }
}
