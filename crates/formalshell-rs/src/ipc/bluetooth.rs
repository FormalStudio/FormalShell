//! `bluetooth`: the radio for compositor keybinds and the
//! rig (`toggle`, `power on|off`, `status`) and the headless path for the
//! panel's TRUSTED toggle (`trust`, `untrust`). No adapter is an honest
//! "error: no bluetooth adapter", and `status` stays JSON either way.
//!
//! `ok` on `trust` means the write was issued, as it does for `power`; a
//! check wanting ground truth reads `bluetoothctl info <address>`.

use fs_types::BtAddress;

use super::registry::{Function, Target, Type, Value};
use crate::services::devices::bluetooth;
use crate::wayland::App;

const NO_ADAPTER: &str = "error: no bluetooth adapter";

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn toggle(app: &mut App, _: &[Value]) -> Value {
    if app.store.devices.bluetooth.powered.is_none() {
        return text(NO_ADAPTER);
    }
    if let Some(rt) = &app.runtime {
        rt.service(bluetooth::toggle_power);
    }
    text("ok")
}

fn power(app: &mut App, a: &[Value]) -> Value {
    let on = match a[0].str() {
        "on" => true,
        "off" => false,
        _ => return text("error: power state must be 'on' or 'off'"),
    };
    if app.store.devices.bluetooth.powered.is_none() {
        return text(NO_ADAPTER);
    }
    bluetooth::ask(bluetooth::Ask::Power(on));
    text("ok")
}

/// A keybind or script can paste an address back in whatever case it has.
fn write_trust(app: &mut App, address: &str, want: bool) -> Value {
    let bt = &app.store.devices.bluetooth;
    if bt.powered.is_none() {
        return text(NO_ADAPTER);
    }
    let Some(device) = BtAddress::parse(address).and_then(|a| bt.devices.iter().find(|d| d.address == a)) else {
        return text(format!("error: unknown device '{address}'"));
    };
    if !device.paired {
        return text(format!("error: device '{address}' is not paired"));
    }
    let found = device.address;
    if let Some(rt) = &app.runtime {
        rt.service(move |ctx| bluetooth::set_trust(ctx, found, want));
    }
    text("ok")
}

fn trust(app: &mut App, a: &[Value]) -> Value {
    write_trust(app, a[0].str(), true)
}

fn untrust(app: &mut App, a: &[Value]) -> Value {
    write_trust(app, a[0].str(), false)
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// The keys in the order the golden replies carry them.
fn status(app: &mut App, _: &[Value]) -> Value {
    let bt = &app.store.devices.bluetooth;
    let Some(powered) = bt.powered else {
        return text(r#"{"available":false,"enabled":false,"connected":0,"devices":[]}"#);
    };
    let rows: Vec<String> = bt
        .devices
        .iter()
        .map(|d| {
            format!(
                r#"{{"address":{},"name":{},"paired":{},"trusted":{},"connected":{}}}"#,
                quote(&d.address.to_string()),
                quote(if d.name.is_empty() { &d.device_name } else { &d.name }),
                d.paired,
                d.trusted,
                d.connected
            )
        })
        .collect();
    let connected = bt.devices.iter().filter(|d| d.connected).count();
    text(format!(r#"{{"available":true,"enabled":{powered},"connected":{connected},"devices":[{}]}}"#, rows.join(",")))
}

pub fn target() -> Target<App> {
    Target {
        name: "bluetooth",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "power", params: &[("state", Type::String)], ret: Type::String, call: power },
            Function { name: "trust", params: &[("address", Type::String)], ret: Type::String, call: trust },
            Function { name: "untrust", params: &[("address", Type::String)], ret: Type::String, call: untrust },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
