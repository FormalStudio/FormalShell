//! Pure banner-effect state stepping: every function is a deterministic
//! function of (column, row, frame, banner), with no clock and no RNG, so the
//! renderer stays a thin per-frame layer and the whole animation is testable
//! frame by frame. The banner is the entire subject: nothing is ever drawn
//! outside its own width/height grid, in any effect.
//!
//! Columns and widths count Unicode scalar values.

/// Plain ASCII so scramble/noise glyphs render identically in any monospace
/// font, regardless of the font's own glyph coverage.
const NOISE_CHARSET: &str = "01ABCDEFGHIJKLMNOPQRSTUVWXYZ!@#$%^&*<>/\\|+=";

pub const EFFECT_NAMES: [&str; 5] = ["decrypt", "rain", "expand", "slide", "scatter"];

#[derive(Debug, Clone, PartialEq)]
pub struct Banner {
    pub width: usize,
    pub height: usize,
    pub rows: Vec<String>,
    cells: Vec<Vec<char>>,
}

/// Splits raw banner text into an equal-width grid of rows: trims one
/// trailing newline and space-pads every row to the widest row's length, so
/// every effect can address (col, row) without its own bounds-checking.
pub fn parse_banner(text: &str) -> Banner {
    let trimmed = text.strip_suffix('\n').unwrap_or(text);
    let raw: Vec<Vec<char>> = trimmed.split('\n').map(|r| r.chars().collect()).collect();
    let width = raw.iter().map(Vec::len).max().unwrap_or(0);
    let cells: Vec<Vec<char>> = raw
        .into_iter()
        .map(|mut row| {
            row.resize(width, ' ');
            row
        })
        .collect();
    Banner {
        width,
        height: cells.len(),
        rows: cells.iter().map(|r| r.iter().collect()).collect(),
        cells,
    }
}

pub fn target_char(banner: &Banner, col: i64, row: i64) -> char {
    if row < 0 || row as usize >= banner.height || col < 0 || col as usize >= banner.width {
        return ' ';
    }
    banner.cells[row as usize][col as usize]
}

/// Deterministic per-cell pseudo-random integer in [0, modulus). Every
/// effect's apparent randomness (stagger timing, arrival order) is this hash,
/// so a whole activation is a pure function of frame.
fn hash(col: i64, row: i64, salt: i64, modulus: i64) -> i64 {
    let h = ((col * 374761393 + row * 668265263 + salt * 2246822519) % 1000003).abs();
    h % modulus
}

