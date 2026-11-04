//! `picker`: the wallpaper route summoned, the generic image
//! selector opened on a directory, `choose` doing what Enter on a tile
//! does and `chooseFor` what its "Use in … mode only" does.

use super::registry::{Function, Target, Type, Value};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn summon(app: &mut App, _: &[Value]) -> Value {
    app.picker_summon();
    text("ok")
}

fn select(app: &mut App, a: &[Value]) -> Value {
    app.picker_select(a[0].str(), a[1].str());
    text("ok")
}

fn choose(app: &mut App, a: &[Value]) -> Value {
    app.resolve_launcher();
    if app.picker_choose(a[0].str()) {
        text("ok")
    } else {
        text("error: not on the picker route, or path not in the current listing")
    }
}

fn choose_for(app: &mut App, a: &[Value]) -> Value {
    let Some(half) = crate::services::state::Half::parse(a[1].str()) else {
        return text("error: mode must be light or dark");
    };
    app.resolve_launcher();
    if app.picker_choose_for(a[0].str(), half) {
        text("ok")
    } else {
        text("error: not on the picker route, or path not a single in the current listing")
    }
}

fn close(app: &mut App, _: &[Value]) -> Value {
    app.menu_close();
    text("ok")
}

fn status(app: &mut App, _: &[Value]) -> Value {
    text(app.launcher.picker_status(&app.store).to_string())
}

pub fn target() -> Target<App> {
    Target {
        name: "picker",
        functions: vec![
            Function { name: "summon", params: &[], ret: Type::String, call: summon },
            Function { name: "select", params: &[("directory", Type::String), ("token", Type::String)], ret: Type::String, call: select },
            Function { name: "choose", params: &[("path", Type::String)], ret: Type::String, call: choose },
            Function { name: "chooseFor", params: &[("path", Type::String), ("mode", Type::String)], ret: Type::String, call: choose_for },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
