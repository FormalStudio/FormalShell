//! Quick settings (specs/2026-11-04-quick-settings.md): the system row,
//! the sliders and a grid of toggles, each driving its service through the
//! shell's own IPC verbs or its service call, the bar cells the chevron
//! governs as info tiles after them (§7), and one in-place menu at a time
//! opening under the row that owns it (§4).

pub mod grid;
pub mod piece;

use std::cell::RefCell;

use fs_audio::{Command, Visual};
use fs_chrome::bar::layout::{Builtin, Entry, EntryKind};
use fs_chrome::bar::panels::widget_panel;
use fs_types::{BtAddress, Edge, Region};
use fs_info::reminders;
use fs_system::capture::elapsed_label;
use fs_system::power::model::{battery_icon, charge_threshold_active};
use fs_upower::Profile;
use serde_json::{Value as Json, json};

use super::{Effect, Panel, View};
use crate::motion::{Animated, Kind as Clock};
use crate::services::devices::bluetooth::{ActionKind as BtAction, Ask as BtAsk};
use crate::services::devices::{self, audio, bluetooth as bt, network as net};
use crate::services::power::{self, Action};
use crate::runtime::now_ms;
use crate::services::ipad::Link;
use crate::services::{display, theme};
use crate::store::{Store, Topic};
use crate::surfaces::activity;
use crate::surfaces::bar::cell::{Action as CellAction, Button, Cell, Env, Summary};
use crate::surfaces::bar::cells;
use crate::ui::el::Ring;
use crate::ui::{El, Event, Ink, Size, What, w};

pub const ID: &str = "quicksettings";

/// What opened the card (§10): a chevron in a region of the strip, the
/// sidebar's chevron button, or an IPC call or keybind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Origin {
    Chevron(Region),
    Sidebar,
    #[default]
    Ipc,
}

impl Origin {
    pub fn label(self) -> String {
        match self {
            Origin::Chevron(r) => format!("chevron:{}", r.as_str()),
            Origin::Sidebar => "sidebar".into(),
            Origin::Ipc => "ipc".into(),
        }
    }

    /// The region whose governed cells become info tiles; with no chevron,
    /// the right region's.
    pub fn region(self) -> Region {
        match self {
            Origin::Chevron(r) => r,
            Origin::Sidebar | Origin::Ipc => Region::Right,
        }
    }
}

/// A slider's and a brightness row's step, Left, Right and a wheel notch.
const STEP: f64 = 0.05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    System,
    Slider,
    Toggle,
    /// A toggle with a menu behind its arrow.
    Split,
    /// The row of things running in the background (§6).
    Background,
    /// A bar cell the chevron governs, its value as the subtitle (§7).
    Info,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::System => "system",
            Kind::Slider => "slider",
            Kind::Toggle => "toggle",
            Kind::Split => "split",
            Kind::Background => "background",
            Kind::Info => "info",
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

/// The share of its opacity an open menu takes off every other row.
const DIM: f32 = 0.4;

/// iPad's menu: the desktop edge the iPad sits past, with each row's glyph
/// and words.
const IPAD_EDGES: [(Edge, &str, &str); 4] =
    [(Edge::Left, "panel-left", "Left"), (Edge::Right, "panel-right", "Right"), (Edge::Top, "panel-top", "Above"), (Edge::Bottom, "panel-bottom", "Below")];

/// The networks Wi-Fi's menu lists at most, as GNOME's does.
const NETWORKS: usize = 8;

/// The battery pill's glyph and percent, as the bar's battery cell reads
/// them; none without a laptop battery.
fn battery(store: &Store) -> Option<(&'static str, String)> {
    let p = &store.power;
    let b = p.battery.as_ref()?;
    let threshold = charge_threshold_active(b.level, b.state, b.rate, b.time_to_full, p.on_battery);
    let icon = battery_icon(b.level, p.on_battery, threshold, crate::settings::battery_thresholds(&store.config));
    Some((icon, format!("{}%", b.level.rounded())))
}

/// Every audio device of one direction: node name and label.
fn audio_devices(store: &Store, sinks: bool) -> Vec<(&str, &str)> {
    store.audio.devices.iter().filter(|d| d.2 == sinks).map(|d| (d.0.as_str(), d.1.as_str())).collect()
}

/// Every control in reading order: the system row, the sliders, then the
/// toggles in GNOME's order, each one the store cannot back left out or
/// disabled with its reason (§3), then Background.
pub fn tiles(store: &Store) -> Vec<Tile> {
    tiles_with(store, None)
}

