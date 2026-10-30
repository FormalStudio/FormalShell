use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use async_io::Timer;
use futures_lite::{Stream, StreamExt, future, stream};
use zbus::fdo::PropertiesProxy;
use zbus::message::Type;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream, proxy};

use crate::model::{
    ConnectionState, Device, DeviceKind, FailReason, Security, SignalStrength, Snapshot,
    WifiNetwork,
};
use crate::settings::{self, NewSettings, Profile, Settings};

const SERVICE: &str = "org.freedesktop.NetworkManager";
const DEVICE_INTERFACE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS_INTERFACE: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT_INTERFACE: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const IP4_INTERFACE: &str = "org.freedesktop.NetworkManager.IP4Config";

/// Longest a connect waits for the device to reach a final state. NetworkManager
/// gives up on a bad passphrase well inside this; DHCP is the slow case.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// A burst of NetworkManager signals (a scan adds a dozen access points)
/// becomes one refresh.
const QUIET: Duration = Duration::from_millis(150);

#[proxy(
    interface = "org.freedesktop.NetworkManager",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager",
    gen_blocking = false
)]
trait Manager {
    fn get_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn activate_connection(
        &self,
        connection: &ObjectPath<'_>,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath>;
    fn add_and_activate_connection(
        &self,
        connection: NewSettings,
        device: &ObjectPath<'_>,
        specific_object: &ObjectPath<'_>,
    ) -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)>;
    fn deactivate_connection(&self, active_connection: &ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(property)]
    fn wireless_enabled(&self) -> zbus::Result<bool>;
    #[zbus(property)]
    fn set_wireless_enabled(&self, enabled: bool) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings",
    default_service = "org.freedesktop.NetworkManager",
    default_path = "/org/freedesktop/NetworkManager/Settings",
    gen_blocking = false
)]
trait SettingsRoot {
    fn list_connections(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Settings.Connection",
    default_service = "org.freedesktop.NetworkManager",
    gen_blocking = false
)]
trait SavedConnection {
    fn get_settings(&self) -> zbus::Result<Settings>;
    fn update(&self, properties: Settings) -> zbus::Result<()>;
    fn delete(&self) -> zbus::Result<()>;
}

