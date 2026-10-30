//! A card's reactive rim (`docs/superpowers/specs/2026-10-30-reactive-rims.md`):
//! what the wallpaper and the windows around a card do to its border, as
//! [`STOPS`] stops at even angles round its centre. Colours are sRGB 0..1;
//! every rect is `(x0, y0, x1, y1)` in output logical pixels.

use std::sync::Arc;

/// Stops round the rim, the first at the centre's +x and clockwise on a
/// y-down output.
pub const STOPS: usize = 24;
/// Logical pixels to one sky cell.
pub const CELL: f64 = 8.0;
/// How far a window lights a rim facing it.
pub const REACH: f64 = 64.0;
/// How far outside and inside the edge the sky is read.
const SKY_OUT: f64 = 6.0;
const SKY_IN: f64 = 6.0;
/// The most the sky's own brightness lifts a stop.
const SKY_LIFT: f32 = 0.25;
/// A window that is not focused, against the focused one.
pub const UNFOCUSED: f32 = 0.45;

/// A downsampled copy of one output's wallpaper.
#[derive(Clone, Debug, PartialEq)]
pub struct Sky {
    pub w: usize,
    pub h: usize,
    /// Logical pixels per cell, along x and y.
    pub cell: (f64, f64),
    pub rgb: Vec<[f32; 3]>,
}

impl Sky {
    /// Box-averaged off BGRA rows (`pw` by `ph`, the picture as drawn) for
    /// an output `logical` pixels big.
    pub fn from_bgra(bgra: &[u8], pw: usize, ph: usize, logical: (f64, f64)) -> Option<Sky> {
        if pw == 0 || ph == 0 || bgra.len() < pw * ph * 4 || !(logical.0 > 0.0 && logical.1 > 0.0) {
            return None;
        }
        let w = ((logical.0 / CELL).ceil() as usize).clamp(1, pw);
        let h = ((logical.1 / CELL).ceil() as usize).clamp(1, ph);
        let mut sum = vec![[0u64; 4]; w * h];
        for y in 0..ph {
            let cy = y * h / ph;
            let row = &bgra[y * pw * 4..(y + 1) * pw * 4];
            for (x, px) in row.as_chunks::<4>().0.iter().enumerate() {
                let s = &mut sum[cy * w + x * w / pw];
                s[0] += u64::from(px[2]);
                s[1] += u64::from(px[1]);
                s[2] += u64::from(px[0]);
                s[3] += 1;
            }
        }
        let rgb = sum
            .iter()
            .map(|s| {
                let n = s[3].max(1) as f32 * 255.0;
                [s[0] as f32 / n, s[1] as f32 / n, s[2] as f32 / n]
            })
            .collect();
        Some(Sky { w, h, cell: (logical.0 / w as f64, logical.1 / h as f64), rgb })
    }

    /// One colour everywhere (a flat wallpaper).
    pub fn flat(rgb: [f32; 3], logical: (f64, f64)) -> Sky {
        Sky { w: 1, h: 1, cell: (logical.0.max(1.0), logical.1.max(1.0)), rgb: vec![rgb] }
    }

    /// The cell under a logical point, clamped to the output.
    pub fn at(&self, x: f64, y: f64) -> [f32; 3] {
        let cx = ((x / self.cell.0).floor().max(0.0) as usize).min(self.w - 1);
        let cy = ((y / self.cell.1).floor().max(0.0) as usize).min(self.h - 1);
        self.rgb[cy * self.w + cx]
    }
}

/// A window as a light: its rect and the colour it glows in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glint {
    pub rect: (f64, f64, f64, f64),
    pub rgb: [f32; 3],
    pub strength: f32,
}

/// Everything around the cards on one output.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    pub sky: Option<Arc<Sky>>,
    pub glints: Vec<Glint>,
}

/// One stop: the colour around it and how much light reaches it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    pub tint: [f32; 3],
    /// 0 with no sky under it.
    pub tinted: f32,
    pub lift: f32,
}

pub fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn q(v: f32, steps: f32) -> f32 {
    (v * steps).round() / steps
}

