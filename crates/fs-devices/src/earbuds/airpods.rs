//! AirPods adapter for the earbuds model: the omarchy-pods librepods
//! daemon's status.json in, the daemon's control-socket verbs out. Talks only
//! to the wire shape the daemon publishes; nothing here is ported from that
//! project's own (GPL) source, this is an independent reimplementation of the
//! documented contract. Ported from `shell/Earbuds/airpods.js`.
//!
//! `left`/`right`/`case` are absent from the daemon's JSON entirely until a
//! battery packet has arrived, never present with `available:false`, so
//! every parse path returns a complete default shape rather than leaving a
//! caller to guard against missing fields. `connected:false` does not mean
//! nothing is known: battery keeps arriving over BLE adverts while the buds
//! sit in the case, which is why [`normalise`] keeps a device with a known
//! level and the link down.

use serde_json::Value as Json;

use super::{
    Battery, BatteryId, ControlOption, Device, DeviceFields, DeviceKind, Value, choice, device, range, toggle, Unit,
};
use fs_js as js;

pub const BACKEND: &str = "airpods";

/// `noise_mode` wire values (the daemon's status.json, and the
/// `noise:<mode>` control verb suffixes).
pub mod noise_mode {
    pub const OFF: f64 = 0.0;
    pub const ANC: f64 = 1.0;
    pub const TRANSPARENCY: f64 = 2.0;
    pub const ADAPTIVE: f64 = 3.0;
}

const NOISE_MODE_KEY: [&str; 4] = ["off", "anc", "transparency", "adaptive"];

/// `ear_detection_behavior`'s own numbering: 0 one out, 1 both out, 2 never.
const EAR_KEY: [&str; 3] = ["one", "both", "off"];

#[derive(Clone, Debug, PartialEq)]
pub struct Pod {
    pub available: bool,
    pub level: f64,
    pub charging: bool,
    pub in_ear: bool,
}

