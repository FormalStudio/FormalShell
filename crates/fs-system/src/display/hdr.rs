//! HDR model: what an output's EDID says about HDR, what to persist when it is
//! switched on, and which outputs a shell start has to put back.
//!
//! Hyprland 0.56 does not put HDR support in `monitors -j`, only the applied
//! `colorManagementPreset`. It decides support itself in
//! CMonitor::supportsHDR(): the EDID's CTA-861 colorimetry block lists BT.2020
//! RGB and its HDR static metadata block lists the PQ (SMPTE ST 2084) EOTF. An
//! unsupported output silently falls back to sRGB when handed `cm = hdr`, so
//! support is read from the EDID beforehand rather than found out by applying
//! the rule.

use super::outputs::{self, Color, Output};
use fs_js::{self as js, lenient};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::{BTreeMap, HashMap, HashSet};

const EXT_COLORIMETRY: u8 = 5;
const EXT_HDR_STATIC: u8 = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct Edid {
    pub present: bool,
    pub wide: bool,
    pub pq: bool,
    pub max_nits: f64,
}

impl Default for Edid {
    fn default() -> Self {
        Edid { present: false, wide: false, pq: false, max_nits: 0.0 }
    }
}

fn hex_bytes(text: &str) -> Vec<u8> {
    js::split_ws(text)
        .into_iter()
        .filter(|t| t.len() == 2 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .filter_map(|t| u8::from_str_radix(t, 16).ok())
        .collect()
}

/// `od -An -tx1 -v` text of /sys/class/drm/<card>-<connector>/edid. `present`
/// is false for an empty or truncated file, which is what the kernel exposes
/// for a connector with no EDID (the VM's vkms).
pub fn parse_edid(hex_text: &str) -> Edid {
    let bytes = hex_bytes(hex_text);
    let mut result = Edid::default();
    if bytes.len() < 128 {
        return result;
    }
    result.present = true;
    let at = |i: usize| bytes.get(i).copied();

    for b in 1..bytes.len() / 128 {
        let base = b * 128;
        if bytes[base] != 0x02 {
            continue;
        }
        let end = usize::from(at(base + 2).unwrap_or(0).min(127));
        let mut i = base + 4;
        while i < base + end {
            let tag = bytes[i] >> 5;
            let len = usize::from(bytes[i] & 0x1f);
            if tag == 7 && len >= 2 {
                let ext = at(i + 1).unwrap_or(0);
                if ext == EXT_COLORIMETRY {
                    result.wide = at(i + 2).unwrap_or(0) & 0x80 != 0;
                } else if ext == EXT_HDR_STATIC {
                    result.pq = at(i + 2).unwrap_or(0) & 0x04 != 0;
                    if len >= 4 {
                        // A missing luminance byte reads as NaN.
                        result.max_nits = match at(i + 4) {
                            Some(code) => js::round(50.0 * 2f64.powf(f64::from(code) / 32.0)),
                            None => f64::NAN,
                        };
                    }
                }
            }
            i += len + 1;
        }
    }
    result
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub supported: bool,
    pub reason: &'static str,
}

/// `edid` is `None` when the reader has not answered yet or found no connector
/// node.
pub fn verdict(edid: Option<&Edid>) -> Verdict {
    let no = |reason| Verdict { supported: false, reason };
    match edid {
        Some(e) if e.present => {
            if !e.pq {
                no("No HDR in EDID")
            } else if !e.wide {
                no("No wide gamut in EDID")
            } else {
                Verdict { supported: true, reason: "" }
            }
        }
        _ => no("No EDID"),
    }
}

pub fn is_on(row: Option<&Output>) -> bool {
    row.is_some_and(|r| outputs::is_hdr_preset(&r.cm))
}

/// The colour settings to restore on the way out. An HDR preset is never
/// recorded as the prior, or a shell restart in HDR could not turn it off.
pub fn prior_of(row: &Output) -> Color {
    Color {
        cm: if outputs::is_hdr_preset(&row.cm) || row.cm.is_empty() { "srgb".into() } else { row.cm.clone() },
        bitdepth: if row.ten_bit { 10 } else { 8 },
        sdrbrightness: if row.sdr_brightness > 0.0 { row.sdr_brightness } else { 1.0 },
        sdrsaturation: if row.sdr_saturation > 0.0 { row.sdr_saturation } else { 1.0 },
    }
}

pub fn on_color(brightness: f64, saturation: f64) -> Color {
    Color { cm: "hdr".into(), bitdepth: 10, sdrbrightness: brightness, sdrsaturation: saturation }
}

/// The prior as state.json holds it: possibly partial, each number as it was
/// written, so a write that does not touch it gives it back unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prior {
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub cm: Option<String>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub bitdepth: Option<Number>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub sdrbrightness: Option<Number>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub sdrsaturation: Option<Number>,
    /// Keys this shell does not write, kept as they are.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl From<&Color> for Prior {
    fn from(c: &Color) -> Self {
        Prior {
            cm: Some(c.cm.clone()),
            bitdepth: Some(c.bitdepth.into()),
            sdrbrightness: Number::from_f64(c.sdrbrightness),
            sdrsaturation: Number::from_f64(c.sdrsaturation),
            extra: Map::new(),
        }
    }
}

