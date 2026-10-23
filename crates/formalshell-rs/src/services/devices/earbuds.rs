//! Every connected pair of earbuds or headphones across
//! the four backends, in fs-devices' one device shape (spec
//! `docs/superpowers/specs/2026-09-30-m77-earbuds.md`). The store slice is
//! what the bar cell and the `earbuds` IPC read; one task per backend owns
//! its source.
//!
//! Nothing runs while nobody holds the service: the earbuds cell holds it
//! for as long as it is on the strip and the earbuds panel while it is open.
//! A file-watch backend (airpods) reads on acquire, a process backend
//! (nothing) keeps one `watch` child per connected device, and the poll
//! backends (soundcore, samsung) run their CLI every 30 seconds and on every
//! change to the connected Bluetooth set, one command at a time.
//!
//! Safety (spec "Safety (Nothing)"): the shell never speaks the Nothing
//! protocol. The only bytes that reach `nothingctl watch`'s stdin are a
//! [`nothing::NothingVerb`], built from the device's own reported state by
//! [`nothing::command`] in [`Earbuds::plan`].

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::{Async, Timer};
use async_process::{ChildStdin, Stdio};
use fs_devices::bluetooth::Device as BtDevice;
use fs_devices::earbuds::{self as model, Device, Value, airpods, nothing, samsung, soundcore};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, StreamExt, future};

use crate::runtime::Ctx;
use crate::services::watch::Watch;
use crate::store;

const POLL: Duration = Duration::from_secs(30);
const BACKOFF_MIN: Duration = Duration::from_secs(5);
const BACKOFF_MAX: Duration = Duration::from_secs(300);
/// A headphone BlueZ has just reported connected is still bringing up its
/// audio profiles: an RFCOMM connect then fails with br-connection-busy and
/// the Fast Pair channel stays silent, for up to ~40s on a CMF Headphone Pro.
/// The first watch waits SETTLE, and failures inside QUICK_WINDOW of the
/// connect retry every QUICK rather than on the doubling backoff.
const SETTLE: Duration = Duration::from_secs(2);
const QUICK: Duration = Duration::from_secs(2);
const QUICK_WINDOW: Duration = Duration::from_secs(90);

/// One backend's published state.
#[derive(Clone, Debug, PartialEq)]
pub struct Backend {
    pub available: bool,
    pub devices: Vec<Device>,
    /// nothing: each listed device's last state line, what its commands are
    /// built from. A device is here only while its `watch` child runs.
    pub nothing: BTreeMap<String, nothing::State>,
    /// soundcore: each connected device's capabilities.
    pub caps: BTreeMap<String, soundcore::Caps>,
}

impl Backend {
    fn new(available: bool) -> Self {
        Self { available, devices: Vec::new(), nothing: BTreeMap::new(), caps: BTreeMap::new() }
    }
}

pub struct Earbuds {
    backends: Vec<(&'static str, Backend)>,
    devices: Vec<Device>,
    /// The user's last pick, overwritten whenever a device newly connects.
    active_key: String,
    was_connected: BTreeMap<String, bool>,
}

impl Default for Earbuds {
    fn default() -> Self {
        // A backend is available until its source is found missing; the
        // airpods daemon is missing until its status file reads.
        let backends = vec![
            (airpods::BACKEND, Backend::new(false)),
            (nothing::BACKEND, Backend::new(true)),
            (soundcore::BACKEND, Backend::new(true)),
            (samsung::BACKEND, Backend::new(true)),
        ];
        Self { backends, devices: Vec::new(), active_key: String::new(), was_connected: BTreeMap::new() }
    }
}

pub enum Diff {
    Backend(&'static str, Backend),
    Select(String),
}

/// A write the UI has already checked against the device and the
/// backend's allow-list.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// One verb on the librepods control socket.
    Airpods(String),
    /// One `nothingctl watch` stdin line, without its newline.
    Nothing { address: String, line: String },
    /// One CLI call on a poll backend, followed by a poll.
    Run { backend: &'static str, argv: Vec<String> },
}

impl Earbuds {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Backend(name, backend) => {
                let Some(slot) = self.backends.iter_mut().find(|(n, _)| *n == name) else { return false };
                if slot.1 == backend {
                    return false;
                }
                slot.1 = backend;
                self.devices = self.backends.iter().flat_map(|(_, b)| b.devices.iter().cloned()).collect();
                let mut now = BTreeMap::new();
                for d in &self.devices {
                    now.insert(d.key.clone(), d.connected);
                    if d.connected && !self.was_connected.get(&d.key).copied().unwrap_or(false) {
                        self.active_key = d.key.clone();
                    }
                }
                self.was_connected = now;
                true
            }
            Diff::Select(key) => {
                let changed = self.active_key != key;
                self.active_key = key;
                changed
            }
        }
    }

