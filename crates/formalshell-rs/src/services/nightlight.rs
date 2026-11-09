//! The opt-in night light, wlsunset
//! (wlr-gamma-control-unstable-v1) held at its low temperature for as long
//! as it runs.
//!
//! wlsunset has no fixed-temperature mode, but its documented SIGUSR1
//! cycle (automatic, forced high, forced low) does the job: two signals pin
//! the low temperature and bypass the sun calculation, the dummy -S/-s
//! times only route around its geo validation. Standard signals are not
//! queued, so each one waits for the previous one's stderr confirmation.
//! Any exit nobody asked for is an error (wlsunset exits 0 on a protocol
//! error); a few at session start ("Gamma ramps size mismatch" while the
//! output settles) are retried.
//!
//! `nightlight.schedule` ("sun", the default, or "off") turns it on at
//! sunset and off at sunrise off the same sun pair `theme.mode: "auto"`
//! reads. Only a crossing acts, so an enable or a disable in between holds
//! until the next one.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::Timer;
use async_process::Stdio;
use chrono::Local;
use fs_theme::sun;
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, FutureExt, StreamExt};
use serde_json::Value;

use super::config;
use crate::runtime::Ctx;
use crate::store;

const DEFAULT_TEMP: i64 = crate::settings::NIGHTLIGHT_TEMP.default as i64;
const RETRIES: u32 = 5;
const RETRY_AFTER: Duration = Duration::from_secs(3);
/// The signal is also sent this long after the process comes up, in case
/// its confirmation line never arrives.
const ARM_AFTER: Duration = Duration::from_millis(150);
/// A suspend does not advance the monotonic clock the sun timer runs on.
const TICK_CAP: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub temp: i64,
    pub scheduled: bool,
    pub start_on: bool,
    pub location: Option<(f64, f64)>,
}

impl Settings {
    fn read(settings: &(impl crate::settings::Doc + ?Sized)) -> Self {
        use crate::settings::{NIGHTLIGHT_SCHEDULE, NIGHTLIGHT_START_ON, location, nightlight_temp};
        Self {
            temp: nightlight_temp(settings),
            scheduled: NIGHTLIGHT_SCHEDULE.get(settings) == "sun",
            start_on: NIGHTLIGHT_START_ON.get(settings),
            location: location(settings),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub active: bool,
    pub last_error: String,
    pub temp: i64,
    pub scheduled: bool,
    /// What the schedule says right now, none while it is off.
    pub schedule_dark: Option<bool>,
    /// The schedule runs off a location rather than the fixed window.
    pub located: bool,
}

impl Default for State {
    fn default() -> Self {
        Self { active: false, last_error: String::new(), temp: DEFAULT_TEMP, scheduled: false, schedule_dark: None, located: false }
    }
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        let changed = *self != diff.0;
        *self = diff.0;
        changed
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> Value {
        serde_json::json!({
            "active": self.active,
            "temp": self.temp,
            "schedule": if self.scheduled { "sun" } else { "off" },
            "scheduleDark": self.schedule_dark,
            "source": if self.located { "location" } else { "fallback" },
            "lastError": self.last_error,
        })
    }
}

/// Where the SIGUSR1 handshake stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    AwaitingHigh,
    AwaitingLow,
}

/// wlsunset's own confirmation lines move the handshake on; true when the
/// next signal is due.
fn on_line(phase: Phase, line: &str) -> (Phase, bool) {
    match phase {
        Phase::AwaitingHigh if line.contains("forcing high temperature") => (Phase::AwaitingLow, true),
        Phase::AwaitingLow if line.contains("forcing low temperature") => (Phase::Idle, false),
        other => (other, false),
    }
}

enum Msg {
    Enable,
    Disable,
    Toggle,
    Child(u64, Event),
}

enum Event {
    Started(u32),
    FailedToStart,
    Line(String),
    Exited(i32),
}

static CHANNEL: LazyLock<(Sender<Msg>, Receiver<Msg>)> = LazyLock::new(async_channel::unbounded);

fn send(msg: Msg) {
    let _ = CHANNEL.0.try_send(msg);
}

pub fn enable() {
    send(Msg::Enable);
}

pub fn disable() {
    send(Msg::Disable);
}

pub fn toggle() {
    send(Msg::Toggle);
}

struct Run {
    generation: u64,
    pid: Option<u32>,
}

struct Machine {
    ctx: Ctx,
    settings: Settings,
    loaded: bool,
    state: State,
    run: Option<Run>,
    generation: u64,
    phase: Phase,
    stopping: bool,
    retries: u32,
    stderr: String,
    arm_at: Option<Instant>,
    retry_at: Option<Instant>,
    tick_at: Option<Instant>,
}

impl Machine {
    fn new(ctx: Ctx) -> Self {
        let settings = Settings { temp: DEFAULT_TEMP, scheduled: false, start_on: false, location: None };
        Self {
            ctx,
            settings,
            loaded: false,
            state: State::default(),
            run: None,
            generation: 0,
            phase: Phase::Idle,
            stopping: false,
            retries: 0,
            stderr: String::new(),
            arm_at: None,
            retry_at: None,
            tick_at: None,
        }
    }