/// The colour settings to put back: an HDR preset or no `cm` is sRGB, a
/// depth other than 10 is 8, and a missing level is 1.
pub fn off_color(prior: &Prior) -> Color {
    let cm = prior.cm.as_deref().unwrap_or("");
    let number = |n: &Option<Number>| n.as_ref().and_then(Number::as_f64);
    let positive = |n: &Option<Number>| number(n).filter(|n| *n > 0.0).unwrap_or(1.0);
    Color {
        cm: if outputs::is_hdr_preset(cm) || cm.is_empty() { "srgb".into() } else { cm.into() },
        bitdepth: if number(&prior.bitdepth) == Some(10.0) { 10 } else { 8 },
        sdrbrightness: positive(&prior.sdrbrightness),
        sdrsaturation: positive(&prior.sdrsaturation),
    }
}

/// One output in state.json's `hdr`, present while the user wants it in HDR:
/// the colour settings to put back and the compositor instance it was put in
/// HDR under.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Want {
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub prior: Option<Prior>,
    #[serde(deserialize_with = "lenient::field", skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Keys this shell does not write, kept as they are.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// state.json's `hdr`, keyed by output name.
pub type Wants = BTreeMap<String, Want>;

/// `instance` is the compositor instance the output was put in HDR under.
pub fn with_output(saved: &Wants, name: &str, prior: &Color, instance: &str) -> Wants {
    let mut out = saved.clone();
    out.insert(name.to_string(), Want { prior: Some(prior.into()), instance: Some(instance.to_string()), extra: Map::new() });
    out
}

/// The same want, now held under `instance`.
pub fn with_instance(saved: &Wants, name: &str, instance: &str) -> Wants {
    let mut out = saved.clone();
    if let Some(w) = out.get_mut(name) {
        w.instance = Some(instance.to_string());
    }
    out
}

/// Whether `name` was put in HDR under the compositor instance still running.
/// That compositor showing it in SDR at a shell start means it was turned off
/// outside the shell, so the want is dropped rather than put back.
pub fn wanted_under(saved: &Wants, name: &str, instance: &str) -> bool {
    !instance.is_empty() && saved.get(name).and_then(|w| w.instance.as_deref()) == Some(instance)
}

pub fn without_output(saved: &Wants, name: &str) -> Wants {
    let mut out = saved.clone();
    out.remove(name);
    out
}

