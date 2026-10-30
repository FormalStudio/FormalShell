//! The apps route and the machinery that parents a provider's rows under its
//! node.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::frecency::{self, Record};
use crate::node::{DesktopEntry, Kind, Node, Tree};

/// `recent` marks the head of the frecency order for the launcher's own
/// `Recent` heading. It is capped rather than "everything with a launch
/// record": the score of a launch decays but never reaches zero, so an
/// uncapped rule would eventually file every app anyone has ever opened under
/// Recent and leave the heading meaning nothing. Five is what a reader takes
/// in without scanning, and the rows below it are the whole list in the same
/// order, so nothing is hidden by the split.
pub const APPS_RECENT_MAX: usize = 5;

pub(crate) fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// One row per desktop entry, ordered by launch frecency before the map,
/// which is the whole of frecency's reach into ranking: equal-score search
/// ties break by declaration order, and these rows are declared in exactly
/// the order returned here. So a launched app leads a tie against an app that
/// matched the query exactly as well, while a stronger match tier still beats
/// any launch count outright. An empty ledger leaves the desktop entries' own
/// order intact.
///
/// `entry.icon` is an icon-theme NAME, never a glyph, so it must not land in
/// the node's `icon` slot, which renders as literal text. `resolve_icon` maps
/// the name to an image URL for the row's image slot, or "" when the theme has
/// none; the row then simply has no leading cell. `entry` stays on the row:
/// activation reads `startup_class` and `id` off it to decide whether to focus
/// a running window instead of launching.
pub fn apps_provider(
    entries: &[DesktopEntry],
    resolve_icon: Option<&dyn Fn(&str) -> String>,
    launches: &[Record],
    now: Option<f64>,
) -> Vec<Node> {
    let now = now.unwrap_or_else(now_ms);
    let ordered = frecency::order(entries.to_vec(), launches, now, |e| e.id.as_str());
    ordered
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let icon_source = match resolve_icon {
                Some(resolve) if !entry.icon.is_empty() => resolve(&entry.icon),
                _ => String::new(),
            };
            let label = if entry.name.is_empty() { entry.id.clone() } else { entry.name.clone() };
            Node {
                title: entry.generic_name.clone(),
                icon_source,
                recent: i < APPS_RECENT_MAX && frecency::score(launches, &entry.id, now) > 0.0,
                entry: Some(entry.clone()),
                ..Node::new(format!("apps.{}", entry.id), label, Kind::App)
            }
        })
        .collect()
}

/// A provider's produced rows, computed on demand.
pub type ProviderFn<'a> = Box<dyn Fn() -> Vec<Node> + 'a>;

/// Parents `children` under the node `provider_id`, registers them in
/// `tree.nodes` and appends them to its `child_ids`. This is the one-source
/// primitive: a caller that recomputed a single provider's rows attaches just
/// those. A child that already names a parent is one level further down, under
/// a node earlier in the same batch that lists it in its own `child_ids`.
///
/// A child whose id `menu.jsonc` already declared takes the declared
/// node's place, keeping that node's `actions` over its own.
pub fn attach_children(tree: &mut Tree, provider_id: &str, children: Vec<Node>) {
    for mut child in children {
        let declared = match tree.nodes.get(&child.id) {
            Some(d) => {
                child.actions = crate::node::merge_actions(&child.actions, &d.actions);
                true
            }
            None => false,
        };
        if child.parent_id.is_none() {
            child.parent_id = Some(provider_id.to_string());
            if let Some(node) = tree.nodes.get_mut(provider_id)
                && !(declared && node.child_ids.contains(&child.id))
            {
                node.child_ids.push(child.id.clone());
            }
        }
        tree.nodes.insert(child.id.clone(), child);
    }
}

/// Merges every "provider" node's produced children into `tree` (a
/// `build_tree` result): for each provider node it looks up its `provider`
/// name in `provider_fns`, parents the returned nodes under it, and registers
/// them in `tree.nodes`. Provider nodes the walk creates are not themselves
/// expanded.
pub fn apply_providers(tree: &mut Tree, provider_fns: &HashMap<String, ProviderFn>) {
    let targets: Vec<(String, String)> = tree
        .nodes
        .values()
        .filter(|n| n.kind == Kind::Provider)
        .filter_map(|n| n.provider.clone().map(|p| (n.id.clone(), p)))
        .collect();
    for (id, provider) in targets {
        if let Some(f) = provider_fns.get(&provider) {
            attach_children(tree, &id, f());
        }
    }
}
