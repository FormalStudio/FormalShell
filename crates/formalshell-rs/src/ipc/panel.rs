//! `panel`: the verbs over the panels' cards. `toggle` hangs a
//! card under its cell when the strip has one; `open` hangs it at the
//! strip's end.

use fs_chrome::bar::panels::{Home, PanelId};
use fs_chrome::plugins::Kind;

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::surfaces::panel::{self, Name};
use crate::wayland::{App, live};

pub fn target() -> Target<App> {
    Target {
        name: "panel",
        reads_screen: false,
        functions: vec![
            Function { name: "open", params: &[("name", Type::String)], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "toggle", params: &[("name", Type::String)], ret: Type::String, call: toggle },
            Function { name: "toggleAt", params: &[("n", Type::Int)], ret: Type::String, call: toggle_at },
            Function { name: "state", params: &[], ret: Type::String, call: state },
        ],
    }
}

fn known(app: &App, name: &str) -> Option<Name> {
    PanelId::parse(name).map(Name::Panel).or_else(|| panel::plugin::find(&app.store, name).map(|_| Name::Plugin(panel::plugin::intern(name))))
}

/// A plugin overlay's key.
fn overlay(app: &App, name: Name) -> Option<&'static str> {
    let Name::Plugin(key) = name else { return None };
    panel::plugin::find(&app.store, key).filter(|p| p.kind == Kind::Overlay).map(|_| key)
}

fn open(app: &mut App, args: &[Value]) -> Reply {
    let Some(name) = known(app, args[0].str()) else { return Err(format!("unknown panel '{}'", args[0].str())) };
    if let Some(key) = overlay(app, name) {
        app.overlay_show(key);
        return ok();
    }
    app.set_panel(name, true, None);
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.atlas_close();
    app.overlay_close();
    app.close_panels();
    if app.launcher_panel().is_some() {
        app.menu_close();
    }
    ok()
}

fn toggle(app: &mut App, args: &[Value]) -> Reply {
    let Some(name) = known(app, args[0].str()) else { return Err(format!("unknown panel '{}'", args[0].str())) };
    if name.panel().is_some_and(|id| id.home() == Home::Modal) {
        app.atlas_toggle();
        return ok();
    }
    if let Some(key) = overlay(app, name) {
        app.overlay_toggle(key);
        return ok();
    }
    let open = app.panel_open() != Some(name);
    // A toggle hangs the card under the cell that owns it; `open` is the
    // anchorless route, at the line's end.
    let anchor = name.panel().and_then(|id| live!(app).bar.panel_anchor(id));
    app.set_panel(name, open, anchor);
    ok()
}

fn toggle_at(app: &mut App, args: &[Value]) -> Reply {
    let n = args[0].int();
    match live!(app).bar.panel_at(n as i64) {
        Some(id) => toggle(app, &[Value::Str(id.as_str().into())]),
        None => text(format!("no panel at {n}")),
    }
}

fn state(app: &mut App, _: &[Value]) -> Reply {
    if app.atlas_open() {
        return text(PanelId::Radio.as_str());
    }
    if let Some(key) = app.overlay_open() {
        return text(key);
    }
    text(app.panel_open().map_or("", Name::as_str))
}
