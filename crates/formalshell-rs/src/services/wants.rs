//! Polls that run only while a cell wants them. A cell holds a [`Want`]
//! for as long as it is on a bar that is on screen; the first one starts
//! the source's task on the service thread and the last one's release stops
//! it a moment later, so a config reload that rebuilds the strip does not
//! restart every poll.
//! A bar naming none of these cells costs nothing.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_io::Timer;
use futures_lite::FutureExt;

use super::info;
use crate::runtime::Ctx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Weather,
    Github,
    Usage,
    SystemUpdate,
    Tailscale,
    Monitor,
    Iphone,
    Calendar,
    Display,
    Processes,
    PowerFlow,
    About,
}

pub const COUNT: usize = 12;

const SOURCES: [Source; COUNT] = [
    Source::Weather,
    Source::Github,
    Source::Usage,
    Source::SystemUpdate,
    Source::Tailscale,
    Source::Monitor,
    Source::Iphone,
    Source::Calendar,
    Source::Display,
    Source::Processes,
    Source::PowerFlow,
    Source::About,
];

/// How long a source with no wanter keeps running.
const GRACE: Duration = Duration::from_secs(2);

static COUNTS: [AtomicUsize; COUNT] = [const { AtomicUsize::new(0) }; COUNT];
static PULSES: [AtomicBool; COUNT] = [const { AtomicBool::new(false) }; COUNT];
static POKE: OnceLock<async_channel::Sender<()>> = OnceLock::new();

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

/// Dropped with the cell that took it.
pub struct Want {
    source: Source,
    held: bool,
}

impl Want {
    pub fn new(source: Source) -> Self {
        COUNTS[source as usize].fetch_add(1, Ordering::SeqCst);
        poke();
        Self { source, held: true }
    }

    /// Lets the source go while the cell's bar is off screen, and takes it
    /// again when the bar comes back.
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
        if self.held {
            COUNTS[self.source as usize].fetch_sub(1, Ordering::SeqCst);
            poke();
        }
    }
}

/// The phone's notifications reach the centre with no cell or panel open,
/// so the bridge child lives as long as the shell.
fn always(source: Source) -> bool {
    source == Source::Iphone
}

fn start(source: Source, ctx: Ctx) -> Pin<Box<dyn Future<Output = ()>>> {
    match source {
        Source::Weather => Box::pin(info::weather::run(ctx)),
        Source::Github => Box::pin(info::github::run(ctx)),
        Source::Usage => Box::pin(info::usage::run(ctx)),
        Source::SystemUpdate => Box::pin(info::update::run(ctx)),
        Source::Tailscale => Box::pin(info::tailscale::run(ctx)),
        Source::Monitor => Box::pin(info::monitor::run(ctx)),
        Source::Iphone => Box::pin(info::iphone::run(ctx)),
        Source::Calendar => Box::pin(info::calendar::run(ctx)),
        Source::Display => Box::pin(super::display::run(ctx)),
        Source::Processes => Box::pin(info::procs::run(ctx)),
        Source::PowerFlow => Box::pin(info::powerflow::run(ctx)),
        Source::About => Box::pin(info::about::run(ctx)),
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = POKE.set(tx);
    let mut running: Vec<Option<async_channel::Sender<()>>> = SOURCES.iter().map(|_| None).collect();
    let mut idle_since: Vec<Option<Instant>> = SOURCES.iter().map(|_| None).collect();
    // Cells built before this task ran already hold their wants.
    loop {
        for source in SOURCES {
            let i = source as usize;
            let pulsed = PULSES[i].swap(false, Ordering::SeqCst);
            if pulsed || always(source) || COUNTS[i].load(Ordering::SeqCst) > 0 {
                idle_since[i] = None;
                if running[i].is_none() {
                    let (stop, stopped) = async_channel::bounded::<()>(1);
                    let task = start(source, ctx.clone());
                    // The stop sender dropping ends the receive and with it
                    // the poll, which kills whatever child it held.
                    ctx.spawn(async move {
                        task.or(async {
                            let _ = stopped.recv().await;
                        })
                        .await
                    });
                    running[i] = Some(stop);
                }
            } else if running[i].is_some() {
                let since = *idle_since[i].get_or_insert_with(Instant::now);
                if since.elapsed() >= GRACE {
                    running[i] = None;
                    idle_since[i] = None;
                } else {
                    ctx.spawn(async {
                        Timer::after(GRACE).await;
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
