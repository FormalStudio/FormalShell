//! `radio`: Radio Atlas's player for keybinds and for reading
//! it from outside the shell. `play` takes a saved station's id (a favourite
//! or a recent one), so a key can be bound to one station. Opening the atlas
//! is `panel toggle radio`.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::radio::{self, Cmd};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "radio",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "play", params: &[("id", Type::String)], ret: Type::String, call: play },
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "random", params: &[], ret: Type::String, call: random },
            Function { name: "stop", params: &[], ret: Type::String, call: stop },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    let mut status = app.store.media.radio.status();
    status["atlas"] = app.atlas.model.status(&app.store.media.radio);
    Ok(status)
}

fn play(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].str();
    let radio = &app.store.media.radio;
    for list in [&radio.favorites, &radio.recent] {
        if let Some(station) = list.iter().find(|s| s.uuid == id) {
            radio::send(Cmd::PlayFromSaved(station.clone(), list.clone()));
            return ok();
        }
    }
    Err(format!("no saved station {id}"))
}

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    if !app.store.media.radio.running() {
        return Err("nothing tuned".into());
    }
    radio::send(Cmd::Toggle);
    ok()
}

fn random(_: &mut App, _: &[Value]) -> Reply {
    radio::send(Cmd::Random);
    ok()
}

fn stop(_: &mut App, _: &[Value]) -> Reply {
    radio::send(Cmd::Stop);
    ok()
}
