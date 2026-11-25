//! The mirror window: borderless, the phone's screen at its true size on
//! the monitor, in the display's own rounded shape with a soft shadow, a
//! hover strip above it. A new picture is drawn on the compositor's next
//! frame callback, so the newest one is always the one shown.

mod gpu;
mod keys;
mod overlay;

use std::collections::BTreeSet;
use std::io::Read;
use std::sync::Arc;
use std::time::{Duration, Instant};

use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::data_device_manager::data_device::{DataDevice, DataDeviceHandler};
use smithay_client_toolkit::data_device_manager::data_offer::{DataOfferHandler, DragOffer};
use smithay_client_toolkit::data_device_manager::data_source::DataSourceHandler;
use smithay_client_toolkit::data_device_manager::{DataDeviceManagerState, WritePipe};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::presentation_time::{PresentTime, PresentationTimeHandler, PresentationTimeState};
use calloop::signals::{Signal, Signals};
use smithay_client_toolkit::reexports::calloop::{EventLoop, LoopHandle, PostAction, channel, ping};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_data_device_manager, wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, Proxy, QueueHandle, WEnum};
use smithay_client_toolkit::reexports::protocols::wp::presentation_time::client::wp_presentation_feedback;
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers};
use smithay_client_toolkit::seat::pointer::{BTN_LEFT, PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::{XdgShell, XdgSurface};
use smithay_client_toolkit::shell::xdg::window::{DecorationMode, Window, WindowConfigure, WindowDecorations, WindowHandler};
use smithay_client_toolkit::{delegate_registry, registry_handlers};
use idevice::core_device::RotationDirection;
use vello_cpu::kurbo::{Point, Rect};

use crate::audio::Audio;
use crate::decode::{Decoder, Latest};
use crate::model::{self, Model};
use crate::session::{self, Button, Cmd, Event, Phase, Status};
use overlay::{Control, MARGIN, Overlay, Scene, Screen, TOP_MARGIN};

pub const APP_ID: &str = "formalshell-iphone-mirror";

/// A picture older than this while streaming reads as a stall.
const STALL: Duration = Duration::from_millis(1500);
/// Wheel ticks pass for a finger dragging this far, in logical pixels.
const WHEEL_STEP: f64 = 40.0;
/// A wheel drag lifts once the wheel has been still this long.
const WHEEL_IDLE: Duration = Duration::from_millis(120);

pub fn run() -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("wayland: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| format!("wayland: {e}"))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    // Before any thread starts, so every thread inherits the blocked mask: a
    // SIGTERM (the shell going away) closes the window the way its button
    // does, phone stood back up.
    let signals = Signals::new(&[Signal::SIGTERM, Signal::SIGINT, Signal::SIGHUP]).map_err(|e| e.to_string())?;
    handle.insert_source(signals, |_, _, app: &mut App| app.exit = true).map_err(|e| e.to_string())?;
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).map_err(|e| e.to_string())?;

    let compositor = CompositorState::bind(&globals, &qh).map_err(|e| format!("wl_compositor: {e}"))?;
    let xdg = XdgShell::bind(&globals, &qh).map_err(|e| format!("xdg_shell: {e}"))?;
    let surface = compositor.create_surface(&qh);
    let window = xdg.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title("iPhone");
    window.set_app_id(APP_ID);
    window.request_decoration_mode(Some(DecorationMode::Client));
    window.commit();

    // The decoder and the session run on threads of their own; the window
    // hears from them through a ping (a new picture) and a channel (status).
    let (frame_ping, frame_source) = ping::make_ping().map_err(|e| e.to_string())?;
    let latest = Latest::new(move || frame_ping.ping());
    let decoder = Decoder::start(latest.clone());
    let audio = Arc::new(Audio::start());
    let (event_tx, event_rx) = channel::channel::<Event>();
    let session = session::spawn(move |e| { let _ = event_tx.send(e); }, decoder, audio.clone());
    handle
        .insert_source(frame_source, |_, _, app: &mut App| app.new_frame())
        .map_err(|e| e.to_string())?;
    handle
        .insert_source(event_rx, |e, _, app: &mut App| {
            if let channel::Event::Msg(e) = e {
                app.session_event(e);
            }
        })
        .map_err(|e| e.to_string())?;

    let mut app = App {
        registry: RegistryState::new(&globals),
        seats: SeatState::new(&globals, &qh),
        outputs: OutputState::new(&globals, &qh),
        presentation: PresentationTimeState::bind(&globals, &qh),
        data: DataDeviceManagerState::bind(&globals, &qh).ok(),
        data_device: None,
        compositor,
        qh: qh.clone(),
        handle: handle.clone(),
        window,
        gpu: None,
        overlay: Overlay::new(),
        scene: None,
        size: (0, 0),
        scale: 1,
        phone: (0.0, 0.0),
        turn: 0,
        spin: None,
        room: (0.0, 0.0),
        model: None,
        bounds: None,
        sized: false,
        floating: false,
        left: None,
        snap: false,
        status: Status::Searching,
        asleep: false,
        frame_at: None,
        configured: false,
        frame_pending: false,
        dirty: true,
        latest,
        session,
        audio,
        pointer: None,
        keyboard: None,
        seat: None,
        cursor: Point::ZERO,
        inside: false,
        touching: false,
        hover: None,
        pressed: None,
        wheel: None,
        held: BTreeSet::new(),
        mods: Modifiers::default(),
        focused: false,
        in_flight: std::collections::VecDeque::new(),
        presented: Vec::new(),
        exit: false,
    };
    while !app.exit {
        let timeout = app.wheel.as_ref().map(|w| WHEEL_IDLE.saturating_sub(w.last.elapsed())).unwrap_or(Duration::from_millis(500));
        event_loop.dispatch(Some(timeout), &mut app).map_err(|e| e.to_string())?;
        app.tick();
    }
    app.send(Cmd::Release);
    // A phone left turned stays turned: stand it back up.
    let back = match app.turn {
        1 => vec![RotationDirection::Left],
        2 => vec![RotationDirection::Right, RotationDirection::Right],
        3 => vec![RotationDirection::Right],
        _ => vec![],
    };
    let wait = Duration::from_millis(1000 + 500 * back.len() as u64);
    for d in back {
        app.send(Cmd::Rotate(d));
    }
    // The window goes at once; the session finishes behind it.
    let session = {
        let app = app;
        let App { session, .. } = app;
        session
    };
    let _ = conn.flush();
    session.close(wait);
    Ok(())
}

