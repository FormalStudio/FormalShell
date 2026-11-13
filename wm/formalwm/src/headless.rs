//! The engine on a compositor with no backend, its outputs and a real
//! client connected to it, for what the rig has no hardware for: a host's
//! workspaces on their outputs, the g815's three displays matched by what
//! their EDIDs say, the touchpad's swipes and the lid.
//!
//! The session is ShojiWM's own test harness
//! (`src/shojiwm_rs/tests/common/mod.rs`), MIT, Copyright (c) 2026 bea4dev.

use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Write},
    os::{fd::AsFd, unix::net::UnixStream},
    rc::Rc,
    sync::Mutex,
    time::Duration,
};

use formalwm_core::{model::Big, rules, settings::Settings};
use fs_wm_proto as proto;
use serde_json::{Value, json};
use shojiwm_lib::{
    config::{RuntimeHdrConfig, RuntimeHdrMode, RuntimeOutputMode},
    state::ShojiWM,
    virtual_input::{
        AxisAmount, AxisFrame, VirtualAxis, VirtualButton, VirtualDevice, VirtualInput, VirtualKey,
        VirtualMotion, VirtualPinch, VirtualTouch,
    },
};
use shojiwm_rs::{
    COMPOSITOR, ConfigBuilder, RuntimeBoot,
    cli::CommonArgs,
    ssd::{GestureSwipeEventSnapshot, GestureSwipePhaseSnapshot as Phase},
};
use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, InputEvent, KeyState},
    input::keyboard::Keycode,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{calloop::EventLoop, wayland_server::Display},
    utils::SERIAL_COUNTER,
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    backend::ObjectId,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool,
        wl_surface,
    },
};
use wayland_protocols::xdg::{
    dialog::v1::client::{xdg_dialog_v1::XdgDialogV1, xdg_wm_dialog_v1::XdgWmDialogV1},
    shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base},
    toplevel_tag::v1::client::xdg_toplevel_tag_manager_v1::XdgToplevelTagManagerV1,
};
use wayland_protocols_wlr::layer_shell::v1::client::{zwlr_layer_shell_v1, zwlr_layer_surface_v1};

use crate::engine::ask;

const HOST: &str = r#"{
  "terminal": "foot",
  "outputs": [
    { "name": "TEST-1", "defaultWorkspace": 1 },
    { "name": "TEST-2", "workspaces": [4, 8], "defaultWorkspace": 4, "vrr": 2 }
  ],
  "lidOutput": "TEST-2",
  "directScanout": false
}"#;
const WIDTH: f64 = 1024.0;
const HEIGHT: f64 = 768.0;

struct Session {
    event_loop: EventLoop<'static, ShojiWM>,
    state: ShojiWM,
    connection: Connection,
    queue: EventQueue<Client>,
    client: Client,
    registry: wl_registry::WlRegistry,
    now_ms: u64,
    files: Vec<File>,
}

/// The tests share the process's environment, so they take turns.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A connected display: its connector, what its EDID says (make, model,
/// serial), its size and refresh in mHz.
struct Screen {
    name: &'static str,
    identity: [&'static str; 3],
    size: (i32, i32),
    refresh: i32,
}

fn test_display(name: &'static str) -> Screen {
    Screen {
        name,
        identity: ["test", "test", ""],
        size: (WIDTH as i32, HEIGHT as i32),
        refresh: 60_000,
    }
}

fn output(display: &Screen) -> Output {
    let [make, model, serial] = display.identity;
    let output = Output::new(
        display.name.into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: make.into(),
            model: model.into(),
            serial_number: serial.into(),
        },
    );
    let mode = Mode {
        size: display.size.into(),
        refresh: display.refresh,
    };
    output.change_current_state(Some(mode), None, None, None);
    output.set_preferred(mode);
    output
}

impl Session {
    /// `displays` connect in order, each right of the one before until the
    /// settings place it.
    fn new(test: &str, host: &str, displays: &[Screen]) -> Self {
        let settings = include_str!("../../example/settings.json");
        Self::with_settings(test, host, displays, settings)
    }

    fn with_settings(test: &str, host: &str, displays: &[Screen], settings: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("formalwm-headless-{}-{test}", std::process::id()));
        let (runtime, config) = (root.join("run"), root.join("config/formalwm"));
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(config.join("settings.json"), settings).unwrap();
        std::fs::write(config.join("host.json"), host).unwrap();
        // SAFETY: each test sets these under `ONE_AT_A_TIME`, before its
        // compositor's thread reads them.
        unsafe {
            std::env::set_var("XDG_RUNTIME_DIR", &runtime);
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
        }

        let mut event_loop: EventLoop<ShojiWM> = EventLoop::try_new().unwrap();
        let display: Display<ShojiWM> = Display::new().unwrap();
        let config = ConfigBuilder::new(crate::setup)
            .name("formalwm")
            .asset_root(crate::layers::install_assets());
        let boot = RuntimeBoot::new(Box::new(config), &CommonArgs::parse(&[], &[]));
        let mut state = ShojiWM::new(&mut event_loop, display, &boot);
        let mut x = 0;
        for display in displays {
            let output = output(display);
            output.change_current_state(None, None, None, Some((x, 0).into()));
            state.create_output_global(&output);
            state.space.map_output(&output, (x, 0));
            x += display.size.0;
        }
        state.enable_initial_decoration_runtime();
        state.sync_runtime_display_state();

        let socket = UnixStream::connect(runtime.join(&state.socket_name)).unwrap();
        let connection = Connection::from_socket(socket).unwrap();
        let queue = connection.new_event_queue();
        let registry = connection.display().get_registry(&queue.handle(), ());
        let mut session = Self {
            event_loop,
            state,
            connection,
            queue,
            client: Client::default(),
            registry,
            now_ms: 1000,
            files: Vec::new(),
        };
        session.roundtrip();
        session
    }

    fn roundtrip(&mut self) {
        for _ in 0..5 {
            self.connection.flush().unwrap();
            self.event_loop
                .dispatch(Some(Duration::from_millis(5)), &mut self.state)
                .unwrap();
            self.state.display_handle.flush_clients().unwrap();
            if let Some(guard) = self.queue.prepare_read() {
                let _ = guard.read();
            }
            self.queue.dispatch_pending(&mut self.client).unwrap();
        }
    }

    fn bind<I>(&self, interface: &str) -> I
    where
        I: Proxy + 'static,
        Client: Dispatch<I, ()>,
    {
        let name = self
            .client
            .globals
            .iter()
            .find(|(name, _)| name == interface)
            .unwrap_or_else(|| panic!("{interface} not advertised"))
            .1;
        self.registry
            .bind::<I, _, _>(name, 1, &self.queue.handle(), ())
    }

    fn open(&mut self, app_id: &str) -> wl_surface::WlSurface {
        self.open_tagged(app_id, None).0
    }

    /// A window that says what it is with xdg-toplevel-tag, as Atoll's do.
    fn open_tagged(
        &mut self,
        app_id: &str,
        tag: Option<&str>,
    ) -> (wl_surface::WlSurface, xdg_toplevel::XdgToplevel) {
        let (surface, toplevel) = self.bare(app_id);
        if let Some(tag) = tag {
            self.tag(&toplevel, tag);
        }
        surface.commit();
        self.roundtrip();
        (surface, toplevel)
    }

    /// A toplevel before its first commit, for what a client says then.
    fn bare(&mut self, app_id: &str) -> (wl_surface::WlSurface, xdg_toplevel::XdgToplevel) {
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: xdg_wm_base::XdgWmBase = self.bind("xdg_wm_base");
        let surface = compositor.create_surface(&qh, ());
        let toplevel = shell
            .get_xdg_surface(&surface, &qh, ())
            .get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.into());
        toplevel.set_title(app_id.into());
        (surface, toplevel)
    }

    /// The commit that brings the configure, with nothing drawn.
    fn commit(&mut self, surface: &wl_surface::WlSurface) {
        surface.commit();
        self.roundtrip();
    }

    /// The size the toplevel was last configured to.
    fn configured(&mut self, toplevel: &xdg_toplevel::XdgToplevel) -> (i32, i32) {
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
        self.client.configured[&toplevel.id()]
    }

    /// Every animation run to its end, and the compositor told where that
    /// leaves the windows, as it is each frame of a session.
    fn caught_up(&mut self) {
        ask(|engine| {
            engine.scene.finish();
            engine.present();
            Ok(())
        })
        .unwrap();
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
    }

    /// A window that has no title yet when it opens.
    fn untitled(&mut self, app_id: &str) -> (wl_surface::WlSurface, xdg_toplevel::XdgToplevel) {
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: xdg_wm_base::XdgWmBase = self.bind("xdg_wm_base");
        let surface = compositor.create_surface(&qh, ());
        let toplevel = shell
            .get_xdg_surface(&surface, &qh, ())
            .get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.into());
        surface.commit();
        self.roundtrip();
        (surface, toplevel)
    }

    /// A layer of the shell's over the whole of the first output, above
    /// every window, that leaves the keyboard alone.
    fn cover(
        &mut self,
    ) -> (
        zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wl_surface::WlSurface,
    ) {
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind("zwlr_layer_shell_v1");
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Overlay,
            "cover".into(),
            &qh,
            (),
        );
        layer.set_anchor(zwlr_layer_surface_v1::Anchor::all());
        surface.commit();
        self.roundtrip();
        (layer, surface)
    }

    /// A window whose title is not its app id, as a browser's player has.
    fn open_titled(&mut self, app_id: &str, title: &str) -> wl_surface::WlSurface {
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: xdg_wm_base::XdgWmBase = self.bind("xdg_wm_base");
        let surface = compositor.create_surface(&qh, ());
        let toplevel = shell
            .get_xdg_surface(&surface, &qh, ())
            .get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.into());
        toplevel.set_title(title.into());
        surface.commit();
        self.roundtrip();
        surface
    }

    fn tag(&mut self, toplevel: &xdg_toplevel::XdgToplevel, tag: &str) {
        let tags: XdgToplevelTagManagerV1 = self.bind("xdg_toplevel_tag_manager_v1");
        tags.set_toplevel_tag(toplevel, tag.into());
    }

    /// The keys of a chord by their evdev codes, pressed in order and let
    /// go in reverse. The windows are read again after it, as a frame does.
    fn chord(&mut self, keys: &[u32]) {
        let pressed = keys.iter().map(|key| (*key, KeyState::Pressed));
        let released = keys.iter().rev().map(|key| (*key, KeyState::Released));
        for (key, state) in pressed.chain(released) {
            self.key(key, state);
        }
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
    }

    /// One key by its evdev code, held down until it is let go.
    fn key(&mut self, key: u32, state: KeyState) {
        self.now_ms += 10;
        let event = VirtualKey {
            device: VirtualDevice {
                id: "test-keyboard".into(),
                name: "test keyboard".into(),
                keyboard: true,
            },
            time: self.now_ms,
            keycode: Keycode::new(key + 8),
            state,
            count: 1,
        };
        self.state
            .process_input_event(InputEvent::<VirtualInput>::Keyboard { event });
        self.roundtrip();
    }

    /// One scroll down by this much: wheel notches, toward the user, or a
    /// touchpad's travel in pixels.
    fn scroll_by(&mut self, source: AxisSource, amount: f64) {
        let mut frame = AxisFrame {
            source: Some(source),
            ..AxisFrame::default()
        };
        match source {
            AxisSource::Wheel => frame.add_discrete(Axis::Vertical, amount * 15.0, amount as i32),
            _ => frame.add(Axis::Vertical, amount),
        }
        self.axis(frame);
    }

    /// A wheel that reports only its steps, in 120ths of a notch, the way a
    /// high-resolution one sends its parts of a notch.
    fn wheel_steps(&mut self, v120: f64) {
        self.axis(AxisFrame {
            source: Some(AxisSource::Wheel),
            vertical: AxisAmount {
                value: None,
                v120: Some(v120),
            },
            ..AxisFrame::default()
        });
    }

    fn axis(&mut self, mut frame: AxisFrame) {
        self.now_ms += 10;
        frame.time_msec = self.now_ms as u32;
        let event = VirtualAxis {
            device: VirtualDevice {
                id: "test-pointer".into(),
                name: "test pointer".into(),
                keyboard: false,
            },
            time: self.now_ms,
            frame,
        };
        self.state
            .process_input_event(InputEvent::<VirtualInput>::PointerAxis { event });
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
    }

    /// The keys the client was sent since the last time it was asked.
    fn typed(&mut self) -> Vec<u32> {
        self.roundtrip();
        std::mem::take(&mut self.client.keys)
    }

    /// A connection to the engine's JSON socket.
    fn socket(&self) -> BufReader<UnixStream> {
        let path = proto::socket_path(
            std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
            self.state.socket_name.to_str(),
        );
        let socket = BufReader::new(UnixStream::connect(path).unwrap());
        socket
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(5)))
            .unwrap();
        socket
    }

    /// The first message on `socket` that `wanted` accepts.
    fn message(
        &mut self,
        socket: &mut BufReader<UnixStream>,
        wanted: impl Fn(&Value) -> bool,
    ) -> Option<Value> {
        let mut line = String::new();
        for _ in 0..200 {
            self.roundtrip();
            // A read that timed out leaves what it had of the line.
            while socket.read_line(&mut line).is_ok() && line.ends_with('\n') {
                let message: Value = serde_json::from_str(&line).unwrap();
                if wanted(&message) {
                    return Some(message);
                }
                line.clear();
            }
        }
        None
    }

    /// One request on `socket` and its result.
    fn call(&mut self, socket: &mut BufReader<UnixStream>, method: &str, params: Value) -> Value {
        let request = json!({ "id": 1, "method": method, "params": params });
        writeln!(socket.get_mut(), "{request}").unwrap();
        let answer = self
            .message(socket, |message| message.get("id").is_some())
            .unwrap_or_else(|| panic!("{method}: no answer"));
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        answer["result"].clone()
    }

    /// Gives a window something under the pointer: without a buffer the
    /// compositor has no surface of it to find there.
    fn draw(&mut self, surface: &wl_surface::WlSurface, width: i32, height: i32) {
        let qh = self.queue.handle();
        let shm: wl_shm::WlShm = self.bind("wl_shm");
        let stride = width * 4;
        let path = std::env::temp_dir().join(format!(
            "formalwm-headless-{}-shm-{}",
            std::process::id(),
            self.files.len()
        ));
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        std::fs::remove_file(&path).unwrap();
        file.set_len((stride * height) as u64).unwrap();
        let pool = shm.create_pool(file.as_fd(), stride * height, &qh, ());
        let buffer =
            pool.create_buffer(0, width, height, stride, wl_shm::Format::Argb8888, &qh, ());
        self.files.push(file);
        surface.attach(Some(&buffer), 0, 0);
        surface.damage(0, 0, width, height);
        surface.commit();
        self.roundtrip();
        self.state.refresh_window_decorations().unwrap();
        // A backend hands a new window the keyboard it asked for once it
        // has painted it, and there is none here.
        self.state.pending_initial_focus_window_ids.clear();
        self.state
            .update_keyboard_focus(SERIAL_COUNTER.next_serial());
        self.roundtrip();
    }

    /// A layer across the top of the first output that takes the keyboard.
    fn layer(
        &mut self,
    ) -> (
        zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        wl_surface::WlSurface,
    ) {
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: zwlr_layer_shell_v1::ZwlrLayerShellV1 = self.bind("zwlr_layer_shell_v1");
        let surface = compositor.create_surface(&qh, ());
        let layer = shell.get_layer_surface(
            &surface,
            None,
            zwlr_layer_shell_v1::Layer::Top,
            "launcher".into(),
            &qh,
            (),
        );
        layer.set_anchor(
            zwlr_layer_surface_v1::Anchor::Top
                | zwlr_layer_surface_v1::Anchor::Left
                | zwlr_layer_surface_v1::Anchor::Right,
        );
        layer.set_size(0, 40);
        layer.set_keyboard_interactivity(zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive);
        surface.commit();
        self.roundtrip();
        (layer, surface)
    }

    /// A mouse moved by this much.
    fn pointer_by(&mut self, dx: f64, dy: f64) {
        self.now_ms += 10;
        let event = VirtualMotion {
            device: VirtualDevice {
                id: "test-pointer".into(),
                name: "test pointer".into(),
                keyboard: false,
            },
            time: self.now_ms,
            dx,
            dy,
        };
        self.state
            .process_input_event(InputEvent::<VirtualInput>::PointerMotion { event });
        self.roundtrip();
    }

    fn pointer(&self) -> (f64, f64) {
        let at = self.state.seat.get_pointer().unwrap().current_location();
        (at.x, at.y)
    }

    fn pointer_to(&mut self, x: f64, y: f64) {
        let (from_x, from_y) = self.pointer();
        self.pointer_by(x - from_x, y - from_y);
    }

    /// The left button pressed and let go where the pointer is.
    fn click(&mut self) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.press(state);
        }
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
    }

    fn button(&mut self, state: ButtonState) {
        self.press(state);
        self.state.refresh_window_decorations().unwrap();
        self.roundtrip();
    }

    fn press(&mut self, state: ButtonState) {
        self.now_ms += 10;
        let event = VirtualButton {
            device: VirtualDevice {
                id: "test-pointer".into(),
                name: "test pointer".into(),
                keyboard: false,
            },
            time: self.now_ms,
            button: 0x110,
            state,
        };
        self.state
            .process_input_event(InputEvent::<VirtualInput>::PointerButton { event });
        self.roundtrip();
    }

    /// Turns the loop until `done`: what a timer of the engine's does is
    /// not there on the turn that set it.
    fn until(&mut self, done: impl Fn() -> bool) -> bool {
        for _ in 0..200 {
            if done() {
                return true;
            }
            self.roundtrip();
        }
        done()
    }

    /// One touchpad event on the first output, through the compositor's
    /// door to its config.
    fn swipe(&mut self, phase: Phase, fingers: u32, total: (f64, f64), velocity: (f64, f64)) {
        thread_local! {
            static LAST: std::cell::Cell<(f64, f64)> = const { std::cell::Cell::new((0.0, 0.0)) };
        }
        let last = if phase == Phase::Begin {
            (0.0, 0.0)
        } else {
            LAST.get()
        };
        LAST.set(total);
        self.now_ms += 16;
        let event = GestureSwipeEventSnapshot {
            phase,
            fingers,
            position: None,
            delta_x: total.0 - last.0,
            delta_y: total.1 - last.1,
            total_x: total.0,
            total_y: total.1,
            velocity_x: velocity.0,
            velocity_y: velocity.1,
            output_name: Some("TEST-1".into()),
            device: None,
            timestamp: self.now_ms,
        };
        assert!(
            self.state
                .config_runtime
                .gesture_swipe(&event, self.now_ms)
                .is_ok()
        );
    }

    /// A whole swipe: down, across `total` in two moves, up at `velocity`.
    fn swiped(&mut self, fingers: u32, total: (f64, f64), velocity: (f64, f64)) {
        self.swipe(Phase::Begin, fingers, (0.0, 0.0), (0.0, 0.0));
        self.swipe(
            Phase::Update,
            fingers,
            (total.0 * 0.1, total.1 * 0.1),
            (0.0, 0.0),
        );
        self.swipe(Phase::Update, fingers, total, (0.0, 0.0));
        self.swipe(Phase::End, fingers, total, velocity);
    }
}

