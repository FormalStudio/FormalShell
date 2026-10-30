//! Live `checked` conditions for the launcher's toggle rows. A toggle row's
//! checkmark has to flip in the same event-loop turn the toggle itself does,
//! so a `@state:` condition is answered from a property snapshot the caller
//! hands in and never spawns a process.
//!
//! The path lists are closed allow-lists, checked by membership: a condition
//! naming a path outside them resolves false, so a hand-written `menu.jsonc`
//! has no route into the engine through this field. A new path has to be
//! added here and to the caller's snapshot literal.

use std::collections::HashMap;

use indexmap::IndexMap;
use serde_json::Value;

use crate::node::Node;

pub const PREFIX: &str = "@state:";

/// Flags.
pub const PATHS: &[&str] = &[
    "nightlight.active",
    "overnight.active",
    "caffeinate.active",
    "notifications.dnd",
    "theme.dark",
    "lights.on",
    "hdr.active",
];

/// Paths whose value is a string rather than a flag, matched as
/// `@state:<path>=<value>`: one row per choice, checked on the one the
/// snapshot holds.
pub const ENUM_PATHS: &[&str] = &[
    "lights.effect",
    "lights.source",
    "lights.colour",
    "lights.speed",
    "lights.brightness",
    "wifi.ssid",
    "audio.sink",
    "audio.source",
    "radio.station",
];

/// Paths whose value is a list of strings, matched by membership.
pub const LIST_PATHS: &[&str] = &["bluetooth.connected"];

#[derive(Clone, Debug, PartialEq)]
pub enum SnapValue {
    Flag(bool),
    Text(String),
    List(Vec<String>),
}

/// The normalized property snapshot: exactly the allow-listed paths as keys.
pub type Snapshot = IndexMap<String, SnapValue>;

pub fn is_state_condition(cond: &str) -> bool {
    cond.starts_with(PREFIX)
}

pub fn state_path(cond: &str) -> Option<&str> {
    cond.strip_prefix(PREFIX)
}

pub fn is_known_path(path: &str) -> bool {
    PATHS.contains(&path)
}

pub fn is_known_enum_path(path: &str) -> bool {
    ENUM_PATHS.contains(&path)
}

pub fn is_known_list_path(path: &str) -> bool {
    LIST_PATHS.contains(&path)
}

/// A new snapshot carrying exactly the allow-listed paths: a missing flag
/// reads false (only a strict `true` counts), a non-string enum reads "", a
/// non-list reads empty, and an unlisted key is dropped.
pub fn snapshot(live: Option<&Value>) -> Snapshot {
    let source = live.and_then(Value::as_object);
    let get = |key: &str| source.and_then(|o| o.get(key));
    let mut out = Snapshot::new();
    for path in PATHS {
        out.insert((*path).to_string(), SnapValue::Flag(get(path) == Some(&Value::Bool(true))));
    }
    for path in ENUM_PATHS {
        let text = get(path).and_then(Value::as_str).unwrap_or("").to_string();
        out.insert((*path).to_string(), SnapValue::Text(text));
    }
    for path in LIST_PATHS {
        let list = match get(path) {
            Some(Value::Array(items)) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            _ => Vec::new(),
        };
        out.insert((*path).to_string(), SnapValue::List(list));
    }
    out
}

/// The keys `snapshot` would silently drop.
pub fn unknown_keys(live: &Value) -> Vec<String> {
    live.as_object()
        .map(|o| {
            o.keys()
                .filter(|k| !is_known_path(k) && !is_known_enum_path(k) && !is_known_list_path(k))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Tri-state by contract. `None` means "not a `@state:` condition at all",
/// which leaves a plain shell-command `checked` field resolving from the
/// caller's process cache. A path outside the allow-list answers `Some(false)`
/// rather than `None`, so a typo renders an off checkmark instead of falling
/// through to a cache that can never hold it.
pub fn resolve_state(cond: &str, snap: Option<&Snapshot>) -> Option<bool> {
    let mut path = state_path(cond)?;
    if let Some((p, value)) = path.split_once('=') {
        path = p;
        if value.is_empty() {
            return Some(false);
        }
        if is_known_enum_path(path) {
            return Some(matches!(snap.and_then(|s| s.get(path)), Some(SnapValue::Text(t)) if t == value));
        }
        if is_known_list_path(path) {
            return Some(
                matches!(snap.and_then(|s| s.get(path)), Some(SnapValue::List(l)) if l.iter().any(|v| v == value)),
            );
        }
        return Some(false);
    }
    if !is_known_path(path) {
        return Some(false);
    }
    Some(matches!(snap.and_then(|s| s.get(path)), Some(SnapValue::Flag(true))))
}

/// The single decision the row delegate makes: live state wins outright, so a
/// stale cached process result can never paint over the current one.
pub fn checked_for(
    node: Option<&Node>,
    snap: Option<&Snapshot>,
    cond_results: Option<&HashMap<String, bool>>,
) -> bool {
    let Some(node) = node else { return false };
    let Some(checked) = &node.checked else { return false };
    if checked == crate::providers::PACKAGE_INSTALLED {
        return true;
    }
    if let Some(live) = resolve_state(checked, snap) {
        return live;
    }
    cond_results.is_some_and(|c| c.get(&node.id) == Some(&true))
}

/// A fresh map with `id` set, leaving `source` alone.
pub fn with_result(source: &HashMap<String, bool>, id: &str, ok: bool) -> HashMap<String, bool> {
    let mut out = source.clone();
    out.insert(id.to_string(), ok);
    out
}
