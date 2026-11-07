use fs_menu::appmatch::{decorate_app_rows, match_windows, next_window};
use fs_menu::node::{DesktopEntry, Node};
use fs_types::{Window, WindowId};

fn win(id: &str, app_id: &str) -> Window {
    Window { id: WindowId::new(id), app_id: app_id.into(), ..Window::default() }
}

fn next(matches: &[WindowId], focused: &str) -> Option<String> {
    next_window(matches, &WindowId::new(focused)).map(|id| id.to_string())
}

fn entry(id: &str, startup_class: &str) -> DesktopEntry {
    DesktopEntry { id: id.into(), startup_class: startup_class.into(), name: id.into(), ..DesktopEntry::default() }
}

fn ids(v: &[&str]) -> Vec<WindowId> {
    v.iter().map(|s| WindowId::new(*s)).collect()
}

#[test]
fn startup_class_beats_id_when_both_present() {
    let windows = [win("1", "firefox"), win("2", "org.mozilla.firefox")];
    let matches = match_windows(Some(&entry("org.mozilla.firefox", "firefox")), &windows);
    assert_eq!(matches, ["1"]);
}

#[test]
fn startup_class_matches_case_insensitively() {
    let windows = [win("1", "Alacritty")];
    let matches = match_windows(Some(&entry("alacritty-terminal", "alacritty")), &windows);
    assert_eq!(matches, ["1"]);
}

#[test]
fn exact_startup_class_wins_over_a_case_folded_one() {
    let windows = [win("1", "Alacritty"), win("2", "alacritty")];
    let matches = match_windows(Some(&entry("alacritty", "alacritty")), &windows);
    assert_eq!(matches, ["2"]);
}

#[test]
fn falls_back_to_entry_id_when_startup_class_is_empty() {
    let windows = [win("1", "mpv")];
    assert_eq!(match_windows(Some(&entry("mpv", "")), &windows), ["1"]);
}

#[test]
fn no_match_returns_empty_list_so_the_caller_spawns() {
    let windows = [win("1", "firefox"), win("2", "mpv")];
    assert!(match_windows(Some(&entry("code", "code")), &windows).is_empty());
}

#[test]
fn empty_app_id_on_a_window_never_matches_an_empty_startup_class() {
    let windows = [win("1", "")];
    assert!(match_windows(Some(&entry("", "")), &windows).is_empty());
    assert!(match_windows(Some(&entry("mpv", "")), &windows).is_empty());
}

#[test]
fn every_instance_of_the_same_app_is_matched_in_window_order() {
    let windows = [win("1", "kitty"), win("2", "mpv"), win("3", "kitty")];
    let matches = match_windows(Some(&entry("kitty", "kitty")), &windows);
    assert_eq!(matches, ["1", "3"]);
}

#[test]
fn next_window_cycles_past_the_focused_instance_and_wraps() {
    let matches = ids(&["a", "b", "c"]);
    assert_eq!(next(&matches, "a").as_deref(), Some("b"));
    assert_eq!(next(&matches, "b").as_deref(), Some("c"));
    assert_eq!(next(&matches, "c").as_deref(), Some("a"));
}

#[test]
fn next_window_returns_first_when_focus_is_elsewhere_or_empty() {
    let matches = ids(&["a", "b"]);
    assert_eq!(next(&matches, "").as_deref(), Some("a"));
    assert_eq!(next(&matches, "zzz").as_deref(), Some("a"));
}

#[test]
fn next_window_of_empty_matches_is_none() {
    assert_eq!(next(&[], "a"), None);
}

#[test]
fn window_ids_are_returned_verbatim_never_parsed() {
    let windows = [win("abc123def", "kitty"), win("0x55f1", "kitty")];
    let matches = match_windows(Some(&entry("kitty", "kitty")), &windows);
    assert_eq!(matches, ["abc123def", "0x55f1"]);
    assert_eq!(next(&matches, "abc123def").as_deref(), Some("0x55f1"));
}

fn app_row(id: &str, label: &str, entry_id: &str) -> Node {
    Node { entry: Some(entry(entry_id, entry_id)), ..Node::new(id, label, fs_menu::node::Kind::App) }
}

#[test]
fn decorate_app_rows_marks_only_matched_rows_with_focus() {
    let windows = [win("1", "firefox")];
    let rows = [app_row("apps.firefox", "Firefox", "firefox"), app_row("apps.mpv", "mpv", "mpv")];
    let decorated = decorate_app_rows(&rows, &windows);
    assert_eq!(decorated.len(), 2);
    assert_eq!(decorated[0].desc.as_deref(), Some("FOCUS"));
    assert_eq!(decorated[0].label, "Firefox");
    assert_eq!(decorated[1].desc, None);
}

#[test]
fn decorate_app_rows_does_not_mutate_the_input_rows() {
    let windows = [win("1", "firefox")];
    let rows = [app_row("apps.firefox", "Firefox", "firefox")];
    let before = rows.clone();
    decorate_app_rows(&rows, &windows);
    assert_eq!(rows, before);
}

#[test]
fn decorate_app_rows_with_no_windows_marks_nothing() {
    let rows = [app_row("apps.mpv", "mpv", "mpv")];
    assert_eq!(decorate_app_rows(&rows, &[])[0].desc, None);
}
