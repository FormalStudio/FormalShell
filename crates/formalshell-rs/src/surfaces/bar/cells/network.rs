//! Wired, Wi-Fi or offline, as one icon; right click
//! flips the Wi-Fi radio.

use fs_chrome::bar::panels::PanelId;

use crate::services::devices;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Env, View};
use crate::paint::Look;

#[derive(Default)]
pub struct Network {
    state: devices::Network,
}

/// The icon the strip warms ahead of the first frame.
pub const ICONS: &[&str] = &["wifi"];

impl Cell for Network {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Network]
    }

    fn read(&mut self, env: &Env) -> bool {
        let state = env.store.network.clone();
        let changed = state != self.state;
        self.state = state;
        changed
    }

    fn view(&self, look: &Look) -> View {
        let s = &self.state;
        let icon = if s.wired {
            "globe"
        } else if s.wifi {
            "wifi"
        } else {
            "wifi-off"
        };
        let head = if s.wired {
            "NETWORK / WIRED".to_owned()
        } else if let Some((ssid, strength)) = &s.ssid {
            format!("WI-FI / {ssid} {}%", strength.rounded())
        } else if s.wifi {
            "WI-FI / CONNECTED".to_owned()
        } else {
            "NETWORK / OFFLINE".to_owned()
        };
        let tooltip = format!("{head} / RIGHT {}", if s.wifi_enabled { "WI-FI OFF" } else { "WI-FI ON" });
        View::icon(icon, look).panel(PanelId::Network).tooltip(tooltip)
    }

    fn click(&mut self, button: Button, _: (f64, f64), _: &Env) -> Action {
        match button {
            Button::Right => Action::ToggleWifi,
            _ => Action::Panel(PanelId::Network),
        }
    }
}
