//! The animated backdrop's sculpture rasterised: `fs_chrome::shards`'s
//! triangles and mark drawn into a premultiplied RGBA buffer at the
//! output's own pixels, for the desktop's own layer over the wallpaper and
//! for the lock's still of it.

use std::sync::Arc;

use fs_chrome::shards::{Cluster, Flat};
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Ellipse, Shape, Stroke};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::scene::IRect;

/// The sculpture's colours and the mark's word, resolved once per theme
/// and wallpaper.
#[derive(Clone)]
pub struct Look {
    pub face: Rgba,
    pub lit: Rgba,
    pub edge: Rgba,
    /// In the preview's 640x400 canvas, scaled by the buffer's height.
    pub edge_width: f64,
    pub mark: Rgba,
    /// The word's glyph outlines at [`WORD_SIZE`], placed about its centre.
    pub word: Arc<Vec<(Arc<BezPath>, f64, f64)>>,
}

impl PartialEq for Look {
    fn eq(&self, o: &Self) -> bool {
        (self.face, self.lit, self.edge, self.edge_width, self.mark) == (o.face, o.lit, o.edge, o.edge_width, o.mark)
            && Arc::ptr_eq(&self.word, &o.word)
    }
}

/// The size the mark's word is shaped at; the raster scales it to the
/// mark's own.
pub const WORD_SIZE: f32 = 100.0;

/// The word the mark carries.
pub const WORD: &str = "FS";

impl Look {
    /// The table's `backdrop.shards` and `backdrop.mark` over `base`, the
    /// wallpaper's own mean colour (the theme's background without one),
    /// every colour times `dim` (the lock's scrim).
    pub fn new(theme: &Theme, base: Rgba, word: Arc<Vec<(Arc<BezPath>, f64, f64)>>, dim: f32) -> Self {
        let s = theme.box_style("backdrop.shards", None);
        let m = theme.box_style("backdrop.mark", None);
        let darken = |c: Rgba| Rgba { r: c.r * dim, g: c.g * dim, b: c.b * dim, a: c.a };
        let edge = s.border.as_ref().map_or((Rgba::TRANSPARENT, 0.0), |b| (b.color, b.width));
        Self {
            face: darken(base.with_alpha(1.0).tint(s.fill)),
            lit: darken(s.ink),
            edge: darken(edge.0),
            edge_width: edge.1,
            mark: darken(m.fill),
            word,
        }
    }
}

/// The mean colour of a wl_shm picture (BGRA in memory), off every 61st
/// pixel: enough to tint by, cheap on the UI thread.
pub fn mean(bgra: &[u8]) -> Option<Rgba> {
    let (mut sum, mut n) = ([0u64; 3], 0u64);
    for px in bgra.chunks_exact(4).step_by(61) {
        sum[0] += u64::from(px[2]);
        sum[1] += u64::from(px[1]);
        sum[2] += u64::from(px[0]);
        n += 1;
    }
    (n > 0).then(|| {
        let c = |v: u64| (v / n) as f32 / 255.0;
        Rgba { r: c(sum[0]), g: c(sum[1]), b: c(sum[2]), a: 1.0 }
    })
}

fn color(c: Rgba) -> AlphaColor<Srgb> {
    let [r, g, b, a] = c.to_u8();
    AlphaColor::from_rgba8(r, g, b, a)
}

pub struct Raster {
    ctx: RenderContext,
    resources: Resources,
    cluster: Cluster,
    /// The mark alone at one size and look, what every frame starts from.
    base: Option<((u16, u16), Look, Pixmap)>,
    /// Bumped whenever `base` is made again: a buffer painted over an older
    /// one has to be painted whole.
    generation: u64,
}

/// What one draw left in its buffer: the base it started from and the box
/// the shards cover over it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Painted {
    pub base: u64,
    pub shards: Option<IRect>,
}

impl Raster {
    pub fn new() -> Self {
        Self { ctx: RenderContext::new(1, 1), resources: Resources::new(), cluster: Cluster::reference(), base: None, generation: 0 }
    }