#[proxy(
    interface = "org.freedesktop.NetworkManager.Device.Wireless",
    default_service = "org.freedesktop.NetworkManager",
    gen_blocking = false
)]
trait Wireless {
    fn get_all_access_points(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn request_scan(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;
}

/// What a connect, forget or disconnect can come back with.
#[derive(Debug)]
pub enum ConnectError {
    /// A secret was supplied and NetworkManager (or the AP) rejected it.
    WrongPassword,
    /// No secret was supplied and the network needs one.
    SecretsRequired,
    /// No device sees the SSID and no saved profile names a device for it.
    SsidNotFound,
    NoWifiDevice,
    /// The device left activation for another reason.
    Failed(FailReason),
    Timeout,
    Bus(zbus::Error),
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongPassword => f.write_str("wrong password"),
            Self::SecretsRequired => f.write_str("a passphrase is required"),
            Self::SsidNotFound => f.write_str("network not found"),
            Self::NoWifiDevice => f.write_str("no managed wifi device"),
            Self::Failed(reason) => write!(f, "connection failed ({reason:?})"),
            Self::Timeout => f.write_str("timed out"),
            Self::Bus(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ConnectError {}

impl From<zbus::Error> for ConnectError {
    fn from(e: zbus::Error) -> Self {
        Self::Bus(e)
    }
}

enum Method {
    Saved,
    Psk(String),
    Eap { identity: String, password: String },
}

impl Method {
    fn supplies_secret(&self) -> bool {
        !matches!(self, Self::Saved)
    }
}

#[derive(Clone)]
pub struct NetworkManager {
    conn: Connection,
    manager: ManagerProxy<'static>,
    settings: SettingsRootProxy<'static>,
}

/// What a [`Snapshot`] is built from, kept between bursts so a signal that
/// carries its whole change is applied without a read.
struct Reading {
    wifi_enabled: bool,
    profiles: Vec<(OwnedObjectPath, Profile)>,
    devices: Vec<ReadDevice>,
}

enum ReadDevice {
    Plain(Device),
    Wifi(WifiDevice),
}

impl Reading {
    fn snapshot(&self) -> Snapshot {
        let mut devices = Vec::new();
        let mut networks = Vec::new();
        for device in &self.devices {
            match device {
                ReadDevice::Plain(d) => devices.push(d.clone()),
                ReadDevice::Wifi(wifi) => {
                    networks.extend(wifi_networks(wifi, &self.profiles));
                    devices.push(wifi.device.clone());
                }
            }
        }
        networks.sort_by(|a, b| {
            (b.connected(), b.visible, b.known, b.signal)
                .cmp(&(a.connected(), a.visible, a.known, a.signal))
                .then_with(|| a.ssid.cmp(&b.ssid))
        });
        Snapshot { wifi_enabled: self.wifi_enabled, devices, networks }
    }

    /// One signal applied in place. False when it says more than this
    /// reading can take from it, and NetworkManager has to be read again.
    fn apply(&mut self, msg: &zbus::Message) -> bool {
        let header = msg.header();
        let (Some(member), Some(path)) = (header.member(), header.path()) else { return false };
        if member.as_str() != "PropertiesChanged" {
            return false;
        }
        // org.freedesktop.DBus.Properties carries the interface in its body;
        // NetworkManager's own older signal of the same name is sent on it.
        let sent_on = header.interface().map(|i| i.as_str().to_owned()).unwrap_or_default();
        let (interface, changed) = if sent_on == "org.freedesktop.DBus.Properties" {
            match msg.body().deserialize::<(String, HashMap<String, OwnedValue>, Vec<String>)>() {
                Ok((interface, changed, invalidated)) if invalidated.is_empty() => (interface, changed),
                _ => return false,
            }
        } else {
            match msg.body().deserialize::<HashMap<String, OwnedValue>>() {
                Ok(changed) => (sent_on, changed),
                Err(_) => return false,
            }
        };
        for key in changed.keys() {
            match (interface.as_str(), key.as_str()) {
                (ACCESS_POINT_INTERFACE, "Strength" | "LastSeen") | (WIRELESS_INTERFACE, "LastScan") => {}
                _ => return false,
            }
        }
        let Some(strength) = get::<u8>(&changed, "Strength") else { return true };
        let path = path.as_str();
        let ap = self.devices.iter_mut().find_map(|d| match d {
            ReadDevice::Wifi(w) => w.access_points.iter_mut().find(|ap| ap.path.as_str() == path),
            ReadDevice::Plain(_) => None,
        });
        match ap {
            Some(ap) => {
                ap.signal = SignalStrength::from_percent(strength);
                true
            }
            None => false,
        }
    }
}

struct AccessPoint {
    path: OwnedObjectPath,
    ssid: String,
    signal: SignalStrength,
    security: Security,
}

struct WifiDevice {
    device: Device,
    access_points: Vec<AccessPoint>,
    active_ap: Option<OwnedObjectPath>,
    /// The saved profile the device's active connection runs, with its state.
    active: Option<(OwnedObjectPath, ConnectionState)>,
    /// Its IP4Config's addresses as `address/prefix`.
    addresses: Vec<String>,
}

impl NetworkManager {
    pub async fn connect(conn: &Connection) -> zbus::Result<Self> {
        let manager = ManagerProxy::builder(conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let settings = SettingsRootProxy::builder(conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        // Fail here, not on the first call, when NetworkManager is absent.
        manager.wireless_enabled().await?;
        Ok(Self {
            conn: conn.clone(),
            manager,
            settings,
        })
    }

    pub async fn snapshot(&self) -> zbus::Result<Snapshot> {
        Ok(self.read().await?.snapshot())
    }

    async fn read(&self) -> zbus::Result<Reading> {
        let wifi_enabled = self.manager.wireless_enabled().await?;
        let profiles = self.profiles().await?;
        let mut devices = Vec::new();
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path).await? else {
                continue;
            };
            if device.kind != DeviceKind::Wifi || !device.managed() {
                devices.push(ReadDevice::Plain(device));
                continue;
            }
            devices.push(ReadDevice::Wifi(self.read_wifi(device).await?));
        }
        Ok(Reading { wifi_enabled, profiles, devices })
    }

    /// The current snapshot, then a new one after every settled burst of
    /// NetworkManager signals that changed it. The match rule is installed
    /// before the first read, so no change slips between the two. A burst
    /// that only moves access points' strength (every scan sends one per
    /// access point) is applied from the signals themselves; anything else
    /// reads NetworkManager again, once per burst.
    ///
    /// Poll it for as long as it lives: zbus stops dispatching on the
    /// connection once the stream's queue fills, which stalls every call.
    pub async fn changes(&self) -> zbus::Result<impl Stream<Item = Snapshot> + use<>> {
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)?
            .build();
        let messages = MessageStream::for_match_rule(rule, &self.conn, None).await?;
        let reading = self.read().await?;
        let first = reading.snapshot();
        let state = (self.clone(), messages, reading, first.clone());
        let later = stream::unfold(state, |(nm, mut messages, mut reading, mut last)| async move {
            loop {
                let mut reread = !reading.apply(&messages.next().await?.ok()?);
                loop {
                    let burst = future::or(async { Some(messages.next().await) }, async {
                        Timer::after(QUIET).await;
                        None
                    })
                    .await;
                    match burst {
                        Some(Some(Ok(msg))) => reread |= !reading.apply(&msg),
                        Some(Some(Err(_))) => reread = true,
                        Some(None) => return None,
                        None => break,
                    }
                }
                // A read can race a device or access point disappearing; the
                // signal that follows brings the next attempt.
                if reread {
                    let Ok(now) = nm.read().await else {
                        continue;
                    };
                    reading = now;
                }
                let now = reading.snapshot();
                if now != last {
                    last = now.clone();
                    return Some((now, (nm, messages, reading, last)));
                }
            }
        });
        Ok(stream::once(first).chain(later))
    }

    pub async fn set_wifi_enabled(&self, enabled: bool) -> zbus::Result<()> {
        self.manager.set_wireless_enabled(enabled).await
    }

    /// Asks every managed wifi device to scan. NetworkManager refuses a scan
    /// right after another one, so a device that refuses is not an error
    /// unless every device did.
    pub async fn request_scan(&self) -> zbus::Result<()> {
        let mut last_error = None;
        let mut asked = 0;
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path.clone()).await? else {
                continue;
            };
            if device.kind != DeviceKind::Wifi || !device.managed() {
                continue;
            }
            let wireless = WirelessProxy::builder(&self.conn)
                .path(path)?
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            match wireless.request_scan(HashMap::new()).await {
                Ok(()) => asked += 1,
                Err(e) => last_error = Some(e),
            }
        }
        match last_error {
            Some(e) if asked == 0 => Err(e),
            _ => Ok(()),
        }
    }

    /// Connects with what NetworkManager has saved for the SSID. An open
    /// network needs nothing saved; a secured one with no profile answers
    /// [`ConnectError::SecretsRequired`].
    pub async fn connect_saved(&self, ssid: &str) -> Result<(), ConnectError> {
        self.activate(ssid, Method::Saved).await
    }

    pub async fn connect_psk(&self, ssid: &str, psk: &str) -> Result<(), ConnectError> {
        self.activate(ssid, Method::Psk(psk.to_owned())).await
    }

    /// 802.1x with PEAP and MSCHAPv2.
    pub async fn connect_eap(
        &self,
        ssid: &str,
        identity: &str,
        password: &str,
    ) -> Result<(), ConnectError> {
        self.activate(
            ssid,
            Method::Eap {
                identity: identity.to_owned(),
                password: password.to_owned(),
            },
        )
        .await
    }

    /// Takes the SSID's active connection down. `Ok(false)` when it was not
    /// active.
    pub async fn disconnect(&self, ssid: &str) -> zbus::Result<bool> {
        let profiles = self.profiles().await?;
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path.clone()).await? else {
                continue;
            };
            if device.kind != DeviceKind::Wifi {
                continue;
            }
            let props = self.props(&path, DEVICE_INTERFACE).await?;
            let Some(active) = get::<OwnedObjectPath>(&props, "ActiveConnection") else {
                continue;
            };
            if active.as_str() == "/" {
                continue;
            }
            let active_props = self.props(&active, ACTIVE_INTERFACE).await?;
            let saved = get::<OwnedObjectPath>(&active_props, "Connection");
            let runs_ssid = profiles
                .iter()
                .any(|(p, profile)| Some(p) == saved.as_ref() && profile.ssid == ssid);
            if runs_ssid {
                return match self.manager.deactivate_connection(&active).await {
                    Ok(()) => Ok(true),
                    // Already on its way down from an earlier call.
                    Err(zbus::Error::MethodError(name, ..))
                        if name.as_str()
                            == "org.freedesktop.NetworkManager.ConnectionNotActive" =>
                    {
                        Ok(false)
                    }
                    Err(e) => Err(e),
                };
            }
        }
        Ok(false)
    }

    /// Brings a wired device up on the profile NetworkManager picks for it
    /// (`/` asks for its best match).
    pub async fn connect_wired(&self, interface: &str) -> zbus::Result<()> {
        let none = ObjectPath::try_from("/")?;
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path.clone()).await? else {
                continue;
            };
            if device.kind == DeviceKind::Wired && device.interface == interface {
                self.manager.activate_connection(&none, &path, &none).await?;
                return Ok(());
            }
        }
        Ok(())
    }

    /// Takes a wired device's active connection down.
    pub async fn disconnect_wired(&self, interface: &str) -> zbus::Result<()> {
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path.clone()).await? else {
                continue;
            };
            if device.kind != DeviceKind::Wired || device.interface != interface {
                continue;
            }
            let props = self.props(&path, DEVICE_INTERFACE).await?;
            if let Some(active) = get::<OwnedObjectPath>(&props, "ActiveConnection").filter(|a| a.as_str() != "/") {
                self.manager.deactivate_connection(&active).await?;
            }
            return Ok(());
        }
        Ok(())
    }

    /// Deletes every saved profile for the SSID. `Ok(false)` when there was
    /// none.
    pub async fn forget(&self, ssid: &str) -> zbus::Result<bool> {
        let mut deleted = false;
        for (path, profile) in self.profiles().await? {
            if profile.ssid == ssid {
                self.saved(&path).await?.delete().await?;
                deleted = true;
            }
        }
        Ok(deleted)
    }

    async fn activate(&self, ssid: &str, method: Method) -> Result<(), ConnectError> {
        let profiles = self.profiles().await?;
        let saved = profiles.iter().find(|(_, p)| p.ssid == ssid);
        let (device, ap) = self.locate(ssid, saved.is_some()).await?;
        let security = ap
            .as_ref()
            .map(|a| a.security)
            .or(saved.map(|(_, p)| p.security()))
            .unwrap_or(Security::Unknown);
        let specific = ap
            .as_ref()
            .map_or_else(|| ObjectPath::from_static_str_unchecked("/"), |a| a.path.as_ref());

        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(SERVICE)?
            .path(device.path.clone())?
            .interface(DEVICE_INTERFACE)?
            .member("StateChanged")?
            .build();
        let mut states = MessageStream::for_match_rule(rule, &self.conn, None).await?;

        let mut created = None;
        match (&method, saved) {
            (Method::Saved, Some((path, _))) => {
                self.manager
                    .activate_connection(path, &device.path, &specific)
                    .await?;
            }
            (Method::Saved, None) if !security.is_secured() => {
                created = Some(self.add_and_activate(base_open(ssid), &device, &specific).await?);
            }
            (Method::Saved, None) => return Err(ConnectError::SecretsRequired),
            (Method::Psk(psk), Some((path, _))) => {
                let saved = self.saved(path).await?;
                saved.update(settings::with_psk(saved.get_settings().await?, psk)).await?;
                self.manager
                    .activate_connection(path, &device.path, &specific)
                    .await?;
            }
            (Method::Psk(psk), None) => {
                let new = settings::psk_profile(ssid, security, psk);
                created = Some(self.add_and_activate(new, &device, &specific).await?);
            }
            (Method::Eap { identity, password }, Some((path, _))) => {
                let new = settings::eap_profile(ssid, identity, password);
                self.saved(path).await?.update(to_owned_settings(new)?).await?;
                self.manager
                    .activate_connection(path, &device.path, &specific)
                    .await?;
            }
            (Method::Eap { identity, password }, None) => {
                let new = settings::eap_profile(ssid, identity, password);
                created = Some(self.add_and_activate(new, &device, &specific).await?);
            }
        }

        let outcome = wait_for_activation(&mut states, method.supplies_secret()).await;
        if outcome.is_err()
            && let Some(profile) = created
        {
            // A profile this call made for a network it could not join would
            // otherwise show as saved.
            if let Ok(saved) = self.saved(&profile).await {
                let _ = saved.delete().await;
            }
        }
        outcome
    }

    async fn add_and_activate(
        &self,
        new: NewSettings,
        device: &Device,
        specific: &ObjectPath<'_>,
    ) -> zbus::Result<OwnedObjectPath> {
        let (profile, _active) = self
            .manager
            .add_and_activate_connection(new, &device.path, specific)
            .await?;
        Ok(profile)
    }

    /// The device and the strongest access point to join `ssid` through.
    async fn locate(
        &self,
        ssid: &str,
        saved: bool,
    ) -> Result<(Device, Option<AccessPoint>), ConnectError> {
        let mut first = None;
        let mut best: Option<(Device, AccessPoint)> = None;
        for path in self.manager.get_devices().await? {
            let Some(device) = self.read_device(path).await? else {
                continue;
            };
            if device.kind != DeviceKind::Wifi || !device.managed() {
                continue;
            }
            let wifi = self.read_wifi(device).await?;
            for ap in wifi.access_points {
                if ap.ssid == ssid && best.as_ref().is_none_or(|(_, b)| ap.signal > b.signal) {
                    best = Some((wifi.device.clone(), ap));
                }
            }
            first.get_or_insert(wifi.device);
        }
        match (best, first) {
            (Some((device, ap)), _) => Ok((device, Some(ap))),
            (None, Some(device)) if saved => Ok((device, None)),
            (None, Some(_)) => Err(ConnectError::SsidNotFound),
            (None, None) => Err(ConnectError::NoWifiDevice),
        }
    }

    async fn profiles(&self) -> zbus::Result<Vec<(OwnedObjectPath, Profile)>> {
        let mut out = Vec::new();
        for path in self.settings.list_connections().await? {
            // A profile can be deleted between the list and the read.
            let Ok(saved) = self.saved(&path).await else {
                continue;
            };
            if let Ok(raw) = saved.get_settings().await
                && let Some(profile) = Profile::parse(&raw)
            {
                out.push((path, profile));
            }
        }
        Ok(out)
    }

    async fn saved(&self, path: &OwnedObjectPath) -> zbus::Result<SavedConnectionProxy<'static>> {
        SavedConnectionProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await
    }

    async fn props(
        &self,
        path: &OwnedObjectPath,
        interface: &'static str,
    ) -> zbus::Result<HashMap<String, OwnedValue>> {
        PropertiesProxy::builder(&self.conn)
            .destination(SERVICE)?
            .path(path.clone())?
            .build()
            .await?
            .get_all(InterfaceName::from_static_str_unchecked(interface))
            .await
            .map_err(Into::into)
    }

    async fn read_device(&self, path: OwnedObjectPath) -> zbus::Result<Option<Device>> {
        // A device can vanish between GetDevices and this read.
        let Ok(props) = self.props(&path, DEVICE_INTERFACE).await else {
            return Ok(None);
        };
        let kind = match get::<u32>(&props, "DeviceType") {
            Some(1) => DeviceKind::Wired,
            Some(2) => DeviceKind::Wifi,
            _ => return Ok(None),
        };
        let nm_state = get(&props, "State").unwrap_or(0);
        Ok(Some(Device {
            path,
            interface: get(&props, "Interface").unwrap_or_default(),
            kind,
            state: ConnectionState::from_device_state(nm_state),
            nm_state,
        }))
    }

    async fn read_wifi(&self, device: Device) -> zbus::Result<WifiDevice> {
        let wireless = WirelessProxy::builder(&self.conn)
            .path(device.path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let mut access_points = Vec::new();
        for path in wireless.get_all_access_points().await? {
            let Ok(props) = self.props(&path, ACCESS_POINT_INTERFACE).await else {
                continue;
            };
            let ssid = get::<Vec<u8>>(&props, "Ssid").unwrap_or_default();
            if ssid.is_empty() {
                continue;
            }
            access_points.push(AccessPoint {
                path,
                ssid: String::from_utf8_lossy(&ssid).into_owned(),
                signal: SignalStrength::from_percent(get(&props, "Strength").unwrap_or(0)),
                security: Security::from_ap_flags(
                    get(&props, "Flags").unwrap_or(0),
                    get(&props, "WpaFlags").unwrap_or(0),
                    get(&props, "RsnFlags").unwrap_or(0),
                ),
            });
        }
        let wireless_props = self.props(&device.path, WIRELESS_INTERFACE).await?;
        let device_props = self.props(&device.path, DEVICE_INTERFACE).await?;
        let mut active = None;
        if let Some(path) = get::<OwnedObjectPath>(&device_props, "ActiveConnection")
            && path.as_str() != "/"
            && let Ok(props) = self.props(&path, ACTIVE_INTERFACE).await
            && let Some(saved) = get::<OwnedObjectPath>(&props, "Connection")
        {
            let state = ConnectionState::from_active_state(get(&props, "State").unwrap_or(0));
            active = Some((saved, state));
        }
        let mut addresses = Vec::new();
        if let Some(path) = get::<OwnedObjectPath>(&device_props, "Ip4Config")
            && path.as_str() != "/"
            && let Ok(props) = self.props(&path, IP4_INTERFACE).await
        {
            addresses = address_data(&props);
        }
        Ok(WifiDevice {
            device,
            access_points,
            active_ap: get(&wireless_props, "ActiveAccessPoint").filter(|p: &OwnedObjectPath| p.as_str() != "/"),
            active,
            addresses,
        })
    }
}

