//! `settings`: the settings UI's keys over IPC. `set` and `reset` write
//! state.json's overrides, never settings.json; a key settings.json sets
//! refuses both.

use serde_json::Value as Json;

use super::registry::{Function, Target, Type, Value};
use crate::services::overrides;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "settings",
        functions: vec![
            Function { name: "get", params: &[("key", Type::String)], ret: Type::String, call: get },
            Function { name: "set", params: &[("key", Type::String), ("value", Type::String)], ret: Type::String, call: set },
            Function { name: "reset", params: &[("key", Type::String)], ret: Type::String, call: reset },
            Function { name: "list", params: &[], ret: Type::String, call: list },
        ],
    }
}

fn get(app: &mut App, args: &[Value]) -> Value {
    match overrides::find(args[0].str()) {
        Some(key) => text(overrides::describe(&app.store, key).to_string()),
        None => text(format!("error: unknown setting '{}'", args[0].str())),
    }
}

fn set(app: &mut App, args: &[Value]) -> Value {
    match overrides::set(&app.store, args[0].str(), overrides::parse_value(args[1].str())) {
        Ok(()) => text("ok"),
        Err(e) => text(e),
    }
}

fn reset(app: &mut App, args: &[Value]) -> Value {
    match overrides::reset(&app.store, args[0].str()) {
        Ok(()) => text("ok"),
        Err(e) => text(e),
    }
}

fn list(app: &mut App, _: &[Value]) -> Value {
    let all: Vec<Json> = overrides::KEYS.iter().map(|k| overrides::describe(&app.store, k)).collect();
    text(Json::Array(all).to_string())
}
