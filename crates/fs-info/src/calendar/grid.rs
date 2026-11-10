//! The calendar panel's month grid: which 42 days a month shows, which ISO
//! week each row belongs to, and how an index into that flat list relates to
//! a date. Months are 1-based.

use chrono::{Datelike, Days, NaiveDate};


pub const COLUMNS: usize = 7;
pub const ROWS: usize = 6;
pub(crate) const CELLS: usize = COLUMNS * ROWS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub in_month: bool,
}

impl Cell {
    pub fn date(&self) -> NaiveDate {
        NaiveDate::from_ymd_opt(self.year, self.month, self.day).expect("cells hold real dates")
    }
}

/// Monday-first, always 42 cells regardless of month length, so stepping
/// months never resizes the card: a 28-day February and a 31-day month both
/// pad out with days from the adjacent months. Each cell carries its fully
/// resolved year/month, so selecting a padding day names a real adjacent-month
/// date rather than a day number with no month behind it.
pub fn month_cells(year: i32, month: u32) -> Vec<Cell> {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("a real month");
    let lead = u64::from(first.weekday().num_days_from_monday());
    let start = first - Days::new(lead);
    (0..CELLS as u64)
        .map(|i| {
            let d = start + Days::new(i);
            Cell {
                year: d.year(),
                month: d.month(),
                day: d.day(),
                in_month: d.month() == month && d.year() == year,
            }
        })
        .collect()
}

pub fn date_at(cells: &[Cell], index: usize) -> Option<NaiveDate> {
    cells.get(index).map(Cell::date)
}

pub fn same_date(a: &impl Datelike, b: &impl Datelike) -> bool {
    a.year() == b.year() && a.month() == b.month() && a.day() == b.day()
}

/// Where the cursor lands when the panel opens or the month steps: the given
/// date's own cell when the grid is showing it, else the first day of the
/// month on screen. Never out of range, so the reveal-only first keypress
/// always has a real position to show.
pub fn index_of_date(cells: &[Cell], date: NaiveDate) -> usize {
    cells
        .iter()
        .position(|c| same_date(&c.date(), &date))
        .or_else(|| cells.iter().position(|c| c.in_month))
        .unwrap_or(0)
}

/// The events dot under a day number. Only in-month days carry one: the
/// padding days either side belong to the adjacent months and are dimmed
/// rather than dotted.
pub fn shows_event_dot(cell: Option<&Cell>, event_count: usize) -> bool {
    cell.is_some_and(|c| c.in_month && event_count > 0)
}

/// Month stepping, kept here so wrapping December into January carries the
/// year with it in one tested place.
pub fn step_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m = month as i32 - 1 + delta;
    (year + m.div_euclid(12), m.rem_euclid(12) as u32 + 1)
}

pub fn iso_date(d: &impl Datelike) -> String {
    format!("{}-{:02}-{:02}", d.year(), d.month(), d.day())
}

/// The IPC `select` verb's parse: strict YYYY-MM-DD naming a real calendar
/// day, so 2026-02-31 is rejected rather than silently becoming March 3rd.
pub fn parse_iso_date(iso: &str) -> Option<NaiveDate> {
    let b = iso.as_bytes();
    let shape = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shape {
        return None;
    }
    let year: i32 = iso[0..4].parse().ok()?;
    // Years 0 to 99 never parse: they fail the round-trip check.
    if year < 100 {
        return None;
    }
    NaiveDate::from_ymd_opt(year, iso[5..7].parse().ok()?, iso[8..10].parse().ok()?)
}
