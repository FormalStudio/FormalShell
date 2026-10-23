//! Nothing and CMF adapter for the earbuds model, over `nothingctl`
//! (github.com/FormalSnake/nothingctl v0.1.1). Field names are the CLI's own
//! serde output: the snapshot is src/protocol/state.rs `Snapshot`, a `list
//! --json` row is src/transport/mod.rs `DeviceInfo`, and `watch` prints the
//! `ack`, `error` and `disconnected` lines from src/session.rs and
//! src/main.rs. Ported from `shell/Earbuds/nothing.js`.
//!
//! Safety: nothingctl is the only thing that speaks the Nothing protocol
//! (spec "Safety (Nothing)"). The one channel into it is [`command`], which
//! returns a [`NothingVerb`], a closed enum of the five verbs `anc`, `eq`,
//! `eq-custom`, `low-latency` and `spatial`. A verb's word argument is a
//! [`Token`] (lowercase ASCII letters only) taken from the device's own
//! reported `available` list, and its gains are whole numbers inside the
//! model's range, so no value can carry a space or a line break onto
//! `watch`'s stdin. Anything else is `None` and never written.

use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value as Json;

use super::{
    Battery, BatteryId, ControlOption, Device, DeviceFields, DeviceKind, Unit, Value, choice, device, range, toggle,
    valid_address,
};
use fs_js as js;

pub const BACKEND: &str = "nothing";
pub const BINARY: &str = "nothingctl";

/// A device nothingctl refused over its model (README "Exit codes"): `watch`
/// prints an error line carrying this code, then exits with
/// [`UNSUPPORTED_EXIT`]. Nothing was sent to it, and retrying cannot change
/// the answer.
pub const UNSUPPORTED: &str = "unsupported-model";
pub const UNSUPPORTED_EXIT: i32 = 3;

/// The hero line while nothingctl's last answer to a write was an error.
pub const FAILED: &str = "Unable to apply the change";

/// The custom EQ gain range per model base, from src/protocol/model.rs
/// (`CustomEq` min_gain/max_gain). The snapshot does not carry it, so a model
/// missing here gets no custom bands.
fn custom_gain(base: &str) -> Option<(f64, f64)> {
    match base {
        "B175" => Some((-6.0, 6.0)),
        _ => None,
    }
}

const BANDS: [(&str, &str); 3] = [("eq-bass", "Bass"), ("eq-mid", "Mid"), ("eq-treble", "Treble")];

fn anc_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "off" => "Off",
        "transparency" => "Transparency",
        "high" => "High",
        "mid" => "Mid",
        "low" => "Low",
        "adaptive" => "Adaptive",
        _ => return None,
    })
}

/// The same glyphs the AirPods modes use, the three strengths as signal bars.
fn anc_icon(name: &str) -> &'static str {
    match name {
        "off" => "circle-off",
        "transparency" => "ear",
        "high" => "signal-high",
        "mid" => "signal-medium",
        "low" => "signal-low",
        "adaptive" => "audio-waveform",
        _ => "",
    }
}

fn anc_line(name: &str) -> Option<&'static str> {
    Some(match name {
        "off" => "Off",
        "transparency" => "Transparency",
        "high" => "High noise cancellation",
        "mid" => "Mid noise cancellation",
        "low" => "Low noise cancellation",
        "adaptive" => "Adaptive",
        _ => return None,
    })
}

fn eq_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "rock" => "Rock",
        "electronic" => "Electronic",
        "pop" => "Pop",
        "vocals" => "Vocals",
        "classical" => "Classical",
        "custom" => "Custom",
        _ => return None,
    })
}

fn spatial_label(name: &str) -> Option<&'static str> {
    Some(match name {
        "off" => "Off",
        "concert" => "Concert",
        "theatre" => "Theatre",
        _ => return None,
    })
}

fn battery_label(id: BatteryId) -> &'static str {
    match id {
        BatteryId::Left => "Left",
        BatteryId::Right => "Right",
        BatteryId::Case => "Case",
        BatteryId::Single => "Battery",
    }
}

