// The SHARE route's pure logic: share_entry_command's text/image split (the GUI
// fallback's launch command), share_peer_entries' dynamic subtree in both its
// shapes (localsend-cli present: peers under Send, a Receive status line;
// absent: the GUI shape), and clipboard_provider's "share" mode reusing the
// "copy" rows. Tree wiring and activation are out of scope.

mod common;

use std::collections::HashMap;

use chrono::{Local, TimeZone};
use common::*;
use fs_menu::clipboard::history::{Entry, EntryKind};
use fs_menu::model::{build_tree, visible_children};
use fs_menu::node::{Entries, Kind};
use fs_menu::providers::{ClipMode, Peer, ReceiveStatus, clipboard_provider, share_entry_command, share_peer_entries};
use serde_json::json;

fn local_ms(h: u32, m: u32) -> i64 {
    Local.with_ymd_and_hms(2026, 1, 1, h, m, 0).unwrap().timestamp_millis()
}

fn peer(name: &str) -> Peer {
    Peer { name: name.into() }
}

#[test]
fn text_entry_writes_a_mktemp_file_and_shares_its_path() {
    assert_eq!(
        share_entry_command(&Entry::text("a", "hello world")),
        "tmp=$(mktemp --suffix=.txt) && printf '%s' 'hello world' > \"$tmp\" && exec localsend_app \"$tmp\""
    );
}

#[test]
fn text_entry_escapes_embedded_single_quotes() {
    assert_eq!(
        share_entry_command(&Entry::text("a", "it's here")),
        "tmp=$(mktemp --suffix=.txt) && printf '%s' 'it'\\''s here' > \"$tmp\" && exec localsend_app \"$tmp\""
    );
}

#[test]
fn image_entry_shares_its_content_addressed_path_directly() {
    assert_eq!(
        share_entry_command(&Entry::image("b", "/state/clipboard-images/abc.png")),
        "localsend_app '/state/clipboard-images/abc.png'"
    );
}

// --- fallback shape (installed=false, the GUI path) ---------------------------

#[test]
fn fallback_empty_clipboard_maps_share_send_to_a_dim_note_row() {
    let out = share_peer_entries(false, &[], &[], None);
    let node = out.get("share.send").expect("share.send");
    assert_eq!(node.label.as_deref(), Some("Nothing to share"));
    assert_eq!(node.kind, Some(Kind::Note));
    assert_eq!(node.dim, Some(true));
    assert_eq!(node.action, None);
}

#[test]
fn fallback_newest_entry_maps_share_send_to_a_launch_action() {
    let out = share_peer_entries(false, &[], &[Entry::text("a", "newest"), Entry::text("b", "older")], None);
    assert_eq!(out["share.send"].label.as_deref(), Some("Send"));
    assert_eq!(
        out["share.send"].action.as_deref(),
        Some("tmp=$(mktemp --suffix=.txt) && printf '%s' 'newest' > \"$tmp\" && exec localsend_app \"$tmp\"")
    );
}

#[test]
fn fallback_receive_launches_the_gui_and_history_stays_a_provider_node() {
    let out = share_peer_entries(false, &[], &[], None);
    assert_eq!(out["share.receive"].action.as_deref(), Some("localsend_app"));
    assert_eq!(out["share.history"].provider.as_deref(), Some("shareHistory"));
}

// --- CLI shape (installed=true) ----------------------------------------------

#[test]
fn installed_no_peers_is_an_honest_empty_send() {
    let out = share_peer_entries(true, &[], &[], None);
    assert_eq!(out["share.send"].label.as_deref(), Some("Send"));
    let empty = &out["share.send.empty"];
    assert_eq!(empty.kind, Some(Kind::Note));
    assert_eq!(empty.dim, Some(true));
    assert!(!out.contains_key("share.send.0"));
}

