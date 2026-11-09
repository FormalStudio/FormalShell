//! A modal card (a drawer on the bar's edge with a scrim under it): the
//! card budding off the line on its own full-output surface, which holds
//! the keyboard while open, and the two single-pixel scrim surfaces riding
//! its pose: the band over the line's own inset, which fades in only as
//! the card lets go of the line (`1 - attach`), and the rest of the
//! output. The launcher, the polkit dialog, the radio atlas and a plugin's
//! overlay are each one of these; the owner lays its content out in
//! `card.content` under `card.content_clip()`.

use std::time::{Duration, Instant};

use fs_types::Edge;
use fs_theme::theme::Theme;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer as Level, LayerShell, LayerSurface};
use smithay_client_toolkit::shm::Shm;
use vello_cpu::kurbo::Rect;

use crate::scene::{IRect, NodeId, Paint, Scene};
use crate::surface::{Backend, Ignore, PixelSurface, Pixels, Sub, Surface};
use crate::surfaces::bar::Bar;
use crate::surfaces::card::{Card, CardSpec, Ends, Join, Sizing};
use crate::trace;
use crate::wayland::App;

/// How long one loop turn draws a modal's surfaces before handing the loop
/// back.
pub const SLICE: Duration = Duration::from_millis(3);

/// What a modal takes from the shell to make its surfaces on the live
/// bar's output and bud off that bar's line.
pub struct Make<'a> {
    pub compositor: &'a CompositorState,
    pub layer_shell: &'a LayerShell,
    pub pixels: &'a Pixels,
    pub shm: &'a Shm,
    pub qh: &'a QueueHandle<App>,
    pub output: Option<&'a WlOutput>,
    /// The output's size and scale.
    pub size: (f64, f64),
    pub scale: f64,
    pub started: Instant,
    pub theme: &'a Theme,
    pub bar: &'a Bar,
    pub motion_scale: f64,
    pub cast: bool,
}

impl Make<'_> {
    /// A layer surface on the output taking no input and reserving nothing.
    fn layer(&self, namespace: &'static str, layer: Level, anchor: Anchor, size: (u32, u32)) -> LayerSurface {
        let ls = self.layer_shell.create_layer_surface(self.qh, self.compositor.create_surface(self.qh), layer, Some(namespace), self.output);
        ls.set_anchor(anchor);
        ls.set_size(size.0, size.1);
        ls.set_exclusive_zone(-1);
        ls.set_keyboard_interactivity(KeyboardInteractivity::None);
        if let Ok(region) = Region::new(self.compositor) {
            ls.set_input_region(Some(region.wl_region()));
        }
        ls
    }

    /// The line a modal card buds off and how far in it lies: a side bar's
    /// inner edge, else Theme.edgeInset's top (the bar's thickness on a top
    /// bar, the frame ring's on a framed one, nothing on a bare edge).
    fn line(&self) -> (Edge, f64) {
        let bar = self.bar;
        let edge = bar.edge();
        if bar.hidden() {
            (Edge::Top, 0.0)
        } else if edge == Edge::Top || (edge.is_vertical() && !bar.framed()) {
            (edge, bar.thickness() as f64)
        } else if bar.framed() {
            (Edge::Top, bar.frame_thickness())
        } else {
            (Edge::Top, 0.0)
        }
    }

    /// A popover card's look as four subsurfaces of `card`, under anything
    /// added to it later, taking no input.
    fn look_quads(&self, name: &'static str, card: &Surface, margin: i32) -> Quads {
        let parts = (0..4)
            .map(|_| {
                let surface = self.compositor.create_surface(self.qh);
                let sub = self.pixels.subcompositor.get_subsurface(&surface, card.layer.wl_surface(), self.qh, Ignore);
                if let Ok(region) = Region::new(self.compositor) {
                    surface.set_input_region(Some(region.wl_region()));
                }
                let viewport = self.pixels.viewporter.get_viewport(&surface, self.qh, Ignore);
                let mut s = Surface::new(name, Backend::Gpu, Sub { surface, sub }, self.shm, self.started, self.scale).with_viewport(viewport).under(card);
                s.raster_budget = Some(SLICE);
                s
            })
            .collect();
        Quads::new(name, parts, margin, self.started)
    }
}

