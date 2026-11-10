//! The media panel's spectrum styles, drawn into the box twelve spectrum
//! columns occupy (~94x32 at the shipped tokens). Every style reads `levels`
//! (0..1 per cava band) and nothing else: a silent frame draws the style's
//! resting state, and anything that moves over time (a falling cap, a
//! particle, a scrolling trace) is only ever set moving by a level.
//!
//! Each style calls a `Scene` the way a canvas 2D context is called, so the
//! result is a list of `Shape`s for a renderer to walk.
//!
//! `State` belongs to one renderer and is thrown away when the style changes
//! or cava stops. `dt` is 0 on a repaint that is not a new frame (a colour
//! change, a resize): a style then redraws what it has without advancing or
//! spawning anything. Randomness comes from the state's own generator, so a
//! seeded state replays.
//!
//! A style whose column count changed under a live state (`ink.columns`)
//! restarts its buffers rather than reading past the old ones.

// The loops index several parallel buffers by one counter, and
// the 6.28 and 6.283 the particle styles seed their phases with are values, not
// approximations of TAU.
#![allow(clippy::needless_range_loop, clippy::approx_constant)]

use std::f64::consts::PI;

use fs_media::visualizer::model::BAR_COUNT;
use fs_media::visualizer::style::Style;
use fs_theme::color::Rgba;
use super::band::{Band, LEVEL_ACCENT_FROM, LEVEL_DIM_BELOW, level_color_band};
use super::scene::{Scene, TextAlign, TextBaseline};
use fs_js::round;

/// Frames past this are a stall (a hidden window, a suspended session), and
/// stepping a whole stall at once would drop every particle and cap at once.
pub const MAX_DT: f64 = 0.1;

#[derive(Debug, Clone, PartialEq)]
pub struct Ink {
    pub groove: Rgba,
    pub dim: Rgba,
    pub content: Rgba,
    pub accent: Rgba,
    /// The mono font family.
    pub mono: String,
    pub columns: usize,
    pub gap: f64,
    pub radius: f64,
}

/// A fixed pool per style: a spawn with every slot taken is dropped, so a loud
/// passage can never grow the work a frame does.
#[derive(Debug, Clone)]
struct Pool {
    alive: Vec<bool>,
    x: Vec<f64>,
    y: Vec<f64>,
    v: Vec<f64>,
    r: Vec<f64>,
    a: Vec<f64>,
    c: Vec<f64>,
}

impl Pool {
    fn new(size: usize) -> Pool {
        Pool {
            alive: vec![false; size],
            x: vec![0.0; size],
            y: vec![0.0; size],
            v: vec![0.0; size],
            r: vec![0.0; size],
            a: vec![0.0; size],
            c: vec![0.0; size],
        }
    }

    fn spawn_slot(&self) -> Option<usize> {
        self.alive.iter().position(|a| !a)
    }
}

/// A per-style ring buffer of one value per sample, oldest first on read.
#[derive(Debug, Clone)]
struct History {
    v: Vec<f64>,
    w: Vec<f64>,
    head: usize,
    acc: f64,
}

impl History {
    fn push(&mut self, v: f64, w: f64) {
        self.v[self.head] = v;
        self.w[self.head] = w;
        self.head = (self.head + 1) % self.v.len();
    }
}

#[derive(Debug, Clone)]
pub struct State {
    rng: fastrand::Rng,
    cap_pos: Vec<f64>,
    cap_hold: Vec<f64>,
    cap_vel: Vec<f64>,
    pool: Option<Pool>,
    hist: Option<History>,
    spawn_acc: Vec<f64>,
    flick: Vec<f64>,
    heads: Vec<f64>,
    phase: Vec<f64>,
    bin_off: Vec<f64>,
    tile_v: Vec<f64>,
    sand: Vec<f64>,
    rk_alive: Vec<bool>,
    rk_x: Vec<f64>,
    rk_y: Vec<f64>,
    rk_top: Vec<f64>,
    rk_l: Vec<f64>,
    acc: f64,
    avg: f64,
    since: Option<f64>,
    beat: Option<f64>,
    beat_amp: f64,
    on_avg: f64,
    on_since: Option<f64>,
    tick: f64,
    scroll: f64,
    drift: f64,
    clock: f64,
    yaw: Option<f64>,
    tumble: f64,
    stars: f64,
    grains: Option<f64>,
    draining: bool,
}

impl Default for State {
    fn default() -> Self {
        State::with_rng(fastrand::Rng::new())
    }
}

impl State {
    /// A state whose random draws replay.
    #[cfg(test)]
    pub(crate) fn with_seed(seed: u64) -> State {
        State::with_rng(fastrand::Rng::with_seed(seed))
    }

    fn with_rng(rng: fastrand::Rng) -> State {
        State {
            rng,
            cap_pos: Vec::new(),
            cap_hold: Vec::new(),
            cap_vel: Vec::new(),
            pool: None,
            hist: None,
            spawn_acc: Vec::new(),
            flick: Vec::new(),
            heads: Vec::new(),
            phase: Vec::new(),
            bin_off: Vec::new(),
            tile_v: Vec::new(),
            sand: Vec::new(),
            rk_alive: Vec::new(),
            rk_x: Vec::new(),
            rk_y: Vec::new(),
            rk_top: Vec::new(),
            rk_l: Vec::new(),
            acc: 0.0,
            avg: 0.0,
            since: None,
            beat: None,
            beat_amp: 0.0,
            on_avg: 0.0,
            on_since: None,
            tick: 0.0,
            scroll: 0.0,
            drift: 0.0,
            clock: 0.0,
            yaw: None,
            tumble: 0.0,
            stars: 0.0,
            grains: None,
            draining: false,
        }
    }
}

/// Everything one frame of one style reads.
struct Frame<'a> {
    w: f64,
    h: f64,
    levels: &'a [f64],
    ink: &'a Ink,
    dt: f64,
    left: &'a [f64],
    right: &'a [f64],
}

type DrawFn = fn(&mut Scene, &Frame, &mut State);

fn draw_fn(style: Style) -> DrawFn {
    match style {
        Style::Bars => bars,
        Style::Peaks => peaks,
        Style::Led => led,
        Style::Mirror => mirror,
        Style::Butterfly => butterfly,
        Style::Outline => outline,
        Style::Wave => wave,
        Style::Dots => dots,
        Style::Ascii => ascii,
        Style::Matrix => matrix,
        Style::Rain => rain,
        Style::Flame => flame,
        Style::Bubbles => bubbles,
        Style::Scope => scope,
        Style::Pulse => pulse,
        Style::Heartbeat => heartbeat,
        Style::Terrain => terrain,
        Style::Bricks => bricks,
        Style::Columns => columns,
        Style::Scatter => scatter,
        Style::Retro => retro,
        Style::Binary => binary,
        Style::Sakura => sakura,
        Style::Firework => firework,
        Style::Firefly => firefly,
        Style::Mosaic => mosaic,
        Style::Sand => sand,
        Style::Geyser => geyser,
        Style::Stereo => stereo,
        Style::Redsector => redsector,
        Style::Stipple => stipple,
    }
}

/// Draws one frame of `style` into `scene` over
/// a `w` x `h` box. `left` and `right` are the same bands per channel; only
/// `stereo` reads them, and it falls back to `levels` for both.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    style: Style,
    scene: &mut Scene,
    w: f64,
    h: f64,
    levels: &[f64],
    state: &mut State,
    ink: &Ink,
    dt: f64,
    left: Option<&[f64]>,
    right: Option<&[f64]>,
) {
    let f = draw_fn(style);
    let dt = if dt.is_finite() && dt > 0.0 {
        dt.min(MAX_DT)
    } else {
        0.0
    };
    if w.is_nan() || w <= 0.0 || h.is_nan() || h <= 0.0 {
        return;
    }
    let frame = Frame {
        w,
        h,
        levels,
        ink,
        dt,
        left: left.unwrap_or(levels),
        right: right.unwrap_or(levels),
    };
    f(scene, &frame, state);
    scene.global_alpha = 1.0;
}

fn buf(v: &mut Vec<f64>, n: usize) {
    if v.len() != n {
        *v = vec![0.0; n];
    }
}

/// Peak of each contiguous group, like `model::downsample`, clamped so a
/// malformed level cannot draw outside the box. NaN and negative levels read
/// as 0.
fn fold(levels: &[f64], count: usize) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    let base = levels.len() / count;
    let rem = levels.len() % count;
    let mut idx = 0;
    let mut out = Vec::with_capacity(count);
    for g in 0..count {
        let size = base + usize::from(g < rem);
        let mut peak = 0.0;
        for j in 0..size {
            let v = levels[idx + j];
            if v > peak {
                peak = v;
            }
        }
        idx += size;
        out.push(if peak > 1.0 { 1.0 } else { peak });
    }
    out
}

fn band(ink: &Ink, l: f64) -> Rgba {
    match level_color_band(l) {
        Band::Dim => ink.dim,
        Band::Content => ink.content,
        Band::Accent => ink.accent,
    }
}

