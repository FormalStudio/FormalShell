//! Today's date as the header's title with the month and its two
//! chevrons beside the close button, a month grid of round day cells under
//! a weekday row (today a filled `primary` circle, the selected day an
//! `accent` one, a dot under a day with events), the selected day's events
//! as grouped rows with a dot and their time, its ISO week on the agenda's
//! caption line, and the year and life progress tracks. Wide, for the
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

use fs_theme::palette::ColorRole;
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, NaiveDate};
use fs_info::calendar::{agenda, grid, ics, progress};
use fs_info::clock;
use serde_json::json;

use super::{Effect, Nav, Panel, View, Walk};
use crate::services::info::kick;
use crate::services::wants::Source;
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, Size, Type, Weight, w};

const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub struct Calendar {
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
        Self { open, year: t.year(), month: t.month(), selected: t, show_life: None, last_year_click: None, cursor: None }
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

    /// `calendar select`: the day `iso` names (YYYY-MM-DD) selected.
    pub fn select_iso(&mut self, iso: &str) -> Result<(), String> {
        let d = grid::parse_iso_date(iso).ok_or_else(|| format!("not a valid YYYY-MM-DD date: {iso}"))?;
        self.select(d);
        Ok(())
    }

    /// settings.json over state.json, then the fraction both give.
    fn life(store: &Store, now: chrono::DateTime<Local>) -> Option<f64> {
        let (birth, life) = crate::settings::calendar_life(&store.config);
        let birth = progress::resolve_override(birth, Some(store.state.data.calendar_birth_year as f64));
        let life = progress::resolve_override(life, Some(store.state.data.calendar_life_expectancy as f64));
        progress::life_fraction(&now, birth, life)
    }
}

