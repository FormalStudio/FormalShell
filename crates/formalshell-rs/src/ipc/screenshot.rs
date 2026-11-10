//! `screenshot`: full, region and the picker's pick,
//! its headless keys and status, the editor, and cancel.

use super::registry::{Function, Json, Reply, Target, Type, Value, ok};
use crate::wayland::capture_host as cap;
use crate::wayland::App;

const PROCESSING: (&str, Type) = ("processing", Type::String);

fn full(app: &mut App, a: &[Value]) -> Reply {
    cap::shot_start(app, false, cap::Processing::parse(a[0].str())).map(Json::String)
}

fn region(app: &mut App, a: &[Value]) -> Reply {
    cap::shot_start(app, true, cap::Processing::parse(a[0].str())).map(Json::String)
}

fn cancel(app: &mut App, _: &[Value]) -> Reply {
    cap::shot_cancel(app, "cancelled on demand")?;
    ok()
}

fn pick(app: &mut App, a: &[Value]) -> Reply {
    cap::shot_pick(app, a[0].str(), cap::Processing::parse(a[1].str())).map(Json::String)
}

fn key(app: &mut App, a: &[Value]) -> Reply {
    cap::picker_key(app, a[0].str())?;
    ok()
}

fn picker_status(app: &mut App, _: &[Value]) -> Reply {
    Ok(cap::picker_status(app))
}

fn edit(app: &mut App, a: &[Value]) -> Reply {
    let path = if a[0].str().is_empty() { app.capture.shot.last_path.clone() } else { a[0].str().to_owned() };
    cap::shot_edit(app, &path)?;
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(cap::shot_status(app))
}

pub fn target() -> Target<App> {
    Target {
        name: "screenshot",
        functions: vec![
            Function { name: "full", params: &[PROCESSING], ret: Type::String, call: full },
            Function { name: "region", params: &[PROCESSING], ret: Type::String, call: region },
            Function { name: "cancel", params: &[], ret: Type::String, call: cancel },
            Function { name: "pick", params: &[("mode", Type::String), PROCESSING], ret: Type::String, call: pick },
            Function { name: "key", params: &[("name", Type::String)], ret: Type::String, call: key },
            Function { name: "pickerStatus", params: &[], ret: Type::String, call: picker_status },
            Function { name: "edit", params: &[("path", Type::String)], ret: Type::String, call: edit },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
