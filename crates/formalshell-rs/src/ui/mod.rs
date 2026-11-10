//! The shared widget set, the one way a surface puts
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
//! - A feature's own drawing is a [`Widget`] in `El::custom`, kept beside
//!   the panel it belongs to; what it keeps between draws is its [`State`],
//!   held here by its path.

pub mod boxes;
pub mod confirm;
mod draw;
pub use draw::{elided, icon, paint, resolve, ring_strokes, ring_turn, shape};
pub mod el;
pub mod field;
pub mod on;
pub mod spectrum;
pub mod thumb;
pub mod w;
mod widget;
pub use widget::{State, Widget};

use std::collections::HashMap;
use std::time::Instant;

use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{NodeId, Scene};
use fs_types::IRect;
use crate::paint::Kit;

pub use el::{El, Ink, Size, Type, Variant, Weight};
pub use on::On;

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
}

/// The pointer button a click came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    /// The element's `on` action.
    pub on: On,
    pub what: What,
    /// The button behind it; a key, a wheel or a drag reads as `Left`.
    pub button: Button,
}

/// One interactive rect from the last draw, topmost last.
#[derive(Clone, Debug)]
pub struct Hit {
    pub rect: IRect,
    pub path: String,
    pub on: Option<On>,
    pub tip: Option<String>,
    /// The cursor stop this rect belongs to, if any.
    pub stop: Option<On>,
    pub what: HitWhat,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitWhat {
    Click,
    Switch(bool),
    /// A track, its value running between this far in from either end
    /// (a slider's knob radius).
    Track(f64),
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
    pub key: On,
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

struct Kept {
    state: Box<dyn State>,
    seen: u64,
}

/// The most control heights a floating list shows before it scrolls.
const FLOAT_ROWS: f64 = 8.0;

/// Everything that persists for one surface between draws.
pub struct Ui {
    groups: HashMap<String, Group>,
    /// Each custom widget's state by its path.
    custom: HashMap<String, Kept>,
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
    /// Every keyed group's box (a quick settings tile, an open menu) by its
    /// key, as last drawn, unavailable ones included (they are no stops).
    pub keyed: Vec<(String, IRect)>,
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
    /// The field a card's rim reads, and the surface's top left on the
    /// output; `None` draws every card's border flat.
    pub lit: Option<(crate::paint::rim::Lit, (f64, f64))>,
    rims: HashMap<String, crate::paint::rim::Rim>,
    /// Where a floating list may go, over everything this surface drew;
    /// `None` keeps one inside the tree's own box.
    pub float_room: Option<IRect>,
    /// Each floating list's own scroll, by path, and the cursor stop it
    /// last kept in view.
    float_scroll: HashMap<String, (f64, Option<String>)>,
    /// The floating cards drawn last with how far each is scrolled, and
    /// the stops inside them.
    pub float_cards: Vec<(IRect, f64)>,
    float_keys: Vec<String>,
}

impl Ui {
    pub fn new(anchor: Option<NodeId>) -> Self {
        Self {
            groups: HashMap::new(),
            custom: HashMap::new(),
            tweens: HashMap::new(),
            births: HashMap::new(),
            wraps: HashMap::new(),
            measured: HashMap::new(),
            frame: 0,
            anchor,
            hits: Vec::new(),
            cards: Vec::new(),
            keyed: Vec::new(),
            stops: Vec::new(),
            hover: None,
            pressed: None,
            cursor: None,
            ring: false,
            halo_owned: true,
            fill_owned: false,
            lit: None,
            rims: HashMap::new(),
            float_room: None,
            float_scroll: HashMap::new(),
            float_cards: Vec::new(),
            float_keys: Vec::new(),
        }
    }

    /// Whether a point lies on a floating list's card.
    pub fn over_float(&self, x: f64, y: f64) -> bool {
        let p = IRect::new(x.floor() as i32, y.floor() as i32, 1, 1);
        self.float_cards.iter().any(|(c, _)| c.intersects(&p))
    }

    /// Whether a stop is a floating list's row.
    pub fn in_float(&self, key: &str) -> bool {
        self.float_keys.iter().any(|k| k == key)
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
        self.keyed.clear();
        self.stops.clear();
        self.float_cards.clear();
        self.float_keys.clear();
        let frame = self.frame;
        let last = self.anchor;
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
            floats: Vec::new(),
        };
        cx.ink = (theme.colors.get(ColorRole::Foreground), theme.colors.get(ColorRole::MutedForeground));
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
        self.custom.retain(|_, k| frame - k.seen < 120);
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

    pub fn hide(&mut self, scene: &mut Scene) {
        for g in self.groups.values() {
            for id in &g.nodes {
                scene.set_visible(*id, false);
            }
        }
    }

    pub fn hit(&self, x: f64, y: f64) -> Option<&Hit> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.hits.iter().rev().find(|h| x >= h.rect.x && x < h.rect.right() && y >= h.rect.y && y < h.rect.bottom())
    }

    /// The event a press of `button` released on `hit` at `x` fires.
    pub fn event(hit: &Hit, x: f64, button: Button) -> Option<Event> {
        let on = hit.on.clone()?;
        let what = match hit.what {
            HitWhat::Click => What::Click,
            HitWhat::Switch(checked) => What::Toggle(!checked),
            HitWhat::Track(inset) => What::Fraction(track_fraction(hit.rect, inset, x)),
            HitWhat::Pick(i) => What::Pick(i),
            HitWhat::Hover | HitWhat::Scroll => return None,
        };
        Some(Event { on, what, button })
    }

    /// The scroll region under a point, under whatever row sits on it.
    pub fn scroll_hit(&self, x: f64, y: f64) -> Option<&Hit> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.hits.iter().rev().find(|h| h.what == HitWhat::Scroll && x >= h.rect.x && x < h.rect.right() && y >= h.rect.y && y < h.rect.bottom())
    }

