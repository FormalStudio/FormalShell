use fs_info::weather::*;
use serde_json::json;

fn current_json(temp: f64, code: i64) -> String {
    json!({
        "current": { "temperature_2m": temp, "weather_code": code },
        "daily": {
            "time": ["2026-07-28", "2026-07-29"],
            "temperature_2m_max": [24.1, 22.8],
            "temperature_2m_min": [14.3, 13.9],
            "weather_code": [code, 3]
        }
    })
    .to_string()
}

const EMPTY_DAILY: &str = r#"{"time":[],"temperature_2m_max":[],"temperature_2m_min":[],"weather_code":[]}"#;

// build_url

#[test]
fn build_url_includes_latitude_and_longitude() {
    let url = build_url(52.52, 13.41).unwrap();
    assert!(url.contains("latitude=52.52"));
    assert!(url.contains("longitude=13.41"));
}

#[test]
fn build_url_targets_open_meteo_forecast_endpoint() {
    assert!(build_url(0.0, 0.0).unwrap().starts_with("https://api.open-meteo.com/v1/forecast?"));
}

#[test]
fn build_url_requests_current_and_daily_variables() {
    let url = build_url(0.0, 0.0).unwrap();
    assert!(url.contains("current=temperature_2m,weather_code"));
    assert!(url.contains("daily=temperature_2m_max,temperature_2m_min,weather_code"));
}

#[test]
fn build_url_handles_negative_coordinates() {
    let url = build_url(-33.87, -70.65).unwrap();
    assert!(url.contains("latitude=-33.87"));
    assert!(url.contains("longitude=-70.65"));
}

#[test]
fn build_url_null_for_nan_longitude() {
    assert_eq!(build_url(52.52, f64::NAN), None);
}

#[test]
fn build_url_null_for_out_of_range_latitude() {
    assert_eq!(build_url(91.0, 0.0), None);
}

#[test]
fn build_url_null_for_out_of_range_longitude() {
    assert_eq!(build_url(0.0, 181.0), None);
}

// parse_response, failure shapes

#[test]
fn parse_response_network_error_on_zero_status() {
    assert_eq!(parse_response(0, "").unwrap_err().as_str(), "network_error");
}

