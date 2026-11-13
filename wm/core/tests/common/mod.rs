#![allow(dead_code)]

use formalwm_core::{
    geom::Rect,
    model::{Model, NewWindow, Options, OutputGeo, Placed, Placement, WorkspaceSpec},
};

pub fn output(name: &str, x: f64, w: f64, h: f64) -> OutputGeo {
    OutputGeo {
        name: name.to_owned(),
        rect: Rect::new(x, 0.0, w, h),
        usable: Rect::new(x, 0.0, w, h),
        scale: 1.0,
        default_workspace: None,
    }
}

/// Nine workspaces on one 1920x1080 output with nothing reserved.
pub fn desk() -> Model {
    let specs: Vec<WorkspaceSpec> = (1..=9)
        .map(|n| WorkspaceSpec {
            name: format!("ws{n}"),
            pin: None,
        })
        .collect();
    let mut model = Model::new(Options::default(), &specs);
    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0)]);
    model
}

/// The desk with a second output right of it holding workspaces 4 and 8.
pub fn two_outputs() -> Model {
    let specs: Vec<WorkspaceSpec> = (1..=9)
        .map(|n| WorkspaceSpec {
            name: format!("ws{n}"),
            pin: matches!(n, 4 | 8).then(|| "B".to_owned()),
        })
        .collect();
    let mut model = Model::new(Options::default(), &specs);
    let mut b = output("B", 1920.0, 1600.0, 1000.0);
    b.default_workspace = Some("4".to_owned());
    model.set_outputs(vec![output("A", 0.0, 1920.0, 1080.0), b]);
    model
}

pub fn open(model: &mut Model, id: &str) {
    open_with(model, id, Placement::default());
}

pub fn open_with(model: &mut Model, id: &str, placement: Placement) {
    model.open(
        NewWindow {
            id: id.to_owned(),
            title: id.to_uppercase(),
            app_id: format!("app.{id}"),
            pid: 100,
            size: None,
        },
        placement,
    );
}

pub fn place(model: &Model, id: &str) -> Placed {
    model
        .layout()
        .into_iter()
        .find(|place| place.id == id)
        .unwrap_or_else(|| panic!("{id} is not laid out"))
}

pub fn focused(model: &Model) -> &str {
    model.focused_window().map_or("", String::as_str)
}

/// Where the window is on screen at rest, its tape's scroll taken off.
pub fn on_screen(model: &Model, id: &str) -> Rect {
    let place = place(model, id);
    let scroll = match (&place.workspace, place.scrolls) {
        (Some(ws), true) => model.workspace(ws).map_or(0.0, |ws| ws.tape.scroll),
        _ => 0.0,
    };
    place.rect.translate(-scroll, 0.0)
}