    /// Moves a scroll region's content by `delta` px.
    pub fn scroll(&mut self, path: &str, delta: f64) {
        if let Some(k) = self.custom.get_mut(path) {
            k.state.scroll(delta);
        }
        // Clamped to the list as it next draws.
        if let Some((s, _)) = self.float_scroll.get_mut(path) {
            *s -= delta;
        }
    }
}

/// Where a viewport can end on whole rows: the bottom of every child of
/// every column in `root` laid out at `width`, top down. A cell is one row,
/// so a grouped card is never cut through.
/// Where `x` falls along a track's hit `rect`, 0 to 1, its ends `inset` in.
pub fn track_fraction(rect: IRect, inset: f64, x: f64) -> f64 {
    let travel = (rect.w as f64 - inset * 2.0).max(1.0);
    ((x - rect.x as f64 - inset) / travel).clamp(0.0, 1.0)
}

pub fn row_ends(root: &El, width: f64, theme: &Theme, kit: &mut Kit) -> Vec<f64> {
    fn walk(node: &El, top: f64, width: f64, theme: &Theme, kit: &mut Kit, out: &mut Vec<f64>) {
        let el::Kind::Column { gap, children } = &node.kind else { return };
        let [ps, pt, pe, _] = node.pad;
        let inner = (width - ps - pe).max(0.0);
        let mut y = top + pt;
        for (i, c) in children.iter().enumerate() {
            if i > 0 {
                y += gap;
            }
            let h = if let el::Kind::Space { along } = c.kind { along } else { measure(c, inner, theme, kit).1 };
            walk(c, y, inner, theme, kit, out);
            y += h;
            // A column's first child heads the rest (a section's caption,
            // a choice's label): no end strands it over nothing.
            if i > 0 || children.len() == 1 {
                out.push(y);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0.0, width, theme, kit, &mut out);
    out
}

/// A body `content_h` tall shown in at most `cap`: all of it when it fits,
/// else down to the last whole row of `ends` (`row_ends`) that does.
pub fn fit_rows(ends: &[f64], content_h: f64, cap: f64) -> f64 {
    if content_h <= cap {
        return content_h;
    }
    match ends.iter().copied().filter(|&e| e <= cap && e > 0.0).fold(0.0, f64::max) {
        0.0 => cap,
        e => e,
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
        floats: Vec::new(),
    };
    draw::measure(&mut cx, root, width)
}

/// A custom widget's state while it paints (`Cx::take`).
pub struct Held<T> {
    path: String,
    kept: Box<T>,
}

impl<T> std::ops::Deref for Held<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.kept
    }
}