fn base_open(ssid: &str) -> NewSettings {
    // An open profile is the base with no security section.
    let mut s = settings::psk_profile(ssid, Security::Open, "");
    s.remove("802-11-wireless-security");
    s
}

fn to_owned_settings(new: NewSettings) -> zbus::Result<Settings> {
    new.into_iter()
        .map(|(name, section)| {
            let section = section
                .into_iter()
                .map(|(k, v)| Ok((k, v.try_to_owned()?)))
                .collect::<zbus::Result<HashMap<_, _>>>()?;
            Ok((name, section))
        })
        .collect()
}

/// One row per SSID: the active access point stands for it, else the
/// strongest, and a saved SSID with no access point in range stays listed.
fn wifi_networks(wifi: &WifiDevice, profiles: &[(OwnedObjectPath, Profile)]) -> Vec<WifiNetwork> {
    let mut rows: Vec<WifiNetwork> = Vec::new();
    for ap in &wifi.access_points {
        let is_active = wifi.active_ap.as_ref() == Some(&ap.path);
        match rows.iter_mut().find(|r| r.ssid == ap.ssid) {
            Some(row) => {
                if !is_active && ap.signal > row.signal && !row_is_active(row, wifi) {
                    row.signal = ap.signal;
                    row.security = ap.security;
                }
            }
            None => rows.push(WifiNetwork {
                device: wifi.device.path.clone(),
                ssid: ap.ssid.clone(),
                signal: ap.signal,
                security: ap.security,
                known: false,
                visible: true,
                state: ConnectionState::Disconnected,
                addresses: Vec::new(),
            }),
        }
    }
    for (path, profile) in profiles {
        let running = wifi
            .active
            .as_ref()
            .filter(|(active_profile, _)| active_profile == path)
            .map(|(_, state)| *state);
        match rows.iter_mut().find(|r| r.ssid == profile.ssid) {
            Some(row) => {
                row.known = true;
                if let Some(state) = running {
                    row.state = state;
                }
            }
            None => rows.push(WifiNetwork {
                device: wifi.device.path.clone(),
                ssid: profile.ssid.clone(),
                signal: SignalStrength::default(),
                security: profile.security(),
                known: true,
                visible: false,
                state: running.unwrap_or(ConnectionState::Disconnected),
                addresses: Vec::new(),
            }),
        }
    }
    for row in rows.iter_mut().filter(|r| r.connected()) {
        row.addresses = wifi.addresses.clone();
    }
    rows
}

