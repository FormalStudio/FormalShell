//! Location's own lookups for a machine geoclue cannot place. geoclue's wifi
//! source only reads wpa_supplicant, so under iwd (NetworkManager's
//! wifi.backend = "iwd") it never has a BSSID to send and never gets a fix.
//! LocationService then sends beaconDB the same query itself, with the BSSIDs
//! iwd publishes on the system bus (or nmcli's, under wpa_supplicant), and
//! beaconDB answers from its IP database when it knows none of them.

use std::sync::LazyLock;

use chrono::{DateTime, Offset, TimeZone, Utc};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use fs_js as js;

pub const GEOLOCATE_URL: &str = "https://api.beacondb.net/v1/geolocate";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessPoint {
    pub mac_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_strength: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frequency: Option<i64>,
}

impl AccessPoint {
    fn bare(mac: String) -> AccessPoint {
        AccessPoint { mac_address: mac, signal_strength: None, frequency: None }
    }
}

fn is_nomap(name: &str) -> bool {
    name.ends_with("_nomap")
}

/// `busctl --json=short call net.connman.iwd / ...ObjectManager
/// GetManagedObjects`: every BasicServiceSet's Address, minus the ones under
/// a network whose name opts out of location services (`_nomap`).
pub fn access_points_from_iwd(text: &str) -> Vec<AccessPoint> {
    let Ok(parsed) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let Some(objects) = parsed.get("data").and_then(|d| d.get(0)).and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for (path, object) in objects {
        let Some(bss) = object.get("net.connman.iwd.BasicServiceSet") else {
            continue;
        };
        if !js::truthy(bss.get("Address")) {
            continue;
        }
        let parent = path.rfind('/').and_then(|i| objects.get(&path[..i]));
        let name = parent
            .and_then(|p| p.get("net.connman.iwd.Network"))
            .and_then(|n| n.get("Name"))
            .filter(|n| js::truthy(Some(n)))
            .map(|n| js::opt_str(n.get("data")))
            .unwrap_or_default();
        if is_nomap(&name) {
            continue;
        }
        let address = js::opt_str(bss.get("Address").and_then(|a| a.get("data")));
        result.push(AccessPoint::bare(address.to_lowercase()));
    }
    result
}

/// Terse mode escapes the colons inside a field with a backslash.
fn terse_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(next) => fields.last_mut().unwrap().push(next),
                None => fields.last_mut().unwrap().push('\\'),
            },
            ':' => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

/// `nmcli -t -f BSSID,SSID,SIGNAL,FREQ dev wifi list`.
pub fn access_points_from_nmcli(text: &str) -> Vec<AccessPoint> {
    static BSSID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9A-Fa-f:]{17}$").unwrap());
    let mut result = Vec::new();
    for line in text.split('\n') {
        let fields = terse_fields(line);
        if fields.len() < 4 || !BSSID.is_match(&fields[0]) || is_nomap(&fields[1]) {
            continue;
        }
        result.push(AccessPoint {
            mac_address: fields[0].to_lowercase(),
            signal_strength: Some(js::parse_int(&fields[2])).filter(|n| !n.is_nan()).map(|s| js::round(s / 2.0 - 100.0) as i64),
            frequency: Some(js::parse_int(&fields[3])).filter(|n| !n.is_nan()).map(|f| f as i64),
        });
    }
    result
}

pub fn geolocate_body(access_points: &[AccessPoint]) -> String {
    serde_json::json!({ "considerIp": true, "wifiAccessPoints": access_points }).to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Wifi,
    Ip,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Wifi => "wifi",
            Source::Ip => "ip",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fix {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: f64,
    pub source: Source,
}

/// beaconDB marks its IP answer `fallback: "ipf"`; anything without a usable
/// location is `None`.
pub fn parse_geolocate(body: &str) -> Option<Fix> {
    let data: Value = serde_json::from_str(body).ok()?;
    let loc = data.get("location").filter(|l| js::truthy(Some(l)))?;
    let latitude = js::to_number(loc.get("lat"));
    let longitude = js::to_number(loc.get("lng"));
    if !latitude.is_finite() || !longitude.is_finite() {
        return None;
    }
    let accuracy = js::to_number(data.get("accuracy"));
    Some(Fix {
        latitude,
        longitude,
        accuracy: if accuracy.is_nan() { 0.0 } else { accuracy },
        source: if data.get("fallback").and_then(Value::as_str) == Some("ipf") {
            Source::Ip
        } else {
            Source::Wifi
        },
    })
}

impl Fix {
    pub fn coarse(&self) -> bool {
        self.source == Source::Ip || self.accuracy >= COARSE_METRES
    }
}

/// Past this a fix places the network's exit, not the machine: beaconDB's
/// IP answer is 25 km, a Wi-Fi one a few hundred metres.
pub const COARSE_METRES: f64 = 10_000.0;

/// zone.tab rather than zone1970.tab: the latter folds a link like
/// Europe/Amsterdam into Europe/Brussels, and the system zone is the link.
pub const ZONE_TAB: &str = "/usr/share/zoneinfo/zone.tab";

#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    /// zone.tab's own words for the area ("Canary Islands"), "" for a
    /// country with one zone.
    pub comment: String,
}

