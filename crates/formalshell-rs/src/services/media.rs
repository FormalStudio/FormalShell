//! Every source the bar reads now-playing from, behind one
//! active pick. Five kinds share the row list: an MPRIS player (fs-mpris),
//! the radio's own mpv (`radio`), the phone's Apple Media Service (`ams`),
//! AirPlay's receiver (`airplay`) and a Bluetooth device's AVRCP player
//! (`bluez:<address>`, BlueZ's `MediaPlayer1` read in `devices::bluetooth`).
//! An app playing with no MPRIS is a `stream:<node id>` row, listed from the
//! audio graph only while the media panel is open or one is picked, and only
//! ever picked by hand; the stream a Bluetooth device plays into is its
//! player's, never a row of its own.
//!
//! `selected` is the one piece of state the UI owns; every source's own
//! state is published whole by its service.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use fs_media::media::{self as pick, LabelledRow, PlayerRow};
use fs_mpris::{LoopStatus, Mpris, PlayerState};
use fs_types::{BtAddress, Fraction};
use serde_json::{Value, json};

use fs_audio::Visual;
use fs_bluez::PlayerCommand;

use super::devices::audio::{self, Routing, StreamInfo};
use super::devices::bluetooth;
use super::{airplay, ams, kopuz, radio, visualizer};
use crate::runtime::Ctx;
use crate::store;

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
    /// At `at`; `position_now` carries it forward while playing.
    pub position: Duration,
    pub at: Instant,
    pub length: Duration,
    pub can_seek: bool,
    pub can_raise: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub shuffle: Option<bool>,
    pub loop_status: Option<LoopStatus>,
    pub volume: Option<fs_mpris::Volume>,
    pub volume_writable: bool,
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
            position: state.position(now),
            at: now,
            length: state.length(now),
            can_seek: state.can_seek_now() && state.position_supported,
            can_raise: state.can_raise,
            can_toggle: state.can_toggle_playing(),
            can_next: state.can_go_next,
            can_previous: state.can_go_previous,
            shuffle: state.shuffle,
            loop_status: state.loop_status,
            volume: state.volume,
            volume_writable: state.volume_writable,
        }
    }

    pub fn position_now(&self, now: Instant) -> Duration {
        carried(self.position, self.length, self.at, self.playing, now)
    }

    /// A position read off the player's own clock is no change; one past
    /// `SEEK_SLACK` from where that clock puts it is a seek, and a paused
    /// pane has nothing else to redraw it.
    fn same(&self, other: &Self) -> bool {
        self.position_now(other.at).abs_diff(other.position) < SEEK_SLACK
            && Self { at: other.at, position: other.position, ..self.clone() } == *other
    }
}

/// A Bluetooth device's AVRCP player, named after the device: the app on
/// it ("YouTube") is only what it happens to be playing through.
#[derive(Clone, Debug, PartialEq)]
pub struct Remote {
    /// `bluez:<the device's address>`.
    pub id: String,
    /// The `MediaPlayer1` object, which the transport goes to.
    pub path: String,
    /// The device's identity address, the one its PipeWire node carries
    /// (the object path can carry a random resolvable one instead). None
    /// for a player whose device is not known.
    pub address: Option<BtAddress>,
    pub label: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub playing: bool,
    /// At `at`; `position_now` carries it forward while playing.
    pub position: Duration,
    pub at: Instant,
    /// Zero when the remote sends no duration.
    pub length: Duration,
}

impl Remote {
    pub fn from(p: &fs_bluez::state::Player, device: Option<&fs_bluez::state::Device>, at: Instant) -> Self {
        let address = device.map(|d| d.info.address);
        let shown = address.map(|a| a.to_string()).unwrap_or_default();
        let label = device.map(|d| if d.info.name.is_empty() { d.info.device_name.clone() } else { d.info.name.clone() }).filter(|l| !l.is_empty()).unwrap_or_else(|| shown.clone());
        Self {
            id: format!("bluez:{shown}"),
            path: p.path.clone(),
            address,
            label,
            title: p.title.clone(),
            artist: p.artist.clone(),
            album: p.album.clone(),
            playing: p.playing(),
            position: p.position,
            at,
            length: p.duration.unwrap_or_default(),
        }
    }

