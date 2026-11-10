//! Pure mapping for the audio visualizer: cava's raw ASCII frames in, 0..1
//! band heights out, through a gain stage and a response curve.
//!
//! `BAR_COUNT` and `MAX_LEVEL` must match the generated cava.conf (`bars`,
//! which counts both stereo channels, so twice `BAR_COUNT`, and
//! `ascii_max_range`). Malformed input (wrong token count, non-numeric
//! values, a blank line) parses to an all-zero frame rather than failing: one
//! bad line from cava must never freeze the widget on a stale render.
//!
//! The sine in `beat_frame`'s hash is the platform libm's, not V8's, so the
//! last bit of a frame can differ from a V8 evaluation of the same hash.

use serde_json::Value;

use fs_js::{encode_uri_component, parse_number, parse_int, to_str};

/// cava's own bar count. The bar cell still reads as `CELL_BAR_COUNT` tracks
/// and downsamples.
pub const BAR_COUNT: usize = 24;
/// 1000 rather than 100: the gain stage divides by a running peak that can
/// sit near the bottom of the range on a quiet track, and at 100 steps a band
/// there only has a handful of values to move through.
pub const MAX_LEVEL: f64 = 1000.0;
pub const CELL_BAR_COUNT: usize = 6;

/// The cava.conf this contract needs. Every key is in cava 0.10.7's own
/// example config. Fixed gain rather than autosens (the gain stage below
/// supplies the height), a 12 kHz upper cutoff to pull hats into the top
/// band, light smoothing so six bars track transients, stereo so the two
/// channels keep their own frame. `source` names the PipeWire node to read,
/// "auto" for the default sink's monitor.
pub fn cava_config(source: &str) -> String {
    format!(
        "[general]\nframerate = 120\nautosens = 0\nsensitivity = 200\nbars = {}\nlower_cutoff_freq = 50\n\
         higher_cutoff_freq = 12000\nsleep_timer = 3\n\n[input]\nmethod = pipewire\nsource = {source}\n\n\
         [output]\nmethod = raw\nchannels = stereo\nraw_target = /dev/stdout\ndata_format = ascii\n\
         ascii_max_range = {}\n\n[smoothing]\nnoise_reduction = 35\nmonstercat = 1.5\n",
        BAR_COUNT * 2,
        MAX_LEVEL as i64
    )
}

/// Below this level a bar reads as silence and snaps flat. 5/1000 at cava's
/// sensitivity 200 is the same acoustic level as 2/100 at 800%.
pub(crate) const NOISE_FLOOR: f64 = 5.0;

// The gain stage. cava runs at a fixed, low sensitivity so a loud master
// never clips at its ceiling, and the height comes from here instead: every
// band is divided by `ref`, a peak follower on the frame's loudest raw band.
// Attack is 0.12s; release is a constant 20dB (10x) fall every 4s, in the log
// domain, so a quiet track after a loud one is back to full height within
// those 4s whatever the size of the drop. AGC_FLOOR caps the gain at
// AGC_TARGET / AGC_FLOOR: 0.04 sits under the quiet pink-noise passage's own
// peak at sensitivity 200 and eight times over NOISE_FLOOR.
pub(crate) const AGC_ATTACK_SECONDS: f64 = 0.12;
pub(crate) const AGC_RELEASE_SECONDS: f64 = 4.0;
pub(crate) const AGC_FLOOR: f64 = 0.04;

/// Where the running peak draws, before the knee.
pub(crate) const AGC_TARGET: f64 = 0.8;

/// Above this the height bends toward 1 and never reaches it (unit slope at
/// the knee, so no visible kink).
pub(crate) const AGC_KNEE: f64 = 0.6;

/// A power under 1 on the peak-relative level lifts the mid-levels, kept
/// milder than sqrt so neighbouring bands stay apart.
pub(crate) const RESPONSE_POWER: f64 = 0.7;