fn noise_char(index: i64) -> char {
    let chars: Vec<char> = NOISE_CHARSET.chars().collect();
    chars[(index % chars.len() as i64).unsigned_abs() as usize]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cell {
    pub ch: char,
    pub opacity: f64,
}

const BLANK: Cell = Cell { ch: ' ', opacity: 0.0 };

fn shown(ch: char) -> Cell {
    Cell { ch, opacity: 1.0 }
}

// decrypt: static noise resolving to the banner, no direction
// Every non-space cell flickers through the noise charset until its own
// (hashed, scattered) reveal frame, then holds its target forever.

/// Every cell's reveal frame is below this, so it bounds convergence.
const DECRYPT_SPAN: i64 = 40;

fn decrypt_cell(col: i64, row: i64, frame: i64, banner: &Banner) -> Cell {
    let target = target_char(banner, col, row);
    if target == ' ' {
        return BLANK;
    }
    let reveal_at = hash(col, row, 11, DECRYPT_SPAN);
    if frame >= reveal_at {
        return shown(target);
    }
    Cell { ch: noise_char(col * 13 + row * 29 + frame * 5), opacity: 0.85 }
}

// rain: falling trail that locks each cell once it passes over
// A per-column head falls monotonically (no wrap: this one must finish)
// leaving a short fading trail; once the head passes a row for the first
// time that cell locks to its target for good.

const RAIN_TRAIL: i64 = 3;

// Frames per row, not rows per frame: the banner is only a handful of rows
// tall, so a head moving a whole row every frame would settle almost
// instantly.
fn rain_column_period(col: i64) -> i64 {
    6 + (col % 5)
}

fn rain_column_start_delay(col: i64) -> i64 {
    (col * 13) % 24
}

fn rain_cell(col: i64, row: i64, frame: i64, banner: &Banner) -> Cell {
    let target = target_char(banner, col, row);
    if target == ' ' {
        return BLANK;
    }
    let elapsed = frame - rain_column_start_delay(col);
    if elapsed < 0 {
        return BLANK;
    }
    let head = elapsed as f64 / rain_column_period(col) as f64;
    if head >= row as f64 {
        return shown(target);
    }
    let behind = row as f64 - head;
    if behind <= RAIN_TRAIL as f64 {
        return Cell {
            ch: noise_char(col * 3 + row * 5 + frame * 7),
            opacity: 1.0 - behind / (RAIN_TRAIL + 1) as f64,
        };
    }
    BLANK
}

fn rain_convergence_frame(banner: &Banner) -> i64 {
    let mut worst = 0;
    for col in 0..banner.width as i64 {
        let needed = rain_column_start_delay(col) + rain_column_period(col) * (banner.height as i64 - 1);
        worst = worst.max(needed);
    }
    worst + RAIN_TRAIL + 2
}

// expand: reveal opens outward from the centre
// A cell is either not-yet-open (blank) or open (its target): the open
// region is a diamond growing outward from the banner's centre, doubling
// row-distance to compensate for glyphs being taller than wide.

const EXPAND_SPEED: i64 = 2;

fn expand_distance(col: i64, row: i64, banner: &Banner) -> f64 {
    let cx = (banner.width as f64 - 1.0) / 2.0;
    let cy = (banner.height as f64 - 1.0) / 2.0;
    (col as f64 - cx).abs().max((row as f64 - cy).abs() * 2.0)
}

fn expand_cell(col: i64, row: i64, frame: i64, banner: &Banner) -> Cell {
    let target = target_char(banner, col, row);
    if target == ' ' {
        return BLANK;
    }
    let reveal_at = expand_distance(col, row, banner).ceil() as i64 * EXPAND_SPEED;
    if frame >= reveal_at {
        return shown(target);
    }
    BLANK
}

fn expand_convergence_frame(banner: &Banner) -> i64 {
    let mut max_dist = 0.0f64;
    for row in 0..banner.height as i64 {
        for col in 0..banner.width as i64 {
            max_dist = max_dist.max(expand_distance(col, row, banner));
        }
    }
    max_dist.ceil() as i64 * EXPAND_SPEED + 1
}

// slide: rows wipe in from alternating edges
// Even rows sweep open left to right, odd rows right to left, rows staggered
// so the banner assembles in a visible zigzag.

const SLIDE_ROW_STAGGER: i64 = 6;
const SLIDE_STEP: i64 = 1;

fn slide_cell(col: i64, row: i64, frame: i64, banner: &Banner) -> Cell {
    let target = target_char(banner, col, row);
    if target == ' ' {
        return BLANK;
    }
    let from_left = row % 2 == 0;
    let sweep_pos = if from_left { col } else { banner.width as i64 - 1 - col };
    let reveal_at = row * SLIDE_ROW_STAGGER + sweep_pos * SLIDE_STEP;
    if frame >= reveal_at {
        return shown(target);
    }
    BLANK
}

fn slide_convergence_frame(banner: &Banner) -> i64 {
    (banner.height as i64 - 1) * SLIDE_ROW_STAGGER + (banner.width as i64 - 1) * SLIDE_STEP + 1
}

// scatter: individual glyphs pop in at scattered moments
// Unlike decrypt's dense noise, scatter's canvas is mostly blank: each cell
// lands at its own hashed arrival frame, fading in over a short final
// approach instead of flickering through unrelated glyphs first.

const SCATTER_SPAN: i64 = 50;
const SCATTER_APPROACH: i64 = 4;

fn scatter_cell(col: i64, row: i64, frame: i64, banner: &Banner) -> Cell {
    let target = target_char(banner, col, row);
    if target == ' ' {
        return BLANK;
    }
    let arrive_at = hash(col, row, 29, SCATTER_SPAN);
    if frame >= arrive_at {
        return shown(target);
    }
    let until_arrival = arrive_at - frame;
    if until_arrival <= SCATTER_APPROACH {
        return Cell { ch: target, opacity: 0.2 + 0.2 * (SCATTER_APPROACH - until_arrival) as f64 };
    }
    BLANK
}

// registry

type CellFn = fn(i64, i64, i64, &Banner) -> Cell;

fn cell_fn(name: &str) -> Option<CellFn> {
    Some(match name {
        "decrypt" => decrypt_cell,
        "rain" => rain_cell,
        "expand" => expand_cell,
        "slide" => slide_cell,
        "scatter" => scatter_cell,
        _ => return None,
    })
}

pub fn is_known_effect(name: &str) -> bool {
    EFFECT_NAMES.contains(&name)
}

/// "random" or any unrecognised name deterministically falls back to a pick
/// keyed on `seed` (the caller supplies a fresh seed per activation, so a
/// long idle session still cycles variants).
pub fn resolve_effect_name(requested: &str, seed: i64) -> &str {
    if is_known_effect(requested) {
        return EFFECT_NAMES.iter().copied().find(|n| *n == requested).unwrap();
    }
    EFFECT_NAMES[(seed.unsigned_abs() % EFFECT_NAMES.len() as u64) as usize]
}

/// The number of frames after which `name` is guaranteed fully converged
/// (every non-space cell showing its target character) for the given banner.
pub fn convergence_frame(name: &str, banner: &Banner) -> i64 {
    match name {
        "decrypt" => DECRYPT_SPAN,
        "rain" => rain_convergence_frame(banner),
        "expand" => expand_convergence_frame(banner),
        "slide" => slide_convergence_frame(banner),
        "scatter" => SCATTER_SPAN,
        _ => convergence_frame(resolve_effect_name(name, 0), banner),
    }
}

/// The full per-frame grid for `name`: `banner.height` rows of `banner.width`
/// cells, rendered directly at a fixed on-screen offset with no further
/// per-effect logic on the renderer's side.
pub fn frame_state(name: &str, frame: i64, banner: &Banner) -> Vec<Vec<Cell>> {
    let f = cell_fn(name).or_else(|| cell_fn(resolve_effect_name(name, 0))).unwrap();
    (0..banner.height as i64)
        .map(|row| (0..banner.width as i64).map(|col| f(col, row, frame, banner)).collect())
        .collect()
}

// continuous cycling
// After an effect converges the controller holds the finished banner for
// screensaver.holdSeconds, then rerolls and animates again, indefinitely.

/// How many auto-timer ticks the converged banner holds before the reroll.
/// Never less than one frame, so `hold_seconds` 0 still yields a rendered
/// converged banner instead of an instant reroll mid-paint.
pub fn hold_frames(hold_seconds: f64, tick_ms: f64) -> i64 {
    ((hold_seconds * 1000.0 / tick_ms).ceil() as i64).max(1)
}

/// The next cycle's effect. A known (pinned) name replays itself; "random"
/// (or any unknown name) picks from every effect except the immediately
/// previous one, so consecutive cycles never repeat.
pub fn reroll_effect_name(requested: &str, previous_effect: &str, seed: i64) -> String {
    if is_known_effect(requested) {
        return requested.to_string();
    }
    let pool: Vec<&str> = EFFECT_NAMES.iter().copied().filter(|n| *n != previous_effect).collect();
    pool[(seed.unsigned_abs() % pool.len() as u64) as usize].to_string()
}

// frame-pin resolution
// The decision logic behind the deterministic frame pin: which counter
// renders, whether the free-running timer ticks, and what the pin resolves
// to across an active/inactive transition.

/// -1 means "not pinned": the free-running counter renders.
pub fn resolve_render_frame(pinned_frame: i64, auto_frame: i64) -> i64 {
    if pinned_frame >= 0 { pinned_frame } else { auto_frame }
}

/// The free-run timer only ticks while nothing is pinned. `active` is the
/// screensaver's own activation, not its surface's mapped state: the surface
/// outlives deactivation by one exit fade, and the animation freezes at the
/// start of that fade.
pub fn auto_timer_should_run(active: bool, pinned_frame: i64) -> bool {
    active && pinned_frame < 0
}

/// Any deactivation releases a stale pin so the next activation free-runs
/// from frame 0.
pub fn next_pinned_frame(active: bool, pinned_frame: i64) -> i64 {
    if active { pinned_frame } else { -1 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn banner() -> Banner {
        parse_banner("FS\nOK")
    }

    // parseBanner

    #[test]
    fn parse_banner_pads_short_rows_to_widest_row() {
        let b = parse_banner("AB\nC");
        assert_eq!(b.width, 2);
        assert_eq!(b.height, 2);
        assert_eq!(b.rows[1], "C ");
    }

    #[test]
    fn parse_banner_trims_one_trailing_newline() {
        let b = parse_banner("AB\n");
        assert_eq!(b.height, 1);
        assert_eq!(b.rows[0], "AB");
    }

    #[test]
    fn target_char_out_of_bounds_is_space() {
        let b = banner();
        assert_eq!(target_char(&b, -1, 0), ' ');
        assert_eq!(target_char(&b, 0, 99), ' ');
    }

    // registry

    #[test]
    fn known_effect_names_are_exactly_five() {
        assert_eq!(EFFECT_NAMES.len(), 5);
        let mut seen = HashSet::new();
        for name in EFFECT_NAMES {
            assert!(is_known_effect(name));
            assert!(seen.insert(name));
        }
    }

    #[test]
    fn resolve_effect_name_passes_through_known_names() {
        for name in EFFECT_NAMES {
            assert_eq!(resolve_effect_name(name, 0), name);
        }
    }

    #[test]
    fn resolve_effect_name_falls_back_for_random_and_unknown() {
        assert!(is_known_effect(resolve_effect_name("random", 3)));
        assert!(is_known_effect(resolve_effect_name("not-a-real-effect", 5)));
    }

    #[test]
    fn resolve_effect_name_is_deterministic_per_seed() {
        assert_eq!(resolve_effect_name("random", 17), resolve_effect_name("random", 17));
    }

    // frameState: shape + bounds, generically over every effect

    #[test]
    fn frame_state_has_exact_row_and_column_counts() {
        let b = banner();
        for name in EFFECT_NAMES {
            let grid = frame_state(name, 5, &b);
            assert_eq!(grid.len(), b.height);
            for row in &grid {
                assert_eq!(row.len(), b.width);
            }
        }
    }

    #[test]
    fn frame_state_is_deterministic_for_same_inputs() {
        let b = banner();
        for name in EFFECT_NAMES {
            assert_eq!(frame_state(name, 12, &b), frame_state(name, 12, &b));
        }
    }

    #[test]
    fn opacity_always_within_0_and_1() {
        let b = banner();
        for name in EFFECT_NAMES {
            for frame in (0..60).step_by(3) {
                for row in frame_state(name, frame, &b) {
                    for cell in row {
                        assert!((0.0..=1.0).contains(&cell.opacity));
                    }
                }
            }
        }
    }

    #[test]
    fn space_target_cells_never_draw_anything() {
        let spaced = parse_banner("A B");
        for name in EFFECT_NAMES {
            for frame in (0..80).step_by(5) {
                let grid = frame_state(name, frame, &spaced);
                assert_eq!(grid[0][1].ch, ' ');
                assert_eq!(grid[0][1].opacity, 0.0);
            }
        }
    }

    // convergence: every effect must reach the finished banner and hold it

    #[test]
    fn every_effect_converges_and_holds() {
        let b = banner();
        for name in EFFECT_NAMES {
            let at = convergence_frame(name, &b);
            for extra in 0..10 {
                let grid = frame_state(name, at + extra, &b);
                for (r, row) in grid.iter().enumerate() {
                    for (c, cell) in row.iter().enumerate() {
                        let expected = target_char(&b, c as i64, r as i64);
                        assert_eq!(cell.ch, expected);
                        assert_eq!(cell.opacity, if expected == ' ' { 0.0 } else { 1.0 });
                    }
                }
            }
        }
    }

    #[test]
    fn every_effect_starts_unconverged() {
        let b = banner();
        for name in EFFECT_NAMES {
            let grid = frame_state(name, 0, &b);
            let any_unresolved = grid.iter().enumerate().any(|(r, row)| {
                row.iter().enumerate().any(|(c, cell)| {
                    let target = target_char(&b, c as i64, r as i64);
                    target != ' ' && cell.ch != target
                })
            });
            assert!(any_unresolved, "{name} should not already be fully converged at frame 0");
        }
    }

    // decrypt

    #[test]
    fn decrypt_noise_glyph_changes_across_frames_before_reveal() {
        let b = parse_banner("XXXXXXXXXX");
        let a = &frame_state("decrypt", 0, &b)[0];
        let c = &frame_state("decrypt", 1, &b)[0];
        assert!((0..a.len()).any(|i| a[i].opacity < 1.0 && a[i].ch != c[i].ch));
    }

    // rain

    #[test]
    fn rain_cell_never_reverts_after_settling() {
        let b = banner();
        let (col, row) = (0usize, b.height - 1);
        let mut settled = None;
        for frame in 0..40 {
            let cell = frame_state("rain", frame, &b)[row][col];
            let target = target_char(&b, col as i64, row as i64);
            if target == ' ' {
                continue;
            }
            if cell.ch == target && cell.opacity == 1.0 {
                settled = Some(frame);
                break;
            }
        }
        let settled = settled.expect("rain never settled");
        for f2 in settled..settled + 20 {
            let cell = frame_state("rain", f2, &b)[row][col];
            assert_eq!(cell.ch, target_char(&b, col as i64, row as i64));
            assert_eq!(cell.opacity, 1.0);
        }
    }

    // expand

    #[test]
    fn expand_centre_opens_before_or_with_corner() {
        let big = parse_banner("XXXXXXXXX\nXXXXXXXXX\nXXXXXXXXX\nXXXXXXXXX\nXXXXXXXXX");
        let cx = (big.width - 1) / 2;
        let cy = (big.height - 1) / 2;
        let mut centre = None;
        let mut corner = None;
        for frame in 0..60 {
            let grid = frame_state("expand", frame, &big);
            if centre.is_none() && grid[cy][cx].opacity > 0.0 {
                centre = Some(frame);
            }
            if corner.is_none() && grid[0][0].opacity > 0.0 {
                corner = Some(frame);
            }
        }
        assert!(centre.unwrap() <= corner.unwrap());
    }

    // slide

    #[test]
    fn slide_alternates_direction_by_row_parity() {
        let wide = parse_banner("XXXXXXXXXX\nXXXXXXXXXX");
        let reveal_frame_of = |row: usize, col: usize| -> i64 {
            (0..60)
                .find(|&f| frame_state("slide", f, &wide)[row][col].opacity > 0.0)
                .unwrap_or(-1)
        };
        let last = wide.width - 1;
        assert!(reveal_frame_of(0, 0) <= reveal_frame_of(0, last));
        assert!(reveal_frame_of(1, last) <= reveal_frame_of(1, 0));
    }

    // scatter

    #[test]
    fn scatter_cells_arrive_at_varied_frames_and_only_show_target_or_blank() {
        let wide = parse_banner("ABCDEFGHIJ");
        let mut arrivals = std::collections::HashMap::new();
        for frame in 0..convergence_frame("scatter", &wide) {
            let row = &frame_state("scatter", frame, &wide)[0];
            for (c, cell) in row.iter().enumerate() {
                let target = target_char(&wide, c as i64, 0);
                assert!(cell.ch == ' ' || cell.ch == target);
                if cell.opacity == 1.0 {
                    arrivals.entry(c).or_insert(frame);
                }
            }
        }
        let distinct: HashSet<i64> = arrivals.values().copied().collect();
        assert!(distinct.len() > 1);
    }

    // continuous cycling

    #[test]
    fn reroll_replays_a_pinned_effect() {
        for name in EFFECT_NAMES {
            assert_eq!(reroll_effect_name(name, name, 123), name);
            assert_eq!(reroll_effect_name(name, "", 456), name);
        }
    }

    #[test]
    fn reroll_random_never_repeats_the_previous_effect() {
        for prev in EFFECT_NAMES {
            for seed in 0..40 {
                let next = reroll_effect_name("random", prev, seed);
                assert!(is_known_effect(&next));
                assert_ne!(next, prev, "reroll repeated '{prev}' at seed {seed}");
            }
        }
    }

    #[test]
    fn reroll_unknown_name_behaves_like_random() {
        for seed in 0..40 {
            assert_ne!(reroll_effect_name("not-a-real-effect", "rain", seed), "rain");
        }
    }

    #[test]
    fn reroll_with_no_previous_effect_reaches_every_effect() {
        let seen: HashSet<String> = (0..40).map(|seed| reroll_effect_name("random", "", seed)).collect();
        assert_eq!(seen.len(), EFFECT_NAMES.len());
    }

    #[test]
    fn reroll_is_deterministic_per_seed() {
        assert_eq!(reroll_effect_name("random", "rain", 17), reroll_effect_name("random", "rain", 17));
    }

    #[test]
    fn hold_frames_matches_seconds_at_the_tick_rate() {
        assert_eq!(hold_frames(6.0, 90.0), 67);
        assert_eq!(hold_frames(2.0, 90.0), 23);
    }

    #[test]
    fn hold_frames_never_drops_below_one_frame() {
        assert_eq!(hold_frames(0.0, 90.0), 1);
    }

    // frame-pin resolution

    #[test]
    fn resolve_render_frame_prefers_the_pin_when_set() {
        assert_eq!(resolve_render_frame(7, 3), 7);
        assert_eq!(resolve_render_frame(0, 3), 0);
    }

    #[test]
    fn resolve_render_frame_falls_back_to_auto_when_unpinned() {
        assert_eq!(resolve_render_frame(-1, 3), 3);
    }

    #[test]
    fn auto_timer_runs_only_while_active_and_unpinned() {
        assert!(auto_timer_should_run(true, -1));
        assert!(!auto_timer_should_run(true, 0));
        assert!(!auto_timer_should_run(false, -1));
    }

    #[test]
    fn pin_is_released_on_deactivate() {
        assert_eq!(next_pinned_frame(false, 12), -1);
    }

    #[test]
    fn pin_survives_while_still_active() {
        assert_eq!(next_pinned_frame(true, 12), 12);
    }
}
