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
use crate::services::proc;
use crate::settings::IPHONE_ENABLE;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const AMS: &str = "omarchy-iphone-ams";
/// One media command written over the phone's GATT link.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// The track changed and no playback reading has come since, so
    /// `elapsed` still belongs to the last one.
    pub position_stale: bool,
    /// When the track changed, while no playback reading has come since.
    changed_at: Option<Instant>,
    /// The elapsed and playback the bridge's last line carried, apart from
    /// the extrapolation base, which a run-out wait resets to 0. A legacy
    /// bridge's line is a reading only when it moves these; `None` once the
    /// session or track is gone, so the next line after a reconnect reads.
    sent: Option<(f64, String)>,
    pub error: String,
}

/// How long a new track waits for a playback reading before it is taken to
/// start at 0 at the change. The phone sends PlaybackInfo only when playback
/// changes, so a track it moves to without one may send none at all; a new
/// track starting at 0 is honest, and lyrics never stay dark.
const READING_WAIT: Duration = Duration::from_secs(1);

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
            position_stale: false,
            changed_at: None,
            sent: None,
            error: String::new(),
        }
    }
}

impl State {
    pub fn playing(&self) -> bool {
        self.playback == "playing"
    }

    /// Everything but the extrapolation base compares; a new reading moves
    /// `at` and `elapsed` together.
    pub fn same(&self, other: &State) -> bool {
        self.installed == other.installed
            && self.available == other.available
            && self.title == other.title
            && self.artist == other.artist
            && self.album == other.album
            && self.duration == other.duration
            && self.playback == other.playback
            && self.volume == other.volume
            && self.elapsed == other.elapsed
            && self.position_stale == other.position_stale
            && self.error == other.error
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

    /// When a stale position runs out of [`READING_WAIT`].
    fn reading_wait(&self) -> Option<Instant> {
        self.changed_at.map(|at| at + READING_WAIT)
    }

    /// A track that waited [`READING_WAIT`] for a reading and got none
    /// starts at 0 at its change, and runs on from there while playing.
    fn expire_reading(&mut self, now: Instant) {
        if let Some(at) = self.changed_at.filter(|&at| now >= at + READING_WAIT) {
            (self.elapsed, self.at) = (0.0, at);
            self.position_stale = false;
            self.changed_at = None;
        }
    }

    fn clear_track(&mut self) {
        self.sent = None;
        self.position_stale = false;
        self.changed_at = None;
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
static COMMAND_TX: std::sync::OnceLock<async_channel::Sender<Command>> = std::sync::OnceLock::new();
static COMMANDING: AtomicBool = AtomicBool::new(false);

pub fn set_connected(on: bool) {
    if let Ok(mut c) = CONNECTED.lock()
        && std::mem::replace(&mut *c, on) != on
        && let Some(tx) = GATE_TX.get()
    {
        let _ = tx.try_send(());
    }
}

/// An AMS transport command, one of the bridge's `command` verbs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Next,
    Prev,
    VolUp,
    VolDown,
}

impl Command {
    fn verb(self) -> &'static str {
        match self {
            Command::Toggle => "toggle",
            Command::Next => "next",
            Command::Prev => "prev",
            Command::VolUp => "volup",
            Command::VolDown => "voldown",
        }
    }
}

/// One is in flight at a time: a held key can fire several a second, and
/// AMS answers over the same BLE write either way, so queuing would only
/// ever lag behind what the phone is doing now.
pub fn command(cmd: Command) -> bool {
    if COMMANDING.swap(true, Ordering::AcqRel) {
        return false;
    }
    match COMMAND_TX.get() {
        Some(tx) if tx.try_send(cmd).is_ok() => true,
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
    Command(Option<Command>),
    Reply(Option<String>),
    ReadingWait,
}

fn apply(state: &mut State, event: AmsEvent) {
    apply_at(state, event, Instant::now());
}

/// The bridge writes one line per attribute the phone updates, each carrying
/// the whole state. New names make a new track, its position stale until the
/// first PlaybackInfo line after them, whatever elapsed that line repeats; a
/// duration alone is the same track refined. Only a PlaybackInfo line moves
/// the extrapolation base. A bridge that names no line's update (the hosts'
/// pinned package before `packaging/iphone-ams-update.patch`) is read the
/// old way: a line whose elapsed or playback moved from the last line is a
/// reading. That bridge cannot tell a seek back to the last elapsed it sent
/// from a repeat, so such a seek goes unread until the patched bridge.
fn apply_at(state: &mut State, event: AmsEvent, now: Instant) {
    match event {
        AmsEvent::Status { available } => {
            state.available = available;
            if available {
                state.error.clear();
            } else {
                state.clear_track();
            }
        }
        AmsEvent::NowPlaying { title, artist, album, duration, elapsed, playback, volume, reading } => {
            if (&title, &artist, &album) != (&state.title, &state.artist, &state.album) {
                state.position_stale = true;
                state.changed_at = Some(now);
            }
            let moved = state.sent.as_ref().is_none_or(|(e, p)| (*e, p) != (elapsed, &playback));
            state.sent = Some((elapsed, playback.clone()));
            if reading.unwrap_or(moved) {
                (state.elapsed, state.at) = (elapsed, now);
                state.position_stale = false;
                state.changed_at = None;
            }
            state.title = title;
            state.artist = artist;
            state.album = album;
            state.duration = duration;
            state.playback = playback;
            if volume >= 0.0 {
                state.volume = volume;
            }
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
            state.sent = None;
            if state.available || !state.error.is_empty() || !state.title.is_empty() {
                state.available = false;
                state.error.clear();
                state.clear_track();
            }
        }
        if !state.same(&last) {
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
        let reading_wait = state.reading_wait();
        let reading = async {
            match reading_wait {
                Some(at) => {
                    Timer::at(at).await;
                    Ev::ReadingWait
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
            let base = sets.or(gate).or(timer).or(reading).or(cmd).or(reply);
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
                state.sent = None;
                state.installed = code != 127;
                state.available = false;
                if wanted {
                    retry_at = Some(Instant::now() + backoff);
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
            }
            Ev::Line(None) => unreachable!("a closed stdout became an exit above"),
            Ev::Retry => retry_at = None,
            Ev::Command(Some(cmd)) => {
                let reply_tx = reply_tx.clone();
                ctx.spawn(async move {
                    let done = proc::capture(&proc::argv(&[AMS, "command", cmd.verb()]), COMMAND_TIMEOUT).await;
                    for line in done.stdout.lines() {
                        let _ = reply_tx.send(line.to_owned()).await;
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
            Ev::ReadingWait => state.expire_reading(Instant::now()),
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

    /// A line the bridge writes for a track attribute (`reading` false) or
    /// for the phone's PlaybackInfo (`reading` true).
    fn line(title: &str, duration: f64, elapsed: f64, playback: &str, reading: bool) -> AmsEvent {
        now_playing(title, duration, elapsed, playback, Some(reading))
    }

    /// The same line from a bridge that names no update.
    fn legacy(title: &str, duration: f64, elapsed: f64, playback: &str) -> AmsEvent {
        now_playing(title, duration, elapsed, playback, None)
    }

    fn now_playing(title: &str, duration: f64, elapsed: f64, playback: &str, reading: Option<bool>) -> AmsEvent {
        AmsEvent::NowPlaying {
            title: title.into(),
            artist: "A".into(),
            album: String::new(),
            duration,
            elapsed,
            playback: playback.into(),
            volume: -1.0,
            reading,
        }
    }

    #[test]
    fn a_legacy_bridge_reads_a_moved_elapsed_after_a_change() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, legacy("One", 200.0, 150.0, "playing"), t0);
        assert!(!s.position_stale && s.elapsed == 150.0);

        let change = t0 + Duration::from_secs(40);
        apply_at(&mut s, legacy("Two", 200.0, 150.0, "playing"), change);
        assert!(s.position_stale, "the title line repeats the old elapsed");
        assert_eq!(s.at, t0);
        apply_at(&mut s, legacy("Two", 200.0, 0.3, "playing"), change + Duration::from_millis(80));
        assert!(!s.position_stale && s.reading_wait().is_none() && s.elapsed == 0.3);
    }

    #[test]
    fn a_legacy_bridge_honours_seeks_and_a_title_line_keeps_the_base() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, legacy("One", 200.0, 10.0, "playing"), t0);
        apply_at(&mut s, legacy("One", 200.0, 90.0, "playing"), t0 + Duration::from_secs(5));
        assert_eq!(s.position(t0 + Duration::from_secs(6)), 91.0, "a seek moves the base");
        apply_at(&mut s, legacy("One", 200.0, 90.0, "paused"), t0 + Duration::from_secs(7));
        assert_eq!(s.position(t0 + Duration::from_secs(30)), 90.0, "a pause is a reading");
        apply_at(&mut s, legacy("One", 181.0, 90.0, "paused"), t0 + Duration::from_secs(8));
        assert_eq!(s.at, t0 + Duration::from_secs(7), "a line moving nothing is no reading");
    }

    #[test]
    fn a_legacy_bridge_starts_a_silent_track_at_zero_once_the_wait_runs_out() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, legacy("One", 200.0, 150.0, "playing"), t0);
        let change = t0 + Duration::from_secs(30);
        apply_at(&mut s, legacy("Two", 200.0, 150.0, "playing"), change);
        s.expire_reading(s.reading_wait().unwrap());
        assert!(!s.position_stale);
        assert_eq!(s.position(change + Duration::from_secs(2)), 2.0);
    }

    #[test]
    fn a_legacy_repeat_after_the_wait_keeps_the_new_track_at_zero() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, legacy("One", 200.0, 150.0, "playing"), t0);
        let change = t0 + Duration::from_secs(30);
        apply_at(&mut s, legacy("Two", 200.0, 150.0, "playing"), change);
        s.expire_reading(s.reading_wait().unwrap());
        apply_at(&mut s, legacy("Two", 200.0, 150.0, "playing"), change + Duration::from_secs(3));
        assert_eq!(s.position(change + Duration::from_secs(4)), 4.0, "the repeat is the old track's elapsed");
    }

    #[test]
    fn a_legacy_line_after_a_reconnect_is_a_reading() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, AmsEvent::Status { available: true }, t0);
        apply_at(&mut s, legacy("One", 200.0, 150.0, "playing"), t0);
        apply_at(&mut s, AmsEvent::Status { available: false }, t0 + Duration::from_secs(1));
        apply_at(&mut s, AmsEvent::Status { available: true }, t0 + Duration::from_secs(5));
        let back = t0 + Duration::from_secs(6);
        apply_at(&mut s, legacy("One", 200.0, 150.0, "playing"), back);
        assert!(!s.position_stale && s.reading_wait().is_none());
        assert_eq!(s.position(back + Duration::from_secs(2)), 152.0);
    }

    #[test]
    fn a_title_holds_the_position_stale_until_the_phone_reads_elapsed_for_it() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, line("One", 200.0, 0.0, "playing", true), t0);
        apply_at(&mut s, line("One", 200.0, 150.0, "playing", true), t0 + Duration::from_secs(150));
        assert!(!s.position_stale);

