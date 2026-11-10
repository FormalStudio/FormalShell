//! `plugins`: `list` (the resolved manifests), `status`
//! (what loaded, what failed and what the scan warned about) and `reload`
//! (scan again, restart every plugin). Summoning a plugin's panel stays on
//! the `panel` target under its `plugin:<id>` name.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::plugins;
use crate::wayland::App;

fn list(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.plugins.list())
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.plugins.status())
}

fn reload(_: &mut App, _: &[Value]) -> Reply {
    plugins::reload();
    ok()
}

pub fn target() -> Target<App> {
    Target {
        name: "plugins",
        reads_screen: false,
        functions: vec![
            Function { name: "list", params: &[], ret: Type::String, call: list },
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "reload", params: &[], ret: Type::String, call: reload },
        ],
    }
}
