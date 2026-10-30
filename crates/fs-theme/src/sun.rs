//! Sunrise and sunset, and the mode the clock resolves to between them.
//!
//! The formula is the NOAA solar-position spreadsheet, the path elementary's
//! settings-daemon takes to the same pair. Both references are GPL: the
//! recipe and its constants are read here, no line of either is ported.
//!
//! Every function takes the clock as an argument, so nothing here reads the
//! system time and the module stays deterministic under test. The caller owns
//! the timer, the location and the writes. Dates are `chrono::DateTime<Tz>`;
//! the service passes `chrono::Local`, tests pin a fixed offset.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike};

/// 90 degrees plus 50 arcminutes for the sun's own radius and the standard
/// refraction allowance, so the times land on a published table rather than
/// on the geometric horizon crossing.
const DARK_ZENITH_DEG: f64 = 90.833;

/// The window with no location to compute one from, elementary's own
/// fallback for a machine geoclue cannot place.
const FALLBACK_DARK_HOUR: u32 = 20;
const FALLBACK_LIGHT_HOUR: u32 = 6;

const MINUTES_PER_DAY: f64 = 1440.0;

/// A day with no sunrise or sunset at all: `Night` where the sun never
/// climbs to the zenith above, `Day` where it never drops to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Polar {
    None,
    Day,
    Night,
}

impl Polar {
    pub fn as_str(self) -> &'static str {
        match self {
            Polar::None => "",
            Polar::Day => "day",
            Polar::Night => "night",
        }
    }
}

/// Minutes are since the zone's local midnight and can fall outside 0..1440
/// where solar noon sits near the edge of the zone's own day. Both are `None`
/// when `polar` is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SunTimes {
    pub latitude: f64,
    pub longitude: f64,
    pub tz_offset_hours: f64,
    pub polar: Polar,
    pub sunrise_minutes: Option<f64>,
    pub sunset_minutes: Option<f64>,
}

/// A manual flip snoozing the schedule for one cycle.
#[derive(Debug, Clone, PartialEq)]
pub struct Override {
    pub mode: String,
    pub until_ms: i64,
}

fn deg2rad(degrees: f64) -> f64 {
    std::f64::consts::PI * degrees / 180.0
}

fn rad2deg(radians: f64) -> f64 {
    radians * 180.0 / std::f64::consts::PI
}

/// JS `Math.round`: halves go up, also for negatives.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// The spreadsheet's serial date: whole days from 1900-01-01 to the date's
/// own UTC day, plus the two days its 1900 leap-year quirk carries. Read off
/// the UTC day rather than the local one, as the reference does; a zone far
/// enough from UTC that the two disagree gets the adjacent day's pair, about
/// a minute out either way.
fn date_serial<Tz: TimeZone>(date: &DateTime<Tz>) -> f64 {
    let epoch = NaiveDate::from_ymd_opt(1900, 1, 1).unwrap();
    (date.naive_utc().date() - epoch).num_days() as f64 + 2.0
}

/// Minutes since local midnight, the unit both times below are in.
pub fn local_minutes<Tz: TimeZone>(date: &DateTime<Tz>) -> f64 {
    date.hour() as f64 * 60.0 + date.minute() as f64 + date.second() as f64 / 60.0
}

