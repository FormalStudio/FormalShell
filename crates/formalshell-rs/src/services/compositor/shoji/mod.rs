//! The ShojiWM backend: FormalShell's engine answers on one
//! newline-delimited JSON socket, the wire in [`proto`] (the 2026-11-11
//! shoji spec, section 3). One connection is held for the shell's life:
//! `state.get` once it is up, then every `state.changed` broadcast, and
//! every write down it in order. The socket going away (the compositor
//! restarting, a broken engine) is the compositor unavailable until it
//! answers again. `binds.list` and `cursor.pos` ask on a connection of
//! their own, so any thread can await them.

mod model;
mod proto;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use async_io::{Async, Timer};
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, StreamExt, io::BufReader};
use fs_system::display::outputs::{Color, Output};
use fs_system::keyboard::Layout;
use fs_types::{Backoff, Bind};
use serde_json::Value;

use self::proto::Method;
use super::{Command, Diff, Gaps, OutputsState, Snapshot};
use crate::runtime::Ctx;
use crate::store;

/// The wait before the next attempt at a socket that is gone or that
/// closes before answering, doubling up to the ceiling while it stays so.
const RETRY_FIRST: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// ShojiWM's control socket (`quit`, `reload`), which marks its session.
const CONTROL_ENV: &str = "SHOJIWM_SOCKET";

pub fn session() -> bool {
    std::env::var_os(CONTROL_ENV).is_some_and(|v| !v.is_empty())
}

/// `shojiwm_rs::ipc::default_socket_path()`. `$SHOJIWM_SOCKET` names the
/// core's own one-line socket beside it, not this one.
fn socket_path() -> Option<PathBuf> {
    let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| "wayland-0".into());
    Some(crate::paths::session_runtime()?.join(format!("shojiwm-{display}.sock")))
}

pub fn instance() -> Option<String> {
    crate::paths::shoji_instance()
}

pub async fn binds() -> Result<Vec<Bind>, ()> {
    let path = socket_path().ok_or(())?;
    let answer = ask(&path, Method::BindsList).await.map_err(|err| eprintln!("shoji: binds.list: {err}"))?;
    serde_json::from_value::<Vec<proto::Bind>>(answer).map(model::binds).map_err(|err| eprintln!("shoji: binds.list: {err}"))
}

pub async fn cursor_pos() -> Option<(f64, f64)> {
    let point: proto::Point = serde_json::from_value(ask(&socket_path()?, Method::CursorPos).await.ok()?).ok()?;
    Some((point.x, point.y))
}

/// What [`Command::SetOutputColor`] sends: the request itself.
pub fn color_request(row: &Output, color: &Color) -> Value {
    let set = proto::OutputsSet { name: row.name.clone(), color: Some(model::color(color)), ..proto::OutputsSet::default() };
    serde_json::to_value(proto::Request { id: None, method: Method::OutputsSet(set) }).unwrap_or_default()
}

/// One request on a connection of its own, its answer or its error.
async fn ask(path: &Path, method: Method) -> io::Result<Value> {
    let stream = Async::<UnixStream>::connect(path).await?;
    (&stream).write_all(proto::line(Some(1), method).as_bytes()).await?;
    let answer = async {
        let mut lines = BufReader::new(&stream).lines();
        while let Some(line) = lines.next().await {
            let msg: proto::Message = serde_json::from_str(&line?).unwrap_or_default();
            if msg.id == Some(1) {
                return match msg.error {
                    Some(err) => Err(io::Error::other(err)),
                    None => Ok(msg.result.unwrap_or(Value::Null)),
                };
            }
        }
        Err(io::ErrorKind::UnexpectedEof.into())
    };
    futures_lite::future::or(answer, async {
        Timer::after(ASK_TIMEOUT).await;
        Err(io::ErrorKind::TimedOut.into())
    })
    .await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    State,
    Outputs,
}

