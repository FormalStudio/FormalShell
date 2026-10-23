//! Samsung Galaxy Buds adapter for the earbuds model, over the `earbuds` CLI
//! (github.com/JojiiOfficial/LiveBudsCli, nixpkgs `earbuds`). Ported from
//! `shell/Earbuds/samsung.js`.
//!
//! The CLI is a client of its own background daemon and starts it on any
//! command, and that daemon acts on its own (auto-pause over MPRIS, PulseAudio
//! sink switching), so the backend runs it only for a device BlueZ reports
//! connected and whose name is a Galaxy Buds one (the daemon itself picks
//! devices by name and the SPP uuid,
//! src/daemon/bluetooth/bt_connection_listener.rs).

use serde_json::Value as Json;

use super::{Battery, BatteryId, ControlOption, Device, DeviceFields, DeviceKind, Value, choice, device, toggle, valid_address};
use crate::bluetooth;
use fs_js as js;

pub const BACKEND: &str = "samsung";
pub const BINARY: &str = "earbuds";

fn is_galaxy_buds(name: &str) -> bool {
    name.get(.."galaxy buds".len()).is_some_and(|p| p.eq_ignore_ascii_case("galaxy buds"))
}

/// A connected device whose name says Galaxy Buds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buds {
    pub address: String,
    pub name: String,
}

/// `device_name` is the name the buds report, `name` the alias the user may
/// have changed.
pub fn connected_buds(bluetooth: &[bluetooth::Device]) -> Vec<Buds> {
    bluetooth
        .iter()
        .filter(|bt| bt.connected && valid_address(&bt.address))
        .filter(|bt| is_galaxy_buds(&bt.device_name) || is_galaxy_buds(&bt.name))
        .map(|bt| Buds {
            address: bt.address.to_uppercase(),
            name: if !bt.name.is_empty() { bt.name.clone() } else { bt.device_name.clone() },
        })
        .collect()
}

/// `-q` keeps "Daemon started successfully" off stdout, `-s` picks the device.
pub fn argv(address: &str, tail: &[String]) -> Option<Vec<String>> {
    valid_address(address).then(|| {
        let mut out: Vec<String> = [BINARY, "-q", "-s", address].map(String::from).to_vec();
        out.extend(tail.iter().cloned());
        out
    })
}

pub fn status_argv(address: &str) -> Option<Vec<String>> {
    argv(address, &["status".into(), "-o".into(), "json".into()])
}

/// Per model, from galaxy_buds_rs' `Model::get_features` (0.2.11) and the
/// daemon's `get_max_ambientsound_volume_level`: whether it has ANC, and the
/// top ambient level (0 for no ambient control). A model outside this table
/// gets batteries and the equalizer only.
struct Model {
    name: &'static str,
    anc: bool,
    ambient: i32,
}

fn model(id: &str) -> Model {
    let (name, anc, ambient) = match id {
        "Buds" => ("Galaxy Buds", false, 3),
        "BudsPlus" => ("Galaxy Buds+", false, 4),
        "BudsLive" => ("Galaxy Buds Live", true, 0),
        "BudsPro" => ("Galaxy Buds Pro", true, 0),
        "BudsPro2" => ("Galaxy Buds Pro 2", true, 0),
        "Buds2" => ("Galaxy Buds 2", true, 3),
        "Buds3Pro" => ("Galaxy Buds3 Pro", true, 0),
        _ => ("Galaxy Buds", false, 0),
    };
    Model { name, anc, ambient }
}

/// EqualizerType's wire numbers (galaxy_buds_rs bud_property.rs) and the word
/// `earbuds set equalizer` takes for each.
const EQ: [(&str, &str); 6] =
    [("normal", "Normal"), ("bass", "Bass"), ("soft", "Soft"), ("dynamic", "Dynamic"), ("clear", "Clear"), ("treble", "Treble")];

/// Placement's wire numbers: 1 in ear, 2 outside, 3 in the open case, 4 in the
/// closed case.
const IN_EAR: f64 = 1.0;

fn in_case(placement: f64) -> bool {
    placement == 3.0 || placement == 4.0
}

