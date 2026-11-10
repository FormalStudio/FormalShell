//! ttfx (github.com/omacom/ttfx, MIT) drives the screensaver's banner
//! animation, the same engine omarchy's own screensaver runs, invoked with
//! the same shape of arguments (bin/omarchy-screensaver: centered canvas,
//! centered text, a random effect, no gradient overrides, so every effect
//! paints in its own upstream colors).
//!
//! No terminal window is spawned and the glyphs are still drawn by the
//! shell's own canvas in its own mono font; what moved out of the shell is
//! the frame math. Everything here is a pure function of its arguments, the
//! argv the surface spawns and the parse of what comes back.

/// `ttfx --help`, ttfx 0.3.0, in its own listed order.
pub const EFFECT_NAMES: [&str; 37] = [
    "beams", "binarypath", "blackhole", "bouncyballs", "bubbles", "burn",
    "colorshift", "crumble", "decrypt", "errorcorrect", "expand", "fireworks",
    "highlight", "laseretch", "matrix", "middleout", "orbittingvolley",
    "overflow", "pour", "print", "rain", "randomsequence", "rings",
    "scattered", "slice", "slide", "smoke", "spotlights", "spray", "swarm",
    "sweep", "synthgrid", "thunderstorm", "unstable", "vhstape", "waves",
    "wipe",
];

/// The two effects ttfx's own README calls out as gated on wall-clock time
/// rather than frame count. At --frame-rate 0 (the deterministic pin path)
/// they emit as many frames as the machine can produce inside that fixed
/// duration, which is not reproducible across hosts, so they are never
/// recorded frame by frame.
pub const TIMED_EFFECTS: [&str; 2] = ["matrix", "thunderstorm"];

/// A pinned (frame-stepped) run re-generates the effect from scratch and
/// counts frames until it reaches the requested one, so a single pathological
/// run can't stream unbounded output. 600 frames is ~10s of animation at
/// 60fps, past the end of every effect that isn't wall-clock gated.
pub const PIN_FRAME_CAP: usize = 600;

pub fn is_known_effect(name: &str) -> bool {
    EFFECT_NAMES.contains(&name)
}

pub fn is_timed_effect(name: &str) -> bool {
    TIMED_EFFECTS.contains(&name)
}

/// A known (pinned) name replays itself; anything else, "random", the
/// default, or an unrecognised name, picks from every effect except the
/// immediately previous one, so consecutive cycles never repeat.
pub fn reroll_effect_name(requested: &str, previous_effect: &str, seed: i64) -> String {
    if is_known_effect(requested) {
        return requested.to_string();
    }
    let pool: Vec<&str> = EFFECT_NAMES
        .iter()
        .copied()
        .filter(|n| *n != previous_effect)
        .collect();
    pool[(seed.unsigned_abs() % pool.len() as u64) as usize].to_string()
}

// wire protocol
// ttfx writes one full canvas repaint per frame to stdout: `rows` lines of
// `columns` cells, separated by \n, with truecolor SGR runs inside them.
// Between two frames it emits restore-cursor, save-cursor, cursor-up-rows,
// which is the only byte sequence that can't occur inside a frame, so it is
// what the stream is split on. The complete escape vocabulary ttfx 0.3.0
// emits is ESC7, ESC8, ESC[<rows>A, ESC[0m, ESC[38;2;R;G;Bm and the
// ESC[?25l/h cursor-visibility pair: no background colors, no cursor
// addressing.

pub fn frame_delimiter(rows: u32) -> String {
    format!("\u{1b}8\u{1b}7\u{1b}[{rows}A")
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub banner_path: String,
    pub columns: u32,
    pub rows: u32,
    pub effect: String,
    pub frame_rate: u32,
    pub background: String,
    pub seed: i64,
}

/// The ttfx flags for one run: omarchy's own set apart from three things a
/// pipe forces.
///
/// `--ignore-terminal-dimensions`: with no tty on the other end ttfx measures
/// nothing and silently falls back to 80x24 whatever `--canvas-width/height`
/// say.
/// `--terminal-background-color`: ttfx blends against it, and every upstream
/// effect gradient is authored against black.
/// `--seed`: a pinned run has to replay identically; a live run gets the
/// activation's own seed so successive cycles don't animate identically.
///
/// No gradient overrides: each effect brings its own upstream colors.
pub fn args(opts: &Opts) -> Vec<String> {
    vec![
        "-i".into(),
        opts.banner_path.clone(),
        "--canvas-width".into(),
        opts.columns.to_string(),
        "--canvas-height".into(),
        opts.rows.to_string(),
        "--anchor-canvas".into(),
        "c".into(),
        "--anchor-text".into(),
        "c".into(),
        "--ignore-terminal-dimensions".into(),
        "--frame-rate".into(),
        opts.frame_rate.to_string(),
        "--terminal-background-color".into(),
        normalize_color(&opts.background),
        "--seed".into(),
        (opts.seed.unsigned_abs() % 2147483647).to_string(),
        opts.effect.clone(),
    ]
}

