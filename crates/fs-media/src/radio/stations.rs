// Portions from omarchy-radio-atlas (MIT, Copyright 2026 Akshar Patel)

//! Radio Browser's wire format in, station records out, plus the saved state
//! schema and cliamp's channel list. The rules are radio-fetch's jq filter
//! and radio-state's validator from the original, transcribed field for
//! field.
//!
//! Randomness is injected as `rng: FnMut() -> f64` (a value in `[0, 1)`, the
//! shape of `Math.random`) so the shuffles stay pure.
//!
//! Station fields JS left `null` or absent read as `""`/`0` here (a cliamp
//! channel has no country or bitrate); every consumer treats the two alike.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use fs_js::{parse_number, slice_utf16, utf16_len};

pub const API_FALLBACK: &str = "https://all.api.radio-browser.info";
pub const SERVERS_URL: &str = "https://all.api.radio-browser.info/json/servers";
pub const MAX_RESPONSE_CHARS: usize = 4_194_304;
pub const MAX_RECORDS: usize = 500;
pub const MAX_FAVORITES: usize = 500;
pub const MAX_RECENT: usize = 30;
pub(crate) const CACHE_MAX_AGE_MS: f64 = 24.0 * 60.0 * 60.0 * 1000.0;

static UUID_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9A-Fa-f-]{20,64}$").unwrap());
static OUTPUT_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9._:+-]{0,160}$").unwrap());
static MIRROR_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9.-]+\.api\.radio-browser\.info$").unwrap());
static STREAM_PATTERN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^https?://").unwrap());
static MAPPABLE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]{2}$").unwrap());
static CONTROL_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\x00-\x1f\x7f]").unwrap());
static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r" {2,}").unwrap());

// cliamp's own channels (github.com/bjarneo/cliamp, MIT). The M3U is the list
// cliamp itself reads, so a channel it adds shows up here with no change.
// Omarchy's channel is left out at the owner's request.
pub const CLIAMP_URL: &str = "https://radio.cliamp.stream/streams.m3u";
static CLIAMP_STREAM_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^https://radio\.cliamp\.stream/([a-z0-9-]{1,64})/stream$").unwrap()
});
const CLIAMP_EXCLUDED: [&str; 1] = ["omarchy"];

pub fn is_uuid(s: &str) -> bool {
    UUID_PATTERN.is_match(s)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Station {
    pub uuid: String,
    pub name: String,
    pub url: String,
    pub homepage: String,
    pub favicon: String,
    pub country: String,
    pub country_code: String,
    pub state: String,
    pub language: String,
    pub tags: String,
    pub codec: String,
    pub bitrate: f64,
    pub votes: f64,
    pub clicks: f64,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// Set by `mergeGeoStations` on a copy placed at a guessed point; never
    /// saved.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub estimated_location: bool,
}

fn json_str(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn json_num(v: &Value, key: &str) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn json_coord(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

impl Station {
    /// A station out of a saved favourite, recent entry or cache document.
    /// Fields of the wrong type read as blank.
    pub fn from_saved(v: &Value) -> Station {
        Station {
            uuid: json_str(v, "uuid"),
            name: json_str(v, "name"),
            url: json_str(v, "url"),
            homepage: json_str(v, "homepage"),
            favicon: json_str(v, "favicon"),
            country: json_str(v, "country"),
            country_code: json_str(v, "countryCode"),
            state: json_str(v, "state"),
            language: json_str(v, "language"),
            tags: json_str(v, "tags"),
            codec: json_str(v, "codec"),
            bitrate: json_num(v, "bitrate"),
            votes: json_num(v, "votes"),
            clicks: json_num(v, "clicks"),
            latitude: json_coord(v, "latitude"),
            longitude: json_coord(v, "longitude"),
            estimated_location: false,
        }
    }
}

fn clean(value: Option<&Value>, limit: usize) -> String {
    let Some(Value::String(s)) = value else {
        return String::new();
    };
    clean_str(s, limit)
}

fn clean_str(s: &str, limit: usize) -> String {
    let spaced = CONTROL_CHARS.replace_all(s, " ");
    let collapsed = MULTI_SPACE.replace_all(&spaced, " ");
    slice_utf16(&collapsed, limit).to_string()
}

fn compact(value: Option<&Value>, limit: usize) -> String {
    let Some(Value::String(s)) = value else {
        return String::new();
    };
    slice_utf16(&CONTROL_CHARS.replace_all(s, ""), limit).to_string()
}

/// jq's `a // b` keeps "" and 0; only null, absent and false fall through.
fn or<'a>(value: Option<&'a Value>, fallback: Option<&'a Value>) -> Option<&'a Value> {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => fallback,
        v => v,
    }
}

/// jq's `tonumber` with a fallback: a number stays, a numeric string parses,
/// anything else (null, "", an object) is the fallback.
fn number(value: Option<&Value>) -> Option<f64> {
    let n = match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) if !s.trim().is_empty() => parse_number(s),
        _ => f64::NAN,
    };
    n.is_finite().then_some(n)
}

