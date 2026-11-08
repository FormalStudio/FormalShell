//! Shares of a whole, each in the scale its source sends, so a 0..1 fraction
//! never meets a 0..100 percentage without a named conversion.

/// A level in percent, 0..=100: UPower's, a sysfs `capacity`, an earbuds
/// backend's.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Percent(f64);

impl Percent {
    /// Clamps to 0..=100; a NaN reads as 0.
    pub fn from_percent(value: f64) -> Self {
        if value.is_nan() {
            return Self(0.0);
        }
        Self(value.clamp(0.0, 100.0))
    }

    /// A whole percentage, for tables of thresholds.
    pub const fn whole(value: u8) -> Self {
        Self(if value > 100 { 100.0 } else { value as f64 })
    }

    pub fn as_percent(self) -> f64 {
        self.0
    }

    pub fn as_fraction(self) -> f64 {
        self.0 / 100.0
    }

    pub fn to_fraction(self) -> Fraction {
        Fraction(self.as_fraction())
    }

    /// The whole number a cell or panel shows.
    pub fn rounded(self) -> u8 {
        self.0.round() as u8
    }

    /// The same level held as the whole number it shows.
    pub fn round(self) -> Self {
        Self(self.0.round())
    }
}

/// A level as a fraction, 0..=1: BlueZ's Battery1 as the shell's models
/// hold it.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Fraction(f64);

impl Fraction {
    /// Clamps to 0..=1; a NaN reads as 0.
    pub fn from_fraction(value: f64) -> Self {
        if value.is_nan() {
            return Self(0.0);
        }
        Self(value.clamp(0.0, 1.0))
    }

    pub fn as_fraction(self) -> f64 {
        self.0
    }

    pub fn as_percent(self) -> f64 {
        self.0 * 100.0
    }

    pub fn to_percent(self) -> Percent {
        Percent(self.as_percent())
    }
}

/// A Bluetooth device address, six octets. Parsed once from BlueZ's or a
/// helper's text; it prints as BlueZ does, upper-case with colons.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BtAddress([u8; 6]);

impl BtAddress {
    /// `AA:BB:CC:DD:EE:FF` in either case, and nothing else.
    pub fn parse(text: &str) -> Option<Self> {
        Self::parse_with(text, |c| c == b':')
    }

    /// Whether `text` is the shape BlueZ falls back to for an unnamed
    /// device's label: six octets joined by colons or dashes.
    pub fn is_label(text: &str) -> bool {
        Self::parse_with(text, |c| c == b':' || c == b'-').is_some()
    }

    fn parse_with(text: &str, sep: impl Fn(u8) -> bool) -> Option<Self> {
        let b = text.as_bytes();
        if b.len() != 17 {
            return None;
        }
        let mut octets = [0u8; 6];
        for (i, octet) in octets.iter_mut().enumerate() {
            let at = i * 3;
            if i > 0 && !sep(b[at - 1]) {
                return None;
            }
            let hex = |c: u8| (c as char).to_digit(16);
            *octet = (hex(b[at])? * 16 + hex(b[at + 1])?) as u8;
        }
        Some(Self(octets))
    }

    /// The `AA_BB_CC_DD_EE_FF` form BlueZ puts in a device's object path.
    pub fn path_segment(self) -> String {
        self.to_string().replace(':', "_")
    }
}

impl Default for BtAddress {
    /// BDADDR_ANY, the address of a device that has not reported one.
    fn default() -> Self {
        Self([0; 6])
    }
}

impl std::fmt::Display for BtAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02X}:{b:02X}:{c:02X}:{d:02X}:{e:02X}:{g:02X}")
    }
}

impl std::fmt::Debug for BtAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BtAddress({self})")
    }
}

impl std::str::FromStr for BtAddress {
    type Err = ();

    fn from_str(text: &str) -> Result<Self, ()> {
        Self::parse(text).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_keeps_the_wire_scale() {
        let p = Percent::from_percent(73.4);
        assert_eq!(p.as_percent(), 73.4);
        assert!((p.as_fraction() - 0.734).abs() < 1e-12);
        assert_eq!(p.rounded(), 73);
        assert_eq!(p.round().as_percent(), 73.0);
    }

    #[test]
    fn percent_clamps_and_survives_nan() {
        assert_eq!(Percent::from_percent(140.0).as_percent(), 100.0);
        assert_eq!(Percent::from_percent(-3.0).as_percent(), 0.0);
        assert_eq!(Percent::from_percent(f64::NAN).as_percent(), 0.0);
        assert_eq!(Percent::whole(120).as_percent(), 100.0);
    }

    #[test]
    fn fraction_converts_by_name() {
        let f = Fraction::from_fraction(0.42);
        assert_eq!(f.as_fraction(), 0.42);
        assert_eq!(f.to_percent().rounded(), 42);
        assert_eq!(Fraction::from_fraction(1.5).as_fraction(), 1.0);
        assert_eq!(Fraction::from_fraction(f64::NAN).as_fraction(), 0.0);
    }

    #[test]
    fn bt_address_parses_either_case_and_prints_upper() {
        let a = BtAddress::parse("aa:bb:cc:dd:ee:0f").unwrap();
        assert_eq!(a.to_string(), "AA:BB:CC:DD:EE:0F");
        assert_eq!(a.path_segment(), "AA_BB_CC_DD_EE_0F");
        assert_eq!(Some(a), BtAddress::parse("AA:BB:CC:DD:EE:0F"));
        for bad in ["", "AA:BB:CC:DD:EE:FF\n", "AA:BB --dry-run", "--help", "AA-BB-CC-DD-EE-FF", "AA:BB:CC:DD:EE", "AA:BB:CC:DD:EE:GG", "AA:BB:CC:DD:EE:FF:00"] {
            assert_eq!(BtAddress::parse(bad), None, "{bad:?}");
        }
        assert!(BtAddress::is_label("aa-bb-cc-dd-ee-ff"));
        assert!(!BtAddress::is_label("Sony WH-1000XM4"));
    }
}
