//! The session lock: ext-session-lock-v1 with
//! one lock surface per output, the password typed into it checked by PAM
//! off the UI thread, and the only unlock being PAM's own success.
//!
//! Fail closed: nothing here unlocks on an error. A shell that dies while
//! locked leaves the compositor holding the session locked, which is the
//! protocol's contract, and a lock this process drops without unlocking is
//! a protocol error that does the same.

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
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{self, ExtIdleNotificationV1};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::session_lock::{
    SessionLock, SessionLockHandler, SessionLockState, SessionLockSurface, SessionLockSurfaceConfigure,
};
use fs_menu::field::Field;
use zeroize::Zeroizing;

use super::App;
use crate::motion::{Animated, EFFECTS, EFFECTS_SLOW, Kind as Clock, SPATIAL};
use crate::scene::Bitmap;
use crate::surface::Ignore;
use crate::services::{clipboard, compositor};
use crate::surface::Surface;
use crate::store::Topic;
use crate::surfaces::lock::{self as view, NowPlaying, Shared, TKey, View};

/// What finishes off the UI thread and comes back to the lock.
pub enum LockMsg {
    Pam(u64, Outcome),
    Avatar(Option<Bitmap>),
    /// The polkit dialog's identity picture.
    PolkitAvatar(Option<Bitmap>),
    Backdrop(Bitmap),
    /// The track's cover, for the art URL it was decoded from.
    Art(String, Option<Bitmap>),
    /// logind's PrepareForSleep.
    Sleep(bool),
    /// Whether the sleep delay inhibitor is held.
    Inhibiting(bool),
    Monitoring(bool),
    /// An action's covering surface unmapped at `at`, with the cursor
    /// where the compositor put it then.
    ActionEnded { action: String, at: i64, cursor: Option<(f64, f64)> },
    Polkit(crate::services::polkit::Event),
}

struct Out {
    output: WlOutput,
    name: String,
    surface: Surface<SessionLockSurface>,
    view: View,
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
    attempt: u64,
    pub(super) tx: Option<Sender<LockMsg>>,
    avatar: Option<Bitmap>,
    backdrop: Option<Bitmap>,
    clock: Option<RegistrationToken>,
    pub(super) dirty: bool,
    transport: i32,
    art: Option<(String, Option<Bitmap>)>,
    fade: Animated,
    rise: Animated,
    wake: Animated,
    entered: bool,
    notifier: Option<ExtIdleNotifierV1>,
    idle_watch: Option<ExtIdleNotificationV1>,
    idle: bool,
    /// Set by a resume from suspend, cleared by any input on the lock.
    resume_guard: bool,
    slept_at: Option<(f64, Instant)>,
    pub sleep: Sleep,
    /// Each output's last report, kept past the unlock until the next lock.
    reports: Map<String, Value>,
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
            attempt: 0,
            tx: None,
            avatar: None,
            backdrop: None,
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
            slept_at: None,
            sleep: Sleep::default(),
            reports: Map::new(),
        }
    }
}

/// The lock's own idle watch: input anywhere, inhibitors ignored, so an
/// app holding the screen awake never keeps a locked screen lit.
pub struct LockIdle;

