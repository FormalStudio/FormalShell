//! The forecast behind the weather cell and the
//! place it is for: `location.latitude/longitude`
//! from settings.json when both are numbers, else GeoClue's fix, else (15 s
//! without one) a beaconDB lookup off the visible Wi-Fi access points. A
//! coarse fix that contradicts the system time zone gives way to the zone's
//! own point (`location::settle`). No fix is its own state, never a guess.

use std::time::Duration;

use fs_info::location::{self, Fix, Placed};
use fs_info::weather::{self, Error, Weather};

use super::geoclue::{self, GeoFix};
use super::{changed, idle, kicked, settings};
use crate::runtime::Ctx;
use crate::services::proc;
use crate::services::wants::Source;
use crate::store;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub located: bool,
    pub error: Option<Error>,
    pub weather: Option<Weather>,
    /// Nominatim's name for the spot (the panel's title), "" until it answers.
    pub place: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Cfg {
    at: Option<(f64, f64)>,
    interval: Duration,
}

fn read() -> Cfg {
    let s = settings();
    let at = s.f64("location.latitude").zip(s.f64("location.longitude"));
    Cfg { at, interval: s.interval("weather.intervalMs", 900_000) }
}

const SCAN: &str = "if busctl --system status net.connman.iwd >/dev/null 2>&1; then echo iwd; \
    busctl --system --json=short call net.connman.iwd / org.freedesktop.DBus.ObjectManager GetManagedObjects; \
    elif command -v nmcli >/dev/null 2>&1; then echo nm; nmcli -t -f BSSID,SSID,SIGNAL,FREQ dev wifi list; \
    else echo none; fi";

async fn geolocate() -> Option<Fix> {
    let scan = proc::capture(&proc::argv(&["sh", "-c", SCAN]), Duration::from_secs(15)).await;
    let body = location::geolocate_body(&location::access_points_from_scan(&scan.stdout));
    let reply = proc::capture(
        &proc::argv(&["curl", "-sS", "--fail", "--max-time", "8", "-H", "Content-Type: application/json", "-d", &body, location::GEOLOCATE_URL]),
        Duration::from_secs(15),
    )
    .await;
    if reply.code != 0 {
        return None;
    }
    location::parse_geolocate(&reply.stdout)
}

/// Read on every settle, so a zone changed while travelling is picked up.
fn settle(at: (f64, f64), coarse: bool) -> Placed {
    // TZ first, as the clocks read it; a path in it is no zone name.
    let system = std::env::var("TZ")
        .ok()
        .map(|tz| tz.trim_start_matches(':').to_string())
        .filter(|tz| !tz.is_empty() && !tz.starts_with('/'))
        .unwrap_or_else(|| iana_time_zone::get_timezone().unwrap_or_default());
    let zones = std::fs::read_to_string(location::ZONE_TAB).map(|t| location::parse_zone_tab(&t)).unwrap_or_default();
    location::settle(at, coarse, &system, &zones, chrono::Utc::now())
}

async fn place_name(key: &str, zoom: u8) -> String {
    let reply = proc::capture(
        &proc::argv(&[
            "curl", "-sS", "--fail", "--max-time", "8", "-H", "User-Agent: FormalShell (https://github.com/FormalStudio/FormalShell)",
            &location::reverse_url(key, zoom),
        ]),
        Duration::from_secs(15),
    )
    .await;
    if reply.code != 0 { String::new() } else { location::parse_place(&reply.stdout) }
}

/// open-meteo through curl: the status trails the body on its own line, and
/// a curl that never reached the server is status 0.
async fn fetch(url: &str) -> Result<Weather, Error> {
    let done = proc::capture(&proc::argv(&["curl", "-sS", "-m", "15", "-w", "\n%{http_code}", url]), Duration::from_secs(20)).await;
    if done.code != 0 {
        return weather::parse_response(0, "");
    }
    let (body, status) = done.stdout.rsplit_once('\n').unwrap_or(("", "0"));
    weather::parse_response(status.trim().parse().unwrap_or(0), body)
}

