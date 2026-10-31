//! The shared widget set (shell/Components/), the one way a surface puts
//! content on a card.
//!
//! The shape, for whoever builds the next surface on it:
//!
//! - A surface describes its content as an [`El`] tree (`el.rs`), built
//!   with the constructors in [`w`], each time it redraws. Building is
//!   cheap and keeps no state: a row is `w::cell(..).stop("key").on("act")`.
//! - [`Ui`] is what persists between two descriptions: each element's
//!   scene nodes and running animations, keyed by its path (an element's
//!   `key`, or its index under its parent). [`Ui::draw`] lays the tree out
//!   at a width and paints it. Nodes are reused in call order inside each
//!   element, so an unchanged frame damages nothing, and a widget that gains
//!   a node (a wash appearing) inserts it in its own place in paint order.
//! - Animations are retargeting tweens keyed the same way (`Cx::tween`):
//!   a value that has not changed costs no frame, and one that has runs on
//!   its `Anim` kind and reports itself in [`Drawn::animating`] until it
//!   lands. Nothing runs a clock at rest.
//! - Input comes back as data: a draw records each interactive element's
//!   rect ([`Hit`]) and each keyboard stop ([`Stop`]) in order; the host
//!   resolves the pointer and keys against those and hands the surface an
//!   [`Event`] naming the element's `on` action.

pub mod boxes;
mod draw;
pub use draw::{ring_strokes, ring_turn};
pub mod el;
pub mod lyrics;
mod spectrum;
mod strip;
pub mod w;

use std::collections::HashMap;
use std::time::Instant;

use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId, Scene};
use crate::surfaces::bar::cell::Kit;

pub use el::{El, Ink, Size, Type, Variant, Weight};

/// What a click, a drag or a key asked of an element.
#[derive(Clone, Debug, PartialEq)]
pub enum What {
    Click,
    Toggle(bool),
    /// One option of a group or a segmented control.
    Pick(usize),
    /// A point along a track, 0 to 1.
    Fraction(f64),
    /// A wheel notch: +1 up, -1 down.
    Wheel(i32),
    /// A scroll over a viewport, in wheel notches along x and y, positive
    /// moving the view right and down.
    Scroll(f64, f64),
    /// A right click on what a left click would have clicked.
    Right,
    /// A middle click on the same.
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The element's `on` action.
    pub on: String,
    pub what: What,
}

/// One interactive rect from the last draw, topmost last.
#[derive(Clone, Debug)]
pub struct Hit {
    pub rect: IRect,
    pub path: String,
    pub on: Option<String>,
    pub tip: Option<String>,
    /// The cursor stop this rect belongs to, if any.
    pub stop: Option<String>,
    pub what: HitWhat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitWhat {
    Click,
    Switch(bool),
    Track,
    Pick(usize),
    /// Hover only (a tooltip carrier, a static row).
    Hover,
    /// A region the wheel scrolls: a viewport along both axes (a
    /// workspace's miniature), or a pane the host moves itself (lyrics).
    Scroll,
}

/// One keyboard stop, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub key: String,
    pub rect: IRect,
    pub radius: f64,
}

#[derive(Default)]
struct Group {
    nodes: Vec<NodeId>,
    seen: u64,
}

enum TweenValue {
    Num(Animated),
    Color { from: Rgba, to: Rgba, t: Animated },
}

struct Tween {
    value: TweenValue,
    seen: u64,
}

/// What a draw left behind.
#[derive(Default)]
pub struct Drawn {
    pub width: f64,
    pub height: f64,
    /// The last node in paint order, for a caller stacking more after it.
    pub last: Option<NodeId>,
    pub animating: bool,
    /// The next moment something holding still starts moving (a marquee's hold).
    pub wake: Option<Instant>,
}

#[derive(Hash, PartialEq, Eq)]
struct WrapKey {
    text: String,
    style: String,
    max: u64,
    lines: usize,
}

struct Wrapped {
    lines: Vec<crate::text::ShapedText>,
    seen: u64,
}

