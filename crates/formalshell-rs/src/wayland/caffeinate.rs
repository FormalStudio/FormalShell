//! Caffeinate:
//! while the toggle is on or a ScreenSaver cookie is live, a 1px transparent
//! layer surface of its own holds a Wayland idle inhibitor, and an
//! ext-idle-notify listener that respects inhibitors reports whether the
//! session went idle. The surface is its own
//! rather than the bar's because the bar unmaps under a fullscreen window,
//! and it keeps its default input region because Hyprland only counts a
//! mapped layer surface that accepts input.
//!
//! Hyprland judges an inhibitor once, when it is created, and does not
//! look again when a layer surface maps later, so the inhibitor is created a
//! second after the surface's first commit instead of with it.

use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::globals::GlobalList;
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notification_v1::{
    self, ExtIdleNotificationV1,
};
use smithay_client_toolkit::reexports::protocols::ext::idle_notify::v1::client::ext_idle_notifier_v1::ExtIdleNotifierV1;
use smithay_client_toolkit::reexports::protocols::wp::idle_inhibit::zv1::client::zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1;
use smithay_client_toolkit::reexports::protocols::wp::idle_inhibit::zv1::client::zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};

use super::App;
use crate::runtime::Msg;
use crate::services::caffeinate::Diff;
use crate::store;
use crate::surface::Ignore;

const NAMESPACE: &str = "formalshell:caffeinate";
const INHIBIT_AFTER: Duration = Duration::from_secs(1);
/// `screensaver.timeoutSeconds`' default.
const IDLE_SECONDS: f64 = 300.0;

macro_rules! ignore {
    ($($iface:ty),*) => {$(
        impl Dispatch2<$iface, App> for Ignore {
            fn event(&self, _: &mut App, _: &$iface, _: <$iface as smithay_client_toolkit::reexports::client::Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
        }
    )*};
}

ignore!(ZwpIdleInhibitManagerV1, ZwpIdleInhibitorV1, ExtIdleNotifierV1);

/// The idle notification's user data: the two events flip the slice.
pub struct IdleEvents;

impl Dispatch2<ExtIdleNotificationV1, App> for IdleEvents {
    fn event(&self, app: &mut App, _: &ExtIdleNotificationV1, event: ext_idle_notification_v1::Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            ext_idle_notification_v1::Event::Idled => app.set_idle(true),
            ext_idle_notification_v1::Event::Resumed => app.set_idle(false),
            _ => {}
        }
    }
}

struct Held {
    layer: LayerSurface,
    buffer: Option<WlBuffer>,
    inhibitor: Option<ZwpIdleInhibitorV1>,
    /// Tells a late timer its surface is gone.
    generation: u64,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(i) = self.inhibitor.take() {
            i.destroy();
        }
        if let Some(b) = self.buffer.take() {
            b.destroy();
        }
    }
}

pub struct Caffeinate {
    inhibit: Option<ZwpIdleInhibitManagerV1>,
    notifier: Option<ExtIdleNotifierV1>,
    held: Option<Held>,
    idle: Option<ExtIdleNotificationV1>,
    armed: bool,
    generation: u64,
}

impl Caffeinate {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Self {
        let inhibit = globals.bind(qh, 1..=1, Ignore).ok();
        if inhibit.is_none() {
            eprintln!("caffeinate: the compositor has no zwp_idle_inhibit_manager_v1");
        }
        Self {
            inhibit,
            notifier: globals.bind(qh, 1..=2, Ignore).ok(),
            held: None,
            idle: None,
            armed: false,
            generation: 0,
        }
    }
}

impl App {
    fn caffeinate_diff(&mut self, diff: Diff) {
        self.receive(Msg::Diff(store::Diff::Caffeinate(diff)));
    }

    /// Once settings.json has loaded: the idle listener with the screensaver
    /// timeout it carries, and `caffeinate.onStartup`. Created once, already
    /// carrying the right timeout: reconfiguring a live notification
    /// recreates it, which loses track of the monitor.
    pub fn arm_caffeinate(&mut self) {
        if self.caffeinate.armed || !self.store.config.loaded {
            return;
        }
        self.caffeinate.armed = true;
        self.arm_idle();
        if self.store.config.bool("caffeinate.onStartup") == Some(true) {
            self.set_caffeinated(true);
        }
    }

    /// Needs a seat, so it is tried again whenever one appears.
    pub(super) fn arm_idle(&mut self) {
        let c = &self.caffeinate;
        if !c.armed || c.idle.is_some() {
            return;
        }
        let (Some(notifier), Some(seat)) = (c.notifier.clone(), self.seats.seats().next()) else { return };
        let seconds = self.store.config.f64("screensaver.timeoutSeconds").filter(|s| *s > 0.0).unwrap_or(IDLE_SECONDS);
        let notification = notifier.get_idle_notification((seconds * 1000.0) as u32, &seat, &self.qh, IdleEvents);
        self.caffeinate.idle = Some(notification);
    }

    pub fn set_caffeinated(&mut self, on: bool) {
        self.caffeinate_diff(Diff::Active(on));
    }

    /// Run on every caffeinate change: the toggle and the cookies share one
    /// inhibitor.
    pub fn hold_inhibitor(&mut self) {
        let want = self.store.caffeinate.held();
        let held = self.caffeinate.held.is_some();
        if want && !held {
            self.hold_surface();
        } else if !want && held {
            self.caffeinate.held = None;
            self.caffeinate_diff(Diff::Inhibiting(false));
        }
    }

    pub fn set_idle(&mut self, idle: bool) {
        self.caffeinate_diff(Diff::Idle(idle));
    }

    fn hold_surface(&mut self) {
        if self.caffeinate.inhibit.is_none() {
            return;
        }
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Background, Some(NAMESPACE), None);
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_size(1, 1);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        self.caffeinate.generation += 1;
        self.caffeinate.held = Some(Held { layer, buffer: None, inhibitor: None, generation: self.caffeinate.generation });
    }

    pub(super) fn caffeinate_owns(&self, layer: &LayerSurface) -> bool {
        self.caffeinate.held.as_ref().is_some_and(|h| h.layer.wl_surface() == layer.wl_surface())
    }

    /// The first configure maps the surface with one transparent pixel; the
    /// inhibitor follows a second later.
    pub(super) fn caffeinate_configure(&mut self) {
        let Some(held) = self.caffeinate.held.as_mut() else { return };
        if held.buffer.is_some() {
            return;
        }
        let buffer = self.pixels.single_pixel.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, Ignore);
        let surface = held.layer.wl_surface();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        held.layer.commit();
        held.buffer = Some(buffer);
        let generation = held.generation;
        let Some(handle) = &self.handle else { return };
        let started = Instant::now() + INHIBIT_AFTER;
        let _ = handle.insert_source(Timer::from_deadline(started), move |_, _, app: &mut App| {
            app.enable_inhibitor(generation);
            TimeoutAction::Drop
        });
    }

    fn enable_inhibitor(&mut self, generation: u64) {
        let Some(manager) = self.caffeinate.inhibit.clone() else { return };
        let Some(held) = self.caffeinate.held.as_mut().filter(|h| h.generation == generation && h.inhibitor.is_none()) else { return };
        held.inhibitor = Some(manager.create_inhibitor(held.layer.wl_surface(), &self.qh, Ignore));
        self.caffeinate_diff(Diff::Inhibiting(true));
    }
}
