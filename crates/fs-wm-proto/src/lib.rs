//! The wire of formalwm's JSON socket, shared by the engine and the shell's
//! `shoji` backend. The socket is at [`socket_path`]; `$SHOJIWM_SOCKET` is
//! not it, that one is the core's own `quit` and `reload` line socket.
//!
//! The wire is newline-delimited JSON, a request
//! `{"id"?, "method", "params"?}`, an answer `{"id", "result"}` or
//! `{"id", "error"}`, a broadcast `{"event", "payload"}`. A request without
//! an id is a command with no answer.
//!
//! Every method's name is in [`method`], its params and result beside it in
//! this crate; every broadcast's name is in [`event`]. A method with no row
//! in a table below takes no params or answers `null`.
//!
//! | Method | Params | Result |
//! | --- | --- | --- |
//! | `state.get` | | [`State`] |
//! | `workspace.focus` | [`WorkspaceFocus`] | |
//! | `workspace.focusAt` | [`WorkspaceFocusAt`] | |
//! | `window.focus`, `window.close`, `window.fullscreen`, `overlay.show` | [`WindowRef`] | |
//! | `window.float` | [`WindowFloat`] | |
//! | `window.moveTo` | [`WindowMoveTo`] | |
//! | `window.swap` | [`WindowSwap`] | |
//! | `window.moveColumn` | [`WindowMoveColumn`] | |
//! | `window.place` | [`WindowPlace`] | |
//! | `overlay.park` | [`OverlayPark`] | |
//! | `zen.set` | [`ZenSet`] | |
//! | `overview.set` | [`OverviewSet`] | |
//! | `spawn` | [`Spawn`] | |
//! | `bind.escape` | [`BindEscape`] | |
//! | `binds.list` | | `Vec<`[`Bind`]`>` |
//! | `cursor.pos` | | [`Point`] |
//! | `cursor.hide` | [`CursorHide`] | |
//! | `outputs.get` | | `Vec<`[`Output`]`>` |
//! | `outputs.set` | [`OutputSet`] | |
//! | `theme.set` | [`Theme`] | |
//! | `session.exit` | | |
//!
//! | Event | Payload |
//! | --- | --- |
//! | `state.changed` | [`State`] |
//! | `config.reloaded` | [`ConfigReloaded`] |
//! | `config.failed` | [`ConfigFailed`] |

use serde::{Deserialize, Serialize};

/// [`State::version`].
pub const VERSION: u32 = 1;

/// Where the JSON socket is, from the two variables of the session's
/// environment: what `shojiwm_rs::ipc::default_socket_path()` answers.
pub fn socket_path(xdg_runtime_dir: Option<&str>, wayland_display: Option<&str>) -> String {
    format!("{}/shojiwm-{}.sock", xdg_runtime_dir.unwrap_or("/tmp"), wayland_display.unwrap_or("wayland-0"))
}

pub mod method {
    pub const STATE_GET: &str = "state.get";
    pub const WORKSPACE_FOCUS: &str = "workspace.focus";
    pub const WORKSPACE_FOCUS_AT: &str = "workspace.focusAt";
    pub const WINDOW_FOCUS: &str = "window.focus";
    pub const WINDOW_CLOSE: &str = "window.close";
    pub const WINDOW_FLOAT: &str = "window.float";
    pub const WINDOW_FULLSCREEN: &str = "window.fullscreen";
    pub const WINDOW_MOVE_TO: &str = "window.moveTo";
    pub const WINDOW_SWAP: &str = "window.swap";
    pub const WINDOW_MOVE_COLUMN: &str = "window.moveColumn";
    pub const WINDOW_PLACE: &str = "window.place";
    pub const OVERLAY_PARK: &str = "overlay.park";
    pub const OVERLAY_SHOW: &str = "overlay.show";
    pub const ZEN_SET: &str = "zen.set";
    pub const OVERVIEW_SET: &str = "overview.set";
    pub const SPAWN: &str = "spawn";
    pub const BIND_ESCAPE: &str = "bind.escape";
    pub const BINDS_LIST: &str = "binds.list";
    pub const CURSOR_POS: &str = "cursor.pos";
    pub const CURSOR_HIDE: &str = "cursor.hide";
    pub const LAYERS_LIST: &str = "layers.list";
    pub const OUTPUTS_GET: &str = "outputs.get";
    pub const OUTPUTS_SET: &str = "outputs.set";
    pub const OUTPUTS_CREATE: &str = "outputs.create";
    pub const OUTPUTS_REMOVE: &str = "outputs.remove";
    pub const THEME_SET: &str = "theme.set";
    pub const SESSION_EXIT: &str = "session.exit";
}

