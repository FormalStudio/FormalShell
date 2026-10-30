//! The compositor backend on mango's one socket, the path in
//! `MANGO_INSTANCE_SIGNATURE` (src/ipc/ipc.c at 0.18.0): a `get` or
//! `dispatch` line answers once and closes, a `watch` line answers with the
//! current JSON and again on every change, one object per line.
//!
//! Two watches, `all-monitors` and `all-clients`, carry everything the model
//! needs; a line only replaces its half and marks the model dirty, so the
//! pair mango sends for one change costs one rebuild.
//!
//! What mango has no counterpart for is a logged no-op: a runtime Escape
//! bind that leaves the key with the app, an output's scale, mirror or
//! colour rules.

mod config;
mod dispatch;
pub mod model;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use async_io::{Async, Timer};
use fs_menu::keybinds::Bind;
use fs_system::compositor::keyboard::{self, Layout};
use fs_system::display::outputs::Output;
use futures_lite::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, StreamExt, io::BufReader};

use super::model::{self as common, Snapshot};
use super::{Command, Diff, OutputsState};
use crate::runtime::Ctx;
use crate::store;

const RETRY_FIRST: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);

fn socket() -> io::Result<PathBuf> {
    std::env::var_os("MANGO_INSTANCE_SIGNATURE")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "MANGO_INSTANCE_SIGNATURE is not set"))
}

async fn connect(line: &str) -> io::Result<Async<UnixStream>> {
    let mut stream = Async::<UnixStream>::connect(socket()?).await?;
    stream.write_all(format!("{line}\n").as_bytes()).await?;
    Ok(stream)
}

async fn request(line: &str) -> io::Result<String> {
    let mut stream = connect(line).await?;
    let mut out = String::new();
    stream.read_to_string(&mut out).await?;
    Ok(out)
}