    pub fn available(&self) -> bool {
        self.backends.iter().any(|(_, b)| b.available)
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn active(&self) -> Option<&Device> {
        model::pick_active(&self.devices, &self.active_key)
    }

    pub fn has(&self, key: &str) -> bool {
        self.devices.iter().any(|d| d.key == key)
    }

    fn backend(&self, name: &str) -> Option<&Backend> {
        self.backends.iter().find(|(n, _)| *n == name).map(|(_, b)| b)
    }

    /// EarbudsService.set against the active device: the write to make
    /// (`None` when the device already holds the value), or the reason it
    /// was refused, worded as the IPC hands it back.
    pub fn plan(&self, control: &str, raw: &str) -> Result<Option<Op>, String> {
        let dev = self.active().ok_or("no device")?;
        let ctl = model::control(Some(dev), control).ok_or_else(|| format!("no control '{control}' on {}", dev.name))?;
        let value = model::coerce(Some(ctl), &Value::str(raw)).ok_or_else(|| format!("refused '{raw}' for {control}"))?;
        let refused = || format!("refused by {}", dev.backend);
        let backend = self.backend(&dev.backend).ok_or_else(refused)?;
        if !backend.devices.iter().any(|d| d.key == dev.key) {
            return Err(refused());
        }
        match dev.backend.as_str() {
            airpods::BACKEND => {
                let verb = airpods::command(control, &value).ok_or_else(refused)?;
                socket_path().ok_or_else(refused)?;
                Ok(Some(Op::Airpods(verb)))
            }
            nothing::BACKEND => {
                let state = backend.nothing.get(&dev.address).ok_or_else(refused)?;
                let verb = nothing::command(control, &value, Some(state)).ok_or_else(refused)?;
                // A range drag lands on the same whole value many times
                // over; the device already holds it, so nothing is written.
                let held = match (ctl.kind, &ctl.value) {
                    (model::ControlKind::Range, Value::Num(n)) => Value::Num((n + 0.5).floor()),
                    (_, v) => v.clone(),
                };
                if held == value {
                    return Ok(None);
                }
                Ok(Some(Op::Nothing { address: dev.address.clone(), line: verb.to_string() }))
            }
            soundcore::BACKEND => {
                let tail = soundcore::command(control, &value, backend.caps.get(&dev.address)).ok_or_else(refused)?;
                let argv = soundcore::setting_argv(&dev.address, &tail).ok_or_else(refused)?;
                Ok(Some(Op::Run { backend: soundcore::BACKEND, argv }))
            }
            samsung::BACKEND => {
                let tail = samsung::command(control, &value).ok_or_else(refused)?;
                let argv = samsung::argv(&dev.address, &tail).ok_or_else(refused)?;
                Ok(Some(Op::Run { backend: samsung::BACKEND, argv }))
            }
            _ => Err(refused()),
        }
    }
}

/// `$XDG_RUNTIME_DIR/librepods.sock`; none with the variable unset, the
/// daemon's own ipcpath.hpp refusing to guess one. A nested smoke session
/// keeps the host's runtime dir, so `FORMALSHELL_SMOKE_LIBREPODS_SOCKET`
/// points it at the leg's stub instead of a real daemon.
fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("FORMALSHELL_SMOKE_LIBREPODS_SOCKET").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(path));
    }
    std::env::var("XDG_RUNTIME_DIR").ok().filter(|d| !d.is_empty()).map(|d| PathBuf::from(d).join("librepods.sock"))
}

fn status_path() -> PathBuf {
    let dir = match std::env::var("XDG_STATE_HOME") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state"),
    };
    dir.join("librepods").join("status.json")
}

enum Msg {
    Hold(bool),
    Bluetooth(Vec<BtDevice>),
    Op(Op),
}

