//! `network`: the Wi-Fi list and its actions for the hwsim
//! rig and for binds, and the speed test the panel runs.

use fs_js as js;

use super::registry::{Function, Target, Type, Value};
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

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let n = state(app);
    let rows: Vec<String> = n
        .rows
        .iter()
        .map(|r| {
            format!(
                "{{\"name\":{},\"known\":{},\"connected\":{},\"stateChanging\":{},\"secured\":{},\"signal\":{},\"inRange\":{}}}",
                quote(&r.ssid),
                r.known,
                r.connected,
                r.changing,
                r.secured,
                js::num_str(r.signal.as_fraction()),
                r.in_range
            )
        })
        .collect();
    let enabled = n.wifi_enabled;
    // The panel's "Known networks" disclosure, null while the panel is closed.
    let known_open = app.panel_call("network", "knownOpen", "").unwrap_or_else(|| "null".into());
    // The SSID whose passphrase step is open, null with none or the panel shut.
    let prompt = app.panel_call("network", "prompt", "").unwrap_or_else(|| "null".into());
    Value::Str(format!("{{\"wifiEnabled\":{enabled},\"knownOpen\":{known_open},\"prompt\":{prompt},\"networks\":[{}]}}", rows.join(",")))
}

/// A network action already in flight is refused rather than queued.
fn busy(n: &Network) -> Option<Value> {
    n.action.as_ref().map(|(kind, ssid)| Value::Str(format!("error: network action already in progress ({} {ssid})", kind.as_str())))
}

fn unknown(ssid: &str) -> Value {
    Value::Str(format!("error: unknown ssid '{ssid}'"))
}

fn connect(app: &mut App, a: &[Value]) -> Value {
    let n = state(app);
    if let Some(e) = busy(n) {
        return e;
    }
    let (ssid, psk) = (a[0].str().to_owned(), a[1].str().to_owned());
    let Some(row) = n.row(&ssid) else { return unknown(&ssid) };
    if psk.is_empty() {
        if row.connected {
            run(app, move |ctx| net::disconnect(ctx, ssid));
            return Value::Str("ok".into());
        }
        let retype = n.failure.as_ref().is_some_and(|f| f.ssid == ssid && f.secret);
        if retype || (row.secured && !row.known) {
            return Value::Str(format!("error: '{ssid}' needs a password"));
        }
        run(app, move |ctx| net::connect(ctx, ssid, Secret::Saved));
    } else {
        run(app, move |ctx| net::connect(ctx, ssid, Secret::Psk(psk)));
    }
    Value::Str("ok".into())
}

fn connect_eap(app: &mut App, a: &[Value]) -> Value {
    let n = state(app);
    if let Some(e) = busy(n) {
        return e;
    }
    let ssid = a[0].str().to_owned();
    if n.row(&ssid).is_none() {
        return unknown(&ssid);
    }
    let secret = Secret::Eap { identity: a[1].str().to_owned(), password: a[2].str().to_owned() };
    run(app, move |ctx| net::connect(ctx, ssid, secret));
    Value::Str("ok".into())
}

fn forget(app: &mut App, a: &[Value]) -> Value {
    let n = state(app);
    if let Some(e) = busy(n) {
        return e;
    }
    let ssid = a[0].str().to_owned();
    if n.row(&ssid).is_none() {
        return unknown(&ssid);
    }
    run(app, move |ctx| net::forget(ctx, ssid));
    Value::Str("ok".into())
}

fn wifi(app: &mut App, a: &[Value]) -> Value {
    let on = a[0].bool();
    run(app, move |ctx| net::set_wifi(ctx, on));
    Value::Str("ok".into())
}

fn speedtest(app: &mut App, _: &[Value]) -> Value {
    if state(app).speed.running() {
        return Value::Str("error: speed test already running".into());
    }
    run(app, |ctx| {
        net::speed_start(ctx);
    });
    Value::Str("ok".into())
}

fn speedstatus(app: &mut App, _: &[Value]) -> Value {
    let s = &state(app).speed;
    Value::Str(format!(
        "{{\"running\":{},\"phase\":\"{}\",\"downMbps\":{},\"upMbps\":{},\"error\":{}}}",
        s.running(),
        s.phase.as_str(),
        js::num_str(s.down_mbps()),
        js::num_str(s.up_mbps()),
        quote(&s.error)
    ))
}
