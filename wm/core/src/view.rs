//! The state the socket publishes, built from the model.

use fs_wm_proto as proto;

use crate::model::{Home, Layer, Model};

/// What the model does not hold.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Extras {
    pub overview: bool,
    pub keyboard_layouts: Vec<String>,
    pub keyboard_active: usize,
    pub cursor_hidden: bool,
    /// The focused window's border, `#rrggbb` or `#rrggbbaa`.
    pub active_border: String,
    pub blur: bool,
    pub motion: proto::Motion,
}

pub fn state(model: &Model, extras: &Extras) -> proto::State {
    let layout = model.layout();
    let focused_window = model.focused_window().cloned().unwrap_or_default();
    let focused_workspace = model
        .focused_workspace()
        .map(|ws| ws.id.clone())
        .unwrap_or_default();
    let outputs = model
        .outputs
        .iter()
        .map(|output| {
            let (rect, usable) = (output.geo.rect, output.geo.usable);
            proto::StateOutput {
                name: output.geo.name.clone(),
                x: rect.x.round() as i32,
                y: rect.y.round() as i32,
                width: rect.w.round() as i32,
                height: rect.h.round() as i32,
                scale: output.geo.scale,
                enabled: true,
                reserved: proto::Gaps {
                    top: (usable.y - rect.y).round() as i32,
                    right: (rect.right() - usable.right()).round() as i32,
                    bottom: (rect.bottom() - usable.bottom()).round() as i32,
                    left: (usable.x - rect.x).round() as i32,
                },
            }
        })
        .collect();
    let workspaces = model
        .workspaces
        .iter()
        .filter(|ws| model.output(&ws.output).is_some())
        .map(|ws| {
            let active = model.is_active(&ws.id);
            proto::Workspace {
                id: ws.id.clone(),
                idx: model
                    .stack_index(&ws.id)
                    .map_or(0, |index| index as i64 + 1),
                name: ws.name.clone(),
                output: ws.output.clone(),
                active,
                focused: active && ws.output == model.focused_output,
                urgent: ws
                    .windows()
                    .any(|id| model.window(id).is_some_and(|w| w.urgent)),
                columns: true,
                zen: ws.zen.clone(),
            }
        })
        .collect();
    let windows = model
        .windows
        .iter()
        .map(|window| {
            let place = layout.iter().find(|place| place.id == window.id);
            let rect = place.map(|place| {
                let rect = model.shown_rect(place);
                proto::Rect {
                    x: rect.x.round() as i32,
                    y: rect.y.round() as i32,
                    width: rect.w.round() as i32,
                    height: rect.h.round() as i32,
                }
            });
            let (workspace_id, overlay) = match &window.home {
                Home::Workspace(id) => (id.clone(), None),
                Home::Overlay(name) => (String::new(), Some(name.clone())),
            };
            proto::Window {
                id: window.id.clone(),
                title: window.title.clone(),
                app_id: window.app_id.clone(),
                initial_app_id: window.initial_app_id.clone(),
                initial_title: window.initial_title.clone(),
                tag: String::new(),
                pid: window.pid,
                workspace_id,
                shown: overlay.is_none() || place.is_some_and(|place| place.shown),
                overlay,
                focused: window.id == focused_window,
                floating: window.floating,
                fullscreen: place.is_some_and(|place| place.layer == Layer::Fullscreen),
                urgent: window.urgent,
                column: place.map_or(-1, |place| place.column),
                row: place.map_or(-1, |place| place.row),
                rect,
            }
        })
        .collect();
    let gap = model.options.gaps_out.round() as i32;
    proto::State {
        version: proto::VERSION,
        outputs,
        workspaces,
        windows,
        focused_window_id: focused_window,
        focused_workspace_id: focused_workspace,
        focused_output: model.focused_output.clone(),
        overview: extras.overview,
        keyboard: proto::Keyboard {
            layouts: extras.keyboard_layouts.clone(),
            active: extras.keyboard_active,
        },
        cursor_hidden: extras.cursor_hidden,
        chrome: proto::ChromeState {
            gaps_out: proto::Gaps {
                top: gap,
                right: gap,
                bottom: gap,
                left: gap,
            },
            border_size: model.options.border.round() as i32,
            active_border: extras.active_border.clone(),
            blur: extras.blur,
            motion: extras.motion,
        },
        hand_over: None,
    }
}
