//! The Hyprland backend on its two raw sockets: `.socket.sock` answers one request per connection,
//! `.socket2.sock` streams `EVENT>>DATA` lines. Every write is an `hl.*`
//! Lua call, a dispatcher sent as `dispatch` and a monitor rule as `eval`.
//!
//! An event only marks the model dirty; one refresh re-reads it, so a burst
//! of events in one turn costs one round of requests.

mod lua;
pub mod model;

use std::cell::RefCell;
use std::collections::HashSet;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use async_io::{Async, Timer};
use fs_menu::keybinds::{self, Bind};
use futures_lite::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, StreamExt, io::BufReader};
use fs_system::compositor::keyboard::{self, Layout};
use fs_system::display::outputs::{self, Output, RuleOverrides};

use super::model::{self as common, Snapshot};
use super::{Command, Diff, OutputsState};
use crate::runtime::Ctx;
use crate::store;

/// The wait before the next attempt at a socket that is gone, doubling up
/// to the ceiling while it stays gone.
const RETRY_FIRST: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);

fn socket_dir() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is not set"))?;
    let sig = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HYPRLAND_INSTANCE_SIGNATURE is not set"))?;
    Ok(PathBuf::from(runtime).join("hypr").join(sig))
}

async fn request(command: &str) -> io::Result<String> {
    let mut stream = Async::<UnixStream>::connect(socket_dir()?.join(".socket.sock")).await?;
    stream.write_all(command.as_bytes()).await?;
    let mut out = String::new();
    stream.read_to_string(&mut out).await?;
    Ok(out)
}

pub async fn binds() -> Result<Vec<Bind>, ()> {
    request("binds").await.map(|text| keybinds::parse_hyprland_binds(&text)).map_err(|_| ())
}

pub async fn cursor_pos() -> Option<(f64, f64)> {
    let text = request("j/cursorpos").await.ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?))
}

/// What the tasks share, all on the one service thread.
#[derive(Default)]
struct Backend {
    snapshot: Snapshot,
    special_shown: String,
    order: Vec<String>,
    urgent: HashSet<String>,
    outputs: Vec<Output>,
    keyboard: Option<Layout>,
}

type Shared = Rc<RefCell<Backend>>;

pub async fn run(ctx: Ctx, rx: async_channel::Receiver<Command>) {
    if let Err(err) = socket_dir() {
        eprintln!("hyprland: {err}");
        return;
    }
    let shared: Shared = Rc::default();
    let (dirty_tx, dirty_rx) = async_channel::bounded::<()>(1);

    ctx.spawn(refresher(ctx.clone(), shared.clone(), dirty_rx));
    ctx.spawn(commands(ctx.clone(), shared.clone(), rx, dirty_tx.clone()));
    events(ctx, shared, dirty_tx).await;
}

/// The event stream, reconnected for as long as the shell runs: a Hyprland
/// that restarts its socket comes back to a full re-read.
async fn events(ctx: Ctx, shared: Shared, dirty: async_channel::Sender<()>) {
    let mut wait = RETRY_FIRST;
    let mut connected_once = false;
    loop {
        let stream = match socket_dir() {
            Ok(dir) => Async::<UnixStream>::connect(dir.join(".socket2.sock")).await,
            Err(err) => Err(err),
        };
        let stream = match stream {
            Ok(stream) => stream,
            Err(err) => {
                if wait == RETRY_FIRST {
                    eprintln!("hyprland: no event socket: {err}");
                }
                Timer::after(wait).await;
                wait = (wait * 2).min(RETRY_MAX);
                continue;
            }
        };
        if connected_once {
            eprintln!("hyprland: event socket reconnected");
        }
        connected_once = true;
        wait = RETRY_FIRST;
        let _ = dirty.try_send(());
        refresh_outputs(&ctx, &shared).await;
        refresh_keyboard(&ctx, &shared).await;
        refresh_blur(&ctx).await;

        let mut lines = BufReader::new(stream).lines();
        while let Some(line) = lines.next().await {
            let Ok(line) = line else { break };
            let (name, data) = line.split_once(">>").unwrap_or((line.as_str(), ""));
            match name {
                "urgent" => {
                    if let Some(id) = model::address_id(data) {
                        shared.borrow_mut().urgent.insert(id);
                    }
                }
                "activelayout" => {
                    let next = shared.borrow().keyboard.as_ref().map(|k| keyboard::apply_active_layout(k, data));
                    if let Some(layout) = next {
                        shared.borrow_mut().keyboard = Some(layout.clone());
                        ctx.publish(store::Diff::Compositor(Diff::Keyboard(layout)));
                    }
                }
                "configreloaded" => {
                    ctx.publish(store::Diff::Compositor(Diff::ConfigReloaded));
                    refresh_outputs(&ctx, &shared).await;
                    refresh_keyboard(&ctx, &shared).await;
                    refresh_blur(&ctx).await;
                }
                "monitoradded" | "monitoraddedv2" | "monitorremoved" | "monitorremovedv2" => {
                    refresh_outputs(&ctx, &shared).await;
                }
                _ => {}
            }
            if !model::ignores(name) {
                let _ = dirty.try_send(());
            }
        }
        eprintln!("hyprland: event socket closed");
        // Gone until it answers again: the honest unavailable state, not
        // the last model it reported.
        shared.borrow_mut().snapshot = Snapshot::default();
        ctx.publish(store::Diff::Compositor(Diff::Snapshot(Snapshot::default())));
    }
}