/// A translucent `#aarrggbb` makes ttfx exit 2 with no frames at all, so the
/// alpha is dropped: a theme that ever stops being opaque degrades to the
/// wrong blend rather than to no screensaver. Anything else becomes black.
pub fn normalize_color(value: &str) -> String {
    let hex = value.strip_prefix('#').filter(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    match hex.map(str::len) {
        Some(8) => format!("#{}", &value[3..]),
        Some(6) => value.to_string(),
        _ => "#000000".to_string(),
    }
}

/// Wrapped in a `command -v` guard: ttfx vanishing from PATH between the
/// startup probe and a real activation surfaces as exit 127 rather than a
/// spawn error with no exit code.
pub fn command(opts: &Opts) -> Vec<String> {
    let mut cmd: Vec<String> = vec![
        "sh".into(),
        "-c".into(),
        r#"command -v ttfx >/dev/null 2>&1 || exit 127; exec ttfx "$@""#.into(),
        "sh".into(),
    ];
    cmd.extend(args(opts));
    cmd
}

// frame parsing

/// A stretch of consecutive characters sharing a color, one draw call on the
/// canvas. `color` is "" for ttfx's default foreground (an SGR reset) and
/// "#rrggbb" otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub col: usize,
    pub color: String,
    pub text: String,
}

pub type Frame = Vec<Vec<Run>>;

/// One frame's text to rows of runs.
pub fn parse_frame(text: &str) -> Frame {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Frame = Vec::new();
    let mut runs: Vec<Run> = Vec::new();
    let mut run: Option<Run> = None;
    let mut col = 0usize;
    let mut color = String::new();
    let mut i = 0usize;

    fn flush(run: &mut Option<Run>, runs: &mut Vec<Run>) {
        if let Some(r) = run.take()
            && !r.text.is_empty()
        {
            runs.push(r);
        }
    }

    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '\u{1b}' => {
                let (next, new_color) = read_escape(&chars, i);
                i = next;
                if let Some(c) = new_color {
                    flush(&mut run, &mut runs);
                    color = c;
                }
            }
            '\n' => {
                flush(&mut run, &mut runs);
                rows.push(std::mem::take(&mut runs));
                col = 0;
                i += 1;
            }
            '\r' => i += 1,
            _ => {
                run.get_or_insert_with(|| Run { col, color: color.clone(), text: String::new() })
                    .text
                    .push(ch);
                col += 1;
                i += 1;
            }
        }
    }
    flush(&mut run, &mut runs);
    rows.push(runs);
    rows
}

/// The index just past the escape sequence starting at `start`, and the color
/// it selects: None when the sequence doesn't change the color (a cursor
/// move, the show/hide-cursor pair, an SGR this doesn't model).
fn read_escape(chars: &[char], start: usize) -> (usize, Option<String>) {
    if chars.get(start + 1) != Some(&'[') {
        return (start + 2, None); // ESC7 / ESC8 and friends
    }
    let mut i = start + 2;
    while i < chars.len() && matches!(chars[i], '0'..='9' | ';' | '?') {
        i += 1;
    }
    let last = chars.get(i).copied();
    let params: String = chars[start + 2..i].iter().collect();
    let next = i + 1;
    if last != Some('m') {
        return (next, None);
    }
    if params.is_empty() || params == "0" || params == "39" {
        return (next, Some(String::new()));
    }
    let parts: Vec<&str> = params.split(';').collect();
    if parts.len() == 5 && parts[0] == "38" && parts[1] == "2" {
        return (next, Some(format!("#{:02x}{:02x}{:02x}", byte(parts[2]), byte(parts[3]), byte(parts[4]))));
    }
    (next, None)
}

/// `parseInt(value, 10) || 0` clamped to a byte: leading digits only.
fn byte(value: &str) -> u8 {
    let mut n: u32 = 0;
    for d in value.chars().map_while(|c| c.to_digit(10)) {
        n = (n * 10 + d).min(255);
    }
    n as u8
}

