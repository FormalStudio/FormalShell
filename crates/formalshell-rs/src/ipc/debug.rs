//! `debug`: the verbs as far as this shell can answer them, and
//! the R0 spike's animation switches (`r0*`) until real targets replace
//! them.

use fs_types::Edge;
use serde_json::json;

use super::registry::{Function, Target, Type, Value};
use crate::scene::IRect;
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
            Function { name: "staleAudit", params: &[("action", Type::String)], ret: Type::String, call: stale_audit },
            Function { name: "skyAt", params: &[("instant", Type::String)], ret: Type::String, call: sky_at },
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
    let reach = fs_js::json_number(theme.card_radius());
    let screen = app.bar_output_name();
    let join = match app.debug_join() {
        Some((x, width)) => json!({"edge": edge.as_str(), "x": x, "width": width, "reach": reach, "screen": screen}),
        None => serde_json::Value::Null,
    };
    let line: Vec<serde_json::Value> = app.bar.line_rects().iter().map(rect).collect();
    let modals: Vec<serde_json::Value> = app
        .modals()
        .map(|m| {
            let scrims: Vec<serde_json::Value> =
                m.scrims().map(|(name, alpha, fade)| json!({"name": name, "alpha": alpha, "fade": fade})).collect();
            let content = m.layer.as_ref().map(|l| rect(&l.place(&m.win.card, true)));
            json!({"namespace": m.namespace, "open": m.open, "scrims": scrims, "card": rect(&m.win.card.live_rect()), "content": content, "popover": m.win.card.popover()})
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
    let mut dump = json!({
        "compositor": "hyprland",
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
        "captures": {
            "switcher": app.capture_sessions(crate::wayland::capture::Owner::Switcher),
            "spaces": app.capture_sessions(crate::wayland::capture::Owner::Preview),
            "launcher": app.capture_sessions(crate::wayland::capture::Owner::Launcher),
            "overview": app.capture_sessions(crate::wayland::capture::Owner::Overview),
        },
        "backdrop": app.backdrop_state(),
        "desktopClock": app.desktop_clock_state(),
        "veil": app.veil_phase(),
        "depth": app.store.depth.status(),
        "render": render(),
        "theme": {
            "radii": {"sm": fs_js::json_number(theme.radii.sm), "md": fs_js::json_number(theme.radii.md), "lg": fs_js::json_number(theme.radii.lg), "xl": fs_js::json_number(theme.radii.xl)},
            "radiusXl": reach,
            "borderWidth": fs_js::json_number(theme.border_width),
            "barPosition": edge.as_str(),
            "layout": theme.layout.as_str(),
            "barStyle": theme.bar_style.as_str(),
            "blur": app.store.theme.blur,
            "surfaceOpacity": fs_js::json_number(theme.surface_opacity),
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
    // Past json!'s recursion limit inside the literal.
    dump["panel"] = match app.panel.as_ref() {
        Some(h) => {
            let (viewport, content_h, scroll, row) = h.viewport();
            let cursor = row.map(|(key, r)| json!({"key": key, "rect": rect(&r)}));
            json!({
                "id": h.id(),
                "open": h.is_open(),
                "header": h.module.header(),
                "buttons": h.head_boxes().into_iter().map(|(key, r)| json!({"key": key, "rect": rect(&r)})).collect::<Vec<_>>(),
                "rest": rect(&h.win.card.rest_rect()),
                "live": rect(&h.win.card.live_rect()),
                "viewport": rect(&viewport),
                "contentH": content_h.round(),
                "scroll": scroll.round(),
                "cursor": cursor,
                "floats": h.floats().iter().map(|(r, scroll)| json!({"rect": rect(r), "scroll": scroll.round()})).collect::<Vec<_>>(),
            })
        }
        None => serde_json::Value::Null,
    };
    // dev/panel-language.py reads the card's column off these.
    dump["panelSpace"] = json!({"inset": theme.space.panel_inset, "rowInset": theme.space.panel_row_inset, "row": theme.space.panel_row});
    dump["toastCards"] = app.toast_cards().into_iter().map(|(key, r)| json!({"key": key, "rect": rect(&r)})).collect();
    dump["sky"] = sky(app);
    dump["rims"] = app.rims_report();
    dump["profilesPill"] = app.profiles_status();
    dump["headset"] = app.headset_status();
    dump["overview"] = app.overview_status();
    dump["sidebarClock"] = app.sidebar_clock_state();
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
    app.kit.motion_scale = app.motion_scale;
    ok()
}

/// The sun the sky desktop is drawn for, null while the desktop is
/// something else.
/// `start` checks every commit from here on against a full draw of the
/// same scene, `read` reports what it found per surface, `stop` reports
/// and ends it.
fn stale_audit(_: &mut App, args: &[Value]) -> Value {
    use crate::surface::audit;
    match args[0].str() {
        "start" => {
            audit::start();
            ok()
        }
        "read" => text(audit::report().to_string()),
        "stop" => {
            audit::stop();
            text(audit::report().to_string())
        }
        other => text(format!("error: unknown action {other}, expected start, read or stop")),
    }
}

fn sky(app: &App) -> serde_json::Value {
    let Some(s) = app.store.theme.sky else { return serde_json::Value::Null };
    let location = crate::services::theme::location(&app.store.config);
    json!({
        "source": app.store.desktop(),
        "step": s.step,
        "elevation": (s.elevation * 100.0).round() / 100.0,
        "located": s.located,
        "pinned": s.pinned,
        "nextMs": s.next_ms(location),
    })
}

/// Reads the sky at an RFC 3339 instant instead of now; an empty one goes
/// back to the clock.
fn sky_at(app: &mut App, args: &[Value]) -> Value {
    let arg = args[0].str();
    let pin = if arg.is_empty() {
        None
    } else {
        match chrono::DateTime::parse_from_rfc3339(arg) {
            Ok(at) => Some(at.timestamp_millis()),
            Err(e) => return text(format!("error: {e}")),
        }
    };
    app.store.theme.sky_at = pin;
    crate::wayland::theme_inputs(app);
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

/// The backend asked for at start and the one drawing; a settings.json
/// changed since reads in `theme` but takes effect at the next start.
fn render() -> serde_json::Value {
    let s = crate::gpu::status();
    json!({"asked": s.asked.as_str(), "backend": s.backend, "reason": s.reason, "device": s.device})
}
