//! `picker`: the wallpaper route summoned, the generic image
//! selector opened on a directory, `choose` doing what Enter on a tile
//! does and `chooseFor` what its "Use in … mode only" does.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

fn summon(app: &mut App, _: &[Value]) -> Reply {
    app.picker_summon();
    ok()
}

fn select(app: &mut App, a: &[Value]) -> Reply {
    app.picker_select(a[0].str(), a[1].str());
    ok()
}

fn choose(app: &mut App, a: &[Value]) -> Reply {
    app.resolve_launcher();
    if app.picker_choose(a[0].str()) {
        ok()
    } else {
        Err("not on the picker route, or path not in the current listing".into())
    }
}

fn choose_for(app: &mut App, a: &[Value]) -> Reply {
    let Some(half) = fs_theme::theme::Mode::parse_exact(a[1].str()) else {
        return Err("mode must be light or dark".into());
    };
    app.resolve_launcher();
    if app.picker_choose_for(a[0].str(), half) {
        ok()
    } else {
        Err("not on the picker route, or path not a single in the current listing".into())
    }
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.menu_close();
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.launcher.picker_status(&app.store))
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