pub mod event {
    pub const STATE_CHANGED: &str = "state.changed";
    /// The settings file was read again and applied; `binds.list` may differ.
    pub const CONFIG_RELOADED: &str = "config.reloaded";
    /// The settings file was refused; the running settings stand.
    pub const CONFIG_FAILED: &str = "config.failed";
}

// The envelope. Generic over the body, so neither side needs a JSON value
// type from this crate.

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request<P> {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub method: String,
    #[serde(default = "none", skip_serializing_if = "Option::is_none")]
    pub params: Option<P>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer<R> {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: Outcome<R>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome<R> {
    Result(R),
    Error(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Broadcast<P> {
    pub event: String,
    pub payload: P,
}

fn none<T>() -> Option<T> {
    None
}

// State.

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct State {
    pub version: u32,
    pub outputs: Vec<StateOutput>,
    pub workspaces: Vec<Workspace>,
    /// In first-seen order.
    pub windows: Vec<Window>,
    /// Empty where there is none.
    pub focused_window_id: String,
    pub focused_workspace_id: String,
    pub focused_output: String,
    pub overview: bool,
    pub keyboard: Keyboard,
    pub cursor_hidden: bool,
    pub chrome: ChromeState,
}

/// An enabled or disabled output, in global logical pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateOutput {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    /// Its slot on its output, from 1.
    pub idx: i64,
    pub name: String,
    pub output: String,
    /// The one its output shows.
    pub active: bool,
    /// Active on the focused output.
    pub focused: bool,
    pub urgent: bool,
    /// False for a floating workspace.
    pub columns: bool,
    /// The window in zen.
    pub zen: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// The ext-foreign-toplevel-list identifier.
    pub id: String,
    pub title: String,
    pub app_id: String,
    pub initial_app_id: String,
    pub initial_title: String,
    /// 0 where the client's is unknown.
    pub pid: i64,
    /// Empty on an overlay.
    pub workspace_id: String,
    /// The overlay it lives on.
    pub overlay: Option<String>,
    /// An overlay window currently on screen. Always true off an overlay.
    pub shown: bool,
    pub focused: bool,
    pub floating: bool,
    pub fullscreen: bool,
    pub urgent: bool,
    /// Its column on the tape from 0, -1 when floating.
    pub column: i32,
    /// Its place in the column from 0, -1 when floating.
    pub row: i32,
    /// Global logical, where it rests (not where an animation has it).
    pub rect: Option<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Keyboard {
    /// XKB layout names, as configured.
    pub layouts: Vec<String>,
    /// Index into `layouts`.
    pub active: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeState {
    pub gaps_out: Gaps,
    /// The focused window's border, `#rrggbb` or `#rrggbbaa`.
    pub active_border: String,
    pub blur: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gaps {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

// Method params and results.

/// `workspace.focus`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceFocus {
    pub id: String,
}

/// `workspace.focusAt`: the slot `idx` (from 1) of `output`, of the focused
/// output without one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceFocusAt {
    pub idx: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

/// `window.focus`, `window.close`, `window.fullscreen` (toggles) and
/// `overlay.show`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRef {
    pub id: String,
}

/// `window.float`: absent `floating` toggles.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowFloat {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub floating: Option<bool>,
}

/// `window.moveTo`: onto the slot `idx` (from 1) of `output`, focus staying.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMoveTo {
    pub id: String,
    pub idx: i64,
    pub output: String,
}

/// `window.swap`: two windows trade places, focus staying.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowSwap {
    pub a: String,
    pub b: String,
}

/// `window.moveColumn`: `id`'s column moves to sit just before `before`'s,
/// or last without one, and `id` is focused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowMoveColumn {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

/// `window.place`: a floating window's rect, global logical.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `overlay.park`: `id` moves onto `overlay` and the overlay is hidden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OverlayPark {
    pub id: String,
    pub overlay: String,
}

/// `zen.set`: absent `id` is the focused window, absent `on` toggles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ZenSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<bool>,
}

