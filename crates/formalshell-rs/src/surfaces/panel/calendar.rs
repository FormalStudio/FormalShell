//! A hero naming today, a month grid of day cells under
//! a weekday row with the ISO week numbers in a gutter, the selected day's
//! events as rows, and the year and life progress tracks. Wide, for the
//! seven day columns.
//!
//! The cursor addresses the 42 grid cells as a grid (`columns`), Enter
//! selects the day under it, `[` and `]` step the month as the two header
//! chevrons do, and selecting a padding day realigns the view to that
//! day's own month. Events come from the calendar service (local `.ics`
//! files and EDS).
//!
//! The life-progress prompt needs the launcher's input mode, which lands
//! with the launcher: until then a double click on the year track only
//! hides and shows a life row whose two values settings.json or state.json
//! already hold.

use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate};
use fs_info::calendar::{agenda, grid, ics, progress};
use fs_info::clock;
use serde_json::json;

use super::{Effect, Panel, View};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, Size, Type, w};

const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub struct Calendar {
    _want: Option<Want>,
    open: bool,
    year: i32,
    month: u32,
    selected: NaiveDate,
    /// The user's own toggle of the life row; unset it follows whether the
    /// two values resolve.
    show_life: Option<bool>,
    last_year_click: Option<Instant>,
    cursor: Option<String>,
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn key(index: usize) -> String {
    format!("day:{index}")
}

impl Calendar {
    pub fn new(open: bool) -> Self {
        let t = today();
        Self { _want: open.then(|| Want::new(Source::Calendar)), open, year: t.year(), month: t.month(), selected: t, show_life: None, last_year_click: None, cursor: None }
    }

    fn cells(&self) -> Vec<grid::Cell> {
        grid::month_cells(self.year, self.month)
    }

    fn select(&mut self, d: NaiveDate) {
        self.selected = d;
        self.year = d.year();
        self.month = d.month();
        self.cursor = Some(key(grid::index_of_date(&self.cells(), d)));
    }

    fn step_month(&mut self, delta: i32) {
        (self.year, self.month) = grid::step_month(self.year, self.month, delta);
        self.selected = today();
        self.cursor = Some(key(grid::index_of_date(&self.cells(), self.selected)));
    }

    fn status(&self) -> String {
        let view = NaiveDate::from_ymd_opt(self.year, self.month, 1).map(|d| d.format("%B %Y").to_string()).unwrap_or_default();
        json!({ "open": self.open, "selected": grid::iso_date(&self.selected), "today": grid::iso_date(&today()), "view": view }).to_string()
    }

    /// settings.json over state.json, then the fraction both give.
    fn life(store: &Store, now: chrono::DateTime<Local>) -> Option<f64> {
        let birth = progress::resolve_override(store.config.f64("calendar.birthYear"), Some(store.state.data.calendar_birth_year as f64));
        let life = progress::resolve_override(store.config.f64("calendar.lifeExpectancy"), Some(store.state.data.calendar_life_expectancy as f64));
        progress::life_fraction(&now, birth, life)
    }
}

impl Panel for Calendar {
    fn id(&self) -> &'static str {
        "calendar"
    }

    fn title(&self, _: &View) -> String {
        "Calendar".into()
    }

    fn icon(&self, _: &View) -> String {
        "calendar".into()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Clock, Topic::State, Topic::Config]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.popup_width_wide
    }

    fn columns(&self) -> usize {
        grid::COLUMNS
    }

    fn start(&mut self, _: &mut Effect) {
        kick(Source::Calendar);
    }

    fn cursor_start(&self) -> Option<String> {
        Some(key(grid::index_of_date(&self.cells(), today())))
    }

    fn take_cursor(&mut self) -> Option<String> {
        self.cursor.take()
    }

    fn actions(&self, _: &View) -> Vec<El> {
        vec![w::icon_button("chevron-left").on("prev").key("prev"), w::icon_button("chevron-right").on("next").key("next")]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let c = &v.theme.colors;
        let now = Local::now();
        let t = now.date_naive();
        let cells = self.cells();
        let weeks = grid::week_numbers(&cells);
        let events = &v.store.info.calendar.events;
        let twelve = clock::uses_meridiem(&crate::surfaces::bar::cells::clock::clock_format(v.store));

        let hero = w::hero(
            s,
            w::Hero {
                glyph: String::new(),
                title: t.format("%A").to_string(),
                meta: format!("Week {}", clock::pad2(clock::iso_week(t).into())),
                readout: t.day().to_string(),
                trailing: None,
                rail: None,
                rail_on: None,
            },
        );

        let month_name = NaiveDate::from_ymd_opt(self.year, self.month, 1).map(|d| d.format("%B %Y").to_string()).unwrap_or_default();
        let mut head: Vec<El> = vec![w::space(s.md)];
        head.extend(WEEKDAYS.iter().map(|d| w::section_label(s, d, None, false).mid().fill()));
        let mut lines = vec![w::row(s.row_gap, head).fill()];
        for (r, line) in cells.chunks(grid::COLUMNS).enumerate() {
            let week = w::value(clock::pad2(weeks[r].into())).size(Type::Caption).ink(Ink::Muted).width(Size::Px(s.md)).mid();
            let mut row = Vec::new();
            for (k, cell) in line.iter().enumerate() {
                let i = r * grid::COLUMNS + k;
                let date = cell.date();
                let is_today = cell.in_month && grid::same_date(&date, &t);
                let is_selected = cell.in_month && grid::same_date(&date, &self.selected);
                let count = if cell.in_month { ics::events_on_date(events, date).len() } else { 0 };
                let lit = is_today && !is_selected || is_selected;
                let dot_color = if is_selected {
                    c.get("accentForeground")
                } else if is_today {
                    c.get("primaryForeground")
                } else {
                    c.get("primary")
                };
                let dot = if grid::shows_event_dot(Some(cell), count) { dot_color } else { fs_theme::color::Rgba::TRANSPARENT };
                let _ = lit;
                let day = w::value(cell.day.to_string()).ink(if cell.in_month { Ink::Fg } else { Ink::Muted }).centred();
                row.push(
                    w::cell(w::column(s.xxs, vec![day, w::dot(dot, s.xs).centred()]))
                        .cell_state(|st| st.small = true)
                        .active(is_today && !is_selected)
                        .selected(is_selected)
                        .interactive()
                        .stop(key(i))
                        .on(key(i)),
                );
            }
            let days = w::row(s.row_gap, row).fill().swap(i64::from(self.year) * 12 + i64::from(self.month));
            lines.push(w::row(s.row_gap, vec![week, days]).fill());
        }
        let month = w::column(s.row_gap, vec![w::heading(s, &month_name, None, true), w::column(s.row_gap, lines)]);

        let selected_is_today = grid::same_date(&self.selected, &t);
        let day_events = agenda::sort_for_day(&ics::events_on_date(events, self.selected));
        let running = selected_is_today && agenda::has_running(&day_events, now);
        let next = if selected_is_today { agenda::next_up(&day_events, now) } else { None };
        let heading = if selected_is_today { "Today".to_owned() } else { format!("{} {}", MONTHS[self.selected.month0() as usize], self.selected.day()) };
        let mut label_row = vec![w::heading(s, &heading, Some(day_events.len()), true).fill()];
        let side = if running {
            Some("Now".to_owned())
        } else {
            next.map(|e| format!("Next {}", agenda::clock_time(e.start, twelve)))
        };
        if let Some(text) = side {
            label_row.push(w::section_label(s, &text, None, false).pad(0.0, 0.0, s.control_padding_x, 0.0));
        }
        let mut agenda_col = vec![w::row(s.icon_gap, label_row).fill()];
        if day_events.is_empty() {
            agenda_col.push(w::section_label(s, "No events", None, true));
        }
        let gauge = agenda::widest_label(&day_events, twelve);
        let rows: Vec<El> = day_events
            .iter()
            .map(|e| {
                let status = agenda::status(e, now);
                let time_ink = if status == agenda::Status::Past { Ink::Muted } else { Ink::Fg };
                let mut parts = vec![w::value(agenda::time_label(e, twelve)).ink(time_ink).gauge(gauge.clone()), w::label(&e.summary).elide()];
                if status == agenda::Status::Now {
                    parts.push(w::section_label(s, "Now", None, false).ink(Ink::Primary));
                }
                w::cell(w::row(s.icon_gap, parts).fill()).ghost()
            })
            .collect();
        agenda_col.push(w::column(0.0, rows));

        let year = progress::year_fraction(&now);
        let life = Self::life(v.store, now);
        let show_life = self.show_life.unwrap_or(life.is_some()) && life.is_some();
        let bar = |name: &str, fraction: f64| {
            w::column(
                s.xxs,
                vec![
                    w::row(s.icon_gap, vec![w::section_label(s, name, None, false).fill(), w::value(progress::format_percent(fraction)).ink(Ink::Fg)]).fill(),
                    w::track(fraction),
                ],
            )
        };
        let mut progress_col = vec![w::heading(s, "Progress", None, true), w::cell(bar("Year", year)).ghost().on("year")];
        if show_life {
            progress_col.push(w::cell(bar("Life", life.unwrap_or(0.0))).ghost());
        }

        w::column(
            s.section_gap,
            vec![hero, month, w::column(s.row_gap, agenda_col), w::column(s.row_gap, progress_col)],
        )
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        match ev.on.as_str() {
            "prev" => self.step_month(-1),
            "next" => self.step_month(1),
            "year" => {
                let now = Instant::now();
                if self.last_year_click.take().is_some_and(|t| now.duration_since(t) <= DOUBLE_CLICK) {
                    let life = Self::life(fx.store, Local::now());
                    if life.is_some() {
                        self.show_life = Some(!self.show_life.unwrap_or(true));
                    }
                } else {
                    self.last_year_click = Some(now);
                }
            }
            other => self.activate(other, fx),
        }
    }

    fn activate(&mut self, stop: &str, _: &mut Effect) {
        if let Some(i) = stop.strip_prefix("day:").and_then(|i| i.parse::<usize>().ok())
            && let Some(d) = grid::date_at(&self.cells(), i)
        {
            self.select(d);
        }
    }

    fn key(&mut self, _: Option<&str>, text: &str, _: &mut Effect) {
        match text {
            "[" => self.step_month(-1),
            "]" => self.step_month(1),
            _ => {}
        }
    }

    fn call(&mut self, verb: &str, arg: &str) -> Option<String> {
        match verb {
            "select" => Some(match grid::parse_iso_date(arg) {
                Some(d) => {
                    self.select(d);
                    "ok".into()
                }
                None => format!("error: not a valid YYYY-MM-DD date: {arg}"),
            }),
            "status" => Some(self.status()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_aligns_the_view_and_moves_the_cursor() {
        let mut c = Calendar::new(false);
        assert_eq!(c.call("select", "2026-02-31").as_deref(), Some("error: not a valid YYYY-MM-DD date: 2026-02-31"));
        assert_eq!(c.call("select", "2026-10-06").as_deref(), Some("ok"));
        let status: serde_json::Value = serde_json::from_str(&c.call("status", "").unwrap()).unwrap();
        assert_eq!(status["selected"], "2026-10-06");
        assert_eq!(status["view"], "October 2026");
        assert_eq!(status["open"], false);
        // October 2026 starts on a Thursday, so the 6th is the ninth cell.
        assert_eq!(c.take_cursor().as_deref(), Some("day:8"));
        assert_eq!(c.take_cursor(), None);
    }

    #[test]
    fn stepping_a_month_resets_the_selection_to_today() {
        let mut c = Calendar::new(false);
        c.select(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap());
        c.step_month(1);
        assert_eq!((c.year, c.month), (2026, 4));
        assert_eq!(c.selected, today());
    }
}
