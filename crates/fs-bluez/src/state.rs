//! BlueZ object state, kept apart from the bus so the device logic runs on
//! plain property maps. `Bluez` feeds it `GetManagedObjects`,
//! `InterfacesAdded/Removed` and `PropertiesChanged`, and gets events back.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use fs_dbus::prop;
pub use fs_dbus::Props;
use fs_types::{BtAddress, Fraction};
use zbus::zvariant::OwnedObjectPath;

pub const ADAPTER_IFACE: &str = "org.bluez.Adapter1";
pub const DEVICE_IFACE: &str = "org.bluez.Device1";
pub const BATTERY_IFACE: &str = "org.bluez.Battery1";
pub const PLAYER_IFACE: &str = "org.bluez.MediaPlayer1";

/// The adapter state, read off `PowerState` when BlueZ
/// reports one (5.65+) and off `Powered` otherwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdapterState {
    #[default]
    Disabled,
    Enabled,
    Enabling,
    Disabling,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Adapter {
    pub path: String,
    pub address: BtAddress,
    /// BlueZ `Name`, the kernel's hostname-derived label.
    pub name: String,
    pub alias: String,
    pub powered: bool,
    pub state: AdapterState,
    pub discovering: bool,
    pub discoverable: bool,
    pub pairable: bool,
}

/// BlueZ's connection state for a device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceState {
    #[default]
    Disconnected,
    Connected,
    Disconnecting,
    Connecting,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub address: BtAddress,
    /// Object path of the `Device1`.
    pub path: String,
    /// Object path of the owning `Adapter1`.
    pub adapter: String,
    /// BlueZ's `Alias`, what the user sees.
    pub alias: String,
    /// BlueZ's own `Name`, the one the device reports.
    pub name: String,
    pub icon: String,
    pub rssi: Option<i16>,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub connected: bool,
    /// `Pair()` is running.
    pub pairing: bool,
    pub state: DeviceState,
    /// Battery1's level; none while the device reports no battery.
    pub battery: Option<Fraction>,
}

impl Device {
    /// The alias, else the name the device reports.
    pub fn label(&self) -> &str {
        if self.alias.is_empty() { &self.name } else { &self.alias }
    }
}

/// A remote's AVRCP target (`MediaPlayer1`, a phone or tablet streaming
/// to this machine): what the app on it is playing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Player {
    pub path: String,
    /// The owning `Device1`'s path.
    pub device: String,
    /// The app playing on the remote ("YouTube").
    pub name: String,
    /// `playing`, `stopped`, `paused`, `forward-seek`, `reverse-seek` or
    /// `error`.
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// `Track.Duration`, absent when the target sends none.
    pub duration: Option<Duration>,
    /// As of BlueZ's last `Position`; the remote's clock runs on from it
    /// while playing.
    pub position: Duration,
    /// A new track arrived with no `Position` of its own, so `position`
    /// still belongs to the last one.
    pub position_stale: bool,
}

