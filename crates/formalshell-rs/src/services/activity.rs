//! Activity: fspec's operation stream, `com.canonical.Unity.LauncherEntry`
//! on the session bus and the notification `value` hint (fed by
//! `services/notifications.rs`), folded into one [`fs_activity::Activity`]
//! on the service thread and published whole when it changes.
//!
//! Nothing here polls. fspec operations are found by inotify on their
//! directory and read off their sockets; LauncherEntry is a match rule; the
//! only timer is the next fold, armed while a finished item waits for it.

use std::collections::{HashMap, HashSet};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_io::{Async, Timer};
use fs_activity::launcher::{self, Entry, Prop};
use fs_activity::progress::Fraction;
use fs_activity::{Activity, Notice, State as ItemState, fspec};
use futures_lite::{AsyncReadExt, StreamExt};
use serde_json::json;
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{MatchRule, MessageStream, fdo};

use super::notifications::now_ms;
use crate::runtime::Ctx;
use crate::services::appicon::{applications_dirs, scan_launchable};
use crate::store;

const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
/// A line longer than this is not fspec's: the connection is dropped.
const MAX_LINE: usize = 1 << 20;
/// Connections in a row that end before a single line, after which a
/// socket that still exists is taken for dead.
const EMPTY_RECONNECTS: u32 = 3;
const RECONNECT_GAP: Duration = Duration::from_millis(100);
const LISTEN_GAP: Duration = Duration::from_millis(50);

/// What the stream reader knows about fspec on this machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Fspec {
    /// `fspec` is on PATH.
    pub installed: bool,
    pub dir: Option<PathBuf>,
    /// The directory is watched for new operations.
    pub watching: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct State {
    pub activity: Activity,
    pub fspec: Fspec,
}

pub enum Diff {
    Activity(Box<Activity>),
    Fspec(Fspec),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Activity(a) if self.activity == *a => false,
            Diff::Activity(a) => {
                self.activity = *a;
                true
            }
            Diff::Fspec(f) if self.fspec == f => false,
            Diff::Fspec(f) => {
                self.fspec = f;
                true
            }
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        let f = &self.fspec;
        let mut v = self.activity.to_json();
        v["fspec"] = json!({
            "installed": f.installed,
            "watching": f.watching,
            "dir": f.dir.as_ref().map(|d| d.display().to_string()),
            "error": f.error,
        });
        v
    }

    /// The IPC `status` reply.
    pub fn status(&self) -> String {
        self.to_json().to_string()
    }
}

pub enum Input {
    /// A line off an fspec socket.
    Op(Box<fs_activity::Item>),
    /// An fspec connection closed without a final state: the operation died.
    Died(String),
    Launcher { app: String, title: String, entry: Entry },
    LauncherGone(String),
    Notice(Notice),
    NoticeClosed(u32),
    /// The failures were looked at.
    Seen,
}

static INPUT: OnceLock<Sender<Input>> = OnceLock::new();

pub fn send(input: Input) {
    if let Some(tx) = INPUT.get() {
        let _ = tx.try_send(input);
    }
}

/// A notification arrived or was replaced (`services/notifications.rs`).
pub fn notification(n: &fs_notifd::Notification) {
    let app = if n.desktop_entry.is_empty() { n.app_name.clone() } else { n.desktop_entry.clone() };
    send(Input::Notice(Notice {
        id: n.id,
        app,
        title: n.summary.clone(),
        body: n.body.clone(),
        value: n.value.map(|v| Fraction::from_percent(v.get())),
        urgent: n.urgency == fs_notifd::Urgency::Critical,
    }));
}

pub fn notification_closed(id: u32) {
    send(Input::NoticeClosed(id));
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = INPUT.set(tx.clone());
    ctx.spawn(fspec_stream(ctx.clone(), tx.clone()));
    ctx.spawn(launcher_entries(ctx.clone(), tx));
    model(ctx, rx).await;
}

enum Wake {
    Input(Input),
    Fold,
    Closed,
}

fn take(activity: &mut Activity, input: Input, now: i64) -> bool {
    match input {
        Input::Op(item) => activity.upsert(*item, now),
        Input::Died(id) => activity.end(&id, ItemState::Failed, Some("fspec stopped without a result".into()), now),
        Input::Launcher { app, title, entry } => activity.launcher(&app, &title, &entry, now),
        Input::LauncherGone(app) => activity.launcher_gone(&app, now),
        Input::Notice(n) => activity.notification(n, now),
        Input::NoticeClosed(id) => activity.notification_closed(id, now),
        Input::Seen => activity.seen(),
    }
}

