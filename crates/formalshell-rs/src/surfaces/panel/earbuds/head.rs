//! The earbuds panel's head: a wireframe head (a globe of latitude and longitude
//! lines, two eyes, two ears, a nose) turned by the buds' orientation relative to
//! where the wearer last recentred. The drawn pose follows each 25 Hz
//! sample by the clock, so the head moves at the output's refresh rate
//! rather than in sample steps, and stops asking for frames once it lands.
//!
//! The picture is a mirror: the wearer turning left sees the head turn to
//! the screen's left, nodding sees it nod toward them. Lines facing away
//! are drawn fainter the further round they are.

use fs_theme::palette::ColorRole;
use std::time::Instant;

use vello_cpu::kurbo::{BezPath, Circle, Point, Rect, Shape};

use crate::scene::{Paint, Scene};
use crate::ui::{Cx, State, Widget};

type Quat = [f64; 4];
type Vec3 = [f64; 3];

const IDENTITY: Quat = [1.0, 0.0, 0.0, 0.0];

/// The wearer's head in a `size` square turned by `orientation` (a
/// relative unit quaternion, `[w, x, y, z]`), eased toward each new one;
/// none draws it still and dim.
#[derive(Clone, Debug, PartialEq)]
pub struct Head {
    pub orientation: Option<[f32; 4]>,
    pub size: f64,
}

impl Widget for Head {
    fn measure(&self, _cx: &mut Cx, _avail: f64) -> (f64, f64) {
        (self.size, self.size)
    }

    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str) {
        paint(cx, inner, self.orientation, path);
    }
}

/// The pose drawn last and when, per element.
struct Pose {
    shown: Quat,
    last: Option<Instant>,
}

impl State for Pose {}

/// The buds' body frame to the screen's, the one place it is decided:
/// body x forward, y left, z up (`fs_devices::earbuds::motion`) to screen
/// x right, y down and z toward the viewer, as a mirror shows it. A
/// measured calibration that finds an axis flipped changes this alone.
fn body_to_screen(v: Vec3) -> Vec3 {
    [-v[1], -v[2], v[0]]
}

/// The camera sits this far above the head's level, so the rings read as
/// ellipses and a nod shows.
const TILT_DEG: f64 = 12.0;
/// The camera's distance in head radii; far enough for a light perspective.
const DISTANCE: f64 = 5.0;
/// The time constant the drawn pose follows a new sample with: one sample
/// period of the 25 Hz stream, so it lands about as the next one arrives.
const FOLLOW_S: f64 = 0.04;
/// Closer than this and the drawn pose takes the sample and stops.
const LANDED_DEG: f64 = 0.1;

const LATITUDES_DEG: [f64; 3] = [-45.0, 0.0, 45.0];
const MERIDIANS: usize = 4;
/// Where the eyes sit on the globe, and their half width and height.
const EYE_LAT_DEG: f64 = 20.0;
const EYE_LON_DEG: f64 = 27.0;
const EYE: (f64, f64) = (0.11, 0.055);
/// Points per full circle; enough that no ring shows a corner.
const SEGMENTS: usize = 96;
/// Depth bands a line is cut into, back to front, each its own alpha.
const BANDS: usize = 6;

fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

fn normalised(q: Quat) -> Quat {
    let n = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if n < 1e-9 { IDENTITY } else { q.map(|c| c / n) }
}

fn rotate(q: Quat, v: Vec3) -> Vec3 {
    let p = mul(mul(q, [0.0, v[0], v[1], v[2]]), [q[0], -q[1], -q[2], -q[3]]);
    [p[1], p[2], p[3]]
}

fn dot(a: Quat, b: Quat) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The angle between two orientations, in degrees.
fn apart_deg(a: Quat, b: Quat) -> f64 {
    2.0 * dot(a, b).abs().min(1.0).acos().to_degrees()
}

/// `t` of the way from `a` to `b` along the shorter arc.
fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let mut d = dot(a, b);
    let b = if d < 0.0 {
        d = -d;
        b.map(|c| -c)
    } else {
        b
    };
    if d > 0.9995 {
        return normalised([0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t));
    }
    let theta = d.acos();
    let (wa, wb) = (((1.0 - t) * theta).sin() / theta.sin(), (t * theta).sin() / theta.sin());
    normalised([0, 1, 2, 3].map(|i| a[i] * wa + b[i] * wb))
}