static CHANNEL: LazyLock<(Sender<Msg>, Receiver<Msg>)> = LazyLock::new(async_channel::unbounded);

fn send(msg: Msg) {
    let _ = CHANNEL.0.try_send(msg);
}

/// A consumer arrived (the cell built, the panel opened). Callable from
/// any thread.
pub fn acquire() {
    send(Msg::Hold(true));
}

pub fn release() {
    send(Msg::Hold(false));
}

static PANEL: AtomicBool = AtomicBool::new(false);

/// The earbuds panel's hold, taken while it is open.
pub fn panel(open: bool) {
    if PANEL.swap(open, Ordering::Relaxed) != open {
        send(Msg::Hold(open));
    }
}

/// BlueZ's devices as the backends read them (the smoke override applied).
pub fn bluetooth(devices: Vec<BtDevice>) {
    send(Msg::Bluetooth(devices));
}

pub fn write(op: Op) {
    send(Msg::Op(op));
}

fn publish(ctx: &Ctx, name: &'static str, backend: Backend) {
    ctx.publish(store::Diff::Devices(super::Diff::Earbuds(Diff::Backend(name, backend))));
}

enum Event {
    Hold(bool),
    Bluetooth(Vec<BtDevice>),
    Op(Op),
}

pub fn start(ctx: &Ctx) {
    let (air_tx, air_rx) = async_channel::unbounded();
    let (not_tx, not_rx) = async_channel::unbounded();
    let (scq_tx, scq_rx) = async_channel::unbounded();
    let (sam_tx, sam_rx) = async_channel::unbounded();
    let epoch = Rc::new(Cell::new(0u64));
    ctx.spawn(run_airpods(ctx.clone(), air_rx));
    ctx.spawn(run_nothing(ctx.clone(), not_rx, not_tx.clone()));
    ctx.spawn(run_poll(ctx.clone(), Poller::Soundcore, scq_rx, epoch.clone()));
    ctx.spawn(run_poll(ctx.clone(), Poller::Samsung, sam_rx, epoch.clone()));
    ctx.spawn(async move {
        let mut holds = 0usize;
        let rx = CHANNEL.1.clone();
        while let Ok(first) = rx.recv().await {
            // A bar rebuild drops its cells and builds new ones in one go:
            // a release and an acquire in the same burst are no change.
            let mut batch = vec![first];
            while let Ok(more) = rx.try_recv() {
                batch.push(more);
            }
            let was = holds > 0;
            for msg in &batch {
                match msg {
                    Msg::Hold(true) => holds += 1,
                    Msg::Hold(false) => holds = holds.saturating_sub(1),
                    _ => {}
                }
            }
            let held = holds > 0;
            if held != was {
                if !held {
                    epoch.set(epoch.get() + 1);
                }
                let _ = air_tx.try_send(Event::Hold(held));
                let _ = not_tx.try_send(NEvent::Hold(held));
                let _ = scq_tx.try_send(Event::Hold(held));
                let _ = sam_tx.try_send(Event::Hold(held));
            }
            for msg in batch {
                match msg {
                Msg::Hold(_) => {}
                Msg::Bluetooth(devices) => {
                    let _ = not_tx.try_send(NEvent::Bluetooth(devices.clone()));
                    let _ = scq_tx.try_send(Event::Bluetooth(devices.clone()));
                    let _ = sam_tx.try_send(Event::Bluetooth(devices));
                }
                Msg::Op(op) => match &op {
                    Op::Airpods(_) => {
                        let _ = air_tx.try_send(Event::Op(op));
                    }
                    Op::Nothing { .. } => {
                        let _ = not_tx.try_send(NEvent::Op(op));
                    }
                    Op::Run { backend, .. } if *backend == soundcore::BACKEND => {
                        let _ = scq_tx.try_send(Event::Op(op));
                    }
                    Op::Run { .. } => {
                        let _ = sam_tx.try_send(Event::Op(op));
                    }
                },
                }
            }
        }
    });
}

/// One short CLI call: its exit code and stdout. -1 when it could not start
/// (no binary on PATH), -2 for an argv an adapter refused to build.
async fn run_cli(argv: Option<Vec<String>>) -> (i32, String) {
    let Some(argv) = argv.filter(|a| !a.is_empty()) else { return (-2, String::new()) };
    let out = crate::services::proc::command(&argv[0]).args(&argv[1..]).stdin(Stdio::null()).stderr(Stdio::null()).output().await;
    match out {
        Ok(out) => (out.status.code().unwrap_or(-3), String::from_utf8_lossy(&out.stdout).into_owned()),
        Err(_) => (-1, String::new()),
    }
}