pub fn playable(station: &Station) -> bool {
    STREAM_PATTERN.is_match(&station.url) && !station.url.contains(['\r', '\n'])
}

pub fn record(row: &Value) -> Station {
    let unknown = Value::String("Unknown station".into());
    let blank = Value::String(String::new());
    let zero = Value::from(0);
    let name = clean(or(row.get("name"), Some(&unknown)), 160);
    let field = |key: &str| or(row.get(key), Some(&blank));
    Station {
        uuid: compact(field("stationuuid"), 64),
        name: if name.is_empty() {
            "Unknown station".into()
        } else {
            name
        },
        url: compact(
            or(or(row.get("url_resolved"), row.get("url")), Some(&blank)),
            2048,
        ),
        homepage: compact(field("homepage"), 2048),
        favicon: compact(field("favicon"), 2048),
        country: clean(field("country"), 100),
        country_code: clean(field("countrycode"), 2).to_uppercase(),
        state: clean(field("state"), 100),
        language: clean(field("language"), 120),
        tags: clean(field("tags"), 500),
        codec: clean(field("codec"), 32),
        bitrate: number(or(row.get("bitrate"), Some(&zero))).unwrap_or(0.0),
        votes: number(or(row.get("votes"), Some(&zero))).unwrap_or(0.0),
        clicks: number(or(row.get("clickcount"), Some(&zero))).unwrap_or(0.0),
        latitude: number(row.get("geo_lat")),
        longitude: number(row.get("geo_long")),
        estimated_location: false,
    }
}

fn keep(station: &Station) -> bool {
    !station.uuid.is_empty()
        && STREAM_PATTERN.is_match(&station.url)
        && station.latitude.is_none_or(|v| (-90.0..=90.0).contains(&v))
        && station
            .longitude
            .is_none_or(|v| (-180.0..=180.0).contains(&v))
}

/// A raw response body to at most `max` sanitised, deduplicated records, or
/// None when the body is not an array of at most `max` entries.
pub fn parse_response(text: &str, max: usize) -> Option<Vec<Station>> {
    if utf16_len(text) > MAX_RESPONSE_CHARS {
        return None;
    }
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    if rows.len() > max {
        return None;
    }
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for row in rows.iter().take(MAX_RECORDS) {
        if out.len() >= MAX_RECORDS {
            break;
        }
        if !row.is_object() {
            continue;
        }
        let station = record(row);
        if !keep(&station) || seen.contains(&station.uuid) {
            continue;
        }
        seen.insert(station.uuid.clone());
        out.push(station);
    }
    Some(out)
}

pub fn union(lists: &[Vec<Station>], max: usize) -> Vec<Station> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for rows in lists {
        for row in rows {
            if out.len() >= max {
                break;
            }
            if seen.contains(&row.uuid) {
                continue;
            }
            seen.insert(row.uuid.clone());
            out.push(row.clone());
        }
    }
    out
}

/// Fisher-Yates, from the end.
pub(crate) fn shuffle<T: Clone>(list: &[T], rng: &mut impl FnMut() -> f64) -> Vec<T> {
    let mut out = list.to_vec();
    for i in (1..out.len()).rev() {
        let j = (rng() * (i + 1) as f64).floor() as usize;
        out.swap(i, j.min(i));
    }
    out
}

/// /json/servers lists each mirror once per address family.
pub fn parse_servers(text: &str, rng: &mut impl FnMut() -> f64) -> Vec<String> {
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for row in &rows {
        let name = row.get("name").and_then(Value::as_str).unwrap_or("");
        let url = format!("https://{name}");
        if !MIRROR_PATTERN.is_match(name) || out.contains(&url) {
            continue;
        }
        out.push(url);
    }
    shuffle(&out, rng)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub max: usize,
    pub path: String,
    pub params: Vec<(String, String)>,
}

fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

const SEARCH_ORDER: [(&str, &str); 3] = [
    ("hidebroken", "true"),
    ("order", "clickcount"),
    ("reverse", "true"),
];

pub fn world_request() -> Request {
    let mut p = params(&[("has_geo_info", "true")]);
    p.extend(params(&SEARCH_ORDER));
    p.extend(params(&[("limit", "500")]));
    Request {
        max: 500,
        path: "/json/stations/search".into(),
        params: p,
    }
}

pub fn world_more_request() -> Request {
    Request {
        max: 500,
        path: "/json/stations/search".into(),
        params: params(&[
            ("hidebroken", "true"),
            ("order", "random"),
            ("limit", "500"),
        ]),
    }
}

