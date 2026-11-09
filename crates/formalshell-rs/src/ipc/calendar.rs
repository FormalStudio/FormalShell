//! `calendar`'s `select` and `status`: the panel's own
//! day selection, headless. A panel that is not open answers from a fresh
//! one, which is what an idle panel holds until it opens.

use super::registry::{Function, Reply, Target, Type, Value, text};
use crate::surfaces::panel::Panel;
use crate::surfaces::panel::calendar::Calendar;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "calendar",
        functions: vec![
            Function { name: "select", params: &[("date", Type::String)], ret: Type::String, call: select },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

fn ask(app: &mut App, verb: &str, arg: &str) -> Reply {
    let reply = app.panel_call("calendar", verb, arg).or_else(|| Calendar::new(false).call(verb, arg));
    reply.unwrap_or_else(|| text(""))
}

fn select(app: &mut App, args: &[Value]) -> Reply {
    ask(app, "select", args[0].str())
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    ask(app, "status", "")
}
