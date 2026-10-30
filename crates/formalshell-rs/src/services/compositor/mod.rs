//! The compositor: one backend per session, picked once at start
//! (`compositor.backend`, else whichever instance signature the session
//! carries), publishing the shared model in `model.rs` and taking the typed
//! writes in [`Command`]. Hyprland is the primary backend; mango (a dwl
//! descendant) maps its per-monitor tags onto workspaces.

pub mod hyprland;
pub mod mango;
pub mod model;

use std::sync::{LazyLock, OnceLock};

use fs_menu::keybinds::Bind;
use fs_system::compositor::keyboard::{self, Layout};
use fs_system::display::outputs::{Color, Output};

use crate::runtime::Ctx;
pub use model::{Snapshot, Window, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hyprland,
    Mango,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Hyprland => "hyprland",
            Kind::Mango => "mango",
        }
    }
}

/// `compositor.backend` ("auto", "hyprland" or "mango") against the
/// session's own signatures. auto, or a value it does not know, takes
/// Hyprland's signature first, then mango's, then Hyprland.
pub fn pick(setting: Option<&str>, hyprland: bool, mango: bool) -> Kind {
    match setting {
        Some("hyprland") => Kind::Hyprland,
        Some("mango") => Kind::Mango,
        _ if hyprland => Kind::Hyprland,
        _ if mango => Kind::Mango,
        _ => Kind::Hyprland,
    }
}

/// Read once: a backend cannot change under a running shell, so a new
/// `compositor.backend` takes effect at the next start.
pub fn kind() -> Kind {
    static KIND: LazyLock<Kind> = LazyLock::new(|| {
        let text = std::fs::read_to_string(crate::services::config::settings_path()).unwrap_or_default();
        let settings: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let setting = settings.get("compositor").and_then(|c| c.get("backend")).and_then(|b| b.as_str());
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        pick(setting, set("HYPRLAND_INSTANCE_SIGNATURE"), set("MANGO_INSTANCE_SIGNATURE"))
    });
    *KIND
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputsState {
    Unknown,
    Ok,
    Failed,
}

impl OutputsState {
    pub fn as_str(self) -> &'static str {
        match self {
            OutputsState::Unknown => "unknown",
            OutputsState::Ok => "ok",
            OutputsState::Failed => "failed",
        }
    }
}

pub struct State {
    pub snapshot: Snapshot,
    /// The last window the compositor reported focused, for focus.js's hold.
    pub remembered_focused_id: String,
    pub outputs: Vec<Output>,
    pub outputs_state: OutputsState,
    pub keyboard: Layout,
    /// False until the first answer (or failure) of the layout query, so a
    /// cell can tell "not asked yet" from "cannot be asked".
    pub keyboard_answered: bool,
    /// Counts config reloads, the contract's `configReloaded` signal.
    pub config_reloads: u64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::default(),
            remembered_focused_id: String::new(),
            outputs: Vec::new(),
            outputs_state: OutputsState::Unknown,
            keyboard: keyboard::unavailable(),
            keyboard_answered: false,
            config_reloads: 0,
        }
    }
}

pub enum Diff {
    Snapshot(Snapshot),
    Outputs(Vec<Output>, OutputsState),
    Keyboard(Layout),
    ConfigReloaded,
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Snapshot(snapshot) => {
                if self.snapshot == snapshot {
                    return false;
                }
                if !snapshot.focused_window_id.is_empty() {
                    self.remembered_focused_id = snapshot.focused_window_id.clone();
                }
                self.snapshot = snapshot;
            }
            Diff::Outputs(rows, state) => {
                if self.outputs == rows && self.outputs_state == state {
                    return false;
                }
                (self.outputs, self.outputs_state) = (rows, state);
            }
            Diff::Keyboard(layout) => {
                let first = !std::mem::replace(&mut self.keyboard_answered, true);
                if self.keyboard == layout && !first {
                    return false;
                }
                self.keyboard = layout;
            }
            Diff::ConfigReloaded => self.config_reloads += 1,
        }
        true
    }

    pub fn held_focused_window_id(&self) -> String {
        let c = &self.snapshot;
        model::held_focus(&c.focused_window_id, &self.remembered_focused_id, &c.windows, &c.focused_workspace_id)
    }

    pub fn window(&self, id: &str) -> Option<&Window> {
        self.snapshot.windows.iter().find(|w| w.id == id)
    }
}

