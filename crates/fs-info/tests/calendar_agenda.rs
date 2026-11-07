use chrono::{DateTime, Local, TimeZone};
use fs_info::calendar::agenda::*;
use fs_info::calendar::ics::Event;

// Local-time construction throughout: agenda.rs reads local wall-clock
// fields, the same single-timezone stance ics.rs's own parser takes.
fn at(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
    Local.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
}

fn event(summary: &str, start: DateTime<Local>, end: Option<DateTime<Local>>, all_day: bool) -> Event {
    Event { uid: summary.into(), summary: summary.into(), start, end, all_day }
}

fn timed(summary: &str, start: DateTime<Local>) -> Event {
    event(summary, start, None, false)
}

fn range(summary: &str, start: DateTime<Local>, end: DateTime<Local>) -> Event {
    event(summary, start, Some(end), false)
}

fn summaries(events: &[Event]) -> String {
    events.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>().join(",")
}

#[test]
fn sort_puts_all_day_events_ahead_of_timed_ones() {
    let t = timed("TIMED", at(2026, 8, 20, 9, 0));
    let a = event("ALLDAY", at(2026, 8, 20, 0, 0), None, true);
    assert_eq!(summaries(&sort_for_day(&[t, a])), "ALLDAY,TIMED");
}

#[test]
fn sort_orders_timed_events_chronologically() {
    let late = timed("LATE", at(2026, 8, 20, 17, 30));
    let early = timed("EARLY", at(2026, 8, 20, 8, 15));
    let noon = timed("NOON", at(2026, 8, 20, 12, 0));
    assert_eq!(summaries(&sort_for_day(&[late, early, noon])), "EARLY,NOON,LATE");
}

#[test]
fn sort_breaks_a_same_minute_tie_on_summary() {
    let b = timed("B", at(2026, 8, 20, 9, 0));
    let a = timed("A", at(2026, 8, 20, 9, 0));
    assert_eq!(summaries(&sort_for_day(&[b, a])), "A,B");
}

#[test]
fn sort_leaves_the_caller_array_untouched() {
    let input = [timed("LATE", at(2026, 8, 20, 17, 0)), timed("EARLY", at(2026, 8, 20, 8, 0))];
    sort_for_day(&input);
    assert_eq!(summaries(&input), "LATE,EARLY");
}

#[test]
fn clock_time_writes_24_hour_by_default() {
    assert_eq!(clock_time(at(2026, 8, 20, 9, 5), false), "09:05");
    assert_eq!(clock_time(at(2026, 8, 20, 21, 30), false), "21:30");
}

#[test]
fn clock_time_writes_meridiem_when_asked() {
    assert_eq!(clock_time(at(2026, 8, 20, 9, 5), true), "9:05 AM");
    assert_eq!(clock_time(at(2026, 8, 20, 21, 30), true), "9:30 PM");
}

#[test]
fn clock_time_writes_midnight_and_noon_as_12() {
    assert_eq!(clock_time(at(2026, 8, 20, 0, 0), true), "12:00 AM");
    assert_eq!(clock_time(at(2026, 8, 20, 12, 0), true), "12:00 PM");
}

#[test]
fn time_label_of_an_all_day_event() {
    let e = event("X", at(2026, 8, 20, 0, 0), None, true);
    assert_eq!(time_label(&e, false), "ALL DAY");
}

#[test]
fn time_label_counts_a_multi_day_all_day_span_from_an_exclusive_end() {
    // DTEND is non-inclusive for VALUE=DATE: Aug 20 to Aug 23 covers three
    // days, so the tail reads +2D.
    let e = event("X", at(2026, 8, 20, 0, 0), Some(at(2026, 8, 23, 0, 0)), true);
    assert_eq!(time_label(&e, false), "ALL DAY +2D");
}

#[test]
fn time_label_of_an_event_with_no_end_is_the_start_alone() {
    assert_eq!(time_label(&timed("X", at(2026, 8, 20, 9, 0)), false), "09:00");
}

