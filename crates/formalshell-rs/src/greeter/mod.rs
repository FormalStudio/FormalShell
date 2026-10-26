//! `formalshell-rs greeter`: greetd runs it as its
//! default_session inside a compositor of its own with no other client, so
//! there is no session lock here, only one overlay layer surface per output
//! drawing the login card (`surfaces::login`) over the wallpaper.
//!
//! Every output draws the same conversation; only the first output's
//! surface takes the keyboard, so there is one field to type into.
//!
//! Nothing here reads state.json: it would be the `greeter` system user's.
//! The backdrop is `wallpaper` in the greeter's state dir, a copy the
//! NixOS module mirrors from a user's own, else `greeter.wallpaper`. The
//! theme is settings.json's preset over theme.json's palette, each falling
//! back the way the shell's own do when absent.
//!
//! The user is picked, never typed: the last one to log in from [`users`],
//! with a conversation opened for them at once, so the field asks for a
//! password. With no users found the field asks for a username instead.
//! The session is the card's chip (a click, or Tab and Shift+Tab) from
//! [`sessions`]; with none found it is `greeter.sessionCommand`.

pub mod greetd;
pub mod sessions;
pub mod users;

use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use calloop::channel::{self, Event};
use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, LoopHandle};
use chrono::{Local, Timelike};
use fs_theme::theme::Theme;
use serde_json::Value;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, repeat::RepeatCallback};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use fs_menu::field::Field;
use zeroize::Zeroizing;

use crate::render::Renderer;
use crate::scene::{Bitmap, IRect};
use crate::services::theme;
use crate::services::wallpaper;
use crate::surface::Surface;
use crate::surfaces::bar::cell::Kit;
use crate::surfaces::lock;
use crate::ui::{Ui, What};
use greetd::{Conversation, Phase, Request, Response, Step};
use sessions::Session;
use users::User;
use crate::surfaces::login::{self, Card, Controls, Menu, View};

/// AuthPrompt's `unavailableText` with no socket to talk to.
const NO_SOCKET: &str = "No greetd socket to talk to";

struct Out {
    output: wl_output::WlOutput,
    surface: Surface<LayerSurface>,
    view: View,
    /// The wallpaper at this output's size, and the size it was asked at.
    backdrop: Option<Bitmap>,
    asked: (i32, i32),
    /// The boot's dice and grid at this output's size.
    stage: Option<(crate::boot::Plan, Vec<(f64, f64, f64, f64)>)>,
}

/// The table's boot played behind the card (`crate::boot::greet`): the
/// emblem assembles on black, moves up as the wallpaper and the card come
/// in, and crumples into black once the session is on its way.
struct Boot {
    emblem: crate::surfaces::boot::Emblem,
    /// The clock, from the first output's configure.
    start: Option<Instant>,
    /// When the emblem started up over the card, and the way-in time it
    /// holds: on its own once assembled, or at the first key.
    settle: Option<(Instant, f64)>,
    out: Option<Instant>,
}

/// A backdrop decoded off the loop.
struct Decoded {
    size: (i32, i32),
    bitmap: Bitmap,
}

struct Greeter {
    registry: RegistryState,
    outputs: OutputState,
    seats: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    qh: QueueHandle<Self>,
    handle: LoopHandle<'static, Greeter>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    cursor_shapes: Option<CursorShapeManager>,
    cursor_device: Option<WpCursorShapeDeviceV1>,
    mods: fs_menu::nav::Modifiers,
    outs: Vec<Out>,
    theme: Theme,
    kit: Kit,
    convo: Conversation,
    field: Field<Zeroizing<String>>,
    /// The worker's end of the socket; None with no greetd to talk to.
    greetd: Option<mpsc::Sender<Request>>,
    sessions: Vec<Session>,
    names: Vec<String>,
    session: usize,
    /// Picked by hand, so a user change does not bring back their own.
    picked: bool,
    users: Vec<User>,
    user_names: Vec<String>,
    user: usize,
    avatar: Vec<Bitmap>,
    menu: Menu,
    /// The conversation reached a question, so its end opens the next one.
    asked: bool,
    /// A user change waits on the old conversation's cancel.
    reopen: bool,
    state_dir: PathBuf,
    wallpaper: Option<String>,
    decoded: channel::Sender<Decoded>,
    dirty: bool,
    exit: bool,
    started: Instant,
    boot: Option<Boot>,
}

/// What the socket thread hands back.
enum Reply {
    Response(Response),
    Broken(String),
}

