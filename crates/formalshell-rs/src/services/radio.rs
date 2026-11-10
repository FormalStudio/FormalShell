// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! The one mpv child, what it is playing, the saved
//! favourites, recents, volume and output, and every Radio Browser request,
//! Radio Atlas's (`Cmd::Atlas`) answered into the store's atlas inbox.
//!
//! mpv runs unsandboxed, so a station URL is only ever checked to be http(s).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use async_io::{Async, Timer};
use fs_media::radio::stations::{self, Station};
use fs_types::Fraction;
use futures_lite::io::{AsyncWriteExt, BufReader, ReadHalf, WriteHalf, split};
use futures_lite::{AsyncBufReadExt, FutureExt, StreamExt};
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;

use super::devices::audio;
use super::http;
use crate::runtime::Ctx;
use crate::store;

const USER_AGENT: &str = "Radio Atlas (FormalShell)";
/// What mpv names its Pipewire stream, which is how an audio graph tells the
/// radio's stream from any other mpv's.
pub const CLIENT_NAME: &str = "FormalShell Radio";
const SAVE_DELAY: Duration = Duration::from_millis(600);
const STOP_GRACE: Duration = Duration::from_millis(1500);
const CONNECT_EVERY: Duration = Duration::from_millis(100);
const CONNECT_TRIES: u32 = 60;

/// mpv's `volume`: a whole percent, capped by `--volume-max=100`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume(i64);

impl Volume {
    /// Rounded to a whole percent in 0..=100, as mpv is asked for it.
    pub fn from_percent(percent: f64) -> Self {
        Self(percent.round().clamp(0.0, 100.0) as i64)
    }

    pub fn percent(self) -> i64 {
        self.0
    }

    /// `by` whole percent from here.
    pub fn step(self, by: i64) -> Self {
        Self::from_percent((self.0 + by) as f64)
    }

    /// The share of a 0..1 track it fills.
    pub fn track(self) -> Fraction {
        Fraction::from_fraction(self.0 as f64 / 100.0)
    }

    pub fn from_track(track: Fraction) -> Self {
        Self::from_percent(track.as_fraction() * 100.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub favorites: Vec<Station>,
    pub recent: Vec<Station>,
    pub volume: Volume,
    pub output: String,
    pub station: Option<Station>,
    pub queue: Vec<Station>,
    pub mpv_paused: bool,
    pub muted: bool,
    pub title: String,
    pub loaded: bool,
    /// A stream that failed or dropped, kept with the station selected and
    /// never retried on its own.
    pub error: String,
    pub player_error: String,
    pub local_error: String,
    /// The launcher's station search: the query
    /// the answer is for, and the stations, `None` when every mirror failed.
    pub search: Option<(String, Option<Vec<Station>>)>,
    /// The audio graph's sinks, node name and label, for the atlas's output
    /// picker.
    pub outputs: Vec<(String, String)>,
    pub outputs_error: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            recent: Vec::new(),
            volume: Volume(70),
            output: String::new(),
            station: None,
            queue: Vec::new(),
            mpv_paused: false,
            muted: false,
            title: String::new(),
            loaded: false,
            error: String::new(),
            player_error: String::new(),
            local_error: String::new(),
            search: None,
            outputs: Vec::new(),
            outputs_error: String::new(),
        }
    }
}

impl State {
    pub fn running(&self) -> bool {
        self.station.is_some()
    }

    pub fn paused(&self) -> bool {
        self.mpv_paused || !self.error.is_empty()
    }

    /// mpv falls back to the stream's file name when the station sends no
    /// title of its own, which is not a track.
    pub fn track_title(&self) -> String {
        let t = self.title.trim();
        let Some(station) = &self.station else { return String::new() };
        let name = station.name.trim();
        let url = station.url.as_str();
        let end = url.find(['?', '#']).unwrap_or(url.len());
        let trimmed = url[..end].trim_end_matches('/');
        let file = trimmed.rsplit('/').next().unwrap_or("");
        if !t.is_empty() && !name.is_empty() && !t.eq_ignore_ascii_case(name) && t != file && t != url {
            t.to_owned()
        } else {
            String::new()
        }
    }

    /// What the bar cell reads beside its icon: the stream's own track title
    /// when it sends one, the station's name otherwise.
    pub fn label(&self) -> String {
        let Some(station) = &self.station else { return String::new() };
        let t = self.track_title();
        let text = if t.is_empty() { station.name.clone() } else { t };
        text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_owned()
    }

    pub fn is_favorite(&self, uuid: &str) -> bool {
        !uuid.is_empty() && self.favorites.iter().any(|s| s.uuid == uuid)
    }

    pub fn output_label(&self, id: &str) -> String {
        self.outputs.iter().find(|(name, _)| name == id).map_or_else(|| id.to_owned(), |(_, label)| label.clone())
    }

