//! The default capture source, opt-in through bar.layout.
//! One icon, live or muted; with no capture device one dim NO MIC label,
//! staying on the strip because the user asked for it. Left click mutes,
//! middle opens the audio panel; no wheel, a mic reads as on or off.

use fs_system::audio::{SourceState, source_state};

use crate::services::devices::Op;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, View};
use crate::paint::Look;

pub struct Mic {
    state: SourceState,
}

impl Default for Mic {
    fn default() -> Self {
        Self { state: SourceState::Unavailable }
    }
}

impl Cell for Mic {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices]
    }

    fn read(&mut self, env: &Env) -> bool {
        let a = &env.store.devices.audio;
        let state = source_state(a.source, a.source_muted);
        let changed = state != self.state;
        self.state = state;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let (part, head) = match self.state {
            SourceState::Unavailable => (Part::Meta { text: "NO MIC".into() }, "NO INPUT DEVICE"),
            SourceState::Muted => (Part::icon("mic-off"), "MIC MUTED"),
            SourceState::Live => (Part::icon("mic"), "MIC LIVE"),
        };
        View::new(vec![part], look.xxs).tooltip(format!("{head} / MIDDLE AUDIO PANEL"))
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Middle => Action::Panel("audio"),
            Button::Left => Action::Device(Op::ToggleSourceMute),
            Button::Right => Action::None,
        }
    }
}
