//! Model for the overnight service. The service lists /sys/class/leds as one
//! "name<TAB>trigger<TAB>brightness" line per device and this module decides
//! which of them overnight is allowed to turn off.
//!
//! Only an LED with no active trigger ("[none]" in its trigger file) is ours:
//! writing 0 to a triggered LED's brightness detaches the trigger in the
//! kernel, and logind's SetBrightness (what brightnessctl goes through) has no
//! way to put it back. That rules out the Wi-Fi activity LED and the lock-key
//! indicators. input<N>:: devices are skipped by name as well, since a lock
//! key's LED with its trigger already gone still belongs to the keyboard, not
//! to us. An LED already at 0 is left alone so disable() never turns on
//! something that was off.

use fs_js as js;
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// Percent, never 0: some panels treat a zero backlight or VCP 10 as off.
pub const SCREEN_PERCENT: i64 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Led {
    pub name: String,
    pub brightness: i64,
}

static INPUT_LED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^input[0-9]+::").unwrap());

pub fn parse_leds(text: &str) -> Vec<Led> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 3 {
            continue;
        }
        let name = fields[0];
        let brightness = js::parse_int(fields[2]);
        if name.is_empty() || INPUT_LED.is_match(name) {
            continue;
        }
        if !fields[1].contains("[none]") {
            continue;
        }
        if brightness.is_nan() || brightness <= 0.0 {
            continue;
        }
        out.push(Led { name: name.to_string(), brightness: brightness as i64 });
    }
    out
}

/// `(name, brightness)` pairs in the order the LEDs were listed, for state.json,
/// where it has to survive a shell restart. Order is part of the contract: the
/// restore loop writes them back in it.
pub fn led_snapshot(leds: &[Led]) -> Vec<(String, i64)> {
    leds.iter().map(|l| (l.name.clone(), l.brightness)).collect()
}

/// argv tail for the restore loop: name, value, name, value. `snapshot` is the
/// saved pairs in file order; a value that is not a positive integer is dropped.
pub fn restore_args(snapshot: &[(String, Value)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, raw) in snapshot {
        let text = match raw {
            Value::String(s) => s.clone(),
            Value::Number(n) => js::num_str(n.as_f64().unwrap_or(f64::NAN)),
            _ => continue,
        };
        let value = js::parse_int(&text);
        if value > 0.0 {
            out.push(name.clone());
            out.push(js::num_str(value));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The g815's own /sys/class/leds, trimmed: the ASUS keyboard backlight lit
    /// with no trigger, a LAN LED with no trigger but already dark, the Wi-Fi LED
    /// on its radio trigger, and lock keys on theirs.
    const G815: &str = "asus::kbd_backlight\t[none] kbd-scrolllock kbd-numlock\t3\n\
        enp130s0-0::lan\t[none] netdev\t0\n\
        phy0-led\tnone [phy0radio] phy0tpt\t1\n\
        input12::capslock\tnone [kbd-capslock]\t1\n\
        input12::compose\t[none] kbd-capslock\t1\n\
        nvidia_0\t[none]\t2\n";

    #[test]
    fn parse_keeps_lit_untriggered_leds() {
        let leds = parse_leds(G815);
        assert_eq!(leds.len(), 2);
        assert_eq!(leds[0].name, "asus::kbd_backlight");
        assert_eq!(leds[0].brightness, 3);
        assert_eq!(leds[1].name, "nvidia_0");
    }

    #[test]
    fn parse_skips_triggered_dark_and_input_leds() {
        let names: Vec<String> = parse_leds(G815).into_iter().map(|l| l.name).collect();
        for skipped in ["phy0-led", "enp130s0-0::lan", "input12::capslock", "input12::compose"] {
            assert!(!names.iter().any(|n| n == skipped), "{skipped}");
        }
    }

    #[test]
    fn parse_tolerates_empty_and_unreadable() {
        assert_eq!(parse_leds("").len(), 0);
        assert_eq!(parse_leds("broken\t[none]\t\n").len(), 0);
    }

    fn pairs(v: Value) -> Vec<(String, Value)> {
        v.as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
    }

    #[test]
    fn restore_args_round_trip() {
        let snap = led_snapshot(&parse_leds(G815));
        assert_eq!(snap[0], ("asus::kbd_backlight".to_string(), 3));
        let as_values: Vec<(String, Value)> = snap.iter().map(|(n, b)| (n.clone(), json!(b))).collect();
        assert_eq!(restore_args(&as_values), ["asus::kbd_backlight", "3", "nvidia_0", "2"]);
    }

    #[test]
    fn restore_args_drops_zero_and_junk() {
        assert_eq!(restore_args(&pairs(json!({ "a": 0, "b": "x", "c": 4 }))), ["c", "4"]);
        assert!(restore_args(&[]).is_empty());
    }
}