/// Flattens a parsed frame back to plain per-row strings, honouring each
/// run's `col` rather than assuming runs are contiguous.
pub fn rows_to_text(rows: &Frame) -> Vec<String> {
    rows.iter()
        .map(|runs| {
            let mut line = String::new();
            let mut len = 0usize;
            for run in runs {
                while len < run.col {
                    line.push(' ');
                    len += 1;
                }
                line.push_str(&run.text);
                len += run.text.chars().count();
            }
            line
        })
        .collect()
}

/// Splits ttfx's stdout into frames on the delimiter, fed chunk by chunk.
/// A chunk may end anywhere, mid escape sequence or mid UTF-8 character, so
/// bytes are buffered until a whole delimited segment is present and only
/// then decoded and parsed.
///
/// The first segment a run yields is ttfx's canvas prep (hide cursor, then
/// the blank canvas), not a frame; it is returned like any other and the
/// caller drops it. Bytes after the last delimiter stay buffered.
#[derive(Debug)]
pub struct FrameParser {
    delimiter: Vec<u8>,
    buf: Vec<u8>,
}

impl FrameParser {
    pub fn new(rows: u32) -> Self {
        Self { delimiter: frame_delimiter(rows).into_bytes(), buf: Vec::new() }
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(chunk);
        let mut frames = Vec::new();
        let mut consumed = 0;
        while let Some(at) = find(&self.buf[consumed..], &self.delimiter) {
            let segment = &self.buf[consumed..consumed + at];
            frames.push(parse_frame(&String::from_utf8_lossy(segment)));
            consumed += at + self.delimiter.len();
        }
        self.buf.drain(..consumed);
        frames
    }

    /// At end of stream, whatever followed the last delimiter: the run's
    /// final frame, which no delimiter closes.
    pub fn finish(&mut self) -> Option<Frame> {
        let rest = std::mem::take(&mut self.buf);
        (!rest.is_empty()).then(|| parse_frame(&String::from_utf8_lossy(&rest)))
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ESC: &str = "\u{1b}";

    fn opts(effect: &str, columns: u32, rows: u32, frame_rate: u32, background: &str, seed: i64) -> Opts {
        Opts {
            banner_path: "/b".into(),
            columns,
            rows,
            effect: effect.into(),
            frame_rate,
            background: background.into(),
            seed,
        }
    }

    fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
        let at = args.iter().position(|a| a == flag).unwrap();
        &args[at + 1]
    }

    // registry

    #[test]
    fn effect_names_are_ttfx_0_3_0s_thirty_seven() {
        assert_eq!(EFFECT_NAMES.len(), 37);
        let mut seen = std::collections::HashSet::new();
        for name in EFFECT_NAMES {
            assert!(is_known_effect(name));
            assert!(seen.insert(name));
        }
        assert!(!is_known_effect("scatter"));
        assert!(is_known_effect("scattered"));
    }

    #[test]
    fn timed_effects_are_the_two_wall_clock_gated_ones() {
        assert!(is_timed_effect("matrix"));
        assert!(is_timed_effect("thunderstorm"));
        assert!(!is_timed_effect("decrypt"));
    }

    #[test]
    fn reroll_replays_a_pinned_effect() {
        for name in EFFECT_NAMES {
            assert_eq!(reroll_effect_name(name, "beams", 7), name);
        }
    }

    #[test]
    fn reroll_never_repeats_the_previous_effect() {
        for seed in 0..80 {
            assert_ne!(reroll_effect_name("random", "decrypt", seed), "decrypt");
        }
    }

    #[test]
    fn reroll_of_an_unknown_name_falls_back_to_random() {
        assert!(is_known_effect(&reroll_effect_name("nonesuch", "", 3)));
    }

    // argv

    #[test]
    fn args_carry_the_canvas_and_the_effect_last() {
        let o = Opts {
            banner_path: "/banner.txt".into(),
            columns: 96,
            rows: 30,
            effect: "decrypt".into(),
            frame_rate: 60,
            background: "#100F0F".into(),
            seed: 12,
        };
        let a = args(&o);
        assert_eq!(a.last().unwrap(), "decrypt");
        assert_eq!(after(&a, "--canvas-width"), "96");
        assert_eq!(after(&a, "--canvas-height"), "30");
        assert_eq!(after(&a, "--frame-rate"), "60");
        assert_eq!(after(&a, "--terminal-background-color"), "#100F0F");
        assert_eq!(after(&a, "-i"), "/banner.txt");
        assert!(a.iter().any(|x| x == "--ignore-terminal-dimensions"));
        assert!(!a.iter().any(|x| x == "--final-gradient-stops"));
    }

