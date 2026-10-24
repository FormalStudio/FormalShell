//! The cartridge boot (spec C6) as a function of time, read off the
//! signed-off preview's keyframes: the grid's lines growing in from the
//! edges at seeded speeds, two emblem rings spinning up from edge-on on the
//! spring, the wordmark's letters stepping in from scattered offsets; on
//! the way out the emblem crumples while the black lifts. Pure: the
//! surfaces keep the clocks and build the shapes (`surfaces::boot`).
//!
//! Every length is in the preview's stage pixels, a 1280 by 800 screen;
//! the surfaces scale them to their own.

use crate::motion::Curve;

/// The preview's `--ease`, on every move but the rings'.
pub const EASE: Curve = Curve::new(&[0.22, 1.0, 0.36, 1.0, 1.0, 1.0]);
/// CSS `ease`, which the lines fade out on.
const CSS_EASE: Curve = Curve::new(&[0.25, 0.1, 0.25, 1.0, 1.0, 1.0]);
/// The preview's `--spring`: CSS `linear()` peaking at 1.42.
pub const SPRING: Curve = Curve::Stops(&[
    (0.0, 0.0),
    (0.009, 0.044),
    (0.02, 0.198),
    (0.068, 1.212),
    (0.081, 1.361),
    (0.087, 1.398),
    (0.094, 1.417),
    (0.104, 1.402),
    (0.115, 1.34),
    (0.164, 0.91),
    (0.176, 0.852),
    (0.189, 0.826),
    (0.199, 0.831),
    (0.21, 0.856),
    (0.26, 1.038),
    (0.272, 1.062),
    (0.285, 1.073),
    (0.306, 1.06),
    (0.356, 0.984),
    (0.38, 0.97),
    (0.476, 1.013),
    (0.571, 0.995),
    (0.666, 1.002),
    (1.0, 1.0),
]);

/// The stage every length here is drawn on.
pub const STAGE: (f64, f64) = (1280.0, 800.0);

/// The two rings: radii to the middle of the stroke, the stroke, and when
/// each starts turning. The outer one is the preview's 520 by 220 box, the
/// inner one inset 26 by 40.
pub const RINGS: [Ring; 2] = [
    Ring { rx: 258.5, ry: 108.5, stroke: 3.0, at: 500.0 },
    Ring { rx: 219.0, ry: 83.0, stroke: 2.0, at: 620.0 },
];
const RING_MS: f64 = 1100.0;
/// The rings come up from edge-on, turned against the clock.
const RING_TILT: f64 = 89.0;
const RING_SPIN: f64 = -30.0;

/// The wordmark's rows: their size, the line height they stack at, and
/// how wide the widest may run inside the inner ring.
pub const ROW_SIZES: [f64; 2] = [64.0, 50.0];
pub const ROW_LEADING: f64 = 0.86;
pub const WORD_ROOM: f64 = 380.0;
const LETTERS_AT: f64 = 1100.0;
const ROW_STEP: f64 = 250.0;
const LETTER_STEP: f64 = 70.0;
const LETTER_MS: f64 = 500.0;
const LETTER_STAIRS: f64 = 3.0;

/// How long the assembled emblem holds before the session's boot leaves
/// on its own.
pub const HOLD: f64 = 600.0;
/// The way out: the emblem crumples, the lines fade, the black lifts.
pub const CRUMPLE_MS: f64 = 600.0;
pub const LINES_OUT_MS: f64 = 600.0;
pub const LIFT_MS: f64 = 900.0;
/// The greeter's emblem moving up over the login card, the wallpaper
/// coming up under it, the card's own way in and its prompt typing.
const SETTLE_MS: f64 = 700.0;
const WORLD_MS: f64 = 900.0;
const CARD_AT: f64 = 200.0;
const CARD_MS: f64 = 600.0;
const CARD_RISE: f64 = 40.0;
const TYPE_MS: f64 = 600.0;
/// The card's way out once the session is on its way.
const CARD_OUT_MS: f64 = 400.0;
/// The emblem's size over the card against its size on the stage.
pub const PERCH: f64 = 0.6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub rx: f64,
    pub ry: f64,
    pub stroke: f64,
    pub at: f64,
}

/// One grid line's way in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// Grows from the far end: up from the bottom, never right to left.
    pub from_end: bool,
    pub delay: f64,
    pub ms: f64,
}

/// One letter's scatter, in stage pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Letter {
    pub dx: f64,
    pub dy: f64,
}

/// One boot's dice: every line's speed, every letter's scatter.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub lines: Vec<Line>,
    pub rows: Vec<Vec<Letter>>,
}

