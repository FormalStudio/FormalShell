//! The default sink's volume, muted or not, with the
//! percentage opt-in (`bar.widgets.audio.showLabel`). Right click mutes,
//! the wheel steps it by 5%.

use fs_audio::Visual;

use crate::services::devices::{self, Op};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Audio {
    state: devices::Audio,
    show_label: bool,
}

/// The icon the strip warms ahead of the first frame.
pub const ICONS: &[&str] = &["volume-2"];

impl Cell for Audio {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices, Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.devices.audio.clone();
        let show_label = env.store.config.bool("bar.widgets.audio.showLabel").unwrap_or(false);
        let changed = state != self.state || show_label != self.show_label;
        (self.state, self.show_label) = (state, show_label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let s = &self.state;
        let percent = (s.volume.map_or(0.0, Visual::as_f64) * 100.0).round();
        let head = if s.muted { "OUTPUT MUTED" } else { "OUTPUT VOLUME" };
        let mut parts = vec![Part::icon(if s.muted { "volume-x" } else { "volume-2" })];
        if self.show_label {
            parts.push(Part::Label { text: format!("{percent}%"), weight: None });
        }
        View::new(parts, look.xs).panel("audio").tooltip(format!("{head} / {percent}% / RIGHT MUTE"))
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => Action::Device(Op::ToggleMute),
            _ => Action::Panel("audio"),
        }
    }

    fn wheel(&mut self, up: bool, _: &Env) -> Action {
        let Some(v) = self.state.volume else { return Action::None };
        Action::Device(Op::Volume(Visual::from_f64(v.as_f64() + if up { 0.05 } else { -0.05 })))
    }
}
