//! The session lock: ext-session-lock-v1 with
//! one lock surface per output, the password typed into it checked by PAM
//! off the UI thread, and the only unlock being PAM's own success.
//!
//! Fail closed: nothing here unlocks on an error. A shell that dies while
//! locked leaves the compositor holding the session locked, which is the
//! protocol's contract, and a lock this process drops without unlocking is
//! a protocol error that does the same.
//!
//! A shell started while its predecessor's lock is still held (a restart, an
//! update, a crash) takes the lock back at once, so the compositor's
//! dead-locker screen is never what the user is left at. Hyprland only lets
//! a second client lock a session whose locker died under
//! `misc.allow_session_lock_restore`.
//!
//! The boot's lock (docs/superpowers/specs/2026-10-30-locked-login.md):
//! the login script leaves `$XDG_RUNTIME_DIR/formalshell/login-lock`, its
//! content the splash's last picture or empty, and the shell raises the lock
//! off it before anything else, that picture alone on its first frame. The
//! first unlock deletes it; until then the card offers "Other user".
//!
//! The screensaver can show inside the lock: its scene is presented on the
//! lock surfaces in place of the card, input dismisses it back to the card,
//! and a lock raised over it brings it back on idle rather than blanking.

use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use calloop::RegistrationToken;
use calloop::channel::{self, Sender};
use calloop::timer::{TimeoutAction, Timer};
use chrono::{Local, Timelike};
use fs_auth::pam::{self, Outcome};
use serde_json::{Map, Value, json};
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{self, ExtIdleNotificationV1};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure,
};
use fs_menu::field::Field;
use zeroize::Zeroizing;

use super::App;
use super::screensaver::SaverInput;
use crate::motion::{Animated, EFFECTS, EFFECTS_SLOW, Kind as Clock, SPATIAL};
use crate::scene::Bitmap;
use crate::surface::Ignore;
use crate::services::wallpaper::DepthCut;
use crate::services::{clipboard, compositor, power};
use crate::surface::Surface;
use crate::store::Topic;
use crate::surfaces::lock::{self as view, Boot, Button, Buttons, NowPlaying, Shared, TKey, View};

/// What finishes off the UI thread and comes back to the lock.
pub enum LockMsg {
    Pam(u64, Outcome),
    Avatar(Option<Bitmap>),
    /// The polkit dialog's identity picture.
    PolkitAvatar(Option<Bitmap>),
    /// The scrimmed wallpaper and its foreground, for a cut.
    Backdrop(Option<DepthCut>, Bitmap, Option<Bitmap>),
    /// The track's cover, for the art URL it was decoded from.
    Art(String, Option<Bitmap>),
    /// logind's PrepareForSleep.
    Sleep(bool),
    /// Whether the sleep delay inhibitor is held.
    Inhibiting(bool),
    Monitoring(bool),
    /// The logind session whose Lock signal is watched, once found.
    Session(Option<String>),
    /// logind's Lock (true) or Unlock (false) for that session.
    Logind(bool),
    /// An action's covering surface unmapped at `at`, with the cursor
    /// where the compositor put it then.
    ActionEnded { action: String, at: i64, cursor: Option<(f64, f64)> },
    Polkit(crate::services::polkit::Event),
    /// logind refused one of the lock's buttons: said under the field.
    Refused(String),
}

struct Out {
    output: WlOutput,
    name: String,
    surface: Surface<SessionLockSurface>,
    view: View,
    /// The surface's last frame was the screensaver's.
    saver: bool,
    /// Where the pointer last was on it.
    pointer: Option<(f64, f64)>,
    /// The boot lock's first frame, the splash alone, has been committed.
    splashed: bool,
    /// The departure's black was committed as this commit, and has been shown.
    dark: Option<u64>,
    dark_landed: bool,
}

#[derive(Default)]
pub struct Sleep {
    pub monitoring: bool,
    pub inhibiting: bool,
    preparing: bool,
    result: String,
    started: Option<Instant>,
    last: Option<Map<String, Value>>,
}

pub struct Lock {
    manager: SessionLockState,
    session: Option<SessionLock>,
    /// Asked for, as `isLocked` reports it.
    pub locked: bool,
    /// The compositor's word that every output is covered.
    pub secure: bool,
    outs: Vec<Out>,
    password: Field<Zeroizing<String>>,
    pub error: String,
    checking: bool,
    /// PAM said yes and the last picture is being handed to the desktop:
    /// nothing is drawn or typed until the session unlocks.
    unlocking: bool,
    attempt: u64,
    pub(super) tx: Option<Sender<LockMsg>>,
    /// The avatar at each output's scale.
    avatar: Vec<Bitmap>,
    /// The scrimmed wallpaper at each output's size, with what of it
    /// stands in front of the clock.
    backdrop: Vec<(Bitmap, Option<Bitmap>)>,
    /// The depth cut the backdrops were last asked for.
    depth_cut: Option<DepthCut>,
    /// The desktop's sculpture held still under the scrim, at half each
    /// output's pixels.
    shards: Vec<Bitmap>,
    clock: Option<RegistrationToken>,
    pub(super) dirty: bool,
    transport: i32,
    /// The track's cover at each output's scale, for the art URL it was
    /// decoded from.
    art: Option<(String, Vec<Bitmap>)>,
    fade: Animated,
    rise: Animated,
    wake: Animated,
    entered: bool,
    notifier: Option<ExtIdleNotifierV1>,
    idle_watch: Option<ExtIdleNotificationV1>,
    idle: bool,
    /// Set by a resume from suspend, cleared by any input on the lock.
    resume_guard: bool,
    /// The screensaver has shown inside this lock: idle brings it back.
    with_saver: bool,
    slept_at: Option<(f64, Instant)>,
    pub sleep: Sleep,
    /// The logind session [`crate::services::session`] watches.
    logind: Option<String>,
    /// Each output's last report, kept past the unlock until the next lock.
    reports: Map<String, Value>,
    /// Raised off the login marker, until the first unlock.
    login: bool,
    /// The splash's picture off its decode thread, once per output scale.
    splash: Vec<Bitmap>,
    splash_taken: bool,
    /// A button asked once, which the next Return on it carries out.
    armed: Option<Button>,
    /// PrepareForShutdown while locked: the lock itself fades to black, 1 at rest.
    depart: Animated,
    departing: bool,
}

