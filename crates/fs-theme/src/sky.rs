//! The sky wallpaper (`wallpaper.color: "sky"`): a gradient from the zenith
//! at the top of the screen to the horizon at its foot, for where the sun
//! stands now. The elevation is cut into steps of [`STEP`] degrees and held
//! at either end of [`LOW`]..[`HIGH`], so the picture only changes through
//! the twilights and the low sun, and a whole day or night is one picture.
//!
//! Pure like `sun`: the caller passes the clock and the location, and owns
//! the timer that [`next_change`] arms.

use chrono::{DateTime, Duration, TimeZone};

use crate::sun;

/// Degrees per step: at a mid latitude about ten minutes of twilight.
pub const STEP: i32 = 2;
/// Astronomical night, past which the sky is black.
pub const LOW: i32 = -18;
/// High enough that the horizon has lost its colour.
pub const HIGH: i32 = 24;

/// Zenith and horizon in sRGB, by elevation.
const KEYS: [(f64, [u8; 3], [u8; 3]); 9] = [
    (-18.0, [0x04, 0x06, 0x0d], [0x0a, 0x0f, 0x20]),
    (-12.0, [0x07, 0x0b, 0x1d], [0x1a, 0x1f, 0x45]),
    (-8.0, [0x0f, 0x15, 0x32], [0x3a, 0x2f, 0x5e]),
    (-4.0, [0x1d, 0x25, 0x52], [0x8a, 0x52, 0x70]),
    (-1.0, [0x2a, 0x3a, 0x72], [0xd9, 0x7a, 0x5e]),
    (2.0, [0x3b, 0x55, 0x94], [0xf2, 0xa4, 0x66]),
    (6.0, [0x4a, 0x73, 0xb6], [0xf0, 0xcf, 0x9c]),
    (12.0, [0x3f, 0x80, 0xcf], [0xbc, 0xd7, 0xee]),
    (24.0, [0x2c, 0x74, 0xd4], [0xa8, 0xcd, 0xf2]),
];

/// The step an elevation falls in, in degrees.
pub fn bucket(elevation: f64) -> i32 {
    let e = elevation.clamp(LOW as f64, HIGH as f64);
    (e / STEP as f64).round() as i32 * STEP
}

/// Where the sun stands at `at`: off the location when there is one, else
/// off the fallback window's stand-in. The flag says which.
pub fn elevation<Tz: TimeZone>(at: &DateTime<Tz>, location: Option<(f64, f64)>) -> (f64, bool) {
    match location.and_then(|(lat, lon)| sun::elevation(at, lat, lon)) {
        Some(e) => (e, true),
        None => (sun::fallback_elevation(at), false),
    }
}

/// The first whole minute after `now` whose step differs from `now`'s, up
/// to a day ahead; `None` where it holds all day (a polar day or night).
pub fn next_change<Tz: TimeZone>(now: &DateTime<Tz>, location: Option<(f64, f64)>) -> Option<DateTime<Tz>> {
    let here = bucket(elevation(now, location).0);
    (1..=24 * 60).map(|m| now.clone() + Duration::minutes(m)).find(|t| bucket(elevation(t, location).0) != here)
}

fn linear(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn encode(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// Zenith and horizon in linear light for a step, between the two keys
/// either side of it.
fn ends(step: i32) -> ([f64; 3], [f64; 3]) {
    let e = step as f64;
    let lin = |c: [u8; 3]| c.map(linear);
    let i = KEYS.iter().position(|k| k.0 >= e).unwrap_or(KEYS.len() - 1);
    if i == 0 {
        return (lin(KEYS[0].1), lin(KEYS[0].2));
    }
    let (a, b) = (KEYS[i - 1], KEYS[i]);
    let t = ((e - a.0) / (b.0 - a.0)).clamp(0.0, 1.0);
    (mix(lin(a.1), lin(b.1), t), mix(lin(a.2), lin(b.2), t))
}

/// 4x4 Bayer thresholds, centred on zero, in 1/16ths of a level.
const BAYER: [[f64; 4]; 4] = [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];

/// The sky for a step as `width` by `height` RGBA, ordered-dithered across
/// one level so a dark gradient carries no bands.
pub fn paint(step: i32, width: u32, height: u32) -> Vec<u8> {
    let (zenith, horizon) = ends(step);
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let t = if h == 1 { 1.0 } else { y as f64 / (h - 1) as f64 };
        let row = mix(zenith, horizon, t.powf(1.6)).map(|c| encode(c) * 255.0);
        for x in 0..w {
            let d = (BAYER[y % 4][x % 4] + 0.5) / 16.0 - 0.5;
            for c in row {
                out.push((c + d).round().clamp(0.0, 255.0) as u8);
            }
            out.push(255);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn the_steps_hold_past_either_end() {
        assert_eq!(bucket(-40.0), LOW);
        assert_eq!(bucket(70.0), HIGH);
        assert_eq!(bucket(0.9), 0);
        assert_eq!(bucket(-3.1), -4);
        assert_eq!(bucket(5.2), 6);
    }

    fn px(img: &[u8], w: u32, x: u32, y: u32) -> [u8; 3] {
        let i = ((y * w + x) * 4) as usize;
        [img[i], img[i + 1], img[i + 2]]
    }

    /// Night is near black, a set sun leaves the horizon warmer than the
    /// zenith, and noon is blue all the way down.
    #[test]
    fn the_sky_reads_as_its_hour() {
        let (w, h) = (8, 64);
        let night = paint(LOW, w, h);
        assert!(px(&night, w, 0, h - 1).iter().all(|&c| c < 40));
        let dusk = paint(-2, w, h);
        let (top, foot) = (px(&dusk, w, 0, 0), px(&dusk, w, 0, h - 1));
        assert!(foot[0] > top[0] + 80 && top[2] > top[0], "{top:?} {foot:?}");
        let day = paint(HIGH, w, h);
        let (top, foot) = (px(&day, w, 0, 0), px(&day, w, 0, h - 1));
        assert!(top[2] > top[0] + 100 && foot[2] > foot[0], "{top:?} {foot:?}");
        assert_eq!(night.len(), (w * h * 4) as usize);
    }

    /// Amsterdam on an autumn evening: the step changes within minutes of
    /// sunset and not at all through the night that follows.
    #[test]
    fn the_next_change_waits_out_a_whole_night() {
        let loc = Some((52.37, 4.9));
        let sunset = Utc.with_ymd_and_hms(2026, 10, 30, 16, 20, 0).unwrap();
        let next = next_change(&sunset, loc).unwrap();
        assert!(next - sunset <= Duration::minutes(15), "{next}");
        let midnight = Utc.with_ymd_and_hms(2026, 10, 30, 23, 0, 0).unwrap();
        let next = next_change(&midnight, loc).unwrap();
        assert!(next - midnight >= Duration::hours(4), "{next}");
        assert!(!elevation(&midnight, None).1);
        assert!(elevation(&midnight, loc).1);
    }
}
