//! The bar over a fullscreen Mac window, as the menu bar slides down over a
//! fullscreen app on the Mac. While `fullscreen.hideChrome` has the bar off
//! the layers and the focused window on its output is a Mac window, a clear
//! line one logical pixel deep lies along the bar's edge on the overlay
//! layer. The pointer entering it maps the bar on the overlay layer,
//! reserving nothing, so the window keeps its size. The bar leaves again
//! once the pointer has been off it, its line and its cards for
//! [`HOLD`] with no card or menu open, or at once when the window leaves
//! fullscreen or focus moves to a window that is not a Mac app's.
//! `org.atoll.MenuBar`'s `Revealed` follows it: the window never sees the
//! pointer that reached its top edge, so the app is told.

use std::time::Duration;

use calloop::RegistrationToken;
use calloop::timer::{TimeoutAction, Timer};
use fs_types::Edge;
use serde_json::{Value, json};
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{KeyboardInteractivity, Layer, LayerSurface};

use super::{App, Target, edge_anchor};
use crate::surface::Ignore;

const NAMESPACE: &str = "formalshell:reveal";

/// How long the bar stays once the pointer is off it.
const HOLD: Duration = Duration::from_millis(500);

struct Sensor {
    output: WlOutput,
    edge: Edge,
    layer: LayerSurface,
    viewport: WpViewport,
    buffer: Option<WlBuffer>,
}

impl Drop for Sensor {
    fn drop(&mut self) {
        self.viewport.destroy();
        if let Some(b) = self.buffer.take() {
            b.destroy();
        }
    }
}

#[derive(Default)]
pub struct Reveal {
    sensor: Option<Sensor>,
    /// The bar is up over the fullscreen window.
    pub shown: bool,
    /// The pointer is on the bar, its line or a card hanging off it.
    over: bool,
    hide: Option<RegistrationToken>,
}

impl App {
    /// The focused window is a Mac app's, fullscreen on the live bar's
    /// output with the bar taken off it.
    fn mac_fullscreen(&self) -> bool {
        if !self.chrome().bar_covered() {
            return false;
        }
        let h = &self.store.compositor;
        let Some(w) = h.window(&h.held_focused_window_id()) else { return false };
        let output = &live!(self).bar.output;
        self.store.appmenu.is_mac(w) && h.snapshot.workspaces.iter().any(|ws| ws.id == w.workspace_id && ws.output == *output)
    }

    /// The bar's window is down: a fullscreen window covers its output and
    /// the bar is not revealed over it.
    pub(super) fn bar_hides(&self) -> bool {
        self.chrome().bar_covered() && !self.reveal.shown
    }

    /// The line and the reveal as the compositor's state wants them.
    pub(super) fn sync_reveal(&mut self) {
        let wanted = self.mac_fullscreen();
        if !wanted && self.reveal.shown {
            self.end_reveal();
        }
        let output = self.bar_wl().cloned().filter(|_| wanted);
        let edge = live!(self).bar.edge();
        let current = self.reveal.sensor.as_ref().map(|s| (&s.output, s.edge));
        if current == output.as_ref().map(|o| (o, edge)) {
            return;
        }
        self.reveal.sensor = None;
        self.reveal.over = false;
        let Some(output) = output else { return };
        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some(NAMESPACE), Some(&output));
        layer.set_anchor(edge_anchor(edge));
        layer.set_size(u32::from(edge.is_vertical()), u32::from(!edge.is_vertical()));
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer.commit();
        let viewport = self.pixels.viewporter.get_viewport(layer.wl_surface(), &self.qh, Ignore);
        self.reveal.sensor = Some(Sensor { output, edge, layer, viewport, buffer: None });
    }

    pub(super) fn reveal_owns(&self, surface: &WlSurface) -> bool {
        self.reveal.sensor.as_ref().is_some_and(|s| s.layer.wl_surface() == surface)
    }

    /// Mapped with one transparent pixel stretched over the line; its
    /// default input region is the whole line.
    pub(super) fn reveal_configure(&mut self, (width, height): (i32, i32)) {
        let Some(s) = self.reveal.sensor.as_mut() else { return };
        if s.buffer.is_some() {
            return;
        }
        let buffer = self.pixels.single_pixel.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, Ignore);
        s.viewport.set_destination(width.max(1), height.max(1));
        let surface = s.layer.wl_surface();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        s.layer.commit();
        s.buffer = Some(buffer);
    }

    pub(super) fn reveal_closed(&mut self) {
        self.reveal.sensor = None;
    }

    pub(super) fn reveal_output_gone(&mut self, output: &WlOutput) {
        if self.reveal.sensor.as_ref().is_some_and(|s| s.output == *output) {
            self.reveal.sensor = None;
        }
    }

    /// Every pointer event, ahead of the owners: where the pointer is, and
    /// the line's enter bringing the bar up.
    pub(super) fn reveal_pointer(&mut self, e: &PointerEvent, target: Option<Target>) {
        if self.reveal.sensor.is_none() && !self.reveal.shown {
            return;
        }
        match e.kind {
            PointerEventKind::Enter { .. } => {
                self.reveal.over = matches!(
                    target,
                    Some(Target::RevealEdge | Target::Bar | Target::Menu | Target::Panel | Target::Outgoing | Target::Tooltip)
                );
                if target == Some(Target::RevealEdge) && !self.reveal.shown {
                    self.reveal.shown = true;
                    crate::trace::note(format_args!("bar revealed over fullscreen"));
                    self.place_chrome();
                    self.refresh_bar(None);
                }
            }
            PointerEventKind::Leave { .. } => self.reveal.over = false,
            _ => {}
        }
        self.hold_reveal();
    }

    /// The bar kept while the pointer is on it or a card hangs off it, and
    /// taken down [`HOLD`] after neither holds.
    pub(super) fn hold_reveal(&mut self) {
        let keep = self.reveal.over || self.bar_busy();
        if !self.reveal.shown || keep {
            if let Some(t) = self.reveal.hide.take() {
                self.handle.remove(t);
            }
            return;
        }
        if self.reveal.hide.is_some() {
            return;
        }
        let token = self.handle.insert_source(Timer::from_duration(HOLD), |_, _, app: &mut App| {
            app.reveal.hide = None;
            if app.reveal.shown && !app.reveal.over && !app.bar_busy() {
                app.end_reveal();
                app.refresh_bar(None);
            }
            TimeoutAction::Drop
        });
        self.reveal.hide = token.ok();
    }

    fn end_reveal(&mut self) {
        if let Some(t) = self.reveal.hide.take() {
            self.handle.remove(t);
        }
        self.reveal.shown = false;
        crate::trace::note(format_args!("bar reveal ended"));
        self.place_chrome();
    }

    pub(super) fn reveal_report(&self) -> Value {
        json!({"sensor": self.reveal.sensor.is_some(), "shown": self.reveal.shown, "over": self.reveal.over})
    }
}
