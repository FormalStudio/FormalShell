//! NetworkManager through fs-network: what the network cell reads (wired or
//! Wi-Fi connected, the network the Wi-Fi device is on, the radio switch)
//! and what the network panel and IPC drive, the action bookkeeping
//! included. The panel's speed test and its password and QR rows are
//! `speed` and `share`, publishing through this module's state.
//!
//! One action is in flight at a time (`connect`, `disconnect`, `forget`).
//! A connect settles on its own activation result; a disconnect or forget
//! settles on the snapshot showing it done. Every action that has not
//! settled 15s in reads "Timed out".

use std::cell::RefCell;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_io::Timer;
use fs_network::{ConnectError, DeviceKind, NetworkManager, Snapshot};
use fs_types::{Fraction, Percent};
use futures_lite::{FutureExt, StreamExt, future};

use super::action::ActionInFlight;
use crate::runtime::Ctx;
use crate::services::info;
use crate::services::wants::Source;
use crate::store;

mod share;
mod speed;

pub use share::{Qr, Reveal, qr_toggle, reveal_toggle};
pub use speed::{Phase, Speed, speed_start, speed_stop};

/// How long a panel action may stay unsettled.
const ACTION_TIMEOUT: Duration = Duration::from_secs(15);
/// How often a held scanner asks the radio for a fresh scan.
const SCAN_EVERY: Duration = Duration::from_secs(10);

/// ConnectivityService's settle between the link coming up and the refetch.
const RECONNECT_SETTLE: Duration = Duration::from_secs(3);

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
    pub signal: Fraction,
    /// `Security::label`'s name for its class.
    pub security: String,
    /// The device's addresses on it, `address/prefix`, while connected.
    pub addresses: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wired {
    /// The interface.
    pub name: String,
    pub connected: bool,
    /// The active connection's name.
    pub connection: Option<String>,
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


#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    /// False until NetworkManager has answered.
    pub known: bool,
    pub wired: bool,
    pub wifi: bool,
    pub wifi_enabled: bool,
    /// The connected network's SSID and signal, in percent.
    pub ssid: Option<(String, Percent)>,
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
    flight: ActionInFlight<(ActionKind, String), Failure>,
    speed: Speed,
    /// Dropping it stops the running speed test.
    speed_stop: Option<async_channel::Sender<()>>,
    scan_stop: Option<async_channel::Sender<()>>,
    published: Option<Network>,
    reveal: Reveal,
    qr: Qr,
    /// The passphrase read both the reveal and the share wait on.
    reading_fields: bool,
}

thread_local! {
    static INNER: RefCell<Inner> = RefCell::new(Inner::default());
}

