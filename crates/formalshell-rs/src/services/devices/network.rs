//! NetworkManager through fs-network: what the network cell reads (wired or
//! Wi-Fi connected, the network the Wi-Fi device is on, the radio switch)
//! and what the network panel and IPC drive, the
//! action bookkeeping and the panel's speed test included.
//!
//! One action is in flight at a time (`connect`, `disconnect`, `forget`).
//! A connect settles on its own activation result; a disconnect or forget
//! settles on the snapshot showing it done. Every action that has not
//! settled 15s in reads "Timed out".

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_io::Timer;
use fs_devices::network::speedtest;
use fs_network::{ConnectError, DeviceKind, NetworkManager, Snapshot};
use futures_lite::{FutureExt, StreamExt};

use crate::runtime::Ctx;
use crate::services::info;
use crate::services::wants::Source;
use crate::store;

/// How long a panel action may stay unsettled.
const ACTION_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a held scanner asks the radio for a fresh scan.
const SCAN_EVERY: Duration = Duration::from_secs(10);

/// ConnectivityService's settle between the link coming up and the refetch.
const RECONNECT_SETTLE: Duration = Duration::from_secs(3);

const ST_PHASE: Duration = Duration::from_millis(5000);
const ST_SAMPLE: Duration = Duration::from_millis(500);
const ST_DOWN_URL: &str = "https://speed.cloudflare.com/__down?bytes=25000000";
const ST_UP_URL: &str = "https://speed.cloudflare.com/__up";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// Empty for a hidden network.
    pub ssid: String,
    pub known: bool,
    pub connected: bool,
    pub changing: bool,
    pub secured: bool,
    pub enterprise: bool,
    /// An access point broadcasts it; false only for a saved network.
    pub in_range: bool,
    /// 0 to 1.
    pub signal: f64,
    /// `Security::label`'s name for its class.
    pub security: String,
    /// The device's addresses on it, `address/prefix`, while connected.
    pub addresses: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wired {
    pub name: String,
    pub connected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Connect,
    Disconnect,
    Forget,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Connect => "connect",
            ActionKind::Disconnect => "disconnect",
            ActionKind::Forget => "forget",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    pub ssid: String,
    pub text: String,
    /// Fixed by typing the secret again rather than retrying.
    pub secret: bool,
}

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