fn col_width(w: f64, count: usize, gap: f64) -> f64 {
    (w - (count as f64 - 1.0) * gap) / count as f64
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn pill(c: &mut Scene, x: f64, y: f64, cw: f64, ch: f64, r: f64) {
    if ch.is_nan() || ch <= 0.0 || cw.is_nan() || cw <= 0.0 {
        return;
    }
    let rr = r.min(cw / 2.0).min(ch / 2.0);
    c.begin_path();
    c.rounded_rect(x, y, cw, ch, rr);
    c.fill();
}

fn max_of(a: &[f64], from: usize, to: usize) -> f64 {
    let mut m = 0.0;
    for &v in &a[from..to] {
        if v > m {
            m = v;
        }
    }
    m
}

fn mean(a: &[f64], from: usize, to: usize) -> f64 {
    if to > from {
        a[from..to].iter().sum::<f64>() / (to - from) as f64
    } else {
        0.0
    }
}

// The Winamp cap: it rides a rising column, holds at the top, then falls under
// gravity until it lands on the column again.
const CAP_HOLD_SECONDS: f64 = 0.3;
const CAP_GRAVITY: f64 = 2.6;

fn caps(state: &mut State, lv: &[f64], n: usize, dt: f64) -> Vec<f64> {
    buf(&mut state.cap_pos, n);
    buf(&mut state.cap_hold, n);
    buf(&mut state.cap_vel, n);
    for i in 0..n {
        let l = lv[i];
        if l >= state.cap_pos[i] {
            state.cap_pos[i] = l;
            state.cap_hold[i] = CAP_HOLD_SECONDS;
            state.cap_vel[i] = 0.0;
        } else if state.cap_hold[i] > 0.0 {
            state.cap_hold[i] = (state.cap_hold[i] - dt).max(0.0);
        } else {
            state.cap_vel[i] += CAP_GRAVITY * dt;
            state.cap_pos[i] = l.max(state.cap_pos[i] - state.cap_vel[i] * dt);
        }
    }
    state.cap_pos.clone()
}

fn history(state: &mut State, n: usize) {
    if state.hist.as_ref().is_none_or(|h| h.v.len() != n) {
        state.hist = Some(History {
            v: vec![0.0; n],
            w: vec![0.0; n],
            head: 0,
            acc: 0.0,
        });
    }
}

fn floor_line(c: &mut Scene, f: &Frame) {
    c.fill_style = f.ink.groove;
    c.fill_rect(0.0, f.h - 1.0, f.w, 1.0);
}

fn bars(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let r = ink.radius.min(cw / 2.0);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        c.fill_style = ink.groove;
        pill(c, x, 0.0, cw, h, r);
        let l = lv[i];
        if l > 0.0 {
            let fh = h.min((h * l).max(r * 2.0));
            c.fill_style = band(ink, l);
            pill(c, x, h - fh, cw, fh, r);
        }
    }
}

fn peaks(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = if !f.levels.is_empty() {
        f.levels.len().min(24)
    } else {
        24
    };
    let lv = fold(f.levels, n);
    let caps = caps(state, &lv, n, f.dt);
    let cw = col_width(w, n, ink.gap);
    let cap_h = 2.0;
    let usable = h - cap_h - 1.0;
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let l = lv[i];
        if l > 0.0 {
            let bh = l * usable;
            c.fill_style = band(ink, l);
            c.fill_rect(x, h - bh, cw, bh);
        }
        c.fill_style = if caps[i] > 0.02 {
            ink.accent
        } else {
            ink.groove
        };
        c.fill_rect(x, h - caps[i] * usable - 1.0 - cap_h, cw, cap_h);
    }
}

fn led(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let caps = caps(state, &lv, n, f.dt);
    let cw = col_width(w, n, ink.gap);
    let seg_h = 2.0;
    let gap_y = 1.0;
    let segs = (((h + gap_y) / (seg_h + gap_y)).floor()).max(1.0);
    let top = h - (segs * (seg_h + gap_y) - gap_y);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let lit = round(lv[i] * segs);
        let cap = round(caps[i] * segs) - 1.0;
        let mut k = 0.0;
        while k < segs {
            let y = h - (k + 1.0) * seg_h - k * gap_y;
            if y < top {
                break;
            }
            if k < lit {
                c.fill_style = band(ink, (k + 1.0) / segs);
            } else if k == cap {
                c.fill_style = ink.accent;
            } else {
                c.fill_style = ink.groove;
            }
            c.fill_rect(x, y, cw, seg_h);
            k += 1.0;
        }
    }
}

fn mirror(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let r = ink.radius.min(cw / 2.0);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        c.fill_style = ink.groove;
        pill(c, x, 0.0, cw, h, r);
        let l = lv[i];
        if l > 0.0 {
            let fh = h.min((h * l).max(r * 2.0));
            c.fill_style = band(ink, l);
            pill(c, x, (h - fh) / 2.0, cw, fh, r);
        }
    }
}

fn butterfly(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let half = n.div_ceil(2);
    let lv = fold(f.levels, half);
    let cw = col_width(w, n, ink.gap);
    let r = ink.radius.min(cw / 2.0);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let band_index: i64 = if (i as f64) < n as f64 / 2.0 {
            (n / 2) as i64 - 1 - i as i64
        } else {
            i as i64 - (n / 2) as i64
        };
        let l = lv[band_index.clamp(0, half as i64 - 1) as usize];
        c.fill_style = ink.groove;
        pill(c, x, 0.0, cw, h, r);
        if l > 0.0 {
            let fh = h.min((h * l).max(r * 2.0));
            c.fill_style = band(ink, l);
            pill(c, x, (h - fh) / 2.0, cw, fh, r);
        }
    }
}

fn outline(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let lw = 1.0;
    let cap_h = 2.0;
    c.stroke_style = ink.dim;
    c.line_width = lw;
    c.begin_path();
    for i in 0..n {
        let x0 = i as f64 * (cw + ink.gap);
        let y = clamp(h - lv[i] * h, cap_h / 2.0, h - cap_h / 2.0);
        let left = if i == 0 { 0.0 } else { x0 - ink.gap / 2.0 };
        let right = if i == n - 1 {
            w
        } else {
            x0 + cw + ink.gap / 2.0
        };
        if i == 0 {
            c.move_to(left, y);
        } else {
            c.line_to(left, y);
        }
        c.line_to(right, y);
    }
    c.stroke();
    for j in 0..n {
        let x = j as f64 * (cw + ink.gap);
        let l = lv[j];
        c.fill_style = if l > 0.0 { band(ink, l) } else { ink.groove };
        c.fill_rect(x, clamp(h - l * h, 0.0, h - cap_h), cw, cap_h);
    }
}

fn wave(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = if f.levels.len() > 1 {
        f.levels.len().min(24)
    } else {
        24
    };
    let lv = fold(f.levels, n);
    let lw = 1.5;
    let peak = max_of(&lv, 0, n);
    c.begin_path();
    c.move_to(0.0, h);
    let mut px = 0.0;
    let mut py = clamp(h - lv[0] * h, lw / 2.0, h - lw / 2.0);
    c.line_to(px, py);
    for i in 1..n {
        let x = i as f64 / (n - 1) as f64 * w;
        let y = clamp(h - lv[i] * h, lw / 2.0, h - lw / 2.0);
        c.quad_to(px, py, (px + x) / 2.0, (py + y) / 2.0);
        px = x;
        py = y;
    }
    c.line_to(px, py);
    c.line_to(w, h);
    c.close_path();
    c.global_alpha = 0.3;
    c.fill_style = if peak > 0.0 {
        band(ink, peak)
    } else {
        ink.groove
    };
    c.fill();
    c.global_alpha = 1.0;

    c.begin_path();
    px = 0.0;
    py = clamp(h - lv[0] * h, lw / 2.0, h - lw / 2.0);
    c.move_to(px, py);
    for j in 1..n {
        let x2 = j as f64 / (n - 1) as f64 * w;
        let y2 = clamp(h - lv[j] * h, lw / 2.0, h - lw / 2.0);
        c.quad_to(px, py, (px + x2) / 2.0, (py + y2) / 2.0);
        px = x2;
        py = y2;
    }
    c.line_to(px, py);
    c.line_width = lw;
    c.stroke_style = if peak > 0.0 { ink.content } else { ink.dim };
    c.stroke();
}

fn dots(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let size = 2.0;
    let pitch = 4.0;
    let cols = ((w + pitch - size) / pitch).floor().max(1.0) as usize;
    let rows = ((h + pitch - size) / pitch).floor().max(1.0) as usize;
    let lv = fold(f.levels, cols);
    let ox = (w - ((cols as f64 - 1.0) * pitch + size)) / 2.0;
    let oy = (h - ((rows as f64 - 1.0) * pitch + size)) / 2.0;
    for col in 0..cols {
        let l = lv[col];
        let lit = round(l * rows as f64);
        let colour = band(ink, l);
        for k in 0..rows {
            c.fill_style = if (k as f64) < lit { colour } else { ink.groove };
            c.fill_rect(
                ox + col as f64 * pitch,
                oy + (rows - 1 - k) as f64 * pitch,
                size,
                size,
            );
        }
    }
}

// Light, medium and dark shade, then the full block.
const SHADES: [&str; 4] = ["\u{2591}", "\u{2592}", "\u{2593}", "\u{2588}"];

fn ascii(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let rows = 4usize;
    let row_h = h / rows as f64;
    c.set_font(row_h.floor(), &ink.mono);
    c.text_baseline = TextBaseline::Middle;
    c.text_align = TextAlign::Center;
    for i in 0..n {
        let cx = i as f64 * (cw + ink.gap) + cw / 2.0;
        let l = lv[i];
        let fill = l * rows as f64;
        let colour = band(ink, l);
        for k in 0..rows {
            let part = clamp(fill - k as f64, 0.0, 1.0);
            let glyph = if part <= 0.0 {
                if k > 0 {
                    break;
                }
                c.fill_style = ink.groove;
                SHADES[0]
            } else {
                c.fill_style = colour;
                SHADES[((part * 4.0).ceil() as usize - 1).min(3)]
            };
            c.fill_text(glyph, cx, h - (k as f64 + 0.5) * row_h);
        }
    }
}

