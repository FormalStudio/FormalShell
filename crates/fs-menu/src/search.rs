//! Tiered fuzzy scorer for the launcher's search field.
//!
//! A hit ranks by a pair, compared tier first: `(tier, bonus)`. The bonus
//! (the root bonus, the app demotion, a learned abbreviation's boost) only
//! reorders rows inside a tier, however large it grows. Then shallower
//! depth, then the picks ledger's frecency, then declaration order. The 40
//! best come back as one list in that order.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::abbrev::{Lookup, Source};
use crate::actions::subject_id;
use crate::frecency::{self, Record};
use crate::node::{Kind, Node};

/// A configured or pinned abbreviation for the query.
pub const TIER_SET: i32 = 1400;
/// A learned abbreviation that dominates the query.
pub const TIER_LEARNED: i32 = 1200;
pub const TIER_EXACT: i32 = 1000;
pub const TIER_STARTS_WITH: i32 = 800;
pub const TIER_CONTAINS: i32 = 600;
pub const TIER_ALIAS: i32 = 400;
pub const TIER_TITLE: i32 = 200;
/// The query is the initials of the label's words: `vsc`, Visual Studio Code.
pub const TIER_INITIALS: i32 = 150;
/// Every query character appears in the label in order: `ff`, Firefox.
pub const TIER_SUBSEQUENCE: i32 = 100;
pub const ROOT_BONUS: i32 = 100;
pub const APP_DEMOTION: i32 = 50;
pub const MAX_RESULTS: usize = 40;

pub fn normalize(text: &str) -> String {
    text.to_lowercase()
}

pub fn slug(text: &str) -> String {
    normalize(text).chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).collect()
}

/// True if any word in `text` starts with `needle`: "here" matches "Open
/// your files here" but "iles" does not.
pub fn word_boundary_match(text: &str, needle: &str) -> bool {
    normalize(text)
        .split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit()))
        .filter(|w| !w.is_empty())
        .any(|w| w.starts_with(needle))
}

/// The node's own identifier for the alias tier, with the parent's id prefix
/// stripped. A provider-built node's `parent_id` is the route's own id
/// ("apps") while the id itself can carry dots of its own, so splitting the
/// id on its last dot would cut it in the wrong place; stripping the parent
/// works for provider and jsonc nodes alike.
fn own_id(node: &Node) -> &str {
    match &node.parent_id {
        Some(parent) if !parent.is_empty() => {
            node.id.strip_prefix(parent.as_str()).and_then(|r| r.strip_prefix('.')).unwrap_or(&node.id)
        }
        _ => &node.id,
    }
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty())
}

fn initials(label: &str) -> String {
    words(label).filter_map(|w| w.chars().next()).flat_map(char::to_lowercase).collect()
}

fn subsequence(label: &str, q: &str) -> bool {
    let mut rest = label.chars();
    q.chars().all(|c| rest.any(|l| l == c))
}

/// The match tier alone, 0 for no match. `q_norm` and `q_slug` are the
/// query normalized and slugged once by `rank`, since a query is invariant
/// across the whole tree walk.
pub fn tier(node: &Node, q_norm: &str, q_slug: &str) -> i32 {
    if q_norm.is_empty() {
        return 0;
    }
    let label = normalize(&node.label);
    if label == q_norm {
        TIER_EXACT
    } else if label.starts_with(q_norm) {
        TIER_STARTS_WITH
    } else if label.contains(q_norm) {
        TIER_CONTAINS
    } else if node.aliases.iter().any(|a| normalize(a).contains(q_norm)) || slug(own_id(node)).contains(q_slug) {
        TIER_ALIAS
    } else if word_boundary_match(&node.title, q_norm) {
        TIER_TITLE
    } else if q_norm.chars().count() >= 2 && initials(&node.label) == q_norm {
        TIER_INITIALS
    } else if q_norm.chars().count() >= 2 && subsequence(&label, q_norm) {
        TIER_SUBSEQUENCE
    } else {
        0
    }
}

/// The root bonus and the app demotion.
fn bonus(node: &Node, depth: usize) -> i32 {
    let mut b = 0;
    if depth == 0 {
        b += ROOT_BONUS;
    }
    if node.kind == Kind::App {
        b -= APP_DEMOTION;
    }
    b
}

/// What lifts a hit past its own match: the query's abbreviations and the
/// picks ledger.
#[derive(Clone, Copy, Debug, Default)]
pub struct Boost<'a> {
    pub abbrev: Option<&'a Lookup>,
    pub picks: &'a [Record],
    pub now: f64,
}

