//! How far along an activity is. Each source sends its own unit: fspec a
//! count of steps, `com.canonical.Unity.LauncherEntry` a 0..1 double, a
//! notification's `value` hint 0..100. They meet here and nowhere else.

use serde_json::{Value, json};

/// A share of the work done, held as 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fraction(f64);

impl Fraction {
    /// LauncherEntry's `progress`, 0..1. Out-of-range values are clamped;
    /// NaN says nothing.
    pub fn from_unit(v: f64) -> Option<Self> {
        (!v.is_nan()).then(|| Self(v.clamp(0.0, 1.0)))
    }

    /// The notification spec's `value` hint, 0..100.
    pub fn from_percent(p: u8) -> Self {
        Self(f64::from(p.min(100)) / 100.0)
    }

    /// `done` of `total`; a total of 0 says nothing.
    pub fn of(done: u64, total: u64) -> Option<Self> {
        (total > 0).then(|| Self((done as f64 / total as f64).clamp(0.0, 1.0)))
    }

    pub fn as_unit(self) -> f64 {
        self.0
    }

    pub fn as_percent(self) -> u8 {
        (self.0 * 100.0).round() as u8
    }
}

/// What fspec counts in. A unit this reader does not know is a plain count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unit {
    Resources,
    Packages,
    Other(String),
}

impl Unit {
    pub fn parse(s: &str) -> Self {
        match s {
            "resources" => Unit::Resources,
            "packages" => Unit::Packages,
            other => Unit::Other(other.to_owned()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Unit::Resources => "resources",
            Unit::Packages => "packages",
            Unit::Other(s) => s,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    /// The source cannot say how far it is: an indeterminate ring.
    Indeterminate,
    Steps { done: u64, total: u64, unit: Unit },
    Fraction(Fraction),
}

impl Progress {
    pub fn fraction(&self) -> Option<Fraction> {
        match self {
            Progress::Indeterminate => None,
            Progress::Steps { done, total, .. } => Fraction::of(*done, *total),
            Progress::Fraction(f) => Some(*f),
        }
    }

    /// How far, for people: `3 of 12 resources`, `40%`; none while it
    /// cannot say.
    pub fn words(&self) -> Option<String> {
        match self {
            Progress::Indeterminate => None,
            Progress::Steps { done, unit: Unit::Other(u), .. } if u == "percent" => Some(format!("{done}%")),
            Progress::Steps { done, total, unit } => Some(format!("{done} of {total} {}", unit.as_str())),
            Progress::Fraction(f) => Some(format!("{}%", f.as_percent())),
        }
    }

    /// `null` while indeterminate; steps keep their count and unit beside
    /// the fraction.
    pub fn to_json(&self) -> Value {
        let fraction = self.fraction().map(Fraction::as_unit);
        match self {
            Progress::Indeterminate => Value::Null,
            Progress::Steps { done, total, unit } => {
                json!({"fraction": fraction, "done": done, "total": total, "unit": unit.as_str()})
            }
            Progress::Fraction(_) => json!({"fraction": fraction}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_source_lands_on_the_same_scale() {
        assert_eq!(Fraction::from_unit(0.4).unwrap().as_percent(), 40);
        assert_eq!(Fraction::from_percent(40).as_unit(), 0.4);
        assert_eq!(Fraction::of(3, 12).unwrap().as_unit(), 0.25);
        assert_eq!(Progress::Steps { done: 3, total: 12, unit: Unit::Resources }.fraction().unwrap().as_percent(), 25);
    }

    #[test]
    fn out_of_range_is_clamped_and_nothing_is_none() {
        assert_eq!(Fraction::from_unit(1.7).unwrap().as_unit(), 1.0);
        assert_eq!(Fraction::from_unit(-0.2).unwrap().as_unit(), 0.0);
        assert!(Fraction::from_unit(f64::NAN).is_none());
        assert_eq!(Fraction::from_percent(250).as_percent(), 100);
        assert!(Fraction::of(0, 0).is_none());
        assert_eq!(Fraction::of(9, 4).unwrap().as_unit(), 1.0);
    }

    #[test]
    fn json_keeps_the_count_and_unit() {
        let p = Progress::Steps { done: 1, total: 4, unit: Unit::parse("bytes") };
        assert_eq!(p.to_json(), json!({"fraction": 0.25, "done": 1, "total": 4, "unit": "bytes"}));
        assert_eq!(Progress::Indeterminate.to_json(), Value::Null);
        assert_eq!(Progress::Fraction(Fraction::from_percent(70)).to_json(), json!({"fraction": 0.7}));
    }
}
