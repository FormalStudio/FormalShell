//! Quick settings (specs/2026-11-04-quick-settings.md): the system row,
//! the sliders and a grid of toggles, each driving its service through the
//! shell's own IPC verbs or its service call, and one in-place menu at a
//! time opening under the row that owns it (§4).

pub mod grid;

use std::cell::RefCell;

use fs_audio::Command;
use fs_system::power::model::{self, battery_icon, charge_threshold_active};
use fs_upower::Profile;
use serde_json::{Value as Json, json};

use super::{Effect, Panel, View};
use crate::motion::{Animated, Kind as Clock};
use crate::services::devices::bluetooth::{ActionKind as BtAction, Ask as BtAsk};
use crate::services::devices::{self, Op, audio, bluetooth as bt, network as net};
use crate::services::power::{self, Action};
use crate::services::{display, theme};
use crate::store::{Store, Topic};
use crate::ui::{El, Event, Ink, Size, What, w};

pub const ID: &str = "quicksettings";

/// A slider's and a brightness row's step, Left, Right and a wheel notch.
const STEP: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Slider,
    Toggle,
    /// A toggle with a menu behind its arrow.
    Split,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::System => "system",
            Kind::Slider => "slider",
            Kind::Toggle => "toggle",
            Kind::Split => "split",
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
    /// A slider's level, 0 to 1.
    pub value: Option<f64>,
    /// Whether it opens a menu now (a slider's arrow shows with two or
    /// more devices).
    pub menu: bool,
}

impl Tile {
    fn new(id: &'static str, kind: Kind, glyph: &'static str, title: &str) -> Self {
        Self { id, kind, glyph, title: title.into(), subtitle: None, on: false, reason: None, value: None, menu: false }
    }

    fn toggle(id: &'static str, glyph: &'static str, title: &str, on: bool) -> Self {
        Self { on, ..Self::new(id, Kind::Toggle, glyph, title) }
    }

    fn slider(id: &'static str, glyph: &'static str, title: &str, value: f64, on: bool, menu: bool) -> Self {
        Self { on, value: Some(value), menu, ..Self::new(id, Kind::Slider, glyph, title) }
    }

    /// A toggle with a menu behind its arrow, opening while `menu`.
    fn split(mut self, menu: bool) -> Self {
        self.kind = Kind::Split;
        self.menu = menu;
        self
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

/// Power mode's menu, top to bottom, with each profile's glyph.
const PROFILES: [(Profile, &str); 3] = [(Profile::Performance, "zap"), (Profile::Balanced, "gauge"), (Profile::PowerSaver, "leaf")];

/// The brightness an open menu takes off every other row, GNOME's
/// `DIM_BRIGHTNESS`.
const DIM: f32 = 0.4;

/// The networks Wi-Fi's menu lists at most, as GNOME's does.
const NETWORKS: usize = 8;

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

/// Every audio device of one direction: node name and label.
fn audio_devices(store: &Store, sinks: bool) -> Vec<(&str, &str)> {
    store.devices.audio.devices.iter().filter(|d| d.2 == sinks).map(|d| (d.0.as_str(), d.1.as_str())).collect()
}

/// Every control in reading order: the system row, the sliders, then the
/// toggles in GNOME's order, each one the store cannot back left out or
/// disabled with its reason (§3).
pub fn tiles(store: &Store) -> Vec<Tile> {
    let mut out = Vec::new();
    if let Some((glyph, percent)) = battery(store) {
        out.push(Tile::new("battery", Kind::System, glyph, "Battery").subtitle(Some(percent)));
    }
    out.push(Tile::new("screenshot", Kind::System, "camera", "Screenshot"));
    out.push(Tile::new("settings", Kind::System, "settings", "Settings"));
    out.push(Tile::new("lock", Kind::System, "lock", "Lock"));
    out.push(Tile { menu: true, ..Tile::new("power", Kind::System, "power", "Power off") });

    let a = &store.devices.audio;
    if let Some(volume) = a.volume {
        let glyph = if a.muted { "volume-x" } else { "volume-2" };
        out.push(Tile::slider("volume", glyph, "Volume", volume, !a.muted, audio_devices(store, true).len() > 1));
    }
    if a.capturing && a.source {
        let glyph = if a.source_muted { "mic-off" } else { "mic" };
        out.push(Tile::slider("microphone", glyph, "Microphone", a.source_volume.unwrap_or(0.0), !a.source_muted, audio_devices(store, false).len() > 1));
    }
    let screens = store.display.devices();
    if let Some(first) = screens.first() {
        out.push(Tile::slider("brightness", "sun", "Brightness", first.percent as f64 / 100.0, true, screens.len() > 1));
    }

    let d = &store.devices;
    let n = &d.network;
    if !n.wired_rows.is_empty() {
        let up = n.wired_rows.iter().find(|r| r.connected);
        let name = up.map(|r| r.connection.clone().unwrap_or_else(|| r.name.clone()));
        out.push(Tile::toggle("wired", "ethernet-port", "Wired", up.is_some()).split(true).subtitle(name));
    }
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
            .split(wifi_reason.is_none())
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
            .split(bt.powered.is_some())
            .subtitle(bt_sub)
            .unless(bt.powered.is_none().then_some("No adapter")),
    );

    let profiles = d.profiles.as_ref();
    out.push(
        Tile::toggle("power-mode", "gauge", "Power mode", profiles.is_some_and(|p| p.active != Profile::Balanced))
            .split(profiles.is_some())
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
        out.push(Tile::toggle("keyboard", "keyboard", "Keyboard", l.brightness > 0).split(true).subtitle(effect));
    }

    out.push(Tile::toggle("caffeinate", "coffee", "Caffeinate", store.caffeinate.active));
    out.push(Tile::toggle("overnight", "moon-star", "Overnight", !store.state.data.overnight.is_null()));

    if let Some(output) = store.compositor.outputs.iter().find(|o| display::hdr_supported(store, &o.name)) {
        out.push(Tile::toggle("hdr", "sun-medium", "HDR", display::hdr_active(store)).subtitle(Some(output.name.clone())));
    }
    out
}

/// One line of an open menu.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// `m:<menu>:<what>`, Enter or a click acting on it; `detail` under its
    /// title, `mark` a glyph at its end (a secured network's lock).
    Row { key: String, glyph: &'static str, title: String, detail: Option<String>, mark: Option<&'static str>, checked: bool, armed: bool },
    /// A device's own level (a display's brightness).
    Slider { key: String, title: String, value: f64 },
    /// A dim line standing for rows there are none of ("Wi-Fi is off").
    Note(String),
    Separator,
}

