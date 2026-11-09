//! One monitor icon opening the display panel. A session
//! always has an output, so there is no absent state, and no single number
//! to summarise; `bar.widgets.display.showLabel` opts a name in.

use fs_chrome::bar::panels::PanelId;

use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Display {
    show_label: bool,
}

impl Cell for Display {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let show_label = env.store.config.bool("bar.widgets.display.showLabel").unwrap_or(false);
        let changed = show_label != self.show_label;
        self.show_label = show_label;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let mut parts = vec![Part::icon("monitor")];
        if self.show_label {
            parts.push(Part::Name { text: "Display".into(), max: f64::INFINITY, dim: false });
        }
        View::new(parts, look.xs).panel(PanelId::Display).tooltip("DISPLAY")
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel(PanelId::Display)
    }
}