/// `overview.set`: absent `open` toggles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OverviewSet {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
}

/// `spawn`: `argv` run directly, no shell.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// `bind.escape`: while `argv` is some, a plain Escape runs it without being
/// taken from the focused app; null removes the bind `name` keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindEscape {
    pub name: String,
    pub argv: Option<Vec<String>>,
}

/// One row of `binds.list`. `chord` is `mods` and `key` joined with `+`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bind {
    pub chord: String,
    /// Of `SUPER`, `CTRL`, `ALT`, `SHIFT`, `CAPS`, `MOD2`, `MOD3`, `MOD5`. The
    /// engine binds the first four and lists them in that order.
    pub mods: Vec<String>,
    /// A keysym name, `code:<n>` for a keycode, or a pointer name
    /// (`mouse:272`, `mouse_up`, `mouse_down`).
    pub key: String,
    /// The engine's action name, `spawn` for a command.
    pub action: String,
    pub args: Vec<String>,
    /// What the settings file calls it, empty without a title.
    pub title: String,
}

/// `cursor.pos`'s answer, global logical.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// `cursor.hide`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CursorHide {
    pub hidden: bool,
}

/// One row of `layers.list`: a mapped layer-shell surface. The rows come in
/// stacking order, the bottom of the stack first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Layer {
    pub namespace: String,
    pub output: String,
    pub layer: LayerLevel,
    /// Global logical pixels.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// The client's process, 0 when the engine does not know it.
    pub pid: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayerLevel {
    Background,
    Bottom,
    Top,
    Overlay,
}

/// One row of `outputs.get`: every connected output, disabled ones too.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// The connector, `DP-1`.
    pub name: String,
    pub make: String,
    pub model: String,
    pub enabled: bool,
    /// Global logical position, 0 when disabled.
    pub x: i32,
    pub y: i32,
    /// The current mode's pixels, 0 when disabled.
    pub width: i32,
    pub height: i32,
    /// Hz, 0 when disabled.
    pub refresh: f64,
    pub scale: f64,
    /// `wl_output.transform`, 0 to 7.
    pub transform: u8,
    /// The output it mirrors, empty when it does not.
    pub mirror_of: String,
    pub vrr: bool,
    pub modes: Vec<Mode>,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub width: i32,
    pub height: i32,
    /// Hz.
    pub refresh: f64,
}

/// An output's colour state, and what `outputs.set` takes for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Color {
    /// `srgb` without HDR.
    pub cm: String,
    /// 8 or 10.
    pub bitdepth: u8,
    pub sdr_brightness: f64,
    pub sdr_saturation: f64,
}

impl Default for Color {
    fn default() -> Self {
        Self { cm: "srgb".into(), bitdepth: 8, sdr_brightness: 1.0, sdr_saturation: 1.0 }
    }
}

/// `outputs.set`: each present field is applied, the rest left alone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputSet {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
    /// The output to mirror, empty to stop mirroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
}

