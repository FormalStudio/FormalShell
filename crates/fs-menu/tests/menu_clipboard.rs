// clipboard_provider's node shape: text rows get a truncated preview label;
// image rows get a fixed Image label, a capture-time desc, and a thumb_source.
// Activation and debounce wiring is out of scope: this is the pure half.

use chrono::{Local, TimeZone};
use fs_menu::clipboard::entry::{Entry, EntryKind};
use fs_menu::node::Kind;
use fs_menu::providers::{
    ClipMode, clipboard_empty_row, clipboard_no_match_row, clipboard_provider, clipboard_search, paste_argv,
};

fn local_ms(h: u32, m: u32) -> i64 {
    Local.with_ymd_and_hms(2026, 1, 1, h, m, 0).unwrap().timestamp_millis()
}

fn text(id: &str, t: &str, captured_at: i64) -> Entry {
    Entry { captured_at: Some(captured_at), ..Entry::text(id, t) }
}

fn image(id: &str, path: &str, captured_at: i64) -> Entry {
    Entry { captured_at: Some(captured_at), ..Entry::image(id, path) }
}

fn argv(chord: &str) -> Option<Vec<String>> {
    paste_argv(chord)
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn text_entry_maps_to_preview_label_action_node() {
    let nodes = clipboard_provider(&[text("a", "hello world", local_ms(14, 2))], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, "clipboard.a");
    assert_eq!(nodes[0].label, "hello world");
    assert_eq!(nodes[0].kind, Kind::Action);
    assert_eq!(nodes[0].desc.as_deref(), Some(""));
    assert_eq!(nodes[0].thumb_source, "");
    assert_eq!(nodes[0].full_text, "hello world");
    assert_eq!(nodes[0].time, "14:02");
    // In-process, not a spawned IPC command: nothing puts that binary on a
    // session PATH, so the spawned form was a silent exit 127 everywhere but
    // the smoke VM.
    assert_eq!(nodes[0].action.as_ref().map(ToString::to_string).as_deref(), Some("@ipc:clipboard.copy:a"));
    assert!(nodes[0].paste_after);
    assert_eq!(nodes[0].verb.as_deref(), Some("Paste"));
}

#[test]
fn emoji_only_rows_are_flagged_and_keep_their_run() {
    let nodes = clipboard_provider(
        &[text("a", "\u{1F602}\n", 0), text("b", "\u{2764}\u{FE0F}\n\u{1F339}", 0), text("c", "rose \u{1F339}", 0), image("d", "/tmp/x.png", 0)],
        ClipMode::Copy,
        true,
        chrono::Local::now(),
    );
    assert!(nodes[0].emoji_only);
    assert_eq!(nodes[0].label, "\u{1F602}");
    assert!(nodes[1].emoji_only);
    assert_eq!(nodes[1].label, "\u{2764}\u{FE0F} \u{1F339}");
    assert!(!nodes[2].emoji_only);
    assert_eq!(nodes[2].label, "rose \u{1F339}");
    assert!(!nodes[3].emoji_only);
}

// Shift+Enter's target, on image rows in copy mode alone: a text row has no
// file to send, and a share row's Shift+Enter has nothing of its own to do.
#[test]
fn clipssh_path_rides_copy_mode_image_rows_only() {
    let entries = [image("img", "/state/clipboard-images/abc.png", 0), text("txt", "hello", 0)];
    let copy = clipboard_provider(&entries, ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(copy[0].clipssh_path, "/state/clipboard-images/abc.png");
    assert_eq!(copy[1].clipssh_path, "");
    let share = clipboard_provider(&entries, ClipMode::Share, true, chrono::Local::now());
    assert_eq!(share[0].clipssh_path, "");
    assert_eq!(share[1].clipssh_path, "");
}

// Entries persisted before the history learned `kind` have none: the provider
// must not mistake that for an image row.
#[test]
fn legacy_text_entry_without_kind_still_maps_as_text() {
    let legacy: Entry = serde_json::from_value(serde_json::json!({ "id": "a", "text": "legacy" })).unwrap();
    assert_eq!(legacy.kind, EntryKind::Text);
    let nodes = clipboard_provider(&[legacy], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(nodes[0].label, "legacy");
    assert_eq!(nodes[0].thumb_source, "");
}

#[test]
fn image_entry_maps_to_image_label_desc_and_thumb() {
    let entry = Entry {
        mime: Some("image/png".into()),
        ..image("b", "/state/clipboard-images/abc.png", local_ms(9, 5))
    };
    let nodes = clipboard_provider(&[entry], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].id, "clipboard.b");
    assert_eq!(nodes[0].label, "Image");
    assert_eq!(nodes[0].desc.as_deref(), Some("09:05"));
    assert_eq!(nodes[0].thumb_source, "/state/clipboard-images/abc.png");
    assert_eq!(nodes[0].full_text, "");
    assert_eq!(nodes[0].time, "09:05");
    assert_eq!(nodes[0].action.as_ref().map(ToString::to_string).as_deref(), Some("@ipc:clipboard.copy:b"));
    assert!(nodes[0].paste_after);
}

#[test]
fn mixed_entries_preserve_order() {
    let nodes = clipboard_provider(&[image("b", "/img/one.png", 1000), text("a", "one", 999)], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].label, "Image");
    assert_eq!(nodes[1].label, "one");
}

#[test]
fn empty_items_maps_to_empty_list() {
    assert!(clipboard_provider(&[], ClipMode::Copy, true, chrono::Local::now()).is_empty());
}

// clipboard.paste off: the row still copies, it just stops touching the
// focused window, and says Copy rather than promising a paste.
#[test]
fn paste_disabled_drops_paste_marker_and_changes_verb() {
    let nodes = clipboard_provider(&[Entry::text("a", "hi")], ClipMode::Copy, false, chrono::Local::now());
    assert_eq!(nodes[0].action.as_ref().map(ToString::to_string).as_deref(), Some("@ipc:clipboard.copy:a"));
    assert!(!nodes[0].paste_after);
    assert_eq!(nodes[0].verb.as_deref(), Some("Copy"));
}

// Share rows hand the entry to LocalSend and never synthesize input, whatever
// clipboard.paste says.
#[test]
fn share_rows_never_paste() {
    let nodes = clipboard_provider(&[Entry::text("a", "hi")], ClipMode::Share, true, chrono::Local::now());
    assert_eq!(nodes[0].id, "share.history.a");
    assert!(!nodes[0].paste_after);
    assert_eq!(nodes[0].verb.as_deref(), Some("Share"));
    assert!(!nodes[0].action.as_ref().map(ToString::to_string).as_deref().unwrap().contains("@ipc:"));
}

// paste_argv: wtype's press/tap/release argv for a chord, releases in reverse
// order so the modifiers unwind the way they were pressed.
#[test]
fn paste_argv_default_chord() {
    assert_eq!(argv("ctrl+v"), Some(strs(&["-M", "ctrl", "-k", "v", "-m", "ctrl"])));
}

#[test]
fn paste_argv_multiple_modifiers_release_in_reverse() {
    assert_eq!(
        argv("ctrl+shift+v"),
        Some(strs(&["-M", "ctrl", "-M", "shift", "-k", "v", "-m", "shift", "-m", "ctrl"]))
    );
}

#[test]
fn paste_argv_is_case_and_space_insensitive() {
    assert_eq!(argv(" Ctrl + V "), Some(strs(&["-M", "ctrl", "-k", "v", "-m", "ctrl"])));
}

#[test]
fn paste_argv_bare_key_needs_no_modifier() {
    assert_eq!(argv("insert"), Some(strs(&["-k", "insert"])));
}

// A typo pastes nothing rather than some other keystroke. "super" is in here
// deliberately: it reads like a modifier wtype would take and is not one
// (probed against the binary: the windows key is "logo").
#[test]
fn paste_argv_rejects_unknown_modifier() {
    assert_eq!(argv("cmd+v"), None);
    assert_eq!(argv("super+v"), None);
    assert_eq!(argv("meta+v"), None);
}

#[test]
fn paste_argv_accepts_the_windows_key_as_logo() {
    assert_eq!(argv("logo+v"), Some(strs(&["-M", "logo", "-k", "v", "-m", "logo"])));
}

#[test]
fn paste_argv_rejects_a_chord_with_no_key() {
    assert_eq!(argv("ctrl+shift"), None);
    assert_eq!(argv(""), None);
}

// clipboard_search: the route-local filter. Pure over already-built rows, so
// these tests build rows through the real provider rather than hand-rolling
// node shapes.
#[test]
fn search_empty_query_returns_rows_unchanged() {
    let rows = clipboard_provider(&[text("a", "hello", 1000)], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(clipboard_search(&rows, "").len(), rows.len());
    assert_eq!(clipboard_search(&rows, "   ").len(), rows.len());
}

#[test]
fn search_matches_beyond_the_truncated_label() {
    let long_text = format!("{}findme", "x".repeat(80));
    let rows = clipboard_provider(&[text("a", &long_text, 1000)], ClipMode::Copy, true, chrono::Local::now());
    // The label truncates at 60 chars; the match text does not.
    assert!(!rows[0].label.contains("findme"));
    assert_eq!(clipboard_search(&rows, "findme").len(), 1);
}

#[test]
fn search_is_case_insensitive() {
    let rows = clipboard_provider(&[text("a", "Hello World", 1000)], ClipMode::Copy, true, chrono::Local::now());
    assert_eq!(clipboard_search(&rows, "WORLD").len(), 1);
    assert_eq!(clipboard_search(&rows, "world").len(), 1);
}

#[test]
fn search_finds_image_rows_by_their_label() {
    let rows = clipboard_provider(
        &[image("a", "/img/one.png", 1000), text("b", "grocery list", 999)],
        ClipMode::Copy,
        true,
        chrono::Local::now(),
    );
    let hits = clipboard_search(&rows, "image");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "clipboard.a");
}

#[test]
fn search_excludes_non_matching_rows() {
    let rows = clipboard_provider(&[text("a", "apple", 1000), text("b", "banana", 999)], ClipMode::Copy, true, chrono::Local::now());
    let hits = clipboard_search(&rows, "apple");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "clipboard.a");
}

// Honest empty-list notes: dim, non-activatable, no colon.
#[test]
fn empty_row_shape() {
    let row = clipboard_empty_row();
    assert_eq!(row.label, "Clipboard history is empty");
    assert_eq!(row.kind, Kind::Note);
    assert_eq!(row.dim, Some(true));
}

#[test]
fn no_match_row_shape() {
    let row = clipboard_no_match_row();
    assert_eq!(row.label, "No matches");
    assert_eq!(row.kind, Kind::Note);
    assert_eq!(row.dim, Some(true));
}