#[derive(Default)]
struct Client {
    globals: Vec<(String, u32)>,
    /// The evdev codes of the keys pressed on its keyboard.
    keys: Vec<u32>,
    /// The size each toplevel was last configured to; 0x0 leaves it to
    /// the client.
    configured: HashMap<ObjectId, (i32, i32)>,
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for Client {
    fn event(
        client: &mut Self,
        toplevel: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_toplevel::Event::Configure { width, height, .. } = event {
            client.configured.insert(toplevel.id(), (width, height));
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed),
            ..
        } = event
        {
            client.keys.push(key);
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Client {
    fn event(
        client: &mut Self,
        _: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            client.globals.push((interface, name));
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for Client {
    fn event(
        _: &mut Self,
        shell: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            shell.pong(serial);
        }
    }
}

macro_rules! ignore_events {
    ($($proxy:ty),*) => {$(
        impl Dispatch<$proxy, ()> for Client {
            fn event(_: &mut Self, _: &$proxy, _: <$proxy as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
        }
    )*};
}

impl Dispatch<xdg_surface::XdgSurface, ()> for Client {
    fn event(
        _: &mut Self,
        surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            surface.ack_configure(serial);
        }
    }
}

ignore_events!(
    wl_compositor::WlCompositor,
    wl_surface::WlSurface,
    wl_shm::WlShm,
    wl_shm_pool::WlShmPool,
    wl_buffer::WlBuffer,
    XdgToplevelTagManagerV1,
    XdgWmDialogV1,
    XdgDialogV1,
    wl_seat::WlSeat,
    zwlr_layer_shell_v1::ZwlrLayerShellV1
);

impl Dispatch<zwlr_layer_surface_v1::ZwlrLayerSurfaceV1, ()> for Client {
    fn event(
        _: &mut Self,
        layer: &zwlr_layer_surface_v1::ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            layer.ack_configure(serial);
        }
    }
}

fn state() -> proto::State {
    ask(|engine| Ok(engine.state())).unwrap()
}

fn stack(state: &proto::State, output: &str) -> Vec<String> {
    let mut stack: Vec<&proto::Workspace> = state
        .workspaces
        .iter()
        .filter(|ws| ws.output == output)
        .collect();
    stack.sort_by_key(|ws| ws.idx);
    stack.iter().map(|ws| ws.id.clone()).collect()
}

/// Lands what is still moving: nothing draws frames here.
fn settle() {
    ask(|engine| {
        engine.scene.finish();
        Ok(())
    })
    .unwrap();
}

/// A lone tile on one output with its right border pressed and dragged
/// 100 px out, the button still down.
fn border_dragged(test: &str) -> Session {
    let mut session = Session::new(test, HOST, &[test_display("TEST-1")]);
    let surface = session.open("a");
    session.draw(&surface, 400, 300);
    session.state.refresh_window_decorations().unwrap();
    session.roundtrip();
    settle();
    let a = tile_rect("a");
    session.pointer_to(f64::from(a.x + a.width + 2), f64::from(a.y + a.height / 2));
    session.button(ButtonState::Pressed);
    session.pointer_by(100.0, 0.0);
    assert!(held("1"), "the border press started a resize");
    session
}

fn tile_rect(app_id: &str) -> proto::Rect {
    let seen = state();
    let window = seen.windows.iter().find(|w| w.app_id == app_id).unwrap();
    window.rect.unwrap()
}

fn held(workspace: &'static str) -> bool {
    ask(move |engine| {
        Ok(engine
            .model
            .workspace(workspace)
            .unwrap()
            .tape
            .held
            .is_some())
    })
    .unwrap()
}

fn scroll(workspace: &'static str) -> (f64, f64) {
    ask(move |engine| {
        let model = engine.model.workspace(workspace).unwrap().tape.scroll;
        Ok((model, engine.scene.scroll_of(workspace)))
    })
    .unwrap()
}

#[test]
fn a_workspace_bind_onto_another_output_takes_the_pointer_there() {
    const SUPER: u32 = 125;
    const FOUR: u32 = 5;
    const EIGHT: u32 = 9;
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = Session::new(
        "warp",
        HOST,
        &[test_display("TEST-1"), test_display("TEST-2")],
    );
    session.pointer_to(100.0, 100.0);
    session.chord(&[SUPER, FOUR]);
    let seen = state();
    assert_eq!(
        (
            seen.focused_workspace_id.as_str(),
            seen.focused_output.as_str()
        ),
        ("4", "TEST-2")
    );
    assert_eq!(session.pointer(), (WIDTH * 1.5, HEIGHT / 2.0));
    // On the output that has the focus, the pointer stays where it is.
    session.pointer_to(1100.0, 100.0);
    session.chord(&[SUPER, EIGHT]);
    assert_eq!(state().focused_workspace_id, "8");
    assert_eq!(session.pointer(), (1100.0, 100.0));

    // A workspace with windows: the pointer goes to the middle of the one
    // last focused there, as under Hyprland's binds:workspace_center_on 1.
    session.open("left");
    session.open("right");
    ask(|engine| {
        let left = engine.model.windows.iter().find(|w| w.app_id == "left");
        let left = left.map(|w| w.id.clone()).ok_or("no left")?;
        engine.model.focus_window(&left);
        engine.commit();
        Ok(())
    })
    .unwrap();
    session.pointer_to(100.0, 100.0);
    assert_eq!(state().focused_output, "TEST-1");
    session.chord(&[SUPER, EIGHT]);
    let seen = state();
    assert_eq!(seen.focused_output, "TEST-2");
    let middle = |app: &str| {
        let rect = seen.windows.iter().find(|w| w.app_id == app).unwrap();
        let rect = rect.rect.as_ref().unwrap();
        (
            f64::from(rect.x) + f64::from(rect.width) / 2.0,
            f64::from(rect.y) + f64::from(rect.height) / 2.0,
        )
    };
    let (left, right) = (middle("left"), middle("right"));
    assert!(left.0 < right.0, "two columns side by side");
    let at = session.pointer();
    assert!(
        (at.0 - left.0).abs() <= 1.0 && (at.1 - left.1).abs() <= 1.0,
        "the pointer at {at:?}, the left window's middle at {left:?}"
    );

    // A focus bind takes the pointer into the middle of the window it
    // focuses, as Hyprland's movefocus does.
    const H: u32 = 35;
    const L: u32 = 38;
    session.chord(&[SUPER, L]);
    let seen = state();
    let focused = seen
        .windows
        .iter()
        .find(|w| w.focused)
        .map(|w| w.app_id.clone());
    assert_eq!(focused.as_deref(), Some("right"));
    let rect = seen.windows.iter().find(|w| w.app_id == "right").unwrap();
    let rect = rect.rect.as_ref().unwrap();
    let right = (
        f64::from(rect.x) + f64::from(rect.width) / 2.0,
        f64::from(rect.y) + f64::from(rect.height) / 2.0,
    );
    let at = session.pointer();
    assert!(
        (at.0 - right.0).abs() <= 1.0 && (at.1 - right.1).abs() <= 1.0,
        "the pointer at {at:?}, the right window's middle at {right:?}"
    );

    // Already over the window the focus goes to, the pointer stays put.
    let rect = seen.windows.iter().find(|w| w.app_id == "left").unwrap();
    let rect = rect.rect.as_ref().unwrap();
    let inside = (f64::from(rect.x) + 10.0, f64::from(rect.y) + 10.0);
    session.pointer_to(inside.0, inside.1);
    session.chord(&[SUPER, H]);
    let focused = state()
        .windows
        .into_iter()
        .find(|w| w.focused)
        .map(|w| w.app_id);
    assert_eq!(focused.as_deref(), Some("left"));
    assert_eq!(session.pointer(), inside);
}

#[test]
fn super_and_the_wheel_take_one_workspace_a_turn() {
    const SUPER: u32 = 125;
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = Session::new(
        "wheel",
        HOST,
        &[test_display("TEST-1"), test_display("TEST-2")],
    );
    for app in ["one", "two", "three"] {
        session.open(app);
    }
    ask(|engine| {
        let ids: Vec<String> = engine.model.windows.iter().map(|w| w.id.clone()).collect();
        engine.model.move_to_workspace(&ids[1], "2", false);
        engine.model.move_to_workspace(&ids[2], "3", false);
        engine.commit();
        Ok(())
    })
    .unwrap();
    session.pointer_to(100.0, 100.0);
    // Hyprland's binds:scroll_event_delay: the scrolls of the 300 ms after
    // one the binds took go to the window.
    let delay = || std::thread::sleep(Duration::from_millis(320));
    let mut seen = Vec::new();
    enum Turn {
        Wheel(f64),
        Steps(f64),
        Fingers(f64),
    }
    let mut turn = |session: &mut Session, kind: Turn| {
        match kind {
            Turn::Wheel(notches) => session.scroll_by(AxisSource::Wheel, notches),
            Turn::Steps(v120) => session.wheel_steps(v120),
            Turn::Fingers(pixels) => session.scroll_by(AxisSource::Finger, pixels),
        }
        seen.push(state().focused_workspace_id);
    };
    session.key(SUPER, KeyState::Pressed);
    turn(&mut session, Turn::Wheel(1.0));
    turn(&mut session, Turn::Wheel(1.0));
    delay();
    turn(&mut session, Turn::Wheel(1.0));
    delay();
    // Eighths of a notch as smooth amounts with no steps: one workspace,
    // the portable's.
    for _ in 0..8 {
        turn(&mut session, Turn::Wheel(0.125));
    }
    delay();
    // A high-resolution wheel's notch, eight events of 15/120 and no
    // amount: one workspace, round to the first.
    for _ in 0..8 {
        turn(&mut session, Turn::Steps(15.0));
    }
    delay();
    // Two fingers bind nothing, and start the delay as a wheel does.
    turn(&mut session, Turn::Fingers(40.0));
    turn(&mut session, Turn::Wheel(1.0));
    delay();
    turn(&mut session, Turn::Wheel(-1.0));
    session.key(SUPER, KeyState::Released);
    let mut expected = vec!["2", "2", "3"];
    expected.extend(["4"; 8]);
    expected.extend(["1"; 8]);
    expected.extend(["1", "1", "4"]);
    assert_eq!(seen, expected);
    assert_eq!(state().focused_output, "TEST-2");
}

#[test]
fn a_border_drag_whose_grab_is_taken_away_lets_the_view_go() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = border_dragged("border-taken");
    // What a session lock does to the grab (`take_pointer_from_clients`): it
    // is unset with the button still down, and no release follows.
    let pointer = session.state.seat.get_pointer().unwrap();
    pointer.unset_grab(&mut session.state, SERIAL_COUNTER.next_serial(), 0);
    assert!(session.until(|| !held("1")), "the hold went with the grab");
    settle();
    let a = tile_rect("a");
    assert!(
        (2 * a.x + a.width - WIDTH as i32).abs() <= 1,
        "centred again: {a:?}"
    );
    session.button(ButtonState::Released);
}

#[test]
fn a_border_drag_let_go_eases_into_place() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = border_dragged("border-eased");
    session.button(ButtonState::Released);
    assert!(session.until(|| !held("1")));
    let animating = ask(|engine| Ok(engine.scene.animating())).unwrap();
    assert!(animating, "the recentring is animated, not jumped to");
}

#[test]
fn two_outputs_and_the_touchpad() {
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = Session::new(
        "two",
        HOST,
        &[test_display("TEST-1"), test_display("TEST-2")],
    );

    // The host's workspaces on their outputs, each showing its default.
    let seen = state();
    assert_eq!(stack(&seen, "TEST-1"), ["1", "2", "3", "5", "6", "7", "9"]);
    assert_eq!(stack(&seen, "TEST-2"), ["4", "8"]);
    let active = |seen: &proto::State, output: &str| {
        seen.workspaces
            .iter()
            .find(|ws| ws.output == output && ws.active)
            .map(|ws| ws.id.clone())
    };
    assert_eq!(active(&seen, "TEST-1").as_deref(), Some("1"));
    assert_eq!(active(&seen, "TEST-2").as_deref(), Some("4"));
    assert_eq!(seen.focused_output, "TEST-1");

    // Four half columns on a tape two wide: the last one in view.
    for app in ["one", "two", "three", "four"] {
        session.open(app);
    }
    let seen = state();
    assert!(seen.windows.iter().all(|window| window.workspace_id == "1"));
    let column = WIDTH / 2.0 - 4.0;
    settle();
    assert_eq!(scroll("1"), (2.0 * column, 2.0 * column));

    // Three fingers sideways: the tape is under them, pixel for pixel.
    session.swipe(Phase::Begin, 3, (0.0, 0.0), (0.0, 0.0));
    session.swipe(Phase::Update, 3, (10.0, 0.0), (0.0, 0.0));
    session.swipe(Phase::Update, 3, (300.0, 2.0), (0.0, 0.0));
    assert_eq!(scroll("1"), (2.0 * column - 300.0, 2.0 * column - 300.0));
    // Let go at rest: it settles on a column and the focus is in view.
    session.swipe(Phase::End, 3, (300.0, 2.0), (0.0, 0.0));
    let (rest, _) = scroll("1");
    assert_eq!(rest % column, 0.0, "the tape rests on a column's edge");
    let seen = state();
    let focused = seen.windows.iter().find(|window| window.focused).unwrap();
    let rect = focused.rect.as_ref().unwrap();
    assert!(rect.x >= 0 && rect.x + rect.width <= WIDTH as i32);
    settle();

    // A fling carries: three fingers flicked right take the tape to its start.
    session.swiped(3, (40.0, 0.0), (2500.0, 0.0));
    assert_eq!(scroll("1").0, 0.0);
    assert!(state().windows.iter().find(|w| w.focused).unwrap().column <= 1);
    settle();

    // Three fingers up: the stack follows the fingers, and a flick walks
    // past the neighbour.
    session.swipe(Phase::Begin, 3, (0.0, 0.0), (0.0, 0.0));
    session.swipe(Phase::Update, 3, (0.0, -10.0), (0.0, 0.0));
    session.swipe(Phase::Update, 3, (3.0, -384.0), (0.0, 0.0));
    let held = ask(|engine| Ok(engine.scene.stack_of("TEST-1"))).unwrap();
    assert_eq!(held, 0.5);
    session.swipe(Phase::End, 3, (3.0, -384.0), (0.0, -3000.0));
    let seen = state();
    assert_eq!(seen.focused_workspace_id, "3");
    assert_eq!(active(&seen, "TEST-2").as_deref(), Some("4"));
    settle();
    // Down again, slowly: one workspace back.
    session.swiped(3, (0.0, 500.0), (0.0, 0.0));
    assert_eq!(state().focused_workspace_id, "2");
    settle();

    // Four fingers up open the overview under the fingers, down closes it.
    session.swipe(Phase::Begin, 4, (0.0, 0.0), (0.0, 0.0));
    session.swipe(Phase::Update, 4, (0.0, -10.0), (0.0, 0.0));
    session.swipe(Phase::Update, 4, (0.0, -192.0), (0.0, 0.0));
    let (progress, held, open) = ask(|engine| {
        let overview = &engine.overview;
        Ok((overview.progress.value(), overview.held, overview.open))
    })
    .unwrap();
    assert_eq!((progress, held, open), (0.5, true, false));
    session.swipe(Phase::Update, 4, (0.0, -250.0), (0.0, 0.0));
    session.swipe(Phase::End, 4, (0.0, -250.0), (0.0, 0.0));
    assert!(state().overview);
    // With the overview open three fingers sideways leave the tape alone.
    let before = scroll("2");
    session.swiped(3, (200.0, 0.0), (0.0, 0.0));
    assert_eq!(scroll("2"), before);
    session.swiped(4, (0.0, 300.0), (0.0, 0.0));
    assert!(!state().overview);

    // A rule's workspace lives on the second output: the window opens
    // there and the view follows it.
    session.open("Slack");
    let seen = state();
    let slack = seen.windows.iter().find(|w| w.app_id == "Slack").unwrap();
    assert_eq!(slack.workspace_id, "4");
    assert_eq!(seen.focused_output, "TEST-2");
    assert_eq!(seen.focused_workspace_id, "4");
    let rect = slack.rect.as_ref().unwrap();
    assert!(rect.x >= WIDTH as i32, "the window is on the second output");

    // An empty workspace in view: the keyboard leaves the window it was on.
    let keyboard = |session: &Session| session.state.window_keyboard_focus.is_some();
    let go = |workspace: &'static str| {
        ask(|engine| {
            engine.model.focus_workspace(workspace);
            engine.commit();
            Ok(())
        })
        .unwrap();
    };
    assert!(keyboard(&session), "Slack has the keyboard");
    go("8");
    session.roundtrip();
    assert!(!keyboard(&session), "nothing in view, nobody has it");
    go("4");
    session.roundtrip();
    assert!(
        keyboard(&session),
        "back on Slack's workspace, Slack has it"
    );