#[test]
fn installed_peer_gets_a_folder_with_clipboard_and_image_rows() {
    let peers = [peer("Kyan's iPhone")];
    let items = [
        Entry::text("t1", "hi"),
        Entry { captured_at: Some(local_ms(9, 5)), ..Entry::image("i1", "/img/one.png") },
    ];
    let out = share_peer_entries(true, &peers, &items, None);
    let folder = &out["share.send.0"];
    assert_eq!(folder.label.as_deref(), Some("Kyan's iPhone"));
    assert_eq!(folder.action, None); // a folder, not itself activatable

    let clip = &out["share.send.0.clipboard"];
    assert_eq!(clip.label.as_deref(), Some("Clipboard"));
    assert_eq!(clip.action.as_deref(), Some("@ipc:localsend.send:0:t1"));

    let img = &out["share.send.0.image.0"];
    assert_eq!(img.label.as_deref(), Some("Image"));
    assert_eq!(img.desc.as_deref(), Some("09:05"));
    assert_eq!(img.thumb_source.as_deref(), Some("/img/one.png"));
    assert_eq!(img.action.as_deref(), Some("@ipc:localsend.send:0:i1"));
}

#[test]
fn installed_peer_with_only_images_gets_no_clipboard_row() {
    let items = [Entry { captured_at: Some(0), ..Entry::image("i1", "/img/one.png") }];
    let out = share_peer_entries(true, &[peer("Desk")], &items, None);
    assert!(!out.contains_key("share.send.0.clipboard"));
    assert!(out.contains_key("share.send.0.image.0"));
}

#[test]
fn installed_peer_with_nothing_to_share_gets_a_dim_note() {
    let out = share_peer_entries(true, &[peer("Desk")], &[], None);
    let empty = &out["share.send.0.empty"];
    assert_eq!(empty.kind, Some(Kind::Note));
    assert_eq!(empty.dim, Some(true));
}

#[test]
fn installed_multiple_peers_each_get_their_own_index() {
    let out = share_peer_entries(true, &[peer("A"), peer("B")], &[Entry::text("t1", "hi")], None);
    assert_eq!(out["share.send.0"].label.as_deref(), Some("A"));
    assert_eq!(out["share.send.0.clipboard"].action.as_deref(), Some("@ipc:localsend.send:0:t1"));
    assert_eq!(out["share.send.1"].label.as_deref(), Some("B"));
    assert_eq!(out["share.send.1.clipboard"].action.as_deref(), Some("@ipc:localsend.send:1:t1"));
}

// --- receive status line -------------------------------------------------------

// `dim` stays false on every branch (never used to grey the row): a dim "note"
// sitting next to "share.send" would be dropped outright by the launcher's own
// live-row filter, which treats a dim note as "this level is empty" rather than
// "muted status line" (the bug this caught in the VM smoke rig, --share's
// screenshot showing Send with no Receive row at all until this was fixed).
fn receive(enabled: bool, receiving: bool) -> ReceiveStatus {
    ReceiveStatus { enabled, receiving, alias: "kyan-laptop".into(), dir: "/home/kyan/Downloads".into() }
}

#[test]
fn receive_status_off_by_config() {
    let status = ReceiveStatus { enabled: false, receiving: false, alias: String::new(), dir: String::new() };
    let out = share_peer_entries(true, &[], &[], Some(&status));
    let node = &out["share.receive"];
    assert_eq!(node.kind, Some(Kind::Note));
    assert_eq!(node.dim, Some(false));
    assert_eq!(node.desc.as_deref(), Some("Off (set localsend.receive: true)"));
    assert_eq!(node.action, None);
}

#[test]
fn receive_status_listening() {
    let out = share_peer_entries(true, &[], &[], Some(&receive(true, true)));
    let node = &out["share.receive"];
    assert_eq!(node.dim, Some(false));
    assert_eq!(node.desc.as_deref(), Some("Listening as kyan-laptop, saving to /home/kyan/Downloads"));
}

#[test]
fn receive_status_starting_after_a_crash_or_at_boot() {
    let out = share_peer_entries(true, &[], &[], Some(&receive(true, false)));
    let node = &out["share.receive"];
    assert_eq!(node.dim, Some(false));
    assert_eq!(node.desc.as_deref(), Some("Starting\u{2026}"));
}

