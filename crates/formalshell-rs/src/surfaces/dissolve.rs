//! The pixel dissolve (docs/superpowers/specs/2026-10-30-locked-login.md,
//! "The pixel dissolve"): one picture breaks into hard square cells, the
//! cells flip to a second picture from the output's middle out, and the
//! second refines back to its own pixels. The boot's splash into the lock,
//! and the unlock's last picture into the desktop, which is no picture at
//! all: the veil's pixels going clear.
//!
//! A frame is one pass over the output's pixels, written straight into the
//! surface's canvas: each cell row is worked out once and copied down the
//! rest of its rows. A surface drawing on the GPU has no canvas to write
//! into, so it draws the same frame as a scene: each picture's cells as a
//! picture one pixel to a cell, stretched back over the output unfiltered,
//! the first cut to the cells not yet flipped.

use std::sync::Arc;
use std::time::Instant;

use vello_cpu::kurbo::{BezPath, Rect, Shape};
use vello_cpu::{PixelMetadata, Pixmap};

use crate::render::Renderer;
use crate::scene::{Bitmap, IRect, Paint, Scene, VOp};

/// The largest cell, `2^MAX_K` device pixels, which the flips are cut on.
const MAX_K: u32 = 5;
const GRID: usize = 1 << MAX_K;

/// The boot's splash into the lock, and the lock into the desktop.
pub const BOOT_MS: f64 = 1100.0;
pub const UNLOCK_MS: f64 = 700.0;

/// `scene` drawn whole at `scale`, in the output's own pixels.
pub fn picture(scene: &Scene, scale: f64) -> Pixmap {
    let mut r = Renderer::new(scene.size.w.max(1) as u16, scene.size.h.max(1) as u16);
    r.set_scale(scale);
    r.render(scene, IRect::new(0, 0, scene.size.w, scene.size.h));
    r.canvas().clone()
}

/// The cell size at `p` as a power of two: up from 0 to [`MAX_K`] over the
/// first third, held there through the middle, and back down over the last.
pub fn cell_k(p: f64) -> u32 {
    let p = p.clamp(0.0, 1.0);
    let steps = f64::from(MAX_K + 1);
    if p < 1.0 / 3.0 {
        ((p * 3.0 * steps) as u32).min(MAX_K)
    } else if p < 2.0 / 3.0 {
        MAX_K
    } else {
        MAX_K - (((p - 2.0 / 3.0) * 3.0 * steps) as u32).min(MAX_K)
    }
}

/// How far the flips have got at `p`: 0 through the first third, 1 through
/// the last. A cell whose threshold is under it shows the destination.
pub fn flip(p: f64) -> f32 {
    ((p - 1.0 / 3.0) * 3.0).clamp(0.0, 1.0) as f32
}

/// A 32 px cell's place in the flip order: its distance from the output's
/// centre (1 at a corner) weighted 0.6 and a hash of it 0.4, under 1.
pub fn threshold(col: usize, row: usize, (w, h): (usize, usize)) -> f32 {
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let (x, y) = (((col * GRID + GRID / 2) as f32).min(w as f32) - cx, ((row * GRID + GRID / 2) as f32).min(h as f32) - cy);
    let d = ((x * x + y * y).sqrt() / (cx * cx + cy * cy).sqrt().max(1.0)).min(1.0);
    (0.6 * d + 0.4 * hash(col as u32, row as u32)).min(0.999_999)
}

/// Uniform in 0..1 for a cell.
fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ 0x9e37_79b9;
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// The flip order of every 32 px cell over a `w` by `h` output, row by row.
pub fn thresholds(w: usize, h: usize) -> Vec<f32> {
    let (cols, rows) = (w.div_ceil(GRID), h.div_ceil(GRID));
    (0..rows).flat_map(|r| (0..cols).map(move |c| threshold(c, r, (w, h)))).collect()
}

