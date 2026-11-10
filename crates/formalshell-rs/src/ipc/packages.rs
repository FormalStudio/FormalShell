//! `packages`: what the shell asked fspec of (`services/packages.rs`):
//! the last search answered, each install or uninstall it started with its
//! operation and the app it brought, and the owner answers by desktop id;
//! the Software card's spec check, and its Apply and Undo.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::packages::{self, Change, Cmd};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.packages.status(app.store.activity.fspec.installed))
}

fn send(cmd: Cmd) -> Reply {
    packages::send(cmd);
    ok()
}

fn check(_: &mut App, _: &[Value]) -> Reply {
    send(Cmd::Check)
}

fn apply(_: &mut App, _: &[Value]) -> Reply {
    send(Cmd::Change(Change::Apply))
}

fn undo(_: &mut App, _: &[Value]) -> Reply {
    send(Cmd::Change(Change::Undo))
}

pub fn target() -> Target<App> {
    Target {
        name: "packages",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "check", params: &[], ret: Type::String, call: check },
            Function { name: "apply", params: &[], ret: Type::String, call: apply },
            Function { name: "undo", params: &[], ret: Type::String, call: undo },
        ],
    }
}
