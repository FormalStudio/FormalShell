use fs_dbus::{Props, Watched, prop};
use futures_lite::Stream;
use zbus::fdo::PropertiesProxy;
use zbus::message::Type;
use zbus::names::InterfaceName;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;
use zbus::{Connection, MatchRule, Message, proxy};

use crate::device::Device;

const SERVICE: &str = "org.freedesktop.UPower";
const ROOT_PATH: &str = "/org/freedesktop/UPower";
const DEVICE_INTERFACE: &str = "org.freedesktop.UPower.Device";

#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower",
    gen_blocking = false
)]
trait Root {
    fn enumerate_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    fn get_display_device(&self) -> zbus::Result<OwnedObjectPath>;

    #[zbus(property)]
    fn on_battery(&self) -> zbus::Result<bool>;
}

/// Everything UPower reports, read once.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Every enumerated device, line power and peripherals included.
    pub devices: Vec<Device>,
    /// UPower's aggregate. On a machine with no battery it is present in the
    /// answer but `is_present` is false and `is_laptop_battery()` fails.
    pub display: Device,
    pub on_battery: bool,
}

#[derive(Clone)]
pub struct UPower {
    conn: Connection,
    root: RootProxy<'static>,
}

impl UPower {
    pub async fn connect(conn: &Connection) -> zbus::Result<Self> {
        let root = RootProxy::builder(conn).cache_properties(CacheProperties::No).build().await?;
        Ok(Self { conn: conn.clone(), root })
    }

    pub async fn snapshot(&self) -> zbus::Result<Snapshot> {
        Ok(self.read().await?.snapshot())
    }

    /// The snapshot, then a new one after every change to it: devices added
    /// and removed, `OnBattery`, and any device's properties, which are
    /// applied from the signal's own payload with no call back. None while
    /// upowerd is not on the bus, and a whole read again when it returns.
    pub fn changes(&self) -> impl Stream<Item = Option<Snapshot>> + use<> {
        fs_dbus::watch(self.conn.clone(), Watch(self.clone()))
    }

    async fn read(&self) -> zbus::Result<Reading> {
        let mut devices = Vec::new();
        for path in self.root.enumerate_devices().await? {
            // A device can vanish between the enumerate and the read.
            if let Ok(props) = self.props(&path).await {
                devices.push((path, props));
            }
        }
        let display = self.root.get_display_device().await?;
        let display = (display.clone(), self.props(&display).await?);
        Ok(Reading { devices, display, on_battery: self.root.on_battery().await? })
    }

    async fn props(&self, path: &OwnedObjectPath) -> zbus::Result<Props> {
        let props = PropertiesProxy::builder(&self.conn).destination(SERVICE)?.path(path.clone())?.build().await?;
        Ok(props.get_all(InterfaceName::from_static_str_unchecked(DEVICE_INTERFACE)).await?)
    }
}

/// Every device's properties as last read or changed.
struct Reading {
    devices: Vec<(OwnedObjectPath, Props)>,
    display: (OwnedObjectPath, Props),
    on_battery: bool,
}

impl Reading {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            devices: self.devices.iter().map(|(path, props)| Device::from_properties(path.clone(), props)).collect(),
            display: Device::from_properties(self.display.0.clone(), &self.display.1),
            on_battery: self.on_battery,
        }
    }

    /// One signal applied in place; false when UPower has to be read again
    /// (a device added, or a property invalidated rather than sent).
    fn apply(&mut self, message: &Message) -> bool {
        let header = message.header();
        let (Some(interface), Some(member), Some(path)) = (header.interface(), header.member(), header.path()) else {
            return true;
        };
        match (interface.as_str(), member.as_str()) {
            ("org.freedesktop.UPower", "DeviceAdded") => false,
            ("org.freedesktop.UPower", "DeviceRemoved") => match message.body().deserialize::<OwnedObjectPath>() {
                Ok(removed) => {
                    self.devices.retain(|(path, _)| *path != removed);
                    true
                }
                Err(_) => false,
            },
            ("org.freedesktop.DBus.Properties", "PropertiesChanged") => {
                let Ok((sent_on, changed, invalidated)) = message.body().deserialize::<(String, Props, Vec<String>)>() else {
                    return false;
                };
                if !invalidated.is_empty() {
                    return false;
                }
                if path.as_str() == ROOT_PATH {
                    if let Some(on) = prop::<bool>(&changed, "OnBattery") {
                        self.on_battery = on;
                    }
                    return true;
                }
                if sent_on != DEVICE_INTERFACE {
                    return true;
                }
                let device = std::iter::once(&mut self.display).chain(self.devices.iter_mut()).find(|(p, _)| p.as_str() == path.as_str());
                match device {
                    Some((_, props)) => {
                        props.extend(changed);
                        true
                    }
                    None => false,
                }
            }
            _ => true,
        }
    }
}

struct Watch(UPower);

impl Watched for Watch {
    type State = Reading;
    type View = Snapshot;
    const SERVICE: &'static str = SERVICE;

    fn rule(&self) -> zbus::Result<MatchRule<'static>> {
        Ok(MatchRule::builder().msg_type(Type::Signal).sender(SERVICE)?.build())
    }

    async fn read(&self) -> zbus::Result<Reading> {
        self.0.read().await
    }

    fn apply(&self, state: &mut Reading, signal: &Message) -> bool {
        state.apply(signal)
    }

    fn view(&self, state: &Reading) -> Snapshot {
        state.snapshot()
    }
}
