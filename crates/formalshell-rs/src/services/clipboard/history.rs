//! Pure clipboard-history reducer: capped, de-duplicated by content, newest
//! first. Every function takes state in and returns state out; there is no
//! clock and no mutation of the input. Image entries dedupe by `path`: the
//! capture side content-addresses the file by its sha256, so path equality
//! already is content equality. The mutating reducers return the image paths
//! any eviction just orphaned, and the caller deletes the files; nothing here
//! touches the filesystem.

use fs_menu::clipboard::entry::{Entry, EntryKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_ENTRIES: usize = 300;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub items: Vec<Entry>,
}

/// The new state plus the image paths the change orphaned.
#[derive(Clone, Debug, PartialEq)]
pub struct Reduced {
    pub state: State,
    pub removed_paths: Vec<String>,
}

fn no_removal(state: &State) -> Reduced {
    Reduced { state: state.clone(), removed_paths: Vec::new() }
}

/// Drops empty and whitespace-only captures (the shape a clipboard clear
/// event produces) and returns the text unchanged otherwise; real content is
/// never trimmed.
///
/// NUL bytes are stripped before the empty check. The watcher's line splitter
/// skips the byte directly after a delimiter match, so two adjacent NULs in
/// the watcher's stream leak one into the front of the next entry: an empty
/// capture followed by a real one recorded `\0/tmp/shot.png` alongside the
/// clean `/tmp/shot.png`. Stripping rather than rejecting collapses that pair
/// back into one row, and a NUL could never survive wl-copy's argv anyway.
pub fn sanitize(text: &str) -> Option<String> {
    let stripped: String = text.chars().filter(|&c| c != '\0').collect();
    if fs_js::trim(&stripped).is_empty() { None } else { Some(stripped) }
}

/// Reads an entry out of persisted JSON, migrating a legacy one (no `kind`)
/// to text. `None` when the value is not an object.
pub fn normalize_entry(value: &Value) -> Option<Entry> {
    if !value.is_object() {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn image_paths_of(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.kind == EntryKind::Image)
        .map(|e| e.path.clone().unwrap_or_default())
        .collect()
}

fn cap_overflow(mut items: Vec<Entry>) -> (Vec<Entry>, Vec<String>) {
    if items.len() <= MAX_ENTRIES {
        return (items, Vec::new());
    }
    let overflow = items.split_off(MAX_ENTRIES);
    (items, image_paths_of(&overflow))
}

/// A capture to add. Text and image entries share a shape; `kind` picks the
/// reducer, and an entry that is not an image is text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NewEntry {
    pub id: String,
    pub kind: EntryKind,
    pub text: Option<String>,
    pub path: Option<String>,
    pub mime: Option<String>,
}

/// Re-copying content already in history moves the existing entry to the
/// front (refreshing `captured_at`) instead of inserting a duplicate; the new
/// entry keeps the OLD id, so anything holding a reference to it (a pending
/// copy or remove) still resolves.
fn add_text(state: &State, entry: &NewEntry, now: i64) -> Reduced {
    let Some(text) = entry.text.as_deref().and_then(sanitize) else { return no_removal(state) };

    let mut items = state.items.clone();
    let mut id = entry.id.clone();
    if let Some(idx) = items.iter().position(|i| i.kind != EntryKind::Image && i.text.as_deref() == Some(&text)) {
        id = items.remove(idx).id;
    }
    items.insert(
        0,
        Entry { id, kind: EntryKind::Text, text: Some(text), captured_at: Some(now), ..Entry::default() },
    );
    let (items, removed_paths) = cap_overflow(items);
    Reduced { state: State { items }, removed_paths }
}

fn add_image(state: &State, entry: &NewEntry, now: i64) -> Reduced {
    let Some(path) = entry.path.as_deref().filter(|p| !p.is_empty()) else { return no_removal(state) };

    let mut items = state.items.clone();
    let mut id = entry.id.clone();
    if let Some(idx) = items.iter().position(|i| i.kind == EntryKind::Image && i.path.as_deref() == Some(path)) {
        id = items.remove(idx).id;
    }
    let mime = entry.mime.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| "image/png".to_string());
    items.insert(
        0,
        Entry {
            id,
            kind: EntryKind::Image,
            path: Some(path.to_string()),
            mime: Some(mime),
            captured_at: Some(now),
            ..Entry::default()
        },
    );
    let (items, removed_paths) = cap_overflow(items);
    Reduced { state: State { items }, removed_paths }
}

