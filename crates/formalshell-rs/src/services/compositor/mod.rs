//! The compositor, the shell's one door to it: one backend per session,
//! picked once at start ([`kind`]). `hyprland/` holds Hyprland's sockets, its
//! wire codecs and the `hl.*` Lua, `shoji/` ShojiWM's JSON socket; this
//! module the snapshot they publish and the typed writes in [`Command`].

pub mod hyprland;
pub mod shoji;

use std::sync::{LazyLock, OnceLock};

use serde_json::Value;

use fs_system::display::outputs::{Color, Output};
use fs_system::keyboard::{self, Layout};

use fs_types::{Bind, IRect, Window, WindowId, Workspace, WorkspaceId};

use crate::runtime::Ctx;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Hyprland,
    /// ShojiWM running FormalShell's engine, which owns the overview and
    /// zen that the shell draws itself under Hyprland.
    Shoji,
}

impl Kind {
    /// The name a session entry's `DesktopNames` leads with for this
    /// compositor.
    pub fn desktop(self) -> &'static str {
        match self {
            Kind::Hyprland => "Hyprland",
            Kind::Shoji => "ShojiWM",
        }
    }
}

/// Read once: ShojiWM's control socket marks its session, else Hyprland's.
/// Each session's start line unsets the other's variable in the user
/// manager, so a unit started there never carries both.
pub fn kind() -> Kind {
    static KIND: LazyLock<Kind> = LazyLock::new(|| if shoji::session() { Kind::Shoji } else { Kind::Hyprland });
    *KIND
}

/// The compositor's outer gap on each side of an output, in logical
/// pixels, zero where it cannot be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gaps {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub left: i32,
}

/// Everything one refresh learns, before the backend's own bookkeeping
/// (window order, urgency) is laid over it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub available: bool,
    pub workspaces: Vec<Workspace>,
    pub windows: Vec<Window>,
    pub fullscreen_outputs: Vec<String>,
    /// The covered outputs with no tiled fullscreen window over them: the
    /// cover is a floating window, fullscreen or the size of the output.
    pub floating_outputs: Vec<String>,
    pub focused_window_id: WindowId,
    pub focused_workspace_id: WorkspaceId,
    pub focused_output_name: String,
    /// Windows put away off every workspace (the console's overlay while it
    /// is hidden).
    pub parked: Vec<WindowId>,
    /// Windows on screen over the focused workspace though they belong to
    /// another (the console's overlay while it shows).
    pub overlaid: Vec<WindowId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputsState {
    Unknown,
    Ok,
    Failed,
}

pub struct State {
    pub snapshot: Snapshot,
    /// The last window the compositor reported focused, for the focus hold.
    pub remembered_focused_id: WindowId,
    pub outputs: Vec<Output>,
    pub outputs_state: OutputsState,
    pub keyboard: Layout,
    /// False until the first answer (or failure) of the layout query, so a
    /// cell can tell "not asked yet" from "cannot be asked".
    pub keyboard_answered: bool,
    /// Counts the compositor's config reloads, the contract's
    /// `configReloaded` signal.
    pub config_reloads: u64,
    /// How many binds the session's config had recorded for the `mac`
    /// submap when it was last built (`services/mackeys.rs`); none until
    /// one is, which a config without the include's MAC APPS block never
    /// does.
    pub mac_binds: Option<u32>,
    pub gaps_out: Gaps,
    /// The focused window's border colour, sRGB 0..1.
    pub active_border: Option<[f32; 3]>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            snapshot: Snapshot::default(),
            remembered_focused_id: WindowId::default(),
            outputs: Vec::new(),
            outputs_state: OutputsState::Unknown,
            keyboard: keyboard::unavailable(),
            keyboard_answered: false,
            config_reloads: 0,
            mac_binds: None,
            gaps_out: Gaps::default(),
            active_border: None,
        }
    }
}

