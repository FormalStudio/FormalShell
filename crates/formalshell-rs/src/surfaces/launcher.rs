//! The launcher. One keyboard-exclusive overlay window over the
//! output, a card budding off the top line (a `Card`) to
//! rest at 30% of the output's height, three bands split by full-bleed
//! rules: the header (the mode token and the search field), the body (the app
//! grid, the row list or the emoji grid, sections and the empty state) and
//! the footer (where you are, what Enter does). The scrim behind it is two
//! single-pixel surfaces the App owns.
//!
//! The model ([`Model`]) lives as long as the shell: the level and the
//! select request outlive a close. The window
//! ([`Shown`]) exists from an open until its exit has run.
//!
//! Rows come off the store's warm index (`services::menu`); a keystroke
//! re-ranks a few hundred nodes and builds only the rows inside the
//! viewport.

use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_menu::actions::{Action, IpcCall, Run, SubjectId, TargetKind};
use fs_menu::model::{self, Mode, SectionCtx};
use fs_menu::nav::{self, Dir};
use fs_menu::intent::Intent;
use fs_menu::node::{ActionEntry, Kind as NodeKind, Node, Provider};
use fs_menu::toggles::{self, Snapshot};
use crate::ui::confirm::Armed;
use fs_types::{Edge, WindowId};
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId};
use crate::scroll::{Touchpad, Travel};
use crate::services::menu::{self as index, Ask};
use crate::services::{clipboard, compositor, plugins, state};
use crate::store::Store;
use crate::paint::{Kit, Painter};
use crate::surfaces::modal::Modal;
use crate::ui::field;
use crate::ui::{self, El, Ink, Size, Type, Ui, Weight, w};

pub mod keys;
use keys::{KeyAction, KeyCtx, KeyMode};

/// Which view draws the level (`menu status`'s `view`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Rows,
    Emoji,
    AppGrid,
    Picker,
    /// The metric tiles over the process table.
    Monitor,
    /// One camera's feed.
    Mirror,
    /// A panel the launcher holds, centred: Settings and its pages, and
    /// Information, Pond's way.
    Panel,
}

impl View {
    fn name(self) -> &'static str {
        match self {
            View::Rows => "rows",
            View::Emoji => "emoji",
            View::AppGrid => "appGrid",
            View::Picker => "picker",
            View::Monitor => "monitor",
            View::Mirror => "mirror",
            View::Panel => "panel",
        }
    }
}

/// One drawn row or cell's place in the body, in body coordinates.
#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    /// The heading band above a row, part of its height.
    band: f64,
}

/// What the body lays out, recomputed on every resolve.
#[derive(Default)]
struct Layout {
    slots: Vec<Slot>,
    /// The headings: text, the band's top and its x, in body coordinates,
    /// and the words at the band's far end.
    headings: Vec<(String, f64, f64, String)>,
    /// A section's empty state under its heading: its top, the line and
    /// the caption under it.
    notes: Vec<(f64, String, String)>,
    content_h: f64,
    columns: usize,
    cell_w: f64,
    cell_h: f64,
    /// Laid out at half the card: the selection moving onto or off a
    /// subject with a preview lays it out again.
    halved: bool,
    /// The body width it was laid out in, which an output narrower than
    /// the card changes.
    body_w: f64,
    /// The list's inset under its last row, kept clear of rows while it
    /// scrolls so the body always ends on a whole one.
    foot: f64,
    /// A held page's row ends (`ui::row_ends`), so a page taller than the
    /// card rests on a whole row.
    row_ends: Vec<f64>,
    /// A held page's room under its body past the frame's own
    /// (`panel::Frame::closing`).
    close: f64,
}

/// What input moves: the rows key, the field, the cursor, an armed row, and
/// the actions column's cursor and target step.
#[derive(PartialEq)]
struct ViewId {
    key: String,
    query: String,
    cursor: String,
    confirm: Option<Confirm>,
    column: Option<(usize, bool)>,
}

/// A screen-reading call parked behind a `wl_display.sync`. Each hold gets
/// an id, and only the callback or deadline carrying the current id gets
/// the call back: a late answer, or one from a session since reopened,
/// finds nothing.
#[derive(Default)]
pub struct Grab {
    next: u64,
    held: Option<(u64, IpcCall)>,
}

impl Grab {
    pub fn hold(&mut self, call: IpcCall) -> u64 {
        self.next += 1;
        self.held = Some((self.next, call));
        self.next
    }

    /// The call, when `id` is still the one held.
    pub fn take(&mut self, id: u64) -> Option<IpcCall> {
        match self.held {
            Some((held, _)) if held == id => self.held.take().map(|(_, call)| call),
            _ => None,
        }
    }

    pub fn cancel(&mut self) {
        self.held = None;
    }
}

pub struct Model {
    resolved: Vec<(String, Option<String>, bool)>,
    pub open: bool,
    pub mode: Mode,
    pub level: Option<String>,
    pub query: field::Field,
    select_prompt: String,
    select_options: Vec<String>,
    select_token: String,
    // What `resolve` committed.
    pub rows: Vec<Node>,
    sections: Vec<String>,
    pub view: View,
    pub app_count: usize,
    empty: Option<Node>,
    searching: bool,
    key: String,
    // The cursor (M72 T1).
    pub cursor: usize,
    pub cursor_id: String,
    placed: bool,
    want: String,
    pub from_keys: bool,
    travels: bool,
    /// The last scroll came off a wheel notch: it glides there without
    /// overshooting either end.
    wheeled: bool,
    confirm: Armed<Confirm>,
    pub scroll: f64,
    /// A touchpad's pull past either end, drawn over `scroll`, and the
    /// coast after a flick, which moves `scroll` frame by frame.
    pub touch: Touchpad,
    layout: Layout,
    body_h: f64,
    /// The rows changed since the window last drew.
    pub dirty: bool,
    /// A `pasteAfter` row ran: the chord goes out once the window is gone.
    pub paste: bool,
    /// A call that reads the screen, held until the window is gone so the
    /// closing card is not in the picture.
    pub after_unmap: Option<IpcCall>,
    /// The screen-reading call whose compositor sync is in flight.
    pub grab: Grab,
    /// Counts closes, so an answer asked for in one session can tell it
    /// arrived in another.
    pub sessions: u64,
    pub picker: Picker,
    /// The picker's sections while it is the level.
    picker_sections: Vec<PickerSection>,
    /// The output's width, which the card is cut to.
    pub output_w: f64,
    /// The monitor route's tile strip height, 0 off the route.
    monitor_strip_h: f64,
    /// The monitor route keeps both polls running while it is the level.
    monitor_wants: Option<(crate::services::wants::Want, crate::services::wants::Want)>,
    /// The pid an Enter armed TERM on; the next Enter on it sends it.
    monitor_armed: Armed<u64>,
    /// The camera the mirror shows, by node path.
    pub mirror_current: String,
    /// The monitor tiles' history lines, started when the route opens.
    monitor_history: MonitorHistory,
    /// `debug motionScale`, which the scroll's glide and coast run at.
    pub motion_scale: f64,
    /// What the query's abbreviations say, for the ranking and `abbrev`.
    lookup: fs_menu::abbrev::Lookup,
    /// The query as an abbreviation, when it is one.
    abbrev_key: Option<String>,
    /// The actions column beside the list, while it is open.
    column: Option<Column>,
    /// The subjects whose own children the list holds (an app's windows, a
    /// folder's entries), stepped into over `level`, outermost first.
    children: Vec<Children>,
    /// The captured frame of the window the preview pane shows.
    pub pane_frame: Option<crate::scene::Bitmap>,
    /// `Edit abbreviation`'s subject and the abbreviation it edits, while
    /// its Input step is up.
    editing: Option<(String, String)>,
    /// The launcher plugin and words last sent as a `query` event.
    plugin_query: Option<(String, String)>,
    /// The panel the settings and info routes hold.
    pub panel: Option<Embedded>,
}

/// A panel module drawn as the launcher's body, with the panel host's own
/// cursor model.
pub struct Embedded {
    pub module: Box<dyn crate::surfaces::panel::Panel>,
    pub cursor: crate::surfaces::panel::host::Cursor,
    pub id: PanelId,
    /// The module's wants, taken as it starts and held while the launcher
    /// holds it.
    pub wants: crate::services::wants::Holding,
    /// `Panel::start` has run.
    pub started: bool,
    /// The module's width, as the window last read it.
    pub width: f64,
}

impl Embedded {
    fn new(id: PanelId) -> Option<Self> {
        let mut module = crate::surfaces::panel::build(id, crate::surfaces::panel::Args::default())?;
        module.opened();
        let cursor = crate::surfaces::panel::host::Cursor::starting(module.cursor_start());
        Some(Self { module, cursor, id, wants: Default::default(), started: false, width: 0.0 })
    }
}

impl Drop for Embedded {
    fn drop(&mut self) {
        self.module.closed();
    }
}

/// The token an `Edit abbreviation` answer comes back under.
pub const ABBREV_EDIT: &str = "abbrev-edit";

/// What a subject stepped into with → lists.
#[derive(Clone, Debug)]
enum Into {
    /// An app's windows, by the app's subject id.
    Windows(String),
    /// A folder's entries, by its path.
    Folder(String),
    /// A Wi-Fi network's details, by its SSID.
    Network(String),
}

/// A subject stepped into with →, which the token names.
#[derive(Clone, Debug)]
struct Children {
    into: Into,
    label: String,
}

impl Children {
    fn key(&self) -> &str {
        match &self.into {
            Into::Windows(id) | Into::Folder(id) | Into::Network(id) => id,
        }
    }
}

/// The selected subject's actions beside the list (launcher spec §5). The
/// field filters it; the list's own query waits in `list_query`.
struct Column {
    node: Node,
    actions: Vec<Action>,
    list_query: String,
    /// An action asking for a target, and the targets, which the column
    /// lists instead of the actions.
    target: Option<(Action, Vec<Node>)>,
    /// What the filter leaves, as positions in the step's list.
    shown: Vec<usize>,
    cursor: usize,
    opened: Instant,
    /// Its own scroll, the list's model: the offset, a touchpad's stretch
    /// and coast over it, and how the last move asks to be drawn.
    scroll: f64,
    touch: Touchpad,
    wheeled: bool,
    travels: bool,
    /// A row's pitch and the rows' viewport, as the window last laid it out.
    row_h: f64,
    view_h: f64,
    /// The launcher plugins sent an `actions` event for this subject that
    /// have not answered, and when it went.
    asked: Vec<String>,
    asked_at: Instant,
    /// Where the first plugin answer joined the actions and when, for its
    /// fade.
    joined: Option<(usize, Instant)>,
}

/// How long a plugin has to answer `actions` before the column opens
/// without it.
const PLUGIN_ANSWER: std::time::Duration = std::time::Duration::from_millis(300);

impl Column {
    fn labels(&self) -> Vec<&str> {
        match &self.target {
            Some((_, targets)) => targets.iter().map(|t| t.label.as_str()).collect(),
            None => self.actions.iter().map(|a| a.label.as_str()).collect(),
        }
    }

    fn ids(&self) -> Vec<&str> {
        self.shown
            .iter()
            .filter_map(|&i| match &self.target {
                Some((_, targets)) => targets.get(i).map(|t| t.id.as_str()),
                None => self.actions.get(i).map(|a| a.id.as_str()),
            })
            .collect()
    }

    fn refilter(&mut self, query: &str) {
        self.shown = fs_menu::actions::filter(&self.labels(), query);
        self.cursor = 0;
        self.scroll = 0.0;
        self.touch.settle();
    }

    fn max_scroll(&self) -> f64 {
        (self.shown.len() as f64 * self.row_h - self.view_h).max(0.0)
    }

    /// The last whole row in view, when the list runs past it and the
    /// cursor is elsewhere: that row reads `n more` instead, `n` counting it
    /// and everything below.
    fn more(&self, scroll: f64) -> Option<(usize, usize)> {
        if self.row_h <= 0.0 {
            return None;
        }
        let last = ((scroll + self.view_h) / self.row_h).floor() as usize;
        let last = last.checked_sub(1)?;
        (last + 1 < self.shown.len() && last != self.cursor).then(|| (last, self.shown.len() - last))
    }

    /// The smallest scroll that shows the cursor's row.
    fn follow(&mut self) {
        let next = crate::scroll::follow(self.cursor as f64 * self.row_h, self.row_h, self.scroll, self.view_h, self.shown.len() as f64 * self.row_h);
        self.scroll = next;
        self.travels = true;
        self.wheeled = false;
        self.touch.settle();
    }
}

/// One axis frame over a scroll area at `offset`, `max` from its end: the
/// offset it lands on and whether it glides there, None for a frame that
/// asks nothing. A wheel's notches step `step` each and stop on either
/// end; a touchpad's travel lands at once and stretches past an end until
/// the fingers lift, springing back over `release_ms` or coasting on.
#[allow(clippy::too_many_arguments)]
fn travel_to(touch: &mut Touchpad, travel: Travel, offset: f64, max: f64, step: f64, view: f64, release_ms: f64, slow: f64) -> Option<(f64, bool)> {
    let now = Instant::now();
    Some(match travel {
        Travel::Notches(n) => {
            touch.settle();
            ((offset + n * step).clamp(0.0, max), true)
        }
        Travel::Pixels { px, finger: true, time } => (touch.drag(now, time, offset, max, px, view), false),
        Travel::Pixels { px, finger: false, .. } => {
            touch.settle();
            ((offset + px).clamp(0.0, max), false)
        }
        Travel::Lift(time) => {
            touch.release(now, time, offset, max, release_ms, slow);
            (offset, false)
        }
        Travel::None => return None,
    })
}

/// The drawn offset headed for `target`: a wheel's notch glides there on
/// `effectsFast`, a key's follow travels on `spatialFast`, a touchpad's
/// travel and a coast's frames land at once.
fn glide(a: &mut Animated, now: Instant, target: f64, wheeled: bool, travels: bool, theme: &Theme, scale: f64) {
    let gap = (a.target() - target).abs();
    if wheeled && gap > 0.5 {
        a.set_on(now, target, Clock::EffectsFast.ms(theme) * scale, Clock::EffectsFast.curve());
    } else if travels && gap > 0.5 {
        a.set_on(now, target, Clock::SpatialFast.ms(theme) * scale, Clock::SpatialFast.curve());
    } else if !wheeled && !travels && gap > 0.0 {
        // A coast's tail moves under half a pixel a frame.
        a.jump(target);
    }
}

/// The rows whose labels are shaped at both weights on a resolve: more
/// than a card at `LAUNCHER.row_height` shows.
const ROWS_SHAPED_AHEAD: usize = 24;

/// One reading per monitor tick: CPU, memory, GPU, and the network's
/// receive and send rates. A reading nobody could take is left out.
#[derive(Default)]
struct MonitorHistory {
    sampled: Option<u64>,
    cpu: std::collections::VecDeque<f64>,
    mem: std::collections::VecDeque<f64>,
    gpu: std::collections::VecDeque<f64>,
    rx: std::collections::VecDeque<f64>,
    tx: std::collections::VecDeque<f64>,
}

const HISTORY: usize = 60;

fn push(q: &mut std::collections::VecDeque<f64>, v: Option<f64>) {
    if let Some(v) = v.filter(|v| v.is_finite()) {
        if q.len() == HISTORY {
            q.pop_front();
        }
        q.push_back(v);
    }
}

/// What the card's size travels on when it changes (`Shown::layout`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct SizeKey {
    level: Option<String>,
    halved: bool,
    app_view: bool,
}

pub const MONITOR_ROUTE: &str = "monitor";
pub const MIRROR_ROUTE: &str = "mirror";

/// The wallpaper route lists `dir`, and in
/// select mode answers `token` in picker-selection.txt instead of setting
/// the wallpaper.
#[derive(Clone, Debug, Default)]
pub struct Picker {
    pub select: bool,
    pub dir: String,
    pub token: String,
    /// `picker select` set this up for the open that follows.
    pending: bool,
    /// The settings page it was summoned from, which leaving it returns to.
    pub from: Option<PanelId>,
}

/// One of the picker's sections: its id, heading, and the words at the
/// heading's far end.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PickerSection {
    pub id: &'static str,
    pub title: String,
    pub far: String,
}

pub const STYLE: &str = "style";
pub const YOURS: &str = "yours";
pub const INCLUDED: &str = "included";
/// A select request's one flat section.
const FILES: &str = "files";

pub const PICKER_ROUTE: &str = "wallpaper";

pub fn picker_selection_path() -> std::path::PathBuf {
    state::state_path().with_file_name("picker-selection.txt")
}

impl Default for Model {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::Menu,
            level: None,
            query: field::Field::default(),
            select_prompt: String::new(),
            select_options: Vec::new(),
            select_token: String::new(),
            resolved: Vec::new(),
            rows: Vec::new(),
            sections: Vec::new(),
            view: View::AppGrid,
            app_count: 0,
            empty: None,
            searching: false,
            key: String::new(),
            cursor: 0,
            cursor_id: String::new(),
            paste: false,
            after_unmap: None,
            grab: Grab::default(),
            sessions: 0,
            picker: Picker::default(),
            picker_sections: Vec::new(),
            output_w: 0.0,
            monitor_strip_h: 0.0,
            monitor_wants: None,
            monitor_armed: Armed::default(),
            mirror_current: String::new(),
            panel: None,
            monitor_history: MonitorHistory::default(),
            motion_scale: 1.0,
            lookup: Default::default(),
            abbrev_key: None,
            column: None,
            children: Vec::new(),
            pane_frame: None,
            editing: None,
            plugin_query: None,
            placed: false,
            want: String::new(),
            from_keys: true,
            travels: false,
            wheeled: false,
            confirm: Armed::default(),
            scroll: 0.0,
            touch: Touchpad::new(),
            layout: Layout::default(),
            body_h: 0.0,
            dirty: true,
        }
    }
}

/// What an activation asks of the shell outside the launcher.
pub enum Out {
    None,
    Close,
    /// An `@ipc:` call the App answers in process.
    Call(IpcCall),
    /// A verb of the launcher's the App answers.
    Intent(Intent),
}

/// The row action a first press armed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Confirm {
    node: String,
    action: String,
}

impl Confirm {
    fn of(node: &Node, action: &Action) -> Confirm {
        Confirm { node: node.id.clone(), action: action.id.clone() }
    }
}

pub fn snapshot(store: &Store) -> Snapshot {
    let radio = &store.media.radio;
    let l = &store.lights;
    let ipad = store.ipad.bridge.as_ref();
    Snapshot {
        nightlight: store.nightlight.active,
        overnight: store.state.data.overnight.is_some(),
        caffeinate: store.caffeinate.active,
        dnd: store.notifications.model.dnd,
        dark: store.state.data.mode == "dark",
        lights_on: l.brightness > 0,
        hdr: crate::services::display::hdr_active(store),
        ipad_active: ipad.is_some_and(|i| i.active),
        lights_effect: l.effect.clone(),
        lights_source: l.source.clone(),
        lights_colour: if l.source == "custom" { l.custom_colour.clone() } else { String::new() },
        lights_speed: l.speed.clone(),
        lights_brightness: l.brightness.to_string(),
        wifi_ssid: store.network.ssid.as_ref().map_or_else(String::new, |(s, _)| s.clone()),
        audio_sink: store.audio.default_sink.clone(),
        audio_source: store.audio.default_source.clone(),
        radio_station: radio.station.as_ref().filter(|_| radio.running()).map_or_else(String::new, |s| s.uuid.clone()),
        ipad_edge: ipad.map_or_else(String::new, |i| i.edge.as_str().to_owned()),
        ipad_stretch: ipad
            .and_then(|i| fs_menu::providers::IPAD_STRETCHES.iter().find(|(_, _, a, b)| i.stretch.near(*a, *b)))
            .map_or_else(String::new, |(id, ..)| (*id).to_owned()),
        bluetooth_connected: store.bluetooth.devices.iter().filter(|x| x.connected).map(|x| x.address.to_string()).collect(),
    }
}

fn radio_station(s: &fs_media::radio::stations::Station) -> fs_menu::providers::RadioStation {
    fs_menu::providers::RadioStation { uuid: s.uuid.clone(), name: s.name.clone(), country: s.country.clone(), codec: s.codec.clone() }
}

/// RadioSearchProvider.rowsFor: the answer for `q` once it is in, a
/// searching row until then. Asking is idempotent; the service debounces.
pub fn radio_search_rows(store: &Store, q: &str) -> Vec<Node> {
    use fs_menu::providers as p;
    let q = q.trim();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    crate::services::radio::send(crate::services::radio::Cmd::Search(q.to_owned()));
    let radio = &store.media.radio;
    match &radio.search {
        Some((answered, found)) if answered == q => match found {
            None => vec![p::radio_failed_row()],
            Some(v) if v.is_empty() => vec![p::radio_no_results_row()],
            Some(v) => {
                let favs: std::collections::HashSet<String> = radio.favorites.iter().map(|s| s.uuid.clone()).collect();
                let list: Vec<_> = v.iter().take(50).map(radio_station).collect();
                p::radio_result_rows(&list, &favs)
            }
        },
        _ => vec![p::radio_searching_row()],
    }
}

fn wifi_network(r: &crate::services::devices::network::Row) -> fs_menu::providers::WifiNetwork {
    fs_menu::providers::WifiNetwork {
        name: r.ssid.clone(),
        known: r.known,
        connected: r.connected,
        secured: r.secured,
        enterprise: r.enterprise,
        in_range: r.in_range,
        signal: r.signal.as_fraction(),
        security: r.security.clone(),
        addresses: r.addresses.clone(),
    }
}

/// A network stepped into: its details off the live scan, a note once it
/// is no longer listed.
fn network_rows(store: &Store, ssid: &str, q: &str) -> Vec<Node> {
    let Some(row) = store.network.row(ssid) else { return vec![Node::note("wifi.details.gone", "No longer listed")] };
    let q = q.trim().to_lowercase();
    fs_menu::providers::wifi_details(&wifi_network(row)).into_iter().filter(|n| q.is_empty() || n.label.to_lowercase().contains(&q)).collect()
}

/// The four device routes' rows, off the store.
pub fn device_sources(store: &Store) -> Vec<(Provider, Vec<Node>)> {
    use fs_menu::providers as p;
    let n = &store.network;
    let wifi = p::WifiState {
        has_device: n.wifi_device.is_some(),
        enabled: n.wifi_enabled,
        networks: n.rows.iter().map(wifi_network).collect(),
        action_ssid: n.action.as_ref().map(|(_, s)| s.clone()).unwrap_or_default(),
        action_kind: n.action.as_ref().map(|(k, _)| k.as_str().to_owned()).unwrap_or_default(),
        failure_ssid: n.failure.as_ref().map(|f| f.ssid.clone()).unwrap_or_default(),
        failure_text: n.failure.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
    };
    let b = &store.bluetooth;
    let bluetooth = p::BluetoothState { available: b.powered.is_some(), enabled: b.powered == Some(true), devices: b.devices.clone() };
    let audio: Vec<p::AudioDevice> =
        store.audio.devices.iter().map(|(name, label, sink)| p::AudioDevice { name: name.clone(), label: label.clone(), is_sink: *sink }).collect();
    let r = &store.media.radio;
    let radio = p::RadioState { running: r.running(), favorites: r.favorites.iter().map(radio_station).collect() };
    vec![
        (Provider::Wifi, p::wifi_rows(&wifi)),
        (Provider::Bluetooth, p::bluetooth_rows(&bluetooth)),
        (Provider::Audio, p::audio_rows(&audio)),
        (Provider::Radio, p::radio_rows(&radio)),
    ]
}

/// Every launcher plugin with a mode, as `(route id, name)`.
pub fn plugin_modes(store: &Store) -> Vec<(String, String)> {
    fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
        .filter_map(|p| Some((fs_chrome::plugins::surface_key(p), p.mode.clone()?)))
        .collect()
}

/// Each plugin mode's rows off what its plugin last printed, or the one dim
/// row that says why there are none.
pub fn plugin_sources(store: &Store) -> Vec<(Provider, Vec<Node>)> {
    use crate::services::plugins::Run as Printed;
    let state = &store.plugins;
    fs_chrome::plugins::launcher_plugins(&state.resolved.plugins)
        .filter(|p| p.mode.is_some())
        .map(|p| {
            let route = fs_chrome::plugins::surface_key(p);
            // Only the mode's own view shows why it has no rows.
            let note = |text: String| vec![Node { local_only: true, ..Node::note(format!("{route}.note"), text) }];
            let rows = match state.runs.get(&p.id) {
                None => note(format!("Loading {}", p.name)),
                Some(Printed::Failed(why)) => note(format!("{} failed: {why}", p.name)),
                Some(Printed::Live(d)) => {
                    let rows = plugin_rows(&p.id, &route, &d.rows);
                    if rows.is_empty() { note("No results".into()) } else { rows }
                }
            };
            (Provider::Plugin(route), rows)
        })
        .collect()
}

