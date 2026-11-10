//! The sampler that draws the satellite picture (`services/earth`) onto the
//! globe's near side: every pixel's place on the sphere, then the picture's
//! texel there from the finest level whose tiles are in.

use std::sync::Arc;

use vello_cpu::Pixmap;

use super::model::{Rot, light};
use crate::scene::Bitmap;
use crate::services::earth::{Earth, Source, Stats, lerp, mix4};

/// The run of pixels the sampler draws between two exactly projected
/// ones, and the shorter run it takes over the outer rim of the disc, where
/// the projection bends hardest.
const SPAN: i32 = 16;
const RIM_SPAN: i32 = 4;

/// The atmosphere seen edge on: its blue laid over the picture from
/// `HAZE_FROM` of the way out, rising with the cube of the distance into
/// it to `HAZE_MAX` at the limb, the way the air thickens along a grazing
/// line of sight.
pub const ATMOSPHERE: [u8; 3] = [118, 170, 255];
const HAZE_FROM: f32 = 0.55;
const HAZE_MAX: f32 = 0.55;
const HAZE_RB: u32 = (ATMOSPHERE[2] as u32) << 16 | ATMOSPHERE[0] as u32;
const HAZE_G: u32 = ATMOSPHERE[1] as u32;

/// atan2 to within 1e-5 rad, a thirtieth of a texel on the 21600 wide
/// level, without the libm call per pixel.
#[inline(always)]
fn fast_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let (lo, hi) = if ax > ay { (ay, ax) } else { (ax, ay) };
    if hi == 0.0 {
        return 0.0;
    }
    let a = lo / hi;
    let s = a * a;
    let mut r = a * (0.999_977_26 + s * (-0.332_623_47 + s * (0.193_543_46 + s * (-0.116_432_87 + s * (0.052_653_32 + s * -0.011_721_2)))));
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = std::f32::consts::PI - r;
    }
    if y < 0.0 { -r } else { r }
}

