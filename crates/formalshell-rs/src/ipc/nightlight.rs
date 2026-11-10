//! `nightlight`: toggle, enable and disable the
//! wlsunset-backed night light, and `status`.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::nightlight;
use crate::wayland::App;

fn toggle(_: &mut App, _: &[Value]) -> Reply {
    nightlight::toggle();
    ok()
}

fn enable(_: &mut App, _: &[Value]) -> Reply {
    nightlight::enable();
    ok()
}

fn disable(_: &mut App, _: &[Value]) -> Reply {
    nightlight::disable();
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.nightlight.status())
}

pub fn target() -> Target<App> {
    Target {
        name: "nightlight",
        reads_screen: false,
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "enable", params: &[], ret: Type::String, call: enable },
            Function { name: "disable", params: &[], ret: Type::String, call: disable },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