/// [`tiles`] with `open` the menu drawn: Background stays while its own
/// menu is open, as GNOME's does, whatever is left running.
pub fn tiles_with(store: &Store, open: Option<&str>) -> Vec<Tile> {
    let mut out = Vec::new();
    if let Some((glyph, percent)) = battery(store) {
        out.push(Tile::new("battery", Kind::System, glyph, "Battery").subtitle(Some(percent)));
    }
    out.push(Tile::new("screenshot", Kind::System, "camera", "Screenshot"));
    out.push(Tile::new("settings", Kind::System, "settings", "Settings"));
    out.push(Tile::new("lock", Kind::System, "lock", "Lock"));
    out.push(Tile { menu: true, ..Tile::new("power", Kind::System, "power", "Power off") });

    let a = &store.audio;
    if let Some(volume) = a.volume {
        let glyph = if a.muted { "volume-x" } else { "volume-2" };
        out.push(Tile::slider("volume", glyph, "Volume", volume.as_f64(), !a.muted, audio_devices(store, true).len() > 1));
    }
    if a.source {
        let glyph = if a.source_muted { "mic-off" } else { "mic" };
        out.push(Tile::slider("microphone", glyph, "Microphone", a.source_volume.map_or(0.0, Visual::as_f64), !a.source_muted, audio_devices(store, false).len() > 1));
    }
    let screens = store.display.devices();
    if let Some(first) = screens.first() {
        out.push(Tile::slider("brightness", "sun", "Brightness", first.percent as f64 / 100.0, true, screens.len() > 1));
    }

    let n = &store.network;
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

    let bt = &store.bluetooth;
    let bt_on = bt.powered == Some(true);
    let connected: Vec<&fs_devices::bluetooth::Device> = bt.devices.iter().filter(|x| x.connected).collect();
    let bt_sub = match connected.as_slice() {
        _ if !bt_on => None,
        [] => None,
        [one] => Some(one.name.clone()),
        many => Some(format!("{} connected", many.len())),
    };
    out.push(
        Tile::toggle("bluetooth", if bt_on { "bluetooth" } else { "bluetooth-off" }, "Bluetooth", bt_on)
            .split(bt.powered.is_some())
            .subtitle(bt_sub)
            .unless(bt.powered.is_none().then_some("No adapter")),
    );

    let profiles = store.profiles.as_ref();
    out.push(
        Tile::toggle("power-mode", "gauge", "Power mode", profiles.is_some_and(|p| p.active != Profile::Balanced))
            .split(profiles.is_some())
            .subtitle(profiles.map(|p| p.active.label().to_owned()))
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
    out.push(Tile::toggle("overnight", "moon-star", "Overnight", store.state.data.overnight.is_some()));

    if let Some(output) = store.compositor.outputs.iter().find(|o| display::hdr_supported(store, &o.name)) {
        out.push(Tile::toggle("hdr", "sun-medium", "HDR", display::hdr_active(store)).subtitle(Some(output.name.clone())));
    }

    let ipad = store.ipad.bridge.as_ref();
    // The tile is too narrow for the name and the state on one line, so
    // the iPad's name stands in for the title once padbridge knows it.
    let ipad_title = ipad.map(|i| i.name.as_str()).filter(|n| !n.is_empty()).unwrap_or("iPad");
    out.push(
        Tile::toggle("ipad", "tablet", ipad_title, ipad.is_some_and(|i| i.active))
            .split(ipad.is_some())
            .subtitle(ipad.map(|i| i.state_label().to_owned()))
            .unless(ipad.is_none().then_some("padbridge is not running")),
    );

    let n = background(store).len();
    if n > 0 || open == Some(BACKGROUND) {
        let title = match n {
            0 => "No background tasks".to_owned(),
            1 => "1 background task".to_owned(),
            n => format!("{n} background tasks"),
        };
        out.push(Tile { menu: true, ..Tile::new(BACKGROUND, Kind::Background, "chevron-right", &title) });
    }
    out
}

pub const BACKGROUND: &str = "background";

/// An info tile as its cell reads now.
#[derive(Clone, Debug, PartialEq)]
pub struct Extra {
    /// The entry's `bar.layout` name, the tile's stop.
    pub id: String,
    pub summary: Summary,
    /// The panel a click hands the card over to, none where the cell's own
    /// click runs instead.
    pub panel: Option<&'static str>,
}

/// Where a click on `entry`'s tile goes.
enum Tap {
    Panel(&'static str),
    Ipc(&'static [&'static str]),
    Cell,
}

fn tap(entry: &Entry) -> Tap {
    match entry.kind {
        EntryKind::Builtin(Builtin::Tray) => Tap::Panel(super::tray::ID),
        // The cell's own click focuses whichever chip it lands on.
        EntryKind::Builtin(Builtin::Workspaces) => Tap::Ipc(&["workspaces", "overview", "open"]),
        EntryKind::Builtin(b) => widget_panel(b).map_or(Tap::Cell, Tap::Panel),
        _ => Tap::Cell,
    }
}

/// The output a cell reads its slice of the compositor for: the focused
/// workspace's.
fn focused_output(store: &Store) -> String {
    store.compositor.snapshot.workspaces.iter().find(|w| w.is_focused).map(|w| w.output.clone()).unwrap_or_default()
}

/// One thing running in the background (§6), in the indicators rail's
/// order: Activity's groups, a recording, the clipboard image going to a
/// host, AirPlay, then pending reminders as one.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    /// `activity:<source>`, `recording`, `clipssh`, `airplay`, `reminders`.
    pub what: String,
    pub glyph: &'static str,
    /// An Activity group's lead ring, drawn in place of the glyph.
    pub ring: Option<Ring>,
    pub title: String,
    pub state: String,
    /// The trailing button where the thing can be stopped: its glyph and
    /// its word.
    pub stop: Option<(&'static str, &'static str)>,
}