/// A parsed `status -o json` answer. Everything but `ok` and `message` is
/// meaningful only when `ok`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub ok: bool,
    pub message: String,
    pub address: String,
    pub ready: bool,
    pub model: String,
    pub batt_left: f64,
    pub batt_right: f64,
    pub batt_case: f64,
    pub placement_left: f64,
    pub placement_right: f64,
    pub equalizer: f64,
    pub anc: bool,
    pub ambient_enabled: bool,
    pub ambient_volume: f64,
    pub extra_high: bool,
}

fn num(payload: &Json, key: &str, fallback: f64) -> f64 {
    match js::field(payload, key) {
        Some(v) if v.is_number() => v.as_f64().filter(|n| n.is_finite()).unwrap_or(fallback),
        _ => fallback,
    }
}

fn flag(payload: &Json, key: &str) -> bool {
    js::field(payload, key) == Some(&Json::Bool(true))
}

/// The byte offset of the first line that starts with "{": `search(/^\{/m)`.
fn first_brace_line(text: &str) -> Option<usize> {
    let mut at_start = true;
    for (i, c) in text.char_indices() {
        if at_start && c == '{' {
            return Some(i);
        }
        at_start = matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    }
    None
}

/// `status -o json` prints the daemon's response as one JSON line: `{ status,
/// device, status_message, payload }`. A "No connected device found" error, or
/// anything unparseable, is `ok: false`. Text before the first line starting
/// with "{" is dropped.
pub fn parse_status(text: &str) -> Status {
    let mut bad = Status { ok: false, ..Default::default() };
    let Some(start) = first_brace_line(text) else { return bad };
    let line = text[start..].split('\n').next().unwrap_or("");
    let Ok(raw) = serde_json::from_str::<Json>(line) else { return bad };
    let payload = js::field(&raw, "payload").filter(|p| js::is_object_like(p));
    let success = js::field(&raw, "status").and_then(Json::as_str) == Some("success");
    let Some(p) = payload.filter(|_| success) else {
        bad.message = js::field(&raw, "status_message").and_then(Json::as_str).unwrap_or("").to_string();
        return bad;
    };
    Status {
        ok: true,
        message: String::new(),
        address: js::field(p, "address").and_then(Json::as_str).unwrap_or("").to_string(),
        ready: flag(p, "ready"),
        model: js::field(p, "model").and_then(Json::as_str).unwrap_or("").to_string(),
        batt_left: num(p, "batt_left", -1.0),
        batt_right: num(p, "batt_right", -1.0),
        batt_case: num(p, "batt_case", -1.0),
        placement_left: num(p, "placement_left", 0.0),
        placement_right: num(p, "placement_right", 0.0),
        equalizer: num(p, "equalizer_type", -1.0),
        anc: flag(p, "noise_reduction"),
        ambient_enabled: flag(p, "ambient_sound_enabled"),
        ambient_volume: num(p, "ambient_sound_volume", 0.0),
        extra_high: flag(p, "extra_high_ambient_volume"),
    }
}

fn bud(id: BatteryId, label: &str, level: f64, placement: f64) -> Battery {
    // 3 and 4 are the case placements, where the buds charge.
    Battery::new(id, label, level, in_case(placement), Some(placement == IN_EAR))
}

fn batteries(s: &Status) -> Vec<Battery> {
    let known = |level: f64| (0.0..=100.0).contains(&level);
    let mut out = Vec::new();
    if known(s.batt_left) {
        out.push(bud(BatteryId::Left, "Left", s.batt_left, s.placement_left));
    }
    if known(s.batt_right) {
        out.push(bud(BatteryId::Right, "Right", s.batt_right, s.placement_right));
    }
    // The CLI itself shows the case only while a bud is in it: with both buds
    // out its level is not reported.
    if known(s.batt_case) && (in_case(s.placement_left) || in_case(s.placement_right)) {
        out.push(Battery::new(BatteryId::Case, "Case", s.batt_case, false, None));
    }
    out
}

fn ambient_level(s: &Status, top: i32) -> f64 {
    if !s.ambient_enabled {
        return 0.0;
    }
    f64::from(top).min(if s.extra_high { 4.0 } else { s.ambient_volume })
}

