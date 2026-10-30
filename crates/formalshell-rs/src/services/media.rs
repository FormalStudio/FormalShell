//! Every source the bar reads now-playing from, behind one
//! active pick. Four kinds share the row list: an MPRIS player (fs-mpris),
//! the radio's own mpv (`radio`), the phone's Apple Media Service (`ams`) and
//! AirPlay's receiver (`airplay`). An app playing with no MPRIS is a
//! `stream:<node id>` row, listed from the audio graph only while the media
//! panel is open or one is picked, and only ever picked by hand.
//!
//! `selected` is the one piece of state the UI owns; every source's own
//! state is published whole by its service.

use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use fs_media::media::{self as pick, LabelledRow, PlayerRow};
use fs_mpris::{LoopStatus, Mpris, PlayerState};
use serde_json::{Value, json};

use super::devices::audio::{self, Routing, StreamInfo};
use super::{airplay, ams, radio, visualizer};
use crate::runtime::Ctx;
use crate::store;

/// The settings keys the media services act on, handed over by the UI thread
/// whenever settings.json reads differently.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub airplay_enable: bool,
    pub airplay_name: String,
    pub iphone_enable: bool,
    pub motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { airplay_enable: false, airplay_name: String::new(), iphone_enable: true, motion: true }
    }
}

impl Settings {
    fn read(settings: &Value) -> Self {
        let at = |path: &str| {
            let mut node = settings;
            for part in path.split('.') {
                node = node.get(part)?;
            }
            Some(node)
        };
        Self {
            airplay_enable: at("airplay.enable") == Some(&Value::Bool(true)),
            airplay_name: at("airplay.name").and_then(Value::as_str).unwrap_or("").to_owned(),
            iphone_enable: at("iphone.enable") != Some(&Value::Bool(false)),
            motion: at("motion.enabled") != Some(&Value::Bool(false)),
        }
    }
}

struct Hub {
    last: Settings,
    subscribers: Vec<async_channel::Sender<Settings>>,
}

static HUB: Mutex<Hub> = Mutex::new(Hub { last: Settings { airplay_enable: false, airplay_name: String::new(), iphone_enable: true, motion: true }, subscribers: Vec::new() });

/// A service's stream of settings, seeded with the last ones handed over.
pub fn subscribe() -> async_channel::Receiver<Settings> {
    let (tx, rx) = async_channel::unbounded();
    if let Ok(mut hub) = HUB.lock() {
        let _ = tx.try_send(hub.last.clone());
        hub.subscribers.push(tx);
    }
    rx
}

/// The UI thread's side: called with the whole settings document.
pub fn configure(settings: &Value) {
    let next = Settings::read(settings);
    visualizer::set_motion(next.motion);
    let Ok(mut hub) = HUB.lock() else { return };
    if hub.last == next {
        return;
    }
    hub.last = next.clone();
    hub.subscribers.retain(|tx| tx.try_send(next.clone()).is_ok());
}

#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub id: String,
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    /// `xesam:url`, for a sibling `.lrc`.
    pub url: String,
    pub playing: bool,
    /// Seconds at `at`; `position_now` carries it forward while playing.
    pub position: f64,
    pub at: Instant,
    pub length: f64,
    pub can_seek: bool,
    pub can_raise: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub shuffle: Option<bool>,
    pub loop_status: Option<LoopStatus>,
    pub volume: Option<f64>,
}

impl Player {
    fn from(state: &PlayerState, now: Instant) -> Self {
        Self {
            id: state.bus_name.clone(),
            identity: state.identity.clone(),
            title: state.metadata.title.clone(),
            artist: state.metadata.artist(),
            album: state.metadata.album.clone(),
            art_url: state.metadata.art_url.clone(),
            url: state.metadata.url.clone(),
            playing: state.is_playing(),
            position: state.position(now).as_secs_f64(),
            at: now,
            length: state.length(now).as_secs_f64(),
            can_seek: state.can_seek_now() && state.position_supported,
            can_raise: state.can_raise,
            can_toggle: state.can_toggle_playing(),
            can_next: state.can_go_next,
            can_previous: state.can_go_previous,
            shuffle: state.shuffle,
            loop_status: state.loop_status,
            volume: state.volume,
        }
    }