/// Which of a modal's three surfaces an event is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    Card,
    Band,
    Dim,
}

pub struct Modal {
    pub card: Card,
    pub surface: Surface,
    band: Option<PixelSurface>,
    dim: PixelSurface,
    /// The layer namespace all three surfaces share, which the
    /// compositor's blur rule matches on.
    pub namespace: &'static str,
    pub open: bool,
    region: Option<IRect>,
    /// The owner's content, when it draws on a layer of its own rather
    /// than in the card's scene.
    pub layer: Option<Layer>,
    /// A popover's look, carried by the compositor.
    quads: Option<Quads>,
}

impl Modal {
    /// A modal card's three surfaces on `layer` under `namespace`, named
    /// `names`: the card's own full-output one holding the keyboard, the
    /// band over the line's inset (none on a bare edge) and the dim under
    /// the rest. The card, `card`'s `state`, rests on the line until its
    /// owner places it. With `carry` a popover's look goes to the
    /// compositor, for an owner whose content draws on a layer of its own:
    /// content drawn in the card's own scene would sit under the look's
    /// subsurfaces, so those owners keep the look in that scene.
    pub fn open(make: &Make, names: [&'static str; 3], namespace: &'static str, layer: Level, state: Option<&'static str>, deform: f64, carry: bool) -> Self {
        let theme = make.theme;
        let output = make.size;
        let (edge, inset) = make.line();
        let framed = make.bar.framed() && edge == Edge::Top;
        let ft = if framed { make.bar.frame_thickness() } else { 0.0 };
        let along = if edge.is_vertical() { output.1 } else { output.0 };
        let ends = Ends { along, inset_start: ft, inset_end: ft, radius: if framed { theme.frame_radius } else { 0.0 } };
        // The scrim's alpha rides in its pixel, under the modal namespaces'
        // `ignore_alpha`, so it only darkens and the card alone is blurred.
        let tone = f64::from(theme.box_style("scrim", None).fill.a);
        let band = (inset > 0.0).then(|| {
            let size = if edge.is_vertical() { (inset as u32, 0) } else { (0, inset as u32) };
            let ls = make.layer(namespace, layer, crate::wayland::edge_anchor(edge), size);
            ls.commit();
            PixelSurface::new(names[1], ls, tone, make.pixels, make.qh, make.started)
        });
        let ls = make.layer(namespace, layer, Anchor::all(), (0, 0));
        let i = inset as i32;
        match edge {
            Edge::Top => ls.set_margin(i, 0, 0, 0),
            Edge::Right => ls.set_margin(0, i, 0, 0),
            Edge::Bottom => ls.set_margin(0, 0, i, 0),
            Edge::Left => ls.set_margin(0, 0, 0, i),
        }
        ls.commit();
        let dim = PixelSurface::new(names[2], ls, tone, make.pixels, make.qh, make.started);
        let ls = make.layer(namespace, layer, Anchor::all(), (0, 0));
        ls.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        ls.commit();
        let mut surface = Surface::new(names[0], Backend::Gpu, ls, make.shm, make.started, make.scale);
        surface.wait_map = true;
        let rest = Rect::new(0.0, inset, 0.0, inset);
        let size = if edge.is_vertical() { (output.1, output.0) } else { output };
        let spec = CardSpec {
            state,
            ends,
            deform,
            compositor_look: carry && !theme.morph,
            ..CardSpec::new(edge, inset, Sizing::Surface { size: (size.0 as i32, size.1 as i32), rest })
        };
        let card = Card::new(theme, spec, make.motion_scale, make.cast);
        let quads = spec.compositor_look.then(|| make.look_quads(surface.name, &surface, card.popover_margin()));
        Self { card, surface, band, dim, namespace, open: true, region: None, layer: None, quads }
    }

    /// The card's look off a new theme, the compositor's corners drawn
    /// again at the next present.
    pub fn restyle(&mut self, theme: &Theme, cast: bool, now: Instant) {
        self.card.restyle(theme, cast);
        self.card.tick(now);
        if let Some(q) = &mut self.quads {
            q.size = (0, 0);
        }
    }

    /// The largest card size the owner is headed for, so a popover's look
    /// is drawn once at it.
    pub fn look_hint(&mut self, w: i32, h: i32) {
        if let Some(q) = &mut self.quads {
            q.hint = (q.hint.0.max(w), q.hint.1.max(h));
        }
    }

    /// The popover's look still being drawn in slices.
    pub fn look_rastering(&self) -> bool {
        self.quads.as_ref().is_some_and(Quads::rastering)
    }

    /// Moves the card's resting rect, given on the output, drawn on this
    /// frame: content laid out in the new rect never shows over the card's
    /// last one. A second tick on one frame leaves the deform to the next
    /// (`Deform::step`).
    pub fn place(&mut self, rest: Rect, now: Instant) {
        let rest = self.card.edge_map().inverse().transform_rect_bbox(rest);
        if rest != self.card.rest() {
            self.card.set_rect(rest, rest);
            self.card.tick(now);
        }
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.surface.mapped() && self.card.finished(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.card.animating(now)
    }

    /// Closes on the card's own clock and lets the keyboard go.
    pub fn close(&mut self, now: Instant) {
        if !self.open {
            return;
        }
        self.open = false;
        self.card.set_open(now, false);
        self.surface.layer.set_keyboard_interactivity(KeyboardInteractivity::None);
        self.surface.layer.commit();
    }

    /// Input everywhere while open, nowhere once closing.
    pub fn sync_region(&mut self, compositor: &smithay_client_toolkit::compositor::CompositorState) {
        let want = if self.open { self.card.scene.size } else { IRect::default() };
        if self.region == Some(want) {
            return;
        }
        self.region = Some(want);
        if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor) {
            if !want.is_empty() {
                region.add(want.x, want.y, want.w, want.h);
            }
            self.surface.layer.set_input_region(Some(region.wl_region()));
        }
    }

    /// The card's frame, then the scrim on the card's own pose. The scrim
    /// asks for frames only while the card itself moves, never for the
    /// owner's content.
    pub fn present(&mut self, animating: bool, now: Instant, qh: &QueueHandle<App>) {
        // The layer is cut to the card's box in the compositor and placed by
        // the layout its last whole buffer holds, so the two never wait on
        // each other: a layout still being drawn in slices leaves the last
        // one showing inside a card that keeps moving. The layer commits
        // just ahead of the card, whose commit carries it.
        // A popover's look goes first, under the content: the content shows
        // only once the card is up and its look is there to sit on.
        let looked = self.quads.as_mut().is_some_and(|q| q.present(&self.card, qh));
        let card_ready = self.surface.frame_pending() || self.surface.prepare(&mut self.card.scene);
        let card_due = self.card.scene.has_damage() || self.surface.rastering() || self.surface.drawn_ahead();
        let card_turn = card_ready && !(card_due && self.surface.frame_pending());
        // A look the compositor carries has just moved whatever the card's
        // own buffer does, so the content moves with it: held back a turn,
        // it pokes past a card that shrank under it.
        let layered = if card_turn || self.quads.is_some() {
            let up = self.surface.mapped() && self.quads.as_ref().is_none_or(|q| q.ready(1, 1));
            let bound = self.quads.as_ref().map(|q| q.covered);
            self.layer.as_mut().is_some_and(|l| l.present(&self.card, !self.card.animating(now), up, bound, qh))
        } else {
            if let Some(l) = &mut self.layer {
                l.surface.prepare(&mut l.scene);
            }
            false
        };
        let before = self.surface.commits();
        if card_turn {
            self.surface.present(&mut self.card.scene, animating, qh);
        }
        if (layered || looked) && self.surface.commits() == before {
            self.surface.layer.commit();
        }
        let pose = self.card.pose(now);
        let attach = self.card.attach(now);
        let moving = self.card.animating(now);
        if let Some(b) = &mut self.band {
            b.present(pose * (1.0 - attach), moving, qh);
        }
        self.dim.present(pose, moving, qh);
    }

    pub fn part(&self, surface: &WlSurface) -> Option<Part> {
        if self.surface.layer.wl_surface() == surface {
            Some(Part::Card)
        } else if self.dim.layer.wl_surface() == surface {
            Some(Part::Dim)
        } else if self.band.as_ref().is_some_and(|b| b.layer.wl_surface() == surface) {
            Some(Part::Band)
        } else {
            None
        }
    }

    /// A frame callback; true when it was the card's, which has stepped.
    pub fn frame(&mut self, part: Part, now: Instant) -> bool {
        match part {
            Part::Card => {
                if self.surface.landed(now) {
                    self.card.mapped(now);
                } else {
                    self.card.tick(now);
                }
                true
            }
            Part::Band | Part::Dim => {
                let s = if part == Part::Band { self.band.as_mut() } else { Some(&mut self.dim) };
                if let Some(s) = s {
                    s.landed();
                }
                false
            }
        }
    }

    pub fn configure(&mut self, part: Part, width: i32, height: i32) {
        match part {
            Part::Card => {
                let size = self.card.scene.size;
                self.surface.configure(size.w, size.h);
            }
            Part::Band => {
                if let Some(b) = &mut self.band {
                    b.configure(width, height);
                }
            }
            Part::Dim => self.dim.configure(width, height),
        }
    }

    /// The scrim's surfaces as (name, pixel alpha, fade), for `debug dump`.
    pub fn scrims(&self) -> impl Iterator<Item = (&'static str, f64, f64)> + '_ {
        self.band.iter().chain(std::iter::once(&self.dim)).map(|s| (s.name, s.alpha, s.fade()))
    }

    pub fn on_card(&self, x: f64, y: f64) -> bool {
        let r = self.card.live_rect();
        x >= r.x as f64 && x < r.right() as f64 && y >= r.y as f64 && y < r.bottom() as f64
    }

    pub fn joins(&self) -> &[Join] {
        &self.card.joins
    }
}

/// The owner's content on a subsurface of the card, laid out once at the
/// size the card is headed for: the card's motion moves, fades and cuts
/// the layer in the compositor (`wl_subsurface.set_position` and a
/// `wp_viewport` source cropping it to the part inside the card and past
/// the line), so a frame of the card's travel or a size morph draws only
/// the card's own frame. A fade is the layer's own buffer copied again at
/// the card's opacity (`Surface::set_opacity`), never a layout.
pub struct Layer {
    pub scene: Scene,
    pub surface: Surface<Sub>,
    pub top: NodeId,
    /// The size the owner last laid its content out at.
    laid: (i32, i32),
    /// The size of the buffer the compositor last got, and the layout
    /// size it was drawn at.
    buffer: (i32, i32),
    drawn: (i32, i32),
    /// What the compositor last got: the shown part and where it sits.
    shown: Option<(IRect, IRect)>,
}

impl Layer {
    /// Starts one pixel square; the first layout sizes it to the card.
    pub fn new(surface: Surface<Sub>) -> Self {
        let mut scene = Scene::clear(1, 1);
        let top = scene.add(IRect::default(), Paint::Shape { fill: None, strokes: Vec::new() });
        scene.set_visible(top, false);
        let mut layer = Self { scene, surface, top, laid: (0, 0), buffer: (0, 0), drawn: (0, 0), shown: None };
        layer.surface.configure(1, 1);
        layer
    }

    /// Room for content of `w` by `h`, the size it is laid out at. The
    /// layer only grows: a size morph back and forth keeps its buffers.
    pub fn fit(&mut self, w: i32, h: i32) {
        self.laid = (w, h);
        let (w, h) = (w.max(self.scene.size.w), h.max(self.scene.size.h));
        if (w, h) != (self.scene.size.w, self.scene.size.h) {
            self.scene.resize(w, h);
            self.surface.configure(w, h);
        }
    }

    /// Where the content the compositor holds sits this frame on the
    /// card's surface: centred across the card's current box and on its
    /// top, moved by the card's pose. Once the card has settled it is the resting box, which
    /// may have moved on before the card next steps.
    pub fn place(&self, card: &Card, settled: bool) -> IRect {
        let (now, rest) = (card.content.0, card.live_rect());
        let (x, y) = if settled { (rest.x, rest.y) } else { (now.x, now.y) };
        // Content wider than the card hangs off its left edge, so the cut
        // starts at the content's own origin (see `present`).
        let x = if self.drawn.0 > rest.w { x } else { x + (rest.w - self.drawn.0) / 2 };
        IRect::new(x, y, self.drawn.0, self.drawn.1)
    }

    /// The part of the content that shows this frame, on the card's
    /// surface: inside the card's current box and its clip.
    fn visible(&self, card: &Card, settled: bool) -> IRect {
        let at = self.place(card, settled);
        let (now, rest) = (card.content.0, card.live_rect());
        let (x, y) = if settled { (rest.x, rest.y) } else { (now.x, now.y) };
        let shown = at.intersect(&IRect::new(x, y, rest.w, rest.h)).intersect(&card.clip);
        // A cut has to start at the content's own origin (see `cut`), so
        // the shape holds the content in on its right and bottom only.
        match card.shape_clip {
            Some(s) => shown.intersect(&IRect::new(at.x, at.y, (s.right() - at.x).max(0), (s.bottom() - at.y).max(0))),
            None => shown,
        }
    }

    /// The layer's draw, then its place, cut and fade off the card's pose.
    /// Hidden until the card itself is up (`card_up`): Hyprland shows a
    /// subsurface's commit at once, whatever its parent has shown yet. True
    /// when anything reached the subsurface.
    fn present(&mut self, card: &Card, settled: bool, card_up: bool, bound: Option<IRect>, qh: &QueueHandle<App>) -> bool {
        // The opacity goes in ahead of the draw, so a frame that both draws
        // and fades copies once; only a frame whose draw changes the cut
        // copies again.
        self.surface.set_opacity(self.cut(card, settled, card_up, bound).3);
        let before = self.surface.commits();
        self.surface.present(&mut self.scene, false, qh);
        let drew = self.surface.commits() != before;
        if drew {
            self.buffer = (self.scene.size.w, self.scene.size.h);
            self.drawn = self.laid;
        }
        let (shown, at, fits, alpha) = self.cut(card, settled, card_up, bound);
        let (shown, at) = match self.shown {
            Some((last, last_at)) if !fits => (last, last_at),
            _ => (shown, at),
        };
        let moved = self.shown != Some((shown, at));
        if moved {
            self.shown = Some((shown, at));
            if fits {
                self.surface.layer.sub.set_position(shown.x, shown.y);
                self.surface.set_crop(IRect::new(0, 0, shown.w, shown.h));
            }
        }
        self.surface.set_opacity(alpha);
        let before = self.surface.commits();
        // A layout still drawing in slices goes on with the next turn's
        // draw above, never on a commit placed for the last one.
        if !self.surface.rastering() {
            self.surface.present(&mut self.scene, false, qh);
        }
        let faded = self.surface.commits() != before;
        if moved && !faded {
            self.surface.recommit();
        }
        if moved && trace::enabled() {
            trace::commit_bare(
                format_args!("{}-cut", self.surface.name),
                format_args!("rect={}x{}+{}+{} fits={} alpha={:.3}", shown.w, shown.h, shown.x, shown.y, fits as u8, alpha),
            );
        }
        drew || moved || faded
    }

    /// This frame's cut of the content: the part shown, where it sits,
    /// whether the compositor's buffer can show it, and its opacity.
    fn cut(&self, card: &Card, settled: bool, card_up: bool, bound: Option<IRect>) -> (IRect, IRect, bool, f32) {
        let at = self.place(card, settled);
        let shown = bound.map_or(self.visible(card, settled), |b| self.visible(card, settled).intersect(&b));
        // A viewport source has to be a non-empty part of the buffer the
        // compositor holds, and it starts at the buffer's origin: on
        // Hyprland 0.56 a source with an offset drew its pixels stretched
        // off the place given. Until such a cut fits, or while the content
        // is cut at its top or left (crossing the line), the last cut stays
        // and the layer is faded out to hide it.
        let fits = !shown.is_empty() && shown.x == at.x && shown.y == at.y
            && shown.right() - at.x <= self.buffer.0 && shown.bottom() - at.y <= self.buffer.1;
        let alpha = if fits && card_up { card.content.1.clamp(0.0, 1.0) } else { 0.0 };
        (shown, at, fits, alpha)
    }

    /// The layer gone, its pool, buffers and canvas kept for the next one.
    pub fn keep(self) -> crate::surface::Kept {
        self.surface.keep()
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
    started: std::time::Instant,
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
    /// What the compositor last got: the crop and the place.
    shown: Option<(IRect, (i32, i32))>,
}

/// One corner's share of a card `w` by `h`: the left (top) part is the
/// floor of half, the right (bottom) part the rest.
fn halves(n: i32) -> (i32, i32) {
    (n / 2, n - n / 2)
}

impl Quads {
    pub fn new(name: &'static str, parts: Vec<Surface<Sub>>, margin: i32, started: std::time::Instant) -> Self {
        let parts = parts
            .into_iter()
            .map(|surface| {
                let mut scene = Scene::clear(1, 1);
                let empty = || Paint::Shape { fill: None, strokes: Vec::new() };
                let casts = scene.add(IRect::default(), empty());
                let shape = scene.add(IRect::default(), empty());
                Quad { scene, nodes: (casts, shape), surface, drawn: None, pending: (0, 0), shown: None }
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
    fn present(&mut self, card: &Card, qh: &QueueHandle<App>) -> bool {
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
            // their own origin to meet them. Every crop starts at 0,0: on
            // Hyprland 0.56 a viewport source with an offset drew the far
            // corners 51px short of where it put them.
            let (_, drw) = halves(dw);
            let (_, dbh) = halves(dh);
            let (w, h) = (frame.w.clamp(drw, dw), frame.h.clamp(dbh, dh));
            covered = (covered.0.min(w), covered.1.min(h));
            let (sx, sy) = (w - drw, h - dbh);
            let (crop, place) = match i {
                0 => (IRect::new(0, 0, m + sx, m + sy), (frame.x - m, frame.y - m)),
                1 => (IRect::new(0, 0, drw + m, m + sy), (frame.x + sx, frame.y - m)),
                2 => (IRect::new(0, 0, m + sx, dbh + m), (frame.x - m, frame.y + sy)),
                _ => (IRect::new(0, 0, drw + m, dbh + m), (frame.x + sx, frame.y + sy)),
            };
            let placed = q.shown != Some((crop, place));
            if placed {
                q.shown = Some((crop, place));
                if !crop.is_empty() {
                    q.surface.layer.sub.set_position(place.0, place.1);
                    q.surface.set_crop(crop);
                }
            }
            // Corners still drawing take their new opacity with the commit
            // that finishes them.
            q.surface.set_opacity(if crop.is_empty() { 0.0 } else { alpha });
            let before = q.surface.commits();
            if drawn {
                q.surface.present(&mut q.scene, false, qh);
            }
            let faded = q.surface.commits() != before;
            if placed && !faded {
                q.surface.recommit();
            }
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
