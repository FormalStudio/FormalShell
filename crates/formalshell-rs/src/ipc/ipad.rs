//! `ipad`: padbridge's state (`status`, `available: false` and nothing else
//! while it is not on the bus), and its three calls. `position` moves the
//! iPad to another edge keeping its stretch, `stretch` moves it along its
//! edge; both reach padbridge as one `SetPosition`.

use fs_types::Edge;

use super::registry::{Function, Target, Type, Value};
use crate::services::ipad::{self, Cmd, Ipad, Stretch};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "ipad",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: |app, _| text(app.store.ipad.status()) },
            Function { name: "switch", params: &[], ret: Type::String, call: |app, _| send(app, |_| Ok(Cmd::Switch)) },
            Function { name: "reconnect", params: &[], ret: Type::String, call: |app, _| send(app, |_| Ok(Cmd::Reconnect)) },
            Function { name: "position", params: &[("edge", Type::String)], ret: Type::String, call: position },
            Function { name: "stretch", params: &[("start", Type::Real), ("end", Type::Real)], ret: Type::String, call: stretch },
        ],
    }
}

fn send(app: &App, cmd: impl FnOnce(&Ipad) -> Result<Cmd, String>) -> Value {
    let Some(i) = &app.store.ipad.bridge else { return text("error: padbridge is not running") };
    match cmd(i) {
        Ok(cmd) => {
            ipad::command(cmd);
            text("ok")
        }
        Err(err) => text(format!("error: {err}")),
    }
}

fn position(app: &mut App, args: &[Value]) -> Value {
    let edge = args[0].str();
    send(app, |i| Edge::parse(edge).map(|e| Cmd::Position(e, i.stretch)).ok_or_else(|| format!("edge must be left, right, top or bottom, not '{edge}'")))
}

fn stretch(app: &mut App, args: &[Value]) -> Value {
    let (start, end) = (args[0].real(), args[1].real());
    send(app, |i| Stretch::new(start, end).map(|s| Cmd::Position(i.edge, s)).ok_or_else(|| "stretch must be 0 <= start < end <= 1".to_owned()))
}