/// One word of lowercase ASCII, the only shape nothingctl reports a name in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn new(word: &str) -> Option<Token> {
        (!word.is_empty() && word.bytes().all(|b| b.is_ascii_lowercase())).then(|| Token(word.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Every command the shell can send nothingctl. There is no firmware, reset,
/// rename, codec or ring verb, and this enum is the whole list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NothingVerb {
    Anc(Token),
    Eq(Token),
    Spatial(Token),
    LowLatency(bool),
    /// Bass, mid and treble gains, whole and inside the model's range.
    EqCustom([i32; 3]),
}

impl NothingVerb {
    /// The verb word at the head of the stdin line.
    pub fn word(&self) -> &'static str {
        match self {
            NothingVerb::Anc(_) => "anc",
            NothingVerb::Eq(_) => "eq",
            NothingVerb::Spatial(_) => "spatial",
            NothingVerb::LowLatency(_) => "low-latency",
            NothingVerb::EqCustom(_) => "eq-custom",
        }
    }
}

/// The `watch` stdin line, without its newline.
impl fmt::Display for NothingVerb {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NothingVerb::Anc(t) | NothingVerb::Eq(t) | NothingVerb::Spatial(t) => write!(f, "{} {}", self.word(), t.as_str()),
            NothingVerb::LowLatency(on) => write!(f, "{} {}", self.word(), if *on { "on" } else { "off" }),
            NothingVerb::EqCustom([a, b, c]) => write!(f, "{} {a} {b} {c}", self.word()),
        }
    }
}

fn json(text: &str) -> Option<Json> {
    serde_json::from_str(text).ok()
}

pub fn list_argv() -> Vec<String> {
    vec![BINARY.into(), "list".into(), "--json".into()]
}

pub fn watch_argv(address: &str) -> Option<Vec<String>> {
    valid_address(address).then(|| vec![BINARY.into(), "watch".into(), "-d".into(), address.into()])
}

/// One row of `list --json`: every paired device that exposes the Nothing
/// control service, connected or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    pub address: String,
    pub name: String,
    pub connected: bool,
}

pub fn parse_list(text: &str) -> Vec<ListEntry> {
    let Some(Json::Array(raw)) = json(text) else { return Vec::new() };
    raw.iter()
        .filter_map(|d| {
            let address = js::field(d, "address")?.as_str().filter(|a| valid_address(a))?;
            Some(ListEntry {
                address: address.to_uppercase(),
                name: js::field(d, "name").and_then(Json::as_str).unwrap_or("").to_string(),
                connected: js::field(d, "connected") == Some(&Json::Bool(true)),
            })
        })
        .collect()
}

/// A mode list: the current value (if any) and the names the device offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mode {
    pub mode: Option<String>,
    pub available: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Equalizer {
    pub preset: Option<String>,
    pub available: Vec<String>,
    pub custom: Option<[f64; 3]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub base: String,
    /// The Fast Pair model id, which tells the colourways apart; "" from a
    /// nothingctl that does not report it.
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateBattery {
    pub id: BatteryId,
    pub level: f64,
    pub charging: bool,
}

/// A device's snapshot, from a `state` line of `watch`.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    pub address: String,
    pub name: String,
    pub model: Model,
    pub battery: Vec<StateBattery>,
    pub anc: Option<Mode>,
    pub eq: Equalizer,
    pub low_latency: Option<bool>,
    pub spatial: Option<Mode>,
}

/// One stdout line of `watch`.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    State(Box<State>),
    Ack { cmd: String },
    Error { code: String, model_id: Option<String>, message: String },
    Disconnected,
}