/// Everything that persists for one surface between draws.
pub struct Ui {
    groups: HashMap<String, Group>,
    spectra: HashMap<String, spectrum::Live>,
    panes: HashMap<String, lyrics::Pane>,
    tweens: HashMap<String, Tween>,
    births: HashMap<String, Instant>,
    /// Each paragraph's lines by text, style, width and line cap. Wrapping
    /// shapes every word prefix, so a long list of them overran the shaper's
    /// cache and reshaped all of it on every frame.
    wraps: HashMap<WrapKey, Wrapped>,
    /// Container sizes by element and width, for this draw only: painting a
    /// column measures each child again, and each of those measures its own
    /// children again, so a long list cost its size squared.
    measured: HashMap<(usize, u64), (f64, f64)>,
    frame: u64,
    /// Nodes this surface adds go after this one in paint order.
    pub anchor: Option<NodeId>,
    pub hits: Vec<Hit>,
    /// Every sidebar card's box, as last drawn.
    pub cards: Vec<IRect>,
    pub stops: Vec<Stop>,
    /// The hit path under the pointer, and the one a button went down on.
    pub hover: Option<String>,
    pub pressed: Option<String>,
    /// The stop holding the cursor, and whether the keyboard put it there
    /// (DESIGN.md §1 "Ring": only then does the ring draw).
    pub cursor: Option<String>,
    pub ring: bool,
    /// The container draws one travelling halo for every row, so rows do
    /// not draw their own.
    pub halo_owned: bool,
    /// The container draws the selected fill and the cursor ring once for
    /// the whole list, travelling, so a selected cell draws neither.
    pub fill_owned: bool,
    /// `debug motionScale` over the theme's clocks.
    pub motion_scale: f64,
    /// The field a card's rim reads, and the surface's top left on the
    /// output; `None` draws every card's border flat.
    pub lit: Option<(crate::surfaces::rim::Lit, (f64, f64))>,
    rims: HashMap<String, crate::surfaces::rim::Rim>,
}

impl Ui {
    pub fn new(anchor: Option<NodeId>) -> Self {
        Self {
            groups: HashMap::new(),
            spectra: HashMap::new(),
            panes: HashMap::new(),
            tweens: HashMap::new(),
            births: HashMap::new(),
            wraps: HashMap::new(),
            measured: HashMap::new(),
            frame: 0,
            anchor,
            hits: Vec::new(),
            cards: Vec::new(),
            stops: Vec::new(),
            hover: None,
            pressed: None,
            cursor: None,
            ring: false,
            halo_owned: true,
            fill_owned: false,
            motion_scale: 1.0,
            lit: None,
            rims: HashMap::new(),
        }
    }

    /// Lays `root` out at `rect`'s width from its top-left and paints it
    /// under `clip`, at `alpha`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        root: &El,
        rect: Rect,
        clip: Option<IRect>,
        alpha: f32,
        theme: &Theme,
        kit: &mut Kit,
        scene: &mut Scene,
        now: Instant,
    ) -> Drawn {
        self.frame += 1;
        self.measured.clear();
        self.hits.clear();
        self.cards.clear();
        self.stops.clear();
        let frame = self.frame;
        let last = self.anchor;
        let motion_scale = self.motion_scale;
        let mut cx = Cx {
            theme,
            kit,
            scene,
            ui: self,
            now,
            alpha,
            clip,
            ink: (Rgba::TRANSPARENT, Rgba::TRANSPARENT),
            last,
            drawn: Drawn::default(),
            motion_scale,
            floats: Vec::new(),
        };
        cx.ink = (theme.colors.get("foreground"), theme.colors.get("mutedForeground"));
        let (w, h) = draw::measure(&mut cx, root, rect.width());
        let bounds = Rect::new(rect.x0, rect.y0, rect.x0 + rect.width(), rect.y0 + h);
        draw::paint(&mut cx, root, bounds, "");
        let reach = cx.paint_floats(bounds);
        let last = cx.last;
        let mut drawn = std::mem::take(&mut cx.drawn);
        drawn.width = w;
        drawn.height = h.max(reach - rect.y0);
        drawn.last = last;
        // Whatever was not drawn this time is gone from the screen; a group
        // unseen for a while gives its nodes back.
        let stale: Vec<String> = self.groups.iter().filter(|(_, g)| g.seen != frame).map(|(k, _)| k.clone()).collect();
        for k in stale {
            let g = self.groups.get_mut(&k).expect("group");
            if frame - g.seen > 120 {
                for id in g.nodes.drain(..) {
                    scene.remove(id);
                }
                self.groups.remove(&k);
                self.births.remove(&k);
                self.rims.remove(&k);
            } else {
                for id in &g.nodes {
                    scene.set_visible(*id, false);
                }
            }
        }
        self.tweens.retain(|_, t| frame - t.seen < 120);
        self.spectra.retain(|_, l| frame - l.seen < 120);
        self.panes.retain(|_, l| frame - l.seen < 120);
        self.wraps.retain(|_, w| frame - w.seen < 120);
        drawn
    }

    /// Puts every node this surface drew under `transform`. The painters
    /// compare against an untransformed node, so a caller resets this to
    /// the identity before `draw` and sets it again after.
    pub fn set_transform(&self, scene: &mut Scene, transform: vello_cpu::kurbo::Affine) {
        for g in self.groups.values() {
            for id in &g.nodes {
                scene.set_transform(*id, transform);
            }
        }
    }

    /// Hides everything this surface drew.
    pub fn hide(&mut self, scene: &mut Scene) {
        for g in self.groups.values() {
            for id in &g.nodes {
                scene.set_visible(*id, false);
            }
        }
    }

    /// The topmost hit under a point.
    pub fn hit(&self, x: f64, y: f64) -> Option<&Hit> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.hits.iter().rev().find(|h| x >= h.rect.x && x < h.rect.right() && y >= h.rect.y && y < h.rect.bottom())
    }

    /// The event a press released on `hit` at `x` fires.
    pub fn event(hit: &Hit, x: f64) -> Option<Event> {
        let on = hit.on.clone()?;
        let what = match hit.what {
            HitWhat::Click => What::Click,
            HitWhat::Switch(checked) => What::Toggle(!checked),
            HitWhat::Track => What::Fraction(((x - hit.rect.x as f64) / hit.rect.w.max(1) as f64).clamp(0.0, 1.0)),
            HitWhat::Pick(i) => What::Pick(i),
            HitWhat::Hover | HitWhat::Scroll => return None,
        };
        Some(Event { on, what })
    }

    /// The scroll region under a point, under whatever row sits on it.
    pub fn scroll_hit(&self, x: f64, y: f64) -> Option<&Hit> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.hits.iter().rev().find(|h| h.what == HitWhat::Scroll && x >= h.rect.x && x < h.rect.right() && y >= h.rect.y && y < h.rect.bottom())
    }

    /// Moves a scroll region's content by `delta` px.
    pub fn scroll(&mut self, path: &str, delta: f64) {
        if let Some(p) = self.panes.get_mut(path) {
            p.scroll(delta);
        }
    }
}