/// The verb that writes `line` to plugin `id`'s stdin.
fn plugin_send(id: &str, line: &str) -> Intent {
    Intent::PluginSend(id.to_owned(), line.to_owned())
}

/// A plugin mode's rows: each activates its row, then its own actions; a
/// `label` row heads the rows under it.
fn plugin_rows(id: &str, route: &str, rows: &[plugins::Row]) -> Vec<Node> {
    use plugins::RowKind;
    let mut section = None;
    let mut out = Vec::new();
    for r in rows {
        if r.kind == RowKind::Label {
            section = Some(r.text.clone()).filter(|t| !t.is_empty());
            continue;
        }
        let toggle = r.kind == RowKind::Toggle;
        let activate = plugins::activate_event(&r.id, toggle.then_some(!r.checked));
        let mut n = Node::new(format!("{route}.{}", r.id), r.text.clone(), NodeKind::Action).with_action(plugin_send(id, &activate));
        n.icon = r.icon.clone();
        n.desc = if toggle { Some(if r.checked { "On" } else { "Off" }.to_owned()) } else { Some(r.detail.clone()).filter(|d| !d.is_empty()) };
        n.verb = toggle.then(|| "Toggle".to_owned());
        n.keep_open = toggle.then_some(true);
        n.section = section.clone();
        n.actions = r
            .actions
            .iter()
            .map(|a| {
                let label = if a.label.is_empty() { &a.id } else { &a.label };
                let run = plugin_send(id, &plugins::action_event(&r.id, &a.id));
                ActionEntry { icon: Some(a.icon.clone()).filter(|i| !i.is_empty()), ..ActionEntry::new(&a.id, label, run) }
            })
            .collect();
        out.push(n);
    }
    out
}

impl Model {
    fn view_id(&self) -> ViewId {
        ViewId {
            key: self.key.clone(),
            query: self.query.text().to_owned(),
            cursor: self.cursor_id.clone(),
            confirm: self.confirm.get().cloned(),
            column: self.column.as_ref().map(|c| (c.cursor, c.target.is_some())),
        }
    }

    fn key_mode(&self) -> KeyMode {
        match self.mode {
            Mode::Menu => KeyMode::Menu,
            Mode::Select => KeyMode::Select,
            Mode::Input => KeyMode::Input,
        }
    }

