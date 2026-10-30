//! A card's reactive rim (`docs/superpowers/specs/2026-10-30-reactive-rims.md`):
//! the stops `fs_chrome::rim` reads off the field around a card, turned into
//! the border's and the specular line's colours, and eased between on
//! `spatialFast` when the field moves. Aimed when the card's rest rect or
//! the field changes, never per frame.

use std::rc::Rc;
use std::time::Instant;

use fs_chrome::rim::{self, Field, Light, STOPS};
use fs_theme::color::Rgba;
use vello_cpu::kurbo::{BezPath, Rect, Shape};

use crate::motion::{Animated, SPATIAL_FAST, SPATIAL_FAST_MS};
use crate::scene::{Bitmap, Paint, RimLine, RimMask};

/// How far the border goes toward the colour around it.
const TINT: f32 = 0.2;
/// How far toward white, and how much of its missing alpha, at full lift.
const BRIGHTEN: f32 = 0.28;
const OPACIFY: f32 = 0.15;
/// The specular line's alpha at full lift, and how far it leans to the tint.
const SPECULAR: f32 = 0.16;
const SPECULAR_TINT: f32 = 0.3;
/// The light spilling onto the fill: its alpha at the lit edge at full
/// lift, about a fifth of what the border moves, and the sky's own tint
/// over the whole fill.
const SPILL: f32 = 0.12;
const SPILL_TINT: f32 = 0.03;
/// How far the spill falls inward from the edge, and the logical pixels to
/// one mask pixel.
const SPILL_DEPTH: f64 = 36.0;
const MASK_CELL: f64 = 4.0;

/// One output's field and the count of times it changed.
#[derive(Clone)]
pub struct Lit {
    pub generation: u64,
    pub field: Rc<Field>,
}

#[derive(Clone, Copy, PartialEq)]
struct Key {
    generation: u64,
    rect: [i32; 4],
    base: [u8; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stops {
    pub border: [Rgba; STOPS],
    pub specular: [Rgba; STOPS],
    pub spill: [Rgba; STOPS],
    /// The sky's mean tint, flat over the whole fill: a sweep carried to
    /// the centre would draw its seams there.
    pub wash: Rgba,
}

impl Stops {
    fn of(base: Rgba, lights: &[Light; STOPS]) -> Self {
        let white = Rgba { r: 1.0, g: 1.0, b: 1.0, a: base.a };
        let mut border = [base; STOPS];
        let mut specular = [Rgba::TRANSPARENT; STOPS];
        let mut spill = [Rgba::TRANSPARENT; STOPS];
        let mut sum = [0.0f32; 4];
        for (i, l) in lights.iter().enumerate() {
            let tint = Rgba { r: l.tint[0], g: l.tint[1], b: l.tint[2], a: base.a };
            let c = base.mix(tint, TINT * l.tinted).mix(white, BRIGHTEN * l.lift);
            border[i] = c.with_alpha(base.a + (1.0 - base.a) * OPACIFY * l.lift);
            let s = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }.mix(tint.with_alpha(1.0), SPECULAR_TINT * l.tinted);
            specular[i] = s.with_alpha(SPECULAR * l.lift);
            let glow = tint.with_alpha(1.0).mix(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, if l.tinted > 0.0 { l.lift } else { 1.0 });
            spill[i] = glow.with_alpha(SPILL * l.lift);
            for (acc, v) in sum.iter_mut().zip([l.tint[0], l.tint[1], l.tint[2], l.tinted]) {
                *acc += v / STOPS as f32;
            }
        }
        let wash = Rgba { r: sum[0], g: sum[1], b: sum[2], a: SPILL_TINT * sum[3] };
        Self { border, specular, spill, wash }
    }

    fn mix(&self, to: &Stops, t: f32) -> Stops {
        let mut out = *to;
        for i in 0..STOPS {
            out.border[i] = self.border[i].mix(to.border[i], t);
            out.specular[i] = self.specular[i].mix(to.specular[i], t);
            out.spill[i] = self.spill[i].mix(to.spill[i], t);
        }
        out.wash = self.wash.mix(to.wash, t);
        out
    }
}

pub struct Rim {
    from: Option<Stops>,
    to: Option<Stops>,
    blend: Animated,
    key: Option<Key>,
    /// The stops changed with no blend to carry them: one more paint.
    fresh: bool,
    pub lights: Option<[Light; STOPS]>,
    /// The spill's falloff for the last fill size, in mask pixels.
    mask: Option<((u16, u16), Bitmap)>,
    /// One white pixel, the wash's mask.
    flat: Option<Bitmap>,
}

impl Default for Rim {
    fn default() -> Self {
        Self { from: None, to: None, blend: Animated::new(1.0, SPATIAL_FAST), key: None, fresh: false, lights: None, mask: None, flat: None }
    }
}

/// The spill's falloff over a fill `size` logical pixels big: white, its
/// alpha a quadratic fall from each edge over `SPILL_DEPTH`.
fn spill_mask(size: (f64, f64), cells: (u16, u16)) -> Bitmap {
    let (w, h) = size;
    let (sx, sy) = (w / f64::from(cells.0), h / f64::from(cells.1));
    let mut px = Vec::with_capacity(usize::from(cells.0) * usize::from(cells.1) * 4);
    for j in 0..cells.1 {
        let y = (f64::from(j) + 0.5) * sy;
        for i in 0..cells.0 {
            let x = (f64::from(i) + 0.5) * sx;
            let d = x.min(w - x).min(y).min(h - y);
            let f = (1.0 - d / SPILL_DEPTH).max(0.0) as f32;
            let a = (f * f * 255.0).round() as u8;
            px.extend_from_slice(&[a; 4]);
        }
    }
    Bitmap::from_premultiplied(cells.0, cells.1, px)
}

impl Rim {
    /// Aims at `lit` for a card at `card` in output pixels over a border
    /// `base`; `None` puts the flat border back. `ms` is the blend's length.
    pub fn aim(&mut self, now: Instant, lit: Option<&Lit>, card: Rect, base: Rgba, ms: f64) {
        let Some(lit) = lit.filter(|_| card.width() > 0.0 && card.height() > 0.0) else {
            if self.to.is_some() {
                (self.from, self.to, self.key, self.lights) = (None, None, None, None);
                self.blend.jump(1.0);
                self.fresh = true;
            }
            return;
        };
        let key = Key {
            generation: lit.generation,
            rect: [card.x0.round() as i32, card.y0.round() as i32, card.x1.round() as i32, card.y1.round() as i32],
            base: base.to_u8(),
        };
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let lights = rim::lights(&lit.field, (card.x0, card.y0, card.x1, card.y1));
        let stops = Stops::of(base, &lights);
        self.lights = Some(lights);
        if self.to == Some(stops) {
            return;
        }
        match self.current(now) {
            // The first aim lands at once: the card is still arriving.
            None => {
                (self.from, self.to) = (Some(stops), Some(stops));
                self.blend.jump(1.0);
                self.fresh = true;
            }
            Some(at) => {
                (self.from, self.to) = (Some(at), Some(stops));
                self.blend.jump(0.0);
                self.blend.set(now, 1.0, ms);
                if ms <= 0.0 {
                    self.fresh = true;
                }
            }
        }
    }