/// One frame at `p` into `out`, RGBA premultiplied like `from`; `to` none
/// is clear. All three are `w * h * 4` bytes.
pub fn frame(from: &[u8], to: Option<&[u8]>, (w, h): (usize, usize), order: &[f32], p: f64, out: &mut [u8]) {
    let c = 1usize << cell_k(p);
    let f = flip(p);
    let cols = w.div_ceil(GRID);
    let stride = w * 4;
    let sample = |src: Option<&[u8]>, x: usize, y: usize| -> [u8; 4] {
        match src {
            Some(s) => {
                let i = y * stride + x * 4;
                [s[i], s[i + 1], s[i + 2], s[i + 3]]
            }
            None => [0; 4],
        }
    };
    let mut y = 0;
    while y < h {
        let rows = c.min(h - y);
        let sy = (y + c / 2).min(h - 1);
        let line = &mut out[y * stride..(y + 1) * stride];
        let order_row = &order[(y / GRID) * cols..(y / GRID + 1) * cols];
        if c == 1 && (f <= 0.0 || f >= 1.0) {
            let src = if f <= 0.0 { Some(from) } else { to };
            match src {
                Some(s) => line.copy_from_slice(&s[y * stride..(y + 1) * stride]),
                None => line.fill(0),
            }
        } else {
            let mut x = 0;
            while x < w {
                let n = c.min(w - x);
                let src = if order_row[x / GRID] < f { to } else { Some(from) };
                let px = sample(src, (x + c / 2).min(w - 1), sy);
                for d in line[x * 4..(x + n) * 4].as_chunks_mut::<4>().0 {
                    d.copy_from_slice(&px);
                }
                x += n;
            }
        }
        for r in 1..rows {
            out.copy_within(y * stride..(y + 1) * stride, (y + r) * stride);
        }
        y += rows;
    }
}

/// `src`'s cells `c` pixels wide, one pixel each: the pixel [`frame`]
/// fills its cell with.
fn cells(src: &Pixmap, c: usize) -> Pixmap {
    let (w, h) = (usize::from(src.width()), usize::from(src.height()));
    let (cw, ch) = (w.div_ceil(c), h.div_ceil(c));
    let data = src.data_as_u8_slice();
    let mut out = Vec::with_capacity(cw * ch * 4);
    for j in 0..ch {
        let sy = (j * c + c / 2).min(h - 1);
        for i in 0..cw {
            let at = sy * w * 4 + (i * c + c / 2).min(w - 1) * 4;
            out.extend_from_slice(&data[at..at + 4]);
        }
    }
    Pixmap::from_parts(out, cw as u16, ch as u16, PixelMetadata::default())
}

/// One dissolve on one output, by the clock.
pub struct Dissolve {
    from: Pixmap,
    to: Option<Pixmap>,
    order: Vec<f32>,
    at: Instant,
    ms: f64,
    /// Each picture's cells at each cell size, made the first time a GPU
    /// frame asks for them.
    levels: [Option<(Arc<Pixmap>, Option<Arc<Pixmap>>)>; MAX_K as usize + 1],
    scene: Option<Scene>,
}

impl Dissolve {
    /// `to` none dissolves into clear; it is the same size as `from`.
    pub fn new(from: Pixmap, to: Option<Pixmap>, ms: f64, now: Instant) -> Self {
        let order = thresholds(usize::from(from.width()), usize::from(from.height()));
        let to = to.filter(|t| (t.width(), t.height()) == (from.width(), from.height()));
        Self { from, to, order, at: now, ms, levels: Default::default(), scene: None }
    }

    pub fn progress(&self, now: Instant) -> f64 {
        crate::motion::progress(self.at, self.ms, now)
    }

    pub fn done(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }

    /// This moment's frame into a canvas of `size` device pixels; false
    /// when the canvas is not the size the pictures were made at.
    pub fn draw(&self, now: Instant, out: &mut [u8], size: (i32, i32)) -> bool {
        let (w, h) = (usize::from(self.from.width()), usize::from(self.from.height()));
        if size != (w as i32, h as i32) || out.len() != w * h * 4 {
            return false;
        }
        let t0 = Instant::now();
        frame(self.from.data_as_u8_slice(), self.to.as_ref().map(Pixmap::data_as_u8_slice), (w, h), &self.order, self.progress(now), out);
        eprintln!("dissolve frame {:.2}", t0.elapsed().as_secs_f64() * 1000.0);
        true
    }