    /// The sculpture `t` ms into its turn over the mark, into `target` (`w`
    /// by `h`, premultiplied RGBA). `prior` is what the last draw into this
    /// same buffer left there; only the pixels it or this frame's shards
    /// touch are painted again, the whole buffer when there is none.
    pub fn draw(&mut self, target: &mut [u8], w: u16, h: u16, t: f64, look: &Look, prior: Option<Painted>) -> Painted {
        let full = IRect::new(0, 0, i32::from(w), i32::from(h));
        let flats = self.cluster.project(t, f64::from(w), f64::from(h));
        let k = f64::from(h) / 400.0;
        let pad = (look.edge_width * k).ceil() + 1.0;
        let shards = fs_chrome::shards::bounds(&flats).map(|(x0, y0, x1, y1)| {
            let (x0, y0) = ((x0 - pad).floor() as i32, (y0 - pad).floor() as i32);
            let (x1, y1) = ((x1 + pad).ceil() as i32, (y1 + pad).ceil() as i32);
            IRect::new(x0, y0, x1 - x0, y1 - y0).intersect(&full)
        }).filter(|r| !r.is_empty());
        self.ensure_base(w, h, look);
        let painted = Painted { base: self.generation, shards };
        let region = match prior.filter(|p| p.base == self.generation) {
            Some(p) => match (p.shards, shards) {
                (Some(a), Some(b)) => a.union(&b),
                (a, b) => match a.or(b) {
                    Some(r) => r,
                    None => return painted,
                },
            },
            None => full,
        };
        let Some((_, _, base)) = &self.base else { return painted };
        if target.len() != usize::from(w) * usize::from(h) * 4 || region.is_empty() {
            return painted;
        }
        // The damaged rectangle alone: the base copied back under it, and a
        // scene of its size rendered over it in place at its offset. The
        // sculpture keeps to one side of the screen, so whole rows would
        // copy and composite twice the pixels.
        let (Ok(rw), Ok(rh), Ok(rx), Ok(ry)) =
            (u16::try_from(region.w), u16::try_from(region.h), u16::try_from(region.x), u16::try_from(region.y))
        else {
            return painted;
        };
        let stride = usize::from(w) * 4;
        let (x0, row) = (usize::from(rx) * 4, usize::from(rw) * 4);
        for y in usize::from(ry)..usize::from(ry) + usize::from(rh) {
            let at = y * stride + x0;
            target[at..at + row].copy_from_slice(&base.data_as_u8_slice()[at..at + row]);
        }
        self.ctx.reset_and_resize(rw, rh);
        self.ctx.set_transform(Affine::translate((-f64::from(region.x), -f64::from(region.y))));
        let edge_width = look.edge_width * k;
        for f in &flats {
            self.ctx.set_paint(color(if f.lit { look.lit } else { look.face }));
            self.ctx.fill_path(&triangle(f));
            if edge_width > 0.0 && look.edge.a > 0.0 {
                self.ctx.set_paint(color(look.edge));
                self.ctx.fill_path(&edge(f.pts[0], f.pts[1], edge_width));
            }
        }
        self.ctx.flush();
        if let Some(pixmap) = vello_cpu::PixmapMut::new(w, h, target) {
            let settings =
                vello_cpu::RasterizerSettings { target_init: vello_cpu::TargetInit::SrcOver, offset: (rx, ry), ..Default::default() };
            self.ctx.render_with(pixmap, &mut self.resources, settings);
        }
        painted
    }

    /// One still frame as a picture of its own, for the lock.
    pub fn still(&mut self, w: u16, h: u16, t: f64, look: &Look) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h);
        self.draw(pixmap.data_as_u8_slice_mut(), w, h, t, look, None);
        pixmap
    }

    fn ensure_base(&mut self, w: u16, h: u16, look: &Look) {
        if self.base.as_ref().is_some_and(|(size, l, _)| *size == (w, h) && l == look) {
            return;
        }
        self.generation += 1;
        let mut pixmap = Pixmap::new(w, h);
        self.ctx.reset_and_resize(w, h);
        if look.mark.a > 0.0 {
            let m = fs_chrome::shards::mark(f64::from(h));
            self.ctx.set_paint(color(look.mark));
            self.ctx.set_stroke(Stroke::new(m.stroke));
            self.ctx.stroke_path(&Ellipse::new((m.cx, m.cy), (m.rx, m.ry), m.angle).to_path(0.1));
            let at = Affine::translate((m.cx, m.cy)) * Affine::rotate(m.angle) * Affine::scale(m.text_size / f64::from(WORD_SIZE));
            for (path, x, y) in look.word.iter() {
                self.ctx.set_transform(at * Affine::translate((*x, *y)));
                self.ctx.fill_path(path);
            }
        }
        self.ctx.flush();
        self.ctx.render(&mut pixmap, &mut self.resources);
        self.base = Some(((w, h), look.clone(), pixmap));
    }
}

