//! `gallery`: the verbs over the dev sheet.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

fn open(app: &mut App, _: &[Value]) -> Reply {
    app.set_gallery(true);
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.set_gallery(false);
    ok()
}

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    let open = !app.gallery_open();
    app.set_gallery(open);
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(serde_json::json!({ "isOpen": app.gallery_open() }))
}

pub fn target() -> Target<App> {
    Target {
        name: "gallery",
        functions: vec![
            Function { name: "open", params: &[], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
