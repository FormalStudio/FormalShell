//! `sidebar`: fold the sidebar to its rail and back (the state lands in
//! state.json), and `status`: its edge, width, the cards it drew and each
//! minimap row's workspace and windows; `focus` hands the minimap the
//! keyboard, refused on the rail. An error under the bar layout.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::surfaces::sidebar;
use crate::wayland::{App, live};

const NO_SIDEBAR: &str = "layout is not sidebar";

fn set(app: &mut App, collapsed: bool) -> Reply {
    if live!(app).bar.sidebar().is_none() {
        return Err(NO_SIDEBAR.into());
    }
    sidebar::set_collapsed(collapsed);
    ok()
}

fn collapse(app: &mut App, _: &[Value]) -> Reply {
    set(app, true)
}

fn expand(app: &mut App, _: &[Value]) -> Reply {
    set(app, false)
}

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    let collapsed = !app.store.state.data.sidebar_collapsed;
    set(app, collapsed)
}

fn focus(app: &mut App, _: &[Value]) -> Reply {
    if live!(app).bar.sidebar().is_some() && app.store.state.data.sidebar_collapsed {
        return Err("sidebar is collapsed".into());
    }
    if app.sidebar_focus(true) { ok() } else { Err(NO_SIDEBAR.into()) }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    live!(app).bar.sidebar_status().ok_or_else(|| NO_SIDEBAR.into())
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