/// `name` is what BlueZ shows for the device. A status the daemon has not
/// finished the handshake for (`ready` false) has no levels to show.
pub fn normalise(status: &Status, name: &str) -> Vec<Device> {
    if !status.ok || !status.ready || !valid_address(&status.address) {
        return Vec::new();
    }
    let model = model(&status.model);
    let mut controls = Vec::new();
    if model.anc {
        controls.push(toggle("anc", "Noise cancellation", "", "Listening mode", status.anc));
    }
    let mut level = -1.0;
    if model.ambient > 0 {
        let options = (0..=model.ambient)
            .map(|i| ControlOption::new(i, &if i == 0 { "Off".to_string() } else { i.to_string() }, ""))
            .collect();
        level = ambient_level(status, model.ambient);
        controls.push(choice("ambient", "Ambient sound", "Listening mode", Value::Num(level), options));
    }
    let eq = (status.equalizer >= 0.0 && status.equalizer.fract() == 0.0)
        .then(|| EQ.get(status.equalizer as usize))
        .flatten()
        .map_or(Value::Null, |(v, _)| Value::str(v));
    controls.push(choice(
        "eq",
        "Equalizer",
        "Equalizer",
        eq,
        EQ.iter().map(|(v, label)| ControlOption::new(*v, label, "")).collect(),
    ));
    let line = if model.anc || model.ambient > 0 {
        if status.anc && model.anc {
            "Noise cancellation"
        } else if level > 0.0 {
            "Ambient sound"
        } else {
            "Off"
        }
    } else {
        ""
    };
    let address = status.address.to_uppercase();
    vec![device(DeviceFields {
        key: format!("{BACKEND}:{address}"),
        backend: BACKEND.into(),
        address,
        name: if name.is_empty() { model.name.into() } else { name.into() },
        kind: DeviceKind::Earbuds,
        connected: true,
        batteries: batteries(status),
        controls,
        state_line: line.into(),
        art: String::new(),
    })]
}