    // A window sent away without the view following gives the keyboard up,
    // and a layer that had it in between hands it to nobody: the workspace
    // the window went to stays out of view.
    let slack = ask(|engine| Ok(engine.model.focused_window().cloned())).unwrap();
    let slack = slack.expect("Slack is focused");
    let send = |workspace: &'static str, follow: bool| {
        let id = slack.clone();
        ask(move |engine| {
            let moved = engine.model.move_to_workspace(&id, workspace, follow);
            engine.commit();
            moved.then_some(()).ok_or("not moved".into())
        })
        .unwrap();
    };
    let typing = |session: &Session| {
        let keyboard = session.state.seat.get_keyboard();
        keyboard
            .and_then(|keyboard| keyboard.current_focus())
            .is_some()
    };
    let shown = || active(&state(), "TEST-2");
    send("8", false);
    session.roundtrip();
    assert_eq!(shown().as_deref(), Some("4"));
    assert!(!keyboard(&session), "sent away, it gives the keyboard up");
    let (layer, surface) = session.layer();
    assert!(typing(&session), "the layer takes the keyboard");
    layer.destroy();
    surface.destroy();
    session.roundtrip();
    assert!(!typing(&session), "the layer gone, nobody has it");
    assert_eq!(
        shown().as_deref(),
        Some("4"),
        "and no workspace is shown for it"
    );
    // The compositor giving it the keyboard all the same shows nothing:
    // the engine hears of it and takes the keyboard back.
    let id = slack.clone();
    ask(move |engine| {
        engine.windows[&id].handle.focus();
        Ok(())
    })
    .unwrap();
    session.roundtrip();
    assert!(keyboard(&session));
    let id = slack.clone();
    ask(move |engine| {
        let window = engine.windows[&id].handle;
        engine.focused(window);
        Ok(())
    })
    .unwrap();
    session.roundtrip();
    assert_eq!(shown().as_deref(), Some("4"));
    assert!(!keyboard(&session), "out of view, it keeps no keyboard");
    send("4", true);
    session.roundtrip();
    assert!(keyboard(&session), "back in view, it has the keyboard");

    // `binds.list`: the shell's Escape is a row while it is bound.
    let escapes = || {
        ask(|engine| {
            let binds = engine.binds_list().into_iter();
            Ok(binds
                .filter(|bind| bind.key == "Escape" && bind.mods.is_empty())
                .collect::<Vec<_>>())
        })
        .unwrap()
    };
    let escape = |argv: Option<&'static str>| {
        ask(move |engine| {
            match argv {
                Some(argv) => engine.escapes.insert("card".into(), vec![argv.into()]),
                None => engine.escapes.remove("card"),
            };
            engine.bind_escapes();
            Ok(())
        })
        .unwrap();
    };
    assert_eq!(escapes(), []);
    escape(Some("true"));
    let listed = escapes();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].action.as_str(), &listed[0].args[..]),
        ("spawn", &["true".to_string()][..])
    );
    escape(None);
    assert_eq!(escapes(), []);

    // Black under everything, and again after a reload, on which the
    // compositor goes back to its own colour.
    let black = [0.0, 0.0, 0.0, 1.0];
    assert_eq!(session.state.clear_color, black);
    session.state.reload_decoration_runtime();
    for _ in 0..40 {
        session.roundtrip();
        if session.state.clear_color == black {
            break;
        }
    }
    assert_eq!(session.state.clear_color, black);

    // `directScanout` false: the core is told to composite every frame.
    let configs = &session.state.runtime_output_configs;
    let scanout = |name: &str| configs.get(name).and_then(|config| config.direct_scanout);
    assert_eq!(
        (scanout("TEST-1"), scanout("TEST-2")),
        (Some(false), Some(false))
    );

    // `cm_auto_hdr`: every output is SDR until fullscreen HDR content.
    let auto = Some(RuntimeHdrConfig::Mode(RuntimeHdrMode::Auto));
    assert_eq!(
        configs.get("TEST-1").and_then(|config| config.hdr.clone()),
        auto
    );

    // `vrr` 2: asked of the core only while a window is fullscreen there.
    let vrr = |session: &Session| {
        let config = session.state.runtime_output_configs.get("TEST-2");
        config.and_then(|config| config.vrr)
    };
    let toggle = || {
        ask(|engine| {
            let id = engine.model.focused_window().cloned().ok_or("no focus")?;
            engine.toggle_fullscreen(&id);
            Ok(())
        })
        .unwrap();
    };
    assert_eq!(vrr(&session), Some(false));
    toggle();
    session.roundtrip();
    assert_eq!(vrr(&session), Some(true), "fullscreen, vrr is asked for");
    toggle();
    session.roundtrip();
    assert_eq!(vrr(&session), Some(false));

    // A window its rule confines the pointer to keeps it for as long as it
    // has the keyboard.
    let surface = session.open("Minecraft 1.21");
    settle();
    session.roundtrip();
    let game = state()
        .windows
        .iter()
        .find(|window| window.app_id == "Minecraft 1.21")
        .and_then(|window| window.rect)
        .unwrap();
    session.draw(&surface, game.width, game.height);
    settle();
    session.roundtrip();
    session.state.refresh_window_decorations().unwrap();
    let right = f64::from(game.x + game.width);
    let (x, y) = session.pointer();
    session.pointer_by(
        f64::from(game.x + game.width / 2) - x,
        f64::from(game.y + game.height / 2) - y,
    );
    session.pointer_by(4000.0, 0.0);
    let (x, _) = session.pointer();
    assert!(
        x < right && x > right - 2.0,
        "the pointer at {x} is not held at the edge at {right}"
    );
    let other = session.open("other");
    session.draw(&other, 400, 300);
    settle();
    session.roundtrip();
    session.pointer_by(4000.0, 0.0);
    let (x, _) = session.pointer();
    assert!(
        x > right,
        "the pointer at {x} is still held, without the keyboard"
    );

    // The dynamic cursor: stretched along its way at speed, as it is at
    // rest, and never shaped while it is hidden.
    let shaped = |session: &Session| session.state.config_cursor_transform;
    ask(|engine| {
        engine.cursor_hidden = false;
        Ok(())
    })
    .unwrap();
    session.pointer_by(-600.0, 0.0);
    let mut seen = None;
    for _ in 0..40 {
        session.pointer_by(-12.0, 0.0);
        seen = seen.or(shaped(&session));
    }
    let seen = seen.expect("at speed the cursor is shaped");
    assert!(seen.scale_x > 1.0 && seen.scale_y < 1.0, "{seen:?}");
    assert_eq!((seen.rotate, seen.scale_axis.abs()), (0.0, 180.0));
    for _ in 0..200 {
        if shaped(&session).is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        session.roundtrip();
    }
    assert_eq!(shaped(&session), None, "at rest it is drawn as it is");
    ask(|engine| {
        engine.cursor_hidden = true;
        Ok(())
    })
    .unwrap();
    for _ in 0..40 {
        session.pointer_by(12.0, 0.0);
        assert_eq!(shaped(&session), None, "a hidden cursor is not shaped");
    }

    // A window's xdg-toplevel-tag is in its row, and follows the client.
    let native = session.open("native");
    session.draw(&native, 400, 300);
    let (mac, toplevel) = session.open_tagged("mac", Some("atoll-mac"));
    session.draw(&mac, 400, 300);
    settle();
    session.roundtrip();
    session.state.refresh_window_decorations().unwrap();
    let row = |app: &str| {
        let seen = state();
        seen.windows
            .into_iter()
            .find(|window| window.app_id == app)
            .unwrap()
    };
    assert_eq!(row("mac").tag, "atoll-mac");
    assert_eq!(row("native").tag, "");
    let mut listener = session.socket();
    session.tag(&toplevel, "atoll-mac-sheet");
    session.roundtrip();
    session.state.refresh_window_decorations().unwrap();
    let told = session.message(&mut listener, |message| {
        message["event"] == proto::event::STATE_CHANGED
            && message["payload"]["windows"]
                .as_array()
                .is_some_and(|rows| rows.iter().any(|row| row["tag"] == "atoll-mac-sheet"))
    });
    assert!(told.is_some(), "the new tag is broadcast");
    assert_eq!(row("mac").tag, "atoll-mac-sheet");
    drop(listener);

    // `binds.handOver`: the Mac window is handed the Super chords while it
    // has the keyboard. Until then Super+Z is the zen bind and the window
    // never sees the Z.
    const SUPER: u32 = 125;
    const Z: u32 = 44;
    const H: u32 = 35;
    const L: u32 = 38;
    let seat: wl_seat::WlSeat = session.bind("wl_seat");
    seat.get_keyboard(&session.queue.handle(), ());
    session.roundtrip();
    let zen = || {
        let seen = state();
        let focused = seen.workspaces.into_iter().find(|ws| ws.focused);
        focused.and_then(|ws| ws.zen).is_some()
    };
    let handed = || state().hand_over.map(|held| (held.window, held.active));
    assert!(row("mac").focused);
    session.typed();
    session.chord(&[SUPER, Z]);
    assert!(zen(), "the bind ran");
    assert_eq!(session.typed(), [SUPER]);
    session.chord(&[SUPER, Z]);
    assert!(!zen());
    session.typed();

    let mut shell = session.socket();
    let reserved = json!(["SUPER + H", "super+l"]);
    session.call(
        &mut shell,
        proto::method::BINDS_HAND_OVER,
        json!({ "on": true, "reserved": reserved, "window": row("mac").id }),
    );
    assert_eq!(handed(), Some((row("mac").id, true)));
    let wire = session.call(&mut shell, proto::method::STATE_GET, Value::Null);
    assert_eq!(wire["handOver"]["reserved"], reserved);
    assert_eq!(wire["handOver"]["active"], json!(true));
    let tags: Vec<&Value> = wire["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|window| &window["tag"])
        .filter(|tag| *tag != "")
        .collect();
    assert_eq!(tags, ["atoll-mac-sheet"]);
    session.chord(&[SUPER, Z]);
    assert!(!zen(), "the chord is the window's, the bind is off the key");
    assert_eq!(session.typed(), [SUPER, Z]);

    // A reserved chord is still the bind's. It moves the focus, and with
    // the keyboard on another window every bind is back.
    session.chord(&[SUPER, H]);
    assert!(row("native").focused);
    assert_eq!(session.typed(), [SUPER]);
    assert_eq!(handed(), Some((row("mac").id, false)));
    session.chord(&[SUPER, Z]);
    assert!(zen(), "over the other window the bind runs");
    session.chord(&[SUPER, Z]);
    assert!(!zen());
    assert_eq!(session.typed(), [SUPER, SUPER]);

    // It holds over the window it was asked for: the keyboard back on it,
    // its chords are its own again without another word from the shell.
    session.chord(&[SUPER, L]);
    assert!(row("mac").focused);
    assert_eq!(handed(), Some((row("mac").id, true)));
    session.typed();
    session.chord(&[SUPER, Z]);
    assert!(!zen());
    assert_eq!(session.typed(), [SUPER, Z]);

    // Not while a layer has the keyboard, and not under the overview.
    let (layer, surface) = session.layer();
    session.roundtrip();
    assert_eq!(handed(), Some((row("mac").id, false)));
    layer.destroy();
    surface.destroy();
    session.roundtrip();
    assert_eq!(handed(), Some((row("mac").id, true)));
    let overview = |open: bool| {
        ask(move |engine| {
            engine.set_overview(Some(open));
            Ok(())
        })
        .unwrap();
    };
    overview(true);
    assert_eq!(handed(), Some((row("mac").id, false)));
    overview(false);
    session.roundtrip();
    assert_eq!(handed(), Some((row("mac").id, true)));

    // Another list over the same window binds the keys again.
    session.call(
        &mut shell,
        proto::method::BINDS_HAND_OVER,
        json!({ "on": true, "reserved": ["SUPER + Z"] }),
    );
    session.typed();
    session.chord(&[SUPER, Z]);
    assert!(zen(), "reserved now, it is the bind's");
    session.chord(&[SUPER, Z]);
    assert!(!zen());
    assert_eq!(session.typed(), [SUPER, SUPER]);

    // The shell asks out with the window it sees focused, not the one held.
    session.call(
        &mut shell,
        proto::method::BINDS_HAND_OVER,
        json!({ "on": false, "window": row("native").id }),
    );
    assert_eq!(handed(), None);

    // The connection that asked is gone: so is the hand-over, and the
    // binds are on the keys again.
    session.call(
        &mut shell,
        proto::method::BINDS_HAND_OVER,
        json!({ "on": true, "reserved": [] }),
    );
    assert_eq!(handed(), Some((row("mac").id, true)));
    drop(shell);
    for _ in 0..200 {
        session.roundtrip();
        if handed().is_none() {
            break;
        }
    }
    assert_eq!(handed(), None, "the shell died over the window");
    session.typed();
    session.chord(&[SUPER, Z]);
    assert!(zen(), "the bind is back");
    session.chord(&[SUPER, Z]);
    assert!(!zen());
    assert_eq!(session.typed(), [SUPER, SUPER]);

    // The window closing ends it too.
    let mut shell = session.socket();
    session.call(
        &mut shell,
        proto::method::BINDS_HAND_OVER,
        json!({ "on": true }),
    );
    assert_eq!(handed(), Some((row("mac").id, true)));
    toplevel.destroy();
    mac.destroy();
    session.roundtrip();
    assert_eq!(handed(), None);
    drop(shell);

    // The keyboard is where the model has the focus, and the model where
    // the keyboard is. Two windows nobody has used yet, the newer one with
    // the keyboard.
    let older = session.open("older");
    let newer = session.open("newer");
    for (surface, app) in [(&older, "older"), (&newer, "newer")] {
        settle();
        let rect = row(app).rect.unwrap();
        session.draw(surface, rect.width, rect.height);
    }
    settle();
    session.roundtrip();
    session.state.refresh_window_decorations().unwrap();
    // The window the seat's keyboard is on, and the one the state says.
    let keyed = || {
        ask(|engine| {
            let focus = COMPOSITOR.input.keyboard_focus().get_untracked();
            let window = focus.and_then(|focus| focus.window_id);
            Ok(window.map(|id| engine.public(&id)))
        })
        .unwrap()
    };
    let said = || state().focused_window_id;
    let (older, newer) = (row("older").id, row("newer").id);
    assert_eq!((keyed(), said()), (Some(newer.clone()), newer.clone()));

    // A focus the compositor refuses does not stay in the model: it gives
    // an older window nothing over a newer one unless it is asked on
    // request.
    let id = older.clone();
    ask(move |engine| {
        let id = engine.resolve(&id)?;
        engine.model.focus_window(&id);
        engine.windows[&id].handle.focus();
        engine.asked_focus = Some(id);
        engine.commit();
        engine.settle_focus();
        Ok(())
    })
    .unwrap();
    assert_eq!(said(), older, "the model has it for now");
    let back = newer.clone();
    assert!(
        session.until(|| state().focused_window_id == back),
        "the model follows the keyboard back"
    );
    assert_eq!(keyed(), Some(newer.clone()));

    // `window.focus`, the shell's pick: the older window has the keyboard
    // and the state says so.
    let mut shell = session.socket();
    session.call(
        &mut shell,
        proto::method::WINDOW_FOCUS,
        json!({ "id": older }),
    );
    session.roundtrip();
    assert_eq!((keyed(), said()), (Some(older.clone()), older.clone()));
    // A press on the newer one: both say so again.
    let rect = row("newer").rect.unwrap();
    session.pointer_to(
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    );
    session.click();
    assert_eq!((keyed(), said()), (Some(newer.clone()), newer.clone()));
    drop(shell);

    // `bind.escape`: the shell's command runs and the Escape is the
    // focused client's all the same.
    const ESCAPE: u32 = 1;
    let mark = std::path::PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap()).join("escaped");
    let argv = vec!["touch".to_owned(), mark.display().to_string()];
    ask(move |engine| {
        engine.escapes.insert("card".into(), argv);
        engine.bind_escapes();
        Ok(())
    })
    .unwrap();
    session.typed();
    session.chord(&[ESCAPE]);
    assert_eq!(session.typed(), [ESCAPE], "the key reaches the client");
    assert!(session.until(|| mark.exists()), "and the command ran");
    escape(None);

    // A modifier let go runs its bind when it was pressed and let go
    // alone. After a chord with it, only a bind that holds the modifier
    // it is on runs.
    const CTRL: u32 = 29;
    const SHIFT: u32 = 42;
    const A: u32 = 30;
    let binds = r#"{ "binds": [
        { "chord": "Control_L", "action": "zen", "release": true, "passthrough": true },
        { "chord": "SHIFT+Shift_L", "action": "float", "release": true, "passthrough": true }
    ] }"#;
    let binds = Settings::parse(binds, None).unwrap().binds;
    let last = ask(move |engine| {
        let mut settings = (*engine.settings).clone();
        settings.binds.extend(binds);
        engine.settings = Rc::new(settings);
        engine.bind_keys();
        Ok(engine.settings.bind_groups().len() - 1)
    })
    .unwrap();
    // `bind_keys` only marks the binds changed: the compositor has them a
    // turn later, and a key pressed before that meets the old ones.
    let last = format!("formalwm:{last}");
    let installed =
        |session: &Session| session.state.runtime_key_bindings.iter().any(|bind| bind.id == last);
    assert!(!installed(&session), "not before the compositor's next turn");
    for _ in 0..50 {
        if installed(&session) {
            break;
        }
        session.state.refresh_window_decorations().unwrap();
        session.roundtrip();
    }
    assert!(installed(&session), "the new binds are the compositor's before a key is pressed");
    session.chord(&[CTRL]);
    assert!(zen(), "let go alone, the bind runs");
    session.chord(&[CTRL, A]);
    assert!(zen(), "after a chord it does not");
    session.chord(&[CTRL]);
    assert!(!zen());
    session.chord(&[SHIFT, A]);
    assert!(row("newer").floating, "this one runs after a chord too");
    session.chord(&[SHIFT]);
    assert!(!row("newer").floating);

    // The output under the pointer is the focused one, and the keyboard
    // stays with its window. The first output shows an empty workspace.
    assert_eq!(state().focused_output, "TEST-2");
    let mut listener = session.socket();
    session.pointer_to(WIDTH / 2.0, HEIGHT / 2.0);
    let told = session.message(&mut listener, |message| {
        message["event"] == proto::event::STATE_CHANGED
            && message["payload"]["focusedOutput"] == "TEST-1"
    });
    assert!(told.is_some(), "the pointer's output is broadcast");
    drop(listener);
    let seen = state();
    assert_eq!(
        (seen.focused_output.as_str(), seen.focused_window_id),
        ("TEST-1", newer.clone())
    );
    assert_eq!(keyed(), Some(newer.clone()));
    // A press on an output with nothing to press on reaches it too.
    let focus_output = |output: &'static str| {
        ask(move |engine| {
            engine.model.focus_output(output);
            engine.commit();
            Ok(())
        })
        .unwrap();
    };
    focus_output("TEST-2");
    assert_eq!(state().focused_output, "TEST-2");
    session.click();
    assert_eq!(state().focused_output, "TEST-1");
    // Not while a window is dragged across.
    let dragging = |on: bool| {
        ask(move |engine| {
            engine.hold.window_drag = on;
            Ok(())
        })
        .unwrap();
    };
    dragging(true);
    session.pointer_to(WIDTH * 1.5, HEIGHT / 2.0);
    assert_eq!(state().focused_output, "TEST-1");
    dragging(false);
    session.pointer_by(1.0, 0.0);
    assert_eq!(state().focused_output, "TEST-2");

    // A workspace asked for while it is in view under the pointer shows
    // nothing new and takes the keyboard all the same: to its window, and
    // to nobody when it has none.
    let mut shell = session.socket();
    let mut ask_for = |session: &mut Session, workspace: &'static str| {
        go(workspace);
        let rect = row("newer").rect.unwrap();
        session.pointer_to(f64::from(rect.x + 20), f64::from(rect.y + 20));
        session.click();
        assert_eq!((keyed(), said()), (Some(newer.clone()), newer.clone()));
        session.pointer_to(WIDTH / 2.0, HEIGHT / 2.0);
        let seen = state();
        assert_eq!(
            (seen.focused_workspace_id.as_str(), seen.focused_window_id),
            (workspace, newer.clone())
        );
        session.call(
            &mut shell,
            proto::method::WORKSPACE_FOCUS,
            json!({ "id": workspace }),
        );
        session.roundtrip();
    };
    ask_for(&mut session, "1");
    let on_one: Vec<String> = state()
        .windows
        .into_iter()
        .filter(|window| window.workspace_id == "1")
        .map(|window| window.id)
        .collect();
    assert!(
        on_one.contains(&said()),
        "a window of workspace 1 is focused"
    );
    assert_eq!(keyed(), Some(said()), "and it has the keyboard");
    ask_for(&mut session, "2");
    assert_eq!((keyed(), said()), (None, String::new()));

    classification(&mut session);

    // The lid: the host's lid output is off while it is shut.
    let off = |session: &Session| {
        session
            .state
            .runtime_output_configs
            .get("TEST-2")
            .is_some_and(|config| config.mode == Some(RuntimeOutputMode::Disabled))
    };
    assert!(!off(&session));
    session.state.lid_switch_toggled(true);
    session.roundtrip();
    assert!(off(&session), "shut, the lid output is disabled");
    assert_eq!(
        state()
            .workspaces
            .iter()
            .filter(|ws| ws.output == "TEST-2")
            .count(),
        0
    );
    // The way back is not seen here: without a backend the compositor
    // stops listing an output it has disabled, where the tty backend keeps
    // the connector. The settings are asked again all the same.
    session.state.lid_switch_toggled(false);
    session.roundtrip();
    let lid = COMPOSITOR.input.lid_switch().get_untracked();
    assert_eq!(lid.map(|lid| lid.closed), Some(false));
}

