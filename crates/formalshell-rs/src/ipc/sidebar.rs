//! `sidebar`: fold the sidebar to its rail and back (the state lands in
//! state.json), and `status`: its edge, width, the cards it drew and each
//! minimap row's workspace and windows; `focus` hands the minimap the
//! keyboard, refused on the rail. An error under the bar layout.

use super::registry::{Function, Target, Type, Value};
use crate::surfaces::sidebar;
use crate::wayland::App;

const NO_SIDEBAR: &str = "error: layout is not sidebar";

fn set(app: &mut App, collapsed: bool) -> Value {
    if app.bar.sidebar.is_none() {
        return Value::Str(NO_SIDEBAR.into());
    }
    sidebar::set_collapsed(collapsed);
    Value::Str("ok".into())
}

fn collapse(app: &mut App, _: &[Value]) -> Value {
    set(app, true)
}

fn expand(app: &mut App, _: &[Value]) -> Value {
    set(app, false)
}

fn toggle(app: &mut App, _: &[Value]) -> Value {
    let collapsed = !app.store.state.data.sidebar_collapsed;
    set(app, collapsed)
}

fn focus(app: &mut App, _: &[Value]) -> Value {
    if app.bar.sidebar.is_some() && app.store.state.data.sidebar_collapsed {
        return Value::Str("error: sidebar is collapsed".into());
    }
    Value::Str(if app.sidebar_focus(true) { "ok".into() } else { NO_SIDEBAR.into() })
}

fn status(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.bar.sidebar_status().map_or_else(|| NO_SIDEBAR.into(), |v| v.to_string()))
}

pub fn target() -> Target<App> {
    Target {
        name: "sidebar",
        functions: vec![
            Function { name: "collapse", params: &[], ret: Type::String, call: collapse },
            Function { name: "expand", params: &[], ret: Type::String, call: expand },
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "focus", params: &[], ret: Type::String, call: focus },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