async fn dispatch(line: &str) {
    match request(&format!("dispatch {line}")).await {
        Ok(text) if text.contains("\"success\":true") => {}
        Ok(text) => eprintln!("mango: dispatch {line}: {}", text.trim()),
        Err(err) => eprintln!("mango: dispatch {line}: {err}"),
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn read_config() -> Option<config::Read> {
    let home = home();
    let entry = config::entry(&home)?;
    config::read(&entry, &home, &|p| std::fs::read_to_string(p).ok())
}

pub async fn binds() -> Result<Vec<Bind>, ()> {
    read_config().map(|r| r.binds).ok_or(())
}

pub async fn cursor_pos() -> Option<(f64, f64)> {
    let text = request("get cursorpos").await.ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((v.get("x")?.as_f64()?, v.get("y")?.as_f64()?))
}

/// mango watches its own config but not the files it sources, which is
/// where the shell's colours and chrome land.
pub async fn reload() {
    dispatch(dispatch::RELOAD).await;
}

/// What the tasks share, all on the one service thread.
#[derive(Default)]
struct Backend {
    monitors: String,
    clients: String,
    names: Vec<String>,
    layouts: Vec<String>,
    order: Vec<String>,
    snapshot: Snapshot,
    outputs: Option<Vec<Output>>,
    keyboard: Option<Layout>,
    /// The last box each window was placed at, put back after the window
    /// changes monitor.
    placed: HashMap<String, (i64, i64, i64, i64)>,
}

type Shared = Rc<RefCell<Backend>>;

pub async fn run(ctx: Ctx, rx: async_channel::Receiver<Command>) {
    if let Err(err) = socket() {
        eprintln!("mango: {err}");
        return;
    }
    let shared: Shared = Rc::default();
    shared.borrow_mut().layouts = read_config().map(|r| r.layouts).unwrap_or_default();
    let (dirty_tx, dirty_rx) = async_channel::bounded::<()>(1);
    ctx.spawn(watch(ctx.clone(), shared.clone(), Half::Monitors, dirty_tx.clone()));
    ctx.spawn(watch(ctx.clone(), shared.clone(), Half::Clients, dirty_tx.clone()));
    ctx.spawn(refresher(ctx.clone(), shared.clone(), dirty_rx));
    commands(shared, rx, dirty_tx).await;
}

#[derive(Clone, Copy)]
enum Half {
    Monitors,
    Clients,
}

impl Half {
    fn topic(self) -> &'static str {
        match self {
            Half::Monitors => "all-monitors",
            Half::Clients => "all-clients",
        }
    }

    fn slot(self, b: &mut Backend) -> &mut String {
        match self {
            Half::Monitors => &mut b.monitors,
            Half::Clients => &mut b.clients,
        }
    }
}

/// One watch, reconnected for as long as the shell runs.
async fn watch(ctx: Ctx, shared: Shared, half: Half, dirty: async_channel::Sender<()>) {
    let mut wait = RETRY_FIRST;
    let mut connected_once = false;
    loop {
        let stream = match connect(&format!("watch {}", half.topic())).await {
            Ok(stream) => stream,
            Err(err) => {
                if wait == RETRY_FIRST {
                    eprintln!("mango: no {} watch: {err}", half.topic());
                }
                Timer::after(wait).await;
                wait = (wait * 2).min(RETRY_MAX);
                continue;
            }
        };
        if connected_once {
            eprintln!("mango: {} watch reconnected", half.topic());
        }
        connected_once = true;
        wait = RETRY_FIRST;
        let mut lines = BufReader::new(stream).lines();
        while let Some(Ok(line)) = lines.next().await {
            *half.slot(&mut shared.borrow_mut()) = line;
            let _ = dirty.try_send(());
        }
        eprintln!("mango: {} watch closed", half.topic());
        half.slot(&mut shared.borrow_mut()).clear();
        // Gone until it answers again: the honest unavailable state, not
        // the last model it reported.
        shared.borrow_mut().snapshot = Snapshot::default();
        ctx.publish(store::Diff::Compositor(Diff::Snapshot(Snapshot::default())));
        Timer::after(wait).await;
    }
}

async fn refresher(ctx: Ctx, shared: Shared, dirty: async_channel::Receiver<()>) {
    while dirty.recv().await.is_ok() {
        let (snapshot, outputs, keyboard) = {
            let mut b = shared.borrow_mut();
            let mut snapshot = model::snapshot(&b.monitors, &b.clients, &b.names);
            snapshot.windows = common::keep_order(&mut b.order, snapshot.windows);
            b.snapshot = snapshot.clone();
            let rows = model::outputs(&b.monitors);
            let outputs = (b.outputs.as_ref() != Some(&rows) && !b.monitors.is_empty()).then(|| {
                b.outputs = Some(rows.clone());
                rows
            });
            let layout = match model::keyboard_layout(&b.monitors) {
                Some(current) => Layout { available: true, names: b.layouts.clone(), current_idx: -1, current },
                None => keyboard::unavailable(),
            };
            let keyboard = (b.keyboard.as_ref() != Some(&layout) && !b.monitors.is_empty()).then(|| {
                b.keyboard = Some(layout.clone());
                layout
            });
            (snapshot, outputs, keyboard)
        };
        ctx.publish(store::Diff::Compositor(Diff::Snapshot(snapshot)));
        if let Some(rows) = outputs {
            crate::services::display::outputs_changed(&ctx, &rows);
            ctx.publish(store::Diff::Compositor(Diff::Outputs(rows, OutputsState::Ok)));
        }
        if let Some(layout) = keyboard {
            ctx.publish(store::Diff::Compositor(Diff::Keyboard(layout)));
        }
    }
}

fn unsupported(what: &str) {
    eprintln!("mango: {what} has no mango counterpart, left as it is");
}

async fn commands(shared: Shared, rx: async_channel::Receiver<Command>, dirty: async_channel::Sender<()>) {
    while let Ok(command) = rx.recv().await {
        match command {
            Command::FocusWorkspace(id) => match model::split_workspace_id(&id) {
                Some((output, tag)) => dispatch(&dispatch::view(output, tag)).await,
                None => eprintln!("mango: no workspace {id}"),
            },
            Command::FocusWorkspaceAt { idx, output } => {
                let output = if output.is_empty() { shared.borrow().snapshot.focused_output_name.clone() } else { output };
                dispatch(&dispatch::view(&output, idx)).await;
            }
            Command::FocusWindow(id) => dispatch(&dispatch::focus_window(&id)).await,
            Command::CloseWindow(id) => dispatch(&dispatch::close_window(&id)).await,
            Command::Spawn(argv) => dispatch(&dispatch::spawn(&argv)).await,
            Command::EscapeBind { argv: Some(_), .. } => unsupported("a runtime Escape bind"),
            Command::EscapeBind { argv: None, .. } => {}
            Command::FloatWindow(id) => {
                let floating = shared.borrow().snapshot.windows.iter().any(|w| w.id == id && w.is_floating);
                if !floating {
                    dispatch(&dispatch::float_window(&id)).await;
                }
            }
            Command::ToggleFloating(id) => dispatch(&dispatch::float_window(&id)).await,
            Command::ToggleFullscreen(_) => unsupported("a fullscreen toggle"),
            Command::MoveWindowTo { id, idx, output } => {
                let output = if output.is_empty() { shared.borrow().snapshot.focused_output_name.clone() } else { output };
                dispatch(&dispatch::move_to(&id, &output, idx)).await;
            }
            Command::SwapWindows { .. } => unsupported("a window swap"),
            Command::MoveColumn { .. } => unsupported("a column move"),
            Command::PlaceFloatingWindow { id, x, y, width, height } => {
                let target = (x.round() as i64, y.round() as i64, width.round() as i64, height.round() as i64);
                shared.borrow_mut().placed.insert(id.clone(), target);
                place(&id, target).await;
            }
            Command::ParkWindow(id) => {
                let on_special = shared
                    .borrow()
                    .snapshot
                    .windows
                    .iter()
                    .any(|w| w.id == id && model::split_workspace_id(&w.workspace_id).is_some_and(|(_, tag)| tag == 0));
                if !on_special {
                    dispatch(&dispatch::park_window(&id)).await;
                }
            }
            Command::UnparkWindow(id) => {
                let here = model::split_workspace_id(&shared.borrow().snapshot.focused_workspace_id).map(|(o, t)| (o.to_owned(), t));
                let Some((output, tag)) = here else {
                    eprintln!("mango: no focused tag to bring {id} to");
                    continue;
                };
                dispatch(&dispatch::move_to(&id, &output, tag)).await;
                let target = shared.borrow().placed.get(&id).copied();
                if let Some(target) = target {
                    place(&id, target).await;
                }
            }
            Command::RefreshWindows | Command::RefreshOutputs => {
                let _ = dirty.try_send(());
            }
            Command::SetOutputEnabled(name, on) => dispatch(&dispatch::enable_output(&name, on)).await,
            Command::SetOutputScale(..) => unsupported("an output scale change"),
            Command::SetOutputMirror(..) => unsupported("output mirroring"),
            Command::SetOutputColor(..) => unsupported("an output colour mode"),
            Command::Cursor { .. } => unsupported("a cursor visibility change"),
            Command::Exit => dispatch(dispatch::QUIT).await,
            Command::WorkspaceNames(names) => {
                shared.borrow_mut().names = names;
                let _ = dirty.try_send(());
            }
        }
    }
}

async fn place(id: &str, (x, y, w, h): (i64, i64, i64, i64)) {
    dispatch(&dispatch::resize_window(id, w, h)).await;
    dispatch(&dispatch::move_window(id, x, y)).await;
}
