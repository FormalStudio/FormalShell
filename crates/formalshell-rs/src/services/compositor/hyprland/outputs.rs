//! `j/monitors all` read into the display model's rows, and the `hl.monitor`
//! rules the display and HDR controls send through `hyprctl eval`.

use std::collections::HashMap;

use fs_js as js;
use fs_system::display::outputs::{self, Color, Output, trim_number};
use serde_json::{Value, json};

fn int(value: f64) -> i64 {
    if value.is_finite() { js::round(value) as i64 } else { 0 }
}

fn positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { fallback }
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Hyprland only accepts a scale whose mode divides into whole logical pixels,
/// counted in 1/120ths, so the acceptable scales are exactly the divisors of
/// gcd(width*120, height*120), and a requested scale rounds UP to the nearest
/// one.
fn clean_scale(scale: f64, width: i64, height: i64) -> f64 {
    let requested = outputs::clamp_scale(scale);
    if width <= 0 || height <= 0 {
        return requested;
    }
    let g = gcd(width * 120, height * 120);
    let mut k = js::round(requested * 120.0) as i64;
    k = k.min(g).max(1);
    while g % k != 0 {
        k += 1;
    }
    k as f64 / 120.0
}

/// `hyprctl monitors all -j`'s raw stdout, the only Hyprland enumeration that
/// includes disabled monitors (plain `monitors` omits them entirely, so an
/// output switched off would vanish from the very list the user needs to
/// switch it back on from). `mirrorOf` is the literal string "none" when the
/// monitor is not mirroring, and otherwise the mirrored monitor's numeric id,
/// which is turned back into the name every rule and row speaks in.
pub fn parse(text: &str) -> Vec<Output> {
    let Ok(Value::Array(data)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };

    let mut names: HashMap<String, String> = HashMap::new();
    for entry in &data {
        if let Some(obj) = entry.as_object()
            && let Some(id) = obj.get("id")
        {
            let key = match id {
                Value::String(s) => s.clone(),
                Value::Number(n) => js::num_str(n.as_f64().unwrap_or(f64::NAN)),
                Value::Bool(b) => b.to_string(),
                Value::Null => "null".to_string(),
                _ => continue,
            };
            names.insert(key, js::text(obj.get("name")));
        }
    }

    let mut rows = Vec::new();
    for entry in &data {
        let Some(m) = entry.as_object() else { continue };
        let name = js::text(m.get("name"));
        if name.is_empty() {
            continue;
        }
        let enabled = m.get("disabled") != Some(&Value::Bool(true));
        let mut mirror_of = js::text(m.get("mirrorOf"));
        if let Some(resolved) = names.get(&mirror_of) {
            mirror_of = resolved.clone();
        }
        let num = |key: &str| js::to_number(m.get(key));
        rows.push(Output {
            name,
            make: js::text(m.get("make")),
            model: js::text(m.get("model")),
            x: if enabled { int(num("x")) } else { 0 },
            y: if enabled { int(num("y")) } else { 0 },
            width: if enabled { int(num("width")) } else { 0 },
            height: if enabled { int(num("height")) } else { 0 },
            refresh: if enabled { positive(num("refreshRate"), 0.0) } else { 0.0 },
            scale: if enabled { positive(num("scale"), 1.0) } else { 1.0 },
            enabled,
            mirror_of: if mirror_of == "none" { String::new() } else { mirror_of },
            transform: int(num("transform")),
            vrr: m.get("vrr") == Some(&Value::Bool(true)),
            cm: js::text(m.get("colorManagementPreset")),
            ten_bit: js::text(m.get("currentFormat")).contains("2101010"),
            sdr_brightness: positive(num("sdrBrightness"), 1.0),
            sdr_saturation: positive(num("sdrSaturation"), 1.0),
        });
    }
    outputs::sort_outputs(&rows)
}