impl<T> std::ops::DerefMut for Held<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.kept
    }
}

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
    /// Lists waiting to float over the finished tree: each one's anchor, its
    /// content, its path, and where in `stops` its own stops go.
    floats: Vec<(Rect, El, String, usize)>,
}

impl Cx<'_> {
    /// A value riding `kind` toward `target`, retargeted from wherever it
    /// is; the first time a key is seen it lands at once.
    pub fn tween(&mut self, key: &str, target: f64, kind: Clock) -> f64 {
        let frame = self.ui.frame;
        let ms = kind.ms(self.theme);
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
        let ms = kind.ms(self.theme);
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

    /// Paints every waiting list on a popover card over the tree, under its
    /// anchor or over it. With a `float_room` the card lies over everything
    /// the surface drew, never moving it: at most `FLOAT_ROWS` control
    /// heights tall, under the anchor where that fits, else above it, else
    /// on whichever side has more room and cut to it, scrolling inside
    /// itself and keeping the cursor's row in view. Without one it stays in
    /// `bounds` and the bottom it reaches is handed back, for a surface that
    /// grows to show it. Its stops go in right after the anchor's.
    fn paint_floats(&mut self, bounds: Rect) -> f64 {
        let mut reach = bounds.y1;
        let floats = std::mem::take(&mut self.floats);
        let room = self.ui.float_room.map(|r| Rect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64));
        let mut shift = 0;
        // A list that closed opens again at its top.
        self.ui.float_scroll.retain(|p, _| floats.iter().any(|f| &f.2 == p));
        for (anchor, list, path, at) in floats {
            let s = self.theme.space.clone();
            let pad = s.xxs;
            let card = self.theme.box_style(Role::Popover, None);
            let area = room.unwrap_or(bounds);
            // Rows fill whatever they are given, so the list has no width of
            // its own: the anchor's, never under a narrow popup's.
            let w = anchor.width().max(s.popup_width_narrow).min(area.width());
            let content = draw::measure(self, &list, w - pad * 2.0).1;
            let full = content + pad * 2.0;
            let x = anchor.x0.min(area.x1 - w).max(area.x0);
            let below = anchor.y1 + s.xxs;
            let (y, h) = if room.is_some() {
                let want = full.min(s.control_height * FLOAT_ROWS + pad * 2.0);
                let (down, up) = (area.y1 - below, anchor.y0 - s.xxs - area.y0);
                if want <= down {
                    (below, want)
                } else if want <= up {
                    (anchor.y0 - s.xxs - want, want)
                } else if down >= up {
                    (below, down.max(0.0))
                } else {
                    (area.y0, up.max(0.0))
                }
            } else {
                let above = anchor.y0 - s.xxs - full;
                (if below + full > bounds.y1 && above >= bounds.y0 { above } else { below }, full)
            };
            reach = reach.max(y + h);
            let r = IRect::new(x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32);
            let view = (h - pad * 2.0).max(0.0);
            let max = (content - view).max(0.0);
            let (mut scroll, followed) = self.ui.float_scroll.get(&path).cloned().unwrap_or_default();
            let cursor = self.ui.cursor.clone();
            if max > 0.0 && cursor.is_some() && cursor != followed {
                // Where the cursor's row sits in the list, laid out unscrolled.
                let before = self.ui.stops.len();
                draw::offscreen_stops(self, &list, Rect::new(0.0, 0.0, w - pad * 2.0, content), &format!("{path}/list"));
                let row = self.ui.stops.drain(before..).find(|st| cursor.as_deref() == Some(st.key.as_str()));
                if let Some(st) = row {
                    let (top, bottom) = (st.rect.y as f64, st.rect.bottom() as f64);
                    if top < scroll {
                        scroll = top;
                    } else if bottom > scroll + view {
                        scroll = bottom - view;
                    }
                }
            }
            let scroll = scroll.clamp(0.0, max);
            self.ui.float_scroll.insert(path.clone(), (scroll, cursor));
            let alpha = self.alpha;
            let saved = self.clip;
            self.clip = if room.is_some() { Some(irect(area)) } else { saved };
            let radius = self.theme.box_radius(&card, h.min(s.control_height * 2.0));
            let mut p = self.painter(&path);
            boxes::paint(&mut p, r, &card, radius, alpha, 0.0);
            let last = p.last();
            p.finish();
            self.done(last);
            let what = if room.is_some() { HitWhat::Scroll } else { HitWhat::Hover };
            self.hit(Hit { rect: r, path: path.clone(), on: room.map(|_| "float".into()), tip: None, stop: None, what });
            let inner = IRect::new(r.x, (y + pad).round() as i32, r.w, view.round() as i32);
            if room.is_some() {
                self.clip = Some(inner.intersect(&irect(area)));
            }
            let (before, hits) = (self.ui.stops.len(), self.ui.hits.len());
            let top = y + pad - scroll;
            draw::paint(self, &list, Rect::new(x + pad, top, x + w - pad, top + content), &format!("{path}/list"));
            self.clip = saved;
            if room.is_some() {
                // A row cut by the card's edge answers only inside it.
                for hit in &mut self.ui.hits[hits..] {
                    hit.rect = hit.rect.intersect(&inner);
                }
                self.ui.float_cards.push((r, scroll));
            }
            let added: Vec<Stop> = self.ui.stops.drain(before..).collect();
            self.ui.float_keys.extend(added.iter().map(|st| st.key.to_string()));
            let at = (at + shift).min(before);
            shift += added.len();
            self.ui.stops.splice(at..at, added);
        }
        reach
    }

    /// The rim of the card at `path`, aimed at the field for its box `r`
    /// over a border `base`; handed back with [`Cx::put_rim`].
    pub fn take_rim(&mut self, path: &str, r: IRect, base: Rgba) -> Option<crate::paint::rim::Rim> {
        let Some((lit, origin)) = self.ui.lit.clone() else {
            self.ui.rims.remove(path);
            return None;
        };
        let mut rim = self.ui.rims.remove(path).unwrap_or_default();
        let card = Rect::new(r.x as f64 + origin.0, r.y as f64 + origin.1, r.right() as f64 + origin.0, r.bottom() as f64 + origin.1);
        rim.aim(self.now, Some(&lit), card, base, crate::paint::rim::blend_ms(self.theme));
        Some(rim)
    }

    pub fn put_rim(&mut self, path: &str, mut rim: crate::paint::rim::Rim) {
        self.drawn.animating |= rim.running(self.now);
        rim.painted();
        self.ui.rims.insert(path.to_owned(), rim);
    }

    pub fn painter(&mut self, path: &str) -> crate::paint::Painter<'_> {
        self.painter_kit(path).0
    }

    /// [`painter`](Self::painter) and the kit beside it, for drawing that
    /// shapes text as it goes.
    pub fn painter_kit(&mut self, path: &str) -> (crate::paint::Painter<'_>, &mut Kit) {
        let frame = self.ui.frame;
        let g = self.ui.groups.entry(path.to_owned()).or_default();
        g.seen = frame;
        // The next group follows this one's last node whatever it draws.
        self.last = g.nodes.last().copied().or(self.last);
        let anchor = if g.nodes.is_empty() { self.last } else { None };
        let p = crate::paint::Painter::new(self.scene, &mut g.nodes, self.clip).after(anchor);
        (p, self.kit)
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

    /// The state the custom widget at `path` keeps, `new` the first time
    /// (or after a widget of another kind held the path), out of `Ui`
    /// while the widget paints; [`Cx::keep`] hands it back.
    pub fn take<T: State>(&mut self, path: &str, new: impl FnOnce() -> T) -> Held<T> {
        match self.ui.custom.remove_entry(path) {
            Some((path, k)) => match (k.state as Box<dyn std::any::Any>).downcast::<T>() {
                Ok(kept) => Held { path, kept },
                Err(_) => Held { path, kept: Box::new(new()) },
            },
            None => Held { path: path.to_owned(), kept: Box::new(new()) },
        }
    }

    pub fn keep<T: State>(&mut self, held: Held<T>) {
        let seen = self.ui.frame;
        self.ui.custom.insert(held.path, Kept { state: held.kept, seen });
    }

    /// Marks `r` damaged whether its nodes changed or not (a group moving
    /// under a transform that leaves its own boxes).
    pub fn touch(&mut self, r: IRect) {
        self.scene.touch(r);
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

