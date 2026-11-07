use chrono::NaiveDate;
use fs_info::calendar::grid::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

// Months are 1-based throughout.

#[test]
fn a_month_is_always_six_weeks_of_seven_days() {
    assert_eq!(COLUMNS, 7);
    assert_eq!(month_cells(2026, 8).len(), 42);
    // February in a non-leap year pads out to the same 42 cells, so stepping
    // months never resizes the card.
    assert_eq!(month_cells(2026, 2).len(), 42);
}

#[test]
fn the_grid_starts_on_the_monday_of_the_first_week() {
    // 2026-08-01 is a Saturday, so the grid opens on Monday 2026-07-27.
    let cells = month_cells(2026, 8);
    assert_eq!(cells[0].year, 2026);
    assert_eq!(cells[0].month, 7);
    assert_eq!(cells[0].day, 27);
    assert!(!cells[0].in_month);
}

#[test]
fn a_month_starting_on_monday_needs_no_leading_padding() {
    // 2026-06-01 is a Monday.
    let cells = month_cells(2026, 6);
    assert_eq!(cells[0].day, 1);
    assert_eq!(cells[0].month, 6);
    assert!(cells[0].in_month);
}

#[test]
fn in_month_marks_exactly_the_month_asked_for() {
    let cells = month_cells(2026, 8);
    let in_month: Vec<_> = cells.iter().filter(|c| c.in_month).collect();
    assert_eq!(in_month.len(), 31);
    assert_eq!(in_month[0].day, 1);
    assert_eq!(in_month[30].day, 31);
}

#[test]
fn index_of_date_finds_the_cell_showing_that_day() {
    let cells = month_cells(2026, 8);
    let index = index_of_date(&cells, d(2026, 8, 1));
    assert_eq!(index, 5);
    assert_eq!(cells[index].day, 1);
}

#[test]
fn index_of_date_falls_back_to_the_first_day_of_the_month() {
    let cells = month_cells(2026, 8);
    // A date the grid isn't showing at all still yields a real position, so
    // the reveal-only first keypress has somewhere to appear.
    let index = index_of_date(&cells, d(2027, 3, 14));
    assert_eq!(index, 5);
    assert!(cells[index].in_month);
}

#[test]
fn date_at_reads_the_cell_back_as_a_date() {
    let cells = month_cells(2026, 8);
    assert_eq!(date_at(&cells, 5), Some(d(2026, 8, 1)));
    assert_eq!(date_at(&cells, 42), None);
}

#[test]
fn same_date_ignores_the_time_of_day() {
    let morning = d(2026, 8, 25).and_hms_opt(9, 15, 0).unwrap();
    let evening = d(2026, 8, 25).and_hms_opt(23, 45, 0).unwrap();
    assert!(same_date(&morning, &evening));
    assert!(!same_date(&morning, &d(2026, 8, 26).and_hms_opt(0, 0, 0).unwrap()));
}

#[test]
fn the_event_dot_is_in_month_days_with_events_only() {
    let cells = month_cells(2026, 8);
    assert!(shows_event_dot(Some(&cells[5]), 1));
    assert!(!shows_event_dot(Some(&cells[5]), 0));
    // A padding day belongs to the adjacent month and is dimmed rather than
    // dotted, however many events it has.
    assert!(!shows_event_dot(Some(&cells[0]), 3));
    assert!(!shows_event_dot(None, 3));
}

#[test]
fn step_month_carries_the_year_across_january_and_december() {
    assert_eq!(step_month(2026, 12, 1), (2027, 1));
    assert_eq!(step_month(2026, 1, -1), (2025, 12));
}

#[test]
fn iso_date_pads_both_components() {
    assert_eq!(iso_date(&d(2026, 1, 5)), "2026-01-05");
    assert_eq!(iso_date(&d(2026, 12, 31)), "2026-12-31");
}

#[test]
fn parse_iso_date_accepts_only_a_real_calendar_day() {
    assert_eq!(parse_iso_date("2026-08-25"), Some(d(2026, 8, 25)));
    // A day that rolls over is rejected rather than silently becoming the
    // next month.
    assert_eq!(parse_iso_date("2026-02-31"), None);
    assert_eq!(parse_iso_date("25/08/2026"), None);
    assert_eq!(parse_iso_date(""), None);
}
