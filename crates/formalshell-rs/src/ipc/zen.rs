//! `zen`: the focused window alone on an empty workspace, floating and
//! centred, with the sidebar on its rail (`enter`), put back where it was
//! (`exit`), `toggle`, and `status`: `on`, and while on the window, the
//! workspace it goes back to and the window whose column it goes before.

use super::registry::{Function, Target, Type, Value};
use crate::wayland::App;

fn enter(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.zen_enter())
}

fn exit(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.zen_exit())
}

fn toggle(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.zen_toggle())
}

fn status(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.zen_status())
}

pub fn target() -> Target<App> {
    Target {
        name: "zen",
        functions: vec![
            Function { name: "enter", params: &[], ret: Type::String, call: enter },
            Function { name: "exit", params: &[], ret: Type::String, call: exit },
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