/// The point on `card`'s edge along the ray from its centre at `angle`
/// (radians, clockwise on a y-down output from +x), and the outward normal
/// there, a corner's being its diagonal.
pub fn edge_point(card: (f64, f64, f64, f64), angle: f64) -> ((f64, f64), (f64, f64)) {
    let (x0, y0, x1, y1) = card;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hw, hh) = (((x1 - x0) / 2.0).max(0.5), ((y1 - y0) / 2.0).max(0.5));
    let (dx, dy) = (angle.cos(), angle.sin());
    let tx = if dx.abs() > 1e-9 { hw / dx.abs() } else { f64::INFINITY };
    let ty = if dy.abs() > 1e-9 { hh / dy.abs() } else { f64::INFINITY };
    let t = tx.min(ty);
    let p = (cx + dx * t, cy + dy * t);
    // Within a radius-ish of a corner the normal turns with it.
    let corner = (tx - ty).abs() * (dx.abs().max(dy.abs())) < 8.0;
    let n = if corner {
        let (nx, ny) = (dx.signum(), dy.signum());
        let l = (nx * nx + ny * ny).sqrt();
        (nx / l, ny / l)
    } else if tx < ty {
        (dx.signum(), 0.0)
    } else {
        (0.0, dy.signum())
    };
    (p, n)
}

/// The nearest point of `r` to `p`.
fn nearest(r: (f64, f64, f64, f64), p: (f64, f64)) -> (f64, f64) {
    (p.0.clamp(r.0, r.2), p.1.clamp(r.1, r.3))
}

fn overlaps(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// The stops for a card at `card`.
pub fn lights(field: &Field, card: (f64, f64, f64, f64)) -> [Light; STOPS] {
    let mut out = [Light { tint: [0.0; 3], tinted: 0.0, lift: 0.0 }; STOPS];
    for (i, light) in out.iter_mut().enumerate() {
        let angle = i as f64 * std::f64::consts::TAU / STOPS as f64;
        let (p, n) = edge_point(card, angle);
        let mut lift = 0.0f32;
        if let Some(sky) = &field.sky {
            let a = sky.at(p.0 + n.0 * SKY_OUT, p.1 + n.1 * SKY_OUT);
            let b = sky.at(p.0 - n.0 * SKY_IN, p.1 - n.1 * SKY_IN);
            let tint = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, (a[2] + b[2]) / 2.0];
            light.tint = tint;
            light.tinted = 1.0;
            lift += luminance(tint) * SKY_LIFT;
        }
        let mut glow = [0.0f32; 3];
        let mut glow_w = 0.0f32;
        for g in &field.glints {
            // A window under the card lights it from behind, not its rim.
            if overlaps(g.rect, card) {
                continue;
            }
            let m = nearest(g.rect, p);
            let (vx, vy) = (m.0 - p.0, m.1 - p.1);
            let d = (vx * vx + vy * vy).sqrt();
            if d >= REACH {
                continue;
            }
            let facing = if d < 1e-6 { 1.0 } else { ((vx * n.0 + vy * n.1) / d).max(0.0) };
            let fall = 1.0 - d / REACH;
            let w = (facing * fall * fall) as f32 * g.strength;
            if w <= 0.0 {
                continue;
            }
            lift += w;
            for (acc, c) in glow.iter_mut().zip(g.rgb) {
                *acc += c * w;
            }
            glow_w += w;
        }
        if glow_w > 0.0 {
            let share = (glow_w / (glow_w + 1.0)).min(1.0);
            let g = [glow[0] / glow_w, glow[1] / glow_w, glow[2] / glow_w];
            let tinted = light.tinted > 0.0;
            for (t, c) in light.tint.iter_mut().zip(g) {
                *t = if tinted { *t + (c - *t) * share } else { c };
            }
            light.tinted = light.tinted.max(share);
        }
        light.lift = q(lift.min(1.0), 64.0);
        for c in &mut light.tint {
            *c = q(c.clamp(0.0, 1.0), 255.0);
        }
        light.tinted = q(light.tinted, 64.0);
    }
    out
}