impl Line {
    fn row(menu: &str, what: &str, glyph: &'static str, title: &str, checked: bool) -> Self {
        Line::Row { key: format!("m:{menu}:{what}"), glyph, title: title.to_owned(), detail: None, mark: None, checked, armed: false }
    }

    fn detail(mut self, d: Option<String>) -> Self {
        if let Line::Row { detail, .. } = &mut self {
            *detail = d;
        }
        self
    }

    fn mark(mut self, m: Option<&'static str>) -> Self {
        if let Line::Row { mark, .. } = &mut self {
            *mark = m;
        }
        self
    }

    /// The stop it is, if any.
    fn key(&self) -> Option<&str> {
        match self {
            Line::Row { key, .. } | Line::Slider { key, .. } => Some(key),
            Line::Note(_) | Line::Separator => None,
        }
    }
}

/// Where a menu's settings row hands the card over to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Panel(&'static str),
    /// A launcher route ("Keyboard lights" is the launcher's effect level).
    Route(&'static str),
}

/// An open menu as the store has it: its header and lines, and the panel
/// its settings row hands over to.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuView {
    pub glyph: &'static str,
    pub title: &'static str,
    pub subtitle: Option<String>,
    pub on: bool,
    pub lines: Vec<Line>,
    pub settings: Option<(&'static str, Hand)>,
}

/// The power menu's actions as `m:power:<what>` names them, with their
/// rows' words at rest and armed. Suspend needs no second Enter.
const POWER: [(&str, &str, &str); 4] = [
    ("suspend", "Suspend", ""),
    ("reboot", "Restart…", "Confirm restart?"),
    ("poweroff", "Power off…", "Confirm power off?"),
    ("logout", "Log out…", "Confirm log out?"),
];