/// The turn's spring: Apple's default, half a second, barely
/// underdamped. Its share of the way, at `t` seconds.
fn spring(t: f64) -> f64 {
    let (omega, zeta): (f64, f64) = (2.0 * std::f64::consts::PI / 0.5, 0.9);
    let wd = omega * (1.0 - zeta * zeta).sqrt();
    1.0 - (-zeta * omega * t).exp() * ((wd * t).cos() + zeta * omega / wd * (wd * t).sin())
}

/// How small a `w` by `h` rectangle turned `angle` must be to stay inside
/// `room`.
fn fit_turned(w: f64, h: f64, room: (f64, f64), angle: f64) -> f64 {
    let (c, s) = (angle.cos().abs(), angle.sin().abs());
    (room.0 / (w * c + h * s)).min(room.1 / (w * s + h * c)).min(1.0)
}

struct Wheel {
    at: Point,
    moved: Point,
    last: Instant,
}

struct App {
    registry: RegistryState,
    seats: SeatState,
    outputs: OutputState,
    presentation: PresentationTimeState,
    data: Option<DataDeviceManagerState>,
    data_device: Option<DataDevice>,
    compositor: CompositorState,
    qh: QueueHandle<App>,
    handle: LoopHandle<'static, App>,
    window: Window,
    gpu: Option<gpu::Gpu>,
    overlay: Overlay,
    scene: Option<Scene>,
    /// The window, logical.
    size: (u32, u32),
    scale: i32,
    /// The phone's screen in the window, logical, as it is now oriented.
    phone: (f64, f64),
    /// Quarter turns clockwise the picture is shown at.
    turn: u8,
    /// The turn under way: where the picture started, in radians from
    /// where it is going, and since when.
    spin: Option<(f64, Instant)>,
    /// The window less its margins, logical, which the phone fits inside
    /// either way round: a square of its long side unless the compositor
    /// set a size (a tile, a resize by hand).
    room: (f64, f64),
    model: Option<Model>,
    /// The largest window the compositor suggests, when it says.
    bounds: Option<(u32, u32)>,
    /// The compositor has sized the window; it keeps that size.
    sized: bool,
    /// Not tiled, maximized or fullscreen: the window is the phone, and it
    /// sizes itself when the phone turns.
    floating: bool,
    /// The size the window last left of its own accord; a configure still
    /// asking for it predates the change.
    left: Option<(u32, u32)>,
    /// The spin has settled and a float shrinks back to the phone.
    snap: bool,
    status: Status,
    /// The phone's screen is dark: it sleeps, locked.
    asleep: bool,
    frame_at: Option<Instant>,
    configured: bool,
    frame_pending: bool,
    dirty: bool,
    latest: Arc<Latest>,
    session: session::Session,
    audio: Arc<Audio>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    seat: Option<wl_seat::WlSeat>,
    cursor: Point,
    inside: bool,
    touching: bool,
    hover: Option<Control>,
    pressed: Option<Control>,
    wheel: Option<Wheel>,
    held: BTreeSet<u8>,
    mods: Modifiers,
    focused: bool,
    /// Frames out with the compositor, by their feedback, with when they decoded.
    in_flight: std::collections::VecDeque<(smithay_client_toolkit::reexports::client::backend::ObjectId, Instant)>,
    /// Decode to on-screen, for the latency log.
    presented: Vec<Duration>,
    exit: bool,
}

