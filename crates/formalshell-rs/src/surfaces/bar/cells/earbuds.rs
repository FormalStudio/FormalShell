//! A headphones icon and the active device's worst bud
//! level (the case left out), gone from the strip until a bud has reported
//! one. The cell holds the earbuds service for as long as it exists, which
//! is what keeps every backend idle on a strip without it.

use fs_devices::earbuds::{battery_summary, worst_level};

use crate::services::devices::earbuds;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Look, Part, Summary, View};

#[derive(Default, PartialEq)]
struct Read {
    worst: f64,
    tooltip: String,
    show_label: bool,
}

pub struct Earbuds {
    r: Read,
}

impl Earbuds {
    pub fn new() -> Self {
        earbuds::acquire();
        Self { r: Read { worst: -1.0, ..Read::default() } }
    }
}

impl Drop for Earbuds {
    fn drop(&mut self) {
        earbuds::release();
    }
}

impl Cell for Earbuds {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices, Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let dev = env.store.devices.earbuds.active();
        let r = Read {
            worst: worst_level(dev),
            tooltip: dev.map_or(String::new(), |d| format!("{} / {}", d.name.to_uppercase(), battery_summary(Some(d)))),
            show_label: env.store.config.bool("bar.widgets.earbuds.showLabel").unwrap_or(true),
        };
        let changed = r != self.r;
        self.r = r;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let r = &self.r;
        if r.worst < 0.0 {
            return View::hidden();
        }
        let mut parts = vec![Part::Icon { name: "headphones".into(), dim: false, dot: false }];
        if r.show_label {
            parts.push(Part::Label { text: format!("{}%", r.worst), weight: None });
        }
        View::new(parts, look.xs).panel("earbuds").tooltip(r.tooltip.clone())
    }

    fn summary(&self) -> Option<Summary> {
        (self.r.worst >= 0.0).then(|| Summary::new("headphones", "Earbuds", format!("{}%", self.r.worst)))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel("earbuds")
    }
}