impl Panel for Calendar {
    fn title(&self, _: &View) -> String {
        today().format("%A %-d").to_string()
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Calendar, Topic::Clock, Topic::State, Topic::Config]
    }

    fn wants(&self, _: &Store) -> Vec<Source> {
        vec![Source::Calendar]
    }

    fn nav(&self, _: Option<&str>) -> Nav {
        Nav { walk: Walk::Grid(grid::COLUMNS), steps: false }
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
        let month = NaiveDate::from_ymd_opt(self.year, self.month, 1).map(|d| d.format("%B %Y").to_string()).unwrap_or_default();
        vec![
            w::icon_button("chevron-left").tip("Previous month").on("prev").key("prev"),
            w::label(month).ink(Ink::Muted),
            w::icon_button("chevron-right").tip("Next month").on("next").key("next"),
        ]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let c = &v.theme.colors;
        let now = Local::now();
        let t = now.date_naive();
        let cells = self.cells();
        let events = &v.store.calendar.events;
        let twelve = clock::uses_meridiem(&crate::surfaces::desktop_clock::clock_format(v.store));

        // One square per day, centred in its seventh of the row, so the
        // circles keep the same rhythm across and down.
        let side = s.panel_row - s.xs;
        let slot = |el: El| w::row(0.0, vec![w::spacer(), el, w::spacer()]).fill();
        let head: Vec<El> = WEEKDAYS.iter().map(|d| slot(w::caption(*d).weight(Weight::Semibold).ink(Ink::Muted))).collect();
        let mut lines = vec![w::row(0.0, head).fill().height(s.panel_row_compact)];
        for (r, line) in cells.chunks(grid::COLUMNS).enumerate() {
            let mut row = Vec::new();
            for (k, cell) in line.iter().enumerate() {
                let i = r * grid::COLUMNS + k;
                let date = cell.date();
                let is_today = cell.in_month && grid::same_date(&date, &t);
                let is_selected = cell.in_month && grid::same_date(&date, &self.selected) && !is_today;
                let count = if cell.in_month { ics::events_on_date(events, date).len() } else { 0 };
                let dot_color = if is_today { c.get(ColorRole::PrimaryForeground) } else { c.get(ColorRole::Primary) };
                let dot = if grid::shows_event_dot(Some(cell), count) { dot_color } else { fs_theme::color::Rgba::TRANSPARENT };
                let day = w::value(cell.day.to_string()).size(Type::Body).ink(if cell.in_month { Ink::Fg } else { Ink::Muted }).centred();
                let face = w::column(0.0, vec![w::spacer(), day, w::dot(dot, s.xs).centred(), w::spacer()]).fill();
                let circle = w::cell(face)
                    .ghost()
                    .cell_state(|st| st.round = true)
                    .active(is_today)
                    .selected(is_selected)
                    .interactive()
                    .width(Size::Px(side))
                    .height(side)
                    .stop(key(i))
                    .on(key(i));
                row.push(slot(circle));
            }
            lines.push(w::row(0.0, row).fill().swap(i64::from(self.year) * 12 + i64::from(self.month)));
        }
        let month = w::panel_text(s, w::column(s.xxs, lines).pad_top(s.panel_gap));

        let selected_is_today = grid::same_date(&self.selected, &t);
        let day_events = agenda::sort_for_day(&ics::events_on_date(events, self.selected));
        let running = selected_is_today && agenda::has_running(&day_events, now);
        let next = if selected_is_today { agenda::next_up(&day_events, now) } else { None };
        let heading = if selected_is_today { "Today".to_owned() } else { self.selected.format("%A, %B %-d").to_string() };
        let mut side_text = vec![format!("Week {}", clock::iso_week(self.selected))];
        if running {
            side_text.push("Now".to_owned());
        } else if let Some(e) = next {
            side_text.push(format!("Next {}", agenda::clock_time(e.start, twelve)));
        }
        let agenda_heading = w::panel_heading_with(s, &heading, side_text.join("  \u{b7}  "));
        let rows: Vec<El> = day_events
            .iter()
            .map(|e| {
                let status = agenda::status(e, now);
                let past = status == agenda::Status::Past;
                let dot = w::dot(if past { c.get(ColorRole::MutedForeground) } else { c.get(ColorRole::Primary) }, s.sm);
                let name = w::label(&e.summary).ink(if past { Ink::Muted } else { Ink::Fg });
                let time = w::caption(agenda::time_label(e, twelve)).mono();
                let now = (status == agenda::Status::Now).then(|| w::caption("Now").weight(Weight::Semibold).ink(Ink::Primary));
                w::panel_row_with(s, Some(dot), name, Some(time), now.into_iter().collect())
            })
            .collect();

        let year = progress::year_fraction(&now);
        let life = Self::life(v.store, now);
        let show_life = self.show_life.unwrap_or(life.is_some()) && life.is_some();
        let bar = |name: &str, fraction: f64| w::panel_meter(s, None, name, vec![w::panel_value(progress::format_percent(fraction))], Some(w::track(fraction)));
        let mut progress_rows = vec![bar("Year", year).on("year")];
        if show_life {
            progress_rows.push(bar("Life", life.unwrap_or(0.0)));
        }

        let agenda = w::panel_section_with(s, agenda_heading, rows, "No events");
        w::column(0.0, vec![month, agenda, w::panel_section(s, "Progress", progress_rows, "")])
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

    fn status(&self) -> serde_json::Value {
        let view = NaiveDate::from_ymd_opt(self.year, self.month, 1).map(|d| d.format("%B %Y").to_string()).unwrap_or_default();
        json!({ "open": self.open, "selected": grid::iso_date(&self.selected), "today": grid::iso_date(&today()), "view": view })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ui::{Button, What};

    fn click(c: &mut Calendar, on: &str) {
        let store = Store::default();
        let mut fx = Effect::new(&store, None);
        c.event(&Event { on: on.into(), what: What::Click, button: Button::Left }, &mut fx);
    }

    fn press(c: &mut Calendar, text: &str) {
        let store = Store::default();
        let mut fx = Effect::new(&store, None);
        c.key(None, text, &mut fx);
    }

    #[test]
    fn select_aligns_the_view_and_moves_the_cursor() {
        let mut c = Calendar::new(false);
        assert_eq!(c.select_iso("2026-02-31"), Err("not a valid YYYY-MM-DD date: 2026-02-31".into()));
        assert_eq!(c.select_iso("2026-10-06"), Ok(()));
        let status = c.status();
        assert_eq!(status["selected"], "2026-10-06");
        assert_eq!(status["view"], "October 2026");
        assert_eq!(status["open"], false);
        // October 2026 starts on a Thursday, so the 6th is the ninth cell.
        assert_eq!(c.take_cursor().as_deref(), Some("day:8"));
        assert_eq!(c.take_cursor(), None);

        // The first cell is a September padding day: clicking it realigns the view.
        click(&mut c, "day:0");
        let status = c.status();
        assert_eq!(status["selected"], "2026-09-28");
        assert_eq!(status["view"], "September 2026");
        // September 2026 starts on a Tuesday, so the 28th is the 29th cell.
        assert_eq!(c.take_cursor().as_deref(), Some("day:28"));
    }

    #[test]
    fn stepping_a_month_resets_the_selection_to_today() {
        let mut c = Calendar::new(false);
        assert_eq!(c.select_iso("2026-03-15"), Ok(()));
        press(&mut c, "]");
        let status = c.status();
        assert_eq!(status["view"], "April 2026");
        assert_eq!(status["selected"], status["today"]);
        click(&mut c, "prev");
        assert_eq!(c.status()["view"], "March 2026");
        press(&mut c, "[");
        assert_eq!(c.status()["view"], "February 2026");
    }
}