/// A point of the picture as shown, turned `turn` quarters clockwise, back
/// on the portrait panel. Must match the shader's `unturn`.
fn unturn(turn: u8, u: f64, v: f64) -> Point {
    match turn % 4 {
        1 => Point::new(v, 1.0 - u),
        2 => Point::new(1.0 - u, 1.0 - v),
        3 => Point::new(1.0 - v, u),
        _ => Point::new(u, v),
    }
}

impl App {
    fn send(&self, cmd: Cmd) {
        self.session.send(cmd);
    }

    fn landscape(&self) -> bool {
        self.turn % 2 == 1
    }

    /// The phone's screen in the picture, portrait as the stream always is:
    /// its native size, less the encoder's padding.
    fn content(&self) -> (u32, u32) {
        let frame = self.gpu.as_ref().and_then(|g| g.frame_size());
        let m = self.model.unwrap_or_else(|| {
            let (w, h) = frame.unwrap_or((1206, 2622));
            model::fallback(w, h)
        });
        match frame {
            Some((fw, fh)) => (m.width.min(fw), m.height.min(fh)),
            None => (m.width, m.height),
        }
    }

    /// Width over height as shown, turned with the interface.
    fn aspect(&self) -> f64 {
        let (w, h) = self.content();
        if self.landscape() { h as f64 / w as f64 } else { w as f64 / h as f64 }
    }

    /// The phone, centred in the room.
    fn phone_rect(&self) -> Rect {
        let x = MARGIN + (self.room.0 - self.phone.0) / 2.0;
        let y = TOP_MARGIN + (self.room.1 - self.phone.1) / 2.0;
        Rect::new(x, y, x + self.phone.0, y + self.phone.1)
    }

    fn radius(&self) -> f64 {
        let m = self.model.unwrap_or_else(|| model::fallback(1206, 2622));
        m.corner_fraction() * self.phone.0.min(self.phone.1)
    }

    /// The phone at its true size on the output the window is on, or the
    /// first one (millimetres over the mode's pixels per millimetre), turned
    /// as it is shown, as far as the compositor's room allows.
    fn natural_room(&self) -> (f64, f64) {
        let m = self.model.unwrap_or_else(|| model::fallback(1206, 2622));
        let info = self.output();
        let current = info.as_ref().and_then(|i| i.modes.iter().find(|m| m.current).map(|m| m.dimensions));
        // The panel's pixels per millimetre, mode and physical size both
        // in the output's own (unrotated) orientation.
        let per_mm = info.as_ref().zip(current).and_then(|(i, (mw, _))| (i.physical_size.0 > 0).then(|| mw as f64 / i.physical_size.0 as f64));
        // Device pixels per logical pixel, whichever way the output is turned.
        let scale = info
            .as_ref()
            .zip(current)
            .and_then(|(i, (mw, mh))| i.logical_size.map(|(lw, lh)| mw.max(mh) as f64 / lw.max(lh).max(1) as f64))
            .unwrap_or(1.0);
        let (w, h) = match per_mm {
            // Pixels per mm of the panel, over the compositor's scale.
            Some(per_mm) => (m.width_mm() * per_mm / scale, m.height_mm() * per_mm / scale),
            // No physical size (a headless or virtual output): the phone's points.
            None => (m.width as f64 / m.scale, m.height as f64 / m.scale),
        };
        tracing::info!("natural size {w:.0}x{h:.0} logical ({} on {:?})", m.name, info.map(|i| i.name));
        self.within_bounds(if self.landscape() { (h, w) } else { (w, h) })
    }

    /// The output the window is on, or the first one.
    fn output(&self) -> Option<smithay_client_toolkit::output::OutputInfo> {
        let on = self.window.wl_surface().data::<smithay_client_toolkit::compositor::SurfaceData<()>>().and_then(|d| d.outputs().next());
        on.or_else(|| self.outputs.outputs().next()).and_then(|o| self.outputs.info(&o))
    }

