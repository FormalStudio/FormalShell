//! `overnight`: toggle, enable, disable, and what was
//! changed and will be put back (`restore`, null while off).

use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
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

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    let want = !active(app);
    on(app, want);
    ok()
}

fn enable(app: &mut App, _: &[Value]) -> Reply {
    on(app, true);
    ok()
}

fn disable(app: &mut App, _: &[Value]) -> Reply {
    on(app, false);
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    Ok(json!({"active": active(app), "restore": app.store.state.data.overnight}))
}
