//! `menu`: the summon routes, the rig's stand-ins for
//! Enter and typing, and the select/input modes, whose answers land in
//! `menu-selection.txt` as `{token, value}` or `{token, cancelled: true}`.

use super::registry::{Function, Target, Type, Value};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "menu",
        functions: vec![
            Function { name: "toggle", params: &[], ret: Type::String, call: toggle },
            Function { name: "summon", params: &[("route", Type::String)], ret: Type::String, call: summon },
            Function { name: "activate", params: &[("index", Type::Int)], ret: Type::String, call: activate },
            Function { name: "activateAlternate", params: &[("index", Type::Int)], ret: Type::String, call: activate_alternate },
            Function { name: "filter", params: &[("text", Type::String)], ret: Type::String, call: filter },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "refresh", params: &[], ret: Type::String, call: refresh },
            Function { name: "ping", params: &[], ret: Type::String, call: |_, _| text("pong") },
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "abbreviations", params: &[], ret: Type::String, call: abbreviations },
            Function {
                name: "select",
                params: &[("prompt", Type::String), ("optionsJson", Type::String), ("token", Type::String)],
                ret: Type::String,
                call: select,
            },
            Function { name: "input", params: &[("prompt", Type::String), ("token", Type::String)], ret: Type::String, call: input },
        ],
    }
}

fn toggle(app: &mut App, _: &[Value]) -> Value {
    app.menu_toggle();
    text("ok")
}

fn summon(app: &mut App, args: &[Value]) -> Value {
    app.menu_open(Some(args[0].str()));
    text("ok")
}

fn activate(app: &mut App, args: &[Value]) -> Value {
    let i = usize::try_from(args[0].int()).unwrap_or(usize::MAX);
    text(if app.menu_activate(i, false) { "ok" } else { "error: menu not open" })
}

fn activate_alternate(app: &mut App, args: &[Value]) -> Value {
    let i = usize::try_from(args[0].int()).unwrap_or(usize::MAX);
    text(if app.menu_activate(i, true) { "ok" } else { "error: menu not open" })
}

fn filter(app: &mut App, args: &[Value]) -> Value {
    text(if app.menu_filter(args[0].str()) { "ok" } else { "error: menu not open" })
}

fn close(app: &mut App, _: &[Value]) -> Value {
    app.menu_close();
    text("ok")
}

fn refresh(app: &mut App, _: &[Value]) -> Value {
    app.menu_refresh();
    text("ok")
}

fn status(app: &mut App, _: &[Value]) -> Value {
    app.resolve_launcher();
    let mut v = app.launcher.status(&app.store);
    if let Some(r) = app.launcher_body() {
        v["body"] = serde_json::json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h});
    }
    text(v.to_string())
}

fn abbreviations(app: &mut App, _: &[Value]) -> Value {
    use crate::surfaces::launcher as l;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64);
    text(fs_menu::abbrev::report(&l::configured_abbreviations(&app.store), &l::abbreviation_book(&app.store), now).to_string())
}

fn select(app: &mut App, args: &[Value]) -> Value {
    let options = match serde_json::from_str::<serde_json::Value>(args[1].str()) {
        Ok(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => return text("error: optionsJson must be a JSON array"),
    };
    app.menu_select(args[0].str(), options, args[2].str());
    text("ok")
}

fn input(app: &mut App, args: &[Value]) -> Value {
    app.menu_input(args[0].str(), args[1].str());
    text("ok")
}