/// The mean lift over the stops whose normal points along `dir` (one of
/// the four sides), for a report.
pub fn side_lift(stops: &[Light; STOPS], card: (f64, f64, f64, f64), dir: (f64, f64)) -> f32 {
    let mut sum = 0.0;
    let mut n = 0;
    for (i, s) in stops.iter().enumerate() {
        let (_, nrm) = edge_point(card, i as f64 * std::f64::consts::TAU / STOPS as f64);
        if nrm == dir {
            sum += s.lift;
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { sum / n as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CARD: (f64, f64, f64, f64) = (100.0, 100.0, 300.0, 200.0);

    fn halves() -> Sky {
        // Red on the left half of a 400 wide output, blue on the right.
        let (pw, ph) = (400usize, 300usize);
        let mut px = Vec::with_capacity(pw * ph * 4);
        for _ in 0..ph {
            for x in 0..pw {
                px.extend_from_slice(if x < 200 { &[0, 0, 255, 255] } else { &[255, 0, 0, 255] });
            }
        }
        Sky::from_bgra(&px, pw, ph, (400.0, 300.0)).unwrap()
    }

    #[test]
    fn the_sky_tints_each_side_by_what_is_under_it() {
        let field = Field { sky: Some(Arc::new(halves())), glints: Vec::new() };
        let s = lights(&field, CARD);
        // Stop 0 faces +x (the blue half), stop 12 faces -x (the red one).
        assert_eq!(s[0].tint, [0.0, 0.0, 1.0]);
        assert_eq!(s[12].tint, [1.0, 0.0, 0.0]);
        assert!(s[0].lift > 0.0 && s[0].lift <= SKY_LIFT);
    }

    #[test]
    fn a_window_lights_only_the_side_facing_it() {
        let window = Glint { rect: (320.0, 120.0, 600.0, 180.0), rgb: [1.0, 1.0, 1.0], strength: 1.0 };
        let field = Field { sky: None, glints: vec![window] };
        let s = lights(&field, CARD);
        let right = side_lift(&s, CARD, (1.0, 0.0));
        let left = side_lift(&s, CARD, (-1.0, 0.0));
        assert!(right > 0.2, "{right}");
        assert_eq!(left, 0.0);
        // Out of reach, nothing.
        let far = Field { sky: None, glints: vec![Glint { rect: (380.0, 120.0, 600.0, 180.0), ..window }] };
        assert_eq!(side_lift(&lights(&far, CARD), CARD, (1.0, 0.0)), 0.0);
    }

    #[test]
    fn a_window_behind_the_card_is_not_a_light() {
        let under = Glint { rect: (0.0, 0.0, 400.0, 300.0), rgb: [1.0, 1.0, 1.0], strength: 1.0 };
        let s = lights(&Field { sky: None, glints: vec![under] }, CARD);
        assert!(s.iter().all(|l| l.lift == 0.0));
    }

    #[test]
    fn the_same_field_gives_the_same_stops() {
        let field = Field { sky: Some(Arc::new(halves())), glints: vec![Glint { rect: (310.0, 0.0, 400.0, 300.0), rgb: [0.2, 0.6, 1.0], strength: 1.0 }] };
        assert_eq!(lights(&field, CARD), lights(&field, CARD));
    }

    #[test]
    fn edge_points_lie_on_the_card() {
        for i in 0..STOPS {
            let (p, n) = edge_point(CARD, i as f64 * std::f64::consts::TAU / STOPS as f64);
            let on_x = (p.0 - CARD.0).abs() < 1e-6 || (p.0 - CARD.2).abs() < 1e-6;
            let on_y = (p.1 - CARD.1).abs() < 1e-6 || (p.1 - CARD.3).abs() < 1e-6;
            assert!(on_x || on_y, "{i} {p:?}");
            assert!(((n.0 * n.0 + n.1 * n.1).sqrt() - 1.0).abs() < 1e-9);
        }
    }
}