    /// A phone bigger than the space the compositor offers (else the
    /// output's) shrinks to fit it.
    fn within_bounds(&self, (w, h): (f64, f64)) -> (f64, f64) {
        let room = self.bounds.filter(|(bw, bh)| *bw > 0 && *bh > 0).or_else(|| self.output()?.logical_size.map(|(w, h)| (w.max(0) as u32, h.max(0) as u32)));
        match room {
            Some((bw, bh)) if bw > 0 && bh > 0 => {
                let room = (bw as f64 - 2.0 * MARGIN, bh as f64 - TOP_MARGIN - MARGIN);
                let k = (room.0 / w).min(room.1 / h).min(1.0);
                (w * k, h * k)
            }
            _ => (w, h),
        }
    }

    /// The phone as big as `room` holds it at the picture's aspect. A turn
    /// refits it in the same room, so it spins in place and the compositor
    /// is never asked for a new size.
    fn fit(&mut self, room: (f64, f64)) {
        let room = (room.0.max(120.0).round(), room.1.max(120.0).round());
        let a = self.aspect();
        let (w, h) = if room.0 / room.1 > a { (room.1 * a, room.1) } else { (room.0, room.0 / a) };
        self.room = room;
        self.phone = (w.round(), h.round());
        self.size = ((room.0 + 2.0 * MARGIN) as u32, (room.1 + TOP_MARGIN + MARGIN) as u32);
    }

    /// The window as the compositor sizes, frames and places it. A float's
    /// is the room, kept to the phone, with the shadow and the strip hanging
    /// outside it; a tile's is all of it, so nothing spills on its neighbours.
    fn geometry(&self) -> (u32, u32) {
        if self.floating { (self.room.0 as u32, self.room.1 as u32) } else { self.size }
    }

    fn set_geometry(&self) {
        let (w, h) = self.geometry();
        let (x, y) = if self.floating { (MARGIN as i32, TOP_MARGIN as i32) } else { (0, 0) };
        self.window.xdg_surface().set_window_geometry(x, y, w as i32, h as i32);
    }

    /// A float takes the size its phone needs: the compositor follows a
    /// float's own size.
    fn resize_self(&mut self, room: (f64, f64)) {
        let was = self.geometry();
        self.fit(room);
        if self.geometry() != was {
            self.left = Some(was);
            self.set_geometry();
        }
        self.set_input_region();
        self.dirty = true;
    }

    fn set_input_region(&self) {
        let Ok(region) = smithay_client_toolkit::compositor::Region::new(&self.compositor) else { return };
        let p = self.phone_rect();
        region.add(p.x0 as i32, p.y0 as i32, p.width() as i32, p.height() as i32);
        let s = overlay::strip_rect(p);
        region.add(s.x0 as i32, s.y0 as i32, s.width() as i32, s.height() as i32 + overlay::STRIP_GAP as i32);
        self.window.wl_surface().set_input_region(Some(region.wl_region()));
    }

    fn device_size(&self) -> (u32, u32) {
        (self.size.0 * self.scale as u32, self.size.1 * self.scale as u32)
    }

    fn screen(&self) -> Screen {
        let has_video = self.gpu.as_ref().is_some_and(|g| g.has_video);
        match &self.status {
            _ if self.asleep => Screen::Locked,
            Status::Streaming if has_video && self.frame_at.is_some_and(|t| t.elapsed() < STALL) => Screen::Live,
            Status::Streaming if has_video => Screen::Live,
            _ if has_video => Screen::Stalled,
            Status::Unpaired => Screen::Waiting { title: "No iPhone paired".into(), detail: "Pair your iPhone with this computer first".into() },
            Status::Searching => Screen::Waiting { title: "Connecting to iPhone".into(), detail: "Looking for it on Wi-Fi".into() },
            Status::Connecting => Screen::Waiting { title: "Connecting to iPhone".into(), detail: "Opening a secure connection".into() },
            Status::Preparing => Screen::Waiting { title: "Connecting to iPhone".into(), detail: "Preparing iPhone".into() },
            Status::Streaming => Screen::Waiting { title: "Connecting to iPhone".into(), detail: "Waiting for the picture".into() },
            Status::Reconnecting(_) => Screen::Waiting { title: "Connecting to iPhone".into(), detail: "Trying again".into() },
        }
    }