/// The CLI arguments after the device flags, or `None` for anything outside
/// this list. Ambient tops out at 4 here; the daemon refuses a level past the
/// model's own maximum.
pub fn command(control_key: &str, value: &Value) -> Option<Vec<String>> {
    let words = |w: &[&str]| Some(w.iter().map(|s| s.to_string()).collect());
    match (control_key, value) {
        ("anc", Value::Bool(on)) => words(&[if *on { "enable" } else { "disable" }, "anc"]),
        ("ambient", Value::Num(n)) if (0.0..=4.0).contains(n) && js::round(*n) == *n => {
            words(&["set", "ambientsound", &js::num_str(*n)])
        }
        ("eq", Value::Str(v)) if EQ.iter().any(|(e, _)| e == v) => words(&["set", "equalizer", v]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::earbuds::{battery_rows, control, sections, worst_level};
    use serde_json::json;

    // LiveBudsCli's `earbuds status -o json` prints the daemon's response
    // verbatim (src/cmd/info.rs), one JSON line: Response<BudsInfoInner> in
    // src/daemon/unix_socket/mod.rs, its payload field order and names from
    // BudsInfoInner in src/daemon/buds_info.rs, the error form from get_err in
    // request_handler.rs (all at commit 37f27b5). No daemon or buds run here,
    // so the values are hand-written to that shape; Model is serde's plain
    // variant name, the number fields the wire values of galaxy_buds_rs
    // 0.2.11's Placement and EqualizerType.
    fn payload(model: &str, extra: serde_json::Value) -> String {
        let mut p = json!({
            "address": "AA:BB:CC:11:22:33",
            "ready": true,
            "batt_left": 82,
            "batt_right": 77,
            "batt_case": 60,
            "placement_left": 1,
            "placement_right": 1,
            "equalizer_type": 1,
            "touchpads_blocked": false,
            "noise_reduction": false,
            "did_battery_notify": false,
            "touchpad_option_left": 2,
            "touchpad_option_right": 2,
            "paused_music_earlier": false,
            "debug": { "voltage_left": 4.1, "voltage_right": 4.1, "temperature_left": 24.5, "temperature_right": 24.5, "current_left": 0.001, "current_right": 0.001 },
            "model": model,
            "ambient_sound_enabled": false,
            "ambient_sound_volume": 0,
            "extra_high_ambient_volume": false,
            "tab_lock_status": { "touch_an_hold_on": true, "triple_tap_on": true, "double_tap_on": true, "tap_on": true, "touch_controls_on": true }
        });
        if let Some(extra) = extra.as_object() {
            for (k, v) in extra {
                p[k] = v.clone();
            }
        }
        json!({ "status": "success", "device": "AA:BB:CC:11:22:33", "status_message": null, "payload": p }).to_string()
    }

    fn plain(model: &str) -> String {
        payload(model, json!({}))
    }

    const ERROR_LINE: &str = r#"{"status":"error","device":"","status_message":"No connected device found","payload":null}"#;

    fn keys<T>(list: &[T], f: impl Fn(&T) -> String) -> String {
        list.iter().map(f).collect::<Vec<_>>().join(",")
    }

    fn device_of(text: &str, name: &str) -> Device {
        let mut devs = normalise(&parse_status(text), name);
        assert_eq!(devs.len(), 1);
        devs.remove(0)
    }

    fn bt(address: &str, name: &str, device_name: &str, connected: bool) -> bluetooth::Device {
        bluetooth::Device { address: address.into(), name: name.into(), device_name: device_name.into(), connected, ..Default::default() }
    }

    fn option_values(ctl: &crate::earbuds::Control) -> String {
        keys(ctl.options.as_ref().unwrap(), |o| match &o.value {
            Value::Num(n) => js::num_str(*n),
            v => v.as_str().unwrap().into(),
        })
    }

    #[test]
    fn buds2_has_anc_and_ambient() {
        let d = device_of(&payload("Buds2", json!({ "noise_reduction": true, "equalizer_type": 3 })), "Kyan's Buds2");
        assert_eq!(d.key, "samsung:AA:BB:CC:11:22:33");
        assert_eq!(d.backend, "samsung");
        assert_eq!(d.name, "Kyan's Buds2");
        assert_eq!(d.state_line, "Noise cancellation");
        assert_eq!(keys(&battery_rows(Some(&d)), |r| r.key.as_str().into()), "left,right");
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "anc,ambient,eq");
        assert_eq!(keys(&sections(Some(&d)), |s| s.section.clone()), "Listening mode,Equalizer");
        assert_eq!(control(Some(&d), "anc").unwrap().kind, crate::earbuds::ControlKind::Toggle);
        assert_eq!(control(Some(&d), "anc").unwrap().value, true.into());
        let ambient = control(Some(&d), "ambient").unwrap();
        assert_eq!(option_values(ambient), "0,1,2,3");
        assert_eq!(keys(ambient.options.as_ref().unwrap(), |o| o.label.clone()), "Off,1,2,3");
        assert_eq!(ambient.value, 0.into());
        assert_eq!(control(Some(&d), "eq").unwrap().value, "dynamic".into());
        assert_eq!(option_values(control(Some(&d), "eq").unwrap()), "normal,bass,soft,dynamic,clear,treble");
    }

    #[test]
    fn buds_plus_ambient_only_with_extra_high() {
        let d = device_of(
            &payload("BudsPlus", json!({ "ambient_sound_enabled": true, "ambient_sound_volume": 3, "extra_high_ambient_volume": true })),
            "",
        );
        assert_eq!(d.name, "Galaxy Buds+");
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "ambient,eq");
        assert_eq!(option_values(control(Some(&d), "ambient").unwrap()), "0,1,2,3,4");
        assert_eq!(control(Some(&d), "ambient").unwrap().value, 4.into());
        assert_eq!(d.state_line, "Ambient sound");
    }

    #[test]
    fn buds_pro_has_anc_only() {
        let d = device_of(&plain("BudsPro"), "");
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "anc,eq");
        assert_eq!(control(Some(&d), "anc").unwrap().value, false.into());
        assert_eq!(d.state_line, "Off");
    }

    #[test]
    fn unlisted_model_gets_no_listening_controls() {
        let d = device_of(&plain("SomethingNew"), "");
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "eq");
        assert_eq!(d.state_line, "");
        assert_eq!(normalise(&parse_status(&plain("constructor")), "").len(), 1);
    }

    #[test]
    fn batteries_placement_and_case() {
        let out = device_of(&payload("Buds2", json!({ "placement_left": 2, "placement_right": 2, "batt_case": 60 })), "");
        assert_eq!(keys(&battery_rows(Some(&out)), |r| r.key.as_str().into()), "left,right");
        assert_eq!(battery_rows(Some(&out))[0].hint, "");
        let mixed = device_of(&payload("Buds2", json!({ "placement_left": 3, "placement_right": 1 })), "");
        let rows = battery_rows(Some(&mixed));
        assert_eq!(keys(&rows, |r| r.key.as_str().into()), "left,right,case");
        assert_eq!(rows[0].hint, "Charging");
        assert_eq!(rows[1].hint, "In ear");
        assert_eq!(rows[2].level, 60.0);
        assert_eq!(worst_level(Some(&mixed)), 77.0);
    }

    #[test]
    fn undetected_equalizer_lights_nothing() {
        let d = device_of(&payload("Buds2", json!({ "equalizer_type": 9 })), "");
        assert_eq!(control(Some(&d), "eq").unwrap().value, Value::Null);
    }

    #[test]
    fn not_ready_or_error_lists_nothing() {
        assert_eq!(normalise(&parse_status(&payload("Buds2", json!({ "ready": false }))), "").len(), 0);
        let e = parse_status(ERROR_LINE);
        assert!(!e.ok);
        assert_eq!(e.message, "No connected device found");
        assert_eq!(normalise(&e, "").len(), 0);
        assert!(!parse_status("").ok);
        assert!(!parse_status("Could not connect to daemon").ok);
        assert!(!parse_status("{not json").ok);
    }

    #[test]
    fn leading_daemon_banner_is_skipped() {
        let s = parse_status(&format!("Daemon started successfully\n{}", plain("Buds2")));
        assert!(s.ok);
        assert_eq!(s.model, "Buds2");
    }

    #[test]
    fn connected_galaxy_buds_only() {
        let found = connected_buds(&[
            bt("aa:bb:cc:11:22:33", "My buds", "Galaxy Buds2 Pro (1234)", true),
            bt("AA:BB:CC:44:55:66", "Galaxy Buds Live (ABCD)", "Galaxy Buds Live (ABCD)", false),
            bt("AA:BB:CC:77:88:99", "WH-1000XM5", "WH-1000XM5", true),
            bt("AA:BB:CC:00:00:01", "Galaxy Buds+ (9F2A)", "", true),
        ]);
        assert_eq!(keys(&found, |b| b.address.clone()), "AA:BB:CC:11:22:33,AA:BB:CC:00:00:01");
        assert_eq!(found[0].name, "My buds");
        assert_eq!(connected_buds(&[]).len(), 0);
    }

    #[test]
    fn command_allow_list() {
        let cmd = |k: &str, v: Value| command(k, &v).map(|a| a.join(" "));
        assert_eq!(cmd("anc", true.into()).as_deref(), Some("enable anc"));
        assert_eq!(cmd("anc", false.into()).as_deref(), Some("disable anc"));
        assert_eq!(cmd("anc", "on".into()), None);
        assert_eq!(cmd("ambient", 2.0.into()).as_deref(), Some("set ambientsound 2"));
        assert_eq!(cmd("ambient", 0.0.into()).as_deref(), Some("set ambientsound 0"));
        assert_eq!(cmd("ambient", 5.0.into()), None);
        assert_eq!(cmd("ambient", 1.5.into()), None);
        assert_eq!(cmd("ambient", "2".into()), None);
        assert_eq!(cmd("eq", "bass".into()).as_deref(), Some("set equalizer bass"));
        assert_eq!(cmd("eq", "bass; reboot".into()), None);
        assert_eq!(cmd("touchpad", true.into()), None);
    }

    #[test]
    fn argv_builders() {
        assert_eq!(status_argv("AA:BB:CC:11:22:33").unwrap().join(" "), "earbuds -q -s AA:BB:CC:11:22:33 status -o json");
        let tail = ["set".to_string(), "equalizer".to_string(), "soft".to_string()];
        assert_eq!(argv("AA:BB:CC:11:22:33", &tail).unwrap().join(" "), "earbuds -q -s AA:BB:CC:11:22:33 set equalizer soft");
        assert_eq!(argv("--daemon", &["status".into()]), None);
    }
}
