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
use fs_js as js;
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

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
                        // A missing luminance byte reads as NaN, as it did in JS.
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

/// The EDID reader's output: one `@@<output name>` line per connector, then its
/// `od` hex. An output with no line was not found under /sys/class/drm at all.
pub fn parse_dump(text: &str) -> HashMap<String, Edid> {
    let mut out: HashMap<String, Edid> = HashMap::new();
    let mut name: Option<String> = None;
    let mut hex = String::new();
    let lines: Vec<&str> = text.split('\n').collect();
    for i in 0..=lines.len() {
        let line = lines.get(i).copied().unwrap_or("@@");
        if let Some(rest) = line.strip_prefix("@@") {
            if let Some(n) = &name
                && !out.get(n).is_some_and(|e| e.present)
            {
                out.insert(n.clone(), parse_edid(&hex));
            }
            name = Some(js::trim(rest).to_string());
            hex.clear();
        } else {
            hex.push_str(line);
            hex.push(' ');
        }
    }
    out
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

/// `prior` is whatever state.json held for the output: an object, possibly
/// partial, or null.
pub fn off_color(prior: &Value) -> Color {
    let cm = prior.get("cm").and_then(Value::as_str).unwrap_or("");
    let positive = |key: &str| prior.get(key).and_then(Value::as_f64).filter(|n| *n > 0.0).unwrap_or(1.0);
    Color {
        cm: if outputs::is_hdr_preset(cm) || cm.is_empty() { "srgb".into() } else { cm.into() },
        bitdepth: if prior.get("bitdepth").and_then(Value::as_f64) == Some(10.0) { 10 } else { 8 },
        sdrbrightness: positive("sdrbrightness"),
        sdrsaturation: positive("sdrsaturation"),
    }
}

/// state.json's `hdr`: `{ "<output>": { prior } }`, an entry present while the
/// user wants that output in HDR. Always a fresh map.
pub fn state_of(saved: &Value) -> Map<String, Value> {
    saved
        .as_object()
        .map(|o| o.iter().filter(|(_, v)| v.is_object()).map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

pub fn with_output(saved: &Value, name: &str, prior: &Color) -> Map<String, Value> {
    let mut out = state_of(saved);
    out.insert(
        name.to_string(),
        json!({ "prior": {
            "cm": prior.cm,
            "bitdepth": prior.bitdepth,
            "sdrbrightness": prior.sdrbrightness,
            "sdrsaturation": prior.sdrsaturation,
        } }),
    );
    out
}

pub fn without_output(saved: &Value, name: &str) -> Map<String, Value> {
    let mut out = state_of(saved);
    out.remove(name);
    out
}

/// Outputs a shell start (or a config reload that reset the rules) has to put
/// back in HDR: wanted in state, present and lit, supported, not in HDR now,
/// and not already tried since the last reset.
pub fn pending_reapply(
    saved: &Value,
    rows: &[Output],
    verdicts: &HashMap<String, Verdict>,
    tried: &HashSet<String>,
) -> Vec<String> {
    let wanted = state_of(saved);
    rows.iter()
        .filter(|row| wanted.contains_key(&row.name) && row.enabled && !is_on(Some(row)))
        .filter(|row| verdicts.get(&row.name).is_some_and(|v| v.supported))
        .filter(|row| !tried.contains(&row.name))
        .map(|row| row.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::outputs::{RuleOverrides, hyprland_color_rule, hyprland_monitor_rule, hyprland_rule_lua, parse_hyprland_outputs};

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
    fn dump_splits_connectors_and_prefers_a_populated_node() {
        let good = edid(&[&COLORIMETRY, &HDR_STATIC]);
        let text = format!("@@eDP-1\n{good}\n@@DP-2\n@@eDP-1\n{good}\n");
        let dump = parse_dump(&text);
        assert!(!dump["DP-2"].present);
        assert!(dump["eDP-1"].pq);
        let twice = parse_dump(&format!("@@eDP-1\n{good}\n@@eDP-1\n\n"));
        assert!(twice["eDP-1"].pq);
    }

    #[test]
    fn color_rule_keeps_mode_position_scale_transform_and_leaves_vrr() {
        let r = row(|r| {
            r.x = 1920;
            r.y = 240;
            r.transform = 1;
            r.vrr = true;
            r.scale = 1.67;
        });
        let rule = hyprland_color_rule(&r, &on_color(1.2, 1.0));
        assert_eq!(rule.output, "eDP-1");
        assert_eq!(rule.mode, "2560x1600@165");
        assert_eq!(rule.position, "1920x240");
        assert_eq!(rule.scale, "1.66667");
        assert_eq!(rule.transform, 1);
        assert_eq!(rule.bitdepth, 10);
        assert_eq!(rule.cm, "hdr");
        assert_eq!(rule.sdrbrightness, "1.2");
        assert_eq!(rule.mirror, "");
        assert!(!hyprland_rule_lua(&rule).contains("vrr"));
    }

    #[test]
    fn color_rule_does_not_clamp_a_live_scale_below_one() {
        let rule = hyprland_color_rule(&row(|r| r.scale = 0.8), &on_color(1.0, 1.0));
        assert_eq!(rule.scale, "0.8");
    }

    #[test]
    fn color_rule_carries_a_mirror() {
        let rule = hyprland_color_rule(&row(|r| r.mirror_of = "DP-1".into()), &on_color(1.0, 1.0));
        assert_eq!(rule.mirror, "DP-1");
        assert!(hyprland_rule_lua(&rule).contains("mirror = \"DP-1\""));
    }

    #[test]
    fn rule_lua_is_an_hl_monitor_call() {
        let rule = hyprland_color_rule(&row(|r| r.scale = 1.0), &on_color(1.2, 1.0));
        assert_eq!(
            hyprland_rule_lua(&rule),
            "hl.monitor({ output = \"eDP-1\", mode = \"2560x1600@165\", position = \"0x0\", scale = 1, transform = 0, bitdepth = 10, cm = \"hdr\", sdrbrightness = 1.2, sdrsaturation = 1, mirror = \"\" })"
        );
    }

    #[test]
    fn scale_change_keeps_an_active_hdr_preset() {
        let r = row(|r| {
            r.cm = "hdr".into();
            r.ten_bit = true;
            r.sdr_brightness = 1.2;
        });
        let lua = hyprland_rule_lua(&hyprland_monitor_rule(&r, &RuleOverrides { scale: Some(2.0), mirror_of: None }));
        assert!(lua.contains("bitdepth = 10, cm = \"hdr\", sdrbrightness = 1.2, sdrsaturation = 1"));
    }

    #[test]
    fn parse_reads_the_colour_fields() {
        let rows = parse_hyprland_outputs(
            &json!([{
                "name": "eDP-1", "x": 0, "y": 0, "width": 2560, "height": 1600, "refreshRate": 165, "scale": 1.6,
                "transform": 3, "vrr": true, "currentFormat": "XRGB2101010", "colorManagementPreset": "hdr",
                "sdrBrightness": 1.2, "sdrSaturation": 0.98, "mirrorOf": "none"
            }])
            .to_string(),
        );
        assert_eq!(rows[0].transform, 3);
        assert!(rows[0].vrr);
        assert!(rows[0].ten_bit);
        assert_eq!(rows[0].cm, "hdr");
        assert_eq!(rows[0].sdr_brightness, 1.2);
    }

    #[test]
    fn off_color_restores_the_prior_and_never_an_hdr_preset() {
        let prior = prior_of(&row(|r| {
            r.cm = "wide".into();
            r.ten_bit = true;
        }));
        assert_eq!(prior.cm, "wide");
        assert_eq!(prior.bitdepth, 10);
        let as_value = json!({ "cm": prior.cm, "bitdepth": prior.bitdepth });
        assert_eq!(off_color(&as_value).cm, "wide");
        assert_eq!(
            prior_of(&row(|r| {
                r.cm = "hdr".into();
                r.ten_bit = true;
            }))
            .cm,
            "srgb"
        );
        assert_eq!(off_color(&json!({ "cm": "hdr" })).cm, "srgb");
        assert_eq!(off_color(&Value::Null).bitdepth, 8);
    }

    #[test]
    fn state_helpers_return_fresh_objects() {
        let prior = Color { cm: "srgb".into(), bitdepth: 8, sdrbrightness: 1.0, sdrsaturation: 1.0 };
        let a = Value::Object(with_output(&Value::Null, "eDP-1", &prior));
        assert!(a.get("eDP-1").is_some());
        let b = without_output(&a, "eDP-1");
        assert!(!b.contains_key("eDP-1"));
        assert!(a.get("eDP-1").is_some());
        assert_eq!(state_of(&json!("junk")).len(), 0);
        let keys: Vec<String> = state_of(&json!({ "x": 1, "y": { "prior": {} } })).keys().cloned().collect();
        assert_eq!(keys.join(","), "y");
    }

    #[test]
    fn reapply_wants_only_supported_lit_outputs_not_in_hdr_and_untried() {
        let saved = json!({ "eDP-1": { "prior": {} }, "DP-1": { "prior": {} }, "DP-2": { "prior": {} }, "DP-3": { "prior": {} } });
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
        assert_eq!(pending_reapply(&Value::Null, &rows, &verdicts, &HashSet::new()).len(), 0);
    }
}
