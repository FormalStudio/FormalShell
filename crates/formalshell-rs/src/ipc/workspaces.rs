//! `workspaces peek <n>|close|overview <open|close|toggle>|status`:
//! `overview` is the workspace overview (`wayland::overview`). `peek` opens the
//! preview of workspace `n` (the number on its chip) off the Spaces cell,
//! the pointer's hover open for a keybind or the rig; `status` is the chips
//! the cell resolved and the preview's state.

use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::overview::Action;
use crate::wayland::{App, live};

pub fn target() -> Target<App> {
    Target {
        name: "workspaces",
        reads_screen: false,
        functions: vec![
            Function { name: "peek", params: &[("n", Type::Int)], ret: Type::String, call: peek },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "overview", params: &[("action", Type::String)], ret: Type::String, call: overview },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

fn peek(app: &mut App, args: &[Value]) -> Reply {
    app.preview_peek(args[0].int() as i64)?;
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.preview_close();
    ok()
}

fn overview(app: &mut App, args: &[Value]) -> Reply {
    let action = match args[0].str() {
        "open" => Action::Open,
        "close" => Action::Close,
        "toggle" => Action::Toggle,
        other => return Err(format!("overview takes open, close or toggle, not {other:?}")),
    };
    app.overview_action(action);
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    let Some(cell) = live!(app).bar.workspaces_status() else {
        return Err("no workspaces cell on any bar".into());
    };
    Ok(json!({"output": cell["output"], "slots": cell["slots"], "preview": app.preview_status()}))
}
