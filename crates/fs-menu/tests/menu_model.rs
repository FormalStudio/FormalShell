mod common;

use std::collections::HashMap;

use common::*;
use fs_menu::model::{
    build_tree, gated_note_row, parse_headered_json, parse_jsonc, visible_children,
};
use fs_menu::node::{Entries, Kind, Node};
use serde_json::json;

#[test]
fn dotted_id_hierarchy_with_auto_parents() {
    let tree = tree_of(json!({ "system.power.reboot": { "label": "Reboot", "action": "systemctl reboot" } }));
    assert_eq!(tree.root_ids, ["system"]);

    let system = &tree.nodes["system"];
    assert_eq!(system.kind, Kind::Submenu);
    assert_eq!(system.parent_id, None);
    assert_eq!(system.label, "System");
    assert_eq!(system.child_ids, ["system.power"]);

    let power = &tree.nodes["system.power"];
    assert_eq!(power.kind, Kind::Submenu);
    assert_eq!(power.parent_id.as_deref(), Some("system"));
    assert_eq!(power.label, "Power");
    assert_eq!(power.child_ids, ["system.power.reboot"]);

    let reboot = &tree.nodes["system.power.reboot"];
    assert_eq!(reboot.parent_id.as_deref(), Some("system.power"));
    assert_eq!(reboot.label, "Reboot");
    assert!(reboot.child_ids.is_empty());
}

#[test]
fn kind_inference_for_all_four_kinds() {
    let tree = tree_of(json!({
        "a": { "label": "Action", "action": "echo hi" },
        "b": { "label": "Link", "target": "a" },
        "c": { "label": "Provider", "provider": "apps" },
        "d": { "label": "Submenu" }
    }));
    assert_eq!(tree.nodes["a"].kind, Kind::Action);
    assert_eq!(tree.nodes["a"].action.as_deref(), Some("echo hi"));
    assert_eq!(tree.nodes["b"].kind, Kind::Link);
    assert_eq!(tree.nodes["b"].target.as_deref(), Some("a"));
    assert_eq!(tree.nodes["c"].kind, Kind::Provider);
    assert_eq!(tree.nodes["c"].provider.as_deref(), Some("apps"));
    assert_eq!(tree.nodes["d"].kind, Kind::Submenu);
}

// The "note"/`dim` escape hatch: an explicit `kind` wins over inference from
// action/target/provider, and `dim` rides along. This is the shape a
// dynamically-injected root entry needs for an honest, non-activatable empty
// row that static jsonc's action/target/provider fields cannot express.
#[test]
fn explicit_kind_override_and_dim_passthrough() {
    let tree = tree_of(json!({ "a": { "label": "Nothing to share", "kind": "note", "dim": true } }));
    assert_eq!(tree.nodes["a"].kind, Kind::Note);
    assert_eq!(tree.nodes["a"].dim, Some(true));
}

// `keepOpen` (toggle hub): an action row that leaves the menu open after
// activation, carried by build_tree and overridable per-key like any other
// field.
#[test]
fn keep_open_passthrough() {
    let def = entries(json!({ "a": { "label": "A", "action": "@ipc:x", "keepOpen": true } }));
    let tree = build_tree(&def, &Entries::new());
    assert_eq!(tree.nodes["a"].kind, Kind::Action);
    assert_eq!(tree.nodes["a"].keep_open, Some(true));
    let overridden = build_tree(&def, &entries(json!({ "a": { "keepOpen": false } })));
    assert_eq!(overridden.nodes["a"].keep_open, Some(false));
}

#[test]
fn user_override_of_default_label() {
    let def = entries(json!({ "apps": { "label": "Applications", "icon": "" } }));
    let user = entries(json!({ "apps": { "label": "My Apps" } }));
    assert_eq!(build_tree(&def, &user).nodes["apps"].label, "My Apps");
}

#[test]
fn hidden_removes_a_subtree() {
    let def = entries(json!({
        "system": { "label": "System" },
        "system.power": { "label": "Power" },
        "system.power.reboot": { "label": "Reboot", "action": "systemctl reboot" }
    }));
    let user = entries(json!({ "system.power": { "hidden": true } }));
    let tree = build_tree(&def, &user);
    assert!(!tree.nodes.contains_key("system.power"));
    assert!(!tree.nodes.contains_key("system.power.reboot"));
    assert!(tree.nodes.contains_key("system"));
    assert!(tree.nodes["system"].child_ids.is_empty());
}

#[test]
fn self_pruning_cascade() {
    let tree = tree_of(json!({
        "system": { "label": "System" },
        "system.power": { "label": "Power" },
        "system.power.reboot": { "label": "Reboot", "action": "systemctl reboot", "when": "false" }
    }));
    let cond = HashMap::from([("system.power.reboot".to_string(), false)]);
    // the leaf itself is hidden by its condition...
    assert_eq!(visible_children(&tree.nodes, Some("system.power"), &cond).len(), 0);
    // ...which empties its parent submenu...
    assert_eq!(visible_children(&tree.nodes, Some("system"), &cond).len(), 0);
    // ...which empties the root list.
    assert_eq!(visible_children(&tree.nodes, None, &cond).len(), 0);
}

#[test]
fn visible_children_no_when_is_always_visible() {
    let tree = tree_of(json!({
        "apps": { "label": "Apps", "action": "true" },
        "system": { "label": "System", "action": "true" }
    }));
    assert_eq!(visible_children(&tree.nodes, None, &no_conds()).len(), 2);
}