    pub fn status(&self) -> Value {
        json!({
            "running": self.running(),
            "paused": self.paused(),
            "loaded": self.loaded,
            "station": self.station.as_ref().map_or("", |s| s.name.as_str()),
            "title": self.title,
            "label": self.label(),
            "volume": self.volume.percent(),
            "muted": self.muted,
            "output": self.output,
            "error": self.error,
            "playerError": self.player_error,
            "queue": self.queue.len(),
            "favorites": self.favorites.len(),
            "recent": self.recent.len(),
        })
    }
}

/// What Radio Atlas asks of Radio Browser, each answer tagged with the
/// token the atlas issued it under.
#[derive(Clone, Debug)]
pub enum Ask {
    World,
    WorldMore,
    /// A country code and the world stations the atlas already holds.
    Country(String, Vec<Station>),
    Search(String),
    Cliamp,
}

#[derive(Clone, Debug)]
pub enum Reply {
    /// `None` when every mirror failed.
    Rows(u64, Option<Vec<Station>>),
    /// The picked list, or why picking failed.
    Random(Result<Vec<Station>, String>),
    Countries(std::sync::Arc<super::countries::Countries>),
    Earth(Result<std::sync::Arc<super::earth::Earth>, String>),
    /// Finer satellite tiles landed for the open atlas.
    Tiles,
}

pub enum Cmd {
    /// A saved station and the list it came from, which becomes its queue.
    PlayFromSaved(Station, Vec<Station>),
    /// A Radio Browser station and the list it was picked from.
    Play(Station, Vec<Station>),
    ToggleMute,
    ClearLocalError,
    RefreshOutputs,
    Atlas(u64, Ask),
    Answered(Reply),
    /// A world (None) or country refresh, answering `token` when it has one.
    Refresh(Option<String>, Option<u64>),
    RefreshDone(Option<String>),
    CliampRead(Option<Vec<Station>>),
    Toggle,
    Next,
    Previous,
    Stop,
    Random,
    SetVolume(Volume),
    /// A sink name, or "" for the default one.
    SetOutput(String),
    RandomRows(Option<Vec<Station>>),
    Refreshed(Vec<Station>),
    /// Saved when it is not a favourite, dropped when it is.
    ToggleFavorite(Station),
    /// A launcher query, run 500ms after the last one asked.
    Search(String),
    Searched(String, Option<Vec<Station>>),
}

static CMD: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

pub fn send(cmd: Cmd) {
    if let Some(tx) = CMD.get() {
        let _ = tx.try_send(cmd);
    }
}

fn runtime_dir() -> Option<PathBuf> {
    crate::paths::session_runtime()
}

fn cache_dir() -> PathBuf {
    crate::paths::cache().join("formalshell").join("radio-atlas")
}

fn cache_path(country: Option<&str>) -> PathBuf {
    match country {
        Some(code) => cache_dir().join("countries").join(format!("{code}.json")),
        None => cache_dir().join("world.json"),
    }
}

/// A cache document and its stations; `None` when it cannot be read.
fn read_cache(path: &std::path::Path) -> Option<(Value, Vec<Station>)> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let rows = doc.get("stations")?.as_array()?.iter().map(Station::from_saved).collect();
    Some((doc, rows))
}

fn write_cache(path: &std::path::Path, rows: &[Station]) {
    let doc = json!({ "fetchedAt": crate::runtime::now_ms() as f64, "stations": rows });
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, format!("{doc}\n")).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn stale_doc(doc: &Value) -> bool {
    stations::stale(doc.get("fetchedAt").and_then(Value::as_f64).unwrap_or(0.0), crate::runtime::now_ms() as f64)
}

fn state_path() -> PathBuf {
    crate::paths::state().join("formalshell").join("radio-atlas.json")
}

async fn fetch(req: &http::Request) -> Option<String> {
    http::get(req).await.map(|r| r.text())
}

/// A small JSON document off a fixed URL, capped at 64 KiB.
fn document(url: &str) -> http::Request {
    http::Request::get(url, 8).connect_timeout(4).max_filesize(65536).https_only().user_agent(USER_AGENT)
}

type Bases = Rc<RefCell<Vec<String>>>;

async fn discover(cache: &Bases) -> Vec<String> {
    if !cache.borrow().is_empty() {
        return cache.borrow().clone();
    }
    let found = match fetch(&document(stations::SERVERS_URL)).await {
        Some(text) => stations::parse_servers(&text, &mut fastrand::f64),
        None => Vec::new(),
    };
    // A failed discovery is not remembered, so the next request asks again.
    if found.is_empty() {
        return vec![stations::API_FALLBACK.to_owned()];
    }
    *cache.borrow_mut() = found.clone();
    found
}

async fn request(cache: &Bases, request: &stations::Request) -> Option<Vec<Station>> {
    request_on(&discover(cache).await, request).await
}