/// The next `ref`. A `ref` of 0 or less is the reset state: it snaps to the
/// first frame's own peak. A non-finite or non-positive `dt_seconds` moves
/// nothing past that snap.
pub fn agc_step(reference: f64, frame_peak: f64, dt_seconds: f64) -> f64 {
    let peak = if frame_peak.is_finite() && frame_peak > 0.0 {
        frame_peak
    } else {
        0.0
    };
    if reference.is_nan() || reference <= 0.0 {
        return peak.max(AGC_FLOOR);
    }
    let mut next = reference;
    if dt_seconds.is_finite() && dt_seconds > 0.0 {
        if peak > reference {
            next =
                reference + (peak - reference) * (1.0 - (-dt_seconds / AGC_ATTACK_SECONDS).exp());
        } else {
            next = peak.max(reference * 10f64.powf(-dt_seconds / AGC_RELEASE_SECONDS));
        }
    }
    next.max(AGC_FLOOR)
}

fn knee(z: f64) -> f64 {
    if z <= AGC_KNEE {
        return z;
    }
    let span = 1.0 - AGC_KNEE;
    AGC_KNEE + span * (1.0 - (-(z - AGC_KNEE) / span).exp())
}

/// Raw 0..1 fraction to drawn 0..1 height against the running peak `reference`.
pub fn normalize(fraction: f64, reference: f64) -> f64 {
    if fraction.is_nan() || fraction <= 0.0 {
        return 0.0;
    }
    let r = if reference > AGC_FLOOR {
        reference
    } else {
        AGC_FLOOR
    };
    knee((fraction / r).powf(RESPONSE_POWER) * AGC_TARGET)
}

pub fn frame_peak(fractions: &[f64]) -> f64 {
    let mut peak = 0.0;
    for &f in fractions {
        if f > peak {
            peak = f;
        }
    }
    peak
}

pub fn level_frame(fractions: &[f64], reference: f64) -> Vec<f64> {
    fractions.iter().map(|&f| normalize(f, reference)).collect()
}

/// All-zero levels, the bar's own dithered-track baseline.
pub fn baseline_levels() -> Vec<f64> {
    vec![0.0; BAR_COUNT]
}

/// Folds a frame down to `count` columns for the bar cell, each the peak (not
/// the average) of its own contiguous group, so a transient stays visible.
/// Groups split as evenly as possible, the remainder spread one-wide over the
/// first groups. A group with nothing in it reads as its own zero.
pub fn downsample(levels: &[f64], count: usize) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    let base = levels.len() / count;
    let remainder = levels.len() % count;
    let mut idx = 0;
    let mut result = Vec::with_capacity(count);
    for g in 0..count {
        let size = base + usize::from(g < remainder);
        let mut peak = 0.0;
        for j in 0..size {
            let v = levels[idx + j];
            if v > peak {
                peak = v;
            }
        }
        idx += size;
        result.push(peak);
    }
    result
}

// cava's frame arrives on its own clock while the screen repaints on the
// compositor's. The drawn levels are carried toward the target a little every
// screen frame: `1 - exp(-dt / tau)` is independent of dt, so a slow frame and
// a fast one converge at the same real-time rate. Quick on the way up so a
// transient reads as a hit, slower on the way down so a bar does not blink
// off between two loud frames.
pub(crate) const RISE_SECONDS: f64 = 0.03;
pub(crate) const FALL_SECONDS: f64 = 0.09;

/// A new array, `target.len()` long. A `shown` shorter than `target` reads its
/// missing entries as 0; a longer one has its extras dropped. A non-finite or
/// non-positive `dt_seconds` moves nothing.
pub fn smooth_levels(shown: &[f64], target: &[f64], dt_seconds: f64) -> Vec<f64> {
    let can_move = dt_seconds.is_finite() && dt_seconds > 0.0;
    target
        .iter()
        .enumerate()
        .map(|(i, &to)| {
            let from = shown.get(i).copied().unwrap_or(0.0);
            let start = if from.is_nan() { 0.0 } else { from };
            if !can_move {
                return start;
            }
            let tau = if to >= start {
                RISE_SECONDS
            } else {
                FALL_SECONDS
            };
            let factor = 1.0 - (-dt_seconds / tau).exp();
            start + (to - start) * factor
        })
        .collect()
}