const MATRIX_GLYPHS: &str = "0123456789:=+<>";
const MATRIX_TRAIL: usize = 3;
// Full level crosses the box's 32px in about a third of a second.
const MATRIX_SPEED: f64 = 90.0;

fn matrix(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let row_h = 8.0;
    let rows = (h / row_h).floor().max(1.0);
    let span = rows + MATRIX_TRAIL as f64;
    if state.heads.len() != n {
        state.heads = (0..n).map(|s| ((s * 5) as f64) % span).collect();
    }
    c.set_font(row_h, &ink.mono);
    c.text_baseline = TextBaseline::Middle;
    c.text_align = TextAlign::Center;
    let oy = (h - rows * row_h) / 2.0;
    let glyphs = MATRIX_GLYPHS.as_bytes();
    for i in 0..n {
        let l = lv[i];
        state.heads[i] = (state.heads[i] + l * MATRIX_SPEED / row_h * dt) % span;
        let head = state.heads[i];
        let hr = head.floor();
        let cx = i as f64 * (cw + ink.gap) + cw / 2.0;
        for k in 0..=MATRIX_TRAIL {
            let row = hr - k as f64;
            if row < 0.0 || row >= rows {
                continue;
            }
            if l <= 0.0 {
                c.fill_style = ink.groove;
                c.global_alpha = 1.0;
            } else if k == 0 {
                c.fill_style = if l >= LEVEL_ACCENT_FROM {
                    ink.accent
                } else {
                    ink.content
                };
                c.global_alpha = 1.0;
            } else {
                c.fill_style = ink.dim;
                c.global_alpha = 1.0 - k as f64 / (MATRIX_TRAIL as f64 + 1.0);
            }
            let mut g = (i as f64 * 7.0 + (hr - k as f64) * 13.0 + (head / span).floor() * 3.0)
                % glyphs.len() as f64;
            if g < 0.0 {
                g += glyphs.len() as f64;
            }
            let glyph = (glyphs[g as usize] as char).to_string();
            c.fill_text(&glyph, cx, oy + row * row_h + row_h / 2.0);
        }
    }
    c.global_alpha = 1.0;
}

const RAIN_POOL: usize = 40;
const RAIN_RATE: f64 = 9.0;

fn rain(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let r = ink.radius.min(cw / 2.0);
    let pool = state.pool.get_or_insert_with(|| Pool::new(RAIN_POOL));
    buf(&mut state.spawn_acc, n);
    let acc = &mut state.spawn_acc;
    let drop_w = cw.min(2.0);
    let drop_h = 3.0;
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let l = lv[i];
        c.fill_style = ink.groove;
        pill(c, x, 0.0, cw, h, r);
        if l > 0.0 {
            let fh = h.min((h * l).max(r * 2.0));
            c.global_alpha = 0.45;
            c.fill_style = ink.dim;
            pill(c, x, h - fh, cw, fh, r);
            c.global_alpha = 1.0;
        }
        acc[i] += l * RAIN_RATE * dt;
        while acc[i] >= 1.0 {
            acc[i] -= 1.0;
            let Some(slot) = pool.spawn_slot() else {
                continue;
            };
            pool.alive[slot] = true;
            pool.x[slot] = x + (cw - drop_w) / 2.0;
            pool.y[slot] = clamp(h - l * h, 0.0, h - drop_h);
            pool.v[slot] = 24.0 + l * 40.0;
            pool.c[slot] = l;
        }
    }
    for p in 0..RAIN_POOL {
        if !pool.alive[p] {
            continue;
        }
        pool.y[p] += pool.v[p] * dt;
        if pool.y[p] + drop_h > h {
            pool.alive[p] = false;
            continue;
        }
        c.fill_style = if pool.c[p] >= LEVEL_ACCENT_FROM {
            ink.accent
        } else {
            ink.content
        };
        c.fill_rect(pool.x[p], pool.y[p], drop_w, drop_h);
    }
}

const FLAME_POOL: usize = 36;
const FLAME_RATE: f64 = 10.0;
const EMBER_LIFE: f64 = 0.45;

fn flame(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let r = ink.radius.min(cw / 2.0);
    let pool = state.pool.get_or_insert_with(|| Pool::new(FLAME_POOL));
    buf(&mut state.spawn_acc, n);
    buf(&mut state.flick, n);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let l = lv[i];
        if dt > 0.0 {
            state.flick[i] = (state.rng.f64() * 2.0 - 1.0) * 0.12 * l;
        }
        let fh = if l > 0.0 {
            clamp(h * l * (1.0 + state.flick[i]), r * 2.0, h)
        } else {
            0.0
        };
        if fh > 0.0 {
            let tip = fh.min(3.0_f64.max(fh * 0.35));
            c.fill_style = ink.dim;
            pill(c, x, h - fh, cw, fh, r);
            c.fill_style = if l >= LEVEL_DIM_BELOW {
                ink.accent
            } else {
                ink.content
            };
            pill(c, x, h - fh, cw, tip, r);
        }
        state.spawn_acc[i] += l * l * FLAME_RATE * dt;
        while state.spawn_acc[i] >= 1.0 {
            state.spawn_acc[i] -= 1.0;
            let Some(slot) = pool.spawn_slot() else {
                continue;
            };
            pool.alive[slot] = true;
            pool.x[slot] = x + state.rng.f64() * (cw - 1.0);
            pool.y[slot] = clamp(h - fh - 2.0, 0.0, h - 2.0);
            pool.v[slot] = 18.0 + l * 30.0;
            pool.a[slot] = EMBER_LIFE;
        }
    }
    floor_line(c, f);
    c.fill_style = ink.accent;
    for p in 0..FLAME_POOL {
        if !pool.alive[p] {
            continue;
        }
        pool.y[p] -= pool.v[p] * dt;
        pool.a[p] -= dt;
        if pool.a[p] <= 0.0 || pool.y[p] < 0.0 {
            pool.alive[p] = false;
            continue;
        }
        c.global_alpha = pool.a[p] / EMBER_LIFE;
        c.fill_rect(pool.x[p], pool.y[p], 1.0, 2.0);
    }
    c.global_alpha = 1.0;
}

const BUBBLE_POOL: usize = 20;
const BUBBLE_RATE: f64 = 16.0;

fn bubbles(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let pool = state.pool.get_or_insert_with(|| Pool::new(BUBBLE_POOL));
    let total: f64 = lv.iter().sum();
    let energy = total / n as f64;
    state.acc += energy * BUBBLE_RATE * dt;
    while state.acc >= 1.0 {
        state.acc -= 1.0;
        let slot = pool.spawn_slot();
        let Some(slot) = slot.filter(|_| total > 0.0) else {
            continue;
        };
        // A column picked in proportion to its own level, so the rings rise
        // off the bands that are actually loud.
        let mut pick = state.rng.f64() * total;
        let mut col = 0;
        while col < n - 1 && pick > lv[col] {
            pick -= lv[col];
            col += 1;
        }
        let rad = 1.5 + lv[col] * 2.5;
        pool.alive[slot] = true;
        pool.r[slot] = rad;
        pool.x[slot] = clamp(
            col as f64 * (cw + ink.gap) + cw / 2.0 + (state.rng.f64() - 0.5) * 2.0 * (cw + ink.gap),
            rad + 0.5,
            w - rad - 0.5,
        );
        pool.y[slot] = h - rad - 1.0;
        pool.v[slot] = 10.0 + energy * 36.0;
        pool.c[slot] = lv[col];
        pool.a[slot] = state.rng.f64() * 6.28;
    }
    floor_line(c, f);
    c.line_width = 1.0;
    for p in 0..BUBBLE_POOL {
        if !pool.alive[p] {
            continue;
        }
        pool.y[p] -= pool.v[p] * dt;
        let rr = pool.r[p];
        if pool.y[p] - rr < 0.5 {
            pool.alive[p] = false;
            continue;
        }
        let bx = clamp(
            pool.x[p] + (pool.y[p] * 0.35 + pool.a[p]).sin(),
            rr + 0.5,
            w - rr - 0.5,
        );
        c.global_alpha = clamp(pool.y[p] / h + 0.25, 0.25, 1.0);
        c.stroke_style = if pool.c[p] >= LEVEL_ACCENT_FROM {
            ink.accent
        } else {
            ink.content
        };
        c.begin_path();
        c.arc(bx, pool.y[p], rr, 0.0, PI * 2.0);
        c.stroke();
    }
    c.global_alpha = 1.0;
}

const SCOPE_BANDS: usize = 8;
const SCOPE_POINTS: usize = 48;

fn scope(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, SCOPE_BANDS);
    buf(&mut state.phase, SCOPE_BANDS);
    let phase = &mut state.phase;
    let mut sum = 0.0;
    let mut amp = 0.0;
    for k in 0..SCOPE_BANDS {
        phase[k] = (phase[k] + dt * (k as f64 + 1.0) * 3.1) % (PI * 2.0);
        sum += lv[k];
        if lv[k] > amp {
            amp = lv[k];
        }
    }
    let mid = h / 2.0;
    let lw = 1.5;
    let reach = mid - lw;
    c.fill_style = ink.groove;
    c.fill_rect(0.0, mid.floor(), w, 1.0);
    c.begin_path();
    for i in 0..=SCOPE_POINTS {
        let u = i as f64 / SCOPE_POINTS as f64;
        let x = u * w;
        let mut s = 0.0;
        if sum > 0.0 {
            for b in 0..SCOPE_BANDS {
                s += lv[b] * ((b as f64 + 1.0) * 2.0 * PI * u * 1.5 + phase[b]).sin();
            }
            s = s / sum * amp;
        }
        let y = clamp(mid - s * reach, lw / 2.0, h - lw / 2.0);
        if i == 0 {
            c.move_to(x, y);
        } else {
            c.line_to(x, y);
        }
    }
    c.line_width = lw;
    c.stroke_style = if amp > 0.0 {
        band(ink, f64::max(amp, LEVEL_DIM_BELOW))
    } else {
        ink.dim
    };
    c.stroke();
}