// Nothing under "share" is declared in default-menu.jsonc any more (the whole
// subtree is dynamic), so this proves build_tree auto-parents
// dynamically-generated dotted ids at any depth exactly like a statically
// declared one.
#[test]
fn merged_tree_auto_parents_the_dynamic_subtree() {
    let mut def = entries(json!({ "share": { "label": "Share" } }));
    let status = ReceiveStatus { enabled: true, receiving: true, alias: "x".into(), dir: "/d".into() };
    let dynamic = share_peer_entries(true, &[peer("A")], &[Entry::text("t1", "hi")], Some(&status));
    for (k, v) in dynamic {
        def.insert(k, v);
    }
    let tree = build_tree(&def, &Entries::new());
    let share = &tree.nodes["share"];
    assert!(share.child_ids.contains(&"share.receive".to_string()));
    assert!(share.child_ids.contains(&"share.send".to_string()));
    let send_folder = &tree.nodes["share.send.0"];
    assert_eq!(send_folder.kind, Kind::Submenu);
    assert!(send_folder.child_ids.contains(&"share.send.0.clipboard".to_string()));
    assert_eq!(tree.nodes["share.send.0.clipboard"].kind, Kind::Action);
    assert_eq!(tree.nodes["share.receive"].kind, Kind::Note);
}

#[test]
fn share_mode_reuses_copy_rows_with_a_distinct_id_namespace() {
    let nodes = clipboard_provider(&[Entry::text("a", "hello")], ClipMode::Share, true);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, "share.history.a");
    assert_eq!(nodes[0].label, "hello");
    assert_eq!(nodes[0].kind, Kind::Action);
    assert_eq!(
        nodes[0].action.as_deref(),
        Some("tmp=$(mktemp --suffix=.txt) && printf '%s' 'hello' > \"$tmp\" && exec localsend_app \"$tmp\"")
    );
}

#[test]
fn share_mode_preserves_image_rows_shape() {
    let entry = Entry { captured_at: Some(local_ms(9, 5)), ..Entry::image("b", "/img/one.png") };
    assert_eq!(entry.kind, EntryKind::Image);
    let nodes = clipboard_provider(&[entry], ClipMode::Share, true);
    assert_eq!(nodes[0].id, "share.history.b");
    assert_eq!(nodes[0].label, "Image");
    assert_eq!(nodes[0].desc.as_deref(), Some("09:05"));
    assert_eq!(nodes[0].thumb_source, "/img/one.png");
    assert_eq!(nodes[0].action.as_deref(), Some("localsend_app '/img/one.png'"));
}

#[test]
fn default_copy_mode_is_unaffected_by_the_new_parameter() {
    let nodes = clipboard_provider(&[Entry::text("a", "hello")], ClipMode::Copy, true);
    assert_eq!(nodes[0].id, "clipboard.a");
    assert_eq!(nodes[0].action.as_deref(), Some("@ipc:clipboard.copy:a"));
}

// The presence-gate itself (default-menu.jsonc's "share" node): the same `when`
// mechanism the self-pruning cascade test already proves, exercised here with
// the localsend command string. visible_children
// only ever sees a resolved cond_results entry (the launcher's own condition
// runner fills that in from a real `sh -c` exit code), so this proves the tree
// wiring honors both outcomes, not the `command -v` shell behavior itself.
#[test]
fn share_node_when_gate_hides_and_shows_the_whole_subtree() {
    let tree = tree_of(json!({
        "share": {
            "label": "Share",
            "when": "command -v localsend-cli >/dev/null 2>&1 || command -v localsend_app >/dev/null 2>&1"
        },
        // kind: "note" matches share_peer_entries' real output: a leaf with no
        // action/target/provider still needs an explicit kind or build_tree
        // infers "submenu", which visible_children only counts as visible when
        // it has visible children of its own.
        "share.receive": { "label": "Receive", "kind": "note" }
    }));
    let hidden = HashMap::from([("share".to_string(), false)]);
    assert_eq!(visible_children(&tree.nodes, None, &hidden).len(), 0);
    let shown = HashMap::from([("share".to_string(), true)]);
    let visible = visible_children(&tree.nodes, None, &shown);
    assert_eq!(ids(&visible), ["share"]);
}
