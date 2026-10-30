//! `activity`: what is running, grouped by source (`services/activity.rs`),
//! `seen`, which folds the failures that stay until looked at, a row's own
//! verbs by source (`surfaces/activity.rs`), and `launch`, a desktop entry
//! spawned with its launch shown (`services/launch.rs`), for a keybind, and
//! `openApp`, the app an install brought, off its row's source.

use super::registry::{Function, Target, Type, Value};
use crate::services::activity::{self, Input};
use crate::surfaces::activity::{self as row, Done, Verb};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.store.activity.status())
}

fn seen(_: &mut App, _: &[Value]) -> Value {
    activity::send(Input::Seen);
    Value::Str("ok".into())
}

fn verb(app: &mut App, verb: Verb, source: &str) -> Value {
    match row::act(&app.store, app.runtime.as_ref(), verb, source) {
        Ok(Done::Panel) => {
            if app.panel_open() != Some("activity") {
                app.set_panel("activity", true, None);
            }
            app.panel_call("activity", "select", source);
            Value::Str("ok".into())
        }
        Ok(Done::Nothing) => Value::Str("ok".into()),
        Err(e) => Value::Str(e),
    }
}

fn open(app: &mut App, args: &[Value]) -> Value {
    verb(app, Verb::Open, args[0].str())
}

fn dismiss(app: &mut App, args: &[Value]) -> Value {
    verb(app, Verb::Dismiss, args[0].str())
}

fn retry(app: &mut App, args: &[Value]) -> Value {
    verb(app, Verb::Retry, args[0].str())
}

fn open_app(app: &mut App, args: &[Value]) -> Value {
    verb(app, Verb::OpenApp, args[0].str())
}

fn launch(app: &mut App, args: &[Value]) -> Value {
    let id = args[0].str();
    let Some(argv) = crate::services::menu::launch_argv(&app.store.menu, id) else {
        return Value::Str(format!("error: no desktop entry '{id}'"));
    };
    crate::services::launch::spawn(&app.store, id, &argv);
    Value::Str("ok".into())
}

pub fn target() -> Target<App> {
    Target {
        name: "activity",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "seen", params: &[], ret: Type::String, call: seen },
            Function { name: "open", params: &[("source", Type::String)], ret: Type::String, call: open },
            Function { name: "dismiss", params: &[("source", Type::String)], ret: Type::String, call: dismiss },
            Function { name: "retry", params: &[("source", Type::String)], ret: Type::String, call: retry },
            Function { name: "launch", params: &[("desktopId", Type::String)], ret: Type::String, call: launch },
            Function { name: "openApp", params: &[("source", Type::String)], ret: Type::String, call: open_app },
        ],
    }
}