fn pulse(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 3);
    let mids = max_of(&lv, 3, 8);
    let highs = max_of(&lv, 8, 12);
    let cx = w / 2.0;
    let cy = h / 2.0;
    let ring_max = cy - 1.0;
    let disc = 2.5 + bass * (ring_max - 6.5);
    let ring = ring_max.min(disc + 2.0 + mids * 4.0);

    let reach = highs * (cx - ring - 2.0);
    if reach > 0.5 {
        c.fill_style = ink.dim;
        c.fill_rect(cx - ring - 1.0 - reach, cy.floor(), reach, 1.0);
        c.fill_rect(cx + ring + 1.0, cy.floor(), reach, 1.0);
    }
    c.line_width = 1.0;
    c.stroke_style = if mids > 0.0 {
        band(ink, f64::max(mids, LEVEL_DIM_BELOW))
    } else {
        ink.groove
    };
    c.begin_path();
    c.arc(cx, cy, ring, 0.0, PI * 2.0);
    c.stroke();
    c.fill_style = if bass > 0.0 {
        band(ink, f64::max(bass, LEVEL_DIM_BELOW))
    } else {
        ink.groove
    };
    c.begin_path();
    c.arc(cx, cy, disc, 0.0, PI * 2.0);
    c.fill();
}

// A beat is a rise in the bass over its own recent average, with a refractory
// window so one kick draws one complex.
const BEAT_RISE: f64 = 0.12;
const BEAT_FLOOR: f64 = 0.3;
const BEAT_REFRACTORY: f64 = 0.25;
const BEAT_SECONDS: f64 = 0.2;
const ECG_SPEED: f64 = 40.0;

// Q dip, R spike, S dip, back to the line, over BEAT_SECONDS.
fn ecg(u: f64) -> f64 {
    if u < 0.15 {
        return -0.2 * (u / 0.15);
    }
    if u < 0.35 {
        return -0.2 + 1.2 * ((u - 0.15) / 0.2);
    }
    if u < 0.55 {
        return 1.0 - 1.4 * ((u - 0.35) / 0.2);
    }
    if u < 1.0 {
        return -0.4 * (1.0 - (u - 0.55) / 0.45);
    }
    0.0
}

fn heartbeat(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 2);
    let n = (w.ceil() as usize).max(2);
    history(state, n);
    if dt > 0.0 {
        state.avg += (bass - state.avg) * (1.0 - (-dt / 0.3).exp());
        state.since = Some(state.since.unwrap_or(BEAT_REFRACTORY) + dt);
        if bass > BEAT_FLOOR
            && bass - state.avg > BEAT_RISE
            && state.since.unwrap_or(0.0) >= BEAT_REFRACTORY
        {
            state.since = Some(0.0);
            state.beat = Some(0.0);
            state.beat_amp = bass;
        }
        let hist = state.hist.as_mut().expect("history sized above");
        hist.acc += dt * ECG_SPEED;
        while hist.acc >= 1.0 {
            hist.acc -= 1.0;
            let mut v = 0.0;
            if let Some(beat) = state.beat.filter(|b| *b < BEAT_SECONDS) {
                v = ecg(beat / BEAT_SECONDS) * state.beat_amp;
                state.beat = Some(beat + 1.0 / ECG_SPEED);
            }
            hist.push(v, bass);
        }
    }
    let hist = state.hist.as_ref().expect("history sized above");
    let mid = round(h * 0.6) + 0.5;
    let lw = 1.5;
    c.fill_style = ink.groove;
    c.fill_rect(0.0, mid - 0.5, w, 1.0);
    // Older samples in `dim`, the newest stretch in `content`, so the trace
    // reads as moving leftward even when it is flat.
    let split = (n as f64 * 0.6).floor() as usize;
    for pass in 0..2 {
        c.begin_path();
        let from = if pass == 0 { 0 } else { split };
        let to = if pass == 0 { split } else { n - 1 };
        for i in from..=to {
            let sample = hist.v[(hist.head + i) % n];
            let y = clamp(
                mid - if sample >= 0.0 {
                    sample * (mid - lw)
                } else {
                    sample * (h - mid - lw)
                },
                lw / 2.0,
                h - lw / 2.0,
            );
            let x = i as f64 / (n - 1) as f64 * w;
            if i == from {
                c.move_to(x, y);
            } else {
                c.line_to(x, y);
            }
        }
        c.line_width = lw;
        c.stroke_style = if pass == 0 { ink.dim } else { ink.content };
        c.stroke();
    }
}

const TERRAIN_STEP: f64 = 2.0;
const TERRAIN_SPEED: f64 = 24.0;

fn terrain(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let n = ((w / TERRAIN_STEP).floor() as usize + 1).max(2);
    history(state, n);
    if dt > 0.0 {
        let sq: f64 = lv.iter().map(|v| v * v).sum();
        let loud = (sq / 12.0).sqrt();
        let bass = max_of(&lv, 0, 3);
        let hist = state.hist.as_mut().expect("history sized above");
        hist.acc += dt * TERRAIN_SPEED / TERRAIN_STEP;
        while hist.acc >= 1.0 {
            hist.acc -= 1.0;
            hist.push(loud, bass);
        }
    }
    let hist = state.hist.as_ref().expect("history sized above");
    let lw = 1.5;
    for layer in 0..2 {
        let key = if layer == 0 { &hist.w } else { &hist.v };
        c.begin_path();
        c.move_to(0.0, h);
        for j in 0..n {
            let y = clamp(h - key[(hist.head + j) % n] * (h - lw), lw / 2.0, h);
            c.line_to(w.min(j as f64 * TERRAIN_STEP), y);
        }
        c.line_to(w, h);
        c.close_path();
        c.global_alpha = if layer == 0 { 1.0 } else { 0.35 };
        c.fill_style = if layer == 0 { ink.groove } else { ink.dim };
        c.fill();
    }
    c.global_alpha = 1.0;
    c.begin_path();
    for k in 0..n {
        let yy = clamp(
            h - hist.v[(hist.head + k) % n] * (h - lw),
            lw / 2.0,
            h - lw / 2.0,
        );
        if k == 0 {
            c.move_to(0.0, yy);
        } else {
            c.line_to(w.min(k as f64 * TERRAIN_STEP), yy);
        }
    }
    c.line_width = lw;
    c.stroke_style = ink.content;
    c.stroke();
}