    pub fn position_now(&self, now: Instant) -> f64 {
        if !self.playing {
            return self.position;
        }
        let at = self.position + now.saturating_duration_since(self.at).as_secs_f64();
        if self.length > 0.0 { at.min(self.length) } else { at }
    }

    /// A position read off the player's own clock is no change; one past
    /// `SEEK_SLACK` from where that clock puts it is a seek, and a paused
    /// pane has nothing else to redraw it.
    fn same(&self, other: &Self) -> bool {
        (self.position_now(other.at) - other.position).abs() < SEEK_SLACK
            && Self { at: other.at, position: other.position, ..self.clone() } == *other
    }
}

/// Seconds a polled position may sit off the player's own clock and still
/// be the same playback.
const SEEK_SLACK: f64 = 0.25;

/// What every bar and IPC read of "the active source" resolves to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Active {
    pub id: String,
    pub kind: &'static str,
    pub identity: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub art_url: String,
    pub playing: bool,
    pub position: f64,
    pub length: f64,
    pub can_seek: bool,
    pub can_raise: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub shuffle: Option<bool>,
    pub loop_name: Option<&'static str>,
    pub volume: Option<f64>,
}

#[derive(Default)]
pub struct State {
    pub mpris: Vec<Player>,
    pub radio: radio::State,
    pub airplay: airplay::State,
    pub ams: ams::State,
    /// A bus name, "radio", "iphone" or "airplay"; "" is auto.
    pub selected: String,
    /// The sinks and playback streams, published while something wants them.
    pub routing: Routing,
    /// Decoded art by url and slot size; `None` is art that failed to load.
    covers: Vec<(String, u32, Option<crate::scene::Bitmap>)>,
    /// The animated cover's latest frame by `motion_art` album key, one
    /// bitmap per slot size.
    motion: Option<(String, Vec<crate::scene::Bitmap>)>,
}

pub enum Diff {
    Mpris(Vec<Player>),
    Radio(radio::State),
    Airplay(airplay::State),
    Ams(ams::State),
    Cover(String, u32, Option<crate::scene::Bitmap>),
    /// The animated cover's latest frame by album key, none once it stopped.
    Motion(Option<(String, Vec<crate::scene::Bitmap>)>),
    Routing(Routing),
    /// The panel's source menu, as `media select` does it.
    Select(String),
}

