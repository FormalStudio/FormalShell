//! T3 Code agent state for the Spaces badges. One `t3-agents` stream per host
//! in `workspaces.agentHosts`, kept open over ssh only while a T3 Code window
//! exists and a Spaces chip can be seen. Every T3 Code window carries the
//! worst state across all hosts. An unreachable host or a reply of the wrong
//! shape carries no state: nothing here makes a badge up.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::time::Duration;

use async_io::Timer;
use fs_info::agents::{self, Badge};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, FutureExt, StreamExt};

use crate::runtime::Ctx;
use crate::store;

const BASE_BACKOFF: Duration = Duration::from_secs(2);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Watch {
    /// Ids of the T3 Code windows the Spaces cell shows.
    pub windows: Vec<String>,
    pub hosts: Vec<String>,
}

/// Window id and host to "working", "blocked" or "done"; a window with
/// nothing to report is absent, a host whose last reply failed reads "".
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub by_window: BTreeMap<String, String>,
    pub by_host: BTreeMap<String, String>,
}

pub struct Diff(pub State);

impl State {
    pub fn apply(&mut self, Diff(next): Diff) -> bool {
        if *self == next {
            return false;
        }
        *self = next;
        true
    }
}

static WATCH: OnceLock<async_channel::Sender<Watch>> = OnceLock::new();
static SHOWN: OnceLock<async_channel::Sender<bool>> = OnceLock::new();

/// Whether a Spaces chip is on screen at all: false under a fullscreen
/// window, the screensaver or the lock. Nothing polls while no badge can be
/// seen.
pub fn shown(on: bool) {
    if let Some(tx) = SHOWN.get() {
        let _ = tx.try_send(on);
    }
}

/// The T3 Code windows and hosts to watch; called whenever they change.
pub fn watch(w: Watch) {
    if let Some(tx) = WATCH.get() {
        let _ = tx.try_send(w);
    }
}

enum Event {
    Watch(Watch),
    Line(String, String),
    Shown(bool),
}

struct Poller {
    /// Dropping it closes the receiver the poll task races, which ends it.
    _stop: async_channel::Sender<()>,
}

#[derive(Default)]
struct Service {
    watch: Watch,
    state: State,
    pollers: HashMap<String, Poller>,
    hidden: bool,
}

impl Service {
    fn publish(&mut self, ctx: &Ctx) {
        let worst = self.state.by_host.values().map(|s| Badge::parse(s)).max().unwrap_or(Badge::None);
        let by_window = if worst == Badge::None {
            BTreeMap::new()
        } else {
            self.watch.windows.iter().map(|id| (id.clone(), worst.as_str().to_owned())).collect()
        };
        self.state.by_window = by_window;
        ctx.publish(store::Diff::Agents(Diff(self.state.clone())));
    }

    /// One poller per host while there is a window to badge; a host no
    /// longer wanted loses its poller and its state.
    fn reconcile(&mut self, ctx: &Ctx, tx: &async_channel::Sender<Event>) {
        let wanted: Vec<String> =
            if self.hidden || self.watch.windows.is_empty() { Vec::new() } else { self.watch.hosts.clone() };
        self.pollers.retain(|host, _| wanted.contains(host));
        self.state.by_host.retain(|host, _| self.watch.hosts.contains(host));
        for host in wanted {
            if self.pollers.contains_key(&host) {
                continue;
            }
            let (stop, stopped) = async_channel::bounded::<()>(1);
            ctx.spawn(poll(host.clone(), agents::poll_command(&host), tx.clone(), stopped));
            self.pollers.insert(host, Poller { _stop: stop });
        }
    }

    fn line(&mut self, ctx: &Ctx, host: &str, line: &str) {
        if !self.pollers.contains_key(host) {
            return;
        }
        let state = agents::parse_line(Some(line)).map_or("", Badge::as_str);
        if self.state.by_host.get(host).map(String::as_str) == Some(state) {
            return;
        }
        self.state.by_host.insert(host.to_owned(), state.to_owned());
        self.publish(ctx);
    }
}

/// One host's stream, restarted on its backoff when the child exits: it only
/// ends by itself when the link dies or the host has no `t3-agents`.
async fn poll(host: String, argv: Vec<String>, tx: async_channel::Sender<Event>, stopped: async_channel::Receiver<()>) {
    let run = async {
        let mut backoff = BASE_BACKOFF;
        loop {
            let child = crate::services::proc::command(&argv[0])
                .args(&argv[1..])
                .stdin(async_process::Stdio::piped())
                .stdout(async_process::Stdio::piped())
                .stderr(async_process::Stdio::null())
                .kill_on_drop(true)
                .spawn();
            if let Ok(mut child) = child {
                let _stdin = child.stdin.take();
                if let Some(out) = child.stdout.take() {
                    let mut lines = BufReader::new(out).lines();
                    while let Some(Ok(line)) = lines.next().await {
                        backoff = BASE_BACKOFF;
                        if tx.send(Event::Line(host.clone(), line)).await.is_err() {
                            return;
                        }
                    }
                }
                let _ = child.status().await;
            }
            // A dead stream clears the host's state rather than leaving its
            // last badge up.
            if tx.send(Event::Line(host.clone(), String::new())).await.is_err() {
                return;
            }
            backoff = (backoff * 2).min(MAX_BACKOFF);
            Timer::after(backoff).await;
        }
    };
    let stop = async {
        let _ = stopped.recv().await;
    };
    run.or(stop).await;
}

pub async fn run(ctx: Ctx) {
    let (wtx, wrx) = async_channel::unbounded();
    let _ = WATCH.set(wtx);
    let (tx, rx) = async_channel::unbounded::<Event>();
    // Watch lists, visibility and poll lines meet in one queue, so one task
    // owns the state.
    let forward = tx.clone();
    ctx.spawn(async move {
        while let Ok(w) = wrx.recv().await {
            if forward.send(Event::Watch(w)).await.is_err() {
                return;
            }
        }
    });
    let (stx, srx) = async_channel::unbounded();
    let _ = SHOWN.set(stx);
    let forward = tx.clone();
    ctx.spawn(async move {
        while let Ok(on) = srx.recv().await {
            if forward.send(Event::Shown(on)).await.is_err() {
                return;
            }
        }
    });

    let mut service = Service::default();
    while let Ok(event) = rx.recv().await {
        match event {
            Event::Watch(w) => {
                if w == service.watch {
                    continue;
                }
                service.watch = w;
                service.reconcile(&ctx, &tx);
                service.publish(&ctx);
            }
            Event::Line(host, line) => service.line(&ctx, &host, &line),
            // Hidden, the pollers stop and the badges keep their last state;
            // shown again, they restart and their first line refreshes.
            Event::Shown(on) if on == !service.hidden => {}
            Event::Shown(on) => {
                service.hidden = !on;
                service.reconcile(&ctx, &tx);
            }
        }
    }
}
