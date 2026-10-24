//! The animated backdrop's sculpture (`wallpaper.animate`): a seeded cluster
//! of thin triangles turning slowly in 3D, projected flat, and the giant
//! faint mark under it. Geometry only; the shell rasterises it.
//!
//! Every number is the signed-off preview's
//! (docs/superpowers/specs/2026-10-23-cartridge-theme/preview.html), whose
//! canvas is the 1280x800 desktop at half resolution, 640x400. A buffer of
//! another size scales by its height against that 400.

/// The preview's canvas height, what one unit of projected length is
/// measured against.
const REFERENCE_HEIGHT: f64 = 400.0;

/// One triangle in the cluster's own space.
type Tri = [[f64; 3]; 3];

#[derive(Clone, Debug)]
pub struct Cluster {
    tris: Vec<Tri>,
}

/// A triangle as drawn, back to front: its corners in buffer pixels, and
/// whether it faces the viewer closely enough to catch the light. The
/// edge drawn over it runs from the first corner to the second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flat {
    pub pts: [(f64, f64); 3],
    pub lit: bool,
}

/// The Park-Miller generator the preview seeds with, so the same seed lays
/// out the same sculpture.
struct Lehmer(u64);

impl Lehmer {
    fn next(&mut self) -> f64 {
        self.0 = (self.0 * 16807) % 2_147_483_647;
        self.0 as f64 / 2_147_483_647.0
    }
}

fn norm(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let l = if l == 0.0 { 1.0 } else { l };
    [v[0] / l, v[1] / l, v[2] / l]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// How closely a face has to point at the viewer to draw lit.
const LIT: f64 = 0.93;

impl Cluster {
    /// The preview's sculpture: seed 7, 140 shards.
    pub fn reference() -> Self {
        Self::seeded(7, 140)
    }

    pub fn seeded(seed: u64, count: usize) -> Self {
        let mut r = Lehmer(seed.max(1));
        let mut tris = Vec::with_capacity(count);
        for _ in 0..count {
            let c = [(r.next() - 0.5) * 360.0, (r.next() - 0.5) * 300.0, (r.next() - 0.5) * 240.0];
            let d = norm([r.next() - 0.5, (r.next() - 0.5) * 0.6, r.next() - 0.5]);
            let p = norm(cross(d, [r.next() - 0.5, r.next() - 0.5, r.next() - 0.5]));
            let (l, w) = (60.0 + r.next() * 170.0, 6.0 + r.next() * 30.0);
            // The preview draws a fresh number per axis for the third corner.
            let mut third = [0.0; 3];
            for (k, v) in third.iter_mut().enumerate() {
                *v = c[k] + p[k] * w + d[k] * (r.next() - 0.5) * l * 0.3;
            }
            tris.push([
                [c[0] - d[0] * l / 2.0, c[1] - d[1] * l / 2.0, c[2] - d[2] * l / 2.0],
                [c[0] + d[0] * l / 2.0, c[1] + d[1] * l / 2.0, c[2] + d[2] * l / 2.0],
                third,
            ]);
        }
        Self { tris }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// The sculpture `t` milliseconds into its turn, over a `w` by `h`
    /// buffer, farthest first.
    pub fn project(&self, t: f64, w: f64, h: f64) -> Vec<Flat> {
        let ay = t * 0.00012;
        let ax = (t * 0.00009).sin() * 0.35;
        let (sy, cy, sx, cx) = (ay.sin(), ay.cos(), ax.sin(), ax.cos());
        let k = h / REFERENCE_HEIGHT;
        let (ox, oy) = (w * 0.7, h * 0.42);
        let mut out: Vec<(f64, Flat)> = self
            .tris
            .iter()
            .map(|tri| {
                let pts = tri.map(|[x, y, z]| {
                    let (x1, z1) = (x * cy + z * sy, -x * sy + z * cy);
                    [x1, y * cx - z1 * sx, y * sx + z1 * cx]
                });
                let n = norm(cross(sub(pts[1], pts[0]), sub(pts[2], pts[0])));
                let zc = (pts[0][2] + pts[1][2] + pts[2][2]) / 3.0;
                let flat = pts.map(|[x, y, z]| {
                    let f = 520.0 / (z + 700.0) * k;
                    (ox + x * f, oy + y * f)
                });
                (zc, Flat { pts: flat, lit: n[2].abs() > LIT })
            })
            .collect();
        out.sort_by(|a, b| b.0.total_cmp(&a.0));
        out.into_iter().map(|(_, f)| f).collect()
    }
}

/// The box every triangle in `flats` lies in, `(x0, y0, x1, y1)`.
pub fn bounds(flats: &[Flat]) -> Option<(f64, f64, f64, f64)> {
    let mut pts = flats.iter().flat_map(|f| f.pts);
    let (x, y) = pts.next()?;
    Some(pts.fold((x, y, x, y), |(x0, y0, x1, y1), (x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y))))
}

/// The faint mark behind the sculpture: an ellipse ring and a word inside
/// it, both turned by `angle` about the ring's centre. In buffer pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    pub cx: f64,
    pub cy: f64,
    /// The ring's radii along its own axes, to the middle of its stroke.
    pub rx: f64,
    pub ry: f64,
    pub stroke: f64,
    /// Radians, clockwise on screen.
    pub angle: f64,
    /// The word's size, centred on the ring's centre.
    pub text_size: f64,
}

