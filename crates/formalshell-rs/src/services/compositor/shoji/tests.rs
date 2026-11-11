//! The backend against a fake engine: a socket speaking section 3 off
//! `fixture.json`, which records every line it reads, broadcasts on
//! demand, and goes away and comes back like a compositor restarting.

use std::io::{BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use calloop::EventLoop;
use calloop::channel::{self, Event};
use fs_types::WindowId;
use serde_json::{Value, json};

use super::*;
use crate::runtime::{Msg, Publisher, Runtime};

const FIXTURE: &str = include_str!("fixture.json");

#[derive(Default)]
struct Inner {
    received: Vec<Value>,
    conns: Vec<UnixStream>,
    state: Value,
}

/// `slam` takes each connection and closes it at once, answering nothing.
struct Fake {
    path: PathBuf,
    inner: Arc<Mutex<Inner>>,
    accepted: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
    slam: bool,
}

impl Fake {
    fn start(path: &Path, slam: bool) -> Self {
        let inner = Inner { state: serde_json::from_str(FIXTURE).unwrap(), ..Inner::default() };
        let mut fake = Fake {
            path: path.to_owned(),
            inner: Arc::new(Mutex::new(inner)),
            accepted: Arc::default(),
            stop: Arc::default(),
            accept: None,
            slam,
        };
        fake.listen();
        fake
    }

    fn listen(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let listener = UnixListener::bind(&self.path).unwrap();
        listener.set_nonblocking(true).unwrap();
        self.stop.store(false, Ordering::SeqCst);
        let (inner, stop, accepted, slam) = (self.inner.clone(), self.stop.clone(), self.accepted.clone(), self.slam);
        self.accept = Some(std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let Ok((stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                accepted.fetch_add(1, Ordering::SeqCst);
                if slam {
                    continue;
                }
                stream.set_nonblocking(false).unwrap();
                inner.lock().unwrap().conns.push(stream.try_clone().unwrap());
                let inner = inner.clone();
                std::thread::spawn(move || answer(stream, inner));
            }
        }));
    }

    /// The compositor gone: every connection shut and the socket removed.
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.accept.take() {
            t.join().unwrap();
        }
        for c in self.inner.lock().unwrap().conns.drain(..) {
            let _ = c.shutdown(Shutdown::Both);
        }
        let _ = std::fs::remove_file(&self.path);
    }

    fn broadcast(&self, event: &str, payload: Value) {
        let line = format!("{}\n", json!({"event": event, "payload": payload}));
        for mut c in &self.inner.lock().unwrap().conns {
            let _ = c.write_all(line.as_bytes());
        }
    }

    fn received(&self) -> Vec<Value> {
        self.inner.lock().unwrap().received.clone()
    }

    fn methods(&self) -> Vec<String> {
        self.received().iter().map(|v| v["method"].as_str().unwrap_or_default().to_owned()).collect()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop();
    }
}

fn answer(stream: UnixStream, inner: Arc<Mutex<Inner>>) {
    let mut out = stream.try_clone().unwrap();
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { return };
        let request: Value = serde_json::from_str(&line).unwrap();
        let result = match request["method"].as_str().unwrap_or_default() {
            "state.get" => inner.lock().unwrap().state.clone(),
            "outputs.get" => json!([{"name": "Virtual-1", "enabled": true, "width": 2400, "height": 1350, "scale": 1.25, "refresh": 60,
                "color": {"cm": "srgb", "bitdepth": 8, "sdrBrightness": 1, "sdrSaturation": 1}}]),
            "binds.list" => json!([{"chord": "SUPER+Return", "mods": ["SUPER"], "key": "Return", "action": "spawn", "args": ["foot"], "title": "Terminal"}]),
            "cursor.pos" => json!({"x": 12.5, "y": 40}),
            _ => Value::Null,
        };
        inner.lock().unwrap().received.push(request.clone());
        if let Some(id) = request.get("id")
            && out.write_all(format!("{}\n", json!({"id": id, "result": result})).as_bytes()).is_err()
        {
            return;
        }
    }
}

#[derive(Default)]
struct Seen {
    snapshots: Vec<Snapshot>,
    outputs: Vec<(usize, OutputsState)>,
    keyboards: Vec<Layout>,
    reloads: usize,
    blur: Vec<bool>,
}

struct Rig {
    event_loop: EventLoop<'static, Seen>,
    seen: Seen,
    commands: async_channel::Sender<Command>,
    _runtime: Runtime,
}

impl Rig {
    fn start(path: &Path) -> Self {
        let event_loop: EventLoop<Seen> = EventLoop::try_new().unwrap();
        let (sender, inbox) = channel::channel::<Msg>();
        event_loop
            .handle()
            .insert_source(inbox, |event, _, seen: &mut Seen| match event {
                Event::Msg(Msg::Diff(store::Diff::Compositor(diff))) => match diff {
                    Diff::Snapshot(s) => seen.snapshots.push(s),
                    Diff::Outputs(rows, state) => seen.outputs.push((rows.len(), state)),
                    Diff::Keyboard(k) => seen.keyboards.push(k),
                    Diff::ConfigReloaded => seen.reloads += 1,
                    _ => {}
                },
                Event::Msg(Msg::Diff(store::Diff::Theme(crate::services::theme::Diff::Blur(b)))) => seen.blur.push(b),
                _ => {}
            })
            .unwrap();
        let (commands, rx) = async_channel::unbounded();
        let path = path.to_owned();
        let runtime = Runtime::start(Publisher::new(sender), move |ctx| {
            ctx.spawn(serve(ctx.clone(), rx, path));
        });
        Rig { event_loop, seen: Seen::default(), commands, _runtime: runtime }
    }

