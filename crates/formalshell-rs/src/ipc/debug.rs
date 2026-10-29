//! `debug`: the verbs as far as this shell can answer them, and
//! the R0 spike's animation switches (`r0*`) until real targets replace
//! them.

use fs_chrome::types::Edge;
use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::scene::IRect;
use crate::surfaces::bar::number;
use crate::wayland::App;

fn text(s: impl Into<String>) -> Value {
    Value::Str(s.into())
}

fn ok() -> Value {
    text("ok")
}

pub fn target() -> Target<App> {
    Target {
        name: "debug",
        functions: vec![
            Function { name: "dump", params: &[], ret: Type::String, call: dump },
            Function {
                name: "join",
                params: &[("edge", Type::String), ("x", Type::Int), ("width", Type::Int)],
                ret: Type::String,
                call: join,
            },
            Function { name: "joinClear", params: &[], ret: Type::String, call: join_clear },
            Function { name: "motionScale", params: &[("percent", Type::Int)], ret: Type::String, call: motion_scale },
            Function { name: "query", params: &[("q", Type::String)], ret: Type::String, call: query },
            Function { name: "r0Spinner", params: &[("on", Type::Bool)], ret: Type::String, call: r0_spinner },
            Function { name: "r0Panel", params: &[("action", Type::String)], ret: Type::String, call: r0_panel },
            Function { name: "r0Scrim", params: &[("on", Type::Bool)], ret: Type::String, call: r0_scrim },
            Function { name: "r0Cast", params: &[("on", Type::Bool)], ret: Type::String, call: r0_cast },
        ],
    }
}

pub fn rect(r: &IRect) -> serde_json::Value {
    json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h})
}

/// Only what this shell holds, keyed and ordered as the full dump:
/// the compositor block, the settings, the bar, its join, the frame and the
/// chrome numbers. Every other key waits for the service behind it.
fn dump(app: &mut App, _: &[Value]) -> Value {
    let h = &app.store.compositor;
    let c = &h.snapshot;
    let edge = app.bar.edge();
    let thickness = app.bar.thickness();
    let frame = app.bar.frame_thickness() as i32;
    let framed = app.bar.framed();
    let inset = |e: Edge| if e == edge { thickness } else if framed { frame } else { 0 };
    let theme = &app.store.theme.theme;
    let reach = number(theme.card_radius());
    let screen = app.bar_output_name();
    let join = match app.debug_join() {
        Some((x, width)) => json!({"edge": edge.as_str(), "x": x, "width": width, "reach": reach, "screen": screen}),
        None => serde_json::Value::Null,
    };
    let line: Vec<serde_json::Value> = app.bar.line_rects().iter().map(rect).collect();
    let modals: Vec<serde_json::Value> = app
        .modals()
        .iter()
        .map(|m| {
            let scrims: Vec<serde_json::Value> =
                m.scrims().map(|(name, alpha, fade)| json!({"name": name, "alpha": alpha, "fade": fade})).collect();
            let content = m.layer.as_ref().map(|l| rect(&l.place(&m.card, true)));
            json!({"namespace": m.namespace, "open": m.open, "scrims": scrims, "card": rect(&m.card.live_rect()), "content": content, "popover": m.card.popover()})
        })
        .collect();
    // Every toast the model holds, tier by tier, for a leg reading what one
    // said rather than only how many there were.
    let m = &app.store.notifications.model;
    let toasts: Vec<serde_json::Value> = [("popup", &m.popups), ("pending", &m.pending), ("past", &m.past)]
        .into_iter()
        .flat_map(|(tier, list)| {
            list.iter().map(move |e| {
                let actions: Vec<&str> = e.actions.iter().map(|a| a.label.as_str()).collect();
                json!({"tier": tier, "summary": e.summary, "body": e.body, "urgency": u8::from(e.urgency), "actions": actions, "arrivedAt": e.arrived_at})
            })
        })
        .collect();
    let phone = &app.store.info.iphone;
    let dump = json!({
        "compositor": crate::services::compositor::kind().as_str(),
        "available": c.available,
        "workspaces": c.workspaces,
        "windows": c.windows,
        "focusedWindowId": c.focused_window_id,
        "heldFocusedWindowId": h.held_focused_window_id(),
        "focusedWorkspaceId": c.focused_workspace_id,
        "fullscreenOutputs": c.fullscreen_outputs,
        "floatingOutputs": c.floating_outputs,
        "configLoaded": app.store.config.settings(),
        "agents": {"stateByWindow": app.store.agents.by_window, "stateByHost": app.store.agents.by_host},
        "iphone": {
            "installed": phone.installed,
            "available": phone.observer,
            "connected": phone.connected,
            "deviceName": phone.device_name,
            "batteryAvailable": phone.battery.is_some(),
            "battery": phone.battery.unwrap_or(0.0),
            "unread": phone.unread,
            "inFocus": phone.in_focus,
            "recentCount": phone.recent.len(),
        },
        "bar": [{"screen": screen, "edge": edge.as_str(), "line": line, "paint": app.bar.paint_state(&app.store)}],
        "join": join,
        "modals": modals,
        "toasts": toasts,
        "activity": app.store.activity.to_json(),
        "frame": app.bar.frame_state(),
        "backdrop": app.backdrop_state(),
        "desktopClock": app.desktop_clock_state(),
        "depth": app.store.depth.status(),
        "render": {"asked": theme.render_backend.as_str(), "backend": "cpu"},
        "theme": {
            "radii": {"sm": number(theme.radii.sm), "md": number(theme.radii.md), "lg": number(theme.radii.lg), "xl": number(theme.radii.xl)},
            "radiusXl": reach,
            "borderWidth": number(theme.border_width),
            "barPosition": edge.as_str(),
            "layout": theme.layout.as_str(),
            "barStyle": theme.bar_style.as_str(),
            "morph": theme.morph,
            "grid": {"wallpaper": theme.wallpaper_grid, "lock": theme.lock_grid},
            "animate": theme.wallpaper_animate,
            "edgeInset": {
                "top": inset(Edge::Top),
                "bottom": inset(Edge::Bottom),
                "left": inset(Edge::Left),
                "right": inset(Edge::Right),
            },
        },
    });
    text(dump.to_string())
}

