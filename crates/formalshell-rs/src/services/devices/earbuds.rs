//! Every connected pair of earbuds or headphones across
//! the four backends, in fs-devices' one device shape (spec
//! `docs/superpowers/specs/2026-09-30-m77-earbuds.md`). The store slice is
//! what the bar cell and the `earbuds` IPC read; one task per backend owns
//! its source.
//!
//! Nothing runs while nobody wants it (`Source::Earbuds`): the earbuds cell
//! holds it for as long as it is on the strip, the headset card while it is
//! up and the earbuds panel while it is open, and the panel alone wants the
//! AirPods' sensor streams (`Source::EarbudsSensors`). A file-watch backend
//! (airpods) reads on the first want, a process backend
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
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use async_io::{Async, Timer};
use async_process::{ChildStdin, Stdio};
use fs_dbus::Backoff;
use fs_devices::bluetooth::Device as BtDevice;
use fs_types::BtAddress;
use fs_devices::earbuds::motion::{Quat, Sample, parse_sample};
use fs_devices::earbuds::{self as model, Device, HeartSample, Motion, Value, airpods, nothing, samsung, soundcore};
use futures_lite::io::{BufReader, Lines};
use futures_lite::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, FutureExt, StreamExt, future};

use crate::runtime::Ctx;
use crate::services::wants::{self, Source};
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
    pub nothing: BTreeMap<BtAddress, nothing::State>,
    /// soundcore: each connected device's capabilities.
    pub caps: BTreeMap<BtAddress, soundcore::Caps>,
}

impl Backend {
    fn new(available: bool) -> Self {
        Self { available, devices: Vec::new(), nothing: BTreeMap::new(), caps: BTreeMap::new() }
    }
}

/// What state.json's `earbudsFaces` keeps per address: the picture set and
/// the name the backend last gave the device.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Face {
    #[serde(deserialize_with = "fs_js::lenient::field", skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
    #[serde(deserialize_with = "fs_js::lenient::field", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Keys this shell does not write, kept as they are.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
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
    Nothing { address: BtAddress, line: String },
    /// One CLI call on a poll backend, followed by a poll.
    Run { backend: &'static str, argv: Vec<String> },
}

impl Earbuds {
    /// The IPC `status` reply: the active device as the panel renders it.
    pub fn status(&self) -> serde_json::Value {
        if !self.available() {
            return serde_json::json!({"available": false});
        }
        let device = self.active().map_or(serde_json::Value::Null, model::to_json);
        serde_json::json!({"available": true, "device": device})
    }

    /// The IPC `devices` reply.
    pub fn list(&self) -> serde_json::Value {
        let active = self.active().map(|d| d.key.clone());
        serde_json::Value::Array(
            self.devices()
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "key": d.key,
                        "backend": d.backend,
                        "name": d.name,
                        "connected": d.connected,
                        "active": active.as_deref() == Some(d.key.as_str()),
                    })
                })
                .collect(),
        )
    }

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

    /// The write to make against the active device
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
                let address = dev.address.ok_or_else(refused)?;
                let state = backend.nothing.get(&address).ok_or_else(refused)?;
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
                Ok(Some(Op::Nothing { address, line: verb.to_string() }))
            }
            soundcore::BACKEND => {
                let address = dev.address.ok_or_else(refused)?;
                let tail = soundcore::command(control, &value, backend.caps.get(&address)).ok_or_else(refused)?;
                let argv = soundcore::setting_argv(address, &tail);
                Ok(Some(Op::Run { backend: soundcore::BACKEND, argv }))
            }
            samsung::BACKEND => {
                let tail = samsung::command(control, &value).ok_or_else(refused)?;
                let argv = samsung::argv(dev.address.ok_or_else(refused)?, &tail);
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
    crate::paths::session_runtime().map(|d| d.join("librepods.sock"))
}

fn status_path() -> PathBuf {
    crate::paths::state().join("librepods").join("status.json")
}