impl Player {
    pub fn playing(&self) -> bool {
        self.status == "playing"
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    AdapterAdded(Adapter),
    AdapterChanged(Adapter),
    AdapterRemoved { path: String },
    DeviceAdded(Device),
    DeviceChanged(Device),
    DeviceRemoved { path: String, address: BtAddress },
    PlayerChanged(Player),
    PlayerRemoved { path: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Pairing,
    Connecting,
    Disconnecting,
}

#[derive(Clone, Debug, Default)]
struct Entry {
    device: Device,
    connecting: bool,
    disconnecting: bool,
}

impl Entry {
    fn view(&self) -> Device {
        let mut d = self.device.clone();
        d.state = match (d.connected, self.connecting, self.disconnecting) {
            (false, true, _) => DeviceState::Connecting,
            (true, _, true) => DeviceState::Disconnecting,
            (true, _, _) => DeviceState::Connected,
            _ => DeviceState::Disconnected,
        };
        d
    }
}

/// A player and when BlueZ last sent its `Position`.
#[derive(Clone, Debug, Default)]
struct PlayerEntry {
    player: Player,
    position_at: Option<Instant>,
}

/// How long a `Position` waits to be claimed by a `Track` arriving after it.
/// On a track change the target sends the new position and the new track
/// separately, and BlueZ fetches the track's attributes in a round trip
/// first, so the position usually lands before the track it belongs to.
const POSITION_CLAIM: Duration = Duration::from_secs(1);

/// Every adapter and device BlueZ currently exports. Empty means no
/// bluetoothd or no controller, never a stand-in.
#[derive(Clone, Debug, Default)]
pub struct State {
    adapters: BTreeMap<String, Adapter>,
    devices: BTreeMap<String, Entry>,
    players: BTreeMap<String, PlayerEntry>,
}

fn get_path(props: &Props, key: &str) -> Option<String> {
    prop::<OwnedObjectPath>(props, key).map(|p| p.to_string())
}

fn apply_adapter(a: &mut Adapter, props: &Props) {
    if let Some(v) = prop::<String>(props, "Address").and_then(|v| BtAddress::parse(&v)) {
        a.address = v;
    }
    if let Some(v) = prop::<String>(props, "Name") {
        a.name = v;
    }
    if let Some(v) = prop::<String>(props, "Alias") {
        a.alias = v;
    }
    if let Some(v) = prop::<bool>(props, "Powered") {
        a.powered = v;
    }
    if let Some(v) = prop::<bool>(props, "Discovering") {
        a.discovering = v;
    }
    if let Some(v) = prop::<bool>(props, "Discoverable") {
        a.discoverable = v;
    }
    if let Some(v) = prop::<bool>(props, "Pairable") {
        a.pairable = v;
    }
    let power_state = prop::<String>(props, "PowerState");
    a.state = match power_state.as_deref() {
        Some("on") => AdapterState::Enabled,
        Some("off") => AdapterState::Disabled,
        Some("off-enabling") => AdapterState::Enabling,
        Some("on-disabling") => AdapterState::Disabling,
        None if props.contains_key("Powered") => {
            if a.powered { AdapterState::Enabled } else { AdapterState::Disabled }
        }
        _ => a.state,
    };
}

fn apply_device(d: &mut Device, props: &Props, invalidated: &[String]) {
    if let Some(v) = prop::<String>(props, "Address").and_then(|v| BtAddress::parse(&v)) {
        d.address = v;
    }
    if let Some(v) = prop::<String>(props, "Name") {
        d.name = v;
    }
    if let Some(v) = prop::<String>(props, "Alias") {
        d.alias = v;
    }
    if let Some(v) = prop::<String>(props, "Icon") {
        d.icon = v;
    }
    if let Some(v) = prop::<bool>(props, "Paired") {
        d.paired = v;
    }
    if let Some(v) = prop::<bool>(props, "Bonded") {
        d.bonded = v;
    }
    if let Some(v) = prop::<bool>(props, "Trusted") {
        d.trusted = v;
    }
    if let Some(v) = prop::<bool>(props, "Connected") {
        d.connected = v;
    }
    if let Some(v) = prop::<i16>(props, "RSSI") {
        d.rssi = Some(v);
    }
    if let Some(v) = get_path(props, "Adapter") {
        d.adapter = v;
    }
    for key in invalidated {
        match key.as_str() {
            "RSSI" => d.rssi = None,
            "Icon" => d.icon.clear(),
            _ => {}
        }
    }
}

/// `Position` and `Track.Duration` are milliseconds on the wire. A new
/// `Track` replaces the whole of the last one, and takes a `Position` from
/// the same change or one at most `POSITION_CLAIM` before it as its own.
fn apply_player(e: &mut PlayerEntry, props: &Props, now: Instant) {
    let p = &mut e.player;
    if let Some(v) = get_path(props, "Device") {
        p.device = v;
    }
    if let Some(v) = prop::<String>(props, "Name") {
        p.name = v;
    }
    if let Some(v) = prop::<String>(props, "Status") {
        p.status = v;
    }
    if let Some(track) = prop::<Props>(props, "Track") {
        let title = prop::<String>(&track, "Title").unwrap_or_default();
        let artist = prop::<String>(&track, "Artist").unwrap_or_default();
        let album = prop::<String>(&track, "Album").unwrap_or_default();
        let duration = prop::<u32>(&track, "Duration").filter(|&ms| ms > 0).map(|ms| Duration::from_millis(ms.into()));
        if (&title, &artist, &album, duration) != (&p.title, &p.artist, &p.album, p.duration) {
            p.position_stale = !e.position_at.is_some_and(|at| now.saturating_duration_since(at) <= POSITION_CLAIM);
        }
        (p.title, p.artist, p.album, p.duration) = (title, artist, album, duration);
    }
    if let Some(v) = prop::<u32>(props, "Position") {
        p.position = Duration::from_millis(v.into());
        p.position_stale = false;
        e.position_at = Some(now);
    }
}

/// `Battery1.Percentage` is 0..100; the shell's models hold a 0..1 fraction.
fn apply_battery(d: &mut Device, props: &Props, invalidated: &[String]) {
    if let Some(p) = prop::<u8>(props, "Percentage") {
        d.battery = Some(Fraction::from_fraction(f64::from(p) / 100.0));
    }
    if invalidated.iter().any(|k| k == "Percentage") {
        d.battery = None;
    }
}

/// A device worth following while no Bluetooth panel or scan is open: one
/// the user has a bond with or a link to. Strangers a discovery turns up
/// wait for a wider subscription.
pub fn known(d: &Device) -> bool {
    d.paired || d.bonded || d.trusted || d.connected
}

/// The `Device1` properties a device's event carries: an RSSI move alone is
/// none of them.
const SHOWN: [&str; 9] = ["Address", "Name", "Alias", "Icon", "Paired", "Bonded", "Trusted", "Connected", "Adapter"];

fn changed(mut before: Device, e: &Entry) -> Vec<Event> {
    let after = e.view();
    before.rssi = after.rssi;
    if before == after { vec![] } else { vec![Event::DeviceChanged(after)] }
}

impl State {
    pub fn adapters(&self) -> Vec<Adapter> {
        self.adapters.values().cloned().collect()
    }

    pub fn devices(&self) -> Vec<Device> {
        self.devices.values().map(Entry::view).collect()
    }

    /// The first adapter by object path, which is the default adapter.
    pub fn default_adapter(&self) -> Option<&Adapter> {
        self.adapters.values().next()
    }

    pub fn players(&self) -> Vec<Player> {
        self.players.values().map(|e| e.player.clone()).collect()
    }

    pub fn device(&self, path: &str) -> Option<Device> {
        self.devices.get(path).map(Entry::view)
    }

    pub fn devices_of(&self, adapter: &str) -> Vec<Device> {
        self.devices.values().filter(|e| e.device.adapter == adapter).map(Entry::view).collect()
    }

    /// The object paths of every [`known`] device.
    pub fn known_paths(&self) -> BTreeSet<String> {
        self.devices.iter().filter(|(_, e)| known(&e.device)).map(|(p, _)| p.clone()).collect()
    }

    /// Scoped to one adapter.
    pub fn find_device(&self, adapter: &str, address: BtAddress) -> Option<Device> {
        self.devices.values().find(|e| e.device.adapter == adapter && e.device.address == address).map(Entry::view)
    }

    pub fn clear(&mut self) -> Vec<Event> {
        let mut out: Vec<Event> = self
            .devices
            .values()
            .map(|e| Event::DeviceRemoved { path: e.device.path.clone(), address: e.device.address })
            .collect();
        out.extend(self.adapters.keys().map(|p| Event::AdapterRemoved { path: p.clone() }));
        out.extend(self.players.keys().map(|p| Event::PlayerRemoved { path: p.clone() }));
        self.devices.clear();
        self.adapters.clear();
        self.players.clear();
        out
    }

    pub fn interface_added(&mut self, path: &str, iface: &str, props: &Props) -> Vec<Event> {
        match iface {
            ADAPTER_IFACE => {
                let known = self.adapters.contains_key(path);
                let a = self.adapters.entry(path.to_string()).or_insert_with(|| Adapter { path: path.to_string(), ..Default::default() });
                let before = a.clone();
                apply_adapter(a, props);
                if !known {
                    vec![Event::AdapterAdded(a.clone())]
                } else if before != *a {
                    vec![Event::AdapterChanged(a.clone())]
                } else {
                    vec![]
                }
            }
            DEVICE_IFACE => {
                let known = self.devices.contains_key(path);
                let e = self.devices.entry(path.to_string()).or_insert_with(|| {
                    let mut e = Entry::default();
                    e.device.path = path.to_string();
                    e
                });
                let before = e.view();
                apply_device(&mut e.device, props, &[]);
                if known { changed(before, e) } else { vec![Event::DeviceAdded(e.view())] }
            }
            BATTERY_IFACE => self.battery(path, props, &[]),
            PLAYER_IFACE => self.player(path, props),
            _ => vec![],
        }
    }

    fn player(&mut self, path: &str, props: &Props) -> Vec<Event> {
        let known = self.players.contains_key(path);
        let e = self.players.entry(path.to_string()).or_insert_with(|| PlayerEntry { player: Player { path: path.to_string(), ..Default::default() }, position_at: None });
        let before = e.player.clone();
        apply_player(e, props, Instant::now());
        if known && e.player == before { vec![] } else { vec![Event::PlayerChanged(e.player.clone())] }
    }

    /// One object's whole interface map, as `InterfacesAdded` and
    /// `GetManagedObjects` carry it. `Battery1` sorts before `Device1` in the
    /// map but needs the device to exist, so it goes last.
    pub fn interfaces_added(&mut self, path: &str, ifaces: &HashMap<String, Props>) -> Vec<Event> {
        let mut out = vec![];
        for (iface, props) in ifaces.iter().filter(|(i, _)| *i != BATTERY_IFACE) {
            out.extend(self.interface_added(path, iface, props));
        }
        if let Some(props) = ifaces.get(BATTERY_IFACE) {
            out.extend(self.interface_added(path, BATTERY_IFACE, props));
        }
        out
    }

    fn battery(&mut self, path: &str, props: &Props, invalidated: &[String]) -> Vec<Event> {
        let Some(e) = self.devices.get_mut(path) else { return vec![] };
        let before = e.view();
        apply_battery(&mut e.device, props, invalidated);
        changed(before, e)
    }

    pub fn properties_changed(&mut self, path: &str, iface: &str, props: &Props, invalidated: &[String]) -> Vec<Event> {
        match iface {
            ADAPTER_IFACE => {
                let Some(a) = self.adapters.get_mut(path) else { return vec![] };
                let before = a.clone();
                apply_adapter(a, props);
                if *a == before { vec![] } else { vec![Event::AdapterChanged(a.clone())] }
            }
            DEVICE_IFACE => {
                let Some(e) = self.devices.get_mut(path) else { return vec![] };
                // A discovery's stream of RSSI and advertising data changes
                // nothing anyone reads off a device: kept, and no event.
                if !props.keys().chain(invalidated).any(|k| SHOWN.contains(&k.as_str())) {
                    apply_device(&mut e.device, props, invalidated);
                    return vec![];
                }
                let before = e.view();
                apply_device(&mut e.device, props, invalidated);
                changed(before, e)
            }
            BATTERY_IFACE => self.battery(path, props, invalidated),
            PLAYER_IFACE if self.players.contains_key(path) => self.player(path, props),
            _ => vec![],
        }
    }

    pub fn interfaces_removed(&mut self, path: &str, ifaces: &[String]) -> Vec<Event> {
        let mut out = vec![];
        for iface in ifaces {
            match iface.as_str() {
                ADAPTER_IFACE => {
                    if self.adapters.remove(path).is_some() {
                        out.push(Event::AdapterRemoved { path: path.to_string() });
                    }
                }
                DEVICE_IFACE => {
                    if let Some(e) = self.devices.remove(path) {
                        out.push(Event::DeviceRemoved { path: path.to_string(), address: e.device.address });
                    }
                }
                BATTERY_IFACE => out.extend(self.battery(path, &Props::new(), &["Percentage".to_string()])),
                PLAYER_IFACE if self.players.remove(path).is_some() => out.push(Event::PlayerRemoved { path: path.to_string() }),
                _ => {}
            }
        }
        out
    }

    /// Swaps in a fresh enumeration and reports the difference, for a
    /// bluetoothd that came back after a restart. Calls in flight keep their
    /// activity flags.
    pub fn resync(&mut self, mut fresh: State) -> Vec<Event> {
        let mut out = vec![];
        for path in self.adapters.keys().filter(|p| !fresh.adapters.contains_key(*p)) {
            out.push(Event::AdapterRemoved { path: path.clone() });
        }
        for (path, e) in self.devices.iter().filter(|(p, _)| !fresh.devices.contains_key(*p)) {
            out.push(Event::DeviceRemoved { path: path.clone(), address: e.device.address });
        }
        for (path, e) in &mut fresh.devices {
            if let Some(old) = self.devices.get(path) {
                e.connecting = old.connecting;
                e.disconnecting = old.disconnecting;
                e.device.pairing = old.device.pairing;
            }
        }
        for (path, a) in &fresh.adapters {
            match self.adapters.get(path) {
                None => out.push(Event::AdapterAdded(a.clone())),
                Some(old) if old != a => out.push(Event::AdapterChanged(a.clone())),
                _ => {}
            }
        }
        for (path, e) in &fresh.devices {
            match self.devices.get(path).map(Entry::view) {
                None => out.push(Event::DeviceAdded(e.view())),
                Some(old) if old != e.view() => out.push(Event::DeviceChanged(e.view())),
                _ => {}
            }
        }
        for path in self.players.keys().filter(|p| !fresh.players.contains_key(*p)) {
            out.push(Event::PlayerRemoved { path: path.clone() });
        }
        for (path, e) in &fresh.players {
            if self.players.get(path).map(|old| &old.player) != Some(&e.player) {
                out.push(Event::PlayerChanged(e.player.clone()));
            }
        }
        *self = fresh;
        out
    }

    /// Local, call-in-flight state BlueZ does not publish: `Pair()` running
    /// is `pairing`, `Connect()` running is `Connecting`.
    pub fn set_activity(&mut self, path: &str, activity: Activity, on: bool) -> Vec<Event> {
        let Some(e) = self.devices.get_mut(path) else { return vec![] };
        let before = e.view();
        match activity {
            Activity::Pairing => e.device.pairing = on,
            Activity::Connecting => e.connecting = on,
            Activity::Disconnecting => e.disconnecting = on,
        }
        changed(before, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{ObjectPath, Value};

    fn props(items: Vec<(&str, Value<'_>)>) -> Props {
        items.into_iter().map(|(k, v)| (k.to_string(), v.try_to_owned().unwrap())).collect()
    }

    const HCI0: &str = "/org/bluez/hci0";
    const DEV: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF";

    fn seeded() -> State {
        let mut s = State::default();
        s.interface_added(
            HCI0,
            ADAPTER_IFACE,
            &props(vec![
                ("Address", "00:11:22:33:44:55".into()),
                ("Name", "laptop".into()),
                ("Alias", "laptop".into()),
                ("Powered", true.into()),
                ("PowerState", "on".into()),
                ("Discovering", false.into()),
                ("Discoverable", false.into()),
                ("Pairable", true.into()),
            ]),
        );
        s.interface_added(
            DEV,
            DEVICE_IFACE,
            &props(vec![
                ("Address", "AA:BB:CC:DD:EE:FF".into()),
                ("Name", "WH-1000XM5".into()),
                ("Alias", "Headphones".into()),
                ("Icon", "audio-headset".into()),
                ("Paired", true.into()),
                ("Bonded", true.into()),
                ("Trusted", false.into()),
                ("Connected", false.into()),
                ("Adapter", Value::ObjectPath(ObjectPath::try_from(HCI0).unwrap())),
            ]),
        );
        s
    }

    #[test]
    fn a_burst_of_rssi_changes_raises_no_event() {
        let mut s = seeded();
        let mut events = 0;
        for i in 0..700 {
            let rssi = props(vec![("RSSI", Value::I16(-40 - (i % 30) as i16)), ("TxPower", Value::I16(4))]);
            events += s.properties_changed(DEV, DEVICE_IFACE, &rssi, &[]).len();
        }
        events += s.properties_changed(DEV, DEVICE_IFACE, &Props::new(), &["RSSI".to_string()]).len();
        assert_eq!(events, 0);
        assert_eq!(s.device(DEV).unwrap().rssi, None);
        let connected = props(vec![("Connected", true.into()), ("RSSI", Value::I16(-50))]);
        assert_eq!(s.properties_changed(DEV, DEVICE_IFACE, &connected, &[]).len(), 1);
    }

    #[test]
    fn empty_state_is_the_no_adapter_state() {
        let s = State::default();
        assert!(s.default_adapter().is_none());
        assert!(s.devices().is_empty());
    }

    #[test]
    fn adapter_and_device_fields_parse() {
        let s = seeded();
        let a = s.default_adapter().unwrap();
        assert_eq!((a.powered, a.pairable, a.state), (true, true, AdapterState::Enabled));
        let d = s.device(DEV).unwrap();
        assert_eq!(d.alias, "Headphones");
        assert_eq!(d.name, "WH-1000XM5");
        assert_eq!(d.icon, "audio-headset");
        assert!(d.paired && d.bonded && !d.trusted);
        assert_eq!(d.adapter, HCI0);
        assert_eq!(d.rssi, None);
    }

    #[test]
    fn default_adapter_is_the_first_path() {
        let mut s = seeded();
        s.interface_added("/org/bluez/hci1", ADAPTER_IFACE, &props(vec![("Powered", false.into())]));
        assert_eq!(s.default_adapter().unwrap().path, HCI0);
        s.interfaces_removed(HCI0, &[ADAPTER_IFACE.to_string()]);
        assert_eq!(s.default_adapter().unwrap().path, "/org/bluez/hci1");
    }

    #[test]
    fn power_state_wins_over_powered() {
        let mut s = seeded();
        let ev = s.properties_changed(HCI0, ADAPTER_IFACE, &props(vec![("PowerState", "on-disabling".into()), ("Powered", true.into())]), &[]);
        assert!(matches!(&ev[..], [Event::AdapterChanged(a)] if a.state == AdapterState::Disabling));
        s.properties_changed(HCI0, ADAPTER_IFACE, &props(vec![("PowerState", "off-enabling".into())]), &[]);
        assert_eq!(s.default_adapter().unwrap().state, AdapterState::Enabling);
    }

    #[test]
    fn powered_alone_drives_state_on_old_bluez() {
        let mut s = State::default();
        s.interface_added(HCI0, ADAPTER_IFACE, &props(vec![("Powered", true.into())]));
        assert_eq!(s.default_adapter().unwrap().state, AdapterState::Enabled);
        s.properties_changed(HCI0, ADAPTER_IFACE, &props(vec![("Powered", false.into())]), &[]);
        assert_eq!(s.default_adapter().unwrap().state, AdapterState::Disabled);
    }

    #[test]
    fn unchanged_properties_emit_nothing() {
        let mut s = seeded();
        assert!(s.properties_changed(DEV, DEVICE_IFACE, &props(vec![("Paired", true.into())]), &[]).is_empty());
    }

    #[test]
    fn rssi_arrives_and_invalidates() {
        let mut s = seeded();
        s.properties_changed(DEV, DEVICE_IFACE, &props(vec![("RSSI", (-61i16).into())]), &[]);
        assert_eq!(s.device(DEV).unwrap().rssi, Some(-61));
        let ev = s.properties_changed(DEV, DEVICE_IFACE, &Props::new(), &["RSSI".to_string()]);
        assert_eq!(s.device(DEV).unwrap().rssi, None);
        assert!(ev.is_empty());
    }

    #[test]
    fn battery_is_a_fraction_and_goes_with_its_interface() {
        let mut s = seeded();
        let ev = s.interface_added(DEV, BATTERY_IFACE, &props(vec![("Percentage", 87u8.into())]));
        assert!(matches!(&ev[..], [Event::DeviceChanged(d)] if d.battery.is_some()));
        assert_eq!(s.device(DEV).unwrap().battery.map(Fraction::as_fraction), Some(0.87));
        s.interfaces_removed(DEV, &[BATTERY_IFACE.to_string()]);
        assert_eq!(s.device(DEV).unwrap().battery, None);
    }

    #[test]
    fn a_device_and_its_battery_arrive_in_one_object() {
        let mut s = State::default();
        let ifaces = HashMap::from([
            (BATTERY_IFACE.to_string(), props(vec![("Percentage", 40u8.into())])),
            (DEVICE_IFACE.to_string(), props(vec![("Address", "AA:BB:CC:DD:EE:FF".into())])),
        ]);
        s.interfaces_added(DEV, &ifaces);
        let d = s.device(DEV).unwrap();
        assert_eq!(d.battery.map(Fraction::as_fraction), Some(0.4));
    }

    #[test]
    fn battery_before_its_device_is_ignored() {
        let mut s = State::default();
        assert!(s.interface_added(DEV, BATTERY_IFACE, &props(vec![("Percentage", 50u8.into())])).is_empty());
    }

    #[test]
    fn activity_drives_pairing_and_connection_state() {
        let mut s = seeded();
        s.set_activity(DEV, Activity::Pairing, true);
        assert!(s.device(DEV).unwrap().pairing);
        s.set_activity(DEV, Activity::Pairing, false);
        s.set_activity(DEV, Activity::Connecting, true);
        assert_eq!(s.device(DEV).unwrap().state, DeviceState::Connecting);
        s.properties_changed(DEV, DEVICE_IFACE, &props(vec![("Connected", true.into())]), &[]);
        assert_eq!(s.device(DEV).unwrap().state, DeviceState::Connected);
        s.set_activity(DEV, Activity::Connecting, false);
        s.set_activity(DEV, Activity::Disconnecting, true);
        assert_eq!(s.device(DEV).unwrap().state, DeviceState::Disconnecting);
    }

    #[test]
    fn find_device_is_case_insensitive_and_adapter_scoped() {
        let s = seeded();
        assert!(s.find_device(HCI0, "aa:bb:cc:dd:ee:ff".parse().unwrap()).is_some());
        assert!(s.find_device("/org/bluez/hci1", "AA:BB:CC:DD:EE:FF".parse().unwrap()).is_none());
    }

    #[test]
    fn removal_reports_the_address() {
        let mut s = seeded();
        let ev = s.interfaces_removed(DEV, &[DEVICE_IFACE.to_string()]);
        assert_eq!(ev, vec![Event::DeviceRemoved { path: DEV.into(), address: "AA:BB:CC:DD:EE:FF".parse().unwrap() }]);
        assert!(s.devices().is_empty());
    }

    #[test]
    fn resync_reports_the_difference_and_keeps_activity() {
        let mut s = seeded();
        s.set_activity(DEV, Activity::Connecting, true);
        let mut fresh = seeded();
        fresh.properties_changed(DEV, DEVICE_IFACE, &props(vec![("Trusted", true.into())]), &[]);
        fresh.interfaces_removed(HCI0, &[ADAPTER_IFACE.to_string()]);
        let ev = s.resync(fresh);
        assert!(ev.iter().any(|e| matches!(e, Event::AdapterRemoved { .. })));
        assert!(ev.iter().any(|e| matches!(e, Event::DeviceChanged(d) if d.trusted)));
        assert_eq!(s.device(DEV).unwrap().state, DeviceState::Connecting);
    }

    const PLAYER: &str = "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF/avrcp/player0";

    fn track(title: &'static str, duration_ms: u32) -> Value<'static> {
        let mut t: HashMap<&'static str, Value<'static>> = HashMap::new();
        t.insert("Title", title.into());
        t.insert("Artist", "Marques Brownlee".into());
        t.insert("Duration", duration_ms.into());
        Value::from(t)
    }

    #[test]
    fn a_player_reads_its_track_in_milliseconds_and_goes_with_its_interface() {
        let mut s = seeded();
        let ev = s.interface_added(
            PLAYER,
            PLAYER_IFACE,
            &props(vec![
                ("Device", Value::ObjectPath(ObjectPath::try_from(DEV).unwrap())),
                ("Name", "YouTube".into()),
                ("Status", "playing".into()),
                ("Position", 359_686u32.into()),
                ("Track", track("OLED iPad Mini", 779_141)),
            ]),
        );
        let [Event::PlayerChanged(p)] = &ev[..] else { panic!("{ev:?}") };
        assert_eq!((p.device.as_str(), p.name.as_str(), p.title.as_str(), p.artist.as_str()), (DEV, "YouTube", "OLED iPad Mini", "Marques Brownlee"));
        assert!(p.playing() && p.album.is_empty());
        assert_eq!((p.position, p.duration), (Duration::from_millis(359_686), Some(Duration::from_millis(779_141))));

        let paused = props(vec![("Status", "paused".into())]);
        assert_eq!(s.properties_changed(PLAYER, PLAYER_IFACE, &paused, &[]).len(), 1);
        assert!(s.properties_changed(PLAYER, PLAYER_IFACE, &paused, &[]).is_empty());
        s.properties_changed(PLAYER, PLAYER_IFACE, &props(vec![("Track", track("Next", 0))]), &[]);
        let p = &s.players()[0];
        assert_eq!((p.title.as_str(), p.duration, p.playing()), ("Next", None, false));

        let ev = s.interfaces_removed(PLAYER, &[PLAYER_IFACE.to_string()]);
        assert_eq!(ev, vec![Event::PlayerRemoved { path: PLAYER.into() }]);
        assert!(s.players().is_empty());
        assert!(s.properties_changed(PLAYER, PLAYER_IFACE, &paused, &[]).is_empty());
    }

    fn playing_one() -> State {
        let mut s = seeded();
        let added = props(vec![
            ("Device", Value::ObjectPath(ObjectPath::try_from(DEV).unwrap())),
            ("Status", "playing".into()),
            ("Position", 200_000u32.into()),
            ("Track", track("One", 300_000)),
        ]);
        s.interface_added(PLAYER, PLAYER_IFACE, &added);
        assert!(!s.players()[0].position_stale);
        s
    }

    /// The last `Position` as if it came well before whatever comes next.
    fn age_position(s: &mut State) {
        let e = s.players.get_mut(PLAYER).unwrap();
        e.position_at = e.position_at.map(|at| at.checked_sub(POSITION_CLAIM * 30).unwrap());
    }

    fn change(s: &mut State, items: Vec<(&str, Value<'_>)>) {
        s.properties_changed(PLAYER, PLAYER_IFACE, &props(items), &[]);
    }

    #[test]
    fn a_track_after_its_position_holds_the_position_stale_until_bluez_reports_one() {
        let mut s = playing_one();
        age_position(&mut s);
        change(&mut s, vec![("Track", track("Two", 180_000))]);
        let p = &s.players()[0];
        assert!(p.position_stale && p.position == Duration::from_millis(200_000));
        change(&mut s, vec![("Position", 1_000u32.into())]);
        let p = &s.players()[0];
        assert!(!p.position_stale && p.position == Duration::from_millis(1_000));
    }

    #[test]
    fn a_track_claims_the_position_just_before_it() {
        let mut s = playing_one();
        age_position(&mut s);
        change(&mut s, vec![("Position", 0u32.into())]);
        change(&mut s, vec![("Track", track("Two", 180_000))]);
        let p = &s.players()[0];
        assert!(!p.position_stale && p.position == Duration::ZERO);
    }

    #[test]
    fn a_track_in_the_same_change_as_its_position_is_never_stale() {
        let mut s = playing_one();
        age_position(&mut s);
        change(&mut s, vec![("Track", track("Two", 90_000)), ("Position", 0u32.into())]);
        assert!(!s.players()[0].position_stale);
        age_position(&mut s);
        change(&mut s, vec![("Track", track("Two", 90_000))]);
        assert!(!s.players()[0].position_stale, "the same track again");
    }

    #[test]
    fn a_paused_track_change_with_no_later_position_lights_off_the_one_before_it() {
        let mut s = playing_one();
        change(&mut s, vec![("Status", "paused".into()), ("Position", 250_000u32.into())]);
        age_position(&mut s);
        change(&mut s, vec![("Position", 0u32.into())]);
        change(&mut s, vec![("Track", track("Two", 180_000))]);
        change(&mut s, vec![("Track", track("Two", 181_000))]);
        let p = &s.players()[0];
        assert!(!p.playing() && !p.position_stale && p.position == Duration::ZERO);
    }

    #[test]
    fn clear_empties_everything() {
        let mut s = seeded();
        let ev = s.clear();
        assert_eq!(ev.len(), 2);
        assert!(s.default_adapter().is_none() && s.devices().is_empty());
    }
}
