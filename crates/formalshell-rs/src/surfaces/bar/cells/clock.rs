//! The time as one mono label in the format state.json keeps
//! (right click walks the ring), stacked one field per line on a vertical
//! bar, opening the calendar.

use fs_chrome::bar::panels::PanelId;
use fs_info::clock::{format_qt, next_format, stacked_lines, substitute_iso_week};

use crate::services::state::{self, Field};
use crate::surfaces::desktop_clock::clock_format;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Clock {
    text: String,
    format: String,
    vertical: bool,
}

fn format_of(env: &Env) -> String {
    clock_format(env.store)
}

impl Cell for Clock {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Clock, Topic::State]
    }

    fn read(&mut self, env: &Env) -> bool {
        let format = format_of(env);
        let now = env.store.clock.now;
        let text = format_qt(now, &substitute_iso_week(&format, now.date()));
        let vertical = env.edge.is_vertical();
        let changed = (text.as_str(), vertical) != (self.text.as_str(), self.vertical);
        (self.text, self.format, self.vertical) = (text, format, vertical);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let parts = if self.vertical {
            stacked_lines(&self.text).into_iter().map(|t| Part::Label { text: t, weight: None }).collect()
        } else {
            vec![Part::Label { text: self.text.clone(), weight: None }]
        };
        View::new(parts, if self.vertical { 0.0 } else { look.xs })
            .panel(PanelId::Calendar)
            .tooltip("RIGHT CYCLE FORMAT / MIDDLE CALENDAR")
    }

    fn summary(&self) -> Option<Summary> {
        Some(Summary::new("clock", "Clock", self.text.clone()))
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => {
                state::set(vec![Field::ClockFormat(next_format(&self.format).to_owned())]);
                Action::None
            }
            _ => Action::Panel(PanelId::Calendar),
        }
    }
}