#[test]
fn visible_children_when_true_shows_node() {
    let tree = tree_of(json!({
        "system": { "label": "System" },
        "system.lock": { "label": "Lock", "action": "loginctl lock-session", "when": "true" }
    }));
    let cond = HashMap::from([("system.lock".to_string(), true)]);
    let visible = visible_children(&tree.nodes, Some("system"), &cond);
    assert_eq!(ids(&visible), ["system.lock"]);
}

#[test]
fn parse_jsonc_strips_line_comments_and_trailing_commas() {
    let text = ["{", "  // a leading comment", "  \"a\": 1,", "  \"b\": [1, 2, ],", "}"].join("\n");
    let obj = parse_jsonc(&text).expect("parses");
    assert_eq!(obj["a"], 1);
    assert_eq!(obj["b"], json!([1, 2]));
}

#[test]
fn parse_jsonc_preserves_comment_like_text_inside_strings() {
    let obj = parse_jsonc(r#"{ "note": "see http://example.com for // details, still here" }"#).expect("parses");
    assert_eq!(obj["note"], "see http://example.com for // details, still here");
}

#[test]
fn visible_children_survives_mutually_referencing_links() {
    let tree = tree_of(json!({
        "a": { "label": "A" },
        "a.toB": { "label": "To B", "target": "b" },
        "b": { "label": "B" },
        "b.toA": { "label": "To A", "target": "a" }
    }));
    // Must terminate instead of recursing forever between the two links; a
    // cycle with no other content bottoms out as invisible.
    assert_eq!(visible_children(&tree.nodes, None, &no_conds()).len(), 0);
}

#[test]
fn parse_jsonc_errors_on_hard_syntax_error() {
    assert!(parse_jsonc(r#"{ "a": }"#).is_err());
}

#[test]
fn parse_headered_json_skips_a_leading_comment_block() {
    let text = ["// generated, do not edit", "// second header line", r#"[{"a": 1}, {"a": 2}]"#].join("\n");
    let arr = parse_headered_json(&text).expect("parses");
    assert_eq!(arr, json!([{ "a": 1 }, { "a": 2 }]));
}

// No leading comment block, and a comment inside the body instead: the fast
// path's plain JSON parse cannot handle that, so it has to fall back to
// parse_jsonc rather than failing.
#[test]
fn parse_headered_json_falls_back_for_a_comment_inside_the_body() {
    let obj = parse_headered_json("{ // inline\n  \"a\": 1 }").expect("parses");
    assert_eq!(obj["a"], 1);
}

// gated_note_row: the route-summon when-gate guard's honest placeholder row.
#[test]
fn gated_note_row_is_a_non_activatable_dim_note_under_the_node() {
    let row = gated_note_row(&Node::new("share", "Share", Kind::Submenu));
    assert_eq!(row.id, "share.unavailable");
    assert_eq!(row.parent_id.as_deref(), Some("share"));
    assert_eq!(row.label, "Unavailable");
    assert_eq!(row.kind, Kind::Note);
    assert_eq!(row.dim, Some(true));
    assert!(row.child_ids.is_empty());
}

fn rest_tree() -> fs_menu::node::Tree {
    let mut tree = tree_of(json!({
        "apps": { "provider": "apps" },
        "clipboard": { "provider": "clipboard" },
        "system": { "label": "System" },
        "system.suspend": { "label": "Suspend", "action": "@ipc:power.suspend" },
        "toggles": { "label": "Toggles" },
        "toggles.dnd": { "label": "Do Not Disturb", "action": "@ipc:notifications.toggleDnd" },
        "hidden": { "label": "Hidden", "provider": "x", "when": "false" },
        "run": { "label": "Run", "action": "true" }
    }));
    let app = Node { parent_id: Some("apps".into()), ..Node::new("apps.firefox", "Firefox", Kind::App) };
    tree.nodes.insert(app.id.clone(), app);
    tree.nodes.get_mut("apps").unwrap().child_ids.push("apps.firefox".into());
    tree
}

fn pick(id: &str, count: f64) -> fs_menu::frecency::Record {
    fs_menu::frecency::Record::new(id, count, 1_000.0)
}

#[test]
fn root_rest_lists_modes_on_a_fresh_profile() {
    let tree = rest_tree();
    let rows = fs_menu::model::root_rest(&tree.nodes, &[pick("app:firefox", 1.0)], 1_000.0, 8, &HashMap::new());
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["apps", "clipboard", "system", "toggles"]);
    assert!(rows.iter().all(|r| r.section.as_deref() == Some(fs_menu::model::MODES_SECTION)));
}

#[test]
fn root_rest_lists_the_best_picks_of_any_kind_skipping_the_gone() {
    let tree = rest_tree();
    let picks = [
        pick("app:firefox", 5.0),
        pick("app:uninstalled", 9.0),
        pick("clipboard", 3.0),
        pick("system.suspend", 4.0),
        pick("hidden", 8.0),
        pick("toggles.dnd", 1.0),
    ];
    let rows = fs_menu::model::root_rest(&tree.nodes, &picks, 1_000.0, 3, &HashMap::new());
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["apps.firefox", "system.suspend", "clipboard"]);
    assert!(rows.iter().all(|r| r.section.as_deref() == Some(fs_menu::model::RECENT_SECTION)));
}

#[test]
fn root_rest_falls_back_to_modes_when_too_few_picks_resolve() {
    let tree = rest_tree();
    let picks = [pick("app:firefox", 5.0), pick("app:gone", 9.0), pick("gone.too", 9.0), pick("clipboard", 1.0)];
    let rows = fs_menu::model::root_rest(&tree.nodes, &picks, 1_000.0, 8, &HashMap::new());
    assert!(rows.iter().all(|r| r.section.as_deref() == Some(fs_menu::model::MODES_SECTION)));
}
