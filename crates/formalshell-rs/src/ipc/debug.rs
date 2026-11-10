//! `debug`: the verbs as far as this shell can answer them, and
//! the R0 spike's animation switches (`r0*`) until real targets replace
//! them.

use fs_chrome::bar::panels::PanelId;
use fs_types::Edge;
use serde_json::json;

use super::registry::{Function, Reply, Target, Type, Value, ok};
use crate::wayland::{App, live};

pub fn target() -> Target<App> {
    Target {
        name: "debug",
        reads_screen: false,
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

/// Only what this shell holds, keyed as the full dump: the compositor
/// block, the settings, the theme table and the chrome numbers, then each
/// window owner's own blocks. Every other key waits for the service behind
/// it.
fn dump(app: &mut App, _: &[Value]) -> Reply {
    let h = &app.store.compositor;
    let c = &h.snapshot;
    let chrome = app.chrome();
    let inset = |e: Edge| chrome.inset(e) as i32;
    let theme = &app.store.theme.theme;
    let mut phone = app.store.iphone.status(&app.store.media.ams);
    if let Some(recent) = phone.as_object_mut().and_then(|p| p.remove("recent")) {
        phone["recentCount"] = json!(recent.as_array().map_or(0, Vec::len));
    }
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
        "agents": {"stateByWindow": badges(&app.store.agents.by_window), "stateByHost": badges(&app.store.agents.by_host)},
        "iphone": phone,
        "activity": app.store.activity.to_json(),
        "depth": app.store.depth.status(),
        "render": render(),
        "theme": {
            "radii": {"sm": fs_js::json_number(theme.radii.sm), "md": fs_js::json_number(theme.radii.md), "lg": fs_js::json_number(theme.radii.lg), "xl": fs_js::json_number(theme.radii.xl)},
            "radiusXl": fs_js::json_number(theme.card_radius()),
            "borderWidth": fs_js::json_number(theme.border_width),
            "barPosition": chrome.edge.as_str(),
            "layout": crate::settings::layout(&app.store.config).as_str(),
            "barStyle": theme.bar_style.as_str(),
            "blur": app.store.theme.blur,
            "surfaceOpacity": fs_js::json_number(theme.surface_opacity),
            "morph": theme.morph,
            "grid": {"wallpaper": theme.wallpaper_grid, "lock": theme.lock_grid},
            "animate": crate::settings::WALLPAPER_ANIMATE.get(&app.store.config),
            "edgeInset": {
                "top": inset(Edge::Top),
                "bottom": inset(Edge::Bottom),
                "left": inset(Edge::Left),
                "right": inset(Edge::Right),
            },
        },
    });
    // dev/panel-language.py reads the card's column off these.
    dump["panelSpace"] = json!({"inset": theme.space.panel_inset, "rowInset": theme.space.panel_row_inset, "row": theme.space.panel_row});
    dump["sky"] = sky(app);
    dump["rims"] = app.rims_report();
    dump["animating"] = json!(crate::surface::running());
    if let Some(map) = dump.as_object_mut() {
        app.report(map);
    }
    Ok(dump)
}

fn join(app: &mut App, args: &[Value]) -> Reply {
    let (edge, x, width) = (args[0].str(), args[1].int(), args[2].int());
    if !["top", "bottom", "left", "right"].contains(&edge) {
        return Err(format!("unknown edge '{edge}' (top|bottom|left|right)"));
    }
    if width <= 0 {
        return Err("width must be positive".into());
    }
    // A join on an edge the strip is not on has no line to open.
    app.set_debug_join((edge == live!(app).bar.edge().as_str()).then_some((x, width)));
    ok()
}

fn join_clear(app: &mut App, _: &[Value]) -> Reply {
    app.set_debug_join(None);
    ok()
}

fn motion_scale(app: &mut App, args: &[Value]) -> Reply {
    let percent = args[0].int();
    if !(1..=5000).contains(&percent) {
        return Err("percent must be 1..5000".into());
    }
    app.apply(crate::store::Diff::Theme(crate::services::theme::Diff::MotionScale(percent as f64 / 100.0)));
    ok()
}

/// The sun the sky desktop is drawn for, null while the desktop is
/// something else.
/// `start` checks every commit from here on against a full draw of the
/// same scene, `read` reports what it found per surface, `stop` reports
/// and ends it.
fn stale_audit(_: &mut App, args: &[Value]) -> Reply {
    use crate::surface::audit;
    match args[0].str() {
        "start" => {
            audit::start();
            ok()
        }
        "read" => Ok(audit::report()),
        "stop" => {
            audit::stop();
            Ok(audit::report())
        }
        other => Err(format!("unknown action {other}, expected start, read or stop")),
    }
}

fn sky(app: &App) -> serde_json::Value {
    let Some(s) = app.store.theme.sky else { return serde_json::Value::Null };
    let location = crate::settings::location(&app.store.config);
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
fn sky_at(app: &mut App, args: &[Value]) -> Reply {
    let arg = args[0].str();
    let pin = if arg.is_empty() {
        None
    } else {
        match chrono::DateTime::parse_from_rfc3339(arg) {
            Ok(at) => Some(at.timestamp_millis()),
            Err(e) => return Err(e.to_string()),
        }
    };
    app.apply(crate::store::Diff::Theme(crate::services::theme::Diff::SkyAt(pin)));
    ok()
}

fn query(app: &mut App, args: &[Value]) -> Reply {
    Ok(app.launcher.query(&app.store, args[0].str()))
}

fn r0_spinner(app: &mut App, args: &[Value]) -> Reply {
    app.set_spinner(args[0].bool());
    ok()
}

fn r0_panel(app: &mut App, args: &[Value]) -> Reply {
    let open = match args[0].str() {
        "open" => true,
        "close" => false,
        "toggle" => !app.shows(PanelId::Calendar),
        other => return Err(format!("unknown action '{other}' (open|close|toggle)")),
    };
    app.set_panel(PanelId::Calendar, open, None);
    ok()
}

fn r0_scrim(app: &mut App, args: &[Value]) -> Reply {
    app.set_scrim(std::time::Instant::now(), args[0].bool());
    ok()
}

fn r0_cast(app: &mut App, args: &[Value]) -> Reply {
    app.cast = args[0].bool();
    ok()
}

/// The backend asked for at start and the one drawing; a settings.json
/// changed since reads in `theme` but takes effect at the next start.
fn render() -> serde_json::Value {
    let s = crate::gpu::status();
    json!({"asked": s.asked.as_str(), "backend": s.backend, "reason": s.reason, "device": s.device})
}

fn badges(by: &std::collections::BTreeMap<String, fs_info::agents::Badge>) -> std::collections::BTreeMap<&str, &'static str> {
    by.iter().map(|(k, b)| (k.as_str(), b.as_str())).collect()
}