    /// The clipboard routes, whose card keeps the split height.
    fn split(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some("clipboard" | "share.history")) && self.children.is_empty()
    }

    /// The selected subject, when it has a preview (launcher spec §7).
    /// The actions column takes the side column over it.
    pub fn pane(&self) -> Option<&Node> {
        if self.mode != Mode::Menu || self.column.is_some() || self.view != View::Rows {
            return None;
        }
        self.rows.get(self.cursor).filter(|n| pane_kind(n).is_some())
    }

    /// The list takes the left half of the card: the selected subject's
    /// preview or the actions column holds the right.
    pub fn halved(&self) -> bool {
        self.column.is_some() || self.pane().is_some()
    }

    fn size_key(&self) -> SizeKey {
        SizeKey { level: self.level.clone(), halved: self.halved(), app_view: self.app_view() }
    }

    /// The query the list is ranked by: the field's, or while the column
    /// filters through the field, the one it held before.
    fn list_text(&self) -> &str {
        self.column.as_ref().map_or(self.query.text(), |c| c.list_query.as_str())
    }

    /// One monitor tick onto the tiles' history lines.
    pub fn monitor_sample(&mut self, store: &Store) {
        let m = &store.monitor;
        if m.sampled_ms.is_none() || m.sampled_ms == self.monitor_history.sampled {
            return;
        }
        let h = &mut self.monitor_history;
        h.sampled = m.sampled_ms;
        push(&mut h.cpu, m.cpu.aggregate);
        push(&mut h.mem, m.mem.as_ref().map(|x| x.used_fraction));
        push(&mut h.gpu, m.gpu_busy());
        push(&mut h.rx, m.net_available.then(|| m.net.iter().map(|n| n.rx_bytes_per_sec).sum()));
        push(&mut h.tx, m.net_available.then(|| m.net.iter().map(|n| n.tx_bytes_per_sec).sum()));
        self.dirty = true;
    }

    /// A held panel stands alone: no foot under it.
    pub fn footless(&self) -> bool {
        self.mode == Mode::Menu && self.view == View::Panel
    }

    /// A card whose height follows its list rather than the level's kind.
    /// A preview pane counts: the card takes the taller of the list and
    /// what the pane has to say. The clipboard keeps Pond's one height and
    /// the actions column the level's.
    fn content_tall(&self) -> bool {
        self.mode != Mode::Input && !self.app_view() && !self.split() && self.column.is_none() && matches!(self.view, View::Rows | View::AppGrid)
    }

    /// The routes that draw a whole view of their own.
    pub fn app_view(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some(MONITOR_ROUTE | MIRROR_ROUTE | PICKER_ROUTE))
    }

    /// One card width per level kind, which the content is laid out at
    /// while the card itself morphs into it, cut to the output less
    /// `screenPadding` either side.
    pub fn card_width(&self, theme: &Theme) -> f64 {
        let kind = self.kind_width(theme);
        if self.output_w > 0.0 { kind.min(self.output_w - theme.space.screen_padding * 2.0) } else { kind }
    }

    fn kind_width(&self, theme: &Theme) -> f64 {
        if let Some(p) = self.panel.as_ref().filter(|p| self.on_panel() && p.width > 0.0) {
            return p.width;
        }
        if self.app_view() { theme.space.popup_width_menu_app } else { launcher_px(fs_theme::tokens::LAUNCHER.width) }
    }

    /// The list's share of a halved body, the side column taking the rest
    /// (Pond's clipboard, 348 of 638).
    fn list_share(&self, _theme: &Theme) -> f64 {
        0.55
    }

    /// The width inside the card's padding.
    pub fn content_width(&self, theme: &Theme) -> f64 {
        self.card_width(theme) - theme.space.panel_padding * 2.0
    }

    /// A row list runs the card's whole width, its rows an `sm` inset off
    /// the edge; the other views keep the card's padding.
    pub fn bleeds(&self) -> bool {
        matches!(self.view, View::Rows | View::AppGrid)
    }

    fn body_width(&self, theme: &Theme) -> f64 {
        if self.bleeds() { self.card_width(theme) } else { self.content_width(theme) }
    }

    /// A held panel's body: `panelRowInset` in from the card's sides, its
    /// rows' text then on `panelInset` as in the panel host.
    fn panel_body(&self, theme: &Theme) -> (f64, f64) {
        let i = theme.space.panel_row_inset;
        (i, self.card_width(theme) - i * 2.0)
    }

    /// How far in the field and its rules sit: a held panel's text column
    /// (its body plus the kit's text inset), so its title and the rule
    /// above its groups and tiles line up with them.
    fn field_inset(&self, theme: &Theme) -> f64 {
        if self.on_panel() { self.panel_body(theme).0 + w::text_inset(&theme.space) } else { theme.space.xxl }
    }

    /// The preview pane's offset into the body and its size: the side
    /// column past the list's share, `lg` in from its divider and the
    /// body's edges. The pane's draw and its picture's decode both read it.
    pub fn pane_extent(&self, theme: &Theme) -> (f64, f64, f64) {
        let lg = theme.space.lg;
        let body_w = self.body_width(theme);
        let half = (body_w * self.list_share(theme)).round();
        (half + lg, (body_w - half - lg * 2.0).max(0.0), (self.body_h - lg * 2.0).max(0.0))
    }

    /// The card's height for the level kind, capped by a share of the
    /// output's.
    fn kind_height(&self, theme: &Theme, output_h: f64) -> f64 {
        let s = &theme.space;
        let (kind, share) = if self.app_view() || self.on_panel() {
            (s.popup_height_menu_app, fs_theme::tokens::LAUNCHER.app_height_share)
        } else if self.split() {
            (s.popup_height_menu_split, fs_theme::tokens::LAUNCHER.height_share)
        } else {
            (s.popup_height_menu, fs_theme::tokens::LAUNCHER.height_share)
        };
        if output_h > 0.0 { kind.min(output_h * share) } else { kind }
    }

    /// The level holds a panel (Settings, Information) as its body.
    pub fn on_panel(&self) -> bool {
        self.mode == Mode::Menu && self.children.is_empty() && self.panel.is_some()
    }

    /// `id`'s panel in the launcher: its route as the level, then the
    /// page itself in the module's place.
    pub fn show_panel(&mut self, store: &Store, id: PanelId) -> bool {
        let Some(route) = id.route() else { return false };
        if self.mode != Mode::Menu || self.level.as_deref() != Some(route.as_str()) {
            self.abandon_select();
            self.children.clear();
            self.enter_level(store, Some(route.as_str().to_owned()));
        }
        if self.panel.as_ref().is_none_or(|p| p.id != id) {
            self.panel = Embedded::new(id);
        }
        self.dirty = true;
        true
    }

    pub fn on_mirror(&self) -> bool {
        self.mode == Mode::Menu && self.level.as_deref() == Some(MIRROR_ROUTE)
    }

    /// MirrorService.cycle: false when there is nothing to step to.
    pub fn mirror_cycle(&mut self, store: &Store, delta: i64) -> bool {
        let next = fs_system::camera::step(&store.mirror.cameras, &self.mirror_current, delta);
        if next == self.mirror_current {
            return false;
        }
        self.mirror_current = next;
        self.dirty = true;
        true
    }

    /// The feed box: the body's width, a 4:3 share of its height.
    pub fn mirror_box(&self, theme: &Theme) -> (f64, f64) {
        let s = &theme.space;
        let w = self.content_width(theme) - s.sm * 2.0;
        (w.round(), (w * 3.0 / 4.0).min(self.body_h - s.md * 2.0).max(1.0).round())
    }

    pub fn on_picker(&self) -> bool {
        self.mode == Mode::Menu && self.level.as_deref() == Some(PICKER_ROUTE)
    }

    /// The picker's tiles by section: the directory's pairs and singles
    /// under Your wallpapers, then the built-ins and the Sky under Included.
    /// A select request lists the directory's files alone, one flat section.
    pub fn picker_tiles(&self, store: &Store) -> Vec<(&'static str, fs_menu::providers::Item)> {
        use fs_menu::providers::{Item, ItemKind, Mode as Filed, image_basename};
        let p = &store.picker;
        let scanned = p.dir == self.picker.dir;
        if self.picker.select {
            let files = if scanned { p.scanned.as_slice() } else { &[] };
            return files
                .iter()
                .map(|f| (FILES, Item { id: f.clone(), kind: ItemKind::Single { path: f.clone(), filed: None }, title: image_basename(f).to_owned(), credit: None }))
                .collect();
        }
        let yours = if scanned { p.items.as_slice() } else { &[] };
        let sky = Item { id: "sky".into(), kind: ItemKind::Sky, title: "Sky".into(), credit: None };
        let style = [(Filed::Light, "light", "Light"), (Filed::Dark, "dark", "Dark")].map(|(mode, name, title)| {
            let path = store.desktop_for(name);
            (STYLE, Item { id: format!("mode-{name}"), kind: ItemKind::Mode { mode, path }, title: title.into(), credit: None })
        });
        style
            .into_iter()
            .chain(yours.iter().map(|i| (YOURS, i.clone())))
            .chain(p.included.iter().chain([&sky]).map(|i| (INCLUDED, i.clone())))
            .collect()
    }

    /// The sections the picker shows, in order.
    fn picker_sections(&self, store: &Store) -> Vec<PickerSection> {
        if self.picker.select {
            return vec![PickerSection { id: FILES, ..Default::default() }];
        }
        let home = crate::services::menu::home();
        let dir = match self.picker.dir.strip_prefix(home.trim_end_matches('/')) {
            Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
            _ => self.picker.dir.clone(),
        };
        let mut licences: Vec<&str> = store.picker.included.iter().filter_map(|i| i.credit.as_ref()).map(|c| c.license.as_str()).collect();
        licences.dedup();
        let licence = match licences.as_slice() {
            [one] => licence_name(one),
            _ => String::new(),
        };
        vec![
            PickerSection { id: STYLE, title: "Style".into(), far: picker_style_note(store) },
            PickerSection { id: YOURS, title: "Your wallpapers".into(), far: dir },
            PickerSection { id: INCLUDED, title: "Included".into(), far: licence },
        ]
    }

    fn picker_rows(&self, store: &Store, query: &str) -> Vec<Node> {
        let dark = store.state.data.mode == "dark";
        let sky = crate::services::wallpaper::sky_source(store.sky_step());
        let tiles = self.picker_tiles(store);
        let mut rows = Vec::new();
        for section in [FILES, STYLE, YOURS, INCLUDED] {
            let items: Vec<fs_menu::providers::Item> = tiles.iter().filter(|(s, _)| *s == section).map(|(_, i)| i.clone()).collect();
            rows.extend(fs_menu::providers::tile_rows(&items, section, dark, &sky, query));
        }
        rows
    }

    /// The tile a row stands for.
    fn picker_tile_of(&self, store: &Store, id: &str) -> Option<fs_menu::providers::Item> {
        let id = id.strip_prefix("wallpaper.")?;
        self.picker_tiles(store).into_iter().map(|(_, i)| i).find(|i| i.id == id)
    }

    /// The tile holds the slot the mode shows: a pair both slots, a single
    /// the picture shown, the Sky while it is up.
    pub fn picker_current(store: &Store, item: &fs_menu::providers::Item) -> bool {
        use fs_menu::providers::ItemKind;
        match &item.kind {
            ItemKind::Pair { light, dark } => store.desktop_for("light") == *light && store.desktop_for("dark") == *dark,
            ItemKind::Single { path, .. } => store.desktop() == *path,
            ItemKind::Sky => crate::services::wallpaper::sky(&store.desktop()).is_some(),
            ItemKind::Mode { .. } => false,
        }
    }

    /// A Style preview stands for the mode in effect.
    pub fn picker_mode_on(store: &Store, item: &fs_menu::providers::Item) -> bool {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        match item.kind {
            ItemKind::Mode { mode, .. } => (mode == Filed::Dark) == (store.state.data.mode == "dark"),
            _ => false,
        }
    }

    /// Enter or a click on a Style preview: `theme mode` under its own rules
    /// (a snooze under `auto`, nothing where settings.json pins the mode,
    /// which the section's far end says), the card left open on the
    /// previews.
    fn picker_style(&mut self, store: &Store, mode: fs_menu::providers::Mode) -> Out {
        let name = if mode == fs_menu::providers::Mode::Dark { "dark" } else { "light" };
        let key = crate::services::theme::mode_key(store.config.settings());
        if let Ok(mode) = crate::services::theme::request_mode(name, &store.state.data.mode, key)
            && mode != store.state.data.mode
        {
            state::set(vec![state::Field::Mode(mode)]);
        }
        self.placed = true;
        self.dirty = true;
        Out::None
    }

    /// A tile's picture size: `pickerTile` wide at the least, as many
    /// columns as fit the card's content an `lg` apart, 16:9. The cursor's
    /// ring sits `xs` outside the picture, inside the content.
    pub fn picker_tile_px(&self, theme: &Theme) -> (f64, f64) {
        let s = &theme.space;
        let width = self.content_width(theme) - s.xs * 2.0;
        let columns = fs_menu::appgrid::columns_for(width + s.lg, s.picker_tile + s.lg).max(1);
        let w = ((width - s.lg * (columns - 1) as f64) / columns as f64).floor().max(1.0);
        (w, (w * 9.0 / 16.0).round())
    }

    /// `openImageSelect`.
    pub fn open_image_select(&mut self, store: &Store, dir: &str, token: &str) {
        self.picker_abandon();
        let dir = crate::services::picker::directory(if dir.is_empty() { crate::settings::PICKER_DIRECTORY.get(&store.config) } else { dir });
        self.picker = Picker { select: true, dir, token: token.into(), pending: true, from: None };
        self.open(store, Some(PICKER_ROUTE));
    }

    /// `chooseImage`: false off the route or for a path no tile holds. A
    /// pair sets both slots, a picture for both modes both, one filed under
    /// a mode its slot and that mode (unless settings.json pins the mode),
    /// the Sky the sky over both.
    pub fn choose_image(&mut self, store: &Store, path: &str) -> bool {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        if !self.open || !self.on_picker() {
            return false;
        }
        let sky = path == fs_theme::theme::SKY || crate::services::wallpaper::sky(path).is_some();
        let tiles = self.picker_tiles(store);
        let Some((_, item)) = tiles.iter().find(|(_, i)| if sky { i.kind == ItemKind::Sky } else { i.files().contains(&path) }) else {
            return false;
        };
        if self.picker.select {
            write_picker_selection(&json!({"token": self.picker.token, "value": path}));
            self.picker.token.clear();
        } else {
            match &item.kind {
                ItemKind::Mode { .. } => return false,
                ItemKind::Sky => state::set_wallpaper(state::Choice::Both(fs_theme::theme::SKY.into()), None),
                ItemKind::Pair { light, dark } => state::set_wallpaper(state::Choice::Pair(light.clone(), dark.clone()), None),
                ItemKind::Single { path, filed: None } => state::set_wallpaper(state::Choice::Both(path.clone()), None),
                ItemKind::Single { path, filed: Some(filed) } => {
                    let (half, name) = if *filed == Filed::Light { (state::Half::Light, "light") } else { (state::Half::Dark, "dark") };
                    let key = crate::services::theme::mode_key(store.config.settings());
                    let mode = crate::services::theme::request_mode(name, &store.state.data.mode, key).ok();
                    state::set_wallpaper(state::Choice::Slot(half, path.clone()), mode.as_deref());
                }
            }
        }
        self.close();
        true
    }

    /// `picker chooseFor`: one single for one mode's slot, the mode left as
    /// it is. False off the route, in select mode, or for anything but a
    /// listed single.
    pub fn choose_for(&mut self, store: &Store, path: &str, half: state::Half) -> bool {
        use fs_menu::providers::ItemKind;
        if !self.open || !self.on_picker() || self.picker.select {
            return false;
        }
        let single = self.picker_tiles(store).iter().any(|(_, i)| matches!(&i.kind, ItemKind::Single { path: p, .. } if p == path));
        if !single {
            return false;
        }
        state::set_wallpaper(state::Choice::Slot(half, path.into()), None);
        self.close();
        true
    }

    /// `pickerStatus`.
    pub fn picker_status(&self, store: &Store) -> Value {
        use fs_menu::providers::{ItemKind, Mode as Filed};
        let tiles = self.picker_tiles(store);
        let sections = self.picker_sections(store);
        let filed = |m: &Option<Filed>| m.map(|m| if m == Filed::Light { "light" } else { "dark" });
        let items: Vec<Value> = tiles
            .iter()
            .map(|(section, i)| {
                let credit = i.credit.clone().unwrap_or_default();
                let (kind, light, dark, filing) = match &i.kind {
                    ItemKind::Pair { light, dark } => ("pair", Some(light.as_str()), Some(dark.as_str()), None),
                    ItemKind::Single { path, filed: f } => {
                        let f = filed(f);
                        ("single", (f != Some("dark")).then_some(path.as_str()), (f != Some("light")).then_some(path.as_str()), f)
                    }
                    ItemKind::Sky => ("sky", None, None, None),
                    ItemKind::Mode { mode: Filed::Light, path } => ("mode", Some(path.as_str()), None, None),
                    ItemKind::Mode { mode: Filed::Dark, path } => ("mode", None, Some(path.as_str()), None),
                };
                json!({
                    "id": i.id,
                    "section": section,
                    "kind": kind,
                    "title": i.title,
                    "author": (!credit.author.is_empty()).then_some(credit.author),
                    "license": (!credit.license.is_empty()).then_some(credit.license),
                    "source": (!credit.source.is_empty()).then_some(credit.source),
                    "light": light,
                    "dark": dark,
                    "filed": filing,
                    "current": !self.picker.select && (Self::picker_current(store, i) || Self::picker_mode_on(store, i)),
                })
            })
            .collect();
        let count = |id: &str| tiles.iter().filter(|(s, _)| *s == id).count();
        json!({
            "open": self.open && self.on_picker(),
            "mode": if self.picker.select { "select" } else { "wallpaper" },
            "directory": self.picker.dir,
            "included": (!store.picker.included_dir.is_empty()).then_some(&store.picker.included_dir),
            "count": tiles.len(),
            "cursor": self.cursor,
            "cursorId": self.cursor_id,
            "cachedThumbnails": tiles.iter().filter(|(_, i)| store.picker.warm(i)).count(),
            "current": {"light": store.desktop_for("light"), "dark": store.desktop_for("dark"), "shown": store.desktop()},
            "sections": sections.iter().map(|s| json!({"id": s.id, "title": s.title, "far": s.far, "count": count(s.id)})).collect::<Vec<_>>(),
            "items": items,
        })
    }

    fn picker_enter(&mut self, store: &Store) {
        if !self.picker.pending {
            self.picker_abandon();
            self.picker.select = false;
            self.picker.dir = crate::services::picker::directory(crate::settings::PICKER_DIRECTORY.get(&store.config));
            self.picker.token.clear();
            self.picker.from = None;
        }
        self.picker.pending = false;
        crate::services::picker::command(crate::services::picker::Cmd::Scan(self.picker.dir.clone()));
    }

    fn picker_abandon(&mut self) {
        if self.picker.select && !self.picker.token.is_empty() {
            write_picker_selection(&json!({"token": self.picker.token, "cancelled": true}));
            self.picker.token.clear();
        }
    }

    pub fn placeholder(&self, store: &Store) -> String {
        if self.mode != Mode::Menu {
            return self.select_prompt.clone();
        }
        if let Some(c) = &self.column {
            return if c.target.is_some() { "Search targets".into() } else { "Search actions".into() };
        }
        if !self.children.is_empty() {
            return "Search".into();
        }
        match self.level.as_deref().and_then(|l| store.menu.nodes().get(l)) {
            Some(node) => model::prompt_for(Some(node)),
            None => "Search apps, files, commands".into(),
        }
    }

    pub fn section_names(&self) -> Vec<String> {
        model::section_names(&self.sections)
    }

    pub fn columns(&self) -> usize {
        match self.view {
            View::Rows => 1,
            _ => self.layout.columns.max(1),
        }
    }

    /// The launcher plugin whose mode the list holds.
    fn plugin_mode(&self) -> Option<&str> {
        let route = self.level.as_deref().filter(|_| self.mode == Mode::Menu && self.children.is_empty())?;
        route.strip_prefix(fs_chrome::plugins::PLUGIN_PREFIX)
    }

    /// The mode token's labels, outermost first; none at the root or
    /// outside Menu mode.
    pub fn token(&self, store: &Store) -> Vec<String> {
        if self.mode != Mode::Menu {
            return Vec::new();
        }
        let mut path = model::mode_path(store.menu.nodes(), self.level.as_deref());
        path.extend(self.children.iter().map(|c| c.label.clone()));
        path
    }

    /// The column's tokens after the mode's: `Actions`, then the target
    /// an action asks for.
    pub fn column_tokens(&self) -> Vec<&'static str> {
        let Some(c) = &self.column else { return Vec::new() };
        let mut out = vec!["Actions"];
        if let Some((Action { run: Run::Target(kind), .. }, _)) = &c.target {
            out.push(kind.label());
        }
        out
    }

    /// The level and its rows, resolved over the warm index.
    fn resolve_rows(&self, store: &Store) -> (View, Vec<Node>, usize, Option<Node>, bool, String) {
        let menu = self.mode == Mode::Menu;
        let q = self.list_text();
        let level = self.level.as_deref();
        let nodes = store.menu.nodes();
        let cond = &store.menu.cond;
        let emoji = menu && level == Some("emoji");
        let children = self.children.last().filter(|_| menu);
        let plugin = self.plugin_mode().is_some();
        let route_rows = children.is_some() || plugin || matches!(level, Some("keybinds" | "calc" | "radio.search" | "install" | "clipboard" | "share.history"));
        let grid_wanted = crate::settings::APP_GRID.get(&store.config);
        let picker = menu && children.is_none() && level == Some(PICKER_ROUTE);
        let monitor = menu && children.is_none() && level == Some(MONITOR_ROUTE);
        let mirror = menu && children.is_none() && level == Some(MIRROR_ROUTE);
        let embedded = menu && children.is_none() && self.panel.is_some();
        let emoji = emoji && children.is_none() && !picker && !monitor && !mirror;
        let view = if embedded {
            View::Panel
        } else if mirror {
            View::Mirror
        } else if monitor {
            View::Monitor
        } else if picker {
            View::Picker
        } else if emoji {
            View::Emoji
        } else if menu && grid_wanted && !route_rows {
            View::AppGrid
        } else {
            View::Rows
        };
        let mut rows: Vec<Node> = match self.mode {
            Mode::Select => {
                let ql = q.to_lowercase();
                self.select_options
                    .iter()
                    .enumerate()
                    .map(|(i, o)| Node::new(format!("select.{i}"), o.clone(), NodeKind::Option))
                    .filter(|n| ql.is_empty() || n.label.to_lowercase().contains(&ql))
                    .collect()
            }
            Mode::Input => Vec::new(),
            Mode::Menu if children.is_some() => children.map_or_else(Vec::new, |c| match &c.into {
                Into::Windows(id) => window_rows(store, id, q),
                Into::Folder(path) => folder_rows(store, path, q),
                Into::Network(ssid) => network_rows(store, ssid, q),
            }),
            Mode::Menu if picker => self.picker_rows(store, q),
            Mode::Menu if monitor => monitor_rows(store, q),
            Mode::Menu if mirror || embedded => Vec::new(),
            Mode::Menu if matches!(level, Some("clipboard" | "share.history")) => {
                let history: Vec<Node> = model::visible_children(nodes, level, cond).into_iter().cloned().collect();
                if history.is_empty() {
                    vec![fs_menu::providers::clipboard_empty_row()]
                } else {
                    let matched: Vec<Node> = fs_menu::providers::clipboard_search(&history, q).into_iter().cloned().collect();
                    if matched.is_empty() { vec![fs_menu::providers::clipboard_no_match_row()] } else { matched }
                }
            }
            Mode::Menu if emoji => {
                let paste = crate::settings::CLIPBOARD_PASTE.get(&store.config);
                store.menu.emoji.as_ref().map_or_else(Vec::new, |e| e.rows(q, paste, &store.state.data.emoji_uses, None))
            }
            Mode::Menu if level == Some("keybinds") => {
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                fs_menu::keybinds::rows_for(reply, q)
            }
            Mode::Menu if level == Some("radio.search") => radio_search_rows(store, q),
            Mode::Menu if level == Some(fs_menu::providers::INSTALL_ROUTE) => crate::surfaces::packages::install_rows(store, q),
            // The plugin ranks its own rows against the query it was sent.
            Mode::Menu if plugin => model::visible_children(nodes, level, cond).into_iter().cloned().collect(),
            Mode::Menu if q.is_empty() && level.is_none() => {
                let outside = |id: &str| outside_subject(store, id);
                let rest = model::root_rest(nodes, &store.state.data.recent_pinned, &store.state.data.picks, now_ms(), model::RECENT_MAX, cond, outside);
                let recent = rest.first().is_some_and(|r| r.section.as_deref() == Some(model::RECENT_SECTION));
                if view == View::AppGrid && recent {
                    rest.into_iter().filter(|r| r.kind != NodeKind::App).collect()
                } else {
                    rest
                }
            }
            Mode::Menu if q.is_empty() => {
                let gated = level.and_then(|l| nodes.get(l)).is_some_and(|n| !model::is_when_visible(n, cond));
                if gated {
                    vec![model::gated_note_row(&nodes[level.unwrap_or_default()])]
                } else {
                    model::visible_children(nodes, level, cond).into_iter().cloned().collect()
                }
            }
            Mode::Menu if level == Some("calc") => fs_menu::calc::result_node(q).into_iter().collect(),
            Mode::Menu => {
                let calc = level.is_none().then(|| fs_menu::calc::result_node(q)).flatten();
                let boost = fs_menu::search::Boost { abbrev: Some(&self.lookup), picks: &store.state.data.picks, now: now_ms() };
                let ranked = fs_menu::search::rank_with(nodes, q, cond, level, boost).into_iter().cloned();
                calc.into_iter().chain(ranked).collect()
            }
        };
        let mut empty = None;
        let live: Vec<Node> = rows.iter().filter(|r| !(r.kind == NodeKind::Note && r.dim == Some(true))).cloned().collect();
        if live.len() < rows.len() {
            if live.is_empty() {
                empty = rows.first().cloned();
            }
            rows = live;
        }
        if view == View::AppGrid && level.is_none() && q.is_empty() {
            let apps: Vec<Node> = model::visible_children(nodes, Some("apps"), cond)
                .into_iter()
                .filter(|r| r.kind == NodeKind::App)
                .take(model::RECENT_MAX)
                .cloned()
                .collect();
            rows = apps.into_iter().chain(rows).collect();
        }
        let mut app_count = 0;
        if view == View::AppGrid {
            let parts = fs_menu::appgrid::partition(rows);
            rows = parts.rows;
            app_count = parts.app_count;
        }
        let searching = menu && !q.is_empty() && !emoji && !picker && !monitor && !route_rows;
        let key = format!(
            "{:?}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
            self.mode,
            level.unwrap_or(""),
            children.map_or("", Children::key),
            q,
            if picker && self.picker.select { "select" } else { "" }
        );
        (view, rows, app_count, empty, searching, key)
    }

    /// The query's abbreviations, read once per resolve.
    fn lookup(&self, store: &Store) -> fs_menu::abbrev::Lookup {
        if self.mode != Mode::Menu {
            return Default::default();
        }
        let book = index::abbreviation_book(store);
        fs_menu::abbrev::lookup(&configured_abbreviations(store), &book, self.list_text(), now_ms(), |id| resolves(store, id))
    }

    /// Commits the rows, their headings and the cursor.
    pub fn resolve(&mut self, store: &Store, theme: &Theme, kit: &mut Kit) {
        // Only while open: the plugin runs only then, and a query cached
        // while it is down would never be sent once it is back.
        let asked = self.plugin_mode().filter(|_| self.open).map(|id| (id.to_owned(), self.query.text().to_owned()));
        if let Some((id, text)) = asked.as_ref().filter(|a| self.plugin_query.as_ref() != Some(a)) {
            plugins::send(id, plugins::query_event(text));
        }
        self.plugin_query = asked;
        self.refresh_peers(store);
        self.lookup = self.lookup(store);
        self.abbrev_key = if self.mode == Mode::Menu { fs_menu::abbrev::key(self.list_text()) } else { None };
        let (view, rows, app_count, empty, searching, key) = self.resolve_rows(store);
        let fresh = key != self.key;
        let level_label = self.level.as_deref().and_then(|l| store.menu.nodes().get(l)).map(|n| n.label.clone()).unwrap_or_default();
        let sections = if view == View::Picker {
            rows.iter().map(|r| r.section.clone().unwrap_or_default()).collect()
        } else if !self.children.is_empty() {
            vec![String::new(); rows.len()]
        } else {
            model::sections_for(
                &rows,
                &SectionCtx {
                    mode: Some(self.mode),
                    grid: view == View::Emoji,
                    cells: app_count,
                    searching,
                    level: self.level.as_deref(),
                    level_label: &level_label,
                },
            )
        };
        let view_changed = view != self.view;
        let changed = rows.len() != self.rows.len() || rows.iter().zip(&self.rows).any(|(a, b)| a.id != b.id);
        self.rows = rows;
        self.sections = sections;
        self.view = view;
        self.app_count = app_count;
        self.empty = empty;
        self.searching = searching;
        if changed {
            self.touch.settle();
        }
        if fresh {
            self.placed = false;
            self.want.clear();
        }
        let ids: Vec<&str> = self.rows.iter().map(|r| r.id.as_str()).collect();
        let mut index = nav::rederive(&self.want, self.cursor as i64, &ids, fresh, self.placed);
        if view == View::Picker && !self.placed && !self.picker.select {
            // The cursor opens on the tile on the desktop, else on the first
            // past the Style previews.
            let tiles = self.picker_tiles(store);
            let current = tiles.iter().find(|(_, i)| Self::picker_current(store, i)).map(|(_, i)| format!("wallpaper.{}", i.id));
            let first = self.sections.iter().position(|s| s != STYLE);
            index = current.and_then(|id| ids.iter().position(|r| *r == id)).or(first).unwrap_or(0);
        }
        self.key = key;
        self.monitor_strip_h = if view == View::Monitor { ui::measure(&monitor_strip(store, theme, &self.monitor_history), self.content_width(theme), theme, kit).1 } else { 0.0 };
        self.picker_sections = if view == View::Picker { self.picker_sections(store) } else { Vec::new() };
        self.layout(theme, kit);
        if changed && matches!(view, View::Rows | View::AppGrid) {
            // The selection sets its row's label at 600: both weights are
            // shaped here, so a step never waits on the shaper.
            for r in self.rows.iter().take(ROWS_SHAPED_AHEAD) {
                for weight in [Weight::Normal, Weight::Semibold] {
                    ui::measure(&row_words(r.label.clone()).weight(weight), f64::MAX, theme, kit);
                }
            }
        }
        // A key's step resolves the level again before it draws: the cursor
        // landing where the step put it keeps the step's travel.
        let travels = self.travels && !fresh && !view_changed && index == self.cursor;
        self.place(index, travels);
        self.relayout(theme, kit);
        if fresh || view_changed {
            self.scroll = 0.0;
            self.touch.settle();
            self.follow();
        } else if changed && view == View::Picker {
            // A rescan that shrank the grid leaves the scroll past its end.
            self.follow();
        }
        self.dirty = true;
    }

    /// A `Send to…` list open in the column follows LocalSend's scan.
    fn refresh_peers(&mut self, store: &Store) {
        let q = self.query.text().to_owned();
        let Some(c) = &mut self.column else { return };
        let Some((action, targets)) = &mut c.target else { return };
        if action.run != Run::Target(TargetKind::Peer) {
            return;
        }
        let fresh = peer_targets(store);
        if fresh.iter().map(|n| (&n.id, &n.label)).ne(targets.iter().map(|n| (&n.id, &n.label))) {
            *targets = fresh;
            c.refilter(&q);
        }
    }

    /// The list or the column is still scrolling, so the body draws again.
    pub fn rows_moving(&self, now: Instant) -> bool {
        self.touch.running(now) || self.column.as_ref().is_some_and(|c| c.touch.running(now))
    }

    fn place(&mut self, index: usize, travels: bool) {
        let valid = index < self.rows.len();
        self.travels = travels;
        self.wheeled = false;
        self.cursor = if valid { index } else { 0 };
        self.cursor_id = if valid { self.rows[index].id.clone() } else { String::new() };
    }

    /// The body's slots: the app grid's cells and tail, the row list's
    /// rows, the emoji grid's tiles.
    fn layout(&mut self, theme: &Theme, kit: &mut Kit) {
        let s = &theme.space;
        let body_w = self.body_width(theme);
        let halved = self.halved();
        let width = if halved { (body_w * self.list_share(theme)).round() } else { body_w };
        // Pond's list sits a clear `lg` in from the card and under the
        // field's rule; the grids keep their own gutters.
        let inset = if self.view == View::Rows { s.lg } else if self.bleeds() { s.sm } else { s.md };
        let gutter = s.sm;
        let heading_x = if self.bleeds() { s.sm + s.lg - s.control_padding_x } else { 0.0 };
        let heading_h = ui::measure(&heading_el(s, "Ag"), width, theme, kit).1;
        let mut out = Layout::default();
        let mut y = inset;
        let band_of = |section: &str, first: bool| -> f64 {
            if section.is_empty() { 0.0 } else { (if first { 0.0 } else { s.section_gap }) + heading_h + s.row_gap }
        };
        let heading = |i: usize, secs: &[String]| -> String {
            let band = secs.get(i).cloned().unwrap_or_default();
            let prev = if i > 0 { secs.get(i - 1).cloned().unwrap_or_default() } else { String::new() };
            if band == prev { String::new() } else { band }
        };
        match self.view {
            View::Picker => {
                y = self.picker_layout(&mut out, theme, kit, width, heading_h, y);
            }
            View::AppGrid | View::Emoji => {
                let (cells, min_cell) = if self.view == View::AppGrid {
                    (self.app_count, s.control_height * 4.0)
                } else {
                    (self.rows.len(), 0.0)
                };
                let grid_x = if self.view == View::AppGrid { gutter } else { 0.0 };
                let width = width - grid_x * 2.0;
                let columns = if self.view == View::Emoji {
                    model::EMOJI_COLUMNS
                } else {
                    fs_menu::appgrid::columns_for(width, min_cell)
                };
                let cell_w = width / columns.max(1) as f64;
                let name_h = ui::measure(&w::label("Ag"), width, theme, kit).1;
                let cell_h = if self.view == View::AppGrid {
                    s.control_height * 2.0 + s.row_gap + name_h + s.control_padding_y * 2.0 + gutter * 2.0
                } else {
                    cell_w
                };
                let heading_text = if self.view == View::AppGrid && cells > 0 { self.sections.first().cloned().unwrap_or_default() } else { String::new() };
                if !heading_text.is_empty() {
                    out.headings.push((heading_text, y, gutter, String::new()));
                    y += heading_h + s.row_gap;
                }
                for i in 0..cells {
                    let (r, c) = (i / columns.max(1), i % columns.max(1));
                    out.slots.push(Slot { x: grid_x + c as f64 * cell_w, y: y + r as f64 * cell_h, w: cell_w, h: cell_h, band: 0.0 });
                }
                y += cells.div_ceil(columns.max(1)) as f64 * cell_h;
                out.columns = columns;
                out.cell_w = cell_w;
                out.cell_h = cell_h;
                let width = width + grid_x * 2.0;
                for i in cells..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x, String::new()));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: gutter, y, w: width - gutter * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
            View::Mirror | View::Panel => out.columns = 1,
            View::Rows | View::Monitor => {
                out.columns = 1;
                let side = if self.view == View::Rows { s.lg } else { s.sm };
                let heading_x = heading_x + side - s.sm;
                if self.monitor_strip_h > 0.0 {
                    y += self.monitor_strip_h + s.section_gap;
                }
                for i in 0..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x, String::new()));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: side, y, w: width - side * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
        }
        out.content_h = y + inset;
        out.foot = if self.view == View::Rows { inset } else { 0.0 };
        out.halved = halved;
        out.body_w = body_w;
        self.layout = out;
    }

    /// The picker's sections down the body: each heading, then its tiles
    /// `lg` apart in as many columns as `width` holds, a section's first row
    /// carrying its heading as their band. Your wallpapers with nothing in
    /// it says so under its heading. Returns the content's foot.
    fn picker_layout(&self, out: &mut Layout, theme: &Theme, kit: &mut Kit, width: f64, heading_h: f64, top: f64) -> f64 {
        let s = &theme.space;
        let (gap, ring) = (s.lg, s.xs);
        let (tile_w, pic_h) = self.picker_tile_px(theme);
        let caption_h = ui::measure(&w::caption("Ag"), width, theme, kit).1;
        // A slot is the picture and its caption with the ring's margin
        // round them; the pictures stand `lg` apart either way.
        let (slot_w, slot_h) = (tile_w + ring * 2.0, pic_h + s.row_gap + caption_h + ring * 2.0);
        let pitch = slot_h - ring * 2.0 + gap;
        let columns = (((width - ring * 2.0 + gap) / (tile_w + gap)).floor() as usize).max(1);
        let mut y = top;
        let mut first = true;
        for sec in &self.picker_sections {
            let tiles: Vec<usize> = (0..self.rows.len()).filter(|&i| self.sections.get(i).map(String::as_str) == Some(sec.id)).collect();
            let note = tiles.is_empty() && sec.id == YOURS && self.query.is_empty();
            if tiles.is_empty() && !note {
                continue;
            }
            if !first {
                y += s.section_gap;
            }
            first = false;
            let band = if sec.title.is_empty() { 0.0 } else { heading_h + s.row_gap };
            if band > 0.0 {
                out.headings.push((sec.title.clone(), y, ring - s.control_padding_x, sec.far.clone()));
            }
            if note {
                let line_h = ui::measure(&w::text("Ag"), width, theme, kit).1;
                out.notes.push((y + band, format!("No pictures in {}", sec.far), "Set picker.directory in settings.json".into()));
                y += band + line_h + s.row_gap + caption_h;
                continue;
            }
            // GNOME centres its two Style previews over the page
            // (`cc-background-panel.blp`).
            let indent = if sec.id == STYLE && tiles.len() < columns { (columns - tiles.len()) as f64 * (tile_w + gap) / 2.0 } else { 0.0 };
            for (k, _) in tiles.iter().enumerate() {
                let (r, c) = (k / columns, k % columns);
                let x = indent + c as f64 * (tile_w + gap);
                out.slots.push(match r {
                    0 => Slot { x, y, w: slot_w, h: band + slot_h, band },
                    _ => Slot { x, y: y + band + r as f64 * pitch, w: slot_w, h: slot_h, band: 0.0 },
                });
            }
            y += band + tiles.len().div_ceil(columns) as f64 * pitch - gap + ring * 2.0;
        }
        out.columns = columns;
        out.cell_w = tile_w;
        out.cell_h = pitch;
        y
    }

    /// The body laid out again where the selection has moved onto or off a
    /// subject with a preview, which halves the list or gives it back, or
    /// the output under it has changed the card's width.
    fn relayout(&mut self, theme: &Theme, kit: &mut Kit) {
        if self.layout.halved != self.halved() || self.layout.body_w != self.body_width(theme) {
            self.layout(theme, kit);
            self.follow();
        }
    }

    /// The tallest body under `cap` that ends on a whole row and the list's
    /// `inset` under it, so a long list never rests on half a row.
    fn whole_rows(&self, cap: f64, inset: f64) -> f64 {
        let bottom = self.layout.slots.iter().map(|s| s.y + s.h).filter(|b| b + inset <= cap).fold(0.0, f64::max);
        if bottom > 0.0 { bottom + inset } else { cap }
    }

    /// The smallest scroll that shows the cursor's slot.
    fn follow(&mut self) {
        let Some(slot) = self.layout.slots.get(self.cursor) else {
            self.scroll = 0.0;
            return;
        };
        // A row heading its section brings the heading with it, and the
        // first row the list's own inset.
        let (top, bottom) = (slot.y, slot.y + slot.h);
        let view = self.body_h - self.layout.foot;
        let mut next = self.scroll;
        let first = self.cursor == 0 || (!matches!(self.view, View::Rows | View::Monitor) && self.cursor < self.cells() && self.cursor < self.layout.columns);
        let top = if first { 0.0 } else { top };
        let h = bottom - top;
        if top < next {
            next = top;
        } else if top + h > next + view {
            next = top + h - view;
        }
        let max = (self.layout.content_h - self.body_h).max(0.0);
        self.scroll = next.clamp(0.0, max);
    }

    fn cells(&self) -> usize {
        match self.view {
            View::AppGrid => self.app_count,
            View::Emoji | View::Picker => self.rows.len(),
            View::Rows | View::Monitor | View::Mirror | View::Panel => 0,
        }
    }

    pub fn move_cursor(&mut self, dir: Dir) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let next = if self.view == View::Picker {
            self.picker_step(dir)
        } else {
            nav::step(self.cursor, dir, n, self.cells(), self.columns(), self.page_step())
        };
        self.place(next.index, next.travels);
        self.placed = true;
        self.want = self.cursor_id.clone();
        self.confirm.clear();
        self.monitor_armed.clear();
        self.from_keys = true;
        self.touch.settle();
        self.follow();
        self.dirty = true;
    }

    /// One key's move over the picker's tiles, by where they sit: Up and
    /// Down to the nearest tile of the row above or under, across
    /// sections; a page a viewport of rows; Left and Right in reading order.
    fn picker_step(&self, dir: Dir) -> nav::Step {
        let slots = &self.layout.slots;
        let n = slots.len().min(self.rows.len());
        let i = self.cursor.min(n.saturating_sub(1));
        let snap = |index| nav::Step { index, travels: false };
        let Some(at) = slots.get(i) else { return snap(0) };
        // A row's tiles share the top of their picture.
        let top = |k: usize| slots[k].y + slots[k].band;
        let mut rows: Vec<f64> = (0..n).map(top).collect();
        rows.sort_by(f64::total_cmp);
        rows.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let row = rows.iter().position(|y| (*y - top(i)).abs() < 0.5).unwrap_or(0);
        let nearest = |r: usize| (0..n).filter(|&k| (top(k) - rows[r]).abs() < 0.5).min_by(|&a, &b| (slots[a].x - at.x).abs().total_cmp(&(slots[b].x - at.x).abs()));
        let page = ((self.body_h / self.layout.cell_h.max(1.0)).floor() as usize).max(1);
        match dir {
            Dir::Home => snap(0),
            Dir::End => snap(n - 1),
            Dir::Left if i > 0 => nav::Step { index: i - 1, travels: true },
            Dir::Right if i + 1 < n => nav::Step { index: i + 1, travels: true },
            Dir::Up if row > 0 => nearest(row - 1).map_or(snap(i), |k| nav::Step { index: k, travels: true }),
            Dir::Down if row + 1 < rows.len() => nearest(row + 1).map_or(snap(i), |k| nav::Step { index: k, travels: true }),
            Dir::PageUp => nearest(row.saturating_sub(page)).map_or(snap(i), snap),
            Dir::PageDown => nearest((row + page).min(rows.len() - 1)).map_or(snap(i), snap),
            _ => snap(i),
        }
    }

    /// Ctrl+Tab and Ctrl+Shift+Tab: the first tile of the next or the
    /// previous section.
    fn section_step(&mut self, forward: bool) -> Out {
        let Some(here) = self.sections.get(self.cursor).cloned() else { return Out::None };
        let firsts: Vec<usize> = (0..self.rows.len()).filter(|&k| k == 0 || self.sections[k] != self.sections[k - 1]).collect();
        let current = firsts.iter().rposition(|&k| self.sections[k] == here).unwrap_or(0);
        let target = if forward { firsts.get(current + 1) } else { current.checked_sub(1).and_then(|c| firsts.get(c)) };
        if let Some(&index) = target {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
            self.from_keys = true;
            self.touch.settle();
            self.follow();
            self.dirty = true;
        }
        Out::None
    }

    fn page_step(&self) -> usize {
        let h = self.body_h;
        if self.view != View::Rows && self.cursor < self.cells() {
            return ((h / self.layout.cell_h.max(1.0)).floor() as usize).max(1) * self.columns();
        }
        1usize.max((h / 32.0).floor() as usize)
    }

    /// The pixels one wheel notch scrolls: a row, or a grid row of cells.
    pub fn wheel_step(&self) -> f64 {
        if matches!(self.view, View::Rows | View::Monitor | View::Panel) { 32.0 } else { self.layout.cell_h.max(1.0) }
    }

    /// One axis frame over the view, with the cursor left where it is: a
    /// wheel's notches glide a step each and stop on either end, a
    /// touchpad's travel lands at once and stretches past an end until the
    /// fingers lift, springing back over `release_ms` or coasting on.
    pub fn scroll(&mut self, travel: Travel, release_ms: f64) {
        let (step, max) = (self.wheel_step(), self.max_scroll());
        let Some((next, glide)) = travel_to(&mut self.touch, travel, self.scroll, max, step, self.body_h, release_ms, self.motion_scale) else {
            return;
        };
        self.travels = false;
        self.wheeled = glide;
        self.scroll = next;
        self.dirty = true;
    }

    fn max_scroll(&self) -> f64 {
        (self.layout.content_h - self.body_h).max(0.0)
    }

    /// The same frame over the actions column, which scrolls on its own.
    pub fn scroll_actions(&mut self, travel: Travel, release_ms: f64) {
        let slow = self.motion_scale;
        let Some(c) = &mut self.column else { return };
        let max = c.max_scroll();
        let Some((next, glide)) = travel_to(&mut c.touch, travel, c.scroll, max, c.row_h, c.view_h, release_ms, slow) else {
            return;
        };
        c.scroll = next;
        c.wheeled = glide;
        c.travels = false;
        self.dirty = true;
    }

    /// A click on row `index` (launcher spec §6): the row takes the
    /// selection and runs its first action, unless the actions column was
    /// open, which the click only closes.
    pub fn click(&mut self, store: &Store, index: usize) -> Out {
        let column = self.column.is_some();
        self.close_actions();
        if index != self.cursor {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
        }
        self.from_keys = false;
        self.dirty = true;
        if column { Out::None } else { self.activate(store, index) }
    }

    pub fn view_cursor(&self) -> Value {
        let k = self.view.name();
        let Some(slot) = self.layout.slots.get(self.cursor).filter(|_| self.cursor < self.rows.len()) else {
            return json!({"view": k, "index": -1, "id": ""});
        };
        let top = (slot.y + slot.band - self.scroll).round() as i64;
        let bottom = (slot.y + slot.h - self.scroll).round() as i64;
        json!({
            "view": k, "index": self.cursor, "id": self.rows[self.cursor].id,
            "top": top, "bottom": bottom, "viewport": self.body_h.round() as i64,
            "left": slot.x.round() as i64, "right": (slot.x + slot.w).round() as i64,
        })
    }

    pub fn status(&self, store: &Store) -> Value {
        let checked: Vec<&str> = {
            let snap = snapshot(store);
            self.rows
                .iter()
                .filter(|n| crate::surfaces::packages::row_checked(store, n, toggles::checked_for(Some(n), Some(&snap), Some(&store.menu.checked))))
                .map(|n| n.id.as_str())
                .collect()
        };
        let cells: Vec<&str> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| r.id.as_str()).collect(),
            _ => self.rows.iter().map(|r| r.id.as_str()).collect(),
        };
        let drawn: Vec<Value> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| json!({"id": r.id, "label": r.label})).collect(),
            View::Rows | View::Monitor | View::Mirror => {
                self.rows
                    .iter()
                    .map(|r| {
                        let picture = !r.icon_source.is_empty() && store.menu.row_icons.get(&r.icon_source).is_some_and(Option::is_some);
                        json!({"id": r.id, "label": r.label, "value": crate::surfaces::packages::row_value(store, r), "picture": picture})
                    })
                    .collect()
            }
            View::Emoji | View::Picker | View::Panel => Vec::new(),
        };
        json!({
            "isOpen": self.open,
            "level": self.level,
            "scrollTop": (self.scroll + self.touch.value(Instant::now())).round() as i64,
            "scrollMax": self.max_scroll().round() as i64,
            "overscroll": self.touch.value(Instant::now()).round() as i64,
            "coasting": self.touch.coasting(),
            "viewHeight": self.body_h.round() as i64,
            "wheelStep": self.wheel_step().round() as i64,
            "placeholder": self.placeholder(store),
            "query": w::masked(&self.query),
            "prompt": (self.mode == Mode::Input).then_some(self.select_prompt.as_str()),
            "secretShown": self.query.secret && self.query.shown,
            "caret": self.query.caret_chars().0,
            "selection": self.query.selection().is_some(),
            "sections": self.section_names(),
            "view": self.view.name(),
            "panel": self.panel.as_ref().filter(|_| self.on_panel()).map(|p| p.id.as_str()),
            "panelCursor": self.panel.as_ref().filter(|_| self.on_panel()).and_then(|p| p.cursor.shown()),
            "columns": self.columns(),
            "cursor": self.cursor,
            "cursorId": self.cursor_id,
            "empty": self.empty.as_ref().map_or("", |n| n.id.as_str()),
            "mode": match self.mode { Mode::Menu => "menu", Mode::Select => "select", Mode::Input => "input" },
            "viewCursor": self.view_cursor(),
            "rows": self.rows.len(),
            "cells": cells,
            "ids": self.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            "checked": checked,
            "drawn": drawn,
            "abbrev": self.rows.get(self.cursor).and_then(|n| self.lookup.source_of(&fs_menu::actions::subject_id(n).to_string())).map(fs_menu::abbrev::Source::as_str),
            "token": self.token(store),
            "actionsOpen": self.column.is_some(),
            "actions": self.column.as_ref().map_or_else(Vec::new, Column::ids),
            "actionCursor": self.column.as_ref().map_or(-1, |c| c.cursor as i64),
            "actionsScroll": self.column.as_ref().map_or(0, |c| (c.scroll + c.touch.value(Instant::now())).round() as i64),
            "actionsScrollMax": self.column.as_ref().map_or(0, |c| c.max_scroll().round() as i64),
            "actionsMore": self.column.as_ref().and_then(|c| c.more(c.scroll + c.touch.value(Instant::now()))).map_or(0, |(_, n)| n),
            "pane": self.pane().map(|n| self.pane_status(store, n)),
            "cardWidth": self.card_width(&store.theme.theme).round() as i64,
        })
    }

    /// What the preview pane holds, for `menu status`.
    fn pane_status(&self, store: &Store, n: &Node) -> Value {
        use fs_menu::providers::Peek;
        let kind = pane_kind(n).map_or("", PaneKind::name);
        let mut v = json!({"kind": kind, "id": n.id, "path": n.path});
        let peek = store.menu.peek.as_ref().filter(|(p, _)| *p == n.path).map(|(_, p)| &p.peek);
        match peek {
            Some(Peek::Text(lines)) => v["lines"] = json!(lines),
            Some(Peek::Folder { count, names }) => {
                v["count"] = json!(count);
                v["names"] = json!(names.iter().map(|e| e.name.as_str()).collect::<Vec<_>>());
            }
            Some(Peek::Image) => v["image"] = json!(true),
            Some(Peek::Binary) => v["binary"] = json!(true),
            Some(Peek::Unreadable(why)) => v["error"] = json!(why),
            None => {}
        }
        match pane_picture(n) {
            Some(PanePicture::Decoded(path)) => {
                v["picture"] = json!(store.clipboard.preview.as_ref().is_some_and(|p| p.path == path && p.bitmap.is_some()));
            }
            Some(PanePicture::Window(_)) => v["picture"] = json!(self.pane_frame.is_some()),
            None => {}
        }
        v
    }

    fn resolve_route(store: &Store, route: &str) -> Option<String> {
        let nodes = store.menu.nodes();
        if nodes.contains_key(route) {
            return Some(route.to_owned());
        }
        nodes.values().find(|n| n.aliases.iter().any(|a| a == route)).map(|n| n.id.clone())
    }

    /// The level a route row stands for: its own id, or a link's target.
    fn level_of(store: &Store, node: &Node) -> Option<String> {
        match node.kind {
            NodeKind::Submenu | NodeKind::Provider => Some(node.id.clone()),
            NodeKind::Link => Some(node.target.clone().filter(|t| store.menu.nodes().contains_key(t)).unwrap_or(node.id.clone())),
            _ => None,
        }
    }

    /// `open(route)`: a fresh session on `route`'s level, or a `:`-led
    /// prefill at the root, which a route's prefix turns into its mode.
    pub fn open(&mut self, store: &Store, route: Option<&str>) {
        self.after_unmap = None;
        self.grab.cancel();
        self.abandon_select();
        self.ask_fresh(store);
        plugins::launcher(true);
        let route = route.filter(|r| !r.is_empty());
        let prefill = route.filter(|r| r.starts_with(':')).unwrap_or("").to_owned();
        let mut target = None;
        if prefill.is_empty()
            && let Some(id) = route.and_then(|r| Self::resolve_route(store, r))
            && let Some(node) = store.menu.nodes().get(&id)
        {
            target = Self::level_of(store, node);
        }
        self.enter_level(store, target);
        self.query.set(&prefill);
        self.query_changed(store);
        self.open = true;
        self.key.clear();
    }

    /// The conditions and the binds, asked for once the tree first arrives
    /// and again on every open. An open draws on the last results; each
    /// check runs on the service thread and only a changed one lands, so
    /// just its row moves. The apps follow their own entry watch and launch
    /// counts.
    pub fn ask_fresh(&self, store: &Store) {
        let conds: Vec<(String, String, bool)> = store
            .menu
            .nodes()
            .values()
            .flat_map(|n| {
                let when = n.when.clone().filter(|c| !c.starts_with(toggles::PREFIX)).map(|c| (n.id.clone(), c, true));
                let checked = match &n.checked {
                    Some(toggles::Checked::Shell(line)) => Some((n.id.clone(), line.clone(), false)),
                    _ => None,
                };
                when.into_iter().chain(checked)
            })
            .collect();
        index::ask(Ask::Conds(conds));
        index::ask(Ask::Binds);
        index::ask(Ask::Files);
        let paths: Vec<String> = store.state.data.picks.iter().filter_map(|r| match SubjectId::parse(&r.id) { SubjectId::File(path) => Some(path), _ => None }).collect();
        if !paths.is_empty() {
            index::ask(Ask::Stat(paths));
        }
    }

    pub fn open_select(&mut self, prompt: &str, options: Vec<String>, token: &str) {
        self.begin_selection();
        self.mode = Mode::Select;
        self.select_prompt = prompt.into();
        self.select_options = options;
        self.select_token = token.into();
        self.confirm.clear();
        self.query.clear();
        self.open = true;
        self.key.clear();
    }

    pub fn open_input(&mut self, prompt: &str, token: &str, secret: bool) {
        self.begin_selection();
        self.mode = Mode::Input;
        self.query.secret = secret;
        self.query.shown = false;
        self.select_prompt = prompt.into();
        self.select_options.clear();
        self.select_token = token.into();
        self.confirm.clear();
        self.query.clear();
        self.open = true;
        self.key.clear();
    }

    /// A secret input step holding `text`, drawn in the clear.
    pub fn open_shown(&mut self, prompt: &str, token: &str, text: &str) {
        self.open_input(prompt, token, true);
        self.query.set(text);
        self.query.shown = true;
    }

    pub fn close(&mut self) {
        self.sessions += 1;
        self.abandon_select();
        if self.on_picker() {
            self.picker_abandon();
        }
        self.monitor_wants = None;
        self.monitor_armed.clear();
        if self.on_mirror() {
            crate::services::mirror::command(crate::services::mirror::Cmd::Stream(None));
        }
        self.open = false;
        self.confirm.clear();
        self.column = None;
        self.children.clear();
        self.panel = None;
        plugins::launcher(false);
        self.plugin_query = None;
    }

    /// Every input and select answer since the last call, for the in-process
    /// token owners: the token, the value (a secret one included, held only
    /// here), and whether it was cancelled.
    pub fn take_resolved(&mut self) -> Vec<(String, Option<String>, bool)> {
        std::mem::take(&mut self.resolved)
    }

    fn write_selection(&mut self, payload: Value) {
        let token = payload.get("token").cloned().unwrap_or(Value::Null);
        self.resolved.push((
            token.as_str().unwrap_or_default().to_owned(),
            payload.get("value").and_then(Value::as_str).map(str::to_owned),
            payload.get("cancelled").and_then(Value::as_bool).unwrap_or(false),
        ));
        let filed = if self.query.secret { json!({"token": token, "cancelled": true, "secret": true}) } else { payload };
        let path = selection_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&path, filed.to_string());
        if self.query.secret {
            self.query.clear();
            self.query.secret = false;
            self.query.shown = false;
        }
    }

    fn abandon_select(&mut self) {
        if self.mode != Mode::Menu {
            let token = self.select_token.clone();
            self.write_selection(json!({"token": token, "cancelled": true}));
            self.mode = Mode::Menu;
        }
    }

    fn begin_selection(&mut self) {
        let pending = self.mode != Mode::Menu;
        self.abandon_select();
        if !pending {
            let _ = std::fs::remove_file(selection_path());
        }
    }

    fn complete_select(&mut self, value: &str) {
        let token = self.select_token.clone();
        self.write_selection(json!({"token": token, "value": value}));
        self.mode = Mode::Menu;
        self.close();
    }

    fn enter_level(&mut self, store: &Store, id: Option<String>) {
        if self.level.as_deref() == Some(PICKER_ROUTE) && id.as_deref() != Some(PICKER_ROUTE) {
            self.picker_abandon();
        }
        let entering = id.as_deref() == Some(PICKER_ROUTE);
        self.monitor_armed.clear();
        if id.as_deref() == Some(MIRROR_ROUTE) {
            self.mirror_current.clear();
            crate::services::mirror::command(crate::services::mirror::Cmd::List);
        } else if self.level.as_deref() == Some(MIRROR_ROUTE) {
            crate::services::mirror::command(crate::services::mirror::Cmd::Stream(None));
        }
        if id.as_deref() == Some(MONITOR_ROUTE) && self.level.as_deref() != Some(MONITOR_ROUTE) {
            self.monitor_history = MonitorHistory::default();
        }
        self.column = None;
        self.children.clear();
        self.monitor_wants = (id.as_deref() == Some(MONITOR_ROUTE)).then(|| {
            use crate::services::wants::{Source, Want};
            (Want::new(Source::Monitor), Want::new(Source::Processes))
        });
        // A level that is a launcher route of its own holds its page.
        let route = id.as_deref().and_then(PanelId::parse).filter(|p| p.route() == Some(*p));
        match route {
            Some(r) if self.level.as_deref() != Some(r.as_str()) || self.panel.is_none() => self.panel = Embedded::new(r),
            Some(_) => {}
            None => self.panel = None,
        }
        self.level = id;
        if entering {
            self.picker_enter(store);
        }
        self.confirm.clear();
        self.from_keys = true;
        self.query.clear();
        if self.level.as_deref() == Some("emoji") && store.menu.emoji.is_none() {
            index::ask(Ask::Emoji);
        }
    }

    /// One level up: Backspace, Escape and ← on an empty field, and a click
    /// on the token. At the root it closes.
    pub fn pop(&mut self, store: &Store) -> Out {
        if self.children.pop().is_some() {
            // One listing is held at a time: the folder stepped back into
            // is read again.
            if let Some(Children { into: Into::Folder(path), .. }) = self.children.last() {
                index::ask(Ask::Folder(path.clone()));
            }
            self.query.clear();
            self.confirm.clear();
            self.from_keys = true;
            self.dirty = true;
            return Out::None;
        }
        let Some(level) = self.level.clone() else {
            self.close();
            return Out::Close;
        };
        if self.on_picker()
            && let Some(page) = self.picker.from.take()
        {
            return Out::Call(IpcCall::new("panel", "open").arg(page.as_str()));
        }
        let parent = store.menu.nodes().get(&level).and_then(|n| n.parent_id.clone());
        self.enter_level(store, parent);
        self.dirty = true;
        Out::None
    }

    /// A click on the token's ancestor at `depth`: every mode inside it
    /// left, that one kept.
    pub fn leave_to(&mut self, store: &Store, depth: usize) {
        let mut held = self.token(store).len();
        while held > depth + 1 {
            self.pop(store);
            let now = self.token(store).len();
            if now >= held {
                break;
            }
            held = now;
        }
    }

    pub fn set_query(&mut self, store: &Store, text: &str) {
        self.query.set(text);
        self.query_changed(store);
    }

    /// The clipboard's text into the query.
    pub fn paste(&mut self, store: &Store, text: &str) {
        if self.query.paste(text) {
            self.query_changed(store);
            self.dirty = true;
        }
    }

    /// A route's prefix and a space become its mode, what follows them the
    /// query inside it.
    fn query_changed(&mut self, store: &Store) {
        self.confirm.clear();
        self.monitor_armed.clear();
        if let Some(c) = &mut self.column {
            c.refilter(self.query.text());
            return;
        }
        if self.mode != Mode::Menu {
            return;
        }
        let Some((level, rest)) = model::prefixed(store.menu.nodes(), self.query.text()).and_then(|(n, rest)| Some((Self::level_of(store, n)?, rest.to_owned()))) else {
            return;
        };
        self.enter_level(store, Some(level));
        self.query.set(&rest);
    }

    /// The row as a subject, with what the shell knows about it.
    pub fn subject<'a>(&'a self, store: &'a Store, node: &'a Node) -> fs_menu::actions::Subject<'a> {
        let windows = match &node.entry {
            Some(entry) if node.kind == NodeKind::App => app_windows(store, entry).len(),
            _ => 0,
        };
        let removable = match &node.entry {
            Some(entry) if node.kind == NodeKind::App && store.activity.fspec.installed => {
                if !store.packages.owners.contains_key(&entry.id) {
                    crate::services::packages::send(crate::services::packages::Cmd::Owner(entry.id.clone()));
                }
                store.packages.removable(&entry.id)
            }
            _ => None,
        };
        let id = fs_menu::actions::subject_id(node).to_string();
        let abbrev = self.abbrev_key.as_deref().zip(self.lookup.source_of(&id));
        fs_menu::actions::Subject {
            node,
            windows,
            discrete_gpu: store.monitor.cards.iter().any(|c| c.discrete),
            picker_select: self.picker.select,
            abbrev,
            pinned: store.state.data.recent_pinned.contains(&id),
            removable,
            single: node.kind == NodeKind::Image
                && self.on_picker()
                && self.picker_tile_of(store, &node.id).is_some_and(|t| matches!(t.kind, fs_menu::providers::ItemKind::Single { .. })),
        }
    }

    /// The cursor onto `index` as a key would put it, the column left as
    /// it is.
    fn select(&mut self, index: usize) {
        if index != self.cursor {
            self.place(index, false);
            self.placed = true;
            self.want = self.cursor_id.clone();
            self.confirm.clear();
        }
    }

    /// Tab, a right-click and `menu actions`: the row's actions in the
    /// column beside the list. False with nothing to list.
    pub fn open_actions(&mut self, store: &Store, index: usize) -> bool {
        if self.mode != Mode::Menu || matches!(self.view, View::Mirror) {
            return false;
        }
        let Some(node) = self.rows.get(index).cloned() else { return false };
        // The sky has no file to open or slot of its own to fill.
        if self.view == View::Picker && !node.path.starts_with('/') {
            return false;
        }
        let actions = fs_menu::actions::actions_for(&self.subject(store, &node));
        let kind = node.kind.as_str();
        let asked: Vec<String> = fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
            .filter(|p| p.subjects.as_ref().is_some_and(|k| k.iter().any(|k| k == kind)))
            .map(|p| p.id.clone())
            .collect();
        if actions.is_empty() && asked.is_empty() {
            return false;
        }
        let subject = fs_menu::actions::subject_id(&node).to_string();
        let asked_at = Instant::now();
        for id in &asked {
            plugins::send(id, plugins::actions_event(&subject, kind, &node.path));
        }
        self.close_actions();
        self.select(index);
        self.confirm.clear();
        let shown = (0..actions.len()).collect();
        let list_query = self.query.text().to_owned();
        self.query.clear();
        self.column = Some(Column {
            node,
            actions,
            list_query,
            target: None,
            shown,
            cursor: 0,
            opened: Instant::now(),
            scroll: 0.0,
            touch: Touchpad::new(),
            wheeled: false,
            travels: false,
            row_h: 0.0,
            view_h: 0.0,
            asked,
            asked_at,
            joined: None,
        });
        self.dirty = true;
        true
    }

    /// The plugins' answers to the open column's `actions` events, joined
    /// under its actions as they arrive. One later than [`PLUGIN_ANSWER`]
    /// is left out of this opening; the selected action stays where it is.
    pub fn plugin_answers(&mut self, store: &Store) -> bool {
        let filter = self.query.text().to_owned();
        let Some(c) = self.column.as_mut().filter(|c| c.target.is_none() && !c.asked.is_empty()) else { return false };
        let subject = fs_menu::actions::subject_id(&c.node).to_string();
        let mut joined = false;
        for id in std::mem::take(&mut c.asked) {
            let answer = store.plugins.answers.get(&id).filter(|a| a.at >= c.asked_at && a.subject == subject);
            let Some(answer) = answer else {
                c.asked.push(id);
                continue;
            };
            if answer.at > c.asked_at + PLUGIN_ANSWER {
                continue;
            }
            let base = c.actions.len();
            for a in &answer.actions {
                if c.actions.iter().any(|have| have.id == a.id) {
                    continue;
                }
                let label = if a.label.is_empty() { a.id.clone() } else { a.label.clone() };
                let mut action = Action::new(&a.id, label, Run::Intent(plugin_send(&id, &plugins::action_event(&subject, &a.id))));
                action.icon = Some(a.icon.clone()).filter(|i| !i.is_empty());
                c.actions.push(action);
            }
            let labels: Vec<&str> = c.actions[base..].iter().map(|a| a.label.as_str()).collect();
            c.shown.extend(fs_menu::actions::filter(&labels, &filter).into_iter().map(|i| i + base));
            if c.actions.len() > base {
                c.joined.get_or_insert((base, Instant::now()));
                joined = true;
            }
        }
        if joined {
            self.dirty = true;
        }
        joined
    }

    /// Uninstall joining an app's open column when the owner query answers
    /// after Tab; the selected action stays where it is.
    pub fn owner_answer(&mut self, store: &Store) -> bool {
        let filter = self.query.text().to_owned();
        let Some(c) = self.column.as_ref().filter(|c| c.target.is_none() && !c.actions.iter().any(|a| a.id == "uninstall")) else { return false };
        let Some(action) = fs_menu::actions::uninstall_action(&self.subject(store, &c.node)) else { return false };
        let Some(c) = self.column.as_mut() else { return false };
        let at = c.actions.len();
        if !fs_menu::actions::filter(&[action.label.as_str()], &filter).is_empty() {
            c.shown.push(at);
        }
        c.actions.push(action);
        self.dirty = true;
        true
    }

    /// The column gone, the list's query back in the field.
    pub fn close_actions(&mut self) {
        if let Some(c) = self.column.take() {
            self.query.set(&c.list_query);
            self.confirm.clear();
            self.dirty = true;
        }
    }

    /// Shift+Tab, Escape and ← in the column: a target list back to its
    /// actions, the actions back to the list.
    fn column_back(&mut self) {
        let Some(c) = &mut self.column else { return };
        if c.target.take().is_none() {
            return self.close_actions();
        }
        self.query.clear();
        c.refilter("");
        c.opened = Instant::now();
        self.confirm.clear();
        self.dirty = true;
    }

    fn column_move(&mut self, dir: Dir) {
        let page = (self.body_h / fs_theme::tokens::LAUNCHER.row_height).floor().max(1.0) as usize;
        let Some(c) = &mut self.column else { return };
        c.cursor = nav::step(c.cursor, dir, c.shown.len(), 0, 1, page).index;
        c.follow();
        self.confirm.clear();
        self.dirty = true;
    }

    /// Enter in the column: the action, or the target, under its cursor.
    fn column_run(&mut self, store: &Store) -> Out {
        let Some(c) = &self.column else { return Out::None };
        let Some(&i) = c.shown.get(c.cursor) else { return Out::None };
        let node = c.node.clone();
        match &c.target {
            Some((_, targets)) if targets[i].kind == NodeKind::Note => Out::None,
            Some((action, targets)) => {
                let (action, target) = (action.clone(), targets[i].clone());
                self.close_actions();
                self.run_target(store, &node, &action, &target)
            }
            None => {
                let action = c.actions[i].clone();
                self.run_from_column(store, &node, &action)
            }
        }
    }

    /// One action off the column: a target action lists its targets in
    /// it, a confirm action arms there, anything else closes it and runs.
    fn run_from_column(&mut self, store: &Store, node: &Node, action: &Action) -> Out {
        match action.run {
            Run::Note => return Out::None,
            Run::Target(kind) => {
                let targets = targets_for(store, kind);
                if let Some(c) = &mut self.column {
                    c.target = Some((action.clone(), targets));
                    c.refilter("");
                    c.opened = Instant::now();
                }
                self.query.clear();
                self.dirty = true;
                return Out::None;
            }
            _ => {}
        }
        let armed = Confirm::of(node, action);
        if action.confirm && !self.confirm.is(&armed) {
            self.confirm.arm(armed);
            self.dirty = true;
            return Out::None;
        }
        self.close_actions();
        // Closing the column disarms; this run is the confirmed one.
        if action.confirm {
            self.confirm.arm(armed);
        }
        self.run(store, node, action)
    }

    /// The column action a first Enter armed, by position in the actions.
    fn column_armed(&self) -> Option<usize> {
        let c = self.column.as_ref()?;
        let armed = self.confirm.get().filter(|k| k.node == c.node.id)?;
        c.actions.iter().position(|a| a.id == armed.action)
    }

    /// A click on the column's `i`th entry.
    pub fn column_pick(&mut self, store: &Store, i: usize) -> Out {
        if let Some(c) = &mut self.column
            && i < c.shown.len()
        {
            c.cursor = i;
        }
        self.column_run(store)
    }

    /// `menu runAction`: the action `id` on row `index`, as the column
    /// would run it, which opens the column for one that asks a target.
    pub fn run_action(&mut self, store: &Store, index: usize, id: &str) -> Result<Out, String> {
        let on_row = self.column.as_ref().is_some_and(|c| self.rows.get(index).is_some_and(|r| r.id == c.node.id));
        if !on_row && !self.open_actions(store, index) {
            return Err(format!("row {index} has no actions"));
        }
        let Some((node, action)) = self.column.as_ref().and_then(|c| Some((c.node.clone(), c.actions.iter().find(|a| a.id == id)?.clone()))) else {
            if !on_row {
                self.close_actions();
            }
            return Err(format!("no action {id}"));
        };
        self.query.clear();
        if let Some(c) = &mut self.column {
            c.target = None;
            c.refilter("");
            c.cursor = c.actions.iter().position(|a| a.id == id).unwrap_or(0);
        }
        Ok(self.run_from_column(store, &node, &action))
    }

    /// →, and `menu into`: row `index`'s own children, `None` when it has
    /// none to step into.
    pub fn step_into(&mut self, store: &Store, index: usize) -> Option<Out> {
        if self.mode != Mode::Menu {
            return None;
        }
        let node = self.rows.get(index).cloned()?;
        let action = fs_menu::actions::into_action(&self.subject(store, &node))?;
        self.close_actions();
        self.select(index);
        Some(self.run(store, &node, &action))
    }

    /// A target action's pick: the subject's windows onto a workspace, a
    /// file opened in an app or sent to a peer.
    fn run_target(&mut self, store: &Store, node: &Node, action: &Action, target: &Node) -> Out {
        match action.run {
            Run::Target(TargetKind::App) => {
                let Some(id) = target.id.strip_prefix("target-app:") else { return Out::None };
                let Some(mut argv) = index::launch_argv(&store.menu, id) else { return Out::None };
                argv.push(node.path.clone());
                crate::services::launch::spawn(store, id, &argv);
            }
            Run::Target(TargetKind::Peer) => {
                use crate::services::localsend;
                let Some(peer) = target.id.strip_prefix("peer:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| store.localsend.peers.get(i)) else {
                    return Out::None;
                };
                let payload = match fs_menu::providers::clipboard_entry_id(node) {
                    Some(id) => {
                        let Some(entry) = store.clipboard.items.iter().find(|e| e.id == id) else { return Out::None };
                        localsend::Payload::of(entry)
                    }
                    None => localsend::Payload::Path(node.path.clone()),
                };
                localsend::command(localsend::Cmd::Send { peer: peer.clone(), payload });
            }
            Run::Target(TargetKind::Workspace) => {
                let Some(idx) = target.id.strip_prefix("workspace:").and_then(|i| i.parse::<i64>().ok()) else { return Out::None };
                let windows = match &node.entry {
                    Some(entry) if node.kind == NodeKind::App => app_windows(store, entry),
                    _ => vec![WindowId::new(node.path.clone())],
                };
                let output = store.compositor.snapshot.focused_output_name.clone();
                for id in windows.into_iter().filter(|w| !w.is_empty()) {
                    compositor::send(compositor::Command::MoveWindowTo { id, idx, output: output.clone() });
                }
            }
            _ => return Out::None,
        }
        self.picked(store, node);
        self.close();
        Out::Close
    }

    /// `Edit abbreviation`'s answer: the subject pinned under the new
    /// abbreviation, the one it edits forgotten.
    pub fn edit_answer(&mut self, store: &Store, value: Option<String>, cancelled: bool) {
        let Some((subject, old)) = self.editing.take() else { return };
        let Some(new) = value.filter(|_| !cancelled).as_deref().and_then(fs_menu::abbrev::key) else { return };
        let book = fs_menu::abbrev::forget(&index::abbreviation_book(store), &old, &subject);
        let book = fs_menu::abbrev::pin(&book, &new, &subject);
        state::set(vec![state::Field::Abbreviations(Some(book))]);
    }

    /// Enter: the row's first action.
    pub fn activate(&mut self, store: &Store, index: usize) -> Out {
        self.close_actions();
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        if node.kind == NodeKind::Option {
            self.complete_select(&node.label);
            return Out::Close;
        }
        if self.view == View::Picker
            && let Some(fs_menu::providers::ItemKind::Mode { mode, .. }) = self.picker_tile_of(store, &node.id).map(|t| t.kind)
        {
            return self.picker_style(store, mode);
        }
        match fs_menu::actions::first_action(&self.subject(store, &node)) {
            Some(action) => self.run(store, &node, &action),
            None => Out::None,
        }
    }

    /// Shift+Enter: the row's second action, else its first.
    pub fn activate_alternate(&mut self, store: &Store, index: usize) -> Out {
        self.close_actions();
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        match fs_menu::actions::second_action(&self.subject(store, &node)) {
            // One that asks a target lists them in the column, as Tab then
            // Enter on it would.
            Some(action) if matches!(action.run, Run::Target(_)) => self.run_action(store, index, &action.id).unwrap_or(Out::None),
            Some(action) => self.run(store, &node, &action),
            None => self.activate(store, index),
        }
    }

    /// One pick of `node` into the picks ledger, and the query it was
    /// picked under taught to the learned abbreviations, unless the query
    /// already names it exactly or the subject never ranks in the tree (an
    /// emoji, a folder's entry). An emoji's grid order keeps its own ledger.
    fn picked(&self, store: &Store, node: &Node) {
        if self.mode != Mode::Menu {
            return;
        }
        let subject = fs_menu::actions::subject_id(node);
        let id = subject.to_string();
        let file = match &subject {
            SubjectId::File(path) => Some(path.as_str()),
            _ => None,
        };
        if file.is_none() && !resolves(store, &id) {
            return;
        }
        if let Some(path) = file {
            index::ask(Ask::Stat(vec![path.to_owned()]));
        }
        let now = now_ms();
        // Only an uninstalled app, once the apps scan has landed, and a
        // deleted file are pruned: a provider's rows (a network out of
        // range, a stopped player) come back, and before the scan no app
        // resolves at all.
        let apps_known = !store.menu.exec.apps.is_empty();
        let gone = |id: &str| match SubjectId::parse(id) {
            SubjectId::File(path) => store.menu.files.get(&path) == Some(&None),
            SubjectId::App(_) => apps_known && !resolves(store, id),
            SubjectId::Emoji(_) | SubjectId::Node(_) => false,
        };
        let held: Vec<fs_menu::frecency::Record> = store.state.data.picks.iter().filter(|r| !gone(&r.id)).cloned().collect();
        let mut fields = vec![state::Field::Picks(fs_menu::frecency::record(&held, &id, now, None))];
        let q = self.list_text();
        let q_norm = fs_menu::search::normalize(q.trim());
        let exact = fs_menu::search::tier(node, &q_norm, &fs_menu::search::slug(&q_norm)) == fs_menu::search::TIER_EXACT;
        if !exact && fs_menu::abbrev::key(q).is_some() && store.menu.nodes().contains_key(&node.id) {
            let book = fs_menu::abbrev::learn(&index::abbreviation_book(store), q, &id, now);
            fields.push(state::Field::Abbreviations(Some(book)));
        }
        state::set(fields);
    }

    /// Whether a confirm action on row `id` is armed.
    pub fn confirming(&self, id: &str) -> bool {
        self.confirm.get().is_some_and(|k| k.node == id)
    }

    /// One action on one row. A `confirm` action arms on the first run and
    /// runs on the second; a `keepOpen` one leaves the launcher up.
    pub fn run(&mut self, store: &Store, node: &Node, action: &fs_menu::actions::Action) -> Out {
        use fs_menu::actions::Run;
        if action.confirm && !self.confirm.press(Confirm::of(node, action)) {
            self.dirty = true;
            return Out::None;
        }
        self.confirm.clear();
        let out = match &action.run {
            Run::Into(id) => {
                self.picked(store, node);
                let into = if *id != fs_menu::actions::subject_id(node).to_string() {
                    None
                } else if node.kind == NodeKind::App {
                    Some(Into::Windows(id.clone()))
                } else if fs_menu::providers::is_folder_subject(node) {
                    index::ask(Ask::Folder(node.path.clone()));
                    Some(Into::Folder(node.path.clone()))
                } else if fs_menu::providers::is_wifi_network(node) {
                    Some(Into::Network(node.label.clone()))
                } else {
                    None
                };
                if let Some(into) = into {
                    self.children.push(Children { into, label: node.label.clone() });
                    self.query.clear();
                    self.from_keys = true;
                    self.dirty = true;
                    return Out::None;
                }
                let to = if store.menu.nodes().contains_key(id) { id.clone() } else { node.id.clone() };
                self.enter_level(store, Some(to));
                return Out::None;
            }
            Run::Input(prompt) => {
                let subject = fs_menu::actions::subject_id(node).to_string();
                let editing = self.abbrev_key.clone().map(|q| (subject, q));
                self.open_input(prompt, ABBREV_EDIT, false);
                self.editing = editing;
                return Out::None;
            }
            Run::Target(_) | Run::Note => return Out::None,
            Run::Ipc(call) => Out::Call(call.clone()),
            Run::Intent(intent) => match intent {
                // An abbreviation's own actions are no pick: picking would
                // teach the pair straight back.
                Intent::AbbrevForget(q) | Intent::AbbrevPin(q) => {
                    let subject = fs_menu::actions::subject_id(node).to_string();
                    let book = index::abbreviation_book(store);
                    let book = if matches!(intent, Intent::AbbrevPin(_)) {
                        fs_menu::abbrev::pin(&book, q, &subject)
                    } else {
                        fs_menu::abbrev::forget(&book, q, &subject)
                    };
                    state::set(vec![state::Field::Abbreviations(Some(book))]);
                    self.dirty = true;
                    if action.keep_open {
                        return Out::None;
                    }
                    self.close();
                    return Out::Close;
                }
                // A pin is no pick either: it would count twice.
                Intent::RecentPin(id) | Intent::RecentUnpin(id) => {
                    let mut pinned: Vec<String> = store.state.data.recent_pinned.iter().filter(|p| *p != id).cloned().collect();
                    if matches!(intent, Intent::RecentPin(_)) {
                        pinned.push(id.clone());
                    }
                    state::set(vec![state::Field::RecentPinned(pinned)]);
                    self.dirty = true;
                    if action.keep_open {
                        return Out::None;
                    }
                    self.close();
                    return Out::Close;
                }
                Intent::PluginSend(id, line) => {
                    plugins::send(id, line.clone());
                    Out::None
                }
                Intent::WindowFocus(id) => {
                    compositor::focus_window(&WindowId::new(id));
                    Out::None
                }
                Intent::WindowClose(id) => {
                    compositor::close_window(&WindowId::new(id));
                    Out::None
                }
                Intent::WindowFloat(id) => {
                    compositor::send(compositor::Command::ToggleFloating(WindowId::new(id)));
                    Out::None
                }
                Intent::WindowFullscreen(id) => {
                    compositor::send(compositor::Command::ToggleFullscreen(WindowId::new(id)));
                    Out::None
                }
                Intent::AppOpen(id) => {
                    let Some(entry) = node.entry.as_ref().filter(|e| e.id == *id) else { return Out::None };
                    let target = fs_menu::appmatch::next_window(&app_windows(store, entry), &store.compositor.snapshot.focused_window_id);
                    if let Some(target) = target {
                        compositor::focus_window(&target);
                    } else if let Some(argv) = index::launch_argv(&store.menu, id) {
                        crate::services::launch::spawn(store, id, &argv);
                    }
                    Out::None
                }
                Intent::AppNew(id) => {
                    if let Some(argv) = index::launch_argv(&store.menu, id) {
                        crate::services::launch::spawn(store, id, &argv);
                    }
                    Out::None
                }
                Intent::AppAction(id, key) => {
                    if let Some(argv) = index::action_argv(&store.menu, id, key) {
                        crate::services::launch::spawn(store, id, &argv);
                    }
                    Out::None
                }
                Intent::WallpaperSet(path) => {
                    self.picked(store, node);
                    return if self.choose_image(store, path) { Out::Close } else { Out::None };
                }
                Intent::WallpaperSetFor(half, path) => {
                    let Some(half) = state::Half::parse(half) else { return Out::None };
                    return if self.choose_for(store, path, half) { Out::Close } else { Out::None };
                }
                Intent::EndProcess(pid) => {
                    self.dirty = true;
                    if !self.monitor_armed.press(*pid) {
                        return Out::None;
                    }
                    Out::Call(IpcCall::new("monitor", "kill").arg(pid.to_string()).arg("TERM"))
                }
                _ => Out::Intent(intent.clone()),
            },
            Run::Spawn(argv) => {
                compositor::spawn(argv);
                Out::None
            }
            Run::Shell(line) => {
                if !line.is_empty() {
                    compositor::spawn(&["sh", "-c", line].map(String::from));
                }
                Out::None
            }
        };
        if action.paste_after {
            self.paste = true;
        }
        self.picked(store, node);
        if node.id.starts_with("emoji.") {
            let next = fs_menu::frecency::record(&store.state.data.emoji_uses, &node.id, now_ms(), None);
            state::set(vec![state::Field::EmojiUses(next)]);
        }
        if action.keep_open {
            return out;
        }
        self.close();
        match out {
            Out::Call(_) | Out::Intent(_) => out,
            Out::None | Out::Close => Out::Close,
        }
    }

    /// One key, through `keys::key_action`, or into the field as `edit`.
    pub fn key(&mut self, store: &Store, key: keys::Key, edit: field::Key, mods: field::Modifiers, repeat: bool, text: Option<&str>) -> Out {
        if self.on_mirror() && matches!(key, keys::Key::Tab | keys::Key::Return | keys::Key::Enter) {
            self.mirror_cycle(store, 1);
            return Out::None;
        }
        let column = self.column.is_some();
        let caret_end = self.query.caret() == self.query.text().len() && self.query.selection().is_none();
        let into = key == keys::Key::Right
            && !column
            && caret_end
            && self.view == View::Rows
            && self.rows.get(self.cursor).is_some_and(|n| fs_menu::actions::into_action(&self.subject(store, n)).is_some());
        let ctx = KeyCtx {
            mode: self.key_mode(),
            query: !self.query.is_empty(),
            grid: !matches!(self.view, View::Rows | View::Monitor),
            app_view: false,
            scrollable: false,
            sections: self.view == View::Picker && self.picker_sections.len() > 1,
            level: self.level.is_some() || !self.children.is_empty(),
            column,
            into,
        };
        let action = keys::key_action(key, mods, repeat, &ctx);
        if column {
            match action {
                KeyAction::Up => self.column_move(Dir::Up),
                KeyAction::Down => self.column_move(Dir::Down),
                KeyAction::PageUp => self.column_move(Dir::PageUp),
                KeyAction::PageDown => self.column_move(Dir::PageDown),
                KeyAction::Home => self.column_move(Dir::Home),
                KeyAction::End => self.column_move(Dir::End),
                KeyAction::Back => self.column_back(),
                KeyAction::Clear => self.set_query(store, ""),
                KeyAction::Activate | KeyAction::ActivateAlternate => return self.column_run(store),
                KeyAction::Pass => return self.edit(store, edit, mods, text),
                _ => {}
            }
            self.dirty = true;
            return Out::None;
        }
        let out = match action {
            KeyAction::Pass => return self.edit(store, edit, mods, text),
            KeyAction::Actions => {
                self.open_actions(store, self.cursor);
                Out::None
            }
            KeyAction::Into => self.step_into(store, self.cursor).unwrap_or(Out::None),
            KeyAction::Up => self.dir(Dir::Up),
            KeyAction::Down => self.dir(Dir::Down),
            KeyAction::Left => self.dir(Dir::Left),
            KeyAction::Right => self.dir(Dir::Right),
            KeyAction::PageUp => self.dir(Dir::PageUp),
            KeyAction::PageDown => self.dir(Dir::PageDown),
            KeyAction::Home => self.dir(Dir::Home),
            KeyAction::End => self.dir(Dir::End),
            KeyAction::Activate => self.activate(store, self.cursor),
            KeyAction::ActivateAlternate => self.activate_alternate(store, self.cursor),
            KeyAction::Submit => {
                let q = self.query.text().to_owned();
                self.complete_select(&q);
                Out::Close
            }
            KeyAction::Clear => {
                self.set_query(store, "");
                Out::None
            }
            KeyAction::Pop => self.pop(store),
            KeyAction::NextSection => self.section_step(true),
            KeyAction::PrevSection => self.section_step(false),
            KeyAction::Close => {
                self.close();
                Out::Close
            }
            _ => Out::None,
        };
        self.dirty = true;
        out
    }

    /// A key the field takes.
    fn edit(&mut self, store: &Store, edit: field::Key, mods: field::Modifiers, text: Option<&str>) -> Out {
        match self.query.key(edit, mods, text) {
            field::Outcome::Pass => return Out::None,
            field::Outcome::Held => {}
            field::Outcome::Edited => self.query_changed(store),
            field::Outcome::Copy(t) => clipboard::command(clipboard::Cmd::SetText(t)),
            field::Outcome::Cut(t) => {
                clipboard::command(clipboard::Cmd::SetText(t));
                self.query_changed(store);
            }
            field::Outcome::Paste => clipboard::command(clipboard::Cmd::Paste),
        }
        self.dirty = true;
        Out::None
    }

    fn dir(&mut self, d: Dir) -> Out {
        self.move_cursor(d);
        Out::None
    }

    /// `debug query`: the live tree ranked, whether or not the window is up:
    /// inside the mode a leading prefix names, else the open launcher's.
    pub fn query(&self, store: &Store, q: &str) -> Value {
        let nodes = store.menu.nodes();
        let (level, q) = match model::prefixed(nodes, q) {
            Some((node, rest)) => (Some(node.id.as_str()), rest),
            None => (self.level.as_deref().filter(|_| self.open), q),
        };
        let noted = |rows: Vec<Node>| -> Value {
            Value::Array(rows.iter().map(|n| json!({"id": n.id, "label": n.label, "desc": n.desc.clone().unwrap_or_default(), "kind": n.kind.as_str()})).collect())
        };
        match level {
            Some("emoji") => {
                let Some(index) = &store.menu.emoji else {
                    index::ask(Ask::Emoji);
                    return json!([]);
                };
                return Value::Array(index.rows(q, true, &store.state.data.emoji_uses, None).iter().map(|n| json!({"id": n.id, "label": n.label, "icon": n.icon, "kind": n.kind.as_str()})).collect());
            }
            Some("keybinds") => {
                index::ask(Ask::Binds);
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                return noted(fs_menu::keybinds::rows_for(reply, q));
            }
            Some("radio.search") => return noted(radio_search_rows(store, q)),
            Some(fs_menu::providers::INSTALL_ROUTE) => return noted(crate::surfaces::packages::install_rows(store, q)),
            _ => {}
        }
        let snap = snapshot(store);
        let book = index::abbreviation_book(store);
        let lookup = fs_menu::abbrev::lookup(&configured_abbreviations(store), &book, q, now_ms(), |id| resolves(store, id));
        let boost = fs_menu::search::Boost { abbrev: Some(&lookup), picks: &store.state.data.picks, now: now_ms() };
        let mut rows: Vec<Value> = fs_menu::search::rank_with(nodes, q, &store.menu.cond, level, boost)
            .into_iter()
            .map(|n| {
                json!({
                    "id": n.id, "label": n.label, "kind": n.kind.as_str(), "iconSource": n.icon_source,
                    "checked": toggles::checked_for(Some(n), Some(&snap), Some(&store.menu.checked))
                })
            })
            .collect();
        if let Some(calc) = level.is_none().then(|| fs_menu::calc::result_node(q)).flatten() {
            rows.insert(0, json!({"id": calc.id, "label": calc.label, "kind": calc.kind.as_str()}));
        }
        Value::Array(rows)
    }
}

pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

/// `menu.abbreviations` in settings.json, query to subject id.
pub fn configured_abbreviations(store: &Store) -> std::collections::HashMap<String, String> {
    match store.config.get("menu.abbreviations") {
        Some(Value::Object(map)) => {
            map.iter().filter_map(|(q, id)| Some((fs_menu::abbrev::key(q)?, id.as_str()?.to_owned()))).collect()
        }
        _ => Default::default(),
    }
}

/// Whether a subject id names something now: in the tree, or a file still
/// on disk, or an emoji in the dataset.
fn resolves(store: &Store, id: &str) -> bool {
    model::subject_node(store.menu.nodes(), id).is_some() || outside_subject(store, id).is_some()
}

/// A subject id naming no tree node, as a row: a file or folder still on
/// disk, an emoji in the dataset. Windows are left out: their ids do not
/// outlive the window.
fn outside_subject(store: &Store, id: &str) -> Option<Node> {
    match SubjectId::parse(id) {
        SubjectId::File(path) => {
            // Read off the service's stat of the picks, never the disk here.
            let dir = (*store.menu.files.get(&path)?)?;
            Some(fs_menu::providers::recent_subject(&path, dir, &std::env::var("HOME").unwrap_or_default()))
        }
        SubjectId::Emoji(ch) => store.menu.emoji.as_ref()?.row_for(&ch, crate::settings::CLIPBOARD_PASTE.get(&store.config)),
        SubjectId::App(_) | SubjectId::Node(_) => None,
    }
}

