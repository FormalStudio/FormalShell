//! The two clock families as `Anim` runs them (tokens.js `MOTION_BASE`,
//! `MOTION_CURVES`), a `Behavior`'s retarget, and the velocity deform
//! with its spring.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use fs_theme::tokens::Easing;

/// Qt's `BezierSpline` as `easing.bezierCurve` takes it: per segment the
/// two control points and the end point, starting from (0,0) and ending on
/// (1,1). Or CSS `linear()`: `(t, v)` stops joined by straight lines.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Bezier { points: [f64; 12], len: usize },
    Stops(&'static [(f64, f64)]),
}

impl Curve {
    pub const fn new(points: &[f64]) -> Self {
        let mut out = [0.0; 12];
        let mut i = 0;
        while i < points.len() && i < 12 {
            out[i] = points[i];
            i += 1;
        }
        Self::Bezier { points: out, len: i }
    }

    /// A table's own curve (`motion.emerge` and friends), spatial if it
    /// carries no whole segment.
    pub fn from_table(easing: &Easing) -> Self {
        match easing {
            Easing::Bezier(points) if points.len() >= 6 => Self::new(points),
            Easing::Bezier(_) => SPATIAL,
            Easing::Stops(stops) => Self::Stops(interned(stops)),
        }
    }
}

/// A stop list for as long as the shell runs. They come out of the
/// compiled-in tables, so each distinct one is leaked once and shared by
/// every `Look` built from it after.
fn interned(stops: &[(f64, f64)]) -> &'static [(f64, f64)] {
    static SEEN: Mutex<Vec<&'static [(f64, f64)]>> = Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(hit) = seen.iter().find(|s| **s == stops) {
        return hit;
    }
    let leaked: &'static [(f64, f64)] = Box::leak(stops.into());
    seen.push(leaked);
    leaked
}

pub const SPATIAL_FAST: Curve = Curve::new(&[0.42, 1.67, 0.21, 0.9, 1.0, 1.0]);
pub const SPATIAL: Curve = Curve::new(&[0.38, 1.21, 0.22, 1.0, 1.0, 1.0]);
pub const EFFECTS: Curve = Curve::new(&[0.34, 0.8, 0.34, 1.0, 1.0, 1.0]);
/// M3's two-segment curve, the workspace pill's alone.
pub const EMPHASIZED: Curve =
    Curve::new(&[0.05, 0.0, 2.0 / 15.0, 0.06, 1.0 / 6.0, 0.4, 5.0 / 24.0, 0.82, 0.25, 1.0, 1.0, 1.0]);

pub const SPATIAL_SLOW: Curve = Curve::new(&[0.39, 1.29, 0.35, 0.98, 1.0, 1.0]);
pub const EFFECTS_FAST: Curve = Curve::new(&[0.31, 0.94, 0.34, 1.0, 1.0, 1.0]);
pub const EFFECTS_SLOW: Curve = Curve::new(&[0.34, 0.88, 0.34, 1.0, 1.0, 1.0]);

/// `Anim`'s `kind`: which family's clock and curve a property rides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SpatialFast,
    Spatial,
    SpatialSlow,
    EffectsFast,
    Effects,
    EffectsSlow,
}

impl Kind {
    pub fn curve(self) -> Curve {
        match self {
            Kind::SpatialFast => SPATIAL_FAST,
            Kind::Spatial => SPATIAL,
            Kind::SpatialSlow => SPATIAL_SLOW,
            Kind::EffectsFast => EFFECTS_FAST,
            Kind::Effects => EFFECTS,
            Kind::EffectsSlow => EFFECTS_SLOW,
        }
    }

    /// The live table's duration for this kind, 0 with motion off.
    pub fn ms(self, theme: &fs_theme::theme::Theme) -> f64 {
        if !theme.motion_enabled {
            return 0.0;
        }
        let m = theme.motion().families;
        match self {
            Kind::SpatialFast => m.spatial_fast,
            Kind::Spatial => m.spatial,
            Kind::SpatialSlow => m.spatial_slow,
            Kind::EffectsFast => m.effects_fast,
            Kind::Effects => m.effects,
            Kind::EffectsSlow => m.effects_slow,
        }
    }
}

pub const SPATIAL_FAST_MS: f64 = 350.0;
pub const SPATIAL_MS: f64 = 500.0;
pub const PULSE_MS: f64 = 900.0;