/// The g815 on its own host file: its three displays matched by what their
/// EDIDs say, connected in the order its tty connects them, each laid out,
/// scaled and holding its workspaces as host.lua has them under Hyprland.
#[test]
fn the_g815s_three_displays() {
    const SUPER: u32 = 125;
    const SHIFT: u32 = 42;
    let key = |number: u32| number + 1;
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let display = |name, identity, size, refresh| Screen {
        name,
        identity,
        size,
        refresh,
    };
    let mut session = Session::new(
        "g815",
        include_str!("../../example/hosts/g815/host.json"),
        &[
            display(
                "HDMI-A-1",
                ["ASUSTek COMPUTER INC", "PA278CGV", "T1LMQS063802"],
                (2560, 1440),
                143_996,
            ),
            display("eDP-1", ["BOE", "NE180QDM-NZC", ""], (2560, 1600), 240_000),
            display(
                "DP-3",
                ["ASUSTek COMPUTER INC", "MB16QHG", "RCLMTF037964"],
                (2560, 1600),
                119_963,
            ),
        ],
    );

    // Positions and scales, in logical pixels: the portable's bottom lines
    // up with the desk's, the panel right of it.
    let seen = state();
    let placed = |seen: &proto::State, name: &str| {
        seen.outputs
            .iter()
            .find(|output| output.name == name)
            .map(|o| (o.x, o.y, o.width, o.height, o.scale))
            .unwrap()
    };
    let desk = (2560, 0, 2560, 1440, 1.0);
    let portable = (5120, 440, 1600, 1000, 1.6);
    assert_eq!(placed(&seen, "HDMI-A-1"), desk);
    assert_eq!(placed(&seen, "DP-3"), portable);
    assert_eq!(placed(&seen, "eDP-1"), (6720, 0, 2048, 1280, 1.25));
    let config = |session: &Session, name: &str| {
        session
            .state
            .runtime_output_configs
            .get(name)
            .cloned()
            .unwrap()
    };
    for name in ["HDMI-A-1", "DP-3", "eDP-1"] {
        assert_eq!(config(&session, name).direct_scanout, Some(false), "{name}");
    }
    // The entry each matched, by its VRR policy: 2 on the desk, 3 on the
    // panel, none on the portable.
    let vrr = ask(|engine| {
        Ok(COMPOSITOR
            .output
            .state()
            .get_untracked()
            .into_values()
            .map(|output| {
                let facts = formalwm_core::settings::OutputFacts {
                    name: &output.name,
                    description: output.description.as_deref().unwrap_or_default(),
                    make: output.make.as_deref().unwrap_or_default(),
                    model: output.model.as_deref().unwrap_or_default(),
                    serial: output.serial.as_deref().unwrap_or_default(),
                };
                let vrr = engine.settings.output_for(&facts).map(|entry| entry.vrr);
                (output.name.clone(), vrr)
            })
            .collect::<std::collections::BTreeMap<_, _>>())
    })
    .unwrap();
    assert_eq!(vrr["HDMI-A-1"], Some(2));
    assert_eq!(vrr["DP-3"], Some(0));
    assert_eq!(vrr["eDP-1"], Some(3));

    // 4 and 8 on the portable, the rest on the desk, each showing its default.
    assert_eq!(
        stack(&seen, "HDMI-A-1"),
        ["1", "2", "3", "5", "6", "7", "9"]
    );
    assert_eq!(stack(&seen, "DP-3"), ["4", "8"]);
    let active = |seen: &proto::State, output: &str| {
        seen.workspaces
            .iter()
            .find(|ws| ws.output == output && ws.active)
            .map(|ws| ws.id.clone())
            .unwrap_or_default()
    };
    assert_eq!(active(&seen, "HDMI-A-1"), "1");
    assert_eq!(active(&seen, "DP-3"), "4");

    let inside = |rect: &proto::Rect, (x, y, width, height, _): (i32, i32, i32, i32, f64)| {
        rect.x >= x
            && rect.y >= y
            && rect.x + rect.width <= x + width
            && rect.y + rect.height <= y + height
    };
    let window = |app: &str| {
        let seen = state();
        let window = seen.windows.iter().find(|w| w.app_id == app).unwrap();
        (window.workspace_id.clone(), window.rect.clone().unwrap())
    };
    let focused = || {
        let seen = state();
        (
            seen.focused_output.clone(),
            seen.focused_workspace_id.clone(),
        )
    };

    // Super+4 from the desk goes to the portable; the desk keeps 1 in view.
    session.chord(&[SUPER, key(1)]);
    assert_eq!(focused(), ("HDMI-A-1".into(), "1".into()));
    session.chord(&[SUPER, key(4)]);
    assert_eq!(focused(), ("DP-3".into(), "4".into()));
    assert_eq!(active(&state(), "HDMI-A-1"), "1");
    // A window opened there opens on the portable.
    session.open("foot");
    let (ws, rect) = window("foot");
    assert_eq!(ws, "4");
    assert!(inside(&rect, portable), "{rect:?} on the portable");

    // From the desk, a window whose rule says 4 opens on the portable too.
    session.chord(&[SUPER, key(1)]);
    assert_eq!(focused(), ("HDMI-A-1".into(), "1".into()));
    session.open("discord");
    let (ws, rect) = window("discord");
    assert_eq!(ws, "4");
    assert!(inside(&rect, portable), "{rect:?} on the portable");

    // Super+8 from the desk, and a desk window sent to 8, both reach the
    // portable.
    session.chord(&[SUPER, key(1)]);
    session.chord(&[SUPER, key(8)]);
    assert_eq!(focused(), ("DP-3".into(), "8".into()));
    session.chord(&[SUPER, key(2)]);
    assert_eq!(focused(), ("HDMI-A-1".into(), "2".into()));
    session.open("ghostty");
    let (ws, rect) = window("ghostty");
    assert_eq!(ws, "2");
    assert!(inside(&rect, desk), "{rect:?} on the desk");
    session.chord(&[SUPER, SHIFT, key(8)]);
    let (ws, rect) = window("ghostty");
    assert_eq!(ws, "8");
    session.chord(&[SUPER, key(8)]);
    let (_, rect_after) = window("ghostty");
    assert!(
        inside(&rect_after, portable),
        "{rect:?} then {rect_after:?} on the portable"
    );

    // VRR as each description-matched entry asks, in what the core is told:
    // the desk's 2 with any window fullscreen there, the panel's 3 only with
    // a game or video, the portable never.
    let vrr = |session: &Session| {
        ["HDMI-A-1", "DP-3", "eDP-1"].map(|name| config(session, name).vrr.unwrap_or_default())
    };
    let toggle = |session: &mut Session| {
        ask(|engine| {
            let id = engine.model.focused_window().cloned().ok_or("no focus")?;
            engine.toggle_fullscreen(&id);
            Ok(())
        })
        .unwrap();
        session.roundtrip();
    };
    assert_eq!(vrr(&session), [false, false, false]);
    session.chord(&[SUPER, key(1)]);
    session.open("desk");
    toggle(&mut session);
    assert_eq!(
        vrr(&session),
        [true, false, false],
        "fullscreen on the desk"
    );
    toggle(&mut session);
    assert_eq!(vrr(&session), [false, false, false]);
    session.chord(&[SUPER, key(4)]);
    toggle(&mut session);
    assert!(
        state()
            .windows
            .iter()
            .any(|w| w.fullscreen && w.workspace_id == "4")
    );
    assert_eq!(
        vrr(&session),
        [false, false, false],
        "fullscreen on the portable"
    );
    toggle(&mut session);
    let panel = active(&state(), "eDP-1");
    ask(move |engine| {
        engine.model.focus_workspace(&panel);
        engine.commit();
        Ok(())
    })
    .unwrap();
    session.roundtrip();
    session.open("panel");
    toggle(&mut session);
    assert_eq!(
        vrr(&session),
        [false, false, false],
        "a window that is no game"
    );
    toggle(&mut session);
    session.open("steam_app_1");
    toggle(&mut session);
    assert_eq!(vrr(&session), [false, false, true], "a game, by its rule");
    toggle(&mut session);
    assert_eq!(vrr(&session), [false, false, false]);

    // The lid takes the panel alone.
    session.state.lid_switch_toggled(true);
    session.roundtrip();
    assert_eq!(
        config(&session, "eDP-1").mode,
        Some(RuntimeOutputMode::Disabled)
    );
    let seen = state();
    assert_eq!(placed(&seen, "HDMI-A-1"), desk);
    assert_eq!(placed(&seen, "DP-3"), portable);
    assert_eq!(
        stack(&seen, "HDMI-A-1"),
        ["1", "2", "3", "5", "6", "7", "9"]
    );
    // The panel's workspace keeps its two windows on the next output over.
    assert_eq!(stack(&seen, "DP-3"), ["4", "8", "d1"]);
}