/// What the panel, IPC and the other device services ask of the backends.
pub enum Cmd {
    /// BlueZ's devices as the backends read them.
    Bluetooth(Vec<BtDevice>),
    Op(Op),
    /// Something started or stopped recording off a source
    /// ([`fs_audio::Graph::capturing`]).
    Mic(bool),
    /// Takes the AirPods' current head orientation as the zero of yaw, pitch
    /// and roll.
    Recenter,
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn send(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

fn publish(ctx: &Ctx, name: &'static str, backend: Backend) {
    ctx.publish(store::Diff::Earbuds(Diff::Backend(name, backend)));
}

enum Event {
    Hold(bool),
    Panel(bool),
    Bluetooth(Vec<BtDevice>),
    Op(Op),
    Mic(bool),
    Recenter,
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
        enum Wake {
            Cmd(Cmd),
            Watched(bool),
            Sensors(bool),
        }
        let rx = CHANNEL.1.clone();
        let watched = wants::watch(Source::Earbuds);
        let sensors = wants::watch(Source::EarbudsSensors);
        let (mut held, mut panel) = (false, false);
        loop {
            let wake = async { rx.recv().await.ok().map(Wake::Cmd) }
                .or(async { watched.recv().await.ok().map(Wake::Watched) })
                .or(async { sensors.recv().await.ok().map(Wake::Sensors) })
                .await;
            let Some(wake) = wake else { return };
            match wake {
                Wake::Watched(on) if on != held => {
                    held = on;
                    if !held {
                        epoch.set(epoch.get() + 1);
                    }
                    let _ = air_tx.try_send(Event::Hold(held));
                    let _ = not_tx.try_send(NEvent::Hold(held));
                    let _ = scq_tx.try_send(Event::Hold(held));
                    let _ = sam_tx.try_send(Event::Hold(held));
                }
                Wake::Sensors(on) if on != panel => {
                    panel = on;
                    let _ = air_tx.try_send(Event::Panel(on));
                }
                Wake::Watched(_) | Wake::Sensors(_) => {}
                Wake::Cmd(Cmd::Mic(on)) => {
                    let _ = air_tx.try_send(Event::Mic(on));
                }
                Wake::Cmd(Cmd::Recenter) => {
                    let _ = air_tx.try_send(Event::Recenter);
                }
                Wake::Cmd(Cmd::Bluetooth(devices)) => {
                    let _ = not_tx.try_send(NEvent::Bluetooth(devices.clone()));
                    let _ = scq_tx.try_send(Event::Bluetooth(devices.clone()));
                    let _ = sam_tx.try_send(Event::Bluetooth(devices));
                }
                Wake::Cmd(Cmd::Op(op)) => match &op {
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

/// How often a spot check runs, how long it takes settled samples past the
/// first, and when it gives up with none. `FORMALSHELL_SMOKE_HEART_CHECK`
/// (`<interval>,<window>` in seconds, read at start) shortens the first two
/// for the rig.
const CHECK_EVERY: Duration = Duration::from_secs(10 * 60);
const CHECK_WINDOW: Duration = Duration::from_secs(30);
const CHECK_GIVE_UP: Duration = Duration::from_secs(20);

fn check_times() -> (Duration, Duration) {
    let raw = std::env::var("FORMALSHELL_SMOKE_HEART_CHECK").unwrap_or_default();
    let mut parts = raw.split(',').map(|p| p.trim().parse::<f64>().ok().filter(|n| *n > 0.0).map(Duration::from_secs_f64));
    match (parts.next().flatten(), parts.next().flatten()) {
        (Some(every), Some(window)) => (every, window),
        _ => (CHECK_EVERY, CHECK_WINDOW),
    }
}

/// The AirPods backend over the omarchy-pods librepods daemon: its whole
/// state is one status.json it rewrites atomically on change and removes on
/// quit, so a missing file is the daemon being down. A daemon whose event
/// loop is stuck leaves the file in place and never takes a verb; a write
/// it never answers marks the device until one is answered.
///
/// The heart rate stream runs while the panel is open: `heartrate:on` once
/// the daemon reports the service available (never before, see
/// [`airpods::heart_command`]), `heartrate:off` on close. Every
/// [`CHECK_EVERY`] with the panel shut, one read of the file decides on a
/// spot check, which streams for its window and leaves one point; between
/// checks it adds no watch and no timer of its own.
///
/// While something records, conversation awareness is off, so the buds do
/// not duck playback each time the user speaks into a call, and it goes back
/// on once nothing records. The file stays watched for the whole recording,
/// so buds connecting part way through are turned off too. A `ca:` verb
/// from the user during it is theirs: nothing is changed or put back after.
async fn run_airpods(ctx: Ctx, rx: Receiver<Event>) {
    let path = status_path();
    let (every, window) = check_times();
    // The file watch settles for 50ms after an event, which a loop woken by
    // a motion sample every 40ms would restart before it ever finished, so
    // it runs in a task of its own and the loop hears of it on a channel.
    let (file_tx, file_rx) = async_channel::unbounded::<()>();
    let mut watch: Option<Sender<()>> = None;
    let mut held = false;
    let mut air = Airpods::default();
    let mut next_check = Instant::now() + every;
    loop {
        let due = air.check.as_ref().map_or(next_check, |c| c.deadline(window).min(next_check));
        let wake = future::or(
            future::or(
                async {
                    match rx.recv().await {
                        Ok(e) => Wake::Event(e),
                        Err(_) => Wake::Closed,
                    }
                },
                async {
                    match air.lines.as_mut() {
                        Some(lines) => Wake::Motion(lines.next().await.and_then(Result::ok)),
                        None => future::pending::<Wake>().await,
                    }
                },
            ),
            future::or(
                async {
                    let _ = file_rx.recv().await;
                    Wake::File
                },
                async {
                    Timer::at(due).await;
                    Wake::Timer
                },
            ),
        )
        .await;
        match wake {
            Wake::Closed => return,
            Wake::Timer => {
                let now = Instant::now();
                if air.check.as_ref().is_some_and(|c| c.deadline(window) <= now) {
                    air.end_check(&ctx, &path).await;
                }
                if next_check <= now {
                    next_check = now + every;
                    air.tick(&ctx, &path).await;
                }
            }
            Wake::File => air.load(&ctx, &path).await,
            Wake::Motion(Some(line)) => air.motion_line(&ctx, &line),
            Wake::Motion(None) => air.motion_ended(&ctx),
            Wake::Event(Event::Hold(on)) => held = on,
            Wake::Event(Event::Panel(open)) => {
                air.panel = open;
                if open {
                    // A check under way carries on as the live stream.
                    if let Some(c) = air.check.take() {
                        air.keep(&c);
                    }
                    air.load(&ctx, &path).await;
                } else {
                    next_check = Instant::now() + every;
                    air.history.clear();
                    let had_motion = air.stop_motion();
                    let stopped = std::mem::take(&mut air.heart_on) && air.send(airpods::HEART_OFF.into()).await;
                    if stopped || had_motion {
                        air.load(&ctx, &path).await;
                    }
                }
            }
            Wake::Event(Event::Mic(on)) => {
                air.mic = on;
                if on {
                    air.awareness(&ctx, &path).await;
                } else {
                    let restore = std::mem::take(&mut air.ca_restore);
                    air.ca_user = false;
                    if restore {
                        let status = air.read(&ctx, &path).await;
                        if status.ok && status.connected && !status.conversational_awareness && air.send(airpods::CA_ON.into()).await {
                            air.load(&ctx, &path).await;
                        }
                    }
                }
            }
            Wake::Event(Event::Op(Op::Airpods(verb))) => {
                if air.mic && airpods::is_ca(&verb) {
                    air.ca_user = true;
                    air.ca_restore = false;
                }
                if air.send(verb).await {
                    air.load(&ctx, &path).await;
                }
            }
            Wake::Event(Event::Recenter) => air.recenter(&ctx),
            Wake::Event(_) => {}
        }
        let want = held || air.check.is_some() || air.mic;
        if want != watch.is_some() {
            watch = if want { Watch::new(path.clone()).ok().map(|w| watch_file(&ctx, w, file_tx.clone())) } else { None };
            if want {
                air.load(&ctx, &path).await;
            }
        }
    }
}

/// Reports each change of the status file on `changes` until the returned
/// sender is dropped.
fn watch_file(ctx: &Ctx, mut watch: Watch, changes: Sender<()>) -> Sender<()> {
    let (stop_tx, stop_rx) = async_channel::bounded::<()>(1);
    ctx.spawn(async move {
        loop {
            let stopped = future::or(
                async {
                    watch.changed().await;
                    false
                },
                async {
                    let _ = stop_rx.recv().await;
                    true
                },
            )
            .await;
            if stopped || changes.send(()).await.is_err() {
                return;
            }
        }
    });
    stop_tx
}

enum Wake {
    Event(Event),
    Closed,
    /// The status file may read differently.
    File,
    Timer,
    /// A line off the motion watch, None once it has closed.
    Motion(Option<String>),
}

/// One background spot check under way.
struct Check {
    started: Instant,
    first_settled: Option<Instant>,
    samples: Vec<HeartSample>,
}

impl Check {
    fn deadline(&self, window: Duration) -> Instant {
        match self.first_settled {
            Some(at) => at + window,
            None => self.started + CHECK_GIVE_UP,
        }
    }
}

type MotionLines = Lines<BufReader<Async<std::os::unix::net::UnixStream>>>;

/// What the open `motion:watch` has delivered.
#[derive(Default)]
struct MotionState {
    latest: Option<Sample>,
    /// The first sample of the watch or of a bud taking the stream over, or
    /// where `recenter` last took it.
    reference: Option<Quat>,
    /// The daemon answered the watch with an `error:` line, or it could not
    /// be opened; not tried again until the panel is reopened.
    failed: bool,
}

#[derive(Default)]
struct Airpods {
    stalled: bool,
    panel: bool,
    /// `heartrate:on` went out and no `off` since. A daemon restart (the
    /// file gone) forgets it.
    heart_on: bool,
    /// Settled samples, kept across reloads; each status line carries only
    /// the last one.
    history: Vec<HeartSample>,
    check: Option<Check>,
    /// The spot checks' points, kept for the session.
    points: Vec<HeartSample>,
    /// The last reason a check was skipped with the buds connected, logged
    /// once per change.
    skipped: Option<&'static str>,
    /// Something records off a source.
    mic: bool,
    /// Conversation awareness was turned off for the recording, to go back
    /// on when it ends.
    ca_restore: bool,
    /// The user set conversation awareness during the recording.
    ca_user: bool,
    /// One `motion:watch` connection, held while the panel is open on
    /// connected AirPods whose status carries a `motion` key.
    lines: Option<MotionLines>,
    motion: MotionState,
    /// The status the device was last published from, what a motion sample
    /// is folded into.
    last: airpods::Status,
}

impl Airpods {
    async fn read(&self, ctx: &Ctx, path: &std::path::Path) -> airpods::Status {
        let path = path.to_path_buf();
        let text = ctx.pool().run(move || std::fs::read_to_string(&path).unwrap_or_default()).await.unwrap_or_default();
        airpods::parse_status(text.trim())
    }

    /// Conversation awareness off for a recording, once per recording
    /// unless the buds turn it back on themselves (a reconnect).
    async fn awareness(&mut self, ctx: &Ctx, path: &std::path::Path) {
        let status = self.read(ctx, path).await;
        if self.ca_off(&status).await {
            self.publish(ctx, &status);
        }
    }

    async fn ca_off(&mut self, status: &airpods::Status) -> bool {
        if !self.mic || self.ca_user || !status.ok || !status.connected || !status.conversational_awareness {
            return false;
        }
        self.ca_restore = true;
        self.send(airpods::CA_OFF.into()).await
    }

    async fn load(&mut self, ctx: &Ctx, path: &std::path::Path) {
        let status = self.read(ctx, path).await;
        self.ca_off(&status).await;
        if !status.ok {
            self.heart_on = false;
            self.history.clear();
            self.check = None;
        }
        if self.panel && airpods::motion_watchable(&status) {
            if self.lines.is_none() && !self.motion.failed {
                self.open_motion().await;
            }
        } else {
            self.stop_motion();
        }
        self.publish(ctx, &status);
        if self.panel && !self.heart_on && let Some(verb) = airpods::heart_command(&status, true) {
            self.heart_on = true;
            if self.send(verb.into()).await {
                self.publish(ctx, &status);
            }
        }
    }

    /// The spot check's turn: one read of the file, and a check started
    /// only on connected AirPods carrying the service with a bud in.
    async fn tick(&mut self, ctx: &Ctx, path: &std::path::Path) {
        if self.panel || self.heart_on || self.check.is_some() {
            return;
        }
        let status = self.read(ctx, path).await;
        if !status.ok || !status.connected {
            self.skipped = None;
            return;
        }
        let skip = if airpods::heart_command(&status, true).is_none() {
            Some("heart rate unavailable")
        } else if !(status.left.in_ear || status.right.in_ear) {
            Some("no bud in")
        } else {
            None
        };
        if let Some(why) = skip {
            if self.skipped.replace(why) != Some(why) {
                eprintln!("earbuds: heart check skipped, {why}");
            }
            return;
        }
        self.skipped = None;
        self.heart_on = true;
        self.check = Some(Check { started: Instant::now(), first_settled: None, samples: Vec::new() });
        if self.send(airpods::HEART_ON.into()).await {
            self.publish(ctx, &status);
        }
    }

    fn keep(&mut self, check: &Check) {
        if let Some(point) = model::spot_point(&check.samples) {
            model::keep_point(&mut self.points, point);
        }
    }

    async fn end_check(&mut self, ctx: &Ctx, path: &std::path::Path) {
        let Some(check) = self.check.take() else { return };
        self.keep(&check);
        if std::mem::take(&mut self.heart_on) {
            self.send(airpods::HEART_OFF.into()).await;
        }
        self.load(ctx, path).await;
    }

    async fn open_motion(&mut self) {
        let Some(socket) = socket_path() else { return };
        let connect = async {
            let mut stream = Async::<std::os::unix::net::UnixStream>::connect(&socket).await.ok()?;
            stream.write_all(airpods::MOTION_WATCH.as_bytes()).await.ok()?;
            stream.flush().await.ok()?;
            Some(stream)
        };
        let stream = future::or(connect, async {
            Timer::after(AIRPODS_ANSWER).await;
            None
        })
        .await;
        match stream {
            Some(stream) => self.lines = Some(BufReader::new(stream).lines()),
            None => {
                eprintln!("earbuds: librepods did not take motion:watch within {}s", AIRPODS_ANSWER.as_secs());
                self.motion.failed = true;
            }
        }
    }

    /// Drops the watch and what it delivered. True when there was any.
    fn stop_motion(&mut self) -> bool {
        let had = self.lines.take().is_some() || self.motion.latest.is_some();
        self.motion = MotionState::default();
        had
    }

    fn republish(&mut self, ctx: &Ctx) {
        let status = self.last.clone();
        self.publish(ctx, &status);
    }

    fn motion_line(&mut self, ctx: &Ctx, line: &str) {
        if line.starts_with("error:") {
            eprintln!("earbuds: librepods refused motion:watch: {line}");
            self.lines = None;
            self.motion = MotionState { failed: true, ..MotionState::default() };
            self.republish(ctx);
            return;
        }
        let Some(sample) = parse_sample(line) else { return };
        // A primary swap hands the stream to the other bud, whose frame the
        // old reference means nothing in.
        if self.motion.latest.is_some_and(|last| last.bud != sample.bud) {
            self.motion.reference = None;
        }
        self.motion.reference.get_or_insert(sample.orientation);
        self.motion.latest = Some(sample);
        self.republish(ctx);
    }

    /// The daemon hung up. The next status read opens a new watch.
    fn motion_ended(&mut self, ctx: &Ctx) {
        self.lines = None;
        self.motion.latest = None;
        self.motion.reference = None;
        self.republish(ctx);
    }

    fn recenter(&mut self, ctx: &Ctx) {
        let Some(sample) = self.motion.latest else { return };
        self.motion.reference = Some(sample.orientation);
        self.republish(ctx);
    }

    fn publish(&mut self, ctx: &Ctx, status: &airpods::Status) {
        self.last = status.clone();
        let mut backend = Backend::new(status.ok);
        backend.devices = airpods::normalise(status);
        for d in &mut backend.devices {
            if self.stalled {
                d.state_line = AIRPODS_STALLED.into();
            }
            if let Some(hr) = d.heart_rate.as_mut() {
                match self.check.as_mut() {
                    Some(check) => {
                        hr.record(&mut check.samples);
                        if check.first_settled.is_none() && !check.samples.is_empty() {
                            check.first_settled = Some(Instant::now());
                        }
                    }
                    None => hr.record(&mut self.history),
                }
                hr.history = self.history.clone();
                hr.points = self.points.clone();
            }
            if self.motion.failed {
                d.motion = None;
            } else if let Some(sample) = self.motion.latest {
                let m = d.motion.get_or_insert_with(Motion::default);
                m.live = self.lines.is_some();
                m.latest = Some(sample);
                m.reference = self.motion.reference;
            }
        }
        publish(ctx, airpods::BACKEND, backend);
    }

    /// True when the stalled mark changed, so the device needs publishing.
    async fn send(&mut self, verb: String) -> bool {
        let Some(socket) = socket_path() else { return false };
        let taken = airpods_send(socket, verb.clone()).await;
        if !taken {
            eprintln!("earbuds: librepods did not take {verb} within {}s", AIRPODS_ANSWER.as_secs());
        }
        std::mem::replace(&mut self.stalled, !taken) == taken
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
fn connected_key(bt: &[BtDevice]) -> Vec<BtAddress> {
    let mut out: Vec<BtAddress> = bt.iter().filter(|d| d.connected).map(|d| d.address).collect();
    out.sort();
    out
}

/// The polling backend, for Soundcore and Samsung.
/// One task, so every CLI call (a poll's or a write's) runs one at a time.
async fn run_poll(ctx: Ctx, kind: Poller, rx: Receiver<Event>, epoch: Rc<Cell<u64>>) {
    let mut held = false;
    let mut bt: Vec<BtDevice> = Vec::new();
    let mut key: Vec<BtAddress> = Vec::new();
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
            Some(Event::Op(_) | Event::Panel(_) | Event::Mic(_) | Event::Recenter) => {}
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
    backend.caps.retain(|address, _| targets.iter().any(|t| t.address == *address));
    let mut found = Vec::new();
    for info in targets {
        if !backend.caps.contains_key(&info.address) {
            let (code, text) = run_cli(Some(soundcore::settings_argv(info.address))).await;
            backend.available = code != -1;
            let Some(caps) = (code == 0).then(|| soundcore::parse_capabilities(&text)).flatten() else { continue };
            backend.caps.insert(info.address, caps);
        }
        let caps = backend.caps[&info.address].clone();
        let argv = (!caps.ids.is_empty()).then(|| soundcore::setting_argv(info.address, &soundcore::get_args(&caps)));
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
        let (code, text) = run_cli(Some(samsung::status_argv(buds.address))).await;
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
    Line(BtAddress, u64, String),
    Exit(BtAddress, u64, i32),
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
    watchers: BTreeMap<BtAddress, Watcher>,
    states: BTreeMap<BtAddress, nothing::State>,
    failures: BTreeMap<BtAddress, &'static str>,
    /// address -> when it is retried, and the backoff that set it.
    retry: BTreeMap<BtAddress, (Instant, Backoff)>,
    /// Kept across release, so closing and reopening the panel is not a way
    /// round a refusal.
    unsupported: BTreeMap<BtAddress, nothing::Mark>,
    connected: Vec<BtAddress>,
    connected_at: BTreeMap<BtAddress, Instant>,
}

/// The Nothing backend over `nothingctl`. Discovery is `list --json`, run on
/// the first want and whenever the connected Bluetooth set changes; each
/// device it reports connected gets one `watch -d <addr>` child while held.
/// A device nothingctl refuses over its model (exit 3) is left alone until
/// BlueZ sees it disconnect and connect again; any other exit is retried
/// every 2 s for the first 90 s after a connect, then on a backoff from 5 s
/// doubling to 5 minutes.
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
                        s.connected_at.insert(*a, now);
                        s.retry.insert(*a, (now + SETTLE, Backoff::new(BACKOFF_MIN, BACKOFF_MAX)));
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
            NEvent::Line(address, id, line) => s.on_line(&ctx, address, id, &line),
            NEvent::Exit(address, id, code) => s.on_exit(&ctx, address, id, code),
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
                self.start_watch(ctx, tx, d.address);
            }
        }
        if self.list_again {
            self.list_again = false;
            self.discover(ctx, tx);
        }
    }

    fn start_watch(&mut self, ctx: &Ctx, tx: &Sender<NEvent>, address: BtAddress) {
        let argv = nothing::watch_argv(address);
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
                let _ = tx.try_send(NEvent::Exit(address, id, -1));
                return;
            }
        };
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
        let (stop, stopped) = async_channel::bounded::<()>(1);
        self.watchers.insert(address, Watcher { id, stdin, stop });
        let tx = tx.clone();
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
                        let _ = tx.send(NEvent::Line(address, id, line)).await;
                    }
                    None => break,
                }
            }
            let _ = child.kill();
            let code = child.status().await.ok().and_then(|s| s.code()).unwrap_or(-3);
            let _ = tx.send(NEvent::Exit(address, id, code)).await;
        });
    }

    fn live(&self, address: BtAddress, id: u64) -> bool {
        self.watchers.get(&address).is_some_and(|w| w.id == id)
    }

    fn on_line(&mut self, ctx: &Ctx, address: BtAddress, id: u64, line: &str) {
        if !self.held || !self.live(address, id) {
            return;
        }
        match nothing::parse_line(line) {
            Some(nothing::Line::State(state)) if state.address == address => {
                self.states.insert(address, *state);
                self.failures.remove(&address);
                self.retry.remove(&address);
            }
            Some(nothing::Line::Ack { .. }) => {
                self.failures.remove(&address);
            }
            Some(nothing::Line::Error { code, message, .. }) if code == nothing::UNSUPPORTED => {
                return self.mark_unsupported(address, &message);
            }
            Some(nothing::Line::Error { message, .. }) => {
                eprintln!("nothingctl {address}: {message}");
                self.failures.insert(address, nothing::FAILED);
            }
            Some(nothing::Line::Disconnected) => {
                self.states.remove(&address);
                self.failures.remove(&address);
            }
            _ => return,
        }
        self.publish(ctx);
    }

    fn mark_unsupported(&mut self, address: BtAddress, message: &str) {
        if self.unsupported.contains_key(&address) {
            return;
        }
        let why = if message.is_empty() { "unsupported model" } else { message };
        eprintln!("nothingctl {address}: {why}; not retried until it reconnects");
        self.unsupported.insert(address, nothing::Mark { seen_down: false });
    }

    fn on_exit(&mut self, ctx: &Ctx, address: BtAddress, id: u64, code: i32) {
        if self.live(address, id) {
            self.watchers.remove(&address);
        }
        if code == nothing::UNSUPPORTED_EXIT {
            self.mark_unsupported(address, "");
        }
        if !self.held {
            return;
        }
        self.states.remove(&address);
        self.failures.remove(&address);
        self.publish(ctx);
        if self.unsupported.contains_key(&address) {
            return;
        }
        let fresh = self.connected_at.get(&address).is_some_and(|at| at.elapsed() < QUICK_WINDOW);
        let mut backoff = self.retry.remove(&address).map_or_else(|| Backoff::new(BACKOFF_MIN, BACKOFF_MAX), |(_, b)| b);
        let delay = if fresh {
            backoff.reset();
            QUICK
        } else {
            backoff.delay()
        };
        self.retry.insert(address, (Instant::now() + delay, backoff));
    }
}

/// A picture set's part (`duo`, `case`, `left`, `right`) in a `size`
/// square at `scale`, from `FS_EARBUDS_ART` (packaging/earbuds-art.py); none without the
/// directory, a set, or that part.
pub fn picture(set: &str, part: &str, size: f64, scale: f64) -> Option<crate::scene::Bitmap> {
    if set.is_empty() {
        return None;
    }
    let dir = std::path::PathBuf::from(std::env::var_os("FS_EARBUDS_ART")?);
    crate::services::icons::load_at(&dir.join(format!("{set}_{part}.webp")), size.round().max(1.0) as u32, scale)
}

