//! The engine on a compositor with no backend, two outputs and a real
//! client connected to it, for what the rig has no hardware for: a host's
//! workspaces on their outputs, the touchpad's swipes and the lid.
//!
//! The session is ShojiWM's own test harness
//! (`src/shojiwm_rs/tests/common/mod.rs`), MIT, Copyright (c) 2026 bea4dev.

use std::{
    fs::File,
    os::{fd::AsFd, unix::net::UnixStream},
    time::Duration,
};

use fs_wm_proto as proto;
use shojiwm_lib::{
    config::{RuntimeHdrConfig, RuntimeHdrMode, RuntimeOutputMode},
    state::ShojiWM,
    virtual_input::{VirtualDevice, VirtualInput, VirtualMotion},
};
use shojiwm_rs::{
    COMPOSITOR, ConfigBuilder, RuntimeBoot,
    cli::CommonArgs,
    ssd::{GestureSwipeEventSnapshot, GestureSwipePhaseSnapshot as Phase},
};
use smithay::{
    backend::input::InputEvent,
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::{calloop::EventLoop, wayland_server::Display},
    utils::SERIAL_COUNTER,
};
use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{wl_buffer, wl_compositor, wl_registry, wl_shm, wl_shm_pool, wl_surface},
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};
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

fn output(name: &str, x: i32) -> Output {
    let output = Output::new(
        name.into(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "test".into(),
            model: "test".into(),
            serial_number: String::new(),
        },
    );
    let mode = Mode {
        size: (WIDTH as i32, HEIGHT as i32).into(),
        refresh: 60_000,
    };
    output.change_current_state(Some(mode), None, None, Some((x, 0).into()));
    output.set_preferred(mode);
    output
}

impl Session {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("formalwm-headless-{}", std::process::id()));
        let (runtime, config) = (root.join("run"), root.join("config/formalwm"));
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(&config).unwrap();
        std::fs::write(
            config.join("settings.json"),
            include_str!("../../example/settings.json"),
        )
        .unwrap();
        std::fs::write(config.join("host.json"), HOST).unwrap();
        // SAFETY: the one test of this binary sets these before any thread
        // reads them.
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
        for (name, x) in [("TEST-1", 0), ("TEST-2", WIDTH as i32)] {
            let output = output(name, x);
            state.create_output_global(&output);
            state.space.map_output(&output, (x, 0));
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
        let qh = self.queue.handle();
        let compositor: wl_compositor::WlCompositor = self.bind("wl_compositor");
        let shell: xdg_wm_base::XdgWmBase = self.bind("xdg_wm_base");
        let surface = compositor.create_surface(&qh, ());
        let toplevel = shell
            .get_xdg_surface(&surface, &qh, ())
            .get_toplevel(&qh, ());
        toplevel.set_app_id(app_id.into());
        toplevel.set_title(app_id.into());
        surface.commit();
        self.roundtrip();
        surface
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
    xdg_toplevel::XdgToplevel,
    zwlr_layer_shell_v1::ZwlrLayerShellV1,
    zwlr_layer_surface_v1::ZwlrLayerSurfaceV1
);

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

fn scroll(workspace: &'static str) -> (f64, f64) {
    ask(move |engine| {
        let model = engine.model.workspace(workspace).unwrap().tape.scroll;
        Ok((model, engine.scene.scroll_of(workspace)))
    })
    .unwrap()
}

/// One test: the engine is one per thread and reads the process's
/// environment.
#[test]
fn two_outputs_and_the_touchpad() {
    let mut session = Session::new();

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
        Ok((overview.progress.value, overview.held, overview.open))
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