/// The writes, queued from any thread and run in order on the service
/// thread against the backend's own latest model.
pub enum Command {
    FocusWorkspace(String),
    /// A persistent slot the compositor has no workspace for yet, on
    /// `output`.
    FocusWorkspaceAt { idx: i64, output: String },
    FocusWindow(String),
    CloseWindow(String),
    Spawn(Vec<String>),
    /// While `argv` is some, a plain Escape runs it without being taken
    /// from the focused app; none removes the bind. `name` keys the bind.
    EscapeBind { name: &'static str, argv: Option<Vec<String>> },
    FloatWindow(String),
    ToggleFloating(String),
    ToggleFullscreen(String),
    /// Onto the workspace slot `idx` of `output`, focus staying put.
    MoveWindowTo { id: String, idx: i64, output: String },
    /// Two windows trade places in the layout, focus staying put.
    SwapWindows { a: String, b: String },
    /// Under a scrolling layout, `id`'s column moves to sit just before
    /// `before`'s, or last with none, the columns between shifting one
    /// along. It focuses `id`.
    MoveColumn { id: String, before: Option<String> },
    PlaceFloatingWindow { id: String, x: f64, y: f64, width: f64, height: f64 },
    ParkWindow(String),
    UnparkWindow(String),
    RefreshWindows,
    RefreshOutputs,
    SetOutputEnabled(String, bool),
    SetOutputScale(String, f64),
    SetOutputMirror(String, String),
    SetOutputColor(String, Color),
    /// `compositor.workspaceNames`, for a backend whose workspaces carry
    /// no names of their own.
    WorkspaceNames(Vec<String>),
    /// The pointer hidden or shown on every output, the session's arrival
    /// and departure.
    Cursor { hidden: bool },
    /// Ends the session.
    Exit,
}

static COMMANDS: OnceLock<async_channel::Sender<Command>> = OnceLock::new();

/// The service has started, so [`send`] reaches it.
pub fn ready() -> bool {
    COMMANDS.get().is_some()
}

/// Dropped when the service has not started.
pub fn send(command: Command) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.try_send(command);
    }
}

pub fn focus_workspace(id: &str) {
    send(Command::FocusWorkspace(id.to_owned()));
}

pub fn focus_workspace_at(idx: i64, output: &str) {
    if idx >= 1 {
        send(Command::FocusWorkspaceAt { idx, output: output.to_owned() });
    }
}

pub fn focus_window(id: &str) {
    send(Command::FocusWindow(id.to_owned()));
}

pub fn close_window(id: &str) {
    send(Command::CloseWindow(id.to_owned()));
}

pub fn spawn(argv: &[String]) {
    send(Command::Spawn(argv.to_vec()));
}

pub fn escape_bind(name: &'static str, argv: Option<&[String]>) {
    send(Command::EscapeBind { name, argv: argv.map(<[String]>::to_vec) });
}

pub fn is_window_parked(state: &State, id: &str) -> bool {
    model::is_parked(&state.snapshot, id)
}

/// The compositor's binds for the launcher's keybinds route.
pub async fn binds() -> Result<Vec<Bind>, ()> {
    match kind() {
        Kind::Hyprland => hyprland::binds().await,
        Kind::Mango => mango::binds().await,
    }
}

/// The pointer in global logical coordinates, `None` when it cannot be
/// asked.
pub async fn cursor_pos() -> Option<(f64, f64)> {
    match kind() {
        Kind::Hyprland => hyprland::cursor_pos().await,
        Kind::Mango => mango::cursor_pos().await,
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = COMMANDS.set(tx);
    match kind() {
        Kind::Hyprland => hyprland::run(ctx, rx).await,
        Kind::Mango => mango::run(ctx, rx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_backend_follows_the_setting_then_the_session() {
        assert_eq!(pick(None, true, true), Kind::Hyprland);
        assert_eq!(pick(Some("auto"), false, true), Kind::Mango);
        assert_eq!(pick(None, false, false), Kind::Hyprland);
        assert_eq!(pick(Some("mango"), true, false), Kind::Mango);
        assert_eq!(pick(Some("hyprland"), false, true), Kind::Hyprland);
        assert_eq!(pick(Some("sway"), false, true), Kind::Mango);
    }
}
