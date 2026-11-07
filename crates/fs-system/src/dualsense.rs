//! Model for the DualSense panel. Testable head-on against fixture strings
//! shaped exactly like hid-playstation's sysfs attributes. Every function takes
//! the raw text a `cat` on the matching sysfs file would produce, or an
//! absent/unparsable one, and returns a complete default shape rather than
//! leaving a caller to guard against `None`.
//!
//! There is no daemon here: the service reads sysfs directly (a power_supply
//! node keyed by Bluetooth MAC, a `leds` node keyed by input index, both
//! first-match-wins globs), and the shell never writes any of it. The owner's
//! host units own the lightbar/player-LED writes; this model only ever
//! describes what was read.

use fs_js as js;

/// warn/critical thresholds mirror the retired `dualsense-bar` command module
/// this panel replaces: a straight read of the capacity percentage, no
/// charge-direction gating, the sysfs `status` string is surfaced separately as
/// `status_label` for the hero meta line instead.
pub(crate) const WARN_PERCENT: i64 = 20;
pub(crate) const CRITICAL_PERCENT: i64 = 10;

#[derive(Debug, Clone, PartialEq)]
pub struct Supply {
    /// -1 is the panel's "no controller" cue.
    pub percent: i64,
    pub status_label: String,
    pub warn: bool,
    pub critical: bool,
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `capacity_text`: the `capacity` sysfs file's own text (0-100, in 10% buckets
/// on real hardware, but this parses whatever integer it holds rather than
/// assuming the bucket size). `status_text`: the `status` sysfs file's own text
/// (POWER_SUPPLY_STATUS values, "Charging", "Discharging", "Full", "Not
/// charging", "Unknown"). Either missing/unparsable leaves `percent` at -1.
pub fn parse_supply(capacity_text: Option<&str>, status_text: Option<&str>) -> Supply {
    let mut percent = -1;
    if let Some(text) = capacity_text {
        let trimmed = js::trim(text);
        if all_digits(trimmed) {
            let n = js::parse_int(trimmed);
            if (0.0..=100.0).contains(&n) {
                percent = n as i64;
            }
        }
    }
    let critical = (0..=CRITICAL_PERCENT).contains(&percent);
    Supply {
        percent,
        status_label: status_text.map_or_else(String::new, |s| js::trim(s).to_string()),
        critical,
        warn: percent >= 0 && !critical && percent <= WARN_PERCENT,
    }
}

/// `text`: the `multi_intensity` sysfs file's own text, "R G B" (each 0-255).
/// A "#rrggbb" string, or `None` when the file is absent/malformed, the
/// LIGHTBAR row's own presence gate.
pub fn parse_lightbar(text: Option<&str>) -> Option<String> {
    let parts = js::split_ws(text?);
    if parts.len() != 3 {
        return None;
    }
    let mut out = String::from("#");
    for part in parts {
        if !all_digits(part) {
            return None;
        }
        let v = js::parse_int(part);
        if v > 255.0 {
            return None;
        }
        out.push_str(&format!("{:02x}", v as u8));
    }
    Some(out)
}

/// `brightnesses`: exactly 5 entries, each the matching `player-N/brightness`
/// sysfs file's own text ("0"/"1") or `None` where that file didn't exist. The
/// lit count (0-5); the caller decides "unreadable" (as opposed to "readable,
/// none lit") by whether it attempted this call at all, since a real DualSense
/// always exposes all five once its lightbar node is found.
pub fn parse_player_leds(brightnesses: Option<&[Option<&str>]>) -> usize {
    brightnesses.map_or(0, |list| list.iter().filter(|b| b.is_some_and(|t| js::trim(t) == "1")).count())
}

/// The hero meta line: the sysfs status word alone, e.g. "Charging" /
/// "Discharging" / "Full" / "Not charging". Sysfs carries no time-to-empty for
/// this device, so nothing is estimated or invented here.
pub fn state_line(supply: Option<&Supply>) -> String {
    match supply {
        Some(s) if s.percent >= 0 && !s.status_label.is_empty() => s.status_label.clone(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(cap: &str, status: &str) -> Supply {
        parse_supply(Some(cap), Some(status))
    }

    #[test]
    fn parse_supply_ok_reading() {
        let s = supply("80\n", "Discharging\n");
        assert_eq!(s.percent, 80);
        assert_eq!(s.status_label, "Discharging");
        assert!(!s.warn);
        assert!(!s.critical);
    }

    #[test]
    fn parse_supply_warn_band() {
        let s = supply("20\n", "Discharging\n");
        assert!(s.warn);
        assert!(!s.critical);
    }

    #[test]
    fn parse_supply_critical_band_wins_over_warn() {
        let s = supply("10\n", "Discharging\n");
        assert!(s.critical);
        assert!(!s.warn);
    }

    #[test]
    fn parse_supply_charging_status() {
        let s = supply("95\n", "Charging\n");
        assert_eq!(s.percent, 95);
        assert_eq!(s.status_label, "Charging");
    }

    #[test]
    fn parse_supply_missing_files() {
        let s = supply("", "");
        assert_eq!(s.percent, -1);
        assert_eq!(s.status_label, "");
        assert!(!s.warn);
        assert!(!s.critical);
    }

    #[test]
    fn parse_supply_malformed_capacity() {
        let s = supply("not a number\n", "Discharging\n");
        assert_eq!(s.percent, -1);
        assert!(!s.critical);
        assert!(!s.warn);
    }

    #[test]
    fn parse_lightbar_ok() {
        assert_eq!(parse_lightbar(Some("255 0 64\n")).as_deref(), Some("#ff0040"));
    }

    #[test]
    fn parse_lightbar_low_values_pad_to_two_digits() {
        assert_eq!(parse_lightbar(Some("0 8 15\n")).as_deref(), Some("#00080f"));
    }

    #[test]
    fn parse_lightbar_missing_file() {
        assert_eq!(parse_lightbar(Some("")), None);
        assert_eq!(parse_lightbar(None), None);
    }

    #[test]
    fn parse_lightbar_malformed() {
        assert_eq!(parse_lightbar(Some("255 0\n")), None);
        assert_eq!(parse_lightbar(Some("red green blue\n")), None);
        assert_eq!(parse_lightbar(Some("255 0 300\n")), None);
    }

    #[test]
    fn parse_player_leds_one_lit() {
        assert_eq!(parse_player_leds(Some(&[Some("0"), Some("1"), Some("0"), Some("0"), Some("0")])), 1);
    }

    #[test]
    fn parse_player_leds_none_lit() {
        assert_eq!(parse_player_leds(Some(&[Some("0"); 5])), 0);
    }

    #[test]
    fn parse_player_leds_missing_entries_read_as_unlit() {
        assert_eq!(parse_player_leds(Some(&[None, Some("1"), None, None, None])), 1);
    }

    #[test]
    fn parse_player_leds_not_an_array() {
        assert_eq!(parse_player_leds(None), 0);
    }

    #[test]
    fn state_line_discharging() {
        assert_eq!(state_line(Some(&supply("80\n", "Discharging\n"))), "Discharging");
    }

    #[test]
    fn state_line_full() {
        assert_eq!(state_line(Some(&supply("100\n", "Full\n"))), "Full");
    }

    #[test]
    fn state_line_no_controller() {
        assert_eq!(state_line(Some(&supply("", ""))), "");
    }
}
