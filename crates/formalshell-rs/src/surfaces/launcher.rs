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

use fs_menu::actions::{Action, Run, TargetKind};
use fs_menu::field;
use fs_menu::model::{self, Mode, SectionCtx};
use fs_menu::nav::{self, Dir, KeyAction, KeyCtx, KeyMode};
use fs_menu::node::{ActionEntry, Kind as NodeKind, Node};
use fs_menu::toggles::{self, Snapshot};
use fs_chrome::types::Edge;
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId};
use crate::scroll::{Touchpad, Travel};
use crate::services::menu::{self as index, Ask};
use crate::services::{clipboard, compositor, plugins, state};
use crate::store::Store;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::surfaces::modal::Modal;
use crate::ui::{self, El, Ink, Size, Type, Ui, Weight, w};

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
    /// The headings: text and the band's top, in body coordinates.
    headings: Vec<(String, f64, f64)>,
    content_h: f64,
    columns: usize,
    cell_w: f64,
    cell_h: f64,
    /// Laid out at half the card: the selection moving onto or off a
    /// subject with a preview lays it out again.
    halved: bool,
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
    confirm: String,
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
    pub picker: Picker,
    /// The Dark | Light switcher's height at the head of the body, 0 when
    /// the route has none.
    picker_switch_h: f64,
    /// The monitor route's tile strip height, 0 off the route.
    monitor_strip_h: f64,
    /// The monitor route keeps both polls running while it is the level.
    monitor_wants: Option<(crate::services::wants::Want, crate::services::wants::Want)>,
    /// The pid an Enter armed TERM on; the next Enter on it sends it.
    monitor_armed: Option<u64>,
    /// The camera the mirror shows, by node path.
    pub mirror_current: String,
    /// The monitor tiles' history lines, started when the route opens.
    monitor_history: MonitorHistory,
    /// The row list's transitions: rows that moved, arrived or left on
    /// the last re-rank inside one level.
    motion: RowMotion,
    /// `debug motionScale`, which the row transitions run at.
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
            Into::Windows(id) | Into::Folder(id) => id,
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
        let top = self.cursor as f64 * self.row_h;
        let mut next = self.scroll;
        if top < next {
            next = top;
        } else if top + self.row_h > next + self.view_h {
            next = top + self.row_h - self.view_h;
        }
        self.scroll = next.clamp(0.0, self.max_scroll());
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

/// AddTransition, MoveTransition and RemoveTransition: an arriving row
/// fades in, one that kept its place in the list slides from where it was,
/// one that left fades out where it stood. Only inside one level and one
/// view, and not past `ROW_RESET_LIMIT` rows, where the list is refilled.
#[derive(Default)]
struct RowMotion {
    level: Option<Option<String>>,
    view: Option<View>,
    moved: std::collections::HashMap<String, (f64, Instant)>,
    added: std::collections::HashMap<String, Instant>,
    left: Vec<(Node, Slot, Instant)>,
    ms: (f64, f64, f64),
}

const ROW_RESET_LIMIT: usize = 64;

/// The rows whose labels are shaped at both weights on a resolve: more
/// than a card at `LAUNCHER.row_height` shows.
const ROWS_SHAPED_AHEAD: usize = 24;

impl RowMotion {
    fn progress(at: Instant, ms: f64, now: Instant) -> f64 {
        if ms <= 0.0 { 1.0 } else { (now.saturating_duration_since(at).as_secs_f64() * 1000.0 / ms).min(1.0) }
    }

    fn running(&self, now: Instant) -> bool {
        self.moved.values().any(|(_, at)| Self::progress(*at, self.ms.0, now) < 1.0)
            || self.added.values().any(|at| Self::progress(*at, self.ms.1, now) < 1.0)
            || self.left.iter().any(|(_, _, at)| Self::progress(*at, self.ms.2, now) < 1.0)
    }

    /// The slide still owed to a row, in body pixels.
    fn offset(&self, id: &str, now: Instant) -> f64 {
        self.moved.get(id).map_or(0.0, |(dy, at)| dy * (1.0 - Clock::Spatial.curve().ease(Self::progress(*at, self.ms.0, now))))
    }

    fn alpha(&self, id: &str, now: Instant) -> f32 {
        self.added.get(id).map_or(1.0, |at| Clock::Effects.curve().ease(Self::progress(*at, self.ms.1, now)) as f32)
    }
}

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
    pub light: bool,
    /// `picker select` set this up for the open that follows.
    pending: bool,
}

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
            picker: Picker::default(),
            picker_switch_h: 0.0,
            monitor_strip_h: 0.0,
            monitor_wants: None,
            monitor_armed: None,
            mirror_current: String::new(),
            monitor_history: MonitorHistory::default(),
            motion: RowMotion::default(),
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
            confirm: String::new(),
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
    /// An in-process `@ipc:` action the App dispatches.
    Internal(String),
}

pub fn snapshot(store: &Store) -> Snapshot {
    let d = &store.devices;
    let radio = &store.media.radio;
    let connected: Vec<String> = d.bluetooth.devices.iter().filter(|x| x.connected).map(|x| x.address.to_uppercase()).collect();
    let l = &store.lights;
    toggles::snapshot(Some(&json!({
        "nightlight.active": store.nightlight.active,
        "lights.on": l.brightness > 0,
        "lights.effect": l.effect,
        "lights.source": l.source,
        "lights.colour": if l.source == "custom" { l.custom_colour.as_str() } else { "" },
        "lights.speed": l.speed,
        "lights.brightness": l.brightness.to_string(),
        "theme.dark": store.state.data.mode == "dark",
        "caffeinate.active": store.caffeinate.active,
        "notifications.dnd": store.notifications.model.dnd,
        "overnight.active": !store.state.data.overnight.is_null(),
        "hdr.active": crate::services::display::hdr_active(store),
        "wifi.ssid": d.network.ssid.as_ref().map_or("", |(s, _)| s.as_str()),
        "audio.sink": d.audio.default_sink,
        "audio.source": d.audio.default_source,
        "bluetooth.connected": connected,
        "radio.station": radio.station.as_ref().filter(|_| radio.running()).map_or("", |s| s.uuid.as_str()),
    })))
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

/// The four device routes' rows, off the store.
pub fn device_sources(store: &Store) -> Vec<(&'static str, Vec<Node>)> {
    use fs_menu::providers as p;
    let d = &store.devices;
    let n = &d.network;
    let wifi = p::WifiState {
        has_device: n.wifi_device.is_some(),
        enabled: n.wifi_enabled,
        networks: n
            .rows
            .iter()
            .map(|r| p::WifiNetwork {
                name: r.ssid.clone(),
                known: r.known,
                connected: r.connected,
                secured: r.secured,
                enterprise: r.enterprise,
                in_range: r.in_range,
                signal: r.signal,
                signal_strength: None,
            })
            .collect(),
        action_ssid: n.action.as_ref().map(|(_, s)| s.clone()).unwrap_or_default(),
        action_kind: n.action.as_ref().map(|(k, _)| k.as_str().to_owned()).unwrap_or_default(),
        failure_ssid: n.failure.as_ref().map(|f| f.ssid.clone()).unwrap_or_default(),
        failure_text: n.failure.as_ref().map(|f| f.text.clone()).unwrap_or_default(),
    };
    let b = &d.bluetooth;
    let bluetooth = p::BluetoothState { available: b.powered.is_some(), enabled: b.powered == Some(true), devices: b.devices.clone() };
    let audio: Vec<p::AudioDevice> =
        d.audio.devices.iter().map(|(name, label, sink)| p::AudioDevice { name: name.clone(), label: label.clone(), is_sink: *sink }).collect();
    let r = &store.media.radio;
    let radio = p::RadioState { running: r.running(), favorites: r.favorites.iter().map(radio_station).collect() };
    vec![
        ("wifi", p::wifi_rows(&wifi)),
        ("bluetooth", p::bluetooth_rows(&bluetooth)),
        ("audio", p::audio_rows(&audio)),
        ("radio", p::radio_rows(&radio)),
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
pub fn plugin_sources(store: &Store) -> Vec<(String, Vec<Node>)> {
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
            (route, rows)
        })
        .collect()
}

/// The `@ipc:` action that writes `line` to plugin `id`'s stdin.
fn plugin_send(id: &str, line: &str) -> String {
    format!("plugin.send:{id}:{line}")
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
        let mut n = Node::new(format!("{route}.{}", r.id), r.text.clone(), NodeKind::Action).with_action(format!("@ipc:{}", plugin_send(id, &activate)));
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
                let run = format!("@ipc:{}", plugin_send(id, &plugins::action_event(&r.id, &a.id)));
                ActionEntry { icon: Some(a.icon.clone()).filter(|i| !i.is_empty()), ..ActionEntry::new(&a.id, label, run) }
            })
            .collect();
        out.push(n);
    }
    out
}

impl Model {
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

    /// The query the list is ranked by: the field's, or while the column
    /// filters through the field, the one it held before.
    fn list_text(&self) -> &str {
        self.column.as_ref().map_or(self.query.text(), |c| c.list_query.as_str())
    }

