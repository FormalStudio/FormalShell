mod common;

use common::*;
use fs_menu::model::{
    Mode, SectionCtx, build_tree, parse_jsonc, prompt_for, section_names, sections_for,
    visible_children,
};
use fs_menu::node::{Entries, Kind, Node};
use serde_json::json;

fn row(id: &str) -> Node {
    Node { id: id.into(), ..Node::default() }
}

fn menu_ctx<'a>() -> SectionCtx<'a> {
    SectionCtx { mode: Some(Mode::Menu), ..SectionCtx::default() }
}

#[test]
fn section_and_prompt_survive_build() {
    let tree = tree_of(json!({
        "apps": { "provider": "apps", "section": "Suggestions", "prompt": "Search apps" },
        "plain": { "label": "Plain" }
    }));
    assert_eq!(tree.nodes["apps"].section.as_deref(), Some("Suggestions"));
    assert_eq!(tree.nodes["apps"].prompt.as_deref(), Some("Search apps"));
    assert_eq!(tree.nodes["plain"].section, None);
    assert_eq!(tree.nodes["plain"].prompt, None);
}

// A user overlay wins per key here like everywhere else, which is what makes
// regrouping the root a one-line menu.jsonc edit.
#[test]
fn user_overlay_moves_a_row_between_groups() {
    let tree = build_tree(
        &entries(json!({ "apps": { "provider": "apps", "section": "Suggestions" } })),
        &entries(json!({ "apps": { "section": "Mine" } })),
    );
    assert_eq!(tree.nodes["apps"].section.as_deref(), Some("Mine"));
}

#[test]
fn declared_section_wins_at_the_root() {
    let rows = [
        Node { section: Some("Suggestions".into()), ..row("apps") },
        Node { section: Some("Suggestions".into()), ..row("emoji") },
        row("tray"),
    ];
    let sections = sections_for(&rows, &menu_ctx());
    assert_eq!(sections, ["Suggestions", "Suggestions", "Commands"]);
    assert_eq!(section_names(&sections), ["Suggestions", "Commands"]);
}

// Inside a level the rows are that level's, so it names them, and the
// frecency head of a provider that marks one leads under Recent.
#[test]
fn level_names_its_own_rows() {
    let rows = [
        Node { recent: true, ..row("apps.a") },
        Node { recent: true, ..row("apps.b") },
        row("apps.c"),
        row("apps.d"),
    ];
    let ctx = SectionCtx { level: Some("apps"), level_label: "Apps", ..menu_ctx() };
    let sections = sections_for(&rows, &ctx);
    assert_eq!(sections, ["Recent", "Recent", "Apps", "Apps"]);
    assert_eq!(section_names(&sections), ["Recent", "Apps"]);
}

// A ranked list is one list in score order under no heading: a heading per
// root route would cut it back into the groups the ranking no longer makes.
#[test]
fn a_query_draws_no_headings() {
    let mut tree = tree_of(json!({
        "apps": { "provider": "apps", "section": "Suggestions" },
        "emoji": { "label": "Emoji", "section": "Suggestions" },
        "system": {},
        "system.power": {},
        "system.power.reboot": { "label": "Reboot" }
    }));
    let app = Node { parent_id: Some("apps".into()), ..Node::new("apps.org.gnome.Nautilus", "Files", Kind::App) };
    tree.nodes.insert(app.id.clone(), app.clone());
    let rows = [tree.nodes["emoji"].clone(), tree.nodes["system.power.reboot"].clone(), app];
    let ctx = SectionCtx { searching: true, ..menu_ctx() };
    assert_eq!(sections_for(&rows, &ctx), ["", "", ""]);
}

// The mode token already says "Panels", so a PANELS heading under it
// separates nothing.
#[test]
fn a_single_group_level_draws_no_heading() {
    let rows = [row("panels.a"), row("panels.b")];
    let ctx = SectionCtx { level: Some("panels"), level_label: "Panels", ..menu_ctx() };
    let sections = sections_for(&rows, &ctx);
    assert_eq!(sections, ["", ""]);
    assert!(section_names(&sections).is_empty());
}