/// `id`'s menu off the store, `armed` the power row a first Enter armed;
/// none for a control with no menu now.
pub fn menu(store: &Store, id: &str, armed: Option<&str>) -> Option<MenuView> {
    let tile = tiles(store).into_iter().find(|t| t.id == id && t.menu)?;
    let row = Line::row;
    let a = &store.devices.audio;
    Some(match id {
        "volume" => MenuView {
            glyph: tile.glyph,
            title: "Sound output",
            subtitle: Some(a.name.clone()),
            on: tile.on,
            lines: audio_devices(store, true).into_iter().map(|(name, label)| row(id, name, "speaker", label, name == a.default_sink)).collect(),
            settings: Some(("Sound settings", Hand::Panel("audio"))),
        },
        "microphone" => MenuView {
            glyph: tile.glyph,
            title: "Sound input",
            subtitle: None,
            on: tile.on,
            lines: audio_devices(store, false).into_iter().map(|(name, label)| row(id, name, "mic", label, name == a.default_source)).collect(),
            settings: Some(("Sound settings", Hand::Panel("audio"))),
        },
        "brightness" => MenuView {
            glyph: tile.glyph,
            title: "Brightness",
            subtitle: None,
            on: true,
            lines: store
                .display
                .devices()
                .into_iter()
                .map(|d| Line::Slider { key: format!("m:brightness:{}", d.id), title: d.label, value: d.percent as f64 / 100.0 })
                .collect(),
            settings: Some(("Display settings", Hand::Panel("display"))),
        },
        "wired" => MenuView {
            glyph: tile.glyph,
            title: "Wired",
            subtitle: tile.subtitle.clone(),
            on: tile.on,
            lines: store
                .devices
                .network
                .wired_rows
                .iter()
                .map(|r| row(id, &r.name, "ethernet-port", r.connection.as_deref().unwrap_or(&r.name), r.connected).detail((r.connection.is_some()).then(|| r.name.clone())))
                .collect(),
            settings: Some(("Network settings", Hand::Panel("network"))),
        },
        "wifi" => {
            let n = &store.devices.network;
            let mut lines: Vec<Line> = n
                .rows
                .iter()
                .filter(|r| r.in_range && !r.ssid.is_empty())
                .take(NETWORKS)
                .map(|r| {
                    let status = match (&n.action, &n.failure) {
                        (Some((kind, ssid)), _) if *ssid == r.ssid => Some(
                            match kind {
                                net::ActionKind::Connect => "Connecting",
                                net::ActionKind::Disconnect => "Disconnecting",
                                net::ActionKind::Forget => "Forgetting",
                            }
                            .to_owned(),
                        ),
                        (_, Some(f)) if f.ssid == r.ssid => Some(f.text.clone()),
                        _ => None,
                    };
                    row(id, &r.ssid, super::network::signal_glyph(r.signal), &r.ssid, r.connected).detail(status).mark(r.secured.then_some("lock"))
                })
                .collect();
            if lines.is_empty() {
                lines.push(Line::Note(if n.wifi_enabled { "Scanning" } else { "Wi-Fi is off" }.into()));
            }
            MenuView { glyph: tile.glyph, title: "Wi-Fi", subtitle: tile.subtitle.clone(), on: tile.on, lines, settings: Some(("All networks", Hand::Panel("network"))) }
        }
        "bluetooth" => {
            let b = &store.devices.bluetooth;
            let mut lines: Vec<Line> = Vec::new();
            if b.powered == Some(true) {
                for d in b.devices.iter().filter(|d| d.paired) {
                    let name = if d.name.is_empty() { &d.device_name } else { &d.name };
                    let doing = b.action.as_ref().filter(|(a, _)| *a == d.address).map(|(_, k)| k.text().to_owned());
                    let failed = b.failure.as_ref().filter(|(a, _)| *a == d.address).map(|(_, why)| (*why).to_owned());
                    let word = if d.connected { "Disconnect" } else { "Connect" };
                    let glyph = if d.connected { "bluetooth-connected" } else { "bluetooth" };
                    lines.push(row(id, &d.address, glyph, name, d.connected).detail(doing.or(failed).or_else(|| Some(word.to_owned()))));
                }
                if lines.is_empty() {
                    lines.push(Line::Note("No paired devices".into()));
                }
            } else {
                lines.push(Line::Note("Turn on Bluetooth to connect to devices".into()));
            }
            MenuView { glyph: tile.glyph, title: "Bluetooth", subtitle: tile.subtitle.clone(), on: tile.on, lines, settings: Some(("Bluetooth settings", Hand::Panel("bluetooth"))) }
        }
        "power-mode" => {
            let p = store.devices.profiles.as_ref()?;
            let lines = PROFILES
                .iter()
                .filter(|(x, _)| *x != Profile::Performance || p.has_performance())
                .map(|&(x, glyph)| row(id, x.to_wire(), glyph, profile_name(x), p.active == x))
                .collect();
            MenuView { glyph: tile.glyph, title: "Power mode", subtitle: tile.subtitle.clone(), on: tile.on, lines, settings: Some(("Power settings", Hand::Panel("power"))) }
        }
        "keyboard" => {
            let l = &store.lights;
            let lines = l.effects.iter().map(|(effect, label)| row(id, effect, "", label, *effect == l.effect)).collect();
            MenuView { glyph: tile.glyph, title: "Keyboard", subtitle: tile.subtitle.clone(), on: tile.on, lines, settings: Some(("Keyboard lights", Hand::Route("lights.effect"))) }
        }
        "power" => {
            let can = store.devices.logind;
            let mut lines = Vec::new();
            for (what, rest, confirm) in POWER {
                let shown = match what {
                    "suspend" => can.suspend,
                    "reboot" => can.reboot,
                    "poweroff" => can.power_off,
                    _ => {
                        if !lines.is_empty() {
                            lines.push(Line::Separator);
                        }
                        true
                    }
                };
                if shown {
                    let armed = armed == Some(what);
                    lines.push(Line::Row { key: format!("m:power:{what}"), glyph: "", title: (if armed { confirm } else { rest }).to_owned(), detail: None, mark: None, checked: false, armed });
                }
            }
            MenuView { glyph: "power", title: "Power off", subtitle: None, on: false, lines, settings: None }
        }
        _ => return None,
    })
}