pub enum Diff {
    Snapshot(Snapshot),
    Outputs(Vec<Output>, OutputsState),
    Keyboard(Layout),
    ConfigReloaded,
    MacBinds(u32),
    GapsOut(Gaps),
    ActiveBorder(Option<[f32; 3]>),
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
            Diff::MacBinds(recorded) => {
                if self.mac_binds == Some(recorded) {
                    return false;
                }
                self.mac_binds = Some(recorded);
            }
            Diff::GapsOut(gaps) => {
                if self.gaps_out == gaps {
                    return false;
                }
                self.gaps_out = gaps;
            }
            Diff::ActiveBorder(c) => {
                if self.active_border == c {
                    return false;
                }
                self.active_border = c;
            }
        }
        true
    }

    pub fn held_focused_window_id(&self) -> WindowId {
        let c = &self.snapshot;
        held_focus(&c.focused_window_id, &self.remembered_focused_id, &c.windows, &c.focused_workspace_id)
    }

    pub fn window(&self, id: &WindowId) -> Option<&Window> {
        self.snapshot.windows.iter().find(|w| w.id == *id)
    }
}

/// The compositor's focused window, or across a gap where
/// it reports none (a shell surface took the keyboard) the last one it did,
/// while that window is still open on the focused workspace.
fn held_focus(focused: &WindowId, remembered: &WindowId, windows: &[Window], focused_workspace: &WorkspaceId) -> WindowId {
    if !focused.is_empty() {
        return focused.clone();
    }
    if remembered.is_empty() {
        return WindowId::default();
    }
    match windows.iter().find(|w| w.id == *remembered) {
        Some(w) if focused_workspace.is_empty() || w.workspace_id == *focused_workspace => remembered.clone(),
        _ => WindowId::default(),
    }
}

/// Parked, or on another workspace than the focused one and not shown over
/// it. A window the backend has never heard of counts as parked, so a
/// console toggle asks for it to be brought here.
pub fn is_parked(s: &Snapshot, id: &WindowId) -> bool {
    match s.windows.iter().find(|w| w.id == *id) {
        None => true,
        Some(w) => {
            s.parked.contains(&w.id) || (w.workspace_id != s.focused_workspace_id && !s.overlaid.contains(&w.id))
        }
    }
}

