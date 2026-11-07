//! RFC 5545 VEVENT reader for the calendar panel's events feature. Feeds
//! from both of CalendarEventsService's backends: local .ics files in a
//! khal/vdir-style directory, and the raw ICS the formalshell-eds companion
//! CLI prints from EDS/GOA.
//!
//! Recurring VEVENTs (RRULE) expand into concrete instances within a bounded
//! query window ([`default_window`] is yesterday through 45 days out,
//! matching formalshell-eds's fetch window). Supported subset:
//! FREQ=DAILY/WEEKLY/MONTHLY/YEARLY, INTERVAL, COUNT, UNTIL, BYDAY on weekly
//! rules, and EXDATE as simple local-date matches. Anything else in the rule,
//! BYSETPOS, BYMONTHDAY, ordinal BYDAY (1MO), an unparseable EXDATE, any part
//! not named above, leaves the anchoring VEVENT as a single occurrence at its
//! DTSTART: honest under-expansion, never a guessed instance. Also out of
//! scope: RECURRENCE-ID overrides (the override renders alongside the
//! generated instance), WKST (accepted but ignored; weeks are Monday-based
//! relative to DTSTART's week), and timezone-aware stepping (instances keep
//! DTSTART's local wall-clock time, the single-timezone stance
//! [`parse_date_value`] documents). Instances carry `uid#<localstamp>` uids
//! so [`merge_events`] dedupes per instance and both backends' expansions of
//! the same master collapse together.

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Timelike, Utc};
use regex::Regex;

use super::{local_from_fields, localize, naive_from_fields};
use fs_js as js;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub start: DateTime<Local>,
    pub end: Option<DateTime<Local>>,
    pub all_day: bool,
}

/// RFC 5545 section 3.1: content lines longer than 75 octets are folded onto
/// a continuation line starting with exactly one space or tab. Unfold before
/// any line-oriented parsing.
pub fn unfold(text: &str) -> String {
    static FOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t]").unwrap());
    FOLD.replace_all(&text.replace("\r\n", "\n"), "").into_owned()
}

fn unescape(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// "DTSTART;VALUE=DATE:20260315" (all-day) / "DTSTART:20260315T090000Z"
/// (UTC) / "DTSTART;TZID=Europe/Madrid:20260315T090000" (zoned or floating)
/// to the instant and the all-day flag. TZID offsets are not resolved: a
/// zoned or floating time is read as local wall-clock time, reasonable for a
/// single-timezone khal setup, a documented limitation otherwise.
pub(crate) fn parse_date_value(params: &str, value: &str) -> Option<(DateTime<Local>, bool)> {
    static DATE_PARAM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"VALUE=DATE\b").unwrap());
    static VALUE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^([0-9]{4})([0-9]{2})([0-9]{2})(?:T([0-9]{2})([0-9]{2})([0-9]{2})(Z)?)?$")
            .unwrap()
    });
    let all_day = DATE_PARAM.is_match(params) && !params.contains("VALUE=DATE-TIME");
    let m = VALUE.captures(value)?;
    let field = |i: usize| m[i].parse::<i64>().unwrap_or(0);
    let (y, mo, d) = (field(1), field(2) - 1, field(3));
    if m.get(4).is_none() {
        return Some((local_from_fields(y, mo, d, 0, 0, 0)?, true));
    }
    let (h, mi, s) = (field(4), field(5), field(6));
    let date = if m.get(7).is_some() {
        Utc.from_utc_datetime(&naive_from_fields(y, mo, d, h, mi, s)?)
            .with_timezone(&Local)
    } else {
        local_from_fields(y, mo, d, h, mi, s)?
    };
    Some((date, all_day))
}

struct Parsed {
    uid: String,
    summary: String,
    start: DateTime<Local>,
    end: Option<DateTime<Local>>,
    all_day: bool,
    rrule: String,
    exdates: Vec<DateTime<Local>>,
    exdate_bad: bool,
}