    /// This moment's frame as a scene `size` logical pixels at `scale`, for
    /// a surface drawing on the GPU; none when `device` (its canvas) is not
    /// the size the pictures were made at.
    pub fn scene(&mut self, now: Instant, size: IRect, scale: f64, device: (i32, i32)) -> Option<&mut Scene> {
        let (w, h) = (usize::from(self.from.width()), usize::from(self.from.height()));
        if device != (w as i32, h as i32) {
            return None;
        }
        let p = self.progress(now);
        let k = cell_k(p) as usize;
        let c = 1usize << k;
        let f = flip(p);
        let (from, to) = self.levels[k].get_or_insert_with(|| {
            let made = |src: &Pixmap| Arc::new(if c == 1 { src.clone() } else { cells(src, c) });
            (made(&self.from), self.to.as_ref().map(made))
        });
        let picture = |pixmap: &Arc<Pixmap>| VOp::Image(Bitmap { pixmap: pixmap.clone(), density: (scale / c as f64) as f32 }, (0.0, 0.0), 1.0);
        let mut ops = Vec::new();
        if let Some(to) = to.as_ref().filter(|_| f > 0.0) {
            ops.push(picture(to));
        }
        if f < 1.0 {
            // The cells still on the first picture, a run of them a rect.
            let cols = w.div_ceil(GRID);
            let mut keep = BezPath::new();
            for (row, line) in self.order.chunks(cols).enumerate() {
                let (y0, y1) = ((row * GRID) as f64 / scale, ((row + 1) * GRID).min(h) as f64 / scale);
                let mut col = 0;
                while col < cols {
                    if line[col] < f {
                        col += 1;
                        continue;
                    }
                    let start = col;
                    while col < cols && line[col] >= f {
                        col += 1;
                    }
                    let (x0, x1) = ((start * GRID) as f64 / scale, (col * GRID).min(w) as f64 / scale);
                    keep.extend(Rect::new(x0, y0, x1, y1).path_elements(0.1));
                }
            }
            ops.extend([VOp::Clip(keep), picture(from), VOp::Pop]);
        }
        // A scene of its own each frame, damaged whole.
        let mut scene = Scene::new(size.w, size.h);
        scene.add(size, Paint::Vector(Arc::new(ops)));
        Some(self.scene.insert(scene))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, px: [u8; 4]) -> Vec<u8> {
        px.iter().copied().cycle().take(w * h * 4).collect()
    }

    /// Every pixel its own colour, so a frame's cells read off it.
    fn ramp(w: usize, h: usize) -> Vec<u8> {
        (0..w * h).flat_map(|i| [(i % 251) as u8, (i / 251 % 251) as u8, (i % 7) as u8 * 30, 255]).collect()
    }

