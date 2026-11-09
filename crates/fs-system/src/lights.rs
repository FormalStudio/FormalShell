//! Model for the keyboard lights service. The service reads asusd's Aura
//! object over busctl and writes through asusctl; this module parses the one
//! and builds the argv for the other, so both ends are testable without a
//! keyboard.

use fs_js as js;
use regex::Regex;
use std::sync::LazyLock;

/// `mode` is asusd's AuraModeNum, which is what LedMode and
/// SupportedBasicModes carry (9 is unused upstream). `colours` is how many
/// colour flags asusctl's subcommand takes, `speed` whether it takes one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    pub id: &'static str,
    pub label: &'static str,
    pub mode: i64,
    pub colours: u8,
    pub speed: bool,
}

const fn fx(id: &'static str, label: &'static str, mode: i64, colours: u8, speed: bool) -> Effect {
    Effect { id, label, mode, colours, speed }
}

pub(crate) const EFFECTS: [Effect; 12] = [
    fx("static", "Static", 0, 1, false),
    fx("breathe", "Breathe", 1, 2, true),
    fx("rainbow-cycle", "Rainbow Cycle", 2, 0, true),
    fx("rainbow-wave", "Rainbow Wave", 3, 0, true),
    fx("stars", "Stars", 4, 2, true),
    fx("rain", "Rain", 5, 0, true),
    fx("highlight", "Highlight", 6, 1, true),
    fx("laser", "Laser", 7, 1, true),
    fx("ripple", "Ripple", 8, 1, true),
    fx("pulse", "Pulse", 10, 1, false),
    fx("comet", "Comet", 11, 1, false),
    fx("flash", "Flash", 12, 1, false),
];

pub const SPEEDS: [&str; 3] = ["low", "med", "high"];

/// asusd's Brightness property and `asusctl leds set`'s words, index for index.
pub const BRIGHTNESS: [&str; 4] = ["off", "low", "med", "high"];

pub const SOURCES: [&str; 2] = ["wallpaper", "custom"];

pub fn effect(id: &str) -> Option<&'static Effect> {
    EFFECTS.iter().find(|e| e.id == id)
}

pub fn effect_for_mode(mode: i64) -> Option<&'static Effect> {
    EFFECTS.iter().find(|e| e.mode == mode)
}

pub fn uses_colour(id: &str) -> bool {
    effect(id).is_some_and(|e| e.colours > 0)
}

static HEX6: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-f]{6}$").unwrap());

/// "rrggbb" lowercase, or "" for anything else. Takes "#RRGGBB" too, which is
/// what a person pastes.
pub fn normalize_hex(text: Option<&str>) -> String {
    let lowered = js::trim(text.unwrap_or("")).to_lowercase();
    let s = lowered.strip_prefix('#').unwrap_or(&lowered);
    if HEX6.is_match(s) { s.to_string() } else { String::new() }
}

pub fn hex_from_rgb(r: f64, g: f64, b: f64) -> String {
    let two = |v: f64| format!("{:02x}", js::round(v).clamp(0.0, 255.0) as u8);
    format!("{}{}{}", two(r), two(g), two(b))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub mode: i64,
    pub colour: String,
    pub speed: String,
    pub brightness: i64,
    pub modes: Vec<i64>,
}

static U_VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^u ([0-9]+)$").unwrap());
static AU_VALUE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^au [0-9]+((?: [0-9]+)*)$").unwrap());
static DATA_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^\(\S+\) [0-9]+ [0-9]+ ([0-9]+) ([0-9]+) ([0-9]+) [0-9]+ [0-9]+ [0-9]+ "([A-Za-z0-9_]+)""#).unwrap()
});

fn int(text: &str) -> i64 {
    js::parse_int(text) as i64
}

/// busctl's text form: `u 0`, `au 3 0 1 2`, and for LedModeData
/// `(uu(yyy)(yyy)ss) 0 0 67 133 190 0 0 0 "Med" "Right"` (mode, zone,
/// colour1, colour2, speed, direction).
pub fn parse_probe(text: &str) -> Probe {
    let mut out = Probe { mode: -1, colour: String::new(), speed: String::new(), brightness: -1, modes: Vec::new() };
    for line in text.split('\n') {
        let Some(eq) = line.find('=') else { continue };
        let key = &line[..eq];
        let value = js::trim(&line[eq + 1..]);
        if key == "MODE" {
            if let Some(m) = U_VALUE.captures(value) {
                out.mode = int(&m[1]);
            }
        } else if key == "BRIGHT" {
            if let Some(m) = U_VALUE.captures(value) {
                out.brightness = int(&m[1]);
            }
        } else if key == "MODES" {
            if let Some(m) = AU_VALUE.captures(value) {
                let rest = js::trim(&m[1]);
                out.modes = if rest.is_empty() { Vec::new() } else { rest.split(' ').map(int).collect() };
            }
        } else if key == "DATA"
            && let Some(m) = DATA_VALUE.captures(value)
        {
            out.colour = hex_from_rgb(int(&m[1]) as f64, int(&m[2]) as f64, int(&m[3]) as f64);
            out.speed = m[4].to_lowercase();
        }
    }
    out
}

/// The effects this chassis reports, in `EFFECTS` order. An empty report (an
/// older asusd, a read that failed) offers the whole table rather than none.
pub fn supported(modes: &[i64]) -> Vec<Effect> {
    if modes.is_empty() {
        return EFFECTS.to_vec();
    }
    EFFECTS.iter().filter(|e| modes.contains(&e.mode)).copied().collect()
}