/// xorshift64*: the same plan every run for a seed, so frames compare.
struct Dice(u64);

impl Dice {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub const SEED: u64 = 0x5eed_b007_c0de_0001;

impl Plan {
    /// `vertical` is the grid's lines in order, true for a column; `rows`
    /// the letters in each row of the wordmark.
    pub fn new(vertical: &[bool], rows: &[usize], seed: u64) -> Self {
        let mut d = Dice(seed.max(1));
        let lines = vertical
            .iter()
            .map(|&v| {
                if v {
                    let from_end = d.next() < 0.5;
                    Line { from_end, delay: d.next() * 400.0, ms: 500.0 + d.next() * 900.0 }
                } else {
                    Line { from_end: false, delay: 300.0 + d.next() * 400.0, ms: 600.0 + d.next() * 800.0 }
                }
            })
            .collect();
        let rows = rows.iter().map(|&n| (0..n).map(|_| Letter { dx: (d.next() - 0.5) * 120.0, dy: (d.next() - 0.5) * 80.0 }).collect()).collect();
        Self { lines, rows }
    }

    /// How much of line `i` has grown in at `t`, 0 to 1.
    pub fn grown(&self, i: usize, t: f64) -> f64 {
        self.lines.get(i).map_or(1.0, |l| EASE.ease(progress(t - l.delay, l.ms)))
    }

    /// When the last letter lands.
    pub fn assembled(&self) -> f64 {
        let last = self
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_empty())
            .map(|(row, r)| letter_at(row, r.len() - 1))
            .fold(LETTERS_AT, f64::max);
        let lines = self.lines.iter().map(|l| l.delay + l.ms).fold(0.0, f64::max);
        let rings = RINGS.iter().map(|r| r.at + RING_MS).fold(0.0, f64::max);
        (last + LETTER_MS).max(lines).max(rings)
    }

    /// Letter `i` of row `row` at `t`: its opacity and how far it still
    /// sits from its place.
    pub fn letter(&self, row: usize, i: usize, t: f64) -> (f32, f64, f64) {
        let Some(l) = self.rows.get(row).and_then(|r| r.get(i)) else { return (1.0, 0.0, 0.0) };
        let p = progress(t - letter_at(row, i), LETTER_MS);
        // steps(3) with the `both` fill: held on the scatter until it starts.
        let q = if p >= 1.0 { 1.0 } else if t < letter_at(row, i) { 0.0 } else { (p * LETTER_STAIRS).floor() / LETTER_STAIRS };
        (q as f32, l.dx * (1.0 - q), l.dy * (1.0 - q))
    }
}

fn letter_at(row: usize, i: usize) -> f64 {
    LETTERS_AT + row as f64 * ROW_STEP + i as f64 * LETTER_STEP
}

/// A ring's pose at `t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// What the turn about x leaves of the ring's height, 0 edge-on.
    pub squash: f64,
    /// The turn about the screen's normal, radians, clockwise on screen.
    pub spin: f64,
    pub alpha: f32,
}

/// `ringIn`: from rotateX(89deg) rotateZ(-30deg) and clear, the opacity
/// up by a fifth of the way in, the whole move on the spring.
pub fn ring(r: &Ring, t: f64) -> Pose {
    if t < r.at {
        return Pose { squash: RING_TILT.to_radians().cos(), spin: RING_SPIN.to_radians(), alpha: 0.0 };
    }
    let p = progress(t - r.at, RING_MS);
    let s = SPRING.ease(p);
    let alpha = SPRING.ease((p / 0.2).min(1.0)).clamp(0.0, 1.0) as f32;
    Pose { squash: (RING_TILT * (1.0 - s)).to_radians().cos().abs(), spin: (RING_SPIN * (1.0 - s)).to_radians(), alpha }
}

/// `crumple` `ms` into the way out: the emblem's scale across and down,
/// its skew (the tangent `skewX` shears by) and its opacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crumple {
    pub sx: f64,
    pub sy: f64,
    pub skew: f64,
    pub alpha: f32,
}

pub fn crumple(ms: f64) -> Crumple {
    let p = progress(ms, CRUMPLE_MS);
    // Keyframes at 40% and 100%, the ease run between each pair; the
    // opacity has only its last one, so it runs the whole way.
    let (from, to, e) = if p < 0.4 { ((1.0, 1.0, 0.0), (1.15, 0.35, -18.0), EASE.ease(p / 0.4)) } else { ((1.15, 0.35, -18.0), (1.6, 0.02, -30.0), EASE.ease((p - 0.4) / 0.6)) };
    let mix = |a: f64, b: f64| a + (b - a) * e;
    let skew: f64 = mix(from.2, to.2);
    Crumple { sx: mix(from.0, to.0), sy: mix(from.1, to.1), skew: skew.to_radians().tan(), alpha: (1.0 - EASE.ease(p)).clamp(0.0, 1.0) as f32 }
}