/// Deterministic 0..1 per integer triple, for layouts that must not change
/// from one frame to the next (a tile's band, a star's place, a bit).
fn hash(a: i32, b: i32, c: i32) -> f64 {
    let mut x = a
        .wrapping_mul(374_761_393)
        .wrapping_add(b.wrapping_mul(668_265_263))
        .wrapping_add(c.wrapping_mul(1_440_662_683));
    x = (x ^ ((x as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    x ^= ((x as u32) >> 16) as i32;
    f64::from(x as u32) / 4_294_967_296.0
}

/// Nearest band for each of `count` slots, for styles with more slots than
/// cava has bands, where `fold` would leave the extra slots empty.
fn spread(levels: &[f64], count: usize) -> Vec<f64> {
    let src = fold(levels, BAR_COUNT);
    (0..count)
        .map(|i| src[(BAR_COUNT - 1).min(i * BAR_COUNT / count)])
        .collect()
}

/// The heartbeat's onset test, on its own keys so a style can read it for any
/// band.
fn onset(state: &mut State, v: f64, dt: f64) -> bool {
    if dt.is_nan() || dt <= 0.0 {
        return false;
    }
    state.on_avg += (v - state.on_avg) * (1.0 - (-dt / 0.3).exp());
    let since = state.on_since.unwrap_or(BEAT_REFRACTORY) + dt;
    state.on_since = Some(since);
    if v > BEAT_FLOOR && v - state.on_avg > BEAT_RISE && since >= BEAT_REFRACTORY {
        state.on_since = Some(0.0);
        return true;
    }
    false
}

fn bricks(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let _ = w;
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(f.w, n, ink.gap);
    let brick_h = 3.0;
    let gap_y = 2.0;
    let rows = ((h + gap_y) / (brick_h + gap_y)).floor().max(1.0);
    let r = 1.0_f64.min(cw / 2.0);
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap);
        let l = lv[i];
        let lit = if l > 0.0 { (l * rows).ceil() } else { 0.0 };
        if lit == 0.0 {
            c.fill_style = ink.groove;
            pill(c, x, h - brick_h, cw, brick_h, r);
            continue;
        }
        c.fill_style = band(ink, l);
        let mut k = 0.0;
        while k < lit {
            pill(c, x, h - (k + 1.0) * brick_h - k * gap_y, cw, brick_h, r);
            k += 1.0;
        }
    }
}

fn columns(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let pitch = 2.0;
    let count = ((w + 1.0) / pitch).floor().max(2.0) as usize;
    let src = fold(f.levels, BAR_COUNT);
    // Whole pixels, or a 1px column straddles two and the gaps blur shut.
    let ox = ((w - ((count as f64 - 1.0) * pitch + 1.0)) / 2.0).floor();
    for col in 0..count {
        let pos = col as f64 / (count - 1) as f64 * (BAR_COUNT - 1) as f64;
        let i0 = pos.floor() as usize;
        let i1 = (BAR_COUNT - 1).min(i0 + 1);
        let l = src[i0] + (src[i1] - src[i0]) * (pos - i0 as f64);
        let bh = l * h;
        let x = ox + col as f64 * pitch;
        if bh < 1.0 {
            c.fill_style = ink.groove;
            c.fill_rect(x, h - 1.0, 1.0, 1.0);
        } else {
            c.fill_style = band(ink, l);
            c.fill_rect(x, h - bh, 1.0, bh);
        }
    }
}

const SCATTER_HZ: f64 = 20.0;

fn scatter(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let size = 2.0;
    let pitch = 3.0;
    let cols = ((w + pitch - size) / pitch).floor().max(1.0) as usize;
    let rows = ((h - 2.0 + pitch - size) / pitch).floor().max(1.0) as usize;
    let lv = spread(f.levels, cols);
    state.acc += dt * SCATTER_HZ;
    if state.acc >= 1.0 {
        state.tick = (state.tick + state.acc.floor()) % 100_000.0;
        state.acc -= state.acc.floor();
    }
    let tick = state.tick;
    let ox = (w - ((cols as f64 - 1.0) * pitch + size)) / 2.0;
    floor_line(c, f);
    for col in 0..cols {
        let l = lv[col];
        if l <= 0.0 {
            continue;
        }
        c.fill_style = band(ink, l);
        for k in 0..rows {
            // k counts up from the floor: grains settle low.
            let weight = 1.0 - 0.5 * k as f64 / f64::max(1.0, rows as f64 - 1.0);
            if hash(col as i32, k as i32, tick as i32) < l * l * weight {
                c.fill_rect(
                    ox + col as f64 * pitch,
                    h - 2.0 - size - k as f64 * pitch,
                    size,
                    size,
                );
            }
        }
    }
}

const RETRO_GRID_SPEED: f64 = 0.9;

fn retro(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 3);
    let energy = mean(&lv, 0, 12);
    let horizon = round(h * 0.5).max(3.0);
    let cx = w / 2.0;

    let sun_r = (horizon - 1.0).min(horizon * 0.7 + bass * 3.0);
    c.fill_style = ink.dim;
    let mut d = 1.0;
    while d <= sun_r {
        // Stripes cut through the lower half, thinning toward the horizon.
        if d < sun_r * 0.55 && (d / 2.0).floor() % 2.0 == 1.0 {
            d += 1.0;
            continue;
        }
        let half = (sun_r * sun_r - (d - 0.5) * (d - 0.5)).sqrt();
        c.fill_rect(cx - half, horizon - d, half * 2.0, 1.0);
        d += 1.0;
    }

    let floor_h = h - horizon - 1.0;
    c.stroke_style = ink.dim;
    c.line_width = 1.0;
    c.global_alpha = 0.6;
    c.begin_path();
    let spokes = 10;
    for i in 0..=spokes {
        let bx = i as f64 / spokes as f64 * w;
        c.move_to(cx + (bx - cx) * 0.08, horizon + 0.5);
        c.line_to(clamp(bx, 0.5, w - 0.5), h - 0.5);
    }
    state.scroll = (state.scroll + energy * RETRO_GRID_SPEED * dt) % 1.0;
    let lines = 4;
    for j in 0..lines {
        let z = (j as f64 + state.scroll) / lines as f64;
        // On a pixel centre, or the 1px stroke smears over two rows.
        let y = (horizon + 2.0 + z * z * (floor_h - 2.0)).floor() + 0.5;
        c.move_to(0.0, y);
        c.line_to(w, y);
    }
    c.stroke();
    c.global_alpha = 1.0;
    c.fill_style = ink.dim;
    c.fill_rect(0.0, horizon, w, 1.0);

    // The spectrum rides the horizon, bass in the middle.
    let n = 12usize;
    let peak = max_of(&lv, 0, n);
    c.begin_path();
    for p in 0..=2 * n {
        let band_index = p.abs_diff(n);
        let l = lv[(n - 1).min(band_index)];
        let px = p as f64 / (2 * n) as f64 * w;
        let py = clamp(
            horizon - 1.0 - l * (horizon - 2.0) * 0.7,
            0.75,
            horizon - 0.75,
        );
        if p == 0 {
            c.move_to(px, py);
        } else {
            c.line_to(px, py);
        }
    }
    c.line_width = 1.5;
    c.stroke_style = if peak > 0.0 {
        band(ink, f64::max(peak, LEVEL_DIM_BELOW))
    } else {
        ink.dim
    };
    c.stroke();
}

// Rows per second at full level.
const BINARY_SPEED: f64 = 12.0;

fn binary(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let row_h = 8.0;
    let char_w = 6.0;
    let rows = (h / row_h).floor().max(1.0) as usize;
    let cols = (w / char_w).floor().max(1.0) as usize;
    let lv = fold(f.levels, cols);
    buf(&mut state.bin_off, cols);
    let off = &mut state.bin_off;
    let ox = (w - cols as f64 * char_w) / 2.0 + char_w / 2.0;
    let oy = (h - rows as f64 * row_h) / 2.0 + row_h / 2.0;
    c.set_font(row_h, &ink.mono);
    c.text_baseline = TextBaseline::Middle;
    c.text_align = TextAlign::Center;
    for col in 0..cols {
        let l = lv[col];
        off[col] = (off[col] + l * BINARY_SPEED * dt) % 100_000.0;
        let scroll = off[col].floor();
        for r in 0..rows {
            let one = hash(col as i32, (r as f64 - scroll) as i32, 3) < 0.15 + 0.6 * l;
            if one && l >= LEVEL_DIM_BELOW {
                c.fill_style = band(ink, l);
            } else if one {
                c.fill_style = ink.dim;
            } else {
                c.fill_style = ink.groove;
            }
            c.fill_text(
                if one { "1" } else { "0" },
                ox + col as f64 * char_w,
                oy + r as f64 * row_h,
            );
        }
    }
}

const SAKURA_PETALS: usize = 16;

fn sakura(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let energy = mean(&lv, 0, 12);
    state.drift = (state.drift + energy * dt) % 10_000.0;
    let drift = state.drift;
    let shown = 6.0 + energy * (SAKURA_PETALS as f64 - 6.0);
    let wrap = h + 8.0;
    for i in 0..SAKURA_PETALS {
        let ii = i as i32;
        let alpha = clamp(shown - i as f64, 0.0, 1.0);
        if alpha <= 0.0 {
            continue;
        }
        let near = hash(ii, 3, 0) > 0.55;
        let size = if near {
            2.4 + hash(ii, 4, 0) * 0.8
        } else {
            1.4 + hash(ii, 4, 0) * 0.5
        };
        // Distant petals fall faster, as in cliamp's reference.
        let speed = if near { 16.0 } else { 26.0 };
        let y = (hash(ii, 2, 0) * wrap + drift * speed) % wrap - 4.0;
        if y < 0.0 || y > h {
            continue;
        }
        let phase = hash(ii, 5, 0) * 6.283;
        let x = hash(ii, 1, 0) * w + (drift * 2.5 + phase).sin() * 3.0;
        let a = drift * 3.0 + phase;
        let ux = a.cos() * size * 1.4;
        let uy = a.sin() * size * 1.4;
        let vx = -a.sin() * size * 1.6;
        let vy = a.cos() * size * 1.6;
        c.global_alpha = alpha;
        c.fill_style = if near { ink.accent } else { ink.dim };
        c.begin_path();
        c.move_to(clamp(x + ux, 0.0, w), clamp(y + uy, 0.0, h));
        c.quad_to(
            clamp(x + vx, 0.0, w),
            clamp(y + vy, 0.0, h),
            clamp(x - ux, 0.0, w),
            clamp(y - uy, 0.0, h),
        );
        c.quad_to(
            clamp(x - vx, 0.0, w),
            clamp(y - vy, 0.0, h),
            clamp(x + ux, 0.0, w),
            clamp(y + uy, 0.0, h),
        );
        c.fill();
    }
    c.global_alpha = 1.0;
}

const FIREWORK_ROCKETS: usize = 5;
const FIREWORK_SPARKS: usize = 72;
const FIREWORK_RATE: f64 = 3.0;
const SPARK_LIFE: f64 = 0.9;
const SPARK_GRAVITY: f64 = 30.0;

fn launch(state: &mut State, w: f64, h: f64, l: f64) {
    for i in 0..FIREWORK_ROCKETS {
        if state.rk_alive[i] {
            continue;
        }
        state.rk_alive[i] = true;
        state.rk_x[i] = 8.0 + state.rng.f64() * f64::max(0.0, w - 16.0);
        state.rk_y[i] = h - 1.0;
        state.rk_top[i] = h * (0.2 + state.rng.f64() * 0.3);
        state.rk_l[i] = l;
        return;
    }
}

