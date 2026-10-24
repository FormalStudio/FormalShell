//! One card per output, shown 400ms
//! after the pointer parks on an item and, within 500ms of the last one
//! standing, handed to the next item at once, travelling there on
//! `spatialFast` with its text crossfading on `effects`. The surface exists
//! only while the card is drawn.

use std::time::{Duration, Instant};

use fs_chrome::types::Edge;
use fs_theme::theme::Theme;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId, Scene};
use crate::surface::Surface;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{ShapedText, TextStyle};

/// omarchy's own delay, and the group's grace after a hide.
const DELAY: Duration = Duration::from_millis(400);
const GRACE: Duration = Duration::from_millis(500);
/// The widest a line runs before it elides, at a font scale of 1.
const MAX_TEXT: f64 = 360.0;

/// What one item asks the group for.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask {
    /// Who is asking: a hide from anyone else is ignored.
    pub owner: String,
    pub text: String,
    /// The item's rect in the output's coordinates.
    pub rect: IRect,
    /// The bar edge it opens away from; none off the bar.
    pub bar: Option<Edge>,
}

/// The tooltip group shared by every output.
#[derive(Default)]
pub struct Group {
    target: Option<Ask>,
    pub shown: Option<Ask>,
    pub drawn: bool,
    delay_until: Option<Instant>,
    grace_until: Option<Instant>,
    /// The commit hands the card over rather than entering it.
    pub travel: bool,
}

impl Group {
    pub fn show(&mut self, ask: Ask, now: Instant) {
        if ask.text.is_empty() {
            return;
        }
        if self.target.as_ref().is_some_and(|t| t.owner == ask.owner) {
            if self.shown.is_some() {
                self.shown = Some(ask.clone());
            }
            self.target = Some(ask);
            return;
        }
        self.target = Some(ask);
        if self.shown.is_some() || self.drawn || self.grace_until.is_some_and(|g| now < g) {
            self.delay_until = None;
            self.commit();
        } else {
            self.delay_until = Some(now + DELAY);
        }
    }

    pub fn hide(&mut self, owner: Option<&str>, now: Instant) {
        if owner.is_some_and(|o| self.target.as_ref().is_some_and(|t| t.owner != o)) {
            return;
        }
        self.delay_until = None;
        if self.shown.is_some() {
            self.grace_until = Some(now + GRACE);
        }
        self.shown = None;
        self.target = None;
    }

    fn commit(&mut self) {
        self.travel = self.drawn;
        self.shown = self.target.clone();
    }

    /// The delay running out. True when that showed something.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.delay_until.is_some_and(|d| now >= d) {
            self.delay_until = None;
            if self.target.is_some() {
                self.commit();
                return true;
            }
        }
        false
    }

    pub fn wake(&self) -> Option<Instant> {
        self.delay_until
    }
}

/// tooltip.js `placement`.
pub fn placement(anchor: IRect, size: (f64, f64), screen: (f64, f64), gap: f64, edge: f64, bar: Option<Edge>) -> (f64, f64) {
    #[derive(Clone, Copy, PartialEq)]
    enum Side {
        Below,
        Above,
        Right,
        Left,
    }
    let preferred = match bar {
        Some(Edge::Bottom) => Side::Above,
        Some(Edge::Left) => Side::Right,
        Some(Edge::Right) => Side::Left,
        _ => Side::Below,
    };
    let (ax, ay, aw, ah) = (anchor.x as f64, anchor.y as f64, anchor.w as f64, anchor.h as f64);
    let fits = |side: Side| match side {
        Side::Below => ay + ah + gap + size.1 + edge <= screen.1,
        Side::Above => ay - gap - size.1 >= edge,
        Side::Right => ax + aw + gap + size.0 + edge <= screen.0,
        Side::Left => ax - gap - size.0 >= edge,
    };
    let opposite = |side: Side| match side {
        Side::Below => Side::Above,
        Side::Above => Side::Below,
        Side::Right => Side::Left,
        Side::Left => Side::Right,
    };
    let side = if !fits(preferred) && fits(opposite(preferred)) { opposite(preferred) } else { preferred };
    let lowest = edge.max(screen.1 - size.1 - edge);
    let rightmost = edge.max(screen.0 - size.0 - edge);
    let (x, y) = match side {
        Side::Below => (ax + aw / 2.0 - size.0 / 2.0, ay + ah + gap),
        Side::Above => (ax + aw / 2.0 - size.0 / 2.0, ay - gap - size.1),
        Side::Right => (ax + aw + gap, ay + ah / 2.0 - size.1 / 2.0),
        Side::Left => (ax - gap - size.0, ay + ah / 2.0 - size.1 / 2.0),
    };
    (x.min(rightmost).max(edge), y.min(lowest).max(edge))
}

/// The card on screen: one full-output surface taking no input.
pub struct Card {
    pub surface: Surface,
    pub scene: Scene,
    nodes: Vec<NodeId>,
    open: bool,
    pose: Animated,
    zoom: Animated,
    x: Animated,
    y: Animated,
    w: Animated,
    /// The two text slots and which one is in front.
    texts: [String; 2],
    front: usize,
    cross: Animated,
    placed: bool,
    scale: f64,
}

