//! `lights`: the keyboard's effect, colour, source, speed and
//! level. A value the service refuses, or a machine with no asusd, answers
//! an error string rather than "ok".

use fs_system::lights as model;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::lights::{self, Cmd};
use crate::wayland::App;

fn answer(app: &App, accepted: bool, what: &str, cmd: impl FnOnce() -> Cmd) -> Reply {
    lights::checked(&app.store.lights, accepted, what, cmd)?;
    ok()
}

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    lights::toggle(&app.store.lights)?;
    ok()
}

fn effect(app: &mut App, a: &[Value]) -> Reply {
    lights::effect(&app.store.lights, a[0].str())?;
    ok()
}

fn color(app: &mut App, a: &[Value]) -> Reply {
    let hex = model::normalize_hex(Some(a[0].str()));
    answer(app, !hex.is_empty(), a[0].str(), || Cmd::Colour(hex.clone()))
}

fn source(app: &mut App, a: &[Value]) -> Reply {
    let v = a[0].str();
    answer(app, model::SOURCES.contains(&v), v, || Cmd::Source(v.to_owned()))
}

fn speed(app: &mut App, a: &[Value]) -> Reply {
    let v = a[0].str();
    answer(app, model::SPEEDS.contains(&v), v, || Cmd::Speed(v.to_owned()))
}

fn brightness(app: &mut App, a: &[Value]) -> Reply {
    let v = a[0].str();
    let level = match v.as_bytes() {
        [d @ b'0'..=b'3'] => Some(i64::from(d - b'0')),
        _ => None,
    };
    answer(app, level.is_some(), v, || Cmd::Brightness(level.unwrap_or(0)))
}

fn refresh(_: &mut App, _: &[Value]) -> Reply {
    lights::command(Cmd::Refresh);
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.lights.status())
}

pub fn target() -> Target<App> {
    Target {
        name: "lights",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "effect", params: &[("id", Type::String)], ret: Type::String, call: effect },
            Function { name: "color", params: &[("hex", Type::String)], ret: Type::String, call: color },
            Function { name: "source", params: &[("value", Type::String)], ret: Type::String, call: source },
            Function { name: "speed", params: &[("value", Type::String)], ret: Type::String, call: speed },
            Function { name: "brightness", params: &[("level", Type::String)], ret: Type::String, call: brightness },
            Function { name: "refresh", params: &[], ret: Type::String, call: refresh },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
