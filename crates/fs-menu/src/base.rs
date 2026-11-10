//! The launcher's base tree: the shipped menu merged with the entries each
//! live source contributes, then the user's `menu.jsonc` over it.

use serde_json::Value;

use crate::clipboard::entry::Entry as ClipEntry;
use crate::model::{build_tree, entries_from_value, parse_jsonc};
use crate::node::{Entries, Entry, Provider, Tree};
use crate::providers::{self, CustomPowerButton, LightEffect, ReceiveStatus};

const DEFAULT_MENU: &str = include_str!("../data/default-menu.jsonc");

/// The vendored emoji dataset, for `providers::parse_emoji_dataset`.
pub const EMOJI: &str = include_str!("../data/emoji.json");

/// What the base tree is built from besides the files.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BaseInputs {
    pub buttons: Value,
    /// The keyboard lights' effects, `None` while no keyboard was found.
    pub lights: Option<Vec<LightEffect>>,
    /// padbridge is on the bus: the iPad route.
    pub ipad: bool,
    /// The share subtree: LocalSend's state and what it can send.
    pub share: Option<ShareInputs>,
    /// Each launcher plugin's mode, `(route id, name)`.
    pub plugin_modes: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShareInputs {
    pub installed: bool,
    pub peers: Vec<String>,
    pub items: Vec<ClipEntry>,
    pub receive: ReceiveStatus,
}

/// The shipped menu's entries. An error here is a build defect, so the
/// caller logs it and carries on with the sources alone.
pub fn defaults() -> Result<Entries, serde_json::Error> {
    entries_from_value(&parse_jsonc(DEFAULT_MENU)?)
}

/// `defaults` merged with the self-targeted fragments and the custom power
/// buttons, then `user` over it.
pub fn base_tree(defaults: Entries, inputs: &BaseInputs, user: &Entries) -> Tree {
    let mut merged = defaults;
    for (k, v) in providers::capture_entries() {
        merged.insert(k, v);
    }
    if let Some(effects) = &inputs.lights {
        for (k, v) in providers::lights_entries(true, effects) {
            merged.insert(k, v);
        }
    }
    for (k, v) in providers::ipad_entries(inputs.ipad) {
        merged.insert(k, v);
    }
    if let Some(sh) = &inputs.share {
        let peers: Vec<providers::Peer> = sh.peers.iter().map(|n| providers::Peer { name: n.clone() }).collect();
        for (k, v) in providers::share_peer_entries(sh.installed, &peers, &sh.items, Some(&sh.receive)) {
            merged.insert(k, v);
        }
    }
    for (k, v) in providers::custom_power_button_entries(&power_buttons(&inputs.buttons)) {
        merged.insert(k, v);
    }
    for (route, name) in &inputs.plugin_modes {
        let entry = Entry { provider: Some(Provider::Plugin(route.clone())), ..Entry::labelled(name).with_icon("puzzle") };
        merged.insert(route.clone(), entry);
    }
    let mut tree = build_tree(&merged, user);
    let panels = providers::panels_provider();
    if tree.nodes.contains_key("panels") {
        providers::attach_children(&mut tree, "panels", panels);
    }
    tree
}

fn power_buttons(v: &Value) -> Vec<CustomPowerButton> {
    let Value::Array(items) = v else { return Vec::new() };
    items
        .iter()
        .filter_map(|b| {
            let s = |k: &str| b.get(k).and_then(Value::as_str).map(str::to_owned);
            Some(CustomPowerButton {
                label: s("label")?,
                icon: s("icon"),
                command: s("command")?,
                confirm: b.get("confirm").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}
