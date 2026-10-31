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
use crate::ui::{El, Event, Type, w};

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
            parts.push(w::panel_empty(s, "No location"));
        } else if let Some(e) = state.error {
            parts.push(w::panel_empty(s, "Unavailable"));
            parts.push(w::panel_text(s, w::caption(e.as_str().to_owned()).elide()));
        } else if state.weather.is_none() {
            parts.push(w::panel_empty(s, "Loading"));
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
                        w::panel_row(s, glyph, day_name, None, vec![w::panel_value(temps)]).stop(format!("day:{i}"))
                    })
                    .collect();
                parts.push(w::panel_section_n(s, "Forecast", reading.forecast.len(), rows));
            }
        }
        w::column(0.0, parts)
    }

    fn event(&mut self, ev: &Event, _: &mut Effect) {
        if ev.on == "refresh" {
            kick(Source::Weather);
        }
    }
}
