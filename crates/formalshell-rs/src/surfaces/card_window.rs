//! The window a card on a layer surface of its own lives in: the panel host,
//! a modal, a popup and the tray menu each hold one. It carries the card and
//! its surface, the frame callback that maps or ticks the card, the
//! configure, the input region (sent only on a change), the hit test on the
//! card's live rect, and a popover's look when the compositor carries it.
//! Also here: the cut every cropped subsurface goes through.

use std::time::Instant;

use fs_theme::theme::Theme;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity;
use vello_cpu::kurbo::Rect;

use crate::scene::{IRect, NodeId, Paint, Scene};
use crate::surface::{Sub, Surface};
use crate::surfaces::card::Card;
use crate::trace;
use crate::wayland::App;

pub struct CardWindow {
    pub card: Card,
    pub surface: Surface,
    /// A popover's look, carried by the compositor.
    pub look: Option<Quads>,
    /// The input region last sent and the holes cut in it.
    region: Option<(IRect, Vec<IRect>)>,
}

impl CardWindow {
    /// The card's frame waits for the surface to be mapped.
    pub fn new(card: Card, mut surface: Surface) -> Self {
        surface.wait_map = true;
        Self { card, surface, look: None, region: None }
    }

    pub fn owns(&self, surface: &WlSurface) -> bool {
        self.surface.layer.wl_surface() == surface
    }

    /// A frame callback: the first maps the card, the rest tick it. True
    /// for the first.
    pub fn frame(&mut self, now: Instant) -> bool {
        let first = self.surface.landed(now);
        if first {
            self.card.mapped(now);
        } else {
            self.card.tick(now);
        }
        first
    }

    /// The compositor's configure: the surface takes the card's own size.
    pub fn configure(&mut self) {
        let size = self.card.scene.size;
        self.surface.configure(size.w, size.h);
    }

    /// Gone from the screen: closed and landed.
    pub fn finished(&self, now: Instant) -> bool {
        self.surface.mapped() && self.card.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.card.animating(now)
    }

    /// The card's look off a new theme, a compositor-carried look drawn
    /// again at the next present.
    pub fn restyle(&mut self, theme: &Theme, cast: bool, now: Instant) {
        self.card.restyle(theme, cast);
        self.card.tick(now);
        if let Some(q) = &mut self.look {
            q.size = (0, 0);
        }
    }

    /// Input over `want` less `holes`. True when that changed, which the
    /// next commit carries.
    pub fn sync_region(&mut self, compositor: &CompositorState, want: IRect, holes: &[IRect]) -> bool {
        if self.region.as_ref().is_some_and(|(w, h)| *w == want && h == holes) {
            return false;
        }
        if let Ok(region) = Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            for hole in holes {
                region.subtract(hole.x, hole.y, hole.w, hole.h);
            }
            self.surface.layer.set_input_region(Some(region.wl_region()));
        }
        self.region = Some((want, holes.to_vec()));
        true
    }

    /// A point on the surface inside the card's frame this frame.
    pub fn on_card(&self, x: f64, y: f64) -> bool {
        let r = self.card.live_rect();
        x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64
    }

    pub fn set_keyboard(&self, keyboard: KeyboardInteractivity) {
        self.surface.layer.set_keyboard_interactivity(keyboard);
        self.surface.layer.commit();
    }

    pub fn present(&mut self, animating: bool, qh: &QueueHandle<App>) {
        self.surface.present(&mut self.card.scene, animating, qh);
    }
}

/// What a subsurface cut by its `wp_viewport` last showed: how much of its
/// buffer and where on its parent.
#[derive(Default)]
pub struct Crop {
    shown: Option<((i32, i32), (i32, i32))>,
}

impl Crop {
    /// Shows `size` of `surface`'s buffer from its top left at `at` on its
    /// parent. Every viewport source starts at the buffer's origin: on
    /// Hyprland 0.56 a source with an offset drew its pixels stretched off
    /// the place given. An empty `size` is kept and sent nowhere. True when
    /// it moved.
    pub fn place(&mut self, surface: &mut Surface<Sub>, size: (i32, i32), at: (i32, i32)) -> bool {
        if self.shown == Some((size, at)) {
            return false;
        }
        self.shown = Some((size, at));
        if size.0 > 0 && size.1 > 0 {
            surface.layer.sub.set_position(at.0, at.1);
            surface.set_crop(IRect::new(0, 0, size.0, size.1));
        }
        true
    }