/// The compositor's windows an app's entry claims.
fn app_windows(store: &Store, entry: &fs_menu::node::DesktopEntry) -> Vec<WindowId> {
    fs_menu::appmatch::match_windows(Some(entry), &store.compositor.snapshot.windows)
}

/// An app subject's windows as rows, each a window subject: its title
/// over the app's icon, its workspace trailing.
fn window_rows(store: &Store, subject: &str, q: &str) -> Vec<Node> {
    let Some(app) = model::subject_node(store.menu.nodes(), subject) else { return Vec::new() };
    let Some(entry) = &app.entry else { return Vec::new() };
    let snap = &store.compositor.snapshot;
    let q = q.trim().to_lowercase();
    app_windows(store, entry)
        .iter()
        .filter_map(|id| store.compositor.window(id))
        .filter(|w| q.is_empty() || w.title.to_lowercase().contains(&q))
        .map(|w| Node {
            path: w.id.to_string(),
            icon: app.icon.clone(),
            icon_source: app.icon_source.clone(),
            meta: snap.workspaces.iter().find(|ws| ws.id == w.workspace_id).map(|ws| ws.name.clone()),
            ..Node::new(
                format!("window:{}", w.id),
                if w.title.is_empty() { app.label.clone() } else { w.title.clone() },
                NodeKind::Window,
            )
        })
        .collect()
}

/// A folder stepped into: its entries as subjects, a note while the
/// service reads it or when it cannot, or when it holds nothing.
fn folder_rows(store: &Store, path: &str, q: &str) -> Vec<Node> {
    match store.menu.folder.as_ref().filter(|(p, _)| p == path).map(|(_, l)| l) {
        None => vec![Node::note("files.reading", "Reading folder")],
        Some(Err(why)) => vec![Node::note("files.unreadable", why.clone())],
        Some(Ok(entries)) => {
            let rows = fs_menu::providers::listing_rows(path, entries, q);
            if rows.is_empty() && q.is_empty() { vec![Node::note("files.empty", "Empty folder")] } else { rows }
        }
    }
}

/// LocalSend's peers off its last scan, each trailing its address; with
/// none, one note saying why.
fn peer_targets(store: &Store) -> Vec<Node> {
    let ls = &store.localsend;
    if !ls.installed {
        return vec![Node::note("peer:none", "LocalSend is not installed")];
    }
    let peers: Vec<Node> = ls
        .peers
        .iter()
        .enumerate()
        .map(|(i, p)| Node { meta: Some(p.ip.clone()), ..Node::new(format!("peer:{i}"), p.name.clone(), NodeKind::Target) })
        .collect();
    match (peers.is_empty(), ls.scanning || ls.last_scan_at == 0.0) {
        (false, _) => peers,
        (true, true) => vec![Node::note("peer:scanning", "Looking for devices\u{2026}")],
        (true, false) => vec![Node::note("peer:none", "No devices found")],
    }
}

/// What a target action picks from. Workspaces: the focused output's, by
/// slot, then the next free one as a new one. Apps: every installed one,
/// by name. Peers: LocalSend's last scan, which a listing asks again.
fn targets_for(store: &Store, kind: TargetKind) -> Vec<Node> {
    match kind {
        TargetKind::App => {
            let mut apps: Vec<Node> = model::visible_children(store.menu.nodes(), Some("apps"), &store.menu.cond)
                .into_iter()
                .filter_map(|n| n.entry.as_ref().map(|e| Node::new(format!("target-app:{}", e.id), n.label.clone(), NodeKind::Target)))
                .collect();
            apps.sort_by_key(|n| n.label.to_lowercase());
            apps
        }
        TargetKind::Peer => {
            use crate::services::localsend;
            localsend::command(localsend::Cmd::Scan { force: false });
            peer_targets(store)
        }
        TargetKind::Workspace => {
            let snap = &store.compositor.snapshot;
            let output = &snap.focused_output_name;
            let mut slots: Vec<(i64, String)> = snap
                .workspaces
                .iter()
                .filter(|w| w.idx >= 1 && (output.is_empty() || w.output == *output))
                .map(|w| (w.idx, w.name.clone()))
                .collect();
            slots.sort_by_key(|(idx, _)| *idx);
            slots.dedup_by_key(|(idx, _)| *idx);
            // One past every output's: Hyprland's idx is global, so one past
            // this output's alone can name another output's workspace.
            let next = snap.workspaces.iter().map(|w| w.idx).max().unwrap_or(0).max(0) + 1;
            let row = |idx: i64, meta: Option<String>| Node {
                meta,
                ..Node::new(format!("workspace:{idx}"), format!("Workspace {idx}"), NodeKind::Workspace)
            };
            let mut out: Vec<Node> = slots.into_iter().map(|(idx, name)| row(idx, (!name.is_empty() && name != idx.to_string()).then_some(name))).collect();
            out.push(row(next, Some("New".into())));
            out
        }
    }
}

