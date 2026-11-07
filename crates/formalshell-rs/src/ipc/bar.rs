//! `bar`: the quick settings card opened and shut from a chevron
//! headlessly, `room` (what the strip shares out and what it hid) and
//! `paint` (the band's reading of the wallpaper).

use fs_types::Region;
use serde_json::{Map, json};

use super::registry::{Function, Target, Type, Value};
use crate::surfaces::panel::quick;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

pub fn target() -> Target<App> {
    Target {
        name: "bar",
        functions: vec![
            Function { name: "chevron", params: &[("action", Type::String)], ret: Type::String, call: chevron },
            Function {
                name: "chevronAt",
                params: &[("action", Type::String), ("region", Type::String)],
                ret: Type::String,
                call: chevron_at,
            },
            Function { name: "room", params: &[], ret: Type::String, call: room },
            Function { name: "paint", params: &[], ret: Type::String, call: paint },
        ],
    }
}

fn chevron_regions(app: &App) -> Vec<Region> {
    Region::ALL.into_iter().filter(|r| app.bar.has_chevron(*r)).collect()
}

fn act(app: &mut App, action: &str, region: &str) -> Value {
    let Some(r) = Region::parse(region) else {
        return text(format!("error: unknown region '{region}' (left|center|right)"));
    };
    if !app.bar.has_chevron(r) {
        return text(format!("error: no chevron in bar.layout.{region}"));
    }
    let open = app.overflow_open() == Some(r);
    let origin = if app.bar.sidebar.is_some() { quick::Origin::Sidebar } else { quick::Origin::Chevron(r) };
    match action {
        "toggle" => app.set_quick(origin, !open),
        "expand" => app.set_quick(origin, true),
        "collapse" => {
            if open {
                app.set_quick(origin, false);
            }
        }
        _ => return text(format!("error: unknown chevron action '{action}' (toggle|expand|collapse)")),
    }
    text("ok")
}

fn status(app: &App) -> Value {
    let mut regions = Map::new();
    for r in Region::ALL {
        // Under the sidebar the right region's chevron is the column's foot
        // button.
        let chevron = app.bar.has_chevron(r);
        let collapses: Vec<String> = if chevron { app.bar.overflow_entries(r).iter().map(|e| e.name()).collect() } else { Vec::new() };
        regions.insert(
            r.as_str().into(),
            json!({
                "chevron": chevron,
                "collapses": collapses,
                "open": app.overflow_open() == Some(r),
            }),
        );
    }
    let names: Vec<&str> = chevron_regions(app).iter().map(|r| r.as_str()).collect();
    text(json!({"regions": regions, "chevronRegions": names}).to_string())
}

fn chevron(app: &mut App, args: &[Value]) -> Value {
    let action = args[0].str();
    if action == "status" {
        return status(app);
    }
    let regions = chevron_regions(app);
    match regions.as_slice() {
        [] => text("error: bar.layout has no chevron in any region"),
        [one] => act(app, action, one.as_str()),
        many => {
            let names: Vec<&str> = many.iter().map(|r| r.as_str()).collect();
            text(format!("error: chevrons in {} regions ({}); use chevronAt <action> <region>", many.len(), names.join(", ")))
        }
    }
}

fn chevron_at(app: &mut App, args: &[Value]) -> Value {
    act(app, args[0].str(), args[1].str())
}

/// One entry per mapped bar, as `debug dump`'s bars fan out.
fn room(app: &mut App, _: &[Value]) -> Value {
    text(json!([app.bar.room()]).to_string())
}

fn paint(app: &mut App, _: &[Value]) -> Value {
    text(json!([{"screen": app.bar.output, "paint": app.bar.paint_state(&app.store)}]).to_string())
}
