//! The condition icon (a dim thermometer before the first
//! reading), the rounded temperature beside it when `showLabel` is on, and
//! the whole reading in the tooltip.

use chrono::{Local, Timelike};
use fs_info::weather as openmeteo;

use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, View};

pub struct Weather {
    want: Want,
    reading: Option<openmeteo::Weather>,
    day: bool,
    label: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Self { want: Want::new(Source::Weather), reading: None, day: true, label: false }
    }
}

/// `Math.round`: halves go up, and a zero is never signed.
fn round(n: f64) -> f64 {
    let r = (n + 0.5).floor();
    if r == 0.0 { 0.0 } else { r }
}

impl Cell for Weather {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info, Topic::Clock]
    }

    fn read(&mut self, env: &Env) -> bool {
        let reading = env.store.info.weather.weather.clone();
        let hour = Local::now().hour();
        let day = (6..20).contains(&hour);
        let label = env.store.config.bool("bar.widgets.weather.showLabel").unwrap_or(false);
        let changed = (&reading, day, label) != (&self.reading, self.day, self.label);
        (self.reading, self.day, self.label) = (reading, day, label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let code = self.reading.as_ref().map_or(-1.0, |w| w.current.code);
        let mut parts = vec![Part::Icon { name: openmeteo::icon_for_code(code, Some(self.day)).into(), dim: self.reading.is_none(), dot: false }];
        let head = match &self.reading {
            Some(w) => {
                let temp = round(w.current.temperature);
                if self.label {
                    parts.push(Part::Label { text: format!("{temp}\u{b0}"), weight: None });
                }
                format!("WEATHER / {} / {temp}\u{b0}", openmeteo::condition_label(w.current.code))
            }
            None => "WEATHER / UNAVAILABLE".to_owned(),
        };
        View::new(parts, look.xs).panel("weather").tooltip(format!("{head} / RIGHT REFRESH"))
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => {
                kick(Source::Weather);
                Action::None
            }
            _ => Action::Panel("weather"),
        }
    }
}