fn names(list: Option<&Json>) -> Vec<String> {
    match list {
        Some(Json::Array(items)) => items.iter().filter_map(Json::as_str).filter(|n| Token::new(n).is_some()).map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

fn parse_mode(raw: Option<&Json>) -> Option<Mode> {
    let raw = raw.filter(|r| js::is_object_like(r))?;
    Some(Mode {
        mode: js::field(raw, "mode").and_then(Json::as_str).map(str::to_string),
        available: names(js::field(raw, "available")),
    })
}

fn parse_state(raw: &Json) -> Option<State> {
    let address = js::field(raw, "address")?.as_str().filter(|a| valid_address(a))?;
    let model = js::field(raw, "model")?;
    let base = js::field(model, "base")?.as_str()?;
    let empty = Json::Null;
    let eq = js::field(raw, "eq").filter(|e| e.is_object()).unwrap_or(&empty);
    let custom = match js::field(eq, "custom") {
        Some(Json::Array(g)) if g.len() == 3 && g.iter().all(Json::is_number) => {
            Some([g[0].as_f64()?, g[1].as_f64()?, g[2].as_f64()?])
        }
        _ => None,
    };
    let battery = match js::field(raw, "battery") {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(|b| {
                let id = BatteryId::parse(js::field(b, "id")?.as_str()?)?;
                let level = js::field(b, "level").filter(|l| l.is_number())?.as_f64()?;
                Some(StateBattery {
                    id,
                    level: js::round(level).clamp(0.0, 100.0),
                    charging: js::field(b, "charging") == Some(&Json::Bool(true)),
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    Some(State {
        address: address.to_uppercase(),
        name: js::field(raw, "name").and_then(Json::as_str).unwrap_or("").to_string(),
        model: Model {
            base: base.to_string(),
            id: js::field(model, "id").and_then(Json::as_str).unwrap_or("").to_ascii_uppercase(),
            name: js::field(model, "name").and_then(Json::as_str).unwrap_or("").to_string(),
            kind: if js::field(model, "kind").and_then(Json::as_str) == Some("headphone") { DeviceKind::Headphone } else { DeviceKind::Earbuds },
        },
        battery,
        anc: parse_mode(js::field(raw, "anc")),
        eq: Equalizer {
            preset: js::field(eq, "preset").and_then(Json::as_str).map(str::to_string),
            available: names(js::field(eq, "available")),
            custom,
        },
        low_latency: js::field(raw, "lowLatency").and_then(Json::as_bool),
        spatial: parse_mode(js::field(raw, "spatial")),
    })
}

/// One stdout line of `watch`. `None` for anything that is not one of its
/// four line types, a state line with no usable address or model included.
pub fn parse_line(line: &str) -> Option<Line> {
    let raw = json(js::trim(line))?;
    if !raw.is_object() {
        return None;
    }
    let text = |key: &str| js::field(&raw, key).and_then(Json::as_str).map(str::to_string);
    match js::field(&raw, "type")?.as_str()? {
        "state" => parse_state(&raw).map(|s| Line::State(Box::new(s))),
        "ack" => Some(Line::Ack { cmd: text("cmd").unwrap_or_default() }),
        "error" => Some(Line::Error {
            code: text("code").unwrap_or_default(),
            model_id: text("modelId"),
            message: text("message").unwrap_or_default(),
        }),
        "disconnected" => Some(Line::Disconnected),
        _ => None,
    }
}

/// Fast Pair model ids to ear-web's picture sets (res/js/control.js
/// `FASTPAIR_MODELS`), where the set names the colourway. Ear (a) and CMF
/// Buds Pro 2 carry two picture families under one base.
const ART: [(&str, &str); 74] = [
    ("624011", "b181_black"), ("31D53D", "b181_white"),
    ("1016DD", "b157_white"),
    ("ACC520", "b155_white"), ("DEE8C0", "b155_black"),
    ("FEB1C7", "b171_white"), ("A20444", "b171_black"),
    ("5E3FBC", "b162_white"), ("03464E", "b162_black"), ("8B6380", "b162_yellow"),
    ("C34F3B", "b183_black"), ("404D6D", "b183_white"), ("839E9A", "b183_yellow"),
    ("FC3AAF", "b174_white"), ("CC3444", "b174_blue"),
    ("5F8F82", "b163_orange"), ("2EB1CA", "b163_white"), ("ADD2C4", "b163_black"),
    ("4DFC4A", "b164_orange"), ("26C190", "b164_white"), ("AE35FD", "b164_black"),
    ("D35E18", "b168_orange"), ("ACCE54", "b168_white"), ("150A27", "b168_black"),
    ("F29566", "b172_black"), ("CA36A6", "b172_white"), ("A7B220", "b172_orange"), ("2B353E", "b172_blue"),
    ("2F45F5", "b187_black"), ("E1BE45", "b187_white"), ("1253C0", "b187_orange"), ("0F1A4F", "b187_blue"),
    ("F51D3D", "b187_black"), ("A544CE", "b187_white"), ("9EAC04", "b187_orange"), ("1D07D4", "b187_blue"),
    ("53BFE6", "b187_green"), ("072714", "b187_pink"),
    ("19EF24", "b179_black"), ("FF2AB0", "b179_green"), ("D9AB5D", "b179_orange"),
    ("3B1320", "b179_black"), ("359A7D", "b179_green"), ("00FDED", "b179_orange"),
    ("5C587F", "b184_blue"), ("4AEB6E", "b184_white"),
    ("70F8E3", "b185_black"), ("ED5412", "b185_white"), ("509CAE", "b185_orange"),
    ("C19ECD", "b170_black"), ("2D6FDA", "b170_grey"),
    ("7D46E5", "b173_black"), ("C1EBFD", "b173_white"),
    ("563DA5", "b175_green"), ("73C9EB", "b175_white"), ("1EFB39", "b175_black"),
    ("BFD53B", "b186_black"), ("DE8953", "b186_white"), ("98D02B", "b186_pink"),
    ("810478", "b198_white"), ("97EF75", "b198_yellow"), ("6F6C71", "b198_black"),
    ("79B3A9", "b198_yellow"), ("A292C6", "b198_pink"),
    ("148887", "b190_pink"), ("7B2328", "b190_white"), ("E9C3BE", "b190_black"), ("DB45D3", "b190_yellow"),
    ("8F28ED", "b189_blue"), ("E6673A", "b189_orange"), ("DCD2CB", "b189_white"), ("DA1280", "b189_black"),
    ("CDFDEC", "b192_black"), ("086485", "b192_silver"),
];

/// The colourway the model id names, else the base's black (white where
/// ear-web has no black), so a device always shows its own shape.
fn art(model: &Model) -> String {
    if let Some((_, set)) = ART.iter().find(|(id, _)| *id == model.id) {
        return (*set).into();
    }
    let base = model.base.to_ascii_lowercase();
    let mut sets = ART.iter().map(|(_, set)| *set).filter(|set| set.split('_').next() == Some(base.as_str()));
    let first = sets.clone().next();
    sets.find(|set| set.ends_with("_black")).or(first).unwrap_or("").into()
}

/// A device refused over its model. `seen_down` is armed once the device is
/// seen disconnected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark {
    pub seen_down: bool,
}

/// `marks`: address -> mark, the devices refused over their model.
/// `connected`: the addresses BlueZ reports connected right now. A mark is
/// armed once its device is seen disconnected and dropped when it is seen
/// connected after that, so a refused device is tried again only after a real
/// reconnect. Returns a new map.
pub fn rearm(marks: &BTreeMap<String, Mark>, connected: &[String]) -> BTreeMap<String, Mark> {
    let mut out = BTreeMap::new();
    for (address, mark) in marks {
        let up = connected.contains(address);
        if up && mark.seen_down {
            continue;
        }
        out.insert(address.clone(), Mark { seen_down: mark.seen_down || !up });
    }
    out
}

fn options(names: &[String], label: fn(&str) -> Option<&'static str>, icon: fn(&str) -> &'static str) -> Vec<ControlOption> {
    names.iter().map(|n| ControlOption::new(n.as_str(), label(n).unwrap_or(n), icon(n))).collect()
}

fn chosen(mode: &Mode) -> Option<&str> {
    mode.mode.as_deref().filter(|m| mode.available.iter().any(|a| a == m))
}

fn gain(state: &State) -> Option<(f64, f64)> {
    custom_gain(&state.model.base)
}

/// Custom bands only while the custom preset is on: nothingctl reads the
/// gains only then, and a write to them does nothing audible under another
/// preset.
fn custom_shown(state: &State) -> Option<((f64, f64), [f64; 3])> {
    if state.eq.preset.as_deref() == Some("custom") && state.eq.available.iter().any(|a| a == "custom") {
        Some((gain(state)?, state.eq.custom?))
    } else {
        None
    }
}

fn mode_value(mode: Option<&str>) -> Value {
    mode.map_or(Value::Null, Value::str)
}

/// `failure`: "" or [`FAILED`] after a refused write, shown in place of the
/// listening mode until the next state line.
pub fn normalise(state: &State, failure: &str) -> Device {
    let mut controls = Vec::new();
    if let Some(anc) = state.anc.as_ref().filter(|m| !m.available.is_empty()) {
        let modes = options(&anc.available, anc_label, anc_icon)
            .into_iter()
            .map(|o| if matches!(o.value.as_str(), Some("high" | "mid" | "low" | "adaptive")) { o.tier("Noise cancelling") } else { o })
            .collect();
        controls.push(choice("anc", "Listening mode", "Listening mode", mode_value(chosen(anc)), modes));
    }
    if !state.eq.available.is_empty() {
        let preset = state.eq.preset.as_deref().filter(|p| state.eq.available.iter().any(|a| a == p));
        controls.push(choice("eq", "Equalizer preset", "Equalizer", mode_value(preset), options(&state.eq.available, eq_label, |_| "")));
    }
    if let Some(((min, max), custom)) = custom_shown(state) {
        for (i, (key, label)) in BANDS.iter().enumerate() {
            controls.push(range(key, label, "Equalizer", custom[i].min(max).max(min), min, max, 1.0, Unit::Db));
        }
    }
    if let Some(spatial) = state.spatial.as_ref().filter(|m| !m.available.is_empty()) {
        controls.push(choice("spatial", "Spatial audio", "Spatial audio", mode_value(chosen(spatial)), options(&spatial.available, spatial_label, |_| "")));
    }
    if let Some(on) = state.low_latency {
        controls.push(toggle("low-latency", "Low latency", "Less audio delay for games and video", "Options", on));
    }
    let anc_name = state.anc.as_ref().and_then(chosen);
    device(DeviceFields {
        key: format!("{BACKEND}:{}", state.address),
        backend: BACKEND.into(),
        address: state.address.clone(),
        name: if state.name.is_empty() { state.model.name.clone() } else { state.name.clone() },
        kind: state.model.kind,
        connected: true,
        batteries: state
            .battery
            .iter()
            .map(|b| Battery::new(b.id, battery_label(b.id), b.level, b.charging, None))
            .collect(),
        controls,
        state_line: if !failure.is_empty() {
            failure.to_string()
        } else {
            anc_name.and_then(anc_line).unwrap_or("").to_string()
        },
        art: art(&state.model),
    })
}

fn pick(available: &[String], value: &Value) -> Option<Token> {
    let word = value.as_str()?;
    let token = Token::new(word)?;
    available.iter().any(|a| a == word).then_some(token)
}

fn whole_gain(g: f64, (min, max): (f64, f64)) -> bool {
    js::round(g) == g && g >= min && g <= max
}

/// The verb for one control change, or `None` when the control key, the value
/// or the device's own state does not allow it. `state` is that device's last
/// parsed state.
pub fn command(control_key: &str, value: &Value, state: Option<&State>) -> Option<NothingVerb> {
    let state = state?;
    match control_key {
        "anc" => pick(&state.anc.as_ref()?.available, value).map(NothingVerb::Anc),
        "eq" => pick(&state.eq.available, value).map(NothingVerb::Eq),
        "spatial" => pick(&state.spatial.as_ref()?.available, value).map(NothingVerb::Spatial),
        "low-latency" => match (state.low_latency, value) {
            (Some(_), Value::Bool(on)) => Some(NothingVerb::LowLatency(*on)),
            _ => None,
        },
        "eq-bass" | "eq-mid" | "eq-treble" => {
            let (range, custom) = custom_shown(state)?;
            let Value::Num(v) = value else { return None };
            let slot = BANDS.iter().position(|(k, _)| *k == control_key)?;
            let mut gains = custom.map(js::round);
            gains[slot] = *v;
            gains.iter().all(|g| whole_gain(*g, range)).then(|| NothingVerb::EqCustom(gains.map(|g| g as i32)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::earbuds::{control, range_fraction, range_text};

    // Fixtures follow nothingctl v0.1.0's own serialisation: the state line
    // is src/protocol/state.rs:178-192 (`Snapshot`, camelCase, serde_json
    // field order) filled the way snapshot() at :194-220 fills it for the
    // B175 table in src/protocol/model.rs:134-193; `ack` and `error` are
    // src/session.rs:193-196, `disconnected` src/main.rs:177, and a `list
    // --json` row src/transport/mod.rs:17-22. `eq.custom` is an f32 array,
    // which serde_json prints with a fractional part.
    const B175: &str = r#"{"type":"state","address":"AA:BB:CC:DD:EE:FF","name":"CMF Headphone Pro","model":{"base":"B175","name":"CMF Headphone Pro","kind":"headphone"},"firmware":"1.0.1.52","battery":[{"id":"single","level":80,"charging":false}],"anc":{"mode":"high","available":["off","transparency","high","mid","low","adaptive"]},"eq":{"preset":"rock","available":["rock","electronic","pop","vocals","classical","custom"],"custom":null},"lowLatency":false,"spatial":{"mode":"off","available":["off","concert","theatre"]}}"#;

    const B175_CUSTOM: &str = r#"{"type":"state","address":"aa:bb:cc:dd:ee:ff","name":"","model":{"base":"B175","name":"CMF Headphone Pro","kind":"headphone"},"firmware":"1.0.1.52","battery":[{"id":"single","level":35,"charging":true}],"anc":{"mode":"adaptive","available":["off","transparency","high","mid","low","adaptive"]},"eq":{"preset":"custom","available":["rock","electronic","pop","vocals","classical","custom"],"custom":[3.0,0.0,-2.0]},"lowLatency":true,"spatial":{"mode":"theatre","available":["off","concert","theatre"]}}"#;

    // Right after connect, before any reply: every value null, the model's
    // lists still present.
    const B175_EARLY: &str = r#"{"type":"state","address":"AA:BB:CC:DD:EE:FF","name":"CMF Headphone Pro","model":{"base":"B175","name":"CMF Headphone Pro","kind":"headphone"},"firmware":null,"battery":[],"anc":{"mode":null,"available":["off","transparency","high","mid","low","adaptive"]},"eq":{"preset":"dirac","available":["rock","electronic","pop","vocals","classical","custom"],"custom":null},"lowLatency":null,"spatial":{"mode":null,"available":["off","concert","theatre"]}}"#;

    fn state(line: &str) -> State {
        match parse_line(line) {
            Some(Line::State(s)) => *s,
            other => panic!("not a state line: {other:?}"),
        }
    }

    fn keys<T>(list: &[T], f: impl Fn(&T) -> String) -> String {
        list.iter().map(f).collect::<Vec<_>>().join(",")
    }

    fn option_keys(ctl: &super::super::Control, f: impl Fn(&ControlOption) -> String) -> String {
        keys(ctl.options.as_ref().unwrap(), f)
    }

    fn cmd(key: &str, value: impl Into<Value>, state: &State) -> Option<String> {
        command(key, &value.into(), Some(state)).map(|v| v.to_string())
    }

    #[test]
    fn parse_line_types() {
        let s = state(B175);
        assert_eq!(s.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(s.model.base, "B175");
        assert_eq!(parse_line(r#"{"cmd":"anc high","type":"ack"}"#), Some(Line::Ack { cmd: "anc high".into() }));
        match parse_line(r#"{"message":"write failed: broken pipe","type":"error"}"#) {
            Some(Line::Error { message, .. }) => assert_eq!(message, "write failed: broken pipe"),
            other => panic!("{other:?}"),
        }
        assert_eq!(parse_line(r#"{"type":"disconnected"}"#), Some(Line::Disconnected));
    }

    // nothingctl v0.1.1's refusal line, src/main.rs `unsupported_line`.
    #[test]
    fn unsupported_model_line() {
        let line = r#"{"type":"error","code":"unsupported-model","modelId":"ABCDEF","message":"model id ABCDEF on AA:BB:CC:DD:EE:FF is not supported (supported: B175 CMF Headphone Pro); nothing was sent"}"#;
        match parse_line(line) {
            Some(Line::Error { code, model_id, .. }) => {
                assert_eq!(code, UNSUPPORTED);
                assert_eq!(model_id.as_deref(), Some("ABCDEF"));
            }
            other => panic!("{other:?}"),
        }
        match parse_line(r#"{"type":"error","code":"unsupported-model","modelId":null,"message":"x"}"#) {
            Some(Line::Error { model_id, .. }) => assert_eq!(model_id, None),
            other => panic!("{other:?}"),
        }
        match parse_line(r#"{"type":"error","message":"write failed"}"#) {
            Some(Line::Error { code, .. }) => assert_eq!(code, ""),
            other => panic!("{other:?}"),
        }
        assert_eq!(UNSUPPORTED_EXIT, 3);
    }

    #[test]
    fn rearm_needs_a_disconnect_then_a_connect() {
        let a = "AA:BB:CC:DD:EE:FF".to_string();
        let mut marks = BTreeMap::from([(a.clone(), Mark { seen_down: false })]);
        marks = rearm(&marks, std::slice::from_ref(&a));
        assert!(!marks[&a].seen_down);
        marks = rearm(&marks, &[]);
        assert!(marks[&a].seen_down);
        marks = rearm(&marks, &[]);
        assert!(marks[&a].seen_down);
        marks = rearm(&marks, std::slice::from_ref(&a));
        assert!(!marks.contains_key(&a));
    }

    #[test]
    fn custom_bands_read_in_db() {
        let dev = normalise(&state(B175_CUSTOM), "");
        let bass = control(Some(&dev), "eq-bass");
        assert_eq!(bass.unwrap().unit.as_str(), "db");
        assert_eq!(range_text(bass), "+3\u{a0}dB");
        assert_eq!(range_fraction(bass), 0.75);
    }

    #[test]
    fn parse_line_garbage() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("error: no Nothing device is connected"), None);
        assert_eq!(parse_line(r#"{"type":"state""#), None);
        assert_eq!(parse_line("[1,2]"), None);
        assert_eq!(parse_line("null"), None);
        assert_eq!(parse_line(r#"{"type":"reboot"}"#), None);
        assert_eq!(parse_line(r#"{"type":"state","address":"; rm -rf","model":{"base":"B175"}}"#), None);
        assert_eq!(parse_line(r#"{"type":"state","address":"AA:BB:CC:DD:EE:FF"}"#), None);
    }

    #[test]
    fn list() {
        let l = parse_list(
            r#"[{"address":"AA:BB:CC:DD:EE:FF","name":"CMF Headphone Pro","connected":true},{"address":"11:22:33:44:55:66","name":"Ear (2)","connected":false},{"address":"-d x","name":"x","connected":true}]"#,
        );
        assert_eq!(l.len(), 2);
        assert!(l[0].connected);
        assert!(!l[1].connected);
        assert_eq!(parse_list("error: cannot reach BlueZ").len(), 0);
        assert_eq!(watch_argv("AA:BB:CC:DD:EE:FF").unwrap().join(" "), "nothingctl watch -d AA:BB:CC:DD:EE:FF");
        assert_eq!(watch_argv("AA:BB --dry-run"), None);
        assert_eq!(list_argv().join(" "), "nothingctl list --json");
    }

    #[test]
    fn normalise_b175() {
        let dev = normalise(&state(B175), "");
        assert_eq!(dev.key, "nothing:AA:BB:CC:DD:EE:FF");
        assert_eq!(dev.backend, "nothing");
        assert_eq!(dev.kind.as_str(), "headphone");
        assert_eq!(dev.name, "CMF Headphone Pro");
        assert_eq!(dev.batteries.len(), 1);
        assert_eq!(dev.batteries[0].id, BatteryId::Single);
        assert_eq!(dev.batteries[0].level, 80.0);
        assert_eq!(crate::earbuds::worst_level(Some(&dev)), 80.0);
        assert_eq!(keys(&dev.controls, |c| c.key.clone()), "anc,eq,spatial,low-latency");
        let anc = control(Some(&dev), "anc").unwrap();
        assert_eq!(option_keys(anc, |o| o.value.as_str().unwrap().into()), "off,transparency,high,mid,low,adaptive");
        assert_eq!(anc.value, "high".into());
        assert_eq!(control(Some(&dev), "eq").unwrap().value, "rock".into());
        assert_eq!(option_keys(control(Some(&dev), "eq").unwrap(), |o| o.label.clone()), "Rock,Electronic,Pop,Vocals,Classical,Custom");
        assert_eq!(control(Some(&dev), "spatial").unwrap().value, "off".into());
        assert_eq!(control(Some(&dev), "low-latency").unwrap().value, false.into());
        assert_eq!(dev.state_line, "High noise cancellation");
    }

    #[test]
    fn normalise_custom_eq_bands() {
        let dev = normalise(&state(B175_CUSTOM), "");
        assert_eq!(dev.address, "AA:BB:CC:DD:EE:FF");
        assert_eq!(dev.name, "CMF Headphone Pro");
        assert_eq!(keys(&dev.controls, |c| c.key.clone()), "anc,eq,eq-bass,eq-mid,eq-treble,spatial,low-latency");
        let bass = control(Some(&dev), "eq-bass").unwrap();
        assert_eq!(bass.kind, crate::earbuds::ControlKind::Range);
        assert_eq!(bass.value, 3.0.into());
        assert_eq!(bass.min, -6.0);
        assert_eq!(bass.max, 6.0);
        assert_eq!(control(Some(&dev), "eq-treble").unwrap().value, (-2.0).into());
        assert!(dev.batteries[0].charging);
        assert_eq!(dev.state_line, "Adaptive");
        assert_eq!(normalise(&state(B175_CUSTOM), FAILED).state_line, FAILED);
    }

    #[test]
    fn normalise_before_replies() {
        let dev = normalise(&state(B175_EARLY), "");
        assert_eq!(dev.batteries.len(), 0);
        assert_eq!(control(Some(&dev), "anc").unwrap().value, Value::Null);
        // dirac is decoded but not offered, so no option is selected.
        assert_eq!(control(Some(&dev), "eq").unwrap().value, Value::Null);
        assert!(control(Some(&dev), "low-latency").is_none());
        assert_eq!(dev.state_line, "");
    }

    #[test]
    fn unknown_model_has_no_custom_bands() {
        let mut s = state(B175_CUSTOM);
        s.model.base = "B999".into();
        assert!(control(Some(&normalise(&s, "")), "eq-bass").is_none());
        assert_eq!(cmd("eq-bass", 1.0, &s), None);
    }

    #[test]
    fn commands() {
        let s = state(B175);
        assert_eq!(cmd("anc", "transparency", &s).as_deref(), Some("anc transparency"));
        assert_eq!(cmd("eq", "classical", &s).as_deref(), Some("eq classical"));
        assert_eq!(cmd("spatial", "concert", &s).as_deref(), Some("spatial concert"));
        assert_eq!(cmd("low-latency", true, &s).as_deref(), Some("low-latency on"));
        assert_eq!(cmd("low-latency", false, &s).as_deref(), Some("low-latency off"));
        let c = state(B175_CUSTOM);
        assert_eq!(cmd("eq-mid", 4.0, &c).as_deref(), Some("eq-custom 3 4 -2"));
        assert_eq!(cmd("eq-bass", -6.0, &c).as_deref(), Some("eq-custom -6 0 -2"));
    }

    #[test]
    fn command_refusals() {
        let s = state(B175);
        assert_eq!(command("anc", &"high".into(), None), None);
        assert_eq!(cmd("rename", "x", &s), None);
        assert_eq!(cmd("firmware", "update", &s), None);
        assert_eq!(cmd("eq-custom", "1 1 1", &s), None);
        assert_eq!(cmd("anc", "loud", &s), None);
        assert_eq!(cmd("eq", "dirac", &s), None);
        assert_eq!(cmd("spatial", "on", &s), None);
        assert_eq!(cmd("low-latency", "on", &s), None);
        assert_eq!(cmd("anc", "high\nset -d 11:22:33:44:55:66 anc off", &s), None);
        assert_eq!(cmd("anc", "high ", &s), None);
        assert_eq!(command("anc", &Value::Other, Some(&s)), None);
        assert_eq!(cmd("eq", "rock\n", &s), None);
        // Custom bands only exist while the custom preset is on.
        assert_eq!(cmd("eq-bass", 2.0, &s), None);
        let c = state(B175_CUSTOM);
        assert_eq!(cmd("eq-bass", 7.0, &c), None);
        assert_eq!(cmd("eq-bass", 1.5, &c), None);
        assert_eq!(cmd("eq-bass", "2", &c), None);
        assert_eq!(cmd("eq-bass", "2\nanc off", &c), None);
    }

    #[test]
    fn injected_available_list_is_not_trusted() {
        let mut s = state(B175);
        s.anc.as_mut().unwrap().available.push("high\nspatial off".into());
        assert_eq!(cmd("anc", "high\nspatial off", &s), None);
        let mut raw: Json = serde_json::from_str(B175).unwrap();
        raw["anc"]["available"].as_array_mut().unwrap().push("off anc high".into());
        let parsed = state(&raw.to_string());
        assert!(!parsed.anc.unwrap().available.iter().any(|a| a == "off anc high"));
    }

    // The closed list: five verbs, each rendering as exactly its own word
    // followed by arguments the enum itself validated.
    #[test]
    fn the_verb_enum_is_the_whole_allow_list() {
        let verbs = [
            NothingVerb::Anc(Token::new("high").unwrap()),
            NothingVerb::Eq(Token::new("rock").unwrap()),
            NothingVerb::Spatial(Token::new("off").unwrap()),
            NothingVerb::LowLatency(true),
            NothingVerb::EqCustom([1, -2, 3]),
        ];
        let words: Vec<&str> = verbs.iter().map(NothingVerb::word).collect();
        assert_eq!(words, ["anc", "eq", "spatial", "low-latency", "eq-custom"]);
        for verb in &verbs {
            let line = verb.to_string();
            assert!(line.starts_with(verb.word()));
            assert!(!line.contains('\n'));
        }
    }

    #[test]
    fn tokens_are_lowercase_ascii_words_only() {
        assert!(Token::new("high").is_some());
        for bad in ["", "High", "off anc", "a\nb", "a-b", "a1", "high ", "é"] {
            assert!(Token::new(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn art_follows_the_model_id_then_the_base() {
        assert_eq!(normalise(&state(B175), "").art, "b175_black");
        let mut s = state(B175);
        s.model.id = "73C9EB".into();
        assert_eq!(normalise(&s, "").art, "b175_white");
        s.model.id = String::new();
        s.model.base = "B157".into();
        assert_eq!(normalise(&s, "").art, "b157_white");
        s.model.base = "B999".into();
        assert_eq!(normalise(&s, "").art, "");
    }
}
