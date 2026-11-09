//! `console`: the quake console's toggle, show and hide,
//! and `status`, whose `windowId` is "" when no console window exists.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    app.console_toggle();
    ok()
}

fn show(app: &mut App, _: &[Value]) -> Reply {
    app.console_show();
    ok()
}

fn hide(app: &mut App, _: &[Value]) -> Reply {
    app.console_hide();
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.console_status())
}

pub fn target() -> Target<App> {
    Target {
        name: "console",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "show", params: &[], ret: Type::String, call: show },
            Function { name: "hide", params: &[], ret: Type::String, call: hide },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