/// The writes, queued from any thread and run in order on the service
/// thread against the backend's own latest model.
pub enum Command {
    FocusWorkspace(WorkspaceId),
    /// A persistent slot the compositor has no workspace for yet.
    FocusWorkspaceAt(i64),
    FocusWindow(WindowId),
    CloseWindow(WindowId),
    Spawn(Vec<String>),
    /// While `argv` is some, a plain Escape runs it without being taken
    /// from the focused app; none removes the bind. `name` keys the bind.
    EscapeBind { name: &'static str, argv: Option<Vec<String>> },
    FloatWindow(WindowId),
    ToggleFloating(WindowId),
    ToggleFullscreen(WindowId),
    /// Onto the workspace slot `idx` of `output`, focus staying put.
    MoveWindowTo { id: WindowId, idx: i64, output: String },
    /// Two windows trade places in the layout, focus staying put.
    SwapWindows { a: WindowId, b: WindowId },
    /// Under a scrolling layout, `id`'s column moves to sit just before
    /// `before`'s, or last with none, the columns between shifting one
    /// along. It focuses `id`.
    MoveColumn { id: WindowId, before: Option<WindowId> },
    PlaceFloatingWindow { id: WindowId, x: f64, y: f64, width: f64, height: f64 },
    ParkWindow(WindowId),
    UnparkWindow(WindowId),
    /// `id` floated at `rect` (logical, absolute) and focused, the rest of
    /// a scrolling workspace's tape slid off the output and held there. The
    /// three zen writes are the shell's own zen; a compositor that owns zen
    /// takes [`Command::ZenMode`] instead.
    Zen { id: WindowId, rect: IRect },
    /// The tape let go, `id` put back at `floating` where it was floating
    /// before, tiled otherwise with its column moved before `before`'s. It
    /// keeps focus.
    Unzen { id: WindowId, floating: Option<IRect>, before: Option<WindowId> },
    /// The focused workspace's tape let go, its zen window having closed.
    ZenRelease,
    /// Zen on the focused window, for a compositor that owns it: on, off,
    /// or none to toggle.
    ZenMode { on: Option<bool> },
    /// The compositor's own overview, for one that draws it: open, closed,
    /// or none to toggle.
    Overview { open: Option<bool> },
    RefreshWindows,
    /// Input went quiet after a burst of keys or pointer motion
    /// (`wayland/settle.rs`): a backend whose events miss some window moves
    /// reads the windows again.
    InputSettled,
    RefreshOutputs,
    SetOutputEnabled(String, bool),
    SetOutputScale(String, f64),
    SetOutputMirror(String, String),
    SetOutputColor(String, Color),
    /// The `mac` submap built again for these reserved chords
    /// (`services/mackeys.rs`).
    MacKeys(Vec<String>),
    /// Whether the window found in front, none for no window, is a Mac
    /// window: into the `mac` submap over it or out of it.
    MacFocus(bool, WindowId),
    /// The pointer hidden or shown on every output, the session's arrival
    /// and departure.
    Cursor { hidden: bool },
    /// The colours and chrome windows are drawn with changed: `colors` the
    /// seven roles off theme.json, `chrome` [`fs_theme::chrome::chrome_json`].
    /// Hyprland reads them again from the files the theme engine wrote, one
    /// reload at a time; Shoji takes them over its socket.
    Theme(Box<fs_wm_proto::Theme>),
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

pub fn focus_workspace(id: &WorkspaceId) {
    send(Command::FocusWorkspace(id.clone()));
}

pub fn focus_workspace_at(idx: i64) {
    if idx >= 1 {
        send(Command::FocusWorkspaceAt(idx));
    }
}

pub fn focus_window(id: &WindowId) {
    send(Command::FocusWindow(id.clone()));
}

pub fn close_window(id: &WindowId) {
    send(Command::CloseWindow(id.clone()));
}

pub fn spawn(argv: &[String]) {
    send(Command::Spawn(argv.to_vec()));
}

/// `argv` started from `dir`: Hyprland's exec takes no working directory,
/// so a shell changes into it first, which serves any compositor.
pub fn spawn_in(dir: &str, argv: &[String]) {
    let mut wrapped: Vec<String> = ["sh", "-c", r#"cd "$1" && shift && exec "$@""#, "sh", dir].map(String::from).to_vec();
    wrapped.extend_from_slice(argv);
    spawn(&wrapped);
}

pub fn escape_bind(name: &'static str, argv: Option<&[String]>) {
    send(Command::EscapeBind { name, argv: argv.map(<[String]>::to_vec) });
}

/// One run of the session's compositor, `None` outside one. An output
/// setting sent over IPC lives as long as that run.
pub fn instance() -> Option<String> {
    instance_in(kind(), &|k| std::env::var(k).ok())
}

/// `instance` for `kind`, its variables read through `var`.
pub fn instance_in(kind: Kind, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    match kind {
        Kind::Hyprland => var(hyprland::INSTANCE_ENV),
        Kind::Shoji => crate::paths::shoji_instance(&var(shoji::CONTROL_ENV)?),
    }
    .filter(|v| !v.is_empty())
}

/// `<runtime>/formalshell/<name>-<instance>`: a file that outlives one
/// shell and belongs to one run of the selected compositor, since socket
/// names like `wayland-1` come back with the next session.
pub fn file(name: &str) -> std::path::PathBuf {
    crate::paths::shell_runtime().join(file_name(name, kind(), &|k| std::env::var(k).ok()))
}

fn file_name(name: &str, kind: Kind, var: &dyn Fn(&str) -> Option<String>) -> String {
    let id = instance_in(kind, var).or_else(|| var("WAYLAND_DISPLAY").filter(|v| !v.is_empty())).unwrap_or_default();
    format!("{name}-{}", id.replace('/', "_"))
}

/// The compositor's binds for the launcher's keybinds route.
pub async fn binds() -> Result<Vec<Bind>, ()> {
    match kind() {
        Kind::Hyprland => hyprland::binds().await,
        Kind::Shoji => shoji::binds().await,
    }
}

/// The pointer in global logical coordinates, `None` when it cannot be
/// asked.
pub async fn cursor_pos() -> Option<(f64, f64)> {
    match kind() {
        Kind::Hyprland => hyprland::cursor_pos().await,
        Kind::Shoji => shoji::cursor_pos().await,
    }
}

/// What [`Command::SetOutputColor`] would send for `row`, unsent, for the
/// `hdr rule` IPC.
pub fn color_request(row: &Output, color: &Color) -> Value {
    match kind() {
        Kind::Hyprland => hyprland::outputs::color_request(row, color),
        Kind::Shoji => shoji::color_request(row, color),
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = COMMANDS.set(tx);
    match kind() {
        Kind::Hyprland => hyprland::run(ctx, rx).await,
        Kind::Shoji => shoji::run(ctx, rx).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_remembered_focus_holds_across_a_gap_while_its_window_stays_on_the_workspace() {
        let windows = vec![Window { id: WindowId::new("a"), workspace_id: WorkspaceId::new("1"), ..Window::default() }];
        let held = |focused: &str, remembered: &str, ws: &str| {
            held_focus(&WindowId::new(focused), &WindowId::new(remembered), &windows, &WorkspaceId::new(ws))
        };
        assert_eq!(held("b", "a", "1"), "b");
        assert_eq!(held("", "a", "1"), "a");
        assert_eq!(held("", "a", "2"), "");
        assert_eq!(held("", "a", ""), "a");
        assert_eq!(held("", "gone", "1"), "");
        assert_eq!(held("", "", "1"), "");
    }

    #[test]
    fn parked_reads_the_snapshot() {
        let w = |id: &str, ws: &str| Window { id: WindowId::new(id), workspace_id: WorkspaceId::new(ws), ..Window::default() };
        let mut s = Snapshot {
            windows: vec![w("here", "1"), w("away", "2"), w("over", "-98"), w("hidden", "1")],
            focused_workspace_id: WorkspaceId::new("1"),
            parked: vec![WindowId::new("hidden")],
            overlaid: vec![WindowId::new("over")],
            ..Snapshot::default()
        };
        let parked = |s: &Snapshot, id: &str| is_parked(s, &WindowId::new(id));
        assert!(!parked(&s, "here"));
        assert!(parked(&s, "away"));
        assert!(!parked(&s, "over"));
        assert!(parked(&s, "hidden"));
        assert!(parked(&s, "unknown"));
        s.overlaid.clear();
        assert!(parked(&s, "over"));
    }

    #[test]
    fn a_compositor_file_belongs_to_the_selected_compositor() {
        let both = |k: &str| match k {
            "HYPRLAND_INSTANCE_SIGNATURE" => Some("host-sig".to_owned()),
            "SHOJIWM_SOCKET" => Some("/run/user/1/shojiwm.wayland-1.sock".to_owned()),
            "WAYLAND_DISPLAY" => Some("wayland-1".to_owned()),
            _ => None,
        };
        assert_eq!(file_name("locked", Kind::Shoji, &both), "locked-shojiwm.wayland-1.sock-0");
        assert_eq!(file_name("locked", Kind::Hyprland, &both), "locked-host-sig");
        assert_eq!(file_name("locked", Kind::Shoji, &|k| (k == "WAYLAND_DISPLAY").then(|| "wayland-1".into())), "locked-wayland-1");
    }
}
