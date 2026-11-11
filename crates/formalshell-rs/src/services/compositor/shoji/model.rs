//! ShojiWM's `State` and answers mapped onto the shapes the shell's
//! surfaces read, and every [`Command`] onto its method. Pure, so it is
//! tested without a compositor.

use fs_system::display::outputs::{self, Color, Output};
use fs_system::keyboard::{self, Layout};
use fs_types::{Bind, IRect, Window, WindowId, Workspace, WorkspaceId};

use super::proto::{self, Method, OutputsSet};
use crate::services::compositor::{Command, Gaps, Snapshot};

/// The console's overlay, the one the engine's rule for
/// `dev.formalshell.console` maps it to.
pub const CONSOLE_OVERLAY: &str = "formalshell-console";

/// The state version this shell reads.
pub const VERSION: u32 = 1;

fn rect(r: &proto::Rect) -> Option<IRect> {
    let (w, h) = (r.width.round() as i32, r.height.round() as i32);
    (w > 0 && h > 0).then(|| IRect::new(r.x.round() as i32, r.y.round() as i32, w, h))
}

/// Within a pixel either way, since a fractional scale rounds the output's
/// logical size one way and the window's another.
fn fills(window: &IRect, output: &IRect) -> bool {
    [(window.x, output.x), (window.y, output.y), (window.w, output.w), (window.h, output.h)]
        .iter()
        .all(|(a, b)| (a - b).abs() <= 1)
}

pub fn snapshot(state: &proto::State) -> Snapshot {
    if state.version != VERSION {
        return Snapshot::default();
    }
    let output_rect = |name: &str| {
        state.outputs.iter().find(|o| o.name == name).and_then(|o| rect(&proto::Rect { x: o.x, y: o.y, width: o.width, height: o.height }))
    };
    let mut workspaces: Vec<Workspace> = state
        .workspaces
        .iter()
        .map(|w| Workspace {
            id: WorkspaceId::new(w.id.as_str()),
            idx: w.idx,
            name: w.name.clone(),
            output: w.output.clone(),
            is_active: w.active,
            is_focused: w.focused,
            is_urgent: w.urgent,
            columns: w.columns,
            zen: w.zen.as_deref().filter(|z| !z.is_empty()).map(WindowId::new),
        })
        .collect();
    workspaces.sort_by_key(|w| w.idx);

    let (mut fullscreen, mut floating_on): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let (mut parked, mut overlaid) = (Vec::new(), Vec::new());
    let mut windows = Vec::with_capacity(state.windows.len());
    for w in &state.windows {
        let id = WindowId::new(w.id.as_str());
        let rect = w.rect.as_ref().and_then(rect);
        // The output a window is on screen on: its workspace's while that
        // is active there, the focused one for a shown overlay.
        let shown_on = match &w.overlay {
            Some(_) if w.shown => Some(state.focused_output.clone()),
            Some(_) => None,
            None => state.workspaces.iter().find(|ws| ws.id == w.workspace_id && ws.active).map(|ws| ws.output.clone()),
        };
        if w.overlay.is_some() {
            if w.shown { overlaid.push(id.clone()) } else { parked.push(id.clone()) }
        }
        // As under Hyprland: a tiled fullscreen window covers its output, a
        // floating one fullscreen or the size of the output covers it as a
        // float, and a tile filling the output (the bar gone) is no cover.
        if let Some(name) = shown_on.filter(|n| !n.is_empty()) {
            let borderless = w.floating && matches!((&rect, output_rect(&name)), (Some(r), Some(o)) if fills(r, &o));
            if w.fullscreen && !w.floating {
                fullscreen.push(name);
            } else if w.fullscreen || borderless {
                floating_on.push(name);
            }
        }
        windows.push(Window {
            title: w.title.clone(),
            app_id: w.app_id.clone(),
            initial_class: w.initial_app_id.clone(),
            initial_title: w.initial_title.clone(),
            pid: w.pid,
            workspace_id: WorkspaceId::new(w.workspace_id.as_str()),
            special: w.overlay.is_some(),
            is_focused: w.focused,
            is_floating: w.floating,
            is_urgent: w.urgent,
            rect,
            id,
        });
    }
    floating_on.retain(|o| !fullscreen.contains(o));
    fullscreen.extend(floating_on.iter().cloned());
    fullscreen.sort();
    fullscreen.dedup();
    floating_on.sort();
    floating_on.dedup();

    Snapshot {
        available: true,
        workspaces,
        windows,
        fullscreen_outputs: fullscreen,
        floating_outputs: floating_on,
        focused_window_id: WindowId::new(state.focused_window_id.as_str()),
        focused_workspace_id: WorkspaceId::new(state.focused_workspace_id.as_str()),
        focused_output_name: state.focused_output.clone(),
        parked,
        overlaid,
    }
}