/// An element's size at `width` without drawing it.
pub fn measure(root: &El, width: f64, theme: &Theme, kit: &mut Kit) -> (f64, f64) {
    let mut scene = Scene::clear(1, 1);
    let mut ui = Ui::new(None);
    let mut cx = Cx {
        theme,
        kit,
        scene: &mut scene,
        ui: &mut ui,
        now: Instant::now(),
        alpha: 1.0,
        clip: None,
        ink: (Rgba::TRANSPARENT, Rgba::TRANSPARENT),
        last: None,
        drawn: Drawn::default(),
        motion_scale: 1.0,
        floats: Vec::new(),
    };
    draw::measure(&mut cx, root, width)
}

/// One draw in progress.
pub struct Cx<'a> {
    pub theme: &'a Theme,
    pub kit: &'a mut Kit,
    scene: &'a mut Scene,
    ui: &'a mut Ui,
    pub now: Instant,
    pub alpha: f32,
    pub clip: Option<IRect>,
    /// The enclosing cell's foreground and dim inks.
    pub ink: (Rgba, Rgba),
    last: Option<NodeId>,
    drawn: Drawn,
    pub motion_scale: f64,
    /// Lists waiting to float over the finished tree: each one's anchor, its
    /// content, its path, and where in `stops` its own stops go.
    floats: Vec<(Rect, El, String, usize)>,
}

