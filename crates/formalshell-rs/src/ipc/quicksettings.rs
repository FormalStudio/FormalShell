//! `quicksettings`: the card's controls as the store has them and as last
//! drawn (`status`), Enter on one of them (`activate`), and its in-place
//! menu opened (`menu`) and closed (`back`), through the same paths as the
//! keyboard.

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
            Function { name: "menu", params: &[("id", Type::String)], ret: Type::String, call: menu },
            Function { name: "back", params: &[], ret: Type::String, call: back },
        ],
    }
}

fn open(app: &App) -> bool {
    app.panel.as_ref().is_some_and(|h| h.is_open() && h.id() == ID)
}

fn status(app: &mut App, _: &[Value]) -> Value {
    let state = app.panel_call(ID, "state", "");
    let host = app.panel.as_ref().filter(|h| h.is_open() && h.id() == ID);
    let boxes = host.map(|h| h.quick_boxes()).unwrap_or_default();
    let boxed = |key: &str| boxes.iter().find(|(k, _)| k == key).map_or(Json::Null, |(_, r)| rect(r));
    let parsed: Json = state.as_deref().and_then(|s| serde_json::from_str(s).ok()).unwrap_or(Json::Null);
    let shown = parsed["shown"].as_str();
    let mut rows: Vec<Json> = quick::tiles_with(&app.store, shown)
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
                "value": t.value,
                "rect": boxed(t.id),
                "icon": boxed(&format!("{}:icon", t.id)),
                "arrow": boxed(&format!("{}:menu", t.id)),
            })
        })
        .collect();
    // The info tiles as the card last drew them, after the toggles and
    // before Background.
    let at = rows.iter().position(|r| r["kind"] == "background").unwrap_or(rows.len());
    let extras = parsed["extras"].as_array().cloned().unwrap_or_default();
    let info = extras.iter().map(|x| {
        let id = x["id"].as_str().unwrap_or_default();
        json!({
            "id": id,
            "kind": quick::Kind::Info.as_str(),
            "title": x["title"],
            "subtitle": x["subtitle"],
            "glyph": x["icon"],
            "tone": x["tone"],
            "panel": x["panel"],
            "on": false,
            "available": true,
            "reason": null,
            "value": null,
            "rect": boxed(id),
            "icon": null,
            "arrow": null,
        })
    });
    rows.splice(at..at, info);
    let (menu, dim) = match &state {
        Some(state) => {
            let (mut menu, dim) = quick::status_menu(&app.store, state);
            if menu["id"].is_null() && menu["shown"].is_null() {
                menu = Json::Null;
            } else {
                menu["rect"] = boxed("menu");
                if let Some(rows) = menu["rows"].as_array_mut() {
                    for r in rows {
                        r["rect"] = r["id"].as_str().map_or(Json::Null, boxed);
                    }
                }
            }
            (menu, dim)
        }
        None => (Json::Null, false),
    };
    let out = json!({
        "open": host.is_some(),
        "from": host.and(parsed["from"].as_str()),
        "card": host.map_or(Json::Null, |h| rect(&h.win.card.rest_rect())),
        "cursor": host.and_then(|h| h.viewport().3.map(|(k, _)| k.to_owned())),
        "rows": rows,
        "menu": menu,
        "dim": dim,
    });
    text(out.to_string())
}

fn activate(app: &mut App, args: &[Value]) -> Value {
    let id = args[0].str();
    if !open(app) {
        return text("error: quick settings is not open");
    }
    match quick::tiles(&app.store).into_iter().find(|t| t.id == id) {
        None if extra(app, id) => {
            app.panel_activate(ID, id);
            text("ok")
        }
        None => text(format!("error: unknown control '{id}'")),
        Some(t) if !t.available() => text(format!("error: {id} is unavailable: {}", t.reason.unwrap_or_default())),
        Some(_) => {
            app.panel_activate(ID, id);
            text("ok")
        }
    }
}

/// Whether `id` is an info tile the open card drew last.
fn extra(app: &mut App, id: &str) -> bool {
    let state: Json = app.panel_call(ID, "state", "").and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Json::Null);
    state["extras"].as_array().is_some_and(|x| x.iter().any(|x| x["id"] == id))
}

fn menu(app: &mut App, args: &[Value]) -> Value {
    let id = args[0].str();
    if !open(app) {
        return text("error: quick settings is not open");
    }
    if !quick::tiles(&app.store).iter().any(|t| t.id == id) {
        return text(format!("error: unknown control '{id}'"));
    }
    if quick::menu(&app.store, id, None).is_none() {
        return text(format!("error: {id} has no menu"));
    }
    text(app.panel_call(ID, "menu", id).unwrap_or_default())
}

fn back(app: &mut App, _: &[Value]) -> Value {
    if !open(app) {
        return text("error: quick settings is not open");
    }
    text(app.panel_call(ID, "back", "").unwrap_or_default())
}
