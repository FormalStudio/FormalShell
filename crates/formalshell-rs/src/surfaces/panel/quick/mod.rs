//! Quick settings (specs/2026-11-04-quick-settings.md): the system row
//! over a grid of toggles, each driving its service through the shell's
//! own IPC verbs, so the card and `formalshell-ipc` act on one path.

pub mod grid;

use std::cell::RefCell;

use fs_system::power::model::{self, battery_icon, charge_threshold_active};
use fs_upower::Profile;

use super::{Effect, Panel, View};
use crate::services::{devices, display, theme};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Size, What, w};

pub const ID: &str = "quicksettings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Toggle,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::System => "system",
            Kind::Toggle => "toggle",
        }
    }
}

/// One control as the store has it now.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pub id: &'static str,
    pub kind: Kind,
    pub glyph: &'static str,
    pub title: String,
    pub subtitle: Option<String>,
    pub on: bool,
    /// Why it cannot be used; `None` while it can.
    pub reason: Option<String>,
}

impl Tile {
    fn new(id: &'static str, kind: Kind, glyph: &'static str, title: &str) -> Self {
        Self { id, kind, glyph, title: title.into(), subtitle: None, on: false, reason: None }
    }

    fn toggle(id: &'static str, glyph: &'static str, title: &str, on: bool) -> Self {
        Self { on, ..Self::new(id, Kind::Toggle, glyph, title) }
    }

    fn subtitle(mut self, s: Option<String>) -> Self {
        self.subtitle = s.filter(|s| !s.is_empty());
        self
    }

    fn unless(mut self, reason: Option<&str>) -> Self {
        self.reason = reason.map(str::to_owned);
        self
    }

    pub fn available(&self) -> bool {
        self.reason.is_none()
    }
}

fn profile_name(p: Profile) -> &'static str {
    match p {
        Profile::PowerSaver => "Power saver",
        Profile::Balanced => "Balanced",
        Profile::Performance => "Performance",
    }
}

/// The battery pill's glyph and percent, as the bar's battery cell reads
/// them; none without a laptop battery.
fn battery(store: &Store) -> Option<(&'static str, String)> {
    let p = &store.devices.power;
    let b = p.battery.as_ref()?;
    let warn = store.config.f64("battery.warnPercent").unwrap_or(model::DEFAULT_WARN_PCT);
    let threshold = charge_threshold_active(b.percent, b.state, b.rate, b.time_to_full, p.on_battery);
    Some((battery_icon(b.percent, p.on_battery, threshold, Some(warn)), format!("{}%", b.percent)))
}