impl Dispatch2<ExtIdleNotificationV1, App> for LockIdle {
    fn event(&self, app: &mut App, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            ext_idle_notification_v1::Event::Idled => app.lock.idle = true,
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
        if let Some(rt) = &self.runtime {
            let polkit = tx.clone();
            rt.service(move |ctx| {
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

    pub fn lock_external(&self) -> bool {
        command(self).first().is_some_and(|b| on_path(b))
    }

    fn raise_lock(&mut self) -> String {
        self.lock.error.clear();
        if self.lock.session.is_none() {
            match self.lock.manager.lock(&self.qh) {
                Ok(session) => self.lock.session = Some(session),
                Err(_) => return "error: lock failed to acquire session lock".into(),
            }
            self.lock.secure = false;
            self.lock.outs.clear();
            self.lock.reports.clear();
            self.lock.password = field();
            self.lock.checking = false;
            self.lock_outputs();
            self.load_lock_pictures();
            self.lock.transport = -1;
            self.lock.art = None;
            self.load_lock_art();
            self.arm_lock_clock();
            self.lock.entered = false;
            (self.lock.idle, self.lock.resume_guard) = (false, false);
            self.watch_lock_idle();
        }
        self.lock.locked = true;
        self.sync_herdr();
        self.lock.dirty = true;
        "ok".into()
    }

    /// `lock.blankAfterSeconds` of no input anywhere blanks the lock.
    fn watch_lock_idle(&mut self) {
        let (Some(notifier), Some(seat)) = (self.lock.notifier.clone(), self.seats.seats().next()) else { return };
        let seconds = self.store.config.f64("lock.blankAfterSeconds").filter(|s| *s > 0.0).unwrap_or(30.0);
        self.lock.idle_watch = Some(notifier.get_input_idle_notification((seconds * 1000.0) as u32, &seat, &self.qh, LockIdle));
    }

    pub fn lock_blanked(&self) -> bool {
        self.lock.session.is_some() && (self.lock.idle || self.lock.resume_guard)
    }

    /// Pointer input on a lock surface is activity: it lifts a resume blank.
    pub(super) fn lock_pointer(&mut self, surfaces: impl Iterator<Item = WlSurface>) {
        for s in surfaces {
            if self.lock.outs.iter().any(|o| *o.surface.layer.wl_surface() == s) && self.lock.resume_guard {
                self.lock.resume_guard = false;
                self.lock.dirty = true;
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
            let surface = Surface::new("lock", ls, &self.shm, self.started);
            self.lock.outs.push(Out { output, name, surface, view: View::new(1, 1) });
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
        let size = crate::surfaces::login::avatar_size(&self.store.theme.theme) as u32;
        let picture = self.store.wallpaper.latest().cloned();
        let avatar_tx = tx.clone();
        rt.pool().submit(move || {
            let _ = avatar_tx.send(LockMsg::Avatar(view::avatar(&path, size.max(1))));
        });
        if let Some(p) = picture {
            rt.pool().submit(move || {
                let _ = tx.send(LockMsg::Backdrop(view::backdrop(&p)));
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
                self.lock.avatar = a;
                self.lock.dirty = true;
            }
            LockMsg::PolkitAvatar(a) => {
                if let Some(d) = &mut self.polkit {
                    d.set_avatar(a);
                }
            }
            LockMsg::Art(url, art) => {
                self.lock.art = Some((url, art));
                self.lock.dirty = true;
            }
            LockMsg::Backdrop(b) => {
                self.lock.backdrop = Some(b);
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
            LockMsg::ActionEnded { action, at, cursor } => self.hot_corner_ended(&action, at, cursor),
            LockMsg::Polkit(event) => self.polkit_event(event),
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
            Outcome::Success => self.unlock(),
            Outcome::Failed(_) => self.lock.error = "Wrong password".into(),
            Outcome::MaxTries(_) => self.lock.error = "Account locked".into(),
            Outcome::StartFailed(why) | Outcome::Error(why) => {
                eprintln!("lock: pam: {why}");
                self.lock.error = "PAM error".into();
            }
        }
    }

    /// PAM's success is the one caller.
    fn unlock(&mut self) {
        if let Some(session) = self.lock.session.take() {
            session.unlock();
        }
        self.lock.outs.clear();
        if let Some(w) = self.lock.idle_watch.take() {
            w.destroy();
        }
        self.hot_corner_action_ended("lock");
        self.lock.locked = false;
        self.sync_herdr();
        self.lock.secure = false;
        self.lock.password = field();
        self.lock.error.clear();
        if let (Some(handle), Some(t)) = (&self.handle, self.lock.clock.take()) {
            handle.remove(t);
        }
    }

    /// What the now-playing block shows, if it shows.
    fn lock_media(&self) -> Option<crate::services::media::Active> {
        self.store.media.active().filter(|a| a.kind != "stream")
    }

    /// The transport's keys first (Lock/model.js's `transportKey` through
    /// the field's own key filter), then the field's.
    fn transport_key(&mut self, event: &KeyEvent) -> bool {
        let count = if self.lock_media().is_some() { view::TRANSPORT as i32 } else { 0 };
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
        if step.press {
            match step.index {
                0 => self.store.media.previous(),
                1 => self.store.media.play_pause(),
                _ => self.store.media.next(),
            }
        }
        step.taken
    }

    /// Redraws for what the lock shows; a new wallpaper is decoded again.
    pub fn lock_changed(&mut self, topic: Topic) {
        if self.lock.session.is_none() {
            return;
        }
        match topic {
            Topic::Wallpaper => {
                self.lock.backdrop = None;
                self.load_lock_pictures();
            }
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
        self.lock.art = Some((url.clone(), None));
        if !url.starts_with("file://") && !url.starts_with("data:") {
            return;
        }
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        let size = (self.store.theme.theme.space.control_height * 2.0).round() as u32;
        rt.pool().submit(move || {
            let art = view::cover(&url, size.max(1));
            let _ = tx.send(LockMsg::Art(url, art));
        });
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
    pub(super) fn lock_key(&mut self, event: &KeyEvent) -> bool {
        if self.lock.session.is_none() {
            return false;
        }
        if self.lock.resume_guard {
            self.lock.resume_guard = false;
        }
        if self.transport_key(event) {
            self.lock.dirty = true;
            return true;
        }
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter => self.submit_password(),
            Keysym::Escape => self.lock.password = field(),
            // A secret field hands nothing out, so a paste is all that can
            // come back from it.
            sym if !self.lock.checking => {
                let out = self.lock.password.key(super::field_key(sym), self.mods, event.utf8.as_deref());
                if out == fs_menu::field::Outcome::Paste {
                    clipboard::command(clipboard::Cmd::Paste);
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
            self.lock.dirty = true;
        }
    }

    pub(super) fn present_lock(&mut self) {
        if self.lock.outs.is_empty() || !self.lock.dirty {
            return;
        }
        self.lock.dirty = false;
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
        let entering = self.lock.fade.running(now) || self.lock.rise.running(now) || self.lock.wake.running(now);
        let has_wallpaper = !self.store.state.data.wallpaper.is_empty();
        let shared = Shared {
            now: Local::now(),
            name: crate::greeter::users::own_name(),
            prompt: view::Prompt::password(self.lock.password.text().chars().count(), self.lock.password.caret_chars(), self.lock.checking),
            error: &self.lock.error,
            has_wallpaper,
            backdrop: self.lock.backdrop.as_ref(),
            picture: self.store.wallpaper.latest().filter(|p| has_wallpaper && p.path == self.store.state.data.wallpaper),
            avatar: self.lock.avatar.as_ref(),
            media: None,
            enter: (self.lock.fade.value(now).clamp(0.0, 1.0) as f32, self.lock.rise.value(now)),
            blanked,
            wake: self.lock.wake.value(now).clamp(0.0, 1.0) as f32,
        };
        let active = self.lock_media();
        let mpris = active.as_ref().and_then(|a| self.store.media.mpris.iter().find(|p| p.id == a.id));
        let art = self.lock.art.as_ref().and_then(|(u, b)| (active.as_ref().is_some_and(|a| a.art_url == *u)).then_some(b.as_ref()).flatten());
        let shared = Shared {
            media: active.as_ref().map(|a| NowPlaying {
                title: &a.title,
                artist: &a.artist,
                playing: a.playing,
                can_previous: mpris.is_none_or(|p| p.can_previous),
                can_toggle: mpris.is_none_or(|p| p.can_toggle),
                can_next: mpris.is_none_or(|p| p.can_next),
                progress: (a.length > 0.0).then(|| a.position / a.length),
                art,
                cursor: self.lock.transport,
            }),
            ..shared
        };
        let qh = self.qh.clone();
        for o in &mut self.lock.outs {
            if !o.surface.configured {
                continue;
            }
            let moving = o.view.draw(&shared, theme, &mut self.bar.kit, now) || entering;
            if !o.name.is_empty() && !blanked {
                self.lock.reports.insert(o.name.clone(), o.view.report.clone());
            }
            o.surface.present(&mut o.view.scene, moving, &qh);
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
            })
        };
        let s = &self.lock.sleep;
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
        eprintln!("lock: secure");
    }

    /// The compositor refused the lock or ended it (another locker holds
    /// the session): nothing of ours is on screen.
    fn finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: SessionLock) {
        eprintln!("lock: finished by the compositor");
        self.lock.session = None;
        self.lock.outs.clear();
        self.lock.locked = false;
        self.sync_herdr();
        self.lock.secure = false;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: SessionLockSurface, configure: SessionLockSurfaceConfigure, _: u32) {
        let (w, h) = (configure.new_size.0 as i32, configure.new_size.1 as i32);
        let Some(o) = self.lock.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == surface.wl_surface()) else { return };
        o.view.resize(w, h);
        o.surface.configure(w, h);
        self.lock.dirty = true;
    }
}
