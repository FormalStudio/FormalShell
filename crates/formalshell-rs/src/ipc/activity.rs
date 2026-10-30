//! `activity`: what is running, grouped by source (`services/activity.rs`),
//! `seen`, which folds the failures that stay until looked at, and a row's
//! own verbs by source (`surfaces/activity.rs`).

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
            app.set_panel("activity", true, None);
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

pub fn target() -> Target<App> {
    Target {
        name: "activity",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "seen", params: &[], ret: Type::String, call: seen },
            Function { name: "open", params: &[("source", Type::String)], ret: Type::String, call: open },
            Function { name: "dismiss", params: &[("source", Type::String)], ret: Type::String, call: dismiss },
            Function { name: "retry", params: &[("source", Type::String)], ret: Type::String, call: retry },
        ],
    }
}
