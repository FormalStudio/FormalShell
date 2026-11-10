//! The network panel's speed test: a few curls against Cloudflare while the
//! interface's own counters are sampled, in two five-second phases. Its
//! state rides the published [`Network`](super::Network) for as long as the
//! panel is open.

use std::time::{Duration, Instant};

use async_io::Timer;
use fs_devices::network::speedtest;
use futures_lite::FutureExt;

use super::{INNER, publish};
use crate::services::{http, proc};
use crate::runtime::Ctx;

const ST_PHASE: Duration = Duration::from_millis(5000);
const ST_SAMPLE: Duration = Duration::from_millis(500);
const ST_DOWN_URL: &str = "https://speed.cloudflare.com/__down?bytes=25000000";
const ST_UP_URL: &str = "https://speed.cloudflare.com/__up";
/// Well past a phase, which drops the worker; curl itself is given no tighter limit.
const WORKER_CAP: u32 = 120;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Resolving,
    Down,
    Up,
    Done,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::Resolving => "resolving",
            Phase::Down => "down",
            Phase::Up => "up",
            Phase::Done => "done",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Speed {
    pub phase: Phase,
    pub error: String,
    pub down: speedtest::Window,
    pub up: speedtest::Window,
    pub down_result: f64,
    pub up_result: f64,
}

impl Speed {
    pub fn running(&self) -> bool {
        matches!(self.phase, Phase::Resolving | Phase::Down | Phase::Up)
    }

    pub fn down_mbps(&self) -> f64 {
        if self.phase == Phase::Down { self.down.live_mbps } else { self.down_result }
    }

    pub fn up_mbps(&self) -> f64 {
        if self.phase == Phase::Up { self.up.live_mbps } else { self.up_result }
    }
}

/// False when a test is already running.
pub(super) fn speed_start(ctx: &Ctx) -> bool {
    let started = INNER.with_borrow_mut(|i| {
        if i.speed.running() {
            return None;
        }
        i.speed = Speed { phase: Phase::Resolving, ..Speed::default() };
        let (stop, stopped) = async_channel::bounded::<()>(1);
        i.speed_stop = Some(stop);
        Some(stopped)
    });
    let Some(stopped) = started else { return false };
    publish();
    ctx.spawn(
        async {
            speed_run().await;
        }
        .or(async move {
            let _ = stopped.recv().await;
        }),
    );
    true
}

/// `_stopSpeedTest`: back to idle, results dropped.
pub(super) fn speed_stop() {
    let was = INNER.with_borrow_mut(|i| {
        i.speed_stop = None;
        let was = i.speed.phase != Phase::Idle;
        i.speed = Speed::default();
        was
    });
    if was {
        publish();
    }
}

fn speed_set(f: impl FnOnce(&mut Speed)) {
    INNER.with_borrow_mut(|i| f(&mut i.speed));
    publish();
}

fn speed_abort(message: &str) {
    speed_set(|s| {
        s.error = message.into();
        s.phase = Phase::Done;
    });
    INNER.with_borrow_mut(|i| i.speed_stop = None);
}

/// `(rx, tx)` off the interface's own counters; a missing interface is
/// `None`, which ends the test honestly rather than reporting a stale rate.
fn stat_bytes(iface: &str) -> Option<(f64, f64)> {
    let read = |which: &str| std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{which}"));
    let text = format!("{}{}", read("rx_bytes").ok()?, read("tx_bytes").ok()?);
    speedtest::parse_stat_bytes(&text).map(|b| (b.rx, b.tx))
}

/// One worker: transfer after transfer until the phase drops it. Each curl
/// is this process's own child, so dropping the future kills it by pid.
async fn worker(up: bool) {
    loop {
        let req = if up {
            http::Request::get(ST_UP_URL, WORKER_CAP).upload("/dev/zero")
        } else {
            http::Request::get(ST_DOWN_URL, WORKER_CAP)
        };
        if http::send(&req.output("/dev/null")).await.is_err() {
            return;
        }
    }
}

/// Samples every half second until the phase's own five seconds are up.
/// False when the interface's counters went away.
async fn phase(iface: &str, up: bool) -> bool {
    let workers = async {
        use futures_lite::future::zip;
        zip(zip(worker(up), worker(up)), zip(worker(up), worker(up))).await;
        futures_lite::future::pending::<bool>().await
    };
    let sampling = async {
        let start = Instant::now();
        loop {
            let Some((rx, tx)) = stat_bytes(iface) else { return false };
            let t = start.elapsed().as_secs_f64() * 1000.0;
            speed_set(|s| {
                if up {
                    s.up = speedtest::add_sample(&s.up, t, tx);
                } else {
                    s.down = speedtest::add_sample(&s.down, t, rx);
                }
            });
            let left = ST_PHASE.saturating_sub(start.elapsed());
            if left.is_zero() {
                return true;
            }
            Timer::after(left.min(ST_SAMPLE)).await;
        }
    };
    sampling.or(workers).await
}

async fn speed_run() {
    let iface = speedtest::parse_iface(&proc::capture(&proc::argv(&["ip", "route", "get", "1.1.1.1"]), Duration::from_secs(5)).await.stdout);
    let Some(iface) = iface else { return speed_abort("NO NETWORK") };
    if !proc::on_path("curl") {
        return speed_abort("NO CURL");
    }
    speed_set(|s| s.phase = Phase::Down);
    if !phase(&iface, false).await {
        return speed_abort("NO NETWORK");
    }
    speed_set(|s| {
        s.down_result = s.down.avg_mbps;
        s.phase = Phase::Up;
    });
    if !phase(&iface, true).await {
        return speed_abort("NO NETWORK");
    }
    speed_set(|s| {
        s.up_result = s.up.avg_mbps;
        s.phase = Phase::Done;
    });
    INNER.with_borrow_mut(|i| i.speed_stop = None);
}