/// The preview's `.mark`: a 450 by 260 border box (half resolution) with a
/// 23px border at -60, 60, turned -14 degrees, and its 150px word.
pub fn mark(h: f64) -> Mark {
    let k = h / REFERENCE_HEIGHT;
    let stroke = 23.0;
    Mark {
        cx: (-60.0 + 225.0) * k,
        cy: (60.0 + 130.0) * k,
        rx: (225.0 - stroke / 2.0) * k,
        ry: (130.0 - stroke / 2.0) * k,
        stroke: stroke * k,
        angle: (-14.0f64).to_radians(),
        text_size: 150.0 * k,
    }
}

/// The share of `area` (`x, y, w, h`) the union of `rects` covers, 0 to 1.
pub fn covered(area: (f64, f64, f64, f64), rects: &[(f64, f64, f64, f64)]) -> f64 {
    let (ax, ay, aw, ah) = area;
    if aw <= 0.0 || ah <= 0.0 {
        return 0.0;
    }
    let clipped: Vec<(f64, f64, f64, f64)> = rects
        .iter()
        .map(|&(x, y, w, h)| (x.max(ax), y.max(ay), (x + w).min(ax + aw), (y + h).min(ay + ah)))
        .filter(|(x0, y0, x1, y1)| x1 > x0 && y1 > y0)
        .collect();
    let mut xs: Vec<f64> = clipped.iter().flat_map(|r| [r.0, r.2]).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup();
    let mut total = 0.0;
    for pair in xs.windows(2) {
        let (x0, x1) = (pair[0], pair[1]);
        let mut spans: Vec<(f64, f64)> = clipped.iter().filter(|r| r.0 <= x0 && r.2 >= x1).map(|r| (r.1, r.3)).collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut run, mut end) = (0.0, f64::NEG_INFINITY);
        for (y0, y1) in spans {
            if y0 > end {
                run += y1 - y0;
                end = y1;
            } else if y1 > end {
                run += y1 - end;
                end = y1;
            }
        }
        total += run * (x1 - x0);
    }
    (total / (aw * ah)).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_lays_out_the_same_sculpture() {
        let (a, b) = (Cluster::reference(), Cluster::reference());
        assert_eq!(a.len(), 140);
        assert_eq!(a.project(1234.0, 640.0, 400.0), b.project(1234.0, 640.0, 400.0));
        assert_ne!(Cluster::seeded(8, 140).project(0.0, 640.0, 400.0), a.project(0.0, 640.0, 400.0));
    }

    #[test]
    fn the_preview_generator_starts_where_the_preview_does() {
        let mut r = Lehmer(7);
        assert!((r.next() - 117649.0 / 2_147_483_647.0).abs() < 1e-15);
    }

    #[test]
    fn it_turns_with_time() {
        let c = Cluster::reference();
        let (a, b) = (c.project(0.0, 640.0, 400.0), c.project(1000.0 / 24.0, 640.0, 400.0));
        assert_ne!(a, b);
        // One frame at 24fps moves the outline by under a pixel or so: slow.
        let (ba, bb) = (bounds(&a).unwrap(), bounds(&b).unwrap());
        assert!((ba.0 - bb.0).abs() < 4.0 && (ba.2 - bb.2).abs() < 4.0, "{ba:?} {bb:?}");
    }

    #[test]
    fn it_sits_on_the_trailing_side_and_scales_with_the_height() {
        let c = Cluster::reference();
        for t in [0.0, 9000.0, 30000.0] {
            let (x0, _, x1, _) = bounds(&c.project(t, 640.0, 400.0)).unwrap();
            let mid = (x0 + x1) / 2.0;
            assert!((300.0..560.0).contains(&mid), "{t}: {mid}");
        }
        let small = bounds(&c.project(0.0, 640.0, 400.0)).unwrap();
        let big = bounds(&c.project(0.0, 1280.0, 800.0)).unwrap();
        assert!(((big.3 - big.1) / (small.3 - small.1) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn some_faces_catch_the_light_and_most_do_not() {
        let c = Cluster::reference();
        let lit: usize = (0..20).map(|i| c.project(f64::from(i) * 2500.0, 640.0, 400.0).iter().filter(|f| f.lit).count()).sum();
        assert!(lit > 0 && lit < 20 * 140 / 4, "{lit}");
    }

    #[test]
    fn the_mark_is_the_previews_box_at_this_height() {
        let m = mark(400.0);
        assert_eq!((m.cx, m.cy, m.stroke, m.text_size), (165.0, 190.0, 23.0, 150.0));
        assert_eq!((m.rx, m.ry), (213.5, 118.5));
        assert_eq!(mark(800.0).cx, 330.0);
    }

    #[test]
    fn coverage_counts_an_overlap_once() {
        let area = (0.0, 0.0, 100.0, 100.0);
        assert_eq!(covered(area, &[]), 0.0);
        assert_eq!(covered(area, &[(0.0, 0.0, 100.0, 100.0)]), 1.0);
        assert_eq!(covered(area, &[(0.0, 0.0, 50.0, 100.0), (50.0, 0.0, 50.0, 100.0)]), 1.0);
        assert_eq!(covered(area, &[(0.0, 0.0, 60.0, 100.0), (40.0, 0.0, 60.0, 50.0)]), 0.8);
        // Only the part inside the area counts.
        assert_eq!(covered((100.0, 0.0, 100.0, 100.0), &[(50.0, 0.0, 100.0, 100.0)]), 0.5);
        assert_eq!(covered(area, &[(200.0, 0.0, 10.0, 10.0)]), 0.0);
    }
}