    /// One monitor tick onto the tiles' history lines.
    pub fn monitor_sample(&mut self, store: &Store) {
        let m = &store.info.monitor;
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

    /// The routes that draw a whole view of their own.
    pub fn app_view(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some(MONITOR_ROUTE | MIRROR_ROUTE))
    }

    /// One card width per level kind, which the content
    /// is laid out at while the card itself morphs into it.
    pub fn card_width(&self, theme: &Theme) -> f64 {
        let s = &theme.space;
        if self.app_view() {
            s.popup_width_menu_app
        } else if self.halved() {
            s.popup_width_menu_split
        } else {
            s.popup_width_menu
        }
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

    /// The card's height for the level kind, capped by a share of the
    /// output's.
    fn kind_height(&self, theme: &Theme, output_h: f64) -> f64 {
        let s = &theme.space;
        let (kind, share) = if self.app_view() {
            (s.popup_height_menu_app, fs_theme::tokens::LAUNCHER.app_height_share)
        } else if self.split() {
            (s.popup_height_menu_split, fs_theme::tokens::LAUNCHER.height_share)
        } else {
            (s.popup_height_menu, fs_theme::tokens::LAUNCHER.height_share)
        };
        if output_h > 0.0 { kind.min(output_h * share) } else { kind }
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

    pub fn body_height(&self) -> f64 {
        self.body_h
    }

    pub fn on_picker(&self) -> bool {
        self.mode == Mode::Menu && self.level.as_deref() == Some(PICKER_ROUTE)
    }

    pub fn picker_variants(&self, store: &Store) -> fs_menu::providers::WallpaperVariants {
        if store.picker.dir != self.picker.dir {
            return Default::default();
        }
        fs_menu::providers::wallpaper_variants(&store.picker.scanned, &self.picker.dir)
    }

    fn picker_variant(&self) -> Option<fs_menu::providers::Variant> {
        use fs_menu::providers::Variant;
        Some(if self.picker.light { Variant::Light } else { Variant::Dark })
    }

    /// The directory's pictures.
    pub fn picker_listing(&self, store: &Store) -> Vec<String> {
        let variants = self.picker_variants(store);
        fs_menu::providers::wallpaper_listing(&variants, self.picker_variant()).to_vec()
    }

    fn picker_rows(&self, store: &Store, query: &str) -> Vec<Node> {
        let q = query.trim().to_lowercase();
        fs_menu::providers::image_rows(&self.picker_listing(store), "")
            .into_iter()
            .filter(|row| q.is_empty() || row.label.to_lowercase().contains(&q))
            .collect()
    }

    /// The wallpaper route's segmented switcher shows.
    pub fn picker_switch(&self, store: &Store) -> bool {
        self.on_picker() && self.picker_variants(store).has_variants
    }

    /// `openImageSelect`.
    pub fn open_image_select(&mut self, store: &Store, dir: &str, token: &str) {
        self.picker_abandon();
        let dir = if dir.is_empty() { store.config.str("picker.directory").unwrap_or("").to_owned() } else { dir.to_owned() };
        self.picker = Picker { select: true, dir, token: token.into(), light: self.picker.light, pending: true };
        self.open(store, Some(PICKER_ROUTE));
    }

    /// `chooseImage`: false off the route or for a path not listed.
    pub fn choose_image(&mut self, store: &Store, path: &str) -> bool {
        if !self.open || !self.on_picker() || !self.picker_listing(store).iter().any(|p| p == path) {
            return false;
        }
        if self.picker.select {
            write_picker_selection(&json!({"token": self.picker.token, "value": path}));
            self.picker.token.clear();
        } else {
            let mode = fs_menu::providers::wallpaper_pick_mode(&self.picker_variants(store), self.picker_variant());
            let mode = mode.map(|v| if v == fs_menu::providers::Variant::Light { "light" } else { "dark" });
            state::set_wallpaper(path, mode);
        }
        self.close();
        true
    }

    /// `setPickerVariant`.
    pub fn set_picker_variant(&mut self, store: &Store, light: bool) -> bool {
        if !self.open || !self.picker_switch(store) {
            return false;
        }
        if self.picker.light != light {
            self.picker.light = light;
            self.from_keys = true;
            self.dirty = true;
        }
        true
    }

    /// `pickerStatus`.
    pub fn picker_status(&self, store: &Store) -> Value {
        let v = self.picker_variants(store);
        let listing = self.picker_listing(store);
        json!({
            "open": self.open && self.on_picker(),
            "mode": if self.picker.select { "select" } else { "wallpaper" },
            "directory": self.picker.dir,
            "count": listing.len(),
            "variant": if v.has_variants { if self.picker.light { "light" } else { "dark" } } else { "none" },
            "hasVariants": v.has_variants,
            "cachedThumbnails": listing.iter().filter(|p| store.picker.cached.contains(*p)).count(),
            "darkCount": v.dark.len(),
            "lightCount": v.light.len(),
            "cursor": self.cursor,
        })
    }

    fn picker_enter(&mut self, store: &Store) {
        if !self.picker.pending {
            self.picker_abandon();
            self.picker.select = false;
            self.picker.dir = store.config.str("picker.directory").unwrap_or("").to_owned();
            self.picker.token.clear();
        }
        self.picker.pending = false;
        self.picker.light = store.state.data.mode == "light";
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
        let route_rows = children.is_some() || plugin || matches!(level, Some("keybinds" | "calc" | "radio.search" | "clipboard" | "share.history"));
        let grid_wanted = store.config.bool("menu.appGrid").unwrap_or(false);
        let picker = menu && children.is_none() && level == Some(PICKER_ROUTE);
        let monitor = menu && children.is_none() && level == Some(MONITOR_ROUTE);
        let mirror = menu && children.is_none() && level == Some(MIRROR_ROUTE);
        let emoji = emoji && children.is_none() && !picker && !monitor && !mirror;
        let view = if mirror {
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
                    .map(|(i, o)| Node::new(format!("select.{i}"), o.clone(), NodeKind::Other("option".into())))
                    .filter(|n| ql.is_empty() || n.label.to_lowercase().contains(&ql))
                    .collect()
            }
            Mode::Input => Vec::new(),
            Mode::Menu if children.is_some() => children.map_or_else(Vec::new, |c| match &c.into {
                Into::Windows(id) => window_rows(store, id, q),
                Into::Folder(path) => folder_rows(store, path, q),
            }),
            Mode::Menu if picker => self.picker_rows(store, q),
            Mode::Menu if monitor => monitor_rows(store, q),
            Mode::Menu if mirror => Vec::new(),
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
                let uses: Vec<fs_menu::frecency::Record> = serde_json::from_value(store.state.data.emoji_uses.clone()).unwrap_or_default();
                let paste = store.config.bool("clipboard.paste").unwrap_or(true);
                store.menu.emoji.as_ref().map_or_else(Vec::new, |e| e.rows(q, paste, &uses, None))
            }
            Mode::Menu if level == Some("keybinds") => {
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                fs_menu::keybinds::rows_for(reply, q)
            }
            Mode::Menu if level == Some("radio.search") => radio_search_rows(store, q),
            // The plugin ranks its own rows against the query it was sent.
            Mode::Menu if plugin => model::visible_children(nodes, level, cond).into_iter().cloned().collect(),
            Mode::Menu if q.is_empty() && level.is_none() => {
                let rest = model::root_rest(nodes, &picks(store), now_ms(), fs_theme::tokens::LAUNCHER.recent as usize, cond);
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
                let picks = picks(store);
                let boost = fs_menu::search::Boost { abbrev: Some(&self.lookup), picks: &picks, now: now_ms() };
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
                .take(fs_theme::tokens::LAUNCHER.recent as usize)
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
            if picker && self.picker.light { "light" } else { "" }
        );
        (view, rows, app_count, empty, searching, key)
    }

    /// The query's abbreviations, read once per resolve.
    fn lookup(&self, store: &Store) -> fs_menu::abbrev::Lookup {
        if self.mode != Mode::Menu {
            return Default::default();
        }
        let book = abbreviation_book(store);
        fs_menu::abbrev::lookup(&configured_abbreviations(store), &book, self.list_text(), now_ms(), |id| resolves(store, id))
    }

    /// Commits the rows, their headings and the cursor.
    pub fn resolve(&mut self, store: &Store, theme: &Theme, kit: &mut Kit) {
        let asked = self.plugin_mode().map(|id| (id.to_owned(), self.query.text().to_owned()));
        if let Some((id, text)) = asked.as_ref().filter(|a| self.plugin_query.as_ref() != Some(a)) {
            plugins::send(id, plugins::query_event(text));
        }
        self.plugin_query = asked;
        self.lookup = self.lookup(store);
        self.abbrev_key = if self.mode == Mode::Menu { fs_menu::abbrev::key(self.list_text()) } else { None };
        let (view, rows, app_count, empty, searching, key) = self.resolve_rows(store);
        let fresh = key != self.key;
        let level_label = self.level.as_deref().and_then(|l| store.menu.nodes().get(l)).map(|n| n.label.clone()).unwrap_or_default();
        let sections = if !self.children.is_empty() {
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
        let before: std::collections::HashMap<String, (Node, Slot)> = if matches!(view, View::Rows | View::AppGrid) {
            let skip = if self.view == View::AppGrid { self.app_count } else { 0 };
            self.rows.iter().cloned().zip(self.layout.slots.iter().copied()).skip(skip).map(|(n, s)| (n.id.clone(), (n, s))).collect()
        } else {
            Default::default()
        };
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
        let index = nav::rederive(&self.want, self.cursor as i64, &ids, fresh, self.placed);
        self.key = key;
        self.monitor_strip_h = if view == View::Monitor { ui::measure(&monitor_strip(store, theme, &self.monitor_history), self.content_width(theme), theme, kit).1 } else { 0.0 };
        self.picker_switch_h = if self.picker_switch(store) { ui::measure(&picker_switch(self), 400.0, theme, kit).1 } else { 0.0 };
        self.layout(theme, kit);
        if changed && matches!(view, View::Rows | View::AppGrid) {
            // The selection sets its row's label at 600: both weights are
            // shaped here, so a step never waits on the shaper.
            for r in self.rows.iter().take(ROWS_SHAPED_AHEAD) {
                for weight in [Weight::Normal, Weight::Semibold] {
                    ui::measure(&w::text(r.label.clone()).weight(weight), f64::MAX, theme, kit);
                }
            }
        }
        self.row_motion(before, view, theme);
        // A key's step resolves the level again before it draws: the cursor
        // landing where the step put it keeps the step's travel.
        let travels = self.travels && !fresh && !view_changed && index == self.cursor;
        self.place(index, travels);
        self.relayout(theme, kit);
        if fresh || view_changed {
            self.scroll = 0.0;
            self.touch.settle();
            self.follow();
        }
        self.dirty = true;
    }

    /// Starts the transitions a re-rank owes, from where each row was.
    fn row_motion(&mut self, before: std::collections::HashMap<String, (Node, Slot)>, view: View, theme: &Theme) {
        let now = Instant::now();
        let same = self.motion.level.as_ref() == Some(&self.level) && self.motion.view == Some(view);
        self.motion.level = Some(self.level.clone());
        self.motion.view = Some(view);
        let k = self.motion_scale;
        let m = &mut self.motion;
        m.ms = (Clock::Spatial.ms(theme) * k, Clock::Effects.ms(theme) * k, Clock::EffectsFast.ms(theme) * k);
        m.moved.retain(|_, (_, at)| RowMotion::progress(*at, m.ms.0, now) < 1.0);
        m.added.retain(|_, at| RowMotion::progress(*at, m.ms.1, now) < 1.0);
        m.left.retain(|(_, _, at)| RowMotion::progress(*at, m.ms.2, now) < 1.0);
        let animate = same && matches!(view, View::Rows | View::AppGrid) && !before.is_empty() && before.len() <= ROW_RESET_LIMIT && self.rows.len() <= ROW_RESET_LIMIT;
        if !animate {
            m.moved.clear();
            m.added.clear();
            m.left.clear();
            return;
        }
        let mut seen = std::collections::HashSet::new();
        let skip = if view == View::AppGrid { self.app_count } else { 0 };
        for (row, slot) in self.rows.iter().zip(self.layout.slots.iter()).skip(skip) {
            seen.insert(row.id.as_str());
            match before.get(&row.id) {
                Some((_, was)) => {
                    let owed = m.moved.get(&row.id).map_or(0.0, |(dy, at)| dy * (1.0 - Clock::Spatial.curve().ease(RowMotion::progress(*at, m.ms.0, now))));
                    let dy = was.y + owed - slot.y;
                    if dy.abs() > 0.5 {
                        m.moved.insert(row.id.clone(), (dy, now));
                    }
                }
                None => {
                    m.added.insert(row.id.clone(), now);
                }
            }
        }
        for (id, (node, slot)) in before {
            if !seen.contains(id.as_str()) && node.kind != NodeKind::Note {
                m.left.push((node, slot, now));
            }
        }
    }

    /// A transition is still running, so the body draws again.
    pub fn rows_moving(&self, now: Instant) -> bool {
        self.motion.running(now) || self.touch.running(now) || self.column.as_ref().is_some_and(|c| c.touch.running(now))
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
        let width = self.body_width(theme);
        let halved = self.halved();
        let width = if halved { (width / 2.0).round() } else { width };
        let inset = if self.bleeds() { s.sm } else { s.md };
        let gutter = s.sm;
        let heading_x = if self.bleeds() { s.sm + s.lg - s.control_padding_x } else { 0.0 };
        let heading_h = ui::measure(&w::heading(s, "Ag", None, true), width, theme, kit).1;
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
            View::AppGrid | View::Emoji | View::Picker => {
                if self.picker_switch_h > 0.0 {
                    y += self.picker_switch_h + s.row_gap;
                }
                let (cells, min_cell) = if self.view == View::AppGrid {
                    (self.app_count, s.control_height * 4.0)
                } else {
                    (self.rows.len(), 0.0)
                };
                let grid_x = if self.view == View::AppGrid { gutter } else { 0.0 };
                let width = width - grid_x * 2.0;
                let columns = if self.view == View::Picker {
                    fs_theme::tokens::LAUNCHER.picker_columns as usize
                } else if self.view == View::Emoji {
                    fs_theme::tokens::LAUNCHER.emoji_columns as usize
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
                    out.headings.push((heading_text, y, gutter));
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
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: gutter, y, w: width - gutter * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
            View::Mirror => out.columns = 1,
            View::Rows | View::Monitor => {
                out.columns = 1;
                let side = s.sm;
                if self.monitor_strip_h > 0.0 {
                    y += self.monitor_strip_h + s.section_gap;
                }
                for i in 0..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, heading_x));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: side, y, w: width - side * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
        }
        out.content_h = y + inset;
        out.halved = halved;
        self.layout = out;
    }

    /// The body laid out again where the selection has moved onto or off a
    /// subject with a preview, which halves the list or gives it back.
    fn relayout(&mut self, theme: &Theme, kit: &mut Kit) {
        if self.layout.halved != self.halved() {
            self.layout(theme, kit);
            self.follow();
        }
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
        let view = self.body_h;
        let mut next = self.scroll;
        let first = self.cursor == 0 || (!matches!(self.view, View::Rows | View::Monitor) && self.cursor < self.cells() && self.cursor < self.layout.columns);
        let top = if first { 0.0 } else { top };
        let h = bottom - top;
        if top < next {
            next = top;
        } else if top + h > next + view {
            next = top + h - view;
        }
        let max = (self.layout.content_h - view).max(0.0);
        self.scroll = next.clamp(0.0, max);
    }

    fn cells(&self) -> usize {
        match self.view {
            View::AppGrid => self.app_count,
            View::Emoji | View::Picker => self.rows.len(),
            View::Rows | View::Monitor | View::Mirror => 0,
        }
    }

    pub fn move_cursor(&mut self, dir: Dir) {
        let n = self.rows.len();
        if n == 0 {
            return;
        }
        let page = self.page_step();
        let next = nav::step(self.cursor, dir, n, self.cells(), self.columns(), page);
        self.place(next.index, next.travels);
        self.placed = true;
        self.want = self.cursor_id.clone();
        self.confirm.clear();
        self.monitor_armed = None;
        self.from_keys = true;
        self.touch.settle();
        self.follow();
        self.dirty = true;
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
        if matches!(self.view, View::Rows | View::Monitor) { 32.0 } else { self.layout.cell_h.max(1.0) }
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
                .filter(|n| toggles::checked_for(Some(n), Some(&snap), Some(&store.menu.checked)))
                .map(|n| n.id.as_str())
                .collect()
        };
        let cells: Vec<&str> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| r.id.as_str()).collect(),
            _ => self.rows.iter().map(|r| r.id.as_str()).collect(),
        };
        let drawn: Vec<Value> = match self.view {
            View::AppGrid => self.rows[..self.app_count].iter().map(|r| json!({"id": r.id, "label": r.label})).collect(),
            View::Rows | View::Monitor | View::Mirror => self.rows.iter().map(|r| json!({"id": r.id, "label": r.label})).collect(),
            View::Emoji | View::Picker => Vec::new(),
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
            "query": w::shown(&self.query),
            "caret": self.query.caret_chars().0,
            "selection": self.query.selection().is_some(),
            "sections": self.section_names(),
            "view": self.view.name(),
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
            "abbrev": self.rows.get(self.cursor).and_then(|n| self.lookup.source_of(&fs_menu::actions::subject_id(n))).map(fs_menu::abbrev::Source::as_str),
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
                let when = n.when.clone().filter(|c| !toggles::is_state_condition(c)).map(|c| (n.id.clone(), c, true));
                let checked = n.checked.clone().filter(|c| !toggles::is_state_condition(c)).map(|c| (n.id.clone(), c, false));
                when.into_iter().chain(checked)
            })
            .collect();
        index::ask(Ask::Conds(conds));
        index::ask(Ask::Binds);
        index::ask(Ask::Files);
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
        self.select_prompt = prompt.into();
        self.select_options.clear();
        self.select_token = token.into();
        self.confirm.clear();
        self.query.clear();
        self.open = true;
        self.key.clear();
    }

    pub fn close(&mut self) {
        self.abandon_select();
        if self.on_picker() {
            self.picker_abandon();
        }
        self.monitor_wants = None;
        self.monitor_armed = None;
        if self.on_mirror() {
            crate::services::mirror::command(crate::services::mirror::Cmd::Stream(None));
        }
        self.open = false;
        self.confirm.clear();
        self.column = None;
        self.children.clear();
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
        self.monitor_armed = None;
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
        self.monitor_armed = None;
        if let Some(c) = &mut self.column {
            c.refilter(self.query.text());
            return;
        }
        if self.mode != Mode::Menu || !self.children.is_empty() {
            return;
        }
        let Some((level, rest)) = model::prefixed(store.menu.nodes(), self.query.text()).and_then(|(n, rest)| Some((Self::level_of(store, n)?, rest.to_owned()))) else {
            return;
        };
        self.enter_level(store, Some(level));
        self.query.set(&rest);
    }

    /// The row as a subject, with what the shell knows about it.
    pub fn subject<'a>(&'a self, store: &Store, node: &'a Node) -> fs_menu::actions::Subject<'a> {
        let windows = match &node.entry {
            Some(entry) if node.kind == NodeKind::App => app_windows(store, entry).len(),
            _ => 0,
        };
        let abbrev = self.abbrev_key.as_deref().zip(self.lookup.source_of(&fs_menu::actions::subject_id(node)));
        fs_menu::actions::Subject {
            node,
            windows,
            discrete_gpu: store.info.monitor.cards.iter().any(|c| c.discrete),
            picker_select: self.picker.select,
            abbrev,
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
        if self.mode != Mode::Menu || !matches!(self.view, View::Rows | View::AppGrid | View::Emoji) {
            return false;
        }
        let Some(node) = self.rows.get(index).cloned() else { return false };
        let actions = fs_menu::actions::actions_for(&self.subject(store, &node));
        let kind = node.kind.as_str();
        let asked: Vec<String> = fs_chrome::plugins::launcher_plugins(&store.plugins.resolved.plugins)
            .filter(|p| p.subjects.as_ref().is_some_and(|k| k.iter().any(|k| k == kind)))
            .map(|p| p.id.clone())
            .collect();
        if actions.is_empty() && asked.is_empty() {
            return false;
        }
        let subject = fs_menu::actions::subject_id(&node);
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
        let subject = fs_menu::actions::subject_id(&c.node);
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
                let mut action = Action::new(&a.id, label, Run::Ipc(plugin_send(&id, &plugins::action_event(&subject, &a.id))));
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
        let armed = format!("{}\u{1}{}", node.id, action.id);
        if action.confirm && self.confirm != armed {
            self.confirm = armed;
            self.dirty = true;
            return Out::None;
        }
        self.close_actions();
        self.run(store, node, action)
    }

    /// The column action a first Enter armed, by position in the actions.
    fn column_armed(&self) -> Option<usize> {
        let c = self.column.as_ref()?;
        c.actions.iter().position(|a| self.confirm == format!("{}\u{1}{}", c.node.id, a.id))
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
            return Err(format!("error: row {index} has no actions"));
        }
        let Some((node, action)) = self.column.as_ref().and_then(|c| Some((c.node.clone(), c.actions.iter().find(|a| a.id == id)?.clone()))) else {
            if !on_row {
                self.close_actions();
            }
            return Err(format!("error: no action {id}"));
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
                let Some(mut argv) = target.id.strip_prefix("target-app:").and_then(|id| index::launch_argv(&store.menu, id)) else { return Out::None };
                argv.push(node.path.clone());
                compositor::spawn(&argv);
            }
            Run::Target(TargetKind::Peer) => {
                use crate::services::localsend;
                let Some(peer) = target.id.strip_prefix("peer:").and_then(|i| i.parse::<usize>().ok()).and_then(|i| store.localsend.peers.get(i)) else {
                    return Out::None;
                };
                localsend::command(localsend::Cmd::Send { peer: peer.clone(), payload: localsend::Payload::Path(node.path.clone()) });
            }
            Run::Target(TargetKind::Workspace) => {
                let Some(idx) = target.id.strip_prefix("workspace:").and_then(|i| i.parse::<i64>().ok()) else { return Out::None };
                let windows = match &node.entry {
                    Some(entry) if node.kind == NodeKind::App => app_windows(store, entry),
                    _ => vec![node.path.clone()],
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
        let book = fs_menu::abbrev::forget(&abbreviation_book(store), &old, &subject);
        let book = fs_menu::abbrev::pin(&book, &new, &subject);
        state::set(vec![state::Field::Abbreviations(json!(book))]);
    }

    /// Enter: the row's first action.
    pub fn activate(&mut self, store: &Store, index: usize) -> Out {
        self.close_actions();
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        if matches!(node.kind, NodeKind::Other(ref k) if k == "option") {
            self.complete_select(&node.label);
            return Out::Close;
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
    /// already names it exactly. Emoji keep their own ledger.
    fn picked(&self, store: &Store, node: &Node) {
        if self.mode != Mode::Menu || node.id.starts_with("emoji.") {
            return;
        }
        let id = fs_menu::actions::subject_id(node);
        if !resolves(store, &id) {
            return;
        }
        let now = now_ms();
        // Only an uninstalled app is pruned, once the apps scan has landed:
        // a provider's rows (a network out of range, a stopped player) come
        // back, and before the scan no app resolves at all.
        let apps_known = !store.menu.exec.is_empty();
        let held: Vec<fs_menu::frecency::Record> =
            picks(store).into_iter().filter(|r| !(apps_known && r.id.starts_with("app:")) || resolves(store, &r.id)).collect();
        let mut fields = vec![state::Field::Picks(json!(fs_menu::frecency::record(&held, &id, now, None)))];
        let q = self.list_text();
        let q_norm = fs_menu::search::normalize(q.trim());
        let exact = fs_menu::search::tier(node, &q_norm, &fs_menu::search::slug(&q_norm)) == fs_menu::search::TIER_EXACT;
        if !exact && fs_menu::abbrev::key(q).is_some() {
            let book = fs_menu::abbrev::learn(&abbreviation_book(store), q, &id, now);
            fields.push(state::Field::Abbreviations(json!(book)));
        }
        state::set(fields);
    }

    /// Whether a confirm action on row `id` is armed.
    pub fn confirming(&self, id: &str) -> bool {
        !self.confirm.is_empty() && self.confirm.split('\u{1}').next() == Some(id)
    }

    /// One action on one row. A `confirm` action arms on the first run and
    /// runs on the second; a `keepOpen` one leaves the launcher up.
    pub fn run(&mut self, store: &Store, node: &Node, action: &fs_menu::actions::Action) -> Out {
        use fs_menu::actions::Run;
        let armed = format!("{}\u{1}{}", node.id, action.id);
        if action.confirm && self.confirm != armed {
            self.confirm = armed;
            self.dirty = true;
            return Out::None;
        }
        self.confirm.clear();
        let out = match &action.run {
            Run::Into(id) => {
                self.picked(store, node);
                let into = if *id != fs_menu::actions::subject_id(node) {
                    None
                } else if node.kind == NodeKind::App {
                    Some(Into::Windows(id.clone()))
                } else if fs_menu::providers::is_folder_subject(node) {
                    index::ask(Ask::Folder(node.path.clone()));
                    Some(Into::Folder(node.path.clone()))
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
                let subject = fs_menu::actions::subject_id(node);
                let editing = self.abbrev_key.clone().map(|q| (subject, q));
                self.open_input(prompt, ABBREV_EDIT, false);
                self.editing = editing;
                return Out::None;
            }
            Run::Target(_) | Run::Note => return Out::None,
            Run::Ipc(name) => {
                let (verb, arg) = name.split_once(':').unwrap_or((name, ""));
                match verb {
                    // An abbreviation's own actions are no pick: picking
                    // would teach the pair straight back.
                    "abbrev.forget" | "abbrev.pin" => {
                        let subject = fs_menu::actions::subject_id(node);
                        let book = abbreviation_book(store);
                        let book = if verb == "abbrev.pin" {
                            fs_menu::abbrev::pin(&book, arg, &subject)
                        } else {
                            fs_menu::abbrev::forget(&book, arg, &subject)
                        };
                        state::set(vec![state::Field::Abbreviations(json!(book))]);
                        self.dirty = true;
                        if action.keep_open {
                            return Out::None;
                        }
                        self.close();
                        return Out::Close;
                    }
                    "plugin.send" => {
                        if let Some((id, line)) = arg.split_once(':') {
                            plugins::send(id, line.to_owned());
                        }
                        Out::None
                    }
                    "window.focus" => {
                        compositor::focus_window(arg);
                        Out::None
                    }
                    "window.close" => {
                        compositor::close_window(arg);
                        Out::None
                    }
                    "window.float" => {
                        compositor::send(compositor::Command::ToggleFloating(arg.to_owned()));
                        Out::None
                    }
                    "window.fullscreen" => {
                        compositor::send(compositor::Command::ToggleFullscreen(arg.to_owned()));
                        Out::None
                    }
                    "app.open" => {
                        let Some(entry) = node.entry.as_ref().filter(|e| e.id == arg) else { return Out::None };
                        let target = fs_menu::appmatch::next_window(&app_windows(store, entry), &store.compositor.snapshot.focused_window_id);
                        if !target.is_empty() {
                            compositor::focus_window(&target);
                        } else if let Some(argv) = index::launch_argv(&store.menu, arg) {
                            compositor::spawn(&argv);
                        }
                        Out::None
                    }
                    "app.new" | "app.action" => {
                        let key = match arg.split_once(':') {
                            Some((id, action)) if verb == "app.action" => index::action_key(id, action),
                            _ => arg.to_owned(),
                        };
                        if let Some(argv) = index::launch_argv(&store.menu, &key) {
                            compositor::spawn(&argv);
                        }
                        Out::None
                    }
                    "wallpaper.set" => {
                        self.picked(store, node);
                        return if self.choose_image(store, arg) { Out::Close } else { Out::None };
                    }
                    "monitor.end" => {
                        let Ok(pid) = arg.parse::<u64>() else { return Out::None };
                        self.dirty = true;
                        if self.monitor_armed != Some(pid) {
                            self.monitor_armed = Some(pid);
                            return Out::None;
                        }
                        self.monitor_armed = None;
                        Out::Internal(format!("monitor.kill:{pid}:TERM"))
                    }
                    _ => Out::Internal(name.clone()),
                }
            }
            Run::Command(cmd) => {
                if !cmd.is_empty() {
                    index::ask(Ask::Spawn(index::ipc_command(cmd)));
                }
                Out::None
            }
        };
        if action.paste_after {
            self.paste = true;
        }
        self.picked(store, node);
        if node.id.starts_with("emoji.") {
            let uses: Vec<fs_menu::frecency::Record> = serde_json::from_value(store.state.data.emoji_uses.clone()).unwrap_or_default();
            let next = fs_menu::frecency::record(&uses, &node.id, now_ms(), None);
            state::set(vec![state::Field::EmojiUses(serde_json::to_value(next).unwrap_or_default())]);
        }
        if action.keep_open {
            return out;
        }
        self.close();
        match out {
            Out::Internal(name) => Out::Internal(name),
            _ => Out::Close,
        }
    }

    /// One key, through `nav::key_action`, or into the field as `edit`.
    pub fn key(&mut self, store: &Store, key: nav::Key, edit: field::Key, mods: nav::Modifiers, repeat: bool, text: Option<&str>) -> Out {
        if self.on_mirror() && matches!(key, nav::Key::Tab | nav::Key::Return | nav::Key::Enter) {
            self.mirror_cycle(store, 1);
            return Out::None;
        }
        let column = self.column.is_some();
        let caret_end = self.query.caret() == self.query.text().len() && self.query.selection().is_none();
        let into = key == nav::Key::Right
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
            variants: self.picker_switch(store),
            level: self.level.is_some() || !self.children.is_empty(),
            column,
            into,
        };
        let action = nav::key_action(key, mods, repeat, &ctx);
        if column {
            match action {
                KeyAction::Up => self.column_move(Dir::Up),
                KeyAction::Down => self.column_move(Dir::Down),
                KeyAction::PageUp => self.column_move(Dir::PageUp),
                KeyAction::PageDown => self.column_move(Dir::PageDown),
                KeyAction::Home => self.column_move(Dir::Home),
                KeyAction::End => self.column_move(Dir::End),
                KeyAction::Back => self.column_back(),
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
            KeyAction::Variant => {
                let light = !self.picker.light;
                self.set_picker_variant(store, light);
                Out::None
            }
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
    fn edit(&mut self, store: &Store, edit: field::Key, mods: nav::Modifiers, text: Option<&str>) -> Out {
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
                let uses: Vec<fs_menu::frecency::Record> = serde_json::from_value(store.state.data.emoji_uses.clone()).unwrap_or_default();
                return Value::Array(index.rows(q, true, &uses, None).iter().map(|n| json!({"id": n.id, "label": n.label, "icon": n.icon, "kind": n.kind.as_str()})).collect());
            }
            Some("keybinds") => {
                index::ask(Ask::Binds);
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                return noted(fs_menu::keybinds::rows_for(reply, q));
            }
            Some("radio.search") => return noted(radio_search_rows(store, q)),
            _ => {}
        }
        let snap = snapshot(store);
        let picks = picks(store);
        let book = abbreviation_book(store);
        let lookup = fs_menu::abbrev::lookup(&configured_abbreviations(store), &book, q, now_ms(), |id| resolves(store, id));
        let boost = fs_menu::search::Boost { abbrev: Some(&lookup), picks: &picks, now: now_ms() };
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

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

pub fn picks(store: &Store) -> Vec<fs_menu::frecency::Record> {
    serde_json::from_value(store.state.data.picks.clone()).unwrap_or_default()
}

/// The app subjects' picks, keyed by desktop id as the apps source orders by.
pub fn launches(store: &Store) -> Vec<fs_menu::frecency::Record> {
    picks(store)
        .into_iter()
        .filter_map(|r| Some(fs_menu::frecency::Record { id: r.id.strip_prefix("app:")?.to_owned(), ..r }))
        .collect()
}

pub fn abbreviation_book(store: &Store) -> fs_menu::abbrev::Book {
    serde_json::from_value(store.state.data.abbreviations.clone()).unwrap_or_default()
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

/// Whether a subject id names something in the tree now.
fn resolves(store: &Store, id: &str) -> bool {
    model::subject_node(store.menu.nodes(), id).is_some()
}

/// The compositor's windows an app's entry claims.
fn app_windows(store: &Store, entry: &fs_menu::node::DesktopEntry) -> Vec<String> {
    let windows: Vec<fs_menu::appmatch::Window> = store
        .compositor
        .snapshot
        .windows
        .iter()
        .map(|w| fs_menu::appmatch::Window { id: w.id.clone(), app_id: w.app_id.clone() })
        .collect();
    fs_menu::appmatch::match_windows(Some(entry), &windows)
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
            path: w.id.clone(),
            icon: app.icon.clone(),
            icon_source: app.icon_source.clone(),
            meta: snap.workspaces.iter().find(|ws| ws.id == w.workspace_id).map(|ws| ws.name.clone()),
            ..Node::new(
                format!("window:{}", w.id),
                if w.title.is_empty() { app.label.clone() } else { w.title.clone() },
                NodeKind::Other(fs_menu::actions::WINDOW_KIND.into()),
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

/// What a target action picks from. Workspaces: the focused output's, by
/// slot, then the next free one as a new one. Apps: every installed one,
/// by name. Peers: LocalSend's last scan, which a listing asks again.
fn targets_for(store: &Store, kind: TargetKind) -> Vec<Node> {
    match kind {
        TargetKind::App => {
            let mut apps: Vec<Node> = model::visible_children(store.menu.nodes(), Some("apps"), &store.menu.cond)
                .into_iter()
                .filter_map(|n| n.entry.as_ref().map(|e| Node::new(format!("target-app:{}", e.id), n.label.clone(), NodeKind::Other("target".into()))))
                .collect();
            apps.sort_by_key(|n| n.label.to_lowercase());
            apps
        }
        TargetKind::Peer => {
            use crate::services::localsend;
            localsend::command(localsend::Cmd::Scan { force: false });
            let peers: Vec<Node> = store
                .localsend
                .peers
                .iter()
                .enumerate()
                .map(|(i, p)| Node { meta: Some(p.ip.clone()), ..Node::new(format!("peer:{i}"), p.name.clone(), NodeKind::Other("target".into())) })
                .collect();
            if peers.is_empty() { vec![Node::note("peer:none", "No devices found")] } else { peers }
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
                ..Node::new(format!("workspace:{idx}"), format!("Workspace {idx}"), NodeKind::Other("workspace".into()))
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

/// A clipboard image row's thumbnail height: twice a body line. The slot is three times as wide and letterboxes.
pub fn thumb_height(theme: &Theme, kit: &mut Kit) -> f64 {
    ui::measure(&w::text("Ag"), 100.0, theme, kit).1 * 2.0
}

fn row_height(node: &Node, theme: &Theme, kit: &mut Kit) -> f64 {
    let s = &theme.space;
    if !node.thumb_source.is_empty() {
        return thumb_height(theme, kit) + s.control_padding_y * 2.0;
    }
    if node.emoji_only {
        let h = ui::measure(&w::text("Ag").size(Type::Display), 100.0, theme, kit).1;
        return h + s.control_padding_y * 2.0;
    }
    launcher_px(theme, fs_theme::tokens::LAUNCHER.row_height)
}

/// A `LAUNCHER` length at the theme's type scale.
fn launcher_px(theme: &Theme, base: f64) -> f64 {
    (base * fs_theme::tokens::font_scale(theme.font_size.base_size)).round()
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
    /// The preview pane's box on the surface while it is drawn, and when
    /// it last arrived.
    pane_rect: Option<Rect>,
    pane_since: Option<Instant>,
    /// The card's width and height travelling between level kinds.
    morph: (Animated, Animated),
    /// One widget tree per row mid-transition, drawn over the body.
    overlays: Vec<Ui>,
    top_node: NodeId,
    /// The card size the content was last laid out for.
    laid_for: (f64, f64),
    /// The cursor's fill and ring, one for the whole body, painted between
    /// `sel_anchor` and the body's own nodes so the cells draw over it.
    sel_anchor: NodeId,
    sel_nodes: Vec<NodeId>,
    /// Its box in body content coordinates (x, y, w, h), travelling.
    sel: [Animated; 4],
    /// The level and query it was last drawn on, while it is shown: a move
    /// inside the same list travels, anything else lands at once.
    sel_on: Option<(Option<String>, String)>,
}

impl Shown {
    /// The launcher's deform amount, lighter than a popout card's.
    pub const DEFORM_AMOUNT: f64 = 0.1;

    pub fn new(theme: &Theme, mut modal: Modal, output: (f64, f64), scale: f64) -> Self {
        let top = modal.layer.as_ref().map_or(modal.card.top_node(), |l| l.top);
        let scene = match &mut modal.layer {
            Some(l) => &mut l.scene,
            None => &mut modal.card.scene,
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
            pane_rect: None,
            pane_since: None,
            overlays: Vec::new(),
            top_node: top,
            laid_for: (0.0, 0.0),
            morph: (Animated::new(theme.space.popup_width_menu, Clock::Spatial.curve()), Animated::new(0.0, Clock::Spatial.curve())),
            sel_anchor,
            sel_nodes: Vec::new(),
            sel: [sel(), sel(), sel(), sel()],
            sel_on: None,
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.modal.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.modal.animating(now)
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
        let card = self.modal.layer.is_none() && (self.modal.animating(now) || self.morph.0.running(now) || self.morph.1.running(now));
        card || self.scroll.running(now) || self.column_scroll.running(now) || self.wake.is_some_and(|w| w <= now)
    }

    /// The field's line, the head band's height under the card's top and
    /// the foot band's, each band with its rule.
    fn metrics(theme: &Theme, m: &Model, output_h: f64, kit: &mut Kit) -> (f64, f64, f64, f64) {
        let s = &theme.space;
        let line = ui::measure(&w::text("Ag").size(Type::Field), 100.0, theme, kit).1;
        let line = if m.mode == Mode::Menu && m.level.is_some() { line.max(s.control_height) } else { line };
        let head = s.lg + line + s.lg + theme.border_width;
        let foot = theme.border_width + s.control_height;
        let body = if m.mode == Mode::Input { 0.0 } else { (m.kind_height(theme, output_h) - head - foot).max(0.0) };
        (line, head, foot, body)
    }

    /// The card at this frame of its size morph: centred along the line it
    /// buds off, its near side 30% of the way across the output. On a
    /// layer that is all a frame of the morph does: the content stays laid
    /// out at the size the card is headed for.
    pub fn place_card(&mut self, theme: &Theme, now: Instant) {
        let (w_, h_) = (self.morph.0.value(now).round(), self.morph.1.value(now).round());
        let pad = theme.space.panel_padding;
        let near = |extent: f64, size: f64| {
            let max = extent - size - pad;
            if max < pad { pad } else { (extent * 0.3).clamp(pad, max) }.round()
        };
        let centred = |extent: f64, size: f64| ((extent - size) / 2.0).max(pad).round();
        let (x, y) = match self.modal.card.edge {
            Edge::Left => (near(self.output.0, w_), centred(self.output.1, h_)),
            Edge::Right => (self.output.0 - w_ - near(self.output.0, w_), centred(self.output.1, h_)),
            _ => (((self.output.0 - w_) / 2.0).round(), near(self.output.1, h_)),
        };
        self.modal.place(Rect::new(x, y, x + w_, y + h_), now);
    }

    /// Lays the card out and draws the three bands.
    pub fn layout(&mut self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let (line, head_h, foot_h, body_h) = Self::metrics(theme, m, self.output.1, kit);
        m.relayout(theme, kit);
        let chrome = head_h + foot_h;
        if (m.body_h - body_h).abs() > 0.5 {
            m.body_h = body_h;
            m.follow();
        }
        // A level that changes kind travels into its size;
        // the first frame after an open lands on it.
        let (tw, th) = (m.card_width(theme), chrome + body_h);
        self.modal.look_hint(tw.round() as i32, th.round() as i32);
        if self.modal.surface.mapped && self.modal.open {
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
                c.row_h = launcher_px(theme, fs_theme::tokens::LAUNCHER.row_height);
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
        self.laid_for = (tw, th);
    }

    /// On a layer, the card is morphing toward a size its content is
    /// already laid out for: another layout (a monitor refresh, rows
    /// settling) waits for the morph to land, since on a starved CPU one
    /// layout of a long view outlasts several frames of the morph.
    pub fn hold_layout(&self, now: Instant) -> bool {
        let morphing = self.morph.0.running(now) || self.morph.1.running(now);
        morphing && self.modal.layer.is_some() && self.laid_for == (self.morph.0.target(), self.morph.1.target())
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
            None => (self.modal.card.content.0, self.modal.card.content.1, self.modal.card.content_clip()),
        };
        let s = theme.space.clone();
        let pad = s.panel_padding;
        let (fx, fy, fw) = (frame.x as f64, frame.y as f64, frame.w as f64);
        let inner_w = (fw - pad * 2.0).max(0.0);
        self.wake = None;
        // Where the layer's pixels sit on the output once settled, for the
        // rects reported out.
        if let Some(l) = &mut self.modal.layer {
            l.fit(frame.w, frame.h);
        }
        let on_output = match &self.modal.layer {
            Some(l) => { let r = l.place(&self.modal.card, true); (r.x, r.y) }
            None => (0, 0),
        };
        let scene = match &mut self.modal.layer {
            Some(l) => &mut l.scene,
            None => &mut self.modal.card.scene,
        };
        self.head.motion_scale = self.scale;
        self.body.motion_scale = self.scale;
        self.foot.motion_scale = self.scale;

        // The field band: `lg` round the line, a full-bleed rule under it.
        let head_rect = Rect::new(fx + s.lg, fy + s.lg, fx + fw - s.lg, fy + s.lg + line);
        let head = header(m, store, theme);
        let d = self.head.draw(&head, head_rect, Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }
        let bw = theme.border_width.round().max(1.0) as i32;
        let rule_top = (fy + s.lg + line + s.lg).round() as i32;
        let body_top = rule_top as f64 + theme.border_width;
        let (body_x, body_w) = if m.bleeds() { (fx, fw) } else { (fx + pad, inner_w) };
        let sep = theme.box_style("separator", None).fill;
        let sep = sep.with_alpha(sep.a * alpha);

        // The body.
        let viewport = IRect::new(body_x.round() as i32, body_top.round() as i32, body_w.round() as i32, body_h.round() as i32);
        let body_clip = viewport.intersect(&clip);
        self.body_rect = (body_h > 0.0).then(|| IRect::new(viewport.x + on_output.0, viewport.y + on_output.1, viewport.w, viewport.h));
        let scroll = self.scroll.value(now) + m.touch.value(now);
        let mut overlays = Vec::new();
        let body = if body_h > 0.0 { body_el(m, store, theme, kit, scroll, body_h, &mut overlays) } else { w::space(0.0) };
        self.body.halo_owned = false;
        self.body.fill_owned = true;
        self.body.cursor = None;
        let origin = (body_x, body_top - scroll);
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
                    theme.box_style("launcher.row", Some("selected"))
                } else {
                    let mut b = theme.box_style("cell", Some("selected"));
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
        while self.overlays.len() < overlays.len() {
            self.overlays.push(Ui::new(Some(self.top_node)));
        }
        for (i, ui) in self.overlays.iter_mut().enumerate() {
            match overlays.get(i) {
                Some((el, r)) => {
                    let at = Rect::new(origin.0 + r.x0, origin.1 + r.y0, origin.0 + r.x1, origin.1 + r.y1);
                    ui.draw(el, at, Some(body_clip), alpha, theme, kit, scene, now);
                }
                None => {
                    ui.draw(&w::space(0.0), Rect::new(0.0, 0.0, 0.0, 0.0), Some(body_clip), alpha, theme, kit, scene, now);
                }
            }
        }
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
            let half = (body_w / 2.0).round();
            let r = Rect::new(body_x + half, body_top, body_x + body_w, body_top + body_h);
            let scroll = self.column_scroll.value(now) + c.touch.value(now);
            let (el, settling) = column_el(m, theme, scroll, now, self.scale);
            let clip = ui::irect(r).intersect(&body_clip);
            let d = self.preview.draw(&el, Rect::new(r.x0, r.y0 - scroll, r.x1, r.y1 - scroll), Some(clip), alpha, theme, kit, scene, now);
            if d.animating || settling {
                self.wake = Some(now);
            }
            self.column_rect = Some(r);
            self.pane_since = None;
        } else if let Some(row) = m.pane().filter(|_| body_h > 0.0) {
            let half = (inner_w / 2.0).round();
            let r = Rect::new(fx + pad + half + s.sm, body_top + s.md, fx + pad + inner_w - s.sm, body_top + body_h - s.md);
            let el = preview_el(m, store, row, theme, kit, r.width(), r.height());
            // The pane's arrival fades its content in on `effects` while the
            // card widens, as the actions column's does.
            let since = *self.pane_since.get_or_insert(now);
            let ms = Clock::Effects.ms(theme) * self.scale;
            let t = if ms <= 0.0 { 1.0 } else { (now.saturating_duration_since(since).as_secs_f64() * 1000.0 / ms).min(1.0) };
            let el = if t < 1.0 { el.fade(Clock::Effects.curve().ease(t) as f32) } else { el };
            let d = self.preview.draw(&el, r, Some(body_clip), alpha, theme, kit, scene, now);
            if d.animating || t < 1.0 {
                self.wake = Some(now);
            }
            self.pane_rect = Some(r);
        } else {
            self.pane_since = None;
            self.preview.draw(&w::space(0.0), Rect::new(0.0, 0.0, 0.0, 0.0), Some(body_clip), alpha, theme, kit, scene, now);
        }

        // The foot band: a full-bleed rule, then the selected subject's keys.
        let foot_rule = (body_top + body_h).round() as i32;
        let foot_top = if body_h > 0.0 { foot_rule as f64 + theme.border_width } else { body_top };
        let foot = footer(m, store, theme);
        let d = self.foot.draw(&foot, Rect::new(fx + s.sm, foot_top, fx + fw - s.sm, foot_top + s.control_height), Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        let mut p = Painter::new(scene, &mut self.rules, Some(clip));
        p.rect(IRect::new(frame.x, rule_top, frame.w, bw), sep, 0.0);
        if body_h > 0.0 {
            p.rect(IRect::new(frame.x, foot_rule, frame.w, bw), sep, 0.0);
        }
        p.finish();
    }

    /// What is under the pointer: a body row or cell, a header or footer
    /// control.
    pub fn hit(&self, x: f64, y: f64) -> Option<crate::ui::Hit> {
        let (x, y) = self.local(x, y);
        self.body.hit(x, y).or_else(|| self.preview.hit(x, y)).or_else(|| self.head.hit(x, y)).or_else(|| self.foot.hit(x, y)).cloned()
    }

    /// An output point in the surface's own pixels.
    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        match &self.modal.layer {
            Some(l) => {
                let at = l.place(&self.modal.card, !self.modal.animating(Instant::now()));
                (x - f64::from(at.x), y - f64::from(at.y))
            }
            None => (x, y),
        }
    }

    /// A box on the surface, settled, as a box on the output.
    fn on_output(&self, r: IRect) -> IRect {
        let at = self.modal.layer.as_ref().map_or((0, 0), |l| {
            let p = l.place(&self.modal.card, true);
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
            Some(l) => l.place(&self.modal.card, true),
            None => self.modal.card.live_rect(),
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

fn header(m: &Model, store: &Store, theme: &Theme) -> El {
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
    parts.push(w::edit(&m.query, &m.placeholder(store), Type::Field));
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
                if let Some(p) = bar.primary {
                    hints.push((p.label, vec!["Enter"], "primary"));
                }
                if m.picker_switch(store) {
                    let label = if m.picker.light { "Show dark" } else { "Show light" };
                    hints.push((label.into(), vec!["Ctrl", "Tab"], "variant"));
                } else if matches!(m.view, View::Rows | View::AppGrid | View::Emoji) {
                    hints.push(("Actions".into(), vec!["Tab"], "actions"));
                }
                if !subject.inert() {
                    if m.view == View::Rows && fs_menu::actions::into_action(&subject).is_some() {
                        hints.push(("Into".into(), vec!["Right"], "into"));
                    }
                    let all = fs_menu::actions::actions_for(&subject);
                    if let Some(second) = all.get(1).filter(|_| !m.confirming(&n.id)) {
                        hints.push((second.label.clone(), vec!["Shift", "Enter"], "alternate"));
                    }
                }
            }
        }
    }
    let mut parts = vec![w::spacer()];
    for (label, keys, on) in hints {
        let content = w::row(s.sm, vec![w::text(label).size(Type::Caption).ink(Ink::Muted), w::chord_keys(s, &keys)]);
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
    let icon_px = launcher_px(theme, fs_theme::tokens::LAUNCHER.row_icon);
    let armed = m.column_armed();
    let first = (scroll / row_h).floor().max(0.0) as usize;
    let fits = (c.view_h / row_h).ceil() as usize + 1;
    let last = (first + fits).min(c.shown.len());
    let more = c.more(scroll);
    let ms = Clock::Effects.ms(theme) * scale;
    let progress = |since: Instant| if ms <= 0.0 { 1.0 } else { (now.saturating_duration_since(since).as_secs_f64() * 1000.0 / ms).min(1.0) };
    let joined = c.joined.map(|(from, at)| (from, progress(at)));
    let mut rows = vec![w::space(first.min(last) as f64 * row_h)];
    let mut drawn_to = last;
    for (at, &i) in c.shown.iter().enumerate().skip(first).take(fits) {
        if let Some((_, n)) = more.filter(|(row, _)| *row == at) {
            rows.push(w::cell(w::text(format!("{n} more")).ink(Ink::Dim)).ghost().height(row_h));
            drawn_to = at + 1;
            break;
        }
        let selected = at == c.cursor;
        let (label, icon, trail, dim, destructive) = match &c.target {
            Some((_, targets)) => {
                let t = &targets[i];
                let meta = t.meta.clone().map(|v| w::text(v).size(Type::BodySmall).ink(Ink::Dim));
                (t.label.clone(), None, meta, false, false)
            }
            None => {
                let a = &c.actions[i];
                let label = if armed == Some(i) { format!("Confirm {}?", a.label) } else { a.label.clone() };
                let trail = matches!(a.run, Run::Into(_)).then(|| w::icon("chevron-right").ink(Ink::Dim));
                (label, a.icon.clone(), trail, a.run == Run::Note, a.destructive)
            }
        };
        let mut lead = Vec::new();
        if let Some(name) = icon {
            let ink = if destructive { Ink::Destructive } else { Ink::Fg };
            lead.push(w::icon(name).ink(ink).width(Size::Px(icon_px)).height(icon_px));
        }
        let mut name = w::text(label).weight(if selected { Weight::Semibold } else { Weight::Normal }).elide();
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

/// The body's widgets for the slots inside the viewport, the rest as room.
/// Rows mid-transition go to `overlays` instead, each with its own rect in
/// body coordinates: they cross other rows, which the stacked column cannot.
fn body_el(m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, scroll: f64, view_h: f64, overlays: &mut Vec<(El, Rect)>) -> El {
    let now = Instant::now();
    let s = &theme.space;
    let lay = &m.layout;
    if m.view == View::Mirror {
        return mirror_body(m, store, theme);
    }
    if m.rows.is_empty() {
        let title = match &m.empty {
            Some(n) => n.label.clone(),
            None if m.mode == Mode::Input => String::new(),
            None if !m.query.is_empty() => format!("No results for \u{201C}{}\u{201D}", m.query.text()),
            None => "No results".into(),
        };
        let mut col = vec![w::space((view_h / 2.0 - s.control_height).max(0.0)), w::text(title).ink(Ink::Muted).centred()];
        if let Some(d) = m.empty.as_ref().and_then(|n| n.desc.clone()).filter(|d| !d.is_empty()) {
            col.push(w::caption(d).mono().centred());
        }
        return w::column(s.row_gap, col);
    }
    let lo = scroll - view_h * 0.5;
    let hi = scroll + view_h * 1.5;
    let visible = |y: f64, h: f64| y + h >= lo && y <= hi;
    let snap = snapshot(store);
    let mut items: Vec<(f64, f64, El)> = Vec::new();
    for (text, y, x) in &lay.headings {
        let h = ui::measure(&w::heading(s, text, None, true), 400.0, theme, kit).1;
        if visible(*y, h) {
            items.push((*y, h, w::heading(s, text, None, true).pad_start(s.control_padding_x + x)));
        }
    }
    if m.monitor_strip_h > 0.0 {
        items.push((lay.slots.first().map_or(s.md, |s0| s0.y) - m.monitor_strip_h - s.section_gap, m.monitor_strip_h, monitor_strip(store, theme, &m.monitor_history)));
    }
    if m.picker_switch_h > 0.0 {
        let top = lay.slots.first().map_or(0.0, |s0| s0.y) - m.picker_switch_h - s.row_gap;
        items.push((top, m.picker_switch_h, picker_switch(m).pad_start(s.sm)));
    }
    let cells = m.cells();
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
            proc_row(m, row, i, selected, theme).width(Size::Px(slot.w))
        } else if i < cells && m.view == View::Picker {
            let gutter = s.sm;
            let edge = (slot.w - (gutter + s.sm) * 2.0).max(1.0);
            let image = store.picker.thumbs.get(&row.path).cloned().flatten();
            let pic = w::picture(image, edge).centred();
            w::cell(pic)
                .ghost()
                .selected(selected)
                .interactive()
                .cell_state(|st| st.cursor = selected && m.from_keys)
                .on(format!("row:{i}"))
                .stop(format!("p:{}", row.id))
                .width(Size::Px(slot.w))
                .height(slot.h - gutter * 2.0)
                .pad(gutter, gutter, gutter, gutter)
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
            let alpha = m.motion.alpha(&row.id, now);
            let thumb = (!row.thumb_source.is_empty())
                .then(|| (store.clipboard.thumbs.get(&row.thumb_source).cloned().flatten(), f64::from(store.clipboard.thumb_box.1)));
            let el = menu_row(m, store, row, i, selected, checked, thumb, theme).width(Size::Px(slot.w)).height(slot.h - slot.band);
            if alpha < 1.0 { el.fade(alpha) } else { el }
        };
        let dy = if i >= cells && matches!(m.view, View::Rows | View::AppGrid) { m.motion.offset(&row.id, now) } else { 0.0 };
        if dy.abs() > 0.5 {
            let (left, top) = (slot.x, slot.y + slot.band + dy);
            overlays.push((el, Rect::new(left, top, left + slot.w, top + slot.h - slot.band)));
            continue;
        }
        items.push((slot.y + slot.band, slot.h - slot.band, el.pad_start(0.0)));
    }
    for (node, slot, at) in &m.motion.left {
        let t = RowMotion::progress(*at, m.motion.ms.2, now);
        if t >= 1.0 || !visible(slot.y, slot.h) {
            continue;
        }
        let fade = 1.0 - Clock::EffectsFast.curve().ease(t) as f32;
        let el = menu_row(m, store, node, usize::MAX, false, false, None, theme).width(Size::Px(slot.w)).height(slot.h - slot.band).fade(fade);
        let top = slot.y + slot.band;
        overlays.push((El { on: None, ..el }, Rect::new(slot.x, top, slot.x + slot.w, top + slot.h - slot.band)));
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
    use fs_system::monitor::{format, procs};
    let p = &store.info.procs;
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
            meta: Some(format!("{}\u{1}{}\u{1}{}", r.pid, format::proc_pct(r.cpu_fraction), format::bytes(Some(r.mem_bytes)))),
            actions: vec![
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("end", "End", format!("@ipc:monitor.end:{}", r.pid)) },
                ActionEntry { keep_open: Some(true), ..ActionEntry::new("kill", "Kill", format!("@ipc:monitor.kill:{}:KILL", r.pid)).destructive() },
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
    let m = &store.info.monitor;
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
fn proc_row(m: &Model, row: &Node, i: usize, selected: bool, theme: &Theme) -> El {
    let s = &theme.space;
    let pid = row.id.strip_prefix("proc.").and_then(|p| p.parse::<u64>().ok());
    let armed = pid.is_some() && m.monitor_armed == pid;
    let meta = row.meta.clone().unwrap_or_default();
    let mut nums = meta.split('\u{1}');
    let (pid_s, cpu, mem) = (nums.next().unwrap_or(""), nums.next().unwrap_or(""), nums.next().unwrap_or(""));
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
    let trail = vec![figure(pid_s, 56.0), figure(cpu, 48.0), figure(mem, 56.0)];
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
}

impl PaneKind {
    pub fn name(self) -> &'static str {
        match self {
            PaneKind::Clip => "clipboard",
            PaneKind::File => "file",
            PaneKind::Folder => "folder",
            PaneKind::Window => "window",
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
    } else if matches!(&n.kind, NodeKind::Other(k) if k == fs_menu::actions::WINDOW_KIND) {
        Some(PaneKind::Window)
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

/// The pane's name and facts lines under what it shows.
fn pane_foot_h(theme: &Theme, kit: &mut Kit, w: f64) -> f64 {
    let s = &theme.space;
    ui::measure(&w::text("Ag"), w, theme, kit).1 + ui::measure(&w::caption("Ag").mono(), w, theme, kit).1 + s.row_gap * 2.0
}

/// The pane's picture box for a pane `w` by `h`.
pub fn preview_box(theme: &Theme, kit: &mut Kit, w: f64, h: f64) -> (u32, u32) {
    let pad = theme.space.control_padding_x * 2.0;
    ((w - pad).max(1.0).round() as u32, (h - pane_foot_h(theme, kit, w) - pad).max(1.0).round() as u32)
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
    let picture = |image: Option<crate::scene::Bitmap>| boxed(w::picture(image, bh_f));
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
            let content = match p.map(|p| &p.peek) {
                None => muted(""),
                Some(Peek::Image) => {
                    let got = decoded(&row.path);
                    let facts = join(vec![dims(got.as_ref().and_then(|g| g.1)).unwrap_or_default(), facts.clone()]);
                    return pane_card(picture(got.and_then(|g| g.0)), &row.label, &facts, s);
                }
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

fn pane_card(content: El, title: &str, facts: &str, s: &fs_theme::tokens::Space) -> El {
    let foot = vec![content, w::text(title.to_owned()).elide(), w::caption(facts.to_owned()).mono().elide()];
    w::cell(w::column(s.row_gap, foot).fill()).fill()
}

/// The wallpaper route's Dark | Light switcher.
fn picker_switch(m: &Model) -> El {
    w::segmented(vec!["Dark".into(), "Light".into()], usize::from(m.picker.light)).on("variant")
}

/// A picker cell's picture edge for a slot `w` wide.
pub fn picker_cell_px(theme: &Theme) -> u32 {
    let s = &theme.space;
    let width = s.popup_width_menu - s.panel_padding * 2.0;
    let cell = width / f64::from(fs_theme::tokens::LAUNCHER.picker_columns.max(1));
    (cell - (s.sm + s.sm) * 2.0).max(1.0).round() as u32
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
fn menu_row(m: &Model, store: &Store, row: &Node, i: usize, selected: bool, checked: bool, thumb: Option<(Option<crate::scene::Bitmap>, f64)>, theme: &Theme) -> El {
    let s = &theme.space;
    let confirming = m.confirming(&row.id);
    let icon_px = launcher_px(theme, fs_theme::tokens::LAUNCHER.row_icon);
    let mut lead = Vec::new();
    if let Some((image, h)) = thumb {
        lead.push(w::picture(image, h).width(Size::Px(h * 3.0)));
    }
    // A plugin's rows and its mode carry icon names from the active set,
    // where every other node's own icon is a raw glyph.
    let glyph = match fs_menu::icons::icon_for(Some(row)) {
        "" if row.id.starts_with(fs_chrome::plugins::PLUGIN_PREFIX) => row.icon.as_str(),
        named => named,
    };
    let picture = (!row.icon_source.is_empty() && glyph.is_empty())
        .then(|| store.menu.row_icons.get(&row.icon_source).cloned().flatten())
        .flatten();
    let drawn = if !glyph.is_empty() {
        glyph.to_owned()
    } else if picture.is_none() && !row.emoji_only && row.thumb_source.is_empty() {
        fs_menu::icons::fallback_for(Some(row)).to_owned()
    } else {
        String::new()
    };
    if let Some(image) = picture {
        let density = image.pixmap.width() as f64 / icon_px.max(1.0);
        lead.push(w::picture(Some(image.at(density)), icon_px));
    } else if !drawn.is_empty() {
        lead.push(w::icon(drawn).size(Type::Heading).width(Size::Px(icon_px)).height(icon_px));
    }
    let label = if confirming { format!("Confirm {}?", row.label) } else { row.label.clone() };
    let mut name = w::text(label).weight(if selected { Weight::Semibold } else { Weight::Normal }).elide();
    if row.dim == Some(true) {
        name = name.ink(Ink::Dim);
    }
    if row.emoji_only {
        name = name.size(Type::Display);
    }
    lead.push(name);
    if let Some(desc) = row.desc.clone().filter(|d| !d.is_empty()) {
        lead.push(w::text(desc).size(Type::BodySmall).ink(Ink::Dim).elide());
    }
    let windows = match &row.entry {
        Some(entry) if row.kind == NodeKind::App && row.meta.as_deref().unwrap_or("").is_empty() => app_windows(store, entry).len(),
        _ => 0,
    };
    let value = match windows {
        0 => fs_menu::actions::accessory_for(Some(row)),
        1 => "1 window".into(),
        n => format!("{n} windows"),
    };
    let mut trail = Vec::new();
    if !value.is_empty() {
        trail.push(w::text(value).size(Type::BodySmall).ink(Ink::Dim));
    }
    if checked && !confirming {
        trail.push(w::icon("check"));
    }
    let mut parts = vec![w::row(s.lg, lead).fill()];
    if !trail.is_empty() {
        parts.push(w::row(s.icon_gap, trail));
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