    pub fn position_now(&self, now: Instant) -> Duration {
        carried(self.position, self.length, self.at, self.playing, now)
    }

    /// Whether `node` (a PipeWire node.name) is the stream this device
    /// plays into: BlueZ's PipeWire plugin names it
    /// `bluez_input.<address, colons as underscores>.<profile>`.
    fn plays_into(&self, node: &str) -> bool {
        let Some(address) = self.address else { return false };
        let want = format!("bluez_input.{}", address.path_segment());
        node.get(..want.len()).is_some_and(|n| n.eq_ignore_ascii_case(&want))
    }
}

/// Where a position read at `at` has got to by `now`: carried forward while
/// playing, stopping at the end when the length is known.
fn carried(position: Duration, length: Duration, at: Instant, playing: bool, now: Instant) -> Duration {
    if !playing {
        return position;
    }
    let moved = position + now.saturating_duration_since(at);
    if length.is_zero() { moved } else { moved.min(length) }
}

/// How far a polled position may sit off the player's own clock and still
/// be the same playback.
const SEEK_SLACK: Duration = Duration::from_millis(250);

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
    pub position: Duration,
    pub length: Duration,
    pub can_seek: bool,
    pub can_raise: bool,
    pub can_next: bool,
    pub can_previous: bool,
    pub shuffle: Option<bool>,
    pub loop_name: Option<&'static str>,
    /// The share of the flat track the source's own volume fills.
    pub volume: Option<Fraction>,
    /// A volume shown that nothing here can set.
    pub volume_fixed: bool,
}