fn publish(ctx: &Ctx, state: State) {
    ctx.publish(store::Diff::Weather(state));
}

pub async fn run(ctx: Ctx) {
    let rx = changed();
    let kick = kicked(Source::Weather);
    let mut lookup: Option<Fix> = None;
    let mut geo: Option<GeoFix> = None;
    let (fixes, geo_fixes) = async_channel::unbounded();
    // No point asking GeoClue while settings.json already says where.
    let geoclue_wanted = read().at.is_none();
    if geoclue_wanted {
        ctx.spawn(geoclue::run(fixes));
    }
    let mut grace_spent = !geoclue_wanted;
    let mut places: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    loop {
        let cfg = read();
        let placed = match cfg.at {
            Some(at) => Some(Placed::Fix(at.0, at.1)),
            None => {
                while let Ok(fix) = geo_fixes.try_recv() {
                    geo = Some(fix);
                }
                if geo.is_none() && !grace_spent {
                    grace_spent = true;
                    let arrived = futures_lite::future::or(async { geo_fixes.recv().await.ok() }, async {
                        async_io::Timer::after(Duration::from_secs(15)).await;
                        None
                    })
                    .await;
                    geo = arrived;
                }
                if geo.is_none() && lookup.is_none() {
                    lookup = geolocate().await;
                }
                geo.map(|(at, accuracy)| (at, accuracy >= location::COARSE_METRES))
                    .or(lookup.map(|f| ((f.latitude, f.longitude), f.coarse())))
                    .map(|(at, coarse)| settle(at, coarse))
            }
        };
        let Some(placed) = placed else {
            publish(&ctx, State::default());
            // A machine that cannot place itself asks again in 15 s.
            futures_lite::future::or(
                futures_lite::future::or(idle(Duration::from_secs(15), &rx, &cfg, read), async {
                    let _ = kick.recv().await;
                }),
                next_fix(&geo_fixes, &mut geo),
            )
            .await;
            continue;
        };
        // A zone's point is no town, so it is named by zone.tab's words for
        // the area or, for a country with one zone, the country.
        let (lat, lon, key, zoom) = match &placed {
            Placed::Fix(lat, lon) => (*lat, *lon, location::place_key(*lat, *lon), 10),
            Placed::Zone(zone) => {
                let key = location::place_key(zone.latitude, zone.longitude);
                if !zone.comment.is_empty() {
                    places.insert(key.clone(), zone.comment.clone());
                }
                (zone.latitude, zone.longitude, key, 3)
            }
        };
        let place = places.get(&key).cloned().unwrap_or_default();
        let mut state = match weather::build_url(lat, lon) {
            None => State { located: true, error: Some(Error::MissingFields), weather: None, place },
            Some(url) => match fetch(&url).await {
                Ok(w) => State { located: true, error: None, weather: Some(w), place },
                Err(e) => State { located: true, error: Some(e), weather: None, place },
            },
        };
        publish(&ctx, state.clone());
        if !key.is_empty() && !places.contains_key(&key) {
            let name = place_name(&key, zoom).await;
            if !name.is_empty() {
                places.insert(key, name.clone());
                state.place = name;
                publish(&ctx, state);
            }
        }
        futures_lite::future::or(
            futures_lite::future::or(idle(cfg.interval, &rx, &cfg, read), async {
                let _ = kick.recv().await;
            }),
            next_fix(&geo_fixes, &mut geo),
        )
        .await;
    }
}

/// A later GeoClue fix replaces the one the forecast is for; once GeoClue
/// has gone quiet for good this never completes.
async fn next_fix(fixes: &async_channel::Receiver<GeoFix>, geo: &mut Option<GeoFix>) {
    match fixes.recv().await {
        Ok(fix) => *geo = Some(fix),
        Err(_) => std::future::pending().await,
    }
}
