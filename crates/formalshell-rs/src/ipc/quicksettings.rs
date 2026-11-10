//! `quicksettings`: the card's controls as the store has them and as last
//! drawn (`status`), Enter on one of them (`activate`), and its in-place
//! menu opened (`menu`) and closed (`back`), through the same paths as the
//! keyboard.

use fs_chrome::bar::panels::PanelId;
use serde_json::{Value as Json, json};

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::surfaces::panel::quick::{self, QuickSettings};
use crate::wayland::App;

const ID: PanelId = PanelId::QuickSettings;

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
    app.panel.as_ref().is_some_and(|h| h.is_open() && h.panel() == Some(ID))
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    let state: Option<Json> = app.panel_status(ID);
    let host = app.panel.as_ref().filter(|h| h.is_open() && h.panel() == Some(ID));
    let boxes = host.map(|h| h.quick_boxes()).unwrap_or_default();
    let boxed = |key: &str| boxes.iter().find(|(k, _)| k == key).map_or(Json::Null, |(_, r)| r.to_json());
    let parsed = state.clone().unwrap_or(Json::Null);
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
        "from": host.map(|h| h.from.label()),
        "card": host.map_or(Json::Null, |h| h.win.card.rest_rect().to_json()),
        "cursor": host.and_then(|h| h.viewport().3.map(|(k, _)| k.to_owned())),
        "rows": rows,
        "menu": menu,
        "dim": dim,
    });
    Ok(out)
}

fn activate(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].str();
    if !open(app) {
        return Err("quick settings is not open".into());
    }
    match quick::tiles(&app.store).into_iter().find(|t| t.id == id) {
        None if extra(app, id) => {
            app.panel_activate(ID, id);
            ok()
        }
        None => Err(format!("unknown control '{id}'")),
        Some(t) if !t.available() => Err(format!("{id} is unavailable: {}", t.reason.unwrap_or_default())),
        Some(_) => {
            app.panel_activate(ID, id);
            ok()
        }
    }
}

/// Whether `id` is an info tile the open card drew last.
fn extra(app: &mut App, id: &str) -> bool {
    let state: Json = app.panel_status(ID).unwrap_or(Json::Null);
    state["extras"].as_array().is_some_and(|x| x.iter().any(|x| x["id"] == id))
}

fn menu(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].str();
    if !open(app) {
        return Err("quick settings is not open".into());
    }
    if !quick::tiles(&app.store).iter().any(|t| t.id == id) {
        return Err(format!("unknown control '{id}'"));
    }
    if quick::menu(&app.store, id, None).is_none() {
        return Err(format!("{id} has no menu"));
    }
    match app.panel_mut::<QuickSettings>(ID) {
        Some(q) => {
            q.open_menu(id);
            Ok(Json::Null)
        }
        None => text(""),
    }
}

fn back(app: &mut App, _: &[Value]) -> Reply {
    if !open(app) {
        return Err("quick settings is not open".into());
    }
    match app.panel_mut::<QuickSettings>(ID).map(QuickSettings::close_menu) {
        Some(true) => Ok(Json::Null),
        Some(false) => Err("no menu is open".into()),
        None => text(""),
    }
}
