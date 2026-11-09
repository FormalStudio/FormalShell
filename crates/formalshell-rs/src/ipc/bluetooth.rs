//! `bluetooth`: the radio for compositor keybinds and the
//! rig (`toggle`, `power on|off`, `status`) and the headless path for the
//! panel's TRUSTED toggle (`trust`, `untrust`). No adapter is an honest
//! "error: no bluetooth adapter", and `status` stays JSON either way.
//!
//! `ok` on `trust` means the write was issued, as it does for `power`; a
//! check wanting ground truth reads `bluetoothctl info <address>`.

use fs_types::BtAddress;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::devices::bluetooth;
use crate::wayland::App;

const NO_ADAPTER: &str = "no bluetooth adapter";

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    if app.store.bluetooth.powered.is_none() {
        return Err(NO_ADAPTER.into());
    }
    if let Some(rt) = &app.runtime {
        rt.service(bluetooth::toggle_power);
    }
    ok()
}

fn power(app: &mut App, a: &[Value]) -> Reply {
    let on = match a[0].str() {
        "on" => true,
        "off" => false,
        _ => return Err("power state must be 'on' or 'off'".into()),
    };
    if app.store.bluetooth.powered.is_none() {
        return Err(NO_ADAPTER.into());
    }
    bluetooth::ask(bluetooth::Ask::Power(on));
    ok()
}

/// A keybind or script can paste an address back in whatever case it has.
fn write_trust(app: &mut App, address: &str, want: bool) -> Reply {
    let bt = &app.store.bluetooth;
    if bt.powered.is_none() {
        return Err(NO_ADAPTER.into());
    }
    let Some(device) = BtAddress::parse(address).and_then(|a| bt.devices.iter().find(|d| d.address == a)) else {
        return Err(format!("unknown device '{address}'"));
    };
    if !device.paired {
        return Err(format!("device '{address}' is not paired"));
    }
    let found = device.address;
    if let Some(rt) = &app.runtime {
        rt.service(move |ctx| bluetooth::set_trust(ctx, found, want));
    }
    ok()
}

fn trust(app: &mut App, a: &[Value]) -> Reply {
    write_trust(app, a[0].str(), true)
}

fn untrust(app: &mut App, a: &[Value]) -> Reply {
    write_trust(app, a[0].str(), false)
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.bluetooth.status())
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