pub fn background(store: &Store) -> Vec<Task> {
    let mut out = Vec::new();
    for g in store.activity.activity.groups() {
        let lead = g.lead();
        let running = g.state() == fs_activity::State::Running;
        out.push(Task {
            what: format!("activity:{}", g.source),
            glyph: "activity",
            ring: Some(activity::ring(lead)),
            title: lead.title.clone(),
            state: activity::detail(lead),
            stop: (!running).then_some(("x", "Dismiss")),
        });
    }
    let rec = &store.recording;
    if rec.active {
        let state = if rec.stopping { "Stopping".to_owned() } else { elapsed_label(rec.elapsed_ms as f64) };
        out.push(Task { what: "recording".into(), glyph: "circle-dot", ring: None, title: "Recording".into(), state, stop: (!rec.stopping).then_some(("square", "Stop")) });
    }
    let target = &store.clipssh.target;
    if !target.is_empty() {
        out.push(Task { what: "clipssh".into(), glyph: "terminal", ring: None, title: "Sending clipboard image".into(), state: format!("To {target}"), stop: None });
    }
    let airplay = &store.media.airplay;
    if airplay.active {
        let state = if airplay.client.is_empty() { "Receiving".to_owned() } else { format!("From {}", airplay.client) };
        out.push(Task { what: "airplay".into(), glyph: "airplay", ring: None, title: "AirPlay".into(), state, stop: None });
    }
    let pending = &store.notifications.reminders;
    if let Some(first) = pending.first() {
        let title = if pending.len() == 1 { first.message.clone() } else { format!("{} reminders", pending.len()) };
        out.push(Task {
            what: "reminders".into(),
            glyph: "alarm-clock",
            ring: None,
            title,
            state: reminders::bar_label(pending, now_ms() as f64),
            stop: Some(("x", "Clear")),
        });
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
    /// Something running in the background: Enter or a click on the row
    /// stops it where it has a `stop` button, else opens where it lives.
    Task { key: String, task: Task },
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
            Line::Row { key, .. } | Line::Slider { key, .. } | Line::Task { key, .. } => Some(key),
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
    if id == BACKGROUND {
        let mut lines: Vec<Line> = background(store).into_iter().map(|task| Line::Task { key: format!("m:{BACKGROUND}:{}", task.what), task }).collect();
        if lines.is_empty() {
            lines.push(Line::Note("Nothing is running in the background".into()));
        }
        return Some(MenuView { glyph: "activity", title: "Background", on: false, lines, settings: Some(("Activity", Hand::Panel("activity"))) });
    }
    let tile = tiles(store).into_iter().find(|t| t.id == id && t.menu)?;
    let row = Line::row;
    let a = &store.audio;
    Some(match id {
        "volume" => MenuView {
            glyph: tile.glyph,
            title: "Sound output",
            on: tile.on,
            lines: audio_devices(store, true).into_iter().map(|(name, label)| row(id, name, "speaker", label, name == a.default_sink)).collect(),
            settings: Some(("Sound settings", Hand::Panel("audio"))),
        },
        "microphone" => MenuView {
            glyph: tile.glyph,
            title: "Sound input",
            on: tile.on,
            lines: audio_devices(store, false).into_iter().map(|(name, label)| row(id, name, "mic", label, name == a.default_source)).collect(),
            settings: Some(("Sound settings", Hand::Panel("audio"))),
        },
        "brightness" => MenuView {
            glyph: tile.glyph,
            title: "Brightness",
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
            title: "Wired connections",
            on: tile.on,
            lines: store
                .network
                .wired_rows
                .iter()
                .map(|r| row(id, &r.name, "ethernet-port", r.connection.as_deref().unwrap_or(&r.name), r.connected).detail((r.connection.is_some()).then(|| r.name.clone())))
                .collect(),
            settings: Some(("Network settings", Hand::Panel("network"))),
        },
        "wifi" => {
            let n = &store.network;
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
            MenuView { glyph: tile.glyph, title: "Wi-Fi", on: tile.on, lines, settings: Some(("All networks", Hand::Panel("network"))) }
        }
        "bluetooth" => {
            let b = &store.bluetooth;
            let mut lines: Vec<Line> = Vec::new();
            if b.powered == Some(true) {
                for d in b.devices.iter().filter(|d| d.paired) {
                    let name = &d.name;
                    let doing = b.action.as_ref().filter(|(a, _)| *a == d.address).map(|(_, k)| k.text().to_owned());
                    let failed = b.failure.as_ref().filter(|(a, _)| *a == d.address).map(|(_, why)| (*why).to_owned());
                    let word = if d.connected { "Disconnect" } else { "Connect" };
                    let glyph = if d.connected { "bluetooth-connected" } else { "bluetooth" };
                    lines.push(row(id, &d.address.to_string(), glyph, name, d.connected).detail(doing.or(failed).or_else(|| Some(word.to_owned()))));
                }
                if lines.is_empty() {
                    lines.push(Line::Note("No paired devices".into()));
                }
            } else {
                lines.push(Line::Note("Turn on Bluetooth to connect to devices".into()));
            }
            MenuView { glyph: tile.glyph, title: "Bluetooth", on: tile.on, lines, settings: Some(("Bluetooth settings", Hand::Panel("bluetooth"))) }
        }
        "power-mode" => {
            let p = store.profiles.as_ref()?;
            let lines = PROFILES
                .iter()
                .filter(|(x, _)| *x != Profile::Performance || p.has_performance())
                .map(|&(x, glyph)| row(id, x.to_wire(), glyph, x.label(), p.active == x))
                .collect();
            MenuView { glyph: tile.glyph, title: "Power mode", on: tile.on, lines, settings: Some(("Power settings", Hand::Panel("power"))) }
        }
        "keyboard" => {
            let l = &store.lights;
            let lines = l.effects.iter().map(|(effect, label)| row(id, effect, "", label, *effect == l.effect)).collect();
            MenuView { glyph: tile.glyph, title: "Keyboard", on: tile.on, lines, settings: Some(("Keyboard lights", Hand::Route("lights.effect"))) }
        }
        "ipad" => {
            let i = store.ipad.bridge.as_ref()?;
            let mut lines: Vec<Line> = IPAD_EDGES.iter().map(|&(edge, glyph, title)| row(id, edge.as_str(), glyph, title, i.edge == edge)).collect();
            if i.link == Link::Down {
                lines.push(Line::Separator);
                lines.push(row(id, "reconnect", "refresh-cw", "Reconnect", false));
            }
            MenuView { glyph: tile.glyph, title: "iPad", on: tile.on, lines, settings: Some(("iPad settings", Hand::Route("ipad"))) }
        }
        "power" => {
            let can = store.logind;
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
            MenuView { glyph: "power", title: "Power off", on: false, lines, settings: None }
        }
        _ => return None,
    })
}

/// The stop a closing menu hands the cursor back to.
fn opener(id: &str) -> String {
    if id == "power" || id == BACKGROUND { id.to_owned() } else { format!("{id}:menu") }
}

/// The one in-place menu: what is asked for, what is drawn, and its three
/// clocks (§2 Motion). Opening grows the box and fades its rows in together;
/// closing fades them out and shrinks it together; the dim rides beside both.
struct MenuState {
    /// The menu asked for, none to close.
    want: Option<String>,
    /// What the clocks were last set toward.
    aimed: Option<String>,
    /// The menu drawn, open or closing.
    shown: Option<String>,
    /// The box's share of its height, `spatialFast`.
    height: Animated,
    /// The rows' opacity, `effectsFast`.
    content: Animated,
    /// The dim's share, `effects`.
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
            content: Animated::new(0.0, Clock::EffectsFast.curve()),
            dim: Animated::new(0.0, Clock::Effects.curve()),
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
    /// Its rows take the keyboard: the menu is open, however far it has grown.
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
                    self.content.set(now, 1.0, ms(Clock::EffectsFast) * (1.0 - c));
                }
                (Some(want), _) => {
                    self.shown = Some(want.clone());
                    self.height.jump(0.0);
                    self.content.jump(0.0);
                    self.height.set(now, 1.0, ms(Clock::SpatialFast));
                    self.content.set(now, 1.0, ms(Clock::EffectsFast));
                }
                (None, _) => {
                    self.content.set(now, 0.0, ms(Clock::EffectsFast) * c);
                    self.height.set(now, 0.0, ms(Clock::SpatialFast) * h);
                }
            }
            self.dim.set(now, if self.want.is_some() { 1.0 } else { 0.0 }, ms(Clock::Effects));
            self.aimed = self.want.clone();
        }
        if self.want.is_none() && !self.height.running(now) {
            self.shown = None;
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
    origin: Origin,
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
    /// The entries the card shows as info tiles (§7), as the bar that
    /// opened it resolved them, and that bar's edge, which their cells read.
    entries: Vec<Entry>,
    edge: Edge,
    /// Each info tile's entry and its cell, built as the card opens and
    /// held seen while it is open, so the cell's own wants run.
    extras: RefCell<Vec<(Entry, Box<dyn Cell>)>>,
    /// The tiles as last drawn, for `quicksettings status`.
    drawn: RefCell<Vec<Extra>>,
}