/// How long the daemon gets to take a verb and hang up. It answers from its
/// own event loop, so a live one is done in milliseconds.
const AIRPODS_ANSWER: Duration = Duration::from_secs(2);

const AIRPODS_STALLED: &str = "librepods not responding";

/// One verb on the control socket, true once the daemon has read it and hung
/// up. The kernel queues a connection the daemon never accepts, so a
/// connect and a write succeeding prove nothing: only its hang-up does.
async fn airpods_send(socket: PathBuf, verb: String) -> bool {
    let talk = async {
        let mut stream = Async::<std::os::unix::net::UnixStream>::connect(&socket).await.ok()?;
        stream.write_all(verb.as_bytes()).await.ok()?;
        stream.flush().await.ok()?;
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).await.ok()?;
        Some(())
    };
    future::or(async { talk.await.is_some() }, async {
        Timer::after(AIRPODS_ANSWER).await;
        false
    })
    .await
}

/// The AirPods backend over the omarchy-pods librepods daemon: its whole
/// state is one status.json it rewrites atomically on change and removes on
/// quit, so a missing file is the daemon being down. A daemon whose event
/// loop is stuck leaves the file in place and never takes a verb; a write
/// it never answers marks the device until one is answered.
async fn run_airpods(ctx: Ctx, rx: Receiver<Event>) {
    let path = status_path();
    let mut watch: Option<Watch> = None;
    let stalled = Rc::new(Cell::new(false));
    let load = |ctx: &Ctx| {
        let (ctx, path, stalled) = (ctx.clone(), path.clone(), stalled.clone());
        async move {
            let text = ctx.pool().run(move || std::fs::read_to_string(&path).unwrap_or_default()).await.unwrap_or_default();
            let status = airpods::parse_status(text.trim());
            let mut backend = Backend::new(status.ok);
            backend.devices = airpods::normalise(&status);
            if stalled.get() {
                for d in &mut backend.devices {
                    d.state_line = AIRPODS_STALLED.into();
                }
            }
            publish(&ctx, airpods::BACKEND, backend);
        }
    };
    loop {
        // None: the status file may read differently.
        let event = match watch.as_mut() {
            Some(w) => {
                future::or(async { Some(rx.recv().await) }, async {
                    w.changed().await;
                    None
                })
                .await
            }
            None => Some(rx.recv().await),
        };
        let event = match event {
            Some(Ok(e)) => Some(e),
            Some(Err(_)) => return,
            None => None,
        };
        match event {
            None => load(&ctx).await,
            Some(Event::Hold(true)) => {
                watch = Watch::new(path.clone()).ok();
                load(&ctx).await;
            }
            Some(Event::Hold(false)) => watch = None,
            Some(Event::Op(Op::Airpods(verb))) => {
                let Some(socket) = socket_path() else { continue };
                let taken = airpods_send(socket, verb.clone()).await;
                if !taken {
                    eprintln!("earbuds: librepods did not take {verb} within {}s", AIRPODS_ANSWER.as_secs());
                }
                if stalled.replace(!taken) != !taken {
                    load(&ctx).await;
                }
            }
            Some(_) => {}
        }
    }
}

#[derive(Clone, Copy)]
enum Poller {
    Soundcore,
    Samsung,
}

impl Poller {
    fn name(self) -> &'static str {
        match self {
            Poller::Soundcore => soundcore::BACKEND,
            Poller::Samsung => samsung::BACKEND,
        }
    }
}

/// The connected addresses, sorted: what a poll is keyed on.
fn connected_key(bt: &[BtDevice]) -> Vec<String> {
    let mut out: Vec<String> = bt.iter().filter(|d| d.connected).map(|d| d.address.clone()).collect();
    out.sort();
    out
}

