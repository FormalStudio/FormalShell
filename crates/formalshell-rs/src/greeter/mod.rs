//! `formalshell-rs greeter`: greetd runs it as its
//! default_session inside a compositor of its own with no other client, so
//! there is no session lock here, only one overlay layer surface per output
//! showing the lock screen's centre column over the flat background.
//!
//! Every output draws the same conversation; only the first output's
//! surface takes the keyboard, so there is one field to type into.
//!
//! Nothing here reads state.json: it would be the `greeter` system user's,
//! and the lock's wallpaper backdrop has no place before anyone logs in.
//! The theme is settings.json's preset over theme.json's palette, each
//! falling back the way the shell's own do when absent.

pub mod greetd;

use std::os::unix::net::UnixStream;
use std::path::Path;
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
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_output, wl_seat, wl_surface};
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
use crate::scene::IRect;
use crate::services::theme;
use crate::surface::Surface;
use crate::surfaces::bar::cell::Kit;
use crate::surfaces::lock::{Prompt, Shared, View};
use greetd::{Conversation, Request, Response, Step};

/// AuthPrompt's `unavailableText` with no socket to talk to.
const NO_SOCKET: &str = "No greetd socket to talk to";

struct Out {
    output: wl_output::WlOutput,
    surface: Surface<LayerSurface>,
    view: View,
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
    mods: fs_menu::nav::Modifiers,
    outs: Vec<Out>,
    theme: Theme,
    kit: Kit,
    convo: Conversation,
    field: Field<Zeroizing<String>>,
    /// The worker's end of the socket; None with no greetd to talk to.
    greetd: Option<mpsc::Sender<Request>>,
    dirty: bool,
    exit: bool,
    started: Instant,
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

    let theme = resolve(&settings, &env);
    eprintln!("greeter: preset {}", theme.preset);
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
        mods: fs_menu::nav::Modifiers::default(),
        outs: Vec::new(),
        kit: Kit::new(&theme),
        theme,
        convo: Conversation::new(session_command(&settings)),
        field: field(),
        greetd: connect(replies),
        dirty: true,
        exit: false,
        started,
    };
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
                self.exit = true;
            }
            Step::Nothing => {}
        }
    }

    fn key(&mut self, event: &KeyEvent) {
        let enabled = self.greetd.is_some() && self.convo.input_enabled();
        match event.keysym {
            Keysym::Return | Keysym::KP_Enter if enabled => {
                let text = self.field.replace(buffer());
                let step = self.convo.submit(text);
                self.step(step);
            }
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
            let surface = Surface::new("greeter", ls, &self.shm, self.started);
            self.outs.push(Out { output, surface, view: View::new(1, 1) });
        }
    }

    fn present(&mut self) {
        if !self.dirty || self.outs.is_empty() {
            return;
        }
        self.dirty = false;
        let now = Instant::now();
        let available = self.greetd.is_some();
        let shared = shared(&self.convo, &self.field, available);
        for o in &mut self.outs {
            if !o.surface.configured {
                continue;
            }
            let moving = o.view.draw(&shared, &self.theme, &mut self.kit, now);
            o.surface.present(&mut o.view.scene, moving, &self.qh);
        }
    }
}

/// The column every output draws, from the conversation as it stands.
fn shared<'a>(convo: &'a Conversation, field: &Field<Zeroizing<String>>, available: bool) -> Shared<'a> {
    let masked = convo.masked();
    let text = if masked { "\u{2022}".repeat(field.text().chars().count()) } else { field.text().to_owned() };
    Shared {
        now: Local::now(),
        prompt: Prompt {
            text,
            placeholder: if masked { "Password" } else { "Username" },
            label: convo.label(),
            unavailable: if available { "" } else { NO_SOCKET },
            enabled: available && convo.input_enabled(),
            busy: false,
            caret: field.caret_chars(),
        },
        error: &convo.error,
        has_wallpaper: false,
        backdrop: None,
        picture: None,
        avatar: None,
        media: None,
        enter: (1.0, 0.0),
        blanked: false,
        wake: 1.0,
        palette_ink: true,
    }
}

/// `--shot <dir>`: the greeter's states under every preset, drawn off
/// screen to `greeter-<preset>-<state>.png`, for reading the look without
/// a greetd. Nothing is sent anywhere.
fn shots(dir: &Path, settings: &Value, env: &theme::Env) {
    let (w, h) = (1280, 800);
    std::fs::create_dir_all(dir).expect("shot directory");
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
        let user = Conversation::new(vec![]);
        let mut asking = Conversation::new(vec![]);
        asking.submit(Zeroizing::new("test".into()));
        asking.receive(Response::AuthMessage { auth_message_type: greetd::MessageType::Secret, auth_message: "Password: ".into() });
        let mut failed = Conversation::new(vec![]);
        failed.submit(Zeroizing::new("test".into()));
        failed.receive(Response::Error { error_type: greetd::ErrorType::AuthError, description: "pam_authenticate: AUTH_ERR".into() });
        failed.receive(Response::Error { error_type: greetd::ErrorType::Error, description: "unable to send message: Connection refused".into() });
        let states: [(&str, &Conversation, &str, bool); 4] = [
            ("user", &user, "test", true),
            ("password", &asking, "hunter2", true),
            ("failed", &failed, "", true),
            ("unavailable", &user, "", false),
        ];
        for (name, convo, typed, available) in states {
            let mut view = View::new(w, h);
            view.draw(&shared(convo, &Field::new(Zeroizing::new(typed.to_owned())), available), &theme, &mut kit, Instant::now());
            let mut r = Renderer::new(w as u16, h as u16);
            r.render(&view.scene, IRect::new(0, 0, w, h));
            let rgba: Vec<u8> = r.canvas().data().iter().flat_map(|p| [p.r, p.g, p.b, p.a]).collect();
            let path = dir.join(format!("greeter-{preset}-{name}.png"));
            image::save_buffer(&path, &rgba, w as u32, h as u32, image::ExtendedColorType::Rgba8).expect("png");
            println!("SHOT {}", path.display());
        }
    }
}

impl CompositorHandler for Greeter {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}

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
        self.dirty = true;
    }
}

impl SeatHandler for Greeter {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let repeat: RepeatCallback<Greeter> = Box::new(|g, _, event| g.key(&event));
            self.keyboard = self.seats.get_keyboard_with_repeat(qh, &seat, None, self.handle.clone(), repeat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
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