pub fn keyboard(k: &proto::Keyboard) -> Layout {
    let Some(current) = usize::try_from(k.active).ok().and_then(|i| k.layouts.get(i)) else {
        return keyboard::unavailable();
    };
    Layout { available: true, names: k.layouts.clone(), current_idx: k.active, current: current.clone() }
}

pub fn gaps(c: &proto::Chrome) -> Gaps {
    let px = |v: f64| v.round() as i32;
    Gaps { top: px(c.gaps_out.top), right: px(c.gaps_out.right), bottom: px(c.gaps_out.bottom), left: px(c.gaps_out.left) }
}

/// `#rrggbb` or `#rrggbbaa` as sRGB 0..1, the alpha dropped.
pub fn active_border(text: &str) -> Option<[f32; 3]> {
    let hex = text.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok().map(|b| f32::from(b) / 255.0);
    Some([ch(0)?, ch(2)?, ch(4)?])
}

/// The outputs a state lists, by name and box, so a change among them
/// (a hotplug, an output switched on or off) asks for `outputs.get` again.
pub fn output_key(state: &proto::State) -> Vec<(String, bool, i64, i64, i64, i64)> {
    let px = |v: f64| v.round() as i64;
    state.outputs.iter().map(|o| (o.name.clone(), o.enabled, px(o.x), px(o.y), px(o.width), px(o.height))).collect()
}

pub fn outputs(rows: &[proto::Output]) -> Vec<Output> {
    let px = |v: f64| if v.is_finite() { v.round() as i64 } else { 0 };
    let positive = |v: f64, fallback: f64| if v.is_finite() && v > 0.0 { v } else { fallback };
    let mapped: Vec<Output> = rows
        .iter()
        .filter(|o| !o.name.is_empty())
        .map(|o| Output {
            name: o.name.clone(),
            make: o.make.clone(),
            model: o.model.clone(),
            x: px(o.x),
            y: px(o.y),
            width: px(o.width),
            height: px(o.height),
            refresh: positive(o.refresh, 0.0),
            scale: positive(o.scale, 1.0),
            enabled: o.enabled,
            mirror_of: o.mirror_of.clone(),
            transform: o.transform,
            vrr: o.vrr,
            cm: o.color.cm.clone(),
            ten_bit: o.color.bitdepth == 10,
            sdr_brightness: positive(o.color.sdr_brightness, 1.0),
            sdr_saturation: positive(o.color.sdr_saturation, 1.0),
        })
        .collect();
    outputs::sort_outputs(&mapped)
}

pub fn color(c: &Color) -> proto::Color {
    proto::Color { cm: c.cm.clone(), bitdepth: if c.bitdepth == 10 { 10 } else { 8 }, sdr_brightness: c.sdrbrightness, sdr_saturation: c.sdrsaturation }
}

/// The modifier names a chord is written with, as the keybinds route
/// matches them; any other name is dropped.
const MODS: [&str; 8] = ["SUPER", "CTRL", "ALT", "SHIFT", "CAPS", "MOD2", "MOD3", "MOD5"];

pub fn binds(rows: Vec<proto::Bind>) -> Vec<Bind> {
    rows.into_iter()
        .map(|b| Bind {
            mods: b.mods.iter().filter_map(|m| MODS.iter().find(|n| n.eq_ignore_ascii_case(m)).copied()).collect(),
            chord: b.chord,
            key: b.key,
            action: b.action,
            args: b.args,
            title: b.title,
            submap: String::new(),
            catchall: false,
        })
        .collect()
}

