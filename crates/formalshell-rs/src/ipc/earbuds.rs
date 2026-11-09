//! `earbuds`: the active device as the panel renders it,
//! the device list, a pick, and a control change against the active device
//! (`set noise anc`, `set ca on`, `set adaptive 40`), refused with the
//! reason rather than dropped, and `recenter`, which zeroes the AirPods'
//! head yaw, pitch and roll.

use fs_devices::earbuds as model;
use serde_json::{Value as Json, json};

use super::registry::{Function, Target, Type, Value};
use crate::services::devices::earbuds;
use crate::store;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "earbuds",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "devices", params: &[], ret: Type::String, call: list },
            Function { name: "select", params: &[("key", Type::String)], ret: Type::String, call: select },
            Function { name: "recenter", params: &[], ret: Type::String, call: recenter },
            Function {
                name: "set",
                params: &[("control", Type::String), ("value", Type::String)],
                ret: Type::String,
                call: set,
            },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let e = &app.store.earbuds;
    if !e.available() {
        return text(json!({"available": false}).to_string());
    }
    let device = e.active().map_or(Json::Null, model::to_json);
    text(json!({"available": true, "device": device}).to_string())
}

fn list(app: &mut App, _: &[Value]) -> Value {
    let e = &app.store.earbuds;
    let active = e.active().map(|d| d.key.clone());
    let rows: Vec<Json> = e
        .devices()
        .iter()
        .map(|d| {
            json!({
                "key": d.key,
                "backend": d.backend,
                "name": d.name,
                "connected": d.connected,
                "active": active.as_deref() == Some(d.key.as_str()),
            })
        })
        .collect();
    text(Json::Array(rows).to_string())
}

fn select(app: &mut App, args: &[Value]) -> Value {
    let key = args[0].str();
    if !app.store.earbuds.has(key) {
        return text(format!("error: no device '{key}'"));
    }
    let diff = earbuds::Diff::Select(key.to_owned());
    app.apply(store::Diff::Earbuds(diff));
    text("ok")
}

fn recenter(_: &mut App, _: &[Value]) -> Value {
    earbuds::recenter();
    text("ok")
}

fn set(app: &mut App, args: &[Value]) -> Value {
    match app.store.earbuds.plan(args[0].str(), args[1].str()) {
        Ok(op) => {
            if let Some(op) = op {
                earbuds::write(op);
            }
            text("ok")
        }
        Err(reason) => text(format!("error: {reason}")),
    }
}