/// The sun's declination in degrees and the equation of time in minutes
/// for a Julian day, the spreadsheet's columns T and V.
fn solar(julian_day: f64) -> (f64, f64) {
    let century = (julian_day - 2451545.0) / 36525.0;
    let mean_long = (280.46646 + century * (36000.76983 + century * 0.0003032)) % 360.0;
    let mean_anom = 357.52911 + century * (35999.05029 - 0.0001537 * century);
    let eccentricity = 0.016708634 - century * (0.000042037 + 0.0000001267 * century);
    let centre = deg2rad(mean_anom).sin() * (1.914602 - century * (0.004817 + 0.000014 * century))
        + deg2rad(2.0 * mean_anom).sin() * (0.019993 - 0.000101 * century)
        + deg2rad(3.0 * mean_anom).sin() * 0.000289;
    let app_long =
        mean_long + centre - 0.00569 - 0.00478 * deg2rad(125.04 - 1934.136 * century).sin();
    let mean_obliquity = 23.0
        + (26.0 + (21.448 - century * (46.815 + century * (0.00059 - century * 0.001813))) / 60.0)
            / 60.0;
    let obliquity = mean_obliquity + 0.00256 * deg2rad(125.04 - 1934.136 * century).cos();
    let declination = rad2deg((deg2rad(obliquity).sin() * deg2rad(app_long).sin()).asin());
    let var_y = deg2rad(obliquity / 2.0).tan() * deg2rad(obliquity / 2.0).tan();
    let equation_of_time = 4.0
        * rad2deg(
            var_y * (2.0 * deg2rad(mean_long)).sin()
                - 2.0 * eccentricity * deg2rad(mean_anom).sin()
                + 4.0
                    * eccentricity
                    * var_y
                    * deg2rad(mean_anom).sin()
                    * (2.0 * deg2rad(mean_long)).cos()
                - 0.5 * var_y * var_y * (4.0 * deg2rad(mean_long)).sin()
                - 1.25 * eccentricity * eccentricity * (2.0 * deg2rad(mean_anom)).sin(),
        );
    (declination, equation_of_time)
}

/// `tz_offset_hours` is the offset the times come back in, hours east of
/// UTC; `None` is the date's own local offset, which makes the pair read as
/// wall clock. Returns `None` for coordinates outside the globe (or not
/// finite).
pub fn sun_times<Tz: TimeZone>(
    date: &DateTime<Tz>,
    latitude: f64,
    longitude: f64,
    tz_offset_hours: Option<f64>,
) -> Option<SunTimes> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }

    let tz_offset =
        tz_offset_hours.unwrap_or_else(|| date.offset().fix().local_minus_utc() as f64 / 3600.0);
    let (declination, equation_of_time) = solar(date_serial(date) + 2415018.5 - tz_offset / 24.0);
    let cos_hour_angle = deg2rad(DARK_ZENITH_DEG).cos()
        / (deg2rad(latitude).cos() * deg2rad(declination).cos())
        - deg2rad(latitude).tan() * deg2rad(declination).tan();
    let solar_noon = 720.0 - 4.0 * longitude - equation_of_time + tz_offset * 60.0;

    let mut times = SunTimes {
        latitude,
        longitude,
        tz_offset_hours: tz_offset,
        polar: Polar::None,
        sunrise_minutes: None,
        sunset_minutes: None,
    };
    // acos has no answer here, and which side it ran off says which way the
    // whole day went: the reference returns a NaN pair instead, which every
    // caller would have to re-derive.
    // Not a range check: a NaN must fall through to acos as in JS.
    #[allow(clippy::manual_range_contains)]
    if cos_hour_angle > 1.0 || cos_hour_angle < -1.0 {
        times.polar = if cos_hour_angle > 1.0 {
            Polar::Night
        } else {
            Polar::Day
        };
        return Some(times);
    }
    let hour_angle = rad2deg(cos_hour_angle.acos());
    times.sunrise_minutes = Some(solar_noon - hour_angle * 4.0);
    times.sunset_minutes = Some(solar_noon + hour_angle * 4.0);
    Some(times)
}

/// The window a machine with no location falls back on.
pub fn fallback_dark<Tz: TimeZone>(now: &DateTime<Tz>) -> bool {
    let hour = now.hour();
    !(FALLBACK_LIGHT_HOUR..FALLBACK_DARK_HOUR).contains(&hour)
}

/// The sun's height above the geometric horizon in degrees at `at`, off the
/// same spreadsheet: its hour angle from the true solar time at that
/// instant. No refraction correction, which only lifts the sun near the
/// horizon by under a degree. `None` for coordinates outside the globe.
pub fn elevation<Tz: TimeZone>(at: &DateTime<Tz>, latitude: f64, longitude: f64) -> Option<f64> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    let utc = at.with_timezone(&chrono::Utc);
    let julian_day = utc.timestamp_millis() as f64 / 86_400_000.0 + 2440587.5;
    let (declination, equation_of_time) = solar(julian_day);
    let utc_minutes = utc.hour() as f64 * 60.0 + utc.minute() as f64 + (utc.second() as f64 + utc.nanosecond() as f64 / 1e9) / 60.0;
    let true_solar = (utc_minutes + equation_of_time + 4.0 * longitude).rem_euclid(MINUTES_PER_DAY);
    let hour_angle = true_solar / 4.0 - 180.0;
    let (lat, dec) = (deg2rad(latitude), deg2rad(declination));
    let cos_zenith = lat.sin() * dec.sin() + lat.cos() * dec.cos() * deg2rad(hour_angle).cos();
    Some(90.0 - rad2deg(cos_zenith.clamp(-1.0, 1.0).acos()))
}

