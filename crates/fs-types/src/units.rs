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
}