/// An IP4Config's `AddressData` (`aa{sv}`) as `address/prefix`, in its
/// order.
fn address_data(props: &HashMap<String, OwnedValue>) -> Vec<String> {
    fn plain<'a, 'b>(v: &'a Value<'b>) -> &'a Value<'b> {
        match v {
            Value::Value(inner) => plain(inner),
            v => v,
        }
    }
    let Some(Value::Array(items)) = props.get("AddressData").map(|v| plain(v)) else {
        return Vec::new();
    };
    items
        .inner()
        .iter()
        .filter_map(|item| {
            let Value::Dict(d) = plain(item) else { return None };
            let (mut address, mut prefix) = (None, None);
            for (k, v) in d.iter() {
                match (plain(k), plain(v)) {
                    (Value::Str(k), Value::Str(a)) if k.as_str() == "address" => address = Some(a.to_string()),
                    (Value::Str(k), Value::U32(p)) if k.as_str() == "prefix" => prefix = Some(*p),
                    _ => {}
                }
            }
            Some(format!("{}/{}", address?, prefix?))
        })
        .collect()
}

fn row_is_active(row: &WifiNetwork, wifi: &WifiDevice) -> bool {
    wifi.access_points
        .iter()
        .any(|ap| ap.ssid == row.ssid && wifi.active_ap.as_ref() == Some(&ap.path))
}

