use chrono::{Local, TimeZone, Timelike};
use fs_info::reminders::*;
use serde_json::json;

fn entry(id: &str, due_at: f64) -> Entry {
    Entry { id: id.into(), message: format!("m-{id}"), set_at: 0.0, due_at }
}

fn entry_msg(id: &str, due_at: f64, message: &str) -> Entry {
    Entry { message: message.into(), ..entry(id, due_at) }
}

fn ids(list: &[Entry]) -> String {
    list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>().join(",")
}

#[test]
fn parse_duration_units() {
    assert_eq!(parse_duration("25m"), Some(1500));
    assert_eq!(parse_duration("90s"), Some(90));
    assert_eq!(parse_duration("1h"), Some(3600));
    assert_eq!(parse_duration("2h5m30s"), Some(7530));
}

#[test]
fn parse_duration_bare_only_token_is_minutes() {
    assert_eq!(parse_duration("10"), Some(600));
}

#[test]
fn parse_duration_bare_after_unit_takes_next_smaller_unit() {
    assert_eq!(parse_duration("1h30"), Some(5400));
    assert_eq!(parse_duration("5m30"), Some(330));
    // Bare after `m` is seconds, so this is 1h + 30m + 20s.
    assert_eq!(parse_duration("1h30m20"), Some(5420));
}

#[test]
fn parse_duration_bare_after_seconds_is_invalid() {
    assert_eq!(parse_duration("30s10"), None);
}

#[test]
fn parse_duration_rejects_non_durations() {
    assert_eq!(parse_duration(""), None);
    assert_eq!(parse_duration("abc"), None);
    assert_eq!(parse_duration("10x"), None);
    assert_eq!(parse_duration("-5"), None);
}

#[test]
fn parse_duration_rejects_zero() {
    assert_eq!(parse_duration("0"), None);
    assert_eq!(parse_duration("0m"), None);
}

#[test]
fn parse_duration_rejects_internal_whitespace() {
    // parse_spec has already split the message off, so a space inside the
    // duration token is a typo rather than a separator.
    assert_eq!(parse_duration("5 m"), None);
}

#[test]
fn parse_duration_max_boundary() {
    assert_eq!(parse_duration("720h"), Some(MAX_DURATION_SECONDS));
    assert_eq!(parse_duration("721h"), None);
}

#[test]
fn parse_duration_is_case_insensitive() {
    assert_eq!(parse_duration("25M"), Some(1500));
}

#[test]
fn parse_spec_splits_duration_from_message() {
    let s = parse_spec("25m coffee break").unwrap();
    assert_eq!(s.seconds, 1500);
    assert_eq!(s.message, "coffee break");
}

#[test]
fn parse_spec_without_message() {
    let s = parse_spec("10").unwrap();
    assert_eq!(s.seconds, 600);
    assert_eq!(s.message, "");
}

#[test]
fn parse_spec_trims_surrounding_whitespace() {
    let s = parse_spec("  25m   coffee  ").unwrap();
    assert_eq!(s.seconds, 1500);
    assert_eq!(s.message, "coffee");
}

#[test]
fn parse_spec_rejects_message_first() {
    assert_eq!(parse_spec("coffee 25m"), None);
    assert_eq!(parse_spec(""), None);
}

#[test]
fn make_entry_shape() {
    let e = make_entry(1500, "coffee", 1000.0, 3);
    assert_eq!(e.id, "rem-1000-3");
    assert_eq!(e.message, "coffee");
    assert_eq!(e.set_at, 1000.0);
    assert_eq!(e.due_at, 1000.0 + 1500.0 * 1000.0);
}

#[test]
fn add_keeps_list_sorted_by_due_at() {
    let mut list = Vec::new();
    list = add(&list, entry("b", 3000.0));
    list = add(&list, entry("a", 1000.0));
    list = add(&list, entry("c", 2000.0));
    assert_eq!(ids(&list), "a,c,b");
}