/// One VEVENT block's unfolded body, or `None` when DTSTART is missing or
/// unparseable: an event this reducer can't place on the grid is dropped
/// rather than rendered wrong.
fn parse_event(block: &str) -> Option<Parsed> {
    let mut uid = String::new();
    let mut summary = String::new();
    let mut start = None;
    let mut end = None;
    let mut all_day = false;
    let mut rrule = String::new();
    let mut exdates = Vec::new();
    let mut exdate_bad = false;

    for line in block.split('\n') {
        let Some((left, value)) = line.split_once(':') else {
            continue;
        };
        let (name, params) = left.split_once(';').unwrap_or((left, ""));
        match name.to_uppercase().as_str() {
            "UID" => uid = value.to_owned(),
            "SUMMARY" => summary = unescape(value),
            "DTSTART" => {
                if let Some((date, day)) = parse_date_value(params, value) {
                    start = Some(date);
                    all_day = day;
                }
            }
            "DTEND" => {
                if let Some((date, _)) = parse_date_value(params, value) {
                    end = Some(date);
                }
            }
            "RRULE" => rrule = value.to_owned(),
            "EXDATE" => {
                for v in value.split(',') {
                    match parse_date_value(params, v) {
                        Some((date, _)) => exdates.push(date),
                        None => exdate_bad = true,
                    }
                }
            }
            _ => {}
        }
    }
    Some(Parsed {
        uid,
        summary: if summary.is_empty() { "(untitled)".into() } else { summary },
        start: start?,
        end,
        all_day,
        rrule,
        exdates,
        exdate_bad,
    })
}

#[derive(Clone, Copy, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

struct Rule {
    freq: Freq,
    interval: i64,
    /// 0 means unbounded.
    count: i64,
    until: Option<DateTime<Local>>,
    /// Weekday numbers with Sunday as 0, weekly rules only.
    byday: Option<Vec<u32>>,
}

/// Safety bound on expansion loops, not a tuning knob: candidate stepping is
/// day-granular at worst, so this covers ~270 years of scanning before an
/// event is silently truncated.
const MAX_ITERATIONS: i64 = 100_000;

/// RRULE value to the supported subset, or `None` for anything outside it
/// (the caller then keeps the single anchor). A date-only UNTIL reads as the
/// end of that local day.
fn parse_rrule(value: &str) -> Option<Rule> {
    let mut freq = None;
    let mut interval = 1;
    let mut count = 0;
    let mut until = None;
    let mut byday: Option<Vec<u32>> = None;

    for part in value.split(';') {
        let (key, val) = part.split_once('=')?;
        match key.to_uppercase().as_str() {
            "FREQ" => {
                freq = match val.to_uppercase().as_str() {
                    "DAILY" => Some(Freq::Daily),
                    "WEEKLY" => Some(Freq::Weekly),
                    "MONTHLY" => Some(Freq::Monthly),
                    "YEARLY" => Some(Freq::Yearly),
                    _ => None,
                }
            }
            "INTERVAL" => {
                interval = Some(js::parse_int(val)).filter(|n| *n >= 1.0)? as i64;
            }
            "COUNT" => {
                count = Some(js::parse_int(val)).filter(|n| *n >= 1.0)? as i64;
            }
            "UNTIL" => {
                let (date, all_day) = parse_date_value("", val)?;
                until = Some(if all_day {
                    local_from_fields(
                        i64::from(date.year()),
                        i64::from(date.month()) - 1,
                        i64::from(date.day()),
                        23,
                        59,
                        59,
                    )?
                } else {
                    date
                });
            }
            "BYDAY" => {
                let mut days = Vec::new();
                for code in val.to_uppercase().split(',') {
                    days.push(match code {
                        "SU" => 0,
                        "MO" => 1,
                        "TU" => 2,
                        "WE" => 3,
                        "TH" => 4,
                        "FR" => 5,
                        "SA" => 6,
                        _ => return None,
                    });
                }
                byday = Some(days);
            }
            "WKST" => {}
            _ => return None,
        }
    }
    let freq = freq?;
    if byday.is_some() && freq != Freq::Weekly {
        return None;
    }
    Some(Rule { freq, interval, count, until, byday })
}

