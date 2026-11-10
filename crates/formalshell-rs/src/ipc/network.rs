//! `network`: the Wi-Fi list and its actions for the hwsim
//! rig and for binds, and the speed test the panel runs.

use fs_chrome::bar::panels::PanelId;

use super::registry::{Function, Json, Reply, Target, Type, Value, ok};
use crate::services::devices::network::{self as net, Network, Secret};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "network",
        reads_screen: false,
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "connect", params: &[("ssid", Type::String), ("psk", Type::String)], ret: Type::String, call: connect },
            Function {
                name: "connectEap",
                params: &[("ssid", Type::String), ("identity", Type::String), ("password", Type::String)],
                ret: Type::String,
                call: connect_eap,
            },
            Function { name: "forget", params: &[("ssid", Type::String)], ret: Type::String, call: forget },
            Function { name: "wifi", params: &[("enabled", Type::Bool)], ret: Type::String, call: wifi },
            Function { name: "speedtest", params: &[], ret: Type::String, call: speedtest },
            Function { name: "speedstatus", params: &[], ret: Type::String, call: speedstatus },
        ],
    }
}

fn state(app: &App) -> &Network {
    &app.store.network
}


fn status(app: &mut App, _: &[Value]) -> Reply {
    // The panel's "Known networks" disclosure and the SSID whose passphrase
    // step is open, both null while the panel is closed.
    let panel = app.panel_status(PanelId::Network).unwrap_or(Json::Null);
    Ok(state(app).status(panel["knownOpen"].clone(), panel["prompt"].clone()))
}

/// A network action already in flight is refused rather than queued.
fn busy(n: &Network) -> Option<String> {
    n.action.as_ref().map(|(kind, ssid)| format!("network action already in progress ({} {ssid})", kind.as_str()))
}

fn unknown(ssid: &str) -> Reply {
    Err(format!("unknown ssid '{ssid}'"))
}

fn connect(app: &mut App, a: &[Value]) -> Reply {
    let n = state(app);
    if let Some(e) = busy(n) {
        return Err(e);
    }
    let (ssid, psk) = (a[0].str().to_owned(), a[1].str().to_owned());
    let Some(row) = n.row(&ssid) else { return unknown(&ssid) };
    if psk.is_empty() {
        if row.connected {
            net::send(net::Cmd::Disconnect(ssid));
            return ok();
        }
        let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
        if retype || (row.secured && !row.known) {
            return Err(format!("'{ssid}' needs a password"));
        }
        net::send(net::Cmd::Connect(ssid, Secret::Saved));
    } else {
        net::send(net::Cmd::Connect(ssid, Secret::Psk(psk)));
    }
    ok()
}

fn connect_eap(app: &mut App, a: &[Value]) -> Reply {
    let n = state(app);
    if let Some(e) = busy(n) {
        return Err(e);
    }
    let ssid = a[0].str().to_owned();
    if n.row(&ssid).is_none() {
        return unknown(&ssid);
    }
    let secret = Secret::Eap { identity: a[1].str().to_owned(), password: a[2].str().to_owned() };
    net::send(net::Cmd::Connect(ssid, secret));
    ok()
}

fn forget(app: &mut App, a: &[Value]) -> Reply {
    let n = state(app);
    if let Some(e) = busy(n) {
        return Err(e);
    }
    let ssid = a[0].str().to_owned();
    if n.row(&ssid).is_none() {
        return unknown(&ssid);
    }
    net::send(net::Cmd::Forget(ssid));
    ok()
}

fn wifi(_: &mut App, a: &[Value]) -> Reply {
    let on = a[0].bool();
    net::send(net::Cmd::Wifi(on));
    ok()
}

fn speedtest(app: &mut App, _: &[Value]) -> Reply {
    if state(app).speed.running() {
        return Err("speed test already running".into());
    }
    net::send(net::Cmd::SpeedTest);
    ok()
}

fn speedstatus(app: &mut App, _: &[Value]) -> Reply {
    Ok(state(app).speed_status())
}