/// Every control in reading order: the system row, then the toggles in
/// GNOME's order, each one the store cannot back left out or disabled
/// with its reason (§3).
pub fn tiles(store: &Store) -> Vec<Tile> {
    let mut out = Vec::new();
    if let Some((glyph, percent)) = battery(store) {
        out.push(Tile::new("battery", Kind::System, glyph, "Battery").subtitle(Some(percent)));
    }
    out.push(Tile::new("screenshot", Kind::System, "camera", "Screenshot"));
    out.push(Tile::new("settings", Kind::System, "settings", "Settings"));
    out.push(Tile::new("lock", Kind::System, "lock", "Lock"));

    let d = &store.devices;
    let n = &d.network;
    let wifi_reason = if !n.known {
        Some("No NetworkManager")
    } else if n.wifi_device.is_none() {
        Some("No Wi-Fi adapter")
    } else {
        None
    };
    let wifi_on = n.wifi_enabled && wifi_reason.is_none();
    out.push(
        Tile::toggle("wifi", if wifi_on { "wifi" } else { "wifi-off" }, "Wi-Fi", wifi_on)
            .subtitle(n.ssid.as_ref().filter(|_| wifi_on).map(|(s, _)| s.clone()))
            .unless(wifi_reason),
    );

    let bt = &d.bluetooth;
    let bt_on = bt.powered == Some(true);
    let connected: Vec<&fs_devices::bluetooth::Device> = bt.devices.iter().filter(|x| x.connected).collect();
    let bt_sub = match connected.as_slice() {
        _ if !bt_on => None,
        [] => None,
        [one] => Some(if one.name.is_empty() { one.device_name.clone() } else { one.name.clone() }),
        many => Some(format!("{} connected", many.len())),
    };
    out.push(
        Tile::toggle("bluetooth", if bt_on { "bluetooth" } else { "bluetooth-off" }, "Bluetooth", bt_on)
            .subtitle(bt_sub)
            .unless(bt.powered.is_none().then_some("No adapter")),
    );

    let profiles = d.profiles.as_ref();
    out.push(
        Tile::toggle("power-mode", "gauge", "Power mode", profiles.is_some_and(|p| p.active != Profile::Balanced))
            .subtitle(profiles.map(|p| profile_name(p.active).to_owned()))
            .unless(profiles.is_none().then_some("power-profiles-daemon is not running")),
    );

    let nl = &store.nightlight;
    let nl_reason = (!nl.active).then(|| nl.last_error.lines().next().unwrap_or("").trim()).filter(|r| !r.is_empty());
    out.push(Tile::toggle("night-light", "lightbulb", "Night light", nl.active).unless(nl_reason));

    let pinned = matches!(theme::mode_key(store.config.settings()), "dark" | "light");
    out.push(Tile::toggle("dark-style", "moon", "Dark style", store.state.data.mode == "dark").unless(pinned.then_some("Set in settings.json")));

    out.push(Tile::toggle("dnd", "bell-off", "Do not disturb", store.notifications.model.dnd));

    let l = &store.lights;
    if l.available {
        let effect = l.effects.iter().find(|(id, _)| *id == l.effect).map(|(_, label)| label.clone());
        out.push(Tile::toggle("keyboard", "keyboard", "Keyboard", l.brightness > 0).subtitle(effect));
    }

    out.push(Tile::toggle("caffeinate", "coffee", "Caffeinate", store.caffeinate.active));
    out.push(Tile::toggle("overnight", "moon-star", "Overnight", !store.state.data.overnight.is_null()));

    if let Some(output) = store.compositor.outputs.iter().find(|o| display::hdr_supported(store, &o.name)) {
        out.push(Tile::toggle("hdr", "sun-medium", "HDR", display::hdr_active(store)).subtitle(Some(output.name.clone())));
    }
    out
}

pub struct QuickSettings {
    /// The profile the Power mode toggle goes back to from Balanced: the
    /// last other one it left, as GNOME's does.
    last_profile: Profile,
    /// The first usable toggle, read as the card opens.
    start: Option<String>,
    /// Each section's usable stops as last drawn, for Tab.
    sections: RefCell<Vec<Vec<String>>>,
}

impl Default for QuickSettings {
    fn default() -> Self {
        Self { last_profile: Profile::PowerSaver, start: None, sections: RefCell::new(Vec::new()) }
    }
}