/// JS prints a whole number without its fraction.
fn num(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

fn loop_name(l: LoopStatus) -> &'static str {
    match l {
        LoopStatus::None => "none",
        LoopStatus::Track => "track",
        LoopStatus::Playlist => "playlist",
    }
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = match diff {
            Diff::Select(id) => {
                let changed = self.selected != id;
                self.select(&id);
                return changed;
            }
            Diff::Cover(url, size, bitmap) => {
                // Only the art something can still be showing is kept.
                let active = self.active().map(|a| a.art_url).unwrap_or_default();
                self.covers.retain(|(u, s, _)| *u == active && !(*u == url && *s == size));
                if url != active {
                    return false;
                }
                self.covers.push((url, size, bitmap));
                return true;
            }
            Diff::Motion(next) => {
                let changed = next.is_some() || self.motion.is_some();
                self.motion = next;
                return changed;
            }
            Diff::Routing(next) => std::mem::replace(&mut self.routing, next) != self.routing,
            Diff::Mpris(next) => {
                let same = self.mpris.len() == next.len() && self.mpris.iter().zip(&next).all(|(a, b)| a.same(b));
                self.mpris = next;
                !same
            }
            Diff::Radio(next) => std::mem::replace(&mut self.radio, next) != self.radio,
            Diff::Airplay(next) => std::mem::replace(&mut self.airplay, next) != self.airplay,
            Diff::Ams(next) => {
                let same = ams_same(&self.ams, &next);
                self.ams = next;
                !same
            }
        };
        if changed {
            self.sync_gate();
        }
        changed
    }

    /// The animated cover's frame for `key` at a `size` slot on a surface
    /// at `scale`, which [`motion_art::Want::at`] asked for.
    pub fn motion(&self, key: &str, size: u32, scale: f64) -> Option<crate::scene::Bitmap> {
        let (k, frames) = self.motion.as_ref()?;
        let px = crate::scene::pixels_at(f64::from(size), scale);
        (k == key).then(|| frames.iter().find(|b| u32::from(b.pixmap.width()) == px).map(|b| b.clone().at(scale)))?
    }

    /// The active art in a `size` square cut to `radius` on a surface at
    /// `scale`, asked for when it has not been yet.
    pub fn cover(&self, url: &str, size: u32, radius: f64, scale: f64) -> Option<crate::scene::Bitmap> {
        let px = crate::scene::pixels_at(f64::from(size), scale);
        if let Some((_, _, b)) = self.covers.iter().find(|(u, s, _)| u == url && *s == px) {
            return b.clone().map(|b| b.at(scale));
        }
        super::cover::request(url, px, radius * scale);
        None
    }

    pub fn select(&mut self, id: &str) {
        self.selected = id.to_owned();
        audio::routing_wanted(1, id.starts_with("stream:"));
        self.sync_gate();
    }

    /// The MPRIS player's `xesam:url`, empty for every other source.
    pub fn url_of(&self, id: &str) -> String {
        self.mpris.iter().find(|p| p.id == id).map(|p| p.url.clone()).unwrap_or_default()
    }

    pub fn players_rows(&self) -> Vec<PlayerRow> {
        self.rows()
    }

    fn rows(&self) -> Vec<PlayerRow> {
        let mut rows: Vec<PlayerRow> = self
            .mpris
            .iter()
            .map(|p| PlayerRow {
                id: p.id.clone(),
                kind: Some("mpris".into()),
                auto: None,
                identity: Some(p.identity.clone()),
                is_playing: p.playing,
            })
            .collect();
        let row = |id: &str, kind: &str, identity: &str, playing: bool| PlayerRow {
            id: id.into(),
            kind: Some(kind.into()),
            auto: None,
            identity: Some(identity.into()),
            is_playing: playing,
        };
        if self.radio.running() {
            rows.push(row("radio", "radio", "Radio", !self.radio.paused()));
        }
        if self.ams.listed() {
            rows.push(row("iphone", "iphone", "iPhone", self.ams.playing()));
        }
        // isPlaying stays false: UxPlay reports no pause state at all.
        if self.airplay.active && !self.airplay.title.is_empty() {
            rows.push(row("airplay", "airplay", "AirPlay", false));
        }
        let mpris: Vec<PlayerRow> = rows.iter().filter(|r| r.kind.as_deref() == Some("mpris")).cloned().collect();
        for s in self.routing.streams.iter().filter(|s| self.stream_owner(s, &mpris).is_empty()) {
            let mut r = row(&format!("stream:{}", s.id), "stream", &s.label, false);
            r.auto = Some(false);
            rows.push(r);
        }
        rows
    }

    /// Whose a playback stream is: the radio's by the client name its mpv
    /// stamps on it, a player's by name, "" for an app with no MPRIS.
    fn stream_owner(&self, s: &StreamInfo, mpris: &[PlayerRow]) -> String {
        if s.keys[0].as_deref() == Some(radio::CLIENT_NAME) {
            return "radio".into();
        }
        let keys: Vec<Option<&str>> = s.keys.iter().map(|k| k.as_deref()).collect();
        pick::stream_owner(&keys, mpris)
    }

    fn mpris_rows(&self) -> Vec<PlayerRow> {
        self.rows().into_iter().filter(|r| r.kind.as_deref() == Some("mpris")).collect()
    }

    /// The streams the active source is playing through right now.
    fn routed_streams(&self) -> Vec<&StreamInfo> {
        let id = self.active_id();
        if let Some(n) = id.strip_prefix("stream:") {
            return self.routing.streams.iter().filter(|s| s.id.to_string() == n).collect();
        }
        let mpris = self.mpris_rows();
        self.routing.streams.iter().filter(|s| !id.is_empty() && self.stream_owner(s, &mpris) == id).collect()
    }

    /// Where the active source can be moved: every sink the graph has.
    pub fn outputs(&self) -> &[(String, String)] {
        &self.routing.sinks
    }

    pub fn can_route(&self) -> bool {
        self.routing.sinks.len() > 1 && (self.active_id() == "radio" || !self.routed_streams().is_empty())
    }

    /// The sink name the active source is on: the radio's saved choice ("" is
    /// the default sink), otherwise wherever its first stream is linked.
    pub fn output_id(&self) -> String {
        let fallback = self.routing.default_sink.clone();
        if self.active_id() == "radio" {
            return if self.radio.output.is_empty() { fallback } else { self.radio.output.clone() };
        }
        match self.routed_streams().first() {
            Some(s) if !s.target.is_empty() => s.target.clone(),
            Some(_) => fallback,
            None => String::new(),
        }
    }

    pub fn set_output(&self, name: &str) {
        if !self.routing.sinks.iter().any(|(id, _)| id == name) {
            return;
        }
        if self.active_id() == "radio" {
            let sink = if name == self.routing.default_sink { "" } else { name };
            return radio::send(radio::Cmd::SetOutput(sink.to_owned()));
        }
        let ids: Vec<u32> = self.routed_streams().iter().map(|s| s.id).collect();
        if ids.is_empty() {
            return;
        }
        let sink = name.to_owned();
        // pactl addresses a stream by its sink-input index and PipeWire by
        // its node id: list the sink inputs, then move each one found.
        std::thread::spawn(move || {
            let Ok(listed) = crate::services::proc::std_command("pactl").args(["-f", "json", "list", "sink-inputs"]).output() else { return };
            let text = String::from_utf8_lossy(&listed.stdout);
            for id in ids {
                let index = pick::sink_input_index(&text, id);
                if index >= 0 {
                    let _ = crate::services::proc::std_command("pactl").args(["move-sink-input", &index.to_string(), &sink]).status();
                }
            }
        });
    }

    pub fn players(&self) -> Vec<LabelledRow> {
        pick::with_labels(&self.rows())
    }

    pub fn active_id(&self) -> String {
        pick::pick_player_id(&self.rows(), &self.selected)
    }

    pub fn active(&self) -> Option<Active> {
        self.active_at(Instant::now())
    }

    fn active_at(&self, now: Instant) -> Option<Active> {
        let id = self.active_id();
        if id.is_empty() {
            return None;
        }
        let label = self.players().into_iter().find(|r| r.id == id).map(|r| r.label).unwrap_or_default();
        let base = Active { id: id.clone(), ..Active::default() };
        Some(match id.as_str() {
            "radio" => {
                let r = &self.radio;
                let track = r.track_title();
                let name = r.station.as_ref().map(|s| s.name.clone()).unwrap_or_default();
                Active {
                    kind: "radio",
                    identity: label,
                    title: if track.is_empty() { name.clone() } else { track.clone() },
                    artist: if track.is_empty() { String::new() } else { name },
                    playing: !r.paused(),
                    can_next: r.queue.len() > 1,
                    can_previous: r.queue.len() > 1,
                    volume: Some(r.volume as f64 / 100.0),
                    ..base
                }
            }
            "iphone" => {
                let a = &self.ams;
                Active {
                    kind: "iphone",
                    identity: label,
                    title: a.title.clone(),
                    artist: a.artist.clone(),
                    album: a.album.clone(),
                    playing: a.playing(),
                    can_next: true,
                    can_previous: true,
                    position: a.position(now),
                    length: a.duration,
                    volume: (a.volume >= 0.0).then(|| pick::clamp_volume(a.volume)),
                    ..base
                }
            }
            "airplay" => {
                let a = &self.airplay;
                // The one cover file is rewritten per track, so the track
                // names which picture this is.
                let art_url = if a.has_cover {
                    format!("file://{}?{}-{}", airplay::dir().join("cover.jpg").display(), a.artist, a.title)
                } else {
                    String::new()
                };
                Active { kind: "airplay", identity: label, title: a.title.clone(), artist: a.artist.clone(), album: a.album.clone(), art_url, ..base }
            }
            stream if stream.starts_with("stream:") => {
                let s = self.routing.streams.iter().find(|s| format!("stream:{}", s.id) == stream)?;
                Active {
                    kind: "stream",
                    identity: label,
                    title: if s.title.is_empty() { s.label.clone() } else { s.title.clone() },
                    volume: s.volume.map(pick::clamp_volume),
                    ..base
                }
            }
            _ => {
                let p = self.mpris.iter().find(|p| p.id == id)?;
                Active {
                    kind: "mpris",
                    identity: p.identity.clone(),
                    title: p.title.clone(),
                    artist: p.artist.clone(),
                    album: p.album.clone(),
                    art_url: p.art_url.clone(),
                    playing: p.playing,
                    position: p.position_now(now),
                    length: p.length,
                    can_seek: p.can_seek,
                    can_raise: p.can_raise,
                    can_next: p.can_next,
                    can_previous: p.can_previous,
                    shuffle: p.shuffle,
                    loop_name: p.loop_status.map(loop_name),
                    volume: p.volume.map(pick::clamp_volume),
                    ..base
                }
            }
        })
    }

    /// Feeds the visualizer's run gate, which the UI thread owns the inputs of.
    fn sync_gate(&self) {
        let a = self.active();
        let (playing, tempo) = a.as_ref().map_or((false, false), |a| (a.playing, a.kind == "iphone"));
        let (artist, title) = a.map_or_else(Default::default, |a| (a.artist, a.title));
        visualizer::set_media(playing, tempo, &artist, &title);
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> Value {
        let a = self.active().unwrap_or_default();
        let available = !a.id.is_empty();
        json!({
            "available": available,
            "id": a.id,
            "kind": a.kind,
            "selectedId": self.selected,
            "output": self.output_id(),
            "canRoute": self.can_route(),
            "playerCount": self.players().len(),
            "identity": a.identity,
            "title": a.title,
            "artist": a.artist,
            "album": a.album,
            "artUrl": a.art_url,
            "isPlaying": a.playing,
            "position": num(a.position),
            "length": num(a.length),
            "canSeek": a.can_seek,
            "canRaise": a.can_raise,
            "shuffleSupported": a.shuffle.is_some(),
            "shuffle": a.shuffle.unwrap_or(false),
            "loopSupported": a.loop_name.is_some(),
            "loop": a.loop_name.unwrap_or("none"),
            "volumeSupported": a.volume.is_some(),
            "volume": num(a.volume.unwrap_or(0.0)),
        })
    }

    /// The IPC `outputs` reply.
    pub fn outputs_json(&self) -> Value {
        Value::Array(self.routing.sinks.iter().map(|(id, label)| json!({"id": id, "label": label})).collect())
    }

    /// The IPC `players` reply.
    pub fn players_json(&self) -> Value {
        Value::Array(
            self.players()
                .into_iter()
                .map(|r| {
                    let mut row = serde_json::Map::new();
                    row.insert("id".into(), r.id.into());
                    row.insert("kind".into(), r.kind.into());
                    row.insert("auto".into(), r.auto.into());
                    if let Some(identity) = r.identity {
                        row.insert("identity".into(), identity.into());
                    }
                    row.insert("label".into(), r.label.into());
                    row.insert("isPlaying".into(), r.is_playing.into());
                    Value::Object(row)
                })
                .collect(),
        )
    }

    // --- Controls, each gated on what the active source can do ----------

    fn mpris_active(&self) -> Option<&Player> {
        let id = self.active_id();
        self.mpris.iter().find(|p| p.id == id)
    }

    pub fn play_pause(&self) {
        match self.active_id().as_str() {
            "radio" => radio::send(radio::Cmd::Toggle),
            "iphone" => {
                ams::command("toggle");
            }
            _ => {
                if let Some(p) = self.mpris_active().filter(|p| p.can_toggle) {
                    mpris_send(Cmd::PlayPause(p.id.clone()));
                }
            }
        }
    }

    pub fn next(&self) {
        match self.active_id().as_str() {
            "radio" => radio::send(radio::Cmd::Next),
            "iphone" => {
                ams::command("next");
            }
            _ => {
                if let Some(p) = self.mpris_active().filter(|p| p.can_next) {
                    mpris_send(Cmd::Next(p.id.clone()));
                }
            }
        }
    }

    pub fn previous(&self) {
        match self.active_id().as_str() {
            "radio" => radio::send(radio::Cmd::Previous),
            "iphone" => {
                ams::command("prev");
            }
            _ => {
                if let Some(p) = self.mpris_active().filter(|p| p.can_previous) {
                    mpris_send(Cmd::Previous(p.id.clone()));
                }
            }
        }
    }

    pub fn set_shuffle(&self, on: bool) {
        if let Some(p) = self.mpris_active().filter(|p| p.shuffle.is_some()) {
            mpris_send(Cmd::Shuffle(p.id.clone(), on));
        }
    }

    pub fn set_loop(&self, name: &str) {
        let status = match name {
            "track" => LoopStatus::Track,
            "playlist" => LoopStatus::Playlist,
            _ => LoopStatus::None,
        };
        if let Some(p) = self.mpris_active().filter(|p| p.loop_status.is_some()) {
            mpris_send(Cmd::Loop(p.id.clone(), status));
        }
    }

    /// 0..1, the player's own scale; the radio's mpv takes 0..100 and the
    /// phone a step in whichever direction the request moves.
    pub fn set_volume(&self, v: f64) {
        let v = pick::clamp_volume(v);
        let Some(a) = self.active() else { return };
        match a.kind {
            "radio" => radio::send(radio::Cmd::SetVolume(v * 100.0)),
            "iphone" => {
                if let Some(now) = a.volume
                    && v != now
                {
                    ams::command(if v > now { "volup" } else { "voldown" });
                }
            }
            "mpris" => mpris_send(Cmd::Volume(a.id, v)),
            "stream" => {
                if let Some(node) = a.id.strip_prefix("stream:").and_then(|n| n.parse().ok()) {
                    audio::write(fs_audio::Command::SetVolume { node, volume: v as f32 });
                }
            }
            _ => {}
        }
    }

    /// An absolute position in seconds, through MPRIS's relative Seek from
    /// where the player is now.
    pub fn seek_to(&self, seconds: f64) {
        let Some(a) = self.active() else { return };
        if a.kind != "mpris" || !a.can_seek || a.length <= 0.0 {
            return;
        }
        let target = seconds.clamp(0.0, a.length);
        mpris_send(Cmd::Seek(a.id, ((target - a.position) * 1_000_000.0).round() as i64));
    }

    pub fn raise(&self) {
        if let Some(p) = self.mpris_active().filter(|p| p.can_raise) {
            mpris_send(Cmd::Raise(p.id.clone()));
        }
    }
}