/// A stand-in elevation with no location, on the fallback window's own
/// clock: up at 06:00, 50 degrees at 13:00, down at 20:00, and 30 below
/// at 01:00.
pub fn fallback_elevation<Tz: TimeZone>(at: &DateTime<Tz>) -> f64 {
    let (rise, set) = (FALLBACK_LIGHT_HOUR as f64 * 60.0, FALLBACK_DARK_HOUR as f64 * 60.0);
    let m = local_minutes(at);
    let pi = std::f64::consts::PI;
    if (rise..set).contains(&m) {
        return 50.0 * (pi * (m - rise) / (set - rise)).sin();
    }
    let night = MINUTES_PER_DAY - (set - rise);
    -30.0 * (pi * (m - set).rem_euclid(MINUTES_PER_DAY) / night).sin()
}

/// `times` `None` means no location, so the fallback window answers instead.
pub fn is_dark<Tz: TimeZone>(now: &DateTime<Tz>, times: Option<&SunTimes>) -> bool {
    let Some(times) = times else {
        return fallback_dark(now);
    };
    match times.polar {
        Polar::Night => return true,
        Polar::Day => return false,
        Polar::None => {}
    }
    let minutes = local_minutes(now);
    // JS reads a missing minute as 0 in the comparison.
    minutes < times.sunrise_minutes.unwrap_or(0.0) || minutes >= times.sunset_minutes.unwrap_or(0.0)
}

/// A wall-clock time in `tz`. A time inside a DST gap lands past it, as JS
/// does; an ambiguous one takes the first occurrence.
fn local_datetime<Tz: TimeZone>(tz: &Tz, naive: NaiveDateTime) -> DateTime<Tz> {
    match tz.from_local_datetime(&naive) {
        chrono::LocalResult::Single(d) => d,
        chrono::LocalResult::Ambiguous(first, _) => first,
        chrono::LocalResult::None => {
            let before = tz.from_utc_datetime(&(naive - Duration::days(1)));
            let offset = before.offset().fix().local_minus_utc() as i64;
            tz.from_utc_datetime(&(naive - Duration::seconds(offset)))
        }
    }
}

fn at_minutes<Tz: TimeZone>(now: &DateTime<Tz>, minutes: f64) -> DateTime<Tz> {
    let midnight = local_datetime(
        &now.timezone(),
        now.date_naive().and_hms_opt(0, 0, 0).unwrap(),
    );
    midnight + Duration::milliseconds(js_round(minutes * 60000.0) as i64)
}

fn next_local_midnight<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Tz> {
    let next = now.date_naive().succ_opt().unwrap();
    local_datetime(&now.timezone(), next.and_hms_opt(0, 0, 0).unwrap())
}

fn fallback_next_change<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Tz> {
    let hour = now.hour();
    if hour < FALLBACK_LIGHT_HOUR {
        return at_minutes(now, FALLBACK_LIGHT_HOUR as f64 * 60.0);
    }
    if hour < FALLBACK_DARK_HOUR {
        return at_minutes(now, FALLBACK_DARK_HOUR as f64 * 60.0);
    }
    next_local_midnight(now) + Duration::hours(FALLBACK_LIGHT_HOUR as i64)
}

