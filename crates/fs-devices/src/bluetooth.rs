//! The bluetooth panel's device buckets: which devices render, in which
//! bucket, sorted how, and their status line. Ported from
//! `shell/Bluetooth/model.js`.

use std::sync::LazyLock;

use regex::Regex;

use fs_js as js;

/// BlueZ's connection state for a device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceState {
    #[default]
    Disconnected,
    Connected,
    Disconnecting,
    Connecting,
}

/// One BlueZ device as the panel and the other device models see it.
/// `battery` is a 0..1 fraction, never a percentage.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Device {
    pub address: String,
    pub dbus_path: String,
    pub name: String,
    pub device_name: String,
    pub connected: bool,
    pub paired: bool,
    pub bonded: bool,
    pub trusted: bool,
    pub pairing: bool,
    pub state: DeviceState,
    pub battery: f64,
    pub battery_available: bool,
}

static MAC_SHAPED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u)^([0-9a-f]{2}[:-]){5}[0-9a-f]{2}$").unwrap());
static UUID_SHAPED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u)^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$").unwrap()
});
static HEX32: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i-u)^[0-9a-f]{32}$").unwrap());

pub(crate) fn is_mac_shaped(name: &str) -> bool {
    MAC_SHAPED.is_match(name)
}

pub(crate) fn is_uuid_shaped(name: &str) -> bool {
    UUID_SHAPED.is_match(name) || HEX32.is_match(name)
}

/// Rejects the empty string and the two label shapes BlueZ falls back to
/// when a device has not advertised a real name yet, a raw address or a
/// service UUID.
pub(crate) fn has_human_name(name: &str) -> bool {
    let label = js::trim(name);
    !(label.is_empty() || is_mac_shaped(label) || is_uuid_shaped(label))
}

fn label(device: &Device) -> &str {
    if !device.name.is_empty() { &device.name } else { &device.device_name }
}

fn sort_by_label(devices: &mut [&Device]) {
    devices.sort_by(|a, b| js::locale_compare(label(a), label(b)));
}

#[derive(Debug, Default)]
pub struct Buckets<'a> {
    pub connected: Vec<&'a Device>,
    pub known: Vec<&'a Device>,
    pub available: Vec<&'a Device>,
}

/// known = paired || bonded || trusted. Available devices only surface while
/// `discovering` is true: a raw scan result the adapter has not paired or
/// trusted has no business staying listed once scanning stops. Devices
/// without a human-readable name are dropped from every bucket.
pub fn buckets(devices: &[Device], discovering: bool) -> Buckets<'_> {
    let mut out = Buckets::default();
    for d in devices {
        if !has_human_name(label(d)) {
            continue;
        }
        if d.connected {
            out.connected.push(d);
        } else if d.paired || d.bonded || d.trusted {
            out.known.push(d);
        } else if discovering {
            out.available.push(d);
        }
    }
    sort_by_label(&mut out.connected);
    sort_by_label(&mut out.known);
    sort_by_label(&mut out.available);
    out
}

/// What BlueZ is doing to this device right now, or "" when nothing.
pub fn activity_text(device: Option<&Device>) -> &'static str {
    match device {
        None => "",
        Some(d) if d.pairing => "Pairing\u{2026}",
        Some(d) if d.state == DeviceState::Connecting => "Connecting\u{2026}",
        Some(_) => "",
    }
}