/// The breathing pulse (`pulseDuration`, InOutQuad): 1 to 0.4 and back, a
/// pulse each way.
pub fn breathe(since: Instant, now: Instant) -> f32 {
    let t = now.saturating_duration_since(since).as_secs_f64() * 1000.0 / PULSE_MS;
    let leg = t % 2.0;
    let x = if leg < 1.0 { leg } else { 2.0 - leg };
    let eased = if x < 0.5 { 2.0 * x * x } else { 1.0 - (-2.0 * x + 2.0).powi(2) / 2.0 };
    (1.0 - 0.6 * eased) as f32
}

/// An emerge on the `spatial` family's duration and curve.
pub const EMERGE: (f64, Curve) = (SPATIAL_MS, SPATIAL);

impl Curve {
    /// y at a given x. Each segment's x(t) is monotonic for control x
    /// inside the segment, so a bisection lands t to f64 precision.
    pub fn ease(&self, x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let (points, len) = match self {
            Self::Bezier { points, len } => (points, *len),
            Self::Stops(stops) => return linear(stops, x),
        };
        let bez = |p0: f64, a: f64, b: f64, p3: f64, t: f64| {
            let u = 1.0 - t;
            u * u * u * p0 + 3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t * p3
        };
        let (mut x0, mut y0) = (0.0, 0.0);
        for seg in points[..len].chunks_exact(6) {
            let [x1, y1, x2, y2, x3, y3] = [seg[0], seg[1], seg[2], seg[3], seg[4], seg[5]];
            if x <= x3 {
                let (mut lo, mut hi) = (0.0, 1.0);
                for _ in 0..60 {
                    let mid = (lo + hi) / 2.0;
                    if bez(x0, x1, x2, x3, mid) < x { lo = mid } else { hi = mid }
                }
                return bez(y0, y1, y2, y3, (lo + hi) / 2.0);
            }
            (x0, y0) = (x3, y3);
        }
        1.0
    }
}

/// CSS `linear()` at `x`: the line between the stops either side of it,
/// the later stop where two share a t.
fn linear(stops: &[(f64, f64)], x: f64) -> f64 {
    let Some(i) = stops.iter().position(|s| s.0 > x) else {
        return stops.last().map_or(1.0, |s| s.1);
    };
    let Some(&(t0, v0)) = i.checked_sub(1).and_then(|p| stops.get(p)) else {
        return stops[0].1;
    };
    let (t1, v1) = stops[i];
    v0 + (v1 - v0) * (x - t0) / (t1 - t0)
}

/// A number under `Behavior on x { Anim {} }`: a new target starts a fresh
/// animation from wherever the value is, on the full duration.
pub struct Animated {
    target: f64,
    from: f64,
    start: Instant,
    duration: Duration,
    curve: Curve,
}

impl Animated {
    pub fn new(value: f64, curve: Curve) -> Self {
        Self { target: value, from: value, start: Instant::now(), duration: Duration::ZERO, curve }
    }

    pub fn value(&self, now: Instant) -> f64 {
        if self.duration.is_zero() {
            return self.target;
        }
        let x = now.saturating_duration_since(self.start).as_secs_f64() / self.duration.as_secs_f64();
        self.from + (self.target - self.from) * self.curve.ease(x)
    }

    pub fn running(&self, now: Instant) -> bool {
        !self.duration.is_zero() && now < self.start + self.duration
    }

    pub fn target(&self) -> f64 {
        self.target
    }

    pub fn set(&mut self, now: Instant, target: f64, duration_ms: f64) {
        if target == self.target {
            return;
        }
        self.from = self.value(now);
        self.target = target;
        self.start = now;
        self.duration = Duration::from_secs_f64(duration_ms.max(0.0) / 1000.0);
    }

    /// `set` on another curve (a Behavior whose `Anim` picks its kind off
    /// the direction it is heading).
    pub fn set_on(&mut self, now: Instant, target: f64, duration_ms: f64, curve: Curve) {
        if target == self.target {
            return;
        }
        self.set(now, target, duration_ms);
        self.curve = curve;
    }

    /// `set` behind a `PauseAnimation`: the value holds where it is for
    /// `delay_ms`, then travels on the full duration.
    pub fn set_after(&mut self, now: Instant, target: f64, duration_ms: f64, delay_ms: f64) {
        if target == self.target {
            return;
        }
        self.set(now, target, duration_ms);
        self.start = now + Duration::from_secs_f64(delay_ms.max(0.0) / 1000.0);
        if self.duration.is_zero() && delay_ms > 0.0 {
            self.duration = Duration::from_nanos(1);
        }
    }

