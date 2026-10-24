//! The launcher. One keyboard-exclusive overlay window over the
//! output, a card budding off the top line (a `Card`) to
//! rest at 30% of the output's height, three bands split by full-bleed
//! rules: the header (back chip, search field, close), the body (the app
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

use fs_menu::field;
use fs_menu::model::{self, Mode, SectionCtx};
use fs_menu::nav::{self, Dir, KeyAction, KeyCtx, KeyMode};
use fs_menu::node::{Kind as NodeKind, Node};
use fs_menu::toggles::{self, Snapshot};
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId};
use crate::scroll::{Touchpad, Travel};
use crate::services::menu::{self as index, Ask};
use crate::services::{clipboard, compositor, state};
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
    /// The row last activated and when, for its ghost (`use`).
    used: Option<(String, Instant)>,
    /// `debug motionScale`, which the row transitions run at.
    pub motion_scale: f64,
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
    /// An open still to start the rows' rise (`arrive.rise`).
    fresh: bool,
    /// The rows rising in off the open, the clock scaled.
    rise: Option<(Instant, fs_theme::style::Rows)>,
    /// A level change's glitch (`swap: glitch`): the rows that were there,
    /// the moment, and the clock scaled.
    swap: Option<(Vec<(Node, Slot)>, Instant, fs_theme::style::Glitch)>,
}

const ROW_RESET_LIMIT: usize = 64;

impl RowMotion {
    fn progress(at: Instant, ms: f64, now: Instant) -> f64 {
        if ms <= 0.0 { 1.0 } else { (now.saturating_duration_since(at).as_secs_f64() * 1000.0 / ms).min(1.0) }
    }

    fn running(&self, now: Instant) -> bool {
        self.rise.as_ref().is_some_and(|(at, r)| crate::effects::rising(r, crate::effects::since(*at, now), ROW_RESET_LIMIT))
            || self.swap.as_ref().is_some_and(|(_, at, g)| crate::effects::glitching(g, crate::effects::since(*at, now)))
            || self.moved.values().any(|(_, at)| Self::progress(*at, self.ms.0, now) < 1.0)
            || self.added.values().any(|at| Self::progress(*at, self.ms.1, now) < 1.0)
            || self.left.iter().any(|(_, _, at)| Self::progress(*at, self.ms.2, now) < 1.0)
    }

    /// The slide still owed to a row, in body pixels.
    fn offset(&self, id: &str, now: Instant) -> f64 {
        self.moved.get(id).map_or(0.0, |(dy, at)| dy * (1.0 - Clock::Spatial.curve().ease(Self::progress(*at, self.ms.0, now))))
    }

    /// The `index`th row's shift and opacity off the open's rise and a level
    /// change's glitch.
    fn arrival(&self, index: usize, now: Instant) -> (f64, f64, f32) {
        let (mut dx, mut dy, mut a) = (0.0, 0.0, 1.0);
        if let Some((at, r)) = &self.rise {
            (dy, a) = crate::effects::rise(r, crate::effects::since(*at, now), index);
        }
        if let Some((_, at, g)) = &self.swap {
            let (x, ga) = crate::effects::glitch(g, crate::effects::since(*at, now)).new;
            (dx, a) = (x, a * ga);
        }
        (dx, dy, a)
    }