/// The drawn pose one frame on: toward `target` by the time since the last
/// frame (a quarter second at most, for a panel just opened or a stall),
/// landing on it once close.
fn follow(shown: Quat, last: Option<Instant>, now: Instant, target: Quat, scale: f64) -> Quat {
    let Some(last) = last else { return target };
    let dt = now.saturating_duration_since(last).as_secs_f64().min(0.25);
    let next = slerp(shown, target, 1.0 - (-dt / (FOLLOW_S * scale.max(f64::EPSILON))).exp());
    if apart_deg(next, target) < LANDED_DEG { target } else { next }
}

/// A point of the head model (head radius 1) after `q`, on screen: its
/// position and how far toward the viewer it sits (-1 behind to 1 in front
/// on the sphere).
struct View {
    q: Quat,
    centre: Point,
    radius: f64,
    tilt: (f64, f64),
}

impl View {
    fn project(&self, body: Vec3) -> (Point, f64) {
        let [x, y, z] = body_to_screen(rotate(self.q, body));
        let (sin, cos) = self.tilt;
        let (y, z) = (y * cos + z * sin, z * cos - y * sin);
        let k = DISTANCE / (DISTANCE - z);
        (Point::new(self.centre.x + x * k * self.radius, self.centre.y + y * k * self.radius), z)
    }
}

/// The band a depth falls in: 0 well behind, `BANDS - 1` well in front,
/// the limb spread across the middle ones.
fn band(depth: f64) -> usize {
    let x = ((depth + 0.35) / 0.7).clamp(0.0, 1.0);
    ((x * BANDS as f64) as usize).min(BANDS - 1)
}

/// How much of the ink a band carries, and its width in border widths.
fn band_ink(b: usize) -> (f32, f64) {
    let f = b as f64 / (BANDS - 1) as f64;
    let eased = f * f * (3.0 - 2.0 * f);
    ((0.14 + 0.66 * eased) as f32, 1.0 + 0.4 * eased)
}

/// A polyline of model points, cut into the depth bands it crosses. Each
/// band continues one subpath for as long as the line stays in it.
fn trace(view: &View, points: &[Vec3], closed: bool, bands: &mut [BezPath]) {
    let projected: Vec<(Point, f64)> = points.iter().map(|p| view.project(*p)).collect();
    let n = projected.len();
    let edges = if closed { n } else { n.saturating_sub(1) };
    let mut open: Option<usize> = None;
    for i in 0..edges {
        let (a, da) = projected[i];
        let (b, db) = projected[(i + 1) % n];
        let at = band((da + db) / 2.0);
        if open != Some(at) {
            bands[at].move_to(a);
            open = Some(at);
        }
        bands[at].line_to(b);
    }
}

fn circle_points(f: impl Fn(f64) -> Vec3) -> Vec<Vec3> {
    (0..SEGMENTS).map(|i| f(i as f64 / SEGMENTS as f64 * std::f64::consts::TAU)).collect()
}

/// An ellipse lying on the globe round the point at `lat`, `lon`.
fn on_globe(lat: f64, lon: f64, (rx, ry): (f64, f64)) -> Vec<Vec3> {
    let (lat, lon) = (lat.to_radians(), lon.to_radians());
    let centre = [lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin()];
    let east = [-lon.sin(), lon.cos(), 0.0];
    let north = [-lat.sin() * lon.cos(), -lat.sin() * lon.sin(), lat.cos()];
    circle_points(|a| {
        let p: Vec3 = [0, 1, 2].map(|i| centre[i] + rx * a.cos() * east[i] + ry * a.sin() * north[i]);
        let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        p.map(|c| c / n)
    })
}