    /// The `Behavior` disabled: the value lands at once.
    pub fn jump(&mut self, target: f64) {
        self.target = target;
        self.from = target;
        self.duration = Duration::ZERO;
    }
}

/// tokens.js `DEFORM`.
const MAX_STRETCH: f64 = 0.35;
const DEAD_BAND: f64 = 5.0;
const STIFFNESS: f64 = 200.0;
const DAMPING: f64 = 16.0;
const EPSILON: f64 = 0.002;

/// The deform for a top-edge drawer: the symmetric 2x2 the three springs
/// carry, sampled off the frame's live position and height.
pub struct Deform {
    pub m00: f64,
    pub m01: f64,
    pub m11: f64,
    v00: f64,
    v01: f64,
    v11: f64,
    pub at_rest: bool,
    sampled: Option<(f64, f64, f64, f64)>,
}

/// The side the frame is anchored to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeformEdge {
    Top,
    Left,
    Right,
}

impl Deform {
    pub fn new() -> Self {
        Self { m00: 1.0, m01: 0.0, m11: 1.0, v00: 0.0, v01: 0.0, v11: 0.0, at_rest: true, sampled: None }
    }

    /// The frame loop stopped: the next step has nothing to subtract.
    pub fn unsample(&mut self) {
        self.sampled = None;
    }

    /// One frame of a top-edge drawer: `x`, `y` the frame's scene
    /// position, `h` its height.
    pub fn step(&mut self, dt: f64, x: f64, y: f64, h: f64, amount: f64) {
        self.step_at(dt, (x, y, 0.0, h), amount, DeformEdge::Top);
    }

    /// One frame of a frame at `(x, y, w, h)` anchored to `edge`: a size
    /// change reads as travel away from that edge.
    pub fn step_at(&mut self, dt: f64, (x, y, w, h): (f64, f64, f64, f64), amount: f64, edge: DeformEdge) {
        // A second step on one frame (a card moved after its tick) keeps
        // the frame's sample, so the next frame still reads the move.
        if dt < 0.001 && self.sampled.is_some() {
            return;
        }
        let Some((px, py, pw, ph)) = self.sampled.filter(|_| (0.001..=0.1).contains(&dt)) else {
            self.sampled = Some((x, y, w, h));
            self.settle(0.0);
            return;
        };
        let (dw, dh) = ((w - pw) / dt, (h - ph) / dt);
        let (mut vx, mut vy) = ((x - px) / dt, (y - py) / dt);
        match edge {
            DeformEdge::Top => vy += dh,
            DeformEdge::Left => vx += dw,
            DeformEdge::Right => vx -= dw,
        }
        self.sampled = Some((x, y, w, h));

        let speed = (vx * vx + vy * vy).sqrt();
        if self.at_rest && speed < DEAD_BAND {
            return;
        }
        self.at_rest = false;

        let (mut t00, mut t01, mut t11) = (1.0, 0.0, 1.0);
        if speed > DEAD_BAND {
            let stretch = 1.0 + (speed * amount / 10000.0).min(MAX_STRETCH);
            let compress = 1.0 / stretch;
            let (cos, sin) = (vx / speed, vy / speed);
            t00 = stretch * cos * cos + compress * sin * sin;
            t01 = (stretch - compress) * cos * sin;
            t11 = stretch * sin * sin + compress * cos * cos;
        }

        // Damping integrated implicitly: an explicit term flips sign once
        // DAMPING * dt passes 1.
        let inv = 1.0 / (1.0 + DAMPING * dt);
        self.v00 = (self.v00 - STIFFNESS * (self.m00 - t00) * dt) * inv;
        self.v01 = (self.v01 - STIFFNESS * (self.m01 - t01) * dt) * inv;
        self.v11 = (self.v11 - STIFFNESS * (self.m11 - t11) * dt) * inv;
        self.m00 += self.v00 * dt;
        self.m01 += self.v01 * dt;
        self.m11 += self.v11 * dt;
        self.settle(speed);
    }