    /// The stops this frame, `None` for the flat border.
    pub fn current(&self, now: Instant) -> Option<Stops> {
        let to = self.to.as_ref()?;
        let t = self.blend.value(now).clamp(0.0, 1.0) as f32;
        Some(match &self.from {
            Some(from) if t < 1.0 => from.mix(to, t),
            _ => *to,
        })
    }

    /// The frame carrying the latest stops was laid.
    pub fn painted(&mut self) {
        self.fresh = false;
    }

    pub fn running(&self, now: Instant) -> bool {
        self.fresh || (self.to.is_some() && self.blend.running(now))
    }

    /// The rim's lines for a shape whose border is `outer` (stroked
    /// `width` wide) and whose fill is `fill`, both in the node's
    /// coordinates; `centre` is the card's centre in surface pixels and
    /// `alpha` the frame's opacity. A stop whose normal is `mute` (the side
    /// a joined card hangs off its line by) draws no specular line.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, now: Instant, outer: BezPath, width: f64, fill: BezPath, card: Rect, alpha: f32, mute: Option<(f64, f64)>) -> Option<Paint> {
        let stops = self.current(now)?;
        let sweep = |colours: &[Rgba; STOPS], muted: bool| -> Vec<(f32, Rgba)> {
            let mut v: Vec<(f32, Rgba)> = (0..=STOPS)
                .map(|i| {
                    let k = i % STOPS;
                    let mut c = colours[k];
                    if muted && mute.is_some_and(|m| stop_normal(card, k) == m) {
                        c = c.with_alpha(0.0);
                    }
                    (i as f32 / STOPS as f32, c.with_alpha(c.a * alpha))
                })
                .collect();
            if let Some(last) = v.last_mut() {
                last.0 = 1.0;
            }
            v
        };
        let centre = ((card.x0 + card.x1) / 2.0, (card.y0 + card.y1) / 2.0);
        let mut lines = Vec::with_capacity(3);
        let bounds = fill.bounding_box();
        if stops.wash.a > 0.0 {
            let flat = self.flat.get_or_insert_with(|| Bitmap::from_premultiplied(1, 1, vec![255; 4])).clone();
            let wash = stops.wash.with_alpha(stops.wash.a * alpha);
            lines.push(RimLine { path: fill.clone(), width: 0.0, clip: None, centre, stops: vec![(0.0, wash), (1.0, wash)], mask: Some(RimMask { image: flat, rect: bounds }) });
        }
        if stops.spill.iter().any(|c| c.a > 0.0) && bounds.width() > 0.0 && bounds.height() > 0.0 {
            // Made again only when the fill grows or shrinks by a mask pixel.
            let cells = ((bounds.width() / MASK_CELL).ceil().clamp(1.0, 4096.0) as u16, (bounds.height() / MASK_CELL).ceil().clamp(1.0, 4096.0) as u16);
            if self.mask.as_ref().is_none_or(|(c, _)| *c != cells) {
                self.mask = Some((cells, spill_mask((bounds.width(), bounds.height()), cells)));
            }
            let image = self.mask.as_ref().map(|(_, m)| m.clone()).expect("made above");
            lines.push(RimLine { path: fill.clone(), width: 0.0, clip: None, centre, stops: sweep(&stops.spill, false), mask: Some(RimMask { image, rect: bounds }) });
        }
        if stops.specular.iter().any(|c| c.a > 0.0) {
            // One border deep on the fill's own edge, cut to the fill, under
            // the border: the pixel inside the border is what shows.
            lines.push(RimLine { path: fill.clone(), width: width * 2.0, clip: Some(fill), centre, stops: sweep(&stops.specular, true), mask: None });
        }
        lines.push(RimLine { path: outer, width, clip: None, centre, stops: sweep(&stops.border, false), mask: None });
        Some(Paint::Rim(lines))
    }

