//! The chords a window is handed instead of the binds: under a Mac app the
//! Super key is Command, so while one has the keyboard the binds on Super
//! alone and on Super with Shift are let through to it, save the ones the
//! shell reserves (`macApps.reservedBinds`). What `hyprland.lua` does with
//! its `mac` submap.

use crate::chord::{Chord, Key, Mods};

/// The chords that stay bound, compared the way the include compares them:
/// case and spacing count for nothing, `CONTROL` is `CTRL`, and a digit is
/// its key of the number row, `code:10` to `code:19`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reserved(Vec<(Mods, String)>);

fn key_of(name: &str) -> String {
    let name = name.to_ascii_lowercase();
    match name.strip_prefix("code:").map(str::parse::<u32>) {
        Some(Ok(code @ 10..=19)) => ((code - 9) % 10).to_string(),
        _ => name,
    }
}

/// A chord as the shell writes it. Nothing is refused: one that names no
/// bind reserves nothing.
fn read(text: &str) -> (Mods, String) {
    let mut mods = Mods::default();
    let mut key = String::new();
    for part in text.split('+').map(str::trim) {
        match part.to_ascii_uppercase().as_str() {
            "SUPER" | "MOD" | "LOGO" => mods.logo = true,
            "SHIFT" => mods.shift = true,
            "CTRL" | "CONTROL" => mods.ctrl = true,
            "ALT" => mods.alt = true,
            _ => key = key_of(part),
        }
    }
    (mods, key)
}

impl Reserved {
    pub fn new(chords: &[String]) -> Self {
        Self(chords.iter().map(|chord| read(chord)).collect())
    }

    /// Whether the window gets the chord: a key on Super alone or on Super
    /// with Shift that is not reserved. A pointer button and the wheel stay
    /// bound, as every chord with Ctrl or Alt does.
    pub fn hands(&self, chord: &Chord) -> bool {
        let Mods {
            logo: true,
            ctrl: false,
            alt: false,
            ..
        } = chord.mods
        else {
            return false;
        };
        let key = match &chord.key {
            Key::Sym(name) => key_of(name),
            Key::Code(code) => key_of(&format!("code:{code}")),
            Key::Button(_) | Key::WheelUp | Key::WheelDown => return false,
        };
        !self.0.contains(&(chord.mods, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hands(reserved: &[&str], chord: &str) -> bool {
        let reserved: Vec<String> = reserved.iter().map(|chord| chord.to_string()).collect();
        Reserved::new(&reserved).hands(&Chord::parse(chord).unwrap())
    }

    #[test]
    fn super_alone_and_with_shift_go_to_the_window() {
        for chord in [
            "SUPER+q",
            "SUPER+SHIFT+q",
            "SUPER+Return",
            "SUPER+code:10",
            "SUPER+SHIFT+code:24",
            "SUPER+Super_L",
            "SUPER+XF86AudioPlay",
        ] {
            assert!(hands(&[], chord), "{chord}");
        }
    }

    #[test]
    fn every_other_chord_stays_bound() {
        for chord in [
            "ALT+Tab",
            "Print",
            "XF86AudioRaiseVolume",
            "SHIFT+q",
            "SUPER+CTRL+q",
            "SUPER+ALT+q",
            "SUPER+SHIFT+CTRL+q",
            "SUPER+mouse:272",
            "SUPER+SHIFT+mouse:273",
            "SUPER+mouse_up",
            "SUPER+mouse_down",
            "ALT+Alt_L",
        ] {
            assert!(!hands(&[], chord), "{chord}");
        }
    }

    #[test]
    fn a_reserved_chord_stays_bound_with_the_modifiers_it_names() {
        let reserved = ["SUPER + H", "SUPER + SHIFT + J", "SUPER + Space"];
        assert!(!hands(&reserved, "SUPER+h"));
        assert!(hands(&reserved, "SUPER+SHIFT+h"));
        assert!(!hands(&reserved, "SUPER+SHIFT+j"));
        assert!(hands(&reserved, "SUPER+j"));
        assert!(!hands(&reserved, "SUPER+space"));
        assert!(hands(&reserved, "SUPER+k"));
    }

    #[test]
    fn case_spacing_and_the_order_of_modifiers_count_for_nothing() {
        assert!(!hands(&["shift+super+RETURN"], "SUPER+SHIFT+Return"));
        assert!(!hands(&["  Super  +  escape "], "SUPER+Escape"));
        assert!(!hands(&["SUPER+Q"], "super+q"));
        // The settings' other names for the key.
        assert!(!hands(&["MOD+q"], "LOGO+q"));
    }

    #[test]
    fn a_digit_is_its_key_of_the_number_row() {
        assert!(!hands(&["SUPER + 1"], "SUPER+code:10"));
        assert!(!hands(&["SUPER + 0"], "SUPER+code:19"));
        assert!(!hands(&["SUPER + SHIFT + 9"], "SUPER+SHIFT+code:18"));
        assert!(!hands(&["SUPER + code:11"], "SUPER+2"));
        assert!(!hands(&["SUPER + 3"], "SUPER+3"));
        assert!(hands(&["SUPER + 1"], "SUPER+code:11"));
        assert!(hands(&["SUPER + 1"], "SUPER+SHIFT+code:10"));
        // A keycode off the number row is itself.
        assert!(!hands(&["SUPER + code:24"], "SUPER+code:24"));
        assert!(hands(&["SUPER + code:24"], "SUPER+code:25"));
    }

    #[test]
    fn what_reserves_nothing() {
        // No list hands every chord over; a chord with Ctrl is not one the
        // window gets, so reserving it changes nothing.
        assert!(hands(&[], "SUPER+h"));
        assert!(hands(&["", "SUPER", "SUPER+CTRL+h", "h"], "SUPER+h"));
    }
}
