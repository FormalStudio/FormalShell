//! Soundcore adapter for the earbuds model, over the `openscq30` CLI
//! (github.com/Oppzippy/OpenSCQ30, cli/src/cli.rs and cli/src/cli/device.rs
//! at v2.12.0). Only the documented `--json` outputs are read, since the
//! plain-text ones are not stable across versions. Ported from
//! `shell/Earbuds/soundcore.js`.
//!
//! A device appears only when it is in openscq30's own paired list (a MAC and
//! a model id it was told about, not the Bluetooth pairing) and BlueZ reports
//! it connected. Demo entries are skipped. A `--get` of a setting id the model
//! does not have fails the whole call, so the ids to fetch come from
//! `list-settings` once per connection ([`Caps`]), never guessed.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value as Json;

use super::{
    Battery, BatteryId, ControlOption, Device, DeviceFields, DeviceKind, Value, choice, device, valid_address,
};
use crate::bluetooth;
use fs_js as js;

pub const BACKEND: &str = "soundcore";
pub const BINARY: &str = "openscq30";

/// ambientSoundMode's option ids, in the order the panel draws them. An id the
/// model's select does not list is left out; AirplaneMode and any future one
/// is never offered.
const MODES: [(&str, &str, &str); 3] =
    [("Normal", "Normal", "circle-off"), ("NoiseCanceling", "ANC", "ear-off"), ("Transparency", "Transparency", "ear")];

/// presetEqualizerProfile has 22 options on most models. Even wrapped, with
/// every label as short as "Treble reducer", that is more than four rows at
/// the panel's width, so the group carries these four.
const PRESETS: [(&str, &str); 4] = [
    ("SoundcoreSignature", "Signature"),
    ("BassBooster", "Bass boost"),
    ("TrebleBooster", "Treble boost"),
    ("Podcast", "Podcast"),
];

const INFORMATION_IDS: [&str; 7] = [
    "batteryLevel",
    "isCharging",
    "batteryLevelLeft",
    "isChargingLeft",
    "batteryLevelRight",
    "isChargingRight",
    "caseBatteryLevel",
];

fn json(text: &str) -> Option<Json> {
    serde_json::from_str(text).ok()
}

/// One row of `paired-devices list --json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paired {
    pub address: String,
    pub model: String,
    pub demo: bool,
}

pub fn parse_paired(text: &str) -> Vec<Paired> {
    let Some(Json::Array(raw)) = json(text) else { return Vec::new() };
    raw.iter()
        .filter_map(|p| {
            let address = js::field(p, "macAddress")?.as_str().filter(|a| valid_address(a))?;
            Some(Paired {
                address: address.to_uppercase(),
                model: js::field(p, "model").and_then(Json::as_str).unwrap_or("").to_string(),
                demo: js::field(p, "isDemo") == Some(&Json::Bool(true)),
            })
        })
        .collect()
}

/// A paired, connected, non-demo device with the name BlueZ shows for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub address: String,
    pub model: String,
    pub name: String,
}

/// `bluetooth`: the BlueZ devices. The paired entry each connected device
/// matches.
pub fn connected_paired(paired: &[Paired], bluetooth: &[bluetooth::Device]) -> Vec<Info> {
    let mut out = Vec::new();
    for bt in bluetooth {
        if !bt.connected || !valid_address(&bt.address) {
            continue;
        }
        let address = bt.address.to_uppercase();
        if let Some(p) = paired.iter().find(|p| p.address == address && !p.demo) {
            let name = if !bt.name.is_empty() { &bt.name } else { &bt.device_name };
            out.push(Info { address, model: p.model.clone(), name: name.clone() });
        }
    }
    out
}

fn select_options(entry: Option<&Json>) -> Vec<&str> {
    match entry.and_then(|e| js::field(e, "setting")).and_then(|s| js::field(s, "options")) {
        Some(Json::Array(options)) => options.iter().filter_map(Json::as_str).collect(),
        _ => Vec::new(),
    }
}

fn type_is(entry: Option<&Json>, ty: &str) -> bool {
    entry.and_then(|e| js::field(e, "type")).and_then(Json::as_str) == Some(ty)
}

/// What this adapter drives out of `list-settings`: the offered options
/// (`modes`, `presets`), the setting ids to `--get`, and which battery
/// settings the model has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caps {
    pub modes: Vec<ControlOption>,
    pub presets: Vec<ControlOption>,
    pub ids: Vec<String>,
    pub single: bool,
    pub dual: bool,
    pub has_case: bool,
}