    fn settle(&mut self, speed: f64) {
        if self.at_rest {
            return;
        }
        let e = EPSILON;
        let settled = (self.m00 - 1.0).abs() < e
            && self.m01.abs() < e
            && (self.m11 - 1.0).abs() < e
            && self.v00.abs() < e
            && self.v01.abs() < e
            && self.v11.abs() < e
            && speed < DEAD_BAND;
        if settled {
            *self = Self { sampled: self.sampled, ..Self::new() };
        }
    }

    /// The spring's matrix about `pivot`, in the coordinates the pivot is in.
    pub fn about(&self, pivot: (f64, f64)) -> vello_cpu::kurbo::Affine {
        use vello_cpu::kurbo::Affine;
        Affine::translate(pivot) * Affine::new([self.m00, self.m01, self.m01, self.m11, 0.0, 0.0]) * Affine::translate((-pivot.0, -pivot.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spatial_overshoots_and_lands() {
        let peak = (1..100).map(|i| SPATIAL.ease(i as f64 / 100.0)).fold(0.0, f64::max);
        assert!(peak > 1.0 && peak < 1.1, "{peak}");
        assert_eq!(SPATIAL.ease(1.0), 1.0);
        assert_eq!(SPATIAL.ease(0.0), 0.0);
    }

    #[test]
    fn a_stop_list_runs_straight_between_its_stops_and_past_one() {
        let spring = Curve::from_table(&Easing::Stops(vec![(0.0, 0.0), (0.25, 1.42), (0.5, 0.9), (0.5, 0.95), (1.0, 1.0)]));
        assert_eq!(spring.ease(0.0), 0.0);
        assert!((spring.ease(0.125) - 0.71).abs() < 1e-9);
        assert!((spring.ease(0.25) - 1.42).abs() < 1e-9);
        assert!((spring.ease(0.375) - 1.16).abs() < 1e-9);
        assert!((spring.ease(0.5) - 0.95).abs() < 1e-9);
        assert!((spring.ease(0.75) - 0.975).abs() < 1e-9);
        assert_eq!(spring.ease(1.0), 1.0);
        let peak = (1..100).map(|i| spring.ease(i as f64 / 100.0)).fold(0.0, f64::max);
        assert!(peak > 1.4, "{peak}");
        assert_eq!(Curve::from_table(&Easing::Bezier(vec![0.1])), SPATIAL);
    }

    #[test]
    fn a_toast_sliding_in_squashes_and_comes_to_rest() {
        let start = Instant::now();
        let mut slide = Animated::new(400.0, SPATIAL);
        slide.set(start, 0.0, SPATIAL_MS);
        let (mut deform, mut peak, mut rested_at) = (Deform::new(), 0.0_f64, None);
        for frame in 0..240 {
            let now = start + Duration::from_millis(frame * 16);
            deform.step_at(0.016, (1500.0 + slide.value(now), 900.0, 380.0, 90.0), 0.15, DeformEdge::Right);
            peak = peak.max((deform.m00 - 1.0).abs());
            if deform.at_rest && !slide.running(now) && rested_at.is_none() {
                rested_at = Some(frame);
            }
        }
        assert!(peak > 0.01, "the slide never deformed the card: {peak}");
        assert!(rested_at.is_some(), "the springs never came to rest");
        assert_eq!((deform.m00, deform.m01, deform.m11), (1.0, 0.0, 1.0));
    }

    #[test]
    fn a_delayed_set_holds_then_travels() {
        let now = Instant::now();
        let mut y = Animated::new(10.0, SPATIAL);
        y.set_after(now, 20.0, 100.0, 50.0);
        assert_eq!(y.value(now + Duration::from_millis(40)), 10.0);
        assert!(y.running(now + Duration::from_millis(40)));
        assert_eq!(y.value(now + Duration::from_millis(200)), 20.0);
        assert!(!y.running(now + Duration::from_millis(200)));
    }

    #[test]
    fn emphasized_runs_through_its_knee() {
        assert!((EMPHASIZED.ease(1.0 / 6.0) - 0.4).abs() < 1e-9);
        assert!(EMPHASIZED.ease(0.05) < 0.05);
        assert!((EMPHASIZED.ease(1.0 / 3.0) - 0.86).abs() < 0.03);
        let mut last = 0.0;
        for i in 1..=100 {
            let y = EMPHASIZED.ease(i as f64 / 100.0);
            assert!(y >= last, "not monotonic at {i}");
            last = y;
        }
    }
}
