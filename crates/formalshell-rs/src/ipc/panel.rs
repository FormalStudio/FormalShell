//! `panel`: the verbs over the panels' cards. `toggle` hangs a
//! card under its cell when the strip has one; `open` hangs it at the
//! strip's end.

use fs_chrome::bar::{layout, panels};
use fs_chrome::plugins::Kind;

use super::registry::{Function, Target, Type, Value};
use crate::surfaces::panel;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "panel",
        functions: vec![
            Function { name: "open", params: &[("name", Type::String)], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "toggle", params: &[("name", Type::String)], ret: Type::String, call: toggle },
            Function { name: "toggleAt", params: &[("n", Type::Int)], ret: Type::String, call: toggle_at },
            Function { name: "state", params: &[], ret: Type::String, call: state },
        ],
    }
}

fn known(app: &App, name: &str) -> Option<&'static str> {
    panel::known(name).or_else(|| panel::plugin::find(&app.store, name).map(|_| panel::plugin::intern(name)))
}

fn is_overlay(app: &App, name: &str) -> bool {
    panel::plugin::find(&app.store, name).is_some_and(|p| p.kind == Kind::Overlay)
}

fn open(app: &mut App, args: &[Value]) -> Value {
    let Some(name) = known(app, args[0].str()) else { return text(format!("error: unknown panel '{}'", args[0].str())) };
    if name == "radio" {
        app.atlas_show();
        return text("ok");
    }
    if is_overlay(app, name) {
        app.overlay_show(name);
        return text("ok");
    }
    app.set_panel(name, true, None);
    text("ok")
}

fn close(app: &mut App, _: &[Value]) -> Value {
    app.atlas_close();
    app.overlay_close();
    app.close_panels();
    if app.launcher_panel().is_some() {
        app.menu_close();
    }
    text("ok")
}

fn toggle(app: &mut App, args: &[Value]) -> Value {
    let Some(name) = known(app, args[0].str()) else { return text(format!("error: unknown panel '{}'", args[0].str())) };
    if name == "radio" {
        app.atlas_toggle();
        return text("ok");
    }
    if is_overlay(app, name) {
        app.overlay_toggle(name);
        return text("ok");
    }
    let open = app.panel_open() != Some(name);
    // A toggle hangs the card under the cell that owns it; `open` is the
    // anchorless route, at the line's end.
    let anchor = app.bar.panel_anchor(name);
    app.set_panel(name, open, anchor);
    text("ok")
}

fn toggle_at(app: &mut App, args: &[Value]) -> Value {
    let n = args[0].int();
    let bar = app.store.config.get("bar").cloned();
    let resolved = layout::resolve(bar.as_ref(), &[]);
    match panels::panel_at(&resolved, n as i64) {
        Some(name) => toggle(app, &[Value::Str(name.into())]),
        None => text(format!("no panel at {n}")),
    }
}

fn state(app: &mut App, _: &[Value]) -> Value {
    if app.atlas_open() {
        return text("radio");
    }
    if let Some(key) = app.overlay_open() {
        return text(key);
    }
    text(app.panel_open().unwrap_or(""))
}
