//! Battery threshold and formatting helpers. Live UPower readings go through
//! `warn_event`, which hands back the `fired` state for the next call, and the
//! static BATTERY meta rows use the formatters below.

use std::time::Duration;

use fs_js as js;
use fs_types::Percent;
use fs_upower::DeviceState;

/// How low a battery reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Low {
    Warn,
    Critical,
}

/// Where a battery starts reading low: one rule for every device, each
/// device's lines here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub warn: Percent,
    pub critical: Percent,
}

impl Thresholds {
    /// The laptop's defaults under `battery.warnPercent` and
    /// `battery.criticalPercent`.
    pub const LAPTOP: Self = Self { warn: Percent::whole(10), critical: Percent::whole(5) };
    /// The retired `dualsense-bar` command module's.
    pub const DUALSENSE: Self = Self { warn: Percent::whole(20), critical: Percent::whole(10) };
    /// The headset card's ring, at the laptop's defaults; it draws one ink
    /// for both.
    pub const HEADSET: Self = Self { warn: Percent::whole(10), critical: Percent::whole(5) };

    /// These lines with any configured over them.
    pub fn with(self, warn: Option<f64>, critical: Option<f64>) -> Self {
        Self {
            warn: warn.map_or(self.warn, Percent::from_percent),
            critical: critical.map_or(self.critical, Percent::from_percent),
        }
    }

