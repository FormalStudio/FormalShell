//! `dock`: what the dock holds and whether it is out, and what a pointer
//! does on it by an app's id: held out and put away, an app kept or let go,
//! its icon clicked, its menu opened and the app quit.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "dock",
        reads_screen: false,
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "open", params: &[], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "pin", params: &[("id", Type::String)], ret: Type::String, call: pin },
            Function { name: "unpin", params: &[("id", Type::String)], ret: Type::String, call: unpin },
            Function { name: "activate", params: &[("id", Type::String)], ret: Type::String, call: activate },
            Function { name: "menu", params: &[("id", Type::String)], ret: Type::String, call: menu },
            Function { name: "quit", params: &[("id", Type::String)], ret: Type::String, call: quit },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.dock_status())
}

fn open(app: &mut App, _: &[Value]) -> Reply {
    app.dock_open()?;
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.dock_close();
    ok()
}

fn pin(app: &mut App, args: &[Value]) -> Reply {
    app.dock_pin(args[0].str(), true)?;
    ok()
}

fn unpin(app: &mut App, args: &[Value]) -> Reply {
    app.dock_pin(args[0].str(), false)?;
    ok()
}

fn activate(app: &mut App, args: &[Value]) -> Reply {
    app.dock_activate_app(args[0].str())?;
    ok()
}

fn menu(app: &mut App, args: &[Value]) -> Reply {
    app.dock_menu_for(args[0].str())?;
    ok()
}

fn quit(app: &mut App, args: &[Value]) -> Reply {
    app.dock_quit(args[0].str())?;
    ok()
}
