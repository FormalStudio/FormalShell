//! `menu`: the summon routes, the rig's stand-ins for
//! Enter and typing, and the select/input modes, whose answers land in
//! `menu-selection.txt` as `{token, value}` or `{token, cancelled: true}`.

use super::registry::{Function, Reply, Target, Type, Value, ok, text};
use crate::wayland::App;

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
            Function { name: "actions", params: &[("index", Type::Int)], ret: Type::String, call: actions },
            Function { name: "runAction", params: &[("index", Type::Int), ("action", Type::String)], ret: Type::String, call: run_action },
            Function { name: "into", params: &[("index", Type::Int)], ret: Type::String, call: into },
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

fn toggle(app: &mut App, _: &[Value]) -> Reply {
    app.menu_toggle();
    ok()
}

fn summon(app: &mut App, args: &[Value]) -> Reply {
    app.menu_open(Some(args[0].str()));
    ok()
}

fn activate(app: &mut App, args: &[Value]) -> Reply {
    let i = usize::try_from(args[0].int()).unwrap_or(usize::MAX);
    if app.menu_activate(i, false) { ok() } else { Err("menu not open".into()) }
}

fn activate_alternate(app: &mut App, args: &[Value]) -> Reply {
    let i = usize::try_from(args[0].int()).unwrap_or(usize::MAX);
    if app.menu_activate(i, true) { ok() } else { Err("menu not open".into()) }
}

fn index(args: &[Value]) -> usize {
    usize::try_from(args[0].int()).unwrap_or(usize::MAX)
}

fn answer(r: Result<(), String>) -> Reply {
    r?;
    ok()
}

fn actions(app: &mut App, args: &[Value]) -> Reply {
    answer(app.menu_actions(index(args)))
}

fn run_action(app: &mut App, args: &[Value]) -> Reply {
    answer(app.menu_run_action(index(args), args[1].str()))
}

fn into(app: &mut App, args: &[Value]) -> Reply {
    answer(app.menu_into(index(args)))
}

fn filter(app: &mut App, args: &[Value]) -> Reply {
    if app.menu_filter(args[0].str()) { ok() } else { Err("menu not open".into()) }
}

fn close(app: &mut App, _: &[Value]) -> Reply {
    app.menu_close();
    ok()
}

fn refresh(app: &mut App, _: &[Value]) -> Reply {
    app.menu_refresh();
    ok()
}

fn status(app: &mut App, _: &[Value]) -> Reply {
    app.resolve_launcher();
    let mut v = app.launcher.status(&app.store);
    let rect = |r: fs_types::IRect| serde_json::json!(r);
    let shown = app.launcher_shown();
    if let Some(r) = shown.and_then(|w| w.body_rect) {
        v["body"] = rect(r);
    }
    if let Some(r) = shown.and_then(|w| w.field_box()) {
        v["fieldBox"] = rect(r);
    }
    if let Some(r) = shown.and_then(|w| w.token_rect()) {
        v["tokenBox"] = rect(r);
    }
    let (hover, action_hover) = shown.map_or((None, None), |w| w.hovered());
    v["hover"] = hover.map_or(-1, |i| i as i64).into();
    v["actionHover"] = action_hover.map_or(-1, |i| i as i64).into();
    if let Some(r) = shown.and_then(|w| w.column_box()) {
        v["actionsBox"] = rect(r);
    }
    if let Some(r) = shown.and_then(|w| w.pane_box()) {
        v["paneBox"] = rect(r);
    }
    if let Some(w) = shown {
        v["cardBox"] = rect(w.card_box());
        v["rowBoxes"] = w.row_boxes().into_iter().map(|(i, r)| keyed("index", i.into(), r)).collect();
    }
    v["footHints"] = shown
        .map_or_else(Vec::new, |w| w.foot_hints())
        .into_iter()
        .map(|(on, r)| keyed("on", on.into(), r))
        .collect();
    Ok(v)
}

/// `r`'s wire form led by one more key.
fn keyed(key: &str, value: serde_json::Value, r: fs_types::IRect) -> serde_json::Value {
    let mut out = serde_json::Map::from_iter([(key.to_owned(), value)]);
    if let serde_json::Value::Object(rect) = serde_json::json!(r) {
        out.extend(rect);
    }
    serde_json::Value::Object(out)
}

fn abbreviations(app: &mut App, _: &[Value]) -> Reply {
    use crate::services::menu as index;
    let now = crate::runtime::now_ms() as f64;
    Ok(fs_menu::abbrev::report(&crate::settings::menu_abbreviations(&app.store.config), &index::abbreviation_book(&app.store), now))
}

fn select(app: &mut App, args: &[Value]) -> Reply {
    let options = match serde_json::from_str::<serde_json::Value>(args[1].str()) {
        Ok(serde_json::Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        _ => return Err("optionsJson must be a JSON array".into()),
    };
    app.menu_select(args[0].str(), options, args[2].str());
    ok()
}

fn input(app: &mut App, args: &[Value]) -> Reply {
    app.menu_input(args[0].str(), args[1].str());
    ok()
}
