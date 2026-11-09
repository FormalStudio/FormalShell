//! The phone's own now-playing, the half of the iPhone support the media
//! source reads: `omarchy-iphone-ams listen`, a second GATT client on the
//! BLE link ANCS rides, owned only while a phone is connected.
//!
//! AMS pushes `elapsed` only when the playback info changes, so the
//! position is extrapolated forward from the last real reading. The phone
//! offers no seek and no absolute volume, only relative transport.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_devices::iphone::{self, AmsEvent};
use futures_lite::io::{AsyncBufReadExt, BufReader};
use futures_lite::{FutureExt, StreamExt};

use super::config::{self, Settings};
use crate::runtime::Ctx;
use crate::settings::IPHONE_ENABLE;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const AMS: &str = "omarchy-iphone-ams";

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub installed: bool,
    /// AMS's own signal that the phone's entity-update characteristic exists.
    pub available: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    /// playing, paused, rewinding, forwarding, or "" before anything came.
    pub playback: String,
    /// -1 while unknown.
    pub volume: f64,
    pub elapsed: f64,
    pub at: Instant,
    pub error: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            installed: false,
            available: false,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration: 0.0,
            playback: String::new(),
            volume: -1.0,
            elapsed: 0.0,
            at: Instant::now(),
            error: String::new(),
        }
    }
}

impl State {
    pub fn playing(&self) -> bool {
        self.playback == "playing"
    }

    pub fn position(&self, now: Instant) -> f64 {
        if !self.playing() {
            return self.elapsed;
        }
        let cap = if self.duration > 0.0 { self.duration } else { f64::INFINITY };
        (self.elapsed + now.saturating_duration_since(self.at).as_secs_f64()).min(cap)
    }

    /// A row only once AMS has reported a track, not merely once the phone
    /// is connected: a bonded phone with nothing queued has no now-playing.
    pub fn listed(&self) -> bool {
        self.available && !self.title.is_empty()
    }

    fn clear_track(&mut self) {
        self.title.clear();
        self.artist.clear();
        self.album.clear();
        self.duration = 0.0;
        self.playback.clear();
        self.volume = -1.0;
    }
}

/// Whether the phone link is up, set by the iPhone service.
static CONNECTED: Mutex<bool> = Mutex::new(false);
static GATE_TX: std::sync::OnceLock<async_channel::Sender<()>> = std::sync::OnceLock::new();
static COMMAND_TX: std::sync::OnceLock<async_channel::Sender<String>> = std::sync::OnceLock::new();
static COMMANDING: AtomicBool = AtomicBool::new(false);

pub fn set_connected(on: bool) {
    if let Ok(mut c) = CONNECTED.lock()
        && std::mem::replace(&mut *c, on) != on
        && let Some(tx) = GATE_TX.get()
    {
        let _ = tx.try_send(());
    }
}

/// One AMS transport command (play, pause, toggle, next, prev, volup,
/// voldown). One is in flight at a time: a held key can fire several a
/// second, and AMS answers over the same BLE write either way, so queuing
/// would only ever lag behind what the phone is doing now.
pub fn command(name: &str) -> bool {
    if COMMANDING.swap(true, Ordering::AcqRel) {
        return false;
    }
    match COMMAND_TX.get() {
        Some(tx) if tx.try_send(name.to_owned()).is_ok() => true,
        _ => {
            COMMANDING.store(false, Ordering::Release);
            false
        }
    }
}

enum Ev {
    Settings(Option<Settings>),
    Gate,
    Line(Option<String>),
    Exit(i32),
    Retry,
    Command(Option<String>),
    Reply(Option<String>),
}

fn apply(state: &mut State, event: AmsEvent) {
    match event {
        AmsEvent::Status { available } => {
            state.available = available;
            if available {
                state.error.clear();
            } else {
                state.clear_track();
            }
        }
        AmsEvent::NowPlaying { title, artist, album, duration, elapsed, playback, volume } => {
            state.title = title;
            state.artist = artist;
            state.album = album;
            state.duration = duration;
            state.playback = playback;
            if volume >= 0.0 {
                state.volume = volume;
            }
            state.elapsed = elapsed;
            state.at = Instant::now();
        }
        AmsEvent::Error { message } => state.error = message,
    }
}