/// The black's opacity `ms` into the way out.
pub fn lift(ms: f64) -> f32 {
    (1.0 - EASE.ease(progress(ms, LIFT_MS))).clamp(0.0, 1.0) as f32
}

/// The lines' opacity `ms` into their fade.
pub fn lines_out(ms: f64) -> f32 {
    (1.0 - CSS_EASE.ease(progress(ms, LINES_OUT_MS))).clamp(0.0, 1.0) as f32
}

/// The way out is over `ms` in.
pub fn gone(ms: f64) -> bool {
    ms >= LIFT_MS.max(CRUMPLE_MS).max(LINES_OUT_MS)
}

/// Where the greeter stands past the way in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Greet {
    /// The emblem's move up over the card, 0 to 1.
    pub settle: f64,
    /// The wallpaper under everything, the boot's lines, and the card.
    pub world: f32,
    pub lines: f32,
    pub card: f32,
    /// How far under its place the card still sits.
    pub rise: f64,
    /// How far the field's prompt has typed, 0 to 1, before its stairs.
    pub typed: f64,
    pub moving: bool,
}

/// The greeter `settled` ms after its emblem started up (`None` while it
/// is still assembling) and `out` ms into the session's way in.
pub fn greet(settled: Option<f64>, out: Option<f64>) -> Greet {
    let Some(s) = settled else {
        return Greet { settle: 0.0, world: 0.0, lines: 1.0, card: 0.0, rise: CARD_RISE, typed: 0.0, moving: true };
    };
    let card = EASE.ease(progress(s - CARD_AT, CARD_MS));
    let mut g = Greet {
        settle: EASE.ease(progress(s, SETTLE_MS)),
        world: EASE.ease(progress(s, WORLD_MS)) as f32,
        lines: lines_out(s - WORLD_MS),
        card: card as f32,
        rise: CARD_RISE * (1.0 - card),
        typed: progress(s - CARD_AT, TYPE_MS),
        moving: s < (CARD_AT + CARD_MS.max(TYPE_MS)).max(WORLD_MS + LINES_OUT_MS).max(SETTLE_MS),
    };
    if let Some(o) = out {
        g.world *= lift(o);
        g.lines = 0.0;
        g.card *= (1.0 - EASE.ease(progress(o, CARD_OUT_MS))) as f32;
        g.moving = !gone(o);
    }
    g
}

/// `elapsed` over `ms`, 0 to 1.
pub fn progress(elapsed: f64, ms: f64) -> f64 {
    if ms <= 0.0 { if elapsed >= 0.0 { 1.0 } else { 0.0 } } else { (elapsed / ms).clamp(0.0, 1.0) }
}

/// The login the boot plays once for: this boot of the machine and the
/// compositor instance the shell runs under, so a shell restarted in the
/// same session does not play it again and the next login does.
pub fn login_key(boot_id: &str, instance: &str) -> String {
    format!("{}/{}", boot_id.trim(), instance.trim())
}

