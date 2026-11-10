//! `settings`: the settings UI's keys over IPC. `set` and `reset` write
//! state.json's overrides, never settings.json; a key settings.json sets
//! refuses both.

use serde_json::Value as Json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::overrides;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "settings",
        reads_screen: false,
        functions: vec![
            Function { name: "get", params: &[("key", Type::String)], ret: Type::String, call: get },
            Function { name: "set", params: &[("key", Type::String), ("value", Type::String)], ret: Type::String, call: set },
            Function { name: "reset", params: &[("key", Type::String)], ret: Type::String, call: reset },
            Function { name: "list", params: &[], ret: Type::String, call: list },
        ],
    }
}

fn get(app: &mut App, args: &[Value]) -> Reply {
    match overrides::find(args[0].str()) {
        Some(key) => Ok(overrides::describe(&app.store, key)),
        None => Err(format!("unknown setting '{}'", args[0].str())),
    }
}

fn set(app: &mut App, args: &[Value]) -> Reply {
    overrides::set(&app.store, args[0].str(), overrides::parse_value(args[1].str()))?;
    ok()
}

fn reset(app: &mut App, args: &[Value]) -> Reply {
    overrides::reset(&app.store, args[0].str())?;
    ok()
}

fn list(app: &mut App, _: &[Value]) -> Reply {
    let all: Vec<Json> = overrides::KEYS.iter().map(|k| overrides::describe(&app.store, k)).collect();
    Ok(Json::Array(all))
}