/// One request, tried against each mirror in turn; None once every mirror
/// has failed.
async fn request_on(bases: &[String], request: &stations::Request) -> Option<Vec<Station>> {
    for base in bases {
        let mut req = http::Request::get(format!("{base}{}", request.path), request.max_time())
            .connect_timeout(4)
            .retry_once()
            .max_filesize(stations::MAX_RESPONSE_CHARS as u64)
            .https_only()
            .user_agent(USER_AGENT);
        for (k, v) in &request.params {
            req = req.query(k, v);
        }
        if let Some(rows) = fetch(&req).await.and_then(|t| stations::parse_response(&t, request.max)) {
            return Some(rows);
        }
    }
    None
}

/// A station search's one deadline, every mirror and all three fields
/// together: past it the search reads failed rather than holding its
/// searching row while each mirror runs out curl's own cap in turn.
const SEARCH_BUDGET: Duration = Duration::from_secs(15);

async fn search(cache: &Bases, q: &str) -> Option<Vec<Station>> {
    let asks = async {
        let bases = discover(cache).await;
        let [name, country, tag] = stations::search_requests(q);
        let ((a, b), c) = futures_lite::future::zip(futures_lite::future::zip(request_on(&bases, &name), request_on(&bases, &country)), request_on(&bases, &tag)).await;
        let lists = [a, b, c];
        lists.iter().any(Option::is_some).then(|| stations::union(&lists.map(Option::unwrap_or_default), stations::MAX_RECORDS))
    };
    asks.or(async {
        Timer::after(SEARCH_BUDGET).await;
        None
    })
    .await
}

type Lines =futures_lite::io::Lines<BufReader<ReadHalf<Async<UnixStream>>>>;

struct Mpv {
    child: async_process::Child,
    reader: Option<Lines>,
    writer: Option<WriteHalf<Async<UnixStream>>>,
    attempts: u32,
    connect_at: Option<Instant>,
}

enum Ev {
    Cmd(Option<Cmd>),
    Line(Option<String>),
    Exit,
    Timer,
}

struct Radio {
    ctx: Ctx,
    st: State,
    mpv: Option<Mpv>,
    socket_dir: Option<PathBuf>,
    pending: Vec<Value>,
    stopping: bool,
    resume: Option<(Station, Vec<Station>)>,
    active: bool,
    recorded: String,
    writable: bool,
    save_at: Option<Instant>,
    stop_at: Option<Instant>,
    bases: Bases,
    last_random: String,
    random_busy: bool,
    follow: async_channel::Sender<Cmd>,
    search_want: String,
    search_at: Option<Instant>,
    search_busy: bool,
    /// The world (None) and the countries a refresh is running for.
    refreshing: Vec<Option<String>>,
    /// cliamp's channels, fetched once per session.
    cliamp: Vec<Station>,
}

impl Radio {
    fn publish(&self) {
        self.ctx.publish(store::Diff::Media(super::media::Diff::Radio(self.st.clone())));
    }

    fn socket_path(&self) -> Option<PathBuf> {
        self.socket_dir.as_ref().map(|d| d.join("mpv.sock"))
    }

