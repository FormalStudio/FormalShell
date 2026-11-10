#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

use fs_menu::model::{build_tree, entries_from_value, parse_jsonc};
use fs_menu::node::{DesktopEntry, Entries, Node, Provider, Tree};
use fs_menu::providers::{ProviderFn, TrayItem, apply_providers, panels_provider, tray_provider};
use serde_json::Value;

/// A file under the repo root.
pub fn read_repo(path: &str) -> String {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    std::fs::read_to_string(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

/// Entries from a `json!` object literal, in the literal's key order.
pub fn entries(value: Value) -> Entries {
    entries_from_value(&value).expect("entries")
}

pub fn tree_of(value: Value) -> Tree {
    build_tree(&entries(value), &Entries::new())
}

pub fn default_menu() -> Entries {
    entries_from_value(&parse_jsonc(&read_repo("crates/fs-menu/data/default-menu.jsonc")).expect("default-menu.jsonc"))
        .expect("default-menu entries")
}

/// The shipped tree with "panels" and "tray" expanded the way the launcher's
/// own registry does it.
pub fn real_tree() -> Tree {
    let mut tree = build_tree(&default_menu(), &Entries::new());
    let mut fns: HashMap<Provider, ProviderFn> = HashMap::new();
    fns.insert(Provider::Panels, Box::new(panels_provider));
    fns.insert(Provider::Tray, Box::new(|| tray_provider(&[] as &[TrayItem])));
    apply_providers(&mut tree, &fns);
    tree
}

pub fn ids<'a>(nodes: &[&'a Node]) -> Vec<&'a str> {
    nodes.iter().map(|n| n.id.as_str()).collect()
}

pub fn desktop_entry(id: &str, name: &str) -> DesktopEntry {
    DesktopEntry { id: id.into(), name: name.into(), ..DesktopEntry::default() }
}

pub fn no_conds() -> HashMap<String, bool> {
    HashMap::new()
}