#[derive(Default)]
pub struct State {
    pub mpris: Vec<Player>,
    pub radio: radio::State,
    pub airplay: airplay::State,
    pub ams: ams::State,
    /// kopuzd's row, whatever track it is; `kopuz()` is the matched one.
    pub kopuz: kopuz::State,
    pub remotes: Vec<Remote>,
    /// A bus name, "radio", "iphone", "airplay" or a `bluez:` id; "" is auto.
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
    Kopuz(kopuz::State),
    Remotes(Vec<Remote>),
    Cover(String, u32, Option<crate::scene::Bitmap>),
    /// The animated cover's latest frame by album key, none once it stopped.
    Motion(Option<(String, Vec<crate::scene::Bitmap>)>),
    Routing(Routing),
    /// The panel's source menu, as `media select` does it.
    Select(String),
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
            Diff::Remotes(next) => std::mem::replace(&mut self.remotes, next) != self.remotes,
            Diff::Airplay(next) => std::mem::replace(&mut self.airplay, next) != self.airplay,
            Diff::Ams(next) => {
                let same = ams_same(&self.ams, &next);
                self.ams = next;
                !same
            }
            Diff::Kopuz(next) => return std::mem::replace(&mut self.kopuz, next) != self.kopuz,
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
                empty: p.title.is_empty(),
            })
            .collect();
        let row = |id: &str, kind: &str, identity: &str, playing: bool| PlayerRow {
            id: id.into(),
            kind: Some(kind.into()),
            auto: None,
            identity: Some(identity.into()),
            is_playing: playing,
            empty: false,
        };
        if self.radio.running() {
            rows.push(row("radio", "radio", "Radio", !self.radio.paused()));
        }
        if self.ams.listed() {
            rows.push(row("iphone", "iphone", "iPhone", self.ams.playing()));
        }
        for r in &self.remotes {
            rows.push(row(&r.id, "bluetooth", &r.label, r.playing));
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
    /// stamps on it, a Bluetooth device's by its node name, a player's by
    /// name, "" for an app with no MPRIS.
    fn stream_owner(&self, s: &StreamInfo, mpris: &[PlayerRow]) -> String {
        if s.keys[0].as_deref() == Some(radio::CLIENT_NAME) {
            return "radio".into();
        }
        if let Some(r) = self.remotes.iter().find(|r| s.keys[3].as_deref().is_some_and(|n| r.plays_into(n))) {
            return r.id.clone();
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
                    volume: Some(r.volume.track()),
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
                    position: Duration::try_from_secs_f64(a.position(now)).unwrap_or_default(),
                    length: Duration::try_from_secs_f64(a.duration).unwrap_or_default(),
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
            remote if remote.starts_with("bluez:") => {
                let r = self.remotes.iter().find(|r| r.id == remote)?;
                Active {
                    kind: "bluetooth",
                    identity: label,
                    title: r.title.clone(),
                    artist: r.artist.clone(),
                    album: r.album.clone(),
                    playing: r.playing,
                    position: r.position_now(now),
                    length: r.length,
                    can_next: true,
                    can_previous: true,
                    volume: self.routed_streams().first().and_then(|s| s.volume).map(|v| pick::clamp_volume(v.as_f64())),
                    ..base
                }
            }
            stream if stream.starts_with("stream:") => {
                let s = self.routing.streams.iter().find(|s| format!("stream:{}", s.id) == stream)?;
                Active {
                    kind: "stream",
                    identity: label,
                    title: if s.title.is_empty() { s.label.clone() } else { s.title.clone() },
                    volume: s.volume.map(|v| pick::clamp_volume(v.as_f64())),
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
                    volume: p.volume.map(|v| pick::clamp_volume(v.get())),
                    volume_fixed: !p.volume_writable,
                    ..base
                }
            }
        })
    }

    /// kopuzd's row when the active player is Kopuz and the row is the
    /// track it reports.
    pub fn kopuz(&self) -> Option<&kopuz::State> {
        let a = self.active()?;
        (a.kind == "mpris" && kopuz::is_kopuz(&a.id) && self.kopuz.matches(&a.title, &a.artist)).then_some(&self.kopuz)
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
        let kopuz = self.kopuz();
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
            "position": fs_js::json_number(a.position.as_secs_f64()),
            "length": fs_js::json_number(a.length.as_secs_f64()),
            "canSeek": a.can_seek,
            "canRaise": a.can_raise,
            "shuffleSupported": a.shuffle.is_some(),
            "shuffle": a.shuffle.unwrap_or(false),
            "loopSupported": a.loop_name.is_some(),
            "loop": a.loop_name.unwrap_or("none"),
            "volumeSupported": a.volume.is_some(),
            "volume": fs_js::json_number(a.volume.map_or(0.0, Fraction::as_fraction)),
            "kopuz": {
                "connected": kopuz.is_some(),
                "key": kopuz.map_or("", |k| k.key.as_str()),
                "favorite": kopuz.is_some_and(|k| k.favorite),
                "actions": kopuz.map_or_else(Vec::new, kopuz::State::actions),
            },
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

    fn remote_active(&self) -> Option<&Remote> {
        let id = self.active_id();
        self.remotes.iter().find(|r| r.id == id)
    }

    pub fn play_pause(&self) {
        if let Some(r) = self.remote_active() {
            return bluetooth::player(&r.path, if r.playing { PlayerCommand::Pause } else { PlayerCommand::Play });
        }
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
        if let Some(r) = self.remote_active() {
            return bluetooth::player(&r.path, PlayerCommand::Next);
        }
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
        if let Some(r) = self.remote_active() {
            return bluetooth::player(&r.path, PlayerCommand::Previous);
        }
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

    /// A position on the flat track, written back in the source's own
    /// scale; the phone takes a step in whichever direction the request
    /// moves.
    pub fn set_volume(&self, v: Fraction) {
        let Some(a) = self.active() else { return };
        match a.kind {
            "radio" => radio::send(radio::Cmd::SetVolume(radio::Volume::from_track(v))),
            "iphone" => {
                if let Some(now) = a.volume
                    && v != now
                {
                    ams::command(if v > now { "volup" } else { "voldown" });
                }
            }
            "mpris" if !a.volume_fixed => mpris_send(Cmd::Volume(a.id, fs_mpris::Volume::new(v.as_fraction()))),
            "stream" => {
                if let Some(node) = a.id.strip_prefix("stream:").and_then(|n| n.parse().ok()) {
                    audio::write(fs_audio::Command::SetVolume { node, volume: Visual::from_f64(v.as_fraction()) });
                }
            }
            "bluetooth" => {
                for s in self.routed_streams() {
                    audio::write(fs_audio::Command::SetVolume { node: s.id, volume: Visual::from_f64(v.as_fraction()) });
                }
            }
            _ => {}
        }
    }

    /// An absolute position in seconds, through MPRIS's relative Seek from
    /// where the player is now.
    pub fn seek_to(&self, target: Duration) {
        let Some(a) = self.active() else { return };
        if a.kind != "mpris" || !a.can_seek || a.length.is_zero() {
            return;
        }
        let target = target.min(a.length);
        mpris_send(Cmd::Seek(a.id, target.as_micros() as i64 - a.position.as_micros() as i64));
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
    Volume(String, fs_mpris::Volume),
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

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let conn = match crate::services::bus::private(&ctx, zbus::connection::Builder::session()).await {
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
            position: Duration::from_secs(5),
            at: Instant::now(),
            length: Duration::from_secs(100),
            can_seek: true,
            can_raise: false,
            can_toggle: true,
            can_next: true,
            can_previous: true,
            shuffle: Some(true),
            loop_status: Some(LoopStatus::Track),
            volume: Some(fs_mpris::Volume::new(0.3)),
            volume_writable: true,
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
        let sought = Player { position: Duration::from_secs_f64(30.6), ..paused.clone() };
        assert!(!paused.same(&sought));
        assert!(paused.same(&Player { at: paused.at + Duration::from_secs(2), ..paused.clone() }));
        let playing = mpris("p", true);
        let later = Player { at: playing.at + Duration::from_secs(2), position: Duration::from_secs(7), ..playing.clone() };
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
            volume: Some(Visual::new(0.5)),
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
        assert_eq!((a.kind, a.identity.as_str(), a.volume), ("stream", "Discord", Some(Fraction::from_fraction(0.5))));
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

    fn remote(playing: bool) -> Remote {
        Remote {
            id: "bluez:C8:89:F3:45:A7:AA".into(),
            path: "/org/bluez/hci0/dev_4A_DC_C3_A2_75_EC/avrcp/player0".into(),
            address: "C8:89:F3:45:A7:AA".parse().ok(),
            label: "FormalPad".into(),
            title: "OLED iPad Mini".into(),
            artist: "Marques Brownlee".into(),
            album: String::new(),
            playing,
            position: Duration::from_secs_f64(359.686),
            at: Instant::now(),
            length: Duration::from_secs_f64(779.141),
        }
    }

    #[test]
    fn a_bluetooth_player_is_a_source_and_its_stream_is_no_row_of_its_own() {
        let mut s = routed();
        s.mpris[0].playing = false;
        s.routing.streams.push(StreamInfo {
            id: 42,
            keys: [None, None, None, Some("bluez_input.C8_89_F3_45_A7_AA.2".into())],
            label: "FormalPad".into(),
            title: String::new(),
            volume: Some(Visual::new(0.8)),
            target: "sink-b".into(),
        });
        assert!(s.players().iter().any(|r| r.id == "stream:42"));
        s.remotes = vec![remote(true)];
        let rows = s.players();
        assert_eq!(rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["org.mpris.MediaPlayer2.mpv", "bluez:C8:89:F3:45:A7:AA", "stream:41"]);
        assert_eq!((rows[1].kind.as_str(), rows[1].label.as_str(), rows[1].auto), ("bluetooth", "FormalPad", true));
        assert_eq!(s.active_id(), "bluez:C8:89:F3:45:A7:AA");
        let a = s.active().unwrap();
        assert_eq!((a.kind, a.title.as_str(), a.artist.as_str(), a.playing, a.volume), ("bluetooth", "OLED iPad Mini", "Marques Brownlee", true, Some(Visual::new(0.8).track())));
        assert!(a.position >= Duration::from_secs_f64(359.686) && a.length == Duration::from_secs_f64(779.141) && !a.can_seek && a.can_next);
        assert_eq!((s.output_id().as_str(), s.can_route()), ("sink-b", true));
        s.remotes = vec![remote(false)];
        assert_eq!(s.active_id(), "org.mpris.MediaPlayer2.mpv");
    }
}