async fn wait_for_activation(
    states: &mut MessageStream,
    secret_supplied: bool,
) -> Result<(), ConnectError> {
    let mut deadline = Timer::after(CONNECT_TIMEOUT);
    let mut activating = false;
    loop {
        let next = future::or(async { Some(states.next().await) }, async {
            (&mut deadline).await;
            None
        })
        .await;
        let message = match next {
            None => return Err(ConnectError::Timeout),
            Some(None) => return Err(ConnectError::Bus(zbus::Error::Failure("bus closed".into()))),
            Some(Some(message)) => message?,
        };
        let Ok((new, _old, reason)) = message.body().deserialize::<(u32, u32, u32)>() else {
            continue;
        };
        match new {
            40..=90 => activating = true,
            100 => return Ok(()),
            120 => return Err(failure(reason, secret_supplied)),
            // Disconnected before any activation step is the previous
            // connection letting go, not this attempt failing.
            30 if activating => return Err(failure(reason, secret_supplied)),
            _ => {}
        }
    }
}

fn failure(reason: u32, secret_supplied: bool) -> ConnectError {
    match FailReason::from_wire(reason) {
        FailReason::NoSecrets if secret_supplied => ConnectError::WrongPassword,
        FailReason::NoSecrets => ConnectError::SecretsRequired,
        FailReason::SupplicantTimeout if secret_supplied => ConnectError::WrongPassword,
        FailReason::SsidNotFound => ConnectError::SsidNotFound,
        other => ConnectError::Failed(other),
    }
}