fn firework(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 3);
    let energy = mean(&lv, 0, 12);
    let peak = max_of(&lv, 0, 12);
    if state.rk_alive.len() != FIREWORK_ROCKETS {
        state.rk_alive = vec![false; FIREWORK_ROCKETS];
        state.rk_x = vec![0.0; FIREWORK_ROCKETS];
        state.rk_y = vec![0.0; FIREWORK_ROCKETS];
        state.rk_top = vec![0.0; FIREWORK_ROCKETS];
        state.rk_l = vec![0.0; FIREWORK_ROCKETS];
    }
    if state.pool.is_none() {
        state.pool = Some(Pool::new(FIREWORK_SPARKS));
    }
    state.acc += energy * FIREWORK_RATE * dt;
    if onset(state, bass, dt) {
        state.acc += 1.0;
    }
    while state.acc >= 1.0 {
        state.acc -= 1.0;
        launch(state, w, h, peak);
    }
    floor_line(c, f);
    c.fill_style = ink.dim;
    let pool = state.pool.as_mut().expect("pool created above");
    for i in 0..FIREWORK_ROCKETS {
        if !state.rk_alive[i] {
            continue;
        }
        state.rk_y[i] -= 60.0 * dt;
        if state.rk_y[i] <= state.rk_top[i] {
            state.rk_alive[i] = false;
            let l = state.rk_l[i];
            let count = 10 + round(l * 10.0) as usize;
            let speed = 14.0 + l * 22.0;
            for k in 0..count {
                let Some(slot) = pool.spawn_slot() else {
                    break;
                };
                let ang = k as f64 / count as f64 * PI * 2.0;
                let sp = speed * (0.6 + state.rng.f64() * 0.4);
                pool.alive[slot] = true;
                pool.x[slot] = state.rk_x[i];
                pool.y[slot] = state.rk_top[i];
                pool.v[slot] = ang.cos() * sp;
                pool.r[slot] = ang.sin() * sp;
                pool.a[slot] = SPARK_LIFE;
                pool.c[slot] = l;
            }
            continue;
        }
        c.fill_rect(state.rk_x[i], clamp(state.rk_y[i], 0.0, h - 3.0), 1.0, 3.0);
    }
    for p in 0..FIREWORK_SPARKS {
        if !pool.alive[p] {
            continue;
        }
        pool.r[p] += SPARK_GRAVITY * dt;
        pool.x[p] += pool.v[p] * dt;
        pool.y[p] += pool.r[p] * dt;
        pool.a[p] -= dt;
        if pool.a[p] <= 0.0
            || pool.x[p] < 0.0
            || pool.x[p] > w - 1.5
            || pool.y[p] < 0.0
            || pool.y[p] > h - 2.5
        {
            pool.alive[p] = false;
            continue;
        }
        c.global_alpha = clamp(pool.a[p] / SPARK_LIFE * 1.4, 0.0, 1.0);
        c.fill_style = if pool.c[p] >= LEVEL_ACCENT_FROM {
            ink.accent
        } else {
            ink.content
        };
        c.fill_rect(pool.x[p], pool.y[p], 1.5, 1.5);
    }
    c.global_alpha = 1.0;
}

const FIREFLIES: usize = 12;

fn firefly(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let bass = mean(&lv, 0, 4);
    let high = mean(&lv, 8, 12);
    let energy = mean(&lv, 0, 12);
    state.clock = (state.clock + energy * 1.5 * dt) % 100_000.0;
    let t = state.clock;

    c.fill_style = ink.groove;
    c.begin_path();
    c.move_to(0.0, h);
    let mut gx = 0.0;
    while gx <= w {
        let gh = 2.5 + 1.2 * (gx * 0.41).sin() + 0.8 * (gx * 0.17 + 2.3).sin();
        c.line_to(gx, h - gh);
        gx += 2.0;
    }
    c.line_to(w, h);
    c.close_path();
    c.fill();

    let halo = 2.5;
    for i in 0..FIREFLIES {
        let ii = i as i32;
        let fi = i as f64;
        let fx = 0.6 + hash(ii, 1, 9) * 0.5;
        let fy = 0.9 + hash(ii, 2, 9) * 0.5;
        let phx = hash(ii, 3, 9) * 6.283;
        let phy = hash(ii, 4, 9) * 6.283;
        let mut x =
            w / 2.0 + (t * fx + phx).cos() * (w / 2.0 - 4.0) + bass * 3.0 * (t * 0.7 + phx).sin();
        let mut y = (h - 6.0) * 0.5 + (t * fy + phy).sin() * (h - 8.0) * 0.4;
        x = clamp(x, halo, w - halo);
        y = clamp(y, halo, h - 5.0);
        let glow = 0.5 + 0.5 * (t * 3.0 + fi * 1.31).sin();
        if glow + high * 0.6 > 0.8 {
            c.global_alpha = 0.3;
            c.fill_style = ink.accent;
            c.begin_path();
            c.arc(x, y, halo, 0.0, PI * 2.0);
            c.fill();
            c.global_alpha = 1.0;
            c.fill_style = if high >= LEVEL_DIM_BELOW {
                ink.accent
            } else {
                ink.content
            };
            c.fill_rect(x - 1.0, y - 1.0, 2.0, 2.0);
        } else {
            c.fill_style = ink.dim;
            c.fill_rect(x - 0.5, y - 0.5, 1.0, 1.0);
        }
    }
}

const MOSAIC_DECAY: f64 = 0.25;

fn mosaic(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let tile_w = 5.0;
    let tile_h = 4.0;
    let gap = 1.0;
    let cols = ((w + gap) / (tile_w + gap)).floor().max(1.0) as usize;
    let rows = ((h + gap) / (tile_h + gap)).floor().max(1.0) as usize;
    let n = cols * rows;
    let src = fold(f.levels, BAR_COUNT);
    buf(&mut state.tile_v, n);
    let value = &mut state.tile_v;
    let decay = (-dt / MOSAIC_DECAY).exp();
    let ox = (w - (cols as f64 * (tile_w + gap) - gap)) / 2.0;
    let oy = (h - (rows as f64 * (tile_h + gap) - gap)) / 2.0;
    for k in 0..n {
        let col = k % cols;
        let r = k / cols;
        let band_index = (hash(col as i32, r as i32, 1) * BAR_COUNT as f64).floor() as usize;
        let threshold = 0.25 + hash(col as i32, r as i32, 2) * 0.65;
        let l = src[band_index];
        if dt > 0.0 {
            value[k] *= decay;
            if l > threshold {
                value[k] = value[k].max(0.4 + 0.6 * (l - threshold) / (1.0 - threshold));
            }
        }
        let v = value[k];
        let x = ox + col as f64 * (tile_w + gap);
        let y = oy + r as f64 * (tile_h + gap);
        if v < 0.05 {
            c.fill_style = ink.groove;
        } else {
            c.fill_style = band(ink, v);
            c.global_alpha = 0.4 + 0.6 * v;
        }
        pill(c, x, y, tile_w, tile_h, 1.0);
        c.global_alpha = 1.0;
    }
}

const SAND_HZ: f64 = 40.0;
// Past this share of the bed a bass hit drains it, and past the second it
// drains on its own, so a passage with no clear hits cannot fill it solid.
const SAND_DRAIN_ON_HIT: f64 = 0.35;
const SAND_DRAIN_ALWAYS: f64 = 0.55;
const SAND_DRAIN_TO: f64 = 0.15;

fn sand(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let cell = 2.0;
    let cols = (w / cell).floor().max(1.0) as usize;
    let rows = (h / cell).floor().max(2.0) as usize;
    buf(&mut state.sand, cols * rows);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 3);
    if onset(state, bass, dt)
        && state
            .grains
            .is_some_and(|g| g > (cols * rows) as f64 * SAND_DRAIN_ON_HIT)
    {
        state.draining = true;
    }
    state.acc += dt * SAND_HZ;
    let mut steps = 0;
    while state.acc >= 1.0 && steps < 4 {
        state.acc -= 1.0;
        steps += 1;
        for b in 0..12 {
            let l = lv[b];
            if l <= 0.0 || state.rng.f64() >= l * l * 0.6 {
                continue;
            }
            let col = clamp(
                round((b as f64 + 0.5) * cols as f64 / 12.0 + (state.rng.f64() - 0.5) * 2.0),
                0.0,
                cols as f64 - 1.0,
            ) as usize;
            if state.sand[col] == 0.0 {
                state.sand[col] = if l >= LEVEL_ACCENT_FROM {
                    3.0
                } else if l >= LEVEL_DIM_BELOW {
                    2.0
                } else {
                    1.0
                };
            }
        }
        if state.draining {
            for d in 0..cols {
                state.sand[(rows - 1) * cols + d] = 0.0;
            }
        }
        let flip = steps % 2 == 0;
        for r in (0..rows - 1).rev() {
            for cc in 0..cols {
                let col = if flip { cols - 1 - cc } else { cc };
                let at = r * cols + col;
                let g = state.sand[at];
                if g == 0.0 {
                    continue;
                }
                let below = at + cols;
                let mut to: Option<usize> = None;
                if state.sand[below] == 0.0 {
                    to = Some(below);
                } else {
                    let first: i64 = if state.rng.f64() < 0.5 { -1 } else { 1 };
                    let c0 = col as i64;
                    if c0 + first >= 0
                        && c0 + first < cols as i64
                        && state.sand[(below as i64 + first) as usize] == 0.0
                    {
                        to = Some((below as i64 + first) as usize);
                    } else if c0 - first >= 0
                        && c0 - first < cols as i64
                        && state.sand[(below as i64 - first) as usize] == 0.0
                    {
                        to = Some((below as i64 - first) as usize);
                    }
                }
                if let Some(to) = to {
                    state.sand[to] = g;
                    state.sand[at] = 0.0;
                }
            }
        }
    }
    if steps > 0 {
        let count = state.sand.iter().filter(|g| **g != 0.0).count() as f64;
        state.grains = Some(count);
        if count > (cols * rows) as f64 * SAND_DRAIN_ALWAYS {
            state.draining = true;
        } else if count <= (cols * rows) as f64 * SAND_DRAIN_TO {
            state.draining = false;
        }
    }
    floor_line(c, f);
    let oy = h - rows as f64 * cell;
    let ox = (w - cols as f64 * cell) / 2.0;
    for row in 0..rows {
        let mut x = 0;
        while x < cols {
            let v = state.sand[row * cols + x];
            if v == 0.0 {
                x += 1;
                continue;
            }
            let start = x;
            while x < cols && state.sand[row * cols + x] == v {
                x += 1;
            }
            c.fill_style = if v == 3.0 {
                ink.accent
            } else if v == 2.0 {
                ink.content
            } else {
                ink.dim
            };
            c.fill_rect(
                ox + start as f64 * cell,
                oy + row as f64 * cell,
                (x - start) as f64 * cell,
                cell,
            );
        }
    }
}

