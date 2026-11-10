//! Where the power goes (adapter, laptop, battery, USB-C ports), built from
//! one sysfs snapshot. The shell reads it into the `@section` text
//! `parse_snapshot` takes, hands that to `build_flow` with whatever it
//! measured itself (RAPL, the dGPU, the iPhone bridge), and the drawing
//! reads the result. Everything the kernel does not
//! report stays `None`, and the drawing prints a missing figure as absent.
//!
//! Units, per Documentation/ABI/testing/sysfs-class-power: power_now is
//! microwatts, voltage_now microvolts, current_now microamps, capacity and
//! charge_control_end_threshold whole percents.

use fs_js as js;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

pub(crate) const FIXED_SUPPLY_VOLTS: f64 = 5.0;

/// The `power_supply` attributes the snapshot text carries per supply.
pub const SUPPLY_KEYS: [&str; 9] =
    ["type", "scope", "online", "status", "capacity", "power_now", "voltage_now", "current_now", "charge_control_end_threshold"];

/// The `typec` port attributes the snapshot text carries per port.
pub const PORT_KEYS: [&str; 2] = ["power_role", "power_operation_mode"];

fn num(text: &str) -> Option<f64> {
    if js::trim(text).is_empty() {
        return None;
    }
    Some(js::parse_number(text)).filter(|n| n.is_finite())
}

