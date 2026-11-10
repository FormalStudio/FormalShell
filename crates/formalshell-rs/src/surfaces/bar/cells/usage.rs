//! A gauge icon and the worst tracked window's percent,
//! `destructive` at or past 90%. Hidden until an enabled provider has
//! answered; the honest states (NO AUTH, STALE, NO CODEX) are words. A
//! click on a stale cell asks for the token refresh before the panel opens.

use fs_chrome::bar::panels::PanelId;

use crate::services::info::usage::{Claude, State};
use crate::services::info::kick;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::{Look, Tone};

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
        &[Topic::Usage]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.usage.clone();
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
        let percent = worst.map(|w| w.to_percent().rounded());
        let tone = if worst.is_some_and(|w| w.as_fraction() >= 0.9) { Tone::Destructive } else { Tone::Rest };
        let status = self.state.status_label();
        let mut parts = vec![Part::icon("gauge").dim(worst.is_none())];
        if self.label {
            if let Some(p) = percent {
                parts.push(Part::Label { text: format!("{p}%"), weight: None });
            } else if !status.is_empty() {
                parts.push(Part::Meta { text: status.to_uppercase() });
            }
        }
        let reading = if let Some(p) = percent {
            format!("{p}%")
        } else if !status.is_empty() {
            status.to_uppercase()
        } else {
            "UNAVAILABLE".to_owned()
        };
        View::new(parts, look.xs)
            .tone(tone)
            .panel(PanelId::Usage)
            .tooltip(format!("USAGE / {reading}"))
    }

    fn summary(&self) -> Option<Summary> {
        if !self.state.settled() {
            return None;
        }
        let worst = self.state.worst();
        let percent = worst.map(|w| w.to_percent().rounded());
        let tone = if worst.is_some_and(|w| w.as_fraction() >= 0.9) { Tone::Destructive } else { Tone::Rest };
        let status = self.state.status_label();
        let value = if let Some(p) = percent {
            format!("{p}%")
        } else if !status.is_empty() {
            status.to_owned()
        } else {
            "Unavailable".to_owned()
        };
        Some(Summary::new("gauge", "Usage", value).tone(tone))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        if self.state.claude == Claude::Stale {
            kick(Source::Usage);
        }
        Action::Panel(PanelId::Usage)
    }
}