/// settings.json, `{}` when absent or unreadable.
fn settings(env: &theme::Env) -> Value {
    let path = env.config_dir.join("formalshell/settings.json");
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("greeter: {} unparsable: {e}", path.display());
            Value::Object(Default::default())
        }),
        Err(_) => Value::Object(Default::default()),
    }
}

fn resolve(settings: &Value, env: &theme::Env) -> Theme {
    let (palette, _) = theme::read_palette(&env.theme_json());
    Theme::resolve(theme::get(settings), &palette)
}

/// `greeter.sessionCommand`: a list of non-empty strings, else `["Hyprland"]`.
fn session_command(settings: &Value) -> Vec<String> {
    let argv: Option<Vec<String>> = theme::get(settings)("greeter.sessionCommand")
        .and_then(|v| v.as_array().cloned())
        .and_then(|items| items.iter().map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)).collect());
    argv.filter(|a| !a.is_empty()).unwrap_or_else(|| vec!["Hyprland".into()])
}

/// `greeter.sessionDirs`, else the NixOS profile's and the FHS one.
fn session_dirs(settings: &Value) -> Vec<PathBuf> {
    let dirs: Option<Vec<PathBuf>> = theme::get(settings)("greeter.sessionDirs").and_then(|v| v.as_array().cloned()).map(|items| items.iter().filter_map(|v| v.as_str()).map(PathBuf::from).collect());
    dirs.filter(|d| !d.is_empty()).unwrap_or_else(|| sessions::DIRS.iter().map(PathBuf::from).collect())
}

/// The desktop's source as this login screen can know it: `wallpaper.color`
/// or the preset's colour, else the mirrored `wallpaper` in the state dir,
/// else `greeter.wallpaper`. The user's own picker choice lives in a
/// state.json the greeter never reads.
fn wallpaper_source(settings: &Value, env: &theme::Env) -> Option<String> {
    let get = theme::get(settings);
    let color = fs_theme::presets::resolve(get("theme.preset").as_ref(), &get).wallpaper_color;
    let mirrored = env.state_dir.join("wallpaper");
    let picture = if mirrored.is_file() {
        Some(mirrored.to_string_lossy().into_owned())
    } else {
        get("greeter.wallpaper").and_then(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned))
    };
    let source = wallpaper::source(color.as_deref(), &Value::Null, picture.as_deref().unwrap_or(""));
    (!source.is_empty()).then_some(source)
}

/// The wallpaper cover-cropped to `size`, as the desktop shows it: the card
/// carries the contrast, so there is no scrim.
fn decode(path: &str, size: (i32, i32)) -> Option<Decoded> {
    let p = wallpaper::render(path, size.0 as u32, size.1 as u32, None, None)?;
    let rgba = p.bgra.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0], 255]).collect();
    Some(Decoded { size, bitmap: Bitmap::from_rgba(p.width as u16, p.height as u16, rgba) })
}

/// The avatar the card draws for `login`, at each scale an output draws at.
fn avatar(theme: &Theme, state: &Path, login: &str) -> Vec<Bitmap> {
    let size = login::avatar_size(theme);
    let Some(path) = users::avatar_path(state, login) else { return Vec::new() };
    crate::surface::output_scales()
        .into_iter()
        .filter_map(|k| lock::avatar(&path, crate::scene::pixels_at(size, k)).map(|b| b.at(k)))
        .collect()
}

/// Which power action a control fires, by logind method.
fn power(on: &str) -> Option<&'static str> {
    match on {
        "suspend" => Some("Suspend"),
        "reboot" => Some("Reboot"),
        "poweroff" => Some("PowerOff"),
        _ => None,
    }
}

/// The socket on a thread of its own: one request out, one response back.
fn connect(replies: channel::Sender<Reply>) -> Option<mpsc::Sender<Request>> {
    let path = std::env::var_os("GREETD_SOCK").filter(|p| !p.is_empty())?;
    let mut stream = match UnixStream::connect(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("greetd: cannot connect to {}: {e}", Path::new(&path).display());
            return None;
        }
    };
    eprintln!("greetd: connected to {}", Path::new(&path).display());
    let (tx, rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("fs-greetd".into())
        .spawn(move || {
            for req in rx {
                eprintln!("greetd: -> {}", req.name());
                let reply = greetd::write(&mut stream, &req).and_then(|()| greetd::read(&mut stream));
                drop(req);
                let reply = match reply {
                    Ok((resp, raw)) => {
                        eprintln!("greetd: <- {raw}");
                        Reply::Response(resp)
                    }
                    Err(e) => {
                        eprintln!("greetd: socket: {e}");
                        Reply::Broken(e.to_string())
                    }
                };
                if replies.send(reply).is_err() {
                    break;
                }
            }
        })
        .ok()?;
    Some(tx)
}