    /// The turn's commit: `scene` drawn when `draw`, and a commit of its own
    /// for a move that draw did not carry. True when the draw committed.
    pub fn commit(surface: &mut Surface<Sub>, scene: &mut Scene, draw: bool, moved: bool, qh: &QueueHandle<App>) -> bool {
        let before = surface.commits();
        if draw {
            surface.present(scene, false, qh);
        }
        let drew = surface.commits() != before;
        if moved && !drew {
            surface.recommit();
        }
        drew
    }
}

/// A popover card's look (casts, fill, border) drawn once and carried by
/// the compositor: four subsurfaces of the card, each holding one corner
/// of the look drawn at the largest size the card has taken, cropped by
/// its `wp_viewport` to the card's size this frame and faded by copying
/// its buffer again at the card's opacity. The edges between the corners are straight runs of
/// one fill, so any size up to the drawn one is the four crops side by
/// side, and the card's open, close and size morph cost no raster at all.
pub struct Quads {
    parts: Vec<Quad>,
    /// The card size the corners were last asked to be drawn at, and the
    /// room around it for the casts.
    size: (i32, i32),
    margin: i32,
    /// The largest size the owner says the card is headed for: drawn at
    /// once rather than grown into a frame at a time.
    pub hint: (i32, i32),
    /// The part of the card inside the border the corners draw this
    /// frame: short of the card while they are drawn again at a larger
    /// size, or while it overshoots the size they hold. The content is cut
    /// to it.
    pub covered: IRect,
    name: &'static str,
    started: Instant,
    steps: u64,
}

struct Quad {
    scene: Scene,
    nodes: (NodeId, NodeId),
    surface: Surface<Sub>,
    /// The card size the buffer the compositor holds was drawn at, and the
    /// one being drawn.
    drawn: Option<(i32, i32)>,
    pending: (i32, i32),
    crop: Crop,
}

/// One corner's share of a card `w` by `h`: the left (top) part is the
/// floor of half, the right (bottom) part the rest.
fn halves(n: i32) -> (i32, i32) {
    (n / 2, n - n / 2)
}

impl Quads {
    pub fn new(name: &'static str, parts: Vec<Surface<Sub>>, margin: i32, started: Instant) -> Self {
        let parts = parts
            .into_iter()
            .map(|surface| {
                let mut scene = Scene::clear(1, 1);
                let empty = || Paint::Shape { fill: None, strokes: Vec::new() };
                let casts = scene.add(IRect::default(), empty());
                let shape = scene.add(IRect::default(), empty());
                Quad { scene, nodes: (casts, shape), surface, drawn: None, pending: (0, 0), crop: Crop::default() }
            })
            .collect();
        Self { parts, size: (0, 0), margin, hint: (0, 0), covered: IRect::default(), name, started, steps: 0 }
    }

    /// Every corner holds a look the card's current size fits in.
    pub fn ready(&self, w: i32, h: i32) -> bool {
        self.parts.iter().all(|q| q.drawn.is_some_and(|(dw, dh)| dw >= w && dh >= h))
    }

    /// The four corners drawn at `w` by `h`: the top-left, top-right,
    /// bottom-left and bottom-right of the look, each with the margin on
    /// its outer sides.
    fn draw_at(&mut self, card: &Card, w: i32, h: i32) {
        self.size = (w, h);
        let m = self.margin;
        let (lw, rw) = halves(w);
        let (th, bh) = halves(h);
        let regions = [(0, 0, m + lw, m + th), (m + lw, 0, rw + m, m + th), (0, m + th, m + lw, bh + m), (m + lw, m + th, rw + m, bh + m)];
        for (q, (rx, ry, qw, qh)) in self.parts.iter_mut().zip(regions) {
            q.scene.resize(qw, qh);
            q.surface.configure(qw, qh);
            q.pending = (w, h);
            let at = Rect::new(f64::from(m - rx), f64::from(m - ry), f64::from(m - rx + w), f64::from(m - ry + h));
            card.popover_look(&mut q.scene, q.nodes, at);
        }
    }

