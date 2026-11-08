use std::collections::HashMap;
use std::time::{Duration, Instant};

use zbus::zvariant::{OwnedValue, Value};

/// Spec value meaning "no current track"; SetPosition against it is a no-op
/// on every player, so it counts as no track id.
const NO_TRACK: &str = "/org/mpris/MediaPlayer2/TrackList/NoTrack";

/// The player's own `Volume`: 0..1, with anything above legal on the wire.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Volume(f64);

impl Volume {
    pub const fn new(volume: f64) -> Self {
        Self(volume)
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    #[default]
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoopStatus {
    #[default]
    None,
    Track,
    Playlist,
}

impl LoopStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            LoopStatus::None => "None",
            LoopStatus::Track => "Track",
            LoopStatus::Playlist => "Playlist",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    pub track_id: Option<String>,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub art_url: String,
    pub url: String,
    /// `mpris:length`, microseconds.
    pub length_us: Option<i64>,
}

impl Metadata {
    /// Artists joined with a comma and a space.
    pub fn artist(&self) -> String {
        self.artists.join(", ")
    }
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    /// Full bus name, `org.mpris.MediaPlayer2.<suffix>`.
    pub bus_name: String,
    pub identity: String,
    pub desktop_entry: Option<String>,
    pub status: PlaybackStatus,
    pub metadata: Metadata,
    pub rate: f64,
    /// `None` when the player does not expose the property.
    pub shuffle: Option<bool>,
    pub loop_status: Option<LoopStatus>,
    pub volume: Option<Volume>,
    /// The player's introspection declares `Volume` writable. Chromium
    /// reports a constant 1 it never lets anyone set.
    pub volume_writable: bool,
    pub can_go_next: bool,
    pub can_go_previous: bool,
    pub can_play: bool,
    pub can_pause: bool,
    pub can_seek: bool,
    pub can_control: bool,
    pub can_raise: bool,
    /// False when reading `Position` fails.
    pub position_supported: bool,
    base_us: i64,
    base_at: Instant,
}

impl PlayerState {
    pub(crate) fn new(bus_name: String, now: Instant) -> Self {
        PlayerState {
            bus_name,
            identity: String::new(),
            desktop_entry: None,
            status: PlaybackStatus::Stopped,
            metadata: Metadata::default(),
            rate: 1.0,
            shuffle: None,
            loop_status: None,
            volume: None,
            volume_writable: false,
            can_go_next: false,
            can_go_previous: false,
            can_play: false,
            can_pause: false,
            can_seek: false,
            can_control: false,
            can_raise: false,
            position_supported: false,
            base_us: 0,
            base_at: now,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.status == PlaybackStatus::Playing
    }

    /// The capability of the verb the toggle would issue next.
    pub fn can_toggle_playing(&self) -> bool {
        if self.is_playing() { self.can_pause } else { self.can_play }
    }

    /// Seeking needs both `CanControl` and `CanSeek`.
    pub fn can_seek_now(&self) -> bool {
        self.can_control && self.can_seek
    }

    /// Track length; falls back to the position when the player publishes no
    /// `mpris:length`.
    pub fn length(&self, now: Instant) -> Duration {
        match self.metadata.length_us {
            Some(us) if us > 0 => micros(us),
            _ => self.position(now),
        }
    }

    /// Current position from the last read plus rate times elapsed time.
    /// Zero while stopped, frozen while paused, clamped to the track length.
    pub fn position(&self, now: Instant) -> Duration {
        let base = self.base_us.max(0);
        let us = match self.status {
            PlaybackStatus::Stopped => 0,
            PlaybackStatus::Paused => base,
            PlaybackStatus::Playing => {
                let elapsed = now.saturating_duration_since(self.base_at).as_secs_f64();
                base.saturating_add((elapsed * self.rate * 1e6) as i64)
            }
        };
        let us = match self.metadata.length_us {
            Some(len) if len > 0 => us.min(len),
            _ => us,
        };
        micros(us)
    }

    pub(crate) fn set_base(&mut self, us: i64, now: Instant) {
        self.base_us = us;
        self.base_at = now;
    }

    pub(crate) fn apply_root(&mut self, props: &HashMap<String, OwnedValue>) {
        self.identity = get_string(props, "Identity").unwrap_or_default();
        self.desktop_entry = get_string(props, "DesktopEntry").filter(|s| !s.is_empty());
        self.can_raise = get_bool(props, "CanRaise").unwrap_or(false);
    }

    pub(crate) fn apply_player(&mut self, props: &HashMap<String, OwnedValue>) {
        self.status = match get_string(props, "PlaybackStatus").as_deref() {
            Some("Playing") => PlaybackStatus::Playing,
            Some("Paused") => PlaybackStatus::Paused,
            _ => PlaybackStatus::Stopped,
        };
        self.metadata = props.get("Metadata").map(parse_metadata).unwrap_or_default();
        self.rate = get_f64(props, "Rate").filter(|r| *r != 0.0).unwrap_or(1.0);
        self.shuffle = get_bool(props, "Shuffle");
        self.loop_status = get_string(props, "LoopStatus").map(|s| match s.as_str() {
            "Track" => LoopStatus::Track,
            "Playlist" => LoopStatus::Playlist,
            _ => LoopStatus::None,
        });
        self.volume = get_f64(props, "Volume").map(Volume);
        self.can_go_next = get_bool(props, "CanGoNext").unwrap_or(false);
        self.can_go_previous = get_bool(props, "CanGoPrevious").unwrap_or(false);
        self.can_play = get_bool(props, "CanPlay").unwrap_or(false);
        self.can_pause = get_bool(props, "CanPause").unwrap_or(false);
        self.can_seek = get_bool(props, "CanSeek").unwrap_or(false);
        self.can_control = get_bool(props, "CanControl").unwrap_or(false);
    }
}

fn micros(us: i64) -> Duration {
    Duration::from_micros(us.max(0) as u64)
}

fn unwrap_value<'a, 'b>(mut v: &'b Value<'a>) -> &'b Value<'a> {
    while let Value::Value(inner) = v {
        v = inner;
    }
    v
}

fn as_string(v: &Value<'_>) -> Option<String> {
    match unwrap_value(v) {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        _ => None,
    }
}

/// Integers arrive as whichever width the player picked (Qt-based ones send
/// `x`, others `t` or `i`).
fn as_i64(v: &Value<'_>) -> Option<i64> {
    match unwrap_value(v) {
        Value::I64(n) => Some(*n),
        Value::U64(n) => i64::try_from(*n).ok(),
        Value::I32(n) => Some(i64::from(*n)),
        Value::U32(n) => Some(i64::from(*n)),
        Value::I16(n) => Some(i64::from(*n)),
        Value::U16(n) => Some(i64::from(*n)),
        Value::U8(n) => Some(i64::from(*n)),
        _ => None,
    }
}

pub(crate) fn as_f64(v: &Value<'_>) -> Option<f64> {
    match unwrap_value(v) {
        Value::F64(n) => Some(*n),
        other => as_i64(other).map(|n| n as f64),
    }
}

fn get_string(props: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    props.get(key).and_then(|v| as_string(v))
}

fn get_bool(props: &HashMap<String, OwnedValue>, key: &str) -> Option<bool> {
    match props.get(key).map(|v| unwrap_value(v)) {
        Some(Value::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn get_f64(props: &HashMap<String, OwnedValue>, key: &str) -> Option<f64> {
    props.get(key).and_then(|v| as_f64(v))
}

pub(crate) fn position_from(v: &Value<'_>) -> Option<i64> {
    as_i64(v)
}

fn parse_metadata(v: &OwnedValue) -> Metadata {
    let Value::Dict(dict) = unwrap_value(v) else {
        return Metadata::default();
    };
    let mut m = Metadata::default();
    for (k, val) in dict.iter() {
        let Some(key) = as_string(k) else { continue };
        let val = unwrap_value(val);
        match key.as_str() {
            "mpris:trackid" => {
                m.track_id = as_string(val).filter(|s| !s.is_empty() && s != NO_TRACK);
            }
            "mpris:length" => m.length_us = as_i64(val),
            "mpris:artUrl" => m.art_url = as_string(val).unwrap_or_default(),
            "xesam:title" => m.title = as_string(val).unwrap_or_default(),
            "xesam:album" => m.album = as_string(val).unwrap_or_default(),
            "xesam:url" => m.url = as_string(val).unwrap_or_default(),
            "xesam:artist" => {
                m.artists = match val {
                    Value::Array(a) => a.iter().filter_map(as_string).collect(),
                    // Some players send the artist as a bare string.
                    other => as_string(other).into_iter().collect(),
                };
            }
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(base_us: i64, rate: f64, len: Option<i64>, t0: Instant) -> PlayerState {
        let mut s = PlayerState::new("org.mpris.MediaPlayer2.x".into(), t0);
        s.status = PlaybackStatus::Playing;
        s.rate = rate;
        s.metadata.length_us = len;
        s.set_base(base_us, t0);
        s
    }

    #[test]
    fn interpolates_with_rate() {
        let t0 = Instant::now();
        let s = playing(10_000_000, 2.0, Some(100_000_000), t0);
        assert_eq!(s.position(t0 + Duration::from_secs(3)), Duration::from_secs(16));
    }

    #[test]
    fn paused_and_stopped_do_not_advance() {
        let t0 = Instant::now();
        let mut s = playing(10_000_000, 1.0, None, t0);
        s.status = PlaybackStatus::Paused;
        assert_eq!(s.position(t0 + Duration::from_secs(9)), Duration::from_secs(10));
        s.status = PlaybackStatus::Stopped;
        assert_eq!(s.position(t0 + Duration::from_secs(9)), Duration::ZERO);
    }

    #[test]
    fn clamps_to_length_and_length_falls_back_to_position() {
        let t0 = Instant::now();
        let s = playing(9_000_000, 1.0, Some(10_000_000), t0);
        assert_eq!(s.position(t0 + Duration::from_secs(30)), Duration::from_secs(10));
        let s = playing(9_000_000, 1.0, None, t0);
        assert_eq!(s.length(t0 + Duration::from_secs(1)), Duration::from_secs(10));
    }
}
