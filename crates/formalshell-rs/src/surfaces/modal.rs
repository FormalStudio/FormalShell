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
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use smithay_client_toolkit::compositor::{CompositorState, Region};
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer as Level, LayerShell, LayerSurface};
use smithay_client_toolkit::shm::Shm;
use vello_cpu::kurbo::Rect;

use crate::chrome::Chrome;
use crate::scene::{IRect, NodeId, Paint, Scene};
use crate::surface::{Backend, Ignore, PixelSurface, Pixels, Sub, Surface};
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::card_window::{CardWindow, Crop, Quads};
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
    /// The output's scale.
    pub scale: f64,
    pub started: Instant,
    pub theme: &'a Theme,
    pub chrome: Chrome,
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
    pub win: CardWindow,
    band: Option<PixelSurface>,
    dim: PixelSurface,
    /// The layer namespace all three surfaces share, which the
    /// compositor's blur rule matches on.
    pub namespace: &'static str,
    pub open: bool,
    /// The owner's content, when it draws on a layer of its own rather
    /// than in the card's scene.
    pub layer: Option<Layer>,
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
        let output = make.chrome.output;
        let (edge, inset, ends) = make.chrome.modal_ends();
        // The scrim's alpha rides in its pixel, under the modal namespaces'
        // `ignore_alpha`, so it only darkens and the card alone is blurred.
        let tone = f64::from(theme.box_style(Role::Scrim, None).fill.a);
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
        let surface = Surface::new(names[0], Backend::Gpu, ls, make.shm, make.started, make.scale);
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
        let look = spec.compositor_look.then(|| make.look_quads(surface.name, &surface, card.popover_margin()));
        let mut win = CardWindow::new(card, surface);
        win.look = look;
        Self { win, band, dim, namespace, open: true, layer: None }
    }

    /// The largest card size the owner is headed for, so a popover's look
    /// is drawn once at it.
    pub fn look_hint(&mut self, w: i32, h: i32) {
        if let Some(q) = &mut self.win.look {
            q.hint = (q.hint.0.max(w), q.hint.1.max(h));
        }
    }

    /// The popover's look still being drawn in slices.
    pub fn look_rastering(&self) -> bool {
        self.win.look.as_ref().is_some_and(Quads::rastering)
    }

    /// Moves the card's resting rect, given on the output, drawn on this
    /// frame: content laid out in the new rect never shows over the card's
    /// last one. A second tick on one frame leaves the deform to the next
    /// (`Deform::step`).
    pub fn place(&mut self, rest: Rect, now: Instant) {
        let card = &mut self.win.card;
        let rest = card.edge_map().inverse().transform_rect_bbox(rest);
        if rest != card.rest() {
            card.set_rect(rest, rest);
            card.tick(now);
        }
    }

    /// Closes on the card's own clock and lets the keyboard go.
    pub fn close(&mut self, now: Instant) {
        if !self.open {
            return;
        }
        self.open = false;
        self.win.card.set_open(now, false);
        self.win.set_keyboard(KeyboardInteractivity::None);
    }

    /// Input everywhere while open, nowhere once closing.
    pub fn sync_region(&mut self, compositor: &CompositorState) {
        let want = if self.open { self.win.card.scene.size } else { IRect::default() };
        self.win.sync_region(compositor, want, &[]);
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
        let w = &mut self.win;
        let looked = w.look.as_mut().is_some_and(|q| q.present(&w.card, qh));
        let card_ready = w.surface.frame_pending() || w.surface.prepare(&mut w.card.scene);
        let card_due = w.card.scene.has_damage() || w.surface.rastering() || w.surface.drawn_ahead();
        let card_turn = card_ready && !(card_due && w.surface.frame_pending());
        // A look the compositor carries has just moved whatever the card's
        // own buffer does, so the content moves with it: held back a turn,
        // it pokes past a card that shrank under it.
        let layered = if card_turn || w.look.is_some() {
            let up = w.surface.mapped() && w.look.as_ref().is_none_or(|q| q.ready(1, 1));
            let bound = w.look.as_ref().map(|q| q.covered);
            self.layer.as_mut().is_some_and(|l| l.present(&w.card, !w.card.animating(now), up, bound, qh))
        } else {
            if let Some(l) = &mut self.layer {
                l.surface.prepare(&mut l.scene);
            }
            false
        };
        let before = w.surface.commits();
        if card_turn {
            w.present(animating, qh);
        }
        if (layered || looked) && w.surface.commits() == before {
            w.surface.layer.commit();
        }
        let pose = w.card.pose(now);
        let attach = w.card.attach(now);
        let moving = w.card.animating(now);
        if let Some(b) = &mut self.band {
            b.present(pose * (1.0 - attach), moving, qh);
        }
        self.dim.present(pose, moving, qh);
    }

    pub fn part(&self, surface: &WlSurface) -> Option<Part> {
        if self.win.owns(surface) {
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
                self.win.frame(now);
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
            Part::Card => self.win.configure(),
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
    /// What the compositor last got.
    crop: Crop,
}

impl Layer {
    /// Starts one pixel square; the first layout sizes it to the card.
    pub fn new(surface: Surface<Sub>) -> Self {
        let mut scene = Scene::clear(1, 1);
        let top = scene.add(IRect::default(), Paint::Shape { fill: None, strokes: Vec::new() });
        scene.set_visible(top, false);
        let mut layer = Self { scene, surface, top, laid: (0, 0), buffer: (0, 0), drawn: (0, 0), crop: Crop::default() };
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
        self.surface.set_opacity(self.cut(card, settled, card_up, bound).2);
        let before = self.surface.commits();
        self.surface.present(&mut self.scene, false, qh);
        let drew = self.surface.commits() != before;
        if drew {
            self.buffer = (self.scene.size.w, self.scene.size.h);
            self.drawn = self.laid;
        }
        // A cut the buffer cannot show leaves the last one standing.
        let (shown, fits, alpha) = self.cut(card, settled, card_up, bound);
        let moved = fits && self.crop.place(&mut self.surface, (shown.w, shown.h), (shown.x, shown.y));
        self.surface.set_opacity(alpha);
        // A layout still drawing in slices goes on with the next turn's
        // draw above, never on a commit placed for the last one.
        let draw = !self.surface.rastering();
        let faded = Crop::commit(&mut self.surface, &mut self.scene, draw, moved, qh);
        if moved && trace::enabled() {
            trace::commit_bare(
                format_args!("{}-cut", self.surface.name),
                format_args!("rect={}x{}+{}+{} fits={} alpha={:.3}", shown.w, shown.h, shown.x, shown.y, fits as u8, alpha),
            );
        }
        drew || moved || faded
    }

    /// This frame's cut of the content: the part shown, whether the
    /// compositor's buffer can show it, and its opacity.
    fn cut(&self, card: &Card, settled: bool, card_up: bool, bound: Option<IRect>) -> (IRect, bool, f32) {
        let at = self.place(card, settled);
        let shown = bound.map_or(self.visible(card, settled), |b| self.visible(card, settled).intersect(&b));
        // A viewport source has to be a non-empty part of the buffer the
        // compositor holds, starting at its origin (`Crop::place`). Until
        // such a cut fits, or while the content is cut at its top or left
        // (crossing the line), the last cut stays and the layer is faded
        // out to hide it.
        let fits = !shown.is_empty() && shown.x == at.x && shown.y == at.y
            && shown.right() - at.x <= self.buffer.0 && shown.bottom() - at.y <= self.buffer.1;
        let alpha = if fits && card_up { card.content.1.clamp(0.0, 1.0) } else { 0.0 };
        (shown, fits, alpha)
    }

    /// The layer gone, its pool, buffers and canvas kept for the next one.
    pub fn keep(self) -> crate::surface::Kept {
        self.surface.keep()
    }
}
