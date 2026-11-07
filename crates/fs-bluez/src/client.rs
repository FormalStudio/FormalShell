use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::Poll;

use async_channel::{Receiver, Sender};
use fs_devices::bluetooth;
use futures_lite::{StreamExt, future};
use zbus::fdo::{DBusProxy, ObjectManagerProxy};
use zbus::message::Type as MessageType;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};
use zbus::{Connection, MatchRule, Message, MessageStream};

use crate::agent::{AGENT_PATH, Agent, AgentRequest, Capability};
use crate::proxies::{Adapter1Proxy, AgentManager1Proxy, Device1Proxy, MediaPlayer1Proxy};
use crate::state::{ADAPTER_IFACE, Activity, BATTERY_IFACE, DEVICE_IFACE, Device, Event, PLAYER_IFACE, Props, State};

const BLUEZ: &str = "org.bluez";

#[derive(Debug)]
pub enum Error {
    NoAdapter,
    UnknownDevice(String),
    Bus(zbus::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoAdapter => f.write_str("no bluetooth adapter"),
            Error::UnknownDevice(a) => write!(f, "unknown device '{a}'"),
            Error::Bus(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<zbus::Error> for Error {
    fn from(e: zbus::Error) -> Self {
        Error::Bus(e)
    }
}

impl Error {
    /// BlueZ's error name, e.g. `org.bluez.Error.AuthenticationFailed`.
    pub fn bluez_name(&self) -> Option<&str> {
        match self {
            Error::Bus(zbus::Error::MethodError(name, _, _)) => Some(name.as_str()),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What a remote's `MediaPlayer1` is asked to do. AVRCP has no toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerCommand {
    Play,
    Pause,
    Next,
    Previous,
}

type Shared = Arc<Mutex<State>>;

fn lock(state: &Shared) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn publish(state: &Shared, events: &Sender<Event>, f: impl FnOnce(&mut State) -> Vec<Event>) {
    let out = f(&mut lock(state));
    for e in out {
        let _ = events.try_send(e);
    }
}

/// A BlueZ client on a caller-owned connection. Commands are plain async
/// calls; changes arrive as `Event`s from the receiver `connect` returns, once
/// the `Monitor` is being polled.
#[derive(Clone)]
pub struct Bluez {
    conn: Connection,
    state: Shared,
    events: Sender<Event>,
    watch: Sender<Watch>,
}

/// The signal pump. Spawn `run` on the service executor and keep it alive for
/// as long as state should track BlueZ.
pub struct Monitor {
    conn: Connection,
    state: Shared,
    events: Sender<Event>,
    signals: MessageStream,
    owner: zbus::fdo::NameOwnerChangedStream,
    watch: Receiver<Watch>,
}

async fn enumerate(conn: &Connection) -> Result<State> {
    let objects = ObjectManagerProxy::builder(conn).destination(BLUEZ)?.path("/")?.build().await?;
    let managed = match objects.get_managed_objects().await {
        Ok(m) => m,
        // No bluetoothd on the bus is the honest no-adapter state, not a failure.
        Err(zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_)) => return Ok(State::default()),
        Err(e) => return Err(zbus::Error::from(e).into()),
    };
    let mut state = State::default();
    for (path, ifaces) in managed {
        let ifaces: HashMap<String, Props> = ifaces.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        state.interfaces_added(path.as_str(), &ifaces);
    }
    Ok(state)
}

impl Bluez {
    /// Subscribes first, then enumerates, so no change falls between the two.
    pub async fn connect(conn: &Connection) -> Result<(Bluez, Receiver<Event>, Monitor)> {
        let rule = MatchRule::builder().msg_type(MessageType::Signal).sender(BLUEZ)?.build();
        let signals = MessageStream::for_match_rule(rule, conn, None).await?;
        let owner = DBusProxy::new(conn).await?.receive_name_owner_changed_with_args(&[(0, BLUEZ)]).await?;
        let fresh = enumerate(conn).await?;
        let state: Shared = Arc::new(Mutex::new(State::default()));
        let (events, rx) = async_channel::unbounded();
        publish(&state, &events, |s| s.resync(fresh));
        let (watch_tx, watch) = async_channel::unbounded();
        let bluez = Bluez { conn: conn.clone(), state: state.clone(), events: events.clone(), watch: watch_tx };
        Ok((bluez, rx, Monitor { conn: conn.clone(), state, events, signals, owner, watch }))
    }

    /// Every BlueZ signal (`true`), for a panel or scan view that shows
    /// strangers and RSSI, or only those about the adapter and the
    /// [`known`](crate::state::known) devices (`false`, the default). A
    /// discovery someone else runs sends hundreds of RSSI updates a minute,
    /// and narrowed, the bus never delivers them. Widening resyncs, since
    /// strangers changed unseen meanwhile.
    pub fn watch_all(&self, on: bool) {
        let _ = self.watch.try_send(Watch::All(on));
    }

    /// A copy of the current state; cheap enough to take per frame.
    pub fn state(&self) -> State {
        lock(&self.state).clone()
    }

    /// What the earbuds backends cross their CLI lists with: the default
    /// adapter's devices.
    pub fn earbuds_devices(&self) -> Vec<bluetooth::Device> {
        let state = lock(&self.state);
        let live: Vec<bluetooth::Device> = match state.default_adapter() {
            Some(a) => state.devices_of(&a.path).into_iter().map(|d| d.info).collect(),
            None => vec![],
        };
        fs_devices::earbuds::bluetooth_devices(&live)
    }

    fn default_adapter(&self) -> Result<String> {
        lock(&self.state).default_adapter().map(|a| a.path.clone()).ok_or(Error::NoAdapter)
    }

    /// The device a command acts on, its changes subscribed before the
    /// command goes out: under the narrow subscription a stranger being
    /// paired would otherwise answer into nothing.
    async fn resolve(&self, address: &str) -> Result<Device> {
        let adapter = self.default_adapter()?;
        let d = lock(&self.state).find_device(&adapter, address).ok_or_else(|| Error::UnknownDevice(address.to_string()))?;
        let (ack, acked) = async_channel::bounded(1);
        if self.watch.try_send(Watch::Path(d.info.dbus_path.clone(), ack)).is_ok() {
            let _ = acked.recv().await;
        }
        Ok(d)
    }

    async fn adapter(&self, path: &str) -> Result<Adapter1Proxy<'static>> {
        Ok(Adapter1Proxy::builder(&self.conn).path(path.to_string())?.build().await?)
    }

    async fn device_proxy(&self, path: &str) -> Result<Device1Proxy<'static>> {
        Ok(Device1Proxy::builder(&self.conn).path(path.to_string())?.build().await?)
    }

    fn activity(&self, path: &str, activity: Activity, on: bool) {
        publish(&self.state, &self.events, |s| s.set_activity(path, activity, on));
    }

    pub async fn set_powered(&self, adapter: &str, on: bool) -> Result<()> {
        Ok(self.adapter(adapter).await?.set_powered(on).await?)
    }

    pub async fn set_discoverable(&self, adapter: &str, on: bool) -> Result<()> {
        Ok(self.adapter(adapter).await?.set_discoverable(on).await?)
    }

    pub async fn set_pairable(&self, adapter: &str, on: bool) -> Result<()> {
        Ok(self.adapter(adapter).await?.set_pairable(on).await?)
    }

    /// Already discovering for this client is success: the panel re-arms it
    /// on a timer.
    pub async fn start_discovery(&self, adapter: &str) -> Result<()> {
        match self.adapter(adapter).await?.start_discovery().await {
            Err(zbus::Error::MethodError(name, _, _)) if name.as_str() == "org.bluez.Error.InProgress" => Ok(()),
            other => Ok(other?),
        }
    }

    pub async fn stop_discovery(&self, adapter: &str) -> Result<()> {
        Ok(self.adapter(adapter).await?.stop_discovery().await?)
    }

    pub async fn connect_device(&self, address: &str) -> Result<()> {
        let d = self.resolve(address).await?;
        let path = d.info.dbus_path.clone();
        let proxy = self.device_proxy(&path).await?;
        self.activity(&path, Activity::Connecting, true);
        let out = proxy.connect().await;
        self.activity(&path, Activity::Connecting, false);
        Ok(out?)
    }

    pub async fn disconnect_device(&self, address: &str) -> Result<()> {
        let d = self.resolve(address).await?;
        let path = d.info.dbus_path.clone();
        let proxy = self.device_proxy(&path).await?;
        self.activity(&path, Activity::Disconnecting, true);
        let out = proxy.disconnect().await;
        self.activity(&path, Activity::Disconnecting, false);
        Ok(out?)
    }

    /// Leaves trusting and connecting to the caller, which decides when
    /// pairing counts as done.
    pub async fn pair(&self, address: &str) -> Result<()> {
        let d = self.resolve(address).await?;
        let path = d.info.dbus_path.clone();
        let proxy = self.device_proxy(&path).await?;
        self.activity(&path, Activity::Pairing, true);
        let out = proxy.pair().await;
        self.activity(&path, Activity::Pairing, false);
        Ok(out?)
    }

    pub async fn cancel_pairing(&self, address: &str) -> Result<()> {
        let d = self.resolve(address).await?;
        Ok(self.device_proxy(&d.info.dbus_path).await?.cancel_pairing().await?)
    }

    pub async fn set_trusted(&self, address: &str, trusted: bool) -> Result<()> {
        let d = self.resolve(address).await?;
        Ok(self.device_proxy(&d.info.dbus_path).await?.set_trusted(trusted).await?)
    }

    /// `Adapter1.RemoveDevice`: unpairs and forgets.
    pub async fn remove(&self, address: &str) -> Result<()> {
        let d = self.resolve(address).await?;
        let path = ObjectPath::try_from(d.info.dbus_path.as_str()).map_err(zbus::Error::from)?;
        Ok(self.adapter(&d.adapter).await?.remove_device(&path).await?)
    }

    pub async fn player_command(&self, path: &str, command: PlayerCommand) -> Result<()> {
        let p = MediaPlayer1Proxy::builder(&self.conn).path(path.to_string())?.build().await?;
        Ok(match command {
            PlayerCommand::Play => p.play().await,
            PlayerCommand::Pause => p.pause().await,
            PlayerCommand::Next => p.next().await,
            PlayerCommand::Previous => p.previous().await,
        }?)
    }

    /// Serves an `Agent1` and makes it BlueZ's default. Requests arrive on
    /// the returned receiver until `unregister_agent`.
    pub async fn register_agent(&self, capability: Capability) -> Result<Receiver<AgentRequest>> {
        let (agent, rx) = Agent::channel();
        self.conn.object_server().at(AGENT_PATH, agent).await?;
        let path = ObjectPath::from_static_str_unchecked(AGENT_PATH);
        let manager = AgentManager1Proxy::new(&self.conn).await?;
        let registered = async {
            manager.register_agent(&path, capability.as_str()).await?;
            manager.request_default_agent(&path).await
        }
        .await;
        if let Err(e) = registered {
            self.conn.object_server().remove::<Agent, _>(AGENT_PATH).await?;
            return Err(e.into());
        }
        Ok(rx)
    }

    pub async fn unregister_agent(&self) -> Result<()> {
        let path = ObjectPath::from_static_str_unchecked(AGENT_PATH);
        let out = AgentManager1Proxy::new(&self.conn).await?.unregister_agent(&path).await;
        self.conn.object_server().remove::<Agent, _>(AGENT_PATH).await?;
        Ok(out?)
    }
}

enum Item {
    Signal(Message),
    Owner(bool),
    Watch(Watch),
    Gone,
}

fn handle_signal(state: &Shared, events: &Sender<Event>, msg: &Message) {
    let header = msg.header();
    let (Some(interface), Some(member)) = (header.interface(), header.member()) else { return };
    let body = msg.body();
    match (interface.as_str(), member.as_str()) {
        ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded") => {
            if let Ok((path, ifaces)) = body.deserialize::<(OwnedObjectPath, HashMap<String, Props>)>() {
                publish(state, events, |s| s.interfaces_added(path.as_str(), &ifaces));
            }
        }
        ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved") => {
            if let Ok((path, ifaces)) = body.deserialize::<(OwnedObjectPath, Vec<String>)>() {
                publish(state, events, |s| s.interfaces_removed(path.as_str(), &ifaces));
            }
        }
        ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
            let Some(path) = header.path() else { return };
            if let Ok((iface, changed, invalidated)) = body.deserialize::<(String, Props, Vec<String>)>() {
                publish(state, events, |s| s.properties_changed(path.as_str(), &iface, &changed, &invalidated));
            }
        }
        _ => {}
    }
}