impl Panel for QuickSettings {
    fn id(&self) -> &'static str {
        ID
    }

    fn title(&self, _: &View) -> String {
        "Quick settings".into()
    }

    fn header(&self) -> bool {
        false
    }

    fn closable(&self) -> bool {
        false
    }

    fn reads(&self) -> &'static [Topic] {
        &[Topic::Devices, Topic::Display, Topic::Compositor, Topic::Config, Topic::State, Topic::Notifications, Topic::NightLight, Topic::Lights, Topic::Caffeinate]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.panel_width_wide
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let all = tiles(v.store);
        let (system, toggles): (Vec<&Tile>, Vec<&Tile>) = all.iter().partition(|t| t.kind == Kind::System);

        let mut row = Vec::new();
        for t in &system {
            if t.id == "battery" {
                row.push(w::quick_pill(s, t.id, t.glyph, t.subtitle.as_deref().unwrap_or("")));
            }
        }
        row.push(w::spacer());
        row.extend(system.iter().filter(|t| t.id != "battery").map(|t| w::quick_button(s, t.id, t.glyph, &t.title)));
        let mut parts = vec![w::row(s.quick_gap, row).fill().height(s.quick_button)];

        let items = vec![grid::Item { span: 1, height: s.quick_toggle }; toggles.len()];
        let placed = grid::place(&items, self.width(v) - s.quick_inset * 2.0, s.quick_gap, None);
        let mut rows: Vec<Vec<El>> = Vec::new();
        for ((t, &r), rect) in toggles.iter().zip(&placed.rows).zip(&placed.rects) {
            if rows.len() <= r {
                rows.push(Vec::new());
            }
            let subtitle = t.reason.as_deref().or(t.subtitle.as_deref());
            rows[r].push(w::quick_toggle(s, t.id, t.glyph, &t.title, subtitle, t.on, t.available()).width(Size::Px(rect.width())));
        }
        parts.extend(rows.into_iter().map(|r| w::row(s.quick_gap, r).fill()));

        let usable = |list: &[&Tile]| list.iter().filter(|t| t.available()).map(|t| t.id.to_owned()).collect::<Vec<_>>();
        *self.sections.borrow_mut() = vec![usable(&system), usable(&toggles)];

        // The host keeps its own room round a headerless body (`Frame`);
        // the body makes up the rest so every side reads `quickInset`.
        let side = s.quick_inset - s.panel_row_inset;
        let top = s.quick_inset - s.panel_gap;
        let body = w::column(s.quick_gap, parts).fill().pad(side, top, side, 0.0);
        let bottom = (s.quick_inset - s.panel_row_inset - w::closing_room(s, &body)).max(0.0);
        body.pad(side, top, side, bottom)
    }

    fn start(&mut self, fx: &mut Effect) {
        if let Some(p) = fx.store.devices.profiles.as_ref().filter(|p| p.active != Profile::Balanced) {
            self.last_profile = p.active;
        }
        self.start = tiles(fx.store).into_iter().find(|t| t.kind == Kind::Toggle && t.available()).map(|t| t.id.to_owned());
    }

    fn cursor_start(&self) -> Option<String> {
        self.start.clone()
    }

    fn spatial(&self) -> bool {
        true
    }

    fn tab(&mut self, stop: Option<&str>, direction: i32) -> Option<String> {
        let sections: Vec<Vec<String>> = self.sections.borrow().iter().filter(|s| !s.is_empty()).cloned().collect();
        if sections.is_empty() {
            return None;
        }
        let at = stop.and_then(|k| sections.iter().position(|s| s.iter().any(|x| x == k))).unwrap_or(0) as i32;
        let next = (at + direction).rem_euclid(sections.len() as i32) as usize;
        sections[next].first().cloned()
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        if let What::Click = ev.what {
            self.go(&ev.on.clone(), fx);
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        self.go(stop, fx);
    }
}

impl QuickSettings {
    fn go(&mut self, id: &str, fx: &mut Effect) {
        let Some(tile) = tiles(fx.store).into_iter().find(|t| t.id == id && t.available()) else { return };
        match tile.id {
            "battery" => fx.panel = Some("power"),
            "screenshot" => {
                fx.close = true;
                fx.ipc(&["screenshot", "pick", "smart", "default"]);
            }
            "settings" => fx.panel = Some("settings"),
            "lock" => fx.call = Some(("lock", "lock")),
            "wifi" => fx.ipc(&["network", "wifi", if tile.on { "false" } else { "true" }]),
            "bluetooth" => fx.ipc(&["bluetooth", "toggle"]),
            "power-mode" => self.power_mode(fx),
            "night-light" => fx.ipc(&["nightlight", "toggle"]),
            "dark-style" => fx.ipc(&["theme", "mode", "toggle"]),
            "dnd" => fx.ipc(&["notifications", "toggleDnd"]),
            "keyboard" => fx.ipc(&["lights", "toggle"]),
            "caffeinate" => fx.ipc(&["caffeinate", "toggle"]),
            "overnight" => fx.ipc(&["overnight", "toggle"]),
            "hdr" => fx.ipc(&["hdr", "toggle"]),
            _ => {}
        }
    }

    /// Balanced and the last other profile, flipped. The shell has no IPC
    /// verb for a profile; this is the power panel's own call.
    fn power_mode(&mut self, fx: &mut Effect) {
        let Some(p) = fx.store.devices.profiles.as_ref() else { return };
        let next = if p.active == Profile::Balanced {
            if self.last_profile == Profile::Performance && !p.has_performance() { Profile::PowerSaver } else { self.last_profile }
        } else {
            self.last_profile = p.active;
            Profile::Balanced
        };
        fx.service(move |ctx| devices::run(ctx, devices::Op::Profile(next)));
    }
}
