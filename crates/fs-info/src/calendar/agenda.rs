//! Agenda shaping for the calendar panel's day ledger: what order the
//! selected day's rows appear in, what time each one prints in its left
//! column, and where the current moment sits among them.

use std::cmp::Ordering;

use chrono::{DateTime, Local, Timelike};

use super::ics::Event;

/// All-day events lead, they have no clock time to sort against and cover
/// the whole day the ledger is listing, then the timed ones in
/// chronological order. Summary breaks a same-minute tie so two events keep a
/// stable order across re-renders instead of following whatever order
/// `merge_events` happened to concatenate its two backends in.
pub fn sort_for_day(events: &[Event]) -> Vec<Event> {
    let mut sorted = events.to_vec();
    sorted.sort_by(|a, b| {
        if a.all_day != b.all_day {
            return if a.all_day { Ordering::Less } else { Ordering::Greater };
        }
        a.start.cmp(&b.start).then_with(|| a.summary.cmp(&b.summary))
    });
    sorted
}

/// Whole local days between two instants, so a DST-shifted day still counts
/// as one.
fn day_delta(from: DateTime<Local>, to: DateTime<Local>) -> i64 {
    (to.date_naive() - from.date_naive()).num_days()
}

/// 24-hour by default, matching the shell's own `hh:mm` convention;
/// `twelve_hour` writes the AM/PM notation the bar clock's twin presets use
/// (`clock::uses_meridiem` resolves which is live).
pub fn clock_time(date: DateTime<Local>, twelve_hour: bool) -> String {
    let h = date.hour();
    let m = date.minute();
    if !twelve_hour {
        return format!("{h:02}:{m:02}");
    }
    let shown = if h.is_multiple_of(12) { 12 } else { h % 12 };
    format!("{shown}:{m:02}{}", if h < 12 { " AM" } else { " PM" })
}

/// A row's left column: "ALL DAY", a bare "09:00" for an event with no
/// DTEND, or the "09:00-10:30" range. An end landing on a later local day
/// takes a "+2D" tail rather than printing a clock time that reads as this
/// day's. RFC 5545 makes DTEND exclusive for a VALUE=DATE event, so an
/// all-day span covering n days ends n midnights out and its tail counts n-1.
pub fn time_label(event: &Event, twelve_hour: bool) -> String {
    if event.all_day {
        let days = event.end.map_or(1, |end| day_delta(event.start, end));
        return if days > 1 {
            format!("ALL DAY +{}D", days - 1)
        } else {
            "ALL DAY".into()
        };
    }
    let mut start = clock_time(event.start, twelve_hour);
    let Some(end) = event.end.filter(|end| *end > event.start) else {
        return start;
    };
    let end_text = clock_time(end, twelve_hour);
    // A 12-hour range with both ends in the same half of the day prints one
    // meridiem, on the right, where it covers both: "9:00-10:30 AM".
    if twelve_hour && (event.start.hour() < 12) == (end.hour() < 12) {
        start.truncate(start.len() - 3);
    }
    let over = day_delta(event.start, end);
    let tail = if over > 0 { format!(" +{over}D") } else { String::new() };
    format!("{start}-{end_text}{tail}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    AllDay,
    Upcoming,
    Now,
    Past,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::AllDay => "allday",
            Status::Upcoming => "upcoming",
            Status::Now => "now",
            Status::Past => "past",
        }
    }
}

/// An all-day event is never past, now or upcoming: it runs for the entire
/// day it is listed under, so the running-row treatment would fill every
/// all-day row and say nothing.
pub fn status(event: &Event, now: DateTime<Local>) -> Status {
    if event.all_day {
        return Status::AllDay;
    }
    if now < event.start {
        return Status::Upcoming;
    }
    // A zero-length event (no DTEND, or one equal to DTSTART) is past the
    // moment it starts rather than running forever.
    let end = event.end.unwrap_or(event.start);
    if now < end { Status::Now } else { Status::Past }
}

pub fn has_running(events: &[Event], now: DateTime<Local>) -> bool {
    events.iter().any(|e| status(e, now) == Status::Now)
}

/// The earliest event of the day still to start, or `None` once they all have.
pub fn next_up(events: &[Event], now: DateTime<Local>) -> Option<&Event> {
    events
        .iter()
        .filter(|e| status(e, now) == Status::Upcoming)
        .fold(None, |best: Option<&Event>, e| match best {
            Some(b) if b.start <= e.start => Some(b),
            _ => Some(e),
        })
}