pub fn add(state: &State, entry: &NewEntry, now: i64) -> Reduced {
    if entry.kind == EntryKind::Image { add_image(state, entry, now) } else { add_text(state, entry, now) }
}

pub fn remove(state: &State, id: &str) -> Reduced {
    let Some(idx) = state.items.iter().position(|i| i.id == id) else { return no_removal(state) };
    let mut items = state.items.clone();
    let removed = items.remove(idx);
    let removed_paths = if removed.kind == EntryKind::Image { vec![removed.path.unwrap_or_default()] } else { Vec::new() };
    Reduced { state: State { items }, removed_paths }
}

/// A copied file reaches the text watcher too, as its own path or uri. Once
/// the same copy has landed as an image that row is its echo. Only the newest
/// row, and only inside `window_ms` of `now`: the echo is always the capture
/// that just happened, never a row further down.
pub fn drop_echo(state: &State, texts: &[&str], now: i64, window_ms: i64) -> State {
    let Some(top) = state.items.first() else { return state.clone() };
    if top.kind == EntryKind::Image || top.captured_at.is_some_and(|at| now - at > window_ms) {
        return state.clone();
    }
    let top_text = fs_js::trim(top.text.as_deref().unwrap_or(""));
    if !texts.contains(&top_text) {
        return state.clone();
    }
    State { items: state.items[1..].to_vec() }
}