    fn draw(&mut self) {
        if !self.configured || self.frame_pending || self.gpu.is_none() {
            self.dirty = true;
            return;
        }
        let mut decoded = None;
        if let Some(frame) = self.latest.take() {
            self.gpu.as_mut().unwrap().set_frame(&frame);
            decoded = Some(frame.decoded);
            self.frame_at = Some(Instant::now());
        }
        let device = self.device_size();
        let scene = Scene {
            size: self.size,
            scale: self.scale as f64,
            phone: self.phone_rect(),
            radius: self.radius(),
            screen: self.screen(),
            strip: self.inside,
            hover: self.hover,
            muted: self.audio.muted.load(std::sync::atomic::Ordering::Relaxed),
        };
        let p = self.phone_rect();
        let s = self.scale as f32;
        let (angle, zoom) = self.spin_now();
        let at = gpu::Placement {
            origin: (p.x0 as f32 * s, p.y0 as f32 * s),
            size: (p.width() as f32 * s, p.height() as f32 * s),
            radius: self.radius() as f32 * s,
            scale: s,
            content: self.content(),
            turn: self.turn,
            angle: angle as f32,
            zoom: zoom as f32,
        };
        let gpu = self.gpu.as_mut().unwrap();
        gpu.resize(device);
        if self.scene.as_ref() != Some(&scene) {
            let pixmap = self.overlay.draw(&scene);
            gpu.set_overlay(pixmap.data_as_u8_slice(), (pixmap.width() as u32, pixmap.height() as u32));
            self.scene = Some(scene);
        }
        let surface = self.window.wl_surface().clone();
        surface.frame(&self.qh, smithay_client_toolkit::compositor::FrameCallbackData(surface.clone()));
        if let Some(t) = decoded
            && let Ok(feedback) = self.presentation.feedback(&surface, &self.qh)
        {
            self.in_flight.push_back((feedback.id(), t));
        }
        surface.set_buffer_scale(self.scale);
        match gpu.render(&at) {
            Ok(()) => {
                self.frame_pending = true;
                // A spin draws every frame until it settles.
                self.dirty = self.spin.is_some();
            }
            Err(e) => tracing::warn!("render: {e}"),
        }
        if std::mem::take(&mut self.snap) {
            self.resize_self(self.phone);
        }
    }

    /// The smallest window.
    fn limits(&self) {
        self.window.set_min_size(Some((120, 120)));
    }

    fn new_frame(&mut self) {
        self.dirty = true;
        self.draw();
    }

    fn session_event(&mut self, e: Event) {
        match e {
            Event::Status(s) => {
                tracing::info!("status: {s:?}");
                if s != Status::Streaming {
                    self.asleep = false;
                }
                self.status = s;
            }
            Event::Turn(turn) => self.set_turn(turn),
            Event::Asleep(asleep) => self.asleep = asleep,
            Event::Phone { product_type } => {
                let m = model::lookup(&product_type);
                tracing::info!("phone: {product_type} ({})", m.map(|m| m.name).unwrap_or("unknown"));
                if m != self.model {
                    self.model = m;
                    if self.configured {
                        // Within the size the compositor gave, once it gave one.
                        if self.sized {
                            self.fit(self.room);
                            self.set_input_region();
                        } else {
                            let room = self.natural_room();
                            self.resize_self(room);
                        }
                    }
                }
            }
        }
        self.dirty = true;
        self.draw();
    }

    /// Runs after every dispatch: lifts a wheel drag gone still, and shows
    /// a stall once the picture has been quiet for a while.
    fn tick(&mut self) {
        if let Some(w) = &self.wheel
            && w.last.elapsed() >= WHEEL_IDLE
        {
            let at = self.to_phone(Point::new(w.at.x + w.moved.x, w.at.y + w.moved.y));
            self.send(Cmd::Touch(Phase::Up, at.x, at.y));
            self.wheel = None;
        }
        let stalled = self.frame_at.is_some_and(|t| t.elapsed() > STALL);
        if self.scene.as_ref().is_some_and(|s| s.screen != self.screen()) || (stalled && self.dirty) {
            self.draw();
        }
    }

    /// A window point as a share of the phone's portrait panel, the way
    /// its touchscreen reads whichever way the interface faces.
    fn to_phone(&self, p: Point) -> Point {
        let r = self.phone_rect();
        let u = ((p.x - r.x0) / r.width()).clamp(0.0, 1.0);
        let v = ((p.y - r.y0) / r.height()).clamp(0.0, 1.0);
        unturn(self.turn, u, v)
    }

    fn control_at(&self, p: Point) -> Option<Control> {
        if !self.inside {
            return None;
        }
        overlay::strip_buttons(self.phone_rect()).into_iter().find(|(_, r)| r.contains(p)).map(|(c, _)| c)
    }