impl Lock {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        Self {
            manager: SessionLockState::new(globals, qh),
            session: None,
            locked: false,
            secure: false,
            outs: Vec::new(),
            password: field(),
            error: String::new(),
            checking: false,
            unlocking: false,
            attempt: 0,
            tx: None,
            avatar: Vec::new(),
            backdrop: Vec::new(),
            depth_cut: None,
            shards: Vec::new(),
            clock: None,
            dirty: false,
            transport: -1,
            art: None,
            fade: Animated::new(1.0, EFFECTS),
            rise: Animated::new(0.0, SPATIAL),
            wake: Animated::new(1.0, EFFECTS_SLOW),
            entered: false,
            notifier: globals.bind(qh, 2..=2, Ignore).ok(),
            idle_watch: None,
            idle: false,
            resume_guard: false,
            with_saver: false,
            slept_at: None,
            sleep: Sleep::default(),
            logind: None,
            reports: Map::new(),
            login: false,
            splash: Vec::new(),
            splash_taken: false,
            armed: None,
            depart: Animated::new(1.0, EFFECTS),
            departing: false,
        }
    }
}

/// The lock's own idle watch: input anywhere, inhibitors ignored, so an
/// app holding the screen awake never keeps a locked screen lit.
pub struct LockIdle;

impl Dispatch2<ExtIdleNotificationV1, App> for LockIdle {
    fn event(&self, app: &mut App, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            ext_idle_notification_v1::Event::Idled => {
                app.lock.idle = true;
                // Not over a password being checked or a lock lifting: the
                // unlock hands over each output's lock picture, and an
                // output showing the screensaver has none to hand over.
                if app.lock.with_saver && app.lock.session.is_some() && !app.lock.checking && !app.lock.unlocking {
                    app.saver_start();
                }
            }
            ext_idle_notification_v1::Event::Resumed => (app.lock.idle, app.lock.resume_guard) = (false, false),
            _ => {}
        }
        app.lock.dirty = true;
    }
}

/// `lock.command` as read: a list of
/// non-empty strings, or nothing.
fn command(app: &App) -> Vec<String> {
    let Some(Value::Array(items)) = app.store.config.get("lock.command") else { return Vec::new() };
    let argv: Vec<String> = items.iter().filter_map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)).collect();
    if argv.len() == items.len() { argv } else { Vec::new() }
}

/// Present while this session's built-in lock is held, whichever shell
/// holds it.
fn held_marker() -> std::path::PathBuf {
    crate::instance::compositor_file("locked")
}

fn mark_held(held: bool) {
    let path = held_marker();
    let result = if held {
        path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&path, b""))
    } else {
        std::fs::remove_file(&path).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(e) })
    };
    if let Err(err) = result {
        eprintln!("lock: {}: {err}", path.display());
    }
}

fn login_marker() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("formalshell").join("login-lock")
}

static SPLASH: Mutex<Option<JoinHandle<Option<Bitmap>>>> = Mutex::new(None);

/// The boot lock's picture decoded on a thread of its own, started before
/// the Wayland connection so the lock's first frame never waits on it.
pub fn preload_login() {
    let Ok(text) = std::fs::read_to_string(login_marker()) else { return };
    let path = text.trim().to_owned();
    if path.is_empty() {
        return;
    }
    let decode = move || {
        let img = image::ImageReader::open(&path).ok().and_then(|r| r.with_guessed_format().ok()).and_then(|r| r.decode().ok()).map(|i| i.to_rgba8());
        let bitmap = img.and_then(|i| Some(Bitmap::from_rgba(u16::try_from(i.width()).ok()?, u16::try_from(i.height()).ok()?, i.into_raw())));
        if bitmap.is_none() {
            eprintln!("lock: the login picture {path} is unreadable, starting on black");
        }
        bitmap
    };
    match std::thread::Builder::new().name("fs-splash".into()).spawn(decode) {
        Ok(handle) => *SPLASH.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle),
        Err(err) => eprintln!("lock: login picture: {err}"),
    }
}

fn take_splash() -> Option<Bitmap> {
    let handle = SPLASH.lock().unwrap_or_else(|e| e.into_inner()).take()?;
    handle.join().ok().flatten()
}

fn via_logind(app: &App) -> bool {
    command(app).first().is_some_and(|b| b.rsplit('/').next() == Some("loginctl"))
}

fn on_path(name: &str) -> bool {
    if name.contains('/') {
        return std::path::Path::new(name).is_file();
    }
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}

impl App {
    /// The lock's own channel, and logind's sleep signal on the service thread.
    pub(super) fn start_lock(&mut self) {
        let Some(handle) = &self.handle else { return };
        let (tx, rx) = channel::channel::<LockMsg>();
        let inserted = handle.insert_source(rx, |event, _, app: &mut App| {
            if let channel::Event::Msg(msg) = event {
                app.lock_msg(msg);
            }
        });
        if inserted.is_err() {
            return;
        }
        self.lock.tx = Some(tx.clone());
        if self.lock.login && self.lock.session.is_some() {
            self.dress_lock();
            self.sync_agents();
            self.sync_shards();
        } else if held_marker().exists() {
            eprintln!("lock: the session was left locked, taking the lock back");
            let result = self.raise_lock();
            if result != "ok" {
                eprintln!("lock: {result}");
            }
        }
        if let Some(rt) = &self.runtime {
            let polkit = tx.clone();
            rt.service(move |ctx| {
                crate::services::session::start(ctx, tx.clone());
                crate::services::sleep::start(ctx, tx);
                crate::services::polkit::start(ctx, polkit);
            });
        }
    }

    /// `lock lock`. An external locker named by `lock.command` runs in
    /// place of the surface, unless it is not on PATH.
    pub fn lock(&mut self) -> String {
        let argv = command(self);
        if let Some(bin) = argv.first() {
            if on_path(bin) {
                compositor::spawn(&argv);
                return "ok".into();
            }
            eprintln!("LockService: lock.command {bin} is not on PATH, the built-in lock is used instead");
        }
        self.raise_lock()
    }

    /// `lock session`: the lock asked of logind, so it comes back through
    /// the same Lock signal every other locker sends. With no logind
    /// session found, the lock itself.
    pub fn lock_session(&mut self) -> String {
        match (&self.runtime, self.lock.logind.is_some()) {
            (Some(rt), true) => {
                rt.service(|ctx| {
                    crate::services::session::request(ctx);
                });
                "ok".into()
            }
            _ => self.lock(),
        }
    }

    fn locked_hint(&self, locked: bool) {
        if let Some(rt) = &self.runtime {
            rt.service(move |ctx| crate::services::session::locked_hint(ctx, locked));
        }
    }