/// The globe's rings, the meridians, the eyes and the ears, in the body
/// frame.
fn wireframe() -> Vec<Vec<Vec3>> {
    let mut lines = Vec::new();
    for lat in LATITUDES_DEG {
        let (z, r) = (lat.to_radians().sin(), lat.to_radians().cos());
        lines.push(circle_points(|a| [r * a.cos(), r * a.sin(), z]));
    }
    for m in 0..MERIDIANS {
        let lon = m as f64 / MERIDIANS as f64 * std::f64::consts::PI;
        let (c, s) = (lon.cos(), lon.sin());
        lines.push(circle_points(|a| [a.cos() * c, a.cos() * s, a.sin()]));
    }
    for side in [1.0, -1.0] {
        lines.push(on_globe(EYE_LAT_DEG, side * EYE_LON_DEG, EYE));
        lines.push(circle_points(|a| [-0.05 + 0.17 * a.cos(), side * 1.06, -0.04 + 0.3 * a.sin()]));
    }
    lines
}

/// The nose: a small pyramid off the face, as its outline on screen and
/// the depth of its tip.
fn nose(view: &View) -> (BezPath, f64) {
    let corners: [Vec3; 4] = [[1.32, 0.0, -0.2], [0.99, 0.0, 0.14], [0.95, 0.15, -0.24], [0.95, -0.15, -0.24]];
    let pts: Vec<(Point, f64)> = corners.iter().map(|c| view.project(*c)).collect();
    let hull = hull(&pts.iter().map(|p| p.0).collect::<Vec<_>>());
    let mut path = BezPath::new();
    for (i, p) in hull.iter().enumerate() {
        if i == 0 { path.move_to(*p) } else { path.line_to(*p) }
    }
    path.close_path();
    (path, pts[0].1)
}

/// The convex hull of a handful of points, by gift wrapping.
fn hull(points: &[Point]) -> Vec<Point> {
    let Some(start) = points.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y))) else {
        return Vec::new();
    };
    let mut out = vec![start];
    let mut at = start;
    loop {
        let mut next = points[0];
        for &p in points {
            let cross = (next.x - at.x) * (p.y - at.y) - (next.y - at.y) * (p.x - at.x);
            if next == at || cross < 0.0 {
                next = p;
            }
        }
        if next == start || out.len() > points.len() {
            break;
        }
        out.push(next);
        at = next;
    }
    out
}