    fn publish(&mut self) {
        self.state.active = self.run.is_some();
        self.state.temp = self.settings.temp;
        self.state.scheduled = self.loaded && self.settings.scheduled;
        self.ctx.publish(store::Diff::NightLight(Diff(self.state.clone())));
    }

    fn start(&mut self) {
        if self.run.is_some() {
            return;
        }
        self.state.last_error.clear();
        self.phase = Phase::AwaitingHigh;
        self.stopping = false;
        self.stderr.clear();
        self.generation += 1;
        let generation = self.generation;
        self.run = Some(Run { generation, pid: None });
        let temp = self.settings.temp.to_string();
        let tx = CHANNEL.0.clone();
        self.ctx.spawn(async move {
            let child = crate::services::proc::command("setpriv")
                .args(["--pdeathsig", "TERM", "--", "wlsunset", "-t", &temp, "-S", "06:00", "-s", "18:00"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn();
            let mut child = match child {
                Ok(child) => child,
                Err(_) => {
                    let _ = tx.send(Msg::Child(generation, Event::FailedToStart)).await;
                    return;
                }
            };
            let _ = tx.send(Msg::Child(generation, Event::Started(child.id()))).await;
            if let Some(stderr) = child.stderr.take() {
                let mut lines = BufReader::new(stderr).lines();
                while let Some(Ok(line)) = lines.next().await {
                    let _ = tx.send(Msg::Child(generation, Event::Line(line))).await;
                }
            }
            let code = child.status().await.ok().and_then(|s| s.code()).unwrap_or(-1);
            let _ = tx.send(Msg::Child(generation, Event::Exited(code))).await;
        });
    }

    fn signal(&self, name: &'static str) {
        let Some(pid) = self.run.as_ref().and_then(|r| r.pid) else { return };
        self.ctx.spawn(async move {
            let _ = crate::services::proc::command("kill").arg(format!("-{name}")).arg(pid.to_string()).stdout(Stdio::null()).stderr(Stdio::null()).status().await;
        });
    }

    fn enable(&mut self) {
        self.retries = 0;
        self.start();
    }

    fn disable(&mut self) {
        self.retry_at = None;
        if self.run.is_none() {
            return;
        }
        self.stopping = true;
        self.phase = Phase::Idle;
        self.arm_at = None;
        self.signal("TERM");
    }

    fn child(&mut self, generation: u64, event: Event) {
        if self.run.as_ref().is_none_or(|r| r.generation != generation) {
            return;
        }
        match event {
            Event::Started(pid) => {
                if let Some(run) = &mut self.run {
                    run.pid = Some(pid);
                }
                if self.stopping {
                    self.signal("TERM");
                } else {
                    self.arm_at = Some(Instant::now() + ARM_AFTER);
                }
            }
            Event::FailedToStart => {
                self.run = None;
                self.phase = Phase::Idle;
                if !self.stopping {
                    self.state.last_error = "wlsunset not found (failed to start)".into();
                }
            }
            Event::Line(line) => {
                self.stderr = line.clone();
                let (phase, due) = on_line(self.phase, &line);
                self.phase = phase;
                if due {
                    self.signal("USR1");
                }
            }
            Event::Exited(code) => {
                self.run = None;
                self.phase = Phase::Idle;
                self.arm_at = None;
                if self.stopping {
                    self.stopping = false;
                    return;
                }
                self.state.last_error = if self.stderr.is_empty() { format!("wlsunset exited {code}") } else { self.stderr.clone() };
                if self.retries < RETRIES {
                    self.retries += 1;
                    self.retry_at = Some(Instant::now() + RETRY_AFTER);
                }
            }
        }
    }

    fn settings(&mut self, settings: Settings) {
        let first = !self.loaded;
        self.loaded = true;
        self.settings = settings;
        if first && self.settings.start_on {
            self.enable();
        }
        self.tick();
    }

    /// The schedule against the clock: a crossing acts, and the next one
    /// sets the next wake.
    fn tick(&mut self) {
        self.tick_at = None;
        if !self.loaded || !self.settings.scheduled {
            self.state.schedule_dark = None;
            self.state.located = false;
            return;
        }
        let now = Local::now();
        let times = self.settings.location.and_then(|(lat, lon)| sun::sun_times(&now, lat, lon, None));
        self.state.located = times.is_some();
        let dark = sun::is_dark(&now, times.as_ref());
        if self.state.schedule_dark != Some(dark) {
            self.state.schedule_dark = Some(dark);
            if dark { self.enable() } else { self.disable() }
        }
        let until = (sun::next_change(&now, times.as_ref()) - now).to_std().unwrap_or(Duration::from_secs(1)) + Duration::from_secs(1);
        self.tick_at = Some(Instant::now() + until.min(TICK_CAP));
    }

    fn deadline(&self) -> Option<Instant> {
        [self.arm_at, self.retry_at, self.tick_at].into_iter().flatten().min()
    }

    fn due(&mut self, now: Instant) {
        if self.arm_at.is_some_and(|t| t <= now) {
            self.arm_at = None;
            if self.run.is_some() && self.phase == Phase::AwaitingHigh {
                self.signal("USR1");
            }
        }
        if self.retry_at.is_some_and(|t| t <= now) {
            self.retry_at = None;
            self.start();
        }
        if self.tick_at.is_some_and(|t| t <= now) {
            self.tick();
        }
    }
}

enum Wake {
    Msg(Option<Msg>),
    Settings(Option<config::Settings>),
    Timer,
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let rx = CHANNEL.1.clone();
    let settings = config::subscribe();
    let mut m = Machine::new(ctx);
    m.publish();
    loop {
        // Nothing until config's first publish, so `startOn` acts on the
        // document with state.json's overrides in it.
        let read = |s: &config::Settings| Some(Settings::read(s));
        let current = m.loaded.then(|| m.settings.clone());
        let recv = async { Wake::Msg(rx.recv().await.ok()) }.or(async { Wake::Settings(config::moved(&settings, &current, read).await) });
        let wake = match m.deadline() {
            Some(at) => {
                let timer = async {
                    Timer::at(at).await;
                    Wake::Timer
                };
                recv.or(timer).await
            }
            None => recv.await,
        };
        match wake {
            Wake::Msg(None) => return,
            Wake::Settings(None) => return,
            Wake::Settings(Some(s)) => m.settings(Settings::read(&s)),
            Wake::Msg(Some(msg)) => match msg {
                Msg::Enable => m.enable(),
                Msg::Disable => m.disable(),
                Msg::Toggle => {
                    if m.run.is_some() { m.disable() } else { m.enable() }
                }
                Msg::Child(g, e) => m.child(g, e),
            },
            Wake::Timer => m.due(Instant::now()),
        }
        m.publish();
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_handshake_walks_high_then_low() {
        assert_eq!(on_line(Phase::AwaitingHigh, "forcing high temperature"), (Phase::AwaitingLow, true));
        assert_eq!(on_line(Phase::AwaitingLow, "forcing low temperature"), (Phase::Idle, false));
        assert_eq!(on_line(Phase::AwaitingHigh, "forcing low temperature"), (Phase::AwaitingHigh, false));
        assert_eq!(on_line(Phase::Idle, "forcing high temperature"), (Phase::Idle, false));
    }

    #[test]
    fn settings_default_to_the_sun_schedule_at_4000k() {
        let s = Settings::read(&json!({}));
        assert_eq!((s.temp, s.scheduled, s.start_on, s.location), (4000, true, false, None));
        let s = Settings::read(&json!({"nightlight": {"schedule": "off", "temp": 3500, "startOn": true}, "location": {"latitude": 1.5, "longitude": 2.5}}));
        assert_eq!((s.temp, s.scheduled, s.start_on, s.location), (3500, false, true, Some((1.5, 2.5))));
    }

    #[test]
    fn status_reads_like_the_golden_reply() {
        let mut s = State::default();
        assert_eq!(s.status().to_string(), r#"{"active":false,"temp":4000,"schedule":"off","scheduleDark":null,"source":"fallback","lastError":""}"#);
        s.scheduled = true;
        s.schedule_dark = Some(true);
        s.located = true;
        s.last_error = "x \"y\"".into();
        assert_eq!(s.status().to_string(), r#"{"active":false,"temp":4000,"schedule":"sun","scheduleDark":true,"source":"location","lastError":"x \"y\""}"#);
    }
}