    /// A `lock.command` that is loginctl comes back as logind's Lock, which
    /// raises the built-in lock: that one is not external.
    pub fn lock_external(&self) -> bool {
        !via_logind(self) && command(self).first().is_some_and(|b| on_path(b))
    }

    /// The boot's lock, called before any other surface is made and before
    /// the services start: the session lock and a surface per output, the
    /// rest left to [`Self::start_lock`].
    pub(super) fn raise_login_lock(&mut self) {
        if !login_marker().exists() {
            return;
        }
        if let Err(err) = self.acquire_lock() {
            eprintln!("lock: login lock: {err}");
            return;
        }
        self.lock.login = true;
        self.lock.locked = true;
        self.lock.dirty = true;
        crate::phase("login lock asked");
    }

    fn raise_lock(&mut self) -> String {
        self.lock.error.clear();
        if self.lock.session.is_none() {
            if let Err(err) = self.acquire_lock() {
                return err;
            }
            self.dress_lock();
        }
        self.lock.locked = true;
        self.sync_agents();
        self.sync_shards();
        self.lock.dirty = true;
        "ok".into()
    }

    fn acquire_lock(&mut self) -> Result<(), String> {
        match self.lock.manager.lock(&self.qh) {
            Ok(session) => self.lock.session = Some(session),
            Err(_) => return Err("error: lock failed to acquire session lock".into()),
        }
        self.lock.secure = false;
        self.lock.outs.clear();
        self.lock.reports.clear();
        self.lock.password = field();
        self.lock.checking = false;
        self.lock.transport = -1;
        self.lock.armed = None;
        self.lock.entered = false;
        self.lock.departing = false;
        self.lock.depart.jump(1.0);
        self.lock_outputs();
        Ok(())
    }

    /// What the lock draws besides its card, and its watches.
    fn dress_lock(&mut self) {
        self.load_lock_pictures();
        self.lock.art = None;
        self.load_lock_art();
        self.arm_lock_clock();
        (self.lock.idle, self.lock.resume_guard, self.lock.with_saver) = (false, false, false);
        self.watch_lock_idle();
        self.saver_rehost();
    }

    /// `lock.blankAfterSeconds` of no input anywhere blanks the lock.
    fn watch_lock_idle(&mut self) {
        let (Some(notifier), Some(seat)) = (self.lock.notifier.clone(), self.seats.seats().next()) else { return };
        let seconds = self.store.config.f64("lock.blankAfterSeconds").filter(|s| *s > 0.0).unwrap_or(30.0);
        self.lock.idle_watch = Some(notifier.get_input_idle_notification((seconds * 1000.0) as u32, &seat, &self.qh, LockIdle));
    }

    pub fn lock_blanked(&self) -> bool {
        self.lock.session.is_some() && !self.saver.active && ((self.lock.idle && !self.lock.with_saver) || self.lock.resume_guard)
    }

    /// This shell's lock surfaces are up, so nothing on a layer is seen.
    pub(super) fn lock_drawn(&self) -> bool {
        self.lock.session.is_some()
    }

    pub(super) fn lock_size(&self, output: &WlOutput) -> Option<(i32, i32)> {
        let o = self.lock.outs.iter().find(|o| o.output == *output && o.surface.configured)?;
        Some((o.view.scene.size.w, o.view.scene.size.h))
    }

    pub(super) fn lock_saver_mapped(&mut self) {
        if self.lock.session.is_some() {
            self.lock.with_saver = true;
        }
    }

    pub(super) fn lock_saver_drawn(&mut self) {
        self.lock.dirty = true;
    }

    /// The screensaver started or stopped under the lock. The card comes
    /// back with its entrance.
    pub(super) fn lock_saver_changed(&mut self) {
        if self.lock.session.is_none() {
            return;
        }
        if !self.saver.active {
            self.lock.entered = false;
        }
        self.lock.dirty = true;
    }

    /// Pointer input on a lock surface is activity: it lifts a resume blank,
    /// and a move or a press dismisses the screensaver. Enter and leave are
    /// the compositor mapping a surface under a still pointer, not activity,
    /// and neither is a motion to where the pointer already was.
    pub(super) fn lock_pointer(&mut self, events: &[PointerEvent]) {
        for e in events {
            let Some(o) = self.lock.outs.iter_mut().find(|o| *o.surface.layer.wl_surface() == e.surface) else { continue };
            let last = std::mem::replace(&mut o.pointer, Some(e.position));
            let moved = match e.kind {
                PointerEventKind::Enter { .. } => continue,
                PointerEventKind::Leave { .. } => {
                    o.pointer = None;
                    continue;
                }
                PointerEventKind::Motion { .. } => last != Some(e.position),
                PointerEventKind::Press { .. } => true,
                _ => false,
            };
            if self.lock.resume_guard {
                self.lock.resume_guard = false;
                self.lock.dirty = true;
            }
            if moved && self.saver.active {
                self.saver_input(None, SaverInput::Press);
            }
        }
    }

