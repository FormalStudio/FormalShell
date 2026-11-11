//! Activity: fspec's operation stream, `com.canonical.Unity.LauncherEntry`
//! on the session bus, the notification `value` hint (fed by
//! `services/notifications.rs`) and the shell's own launches (fed by
//! `services/launch.rs`), folded into one [`fs_activity::Activity`] on the
//! service thread and published whole when it changes.
//!
//! Nothing here polls. fspec operations are found by inotify on their
//! directory and read off their sockets; LauncherEntry is a match rule; the
//! only timer is the next fold, armed while a finished item waits for it or
//! a launch for its window.

use std::collections::{HashMap, HashSet};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use async_io::{Async, Timer};
use fs_activity::launcher::{self, Entry};
use fs_activity::{Activity, FspecSource, Notice, Origin, Retry, State as ItemState, fspec};
use fs_dbus::{Owner, OwnerWatch};
use fs_types::NotificationId;
use futures_lite::{AsyncReadExt, StreamExt};
use serde_json::json;
use zbus::message::Type;
use zbus::{MatchRule, MessageStream};

use crate::runtime::{Ctx, Runtime, now_ms};
use crate::services::appicon::{applications_dirs, scan_launchable};
use crate::services::notifications::{self, Op};
use crate::services::proc::on_path;
use crate::services::{compositor, launch};
use crate::store::{self, Store};

const LAUNCHER_ENTRY: &str = "com.canonical.Unity.LauncherEntry";
/// A line longer than this is not fspec's: the connection is dropped.
const MAX_LINE: usize = 1 << 20;
/// Connections in a row that end before a single line, after which a
/// socket that still exists is taken for dead.
const EMPTY_RECONNECTS: u32 = 3;
const RECONNECT_GAP: Duration = Duration::from_millis(100);
/// Silence on a running operation's connection past this asks fspec
/// again on a second connection, kept only once it answers; the first stays
/// open until then, so a final line written to it meanwhile is still read.
/// It recovers a connection fspec stopped writing to while holding it open
/// (one whose snapshot it could not send whole), the cause suspected and
/// not confirmed for a run read stuck at its first line.
const QUIET: Duration = Duration::from_secs(3);
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

}

pub enum Input {
    /// A line off an fspec socket.
    Op(Box<fs_activity::Item>),
    /// An fspec connection closed without a final state: the operation died.
    Died(String),
    Launcher { app: String, title: String, entry: Entry },
    LauncherGone(String),
    Notice(Notice),
    NoticeClosed(NotificationId),
    /// The shell spawned desktop entry `app` (`services/launch.rs`).
    Launch { app: String, title: String },
    /// A window of `app` mapped.
    Launched(String),
    /// The failures were looked at.
    Seen,
    /// A group put away (`Activity::dismiss`).
    Dismiss(String),
    /// The centre's `Clear` on folded activity.
    ClearHistory,
}

static INPUT: OnceLock<Sender<Input>> = OnceLock::new();

pub fn send(input: Input) {
    if let Some(tx) = INPUT.get() {
        let _ = tx.try_send(input);
    }
}

/// A notification arrived or was replaced (`services/notifications.rs`).
pub fn notification(notice: Notice) {
    send(Input::Notice(notice));
}

