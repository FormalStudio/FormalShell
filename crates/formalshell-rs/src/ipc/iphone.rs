//! `iphone`: `status` answers exactly what the panel
//! renders from, `installed: false` and nothing else when the bridge has
//! never answered a line. The rest drive the service the way the panel does.

use serde_json::{Value as Json, json};

use super::registry::{Function, Target, Type, Value};
use crate::services::info::iphone::{self, Cmd, State};
use crate::services::wants::{self, Source};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "iphone",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "pair", params: &[], ret: Type::String, call: pair },
            Function { name: "invoke", params: &[("id", Type::String), ("action", Type::String)], ret: Type::String, call: invoke },
            Function { name: "dismiss", params: &[("id", Type::String)], ret: Type::String, call: dismiss },
            Function { name: "clear", params: &[], ret: Type::String, call: clear },
            Function { name: "markRead", params: &[], ret: Type::String, call: mark_read },
        ],
    }
}

fn entry(n: &fs_devices::iphone::Notification) -> Json {
    json!({
        "id": n.id,
        "bundleId": n.bundle_id,
        "appName": n.app_name,
        "title": n.title,
        "subtitle": n.subtitle,
        "body": n.body,
        "deviceName": n.device_name,
        "deviceHandle": n.device_handle,
        "positiveAction": n.positive_action,
        "negativeAction": n.negative_action,
        "category": n.category,
        "categoryCount": n.category_count,
        "silent": n.silent,
        "important": n.important,
        "preexisting": n.preexisting,
        "session": n.session,
        "ts": fs_js::json_number(n.ts),
    })
}

fn status(app: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Iphone);
    let s: &State = &app.store.iphone;
    if !s.installed {
        return text(json!({ "installed": false }).to_string());
    }
    let ams = &app.store.media.ams;
    let battery = s.battery;
    text(
        json!({
            "installed": true,
            "available": s.observer,
            "connected": s.connected,
            "bonded": s.bonded,
            "bondAddress": s.bond_address.map(|a| a.to_string()).unwrap_or_default(),
            "deviceName": s.device_name,
            "batteryAvailable": battery.is_some(),
            "battery": battery.map_or(json!(0), fs_js::json_number),
            "unread": s.unread,
            "inFocus": s.in_focus,
            "pairingCode": s.pairing_code,
            "advertising": s.advertising,
            "lastError": crate::surfaces::panel::iphone::last_error(&app.store),
            "mediaAvailable": ams.available,
            "mediaTitle": ams.title,
            "recent": s.recent.iter().map(entry).collect::<Vec<_>>(),
        })
        .to_string(),
    )
}

fn pair(app: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Iphone);
    if !app.store.iphone.installed {
        return text("error: no bridge on PATH");
    }
    iphone::command(Cmd::Pair);
    text("ok")
}

/// Whatever does not parse as a number names nothing.
fn number(id: &str) -> Option<i64> {
    id.trim().parse::<f64>().ok().filter(|n| n.fract() == 0.0).map(|n| n as i64)
}

fn invoke(app: &mut App, args: &[Value]) -> Value {
    let action = args[1].str();
    if action != "positive" && action != "negative" {
        return text("error: action must be 'positive' or 'negative'");
    }
    wants::pulse(Source::Iphone);
    let id = number(args[0].str());
    match id.filter(|id| iphone::actionable(&app.store.iphone, *id)) {
        Some(id) => {
            iphone::command(Cmd::Invoke { id, positive: action == "positive" });
            text("ok")
        }
        None => text(format!("error: unknown notification '{}'", args[0].str())),
    }
}

fn dismiss(app: &mut App, args: &[Value]) -> Value {
    wants::pulse(Source::Iphone);
    let state = &app.store.iphone;
    let found = number(args[0].str()).and_then(|id| state.recent.iter().find(|e| e.id == id).map(|e| (id, e.negative_action.is_empty())));
    match found.filter(|(id, bare)| *bare || iphone::actionable(state, *id)) {
        Some((id, _)) => {
            iphone::command(Cmd::Dismiss(id));
            text("ok")
        }
        None => text(format!("error: unknown notification '{}'", args[0].str())),
    }
}

fn clear(_: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Iphone);
    iphone::command(Cmd::Clear);
    text("ok")
}

fn mark_read(_: &mut App, _: &[Value]) -> Value {
    wants::pulse(Source::Iphone);
    iphone::command(Cmd::MarkRead);
    text("ok")
}
