//! A phone icon with a primary dot while messages are
//! unread, the battery beside it, the cell at 60% while the phone is out of
//! range. Absent unless `omarchy-iphone-bridge` is installed.

use fs_chrome::bar::panels::PanelId;

use crate::services::iphone::State;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, Summary, View};
use crate::paint::Look;

pub struct Iphone {
    state: State,
    label: bool,
}

impl Default for Iphone {
    fn default() -> Self {
        Self { state: State::default(), label: true }
    }
}

impl Cell for Iphone {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Iphone]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.iphone.clone();
        let label = env.store.config.bool("bar.widgets.iphone.showLabel").unwrap_or(true);
        let changed = state != self.state || label != self.label;
        (self.state, self.label) = (state, label);
        changed
    }

    fn view(&self, look: &Look) -> View {
        let s = &self.state;
        if !s.installed {
            return View::hidden();
        }
        let percent = s.battery.map(|b| b.rounded());
        let mut parts = vec![Part::icon("smartphone").dot(s.unread > 0)];
        if let (true, Some(p)) = (self.label, percent) {
            parts.push(Part::Label { text: format!("{p}%"), weight: None });
        }
        let mut tooltip = vec![if s.device_name.is_empty() { "IPHONE".to_owned() } else { s.device_name.clone() }];
        tooltip.push(if s.connected { "CONNECTED" } else { "OUT OF RANGE" }.to_owned());
        if let Some(p) = percent {
            tooltip.push(format!("{p}%"));
        }
        if s.unread > 0 {
            tooltip.push(format!("{} UNREAD", s.unread));
        }
        let mut view = View::new(parts, look.xs).panel(PanelId::Iphone).tooltip(tooltip.join(" / "));
        view.opacity = if s.connected { 1.0 } else { 0.6 };
        view
    }

    fn summary(&self) -> Option<Summary> {
        let s = &self.state;
        if !s.installed {
            return None;
        }
        let name = if s.device_name.is_empty() { "iPhone" } else { s.device_name.as_str() };
        let mut value = if s.connected { "Connected" } else { "Out of range" }.to_owned();
        if let Some(b) = s.battery {
            value.push_str(&format!(", {}%", b.rounded()));
        }
        Some(Summary::new("smartphone", name, value))
    }

    fn click(&mut self, _: Button, _: (f64, f64), _: &Env) -> Action {
        Action::Panel(PanelId::Iphone)
    }
}
