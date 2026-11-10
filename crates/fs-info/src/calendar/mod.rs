//! Calendar logic. Instants are `DateTime<Local>`: the panel reads local
//! wall-clock fields throughout, the single-timezone stance ics.rs documents.

use chrono::{DateTime, Days, Duration, Local, LocalResult, NaiveDate, NaiveDateTime, TimeZone};

pub mod agenda;
pub mod grid;
pub mod ics;
pub mod progress;

/// The local instant for a wall-clock time. A time repeated by a DST fall
/// back takes its first occurrence and one skipped by a spring forward moves
/// ahead by the gap.
pub(crate) fn localize(naive: NaiveDateTime) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&naive) {
        LocalResult::Single(t) => Some(t),
        LocalResult::Ambiguous(first, _) => Some(first),
        LocalResult::None => Local
            .from_local_datetime(&(naive + Duration::hours(1)))
            .earliest(),
    }
}

/// Wall-clock fields with overflow carried over (month 13 is January of the
/// next year, hour 24 is midnight of the next day), month 0-based.
pub(crate) fn naive_from_fields(
    year: i64,
    month0: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<NaiveDateTime> {
    let months = year * 12 + month0;
    let y = i32::try_from(months.div_euclid(12)).ok()?;
    let m = u32::try_from(months.rem_euclid(12)).ok()? + 1;
    let first = NaiveDate::from_ymd_opt(y, m, 1)?;
    let date = if day >= 1 {
        first.checked_add_days(Days::new((day - 1).unsigned_abs()))?
    } else {
        first.checked_sub_days(Days::new((1 - day).unsigned_abs()))?
    };
    date.and_hms_opt(0, 0, 0)?
        .checked_add_signed(Duration::seconds(hour * 3600 + minute * 60 + second))
}

pub(crate) fn local_from_fields(
    year: i64,
    month0: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
) -> Option<DateTime<Local>> {
    localize(naive_from_fields(year, month0, day, hour, minute, second)?)
}