    fn on_phone(&self, p: Point) -> bool {
        self.phone_rect().contains(p)
    }

    fn activate(&mut self, control: Control) {
        match control {
            Control::Home => self.send(Cmd::Button(Button::Home)),
            Control::AppSwitcher => self.send(Cmd::Button(Button::AppSwitcher)),
            Control::Mute => {
                let m = &self.audio.muted;
                m.store(!m.load(std::sync::atomic::Ordering::Relaxed), std::sync::atomic::Ordering::Relaxed);
            }
            Control::Rotate => {
                // Portrait and landscape, the phone's top to the right.
                let (direction, turn) = match self.turn {
                    1 => (RotationDirection::Left, 0),
                    3 => (RotationDirection::Right, 0),
                    t => (RotationDirection::Right, (t + 1) % 4),
                };
                self.send(Cmd::Rotate(direction));
                // The window turns now; the phone's answer corrects it.
                self.set_turn(turn);
            }
            Control::Close => self.exit = true,
        }
        self.dirty = true;
        self.draw();
    }

    /// Shows the picture `turn` quarters clockwise: the phone takes the new
    /// aspect at once and spins into it.
    fn set_turn(&mut self, turn: u8) {
        if turn == self.turn {
            return;
        }
        let quarters = match (turn + 4 - self.turn) % 4 {
            3 => -1.0,
            q => q as f64,
        };
        let now = self.spin.map(|(from, at)| from * (1.0 - spring(at.elapsed().as_secs_f64()))).unwrap_or(0.0);
        self.spin = Some((now - quarters * std::f64::consts::FRAC_PI_2, Instant::now()));
        let flips = self.landscape() != (turn % 2 == 1);
        self.turn = turn;
        if self.configured && flips {
            if self.floating {
                // A square of the phone's long side to spin in, centred where
                // the phone was; the spin's end shrinks it to the phone.
                let long = self.phone.0.max(self.phone.1);
                self.snap = false;
                self.resize_self((long, long));
            } else {
                self.fit(self.room);
                self.set_input_region();
            }
        }
    }

    /// The picture's extra turn and shrink this frame, while it spins.
    fn spin_now(&mut self) -> (f64, f64) {
        let Some((from, at)) = self.spin else { return (0.0, 1.0) };
        let t = at.elapsed().as_secs_f64();
        let angle = from * (1.0 - spring(t));
        if t > 1.0 || angle.abs() < 1e-3 {
            self.spin = None;
            self.snap = self.floating;
            return (0.0, 1.0);
        }
        (angle, fit_turned(self.phone.0, self.phone.1, self.room, angle))
    }

    fn release_keys(&mut self) {
        if !self.held.is_empty() {
            self.held.clear();
            self.send(Cmd::Keys(BTreeSet::new()));
        }
    }