fn join(app: &mut App, args: &[Value]) -> Value {
    let (edge, x, width) = (args[0].str(), args[1].int(), args[2].int());
    if !["top", "bottom", "left", "right"].contains(&edge) {
        return text(format!("error: unknown edge '{edge}' (top|bottom|left|right)"));
    }
    if width <= 0 {
        return text("error: width must be positive");
    }
    // A join on an edge the strip is not on has no line to open.
    app.set_debug_join((edge == app.bar.edge().as_str()).then_some((x, width)));
    ok()
}

fn join_clear(app: &mut App, _: &[Value]) -> Value {
    app.set_debug_join(None);
    ok()
}

fn motion_scale(app: &mut App, args: &[Value]) -> Value {
    let percent = args[0].int();
    if !(1..=5000).contains(&percent) {
        return text("error: percent must be 1..5000");
    }
    app.motion_scale = percent as f64 / 100.0;
    app.bar.kit.motion_scale = app.motion_scale;
    ok()
}

fn query(app: &mut App, args: &[Value]) -> Value {
    text(app.launcher.query(&app.store, args[0].str()).to_string())
}

fn r0_spinner(app: &mut App, args: &[Value]) -> Value {
    app.set_spinner(args[0].bool());
    ok()
}

fn r0_panel(app: &mut App, args: &[Value]) -> Value {
    let open = match args[0].str() {
        "open" => true,
        "close" => false,
        "toggle" => app.panel_open() != Some("calendar"),
        other => return text(format!("error: unknown action '{other}' (open|close|toggle)")),
    };
    app.set_panel("calendar", open, None);
    ok()
}

fn r0_scrim(app: &mut App, args: &[Value]) -> Value {
    app.set_scrim(std::time::Instant::now(), args[0].bool());
    ok()
}

fn r0_cast(app: &mut App, args: &[Value]) -> Value {
    app.cast = args[0].bool();
    ok()
}