#[test]
fn parse_response_http_error_on_non_200_status() {
    let r = parse_response(400, r#"{"error":true,"reason":"bad request"}"#);
    assert_eq!(r.unwrap_err().as_str(), "http_error");
}

#[test]
fn parse_response_malformed_json() {
    assert_eq!(parse_response(200, "not json{{{").unwrap_err().as_str(), "malformed_json");
}

#[test]
fn parse_response_missing_current_field() {
    let body = format!(r#"{{"daily":{EMPTY_DAILY}}}"#);
    assert_eq!(parse_response(200, &body).unwrap_err().as_str(), "missing_fields");
}

#[test]
fn parse_response_missing_daily_field() {
    let body = r#"{"current":{"temperature_2m":10,"weather_code":0}}"#;
    assert_eq!(parse_response(200, body).unwrap_err().as_str(), "missing_fields");
}

#[test]
fn parse_response_current_missing_temperature() {
    let body = format!(r#"{{"current":{{"weather_code":0}},"daily":{EMPTY_DAILY}}}"#);
    assert_eq!(parse_response(200, &body).unwrap_err().as_str(), "missing_fields");
}

#[test]
fn parse_response_daily_missing_array() {
    let body = r#"{"current":{"temperature_2m":10,"weather_code":0},"daily":{"time":[],"temperature_2m_max":[],"weather_code":[]}}"#;
    assert_eq!(parse_response(200, body).unwrap_err().as_str(), "missing_fields");
}

// parse_response, success shape

#[test]
fn parse_response_success_current_model() {
    let r = parse_response(200, &current_json(18.5, 2)).unwrap();
    assert_eq!(r.current.temperature, 18.5);
    assert_eq!(r.current.code, 2.0);
}

#[test]
fn parse_response_success_forecast_rows() {
    let r = parse_response(200, &current_json(18.5, 2)).unwrap();
    assert_eq!(r.forecast.len(), 2);
    assert_eq!(r.forecast[0].date, "2026-07-28");
    assert_eq!(r.forecast[0].high, 24.1);
    assert_eq!(r.forecast[0].low, 14.3);
    assert_eq!(r.forecast[0].code, 2.0);
    assert_eq!(r.forecast[1].code, 3.0);
}

#[test]
fn parse_response_skips_malformed_forecast_row() {
    let body = json!({
        "current": { "temperature_2m": 10, "weather_code": 0 },
        "daily": {
            "time": ["2026-07-28", "2026-07-29"],
            "temperature_2m_max": [20, "bad"],
            "temperature_2m_min": [10, 9],
            "weather_code": [0, 1]
        }
    })
    .to_string();
    let r = parse_response(200, &body).unwrap();
    assert_eq!(r.forecast.len(), 1);
    assert_eq!(r.forecast[0].date, "2026-07-28");
}

#[test]
fn parse_response_empty_forecast_arrays_still_ok() {
    let body = format!(r#"{{"current":{{"temperature_2m":10,"weather_code":0}},"daily":{EMPTY_DAILY}}}"#);
    assert_eq!(parse_response(200, &body).unwrap().forecast.len(), 0);
}

// condition_key / condition_label

#[test]
fn condition_label_matches_key() {
    assert_eq!(condition_label(0), "CLEAR");
    assert_eq!(condition_label(999), "UNAVAILABLE");
}

// weekday_label

#[test]
fn weekday_label_formats_three_letters_sentence_case() {
    let label = weekday_label("2026-07-28").unwrap();
    assert_eq!(label.len(), 3);
    let (head, tail) = label.split_at(1);
    assert_eq!(label, format!("{}{}", head.to_uppercase(), tail.to_lowercase()));
}

#[test]
fn weekday_label_matches_known_date() {
    // 2026-07-28 is a Tuesday (UTC).
    assert_eq!(weekday_label("2026-07-28"), Some("Tue"));
}

// condition_text

#[test]
fn condition_text_is_the_label_in_sentence_case() {
    assert_eq!(condition_text(0), "Clear");
    assert_eq!(condition_text(2), "Partly cloudy");
    assert_eq!(condition_text(999), "Unavailable");
}

// icon_for_code: names only, never codepoints: the active icon set
// (theme.icons) decides what a name draws.

#[test]
fn icon_for_code_clear_splits_day_from_night() {
    assert_eq!(icon_for_code(0, Some(true)), "sun");
    assert_eq!(icon_for_code(0, Some(false)), "moon");
}

#[test]
fn icon_for_code_partly_cloudy_splits_day_from_night() {
    assert_eq!(icon_for_code(2, Some(true)), "cloud-sun");
    assert_eq!(icon_for_code(2, Some(false)), "cloud-moon");
}

#[test]
fn icon_for_code_defaults_to_day_when_isday_omitted() {
    assert_eq!(icon_for_code(0, None), icon_for_code(0, Some(true)));
}

#[test]
fn icon_for_code_is_the_same_where_the_sky_is_not_the_subject() {
    // Rain and snow draw the same whatever time it is; only the two sky
    // conditions above carry a night variant.
    for code in [3, 45, 51, 56, 61, 71, 80, 95] {
        assert_eq!(icon_for_code(code, Some(true)), icon_for_code(code, Some(false)));
    }
}

#[test]
fn icon_for_code_unknown_code_returns_the_fallback() {
    assert_eq!(icon_for_code(999, Some(true)), FALLBACK_ICON);
    assert_eq!(icon_for_code(999, Some(false)), FALLBACK_ICON);
}

#[test]
fn icon_for_code_totality_every_documented_code_resolves() {
    // Every WMO code the condition table maps: none of these may fall back.
    let codes = [
        0, 1, 2, 3, 45, 48, 51, 53, 55, 56, 57, 66, 67, 61, 63, 65, 71, 73, 75, 77, 85, 86, 80, 81,
        82, 95, 96, 99,
    ];
    for code in codes {
        assert_ne!(icon_for_code(code, Some(true)), FALLBACK_ICON);
        assert_ne!(icon_for_code(code, Some(false)), FALLBACK_ICON);
    }
}