#[test]
fn normalize_drops_malformed_entries() {
    let raw = json!([
        { "id": "a", "message": "m-a", "setAt": 0, "dueAt": 2000 },
        { "message": "no id", "dueAt": 1000 },
        { "id": "no-due" },
        { "id": "bad-due", "dueAt": "soon" },
        null,
        { "id": "b", "message": "m-b", "setAt": 0, "dueAt": 1000 },
    ]);
    assert_eq!(ids(&normalize(&raw)), "b,a");
}

#[test]
fn normalize_of_non_array_is_empty() {
    assert_eq!(normalize(&json!(null)).len(), 0);
    assert_eq!(normalize(&json!({ "id": "a", "dueAt": 1 })).len(), 0);
}

#[test]
fn normalize_fills_missing_set_at_from_due_at() {
    let list = normalize(&json!([{ "id": "a", "message": "x", "dueAt": 5000 }]));
    assert_eq!(list[0].set_at, 5000.0);
}

#[test]
fn due_returns_same_remaining_when_nothing_fired() {
    // The service's 1s tick skips its state.json write on an empty `fired`.
    let list = vec![entry("a", 5000.0)];
    let split = due(&list, 1000.0);
    assert_eq!(split.fired.len(), 0);
    assert_eq!(split.remaining, list);
}

#[test]
fn due_splits_fired_from_remaining() {
    let list = vec![entry("a", 1000.0), entry("b", 5000.0)];
    let split = due(&list, 2000.0);
    assert_eq!(ids(&split.fired), "a");
    assert_eq!(ids(&split.remaining), "b");
    assert_ne!(split.remaining, list);
}

#[test]
fn due_fires_entry_whose_due_time_passed_while_shell_was_down() {
    let list = vec![entry("stale", 1000.0)];
    let split = due(&list, 1000.0 + 6.0 * 60.0 * 60.0 * 1000.0);
    assert_eq!(ids(&split.fired), "stale");
    assert_eq!(split.remaining.len(), 0);
}

#[test]
fn due_fires_several_at_once_in_due_at_order() {
    let list = vec![entry("a", 1000.0), entry("b", 2000.0), entry("c", 9000.0)];
    let split = due(&list, 5000.0);
    assert_eq!(ids(&split.fired), "a,b");
    assert_eq!(ids(&split.remaining), "c");
}

#[test]
fn remaining_seconds_rounds_up_and_floors_at_zero() {
    assert_eq!(remaining_seconds(&entry("a", 10500.0), 10000.0), 1.0);
    assert_eq!(remaining_seconds(&entry("a", 10000.0), 10000.0), 0.0);
    assert_eq!(remaining_seconds(&entry("a", 1000.0), 99000.0), 0.0);
}

#[test]
fn countdown_label_widths() {
    assert_eq!(countdown_label(0.0), "00:00");
    assert_eq!(countdown_label(65.0), "01:05");
    assert_eq!(countdown_label(3599.0), "59:59");
    assert_eq!(countdown_label(3600.0), "1:00:00");
    assert_eq!(countdown_label(90000.0), "25:00:00");
}

#[test]
fn bar_label_empty_list() {
    assert_eq!(bar_label(&[], 0.0), "");
}

#[test]
fn bar_label_single_entry_is_bare_countdown() {
    assert_eq!(bar_label(&[entry("a", 723000.0)], 0.0), "12:03");
}

#[test]
fn bar_label_fuses_count_when_more_than_one() {
    let list = [entry("a", 723000.0), entry("b", 800000.0), entry("c", 900000.0)];
    assert_eq!(bar_label(&list, 0.0), "12:03 / 3");
}

#[test]
fn summary_lines_render_message_and_countdown() {
    assert_eq!(summary_lines(&[], 0.0).len(), 0);
    assert_eq!(
        summary_lines(&[entry_msg("a", 723000.0, "coffee")], 0.0).join("|"),
        "coffee / 12:03"
    );
}

#[test]
fn due_clock_is_local_wall_clock() {
    // Built from the same instant rather than a literal: this runs under
    // whatever TZ the test host has.
    let when = Local.with_ymd_and_hms(2026, 8, 11, 14, 32, 0).unwrap();
    let expected = format!("{:02}:{:02}", when.hour(), when.minute());
    assert_eq!(due_clock(when.timestamp_millis() as f64), expected);
}
