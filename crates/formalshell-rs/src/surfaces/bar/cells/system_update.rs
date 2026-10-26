//! A package icon and the model's own summary
//! ("2 behind", "Up to date", "No flake"). Visible in every state, since the
//! user opted in by naming it. Behind inputs make it `warning`.

use crate::services::info::update::State;
use crate::services::wants::{Source, Want};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Tone, View};

pub struct SystemUpdate {
    want: Want,
    state: State,
    label: bool,
}

impl Default for SystemUpdate {
    fn default() -> Self {
        Self { want: Want::new(Source::SystemUpdate), state: State::default(), label: true }
    }
}

impl Cell for SystemUpdate {
    fn visible(&mut self, on: bool) {
        self.want.hold(on);
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Info]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.info.update.clone();
        let label = env.store.config.bool("bar.widgets.systemUpdate.showLabel").unwrap_or(true);
        let changed = state != self.state || label != self.label;
        (self.state, self.label) = (state, label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let behind = self.state.counts().behind;
        let summary = self.state.summary();
        let mut parts = vec![Part::Icon { name: if behind > 0 { "package-plus" } else { "package" }.into(), dim: false, dot: false }];
        if self.label {
            parts.push(Part::Meta { text: summary.to_uppercase() });
        }
        View::new(parts, look.xs)
            .tone(if behind > 0 { Tone::Warning } else { Tone::Rest })
            .panel("systemupdate")
            .tooltip(format!("FLAKE INPUTS / {}", summary.to_uppercase()))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("systemupdate")
    }
}
