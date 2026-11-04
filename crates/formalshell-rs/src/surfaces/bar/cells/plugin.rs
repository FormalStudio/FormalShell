//! A command plugin's bar cell: what its process last printed as an icon and
//! a label, its `class` as the tone. One that has printed nothing yet takes
//! no room; one that exited is the dim PLUGIN ERROR with the reason in its
//! tooltip. Clicks and wheel notches go to the process as JSON lines.

use crate::services::plugins::{self, Display, Run};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Summary, Tone, View};

pub struct Plugin {
    id: String,
    run: Option<Run>,
}

impl Plugin {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), run: None }
    }
}

impl Cell for Plugin {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::PluginOutput]
    }

    fn read(&mut self, env: &Env) -> bool {
        let run = env.store.plugins.runs.get(&self.id).cloned();
        let changed = run != self.run;
        self.run = run;
        changed
    }

    fn view(&self, look: &Look) -> View {
        match &self.run {
            None => View::hidden(),
            Some(Run::Failed(reason)) => {
                View::new(vec![Part::Meta { text: "PLUGIN ERROR".into() }], look.xxs).tooltip(format!("{}: {reason}", self.id.to_uppercase()))
            }
            Some(Run::Live(Display { text, icon, tooltip, class, .. })) => {
                let mut parts = Vec::new();
                if !icon.is_empty() {
                    parts.push(Part::Icon { name: icon.clone(), dim: false, dot: false });
                }
                if !text.is_empty() {
                    parts.push(Part::Label { text: text.clone(), weight: None });
                }
                if parts.is_empty() {
                    return View::hidden();
                }
                View::new(parts, look.xs).tone(super::command::tone(class)).tooltip(tooltip.clone())
            }
        }
    }

    fn summary(&self) -> Option<Summary> {
        match self.run.as_ref()? {
            Run::Failed(_) => Some(Summary::new("puzzle", self.id.clone(), "Plugin error").tone(Tone::Destructive)),
            Run::Live(Display { text, icon, class, .. }) => {
                if text.is_empty() && icon.is_empty() {
                    return None;
                }
                let icon = if icon.is_empty() { "puzzle" } else { icon.as_str() };
                Some(Summary::new(icon, self.id.clone(), text.clone()).tone(super::command::tone(class)))
            }
        }
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        let name = match button {
            Button::Left => "left",
            Button::Right => "right",
            Button::Middle => "middle",
        };
        plugins::send(&self.id, plugins::click_event(name));
        Action::None
    }

    fn wheel(&mut self, up: bool, _: &Env) -> Action {
        plugins::send(&self.id, plugins::scroll_event(up));
        Action::None
    }
}