/// The streams for one subscription: everything from BlueZ, or the object
/// manager's adds and removes, adapter, battery and media player changes,
/// and `Device1` changes on the known paths alone.
async fn subscribe(conn: &Connection, wide: bool, known: &BTreeSet<String>) -> zbus::Result<Vec<Pin<Box<MessageStream>>>> {
    let base = || MatchRule::builder().msg_type(MessageType::Signal).sender(BLUEZ);
    let changed = || base()?.interface("org.freedesktop.DBus.Properties")?.member("PropertiesChanged");
    let mut rules = Vec::new();
    if wide {
        rules.push(base()?.build());
    } else {
        rules.push(base()?.interface("org.freedesktop.DBus.ObjectManager")?.build());
        rules.push(changed()?.arg(0, ADAPTER_IFACE)?.build());
        rules.push(changed()?.arg(0, BATTERY_IFACE)?.build());
        rules.push(changed()?.arg(0, PLAYER_IFACE)?.build());
        for path in known {
            rules.push(changed()?.path(path.as_str())?.arg(0, DEVICE_IFACE)?.build());
        }
    }
    let mut out = Vec::with_capacity(rules.len());
    for rule in rules {
        out.push(Box::pin(MessageStream::for_match_rule(rule, conn, None).await?));
    }
    Ok(out)
}