/// `list-settings --no-categories --json`: `{ <settingId>: { type, setting? } }`.
/// `None` when the text is not that object.
pub fn parse_capabilities(text: &str) -> Option<Caps> {
    let raw = json(text)?;
    if !raw.is_object() {
        return None;
    }
    let mut caps = Caps::default();
    let ambient = js::field(&raw, "ambientSoundMode");
    if type_is(ambient, "select") {
        let options = select_options(ambient);
        caps.modes = MODES
            .iter()
            .filter(|(v, _, _)| options.contains(v))
            .map(|(v, label, icon)| ControlOption::new(*v, label, icon))
            .collect();
        if !caps.modes.is_empty() {
            caps.ids.push("ambientSoundMode".into());
        }
    }
    let preset = js::field(&raw, "presetEqualizerProfile");
    if type_is(preset, "optionalSelect") {
        let options = select_options(preset);
        caps.presets = PRESETS
            .iter()
            .filter(|(v, _)| options.contains(v))
            .map(|(v, label)| ControlOption::new(*v, label, ""))
            .collect();
        if !caps.presets.is_empty() {
            caps.ids.push("presetEqualizerProfile".into());
        }
    }
    for id in INFORMATION_IDS {
        if type_is(js::field(&raw, id), "information") {
            caps.ids.push(id.into());
        }
    }
    let has = |id: &str| js::field(&raw, id).is_some();
    caps.single = has("batteryLevel");
    caps.dual = has("batteryLevelLeft") || has("batteryLevelRight");
    caps.has_case = has("caseBatteryLevel");
    Some(caps)
}

pub fn paired_argv() -> Vec<String> {
    [BINARY, "paired-devices", "list", "--json"].map(String::from).to_vec()
}

pub fn settings_argv(address: &str) -> Option<Vec<String>> {
    valid_address(address).then(|| {
        [BINARY, "device", "-a", address, "list-settings", "--no-categories", "--json"].map(String::from).to_vec()
    })
}

/// `setting ... --json`: the gets, in order, then the sets. A failing one
/// exits 1 after printing whatever was collected before it.
pub fn setting_argv(address: &str, tail: &[String]) -> Option<Vec<String>> {
    valid_address(address).then(|| {
        let mut argv: Vec<String> = [BINARY, "device", "-a", address, "setting"].map(String::from).to_vec();
        argv.extend(tail.iter().cloned());
        argv.push("--json".into());
        argv
    })
}

pub fn get_args(caps: &Caps) -> Vec<String> {
    caps.ids.iter().flat_map(|id| ["--get".to_string(), id.clone()]).collect()
}

/// `{ settingId: value }` out of a `setting --get ... --json` read.
pub type Values = BTreeMap<String, Json>;

/// `setting --get ... --json`: `[{ settingId, value: { type, value } }]`.
/// Empty on anything else.
pub fn parse_values(text: &str) -> Values {
    let mut out = Values::new();
    let Some(Json::Array(rows)) = json(text) else { return out };
    for row in &rows {
        if let (Some(Json::String(id)), Some(value)) = (js::field(row, "settingId"), js::field(row, "value"))
            && js::is_object_like(value)
        {
            out.insert(id.clone(), js::field(value, "value").cloned().unwrap_or(Json::Null));
        }
    }
    out
}

static LEVEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([0-9]+)/([0-9]+)$").unwrap());

/// A battery information setting reads "<step>/<max>", "4/5". openscq30
/// rounds down when it prints the percentage, so this does too. -1 when
/// unknown.
fn level(value: &Json) -> f64 {
    let text = js::to_str(value);
    let Some(m) = LEVEL.captures(&text) else { return -1.0 };
    let (step, max) = (js::parse_number(&m[1]), js::parse_number(&m[2]));
    if max == 0.0 {
        return -1.0;
    }
    (step * 100.0 / max).floor().min(100.0)
}

fn batteries(values: &Values) -> Vec<Battery> {
    let mut out = Vec::new();
    let mut add = |id: BatteryId, label: &str, level_id: &str, charging_id: &str| {
        let lvl = values.get(level_id).map_or(-1.0, level);
        if lvl < 0.0 {
            return;
        }
        let charging = values.get(charging_id).and_then(Json::as_str) == Some("Yes");
        out.push(Battery::new(id, label, lvl, charging, None));
    };
    add(BatteryId::Left, "Left", "batteryLevelLeft", "isChargingLeft");
    add(BatteryId::Right, "Right", "batteryLevelRight", "isChargingRight");
    add(BatteryId::Case, "Case", "caseBatteryLevel", "");
    add(BatteryId::Single, "Battery", "batteryLevel", "isCharging");
    out
}