pub fn selection_path() -> std::path::PathBuf {
    state::state_path().with_file_name("menu-selection.txt")
}

fn row_height(node: &Node, theme: &Theme, kit: &mut Kit) -> f64 {
    let s = &theme.space;
    let row = launcher_px(fs_theme::tokens::LAUNCHER.row_height);
    if node.emoji_only {
        let h = ui::measure(&w::text("Ag").size(Type::Display), 100.0, theme, kit).1;
        return row.max(h + s.control_padding_y * 2.0);
    }
    row
}

/// A clipboard entry's glyph by what it holds, as Pond marks them: the
/// picture itself shows in the pane, never in the row.
fn clip_glyph(row: &Node) -> Option<&'static str> {
    let clip = fs_menu::providers::clipboard_entry_id(row).is_some() || row.id.starts_with("share.history.");
    if !clip || row.emoji_only {
        return None;
    }
    let text = row.full_text.trim();
    Some(if !row.thumb_source.is_empty() {
        "image"
    } else if !text.contains(char::is_whitespace) && (text.starts_with("http://") || text.starts_with("https://") || text.starts_with("www.")) {
        "link"
    } else {
        "text"
    })
}

/// A section heading over the rows: the panels' `caption` 600 in `muted`
/// (panel-language spec §5), on the rows' text column.
fn heading_el(s: &fs_theme::tokens::Space, text: &str) -> El {
    w::text(text.to_owned()).size(Type::Caption).weight(Weight::Semibold).ink(Ink::Muted).pad_start(s.control_padding_x)
}

/// A list row's words, the label, description and value alike, at Pond's
/// one row size.
fn row_words(text: String) -> El {
    w::text(text).size(Type::Row).elide()
}

/// A `LAUNCHER` length in whole pixels.
fn launcher_px(base: f64) -> f64 {
    base.round()
}

/// The window: the card, its surface and the three bands' widgets.
pub struct Shown {
    pub modal: Modal,
    head: Ui,
    body: Ui,
    preview: Ui,
    /// The mirror's feed box and the picture inside it, on the surface.
    pub feed: Option<(IRect, Option<IRect>)>,
    /// The body's viewport on the output, for `menu status`.
    pub body_rect: Option<IRect>,
    foot: Ui,
    rules: Vec<NodeId>,
    pub output: (f64, f64),
    scale: f64,
    pub wake: Option<Instant>,
    /// The card's top, settled: the scroll's own animation.
    scroll: Animated,
    /// The actions column's own, and its box on the surface while it is
    /// drawn.
    column_scroll: Animated,
    column_rect: Option<Rect>,
    /// The field band over the rule, which the token and the query centre in.
    field_rect: Option<Rect>,
    /// The preview pane's box on the surface while it is drawn, and when
    /// it last arrived.
    pane_rect: Option<Rect>,
    pane_since: Option<Instant>,
    /// The card's width and height travelling between level kinds.
    morph: (Animated, Animated),
    /// The level and halves the card was last sized for, and the query.
    sized_for: Option<(SizeKey, String)>,
    /// The card size the content was last laid out for.
    laid_for: (f64, f64),
    /// The view the content was last laid out for.
    laid_view: Option<ViewId>,
    /// The cursor's fill and ring, one for the whole body, painted between
    /// `sel_anchor` and the body's own nodes so the cells draw over it.
    sel_anchor: NodeId,
    sel_nodes: Vec<NodeId>,
    /// Its box in body content coordinates (x, y, w, h), travelling.
    sel: [Animated; 4],
    /// The level and query it was last drawn on, while it is shown: a move
    /// inside the same list travels, anything else lands at once.
    sel_on: Option<(Option<String>, String)>,
    /// The card centred both ways, as Pond's settings is.
    centred: bool,
    /// The embedded panel's content top on the surface, scroll included.
    panel_origin: f64,
}

impl Shown {
    /// The launcher's deform amount, lighter than a popout card's.
    pub const DEFORM_AMOUNT: f64 = 0.1;