    /// `level` against the lines, at or under each; the caller passes the
    /// level it shows.
    pub fn low(self, level: Percent) -> Option<Low> {
        if level <= self.critical {
            Some(Low::Critical)
        } else if level <= self.warn {
            Some(Low::Warn)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fired {
    pub warn: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarnResult {
    pub fired: Fired,
    pub event: Option<Low>,
}

/// `fired` is whatever this function returned last call (`Fired::default()`
/// on the very first one).
///
/// Charging re-arms both thresholds immediately, regardless of percentage: a
/// plugged-in battery is never "discharging" mid-crossing, so a later unplug
/// while still low fires again. While discharging, a threshold fires once when
/// the reading is at/below it and hasn't already fired since the last re-arm;
/// a boot (or resume) that starts already below a threshold fires
/// immediately, since `prev` is `None` on the very first call and staying
/// silent about a real critical/low state would be dishonest. `not_rising`
/// guards the fire against a single noisy uptick (UPower's rate estimate can
/// blip for one reading while genuinely still discharging) without weakening
/// the re-arm rule: once fired, a threshold only clears via charging, never by
/// drifting back above the line on its own.
pub fn warn_event(prev: Option<Percent>, level: Percent, charging: bool, fired: Fired, lines: Thresholds) -> WarnResult {
    if charging {
        return WarnResult { fired: Fired::default(), event: None };
    }
    let not_rising = prev.is_none_or(|p| level <= p);
    match lines.low(level) {
        Some(Low::Critical) if not_rising && !fired.critical => {
            WarnResult { fired: Fired { warn: true, critical: true }, event: Some(Low::Critical) }
        }
        Some(_) if not_rising && !fired.warn => WarnResult { fired: Fired { warn: true, critical: fired.critical }, event: Some(Low::Warn) },
        _ => WarnResult { fired, event: None },
    }
}

/// "2h 14m" / "14m" / "1d 3h", lowercase units per the notification
/// relative-time strings.
pub fn format_duration(d: Duration) -> String {
    let total_mins = (d.as_secs_f64() / 60.0).floor();
    let hours = (total_mins / 60.0).floor();
    let mins = total_mins % 60.0;
    if hours >= 24.0 {
        return format!("{}d {}h", js::num_str((hours / 24.0).floor()), js::num_str(hours % 24.0));
    }
    if hours > 0.0 {
        return format!("{}h {}m", js::num_str(hours), js::num_str(mins));
    }
    format!("{}m", js::num_str(mins))
}

/// `UPowerDevice.changeRate` is signed (positive charging, negative
/// discharging); callers already know the sign from `state`, so the display
/// text only ever wants the magnitude.
pub fn format_rate(watts: f64) -> String {
    format!("{}W", js::to_fixed(watts.abs(), 1))
}

/// RAPL package power. The counter itself is root-only, so the nix module's
/// power poller publishes the package draw as integer milliwatts averaged over
/// its own interval. `None` on an absent file or anything but one
/// non-negative integer, never 0 or a guess.
pub fn parse_rapl_mw(text: &str) -> Option<f64> {
    let t = js::trim(text);
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(js::parse_int(t) / 1000.0)
}

/// Charge-threshold detection. A laptop holding at a configured charge limit
/// reports one of three UPower shapes that all mean "plugged in but not
/// actually charging toward 100": PendingCharge outright, FullyCharged below
/// 99% (the limit sits under what UPower calls full), or Charging with a
/// near-zero rate or a time-to-full of 8 hours or more. `on_battery` is
/// UPower's own aggregate property, not a per-device state parse. `level` is
/// the whole percent the cell shows.
pub fn charge_threshold_active(
    level: Percent,
    state: DeviceState,
    change_rate: f64,
    time_to_full: Option<Duration>,
    on_battery: bool,
) -> bool {
    let pct = level.as_percent();
    if on_battery {
        return false;
    }
    if state == DeviceState::PendingCharge {
        return true;
    }
    if state == DeviceState::FullyCharged {
        return pct < 99.0;
    }
    if state != DeviceState::Charging || pct >= 99.0 {
        return false;
    }
    let rate = if change_rate.is_nan() { 0.0 } else { change_rate };
    rate.abs() <= 0.2 || time_to_full.is_some_and(|t| t >= Duration::from_secs(8 * 60 * 60))
}

/// The bar tooltip and the hero meta line's four-way state word.
pub fn charge_state_label(level: Percent, state: DeviceState, on_battery: bool, threshold_active: bool) -> &'static str {
    if threshold_active {
        "THRESHOLD"
    } else if on_battery {
        "ON BATTERY"
    } else if state == DeviceState::FullyCharged || level.as_percent() >= 100.0 {
        "FULLY CHARGED"
    } else {
        "CHARGING"
    }
}

/// Icon name for the battery's state: a named set draws state, not a decile
/// ramp, so the level detail lives in the percentage beside it. The alert
/// icon takes the same `lines` as `warn_event`, so it and the low-battery
/// notification agree on where low starts.
pub fn battery_icon(level: Percent, on_battery: bool, threshold_active: bool, lines: Thresholds) -> &'static str {
    let pct = level.as_percent();
    if !on_battery && !threshold_active {
        "battery-charging"
    } else if lines.low(level).is_some() {
        "battery-warning"
    } else if pct >= 66.0 {
        "battery-full"
    } else if pct >= 33.0 {
        "battery-medium"
    } else {
        "battery-low"
    }
}

/// `charge_control_end_threshold`'s percentage as a limit worth showing:
/// whole, in 1..100. 100 is the firmware's way of saying "charge to full",
/// which is not a limit worth a row of its own.
pub fn charge_limit(value: f64) -> Option<u8> {
    (value.is_finite() && (1.0..100.0).contains(&value)).then_some(value as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(value: f64) -> Percent {
        Percent::from_percent(value)
    }

    fn warn(prev: Option<f64>, pct: f64, charging: bool, fired: Fired) -> WarnResult {
        warn_event(prev.map(p), p(pct), charging, fired, Thresholds::LAPTOP)
    }

    // warnEvent

    #[test]
    fn boot_below_warn_fires_immediately() {
        let r = warn(None, 8.0, false, Fired::default());
        assert_eq!(r.event, Some(Low::Warn));
        assert!(r.fired.warn);
        assert!(!r.fired.critical);
    }

    #[test]
    fn boot_below_critical_fires_critical_only() {
        let r = warn(None, 3.0, false, Fired::default());
        assert_eq!(r.event, Some(Low::Critical));
        assert!(r.fired.warn);
        assert!(r.fired.critical);
    }

    #[test]
    fn crossing_fires_warn_once() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        assert_eq!(r1.event, Some(Low::Warn));
        let r2 = warn(Some(9.0), 9.0, false, r1.fired);
        assert_eq!(r2.event, None);
        assert!(r2.fired.warn);
    }

    #[test]
    fn crossing_fires_critical_after_warn() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        let r2 = warn(Some(9.0), 4.0, false, r1.fired);
        assert_eq!(r2.event, Some(Low::Critical));
        assert!(r2.fired.warn);
        assert!(r2.fired.critical);
    }

    #[test]
    fn single_tick_below_both_fires_critical_not_warn() {
        let r = warn(Some(50.0), 2.0, false, Fired::default());
        assert_eq!(r.event, Some(Low::Critical));
        assert!(r.fired.warn);
        assert!(r.fired.critical);
    }

    #[test]
    fn rearm_on_charge_clears_both_flags() {
        let r = warn(Some(4.0), 4.0, true, Fired { warn: true, critical: true });
        assert_eq!(r.event, None);
        assert!(!r.fired.warn);
        assert!(!r.fired.critical);
    }

    #[test]
    fn charge_interruption_refires_warn() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        let r2 = warn(Some(9.0), 9.0, true, r1.fired);
        assert!(!r2.fired.warn);
        let r3 = warn(Some(9.0), 9.0, false, r2.fired);
        assert_eq!(r3.event, Some(Low::Warn));
    }