/// One cava frame (`;`-separated) to `bar_count` integer levels; anything
/// unparsable or negative is 0.
pub(crate) fn parse_frame(line: Option<&str>, bar_count: usize) -> Vec<f64> {
    let mut levels = vec![0.0; bar_count];
    let Some(line) = line.filter(|l| !l.is_empty()) else {
        return levels;
    };
    for (slot, part) in levels.iter_mut().zip(line.split(';')) {
        let n = parse_int(part);
        *slot = if n.is_nan() || n < 0.0 { 0.0 } else { n };
    }
    levels
}

/// level 0..max to a raw 0..1 fraction, linear, clamped. The height a bar
/// draws is `normalize` of this, never this directly.
pub(crate) fn level_to_fraction(level: f64, max_level: f64) -> f64 {
    if max_level <= 0.0 || level < NOISE_FLOOR {
        return 0.0;
    }
    (level / max_level).clamp(0.0, 1.0)
}

#[derive(Debug, Clone, PartialEq)]
pub struct StereoLevels {
    pub mono: Vec<f64>,
    pub left: Vec<f64>,
    pub right: Vec<f64>,
}

/// cava's `channels = stereo` raw frame (0.10.7's cava.c, reverse = 0, no
/// horizontal_stereo): `2 * bar_count` values, the left channel's bands high
/// to low, then the right channel's low to high, so the bass meets in the
/// middle. `mono` is the per-band average of the two integer levels, so the
/// noise floor cuts it exactly where it cut the mono output.
pub fn stereo_frame_to_levels(
    line: Option<&str>,
    bar_count: usize,
    max_level: f64,
) -> StereoLevels {
    let raw = parse_frame(line, bar_count * 2);
    let mut out = StereoLevels {
        mono: Vec::with_capacity(bar_count),
        left: Vec::with_capacity(bar_count),
        right: Vec::with_capacity(bar_count),
    };
    for i in 0..bar_count {
        let l = raw[bar_count - 1 - i];
        let r = raw[bar_count + i];
        out.left.push(level_to_fraction(l, max_level));
        out.right.push(level_to_fraction(r, max_level));
        out.mono.push(level_to_fraction((l + r) / 2.0, max_level));
    }
    out
}

/// A source whose audio never reaches this machine's PipeWire graph (the
/// iPhone over AMS) has nothing for cava to hear, so a frame is drawn off the
/// track's tempo instead. Deezer's free API carries a `bpm` per track, 0 when
/// it has none; this covers that and a miss.
pub(crate) const FALLBACK_BPM: f64 = 120.0;

pub fn deezer_search_url(artist: &str, title: &str) -> String {
    format!(
        "https://api.deezer.com/search?limit=1&q={}",
        encode_uri_component(&format!("{title} {artist}"))
    )
}

pub fn deezer_track_url(id: &str) -> String {
    format!("https://api.deezer.com/track/{}", encode_uri_component(id))
}

/// The first hit's id, or "" for no hit or a body that isn't the search shape.
pub fn parse_deezer_search(body: &str) -> String {
    let Ok(doc) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    match doc
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(|hit| hit.get("id"))
    {
        Some(id) => to_str(id),
        None => String::new(),
    }
}

/// The track's bpm, or 0 when Deezer has none or the body is not a track.
pub fn parse_deezer_bpm(body: &str) -> f64 {
    let Ok(doc) = serde_json::from_str::<Value>(body) else {
        return 0.0;
    };
    let bpm = match doc.get("bpm") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => parse_number(s),
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        _ => f64::NAN,
    };
    if bpm.is_finite() && (40.0..=250.0).contains(&bpm) {
        bpm
    } else {
        0.0
    }
}

/// A stable 0..1 value per integer, so a given beat or sixteenth always draws
/// the same way and a paused track holds its frame.
fn hash(n: f64) -> f64 {
    let x = (n * 12.9898).sin() * 43758.5453;
    x - x.floor()
}

