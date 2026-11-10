//! `localsend`: the receiver's and the last scan's
//! state, a scan, and one file sent to a peer the last scan found.

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::localsend::{self, Cmd, Payload};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.localsend.status())
}

fn peers(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.store.localsend.peers_json())
}

fn scan(app: &mut App, _: &[Value]) -> Reply {
    if !app.store.localsend.installed {
        return Err("localsend-cli is not installed".into());
    }
    localsend::command(Cmd::Scan { force: true });
    ok()
}

fn send(app: &mut App, a: &[Value]) -> Reply {
    let name = a[0].str();
    let Some(peer) = fs_devices::localsend::resolve_peer(&app.store.localsend.peers, name).cloned() else {
        let body = format!("'{name}' is not in the last scan, rescan and try again");
        app.apply(crate::services::notifications::Op::Notify("LOCALSEND FAILED".into(), body, fs_info::notifications::Urgency::Critical));
        return Err(format!("unknown peer '{name}'"));
    };
    localsend::command(Cmd::Send { peer, payload: Payload::Path(a[1].str().to_owned()) });
    ok()
}

pub fn target() -> Target<App> {
    Target {
        name: "localsend",
        reads_screen: false,
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "peers", params: &[], ret: Type::String, call: peers },
            Function { name: "scan", params: &[], ret: Type::String, call: scan },
            Function { name: "send", params: &[("peer", Type::String), ("path", Type::String)], ret: Type::String, call: send },
        ],
    }
}
