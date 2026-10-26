//! Output/display model for the display panel: Hyprland's wire shape
//! normalized onto one row contract, plus every derivation the panel renders,
//! sort order, the mode/scale/status labels, what a mirror would actually do,
//! and whether an output may be switched off at all.
//!
//! `name` is the compositor's own output name and stays an opaque string end
//! to end: nothing here parses or compares it numerically, and Hyprland's
//! monitor rule takes the name verbatim.
//!
//! `width`/`height`/`refresh` describe the CURRENT mode in physical pixels and
//! Hz. A disabled output reports zeros for all three: Hyprland has no mode to
//! report for one, and inventing its last known mode would be exactly the
//! stubbed value the honest-state rule forbids. `mirror_of` is "" when the
//! output isn't mirroring anything.

use fs_js as js;
use crate::monitor::gpu;
use serde_json::Value;
use std::collections::HashMap;

/// The scale slider's range and quantization. 0.25 is binary-exact, so
/// `quantize_scale` never accumulates float drift across repeated steps.
pub const SCALE_MIN: f64 = 1.0;
pub const SCALE_MAX: f64 = 3.0;
pub const SCALE_STEP: f64 = 0.25;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Output {
    pub name: String,
    pub make: String,
    pub model: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    pub refresh: f64,
    pub scale: f64,
    pub enabled: bool,
    pub mirror_of: String,
    pub transform: i64,
    pub vrr: bool,
    pub cm: String,
    pub ten_bit: bool,
    pub sdr_brightness: f64,
    pub sdr_saturation: f64,
}

fn int(value: f64) -> i64 {
    if value.is_finite() { js::round(value) as i64 } else { 0 }
}

fn positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 { value } else { fallback }
}

/// Fixed-decimal, then back through a number so a whole scale prints "1"
/// rather than "1.00" and a two-place refresh prints "60" rather than "60.00".
fn trim_number(value: f64, decimals: usize) -> String {
    js::num_str(js::parse_number(&js::to_fixed(value, decimals)))
}

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

// ---- Scale -------------------------------------------------------------

pub fn clamp_scale(scale: f64) -> f64 {
    if !scale.is_finite() {
        return SCALE_MIN;
    }
    SCALE_MIN.max(SCALE_MAX.min(scale))
}

pub fn quantize_scale(scale: f64) -> f64 {
    js::round(clamp_scale(scale) / SCALE_STEP) * SCALE_STEP
}

pub fn fraction_for_scale(scale: f64) -> f64 {
    (clamp_scale(scale) - SCALE_MIN) / (SCALE_MAX - SCALE_MIN)
}

pub fn scale_for_fraction(fraction: f64) -> f64 {
    let f = if fraction.is_finite() { fraction } else { 0.0 };
    let f = f.clamp(0.0, 1.0);
    quantize_scale(SCALE_MIN + f * (SCALE_MAX - SCALE_MIN))
}

/// One notch either way from wherever the compositor currently reports the
/// output, quantized first so an off-grid live scale (a compositor is free to
/// answer 1.6) still steps onto the grid rather than staying off it forever.
pub fn step_scale(scale: f64, direction: f64) -> f64 {
    quantize_scale(quantize_scale(scale) + if direction > 0.0 { SCALE_STEP } else { -SCALE_STEP })
}

pub fn format_scale(scale: f64) -> String {
    if !scale.is_finite() || scale <= 0.0 {
        return String::new();
    }
    format!("{}X", trim_number(scale, 2))
}

/// Hyprland only accepts a scale whose mode divides into whole logical pixels,
/// counted in 1/120ths, so the acceptable scales are exactly the divisors of
/// gcd(width*120, height*120), and a requested scale rounds UP to the nearest
/// one.
pub fn clean_scale(scale: f64, width: i64, height: i64) -> f64 {
    let requested = clamp_scale(scale);
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

// ---- Row derivations ---------------------------------------------------

/// Enabled outputs first (the ones an action can act on), then left-to-right /
/// top-to-bottom by logical position, then by name, so the order never
/// depends on the map key order or array order the compositor answered in.
pub fn sort_outputs(rows: &[Output]) -> Vec<Output> {
    let mut out = rows.to_vec();
    out.sort_by(|a, b| {
        b.enabled
            .cmp(&a.enabled)
            .then(a.x.cmp(&b.x))
            .then(a.y.cmp(&b.y))
            // JS compares strings by UTF-16 code unit.
            .then_with(|| a.name.encode_utf16().cmp(b.name.encode_utf16()))
    });
    out
}

pub fn find_output<'a>(rows: &'a [Output], name: &str) -> Option<&'a Output> {
    rows.iter().find(|r| r.name == name)
}