/// The stop a closing menu hands the cursor back to.
fn opener(id: &str) -> String {
    if id == "power" { id.to_owned() } else { format!("{id}:menu") }
}

/// The one in-place menu: what is asked for, what is drawn, and its three
/// clocks (§2 Motion). Opening grows the box, then fades its rows in;
/// closing fades them out, then shrinks it; the dim rides beside both.
struct MenuState {
    /// The menu asked for, none to close.
    want: Option<String>,
    /// What the clocks were last set toward.
    aimed: Option<String>,
    /// The menu drawn, open or closing.
    shown: Option<String>,
    /// The box's share of its height, `spatialFast`.
    height: Animated,
    /// The rows' opacity, `effects`.
    content: Animated,
    /// The dim's share, `effectsSlow`.
    dim: Animated,
    /// The power row a first Enter armed.
    armed: Option<String>,
    /// The cursor has gone into the menu since it opened.
    entered: bool,
    /// Where the cursor goes next, once.
    cursor: Option<String>,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            want: None,
            aimed: None,
            shown: None,
            height: Animated::new(0.0, Clock::SpatialFast.curve()),
            content: Animated::new(0.0, Clock::Effects.curve()),
            dim: Animated::new(0.0, Clock::EffectsSlow.curve()),
            armed: None,
            entered: false,
            cursor: None,
        }
    }
}

/// What a body draws of the menu this frame.
struct Frame {
    id: Option<String>,
    height: f64,
    content: f32,
    /// Its rows take the keyboard: the box has landed open.
    ready: bool,
    /// The dim's share over everything but the menu.
    dim: f32,
}

impl MenuState {
    /// Sets the clocks toward what is asked, on the body's own clock.
    fn tick(&mut self, v: &View) -> Frame {
        let now = v.now;
        let ms = |c: Clock| c.ms(v.theme) * v.motion_scale;
        if self.want != self.aimed {
            let (h, c) = (self.height.value(now).clamp(0.0, 1.0), self.content.value(now).clamp(0.0, 1.0));
            match (&self.want, &self.shown) {
                (Some(want), Some(shown)) if want == shown => {
                    self.height.set(now, 1.0, ms(Clock::SpatialFast) * (1.0 - h));
                }
                (Some(want), _) => {
                    self.shown = Some(want.clone());
                    self.height.jump(0.0);
                    self.content.jump(0.0);
                    self.height.set(now, 1.0, ms(Clock::SpatialFast));
                }
                (None, _) => {
                    let fade = ms(Clock::Effects) * c;
                    self.content.set(now, 0.0, fade);
                    self.height.set_after(now, 0.0, ms(Clock::SpatialFast) * h, fade);
                }
            }
            self.dim.set(now, if self.want.is_some() { 1.0 } else { 0.0 }, ms(Clock::EffectsSlow));
            self.aimed = self.want.clone();
        }
        if self.want.is_none() && !self.height.running(now) {
            self.shown = None;
        }
        // The spatial curve overshoots and comes back under 1 in its tail: the
        // rows come in once it first lands and stay in through the rest.
        let landed = self.height.target() == 1.0 && self.height.value(now) >= 0.999;
        if self.want.is_some() && landed && self.content.target() != 1.0 {
            self.content.set(now, 1.0, ms(Clock::Effects));
        }
        let ready = self.want.is_some() && self.content.target() == 1.0;
        Frame {
            id: self.shown.clone(),
            height: self.height.value(now).clamp(0.0, 1.0),
            content: self.content.value(now).clamp(0.0, 1.0) as f32,
            ready,
            dim: self.dim.value(now).clamp(0.0, 1.0) as f32,
        }
    }

    fn running(&self, v: &View) -> bool {
        self.want != self.aimed || [&self.height, &self.content, &self.dim].iter().any(|a| a.running(v.now)) || (self.want.is_none() && self.shown.is_some())
    }

    fn open(&mut self, id: &str) {
        self.want = Some(id.to_owned());
        self.armed = None;
        self.entered = false;
    }

    /// True when a menu was open.
    fn close(&mut self) -> bool {
        let Some(id) = self.want.take() else { return false };
        self.cursor = Some(opener(&id));
        self.armed = None;
        true
    }
}