impl Card {
    pub fn new(surface: Surface, size: (i32, i32), scale: f64) -> Self {
        let f = Clock::Effects.curve();
        let s = Clock::SpatialFast.curve();
        Self {
            surface,
            scene: Scene::clear(size.0, size.1),
            nodes: Vec::new(),
            open: true,
            pose: Animated::new(0.0, f),
            zoom: Animated::new(0.0, s),
            x: Animated::new(0.0, s),
            y: Animated::new(0.0, s),
            w: Animated::new(0.0, s),
            texts: [String::new(), String::new()],
            front: 0,
            cross: Animated::new(1.0, f),
            placed: false,
            scale,
        }
    }

    pub fn mapped(&mut self, theme: &Theme, now: Instant) {
        self.set_open(theme, self.open, now);
    }

    pub fn set_open(&mut self, theme: &Theme, open: bool, now: Instant) {
        self.open = open;
        let target = if open && self.surface.mapped { 1.0 } else { 0.0 };
        self.pose.set(now, target, Clock::Effects.ms(theme) * self.scale);
        self.zoom.set(now, target, Clock::SpatialFast.ms(theme) * self.scale);
    }

    pub fn finished(&self, now: Instant) -> bool {
        !self.open && !self.pose.running(now) && !self.zoom.running(now)
    }

    pub fn animating(&self, now: Instant) -> bool {
        [&self.pose, &self.zoom, &self.x, &self.y, &self.w, &self.cross].iter().any(|a| a.running(now)) || !self.surface.mapped
    }

    fn style(theme: &Theme, kit: &Kit) -> TextStyle {
        TextStyle { family: kit.look.sans, size: theme.font_size.caption as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }

    fn shape(theme: &Theme, kit: &mut Kit, text: &str) -> ShapedText {
        let style = Self::style(theme, kit);
        let max = MAX_TEXT * theme.font_scale;
        let whole = kit.shape(text, style);
        if whole.width as f64 <= max {
            return whole;
        }
        let chars: Vec<char> = text.chars().collect();
        let mut n = chars.len();
        while n > 0 {
            let t: String = chars[..n].iter().collect::<String>() + "\u{2026}";
            let s = kit.shape(&t, style);
            if s.width as f64 <= max {
                return s;
            }
            n -= 1;
        }
        whole
    }

    /// Takes the group's committed ask: a new line crossfades in, a new
    /// item is travelled to when the card is already up.
    pub fn take(&mut self, theme: &Theme, kit: &mut Kit, ask: &Ask, travel: bool, now: Instant) {
        let effects = Clock::Effects.ms(theme) * self.scale;
        let fast = Clock::SpatialFast.ms(theme) * self.scale;
        if self.texts[self.front] != ask.text {
            if travel && self.placed {
                self.front = 1 - self.front;
                self.texts[self.front] = ask.text.clone();
                self.cross.jump(0.0);
                self.cross.set(now, 1.0, effects);
            } else {
                self.texts[self.front] = ask.text.clone();
            }
        }
        let s = &theme.space;
        let text = Self::shape(theme, kit, &ask.text);
        let w = text.width as f64 + s.control_padding_x * 2.0;
        let h = text.line_height() as f64 + s.control_padding_y * 2.0;
        let screen = (self.scene.size.w as f64, self.scene.size.h as f64);
        let (x, y) = placement(ask.rect, (w, h), screen, s.md, s.screen_padding, ask.bar);
        for (a, v) in [(&mut self.x, x), (&mut self.y, y), (&mut self.w, w)] {
            if travel && self.placed { a.set(now, v, fast) } else { a.jump(v) }
        }
        self.placed = true;
    }

    pub fn draw(&mut self, theme: &Theme, kit: &mut Kit, now: Instant) {
        let b = theme.box_style("popover", None);
        let s = &theme.space;
        let pose = self.pose.value(now).clamp(0.0, 1.0) as f32;
        let zoom = 0.97 + 0.03 * self.zoom.value(now);
        let ink = theme.colors.get("popoverForeground");
        let front = Self::shape(theme, kit, &self.texts[self.front].clone());
        let back = Self::shape(theme, kit, &self.texts[1 - self.front].clone());
        let cross = self.cross.value(now).clamp(0.0, 1.0) as f32;
        let w = self.w.value(now);
        let h = front.line_height() as f64 + s.control_padding_y * 2.0;
        let (cx, cy) = (self.x.value(now) + w / 2.0, self.y.value(now) + h / 2.0);
        let (zw, zh) = (w * zoom, h * zoom);
        let rect = IRect::new((cx - zw / 2.0).round() as i32, (cy - zh / 2.0).round() as i32, zw.round() as i32, zh.round() as i32);
        let radius = theme.box_radius(&b, rect.h as f64);
        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        crate::ui::boxes::paint(&mut p, rect, &b, radius, pose, 0.0);
        for (t, a) in [(&back, 1.0 - cross), (&front, cross)] {
            if a <= 0.0 || t.glyphs.is_empty() {
                continue;
            }
            let x = rect.x + (rect.w - t.width) / 2;
            let y = rect.y + (rect.h - t.line_height()) / 2;
            let clip = Some(rect);
            let (bw, bh) = t.box_size();
            let bounds = IRect::new(x - crate::text::PAD, y - crate::text::PAD, bw, bh);
            p.text_in(t, bounds, vello_cpu::kurbo::Affine::IDENTITY, clip, ink.with_alpha(ink.a * pose * a), &[]);
        }
        p.finish();
    }
}
