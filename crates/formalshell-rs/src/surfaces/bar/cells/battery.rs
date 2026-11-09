//! The laptop battery's state icon and percentage, gone from
//! the strip without one. Critical and low while discharging put their
//! colour on the border and the ink; right click flips the percentage.

use fs_system::power::model::{Low, Thresholds, battery_icon, charge_state_label, charge_threshold_active, format_duration};
use fs_upower::DeviceState;

use crate::services::devices::power;
use crate::services::state::{self, Field};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, Part, View};
use crate::paint::{Look, Tone};

/// The label toggled from the cell, kept in state.json over settings.
pub fn show_percent(on: bool) {
    state::set(vec![Field::BatteryShowPercent(Some(on))]);
}

#[derive(Default, PartialEq)]
struct Read {
    /// The battery and where it reads low.
    battery: Option<(power::Battery, Thresholds)>,
    on_battery: bool,
    show_label: bool,
}

#[derive(Default)]
pub struct Battery {
    r: Read,
}

/// The icon the strip warms ahead of the first frame.
pub const ICONS: &[&str] = &["battery"];

impl Cell for Battery {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Power, Topic::Config, Topic::State]
    }

    fn read(&mut self, env: &Env) -> bool {
        let c = &env.store.config;
        let p = &env.store.power;
        // state.json's override wins; null there falls back to settings.
        let show_label = env.store.state.data.battery_show_percent.unwrap_or_else(|| c.bool("bar.widgets.battery.showLabel").unwrap_or(true));
        let r = Read {
            battery: p.battery.clone().map(|b| (b, crate::settings::battery_thresholds(c))),
            on_battery: p.on_battery,
            show_label,
        };
        let changed = r != self.r;
        self.r = r;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let r = &self.r;
        let Some((b, lines)) = &r.battery else { return View::hidden() };
        let pct = b.level.rounded();
        let discharging = b.state == DeviceState::Discharging;
        let charging = b.state == DeviceState::Charging;
        let low = if discharging { lines.low(b.level) } else { None };
        let threshold = charge_threshold_active(b.level, b.state, b.rate, b.time_to_full, r.on_battery);
        let icon = battery_icon(b.level, r.on_battery, threshold, *lines);
        let head = format!("BATTERY {pct}%");
        let body = match (b.time_to_full, b.time_to_empty) {
            _ if threshold => format!("{head} / THRESHOLD"),
            (Some(t), _) if charging => format!("{head} / FULL IN {}", format_duration(t)),
            (_, Some(t)) if discharging => format!("{head} / {} LEFT", format_duration(t)),
            _ => format!("{head} / {}", charge_state_label(b.level, b.state, r.on_battery, threshold)),
        };
        let tooltip = format!("{body} / RIGHT {}", if r.show_label { "HIDE %" } else { "SHOW %" });
        let mut parts = vec![Part::icon(icon)];
        if r.show_label {
            parts.push(Part::Label { text: format!("{pct}%"), weight: None });
        }
        let tone = match low {
            Some(Low::Critical) => Tone::Destructive,
            Some(Low::Warn) => Tone::Warning,
            None => Tone::Rest,
        };
        View::new(parts, look.xs).panel("power").tooltip(tooltip).tone(tone)
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => Action::BatteryPercent(!self.r.show_label),
            _ => Action::Panel("power"),
        }
    }
}