    fn save_state(&mut self) {
        if !self.writable {
            return;
        }
        let doc = json!({
            "favorites": self.st.favorites,
            "recent": self.st.recent,
            "volume": self.st.volume.percent(),
            "output": self.st.output,
        });
        let path = state_path();
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, format!("{doc}\n"))?;
            std::fs::rename(&tmp, &path)
        };
        if write().is_err() {
            self.st.local_error = "Saved stations could not be written".into();
            self.publish();
        }
    }

    fn load_state(&mut self) {
        match std::fs::read_to_string(state_path()) {
            Ok(text) => match stations::parse_state(&text) {
                Some(saved) => {
                    self.st.favorites = saved.favorites;
                    self.st.recent = saved.recent;
                    self.st.volume = Volume::from_percent(saved.volume as f64);
                    self.st.output = saved.output;
                    self.writable = true;
                }
                None => self.st.local_error = "Saved stations could not be loaded".into(),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.writable = true,
            Err(_) => self.st.local_error = "Saved stations could not be loaded".into(),
        }
    }

    fn mpv_argv(&self, socket: &std::path::Path) -> Vec<String> {
        let mut args: Vec<String> = [
            "mpv", "--no-config", "--no-video", "--force-window=no", "--audio-display=no", "--idle=yes",
            "--load-scripts=no", "--ytdl=no", "--load-unsafe-playlists=no", "--cache-secs=20",
            "--demuxer-max-bytes=8MiB", "--demuxer-max-back-bytes=2MiB", "--network-timeout=30",
        ]
        .map(String::from)
        .to_vec();
        args.push(format!("--volume={}", self.st.volume.percent()));
        args.extend(
            [
                "--volume-max=100",
                "--demuxer-lavf-o=protocol_whitelist=[http,https,tls,tcp]",
                "--stream-lavf-o=protocol_whitelist=[http,https,tls,tcp]",
            ]
            .map(String::from),
        );
        args.push(format!("--input-ipc-server={}", socket.display()));
        args.push("--no-terminal".into());
        args.push(format!("--audio-client-name={CLIENT_NAME}"));
        if !self.st.output.is_empty() {
            args.push(format!("--audio-device=pulse/{}", self.st.output));
        }
        args
    }

    /// The socket's directory has to exist before mpv binds in it, and `exec`
    /// leaves mpv itself as the child.
    fn start_mpv(&mut self) {
        let (Some(dir), Some(socket)) = (self.socket_dir.clone(), self.socket_path()) else {
            self.st.player_error = "Radio Atlas needs XDG_RUNTIME_DIR".into();
            self.pending.clear();
            self.st.station = None;
            return;
        };
        self.stopping = false;
        let argv = self.mpv_argv(&socket);
        let child = crate::services::proc::command("sh")
            .args(["-c", "mkdir -p -m 700 \"$1\" && shift && exec \"$@\"", "sh"])
            .arg(dir)
            .args(argv)
            .stdin(async_process::Stdio::null())
            .stdout(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        match child {
            Ok(child) => {
                self.mpv = Some(Mpv { child, reader: None, writer: None, attempts: 0, connect_at: Some(Instant::now() + CONNECT_EVERY) })
            }
            Err(_) => {
                self.st.player_error = "Could not start the player".into();
                self.pending.clear();
                self.st.station = None;
            }
        }
    }

    async fn write(&mut self, command: Value) {
        let Some(writer) = self.mpv.as_mut().and_then(|m| m.writer.as_mut()) else { return };
        let line = format!("{}\n", json!({ "command": command }));
        let _ = writer.write_all(line.as_bytes()).await;
        let _ = writer.flush().await;
    }

    fn ready(&self) -> bool {
        self.mpv.as_ref().is_some_and(|m| m.writer.is_some())
    }

    async fn send_mpv(&mut self, command: Value) {
        if self.ready() {
            self.write(command).await;
            return;
        }
        self.pending.push(command);
        if self.mpv.is_none() {
            self.start_mpv();
        }
    }

    async fn try_connect(&mut self) {
        let Some(path) = self.socket_path() else { return };
        let Some(mpv) = self.mpv.as_mut() else { return };
        mpv.connect_at = None;
        match Async::<UnixStream>::connect(path).await {
            Ok(stream) => {
                let (r, w) = split(stream);
                mpv.reader = Some(BufReader::new(r).lines());
                mpv.writer = Some(w);
                mpv.attempts = 0;
                for (i, prop) in ["pause", "volume", "mute", "media-title", "idle-active"].iter().enumerate() {
                    self.write(json!(["observe_property", i + 1, prop])).await;
                }
                for command in std::mem::take(&mut self.pending) {
                    self.write(command).await;
                }
            }
            Err(_) => {
                mpv.attempts += 1;
                if mpv.attempts < CONNECT_TRIES {
                    mpv.connect_at = Some(Instant::now() + CONNECT_EVERY);
                } else {
                    self.st.player_error = "Could not reach the player".into();
                    let _ = mpv.child.kill();
                }
            }
        }
    }

    fn on_mpv_exited(&mut self) {
        let unexpected = !self.stopping && self.st.station.is_some();
        let resume = self.resume.take();
        self.active = false;
        self.mpv = None;
        self.pending.clear();
        self.stopping = false;
        self.stop_at = None;
        self.st.station = None;
        self.st.queue.clear();
        self.st.mpv_paused = false;
        self.st.muted = false;
        self.st.title.clear();
        self.st.loaded = false;
        self.st.error.clear();
        self.recorded.clear();
        if unexpected {
            self.st.player_error = "The player stopped unexpectedly".into();
        }
        if let Some((station, list)) = resume {
            self.follow_up(Cmd::PlayFromSaved(station, list));
        }
    }

    fn follow_up(&self, cmd: Cmd) {
        let _ = self.follow.try_send(cmd);
    }

    fn failure(&self) -> String {
        if self.st.loaded { "Stream disconnected" } else { "Station could not be played" }.into()
    }

    fn record_played(&mut self, station: &Station) {
        if station.uuid.is_empty() || !self.writable {
            return;
        }
        self.st.recent.retain(|s| s.uuid != station.uuid);
        self.st.recent.insert(0, stations::saved_record(station));
        self.st.recent.truncate(stations::MAX_RECENT);
        self.save_state();
    }

    fn on_line(&mut self, line: &str) {
        let Ok(msg) = serde_json::from_str::<Value>(line) else { return };
        let Some(event) = msg.get("event").and_then(Value::as_str) else { return };
        match event {
            "property-change" => {
                let data = msg.get("data");
                match msg.get("name").and_then(Value::as_str) {
                    Some("pause") => self.st.mpv_paused = data == Some(&Value::Bool(true)),
                    Some("mute") => self.st.muted = data == Some(&Value::Bool(true)),
                    Some("media-title") => {
                        let raw = data.and_then(Value::as_str).unwrap_or("");
                        self.st.title = raw.chars().map(|c| if c.is_control() { ' ' } else { c }).take(512).collect();
                    }
                    Some("idle-active") => {
                        let idle = data == Some(&Value::Bool(true));
                        if !idle {
                            self.active = true;
                        } else if self.active && self.st.station.is_some() && self.st.error.is_empty() {
                            self.st.error = self.failure();
                        }
                    }
                    Some("volume") => {
                        if let Some(v) = data.and_then(Value::as_f64) {
                            let v = Volume::from_percent(v);
                            if v != self.st.volume {
                                self.st.volume = v;
                                self.save_at = Some(Instant::now() + SAVE_DELAY);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "start-file" => {
                self.st.loaded = false;
                self.st.error.clear();
            }
            "file-loaded" => {
                self.st.loaded = true;
                if let Some(station) = self.st.station.clone()
                    && station.uuid != self.recorded
                {
                    self.recorded = station.uuid.clone();
                    self.record_played(&station);
                }
            }
            "end-file" => {
                let reason = msg.get("reason").and_then(Value::as_str);
                // A live stream has no end, so eof is a dropped connection too.
                if matches!(reason, Some("error" | "eof")) && self.st.station.is_some() {
                    self.st.error = self.failure();
                }
                self.st.loaded = false;
            }
            _ => {}
        }
    }

    async fn play(&mut self, station: Station, list: Vec<Station>) {
        if !stations::playable(&station) {
            self.st.player_error = "Station is unavailable or has an unsafe stream URL".into();
            return;
        }
        let mut rows: Vec<Station> = list.into_iter().filter(stations::playable).collect();
        if !rows.iter().any(|s| s.uuid == station.uuid) {
            rows.insert(0, station.clone());
        }
        if self.stopping {
            self.resume = Some((station, rows));
            return;
        }
        self.st.queue = rows;
        self.st.station = Some(station.clone());
        self.st.error.clear();
        self.st.player_error.clear();
        self.st.title.clear();
        self.st.loaded = false;
        self.send_mpv(json!(["loadfile", station.url, "replace"])).await;
        self.send_mpv(json!(["set_property", "pause", false])).await;
        self.count_fetch(&station.uuid);
    }

    async fn step(&mut self, delta: i64) {
        let Some(current) = self.st.station.clone() else { return };
        let n = self.st.queue.len() as i64;
        if n == 0 {
            return;
        }
        let at = self.st.queue.iter().position(|s| s.uuid == current.uuid).map_or(-1, |i| i as i64);
        let index = (((at + delta) % n) + n) % n;
        let next = self.st.queue[index as usize].clone();
        let list = self.st.queue.clone();
        self.play(next, list).await;
    }

    async fn stop(&mut self) {
        if self.mpv.is_none() {
            return;
        }
        self.stopping = true;
        if self.ready() {
            self.write(json!(["quit"])).await;
            self.stop_at = Some(Instant::now() + STOP_GRACE);
        } else if let Some(mpv) = self.mpv.as_mut() {
            let _ = mpv.child.kill();
        }
    }

    // Every Radio Browser request runs in a task of its own, so a slow mirror
    // never holds up what mpv is saying.

    fn count_fetch(&self, uuid: &str) {
        if !stations::is_uuid(uuid) {
            return;
        }
        let (bases, uuid) = (self.bases.clone(), uuid.to_owned());
        self.ctx.spawn(async move {
            let url = format!("{}/json/url/{uuid}", discover(&bases).await[0]);
            let _ = http::get(&http::Request::get(url, 4).https_only().user_agent(USER_AGENT).output("/dev/null")).await;
        });
    }

    /// radio-player's refresh of a saved list: the stored URL plays at once,
    /// and whatever Radio Browser now says about those stations replaces the
    /// stored records for next time.
    fn refresh_saved(&self, list: &[Station]) {
        let uuids: Vec<String> = list.iter().map(|s| s.uuid.clone()).filter(|u| stations::is_uuid(u)).collect();
        if uuids.is_empty() || uuids.join(",").len() > 4096 {
            return;
        }
        let (bases, follow) = (self.bases.clone(), self.follow.clone());
        self.ctx.spawn(async move {
            if let Some(rows) = request(&bases, &stations::resolve_request(&uuids)).await
                && !rows.is_empty()
            {
                let _ = follow.try_send(Cmd::Refreshed(rows));
            }
        });
    }

    fn tune_random(&mut self) {
        if self.random_busy {
            return;
        }
        self.random_busy = true;
        let (bases, follow) = (self.bases.clone(), self.follow.clone());
        self.ctx.spawn(async move {
            let rows = request(&bases, &stations::random_request()).await;
            let _ = follow.try_send(Cmd::RandomRows(rows));
        });
    }

    fn start_search(&mut self) {
        let q = self.search_want.clone();
        if self.search_busy || q.is_empty() || q.chars().count() > 128 {
            return;
        }
        self.search_busy = true;
        let (bases, follow) = (self.bases.clone(), self.follow.clone());
        self.ctx.spawn(async move {
            let found = search(&bases, &q).await;
            let _ = follow.try_send(Cmd::Searched(q, found));
        });
    }

    async fn random_rows(&mut self, rows: Option<Vec<Station>>) {
        self.random_busy = false;
        let Some(rows) = rows else {
            self.answer(Reply::Random(Err("Radio Browser is unavailable. Try again shortly.".into())));
            return;
        };
        let mut excluded: Vec<String> = Vec::new();
        let mut add = |uuid: &str| {
            if stations::is_uuid(uuid) && !excluded.iter().any(|e| e == uuid) {
                excluded.push(uuid.to_owned());
            }
        };
        add(self.st.station.as_ref().map_or("", |s| s.uuid.as_str()));
        add(&self.last_random);
        for s in &self.st.recent {
            add(&s.uuid);
        }
        excluded.truncate(32);
        let picked = stations::pick_random(&rows, &excluded, &mut fastrand::f64);
        self.answer(Reply::Random(Ok(picked.clone())));
        if let Some(first) = picked.first().cloned() {
            self.last_random = first.uuid.clone();
            self.play(first, picked).await;
        }
    }

    fn answer(&self, reply: Reply) {
        self.ctx.publish(store::Diff::Atlas(reply));
    }

    /// The world and country refresh: the list
    /// fetched again and written to its cache, a country's only when every
    /// row is that country's. A background refresh already running is not
    /// started twice.
    fn refresh(&mut self, country: Option<String>, token: Option<u64>) {
        if self.refreshing.contains(&country) && token.is_none() {
            return;
        }
        self.refreshing.push(country.clone());
        let (bases, follow, ctx) = (self.bases.clone(), self.follow.clone(), self.ctx.clone());
        self.ctx.spawn(async move {
            let req = match &country {
                Some(code) => stations::country_request(code),
                None => stations::world_request(),
            };
            let rows = request(&bases, &req).await;
            let rows = match &country {
                Some(code) => rows.filter(|r| r.iter().all(|s| s.country_code == *code)),
                None => rows.filter(|r| !r.is_empty()),
            };
            if let Some(rows) = rows.clone() {
                let path = cache_path(country.as_deref());
                let _ = ctx.pool().run(move || write_cache(&path, &rows)).await;
            }
            let _ = follow.try_send(Cmd::RefreshDone(country));
            if let Some(token) = token {
                let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, rows)));
            }
        });
    }

    fn atlas(&mut self, token: u64, ask: Ask) {
        let (bases, follow, ctx) = (self.bases.clone(), self.follow.clone(), self.ctx.clone());
        match ask {
            // The cache when it has one, refreshed behind it once a day old.
            Ask::World => self.ctx.spawn(async move {
                let cached = ctx.pool().run(|| read_cache(&cache_path(None))).await.flatten();
                match cached {
                    Some((doc, rows)) if stations::valid_world_cache(&doc) => {
                        let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, Some(rows))));
                        if stale_doc(&doc) {
                            let _ = follow.try_send(Cmd::Refresh(None, None));
                        }
                    }
                    _ => {
                        let _ = follow.try_send(Cmd::Refresh(None, Some(token)));
                    }
                }
            }),
            Ask::WorldMore => self.ctx.spawn(async move {
                let rows = request(&bases, &stations::world_more_request()).await;
                let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, rows)));
            }),
            // A country's own cache, else what the atlas already holds for it
            // while the country's list is fetched behind it.
            Ask::Country(code, known) => {
                if code.len() != 2 || !code.chars().all(|c| c.is_ascii_uppercase()) {
                    return self.answer(Reply::Rows(token, None));
                }
                self.ctx.spawn(async move {
                    let path = cache_path(Some(&code));
                    let cached = ctx.pool().run(move || read_cache(&path)).await.flatten();
                    if let Some((doc, rows)) = cached.filter(|(d, r)| stations::valid_country_cache(d, &code) && !r.is_empty()) {
                        let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, Some(rows))));
                        if stale_doc(&doc) {
                            let _ = follow.try_send(Cmd::Refresh(Some(code), None));
                        }
                        return;
                    }
                    let local: Vec<Station> = known.into_iter().filter(|s| s.country_code == code).take(100).collect();
                    if local.is_empty() {
                        let _ = follow.try_send(Cmd::Refresh(Some(code), Some(token)));
                    } else {
                        let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, Some(local))));
                        let _ = follow.try_send(Cmd::Refresh(Some(code), None));
                    }
                });
            }
            Ask::Search(q) => {
                let q = q.trim().to_owned();
                if q.is_empty() || q.encode_utf16().count() > 128 {
                    return self.answer(Reply::Rows(token, None));
                }
                self.ctx.spawn(async move {
                    let found = search(&bases, &q).await;
                    let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, found)));
                });
            }
            Ask::Cliamp => {
                if !self.cliamp.is_empty() {
                    return self.answer(Reply::Rows(token, Some(self.cliamp.clone())));
                }
                self.ctx.spawn(async move {
                    let rows = fetch(&document(stations::CLIAMP_URL)).await.and_then(|t| stations::parse_cliamp(&t)).filter(|r| !r.is_empty());
                    let _ = follow.try_send(Cmd::CliampRead(rows.clone()));
                    let _ = follow.try_send(Cmd::Answered(Reply::Rows(token, rows)));
                });
            }
        }
    }

    fn refresh_outputs(&mut self) {
        let sinks = audio::with_graph(audio::sinks);
        self.st.outputs_error = if sinks.is_none() { "Audio outputs are unavailable".into() } else { String::new() };
        self.st.outputs = sinks.unwrap_or_default();
    }

    async fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play(station, list) => self.play(station, list).await,
            Cmd::ToggleMute => {
                if self.ready() {
                    self.write(json!(["cycle", "mute"])).await;
                }
            }
            Cmd::ClearLocalError => self.st.local_error.clear(),
            Cmd::RefreshOutputs => self.refresh_outputs(),
            Cmd::Atlas(token, ask) => self.atlas(token, ask),
            Cmd::Answered(reply) => self.answer(reply),
            Cmd::Refresh(country, token) => self.refresh(country, token),
            Cmd::RefreshDone(country) => {
                if let Some(i) = self.refreshing.iter().position(|c| *c == country) {
                    self.refreshing.remove(i);
                }
            }
            Cmd::CliampRead(rows) => {
                if let Some(rows) = rows {
                    self.cliamp = rows;
                }
            }
            Cmd::PlayFromSaved(station, list) => {
                self.play(station, list.clone()).await;
                self.refresh_saved(&list);
            }
            Cmd::RandomRows(rows) => self.random_rows(rows).await,
            Cmd::Refreshed(rows) => {
                let swap = |s: &Station| rows.iter().find(|r| r.uuid == s.uuid).map_or_else(|| s.clone(), stations::saved_record);
                self.st.favorites = self.st.favorites.iter().map(swap).collect();
                self.st.recent = self.st.recent.iter().map(swap).collect();
                self.save_state();
            }
            Cmd::Toggle => {
                let Some(station) = self.st.station.clone() else { return };
                if self.st.error.is_empty() {
                    self.send_mpv(json!(["cycle", "pause"])).await;
                } else {
                    let list = self.st.queue.clone();
                    self.play(station, list).await;
                }
            }
            Cmd::ToggleFavorite(station) => {
                if station.uuid.is_empty() {
                    return;
                }
                if !self.writable {
                    self.st.local_error = "Favorite could not be updated".into();
                    return;
                }
                let before = self.st.favorites.len();
                self.st.favorites.retain(|s| s.uuid != station.uuid);
                if self.st.favorites.len() == before {
                    self.st.favorites.insert(0, stations::saved_record(&station));
                    self.st.favorites.truncate(stations::MAX_FAVORITES);
                }
                self.st.local_error.clear();
                self.save_state();
            }
            Cmd::Search(q) => {
                let q = q.trim().to_owned();
                let answered = self.st.search.as_ref().is_some_and(|(a, _)| *a == q);
                if q.chars().count() < 2 || q == self.search_want || answered {
                    return;
                }
                self.search_want = q;
                self.search_at = Some(Instant::now() + Duration::from_millis(500));
            }
            Cmd::Searched(q, found) => {
                self.search_busy = false;
                if q != self.search_want {
                    self.start_search();
                    return;
                }
                self.st.search = Some((q, found));
            }
            Cmd::Next => self.step(1).await,
            Cmd::Previous => self.step(-1).await,
            Cmd::Stop => self.stop().await,
            Cmd::Random => self.tune_random(),
            Cmd::SetOutput(sink) => {
                if !sink.is_empty() && audio::with_graph(|g| g.sinks().any(|n| n.name == sink)) != Some(true) {
                    self.st.player_error = "Unknown audio output".into();
                    return;
                }
                if self.ready() {
                    let device = if sink.is_empty() { "auto".to_owned() } else { format!("pulse/{sink}") };
                    self.write(json!(["set_property", "audio-device", device])).await;
                }
                self.st.output = sink;
                self.save_at = Some(Instant::now() + SAVE_DELAY);
            }
            Cmd::SetVolume(v) => {
                if v == self.st.volume {
                    return;
                }
                self.st.volume = v;
                if self.ready() {
                    self.write(json!(["set_property", "volume", v.percent()])).await;
                }
                self.save_at = Some(Instant::now() + SAVE_DELAY);
            }
        }
    }

    fn deadline(&self) -> Option<Instant> {
        [self.save_at, self.stop_at, self.search_at, self.mpv.as_ref().and_then(|m| m.connect_at)].into_iter().flatten().min()
    }

    async fn next_event(&mut self, rx: &async_channel::Receiver<Cmd>) -> Ev {
        let deadline = self.deadline();
        let timer = async {
            match deadline {
                Some(at) => {
                    Timer::at(at).await;
                    Ev::Timer
                }
                None => std::future::pending().await,
            }
        };
        let cmd = async { Ev::Cmd(rx.recv().await.ok()) };
        match self.mpv.as_mut() {
            Some(Mpv { child, reader, .. }) => {
                let line = async {
                    match reader.as_mut() {
                        Some(r) => Ev::Line(r.next().await.and_then(Result::ok)),
                        None => std::future::pending().await,
                    }
                };
                let exit = async {
                    let _ = child.status().await;
                    Ev::Exit
                };
                cmd.or(timer).or(line).or(exit).await
            }
            None => cmd.or(timer).await,
        }
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    if CMD.set(tx.clone()).is_err() {
        return;
    }
    let mut radio = Radio {
        ctx: ctx.clone(),
        st: State::default(),
        mpv: None,
        socket_dir: runtime_dir().map(|d| d.join("formalshell-radio-atlas")),
        pending: Vec::new(),
        stopping: false,
        resume: None,
        active: false,
        recorded: String::new(),
        writable: false,
        save_at: None,
        stop_at: None,
        bases: Bases::default(),
        last_random: String::new(),
        random_busy: false,
        follow: tx,
        search_want: String::new(),
        search_at: None,
        search_busy: false,
        refreshing: Vec::new(),
        cliamp: Vec::new(),
    };
    radio.load_state();
    // formalshell.service stops with KillMode=process, so a shell restart
    // leaves the last shell's mpv playing on this socket with nothing left to
    // control it. One connect tells it to quit; no answer means nothing was
    // there.
    if let Some(path) = radio.socket_path()
        && let Ok(mut stream) = Async::<UnixStream>::connect(path).await
    {
        let _ = stream.write_all(b"{\"command\":[\"quit\"]}\n").await;
        let _ = stream.flush().await;
    }
    radio.publish();
    let mut last = radio.st.clone();
    loop {
        match radio.next_event(&rx).await {
            Ev::Cmd(Some(cmd)) => radio.handle(cmd).await,
            Ev::Cmd(None) => return,
            Ev::Line(Some(line)) => radio.on_line(&line),
            Ev::Line(None) => {
                if let Some(m) = radio.mpv.as_mut() {
                    m.reader = None;
                    m.writer = None;
                }
            }
            Ev::Exit => radio.on_mpv_exited(),
            Ev::Timer => {
                let now = Instant::now();
                if radio.save_at.is_some_and(|t| t <= now) {
                    radio.save_at = None;
                    radio.save_state();
                }
                if radio.search_at.is_some_and(|t| t <= now) {
                    radio.search_at = None;
                    radio.start_search();
                }
                if radio.stop_at.is_some_and(|t| t <= now) {
                    radio.stop_at = None;
                    if let Some(m) = radio.mpv.as_mut() {
                        let _ = m.child.kill();
                    }
                }
                if radio.mpv.as_ref().and_then(|m| m.connect_at).is_some_and(|t| t <= now) {
                    radio.try_connect().await;
                }
            }
        }
        if radio.st != last {
            last = radio.st.clone();
            radio.publish();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tuned(title: &str) -> State {
        State {
            station: Some(Station { uuid: "u".into(), name: "Smoke Radio".into(), url: "http://h/live/station.mp3?x=1".into(), ..Station::default() }),
            title: title.into(),
            ..State::default()
        }
    }

    #[test]
    fn the_stream_file_name_is_not_a_track() {
        assert_eq!(tuned("station.mp3").track_title(), "");
        assert_eq!(tuned("smoke radio").track_title(), "");
        assert_eq!(tuned("").track_title(), "");
        assert_eq!(tuned("Artist - Song").track_title(), "Artist - Song");
    }

    #[test]
    fn the_label_falls_back_to_the_station_and_strips_controls() {
        assert_eq!(tuned("station.mp3").label(), "Smoke Radio");
        assert_eq!(tuned("A\u{1}B").label(), "A B");
        assert_eq!(State::default().label(), "");
    }

    #[test]
    fn a_failed_stream_reads_as_paused() {
        let mut st = tuned("");
        assert!(!st.paused());
        st.error = "Stream disconnected".into();
        assert!(st.paused());
        assert!(st.running());
    }
}