fn listen() -> Option<(async_process::Child, futures_lite::io::Lines<BufReader<async_process::ChildStdout>>)> {
    let mut child = crate::services::proc::command(AMS)
        .arg("listen")
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let lines = BufReader::new(child.stdout.take()?).lines();
    Some((child, lines))
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let settings_rx = config::subscribe();
    let (gate_tx, gate_rx) = async_channel::unbounded();
    let (cmd_tx, cmd_rx) = async_channel::unbounded();
    let (reply_tx, reply_rx) = async_channel::unbounded();
    let _ = GATE_TX.set(gate_tx);
    let _ = COMMAND_TX.set(cmd_tx);

    let mut state = State::default();
    let mut settings = Settings::default();
    let mut child = None;
    let mut retry_at: Option<Instant> = None;
    let mut backoff = BASE_BACKOFF;
    let mut last = state.clone();

    loop {
        let wanted = IPHONE_ENABLE.get(&settings) && CONNECTED.lock().map(|c| *c).unwrap_or(false);
        if wanted && child.is_none() && retry_at.is_none() {
            backoff = BASE_BACKOFF;
            child = listen();
            if child.is_none() {
                // The bridge is not installed (or would not start): the
                // same answer a run that exited 127 gave.
                state.installed = false;
                state.available = false;
                retry_at = Some(Instant::now() + backoff);
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        } else if !wanted {
            retry_at = None;
            if let Some((mut c, _)) = child.take() {
                let _ = c.kill();
            }
            if state.available || !state.error.is_empty() || !state.title.is_empty() {
                state.available = false;
                state.error.clear();
                state.clear_track();
            }
        }
        // Everything but the extrapolation base compares; a new reading
        // moves `at` and `elapsed` together.
        if state.available != last.available
            || state.installed != last.installed
            || state.title != last.title
            || state.artist != last.artist
            || state.album != last.album
            || state.duration != last.duration
            || state.playback != last.playback
            || state.volume != last.volume
            || state.elapsed != last.elapsed
            || state.error != last.error
        {
            last = state.clone();
            ctx.publish(store::Diff::Media(super::media::Diff::Ams(state.clone())));
        }

        let timer = async {
            match retry_at {
                Some(at) => {
                    Timer::at(at).await;
                    Ev::Retry
                }
                None => std::future::pending().await,
            }
        };
        let event = {
            let sets = async { Ev::Settings(config::moved(&settings_rx, &IPHONE_ENABLE.get(&settings), |s| IPHONE_ENABLE.get(s)).await) };
            let gate = async {
                let _ = gate_rx.recv().await;
                Ev::Gate
            };
            let cmd = async { Ev::Command(cmd_rx.recv().await.ok()) };
            let reply = async { Ev::Reply(reply_rx.recv().await.ok()) };
            let base = sets.or(gate).or(timer).or(cmd).or(reply);
            match child.as_mut() {
                Some((c, lines)) => {
                    let line = async { Ev::Line(lines.next().await.and_then(Result::ok)) };
                    let exit = async {
                        let status = c.status().await;
                        Ev::Exit(status.ok().and_then(|s| s.code()).unwrap_or(-1))
                    };
                    base.or(line).or(exit).await
                }
                None => base.await,
            }
        };
        // A closed stdout is ready on every poll and wins the select over the
        // exit future, so the exit is awaited here instead.
        let event = match (event, child.as_mut()) {
            (Ev::Line(None), Some((c, _))) => Ev::Exit(c.status().await.ok().and_then(|s| s.code()).unwrap_or(-1)),
            (Ev::Line(None), None) => Ev::Exit(-1),
            (event, _) => event,
        };
        match event {
            Ev::Settings(Some(next)) => settings = next,
            Ev::Settings(None) => return,
            Ev::Gate => {}
            Ev::Line(Some(line)) => {
                state.installed = true;
                if let Some(event) = iphone::parse_ams_line(&line) {
                    if matches!(event, AmsEvent::Status { available: true }) {
                        backoff = BASE_BACKOFF;
                    }
                    apply(&mut state, event);
                }
            }
            Ev::Exit(code) => {
                child = None;
                state.installed = code != 127;
                state.available = false;
                if wanted {
                    retry_at = Some(Instant::now() + backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
            Ev::Line(None) => unreachable!("a closed stdout became an exit above"),
            Ev::Retry => retry_at = None,
            Ev::Command(Some(name)) => {
                let reply_tx = reply_tx.clone();
                ctx.spawn(async move {
                    let out = crate::services::proc::command(AMS)
                        .args(["command", &name])
                        .stdin(async_process::Stdio::null())
                        .stderr(async_process::Stdio::null())
                        .kill_on_drop(true)
                        .output()
                        .await;
                    if let Ok(out) = out {
                        for line in String::from_utf8_lossy(&out.stdout).lines() {
                            let _ = reply_tx.send(line.to_owned()).await;
                        }
                    }
                    COMMANDING.store(false, Ordering::Release);
                });
            }
            Ev::Command(None) => return,
            Ev::Reply(Some(line)) => {
                if let Some(event) = iphone::parse_ams_line(&line) {
                    apply(&mut state, event);
                }
            }
            Ev::Reply(None) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_runs_forward_only_while_playing_and_stops_at_the_end() {
        let at = Instant::now();
        let mut s = State { elapsed: 10.0, at, duration: 12.0, playback: "playing".into(), ..State::default() };
        assert_eq!(s.position(at + Duration::from_secs(1)), 11.0);
        assert_eq!(s.position(at + Duration::from_secs(30)), 12.0);
        s.playback = "paused".into();
        assert_eq!(s.position(at + Duration::from_secs(30)), 10.0);
    }

    #[test]
    fn a_track_is_listed_once_ams_reports_one() {
        let mut s = State::default();
        apply(&mut s, AmsEvent::Status { available: true });
        assert!(!s.listed());
        apply(
            &mut s,
            AmsEvent::NowPlaying { title: "T".into(), artist: "A".into(), album: String::new(), duration: 0.0, elapsed: 0.0, playback: "playing".into(), volume: -1.0 },
        );
        assert!(s.listed());
        assert_eq!(s.volume, -1.0);
        apply(&mut s, AmsEvent::Status { available: false });
        assert!(!s.listed());
        assert_eq!(s.playback, "");
    }
}