async fn model(ctx: Ctx, rx: Receiver<Input>) {
    let mut activity = Activity::default();
    loop {
        let input = async { rx.recv().await.map_or(Wake::Closed, Wake::Input) };
        let wake = match activity.next_fold() {
            Some(at) => {
                let wait = Duration::from_millis((at - now_ms()).max(0) as u64);
                futures_lite::future::or(input, async {
                    Timer::after(wait).await;
                    Wake::Fold
                })
                .await
            }
            None => input.await,
        };
        let now = now_ms();
        let mut changed = match wake {
            Wake::Input(input) => take(&mut activity, input, now),
            Wake::Fold => false,
            Wake::Closed => return,
        };
        // A burst of lines is one publish.
        while let Ok(input) = rx.try_recv() {
            changed |= take(&mut activity, input, now);
        }
        changed |= activity.fold(now);
        if changed {
            ctx.publish(store::Diff::Activity(Diff::Activity(Box::new(activity.clone()))));
        }
    }
}

// ---- fspec ------------------------------------------------------------------

/// `$XDG_RUNTIME_DIR/fspec/activity`, or fspec's own fallback without one.
fn fspec_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d).join("fspec/activity"),
        None => std::env::temp_dir().join(format!("fspec-{}", unsafe { libc::getuid() })).join("activity"),
    }
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

fn is_socket(name: &std::ffi::OsStr) -> bool {
    Path::new(name).extension().is_some_and(|e| e == "sock")
}

async fn fspec_stream(ctx: Ctx, tx: Sender<Input>) {
    let dir = fspec_dir();
    let mut status = Fspec { installed: on_path("fspec"), dir: Some(dir.clone()), ..Fspec::default() };
    match watch_dir(&ctx, &dir, &tx).await {
        Ok(watch) => {
            status.watching = true;
            ctx.publish(store::Diff::Activity(Diff::Fspec(status.clone())));
            let err = watch.await;
            status.watching = false;
            status.error = Some(err.to_string());
        }
        Err(err) => status.error = Some(err.to_string()),
    }
    if let Some(err) = &status.error {
        eprintln!("activity: fspec stream in {}: {err}", dir.display());
    }
    ctx.publish(store::Diff::Activity(Diff::Fspec(status)));
}

/// Creates and watches the directory, connects to every socket already in
/// it, and resolves to the loop that follows each new one. The watch is in
/// place before the listing, so an operation starting between the two is
/// not missed.
#[cfg(target_os = "linux")]
async fn watch_dir(ctx: &Ctx, dir: &Path, tx: &Sender<Input>) -> std::io::Result<impl Future<Output = std::io::Error> + use<>> {
    use std::os::fd::AsFd;
    use std::os::unix::fs::DirBuilderExt;

    use inotify::{Inotify, WatchMask};

    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let mut inotify = Inotify::init()?;
    inotify.watches().add(dir, WatchMask::CREATE | WatchMask::MOVED_TO)?;
    let ready = Async::new(inotify.as_fd().try_clone_to_owned()?)?;
    let followed = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
    for entry in std::fs::read_dir(dir)?.flatten() {
        if is_socket(&entry.file_name()) {
            follow(ctx, entry.path(), tx, &followed);
        }
    }
    let (ctx, dir, tx) = (ctx.clone(), dir.to_path_buf(), tx.clone());
    Ok(async move {
        let mut buffer = [0u8; 4096];
        loop {
            match inotify.read_events(&mut buffer) {
                Ok(events) => {
                    for name in events.filter_map(|e| e.name) {
                        if is_socket(name) {
                            follow(&ctx, dir.join(name), &tx, &followed);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Err(e) = ready.readable().await {
                        return e;
                    }
                }
                Err(e) => return e,
            }
        }
    })
}

#[cfg(not(target_os = "linux"))]
async fn watch_dir(_: &Ctx, _: &Path, _: &Sender<Input>) -> std::io::Result<std::future::Pending<std::io::Error>> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no inotify on this system"))
}

/// One reader per socket path at a time.
fn follow(ctx: &Ctx, path: PathBuf, tx: &Sender<Input>, followed: &std::rc::Rc<std::cell::RefCell<HashSet<PathBuf>>>) {
    if !followed.borrow_mut().insert(path.clone()) {
        return;
    }
    let (tx, followed) = (tx.clone(), followed.clone());
    ctx.spawn(async move {
        read_operation(&path, &tx).await;
        followed.borrow_mut().remove(&path);
    });
}

/// A refused connect is an operation that died without cleaning up, or
/// one caught between its `bind` and its `listen`, which inotify can see:
/// it is asked once more after a moment.
async fn connect(path: &Path) -> Option<Async<UnixStream>> {
    match Async::<UnixStream>::connect(path).await {
        Ok(stream) => Some(stream),
        Err(_) => {
            Timer::after(LISTEN_GAP).await;
            Async::<UnixStream>::connect(path).await.ok()
        }
    }
}

/// Reads one operation until it ends. A connection cut short (a reader
/// that fell behind) is reconnected while the socket still exists; one
/// that ends with no final state and no socket to go back to is a death.
async fn read_operation(path: &Path, tx: &Sender<Input>) {
    let mut last: Option<String> = None;
    let mut ended = false;
    let mut empty = 0;
    loop {
        let Some(mut stream) = connect(path).await else { break };
        let mut lines = 0;
        let mut pending: Vec<u8> = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            pending.extend_from_slice(&chunk[..n]);
            while let Some(at) = pending.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = pending.drain(..=at).collect();
                let Some(op) = std::str::from_utf8(&line[..at]).ok().and_then(fspec::parse) else { continue };
                lines += 1;
                ended = op.state() != ItemState::Running;
                last = Some(fspec::item_id(&op.id));
                let _ = tx.send(Input::Op(Box::new(op.item()))).await;
            }
            if pending.len() > MAX_LINE {
                break;
            }
        }
        // What is left in `pending` is a line the writer never finished.
        if ended {
            return;
        }
        empty = if lines == 0 { empty + 1 } else { 0 };
        if empty >= EMPTY_RECONNECTS || !path.exists() {
            break;
        }
        Timer::after(RECONNECT_GAP).await;
    }
    if let (Some(id), false) = (last, ended) {
        let _ = tx.send(Input::Died(id)).await;
    }
}