/// Float or tile by what the client says by its first buffer, as Hyprland
/// decides at map; the owner's rules over that. Each window is drawn at the
/// size it asks for, so a float shows at it.
fn classification(session: &mut Session) {
    ask(|engine| {
        engine.model.focus_workspace("3");
        engine.commit();
        Ok(())
    })
    .unwrap();
    let row = |app: &str| {
        state()
            .windows
            .into_iter()
            .find(|window| window.app_id == app)
            .unwrap_or_else(|| panic!("{app} is not open"))
    };
    let floats = |app: &str| {
        let window = row(app);
        if app.starts_with("cls-") {
            assert_eq!(window.workspace_id, "3", "{app} opens where the focus is");
        }
        window.floating
    };
    let held = |app: &str| {
        let app = app.to_owned();
        ask(move |engine| {
            Ok(engine.windows.values().any(|win| {
                win.held.is_some()
                    && win.handle.app_id().get_untracked().as_deref() == Some(app.as_str())
            }))
        })
        .unwrap()
    };
    let big = |app: &str| {
        let app = app.to_owned();
        ask(move |engine| {
            let id = engine
                .windows
                .iter()
                .find(|(_, win)| {
                    win.handle.app_id().get_untracked().as_deref() == Some(app.as_str())
                })
                .map(|(id, _)| id.clone());
            Ok(id.and_then(|id| engine.model.big_of(&id)))
        })
        .unwrap()
    };
    // Its size, and its centre against where it should be.
    let at = |app: &str| {
        let rect = row(app).rect.unwrap();
        (
            (rect.width, rect.height),
            (rect.x + rect.width / 2, rect.y + rect.height / 2),
        )
    };
    let middle = ((WIDTH / 2.0) as i32, (HEIGHT / 2.0) as i32);
    let mut opened = Vec::new();

    let (main, main_top) = session.bare("cls-main");
    session.commit(&main);
    session.draw(&main, 400, 300);
    assert!(!floats("cls-main"), "a plain window tiles");
    let parent = at("cls-main").1;

    // xdg_toplevel.set_parent before the first commit: a transient dialog,
    // a file chooser, a portal's window.
    let (child, top) = session.bare("cls-child");
    top.set_parent(Some(&main_top));
    session.commit(&child);
    session.draw(&child, 200, 120);
    assert!(floats("cls-child"), "a window with a parent floats");
    assert_eq!(
        at("cls-child"),
        ((200, 120), parent),
        "over its parent's middle"
    );
    opened.push((child, top));

    // min == max before the first commit.
    let (fixed, top) = session.bare("cls-fixed");
    top.set_min_size(320, 240);
    top.set_max_size(320, 240);
    session.commit(&fixed);
    session.draw(&fixed, 320, 240);
    assert!(floats("cls-fixed"), "a fixed-size window floats");
    assert_eq!(
        at("cls-fixed"),
        ((320, 240), middle),
        "at its size, centred"
    );
    opened.push((fixed, top));

    // GTK 3's message dialog, Electron's showErrorBox among them: no
    // parent, and min == max only after the first configure.
    let (late, top) = session.bare("cls-errorbox");
    session.commit(&late);
    assert!(!floats("cls-errorbox"), "placed before it said it is fixed");
    top.set_min_size(560, 249);
    top.set_max_size(560, 249);
    session.draw(&late, 560, 249);
    assert!(floats("cls-errorbox"), "fixed by its first buffer: floats");
    assert_eq!(
        at("cls-errorbox"),
        ((560, 249), middle),
        "at its size, centred"
    );
    opened.push((late, top));

    // A parent set after the first configure counts the same.
    let (adopted, top) = session.bare("cls-adopted");
    session.commit(&adopted);
    top.set_parent(Some(&main_top));
    session.draw(&adopted, 240, 160);
    assert!(floats("cls-adopted"), "a parent by the first buffer floats");
    opened.push((adopted, top));

    // A resizable child whose parent comes after the first configure has
    // drawn its first buffer at the tile's size. It is asked again with no
    // size, out of sight, and shown at the size it picks.
    let (child, top) = session.bare("cls-late-child");
    session.commit(&child);
    let tile = session.configured(&top);
    assert!(
        tile.0 > 300 && tile.1 > 300,
        "a tile is told its size: {tile:?}"
    );
    top.set_parent(Some(&main_top));
    session.draw(&child, tile.0, tile.1);
    assert!(
        floats("cls-late-child"),
        "a parent by the first buffer floats"
    );
    assert_eq!(session.configured(&top), (0, 0), "asked for its own size");
    assert!(held("cls-late-child"), "not shown at the tile's size");
    session.draw(&child, 300, 200);
    assert!(!held("cls-late-child"));
    assert_eq!(
        at("cls-late-child"),
        ((300, 200), parent),
        "at the size it picked, over its parent"
    );
    opened.push((child, top));

    // Fixed in both axes by its first buffer, but that buffer is still the
    // tile's: it is held until it draws the fixed size.
    let (stale, top) = session.bare("cls-stale-fixed");
    session.commit(&stale);
    let tile = session.configured(&top);
    top.set_min_size(tile.0, 200);
    top.set_max_size(tile.0, 200);
    session.draw(&stale, tile.0, tile.1);
    assert!(
        floats("cls-stale-fixed"),
        "fixed by its first buffer: floats"
    );
    assert!(
        held("cls-stale-fixed"),
        "a buffer of the tile's size is not its fixed size"
    );
    session.draw(&stale, tile.0, 200);
    assert!(!held("cls-stale-fixed"));
    assert_eq!(at("cls-stale-fixed").0, (tile.0, 200), "at its fixed size");
    opened.push((stale, top));

    // A maximize asked before the first configure and taken back before
    // the first buffer stays taken back when the window floats by it.
    let (unmaximized, top) = session.bare("cls-unmaximized");
    top.set_maximized();
    session.commit(&unmaximized);
    assert_eq!(big("cls-unmaximized"), Some(Big::Maximized));
    top.unset_maximized();
    session.roundtrip();
    assert_eq!(big("cls-unmaximized"), None);
    top.set_parent(Some(&main_top));
    session.draw(&unmaximized, 240, 160);
    assert!(
        floats("cls-unmaximized"),
        "a parent by the first buffer floats"
    );
    assert_eq!(big("cls-unmaximized"), None, "not maximized again");
    opened.push((unmaximized, top));

    // xdg-dialog-v1's modal hint has no effect without a parent.
    let dialogs: XdgWmDialogV1 = session.bind("xdg_wm_dialog_v1");
    let (modal, top) = session.bare("cls-modal");
    let hint = dialogs.get_xdg_dialog(&top, &session.queue.handle(), ());
    hint.set_modal();
    session.commit(&modal);
    session.draw(&modal, 300, 180);
    assert!(!floats("cls-modal"), "a modal hint without a parent tiles");
    opened.push((modal, top));

    // Once drawn, the decision stands: a tile does not jump to a float.
    let (shown, top) = session.bare("cls-shown");
    session.commit(&shown);
    session.draw(&shown, 400, 300);
    top.set_min_size(400, 300);
    top.set_max_size(400, 300);
    session.draw(&shown, 400, 300);
    assert!(
        !floats("cls-shown"),
        "fixed after its first buffer: still tiled"
    );
    opened.push((shown, top));

    // The owner's rule wins: the example's Beeper rule says `float: false`.
    let (beeper, top) = session.bare("Beeper");
    session.commit(&beeper);
    top.set_min_size(500, 400);
    top.set_max_size(500, 400);
    session.draw(&beeper, 500, 400);
    assert!(!floats("Beeper"), "a rule's float: false over a fixed size");
    opened.push((beeper, top));

    // A rule that matches only once the window floats, by a parent set
    // after the first configure: its size and workspace apply then.
    ask(|engine| {
        std::rc::Rc::make_mut(&mut engine.settings)
            .rules
            .push(rules::Rule {
                matcher: rules::Matcher {
                    app_id: Some(rules::Pattern::new("ruled-dialog")?),
                    floating: Some(true),
                    ..Default::default()
                },
                effect: rules::Effect {
                    workspace: Some(2),
                    size: Some((rules::Length::Px(500.0), rules::Length::Px(300.0))),
                    ..Default::default()
                },
            });
        Ok(())
    })
    .unwrap();
    let (ruled, top) = session.bare("ruled-dialog");
    session.commit(&ruled);
    let tile = session.configured(&top);
    top.set_parent(Some(&main_top));
    session.draw(&ruled, tile.0, tile.1);
    assert!(row("ruled-dialog").floating);
    assert_eq!(
        row("ruled-dialog").workspace_id,
        "2",
        "the rule's workspace"
    );
    assert_eq!(session.configured(&top), (500, 300), "told the rule's size");
    assert_eq!(at("ruled-dialog").0, (500, 300), "at the rule's size");
    opened.push((ruled, top));
    ask(|engine| {
        std::rc::Rc::make_mut(&mut engine.settings).rules.pop();
        Ok(())
    })
    .unwrap();

    hint.destroy();
    for (_, top) in opened.iter().chain([&(main, main_top)]) {
        top.destroy();
    }
    session.roundtrip();
    assert!(
        state()
            .windows
            .iter()
            .all(|window| !window.app_id.starts_with("cls-")
                && !["Beeper", "ruled-dialog"].contains(&window.app_id.as_str()))
    );
}

