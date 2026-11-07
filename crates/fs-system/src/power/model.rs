//! Battery threshold and formatting helpers. Live UPower readings go through
//! `warn_event`, which hands back the `fired` state for the next call, and the
//! static BATTERY meta rows use the formatters below.

use fs_js as js;

pub const DEFAULT_WARN_PCT: f64 = 10.0;
pub const DEFAULT_CRITICAL_PCT: f64 = 5.0;

/// UPower's `UPowerDeviceState` values, by their wire numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Unknown = 0,
    Charging = 1,
    Discharging = 2,
    Empty = 3,
    FullyCharged = 4,
    PendingCharge = 5,
    PendingDischarge = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fired {
    pub warn: bool,
    pub critical: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarnEvent {
    Warn,
    Critical,
}

impl WarnEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            WarnEvent::Warn => "warn",
            WarnEvent::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarnResult {
    pub fired: Fired,
    pub event: Option<WarnEvent>,
}

/// `prev_pct`/`pct` are whole-number percentages (0..100); UPower's own 0..1
/// fraction is converted exactly once, at the call site, never in here.
/// `fired` is whatever this function returned last call (`Fired::default()` on
/// the very first one).
///
/// Charging re-arms both thresholds immediately, regardless of percentage: a
/// plugged-in battery is never "discharging" mid-crossing, so a later unplug
/// while still low fires again. While discharging, a threshold fires once when
/// the reading is at/below it and hasn't already fired since the last re-arm;
/// a boot (or resume) that starts already below a threshold fires
/// immediately, since `prev_pct` is `None` on the very first call and staying
/// silent about a real critical/low state would be dishonest. `not_rising`
/// guards the fire against a single noisy uptick (UPower's rate estimate can
/// blip for one reading while genuinely still discharging) without weakening
/// the re-arm rule: once fired, a threshold only clears via charging, never by
/// drifting back above the line on its own.
pub fn warn_event(
    prev_pct: Option<f64>,
    pct: f64,
    charging: bool,
    fired: Option<Fired>,
    warn_pct: Option<f64>,
    critical_pct: Option<f64>,
) -> WarnResult {
    let warn_pct = warn_pct.unwrap_or(DEFAULT_WARN_PCT);
    let critical_pct = critical_pct.unwrap_or(DEFAULT_CRITICAL_PCT);
    let prev_fired = fired.unwrap_or_default();

    if charging {
        return WarnResult { fired: Fired::default(), event: None };
    }

    let not_rising = prev_pct.is_none_or(|p| pct <= p);

    if not_rising && pct <= critical_pct && !prev_fired.critical {
        return WarnResult { fired: Fired { warn: true, critical: true }, event: Some(WarnEvent::Critical) };
    }
    if not_rising && pct <= warn_pct && !prev_fired.warn {
        return WarnResult { fired: Fired { warn: true, critical: prev_fired.critical }, event: Some(WarnEvent::Warn) };
    }
    WarnResult { fired: prev_fired, event: None }
}

/// "2h 14m" / "14m" / "1d 3h", lowercase units per the notification
/// relative-time strings.
pub fn format_duration(total_seconds: f64) -> String {
    let total_mins = (total_seconds / 60.0).floor();
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
/// UPower's own aggregate property, not a per-device state parse. `pct` is a
/// whole-number percentage (0..100).
pub fn charge_threshold_active(
    pct: f64,
    state: DeviceState,
    change_rate: f64,
    time_to_full: f64,
    on_battery: bool,
) -> bool {
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
    let ttf = if time_to_full.is_nan() { 0.0 } else { time_to_full };
    rate.abs() <= 0.2 || ttf >= 8.0 * 60.0 * 60.0
}

/// The bar tooltip and the hero meta line's four-way state word.
pub fn charge_state_label(pct: f64, state: DeviceState, on_battery: bool, threshold_active: bool) -> &'static str {
    if threshold_active {
        "THRESHOLD"
    } else if on_battery {
        "ON BATTERY"
    } else if state == DeviceState::FullyCharged || pct >= 100.0 {
        "FULLY CHARGED"
    } else {
        "CHARGING"
    }
}

/// Icon name for the battery's state: a named set draws state, not a decile
/// ramp, so the level detail lives in the percentage beside it. `warn_pct`
/// defaults to the same 10% `warn_event` uses, so the alert icon and the
/// low-battery notification agree on where low starts.
pub fn battery_icon(pct: f64, on_battery: bool, threshold_active: bool, warn_pct: Option<f64>) -> &'static str {
    let warn_pct = warn_pct.unwrap_or(DEFAULT_WARN_PCT);
    if !on_battery && !threshold_active {
        "battery-charging"
    } else if pct <= warn_pct {
        "battery-warning"
    } else if pct >= 66.0 {
        "battery-full"
    } else if pct >= 33.0 {
        "battery-medium"
    } else {
        "battery-low"
    }
}