    #[test]
    fn args_clamp_the_seed_into_ttfxs_range() {
        let a = args(&opts("rain", 8, 4, 0, "#000000", 1786395783000));
        let seed: i64 = after(&a, "--seed").parse().unwrap();
        assert!((0..2147483647).contains(&seed));
    }

    #[test]
    fn args_drop_the_alpha_a_translucent_color_carries() {
        let a = args(&opts("rain", 8, 4, 60, "#80ff0000", 1));
        assert_eq!(after(&a, "--terminal-background-color"), "#ff0000");
        assert_eq!(normalize_color("#100f0f"), "#100f0f");
        assert_eq!(normalize_color("transparent"), "#000000");
    }

    #[test]
    fn command_guards_against_ttfx_leaving_path() {
        let cmd = command(&opts("rain", 8, 4, 60, "#000000", 1));
        assert_eq!(cmd[0], "sh");
        assert_eq!(cmd[1], "-c");
        assert!(cmd[2].contains("command -v ttfx"));
        assert!(cmd[2].contains("exit 127"));
        assert_eq!(cmd.last().unwrap(), "rain");
    }

    // wire protocol

    #[test]
    fn frame_delimiter_is_restore_save_cursor_up_rows() {
        assert_eq!(frame_delimiter(30), format!("{ESC}8{ESC}7{ESC}[30A"));
    }

    // frame parsing

    #[test]
    fn parse_frame_splits_rows_and_keeps_columns() {
        let rows = parse_frame("  AB  \n      \n  CD  ");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows_to_text(&rows), ["  AB  ", "      ", "  CD  "]);
    }

    #[test]
    fn parse_frame_reads_truecolor_runs() {
        let frame = format!("  {ESC}[38;2;0;209;255mAB{ESC}[0m  ");
        let runs = &parse_frame(&frame)[0];
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].text, "  ");
        assert_eq!(runs[0].color, "");
        assert_eq!(runs[1].col, 2);
        assert_eq!(runs[1].text, "AB");
        assert_eq!(runs[1].color, "#00d1ff");
        assert_eq!(runs[2].col, 4);
        assert_eq!(runs[2].color, "");
    }

    #[test]
    fn parse_frame_pads_single_digit_color_channels() {
        let runs = &parse_frame(&format!("{ESC}[38;2;5;16;0mX"))[0];
        assert_eq!(runs[0].color, "#051000");
    }

    #[test]
    fn parse_frame_ignores_cursor_and_unmodelled_sequences() {
        let frame = format!("{ESC}7AB{ESC}[?25h");
        let runs = &parse_frame(&frame)[0];
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].col, 0);
        assert_eq!(runs[0].text, "AB");
    }

    fn real_expand_frame() -> String {
        format!(
            "            \n     {ESC}[38;2;193;125;193mA{ESC}[0m{ESC}[38;2;204;150;204mB{ESC}[0m     \n     {ESC}[38;2;0;209;255mC{ESC}[0m{ESC}[38;2;0;209;255mD{ESC}[0m     \n            "
        )
    }

    #[test]
    fn parse_frame_of_a_real_expand_frame() {
        let rows = parse_frame(&real_expand_frame());
        assert_eq!(rows.len(), 4);
        assert_eq!(rows_to_text(&rows), ["            ", "     AB     ", "     CD     ", "            "]);
        assert_eq!(rows[1][1].color, "#c17dc1");
        assert_eq!(rows[2][1].color, "#00d1ff");
    }

    // streaming

    fn stream() -> Vec<u8> {
        let delim = frame_delimiter(4);
        format!("{ESC}[?25l  \n  {delim}{}{delim}\u{2588}\u{2584}{delim}tail", real_expand_frame()).into_bytes()
    }

    fn whole(bytes: &[u8]) -> Vec<Frame> {
        FrameParser::new(4).feed(bytes)
    }

    #[test]
    fn frame_parser_yields_one_frame_per_delimiter() {
        let frames = whole(&stream());
        assert_eq!(frames.len(), 3);
        assert_eq!(rows_to_text(&frames[1]), ["            ", "     AB     ", "     CD     ", "            "]);
        assert_eq!(rows_to_text(&frames[2]), ["\u{2588}\u{2584}"]);
    }

    #[test]
    fn frame_parser_is_chunk_boundary_independent() {
        let bytes = stream();
        let expected = whole(&bytes);
        for size in 1..=bytes.len() {
            let mut parser = FrameParser::new(4);
            let mut got = Vec::new();
            for chunk in bytes.chunks(size) {
                got.extend(parser.feed(chunk));
            }
            assert_eq!(got, expected, "chunk size {size}");
        }
    }
}