/// Calendar arithmetic on local wall-clock fields, so a day step across a DST
/// boundary keeps the event's local time, which instant arithmetic would not.
fn shift_days(base: DateTime<Local>, days: i64) -> Option<DateTime<Local>> {
    let naive = base.naive_local();
    let date = if days >= 0 {
        naive.date().checked_add_days(Days::new(days as u64))?
    } else {
        naive.date().checked_sub_days(Days::new(days.unsigned_abs()))?
    };
    localize(date.and_time(naive.time()))
}

/// The k-th candidate occurrence for the non-BYDAY frequencies, or `None`
/// when the target month/year has no such calendar date (Jan 31 monthly in
/// February, Feb 29 yearly off-leap): RFC 5545 skips those rather than
/// shifting them, and they don't consume COUNT.
fn nth_occurrence(start: DateTime<Local>, freq: Freq, n: i64) -> Option<DateTime<Local>> {
    match freq {
        Freq::Daily => shift_days(start, n),
        Freq::Weekly => shift_days(start, 7 * n),
        Freq::Monthly | Freq::Yearly => {
            let naive = start.naive_local();
            let step = if freq == Freq::Monthly { n } else { n * 12 };
            let months = i64::from(naive.year()) * 12 + i64::from(naive.month0()) + step;
            let year = i32::try_from(months.div_euclid(12)).ok()?;
            let month = u32::try_from(months.rem_euclid(12)).ok()? + 1;
            let date = NaiveDate::from_ymd_opt(year, month, naive.day())?;
            localize(date.and_time(naive.time()))
        }
    }
}

fn monday_of(d: DateTime<Local>) -> NaiveDate {
    let date = d.date_naive();
    date - Days::new(u64::from(date.weekday().num_days_from_monday()))
}

fn same_date(a: DateTime<Local>, b: DateTime<Local>) -> bool {
    a.date_naive() == b.date_naive()
}

enum Verdict {
    Stop,
    Skip,
    Take,
}

/// One occurrence through the bounds, in RFC evaluation order: UNTIL and
/// COUNT define the recurrence set (so pre-window and EXDATE-removed members
/// still consume COUNT), EXDATE and the window then filter what renders.
/// `Stop` ends the scan (candidates are monotonic), `Skip` drops just this
/// one, `Take` emits it.
fn admit(
    s: DateTime<Local>,
    rule: &Rule,
    exdates: &[DateTime<Local>],
    window: (DateTime<Local>, DateTime<Local>),
    made: &mut i64,
) -> Verdict {
    if rule.until.is_some_and(|until| s > until) {
        return Verdict::Stop;
    }
    *made += 1;
    if rule.count > 0 && *made > rule.count {
        return Verdict::Stop;
    }
    if s > window.1 {
        return Verdict::Stop;
    }
    if s < window.0 || exdates.iter().any(|ex| same_date(s, *ex)) {
        return Verdict::Skip;
    }
    Verdict::Take
}

fn stamp(d: DateTime<Local>) -> String {
    format!(
        "{}{:02}{:02}T{:02}{:02}{:02}",
        d.year(),
        d.month(),
        d.day(),
        d.hour(),
        d.minute(),
        d.second()
    )
}

fn instance(event: &Parsed, start: DateTime<Local>) -> Event {
    let end = event.end.map(|end| start + (end - event.start));
    let uid = if event.uid.is_empty() {
        String::new()
    } else {
        format!("{}#{}", event.uid, stamp(start))
    };
    Event { uid, summary: event.summary.clone(), start, end, all_day: event.all_day }
}