/// When the mode this schedule resolves to changes next. `times` must be the
/// pair for `now`'s own day; past today's sunset that means tomorrow's
/// sunrise, recomputed off the coordinates the pair carries. A polar day
/// answers local midnight: nothing flips before the calculation itself does.
pub fn next_change<Tz: TimeZone>(now: &DateTime<Tz>, times: Option<&SunTimes>) -> DateTime<Tz> {
    let Some(times) = times else {
        return fallback_next_change(now);
    };
    if times.polar != Polar::None {
        return next_local_midnight(now);
    }
    let minutes = local_minutes(now);
    let sunrise = times.sunrise_minutes.unwrap_or(0.0);
    let sunset = times.sunset_minutes.unwrap_or(0.0);
    if minutes < sunrise {
        return at_minutes(now, sunrise);
    }
    if minutes < sunset {
        return at_minutes(now, sunset);
    }
    // Noon tomorrow rather than now plus 24 hours: a DST jump would land the
    // latter back on today.
    let noon = now
        .date_naive()
        .succ_opt()
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let tomorrow = local_datetime(&now.timezone(), noon);
    match sun_times(
        &tomorrow,
        times.latitude,
        times.longitude,
        Some(times.tz_offset_hours),
    ) {
        Some(next) if next.polar == Polar::None => {
            at_minutes(&tomorrow, next.sunrise_minutes.unwrap_or(0.0))
        }
        _ => next_local_midnight(now),
    }
}

/// The mode `theme.mode` resolves to, or `None` for a key that leaves the
/// mode to state.json alone. "dark" and "light" pin it; "auto" reads the
/// schedule, unless an override is still snoozing it (one cycle,
/// elementary's own reading of a manual flip under a schedule).
pub fn effective_mode<Tz: TimeZone>(
    key: &str,
    now: &DateTime<Tz>,
    times: Option<&SunTimes>,
    override_: Option<&Override>,
) -> Option<&'static str> {
    match key {
        "dark" => return Some("dark"),
        "light" => return Some("light"),
        "auto" => {}
        _ => return None,
    }
    if let Some(o) = override_.filter(|o| o.until_ms > now.timestamp_millis()) {
        return Some(if o.mode == "light" { "light" } else { "dark" });
    }
    Some(if is_dark(now, times) { "dark" } else { "light" })
}