    #[test]
    fn no_refire_while_still_low_without_recharge() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        let r2 = warn(Some(9.0), 8.0, false, r1.fired);
        assert_eq!(r2.event, None);
        let r3 = warn(Some(8.0), 9.0, false, r2.fired);
        assert_eq!(r3.event, None);
        assert!(r3.fired.warn);
    }

    #[test]
    fn rising_reading_does_not_fire() {
        let r = warn(Some(8.0), 10.0, false, Fired::default());
        assert_eq!(r.event, None);
        assert!(!r.fired.warn);
    }

    #[test]
    fn custom_thresholds_respected() {
        let r = warn_event(None, p(25.0), false, Fired::default(), Thresholds::LAPTOP.with(Some(30.0), Some(15.0)));
        assert_eq!(r.event, Some(Low::Warn));
    }

    #[test]
    fn above_thresholds_never_fires() {
        let r = warn(Some(50.0), 45.0, false, Fired::default());
        assert_eq!(r.event, None);
        assert!(!r.fired.warn);
        assert!(!r.fired.critical);
    }

    #[test]
    fn each_device_keeps_its_lines() {
        let lines = |t: Thresholds| (t.warn.as_percent(), t.critical.as_percent());
        assert_eq!(lines(Thresholds::LAPTOP), (10.0, 5.0));
        assert_eq!(lines(Thresholds::DUALSENSE), (20.0, 10.0));
        assert_eq!(lines(Thresholds::HEADSET), (10.0, 5.0));
        assert_eq!(lines(Thresholds::LAPTOP.with(Some(30.0), None)), (30.0, 5.0));
    }

    #[test]
    fn low_is_at_or_under_each_line() {
        let t = Thresholds::DUALSENSE;
        assert_eq!(t.low(p(21.0)), None);
        assert_eq!(t.low(p(20.0)), Some(Low::Warn));
        assert_eq!(t.low(p(11.0)), Some(Low::Warn));
        assert_eq!(t.low(p(10.0)), Some(Low::Critical));
        assert_eq!(t.low(p(0.0)), Some(Low::Critical));
    }

    // formatDuration

    #[test]
    fn format_duration_minutes_only() {
        assert_eq!(format_duration(Duration::from_secs(14 * 60)), "14m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        assert_eq!(format_duration(Duration::from_secs(2 * 3600 + 14 * 60)), "2h 14m");
    }

    #[test]
    fn format_duration_days_and_hours() {
        assert_eq!(format_duration(Duration::from_secs(27 * 3600)), "1d 3h");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(Duration::ZERO), "0m");
    }

    // formatRate

    #[test]
    fn format_rate_positive() {
        assert_eq!(format_rate(12.34), "12.3W");
    }

    #[test]
    fn format_rate_negative_shows_magnitude() {
        assert_eq!(format_rate(-8.05), "8.1W");
    }

    // charge_limit

    #[test]
    fn charge_limit_keeps_a_percentage_below_full() {
        assert_eq!(charge_limit(80.0), Some(80));
        assert_eq!(charge_limit(1.0), Some(1));
    }

    #[test]
    fn charge_limit_treats_a_full_charge_target_as_no_limit() {
        assert_eq!(charge_limit(100.0), None);
    }

    #[test]
    fn charge_limit_rejects_an_out_of_range_value() {
        assert_eq!(charge_limit(0.0), None);
        assert_eq!(charge_limit(-5.0), None);
        assert_eq!(charge_limit(f64::NAN), None);
    }

    // parseRaplMw

    #[test]
    fn parse_rapl_mw_milliwatts_to_watts() {
        assert_eq!(parse_rapl_mw("8500\n"), Some(8.5));
        assert_eq!(parse_rapl_mw("0"), Some(0.0));
    }

    #[test]
    fn parse_rapl_mw_missing_or_garbage_is_none() {
        assert_eq!(parse_rapl_mw(""), None);
        assert_eq!(parse_rapl_mw("-5"), None);
        assert_eq!(parse_rapl_mw("cat: /run/formalshell/rapl: No such file"), None);
    }

    // chargeThresholdActive

    use DeviceState::*;

    #[test]
    fn threshold_false_on_battery() {
        assert!(!charge_threshold_active(p(50.0), Charging, 10.0, None, true));
    }

    #[test]
    fn threshold_true_pending_charge() {
        assert!(charge_threshold_active(p(80.0), PendingCharge, 0.0, None, false));
    }

    #[test]
    fn threshold_true_fully_charged_below_99() {
        assert!(charge_threshold_active(p(95.0), FullyCharged, 0.0, None, false));
    }

    #[test]
    fn threshold_false_fully_charged_at_99() {
        assert!(!charge_threshold_active(p(99.0), FullyCharged, 0.0, None, false));
    }

    #[test]
    fn threshold_true_charging_near_zero_rate() {
        assert!(charge_threshold_active(p(50.0), Charging, 0.1, Some(Duration::from_secs(3600)), false));
    }

    #[test]
    fn threshold_true_charging_long_time_to_full() {
        assert!(charge_threshold_active(p(50.0), Charging, 15.0, Some(Duration::from_secs(9 * 3600)), false));
    }

    #[test]
    fn threshold_false_charging_normally() {
        assert!(!charge_threshold_active(p(50.0), Charging, 15.0, Some(Duration::from_secs(3600)), false));
    }

    #[test]
    fn threshold_false_charging_above_99() {
        assert!(!charge_threshold_active(p(99.0), Charging, 0.1, Some(Duration::from_secs(9 * 3600)), false));
    }

    #[test]
    fn threshold_false_discharging_state() {
        assert!(!charge_threshold_active(p(50.0), Discharging, 0.0, None, false));
    }

    // chargeStateLabel

    #[test]
    fn label_threshold_wins() {
        assert_eq!(charge_state_label(p(80.0), Charging, false, true), "THRESHOLD");
    }

    #[test]
    fn label_on_battery() {
        assert_eq!(charge_state_label(p(50.0), Discharging, true, false), "ON BATTERY");
    }

    #[test]
    fn label_fully_charged_by_state() {
        assert_eq!(charge_state_label(p(99.0), FullyCharged, false, false), "FULLY CHARGED");
    }

    #[test]
    fn label_fully_charged_by_percent() {
        assert_eq!(charge_state_label(p(100.0), Charging, false, false), "FULLY CHARGED");
    }

    #[test]
    fn label_charging() {
        assert_eq!(charge_state_label(p(50.0), Charging, false, false), "CHARGING");
    }

    // batteryIcon

    #[test]
    fn icon_charging_beats_every_level() {
        assert_eq!(battery_icon(p(5.0), false, false, Thresholds::LAPTOP), "battery-charging");
        assert_eq!(battery_icon(p(90.0), false, false, Thresholds::LAPTOP), "battery-charging");
    }

    #[test]
    fn icon_low_reading_warns() {
        assert_eq!(battery_icon(p(10.0), true, false, Thresholds::LAPTOP), "battery-warning");
        assert_eq!(battery_icon(p(3.0), true, false, Thresholds::LAPTOP), "battery-warning");
    }

    #[test]
    fn icon_level_ramp() {
        assert_eq!(battery_icon(p(80.0), true, false, Thresholds::LAPTOP), "battery-full");
        assert_eq!(battery_icon(p(40.0), true, false, Thresholds::LAPTOP), "battery-medium");
        assert_eq!(battery_icon(p(20.0), true, false, Thresholds::LAPTOP), "battery-low");
    }

    #[test]
    fn icon_threshold_reads_as_a_level_not_a_charge() {
        assert_eq!(battery_icon(p(50.0), false, true, Thresholds::LAPTOP), battery_icon(p(50.0), true, false, Thresholds::LAPTOP));
    }

    #[test]
    fn icon_warn_percent_is_the_callers_to_set() {
        assert_eq!(battery_icon(p(18.0), true, false, Thresholds::LAPTOP.with(Some(20.0), None)), "battery-warning");
        assert_eq!(battery_icon(p(18.0), true, false, Thresholds::LAPTOP), "battery-low");
    }

    // formatWh / formatHealthPercent

    // timeRowLabel / timeRowValue

    // rateRowLabel / rateRowValue

}
