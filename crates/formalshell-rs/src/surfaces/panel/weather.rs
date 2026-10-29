//! A hero carrying the current condition, today's high
//! and low and the temperature, then one row per forecast day. The header
//! icon tracks the live condition. Opening asks the poll to go again, and
//! the panel holds the weather source for as long as it is up, so a bar
//! without the weather cell still gets a forecast.

use chrono::{Local, Timelike};
use fs_info::weather as openmeteo;

use super::{Effect, Panel, View};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::ui::{El, Event, Ink, Type, w};

pub struct Weather {
    _want: Want,
}

impl Default for Weather {
    fn default() -> Self {
        Self { _want: Want::new(Source::Weather) }
    }
}

/// `Math.round`: halves go up, and a zero is never signed.
fn round(n: f64) -> f64 {
    let r = (n + 0.5).floor();
    if r == 0.0 { 0.0 } else { r }
}

fn is_day() -> bool {
    (6..20).contains(&Local::now().hour())
}

impl Panel for Weather {
    fn id(&self) -> &'static str {
        "weather"
    }

    fn title(&self, v: &View) -> String {
        let place = &v.store.info.weather.place;
        if place.is_empty() { "Weather".into() } else { place.clone() }
    }

    fn icon(&self, v: &View) -> String {
        match &v.store.info.weather.weather {
            Some(w) => openmeteo::icon_for_code(w.current.code, Some(is_day())).into(),
            None => "cloud".into(),
        }
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Clock]
    }

    fn opened(&mut self) {
        kick(Source::Weather);
    }

    fn actions(&self, v: &View) -> Vec<El> {
        vec![w::icon_button("refresh-cw").enabled(v.store.info.weather.located).on("refresh").key("refresh")]
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let state = &v.store.info.weather;
        let mut parts = Vec::new();
        if !state.located {
            parts.push(w::section_label(s, "No location", None, true));
        } else if let Some(e) = state.error {
            parts.push(
                w::cell(w::column(s.xxs, vec![
                    w::section_label(s, "Unavailable", None, false),
                    w::value(e.as_str()).ink(Ink::Muted).elide(),
                ]))
                .ghost(),
            );
        } else if state.weather.is_none() {
            parts.push(w::section_label(s, "Loading", None, true));
        }
        if let Some(reading) = &state.weather {
            let day = is_day();
            let range = reading
                .forecast
                .first()
                .map(|d| format!("{}\u{b0} / {}\u{b0}", round(d.high), round(d.low)))
                .unwrap_or_default();
            parts.push(w::hero_with(
                s,
                w::Hero {
                    glyph: openmeteo::icon_for_code(reading.current.code, Some(day)).into(),
                    title: openmeteo::condition_text(reading.current.code),
                    meta: range,
                    readout: format!("{}\u{b0}", round(reading.current.temperature)),
                    trailing: None,
                    rail: None,
                    rail_on: None,
                },
                true,
                Type::Display,
                None,
            ));
            if !reading.forecast.is_empty() {
                let rows = reading
                    .forecast
                    .iter()
                    .enumerate()
                    .map(|(i, d)| {
                        let temps = format!("{}\u{b0} / {}\u{b0}", round(d.high), round(d.low));
                        let day_name = openmeteo::weekday_label(&d.date).unwrap_or("");
                        let glyph = openmeteo::icon_for_code(d.code, Some(true));
                        w::list_row(s, glyph, day_name, vec![w::value(temps)]).stop(format!("day:{i}"))
                    })
                    .collect();
                parts.push(w::section(s, "Forecast", Some(reading.forecast.len()), rows));
            }
        }
        w::column(s.section_gap, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        if ev.on == "refresh" {
            kick(Source::Weather);
        }
    }
}
