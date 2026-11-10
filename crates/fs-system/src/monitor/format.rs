//! Display strings for the monitor view. A missing reading is a dash,
//! everywhere: a reading nobody has taken yet must not render as a zero.
//! Fractions are 0..1, the repo-wide convention.

use fs_js as js;
use fs_types::Fraction;

fn missing(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

/// Whole percent, for machine-wide figures.
pub fn pct(fraction: Option<Fraction>) -> String {
    match fraction {
        None => "--".into(),
        Some(f) => format!("{}%", f.to_percent().rounded()),
    }
}

/// One decimal, for a process's share of the whole machine, where 0.4% and
/// 4.0% are the difference between idle and busy on a many-thread box.
pub fn proc_pct(fraction: Option<Fraction>) -> String {
    match fraction {
        None => "--".into(),
        Some(f) => format!("{}%", js::to_fixed(f.as_percent(), 1)),
    }
}

pub fn bytes(value: Option<f64>) -> String {
    let Some(value) = missing(value) else {
        return "--".into();
    };
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut scaled = value;
    let mut i = 0;
    while scaled >= 1024.0 && i < UNITS.len() - 1 {
        scaled /= 1024.0;
        i += 1;
    }
    let num = if i == 0 { js::num_str(js::round(scaled)) } else { js::to_fixed(scaled, 1) };
    format!("{num}{}", UNITS[i])
}

pub fn rate(bytes_per_sec: Option<f64>) -> String {
    match missing(bytes_per_sec) {
        None => "--".into(),
        Some(v) => format!("{}/s", bytes(Some(v))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_readings_are_dashes_never_zeros() {
        assert_eq!(pct(None), "--");
        assert_eq!(proc_pct(None), "--");
        assert_eq!(bytes(None), "--");
        assert_eq!(rate(None), "--");
    }

    #[test]
    fn percentages() {
        assert_eq!(pct(Some(Fraction::from_fraction(0.514))), "51%");
        assert_eq!(pct(Some(Fraction::from_fraction(0.0))), "0%");
        assert_eq!(proc_pct(Some(Fraction::from_fraction(0.0041))), "0.4%");
        assert_eq!(proc_pct(Some(Fraction::from_fraction(0.165))), "16.5%");
    }

    #[test]
    fn bytes_step_through_binary_units() {
        assert_eq!(bytes(Some(0.0)), "0B");
        assert_eq!(bytes(Some(141.0)), "141B");
        assert_eq!(bytes(Some(1536.0)), "1.5K");
        assert_eq!(bytes(Some(3.6 * 1024.0 * 1024.0)), "3.6M");
        assert_eq!(rate(Some(2048.0)), "2.0K/s");
    }
}