    /// The corners placed, cut and faded for the card's frame this turn,
    /// drawn again first if the card has outgrown them. True when anything
    /// reached a corner.
    pub fn present(&mut self, card: &Card, qh: &QueueHandle<App>) -> bool {
        let (frame, alpha) = card.popover_frame;
        let rest = card.rest_rect();
        // An owner's hint is the size its morph settles on: a few pixels of
        // overshoot past it are clipped rather than drawn again.
        let (want_w, want_h) = if self.hint.0 > 0 { self.hint } else { (frame.w.max(rest.w), frame.h.max(rest.h)) };
        if want_w > self.size.0 || want_h > self.size.1 {
            self.draw_at(card, want_w.max(self.size.0), want_h.max(self.size.1));
        }
        let m = self.margin;
        let mut any = false;
        let mut moved = false;
        let mut covered = (frame.w, frame.h);
        // A look drawn again reaches the compositor only once all four
        // corners hold it: corners of two sizes side by side leave holes.
        let mut drawn = true;
        for q in &mut self.parts {
            // Ahead of the draw, so a corner drawn and faded on one frame
            // is copied once.
            q.surface.set_opacity(alpha);
            drawn &= q.surface.prepare(&mut q.scene);
        }
        for (i, q) in self.parts.iter_mut().enumerate() {
            let before = q.surface.commits();
            if drawn {
                q.surface.present(&mut q.scene, false, qh);
            }
            if q.surface.commits() != before {
                q.drawn = Some(q.pending);
                any = true;
            }
            let Some((dw, dh)) = q.drawn else {
                covered = (0, 0);
                continue;
            };
            // The card this frame, between the far corners' own size and
            // the size the corners hold. The far corners show whole, flush
            // with the card's far edges, and the near ones are cut from
            // their own origin to meet them.
            let (_, drw) = halves(dw);
            let (_, dbh) = halves(dh);
            let (w, h) = (frame.w.clamp(drw, dw), frame.h.clamp(dbh, dh));
            covered = (covered.0.min(w), covered.1.min(h));
            let (sx, sy) = (w - drw, h - dbh);
            let (size, place) = match i {
                0 => ((m + sx, m + sy), (frame.x - m, frame.y - m)),
                1 => ((drw + m, m + sy), (frame.x + sx, frame.y - m)),
                2 => ((m + sx, dbh + m), (frame.x - m, frame.y + sy)),
                _ => ((drw + m, dbh + m), (frame.x + sx, frame.y + sy)),
            };
            let placed = q.crop.place(&mut q.surface, size, place);
            // Corners still drawing take their new opacity with the commit
            // that finishes them.
            q.surface.set_opacity(if size.0 <= 0 || size.1 <= 0 { 0.0 } else { alpha });
            let faded = Crop::commit(&mut q.surface, &mut q.scene, drawn, placed, qh);
            any |= placed || faded;
            moved |= placed || faded;
        }
        // Inside the border along the far edges, which content laid out for
        // a wider card would cover while the card is still narrower.
        let bw = card.border_width().ceil() as i32;
        self.covered = IRect::new(frame.x, frame.y, covered.0 - bw, covered.1 - bw);
        // The card's motion lands here rather than in a buffer of its own,
        // so the commit log the budget legs read carries one line per step.
        if moved && trace::enabled() {
            self.steps += 1;
            trace::commit(
                format_args!("{}-look", self.name),
                self.steps,
                trace::since(self.started),
                format_args!("rect={}x{}+{}+{} alpha={:.3}", frame.w, frame.h, frame.x, frame.y, alpha),
            );
        }
        any
    }

    /// Any corner still drawing in slices.
    pub fn rastering(&self) -> bool {
        self.parts.iter().any(|q| q.surface.rastering())
    }
}
