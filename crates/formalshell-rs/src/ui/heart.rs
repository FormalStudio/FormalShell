//! The heart element: a Lucide heart on a soft disc that beats at a
//! measured rate, a ring going out from the disc on every beat. The beat's
//! phase is kept per element and advanced by the clock, so a new rate
//! changes the tempo from where the beat is rather than jumping.

use std::time::Instant;

use vello_cpu::kurbo::{Affine, BezPath, Circle, Rect, Shape};

use super::Cx;
use crate::scene::{Paint, Scene};
use crate::text::{Family, TextStyle};

pub struct Beat {
    /// Beats since the element first beat; the fraction is where in one.
    phase: f64,
    last: Option<Instant>,
    pub seen: u64,
}

/// The first beat's swell and the smaller one after it, as a share of the
/// heart's size.
const LUB: f64 = 0.16;
const DUB: f64 = 0.07;
/// The share of a beat the ring takes to reach the element's edge.
const RING: f64 = 0.7;

/// A pulse rising to 1 at `peak` of a beat and settling after it.
fn bump(t: f64, at: f64, peak: f64) -> f64 {
    let x = (t - at) / peak;
    if x <= 0.0 { 0.0 } else { x * (1.0 - x).exp() }
}

/// The heart's scale at `t` through one beat (0..1).
pub fn swell(t: f64) -> f64 {
    1.0 + LUB * bump(t, 0.0, 0.07) + DUB * bump(t, 0.17, 0.06)
}

/// How far out the ring is at `t` through one beat, none once it has gone.
pub fn ring(t: f64) -> Option<f64> {
    (t < RING).then_some(t / RING)
}

/// Advances the beat at `bpm` to `now`. A frame late past a quarter second
/// (the panel just opened, a stall) counts as that quarter, so the beat
/// carries on from where it was.
fn advance(phase: f64, last: Option<Instant>, now: Instant, bpm: f64, scale: f64) -> f64 {
    let dt = last.map_or(0.0, |l| now.saturating_duration_since(l).as_secs_f64().min(0.25));
    phase + dt * bpm / 60.0 / scale.max(f64::EPSILON)
}

/// `bpm` none draws the heart still in `mutedForeground`, for a rate not
/// measured yet.
pub fn paint(cx: &mut Cx, r: Rect, bpm: Option<f64>, path: &str) {
    let t = cx.theme;
    let live = bpm.is_some_and(|b| b > 0.0);
    let beating = live && t.motion_enabled;
    let (now, frame) = (cx.now, cx.ui.frame);
    let st = cx.ui.beats.entry(path.to_owned()).or_insert(Beat { phase: 0.0, last: None, seen: frame });
    st.seen = frame;
    if beating {
        st.phase = advance(st.phase, st.last, now, bpm.unwrap_or_default(), cx.motion_scale);
        st.last = Some(now);
    } else {
        st.last = None;
    }
    let through = if beating { st.phase.fract() } else { 0.5 };
    if beating {
        cx.animate();
    }

    let ink = if live { t.colors.get("destructive") } else { t.colors.get("mutedForeground") };
    let ink = cx.a(ink);
    let centre = (r.center().x, r.center().y);
    let reach = r.width().min(r.height()) / 2.0;
    let disc = reach * 0.72;
    let well = ink.with_alpha(ink.a * if live { 0.14 } else { 0.1 });

    let g = fs_theme::icons::glyph("heart");
    let size = t.font_size.display_large as f32;
    let shaped = cx.kit.shape(g.text, TextStyle { family: Family::Named(g.family), size, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None });
    let mut heart = BezPath::new();
    for glyph in &shaped.glyphs {
        let mut p = (*glyph.path).clone();
        p.apply_affine(Affine::translate((f64::from(glyph.x), f64::from(glyph.y))));
        heart.extend(p.elements().iter().copied());
    }
    let b = heart.bounding_box();
    let k = if beating { swell(through) } else { 1.0 };
    heart.apply_affine(Affine::translate(centre) * Affine::scale(k) * Affine::translate((-b.center().x, -b.center().y)));

    let mut p = cx.painter(path);
    let well_path = Circle::new(centre, disc).to_path(0.1);
    p.shape(Scene::cover(well_path.bounding_box(), Affine::IDENTITY, 1.0), Paint::Shape { fill: Some((well_path, well)), strokes: Vec::new() });
    p.shape(Scene::cover(heart.bounding_box(), Affine::IDENTITY, 1.0), Paint::Shape { fill: Some((heart, ink)), strokes: Vec::new() });
    // Last, so the disc and the heart keep their nodes whether a ring
    // shows or not.
    let ring_at = ring(through).filter(|_| beating);
    if let Some(f) = ring_at {
        let eased = 1.0 - (1.0 - f).powi(2);
        let radius = disc + (reach - disc) * eased;
        let width = t.space.xxs * (1.0 - f) + t.border_width * f;
        let fade = (1.0 - f).powi(2) * 0.55;
        let circle = Circle::new(centre, radius - width / 2.0).to_path(0.1);
        p.shape(Scene::cover(r, Affine::IDENTITY, 1.0), Paint::Shape { fill: None, strokes: vec![(circle, ink.with_alpha(ink.a * fade as f32), width)] });
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_beat_swells_quickly_and_settles() {
        let peak = (0..100).map(|i| i as f64 / 100.0).max_by(|a, b| swell(*a).total_cmp(&swell(*b))).unwrap();
        assert!(peak < 0.15, "the swell peaks early in the beat, at {peak}");
        assert!(swell(peak) > 1.1);
        assert!((swell(0.95) - 1.0).abs() < 0.01, "settled by the beat's end: {}", swell(0.95));
        assert_eq!(swell(0.0), 1.0);
    }

    #[test]
    fn the_ring_goes_out_then_is_gone() {
        assert_eq!(ring(0.0), Some(0.0));
        assert!(ring(0.35).is_some_and(|f| (f - 0.5).abs() < 1e-9));
        assert_eq!(ring(0.8), None);
    }

    #[test]
    fn a_new_rate_carries_the_phase_on() {
        let t0 = Instant::now();
        let at60 = advance(0.0, Some(t0), t0 + Duration::from_millis(250), 60.0, 1.0);
        assert!((at60 - 0.25).abs() < 1e-9);
        let at120 = advance(at60, Some(t0 + Duration::from_millis(250)), t0 + Duration::from_millis(500), 120.0, 1.0);
        assert!((at120 - 0.75).abs() < 1e-9, "from where it was, twice as fast: {at120}");
        let slow = advance(0.0, Some(t0), t0 + Duration::from_millis(100), 60.0, 10.0);
        assert!((slow - 0.01).abs() < 1e-9, "the motion scale slows it: {slow}");
        assert_eq!(advance(0.4, None, t0, 60.0, 1.0), 0.4);
    }
}