/// The connected network's saved passphrase, read only when asked for and
/// dropped on hide, on the panel closing and on roaming. No IPC verb reads
/// it, and its `Debug` never prints it.
#[derive(Clone, Default, PartialEq)]
pub enum Reveal {
    #[default]
    Idle,
    Reading,
    Shown(String),
    Failed(&'static str),
}

/// The share row's code. The matrix encodes the passphrase, so it lives
/// exactly as long as the expanded row.
#[derive(Clone, Default, PartialEq)]
pub enum Qr {
    #[default]
    Closed,
    Generating,
    Failed(&'static str),
    Shown(Vec<String>),
}

impl std::fmt::Debug for Reveal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reveal::Shown(_) => f.write_str("Shown(..)"),
            Reveal::Idle => f.write_str("Idle"),
            Reveal::Reading => f.write_str("Reading"),
            Reveal::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

impl std::fmt::Debug for Qr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Qr::Shown(_) => f.write_str("Shown(..)"),
            Qr::Closed => f.write_str("Closed"),
            Qr::Generating => f.write_str("Generating"),
            Qr::Failed(e) => write!(f, "Failed({e})"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    /// False until NetworkManager has answered.
    pub known: bool,
    pub wired: bool,
    pub wifi: bool,
    pub wifi_enabled: bool,
    /// The connected network's SSID and signal, in percent.
    pub ssid: Option<(String, u8)>,
    /// The first Wi-Fi device's interface and hardware address.
    pub wifi_device: Option<(String, String)>,
    pub wired_rows: Vec<Wired>,
    /// Connected first, then the rest in range (saved, then by signal), then
    /// saved networks out of range.
    pub rows: Vec<Row>,
    pub action: Option<(ActionKind, String)>,
    pub failure: Option<Failure>,
    pub speed: Speed,
    pub reveal: Reveal,
    pub qr: Qr,
}

impl Network {
    pub fn row(&self, ssid: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.ssid == ssid)
    }
}

#[derive(Default)]
struct Inner {
    ctx: Option<Ctx>,
    nm: Option<NetworkManager>,
    snapshot: Option<Snapshot>,
    action: Option<(ActionKind, String)>,
    /// Which action the running timeout belongs to.
    generation: u64,
    failure: Option<Failure>,
    speed: Speed,
    /// Dropping it stops the running speed test.
    speed_stop: Option<async_channel::Sender<()>>,
    scan_stop: Option<async_channel::Sender<()>>,
    published: Option<Network>,
    reveal: Reveal,
    qr: Qr,
    /// The nmcli read both the reveal and the share wait on.
    reading_fields: bool,
}

thread_local! {
    static INNER: RefCell<Inner> = RefCell::new(Inner::default());
}

fn address(iface: &str) -> String {
    std::fs::read_to_string(format!("/sys/class/net/{iface}/address")).map(|s| s.trim().to_owned()).unwrap_or_default()
}

fn view(i: &Inner) -> Network {
    let mut n = Network {
        action: i.action.clone(),
        failure: i.failure.clone(),
        speed: i.speed.clone(),
        reveal: i.reveal.clone(),
        qr: i.qr.clone(),
        ..Network::default()
    };
    let Some(s) = &i.snapshot else { return n };
    let up = |kind| s.devices.iter().any(|d| d.kind == kind && d.connected());
    n.known = true;
    n.wired = up(DeviceKind::Wired);
    n.wifi = up(DeviceKind::Wifi);
    n.wifi_enabled = s.wifi_enabled;
    n.ssid = s.networks.iter().find(|n| n.connected()).map(|n| (n.ssid.clone(), n.signal.as_percent()));
    // The interface, not the device: an address read here keeps one sysfs
    // read per snapshot, never one per draw.
    let reuse = i.published.as_ref().and_then(|p| p.wifi_device.clone());
    n.wifi_device = s.devices.iter().find(|d| d.kind == DeviceKind::Wifi && d.managed()).map(|d| match reuse {
        Some((iface, mac)) if iface == d.interface => (iface, mac),
        _ => (d.interface.clone(), address(&d.interface)),
    });
    n.wired_rows = s
        .devices
        .iter()
        .filter(|d| d.kind == DeviceKind::Wired && d.managed())
        .map(|d| Wired { name: d.interface.clone(), connected: d.connected() })
        .collect();
    n.rows = s
        .networks
        .iter()
        .map(|w| Row {
            ssid: w.ssid.clone(),
            known: w.known,
            connected: w.connected(),
            changing: w.state.is_changing(),
            secured: w.security.is_secured(),
            enterprise: w.security.is_enterprise(),
            in_range: w.visible || w.connected(),
            signal: w.signal.as_fraction(),
            security: w.security.label().to_owned(),
            addresses: w.addresses.clone(),
        })
        .collect();
    n
}

fn publish() {
    INNER.with_borrow_mut(|i| {
        let n = view(i);
        if i.published.as_ref() == Some(&n) {
            return;
        }
        i.published = Some(n.clone());
        if let Some(ctx) = &i.ctx {
            ctx.publish(store::Diff::Devices(super::Diff::Network(n)));
        }
    });
}

pub async fn run(ctx: Ctx) {
    INNER.with_borrow_mut(|i| i.ctx = Some(ctx.clone()));
    ctx.spawn(holds(ctx.clone()));
    let publish_none = || ctx.publish(store::Diff::Devices(super::Diff::Network(Network::default())));
    let Ok(conn) = crate::services::bus::private(&ctx, zbus::connection::Builder::system()).await else { return publish_none() };
    let nm = match NetworkManager::connect(&conn).await {
        Ok(nm) => nm,
        Err(err) => {
            eprintln!("network: {err}");
            return publish_none();
        }
    };
    let mut changes = match nm.changes().await {
        Ok(changes) => Box::pin(changes),
        Err(err) => {
            eprintln!("network: {err}");
            return publish_none();
        }
    };
    INNER.with_borrow_mut(|i| i.nm = Some(nm));
    let mut online = false;
    let mut settling: Option<std::rc::Rc<std::cell::Cell<bool>>> = None;
    while let Some(snapshot) = changes.next().await {
        let up = snapshot.devices.iter().any(|d| d.connected());
        if up != online {
            online = up;
            if let Some(stale) = settling.take() {
                stale.set(true);
            }
            if up {
                // DHCP and DNS on a fresh association are not up yet, so the
                // polls that went stale offline go again after a settle.
                let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
                settling = Some(cancelled.clone());
                ctx.spawn(async move {
                    Timer::after(RECONNECT_SETTLE).await;
                    if !cancelled.get() {
                        for source in [Source::Weather, Source::Github, Source::Tailscale, Source::SystemUpdate] {
                            info::kick(source);
                        }
                    }
                });
            }
        }
        INNER.with_borrow_mut(|i| {
            let was = connected_ssid(i);
            i.snapshot = Some(snapshot);
            if connected_ssid(i) != was {
                i.reveal = Reveal::Idle;
            }
            settle(i);
        });
        publish();
    }
}

fn connected_ssid(i: &Inner) -> Option<String> {
    i.snapshot.as_ref()?.networks.iter().find(|n| n.connected()).map(|n| n.ssid.clone())
}

fn nm() -> Option<NetworkManager> {
    INNER.with_borrow(|i| i.nm.clone())
}

/// The completion check for the two actions that
/// settle on what NetworkManager shows rather than on a call's result.
fn settle(i: &mut Inner) {
    let Some((kind, ssid)) = i.action.clone() else { return };
    let Some(s) = &i.snapshot else { return };
    let net = s.networks.iter().find(|n| n.ssid == ssid);
    let done = match kind {
        ActionKind::Connect => false,
        ActionKind::Disconnect => net.is_none_or(|n| !n.connected() && !n.state.is_changing()),
        ActionKind::Forget => net.is_none_or(|n| !n.known && !n.state.is_changing()),
    };
    if done {
        i.action = None;
    }
}

/// `runAction`: false while another action runs.
fn begin(ctx: &Ctx, kind: ActionKind, ssid: &str) -> bool {
    let generation = INNER.with_borrow_mut(|i| {
        if i.action.is_some() {
            return None;
        }
        i.action = Some((kind, ssid.to_owned()));
        i.failure = None;
        i.generation += 1;
        Some(i.generation)
    });
    let Some(generation) = generation else { return false };
    publish();
    ctx.spawn(async move {
        Timer::after(ACTION_TIMEOUT).await;
        let timed_out = INNER.with_borrow_mut(|i| {
            if i.generation != generation || i.action.is_none() {
                return false;
            }
            let (_, ssid) = i.action.take().unwrap_or((kind, String::new()));
            i.failure = Some(Failure { ssid, text: "Timed out".into(), secret: false });
            true
        });
        if timed_out {
            publish();
        }
    });
    true
}

/// The connect started by `generation` came back.
fn finish_connect(generation: u64, ssid: &str, result: Result<(), ConnectError>) {
    INNER.with_borrow_mut(|i| {
        if i.generation != generation || i.action.as_ref().is_none_or(|(k, s)| *k != ActionKind::Connect || s != ssid) {
            return;
        }
        i.action = None;
        let (text, secret) = match result {
            Ok(()) => return,
            Err(ConnectError::WrongPassword) => ("Wrong password", true),
            Err(ConnectError::SecretsRequired) => ("Passphrase required", true),
            Err(ConnectError::Timeout) => ("Timed out", false),
            Err(err) => {
                eprintln!("network: {ssid}: {err}");
                ("Connection failed", false)
            }
        };
        i.failure = Some(Failure { ssid: ssid.to_owned(), text: text.into(), secret });
    });
    publish();
}

/// How a connect authenticates.
#[derive(Clone)]
pub enum Secret {
    Saved,
    Psk(String),
    Eap { identity: String, password: String },
}

/// What a connect attempt asks of the service. Secrets ride the closure to
/// NetworkManager's D-Bus API and are never logged or kept.
pub fn connect(ctx: &Ctx, ssid: String, secret: Secret) {
    let Some(nm) = nm() else { return };
    if !begin(ctx, ActionKind::Connect, &ssid) {
        return;
    }
    let generation = INNER.with_borrow(|i| i.generation);
    ctx.spawn(async move {
        let result = match secret {
            Secret::Saved => nm.connect_saved(&ssid).await,
            Secret::Psk(psk) => nm.connect_psk(&ssid, &psk).await,
            Secret::Eap { identity, password } => nm.connect_eap(&ssid, &identity, &password).await,
        };
        finish_connect(generation, &ssid, result);
    });
}

pub fn disconnect(ctx: &Ctx, ssid: String) {
    let Some(nm) = nm() else { return };
    if !begin(ctx, ActionKind::Disconnect, &ssid) {
        return;
    }
    ctx.spawn(async move {
        if let Err(err) = nm.disconnect(&ssid).await {
            eprintln!("network: {err}");
            clear(ActionKind::Disconnect, &ssid);
        }
    });
}

pub fn forget(ctx: &Ctx, ssid: String) {
    let Some(nm) = nm() else { return };
    if !begin(ctx, ActionKind::Forget, &ssid) {
        return;
    }
    ctx.spawn(async move {
        match nm.forget(&ssid).await {
            Ok(_) => {
                // A profile out of range has nothing left to signal.
                if let Ok(s) = nm.snapshot().await {
                    INNER.with_borrow_mut(|i| {
                        i.snapshot = Some(s);
                        settle(i);
                    });
                    publish();
                }
            }
            Err(err) => {
                eprintln!("network: {err}");
                clear(ActionKind::Forget, &ssid);
            }
        }
    });
}

/// A wired row's click: down while it is up, else up on the profile
/// NetworkManager picks.
pub fn wired_toggle(ctx: &Ctx, interface: String, connected: bool) {
    let Some(nm) = nm() else { return };
    ctx.spawn(async move {
        let done = if connected { nm.disconnect_wired(&interface).await } else { nm.connect_wired(&interface).await };
        if let Err(err) = done {
            eprintln!("network: {err}");
        }
    });
}

fn clear(kind: ActionKind, ssid: &str) {
    INNER.with_borrow_mut(|i| {
        if i.action.as_ref().is_some_and(|(k, s)| *k == kind && s == ssid) {
            i.action = None;
        }
    });
    publish();
}

/// NetworkManager's radio switch, set outright.
pub fn set_wifi(ctx: &Ctx, on: bool) {
    let Some(nm) = nm() else { return };
    ctx.spawn(async move {
        if let Err(err) = nm.set_wifi_enabled(on).await {
            eprintln!("network: {err}");
        }
    });
}

/// `RequestScan` on every Wi-Fi device NetworkManager has.
pub fn rescan(ctx: &Ctx) {
    let Some(nm) = nm() else { return };
    ctx.spawn(async move {
        if let Err(err) = nm.request_scan().await {
            eprintln!("network: {err}");
        }
    });
}

/// The network cell's right click: `Networking.wifiEnabled` flipped.
pub fn toggle_wifi(ctx: &Ctx) {
    let Some(nm) = nm() else { return };
    ctx.spawn(async move {
        let result = match nm.snapshot().await {
            Ok(s) => nm.set_wifi_enabled(!s.wifi_enabled).await,
            Err(err) => Err(err),
        };
        if let Err(err) = result {
            eprintln!("network: {err}");
        }
    });
}

// ---- Holds: the open panel keeps the scanner running, and its close ends
// the speed test (WifiService.holdScan, NetworkPanel.onIsOpenChanged).

static HOLDS: AtomicUsize = AtomicUsize::new(0);
static POKE: OnceLock<async_channel::Sender<()>> = OnceLock::new();

fn poke() {
    if let Some(tx) = POKE.get() {
        let _ = tx.try_send(());
    }
}

/// Held by the network panel for as long as it is open.
pub struct Hold;

impl Hold {
    pub fn new() -> Self {
        HOLDS.fetch_add(1, Ordering::SeqCst);
        poke();
        Self
    }
}

impl Drop for Hold {
    fn drop(&mut self) {
        HOLDS.fetch_sub(1, Ordering::SeqCst);
        poke();
    }
}

async fn holds(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = POKE.set(tx);
    loop {
        let held = HOLDS.load(Ordering::SeqCst) > 0;
        let scanning = INNER.with_borrow(|i| i.scan_stop.is_some());
        if held && !scanning {
            let (stop, stopped) = async_channel::bounded::<()>(1);
            INNER.with_borrow_mut(|i| i.scan_stop = Some(stop));
            let c = ctx.clone();
            ctx.spawn(
                async move {
                    loop {
                        rescan(&c);
                        Timer::after(SCAN_EVERY).await;
                    }
                }
                .or(async move {
                    let _ = stopped.recv().await;
                }),
            );
        } else if !held && scanning {
            INNER.with_borrow_mut(|i| {
                i.scan_stop = None;
                i.reveal = Reveal::Idle;
                i.qr = Qr::Closed;
            });
            speed_stop();
            publish();
        }
        if rx.recv().await.is_err() {
            return;
        }
    }
}

// ---- Speed test.

/// False when a test is already running.
pub fn speed_start(ctx: &Ctx) -> bool {
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
pub fn speed_stop() {
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

async fn sh(script: &str) -> Option<(bool, String)> {
    let out = crate::services::proc::command("sh")
        .args(["-c", script])
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .output()
        .await
        .ok()?;
    Some((out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned()))
}

/// `(rx, tx)` off the interface's own counters; a missing interface is
/// `None`, which ends the test honestly rather than reporting a stale rate.
fn stat_bytes(iface: &str) -> Option<(f64, f64)> {
    let read = |which: &str| std::fs::read_to_string(format!("/sys/class/net/{iface}/statistics/{which}"));
    let text = format!("{}{}", read("rx_bytes").ok()?, read("tx_bytes").ok()?);
    speedtest::parse_stat_bytes(&text).map(|b| (b.rx, b.tx))
}

/// One worker: curl after curl until the phase drops it. Each curl is this
/// process's own child, so dropping the future kills it by pid.
async fn worker(up: bool) {
    loop {
        let mut cmd = crate::services::proc::command("curl");
        cmd.args(["-s", "-o", "/dev/null"]);
        if up {
            cmd.args(["-X", "POST", "-T", "/dev/zero", ST_UP_URL]);
        } else {
            cmd.arg(ST_DOWN_URL);
        }
        let child = cmd
            .stdin(async_process::Stdio::null())
            .stdout(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else { return };
        match child.status().await {
            Ok(s) if s.success() => {}
            _ => return,
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
    let iface = sh("ip route get 1.1.1.1 2>/dev/null").await.and_then(|(_, out)| speedtest::parse_iface(&out));
    let Some(iface) = iface else { return speed_abort("NO NETWORK") };
    if !sh("command -v curl >/dev/null 2>&1").await.is_some_and(|(ok, _)| ok) {
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

// ---- Share and reveal (the panel's QR share and password rows).

/// One nmcli read serves both: the connection the default route runs when
/// it is wireless, else the first connected Wi-Fi device. Exit 2 is no
/// nmcli, 3 no active Wi-Fi connection.
const FIELDS_SCRIPT: &str = concat!(
    "command -v nmcli >/dev/null 2>&1 || exit 2;",
    " d=$(ip route get 1.1.1.1 2>/dev/null | awk '{ for (i = 1; i <= NF; i++) if ($i == \"dev\") { print $(i + 1); exit } }');",
    " if [ -z \"$d\" ] || [ ! -d \"/sys/class/net/$d/wireless\" ]; then",
    " d=$(LC_ALL=C nmcli -t -f DEVICE,TYPE,STATE device status 2>/dev/null | awk -F: '$2 == \"wifi\" && $3 ~ /^connected/ { print $1; exit }');",
    " fi;",
    " [ -n \"$d\" ] || exit 3;",
    " u=$(nmcli --get-values GENERAL.CON-UUID device show \"$d\" 2>/dev/null | head -n 1);",
    " [ -n \"$u\" ] && [ \"$u\" != \"--\" ] || exit 3;",
    " nmcli --show-secrets --escape no --get-values",
    " 802-11-wireless.ssid,802-11-wireless-security.key-mgmt,802-11-wireless-security.psk,802-11-wireless.hidden,802-11-wireless-security.wep-key0",
    " connection show uuid \"$u\""
);

/// SHOW or HIDE on the password row.
pub fn reveal_toggle(ctx: &Ctx) {
    let read = INNER.with_borrow_mut(|i| {
        if i.reveal != Reveal::Idle {
            i.reveal = Reveal::Idle;
            false
        } else {
            i.reveal = Reveal::Reading;
            true
        }
    });
    publish();
    if read {
        request_fields(ctx);
    }
}

/// The share row expanding or collapsing.
pub fn qr_toggle(ctx: &Ctx) {
    let open = INNER.with_borrow_mut(|i| {
        if i.qr != Qr::Closed {
            i.qr = Qr::Closed;
            false
        } else {
            i.qr = Qr::Generating;
            true
        }
    });
    publish();
    if !open {
        return;
    }
    let c = ctx.clone();
    ctx.spawn(async move {
        let have = sh("command -v qrencode >/dev/null 2>&1").await.is_some_and(|(ok, _)| ok);
        if !have {
            return qr_fail("NO QRENCODE");
        }
        if INNER.with_borrow(|i| i.qr == Qr::Generating) {
            request_fields(&c);
        }
    });
}

fn qr_fail(message: &'static str) {
    INNER.with_borrow_mut(|i| {
        if i.qr == Qr::Generating {
            i.qr = Qr::Failed(message);
        }
    });
    publish();
}

/// A request while a read is in flight joins it.
fn request_fields(ctx: &Ctx) {
    if INNER.with_borrow_mut(|i| std::mem::replace(&mut i.reading_fields, true)) {
        return;
    }
    let c = ctx.clone();
    ctx.spawn(async move {
        let out = crate::services::proc::command("sh")
            .args(["-c", FIELDS_SCRIPT])
            .stdin(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .output()
            .await;
        INNER.with_borrow_mut(|i| i.reading_fields = false);
        let (code, text) = match out {
            Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned()),
            Err(_) => (-1, String::new()),
        };
        on_fields_reveal(code, &text);
        on_fields_qr(&c, code, &text);
    });
}

fn on_fields_reveal(code: i32, text: &str) {
    use fs_devices::network::wifiqr;
    let next = if code == 2 {
        Reveal::Failed("NO NMCLI")
    } else if code != 0 {
        Reveal::Failed("ERROR")
    } else {
        let f = wifiqr::parse_fields(text);
        if wifiqr::is_enterprise_key_mgmt(&f.key_mgmt) {
            Reveal::Failed("ENTERPRISE")
        } else {
            let secured = !f.key_mgmt.is_empty() && f.key_mgmt != "none";
            let secret = if secured { f.password } else { f.wep_key };
            match (secret.is_empty(), secured) {
                (true, true) => Reveal::Failed("NO PASSWORD SAVED"),
                (true, false) => Reveal::Failed("OPEN NETWORK"),
                (false, _) => Reveal::Shown(secret),
            }
        }
    };
    INNER.with_borrow_mut(|i| {
        if i.reveal == Reveal::Reading {
            i.reveal = next;
        }
    });
    publish();
}

fn on_fields_qr(ctx: &Ctx, code: i32, text: &str) {
    use fs_devices::network::wifiqr::{self, PayloadError};
    if INNER.with_borrow(|i| i.qr != Qr::Generating) {
        return;
    }
    if code == 3 {
        return qr_fail("NOT CONNECTED");
    }
    if code != 0 {
        return qr_fail("ERROR");
    }
    let payload = match wifiqr::build_payload(&wifiqr::parse_fields(text)) {
        Ok(p) => p,
        Err(PayloadError::Enterprise) => return qr_fail("ENTERPRISE CANNOT SHARE"),
        Err(PayloadError::NoSsid) => return qr_fail("NOT CONNECTED"),
        Err(_) => return qr_fail("ERROR"),
    };
    // The payload reaches qrencode over stdin, never argv.
    ctx.spawn(async move {
        use futures_lite::AsyncWriteExt;
        let child = crate::services::proc::command("sh")
            .args(["-c", "qrencode --type ASCII --margin 4 --output -"])
            .stdin(async_process::Stdio::piped())
            .stdout(async_process::Stdio::piped())
            .stderr(async_process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return qr_fail("ERROR") };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(payload.as_bytes()).await;
        }
        drop(payload);
        let Ok(out) = child.output().await else { return qr_fail("ERROR") };
        if !out.status.success() {
            return qr_fail("ERROR");
        }
        let matrix = wifiqr::parse_matrix(&String::from_utf8_lossy(&out.stdout));
        if matrix.is_empty() {
            return qr_fail("ERROR");
        }
        INNER.with_borrow_mut(|i| {
            if i.qr == Qr::Generating {
                i.qr = Qr::Shown(matrix);
            }
        });
        publish();
    });
}