        // The new title's line repeats the old elapsed.
        let later = t0 + Duration::from_secs(200);
        apply_at(&mut s, line("Two", 200.0, 150.0, "playing", false), later);
        assert!(s.position_stale && s.elapsed == 150.0);
        assert_eq!(s.at, t0 + Duration::from_secs(150), "a track line is no reading");
        apply_at(&mut s, line("Two", 200.0, 0.4, "playing", true), later + Duration::from_millis(80));
        assert!(!s.position_stale && s.elapsed == 0.4);
    }

    #[test]
    fn a_reading_before_the_title_is_the_last_tracks() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, line("One", 200.0, 150.0, "paused", true), t0);
        apply_at(&mut s, line("One", 200.0, 0.0, "paused", true), t0 + Duration::from_secs(60));
        apply_at(&mut s, line("Two", 200.0, 0.0, "paused", false), t0 + Duration::from_secs(60));
        assert!(s.position_stale);
    }

    #[test]
    fn a_repeated_elapsed_after_a_change_is_still_a_reading() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, line("One", 200.0, 0.0, "paused", true), t0);
        apply_at(&mut s, line("Two", 200.0, 0.0, "paused", false), t0 + Duration::from_secs(5));
        assert!(s.position_stale);
        apply_at(&mut s, line("Two", 200.0, 0.0, "paused", true), t0 + Duration::from_secs(5));
        assert!(!s.position_stale && s.reading_wait().is_none());
    }

    #[test]
    fn a_duration_refinement_keeps_the_claimed_reading() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, line("One", 200.0, 0.0, "playing", true), t0);
        apply_at(&mut s, line("Two", 200.0, 0.0, "playing", false), t0 + Duration::from_secs(9));
        apply_at(&mut s, line("Two", 200.0, 3.0, "playing", true), t0 + Duration::from_secs(10));
        apply_at(&mut s, line("Two", 181.0, 3.0, "playing", false), t0 + Duration::from_secs(11));
        assert!(!s.position_stale && s.duration == 181.0);
        assert_eq!(s.position(t0 + Duration::from_secs(12)), 5.0);
    }

    #[test]
    fn a_track_that_never_gets_a_reading_starts_at_zero_once_the_wait_runs_out() {
        let t0 = Instant::now();
        let mut s = State::default();
        apply_at(&mut s, line("One", 200.0, 150.0, "playing", true), t0);
        let change = t0 + Duration::from_secs(30);
        apply_at(&mut s, line("Two", 200.0, 150.0, "playing", false), change);
        let at = s.reading_wait().unwrap();
        s.expire_reading(at - Duration::from_millis(1));
        assert!(s.position_stale);
        s.expire_reading(at);
        assert!(!s.position_stale && s.reading_wait().is_none());
        assert_eq!(s.position(change + Duration::from_secs(4)), 4.0, "run on from the change");

        apply_at(&mut s, line("Three", 200.0, 4.0, "paused", true), change + Duration::from_secs(4));
        apply_at(&mut s, line("Four", 200.0, 4.0, "paused", false), change + Duration::from_secs(9));
        s.expire_reading(change + Duration::from_secs(60));
        assert!(!s.position_stale && s.position(change + Duration::from_secs(90)) == 0.0, "paused at 0");
    }

    #[test]
    fn a_track_is_listed_once_ams_reports_one() {
        let mut s = State::default();
        apply(&mut s, AmsEvent::Status { available: true });
        assert!(!s.listed());
        apply(
            &mut s,
            AmsEvent::NowPlaying { title: "T".into(), artist: "A".into(), album: String::new(), duration: 0.0, elapsed: 0.0, playback: "playing".into(), volume: -1.0, reading: Some(true) },
        );
        assert!(s.listed());
        assert_eq!(s.volume, -1.0);
        apply(&mut s, AmsEvent::Status { available: false });
        assert!(!s.listed());
        assert_eq!(s.playback, "");
    }
}
