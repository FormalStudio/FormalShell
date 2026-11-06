use fs_info::location::*;
use chrono::{TimeZone, Utc};
use serde_json::json;

fn ap(mac: &str) -> AccessPoint {
    AccessPoint { mac_address: mac.into(), signal_strength: None, frequency: None }
}

#[test]
fn iwd_objects_give_every_bss_but_nomap() {
    let text = json!({ "type": "a{oa{sa{sv}}}", "data": [{
        "/net/connman/iwd/0/3/aa_psk": { "net.connman.iwd.Network": { "Name": { "type": "s", "data": "Home" } } },
        "/net/connman/iwd/0/3/aa_psk/2c9682fd9f57": { "net.connman.iwd.BasicServiceSet": { "Address": { "type": "s", "data": "2C:96:82:FD:9F:57" } } },
        "/net/connman/iwd/0/3/bb_psk": { "net.connman.iwd.Network": { "Name": { "type": "s", "data": "Cafe_nomap" } } },
        "/net/connman/iwd/0/3/bb_psk/111111111111": { "net.connman.iwd.BasicServiceSet": { "Address": { "type": "s", "data": "11:11:11:11:11:11" } } }
    }] })
    .to_string();
    assert_eq!(access_points_from_iwd(&text), vec![ap("2c:96:82:fd:9f:57")]);
    assert_eq!(access_points_from_iwd("nope"), vec![]);
}

#[test]
fn nmcli_unescapes_and_converts_signal() {
    let text = "2C\\:96\\:82\\:FD\\:9F\\:57:Home:70:5260 MHz\n11\\:11\\:11\\:11\\:11\\:11:Cafe_nomap:50:2412 MHz\n";
    assert_eq!(
        access_points_from_nmcli(text),
        vec![AccessPoint {
            mac_address: "2c:96:82:fd:9f:57".into(),
            signal_strength: Some(-65),
            frequency: Some(5260),
        }]
    );
}

#[test]
fn scan_head_picks_the_parser() {
    assert_eq!(
        access_points_from_scan("nm\n2C\\:96\\:82\\:FD\\:9F\\:57:Home:70:5260 MHz").len(),
        1
    );
    assert_eq!(access_points_from_scan("none\n"), vec![]);
}

#[test]
fn parse_geolocate_tells_wifi_from_ip() {
    assert_eq!(
        parse_geolocate(r#"{"location":{"lat":29.1,"lng":-13.5},"accuracy":30}"#),
        Some(Fix { latitude: 29.1, longitude: -13.5, accuracy: 30.0, source: Source::Wifi })
    );
    assert_eq!(
        parse_geolocate(r#"{"location":{"lat":28.9,"lng":-13.5},"accuracy":25000,"fallback":"ipf"}"#)
            .unwrap()
            .source
            .as_str(),
        "ip"
    );
    assert_eq!(parse_geolocate(r#"{"error":{}}"#), None);
}

#[test]
fn place_key_rounds_to_a_kilometre() {
    assert_eq!(place_key(29.118083, -13.566583), "29.12,-13.57");
    assert_eq!(place_key(f64::NAN, 1.0), "");
    assert_eq!(
        reverse_url("29.12,-13.57", 10),
        "https://nominatim.openstreetmap.org/reverse?format=jsonv2&zoom=10&accept-language=en&lat=29.12&lon=-13.57"
    );
}

#[test]
fn parse_place_joins_town_and_region() {
    assert_eq!(
        parse_place(r#"{"name":"Teguise","address":{"city":"Teguise","state":"Canary Islands"}}"#),
        "Teguise, Canary Islands"
    );
    assert_eq!(parse_place(r#"{"address":{"village":"Nazaret"}}"#), "Nazaret");
    assert_eq!(parse_place("nope"), "");
}

const ZONE_TAB: &str = "# comment\n\
ES\t+4024-00341\tEurope/Madrid\tSpain (mainland)\n\
ES\t+2806-01524\tAtlantic/Canary\tCanary Islands\n\
PT\t+3843-00908\tEurope/Lisbon\tPortugal (mainland)\n\
US\t+404251-0740023\tAmerica/New_York\tEastern (most areas)\n\
CU\t+2308-08222\tAmerica/Havana\n";

#[test]
fn zone_tab_reads_both_coordinate_widths() {
    let zones = parse_zone_tab(ZONE_TAB);
    assert_eq!(zones.len(), 5);
    let canary = &zones[1];
    assert_eq!((canary.name.as_str(), canary.comment.as_str()), ("Atlantic/Canary", "Canary Islands"));
    assert!((canary.latitude - 28.1).abs() < 1e-9 && (canary.longitude + 15.4).abs() < 1e-9);
    let ny = &zones[3];
    assert!((ny.latitude - 40.714167).abs() < 1e-5 && (ny.longitude + 74.006389).abs() < 1e-5);
    assert_eq!(zones[4].comment, "");
}

#[test]
fn settle_drops_an_ip_fix_from_another_clock() {
    let zones = parse_zone_tab(ZONE_TAB);
    let now = Utc.with_ymd_and_hms(2026, 11, 6, 14, 0, 0).unwrap();
    let madrid = (40.4168, -3.70379);
    // The report: a Lanzarote line geolocated to Madrid, an hour ahead.
    assert_eq!(settle(madrid, true, "Atlantic/Canary", &zones, now), Placed::Zone(zones[1].clone()));
    // Lisbon keeps the Canaries' clock, so a fix there could be honest.
    assert_eq!(settle((38.72, -9.14), true, "Atlantic/Canary", &zones, now), Placed::Fix(38.72, -9.14));
    // Miami's nearest zone is Havana, on New York's clock.
    assert_eq!(settle((25.76, -80.19), true, "America/New_York", &zones, now), Placed::Fix(25.76, -80.19));
    assert_eq!(settle(madrid, true, "Europe/Madrid", &zones, now), Placed::Fix(madrid.0, madrid.1));
    assert_eq!(settle(madrid, false, "Atlantic/Canary", &zones, now), Placed::Fix(madrid.0, madrid.1));
    assert_eq!(settle(madrid, true, "Etc/Unknown", &zones, now), Placed::Fix(madrid.0, madrid.1));
}

#[test]
fn coarse_is_the_ip_answer_or_a_wide_one() {
    let fix = |accuracy, source| Fix { latitude: 0.0, longitude: 0.0, accuracy, source };
    assert!(fix(25_000.0, Source::Ip).coarse());
    assert!(fix(50.0, Source::Ip).coarse());
    assert!(fix(25_000.0, Source::Wifi).coarse());
    assert!(!fix(30.0, Source::Wifi).coarse());
}