fn get<T>(props: &HashMap<String, OwnedValue>, name: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    props.get(name)?.try_clone().ok()?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(s: &str) -> OwnedObjectPath {
        OwnedObjectPath::try_from(s).unwrap()
    }

    fn device() -> Device {
        Device {
            path: path("/org/freedesktop/NetworkManager/Devices/3"),
            interface: "wlan0".into(),
            kind: DeviceKind::Wifi,
            state: ConnectionState::Disconnected,
            nm_state: 30,
        }
    }

    fn ap(n: u32, ssid: &str, strength: u8) -> AccessPoint {
        AccessPoint {
            path: path(&format!("/org/freedesktop/NetworkManager/AccessPoint/{n}")),
            ssid: ssid.into(),
            signal: SignalStrength::from_percent(strength),
            security: Security::Wpa2Psk,
        }
    }

    fn profile(ssid: &str) -> Profile {
        Profile {
            ssid: ssid.into(),
            key_mgmt: Some("wpa-psk".into()),
        }
    }

    fn changed(path: &str, interface: &str, props: &[(&str, Value<'_>)]) -> zbus::Message {
        let props: HashMap<&str, Value<'_>> = props.iter().cloned().collect();
        zbus::Message::signal(path, "org.freedesktop.DBus.Properties", "PropertiesChanged")
            .unwrap()
            .build(&(interface, props, Vec::<String>::new()))
            .unwrap()
    }

    #[test]
    fn a_strength_burst_applies_without_a_read_and_anything_else_asks_for_one() {
        let wifi = WifiDevice {
            device: device(),
            access_points: vec![ap(1, "home", 40), ap(2, "cafe", 55)],
            active_ap: None,
            active: None,
            addresses: Vec::new(),
        };
        let mut reading = Reading { wifi_enabled: true, profiles: Vec::new(), devices: vec![ReadDevice::Wifi(wifi)] };
        let ap1 = "/org/freedesktop/NetworkManager/AccessPoint/1";
        assert!(reading.apply(&changed(ap1, ACCESS_POINT_INTERFACE, &[("Strength", Value::U8(90)), ("LastSeen", Value::I32(7))])));
        assert!(reading.apply(&changed("/org/freedesktop/NetworkManager/Devices/3", WIRELESS_INTERFACE, &[("LastScan", Value::I64(9))])));
        let snap = reading.snapshot();
        assert_eq!((snap.networks[0].ssid.as_str(), snap.networks[0].signal.as_percent()), ("home", 90));
        assert!(!reading.apply(&changed(ap1, ACCESS_POINT_INTERFACE, &[("Ssid", Value::from(vec![b'x']))])));
        assert!(!reading.apply(&changed("/org/freedesktop/NetworkManager/AccessPoint/9", ACCESS_POINT_INTERFACE, &[("Strength", Value::U8(10))])));
        assert!(!reading.apply(&changed("/org/freedesktop/NetworkManager/Devices/3", DEVICE_INTERFACE, &[("State", Value::U32(100))])));
    }

    #[test]
    fn two_access_points_make_one_row_at_the_stronger_signal() {
        let wifi = WifiDevice {
            device: device(),
            access_points: vec![ap(1, "home", 40), ap(2, "home", 77), ap(3, "cafe", 55)],
            active_ap: None,
            active: None,
            addresses: Vec::new(),
        };
        let rows = wifi_networks(&wifi, &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].signal.as_percent(), 77);
    }

    #[test]
    fn the_active_access_point_wins_over_a_stronger_one() {
        let active = ap(1, "home", 40);
        let wifi = WifiDevice {
            device: device(),
            active_ap: Some(active.path.clone()),
            access_points: vec![active, ap(2, "home", 90)],
            active: None,
            addresses: Vec::new(),
        };
        let rows = wifi_networks(&wifi, &[]);
        assert_eq!(rows[0].signal.as_percent(), 40);
    }

    #[test]
    fn a_saved_network_out_of_range_stays_listed() {
        let saved = path("/org/freedesktop/NetworkManager/Settings/9");
        let wifi = WifiDevice {
            device: device(),
            access_points: vec![ap(1, "cafe", 55)],
            active_ap: None,
            active: Some((saved.clone(), ConnectionState::Connecting)),
            addresses: Vec::new(),
        };
        let rows = wifi_networks(&wifi, &[(saved, profile("home"))]);
        let home = rows.iter().find(|r| r.ssid == "home").unwrap();
        assert!(home.known && !home.visible);
        assert_eq!(home.signal.as_percent(), 0);
        assert!(home.state.is_changing());
        let cafe = rows.iter().find(|r| r.ssid == "cafe").unwrap();
        assert!(!cafe.known && cafe.visible);
    }

    #[test]
    fn saved_networks_out_of_range_sort_after_every_network_in_range() {
        let wifi = WifiDevice {
            device: device(),
            access_points: vec![ap(1, "cafe", 30), ap(2, "home", 20), ap(3, "street", 70)],
            active_ap: None,
            active: None,
            addresses: Vec::new(),
        };
        let profiles = vec![
            (path("/org/freedesktop/NetworkManager/Settings/1"), profile("home")),
            (path("/org/freedesktop/NetworkManager/Settings/2"), profile("hostel")),
        ];
        let reading = Reading { wifi_enabled: true, profiles, devices: vec![ReadDevice::Wifi(wifi)] };
        let order: Vec<String> = reading.snapshot().networks.into_iter().map(|n| n.ssid).collect();
        assert_eq!(order, ["home", "street", "cafe", "hostel"]);
    }

    #[test]
    fn a_known_visible_network_reports_its_active_state() {
        let saved = path("/org/freedesktop/NetworkManager/Settings/9");
        let wifi = WifiDevice {
            device: device(),
            access_points: vec![ap(1, "home", 60)],
            active_ap: None,
            active: Some((saved.clone(), ConnectionState::Connected)),
            addresses: vec!["10.0.0.5/24".into()],
        };
        let rows = wifi_networks(&WifiDevice { access_points: vec![ap(1, "home", 60), ap(2, "cafe", 70)], ..wifi }, &[(saved, profile("home"))]);
        assert!(rows[0].known && rows[0].connected());
        assert_eq!(rows[0].addresses, ["10.0.0.5/24"]);
        assert!(rows[1].addresses.is_empty());
    }

    #[test]
    fn address_data_reads_each_address_and_its_prefix() {
        let entry = |a: &str, p: u32| {
            let mut m: HashMap<String, Value<'static>> = HashMap::new();
            m.insert("address".into(), Value::Value(Box::new(Value::from(a.to_owned()))));
            m.insert("prefix".into(), Value::Value(Box::new(Value::U32(p))));
            m
        };
        let data = Value::from(vec![entry("192.168.4.20", 24), entry("10.1.2.3", 8)]);
        let props: HashMap<String, OwnedValue> = [("AddressData".to_owned(), OwnedValue::try_from(data).unwrap())].into();
        assert_eq!(address_data(&props), ["192.168.4.20/24", "10.1.2.3/8"]);
        assert!(address_data(&HashMap::new()).is_empty());
    }

    #[test]
    fn rejected_secrets_are_a_wrong_password_only_when_one_was_given() {
        assert!(matches!(failure(7, true), ConnectError::WrongPassword));
        assert!(matches!(failure(7, false), ConnectError::SecretsRequired));
        assert!(matches!(failure(11, true), ConnectError::WrongPassword));
        assert!(matches!(failure(53, true), ConnectError::SsidNotFound));
        assert!(matches!(
            failure(17, true),
            ConnectError::Failed(FailReason::DhcpFailed)
        ));
    }
}