    /// The rows a level change replaced, while they are still going out.
    fn going(&self, now: Instant) -> Option<(&[(Node, Slot)], f64, f32)> {
        let (rows, at, g) = self.swap.as_ref()?;
        let (dx, a) = crate::effects::glitch(g, crate::effects::since(*at, now)).old?;
        Some((rows.as_slice(), dx, a))
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
            used: None,
            motion_scale: 1.0,
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

impl Model {
    fn key_mode(&self) -> KeyMode {
        match self.mode {
            Mode::Menu => KeyMode::Menu,
            Mode::Select => KeyMode::Select,
            Mode::Input => KeyMode::Input,
        }
    }

    /// The clipboard routes' 50/50 split: the list, then the cursor row's
    /// whole content.
    pub fn split(&self) -> bool {
        self.mode == Mode::Menu && matches!(self.level.as_deref(), Some("clipboard" | "share.history"))
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
        } else if self.split() {
            s.popup_width_menu_split
        } else {
            s.popup_width_menu
        }
    }

    /// The width inside the card's padding.
    pub fn content_width(&self, theme: &Theme) -> f64 {
        self.card_width(theme) - theme.space.panel_padding * 2.0
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
        (w.round(), (w * 3.0 / 4.0).min(self.body_h - s.lg * 2.0).max(1.0).round())
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

    pub fn picker_listing(&self, store: &Store) -> Vec<String> {
        fs_menu::providers::wallpaper_listing(&self.picker_variants(store), self.picker_variant()).to_vec()
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
        match self.level.as_deref().and_then(|l| store.menu.nodes().get(l)) {
            Some(node) => model::prompt_for(Some(node)),
            None => "Type a command or search...".into(),
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

    fn level_node<'a>(&self, store: &'a Store) -> Option<&'a Node> {
        if self.mode != Mode::Menu {
            return None;
        }
        self.level.as_deref().and_then(|l| store.menu.nodes().get(l))
    }

    pub fn level_name(&self, store: &Store) -> String {
        match self.mode {
            Mode::Select => "Select".into(),
            Mode::Input => "Input".into(),
            Mode::Menu => self.level_node(store).map_or_else(|| "Launcher".into(), |n| n.label.clone()),
        }
    }

    fn level_icon(&self, store: &Store) -> String {
        if self.mode != Mode::Menu {
            return "list".into();
        }
        match self.level_node(store) {
            Some(n) => {
                let i = fs_menu::icons::icon_for(Some(n));
                if i.is_empty() { fs_menu::icons::fallback_for(Some(n)).into() } else { i.into() }
            }
            None => "search".into(),
        }
    }

    /// The level and its rows, resolved over the warm index.
    fn resolve_rows(&self, store: &Store) -> (View, Vec<Node>, usize, Option<Node>, bool, String) {
        let menu = self.mode == Mode::Menu;
        let q = self.query.text();
        let level = self.level.as_deref();
        let nodes = store.menu.nodes();
        let cond = &store.menu.cond;
        let emoji_q = if menu { fs_menu::providers::emoji_trigger_query(q) } else { None };
        let keys_q = if menu { fs_menu::keybinds::trigger_query(q) } else { None };
        let radio_q = if menu { fs_menu::providers::radio_trigger_query(q) } else { None };
        let emoji = menu && (level == Some("emoji") || emoji_q.is_some());
        let route_rows = matches!(level, Some("nix" | "keybinds" | "calc" | "radio.search" | "clipboard" | "share.history"));
        let grid_wanted = store.config.bool("menu.appGrid").unwrap_or(true);
        let picker = menu && level == Some(PICKER_ROUTE);
        let monitor = menu && level == Some(MONITOR_ROUTE);
        let mirror = menu && level == Some(MIRROR_ROUTE);
        let emoji = emoji && !picker && !monitor && !mirror;
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
            Mode::Menu if picker => fs_menu::providers::image_rows(&self.picker_listing(store), q),
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
                let eq = emoji_q.unwrap_or(q);
                store.menu.emoji.as_ref().map_or_else(Vec::new, |e| e.rows(eq, paste, &uses, None))
            }
            Mode::Menu if level == Some("keybinds") || keys_q.is_some() => {
                let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
                fs_menu::keybinds::rows_for(reply, keys_q.unwrap_or(q))
            }
            Mode::Menu if level == Some("radio.search") || radio_q.is_some() => radio_search_rows(store, radio_q.unwrap_or(q)),
            Mode::Menu if q.is_empty() => {
                let gated = level.and_then(|l| nodes.get(l)).is_some_and(|n| !model::is_when_visible(n, cond));
                if gated {
                    vec![model::gated_note_row(&nodes[level.unwrap_or_default()])]
                } else {
                    model::visible_children(nodes, level, cond).into_iter().cloned().collect()
                }
            }
            Mode::Menu => {
                let calc = fs_menu::calc::result_node(q);
                if level == Some("calc") {
                    calc.into_iter().collect()
                } else {
                    let ranked = fs_menu::search::rank(nodes, q, cond, level).into_iter().cloned();
                    calc.into_iter().chain(ranked).collect()
                }
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
                .take(fs_theme::tokens::LAUNCHER.root_apps as usize)
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
        let searching = menu && !q.is_empty() && !emoji && !picker && !monitor && !route_rows && keys_q.is_none();
        let key = format!("{:?}\u{1}{}\u{1}{}\u{1}{}", self.mode, level.unwrap_or(""), q, if picker && self.picker.light { "light" } else { "" });
        (view, rows, app_count, empty, searching, key)
    }

    /// Commits the rows, their headings and the cursor.
    pub fn resolve(&mut self, store: &Store, theme: &Theme, kit: &mut Kit) {
        let (view, rows, app_count, empty, searching, key) = self.resolve_rows(store);
        let fresh = key != self.key;
        let level_label = self.level.as_deref().and_then(|l| store.menu.nodes().get(l)).map(|n| n.label.clone()).unwrap_or_default();
        let sections = model::sections_for(
            &rows,
            &SectionCtx {
                mode: Some(self.mode),
                grid: view == View::Emoji,
                cells: app_count,
                searching,
                level: self.level.as_deref(),
                level_label: &level_label,
                nodes: Some(store.menu.nodes()),
            },
        );
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
        self.row_motion(before, view, theme);
        // A key's step resolves the level again before it draws: the cursor
        // landing where the step put it keeps the step's travel.
        let travels = self.travels && !fresh && !view_changed && index == self.cursor;
        self.place(index, travels);
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
        let left_level = self.motion.level.as_ref().is_some_and(|l| *l != self.level);
        self.motion.level = Some(self.level.clone());
        self.motion.view = Some(view);
        let k = self.motion_scale;
        let m = &mut self.motion;
        let c = theme.motion().catalogue;
        if std::mem::take(&mut m.fresh) {
            m.swap = None;
            m.rise = c.rows.map(|r| {
                let clock = fs_theme::style::Clock { duration: r.clock.duration * k, ..r.clock.clone() };
                (now, fs_theme::style::Rows { clock, stagger: r.stagger * k, ..r })
            });
        } else if left_level && let Some(g) = c.swap {
            m.rise = None;
            let mut rows: Vec<(Node, Slot)> = before.values().filter(|(n, _)| n.kind != NodeKind::Note).cloned().collect();
            rows.sort_by(|a, b| a.1.y.total_cmp(&b.1.y));
            m.swap = Some((rows, now, fs_theme::style::Glitch { out: g.out * k, into: g.into * k, ..g }));
        }
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
        self.motion.running(now) || self.touch.running(now)
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
        let width = self.content_width(theme);
        let inset = s.lg;
        let gutter = s.sm;
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
                let columns = if self.view == View::Picker {
                    fs_theme::tokens::LAUNCHER.picker_columns as usize
                } else if self.view == View::Emoji {
                    fs_theme::tokens::LAUNCHER.emoji_columns as usize
                } else {
                    fs_menu::appgrid::columns_for(width, min_cell)
                };
                let cell_w = width / columns.max(1) as f64;
                let name_h = ui::measure(&w::label("Ag"), width, theme, kit).1;
                let hex = self.view == View::AppGrid && theme.habit_str("grid") == "hex";
                let cell_h = if hex {
                    hex_tile(s) + hex_name_gap(s) + name_h + s.control_padding_y * 2.0
                } else if self.view == View::AppGrid {
                    s.control_height * 2.0 + s.row_gap + name_h + s.control_padding_y * 2.0 + gutter * 2.0
                } else {
                    cell_w
                };
                let heading_text = if self.view == View::AppGrid && cells > 0 { self.sections.first().cloned().unwrap_or_default() } else { String::new() };
                // The first row's upper bracket sits outside the hexagon: it
                // clears the heading, or the card's header, by the room the
                // lower one keeps over the name.
                if !heading_text.is_empty() {
                    out.headings.push((heading_text, y, gutter));
                    y += heading_h + if hex { hex_name_gap(s) } else { s.row_gap };
                } else if theme.habit_str("grid") == "hex" && cells > 0 {
                    y += hex_name_gap(s) - s.row_gap;
                }
                for i in 0..cells {
                    let (r, c) = (i / columns.max(1), i % columns.max(1));
                    out.slots.push(Slot { x: c as f64 * cell_w, y: y + r as f64 * cell_h, w: cell_w, h: cell_h, band: 0.0 });
                }
                y += cells.div_ceil(columns.max(1)) as f64 * cell_h;
                out.columns = columns;
                out.cell_w = cell_w;
                out.cell_h = cell_h;
                for i in cells..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, 0.0));
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
                let width = if self.split() { (width / 2.0).round() } else { width };
                if self.monitor_strip_h > 0.0 {
                    y += self.monitor_strip_h + s.section_gap;
                }
                for i in 0..self.rows.len() {
                    let h_text = heading(i, &self.sections);
                    let band = band_of(&h_text, i == 0);
                    if !h_text.is_empty() {
                        out.headings.push((h_text, y + band - heading_h - s.row_gap, 0.0));
                    }
                    let row_h = row_height(&self.rows[i], theme, kit);
                    out.slots.push(Slot { x: side, y, w: width - side * 2.0, h: band + row_h, band });
                    y += band + row_h;
                }
            }
        }
        out.content_h = y + inset;
        self.layout = out;
    }

    /// The smallest scroll that shows the cursor's slot.
    fn follow(&mut self) {
        let Some(slot) = self.layout.slots.get(self.cursor) else {
            self.scroll = 0.0;
            return;
        };
        let (top, h) = (slot.y + slot.band, slot.h - slot.band);
        let view = self.body_h;
        let mut next = self.scroll;
        // The grid's own heading rides with its first row.
        let top = if !matches!(self.view, View::Rows | View::Monitor) && self.cursor < self.cells() && self.cursor < self.layout.columns { 0.0 } else { top };
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
        let now = Instant::now();
        let (next, glide) = match travel {
            Travel::Notches(n) => {
                self.touch.settle();
                ((self.scroll + n * self.wheel_step()).clamp(0.0, self.max_scroll()), true)
            }
            Travel::Pixels { px, finger: true, time } => (self.touch.drag(now, time, self.scroll, self.max_scroll(), px, self.body_h), false),
            Travel::Pixels { px, finger: false, .. } => {
                self.touch.settle();
                ((self.scroll + px).clamp(0.0, self.max_scroll()), false)
            }
            Travel::Lift(time) => {
                self.touch.release(now, time, self.scroll, self.max_scroll(), release_ms, self.motion_scale);
                (self.scroll, false)
            }
            Travel::None => return,
        };
        self.travels = false;
        self.wheeled = glide;
        self.scroll = next;
        self.dirty = true;
    }

    fn max_scroll(&self) -> f64 {
        (self.layout.content_h - self.body_h).max(0.0)
    }

    /// The pointer named a row.
    pub fn set_cursor(&mut self, index: usize) {
        if index == self.cursor {
            return;
        }
        self.place(index, false);
        self.placed = true;
        self.want = self.cursor_id.clone();
        self.confirm.clear();
        self.from_keys = false;
        self.dirty = true;
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
        })
    }

    fn resolve_route(store: &Store, route: &str) -> Option<String> {
        let nodes = store.menu.nodes();
        if nodes.contains_key(route) {
            return Some(route.to_owned());
        }
        nodes.values().find(|n| n.aliases.iter().any(|a| a == route)).map(|n| n.id.clone())
    }

    /// `open(route)`: a fresh session on `route`'s level, or a `:`-led
    /// prefill at the root.
    pub fn open(&mut self, store: &Store, route: Option<&str>) {
        self.abandon_select();
        self.ask_fresh(store);
        let route = route.filter(|r| !r.is_empty());
        let prefill = route.filter(|r| r.starts_with(':')).unwrap_or("").to_owned();
        let mut target = None;
        if prefill.is_empty()
            && let Some(id) = route.and_then(|r| Self::resolve_route(store, r))
            && let Some(node) = store.menu.nodes().get(&id)
        {
            target = match node.kind {
                NodeKind::Submenu | NodeKind::Provider => Some(node.id.clone()),
                NodeKind::Link => Some(node.target.clone().filter(|t| store.menu.nodes().contains_key(t)).unwrap_or(node.id.clone())),
                _ => None,
            };
        }
        self.enter_level(store, target);
        self.query.set(&prefill);
        self.open = true;
        self.motion.fresh = true;
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

    fn pop(&mut self, store: &Store) -> Out {
        let Some(level) = self.level.clone() else {
            self.close();
            return Out::Close;
        };
        let parent = store.menu.nodes().get(&level).and_then(|n| n.parent_id.clone());
        self.enter_level(store, parent);
        Out::None
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

    fn query_changed(&mut self, store: &Store) {
        self.confirm.clear();
        self.monitor_armed = None;
        if self.mode == Mode::Menu && fs_menu::providers::emoji_trigger_query(self.query.text()).is_some() && store.menu.emoji.is_none() {
            index::ask(Ask::Emoji);
        }
    }

    pub fn activate(&mut self, store: &Store, index: usize) -> Out {
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        self.used = Some((node.id.clone(), Instant::now()));
        if self.view == View::Monitor {
            let Some(pid) = node.id.strip_prefix("proc.").and_then(|p| p.parse::<u64>().ok()) else { return Out::None };
            self.dirty = true;
            if self.monitor_armed == Some(pid) {
                self.monitor_armed = None;
                return Out::Internal(format!("monitor.kill:{pid}"));
            }
            self.monitor_armed = Some(pid);
            return Out::None;
        }
        match node.kind {
            NodeKind::Other(ref k) if k == "option" => {
                self.complete_select(&node.label);
                Out::Close
            }
            NodeKind::Action => {
                if node.confirm == Some(true) && self.confirm != node.id {
                    self.confirm = node.id.clone();
                    self.dirty = true;
                    return Out::None;
                }
                let out = run_action(node.action.as_deref().unwrap_or(""));
                if node.paste_after {
                    self.paste = true;
                }
                if node.id.starts_with("emoji.") {
                    let uses: Vec<fs_menu::frecency::Record> = serde_json::from_value(store.state.data.emoji_uses.clone()).unwrap_or_default();
                    let next = fs_menu::frecency::record(&uses, &node.id, now_ms(), None);
                    state::set(vec![state::Field::EmojiUses(serde_json::to_value(next).unwrap_or_default())]);
                }
                if node.keep_open == Some(true) {
                    self.confirm.clear();
                    return out;
                }
                match out {
                    Out::Internal(name) => {
                        self.close();
                        Out::Internal(name)
                    }
                    _ => {
                        self.close();
                        Out::Close
                    }
                }
            }
            NodeKind::App => {
                let Some(entry) = node.entry.clone() else { return Out::None };
                let windows: Vec<fs_menu::appmatch::Window> = store
                    .compositor
                    .snapshot
                    .windows
                    .iter()
                    .map(|w| fs_menu::appmatch::Window { id: w.id.clone(), app_id: w.app_id.clone(), ..Default::default() })
                    .collect();
                let matches = fs_menu::appmatch::match_windows(Some(&entry), &windows);
                let target = fs_menu::appmatch::next_window(&matches, &store.compositor.snapshot.focused_window_id);
                if !target.is_empty() {
                    compositor::focus_window(&target);
                } else if let Some(argv) = index::launch_argv(&store.menu, &entry.id) {
                    compositor::spawn(&argv);
                }
                record_launch(store, &entry.id);
                self.close();
                Out::Close
            }
            NodeKind::Image => {
                let path = node.path.clone();
                if self.choose_image(store, &path) { Out::Close } else { Out::None }
            }
            NodeKind::Submenu | NodeKind::Provider => {
                self.enter_level(store, Some(node.id.clone()));
                Out::None
            }
            NodeKind::Link => {
                let to = node.target.clone().filter(|t| store.menu.nodes().contains_key(t)).unwrap_or(node.id.clone());
                self.enter_level(store, Some(to));
                Out::None
            }
            _ => Out::None,
        }
    }

    /// A row's own alternate, else Enter.
    pub fn activate_alternate(&mut self, store: &Store, index: usize) -> Out {
        let Some(node) = self.rows.get(index).cloned() else { return Out::None };
        if let Some(alt) = node.alternate.as_deref() {
            let out = run_action(alt);
            if node.keep_open != Some(true) {
                self.close();
                return if matches!(out, Out::Internal(_)) { out } else { Out::Close };
            }
            return out;
        }
        self.activate(store, index)
    }

    /// One key, through `nav::key_action`, or into the field as `edit`.
    pub fn key(&mut self, store: &Store, key: nav::Key, edit: field::Key, mods: nav::Modifiers, repeat: bool, text: Option<&str>) -> Out {
        if self.on_mirror() && matches!(key, nav::Key::Tab | nav::Key::Return | nav::Key::Enter) {
            self.mirror_cycle(store, 1);
            return Out::None;
        }
        let ctx = KeyCtx {
            mode: self.key_mode(),
            query: !self.query.is_empty(),
            grid: !matches!(self.view, View::Rows | View::Monitor),
            app_view: false,
            scrollable: false,
            variants: self.picker_switch(store),
        };
        let action = nav::key_action(key, mods, repeat, &ctx);
        let out = match action {
            KeyAction::Pass => {
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
                Out::None
            }
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

    fn dir(&mut self, d: Dir) -> Out {
        self.move_cursor(d);
        Out::None
    }

    /// `debug query`: the live tree ranked, whether or not the window is up.
    pub fn query(&self, store: &Store, q: &str) -> Value {
        if let Some(eq) = fs_menu::providers::emoji_trigger_query(q) {
            let Some(index) = &store.menu.emoji else {
                index::ask(Ask::Emoji);
                return json!([]);
            };
            let uses: Vec<fs_menu::frecency::Record> = serde_json::from_value(store.state.data.emoji_uses.clone()).unwrap_or_default();
            return Value::Array(index.rows(eq, true, &uses, None).iter().map(|n| json!({"id": n.id, "label": n.label, "icon": n.icon, "kind": n.kind.as_str()})).collect());
        }
        if let Some(kq) = fs_menu::keybinds::trigger_query(q) {
            index::ask(Ask::Binds);
            let reply = store.menu.binds.as_ref().map(|r| r.as_deref().map_err(|_| ()));
            return Value::Array(
                fs_menu::keybinds::rows_for(reply, kq)
                    .iter()
                    .map(|n| json!({"id": n.id, "label": n.label, "desc": n.desc.clone().unwrap_or_default(), "kind": n.kind.as_str()}))
                    .collect(),
            );
        }
        if let Some(rq) = fs_menu::providers::radio_trigger_query(q) {
            return Value::Array(
                radio_search_rows(store, rq)
                    .iter()
                    .map(|n| json!({"id": n.id, "label": n.label, "desc": n.desc.clone().unwrap_or_default(), "kind": n.kind.as_str()}))
                    .collect(),
            );
        }
        let nodes = store.menu.nodes();
        let snap = snapshot(store);
        let mut rows: Vec<Value> = fs_menu::search::rank(nodes, q, &store.menu.cond, self.level.as_deref())
            .into_iter()
            .map(|n| {
                json!({
                    "id": n.id, "label": n.label, "kind": n.kind.as_str(), "iconSource": n.icon_source,
                    "checked": toggles::checked_for(Some(n), Some(&snap), Some(&store.menu.checked)),
                    "section": model::search_section_of(Some(nodes), n),
                })
            })
            .collect();
        if let Some(calc) = fs_menu::calc::result_node(q) {
            rows.insert(0, json!({"id": calc.id, "label": calc.label, "kind": calc.kind.as_str(), "section": model::search_section_of(Some(nodes), &calc)}));
        }
        Value::Array(rows)
    }
}

fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

pub fn launches(store: &Store) -> Vec<fs_menu::frecency::Record> {
    serde_json::from_value(store.state.data.app_launches.clone()).unwrap_or_default()
}

fn record_launch(store: &Store, id: &str) {
    let next = fs_menu::frecency::record(&launches(store), id, now_ms(), None);
    state::set(vec![state::Field::AppLaunches(serde_json::to_value(next).unwrap_or_default())]);
}

/// `@ipc:` stays in process, anything else is a shell
/// command spawned through the compositor.
fn run_action(action: &str) -> Out {
    if let Some(name) = action.strip_prefix("@ipc:") {
        return Out::Internal(name.to_owned());
    }
    if !action.is_empty() {
        index::ask(Ask::Spawn(index::ipc_command(action)));
    }
    Out::None
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
    s.control_height
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
    grain: ui::Grain,
    /// The cursor row's initial behind the list (`launcher.letter`), under
    /// everything the body draws.
    letter_anchor: NodeId,
    letter_nodes: Vec<NodeId>,
    /// The initial shown, the one it replaced and when it changed.
    letter: (String, String, Instant),
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
        let letter_anchor = marker(top);
        let sel_anchor = marker(letter_anchor);
        let body_anchor = marker(sel_anchor);
        let sel = || Animated::new(0.0, Clock::SpatialFast.curve());
        Self {
            grain: ui::Grain::default(),
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
            overlays: Vec::new(),
            top_node: top,
            laid_for: (0.0, 0.0),
            morph: (Animated::new(theme.space.popup_width_menu, Clock::Spatial.curve()), Animated::new(0.0, Clock::Spatial.curve())),
            sel_anchor,
            sel_nodes: Vec::new(),
            sel: [sel(), sel(), sel(), sel()],
            sel_on: None,
            letter_anchor,
            letter_nodes: Vec::new(),
            letter: (String::new(), String::new(), Instant::now()),
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.modal.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.modal.animating(now)
            || self.scroll.running(now)
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
        card || self.scroll.running(now) || self.wake.is_some_and(|w| w <= now)
    }

    fn metrics(theme: &Theme, m: &Model, output_h: f64) -> (f64, f64, f64) {
        let s = &theme.space;
        let band = s.control_height + s.panel_padding * 2.0 + theme.border_width;
        let chrome = band * 2.0;
        let body = if m.mode == Mode::Input { 0.0 } else { (m.kind_height(theme, output_h) - chrome).max(0.0) };
        (band, chrome, body)
    }

    /// The card at this frame of its size morph, centred across the
    /// output. On a layer that is all a frame of the morph does: the
    /// content stays laid out at the size the card is headed for.
    pub fn place_card(&mut self, theme: &Theme, now: Instant) {
        let (w_, h_) = (self.morph.0.value(now).round(), self.morph.1.value(now).round());
        let x = ((self.output.0 - w_) / 2.0).round();
        let pad = theme.space.panel_padding;
        let max_top = self.output.1 - h_ - pad;
        let y = if max_top < pad { pad } else { (self.output.1 * 0.3).clamp(pad, max_top) }.round();
        self.modal.place(Rect::new(x, y, x + w_, y + h_), now);
    }

    /// Lays the card out and draws the three bands.
    pub fn layout(&mut self, m: &mut Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant) {
        let (_, chrome, body_h) = Self::metrics(theme, m, self.output.1);
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
        let target = m.scroll;
        let gap = (self.scroll.target() - target).abs();
        if m.wheeled && gap > 0.5 {
            self.scroll.set_on(now, target, Clock::EffectsFast.ms(theme) * self.scale, Clock::EffectsFast.curve());
        } else if m.travels && gap > 0.5 {
            self.scroll.set_on(now, target, Clock::SpatialFast.ms(theme) * self.scale, Clock::SpatialFast.curve());
        } else if !m.wheeled && !m.travels && gap > 0.0 {
            // A coast's tail moves under half a pixel a frame.
            self.scroll.jump(target);
        }
        self.draw(m, store, theme, kit, now, body_h, (tw, th));
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
    fn draw(&mut self, m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, now: Instant, body_h: f64, target: (f64, f64)) {
        // On a layer the content is laid out in the layer's own pixels at
        // the size the card is headed for: the compositor moves, cuts and
        // fades it with the card.
        let (frame, alpha, clip) = match &self.modal.layer {
            Some(_) => {
                let size = IRect::new(0, 0, target.0.round() as i32, target.1.round() as i32);
                (size, 1.0, size)
            }
            None => (self.modal.card.content.0, self.modal.card.content.1, self.modal.card.clip),
        };
        let s = theme.space.clone();
        let pad = s.panel_padding;
        let (fx, fy, fw) = (frame.x as f64, frame.y as f64, frame.w as f64);
        let inner_w = (fw - pad * 2.0).max(0.0);
        let side = s.sm;
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
        let entered = self.modal.card.entered;
        let scene = match &mut self.modal.layer {
            Some(l) => &mut l.scene,
            None => &mut self.modal.card.scene,
        };
        self.head.motion_scale = self.scale;
        self.body.motion_scale = self.scale;
        self.foot.motion_scale = self.scale;

        // The header band.
        let head_rect = Rect::new(fx + pad, fy + pad, fx + pad + inner_w, fy + pad + s.control_height);
        let head = header(m, store, theme);
        let d = self.head.draw(&head, head_rect, Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }
        let bw = theme.border_width.round().max(1.0) as i32;
        let rule_top = (fy + pad + s.control_height + pad).round() as i32;
        let body_top = rule_top as f64 + theme.border_width;
        let sep = theme.box_style("separator", None).fill;
        let sep = sep.with_alpha(sep.a * alpha);

        // The body.
        let viewport = IRect::new((fx + pad).round() as i32, body_top.round() as i32, inner_w.round() as i32, body_h.round() as i32);
        let body_clip = viewport.intersect(&clip);
        self.body_rect = (body_h > 0.0).then(|| IRect::new(viewport.x + on_output.0, viewport.y + on_output.1, viewport.w, viewport.h));
        let scroll = self.scroll.value(now) + m.touch.value(now);
        let mut overlays = Vec::new();
        let body = if body_h > 0.0 { body_el(m, store, theme, kit, scroll, body_h, &mut overlays) } else { w::space(0.0) };
        self.body.halo_owned = false;
        self.body.fill_owned = true;
        self.body.cursor = None;
        self.body.used = m.used.as_ref().and_then(|(id, at)| {
            let key = self.body.stops.iter().find(|st| st.key.split_once(':').is_some_and(|(_, k)| k == id))?.key.clone();
            Some((key, *at))
        });
        let origin = (fx + pad, body_top - scroll);
        let d = self.body.draw(&body, Rect::new(origin.0, origin.1, origin.0 + inner_w, origin.1 + m.layout.content_h), Some(body_clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        if paint_letter(scene, (&mut self.letter_nodes, self.letter_anchor, &mut self.letter), m, theme, kit, (frame, clip, alpha), self.scale, now) {
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
                let mut b = theme.box_style("cell", Some("selected"));
                b.border = None;
                b.wash = None;
                // A grid cell takes the brackets, a row the leading bar.
                let b = if m.cursor < m.cells() {
                    let mut b = theme.with_brackets(b, m.from_keys, true);
                    b.hex = theme.habit_str("grid") == "hex";
                    b
                } else {
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
            let feed = IRect::new((fx + pad + s.sm).round() as i32, (body_top + s.lg).round() as i32, w as i32, h as i32);
            let pic = store.mirror.frame.as_ref().filter(|(id, _)| *id == m.mirror_current).map(|(_, b)| {
                let (bw, bh) = b.size();
                IRect::new(feed.x + (feed.w - bw) / 2, feed.y + (feed.h - bh) / 2, bw, bh)
            });
            let out = |r: IRect| IRect::new(r.x + on_output.0, r.y + on_output.1, r.w, r.h);
            (out(feed), pic.map(out))
        });
        // The split preview: the cursor row's whole content beside the list.
        if m.split() && body_h > 0.0 {
            let half = (inner_w / 2.0).round();
            let r = Rect::new(fx + pad + half + s.sm, body_top + s.lg, fx + pad + inner_w - side, body_top + body_h - s.lg);
            let el = split_preview(m, store, theme, kit, r.width(), r.height());
            let d = self.preview.draw(&el, r, Some(body_clip), alpha, theme, kit, scene, now);
            if d.animating {
                self.wake = Some(now);
            }
        } else {
            self.preview.draw(&w::space(0.0), Rect::new(0.0, 0.0, 0.0, 0.0), Some(body_clip), alpha, theme, kit, scene, now);
        }
        let _ = side;

        // The footer band.
        let foot_rule = (body_top + body_h).round() as i32;
        let foot_top = if body_h > 0.0 { foot_rule as f64 + theme.border_width + pad } else { body_top + pad };
        let foot = footer(m, store, theme);
        let d = self.foot.draw(&foot, Rect::new(fx + pad + side, foot_top, fx + pad + inner_w - side, foot_top + s.control_height), Some(clip), alpha, theme, kit, scene, now);
        if d.animating {
            self.wake = Some(now);
        }

        let mut p = Painter::new(scene, &mut self.rules, Some(clip));
        p.rect(IRect::new(frame.x, rule_top, frame.w, bw), sep, 0.0);
        if body_h > 0.0 {
            p.rect(IRect::new(frame.x, foot_rule, frame.w, bw), sep, 0.0);
        }
        p.finish();
        if self.grain.paint(scene, frame, clip, theme, entered, self.scale, now) {
            self.wake = Some(now);
        }
    }

    /// What is under the pointer: a body row or cell, a header or footer
    /// control.
    pub fn hit(&self, x: f64, y: f64) -> Option<crate::ui::Hit> {
        let (x, y) = match &self.modal.layer {
            Some(l) => {
                let at = l.place(&self.modal.card, !self.modal.animating(Instant::now()));
                (x - f64::from(at.x), y - f64::from(at.y))
            }
            None => (x, y),
        };
        self.body.hit(x, y).or_else(|| self.head.hit(x, y)).or_else(|| self.foot.hit(x, y)).cloned()
    }

    pub fn set_hover(&mut self, path: Option<String>) -> bool {
        let changed = self.body.hover != path;
        self.body.hover = path.clone();
        self.head.hover = path.clone();
        self.foot.hover = path;
        changed
    }
}

/// The cursor row's initial, giant and faint against the card's far
/// corner, crossfading to the next row's: true while it fades. Drawn on
/// a table whose `launcher.letter` has ink, never with the cursor in a grid.
#[allow(clippy::type_complexity)]
fn paint_letter(
    scene: &mut crate::scene::Scene,
    (nodes, anchor, letter): (&mut Vec<NodeId>, NodeId, &mut (String, String, Instant)),
    m: &Model,
    theme: &Theme,
    kit: &mut Kit,
    (frame, clip, alpha): (IRect, IRect, f32),
    scale: f64,
    now: Instant,
) -> bool {
    let b = theme.box_style("launcher.letter", None);
    let initial = m
        .rows
        .get(m.cursor)
        .filter(|_| (m.view == View::Rows || (m.view == View::AppGrid && m.cursor >= m.app_count)) && b.ink.a > 0.0)
        .and_then(|r| r.label.chars().find(|c| c.is_alphanumeric()))
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    if initial != letter.0 {
        let was = std::mem::take(&mut letter.0);
        *letter = (initial, was, now);
    }
    let ms = theme.metric("launcher.letter", "duration") * scale;
    let t = crate::effects::progress(crate::effects::since(letter.2, now), ms);
    let e = Clock::Effects.curve().ease(t) as f32;
    let mut p = Painter::new(scene, nodes, Some(clip)).after(Some(anchor));
    let size = (f64::from(frame.h) * 0.62).round() as f32;
    let style = crate::text::TextStyle {
        family: crate::surfaces::bar::cell::face(theme.faces.display),
        size,
        weight: theme.metric("launcher.letter", "wght") as f32,
        tracking: 0.0,
        wdth: theme.metric("launcher.letter", "wdth") as f32,
        wght: Some(theme.metric("launcher.letter", "wght") as f32),
    };
    for (text, share) in [(letter.1.clone(), 1.0 - e), (letter.0.clone(), e)] {
        if text.is_empty() || share <= 0.0 {
            continue;
        }
        let shaped = kit.shape(&text, style);
        // Past the card's trailing and bottom edges, as the reference's
        // letter is, so it reads as a shape rather than a word.
        let x = frame.right() - shaped.width + (size * 0.06) as i32;
        let y = frame.bottom() - shaped.line_height() + (size * 0.16) as i32;
        p.text(&shaped, (x, y), b.ink.with_alpha(b.ink.a * alpha * share), &[]);
    }
    p.finish();
    t < 1.0
}

fn header(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let mut parts = Vec::new();
    let in_level = m.mode == Mode::Menu && m.level.is_some();
    if in_level {
        parts.push(w::space(s.sm));
        parts.push(w::icon_text_button(m.level_icon(store), m.level_name(store)).variant(ui::Variant::Outline).tip("Back").on("back"));
        parts.push(w::space(s.icon_gap));
    } else {
        parts.push(w::space(s.sm + s.control_padding_x));
    }
    parts.push(w::edit(&m.query, &m.placeholder(store), Type::Subtitle));
    parts.push(w::space(s.icon_gap));
    parts.push(w::icon_button("x").tip("Close").on("close"));
    parts.push(w::space(s.sm));
    w::row(0.0, parts).fill()
}

fn footer(m: &Model, store: &Store, theme: &Theme) -> El {
    let s = &theme.space;
    let node = m.rows.get(m.cursor);
    let bar = fs_menu::actions::action_bar(&fs_menu::actions::ActionCtx {
        mode: m.mode,
        node,
        at_root: m.level.is_none(),
        confirming: !m.confirm.is_empty() && node.is_some_and(|n| n.id == m.confirm),
        clipssh_image: node.is_some_and(|n| n.alternate.is_none() && !n.clipssh_path.is_empty()),
        alternate_label: node.and_then(|n| n.alternate_label.as_deref()).unwrap_or(""),
        ..Default::default()
    });
    let name = if m.view == View::Emoji {
        node.map(|n| {
            let l = n.label.to_lowercase();
            let mut c = l.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .unwrap_or_else(|| m.level_name(store))
    } else {
        m.level_name(store)
    };
    let mut parts = if theme.habit_str("panel") == "console" {
        // The console's status line: the level and what it holds, typing
        // in whenever either changes.
        let count = m.rows.len() - m.rows.iter().filter(|r| r.kind == NodeKind::Note).count();
        let line = format!("{} :: {count} {}", name.to_uppercase(), if count == 1 { "entry" } else { "entries" });
        vec![w::space(s.control_padding_x), w::value(line).ink(Ink::Primary).elide().typed()]
    } else {
        vec![
            w::space(s.control_padding_x),
            w::icon(m.level_icon(store)).ink(Ink::Muted),
            w::space(s.icon_gap),
            w::text(name).size(Type::BodySmall).ink(Ink::Muted).elide(),
        ]
    };
    let mut actions = Vec::new();
    if let Some(p) = &bar.primary {
        actions.push(w::row(0.0, vec![w::button(p.label.clone()).variant(ui::Variant::Ghost).on("primary"), w::chord_keys(s, &p.keys)]));
    }
    for h in &bar.hints {
        actions.push(w::row(s.sm, vec![w::text(h.label.clone()).size(Type::BodySmall).ink(Ink::Muted), w::chord_keys(s, &h.keys)]));
    }
    parts.push(w::row(s.xxl, actions));
    parts.push(w::space(s.control_padding_x));
    // A band a `controlHeight` tall whether or not a button is in it, so
    // the level's name sits on one line on every level.
    parts.push(El::new(ui::el::Kind::Swatch { color: fs_theme::color::Rgba::TRANSPARENT, w: 0.0, h: s.control_height, radius: 0.0, border: false }));
    w::row(0.0, parts).fill()
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
            let (dx, _, a) = m.motion.arrival(0, now);
            let label = w::heading(s, text, None, true).pad_start(s.control_padding_x + x + dx);
            items.push((*y, h, if a < 1.0 { label.fade(a) } else { label }));
        }
    }
    if m.monitor_strip_h > 0.0 {
        items.push((lay.slots.first().map_or(s.lg, |s0| s0.y) - m.monitor_strip_h - s.section_gap, m.monitor_strip_h, monitor_strip(store, theme, &m.monitor_history)));
    }
    if m.picker_switch_h > 0.0 {
        let top = lay.slots.first().map_or(0.0, |s0| s0.y) - m.picker_switch_h - s.row_gap;
        items.push((top, m.picker_switch_h, picker_switch(m).pad_start(s.sm)));
    }
    let cells = m.cells();
    let hex = theme.habit_str("grid") == "hex";
    for (i, slot) in lay.slots.iter().enumerate() {
        if !visible(slot.y, slot.h) {
            continue;
        }
        let row = &m.rows[i];
        let selected = i == m.cursor;
        let el = if i < cells && m.view == View::AppGrid && hex {
            // The tile holds the icon alone and the name sits under it, so
            // the hexagon's slants never cut a name short.
            let gutter = s.sm;
            let tile = hex_tile(s);
            let edge = s.control_height * 2.0;
            let image = store.menu.icons.get(&row.icon_source).cloned().flatten();
            let pic = if image.is_some() {
                w::picture(image, edge)
            } else {
                El::new(ui::el::Kind::Icon { name: "layout-grid".into(), size: Type::Display, ink: Ink::Dim }).width(Size::Px(edge)).height(edge)
            };
            let tile_el = w::cell(w::column(0.0, vec![pic.centred()]))
                .hex()
                .selected(selected)
                .interactive()
                .on(format!("row:{i}"))
                .stop(format!("c:{}", row.id))
                .width(Size::Px(slot.w))
                .height(tile);
            let name = w::label(row.label.clone()).elide().hug().centred().pad(gutter, 0.0, gutter, 0.0);
            w::column(hex_name_gap(s), vec![tile_el, name]).width(Size::Px(slot.w)).key(format!("c:{}", row.id))
        } else if i < cells && m.view == View::AppGrid {
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
                .cell_state(|st| st.cursor = selected && m.from_keys)
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
            let cell = if hex { w::cell(w::column(0.0, vec![pic.hex()])).hex() } else { w::cell(pic).ghost() };
            cell.selected(selected)
                .interactive()
                .cell_state(|st| st.cursor = selected && m.from_keys)
                .on(format!("row:{i}"))
                .stop(format!("p:{}", row.id))
                .width(Size::Px(slot.w))
                .height(slot.h - gutter * 2.0)
                .pad(gutter, gutter, gutter, gutter)
        } else if i < cells {
            let glyph = w::text(row.icon.clone()).size(Type::Display).centred();
            let cell = w::cell(w::column(0.0, vec![glyph]));
            let cell = if hex { cell.hex() } else { cell.ghost() };
            cell.selected(selected)
                .interactive()
                .cell_state(|st| st.cursor = selected && m.from_keys)
                .on(format!("row:{i}"))
                .stop(format!("e:{}", row.id))
                .width(Size::Px(slot.w))
                .height(slot.h)
        } else {
            let checked = toggles::checked_for(Some(row), Some(&snap), Some(&store.menu.checked));
            let alpha = m.motion.alpha(&row.id, now);
            let thumb = (!row.thumb_source.is_empty())
                .then(|| (store.clipboard.thumbs.get(&row.thumb_source).cloned().flatten(), f64::from(store.clipboard.thumb_box.1)));
            let el = menu_row(m, row, i, selected, checked, thumb, theme).width(Size::Px(slot.w));
            if alpha < 1.0 { el.fade(alpha) } else { el }
        };
        let dy = if i >= cells && matches!(m.view, View::Rows | View::AppGrid) { m.motion.offset(&row.id, now) } else { 0.0 };
        let (ax, ay, aa) = m.motion.arrival(i, now);
        let el = if aa < 1.0 { let f = el.fade.unwrap_or(1.0); el.fade(f * aa) } else { el };
        let (dx, dy) = (ax, dy + ay);
        if dy.abs() > 0.5 || dx.abs() > 0.5 {
            let (left, top) = (slot.x + dx, slot.y + slot.band + dy);
            overlays.push((el, Rect::new(left, top, left + slot.w, top + slot.h - slot.band)));
            continue;
        }
        items.push((slot.y + slot.band, slot.h - slot.band, el.pad_start(0.0)));
    }
    if let Some((rows, dx, fade)) = m.motion.going(now) {
        for (node, slot) in rows.iter().filter(|(_, slot)| visible(slot.y, slot.h)) {
            let el = menu_row(m, node, usize::MAX, false, false, None, theme).width(Size::Px(slot.w)).fade(fade);
            let (left, top) = (slot.x + dx, slot.y + slot.band);
            overlays.push((El { on: None, ..el }, Rect::new(left, top, left + slot.w, top + slot.h - slot.band)));
        }
    }
    for (node, slot, at) in &m.motion.left {
        let t = RowMotion::progress(*at, m.motion.ms.2, now);
        if t >= 1.0 || !visible(slot.y, slot.h) {
            continue;
        }
        let fade = 1.0 - Clock::EffectsFast.curve().ease(t) as f32;
        let el = menu_row(m, node, usize::MAX, false, false, None, theme).width(Size::Px(slot.w)).fade(fade);
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
        col.push(w::row(0.0, line).top().height(h));
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
    w::column(0.0, vec![w::space(s.lg), w::row(0.0, vec![w::space(s.sm), feed])])
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
                vec![w::section_label(s, label, None, false), w::text(figure).size(Type::Title).mono().weight(Weight::Semibold), plot],
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

/// The split pane's picture box for an area `w` by `h`.
pub fn preview_box(theme: &Theme, kit: &mut Kit, w: f64, h: f64) -> (u32, u32) {
    let head = ui::measure(&w::section_label(&theme.space, "Ag", None, false), w, theme, kit).1 + theme.space.row_gap;
    let pad = theme.space.control_padding_x * 2.0;
    ((w - pad).max(1.0).round() as u32, (h - head - pad).max(1.0).round() as u32)
}

/// The split preview: "Text" or "Image" and the capture time over the full
/// text, the emoji at display size, or the picture fitted to the pane.
fn split_preview(m: &Model, store: &Store, theme: &Theme, kit: &mut Kit, width: f64, height: f64) -> El {
    let s = &theme.space;
    let Some(row) = m.rows.get(m.cursor).filter(|r| r.kind != NodeKind::Note) else { return w::space(0.0) };
    let image = !row.thumb_source.is_empty();
    let head = w::row(
        s.sm,
        vec![w::section_label(s, if image { "Image" } else { "Text" }, None, false), w::caption(row.time.clone()).mono().ink(Ink::Muted)],
    );
    let body = if image {
        let (bw, bh) = preview_box(theme, kit, width, height);
        let bitmap = store.clipboard.preview.as_ref().filter(|(p, size, _)| *p == row.thumb_source && size.0 == (bw, bh)).and_then(|(_, _, b)| b.clone());
        w::picture(bitmap, f64::from(bw.max(bh))).width(Size::Px(f64::from(bw)))
    } else if row.emoji_only {
        w::text(row.full_text.clone()).size(Type::Display).centred()
    } else {
        let line = ui::measure(&w::text("Ag"), width, theme, kit).1.max(1.0);
        let lines = ((height - line * 2.0) / line).floor().max(1.0) as usize;
        w::para(row.full_text.clone(), Type::Body, Weight::Normal, Ink::Fg, lines)
    };
    // The pane keeps its height whatever it holds: room under a text,
    // and an emoji centred in it.
    let head_h = ui::measure(&w::section_label(s, "Ag", None, false), width, theme, kit).1;
    let body_h = ui::measure(&body, width, theme, kit).1;
    let room = (height - head_h - s.row_gap * 3.0 - s.control_padding_y * 2.0 - body_h).max(0.0);
    let above = if row.emoji_only { (room / 2.0).floor() } else { 0.0 };
    w::cell(w::column(s.row_gap, vec![head, w::space(above), body, w::space(room - above)]).fill()).fill()
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

fn menu_row(m: &Model, row: &Node, i: usize, selected: bool, checked: bool, thumb: Option<(Option<crate::scene::Bitmap>, f64)>, theme: &Theme) -> El {
    let s = &theme.space;
    let confirming = !m.confirm.is_empty() && m.confirm == row.id;
    let mut lead = Vec::new();
    if let Some((image, h)) = thumb {
        lead.push(w::picture(image, h).width(Size::Px(h * 3.0)));
    }
    let glyph = fs_menu::icons::icon_for(Some(row));
    let drawn = if !glyph.is_empty() {
        glyph.to_owned()
    } else if row.icon_source.is_empty() && !row.icon.is_empty() {
        fs_menu::icons::fallback_for(Some(row)).to_owned()
    } else {
        String::new()
    };
    if !drawn.is_empty() {
        lead.push(w::icon(drawn));
    }
    let label = if confirming { format!("Confirm {}?", row.label) } else { row.label.clone() };
    // A console table sets the root's rows as its menu: wide caps, a step up.
    let menu = theme.habit_str("panel") == "console" && m.mode == Mode::Menu && m.level.is_none() && m.query.is_empty() && !row.emoji_only;
    let mut name = if menu { w::label(label.to_uppercase()).size(Type::Subtitle).elide() } else { w::label(label).elide() };
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
    let mut trail = Vec::new();
    let chord = fs_menu::actions::chord_keys_for(Some(row));
    let accessory = fs_menu::actions::accessory_for(Some(row));
    if !chord.is_empty() {
        trail.push(w::chord_keys(s, &chord));
    } else if !accessory.is_empty() {
        let word = row.meta.as_deref().unwrap_or("").is_empty() && matches!(row.kind, NodeKind::App | NodeKind::Action);
        let a = w::caption(accessory).ink(Ink::Dim);
        trail.push(if word { a } else { a.mono() });
    }
    if checked && !confirming {
        trail.push(w::icon("check"));
    }
    let mut parts = vec![w::row(s.icon_gap, lead).fill()];
    if !trail.is_empty() {
        parts.push(w::row(s.icon_gap, trail));
    }
    let _ = Weight::Medium;
    w::cell(w::row(s.control_padding_x, parts).fill())
        .ghost()
        .selected(selected)
        .interactive()
        .cell_state(|st| st.destructive = confirming)
        .on(format!("row:{i}"))
        .stop(format!("r:{}", row.id))
}

/// A hex app tile's height: the icon decodes at twice the control height,
/// and a pointy-topped hexagon holds a centred square of at most 0.63 of
/// its height.
fn hex_tile(s: &fs_theme::tokens::Space) -> f64 {
    (s.control_height * 2.0 / 0.6).round()
}

/// Room under a hex tile for the cursor's lower bracket, which sits outside
/// the hexagon, to clear the name; the same over the first row for its upper
/// one.
fn hex_name_gap(s: &fs_theme::tokens::Space) -> f64 {
    s.row_gap + s.sm * 2.0
}
