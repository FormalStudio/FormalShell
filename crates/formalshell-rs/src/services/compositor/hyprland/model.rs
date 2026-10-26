//! Hyprland's `j/` answers mapped onto the shapes the shell's surfaces
//! read. Pure, so it is tested without a compositor.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use super::lua::PARK_WORKSPACE;
use crate::services::compositor::model::{Rect, Snapshot, Window, Workspace};

/// One refresh: the shared snapshot and the focused monitor's
/// `specialWorkspace.name`, "" when none is shown, which only the park
/// logic reads.
pub struct Read {
    pub snapshot: Snapshot,
    pub special_shown: String,
}

fn int(v: Option<&Value>) -> Option<i64> {
    let v = v?;
    v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))
}

fn string(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn array(text: &str) -> Vec<Value> {
    match serde_json::from_str(text) {
        Ok(Value::Array(items)) => items,
        _ => Vec::new(),
    }
}

/// `HyprlandToplevel.address`: the hex number with no `0x` and no leading
/// zeros, as `QString::number(address, 16)` prints it. `None` for an
/// address that is not one.
pub fn address_id(raw: &str) -> Option<String> {
    let hex = raw.strip_prefix("0x").unwrap_or(raw);
    u64::from_str_radix(hex, 16).ok().filter(|n| *n != 0).map(|n| format!("{n:x}"))
}

fn pair(v: Option<&Value>) -> Option<(i64, i64)> {
    let items = v?.as_array()?;
    Some((int(items.first())?, int(items.get(1))?))
}

/// A monitor's box in the layout's logical pixels, the space a client's
/// `at` and `size` are in: `width` and `height` are the mode's pixels, so
/// they swap under a 90 or 270 degree transform and divide by the scale.
fn logical_rect(m: &Value) -> Option<Rect> {
    let (mut w, mut h) = (int(m.get("width"))?, int(m.get("height"))?);
    if int(m.get("transform")).unwrap_or(0) % 2 == 1 {
        (w, h) = (h, w);
    }
    let scale = m.get("scale").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
    let (width, height) = ((w as f64 / scale).round() as i64, (h as f64 / scale).round() as i64);
    (width > 0 && height > 0).then(|| Rect { x: int(m.get("x")).unwrap_or(0), y: int(m.get("y")).unwrap_or(0), width, height })
}

/// `fullscreen` is a mode mask (1 maximized, 2 fullscreen) on current
/// Hyprland and a bool on older ones. Maximized alone leaves the bar up.
fn is_fullscreen(c: &Value) -> bool {
    match c.get("fullscreen") {
        Some(Value::Bool(b)) => *b,
        v => int(v).unwrap_or(0) & 2 != 0,
    }
}

/// Within a pixel either way, since a fractional scale rounds the
/// monitor's logical size one way and the client's another.
fn fills(window: &Rect, monitor: &Rect) -> bool {
    [(window.x, monitor.x), (window.y, monitor.y), (window.width, monitor.width), (window.height, monitor.height)]
        .iter()
        .all(|(a, b)| (a - b).abs() <= 1)
}

/// The four `j/` answers one refresh reads. `urgent` holds the window ids an
/// `urgent` event flagged and nothing has cleared since.
pub fn snapshot(monitors: &str, workspaces: &str, clients: &str, active: &str, urgent: &HashSet<String>) -> Read {
    let monitors = array(monitors);
    // A populated monitor list is the backend's `available`: the request
    // socket answered with a real session behind it.
    let available = !monitors.is_empty();
    let mut active_on: HashMap<String, i64> = HashMap::new();
    let mut focused_output = String::new();
    let mut special_shown = String::new();
    // Each workspace shown on a monitor, the special one included, and the
    // box it is shown in.
    let mut shown_on: HashMap<i64, (String, Option<Rect>)> = HashMap::new();
    for m in &monitors {
        let name = string(m.get("name"));
        if let Some(ws) = int(m.get("activeWorkspace").and_then(|w| w.get("id"))) {
            active_on.insert(name.clone(), ws);
            shown_on.insert(ws, (name.clone(), logical_rect(m)));
        }
        if let Some(ws) = int(m.get("specialWorkspace").and_then(|w| w.get("id"))).filter(|n| *n != 0) {
            shown_on.insert(ws, (name.clone(), logical_rect(m)));
        }
        if m.get("focused").and_then(Value::as_bool) == Some(true) {
            special_shown = string(m.get("specialWorkspace").and_then(|w| w.get("name")));
            focused_output = name;
        }
    }
    let focused_ws = active_on.get(&focused_output).copied();

    let active: Value = serde_json::from_str(active).unwrap_or(Value::Null);
    let focused_window = active.get("address").and_then(Value::as_str).and_then(address_id).unwrap_or_default();

    let mut windows = Vec::new();
    let (mut fullscreen, mut floating_on): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let (mut parked, mut overlaid) = (Vec::new(), Vec::new());
    for c in array(clients) {
        let Some(id) = c.get("address").and_then(Value::as_str).and_then(address_id) else { continue };
        let focused = id == focused_window;
        let hidden = c.get("hidden").and_then(Value::as_bool).unwrap_or(false);
        let rect = match (pair(c.get("at")), pair(c.get("size"))) {
            (Some((x, y)), Some((w, h))) if !hidden && w > 0 && h > 0 => Some(Rect { x, y, width: w, height: h }),
            _ => None,
        };
        let workspace = int(c.get("workspace").and_then(|w| w.get("id")));
        let workspace_id = workspace.map(|n| n.to_string()).unwrap_or_default();
        if string(c.get("workspace").and_then(|w| w.get("name"))) == PARK_WORKSPACE {
            if special_shown == PARK_WORKSPACE { overlaid.push(id.clone()) } else { parked.push(id.clone()) }
        }
        let floating = c.get("floating").and_then(Value::as_bool).unwrap_or(false);
        // A game on a monitor without focus still covers it, and a
        // borderless one is a floating window the size of the monitor. A
        // tiled one is left out: with the bar hidden its exclusive zone goes,
        // and a lone tile grows to the whole monitor and would keep it hidden.
        if let Some((name, output)) = workspace.and_then(|ws| shown_on.get(&ws)).filter(|(n, _)| !n.is_empty()) {
            let borderless = floating && matches!((&rect, output), (Some(r), Some(o)) if fills(r, o));
            if is_fullscreen(&c) && !floating {
                fullscreen.push(name.clone());
            } else if is_fullscreen(&c) || borderless {
                floating_on.push(name.clone());
            }
        }
        windows.push(Window {
            title: string(c.get("title")),
            app_id: string(c.get("class")),
            initial_class: string(c.get("initialClass")),
            initial_title: string(c.get("initialTitle")),
            pid: int(c.get("pid")).unwrap_or(0),
            is_focused: focused,
            is_floating: floating,
            // A window's urgency clears once it is activated or
            // its workspace takes focus.
            is_urgent: urgent.contains(&id) && !focused && focused_ws.map(|n| n.to_string()) != Some(workspace_id.clone()),
            rect,
            workspace_id,
            id,
            toplevel: String::new(),
        });
    }
    floating_on.retain(|o| !fullscreen.contains(o));
    fullscreen.extend(floating_on.iter().cloned());
    fullscreen.sort();
    fullscreen.dedup();
    floating_on.sort();
    floating_on.dedup();

    // Special workspaces carry a negative id and are overlays, not places:
    // model.js drops them, and the console lives on one for good.
    let mut out: Vec<Workspace> = array(workspaces)
        .iter()
        .filter_map(|w| {
            let idx = int(w.get("id"))?;
            if idx < 0 {
                return None;
            }
            let output = string(w.get("monitor"));
            let is_active = active_on.get(&output) == Some(&idx);
            let id = idx.to_string();
            let is_focused = is_active && output == focused_output;
            let is_urgent = !is_focused && windows.iter().any(|win| win.is_urgent && win.workspace_id == id);
            Some(Workspace { id, idx, name: string(w.get("name")), output, is_active, is_focused, is_urgent })
        })
        .collect();
    out.sort_by_key(|w| w.idx);

    let snapshot = Snapshot {
        available,
        workspaces: out,
        windows,
        fullscreen_outputs: fullscreen,
        floating_outputs: floating_on,
        focused_window_id: focused_window,
        focused_workspace_id: focused_ws.map(|n| n.to_string()).unwrap_or_default(),
        focused_output_name: focused_output,
        parked,
        overlaid,
    };
    Read { snapshot, special_shown }
}

/// The events after which nothing the backend reports can have changed.
pub fn ignores(event: &str) -> bool {
    matches!(event, "activelayout" | "submap" | "screencast" | "bell" | "minimized" | "ignoregrouplock" | "lockgroups")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITORS: &str = r#"[{"id":0,"name":"Virtual-1","focused":true,"activeWorkspace":{"id":2,"name":"2"},"specialWorkspace":{"id":0,"name":""}},
        {"id":1,"name":"HDMI-A-1","focused":false,"activeWorkspace":{"id":3,"name":"3"},"specialWorkspace":{"id":0,"name":""}}]"#;
    const WORKSPACES: &str = r#"[{"id":3,"name":"3","monitor":"HDMI-A-1","windows":0},{"id":-98,"name":"special:formalshell-console","monitor":"Virtual-1","windows":1},
        {"id":1,"name":"1","monitor":"Virtual-1","windows":1},{"id":2,"name":"2","monitor":"Virtual-1","windows":1}]"#;
    const CLIENTS: &str = r#"[{"address":"0x55d0c1a0","mapped":true,"hidden":false,"at":[10,50],"size":[800,600],"workspace":{"id":1,"name":"1"},"floating":false,"monitor":0,"class":"foot","title":"one","initialClass":"foot","initialTitle":"foot","pid":42,"fullscreen":0},
        {"address":"0x000055d0c1b0","mapped":true,"hidden":true,"at":[10,50],"size":[800,600],"workspace":{"id":2,"name":"2"},"floating":true,"monitor":0,"class":"mpv","title":"two","initialClass":"mpv","initialTitle":"mpv","pid":43,"fullscreen":2}]"#;
    const ACTIVE: &str = r#"{"address":"0x55d0c1b0"}"#;

    #[test]
    fn maps_the_contract() {
        let urgent = HashSet::from(["55d0c1a0".to_string()]);
        let s = snapshot(MONITORS, WORKSPACES, CLIENTS, ACTIVE, &urgent).snapshot;
        assert!(s.available);
        assert_eq!(s.focused_output_name, "Virtual-1");
        assert_eq!(s.focused_workspace_id, "2");
        assert_eq!(s.focused_window_id, "55d0c1b0");
        let ids: Vec<&str> = s.workspaces.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["1", "2", "3"]);
        assert!(s.workspaces[1].is_focused && s.workspaces[1].is_active);
        assert!(s.workspaces[2].is_active && !s.workspaces[2].is_focused);
        assert!(s.workspaces[0].is_urgent);
        assert_eq!(s.windows[0].rect, Some(Rect { x: 10, y: 50, width: 800, height: 600 }));
        assert_eq!(s.windows[1].rect, None, "a hidden window reports no box");
        assert!(s.windows[1].is_focused && s.windows[1].is_floating);
        assert_eq!(s.fullscreen_outputs, ["Virtual-1"]);
        assert_eq!(
            serde_json::to_string(&s.workspaces[0]).unwrap(),
            r#"{"id":"1","idx":1,"name":"1","output":"Virtual-1","isActive":false,"isFocused":false,"isUrgent":true}"#
        );
        assert!(serde_json::to_string(&s.windows[1]).unwrap().ends_with(r#""isUrgent":false,"rect":null}"#));
    }

    const FS_MONITORS: &str = r#"[{"id":0,"name":"eDP-1","focused":true,"x":0,"y":0,"width":2880,"height":1800,"scale":1.5,"transform":0,"activeWorkspace":{"id":1,"name":"1"},"specialWorkspace":{"id":0,"name":""}},
        {"id":1,"name":"DP-1","focused":false,"x":1920,"y":0,"width":1080,"height":1920,"scale":1,"transform":1,"activeWorkspace":{"id":2,"name":"2"},"specialWorkspace":{"id":0,"name":""}}]"#;

    fn covered(clients: &str) -> Vec<String> {
        snapshot(FS_MONITORS, "[]", clients, r#"{"address":"0x1"}"#, &HashSet::new()).snapshot.fullscreen_outputs
    }

    fn client(address: &str, workspace: i64, at: (i64, i64), size: (i64, i64), fullscreen: &str) -> String {
        format!(
            r#"{{"address":"{address}","hidden":false,"floating":true,"at":[{},{}],"size":[{},{}],"workspace":{{"id":{workspace},"name":"{workspace}"}},"monitor":0,"fullscreen":{fullscreen}}}"#,
            at.0, at.1, size.0, size.1
        )
    }

    #[test]
    fn maximized_leaves_the_output_uncovered() {
        assert!(covered(&format!("[{}]", client("0x1", 1, (0, 30), (1920, 1170), "1"))).is_empty());
    }

    #[test]
    fn fullscreen_covers_its_output_focused_or_not() {
        assert_eq!(covered(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "2"))), ["eDP-1"]);
        assert_eq!(covered(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "3"))), ["eDP-1"]);
        assert_eq!(covered(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "true"))), ["eDP-1"]);
        let two = format!("[{},{}]", client("0x1", 1, (10, 40), (800, 600), "0"), client("0x2", 2, (1920, 0), (1920, 1080), "2"));
        assert_eq!(covered(&two), ["DP-1"], "the game is on the monitor without focus");
    }

    #[test]
    fn a_window_the_size_of_its_monitor_covers_it() {
        assert_eq!(covered(&format!("[{}]", client("0x2", 2, (1920, 0), (1920, 1080), "0"))), ["DP-1"]);
        assert_eq!(covered(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "0"))), ["eDP-1"]);
        assert!(covered(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1170), "0"))).is_empty());
        let hidden = client("0x1", 1, (0, 0), (1920, 1200), "0").replace(r#""hidden":false"#, r#""hidden":true"#);
        assert!(covered(&format!("[{hidden}]")).is_empty());
        let tiled = client("0x1", 1, (0, 0), (1920, 1200), "0").replace(r#""floating":true"#, r#""floating":false"#);
        assert!(covered(&format!("[{tiled}]")).is_empty(), "a lone tile fills the monitor once the bar is gone");
    }

    #[test]
    fn a_floating_cover_is_told_apart_from_a_tiled_fullscreen() {
        let floating = |clients: &str| snapshot(FS_MONITORS, "[]", clients, "{}", &HashSet::new()).snapshot.floating_outputs;
        let tiled = |c: String| c.replace(r#""floating":true"#, r#""floating":false"#);
        assert_eq!(floating(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "0"))), ["eDP-1"], "borderless");
        assert_eq!(floating(&format!("[{}]", client("0x1", 1, (0, 0), (1920, 1200), "2"))), ["eDP-1"], "floating fullscreen");
        assert!(floating(&format!("[{}]", tiled(client("0x1", 1, (0, 0), (1920, 1200), "2")))).is_empty());
        let both = format!("[{},{}]", client("0x1", 1, (0, 0), (1920, 1200), "0"), tiled(client("0x2", 1, (0, 0), (1920, 1200), "2")));
        assert!(floating(&both).is_empty(), "a tiled fullscreen window on the same output wins");
    }

    #[test]
    fn a_workspace_not_shown_covers_nothing() {
        assert!(covered(&format!("[{}]", client("0x1", 5, (0, 0), (1920, 1200), "2"))).is_empty());
        let special = FS_MONITORS.replacen(r#""specialWorkspace":{"id":0,"name":""}"#, r#""specialWorkspace":{"id":-99,"name":"special:game"}"#, 1);
        let s = snapshot(&special, "[]", &format!("[{}]", client("0x1", -99, (0, 0), (1920, 1200), "2")), "{}", &HashSet::new());
        assert_eq!(s.snapshot.fullscreen_outputs, ["eDP-1"], "a special workspace shown on a monitor covers it");
    }

    #[test]
    fn no_session_is_unavailable() {
        let s = snapshot("", "", "", "", &HashSet::new()).snapshot;
        assert!(!s.available);
        assert!(s.workspaces.is_empty() && s.focused_workspace_id.is_empty());
    }

    #[test]
    fn address_prints_as_hyprland_takes_it() {
        assert_eq!(address_id("0x000055d0c1b0").as_deref(), Some("55d0c1b0"));
        assert_eq!(address_id("0xABC").as_deref(), Some("abc"));
        assert_eq!(address_id("0x0"), None);
        assert_eq!(address_id("nope"), None);
    }

    #[test]
    fn the_console_is_parked_until_its_special_workspace_shows() {
        let console = r#"[{"address":"0xc0","at":[0,0],"size":[10,10],"workspace":{"id":-98,"name":"special:formalshell-console"},"class":"dev.formalshell.console"}]"#;
        let hidden = snapshot(MONITORS, WORKSPACES, console, "{}", &HashSet::new());
        assert_eq!((hidden.snapshot.parked.as_slice(), hidden.snapshot.overlaid.len()), (["c0".to_string()].as_slice(), 0));
        let shown_monitors = MONITORS.replacen(r#""specialWorkspace":{"id":0,"name":""}"#, r#""specialWorkspace":{"id":-98,"name":"special:formalshell-console"}"#, 1);
        let shown = snapshot(&shown_monitors, WORKSPACES, console, "{}", &HashSet::new());
        assert_eq!(shown.special_shown, PARK_WORKSPACE);
        assert!(shown.snapshot.parked.is_empty());
        assert_eq!(shown.snapshot.overlaid, ["c0"]);
    }
}