pub fn main(args: &[String]) {
    let env = theme::Env::from_process();
    let settings = settings(&env);
    if let Some(dir) = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)) {
        shots(Path::new(dir), &settings, &env);
        return;
    }
    let started = Instant::now();
    let conn = Connection::connect_to_env().expect("no Wayland compositor to connect to");
    let (globals, queue) = registry_queue_init::<Greeter>(&conn).expect("wl_registry");
    let qh = queue.handle();
    let mut event_loop: EventLoop<'static, Greeter> = EventLoop::try_new().expect("event loop");
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).expect("wayland source");

    let (replies, inbox) = channel::channel::<Reply>();
    handle
        .insert_source(inbox, |event, _, g| {
            if let Event::Msg(reply) = event {
                g.reply(reply);
            }
        })
        .expect("greetd channel");

    let (decoded, decodes) = channel::channel::<Decoded>();
    handle
        .insert_source(decodes, |event, _, g| {
            if let Event::Msg(d) = event {
                g.backdrop(d);
            }
        })
        .expect("backdrop channel");

    let theme = resolve(&settings, &env);
    eprintln!("greeter: preset {}", theme.preset);
    let sessions = sessions::scan(&session_dirs(&settings));
    let users = users::scan();
    eprintln!("greeter: {} sessions, {} users", sessions.len(), users.len());
    crate::surface::bind_scaling(&globals, &qh);
    let mut g = Greeter {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seats: SeatState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).expect("wl_compositor is not available"),
        layer_shell: LayerShell::bind(&globals, &qh).expect("zwlr_layer_shell_v1 is not available"),
        shm: Shm::bind(&globals, &qh).expect("wl_shm is not available"),
        qh: qh.clone(),
        handle: handle.clone(),
        keyboard: None,
        pointer: None,
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        cursor_device: None,
        mods: fs_menu::nav::Modifiers::default(),
        outs: Vec::new(),
        kit: Kit::new(&theme),
        theme,
        convo: Conversation::new(session_command(&settings)),
        field: field(),
        greetd: connect(replies),
        names: sessions.iter().map(|s| s.name.clone()).collect(),
        sessions,
        session: 0,
        picked: false,
        user_names: users.iter().map(|u| u.name.clone()).collect(),
        users,
        user: 0,
        avatar: Vec::new(),
        menu: Menu::Closed,
        asked: false,
        reopen: false,
        wallpaper: wallpaper_source(&settings, &env),
        state_dir: env.state_dir.clone(),
        decoded,
        dirty: true,
        exit: false,
        started,
        boot: None,
    };
    if g.theme.boot && g.theme.motion_enabled {
        let host = crate::boot::hostname();
        let emblem = crate::surfaces::boot::Emblem::new(&mut g.kit, &g.theme, [host.as_str(), "FORMAL"]);
        g.boot = Some(Boot { emblem, start: None, settle: None, out: None });
    }
    let last = sessions::remembered(&g.state_dir, None);
    g.select(sessions::pick(&g.sessions, last.as_deref()));
    let last_user = sessions::last_user(&g.state_dir);
    g.user = g.users.iter().position(|u| Some(&u.login) == last_user.as_ref()).unwrap_or(0);
    g.open_user();
    g.arm_clock();

    let signal = event_loop.get_signal();
    let ended = event_loop.run(None, &mut g, |g| {
        g.present();
        if g.exit {
            signal.stop();
        }
    });
    if let Err(err) = ended {
        eprintln!("greeter: event loop: {err}");
    }
    eprintln!("greeter: exiting after {}ms", g.started.elapsed().as_millis());
}

/// A typed secret's buffer, sized so typing never reallocates.
fn buffer() -> Zeroizing<String> {
    Zeroizing::new(String::with_capacity(256))
}

/// Masked or not, nothing is copied out of the greeter's field.
fn field() -> Field<Zeroizing<String>> {
    Field::new(buffer()).secret()
}

impl Greeter {
    fn reply(&mut self, reply: Reply) {
        let step = match reply {
            Reply::Response(resp) => self.convo.receive(resp),
            Reply::Broken(why) => {
                self.convo.broken(&why);
                self.greetd = None;
                Step::Nothing
            }
        };
        self.step(step);
        match self.convo.phase {
            Phase::Asking => self.asked = true,
            Phase::Inactive if self.asked || self.reopen => {
                self.asked = false;
                self.reopen = false;
                self.open_user();
            }
            _ => {}
        }
        self.dirty = true;
    }

