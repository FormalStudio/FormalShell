//! `overnight`: toggle, enable, disable, and what was
//! changed and will be put back (`restore`, null while off).

use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::services::overnight;
use crate::wayland::App;

pub fn target() -> Target<App> {
    Target {
        name: "overnight",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "enable", params: &[], ret: Type::String, call: enable },
            Function { name: "disable", params: &[], ret: Type::String, call: disable },
            Function { name: "status", params: &[], ret: Type::String, call: status },
        ],
    }
}

fn active(app: &App) -> bool {
    app.store.state.data.overnight.is_some()
}

fn on(app: &mut App, want: bool) {
    if active(app) == want {
        return;
    }
    let snap = app.store.state.data.overnight.clone().unwrap_or_default();
    if let Some(rt) = &app.runtime {
        if want {
            rt.service(overnight::enable);
        } else {
            rt.service(move |ctx| overnight::disable(ctx, snap));
        }
    }
}

fn toggle(app: &mut App, _: &[Value]) -> Value {
    let want = !active(app);
    on(app, want);
    Value::Str("ok".into())
}

fn enable(app: &mut App, _: &[Value]) -> Value {
    on(app, true);
    Value::Str("ok".into())
}

fn disable(app: &mut App, _: &[Value]) -> Value {
    on(app, false);
    Value::Str("ok".into())
}

fn status(app: &mut App, _: &[Value]) -> Value {
    Value::Str(json!({"active": active(app), "restore": app.store.state.data.overnight}).to_string())
}