/// Minutes since local midnight as wall clock, for `theme status`. Wraps a
/// pair whose solar noon put it outside the day; `None` or non-finite is "".
pub fn hhmm(minutes: Option<f64>) -> String {
    let Some(minutes) = minutes.filter(|m| m.is_finite()) else {
        return String::new();
    };
    let mut total = js_round(minutes) % MINUTES_PER_DAY;
    if total < 0.0 {
        total += MINUTES_PER_DAY;
    }
    let hours = (total / 60.0).floor() as u32;
    let rest = (total % 60.0) as u32;
    format!("{hours:02}:{rest:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, FixedOffset, Utc};

    const TOLERANCE_MINUTES: f64 = 3.0;

    fn utc(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap()
    }

    /// `new Date(y, m, d, h, min)` in a pinned +02:00 zone, month 1-based.
    fn local(y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(2 * 3600)
            .unwrap()
            .with_ymd_and_hms(y, m, d, h, min, 0)
            .unwrap()
    }

    fn minutes(hh: u32, mm: u32) -> f64 {
        (hh * 60 + mm) as f64
    }

    fn close_to(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= TOLERANCE_MINUTES
    }

    fn times(sunrise: f64, sunset: f64) -> SunTimes {
        SunTimes {
            latitude: 51.4779,
            longitude: -0.0015,
            tz_offset_hours: 2.0,
            polar: Polar::None,
            sunrise_minutes: Some(sunrise),
            sunset_minutes: Some(sunset),
        }
    }

    fn polar(polar: Polar) -> SunTimes {
        SunTimes {
            latitude: 0.0,
            longitude: 0.0,
            tz_offset_hours: 0.0,
            polar,
            sunrise_minutes: None,
            sunset_minutes: None,
        }
    }

    #[test]
    fn sun_times_greenwich_equinox() {
        let t = sun_times(&utc(2026, 9, 22, 12), 51.4779, -0.0015, Some(0.0)).unwrap();
        assert_eq!(t.polar, Polar::None);
        assert!(close_to(t.sunrise_minutes.unwrap(), minutes(5, 45)));
        assert!(close_to(t.sunset_minutes.unwrap(), minutes(17, 58)));
    }

    #[test]
    fn sun_times_high_latitude_summer() {
        let t = sun_times(&utc(2026, 6, 21, 12), 59.3293, 18.0686, Some(0.0)).unwrap();
        assert_eq!(t.polar, Polar::None);
        assert!(close_to(t.sunrise_minutes.unwrap(), minutes(1, 30)));
        assert!(close_to(t.sunset_minutes.unwrap(), minutes(20, 8)));
    }

    #[test]
    fn sun_times_southern_hemisphere_winter() {
        let t = sun_times(&utc(2026, 7, 15, 12), -33.8688, 151.2093, Some(10.0)).unwrap();
        assert_eq!(t.polar, Polar::None);
        assert!(close_to(t.sunrise_minutes.unwrap(), minutes(6, 58)));
        assert!(close_to(t.sunset_minutes.unwrap(), minutes(17, 4)));
    }

    #[test]
    fn sun_times_can_fall_outside_the_day() {
        let t = sun_times(&utc(2026, 7, 15, 12), -33.8688, 151.2093, Some(0.0)).unwrap();
        assert!(t.sunrise_minutes.unwrap() < 0.0);
        assert_eq!(hhmm(t.sunrise_minutes), "20:58");
    }

    #[test]
    fn sun_times_polar_day() {
        let t = sun_times(&utc(2026, 6, 21, 12), 78.2232, 15.6469, Some(0.0)).unwrap();
        assert_eq!(t.polar.as_str(), "day");
        assert_eq!(t.sunrise_minutes, None);
        assert_eq!(t.sunset_minutes, None);
    }

    #[test]
    fn sun_times_polar_night() {
        let t = sun_times(&utc(2026, 6, 21, 12), -75.1, 123.33, Some(0.0)).unwrap();
        assert_eq!(t.polar.as_str(), "night");
        assert_eq!(t.sunrise_minutes, None);
    }

    #[test]
    fn sun_times_null_for_out_of_range_latitude() {
        assert_eq!(sun_times(&utc(2026, 6, 21, 12), 91.0, 0.0, Some(0.0)), None);
    }

    #[test]
    fn sun_times_null_for_out_of_range_longitude() {
        assert_eq!(
            sun_times(&utc(2026, 6, 21, 12), 0.0, -181.0, Some(0.0)),
            None
        );
    }

    /// A typed f64 cannot carry the string "52", so only the NaN case applies.
    #[test]
    fn sun_times_null_for_non_number_coordinates() {
        assert_eq!(
            sun_times(&utc(2026, 6, 21, 12), 52.0, f64::NAN, Some(0.0)),
            None
        );
    }

    #[test]
    fn sun_times_defaults_to_the_local_offset() {
        let date = utc(2026, 6, 21, 12).with_timezone(&FixedOffset::east_opt(2 * 3600).unwrap());
        let local = sun_times(&date, 59.3293, 18.0686, None).unwrap();
        let utc = sun_times(&date, 59.3293, 18.0686, Some(0.0)).unwrap();
        let offset = 2.0;
        assert!(
            ((local.sunrise_minutes.unwrap() - utc.sunrise_minutes.unwrap()) - offset * 60.0).abs()
                < 1.0
        );
    }

    #[test]
    fn fallback_dark_window_edges() {
        assert!(!fallback_dark(&local(2026, 9, 22, 19, 59)));
        assert!(fallback_dark(&local(2026, 9, 22, 20, 0)));
        assert!(fallback_dark(&local(2026, 9, 22, 5, 59)));
        assert!(!fallback_dark(&local(2026, 9, 22, 6, 0)));
    }

    #[test]
    fn fallback_dark_across_midnight() {
        assert!(fallback_dark(&local(2026, 9, 22, 23, 30)));
        assert!(fallback_dark(&local(2026, 9, 22, 0, 30)));
        assert!(!fallback_dark(&local(2026, 9, 22, 12, 0)));
    }

    #[test]
    fn is_dark_between_sunset_and_sunrise() {
        let t = times(6.0 * 60.0, 18.0 * 60.0);
        assert!(is_dark(&local(2026, 9, 22, 5, 59), Some(&t)));
        assert!(!is_dark(&local(2026, 9, 22, 6, 0), Some(&t)));
        assert!(!is_dark(&local(2026, 9, 22, 17, 59), Some(&t)));
        assert!(is_dark(&local(2026, 9, 22, 18, 0), Some(&t)));
    }

    #[test]
    fn is_dark_polar() {
        let night = polar(Polar::Night);
        let day = polar(Polar::Day);
        assert!(is_dark(&local(2026, 9, 22, 12, 0), Some(&night)));
        assert!(!is_dark(&local(2026, 9, 22, 12, 0), Some(&day)));
        assert!(!is_dark(&local(2026, 9, 22, 2, 0), Some(&day)));
    }

    #[test]
    fn is_dark_without_times_takes_the_fallback_window() {
        assert!(is_dark(&local(2026, 9, 22, 21, 0), None));
        assert!(!is_dark(&local(2026, 9, 22, 12, 0), None));
    }

    #[test]
    fn next_change_before_sunrise_is_sunrise() {
        let next = next_change(
            &local(2026, 9, 22, 3, 0),
            Some(&times(6.0 * 60.0, 18.0 * 60.0)),
        );
        assert_eq!(next.hour(), 6);
        assert_eq!(next.minute(), 0);
        assert_eq!(next.day(), 22);
    }

    #[test]
    fn next_change_during_the_day_is_sunset() {
        let next = next_change(
            &local(2026, 9, 22, 10, 0),
            Some(&times(6.0 * 60.0, 18.0 * 60.0)),
        );
        assert_eq!(next.hour(), 18);
        assert_eq!(next.day(), 22);
    }

    #[test]
    fn next_change_after_sunset_is_tomorrows_sunrise() {
        let now = local(2026, 9, 22, 23, 0);
        let next = next_change(&now, Some(&times(6.0 * 60.0, 18.0 * 60.0)));
        assert_eq!(next.day(), 23);
        assert!(next.timestamp_millis() > now.timestamp_millis());
        let tomorrow = sun_times(&local(2026, 9, 23, 12, 0), 51.4779, -0.0015, None).unwrap();
        assert!(
            ((next.hour() * 60 + next.minute()) as f64 - tomorrow.sunrise_minutes.unwrap()).abs()
                < 1.0
        );
    }

    #[test]
    fn next_change_polar_is_local_midnight() {
        let next = next_change(&local(2026, 9, 22, 14, 0), Some(&polar(Polar::Day)));
        assert_eq!(next.day(), 23);
        assert_eq!(next.hour(), 0);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn next_change_without_times_walks_the_fallback_window() {
        let morning = next_change(&local(2026, 9, 22, 3, 0), None);
        assert_eq!(morning.hour(), 6);
        assert_eq!(morning.day(), 22);
        let noon = next_change(&local(2026, 9, 22, 12, 0), None);
        assert_eq!(noon.hour(), 20);
        assert_eq!(noon.day(), 22);
        let night = next_change(&local(2026, 9, 22, 23, 0), None);
        assert_eq!(night.hour(), 6);
        assert_eq!(night.day(), 23);
    }

    #[test]
    fn effective_mode_pinned_keys() {
        assert_eq!(
            effective_mode("dark", &local(2026, 9, 22, 12, 0), None, None),
            Some("dark")
        );
        assert_eq!(
            effective_mode("light", &local(2026, 9, 22, 23, 0), None, None),
            Some("light")
        );
    }

    #[test]
    fn effective_mode_manual_key_leaves_the_mode_alone() {
        assert_eq!(
            effective_mode("", &local(2026, 9, 22, 12, 0), None, None),
            None
        );
        assert_eq!(
            effective_mode("sunset-to-sunrise", &local(2026, 9, 22, 12, 0), None, None),
            None
        );
    }

    #[test]
    fn effective_mode_auto_reads_the_schedule() {
        let t = times(6.0 * 60.0, 18.0 * 60.0);
        assert_eq!(
            effective_mode("auto", &local(2026, 9, 22, 12, 0), Some(&t), None),
            Some("light")
        );
        assert_eq!(
            effective_mode("auto", &local(2026, 9, 22, 19, 0), Some(&t), None),
            Some("dark")
        );
    }

    #[test]
    fn effective_mode_auto_without_location_reads_the_fallback() {
        assert_eq!(
            effective_mode("auto", &local(2026, 9, 22, 21, 0), None, None),
            Some("dark")
        );
        assert_eq!(
            effective_mode("auto", &local(2026, 9, 22, 7, 0), None, None),
            Some("light")
        );
    }

    #[test]
    fn effective_mode_auto_override_wins_until_it_expires() {
        let t = times(6.0 * 60.0, 18.0 * 60.0);
        let now = local(2026, 9, 22, 12, 0);
        let live = Override {
            mode: "dark".into(),
            until_ms: local(2026, 9, 22, 18, 0).timestamp_millis(),
        };
        assert_eq!(
            effective_mode("auto", &now, Some(&t), Some(&live)),
            Some("dark")
        );
        let expired = Override {
            mode: "dark".into(),
            until_ms: local(2026, 9, 22, 11, 0).timestamp_millis(),
        };
        assert_eq!(
            effective_mode("auto", &now, Some(&t), Some(&expired)),
            Some("light")
        );
    }

    #[test]
    fn effective_mode_auto_override_of_an_unknown_mode_reads_as_dark() {
        let live = Override {
            mode: "sepia".into(),
            until_ms: local(2026, 9, 22, 18, 0).timestamp_millis(),
        };
        assert_eq!(
            effective_mode(
                "auto",
                &local(2026, 9, 22, 12, 0),
                Some(&times(6.0 * 60.0, 18.0 * 60.0)),
                Some(&live)
            ),
            Some("dark")
        );
    }

    #[test]
    fn hhmm_formats_wall_clock() {
        assert_eq!(hhmm(Some(0.0)), "00:00");
        assert_eq!(hhmm(Some(345.6)), "05:46");
        assert_eq!(hhmm(Some(1080.0)), "18:00");
    }

    #[test]
    fn hhmm_wraps_a_pair_outside_the_day() {
        assert_eq!(hhmm(Some(-90.0)), "22:30");
        assert_eq!(hhmm(Some(1500.0)), "01:00");
    }

    #[test]
    fn hhmm_empty_for_no_time() {
        assert_eq!(hhmm(None), "");
        assert_eq!(hhmm(Some(f64::NAN)), "");
    }

    /// Amsterdam at the June solstice: 90 - 52.37 + 23.44 at solar noon,
    /// and the refracted zenith the times are cut at on either side of it.
    #[test]
    fn the_elevation_meets_the_times_at_the_horizon() {
        let (lat, lon) = (52.37, 4.9);
        let noon = (0..1440)
            .map(|m| Utc.with_ymd_and_hms(2026, 6, 21, 0, 0, 0).unwrap() + Duration::minutes(m))
            .map(|t| elevation(&t, lat, lon).unwrap())
            .fold(f64::MIN, f64::max);
        assert!((noon - 61.07).abs() < 0.2, "{noon}");
        let day = Utc.with_ymd_and_hms(2026, 6, 21, 12, 0, 0).unwrap();
        let times = sun_times(&day, lat, lon, Some(0.0)).unwrap();
        for minutes in [times.sunrise_minutes.unwrap(), times.sunset_minutes.unwrap()] {
            let at = Utc.with_ymd_and_hms(2026, 6, 21, 0, 0, 0).unwrap() + Duration::milliseconds((minutes * 60000.0) as i64);
            let e = elevation(&at, lat, lon).unwrap();
            assert!((e + 0.833).abs() < 0.3, "{minutes}: {e}");
        }
        let midnight = elevation(&Utc.with_ymd_and_hms(2026, 12, 21, 23, 40, 0).unwrap(), lat, lon).unwrap();
        assert!((midnight + 61.07).abs() < 0.3, "{midnight}");
        assert_eq!(elevation(&day, 91.0, 0.0), None);
    }

    #[test]
    fn the_fallback_sun_rides_the_fallback_window() {
        let at = |h, m| local(2026, 3, 1, h, m);
        assert!(fallback_elevation(&at(6, 0)).abs() < 1e-9);
        assert!(fallback_elevation(&at(20, 0)).abs() < 1e-9);
        assert!((fallback_elevation(&at(13, 0)) - 50.0).abs() < 1e-9);
        assert!((fallback_elevation(&at(1, 0)) + 30.0).abs() < 1e-9);
        assert!(fallback_elevation(&at(5, 59)) < 0.0 && fallback_elevation(&at(6, 1)) > 0.0);
    }
}