const GEYSER_POOL: usize = 90;
const GEYSER_GRAVITY: f64 = 120.0;

fn jet(pool: &mut Pool, x: f64, y: f64, vx: f64, vy: f64, l: f64) {
    let Some(slot) = pool.spawn_slot() else {
        return;
    };
    pool.alive[slot] = true;
    pool.x[slot] = x;
    pool.y[slot] = y;
    pool.v[slot] = vx;
    pool.r[slot] = vy;
    pool.c[slot] = l;
}

fn geyser(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, 12);
    let bass = max_of(&lv, 0, 3);
    let mid = mean(&lv, 3, 8);
    let high = mean(&lv, 8, 12);
    let steady = bass * 0.85 + mid * 0.25 + high * 0.08;
    state.pool.get_or_insert_with(|| Pool::new(GEYSER_POOL));
    let jx = w / 2.0;
    let floor_y = h - 2.0;
    state.acc += steady * 60.0 * dt;
    while state.acc >= 1.0 {
        state.acc -= 1.0;
        let (a, b, d) = (state.rng.f64(), state.rng.f64(), state.rng.f64());
        let pool = state.pool.as_mut().expect("pool created above");
        jet(
            pool,
            jx + (a - 0.5) * 3.0,
            floor_y,
            (b - 0.5) * 12.0,
            -(30.0 + steady * 50.0) * (0.8 + d * 0.2),
            bass,
        );
    }
    if onset(state, bass, dt) {
        for _ in 0..20 {
            let (a, b, d) = (state.rng.f64(), state.rng.f64(), state.rng.f64());
            let pool = state.pool.as_mut().expect("pool created above");
            jet(
                pool,
                jx + (a - 0.5) * 4.0,
                floor_y,
                (b - 0.5) * 36.0,
                -(60.0 + bass * 30.0) * (0.7 + d * 0.3),
                1.0,
            );
        }
    }
    floor_line(c, f);
    c.fill_style = ink.groove;
    c.fill_rect(jx - 3.0, h - 2.0, 6.0, 1.0);
    let pool = state.pool.as_mut().expect("pool created above");
    for p in 0..GEYSER_POOL {
        if !pool.alive[p] {
            continue;
        }
        pool.r[p] += GEYSER_GRAVITY * dt;
        pool.x[p] += pool.v[p] * dt;
        pool.y[p] += pool.r[p] * dt;
        if pool.y[p] > floor_y || pool.x[p] < 0.0 || pool.x[p] > w - 1.5 {
            pool.alive[p] = false;
            continue;
        }
        c.fill_style = band(ink, pool.c[p]);
        c.fill_rect(pool.x[p], pool.y[p].max(0.0), 1.5, 1.5);
    }
}

fn loudness(a: &[f64]) -> f64 {
    let sq: f64 = a.iter().map(|v| v * v).sum();
    if a.is_empty() {
        0.0
    } else {
        (sq / a.len() as f64).sqrt()
    }
}

fn stereo(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let lv_l = fold(f.left, BAR_COUNT);
    let lv_r = fold(f.right, BAR_COUNT);
    let meter = [loudness(&lv_l), loudness(&lv_r)];
    let caps = caps(state, &meter, 2, f.dt);
    let label_w = 9.0;
    let seg_w = 3.0;
    let gap_x = 1.0;
    let segs = ((w - label_w + gap_x) / (seg_w + gap_x)).floor().max(1.0);
    let x0 = w - (segs * (seg_w + gap_x) - gap_x);
    let meter_h = (((h - 2.0) / 2.0).floor() - 2.0).max(1.0);
    c.set_font(8.0, &ink.mono);
    c.text_baseline = TextBaseline::Middle;
    c.text_align = TextAlign::Left;
    for ch in 0..2 {
        let y = if ch == 0 {
            h / 2.0 - 1.0 - meter_h
        } else {
            h / 2.0 + 1.0
        };
        c.fill_style = ink.dim;
        c.fill_text(if ch == 0 { "L" } else { "R" }, 0.0, y + meter_h / 2.0);
        let lit = round(meter[ch] * segs);
        let cap = round(caps[ch] * segs) - 1.0;
        let mut k = 0.0;
        while k < segs {
            if k < lit {
                c.fill_style = band(ink, (k + 1.0) / segs);
            } else if k == cap {
                c.fill_style = ink.accent;
            } else {
                c.fill_style = ink.groove;
            }
            c.fill_rect(x0 + k * (seg_w + gap_x), y, seg_w, meter_h);
            k += 1.0;
        }
    }
}

