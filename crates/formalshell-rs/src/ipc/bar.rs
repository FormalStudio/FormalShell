//! `bar`: the quick settings card opened and shut from a chevron
//! headlessly, `room` (what the strip shares out and what it hid) and
//! `paint` (the band's reading of the wallpaper).

use fs_types::Region;
use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::surfaces::panel::quick;
use crate::wayland::{App, live};

pub fn target() -> Target<App> {
    Target {
        name: "bar",
        reads_screen: false,
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

fn act(app: &mut App, action: &str, region: &str) -> Reply {
    let Some(r) = Region::parse(region) else {
        return Err(format!("unknown region '{region}' (left|center|right)"));
    };
    if !live!(app).bar.has_chevron(r) {
        return Err(format!("no chevron in bar.layout.{region}"));
    }
    let open = app.overflow_open() == Some(r);
    let origin = if live!(app).bar.sidebar().is_some() { quick::Origin::Sidebar } else { quick::Origin::Chevron(r) };
    match action {
        "toggle" => app.set_quick(origin, !open),
        "expand" => app.set_quick(origin, true),
        "collapse" => {
            if open {
                app.set_quick(origin, false);
            }
        }
        _ => return Err(format!("unknown chevron action '{action}' (toggle|expand|collapse)")),
    }
    ok()
}

fn chevron(app: &mut App, args: &[Value]) -> Reply {
    let action = args[0].str();
    if action == "status" {
        return Ok(live!(app).bar.chevron_status(app.overflow_open()));
    }
    let regions = live!(app).bar.chevron_regions();
    match regions.as_slice() {
        [] => Err("bar.layout has no chevron in any region".into()),
        [one] => act(app, action, one.as_str()),
        many => {
            let names: Vec<&str> = many.iter().map(|r| r.as_str()).collect();
            Err(format!("chevrons in {} regions ({}); use chevronAt <action> <region>", many.len(), names.join(", ")))
        }
    }
}

fn chevron_at(app: &mut App, args: &[Value]) -> Reply {
    act(app, args[0].str(), args[1].str())
}

/// One entry per mapped bar, as `debug dump`'s bars fan out.
fn room(app: &mut App, _: &[Value]) -> Reply {
    Ok(json!([live!(app).bar.room(&app.kit)]))
}

fn paint(app: &mut App, _: &[Value]) -> Reply {
    Ok(json!([{"screen": live!(app).bar.output, "paint": live!(app).bar.paint_state(&app.store)}]))
}
