//! `mirror`: the launcher's camera route opened, closed and
//! stepped, and what it shows.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::surfaces::launcher::MIRROR_ROUTE;
use crate::wayland::App;

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    if app.mirror_showing() {
        app.menu_close();
    } else {
        app.menu_open(Some(MIRROR_ROUTE));
    }
    ok()
}

fn open(app: &mut App, _: &[Value]) -> Reply {
    app.menu_open(Some(MIRROR_ROUTE));
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    if app.mirror_showing() {
        app.menu_close();
    }
    ok()
}

fn step(app: &mut App, delta: i64) -> Reply {
    if app.mirror_cycle(delta) { ok() } else { Err("mirror is not showing".into()) }
}

fn next(app: &mut App, _: &[Value]) -> Reply {
    step(app, 1)
}

fn previous(app: &mut App, _: &[Value]) -> Reply {
    step(app, -1)
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.mirror_status())
}

pub fn target() -> Target<App> {
    Target {
        name: "mirror",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "open", params: &[], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "next", params: &[], ret: Type::String, call: next },
            Function { name: "previous", params: &[], ret: Type::String, call: previous },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
