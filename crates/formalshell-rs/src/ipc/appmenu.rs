//! `appmenu`: the focused Mac app's menu as the shell holds it, a title's
//! card opened and shut, and an item run by its path or its id, as a click
//! on its row does.

use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::services::{appmenu, mackeys, tray::MenuNode};
use crate::wayland::{App, live};

pub fn target() -> Target<App> {
    Target {
        name: "appmenu",
        reads_screen: false,
        functions: vec![
            Function { name: "status", params: &[], ret: Type::String, call: status },
            Function { name: "items", params: &[("path", Type::String)], ret: Type::String, call: items },
            Function { name: "open", params: &[("title", Type::String)], ret: Type::String, call: open },
            Function { name: "close", params: &[], ret: Type::String, call: close },
            Function { name: "activate", params: &[("path", Type::String)], ret: Type::String, call: activate },
            Function { name: "run", params: &[("id", Type::Int)], ret: Type::String, call: run },
        ],
    }
}

/// A title's `x` and `width` are its box on a horizontal strip, from the
/// strip's start, absent where the bar draws no titles. `rows` are the
/// open card's, each box in the card's surface. `macKeys` is whether the
/// shell last asked the session into the `mac` submap, which a config
/// without the include's MAC APPS block does not have, and `macBinds` how
/// many binds that block had recorded when the submap was last built, null
/// until it is: a bind the config makes before the block is not among them
/// and does nothing over a Mac window.
fn status(app: &mut App, _: &[Value]) -> Reply {
    let menus = &app.store.appmenu;
    let boxes = live!(app).bar.app_menu_titles();
    let titles: Vec<_> = menus
        .titles()
        .iter()
        .map(|t| match boxes.iter().find(|(id, ..)| *id == t.id) {
            Some((_, x, width)) => json!({"id": t.id, "label": t.label, "x": x.round(), "width": width.round()}),
            None => json!({"id": t.id, "label": t.label}),
        })
        .collect();
    let rows: Vec<_> = app.app_menu_rows().iter().map(|(id, r)| json!({"id": id, "x": r.x, "y": r.y, "width": r.w, "height": r.h})).collect();
    Ok(json!({
        "pid": menus.menu.as_ref().map(|m| m.pid),
        "titles": titles,
        "open": app.app_menu_open(),
        "rows": rows,
        "macKeys": mackeys::inside(),
        "macBinds": app.store.compositor.mac_binds,
    }))
}

fn node<'a>(app: &'a App, path: &str) -> Result<&'a MenuNode, String> {
    app.store.appmenu.at_path(path).ok_or_else(|| format!("no menu item at '{path}'"))
}

fn items(app: &mut App, args: &[Value]) -> Reply {
    let rows: Vec<_> = node(app, args[0].str())?
        .children
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "label": c.label,
                "enabled": c.enabled,
                "checked": c.checked,
                "separator": c.separator,
                "section": c.section,
                "submenu": c.has_children,
                "shortcut": c.shortcut,
            })
        })
        .collect();
    Ok(json!(rows))
}

fn open(app: &mut App, args: &[Value]) -> Reply {
    let id = node(app, args[0].str())?.id;
    if app.app_menu_open() != Some(id) {
        app.open_app_menu(id, None)?;
    }
    ok()
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    if app.app_menu_open().is_some() {
        app.close_tray_menu();
    }
    ok()
}

fn click(item: &MenuNode, name: &str) -> Reply {
    if item.has_children {
        return Err(format!("{name} is a menu, not an item"));
    }
    if !item.enabled {
        return Err(format!("{name} is disabled"));
    }
    appmenu::click(item.id);
    ok()
}

fn activate(app: &mut App, args: &[Value]) -> Reply {
    let path = args[0].str();
    click(node(app, path)?, &format!("'{path}'"))
}

/// The item by its dbusmenu id, which a launcher row carries.
fn run(app: &mut App, args: &[Value]) -> Reply {
    let id = args[0].int();
    match app.store.appmenu.node(id).filter(|_| id != 0) {
        Some(item) => click(item, &format!("item {id}")),
        None => Err(format!("no menu item with id {id}")),
    }
}