async fn refresher(ctx: Ctx, shared: Shared, dirty: async_channel::Receiver<()>) {
    while dirty.recv().await.is_ok() {
        refresh_compositor(&ctx, &shared).await;
    }
}

async fn refresh_compositor(ctx: &Ctx, shared: &Shared) {
    let read = async {
        let monitors = request("j/monitors").await?;
        let workspaces = request("j/workspaces").await?;
        let clients = request("j/clients").await?;
        let active = request("j/activewindow").await?;
        io::Result::Ok((monitors, workspaces, clients, active))
    };
    let (monitors, workspaces, clients, active) = match read.await {
        Ok(answers) => answers,
        Err(err) => {
            eprintln!("hyprland: refresh: {err}");
            return;
        }
    };
    let snapshot = {
        let mut b = shared.borrow_mut();
        let read = model::snapshot(&monitors, &workspaces, &clients, &active, &b.urgent);
        let mut snapshot = read.snapshot;
        b.special_shown = read.special_shown;
        snapshot.windows = common::keep_order(&mut b.order, snapshot.windows);
        let live: HashSet<&str> = snapshot.windows.iter().filter(|w| w.is_urgent).map(|w| w.id.as_str()).collect();
        b.urgent.retain(|id| live.contains(id.as_str()));
        b.snapshot = snapshot.clone();
        snapshot
    };
    ctx.publish(store::Diff::Compositor(Diff::Snapshot(snapshot)));
}

/// `monitors all`: the only enumeration that lists a disabled output, which
/// is the one the display panel needs to switch back on.
async fn refresh_outputs(ctx: &Ctx, shared: &Shared) {
    let (rows, state) = match request("j/monitors all").await {
        Ok(text) => (outputs::parse_hyprland_outputs(&text), OutputsState::Ok),
        Err(err) => {
            eprintln!("hyprland: outputs: {err}");
            (Vec::new(), OutputsState::Failed)
        }
    };
    crate::services::display::outputs_changed(ctx, &rows);
    shared.borrow_mut().outputs = rows.clone();
    ctx.publish(store::Diff::Compositor(Diff::Outputs(rows, state)));
}

async fn refresh_keyboard(ctx: &Ctx, shared: &Shared) {
    let layout = match request("j/devices").await {
        Ok(text) => keyboard::parse_hyprland_layouts(&text),
        Err(_) => keyboard::unavailable(),
    };
    shared.borrow_mut().keyboard = Some(layout.clone());
    ctx.publish(store::Diff::Compositor(Diff::Keyboard(layout)));
}

/// `decoration:blur:enabled`, which decides whether the cards may be
/// translucent. An answer it cannot read counts as no blur.
async fn refresh_blur(ctx: &Ctx) {
    let blur = request("j/getoption decoration:blur:enabled").await.is_ok_and(|text| blur_enabled(&text));
    ctx.publish(store::Diff::Theme(crate::services::theme::Diff::Blur(blur)));
}

/// getoption's JSON carries the value under its type's key: `int` for the
/// hyprlang config, `bool` under a Lua one.
fn blur_enabled(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return false };
    v.get("bool").and_then(serde_json::Value::as_bool).or_else(|| v.get("int").and_then(serde_json::Value::as_i64).map(|i| i != 0)).unwrap_or(false)
}

async fn dispatch(lua: &str) {
    report("dispatch", request(&format!("dispatch {lua}")).await);
}

/// Monitor rules are config calls rather than dispatchers; `hyprctl eval`
/// is the documented way in under a Lua config.
async fn eval(ctx: &Ctx, lua: &str) {
    let lua = lua.to_owned();
    let out = ctx.pool().run(move || crate::services::proc::std_command("hyprctl").args(["eval", &lua]).output()).await;
    let answer = match out {
        Some(Ok(out)) if out.status.success() => Ok(String::from_utf8_lossy(&out.stdout).into_owned()),
        Some(Ok(out)) => Ok(String::from_utf8_lossy(&out.stderr).into_owned()),
        Some(Err(err)) => Err(err),
        None => Err(io::Error::other("the blocking pool is gone")),
    };
    report("eval", answer);
}

fn report(what: &str, answer: io::Result<String>) {
    match answer {
        Ok(text) if text.trim() == "ok" => {}
        Ok(text) => eprintln!("hyprland: {what}: {}", text.trim()),
        Err(err) => eprintln!("hyprland: {what}: {err}"),
    }
}

