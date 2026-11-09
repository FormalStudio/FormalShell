//! `network`: the Wi-Fi list and its actions for the hwsim
//! rig and for binds, and the speed test the panel runs.

use fs_chrome::bar::panels::PanelId;

use super::registry::{Function, Json, Reply, Target, Type, Value, ok};
use crate::runtime::Ctx;
use crate::services::devices::network::{self as net, Network, Secret};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "network",
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

fn run(app: &App, f: impl FnOnce(&Ctx) + Send + 'static) {
    if let Some(rt) = &app.runtime {
        rt.service(f);
    }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    // The panel's "Known networks" disclosure, null while the panel is closed.
    let known_open = app.panel_call(PanelId::Network, "knownOpen", "").and_then(Result::ok).unwrap_or(Json::Null);
    // The SSID whose passphrase step is open, null with none or the panel shut.
    let prompt = app.panel_call(PanelId::Network, "prompt", "").and_then(Result::ok).unwrap_or(Json::Null);
    Ok(state(app).status(known_open, prompt))
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
            run(app, move |ctx| net::disconnect(ctx, ssid));
            return ok();
        }
        let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
        if retype || (row.secured && !row.known) {
            return Err(format!("'{ssid}' needs a password"));
        }
        run(app, move |ctx| net::connect(ctx, ssid, Secret::Saved));
    } else {
        run(app, move |ctx| net::connect(ctx, ssid, Secret::Psk(psk)));
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
    run(app, move |ctx| net::connect(ctx, ssid, secret));
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
    run(app, move |ctx| net::forget(ctx, ssid));
    ok()
}

fn wifi(app: &mut App, a: &[Value]) -> Reply {
    let on = a[0].bool();
    run(app, move |ctx| net::set_wifi(ctx, on));
    ok()
}

fn speedtest(app: &mut App, _: &[Value]) -> Reply {
    if state(app).speed.running() {
        return Err("speed test already running".into());
    }
    run(app, |ctx| {
        net::speed_start(ctx);
    });
    ok()
}

fn speedstatus(app: &mut App, _: &[Value]) -> Reply {
    Ok(state(app).speed_status())
}