fn view(i: &Inner) -> Network {
    let mut n = Network {
        action: i.flight.action.clone(),
        failure: i.flight.failure.clone(),
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
    n.ssid = s.networks.iter().find(|n| n.connected()).map(|n| (n.ssid.clone(), n.signal.to_percent()));
    n.wifi_device = s
        .devices
        .iter()
        .find(|d| d.kind == DeviceKind::Wifi && d.managed())
        .map(|d| (d.interface.clone(), d.hw_address.clone()));
    n.wired_rows = s
        .devices
        .iter()
        .filter(|d| d.kind == DeviceKind::Wired && d.managed())
        .map(|d| Wired { name: d.interface.clone(), connected: d.connected(), connection: d.connection.clone() })
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
            signal: w.signal.to_fraction(),
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
            ctx.publish(store::Diff::Network(n));
        }
    });
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    INNER.with_borrow_mut(|i| i.ctx = Some(ctx.clone()));
    ctx.spawn(holds(ctx.clone()));
    let publish_none = || ctx.publish(store::Diff::Network(Network::default()));
    // A connect awaits NetworkManager's replies while its StateChanged
    // stream waits, so NetworkManager gets a connection of its own.
    let Ok(conn) = crate::services::bus::private(&ctx, zbus::connection::Builder::system()).await else { return publish_none() };
    let nm = match NetworkManager::connect(&conn, fs_auth::pam::current_user()).await {
        Ok(nm) => nm,
        Err(err) => {
            eprintln!("network: {err}");
            return publish_none();
        }
    };
    let mut changes = std::pin::pin!(nm.changes());
    let mut online = false;
    // DHCP and DNS on a fresh association are not up yet, so the polls that
    // went stale offline go again once this fires; dropped when the link
    // goes down first.
    let mut settling: Option<Timer> = None;
    loop {
        let next = match settling.as_mut() {
            Some(timer) => {
                future::or(async { Some(changes.next().await) }, async {
                    timer.await;
                    None
                })
                .await
            }
            None => Some(changes.next().await),
        };
        let snapshot = match next {
            Some(Some(snapshot)) => snapshot,
            Some(None) => return,
            None => {
                settling = None;
                for source in [Source::Weather, Source::Github, Source::Tailscale, Source::SystemUpdate] {
                    info::kick(source);
                }
                continue;
            }
        };
        let up = snapshot.as_ref().is_some_and(|s| s.devices.iter().any(|d| d.connected()));
        if up != online {
            online = up;
            settling = up.then(|| Timer::after(RECONNECT_SETTLE));
        }
        INNER.with_borrow_mut(|i| {
            let was = connected_ssid(i);
            // Actions are refused while NetworkManager is off the bus.
            i.nm = snapshot.is_some().then(|| nm.clone());
            i.snapshot = snapshot;
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
    let Some((kind, ssid)) = i.flight.action.clone() else { return };
    let Some(s) = &i.snapshot else { return };
    let net = s.networks.iter().find(|n| n.ssid == ssid);
    let done = match kind {
        ActionKind::Connect => false,
        ActionKind::Disconnect => net.is_none_or(|n| !n.connected() && !n.state.is_changing()),
        ActionKind::Forget => net.is_none_or(|n| !n.known && !n.state.is_changing()),
    };
    if done {
        i.flight.action = None;
    }
}

/// `runAction`: false while another action runs.
fn begin(ctx: &Ctx, kind: ActionKind, ssid: &str) -> bool {
    let generation = INNER.with_borrow_mut(|i| i.flight.begin((kind, ssid.to_owned())));
    let Some(generation) = generation else { return false };
    publish();
    ctx.spawn(async move {
        Timer::after(ACTION_TIMEOUT).await;
        let timed_out = INNER.with_borrow_mut(|i| {
            i.flight.end(generation, |(_, ssid)| Some(Failure { ssid, text: "Timed out".into(), secret: false }))
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
        if i.flight.action.as_ref().is_none_or(|(k, s)| *k != ActionKind::Connect || s != ssid) {
            return;
        }
        i.flight.end(generation, |_| {
            let (text, secret) = match result {
                Ok(()) => return None,
                Err(ConnectError::WrongPassword) => ("Wrong password", true),
                Err(ConnectError::SecretsRequired) => ("Passphrase required", true),
                Err(ConnectError::Timeout) => ("Timed out", false),
                Err(err) => {
                    eprintln!("network: {ssid}: {err}");
                    ("Connection failed", false)
                }
            };
            Some(Failure { ssid: ssid.to_owned(), text: text.into(), secret })
        });
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
    let generation = INNER.with_borrow(|i| i.flight.generation());
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

/// A saved passphrase read back for the launcher, held in the store only
/// until the launcher takes it.
#[derive(Clone, PartialEq)]
pub struct Revealed {
    pub ssid: String,
    /// The caller's id for the request this answers.
    pub request: u64,
    /// The passphrase, `Ok(None)` with none saved, or why it was not read.
    pub secret: Result<Option<String>, String>,
}

pub fn reveal(ctx: &Ctx, ssid: String, request: u64) {
    let Some(nm) = nm() else { return };
    let ctx = ctx.clone();
    ctx.clone().spawn(async move {
        let secret = nm.saved_secret(&ssid).await.map(|s| s.map(|s| s.text().to_owned())).map_err(|err| err.to_string());
        if let Err(err) = &secret {
            eprintln!("network: no passphrase read for {ssid}: {err}");
        }
        ctx.publish(store::Diff::Revealed(Revealed { ssid, request, secret }));
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
        i.flight.clear_if(|(k, s)| *k == kind && s == ssid);
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