    /// A conversation for the picked user, the field then asking for a
    /// password. Their own last session comes back unless one was picked.
    fn open_user(&mut self) {
        let Some(login) = self.users.get(self.user).map(|u| u.login.clone()) else { return };
        self.avatar = avatar(&self.theme, &self.state_dir, &login);
        if !self.picked
            && let Some(own) = sessions::remembered(&self.state_dir, Some(&login))
        {
            self.select(sessions::pick(&self.sessions, Some(&own)));
        }
        if self.greetd.is_some() {
            let step = self.convo.open(&login);
            self.step(step);
        }
        self.dirty = true;
    }

    fn pick_user(&mut self, i: usize) {
        if i == self.user || i >= self.users.len() {
            return;
        }
        self.user = i;
        self.field = field();
        self.convo.error.clear();
        match self.convo.phase {
            Phase::Inactive => self.open_user(),
            _ => {
                self.reopen = true;
                let step = self.convo.cancel();
                self.step(step);
            }
        }
    }

    /// A card control, clicked.
    fn activate(&mut self, on: &str, what: What) {
        if let Some(method) = power(on) {
            let _ = std::thread::Builder::new().name("fs-power".into()).spawn(move || {
                if let Err(e) = futures_lite::future::block_on(crate::services::power::call(method)) {
                    eprintln!("greeter: {method}: {e}");
                }
            });
            return;
        }
        let toggle = |m: Menu, now: Menu| if now == m { Menu::Closed } else { m };
        match (on, what) {
            ("sessions", _) => self.menu = toggle(Menu::Sessions, self.menu),
            ("users", _) => self.menu = toggle(Menu::Users, self.menu),
            _ => {
                let Some((kind, i)) = on.split_once(':').and_then(|(k, i)| Some((k, i.parse::<usize>().ok()?))) else { return };
                match kind {
                    "session" => self.pick(i),
                    "user" => self.pick_user(i),
                    _ => return,
                }
                self.menu = Menu::Closed;
            }
        }
        self.dirty = true;
    }

    fn step(&mut self, step: Step) {
        match step {
            Step::Send(req) => {
                if self.greetd.as_ref().is_none_or(|tx| tx.send(req).is_err()) {
                    self.convo.broken("greetd went away");
                }
            }
            Step::Exit => {
                eprintln!("greeter: session started");
                if let Some(s) = self.sessions.get(self.session) {
                    sessions::remember(&self.state_dir, &self.convo.user, &s.id);
                }
                sessions::remember_user(&self.state_dir, &self.convo.user);
                // The emblem crumples into black first; the session lifts it.
                match &mut self.boot {
                    Some(b) => {
                        eprintln!("greeter: boot crumpling");
                        b.settle.get_or_insert((Instant::now(), 0.0));
                        b.out = Some(Instant::now());
                        self.dirty = true;
                    }
                    None => self.exit = true,
                }
            }
            Step::Nothing => {}
        }
    }

