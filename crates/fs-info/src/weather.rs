//! Open-meteo glue: request URL construction and response parsing, including
//! every failure shape the weather panel needs to render an honest error cell
//! instead of a hang or a silent blank. No clock and no network in here.

use chrono::{Datelike, NaiveDate};
use serde_json::Value;

use fs_js as js;

pub fn build_url(latitude: f64, longitude: f64) -> Option<String> {
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
        return None;
    }
    Some(format!(
        "https://api.open-meteo.com/v1/forecast?latitude={}&longitude={}\
         &current=temperature_2m,weather_code\
         &daily=temperature_2m_max,temperature_2m_min,weather_code\
         &timezone=auto&forecast_days=5",
        js::num_str(latitude),
        js::num_str(longitude),
    ))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    NetworkError,
    HttpError,
    MalformedJson,
    MissingFields,
}

impl Error {
    pub fn as_str(self) -> &'static str {
        match self {
            Error::NetworkError => "network_error",
            Error::HttpError => "http_error",
            Error::MalformedJson => "malformed_json",
            Error::MissingFields => "missing_fields",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Current {
    pub temperature: f64,
    pub code: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: String,
    pub high: f64,
    pub low: f64,
    pub code: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Weather {
    pub current: Current,
    pub forecast: Vec<Day>,
}

/// Status 0 is the HTTP client's own signal for "never reached the server"
/// (DNS failure, no route, timeout), distinct from a real HTTP error status
/// the server sent back, so the two get separate honest error labels.
pub fn parse_response(status: u16, body: &str) -> Result<Weather, Error> {
    if status == 0 {
        return Err(Error::NetworkError);
    }
    if status != 200 {
        return Err(Error::HttpError);
    }
    let data: Value = serde_json::from_str(body).map_err(|_| Error::MalformedJson)?;

    let current = data.get("current").ok_or(Error::MissingFields)?;
    let (Some(temperature), Some(code)) = (
        js::finite_number(current.get("temperature_2m")),
        js::finite_number(current.get("weather_code")),
    ) else {
        return Err(Error::MissingFields);
    };

    let daily = data.get("daily").ok_or(Error::MissingFields)?;
    let array = |key: &str| daily.get(key).and_then(Value::as_array).ok_or(Error::MissingFields);
    let (time, max, min, codes) = (
        array("time")?,
        array("temperature_2m_max")?,
        array("temperature_2m_min")?,
        array("weather_code")?,
    );

    let forecast = time
        .iter()
        .enumerate()
        .filter_map(|(i, date)| {
            Some(Day {
                date: match date {
                    Value::String(s) => s.clone(),
                    other => js::to_str(other),
                },
                high: js::finite_number(max.get(i))?,
                low: js::finite_number(min.get(i))?,
                code: js::finite_number(codes.get(i))?,
            })
        })
        .collect();

    Ok(Weather { current: Current { temperature, code }, forecast })
}

/// WMO weather_code (open-meteo's own scheme) grouped down to the handful of
/// conditions the panel draws an icon for. The key is semantic, so both the
/// label table and the icon table key off one grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Condition {
    Clear,
    PartlyCloudy,
    Overcast,
    Fog,
    Drizzle,
    FreezingRain,
    Rain,
    Snow,
    Showers,
    Thunderstorm,
    Unknown,
}

impl Condition {
    pub fn label(self) -> &'static str {
        match self {
            Condition::Clear => "CLEAR",
            Condition::PartlyCloudy => "PARTLY CLOUDY",
            Condition::Overcast => "OVERCAST",
            Condition::Fog => "FOG",
            Condition::Drizzle => "DRIZZLE",
            Condition::FreezingRain => "FREEZING RAIN",
            Condition::Rain => "RAIN",
            Condition::Snow => "SNOW",
            Condition::Showers => "SHOWERS",
            Condition::Thunderstorm => "THUNDERSTORM",
            Condition::Unknown => "UNAVAILABLE",
        }
    }
}

pub(crate) fn condition(code: impl Into<f64>) -> Condition {
    let code = code.into();
    if code.fract() != 0.0 {
        return Condition::Unknown;
    }
    match code as i64 {
        0 | 1 => Condition::Clear,
        2 => Condition::PartlyCloudy,
        3 => Condition::Overcast,
        45 | 48 => Condition::Fog,
        51 | 53 | 55 => Condition::Drizzle,
        56 | 57 | 66 | 67 => Condition::FreezingRain,
        61 | 63 | 65 => Condition::Rain,
        71 | 73 | 75 | 77 | 85 | 86 => Condition::Snow,
        80..=82 => Condition::Showers,
        95 | 96 | 99 => Condition::Thunderstorm,
        _ => Condition::Unknown,
    }
}

pub fn condition_label(code: impl Into<f64>) -> &'static str {
    condition(code).label()
}

/// The same condition as body text rather than a marker (uppercase belongs
/// to section labels alone), for the panel hero's own line. Derived from the
/// one table so a new condition never has to be written out twice.
pub fn condition_text(code: impl Into<f64>) -> String {
    let label = condition_label(code);
    format!("{}{}", &label[..1], label[1..].to_lowercase())
}

/// Three-letter weekday for a `daily.time` date-only string ("2026-07-28"),
/// sentence case: a forecast row's day name is a word, so it reads in sans
/// and is not uppercased. Date-only ISO strings are UTC midnight, so this is
/// the UTC day of week, never the local one; the local weekday would misdate
/// the first and last forecast rows whenever the host's timezone sits west of
/// UTC. `None` for a string that is not a date.
pub fn weekday_label(date: &str) -> Option<&'static str> {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some(WEEKDAYS[date.weekday().num_days_from_sunday() as usize])
}

pub const FALLBACK_ICON: &str = "thermometer";

/// Condition to icon name: the panel and the bar cell both render these
/// through the icon component, so the active set decides the codepoint and no
/// surface file carries one. Day and night differ only where the sky itself
/// is the subject. An unmapped code falls back to the thermometer, the same
/// "no reading yet" icon the bar cell shows before the first fetch lands.
///
/// `is_day` defaults to day unless explicitly `Some(false)`: a first-fetch
/// caller with no day/night signal yet should read as day rather than
/// guessing night.
pub fn icon_for_code(code: impl Into<f64>, is_day: Option<bool>) -> &'static str {
    let (day, night) = match condition(code) {
        Condition::Clear => ("sun", "moon"),
        Condition::PartlyCloudy => ("cloud-sun", "cloud-moon"),
        Condition::Overcast => ("cloudy", "cloudy"),
        Condition::Fog => ("cloud-fog", "cloud-fog"),
        Condition::Drizzle => ("cloud-drizzle", "cloud-drizzle"),
        Condition::FreezingRain => ("cloud-hail", "cloud-hail"),
        Condition::Rain => ("cloud-rain", "cloud-rain"),
        Condition::Snow => ("cloud-snow", "cloud-snow"),
        Condition::Showers => ("cloud-rain-wind", "cloud-rain-wind"),
        Condition::Thunderstorm => ("cloud-lightning", "cloud-lightning"),
        Condition::Unknown => return FALLBACK_ICON,
    };
    if is_day == Some(false) { night } else { day }
}
