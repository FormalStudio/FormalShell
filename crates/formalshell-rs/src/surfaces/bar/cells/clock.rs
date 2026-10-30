//! The time as one mono label in the format state.json keeps
//! (right click walks the ring), stacked one field per line on a vertical
//! bar, opening the calendar.

use fs_info::clock::{CLOCK_FORMATS, format_qt, next_format, stacked_lines, substitute_iso_week};

use crate::services::state::{self, Field};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, View};

#[derive(Default)]
pub struct Clock {
    text: String,
    format: String,
    vertical: bool,
}

/// The clock format every clock reads: the one state.json saved, else the
/// locale's own 12 or 24 hour preset.
pub fn clock_format(store: &crate::store::Store) -> String {
    let saved = &store.state.data.clock_format;
    match saved.is_empty() {
        false => saved.clone(),
        true if twelve_hour() => CLOCK_FORMATS[1].to_owned(),
        true => CLOCK_FORMATS[0].to_owned(),
    }
}

/// Whether the locale's LC_TIME writes a 12 hour clock: its own time
/// format (`nl_langinfo(T_FMT)`) carrying an AM/PM or a 12 hour field.
fn twelve_hour() -> bool {
    static TWELVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TWELVE.get_or_init(|| {
        // A locale object of its own: setlocale would change the process's.
        let loc = unsafe { libc::newlocale(libc::LC_TIME_MASK, c"".as_ptr(), std::ptr::null_mut()) };
        if loc.is_null() {
            return false;
        }
        let fmt = unsafe { std::ffi::CStr::from_ptr(libc::nl_langinfo_l(libc::T_FMT, loc)) }.to_string_lossy().into_owned();
        unsafe { libc::freelocale(loc) };
        ["%p", "%P", "%r", "%I", "%l"].iter().any(|f| fmt.contains(f))
    })
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
            .panel("calendar")
            .tooltip("RIGHT CYCLE FORMAT / MIDDLE CALENDAR")
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => {
                state::set(vec![Field::ClockFormat(next_format(&self.format).to_owned())]);
                Action::None
            }
            _ => Action::Panel("calendar"),
        }
    }
}