/// The option whose value is `value`, strictly.
fn choose<'a>(options: &'a [ControlOption], value: Option<&Json>) -> Option<&'a str> {
    let wanted = value?.as_str()?;
    options.iter().find_map(|o| o.value.as_str().filter(|v| *v == wanted))
}

fn mode_line(mode: &str) -> &'static str {
    match mode {
        "Normal" => "Normal",
        "NoiseCanceling" => "Noise cancellation",
        _ => "Transparency",
    }
}

fn selected(value: Option<&str>) -> Value {
    value.map_or(Value::Null, Value::str)
}

/// `values` may be empty when the last read failed, which lists the device
/// with no controls rather than hiding one BlueZ says is connected.
pub fn normalise(info: &Info, caps: &Caps, values: &Values) -> Device {
    let mut controls = Vec::new();
    let mut mode = None;
    if !caps.modes.is_empty() && values.contains_key("ambientSoundMode") {
        mode = choose(&caps.modes, values.get("ambientSoundMode"));
        controls.push(choice("mode", "Listening mode", "Listening mode", selected(mode), caps.modes.clone()));
    }
    if !caps.presets.is_empty() && values.contains_key("presetEqualizerProfile") {
        let preset = choose(&caps.presets, values.get("presetEqualizerProfile"));
        controls.push(choice("eq", "Equalizer preset", "Equalizer", selected(preset), caps.presets.clone()));
    }
    device(DeviceFields {
        key: format!("{BACKEND}:{}", info.address),
        backend: BACKEND.into(),
        address: info.address.clone(),
        name: if info.name.is_empty() { "Soundcore".into() } else { info.name.clone() },
        kind: if caps.single && !caps.dual { DeviceKind::Headphone } else { DeviceKind::Earbuds },
        connected: true,
        batteries: batteries(values),
        controls,
        state_line: mode.map_or("", mode_line).to_string(),
        art: String::new(),
    })
}