impl Default for QuickSettings {
    fn default() -> Self {
        Self {
            origin: Origin::Ipc,
            last_profile: Profile::PowerSaver,
            start: None,
            sections: RefCell::new(Vec::new()),
            menu: RefCell::new(MenuState::default()),
            scan: RefCell::new(None),
            entries: Vec::new(),
            edge: Edge::Top,
            extras: RefCell::new(Vec::new()),
            drawn: RefCell::new(Vec::new()),
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
            Line::Task { key, task } => {
                let lead = task.ring.map_or_else(|| w::panel_chip(s, task.glyph, false), w::ring);
                let detail = (!task.state.is_empty()).then(|| w::caption(task.state.clone()));
                let trailing = task.stop.map(|(glyph, tip)| w::icon_button(glyph).tip(tip).on(key.clone())).into_iter().collect();
                stop(w::panel_row_with(s, Some(lead), w::label(task.title.clone()), detail, trailing), key)
            }
        })
        .collect();
    let settings = m.settings.map(|(label, _)| {
        let row = w::panel_row(s, "", label, None, Vec::new());
        stop(row, "m:settings")
    });
    piece::menu(s, m.glyph, m.title, m.on, rows, settings, content)
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
        &[
            Topic::Bluetooth,
            Topic::Network,
            Topic::Audio,
            Topic::Power,
            Topic::Earbuds,
            Topic::Profiles,
            Topic::Logind,
            Topic::Display,
            Topic::Compositor,
            Topic::Config,
            Topic::State,
            Topic::Notifications,
            Topic::NightLight,
            Topic::Lights,
            Topic::Ipad,
            Topic::Caffeinate,
            Topic::Activity,
            Topic::Recording,
            Topic::Clipssh,
            Topic::Media,
            Topic::Weather,
            Topic::Github,
            Topic::Usage,
            Topic::SystemUpdate,
            Topic::Tailscale,
            Topic::Monitor,
            Topic::Iphone,
            Topic::Clock,
            Topic::Tray,
            Topic::Agents,
            Topic::AppIcon,
            Topic::Commands,
            Topic::PluginOutput,
        ]
    }

    fn width(&self, v: &View) -> f64 {
        v.theme.space.panel_width_wide
    }

    fn body(&self, v: &View) -> El {
        let s = &v.theme.space;
        let mut state = self.menu.borrow_mut();
        let frame = state.tick(v);
        let all = tiles_with(v.store, frame.id.as_deref());
        let system: Vec<&Tile> = all.iter().filter(|t| t.kind == Kind::System).collect();
        let sliders: Vec<&Tile> = all.iter().filter(|t| t.kind == Kind::Slider).collect();
        let toggles: Vec<&Tile> = all.iter().filter(|t| matches!(t.kind, Kind::Toggle | Kind::Split)).collect();
        let tail: Vec<&Tile> = all.iter().filter(|t| t.kind == Kind::Background).collect();
        let extras = self.extras_now(v.store, v.raster);
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

        // Each part is one row of the card, keyed by what it holds, and the
        // Tab section it belongs to.
        let mut parts: Vec<(Vec<&str>, El)> = Vec::new();
        let mut part_section: Vec<usize> = Vec::new();
        // GNOME's two spacers (`system.js` 275-292): after the battery on a
        // laptop, between Settings and Lock on a desktop.
        let laptop = system.iter().any(|t| t.id == "battery");
        let mut row = Vec::new();
        for t in &system {
            if t.id == "battery" {
                row.push(piece::battery(s, t.id, t.glyph, t.subtitle.as_deref().unwrap_or("")));
            } else {
                if (laptop && row.len() == 1) || (!laptop && t.id == "lock") {
                    row.push(w::spacer());
                }
                row.push(piece::button(s, t.id, t.glyph, &t.title));
            }
        }
        parts.push((system.iter().map(|t| t.id).collect(), w::row(s.panel_gap, row).fill().height(s.panel_row)));
        part_section.push(0);

        for t in &sliders {
            parts.push((vec![t.id], piece::slider_row(s, t.id, t.glyph, t.value.unwrap_or(0.0), t.menu)));
            part_section.push(1);
        }

        // The toggles, then the info tiles, on one grid.
        let mut cells: Vec<(&str, El)> = toggles
            .iter()
            .map(|t| {
                let subtitle = t.reason.as_deref().or(t.subtitle.as_deref());
                let el = if t.kind == Kind::Split {
                    piece::split(s, t.id, t.glyph, &t.title, subtitle, t.on, t.available())
                } else {
                    piece::toggle(s, t.id, t.glyph, &t.title, subtitle, t.on, t.available())
                };
                (t.id, el)
            })
            .collect();
        cells.extend(extras.iter().map(|x| (x.id.as_str(), piece::info(s, &x.id, &x.summary.icon, &x.summary.name, x.summary.value.as_deref()))));
        let items = vec![grid::Item { span: 1, height: s.panel_row }; cells.len()];
        let placed = grid::place(&items, self.width(v) - s.panel_row_inset * 2.0, s.panel_gap, None);
        let mut rows: Vec<(Vec<&str>, Vec<El>)> = Vec::new();
        for (((id, el), &r), rect) in cells.into_iter().zip(&placed.rows).zip(&placed.rects) {
            if rows.len() <= r {
                rows.push((Vec::new(), Vec::new()));
            }
            rows[r].0.push(id);
            rows[r].1.push(el.width(Size::Px(rect.width())));
        }
        part_section.extend(rows.iter().map(|_| 2));
        parts.extend(rows.into_iter().map(|(ids, r)| (ids, w::row(s.panel_gap, r).fill())));
        for t in &tail {
            parts.push((vec![t.id], piece::background(s, t.id, &t.title)));
            part_section.push(3);
        }
        if frame.dim > 0.0 {
            parts = parts.into_iter().map(|(ids, el)| (ids, el.fade(1.0 - DIM * frame.dim))).collect();
        }

        // The menu under its owner's row, the gap before it growing with it.
        let mut owner = None;
        if let (Some(m), Some(id)) = (&open, frame.id.as_deref())
            && let Some(at) = parts.iter().position(|(ids, _)| ids.contains(&id))
        {
            let (ids, el) = parts.remove(at);
            let grown = w::column(0.0, vec![el, w::space(s.panel_gap * frame.height), menu_el(v, m, frame.content, frame.ready).reveal(frame.height)]).fill();
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
        let mut grid_stops = usable(&toggles);
        grid_stops.extend(extras.iter().map(|x| x.id.clone()));
        let mut sections = vec![usable(&system), usable(&sliders), grid_stops, tail.iter().map(|t| t.id.to_owned()).collect()];
        if let (Some(m), true, Some(at)) = (&open, frame.ready, owner) {
            let mut keys: Vec<String> = m.lines.iter().filter_map(|l| l.key().map(str::to_owned)).collect();
            if m.settings.is_some() {
                keys.push("m:settings".into());
            }
            sections.insert(part_section[at] + 1, keys);
        }
        *self.sections.borrow_mut() = sections;

        w::column(s.panel_gap, parts.into_iter().map(|(_, el)| el).collect()).fill()
    }

    fn start(&mut self, fx: &mut Effect) {
        if let Some(p) = fx.store.profiles.as_ref().filter(|p| p.active != Profile::Balanced) {
            self.last_profile = p.active;
        }
        self.start = tiles(fx.store).into_iter().find(|t| matches!(t.kind, Kind::Toggle | Kind::Split) && t.available()).map(|t| t.id.to_owned());
        fx.service(power::ask);
        let extras = self.extras.get_mut();
        for e in &self.entries {
            for mut cell in cells::build(e, &self.entries) {
                cell.visible(true);
                extras.push((e.clone(), cell));
            }
        }
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
        self.extras.get_mut().clear();
        self.drawn.get_mut().clear();
    }

    fn wake(&self, v: &View) -> Option<std::time::Instant> {
        if self.menu.borrow().running(v) {
            return Some(v.now);
        }
        // Background's menu counts reminders down a second at a time.
        let counting = self.menu.borrow().shown.as_deref() == Some(BACKGROUND) && !v.store.notifications.reminders.is_empty();
        counting.then(|| v.now + std::time::Duration::from_secs(1))
    }

    fn follows(&self, v: &View) -> bool {
        let m = self.menu.borrow();
        m.want != m.aimed || m.height.running(v.now)
    }

    fn escape(&mut self) -> bool {
        self.menu.get_mut().close()
    }

    fn call(&mut self, verb: &str, arg: &str) -> Option<Result<Json, String>> {
        let m = self.menu.get_mut();
        match verb {
            "menu" => {
                m.open(arg);
                Some(Ok(Json::Null))
            }
            "back" => Some(if m.close() { Ok(Json::Null) } else { Err("no menu is open".into()) }),
            "state" => {
                let now = std::time::Instant::now();
                let share = |a: &Animated| (a.value(now).clamp(0.0, 1.0) * 1000.0).round() / 1000.0;
                let extras: Vec<Json> = self
                    .drawn
                    .get_mut()
                    .iter()
                    .map(|x| json!({ "id": x.id, "title": x.summary.name, "subtitle": x.summary.value, "icon": x.summary.icon, "tone": x.summary.tone.as_str(), "panel": x.panel }))
                    .collect();
                let state = json!({ "from": self.origin.label(), "menu": m.want, "shown": m.shown, "armed": m.armed, "dim": m.want.is_some(), "height": share(&m.height), "content": share(&m.content), "extras": extras });
                Some(Ok(state))
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
    /// The card `origin` opened, its info tiles `entries` off a bar on
    /// `edge`.
    pub fn opened_by(origin: Origin, entries: Vec<Entry>, edge: Edge) -> Self {
        Self { origin, entries, edge, ..Self::default() }
    }

    /// Every info tile as its cell reads now, in `bar.layout` order; a cell
    /// whose view is hidden has none.
    fn extras_now(&self, store: &Store, raster: f64) -> Vec<Extra> {
        let output = focused_output(store);
        let env = Env { store, edge: self.edge, output: &output, raster };
        let mut held = self.extras.borrow_mut();
        let out: Vec<Extra> = held
            .iter_mut()
            .filter_map(|(entry, cell)| {
                cell.read(&env);
                let panel = match tap(entry) {
                    Tap::Panel(p) => Some(p),
                    _ => None,
                };
                cell.summary().map(|summary| Extra { id: entry.name(), summary, panel })
            })
            .collect();
        *self.drawn.borrow_mut() = out.clone();
        out
    }

    /// A click or Enter on an info tile: its panel takes the card's place,
    /// or the cell's own click runs. False when `id` is no tile now.
    fn open_extra(&mut self, id: &str, fx: &mut Effect) -> bool {
        let output = focused_output(fx.store);
        let env = Env { store: fx.store, edge: self.edge, output: &output, raster: fx.store.display.scales.primary() };
        let Some((entry, cell)) = self.extras.get_mut().iter_mut().find(|(e, _)| e.name() == id) else { return false };
        cell.read(&env);
        if cell.summary().is_none() {
            return false;
        }
        match tap(entry) {
            Tap::Panel(p) => fx.panel = Some(p),
            Tap::Ipc(argv) => {
                fx.close = true;
                fx.ipc(argv);
            }
            Tap::Cell => match cell.click(Button::Left, (0.0, 0.0), &env) {
                CellAction::None => {}
                CellAction::Panel(p) => fx.panel = Some(p),
                action => fx.act = Some(action),
            },
        }
        true
    }

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
        if self.open_extra(id, fx) {
            return;
        }
        let open = self.menu.get_mut().want.clone();
        let Some(tile) = tiles_with(fx.store, open.as_deref()).into_iter().find(|t| t.id == id && t.available()) else { return };
        match tile.id {
            "battery" => fx.panel = Some("power"),
            "screenshot" => {
                fx.close = true;
                fx.ipc(&["screenshot", "pick", "smart", "default"]);
            }
            "settings" => fx.panel = Some("settings"),
            "lock" => fx.call = Some(("lock", "lock")),
            "power" | BACKGROUND => self.toggle_menu(tile.id, fx.store),
            "volume" | "microphone" => mute(tile.id, fx),
            "wired" => {
                let n = &fx.store.network;
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
            "ipad" => fx.ipc(&["ipad", "switch"]),
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
                if let Some(r) = fx.store.network.wired_rows.iter().find(|r| r.name == what) {
                    let connected = r.connected;
                    fx.service(move |ctx| net::wired_toggle(ctx, what, connected));
                }
            }
            "wifi" => wifi_row(what, fx),
            "bluetooth" => {
                let b = &fx.store.bluetooth;
                if b.action.is_none()
                    && let Some(address) = BtAddress::parse(&what)
                    && let Some(d) = b.devices.iter().find(|d| d.address == address && d.paired)
                {
                    bt::ask(BtAsk::Run(address, if d.connected { BtAction::Disconnect } else { BtAction::Connect }));
                }
            }
            "power-mode" => {
                let Some(p) = Profile::from_wire(&what) else { return };
                if p != Profile::Balanced {
                    self.last_profile = p;
                }
                fx.service(move |ctx| devices::power::set_profile(ctx, p));
            }
            "keyboard" => fx.ipc(&["lights", "effect", &what]),
            "ipad" if what == "reconnect" => fx.ipc(&["ipad", "reconnect"]),
            "ipad" => fx.ipc(&["ipad", "position", &what]),
            "power" => self.power(&what, fx),
            BACKGROUND => background_row(&what, fx),
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
        let a = &store.audio;
        match key {
            "volume" => {
                let v = Visual::from_f64(at(a.volume.map_or(0.0, Visual::as_f64)));
                fx.service(move |ctx| audio::set_volume(ctx, v));
            }
            "microphone" => {
                let v = Visual::from_f64(at(a.source_volume.map_or(0.0, Visual::as_f64)));
                fx.service(move |ctx| audio::set_source_volume(ctx, v));
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
        let Some(p) = fx.store.profiles.as_ref() else { return };
        let next = if p.active == Profile::Balanced {
            if self.last_profile == Profile::Performance && !p.has_performance() { Profile::PowerSaver } else { self.last_profile }
        } else {
            self.last_profile = p.active;
            Profile::Balanced
        };
        fx.service(move |ctx| devices::power::set_profile(ctx, next));
    }
}

/// Enter on a network in Wi-Fi's menu: a saved or open one connects, a
/// secured one nobody knows (or whose password just failed) hands the card
/// over to the network panel with its passphrase step open, and the
/// connected one stays as it is.
fn wifi_row(ssid: String, fx: &mut Effect) {
    let n = &fx.store.network;
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

/// Enter or a click on one of Background's rows: what can be stopped
/// stops (a finished Activity group is dismissed), a running group hands
/// the card over to Activity and AirPlay to the media panel.
fn background_row(what: &str, fx: &mut Effect) {
    let Some(task) = background(fx.store).into_iter().find(|t| t.what == what) else { return };
    match (task.what.as_str(), task.stop) {
        ("recording", Some(_)) => fx.ipc(&["record", "stop"]),
        ("reminders", Some(_)) => fx.ipc(&["reminder", "clear"]),
        ("airplay", _) => fx.panel = Some("media"),
        (what, stop) => {
            if let Some(source) = what.strip_prefix("activity:") {
                if stop.is_some() {
                    fx.ipc(&["activity", "dismiss", source]);
                } else {
                    fx.panel = Some("activity");
                }
            }
        }
    }
}

/// A slider's icon (or Enter on its track): mute its device.
fn mute(id: &str, fx: &mut Effect) {
    match id {
        "volume" => fx.service(audio::toggle_mute),
        "microphone" => fx.service(audio::toggle_source_mute),
        _ => {}
    }
}

/// The open menu for `quicksettings status`: its id, its lines and the
/// armed row, off the module's own answer to `state`.
pub fn status_menu(store: &Store, state: &Json) -> (Json, bool) {
    let id = state["menu"].as_str();
    let armed = state["armed"].as_str();
    let open = id.and_then(|id| menu(store, id, armed));
    let lines = open.as_ref().map(|m| {
        m.lines
            .iter()
            .filter_map(|l| match l {
                Line::Row { key, title, detail, checked, armed, .. } => Some(json!({ "id": key, "title": title, "detail": detail, "checked": checked, "armed": armed })),
                Line::Slider { key, title, value } => Some(json!({ "id": key, "title": title, "value": value })),
                Line::Task { key, task } => Some(json!({ "id": key, "title": task.title, "detail": task.state, "stop": task.stop.map(|(_, word)| word) })),
                Line::Note(_) | Line::Separator => None,
            })
            .chain(m.settings.map(|(label, hand)| match hand {
                Hand::Panel(p) => json!({ "id": "m:settings", "title": label, "panel": p }),
                Hand::Route(r) => json!({ "id": "m:settings", "title": label, "route": r }),
            }))
            .collect::<Vec<_>>()
    });
    let notes: Option<Vec<&str>> = open.as_ref().map(|m| m.lines.iter().filter_map(|l| if let Line::Note(t) = l { Some(t.as_str()) } else { None }).collect());
    let header = open.as_ref().map(|m| json!({ "title": m.title, "on": m.on }));
    (
        json!({ "id": id, "shown": state["shown"], "height": state["height"], "content": state["content"], "header": header, "rows": lines, "notes": notes }),
        state["dim"].as_bool().unwrap_or(false),
    )
}