/// Recurring event plus supported rule to its in-window instances. DTSTART is
/// always the recurrence set's first member (RFC 5545 leaves an out-of-sync
/// DTSTART undefined; counting it is the common reading). For weekly BYDAY
/// the scan is day-granular with week parity taken against DTSTART's
/// Monday-based week.
fn expand_recurring(
    event: &Parsed,
    rule: &Rule,
    window: (DateTime<Local>, DateTime<Local>),
) -> Vec<Event> {
    let mut instances = Vec::new();
    let mut made = 0;
    let start_week = monday_of(event.start);
    for i in 0..MAX_ITERATIONS {
        let s = if let Some(byday) = &rule.byday {
            let Some(s) = shift_days(event.start, i) else { break };
            let weeks = (monday_of(s) - start_week).num_days() / 7;
            let member = i == 0
                || (byday.contains(&s.weekday().num_days_from_sunday())
                    && weeks % rule.interval == 0);
            if !member {
                if s > window.1 {
                    break;
                }
                continue;
            }
            s
        } else {
            match nth_occurrence(event.start, rule.freq, i * rule.interval) {
                Some(s) => s,
                None => continue,
            }
        };
        match admit(s, rule, &event.exdates, window, &mut made) {
            Verdict::Stop => break,
            Verdict::Take => instances.push(instance(event, s)),
            Verdict::Skip => {}
        }
    }
    instances
}

/// Yesterday through 45 days out, the window the no-window call shape uses.
pub fn default_window(now: DateTime<Local>) -> (DateTime<Local>, DateTime<Local>) {
    let (y, m, d) = (i64::from(now.year()), i64::from(now.month()) - 1, i64::from(now.day()));
    (
        local_from_fields(y, m, d - 1, 0, 0, 0).unwrap_or(now),
        local_from_fields(y, m, d + 45, 23, 59, 59).unwrap_or(now),
    )
}

/// Concatenated .ics text (any number of VCALENDARs, each with any number of
/// VEVENTs) to a flat, unsorted list of events, with recurring events
/// expanded into instances inside `window` (both ends inclusive).
/// Non-recurring events pass through regardless of the window. Garbage input
/// (no VEVENT blocks at all) yields an empty list.
pub fn parse_events(text: &str, window: (DateTime<Local>, DateTime<Local>)) -> Vec<Event> {
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)BEGIN:VEVENT\n(.*?)END:VEVENT").unwrap());
    let unfolded = unfold(text);
    let mut events = Vec::new();
    for m in BLOCK.captures_iter(&unfolded) {
        let Some(event) = parse_event(&m[1]) else { continue };
        let rule = if event.rrule.is_empty() || event.exdate_bad {
            None
        } else {
            parse_rrule(&event.rrule)
        };
        match rule {
            None => events.push(Event {
                uid: event.uid,
                summary: event.summary,
                start: event.start,
                end: event.end,
                all_day: event.all_day,
            }),
            Some(rule) => events.extend(expand_recurring(&event, &rule, window)),
        }
    }
    events
}

/// Two event lists to one, deduped by UID with `primary` winning a collision
/// (the merge CalendarEventsService runs over its ics and EDS backends). An
/// event with an empty UID identifies nothing, so those are always kept
/// rather than collapsed onto each other.
pub fn merge_events(primary: &[Event], secondary: &[Event]) -> Vec<Event> {
    let mut seen = HashSet::new();
    primary
        .iter()
        .chain(secondary)
        .filter(|e| e.uid.is_empty() || seen.insert(e.uid.as_str()))
        .cloned()
        .collect()
}

/// Events whose local start date falls on `date`, the day-cell dot/list
/// query the panel makes once per visible day.
pub fn events_on_date(events: &[Event], date: NaiveDate) -> Vec<Event> {
    events
        .iter()
        .filter(|e| e.start.date_naive() == date)
        .cloned()
        .collect()
}
