//! The motion catalogue a table may name past its clocks (cartridge spec
//! C5), as pure functions of a progress: what each effect does to a
//! position, an opacity, a scale or a label's axes at that moment. The
//! surfaces own the clocks; nothing here keeps time.

use fs_theme::style::{Glitch, Rows, TYPE_STEPS, Use};

use crate::motion::Curve;
use crate::scene::Bitmap;

/// CSS `steps(n)` (jump-end): the progress held flat on `n` stairs, landing
/// on 1 only at the end.
pub fn steps(p: f64, n: usize) -> f64 {
    if p >= 1.0 {
        return 1.0;
    }
    let n = n.max(1) as f64;
    (p.max(0.0) * n).floor() / n
}

/// How many of `chars` characters a typing reveal shows at `p`.
pub fn typed(chars: usize, p: f64) -> usize {
    ((chars as f64) * steps(p, TYPE_STEPS)).floor() as usize
}

/// The first `n` characters of `s`.
pub fn prefix(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(b, _)| &s[..b])
}

/// A row's rise on open, `elapsed` ms after the card started opening and
/// `index` rows down: how far under its place it still sits and its opacity.
/// Past `STAGGERED` rows the delay stops growing, so a long list lands
/// within one row's clock of the last staggered one.
pub fn rise(rows: &Rows, elapsed: f64, index: usize) -> (f64, f32) {
    const STAGGERED: usize = 10;
    let delay = rows.stagger * index.min(STAGGERED) as f64;
    let p = progress(elapsed - delay, rows.clock.duration);
    let e = Curve::from_table(&rows.clock.curve).ease(p);
    (rows.rise * (1.0 - e), e.clamp(0.0, 1.0) as f32)
}

/// Whether a rise begun `elapsed` ms ago still moves any of `count` rows.
pub fn rising(rows: &Rows, elapsed: f64, count: usize) -> bool {
    elapsed < rows.stagger * count.min(10) as f64 + rows.clock.duration
}

/// The glitch swap `elapsed` ms after the content was replaced: the old
/// content's shift and opacity while it is still out, and the new one's.
pub struct Swap {
    pub old: Option<(f64, f32)>,
    pub new: (f64, f32),
}

pub fn glitch(g: &Glitch, elapsed: f64) -> Swap {
    use fs_theme::style::{GLITCH_AHEAD, GLITCH_BACK};
    if elapsed < g.out {
        let s = steps(progress(elapsed, g.out), 2);
        return Swap { old: Some((-GLITCH_BACK * s, (1.0 - s) as f32)), new: (GLITCH_AHEAD, 0.0) };
    }
    Swap { old: None, new: glitch_in(g, elapsed - g.out) }
}

/// The way in alone, for a body whose old content is already gone (a
/// panel handed over to another).
pub fn glitch_in(g: &Glitch, elapsed: f64) -> (f64, f32) {
    use fs_theme::style::GLITCH_AHEAD;
    let curve = Curve::from_table(&g.curve);
    let p = progress(elapsed, g.into);
    let alpha = curve.ease((p / 0.3).min(1.0));
    (GLITCH_AHEAD * (1.0 - curve.ease(p)), alpha.clamp(0.0, 1.0) as f32)
}

pub fn glitching(g: &Glitch, elapsed: f64) -> bool {
    elapsed < g.out + g.into
}

/// The ghost of a used row at `p`: its scale across and down, and its opacity.
pub fn ghost(u: &Use, p: f64) -> (f64, f64, f32) {
    let e = Curve::from_table(&u.clock.curve).ease(p);
    (1.0 + (u.scale.0 - 1.0) * e, 1.0 + (u.scale.1 - 1.0) * e, (1.0 - e).clamp(0.0, 1.0) as f32)
}