/// What the tasks share, all on the one service thread.
#[derive(Default)]
struct Backend {
    /// The live connection's queue of lines, none while there is none.
    out: Option<async_channel::Sender<String>>,
    next_id: u64,
    pending: HashMap<u64, Ask>,
    /// The last `theme.set`, sent again on every connect.
    theme: Option<String>,
    /// The last of each published value, so an unchanged one is not sent
    /// to the UI thread again with every state.
    snapshot: Option<Snapshot>,
    keyboard: Option<Layout>,
    gaps: Option<Gaps>,
    border: Option<Option<[f32; 3]>>,
    blur: Option<bool>,
    outputs_key: Vec<(String, bool, i64, i64, i64, i64)>,
}

impl Backend {
    fn send(&self, line: String) {
        if let Some(out) = &self.out {
            let _ = out.try_send(line);
        }
    }

    fn ask(&mut self, ask: Ask, method: Method) {
        if self.out.is_none() {
            return;
        }
        self.next_id += 1;
        self.pending.insert(self.next_id, ask);
        self.send(proto::line(Some(self.next_id), method));
    }

    fn forget(&mut self) {
        self.out = None;
        self.pending.clear();
        (self.snapshot, self.keyboard, self.gaps, self.border, self.blur) = (None, None, None, None, None);
        self.outputs_key.clear();
    }
}

type Shared = Rc<RefCell<Backend>>;

pub async fn run(ctx: Ctx, rx: async_channel::Receiver<Command>) {
    let Some(path) = socket_path() else {
        eprintln!("shoji: XDG_RUNTIME_DIR is not set");
        return;
    };
    serve(ctx, rx, path).await
}

async fn serve(ctx: Ctx, rx: async_channel::Receiver<Command>, path: PathBuf) {
    let shared: Shared = Rc::default();
    ctx.spawn(commands(shared.clone(), rx));
    connection(ctx, shared, path).await
}

async fn commands(shared: Shared, rx: async_channel::Receiver<Command>) {
    while let Ok(command) = rx.recv().await {
        let Some(method) = model::method(&command) else { continue };
        let mut b = shared.borrow_mut();
        match method {
            Method::ThemeSet(_) => {
                let line = proto::line(None, method);
                b.send(line.clone());
                b.theme = Some(line);
            }
            Method::OutputsGet => b.ask(Ask::Outputs, method),
            Method::OutputsSet(_) => {
                b.send(proto::line(None, method));
                b.ask(Ask::Outputs, Method::OutputsGet);
            }
            method => b.send(proto::line(None, method)),
        }
    }
}

/// The socket, reconnected for as long as the shell runs.
async fn connection(ctx: Ctx, shared: Shared, path: PathBuf) {
    let mut retry = Backoff::new(RETRY_FIRST, RETRY_MAX);
    let (mut failing, mut connected_once) = (false, false);
    loop {
        let stream = match Async::<UnixStream>::connect(&path).await {
            Ok(stream) => Rc::new(stream),
            Err(err) => {
                if !failing {
                    eprintln!("shoji: no socket at {}: {err}", path.display());
                }
                failing = true;
                Timer::after(retry.delay()).await;
                continue;
            }
        };
        if connected_once {
            eprintln!("shoji: socket reconnected");
        }
        (connected_once, failing) = (true, false);
        let (out, lines) = async_channel::unbounded::<String>();
        ctx.spawn(writer(stream.clone(), lines));
        {
            let mut b = shared.borrow_mut();
            b.out = Some(out);
            b.ask(Ask::State, Method::StateGet);
            if let Some(theme) = b.theme.clone() {
                b.send(theme);
            }
        }

        let mut answered = false;
        let mut lines = BufReader::new(&*stream).lines();
        while let Some(Ok(line)) = lines.next().await {
            if receive(&ctx, &shared, &line) == Some(Ask::State) {
                answered = true;
                retry.reset();
            }
        }
        eprintln!("shoji: socket closed");
        shared.borrow_mut().forget();
        // Gone until it answers again: the honest unavailable state, not
        // the last model it reported.
        ctx.publish(store::Diff::Compositor(Diff::Snapshot(Snapshot::default())));
        // A socket that takes the connection and closes it before any state
        // (a listener that is not the engine) waits like an absent one.
        if !answered {
            Timer::after(retry.delay()).await;
        }
    }
}