/// The near-side view of the picture over `area` (the disc's box cut to
/// the pane, in buffer pixels), centred at `centre` with `r` pixels to
/// the unit and the camera `distance` sphere radii out, turned by
/// `rot`, and lit by `light` the way the flat globe's land is. Pixels
/// off the sphere stay clear; the caller clips the edge.
///
/// The level is the coarsest with a texel or more per pixel at the
/// disc's centre, where a pixel spans 1/`r` radians.
///
/// The pixels land in a buffer out of `pool` nothing else holds any
/// more (the frame before last's), so a drag maps no fresh pages a
/// frame. `coarse` samples every other pixel of every other row and
/// doubles each, for a globe in motion.
#[allow(clippy::too_many_arguments)]
pub(super) fn sample(earth: &Earth, pool: &mut Vec<Arc<Pixmap>>, coarse: bool, area: (i32, i32, i32, i32), centre: (f64, f64), r: f64, horizon_px: f64, distance: f64, rot: &Rot) -> Option<(Bitmap, Stats)> {
    let (x0, y0, x1, y1) = area;
    let (bw, bh) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
    if bw == 0 || bh == 0 || bw > u16::MAX as usize || bh > u16::MAX as usize {
        return None;
    }
    let top = earth.level_for(std::f64::consts::TAU * r);
    let mut view = earth.view(top);
    let i = match pool.iter_mut().position(|p| Arc::get_mut(p).is_some()) {
        Some(i) => i,
        None => {
            pool.push(Arc::new(Pixmap::new(bw as u16, bh as u16)));
            pool.len() - 1
        }
    };
    let pixmap = Arc::get_mut(&mut pool[i])?;
    pixmap.resize(bw as u16, bh as u16);
    pixmap.set_may_have_transparency(true);
    let out = pixmap.data_as_u8_slice_mut();
    let (cx, cy) = (centre.0 as f32, centre.1 as f32);
    let inv_r = 1.0 / r as f32;
    let d = distance as f32;
    let dm1 = d - 1.0;
    let dd = d * dm1;
    let k2 = d * d - 1.0;
    let reach = horizon_px as f32 + 1.0;
    let inv_horizon = 1.0 / (horizon_px as f32).max(1.0);
    let (cl, sl) = (rot.cos_lat as f32, rot.sin_lat as f32);
    let (co, so) = (rot.cos_lon as f32, rot.sin_lon as f32);
    let l = light();
    let (l0, l1, l2) = (l[0] as f32, l[1] as f32, l[2] as f32);
    let (wf, hf) = (view.levels[top].w as f32, view.levels[top].h as f32);
    let to_x = wf / std::f32::consts::TAU;
    let to_y = hf / std::f32::consts::PI;
    let step = if coarse { 2 } else { 1 };
    for j in 0..bh {
        if coarse && j % 2 == 1 {
            out.copy_within((j - 1) * bw * 4..j * bw * 4, j * bw * 4);
            continue;
        }
        let py = (y0 + j as i32) as f32 + 0.5;
        let dy = py - cy;
        let half2 = reach * reach - dy * dy;
        let row = &mut out[j * bw * 4..(j + 1) * bw * 4];
        if half2 <= 0.0 {
            row.fill(0);
            continue;
        }
        let half = half2.sqrt();
        let xs = ((cx - half).floor() as i32).clamp(x0, x1);
        let xe = ((cx + half).ceil() as i32).clamp(xs, x1);
        row[..(xs - x0) as usize * 4].fill(0);
        row[(xe - x0) as usize * 4..].fill(0);
        let v = -dy * inv_r;
        let vv = v * v + dm1 * dm1;
        // The picture's coordinates at the frame's level and the light
        // at a pixel centre, fx unwrapped (-0.5 up to the width).
        let exact = |x: i32| {
            let u = ((x as f32) + 0.5 - cx) * inv_r;
            let a = u * u + vv;
            let disc = (dd * dd - a * k2).max(0.0);
            let t = (dd - disc.sqrt()) / a;
            let (vx, vy, vz) = (t * u, t * v, d - t * dm1);
            // The view frame back to the world: Rot::apply inverted.
            let p2 = cl * vy + sl * vz;
            let hz = cl * vz - sl * vy;
            let p0 = hz * co - vx * so;
            let p1 = hz * so + vx * co;
            let lon = fast_atan2(p1, p0);
            let lat = fast_atan2(p2, (1.0 - p2 * p2).max(0.0).sqrt());
            let shade = (0.7 + 0.4 * (vx * l0 + vy * l1 + vz * l2).max(0.0)).min(1.0);
            // A narrow limb darkening and the atmosphere's haze over
            // it, both off the distance from the disc's centre, folded
            // in here rather than painted as passes over the disc.
            let dx = x as f32 + 0.5 - cx;
            let rho = (dx * dx + dy * dy).sqrt() * inv_horizon;
            let edge = ((rho - 0.94) / 0.06).clamp(0.0, 1.0);
            let shade = shade * (1.0 - 0.3 * edge);
            let haze = ((rho - HAZE_FROM) / (1.0 - HAZE_FROM)).clamp(0.0, 1.0);
            (lon * to_x + wf * 0.5 - 0.5, hf * 0.5 - lat * to_y - 0.5, shade * 256.0, haze * haze * haze * HAZE_MAX * 256.0)
        };
        // Exact every SPAN pixels and straight lines between: the
        // mapping bends slowly enough that the error stays under a
        // texel everywhere but the last pixels before the limb and
        // round a pole, where the picture's rows are one colour.
        let rim = (0.8 * horizon_px as f32).powi(2) - dy * dy;
        let mut a = xs;
        let mut at = exact(a);
        while a < xe {
            let off = (a as f32 + 0.5 - cx).abs().min((a as f32 + SPAN as f32 + 0.5 - cx).abs());
            let b = (a + if off * off < rim { SPAN } else { RIM_SPAN }).min(xe);
            let bt = exact(b);
            let mut dfx = bt.0 - at.0;
            if dfx > wf * 0.5 {
                dfx -= wf;
            } else if dfx < -wf * 0.5 {
                dfx += wf;
            }
            let n = 1.0 / (b - a) as f32;
            let (sx, sy, ss) = (dfx * n * step as f32, (bt.1 - at.1) * n * step as f32, (bt.2 - at.2) * n * step as f32);
            let sh = (bt.3 - at.3) * n * step as f32;
            // Only the runs out near the rim carry any haze.
            let hazy = at.3 > 0.0 || bt.3 > 0.0;
            let (fx, fy, mut shade, mut haze) = at;
            let mut put = |x: i32, rb: u32, g: u32, shade: f32, haze: f32| {
                let k = shade as u32;
                let (mut rb, mut g) = (((rb * k) >> 8) & 0x00ff_00ff, (g * k) >> 8);
                if hazy {
                    let t = haze as u32;
                    rb = lerp(rb, HAZE_RB, t);
                    g = lerp(g, HAZE_G, t);
                }
                let px = rb | (g << 8) | 0xff00_0000;
                let o = (x - x0) as usize * 4;
                row[o..o + 4].copy_from_slice(&px.to_le_bytes());
                if coarse && x + 1 < b {
                    row[o + 4..o + 8].copy_from_slice(&px.to_le_bytes());
                }
            };
            match view.resolve(fx.min(fx + dfx), fx.max(fx + dfx), at.1.min(bt.1), at.1.max(bt.1)) {
                Source::Tile(l, ti) => {
                    // The whole run inside one tile: today's bilinear
                    // with no wrap or tile lookup per pixel.
                    let lv = &view.levels[l];
                    let tile = lv.tiles[ti].as_deref().expect("a resolved tile");
                    let (ox, oy) = (((ti % lv.cols) * view.tile) as f32, ((ti / lv.cols) * view.tile) as f32);
                    let ymax = (lv.h - 1) as f32;
                    let (tw, th) = (tile.w, tile.h);
                    let stride = tw * 3;
                    let tex = &tile.rgb[..];
                    let (mut lx, mut wy_) = ((fx + 0.5) * lv.sx - 0.5 - ox, (fy + 0.5) * lv.sy - 0.5);
                    let (dx, dy) = (sx * lv.sx, sy * lv.sy);
                    for x in (a..b).step_by(step) {
                        let ly = wy_.clamp(0.0, ymax) - oy;
                        let (gx, gy) = (lx.max(0.0).floor(), ly.max(0.0).floor());
                        let (wx, wy) = (((lx - gx) * 256.0) as u32, ((ly - gy) * 256.0) as u32);
                        let tx0 = (gx as usize).min(tw - 1);
                        let tx1 = (tx0 + 1).min(tw - 1);
                        let ty0 = (gy as usize).min(th - 1);
                        let ty1 = (ty0 + 1).min(th - 1);
                        let (r0, r1) = (ty0 * stride, ty1 * stride);
                        let texel = |i: usize| u32::from_le_bytes(tex[i..i + 4].try_into().unwrap_or([0; 4]));
                        let (rb, g) = mix4([texel(r0 + tx0 * 3), texel(r0 + tx1 * 3), texel(r1 + tx0 * 3), texel(r1 + tx1 * 3)], wx, wy);
                        put(x, rb, g, shade, haze);
                        lx += dx;
                        wy_ += dy;
                        shade += ss;
                        haze += sh;
                    }
                }
                Source::Each(l) => {
                    let (mut fx, mut fy) = (fx, fy);
                    for x in (a..b).step_by(step) {
                        let (rb, g) = view.bilinear(l, fx, fy);
                        put(x, rb, g, shade, haze);
                        fx += sx;
                        fy += sy;
                        shade += ss;
                        haze += sh;
                    }
                }
            }
            a = b;
            at = bt;
        }
    }
    let pixmap = pool[i].clone();
    pool.truncate(3);
    Some((Bitmap { pixmap, density: 1.0 }, earth.finish(view)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::earth::pictures::{coded, pyramid};

    const ROT: Rot = Rot { sin_lat: 0.3, cos_lat: 0.953_939_2, sin_lon: 0.5, cos_lon: 0.866_025_4 };

    fn draw(e: &Earth, r: f64) -> (Pixmap, Stats) {
        let mut pool = Vec::new();
        let (b, stats) = sample(e, &mut pool, false, (100, 50, 900, 850), (500.0, 450.0), r, 396.0, crate::surfaces::atlas::model::view_distance(1.0), &ROT).unwrap();
        ((*b.pixmap).clone(), stats)
    }

    fn worst(a: &Pixmap, b: &Pixmap) -> u8 {
        a.data_as_u8_slice().iter().zip(b.data_as_u8_slice()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    #[test]
    fn tiles_draw_what_the_whole_picture_draws() {
        // 2*pi*547 asks for 3437 texels: the 4096 level, cut into 512 px
        // tiles and every one decoded, against the same 4096 picture whole.
        let tiled = pyramid(&[(1024, 512), (4096, 2048)], 512, true);
        let whole = Earth::single(4096, 2048, coded(4096, 2048));
        let (a, stats) = draw(&tiled, 547.0);
        let (b, _) = draw(&whole, 547.0);
        assert_eq!((stats.level, stats.missing), (4096, 0));
        assert!(stats.tiles > 1, "{stats:?}");
        assert!(worst(&a, &b) <= 1, "tiles and the whole picture differ by {}", worst(&a, &b));
    }

    #[test]
    fn a_missing_level_draws_the_one_under_it_and_asks_for_its_tiles() {
        let empty = pyramid(&[(1024, 512), (4096, 2048)], 512, false);
        let coarse = Earth::single(1024, 512, coded(1024, 512));
        let (a, stats) = draw(&empty, 547.0);
        let (b, _) = draw(&coarse, 547.0);
        assert_eq!(stats.level, 4096);
        assert!(stats.missing > 4 && stats.missing == stats.tiles, "{stats:?}");
        assert!(worst(&a, &b) <= 1, "the fallback differs from the coarse level by {}", worst(&a, &b));
        let (_, resident) = draw(&pyramid(&[(1024, 512), (4096, 2048)], 512, true), 547.0);
        assert_eq!((stats.level, stats.missing), (resident.level, resident.tiles), "it should ask for every tile the resident frame draws");
        assert_eq!(draw(&empty, 547.0).1, stats, "the same view should ask for the same tiles again");
    }

    #[test]
    fn fast_atan2_holds_a_fraction_of_a_texel() {
        let mut worst = 0.0f32;
        for i in 0..3600 {
            let a = (i as f32 / 10.0).to_radians() - std::f32::consts::PI;
            for m in [0.3f32, 1.0, 7.0] {
                let (y, x) = (a.sin() * m, a.cos() * m);
                let e = (fast_atan2(y, x) - y.atan2(x)).abs();
                worst = worst.max(e.min((e - std::f32::consts::TAU).abs()));
            }
        }
        assert!(worst < 1.5e-4, "{worst}");
    }
}