/// `outputs.create`: an output with no connector, made while the session
/// runs. It is an output like any other from then on. Answers `OutputRef`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct OutputCreate {
    /// The engine names it when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The mode's pixels.
    pub width: i32,
    pub height: i32,
    /// 1 when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<f64>,
}

/// `outputs.create`'s answer and `outputs.remove`'s params; only an output
/// `outputs.create` made can be removed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputRef {
    pub name: String,
}

// The theme.

/// `theme.set`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Theme {
    pub colors: Colors,
    pub chrome: Chrome,
}

/// The seven roles of `fs-theme/templates/hyprland-colors.lua.tmpl`, each
/// `#rrggbb`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Colors {
    pub primary: String,
    pub primary_foreground: String,
    pub background: String,
    pub foreground: String,
    pub border: String,
    pub destructive: String,
    pub warning: String,
}

/// Flexoki, what `hyprland.lua` falls back to before the shell's first
/// render.
impl Default for Colors {
    fn default() -> Self {
        Self {
            primary: "#4385BE".into(),
            primary_foreground: "#FFFCF0".into(),
            background: "#100F0F".into(),
            foreground: "#CECDC3".into(),
            border: "#575653".into(),
            destructive: "#D14D41".into(),
            warning: "#DA702C".into(),
        }
    }
}

/// Every key `fs-theme/src/chrome.rs` writes. A colour here is `#rrggbbaa`
/// or a key of [`Colors`] the engine resolves. The defaults are the
/// metamorphosis preset's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Chrome {
    pub rounding: i32,
    pub blur: bool,
    /// Per window side, so two windows sit `2 * gapsIn` apart.
    pub gaps_in: i32,
    /// At the output's edge.
    pub gaps_out: i32,
    pub border_size: i32,
    pub border_color: String,
    pub shadow: bool,
    pub shadow_range: i32,
    pub shadow_power: i32,
    pub shadow_offset: [i32; 2],
    pub shadow_color: String,
    pub shadow_inactive_color: String,
    pub motion: Motion,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            rounding: 10,
            blur: true,
            gaps_in: 4,
            gaps_out: 8,
            border_size: 2,
            border_color: "primary".into(),
            shadow: false,
            shadow_range: 4,
            shadow_power: 3,
            shadow_offset: [0, 0],
            shadow_color: "#000000ed".into(),
            shadow_inactive_color: "#000000ed".into(),
            motion: Motion::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Motion {
    /// False lands everything at once.
    pub enabled: bool,
    pub families: Families,
}

impl Default for Motion {
    fn default() -> Self {
        Self { enabled: true, families: Families::default() }
    }
}

/// The shell's six clock families (`fs_theme::tokens::motion_tokens`):
/// anything with a position or a size rides a spatial one, opacity and
/// colour an effects one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Families {
    pub spatial_fast: Family,
    pub spatial: Family,
    pub spatial_slow: Family,
    pub effects_fast: Family,
    pub effects: Family,
    pub effects_slow: Family,
}

impl Default for Families {
    fn default() -> Self {
        let family = |x1, y1, x2, y2, ms| Family { curve: [x1, y1, x2, y2], ms };
        Self {
            spatial_fast: family(0.42, 1.67, 0.21, 0.9, 350.0),
            spatial: family(0.38, 1.21, 0.22, 1.0, 500.0),
            spatial_slow: family(0.39, 1.29, 0.35, 0.98, 650.0),
            effects_fast: family(0.31, 0.94, 0.34, 1.0, 150.0),
            effects: family(0.34, 0.8, 0.34, 1.0, 200.0),
            effects_slow: family(0.34, 0.88, 0.34, 1.0, 300.0),
        }
    }
}

/// One family: a cubic bezier's two control points as `x1, y1, x2, y2` (its
/// ends fixed at 0,0 and 1,1) and its duration.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Family {
    pub curve: [f64; 4],
    pub ms: f64,
}

// Event payloads.

/// `config.reloaded`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigReloaded {}

