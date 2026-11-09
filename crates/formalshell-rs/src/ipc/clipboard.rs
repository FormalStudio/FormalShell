//! `clipboard`: the ledger as JSON, and the three verbs
//! the launcher's rows use too. Each answers "ok" whether or not the id
//! named an entry.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::clipboard::{self, Cmd};
use crate::wayland::App;

fn list(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.clipboard.list())
}

fn copy(_: &mut App, a: &[Value]) -> Reply {
    clipboard::command(Cmd::Copy(a[0].str().to_owned()));
    ok()
}

fn remove(_: &mut App, a: &[Value]) -> Reply {
    clipboard::command(Cmd::Remove(a[0].str().to_owned()));
    ok()
}

fn clear(_: &mut App, _: &[Value]) -> Reply {
    clipboard::command(Cmd::Clear);
    ok()
}

pub fn target() -> Target<App> {
    Target {
        name: "clipboard",
        functions: vec![
            Function { name: "list", params: &[], ret: Type::String, call: list },
            Function { name: "copy", params: &[("id", Type::String)], ret: Type::String, call: copy },
            Function { name: "remove", params: &[("id", Type::String)], ret: Type::String, call: remove },
            Function { name: "clear", params: &[], ret: Type::String, call: clear },
        ],
    }
}