    /// A lock surface for every output that has none yet.
    pub(super) fn lock_outputs(&mut self) {
        let Some(session) = self.lock.session.clone() else { return };
        let outputs: Vec<WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            if self.lock.outs.iter().any(|o| o.output == output) {
                continue;
            }
            let name = self.outputs.info(&output).and_then(|i| i.name).unwrap_or_default();
            let wl = self.compositor.create_surface(&self.qh);
            let ls = session.create_lock_surface(wl, &output, &self.qh);
            let surface = Surface::on("lock", ls, &self.shm, self.started, &output);
            self.lock.outs.push(Out { output, name, surface, view: View::new(1, 1), saver: false, pointer: None, splashed: false, dark: None, dark_landed: false });
        }
    }

    /// The output gone: its lock surface destroyed with it, as
    /// ext-session-lock asks of a client.
    pub(super) fn lock_output_gone(&mut self, output: &WlOutput) {
        self.lock.outs.retain(|o| o.output != *output);
    }

    /// The avatar and the scrimmed wallpaper, decoded on the pool.
    fn load_lock_pictures(&mut self) {
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        let home = std::env::var("HOME").unwrap_or_default();
        let path = self.store.config.avatar_path(&home);
        let size = crate::surfaces::login::avatar_size(&self.store.theme.theme);
        self.lock.avatar.clear();
        self.lock.shards.clear();
        for scale in crate::surface::output_scales() {
            let (path, tx) = (path.clone(), tx.clone());
            rt.pool().submit(move || {
                let _ = tx.send(LockMsg::Avatar(view::avatar(&path, crate::scene::pixels_at(size, scale)).map(|b| b.at(scale))));
            });
        }
        self.load_lock_backdrops();
    }

    /// The scrimmed wallpaper and its foreground, decoded on the pool. What
    /// is on screen stays until its replacement lands.
    fn load_lock_backdrops(&mut self) {
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        let cut = DepthCut::read(&self.store.config, "lock", Some(DepthCut::THRESHOLD));
        self.lock.depth_cut = cut;
        let path = self.store.wallpaper.latest().map(|p| p.path.clone());
        for p in self.store.wallpaper.all().filter(|p| Some(&p.path) == path.as_ref()).cloned() {
            let tx = tx.clone();
            rt.pool().submit(move || {
                let (b, front) = view::backdrop(&p, cut);
                let _ = tx.send(LockMsg::Backdrop(cut, b, front));
            });
        }
    }

    /// The clock redraws on the minute, and on nothing else.
    fn arm_lock_clock(&mut self) {
        let Some(handle) = &self.handle else { return };
        if let Some(t) = self.lock.clock.take() {
            handle.remove(t);
        }
        let now = Local::now();
        let playing = self.store.media.active().is_some_and(|a| a.playing && a.length > 0.0);
        let left = if playing { 1 } else { 60 - now.second() as u64 };
        let at = Instant::now() + Duration::from_secs(left) - Duration::from_nanos(now.nanosecond() as u64 % 1_000_000_000);
        self.lock.clock = handle
            .insert_source(Timer::from_deadline(at), |_, _, app: &mut App| {
                app.lock.clock = None;
                if app.lock.session.is_some() {
                    app.lock.dirty = true;
                    app.arm_lock_clock();
                }
                TimeoutAction::Drop
            })
            .ok();
    }

    fn lock_msg(&mut self, msg: LockMsg) {
        match msg {
            LockMsg::Pam(attempt, outcome) if attempt == self.lock.attempt => self.lock_result(outcome),
            LockMsg::Pam(..) => {}
            LockMsg::Avatar(a) => {
                if let Some(a) = a {
                    crate::scene::keep(&mut self.lock.avatar, a);
                }
                self.lock.dirty = true;
            }
            LockMsg::PolkitAvatar(a) => {
                if let Some(d) = &mut self.polkit {
                    d.set_avatar(a);
                }
            }
            LockMsg::Art(url, art) => {
                if let (Some((u, made)), Some(art)) = (&mut self.lock.art, art)
                    && *u == url
                {
                    crate::scene::keep(made, art);
                }
                self.lock.dirty = true;
            }
            LockMsg::Backdrop(cut, ..) if cut != self.lock.depth_cut => {}
            LockMsg::Backdrop(_, b, front) => {
                let size = (b.pixmap.width(), b.pixmap.height());
                self.lock.backdrop.retain(|(m, _)| (m.pixmap.width(), m.pixmap.height()) != size);
                self.lock.backdrop.push((b, front));
                self.lock.dirty = true;
            }
            LockMsg::Sleep(sleeping) => {
                // Waking from a real suspend with the lock up blanks it until
                // the next input: CLOCK_BOOTTIME
                // runs through a suspend and CLOCK_MONOTONIC does not. The
                // blank goes up before the machine sleeps, cut rather than
                // faded, so the frame the panel shows on wake is already black
                // and nothing on screen changes until a key or the pointer.
                if sleeping {
                    self.lock.slept_at = Some((boottime(), Instant::now()));
                    self.prepare_for_sleep(true);
                    if self.lock.session.is_some() {
                        self.lock.resume_guard = true;
                        self.lock.wake.jump(0.0);
                        self.lock.dirty = true;
                    }
                } else {
                    let slept = self.lock.slept_at.take().is_some_and(|(boot, mono)| (boottime() - boot) - mono.elapsed().as_secs_f64() > 3.0);
                    if self.lock.session.is_some() && !slept && self.lock.resume_guard {
                        self.lock.resume_guard = false;
                        self.lock.dirty = true;
                    }
                    self.prepare_for_sleep(false);
                }
            }
            LockMsg::Inhibiting(on) => self.lock.sleep.inhibiting = on,
            LockMsg::Monitoring(on) => self.lock.sleep.monitoring = on,
            LockMsg::Session(id) => {
                // A lock that went secure before the session was found, and a
                // hint left set by a shell that died locked.
                if id.is_some() {
                    self.locked_hint(self.lock.secure);
                }
                self.lock.logind = id;
            }
            LockMsg::Logind(true) => {
                eprintln!("lock: logind asked for the lock");
                if via_logind(self) {
                    self.raise_lock();
                } else if !self.lock.locked || self.lock_external() {
                    self.lock();
                }
            }
            // ext-session-lock takes an unlock only once the compositor has
            // said the session is locked.
            LockMsg::Logind(false) if self.lock.session.is_some() && self.lock.secure => {
                eprintln!("lock: logind asked for the unlock");
                // A password check still running belongs to this lock: its
                // answer must not lift the next one.
                self.lock.attempt += 1;
                self.lock.checking = false;
                self.begin_unlock();
            }
            LockMsg::Logind(false) => eprintln!("lock: logind's Unlock with no secure lock of this shell's to lift"),
            LockMsg::ActionEnded { action, at, cursor } => self.hot_corner_ended(&action, at, cursor),
            LockMsg::Polkit(event) => self.polkit_event(event),
            LockMsg::Refused(why) => {
                if self.lock.session.is_some() {
                    self.lock.error = why;
                    self.lock.dirty = true;
                }
            }
        }
    }

    fn submit_password(&mut self) {
        if self.lock.checking {
            return;
        }
        let Some(tx) = self.lock.tx.clone() else { return };
        let password = self.lock.password.replace(buffer());
        self.lock.error.clear();
        self.lock.checking = true;
        self.lock.attempt += 1;
        self.lock.dirty = true;
        let attempt = self.lock.attempt;
        let spawned = std::thread::Builder::new().name("fs-lock-auth".into()).spawn(move || {
            let outcome = match pam::current_user() {
                Some(user) => pam::check(pam::LOCK_SERVICE, &user, password),
                None => Outcome::StartFailed("no user".into()),
            };
            let _ = tx.send(LockMsg::Pam(attempt, outcome));
        });
        if spawned.is_err() {
            self.lock.checking = false;
            self.lock.error = "PAM error".into();
        }
    }

    fn lock_result(&mut self, outcome: Outcome) {
        self.lock.checking = false;
        self.lock.dirty = true;
        match outcome {
            Outcome::Success => self.begin_unlock(),
            Outcome::Failed(_) => self.lock.error = "Wrong password".into(),
            Outcome::MaxTries(_) => self.lock.error = "Account locked".into(),
            Outcome::AccountUnavailable(why) => {
                eprintln!("lock: pam account: {why}");
                self.lock.error = "Account unavailable".into();
            }
            // The lock has no step to set a new password in.
            Outcome::NewPasswordRequired(why) => {
                eprintln!("lock: pam account: {why}");
                self.lock.error = "Password expired, change it on tty2".into();
            }
            Outcome::StartFailed(why) | Outcome::Error(why) => {
                eprintln!("lock: pam: {why}");
                self.lock.error = "PAM error".into();
            }
        }
    }

    /// PAM's success and logind's Unlock. Each output's last picture goes to
    /// a veil first (wayland/veil.rs), so the desktop comes in under it.
    fn begin_unlock(&mut self) {
        if self.lock.unlocking {
            return;
        }
        let pictures: Vec<_> = self
            .lock
            .outs
            .iter_mut()
            .filter(|o| o.surface.mapped && !o.saver)
            .map(|o| (o.output.clone(), std::mem::replace(&mut o.view.scene, crate::scene::Scene::new(1, 1))))
            .collect();
        self.lock.unlocking = true;
        if !self.hand_over(pictures) {
            self.finish_unlock();
        }
    }

    pub(super) fn lock_unlocking(&self) -> bool {
        self.lock.unlocking
    }

    /// The unlock itself, once the pictures are committed or the wait for
    /// them is over.
    pub(super) fn finish_unlock(&mut self) {
        self.lock.unlocking = false;
        if let Some(session) = self.lock.session.take() {
            session.unlock();
            // The unlock reaches the compositor before the markers below go:
            // a shell killed between the two would leave a lock no restarted
            // shell takes back.
            if let Some(Err(err)) = self.lock.outs.first().and_then(|o| o.output.backend().upgrade()).map(|b| b.flush()) {
                eprintln!("lock: flushing the unlock: {err}");
            }
        }
        self.lock.outs.clear();
        if let Some(w) = self.lock.idle_watch.take() {
            w.destroy();
        }
        self.saver_unlocked();
        self.hot_corner_action_ended("lock");
        self.hot_corner_action_ended("screensaver");
        self.lock.locked = false;
        self.sync_agents();
        self.sync_shards();
        self.lock.secure = false;
        self.locked_hint(false);
        mark_held(false);
        self.lock.password = field();
        self.lock.error.clear();
        self.lock.armed = None;
        if std::mem::take(&mut self.lock.login) {
            self.lock.splash.clear();
            if let Err(err) = std::fs::remove_file(login_marker())
                && err.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("lock: {}: {err}", login_marker().display());
            }
        }
        if let (Some(handle), Some(t)) = (&self.handle, self.lock.clock.take()) {
            handle.remove(t);
        }
        self.lift_handover();
    }

    /// What the now-playing block shows, if it shows.
    fn lock_media(&self) -> Option<crate::services::media::Active> {
        self.store.media.active().filter(|a| a.kind != "stream")
    }

    /// Shutdown and Reboot, and Other user on the boot's lock.
    fn lock_buttons(&self) -> Vec<Button> {
        let mut buttons = vec![Button::PowerOff, Button::Reboot];
        if self.lock.login {
            buttons.push(Button::OtherUser);
        }
        buttons
    }

    /// The transport's stops, then the buttons'.
    fn lock_media_stops(&self) -> i32 {
        if self.lock_media().is_some() { view::TRANSPORT as i32 } else { 0 }
    }

    /// The transport's and the buttons' keys first (Lock/model.js's
    /// `transportKey` through the field's own key filter, over both), then
    /// the field's.
    fn transport_key(&mut self, event: &KeyEvent) -> bool {
        let media = self.lock_media_stops();
        let buttons = self.lock_buttons();
        let count = media + buttons.len() as i32;
        let key = match event.keysym {
            Keysym::Tab => TKey::Tab,
            Keysym::ISO_Left_Tab => TKey::Backtab,
            Keysym::Left => TKey::Left,
            Keysym::Right => TKey::Right,
            Keysym::Return | Keysym::KP_Enter => TKey::Enter,
            Keysym::Escape => TKey::Escape,
            Keysym::Shift_L | Keysym::Shift_R | Keysym::Control_L | Keysym::Control_R | Keysym::Alt_L | Keysym::Alt_R
            | Keysym::ISO_Level3_Shift | Keysym::Meta_L | Keysym::Meta_R | Keysym::Super_L | Keysym::Super_R | Keysym::Caps_Lock => TKey::Modifier,
            _ => TKey::Other,
        };
        let step = view::transport_key(self.lock.transport, count, key);
        self.lock.transport = step.index;
        let on = usize::try_from(step.index - media).ok().and_then(|i| buttons.get(i).copied());
        if on != self.lock.armed {
            self.lock.armed = None;
        }
        if step.press {
            match on {
                Some(b) if self.lock.armed == Some(b) => {
                    self.lock.armed = None;
                    self.lock_button(b);
                }
                Some(b) => self.lock.armed = Some(b),
                None => match step.index {
                    0 => self.store.media.previous(),
                    1 => self.store.media.play_pause(),
                    _ => self.store.media.next(),
                },
            }
        }
        step.taken
    }

    /// A confirmed button. logind is asked without interactive
    /// authorization: no polkit agent can show over the lock.
    fn lock_button(&mut self, b: Button) {
        self.lock.error.clear();
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        eprintln!("lock: {} confirmed", b.id());
        match b {
            Button::PowerOff | Button::Reboot => {
                let action = if b == Button::PowerOff { power::Action::PowerOff } else { power::Action::Reboot };
                rt.service(move |ctx| {
                    ctx.spawn(async move {
                        let method = action.method();
                        match power::call_quiet(method).await {
                            Ok(()) => eprintln!("lock: {method} accepted"),
                            Err(err) => {
                                eprintln!("lock: {method}: {err}");
                                let _ = tx.send(LockMsg::Refused(format!("{} refused", b.label())));
                            }
                        }
                    });
                });
            }
            Button::OtherUser => match self.lock.logind.clone() {
                Some(id) => rt.service(move |ctx| crate::services::session::terminate(ctx, id, tx)),
                None => self.lock.error = "Could not end the session".into(),
            },
        }
    }

    /// Redraws for what the lock shows; a new wallpaper is decoded again.
    pub fn lock_changed(&mut self, topic: Topic) {
        if self.lock.session.is_none() {
            return;
        }
        match topic {
            Topic::Wallpaper => {
                self.lock.backdrop.clear();
                self.load_lock_pictures();
            }
            Topic::Config if DepthCut::read(&self.store.config, "lock", Some(DepthCut::THRESHOLD)) != self.lock.depth_cut => self.load_lock_backdrops(),
            // A map written for the picture under the lock.
            Topic::Depth if self.store.wallpaper.latest().is_some_and(|p| self.store.depth.wrote(&p.path)) => self.load_lock_backdrops(),
            Topic::Media => {
                if self.lock_media().is_none() {
                    self.lock.transport = -1;
                }
                self.load_lock_art();
            }
            _ => {}
        }
        self.lock.dirty = true;
    }

    /// The active track's cover, decoded on the pool when its URL changes.
    fn load_lock_art(&mut self) {
        let url = self.lock_media().map(|a| a.art_url).unwrap_or_default();
        if self.lock.art.as_ref().is_some_and(|(u, _)| *u == url) {
            return;
        }
        self.lock.art = Some((url.clone(), Vec::new()));
        if !url.starts_with("file://") && !url.starts_with("data:") {
            return;
        }
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        let size = self.store.theme.theme.space.control_height * 2.0;
        for scale in crate::surface::output_scales() {
            let (url, tx) = (url.clone(), tx.clone());
            rt.pool().submit(move || {
                let art = view::cover(&url, crate::scene::pixels_at(size, scale)).map(|b| b.at(scale));
                let _ = tx.send(LockMsg::Art(url, art));
            });
        }
    }

    /// The clipboard's text into the password field. True while locked.
    pub(super) fn lock_paste(&mut self, text: &str) -> bool {
        if self.lock.session.is_none() {
            return false;
        }
        if !self.lock.checking && self.lock.password.paste(text) {
            self.lock.dirty = true;
        }
        true
    }

    /// Every key while locked is the lock's. True when it took the key.
    pub(super) fn lock_key(&mut self, event: &KeyEvent, repeat: bool) -> bool {
        if self.lock.session.is_none() {
            return false;
        }
        if self.lock.unlocking {
            return true;
        }
        // Repeats are timed on this loop, so a frame that holds back a
        // release types the key again behind the dots. Only editing keys
        // repeat on the password.
        if repeat && !matches!(event.keysym, Keysym::BackSpace | Keysym::Delete | Keysym::KP_Delete | Keysym::Left | Keysym::Right) {
            return true;
        }
        if self.lock.resume_guard {
            self.lock.resume_guard = false;
        }
        if self.saver.active {
            self.saver_input(None, SaverInput::Key);
            return true;
        }
        if self.transport_key(event) {
            self.lock.dirty = true;
            return true;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => self.submit_password(),
            Keysym::Escape => {
                self.lock.password = field();
                self.lock.error.clear();
            }
            // A secret field hands nothing out, so a paste is all that can
            // come back from it.
            sym if !self.lock.checking => {
                let out = self.lock.password.key(super::field_key(sym), self.mods, event.utf8.as_deref());
                match out {
                    fs_menu::field::Outcome::Paste => clipboard::command(clipboard::Cmd::Paste),
                    _ => {}
                }
            }
            _ => {}
        }
        self.lock.dirty = true;
        true
    }

    pub(super) fn lock_frame(&mut self, surface: &WlSurface) {
        if let Some(o) = self.lock.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == surface) {
            let s = &mut o.surface;
            (s.frame_pending, s.mapped, s.callbacks) = (false, true, s.callbacks + 1);
            // No commit goes out while a callback is pending, so this one
            // is the black frame's own.
            if o.dark.is_some() {
                o.dark_landed = true;
            }
            self.lock.dirty = true;
        }
    }

    /// The still each output's lock draws of the sculpture, at the clock
    /// the desktop's stopped on, made once per lock and size.
    fn lock_shards(&mut self) {
        if !self.store.theme.theme.wallpaper_animate {
            self.lock.shards.clear();
            return;
        }
        let devices: Vec<(i32, i32)> = self
            .lock
            .outs
            .iter()
            .filter(|o| o.surface.configured)
            .map(|o| (crate::render::scaled(o.view.scene.size.w, o.surface.scale()), crate::render::scaled(o.view.scene.size.h, o.surface.scale())))
            .filter(|&(w, h)| !self.lock.shards.iter().any(|b| (i32::from(b.pixmap.width()), i32::from(b.pixmap.height())) == (w, h)))
            .collect();
        if devices.is_empty() {
            return;
        }
        let word = self.shard_word();
        let theme = &self.store.theme.theme;
        let wanted = self.store.desktop();
        let base = self.store.wallpaper.latest().filter(|p| p.path == wanted).and_then(|p| crate::shards::mean(&p.bgra)).unwrap_or(theme.colors.get("background"));
        let look = crate::shards::Look::new(theme, base, word, (1.0 - view::SCRIM) as f32);
        let t = self.heads.iter().find_map(|h| h.backdrop.as_ref()?.shards.as_ref().map(|s| s.t)).unwrap_or(0.0);
        let mut raster = crate::shards::Raster::new();
        for (w, h) in devices {
            let (Ok(bw), Ok(bh)) = (u16::try_from(w), u16::try_from(h)) else { continue };
            let pixmap = raster.still(bw, bh, t, &look);
            self.lock.shards.push(Bitmap { pixmap: std::sync::Arc::new(pixmap), density: 1.0 });
        }
    }

    /// An output of the boot's lock still owes its first frame: nothing
    /// else is drawn until it has gone out.
    pub(super) fn lock_splash_pending(&self) -> bool {
        self.lock.login && !self.lock.unlocking && self.lock.outs.iter().any(|o| !o.splashed)
    }

    fn present_splash(&mut self) {
        if !std::mem::replace(&mut self.lock.splash_taken, true) {
            self.lock.splash = take_splash().into_iter().collect();
        }
        let qh = self.qh.clone();
        let mut first = false;
        for o in self.lock.outs.iter_mut().filter(|o| o.surface.configured && !o.splashed) {
            let k = o.surface.scale();
            let picture = self.lock.splash.first().map(|b| b.clone().at(k));
            o.view.draw_splash(picture.as_ref());
            o.surface.present(&mut o.view.scene, true, &qh);
            o.splashed = true;
            first |= o.surface.commits() == 1;
        }
        if first {
            crate::phase("login lock first commit");
        }
        self.lock.dirty = true;
    }

    /// PrepareForShutdown (or its cancel) while this shell's lock is up:
    /// the lock fades itself, since the compositor draws no overlay over a
    /// session lock. False when there is no lock to fade.
    pub(super) fn lock_depart(&mut self, going: bool, ms: f64) -> bool {
        if self.lock.session.is_none() || (!going && !self.lock.departing) {
            return false;
        }
        self.lock.departing = going;
        for o in &mut self.lock.outs {
            (o.dark, o.dark_landed) = (None, false);
        }
        self.lock.depart.set(Instant::now(), if going { 0.0 } else { 1.0 }, ms);
        self.lock.dirty = true;
        true
    }

    /// Every output has shown the departure's black.
    pub(super) fn lock_departed(&self) -> bool {
        !self.lock.departing || self.lock.outs.iter().filter(|o| o.surface.configured).all(|o| o.dark_landed)
    }

    pub(super) fn present_lock(&mut self) {
        if self.lock.outs.is_empty() || !self.lock.dirty || self.lock.unlocking {
            return;
        }
        self.lock.dirty = false;
        if self.lock_splash_pending() {
            self.present_splash();
            return;
        }
        self.lock_shards();
        let now = Instant::now();
        let theme = &self.store.theme.theme;
        // The entrance: a fade that must
        // not overshoot and a rise that does, once the first output can draw.
        if !self.lock.entered && self.lock.outs.iter().any(|o| o.surface.configured) {
            self.lock.entered = true;
            let scale = self.motion_scale;
            self.lock.fade.jump(0.0);
            self.lock.fade.set(now, 1.0, Clock::Effects.ms(theme) * scale);
            self.lock.rise.jump(theme.space.section_gap);
            self.lock.rise.set(now, 0.0, Clock::Spatial.ms(theme) * scale);
        }
        let blanked = self.lock_blanked();
        let reveal = if theme.motion_enabled { theme.motion().families.reveal * self.motion_scale } else { 0.0 };
        self.lock.wake.set(now, if blanked { 0.0 } else { 1.0 }, reveal);
        let departing = self.lock.departing || self.lock.depart.running(now);
        let entering = self.lock.fade.running(now) || self.lock.rise.running(now) || self.lock.wake.running(now) || departing;
        let dark = self.lock.departing && !self.lock.depart.running(now);
        let login = self.lock.login;
        let has_wallpaper = !self.store.desktop().is_empty() || login;
        let shared = Shared {
            now: Local::now(),
            name: crate::greeter::users::own_name(),
            prompt: view::Prompt::password(self.lock.password.text().chars().count(), self.lock.password.caret_chars(), self.lock.checking),
            error: &self.lock.error,
            has_wallpaper,
            backdrop: None,
            foreground: None,
            shards: None,
            picture: self.store.wallpaper.latest().filter(|p| !login && has_wallpaper && p.path == self.store.desktop()),
            avatar: None,
            media: None,
            enter: (self.lock.fade.value(now).clamp(0.0, 1.0) as f32, self.lock.rise.value(now)),
            blanked,
            wake: (self.lock.wake.value(now) * self.lock.depart.value(now)).clamp(0.0, 1.0) as f32,
            boot: None,
            buttons: Buttons::default(),
        };
        let active = self.lock_media();
        let mpris = active.as_ref().and_then(|a| self.store.media.mpris.iter().find(|p| p.id == a.id));
        let arts = self.lock.art.as_ref().filter(|(u, _)| active.as_ref().is_some_and(|a| a.art_url == *u)).map(|(_, b)| b.as_slice());
        let media = self.lock_media_stops();
        let shown = self.lock_buttons();
        for k in self.lock.outs.iter().map(|o| o.surface.scale()).collect::<Vec<_>>() {
            if let Some(b) = self.lock.splash.first().filter(|_| !self.lock.splash.iter().any(|b| f64::from(b.density) == k)).cloned() {
                self.lock.splash.push(b.at(k));
            }
        }
        let mut shared = Shared {
            media: active.as_ref().map(|a| NowPlaying {
                title: &a.title,
                artist: &a.artist,
                playing: a.playing,
                can_previous: mpris.is_none_or(|p| p.can_previous),
                can_toggle: mpris.is_none_or(|p| p.can_toggle),
                can_next: mpris.is_none_or(|p| p.can_next),
                progress: (a.length > 0.0).then(|| a.position / a.length),
                art: None,
                cursor: if self.lock.transport < media { self.lock.transport } else { -1 },
            }),
            buttons: Buttons { shown, cursor: if self.lock.transport >= media { self.lock.transport - media } else { -1 }, armed: self.lock.armed },
            ..shared
        };
        let qh = self.qh.clone();
        let saver = &mut self.saver;
        let going = self.lock.departing;
        for o in &mut self.lock.outs {
            if !o.surface.configured {
                continue;
            }
            let size = o.view.scene.size;
            let k = o.surface.scale();
            let device = (crate::render::scaled(size.w, k), crate::render::scaled(size.h, k));
            let backdrops = &self.lock.backdrop;
            let pair = backdrops.iter().find(|(b, _)| (i32::from(b.pixmap.width()), i32::from(b.pixmap.height())) == device).or(backdrops.last());
            shared.backdrop = pair.map(|(b, _)| b);
            shared.foreground = pair.and_then(|(_, f)| f.as_ref());
            shared.shards = self.lock.shards.iter().find(|b| (i32::from(b.pixmap.width()), i32::from(b.pixmap.height())) == device);
            shared.avatar = crate::scene::nearest(&self.lock.avatar, k);
            if login {
                shared.boot = Some(Boot { splash: self.lock.splash.iter().find(|b| f64::from(b.density) == k) });
                (shared.backdrop, shared.foreground, shared.shards) = (None, None, None);
            }
            if let Some(m) = &mut shared.media {
                m.art = arts.and_then(|a| crate::scene::nearest(a, k));
            }
            // One renderer behind two scenes: a switch repaints all of it. A
            // departure fades the lock's own scene, the screensaver's too.
            if let Some(scene) = saver.lock_scene(&o.output, (size.w, size.h)).filter(|_| !going) {
                if !std::mem::replace(&mut o.saver, true) {
                    scene.touch(size);
                }
                // ttfx frames arrive faster than the output refreshes: each
                // commit waits for the last one's frame callback.
                let damaged = scene.has_damage();
                o.surface.present(scene, damaged, &qh);
                continue;
            }
            let _raster = crate::scene::raster_at(k);
            o.view.ui.motion_scale = self.motion_scale;
            let moving = o.view.draw(&shared, theme, &mut self.bar.kit, now) || entering;
            if std::mem::replace(&mut o.saver, false) {
                o.view.scene.touch(size);
            }
            if !o.name.is_empty() && !blanked {
                self.lock.reports.insert(o.name.clone(), o.view.report.clone());
            }
            let commits = o.surface.commits();
            o.surface.present(&mut o.view.scene, moving || (self.lock.departing && !o.dark_landed), &qh);
            if dark && o.dark.is_none() && o.surface.commits() > commits {
                o.dark = Some(o.surface.commits());
            }
        }
    }

    /// `lock status`'s object (Lock/lock.js's `status` plus `beforeSleep`).
    pub fn lock_status(&self) -> String {
        let external = self.lock_external();
        let mut state = if external {
            json!({"external": true, "locked": null, "secure": null, "authError": null, "blanked": null, "outputs": null})
        } else {
            let outputs = self.lock.reports.clone();
            json!({
                "external": false,
                "locked": self.lock.locked,
                "secure": self.lock.secure,
                "authError": self.lock.error,
                "blanked": self.lock_blanked(),
                "outputs": outputs,
                "boot": self.lock.login,
                "buttons": self.lock_buttons().iter().map(|b| b.id()).collect::<Vec<_>>(),
                "buttonCursor": usize::try_from(self.lock.transport - self.lock_media_stops()).ok().and_then(|i| self.lock_buttons().get(i).map(|b| b.id())),
                "armed": self.lock.armed.map(Button::id),
            })
        };
        let s = &self.lock.sleep;
        state["logindSession"] = self.lock.logind.clone().map_or(Value::Null, Value::String);
        state["beforeSleep"] = json!({
            "enabled": self.lock_before_sleep(),
            "monitoring": s.monitoring,
            "inhibiting": s.inhibiting,
            "last": s.last.clone().map_or(Value::Null, Value::Object),
        });
        state.to_string()
    }

    pub fn lock_before_sleep(&self) -> bool {
        self.store.config.bool("lock.beforeSleep") != Some(false)
    }

    /// Lock, then let the delay
    /// inhibitor go once the lock is secure (or failed, or timed out).
    fn prepare_for_sleep(&mut self, sleeping: bool) {
        if !sleeping {
            self.lock.sleep.preparing = false;
            return;
        }
        if !self.lock_before_sleep() {
            if let Some(rt) = &self.runtime {
                rt.service(|_| crate::services::sleep::release());
            }
            return;
        }
        if self.lock.sleep.preparing {
            return;
        }
        self.lock.sleep.preparing = true;
        let result = if self.lock.locked && !self.lock_external() { "already".to_owned() } else { self.lock() };
        let at = epoch_ms();
        let mut last = Map::new();
        last.insert("result".into(), json!(result));
        last.insert("release".into(), Value::Null);
        last.insert("preparedAt".into(), json!(at));
        last.insert("releasedAt".into(), Value::Null);
        self.lock.sleep.last = Some(last);
        self.lock.sleep.result = result;
        self.lock.sleep.started = Some(Instant::now());
        self.check_sleep_release();
    }

    /// Lock/lock.js's `sleepRelease`, polled every 50ms until it answers.
    fn check_sleep_release(&mut self) {
        let Some(started) = self.lock.sleep.started else { return };
        let elapsed = started.elapsed().as_millis() as u64;
        let external = self.lock_external();
        let result = self.lock.sleep.result.as_str();
        // The blank set for the sleep is committed before the machine may go.
        let drawing = self.lock.dirty && !self.lock.outs.is_empty() && elapsed < 3000;
        let why = if drawing {
            ""
        } else if result == "already" {
            "already"
        } else if external {
            if elapsed >= 1000 { "external" } else { "" }
        } else if result != "ok" {
            "failed"
        } else if self.lock.secure {
            "secure"
        } else if elapsed >= 3000 {
            "timeout"
        } else {
            ""
        };
        if why.is_empty() {
            let Some(handle) = &self.handle else { return };
            let _ = handle.insert_source(Timer::from_duration(Duration::from_millis(50)), |_, _, app: &mut App| {
                app.check_sleep_release();
                TimeoutAction::Drop
            });
            return;
        }
        self.lock.sleep.started = None;
        if let Some(last) = &mut self.lock.sleep.last {
            last.insert("release".into(), json!(why));
            last.insert("releasedAt".into(), json!(epoch_ms()));
        }
        if why == "failed" || why == "timeout" {
            eprintln!("LockService: letting suspend proceed without a lock: {why} {result}");
        }
        if let Some(rt) = &self.runtime {
            rt.service(|_| crate::services::sleep::release());
        }
    }
}