/// The polling backend, for Soundcore and Samsung.
/// One task, so every CLI call (a poll's or a write's) runs one at a time.
async fn run_poll(ctx: Ctx, kind: Poller, rx: Receiver<Event>, epoch: Rc<Cell<u64>>) {
    let mut held = false;
    let mut bt: Vec<BtDevice> = Vec::new();
    let mut key: Vec<String> = Vec::new();
    let mut next = Instant::now();
    let mut backend = Backend::new(true);
    loop {
        let event = if held {
            match future::or(async { Some(rx.recv().await) }, async {
                Timer::at(next).await;
                None
            })
            .await
            {
                Some(Ok(e)) => Some(e),
                Some(Err(_)) => return,
                None => None,
            }
        } else {
            match rx.recv().await {
                Ok(e) => Some(e),
                Err(_) => return,
            }
        };
        let mut poll = false;
        match event {
            None => {
                next += POLL;
                poll = true;
            }
            Some(Event::Hold(on)) => {
                held = on;
                next = Instant::now() + POLL;
            }
            Some(Event::Bluetooth(devices)) => bt = devices,
            Some(Event::Op(Op::Run { argv, .. })) if held => {
                let (code, _) = run_cli(Some(argv)).await;
                backend.available = code != -1;
                poll = true;
            }
            Some(Event::Op(_)) => {}
        }
        let now_key = if held { connected_key(&bt) } else { Vec::new() };
        if now_key != key {
            key = now_key;
            poll |= held;
        }
        if !poll || !held {
            continue;
        }
        let start = epoch.get();
        let polled = match kind {
            Poller::Soundcore => poll_soundcore(&bt, &mut backend).await,
            Poller::Samsung => poll_samsung(&bt, &mut backend).await,
        };
        // A poll that began before a release is not read.
        if epoch.get() == start {
            backend.devices = polled;
            publish(&ctx, kind.name(), backend.clone());
        }
    }
}

async fn poll_soundcore(bt: &[BtDevice], backend: &mut Backend) -> Vec<Device> {
    if !bt.iter().any(|d| d.connected) {
        backend.caps.clear();
        return Vec::new();
    }
    let (code, text) = run_cli(Some(soundcore::paired_argv())).await;
    backend.available = code != -1;
    let targets = if code == 0 { soundcore::connected_paired(&soundcore::parse_paired(&text), bt) } else { Vec::new() };
    backend.caps.retain(|address, _| targets.iter().any(|t| &t.address == address));
    let mut found = Vec::new();
    for info in targets {
        if !backend.caps.contains_key(&info.address) {
            let (code, text) = run_cli(soundcore::settings_argv(&info.address)).await;
            backend.available = code != -1;
            let Some(caps) = (code == 0).then(|| soundcore::parse_capabilities(&text)).flatten() else { continue };
            backend.caps.insert(info.address.clone(), caps);
        }
        let caps = backend.caps[&info.address].clone();
        let argv = (!caps.ids.is_empty()).then(|| soundcore::setting_argv(&info.address, &soundcore::get_args(&caps))).flatten();
        let (code, text) = run_cli(argv).await;
        if code != -2 {
            backend.available = code != -1;
        }
        let values = if code == 0 { soundcore::parse_values(&text) } else { Default::default() };
        found.push(soundcore::normalise(&info, &caps, &values));
    }
    found
}

async fn poll_samsung(bt: &[BtDevice], backend: &mut Backend) -> Vec<Device> {
    let mut found = Vec::new();
    for buds in samsung::connected_buds(bt) {
        let (code, text) = run_cli(samsung::status_argv(&buds.address)).await;
        if code != -2 {
            backend.available = code != -1;
        }
        if code == 0 {
            found.extend(samsung::normalise(&samsung::parse_status(&text), &buds.name));
        }
    }
    found
}

enum NEvent {
    Hold(bool),
    Bluetooth(Vec<BtDevice>),
    Op(Op),
    List(i32, String),
    Line(String, u64, String),
    Exit(String, u64, i32),
}

struct Watcher {
    id: u64,
    stdin: ChildStdin,
    stop: Sender<()>,
}

#[derive(Default)]
struct Nothing {
    held: bool,
    available: bool,
    listing: bool,
    list_again: bool,
    next_id: u64,
    watchers: BTreeMap<String, Watcher>,
    states: BTreeMap<String, nothing::State>,
    failures: BTreeMap<String, &'static str>,
    /// address -> (when, the delay that got it there).
    retry: BTreeMap<String, (Instant, Duration)>,
    /// Kept across release, so closing and reopening the panel is not a way
    /// round a refusal.
    unsupported: BTreeMap<String, nothing::Mark>,
    connected: Vec<String>,
    connected_at: BTreeMap<String, Instant>,
}