    /// For the dump: the mean lift on each side, top, right, bottom, left.
    pub fn report(&self, card: Rect) -> Option<serde_json::Value> {
        let lights = self.lights.as_ref()?;
        let c = (card.x0, card.y0, card.x1, card.y1);
        let side = |d| f64::from(rim::side_lift(lights, c, d));
        let stops = self.to.as_ref()?;
        let hex = |c: Rgba| {
            let [r, g, b, a] = c.to_u8();
            format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
        };
        Some(serde_json::json!({
            "rect": [card.x0, card.y0, card.x1, card.y1],
            "lift": { "top": side((0.0, -1.0)), "right": side((1.0, 0.0)), "bottom": side((0.0, 1.0)), "left": side((-1.0, 0.0)) },
            "border": { "right": hex(stops.border[0]), "bottom": hex(stops.border[STOPS / 4]), "left": hex(stops.border[STOPS / 2]), "top": hex(stops.border[STOPS * 3 / 4]) },
        }))
    }
}

/// Stop `i`'s outward normal on `card`.
fn stop_normal(card: Rect, i: usize) -> (f64, f64) {
    rim::edge_point((card.x0, card.y0, card.x1, card.y1), i as f64 * std::f64::consts::TAU / STOPS as f64).1
}

/// The blend's length off the theme.
pub fn blend_ms(theme: &fs_theme::theme::Theme, scale: f64) -> f64 {
    if theme.motion_enabled { SPATIAL_FAST_MS * scale } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn lit(rgb: [f32; 3], generation: u64) -> Lit {
        Lit { generation, field: Rc::new(Field { sky: Some(Arc::new(rim::Sky::flat(rgb, (400.0, 300.0)))), glints: Vec::new() }) }
    }

    const BASE: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.75 };

    #[test]
    fn a_red_sky_and_a_blue_one_give_different_borders() {
        let card = Rect::new(100.0, 100.0, 300.0, 200.0);
        let now = Instant::now();
        let (mut red, mut blue) = (Rim::default(), Rim::default());
        red.aim(now, Some(&lit([1.0, 0.0, 0.0], 1)), card, BASE, 0.0);
        blue.aim(now, Some(&lit([0.0, 0.0, 1.0], 1)), card, BASE, 0.0);
        let (r, b) = (red.current(now).unwrap().border[0], blue.current(now).unwrap().border[0]);
        assert!(r.r > b.r && b.b > r.b, "{r:?} {b:?}");
    }

    #[test]
    fn the_same_aim_twice_starts_nothing() {
        let card = Rect::new(100.0, 100.0, 300.0, 200.0);
        let now = Instant::now();
        let mut rim = Rim::default();
        rim.aim(now, Some(&lit([1.0, 0.0, 0.0], 1)), card, BASE, 350.0);
        rim.painted();
        assert!(!rim.running(now));
        rim.aim(now, Some(&lit([1.0, 0.0, 0.0], 1)), card, BASE, 350.0);
        assert!(!rim.running(now));
        // A new generation that changes nothing either.
        rim.aim(now, Some(&lit([1.0, 0.0, 0.0], 2)), card, BASE, 350.0);
        assert!(!rim.running(now));
        rim.aim(now, Some(&lit([0.0, 0.0, 1.0], 3)), card, BASE, 350.0);
        assert!(rim.running(now));
    }
}