/// A typed secret's buffer, sized up front so typing never reallocates and
/// leaves an unwiped copy behind.
pub(super) fn buffer() -> Zeroizing<String> {
    Zeroizing::new(String::with_capacity(256))
}

/// An empty masked field over a fresh [`buffer`].
pub(super) fn field() -> Field<Zeroizing<String>> {
    Field::new(buffer()).secret()
}

/// Seconds on CLOCK_BOOTTIME.
fn boottime() -> f64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    // SAFETY: clock_gettime writes one timespec.
    unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) };
    ts.tv_sec as f64 + ts.tv_nsec as f64 / 1e9
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl SessionLockHandler for App {
    fn locked(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        self.lock.secure = true;
        self.locked_hint(true);
        mark_held(true);
        eprintln!("lock: secure");
    }

    /// The compositor refused the lock or ended it (another locker holds
    /// the session): nothing of ours is on screen.
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        eprintln!("lock: finished by the compositor");
        self.lock.unlocking = false;
        self.lift_handover();
        self.lock.session = None;
        self.lock.outs.clear();
        self.lock.locked = false;
        self.sync_agents();
        self.sync_shards();
        // A lock refused before it went secure (a restore the compositor
        // does not allow) leaves the predecessor's lock held, marker and all.
        if self.lock.secure {
            mark_held(false);
        }
        self.lock.secure = false;
        self.locked_hint(false);
        self.saver_rehost();
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, configure: SessionLockSurfaceConfigure, _: u32) {
        let (w, h) = (configure.new_size.0 as i32, configure.new_size.1 as i32);
        let Some(o) = self.lock.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == surface.wl_surface()) else { return };
        o.view.resize(w, h);
        o.surface.configure(w, h);
        let output = o.output.clone();
        self.saver_lock_configure(&output, w, h);
        self.lock.dirty = true;
    }
}