fn ams_same(a: &ams::State, b: &ams::State) -> bool {
    a.installed == b.installed
        && a.available == b.available
        && a.title == b.title
        && a.artist == b.artist
        && a.album == b.album
        && a.duration == b.duration
        && a.playback == b.playback
        && a.volume == b.volume
        && a.elapsed == b.elapsed
        && a.error == b.error
}

enum Cmd {
    PlayPause(String),
    Next(String),
    Previous(String),
    Shuffle(String, bool),
    Loop(String, LoopStatus),
    Volume(String, f64),
    Raise(String),
    Seek(String, i64),
}

static CMD: OnceLock<async_channel::Sender<Cmd>> = OnceLock::new();

fn mpris_send(cmd: Cmd) {
    if let Some(tx) = CMD.get() {
        let _ = tx.try_send(cmd);
    }
}

fn publish(ctx: &Ctx, mpris: &Mpris) {
    let now = Instant::now();
    let players: Vec<Player> = mpris.players().map(|p| Player::from(p, now)).collect();
    ctx.publish(store::Diff::Media(Diff::Mpris(players)));
}

pub async fn run(ctx: Ctx) {
    let conn = match zbus::Connection::session().await {
        Ok(conn) => conn,
        Err(err) => return eprintln!("media: no session bus: {err}"),
    };
    let mut mpris = match Mpris::connect(&conn).await {
        Ok(mpris) => mpris,
        Err(err) => return eprintln!("media: {err}"),
    };
    let (tx, rx) = async_channel::unbounded();
    let _ = CMD.set(tx);
    let controls = mpris.controls();
    ctx.spawn(async move {
        while let Ok(cmd) = rx.recv().await {
            let _ = match cmd {
                Cmd::PlayPause(id) => controls.play_pause(&id).await,
                Cmd::Next(id) => controls.next(&id).await,
                Cmd::Previous(id) => controls.previous(&id).await,
                Cmd::Shuffle(id, on) => controls.set_shuffle(&id, on).await,
                Cmd::Loop(id, status) => controls.set_loop_status(&id, status).await,
                Cmd::Volume(id, v) => controls.set_volume(&id, v).await,
                Cmd::Raise(id) => controls.raise(&id).await,
                Cmd::Seek(id, us) => controls.seek(&id, us).await,
            };
        }
    });
    publish(&ctx, &mpris);
    while mpris.next().await.is_some() {
        publish(&ctx, &mpris);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mpris(id: &str, playing: bool) -> Player {
        Player {
            id: id.into(),
            identity: "mpv".into(),
            title: "Song".into(),
            artist: "Band".into(),
            album: String::new(),
            art_url: String::new(),
            url: String::new(),
            playing,
            position: 5.0,
            at: Instant::now(),
            length: 100.0,
            can_seek: true,
            can_raise: false,
            can_toggle: true,
            can_next: true,
            can_previous: true,
            shuffle: Some(true),
            loop_status: Some(LoopStatus::Track),
            volume: Some(0.3),
        }
    }

    fn radio_on() -> radio::State {
        radio::State {
            station: Some(fs_media::radio::stations::Station { uuid: "r".into(), name: "Smoke Radio".into(), url: "http://h/s".into(), ..Default::default() }),
            ..radio::State::default()
        }
    }

    #[test]
    fn a_paused_seek_is_a_change_and_a_playing_clock_is_not() {
        let paused = mpris("p", false);
        let sought = Player { position: 30.6, ..paused.clone() };
        assert!(!paused.same(&sought));
        assert!(paused.same(&Player { at: paused.at + std::time::Duration::from_secs(2), ..paused.clone() }));
        let playing = mpris("p", true);
        let later = Player { at: playing.at + std::time::Duration::from_secs(2), position: 7.0, ..playing.clone() };
        assert!(playing.same(&later));
    }

    #[test]
    fn nothing_playing_is_honestly_unavailable() {
        let s = State::default().status();
        assert_eq!(s["available"], false);
        assert_eq!(s["kind"], "");
        assert_eq!(s["loop"], "none");
    }

    #[test]
    fn an_mpris_player_reports_its_own_capabilities_lowercase() {
        let s = State { mpris: vec![mpris("org.mpris.MediaPlayer2.mpv", true)], ..State::default() };
        let status = s.status();
        assert_eq!(status["kind"], "mpris");
        assert_eq!(status["loop"], "track");
        assert_eq!(status["shuffle"], true);
        assert_eq!(status["volume"], 0.3);
        assert_eq!(status["playerCount"], 1);
    }

    #[test]
    fn the_playing_source_wins_and_a_selection_overrides_it() {
        let mut s = State { mpris: vec![mpris("org.mpris.MediaPlayer2.mpv", false)], radio: radio_on(), ..State::default() };
        assert_eq!(s.active_id(), "radio");
        let a = s.active().unwrap();
        assert_eq!((a.kind, a.identity.as_str(), a.title.as_str()), ("radio", "Radio", "Smoke Radio"));
        s.selected = "org.mpris.MediaPlayer2.mpv".into();
        assert_eq!(s.active_id(), "org.mpris.MediaPlayer2.mpv");
        s.selected = "gone".into();
        assert_eq!(s.active_id(), "radio");
    }

    #[test]
    fn airplay_is_listed_only_with_a_title_and_is_never_playing() {
        let mut s = State { airplay: airplay::State { active: true, ..airplay::State::default() }, ..State::default() };
        assert!(s.players().is_empty());
        s.airplay.title = "Waves".into();
        let rows = s.players();
        assert_eq!((rows[0].id.as_str(), rows[0].label.as_str(), rows[0].is_playing), ("airplay", "AirPlay", false));
        assert_eq!(s.status()["kind"], "airplay");
    }

    #[test]
    fn players_keep_the_wire_keys() {
        let s = State { radio: radio_on(), ..State::default() };
        assert_eq!(
            s.players_json().to_string(),
            r#"[{"id":"radio","kind":"radio","auto":true,"identity":"Radio","label":"Radio","isPlaying":true}]"#
        );
    }

    fn stream(id: u32, app: &str, target: &str) -> StreamInfo {
        StreamInfo {
            id,
            keys: [Some(app.into()), None, None, Some("node".into())],
            label: app.into(),
            title: String::new(),
            volume: Some(0.5),
            target: target.into(),
        }
    }

    fn routed() -> State {
        State {
            mpris: vec![mpris("org.mpris.MediaPlayer2.mpv", true)],
            routing: Routing {
                sinks: vec![("sink-a".into(), "Speakers".into()), ("sink-b".into(), "Headset".into())],
                streams: vec![stream(40, "mpv", "sink-b"), stream(41, "Discord", "sink-a")],
                default_sink: "sink-a".into(),
            },
            ..State::default()
        }
    }

    #[test]
    fn an_app_with_no_player_is_a_stream_row_only_picked_by_hand() {
        let mut s = routed();
        let rows = s.players();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["org.mpris.MediaPlayer2.mpv", "stream:41"]);
        assert_eq!(s.active_id(), "org.mpris.MediaPlayer2.mpv");
        s.selected = "stream:41".into();
        let a = s.active().unwrap();
        assert_eq!((a.kind, a.identity.as_str(), a.volume), ("stream", "Discord", Some(0.5)));
        assert_eq!(s.status()["canRoute"], true);
        assert_eq!(s.status()["output"], "sink-a");
    }

    #[test]
    fn a_player_routes_by_the_streams_its_name_owns() {
        let s = routed();
        assert!(s.can_route());
        assert_eq!(s.output_id(), "sink-b");
        let lone = State { routing: Routing { sinks: vec![("sink-a".into(), "Speakers".into())], ..s.routing.clone() }, ..routed() };
        assert!(!lone.can_route());
    }

    #[test]
    fn the_radio_routes_on_its_saved_choice_over_the_default() {
        let mut s = State { radio: radio_on(), ..routed() };
        s.mpris.clear();
        assert_eq!(s.output_id(), "sink-a");
        s.radio.output = "sink-b".into();
        assert_eq!(s.output_id(), "sink-b");
        assert_eq!(s.outputs_json().to_string(), r#"[{"id":"sink-a","label":"Speakers"},{"id":"sink-b","label":"Headset"}]"#);
    }

    #[test]
    fn settings_read_their_defaults() {
        assert_eq!(Settings::read(&json!({})), Settings::default());
        let s = Settings::read(&json!({"airplay": {"enable": true, "name": "Mac"}, "iphone": {"enable": false}, "motion": {"enabled": false}}));
        assert!(s.airplay_enable && !s.iphone_enable && !s.motion);
        assert_eq!(s.airplay_name, "Mac");
    }
}
