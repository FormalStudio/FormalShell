//! JSONC parsing, the dotted-id tree and the launcher's group headings.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde_json::Value;

use crate::node::{Entries, Entry, Kind, Node, Tree};

/// Strips `//` line comments and trailing commas, then parses.
pub fn parse_jsonc(text: &str) -> Result<Value, serde_json::Error> {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < len {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < len {
                out.push(chars[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < len && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == ',' {
            let mut j = i + 1;
            while j < len {
                let wc = chars[j];
                if matches!(wc, ' ' | '\t' | '\n' | '\r') {
                    j += 1;
                    continue;
                }
                if wc == '/' && chars.get(j + 1) == Some(&'/') {
                    while j < len && chars[j] != '\n' {
                        j += 1;
                    }
                    continue;
                }
                break;
            }
            if j < len && (chars[j] == '}' || chars[j] == ']') {
                i += 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    serde_json::from_str(&out)
}

/// Fast path for a file whose only comments are a leading block of `//`
/// lines: skips them and hands the rest straight to the JSON parser, falling
/// back to `parse_jsonc` on the original text for anything else.
pub fn parse_headered_json(text: &str) -> Result<Value, serde_json::Error> {
    let mut i = 0;
    loop {
        let rest = &text[i..];
        let (line, next) = match rest.find('\n') {
            Some(n) => (&rest[..n], Some(i + n + 1)),
            None => (rest, None),
        };
        if !line.trim_start().starts_with("//") {
            break;
        }
        match next {
            Some(n) => i = n,
            None => {
                i = text.len();
                break;
            }
        }
    }
    serde_json::from_str(&text[i..]).or_else(|_| parse_jsonc(text))
}

/// Parses a JSONC object of dotted-id entries, keeping declaration order.
pub fn entries_from_value(value: &Value) -> Result<Entries, serde_json::Error> {
    match value {
        Value::Object(map) => map
            .iter()
            .map(|(k, v)| Ok((k.clone(), serde_json::from_value::<Entry>(v.clone())?)))
            .collect(),
        _ => Ok(Entries::new()),
    }
}

/// `(parent id, last segment)`; the parent is `None` for an undotted id.
pub fn split_id(id: &str) -> (Option<&str>, &str) {
    match id.rfind('.') {
        None => (None, id),
        Some(dot) => (Some(&id[..dot]), &id[dot + 1..]),
    }
}

pub fn title_case(segment: &str) -> String {
    let mut chars = segment.chars();
    match chars.next() {
        // JS upper-cases a UTF-16 unit at a time; the first char is the same
        // for every segment a menu file can name.
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// An explicit `kind` wins outright; otherwise `action`, `target`, then
/// `provider`, else a submenu.
pub fn infer_kind(entry: &Entry) -> Kind {
    if let Some(kind) = &entry.kind {
        return kind.clone();
    }
    if entry.action.is_some() {
        Kind::Action
    } else if entry.target.is_some() {
        Kind::Link
    } else if entry.provider.is_some() {
        Kind::Provider
    } else {
        Kind::Submenu
    }
}

/// Turns the shipped default and the user override into the hierarchy:
/// dots imply parentage (missing ancestors are created), a user entry
/// overrides the default's fields per key, and `hidden: true` drops that id
/// and its whole subtree.
pub fn build_tree(default_obj: &Entries, user_obj: &Entries) -> Tree {
    let mut declared: Vec<&String> = default_obj.keys().collect();
    for id in user_obj.keys() {
        if !default_obj.contains_key(id) {
            declared.push(id);
        }
    }

    let hidden_ids: HashSet<&str> = declared
        .iter()
        .filter(|id| user_obj.get(id.as_str()).is_some_and(|u| u.hidden == Some(true)))
        .map(|id| id.as_str())
        .collect();

    let under_hidden = |id: &str| {
        let mut prefix = String::new();
        for part in id.split('.') {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(part);
            if hidden_ids.contains(prefix.as_str()) {
                return true;
            }
        }
        false
    };

    let mut all_ids: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for id in &declared {
        if under_hidden(id) {
            continue;
        }
        let mut prefix = String::new();
        for part in id.split('.') {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(part);
            if hidden_ids.contains(prefix.as_str()) {
                break;
            }
            if seen.insert(prefix.clone()) {
                all_ids.push(prefix.clone());
            }
        }
    }

    let empty = Entry::default();
    let mut nodes: IndexMap<String, Node> = IndexMap::new();
    for id in &all_ids {
        let (parent, segment) = split_id(id);
        let entry = default_obj.get(id).unwrap_or(&empty).merged(user_obj.get(id));
        let kind = infer_kind(&entry);
        nodes.insert(
            id.clone(),
            Node {
                id: id.clone(),
                parent_id: parent.map(str::to_string),
                label: entry.label.unwrap_or_else(|| title_case(segment)),
                icon: entry.icon.unwrap_or_default(),
                title: entry.title.unwrap_or_default(),
                aliases: entry.aliases.unwrap_or_default(),
                kind,
                action: entry.action,
                target: entry.target,
                provider: entry.provider,
                when: entry.when,
                checked: entry.checked,
                confirm: entry.confirm,
                dim: entry.dim,
                keep_open: entry.keep_open,
                route_only: entry.route_only,
                section: entry.section,
                prompt: entry.prompt,
                prefix: entry.prefix,
                actions: entry.actions.unwrap_or_default(),
                alternate: entry.alternate,
                alternate_label: entry.alternate_label,
                ..Node::default()
            },
        );
    }

    let mut root_ids = Vec::new();
    for id in &all_ids {
        let parent = nodes[id].parent_id.clone();
        match parent {
            Some(p) if nodes.contains_key(&p) => {
                nodes[&p].child_ids.push(id.clone());
            }
            _ => root_ids.push(id.clone()),
        }
    }
    Tree { root_ids, nodes }
}

/// The heading a level's rows fall back to when nothing declares one.
pub const ROOT_SECTION: &str = "Commands";

/// The root's heading at rest over the best picks.
pub const RECENT_SECTION: &str = "Recent";

/// The root's heading at rest on a profile with too few picks for `Recent`.
pub const MODES_SECTION: &str = "Modes";

/// Fewer resolving picks than this and the root lists its modes.
pub const RECENT_MIN: usize = 3;

/// The node a pick, an abbreviation or `Recent` names, while it is still in
/// the tree: `app:<desktop id>` is the app under `apps`.
pub fn subject_node<'a>(nodes: &'a IndexMap<String, Node>, id: &str) -> Option<&'a Node> {
    match id.strip_prefix("app:") {
        Some(app) => nodes.get(&format!("apps.{app}")),
        None => nodes.get(id),
    }
}

/// What the root lists with an empty field: the subjects pinned to it,
/// then the best picks, that still resolve and pass their `when`, any kind,
/// `limit` in all, under `Recent`; with no pin and fewer than
/// [`RECENT_MIN`] picks, the root's routes in declaration order under
/// `Modes`. Never both, and nothing else.
pub fn root_rest(
    nodes: &IndexMap<String, Node>,
    pinned: &[String],
    picks: &[crate::frecency::Record],
    now_ms: f64,
    limit: usize,
    cond_results: &HashMap<String, bool>,
) -> Vec<Node> {
    let mut ranked: Vec<&crate::frecency::Record> = picks.iter().filter(|r| crate::frecency::score_of(Some(r), now_ms) > 0.0).collect();
    ranked.sort_by(|a, b| {
        let (sa, sb) = (crate::frecency::score_of(Some(a), now_ms), crate::frecency::score_of(Some(b), now_ms));
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });
    let visible = |id: &str| subject_node(nodes, id).filter(|n| is_when_visible(n, cond_results));
    let pins: Vec<&Node> = pinned.iter().filter_map(|id| visible(id)).collect();
    let mut seen = HashSet::new();
    let recent: Vec<Node> = pins
        .iter()
        .copied()
        .chain(ranked.into_iter().filter_map(|r| visible(&r.id)))
        .filter(|n| seen.insert(n.id.clone()))
        .take(limit)
        .map(|n| Node { section: Some(RECENT_SECTION.into()), ..n.clone() })
        .collect();
    if !pins.is_empty() || recent.len() >= RECENT_MIN {
        return recent;
    }
    visible_children(nodes, None, cond_results)
        .into_iter()
        .filter(|n| matches!(n.kind, Kind::Submenu | Kind::Provider | Kind::Link))
        .map(|n| Node { section: Some(MODES_SECTION.into()), ..n.clone() })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Menu,
    Select,
    Input,
}

/// What `sections_for` needs to know about the surface the rows sit in.
#[derive(Clone, Debug, Default)]
pub struct SectionCtx<'a> {
    pub mode: Option<Mode>,
    pub grid: bool,
    /// How many leading rows are the app grid's cells.
    pub cells: usize,
    pub searching: bool,
    pub level: Option<&'a str>,
    pub level_label: &'a str,
}

/// One heading per row, index-aligned with `rows`. The launcher draws a
/// heading wherever the value changes. A grid has nowhere to draw a
/// full-width band between cells, so it gets an empty vector rather than one
/// empty string per row (the emoji grid browses 3944 rows per keystroke).
pub fn sections_for(rows: &[Node], ctx: &SectionCtx) -> Vec<String> {
    if ctx.grid {
        return Vec::new();
    }
    let out: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            if i < ctx.cells {
                String::new()
            } else {
                section_of(row, ctx)
            }
        })
        .collect();
    collapse_single_level_group(out, ctx)
}

/// A level whose rows are all one group is already named by the mode token,
/// so its heading goes. A level that splits keeps every one.
fn collapse_single_level_group(sections: Vec<String>, ctx: &SectionCtx) -> Vec<String> {
    if ctx.level.is_none() || ctx.searching {
        return sections;
    }
    if ctx.mode.is_some_and(|m| m != Mode::Menu) {
        return sections;
    }
    let mut only = "";
    for s in &sections {
        if s.is_empty() {
            continue;
        }
        if !only.is_empty() && s != only {
            return sections;
        }
        only = s;
    }
    vec![String::new(); sections.len()]
}

fn section_of(row: &Node, ctx: &SectionCtx) -> String {
    match ctx.mode {
        Some(Mode::Select) => return "Options".to_string(),
        Some(Mode::Input) => return String::new(),
        _ => {}
    }
    if ctx.searching {
        return String::new();
    }
    if row.recent {
        return "Recent".to_string();
    }
    if let Some(section) = row.section.as_deref().filter(|s| !s.is_empty()) {
        return section.to_string();
    }
    if ctx.level.is_none() {
        return ROOT_SECTION.to_string();
    }
    ctx.level_label.to_string()
}

/// The distinct headings of a `sections_for` result, in first-seen order.
pub fn section_names(sections: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in sections {
        if !s.is_empty() && !out.contains(s) {
            out.push(s.clone());
        }
    }
    out
}

/// The route a query's leading `prefix` and a space name, and the query
/// after them.
pub fn prefixed<'a, 'q>(nodes: &'a IndexMap<String, Node>, text: &'q str) -> Option<(&'a Node, &'q str)> {
    nodes.values().find_map(|n| {
        let prefix = n.prefix.as_deref().filter(|p| !p.is_empty())?;
        text.strip_prefix(prefix)?.strip_prefix(' ').map(|rest| (n, rest))
    })
}

/// The labels from the outermost mode down to `level`.
pub fn mode_path(nodes: &IndexMap<String, Node>, level: Option<&str>) -> Vec<String> {
    let mut path = Vec::new();
    let mut at = level.and_then(|l| nodes.get(l));
    while let Some(node) = at {
        path.push(node.label.clone());
        at = node.parent_id.as_deref().and_then(|p| nodes.get(p));
    }
    path.reverse();
    path
}

/// What the empty search field says a level is for: the level's own `prompt`
/// key, or "Search <label>".
pub fn prompt_for(node: Option<&Node>) -> String {
    let Some(node) = node else { return String::new() };
    match node.prompt.as_deref() {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => format!("Search {}", node.label),
    }
}

pub fn is_when_visible(node: &Node, cond_results: &HashMap<String, bool>) -> bool {
    node.when.is_none() || cond_results.get(&node.id) == Some(&true)
}

/// The placeholder for a level whose own `when` gate is not satisfied but
/// which was summoned directly: never activatable.
pub fn gated_note_row(node: &Node) -> Node {
    Node {
        parent_id: Some(node.id.clone()),
        ..Node::note(format!("{}.unavailable", node.id), "Unavailable")
    }
}

/// A link's own subtree is empty by construction, so pruning follows the
/// target's children rather than the link node's.
pub fn direct_children<'a>(nodes: &'a IndexMap<String, Node>, id: Option<&str>) -> Vec<&'a Node> {
    let Some(id) = id else {
        return nodes.values().filter(|n| n.parent_id.is_none()).collect();
    };
    let Some(node) = nodes.get(id) else { return Vec::new() };
    let child_ids = match (&node.kind, &node.target) {
        (Kind::Link, Some(target)) => match nodes.get(target) {
            Some(t) => &t.child_ids,
            None => &node.child_ids,
        },
        _ => &node.child_ids,
    };
    child_ids.iter().filter_map(|cid| nodes.get(cid)).collect()
}

/// The children of `id` that are visible: their `when` resolved true, and a
/// submenu or link only while something under it is visible. A link cycle
/// bottoms out as "no visible children".
pub fn visible_children<'a>(
    nodes: &'a IndexMap<String, Node>,
    id: Option<&str>,
    cond_results: &HashMap<String, bool>,
) -> Vec<&'a Node> {
    visible_children_in(nodes, id, cond_results, &HashSet::new())
}

fn visible_children_in<'a>(
    nodes: &'a IndexMap<String, Node>,
    id: Option<&str>,
    cond_results: &HashMap<String, bool>,
    ancestry: &HashSet<String>,
) -> Vec<&'a Node> {
    let mut path = ancestry.clone();
    if let Some(id) = id {
        if path.contains(id) {
            return Vec::new();
        }
        path.insert(id.to_string());
    }
    direct_children(nodes, id)
        .into_iter()
        .filter(|child| {
            if !is_when_visible(child, cond_results) {
                return false;
            }
            if matches!(child.kind, Kind::Submenu | Kind::Link) {
                return !visible_children_in(nodes, Some(&child.id), cond_results, &path).is_empty();
            }
            true
        })
        .collect()
}
