use std::collections::HashMap;
use std::time::Duration;

use fs_types::Percent;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// `org.freedesktop.UPower.Device.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Unknown,
    LinePower,
    Battery,
    Ups,
    Monitor,
    Mouse,
    Keyboard,
    Pda,
    Phone,
    MediaPlayer,
    Tablet,
    Computer,
    GamingInput,
    Pen,
    Touchpad,
    Modem,
    Network,
    Headset,
    Speakers,
    Headphones,
    Video,
    OtherAudio,
    RemoteControl,
    Printer,
    Scanner,
    Camera,
    Wearable,
    Toy,
    BluetoothGeneric,
}

impl DeviceKind {
    fn from_wire(wire: u32) -> Self {
        use DeviceKind::*;
        const ALL: [DeviceKind; 29] = [
            Unknown, LinePower, Battery, Ups, Monitor, Mouse, Keyboard, Pda, Phone, MediaPlayer,
            Tablet, Computer, GamingInput, Pen, Touchpad, Modem, Network, Headset, Speakers,
            Headphones, Video, OtherAudio, RemoteControl, Printer, Scanner, Camera, Wearable, Toy,
            BluetoothGeneric,
        ];
        ALL.get(wire as usize).copied().unwrap_or(Unknown)
    }
}

/// `org.freedesktop.UPower.Device.State`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Unknown,
    Charging,
    Discharging,
    Empty,
    FullyCharged,
    PendingCharge,
    PendingDischarge,
}

impl DeviceState {
    fn from_wire(wire: u32) -> Self {
        match wire {
            1 => Self::Charging,
            2 => Self::Discharging,
            3 => Self::Empty,
            4 => Self::FullyCharged,
            5 => Self::PendingCharge,
            6 => Self::PendingDischarge,
            _ => Self::Unknown,
        }
    }
}

/// One UPower device, read in a single `GetAll`.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    pub path: OwnedObjectPath,
    pub kind: DeviceKind,
    pub state: DeviceState,
    pub percentage: Percent,
    pub is_present: bool,
    pub power_supply: bool,
    pub model: String,
    pub icon_name: String,
    pub native_path: String,
    /// Watt-hours.
    pub energy: f64,
    pub energy_full: f64,
    /// Watts, as UPower reports it (a magnitude on most drivers).
    pub energy_rate: f64,
    /// `None` when UPower reports 0: the estimate does not apply to the
    /// current state, or there is no reading yet.
    pub time_to_empty: Option<Duration>,
    pub time_to_full: Option<Duration>,
    /// Health as a share of design capacity. `None` when the driver reports
    /// no capacity (UPower sends 0).
    pub capacity: Option<Percent>,
}

impl Device {
    /// An internal pack,
    /// not a mouse or a UPS.
    pub fn is_laptop_battery(&self) -> bool {
        self.kind == DeviceKind::Battery && self.power_supply
    }

    pub(crate) fn from_properties(
        path: OwnedObjectPath,
        props: &HashMap<String, OwnedValue>,
    ) -> Self {
        let seconds = |name| {
            let wire: i64 = get(props, name).unwrap_or(0);
            u64::try_from(wire)
                .ok()
                .filter(|s| *s > 0)
                .map(Duration::from_secs)
        };
        let capacity: f64 = get(props, "Capacity").unwrap_or(0.0);
        Self {
            path,
            kind: DeviceKind::from_wire(get(props, "Type").unwrap_or(0)),
            state: DeviceState::from_wire(get(props, "State").unwrap_or(0)),
            percentage: Percent::from_percent(get(props, "Percentage").unwrap_or(0.0)),
            is_present: get(props, "IsPresent").unwrap_or(false),
            power_supply: get(props, "PowerSupply").unwrap_or(false),
            model: get(props, "Model").unwrap_or_default(),
            icon_name: get(props, "IconName").unwrap_or_default(),
            native_path: get(props, "NativePath").unwrap_or_default(),
            energy: get(props, "Energy").unwrap_or(0.0),
            energy_full: get(props, "EnergyFull").unwrap_or(0.0),
            energy_rate: get(props, "EnergyRate").unwrap_or(0.0),
            time_to_empty: seconds("TimeToEmpty"),
            time_to_full: seconds("TimeToFull"),
            capacity: (capacity > 0.0).then(|| Percent::from_percent(capacity)),
        }
    }
}

pub(crate) fn get<T>(props: &HashMap<String, OwnedValue>, name: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    props.get(name)?.try_clone().ok()?.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::Value;

    fn props(entries: &[(&str, Value<'_>)]) -> HashMap<String, OwnedValue> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.try_to_owned().unwrap()))
            .collect()
    }

    fn path() -> OwnedObjectPath {
        OwnedObjectPath::try_from("/org/freedesktop/UPower/devices/battery_BAT0").unwrap()
    }

    #[test]
    fn reads_a_discharging_pack() {
        let d = Device::from_properties(
            path(),
            &props(&[
                ("Type", Value::U32(2)),
                ("State", Value::U32(2)),
                ("Percentage", Value::F64(41.0)),
                ("PowerSupply", Value::Bool(true)),
                ("IsPresent", Value::Bool(true)),
                ("TimeToEmpty", Value::I64(5400)),
                ("TimeToFull", Value::I64(0)),
                ("EnergyRate", Value::F64(7.5)),
                ("Capacity", Value::F64(93.0)),
                ("Model", Value::from("DELL 4GVMP")),
            ]),
        );
        assert!(d.is_laptop_battery());
        assert_eq!(d.state, DeviceState::Discharging);
        assert_eq!(d.percentage.rounded(), 41);
        assert_eq!(d.time_to_empty, Some(Duration::from_secs(5400)));
        assert_eq!(d.time_to_full, None);
        assert_eq!(d.capacity.map(Percent::rounded), Some(93));
        assert_eq!(d.model, "DELL 4GVMP");
    }

    #[test]
    fn missing_properties_read_as_the_empty_device() {
        let d = Device::from_properties(path(), &HashMap::new());
        assert_eq!(d.kind, DeviceKind::Unknown);
        assert_eq!(d.state, DeviceState::Unknown);
        assert!(!d.is_laptop_battery());
        assert_eq!(d.capacity, None);
    }

    #[test]
    fn a_mouse_is_not_a_laptop_battery() {
        let d = Device::from_properties(
            path(),
            &props(&[("Type", Value::U32(5)), ("PowerSupply", Value::Bool(false))]),
        );
        assert_eq!(d.kind, DeviceKind::Mouse);
        assert!(!d.is_laptop_battery());
    }

    #[test]
    fn unknown_wire_values_do_not_panic() {
        assert_eq!(DeviceKind::from_wire(999), DeviceKind::Unknown);
        assert_eq!(DeviceState::from_wire(999), DeviceState::Unknown);
    }
}