    fn key(&mut self, event: &KeyEvent) {
        // A key while the emblem assembles brings the card up at once, and
        // still reaches the field.
        if let Some(b) = self.boot.as_mut().filter(|b| b.settle.is_none()) {
            let held = self.outs.iter().filter_map(|o| o.stage.as_ref()).map(|(p, _)| p.assembled()).fold(0.0, f64::max);
            let t = b.start.map_or(0, |s| s.elapsed().as_millis());
            eprintln!("greeter: boot skipped by a key t={t}ms");
            b.settle = Some((Instant::now(), held));
            self.dirty = true;
        }
        let enabled = self.greetd.is_some() && self.convo.input_enabled();
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter if enabled && self.convo.phase == Phase::Inactive && !self.users.is_empty() => self.open_user(),
            Keysym::Return | Keysym::KP_Enter if enabled => {
                let text = self.field.replace(buffer());
                if self.convo.phase == Phase::Inactive && !self.picked {
                    let own = sessions::remembered(&self.state_dir, Some(text.as_str()));
                    if own.is_some() {
                        self.select(sessions::pick(&self.sessions, own.as_deref()));
                    }
                }
                let step = self.convo.submit(text);
                self.step(step);
            }
            Keysym::Tab | Keysym::ISO_Left_Tab if self.sessions.len() > 1 => {
                let n = self.sessions.len();
                let back = event.keysym == Keysym::ISO_Left_Tab || self.mods.shift;
                self.pick((self.session + if back { n - 1 } else { 1 }) % n);
            }
            Keysym::Escape if self.menu != Menu::Closed => self.menu = Menu::Closed,
            Keysym::Escape => self.field = field(),
            sym if enabled => {
                if self.field.key(crate::wayland::field_key(sym), self.mods, event.utf8.as_deref()) == fs_menu::field::Outcome::Pass {
                    return;
                }
            }
            _ => return,
        }
        self.dirty = true;
    }

    /// The session a login starts from here on.
    fn select(&mut self, i: usize) {
        let Some(s) = self.sessions.get(i) else { return };
        self.session = i;
        self.convo.command = s.command();
        self.convo.env = s.env();
        if self.boot.is_some() {
            self.convo.env.push(format!("{}={}", crate::boot::HANDOFF_ENV, crate::boot::HANDOFF));
        }
        self.dirty = true;
    }

    /// A pick by hand, which only counts before the session starts.
    fn pick(&mut self, i: usize) {
        if self.convo.phase != Phase::Launched {
            self.picked = true;
            self.select(i);
        }
    }

    /// The wallpaper decoded at each output's size in its own pixels, again
    /// when the size or the scale moved.
    fn ask_backdrops(&mut self) {
        let Some(path) = self.wallpaper.clone() else { return };
        for o in self.outs.iter_mut().filter(|o| o.surface.configured) {
            let (size, k) = (o.view.scene.size, o.surface.scale());
            let want = (crate::render::scaled(size.w, k), crate::render::scaled(size.h, k));
            if o.asked == want {
                continue;
            }
            o.asked = want;
            let tx = self.decoded.clone();
            let path = path.clone();
            let _ = std::thread::Builder::new().name("fs-backdrop".into()).spawn(move || match decode(&path, want) {
                Some(d) => {
                    let _ = tx.send(d);
                }
                None => eprintln!("greeter: cannot decode {path}"),
            });
        }
    }

    fn backdrop(&mut self, d: Decoded) {
        if let Some(o) = self.outs.iter_mut().find(|o| o.asked == d.size) {
            o.backdrop = Some(d.bitmap);
            self.dirty = true;
        }
    }

    /// The clock redraws on the minute.
    fn arm_clock(&mut self) {
        let now = Local::now();
        let at = Instant::now() + Duration::from_secs(60 - now.second() as u64) - Duration::from_nanos(now.nanosecond() as u64 % 1_000_000_000);
        let _ = self.handle.insert_source(Timer::from_deadline(at), |_, _, g: &mut Greeter| {
            g.dirty = true;
            g.arm_clock();
            TimeoutAction::Drop
        });
    }

    fn on_outputs(&mut self) {
        crate::surface::note_outputs(&self.outputs, None);
        let outputs: Vec<wl_output::WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            if self.outs.iter().any(|o| o.output == output) {
                continue;
            }
            let first = self.outs.is_empty();
            let wl = self.compositor.create_surface(&self.qh);
            let ls = self.layer_shell.create_layer_surface(&self.qh, wl, Layer::Overlay, Some("formalshell:greeter"), Some(&output));
            ls.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            ls.set_exclusive_zone(-1);
            ls.set_keyboard_interactivity(if first { KeyboardInteractivity::Exclusive } else { KeyboardInteractivity::None });
            ls.commit();
            let surface = Surface::on("greeter", ls, &self.shm, self.started, &output);
            self.outs.push(Out { output, surface, view: View::new(1, 1), backdrop: None, asked: (0, 0), stage: None });
        }
    }

    fn present(&mut self) {
        if !self.dirty || self.outs.is_empty() {
            return;
        }
        self.dirty = false;
        self.ask_backdrops();
        let now = Instant::now();
        let ms = |at: Instant| now.saturating_duration_since(at).as_secs_f64() * 1000.0;
        let assembled = self.outs.iter().filter_map(|o| o.stage.as_ref()).map(|(p, _)| p.assembled()).fold(0.0, f64::max);
        if let Some(b) = &mut self.boot
            && let Some(start) = b.start
            && b.settle.is_none()
            && ms(start) >= assembled
        {
            eprintln!("greeter: boot assembled t={}ms", assembled.round());
            b.settle = Some((now, assembled));
        }
        let timing = self.boot.as_ref().map(|b| {
            let t = match (b.settle, b.start) {
                (Some((_, held)), _) => held,
                (None, Some(start)) => ms(start),
                (None, None) => 0.0,
            };
            let out = b.out.map(ms);
            (t, out, crate::boot::greet(b.settle.map(|(at, _)| ms(at)), out))
        });
        if timing.is_some_and(|(_, out, g)| out.is_some() && !g.moving) {
            self.exit = true;
        }
        let controls = Controls { sessions: &self.names, session: self.session, users: &self.user_names, user: self.user, menu: self.menu };
        let base = card(&self.convo, &self.field, self.greetd.is_some(), self.users.is_empty());
        // The prompt types itself in as the card arrives.
        let placeholder = match timing {
            Some((_, _, g)) => crate::effects::prefix(base.placeholder, crate::effects::typed(base.placeholder.chars().count(), g.typed)),
            None => base.placeholder,
        };
        let mut card = Card {
            name: self.users.get(self.user).map_or("", |u| u.name.as_str()),
            avatar: None,
            below: Some(controls.el(&self.theme)),
            placeholder,
            ..base
        };
        let empty = crate::surfaces::boot::Emblem::empty();
        for o in &mut self.outs {
            if !o.surface.configured {
                continue;
            }
            let k = o.surface.scale();
            card.avatar = crate::scene::nearest(&self.avatar, k);
            let _raster = crate::scene::raster_at(k);
            let stage = match (timing, &o.stage) {
                (Some((t, out, greet)), Some((plan, lines))) => Some(login::Stage {
                    plan,
                    lines,
                    emblem: self.boot.as_ref().map_or(&empty, |b| &b.emblem),
                    t,
                    out,
                    greet,
                }),
                _ => None,
            };
            let moving = o.view.draw(&card, o.backdrop.as_ref(), &self.theme, &mut self.kit, now, stage.as_ref());
            o.surface.present(&mut o.view.scene, moving || timing.is_some_and(|(_, _, g)| g.moving), &self.qh);
        }
    }
}