const RS_BARS: usize = 5;
const RS_STARS: usize = 28;
const RS_DISTANCE: f64 = 7.0;
const RS_SPACING: f64 = 1.0;
const RS_HALF: f64 = 0.3;
const RS_MAX_HEIGHT: f64 = 2.0;
const RS_MIN_HEIGHT: f64 = 0.15;
// Corner signs, then faces as corner indices with their outward normal.
const RS_CORNERS: [[f64; 3]; 8] = [
    [-1.0, 0.0, -1.0],
    [1.0, 0.0, -1.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [-1.0, 1.0, -1.0],
    [1.0, 1.0, -1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];
const RS_FACES: [([usize; 4], [f64; 3]); 6] = [
    ([0, 1, 2, 3], [0.0, -1.0, 0.0]),
    ([4, 5, 6, 7], [0.0, 1.0, 0.0]),
    ([0, 1, 5, 4], [0.0, 0.0, -1.0]),
    ([3, 2, 6, 7], [0.0, 0.0, 1.0]),
    ([0, 3, 7, 4], [-1.0, 0.0, 0.0]),
    ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
];

/// The frame's yaw and pitch as their sines and cosines.
struct Rot {
    cy: f64,
    sy: f64,
    cp: f64,
    sp: f64,
}

fn rot(r: &Rot, x: f64, y: f64, z: f64, out: &mut [f64], k: usize) {
    let x1 = x * r.cy + z * r.sy;
    let z1 = -x * r.sy + z * r.cy;
    out[k] = x1;
    out[k + 1] = y * r.cp - z1 * r.sp;
    out[k + 2] = y * r.sp + z1 * r.cp;
}

fn redsector(c: &mut Scene, f: &Frame, state: &mut State) {
    let (w, h, ink, dt) = (f.w, f.h, f.ink, f.dt);
    let lv = fold(f.levels, RS_BARS * 2);
    let energy = mean(&lv, 0, RS_BARS * 2);
    state.yaw = Some((state.yaw.unwrap_or(0.6) + energy * 1.2 * dt) % (PI * 2.0));
    state.tumble = (state.tumble + energy * 0.7 * dt) % (PI * 2.0);
    state.stars = (state.stars + energy * 14.0 * dt) % 100_000.0;
    // Negative pitch tips the top toward the camera, so the caps show.
    let pitch = -0.35 - state.tumble.sin() * 0.2;
    let yaw = state.yaw.unwrap_or(0.6);
    let r = Rot {
        cy: yaw.cos(),
        sy: yaw.sin(),
        cp: pitch.cos(),
        sp: pitch.sin(),
    };

    for s in 0..RS_STARS {
        let si = s as i32;
        let depth = 0.3 + 0.7 * hash(si, 2, 7);
        let sx = ((hash(si, 0, 7) * w - state.stars * depth) % w + w) % w;
        let sy = hash(si, 1, 7) * (h - 1.0);
        c.global_alpha = 0.3 + 0.5 * depth;
        c.fill_style = ink.dim;
        c.fill_rect(sx.min(w - 1.0), sy, 1.0, 1.0);
    }
    c.global_alpha = 1.0;

    // The fit is taken at full height for the current pose, so the object only
    // grows or shrinks as it turns, never as the bars move.
    let mut pts = [0.0; RS_BARS * 8 * 3];
    let cxs = (RS_BARS as f64 - 1.0) / 2.0;
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (1e9_f64, -1e9_f64, 1e9_f64, -1e9_f64);
    for b in 0..RS_BARS {
        for (ci, sg) in RS_CORNERS.iter().enumerate() {
            let k = (b * 8 + ci) * 3;
            rot(
                &r,
                (b as f64 - cxs) * RS_SPACING + sg[0] * RS_HALF,
                sg[1] * RS_MAX_HEIGHT - RS_MAX_HEIGHT / 2.0,
                sg[2] * RS_HALF,
                &mut pts,
                k,
            );
            let zf = 1.0 / (pts[k + 2] + RS_DISTANCE);
            min_x = min_x.min(pts[k] * zf);
            max_x = max_x.max(pts[k] * zf);
            min_y = min_y.min(-pts[k + 1] * zf);
            max_y = max_y.max(-pts[k + 1] * zf);
        }
    }
    let fit =
        ((w - 2.0) / f64::max(1e-6, max_x - min_x)).min((h - 2.0) / f64::max(1e-6, max_y - min_y));
    let ox = w / 2.0 - (min_x + max_x) / 2.0 * fit;
    let oy = h / 2.0 - (min_y + max_y) / 2.0 * fit;

    c.line_width = 1.0;
    for bar in 0..RS_BARS {
        let l = f64::max(lv[bar * 2], lv[bar * 2 + 1]);
        let bh = RS_MIN_HEIGHT + l * (RS_MAX_HEIGHT - RS_MIN_HEIGHT);
        for (ci, sg) in RS_CORNERS.iter().enumerate() {
            let k2 = (bar * 8 + ci) * 3;
            rot(
                &r,
                (bar as f64 - cxs) * RS_SPACING + sg[0] * RS_HALF,
                sg[1] * bh - RS_MAX_HEIGHT / 2.0,
                sg[2] * RS_HALF,
                &mut pts,
                k2,
            );
        }
        c.stroke_style = if l > 0.0 {
            band(ink, f64::max(l, LEVEL_DIM_BELOW))
        } else {
            ink.dim
        };
        c.begin_path();
        for (verts, n) in &RS_FACES {
            let nrx = n[0] * r.cy + n[2] * r.sy;
            let nrz1 = -n[0] * r.sy + n[2] * r.cy;
            let nry = n[1] * r.cp - nrz1 * r.sp;
            let nrz = n[1] * r.sp + nrz1 * r.cp;
            let v0 = (bar * 8 + verts[0]) * 3;
            // The camera sits at z = -RS_DISTANCE looking down +z.
            if nrx * pts[v0] + nry * pts[v0 + 1] + nrz * (pts[v0 + 2] + RS_DISTANCE) >= 0.0 {
                continue;
            }
            for e in 0..=4 {
                let vk = (bar * 8 + verts[e % 4]) * 3;
                let zf2 = 1.0 / (pts[vk + 2] + RS_DISTANCE);
                let px = clamp(ox + pts[vk] * zf2 * fit, 0.5, w - 0.5);
                let py = clamp(oy - pts[vk + 1] * zf2 * fit, 0.5, h - 0.5);
                if e == 0 {
                    c.move_to(px, py);
                } else {
                    c.line_to(px, py);
                }
            }
        }
        c.stroke();
    }
}

fn stipple(c: &mut Scene, f: &Frame, _state: &mut State) {
    let (w, h, ink) = (f.w, f.h, f.ink);
    let n = ink.columns;
    let lv = fold(f.levels, n);
    let cw = col_width(w, n, ink.gap);
    let pitch = 2.0;
    let dots_x = ((cw + 1.0) / pitch).floor().max(1.0) as usize;
    let rows = ((h + 1.0) / pitch).floor().max(1.0);
    let inset = (cw - ((dots_x as f64 - 1.0) * pitch + 1.0)) / 2.0;
    for i in 0..n {
        let x = i as f64 * (cw + ink.gap) + inset;
        let lit = round(lv[i] * rows);
        let mut k = 0.0;
        while k < lit.max(1.0) {
            c.fill_style = if lit == 0.0 {
                ink.groove
            } else {
                band(ink, (k + 1.0) / rows)
            };
            let y = h - 1.0 - k * pitch;
            for d in 0..dots_x {
                c.fill_rect(round(x + d as f64 * pitch), y, 1.0, 1.0);
            }
            k += 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::spectrum::scene::{PathEl, Shape};

    const BOX_W: f64 = 94.0;
    const BOX_H: f64 = 32.0;

    fn ink() -> Ink {
        Ink {
            groove: Rgba::parse("#111111").unwrap(),
            dim: Rgba::parse("#555555").unwrap(),
            content: Rgba::parse("#eeeeee").unwrap(),
            accent: Rgba::parse("#ff8800").unwrap(),
            mono: "monospace".into(),
            columns: 12,
            gap: 2.0,
            radius: 4.0,
        }
    }

    fn frame(value: impl Fn(usize) -> f64) -> Vec<f64> {
        (0..BAR_COUNT).map(value).collect()
    }

    // Every coordinate a shape hands the renderer, in call order.
    fn points(scene: &Scene) -> Vec<(f64, f64, &'static str)> {
        let mut out = Vec::new();
        for shape in &scene.shapes {
            match shape {
                Shape::Rect { x, y, w, h, .. } => {
                    out.push((*x, *y, "fillRect"));
                    out.push((x + w, y + h, "fillRect end"));
                }
                Shape::Fill { path, .. } | Shape::Stroke { path, .. } => {
                    for el in path {
                        match *el {
                            PathEl::MoveTo(x, y) => out.push((x, y, "moveTo")),
                            PathEl::LineTo(x, y) => out.push((x, y, "lineTo")),
                            PathEl::QuadTo(cx, cy, x, y) => {
                                out.push((cx, cy, "quadCtl"));
                                out.push((x, y, "quadTo"));
                            }
                            PathEl::Arc { cx, cy, r, .. } => {
                                out.push((cx - r, cy - r, "arc"));
                                out.push((cx + r, cy + r, "arc end"));
                            }
                            PathEl::RoundedRect { x, y, w, h, .. } => {
                                out.push((x, y, "roundedRect"));
                                out.push((x + w, y + h, "roundedRect end"));
                            }
                            PathEl::Close => {}
                        }
                    }
                }
                Shape::Text { text, x, y, .. } => {
                    assert!(!text.is_empty(), "fillText without a glyph");
                    out.push((*x, *y, "fillText"));
                }
            }
        }
        out
    }

    fn fills(scene: &Scene) -> Vec<Rgba> {
        scene
            .shapes
            .iter()
            .filter_map(|s| {
                if let Shape::Rect { color, .. } = s {
                    Some(*color)
                } else {
                    None
                }
            })
            .collect()
    }

    fn check_inside(id: &str, label: &str, scene: &Scene) {
        let eps = 0.001;
        for (x, y, what) in points(scene) {
            assert!(
                x.is_finite() && y.is_finite(),
                "{id} {label}: non-finite {what} {x},{y}"
            );
            assert!(
                x >= -eps && x <= BOX_W + eps && y >= -eps && y <= BOX_H + eps,
                "{id} {label}: {what} outside the box at {x},{y}"
            );
        }
        assert_eq!(
            scene.global_alpha, 1.0,
            "{id} {label} leaves globalAlpha at 1"
        );
    }

    fn draw_one(id: &str, levels: &[f64], state: &mut State, dt: f64) -> Scene {
        let mut scene = Scene::new();
        draw(
            Style::parse(id).expect("a known id"),
            &mut scene,
            BOX_W,
            BOX_H,
            levels,
            state,
            &ink(),
            dt,
            None,
            None,
        );
        scene
    }

    #[test]
    fn every_style_stays_inside_the_box() {
        let inputs: Vec<(&str, Vec<f64>)> = vec![
            ("silence", frame(|_| 0.0)),
            ("flat loud", frame(|_| 1.0)),
            ("ramp", frame(|i| i as f64 / (BAR_COUNT - 1) as f64)),
            (
                "garbage",
                frame(|i| match i % 3 {
                    0 => f64::NAN,
                    1 => 3.0,
                    _ => -1.0,
                }),
            ),
        ];
        let steps = [0.0, 0.016, 0.016, 0.05, 0.5, 0.016];
        for id in Style::ids() {
            for (label, levels) in &inputs {
                let mut state = State::with_seed(7);
                // Several frames loud, then the input under test, so falling
                // caps and live particles are exercised on the way down too.
                for _ in 0..20 {
                    let scene = draw_one(id, &frame(|_| 1.0), &mut state, 0.016);
                    check_inside(id, "warm-up", &scene);
                }
                for dt in steps {
                    let scene = draw_one(id, levels, &mut state, dt);
                    check_inside(id, label, &scene);
                }
            }
        }
    }

    #[test]
    fn silence_from_a_fresh_state_draws_something_still() {
        for id in Style::ids() {
            let mut state = State::with_seed(7);
            let a = draw_one(id, &frame(|_| 0.0), &mut state, 0.016);
            let b = draw_one(id, &frame(|_| 0.0), &mut state, 0.016);
            assert!(!points(&a).is_empty(), "{id} draws a resting state");
            assert_eq!(points(&b), points(&a), "{id} does not move in silence");
        }
    }

    #[test]
    fn stereo_draws_each_channel_on_its_own() {
        let loud = frame(|_| 0.9);
        let quiet = frame(|_| 0.1);
        let run = |left: Option<&[f64]>, right: Option<&[f64]>| {
            let mut scene = Scene::new();
            draw(
                Style::parse("stereo").unwrap(),
                &mut scene,
                BOX_W,
                BOX_H,
                &loud,
                &mut State::with_seed(7),
                &ink(),
                0.016,
                left,
                right,
            );
            scene
        };
        let a = run(Some(&loud), Some(&quiet));
        let b = run(Some(&quiet), Some(&loud));
        let c = run(None, None);
        check_inside("stereo", "split", &a);
        assert_ne!(
            fills(&a),
            fills(&b),
            "swapping the channels changes the meters"
        );
        assert_ne!(
            fills(&a),
            fills(&c),
            "a missing channel pair falls back to the mix"
        );
    }

    #[test]
    fn a_seeded_state_replays() {
        let loud = frame(|i| (i % 5) as f64 / 5.0 + 0.1);
        for id in ["flame", "sand", "bubbles", "firework", "geyser"] {
            let run = || {
                let mut state = State::with_seed(42);
                let mut last = Scene::new();
                for _ in 0..30 {
                    last = draw_one(id, &loud, &mut state, 0.016);
                }
                last.shapes
            };
            assert_eq!(run(), run(), "{id} is not deterministic under a seed");
        }
    }
}
