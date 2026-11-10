//! `capture`: text (OCR) and colour off a slurp selection
//! or a geometry the caller already has, cancel and status.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::capture_host as cap;
use crate::wayland::App;

fn grab_text(app: &mut App, _: &[Value]) -> Reply {
    cap::grab_start(app, "text", "")?;
    ok()
}

fn color(app: &mut App, _: &[Value]) -> Reply {
    cap::grab_start(app, "color", "")?;
    ok()
}

fn text_at(app: &mut App, a: &[Value]) -> Reply {
    cap::grab_start(app, "text", a[0].str())?;
    ok()
}

fn color_at(app: &mut App, a: &[Value]) -> Reply {
    cap::grab_start(app, "color", a[0].str())?;
    ok()
}

fn cancel(app: &mut App, _: &[Value]) -> Reply {
    cap::grab_cancel(app, "cancelled on demand")?;
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(cap::grab_status(app))
}

pub fn target() -> Target<App> {
    Target {
        name: "capture",
        functions: vec![
            Function { name: "text", params: &[], ret: Type::String, call: grab_text },
            Function { name: "color", params: &[], ret: Type::String, call: color },
            Function { name: "textAt", params: &[("geometry", Type::String)], ret: Type::String, call: text_at },
            Function { name: "colorAt", params: &[("geometry", Type::String)], ret: Type::String, call: color_at },
            Function { name: "cancel", params: &[], ret: Type::String, call: cancel },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