fn triangle(f: &Flat) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(f.pts[0]);
    p.line_to(f.pts[1]);
    p.line_to(f.pts[2]);
    p.close_path();
    p
}

/// The line from `a` to `b`, `width` across with square ends, as an
/// outline to fill: half the cost of stroking it, and a frame strokes 140.
fn edge(a: (f64, f64), b: (f64, f64), width: f64) -> BezPath {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    let (nx, ny) = (-dy / len * width / 2.0, dx / len * width / 2.0);
    let mut p = BezPath::new();
    p.move_to((a.0 + nx, a.1 + ny));
    p.line_to((b.0 + nx, b.1 + ny));
    p.line_to((b.0 - nx, b.1 - ny));
    p.line_to((a.0 - nx, a.1 - ny));
    p.close_path();
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look() -> Look {
        Look {
            face: Rgba::hex(0x33504a),
            lit: Rgba::hex(0xe8e6e3),
            edge: Rgba::hex(0xe8e6e3).with_alpha(0.85),
            edge_width: 0.6,
            mark: Rgba::hex(0x111314).with_alpha(0.16),
            word: Arc::new(Vec::new()),
        }
    }

    #[test]
    fn a_frame_paints_shards_and_leaves_the_rest_clear() {
        let (w, h) = (320u16, 200u16);
        let mut r = Raster::new();
        let mut buf = vec![0u8; usize::from(w) * usize::from(h) * 4];
        let painted = r.draw(&mut buf, w, h, 0.0, &look(), None);
        let damage = painted.shards.expect("the shards land in the buffer");
        let opaque = buf.chunks_exact(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 1000, "{opaque}");
        // The top right corner is past the cluster and the mark.
        assert_eq!(&buf[(usize::from(w) - 1) * 4..usize::from(w) * 4], &[0, 0, 0, 0]);
        assert!(damage.x > 0 && damage.right() <= i32::from(w), "{damage:?}");
    }

    #[test]
    fn frames_a_beat_apart_differ() {
        let (w, h) = (320u16, 200u16);
        let mut r = Raster::new();
        let a = r.still(w, h, 0.0, &look());
        let b = r.still(w, h, 1000.0, &look());
        assert_ne!(a.data_as_u8_slice(), b.data_as_u8_slice());
        assert_eq!(a.data_as_u8_slice(), r.still(w, h, 0.0, &look()).data_as_u8_slice());
    }

    #[test]
    fn a_buffer_painted_in_part_matches_one_painted_whole() {
        let (w, h) = (320u16, 200u16);
        let mut r = Raster::new();
        let len = usize::from(w) * usize::from(h) * 4;
        // Two buffers in turn, as the surface hands them out.
        let (mut a, mut b) = (vec![0u8; len], vec![0u8; len]);
        let mut pa = r.draw(&mut a, w, h, 0.0, &look(), None);
        let mut pb = r.draw(&mut b, w, h, 500.0, &look(), None);
        for i in 1..6 {
            let t = 500.0 + f64::from(i) * 400.0;
            if i % 2 == 1 {
                pa = r.draw(&mut a, w, h, t, &look(), Some(pa));
                assert_eq!(a, r.still(w, h, t, &look()).data_as_u8_slice(), "frame {i}");
            } else {
                pb = r.draw(&mut b, w, h, t, &look(), Some(pb));
                assert_eq!(b, r.still(w, h, t, &look()).data_as_u8_slice(), "frame {i}");
            }
        }
    }

    #[test]
    fn the_mean_reads_bgra() {
        assert_eq!(mean(&[0, 0, 255, 255, 0, 0, 255, 255]).map(|c| c.to_u8()), Some([255, 0, 0, 255]));
        assert_eq!(mean(&[]), None);
    }
}