    fn paste(&mut self) {
        let Some(offer) = self.data_device.as_ref().and_then(|d| d.data().selection_offer()) else { return };
        let mime = offer.with_mime_types(|m| {
            ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"].iter().find(|t| m.iter().any(|x| x == *t)).map(|t| t.to_string())
        });
        let Some(mime) = mime else { return };
        let Ok(pipe) = offer.receive(mime) else { return };
        let mut text = Vec::new();
        let _ = self.handle.insert_source(pipe, move |_, file, app: &mut App| {
            let mut buf = [0u8; 4096];
            // SAFETY: the pipe is the read end calloop handed us, open until removed.
            match unsafe { file.get_mut() }.read(&mut buf) {
                Ok(0) => {
                    let s = String::from_utf8_lossy(&text).into_owned();
                    if !s.is_empty() {
                        app.send(Cmd::Paste(s));
                    }
                    PostAction::Remove
                }
                Ok(n) => {
                    text.extend_from_slice(&buf[..n]);
                    PostAction::Continue
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => PostAction::Continue,
                Err(_) => PostAction::Remove,
            }
        });
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, factor: i32) {
        if factor != self.scale {
            self.scale = factor.max(1);
            self.scene = None;
            self.dirty = true;
            self.draw();
        }
    }

    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw();
        }
    }

    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}

    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl PresentationTimeHandler for App {
    fn presentation_time_state(&mut self) -> &mut PresentationTimeState {
        &mut self.presentation
    }

    fn presented(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        feedback: &wp_presentation_feedback::WpPresentationFeedback,
        _: &wl_surface::WlSurface,
        _: Vec<wl_output::WlOutput>,
        time: PresentTime,
        _: u32,
        _: u64,
        _: WEnum<wp_presentation_feedback::Kind>,
    ) {
        let Some(at) = self.in_flight.iter().position(|(id, _)| *id == feedback.id()) else { return };
        let (_, decoded) = self.in_flight.remove(at).unwrap();
        // The feedback is on the compositor's clock (CLOCK_MONOTONIC on
        // every compositor we run on), the decode time on ours.
        let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: a plain clock read into a local.
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
        let now = Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32);
        let shown = Duration::new(time.tv_sec, time.tv_nsec);
        let since_shown = now.saturating_sub(shown);
        let on_screen = decoded.elapsed().saturating_sub(since_shown);
        self.presented.push(on_screen);
        if self.presented.len() == 240 {
            self.presented.sort();
            tracing::debug!("decoded to on screen: p50 {:?} p95 {:?}", self.presented[120], self.presented[228]);
            self.presented.clear();
        }
    }

    fn discarded(&mut self, _: &Connection, _: &QueueHandle<Self>, feedback: &wp_presentation_feedback::WpPresentationFeedback, _: &wl_surface::WlSurface) {
        self.in_flight.retain(|(id, _)| *id != feedback.id());
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(&mut self, conn: &Connection, _: &QueueHandle<Self>, window: &Window, configure: WindowConfigure, _: u32) {
        let first = !self.configured;
        if configure.suggested_bounds.is_some() {
            self.bounds = configure.suggested_bounds;
        }
        self.configured = true;
        self.floating = !(configure.is_tiled() || configure.is_maximized() || configure.is_fullscreen());
        let asked = configure.new_size.0.zip(configure.new_size.1).map(|(w, h)| (w.get(), h.get()));
        let stale = self.floating && asked.is_some() && (asked == self.left || asked == Some(self.geometry()));
        if !stale {
            self.left = None;
        }
        match asked {
            Some((w, h)) if !first && !stale => {
                self.sized = true;
                let (w, h) = (w as f64, h as f64);
                self.fit(if self.floating { (w, h) } else { (w - 2.0 * MARGIN, h - TOP_MARGIN - MARGIN) });
            }
            _ if first || self.phone.0 == 0.0 => {
                let room = self.natural_room();
                self.fit(room);
            }
            _ => {}
        }
        self.limits();
        self.set_geometry();
        self.set_input_region();
        if self.gpu.is_none() {
            let display = conn.backend().display_ptr() as *mut std::ffi::c_void;
            let surface = window.wl_surface().id().as_ptr() as *mut std::ffi::c_void;
            // SAFETY: the connection and the window live as long as the App
            // that owns the Gpu, which drops first (field order).
            match unsafe { gpu::Gpu::new(display, surface, self.device_size()) } {
                Ok(g) => self.gpu = Some(g),
                Err(e) => {
                    tracing::error!("gpu: {e}");
                    self.exit = true;
                    return;
                }
            }
        }
        self.scene = None;
        self.frame_pending = false;
        self.dirty = true;
        self.draw();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seats
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}

    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        if self.data_device.is_none()
            && let Some(data) = &self.data
        {
            self.data_device = Some(data.get_data_device(qh, &seat));
        }
        self.seat = Some(seat.clone());
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seats.get_keyboard(qh, &seat, None).ok();
        }
        if capability == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seats.get_pointer(qh, &seat).ok();
        }
    }

    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, capability: Capability) {
        if capability == Capability::Keyboard
            && let Some(k) = self.keyboard.take()
        {
            k.release();
        }
        if capability == Capability::Pointer
            && let Some(p) = self.pointer.take()
        {
            p.release();
        }
    }

    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        for event in events {
            if &event.surface != self.window.wl_surface() {
                continue;
            }
            let p = Point::new(event.position.0, event.position.1);
            self.cursor = p;
            match event.kind {
                PointerEventKind::Enter { .. } => {
                    self.inside = true;
                    self.hover = self.control_at(p);
                    self.dirty = true;
                }
                PointerEventKind::Leave { .. } => {
                    if self.touching {
                        let at = self.to_phone(p);
                        self.send(Cmd::Touch(Phase::Up, at.x, at.y));
                        self.touching = false;
                    }
                    self.inside = false;
                    self.hover = None;
                    self.pressed = None;
                    self.dirty = true;
                }
                PointerEventKind::Motion { .. } => {
                    if self.touching {
                        let at = self.to_phone(p);
                        self.send(Cmd::Touch(Phase::Move, at.x, at.y));
                    }
                    let hover = self.control_at(p);
                    if hover != self.hover {
                        self.hover = hover;
                        self.dirty = true;
                    }
                }
                PointerEventKind::Press { button, serial, .. } if button == BTN_LEFT => {
                    if let Some(c) = self.control_at(p) {
                        self.pressed = Some(c);
                    } else if self.on_phone(p) && self.asleep {
                        self.send(Cmd::Button(Button::Side));
                    } else if self.on_phone(p) {
                        let at = self.to_phone(p);
                        self.send(Cmd::Touch(Phase::Down, at.x, at.y));
                        self.touching = true;
                    } else if let Some(seat) = &self.seat {
                        // The strip's own background moves the window.
                        self.window.move_(seat, serial);
                    }
                }
                PointerEventKind::Release { button, .. } if button == BTN_LEFT => {
                    if self.touching {
                        let at = self.to_phone(p);
                        self.send(Cmd::Touch(Phase::Up, at.x, at.y));
                        self.touching = false;
                    }
                    if let Some(c) = self.pressed.take()
                        && self.control_at(p) == Some(c)
                    {
                        self.activate(c);
                    }
                }
                PointerEventKind::Axis { horizontal, vertical, .. } => {
                    if self.touching || !self.on_phone(p) {
                        continue;
                    }
                    // Discrete ticks scroll a fixed step; a touchpad's
                    // continuous axis moves by its own distance.
                    let dy = if vertical.discrete != 0 || vertical.value120 != 0 { vertical.value120 as f64 / 120.0 * WHEEL_STEP } else { vertical.absolute };
                    let dx = if horizontal.discrete != 0 || horizontal.value120 != 0 { horizontal.value120 as f64 / 120.0 * WHEEL_STEP } else { horizontal.absolute };
                    if dx == 0.0 && dy == 0.0 {
                        continue;
                    }
                    if self.wheel.is_none() {
                        let at = self.to_phone(p);
                        self.send(Cmd::Touch(Phase::Down, at.x, at.y));
                        self.wheel = Some(Wheel { at: p, moved: Point::ZERO, last: Instant::now() });
                    }
                    let wheel = self.wheel.as_mut().unwrap();
                    // Content follows the finger: scrolling down drags up.
                    wheel.moved = Point::new(wheel.moved.x - dx, wheel.moved.y - dy);
                    wheel.last = Instant::now();
                    let to = Point::new(wheel.at.x + wheel.moved.x, wheel.at.y + wheel.moved.y);
                    let at = self.to_phone(to);
                    self.send(Cmd::Touch(Phase::Move, at.x, at.y));
                }
                _ => {}
            }
        }
        if self.dirty {
            self.draw();
        }
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {
        if surface == self.window.wl_surface() {
            self.focused = true;
        }
    }

    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, surface: &wl_surface::WlSurface, _: u32) {
        if surface == self.window.wl_surface() {
            self.focused = false;
            self.release_keys();
            self.send(Cmd::Release);
        }
    }

    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if !self.focused {
            return;
        }
        let ctrl_or_super = self.mods.ctrl || self.mods.logo;
        if ctrl_or_super && matches!(event.keysym, Keysym::v | Keysym::V) {
            // The phone pastes its own pasteboard: fill it with ours first.
            self.paste();
            return;
        }
        if let Some(usage) = keys::usage(event.raw_code) {
            self.held.insert(usage);
            let sent = keys::for_phone(&self.held);
            self.send(Cmd::Keys(sent));
        }
    }

    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}

    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if let Some(usage) = keys::usage(event.raw_code)
            && self.held.remove(&usage)
        {
            let sent = keys::for_phone(&self.held);
            self.send(Cmd::Keys(sent));
        }
    }

    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, modifiers: Modifiers, _: RawModifiers, _: u32) {
        self.mods = modifiers;
    }
}

impl DataDeviceHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_device::WlDataDevice, _: f64, _: f64, _: &wl_surface::WlSurface) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_device::WlDataDevice) {}
    fn motion(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_device::WlDataDevice, _: f64, _: f64) {}
    fn selection(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_device::WlDataDevice) {}
    fn drop_performed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_device::WlDataDevice) {}
}

impl DataOfferHandler for App {
    fn source_actions(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: wl_data_device_manager::DndAction) {}
    fn selected_action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &mut DragOffer, _: wl_data_device_manager::DndAction) {}
}

impl DataSourceHandler for App {
    fn accept_mime(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource, _: Option<String>) {}
    fn send_request(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource, _: String, _: WritePipe) {}
    fn cancelled(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource) {}
    fn dnd_dropped(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource) {}
    fn dnd_finished(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource) {}
    fn action(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &smithay_client_toolkit::reexports::client::protocol::wl_data_source::WlDataSource, _: wl_data_device_manager::DndAction) {}
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
