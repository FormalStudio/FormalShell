//! The one device shape every earbuds backend normalises into (spec
//! `docs/superpowers/specs/2026-09-30-m77-earbuds.md`), and the helpers the
//! panel, bar cell and IPC read it through. Ported from
//! `shell/Earbuds/model.js`.
//!
//! A capability the device lacks is absent from `controls`, and an adapter
//! lists a battery only once its level is known.

use serde_json::Value as Json;

use crate::bluetooth;
use fs_js as js;

pub mod airpods;
pub mod nothing;
pub mod samsung;
pub mod soundcore;

/// A control's value or an IPC argument: the JS models compare and coerce
/// these loosely, so the type keeps every shape a caller can hand over.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    /// An array or object, which no control accepts.
    Other,
}

impl Value {
    pub fn str(s: &str) -> Value {
        Value::Str(s.to_string())
    }

    pub fn from_json(v: &Json) -> Value {
        match v {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => Value::Num(n.as_f64().unwrap_or(f64::NAN)),
            Json::String(s) => Value::Str(s.clone()),
            Json::Array(_) | Json::Object(_) => Value::Other,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    /// `String(value)`.
    fn js_string(&self) -> String {
        match self {
            Value::Null => "null".into(),
            Value::Bool(b) => b.to_string(),
            Value::Num(n) => js::num_str(*n),
            Value::Str(s) => s.clone(),
            Value::Other => "[object Object]".into(),
        }
    }

    /// `Number(value)`.
    fn js_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Num(n) => *n,
            Value::Str(s) => js::parse_number(s),
            Value::Other => f64::NAN,
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        Value::Str(s.to_string())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Value {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Value {
        Value::Num(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Value {
        Value::Num(f64::from(n))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeviceKind {
    #[default]
    Earbuds,
    Headphone,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DeviceKind::Earbuds => "earbuds",
            DeviceKind::Headphone => "headphone",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryId {
    Left,
    Right,
    Case,
    Single,
}

impl BatteryId {
    pub fn as_str(self) -> &'static str {
        match self {
            BatteryId::Left => "left",
            BatteryId::Right => "right",
            BatteryId::Case => "case",
            BatteryId::Single => "single",
        }
    }

    pub fn parse(s: &str) -> Option<BatteryId> {
        match s {
            "left" => Some(BatteryId::Left),
            "right" => Some(BatteryId::Right),
            "case" => Some(BatteryId::Case),
            "single" => Some(BatteryId::Single),
            _ => None,
        }
    }
}

/// `in_ear` is `None` for a component with no in-ear sensor.
#[derive(Clone, Debug, PartialEq)]
pub struct Battery {
    pub id: BatteryId,
    pub label: String,
    pub level: f64,
    pub charging: bool,
    pub in_ear: Option<bool>,
}

impl Battery {
    pub fn new(id: BatteryId, label: &str, level: f64, charging: bool, in_ear: Option<bool>) -> Battery {
        Battery { id, label: label.into(), level, charging, in_ear }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Choice,
    Toggle,
    Range,
}

/// How a range reads beside its track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// Not a range.
    None,
    /// The position in its span.
    Percent,
    /// The value itself as signed decibels.
    Db,
}

impl Unit {
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::None => "",
            Unit::Percent => "percent",
            Unit::Db => "db",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlOption {
    pub value: Value,
    pub label: String,
    pub icon: String,
    /// The name an option shares with its neighbours in one tier (Nothing's
    /// four noise cancelling strengths), "" for an option standing alone.
    /// The panel shows a tier as one choice and its members under it.
    pub tier: String,
}

impl ControlOption {
    pub fn new(value: impl Into<Value>, label: &str, icon: &str) -> ControlOption {
        ControlOption { value: value.into(), label: label.into(), icon: icon.into(), tier: String::new() }
    }

    pub fn tier(mut self, name: &str) -> ControlOption {
        self.tier = name.into();
        self
    }
}

/// `hint` is the dim second line under a toggle's label, "" for none.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub key: String,
    pub kind: ControlKind,
    pub label: String,
    pub hint: String,
    pub section: String,
    pub value: Value,
    pub enabled: bool,
    pub options: Option<Vec<ControlOption>>,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: Unit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub key: String,
    pub backend: String,
    pub address: String,
    pub name: String,
    pub kind: DeviceKind,
    pub connected: bool,
    pub batteries: Vec<Battery>,
    pub controls: Vec<Control>,
    pub state_line: String,
    /// The product picture set, `<set>_<part>.webp` under `FS_EARBUDS_ART`
    /// (nix/earbuds-art.py), "" for a device with none.
    pub art: String,
}

/// The fields a backend fills in; everything else defaults.
#[derive(Clone, Debug, Default)]
pub struct DeviceFields {
    pub key: String,
    pub backend: String,
    pub address: String,
    pub name: String,
    pub kind: DeviceKind,
    pub connected: bool,
    pub batteries: Vec<Battery>,
    pub controls: Vec<Control>,
    pub state_line: String,
    pub art: String,
}

pub fn device(fields: DeviceFields) -> Device {
    Device {
        key: fields.key,
        backend: fields.backend,
        address: fields.address,
        name: fields.name,
        kind: fields.kind,
        connected: fields.connected,
        batteries: fields.batteries,
        controls: fields.controls,
        state_line: fields.state_line,
        art: fields.art,
    }
}

pub fn choice(key: &str, label: &str, section: &str, value: Value, options: Vec<ControlOption>) -> Control {
    Control {
        key: key.into(),
        kind: ControlKind::Choice,
        label: label.into(),
        hint: String::new(),
        section: section.into(),
        value,
        enabled: true,
        options: Some(options),
        min: 0.0,
        max: 0.0,
        step: 0.0,
        unit: Unit::None,
    }
}

pub fn toggle(key: &str, label: &str, hint: &str, section: &str, value: bool) -> Control {
    Control {
        key: key.into(),
        kind: ControlKind::Toggle,
        label: label.into(),
        hint: hint.into(),
        section: section.into(),
        value: Value::Bool(value),
        enabled: true,
        options: None,
        min: 0.0,
        max: 0.0,
        step: 0.0,
        unit: Unit::None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn range(key: &str, label: &str, section: &str, value: f64, min: f64, max: f64, step: f64, unit: Unit) -> Control {
    Control {
        key: key.into(),
        kind: ControlKind::Range,
        label: label.into(),
        hint: String::new(),
        section: section.into(),
        value: Value::Num(value),
        enabled: true,
        options: None,
        min,
        max,
        step,
        unit: if unit == Unit::Db { Unit::Db } else { Unit::Percent },
    }
}

/// In ear and Charging fused " / ".
fn hint(battery: &Battery) -> String {
    let mut parts = Vec::new();
    if battery.in_ear == Some(true) {
        parts.push("In ear");
    }
    if battery.charging {
        parts.push("Charging");
    }
    parts.join(" / ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct BatteryRow {
    pub key: BatteryId,
    pub label: String,
    pub level: f64,
    pub hint: String,
}

pub fn battery_rows(dev: Option<&Device>) -> Vec<BatteryRow> {
    let Some(dev) = dev else { return Vec::new() };
    dev.batteries
        .iter()
        .map(|b| BatteryRow { key: b.id, label: b.label.clone(), level: b.level, hint: hint(b) })
        .collect()
}

/// The bar cell's headline: the worst bud (or the one battery a headphone
/// has). The case is left out, a full case beside a near-dead bud would read
/// backwards. -1 when no bud has a level.
pub fn worst_level(dev: Option<&Device>) -> f64 {
    let Some(dev) = dev else { return -1.0 };
    let mut worst = -1.0;
    for b in &dev.batteries {
        if b.id == BatteryId::Case {
            continue;
        }
        if worst < 0.0 || b.level < worst {
            worst = b.level;
        }
    }
    worst
}

pub fn battery_summary(dev: Option<&Device>) -> String {
    let Some(dev) = dev else { return String::new() };
    dev.batteries
        .iter()
        .map(|b| {
            let tag = match b.id {
                BatteryId::Left => "L",
                BatteryId::Right => "R",
                BatteryId::Case => "CASE",
                BatteryId::Single => "",
            };
            let level = js::num_str(b.level);
            if tag.is_empty() { level } else { format!("{tag} {level}") }
        })
        .collect::<Vec<_>>()
        .join(" / ")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Section<'a> {
    pub section: String,
    pub controls: Vec<&'a Control>,
}

/// Controls grouped by `section` in first-appearance order, the order the
/// panel draws its section labels in.
pub fn sections(dev: Option<&Device>) -> Vec<Section<'_>> {
    let mut out: Vec<Section> = Vec::new();
    let Some(dev) = dev else { return out };
    for c in &dev.controls {
        match out.iter_mut().find(|s| s.section == c.section) {
            Some(s) => s.controls.push(c),
            None => out.push(Section { section: c.section.clone(), controls: vec![c] }),
        }
    }
    out
}

pub fn control<'a>(dev: Option<&'a Device>, key: &str) -> Option<&'a Control> {
    dev?.controls.iter().find(|c| c.key == key)
}

pub fn option_index(ctl: Option<&Control>) -> Option<usize> {
    let ctl = ctl?;
    ctl.options.as_ref()?.iter().position(|o| o.value == ctl.value)
}

/// `raw` is whatever a caller has, an IPC string included. Returns the value
/// in the control's own type, or `None` when it is not one the control
/// accepts: a choice outside its options, a toggle that is not on/off, a
/// range that is not a number. A range value is clamped, and rounded when
/// `step` is whole. `step` is the keyboard and wheel increment, not a grid a
/// drag has to land on.
pub fn coerce(ctl: Option<&Control>, raw: &Value) -> Option<Value> {
    let ctl = ctl.filter(|c| c.enabled)?;
    match ctl.kind {
        ControlKind::Choice => {
            let wanted = raw.js_string();
            ctl.options
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|o| o.value.js_string() == wanted)
                .map(|o| o.value.clone())
        }
        ControlKind::Toggle => match raw {
            Value::Bool(true) => Some(Value::Bool(true)),
            Value::Bool(false) => Some(Value::Bool(false)),
            Value::Str(s) if s == "on" || s == "true" => Some(Value::Bool(true)),
            Value::Str(s) if s == "off" || s == "false" => Some(Value::Bool(false)),
            _ => None,
        },
        ControlKind::Range => {
            let n = match raw {
                Value::Num(n) => *n,
                other => js::parse_number(&other.js_string()),
            };
            if js::trim(&raw.js_string()).is_empty() || !n.is_finite() {
                return None;
            }
            let n = n.min(ctl.max).max(ctl.min);
            Some(Value::Num(if js::round(ctl.step) == ctl.step { js::round(n) } else { n }))
        }
    }
}

/// The range's position as a whole percent of its span, what the panel
/// prints beside the track.
pub fn range_percent(ctl: Option<&Control>) -> f64 {
    match ctl {
        Some(c) if c.max > c.min => js::round((c.value.js_number() - c.min) / (c.max - c.min) * 100.0),
        _ => 0.0,
    }
}

/// How far along its span the value sits, 0..1, for the track's fill: min is
/// empty and max full whatever sign either has, so a -6..6 band at 0 is half
/// full.
pub fn range_fraction(ctl: Option<&Control>) -> f64 {
    match ctl {
        Some(c) if c.max > c.min => ((c.value.js_number() - c.min) / (c.max - c.min)).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// The readout beside the track. Percent: "40%". Db: the value signed,
/// "+5 dB", "0 dB", "-2 dB", whole when `step` is, with a no-break space so
/// the unit never wraps off its number.
pub fn range_text(ctl: Option<&Control>) -> String {
    let Some(c) = ctl else { return String::new() };
    if c.unit != Unit::Db {
        return format!("{}%", js::num_str(range_percent(ctl)));
    }
    let value = c.value.js_number();
    let v = if js::round(c.step) == c.step { js::round(value) } else { js::round(value * 10.0) / 10.0 };
    format!("{}{}\u{a0}dB", if v > 0.0 { "+" } else { "" }, js::num_str(v))
}

/// `preferred_key`: the key the user last selected or the device that last
/// connected. Falls back to the first connected device, then the first.
pub fn pick_active<'a>(devices: &'a [Device], preferred_key: &str) -> Option<&'a Device> {
    devices
        .iter()
        .find(|d| d.key == preferred_key)
        .or_else(|| devices.iter().find(|d| d.connected))
        .or_else(|| devices.first())
}

/// `values`: the BlueZ devices, or empty with no adapter. `override_json`:
/// FORMALSHELL_SMOKE_BLUETOOTH, a JSON array of `{ address, name,
/// connected }` that replaces them outright. It exists for the smoke rig,
/// whose VM has no Bluetooth controller, so a backend that crosses its CLI's
/// list with BlueZ's connected devices can still be driven. Unset or
/// unparsable, the adapter is the answer. Every address comes back upper case.
pub fn bluetooth_devices(values: &[bluetooth::Device], override_json: Option<&str>) -> Vec<bluetooth::Device> {
    if let Some(text) = override_json.filter(|t| !t.is_empty())
        && let Ok(Json::Array(raw)) = serde_json::from_str::<Json>(text)
    {
        return raw
            .iter()
            .filter_map(|d| {
                let address = js::field(d, "address")?.as_str()?;
                let name = js::field(d, "name").and_then(Json::as_str).unwrap_or("").to_string();
                Some(bluetooth::Device {
                    address: address.to_uppercase(),
                    device_name: name.clone(),
                    name,
                    connected: js::field(d, "connected") == Some(&Json::Bool(true)),
                    ..Default::default()
                })
            })
            .collect();
    }
    values
        .iter()
        .map(|v| bluetooth::Device {
            address: v.address.to_uppercase(),
            name: v.name.clone(),
            device_name: v.device_name.clone(),
            connected: v.connected,
            ..Default::default()
        })
        .collect()
}

/// A number as `JSON.stringify` prints it: whole values without a fraction,
/// NaN and the infinities as null.
fn js_number(n: f64) -> Json {
    if !n.is_finite() {
        Json::Null
    } else if n == n.trunc() && n.abs() < 9_007_199_254_740_992.0 {
        Json::from(n as i64)
    } else {
        Json::from(n)
    }
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Null | Value::Other => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Num(n) => js_number(*n),
        Value::Str(s) => Json::String(s.clone()),
    }
}

impl ControlKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ControlKind::Choice => "choice",
            ControlKind::Toggle => "toggle",
            ControlKind::Range => "range",
        }
    }
}

/// The device as `JSON.stringify` prints model.js's object, key for key.
pub fn to_json(dev: &Device) -> Json {
    let batteries: Vec<Json> = dev
        .batteries
        .iter()
        .map(|b| {
            serde_json::json!({
                "id": b.id.as_str(),
                "label": b.label,
                "level": js_number(b.level),
                "charging": b.charging,
                "inEar": b.in_ear,
            })
        })
        .collect();
    let controls: Vec<Json> = dev
        .controls
        .iter()
        .map(|c| {
            let options = c.options.as_ref().map(|list| {
                list.iter()
                    .map(|o| serde_json::json!({"value": value_json(&o.value), "label": o.label, "icon": o.icon}))
                    .collect::<Vec<Json>>()
            });
            serde_json::json!({
                "key": c.key,
                "kind": c.kind.as_str(),
                "label": c.label,
                "hint": c.hint,
                "section": c.section,
                "value": value_json(&c.value),
                "enabled": c.enabled,
                "options": options,
                "min": js_number(c.min),
                "max": js_number(c.max),
                "step": js_number(c.step),
                "unit": c.unit.as_str(),
            })
        })
        .collect();
    serde_json::json!({
        "key": dev.key,
        "backend": dev.backend,
        "address": dev.address,
        "name": dev.name,
        "kind": dev.kind.as_str(),
        "connected": dev.connected,
        "batteries": batteries,
        "controls": controls,
        "stateLine": dev.state_line,
    })
}

/// `^[0-9A-Fa-f]{2}(:[0-9A-Fa-f]{2}){5}$`, the only shape any adapter lets
/// reach a child process's argv.
pub fn valid_address(address: &str) -> bool {
    let b = address.as_bytes();
    b.len() == 17
        && b.iter().enumerate().all(|(i, c)| if i % 3 == 2 { *c == b':' } else { c.is_ascii_hexdigit() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buds(key: &str, connected: bool, batteries: Vec<Battery>) -> Device {
        device(DeviceFields {
            key: key.into(),
            backend: "test".into(),
            name: key.into(),
            connected,
            batteries,
            ..Default::default()
        })
    }

    fn bat(id: BatteryId, level: f64) -> Battery {
        Battery::new(id, "", level, false, None)
    }

    fn opt(value: &str) -> ControlOption {
        ControlOption::new(value, "", "")
    }

    #[test]
    fn to_json_prints_as_json_stringify() {
        let mut d = buds("k", true, vec![bat(BatteryId::Left, 40.0)]);
        d.controls.push(range("adaptive", "Level", "s", 40.0, 0.0, 100.0, 1.0, Unit::Percent));
        assert_eq!(
            to_json(&d).to_string(),
            r#"{"key":"k","backend":"test","address":"","name":"k","kind":"earbuds","connected":true,"batteries":[{"id":"left","label":"","level":40,"charging":false,"inEar":null}],"controls":[{"key":"adaptive","kind":"range","label":"Level","hint":"","section":"s","value":40,"enabled":true,"options":null,"min":0,"max":100,"step":1,"unit":"percent"}],"stateLine":""}"#
        );
    }

    #[test]
    fn device_fills_defaults() {
        let d = device(DeviceFields { key: "k".into(), backend: "b".into(), ..Default::default() });
        assert_eq!(d.kind.as_str(), "earbuds");
        assert!(!d.connected);
        assert_eq!(d.batteries.len(), 0);
        assert_eq!(d.controls.len(), 0);
        assert_eq!(d.state_line, "");
        let h = device(DeviceFields { key: "k".into(), backend: "b".into(), kind: DeviceKind::Headphone, ..Default::default() });
        assert_eq!(h.kind.as_str(), "headphone");
    }

    #[test]
    fn battery_hints() {
        let d = buds(
            "a",
            true,
            vec![
                Battery::new(BatteryId::Left, "Left", 50.0, true, Some(true)),
                Battery::new(BatteryId::Case, "Case", 90.0, false, None),
            ],
        );
        let rows = battery_rows(Some(&d));
        assert_eq!(rows[0].hint, "In ear / Charging");
        assert_eq!(rows[1].hint, "");
        assert_eq!(battery_rows(None).len(), 0);
    }

    #[test]
    fn worst_level_leaves_the_case_out() {
        let d = buds("a", true, vec![bat(BatteryId::Left, 70.0), bat(BatteryId::Right, 30.0), bat(BatteryId::Case, 5.0)]);
        assert_eq!(worst_level(Some(&d)), 30.0);
        assert_eq!(worst_level(Some(&buds("a", true, vec![bat(BatteryId::Single, 64.0)]))), 64.0);
        assert_eq!(worst_level(Some(&buds("a", true, vec![bat(BatteryId::Case, 5.0)]))), -1.0);
        assert_eq!(worst_level(None), -1.0);
    }

    #[test]
    fn battery_summary_reads_each_component() {
        let d = buds("a", true, vec![bat(BatteryId::Left, 70.0), bat(BatteryId::Right, 30.0), bat(BatteryId::Case, 5.0)]);
        assert_eq!(battery_summary(Some(&d)), "L 70 / R 30 / CASE 5");
        assert_eq!(battery_summary(Some(&buds("a", true, vec![bat(BatteryId::Single, 64.0)]))), "64");
    }

    #[test]
    fn sections_keep_first_appearance_order() {
        let d = device(DeviceFields {
            key: "k".into(),
            backend: "b".into(),
            controls: vec![
                choice("anc", "Noise", "Listening mode", "on".into(), vec![]),
                toggle("lowLatency", "Low latency", "", "Options", false),
                range("level", "Level", "Listening mode", 2.0, 0.0, 3.0, 1.0, Unit::Percent),
            ],
            ..Default::default()
        });
        let s = sections(Some(&d));
        assert_eq!(s.iter().map(|x| x.section.as_str()).collect::<Vec<_>>().join(","), "Listening mode,Options");
        assert_eq!(s[0].controls.iter().map(|c| c.key.as_str()).collect::<Vec<_>>().join(","), "anc,level");
    }

    #[test]
    fn coerce_choice_toggle_and_range() {
        let ch = choice("anc", "Noise", "S", "off".into(), vec![opt("off"), opt("on")]);
        assert_eq!(coerce(Some(&ch), &"on".into()), Some("on".into()));
        assert_eq!(coerce(Some(&ch), &"max".into()), None);
        assert_eq!(option_index(Some(&ch)), Some(0));

        let t = toggle("t", "T", "", "S", false);
        assert_eq!(coerce(Some(&t), &"on".into()), Some(true.into()));
        assert_eq!(coerce(Some(&t), &false.into()), Some(false.into()));
        assert_eq!(coerce(Some(&t), &"maybe".into()), None);

        let r = range("r", "R", "S", 40.0, 0.0, 100.0, 5.0, Unit::Percent);
        assert_eq!(coerce(Some(&r), &"42".into()), Some(42.0.into()));
        assert_eq!(coerce(Some(&r), &42.6.into()), Some(43.0.into()));
        assert_eq!(coerce(Some(&r), &250.0.into()), Some(100.0.into()));
        assert_eq!(coerce(Some(&r), &(-3.0).into()), Some(0.0.into()));
        assert_eq!(coerce(Some(&r), &"".into()), None);
        assert_eq!(coerce(Some(&r), &"loud".into()), None);
        assert_eq!(range_percent(Some(&r)), 40.0);
        assert_eq!(range_percent(Some(&range("r", "R", "S", 2.0, 0.0, 4.0, 1.0, Unit::Percent))), 50.0);
    }

    #[test]
    fn coerce_refuses_a_disabled_control_and_a_missing_one() {
        let mut ch = choice("anc", "Noise", "S", "off".into(), vec![opt("off")]);
        assert_eq!(coerce(None, &"off".into()), None);
        ch.enabled = false;
        assert_eq!(coerce(Some(&ch), &"off".into()), None);
    }

    #[test]
    fn coerce_choice_matches_numeric_options_by_their_string() {
        let ch = choice("ambient", "A", "S", 0.into(), vec![ControlOption::new(0, "Off", ""), ControlOption::new(1, "1", "")]);
        assert_eq!(coerce(Some(&ch), &"1".into()), Some(1.into()));
        assert_eq!(coerce(Some(&ch), &1.0.into()), Some(1.into()));
    }

    #[test]
    fn pick_active_prefers_the_key_then_a_connected_device() {
        let a = buds("a", false, vec![]);
        let b = buds("b", true, vec![]);
        let c = buds("c", true, vec![]);
        assert!(pick_active(&[], "a").is_none());
        let all = [a.clone(), b, c];
        assert_eq!(pick_active(&all, "c").unwrap().key, "c");
        assert_eq!(pick_active(&all, "gone").unwrap().key, "b");
        assert_eq!(pick_active(&[a], "").unwrap().key, "a");
    }

    #[test]
    fn range_readout_and_fill() {
        let pct = range("adaptive", "Adaptive noise", "S", 40.0, 0.0, 100.0, 5.0, Unit::Percent);
        assert_eq!(pct.unit.as_str(), "percent");
        assert_eq!(range_text(Some(&pct)), "40%");
        assert_eq!(range_fraction(Some(&pct)), 0.4);

        let band = |v: f64| range("eq-bass", "Bass", "S", v, -6.0, 6.0, 1.0, Unit::Db);
        assert_eq!(range_text(Some(&band(5.0))), "+5\u{a0}dB");
        assert_eq!(range_text(Some(&band(0.0))), "0\u{a0}dB");
        assert_eq!(range_text(Some(&band(-2.0))), "-2\u{a0}dB");
        assert_eq!(range_text(Some(&band(-0.4))), "0\u{a0}dB");
        assert_eq!(range_fraction(Some(&band(-6.0))), 0.0);
        assert_eq!(range_fraction(Some(&band(0.0))), 0.5);
        assert_eq!(range_fraction(Some(&band(6.0))), 1.0);
        assert_eq!(range_fraction(Some(&band(-3.0))), 0.25);
        assert_eq!(choice("c", "C", "S", Value::Null, vec![]).unit.as_str(), "");
    }

    #[test]
    fn a_fractional_db_step_keeps_one_decimal() {
        let mut band = range("eq", "E", "S", 1.26, -6.0, 6.0, 0.5, Unit::Db);
        assert_eq!(range_text(Some(&band)), "+1.3\u{a0}dB");
        band.value = Value::Num(-1.24);
        assert_eq!(range_text(Some(&band)), "-1.2\u{a0}dB");
    }

    #[test]
    fn bluetooth_devices_and_the_smoke_override() {
        let live = [bluetooth::Device {
            address: "aa:bb:cc:dd:ee:ff".into(),
            name: "Buds".into(),
            device_name: "Buds".into(),
            connected: true,
            ..Default::default()
        }];
        assert_eq!(bluetooth_devices(&live, Some(""))[0].address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(bluetooth_devices(&[], Some("")).len(), 0);
        assert_eq!(bluetooth_devices(&live, Some("not json"))[0].name, "Buds");
        let o = bluetooth_devices(
            &live,
            Some(r#"[{"address":"ac:12:2f:11:22:33","name":"Liberty","connected":true},{"name":"no address"}]"#),
        );
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].address, "AC:12:2F:11:22:33");
        assert_eq!(o[0].device_name, "Liberty");
        assert!(o[0].connected);
    }

    #[test]
    fn a_non_array_override_falls_through_to_the_adapter() {
        let live = [bluetooth::Device { address: "aa:bb".into(), ..Default::default() }];
        assert_eq!(bluetooth_devices(&live, Some("{}"))[0].address, "AA:BB");
    }

    #[test]
    fn valid_address_is_a_mac_and_nothing_else() {
        assert!(valid_address("AA:BB:CC:DD:EE:FF"));
        assert!(valid_address("aa:bb:cc:dd:ee:ff"));
        assert!(!valid_address("AA:BB:CC:DD:EE:FF\n"));
        assert!(!valid_address("AA:BB --dry-run"));
        assert!(!valid_address("--help"));
        assert!(!valid_address(""));
        assert!(!valid_address("AA-BB-CC-DD-EE-FF"));
    }
}
