//! A `bar.modules` command's last answer as a label,
//! its `class` as the cell's tone; an empty answer is no cell at all. The
//! runs themselves are the commands service's.

use crate::store::Topic;
use crate::surfaces::bar::cell::{Cell, Env, Part, Summary, View};
use crate::paint::{Look, Tone};

pub struct Command {
    id: String,
    out: crate::services::commands::Output,
}

/// A module's or plugin's `class` as the cell's tone.
pub(super) fn tone(class: &str) -> Tone {
    match class {
        "warning" => Tone::Warning,
        "critical" | "urgent" => Tone::Destructive,
        _ => Tone::Rest,
    }
}

impl Command {
    pub fn new(id: &str) -> Self {
        Self { id: id.to_owned(), out: Default::default() }
    }
}

impl Cell for Command {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Commands]
    }

    fn read(&mut self, env: &Env) -> bool {
        let out = env.store.commands.outputs.get(&self.id).cloned().unwrap_or_default();
        let changed = out != self.out;
        self.out = out;
        changed
    }

    fn summary(&self) -> Option<Summary> {
        (!self.out.text.is_empty()).then(|| Summary::new("terminal", self.id.clone(), self.out.text.clone()).tone(tone(&self.out.class)))
    }

    fn view(&self, look: &Look) -> View {
        if self.out.text.is_empty() {
            return View::hidden();
        }
        let mut view = View::new(vec![Part::Label { text: self.out.text.clone(), weight: Some(400.0) }], look.xxs)
            .tone(tone(&self.out.class))
            .tooltip(self.out.tooltip.clone());
        view.interactive = false;
        view
    }
}
