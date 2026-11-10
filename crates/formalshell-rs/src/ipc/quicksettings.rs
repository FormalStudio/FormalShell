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
        reads_screen: false,
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
    let host = app.panel.as_ref().filter(|h| h.is_open() && h.panel() == Some(ID));
    let boxes = host.map(|h| h.quick_boxes()).unwrap_or_default();
    let mut out = quick::report(&app.store, app.panel_ref::<QuickSettings>(ID), &boxes);
    out["open"] = json!(host.is_some());
    out["from"] = json!(host.map(|h| h.from.label()));
    out["card"] = host.map_or(Json::Null, |h| json!(h.win.card.rest_rect()));
    out["cursor"] = json!(host.and_then(|h| h.viewport().3.map(|(k, _)| k.to_owned())));
    Ok(out)
}

fn activate(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].str();
    if !open(app) {
        return Err("quick settings is not open".into());
    }
    match quick::tiles(&app.store).into_iter().find(|t| t.id == id) {
        None if extra(app, id) => {
            app.panel_activate(ID, id.into());
            ok()
        }
        None => Err(format!("unknown control '{id}'")),
        Some(t) if !t.available() => Err(format!("{id} is unavailable: {}", t.reason.unwrap_or_default())),
        Some(_) => {
            app.panel_activate(ID, id.into());
            ok()
        }
    }
}

/// Whether `id` is an info tile the open card drew last.
fn extra(app: &App, id: &str) -> bool {
    app.panel_ref::<QuickSettings>(ID).is_some_and(|q| quick::drew(q, id))
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