#[test]
fn time_label_of_a_zero_length_event_is_the_start_alone() {
    let e = range("X", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 9, 0));
    assert_eq!(time_label(&e, false), "09:00");
}

#[test]
fn time_label_of_a_timed_range() {
    let e = range("X", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 10, 30));
    assert_eq!(time_label(&e, false), "09:00-10:30");
}

#[test]
fn time_label_tails_a_range_that_ends_on_a_later_day() {
    let e = range("X", at(2026, 8, 20, 22, 0), at(2026, 8, 21, 1, 30));
    assert_eq!(time_label(&e, false), "22:00-01:30 +1D");
}

#[test]
fn time_label_prints_one_meridiem_for_a_range_inside_one_half_day() {
    let e = range("X", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 10, 30));
    assert_eq!(time_label(&e, true), "9:00-10:30 AM");
}

#[test]
fn time_label_prints_both_meridiems_across_noon() {
    let e = range("X", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 13, 30));
    assert_eq!(time_label(&e, true), "9:00 AM-1:30 PM");
}

#[test]
fn status_of_an_all_day_event_is_never_running() {
    let e = event("X", at(2026, 8, 20, 0, 0), Some(at(2026, 8, 21, 0, 0)), true);
    assert_eq!(status(&e, at(2026, 8, 20, 12, 0)).as_str(), "allday");
}

#[test]
fn status_before_the_start_is_upcoming() {
    let e = range("X", at(2026, 8, 20, 14, 0), at(2026, 8, 20, 15, 0));
    assert_eq!(status(&e, at(2026, 8, 20, 13, 59)).as_str(), "upcoming");
}

#[test]
fn status_inside_the_range_is_now() {
    let e = range("X", at(2026, 8, 20, 14, 0), at(2026, 8, 20, 15, 0));
    assert_eq!(status(&e, at(2026, 8, 20, 14, 0)).as_str(), "now");
    assert_eq!(status(&e, at(2026, 8, 20, 14, 59)).as_str(), "now");
}

#[test]
fn status_at_the_end_instant_is_already_past() {
    let e = range("X", at(2026, 8, 20, 14, 0), at(2026, 8, 20, 15, 0));
    assert_eq!(status(&e, at(2026, 8, 20, 15, 0)).as_str(), "past");
}

#[test]
fn status_of_an_endless_event_is_past_once_it_starts() {
    let e = timed("X", at(2026, 8, 20, 14, 0));
    assert_eq!(status(&e, at(2026, 8, 20, 14, 1)).as_str(), "past");
}

#[test]
fn has_running_finds_the_event_in_progress() {
    let events = [
        range("DONE", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 10, 0)),
        range("LIVE", at(2026, 8, 20, 14, 0), at(2026, 8, 20, 15, 0)),
    ];
    assert!(has_running(&events, at(2026, 8, 20, 14, 30)));
    assert!(!has_running(&events, at(2026, 8, 20, 16, 0)));
}

#[test]
fn next_up_is_the_earliest_event_still_to_start() {
    let events = [
        timed("EVENING", at(2026, 8, 20, 19, 0)),
        range("DONE", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 10, 0)),
        timed("AFTERNOON", at(2026, 8, 20, 15, 0)),
    ];
    assert_eq!(next_up(&events, at(2026, 8, 20, 12, 0)).unwrap().summary, "AFTERNOON");
}

#[test]
fn next_up_is_null_once_the_day_is_spent() {
    let events = [range("DONE", at(2026, 8, 20, 9, 0), at(2026, 8, 20, 10, 0))];
    assert!(next_up(&events, at(2026, 8, 20, 23, 0)).is_none());
}

#[test]
fn next_up_ignores_all_day_events() {
    let events = [event("ALLDAY", at(2026, 8, 20, 0, 0), None, true)];
    assert!(next_up(&events, at(2026, 8, 20, 8, 0)).is_none());
}
