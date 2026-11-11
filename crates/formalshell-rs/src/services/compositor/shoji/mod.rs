//! The ShojiWM backend: FormalShell's engine answers on one
//! newline-delimited JSON socket, the wire in [`fs_wm_proto`] (the
//! 2026-11-11 shoji spec, section 3). One connection is held for the shell's life:
//! `state.get` once it is up, then every `state.changed` broadcast, and
//! every write down it in order. The socket going away (the compositor
//! restarting, a broken engine) is the compositor unavailable until it
//! answers again. `binds.list` and `cursor.pos` ask on a connection of
//! their own, so any thread can await them.

mod model;
#[cfg(test)]
mod tests;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use async_io::{Async, Timer};
use futures_lite::{AsyncBufReadExt, AsyncWriteExt, StreamExt, io::BufReader};
use fs_system::display::outputs::{Color, Output};
use fs_system::keyboard::Layout;
use fs_info::notifications::Urgency;
use fs_types::{Backoff, Bind};
use fs_wm_proto::{self as proto, event, method};
use serde::Deserialize;
use serde_json::Value;

use self::model::Call;
use super::{Command, Diff, Gaps, OutputsState, Snapshot};
use crate::runtime::Ctx;
use crate::services::notifications::{self, Op};
use crate::store;

/// The wait before the next attempt at a socket that is gone or that
/// closes before answering, doubling up to the ceiling while it stays so.
const RETRY_FIRST: Duration = Duration::from_millis(250);
const RETRY_MAX: Duration = Duration::from_secs(5);
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

/// ShojiWM's control socket (`quit`, `reload`), which marks its session.
pub const CONTROL_ENV: &str = "SHOJIWM_SOCKET";

pub fn session() -> bool {
    std::env::var_os(CONTROL_ENV).is_some_and(|v| !v.is_empty())
}

/// The JSON socket, under the runtime directory the shell trusts.
/// `$SHOJIWM_SOCKET` names the core's own one-line socket beside it.
fn socket_path() -> Option<PathBuf> {
    let runtime = crate::paths::session_runtime()?;
    let display = std::env::var("WAYLAND_DISPLAY").ok().filter(|d| !d.is_empty());
    Some(proto::socket_path(Some(&runtime.to_string_lossy()), display.as_deref()).into())
}

pub async fn binds() -> Result<Vec<Bind>, ()> {
    let path = socket_path().ok_or(())?;
    let answer = ask(&path, Call::bare(method::BINDS_LIST)).await.map_err(|err| eprintln!("shoji: binds.list: {err}"))?;
    serde_json::from_value::<Vec<proto::Bind>>(answer).map(model::binds).map_err(|err| eprintln!("shoji: binds.list: {err}"))
}

pub async fn cursor_pos() -> Option<(f64, f64)> {
    let point: proto::Point = serde_json::from_value(ask(&socket_path()?, Call::bare(method::CURSOR_POS)).await.ok()?).ok()?;
    Some((point.x, point.y))
}

/// What [`Command::SetOutputColor`] sends: the request itself.
pub fn color_request(row: &Output, color: &Color) -> Value {
    let set = proto::OutputSet { name: row.name.clone(), color: Some(model::color(color)), ..proto::OutputSet::default() };
    serde_json::to_value(Call::with(method::OUTPUTS_SET, set).request(None)).unwrap_or_default()
}

/// One line from the engine.
#[derive(Deserialize)]
#[serde(untagged)]
enum Incoming {
    Answer(proto::Answer<Value>),
    Broadcast(proto::Broadcast<Value>),
}

