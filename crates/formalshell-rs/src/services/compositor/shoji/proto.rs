//! The wire of the 2026-11-11 shoji spec, section 3, as serde types: what
//! `state.get` and `state.changed` carry, the methods and their params, and
//! the answers the shell reads. Shaped field for field like the spec.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    pub version: u32,
    pub outputs: Vec<StateOutput>,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub focused_window_id: String,
    pub focused_workspace_id: String,
    pub focused_output: String,
    pub overview: bool,
    pub keyboard: Keyboard,
    pub cursor_hidden: bool,
    pub chrome: Chrome,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StateOutput {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub scale: f64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub idx: i64,
    pub name: String,
    pub output: String,
    pub active: bool,
    pub focused: bool,
    pub urgent: bool,
    pub columns: bool,
    pub zen: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Window {
    pub id: String,
    pub title: String,
    pub app_id: String,
    pub initial_app_id: String,
    pub initial_title: String,
    pub pid: i64,
    pub workspace_id: String,
    pub overlay: Option<String>,
    pub shown: bool,
    pub focused: bool,
    pub floating: bool,
    pub fullscreen: bool,
    pub urgent: bool,
    pub column: i64,
    pub row: i64,
    pub rect: Option<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Keyboard {
    pub layouts: Vec<String>,
    pub active: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chrome {
    pub gaps_out: Sides,
    pub active_border: String,
    pub blur: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Sides {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

/// `outputs.get`'s rows.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Output {
    pub name: String,
    pub make: String,
    pub model: String,
    pub enabled: bool,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub refresh: f64,
    pub scale: f64,
    pub transform: i64,
    pub mirror_of: String,
    pub vrr: bool,
    pub modes: Vec<Mode>,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Mode {
    pub width: f64,
    pub height: f64,
    pub refresh: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Color {
    pub cm: String,
    pub bitdepth: i64,
    pub sdr_brightness: f64,
    pub sdr_saturation: f64,
}

/// `binds.list`'s rows.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Bind {
    pub chord: String,
    pub mods: Vec<String>,
    pub key: String,
    pub action: String,
    pub args: Vec<String>,
    pub title: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Theme {
    pub colors: Map<String, Value>,
    pub chrome: Value,
}

/// A request's method and params, the table in the spec's section 3.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "method", content = "params")]
pub enum Method {
    #[serde(rename = "state.get")]
    StateGet,
    #[serde(rename = "workspace.focus")]
    WorkspaceFocus { id: String },
    #[serde(rename = "workspace.focusAt")]
    WorkspaceFocusAt {
        idx: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<String>,
    },
    #[serde(rename = "window.focus")]
    WindowFocus { id: String },
    #[serde(rename = "window.close")]
    WindowClose { id: String },
    #[serde(rename = "window.float")]
    WindowFloat {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        floating: Option<bool>,
    },
    #[serde(rename = "window.fullscreen")]
    WindowFullscreen { id: String },
    #[serde(rename = "window.moveTo")]
    WindowMoveTo { id: String, idx: i64, output: String },
    #[serde(rename = "window.swap")]
    WindowSwap { a: String, b: String },
    #[serde(rename = "window.moveColumn")]
    WindowMoveColumn {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    #[serde(rename = "window.place")]
    WindowPlace { id: String, x: f64, y: f64, width: f64, height: f64 },
    #[serde(rename = "overlay.park")]
    OverlayPark { id: String, overlay: String },
    #[serde(rename = "overlay.show")]
    OverlayShow { id: String },
    #[serde(rename = "zen.set")]
    ZenSet {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        on: Option<bool>,
    },
    #[serde(rename = "overview.set")]
    OverviewSet {
        #[serde(skip_serializing_if = "Option::is_none")]
        open: Option<bool>,
    },
    #[serde(rename = "spawn")]
    Spawn {
        argv: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        cwd: Option<String>,
    },
    #[serde(rename = "bind.escape")]
    BindEscape { name: String, argv: Option<Vec<String>> },
    #[serde(rename = "binds.list")]
    BindsList,
    #[serde(rename = "cursor.pos")]
    CursorPos,
    #[serde(rename = "cursor.hide")]
    CursorHide { hidden: bool },
    #[serde(rename = "outputs.get")]
    OutputsGet,
    #[serde(rename = "outputs.set")]
    OutputsSet(OutputsSet),
    #[serde(rename = "theme.set")]
    ThemeSet(Theme),
    #[serde(rename = "session.exit")]
    SessionExit,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OutputsSet {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Request {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub method: Method,
}

/// One line from the compositor: an answer (`id` with `result` or
/// `error`) or a broadcast (`event` with `payload`).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Message {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub event: Option<String>,
    pub payload: Option<Value>,
}

/// One request as its line, newline included.
pub fn line(id: Option<u64>, method: Method) -> String {
    let mut text = serde_json::to_string(&Request { id, method }).expect("a request always serializes");
    text.push('\n');
    text
}