/// Swaps in a new subscription once it is in place. What the old one
/// still holds is applied before it goes: a signal can land in both, and
/// applying one twice changes nothing the second time.
async fn resubscribe(
    conn: &Connection,
    state: &Shared,
    events: &Sender<Event>,
    streams: &mut Vec<Pin<Box<MessageStream>>>,
    wide: bool,
    paths: &BTreeSet<String>,
) {
    let Ok(next) = subscribe(conn, wide, paths).await else { return };
    for mut s in std::mem::replace(streams, next) {
        while let Some(Some(msg)) = future::poll_once(s.next()).await {
            if let Ok(msg) = msg {
                handle_signal(state, events, &msg);
            }
        }
    }
}

/// What the client asks of the monitor's subscription.
enum Watch {
    All(bool),
    /// Follow one device's changes; acked once its rule is in place.
    Path(String, Sender<()>),
}

impl Monitor {
    pub async fn run(self) {
        let Monitor { conn, state, events, signals, owner, watch } = self;
        let mut wide = false;
        // Paths a command asked to follow, kept for the monitor's life.
        let mut followed = BTreeSet::new();
        let mut paths = lock(&state).known_paths();
        // `connect` subscribed to everything so nothing fell before its
        // enumerate; the narrow set replaces it once it is in place.
        let mut streams = vec![Box::pin(signals)];
        resubscribe(&conn, &state, &events, &mut streams, false, &paths).await;
        let mut owner = Box::pin(owner);
        let mut watch = Box::pin(watch);
        loop {
            let item = future::poll_fn(|cx| {
                if let Poll::Ready(o) = owner.as_mut().poll_next(cx) {
                    return Poll::Ready(o.map_or(Item::Gone, |s| Item::Owner(s.args().is_ok_and(|a| a.new_owner.is_some()))));
                }
                if let Poll::Ready(w) = watch.as_mut().poll_next(cx) {
                    return Poll::Ready(w.map_or(Item::Gone, Item::Watch));
                }
                for s in &mut streams {
                    loop {
                        match s.as_mut().poll_next(cx) {
                            Poll::Ready(Some(Ok(msg))) => return Poll::Ready(Item::Signal(msg)),
                            Poll::Ready(Some(Err(_))) => continue,
                            Poll::Ready(None) => return Poll::Ready(Item::Gone),
                            Poll::Pending => break,
                        }
                    }
                }
                Poll::Pending
            })
            .await;
            let resync = match item {
                Item::Gone => return,
                Item::Signal(msg) => {
                    handle_signal(&state, &events, &msg);
                    false
                }
                Item::Owner(false) => {
                    publish(&state, &events, State::clear);
                    false
                }
                Item::Owner(true) => true,
                Item::Watch(Watch::All(on)) if on == wide => false,
                Item::Watch(Watch::All(on)) => {
                    wide = on;
                    resubscribe(&conn, &state, &events, &mut streams, wide, &paths).await;
                    wide
                }
                Item::Watch(Watch::Path(path, ack)) => {
                    followed.insert(path);
                    if !wide {
                        paths.extend(followed.iter().cloned());
                        resubscribe(&conn, &state, &events, &mut streams, false, &paths).await;
                    }
                    let _ = ack.try_send(());
                    false
                }
            };
            if resync && let Ok(fresh) = enumerate(&conn).await {
                publish(&state, &events, |s| s.resync(fresh));
            }
            if !wide {
                let mut now = lock(&state).known_paths();
                now.extend(followed.iter().cloned());
                if now != paths {
                    paths = now;
                    resubscribe(&conn, &state, &events, &mut streams, false, &paths).await;
                }
            }
        }
    }
}
