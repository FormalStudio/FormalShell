//! Output/display model for the display panel: one row contract the
//! compositor backend reads its outputs into, plus every derivation the panel
//! renders, sort order, the mode/scale/status labels, what a mirror would
//! actually do, and whether an output may be switched off at all.
//!
//! `name` is the compositor's own output name and stays an opaque string end
//! to end: nothing here parses or compares it numerically.
//!
//! `width`/`height`/`refresh` describe the CURRENT mode in physical pixels and
//! Hz. A disabled output reports zeros for all three: Hyprland has no mode to
//! report for one, and inventing its last known mode would be exactly the
//! stubbed value the honest-state rule forbids. `mirror_of` is "" when the
//! output isn't mirroring anything.

use fs_js as js;
use crate::monitor::gpu;

/// The scale slider's range and quantization. 0.25 is binary-exact, so
/// `quantize_scale` never accumulates float drift across repeated steps.
pub(crate) const SCALE_MIN: f64 = 1.0;
pub(crate) const SCALE_MAX: f64 = 3.0;
pub(crate) const SCALE_STEP: f64 = 0.25;

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

/// Fixed-decimal, then back through a number so a whole scale prints "1"
/// rather than "1.00" and a two-place refresh prints "60" rather than "60.00".
pub fn trim_number(value: f64, decimals: usize) -> String {
    js::num_str(js::parse_number(&js::to_fixed(value, decimals)))
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

pub(crate) fn enabled_count(rows: &[Output]) -> usize {
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

pub(crate) fn is_hdr_preset(cm: &str) -> bool {
    cm == "hdr" || cm == "hdredid"
}

/// The colour fields of a monitor rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Color {
    pub cm: String,
    pub bitdepth: i64,
    pub sdrbrightness: f64,
    pub sdrsaturation: f64,
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