/// ISO 6709 as zone.tab writes it: `+DDMM-DDDMM` or `+DDMMSS-DDDMMSS`.
fn iso6709(text: &str) -> Option<(f64, f64)> {
    let split = text.get(1..)?.find(['+', '-'])? + 1;
    let part = |p: &str, deg: usize| -> Option<f64> {
        let sign = match p.as_bytes().first()? {
            b'+' => 1.0,
            b'-' => -1.0,
            _ => return None,
        };
        let digits = &p[1..];
        if !digits.bytes().all(|b| b.is_ascii_digit()) || (digits.len() != deg + 2 && digits.len() != deg + 4) {
            return None;
        }
        let n = |r: std::ops::Range<usize>| digits.get(r).map_or(0.0, |d| d.parse::<f64>().unwrap_or(0.0));
        Some(sign * (n(0..deg) + n(deg..deg + 2) / 60.0 + n(deg + 2..deg + 4) / 3600.0))
    };
    Some((part(&text[..split], 2)?, part(&text[split..], 3)?))
}

pub fn parse_zone_tab(text: &str) -> Vec<Zone> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (_, coords, name) = (f.next()?, f.next()?, f.next()?);
            let (latitude, longitude) = iso6709(coords)?;
            Some(Zone { name: name.into(), latitude, longitude, comment: f.next().unwrap_or("").into() })
        })
        .collect()
}

fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let (dla, dlo) = (la2 - la1, (b.1 - a.1).to_radians());
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    12_742.0 * h.sqrt().asin()
}

fn offset_at(zone: &str, now: DateTime<Utc>) -> Option<i32> {
    let tz: chrono_tz::Tz = zone.parse().ok()?;
    Some(tz.offset_from_utc_datetime(&now.naive_utc()).fix().local_minus_utc())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Placed {
    Fix(f64, f64),
    /// The system zone's own point, standing in for a fix that contradicts it.
    Zone(Zone),
}

/// A coarse fix whose nearest zone keeps another clock than the system's
/// is the ISP's exit, not the machine: Canary Islands lines leave through
/// Madrid, an hour ahead. The system zone's point is then the honest place.
/// A fine fix, a zone missing from the table or one chrono-tz cannot read
/// leaves the fix alone.
pub fn settle(at: (f64, f64), coarse: bool, system: &str, zones: &[Zone], now: DateTime<Utc>) -> Placed {
    let keep = Placed::Fix(at.0, at.1);
    if !coarse {
        return keep;
    }
    let Some(home) = zones.iter().find(|z| z.name == system) else { return keep };
    let Some(nearest) = zones.iter().min_by(|a, b| {
        distance_km(at, (a.latitude, a.longitude)).total_cmp(&distance_km(at, (b.latitude, b.longitude)))
    }) else {
        return keep;
    };
    match (offset_at(system, now), offset_at(&nearest.name, now)) {
        (Some(ours), Some(theirs)) if ours != theirs => Placed::Zone(home.clone()),
        _ => keep,
    }
}

/// Rounded to about a kilometre, so a fix that drifts a few metres is the
/// same place and never another lookup.
pub fn place_key(latitude: f64, longitude: f64) -> String {
    if !latitude.is_finite() || !longitude.is_finite() {
        return String::new();
    }
    format!("{},{}", js::to_fixed(latitude, 2), js::to_fixed(longitude, 2))
}

pub fn reverse_url(key: &str, zoom: u8) -> String {
    let mut parts = key.split(',');
    let lat = parts.next().unwrap_or("");
    // A key without a comma reads its longitude as "undefined", the way the
    // string concatenation always has.
    let lon = parts.next().unwrap_or("undefined");
    format!(
        "https://nominatim.openstreetmap.org/reverse?format=jsonv2&zoom={zoom}&accept-language=en&lat={lat}&lon={lon}"
    )
}

/// "Teguise, Canary Islands": the town and its region, "" when Nominatim has
/// neither.
pub fn parse_place(body: &str) -> String {
    let Ok(data) = serde_json::from_str::<Value>(body) else {
        return String::new();
    };
    let Some(data) = data.as_object() else {
        return String::new();
    };
    let address = data.get("address").filter(|a| js::truthy(Some(a)));
    let first_of = |keys: &[&str], fallback: Option<&Value>| -> String {
        let hit = keys
            .iter()
            .filter_map(|k| address.and_then(|a| a.get(*k)))
            .chain(fallback)
            .find(|v| js::truthy(Some(v)));
        js::opt_str(hit)
    };
    let town = first_of(&["city", "town", "village", "municipality", "county"], data.get("name"));
    let region = first_of(&["state", "country"], None);
    if !town.is_empty() && !region.is_empty() && town != region {
        format!("{town}, {region}")
    } else if !town.is_empty() {
        town
    } else {
        region
    }
}