/// The card's conversation half, as it stands; the caller fills in who and
/// which session.
fn card<'a>(convo: &'a Conversation, field: &Field<Zeroizing<String>>, available: bool, typed_user: bool) -> Card<'a> {
    let masked = convo.masked();
    let text = if masked { "\u{2022}".repeat(field.text().chars().count()) } else { field.text().to_owned() };
    let checking = convo.phase == Phase::Waiting && !convo.user.is_empty();
    let placeholder = match convo.phase {
        Phase::Inactive if typed_user => "Username",
        Phase::Waiting | Phase::Launched if checking => "Signing in\u{2026}",
        _ => "Password",
    };
    // PAM's own "Password:" says what the placeholder already does.
    let label = match convo.phase {
        Phase::Asking if !convo.message.trim_end().trim_end_matches(':').eq_ignore_ascii_case("password") => convo.label(),
        _ => "",
    };
    Card {
        now: Local::now(),
        name: "",
        avatar: None,
        text,
        placeholder,
        label,
        error: &convo.error,
        unavailable: if available { "" } else { NO_SOCKET },
        enabled: available && convo.input_enabled(),
        busy: checking,
        caret: field.caret_chars(),
        below: None,
    }
}

/// `--shot <dir>`: the greeter's states under every preset, drawn off
/// screen to `greeter-<preset>-<state>.png`, for reading the look without
/// a greetd. Nothing is sent anywhere.
fn shots(dir: &Path, settings: &Value, env: &theme::Env) {
    let (w, h) = (1280, 800);
    std::fs::create_dir_all(dir).expect("shot directory");
    let backdrop = wallpaper_source(settings, env).and_then(|p| decode(&p, (w, h)));
    let names: Vec<String> = ["Hyprland (uwsm-managed)", "Plasma (Wayland)", "Waylandcraft"].map(String::from).to_vec();
    let people: Vec<String> = ["Kyan De Sutter", "Guest"].map(String::from).to_vec();
    for preset in fs_theme::presets::NAMES {
        let mut s = settings.clone();
        if let Some(obj) = s.as_object_mut() {
            let theme_obj = obj.entry("theme").or_insert_with(|| Value::Object(Default::default()));
            if let Some(t) = theme_obj.as_object_mut() {
                t.insert("preset".into(), Value::from(preset));
            }
        }
        let theme = resolve(&s, env);
        let mut kit = Kit::new(&theme);
        let pic = avatar(&theme, &env.state_dir, "shot");
        let mut asking = Conversation::new(vec![]);
        asking.open("kyan");
        asking.receive(Response::AuthMessage { auth_message_type: greetd::MessageType::Secret, auth_message: "Password: ".into() });
        let mut failed = Conversation::new(vec![]);
        failed.open("kyan");
        failed.receive(Response::AuthMessage { auth_message_type: greetd::MessageType::Secret, auth_message: "Password: ".into() });
        failed.submit(Zeroizing::new("nope".into()));
        failed.receive(Response::Error { error_type: greetd::ErrorType::AuthError, description: "Wrong password".into() });
        failed.receive(Response::Success);
        failed.open("kyan");
        failed.receive(Response::AuthMessage { auth_message_type: greetd::MessageType::Secret, auth_message: "Password: ".into() });
        let states: [(&str, &Conversation, &str, Menu, bool); 5] = [
            ("password", &asking, "", Menu::Closed, true),
            ("typing", &asking, "hunter2", Menu::Closed, true),
            ("failed", &failed, "", Menu::Closed, true),
            ("sessions", &asking, "", Menu::Sessions, true),
            ("unavailable", &asking, "", Menu::Closed, false),
        ];
        for (name, convo, typed, menu, available) in states {
            let mut view = View::new(w, h);
            let controls = Controls { sessions: &names, session: 0, users: &people, user: 0, menu };
            let c = Card {
                name: "Kyan De Sutter",
                avatar: crate::scene::nearest(&pic, 1.0),
                below: Some(controls.el(&theme)),
                ..card(convo, &Field::new(Zeroizing::new(typed.to_owned())), available, false)
            };
            view.draw(&c, backdrop.as_ref().map(|d| &d.bitmap), &theme, &mut kit, Instant::now(), None);
            let mut r = Renderer::new(w as u16, h as u16);
            r.render(&view.scene, IRect::new(0, 0, w, h));
            let rgba: Vec<u8> = r.canvas().data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
            let path = dir.join(format!("greeter-{preset}-{name}.png"));
            image::save_buffer(&path, &rgba, w as u32, h as u32, image::ExtendedColorType::Rgba8).expect("png");
            println!("SHOT {}", path.display());
        }
        // The boot behind the card, at moments through the way in, the
        // emblem moving up, and the session's way out.
        if theme.boot {
            let host = crate::boot::hostname();
            let emblem = crate::surfaces::boot::Emblem::new(&mut kit, &theme, [host.as_str(), "FORMAL"]);
            let lines = crate::surfaces::boot::grid_lines(&theme, (w, h));
            let plan = crate::boot::Plan::new(&crate::surfaces::boot::columns(&lines, (w, h)), &emblem.counts(), crate::boot::SEED);
            let done = plan.assembled();
            let moments: [(&str, f64, Option<f64>, Option<f64>); 8] = [
                ("0300", 300.0, None, None),
                ("0700", 700.0, None, None),
                ("1100", 1100.0, None, None),
                ("1500", 1500.0, None, None),
                ("2000", 2000.0, None, None),
                ("settle", done, Some(450.0), None),
                ("rest", done, Some(3000.0), None),
                ("out", done, Some(3000.0), Some(250.0)),
            ];
            for (name, t, settled, out) in moments {
                let greet = crate::boot::greet(settled, out);
                let base = card(&asking, &Field::new(Zeroizing::new(String::new())), true, false);
                let placeholder = crate::effects::prefix(base.placeholder, crate::effects::typed(base.placeholder.chars().count(), greet.typed));
                let controls = Controls { sessions: &names, session: 0, users: &people, user: 0, menu: Menu::Closed };
                let c = Card { name: "Kyan De Sutter", avatar: crate::scene::nearest(&pic, 1.0), below: Some(controls.el(&theme)), placeholder, ..base };
                let stage = login::Stage { plan: &plan, lines: &lines, emblem: &emblem, t, out, greet };
                let mut view = View::new(w, h);
                view.draw(&c, backdrop.as_ref().map(|d| &d.bitmap), &theme, &mut kit, Instant::now(), Some(&stage));
                let mut r = Renderer::new(w as u16, h as u16);
                r.render(&view.scene, IRect::new(0, 0, w, h));
                let rgba: Vec<u8> = r.canvas().data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
                let path = dir.join(format!("greeter-{preset}-boot-{name}.png"));
                image::save_buffer(&path, &rgba, w as u32, h as u32, image::ExtendedColorType::Rgba8).expect("png");
                println!("SHOT {}", path.display());
            }
        }
        // The lock screen's card beside the greeter's, locked and playing.
        for (name, playing) in [("lock", false), ("lock-media", true)] {
            let mut view = lock::View::new(w, h);
            let media = playing.then(|| lock::NowPlaying {
                title: "Teardrop",
                artist: "Massive Attack",
                playing: true,
                can_previous: true,
                can_toggle: true,
                can_next: true,
                progress: Some(0.4),
                art: None,
                cursor: -1,
            });
            let shared = lock::Shared {
                now: Local::now(),
                name: "Kyan De Sutter",
                prompt: lock::Prompt::password(0, (0, 0), false),
                error: "",
                has_wallpaper: backdrop.is_some(),
                backdrop: backdrop.as_ref().map(|d| &d.bitmap),
                foreground: None,
                shards: None,
                picture: None,
                avatar: crate::scene::nearest(&pic, 1.0),
                media,
                enter: (1.0, 0.0),
                slam: 1.0,
                blanked: false,
                wake: 1.0,
            };
            view.draw(&shared, &theme, &mut kit, Instant::now());
            let mut r = Renderer::new(w as u16, h as u16);
            r.render(&view.scene, IRect::new(0, 0, w, h));
            let rgba: Vec<u8> = r.canvas().data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
            let path = dir.join(format!("greeter-{preset}-{name}.png"));
            image::save_buffer(&path, &rgba, w as u32, h as u32, image::ExtendedColorType::Rgba8).expect("png");
            println!("SHOT {}", path.display());
        }
    }
}