/// One request on a connection of its own, its answer or its error.
async fn ask(path: &Path, call: Call) -> io::Result<Value> {
    let stream = Async::<UnixStream>::connect(path).await?;
    (&stream).write_all(call.line(Some(1)).as_bytes()).await?;
    let answer = async {
        let mut lines = BufReader::new(&stream).lines();
        while let Some(line) = lines.next().await {
            if let Ok(Incoming::Answer(answer)) = serde_json::from_str(&line?)
                && answer.id == 1
            {
                return match answer.outcome {
                    proto::Outcome::Result(result) => Ok(result),
                    proto::Outcome::Error(err) => Err(io::Error::other(err)),
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
    /// Each unanswered request and when it went out.
    pending: HashMap<u64, (Ask, Instant)>,
    /// The last `theme.set`, sent again on every connect.
    theme: Option<String>,
    /// The last of each published value, so an unchanged one is not sent
    /// to the UI thread again with every state.
    snapshot: Option<Snapshot>,
    keyboard: Option<Layout>,
    gaps: Option<Gaps>,
    border: Option<Option<[f32; 3]>>,
    blur: Option<bool>,
    outputs_key: Vec<(String, bool, i32, i32, i32, i32)>,
}

impl Backend {
    fn send(&self, line: String) {
        if let Some(out) = &self.out {
            let _ = out.try_send(line);
        }
    }

    fn ask(&mut self, ask: Ask, call: &Call) {
        if self.out.is_none() {
            return;
        }
        self.next_id += 1;
        self.pending.insert(self.next_id, (ask, Instant::now()));
        self.send(call.line(Some(self.next_id)));
    }

    fn oldest(&self) -> Option<Instant> {
        self.pending.values().map(|&(_, at)| at).min()
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
    serve(ctx, rx, path, ASK_TIMEOUT).await
}

/// `answer_within`: how long a request on the live connection may go
/// unanswered before the connection counts as gone.
async fn serve(ctx: Ctx, rx: async_channel::Receiver<Command>, path: PathBuf, answer_within: Duration) {
    let shared: Shared = Rc::default();
    ctx.spawn(commands(shared.clone(), rx));
    connection(ctx, shared, path, answer_within).await
}

async fn commands(shared: Shared, rx: async_channel::Receiver<Command>) {
    while let Ok(command) = rx.recv().await {
        let Some(call) = model::method(&command) else { continue };
        let mut b = shared.borrow_mut();
        match call.method {
            method::THEME_SET => {
                let line = call.line(None);
                b.send(line.clone());
                b.theme = Some(line);
            }
            method::OUTPUTS_GET => b.ask(Ask::Outputs, &call),
            method::OUTPUTS_SET => {
                b.send(call.line(None));
                b.ask(Ask::Outputs, &Call::bare(method::OUTPUTS_GET));
            }
            _ => b.send(call.line(None)),
        }
    }
}

/// The socket, reconnected for as long as the shell runs.
async fn connection(ctx: Ctx, shared: Shared, path: PathBuf, answer_within: Duration) {
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
            b.ask(Ask::State, &Call::bare(method::STATE_GET));
            if let Some(theme) = b.theme.clone() {
                b.send(theme);
            }
        }

        let mut answered = false;
        let mut lines = BufReader::new(&*stream).lines();
        loop {
            // Woken at the oldest request's deadline, or a timeout from now
            // to look again at one sent while this read waited.
            let wake = shared.borrow().oldest().unwrap_or_else(Instant::now) + answer_within;
            let read = futures_lite::future::or(async { Some(lines.next().await.and_then(Result::ok)) }, async {
                Timer::at(wake).await;
                None
            })
            .await;
            match read {
                Some(Some(line)) => {
                    if receive(&ctx, &shared, &line) == Some(Ask::State) {
                        answered = true;
                        retry.reset();
                    }
                }
                Some(None) => break,
                // An engine that holds the socket open and answers nothing
                // is as gone as a closed one.
                None if shared.borrow().oldest().is_some_and(|at| at.elapsed() >= answer_within) => {
                    eprintln!("shoji: no answer within {answer_within:?}");
                    answered = false;
                    if shared.borrow().pending.values().any(|&(ask, _)| ask == Ask::Outputs) {
                        ctx.publish(store::Diff::Compositor(Diff::Outputs(Vec::new(), OutputsState::Failed)));
                    }
                    break;
                }
                None => {}
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
    let broadcast = match serde_json::from_str(line) {
        Ok(Incoming::Broadcast(broadcast)) => broadcast,
        Ok(Incoming::Answer(answer)) => {
            let (ask, _) = shared.borrow_mut().pending.remove(&answer.id)?;
            match answer.outcome {
                proto::Outcome::Result(result) => match ask {
                    Ask::State => apply_state(ctx, shared, result),
                    Ask::Outputs => apply_outputs(ctx, result),
                },
                proto::Outcome::Error(err) => {
                    eprintln!("shoji: {ask:?}: {err}");
                    if ask == Ask::Outputs {
                        ctx.publish(store::Diff::Compositor(Diff::Outputs(Vec::new(), OutputsState::Failed)));
                    }
                    return None;
                }
            }
            return Some(ask);
        }
        Err(err) => {
            eprintln!("shoji: unreadable line: {err}");
            return None;
        }
    };
    match broadcast.event.as_str() {
        event::STATE_CHANGED => apply_state(ctx, shared, broadcast.payload),
        event::CONFIG_RELOADED => ctx.publish(store::Diff::Compositor(Diff::ConfigReloaded)),
        event::CONFIG_FAILED => config_failed(ctx, broadcast.payload),
        _ => {}
    }
    None
}

/// A refused settings file, toasted like any failed operation: the
/// running settings stand, so nothing else changes.
fn config_failed(ctx: &Ctx, payload: Value) {
    let error = serde_json::from_value::<proto::ConfigFailed>(payload).map_or_else(|err| format!("unreadable report: {err}"), |f| f.error);
    eprintln!("shoji: settings refused: {error}");
    let op = Op::Notify("Window manager settings refused".into(), error, Urgency::Critical);
    ctx.publish(store::Diff::Notifications(notifications::Diff::Op(op)));
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
    if state.version != proto::VERSION {
        eprintln!("shoji: state version {} is not {}", state.version, proto::VERSION);
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
        b.ask(Ask::Outputs, &Call::bare(method::OUTPUTS_GET));
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
