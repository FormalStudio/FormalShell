//! A gauge icon and the worst tracked window's percent,
//! `destructive` at or past 90%. Hidden until an enabled provider has
//! answered; the honest states (NO AUTH, STALE, NO CODEX) are words. A
//! click on a stale cell asks for the token refresh before the panel opens.

use crate::services::info::usage::{Claude, State};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Tone, View};

pub struct Usage {
    want: Want,
    state: State,
    label: bool,
}

impl Default for Usage {
    fn default() -> Self {
        Self { want: Want::new(Source::Usage), state: State::default(), label: true }
    }
}

impl Cell for Usage {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.info.usage.clone();
        let label = env.store.config.bool("bar.widgets.usage.showLabel").unwrap_or(true);
        let changed = state != self.state || label != self.label;
        (self.state, self.label) = (state, label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        if !self.state.settled() {
            return View::hidden();
        }
        let worst = self.state.worst();
        let status = self.state.status_label();
        let mut parts = vec![Part::Icon { name: "gauge".into(), dim: worst < 0.0, dot: false }];
        if self.label {
            if worst >= 0.0 {
                parts.push(Part::Label { text: format!("{}%", (worst * 100.0 + 0.5).floor()), weight: None });
            } else if !status.is_empty() {
                parts.push(Part::Meta { text: status.to_uppercase() });
            }
        }
        let reading = if worst >= 0.0 {
            format!("{}%", (worst * 100.0 + 0.5).floor())
        } else if !status.is_empty() {
            status.to_uppercase()
        } else {
            "UNAVAILABLE".to_owned()
        };
        View::new(parts, look.xs)
            .tone(if worst >= 0.9 { Tone::Destructive } else { Tone::Rest })
            .panel("usage")
            .tooltip(format!("USAGE / {reading}"))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        if self.state.claude == Claude::Stale {
            kick(Source::Usage);
        }
        Action::Panel("usage")
    }
}