impl crate::surface::ScaleHandler for Greeter {
    fn rescaled(&mut self) {
        self.dirty = true;
    }
}

impl CompositorHandler for Greeter {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {
        self.dirty = true;
    }

    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        if let Some(o) = self.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == surface) {
            o.surface.landed(Instant::now());
            o.surface.mapped = true;
            self.dirty = true;
        }
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl LayerShellHandler for Greeter {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.outs.retain(|o| o.surface.layer.wl_surface() != layer.wl_surface());
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let Some(o) = self.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == layer.wl_surface()) else { return };
        let (w, h) = (configure.new_size.0.max(1) as i32, configure.new_size.1.max(1) as i32);
        o.view.resize(w, h);
        o.surface.configure(w, h);
        if let Some(b) = &mut self.boot {
            let lines = crate::surfaces::boot::grid_lines(&self.theme, (w, h));
            let plan = crate::boot::Plan::new(&crate::surfaces::boot::columns(&lines, (w, h)), &b.emblem.counts(), crate::boot::SEED);
            o.stage = Some((plan, lines));
            if b.start.is_none() {
                eprintln!("greeter: boot playing");
                b.start = Some(Instant::now());
            }
        }
        self.ask_backdrops();
        self.dirty = true;
    }
}

impl SeatHandler for Greeter {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
            self.cursor_device = match (&self.cursor_shapes, &self.pointer) {
                (Some(m), Some(p)) => Some(m.get_shape_device(p, qh)),
                _ => None,
            };
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let repeat: RepeatCallback<Greeter> = Box::new(|g, _, event| g.key(&event));
            self.keyboard = self.seats.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), repeat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if let Some(p) = self.pointer.take().filter(|_| capability == Capability::Pointer) {
            p.release();
        }
        if let Some(k) = self.keyboard.take().filter(|_| capability == Capability::Keyboard) {
            k.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for Greeter {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        self.key(&event);
    }

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, m: Modifiers, _: RawModifiers, _: u32) {
        self.mods = fs_menu::nav::Modifiers { ctrl: m.ctrl, shift: m.shift, alt: m.alt };
    }
}