/// One `count`-long 0..1 frame at `seconds` into the track, built like a drum
/// pattern over a spectrum: a kick on every beat in the low bands, a snare on
/// beats two and four in the mids, hats on every sixteenth in the highs, each
/// hit a sharp attack and a fast decay at its own random strength, plus
/// per-band jitter that changes every sixteenth. `channel` (0 mix, -1 left, 1
/// right) reseeds the randomness so the stereo style's halves differ.
pub fn beat_frame(seconds: f64, bpm: f64, count: usize, channel: i32) -> Vec<f64> {
    let tempo = if bpm > 0.0 { bpm } else { FALLBACK_BPM };
    let beats = seconds.max(0.0) * tempo / 60.0;
    let beat = beats.floor();
    let phase = beats - beat;
    let sixteenths = beats * 4.0;
    let sixteenth = sixteenths.floor();
    let sixteenth_phase = sixteenths - sixteenth;
    let seed = f64::from(channel) * 7919.0;
    let kick = (-phase * 10.0).exp() * (0.7 + 0.3 * hash(beat + seed));
    let snare = if beat % 2.0 == 1.0 {
        (-phase * 8.0).exp() * (0.8 + 0.2 * hash(beat * 3.0 + seed))
    } else {
        0.0
    };
    let hat = (-sixteenth_phase * 18.0).exp() * (0.5 + 0.5 * hash(sixteenth * 5.0 + seed));
    (0..count)
        .map(|i| {
            let fi = i as f64;
            let x = if count > 1 {
                fi / (count - 1) as f64
            } else {
                0.0
            };
            let low = (1.0 - x * 2.5).max(0.0).powf(1.2);
            let mid = (-((x - 0.45) / 0.18).powi(2)).exp();
            let high = x.powf(1.5);
            let jitter = hash(sixteenth * 31.0 + fi + seed);
            let drift = 0.5 + 0.5 * (seconds * (1.1 + fi * 0.29) + fi * 1.7).sin();
            let v = 0.04
                + 0.9 * low * kick
                + 0.75 * mid * snare
                + 0.7 * high * hat
                + 0.12 * jitter
                + 0.06 * drift;
            v.clamp(0.0, 1.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_is_bar_count_zeros() {
        let b = baseline_levels();
        assert_eq!(b.len(), BAR_COUNT);
        assert!(b.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn parse_frame_splits_on_semicolon() {
        assert_eq!(parse_frame(Some("10;20;30"), 3), [10.0, 20.0, 30.0]);
    }

    #[test]
    fn parse_frame_pads_short_lines_with_zero() {
        assert_eq!(parse_frame(Some("10;20"), 5), [10.0, 20.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn parse_frame_ignores_extra_tokens() {
        assert_eq!(parse_frame(Some("1;2;3;4;5"), 3), [1.0, 2.0, 3.0]);
    }

    #[test]
    fn parse_frame_treats_non_numeric_tokens_as_zero() {
        assert_eq!(parse_frame(Some("12;abc;7"), 3), [12.0, 0.0, 7.0]);
    }

    #[test]
    fn parse_frame_treats_negative_values_as_zero() {
        assert_eq!(parse_frame(Some("-5;3"), 2), [0.0, 3.0]);
    }

    #[test]
    fn parse_frame_handles_empty_line() {
        assert_eq!(parse_frame(Some(""), 4), [0.0; 4]);
    }

    #[test]
    fn parse_frame_handles_undefined_line() {
        assert_eq!(parse_frame(None, 4), [0.0; 4]);
    }

    #[test]
    fn parse_frame_handles_garbage_line() {
        assert_eq!(parse_frame(Some("not cava output at all"), 2), [0.0, 0.0]);
    }

    #[test]
    fn level_to_fraction_zero_is_empty() {
        assert_eq!(level_to_fraction(0.0, 100.0), 0.0);
    }

    #[test]
    fn level_to_fraction_max_is_full() {
        assert_eq!(level_to_fraction(100.0, 100.0), 1.0);
    }

    // Linear: the curve lives in normalize, after the gain stage.
    #[test]
    fn level_to_fraction_is_linear() {
        assert_eq!(level_to_fraction(250.0, 1000.0), 0.25);
    }

    // Near-silence is flat (zero fill), not a jittering bottom pixel, which
    // is what cava's deprecated `ignore` knob did.
    #[test]
    fn level_to_fraction_snaps_below_noise_floor_to_empty() {
        assert_eq!(level_to_fraction(NOISE_FLOOR - 1.0, MAX_LEVEL), 0.0);
        assert_ne!(level_to_fraction(NOISE_FLOOR, MAX_LEVEL), 0.0);
    }

    #[test]
    fn level_to_fraction_clamps_values_above_max() {
        // cava can still overshoot ascii_max_range on a transient even with
        // autosens off.
        assert_eq!(level_to_fraction(5000.0, 1000.0), 1.0);
    }

    #[test]
    fn level_to_fraction_clamps_negative_values() {
        assert_eq!(level_to_fraction(-10.0, 100.0), 0.0);
    }

    #[test]
    fn level_to_fraction_handles_zero_max_without_dividing_by_zero() {
        assert_eq!(level_to_fraction(5.0, 0.0), 0.0);
    }

    // cava writes the left channel high to low, then the right low to high,
    // with a trailing delimiter before the newline.
    #[test]
    fn stereo_frame_splits_the_mirrored_halves_into_channels() {
        let s = stereo_frame_to_levels(Some("40;30;20;10;60;70;80;100;"), 4, 100.0);
        assert_eq!(s.left, [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(s.right, [0.6, 0.7, 0.8, 1.0]);
        assert_eq!(s.mono, [0.35, 0.45, 0.55, 0.7]);
    }

    #[test]
    fn stereo_frame_mono_is_the_average_before_the_noise_floor() {
        let n = NOISE_FLOOR;
        let line = [n - 3.0, 0.0, 0.0, n + 1.0]
            .map(|v| v.to_string())
            .join(";");
        let s = stereo_frame_to_levels(Some(&line), 2, MAX_LEVEL);
        assert_eq!(s.left, [0.0, 0.0]);
        assert_eq!(s.right, [0.0, (n + 1.0) / MAX_LEVEL]);
        assert_eq!(s.mono, [0.0, 0.0]);
    }

    #[test]
    fn stereo_frame_tolerates_short_and_malformed_lines() {
        let s = stereo_frame_to_levels(Some("garbage;;"), BAR_COUNT, MAX_LEVEL);
        assert_eq!(s.mono, baseline_levels());
        assert_eq!(s.left, baseline_levels());
        assert_eq!(s.right, baseline_levels());
        let u = stereo_frame_to_levels(None, 3, 100.0);
        assert_eq!(u.left.len(), 3);
        assert_eq!(u.right, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn downsample_takes_the_peak_of_each_group() {
        let levels = [
            1.0, 2.0, 3.0, 4.0, 9.0, 1.0, 2.0, 3.0, 5.0, 6.0, 7.0, 8.0, 1.0, 1.0, 1.0, 1.0, 9.0,
            1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 5.0,
        ];
        assert_eq!(downsample(&levels, 6), [4.0, 9.0, 8.0, 1.0, 9.0, 5.0]);
    }

    #[test]
    fn downsample_short_input_pads_with_zeros() {
        assert_eq!(downsample(&[3.0, 7.0], 6), [3.0, 7.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn downsample_empty_input_is_all_zeros() {
        assert_eq!(downsample(&[], 6), [0.0; 6]);
    }

    #[test]
    fn downsample_count_equal_to_length_is_identity() {
        let levels = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(downsample(&levels, 6), levels);
    }

    #[test]
    fn smooth_levels_moves_toward_the_target() {
        let result = smooth_levels(&[0.0], &[1.0], RISE_SECONDS);
        assert!(result[0] > 0.0);
        assert!(result[0] < 1.0);
    }

    #[test]
    fn smooth_levels_rises_faster_than_it_falls() {
        let dt = 0.02;
        let rising = smooth_levels(&[0.0], &[1.0], dt);
        let falling = smooth_levels(&[1.0], &[0.0], dt);
        assert!(rising[0] > 1.0 - falling[0]);
    }

    #[test]
    fn smooth_levels_zero_dt_does_not_move() {
        assert_eq!(smooth_levels(&[0.5], &[1.0], 0.0), [0.5]);
    }

    #[test]
    fn smooth_levels_non_finite_dt_does_not_move() {
        assert_eq!(smooth_levels(&[0.5], &[1.0], f64::NAN), [0.5]);
        assert_eq!(smooth_levels(&[0.5], &[1.0], -1.0), [0.5]);
    }

    #[test]
    fn smooth_levels_converges_within_half_a_second_of_16ms_steps() {
        let mut shown = vec![0.0];
        let target = [1.0];
        let steps = (0.5_f64 / 0.016).round() as usize;
        for _ in 0..steps {
            shown = smooth_levels(&shown, &target, 0.016);
        }
        assert!((shown[0] - 1.0).abs() < 0.01);
    }

    #[test]
    fn smooth_levels_reconciles_lengths_to_the_target() {
        assert_eq!(smooth_levels(&[1.0, 2.0, 3.0], &[0.0, 0.0], 0.03).len(), 2);
        assert_eq!(smooth_levels(&[1.0], &[0.0, 0.0, 0.0], 0.03).len(), 3);
        // A missing shown value starts from 0, not undefined.
        let grown = smooth_levels(&[1.0], &[1.0, 1.0], RISE_SECONDS);
        assert!(grown[1] > 0.0);
    }

    #[test]
    fn smooth_levels_empty_target_is_empty() {
        assert!(smooth_levels(&[1.0, 2.0, 3.0], &[], 0.03).is_empty());
    }

    // Two windows syncing at different refresh rates feed this different,
    // irregular dt every call; the factor has to be a function of dt alone.
    #[test]
    fn smooth_levels_is_time_invariant_not_frame_count_invariant() {
        let two_steps = smooth_levels(&smooth_levels(&[0.0], &[1.0], 0.008), &[1.0], 0.008);
        let one_step = smooth_levels(&[0.0], &[1.0], 0.016);
        assert!((two_steps[0] - one_step[0]).abs() < 1e-6);
    }

    #[test]
    fn smooth_levels_large_dt_moves_nearly_all_the_way() {
        // dt this far past RISE_SECONDS (0.1s against a 0.03s time constant)
        // should land past 95% of the way there.
        assert!(smooth_levels(&[0.0], &[1.0], 0.1)[0] > 0.95);
    }

    // Runs `seconds` of identical frames at cava's own 120fps through the gain
    // stage and returns the running peak it settles on.
    fn run(mut reference: f64, frame: &[f64], seconds: f64) -> f64 {
        let dt = 1.0 / 120.0;
        let steps = (seconds / dt).round() as usize;
        for _ in 0..steps {
            reference = agc_step(reference, frame_peak(frame), dt);
        }
        reference
    }

    fn scaled(shape: &[f64], gain: f64) -> Vec<f64> {
        shape.iter().map(|v| v * gain).collect()
    }

    // The complaint: a louder track or a raised volume pegged every column.
    // The same passage at three input gains settles on the same drawn height,
    // and under 0.9.
    #[test]
    fn agc_levels_one_passage_at_any_gain_to_the_same_height() {
        let shape = [0.2, 0.5, 1.0, 0.7, 0.3];
        let gains = [0.1, 0.3, 0.9];
        let heights: Vec<f64> = gains
            .iter()
            .map(|&g| {
                let frame = scaled(&shape, g);
                let r = run(0.0, &frame, 2.0);
                frame_peak(&level_frame(&frame, r))
            })
            .collect();
        for (i, h) in heights.iter().enumerate() {
            assert!(*h < 0.9, "gain {} drew {h}", gains[i]);
            assert!(*h > 0.67, "gain {} drew {h}", gains[i]);
            assert!((h - heights[0]).abs() < 1e-6);
        }
    }

    #[test]
    fn agc_quiet_passage_after_a_loud_one_recovers_within_the_release_time() {
        let loud = [0.8, 0.4];
        let quiet = [0.08, 0.04];
        let mut r = run(0.0, &loud, 2.0);
        let steady = frame_peak(&level_frame(&loud, r));
        let halfway = run(r, &quiet, AGC_RELEASE_SECONDS / 2.0);
        assert!(frame_peak(&level_frame(&quiet, halfway)) < steady - 0.1);
        r = run(r, &quiet, AGC_RELEASE_SECONDS + 0.1);
        assert!((frame_peak(&level_frame(&quiet, r)) - steady).abs() < 0.01);
    }

    #[test]
    fn agc_floor_keeps_a_band_just_over_the_noise_floor_low() {
        let frame = [NOISE_FLOOR / MAX_LEVEL];
        let r = run(0.0, &frame, 5.0);
        assert!(level_frame(&frame, r)[0] < 0.2);
    }

    #[test]
    fn agc_transient_over_the_running_peak_draws_taller_than_the_steady_level() {
        let steady_frame = [0.3, 0.15];
        let mut r = run(0.0, &steady_frame, 2.0);
        let steady = level_frame(&steady_frame, r)[0];
        let hit_frame = [0.9, 0.15];
        r = agc_step(r, frame_peak(&hit_frame), 1.0 / 120.0);
        let hit = level_frame(&hit_frame, r)[0];
        assert!(hit > steady);
        assert!(hit < 1.0);
    }

    #[test]
    fn agc_preserves_band_order_and_contrast() {
        let frame = [0.05, 0.1, 0.2, 0.4];
        let r = run(0.0, &frame, 2.0);
        let drawn = level_frame(&frame, r);
        for i in 1..drawn.len() {
            assert!(drawn[i] - drawn[i - 1] > 0.1, "band {i}: {drawn:?}");
        }
    }

    #[test]
    fn agc_step_from_reset_snaps_to_the_first_peak() {
        assert_eq!(agc_step(0.0, 0.5, 1.0 / 120.0), 0.5);
        assert_eq!(agc_step(0.0, 0.0, 1.0 / 120.0), AGC_FLOOR);
    }

    #[test]
    fn agc_step_does_not_move_on_a_bad_dt() {
        assert_eq!(agc_step(0.5, 0.9, 0.0), 0.5);
        assert_eq!(agc_step(0.5, 0.1, f64::NAN), 0.5);
    }

    #[test]
    fn normalize_approaches_one_and_never_reaches_it() {
        let h = normalize(1.0, 0.1);
        assert!(h > 0.99);
        assert!(h < 1.0);
        assert_eq!(normalize(0.0, 0.1), 0.0);
    }

    #[test]
    fn deezer_search_url_encodes_the_query() {
        assert_eq!(
            deezer_search_url("Daft Punk", "Get Lucky & Co"),
            "https://api.deezer.com/search?limit=1&q=Get%20Lucky%20%26%20Co%20Daft%20Punk"
        );
    }

    #[test]
    fn parse_deezer_search_takes_the_first_id() {
        assert_eq!(
            parse_deezer_search(r#"{"data":[{"id":67238735},{"id":1}]}"#),
            "67238735"
        );
        assert_eq!(parse_deezer_search(r#"{"data":[]}"#), "");
        assert_eq!(parse_deezer_search("not json"), "");
    }

    #[test]
    fn parse_deezer_bpm_reads_zero_as_unknown() {
        assert_eq!(parse_deezer_bpm(r#"{"bpm":116.1}"#), 116.1);
        assert_eq!(parse_deezer_bpm(r#"{"bpm":0}"#), 0.0);
        assert_eq!(parse_deezer_bpm(r#"{"error":{}}"#), 0.0);
    }

    #[test]
    fn beat_frame_peaks_on_the_beat_in_the_low_bands() {
        let on_beat = beat_frame(2.0, 120.0, 24, 0);
        let between = beat_frame(2.2, 120.0, 24, 0);
        assert_eq!(on_beat.len(), 24);
        assert!(on_beat[0] > between[0] + 0.3);
        assert!(on_beat.iter().all(|v| (0.0..=1.0).contains(v)));
    }

    #[test]
    fn beat_frame_hits_the_high_bands_on_the_off_beat() {
        let off_beat = beat_frame(2.25, 120.0, 24, 0);
        let before = beat_frame(2.2, 120.0, 24, 0);
        assert!(off_beat[23] > before[23]);
    }

    #[test]
    fn beat_frame_falls_back_without_a_bpm() {
        assert_eq!(
            beat_frame(3.3, 0.0, 24, 0),
            beat_frame(3.3, FALLBACK_BPM, 24, 0)
        );
    }

    #[test]
    fn beat_frame_channels_differ() {
        assert_ne!(
            beat_frame(1.3, 120.0, 24, -1),
            beat_frame(1.3, 120.0, 24, 1)
        );
    }
}