impl Default for Pod {
    fn default() -> Pod {
        Pod { available: false, level: -1.0, charging: false, in_ear: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaseBattery {
    pub available: bool,
    pub level: f64,
    pub charging: bool,
}

impl Default for CaseBattery {
    fn default() -> CaseBattery {
        CaseBattery { available: false, level: -1.0, charging: false }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Status {
    pub ok: bool,
    pub connected: bool,
    pub device_name: String,
    pub model_name: String,
    pub model_number: String,
    pub is_pro: bool,
    pub supports_off: bool,
    pub noise_mode: f64,
    pub left: Pod,
    pub right: Pod,
    pub case_battery: CaseBattery,
    pub conversational_awareness: bool,
    pub adaptive_noise_level: f64,
    pub one_bud_anc: bool,
    pub ear_detection: f64,
    pub lid_state: f64,
}

impl Default for Status {
    fn default() -> Status {
        Status {
            ok: false,
            connected: false,
            device_name: String::new(),
            model_name: String::new(),
            model_number: String::new(),
            is_pro: false,
            supports_off: true,
            noise_mode: -1.0,
            left: Pod::default(),
            right: Pod::default(),
            case_battery: CaseBattery::default(),
            conversational_awareness: false,
            adaptive_noise_level: 0.0,
            one_bud_anc: false,
            ear_detection: 0.0,
            lid_state: 2.0,
        }
    }
}

fn flag(raw: &Json, key: &str) -> bool {
    js::field(raw, key) == Some(&Json::Bool(true))
}

fn number(raw: &Json, key: &str) -> Option<f64> {
    js::field(raw, key).and_then(|v| if v.is_number() { v.as_f64() } else { None })
}

fn text(raw: &Json, key: &str) -> String {
    js::field(raw, key).and_then(Json::as_str).unwrap_or("").to_string()
}

fn parse_pod(raw: Option<&Json>) -> Pod {
    match raw.filter(|r| js::is_object_like(r)) {
        None => Pod::default(),
        Some(raw) => Pod {
            available: flag(raw, "available"),
            level: number(raw, "level").unwrap_or(-1.0),
            charging: flag(raw, "charging"),
            in_ear: flag(raw, "in_ear"),
        },
    }
}

fn parse_case(raw: Option<&Json>) -> CaseBattery {
    match raw.filter(|r| js::is_object_like(r)) {
        None => CaseBattery::default(),
        Some(raw) => CaseBattery {
            available: flag(raw, "available"),
            level: number(raw, "level").unwrap_or(-1.0),
            charging: flag(raw, "charging"),
        },
    }
}

/// `text`: one line of the daemon's own JSON, or "" / malformed / a foreign
/// schema_version. Every one of those returns the same complete default shape
/// (`ok: false`) rather than a partial one: a caller checks `ok` once.
pub fn parse_status(text_in: &str) -> Status {
    let mut result = Status::default();
    if text_in.is_empty() {
        return result;
    }
    let Ok(raw) = serde_json::from_str::<Json>(text_in) else { return result };
    if !js::is_object_like(&raw) || number(&raw, "schema_version") != Some(1.0) {
        return result;
    }

    result.ok = true;
    result.connected = flag(&raw, "connected");
    result.device_name = text(&raw, "device_name");
    result.model_name = text(&raw, "model_name");
    result.model_number = text(&raw, "model_number");
    result.is_pro = flag(&raw, "is_pro_series");
    // A missing key defaults to supporting Off, only Pro 3 sets this false.
    result.supports_off = js::field(&raw, "supports_noise_off") != Some(&Json::Bool(false));
    result.noise_mode = number(&raw, "noise_mode").unwrap_or(-1.0);
    result.left = parse_pod(js::field(&raw, "left"));
    result.right = parse_pod(js::field(&raw, "right"));
    result.case_battery = parse_case(js::field(&raw, "case"));
    result.conversational_awareness = flag(&raw, "conversational_awareness");
    result.adaptive_noise_level = number(&raw, "adaptive_noise_level").unwrap_or(0.0);
    result.one_bud_anc = flag(&raw, "one_bud_anc_mode");
    result.ear_detection = number(&raw, "ear_detection_behavior").unwrap_or(0.0);
    result.lid_state = number(&raw, "lid_state").unwrap_or(2.0);
    result
}

pub fn lid_label(n: f64) -> &'static str {
    if n == 0.0 {
        "Lid open"
    } else if n == 1.0 {
        "Lid closed"
    } else {
        ""
    }
}

pub fn noise_mode_label(n: f64) -> &'static str {
    match n {
        0.0 => "Off",
        1.0 => "Noise cancellation",
        2.0 => "Transparency",
        3.0 => "Adaptive",
        _ => "Unknown",
    }
}

/// The hero meta line: the noise mode when connected, else "Not connected",
/// fused with the lid state when known, " / ".
pub fn state_line(status: &Status) -> String {
    let mut parts = vec![if status.connected { noise_mode_label(status.noise_mode) } else { "Not connected" }];
    let lid = lid_label(status.lid_state);
    if !lid.is_empty() {
        parts.push(lid);
    }
    parts.join(" / ")
}

fn batteries(status: &Status) -> Vec<Battery> {
    let mut out = Vec::new();
    if status.left.available && status.left.level >= 0.0 {
        out.push(Battery::new(BatteryId::Left, "Left", status.left.level, status.left.charging, Some(status.left.in_ear)));
    }
    if status.right.available && status.right.level >= 0.0 {
        out.push(Battery::new(BatteryId::Right, "Right", status.right.level, status.right.charging, Some(status.right.in_ear)));
    }
    if status.case_battery.available && status.case_battery.level >= 0.0 {
        out.push(Battery::new(BatteryId::Case, "Case", status.case_battery.level, status.case_battery.charging, None));
    }
    out
}

/// `table[n]` for a number used as an array index.
fn table(keys: &'static [&'static str], n: f64) -> Option<&'static str> {
    (n >= 0.0 && n.fract() == 0.0).then(|| keys.get(n as usize).copied()).flatten()
}

fn key_value(key: Option<&str>) -> Value {
    key.map_or(Value::Null, Value::str)
}

/// Listening mode and the two Pro toggles need the L2CAP link up to mean
/// anything. Off only exists while the device supports it (Pro 3 dropped it),
/// Adaptive and its level only on Pro models. Ear detection is host-side
/// daemon policy, so it stays whenever the device is known at all, in-case
/// included.
fn controls(status: &Status) -> Vec<super::Control> {
    let mut out = Vec::new();
    if status.connected {
        let mut modes = Vec::new();
        if status.supports_off {
            modes.push(ControlOption::new("off", "Off", "circle-off"));
        }
        modes.push(ControlOption::new("anc", "ANC", "ear-off"));
        modes.push(ControlOption::new("transparency", "Transparency", "ear"));
        if status.is_pro {
            modes.push(ControlOption::new("adaptive", "Adaptive", "audio-waveform"));
        }
        let mode = table(&NOISE_MODE_KEY, status.noise_mode);
        out.push(choice("noise", "Listening mode", "Listening mode", key_value(mode), modes));
        if status.noise_mode == noise_mode::ADAPTIVE {
            out.push(range("adaptive", "Adaptive noise", "Listening mode", status.adaptive_noise_level, 0.0, 100.0, 5.0, Unit::Percent));
        }
        if status.is_pro {
            out.push(toggle("ca", "Conversation awareness", "Lowers volume when you talk", "Options", status.conversational_awareness));
            out.push(toggle("onebud", "One-bud ANC", "Keeps ANC with one pod in", "Options", status.one_bud_anc));
        }
    }
    let ear = table(&EAR_KEY, status.ear_detection);
    out.push(choice(
        "ear",
        "Ear detection",
        "Ear detection",
        key_value(ear),
        vec![ControlOption::new("one", "One", ""), ControlOption::new("both", "Both", ""), ControlOption::new("off", "Off", "")],
    ));
    out
}

/// Model numbers to picture sets, from librepods' own model table
/// (android/.../data/AirPods.kt); the AirPods 5 with the wireless case
/// reuses the 4's pictures there too.
const ART: [(&[&str], &str); 11] = [
    (&["A1523", "A1722"], "airpods_1"),
    (&["A2032", "A2031"], "airpods_2"),
    (&["A2565", "A2564"], "airpods_3"),
    (&["A3053", "A3050", "A3054"], "airpods_4"),
    (&["A3056", "A3055", "A3057"], "airpods_4_anc"),
    (&["A3531", "A3532", "A3533"], "airpods_5"),
    (&["A3439", "A3440", "A3441"], "airpods_4"),
    (&["A2084", "A2083"], "airpods_pro_1"),
    (&["A2931", "A2699", "A2698"], "airpods_pro_2"),
    (&["A3047", "A3048", "A3049"], "airpods_pro_2"),
    (&["A3063", "A3064", "A3065"], "airpods_pro_3"),
];

/// The model number when the table knows it, else the daemon's model name,
/// which spells the generation out ("AirPods Pro 3", "AirPods (3rd
/// generation)"). An AirPods Max has no pictures.
fn art(status: &Status) -> String {
    if let Some((_, set)) = ART.iter().find(|(numbers, _)| numbers.contains(&status.model_number.as_str())) {
        return (*set).into();
    }
    let name = status.model_name.to_ascii_lowercase();
    let names = [("pro 3", "airpods_pro_3"), ("pro 2", "airpods_pro_2"), ("pro", "airpods_pro_1"), ("4 (anc", "airpods_4_anc"), ("4", "airpods_4"), ("3", "airpods_3"), ("2", "airpods_2")];
    if name.contains("max") {
        return String::new();
    }
    names.iter().find(|(word, _)| name.contains(word)).map_or("", |(_, set)| set).into()
}

/// A live daemon that has never seen a battery packet and reports the link
/// down knows of no device: the file existing only proves the daemon is up.
pub fn normalise(status: &Status) -> Vec<Device> {
    if !status.ok {
        return Vec::new();
    }
    let batteries = batteries(status);
    if !status.connected && batteries.is_empty() {
        return Vec::new();
    }
    vec![device(DeviceFields {
        key: BACKEND.into(),
        backend: BACKEND.into(),
        address: String::new(),
        name: if status.device_name.is_empty() { "AirPods".into() } else { status.device_name.clone() },
        kind: DeviceKind::Earbuds,
        connected: status.connected,
        batteries,
        controls: controls(status),
        state_line: state_line(status),
        art: art(status),
    })]
}

/// The daemon socket's own verbs (`daemon/librepods-ctl.cpp`'s usage text,
/// read-reference only). connect/disconnect/forget are deliberately left out:
/// those shell out to bluetoothctl, and that job belongs to the Bluetooth
/// panel. `None` for anything outside this list.
pub fn command(control_key: &str, value: &Value) -> Option<String> {
    match (control_key, value) {
        ("noise", Value::Str(v)) if NOISE_MODE_KEY.contains(&v.as_str()) => Some(format!("noise:{v}")),
        ("adaptive", Value::Num(n)) if (0.0..=100.0).contains(n) => Some(format!("adaptive:{}", js::num_str(js::round(*n)))),
        ("ca", Value::Bool(on)) => Some(format!("ca:{}", if *on { "on" } else { "off" })),
        ("onebud", Value::Bool(on)) => Some(format!("onebud:{}", if *on { "on" } else { "off" })),
        ("ear", Value::Str(v)) if EAR_KEY.contains(&v.as_str()) => Some(format!("ear:{v}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::earbuds::{battery_rows, coerce, control, range_text, sections, worst_level};

    // Fixture lines: one-line, sorted-key JSON, exactly the shape the
    // librepods daemon writes to status.json.
    const PRO3: &str = r#"{"adaptive_noise_level":40,"case":{"available":true,"charging":false,"level":80},"connected":true,"conversational_awareness":true,"device_name":"Kyan's AirPods Pro","ear_detection_behavior":0,"is_pro_series":true,"left":{"available":true,"charging":false,"in_ear":true,"level":85},"lid_state":1,"model_int":1,"model_name":"AirPods Pro 3","model_number":"A3456","noise_mode":3,"one_bud_anc_mode":true,"right":{"available":true,"charging":false,"in_ear":true,"level":90},"schema_version":1,"supports_noise_off":false}"#;

    const NON_PRO: &str = r#"{"adaptive_noise_level":0,"case":{"available":true,"charging":true,"level":55},"connected":true,"conversational_awareness":false,"device_name":"AirPods","ear_detection_behavior":1,"is_pro_series":false,"left":{"available":true,"charging":false,"in_ear":false,"level":60},"lid_state":0,"model_int":2,"model_name":"AirPods (3rd generation)","model_number":"A2564","noise_mode":1,"one_bud_anc_mode":false,"right":{"available":true,"charging":false,"in_ear":true,"level":58},"schema_version":1,"supports_noise_off":true}"#;

    const FRESH_DAEMON: &str = r#"{"adaptive_noise_level":0,"connected":false,"conversational_awareness":false,"device_name":"","ear_detection_behavior":0,"is_pro_series":false,"lid_state":2,"model_int":0,"model_name":"","model_number":"","noise_mode":-1,"one_bud_anc_mode":false,"schema_version":1,"supports_noise_off":true}"#;

    const IN_CASE: &str = r#"{"adaptive_noise_level":0,"case":{"available":true,"charging":true,"level":45},"connected":false,"conversational_awareness":true,"device_name":"Kyan's AirPods Pro","ear_detection_behavior":0,"is_pro_series":true,"left":{"available":true,"charging":true,"in_ear":false,"level":72},"lid_state":1,"model_int":1,"model_name":"AirPods Pro 3","model_number":"A3456","noise_mode":-1,"one_bud_anc_mode":false,"right":{"available":true,"charging":true,"in_ear":false,"level":71},"schema_version":1,"supports_noise_off":false}"#;

    fn wrong_schema() -> String {
        PRO3.replace("\"schema_version\":1", "\"schema_version\":2")
    }

    fn keys<T, F: Fn(&T) -> String>(list: &[T], f: F) -> String {
        list.iter().map(f).collect::<Vec<_>>().join(",")
    }

    fn device_of(text: &str) -> Device {
        let mut devs = normalise(&parse_status(text));
        assert_eq!(devs.len(), 1);
        devs.remove(0)
    }

    fn option_values(ctl: &super::super::Control, f: impl Fn(&ControlOption) -> String) -> String {
        keys(ctl.options.as_ref().unwrap(), f)
    }

    fn value_of(o: &ControlOption) -> String {
        o.value.as_str().unwrap().to_string()
    }

    #[test]
    fn pro3_full_status() {
        let s = parse_status(PRO3);
        assert!(s.ok);
        assert!(s.is_pro);
        assert!(!s.supports_off);
        assert_eq!(s.noise_mode, 3.0);

        let d = device_of(PRO3);
        assert_eq!(d.key, "airpods");
        assert_eq!(d.backend, "airpods");
        assert_eq!(d.name, "Kyan's AirPods Pro");
        assert!(d.connected);
        assert_eq!(d.state_line, "Adaptive / Lid closed");

        let rows = battery_rows(Some(&d));
        assert_eq!(keys(&rows, |r| r.key.as_str().into()), "left,right,case");
        assert_eq!(rows[0].level, 85.0);
        assert_eq!(rows[0].hint, "In ear");
        assert_eq!(rows[2].hint, "");
        assert_eq!(worst_level(Some(&d)), 85.0);

        assert_eq!(keys(&d.controls, |c| c.key.clone()), "noise,adaptive,ca,onebud,ear");
        assert_eq!(keys(&sections(Some(&d)), |s| s.section.clone()), "Listening mode,Options,Ear detection");

        let noise = control(Some(&d), "noise").unwrap();
        assert_eq!(noise.kind, super::super::ControlKind::Choice);
        assert_eq!(option_values(noise, value_of), "anc,transparency,adaptive");
        assert_eq!(option_values(noise, |o| o.icon.clone()), "ear-off,ear,audio-waveform");
        assert_eq!(noise.value, "adaptive".into());

        let adaptive = control(Some(&d), "adaptive").unwrap();
        assert_eq!(adaptive.kind, super::super::ControlKind::Range);
        assert_eq!(adaptive.value, 40.0.into());
        assert_eq!(adaptive.step, 5.0);
        assert_eq!(range_text(Some(adaptive)), "40%");

        assert_eq!(control(Some(&d), "ca").unwrap().value, true.into());
        assert_eq!(control(Some(&d), "ca").unwrap().hint, "Lowers volume when you talk");
        assert_eq!(control(Some(&d), "onebud").unwrap().value, true.into());
        assert_eq!(control(Some(&d), "ear").unwrap().value, "one".into());
    }

    #[test]
    fn nonpro_status_filters_adaptive_and_keeps_off() {
        let d = device_of(NON_PRO);
        let noise = control(Some(&d), "noise").unwrap();
        assert_eq!(option_values(noise, value_of), "off,anc,transparency");
        assert_eq!(noise.value, "anc".into());
        assert!(control(Some(&d), "adaptive").is_none());
        assert!(control(Some(&d), "ca").is_none());
        assert!(control(Some(&d), "onebud").is_none());
        assert_eq!(control(Some(&d), "ear").unwrap().value, "both".into());
        assert_eq!(keys(&sections(Some(&d)), |s| s.section.clone()), "Listening mode,Ear detection");

        assert_eq!(battery_rows(Some(&d))[1].hint, "In ear");
        assert_eq!(d.state_line, "Noise cancellation / Lid open");
    }

    #[test]
    fn adaptive_level_only_in_adaptive_mode() {
        let d = device_of(&PRO3.replace("\"noise_mode\":3", "\"noise_mode\":1"));
        assert_eq!(control(Some(&d), "noise").unwrap().value, "anc".into());
        assert!(control(Some(&d), "adaptive").is_none());
    }

    #[test]
    fn fresh_daemon_knows_no_device() {
        let s = parse_status(FRESH_DAEMON);
        assert!(s.ok);
        assert!(!s.left.available);
        assert_eq!(s.left.level, -1.0);
        assert_eq!(normalise(&s).len(), 0);
    }

    #[test]
    fn in_case_keeps_battery_and_ear_detection_only() {
        let d = device_of(IN_CASE);
        assert!(!d.connected);
        assert_eq!(keys(&battery_rows(Some(&d)), |r| r.key.as_str().into()), "left,right,case");
        assert_eq!(battery_rows(Some(&d))[0].hint, "Charging");
        assert_eq!(battery_rows(Some(&d))[2].hint, "Charging");
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "ear");
        assert_eq!(d.state_line, "Not connected / Lid closed");
    }

    #[test]
    fn wrong_schema_version_returns_default_shape() {
        let s = parse_status(&wrong_schema());
        assert!(!s.ok);
        assert_eq!(s.device_name, "");
        assert_eq!(normalise(&s).len(), 0);
    }

    #[test]
    fn malformed_json_returns_default_shape() {
        let s = parse_status("{not json at all");
        assert!(!s.ok);
        assert!(!s.left.available);
        assert_eq!(normalise(&s).len(), 0);
    }

    #[test]
    fn empty_text_returns_default_shape() {
        let s = parse_status("");
        assert!(!s.ok);
        assert!(s.supports_off);
        assert_eq!(s.lid_state, 2.0);
    }

    #[test]
    fn lid_and_noise_mode_labels() {
        assert_eq!(lid_label(0.0), "Lid open");
        assert_eq!(lid_label(1.0), "Lid closed");
        assert_eq!(lid_label(2.0), "");
        assert_eq!(noise_mode_label(0.0), "Off");
        assert_eq!(noise_mode_label(1.0), "Noise cancellation");
        assert_eq!(noise_mode_label(2.0), "Transparency");
        assert_eq!(noise_mode_label(3.0), "Adaptive");
        assert_eq!(noise_mode_label(-1.0), "Unknown");
    }

    // The daemon socket's verbs, and nothing outside them.
    #[test]
    fn command_allow_list() {
        let cmd = |k: &str, v: Value| command(k, &v);
        assert_eq!(cmd("noise", "anc".into()).as_deref(), Some("noise:anc"));
        assert_eq!(cmd("noise", "adaptive".into()).as_deref(), Some("noise:adaptive"));
        assert_eq!(cmd("noise", "loud".into()), None);
        assert_eq!(cmd("adaptive", 40.0.into()).as_deref(), Some("adaptive:40"));
        assert_eq!(cmd("adaptive", 101.0.into()), None);
        assert_eq!(cmd("adaptive", "40".into()), None);
        assert_eq!(cmd("ca", true.into()).as_deref(), Some("ca:on"));
        assert_eq!(cmd("ca", false.into()).as_deref(), Some("ca:off"));
        assert_eq!(cmd("onebud", true.into()).as_deref(), Some("onebud:on"));
        assert_eq!(cmd("ear", "both".into()).as_deref(), Some("ear:both"));
        assert_eq!(cmd("ear", "never".into()), None);
        assert_eq!(cmd("connect", true.into()), None);
        assert_eq!(cmd("forget", "".into()), None);
    }

    // What an IPC string becomes before it reaches command().
    #[test]
    fn ipc_strings_coerce_into_commands() {
        let d = device_of(PRO3);
        let via = |key: &str, raw: &str| {
            let v = coerce(control(Some(&d), key), &raw.into())?;
            command(key, &v)
        };
        assert_eq!(via("ca", "off").as_deref(), Some("ca:off"));
        assert_eq!(via("adaptive", "42").as_deref(), Some("adaptive:42"));
        assert_eq!(via("noise", "transparency").as_deref(), Some("noise:transparency"));
        assert_eq!(coerce(control(Some(&d), "noise"), &"off".into()), None);
    }

    #[test]
    fn art_by_model_number_then_name() {
        assert_eq!(device_of(NON_PRO).art, "airpods_3");
        // A3456 is no real model number, so the name decides.
        assert_eq!(device_of(PRO3).art, "airpods_pro_3");
        let max = Status { model_name: "AirPods Max".into(), ..Status::default() };
        assert_eq!(art(&max), "");
    }
}
