//! `quicksettings`: the card's controls as the store has them and as last
//! drawn (`status`), and Enter on one of them (`activate`), through the
//! same path as the keyboard.

use serde_json::{Value as Json, json};

use super::debug::rect;
use super::registry::{Function, Target, Type, Value};
use crate::surfaces::panel::quick::{self, ID};
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "quicksettings",
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "activate", params: &[("id", Type::String)], ret: Type::String, call: activate },
        ],
    }
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let host = app.panel.as_ref().filter(|h| h.is_open() && h.id() == ID);
    let boxes = host.map(|h| h.quick_boxes()).unwrap_or_default();
    let boxed = |key: &str| boxes.iter().find(|(k, _)| k == key).map_or(Json::Null, |(_, r)| rect(r));
    let rows: Vec<Json> = quick::tiles(&app.store)
        .into_iter()
        .map(|t| {
            json!({
                "id": t.id,
                "kind": t.kind.as_str(),
                "title": t.title,
                "subtitle": t.subtitle,
                "on": t.on,
                "available": t.available(),
                "reason": t.reason,
                "rect": boxed(t.id),
                "arrow": boxed(&format!("{}:menu", t.id)),
            })
        })
        .collect();
    let out = json!({
        "open": host.is_some(),
        "from": host.map(|_| "ipc"),
        "card": host.map_or(Json::Null, |h| rect(&h.card.rest_rect())),
        "cursor": host.and_then(|h| h.viewport().3.map(|(k, _)| k.to_owned())),
        "rows": rows,
        "menu": Json::Null,
        "dim": false,
    });
    text(out.to_string())
}

fn activate(app: &mut App, args: &[Value]) -> Value {
    let id = args[0].str();
    if !app.panel.as_ref().is_some_and(|h| h.is_open() && h.id() == ID) {
        return text("error: quick settings is not open");
    }
    match quick::tiles(&app.store).into_iter().find(|t| t.id == id) {
        None => text(format!("error: unknown control '{id}'")),
        Some(t) if !t.available() => text(format!("error: {id} is unavailable: {}", t.reason.unwrap_or_default())),
        Some(_) => {
            app.panel_activate(ID, id);
            text("ok")
        }
    }
}