    #[test]
    fn the_scene_draws_the_frame_the_canvas_gets() {
        let (w, h) = (100usize, 70usize);
        let pix = |b: Vec<u8>| Pixmap::from_parts(b, w as u16, h as u16, PixelMetadata::default());
        let from = ramp(w, h);
        let to: Vec<u8> = ramp(h, w).into_iter().rev().collect();
        let order = thresholds(w, h);
        let at = Instant::now();
        for to in [None, Some(&to)] {
            let mut d = Dissolve::new(pix(from.clone()), to.map(|t| pix(t.clone())), 1000.0, at);
            for p in [0.0, 0.1, 0.2, 0.4, 0.5, 0.6, 0.8, 0.9, 1.0] {
                let mut want = vec![0u8; w * h * 4];
                frame(&from, to.map(Vec::as_slice), (w, h), &order, p, &mut want);
                let now = at + std::time::Duration::from_secs_f64(p);
                let scene = d.scene(now, IRect::new(0, 0, w as i32, h as i32), 1.0, (w as i32, h as i32)).unwrap();
                let mut r = Renderer::new(w as u16, h as u16);
                r.render(scene, scene.size);
                let got = r.canvas().data_as_u8_slice();
                let off = got.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > 1)).count();
                assert_eq!(off, 0, "{off} pixels off the canvas frame at {p}, to {}", to.is_some());
            }
        }
    }

    #[test]
    fn cells_coarsen_hold_and_refine() {
        assert_eq!(cell_k(0.0), 0);
        assert_eq!(cell_k(0.17), 3);
        assert_eq!(cell_k(0.33), 5);
        assert_eq!(cell_k(0.5), 5);
        assert_eq!(cell_k(0.67), 5);
        assert_eq!(cell_k(0.84), 2);
        assert_eq!(cell_k(1.0), 0);
        let ks: Vec<u32> = (0..=300).map(|i| cell_k(f64::from(i) / 300.0)).collect();
        assert!(ks[..100].windows(2).all(|w| w[0] <= w[1]));
        assert!(ks[200..].windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(flip(0.3), 0.0);
        assert_eq!(flip(0.7), 1.0);
    }

    #[test]
    fn thresholds_open_from_the_middle_and_stay_under_one() {
        let (w, h) = (2560, 1440);
        let order = thresholds(w, h);
        assert!(order.iter().all(|t| (0.0..1.0).contains(t)));
        let cols = w.div_ceil(GRID);
        let centre = order[(h / GRID / 2) * cols + cols / 2];
        let corner = order[0];
        assert!(centre < 0.4 && corner > 0.6, "centre {centre} corner {corner}");
    }

    #[test]
    fn the_ends_are_the_pictures_exactly() {
        let (w, h) = (77, 45);
        let (from, to) = (ramp(w, h), flat(w, h, [10, 200, 30, 255]));
        let order = thresholds(w, h);
        let mut out = vec![0; w * h * 4];
        frame(&from, Some(&to), (w, h), &order, 0.0, &mut out);
        assert_eq!(out, from);
        frame(&from, Some(&to), (w, h), &order, 1.0, &mut out);
        assert_eq!(out, to);
        frame(&from, None, (w, h), &order, 1.0, &mut out);
        assert!(out.iter().all(|b| *b == 0));
    }

    #[test]
    fn a_mid_frame_is_whole_cells_of_both_on_the_grid() {
        let (w, h) = (200, 130);
        let (from, to) = (ramp(w, h), flat(w, h, [10, 200, 30, 255]));
        let order = thresholds(w, h);
        let mut out = vec![0; w * h * 4];
        frame(&from, Some(&to), (w, h), &order, 0.5, &mut out);
        let px = |x: usize, y: usize| &out[(y * w + x) * 4..(y * w + x) * 4 + 4];
        let (mut src, mut dst) = (0, 0);
        for cy in (0..h).step_by(GRID) {
            for cx in (0..w).step_by(GRID) {
                let first = px(cx, cy).to_vec();
                for y in cy..(cy + GRID).min(h) {
                    for x in cx..(cx + GRID).min(w) {
                        assert_eq!(px(x, y), &first[..], "cell {cx},{cy} not one colour at {x},{y}");
                    }
                }
                let (sx, sy) = ((cx + GRID / 2).min(w - 1), (cy + GRID / 2).min(h - 1));
                if first == [10, 200, 30, 255] {
                    dst += 1;
                } else {
                    assert_eq!(&first[..], &from[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4], "a cell shows its centre pixel");
                    src += 1;
                }
            }
        }
        assert!(src > 0 && dst > 0, "{src} source cells, {dst} destination cells");
    }

    #[test]
    fn flipped_cells_stay_flipped_as_the_cells_refine() {
        let (w, h) = (96, 64);
        let (from, to) = (flat(w, h, [255, 0, 0, 255]), flat(w, h, [0, 0, 255, 255]));
        let order = thresholds(w, h);
        let mut out = vec![0; w * h * 4];
        let mut was: Vec<bool> = vec![false; w * h];
        for i in 0..=60 {
            frame(&from, Some(&to), (w, h), &order, f64::from(i) / 60.0, &mut out);
            for (k, px) in out.as_chunks::<4>().0.iter().enumerate() {
                let now = px[2] == 255;
                assert!(now || !was[k], "pixel {k} went back at step {i}");
                was[k] = now;
            }
        }
        assert!(was.iter().all(|b| *b));
    }

    #[test]
    fn a_full_frame_fits_its_budget() {
        let (w, h) = (2560, 1440);
        let (from, to) = (ramp(w, h), flat(w, h, [10, 200, 30, 255]));
        let order = thresholds(w, h);
        let mut out = vec![0; w * h * 4];
        let mut worst = 0.0f64;
        for p in [0.05, 0.2, 0.4, 0.5, 0.6, 0.8, 0.95] {
            let t0 = Instant::now();
            frame(&from, Some(&to), (w, h), &order, p, &mut out);
            worst = worst.max(t0.elapsed().as_secs_f64() * 1000.0);
        }
        eprintln!("dissolve worst frame at 2560x1440: {worst:.2}ms");
        if !cfg!(debug_assertions) {
            assert!(worst < 6.0, "{worst}ms");
        }
    }
}