pub fn country_request(code: &str) -> Request {
    let mut p = params(&SEARCH_ORDER);
    p.extend(params(&[("limit", "25")]));
    Request {
        max: 25,
        path: format!("/json/stations/bycountrycodeexact/{code}"),
        params: p,
    }
}

pub fn search_requests(query: &str) -> [Request; 3] {
    ["name", "country", "tag"].map(|field| {
        let mut p = params(&[(field, query)]);
        p.extend(params(&SEARCH_ORDER));
        p.extend(params(&[("limit", "80")]));
        Request {
            max: 80,
            path: "/json/stations/search".into(),
            params: p,
        }
    })
}

pub fn random_request() -> Request {
    Request {
        max: 60,
        path: "/json/stations/search".into(),
        params: params(&[("hidebroken", "true"), ("order", "random"), ("limit", "60")]),
    }
}

pub fn resolve_request(uuids: &[String]) -> Request {
    Request {
        max: uuids.len(),
        path: "/json/stations/byuuid".into(),
        params: vec![("uuids".into(), uuids.join(","))],
    }
}

/// curl's argv for one request against one mirror, the flags radio-fetch
/// passes. `--max-filesize` aborts a transfer past the cap even when the
/// server sends no length.
pub fn curl_args(base: &str, request: &Request, user_agent: &str) -> Vec<String> {
    let mut args: Vec<String> = [
        "curl",
        "--fail",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "4",
        "--max-time",
        if request.max >= 500 { "45" } else { "12" },
        "--retry",
        "1",
        "--retry-delay",
        "0",
        "--max-filesize",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(MAX_RESPONSE_CHARS.to_string());
    args.extend(["--proto", "=https", "--user-agent"].map(String::from));
    args.push(user_agent.to_string());
    args.push("--get".into());
    args.push(format!("{base}{}", request.path));
    for (k, v) in &request.params {
        args.push("--data-urlencode".into());
        args.push(format!("{k}={v}"));
    }
    args
}

pub(crate) fn mappable(station: &Station) -> bool {
    (station.latitude.is_some() && station.longitude.is_some())
        || MAPPABLE_CODE.is_match(&station.country_code)
}

/// radio-fetch's random pick: mappable stations only, the excluded ones
/// dropped unless that would leave nothing, then shuffled.
pub fn pick_random(
    rows: &[Station],
    excluded: &[String],
    rng: &mut impl FnMut() -> f64,
) -> Vec<Station> {
    let usable: Vec<Station> = rows.iter().filter(|s| mappable(s)).cloned().collect();
    let fresh: Vec<Station> = usable
        .iter()
        .filter(|s| !excluded.contains(&s.uuid))
        .cloned()
        .collect();
    shuffle(if fresh.is_empty() { &usable } else { &fresh }, rng)
}

fn fetched_at(doc: &Value) -> Option<f64> {
    doc.get("fetchedAt").and_then(Value::as_f64)
}

pub fn valid_world_cache(doc: &Value) -> bool {
    match (fetched_at(doc), doc.get("stations")) {
        (Some(_), Some(Value::Array(s))) => !s.is_empty() && s.len() <= MAX_RECORDS,
        _ => false,
    }
}

pub fn valid_country_cache(doc: &Value, code: &str) -> bool {
    match (fetched_at(doc), doc.get("stations")) {
        (Some(_), Some(Value::Array(s))) => {
            s.len() <= 25
                && s.iter()
                    .all(|st| st.get("countryCode").and_then(Value::as_str) == Some(code))
        }
        _ => false,
    }
}

pub fn stale(fetched_at: f64, now: f64) -> bool {
    now - fetched_at > CACHE_MAX_AGE_MS
}

/// What a favourite or a recent entry keeps: the record's own fields, never
/// the estimatedLocation copy a map merge made.
pub fn saved_record(station: &Station) -> Station {
    Station {
        estimated_location: false,
        ..station.clone()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub favorites: Vec<Station>,
    pub recent: Vec<Station>,
    pub volume: i64,
    pub output: String,
}

/// radio-state's validator: the file is taken whole or not at all, so a state
/// that fails here is left on disk untouched.
pub fn parse_state(text: &str) -> Option<State> {
    let Ok(doc @ Value::Object(_)) = serde_json::from_str::<Value>(text) else {
        return None;
    };
    let volume = match doc.get("volume") {
        None | Some(Value::Null) => 70.0,
        Some(Value::Number(n)) => n.as_f64()?,
        Some(_) => return None,
    };
    let output = match doc.get("output") {
        None | Some(Value::Null) => "",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return None,
    };
    let (Some(Value::Array(favorites)), Some(Value::Array(recent))) =
        (doc.get("favorites"), doc.get("recent"))
    else {
        return None;
    };
    if favorites.len() > MAX_FAVORITES
        || recent.len() > MAX_RECENT
        || !(0.0..=100.0).contains(&volume)
        || !OUTPUT_PATTERN.is_match(output)
    {
        return None;
    }
    let saved = |s: &&Value| matches!(s.get("uuid"), Some(Value::String(u)) if !u.is_empty());
    Some(State {
        favorites: favorites
            .iter()
            .filter(saved)
            .map(Station::from_saved)
            .collect(),
        recent: recent
            .iter()
            .filter(saved)
            .map(Station::from_saved)
            .collect(),
        volume: volume.round() as i64,
        output: output.to_string(),
    })
}

/// The M3U to station records, ids `cliamp:<slug>` so a favourite or a recent
/// entry survives a reorder of the list. Anything that is not one of cliamp's
/// own streams is dropped.
pub fn parse_cliamp(text: &str) -> Option<Vec<Station>> {
    if utf16_len(text) > 65536 {
        return None;
    }
    let mut lines = text.split('\n');
    if lines.next()?.trim() != "#EXTM3U" {
        return None;
    }
    let mut out: Vec<Station> = Vec::new();
    let mut name = String::new();
    for raw in lines {
        let line = raw.trim();
        if line.starts_with("#EXTINF:") {
            name = match line.find(',') {
                Some(comma) => clean_str(line[comma + 1..].trim(), 160),
                None => String::new(),
            };
            continue;
        }
        let Some(m) = CLIAMP_STREAM_PATTERN.captures(line) else {
            if !line.is_empty() && !line.starts_with('#') {
                name.clear();
            }
            continue;
        };
        let slug = &m[1];
        let uuid = format!("cliamp:{slug}");
        if !CLIAMP_EXCLUDED.contains(&slug) && !out.iter().any(|s| s.uuid == uuid) {
            out.push(Station {
                name: if name.is_empty() {
                    slug.to_string()
                } else {
                    name.clone()
                },
                uuid,
                url: line.to_string(),
                homepage: "https://cliamp.stream".into(),
                tags: "cliamp radio".into(),
                ..Station::default()
            });
        }
        name.clear();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const M3U: &str = "#EXTM3U\n\
        #EXTINF:-1,Lofi\nhttps://radio.cliamp.stream/lofi/stream\n\
        #EXTINF:-1,Omarchy\nhttps://radio.cliamp.stream/omarchy/stream\n\
        #EXTINF:-1,Elsewhere\nhttps://example.com/x/stream\n\
        #EXTINF:-1,NCS Drum & Bass\r\nhttps://radio.cliamp.stream/ncs-dnb/stream\r\n\
        https://radio.cliamp.stream/lofi/stream\n";

    #[test]
    fn channels_parse_without_omarchy() {
        let rows = parse_cliamp(M3U).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].uuid, "cliamp:lofi");
        assert_eq!(rows[0].name, "Lofi");
        assert_eq!(rows[1].uuid, "cliamp:ncs-dnb");
        assert_eq!(rows[1].name, "NCS Drum & Bass");
        assert!(playable(&rows[1]));
        assert_eq!(rows[1].latitude, None);
    }

    #[test]
    fn not_an_m3u() {
        assert_eq!(parse_cliamp("<html>"), None);
    }

    #[test]
    fn a_channel_survives_a_save() {
        let channels = parse_cliamp(M3U).unwrap();
        let doc = serde_json::json!({ "favorites": channels, "recent": [] });
        let state = parse_state(&doc.to_string()).unwrap();
        assert_eq!(state.favorites.len(), 2);
        assert_eq!(state.favorites[0].uuid, "cliamp:lofi");
    }

    #[test]
    fn record_sanitises_and_filters() {
        let row = serde_json::json!({
            "stationuuid": "960e57c5-0601-11e8-ae97-52543be04c81",
            "name": "  Jazz\u{1}  FM ",
            "url": "http://a.example/s",
            "url_resolved": "https://a.example/resolved",
            "countrycode": "es",
            "bitrate": "128",
            "geo_lat": 40.4,
            "geo_long": "-3.7",
        });
        let s = record(&row);
        assert_eq!(s.name, " Jazz FM ");
        assert_eq!(s.url, "https://a.example/resolved");
        assert_eq!(s.country_code, "ES");
        assert_eq!(s.bitrate, 128.0);
        assert_eq!(s.longitude, Some(-3.7));
        let body =
            serde_json::json!([row, row, { "stationuuid": "x", "url": "ftp://x" }]).to_string();
        assert_eq!(parse_response(&body, 500).unwrap().len(), 1);
        assert_eq!(parse_response(&body, 2), None);
    }
}