/// `config.failed`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFailed {
    /// The file and what was wrong with it, for a person.
    pub error: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_str, from_value, json, to_value};

    #[test]
    fn the_state_reads_as_the_spec_writes_it() {
        let wire = json!({
            "version": 1,
            "outputs": [{ "name": "DP-1", "x": 0, "y": 0, "width": 2560, "height": 1440, "scale": 1.0, "enabled": true }],
            "workspaces": [{
                "id": "DP-1:1", "idx": 1, "name": "web", "output": "DP-1",
                "active": true, "focused": true, "urgent": false, "columns": true, "zen": null
            }],
            "windows": [{
                "id": "a1", "title": "t", "appId": "helium", "initialAppId": "helium", "initialTitle": "t",
                "pid": 42, "workspaceId": "DP-1:1", "overlay": null, "shown": true,
                "focused": true, "floating": false, "fullscreen": false, "urgent": false,
                "column": 0, "row": 0, "rect": { "x": 8, "y": 8, "width": 1272, "height": 1424 }
            }],
            "focusedWindowId": "a1", "focusedWorkspaceId": "DP-1:1", "focusedOutput": "DP-1",
            "overview": false,
            "keyboard": { "layouts": ["es"], "active": 0 },
            "cursorHidden": false,
            "chrome": { "gapsOut": { "top": 8, "right": 8, "bottom": 8, "left": 8 }, "activeBorder": "#4385be", "blur": true }
        });
        let state: State = from_value(wire.clone()).unwrap();
        assert_eq!(state.version, VERSION);
        assert_eq!(state.windows[0].rect, Some(Rect { x: 8, y: 8, width: 1272, height: 1424 }));
        assert_eq!(state.workspaces[0].zen, None);
        assert_eq!(to_value(&state).unwrap(), wire);
    }

    #[test]
    fn layers_and_made_outputs_read_as_the_spec_writes_them() {
        let wire = json!([{
            "namespace": "formalshell-bar", "output": "DP-1", "layer": "top",
            "x": 0, "y": 0, "width": 2560, "height": 36, "pid": 4242
        }]);
        let layers: Vec<Layer> = from_value(wire.clone()).unwrap();
        assert_eq!(layers[0].layer, LayerLevel::Top);
        assert_eq!(to_value(&layers).unwrap(), wire);

        let create: OutputCreate = from_str(r#"{"width": 1920, "height": 1080}"#).unwrap();
        assert_eq!((create.name, create.scale), (None, None));
        assert_eq!(
            to_value(OutputCreate { name: Some("HEADLESS-2".into()), width: 1280, height: 720, scale: Some(2.0) }).unwrap(),
            json!({ "name": "HEADLESS-2", "width": 1280, "height": 720, "scale": 2.0 })
        );
        assert_eq!(to_value(OutputRef { name: "HEADLESS-2".into() }).unwrap(), json!({ "name": "HEADLESS-2" }));
    }

    #[test]
    fn an_absent_optional_param_is_none_and_is_not_written() {
        let float: WindowFloat = from_str(r#"{"id":"a"}"#).unwrap();
        assert_eq!(float, WindowFloat { id: "a".into(), floating: None });
        assert_eq!(to_value(&float).unwrap(), json!({ "id": "a" }));
        assert_eq!(from_str::<ZenSet>("{}").unwrap(), ZenSet::default());
        assert_eq!(from_str::<OverviewSet>(r#"{"open":true}"#).unwrap(), OverviewSet { open: Some(true) });
        let escape: BindEscape = from_str(r#"{"name":"menu","argv":null}"#).unwrap();
        assert_eq!(escape.argv, None);
        assert_eq!(to_value(&escape).unwrap(), json!({ "name": "menu", "argv": null }));
    }

    #[test]
    fn the_envelope_is_shojiwms() {
        let command: Request<WindowRef> = from_str(r#"{"method":"window.focus","params":{"id":"a"}}"#).unwrap();
        assert_eq!(command.id, None);
        assert_eq!(command.params, Some(WindowRef { id: "a".into() }));
        let bare: Request<()> = from_str(r#"{"id":7,"method":"state.get"}"#).unwrap();
        assert_eq!((bare.id, bare.params), (Some(7), None));
        let ok = Answer { id: 7, outcome: Outcome::Result(Point { x: 1.0, y: 2.0 }) };
        assert_eq!(to_value(&ok).unwrap(), json!({ "id": 7, "result": { "x": 1.0, "y": 2.0 } }));
        let failed: Answer<Point> = from_str(r#"{"id":7,"error":"no such window"}"#).unwrap();
        assert_eq!(failed.outcome, Outcome::Error("no such window".into()));
        let event = Broadcast { event: event::CONFIG_RELOADED.to_owned(), payload: ConfigReloaded {} };
        assert_eq!(to_value(&event).unwrap(), json!({ "event": "config.reloaded", "payload": {} }));
    }

    #[test]
    fn a_partial_theme_keeps_the_defaults_for_the_rest() {
        let theme: Theme = from_value(json!({
            "colors": { "primary": "#112233" },
            "chrome": { "gapsOut": 6, "motion": { "enabled": false, "families": { "spatial": { "curve": [0.1, 0.2, 0.3, 0.4], "ms": 400 } } } }
        }))
        .unwrap();
        assert_eq!(theme.colors.primary, "#112233");
        assert_eq!(theme.colors.border, Colors::default().border);
        assert_eq!((theme.chrome.gaps_in, theme.chrome.gaps_out), (4, 6));
        assert!(!theme.chrome.motion.enabled);
        assert_eq!(theme.chrome.motion.families.spatial, Family { curve: [0.1, 0.2, 0.3, 0.4], ms: 400.0 });
        assert_eq!(theme.chrome.motion.families.effects, Families::default().effects);
    }

    #[test]
    fn an_output_row_and_its_colour_read_as_the_spec_writes_them() {
        let wire = json!({
            "name": "DP-1", "make": "ASUSTek", "model": "PA278CGV", "enabled": true,
            "x": 2560, "y": 0, "width": 2560, "height": 1440, "refresh": 144.0,
            "scale": 1.0, "transform": 0, "mirrorOf": "", "vrr": true,
            "modes": [{ "width": 2560, "height": 1440, "refresh": 144.0 }],
            "color": { "cm": "srgb", "bitdepth": 8, "sdrBrightness": 1.0, "sdrSaturation": 1.0 }
        });
        let output: Output = from_value(wire.clone()).unwrap();
        assert_eq!(output.color, Color::default());
        assert_eq!(to_value(&output).unwrap(), wire);
        let set: OutputSet = from_str(r#"{"name":"DP-1","mirror":""}"#).unwrap();
        assert_eq!((set.mirror.as_deref(), set.enabled, set.color), (Some(""), None, None));
    }

    #[test]
    fn the_json_socket_is_not_the_control_socket() {
        assert_eq!(socket_path(Some("/run/user/1000"), Some("wayland-1")), "/run/user/1000/shojiwm-wayland-1.sock");
        assert_eq!(socket_path(None, None), "/tmp/shojiwm-wayland-0.sock");
    }

    #[test]
    fn the_default_chrome_is_what_chrome_rs_writes() {
        let keys: Vec<String> = to_value(Chrome::default()).unwrap().as_object().unwrap().keys().cloned().collect();
        for key in [
            "rounding", "blur", "gapsIn", "gapsOut", "borderSize", "borderColor", "shadow", "shadowRange",
            "shadowPower", "shadowOffset", "shadowColor", "shadowInactiveColor", "motion",
        ] {
            assert!(keys.iter().any(|k| k == key), "{key}");
        }
        assert_eq!(keys.len(), 13);
    }
}