    /// Dispatches until `done` holds; fails the test past five seconds.
    fn until(&mut self, what: &str, mut done: impl FnMut(&Seen) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done(&self.seen) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            self.event_loop.dispatch(Some(Duration::from_millis(10)), &mut self.seen).unwrap();
        }
    }

    fn send(&self, command: Command) {
        self.commands.send_blocking(command).unwrap();
    }

    fn last(&self) -> &Snapshot {
        self.seen.snapshots.last().expect("a snapshot")
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn socket(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("shojiwm-wayland-9.sock")
}

#[test]
fn follows_the_state_sends_commands_in_order_and_recovers_from_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = socket(&dir);
    let mut fake = Fake::start(&path, false);
    let mut rig = Rig::start(&path);

    rig.until("the first state", |s| s.snapshots.last().is_some_and(|s| s.available));
    assert_eq!(rig.last().focused_window_id, "tl-b");
    rig.until("the outputs", |s| !s.outputs.is_empty());
    assert_eq!(rig.seen.outputs[0], (1, OutputsState::Ok));
    assert_eq!(rig.seen.keyboards.last().unwrap().current, "Spanish");
    assert_eq!(rig.seen.blur, [true]);

    rig.send(Command::Theme { colors: serde_json::Map::from_iter([("primary".into(), json!("#648db8"))]), chrome: json!({"rounding": 6}) });
    rig.send(Command::Zen { id: WindowId::new("tl-a"), rect: fs_types::IRect::new(0, 0, 10, 10) });
    rig.send(Command::FocusWindow(WindowId::new("tl-a")));
    rig.send(Command::SetOutputScale("Virtual-1".into(), 1.5));
    wait_for("the writes", || fake.methods().iter().filter(|m| *m == "outputs.get").count() >= 2);
    let methods = fake.methods();
    let tail: Vec<&str> = methods.iter().map(String::as_str).skip_while(|m| *m != "theme.set").collect();
    assert_eq!(tail, ["theme.set", "window.focus", "outputs.set", "outputs.get"], "the shell's own zen sends nothing");
    let received = fake.received();
    let theme = received.iter().find(|v| v["method"] == "theme.set").unwrap();
    assert_eq!(theme["params"], json!({"colors": {"primary": "#648db8"}, "chrome": {"rounding": 6}}));
    assert!(theme.get("id").is_none(), "a command carries no id");
    assert_eq!(received.iter().find(|v| v["method"] == "outputs.set").unwrap()["params"], json!({"name": "Virtual-1", "scale": 1.5}));

    let mut moved: Value = serde_json::from_str(FIXTURE).unwrap();
    moved["focusedWindowId"] = json!("tl-a");
    fake.broadcast("state.changed", moved.clone());
    rig.until("the broadcast state", |s| s.snapshots.last().is_some_and(|s| s.focused_window_id == "tl-a"));
    let published = rig.seen.snapshots.len();
    fake.broadcast("state.changed", moved);
    fake.broadcast("config.reloaded", Value::Null);
    rig.until("the reload", |s| s.reloads == 1);
    assert_eq!(rig.seen.snapshots.len(), published, "an unchanged state is not published again");

    fake.stop();
    rig.until("the compositor unavailable", |s| s.snapshots.last().is_some_and(|s| !s.available));
    fake.listen();
    rig.until("the state after the restart", |s| s.snapshots.last().is_some_and(|s| s.available));
    wait_for("the theme sent again", || fake.methods().iter().filter(|m| *m == "theme.set").count() == 2);
}

#[test]
fn a_socket_that_closes_at_once_is_retried_with_a_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let path = socket(&dir);
    let fake = Fake::start(&path, true);
    let mut rig = Rig::start(&path);
    let started = Instant::now();
    while started.elapsed() < Duration::from_millis(1200) {
        rig.event_loop.dispatch(Some(Duration::from_millis(20)), &mut rig.seen).unwrap();
    }
    // 250, 500 then 1000 ms apart: four at most in 1.2 s, not hundreds.
    let accepted = fake.accepted.load(Ordering::SeqCst);
    assert!((2..=4).contains(&accepted), "{accepted} connections in 1.2 s");
    assert!(rig.seen.snapshots.iter().all(|s| !s.available));
}

#[test]
fn asks_on_a_connection_of_their_own() {
    let dir = tempfile::tempdir().unwrap();
    let path = socket(&dir);
    let _fake = Fake::start(&path, false);
    let answer = async_io::block_on(ask(&path, Method::BindsList)).unwrap();
    let binds = model::binds(serde_json::from_value(answer).unwrap());
    assert_eq!((binds[0].chord.as_str(), binds[0].mods.as_slice(), binds[0].args.as_slice()), ("SUPER+Return", ["SUPER"].as_slice(), ["foot".to_owned()].as_slice()));
    let point: proto::Point = serde_json::from_value(async_io::block_on(ask(&path, Method::CursorPos)).unwrap()).unwrap();
    assert_eq!((point.x, point.y), (12.5, 40.0));
    let missing = dir.path().join("none.sock");
    assert!(async_io::block_on(ask(&missing, Method::CursorPos)).is_err());
}