    pub fn new(theme: &Theme, mut modal: Modal, output: (f64, f64), scale: f64) -> Self {
        let top = modal.layer.as_ref().map_or(modal.win.card.top_node(), |l| l.top);
        let scene = match &mut modal.layer {
            Some(l) => &mut l.scene,
            None => &mut modal.win.card.scene,
        };
        let mut marker = |prev: NodeId| {
            let id = scene.add_after(Some(prev), IRect::default(), crate::scene::Paint::Rect { fill: fs_theme::color::Rgba::TRANSPARENT, radius: 0.0 });
            scene.set_visible(id, false);
            id
        };
        let sel_anchor = marker(top);
        let body_anchor = marker(sel_anchor);
        let sel = || Animated::new(0.0, Clock::SpatialFast.curve());
        Self {
            modal,
            head: Ui::new(Some(top)),
            body: Ui::new(Some(body_anchor)),
            preview: Ui::new(Some(top)),
            feed: None,
            body_rect: None,
            foot: Ui::new(Some(top)),
            rules: Vec::new(),
            output,
            scale,
            wake: None,
            scroll: Animated::new(0.0, Clock::SpatialFast.curve()),
            column_scroll: Animated::new(0.0, Clock::SpatialFast.curve()),
            column_rect: None,
            field_rect: None,
            pane_rect: None,
            pane_since: None,
            laid_for: (0.0, 0.0),
            laid_view: None,
            morph: (Animated::new(theme.space.popup_width_menu, Clock::Spatial.curve()), Animated::new(0.0, Clock::Spatial.curve())),
            sized_for: None,
            sel_anchor,
            sel_nodes: Vec::new(),
            sel: [sel(), sel(), sel(), sel()],
            sel_on: None,
            centred: false,
            panel_origin: 0.0,
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.modal.win.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.modal.win.animating(now)
            || self.scroll.running(now)
            || self.column_scroll.running(now)
            || self.morph.0.running(now)
            || self.morph.1.running(now)
            || self.sel.iter().any(|a| a.running(now))
            || self.wake.is_some_and(|w| w <= now)
    }

    /// Whether the content itself moves, past the card carrying it: on a
    /// layer, the card's own motion and its size morph need no new layout,
    /// the compositor moves and cuts the layer.
    pub fn content_animating(&self, now: Instant) -> bool {
        let card = self.modal.layer.is_none() && (self.modal.win.animating(now) || self.morph.0.running(now) || self.morph.1.running(now));
        card || self.scroll.running(now) || self.column_scroll.running(now) || self.wake.is_some_and(|w| w <= now)
    }

    /// The field's line, the head band's height under the card's top and
    /// the foot band's, each band with its rule.
    fn metrics(theme: &Theme, m: &Model, store: &Store, output_h: f64, kit: &mut Kit) -> (f64, f64, f64, f64) {
        let s = &theme.space;
        let line = ui::measure(&w::text("Ag").size(Type::Field), 100.0, theme, kit).1.max(s.control_height);
        // A held page wears a panel card's frame: its header row, hairline
        // and the room under its body.
        let page = m.on_panel().then(|| crate::surfaces::panel::Frame::new(theme, true));
        let head = match page {
            Some(f) => f.body(),
            None => launcher_px(fs_theme::tokens::LAUNCHER.field_band) + theme.border_width,
        };
        let foot = match page {
            Some(f) => f.foot + m.layout.close,
            None if m.footless() => 0.0,
            None => theme.border_width + s.control_height,
        };
        let cap = (m.kind_height(theme, output_h) - head - foot).max(0.0);
        let body = if m.mode == Mode::Input {
            0.0
        } else if m.on_panel() {
            ui::fit_rows(&m.layout.row_ends, m.layout.content_h, cap)
        } else if m.content_tall() {
            // The card hugs its list.
            let pane = match m.pane() {
                Some(row) => {
                    let (_, pw, _) = m.pane_extent(theme);
                    let inner = (cap - s.lg * 2.0).max(0.0);
                    ui::measure(&preview_el(m, store, row, theme, kit, pw, inner), pw, theme, kit).1.min(inner) + s.lg * 2.0
                }
                None => 0.0,
            };
            // An empty level has no rows to hug: room for its words, a
            // control height either side.
            let content = if m.rows.is_empty() {
                ui::measure(&empty_words(m, s), m.body_width(theme), theme, kit).1 + s.control_height * 2.0
            } else {
                m.layout.content_h
            };
            let tall = content.max(pane);
            if tall > cap && pane == 0.0 && m.view == View::Rows { m.whole_rows(cap, s.lg) } else { tall.min(cap) }
        } else {
            cap
        };
        (line, head, foot, body)
    }

    /// The stop under the pointer, whichever of its parts it is on, as the
    /// panel host reads it.
    pub fn hovered_stop(&self) -> Option<String> {
        let path = self.body.hover.as_ref()?;
        let hit = self.body.hits.iter().find(|h| &h.path == path)?;
        if hit.stop.is_some() {
            return hit.stop.clone();
        }
        let r = hit.rect;
        self.body
            .hits
            .iter()
            .filter(|h| h.stop.is_some() && h.rect.x <= r.x && h.rect.y <= r.y && h.rect.right() >= r.right() && h.rect.bottom() >= r.bottom())
            .min_by_key(|h| h.rect.w * h.rect.h)
            .and_then(|h| h.stop.clone())
    }

    /// The embedded panel's body, ending as far above the card's edge as
    /// its last thing stands off the sides (panel-language spec §2).
    fn panel_el(&self, m: &Model, store: &Store, theme: &Theme) -> Option<El> {
        let p = m.panel.as_ref().filter(|_| m.on_panel())?;
        let v = crate::surfaces::panel::View {
            store,
            theme,
            output: self.output,
            cursor: p.cursor.shown(),
            hovered: self.hovered_stop(),
            now: Instant::now(),
            motion_scale: self.scale,
            raster: self.modal.win.surface.scale(),
        };
        Some(p.module.body(&v))
    }

    fn measure_panel(&self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit) {
        let Some(p) = m.panel.as_mut() else { return };
        let v = crate::surfaces::panel::View {
            store,
            theme,
            output: self.output,
            cursor: None,
            hovered: None,
            now: Instant::now(),
            motion_scale: self.scale,
            raster: self.modal.win.surface.scale(),
        };
        p.width = crate::surfaces::panel::card_width(p.module.as_ref(), p.id.desc().width, &v, kit);
        let Some(el) = self.panel_el(m, store, theme) else { return };
        let width = m.panel_body(theme).1;
        m.layout.content_h = ui::measure(&el, width, theme, kit).1;
        m.layout.row_ends = ui::row_ends(&el, width, theme, kit);
        m.layout.close = w::closing_room(&theme.space, &el);
    }

    /// The body's stops, for the embedded panel's cursor.
    pub fn stops(&self) -> &[ui::Stop] {
        &self.body.stops
    }

    /// The smallest scroll that shows the embedded panel's cursor stop.
    pub fn follow_panel(&self, m: &mut Model) {
        let Some(st) = m.panel.as_ref().and_then(|p| p.cursor.shown()).and_then(|k| self.body.stops.iter().find(|st| st.key == k)) else { return };
        let top = st.rect.y as f64 - self.panel_origin;
        let (h, view) = (st.rect.h as f64, m.body_h);
        let mut next = m.scroll;
        if top < next {
            next = top;
        } else if top + h > next + view {
            next = top + h - view;
        }
        m.scroll = next.clamp(0.0, (m.layout.content_h - view).max(0.0));
        m.travels = true;
        m.wheeled = false;
        m.touch.settle();
    }

    /// The card at this frame of its size morph. Off a top bar it is
    /// centred across the output, its top 30% down. Off a side bar it is
    /// centred both ways in the area the bar leaves: the windows tile in
    /// that area, so its centre is where the eye reads the screen's centre
    /// beside a 160px column. On a layer that is all a frame of the morph
    /// does: the content stays laid out at the size the card is headed for.
    pub fn place_card(&mut self, theme: &Theme, now: Instant) {
        let (w_, h_) = (self.morph.0.value(now).round(), self.morph.1.value(now).round());
        let pad = theme.space.panel_padding;
        let near = |extent: f64, size: f64| {
            let max = extent - size - pad;
            if max < pad { pad } else { (extent * 0.3).clamp(pad, max) }.round()
        };
        let centred = |extent: f64, size: f64| ((extent - size) / 2.0).max(pad).round();
        let line = self.modal.win.card.line_at();
        let (x, y) = match self.modal.win.card.edge {
            Edge::Left => (line + centred(self.output.0 - line, w_), centred(self.output.1, h_)),
            Edge::Right => (centred(self.output.0 - line, w_), centred(self.output.1, h_)),
            _ if self.centred => (((self.output.0 - w_) / 2.0).round(), centred(self.output.1, h_)),
            _ => (((self.output.0 - w_) / 2.0).round(), near(self.output.1, h_)),
        };
        self.modal.place(Rect::new(x, y, x + w_, y + h_), now);
    }

    /// Lays the card out and draws the three bands.
    pub fn layout(&mut self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        m.output_w = self.output.0;
        m.relayout(theme, kit);
        self.centred = m.on_panel();
        if m.on_panel() {
            self.measure_panel(m, store, theme, kit);
        }
        let (line, head_h, foot_h, body_h) = Self::metrics(theme, m, store, self.output.1, kit);
        let chrome = head_h + foot_h;
        if (m.body_h - body_h).abs() > 0.5 {
            m.body_h = body_h;
            m.follow();
        }
        // A new level, or the column or pane taking half the card, travels
        // into its size. A re-rank inside a level lands at once, a typed
        // query's above all: the list for it is whole on the next frame.
        // The first frame after an open lands on it too.
        let (tw, th) = (m.card_width(theme), chrome + body_h);
        self.modal.look_hint(tw.round() as i32, th.round() as i32);
        let key = m.size_key();
        // The query the list is ranked by: the actions column takes the
        // field over and gives it back on close, which is no typing.
        let query = m.list_text().to_owned();
        let typed = self.sized_for.as_ref().is_some_and(|(k, q)| k.level == key.level && *q != query);
        let resized = self.sized_for.as_ref().is_some_and(|(k, _)| *k != key);
        let morphing = self.morph.0.running(now) || self.morph.1.running(now);
        self.sized_for = Some((key, query));
        if self.modal.win.surface.mapped() && self.modal.open && !typed && (resized || morphing) {
            let ms = Clock::Spatial.ms(theme) * self.scale;
            self.morph.0.set(now, tw, ms);
            self.morph.1.set(now, th, ms);
        } else {
            self.morph.0.jump(tw);
            self.morph.1.jump(th);
        }
        self.place_card(theme, now);
        if let Some(coast) = m.touch.tick(now, m.max_scroll()) {
            m.scroll = coast;
            m.travels = false;
            m.wheeled = false;
        }
        glide(&mut self.scroll, now, m.scroll, m.wheeled, m.travels, theme, self.scale);
        match &mut m.column {
            Some(c) => {
                c.row_h = launcher_px(fs_theme::tokens::LAUNCHER.row_height);
                c.view_h = (body_h - theme.space.sm * 2.0).max(0.0);
                c.scroll = c.scroll.min(c.max_scroll());
                if let Some(coast) = c.touch.tick(now, c.max_scroll()) {
                    c.scroll = coast;
                    c.wheeled = false;
                    c.travels = false;
                }
                glide(&mut self.column_scroll, now, c.scroll, c.wheeled, c.travels, theme, self.scale);
            }
            None => self.column_scroll.jump(0.0),
        }
        self.draw(m, store, theme, kit, now, line, body_h, (tw, th));
        if let Some(p) = m.panel.as_mut() {
            p.cursor.sync(&self.body.stops);
            p.cursor.travels = false;
        }
        self.laid_for = (tw, th);
        self.laid_view = Some(m.view_id());
    }

    /// On a layer, the card is morphing toward a size its content is
    /// already laid out for: another layout (a monitor refresh, rows
    /// settling) waits for the morph to land, since on a starved CPU one
    /// layout of a long view outlasts several frames of the morph. A key,
    /// click or call that moved the view is never held.
    pub fn hold_layout(&self, m: &Model, now: Instant) -> bool {
        let morphing = self.morph.0.running(now) || self.morph.1.running(now);
        morphing
            && self.modal.layer.is_some()
            && self.laid_for == (self.morph.0.target(), self.morph.1.target())
            && self.laid_view.as_ref() == Some(&m.view_id())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw(&mut self, m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant, line: f64, body_h: f64, target: (f64, f64)) {
        // On a layer the content is laid out in the layer's own pixels at
        // the size the card is headed for: the compositor moves, cuts and
        // fades it with the card.
        let (frame, alpha, clip) = match &self.modal.layer {
            Some(_) => {
                let size = IRect::new(0, 0, target.0.round() as i32, target.1.round() as i32);
                (size, 1.0, size)
            }
            None => (self.modal.win.card.content.0, self.modal.win.card.content.1, self.modal.win.card.content_clip()),
        };
        let s = theme.space.clone();
        let pad = s.panel_padding;
        let panel = self.panel_el(m, store, theme);
        let (fx, fy, fw) = (frame.x as f64, frame.y as f64, frame.w as f64);
        let inner_w = (fw - pad * 2.0).max(0.0);
        self.wake = None;
        // Where the layer's pixels sit on the output once settled, for the
        // rects reported out.
        if let Some(l) = &mut self.modal.layer {
            l.fit(frame.w, frame.h);
        }
        let on_output = match &self.modal.layer {
            Some(l) => { let r = l.place(&self.modal.win.card, true); (r.x, r.y) }
            None => (0, 0),
        };
        let scene = match &mut self.modal.layer {
            Some(l) => &mut l.scene,
            None => &mut self.modal.win.card.scene,
        };
        self.head.motion_scale = self.scale;
        self.body.motion_scale = self.scale;
        self.foot.motion_scale = self.scale;

        // The field band: the line centred in it `xxl` in from either side,
        // a rule as far in under it. A held page's band is a panel card's
        // header, row and hairline where the panel host draws them.
        let page = m.on_panel().then(|| crate::surfaces::panel::Frame::new(theme, true));
        let (band, row_top, row_h) = match page {
            Some(f) => (f.rule(theme), fy + f.top, f.head),
            None => {
                let band = launcher_px(fs_theme::tokens::LAUNCHER.field_band);
                (band, fy + ((band - line) / 2.0).round(), line)
            }
        };
        let field_x = m.field_inset(theme);
        let head_rect = Rect::new(fx + field_x, row_top, fx + fw - field_x, row_top + row_h);
        // The row stands the band's height whatever it holds, so a token
        // with no query beside it centres in the band too.
        let head = header(m, store, theme, self.modal.win.surface.scale()).height(row_h);
        self.field_rect = Some(Rect::new(fx, fy, fx + fw, fy + band));
        let d = self.head.draw(&head, head_rect, Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }
        let bw = theme.border_width.round().max(1.0) as i32;
        let rule_top = (fy + band).round() as i32;
        let body_top = match page {
            Some(f) => fy + f.body(),
            None => rule_top as f64 + theme.border_width,
        };
        let (body_x, body_w) = if m.on_panel() {
            let (x, w_) = m.panel_body(theme);
            (fx + x, w_)
        } else if m.bleeds() {
            (fx, fw)
        } else {
            (fx + pad, inner_w)
        };
        let sep = theme.box_style(Role::Separator, None).fill;
        let sep = sep.with_alpha(sep.a * alpha);

        // The body.
        let viewport = IRect::new(body_x.round() as i32, body_top.round() as i32, body_w.round() as i32, body_h.round() as i32);
        let rows_end = IRect::new(viewport.x, viewport.y, viewport.w, (body_h - m.layout.foot).max(0.0).round() as i32);
        let body_clip = rows_end.intersect(&clip);
        let view_clip = viewport.intersect(&clip);
        self.body_rect = (body_h > 0.0).then(|| IRect::new(viewport.x + on_output.0, viewport.y + on_output.1, viewport.w, viewport.h));
        let scroll = self.scroll.value(now) + m.touch.value(now);
        let body = match panel {
            Some(el) => el,
            None if body_h > 0.0 => body_el(m, store, theme, kit, scroll, body_h),
            None => w::space(0.0),
        };
        let held = m.panel.as_ref().filter(|_| m.on_panel());
        self.body.halo_owned = false;
        self.body.fill_owned = held.is_none();
        self.body.cursor = held.and_then(|p| p.cursor.shown()).map(str::to_owned);
        self.body.ring = held.is_some_and(|p| p.cursor.keyed);
        let origin = (body_x, body_top - scroll);
        self.panel_origin = origin.1;
        let d = self.body.draw(&body, Rect::new(origin.0, origin.1, origin.0 + body_w, origin.1 + m.layout.content_h), Some(body_clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        // The cursor's fill travels between cells on `spatialFast` (DESIGN.md
        // §1 "Motion"), keyboard and pointer alike; a wrap, a filter and a
        // level change land at once.
        let id = m.rows.get(m.cursor).map(|r| r.id.as_str());
        let stop = id.and_then(|id| self.body.stops.iter().find(|st| st.key.split_once(':').is_some_and(|(_, k)| k == id))).cloned();
        let mut p = Painter::new(scene, &mut self.sel_nodes, Some(body_clip)).after(Some(self.sel_anchor));
        match stop {
            Some(st) => {
                let on = (m.level.clone(), m.query.text().to_owned());
                let travels = self.sel_on.as_ref() == Some(&on) && (m.travels || !m.from_keys);
                let local = [st.rect.x as f64 - origin.0, st.rect.y as f64 - origin.1, st.rect.w as f64, st.rect.h as f64];
                let ms = Clock::SpatialFast.ms(theme) * self.scale;
                for (a, t) in self.sel.iter_mut().zip(local) {
                    if travels { a.set_on(now, t, ms, Clock::SpatialFast.curve()) } else { a.jump(t) }
                }
                self.sel_on = Some(on);
                let [x, y, w_, h] = [0, 1, 2, 3].map(|i| self.sel[i].value(now));
                let r = ui::irect(Rect::new(origin.0 + x, origin.1 + y, origin.0 + x + w_, origin.1 + y + h));
                let b = if matches!(m.view, View::Rows | View::AppGrid | View::Emoji) {
                    theme.box_style(Role::LauncherRow, Some("selected"))
                } else {
                    let mut b = theme.box_style(Role::Cell, Some("selected"));
                    b.border = None;
                    b.wash = None;
                    theme.with_cursor(b, m.from_keys, true)
                };
                crate::ui::boxes::paint(&mut p, r, &b, st.radius, alpha, 0.0);
                if self.sel.iter().any(|a| a.running(now)) {
                    self.wake = Some(now);
                }
            }
            None => self.sel_on = None,
        }
        p.finish();
        self.feed = (m.view == View::Mirror).then(|| {
            let (w, h) = m.mirror_box(theme);
            let feed = IRect::new((fx + pad + s.sm).round() as i32, (body_top + s.md).round() as i32, w as i32, h as i32);
            let pic = store.mirror.frame.as_ref().filter(|(id, _)| *id == m.mirror_current).map(|(_, b)| {
                let (bw, bh) = b.size();
                IRect::new(feed.x + (feed.w - bw) / 2, feed.y + (feed.h - bh) / 2, bw, bh)
            });
            let out = |r: IRect| IRect::new(r.x + on_output.0, r.y + on_output.1, r.w, r.h);
            (out(feed), pic.map(out))
        });
        // The actions column over the right half, else the selected
        // subject's preview there.
        self.column_rect = None;
        self.pane_rect = None;
        if let Some(c) = m.column.as_ref().filter(|_| body_h > 0.0) {
            let half = (body_w * m.list_share(theme)).round();
            let r = Rect::new(body_x + half, body_top, body_x + body_w, body_top + body_h);
            let scroll = self.column_scroll.value(now) + c.touch.value(now);
            let (el, settling) = column_el(m, theme, scroll, now, self.scale);
            let clip = ui::irect(r).intersect(&view_clip);
            let d = self.preview.draw(&el, Rect::new(r.x0, r.y0 - scroll, r.x1, r.y1 - scroll), Some(clip), alpha, theme, kit, scene, now);
            if d.animating || settling {
                self.wake = Some(now);
            }
            self.column_rect = Some(r);
            self.pane_since = None;
        } else if let Some(row) = m.pane().filter(|_| body_h > 0.0) {
            let (dx, pw, ph) = m.pane_extent(theme);
            let r = Rect::new(body_x + dx, body_top + s.lg, body_x + dx + pw, body_top + s.lg + ph);
            let el = preview_el(m, store, row, theme, kit, r.width(), r.height());
            // The pane's arrival fades its content in on `effects` while the
            // card widens, as the actions column's does.
            let since = *self.pane_since.get_or_insert(now);
            let ms = Clock::Effects.ms(theme) * self.scale;
            let t = crate::motion::progress(since, ms, now);
            let el = if t < 1.0 { el.fade(Clock::Effects.curve().ease(t) as f32) } else { el };
            let d = self.preview.draw(&el, r, Some(view_clip), alpha, theme, kit, scene, now);
            if d.animating || t < 1.0 {
                self.wake = Some(now);
            }
            self.pane_rect = Some(r);
        } else {
            self.pane_since = None;
            self.preview.draw(&w::space(0.0), Rect::new(0.0, 0.0, 0.0, 0.0), Some(body_clip), alpha, theme, kit, scene, now);
        }

        // The foot band: the selected subject's keys under the list, on the
        // rows' inset, its rule as short as the field's.
        let foot_rule = (body_top + body_h).round() as i32;
        let foot_top = if body_h > 0.0 { foot_rule as f64 + theme.border_width } else { body_top };
        let foot = if m.footless() { w::space(0.0) } else { footer(m, store, theme) };
        let d = self.foot.draw(&foot, Rect::new(fx + s.lg, foot_top, fx + fw - s.lg, foot_top + s.control_height), Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        let mut p = Painter::new(scene, &mut self.rules, Some(clip));
        let inset = field_x.round() as i32;
        p.rect(IRect::new(frame.x + inset, rule_top, (frame.w - inset * 2).max(0), bw), sep, 0.0);
        if body_h > 0.0 && !m.footless() {
            p.rect(IRect::new(frame.x + inset, foot_rule, (frame.w - inset * 2).max(0), bw), sep, 0.0);
        }
        // Pond's divider between the list and the side column, `lg` short
        // of the body's ends.
        if let Some(r) = self.column_rect.or(self.pane_rect.map(|_| {
            let half = (body_w * m.list_share(theme)).round();
            Rect::new(body_x + half, body_top, body_x + body_w, body_top + body_h)
        })) {
            let top = (body_top + s.lg).round() as i32;
            p.rect(IRect::new(r.x0.round() as i32, top, bw, ((body_h - s.lg * 2.0).round() as i32).max(0)), sep, 0.0);
        }
        p.finish();
    }

    /// What is under the pointer: a body row or cell, a header or footer
    /// control.
    pub fn hit(&self, x: f64, y: f64) -> Option<crate::ui::Hit> {
        let (x, y) = self.local(x, y);
        self.body.hit(x, y).or_else(|| self.preview.hit(x, y)).or_else(|| self.head.hit(x, y)).or_else(|| self.foot.hit(x, y)).cloned()
    }

    /// What a click of `button` at an output point means to the held
    /// panel's body.
    pub fn panel_event(&self, x: f64, y: f64, button: crate::ui::Button) -> Option<crate::ui::Event> {
        let (x, y) = self.local(x, y);
        Ui::event(self.body.hit(x, y)?, x, button)
    }

    /// An output point in the surface's own pixels.
    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        match &self.modal.layer {
            Some(l) => {
                let at = l.place(&self.modal.win.card, !self.modal.win.animating(Instant::now()));
                (x - f64::from(at.x), y - f64::from(at.y))
            }
            None => (x, y),
        }
    }

    /// A box on the surface, settled, as a box on the output.
    fn on_output(&self, r: IRect) -> IRect {
        let at = self.modal.layer.as_ref().map_or((0, 0), |l| {
            let p = l.place(&self.modal.win.card, true);
            (p.x, p.y)
        });
        IRect::new(r.x + at.0, r.y + at.1, r.w, r.h)
    }

    /// Whether an output point is over the actions column, which a wheel
    /// there scrolls instead of the list.
    pub fn over_column(&self, x: f64, y: f64) -> bool {
        let (x, y) = self.local(x, y);
        self.column_rect.is_some_and(|r| r.contains((x, y)))
    }

    /// The mode token's box on the output, while one is drawn.
    pub fn token_rect(&self) -> Option<IRect> {
        let r = self.head.hits.iter().find(|h| h.on.as_deref() == Some("token"))?.rect;
        Some(self.on_output(r))
    }

    /// The field band on the output.
    pub fn field_box(&self) -> Option<IRect> {
        self.field_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The actions column's box on the output, while it is drawn.
    pub fn column_box(&self) -> Option<IRect> {
        self.column_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The preview pane's box on the output, while it is drawn.
    pub fn pane_box(&self) -> Option<IRect> {
        self.pane_rect.map(|r| self.on_output(ui::irect(r)))
    }

    /// The card's settled box on the output.
    pub fn card_box(&self) -> IRect {
        match &self.modal.layer {
            Some(l) => l.place(&self.modal.win.card, true),
            None => self.modal.win.card.live_rect(),
        }
    }

    /// Every list row drawn, by index, its box on the output cut to what
    /// shows.
    pub fn row_boxes(&self) -> Vec<(usize, IRect)> {
        self.body
            .hits
            .iter()
            .filter_map(|h| Some((h.on.as_deref()?.strip_prefix("row:")?.parse().ok()?, self.on_output(h.rect))))
            .collect()
    }

    /// The foot's buttons on the output, each by what a click on it does.
    pub fn foot_hints(&self) -> Vec<(String, IRect)> {
        self.foot.hits.iter().filter_map(|h| Some((h.on.clone()?, self.on_output(h.rect)))).collect()
    }

    /// What the pointer hovers in the list and in the column: the row's
    /// index and the action's position, as they were last drawn.
    pub fn hovered(&self) -> (Option<usize>, Option<usize>) {
        let on = |ui: &Ui, prefix: &str| {
            let path = ui.hover.as_deref()?;
            ui.hits.iter().find(|h| h.path == path)?.on.as_deref()?.strip_prefix(prefix)?.parse().ok()
        };
        (on(&self.body, "row:"), on(&self.preview, "action:"))
    }

    pub fn set_hover(&mut self, path: Option<String>) -> bool {
        let changed = self.body.hover != path;
        self.body.hover = path.clone();
        self.preview.hover = path.clone();
        self.head.hover = path.clone();
        self.foot.hover = path;
        changed
    }
}

fn header(m: &Model, store: &Store, theme: &Theme, raster: f64) -> El {
    let s = &theme.space;
    let mut parts = Vec::new();
    let token = m.token(store);
    if !token.is_empty() {
        parts.push(w::token(s, &token).on("token"));
        parts.push(w::space(s.sm));
    }
    for word in m.column_tokens() {
        parts.push(w::token(s, &[word]).on("column-token"));
        parts.push(w::space(s.sm));
    }
    // A held panel takes the keys the field would: its page's title stands
    // where the query would be.
    if let Some(p) = m.panel.as_ref().filter(|_| m.on_panel()) {
        let v = crate::surfaces::panel::View { store, theme, output: (0.0, 0.0), cursor: None, hovered: None, now: Instant::now(), motion_scale: 1.0, raster };
        let title = p.module.title(&v);
        if token.last() != Some(&title) {
            parts.push(crate::surfaces::panel::title(title));
        }
        parts.push(w::spacer());
        return w::row(0.0, parts).fill();
    }
    parts.push(w::edit(&m.query, &m.placeholder(store), Type::Field));
    parts.push(w::space(s.lg));
    parts.push(w::icon("search").size(Type::Heading).ink(Ink::Dim));
    w::row(0.0, parts).fill()
}

/// The selected subject's keys and nothing else, right-aligned, each a
/// button doing what its key does.
fn footer(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let node = m.rows.get(m.cursor);
    let mut hints: Vec<(String, Vec<&'static str>, &'static str)> = Vec::new();
    match m.mode {
        Mode::Menu if m.column.is_some() => {
            let label = if m.column_armed().is_some() { "Confirm" } else { "Run" };
            hints.push((label.into(), vec!["Enter"], "column-run"));
            hints.push(("Back".into(), vec!["Shift", "Tab"], "column-back"));
        }
        Mode::Input => hints.push(("Submit".into(), vec!["Enter"], "primary")),
        Mode::Select => {
            if node.is_some() {
                hints.push(("Select".into(), vec!["Enter"], "primary"));
            }
        }
        Mode::Menu => {
            if let Some(n) = node {
                let subject = m.subject(store, n);
                let bar = fs_menu::actions::action_bar(&fs_menu::actions::ActionCtx {
                    mode: m.mode,
                    node,
                    at_root: m.level.is_none(),
                    picker_select: m.picker.select,
                    confirming: m.confirming(&n.id),
                    ..Default::default()
                });
                // A Style preview runs `theme mode`, which a pinned key refuses.
                let mode_tile = m.view == View::Picker && n.kind == NodeKind::WallpaperMode;
                if mode_tile {
                    let key = crate::services::theme::mode_key(store.config.settings());
                    if key != "light" && key != "dark" {
                        hints.push((format!("Set {} mode", n.label.to_lowercase()), vec!["Enter"], "primary"));
                    }
                } else if let Some(p) = bar.primary {
                    hints.push((p.label, vec!["Enter"], "primary"));
                }
                if !subject.inert() {
                    let all = fs_menu::actions::actions_for(&subject);
                    if let Some(second) = all.get(1).filter(|_| !m.confirming(&n.id)) {
                        hints.push((second.label.clone(), vec!["Shift", "Enter"], "alternate"));
                    }
                }
                if m.view == View::Picker && m.picker_sections.len() > 1 {
                    hints.push(("Next section".into(), vec!["Ctrl", "Tab"], "section"));
                }
                if m.view != View::Mirror && !mode_tile {
                    hints.push(("Actions".into(), vec!["Tab"], "actions"));
                }
                if !subject.inert() && m.view == View::Rows && fs_menu::actions::into_action(&subject).is_some() {
                    hints.push(("Into".into(), vec!["Right"], "into"));
                }
            }
        }
    }
    let mut parts = vec![w::spacer()];
    for (label, keys, on) in hints {
        // The mockup's keys: ↵, ⇥ and ⇧ as glyphs rather than words.
        let keys: Vec<&str> = keys
            .iter()
            .map(|k| match *k {
                "Enter" => "enter-glyph",
                "Tab" => "tab-glyph",
                "Shift" => "shift-glyph",
                other => other,
            })
            .collect();
        // Quieter than the rows: Pond's card has nothing at its foot.
        let content = w::row(s.sm, vec![w::text(label).size(Type::Caption).ink(Ink::Dim), w::chord_keys(s, &keys).fade(0.6)]);
        parts.push(w::cell(content).ghost().interactive().cell_state(|c| c.chip = true).on(on));
    }
    // A band a `controlHeight` tall whether or not a hint is in it.
    parts.push(El::new(ui::el::Kind::Swatch { color: fs_theme::color::Rgba::TRANSPARENT, w: 0.0, h: s.control_height, radius: 0.0, border: false }));
    w::row(s.xs, parts).fill()
}

/// The actions column, or the targets one of them asks for: rows of the
/// list's height and inset, the cursor's row filled, faded in on `effects`
/// as it opens, built only where `scroll` puts the viewport. True while
/// that fade runs.
fn column_el(m: &Model, theme: &Theme, scroll: f64, now: Instant, scale: f64) -> (El, bool) {
    let s = &theme.space;
    let Some(c) = &m.column else { return (w::space(0.0), false) };
    let row_h = c.row_h.max(1.0);
    let icon_px = launcher_px(fs_theme::tokens::LAUNCHER.row_icon);
    let armed = m.column_armed();
    let first = (scroll / row_h).floor().max(0.0) as usize;
    let fits = (c.view_h / row_h).ceil() as usize + 1;
    let last = (first + fits).min(c.shown.len());
    let more = c.more(scroll);
    let ms = Clock::Effects.ms(theme) * scale;
    let progress = |since: Instant| crate::motion::progress(since, ms, now);
    let joined = c.joined.map(|(from, at)| (from, progress(at)));
    let mut rows = vec![w::space(first.min(last) as f64 * row_h)];
    let mut drawn_to = last;
    for (at, &i) in c.shown.iter().enumerate().skip(first).take(fits) {
        if let Some((_, n)) = more.filter(|(row, _)| *row == at) {
            rows.push(w::cell(row_words(format!("{n} more")).ink(Ink::Dim)).ghost().height(row_h));
            drawn_to = at + 1;
            break;
        }
        let selected = at == c.cursor;
        let (label, icon, trail, dim, destructive) = match &c.target {
            Some((_, targets)) => {
                let t = &targets[i];
                let meta = t.meta.clone().map(|v| row_words(v).ink(Ink::Dim));
                (t.label.clone(), None, meta, false, false)
            }
            None => {
                let a = &c.actions[i];
                // A destructive label already names what it does (`Uninstall Firefox?`).
                let label = match armed == Some(i) {
                    true if a.destructive => format!("{}?", a.label),
                    true => crate::ui::confirm::ask(&a.label),
                    false => a.label.clone(),
                };
                let trail = matches!(a.run, Run::Into(_)).then(|| w::icon("chevron-right").ink(Ink::Dim));
                (label, a.icon.clone(), trail, a.run == Run::Note, a.destructive)
            }
        };
        let mut lead = Vec::new();
        if let Some(name) = icon {
            let ink = if destructive { Ink::Destructive } else { Ink::Fg };
            lead.push(w::icon(name).ink(ink).width(Size::Px(icon_px)).height(icon_px));
        }
        let mut name = row_words(label);
        if dim {
            name = name.ink(Ink::Dim);
        }
        lead.push(name);
        let mut parts = vec![w::row(s.lg, lead).fill()];
        parts.extend(trail);
        let confirming = armed == Some(i) && c.target.is_none();
        // A plugin's actions fade in under the built-ins as they join.
        let fade = joined.filter(|(from, t)| c.target.is_none() && i >= *from && *t < 1.0).map(|(_, t)| Clock::Effects.curve().ease(t) as f32);
        let row = w::cell(w::row(s.lg, parts).fill())
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| {
                    st.row = true;
                    st.destructive = confirming;
                })
                .on(format!("action:{at}"))
                .key(format!("a{at}"))
                .height(row_h);
        rows.push(match fade {
            Some(f) => row.fade(f),
            None => row,
        });
    }
    if c.shown.is_empty() {
        rows.push(w::text("No matches").ink(Ink::Muted).pad(s.lg, s.md, s.lg, s.md));
    }
    rows.push(w::space(c.shown.len().saturating_sub(drawn_to) as f64 * row_h));
    let t = progress(c.opened);
    let settling = t < 1.0 || joined.is_some_and(|(_, j)| j < 1.0);
    let el = w::column(0.0, rows).pad(s.sm, s.sm, s.sm, s.sm);
    (if t < 1.0 { el.fade(Clock::Effects.curve().ease(t) as f32) } else { el }, settling)
}

/// A level with no rows: its own note, else what the query found nothing
/// for.
fn empty_words(m: &Model, s: &fs_theme::tokens::Space) -> El {
    let title = match &m.empty {
        Some(n) => n.label.clone(),
        None if m.mode == Mode::Input => String::new(),
        None if m.view == View::Picker && !m.query.is_empty() => "No wallpapers match".into(),
        None if !m.query.is_empty() => format!("No results for \u{201C}{}\u{201D}", m.query.text()),
        None => "No results".into(),
    };
    let mut col = vec![w::text(title).ink(Ink::Muted).centred()];
    if let Some(d) = m.empty.as_ref().and_then(|n| n.desc.clone()).filter(|d| !d.is_empty()) {
        col.push(w::caption(d).mono().centred());
    }
    w::column(s.row_gap, col)
}

/// The body's widgets for the slots inside the viewport, the rest as room.
fn body_el(m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, scroll: f64, view_h: f64) -> El {
    let s = &theme.space;
    let lay = &m.layout;
    if m.view == View::Mirror {
        return mirror_body(m, store, theme);
    }
    if m.rows.is_empty() {
        // A card hugging its content was given a control height either side
        // of the words; a fixed one sets them a little above its middle.
        let above = if m.content_tall() { s.control_height } else { (view_h / 2.0 - s.control_height).max(0.0) };
        return w::column(0.0, vec![w::space(above), empty_words(m, s)]);
    }
    let lo = scroll - view_h * 0.5;
    let hi = scroll + view_h * 1.5;
    let visible = |y: f64, h: f64| y + h >= lo && y <= hi;
    let snap = snapshot(store);
    let mut items: Vec<(f64, f64, El)> = Vec::new();
    for (text, y, x, far) in &lay.headings {
        let h = ui::measure(&heading_el(s, text), 400.0, theme, kit).1;
        if !visible(*y, h) {
            continue;
        }
        let heading = heading_el(s, text).pad_start(s.control_padding_x + x);
        if far.is_empty() {
            items.push((*y, h, heading));
        } else {
            // The pictures' own width, so the far words end on the last
            // column's edge.
            let columns = lay.columns.max(1) as f64;
            let grid = columns * lay.cell_w + (columns - 1.0) * s.lg + s.xs;
            let far = w::caption(far.clone()).elide().hug();
            items.push((*y, h, w::row(s.sm, vec![heading.hug(), w::spacer(), far]).width(Size::Px(grid))));
        }
    }
    for (y, line, caption) in &lay.notes {
        let note = w::column(s.row_gap, vec![w::text(line.clone()).ink(Ink::Muted), w::caption(caption.clone())]).pad_start(s.xs);
        items.push((*y, ui::measure(&note, 400.0, theme, kit).1, note));
    }
    if m.monitor_strip_h > 0.0 {
        // The list's half while the actions column holds the other.
        let width = if lay.halved { (lay.body_w * m.list_share(theme)).round() } else { lay.body_w };
        let strip = monitor_strip(store, theme, &m.monitor_history).width(Size::Px(width));
        items.push((lay.slots.first().map_or(s.md, |s0| s0.y) - m.monitor_strip_h - s.section_gap, m.monitor_strip_h, strip));
    }
    let cells = m.cells();
    let tiles = if m.view == View::Picker { m.picker_tiles(store) } else { Vec::new() };
    for (i, slot) in lay.slots.iter().enumerate() {
        if !visible(slot.y, slot.h) {
            continue;
        }
        let row = &m.rows[i];
        let selected = i == m.cursor;
        let el = if i < cells && m.view == View::AppGrid {
            let gutter = s.sm;
            let image = store.menu.icons.get(&row.icon_source).cloned().flatten();
            let pic = if image.is_some() {
                w::picture(image, s.control_height * 2.0)
            } else {
                El::new(ui::el::Kind::Icon { name: "layout-grid".into(), size: Type::DisplayLarge, ink: Ink::Dim })
                    .width(Size::Px(s.control_height * 2.0))
                    .height(s.control_height * 2.0)
            };
            let content = w::column(s.row_gap, vec![pic.centred(), w::label(row.label.clone()).elide().hug().centred()]);
            w::cell(content)
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| st.row = true)
                .on(format!("row:{i}"))
                .stop(format!("c:{}", row.id))
                .width(Size::Px(slot.w))
                .pad(gutter, gutter, gutter, gutter)
        } else if m.view == View::Monitor {
            proc_row(m, store, row, i, selected, theme).width(Size::Px(slot.w))
        } else if i < cells && m.view == View::Picker {
            picker_tile(m, store, theme, &tiles, row, i, selected, slot)
        } else if i < cells {
            let glyph = w::text(row.icon.clone()).size(Type::Display).centred();
            w::cell(w::column(0.0, vec![glyph]))
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| st.row = true)
                .on(format!("row:{i}"))
                .stop(format!("e:{}", row.id))
                .width(Size::Px(slot.w))
                .height(slot.h)
        } else {
            let checked = toggles::checked_for(Some(row), Some(&snap), Some(&store.menu.checked));
            menu_row(m, store, row, i, selected, checked, theme).width(Size::Px(slot.w)).height(slot.h - slot.band)
        };
        items.push((slot.y + slot.band, slot.h - slot.band, el.pad_start(0.0)));
    }
    // Absolutely placed by stacking: a column of rows, each a row of what
    // shares its top.
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut col: Vec<El> = Vec::new();
    let mut y = 0.0;
    let i = 0;
    while i < items.len() {
        let top = items[i].0;
        let mut line = Vec::new();
        let mut h: f64 = 0.0;
        let mut x = 0.0;
        while i < items.len() && (items[i].0 - top).abs() < 0.5 {
            let (_, ih, el) = items.remove(i);
            let slot_x = slot_x_of(m, &el);
            if slot_x > x {
                line.push(w::space(slot_x - x));
                x = slot_x;
            }
            x += match el.width { Size::Px(px) => px, _ => 0.0 };
            line.push(el);
            h = h.max(ih);
        }
        if top > y {
            col.push(w::space(top - y));
        }
        // Exactly the slot's height: the next line's gap is reckoned from
        // the layout, so a line shorter than its slot would pull every line
        // under it up and leave the viewport's foot empty.
        // Keyed by the line's top in the content, so a scroll that culls
        // the lines above it leaves its rows' hover and tweens on them.
        col.push(w::row(0.0, line).top().height(h).key(format!("y{}", top.round() as i64)));
        y = top + h;
    }
    col.push(w::space((lay.content_h - y).max(0.0)));
    w::column(0.0, col)
}

/// The mirror's feed: the picture in its box, or why there is none.
fn mirror_body(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let (w, h) = m.mirror_box(theme);
    let mi = &store.mirror;
    let frame = mi.frame.as_ref().filter(|(id, _)| *id == m.mirror_current).map(|(_, b)| b.clone());
    let feed = match frame {
        Some(b) => w::picture(Some(b), h).width(Size::Px(w)),
        None => {
            let word = if !mi.listed {
                ""
            } else if mi.cameras.is_empty() {
                "No camera"
            } else if !mi.error.is_empty() {
                mi.error.as_str()
            } else {
                "Starting\u{2026}"
            };
            let line = 20.0;
            w::column(0.0, vec![w::space(((h - line) / 2.0).max(0.0)), w::text(word).ink(Ink::Muted).centred(), w::space(((h - line) / 2.0).max(0.0))])
                .width(Size::Px(w))
        }
    };
    w::column(0.0, vec![w::space(s.md), w::row(0.0, vec![w::space(s.sm), feed])])
}

/// The process table's rows: the filter, busiest first.
fn monitor_rows(store: &Store, q: &str) -> Vec<Node> {
    use fs_system::monitor::procs;
    let p = &store.procs;
    if !p.available {
        let note = if p.sampled_ms.is_none() { "Reading processes" } else { "No process table" };
        return vec![Node { dim: Some(true), ..Node::note("monitor.empty", note) }];
    }
    let rows = procs::sort_rows(&procs::filter_rows(&p.rows, q), "cpu");
    if rows.is_empty() {
        return vec![Node { dim: Some(true), ..Node::note("monitor.empty", format!("No process matches \u{201C}{q}\u{201D}")) }];
    }
    rows.iter()
        .take(200)
        .map(|r| Node {
            desc: Some(r.cmd.clone()),
            actions: vec![
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("end", "End", Intent::EndProcess(r.pid)) },
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("kill", "Kill", IpcCall::new("monitor", "kill").arg(r.pid.to_string()).arg("KILL")).destructive() },
            ],
            ..Node::new(format!("proc.{}", r.pid), r.name.clone(), NodeKind::Action)
        })
        .collect()
}

/// The monitor's strip: CPU, Memory, GPU, Disk and Network, each a
/// label over one figure; a reading nobody has taken yet is a dash.
fn monitor_strip(store: &Store, theme: &Theme, h: &MonitorHistory) -> El {
    use fs_system::monitor::format;
    let s = &theme.space;
    let m = &store.monitor;
    let gpu = m
        .cards
        .iter()
        .find(|c| c.record.metrics.available && c.record.metrics.busy.is_some())
        .or_else(|| m.cards.first());
    let gpu_figure = match gpu {
        None => "No GPU".to_owned(),
        Some(c) => c.record.metrics.busy.filter(|_| c.record.metrics.available).map_or_else(|| "No metrics".to_owned(), |b| format::pct(Some(b))),
    };
    let disk = m.disk.iter().find(|d| d.mount == "/").or_else(|| m.disk.first());
    let rx: Option<f64> = m.net_available.then(|| m.net.iter().map(|n| n.rx_bytes_per_sec).sum());
    let line = |q: &std::collections::VecDeque<f64>, second: Option<&std::collections::VecDeque<f64>>, ceiling: f64| {
        w::sparkline(q.iter().copied().collect(), second.map_or_else(Vec::new, |s| s.iter().copied().collect()), ceiling, HISTORY)
    };
    let net_ceiling = h.rx.iter().chain(h.tx.iter()).copied().fold(1.0, f64::max);
    // Disk fill has no history worth drawing: a groove instead.
    let tiles = [
        ("CPU", format::pct(m.cpu.aggregate), line(&h.cpu, None, 1.0)),
        ("Memory", format::pct(m.mem.as_ref().map(|x| x.used_fraction)), line(&h.mem, None, 1.0)),
        ("GPU", gpu_figure, line(&h.gpu, None, 1.0)),
        ("Disk", format::pct(disk.map(|d| d.fraction)), w::column(0.0, vec![w::space((s.control_height - s.track_thickness).max(0.0)), w::track(disk.map_or(0.0, |d| d.fraction.clamp(0.0, 1.0)))])),
        ("Network", format::rate(rx), line(&h.rx, Some(&h.tx), net_ceiling)),
    ];
    let cells = tiles
        .into_iter()
        .map(|(label, figure, plot)| {
            w::cell(w::column(
                s.xxs,
                vec![w::section_label(s, label, None, false), w::text(figure).size(Type::Title).mono(), plot],
            ))
            .fill()
        })
        .collect();
    w::row(s.sm, cells).fill().pad(s.sm, 0.0, s.sm, 0.0)
}

/// One process: name and command line, then pid, CPU and memory in mono.
/// An armed row turns destructive under a confirm.
fn proc_row(m: &Model, store: &Store, row: &Node, i: usize, selected: bool, theme: &Theme) -> El {
    use fs_system::monitor::format;
    let s = &theme.space;
    let pid = row.id.strip_prefix("proc.").and_then(|p| p.parse::<u64>().ok());
    let armed = pid.is_some_and(|p| m.monitor_armed.is(&p));
    let figures = pid.and_then(|p| store.procs.rows.iter().find(|r| r.pid == p));
    let (pid_s, cpu, mem) = figures.map_or_else(Default::default, |r| (r.pid.to_string(), format::proc_pct(r.cpu_fraction), format::bytes(Some(r.mem_bytes))));
    let name = if armed { format!("End {}? Enter again", row.label) } else { row.label.clone() };
    let mut lead = vec![w::label(name).elide()];
    if row.kind == NodeKind::Note {
        return w::cell(w::text(row.label.clone()).ink(Ink::Dim)).ghost();
    }
    if let Some(cmd) = row.desc.clone().filter(|c| !c.is_empty() && !armed) {
        lead.push(w::text(cmd).size(Type::BodySmall).ink(Ink::Dim).elide());
    }
    // Fixed gutters, so the mono figures line up down the table.
    let figure = |t: &str, px: f64| w::caption(t.to_owned()).mono().ink(Ink::Muted).width(Size::Px(px));
    let trail = vec![figure(&pid_s, 56.0), figure(&cpu, 48.0), figure(&mem, 56.0)];
    let parts = vec![w::row(s.icon_gap, lead).fill(), w::row(s.sm, trail)];
    w::cell(w::row(s.control_padding_x, parts).fill())
        .ghost()
        .selected(selected)
        .interactive()
        .cell_state(|st| st.destructive = armed)
        .on(format!("row:{i}"))
        .stop(format!("r:{}", row.id))
}

/// What the preview pane shows for a subject (launcher spec §7), `None`
/// for one with no preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Clip,
    File,
    Folder,
    Window,
    Package,
}