/// The colour rule for `row`, unsent: `{ rule, lua }`, the rule's numbers
/// as numbers.
pub fn color_request(row: &Output, color: &Color) -> Value {
    let r = color_rule(row, color);
    let number = |s: &str| s.parse::<f64>().map_or_else(|_| Value::String(s.to_owned()), js::json_number);
    let rule = json!({
        "output": r.output,
        "mode": r.mode,
        "position": r.position,
        "scale": number(&r.scale),
        "transform": r.transform,
        "bitdepth": r.bitdepth,
        "cm": r.cm,
        "sdrbrightness": number(&r.sdrbrightness),
        "sdrsaturation": number(&r.sdrsaturation),
        "mirror": r.mirror,
    });
    json!({"rule": rule, "lua": rule_lua(&r)})
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonitorRule {
    pub output: String,
    pub mode: String,
    pub position: String,
    pub scale: String,
    pub transform: i64,
    pub bitdepth: i64,
    pub cm: String,
    pub sdrbrightness: String,
    pub sdrsaturation: String,
    pub mirror: String,
}

/// `hl.monitor` merges into the output's existing rule, but an output with no
/// rule of its own starts from the defaults (mode preferred, scale auto), so
/// every rule below restates mode, position, scale, transform, mirror and the
/// colour fields, and overrides only the ones the caller means to change.
/// The mirror is always sent, empty to clear it, because an omitted one keeps
/// the previous rule's. vrr is never sent: the merge keeps a rule's own, an
/// output with none follows `misc.vrr`, and `monitors -j` reports vrr as a
/// flag, so restating it would turn a configured 2 or 3 into 1.
fn rule(row: &Output, position: String, scale: f64, mirror: &str, color: &Color) -> MonitorRule {
    MonitorRule {
        output: row.name.clone(),
        mode: mode(row),
        position,
        scale: trim_number(scale, 5),
        transform: row.transform,
        bitdepth: if color.bitdepth == 10 { 10 } else { 8 },
        cm: color.cm.clone(),
        sdrbrightness: trim_number(color.sdrbrightness, 2),
        sdrsaturation: trim_number(color.sdrsaturation, 2),
        mirror: mirror.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuleOverrides {
    pub scale: Option<f64>,
    pub mirror_of: Option<String>,
}

/// A scale or mirror change. The colour fields stay as the row holds them, so
/// a scale change on an output in HDR does not drop HDR. Position stays
/// "auto": a literal x/y here would fight the compositor's layout on every
/// change when neither control means to move anything.
pub fn monitor_rule(row: &Output, overrides: &RuleOverrides) -> MonitorRule {
    let scale = clean_scale(overrides.scale.unwrap_or(row.scale), row.width, row.height);
    let mirror_of = overrides.mirror_of.as_deref().unwrap_or(&row.mirror_of);
    let color = Color {
        cm: if row.cm.is_empty() { "srgb".to_string() } else { row.cm.clone() },
        bitdepth: if row.ten_bit { 10 } else { 8 },
        sdrbrightness: positive(row.sdr_brightness, 1.0),
        sdrsaturation: positive(row.sdr_saturation, 1.0),
    };
    rule(row, "auto".to_string(), scale, mirror_of, &color)
}

/// The row's scale as the compositor holds it. `monitors -j` prints scale to
/// two places, so 1.6667 arrives as 1.67; the intended value is the nearest
/// 1/120th when that is within the print rounding. No clamp and no divisor
/// search (`clean_scale`'s job for a scale the user asks for): this one is
/// already live, and a toggle must never rescale.
fn live_scale(row: &Output) -> f64 {
    let scale = positive(row.scale, 1.0);
    let snapped = js::round(scale * 120.0) / 120.0;
    if (snapped - scale).abs() <= 0.005 { snapped } else { scale }
}

/// A complete monitor rule for `row` that restates mode, position, scale,
/// transform and mirror as they are now and sets the colour fields from
/// `color`.
pub fn color_rule(row: &Output, color: &Color) -> MonitorRule {
    let position = if row.width > 0 { format!("{}x{}", row.x, row.y) } else { "auto".to_string() };
    rule(row, position, live_scale(row), &row.mirror_of, color)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// The `hyprctl eval` argument for a rule. `hyprctl keyword` is refused under
/// a Lua config ("Use eval").
pub fn rule_lua(rule: &MonitorRule) -> String {
    let fields = [
        format!("output = {}", json_str(&rule.output)),
        format!("mode = {}", json_str(&rule.mode)),
        format!("position = {}", json_str(&rule.position)),
        format!("scale = {}", rule.scale),
        format!("transform = {}", rule.transform),
        format!("bitdepth = {}", rule.bitdepth),
        format!("cm = {}", json_str(&rule.cm)),
        format!("sdrbrightness = {}", rule.sdrbrightness),
        format!("sdrsaturation = {}", rule.sdrsaturation),
        format!("mirror = {}", json_str(&rule.mirror)),
    ];
    format!("hl.monitor({{ {} }})", fields.join(", "))
}

/// Switching an output back on re-derives mode, position and scale instead of
/// restating the row's own: a disabled monitor reports a zero mode, so there
/// is nothing truthful left to restate. `disabled = false` is explicit because
/// the merge would otherwise keep the rule's `disabled = true`.
pub fn enabled_lua(name: &str, enabled: bool) -> String {
    if !enabled {
        return format!("hl.monitor({{ output = {}, disabled = true }})", json_str(name));
    }
    format!(
        "hl.monitor({{ output = {}, disabled = false, mode = \"preferred\", position = \"auto\", scale = \"auto\" }})",
        json_str(name)
    )
}

fn mode(row: &Output) -> String {
    if row.width <= 0 || row.height <= 0 {
        return "preferred".to_string();
    }
    let mut mode = format!("{}x{}", row.width, row.height);
    if row.refresh > 0.0 {
        mode.push_str(&format!("@{}", trim_number(row.refresh, 5)));
    }
    mode
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_system::display::hdr::on_color;
    use fs_system::display::outputs::find_output;
    use serde_json::json;

    fn row(f: impl FnOnce(&mut Output)) -> Output {
        let mut r = Output {
            name: "eDP-1".into(),
            width: 1920,
            height: 1080,
            refresh: 60.0,
            scale: 1.0,
            enabled: true,
            sdr_brightness: 1.0,
            sdr_saturation: 1.0,
            ..Default::default()
        };
        f(&mut r);
        r
    }

    fn mirroring(name: &str, of: &str) -> Output {
        row(|r| {
            r.name = name.into();
            r.mirror_of = of.into();
        })
    }

    /// A laptop panel in sRGB, the HDR controls' row.
    fn hdr_row(f: impl FnOnce(&mut Output)) -> Output {
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

    #[test]
    fn clean_scale_rounds_up_to_a_divisor_of_the_mode() {
        // gcd(1920*120, 1080*120) = 14400; 1.6 -> k=192, which already
        // divides 14400, so it survives intact.
        assert_eq!(clean_scale(1.6, 1920, 1080), 1.6);
    }

    #[test]
    fn clean_scale_moves_an_unrepresentable_scale_up() {
        // gcd(1366*120, 768*120) = 240; 1.25 -> k=150, and the next divisor of
        // 240 at or above it is 240.
        assert_eq!(clean_scale(1.25, 1366, 768), 2.0);
    }

    #[test]
    fn clean_scale_passes_through_when_the_mode_is_unknown() {
        assert_eq!(clean_scale(1.5, 0, 0), 1.5);
    }

    #[test]
    fn parse_reads_disabled_and_mirror_state() {
        let rows = parse(
            &json!([
                { "name": "eDP-1", "make": "Sharp", "model": "LQ140M1", "x": 0, "y": 0, "width": 2560, "height": 1440, "refreshRate": 59.951, "scale": 1.5, "disabled": false, "mirrorOf": "none" },
                { "name": "DP-1", "make": "Dell", "model": "U2720Q", "x": 2560, "y": 0, "width": 3840, "height": 2160, "refreshRate": 60, "scale": 2, "disabled": false, "mirrorOf": "eDP-1" },
                { "name": "HDMI-A-1", "x": 0, "y": 0, "width": 0, "height": 0, "refreshRate": 0, "scale": 1, "disabled": true, "mirrorOf": "none" }
            ])
            .to_string(),
        );
        let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(names.join("|"), "eDP-1|DP-1|HDMI-A-1");
        assert_eq!(rows[0].mirror_of, "");
        assert_eq!(rows[0].scale, 1.5);
        assert_eq!(rows[1].mirror_of, "eDP-1");
        assert!(!rows[2].enabled);
    }

    #[test]
    fn parse_drops_a_nameless_entry() {
        let rows = parse(&json!([{ "width": 1920, "height": 1080 }]).to_string());
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn parse_resolves_a_mirror_id_to_the_name() {
        let rows = parse(
            &json!([
                { "id": 0, "name": "eDP-1", "width": 1920, "height": 1080, "refreshRate": 60, "scale": 1, "disabled": false, "mirrorOf": "none" },
                { "id": 1, "name": "DP-1", "width": 1920, "height": 1080, "refreshRate": 60, "scale": 1, "disabled": false, "mirrorOf": "0" }
            ])
            .to_string(),
        );
        assert_eq!(find_output(&rows, "DP-1").unwrap().mirror_of, "eDP-1");
        assert_eq!(find_output(&rows, "eDP-1").unwrap().mirror_of, "");
    }

    #[test]
    fn parse_malformed_json() {
        assert_eq!(parse("not json{{{").len(), 0);
    }

    #[test]
    fn parse_non_array_payload() {
        assert_eq!(parse(&json!({ "name": "eDP-1" }).to_string()).len(), 0);
    }

    fn lua(r: &Output, overrides: &RuleOverrides) -> String {
        rule_lua(&monitor_rule(r, overrides))
    }

    fn scale_override(scale: f64) -> RuleOverrides {
        RuleOverrides { scale: Some(scale), mirror_of: None }
    }

    fn uhd(name: &str) -> Output {
        row(|r| {
            r.name = name.into();
            r.width = 3840;
            r.height = 2160;
            r.refresh = 60.0;
        })
    }

    #[test]
    fn rule_restates_the_row_and_applies_the_scale() {
        let rule = monitor_rule(&uhd("DP-1"), &scale_override(2.0));
        assert_eq!(rule.output, "DP-1");
        assert_eq!(rule.mode, "3840x2160@60");
        assert_eq!(rule.position, "auto");
        assert_eq!(rule.scale, "2");
        assert_eq!(rule.mirror, "");
    }

    #[test]
    fn rule_carries_an_existing_mirror_through_a_scale_change() {
        let rule = monitor_rule(&mirroring("DP-1", "eDP-1"), &scale_override(2.0));
        assert_eq!(rule.scale, "2");
        assert_eq!(rule.mirror, "eDP-1");
    }

    #[test]
    fn rule_keeps_the_scale_through_a_mirror_change() {
        let r = row(|r| {
            r.name = "DP-1".into();
            r.scale = 1.5;
        });
        let rule = monitor_rule(&r, &RuleOverrides { scale: None, mirror_of: Some("eDP-1".into()) });
        assert_eq!(rule.scale, "1.5");
        assert_eq!(rule.mirror, "eDP-1");
    }

    #[test]
    fn rule_clears_a_mirror_with_an_empty_source() {
        let rule = monitor_rule(&mirroring("DP-1", "eDP-1"), &RuleOverrides { scale: None, mirror_of: Some(String::new()) });
        assert_eq!(rule.mirror, "");
    }

    #[test]
    fn rule_falls_back_to_preferred_without_a_mode() {
        let r = row(|r| {
            r.name = "DP-1".into();
            r.width = 0;
            r.height = 0;
            r.refresh = 0.0;
        });
        assert_eq!(monitor_rule(&r, &RuleOverrides::default()).mode, "preferred");
    }

    #[test]
    fn rule_is_an_hl_monitor_call() {
        assert_eq!(
            lua(&uhd("DP-1"), &scale_override(2.0)),
            "hl.monitor({ output = \"DP-1\", mode = \"3840x2160@60\", position = \"auto\", scale = 2, transform = 0, bitdepth = 8, cm = \"srgb\", sdrbrightness = 1, sdrsaturation = 1, mirror = \"\" })"
        );
        assert!(lua(&mirroring("DP-1", "eDP-1"), &scale_override(2.0)).contains("mirror = \"eDP-1\" })"));
    }

    #[test]
    fn enable_and_disable_lua() {
        assert_eq!(enabled_lua("DP-1", false), "hl.monitor({ output = \"DP-1\", disabled = true })");
        assert_eq!(
            enabled_lua("DP-1", true),
            "hl.monitor({ output = \"DP-1\", disabled = false, mode = \"preferred\", position = \"auto\", scale = \"auto\" })"
        );
    }

    #[test]
    fn hdr_rule_keeps_mode_position_scale_transform_and_leaves_vrr() {
        let r = hdr_row(|r| {
            r.x = 1920;
            r.y = 240;
            r.transform = 1;
            r.vrr = true;
            r.scale = 1.67;
        });
        let rule = color_rule(&r, &on_color(1.2, 1.0));
        assert_eq!(rule.output, "eDP-1");
        assert_eq!(rule.mode, "2560x1600@165");
        assert_eq!(rule.position, "1920x240");
        assert_eq!(rule.scale, "1.66667");
        assert_eq!(rule.transform, 1);
        assert_eq!(rule.bitdepth, 10);
        assert_eq!(rule.cm, "hdr");
        assert_eq!(rule.sdrbrightness, "1.2");
        assert_eq!(rule.mirror, "");
        assert!(!rule_lua(&rule).contains("vrr"));
    }

    #[test]
    fn hdr_rule_does_not_clamp_a_live_scale_below_one() {
        let rule = color_rule(&hdr_row(|r| r.scale = 0.8), &on_color(1.0, 1.0));
        assert_eq!(rule.scale, "0.8");
    }

    #[test]
    fn hdr_rule_carries_a_mirror() {
        let rule = color_rule(&hdr_row(|r| r.mirror_of = "DP-1".into()), &on_color(1.0, 1.0));
        assert_eq!(rule.mirror, "DP-1");
        assert!(rule_lua(&rule).contains("mirror = \"DP-1\""));
    }

    #[test]
    fn hdr_rule_lua_is_an_hl_monitor_call() {
        let rule = color_rule(&hdr_row(|r| r.scale = 1.0), &on_color(1.2, 1.0));
        assert_eq!(
            rule_lua(&rule),
            "hl.monitor({ output = \"eDP-1\", mode = \"2560x1600@165\", position = \"0x0\", scale = 1, transform = 0, bitdepth = 10, cm = \"hdr\", sdrbrightness = 1.2, sdrsaturation = 1, mirror = \"\" })"
        );
    }

    #[test]
    fn scale_change_keeps_an_active_hdr_preset() {
        let r = hdr_row(|r| {
            r.cm = "hdr".into();
            r.ten_bit = true;
            r.sdr_brightness = 1.2;
        });
        let lua = rule_lua(&monitor_rule(&r, &RuleOverrides { scale: Some(2.0), mirror_of: None }));
        assert!(lua.contains("bitdepth = 10, cm = \"hdr\", sdrbrightness = 1.2, sdrsaturation = 1"));
    }

    #[test]
    fn parse_reads_the_colour_fields() {
        let rows = parse(
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
}