// A grid draws no headings, so it must not claim any either. A grid answers
// with nothing at all rather than one "" per row: it has nowhere to put a
// heading, and the delegate that reads this array by index is the row list,
// which a grid route does not render. The empty answer is what keeps a
// 3944-cell emoji browse off three passes over the row list per keystroke.
#[test]
fn a_grid_has_no_headings() {
    let rows = [row("emoji.a"), row("emoji.b")];
    let ctx = SectionCtx { grid: true, level: Some("emoji"), level_label: "Emoji", ..menu_ctx() };
    let sections = sections_for(&rows, &ctx);
    assert!(sections.is_empty());
    assert!(section_names(&sections).is_empty());
}

#[test]
fn dmenu_modes_name_themselves() {
    let rows = [row("select.0"), row("select.1")];
    let select = SectionCtx { mode: Some(Mode::Select), ..SectionCtx::default() };
    assert_eq!(sections_for(&rows, &select), ["Options", "Options"]);
    let input = SectionCtx { mode: Some(Mode::Input), ..SectionCtx::default() };
    assert!(sections_for(&[], &input).is_empty());
    assert!(section_names(&["".to_string(), "".to_string()]).is_empty());
}

#[test]
fn prompt_is_the_nodes_own_or_its_label() {
    let with_prompt = Node { prompt: Some("Search emoji".into()), ..Node::new("e", "Emoji", Kind::Submenu) };
    assert_eq!(prompt_for(Some(&with_prompt)), "Search emoji");
    assert_eq!(prompt_for(Some(&Node::new("t", "Toggles", Kind::Submenu))), "Search Toggles");
    assert_eq!(prompt_for(None), "");
}

// The shipped root: one Suggestions block, then everything else, with no
// heading appearing twice.
#[test]
fn shipped_root_is_two_contiguous_groups() {
    let parsed = parse_jsonc(&read_repo("crates/fs-menu/data/default-menu.jsonc")).expect("parses");
    let tree = build_tree(&fs_menu::model::entries_from_value(&parsed).expect("entries"), &Entries::new());
    let rows: Vec<Node> = visible_children(&tree.nodes, None, &no_conds()).into_iter().cloned().collect();
    assert!(rows.len() > 8);
    let sections = sections_for(&rows, &menu_ctx());
    assert_eq!(section_names(&sections), ["Suggestions", "Commands"]);
    let mut seen: Vec<&str> = Vec::new();
    for (i, s) in sections.iter().enumerate() {
        if i > 0 && *s == sections[i - 1] {
            continue;
        }
        assert!(!seen.contains(&s.as_str()), "section {s} is declared in two blocks");
        seen.push(s);
    }
    assert_eq!(sections[0], "Suggestions");
    assert_eq!(sections[sections.len() - 1], "Commands");
}

// The app grid's cells sit under no heading, and the rows under the grid
// keep the headings they carry anywhere else.
#[test]
fn the_app_grid_leaves_its_cells_unheaded() {
    let rows = [
        Node { kind: Kind::App, ..row("apps.a") },
        Node { kind: Kind::App, ..row("apps.b") },
        Node { section: Some("Suggestions".into()), ..row("apps") },
        row("tray"),
    ];
    let ctx = SectionCtx { cells: 2, ..menu_ctx() };
    let sections = sections_for(&rows, &ctx);
    assert_eq!(sections, ["", "", "Suggestions", "Commands"]);
    assert_eq!(section_names(&sections), ["Suggestions", "Commands"]);
}

// Inside a level whose cells are the whole of it (the apps route), the one
// heading goes: the back chip already names the level.
#[test]
fn a_level_of_cells_alone_draws_no_heading() {
    let rows = [Node { kind: Kind::App, ..row("apps.a") }, Node { kind: Kind::App, ..row("apps.b") }];
    let ctx = SectionCtx { level: Some("apps"), level_label: "Apps", cells: 2, ..menu_ctx() };
    assert!(section_names(&sections_for(&rows, &ctx)).is_empty());
}
