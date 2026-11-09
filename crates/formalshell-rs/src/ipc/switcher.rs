//! `switcher`: the whole summon path for the window
//! switcher. `switcher.enabled: false` answers every verb with the error
//! string rather than accepting a call that would do nothing.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::App;
use crate::wayland::switcher::OFF;

fn guard(app: &App) -> Result<(), String> {
    if app.switcher_enabled() { Ok(()) } else { Err(OFF.into()) }
}

fn next(app: &mut App, _: &[Value]) -> Reply {
    guard(app)?;
    app.switcher_step(1);
    ok()
}

fn prev(app: &mut App, _: &[Value]) -> Reply {
    guard(app)?;
    app.switcher_step(-1);
    ok()
}

fn commit(app: &mut App, _: &[Value]) -> Reply {
    guard(app)?;
    if app.switcher_commit() { ok() } else { Err("nothing to commit".into()) }
}

fn cancel(app: &mut App, _: &[Value]) -> Reply {
    guard(app)?;
    app.switcher_close();
    ok()
}

fn state(app: &mut App, _: &[Value]) -> Reply {
    guard(app)?;
    Ok(app.switcher_state())
}

pub fn target() -> Target<App> {
    Target {
        name: "switcher",
        functions: vec![
            Function { name: "next", params: &[], ret: Type::String, call: next },
            Function { name: "prev", params: &[], ret: Type::String, call: prev },
            Function { name: "commit", params: &[], ret: Type::String, call: commit },
            Function { name: "cancel", params: &[], ret: Type::String, call: cancel },
            Function { name: "state", params: &[], ret: Type::String, call: state },
        ],
    }
}