/// Outputs a shell start (or a config reload that reset the rules) has to put
/// back in HDR: wanted in state, present and lit, supported, not in HDR now,
/// and not already tried since the last reset.
pub fn pending_reapply(saved: &Wants, rows: &[Output], verdicts: &HashMap<String, Verdict>, tried: &HashSet<String>) -> Vec<String> {
    rows.iter()
        .filter(|row| saved.contains_key(&row.name) && row.enabled && !is_on(Some(row)))
        .filter(|row| verdicts.get(&row.name).is_some_and(|v| v.supported))
        .filter(|row| !tried.contains(&row.name))
        .map(|row| row.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(f: impl FnOnce(&mut Output)) -> Output {
        let mut r = Output {
            name: "eDP-1".into(),
            width: 2560,
            height: 1600,
            refresh: 165.0,
            scale: 1.6,
            enabled: true,
            cm: "srgb".into(),
            sdr_brightness: 1.0,
            sdr_saturation: 1.0,
            ..Default::default()
        };
        f(&mut r);
        r
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }

    /// A base block plus one CTA-861 extension carrying the given data blocks.
    fn edid(data_blocks: &[&[u8]]) -> String {
        let mut base = vec![0u8; 128];
        base[126] = 1;
        let mut ext: Vec<u8> = vec![0x02, 0x03, 0, 0];
        for b in data_blocks {
            ext.extend_from_slice(b);
        }
        ext[2] = ext.len() as u8;
        while ext.len() < 128 {
            ext.push(0);
        }
        base.extend(ext);
        hex(&base)
    }

    /// Colorimetry: extended tag 7, length 3, ext 5, BT2020 RGB in bit 7.
    const COLORIMETRY: [u8; 4] = [0xe3, 0x05, 0xc0, 0x00];
    /// HDR static metadata: ext 6, EOTF byte with PQ (bit 2), descriptor byte,
    /// max luminance code 96 (50 * 2^3 = 400 nits).
    const HDR_STATIC: [u8; 7] = [0xe6, 0x06, 0x05, 0x01, 96, 96, 0];

    #[test]
    fn edid_with_pq_and_bt2020_is_supported() {
        let e = parse_edid(&edid(&[&COLORIMETRY, &HDR_STATIC]));
        assert!(e.present);
        assert!(e.wide);
        assert!(e.pq);
        assert_eq!(e.max_nits, 400.0);
        assert!(verdict(Some(&e)).supported);
    }

    #[test]
    fn edid_without_a_static_metadata_block_says_no_hdr() {
        let v = verdict(Some(&parse_edid(&edid(&[&COLORIMETRY]))));
        assert!(!v.supported);
        assert_eq!(v.reason, "No HDR in EDID");
    }

    #[test]
    fn edid_with_hlg_only_is_not_pq() {
        let hlg_only: [u8; 7] = [0xe6, 0x06, 0x09, 0x01, 96, 96, 0];
        assert_eq!(verdict(Some(&parse_edid(&edid(&[&COLORIMETRY, &hlg_only])))).reason, "No HDR in EDID");
    }

    #[test]
    fn edid_with_pq_but_no_bt2020_says_no_wide_gamut() {
        let v = verdict(Some(&parse_edid(&edid(&[&HDR_STATIC]))));
        assert!(!v.supported);
        assert_eq!(v.reason, "No wide gamut in EDID");
    }

    #[test]
    fn empty_or_truncated_edid_is_not_present() {
        assert_eq!(verdict(Some(&parse_edid(""))).reason, "No EDID");
        assert_eq!(verdict(Some(&parse_edid("00 ff ff"))).reason, "No EDID");
        assert_eq!(verdict(None).reason, "No EDID");
    }

    #[test]
    fn off_color_restores_the_prior_and_never_an_hdr_preset() {
        let prior = prior_of(&row(|r| {
            r.cm = "wide".into();
            r.ten_bit = true;
        }));
        assert_eq!(prior.cm, "wide");
        assert_eq!(prior.bitdepth, 10);
        let partial: Prior = serde_json::from_value(json!({ "cm": prior.cm, "bitdepth": prior.bitdepth })).unwrap();
        assert_eq!(off_color(&partial), Color { cm: "wide".into(), bitdepth: 10, sdrbrightness: 1.0, sdrsaturation: 1.0 });
        assert_eq!(
            prior_of(&row(|r| {
                r.cm = "hdr".into();
                r.ten_bit = true;
            }))
            .cm,
            "srgb"
        );
        assert_eq!(off_color(&Prior { cm: Some("hdr".into()), ..Prior::default() }).cm, "srgb");
        assert_eq!(off_color(&Prior::default()).bitdepth, 8);
    }

    #[test]
    fn state_helpers_return_fresh_objects() {
        let prior = Color { cm: "srgb".into(), bitdepth: 8, sdrbrightness: 1.0, sdrsaturation: 1.0 };
        let a = with_output(&Wants::new(), "eDP-1", &prior, "sig-a");
        assert!(a.contains_key("eDP-1"));
        assert!(wanted_under(&a, "eDP-1", "sig-a"));
        assert!(!wanted_under(&a, "eDP-1", "sig-b"));
        assert!(!wanted_under(&a, "eDP-1", ""));
        let bare: Wants = [("eDP-1".to_string(), Want::default())].into();
        assert!(!wanted_under(&bare, "eDP-1", "sig-a"));
        let moved = with_instance(&a, "eDP-1", "sig-b");
        assert!(wanted_under(&moved, "eDP-1", "sig-b"));
        assert_eq!(moved["eDP-1"].prior, a["eDP-1"].prior);
        let b = without_output(&a, "eDP-1");
        assert!(!b.contains_key("eDP-1"));
        assert!(a.contains_key("eDP-1"));
    }

    #[test]
    fn reapply_wants_only_supported_lit_outputs_not_in_hdr_and_untried() {
        let saved: Wants = ["eDP-1", "DP-1", "DP-2", "DP-3"].into_iter().map(|n| (n.to_string(), Want::default())).collect();
        let named = |name: &str| row(|r| r.name = name.into());
        let rows = vec![
            named("eDP-1"),
            named("DP-1"),
            row(|r| {
                r.name = "DP-2".into();
                r.cm = "hdr".into();
            }),
            row(|r| {
                r.name = "DP-3".into();
                r.enabled = false;
            }),
            named("DP-4"),
        ];
        let yes = Verdict { supported: true, reason: "" };
        let no = Verdict { supported: false, reason: "" };
        let verdicts: HashMap<String, Verdict> = [
            ("eDP-1", yes.clone()),
            ("DP-1", no),
            ("DP-2", yes.clone()),
            ("DP-3", yes.clone()),
            ("DP-4", yes),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        assert_eq!(pending_reapply(&saved, &rows, &verdicts, &HashSet::new()).join(","), "eDP-1");
        let tried: HashSet<String> = ["eDP-1".to_string()].into();
        assert_eq!(pending_reapply(&saved, &rows, &verdicts, &tried).len(), 0);
        assert_eq!(pending_reapply(&Wants::new(), &rows, &verdicts, &HashSet::new()).len(), 0);
    }

    #[test]
    fn a_want_reads_back_as_written() {
        let text = json!({ "DP-2": { "note": "kept", "prior": { "cm": "wide", "sdrbrightness": 1, "x": [1] } } });
        let wants: Wants = serde_json::from_value(text.clone()).unwrap();
        assert_eq!(off_color(wants["DP-2"].prior.as_ref().unwrap()).sdrbrightness, 1.0);
        assert_eq!(serde_json::to_value(&wants).unwrap(), text);
        assert_eq!(serde_json::to_string(&wants).unwrap(), r#"{"DP-2":{"prior":{"cm":"wide","sdrbrightness":1,"x":[1]},"note":"kept"}}"#);
        let bad: Want = serde_json::from_value(json!({ "prior": { "cm": 3, "bitdepth": 10 }, "instance": 4 })).unwrap();
        assert_eq!(bad.prior.unwrap().bitdepth, Some(10.into()));
        assert_eq!(bad.instance, None);
    }
}