/// `orientation` none draws the head facing the viewer in
/// `mutedForeground`, for no sample yet.
fn paint(cx: &mut Cx, r: Rect, orientation: Option<[f32; 4]>, path: &str) {
    let t = cx.theme;
    let target = orientation.map_or(IDENTITY, |q| normalised(q.map(f64::from)));
    let (now, scale) = (cx.now, t.motion_scale);
    let mut held = cx.take(path, || Pose { shown: target, last: None });
    let st = &mut *held;
    if orientation.is_some() && t.motion_enabled {
        st.shown = follow(st.shown, st.last, now, target, scale);
        st.last = Some(now);
    } else {
        st.shown = target;
        st.last = None;
    }
    let shown = st.shown;
    cx.keep(held);
    if shown != target {
        cx.animate();
    }

    let live = orientation.is_some();
    let ink = cx.a(if live { t.colors.get(ColorRole::Foreground) } else { t.colors.get(ColorRole::MutedForeground) });
    let accent = cx.a(if live { t.colors.get(ColorRole::Primary) } else { t.colors.get(ColorRole::MutedForeground) });
    let reach = r.width().min(r.height()) / 2.0;
    let tilt = TILT_DEG.to_radians();
    // The ears reach past the globe; leave them room at any turn.
    let view = View { q: shown, centre: r.center(), radius: reach / 1.35, tilt: (tilt.sin(), tilt.cos()) };

    let mut bands: Vec<BezPath> = (0..BANDS).map(|_| BezPath::new()).collect();
    for line in wireframe() {
        trace(&view, &line, true, &mut bands);
    }
    let (nose_path, nose_depth) = nose(&view);
    // The sphere's outline under perspective sits a little outside its
    // radius, where the sight lines graze it.
    let limb = view.radius * DISTANCE / (DISTANCE * DISTANCE - 1.0).sqrt();
    let disc = Circle::new(view.centre, limb).to_path(0.1);
    let well = ink.with_alpha(ink.a * if live { 0.06 } else { 0.04 });
    let width = t.border_width;
    let strokes = |range: std::ops::Range<usize>, bands: &mut Vec<BezPath>| {
        range
            .map(|b| {
                let (alpha, w) = band_ink(b);
                (std::mem::take(&mut bands[b]), ink.with_alpha(ink.a * alpha), width * w)
            })
            .collect::<Vec<_>>()
    };
    let half = BANDS / 2;
    let back = strokes(0..half, &mut bands);
    let mut front = strokes(half..BANDS, &mut bands);
    front.insert(0, (disc.clone(), ink.with_alpha(ink.a * 0.5), width));
    let (nose_alpha, _) = band_ink(band(nose_depth));
    let nose_fill = Some((nose_path, accent.with_alpha(accent.a * nose_alpha)));
    let behind = nose_depth < 0.0;

    // A fixed run of nodes whichever side the nose is on.
    let cover = Scene::cover(r, vello_cpu::kurbo::Affine::IDENTITY, 1.0);
    let mut p = cx.painter(path);
    p.shape(cover, Paint::Shape { fill: Some((disc, well)), strokes: Vec::new() });
    p.shape(cover, Paint::Shape { fill: None, strokes: back });
    p.shape(cover, Paint::Shape { fill: if behind { nose_fill.clone() } else { None }, strokes: Vec::new() });
    p.shape(cover, Paint::Shape { fill: None, strokes: front });
    p.shape(cover, Paint::Shape { fill: if behind { None } else { nose_fill }, strokes: Vec::new() });
    let last = p.last();
    p.finish();
    cx.done(last);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn yaw(deg: f64) -> Quat {
        let h = deg.to_radians() / 2.0;
        [h.cos(), 0.0, 0.0, h.sin()]
    }

    #[test]
    fn a_left_turn_takes_the_nose_to_the_screens_left() {
        let view = View { q: yaw(40.0), centre: Point::ZERO, radius: 1.0, tilt: (0.0, 1.0) };
        let (tip, depth) = view.project([1.0, 0.0, 0.0]);
        assert!(tip.x < -0.5, "the nose goes left: {tip:?}");
        assert!(depth > 0.5, "still facing the viewer: {depth}");
        let ahead = View { q: IDENTITY, ..view };
        let (tip, depth) = ahead.project([1.0, 0.0, 0.0]);
        assert!(tip.x.abs() < 1e-9 && depth > 0.99, "at rest the nose faces out of the screen");
        let (crown, _) = ahead.project([0.0, 0.0, 1.0]);
        assert!(crown.y < -0.9, "the crown is up the screen: {crown:?}");
    }

    #[test]
    fn the_drawn_pose_follows_by_the_clock_and_lands() {
        let t0 = Instant::now();
        let target = yaw(40.0);
        assert_eq!(follow(IDENTITY, None, t0, target, 1.0), target, "a first frame takes the sample");
        let one = follow(IDENTITY, Some(t0), t0 + Duration::from_millis(16), target, 1.0);
        let moved = apart_deg(IDENTITY, one);
        assert!(moved > 5.0 && moved < 35.0, "part way in one frame: {moved}");
        let slow = follow(IDENTITY, Some(t0), t0 + Duration::from_millis(16), target, 10.0);
        assert!(apart_deg(IDENTITY, slow) < moved / 5.0, "the motion scale slows it");
        let late = follow(one, Some(t0), t0 + Duration::from_millis(250), target, 1.0);
        assert_eq!(late, target, "a quarter second on it has landed");
    }

    #[test]
    fn slerp_takes_the_short_way_round() {
        let a = yaw(10.0);
        let b = yaw(30.0).map(|c| -c);
        let mid = slerp(a, b, 0.5);
        assert!((apart_deg(mid, yaw(20.0))).abs() < 1e-6);
    }

    #[test]
    fn the_hull_of_a_square_and_its_centre_is_the_square() {
        let pts = [Point::new(0.0, 0.0), Point::new(1.0, 0.0), Point::new(0.5, 0.5), Point::new(1.0, 1.0), Point::new(0.0, 1.0)];
        assert_eq!(hull(&pts).len(), 4);
    }

    #[test]
    fn depth_bands_run_back_to_front() {
        assert_eq!(band(-1.0), 0);
        assert_eq!(band(1.0), BANDS - 1);
        assert!(band_ink(0).0 < band_ink(BANDS - 1).0);
    }
}