/// The method a command is sent as, `None` for one the engine has no
/// method for: the shell's own zen (the engine owns zen), and the
/// re-reads, since every move reaches the shell through `state.changed`.
pub fn method(command: &Command) -> Option<Method> {
    let s = |id: &WindowId| id.as_str().to_owned();
    Some(match command {
        Command::FocusWorkspace(id) => Method::WorkspaceFocus { id: id.as_str().to_owned() },
        Command::FocusWorkspaceAt(idx) => Method::WorkspaceFocusAt { idx: *idx, output: None },
        Command::FocusWindow(id) => Method::WindowFocus { id: s(id) },
        Command::CloseWindow(id) => Method::WindowClose { id: s(id) },
        Command::Spawn(argv) => Method::Spawn { argv: argv.clone(), cwd: None },
        Command::EscapeBind { name, argv } => Method::BindEscape { name: (*name).to_owned(), argv: argv.clone() },
        Command::FloatWindow(id) => Method::WindowFloat { id: s(id), floating: Some(true) },
        Command::ToggleFloating(id) => Method::WindowFloat { id: s(id), floating: None },
        Command::ToggleFullscreen(id) => Method::WindowFullscreen { id: s(id) },
        Command::MoveWindowTo { id, idx, output } => Method::WindowMoveTo { id: s(id), idx: *idx, output: output.clone() },
        Command::SwapWindows { a, b } => Method::WindowSwap { a: s(a), b: s(b) },
        Command::MoveColumn { id, before } => Method::WindowMoveColumn { id: s(id), before: before.as_ref().map(s) },
        Command::PlaceFloatingWindow { id, x, y, width, height } => {
            Method::WindowPlace { id: s(id), x: *x, y: *y, width: *width, height: *height }
        }
        Command::ParkWindow(id) => Method::OverlayPark { id: s(id), overlay: CONSOLE_OVERLAY.to_owned() },
        Command::UnparkWindow(id) => Method::OverlayShow { id: s(id) },
        Command::ZenMode { on } => Method::ZenSet { id: None, on: *on },
        Command::Overview { open } => Method::OverviewSet { open: *open },
        Command::RefreshOutputs => Method::OutputsGet,
        Command::SetOutputEnabled(name, enabled) => Method::OutputsSet(OutputsSet { name: name.clone(), enabled: Some(*enabled), ..OutputsSet::default() }),
        Command::SetOutputScale(name, scale) => Method::OutputsSet(OutputsSet { name: name.clone(), scale: Some(*scale), ..OutputsSet::default() }),
        Command::SetOutputMirror(name, source) => Method::OutputsSet(OutputsSet { name: name.clone(), mirror: Some(source.clone()), ..OutputsSet::default() }),
        Command::SetOutputColor(name, c) => Method::OutputsSet(OutputsSet { name: name.clone(), color: Some(color(c)), ..OutputsSet::default() }),
        Command::Cursor { hidden } => Method::CursorHide { hidden: *hidden },
        Command::Theme { colors, chrome } => Method::ThemeSet(proto::Theme { colors: colors.clone(), chrome: chrome.clone() }),
        Command::Exit => Method::SessionExit,
        Command::Zen { .. } | Command::Unzen { .. } | Command::ZenRelease | Command::RefreshWindows | Command::InputSettled => return None,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    pub(crate) fn fixture() -> proto::State {
        serde_json::from_str(include_str!("fixture.json")).unwrap()
    }

    #[test]
    fn maps_the_contract() {
        let s = snapshot(&fixture());
        assert!(s.available);
        assert_eq!(s.focused_output_name, "Virtual-1");
        assert_eq!(s.focused_workspace_id, "ws-2");
        assert_eq!(s.focused_window_id, "tl-b");
        let ids: Vec<&str> = s.workspaces.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["ws-1", "ws-2", "ws-3"]);
        assert!(s.workspaces[1].is_focused && s.workspaces[1].is_active && s.workspaces[1].columns);
        assert!(s.workspaces[0].is_urgent && !s.workspaces[0].is_active);
        assert_eq!(s.workspaces[1].zen, Some(WindowId::new("tl-b")));
        assert_eq!(s.workspaces[0].zen, None);
        let w: Vec<&str> = s.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(w, ["tl-a", "tl-b", "tl-console", "tl-game"], "first-seen order, as sent");
        assert_eq!(s.windows[0].rect, Some(IRect::new(8, 8, 952, 1064)));
        assert!(s.windows[1].is_focused && s.windows[1].is_floating);
        assert_eq!(s.windows[2].rect, None);
        assert!(s.windows[2].special && s.windows[2].workspace_id.is_empty());
        assert_eq!(s.parked, ["tl-console"]);
        assert!(s.overlaid.is_empty());
        assert_eq!(s.windows[0].initial_class, "foot");
        assert_eq!(s.fullscreen_outputs, ["HDMI-A-1"], "the game is on the other output's active workspace");
        assert!(s.floating_outputs.is_empty());
    }

    #[test]
    fn a_shown_overlay_is_overlaid_on_the_focused_output() {
        let mut state = fixture();
        state.windows[2].shown = true;
        let s = snapshot(&state);
        assert!(s.parked.is_empty());
        assert_eq!(s.overlaid, ["tl-console"]);
        assert!(!crate::services::compositor::is_parked(&s, &WindowId::new("tl-console")));
    }

    #[test]
    fn a_float_the_size_of_its_output_covers_it_as_a_float() {
        let mut state = fixture();
        state.windows[3].fullscreen = false;
        state.windows[3].floating = true;
        state.windows[3].rect = Some(proto::Rect { x: 1920.0, y: 0.0, width: 1280.0, height: 720.0 });
        let s = snapshot(&state);
        assert_eq!(s.floating_outputs, ["HDMI-A-1"]);
        assert_eq!(s.fullscreen_outputs, ["HDMI-A-1"]);
    }

    #[test]
    fn another_version_is_unavailable() {
        let mut state = fixture();
        state.version = 2;
        assert!(!snapshot(&state).available);
    }

    #[test]
    fn keyboard_gaps_and_border() {
        let state = fixture();
        let k = keyboard(&state.keyboard);
        assert_eq!((k.available, k.current.as_str(), k.current_idx), (true, "Spanish", 1));
        assert!(!keyboard(&proto::Keyboard { layouts: vec![], active: 0 }).available);
        assert!(!keyboard(&proto::Keyboard { layouts: vec!["a".into()], active: 3 }).available);
        assert_eq!(gaps(&state.chrome), Gaps { top: 8, right: 8, bottom: 8, left: 8 });
        assert_eq!(active_border("#ff8000"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(active_border("#ff8000cc"), Some([1.0, 128.0 / 255.0, 0.0]));
        assert_eq!(active_border("ff8000"), None);
        assert_eq!(active_border("#ff80"), None);
    }

    #[test]
    fn outputs_map_disabled_ones_too() {
        let rows: Vec<proto::Output> = serde_json::from_value(json!([
            {"name": "HDMI-A-1", "make": "Dell", "model": "U2720Q", "enabled": false, "x": 0, "y": 0, "width": 0, "height": 0,
             "refresh": 0, "scale": 1, "transform": 0, "mirrorOf": "", "vrr": false, "modes": [],
             "color": {"cm": "srgb", "bitdepth": 8, "sdrBrightness": 1, "sdrSaturation": 1}},
            {"name": "eDP-1", "make": "BOE", "model": "x", "enabled": true, "x": 0, "y": 0, "width": 2880, "height": 1800,
             "refresh": 120.0, "scale": 1.5, "transform": 0, "mirrorOf": "", "vrr": true,
             "modes": [{"width": 2880, "height": 1800, "refresh": 120.0}],
             "color": {"cm": "hdr", "bitdepth": 10, "sdrBrightness": 1.2, "sdrSaturation": 0.9}}
        ]))
        .unwrap();
        let out = outputs(&rows);
        assert_eq!(out.len(), 2);
        let edp = out.iter().find(|o| o.name == "eDP-1").unwrap();
        assert!(edp.enabled && edp.ten_bit && edp.vrr);
        assert_eq!((edp.width, edp.height, edp.scale, edp.cm.as_str()), (2880, 1800, 1.5, "hdr"));
        assert!(!out.iter().find(|o| o.name == "HDMI-A-1").unwrap().enabled);
    }

    #[test]
    fn binds_keep_the_known_modifiers() {
        let b = binds(vec![proto::Bind {
            chord: "SUPER+SHIFT+Q".into(),
            mods: vec!["SUPER".into(), "shift".into(), "HYPER".into()],
            key: "Q".into(),
            action: "close".into(),
            args: vec![],
            title: "Close".into(),
        }]);
        assert_eq!(b[0].mods, ["SUPER", "SHIFT"]);
        assert_eq!(b[0].chord, "SUPER+SHIFT+Q");
    }

    fn wire(command: Command) -> serde_json::Value {
        let line = proto::line(None, method(&command).expect("has a method"));
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn every_command_reaches_its_method() {
        let id = || WindowId::new("tl-a");
        assert_eq!(wire(Command::FocusWorkspace(WorkspaceId::new("ws-2"))), json!({"method": "workspace.focus", "params": {"id": "ws-2"}}));
        assert_eq!(wire(Command::FocusWorkspaceAt(3)), json!({"method": "workspace.focusAt", "params": {"idx": 3}}));
        assert_eq!(wire(Command::FloatWindow(id())), json!({"method": "window.float", "params": {"id": "tl-a", "floating": true}}));
        assert_eq!(wire(Command::ToggleFloating(id())), json!({"method": "window.float", "params": {"id": "tl-a"}}));
        assert_eq!(
            wire(Command::MoveWindowTo { id: id(), idx: 2, output: "DP-1".into() }),
            json!({"method": "window.moveTo", "params": {"id": "tl-a", "idx": 2, "output": "DP-1"}})
        );
        assert_eq!(wire(Command::MoveColumn { id: id(), before: None }), json!({"method": "window.moveColumn", "params": {"id": "tl-a"}}));
        assert_eq!(
            wire(Command::PlaceFloatingWindow { id: id(), x: 10.0, y: 20.0, width: 300.0, height: 200.0 }),
            json!({"method": "window.place", "params": {"id": "tl-a", "x": 10.0, "y": 20.0, "width": 300.0, "height": 200.0}})
        );
        assert_eq!(wire(Command::ParkWindow(id())), json!({"method": "overlay.park", "params": {"id": "tl-a", "overlay": "formalshell-console"}}));
        assert_eq!(wire(Command::UnparkWindow(id())), json!({"method": "overlay.show", "params": {"id": "tl-a"}}));
        assert_eq!(wire(Command::ZenMode { on: None }), json!({"method": "zen.set", "params": {}}));
        assert_eq!(wire(Command::Overview { open: Some(true) }), json!({"method": "overview.set", "params": {"open": true}}));
        assert_eq!(
            wire(Command::EscapeBind { name: "menu", argv: None }),
            json!({"method": "bind.escape", "params": {"name": "menu", "argv": null}})
        );
        assert_eq!(
            wire(Command::SetOutputMirror("DP-1".into(), String::new())),
            json!({"method": "outputs.set", "params": {"name": "DP-1", "mirror": ""}})
        );
        let hdr = Color { cm: "hdr".into(), bitdepth: 10, sdrbrightness: 1.2, sdrsaturation: 1.0 };
        assert_eq!(
            wire(Command::SetOutputColor("DP-1".into(), hdr)),
            json!({"method": "outputs.set", "params": {"name": "DP-1", "color": {"cm": "hdr", "bitdepth": 10, "sdrBrightness": 1.2, "sdrSaturation": 1.0}}})
        );
        assert_eq!(wire(Command::Cursor { hidden: true }), json!({"method": "cursor.hide", "params": {"hidden": true}}));
        assert_eq!(wire(Command::Exit), json!({"method": "session.exit"}));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&proto::line(Some(7), Method::StateGet)).unwrap(),
            json!({"id": 7, "method": "state.get"})
        );
        assert!(method(&Command::ZenRelease).is_none());
        assert!(method(&Command::RefreshWindows).is_none());
    }
}