/// The id itself plus every dotted prefix of it.
fn ancestry(id: Option<&str>) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut prefix = String::new();
    for part in id.unwrap_or("").split('.') {
        if part.is_empty() {
            continue;
        }
        if !prefix.is_empty() {
            prefix.push('.');
        }
        prefix.push_str(part);
        out.insert(prefix.clone());
    }
    out
}

struct Hit<'a> {
    node: &'a Node,
    tier: i32,
    bonus: i32,
    depth: usize,
    picked: f64,
    decl_index: usize,
}

/// Depth-first walk of `nodes` from the roots, or from `within_id`'s own
/// children inside a mode, scoring every when-visible node against `query`
/// (a node whose `when` is not satisfied is skipped with its whole subtree).
/// Returns the 40 best matches in rank order.
///
/// A `route_only` node's children are reached only while standing inside
/// it (the tray names its items after applications that are already
/// listed). `local_only` is the per-node counterpart: scored only while the
/// node's parent level is open.
pub fn rank<'a>(
    nodes: &'a IndexMap<String, Node>,
    query: &str,
    cond_results: &HashMap<String, bool>,
    within_id: Option<&str>,
) -> Vec<&'a Node> {
    rank_with(nodes, query, cond_results, within_id, Boost::default())
}

pub fn rank_with<'a>(
    nodes: &'a IndexMap<String, Node>,
    query: &str,
    cond_results: &HashMap<String, bool>,
    within_id: Option<&str>,
    boost: Boost,
) -> Vec<&'a Node> {
    let open = ancestry(within_id);
    let q_norm = normalize(query);
    let q_slug = slug(&q_norm);
    let mut hits: Vec<Hit<'a>> = Vec::new();
    let mut decl_index = 0;

    struct Walk<'w, 'a> {
        nodes: &'a IndexMap<String, Node>,
        cond_results: &'w HashMap<String, bool>,
        open: &'w HashSet<String>,
        q_norm: &'w str,
        q_slug: &'w str,
        boost: Boost<'w>,
        picks: &'w HashMap<&'w str, &'w Record>,
    }

    fn visit<'a>(w: &Walk<'_, 'a>, id: &str, depth: usize, decl_index: &mut usize, hits: &mut Vec<Hit<'a>>) {
        let Some(node) = w.nodes.get(id) else { return };
        if node.when.is_some() && w.cond_results.get(&node.id) != Some(&true) {
            return;
        }
        let idx = *decl_index;
        *decl_index += 1;
        let parent_open = node.parent_id.as_ref().is_some_and(|p| w.open.contains(p));
        if !(node.local_only && !parent_open) {
            let id = subject_id(node);
            let abbrev = w.boost.abbrev.filter(|_| node.kind != Kind::Note);
            let leads = abbrev.and_then(|l| l.lead.as_ref()).filter(|(lead, _)| *lead == id).map(|(_, source)| *source);
            let tier = match leads {
                Some(Source::Configured | Source::Pinned) => TIER_SET,
                Some(Source::Learned) => TIER_LEARNED,
                None => tier(node, w.q_norm, w.q_slug),
            };
            if tier > 0 {
                let learned = abbrev.and_then(|l| l.boost.get(&id)).copied().unwrap_or(0);
                let picked = frecency::score_of(w.picks.get(id.as_str()).copied(), w.boost.now);
                hits.push(Hit { node, tier, bonus: bonus(node, depth) + learned, depth, picked, decl_index: idx });
            }
        }
        if node.route_only == Some(true) && !w.open.contains(id) {
            return;
        }
        for cid in &node.child_ids {
            visit(w, cid, depth + 1, decl_index, hits);
        }
    }

    let picks: HashMap<&str, &Record> = boost.picks.iter().map(|r| (r.id.as_str(), r)).collect();
    let walk = Walk { nodes, cond_results, open: &open, q_norm: &q_norm, q_slug: &q_slug, boost, picks: &picks };
    for node in crate::model::direct_children(nodes, within_id) {
        visit(&walk, &node.id, 0, &mut decl_index, &mut hits);
    }

    hits.sort_by(|a, b| {
        b.tier
            .cmp(&a.tier)
            .then(b.bonus.cmp(&a.bonus))
            .then(a.depth.cmp(&b.depth))
            .then(b.picked.partial_cmp(&a.picked).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.decl_index.cmp(&b.decl_index))
    });
    hits.into_iter().take(MAX_RESULTS).map(|h| h.node).collect()
}