/// A window as the engine has it and as it tells the compositor to show it.
struct Seen {
    pip: bool,
    pinned: bool,
    floating: bool,
    output: String,
    /// Global logical: where the model rests it and where it is drawn now.
    rect: formalwm_core::geom::Rect,
    drawn: formalwm_core::geom::Rect,
    layer: formalwm_core::model::Layer,
    z: i32,
    idle: bool,
    live: bool,
    above_fullscreen: bool,
}

fn id_of(title: &str) -> String {
    ask(|engine| {
        let found = engine.model.windows.iter().find(|w| w.title == title);
        found.map(|w| w.id.clone()).ok_or(format!("no window {title}"))
    })
    .unwrap()
}

fn look(id: &str) -> Seen {
    use crate::windows::{IDLE, LIVE, PIP, Z};
    ask(|engine| {
        let record = engine.model.window(id).ok_or("not in the model")?;
        let visual = engine.scene.visual(id).ok_or("not in the scene")?;
        let win = engine.windows.get(id).ok_or("not a window")?;
        let output = engine.model.output_of(id).ok_or("on no output")?;
        let origin = engine.model.output(&output).ok_or("no output")?.geo.rect;
        Ok(Seen {
            pip: record.pip,
            pinned: record.pinned,
            floating: record.floating,
            output,
            rect: record.float_rect.translate(origin.x, origin.y),
            drawn: visual.rect,
            layer: visual.layer,
            z: win.handle.state(&Z).get_untracked(),
            idle: win.handle.state(&IDLE).get_untracked(),
            live: win.handle.state(&LIVE).get_untracked(),
            above_fullscreen: win.handle.state(&PIP).get_untracked(),
        })
    })
    .unwrap()
}