pub fn clear(state: &State) -> Reduced {
    if state.items.is_empty() {
        return no_removal(state);
    }
    Reduced { state: State::default(), removed_paths: image_paths_of(&state.items) }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entry(id: &str, text: &str) -> NewEntry {
        NewEntry { id: id.into(), text: Some(text.into()), ..NewEntry::default() }
    }

    fn image(id: &str, path: &str) -> NewEntry {
        NewEntry { id: id.into(), kind: EntryKind::Image, path: Some(path.into()), ..NewEntry::default() }
    }

    fn texts(s: &State) -> Vec<&str> {
        s.items.iter().map(|i| i.text.as_deref().unwrap_or("")).collect()
    }

    #[test]
    fn sanitize_drops_empty_string() {
        assert_eq!(sanitize(""), None);
    }

    #[test]
    fn sanitize_drops_whitespace_only() {
        assert_eq!(sanitize("   \n\t  "), None);
    }

    #[test]
    fn sanitize_keeps_real_text() {
        assert_eq!(sanitize("  hello  ").as_deref(), Some("  hello  "));
    }

    #[test]
    fn sanitize_strips_nul_bytes() {
        assert_eq!(sanitize("\u{0}/tmp/shot.png").as_deref(), Some("/tmp/shot.png"));
    }

    #[test]
    fn sanitize_drops_a_nul_only_capture() {
        assert_eq!(sanitize("\u{0}"), None);
    }

    // The SplitParser delimiter the capture side lost (see sanitize's comment):
    // both halves have to land as the one row.
    #[test]
    fn add_nul_prefixed_capture_dedupes_with_the_clean_one() {
        let mut s = State::default();
        s = add(&s, &entry("a", "\u{0}/tmp/shot.png"), 1000).state;
        s = add(&s, &entry("b", "/tmp/shot.png"), 1001).state;
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].id, "a");
        assert_eq!(s.items[0].text.as_deref(), Some("/tmp/shot.png"));
    }

    #[test]
    fn add_appends_new_entry_to_front() {
        let s = add(&State::default(), &entry("a", "hello"), 1000).state;
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].id, "a");
        assert_eq!(s.items[0].kind, EntryKind::Text);
        assert_eq!(s.items[0].text.as_deref(), Some("hello"));
        assert_eq!(s.items[0].captured_at, Some(1000));
    }

    #[test]
    fn add_multiple_entries_newest_first() {
        let mut s = State::default();
        s = add(&s, &entry("a", "one"), 1000).state;
        s = add(&s, &entry("b", "two"), 1001).state;
        s = add(&s, &entry("c", "three"), 1002).state;
        assert_eq!(texts(&s), ["three", "two", "one"]);
    }

    #[test]
    fn add_dedup_moves_existing_to_front_without_duplicate() {
        let mut s = State::default();
        s = add(&s, &entry("a", "one"), 1000).state;
        s = add(&s, &entry("b", "two"), 1001).state;
        s = add(&s, &entry("c", "one"), 1002).state; // re-copy of "one"
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].text.as_deref(), Some("one"));
        assert_eq!(s.items[0].captured_at, Some(1002));
        assert_eq!(s.items[1].text.as_deref(), Some("two"));
    }

    #[test]
    fn add_dedup_preserves_original_id() {
        let mut s = State::default();
        s = add(&s, &entry("a", "one"), 1000).state;
        s = add(&s, &entry("b", "one"), 2000).state; // dedup should keep id "a"
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].id, "a");
    }

    #[test]
    fn add_whitespace_only_is_noop() {
        let s = State::default();
        let result = add(&s, &entry("a", "   "), 1000);
        assert_eq!(result.state, s);
        assert!(result.removed_paths.is_empty());
    }

    #[test]
    fn add_cap_overflow_drops_oldest() {
        let mut s = State::default();
        for i in 0..300 {
            s = add(&s, &entry(&format!("id{i}"), &format!("text{i}")), 1000 + i).state;
        }
        assert_eq!(s.items.len(), 300);
        assert_eq!(s.items[0].text.as_deref(), Some("text299"));

        let result = add(&s, &entry("id300", "text300"), 1300);
        s = result.state;
        assert_eq!(s.items.len(), 300);
        assert_eq!(s.items[0].text.as_deref(), Some("text300"));
        assert!(result.removed_paths.is_empty()); // evicted entry was text, not image
        assert!(!s.items.iter().any(|i| i.id == "id0")); // oldest evicted
    }

    #[test]
    fn add_image_appends_with_kind_and_defaults_mime() {
        let s = add(&State::default(), &image("a", "/state/clipboard-images/abc.png"), 1000).state;
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].kind, EntryKind::Image);
        assert_eq!(s.items[0].path.as_deref(), Some("/state/clipboard-images/abc.png"));
        assert_eq!(s.items[0].mime.as_deref(), Some("image/png"));
        assert_eq!(s.items[0].captured_at, Some(1000));
    }

    #[test]
    fn add_image_without_path_is_noop() {
        let s = State::default();
        assert_eq!(add(&s, &image("a", ""), 1000).state, s);
    }

    #[test]
    fn add_image_dedups_by_path_moves_to_front_keeps_id() {
        let mut s = State::default();
        s = add(&s, &image("a", "/img/one.png"), 1000).state;
        s = add(&s, &entry("b", "text between"), 1001).state;
        s = add(&s, &image("c", "/img/one.png"), 1002).state; // re-copy of same path
        assert_eq!(s.items.len(), 2);
        assert_eq!(s.items[0].kind, EntryKind::Image);
        assert_eq!(s.items[0].id, "a"); // original id kept
        assert_eq!(s.items[0].captured_at, Some(1002));
        assert_eq!(s.items[1].text.as_deref(), Some("text between"));
    }

    // Text dedupes by text, images dedupe by path: a text entry never matches an
    // image entry regardless of shared fields.
    #[test]
    fn add_text_and_image_with_same_id_do_not_collide() {
        let mut s = State::default();
        s = add(&s, &image("a", "/img/one.png"), 1000).state;
        s = add(&s, &entry("b", "/img/one.png"), 1001).state; // same string, different kind
        assert_eq!(s.items.len(), 2);
    }

    #[test]
    fn add_cap_overflow_reports_evicted_image_paths() {
        let mut s = State::default();
        for i in 0..300 {
            s = add(&s, &image(&format!("id{i}"), &format!("/img/{i}.png")), 1000 + i).state;
        }
        let result = add(&s, &entry("text300", "final text"), 1300);
        assert_eq!(result.state.items.len(), 300);
        assert_eq!(result.removed_paths, ["/img/0.png"]); // oldest (image) evicted
    }

    #[test]
    fn remove_deletes_by_id() {
        let mut s = State::default();
        s = add(&s, &entry("a", "one"), 1000).state;
        s = add(&s, &entry("b", "two"), 1001).state;
        let result = remove(&s, "a");
        assert_eq!(result.state.items.len(), 1);
        assert_eq!(result.state.items[0].id, "b");
        assert!(result.removed_paths.is_empty());
    }

    #[test]
    fn remove_image_reports_its_path() {
        let s = add(&State::default(), &image("a", "/img/one.png"), 1000).state;
        let result = remove(&s, "a");
        assert_eq!(result.state.items.len(), 0);
        assert_eq!(result.removed_paths, ["/img/one.png"]);
    }

    #[test]
    fn remove_unknown_id_is_noop() {
        let s = add(&State::default(), &entry("a", "one"), 1000).state;
        let result = remove(&s, "missing");
        assert_eq!(result.state, s);
        assert!(result.removed_paths.is_empty());
    }

    #[test]
    fn clear_empties_items() {
        let mut s = State::default();
        s = add(&s, &entry("a", "one"), 1000).state;
        s = add(&s, &entry("b", "two"), 1001).state;
        assert_eq!(clear(&s).state.items.len(), 0);
    }

    #[test]
    fn clear_reports_every_image_path() {
        let mut s = State::default();
        s = add(&s, &image("a", "/img/one.png"), 1000).state;
        s = add(&s, &entry("b", "text"), 1001).state;
        s = add(&s, &image("c", "/img/two.png"), 1002).state;
        let result = clear(&s);
        assert_eq!(result.removed_paths.len(), 2);
        assert!(result.removed_paths.contains(&"/img/one.png".to_string()));
        assert!(result.removed_paths.contains(&"/img/two.png".to_string()));
    }

    #[test]
    fn clear_on_empty_is_noop() {
        let s = State::default();
        let result = clear(&s);
        assert_eq!(result.state, s);
        assert!(result.removed_paths.is_empty());
    }

    #[test]
    fn normalize_entry_legacy_entry_without_kind_reads_as_text() {
        let normalized = normalize_entry(&json!({ "id": "a", "text": "hello", "capturedAt": 1000 })).unwrap();
        assert_eq!(normalized.kind, EntryKind::Text);
        assert_eq!(normalized.text.as_deref(), Some("hello"));
        assert_eq!(normalized.id, "a");
    }

    #[test]
    fn normalize_entry_is_a_noop_for_already_tagged_entries() {
        let img = json!({ "id": "a", "kind": "image", "path": "/img/one.png", "capturedAt": 1000 });
        let normalized = normalize_entry(&img).unwrap();
        assert_eq!(normalized.kind, EntryKind::Image);
        assert_eq!(normalized, Entry { id: "a".into(), kind: EntryKind::Image, path: Some("/img/one.png".into()), captured_at: Some(1000), ..Entry::default() });
        assert_eq!(serde_json::to_value(&normalized).unwrap(), img);
    }

    // drop_echo, the part of the uri-list echo handling that lives in the history
    // reducer.
    #[test]
    fn drop_echo_takes_the_fresh_path_row() {
        let mut s = add(&State::default(), &entry("a", "older"), 1000).state;
        s = add(&s, &entry("b", "/tmp/a.png\n"), 2000).state;
        s = drop_echo(&s, &["/tmp/a.png", "file:///tmp/a.png"], 2500, 5000);
        assert_eq!(s.items.len(), 1);
        assert_eq!(s.items[0].text.as_deref(), Some("older"));
    }

    #[test]
    fn drop_echo_leaves_other_rows() {
        let mut s = add(&State::default(), &entry("a", "/tmp/a.png"), 1000).state;
        assert_eq!(drop_echo(&s, &["/tmp/a.png"], 9000, 5000), s);
        s = add(&s, &entry("b", "newer"), 2000).state;
        assert_eq!(drop_echo(&s, &["/tmp/a.png"], 2500, 5000), s);
        assert_eq!(drop_echo(&State::default(), &["/tmp/a.png"], 0, 5000).items.len(), 0);
    }

    #[test]
    fn max_entries_is_300() {
        assert_eq!(MAX_ENTRIES, 300);
    }
}