impl PointerHandler for Greeter {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for e in events {
            let Some(o) = self.outs.iter_mut().find(|o| o.surface.layer.wl_surface() == &e.surface) else { continue };
            let ui = &mut o.view.ui;
            let (x, y) = e.position;
            let path = ui.hit(x, y).map(|h| h.path.clone());
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(device) = &self.cursor_device {
                        device.set_shape(serial, Shape::Default);
                    }
                    ui.hover = path;
                }
                PointerEventKind::Motion { .. } => ui.hover = path,
                PointerEventKind::Leave { .. } => ui.hover = None,
                PointerEventKind::Press { button: 0x110, .. } => ui.pressed = path,
                PointerEventKind::Release { button: 0x110, .. } => {
                    let pressed = ui.pressed.take();
                    let event = ui.hit(x, y).filter(|_| pressed.is_some() && pressed == path).and_then(|h| Ui::event(h, x));
                    if let Some(e) = event {
                        self.activate(&e.on, e.what);
                    }
                }
                _ => continue,
            }
            self.dirty = true;
        }
    }
}

impl OutputHandler for Greeter {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.on_outputs();
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.outs.retain(|o| o.output != output);
    }
}

impl ShmHandler for Greeter {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Greeter {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Greeter);
delegate_dispatch2!(Greeter);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn session_command_falls_back_to_hyprland() {
        assert_eq!(session_command(&json!({})), ["Hyprland"]);
        assert_eq!(session_command(&json!({"greeter": {"sessionCommand": ["sway", "-d"]}})), ["sway", "-d"]);
        assert_eq!(session_command(&json!({"greeter": {"sessionCommand": ["sway", ""]}})), ["Hyprland"]);
        assert_eq!(session_command(&json!({"greeter": {"sessionCommand": []}})), ["Hyprland"]);
    }
}