pub fn enabled_count(rows: &[Output]) -> usize {
    rows.iter().filter(|r| r.enabled).count()
}

/// A display may always be switched on; switching one off is refused when it
/// is the last enabled output, which would leave the session with nothing on
/// screen and no surface left to undo it from.
pub fn can_toggle(rows: &[Output], name: &str) -> bool {
    match find_output(rows, name) {
        None => false,
        Some(row) if !row.enabled => true,
        Some(_) => enabled_count(rows) > 1,
    }
}

/// "2560x1440@59.95", or "" for an output with no current mode to report.
pub fn mode_label(row: &Output) -> String {
    if !row.enabled || row.width <= 0 || row.height <= 0 {
        return String::new();
    }
    let mut label = format!("{}x{}", row.width, row.height);
    if row.refresh > 0.0 {
        label.push_str(&format!("@{}", trim_number(row.refresh, 2)));
    }
    label
}

/// "DELL U2720Q" for the row's second meta line, "" when the compositor
/// reports neither half, never a placeholder standing in for hardware
/// identity we were not given.
pub fn describe(row: &Output) -> String {
    [row.make.as_str(), row.model.as_str()].iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirrorPlan {
    pub ok: bool,
    pub reason: &'static str,
    pub primary: String,
    pub targets: Vec<String>,
}

/// What turning MIRROR on would do: every other enabled output mirrors
/// `primary_name` (the focused output when it is enabled, else the first
/// sorted enabled one). `ok: false` with reason `"single"` when fewer than two
/// outputs are enabled: there is nothing to mirror onto, and the panel says so
/// rather than rendering a toggle that cannot act.
pub fn mirror_plan(rows: &[Output], primary_name: &str) -> MirrorPlan {
    let enabled: Vec<Output> = sort_outputs(rows).into_iter().filter(|r| r.enabled).collect();
    if enabled.len() < 2 {
        return MirrorPlan { ok: false, reason: "single", primary: String::new(), targets: Vec::new() };
    }
    let primary = find_output(&enabled, primary_name).unwrap_or(&enabled[0]).name.clone();
    MirrorPlan {
        ok: true,
        reason: "",
        targets: enabled.iter().filter(|r| r.name != primary).map(|r| r.name.clone()).collect(),
        primary,
    }
}

/// Every output currently mirroring something, the set MIRROR OFF has to
/// clear, and the reason the toggle reads as on.
pub fn mirrored_names(rows: &[Output]) -> Vec<String> {
    rows.iter().filter(|r| !r.mirror_of.is_empty()).map(|r| r.name.clone()).collect()
}

/// The output every active mirror points at, or "" when nothing mirrors. The
/// panel drives one source at a time, so the first mirroring row answers for
/// the whole set.
pub fn mirror_source(rows: &[Output]) -> String {
    mirrored_names(rows)
        .first()
        .and_then(|n| find_output(rows, n))
        .map_or_else(String::new, |r| r.mirror_of.clone())
}

// ---- Backend normalizer ------------------------------------------------

/// `hyprctl monitors all -j`'s raw stdout, the only Hyprland enumeration that
/// includes disabled monitors (plain `monitors` omits them entirely, so an
/// output switched off would vanish from the very list the user needs to
/// switch it back on from). `mirrorOf` is the literal string "none" when the
/// monitor is not mirroring, and otherwise the mirrored monitor's numeric id,
/// which is turned back into the name every rule and row here speaks in.
pub fn parse_hyprland_outputs(text: &str) -> Vec<Output> {
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
    sort_outputs(&rows)
}

pub fn is_hdr_preset(cm: &str) -> bool {
    cm == "hdr" || cm == "hdredid"
}

// ---- Monitor rules -----------------------------------------------------

/// The colour fields of a monitor rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    pub cm: String,
    pub bitdepth: i64,
    pub sdrbrightness: f64,
    pub sdrsaturation: f64,
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
        mode: hyprland_mode(row),
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
pub fn hyprland_monitor_rule(row: &Output, overrides: &RuleOverrides) -> MonitorRule {
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
pub fn hyprland_color_rule(row: &Output, color: &Color) -> MonitorRule {
    let position = if row.width > 0 { format!("{}x{}", row.x, row.y) } else { "auto".to_string() };
    rule(row, position, live_scale(row), &row.mirror_of, color)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// The `hyprctl eval` argument for a rule. `hyprctl keyword` is refused under
/// a Lua config ("Use eval").
pub fn hyprland_rule_lua(rule: &MonitorRule) -> String {
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
pub fn hyprland_enabled_lua(name: &str, enabled: bool) -> String {
    if !enabled {
        return format!("hl.monitor({{ output = {}, disabled = true }})", json_str(name));
    }
    format!(
        "hl.monitor({{ output = {}, disabled = false, mode = \"preferred\", position = \"auto\", scale = \"auto\" }})",
        json_str(name)
    )
}

fn hyprland_mode(row: &Output) -> String {
    if row.width <= 0 || row.height <= 0 {
        return "preferred".to_string();
    }
    let mut mode = format!("{}x{}", row.width, row.height);
    if row.refresh > 0.0 {
        mode.push_str(&format!("@{}", trim_number(row.refresh, 5)));
    }
    mode
}

// ---- GPU annotation ---------------------------------------------------------

/// The row's own meta line naming the card driving it, e.g. "NVIDIA /
/// DISCRETE" for `connector_name`'s row when it matches one of `cards`'
/// connector names verbatim. "" when no card claims the connector, never a
/// guess. Also "" whenever there is one card or fewer, since annotating every
/// row with the same card is noise on a single-GPU machine, the common case.
pub fn output_card_label(connector_name: &str, cards: &[gpu::Card]) -> String {
    if cards.len() <= 1 {
        return String::new();
    }
    let Some(card) = gpu::output_card(connector_name, cards) else {
        return String::new();
    };
    let vendor = match gpu::vendor_name(&card.vendor_id) {
        "" => card.driver.to_uppercase(),
        v => v.to_string(),
    };
    format!("{vendor} / {}", if gpu::is_discrete(card) { "DISCRETE" } else { "INTEGRATED" })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn named(name: &str) -> Output {
        row(|r| r.name = name.into())
    }

    fn joined(v: &[String]) -> String {
        v.join("|")
    }

    // scale

    #[test]
    fn clamp_scale_holds_the_slider_range() {
        assert_eq!(clamp_scale(0.4), 1.0);
        assert_eq!(clamp_scale(9.0), 3.0);
        assert_eq!(clamp_scale(1.75), 1.75);
    }

    #[test]
    fn clamp_scale_falls_back_on_a_non_number() {
        assert_eq!(clamp_scale(f64::NAN), 1.0);
    }

    #[test]
    fn quantize_scale_snaps_to_the_quarter_grid() {
        assert_eq!(quantize_scale(1.6), 1.5);
        assert_eq!(quantize_scale(1.7), 1.75);
        assert_eq!(quantize_scale(2.0), 2.0);
    }

    #[test]
    fn fraction_for_scale_spans_the_track() {
        assert_eq!(fraction_for_scale(1.0), 0.0);
        assert_eq!(fraction_for_scale(2.0), 0.5);
        assert_eq!(fraction_for_scale(3.0), 1.0);
    }

    #[test]
    fn scale_for_fraction_quantizes_and_clamps() {
        assert_eq!(scale_for_fraction(0.0), 1.0);
        assert_eq!(scale_for_fraction(0.5), 2.0);
        assert_eq!(scale_for_fraction(1.0), 3.0);
        assert_eq!(scale_for_fraction(-4.0), 1.0);
        assert_eq!(scale_for_fraction(4.0), 3.0);
    }

    #[test]
    fn step_scale_moves_one_notch_from_an_off_grid_value() {
        assert_eq!(step_scale(1.6, 1.0), 1.75);
        assert_eq!(step_scale(1.6, -1.0), 1.25);
    }

    #[test]
    fn step_scale_stops_at_the_range_ends() {
        assert_eq!(step_scale(1.0, -1.0), 1.0);
        assert_eq!(step_scale(3.0, 1.0), 3.0);
    }

    #[test]
    fn format_scale_drops_trailing_zeros() {
        assert_eq!(format_scale(1.0), "1X");
        assert_eq!(format_scale(1.5), "1.5X");
        assert_eq!(format_scale(1.25), "1.25X");
    }

    #[test]
    fn format_scale_empty_for_a_missing_value() {
        assert_eq!(format_scale(0.0), "");
        assert_eq!(format_scale(f64::NAN), "");
    }

    // cleanScale: Hyprland's whole-logical-pixel constraint

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

    // sortOutputs

    #[test]
    fn sort_puts_enabled_outputs_first_then_left_to_right() {
        let rows = sort_outputs(&[
            row(|r| {
                r.name = "HDMI-A-1".into();
                r.enabled = false;
            }),
            row(|r| {
                r.name = "DP-2".into();
                r.x = 3840;
            }),
            row(|r| {
                r.name = "DP-1".into();
                r.x = 1920;
            }),
            row(|r| {
                r.name = "eDP-1".into();
                r.x = 0;
            }),
        ]);
        let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(joined(&names), "eDP-1|DP-1|DP-2|HDMI-A-1");
    }

    #[test]
    fn sort_breaks_position_ties_by_name() {
        let rows = sort_outputs(&[named("DP-2"), named("DP-1")]);
        assert_eq!(rows[0].name, "DP-1");
    }

    // canToggle

    fn off(name: &str) -> Output {
        row(|r| {
            r.name = name.into();
            r.enabled = false;
        })
    }

    #[test]
    fn can_toggle_refuses_to_disable_the_last_enabled_output() {
        assert!(!can_toggle(&[named("eDP-1"), off("DP-1")], "eDP-1"));
    }

    #[test]
    fn can_toggle_always_allows_enabling() {
        assert!(can_toggle(&[named("eDP-1"), off("DP-1")], "DP-1"));
    }

    #[test]
    fn can_toggle_allows_disabling_when_another_stays_on() {
        let dp1 = row(|r| {
            r.name = "DP-1".into();
            r.x = 1920;
        });
        assert!(can_toggle(&[named("eDP-1"), dp1], "eDP-1"));
    }

    #[test]
    fn can_toggle_false_for_an_unknown_output() {
        assert!(!can_toggle(&[named("eDP-1")], "DP-9"));
    }

    // labels

    #[test]
    fn mode_label_reports_the_current_mode() {
        let r = row(|r| {
            r.width = 2560;
            r.height = 1440;
            r.refresh = 59.951;
        });
        assert_eq!(mode_label(&r), "2560x1440@59.95");
    }

    #[test]
    fn mode_label_empty_for_a_disabled_output() {
        let r = row(|r| {
            r.enabled = false;
            r.width = 0;
            r.height = 0;
            r.refresh = 0.0;
        });
        assert_eq!(mode_label(&r), "");
    }

    #[test]
    fn describe_joins_make_and_model() {
        let r = row(|r| {
            r.make = "Dell".into();
            r.model = "U2720Q".into();
        });
        assert_eq!(describe(&r), "Dell U2720Q");
    }

    #[test]
    fn describe_empty_when_the_compositor_reports_neither() {
        assert_eq!(describe(&named("eDP-1")), "");
    }

    // mirror

    fn at(name: &str, x: i64) -> Output {
        row(|r| {
            r.name = name.into();
            r.x = x;
        })
    }

    #[test]
    fn mirror_plan_points_every_other_output_at_the_focused_one() {
        let rows = [named("eDP-1"), at("DP-1", 1920), at("DP-2", 3840)];
        let plan = mirror_plan(&rows, "DP-1");
        assert!(plan.ok);
        assert_eq!(plan.primary, "DP-1");
        assert_eq!(joined(&plan.targets), "eDP-1|DP-2");
    }

    #[test]
    fn mirror_plan_falls_back_to_the_first_sorted_output() {
        let plan = mirror_plan(&[at("DP-1", 1920), at("eDP-1", 0)], "");
        assert_eq!(plan.primary, "eDP-1");
        assert_eq!(joined(&plan.targets), "DP-1");
    }

    #[test]
    fn mirror_plan_skips_disabled_outputs_entirely() {
        let plan = mirror_plan(&[named("eDP-1"), off("DP-1")], "eDP-1");
        assert!(!plan.ok);
        assert_eq!(plan.reason, "single");
        assert_eq!(plan.targets.len(), 0);
    }

    fn mirroring(name: &str, of: &str) -> Output {
        row(|r| {
            r.name = name.into();
            r.mirror_of = of.into();
        })
    }

    #[test]
    fn mirrored_names_lists_only_the_mirroring_outputs() {
        let rows = [named("eDP-1"), mirroring("DP-1", "eDP-1")];
        assert_eq!(joined(&mirrored_names(&rows)), "DP-1");
    }

    #[test]
    fn mirror_source_reports_what_the_mirror_points_at() {
        let rows = [named("eDP-1"), mirroring("DP-1", "eDP-1")];
        assert_eq!(mirror_source(&rows), "eDP-1");
    }

    #[test]
    fn mirror_source_empty_when_nothing_mirrors() {
        assert_eq!(mirror_source(&[named("eDP-1")]), "");
    }

    // parseHyprlandOutputs

    #[test]
    fn parse_hyprland_reads_disabled_and_mirror_state() {
        let rows = parse_hyprland_outputs(
            &json!([
                { "name": "eDP-1", "make": "Sharp", "model": "LQ140M1", "x": 0, "y": 0, "width": 2560, "height": 1440, "refreshRate": 59.951, "scale": 1.5, "disabled": false, "mirrorOf": "none" },
                { "name": "DP-1", "make": "Dell", "model": "U2720Q", "x": 2560, "y": 0, "width": 3840, "height": 2160, "refreshRate": 60, "scale": 2, "disabled": false, "mirrorOf": "eDP-1" },
                { "name": "HDMI-A-1", "x": 0, "y": 0, "width": 0, "height": 0, "refreshRate": 0, "scale": 1, "disabled": true, "mirrorOf": "none" }
            ])
            .to_string(),
        );
        let names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(joined(&names), "eDP-1|DP-1|HDMI-A-1");
        assert_eq!(rows[0].mirror_of, "");
        assert_eq!(rows[0].scale, 1.5);
        assert_eq!(rows[1].mirror_of, "eDP-1");
        assert!(!rows[2].enabled);
    }

    #[test]
    fn parse_hyprland_drops_a_nameless_entry() {
        let rows = parse_hyprland_outputs(&json!([{ "width": 1920, "height": 1080 }]).to_string());
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn parse_hyprland_resolves_a_mirror_id_to_the_name() {
        let rows = parse_hyprland_outputs(
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
    fn parse_hyprland_malformed_json() {
        assert_eq!(parse_hyprland_outputs("not json{{{").len(), 0);
    }

    #[test]
    fn parse_hyprland_non_array_payload() {
        assert_eq!(parse_hyprland_outputs(&json!({ "name": "eDP-1" }).to_string()).len(), 0);
    }

    // hyprlandMonitorRule

    fn lua(r: &Output, overrides: &RuleOverrides) -> String {
        hyprland_rule_lua(&hyprland_monitor_rule(r, overrides))
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
    fn hyprland_rule_restates_the_row_and_applies_the_scale() {
        let rule = hyprland_monitor_rule(&uhd("DP-1"), &scale_override(2.0));
        assert_eq!(rule.output, "DP-1");
        assert_eq!(rule.mode, "3840x2160@60");
        assert_eq!(rule.position, "auto");
        assert_eq!(rule.scale, "2");
        assert_eq!(rule.mirror, "");
    }

    #[test]
    fn hyprland_rule_carries_an_existing_mirror_through_a_scale_change() {
        let rule = hyprland_monitor_rule(&mirroring("DP-1", "eDP-1"), &scale_override(2.0));
        assert_eq!(rule.scale, "2");
        assert_eq!(rule.mirror, "eDP-1");
    }

    #[test]
    fn hyprland_rule_keeps_the_scale_through_a_mirror_change() {
        let r = row(|r| {
            r.name = "DP-1".into();
            r.scale = 1.5;
        });
        let rule = hyprland_monitor_rule(&r, &RuleOverrides { scale: None, mirror_of: Some("eDP-1".into()) });
        assert_eq!(rule.scale, "1.5");
        assert_eq!(rule.mirror, "eDP-1");
    }

    #[test]
    fn hyprland_rule_clears_a_mirror_with_an_empty_source() {
        let rule = hyprland_monitor_rule(&mirroring("DP-1", "eDP-1"), &RuleOverrides { scale: None, mirror_of: Some(String::new()) });
        assert_eq!(rule.mirror, "");
    }

    #[test]
    fn hyprland_rule_falls_back_to_preferred_without_a_mode() {
        let r = row(|r| {
            r.name = "DP-1".into();
            r.width = 0;
            r.height = 0;
            r.refresh = 0.0;
        });
        assert_eq!(hyprland_monitor_rule(&r, &RuleOverrides::default()).mode, "preferred");
    }

    #[test]
    fn hyprland_rule_is_an_hl_monitor_call() {
        assert_eq!(
            lua(&uhd("DP-1"), &scale_override(2.0)),
            "hl.monitor({ output = \"DP-1\", mode = \"3840x2160@60\", position = \"auto\", scale = 2, transform = 0, bitdepth = 8, cm = \"srgb\", sdrbrightness = 1, sdrsaturation = 1, mirror = \"\" })"
        );
        assert!(lua(&mirroring("DP-1", "eDP-1"), &scale_override(2.0)).contains("mirror = \"eDP-1\" })"));
    }

    #[test]
    fn enable_and_disable_lua() {
        assert_eq!(hyprland_enabled_lua("DP-1", false), "hl.monitor({ output = \"DP-1\", disabled = true })");
        assert_eq!(
            hyprland_enabled_lua("DP-1", true),
            "hl.monitor({ output = \"DP-1\", disabled = false, mode = \"preferred\", position = \"auto\", scale = \"auto\" })"
        );
    }

    // outputCardLabel (tst_display_outputs)

    fn section(blob: &str, name: &str) -> String {
        let lines: Vec<&str> = blob.split('\n').collect();
        let marker = format!("@{name}");
        let Some(start) = lines.iter().position(|l| *l == marker) else {
            return String::new();
        };
        lines[start + 1..].iter().take_while(|l| !l.starts_with('@')).copied().collect::<Vec<_>>().join("\n")
    }

    const HYBRID: &str = include_str!("../../tests/fixtures/gpu-hybrid.txt");
    const SINGLE: &str = include_str!("../../tests/fixtures/gpu-single.txt");

    #[test]
    fn hybrid_hdmi_reads_discrete_nvidia() {
        let cards = gpu::parse_cards(&section(HYBRID, "drm"));
        assert_eq!(output_card_label("HDMI-A-1", &cards), "NVIDIA / DISCRETE");
    }

    #[test]
    fn hybrid_edp_reads_integrated_intel() {
        let cards = gpu::parse_cards(&section(HYBRID, "drm"));
        assert_eq!(output_card_label("eDP-1", &cards), "Intel / INTEGRATED");
    }

    #[test]
    fn hybrid_unknown_connector_yields_no_annotation() {
        let cards = gpu::parse_cards(&section(HYBRID, "drm"));
        assert_eq!(output_card_label("DP-99", &cards), "");
    }

    #[test]
    fn single_card_suppresses_annotation() {
        let cards = gpu::parse_cards(&section(SINGLE, "drm"));
        assert_eq!(cards.len(), 1);
        assert_eq!(output_card_label("eDP-1", &cards), "");
    }

    #[test]
    fn no_cards_suppresses_annotation() {
        assert_eq!(output_card_label("eDP-1", &[]), "");
    }
}
