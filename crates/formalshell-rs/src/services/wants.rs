//! Work that runs only while something on screen wants it. A cell, a panel
//! or a service's own pick holds a [`Want`] on a [`Source`] for as long as
//! it needs it. A poll source's first want starts its task on the service
//! thread and the last one's release stops it a grace period later, so a
//! config reload that rebuilds the strip does not restart every poll. Every
//! other source is a demand its service reads through [`watch`], switched
//! the moment the count crosses zero: a panel's close has to stop what it
//! held at once (the earbuds' heart rate stream, the Wi-Fi scan).
//! A bar naming none of these cells costs nothing.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::Timer;
use futures_lite::FutureExt;

use super::{devices, info};
use crate::runtime::Ctx;

macro_rules! sources {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Source {
            $($(#[$doc])* $name,)*
        }

        impl Source {
            pub const ALL: &[Source] = &[$(Source::$name),*];
        }
    };
}

sources! {
    Weather,
    Github,
    Usage,
    SystemUpdate,
    Tailscale,
    Monitor,
    Calendar,
    Display,
    Processes,
    PowerFlow,
    About,
    /// The DualSense's lightbar and player LEDs, read off sysfs.
    PadLights,
    /// The earbuds backends' watch: a cell, the headset card or the panel.
    Earbuds,
    /// The AirPods' heart rate and head motion streams, the panel's alone.
    EarbudsSensors,
    /// Every Bluetooth device, strangers included, and discovery.
    BluetoothDevices,
    /// The audio panel's device and stream lists.
    AudioLists,
    /// The sinks and streams a media source can be moved between.
    AudioRouting,
    /// NetworkManager's periodic Wi-Fi scan.
    WifiScan,
    /// cava's spectrum, for a bar cell or the media panel's band.
    Spectrum,
    /// The Kopuz daemon, while its player is the active source.
    Kopuz,
    /// The output latency read, while a synced track holds its lyrics back.
    LyricsLatency,
    /// The T3 Code agent pollers, while a Spaces chip can show a badge.
    AgentBadges,
}

const COUNT: usize = Source::ALL.len();

/// How long a poll with no wanter keeps running.
const POLL_GRACE: Duration = Duration::from_secs(2);

impl Source {
    /// The poll the source runs while wanted; `None` for a demand its
    /// service reads through [`watch`].
    fn task(self, ctx: Ctx) -> Option<Pin<Box<dyn Future<Output = ()>>>> {
        Some(match self {
            Source::Weather => Box::pin(info::weather::run(ctx)),
            Source::Github => Box::pin(info::github::run(ctx)),
            Source::Usage => Box::pin(info::usage::run(ctx)),
            Source::SystemUpdate => Box::pin(info::update::run(ctx)),
            Source::Tailscale => Box::pin(info::tailscale::run(ctx)),
            Source::Monitor => Box::pin(info::monitor::run(ctx)),
            Source::Calendar => Box::pin(info::calendar::run(ctx)),
            Source::Display => Box::pin(super::display::run(ctx)),
            Source::Processes => Box::pin(info::procs::run(ctx)),
            Source::PowerFlow => Box::pin(info::powerflow::run(ctx)),
            Source::About => Box::pin(info::about::run(ctx)),
            Source::PadLights => Box::pin(devices::power::lights(ctx)),
            _ => return None,
        })
    }

    /// The info polls ride out a strip rebuild; a device or media demand
    /// goes with its last holder.
    fn grace(self) -> Duration {
        match self {
            Source::Weather
            | Source::Github
            | Source::Usage
            | Source::SystemUpdate
            | Source::Tailscale
            | Source::Monitor
            | Source::Calendar
            | Source::Display
            | Source::Processes
            | Source::PowerFlow
            | Source::About => POLL_GRACE,
            _ => Duration::ZERO,
        }
    }

    /// A poll that reads the network, so goes again once a link comes back
    /// ([`devices::network::reconnects`]).
    pub fn online(self) -> bool {
        matches!(self, Source::Weather | Source::Github | Source::Tailscale | Source::SystemUpdate)
    }
}

static COUNTS: [AtomicUsize; COUNT] = [const { AtomicUsize::new(0) }; COUNT];
static PULSES: [AtomicBool; COUNT] = [const { AtomicBool::new(false) }; COUNT];
/// Whether each source is on, as its service sees it.
static ON: [AtomicBool; COUNT] = [const { AtomicBool::new(false) }; COUNT];
static WATCHERS: Mutex<Vec<(Source, Sender<bool>)>> = Mutex::new(Vec::new());
static POKE: OnceLock<Sender<()>> = OnceLock::new();

fn poke() {
    if let Some(tx) = POKE.get() {
        let _ = tx.try_send(());
    }
}

/// Wants `source` for one grace period, for a reader (an IPC call) that
/// has no cell: the first call finds what the last one left running.
pub fn pulse(source: Source) {
    PULSES[source as usize].store(true, Ordering::SeqCst);
    poke();
}

/// Whether `source` is on, sent now and on every change after, for the
/// service that serves it.
pub fn watch(source: Source) -> Receiver<bool> {
    let (tx, rx) = async_channel::unbounded();
    let Ok(mut watchers) = WATCHERS.lock() else { return rx };
    let _ = tx.try_send(ON[source as usize].load(Ordering::SeqCst));
    watchers.push((source, tx));
    rx
}

fn switch(source: Source, on: bool) {
    let Ok(mut watchers) = WATCHERS.lock() else { return };
    ON[source as usize].store(on, Ordering::SeqCst);
    watchers.retain(|(s, tx)| *s != source || tx.try_send(on).is_ok());
}

/// Dropped with whatever took it.
pub struct Want {
    source: Source,
    held: bool,
}

impl Want {
    pub fn new(source: Source) -> Self {
        Self::held(source, true)
    }

    /// A want that starts held or not.
    pub fn held(source: Source, on: bool) -> Self {
        let mut want = Self { source, held: false };
        want.hold(on);
        want
    }

    /// Lets the source go while the holder is off screen, and takes it
    /// again when it comes back.
    pub fn hold(&mut self, on: bool) {
        if on == self.held {
            return;
        }
        self.held = on;
        if on {
            COUNTS[self.source as usize].fetch_add(1, Ordering::SeqCst);
        } else {
            COUNTS[self.source as usize].fetch_sub(1, Ordering::SeqCst);
        }
        poke();
    }
}

impl Drop for Want {
    fn drop(&mut self) {
        self.hold(false);
    }
}

/// The wants of a holder that names its sources again as it changes (an
/// open panel): a source still named keeps its want, one no longer named
/// lets go.
#[derive(Default)]
pub struct Holding(Vec<Want>);

impl Holding {
    pub fn set(&mut self, sources: &[Source]) {
        self.0.retain(|w| sources.contains(&w.source));
        for &source in sources {
            if !self.0.iter().any(|w| w.source == source) {
                self.0.push(Want::new(source));
            }
        }
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = POKE.set(tx);
    let mut running: Vec<Option<Sender<()>>> = Source::ALL.iter().map(|_| None).collect();
    let mut idle_since: Vec<Option<Instant>> = Source::ALL.iter().map(|_| None).collect();
    // Holders built before this task ran already hold their wants.
    loop {
        for &source in Source::ALL {
            let i = source as usize;
            let pulsed = PULSES[i].swap(false, Ordering::SeqCst);
            let on = ON[i].load(Ordering::SeqCst);
            if pulsed || COUNTS[i].load(Ordering::SeqCst) > 0 {
                idle_since[i] = None;
                if !on {
                    if let Some(task) = source.task(ctx.clone()) {
                        let (stop, stopped) = async_channel::bounded::<()>(1);
                        // The stop sender dropping ends the receive and with
                        // it the poll, which kills whatever child it held.
                        ctx.spawn(async move {
                            task.or(async {
                                let _ = stopped.recv().await;
                            })
                            .await
                        });
                        running[i] = Some(stop);
                    }
                    switch(source, true);
                }
            } else if on {
                let grace = source.grace();
                let since = *idle_since[i].get_or_insert_with(Instant::now);
                if since.elapsed() >= grace {
                    running[i] = None;
                    idle_since[i] = None;
                    switch(source, false);
                } else {
                    ctx.spawn(async move {
                        Timer::after(grace).await;
                        poke();
                    });
                }
            }
        }
        if rx.recv().await.is_err() {
            return;
        }
    }
}