/// The row's trailing value: only a connected device that reports a battery
/// has one.
pub fn battery_text(device: Option<&Device>) -> String {
    match device {
        Some(d) if d.connected && d.battery_available => format!("{}%", js::num_str(js::round(d.battery * 100.0))),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str) -> Device {
        Device { name: name.into(), ..Default::default() }
    }

    fn names(devices: &[&Device]) -> String {
        devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(",")
    }

    #[test]
    fn has_human_name_rejects_empty_mac_and_uuid() {
        assert!(!has_human_name(""));
        assert!(!has_human_name("   "));
        assert!(!has_human_name("AA:BB:CC:DD:EE:FF"));
        assert!(!has_human_name("aa-bb-cc-dd-ee-ff"));
        assert!(!has_human_name("4c55a1e0-b100-4ac3-a2fc-2f2e1234abcd"));
        assert!(!has_human_name("4c55a1e0b1004ac3a2fc2f2e1234abcd"));
        assert!(has_human_name("Sony WH-1000XM4"));
    }

    #[test]
    fn buckets_accepts_a_plain_slice() {
        let devices = [
            Device { connected: true, paired: true, bonded: true, trusted: true, ..dev("AirPods Pro") },
            Device { connected: true, paired: true, bonded: true, trusted: true, ..dev("MX Master 3S M") },
        ];
        let b = buckets(&devices, false);
        assert_eq!(names(&b.connected), "AirPods Pro,MX Master 3S M");
        assert!(b.known.is_empty());
        assert!(b.available.is_empty());
    }

    #[test]
    fn buckets_split_connected_known_available_while_discovering() {
        let devices = [
            Device { connected: true, paired: true, ..dev("Zeta") },
            Device { paired: true, ..dev("Alpha") },
            Device { trusted: true, ..dev("Beta") },
            dev("Gamma"),
            dev("AA:BB:CC:DD:EE:FF"),
        ];
        let b = buckets(&devices, true);
        assert_eq!(names(&b.connected), "Zeta");
        assert_eq!(names(&b.known), "Alpha,Beta");
        assert_eq!(names(&b.available), "Gamma");
    }

    #[test]
    fn buckets_hide_available_when_not_discovering() {
        let devices = [Device { connected: true, ..dev("Zeta") }, dev("Gamma")];
        let b = buckets(&devices, false);
        assert_eq!(b.connected.len(), 1);
        assert_eq!(b.available.len(), 0);
    }

    #[test]
    fn buckets_filter_unnamed_devices_from_every_bucket() {
        let devices = [
            Device { connected: true, ..dev("AA:BB:CC:DD:EE:FF") },
            Device { paired: true, ..dev("") },
        ];
        let b = buckets(&devices, true);
        assert_eq!(b.connected.len(), 0);
        assert_eq!(b.known.len(), 0);
    }

    #[test]
    fn buckets_sort_alphabetically_within_a_bucket() {
        let devices = [
            Device { paired: true, ..dev("Zeta") },
            Device { paired: true, ..dev("Alpha") },
            Device { paired: true, ..dev("Mid") },
        ];
        assert_eq!(names(&buckets(&devices, false).known), "Alpha,Mid,Zeta");
    }

    #[test]
    fn a_device_label_falls_back_to_its_reported_name() {
        let devices = [Device { paired: true, device_name: "Reported".into(), ..Default::default() }];
        assert_eq!(buckets(&devices, false).known.len(), 1);
    }

    #[test]
    fn activity_text_pairing_beats_a_connecting_state() {
        let d = Device { pairing: true, state: DeviceState::Connecting, ..dev("A") };
        assert_eq!(activity_text(Some(&d)), "Pairing\u{2026}");
    }

    #[test]
    fn activity_text_connecting_state() {
        let d = Device { state: DeviceState::Connecting, ..dev("A") };
        assert_eq!(activity_text(Some(&d)), "Connecting\u{2026}");
    }

    #[test]
    fn activity_text_blank_for_a_settled_device() {
        let d = Device { connected: true, ..dev("A") };
        assert_eq!(activity_text(Some(&d)), "");
        assert_eq!(activity_text(None), "");
    }

    #[test]
    fn battery_text_percent_when_connected() {
        let d = Device { connected: true, battery_available: true, battery: 0.42, ..dev("A") };
        assert_eq!(battery_text(Some(&d)), "42%");
    }

    #[test]
    fn battery_text_blank_when_connected_without_battery() {
        let d = Device { connected: true, battery_available: false, ..dev("A") };
        assert_eq!(battery_text(Some(&d)), "");
    }

    #[test]
    fn battery_text_blank_for_a_disconnected_device() {
        let d = Device { connected: false, battery_available: true, battery: 0.9, ..dev("A") };
        assert_eq!(battery_text(Some(&d)), "");
        assert_eq!(battery_text(None), "");
    }

}