/// argv for one `asusctl aura effect` call, or `None` for an unknown effect. A
/// two-colour effect gets the same colour twice, which asusd draws as a
/// single-colour breathe or starfield.
pub fn effect_args(id: &str, colour: &str, speed: &str) -> Option<Vec<String>> {
    let e = effect(id)?;
    let mut args: Vec<String> = ["asusctl", "aura", "effect", e.id].map(String::from).to_vec();
    let hex = match normalize_hex(Some(colour)) {
        h if h.is_empty() => "ffffff".to_string(),
        h => h,
    };
    if e.colours >= 1 {
        args.extend(["--colour".to_string(), hex.clone()]);
    }
    if e.colours >= 2 {
        args.extend(["--colour2".to_string(), hex]);
    }
    if e.speed {
        let speed = if SPEEDS.contains(&speed) { speed } else { "med" };
        args.extend(["--speed".to_string(), speed.to_string()]);
    }
    if e.id == "rainbow-wave" {
        args.extend(["--direction".to_string(), "right".to_string()]);
    }
    Some(args)
}

/// asusctl's Aura zones. A zone the chassis lacks makes asusctl exit non-zero,
/// which the script ignores.
const AURA_ZONES: [&str; 5] = ["keyboard", "logo", "lightbar", "lid", "rear-glow"];

/// `asusctl aura power <zone>` with no flags clears every power state for the
/// zone, which is off. Exit 3 is this script's own "no asusctl".
const AURA_OFF: &str = "command -v asusctl >/dev/null || exit 3; for z; do asusctl aura power \"$z\" >/dev/null 2>&1; done; exit 0";

/// The states the host's asus-aura unit sets at boot (sleep left off, so the
/// zones stay dark while suspended).
const AURA_ON: &str = "for z; do asusctl aura power \"$z\" --boot --awake --shutdown >/dev/null 2>&1; done; exit 0";

/// argv that turns every Aura zone's power states off (`true`) or back to the
/// boot unit's (`false`).
pub fn aura_power_argv(off: bool) -> Vec<String> {
    let mut argv: Vec<String> = ["sh", "-c", if off { AURA_OFF } else { AURA_ON }, "sh"].map(String::from).to_vec();
    argv.extend(AURA_ZONES.map(String::from));
    argv
}

pub fn brightness_args(level: i64) -> Option<Vec<String>> {
    let word = usize::try_from(level).ok().and_then(|i| BRIGHTNESS.get(i))?;
    Some(["asusctl", "leds", "set", word].map(String::from).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the probe prints on the g815 (asusd 6.x, busctl's text form),
    /// static Flexoki blue at full brightness.
    const G815: &str = "MODE=u 0\n\
        DATA=(uu(yyy)(yyy)ss) 0 0 67 133 190 0 0 0 \"Med\" \"Right\"\n\
        BRIGHT=u 3\n\
        MODES=au 12 0 1 2 3 4 5 6 7 8 10 11 12\n";

    #[test]
    fn parse_probe_reads_the_g815() {
        let p = parse_probe(G815);
        assert_eq!(p.mode, 0);
        assert_eq!(p.colour, "4385be");
        assert_eq!(p.speed, "med");
        assert_eq!(p.brightness, 3);
        assert_eq!(p.modes.len(), 12);
        assert_eq!(effect_for_mode(p.mode).unwrap().id, "static");
    }

    #[test]
    fn parse_probe_tolerates_empty_fields() {
        let p = parse_probe("MODE=\nDATA=\nBRIGHT=\nMODES=\n");
        assert_eq!(p.mode, -1);
        assert_eq!(p.colour, "");
        assert_eq!(p.brightness, -1);
        assert_eq!(p.modes.len(), 0);
        assert_eq!(parse_probe("").mode, -1);
    }

    #[test]
    fn supported_follows_the_chassis() {
        let ids: Vec<&str> = supported(&[0, 1, 10]).iter().map(|e| e.id).collect();
        assert_eq!(ids.join(","), "static,breathe,pulse");
        // No report at all offers the whole table rather than an empty menu.
        assert_eq!(supported(&[]).len(), EFFECTS.len());
    }

    #[test]
    fn normalize_hex_cases() {
        assert_eq!(normalize_hex(Some("#FF8800")), "ff8800");
        assert_eq!(normalize_hex(Some(" ff8800 ")), "ff8800");
        assert_eq!(normalize_hex(Some("f80")), "");
        assert_eq!(normalize_hex(Some("orange")), "");
        assert_eq!(normalize_hex(None), "");
    }

    fn args(id: &str, colour: &str, speed: &str) -> Option<String> {
        effect_args(id, colour, speed).map(|a| a.join(" "))
    }

    #[test]
    fn effect_args_match_asusctl_usage() {
        assert_eq!(args("static", "ff8800", "med").unwrap(), "asusctl aura effect static --colour ff8800");
        assert_eq!(
            args("breathe", "ff8800", "high").unwrap(),
            "asusctl aura effect breathe --colour ff8800 --colour2 ff8800 --speed high"
        );
        assert_eq!(args("rainbow-wave", "", "low").unwrap(), "asusctl aura effect rainbow-wave --speed low --direction right");
        assert_eq!(args("rain", "ff8800", "bogus").unwrap(), "asusctl aura effect rain --speed med");
        assert_eq!(args("comet", "nope", "med").unwrap(), "asusctl aura effect comet --colour ffffff");
        assert_eq!(args("disco", "ff8800", "med"), None);
    }

    #[test]
    fn brightness_args_cases() {
        assert_eq!(brightness_args(0).unwrap().join(" "), "asusctl leds set off");
        assert_eq!(brightness_args(3).unwrap().join(" "), "asusctl leds set high");
        assert_eq!(brightness_args(4), None);
        assert_eq!(brightness_args(-1), None);
    }
}