/// The connection's writes in the order they were queued, until it closes
/// or the backend lets the queue go.
async fn writer(stream: Rc<Async<UnixStream>>, lines: async_channel::Receiver<String>) {
    while let Ok(line) = lines.recv().await {
        if (&*stream).write_all(line.as_bytes()).await.is_err() {
            return;
        }
    }
}

/// One line off the socket. Returns which ask it answered.
fn receive(ctx: &Ctx, shared: &Shared, line: &str) -> Option<Ask> {
    let msg: proto::Message = match serde_json::from_str(line) {
        Ok(msg) => msg,
        Err(err) => {
            eprintln!("shoji: unreadable line: {err}");
            return None;
        }
    };
    if let Some(id) = msg.id {
        let ask = shared.borrow_mut().pending.remove(&id)?;
        if let Some(err) = msg.error {
            eprintln!("shoji: {ask:?}: {err}");
            if ask == Ask::Outputs {
                ctx.publish(store::Diff::Compositor(Diff::Outputs(Vec::new(), OutputsState::Failed)));
            }
            return None;
        }
        let result = msg.result.unwrap_or(Value::Null);
        match ask {
            Ask::State => apply_state(ctx, shared, result),
            Ask::Outputs => apply_outputs(ctx, result),
        }
        return Some(ask);
    }
    match msg.event.as_deref() {
        Some("state.changed") => apply_state(ctx, shared, msg.payload.unwrap_or(Value::Null)),
        Some("config.reloaded") => ctx.publish(store::Diff::Compositor(Diff::ConfigReloaded)),
        _ => {}
    }
    None
}

/// Publishes `next` when it differs from the last one published.
fn changed<T: PartialEq + Clone>(last: &mut Option<T>, next: &T) -> bool {
    if last.as_ref() == Some(next) {
        return false;
    }
    *last = Some(next.clone());
    true
}

fn apply_state(ctx: &Ctx, shared: &Shared, value: Value) {
    let state: proto::State = match serde_json::from_value(value) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("shoji: unreadable state: {err}");
            return;
        }
    };
    if state.version != model::VERSION {
        eprintln!("shoji: state version {} is not {}", state.version, model::VERSION);
    }
    let snapshot = model::snapshot(&state);
    let keyboard = model::keyboard(&state.keyboard);
    let gaps = model::gaps(&state.chrome);
    let border = model::active_border(&state.chrome.active_border);
    let outputs_key = model::output_key(&state);
    let mut b = shared.borrow_mut();
    if changed(&mut b.snapshot, &snapshot) {
        ctx.publish(store::Diff::Compositor(Diff::Snapshot(snapshot)));
    }
    if changed(&mut b.keyboard, &keyboard) {
        ctx.publish(store::Diff::Compositor(Diff::Keyboard(keyboard)));
    }
    if changed(&mut b.gaps, &gaps) {
        ctx.publish(store::Diff::Compositor(Diff::GapsOut(gaps)));
    }
    if changed(&mut b.border, &border) {
        ctx.publish(store::Diff::Compositor(Diff::ActiveBorder(border)));
    }
    if changed(&mut b.blur, &state.chrome.blur) {
        ctx.publish(store::Diff::Theme(crate::services::theme::Diff::Blur(state.chrome.blur)));
    }
    // `outputs.get` carries what the display panel reads; the state only
    // says when the set of outputs moved.
    if b.outputs_key != outputs_key {
        b.outputs_key = outputs_key;
        b.ask(Ask::Outputs, Method::OutputsGet);
    }
}

fn apply_outputs(ctx: &Ctx, value: Value) {
    let (rows, state) = match serde_json::from_value::<Vec<proto::Output>>(value) {
        Ok(rows) => (model::outputs(&rows), OutputsState::Ok),
        Err(err) => {
            eprintln!("shoji: unreadable outputs: {err}");
            (Vec::new(), OutputsState::Failed)
        }
    };
    crate::services::display::outputs_changed(ctx, &rows);
    ctx.publish(store::Diff::Compositor(Diff::Outputs(rows, state)));
}