pub struct QuickSettings {
    /// The profile the Power mode toggle goes back to from Balanced: the
    /// last other one it left, as GNOME's does.
    last_profile: Profile,
    /// The first usable toggle, read as the card opens.
    start: Option<String>,
    /// Each section's usable stops as last drawn, for Tab.
    sections: RefCell<Vec<Vec<String>>>,
    menu: RefCell<MenuState>,
    /// NetworkManager scans while Wi-Fi's menu is open, as the network
    /// panel has it scan while it is.
    scan: RefCell<Option<net::Hold>>,
}

impl Default for QuickSettings {
    fn default() -> Self {
        Self {
            last_profile: Profile::PowerSaver,
            start: None,
            sections: RefCell::new(Vec::new()),
            menu: RefCell::new(MenuState::default()),
            scan: RefCell::new(None),
        }
    }
}

/// An open menu's element, its rows stops only once `ready`.
fn menu_el(v: &View, m: &MenuView, content: f32, ready: bool) -> El {
    let s = &v.theme.space;
    let stop = |e: El, key: &str| if ready { e.on(key).stop(key) } else { e };
    let rows = m
        .lines
        .iter()
        .map(|l| match l {
            Line::Row { key, glyph, title, detail, mark, checked, armed } => {
                let row = if *armed {
                    w::panel_row_with(s, None, w::label(title).ink(Ink::Destructive), None, Vec::new())
                } else {
                    let marks = mark.iter().map(|m| w::icon(*m).ink(Ink::Muted)).collect();
                    w::panel_row_checked(s, glyph, title, detail.clone(), marks, *checked)
                };
                stop(row, key)
            }
            Line::Slider { key, title, value } => {
                let track = w::slider(*value).on(key.clone());
                stop(w::panel_meter(s, Some("monitor"), title, vec![w::panel_value(format!("{}%", (value * 100.0).round()))], Some(track)), key)
            }
            Line::Note(text) => w::panel_empty(s, text),
            Line::Separator => w::separator(),
        })
        .collect();
    let settings = m.settings.map(|(label, _)| {
        let row = w::panel_row(s, "", label, None, Vec::new());
        stop(row, "m:settings")
    });
    w::quick_menu(s, m.glyph, m.title, m.subtitle.as_deref(), m.on, rows, settings, content)
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
        let system: Vec<&Tile> = all.iter().filter(|t| t.kind == Kind::System).collect();
        let sliders: Vec<&Tile> = all.iter().filter(|t| t.kind == Kind::Slider).collect();
        let toggles: Vec<&Tile> = all.iter().filter(|t| matches!(t.kind, Kind::Toggle | Kind::Split)).collect();

        let mut state = self.menu.borrow_mut();
        let frame = state.tick(v);
        let scanning = state.want.as_deref() == Some("wifi");
        if scanning != self.scan.borrow().is_some() {
            *self.scan.borrow_mut() = scanning.then(net::Hold::new);
        }
        let open = frame.id.as_deref().and_then(|id| menu(v.store, id, state.armed.as_deref()));
        // A menu whose devices went (a sink unplugged to one) closes.
        if frame.id.is_some() && open.is_none() {
            state.close();
        }
        // A row fully faded out is not drawn, so it registers no stop yet.
        if frame.ready
            && frame.content > 0.0
            && !state.entered
            && let Some(first) = open.as_ref().and_then(|m| m.lines.iter().find_map(|l| l.key().map(str::to_owned)).or_else(|| m.settings.map(|_| "m:settings".to_owned())))
        {
            state.entered = true;
            state.cursor = Some(first);
        }
        drop(state);

        // Each part is one row of the card, keyed by what it holds.
        let mut parts: Vec<(Vec<&str>, El)> = Vec::new();
        let mut row = Vec::new();
        for t in &system {
            if t.id == "battery" {
                row.push(w::quick_pill(s, t.id, t.glyph, t.subtitle.as_deref().unwrap_or("")));
            }
        }
        row.push(w::spacer());
        row.extend(system.iter().filter(|t| t.id != "battery").map(|t| w::quick_button(s, t.id, t.glyph, &t.title)));
        parts.push((system.iter().map(|t| t.id).collect(), w::row(s.quick_gap, row).fill().height(s.quick_button)));

        for t in &sliders {
            parts.push((vec![t.id], w::quick_slider(s, t.id, t.glyph, t.value.unwrap_or(0.0), t.menu)));
        }

        let items = vec![grid::Item { span: 1, height: s.quick_toggle }; toggles.len()];
        let placed = grid::place(&items, self.width(v) - s.quick_inset * 2.0, s.quick_gap, None);
        let mut rows: Vec<(Vec<&str>, Vec<El>)> = Vec::new();
        for ((t, &r), rect) in toggles.iter().zip(&placed.rows).zip(&placed.rects) {
            if rows.len() <= r {
                rows.push((Vec::new(), Vec::new()));
            }
            let subtitle = t.reason.as_deref().or(t.subtitle.as_deref());
            let el = if t.kind == Kind::Split {
                w::quick_split(s, t.id, t.glyph, &t.title, subtitle, t.on, t.available())
            } else {
                w::quick_toggle(s, t.id, t.glyph, &t.title, subtitle, t.on, t.available())
            };
            rows[r].0.push(t.id);
            rows[r].1.push(el.width(Size::Px(rect.width())));
        }
        parts.extend(rows.into_iter().map(|(ids, r)| (ids, w::row(s.quick_gap, r).fill())));
        if frame.dim > 0.0 {
            parts = parts.into_iter().map(|(ids, el)| (ids, el.shade(DIM * frame.dim))).collect();
        }

        // The menu under its owner's row, the gap before it growing with it.
        let mut owner = None;
        if let (Some(m), Some(id)) = (&open, frame.id.as_deref())
            && let Some(at) = parts.iter().position(|(ids, _)| ids.contains(&id))
        {
            let (ids, el) = parts.remove(at);
            let grown = w::column(0.0, vec![el, w::space(s.quick_gap * frame.height), menu_el(v, m, frame.content, frame.ready).reveal(frame.height)]).fill();
            parts.insert(at, (ids, grown));
            owner = Some(at);
        }

        let usable = |list: &[&Tile]| {
            let mut out = Vec::new();
            for t in list.iter().filter(|t| t.available()) {
                out.push(t.id.to_owned());
                if t.menu && t.kind != Kind::System {
                    out.push(format!("{}:menu", t.id));
                }
            }
            out
        };
        let mut sections = vec![usable(&system), usable(&sliders), usable(&toggles)];
        if let (Some(m), true, Some(at)) = (&open, frame.ready, owner) {
            let mut keys: Vec<String> = m.lines.iter().filter_map(|l| l.key().map(str::to_owned)).collect();
            if m.settings.is_some() {
                keys.push("m:settings".into());
            }
            let section = if at == 0 { 0 } else if at <= sliders.len() { 1 } else { 2 };
            sections.insert(section + 1, keys);
        }
        *self.sections.borrow_mut() = sections;

        // The host keeps its own room round a headerless body (`Frame`);
        // the body makes up the rest so every side reads `quickInset`.
        let side = s.quick_inset - s.panel_row_inset;
        let top = s.quick_inset - s.panel_gap;
        let body = w::column(s.quick_gap, parts.into_iter().map(|(_, el)| el).collect()).fill().pad(side, top, side, 0.0);
        let bottom = (s.quick_inset - s.panel_row_inset - w::closing_room(s, &body)).max(0.0);
        body.pad(side, top, side, bottom)
    }

    fn start(&mut self, fx: &mut Effect) {
        if let Some(p) = fx.store.devices.profiles.as_ref().filter(|p| p.active != Profile::Balanced) {
            self.last_profile = p.active;
        }
        self.start = tiles(fx.store).into_iter().find(|t| matches!(t.kind, Kind::Toggle | Kind::Split) && t.available()).map(|t| t.id.to_owned());
        fx.service(power::ask);
    }

    fn cursor_start(&self) -> Option<String> {
        self.start.clone()
    }

    fn take_cursor(&mut self) -> Option<String> {
        self.menu.get_mut().cursor.take()
    }

    fn closed(&mut self) {
        *self.menu.get_mut() = MenuState::default();
        *self.scan.get_mut() = None;
    }

    fn wake(&self, v: &View) -> Option<std::time::Instant> {
        self.menu.borrow().running(v).then_some(v.now)
    }

    fn follows(&self, v: &View) -> bool {
        let m = self.menu.borrow();
        m.want != m.aimed || m.height.running(v.now)
    }

    fn escape(&mut self) -> bool {
        self.menu.get_mut().close()
    }

    fn call(&mut self, verb: &str, arg: &str) -> Option<String> {
        let m = self.menu.get_mut();
        match verb {
            "menu" => {
                m.open(arg);
                Some("ok".into())
            }
            "back" => Some(if m.close() { "ok".into() } else { "error: no menu is open".into() }),
            "state" => {
                let now = std::time::Instant::now();
                let share = |a: &Animated| (a.value(now).clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
                Some(json!({ "menu": m.want, "shown": m.shown, "armed": m.armed, "dim": m.want.is_some(), "height": share(&m.height), "content": share(&m.content) }).to_string())
            }
            _ => None,
        }
    }

    fn spatial(&self) -> bool {
        true
    }

    fn steps_at(&self, stop: &str) -> bool {
        matches!(stop, "volume" | "microphone" | "brightness") || stop.starts_with("m:")
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

    fn reached(&mut self, stop: &str, _fx: &mut Effect) {
        let m = self.menu.get_mut();
        if m.armed.as_ref().is_some_and(|a| format!("m:power:{a}") != stop) {
            m.armed = None;
        }
    }

    fn event(&mut self, ev: &Event, fx: &mut Effect) {
        match ev.what {
            What::Click => self.go(&ev.on.clone(), fx),
            What::Fraction(f) => self.level(&ev.on, Some(f), 0.0, fx),
            What::Wheel(n) => self.level(&ev.on, None, n as f64 * STEP, fx),
            _ => {}
        }
    }

    fn activate(&mut self, stop: &str, fx: &mut Effect) {
        self.go(stop, fx);
    }

    fn step(&mut self, stop: &str, direction: i32, fx: &mut Effect) {
        if stop.starts_with("m:") && !stop.starts_with("m:brightness:") {
            if direction < 0 {
                self.menu.get_mut().close();
            }
            return;
        }
        self.level(stop, None, direction as f64 * STEP, fx);
    }
}

impl QuickSettings {
    fn go(&mut self, id: &str, fx: &mut Effect) {
        if let Some(owner) = id.strip_suffix(":menu") {
            return self.toggle_menu(owner, fx.store);
        }
        if let Some(owner) = id.strip_suffix(":icon") {
            return mute(owner, fx);
        }
        if id == "m:settings" {
            let shown = self.menu.get_mut().want.clone();
            match shown.and_then(|m| menu(fx.store, &m, None)).and_then(|m| m.settings).map(|(_, h)| h) {
                Some(Hand::Panel(p)) => fx.panel = Some(p),
                Some(Hand::Route(r)) => fx.summon = Some(r),
                None => {}
            }
            return;
        }
        if let Some(rest) = id.strip_prefix("m:") {
            return self.menu_row(rest, fx);
        }
        let Some(tile) = tiles(fx.store).into_iter().find(|t| t.id == id && t.available()) else { return };
        match tile.id {
            "battery" => fx.panel = Some("power"),
            "screenshot" => {
                fx.close = true;
                fx.ipc(&["screenshot", "pick", "smart", "default"]);
            }
            "settings" => fx.panel = Some("settings"),
            "lock" => fx.call = Some(("lock", "lock")),
            "power" => self.toggle_menu("power", fx.store),
            "volume" | "microphone" => mute(tile.id, fx),
            "wired" => {
                let n = &fx.store.devices.network;
                if let Some(r) = n.wired_rows.iter().find(|r| r.connected).or(n.wired_rows.first()) {
                    let (name, connected) = (r.name.clone(), r.connected);
                    fx.service(move |ctx| net::wired_toggle(ctx, name, connected));
                }
            }
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

    /// The arrow, the Power button or `quicksettings menu`: opens `id`'s
    /// menu in place of any other, or closes it when it is the one open.
    fn toggle_menu(&mut self, id: &str, store: &Store) {
        let m = self.menu.get_mut();
        if m.want.as_deref() == Some(id) {
            m.close();
        } else if menu(store, id, None).is_some() {
            m.open(id);
        }
    }

    /// Enter or a click on `<menu>:<what>`.
    fn menu_row(&mut self, rest: &str, fx: &mut Effect) {
        let Some((owner, what)) = rest.split_once(':') else { return };
        let what = what.to_owned();
        match owner {
            "volume" => fx.service(move |_| audio::command(Command::SetDefaultSink(Some(what)))),
            "microphone" => fx.service(move |_| audio::command(Command::SetDefaultSource(Some(what)))),
            "wired" => {
                if let Some(r) = fx.store.devices.network.wired_rows.iter().find(|r| r.name == what) {
                    let connected = r.connected;
                    fx.service(move |ctx| net::wired_toggle(ctx, what, connected));
                }
            }
            "wifi" => wifi_row(what, fx),
            "bluetooth" => {
                let b = &fx.store.devices.bluetooth;
                if b.action.is_none()
                    && let Some(d) = b.devices.iter().find(|d| d.address == what && d.paired)
                {
                    bt::ask(BtAsk::Run(what, if d.connected { BtAction::Disconnect } else { BtAction::Connect }));
                }
            }
            "power-mode" => {
                let Some(p) = Profile::from_wire(&what) else { return };
                if p != Profile::Balanced {
                    self.last_profile = p;
                }
                fx.service(move |ctx| devices::run(ctx, Op::Profile(p)));
            }
            "keyboard" => fx.ipc(&["lights", "effect", &what]),
            "power" => self.power(&what, fx),
            _ => {}
        }
    }

    /// Suspend at once; the rest arm on a first Enter and run on a second,
    /// as the launcher's Shutdown and Reboot rows do. The card closes first.
    fn power(&mut self, what: &str, fx: &mut Effect) {
        let m = self.menu.get_mut();
        let action = match what {
            "suspend" => Some(Action::Suspend),
            "reboot" => Some(Action::Reboot),
            "poweroff" => Some(Action::PowerOff),
            "logout" => None,
            _ => return,
        };
        if what != "suspend" && m.armed.as_deref() != Some(what) {
            m.armed = Some(what.to_owned());
            return;
        }
        m.armed = None;
        fx.close = true;
        match action {
            Some(a) => power::request(a),
            None => fx.ipc(&["power", "logout"]),
        }
    }

    /// A slider or a menu's brightness row set to `to`, or moved by `by`.
    fn level(&mut self, key: &str, to: Option<f64>, by: f64, fx: &mut Effect) {
        let store = fx.store;
        let at = |cur: f64| to.unwrap_or(cur + by).clamp(0.0, 1.0);
        let a = &store.devices.audio;
        match key {
            "volume" => {
                let v = at(a.volume.unwrap_or(0.0));
                fx.service(move |ctx| devices::run(ctx, Op::Volume(v)));
            }
            "microphone" => {
                let v = at(a.source_volume.unwrap_or(0.0));
                fx.service(move |ctx| devices::run(ctx, Op::SourceVolume(v)));
            }
            "brightness" => {
                let screens = store.display.devices();
                let Some(first) = screens.first() else { return };
                let pct = at(first.percent as f64 / 100.0) * 100.0;
                fx.service(move |ctx| {
                    for d in &screens {
                        display::set_percent(ctx, &d.id, pct, d.max);
                    }
                });
            }
            _ => {
                let Some(id) = key.strip_prefix("m:brightness:") else { return };
                let Some(d) = store.display.devices().into_iter().find(|d| d.id == id) else { return };
                let pct = at(d.percent as f64 / 100.0) * 100.0;
                fx.service(move |ctx| display::set_percent(ctx, &d.id, pct, d.max));
            }
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

/// Enter on a network in Wi-Fi's menu: a saved or open one connects, a
/// secured one nobody knows (or whose password just failed) hands the card
/// over to the network panel with its passphrase step open, and the
/// connected one stays as it is.
fn wifi_row(ssid: String, fx: &mut Effect) {
    let n = &fx.store.devices.network;
    let Some(row) = n.row(&ssid).filter(|r| !r.connected) else { return };
    if n.action.is_some() {
        return;
    }
    let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
    if retype || (row.secured && !row.known) {
        fx.panel = Some("network");
        fx.then = Some(format!("wifi:{ssid}"));
        return;
    }
    fx.service(move |ctx| net::connect(ctx, ssid, net::Secret::Saved));
}

/// A slider's icon (or Enter on its track): mute its device.
fn mute(id: &str, fx: &mut Effect) {
    let op = match id {
        "volume" => Op::ToggleMute,
        "microphone" => Op::ToggleSourceMute,
        _ => return,
    };
    fx.service(move |ctx| devices::run(ctx, op));
}

/// The open menu for `quicksettings status`: its id, its lines and the
/// armed row, off the module's own answer to `state`.
pub fn status_menu(store: &Store, state: &str) -> (Json, bool) {
    let state: Json = serde_json::from_str(state).unwrap_or(Json::Null);
    let id = state["menu"].as_str();
    let armed = state["armed"].as_str();
    let open = id.and_then(|id| menu(store, id, armed));
    let lines = open.as_ref().map(|m| {
        m.lines
            .iter()
            .filter_map(|l| match l {
                Line::Row { key, title, detail, checked, armed, .. } => Some(json!({ "id": key, "title": title, "detail": detail, "checked": checked, "armed": armed })),
                Line::Slider { key, title, value } => Some(json!({ "id": key, "title": title, "value": value })),
                Line::Note(_) | Line::Separator => None,
            })
            .chain(m.settings.map(|(label, hand)| match hand {
                Hand::Panel(p) => json!({ "id": "m:settings", "title": label, "panel": p }),
                Hand::Route(r) => json!({ "id": "m:settings", "title": label, "route": r }),
            }))
            .collect::<Vec<_>>()
    });
    let notes: Option<Vec<&str>> = open.as_ref().map(|m| m.lines.iter().filter_map(|l| if let Line::Note(t) = l { Some(t.as_str()) } else { None }).collect());
    let header = open.as_ref().map(|m| json!({ "title": m.title, "subtitle": m.subtitle, "on": m.on }));
    (
        json!({ "id": id, "shown": state["shown"], "height": state["height"], "content": state["content"], "header": header, "rows": lines, "notes": notes }),
        state["dim"].as_bool().unwrap_or(false),
    )
}
