//! BlueZ through fs-bluez: the default adapter's state for the bar cell and
//! the Bluetooth panel, the device list the earbuds backends cross their
//! own lists with, the panel's one action in flight, and every device's
//! AVRCP player as a media source (`media::Remote`).
//!
//! The panel's actions run here, one at a time, each settled by BlueZ's own
//! answer: a method that errors fails its row, one that never answers fails
//! it after [`ACTION_TIMEOUT`].
//! Discovery runs only while the panel is open on a powered adapter, and is
//! re-armed whenever BlueZ lets it lapse; a refused start waits for the
//! adapter to change (it powering up) rather than retrying on a timer.

use std::cell::RefCell;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use fs_bluez::state::{AdapterState, Device as BluezDevice, DeviceState, Event, known};
use fs_types::BtAddress;
use futures_lite::{FutureExt, future};

use super::action::ActionInFlight;
use super::headsets::{self, Headset};
use crate::runtime::Ctx;
use crate::services::media::{self, Remote};
use crate::store;

const ACTION_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Pair,
    Connect,
    Disconnect,
    Forget,
    Trust,
    Untrust,
}

impl ActionKind {
    /// The row's status line while it runs.
    pub fn text(self) -> &'static str {
        match self {
            ActionKind::Pair => "Pairing\u{2026}",
            ActionKind::Connect => "Connecting\u{2026}",
            ActionKind::Disconnect => "Disconnecting\u{2026}",
            ActionKind::Forget => "Forgetting\u{2026}",
            ActionKind::Trust => "Trusting\u{2026}",
            ActionKind::Untrust => "Untrusting\u{2026}",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bluetooth {
    /// None with no adapter (or no bluetoothd).
    pub powered: Option<bool>,
    pub connected: Vec<String>,
    /// The default adapter's name, and its state word.
    pub adapter: String,
    pub state: &'static str,
    pub discovering: bool,
    /// Every device of the default adapter.
    pub devices: Vec<fs_devices::bluetooth::Device>,
    /// The action in flight: its device's address and what it does.
    pub action: Option<(BtAddress, ActionKind)>,
    /// The last action that failed: its device's address and why.
    pub failure: Option<(BtAddress, &'static str)>,
    /// The Bluetooth audio devices, connected or not; none until BlueZ (or
    /// its absence) has answered once.
    pub headsets: Option<Vec<Headset>>,
}

thread_local! {
    static BLUEZ: RefCell<Option<fs_bluez::Bluez>> = const { RefCell::new(None) };
}

/// The adapter state's name.
fn state_word(s: AdapterState) -> &'static str {
    match s {
        AdapterState::Disabled => "Disabled",
        AdapterState::Enabled => "Enabled",
        AdapterState::Enabling => "Enabling",
        AdapterState::Disabling => "Disabling",
    }
}

/// The panel's view of a BlueZ device: its name is the label the user sees.
fn panel_device(d: BluezDevice) -> fs_devices::bluetooth::Device {
    fs_devices::bluetooth::Device {
        address: d.address,
        name: d.label().to_owned(),
        device_name: d.name,
        connected: d.connected,
        paired: d.paired,
        bonded: d.bonded,
        trusted: d.trusted,
        pairing: d.pairing,
        connecting: d.state == DeviceState::Connecting,
        battery: d.battery,
    }
}

/// Whether an event can move what a surface shows: any adapter change, and
/// a device change for a known device, one already on show (`shown`, by
/// address), or any device while a panel is `open`.
fn matters(e: &Event, open: bool, shown: &[BtAddress]) -> bool {
    match e {
        Event::DeviceAdded(d) | Event::DeviceChanged(d) => open || known(d) || shown.contains(&d.address),
        Event::DeviceRemoved { address, .. } => open || shown.contains(address),
        Event::AdapterAdded(_) | Event::AdapterChanged(_) | Event::AdapterRemoved { .. } => true,
        Event::PlayerChanged(_) | Event::PlayerRemoved { .. } => true,
    }
}

/// Signals that land within one frame of each other make one view.
const COALESCE: Duration = Duration::from_millis(16);

fn snapshot(state: &fs_bluez::state::State, open: bool) -> Bluetooth {
    let headsets = Some(headsets::read(state));
    let Some(adapter) = state.default_adapter() else { return Bluetooth { headsets, ..Bluetooth::default() } };
    let devices: Vec<_> =
        state.devices_of(&adapter.path).into_iter().filter(|d| open || known(d)).map(panel_device).collect();
    let connected = devices.iter().filter(|d| d.connected).map(|d| d.name.clone()).collect();
    Bluetooth {
        powered: Some(adapter.powered),
        connected,
        adapter: if adapter.alias.is_empty() { adapter.name.clone() } else { adapter.alias.clone() },
        state: state_word(adapter.state),
        discovering: adapter.discovering,
        devices,
        action: None,
        failure: None,
        headsets,
    }
}

/// What the earbuds backends cross their CLI lists with: the default
/// adapter's devices.
fn earbuds_devices(state: &fs_bluez::state::State) -> Vec<fs_devices::bluetooth::Device> {
    let Some(adapter) = state.default_adapter() else { return Vec::new() };
    let live: Vec<_> = state.devices_of(&adapter.path).into_iter().map(panel_device).collect();
    fs_devices::earbuds::bluetooth_devices(&live)
}

/// No system bus, or no BlueZ client on it: no adapter and no headsets.
fn absent(ctx: &Ctx) {
    ctx.publish(store::Diff::Bluetooth(Bluetooth { headsets: Some(Vec::new()), ..Bluetooth::default() }));
    super::earbuds::bluetooth(Vec::new());
}

/// What the panel asks of the service.
#[derive(Clone, Debug)]
pub enum Ask {
    Open(bool),
    Run(BtAddress, ActionKind),
    Rescan,
    Power(bool),
    /// A media source's transport, to its `MediaPlayer1`.
    Player(String, fs_bluez::PlayerCommand),
}

static ASKS: LazyLock<(Sender<Ask>, Receiver<Ask>)> = LazyLock::new(async_channel::unbounded);
static PANEL: AtomicBool = AtomicBool::new(false);

/// The panel's hold on discovery, taken while it is open.
pub fn panel(open: bool) {
    if PANEL.swap(open, Ordering::Relaxed) != open {
        let _ = ASKS.0.try_send(Ask::Open(open));
    }
}

/// Callable from any thread.
pub fn ask(a: Ask) {
    let _ = ASKS.0.try_send(a);
}

pub fn player(path: &str, command: fs_bluez::PlayerCommand) {
    ask(Ask::Player(path.to_owned(), command));
}

/// The devices' players as media sources. One whose position and state did
/// not move keeps the moment it was read at, so its clock runs on unbroken.
fn remotes(state: &fs_bluez::state::State, last: &[Remote]) -> Vec<Remote> {
    let now = Instant::now();
    state
        .players()
        .iter()
        .map(|p| {
            let r = Remote::from(p, state.device(&p.device).as_ref(), now);
            match last.iter().find(|l| l.path == r.path) {
                Some(l) if l.position == r.position && l.playing == r.playing => Remote { at: l.at, ..r },
                _ => r,
            }
        })
        .collect()
}

enum Done {
    /// A discovery attempt finished; true when BlueZ refused it.
    Armed(bool),
    Action(u64, Option<&'static str>),
}

enum Wake {
    State(fs_bluez::state::Event),
    Ask(Ask),
    Done(Done),
    Gone,
}

fn on_adapter<F: Future<Output = ()> + 'static>(bluez: &fs_bluez::Bluez, ctx: &Ctx, f: impl FnOnce(fs_bluez::Bluez, String) -> F) {
    let Some(a) = bluez.state().default_adapter().cloned() else { return };
    ctx.spawn(f(bluez.clone(), a.path));
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let Ok(conn) = crate::services::bus::private(&ctx, zbus::connection::Builder::system()).await else { return absent(&ctx) };
    let (bluez, events, monitor) = match fs_bluez::Bluez::connect(&conn).await {
        Ok(v) => v,
        Err(err) => {
            eprintln!("bluetooth: {err}");
            return absent(&ctx);
        }
    };
    ctx.spawn(monitor.run());
    BLUEZ.with_borrow_mut(|b| *b = Some(bluez.clone()));
    let (done_tx, done_rx) = async_channel::unbounded::<Done>();
    let mut open = PANEL.load(Ordering::Relaxed);
    bluez.watch_all(open);
    let mut flight = ActionInFlight::<(BtAddress, ActionKind), (BtAddress, &'static str)>::default();
    let mut arming = false;
    let mut refused = false;
    let mut refresh = true;
    let mut last: Option<Bluetooth> = None;
    let mut last_shown: Vec<BtAddress> = Vec::new();
    let mut last_buds: Option<Vec<fs_devices::bluetooth::Device>> = None;
    let mut last_remotes: Vec<Remote> = Vec::new();
    loop {
        // A signal no surface reads costs its own apply in fs-bluez and the
        // wake here, never a copy of the state.
        if refresh {
            let snap = bluez.state();
            if let Some(a) = snap.default_adapter()
                && open
                && a.state == AdapterState::Enabled
                && !a.discovering
                && !arming
                && !refused
            {
                arming = true;
                let (b, path, tx) = (bluez.clone(), a.path.clone(), done_tx.clone());
                ctx.spawn(async move {
                    let refused = match b.start_discovery(&path).await {
                        Err(err) if !err.is_in_progress() => {
                            eprintln!("bluetooth: discovery: {err}");
                            true
                        }
                        _ => false,
                    };
                    let _ = tx.send(Done::Armed(refused)).await;
                });
            }
            let mut view = snapshot(&snap, open);
            view.action = flight.action;
            view.failure = flight.failure;
            // Only what a surface would show differently reaches the UI.
            if last.as_ref() != Some(&view) {
                let shown: Vec<BtAddress> = view.devices.iter().map(|d| d.address).collect();
                ctx.publish(store::Diff::Bluetooth(view.clone()));
                last = Some(view);
                last_shown = shown;
            }
            let next = remotes(&snap, &last_remotes);
            if next != last_remotes {
                ctx.publish(store::Diff::Media(media::Diff::Remotes(next.clone())));
                last_remotes = next;
            }
            let buds = earbuds_devices(&snap);
            if last_buds.as_ref() != Some(&buds) {
                super::earbuds::bluetooth(buds.clone());
                last_buds = Some(buds);
            }
        }
        refresh = true;
        let wake = async { events.recv().await.map_or(Wake::Gone, Wake::State) }
            .or(async { ASKS.1.recv().await.map_or(Wake::Gone, Wake::Ask) })
            .or(async { done_rx.recv().await.map_or(Wake::Gone, Wake::Done) })
            .await;
        match wake {
            Wake::Gone => return,
            Wake::State(first) => {
                let mut seen = |e: &Event| {
                    refused &= !matches!(e, Event::AdapterAdded(_) | Event::AdapterChanged(_));
                    matters(e, open, &last_shown)
                };
                refresh = seen(&first);
                while let Ok(e) = events.try_recv() {
                    refresh |= seen(&e);
                }
                if refresh {
                    // A burst lands as one view, a frame's worth later.
                    async_io::Timer::after(COALESCE).await;
                    while events.try_recv().is_ok() {}
                }
            }
            Wake::Done(Done::Armed(turned_down)) => {
                arming = false;
                refused = turned_down;
            }
            Wake::Done(Done::Action(generation, result)) => {
                flight.end(generation, |(address, _)| result.map(|text| (address, text)));
            }
            Wake::Ask(Ask::Open(on)) => {
                open = on;
                bluez.watch_all(on);
                if !on && bluez.state().default_adapter().is_some_and(|a| a.discovering) {
                    on_adapter(&bluez, &ctx, |b, path| async move {
                        let _ = b.stop_discovery(&path).await;
                    });
                }
            }
            // StartDiscovery on a discovering adapter is a no-op, so a
            // rescan stops it and lets the loop above arm it again.
            Wake::Ask(Ask::Rescan) => on_adapter(&bluez, &ctx, |b, path| async move {
                let _ = b.stop_discovery(&path).await;
            }),
            Wake::Ask(Ask::Power(on)) => on_adapter(&bluez, &ctx, |b, path| async move {
                if let Err(err) = b.set_powered(&path, on).await {
                    eprintln!("bluetooth: {err}");
                }
            }),
            Wake::Ask(Ask::Player(path, command)) => {
                let b = bluez.clone();
                ctx.spawn(async move {
                    if let Err(err) = b.player_command(&path, command).await {
                        eprintln!("bluetooth: {command:?} on {path}: {err}");
                    }
                });
            }
            Wake::Ask(Ask::Run(address, kind)) => {
                let Some(generation) = flight.begin((address, kind)) else { continue };
                let (b, tx) = (bluez.clone(), done_tx.clone());
                ctx.spawn(async move {
                    let result = run_action(&b, address, kind)
                        .or(async {
                            async_io::Timer::after(ACTION_TIMEOUT).await;
                            Some("TIMED OUT")
                        })
                        .await;
                    let _ = tx.send(Done::Action(generation, result)).await;
                });
            }
        }
    }
}

/// One action against BlueZ: `None` once BlueZ has done it, else the row's
/// failure text. Pairing goes on to trust and connect once
/// `paired` reports true.
async fn run_action(b: &fs_bluez::Bluez, address: BtAddress, kind: ActionKind) -> Option<&'static str> {
    let out = match kind {
        ActionKind::Connect => b.connect_device(address).await,
        ActionKind::Disconnect => b.disconnect_device(address).await,
        ActionKind::Forget => b.remove(address).await,
        ActionKind::Pair => match pair(b, address).await {
            Ok(()) => match b.set_trusted(address, true).await {
                Ok(()) => b.connect_device(address).await,
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
        ActionKind::Trust | ActionKind::Untrust => b.set_trusted(address, kind == ActionKind::Trust).await,
    };
    let err = out.err()?;
    eprintln!("bluetooth: {address}: {err}");
    Some(match kind {
        ActionKind::Trust => "TRUST FAILED",
        ActionKind::Untrust => "UNTRUST FAILED",
        _ => "FAILED",
    })
}

/// BlueZ refuses an SSP pairing that wants a confirmation when no agent is
/// registered, so one answers for the length of the pairing. The panel has
/// no passkey prompt and the user picked this device, so its requests are
/// accepted; any other device's are refused.
async fn pair(b: &fs_bluez::Bluez, address: BtAddress) -> fs_bluez::Result<()> {
    use fs_bluez::agent::{AgentRequest, Capability};
    let session = match b.register_agent(Capability::DisplayYesNo).await {
        Ok(session) => Some(session),
        Err(e) => {
            eprintln!("bluetooth: agent: {e}");
            None
        }
    };
    let answer = async {
        let Some(session) = &session else { return future::pending().await };
        let mine = |device: &str| fs_bluez::is_device_path(device, address);
        while let Ok(request) = session.requests().recv().await {
            match request {
                AgentRequest::Confirm { device, reply, .. }
                | AgentRequest::Authorize { device, reply }
                | AgentRequest::AuthorizeService { device, reply, .. } => {
                    let _ = reply.try_send(mine(&device));
                }
                AgentRequest::PinCode { device, reply } => {
                    let _ = reply.try_send(mine(&device).then(|| "0000".to_owned()));
                }
                AgentRequest::Passkey { reply, .. } => {
                    let _ = reply.try_send(None);
                }
                AgentRequest::DisplayPinCode { .. } | AgentRequest::DisplayPasskey { .. } | AgentRequest::Cancel => {}
            }
        }
        future::pending().await
    };
    b.pair(address).or(answer).await
}

/// The bluetooth cell's right click: the default adapter's radio flipped.
pub fn toggle_power(ctx: &Ctx) {
    let Some(bluez) = BLUEZ.with_borrow(|b| b.clone()) else { return };
    ctx.spawn(async move {
        let Some(adapter) = bluez.state().default_adapter().cloned() else { return };
        if let Err(err) = bluez.set_powered(&adapter.path, !adapter.powered).await {
            eprintln!("bluetooth: {err}");
        }
    });
}

/// The device's `Trusted` property written as asked, with no panel row
/// marked in flight.
pub fn set_trust(ctx: &Ctx, address: BtAddress, trusted: bool) {
    let Some(bluez) = BLUEZ.with_borrow(|b| b.clone()) else { return };
    ctx.spawn(async move {
        if let Err(err) = bluez.set_trusted(address, trusted).await {
            eprintln!("bluetooth: {err}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(address: &str, paired: bool) -> BluezDevice {
        BluezDevice { address: address.parse().unwrap(), paired, ..BluezDevice::default() }
    }

    /// A discovery nobody asked for, with the panel shut: its strangers and
    /// their RSSI reach no surface.
    #[test]
    fn a_stranger_matters_only_while_the_panel_is_open() {
        let shown = ["AA:BB:CC:DD:EE:FF".parse().unwrap()];
        let stranger = device("11:22:33:44:55:66", false);
        let added = Event::DeviceAdded(stranger.clone());
        let removed = Event::DeviceRemoved { path: String::new(), address: stranger.address };
        assert!(!matters(&added, false, &shown) && !matters(&removed, false, &shown));
        assert!(matters(&added, true, &shown) && matters(&removed, true, &shown));
    }

    #[test]
    fn a_paired_or_shown_device_matters_shut() {
        let paired = device("AA:BB:CC:DD:EE:01", true);
        assert!(matters(&Event::DeviceChanged(paired), false, &[]));
        let shown = device("AA:BB:CC:DD:EE:02", false);
        assert!(matters(&Event::DeviceChanged(shown.clone()), false, &[shown.address]));
        assert!(matters(&Event::AdapterRemoved { path: String::new() }, false, &[]));
    }
}