/// What an output leaves a picture: its work area less the gap it keeps.
fn picture_area(output: &'static str) -> formalwm_core::geom::Rect {
    ask(|engine| {
        let options = &engine.model.options;
        let usable = engine.model.output(output).ok_or("no output")?.geo.usable;
        Ok(usable.inset(options.gaps_out + options.border))
    })
    .unwrap()
}

/// The pointer takes a picture at `from` and lets it go after `path`: how
/// far it has been moved, and when in milliseconds.
fn dragged(id: &str, from: formalwm_core::geom::Rect, path: &[((f64, f64), f64)]) {
    use shojiwm_rs::ssd::WindowMovePhaseSnapshot as Move;
    ask(|engine| {
        let hand = |(dx, dy): (f64, f64)| (from.x + 20.0 + dx, from.y + 20.0 + dy);
        engine.pip_move(id, from, hand((0.0, 0.0)), Move::Start, 0.0);
        let mut last = ((0.0, 0.0), 0.0);
        for (by, at) in path {
            last = (*by, *at);
            engine.pip_move(id, from.translate(by.0, by.1), hand(*by), Move::Update, *at);
        }
        let (by, at) = last;
        engine.pip_move(id, from.translate(by.0, by.1), hand(by), Move::End, at);
        Ok(())
    })
    .unwrap()
}

#[test]
fn picture_in_picture() {
    use formalwm_core::model::{Big, Layer};
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // The example's settings less its rule for the player: what is left
    // names no picture in picture.
    let settings: String = include_str!("../../example/settings.json")
        .lines()
        .filter(|line| !line.contains("[Ii]n[ -][Pp]icture"))
        .map(|line| format!("{line}\n"))
        .collect();
    let mut session = Session::with_settings(
        "pip",
        HOST,
        &[test_display("TEST-1"), test_display("TEST-2")],
        &settings,
    );
    let on_a_corner = |seen: &Seen, output: &'static str| {
        let area = picture_area(output);
        let rect = seen.rect;
        (rect.x == area.x.round() || (rect.right() - area.right()).abs() < 1.0)
            && (rect.y == area.y.round() || (rect.bottom() - area.bottom()).abs() < 1.0)
    };
    let focused = || state().windows.iter().find(|w| w.focused).map(|w| w.title.clone());

    // Chromium's player says it with its title alone: no rule of the
    // example's names it.
    let term = session.open("term");
    session.draw(&term, 400, 300);
    let player = session.open_titled("chromium", "Picture in picture");
    session.draw(&player, 480, 270);
    session.caught_up();
    let (pip, term_id) = (id_of("Picture in picture"), id_of("term"));
    let seen = look(&pip);
    assert!(
        seen.pip && seen.pinned && seen.floating,
        "the player is a picture: pip {} pinned {} floating {}",
        seen.pip,
        seen.pinned,
        seen.floating
    );
    assert_eq!(seen.layer, Layer::Pip);
    assert_eq!((seen.rect.w, seen.rect.h), (480.0, 270.0), "at the size it asked for");
    let area = picture_area("TEST-1");
    assert_eq!(
        (seen.rect.right(), seen.rect.bottom()),
        (area.right(), area.bottom()),
        "in the bottom right corner of what the layers leave free"
    );
    assert!(seen.z > look(&term_id).z);
    assert_eq!(focused().as_deref(), Some("term"), "it opens without the keyboard");

    // A page that only starts like one is a window like any other.
    let page = session.open_titled("helium", "Picture in picture - Wikipedia");
    session.draw(&page, 400, 300);
    let page_id = id_of("Picture in picture - Wikipedia");
    assert!(!look(&page_id).pip && !look(&page_id).floating);

    // Another workspace of its output: it is there too, over what that shows.
    ask(|engine| {
        engine.model.focus_workspace("2");
        engine.commit();
        Ok(())
    })
    .unwrap();
    session.caught_up();
    let game = session.open("game");
    session.draw(&game, 400, 300);
    session.caught_up();
    let game_id = id_of("game");
    let seen = look(&pip);
    assert!(!seen.idle && seen.live, "shown on the workspace switched to");
    assert_eq!((seen.rect.right(), seen.rect.bottom()), (area.right(), area.bottom()));
    assert!(look(&term_id).idle, "the workspace left is not drawn");
    let home = |id: &str| {
        let id = ask(|engine| Ok(engine.public(id))).unwrap();
        state().windows.iter().find(|w| w.id == id).map(|w| w.workspace_id.clone())
    };
    assert_eq!(home(&pip).as_deref(), Some("2"));

    // Over a fullscreen window, which stays fullscreen for what that turns
    // off on its output.
    ask(|engine| {
        engine.toggle_fullscreen(&game_id);
        Ok(())
    })
    .unwrap();
    // The request comes back to the engine, its rect goes to the client,
    // and the client draws at it: fullscreen as committed.
    for _ in 0..3 {
        session.roundtrip();
        session.state.refresh_window_decorations().unwrap();
    }
    session.draw(&game, WIDTH as i32, HEIGHT as i32);
    session.caught_up();
    assert_eq!(
        ask(|engine| Ok(engine.model.big_of(&game_id))).unwrap(),
        Some(Big::Fullscreen)
    );
    let seen = look(&pip);
    assert!(seen.z > look(&game_id).z, "over the fullscreen window");
    assert!(seen.above_fullscreen && !seen.idle && seen.live);
    let first = session
        .state
        .space
        .outputs()
        .find(|output| output.name() == "TEST-1")
        .cloned()
        .unwrap();
    assert!(session.state.output_has_fullscreen_window_under_pictures(&first));
    let under = |session: &Session, at: (f64, f64)| {
        session
            .state
            .surface_under(at.into())
            .map(|(surface, _)| smithay::reexports::wayland_server::Resource::id(&surface).protocol_id())
    };
    let centre = seen.rect.center();
    assert_eq!(under(&session, centre), Some(player.id().protocol_id()));

    // The locked session: no surface of a window is under a finger, and
    // the engine hears nothing of the touch.
    session.state.set_session_lock_active(true);
    assert!(session.state.session_lock_surface_under(centre.into()).is_none());
    session.state.inject_touch(VirtualTouch::Down {
        id: 0,
        x: centre.0,
        y: centre.1,
    });
    session.roundtrip();
    assert!(ask(|engine| Ok(engine.pip.untouched())).unwrap());
    session.state.inject_touch(VirtualTouch::Up { id: 0 });
    session.state.set_session_lock_active(false);
    session.roundtrip();
    ask(|engine| {
        engine.toggle_fullscreen(&game_id);
        Ok(())
    })
    .unwrap();
    // The request comes back to the engine, the engine's rect goes to the
    // client, and the client draws at it.
    for _ in 0..3 {
        session.roundtrip();
        session.state.refresh_window_decorations().unwrap();
    }
    session.draw(&game, 400, 300);
    session.caught_up();
    assert!(!session.state.output_has_fullscreen_window_under_pictures(&first));

    // The overview leaves it where it is, and out of reach while it is open.
    ask(|engine| {
        engine.set_overview(Some(true));
        engine.overview.progress.jump(1.0);
        engine.present();
        Ok(())
    })
    .unwrap();
    session.roundtrip();
    let seen = look(&pip);
    assert_eq!((seen.rect.right(), seen.rect.bottom()), (area.right(), area.bottom()));
    assert!(!seen.live && !seen.idle);
    ask(|engine| {
        engine.set_overview(Some(false));
        engine.overview.progress.jump(0.0);
        engine.present();
        Ok(())
    })
    .unwrap();
    session.roundtrip();

    // The pointer on its body: a press is the client's, and past the slop
    // of a click the compositor takes the picture, since a player has no
    // title bar to hold.
    let before = look(&pip).rect;
    let centre = before.center();
    session.pointer_to(centre.0, centre.1);
    session.button(ButtonState::Pressed);
    session.pointer_by(-4.0, -3.0);
    session.state.refresh_window_decorations().unwrap();
    session.roundtrip();
    assert_eq!(look(&pip).rect, before, "inside the slop it stays the client's press");
    for _ in 0..4 {
        session.pointer_by(-30.0, -20.0);
        session.state.refresh_window_decorations().unwrap();
        session.roundtrip();
    }
    let held = look(&pip).rect;
    assert!(
        held.x <= before.x - 60.0 && held.y <= before.y - 40.0,
        "under the pointer: {held:?} from {before:?}"
    );
    // The compositor stamps a move with its own clock, so the hand rests
    // for longer than the velocity is measured over before it lets go.
    std::thread::sleep(Duration::from_millis(150));
    session.button(ButtonState::Released);
    session.state.refresh_window_decorations().unwrap();
    session.roundtrip();
    session.caught_up();
    assert!(on_a_corner(&look(&pip), "TEST-1"));

    // Dragged across and let go at rest: the nearest corner, on a spring.
    let from = look(&pip).rect;
    dragged(&pip, from, &[((-200.0, -150.0), 400.0), ((-420.0, -380.0), 800.0)]);
    let seen = look(&pip);
    assert_eq!((seen.rect.x, seen.rect.y), (area.x, area.y), "the top left corner");
    assert_ne!(seen.drawn, seen.rect, "on its way there, not there");
    // Caught on the way: it goes on from where it is drawn.
    let caught = seen.drawn;
    ask(|engine| {
        use shojiwm_rs::ssd::WindowMovePhaseSnapshot as Move;
        engine.pip_move(&pip, caught, (caught.x + 20.0, caught.y + 20.0), Move::Start, 0.0);
        Ok(())
    })
    .unwrap();
    assert_eq!(look(&pip).drawn, caught);
    dragged(&pip, caught, &[((-10.0, -10.0), 400.0)]);
    session.caught_up();

    // Flicked at the edge no output is behind: stashed, a tab left on screen.
    let from = look(&pip).rect;
    dragged(&pip, from, &[((-40.0, 0.0), 16.0), ((-90.0, 0.0), 32.0)]);
    session.caught_up();
    let seen = look(&pip);
    assert_eq!(seen.rect.right(), formalwm_core::pip::TAB, "all but the tab is off screen");
    assert_ne!(focused().as_deref(), Some("Picture in picture"));
    // A press on the tab brings it back.
    session.state.refresh_window_decorations().unwrap();
    session.roundtrip();
    session.pointer_to(10.0, seen.rect.y + 40.0);
    session.click();
    session.caught_up();
    let seen = look(&pip);
    assert_eq!(seen.rect.x, area.x);
    assert!(on_a_corner(&seen, "TEST-1"));

    // Flicked at the edge another output is behind: it lands on that one.
    let from = seen.rect;
    dragged(&pip, from, &[((60.0, 0.0), 16.0), ((140.0, 0.0), 32.0)]);
    session.caught_up();
    let seen = look(&pip);
    assert_eq!(seen.output, "TEST-2");
    assert!(on_a_corner(&seen, "TEST-2"));

    // Two fingers on the touchpad, the pointer on the picture: it grows
    // about the pointer and keeps its shape.
    let before = seen.rect;
    let centre = before.center();
    session.pointer_to(centre.0, centre.1);
    session.state.inject_pinch(VirtualPinch::Begin { fingers: 2 });
    session.state.inject_pinch(VirtualPinch::Update {
        scale: 1.25,
        dx: 0.0,
        dy: 0.0,
    });
    session.roundtrip();
    let seen = look(&pip);
    assert_eq!((seen.rect.w, seen.rect.h), (600.0, 338.0));
    assert!((seen.rect.center().0 - centre.0).abs() <= 1.0);
    // Past the largest it can be, the client stays at the limit and the
    // picture is drawn stretched a little further.
    session.state.inject_pinch(VirtualPinch::Update {
        scale: 2.0,
        dx: 0.0,
        dy: 0.0,
    });
    session.roundtrip();
    let seen = look(&pip);
    let most = (picture_area("TEST-2").w * 0.6).round();
    assert_eq!(seen.rect.w, most);
    assert!(seen.drawn.w > most + 20.0 && seen.drawn.w < most * 1.35);
    // Let go: back to the limit, in a corner.
    session.state.inject_pinch(VirtualPinch::End { cancelled: false });
    session.roundtrip();
    session.caught_up();
    let seen = look(&pip);
    assert_eq!((seen.rect.w, seen.drawn), (most, seen.rect));
    assert!(on_a_corner(&seen, "TEST-2"));
    // The pointer anywhere else: a pinch is not the picture's.
    session.pointer_to(100.0, 100.0);
    session.state.inject_pinch(VirtualPinch::Begin { fingers: 2 });
    session.state.inject_pinch(VirtualPinch::Update {
        scale: 0.5,
        dx: 0.0,
        dy: 0.0,
    });
    session.state.inject_pinch(VirtualPinch::End { cancelled: false });
    session.roundtrip();
    assert_eq!(look(&pip).rect, seen.rect);

    // A finger on the screen: a tap is the client's.
    let centre = seen.rect.center();
    let finger = |session: &mut Session, touch: VirtualTouch| {
        session.state.inject_touch(touch);
        session.roundtrip();
    };
    let at = |id: u32, by: (f64, f64)| (id, centre.0 + by.0, centre.1 + by.1);
    let (id, x, y) = at(0, (0.0, 0.0));
    finger(&mut session, VirtualTouch::Down { id, x, y });
    let (id, x, y) = at(0, (4.0, 3.0));
    finger(&mut session, VirtualTouch::Motion { id, x, y });
    assert!(!session.state.touch.taken(), "inside the slop the touch is the client's");
    finger(&mut session, VirtualTouch::Up { id: 0 });
    assert_eq!(look(&pip).rect, seen.rect);
    assert!(ask(|engine| Ok(engine.pip.untouched())).unwrap());

    // Past the slop it is a drag: the client is told the touch is over and
    // the picture is under the finger.
    let (id, x, y) = at(0, (0.0, 0.0));
    finger(&mut session, VirtualTouch::Down { id, x, y });
    let (id, x, y) = at(0, (-6.0, -4.0));
    finger(&mut session, VirtualTouch::Motion { id, x, y });
    let (id, x, y) = at(0, (-60.0, -30.0));
    finger(&mut session, VirtualTouch::Motion { id, x, y });
    assert!(session.state.touch.taken());
    let held = look(&pip).rect;
    assert_eq!((held.x, held.y), (seen.rect.x - 60.0, seen.rect.y - 30.0));
    finger(&mut session, VirtualTouch::Up { id: 0 });
    assert!(!session.state.touch.taken(), "the next touch is the clients' again");
    session.caught_up();
    let seen = look(&pip);
    assert!(on_a_corner(&seen, "TEST-1") || on_a_corner(&seen, "TEST-2"));

    // Two fingers on it: it shrinks to how far apart they are.
    let centre = seen.rect.center();
    let at = |id: u32, by: (f64, f64)| (id, centre.0 + by.0, centre.1 + by.1);
    let (id, x, y) = at(0, (-50.0, 0.0));
    finger(&mut session, VirtualTouch::Down { id, x, y });
    let (id, x, y) = at(1, (50.0, 0.0));
    finger(&mut session, VirtualTouch::Down { id, x, y });
    assert!(session.state.touch.taken());
    let (id, x, y) = at(1, (0.0, 0.0));
    finger(&mut session, VirtualTouch::Motion { id, x, y });
    assert_eq!(look(&pip).rect.w, (most / 2.0).round());
    finger(&mut session, VirtualTouch::Up { id: 1 });
    finger(&mut session, VirtualTouch::Up { id: 0 });
    session.caught_up();
    let seen = look(&pip);
    assert_eq!(seen.rect.w, (most / 2.0).round());

    // Firefox's spelling. The next picture opens the size the last was left.
    let second = session.open_titled("firefox", "Picture-in-Picture");
    session.draw(&second, 640, 360);
    session.caught_up();
    let other = look(&id_of("Picture-in-Picture"));
    assert!(other.pip);
    assert_eq!(other.rect.w, (most / 2.0).round());
    assert!(
        other.output != seen.output || (other.rect.x, other.rect.y) != (seen.rect.x, seen.rect.y),
        "in a corner the first one is not in"
    );

    // Its output goes: it is in a corner of one that is left.
    let on = seen.output.clone();
    let lid = on == "TEST-2";
    if !lid {
        dragged(&pip, seen.rect, &[((60.0, 0.0), 16.0), ((140.0, 0.0), 32.0)]);
        session.caught_up();
    }
    assert_eq!(look(&pip).output, "TEST-2");
    session.state.lid_switch_toggled(true);
    assert!(session.until(|| look(&pip).output == "TEST-1"));
    session.caught_up();
    assert!(on_a_corner(&look(&pip), "TEST-1"));

    // What the shell draws is over it: a layer on top takes the press.
    let (_, bar) = session.layer();
    session.draw(&bar, WIDTH as i32, 40);
    let from = look(&pip).rect;
    ask(|engine| {
        use shojiwm_rs::ssd::WindowMovePhaseSnapshot as Move;
        let top = formalwm_core::geom::Rect::new(from.x, 0.0, from.w, from.h);
        engine.pip_move(&pip, top, (top.x + 20.0, 20.0), Move::Start, 0.0);
        Ok(())
    })
    .unwrap();
    session.state.refresh_window_decorations().unwrap();
    session.roundtrip();
    let seen = look(&pip);
    let point = (seen.rect.x + 30.0, 10.0);
    assert_eq!(under(&session, point), Some(bar.id().protocol_id()));
    let below = under(&session, (seen.rect.x + 30.0, 60.0));
    let pictures = [player.id().protocol_id(), second.id().protocol_id()];
    assert!(below.is_some_and(|surface| pictures.contains(&surface)));
}

