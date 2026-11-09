//! The default adapter's state as one icon; right
//! click flips its radio.

use crate::services::devices;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Bluetooth {
    state: devices::Bluetooth,
}

/// The icon the strip warms ahead of the first frame.
pub const ICONS: &[&str] = &["bluetooth"];

impl Cell for Bluetooth {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Bluetooth]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.bluetooth.clone();
        let changed = state != self.state;
        self.state = state;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let s = &self.state;
        let icon = match s.powered {
            Some(true) if !s.connected.is_empty() => "bluetooth-connected",
            Some(true) => "bluetooth",
            _ => "bluetooth-off",
        };
        let tooltip = match s.powered {
            None => "BLUETOOTH / NO ADAPTER".to_owned(),
            Some(on) => {
                let head = if !on {
                    "BLUETOOTH / OFF".to_owned()
                } else if s.connected.is_empty() {
                    "BLUETOOTH / NO DEVICES".to_owned()
                } else {
                    format!("BLUETOOTH / {}", s.connected.join(", "))
                };
                format!("{head} / RIGHT {}", if on { "RADIO OFF" } else { "RADIO ON" })
            }
        };
        View::icon(icon, look).panel("bluetooth").tooltip(tooltip)
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right if self.state.powered.is_some() => Action::ToggleBluetooth,
            Button::Right => Action::None,
            _ => Action::Panel("bluetooth"),
        }
    }
}
