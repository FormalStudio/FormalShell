//! `power`: leaving the session. Suspend, reboot and shutdown are the
//! launcher's rows, asked of logind (`services/power.rs`).

use super::registry::{Function, Target, Type, Value};
use crate::wayland::App;

/// The departure veil, then the compositor's exit.
fn logout(app: &mut App, _: &[Value]) -> Value {
    app.logout();
    Value::Str("ok".into())
}

pub fn target() -> Target<App> {
    Target { name: "power", functions: vec![Function { name: "logout", params: &[], ret: Type::String, call: logout }] }
}