/// A rule that tiles a window keeps it a tile whenever it says it is a
/// picture, and what the shell draws over a picture has the press, the
/// finger and the pinch that land on it.
#[test]
fn the_rules_and_the_shell_come_before_a_picture() {
    use formalwm_core::model::Layer;
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let settings: String = include_str!("../../example/settings.json")
        .lines()
        .filter(|line| !line.contains("[Ii]n[ -][Pp]icture"))
        .map(|line| format!("{line}\n"))
        .collect();
    let mut session = Session::with_settings("pip-shell", HOST, &[test_display("TEST-1")], &settings);
    let renamed = |session: &mut Session, top: &xdg_toplevel::XdgToplevel, title: &str| {
        top.set_title(title.into());
        for _ in 0..3 {
            session.roundtrip();
            session.state.refresh_window_decorations().unwrap();
        }
        session.caught_up();
    };

    // The example tiles Beeper (`"float": false`). Its window is a tile
    // with the player's title from the start, and with it as a first title
    // after it opened.
    let early = session.open_titled("Beeper", "Picture in picture");
    session.draw(&early, 480, 270);
    session.caught_up();
    let seen = look(&id_of("Picture in picture"));
    assert!(!seen.pip && !seen.floating && !seen.pinned, "titled from the start");
    let (late, top) = session.untitled("Beeper");
    session.draw(&late, 480, 270);
    session.caught_up();
    renamed(&mut session, &top, "Picture-in-Picture");
    let seen = look(&id_of("Picture-in-Picture"));
    assert!(!seen.pip && !seen.floating && !seen.pinned, "titled after it opened");
    // With no rule against it the late title makes a picture.
    let (player, top) = session.untitled("late-player");
    session.draw(&player, 480, 270);
    session.caught_up();
    renamed(&mut session, &top, "PICTURE IN PICTURE");
    let pip = id_of("PICTURE IN PICTURE");
    let seen = look(&pip);
    assert!(seen.pip && seen.pinned && seen.layer == Layer::Pip);

    // A fullscreen window under the picture and the shell's bar on top:
    // the bar is drawn over both, so it has the press.
    let game = session.open("game");
    session.draw(&game, 400, 300);
    let game_id = id_of("game");
    ask(|engine| {
        engine.toggle_fullscreen(&game_id);
        Ok(())
    })
    .unwrap();
    // The request comes back to the engine, its rect goes to the client,
    // and the client draws at it: fullscreen as committed.
    for _ in 0..3 {
        session.roundtrip();
        session.state.refresh_window_decorations().unwrap();
    }
    session.draw(&game, WIDTH as i32, HEIGHT as i32);
    session.caught_up();
    let (_, bar) = session.layer();
    session.draw(&bar, WIDTH as i32, 40);
    let first = session.state.space.outputs().next().cloned().unwrap();
    let under = |session: &Session, at: (f64, f64)| {
        session
            .state
            .surface_under(at.into())
            .map(|(surface, _)| smithay::reexports::wayland_server::Resource::id(&surface).protocol_id())
    };
    let seen = look(&pip);
    assert!(seen.z > look(&game_id).z && seen.above_fullscreen);
    assert!(session.state.output_has_fullscreen_window_under_pictures(&first));
    assert!(!session.state.output_has_topmost_fullscreen_window(&first));
    assert_eq!(under(&session, (30.0, 10.0)), Some(bar.id().protocol_id()));
    assert_eq!(under(&session, (30.0, 60.0)), Some(game.id().protocol_id()));
    let centre = seen.rect.center();
    assert_eq!(under(&session, centre), Some(player.id().protocol_id()));

    // A layer of the shell's over the picture: a finger dragged on it, a
    // press dragged on it and a pinch over it are the layer's.
    let (cover, lid) = session.cover();
    session.draw(&lid, WIDTH as i32, HEIGHT as i32);
    assert_eq!(under(&session, centre), Some(lid.id().protocol_id()));
    let finger = |session: &mut Session, touch: VirtualTouch| {
        session.state.inject_touch(touch);
        session.roundtrip();
    };
    let (x, y) = centre;
    finger(&mut session, VirtualTouch::Down { id: 0, x, y });
    finger(&mut session, VirtualTouch::Motion { id: 0, x: x - 6.0, y: y - 4.0 });
    finger(&mut session, VirtualTouch::Motion { id: 0, x: x - 60.0, y: y - 30.0 });
    assert!(!session.state.touch.taken(), "the layer keeps its touch");
    finger(&mut session, VirtualTouch::Up { id: 0 });
    session.pointer_to(x, y);
    session.button(ButtonState::Pressed);
    for _ in 0..4 {
        session.pointer_by(-30.0, -20.0);
        session.state.refresh_window_decorations().unwrap();
        session.roundtrip();
    }
    session.button(ButtonState::Released);
    session.pointer_to(x, y);
    session.state.inject_pinch(VirtualPinch::Begin { fingers: 2 });
    session.state.inject_pinch(VirtualPinch::Update {
        scale: 0.5,
        dx: 0.0,
        dy: 0.0,
    });
    session.roundtrip();
    assert!(ask(|engine| Ok(engine.pip.unpinched())).unwrap());
    session.state.inject_pinch(VirtualPinch::End { cancelled: false });
    session.roundtrip();
    session.caught_up();
    let after = look(&pip);
    assert_eq!((after.rect, after.drawn), (seen.rect, seen.rect), "it never moved");
    assert!(ask(|engine| Ok(engine.pip.untouched())).unwrap());
    cover.destroy();
    lid.destroy();
    session.roundtrip();
    assert_eq!(under(&session, centre), Some(player.id().protocol_id()));

    // Two fingers pinch it, a third comes down beside it, and the device
    // gives the touch up, the third finger's end told first: the pinch is
    // over and the picture is let go.
    finger(&mut session, VirtualTouch::Down { id: 1, x: x - 50.0, y });
    finger(&mut session, VirtualTouch::Down { id: 2, x: x + 50.0, y });
    assert!(session.state.touch.taken());
    finger(&mut session, VirtualTouch::Down { id: 0, x: 100.0, y: 400.0 });
    finger(&mut session, VirtualTouch::Motion { id: 2, x: x + 30.0, y });
    assert!(!ask(|engine| Ok(engine.pip.unpinched())).unwrap());
    assert_ne!(look(&pip).drawn.w, seen.rect.w, "held and pinched");
    finger(&mut session, VirtualTouch::Cancel);
    assert!(ask(|engine| Ok(engine.pip.unpinched() && engine.pip.untouched())).unwrap());
    session.caught_up();
    let after = look(&pip);
    assert_eq!(after.drawn, after.rect, "at rest, not held");
}

/// The example's rule for the player still says where it opens and how big;
/// it is a picture in picture all the same, and a hand takes it from there.
#[test]
fn a_rule_places_a_picture_in_picture() {
    use formalwm_core::model::Layer;
    let _turn = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut session = Session::new("pip-rule", HOST, &[test_display("TEST-1")]);
    let term = session.open("term");
    session.draw(&term, 400, 300);
    let player = session.open_titled("", "Picture in picture");
    session.draw(&player, 480, 270);
    session.caught_up();
    let pip = id_of("Picture in picture");
    let seen = look(&pip);
    assert!(seen.pip && seen.pinned && seen.layer == Layer::Pip);
    let (w, h) = ((WIDTH * 0.28).round(), (HEIGHT * 0.28).round());
    assert_eq!((seen.rect.w, seen.rect.h), (w, h), "the rule's size");
    assert_eq!(
        (seen.rect.right(), seen.rect.bottom()),
        (WIDTH - 16.0, HEIGHT - 16.0),
        "the rule's corner and inset"
    );
    dragged(&pip, seen.rect, &[((-300.0, -200.0), 400.0), ((-600.0, -450.0), 800.0)]);
    session.caught_up();
    let (seen, area) = (look(&pip), picture_area("TEST-1"));
    assert_eq!((seen.rect.x, seen.rect.y), (area.x, area.y));
}