/// The Nothing backend over `nothingctl`. Discovery is `list --json`, run on
/// acquire and whenever the connected Bluetooth set changes; each device it
/// reports connected gets one `watch -d <addr>` child while held. A device
/// nothingctl refuses over its model (exit 3) is left alone until BlueZ sees
/// it disconnect and connect again; any other exit is retried every 2 s for
/// the first 90 s after a connect, then on a backoff from 5 s doubling to 5
/// minutes.
async fn run_nothing(ctx: Ctx, rx: Receiver<NEvent>, tx: Sender<NEvent>) {
    let mut s = Nothing { available: true, ..Nothing::default() };
    loop {
        let now = Instant::now();
        let due = s.retry.values().map(|(at, _)| *at).filter(|at| *at > now).min();
        let event = match due {
            Some(at) => {
                future::or(async { Some(rx.recv().await) }, async {
                    Timer::at(at).await;
                    None
                })
                .await
            }
            None => Some(rx.recv().await),
        };
        let event = match event {
            Some(Ok(e)) => e,
            Some(Err(_)) => return,
            None => {
                s.discover(&ctx, &tx);
                continue;
            }
        };
        match event {
            NEvent::Hold(true) => {
                s.held = true;
                s.discover(&ctx, &tx);
            }
            NEvent::Hold(false) => {
                s.held = false;
                s.list_again = false;
                for (_, w) in std::mem::take(&mut s.watchers) {
                    let _ = w.stop.try_send(());
                }
                s.states.clear();
                s.failures.clear();
                s.retry.clear();
                s.publish(&ctx);
            }
            NEvent::Bluetooth(devices) => {
                let connected = connected_key(&devices);
                if connected != s.connected {
                    s.unsupported = nothing::rearm(&s.unsupported, &connected);
                    let now = Instant::now();
                    s.connected_at.retain(|a, _| connected.contains(a));
                    for a in connected.iter().filter(|a| !s.connected.contains(a)) {
                        s.connected_at.insert(a.clone(), now);
                        s.retry.insert(a.clone(), (now + SETTLE, SETTLE));
                    }
                    s.connected = connected;
                    s.discover(&ctx, &tx);
                }
            }
            NEvent::Op(Op::Nothing { address, line }) => {
                if let Some(w) = s.watchers.get_mut(&address) {
                    let _ = w.stdin.write_all(format!("{line}\n").as_bytes()).await;
                    let _ = w.stdin.flush().await;
                }
            }
            NEvent::Op(_) => {}
            NEvent::List(code, text) => s.on_list(&ctx, &tx, code, &text),
            NEvent::Line(address, id, line) => s.on_line(&ctx, &address, id, &line),
            NEvent::Exit(address, id, code) => s.on_exit(&ctx, &address, id, code),
        }
    }
}

impl Nothing {
    fn publish(&self, ctx: &Ctx) {
        let mut backend = Backend::new(self.available);
        backend.devices = self.states.iter().map(|(a, st)| nothing::normalise(st, self.failures.get(a).copied().unwrap_or(""))).collect();
        backend.nothing = self.states.clone();
        publish(ctx, nothing::BACKEND, backend);
    }

    fn discover(&mut self, ctx: &Ctx, tx: &Sender<NEvent>) {
        if !self.held {
            return;
        }
        if self.listing {
            self.list_again = true;
            return;
        }
        self.listing = true;
        let tx = tx.clone();
        ctx.spawn(async move {
            let (code, text) = run_cli(Some(nothing::list_argv())).await;
            let _ = tx.send(NEvent::List(code, text)).await;
        });
    }

    fn on_list(&mut self, ctx: &Ctx, tx: &Sender<NEvent>, code: i32, text: &str) {
        self.listing = false;
        let available = code != -1;
        if available != self.available {
            self.available = available;
            self.publish(ctx);
        }
        if !self.held {
            return;
        }
        let now = Instant::now();
        if code == 0 {
            for d in nothing::parse_list(text) {
                let waiting = self.retry.get(&d.address).is_some_and(|(at, _)| *at > now);
                if !d.connected || self.watchers.contains_key(&d.address) || self.unsupported.contains_key(&d.address) || waiting {
                    continue;
                }
                self.start_watch(ctx, tx, &d.address);
            }
        }
        if self.list_again {
            self.list_again = false;
            self.discover(ctx, tx);
        }
    }