impl PaneKind {
    pub fn name(self) -> &'static str {
        match self {
            PaneKind::Clip => "clipboard",
            PaneKind::File => "file",
            PaneKind::Folder => "folder",
            PaneKind::Window => "window",
            PaneKind::Package => "package",
        }
    }
}

pub fn pane_kind(n: &Node) -> Option<PaneKind> {
    if matches!(n.parent_id.as_deref(), Some("clipboard" | "share.history")) && n.kind != NodeKind::Note {
        Some(PaneKind::Clip)
    } else if fs_menu::providers::is_file_subject(n) {
        Some(PaneKind::File)
    } else if fs_menu::providers::is_folder_subject(n) {
        Some(PaneKind::Folder)
    } else if n.kind == NodeKind::Window {
        Some(PaneKind::Window)
    } else if fs_menu::providers::is_package(n) {
        Some(PaneKind::Package)
    } else {
        None
    }
}

/// The picture a pane subject shows and where it is read from: a
/// clipboard capture or an image file through the decoder, or the window
/// the pane captures.
pub enum PanePicture<'a> {
    Decoded(&'a str),
    Window(&'a str),
}

pub fn pane_picture(n: &Node) -> Option<PanePicture<'_>> {
    match pane_kind(n)? {
        PaneKind::Clip if !n.thumb_source.is_empty() => Some(PanePicture::Decoded(&n.thumb_source)),
        PaneKind::File if fs_menu::providers::is_image(&n.path) => Some(PanePicture::Decoded(&n.path)),
        PaneKind::Window => Some(PanePicture::Window(&n.path)),
        _ => None,
    }
}

/// An Install row's screenshot box in a pane `w` wide: the width, at most
/// 10:16 of it tall.
pub fn shot_box(w: f64) -> (u32, u32) {
    (w.max(1.0).round() as u32, (w * 0.625).max(1.0).round() as u32)
}

/// The Install pane, as tall as what `fspec info` had to say: the icon,
/// name and summary, the description clamped, the catalog's facts as
/// label-over-value lines in two columns, then the first screenshot once
/// fetched, in what height is left. A field the catalog left out is not
/// drawn.
fn package_pane(store: &Store, row: &Node, theme: &Theme, kit: &mut Kit, width: f64, height: f64) -> El {
    let s = &theme.space;
    let info = store.packages.info.get(&row.path);
    let ok = info.and_then(|i| i.as_ref().ok());
    let found = store.packages.search.as_ref().and_then(|(_, f)| f.as_ref().ok()).and_then(|f| f.iter().find(|p| p.id == row.path));
    let icon_px = launcher_px(48.0);
    let mark = match store.menu.row_icons.get(&row.icon_source).cloned().flatten() {
        Some(image) => {
            let density = image.pixmap.width() as f64 / icon_px.max(1.0);
            w::picture(Some(image.at(density)), icon_px)
        }
        None => w::icon("package").size(Type::Display).ink(Ink::Muted).width(Size::Px(icon_px)).height(icon_px),
    };
    let name = ok.and_then(|i| i.name.clone()).unwrap_or_else(|| row.label.clone());
    let summary = ok.and_then(|i| i.summary.clone()).or_else(|| found.and_then(|p| p.description.clone())).unwrap_or_default();
    let text_w = (width - icon_px - s.lg).max(1.0);
    let mut head = vec![w::text(name).size(Type::Heading).weight(Weight::Medium).elide()];
    if !summary.is_empty() {
        head.push(w::para(summary, Type::BodySmall, Weight::Normal, Ink::Muted, 2));
    }
    let mut parts = vec![w::row(s.lg, vec![mark, w::column(s.xs, head).width(Size::Px(text_w))])];
    if let Some(d) = ok.and_then(|i| i.description.clone()) {
        parts.push(w::para(d, Type::BodySmall, Weight::Normal, Ink::Fg, 4));
    }
    let source = ok.map(|i| i.source.clone()).or_else(|| found.map(|p| p.source.clone())).filter(|v| !v.is_empty());
    let version = ok.and_then(|i| i.version.clone()).or_else(|| found.and_then(|p| p.version.clone()));
    let facts: Vec<(&str, String)> = [
        ("Version", version),
        ("Source", source),
        ("Download", ok.and_then(|i| i.download.clone())),
        ("Installed size", ok.and_then(|i| i.installed.clone())),
        ("License", ok.and_then(|i| i.license.clone())),
        ("Developer", ok.and_then(|i| i.developer.clone())),
    ]
    .into_iter()
    .filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| (k, v)))
    .collect();
    let col_w = ((width - s.lg) / 2.0).floor().max(1.0);
    let line = |(label, value): &(&str, String)| {
        w::column(s.xxs, vec![w::text(label.to_string()).size(Type::BodySmall).weight(Weight::Medium), w::caption(value.clone()).ink(Ink::Muted).elide()]).width(Size::Px(col_w))
    };
    let grid: Vec<El> = facts.chunks(2).map(|pair| w::row(s.lg, pair.iter().map(line).collect())).collect();
    if !grid.is_empty() {
        parts.push(w::column(s.md, grid));
    }
    if let Some(Err(why)) = info {
        parts.push(w::caption(why.clone()).ink(Ink::Muted));
    }
    let shot = ok.and_then(|i| i.screenshot.as_ref()).and_then(|u| store.packages.shots.get(u)).cloned().flatten();
    let (bw, bh) = shot_box(width);
    if let Some(bitmap) = shot.and_then(|path| store.clipboard.preview.as_ref().filter(|p| p.path == path && p.size.0 == (bw, bh)).and_then(|p| p.bitmap.clone())) {
        let used = ui::measure(&w::column(s.lg, parts.clone()), width, theme, kit).1 + s.lg;
        let (iw, ih) = (f64::from(bitmap.pixmap.width()).max(1.0), f64::from(bitmap.pixmap.height()).max(1.0));
        let h = (f64::from(bw) * ih / iw).min(f64::from(bh)).min(height - used).floor();
        // Too short a strip to show anything is left out.
        if h >= 48.0 {
            let pw = (h * iw / ih).min(f64::from(bw)).floor();
            parts.push(w::picture(Some(bitmap), h).width(Size::Px(pw)).height(h));
        }
    }
    w::column(s.lg, parts)
}

/// The pane's name and facts lines under what it shows.
fn pane_foot_h(theme: &Theme, kit: &mut Kit, w: f64) -> f64 {
    let s = &theme.space;
    ui::measure(&w::text("Ag"), w, theme, kit).1 + ui::measure(&w::caption("Ag").mono(), w, theme, kit).1 + s.row_gap * 2.0
}

/// The pane's picture box for a pane `w` by `h`.
pub fn preview_box(theme: &Theme, kit: &mut Kit, w: f64, h: f64) -> (u32, u32) {
    (w.max(1.0).round() as u32, (h - pane_foot_h(theme, kit, w) - theme.space.lg).max(1.0).round() as u32)
}

/// A frame of a `from` window fitted inside `box_`, its shape kept.
pub fn fit_inside(from: (f64, f64), box_: (u32, u32)) -> (u32, u32) {
    let (bw, bh) = (f64::from(box_.0), f64::from(box_.1));
    if from.0 <= 0.0 || from.1 <= 0.0 {
        return box_;
    }
    let k = (bw / from.0).min(bh / from.1);
    ((from.0 * k).round().max(1.0) as u32, (from.1 * k).round().max(1.0) as u32)
}

/// The preview pane, DESIGN's one card inside the surface: what the
/// selected subject holds in the room above, then its name and a line of
/// facts in `mono`.
fn preview_el(m: &Model, store: &Store, row: &Node, theme: &Theme, kit: &mut Kit, width: f64, height: f64) -> El {
    use fs_menu::providers::Peek;
    let s = &theme.space;
    let Some(kind) = pane_kind(row) else { return w::space(0.0) };
    let (bw, bh) = preview_box(theme, kit, width, height);
    let (bw_f, bh_f) = (f64::from(bw), f64::from(bh));
    let boxed = |el: El| el.width(Size::Px(bw_f)).height(bh_f);
    let decoded = |path: &str| {
        store.clipboard.preview.as_ref().filter(|p| p.path == path && p.size.0 == (bw, bh)).map(|p| (p.bitmap.clone(), p.dims))
    };
    // The picture hangs from the pane's top at its own shape, the space
    // under it keeping the name and facts on the pane's foot.
    let picture = |image: Option<crate::scene::Bitmap>| {
        let h = image.as_ref().map_or(bh_f, |b| {
            let (iw, ih) = (f64::from(b.pixmap.width()).max(1.0), f64::from(b.pixmap.height()));
            (bw_f * ih / iw).min(bh_f).round()
        });
        boxed(w::column(0.0, vec![w::picture(image, h).width(Size::Px(bw_f)).height(h), w::space((bh_f - h).max(0.0))]))
    };
    let line = ui::measure(&w::caption("Ag").mono(), width, theme, kit).1.max(1.0);
    let lines_el = |lines: &[String]| {
        let fits = ((bh_f / line).floor() as usize).max(1);
        let col: Vec<El> = lines.iter().take(fits).map(|l| w::caption(l.clone()).mono().ink(Ink::Fg).elide()).collect();
        boxed(w::column(0.0, col))
    };
    let muted = |t: &str| boxed(w::column(0.0, vec![w::space((bh_f / 2.0 - line).max(0.0)), w::text(t.to_owned()).ink(Ink::Muted).centred()]));
    let peeked = || store.menu.peek.as_ref().filter(|(p, _)| *p == row.path).map(|(_, p)| p);
    let dims = |d: Option<(u32, u32)>| d.map(|(w_, h_)| format!("{w_} \u{D7} {h_}"));
    let join = |parts: Vec<String>| parts.into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" \u{B7} ");
    let (content, title, facts) = match kind {
        PaneKind::Clip if !row.thumb_source.is_empty() => {
            let got = decoded(&row.thumb_source);
            let facts = join(vec![dims(got.as_ref().and_then(|g| g.1)).unwrap_or_default(), row.time.clone()]);
            (picture(got.and_then(|g| g.0)), "Image".to_owned(), facts)
        }
        PaneKind::Clip if row.emoji_only => (boxed(w::column(0.0, vec![w::space((bh_f / 2.0 - line * 2.0).max(0.0)), w::text(row.full_text.clone()).size(Type::Display).centred()])), "Text".to_owned(), row.time.clone()),
        PaneKind::Clip => {
            let body_line = ui::measure(&w::text("Ag"), width, theme, kit).1.max(1.0);
            let lines = ((bh_f / body_line).floor() as usize).max(1);
            (boxed(w::para(row.full_text.clone(), Type::Body, Weight::Normal, Ink::Fg, lines)), "Text".to_owned(), row.time.clone())
        }
        PaneKind::File | PaneKind::Folder => {
            let p = peeked();
            let facts = p.map(fs_menu::providers::facts).unwrap_or_default();
            // A picture shows off its own decode, which is asked beside the
            // peek; the peek only adds its facts.
            let content = match p.map(|p| &p.peek) {
                _ if matches!(kind, PaneKind::File) && fs_menu::providers::is_image(&row.path) && !matches!(p.map(|p| &p.peek), Some(Peek::Unreadable(_))) => {
                    let got = decoded(&row.path);
                    let facts = join(vec![dims(got.as_ref().and_then(|g| g.1)).unwrap_or_default(), facts.clone()]);
                    return pane_card(picture(got.and_then(|g| g.0)), &row.label, &facts, s);
                }
                None | Some(Peek::Image) => muted(""),
                Some(Peek::Text(lines)) if lines.iter().all(|l| l.trim().is_empty()) => muted("Empty file"),
                Some(Peek::Text(lines)) => lines_el(lines),
                Some(Peek::Binary) => muted("No preview for this file"),
                Some(Peek::Folder { names, .. }) if names.is_empty() => muted("Empty folder"),
                Some(Peek::Folder { names, .. }) => {
                    let row_h = ui::measure(&w::text("Ag"), width, theme, kit).1.max(1.0) + s.xs;
                    let fits = ((bh_f / row_h).floor() as usize).max(1);
                    let col: Vec<El> = names
                        .iter()
                        .take(fits)
                        .map(|e| w::row(s.sm, vec![w::icon(if e.dir { "folder" } else { "file" }).ink(Ink::Dim), w::text(e.name.clone()).elide()]).height(row_h))
                        .collect();
                    boxed(w::column(0.0, col))
                }
                Some(Peek::Unreadable(why)) => muted(why),
            };
            (content, row.label.clone(), facts)
        }
        PaneKind::Package => return package_pane(store, row, theme, kit, width, height),
        PaneKind::Window => {
            let facts = row.meta.clone().filter(|m| !m.is_empty()).map(|ws| format!("Workspace {ws}")).unwrap_or_default();
            let content = match &m.pane_frame {
                Some(frame) => picture(Some(frame.clone())),
                None => muted(""),
            };
            (content, row.label.clone(), facts)
        }
    };
    pane_card(content, &title, &facts, s)
}

/// Pond's pane: the preview at the top, its name and facts at the foot,
/// no card of its own round them.
fn pane_card(content: El, title: &str, facts: &str, s: &fs_theme::tokens::Space) -> El {
    let foot = vec![content, w::space((s.lg - s.row_gap).max(0.0)), w::text(title.to_owned()).weight(Weight::Medium).elide(), w::caption(facts.to_owned()).ink(Ink::Muted).elide()];
    w::column(s.row_gap, foot).fill()
}

/// A picker tile: its picture 16:9 with the current mark and a filed
/// single's mode badge, the caption under it, the cursor's ring round both.
#[allow(clippy::too_many_arguments)]
fn picker_tile(m: &Model, store: &Store, theme: &Theme, tiles: &[(&str, fs_menu::providers::Item)], row: &Node, i: usize, selected: bool, slot: &Slot) -> El {
    use fs_menu::providers::{ItemKind, Mode as Filed};
    let s = &theme.space;
    let item = tiles.iter().map(|(_, t)| t).find(|t| row.id.strip_prefix("wallpaper.") == Some(t.id.as_str()));
    let (w, h) = m.picker_tile_px(theme);
    let key = item.map(|t| crate::services::picker::tile_key(t, &row.path)).unwrap_or_default();
    let image = store.picker.thumbs.get(&key).cloned().flatten();
    let current = !m.picker.select && item.is_some_and(|t| Model::picker_current(store, t));
    let ringed = item.is_some_and(|t| Model::picker_mode_on(store, t));
    let badge = match item.map(|t| &t.kind) {
        Some(ItemKind::Single { filed: Some(Filed::Dark), .. }) if !m.picker.select => Some("moon"),
        Some(ItemKind::Single { filed: Some(Filed::Light), .. }) if !m.picker.select => Some("sun"),
        _ => None,
    };
    let by = match item.map(|t| &t.kind) {
        Some(ItemKind::Sky) => "Follows the sun".to_owned(),
        _ => item.and_then(|t| t.credit.as_ref()).map(|c| c.author.clone()).unwrap_or_default(),
    };
    let mut words = vec![w::text(row.label.clone()).size(Type::Caption).weight(Weight::Medium).elide().hug()];
    if !by.is_empty() {
        words.push(w::text(format!("\u{B7} {by}")).size(Type::Caption).weight(Weight::Medium).ink(Ink::Muted).elide());
    }
    let caption = w::row(s.xs, words).width(Size::Px(w));
    let content = w::column(s.row_gap, vec![w::tile_picture(image, w, h, current, ringed, badge), caption]);
    w::cell(content)
        .ghost()
        .interactive()
        .cell_state(|st| {
            st.cursor = selected && m.from_keys;
            st.inset = Some((s.xs, s.xs));
        })
        .on(format!("row:{i}"))
        .stop(format!("p:{}", row.id))
        .width(Size::Px(slot.w))
        .height(slot.h - slot.band)
}

/// The Style heading's far end: where settings.json pins the mode, so;
/// under `auto`, the mode and when the schedule next turns it.
fn picker_style_note(store: &Store) -> String {
    use crate::services::theme;
    let key = theme::mode_key(store.config.settings());
    match key {
        "dark" | "light" => "Set in settings.json".into(),
        "auto" => {
            let data = &store.state.data;
            let times = store.theme.schedule.as_ref();
            let over = theme::parse_override(&data.mode_override);
            let mode = theme::effective_mode(key, times, over.as_ref(), &data.mode);
            let until = fs_theme::sun::next_change(&chrono::Local::now(), times);
            format!("Auto, {mode} until {}", until.format("%H:%M"))
        }
        _ => String::new(),
    }
}

/// An SPDX licence id as a heading names it: `CC0-1.0` reads `CC0`.
fn licence_name(spdx: &str) -> String {
    match spdx.rsplit_once('-') {
        Some((name, version)) if !name.is_empty() && version.chars().all(|c| c.is_ascii_digit() || c == '.') => name.to_owned(),
        _ => spdx.to_owned(),
    }
}

fn write_picker_selection(payload: &Value) {
    let path = picker_selection_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, payload.to_string());
}

/// Where an item sits across the body, off its key.
fn slot_x_of(m: &Model, el: &El) -> f64 {
    let Some(key) = &el.key else { return 0.0 };
    let id = key.split_once(':').map_or("", |(_, id)| id);
    m.rows.iter().position(|r| r.id == id).and_then(|i| m.layout.slots.get(i)).map_or(0.0, |s| s.x)
}

/// One row: a `row_icon` mark, the label at 400 (600 under the selection),
/// a dim trailing value, `lg` either side.
#[allow(clippy::too_many_arguments)]
fn menu_row(m: &Model, store: &Store, row: &Node, i: usize, selected: bool, checked: bool, theme: &Theme) -> El {
    let s = &theme.space;
    let confirming = m.confirming(&row.id);
    let icon_px = launcher_px(fs_theme::tokens::LAUNCHER.row_icon);
    let live = fs_menu::providers::is_package(row).then(|| crate::surfaces::packages::live(store, &row.path)).flatten();
    let checked = crate::surfaces::packages::row_checked(store, row, checked);
    let mut lead = Vec::new();
    let ringed = live.as_ref().is_some_and(|l| l.ring.is_some());
    if let Some(ring) = live.as_ref().and_then(|l| l.ring) {
        lead.push(w::ring(ring).width(Size::Px(icon_px)).height(icon_px));
    }
    // A plugin's rows and its mode carry icon names from the active set,
    // where every other node's own icon is a raw glyph.
    let glyph = match clip_glyph(row).unwrap_or(fs_menu::icons::icon_for(Some(row))) {
        "" if row.id.starts_with(fs_chrome::plugins::PLUGIN_PREFIX) => row.icon.as_str(),
        named => named,
    };
    let picture = (!row.icon_source.is_empty() && glyph.is_empty())
        .then(|| store.menu.row_icons.get(&row.icon_source).cloned().flatten())
        .flatten();
    // An emoji in Recent is its own picture, drawn as text.
    let emoji = matches!(fs_menu::actions::subject_id(row), SubjectId::Emoji(_));
    let drawn = if !glyph.is_empty() {
        glyph.to_owned()
    } else if picture.is_none() && !row.emoji_only && !emoji {
        fs_menu::icons::fallback_for(Some(row)).to_owned()
    } else {
        String::new()
    };
    if ringed {
        // The ring stands in the icon slot.
    } else if let Some(image) = picture {
        let density = image.pixmap.width() as f64 / icon_px.max(1.0);
        lead.push(w::picture(Some(image.at(density)), icon_px));
    } else if !drawn.is_empty() {
        lead.push(w::icon(drawn).size(Type::Heading).width(Size::Px(icon_px)).height(icon_px));
    } else if emoji {
        lead.push(w::text(row.icon.clone()).size(Type::Heading).centred().width(Size::Px(icon_px)));
    }
    let label = if confirming { crate::ui::confirm::ask(&row.label) } else { row.label.clone() };
    let mut name = row_words(label);
    if row.dim == Some(true) {
        name = name.ink(Ink::Dim);
    }
    if row.emoji_only {
        name = name.size(Type::Display);
    }
    lead.push(name);
    if let Some(desc) = row.desc.clone().filter(|d| !d.is_empty()) {
        lead.push(row_words(desc).ink(Ink::Dim));
    }
    let windows = match &row.entry {
        Some(entry) if row.kind == NodeKind::App && row.meta.as_deref().unwrap_or("").is_empty() => app_windows(store, entry).len(),
        _ => 0,
    };
    let value = match windows {
        _ if live.is_some() => live.map(|l| l.value).unwrap_or_default(),
        0 => fs_menu::actions::accessory_for(Some(row)),
        1 => "1 window".into(),
        n => format!("{n} windows"),
    };
    let mut trail = Vec::new();
    if !value.is_empty() {
        trail.push(row_words(value).ink(Ink::Dim).end());
    }
    if checked && !confirming {
        trail.push(w::icon("check"));
    }
    let mut parts = vec![w::row(s.lg, lead).fill()];
    if !trail.is_empty() {
        parts.push(w::row(s.icon_gap, trail).width(Size::AtMost(0.4)));
    }
    w::cell(w::row(s.lg, parts).fill())
        .ghost()
        .selected(selected)
        .interactive()
        .cell_state(|st| {
            st.row = true;
            st.destructive = confirming;
        })
        .on(format!("row:{i}"))
        .stop(format!("r:{}", row.id))
}


#[cfg(test)]
mod grab_tests {
    use super::*;

    fn call(function: &str) -> IpcCall {
        IpcCall::new("screenshot", function)
    }

    #[test]
    fn a_stale_id_gets_nothing() {
        let mut g = Grab::default();
        let first = g.hold(call("full"));
        let second = g.hold(call("region"));
        assert_eq!(g.take(first), None);
        assert_eq!(g.take(second), Some(call("region")));
        assert_eq!(g.take(second), None);
    }

    #[test]
    fn a_cancelled_hold_is_never_released() {
        let mut g = Grab::default();
        let id = g.hold(call("full"));
        g.cancel();
        assert_eq!(g.take(id), None);
    }
}
