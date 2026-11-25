//! Keys pass through by position, as a USB keyboard's would: the phone
//! turns usages into characters with its own hardware-keyboard layout, dead
//! keys and AltGr included. Control stands in for Command, the way shortcuts
//! sit under the same finger on a Mac, except with Space, which the phone
//! keeps as Control+Space for switching keyboards.

use std::collections::BTreeSet;

const CTRL: [u8; 2] = [0xe0, 0xe4];
const GUI: [u8; 2] = [0xe3, 0xe7];
const SPACE: u8 = 0x2c;

/// The HID keyboard usage for an evdev key code.
pub fn usage(code: u32) -> Option<u8> {
    Some(match code {
        1 => 0x29,
        2..=10 => 0x1e + (code - 2) as u8,
        11 => 0x27,
        12 => 0x2d,
        13 => 0x2e,
        14 => 0x2a,
        15 => 0x2b,
        16 => 0x14,
        17 => 0x1a,
        18 => 0x08,
        19 => 0x15,
        20 => 0x17,
        21 => 0x1c,
        22 => 0x18,
        23 => 0x0c,
        24 => 0x12,
        25 => 0x13,
        26 => 0x2f,
        27 => 0x30,
        28 => 0x28,
        29 => 0xe0,
        30 => 0x04,
        31 => 0x16,
        32 => 0x07,
        33 => 0x09,
        34 => 0x0a,
        35 => 0x0b,
        36 => 0x0d,
        37 => 0x0e,
        38 => 0x0f,
        39 => 0x33,
        40 => 0x34,
        41 => 0x35,
        42 => 0xe1,
        // The key left of Return. Linux reads ANSI's backslash and ISO's
        // Non-US # as one code; ISO's is the one a European layout expects.
        43 => 0x32,
        44 => 0x1d,
        45 => 0x1b,
        46 => 0x06,
        47 => 0x19,
        48 => 0x05,
        49 => 0x11,
        50 => 0x10,
        51 => 0x36,
        52 => 0x37,
        53 => 0x38,
        54 => 0xe5,
        55 => 0x55,
        56 => 0xe2,
        57 => 0x2c,
        58 => 0x39,
        59..=68 => 0x3a + (code - 59) as u8,
        69 => 0x53,
        71 => 0x5f,
        72 => 0x60,
        73 => 0x61,
        74 => 0x56,
        75 => 0x5c,
        76 => 0x5d,
        77 => 0x5e,
        78 => 0x57,
        79 => 0x59,
        80 => 0x5a,
        81 => 0x5b,
        82 => 0x62,
        83 => 0x63,
        86 => 0x64,
        87 => 0x44,
        88 => 0x45,
        96 => 0x58,
        97 => 0xe4,
        98 => 0x54,
        100 => 0xe6,
        102 => 0x4a,
        103 => 0x52,
        104 => 0x4b,
        105 => 0x50,
        106 => 0x4f,
        107 => 0x4d,
        108 => 0x51,
        109 => 0x4e,
        110 => 0x49,
        111 => 0x4c,
        125 => 0xe3,
        126 => 0xe7,
        _ => return None,
    })
}

/// What the phone is sent for the keys held here.
pub fn for_phone(held: &BTreeSet<u8>) -> BTreeSet<u8> {
    if held.contains(&SPACE) {
        return held.clone();
    }
    held.iter().map(|u| CTRL.iter().position(|c| c == u).map_or(*u, |i| GUI[i])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_shortcuts() {
        assert_eq!(usage(30), Some(0x04));
        assert_eq!(usage(44), Some(0x1d));
        assert_eq!(usage(11), Some(0x27));
        let copy = BTreeSet::from([0xe0, 0x06]);
        assert_eq!(for_phone(&copy), BTreeSet::from([0xe3, 0x06]));
        let switch = BTreeSet::from([0xe0, SPACE]);
        assert_eq!(for_phone(&switch), switch);
    }
}