fn micro(text: &str) -> Option<f64> {
    num(text).map(|n| n / 1_000_000.0)
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Supply {
    pub name: String,
    pub kind: String,
    pub scope: String,
    pub online: Option<bool>,
    pub status: String,
    pub capacity: Option<f64>,
    pub power_w: Option<f64>,
    pub voltage_v: Option<f64>,
    pub current_a: Option<f64>,
    pub limit: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct TypecPort {
    pub name: String,
    pub index: u32,
    pub role: String,
    pub mode: String,
    pub partner: bool,
    pub product: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contract {
    pub position: u32,
    pub operating_a: f64,
    pub max_a: f64,
    pub watts: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectorStatus {
    pub mode: String,
    pub connected: bool,
    pub provider: bool,
    pub contract: Option<Contract>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub supplies: Vec<Supply>,
    pub ports: Vec<TypecPort>,
    pub ucsi: HashMap<String, ConnectorStatus>,
}

static UCSI_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\s+((?:0x)?[0-9a-fA-F]+)$").unwrap());

enum Current {
    None,
    Supply,
    Port,
}

/// `@supply NAME`, `@typec portN` and `@ucsi` headers, `key=value` lines in
/// the first two, `<connector> <0x hex>` lines in the last. A key the kernel
/// does not expose is simply not in the output, so it reads `None`.
pub fn parse_snapshot(text: &str) -> Snapshot {
    let mut out = Snapshot::default();
    let mut current = Current::None;
    let mut section = String::new();

    for raw in text.split('\n') {
        let line = js::trim(raw);
        if line.is_empty() {
            continue;
        }

        if line.starts_with('@') {
            let parts: Vec<&str> = js::split_ws(line);
            section = parts[0].to_string();
            let second = parts.get(1).copied().unwrap_or("");
            if section == "@supply" {
                out.supplies.push(Supply { name: second.to_string(), ..Default::default() });
                current = Current::Supply;
            } else if section == "@typec" {
                let digits: String = second.chars().filter(char::is_ascii_digit).collect();
                out.ports.push(TypecPort {
                    name: second.to_string(),
                    index: digits.parse().unwrap_or(0),
                    ..Default::default()
                });
                current = Current::Port;
            }
            continue;
        }

        if section == "@ucsi" {
            if let Some(m) = UCSI_LINE.captures(line) {
                match decode_connector_status(&m[2]) {
                    Some(status) => {
                        out.ucsi.insert(m[1].to_string(), status);
                    }
                    None => {
                        out.ucsi.remove(&m[1]);
                    }
                }
            }
            continue;
        }

        let Some(eq) = line.find('=') else { continue };
        let key = &line[..eq];
        let value = js::trim(&line[eq + 1..]);

        match (&current, section.as_str()) {
            (Current::Supply, "@supply") => {
                let s = out.supplies.last_mut().unwrap();
                match key {
                    "type" => s.kind = value.to_string(),
                    "scope" => s.scope = value.to_string(),
                    "online" => s.online = if value.is_empty() { None } else { Some(value != "0") },
                    "status" => s.status = value.to_string(),
                    "capacity" => s.capacity = num(value),
                    "power_now" => s.power_w = micro(value),
                    "voltage_now" => s.voltage_v = micro(value),
                    "current_now" => s.current_a = micro(value),
                    "charge_control_end_threshold" => s.limit = num(value),
                    _ => {}
                }
            }
            (Current::Port, "@typec") => {
                let p = out.ports.last_mut().unwrap();
                match key {
                    "power_role" => p.role = parse_bracketed(value),
                    "power_operation_mode" => p.mode = value.to_string(),
                    "partner" => p.partner = value == "1",
                    "product" => p.product = value.to_string(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// sysfs marks the selected entry of a choice attribute with brackets:
/// "[source] sink" is a port currently sourcing.
pub(crate) fn parse_bracketed(text: &str) -> String {
    let Some(open) = text.find('[') else { return String::new() };
    let rest = &text[open + 1..];
    match rest.find(']') {
        Some(close) if close > 0 => rest[..close].to_string(),
        _ => String::new(),
    }
}

const OPERATION_MODES: [&str; 6] = ["", "USB", "BC", "PD", "USB-C 1.5A", "USB-C 3A"];

static HEX32: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]{1,32}$").unwrap());

/// UCSI GET_CONNECTOR_STATUS (0x12 | connector << 16) response, the 128-bit
/// value the debugfs `response` file prints as one hex string. Bits 16-18 are
/// the power operation mode, bit 19 connected, bit 20 the power direction
/// (1 = this machine provides), bits 32-63 the request data object of the
/// negotiated contract.
pub(crate) fn decode_connector_status(hex: &str) -> Option<ConnectorStatus> {
    let digits = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")).unwrap_or(hex);
    if !HEX32.is_match(digits) {
        return None;
    }
    let padded = format!("{digits:0>32}");
    let lo = u32::from_str_radix(&padded[24..], 16).ok()?;
    let rdo = u32::from_str_radix(&padded[16..24], 16).ok()?;

    let mode = ((lo >> 16) & 7) as usize;
    let position = (rdo >> 28) & 0xf;
    // 10 mA units. Position 1 is always the 5 V vSafe5V object; any other
    // position needs the source capabilities to know its voltage, which the
    // kernel does not hand out, so no wattage is claimed for it.
    let op_a = f64::from((rdo >> 10) & 0x3ff) * 0.01;
    let max_a = f64::from(rdo & 0x3ff) * 0.01;
    Some(ConnectorStatus {
        mode: OPERATION_MODES.get(mode).copied().unwrap_or("").to_string(),
        connected: (lo >> 19) & 1 == 1,
        provider: (lo >> 20) & 1 == 1,
        contract: (position != 0).then(|| Contract {
            position,
            operating_a: op_a,
            max_a,
            watts: (position == 1).then(|| js::round(FIXED_SUPPLY_VOLTS * max_a * 10.0) / 10.0),
        }),
    })
}

pub(crate) fn watt_text(w: Option<f64>) -> String {
    let Some(w) = w else { return String::new() };
    let abs = w.abs();
    let figure = if abs >= 10.0 { js::num_str(js::round(abs)) } else { js::to_fixed(abs, 1) };
    format!("{figure}W")
}

fn sum_known(parts: &[(&'static str, Option<f64>)]) -> Option<(f64, Vec<&'static str>)> {
    let mut total = 0.0;
    let mut names = Vec::new();
    for (name, w) in parts {
        let Some(w) = w else { continue };
        total += w;
        names.push(*name);
    }
    (!names.is_empty()).then_some((total, names))
}

/// Watts a supply reports, from power_now or from voltage times current. A
/// source port's current_now is a limit rather than a reading, so this is only
/// asked of the mains adapter and of a port that says it is online.
fn supply_watts(s: &Supply) -> Option<f64> {
    if let Some(p) = s.power_w.filter(|p| *p > 0.0) {
        return Some(p);
    }
    match (s.voltage_v, s.current_a) {
        (Some(v), Some(i)) if v > 0.0 && i > 0.0 => Some(v * i),
        _ => None,
    }
}

fn is_battery(s: &Supply) -> bool {
    s.kind == "Battery" && s.scope != "Device"
}

fn is_adapter(s: &Supply) -> bool {
    !is_battery(s) && !s.kind.is_empty() && !s.name.contains("source") && s.scope != "Device"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryState {
    None,
    Held,
    Charging,
    Discharging,
    Full,
    Idle,
    Unknown,
}

impl BatteryState {
    pub fn as_str(self) -> &'static str {
        match self {
            BatteryState::None => "none",
            BatteryState::Held => "held",
            BatteryState::Charging => "charging",
            BatteryState::Discharging => "discharging",
            BatteryState::Full => "full",
            BatteryState::Idle => "idle",
            BatteryState::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    In,
    Out,
    None,
}

impl Link {
    pub fn as_str(self) -> &'static str {
        match self {
            Link::In => "in",
            Link::Out => "out",
            Link::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub present: bool,
    pub state: BatteryState,
    pub percent: Option<f64>,
    pub watts: Option<f64>,
    pub limit: Option<f64>,
    pub icon: &'static str,
    pub caption: &'static str,
    pub value: String,
    pub detail: String,
    pub link: Link,
}

fn battery(supply: Option<&Supply>, held: bool) -> Battery {
    let Some(supply) = supply else {
        return Battery {
            present: false,
            state: BatteryState::None,
            percent: None,
            watts: None,
            limit: None,
            icon: "battery",
            caption: "Battery",
            value: String::new(),
            detail: String::new(),
            link: Link::None,
        };
    };

    let status = supply.status.as_str();
    let state = if held {
        BatteryState::Held
    } else {
        match status {
            "Charging" => BatteryState::Charging,
            "Discharging" => BatteryState::Discharging,
            "Full" => BatteryState::Full,
            "Not charging" => BatteryState::Idle,
            _ => BatteryState::Unknown,
        }
    };
    let watts = supply_watts(supply);
    let flowing = matches!(state, BatteryState::Charging | BatteryState::Discharging);
    let with_watts = |word: &str| match watts {
        Some(_) => format!("{word} {}", watt_text(watts)),
        None => word.to_string(),
    };

    let detail = match state {
        BatteryState::Held => match supply.limit.and_then(super::model::charge_limit) {
            Some(limit) => format!("Held at {limit}%"),
            None => "Held at limit".to_string(),
        },
        BatteryState::Charging => with_watts("Charging"),
        BatteryState::Discharging => with_watts("Discharging"),
        BatteryState::Full => "Full".to_string(),
        BatteryState::Idle => "Not charging".to_string(),
        _ => String::new(),
    };

    Battery {
        present: true,
        state,
        percent: supply.capacity,
        watts: if flowing { watts } else { None },
        limit: supply.limit,
        icon: if state == BatteryState::Charging { "battery-charging" } else { "battery" },
        caption: "Battery",
        value: supply.capacity.map_or(String::new(), |c| format!("{}%", js::num_str(js::round(c)))),
        detail,
        link: match state {
            BatteryState::Charging => Link::In,
            BatteryState::Discharging => Link::Out,
            _ => Link::None,
        },
    }
}

fn port_supply(supplies: &[Supply], index: u32) -> Option<&Supply> {
    supplies.iter().find(|s| {
        let tail = s.name.rfind(':').map_or(s.name.as_str(), |i| &s.name[i + 1..]);
        s.name.contains("source") && js::parse_int(tail) == f64::from(index + 1)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WattsKind {
    None,
    Measured,
    Contract,
}

impl WattsKind {
    pub fn as_str(self) -> &'static str {
        match self {
            WattsKind::None => "",
            WattsKind::Measured => "measured",
            WattsKind::Contract => "contract",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Port {
    pub name: String,
    pub connected: bool,
    pub powering: bool,
    pub supplying: bool,
    pub label: String,
    pub detail: String,
    pub watts: Option<f64>,
    pub watts_kind: WattsKind,
    pub mode: String,
}

/// What the panel measured itself, each `None` when its source is unavailable.
#[derive(Debug, Clone, Copy, Default)]
pub struct Extras {
    pub cpu_w: Option<f64>,
    pub gpu_w: Option<f64>,
    pub iphone_percent: Option<f64>,
    /// The battery holds at its charge limit, as UPower's state says
    /// (`charge_threshold_active`); the supply files cannot tell it.
    pub held: bool,
}

/// The kernel numbers typec ports from 0 and UCSI connectors, and the
/// ucsi-source-psy-...:00N supplies, from 1.
fn port(port: &TypecPort, snapshot: &Snapshot, extras: &Extras) -> Port {
    let ucsi = snapshot.ucsi.get(&(port.index + 1).to_string());
    let supply = port_supply(&snapshot.supplies, port.index);
    let connected = ucsi.map_or(port.partner, |u| u.connected);
    let provider = match ucsi {
        Some(u) => u.connected && u.provider,
        None => port.partner && port.role == "source",
    };
    let consumer = connected && !provider && (ucsi.is_some() || port.role == "sink");
    let mode = match ucsi {
        Some(u) if !u.mode.is_empty() => u.mode.clone(),
        _ if port.mode == "usb_power_delivery" => "PD".to_string(),
        _ => port.mode.clone(),
    };

    let mut watts = None;
    let mut kind = WattsKind::None;
    let measured = supply.filter(|s| {
        provider
            && s.online == Some(true)
            && s.voltage_v.is_some_and(|v| v > 0.0)
            && s.current_a.is_some_and(|i| i > 0.0)
    });
    if let Some(s) = measured {
        watts = Some(s.voltage_v.unwrap_or(0.0) * s.current_a.unwrap_or(0.0));
        kind = WattsKind::Measured;
    } else if let Some(w) = ucsi.filter(|_| provider).and_then(|u| u.contract.as_ref()).and_then(|c| c.watts) {
        watts = Some(w);
        kind = WattsKind::Contract;
    }

    let product = port.product.as_str();
    let percent = if !product.is_empty() && product.to_lowercase().contains("iphone") {
        extras.iphone_percent.map(js::round)
    } else {
        None
    };

    let mut detail = if !connected {
        "Nothing attached".to_string()
    } else if provider {
        match watts {
            Some(_) => format!(
                "Powering, {}{}",
                watt_text(watts),
                if kind == WattsKind::Contract { " contract" } else { "" }
            ),
            None => "Powering a device".to_string(),
        }
    } else if consumer {
        "Powering the laptop".to_string()
    } else {
        "Connected".to_string()
    };
    if connected && provider && !mode.is_empty() && watts.is_none() {
        detail.push_str(&format!(", {mode}"));
    }

    Port {
        name: format!("USB-C {}", port.index + 1),
        connected,
        powering: provider,
        supplying: consumer,
        label: if connected {
            let who = if product.is_empty() { "Device" } else { product };
            match percent {
                Some(p) => format!("{who}, {}%", js::num_str(p)),
                None => who.to_string(),
            }
        } else {
            String::new()
        },
        detail,
        watts,
        watts_kind: kind,
        mode,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    pub present: bool,
    pub online: bool,
    pub icon: &'static str,
    pub caption: &'static str,
    pub value: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Laptop {
    pub icon: &'static str,
    pub caption: &'static str,
    pub value: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Links {
    pub adapter: bool,
    pub battery: Link,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Flow {
    pub available: bool,
    pub adapter: Adapter,
    pub laptop: Laptop,
    pub battery: Battery,
    pub links: Links,
    pub ports: Vec<Port>,
    pub powering_ports: usize,
}

/// On battery the laptop node is the battery's own draw; on AC power_now is 0,
/// so the node is the sum of the parts measured here and is captioned with
/// exactly which parts those are.
pub fn build_flow(snapshot: &Snapshot, extras: &Extras) -> Flow {
    let mut battery_supply: Option<&Supply> = None;
    let mut adapter_supply: Option<&Supply> = None;
    let mut adapter_online = false;

    for s in &snapshot.supplies {
        if is_battery(s) && battery_supply.is_none() {
            battery_supply = Some(s);
        } else if is_adapter(s) {
            if adapter_supply.is_none() || s.online == Some(true) {
                adapter_supply = Some(s);
            }
            if s.online == Some(true) {
                adapter_online = true;
            }
        }
    }

    let ports: Vec<Port> = snapshot.ports.iter().map(|p| port(p, snapshot, extras)).collect();
    let usbc_charger = ports.iter().any(|p| p.supplying);
    adapter_online = adapter_online || usbc_charger;

    let bat = battery(battery_supply, extras.held);
    let adapter_watts = adapter_supply.filter(|s| s.online == Some(true)).and_then(supply_watts);
    let adapter_present = adapter_supply.is_some() || usbc_charger;

    let mut laptop_watts = None;
    let mut laptop_detail = "No reading".to_string();
    if bat.state == BatteryState::Discharging && bat.watts.is_some() {
        laptop_watts = bat.watts;
        laptop_detail = "System draw".to_string();
    } else if let Some((watts, names)) = sum_known(&[("CPU", extras.cpu_w), ("GPU", extras.gpu_w)]) {
        laptop_watts = Some(watts);
        laptop_detail = format!("{} only", names.join(" + "));
    }

    let powering_ports = ports.iter().filter(|p| p.powering).count();

    Flow {
        available: adapter_present || bat.present || !ports.is_empty(),
        adapter: Adapter {
            present: adapter_present,
            online: adapter_online,
            icon: if adapter_online { "plug-zap" } else { "plug" },
            caption: "Adapter",
            value: if adapter_present && adapter_watts.is_some() { watt_text(adapter_watts) } else { String::new() },
            detail: if !adapter_present {
                String::new()
            } else if adapter_online {
                if adapter_watts.is_some() { String::new() } else { "Plugged in".to_string() }
            } else {
                "Unplugged".to_string()
            },
        },
        laptop: Laptop {
            icon: "laptop",
            caption: "Laptop",
            value: if laptop_watts.is_some() { watt_text(laptop_watts) } else { String::new() },
            detail: laptop_detail,
        },
        links: Links { adapter: adapter_online, battery: bat.link },
        battery: bat,
        ports,
        powering_ports,
    }
}

/// The measured watts shown in the gallery come from this fixed snapshot, the
/// g815 read over sysfs and UCSI on 2026-09-29 (AC, charge limit 80, both
/// USB-C ports supplying a MacBook and an iPhone), never from a running
/// machine. The iPhone's 87 percent is a stand-in for the bridge.
pub fn sample_flow() -> Flow {
    let snapshot = parse_snapshot(
        &[
            "@supply ADP0",
            "type=Mains",
            "online=1",
            "@supply BAT0",
            "type=Battery",
            "status=Not charging",
            "capacity=80",
            "power_now=0",
            "voltage_now=16659000",
            "charge_control_end_threshold=80",
            "@supply ucsi-source-psy-USBC000:001",
            "type=USB",
            "online=0",
            "voltage_now=0",
            "current_now=3000000",
            "@supply ucsi-source-psy-USBC000:002",
            "type=USB",
            "online=0",
            "voltage_now=0",
            "current_now=3000000",
            "@typec port0",
            "power_role=[source] sink",
            "power_operation_mode=usb_power_delivery",
            "partner=1",
            "product=iPhone",
            "@typec port1",
            "power_role=[source] sink",
            "power_operation_mode=usb_power_delivery",
            "partner=1",
            "@ucsi",
            "1 0x00000000000000011784b12c403b0000",
            "2 0x00000000000000011504b12c801b0000",
        ]
        .join("\n"),
    );
    build_flow(&snapshot, &Extras { cpu_w: None, gpu_w: Some(22.73), iphone_percent: Some(87.0), held: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The g815 on AC with its charge limit at 80, both USB-C ports sourcing.
    const G815: &str = "@supply ADP0\ntype=Mains\nonline=1\n\
        @supply BAT0\ntype=Battery\nstatus=Not charging\ncapacity=80\npower_now=0\n\
        voltage_now=16659000\ncharge_control_end_threshold=80\n\
        @supply ucsi-source-psy-USBC000:001\ntype=USB\nonline=0\nvoltage_now=0\ncurrent_now=3000000\n\
        @supply ucsi-source-psy-USBC000:002\ntype=USB\nonline=0\nvoltage_now=0\ncurrent_now=3000000\n\
        @typec port0\npower_role=[source] sink\npower_operation_mode=usb_power_delivery\npartner=1\nproduct=iPhone\n\
        @typec port1\npower_role=[source] sink\npower_operation_mode=usb_power_delivery\npartner=1\n\
        @ucsi\n1 0x00000000000000011784b12c403b0000\n2 0x00000000000000011504b12c801b0000";

    fn flow(text: &str, extras: Extras) -> Flow {
        build_flow(&parse_snapshot(text), &extras)
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 0.001, "{a} vs {b}");
    }

    // decodeConnectorStatus

    #[test]
    fn decode_g815_connector_one() {
        let c = decode_connector_status("0x00000000000000011784b12c403b0000").unwrap();
        assert_eq!(c.mode, "PD");
        assert!(c.connected);
        assert!(c.provider);
        let contract = c.contract.unwrap();
        assert_eq!(contract.position, 1);
        close(contract.operating_a, 3.0);
        close(contract.max_a, 3.0);
        assert_eq!(contract.watts, Some(15.0));
    }

    #[test]
    fn decode_g815_connector_two() {
        let c = decode_connector_status("0x00000000000000011504b12c801b0000").unwrap();
        assert!(c.connected);
        assert!(c.provider);
        assert_eq!(c.contract.unwrap().watts, Some(15.0));
    }

    #[test]
    fn decode_empty_connector() {
        let c = decode_connector_status("0x00000000000000000000000000000000").unwrap();
        assert!(!c.connected);
        assert!(!c.provider);
        assert_eq!(c.contract, None);
    }

    #[test]
    fn decode_higher_object_position_claims_no_watts() {
        let c = decode_connector_status(&format!("0x0000000000000000{}{}", "2000c12c", "00190000")).unwrap();
        let contract = c.contract.unwrap();
        assert_eq!(contract.position, 2);
        assert_eq!(contract.watts, None);
    }

    #[test]
    fn decode_garbage_is_none() {
        assert_eq!(decode_connector_status("0xzz"), None);
    }

    #[test]
    fn parse_ucsi_lines_with_or_without_prefix() {
        let s = parse_snapshot("@ucsi\n1 0x00000000000000011784b12c403b0000\n2 00000000000000011504b12c801b0000\n");
        assert!(s.ucsi["1"].connected);
        assert_eq!(s.ucsi["2"].contract.as_ref().unwrap().watts, Some(15.0));
    }

    // parseSnapshot units

    #[test]
    fn parse_converts_micro_units() {
        let s = parse_snapshot(
            "@supply BAT0\ntype=Battery\npower_now=12500000\nvoltage_now=16659000\ncurrent_now=750000\ncapacity=55",
        );
        let b = &s.supplies[0];
        assert_eq!(b.power_w, Some(12.5));
        assert_eq!(b.voltage_v, Some(16.659));
        assert_eq!(b.current_a, Some(0.75));
        assert_eq!(b.capacity, Some(55.0));
    }

    #[test]
    fn parse_missing_key_is_none() {
        let s = parse_snapshot("@supply ADP0\ntype=Mains\nonline=1");
        assert_eq!(s.supplies[0].power_w, None);
        assert_eq!(s.supplies[0].limit, None);
        assert_eq!(s.supplies[0].online, Some(true));
    }

    #[test]
    fn parse_role_brackets() {
        assert_eq!(parse_bracketed("[source] sink"), "source");
        assert_eq!(parse_bracketed("source [sink]"), "sink");
        assert_eq!(parse_bracketed("source"), "");
    }

    #[test]
    fn parse_product_keeps_equals() {
        let s = parse_snapshot("@typec port0\npartner=1\nproduct=A=B");
        assert_eq!(s.ports[0].product, "A=B");
    }

    // buildFlow on the g815

    #[test]
    fn g815_battery_is_held_with_no_flow() {
        let f = flow(G815, Extras { gpu_w: Some(22.73), held: true, ..Default::default() });
        assert_eq!(f.battery.state.as_str(), "held");
        assert_eq!(f.battery.detail, "Held at 80%");
        assert_eq!(f.battery.value, "80%");
        assert_eq!(f.battery.link.as_str(), "none");
        assert_eq!(f.links.battery.as_str(), "none");
        assert!(f.links.adapter);
    }

    #[test]
    fn g815_adapter_has_no_watts() {
        let f = flow(G815, Extras::default());
        assert!(f.adapter.online);
        assert_eq!(f.adapter.value, "");
        assert_eq!(f.adapter.detail, "Plugged in");
    }

    #[test]
    fn g815_laptop_is_measured_parts_only() {
        let f = flow(G815, Extras { cpu_w: None, gpu_w: Some(22.73), ..Default::default() });
        assert_eq!(f.laptop.caption, "Laptop");
        assert_eq!(f.laptop.value, "23W");
        assert_eq!(f.laptop.detail, "GPU only");
    }

    #[test]
    fn laptop_sums_cpu_and_gpu() {
        let f = flow(G815, Extras { cpu_w: Some(6.4), gpu_w: Some(22.6), ..Default::default() });
        assert_eq!(f.laptop.detail, "CPU + GPU only");
        assert_eq!(f.laptop.value, "29W");
    }

    #[test]
    fn laptop_with_no_parts_reads_nothing() {
        let f = flow(G815, Extras::default());
        assert_eq!(f.laptop.value, "");
        assert_eq!(f.laptop.detail, "No reading");
    }

    #[test]
    fn g815_ports_from_ucsi() {
        let f = flow(G815, Extras { iphone_percent: Some(87.4), ..Default::default() });
        assert_eq!(f.ports.len(), 2);
        assert_eq!(f.powering_ports, 2);
        assert_eq!(f.ports[0].name, "USB-C 1");
        assert_eq!(f.ports[0].label, "iPhone, 87%");
        assert_eq!(f.ports[0].detail, "Powering, 15W contract");
        assert_eq!(f.ports[0].watts_kind.as_str(), "contract");
        assert_eq!(f.ports[1].label, "Device");
        assert_eq!(f.ports[1].watts, Some(15.0));
    }

    #[test]
    fn ports_without_ucsi_claim_no_number() {
        let text = G815.split("@ucsi").next().unwrap();
        let f = flow(text, Extras::default());
        assert!(f.ports[0].powering);
        assert_eq!(f.ports[0].watts, None);
        assert_eq!(f.ports[0].detail, "Powering a device, PD");
    }

    #[test]
    fn iphone_percent_only_on_iphone_port() {
        let f = flow(G815, Extras { iphone_percent: Some(50.0), ..Default::default() });
        assert_eq!(f.ports[1].label, "Device");
    }

    #[test]
    fn idle_port() {
        let f = flow("@typec port0\npower_role=[source] sink\npower_operation_mode=usb", Extras::default());
        assert!(!f.ports[0].connected);
        assert!(!f.ports[0].powering);
        assert_eq!(f.ports[0].detail, "Nothing attached");
    }

    #[test]
    fn port_measured_from_online_source_supply() {
        let text = "@supply ucsi-source-psy-USBC000:001\ntype=USB\nonline=1\nvoltage_now=9000000\ncurrent_now=1500000\n\
                    @typec port0\npower_role=[source] sink\npartner=1";
        let f = flow(text, Extras::default());
        assert_eq!(f.ports[0].watts_kind.as_str(), "measured");
        close(f.ports[0].watts.unwrap(), 13.5);
        assert_eq!(f.ports[0].detail, "Powering, 14W");
    }

    #[test]
    fn source_supply_limit_is_not_a_measurement() {
        let f = flow(G815.split("@ucsi").next().unwrap(), Extras::default());
        assert_eq!(f.ports[0].watts_kind.as_str(), "");
    }

    // battery directions

    #[test]
    fn discharging_uses_battery_draw_as_system() {
        let f = flow(
            "@supply BAT0\ntype=Battery\nstatus=Discharging\ncapacity=60\npower_now=11200000\n@supply AC\ntype=Mains\nonline=0",
            Extras { cpu_w: Some(3.0), ..Default::default() },
        );
        assert_eq!(f.battery.state.as_str(), "discharging");
        assert_eq!(f.links.battery.as_str(), "out");
        assert!(!f.links.adapter);
        assert_eq!(f.laptop.detail, "System draw");
        assert_eq!(f.laptop.value, "11W");
        assert_eq!(f.adapter.detail, "Unplugged");
        assert_eq!(f.battery.detail, "Discharging 11W");
    }

    #[test]
    fn charging_flows_into_battery() {
        let f = flow(
            "@supply BAT0\ntype=Battery\nstatus=Charging\ncapacity=40\npower_now=30000000\n@supply ADP0\ntype=Mains\nonline=1",
            Extras::default(),
        );
        assert_eq!(f.battery.state.as_str(), "charging");
        assert_eq!(f.links.battery.as_str(), "in");
        assert_eq!(f.battery.watts, Some(30.0));
        assert_eq!(f.battery.icon, "battery-charging");
    }

    #[test]
    fn adapter_reports_watts_when_kernel_does() {
        let f = flow(
            "@supply ADP0\ntype=Mains\nonline=1\nvoltage_now=20000000\ncurrent_now=3250000\n@supply BAT0\ntype=Battery\nstatus=Charging\ncapacity=40",
            Extras::default(),
        );
        assert_eq!(f.adapter.value, "65W");
    }

    #[test]
    fn current_and_voltage_give_battery_watts() {
        let f = flow(
            "@supply BAT0\ntype=Battery\nstatus=Discharging\ncapacity=40\nvoltage_now=12000000\ncurrent_now=1000000",
            Extras::default(),
        );
        assert_eq!(f.battery.watts, Some(12.0));
    }

    #[test]
    fn usbc_charger_counts_as_adapter() {
        let f = flow(
            "@supply BAT0\ntype=Battery\nstatus=Charging\ncapacity=40\n@typec port0\npower_role=source [sink]\npartner=1",
            Extras::default(),
        );
        assert!(f.adapter.online);
        assert!(f.ports[0].supplying);
        assert_eq!(f.ports[0].detail, "Powering the laptop");
    }

    #[test]
    fn device_scope_battery_is_ignored() {
        let f = flow("@supply hidpp\ntype=Battery\nscope=Device\ncapacity=50", Extras::default());
        assert!(!f.battery.present);
        assert!(!f.available);
    }

    #[test]
    fn nothing_at_all_is_unavailable() {
        let f = flow("", Extras::default());
        assert!(!f.available);
        assert_eq!(f.ports.len(), 0);
        assert!(!f.battery.present);
        assert!(!f.adapter.present);
    }

    #[test]
    fn sample_matches_the_g815_reading() {
        let f = sample_flow();
        assert_eq!(f.battery.state.as_str(), "held");
        assert_eq!(f.powering_ports, 2);
        assert_eq!(f.laptop.detail, "GPU only");
    }
}
