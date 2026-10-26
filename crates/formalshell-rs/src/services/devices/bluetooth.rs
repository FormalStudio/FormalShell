//! BlueZ through fs-bluez: the default adapter's state for the bar cell and
//! the Bluetooth panel, the device list the earbuds backends cross their
//! own lists with, and the panel's one action in flight.
//!
//! The panel's actions run here, one at a time, each settled by BlueZ's own
//! answer: a method that errors fails its row, one that never answers fails
//! it after [`ACTION_TIMEOUT`].
//! Discovery runs only while the panel is open on a powered adapter, and is
//! re-armed whenever BlueZ lets it lapse.

use std::cell::RefCell;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_bluez::state::AdapterState;
use futures_lite::{FutureExt, future};

use super::headsets::{self, Headset};
use crate::runtime::Ctx;
use crate::services::watch::Watch;
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
    pub action: Option<(String, ActionKind)>,
    /// The last action that failed: its device's address and why.
    pub failure: Option<(String, &'static str)>,
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

use fs_bluez::state::{known, matters};

/// Signals that land within one frame of each other make one view.
const COALESCE: Duration = Duration::from_millis(16);

fn snapshot(state: &fs_bluez::state::State, over: Option<&str>, open: bool) -> Bluetooth {
    let headsets = Some(headsets::read(Some(state), over));
    let Some(adapter) = state.default_adapter() else { return Bluetooth { headsets, ..Bluetooth::default() } };
    let devices: Vec<_> =
        state.devices_of(&adapter.path).into_iter().map(|d| d.info).filter(|d| open || known(d)).collect();
    let connected = devices
        .iter()
        .filter(|d| d.connected)
        .map(|d| if d.name.is_empty() { d.device_name.clone() } else { d.name.clone() })
        .collect();
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

fn smoke_override() -> Option<String> {
    headsets::smoke_text()
}

/// Signals each change of the file an `@path` override names.
fn smoke_changes(ctx: &Ctx) -> Option<Receiver<()>> {
    let mut watch = Watch::new(headsets::smoke_file()?).ok()?;
    let (tx, rx) = async_channel::unbounded();
    ctx.spawn(async move {
        loop {
            watch.changed().await;
            if tx.send(()).await.is_err() {
                return;
            }
        }
    });
    Some(rx)
}

fn absent(ctx: &Ctx) {
    let over = smoke_override();
    let devices = fs_devices::earbuds::bluetooth_devices(&[], over.as_deref());
    ctx.publish(store::Diff::Devices(super::Diff::Bluetooth(Bluetooth { headsets: Some(headsets::read(None, over.as_deref())), ..Bluetooth::default() })));
    super::earbuds::bluetooth(devices);
}

/// What the panel asks of the service.
#[derive(Clone, Debug)]
pub enum Ask {
    Open(bool),
    Run(String, ActionKind),
    Rescan,
    Power(bool),
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

enum Done {
    /// A discovery attempt finished, armed or refused.
    Armed,
    Action(Option<&'static str>),
}

enum Wake {
    State(fs_bluez::state::Event),
    Smoke,
    Ask(Ask),
    Done(Done),
    Gone,
}

fn on_adapter<F: Future<Output = ()> + 'static>(bluez: &fs_bluez::Bluez, ctx: &Ctx, f: impl FnOnce(fs_bluez::Bluez, String) -> F) {
    let Some(a) = bluez.state().default_adapter().cloned() else { return };
    ctx.spawn(f(bluez.clone(), a.path));
}

pub async fn run(ctx: Ctx) {
    let Ok(conn) = zbus::Connection::system().await else { return absent(&ctx) };
    let (bluez, events, monitor) = match fs_bluez::Bluez::connect(&conn).await {
        Ok(v) => v,
        Err(err) => {
            eprintln!("bluetooth: {err}");
            return absent(&ctx);
        }
    };
    ctx.spawn(monitor.run());
    BLUEZ.with_borrow_mut(|b| *b = Some(bluez.clone()));
    let changes = smoke_changes(&ctx);
    let (done_tx, done_rx) = async_channel::unbounded::<Done>();
    let mut open = PANEL.load(Ordering::Relaxed);
    bluez.watch_all(open);
    let mut action: Option<(String, ActionKind)> = None;
    let mut failure: Option<(String, &'static str)> = None;
    let mut arming = false;
    let mut refresh = true;
    let mut last: Option<Bluetooth> = None;
    let mut last_shown: Vec<String> = Vec::new();
    let mut last_buds: Option<Vec<fs_devices::bluetooth::Device>> = None;
    loop {
        // A signal no surface reads costs its own apply in fs-bluez and the
        // wake here, never a copy of the state.
        if refresh {
            let snap = bluez.state();
            if let Some(a) = snap.default_adapter()
                && open
                && a.powered
                && !a.discovering
                && !arming
            {
                arming = true;
                let (b, path, tx) = (bluez.clone(), a.path.clone(), done_tx.clone());
                ctx.spawn(async move {
                    // BlueZ refuses StartDiscovery while the adapter powers up.
                    if let Err(err) = b.start_discovery(&path).await {
                        eprintln!("bluetooth: discovery: {err}");
                        async_io::Timer::after(Duration::from_secs(1)).await;
                    }
                    let _ = tx.send(Done::Armed).await;
                });
            }
            let over = smoke_override();
            let mut view = snapshot(&snap, over.as_deref(), open);
            view.action = action.clone();
            view.failure = failure.clone();
            // Only what a surface would show differently reaches the UI.
            if last.as_ref() != Some(&view) {
                let shown: Vec<String> = view.devices.iter().map(|d| d.address.clone()).collect();
                ctx.publish(store::Diff::Devices(super::Diff::Bluetooth(view.clone())));
                last = Some(view);
                last_shown = shown;
            }
            let buds = bluez.earbuds_devices(over.as_deref());
            if last_buds.as_ref() != Some(&buds) {
                super::earbuds::bluetooth(buds.clone());
                last_buds = Some(buds);
            }
        }
        refresh = true;
        let wake = async { events.recv().await.map_or(Wake::Gone, Wake::State) }
            .or(async { ASKS.1.recv().await.map_or(Wake::Gone, Wake::Ask) })
            .or(async { done_rx.recv().await.map_or(Wake::Gone, Wake::Done) })
            .or(async {
                match &changes {
                    Some(rx) => rx.recv().await.map_or(Wake::Gone, |_| Wake::Smoke),
                    None => future::pending().await,
                }
            })
            .await;
        match wake {
            Wake::Gone => return,
            Wake::Smoke => {}
            Wake::State(first) => {
                refresh = matters(&first, open, &last_shown);
                while let Ok(e) = events.try_recv() {
                    refresh |= matters(&e, open, &last_shown);
                }
                if refresh {
                    // A burst lands as one view, a frame's worth later.
                    async_io::Timer::after(COALESCE).await;
                    while events.try_recv().is_ok() {}
                }
            }
            Wake::Done(Done::Armed) => arming = false,
            Wake::Done(Done::Action(result)) => {
                if let (Some((address, _)), Some(text)) = (action.take(), result) {
                    failure = Some((address, text));
                }
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
            Wake::Ask(Ask::Run(address, kind)) => {
                if action.is_some() {
                    continue;
                }
                action = Some((address.clone(), kind));
                failure = None;
                let (b, tx) = (bluez.clone(), done_tx.clone());
                ctx.spawn(async move {
                    let result = run_action(&b, &address, kind)
                        .or(async {
                            async_io::Timer::after(ACTION_TIMEOUT).await;
                            Some("TIMED OUT")
                        })
                        .await;
                    let _ = tx.send(Done::Action(result)).await;
                });
            }
        }
    }
}

/// One action against BlueZ: `None` once BlueZ has done it, else the row's
/// failure text. Pairing goes on to trust and connect once
/// `paired` reports true.
async fn run_action(b: &fs_bluez::Bluez, address: &str, kind: ActionKind) -> Option<&'static str> {
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
async fn pair(b: &fs_bluez::Bluez, address: &str) -> fs_bluez::Result<()> {
    use fs_bluez::agent::{AgentRequest, Capability};
    // A pairing cut off by ACTION_TIMEOUT never got to unregister.
    let _ = b.unregister_agent().await;
    let requests = match b.register_agent(Capability::DisplayYesNo).await {
        Ok(rx) => Some(rx),
        Err(e) => {
            eprintln!("bluetooth: agent: {e}");
            None
        }
    };
    let ours = format!("/dev_{}", address.replace(':', "_"));
    let answer = async {
        let mine = |device: &str| device.ends_with(&ours);
        while let Some(Ok(request)) = match &requests {
            Some(rx) => Some(rx.recv().await),
            None => None,
        } {
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
    let out = b.pair(address).or(answer).await;
    if requests.is_some()
        && let Err(e) = b.unregister_agent().await
    {
        eprintln!("bluetooth: agent: {e}");
    }
    out
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

/// Trusts the device at `address`: BlueZ otherwise asks an agent to
/// authorize every profile on each reconnect (the iPhone service, once a
/// pairing lands).
pub fn trust(ctx: &Ctx, address: &str) {
    set_trust(ctx, address, true);
}

/// The device's `Trusted` property written as asked, with no panel row
/// marked in flight.
pub fn set_trust(ctx: &Ctx, address: &str, trusted: bool) {
    let Some(bluez) = BLUEZ.with_borrow(|b| b.clone()) else { return };
    let address = address.to_owned();
    ctx.spawn(async move {
        if let Err(err) = bluez.set_trusted(&address, trusted).await {
            eprintln!("bluetooth: {err}");
        }
    });
}