    fn start_watch(&mut self, ctx: &Ctx, tx: &Sender<NEvent>, address: &str) {
        let Some(argv) = nothing::watch_argv(address) else { return };
        self.next_id += 1;
        let id = self.next_id;
        let child = crate::services::proc::command(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(_) => {
                let _ = tx.try_send(NEvent::Exit(address.to_owned(), id, -1));
                return;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
        let (stop, stopped) = async_channel::bounded::<()>(1);
        self.watchers.insert(address.to_owned(), Watcher { id, stdin, stop });
        let (tx, address) = (tx.clone(), address.to_owned());
        ctx.spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                let next = future::or(async { lines.next().await.and_then(Result::ok) }, async {
                    let _ = stopped.recv().await;
                    None
                })
                .await;
                match next {
                    Some(line) => {
                        let _ = tx.send(NEvent::Line(address.clone(), id, line)).await;
                    }
                    None => break,
                }
            }
            let _ = child.kill();
            let code = child.status().await.ok().and_then(|s| s.code()).unwrap_or(-3);
            let _ = tx.send(NEvent::Exit(address, id, code)).await;
        });
    }

    fn live(&self, address: &str, id: u64) -> bool {
        self.watchers.get(address).is_some_and(|w| w.id == id)
    }

    fn on_line(&mut self, ctx: &Ctx, address: &str, id: u64, line: &str) {
        if !self.held || !self.live(address, id) {
            return;
        }
        match nothing::parse_line(line) {
            Some(nothing::Line::State(state)) if state.address == address => {
                self.states.insert(address.to_owned(), *state);
                self.failures.remove(address);
                self.retry.remove(address);
            }
            Some(nothing::Line::Ack { .. }) => {
                self.failures.remove(address);
            }
            Some(nothing::Line::Error { code, message, .. }) if code == nothing::UNSUPPORTED => {
                return self.mark_unsupported(address, &message);
            }
            Some(nothing::Line::Error { message, .. }) => {
                eprintln!("nothingctl {address}: {message}");
                self.failures.insert(address.to_owned(), nothing::FAILED);
            }
            Some(nothing::Line::Disconnected) => {
                self.states.remove(address);
                self.failures.remove(address);
            }
            _ => return,
        }
        self.publish(ctx);
    }

    fn mark_unsupported(&mut self, address: &str, message: &str) {
        if self.unsupported.contains_key(address) {
            return;
        }
        let why = if message.is_empty() { "unsupported model" } else { message };
        eprintln!("nothingctl {address}: {why}; not retried until it reconnects");
        self.unsupported.insert(address.to_owned(), nothing::Mark { seen_down: false });
    }

    fn on_exit(&mut self, ctx: &Ctx, address: &str, id: u64, code: i32) {
        if self.live(address, id) {
            self.watchers.remove(address);
        }
        if code == nothing::UNSUPPORTED_EXIT {
            self.mark_unsupported(address, "");
        }
        if !self.held {
            return;
        }
        self.states.remove(address);
        self.failures.remove(address);
        self.publish(ctx);
        if self.unsupported.contains_key(address) {
            return;
        }
        let fresh = self.connected_at.get(address).is_some_and(|at| at.elapsed() < QUICK_WINDOW);
        let delay = match self.retry.get(address) {
            _ if fresh => QUICK,
            Some((_, d)) => (*d * 2).clamp(BACKOFF_MIN, BACKOFF_MAX),
            None => BACKOFF_MIN,
        };
        self.retry.insert(address.to_owned(), (Instant::now() + delay, delay));
    }
}

/// A picture set's part (`duo`, `case`, `left`, `right`) in a `size`
/// square, from `FS_EARBUDS_ART` (nix/earbuds-art.py); none without the
/// directory, a set, or that part.
pub fn picture(set: &str, part: &str, size: f64) -> Option<crate::scene::Bitmap> {
    if set.is_empty() {
        return None;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("FS_EARBUDS_ART")?);
    crate::services::icons::load_at(&dir.join(format!("{set}_{part}.webp")), size.round().max(1.0) as u32, crate::scene::raster_scale())
}

