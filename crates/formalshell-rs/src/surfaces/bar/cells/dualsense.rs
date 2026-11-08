//! A gamepad icon and the controller's battery, gone
//! from the strip with no controller. Warn and critical colour the border
//! and the ink, as the laptop battery's cell does.

use fs_system::dualsense::{Supply, state_line};
use fs_system::power::model::Low;

use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Summary, Tone, View};

fn tone(s: &Supply) -> Tone {
    match s.low {
        Some(Low::Critical) => Tone::Destructive,
        Some(Low::Warn) => Tone::Warning,
        None => Tone::Rest,
    }
}

#[derive(Default)]
pub struct Dualsense {
    supply: Option<Supply>,
    show_label: bool,
}

impl Cell for Dualsense {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices, Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let supply = env.store.devices.power.dualsense.clone();
        let show_label = env.store.config.bool("bar.widgets.dualsense.showLabel").unwrap_or(true);
        let changed = supply != self.supply || show_label != self.show_label;
        (self.supply, self.show_label) = (supply, show_label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let Some(s) = &self.supply else { return View::hidden() };
        let head = format!("DUALSENSE {}%", s.level.rounded());
        let line = state_line(Some(s));
        let tooltip = if line.is_empty() { head } else { format!("{head} / {line}") };
        let mut parts = vec![Part::Icon { name: "gamepad-2".into(), dim: false, dot: false }];
        if self.show_label {
            parts.push(Part::Label { text: format!("{}%", s.level.rounded()), weight: None });
        }
        View::new(parts, look.xs).panel("dualsense").tooltip(tooltip).tone(tone(s))
    }

    fn summary(&self) -> Option<Summary> {
        let s = self.supply.as_ref()?;
        Some(Summary::new("gamepad-2", "DualSense", format!("{}%", s.level.rounded())).tone(tone(s)))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("dualsense")
    }
}