async fn commands(
    ctx: Ctx,
    shared: Shared,
    rx: async_channel::Receiver<Command>,
    dirty: async_channel::Sender<()>,
) {
    while let Ok(command) = rx.recv().await {
        match command {
            Command::FocusWorkspace(id) => dispatch(&lua::focus_workspace(&id)).await,
            // Hyprland's `workspace` dispatcher makes the workspace on the
            // focused monitor; it has no per-output slots.
            Command::FocusWorkspaceAt { idx, .. } => dispatch(&lua::focus_workspace(&idx.to_string())).await,
            Command::FocusWindow(id) => dispatch(&lua::focus_window(&id)).await,
            Command::CloseWindow(id) => dispatch(&lua::close_window(&id)).await,
            Command::Spawn(argv) => dispatch(&lua::spawn(&argv)).await,
            Command::EscapeBind { name, argv } => {
                let lua = match argv {
                    Some(argv) => lua::bind_escape(name, &argv),
                    None => lua::unbind(name),
                };
                eval(&ctx, &lua).await;
            }
            Command::WorkspaceNames(_) => {}
            Command::Cursor { hidden } => eval(&ctx, &lua::cursor(hidden)).await,
            Command::Exit => dispatch(&lua::exit()).await,
            Command::FloatWindow(id) => {
                let floating = shared.borrow().snapshot.windows.iter().any(|w| w.id == id && w.is_floating);
                if !floating {
                    dispatch(&lua::float_window(&id)).await;
                }
            }
            Command::ToggleFloating(id) => dispatch(&lua::toggle_floating(&id)).await,
            Command::ToggleFullscreen(id) => dispatch(&lua::toggle_fullscreen(&id)).await,
            Command::MoveWindowTo { id, idx, .. } => dispatch(&lua::move_to_workspace(&id, idx)).await,
            Command::SwapWindows { a, b } => {
                dispatch(&lua::swap_windows(&a, &b)).await;
                // A swap moves no window between workspaces and raises no
                // event, so the rects are asked for again.
                let _ = dirty.try_send(());
            }
            Command::PlaceFloatingWindow { id, x, y, width, height } => {
                let (w, h) = ((width.round() as i64).max(1), (height.round() as i64).max(1));
                dispatch(&lua::resize_window(&id, w, h)).await;
                dispatch(&lua::move_window(&id, x.round() as i64, y.round() as i64)).await;
            }
            Command::ParkWindow(id) => park(&ctx, &shared, &id, false).await,
            Command::UnparkWindow(id) => park(&ctx, &shared, &id, true).await,
            Command::RefreshWindows => {
                let _ = dirty.try_send(());
            }
            Command::RefreshOutputs => refresh_outputs(&ctx, &shared).await,
            Command::SetOutputEnabled(name, enabled) => {
                eval(&ctx, &outputs::hyprland_enabled_lua(&name, enabled)).await;
                refresh_outputs(&ctx, &shared).await;
            }
            Command::SetOutputScale(name, scale) => {
                let row = outputs::find_output(&shared.borrow().outputs, &name).cloned();
                if let Some(row) = row {
                    let overrides = RuleOverrides { scale: Some(scale), mirror_of: None };
                    eval(&ctx, &outputs::hyprland_rule_lua(&outputs::hyprland_monitor_rule(&row, &overrides))).await;
                    refresh_outputs(&ctx, &shared).await;
                }
            }
            Command::SetOutputMirror(name, source) => {
                let row = outputs::find_output(&shared.borrow().outputs, &name).cloned();
                if let Some(row) = row {
                    let overrides = RuleOverrides { scale: None, mirror_of: Some(source) };
                    eval(&ctx, &outputs::hyprland_rule_lua(&outputs::hyprland_monitor_rule(&row, &overrides))).await;
                    refresh_outputs(&ctx, &shared).await;
                }
            }
            Command::SetOutputColor(name, color) => {
                let row = outputs::find_output(&shared.borrow().outputs, &name).cloned();
                if let Some(row) = row.filter(|r| r.enabled) {
                    eval(&ctx, &outputs::hyprland_rule_lua(&outputs::hyprland_color_rule(&row, &color))).await;
                    refresh_outputs(&ctx, &shared).await;
                }
            }
        }
    }
}

/// The console lives on its special workspace for good; showing it is the
/// compositor's own toggle of that workspace. `show` false parks it.
async fn park(ctx: &Ctx, shared: &Shared, id: &str, show: bool) {
    let on_special = {
        let b = shared.borrow();
        b.snapshot.parked.iter().chain(&b.snapshot.overlaid).any(|w| w == id)
    };
    if !on_special {
        dispatch(&lua::park_window(id)).await;
    }
    let shown = shared.borrow().special_shown == lua::PARK_WORKSPACE;
    if shown != show {
        dispatch(&lua::toggle_special()).await;
        refresh_compositor(ctx, shared).await;
    }
}
