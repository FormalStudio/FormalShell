//! Model for the DualSense panel. The battery is UPower's own device for
//! hid-playstation's power_supply node; the light parsers take the raw text a
//! `cat` on the matching sysfs file would produce, or an absent/unparsable
//! one, and return a complete default shape rather than leaving a caller to
//! guard against `None`.
//!
//! The service reads the `leds` node directly (keyed by input index,
//! first-match-wins), and the shell never writes any of it. The owner's
//! host units own the lightbar/player-LED writes; this model only ever
//! describes what was read.

use fs_js as js;
use fs_types::Percent;
use fs_upower::DeviceState;

use crate::power::model::{Low, Thresholds};

#[derive(Debug, Clone, PartialEq)]
pub struct Supply {
    pub level: Percent,
    pub state: DeviceState,
    /// Off a straight read of the level, with no charge-direction gating.
    pub low: Option<Low>,
}

impl Supply {
    pub fn new(level: Percent, state: DeviceState) -> Supply {
        Supply { level, state, low: Thresholds::DUALSENSE.low(level) }
    }

    /// The hero meta line: the sysfs status word UPower's state stands for.
    pub fn status_label(&self) -> &'static str {
        match self.state {
            DeviceState::Charging => "Charging",
            DeviceState::Discharging | DeviceState::Empty => "Discharging",
            DeviceState::FullyCharged => "Full",
            DeviceState::PendingCharge | DeviceState::PendingDischarge => "Not charging",
            DeviceState::Unknown => "Unknown",
        }
    }
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
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

/// The hero meta line: the status word alone. UPower carries no
/// time-to-empty for this device, so nothing is estimated or invented here.
pub fn state_line(supply: Option<&Supply>) -> String {
    supply.map_or_else(String::new, |s| s.status_label().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(level: u8, state: DeviceState) -> Supply {
        Supply::new(Percent::whole(level), state)
    }

    #[test]
    fn supply_ok_reading() {
        let s = supply(80, DeviceState::Discharging);
        assert_eq!(s.level.rounded(), 80);
        assert_eq!(s.status_label(), "Discharging");
        assert_eq!(s.low, None);
    }

    #[test]
    fn supply_warn_band() {
        assert_eq!(supply(20, DeviceState::Discharging).low, Some(Low::Warn));
    }

    #[test]
    fn supply_critical_band_wins_over_warn() {
        assert_eq!(supply(10, DeviceState::Discharging).low, Some(Low::Critical));
    }

    #[test]
    fn supply_ignores_charging() {
        let s = supply(5, DeviceState::Charging);
        assert_eq!(s.status_label(), "Charging");
        assert_eq!(s.low, Some(Low::Critical));
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
        assert_eq!(state_line(Some(&supply(80, DeviceState::Discharging))), "Discharging");
    }

    #[test]
    fn state_line_full() {
        assert_eq!(state_line(Some(&supply(100, DeviceState::FullyCharged))), "Full");
    }

    #[test]
    fn state_line_no_controller() {
        assert_eq!(state_line(None), "");
    }
}
