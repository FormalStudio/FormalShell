// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! The globe beside a station list and the player, inside
//! the overlay's card. Playback, saved stations and every Radio
//! Browser request belong to the radio service; this is the atlas's own
//! state (mode, lists, selection, search) and how it draws.

pub mod earth;
pub mod globe;
pub mod model;
pub mod view;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use model::{self as rm, EstimateCache};
use fs_media::radio::stations::Station;
use crate::ui::field::{self, Modifiers, Outcome};

use crate::services::clipboard;
use crate::services::radio::{self, Ask, Cmd, Reply};
use globe::Globe;

pub const WORLD_LIMIT: usize = 5000;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
const PREFERRED: (f64, f64) = (1180.0, 760.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    World,
    Cliamp,
    Favorites,
    Recent,
    Search,
    Country,
    Random,
}

impl Mode {
    /// The name `radio status` reports.
    fn name(self) -> &'static str {
        match self {
            Mode::World => "world",
            Mode::Cliamp => "cliamp",
            Mode::Favorites => "favorites",
            Mode::Recent => "recent",
            Mode::Search => "search",
            Mode::Country => "country",
            Mode::Random => "random",
        }
    }

    fn tab(self) -> Option<usize> {
        match self {
            Mode::World => Some(0),
            Mode::Cliamp => Some(1),
            Mode::Favorites => Some(2),
            Mode::Recent => Some(3),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Purpose {
    World,
    Expand,
    Cliamp,
    Search,
    Country,
}

/// One key the atlas reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    Escape,
    Up,
    Down,
    Enter,
    Space,
    Backspace,
    Text(String),
}

pub struct Atlas {
    pub open: bool,
    pub mode: Mode,
    pub world: Vec<Station>,
    pub results: Vec<Station>,
    pub active_code: String,
    pub active_name: String,
    pub help: bool,
    pub outputs: bool,
    pub output_cursor: usize,
    pub selected_index: Option<usize>,
    pub selected: Option<Station>,
    pub keyboard_selection: bool,
    pub fetching: bool,
    pub fetch_error: String,
    token: u64,
    fetch_gen: u64,
    pending: HashMap<u64, (Purpose, u64)>,
    misses: u32,
    expanding: bool,
    pub expand_at: Option<Instant>,
    pub search: field::Field,
    pub editing: bool,
    pub search_at: Option<Instant>,
    countries_asked: bool,
    earth_asked: bool,
    estimates: EstimateCache,
    geo_inputs: Option<(Mode, Vec<String>, usize, bool)>,
    pub geo: Vec<Station>,
    pub globe: Globe,
    pub scroll: f64,
    pub list_h: f64,
    pub row_h: f64,
    geo_world_first: Option<String>,
    pub dirty: bool,
}

impl Default for Atlas {
    fn default() -> Self {
        Self {
            open: false,
            mode: Mode::World,
            world: Vec::new(),
            results: Vec::new(),
            active_code: String::new(),
            active_name: String::new(),
            help: false,
            outputs: false,
            output_cursor: 0,
            selected_index: None,
            selected: None,
            keyboard_selection: false,
            fetching: false,
            fetch_error: String::new(),
            token: 0,
            fetch_gen: 0,
            pending: HashMap::new(),
            misses: 0,
            expanding: false,
            expand_at: None,
            search: field::Field::default(),
            editing: false,
            search_at: None,
            countries_asked: false,
            earth_asked: false,
            estimates: EstimateCache::default(),
            geo_inputs: None,
            geo: Vec::new(),
            globe: Globe::default(),
            scroll: 0.0,
            list_h: 0.0,
            row_h: 0.0,
            geo_world_first: None,
            dirty: true,
        }
    }
}

fn remote(mode: Mode) -> bool {
    !matches!(mode, Mode::Favorites | Mode::Recent)
}

impl Atlas {
    pub fn display<'a>(&'a self, r: &'a radio::State) -> &'a [Station] {
        match self.mode {
            Mode::Favorites => &r.favorites,
            Mode::Recent => &r.recent,
            _ => &self.results,
        }
    }

    fn remote(&self) -> bool {
        remote(self.mode)
    }

    /// `radio status`'s `atlas` block: the list on show and the globe's pose.
    pub fn status(&self, r: &radio::State) -> serde_json::Value {
        serde_json::json!({
            "open": self.open,
            "mode": self.mode.name(),
            "stations": self.display(r).len(),
            "world": self.world.len(),
            "fetching": self.fetching,
            "error": self.fetch_error,
            "globe": {
                "longitude": self.globe.centre_longitude,
                "latitude": self.globe.centre_latitude,
                "coasting": self.globe.kinetic,
                "imagery": self.globe.earth.is_some(),
                "scale": self.globe.scale,
                "earth": {
                    "level": self.globe.earth_stats.level,
                    "tiles": self.globe.earth_stats.tiles,
                    "missing": self.globe.earth_stats.missing,
                    "cached": self.globe.earth_stats.cached,
                    "cachedKb": self.globe.earth_stats.cached_bytes / 1024,
                },
            },
        })
    }

    /// Whether the countries still need asking for, the first time the
    /// atlas opens.
    pub fn want_countries(&mut self) -> bool {
        !std::mem::replace(&mut self.countries_asked, true)
    }

    /// Whether the satellite picture needs decoding for this open: it is
    /// let go when the atlas unmaps.
    pub fn want_earth(&mut self) -> bool {
        if self.globe.earth.is_some() || self.earth_asked {
            return false;
        }
        self.earth_asked = true;
        true
    }

    fn ask(&mut self, purpose: Purpose, ask: Ask) -> u64 {
        self.token += 1;
        let id = self.token;
        self.pending.insert(id, (purpose, self.fetch_gen));
        radio::send(Cmd::Atlas(id, ask));
        id
    }

    pub fn opened(&mut self, r: &radio::State, now: Instant) {
        self.open = true;
        self.misses = 0;
        self.fetch_error.clear();
        if self.world.is_empty() {
            self.show_world(r, true);
        }
        self.schedule_expansion(now, 800);
        self.dirty = true;
    }

    pub fn closed(&mut self) {
        self.open = false;
        self.help = false;
        self.outputs = false;
        self.editing = false;
        self.globe.stop(true);
        self.expand_at = None;
    }

    pub fn toggle_controls(&mut self) {
        self.help = !self.help;
        self.outputs = false;
        self.editing = false;
    }

    pub fn toggle_outputs(&mut self, r: &radio::State) {
        self.outputs = !self.outputs;
        if !self.outputs {
            return;
        }
        let current = std::iter::once("").chain(r.outputs.iter().map(|(id, _)| id.as_str())).position(|id| id == r.output);
        self.output_cursor = current.unwrap_or(0);
        radio::send(Cmd::RefreshOutputs);
    }

    pub fn select_output(&mut self, id: String) {
        radio::send(Cmd::SetOutput(id));
        self.outputs = false;
    }

    fn set_search_text(&mut self, text: &str) {
        self.search.set(text);
    }

    pub fn highlight_station_country(&mut self, station: Option<&Station>, focus: bool) {
        let Some(station) = station else {
            self.active_code.clear();
            self.active_name.clear();
            return;
        };
        self.active_code = station.country_code.to_uppercase();
        self.active_name = if station.country.is_empty() { self.active_code.clone() } else { station.country.clone() };
        self.globe.active_country = self.active_code.clone();
        if !focus {
            return;
        }
        match (station.latitude, station.longitude) {
            (Some(lat), Some(lon)) if lat.is_finite() && lon.is_finite() => self.globe.focus_coordinate(lat, lon),
            _ if !self.active_code.is_empty() => {
                let code = self.active_code.clone();
                self.globe.focus_country(&code);
            }
            _ => {}
        }
    }

    fn restore_playing_country(&mut self, r: &radio::State, focus: bool) {
        if r.running() {
            let s = r.station.clone();
            self.highlight_station_country(s.as_ref(), focus);
            return;
        }
        self.active_code.clear();
        self.active_name.clear();
    }

    pub fn set_selection(&mut self, r: &radio::State, index: Option<usize>, keyboard: bool) {
        self.keyboard_selection = keyboard;
        let stations = self.display(r);
        let Some(index) = index.filter(|_| !stations.is_empty()) else {
            self.selected_index = None;
            self.selected = None;
            return;
        };
        let index = index.min(stations.len() - 1);
        self.selected = Some(stations[index].clone());
        self.selected_index = Some(index);
        self.contain(index);
    }

    /// ListView.positionViewAtIndex(Contain).
    fn contain(&mut self, index: usize) {
        if self.list_h <= 0.0 {
            return;
        }
        let row = self.row_h;
        let top = index as f64 * row;
        if top < self.scroll {
            self.scroll = top;
        } else if top + row > self.scroll + self.list_h {
            self.scroll = top + row - self.list_h;
        }
    }

    pub fn move_selection(&mut self, r: &radio::State, delta: i64) {
        let n = self.display(r).len() as i64;
        if n == 0 {
            return;
        }
        let next = match self.selected_index {
            None => if delta < 0 { n - 1 } else { 0 },
            Some(i) => (i as i64 + delta + n) % n,
        };
        self.set_selection(r, Some(next as usize), true);
    }

    fn set_list(&mut self, r: &radio::State, mode: Mode, stations: Vec<Station>) {
        self.mode = mode;
        let any = !stations.is_empty();
        if remote(mode) {
            self.results = stations;
        }
        self.scroll = 0.0;
        self.set_selection(r, any.then_some(0), false);
    }

    fn refresh_local_selection(&mut self, r: &radio::State) {
        if self.remote() {
            return;
        }
        let stations = self.display(r);
        let playing = r.station.as_ref().map(|s| s.uuid.clone()).unwrap_or_default();
        let preferred = self.selected.as_ref().map_or(playing, |s| s.uuid.clone());
        let mut index = if preferred.is_empty() { None } else { rm::index_by_uuid(stations, &preferred) };
        if index.is_none() && !stations.is_empty() {
            index = Some(self.selected_index.unwrap_or(0).min(stations.len() - 1));
        }
        self.set_selection(r, index, false);
    }

    fn begin_fetch(&mut self) {
        self.fetching = true;
        self.fetch_error.clear();
        self.fetch_gen += 1;
    }

    fn unavailable(&self, r: &radio::State) -> String {
        if self.display(r).is_empty() {
            "Radio Browser is unavailable. Try again shortly.".into()
        } else {
            "Showing cached stations · Radio Browser is unavailable".into()
        }
    }

    pub fn show_world(&mut self, r: &radio::State, refresh: bool) {
        self.search_at = None;
        self.set_search_text("");
        self.outputs = false;
        self.restore_playing_country(r, false);
        radio::send(Cmd::ClearLocalError);
        let world = self.world.clone();
        self.set_list(r, Mode::World, world);
        if !refresh {
            self.fetching = false;
            self.fetch_gen += 1;
            return;
        }
        self.begin_fetch();
        self.ask(Purpose::World, Ask::World);
    }

    pub fn show_cliamp(&mut self, r: &radio::State) {
        self.search_at = None;
        self.set_search_text("");
        self.outputs = false;
        self.restore_playing_country(r, false);
        radio::send(Cmd::ClearLocalError);
        self.set_list(r, Mode::Cliamp, Vec::new());
        self.begin_fetch();
        self.ask(Purpose::Cliamp, Ask::Cliamp);
    }

    fn show_local(&mut self, r: &radio::State, mode: Mode) {
        self.search_at = None;
        self.set_search_text("");
        self.outputs = false;
        self.fetch_gen += 1;
        self.fetching = false;
        self.fetch_error.clear();
        self.mode = mode;
        self.scroll = 0.0;
        self.restore_playing_country(r, true);
        let any = !self.display(r).is_empty();
        self.set_selection(r, any.then_some(0), false);
    }

    pub fn show_tab(&mut self, r: &radio::State, i: usize) {
        match i {
            1 => self.show_cliamp(r),
            2 => self.show_local(r, Mode::Favorites),
            3 => self.show_local(r, Mode::Recent),
            _ => self.show_world(r, true),
        }
    }

    fn preview_search(&mut self, r: &radio::State, text: &str) -> bool {
        let query = text.trim();
        if query.is_empty() {
            self.show_world(r, true);
            return false;
        }
        self.restore_playing_country(r, false);
        self.fetch_error.clear();
        let found = rm::search_stations(&self.world, query, 500);
        self.set_list(r, Mode::Search, found);
        true
    }

    pub fn search_now(&mut self, r: &radio::State) {
        let query = self.search.text().trim().to_owned();
        if !self.preview_search(r, &query) {
            return;
        }
        self.begin_fetch();
        self.ask(Purpose::Search, Ask::Search(query));
    }

    /// The field's text changed (Input.onTextChanged).
    fn search_edited(&mut self, r: &radio::State, now: Instant) {
        if self.search.text().encode_utf16().count() > 128 {
            let cut: String = String::from_utf16_lossy(&self.search.text().encode_utf16().take(128).collect::<Vec<_>>());
            self.search.set(&cut);
        }
        let text = self.search.text().to_owned();
        if self.preview_search(r, &text) {
            self.search_at = Some(now + SEARCH_DEBOUNCE);
        }
    }

    pub fn browse_country(&mut self, r: &radio::State, code: &str, name: &str) {
        let code = code.to_uppercase();
        if code.len() != 2 || !code.chars().all(|c| c.is_ascii_uppercase()) {
            return;
        }
        let mut name = name.to_owned();
        if name.is_empty()
            && let Some(c) = &self.globe.countries
        {
            name = c.features.iter().filter_map(|f| f.properties.as_ref()).find(|p| p.code.to_uppercase() == code).map(|p| p.name.clone()).unwrap_or_default();
        }
        if name.is_empty() {
            name = code.clone();
        }
        self.search_at = None;
        self.outputs = false;
        let cached = rm::stations_for_country(&self.world, &code, 500);
        self.world = rm::prioritize_stations(&cached, &self.world, WORLD_LIMIT);
        self.set_list(r, Mode::Country, cached);
        self.active_code = code.clone();
        self.globe.active_country = code.clone();
        self.active_name = name.clone();
        self.set_search_text(&name);
        self.editing = false;
        self.begin_fetch();
        let world = self.world.clone();
        self.ask(Purpose::Country, Ask::Country(code, world));
    }

    pub fn tune_random(&mut self, r: &radio::State) {
        self.search_at = None;
        self.set_search_text("");
        self.outputs = false;
        self.set_list(r, Mode::Random, Vec::new());
        self.restore_playing_country(r, false);
        self.begin_fetch();
        radio::send(Cmd::Random);
    }

    pub fn activate_map_station(&mut self, r: &radio::State, station: &Station) {
        let index = match rm::index_by_uuid(self.display(r), &station.uuid) {
            Some(i) => i,
            None => {
                let Some(i) = rm::index_by_uuid(&self.world, &station.uuid) else { return };
                self.show_world(r, false);
                i
            }
        };
        self.set_selection(r, Some(index), false);
        self.play_selected(r);
    }

    pub fn play_selected(&mut self, r: &radio::State) {
        let Some(station) = self.selected.clone() else { return };
        self.highlight_station_country(Some(&station), true);
        if self.remote() {
            radio::send(Cmd::Play(station.clone(), rm::station_window(&self.results, &station.uuid, 500)));
        } else {
            radio::send(Cmd::PlayFromSaved(station, self.display(r).to_vec()));
        }
    }

    pub fn play_pause(&mut self, r: &radio::State) {
        if r.running() {
            radio::send(Cmd::Toggle);
        } else {
            self.play_selected(r);
        }
    }

    fn schedule_expansion(&mut self, now: Instant, delay_ms: u64) {
        if !self.open || self.world.is_empty() || self.world.len() >= WORLD_LIMIT || self.misses >= 3 {
            return;
        }
        self.expand_at = Some(now + Duration::from_millis(delay_ms.max(500)));
    }

    /// The atlas's own timers: the search debounce and the world expansion.
    pub fn tick(&mut self, r: &radio::State, now: Instant) {
        if self.search_at.is_some_and(|t| t <= now) {
            self.search_at = None;
            self.search_now(r);
            self.dirty = true;
        }
        if self.expand_at.is_some_and(|t| t <= now) {
            self.expand_at = None;
            if self.open && !self.expanding && self.world.len() < WORLD_LIMIT {
                self.expanding = true;
                self.ask(Purpose::Expand, Ask::WorldMore);
            }
        }
    }

    pub fn deadline(&self) -> Option<Instant> {
        [self.search_at, self.expand_at].into_iter().flatten().min()
    }

    pub fn empty_text(&self, r: &radio::State) -> String {
        if !self.fetch_error.is_empty() {
            return self.fetch_error.clone();
        }
        if !r.local_error.is_empty() {
            return r.local_error.clone();
        }
        match self.mode {
            Mode::Favorites => "No favorites yet. Select a station and press F.".into(),
            Mode::Recent => "No listening history yet.".into(),
            Mode::Search => format!("No stations match \u{201c}{}\u{201d}.", self.search.text().trim()),
            Mode::Cliamp => "No cliamp channels found.".into(),
            Mode::Country => {
                format!("No working stations found in {}.", if self.active_name.is_empty() { "this country" } else { &self.active_name })
            }
            _ => "No working stations found.".into(),
        }
    }

    /// A Radio Browser answer or the outlines landing.
    pub fn reply(&mut self, r: &radio::State, reply: Reply, now: Instant) {
        self.dirty = true;
        match reply {
            Reply::Countries(c) => self.globe.set_countries(c),
            Reply::Earth(earth) => {
                self.earth_asked = false;
                match earth {
                    Ok(e) if self.open => self.globe.earth = Some(e),
                    Ok(_) => {}
                    Err(e) => eprintln!("radio atlas: no satellite picture, drawing the flat globe: {e}"),
                }
            }
            Reply::Tiles => {}
            Reply::Random(answer) => {
                if self.mode != Mode::Random {
                    return;
                }
                self.fetching = false;
                match answer {
                    Ok(rows) => self.set_list(r, Mode::Random, rows),
                    Err(message) => self.fetch_error = message,
                }
            }
            Reply::Rows(id, rows) => {
                let Some((purpose, issued)) = self.pending.remove(&id) else { return };
                let current = issued == self.fetch_gen;
                match purpose {
                    Purpose::World => {
                        if let Some(rows) = &rows {
                            self.world = rm::merge_stations(&self.world, rows, WORLD_LIMIT);
                        }
                        if !current {
                            return;
                        }
                        self.fetching = false;
                        if rows.is_none() {
                            self.fetch_error = self.unavailable(r);
                            return;
                        }
                        let world = self.world.clone();
                        self.set_list(r, Mode::World, world);
                        self.schedule_expansion(now, 1200);
                    }
                    Purpose::Cliamp | Purpose::Search | Purpose::Country => {
                        let mode = match purpose {
                            Purpose::Cliamp => Mode::Cliamp,
                            Purpose::Search => Mode::Search,
                            _ => Mode::Country,
                        };
                        if !current || self.mode != mode {
                            return;
                        }
                        self.fetching = false;
                        let Some(rows) = rows else {
                            self.fetch_error = if mode == Mode::Cliamp {
                                "cliamp radio is unavailable. Try again shortly.".into()
                            } else {
                                self.unavailable(r)
                            };
                            return;
                        };
                        if mode == Mode::Country {
                            let merged = rm::merge_stations(&self.results, &rows, 500);
                            self.world = rm::prioritize_stations(&merged, &self.world, WORLD_LIMIT);
                            self.set_list(r, mode, merged);
                        } else {
                            self.set_list(r, mode, rows);
                        }
                    }
                    Purpose::Expand => {
                        self.expanding = false;
                        if !self.open {
                            return;
                        }
                        self.misses += 1;
                        let rows = rows.unwrap_or_default();
                        if rows.is_empty() {
                            return self.schedule_expansion(now, 30000);
                        }
                        let merged = rm::merge_stations(&self.world, &rows, WORLD_LIMIT);
                        if merged.len() == self.world.len() {
                            return self.schedule_expansion(now, 10000);
                        }
                        self.misses = 0;
                        self.world = merged.clone();
                        if self.mode == Mode::World {
                            self.results = merged;
                        }
                        self.schedule_expansion(now, 1600);
                    }
                }
            }
        }
    }

    /// RadioService's station, favourites or recents moved.
    pub fn radio_changed(&mut self, before: &radio::State, r: &radio::State) {
        self.dirty = true;
        let uuid = |s: &Option<Station>| s.as_ref().map(|s| s.uuid.clone());
        if uuid(&before.station) != uuid(&r.station)
            && let Some(station) = r.station.clone()
        {
            let index = rm::index_by_uuid(self.display(r), &station.uuid);
            if let Some(i) = index
                && Some(i) != self.selected_index
            {
                self.set_selection(r, Some(i), false);
            }
            self.highlight_station_country(Some(&station), true);
        }
        if (self.mode == Mode::Favorites && before.favorites != r.favorites) || (self.mode == Mode::Recent && before.recent != r.recent) {
            self.refresh_local_selection(r);
        }
    }

    /// A key into the open search field, with what it types.
    pub fn edit(&mut self, r: &radio::State, key: field::Key, mods: Modifiers, text: Option<&str>, now: Instant) {
        self.dirty = true;
        match self.search.key(key, mods, text) {
            Outcome::Edited => self.search_edited(r, now),
            Outcome::Copy(t) => clipboard::command(clipboard::Cmd::SetText(t)),
            Outcome::Cut(t) => {
                clipboard::command(clipboard::Cmd::SetText(t));
                self.search_edited(r, now);
            }
            Outcome::Paste => clipboard::command(clipboard::Cmd::Paste),
            Outcome::Pass | Outcome::Held => {}
        }
    }

    /// The clipboard's text into the search field, when it is open.
    pub fn paste(&mut self, r: &radio::State, text: &str, now: Instant) -> bool {
        if !self.editing {
            return false;
        }
        self.dirty = true;
        if self.search.paste(text) {
            self.search_edited(r, now);
        }
        true
    }

    /// True when the key was taken; `close` set
    /// when it asked the overlay to close.
    pub fn key(&mut self, r: &radio::State, key: Key, close: &mut bool) -> bool {
        self.dirty = true;
        let help_key = matches!(&key, Key::Text(t) if t == "?");
        if self.editing {
            match key {
                Key::Escape => {
                    if self.search.is_empty() {
                        self.editing = false;
                    } else {
                        self.show_world(r, true);
                    }
                }
                Key::Enter => {
                    self.search_at = None;
                    self.search_now(r);
                }
                _ => return false,
            }
            return true;
        }
        if self.help {
            if key == Key::Escape || help_key {
                self.toggle_controls();
                return true;
            }
            return false;
        }
        if self.outputs {
            let n = r.outputs.len() + 1;
            match key {
                Key::Escape => self.outputs = false,
                Key::Up => self.output_cursor = (self.output_cursor + n - 1) % n,
                Key::Down => self.output_cursor = (self.output_cursor + 1) % n,
                Key::Enter | Key::Space => {
                    let id = if self.output_cursor == 0 { String::new() } else { r.outputs[self.output_cursor - 1].0.clone() };
                    self.select_output(id);
                }
                _ => return false,
            }
            return true;
        }
        match key {
            Key::Escape => {
                if self.search.is_empty() {
                    *close = true;
                } else {
                    self.show_world(r, true);
                }
            }
            _ if help_key => self.toggle_controls(),
            Key::Text(t) if t == "/" => self.editing = true,
            Key::Up => self.move_selection(r, -1),
            Key::Down => self.move_selection(r, 1),
            Key::Enter => self.play_selected(r),
            Key::Space => self.play_pause(r),
            Key::Text(t) if t.eq_ignore_ascii_case("r") => self.tune_random(r),
            Key::Text(t) if t == "+" || t == "=" => radio::send(Cmd::SetVolume(r.volume.step(5))),
            Key::Text(t) if t == "-" => radio::send(Cmd::SetVolume(r.volume.step(-5))),
            Key::Text(t) if t.eq_ignore_ascii_case("m") => radio::send(Cmd::ToggleMute),
            Key::Text(t) if t.eq_ignore_ascii_case("f") && self.selected.is_some() => {
                radio::send(Cmd::ToggleFavorite(self.selected.clone().expect("selected")));
            }
            _ => return false,
        }
        true
    }

    /// currentGeoStations: the world and the list on show, merged and given
    /// a guessed point when they carry none, recomputed when either moved.
    pub fn sync_geo(&mut self, r: &radio::State) {
        let display = self.display(r).to_vec();
        let inputs = (self.mode, display.iter().map(|s| s.uuid.clone()).collect::<Vec<_>>(), self.world.len(), self.globe.countries.is_some());
        if self.geo_inputs.as_ref() == Some(&inputs) && self.geo_world_first == self.world.first().map(|s| s.uuid.clone()) {
            return;
        }
        let empty = Vec::new();
        let features = self.globe.countries.as_ref().map_or(&empty, |c| &c.features);
        self.geo = rm::merge_geo_stations(&mut self.estimates, &self.world, &display, features);
        self.geo_inputs = Some(inputs);
        self.geo_world_first = self.world.first().map(|s| s.uuid.clone());
        self.globe.set_stations(self.geo.clone());
    }
}