// ---- com.canonical.Unity.LauncherEntry ---------------------------------------

fn prop(name: &str, v: &OwnedValue) -> Option<Prop> {
    let v: &Value = v;
    Some(match (name, v) {
        ("count", Value::I64(n)) => Prop::Count(*n),
        ("count", Value::I32(n)) => Prop::Count(i64::from(*n)),
        ("count", Value::U32(n)) => Prop::Count(i64::from(*n)),
        ("count-visible", Value::Bool(b)) => Prop::CountVisible(*b),
        ("progress", Value::F64(p)) => Prop::Progress(*p),
        ("progress-visible", Value::Bool(b)) => Prop::ProgressVisible(*b),
        ("urgent", Value::Bool(b)) => Prop::Urgent(*b),
        _ => return None,
    })
}

enum Bus {
    Update(zbus::Message),
    Gone(String),
}

async fn launcher_entries(ctx: Ctx, tx: Sender<Input>) {
    if let Err(err) = listen_launcher(&ctx, &tx).await {
        eprintln!("activity: {LAUNCHER_ENTRY}: {err}");
    }
}

async fn listen_launcher(ctx: &Ctx, tx: &Sender<Input>) -> zbus::Result<()> {
    let conn = zbus::Connection::session().await?;
    let rule = MatchRule::builder().msg_type(Type::Signal).interface(LAUNCHER_ENTRY)?.member("Update")?.build();
    let updates = MessageStream::for_match_rule(rule, &conn, None).await?.filter_map(|m| m.ok().map(Bus::Update));
    let owners = fdo::DBusProxy::new(&conn).await?.receive_name_owner_changed().await?.filter_map(|s| {
        let args = s.args().ok()?;
        args.new_owner().is_none().then(|| Bus::Gone(args.name().to_string()))
    });
    let mut events = updates.or(owners);
    let mut entries: HashMap<String, (String, Entry)> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    while let Some(event) = events.next().await {
        match event {
            Bus::Update(message) => {
                let sender = message.header().sender().map(|s| s.to_string()).unwrap_or_default();
                let Ok((uri, props)) = message.body().deserialize::<(String, HashMap<String, OwnedValue>)>() else { continue };
                let app = launcher::desktop_id(&uri).to_owned();
                if app.is_empty() {
                    continue;
                }
                if !names.contains_key(&app) {
                    let id = app.clone();
                    let name = ctx
                        .pool()
                        .run(move || scan_launchable(&applications_dirs()).into_iter().find(|e| e.id == id).map(|e| e.name))
                        .await
                        .flatten();
                    names.insert(app.clone(), name.unwrap_or_else(|| app.clone()));
                }
                let (owner, entry) = entries.entry(app.clone()).or_default();
                *owner = sender;
                for (name, value) in &props {
                    if let Some(p) = prop(name, value) {
                        entry.apply(p);
                    }
                }
                let title = names[&app].clone();
                let _ = tx.send(Input::Launcher { app, title, entry: entry.clone() }).await;
            }
            Bus::Gone(name) => {
                let gone: Vec<String> = entries.iter().filter(|(_, (owner, _))| *owner == name).map(|(app, _)| app.clone()).collect();
                for app in gone {
                    entries.remove(&app);
                    let _ = tx.send(Input::LauncherGone(app)).await;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    use futures_lite::future::block_on;

    use super::*;

    fn line(id: &str, state: &str, done: u64) -> String {
        format!(
            "{{\"v\":1,\"type\":\"update\",\"op\":{{\"id\":\"{id}\",\"kind\":\"apply\",\"source\":\"fspec:apply:rig\",\"title\":\"Applying rig\",\"state\":\"{state}\",\"progress\":{{\"done\":{done},\"total\":4,\"unit\":\"resources\"}}}}}}\n"
        )
    }

    fn drain(rx: &Receiver<Input>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(input) = rx.try_recv() {
            out.push(match input {
                Input::Op(item) => format!("{} {}", item.state.as_str(), item.progress.fraction().unwrap().as_percent()),
                Input::Died(id) => format!("died {id}"),
                _ => "other".into(),
            });
        }
        out
    }

    /// A served operation read to its final line, with a line cut short in
    /// the middle of the stream dropped rather than misread.
    #[test]
    fn reads_an_operation_to_its_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(line("a", "running", 1).as_bytes()).unwrap();
            s.write_all(b"{\"v\":1,\"type\":\"upd\n").unwrap();
            s.write_all(line("a", "succeeded", 4).as_bytes()).unwrap();
        });
        block_on(read_operation(&path, &tx));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "succeeded 100"]);
    }

    /// A connection that closes with no final state and no socket left is a
    /// death; a truncated last line is not read.
    #[test]
    fn a_close_without_a_result_is_a_death() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let gone = path.clone();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(line("b", "running", 2).as_bytes()).unwrap();
            s.write_all(line("b", "succeeded", 4).trim_end().as_bytes()).unwrap();
            std::fs::remove_file(&gone).unwrap();
        });
        block_on(read_operation(&path, &tx));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 50", "died fspec:b"]);
    }

    /// A reader cut off mid-line reconnects while the socket is there and
    /// reads the snapshot again.
    #[test]
    fn a_cut_reader_reconnects() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(line("c", "running", 1).as_bytes()).unwrap();
            s.write_all(b"{\"v\":1,\"ty").unwrap();
            drop(s);
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(line("c", "running", 3).as_bytes()).unwrap();
            s.write_all(line("c", "failed", 3).as_bytes()).unwrap();
        });
        block_on(read_operation(&path, &tx));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "running 75", "failed 75"]);
    }

    #[test]
    fn a_refused_socket_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        drop(UnixListener::bind(&path).unwrap());
        let (tx, rx) = async_channel::unbounded();
        block_on(read_operation(&path, &tx));
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn launcher_props_take_the_wire_types() {
        let v = |x: Value<'static>| OwnedValue::try_from(x).unwrap();
        assert!(matches!(prop("count", &v(Value::I64(4))), Some(Prop::Count(4))));
        assert!(matches!(prop("progress", &v(Value::F64(0.5))), Some(Prop::Progress(p)) if p == 0.5));
        assert!(matches!(prop("urgent", &v(Value::Bool(true))), Some(Prop::Urgent(true))));
        assert!(prop("progress", &v(Value::Bool(true))).is_none());
        assert!(prop("quicklist", &v(Value::from("/x"))).is_none());
    }

    #[test]
    fn status_carries_fspec_and_the_groups() {
        let s = State { fspec: Fspec { installed: false, dir: Some("/run/x".into()), watching: true, error: None }, ..State::default() };
        let v: serde_json::Value = serde_json::from_str(&s.status()).unwrap();
        assert_eq!(v["fspec"], json!({"installed": false, "watching": true, "dir": "/run/x", "error": null}));
        assert_eq!(v["groups"], json!([]));
        assert_eq!(v["history"], json!([]));
    }
}