/// `charge_control_end_threshold`'s only line: the percentage the firmware
/// stops charging at. Linux exposes it per battery under
/// /sys/class/power_supply/<name>/, and UPower carries the <name> as
/// nativePath, so the caller builds the path rather than globbing for it.
///
/// `None` for anything that isn't a whole percentage in 1..100: a battery with
/// no limit support has no such file and `cat` writes nothing to stdout, and
/// 100 is the firmware's own way of saying "charge to full", which is not a
/// limit worth a row of its own.
pub fn parse_charge_limit(text: &str) -> Option<i64> {
    let line = js::trim(text.split('\n').next().unwrap_or(""));
    if line.is_empty() {
        return None;
    }
    let value = js::parse_int(line);
    if !value.is_finite() || value < 1.0 || value >= 100.0 {
        return None;
    }
    Some(value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Option<Fired> {
        Some(Fired::default())
    }

    fn warn(prev: Option<f64>, pct: f64, charging: bool, fired: Fired) -> WarnResult {
        warn_event(prev, pct, charging, Some(fired), None, None)
    }

    // warnEvent

    #[test]
    fn boot_below_warn_fires_immediately() {
        let r = warn_event(None, 8.0, false, fresh(), None, None);
        assert_eq!(r.event, Some(WarnEvent::Warn));
        assert!(r.fired.warn);
        assert!(!r.fired.critical);
    }

    #[test]
    fn boot_below_critical_fires_critical_only() {
        let r = warn_event(None, 3.0, false, fresh(), None, None);
        assert_eq!(r.event, Some(WarnEvent::Critical));
        assert!(r.fired.warn);
        assert!(r.fired.critical);
    }

    #[test]
    fn crossing_fires_warn_once() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        assert_eq!(r1.event, Some(WarnEvent::Warn));
        let r2 = warn(Some(9.0), 9.0, false, r1.fired);
        assert_eq!(r2.event, None);
        assert!(r2.fired.warn);
    }

    #[test]
    fn crossing_fires_critical_after_warn() {
        let r1 = warn(Some(15.0), 9.0, false, Fired::default());
        let r2 = warn(Some(9.0), 4.0, false, r1.fired);
        assert_eq!(r2.event, Some(WarnEvent::Critical));
        assert!(r2.fired.warn);
        assert!(r2.fired.critical);
    }

    #[test]
    fn single_tick_below_both_fires_critical_not_warn() {
        let r = warn(Some(50.0), 2.0, false, Fired::default());
        assert_eq!(r.event, Some(WarnEvent::Critical));
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
        assert_eq!(r3.event, Some(WarnEvent::Warn));
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
        let r = warn_event(None, 25.0, false, fresh(), Some(30.0), Some(15.0));
        assert_eq!(r.event, Some(WarnEvent::Warn));
    }

    #[test]
    fn above_thresholds_never_fires() {
        let r = warn(Some(50.0), 45.0, false, Fired::default());
        assert_eq!(r.event, None);
        assert!(!r.fired.warn);
        assert!(!r.fired.critical);
    }

    // formatDuration

    #[test]
    fn format_duration_minutes_only() {
        assert_eq!(format_duration(14.0 * 60.0), "14m");
    }

    #[test]
    fn format_duration_hours_and_minutes() {
        assert_eq!(format_duration(2.0 * 3600.0 + 14.0 * 60.0), "2h 14m");
    }

    #[test]
    fn format_duration_days_and_hours() {
        assert_eq!(format_duration(27.0 * 3600.0), "1d 3h");
    }

    #[test]
    fn format_duration_zero() {
        assert_eq!(format_duration(0.0), "0m");
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

    // parseChargeLimit

    #[test]
    fn parse_charge_limit_reads_the_threshold_percentage() {
        assert_eq!(parse_charge_limit("80\n"), Some(80));
    }

    #[test]
    fn parse_charge_limit_tolerates_no_trailing_newline() {
        assert_eq!(parse_charge_limit("60"), Some(60));
    }

    #[test]
    fn parse_charge_limit_is_none_when_the_file_is_absent() {
        assert_eq!(parse_charge_limit(""), None);
    }

    #[test]
    fn parse_charge_limit_treats_a_full_charge_target_as_no_limit() {
        assert_eq!(parse_charge_limit("100\n"), None);
    }

    #[test]
    fn parse_charge_limit_rejects_an_out_of_range_or_unparseable_value() {
        assert_eq!(parse_charge_limit("0\n"), None);
        assert_eq!(parse_charge_limit("-5\n"), None);
        assert_eq!(parse_charge_limit("nope\n"), None);
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
        assert!(!charge_threshold_active(50.0, Charging, 10.0, 0.0, true));
    }

    #[test]
    fn threshold_true_pending_charge() {
        assert!(charge_threshold_active(80.0, PendingCharge, 0.0, 0.0, false));
    }

    #[test]
    fn threshold_true_fully_charged_below_99() {
        assert!(charge_threshold_active(95.0, FullyCharged, 0.0, 0.0, false));
    }

    #[test]
    fn threshold_false_fully_charged_at_99() {
        assert!(!charge_threshold_active(99.0, FullyCharged, 0.0, 0.0, false));
    }

    #[test]
    fn threshold_true_charging_near_zero_rate() {
        assert!(charge_threshold_active(50.0, Charging, 0.1, 3600.0, false));
    }

    #[test]
    fn threshold_true_charging_long_time_to_full() {
        assert!(charge_threshold_active(50.0, Charging, 15.0, 9.0 * 3600.0, false));
    }

    #[test]
    fn threshold_false_charging_normally() {
        assert!(!charge_threshold_active(50.0, Charging, 15.0, 3600.0, false));
    }

    #[test]
    fn threshold_false_charging_above_99() {
        assert!(!charge_threshold_active(99.0, Charging, 0.1, 9.0 * 3600.0, false));
    }

    #[test]
    fn threshold_false_discharging_state() {
        assert!(!charge_threshold_active(50.0, Discharging, 0.0, 0.0, false));
    }

    // chargeStateLabel

    #[test]
    fn label_threshold_wins() {
        assert_eq!(charge_state_label(80.0, Charging, false, true), "THRESHOLD");
    }

    #[test]
    fn label_on_battery() {
        assert_eq!(charge_state_label(50.0, Discharging, true, false), "ON BATTERY");
    }

    #[test]
    fn label_fully_charged_by_state() {
        assert_eq!(charge_state_label(99.0, FullyCharged, false, false), "FULLY CHARGED");
    }

    #[test]
    fn label_fully_charged_by_percent() {
        assert_eq!(charge_state_label(100.0, Charging, false, false), "FULLY CHARGED");
    }

    #[test]
    fn label_charging() {
        assert_eq!(charge_state_label(50.0, Charging, false, false), "CHARGING");
    }

    // batteryIcon

    #[test]
    fn icon_charging_beats_every_level() {
        assert_eq!(battery_icon(5.0, false, false, None), "battery-charging");
        assert_eq!(battery_icon(90.0, false, false, None), "battery-charging");
    }

    #[test]
    fn icon_low_reading_warns() {
        assert_eq!(battery_icon(10.0, true, false, None), "battery-warning");
        assert_eq!(battery_icon(3.0, true, false, None), "battery-warning");
    }

    #[test]
    fn icon_level_ramp() {
        assert_eq!(battery_icon(80.0, true, false, None), "battery-full");
        assert_eq!(battery_icon(40.0, true, false, None), "battery-medium");
        assert_eq!(battery_icon(20.0, true, false, None), "battery-low");
    }

    #[test]
    fn icon_threshold_reads_as_a_level_not_a_charge() {
        assert_eq!(battery_icon(50.0, false, true, None), battery_icon(50.0, true, false, None));
    }

    #[test]
    fn icon_warn_percent_is_the_callers_to_set() {
        assert_eq!(battery_icon(18.0, true, false, Some(20.0)), "battery-warning");
        assert_eq!(battery_icon(18.0, true, false, None), "battery-low");
    }

    // formatWh / formatHealthPercent

    // timeRowLabel / timeRowValue

    // rateRowLabel / rateRowValue

}
