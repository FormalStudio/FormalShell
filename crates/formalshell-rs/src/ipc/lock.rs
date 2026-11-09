//! `lock`: `lock`, `session` (the lock asked of logind), `isLocked` and
//! `status`. No unlock verb: the
//! password typed into the surface is the only way out.

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::wayland::App;

fn lock(app: &mut App, _: &[Value]) -> Reply {
    app.lock()?;
    ok()
}

/// "unknown" while an external locker owns the session: it never reports back.
fn is_locked(app: &mut App, _: &[Value]) -> Reply {
    if app.lock_external() {
        return text("unknown");
    }
    text(app.lock.locked.to_string())
}

fn session(app: &mut App, _: &[Value]) -> Reply {
    app.lock_session()?;
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(app.lock_status())
}

pub fn target() -> Target<App> {
    Target {
        name: "lock",
        functions: vec![
            Function { name: "lock", params: &[], ret: Type::String, call: lock },
            Function { name: "session", params: &[], ret: Type::String, call: session },
            Function { name: "isLocked", params: &[], ret: Type::String, call: is_locked },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}