/// This machine's own login key, read off the kernel and the environment.
pub fn this_login() -> String {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap_or_default();
    let instance = ["HYPRLAND_INSTANCE_SIGNATURE", "MANGO_INSTANCE_SIGNATURE", "XDG_SESSION_ID"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    login_key(&boot_id, &instance)
}

/// The machine's name for the wordmark, in caps; "FORMAL" when it has
/// none.
pub fn hostname() -> String {
    let name = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default();
    let name = name.trim();
    if name.is_empty() { "FORMAL".into() } else { name.chars().take(16).collect::<String>().to_uppercase() }
}

/// The greetd session the greeter starts carries this, so the session's
/// own boot picks up where the greeter's left off instead of playing again.
pub const HANDOFF_ENV: &str = "FORMALSHELL_BOOT";
pub const HANDOFF: &str = "greeter";

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        Plan::new(&[true, true, true, false, false], &[4, 6], SEED)
    }

    #[test]
    fn the_dice_are_the_same_every_run_and_in_range() {
        let (a, b) = (plan(), plan());
        assert_eq!(a, b);
        for l in &a.lines[..3] {
            assert!(l.delay >= 0.0 && l.delay < 400.0 && l.ms >= 500.0 && l.ms < 1400.0, "{l:?}");
        }
        for l in &a.lines[3..] {
            assert!(!l.from_end && l.delay >= 300.0 && l.delay < 700.0 && l.ms >= 600.0 && l.ms < 1400.0, "{l:?}");
        }
        assert!(a.rows.iter().flatten().all(|l| l.dx.abs() <= 60.0 && l.dy.abs() <= 40.0));
        assert_ne!(Plan::new(&[true; 5], &[4], 7), Plan::new(&[true; 5], &[4], 8));
    }

    #[test]
    fn lines_grow_from_nothing_to_whole() {
        let p = plan();
        for i in 0..p.lines.len() {
            assert_eq!(p.grown(i, 0.0), 0.0);
            assert_eq!(p.grown(i, 2100.0), 1.0);
        }
        let l = p.lines[3];
        let mid = p.grown(3, l.delay + l.ms / 2.0);
        assert!(mid > 0.5 && mid < 1.0, "{mid}");
    }

    #[test]
    fn rings_spin_up_from_edge_on_and_overshoot() {
        let r = &RINGS[0];
        let before = ring(r, 0.0);
        assert_eq!(before.alpha, 0.0);
        assert!(before.squash < 0.02);
        let rest = ring(r, r.at + RING_MS);
        assert!((rest.squash - 1.0).abs() < 1e-9 && rest.spin.abs() < 1e-9 && rest.alpha == 1.0);
        // The spring carries the spin past its rest before it settles.
        let over = ring(r, r.at + 0.094 * RING_MS);
        assert!(over.spin > 0.0, "{over:?}");
        assert_eq!(ring(r, r.at + 0.2 * RING_MS).alpha, 1.0);
    }

    #[test]
    fn letters_step_in_three_stairs_in_order() {
        let p = plan();
        assert_eq!(p.letter(0, 0, 0.0).0, 0.0);
        let first = letter_at(0, 0);
        assert_eq!(p.letter(0, 0, first + 10.0).0, 0.0);
        let (a, dx, _) = p.letter(0, 0, first + LETTER_MS / 2.0);
        assert!((a - 1.0 / 3.0).abs() < 1e-6, "{a}");
        assert!((dx - p.rows[0][0].dx * 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(p.letter(0, 0, first + LETTER_MS), (1.0, 0.0, 0.0));
        // The second row starts after the first's first letter.
        assert_eq!(p.letter(1, 0, first + 100.0).0, 0.0);
        let done = p.assembled();
        assert!(done >= letter_at(1, 5) + LETTER_MS, "{done}");
        for row in 0..2 {
            for i in 0..p.rows[row].len() {
                assert_eq!(p.letter(row, i, done), (1.0, 0.0, 0.0));
            }
        }
    }

    #[test]
    fn the_way_out_crumples_and_lifts() {
        assert_eq!(crumple(0.0), Crumple { sx: 1.0, sy: 1.0, skew: 0.0, alpha: 1.0 });
        let mid = crumple(0.4 * CRUMPLE_MS);
        assert!((mid.sx - 1.15).abs() < 1e-9 && (mid.sy - 0.35).abs() < 1e-9 && mid.skew < 0.0);
        let end = crumple(CRUMPLE_MS);
        assert!((end.sy - 0.02).abs() < 1e-9 && end.alpha == 0.0);
        assert_eq!(lift(0.0), 1.0);
        assert_eq!(lift(LIFT_MS), 0.0);
        assert_eq!(lines_out(LINES_OUT_MS), 0.0);
        assert!(!gone(LIFT_MS - 1.0) && gone(LIFT_MS));
    }

    #[test]
    fn the_greeter_brings_its_card_up_under_the_emblem_and_hands_over_on_black() {
        let before = greet(None, None);
        assert_eq!((before.world, before.card, before.lines), (0.0, 0.0, 1.0));
        assert!(before.moving);
        let early = greet(Some(CARD_AT), None);
        assert!(early.settle > 0.0 && early.card == 0.0 && early.rise == CARD_RISE);
        let rest = greet(Some(5000.0), None);
        assert_eq!((rest.settle, rest.world, rest.lines, rest.card, rest.rise, rest.typed), (1.0, 1.0, 0.0, 1.0, 0.0, 1.0));
        assert!(!rest.moving);
        let leaving = greet(Some(5000.0), Some(LIFT_MS));
        assert_eq!((leaving.world, leaving.card), (0.0, 0.0));
        assert!(!leaving.moving);
        assert!(greet(Some(5000.0), Some(10.0)).moving);
    }

    #[test]
    fn the_login_key_names_the_boot_and_the_instance() {
        assert_eq!(login_key("abc\n", "sig"), "abc/sig");
        assert_ne!(login_key("abc", "one"), login_key("abc", "two"));
    }
}
