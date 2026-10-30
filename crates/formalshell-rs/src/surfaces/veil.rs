//! Black over a whole output, faded in or out by its surface's opacity:
//! the greeter's way out to the session, and the shell's arrival from black
//! and departure into it (docs/superpowers/specs/2026-10-30-seams.md). On
//! unlock it carries the lock's last picture instead of black. The
//! caller makes the layer surface (Overlay, every edge anchored, an empty
//! input region) and routes its configure and frame callbacks here. The
//! shell's is a [`PixelSurface`], so a fade step is one multiplier; the
//! greeter, which binds no single-pixel buffers, draws its own black.

use std::time::{Duration, Instant};

use fs_theme::color::Rgba;
use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::reexports::client::protocol::wl_callback::WlCallback;
use smithay_client_toolkit::reexports::client::{Dispatch, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::WpFractionalScaleV1;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;

use crate::scene::{IRect, NodeId, Scene};
use crate::surface::{Inert, PixelSurface, Preferred, Surface};
use crate::surfaces::bar::cell::Painter;
use crate::wayland::App;

/// One fade: where it started, from and to which opacity, over how long.
#[derive(Clone, Copy)]
struct Fade {
    at: Instant,
    from: f32,
    to: f32,
    ms: f64,
}

pub struct Veil<S = Surface<LayerSurface>> {
    pub surface: S,
    scene: Scene,
    nodes: Vec<NodeId>,
    fade: Fade,
    /// The last commit carried the fade's end, and then a frame callback
    /// said the compositor has shown it.
    committed_end: bool,
    shown_end: bool,
    /// The scene is a picture handed over whole, drawn as it stands.
    held: bool,
}

impl<S> Veil<S> {
    /// A veil standing at `opacity` until the first `fade`.
    pub fn new(surface: S, opacity: f32, now: Instant) -> Self {
        Self { surface, scene: Scene::new(1, 1), nodes: Vec::new(), fade: Fade { at: now, from: opacity, to: opacity, ms: 0.0 }, committed_end: false, shown_end: false, held: false }
    }

    /// An opaque veil drawing `scene` rather than black.
    pub fn picture(surface: S, scene: Scene, now: Instant) -> Self {
        Self { scene, held: true, ..Self::new(surface, 1.0, now) }
    }

    /// From wherever the veil stands now to `to`, over `ms`.
    pub fn fade(&mut self, to: f32, ms: f64, now: Instant) {
        self.fade = Fade { at: now, from: self.opacity(now), to, ms };
        (self.committed_end, self.shown_end) = (false, false);
    }

    fn done(&self, now: Instant) -> bool {
        let f = self.fade;
        f.ms <= 0.0 || now.saturating_duration_since(f.at).as_secs_f64() * 1000.0 >= f.ms
    }

    pub fn opacity(&self, now: Instant) -> f32 {
        let f = self.fade;
        let t = if f.ms <= 0.0 { 1.0 } else { (now.saturating_duration_since(f.at).as_secs_f64() * 1000.0 / f.ms).min(1.0) };
        f.from + (f.to - f.from) * crate::motion::EFFECTS.ease(t) as f32
    }

    /// The fade has reached its end and the compositor has shown it.
    pub fn settled(&self) -> bool {
        self.shown_end
    }

    /// When the fade reaches its end, while that end is still to be shown.
    pub fn deadline(&self) -> Option<Instant> {
        (!self.shown_end).then(|| self.fade.at + Duration::from_secs_f64(self.fade.ms.max(0.0) / 1000.0))
    }
}

impl Veil<PixelSurface> {
    pub fn configure(&mut self, width: i32, height: i32) {
        self.surface.configure(width, height);
    }

    /// The surface's frame callback.
    pub fn landed(&mut self) {
        let s = &mut self.surface;
        (s.frame_pending, s.mapped, s.callbacks) = (false, true, s.callbacks + 1);
        self.shown_end |= self.committed_end;
    }

    /// Commits this moment's opacity; true until the fade's end has been
    /// shown.
    pub fn present(&mut self, now: Instant, qh: &QueueHandle<App>) -> bool {
        if self.surface.size().is_none() {
            return true;
        }
        // A fade out ends in the layer's destruction, so its end needs no
        // commit or callback: near alpha 0 Hyprland stops drawing the layer
        // and calling it back, until something else repaints the output,
        // which an idle desktop may not do for a minute. `deadline` wakes
        // the loop for it.
        if self.fade.to == 0.0 && self.done(now) {
            self.shown_end = true;
            return false;
        }
        let (done, before, waiting) = (self.done(now), self.surface.commits(), self.surface.frame_pending);
        self.surface.present(f64::from(self.opacity(now)), !self.shown_end, qh);
        if self.surface.commits() > before || (!waiting && self.surface.commits() > 0) {
            self.committed_end = done;
        }
        !self.shown_end
    }
}

impl Veil {
    pub fn configure(&mut self, width: i32, height: i32) {
        let (w, h) = (width.max(1), height.max(1));
        if self.held {
            self.scene.touch(self.scene.size);
        } else if (self.scene.size.w, self.scene.size.h) != (w, h) || self.nodes.is_empty() {
            self.scene.resize(w, h);
            let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
            p.rect(IRect::new(0, 0, w, h), Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
            p.finish();
            self.scene.touch(self.scene.size);
        }
        self.surface.configure(w, h);
    }

    /// The surface's frame callback.
    pub fn landed(&mut self, now: Instant) {
        self.surface.landed(now);
        self.surface.mapped = true;
        self.shown_end |= self.committed_end;
    }

    /// Draws the veil at this moment's opacity; true until the fade's end
    /// has been shown.
    pub fn present<D>(&mut self, now: Instant, qh: &QueueHandle<D>) -> bool
    where
        D: Dispatch<WlCallback, FrameCallbackData> + Dispatch<WpFractionalScaleV1, Preferred> + Dispatch<WpViewport, Inert> + 'static,
    {
        if !self.surface.configured {
            return true;
        }
        // As the shell's own veils: a fade out ends in the layer's
        // destruction, with no callback to wait on near alpha 0.
        if self.fade.to == 0.0 && self.done(now) {
            self.shown_end = true;
            return false;
        }
        self.surface.set_opacity(self.opacity(now));
        let (done, before, waiting) = (self.done(now), self.surface.commits(), self.surface.frame_pending);
        self.surface.present(&mut self.scene, !self.shown_end, qh);
        // The opacity is a 1/256 step, so the fade's last stretch can round
        // to the step already on screen: nothing is drawn, and the end is
        // already what the last commit carried.
        if self.surface.commits() > before || (!waiting && self.surface.commits() > 0) {
            self.committed_end = done;
        }
        !self.shown_end
    }
}
