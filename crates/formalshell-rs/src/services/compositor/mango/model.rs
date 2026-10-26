//! mango's `all-monitors` and `all-clients` answers (`build_monitor_json`
//! and `build_client_json` in src/ipc/ipc.c at 0.18.0) mapped onto the
//! shared model. Pure, so it is tested without a compositor.
//!
//! Tags are per monitor, the dwl model: each occupied or viewed tag is one
//! workspace, `idx` the tag number and the id `<monitor>:<tag>`. Tag 0 is
//! the special overlay and, like Hyprland's special workspaces, never a
//! workspace of its own.

use serde_json::Value;

use crate::services::compositor::model::{Rect, Snapshot, Window, Workspace};
use fs_system::display::outputs::{self, Output};

fn int(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64))
}

fn string(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The array under `key` of a `{"<key>": [...]}` answer.
fn list(text: &str, key: &str) -> Vec<Value> {
    match serde_json::from_str::<Value>(text) {
        Ok(mut v) => match v.get_mut(key).map(Value::take) {
            Some(Value::Array(items)) => items,
            _ => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

pub fn workspace_id(monitor: &str, tag: i64) -> String {
    format!("{monitor}:{tag}")
}

/// The monitor and tag a [`workspace_id`] was made from. Output names carry
/// no colon, so the last one splits.
pub fn split_workspace_id(id: &str) -> Option<(&str, i64)> {
    let (monitor, tag) = id.rsplit_once(':')?;
    Some((monitor, tag.parse().ok()?))
}

fn tags(client: &Value) -> Vec<i64> {
    client.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(|n| int(Some(n))).collect()).unwrap_or_default()
}

/// `names[tag - 1]` when the setting gives one, else the number.
fn tag_name(names: &[String], tag: i64) -> String {
    usize::try_from(tag - 1).ok().and_then(|i| names.get(i)).filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| tag.to_string())
}

pub fn snapshot(monitors: &str, clients: &str, names: &[String]) -> Snapshot {
    let monitors = list(monitors, "monitors");
    let clients = list(clients, "clients");
    let focused = monitors.iter().find(|m| flag(m, "active"));
    let focused_output = focused.map(|m| string(m.get("name"))).unwrap_or_default();

    // mango reports a monitor's box and a client's in the same logical pixels.
    let boxes: Vec<(String, Rect)> = monitors
        .iter()
        .filter_map(|m| {
            let (x, y, width, height) = (int(m.get("x"))?, int(m.get("y"))?, int(m.get("width"))?, int(m.get("height"))?);
            Some((string(m.get("name")), Rect { x, y, width, height }))
        })
        .collect();

    let mut workspaces = Vec::new();
    let mut focused_workspace = String::new();
    for m in &monitors {
        let output = string(m.get("name"));
        let is_focused_output = output == focused_output;
        for t in m.get("tags").and_then(Value::as_array).into_iter().flatten() {
            let Some(idx) = int(t.get("index")) else { continue };
            let is_active = flag(t, "is_active");
            if !is_active && int(t.get("client_count")).unwrap_or(0) == 0 {
                continue;
            }
            let id = workspace_id(&output, idx);
            let is_focused = is_active && is_focused_output && focused_workspace.is_empty();
            if is_focused {
                focused_workspace = id.clone();
            }
            workspaces.push(Workspace {
                id,
                idx,
                name: tag_name(names, idx),
                output: output.clone(),
                is_active,
                is_focused,
                is_urgent: flag(t, "is_urgent") && !is_focused,
            });
        }
    }
    workspaces.sort_by(|a, b| a.idx.cmp(&b.idx).then_with(|| a.output.cmp(&b.output)));

    let mut windows = Vec::new();
    let (mut parked, mut overlaid, mut fullscreen, mut floating_on) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut focused_window = String::new();
    for c in &clients {
        let Some(id) = int(c.get("id")).map(|n| n.to_string()) else { continue };
        let monitor = string(c.get("monitor"));
        let minimized = flag(c, "is_minimized");
        let tags = tags(c);
        let workspace_id = match tags.iter().find(|t| **t > 0).or(tags.first()) {
            Some(tag) => workspace_id(&monitor, *tag),
            None => String::new(),
        };
        let focused = flag(c, "is_focused");
        if focused {
            focused_window = id.clone();
        }
        if minimized {
            parked.push(id.clone());
        } else if flag(c, "is_visible") && monitor == focused_output && workspace_id != focused_workspace {
            overlaid.push(id.clone());
        }
        let (x, y, width, height) = (int(c.get("x")), int(c.get("y")), int(c.get("width")), int(c.get("height")));
        let rect = match (x, y, width, height) {
            (Some(x), Some(y), Some(width), Some(height)) if !minimized && width > 0 && height > 0 => {
                Some(Rect { x, y, width, height })
            }
            _ => None,
        };
        // Focus does not matter: a game on another monitor still covers it,
        // and a borderless one is a floating window the size of its monitor.
        // A tiled one is left out: with the bar hidden its exclusive zone
        // goes, and a lone tile grows to the whole monitor.
        if flag(c, "is_visible") && !minimized && !monitor.is_empty() {
            let borderless = flag(c, "is_floating") && rect.as_ref().is_some_and(|r| boxes.iter().any(|(name, o)| *name == monitor && o == r));
            if flag(c, "is_fullscreen") && !flag(c, "is_floating") {
                fullscreen.push(monitor.clone());
            } else if flag(c, "is_fullscreen") || borderless {
                floating_on.push(monitor.clone());
            }
        }
        windows.push(Window {
            title: string(c.get("title")),
            app_id: string(c.get("appid")),
            initial_class: String::new(),
            initial_title: String::new(),
            pid: int(c.get("pid")).unwrap_or(0),
            workspace_id,
            is_focused: focused,
            is_floating: flag(c, "is_floating"),
            is_urgent: flag(c, "is_urgent") && !focused,
            rect,
            toplevel: string(c.get("foreign_toplevel_id")),
            id,
        });
    }
    floating_on.retain(|o: &String| !fullscreen.contains(o));
    fullscreen.extend(floating_on.iter().cloned());
    fullscreen.sort();
    fullscreen.dedup();
    floating_on.sort();
    floating_on.dedup();

    Snapshot {
        available: !monitors.is_empty(),
        workspaces,
        windows,
        fullscreen_outputs: fullscreen,
        floating_outputs: floating_on,
        focused_window_id: focused_window,
        focused_workspace_id: focused_workspace,
        focused_output_name: focused_output,
        parked,
        overlaid,
    }
}

/// The display panel's rows. mango reports each monitor's box in logical
/// pixels and no refresh rate, so the mode is the box times the scale and
/// the rate is unknown (0). An output mango has switched off still sits in
/// the list with an empty box.
pub fn outputs(monitors: &str) -> Vec<Output> {
    let rows: Vec<Output> = list(monitors, "monitors")
        .iter()
        .filter_map(|m| {
            let name = string(m.get("name"));
            if name.is_empty() {
                return None;
            }
            let scale = m.get("scale").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
            let (w, h) = (int(m.get("width")).unwrap_or(0), int(m.get("height")).unwrap_or(0));
            let enabled = w > 0 && h > 0;
            Some(Output {
                name,
                x: if enabled { int(m.get("x")).unwrap_or(0) } else { 0 },
                y: if enabled { int(m.get("y")).unwrap_or(0) } else { 0 },
                width: if enabled { (w as f64 * scale).round() as i64 } else { 0 },
                height: if enabled { (h as f64 * scale).round() as i64 } else { 0 },
                scale: if enabled { scale } else { 1.0 },
                enabled,
                vrr: flag(m, "is_vrr"),
                sdr_brightness: 1.0,
                sdr_saturation: 1.0,
                ..Output::default()
            })
        })
        .collect();
    outputs::sort_outputs(&rows)
}

/// The focused monitor's `keyboardlayout`: the active xkb layout's name.
pub fn keyboard_layout(monitors: &str) -> Option<String> {
    let monitors = list(monitors, "monitors");
    let m = monitors.iter().find(|m| flag(m, "active")).or(monitors.first())?;
    Some(string(m.get("keyboardlayout"))).filter(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes as build_monitor_json and build_client_json print them.
    const MONITORS: &str = r#"{"monitors":[
        {"name":"eDP-1","active":true,"is_hdr":false,"is_vrr":false,"x":0,"y":0,"width":1280,"height":800,"scale":1.5,
         "layout_index":0,"layout_symbol":"T","last_open_surface":"","tag_num":9,"hide_clients":0,
         "tags":[{"index":1,"is_active":false,"is_urgent":true,"layout":"T","client_count":1},
                 {"index":2,"is_active":true,"is_urgent":false,"layout":"T","client_count":1},
                 {"index":3,"is_active":false,"is_urgent":false,"layout":"T","client_count":0}],
         "active_tags":[2],"active_client":{"id":7,"title":"two","appid":"mpv"},"keymode":"default","keyboardlayout":"English (US)"},
        {"name":"HDMI-A-1","active":false,"is_hdr":false,"is_vrr":true,"x":1280,"y":0,"width":1920,"height":1080,"scale":1,
         "layout_index":0,"layout_symbol":"T","last_open_surface":"","tag_num":9,"hide_clients":0,
         "tags":[{"index":1,"is_active":true,"is_urgent":false,"layout":"T","client_count":0}],
         "active_tags":[1],"active_client":{"id":null,"title":null,"appid":null},"keymode":"default","keyboardlayout":"English (US)"},
        {"name":"DP-2","active":false,"is_hdr":false,"is_vrr":false,"x":0,"y":0,"width":0,"height":0,"scale":1,"tags":[]}]}"#;
    const CLIENTS: &str = r#"{"clients":[
        {"id":3,"pid":42,"foreign_toplevel_id":"a1b2","title":"one","appid":"foot","monitor":"eDP-1","tags":[1],"is_xwayland":false,
         "is_visible":false,"is_focused":false,"is_fullscreen":false,"is_floating":false,"is_minimized":false,"is_urgent":true,
         "x":10,"y":50,"width":800,"height":600},
        {"id":7,"pid":43,"foreign_toplevel_id":"c3d4","title":"two","appid":"mpv","monitor":"eDP-1","tags":[2],
         "is_visible":true,"is_focused":true,"is_fullscreen":true,"is_floating":true,"is_minimized":false,"is_urgent":false,
         "x":0,"y":0,"width":1280,"height":800},
        {"id":9,"pid":44,"foreign_toplevel_id":"e5","title":"console","appid":"dev.formalshell.console","monitor":"eDP-1","tags":[0],
         "is_visible":true,"is_focused":false,"is_fullscreen":false,"is_floating":true,"is_minimized":false,"is_urgent":false,
         "x":10,"y":40,"width":1260,"height":380},
        {"id":11,"pid":45,"foreign_toplevel_id":"f6","title":"gone","appid":"foot","monitor":"eDP-1","tags":[],
         "is_visible":false,"is_focused":false,"is_fullscreen":false,"is_floating":false,"is_minimized":true,"is_urgent":false,
         "x":0,"y":0,"width":500,"height":500}]}"#;

    #[test]
    fn tags_become_workspaces() {
        let names = vec!["web".to_string(), String::new()];
        let s = snapshot(MONITORS, CLIENTS, &names);
        assert!(s.available);
        assert_eq!(s.focused_output_name, "eDP-1");
        assert_eq!(s.focused_workspace_id, "eDP-1:2");
        let ids: Vec<&str> = s.workspaces.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["HDMI-A-1:1", "eDP-1:1", "eDP-1:2"], "an empty unviewed tag is left out");
        assert_eq!(
            serde_json::to_string(&s.workspaces[1]).unwrap(),
            r#"{"id":"eDP-1:1","idx":1,"name":"web","output":"eDP-1","isActive":false,"isFocused":false,"isUrgent":true}"#
        );
        assert_eq!(s.workspaces[2].name, "2", "an empty name falls back to the number");
        assert!(s.workspaces[2].is_focused);
        assert!(s.workspaces[0].is_active && !s.workspaces[0].is_focused);
    }

    #[test]
    fn clients_become_windows() {
        let s = snapshot(MONITORS, CLIENTS, &[]);
        assert_eq!(s.focused_window_id, "7");
        assert_eq!(s.fullscreen_outputs, ["eDP-1"]);
        let one = &s.windows[0];
        assert_eq!((one.id.as_str(), one.app_id.as_str(), one.workspace_id.as_str(), one.toplevel.as_str()), ("3", "foot", "eDP-1:1", "a1b2"));
        assert_eq!(one.rect, Some(Rect { x: 10, y: 50, width: 800, height: 600 }));
        assert!(one.is_urgent && !one.is_floating);
        assert!(s.windows[1].is_floating && !s.windows[1].is_urgent);
        assert_eq!(s.windows[2].workspace_id, "eDP-1:0");
        assert_eq!(s.overlaid, ["9"], "a shown special tag sits over the focused one");
        assert_eq!(s.parked, ["11"]);
        assert_eq!(s.windows[3].rect, None);
        assert_eq!(s.windows[3].workspace_id, "");
    }

    #[test]
    fn a_visible_game_covers_its_monitor_focused_or_not() {
        let unfocused = CLIENTS.replacen(r#""is_visible":true,"is_focused":true"#, r#""is_visible":true,"is_focused":false"#, 1);
        assert_eq!(snapshot(MONITORS, &unfocused, &[]).fullscreen_outputs, ["eDP-1"]);
        let borderless = unfocused.replacen(r#""is_fullscreen":true"#, r#""is_fullscreen":false"#, 1);
        assert_eq!(snapshot(MONITORS, &borderless, &[]).fullscreen_outputs, ["eDP-1"], "a window the size of the monitor");
        assert_eq!(snapshot(MONITORS, &borderless, &[]).floating_outputs, ["eDP-1"]);
        let shrunk = borderless.replacen(r#""width":1280,"height":800"#, r#""width":1280,"height":770"#, 1);
        assert!(snapshot(MONITORS, &shrunk, &[]).fullscreen_outputs.is_empty());
        let tiled = borderless.replacen(r#""is_floating":true,"is_minimized":false,"is_urgent":false,
         "x":0"#, r#""is_floating":false,"is_minimized":false,"is_urgent":false,
         "x":0"#, 1);
        assert!(snapshot(MONITORS, &tiled, &[]).fullscreen_outputs.is_empty(), "a lone tile is not a game");
        let unseen = unfocused.replacen(r#""is_visible":true,"is_focused":false,"is_fullscreen":true"#, r#""is_visible":false,"is_focused":false,"is_fullscreen":true"#, 1);
        assert!(snapshot(MONITORS, &unseen, &[]).fullscreen_outputs.is_empty(), "a tag not shown covers nothing");
    }

    #[test]
    fn nothing_answered_is_unavailable() {
        let s = snapshot("", "", &[]);
        assert!(!s.available && s.workspaces.is_empty() && s.windows.is_empty());
        assert!(snapshot(r#"{"error":"unknown command"}"#, "{}", &[]).workspaces.is_empty());
    }

    #[test]
    fn workspace_ids_round_trip() {
        assert_eq!(split_workspace_id(&workspace_id("HDMI-A-1", 4)), Some(("HDMI-A-1", 4)));
        assert_eq!(split_workspace_id("nope"), None);
    }

    #[test]
    fn monitors_become_outputs() {
        let rows = outputs(MONITORS);
        let edp = rows.iter().find(|o| o.name == "eDP-1").unwrap();
        assert_eq!((edp.width, edp.height, edp.scale, edp.enabled), (1920, 1200, 1.5, true));
        let hdmi = rows.iter().find(|o| o.name == "HDMI-A-1").unwrap();
        assert!(hdmi.vrr && hdmi.x == 1280);
        let off = rows.iter().find(|o| o.name == "DP-2").unwrap();
        assert!(!off.enabled && off.width == 0);
        assert_eq!(keyboard_layout(MONITORS).as_deref(), Some("English (US)"));
        assert_eq!(keyboard_layout("{}"), None);
    }
}
