//! `activity`: what is running, grouped by source (`services/activity.rs`),
//! and `seen`, which folds the failures that stay until looked at.

use super::registry::{Function, Target, Type, Value};
use crate::services::activity::{self, Input};
use crate::wayland::App;

fn status(app: &mut App, _: &[Value]) -> Value {
    Value::Str(app.store.activity.status())
}

fn seen(_: &mut App, _: &[Value]) -> Value {
    activity::send(Input::Seen);
    Value::Str("ok".into())
}

pub fn target() -> Target<App> {
    Target {
        name: "activity",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "seen", params: &[], ret: Type::String, call: seen },
        ],
    }
}