impl Cx<'_> {
    /// A value riding `kind` toward `target`, retargeted from wherever it
    /// is; the first time a key is seen it lands at once.
    pub fn tween(&mut self, key: &str, target: f64, kind: Clock) -> f64 {
        let frame = self.ui.frame;
        let ms = kind.ms(self.theme) * self.motion_scale;
        let now = self.now;
        let t = self.ui.tweens.entry(key.to_owned()).or_insert_with(|| Tween {
            value: TweenValue::Num(Animated::new(target, kind.curve())),
            seen: frame,
        });
        t.seen = frame;
        let TweenValue::Num(a) = &mut t.value else {
            t.value = TweenValue::Num(Animated::new(target, kind.curve()));
            return target;
        };
        a.set(now, target, ms);
        if a.running(now) {
            self.drawn.animating = true;
        }
        a.value(now)
    }

    /// A swap of what a node shows (CalendarPanel's month step): the new
    /// content fades up from one spacing step off rest, on the side it came
    /// from. A swap still running keeps its own direction and finishes.
    /// Returns the progress, 0 to 1, and that direction.
    pub fn swap(&mut self, path: &str, token: i64) -> (f64, f64) {
        let (tok, prog, dir) = (format!("{path}.token"), format!("{path}.swap"), format!("{path}.dir"));
        let now = self.now;
        let peek = |ui: &Ui, key: &str| match ui.tweens.get(key).map(|t| &t.value) {
            Some(TweenValue::Num(a)) => Some(a.value(now)),
            _ => None,
        };
        let previous = peek(self.ui, &tok);
        let running = peek(self.ui, &prog).is_some_and(|p| p < 1.0 - 1e-6);
        self.jump(&tok, token as f64, Clock::Effects);
        if let Some(p) = previous.filter(|p| *p != token as f64 && !running) {
            self.jump(&dir, if token as f64 >= p { 1.0 } else { -1.0 }, Clock::Effects);
            self.jump(&prog, 0.0, Clock::Effects);
        }
        let progress = self.tween(&prog, 1.0, Clock::Effects);
        let sign = peek(self.ui, &dir).unwrap_or(1.0);
        self.jump(&dir, sign, Clock::Effects);
        (progress, sign)
    }

    /// Puts a tween's value at `value` with nothing running.
    pub fn jump(&mut self, key: &str, value: f64, kind: Clock) {
        let frame = self.ui.frame;
        let t = self.ui.tweens.entry(key.to_owned()).or_insert_with(|| Tween { value: TweenValue::Num(Animated::new(value, kind.curve())), seen: frame });
        t.seen = frame;
        match &mut t.value {
            TweenValue::Num(a) => a.jump(value),
            v => *v = TweenValue::Num(Animated::new(value, kind.curve())),
        }
    }

    /// A colour crossfading to `target` (`CAnim`, always `effectsSlow`).
    pub fn color(&mut self, key: &str, target: Rgba) -> Rgba {
        let frame = self.ui.frame;
        let kind = Clock::EffectsSlow;
        let ms = kind.ms(self.theme) * self.motion_scale;
        let now = self.now;
        let t = self.ui.tweens.entry(key.to_owned()).or_insert_with(|| Tween {
            value: TweenValue::Color { from: target, to: target, t: Animated::new(1.0, kind.curve()) },
            seen: frame,
        });
        t.seen = frame;
        let TweenValue::Color { from, to, t: a } = &mut t.value else {
            t.value = TweenValue::Color { from: target, to: target, t: Animated::new(1.0, kind.curve()) };
            return target;
        };
        if *to != target {
            let p = a.value(now).clamp(0.0, 1.0) as f32;
            *from = lerp(*from, *to, p);
            *to = target;
            a.jump(0.0);
            a.set(now, 1.0, ms);
        }
        if a.running(now) {
            self.drawn.animating = true;
        }
        lerp(*from, *to, a.value(now).clamp(0.0, 1.0) as f32)
    }

    pub fn wake_at(&mut self, at: Instant) {
        self.drawn.wake = Some(self.drawn.wake.map_or(at, |w| w.min(at)));
    }

    pub fn animate(&mut self) {
        self.drawn.animating = true;
    }

    /// The painter for one element's own nodes.
    pub(crate) fn float(&mut self, anchor: Rect, list: El, path: String) {
        let at = self.ui.stops.len();
        self.floats.push((anchor, list, path, at));
    }

    /// Paints every waiting list on a popover card over the tree: under its
    /// anchor, or over it when the tree ends too soon below and there is
    /// room above, kept inside `bounds` across. Its stops go in right after the anchor's. The bottom the
    /// lowest list reaches, for a surface that has to grow to show it.
    fn paint_floats(&mut self, bounds: Rect) -> f64 {
        let mut reach = bounds.y1;
        let floats = std::mem::take(&mut self.floats);
        let mut shift = 0;
        for (anchor, list, path, at) in floats {
            let s = self.theme.space.clone();
            let pad = s.xxs;
            let card = self.theme.box_style("popover", None);
            // Rows fill whatever they are given, so the list has no width of
            // its own: the anchor's, never under a narrow popup's.
            let w = anchor.width().max(s.popup_width_narrow).min(bounds.width());
            let h = draw::measure(self, &list, w - pad * 2.0).1 + pad * 2.0;
            let x = anchor.x0.min(bounds.x1 - w).max(bounds.x0);
            let below = anchor.y1 + s.xxs;
            let above = anchor.y0 - s.xxs - h;
            let y = if below + h > bounds.y1 && above >= bounds.y0 { above } else { below };
            reach = reach.max(y + h);
            let r = IRect::new(x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32);
            let radius = self.theme.box_radius(&card, h.min(s.control_height * 2.0));
            let alpha = self.alpha;
            let mut p = self.painter(&path);
            boxes::paint(&mut p, r, &card, radius, alpha, 0.0);
            let last = p.last();
            p.finish();
            self.done(last);
            self.hit(Hit { rect: r, path: path.clone(), on: None, tip: None, stop: None, what: HitWhat::Hover });
            let before = self.ui.stops.len();
            draw::paint(self, &list, Rect::new(x + pad, y + pad, x + w - pad, y + h - pad), &format!("{path}/list"));
            let added: Vec<Stop> = self.ui.stops.drain(before..).collect();
            let at = (at + shift).min(before);
            shift += added.len();
            self.ui.stops.splice(at..at, added);
        }
        reach
    }

    /// The rim of the card at `path`, aimed at the field for its box `r`
    /// over a border `base`; handed back with [`Cx::put_rim`].
    pub fn take_rim(&mut self, path: &str, r: IRect, base: Rgba) -> Option<crate::surfaces::rim::Rim> {
        let Some((lit, origin)) = self.ui.lit.clone() else {
            self.ui.rims.remove(path);
            return None;
        };
        let mut rim = self.ui.rims.remove(path).unwrap_or_default();
        let card = Rect::new(r.x as f64 + origin.0, r.y as f64 + origin.1, r.right() as f64 + origin.0, r.bottom() as f64 + origin.1);
        rim.aim(self.now, Some(&lit), card, base, crate::surfaces::rim::blend_ms(self.theme, self.motion_scale));
        Some(rim)
    }

    pub fn put_rim(&mut self, path: &str, mut rim: crate::surfaces::rim::Rim) {
        self.drawn.animating |= rim.running(self.now);
        rim.painted();
        self.ui.rims.insert(path.to_owned(), rim);
    }

    pub fn painter(&mut self, path: &str) -> crate::surfaces::bar::cell::Painter<'_> {
        let frame = self.ui.frame;
        let g = self.ui.groups.entry(path.to_owned()).or_default();
        g.seen = frame;
        // The next group follows this one's last node whatever it draws.
        self.last = g.nodes.last().copied().or(self.last);
        let anchor = if g.nodes.is_empty() { self.last } else { None };
        crate::surfaces::bar::cell::Painter::new(self.scene, &mut g.nodes, self.clip).after(anchor)
    }

    /// Records where the group just painted ended in paint order.
    pub fn done(&mut self, last: Option<NodeId>) {
        if last.is_some() {
            self.last = last;
        }
    }

    pub fn hover(&self, path: &str) -> bool {
        self.ui.hover.as_deref() == Some(path)
    }

    pub fn pressed(&self, path: &str) -> bool {
        self.ui.pressed.as_deref() == Some(path) && self.hover(path)
    }

    /// Whether `key` holds the cursor, and whether its ring draws.
    pub fn cursor(&self, key: Option<&str>) -> (bool, bool) {
        let on = key.is_some() && self.ui.cursor.as_deref() == key;
        (on, on && self.ui.ring)
    }

    pub fn halo_owned(&self) -> bool {
        self.ui.halo_owned
    }

    pub fn fill_owned(&self) -> bool {
        self.ui.fill_owned
    }

    pub fn hit(&mut self, hit: Hit) {
        if !hit.rect.is_empty() {
            let rect = self.clip.map_or(hit.rect, |c| c.intersect(&hit.rect));
            if !rect.is_empty() {
                self.ui.hits.push(Hit { rect, ..hit });
            }
        }
    }

    pub fn hits_len(&self) -> usize {
        self.ui.hits.len()
    }

    /// A hit under those recorded since `at` (a row under its own switch).
    pub fn hit_at(&mut self, at: usize, hit: Hit) {
        let rect = self.clip.map_or(hit.rect, |c| c.intersect(&hit.rect));
        if !rect.is_empty() {
            self.ui.hits.insert(at.min(self.ui.hits.len()), Hit { rect, ..hit });
        }
    }

    pub fn ui_ring(&self) -> bool {
        self.ui.ring
    }

    /// When an element was first drawn, for a loop that runs off its own start.
    pub fn born(&mut self, path: &str) -> Instant {
        let now = self.now;
        *self.ui.births.entry(path.to_owned()).or_insert(now)
    }

    pub fn stop(&mut self, stop: Stop) {
        self.ui.stops.push(stop);
    }

    pub fn a(&self, c: Rgba) -> Rgba {
        c.with_alpha(c.a * self.alpha)
    }
}

pub fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    a.mix(b, t)
}

pub fn irect(r: Rect) -> IRect {
    let (x0, y0) = (r.x0.round() as i32, r.y0.round() as i32);
    IRect::new(x0, y0, r.x1.round() as i32 - x0, r.y1.round() as i32 - y0)
}