/// A label's axes while it shakes: weight, width in percent and tracking in
/// ems, back on `rest` at the end. CSS `ease` between each keyframe.
pub fn shake(p: f64, rest: (f64, f64, f64)) -> (f64, f64, f64) {
    const EASE: Curve = Curve::new(&[0.25, 0.1, 0.25, 1.0, 1.0, 1.0]);
    let keys = [(0.0, (500.0, 110.0, 0.07)), (0.25, (100.0, 120.0, 0.01)), (0.5, (600.0, 90.0, 0.2)), (0.75, (400.0, 120.0, 0.015)), (1.0, rest)];
    if p >= 1.0 {
        return rest;
    }
    let p = p.max(0.0);
    let i = keys.iter().rposition(|k| k.0 <= p).unwrap_or(0).min(keys.len() - 2);
    let ((t0, a), (t1, b)) = (keys[i], keys[i + 1]);
    let e = EASE.ease((p - t0) / (t1 - t0));
    let mix = |x: f64, y: f64| x + (y - x) * e;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// A picture landing at `p`: its scale, how bright it is and its opacity, or
/// `None` while it is not shown yet. The site's keyframes (20 to 3 to 1,
/// brightness 0.2 to 0.8 to 1) each run in `steps(2)`.
pub fn zoom(p: f64) -> Option<(f64, f32, f32)> {
    if p >= 1.0 {
        return Some((1.0, 1.0, 1.0));
    }
    let keys = [(20.0, 0.2, 0.0), (3.0, 0.8, 1.0), (1.0, 1.0, 1.0)];
    let half = if p < 0.5 { 0 } else { 1 };
    let s = steps((p - 0.5 * half as f64) * 2.0, 2);
    let (a, b) = (keys[half], keys[half + 1]);
    let mix = |x: f64, y: f64| x + (y - x) * s;
    let alpha = mix(a.2, b.2);
    (alpha > 0.0).then(|| (mix(a.0, b.0), mix(a.1, b.1) as f32, alpha as f32))
}

/// A progress readout as blocks: `█████░░░░░ 50%`.
pub fn blocks(fraction: f64, cells: usize, percent: i64) -> String {
    let f = fraction.clamp(0.0, 1.0);
    let full = ((f * cells as f64).round() as usize).min(cells);
    format!("{}{} {percent}%", "\u{2588}".repeat(full), "\u{2591}".repeat(cells - full))
}

/// One grain tile: grey noise at full opacity, the same every run, which a
/// surface draws at the noise's alpha.
pub fn grain(size: u16) -> Bitmap {
    let mut seed: u32 = 0x9e37_79b9;
    let mut rgba = Vec::with_capacity(size as usize * size as usize * 4);
    for _ in 0..(size as usize * size as usize) {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let v = (seed >> 24) as u8;
        rgba.extend_from_slice(&[v, v, v, 255]);
    }
    Bitmap::from_rgba(size, size, rgba)
}

/// `elapsed` over `ms`, 0 to 1, landed at once on a zero clock.
pub fn progress(elapsed: f64, ms: f64) -> f64 {
    if ms <= 0.0 { if elapsed >= 0.0 { 1.0 } else { 0.0 } } else { (elapsed / ms).clamp(0.0, 1.0) }
}

/// Milliseconds from `from` to `now`, negative when `from` is still ahead.
pub fn since(from: std::time::Instant, now: std::time::Instant) -> f64 {
    match now.checked_duration_since(from) {
        Some(d) => d.as_secs_f64() * 1000.0,
        None => -(from.duration_since(now).as_secs_f64() * 1000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_theme::style::Clock;
    use fs_theme::tokens::Easing;

    fn ease() -> Easing {
        Easing::Bezier(vec![0.22, 1.0, 0.36, 1.0, 1.0, 1.0])
    }

    #[test]
    fn steps_hold_flat_and_land_at_the_end() {
        assert_eq!(steps(0.0, 20), 0.0);
        assert_eq!(steps(0.049, 20), 0.0);
        assert_eq!(steps(0.05, 20), 0.05);
        assert_eq!(steps(0.999, 2), 0.5);
        assert_eq!(steps(1.0, 2), 1.0);
        assert_eq!(typed(10, 0.5), 5);
        assert_eq!(typed(7, 1.0), 7);
        assert_eq!(prefix("héllo", 2), "hé");
        assert_eq!(prefix("ab", 9), "ab");
    }

    #[test]
    fn rows_rise_in_order_and_land() {
        let rows = Rows { clock: Clock { duration: 900.0, curve: ease(), stagger: 0.0 }, rise: 40.0, stagger: 60.0 };
        assert_eq!(rise(&rows, 0.0, 0), (40.0, 0.0));
        let (first, _) = rise(&rows, 120.0, 0);
        let (third, a3) = rise(&rows, 120.0, 2);
        assert!(first < third && third == 40.0 && a3 == 0.0);
        assert_eq!(rise(&rows, 900.0 + 60.0 * 3.0, 3), (0.0, 1.0));
        assert!(rising(&rows, 1400.0, 12) && !rising(&rows, 1500.0, 12));
    }

    #[test]
    fn the_glitch_jumps_back_out_then_comes_in_from_ahead() {
        let g = Glitch { out: 180.0, into: 500.0, curve: ease() };
        let s = glitch(&g, 0.0);
        assert_eq!((s.old, s.new), (Some((0.0, 1.0)), (20.0, 0.0)));
        assert_eq!(glitch(&g, 100.0).old, Some((-5.0, 0.5)));
        let s = glitch(&g, 180.0);
        assert!(s.old.is_none() && s.new == (20.0, 0.0));
        let (dx, a) = glitch(&g, 180.0 + 150.0).new;
        assert!(dx > 0.0 && dx < 20.0 && a == 1.0);
        assert_eq!(glitch(&g, 680.0).new, (0.0, 1.0));
        assert!(!glitching(&g, 680.0));
    }

    #[test]
    fn the_ghost_grows_and_fades() {
        let u = Use { clock: Clock { duration: 400.0, curve: ease(), stagger: 0.0 }, scale: (1.1, 1.3) };
        assert_eq!(ghost(&u, 0.0), (1.0, 1.0, 1.0));
        let (sx, sy, a) = ghost(&u, 1.0);
        assert!((sx - 1.1).abs() < 1e-9 && (sy - 1.3).abs() < 1e-9 && a == 0.0);
    }

    #[test]
    fn the_shake_runs_its_keyframes_and_rests() {
        let rest = (400.0, 100.0, 0.0);
        assert_eq!(shake(0.0, rest), (500.0, 110.0, 0.07));
        assert_eq!(shake(0.5, rest), (600.0, 90.0, 0.2));
        assert_eq!(shake(1.0, rest), rest);
        let (w, _, _) = shake(0.125, rest);
        assert!(w < 500.0 && w > 100.0);
    }

    #[test]
    fn the_zoom_lands_in_four_stairs() {
        assert_eq!(zoom(0.0), None);
        assert_eq!(zoom(0.3), Some((11.5, 0.5, 0.5)));
        assert_eq!(zoom(0.5), Some((3.0, 0.8, 1.0)));
        assert_eq!(zoom(0.8), Some((2.0, 0.9, 1.0)));
        assert_eq!(zoom(1.0), Some((1.0, 1.0, 1.0)));
    }

    #[test]
    fn blocks_read_like_the_site() {
        assert_eq!(blocks(0.5, 10, 50), "█████░░░░░ 50%");
        assert_eq!(blocks(0.0, 10, 0), "░░░░░░░░░░ 0%");
        assert_eq!(blocks(1.2, 10, 100), "██████████ 100%");
    }

    #[test]
    fn a_zero_clock_lands_at_once() {
        assert_eq!(progress(0.0, 0.0), 1.0);
        assert_eq!(progress(-1.0, 0.0), 0.0);
        assert_eq!(progress(50.0, 100.0), 0.5);
    }
}
