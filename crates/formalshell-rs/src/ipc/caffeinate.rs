//! `caffeinate`: toggle, enable and disable the idle
//! inhibitor, and `status` (`inhibiting` is the surface's own word that the
//! Wayland inhibitor is up, not a restatement of `active`).

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    let on = !app.store.caffeinate.active;
    app.set_caffeinated(on);
    ok()
}

fn enable(app: &mut App, _: &[Value]) -> Reply {
    app.set_caffeinated(true);
    ok()
}

fn disable(app: &mut App, _: &[Value]) -> Reply {
    app.set_caffeinated(false);
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.caffeinate.status())
}

pub fn target() -> Target<App> {
    Target {
        name: "caffeinate",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "enable", params: &[], ret: Type::String, call: enable },
            Function { name: "disable", params: &[], ret: Type::String, call: disable },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
