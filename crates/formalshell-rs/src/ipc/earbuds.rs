//! `earbuds`: the active device as the panel renders it,
//! the device list, a pick, and a control change against the active device
//! (`set noise anc`, `set ca on`, `set adaptive 40`), refused with the
//! reason rather than dropped, and `recenter`, which zeroes the AirPods'
//! head yaw, pitch and roll.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::devices::earbuds;
use crate::store;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "earbuds",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "devices", params: &[], ret: Type::String, call: list },
            Function { name: "select", params: &[("key", Type::String)], ret: Type::String, call: select },
            Function { name: "recenter", params: &[], ret: Type::String, call: recenter },
            Function {
                name: "set",
                params: &[("control", Type::String), ("value", Type::String)],
                ret: Type::String,
                call: set,
            },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.earbuds.status())
}

fn list(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.earbuds.list())
}

fn select(app: &mut App, args: &[Value]) -> Reply {
    let key = args[0].str();
    if !app.store.earbuds.has(key) {
        return Err(format!("no device '{key}'"));
    }
    let diff = earbuds::Diff::Select(key.to_owned());
    app.apply(store::Diff::Earbuds(diff));
    ok()
}

fn recenter(_: &mut App, _: &[Value]) -> Reply {
    earbuds::send(earbuds::Cmd::Recenter);
    ok()
}

fn set(app: &mut App, args: &[Value]) -> Reply {
    match app.store.earbuds.plan(args[0].str(), args[1].str()) {
        Ok(op) => {
            if let Some(op) = op {
                earbuds::send(earbuds::Cmd::Op(op));
            }
            ok()
        }
        Err(reason) => Err(reason),
    }
}