pub fn notification_closed(id: NotificationId) {
    send(Input::NoticeClosed(id));
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
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
        Input::Launch { app, title } => activity.launch(&app, &title, now),
        Input::Launched(app) => activity.launched(&app),
        Input::Seen => activity.seen(),
        Input::Dismiss(source) => activity.dismiss(&source),
        Input::ClearHistory => activity.clear_history(),
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

/// `$XDG_RUNTIME_DIR/fspec/activity`, or fspec's own fallback without one.
fn fspec_dir() -> PathBuf {
    match crate::paths::session_runtime() {
        Some(d) => d.join("fspec/activity"),
        None => std::env::temp_dir().join(format!("fspec-{}", unsafe { libc::getuid() })).join("activity"),
    }
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
        read_operation(&path, &tx, QUIET).await;
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

/// What a reader has passed on of one operation.
#[derive(Default)]
struct Seen {
    last: Option<String>,
    ended: bool,
}

enum Got {
    Lines(usize),
    Quiet,
    Closed,
}

struct Conn {
    stream: Async<UnixStream>,
    pending: Vec<u8>,
}

impl Conn {
    /// Reads one chunk, or nothing for `quiet`, sending every line it completes.
    async fn step(&mut self, seen: &mut Seen, tx: &Sender<Input>, quiet: Duration) -> Got {
        let mut chunk = [0u8; 8192];
        let read = async { Some(self.stream.read(&mut chunk).await) };
        let n = match futures_lite::future::or(read, async {
            Timer::after(quiet).await;
            None
        })
        .await
        {
            None => return Got::Quiet,
            Some(Ok(0) | Err(_)) => return Got::Closed,
            Some(Ok(n)) => n,
        };
        self.pending.extend_from_slice(&chunk[..n]);
        let mut lines = 0;
        while let Some(at) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=at).collect();
            let Some(op) = std::str::from_utf8(&line[..at]).ok().and_then(fspec::parse) else { continue };
            lines += 1;
            seen.ended = op.state() != ItemState::Running;
            seen.last = Some(fspec::item_id(&op.id));
            let _ = tx.send(Input::Op(Box::new(op.item()))).await;
        }
        // What is left in `pending` is a line the writer never finished.
        if self.pending.len() > MAX_LINE { Got::Closed } else { Got::Lines(lines) }
    }
}

/// A second connection that has answered with at least one line.
async fn ask_again(path: &Path, seen: &mut Seen, tx: &Sender<Input>, quiet: Duration) -> Option<(Conn, usize)> {
    let stream = Async::<UnixStream>::connect(path).await.ok()?;
    let mut fresh = Conn { stream, pending: Vec::new() };
    // A snapshot can arrive in pieces: read on until a whole line lands.
    loop {
        match fresh.step(seen, tx, quiet).await {
            Got::Lines(0) => {}
            Got::Lines(n) => return Some((fresh, n)),
            Got::Quiet | Got::Closed => return None,
        }
    }
}