/// The `--set` arguments for one control, or `None` for anything outside what
/// the model offers. `caps` is that device's own capabilities.
pub fn command(control_key: &str, value: &Value, caps: Option<&Caps>) -> Option<Vec<String>> {
    let caps = caps?;
    let word = value.as_str()?;
    let offered = |options: &[ControlOption]| options.iter().any(|o| o.value.as_str() == Some(word));
    match control_key {
        "mode" if offered(&caps.modes) => Some(vec!["--set".into(), format!("ambientSoundMode={word}")]),
        "eq" if offered(&caps.presets) => Some(vec!["--set".into(), format!("presetEqualizerProfile={word}")]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::earbuds::{battery_rows, control, sections, worst_level};
    use serde_json::json;

    // Fixtures are stdout captured from a real openscq30 2.12.0 binary (nix
    // build of github.com/Oppzippy/OpenSCQ30 v2.12.0, run on the aarch64 test
    // VM against demo devices, `paired-devices add --demo`), so the shapes are
    // the CLI's own. The list-settings ones are trimmed to the settings this
    // adapter reads; battery levels are the demo's 0/5. Demo devices:
    // 00:00:00:00:00:01 SoundcoreA3027 (Life Q35, one battery),
    // 00:00:00:00:00:03 SoundcoreA3947 (Liberty 4 NC, earbuds with a case).
    const PAIRED: &str = r#"[{"macAddress":"00:00:00:00:00:01","model":"SoundcoreA3027","isDemo":true},{"macAddress":"AC:12:2F:11:22:33","model":"SoundcoreA3947","isDemo":false},{"macAddress":"AC:12:2F:44:55:66","model":"SoundcoreA3027","isDemo":false}]"#;

    const PRESET_SETTING: &str = r#"{"type":"optionalSelect","setting":{"options":["SoundcoreSignature","Acoustic","BassBooster","BassReducer","Classical","Podcast","Dance","Deep","Electronic","Flat","HipHop","Jazz","Latin","Lounge","Piano","Pop","RnB","Rock","SmallSpeakers","SpokenWord","TrebleBooster","TrebleReducer"],"localizedOptions":["Soundcore Signature","Acoustic","Bass Booster","Bass Reducer","Classical","Podcast","Dance","Deep","Electronic","Flat","Hip Hop","Jazz","Latin","Lounge","Piano","Pop","RnB","Rock","Small Speakers","Spoken Word","Treble Booster","Treble Reducer"]}}"#;

    fn settings_headphone() -> String {
        format!(
            r#"{{"ambientSoundMode":{{"type":"select","setting":{{"options":["Normal","Transparency","NoiseCanceling"],"localizedOptions":["Normal","Transparency","Noise Canceling"]}}}},"noiseCancelingMode":{{"type":"select","setting":{{"options":["Transport","Indoor","Outdoor"],"localizedOptions":["Transport","Indoor","Outdoor"]}}}},"presetEqualizerProfile":{PRESET_SETTING},"isCharging":{{"type":"information"}},"batteryLevel":{{"type":"information"}},"wearingDetection":{{"type":"toggle"}}}}"#
        )
    }

    fn settings_earbuds() -> String {
        format!(
            r#"{{"ambientSoundMode":{{"type":"select","setting":{{"options":["NoiseCanceling","Transparency","Normal"],"localizedOptions":["Noise Canceling","Transparency","Normal"]}}}},"transparencyMode":{{"type":"select","setting":{{"options":["FullyTransparent","VocalMode"],"localizedOptions":["Fully Transparent","Vocal Mode"]}}}},"presetEqualizerProfile":{PRESET_SETTING},"isChargingLeft":{{"type":"information"}},"isChargingRight":{{"type":"information"}},"batteryLevelLeft":{{"type":"information"}},"batteryLevelRight":{{"type":"information"}},"caseBatteryLevel":{{"type":"information"}}}}"#
        )
    }

    const VALUES_HEADPHONE: &str = r#"[{"settingId":"ambientSoundMode","value":{"type":"string","value":"NoiseCanceling"}},{"settingId":"presetEqualizerProfile","value":{"type":"optionalString","value":"SoundcoreSignature"}},{"settingId":"batteryLevel","value":{"type":"string","value":"0/5"}},{"settingId":"isCharging","value":{"type":"string","value":"No"}}]"#;

    const VALUES_EARBUDS: &str = r#"[{"settingId":"ambientSoundMode","value":{"type":"string","value":"NoiseCanceling"}},{"settingId":"presetEqualizerProfile","value":{"type":"optionalString","value":"SoundcoreSignature"}},{"settingId":"batteryLevelLeft","value":{"type":"string","value":"0/5"}},{"settingId":"batteryLevelRight","value":{"type":"string","value":"0/5"}},{"settingId":"caseBatteryLevel","value":{"type":"string","value":"0/5"}},{"settingId":"isChargingLeft","value":{"type":"string","value":"No"}},{"settingId":"isChargingRight","value":{"type":"string","value":"No"}}]"#;

    // The same shape with values a real pair would report, edited by hand: 4/5
    // and 3/5 steps, the right bud charging, a preset outside the row.
    const VALUES_EDITED: &str = r#"[{"settingId":"ambientSoundMode","value":{"type":"string","value":"Transparency"}},{"settingId":"presetEqualizerProfile","value":{"type":"optionalString","value":"Jazz"}},{"settingId":"batteryLevelLeft","value":{"type":"string","value":"4/5"}},{"settingId":"batteryLevelRight","value":{"type":"string","value":"3/5"}},{"settingId":"caseBatteryLevel","value":{"type":"string","value":"5/5"}},{"settingId":"isChargingLeft","value":{"type":"string","value":"No"}},{"settingId":"isChargingRight","value":{"type":"string","value":"Yes"}}]"#;

    fn keys<T>(list: &[T], f: impl Fn(&T) -> String) -> String {
        list.iter().map(f).collect::<Vec<_>>().join(",")
    }

    fn bt(address: &str, name: &str, connected: bool) -> bluetooth::Device {
        bluetooth::Device { address: address.into(), name: name.into(), device_name: name.into(), connected, ..Default::default() }
    }

    fn info(address: &str, model: &str, name: &str) -> Info {
        Info { address: address.into(), model: model.into(), name: name.into() }
    }

    fn option_values(options: &[ControlOption]) -> String {
        keys(options, |o| o.value.as_str().unwrap().into())
    }

    #[test]
    fn paired_list_parses_and_skips_junk() {
        let p = parse_paired(PAIRED);
        assert_eq!(p.len(), 3);
        assert!(p[0].demo);
        assert_eq!(p[1].address, "AC:12:2F:11:22:33");
        assert_eq!(p[1].model, "SoundcoreA3947");
        assert_eq!(parse_paired("").len(), 0);
        assert_eq!(parse_paired("Error: no session").len(), 0);
        assert_eq!(parse_paired(r#"{"macAddress":"AC:12:2F:11:22:33"}"#).len(), 0);
        assert_eq!(parse_paired(r#"[{"macAddress":"; rm -rf","model":"x"}]"#).len(), 0);
    }

    #[test]
    fn only_connected_non_demo_paired_devices_are_listed() {
        let p = parse_paired(PAIRED);
        let found = connected_paired(
            &p,
            &[
                bt("ac:12:2f:11:22:33", "Liberty 4 NC", true),
                bt("AC:12:2F:44:55:66", "Life Q35", false),
                bt("00:00:00:00:00:01", "Demo", true),
                bt("11:22:33:44:55:66", "Unknown speaker", true),
            ],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].address, "AC:12:2F:11:22:33");
        assert_eq!(found[0].model, "SoundcoreA3947");
        assert_eq!(found[0].name, "Liberty 4 NC");
        assert_eq!(connected_paired(&p, &[]).len(), 0);
    }

    #[test]
    fn capabilities_headphone() {
        let caps = parse_capabilities(&settings_headphone()).unwrap();
        assert_eq!(option_values(&caps.modes), "Normal,NoiseCanceling,Transparency");
        assert_eq!(option_values(&caps.presets), "SoundcoreSignature,BassBooster,TrebleBooster,Podcast");
        assert_eq!(caps.ids.join(","), "ambientSoundMode,presetEqualizerProfile,batteryLevel,isCharging");
        assert!(caps.single);
        assert!(!caps.dual);
        assert_eq!(
            get_args(&caps).join(" "),
            "--get ambientSoundMode --get presetEqualizerProfile --get batteryLevel --get isCharging"
        );
    }

    #[test]
    fn capabilities_earbuds() {
        let caps = parse_capabilities(&settings_earbuds()).unwrap();
        assert!(caps.dual);
        assert!(!caps.single);
        assert!(caps.has_case);
        assert_eq!(
            caps.ids.join(","),
            "ambientSoundMode,presetEqualizerProfile,batteryLevelLeft,isChargingLeft,batteryLevelRight,isChargingRight,caseBatteryLevel"
        );
    }

    #[test]
    fn capabilities_reject_non_objects() {
        assert_eq!(parse_capabilities(""), None);
        assert_eq!(parse_capabilities("[]"), None);
        let bare = parse_capabilities("{}").unwrap();
        assert_eq!(bare.ids.len(), 0);
    }

    #[test]
    fn values_parse() {
        let v = parse_values(VALUES_EARBUDS);
        assert_eq!(v["ambientSoundMode"], json!("NoiseCanceling"));
        assert_eq!(v["batteryLevelLeft"], json!("0/5"));
        assert_eq!(v["isChargingRight"], json!("No"));
        assert_eq!(parse_values("[]").len(), 0);
        assert_eq!(parse_values("Error: nope").len(), 0);
    }

    #[test]
    fn headphone_device() {
        let caps = parse_capabilities(&settings_headphone()).unwrap();
        let d = normalise(&info("AC:12:2F:44:55:66", "SoundcoreA3027", "Life Q35"), &caps, &parse_values(VALUES_HEADPHONE));
        assert_eq!(d.key, "soundcore:AC:12:2F:44:55:66");
        assert_eq!(d.backend, "soundcore");
        assert_eq!(d.kind.as_str(), "headphone");
        assert_eq!(d.name, "Life Q35");
        assert!(d.connected);
        assert_eq!(d.state_line, "Noise cancellation");
        let rows = battery_rows(Some(&d));
        assert_eq!(keys(&rows, |r| r.key.as_str().into()), "single");
        assert_eq!(rows[0].level, 0.0);
        assert_eq!(keys(&d.controls, |c| c.key.clone()), "mode,eq");
        assert_eq!(keys(&sections(Some(&d)), |s| s.section.clone()), "Listening mode,Equalizer");
        let mode = control(Some(&d), "mode").unwrap();
        assert_eq!(mode.kind, crate::earbuds::ControlKind::Choice);
        assert_eq!(option_values(mode.options.as_ref().unwrap()), "Normal,NoiseCanceling,Transparency");
        assert_eq!(keys(mode.options.as_ref().unwrap(), |o| o.icon.clone()), "circle-off,ear-off,ear");
        assert_eq!(mode.value, "NoiseCanceling".into());
        assert_eq!(control(Some(&d), "eq").unwrap().value, "SoundcoreSignature".into());
    }

    #[test]
    fn earbuds_device_with_levels() {
        let caps = parse_capabilities(&settings_earbuds()).unwrap();
        let d = normalise(&info("AC:12:2F:11:22:33", "SoundcoreA3947", "Liberty 4 NC"), &caps, &parse_values(VALUES_EDITED));
        assert_eq!(d.kind.as_str(), "earbuds");
        assert_eq!(d.state_line, "Transparency");
        let rows = battery_rows(Some(&d));
        assert_eq!(keys(&rows, |r| r.key.as_str().into()), "left,right,case");
        assert_eq!(rows[0].level, 80.0);
        assert_eq!(rows[1].level, 60.0);
        assert_eq!(rows[1].hint, "Charging");
        assert_eq!(rows[2].level, 100.0);
        assert_eq!(worst_level(Some(&d)), 60.0);
        assert_eq!(control(Some(&d), "mode").unwrap().value, "Transparency".into());
        // Jazz is a real preset the row has no button for: nothing lit.
        assert_eq!(control(Some(&d), "eq").unwrap().value, Value::Null);
    }

    #[test]
    fn failed_read_lists_device_without_controls() {
        let caps = parse_capabilities(&settings_earbuds()).unwrap();
        let d = normalise(&info("AC:12:2F:11:22:33", "SoundcoreA3947", ""), &caps, &Values::new());
        assert_eq!(d.name, "Soundcore");
        assert_eq!(d.controls.len(), 0);
        assert_eq!(d.batteries.len(), 0);
        assert_eq!(d.state_line, "");
    }

    #[test]
    fn unknown_battery_text_is_dropped() {
        let caps = parse_capabilities(&settings_headphone()).unwrap();
        let q = info("AC:12:2F:44:55:66", "SoundcoreA3027", "Q");
        let values = Values::from([("batteryLevel".to_string(), json!("n/a")), ("isCharging".to_string(), json!("No"))]);
        assert_eq!(normalise(&q, &caps, &values).batteries.len(), 0);
        let values = Values::from([("batteryLevel".to_string(), json!("1/0"))]);
        assert_eq!(normalise(&q, &caps, &values).batteries.len(), 0);
    }

    #[test]
    fn command_allow_list() {
        let caps = parse_capabilities(&settings_earbuds()).unwrap();
        let cmd = |k: &str, v: &str, caps: Option<&Caps>| command(k, &v.into(), caps);
        assert_eq!(cmd("mode", "NoiseCanceling", Some(&caps)).unwrap().join(" "), "--set ambientSoundMode=NoiseCanceling");
        assert_eq!(cmd("eq", "BassBooster", Some(&caps)).unwrap().join(" "), "--set presetEqualizerProfile=BassBooster");
        assert_eq!(cmd("mode", "AirplaneMode", Some(&caps)), None);
        assert_eq!(cmd("mode", "Normal; reboot", Some(&caps)), None);
        assert_eq!(cmd("eq", "Jazz", Some(&caps)), None);
        assert_eq!(cmd("battery", "x", Some(&caps)), None);
        assert_eq!(cmd("mode", "Normal", None), None);
        // A model whose select lacks a mode refuses it.
        let caps2 = parse_capabilities(r#"{"ambientSoundMode":{"type":"select","setting":{"options":["Normal","NoiseCanceling"]}}}"#).unwrap();
        assert_eq!(cmd("mode", "Transparency", Some(&caps2)), None);
        assert_eq!(cmd("mode", "Normal", Some(&caps2)).unwrap().len(), 2);
    }

    #[test]
    fn argv_builders_refuse_a_bad_address() {
        assert_eq!(paired_argv().join(" "), "openscq30 paired-devices list --json");
        assert_eq!(
            settings_argv("AC:12:2F:11:22:33").unwrap().join(" "),
            "openscq30 device -a AC:12:2F:11:22:33 list-settings --no-categories --json"
        );
        let tail = ["--set".to_string(), "ambientSoundMode=Normal".to_string()];
        assert_eq!(
            setting_argv("AC:12:2F:11:22:33", &tail).unwrap().join(" "),
            "openscq30 device -a AC:12:2F:11:22:33 setting --set ambientSoundMode=Normal --json"
        );
        assert_eq!(setting_argv("--help", &["--get".into(), "x".into()]), None);
        assert_eq!(settings_argv(""), None);
    }
}
