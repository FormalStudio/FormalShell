//! `iphone`: `status` answers exactly what the panel
//! renders from, `installed: false` and nothing else when the bridge has
//! never answered a line. The rest drive the service the way the panel does.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::iphone::{self, Cmd};
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "iphone",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "pair", params: &[], ret: Type::String, call: pair },
            Function { name: "invoke", params: &[("id", Type::String), ("action", Type::String)], ret: Type::String, call: invoke },
            Function { name: "dismiss", params: &[("id", Type::String)], ret: Type::String, call: dismiss },
            Function { name: "clear", params: &[], ret: Type::String, call: clear },
            Function { name: "markRead", params: &[], ret: Type::String, call: mark_read },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.iphone.status(&app.store.media.ams))
}

fn pair(app: &mut App, _: &[Value]) -> Reply {
    if !app.store.iphone.installed {
        return Err("no bridge on PATH".into());
    }
    iphone::command(Cmd::Pair);
    ok()
}

/// Whatever does not parse as a number names nothing.
fn number(id: &str) -> Option<i64> {
    id.trim().parse::<f64>().ok().filter(|n| n.fract() == 0.0).map(|n| n as i64)
}

fn invoke(app: &mut App, args: &[Value]) -> Reply {
    let action = args[1].str();
    if action != "positive" && action != "negative" {
        return Err("action must be 'positive' or 'negative'".into());
    }
    let id = number(args[0].str());
    match id.filter(|id| iphone::actionable(&app.store.iphone, *id)) {
        Some(id) => {
            iphone::command(Cmd::Invoke { id, positive: action == "positive" });
            ok()
        }
        None => Err(format!("unknown notification '{}'", args[0].str())),
    }
}

fn dismiss(app: &mut App, args: &[Value]) -> Reply {
    let state = &app.store.iphone;
    let found = number(args[0].str()).and_then(|id| state.recent.iter().find(|e| e.id == id).map(|e| (id, e.negative_action.is_empty())));
    match found.filter(|(id, bare)| *bare || iphone::actionable(state, *id)) {
        Some((id, _)) => {
            iphone::command(Cmd::Dismiss(id));
            ok()
        }
        None => Err(format!("unknown notification '{}'", args[0].str())),
    }
}

fn clear(_: &mut App, _: &[Value]) -> Reply {
    iphone::command(Cmd::Clear);
    ok()
}

fn mark_read(_: &mut App, _: &[Value]) -> Reply {
    iphone::command(Cmd::MarkRead);
    ok()
}