/// Reads one operation until it ends. A connection cut short (a reader
/// that fell behind) is reconnected while the socket still exists; one
/// that ends with no final state and no socket to go back to is a death.
async fn read_operation(path: &Path, tx: &Sender<Input>, quiet: Duration) {
    let mut seen = Seen::default();
    let mut empty = 0;
    loop {
        let Some(stream) = connect(path).await else { break };
        let mut conn = Conn { stream, pending: Vec::new() };
        let mut lines = 0;
        loop {
            match conn.step(&mut seen, tx, quiet).await {
                Got::Lines(n) => lines += n,
                Got::Closed => break,
                Got::Quiet => {
                    if let Some((fresh, n)) = ask_again(path, &mut seen, tx, quiet).await {
                        (conn, lines) = (fresh, n);
                    }
                }
            }
            if seen.ended {
                return;
            }
        }
        empty = if lines == 0 { empty + 1 } else { 0 };
        if empty >= EMPTY_RECONNECTS || !path.exists() {
            break;
        }
        Timer::after(RECONNECT_GAP).await;
    }
    if let (Some(id), false) = (seen.last, seen.ended) {
        let _ = tx.send(Input::Died(id)).await;
    }
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
    let conn = crate::services::bus::session().await?;
    let rule = MatchRule::builder().msg_type(Type::Signal).interface(LAUNCHER_ENTRY)?.member("Update")?.build();
    let updates = MessageStream::for_match_rule(rule, &conn, None).await?.filter_map(|m| m.ok().map(Bus::Update));
    let owners = OwnerWatch::any(&conn).await?.filter_map(|change| (change.owner == Owner::Gone).then(|| Bus::Gone(change.name)));
    let mut events = updates.or(owners);
    let mut entries: HashMap<String, (String, Entry)> = HashMap::new();
    let mut names: HashMap<String, String> = HashMap::new();
    while let Some(event) = events.next().await {
        match event {
            Bus::Update(message) => {
                let sender = message.header().sender().map(|s| s.to_string()).unwrap_or_default();
                let Ok((uri, props)) = message.body().deserialize::<(String, fs_dbus::Props)>() else { continue };
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
                entry.apply_update(&props);
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

/// A row's verbs, carried in its widgets' `on` as `<verb>:<source>`, and
/// what the IPC verbs name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Open,
    Dismiss,
    Retry,
    /// The app an install brought.
    OpenApp,
}

impl Verb {
    const ALL: [Verb; 4] = [Verb::Open, Verb::Dismiss, Verb::Retry, Verb::OpenApp];

    fn word(self) -> &'static str {
        match self {
            Verb::Open => "open",
            Verb::Dismiss => "dismiss",
            Verb::Retry => "retry",
            Verb::OpenApp => "open-app",
        }
    }

    /// The `on` a widget carries for this verb on group `source`.
    pub fn on(self, source: &str) -> String {
        format!("{}:{source}", self.word())
    }

    pub fn parse(on: &str) -> Option<(Verb, &str)> {
        let (word, source) = on.split_once(':')?;
        Verb::ALL.into_iter().find(|v| v.word() == word).map(|verb| (verb, source))
    }
}

/// What the caller still has to do itself.
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    Nothing,
    /// An fspec row opens the Activity panel.
    Panel,
}

fn notify(runtime: &Runtime, op: Op) {
    runtime.service(move |ctx| ctx.publish(store::Diff::Notifications(notifications::Diff::Op(op))));
}

/// The window of desktop entry `app`, else the entry launched.
fn focus_app(store: &Store, app: &str) {
    let id = app.trim_end_matches(".desktop");
    let window = store.compositor.snapshot.windows.iter().find(|w| w.app_id.eq_ignore_ascii_case(id) || w.initial_class.eq_ignore_ascii_case(id));
    match window {
        Some(win) => compositor::focus_window(&win.id),
        None => {
            if let Some(argv) = launch::app_argv(&store.menu, id) {
                launch::spawn(store, id, &argv);
            }
        }
    }
}

fn spawn(retry: Retry) {
    match retry.cwd {
        Some(dir) => compositor::spawn_in(&dir, &retry.command.argv()),
        None => compositor::spawn(&retry.command.argv()),
    }
}

/// Runs `verb` on the live group `source`. The error is the IPC reply.
pub fn act(store: &Store, runtime: &Runtime, verb: Verb, source: &str) -> Result<Done, String> {
    let activity = &store.activity.activity;
    let groups = activity.groups();
    let Some(g) = groups.iter().find(|g| g.source == source) else { return Err(format!("no activity from '{source}'")) };
    match verb {
        Verb::Open => {
            let lead = g.lead();
            match (lead.origin, lead.app.as_deref(), lead.notification()) {
                (Origin::Fspec | Origin::Atoll, ..) => return Ok(Done::Panel),
                (Origin::LauncherEntry | Origin::Launch, Some(app), _) => focus_app(store, app),
                (Origin::Notification, _, Some(id)) => notify(runtime, Op::Invoke(id, "default".into())),
                _ => {}
            }
        }
        Verb::Dismiss => {
            let ids: Vec<_> = g.items.iter().filter_map(|i| i.notification()).collect();
            if !ids.is_empty() {
                notify(runtime, Op::DismissGroup(ids));
            }
            send(Input::Dismiss(source.to_owned()));
        }
        Verb::OpenApp => {
            let Some(app) = super::packages::landed_app(store, source) else { return Err(format!("'{source}' brought no app")) };
            focus_app(store, app);
            send(Input::Dismiss(source.to_owned()));
        }
        Verb::Retry => {
            let Some(retry) = activity.retry(source) else { return Err(format!("'{source}' cannot be retried")) };
            // The packages service follows its own installs' operations.
            match FspecSource::parse(source) {
                Some(FspecSource { kind: "install", arg: Some(id) }) => super::packages::install(store, id),
                Some(FspecSource { kind: "uninstall", arg: Some(id) }) => {
                    super::packages::send(super::packages::Cmd::Uninstall(id.to_owned()));
                }
                _ => spawn(retry),
            }
            send(Input::Dismiss(source.to_owned()));
        }
    }
    Ok(Done::Nothing)
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

    /// Short enough to keep the silent-connection test quick, and ten times
    /// the pause inside a split snapshot, which must not read as silence.
    const TEST_QUIET: Duration = Duration::from_millis(300);

    fn drain(rx: &Receiver<Input>) -> Vec<String> {
        let mut out = Vec::new();
        while let Ok(input) = rx.try_recv() {
            out.push(match input {
                Input::Op(item) => format!("{} {}", item.state.as_str(), item.progress.fraction().unwrap().to_percent().rounded()),
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
        block_on(read_operation(&path, &tx, TEST_QUIET));
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
        block_on(read_operation(&path, &tx, TEST_QUIET));
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
        block_on(read_operation(&path, &tx, TEST_QUIET));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "running 75", "failed 75"]);
    }

    /// A connection fspec stopped writing to while holding it open: past
    /// `QUIET` the reader connects again and reads the end.
    #[test]
    fn a_silent_connection_is_asked_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let server = std::thread::spawn(move || {
            let (mut stranded, _) = listener.accept().unwrap();
            stranded.write_all(line("e", "running", 1).as_bytes()).unwrap();
            let (mut s, _) = listener.accept().unwrap();
            s.write_all(line("e", "running", 4).as_bytes()).unwrap();
            s.write_all(line("e", "succeeded", 4).as_bytes()).unwrap();
            drop(stranded);
        });
        block_on(read_operation(&path, &tx, TEST_QUIET));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "running 100", "succeeded 100"]);
    }

    /// The operation ending while the reader asks again: the final line
    /// lands on the first connection and the socket is gone, so the second
    /// never answers, and the first is still read to its end.
    #[test]
    fn an_end_during_the_second_ask_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let gone = path.clone();
        let server = std::thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            first.write_all(line("f", "running", 1).as_bytes()).unwrap();
            let (second, _) = listener.accept().unwrap();
            first.write_all(line("f", "succeeded", 4).as_bytes()).unwrap();
            std::fs::remove_file(&gone).unwrap();
            drop(second);
            drop(listener);
            drop(first);
        });
        block_on(read_operation(&path, &tx, TEST_QUIET));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "succeeded 100"]);
    }

    /// A second connection whose snapshot arrives in two writes is still
    /// the one kept.
    #[test]
    fn a_split_snapshot_on_the_second_ask_is_read() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("g.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = async_channel::unbounded();
        let server = std::thread::spawn(move || {
            let (mut stranded, _) = listener.accept().unwrap();
            stranded.write_all(line("g", "running", 1).as_bytes()).unwrap();
            let (mut s, _) = listener.accept().unwrap();
            let snapshot = line("g", "running", 3);
            let (head, tail) = snapshot.split_at(snapshot.len() / 2);
            s.write_all(head.as_bytes()).unwrap();
            std::thread::sleep(TEST_QUIET / 10);
            s.write_all(tail.as_bytes()).unwrap();
            s.write_all(line("g", "succeeded", 4).as_bytes()).unwrap();
            drop(stranded);
        });
        block_on(read_operation(&path, &tx, TEST_QUIET));
        server.join().unwrap();
        assert_eq!(drain(&rx), ["running 25", "running 75", "succeeded 100"]);
    }

    #[test]
    fn a_refused_socket_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        drop(UnixListener::bind(&path).unwrap());
        let (tx, rx) = async_channel::unbounded();
        block_on(read_operation(&path, &tx, TEST_QUIET));
        assert!(drain(&rx).is_empty());
    }

    #[test]
    fn status_carries_fspec_and_the_groups() {
        let s = State { fspec: Fspec { installed: false, dir: Some("/run/x".into()), watching: true, error: None }, ..State::default() };
        let v: serde_json::Value = s.to_json();
        assert_eq!(v["fspec"], json!({"installed": false, "watching": true, "dir": "/run/x", "error": null}));
        assert_eq!(v["groups"], json!([]));
        assert_eq!(v["history"], json!([]));
    }

    #[test]
    fn every_verb_prints_and_parses_back() {
        let source = "fspec:install:org.mozilla.firefox";
        for (verb, word) in [(Verb::Open, "open"), (Verb::Dismiss, "dismiss"), (Verb::Retry, "retry"), (Verb::OpenApp, "open-app")] {
            let on = format!("{word}:{source}");
            assert_eq!(verb.on(source), on);
            assert_eq!(Verb::parse(&on), Some((verb, source)));
        }
        assert_eq!(Verb::parse("close"), None);
        assert_eq!(Verb::parse("action:k"), None);
    }
}
