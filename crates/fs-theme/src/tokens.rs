//! The token maths from `shell/Theme/tokens.js` (DESIGN.md §1): type and
//! spacing scales, the radius ladder, the motion families and the deform
//! constants. `docs/DESIGN.md` and tokens.js carry the reasoning behind each
//! number; this file only has to agree with them.

use serde_json::Value;

/// libadwaita's ramp as ratios of the body size (`widgets/_labels.scss`):
/// `.caption`, a row subtitle's `smaller`, `.body`, `.heading`, `.title-4`,
/// `.title-3` and `.title-1`. `displayLarge` has no class; the clocks size
/// their own digits.
pub const FONT_MULTIPLIERS: [(&str, f64); 8] = [
    ("caption", 0.82),
    ("bodySmall", 0.833),
    ("body", 1.0),
    ("subtitle", 1.0),
    ("title", 1.18),
    ("heading", 1.36),
    ("display", 1.81),
    ("displayLarge", 2.333),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontTokens {
    pub base_size: f64,
    pub caption: f64,
    pub body_small: f64,
    pub body: f64,
    pub subtitle: f64,
    pub title: f64,
    pub heading: f64,
    pub display: f64,
    pub display_large: f64,
}

/// The type ramp at the body size the shell runs at.
pub fn font_tokens() -> FontTokens {
    let base_size = 13.0;
    let at = |name: &str| {
        let (_, m) = FONT_MULTIPLIERS.iter().find(|(k, _)| *k == name).expect("every FontTokens field has a ratio");
        fs_js::round(base_size * m)
    };
    FontTokens {
        base_size,
        caption: at("caption"),
        body_small: at("bodySmall"),
        body: at("body"),
        subtitle: at("subtitle"),
        title: at("title"),
        heading: at("heading"),
        display: at("display"),
        display_large: at("displayLarge"),
    }
}

/// Every spacing token at the one scale the shell runs at: libadwaita's
/// grid of 3 and 6 (`xxs` aside), then the semantic tokens. The struct, its
/// string lookup and its values come from this one list.
macro_rules! spacing {
    ($($field:ident $name:literal $value:literal,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Space {
            $(pub $field: f64,)*
        }

        impl Space {
            pub const TOKENS: Self = Self { $($field: $value,)* };

            /// A token by its tokens.js name, for the readers (IPC dumps, tests)
            /// that address them by string.
            pub fn get(&self, name: &str) -> Option<f64> {
                match name {
                    $($name => Some(self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

spacing! {
    xxs "xxs" 2.0,
    xs "xs" 3.0,
    sm "sm" 6.0,
    md "md" 9.0,
    lg "lg" 12.0,
    xl "xl" 18.0,
    xxl "xxl" 24.0,
    huge "huge" 36.0,
    control_gap "controlGap" 6.0,
    control_padding_x "controlPaddingX" 10.0,
    control_padding_y "controlPaddingY" 5.0,
    control_height "controlHeight" 34.0,
    bar_cell_height "barCellHeight" 28.0,
    bar_cell_width "barCellWidth" 44.0,
    bar_margin "barMargin" 6.0,
    popup_row_height "popupRowHeight" 32.0,
    row_gap "rowGap" 4.0,
    icon_gap "iconGap" 6.0,
    panel_padding "panelPadding" 12.0,
    section_gap "sectionGap" 18.0,
    screen_padding "screenPadding" 12.0,
    track_thickness "trackThickness" 6.0,
    popup_width_narrow "popupWidthNarrow" 320.0,
    popup_width_default "popupWidthDefault" 380.0,
    popup_width_wide "popupWidthWide" 480.0,
    popup_width_menu "popupWidthMenu" 560.0,
    popup_width_menu_split "popupWidthMenuSplit" 840.0,
    popup_width_menu_app "popupWidthMenuApp" 900.0,
    popup_width_bubble "popupWidthBubble" 332.0,
    popup_height_menu "popupHeightMenu" 520.0,
    popup_height_menu_split "popupHeightMenuSplit" 462.0,
    popup_height_menu_app "popupHeightMenuApp" 720.0,
    switcher_thumb "switcherThumb" 128.0,
    switcher_inset "switcherInset" 64.0,
    keycap_height "keycapHeight" 20.0,
    sidebar_width "sidebarWidth" 154.0,
    sidebar_rail "sidebarRail" 60.0,
    // The panel language's 4px grid (specs/2026-10-30-panel-language.md §2),
    // Pond's numbers rounded onto it.
    panel_inset "panelInset" 20.0,
    panel_row_inset "panelRowInset" 8.0,
    panel_row "panelRow" 44.0,
    panel_row_compact "panelRowCompact" 40.0,
    panel_gap "panelGap" 8.0,
    panel_section "panelSection" 16.0,
    panel_icon "panelIcon" 20.0,
    panel_mark "panelMark" 16.0,
    panel_width "panelWidth" 320.0,
    panel_width_wide "panelWidthWide" 400.0,
    // The wallpaper picker's narrowest 16:9 tile; columns are as many as fit.
    picker_tile "pickerTile" 180.0,
}

/// The launcher's counts and shares, unscaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launcher {
    pub height_share: f64,
    pub app_height_share: f64,
    /// The field's type, a ratio on the base size. Sized by x-height, not
    /// em: Pond's field (Gramercy at 26px) has an x-height 1.46 times its
    /// rows', which our sans (x-height 0.53 of the em) reaches at 21px.
    pub field_size: f64,
    /// A row's words, the title, subtitle and accessory alike, a ratio on
    /// the base size. Pond's 17.3px rows carry a 7.8px x-height, which our
    /// sans reaches at 15px.
    pub row_size: f64,
    pub row_height: f64,
    pub row_icon: f64,
    /// The card's one width: a preview pane or the actions column splits
    /// it rather than widening it.
    pub width: f64,
    /// The field's band, whether a mode token stands in it or not.
    pub field_band: f64,
}

pub const LAUNCHER: Launcher = Launcher {
    height_share: 0.6,
    app_height_share: 0.82,
    field_size: 21.0 / 13.0,
    row_size: 15.0 / 13.0,
    row_height: 48.0,
    row_icon: 24.0,
    width: 640.0,
    field_band: 64.0,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LetterSpacing {
    pub meta: f64,
    pub wide: f64,
    pub display: f64,
}

pub const LETTER_SPACING: LetterSpacing = LetterSpacing { meta: 1.0, wide: 2.0, display: 6.0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub normal: f64,
    pub medium: f64,
    pub semibold: f64,
    pub bold: f64,
    pub heavy: f64,
}

pub const WEIGHTS: Weights = Weights { normal: 400.0, medium: 500.0, semibold: 600.0, bold: 700.0, heavy: 800.0 };

/// Holds a settings number inside a range. Anything that is not a finite
/// number resolves to `fallback` rather than to `min`, which for an alpha
/// would be a surface nobody can see.
pub fn clamp(value: Option<&Value>, min: f64, max: f64, fallback: f64) -> f64 {
    match value {
        None | Some(Value::Null) => fallback,
        Some(v) => {
            let n = fs_js::to_number(Some(v));
            if n.is_finite() { n.min(max).max(min) } else { fallback }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Radii {
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    pub xl: f64,
}

impl Radii {
    pub fn step(&self, name: &str) -> Option<f64> {
        match name {
            "sm" => Some(self.sm),
            "md" => Some(self.md),
            "lg" => Some(self.lg),
            "xl" => Some(self.xl),
            _ => None,
        }
    }
}

/// libadwaita's corners: `$check_radius`, `$button_radius`, `$card_radius`
/// and `$popover_radius` (`_common.scss`, `widgets/_checks.scss`).
pub const RADII: Radii = Radii { sm: 6.0, md: 9.0, lg: 12.0, xl: 15.0 };

/// A picture's corner: a quarter of its shorter side, capped at `sm` and
/// floored at 2, square when `sm` is.
pub fn cover_radius(sm: f64, extent: f64) -> f64 {
    if sm.is_nan() || sm <= 0.0 {
        return 0.0;
    }
    let e = if extent.is_finite() { extent } else { 0.0 };
    sm.min(fs_js::round(e / 4.0)).max(2.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub spatial_fast: f64,
    pub spatial: f64,
    pub spatial_slow: f64,
    pub effects_fast: f64,
    pub effects: f64,
    pub effects_slow: f64,
    pub emphasized: f64,
    pub reveal: f64,
    pub marquee_px_per_sec: f64,
    pub marquee_hold_ms: f64,
}

impl Motion {
    /// A family by its tokens.js name, which is how a table names a clock.
    pub fn family(&self, name: &str) -> Option<f64> {
        Some(match name {
            "spatialFast" => self.spatial_fast,
            "spatial" => self.spatial,
            "spatialSlow" => self.spatial_slow,
            "effectsFast" => self.effects_fast,
            "effects" => self.effects,
            "effectsSlow" => self.effects_slow,
            "emphasized" => self.emphasized,
            "reveal" => self.reveal,
            "marqueePxPerSec" => self.marquee_px_per_sec,
            "marqueeHoldMs" => self.marquee_hold_ms,
            _ => return None,
        })
    }
}

/// `enabled: false` zeroes every duration and leaves the marquee's rate and
/// hold alone: its consumer gates the scroll on `motionEnabled` itself.
pub fn motion_tokens(enabled: bool) -> Motion {
    let d = |ms: f64| if enabled { ms } else { 0.0 };
    Motion {
        spatial_fast: d(350.0),
        spatial: d(500.0),
        spatial_slow: d(650.0),
        effects_fast: d(150.0),
        effects: d(200.0),
        effects_slow: d(300.0),
        emphasized: d(400.0),
        reveal: d(400.0),
        marquee_px_per_sec: 30.0,
        marquee_hold_ms: 2000.0,
    }
}

/// The breathing pulse's length (`pulseDuration`): one leg, 1 to 0.4.
pub const PULSE_MS: f64 = 900.0;

/// One cubic bezier per kind as Qt's `easing.bezierCurve` takes it: control
/// points, then the (1, 1) end point. `emphasized` is two segments.
pub const MOTION_CURVES: [(&str, &[f64]); 8] = [
    ("spatialFast", &[0.42, 1.67, 0.21, 0.9, 1.0, 1.0]),
    ("spatial", &[0.38, 1.21, 0.22, 1.0, 1.0, 1.0]),
    ("spatialSlow", &[0.39, 1.29, 0.35, 0.98, 1.0, 1.0]),
    ("effectsFast", &[0.31, 0.94, 0.34, 1.0, 1.0, 1.0]),
    ("effects", &[0.34, 0.8, 0.34, 1.0, 1.0, 1.0]),
    ("effectsSlow", &[0.34, 0.88, 0.34, 1.0, 1.0, 1.0]),
    (
        "emphasized",
        &[0.05, 0.0, 2.0 / 15.0, 0.06, 1.0 / 6.0, 0.4, 5.0 / 24.0, 0.82, 0.25, 1.0, 1.0, 1.0],
    ),
    ("emphasizedDecel", &[0.05, 0.7, 0.1, 1.0, 1.0, 1.0]),
];

/// A table's curve: control points the way [`MOTION_CURVES`] writes them,
/// or CSS `linear()`'s `(t, v)` stops, which may pass 1 for an overshoot.
#[derive(Clone, Debug, PartialEq)]
pub enum Easing {
    Bezier(Vec<f64>),
    Stops(Vec<(f64, f64)>),
}

/// The curve a table or a caller names, `None` for an unknown name.
pub fn curve(name: &str) -> Option<&'static [f64]> {
    MOTION_CURVES.iter().find(|(k, _)| *k == name).map(|(_, c)| *c)
}

/// `reveal` rides `effectsSlow`'s curve, and an unknown kind falls back to
/// the spatial curve rather than to Qt's linear default.
pub fn motion_curve(kind: &str) -> &'static [f64] {
    if kind == "reveal" {
        return curve("effectsSlow").expect("effectsSlow is in MOTION_CURVES");
    }
    curve(kind).unwrap_or_else(|| curve("spatial").expect("spatial is in MOTION_CURVES"))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deform {
    pub max_stretch: f64,
    pub dead_band: f64,
    pub stiffness: f64,
    pub damping: f64,
    pub epsilon: f64,
}

/// caelestia's constants verbatim (blobrect.cpp at ce84c7b).
pub const DEFORM: Deform = Deform { max_stretch: 0.35, dead_band: 5.0, stiffness: 200.0, damping: 16.0, epsilon: 0.002 };

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn font_tokens_at_default_base_match_design_doc_table() {
        let f = font_tokens();
        assert_eq!(f.caption, 11.0);
        assert_eq!(f.body_small, 11.0);
        assert_eq!(f.body, 13.0);
        assert_eq!(f.subtitle, 13.0);
        assert_eq!(f.title, 15.0);
        assert_eq!(f.heading, 18.0);
        assert_eq!(f.display, 24.0);
        assert_eq!(f.display_large, 30.0);
    }

    #[test]
    fn spacing_tokens_at_scale_one_are_the_adwaita_grid() {
        let s = Space::TOKENS;
        assert_eq!(
            [s.xxs, s.xs, s.sm, s.md, s.lg, s.xl, s.xxl, s.huge],
            [2.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0, 36.0]
        );
        assert_eq!(s.track_thickness, 6.0);
    }

    #[test]
    fn semantic_spacing_tokens_match_the_adwaita_table() {
        let s = Space::TOKENS;
        assert_eq!(s.control_height, 34.0);
        assert_eq!(s.control_padding_x, 10.0);
        assert_eq!(s.control_padding_y, 5.0);
        assert_eq!(s.control_gap, 6.0);
        assert_eq!(s.icon_gap, 6.0);
        assert_eq!(s.popup_row_height, 32.0);
        assert_eq!(s.section_gap, 18.0);
        assert_eq!(s.bar_cell_height, 28.0);
        assert_eq!(s.bar_cell_width, 44.0);
        assert_eq!(s.bar_margin, 6.0);
        assert_eq!(s.row_gap, 4.0);
        assert_eq!(s.panel_padding, 12.0);
        assert_eq!(s.screen_padding, 12.0);
        assert_eq!(s.popup_width_narrow, 320.0);
        assert_eq!(s.popup_width_default, 380.0);
        assert_eq!(s.popup_width_wide, 480.0);
        assert_eq!(s.popup_width_menu, 560.0);
        assert_eq!(s.popup_width_menu_split, 840.0);
        assert_eq!(s.popup_width_menu_app, 900.0);
    }

    #[test]
    fn radii_are_the_libadwaita_ladder() {
        assert_eq!((RADII.sm, RADII.md, RADII.lg, RADII.xl), (6.0, 9.0, 12.0, 15.0));
        assert_eq!(RADII.step("lg"), Some(12.0));
        assert_eq!(RADII.step("pill"), None);
    }

    #[test]
    fn cover_radius_is_a_quarter_of_the_slot() {
        assert_eq!(cover_radius(RADII.sm, 17.0), 4.0);
        assert_eq!(cover_radius(RADII.sm, 12.0), 3.0);
    }

    #[test]
    fn cover_radius_caps_at_the_small_step() {
        assert_eq!(cover_radius(RADII.sm, 96.0), RADII.sm);
        assert_eq!(cover_radius(RADII.sm, 400.0), RADII.sm);
    }

    #[test]
    fn cover_radius_floors_at_two_and_never_reaches_a_circle() {
        assert_eq!(cover_radius(RADII.sm, 4.0), 2.0);
        assert_eq!(cover_radius(RADII.sm, 0.0), 2.0);
        assert!(cover_radius(RADII.sm, 17.0) < 17.0 / 2.0);
        assert!(cover_radius(RADII.sm, 96.0) < 96.0 / 2.0);
    }

    #[test]
    fn cover_radius_is_square_at_a_zero_step() {
        assert_eq!(cover_radius(0.0, 96.0), 0.0);
    }

    #[test]
    fn weights_carry_adwaita_bold_and_heavy() {
        assert_eq!(WEIGHTS.normal, 400.0);
        assert_eq!(WEIGHTS.medium, 500.0);
        assert_eq!(WEIGHTS.semibold, 600.0);
        assert_eq!(WEIGHTS.bold, 700.0);
        assert_eq!(WEIGHTS.heavy, 800.0);
    }

    #[test]
    fn clamp_passes_a_value_already_in_range() {
        assert_eq!(clamp(Some(&json!(0.85)), 0.0, 1.0, 0.85), 0.85);
        assert_eq!(clamp(Some(&json!(0)), 0.0, 1.0, 0.85), 0.0);
        assert_eq!(clamp(Some(&json!(1)), 0.0, 1.0, 0.85), 1.0);
    }

    #[test]
    fn clamp_holds_a_value_at_the_bounds() {
        assert_eq!(clamp(Some(&json!(-3)), 0.0, 1.0, 0.85), 0.0);
        assert_eq!(clamp(Some(&json!(42)), 0.0, 1.0, 0.85), 1.0);
    }

    // JSON has no NaN or Infinity, so those two arrive as the strings a
    // settings file could carry.
    #[test]
    fn clamp_falls_back_on_anything_that_is_not_a_number() {
        assert_eq!(clamp(None, 0.0, 1.0, 0.85), 0.85);
        assert_eq!(clamp(Some(&Value::Null), 0.0, 1.0, 0.85), 0.85);
        assert_eq!(clamp(Some(&json!("opaque")), 0.0, 1.0, 0.85), 0.85);
        assert_eq!(clamp(Some(&json!("NaN")), 0.0, 1.0, 0.85), 0.85);
        assert_eq!(clamp(Some(&json!("Infinity")), 0.0, 1.0, 0.85), 0.85);
    }

    #[test]
    fn clamp_accepts_a_numeric_string() {
        assert_eq!(clamp(Some(&json!("0.5")), 0.0, 1.0, 0.85), 0.5);
    }

    #[test]
    fn motion_tokens_emphasized_is_the_indicator_step() {
        let m = motion_tokens(true);
        assert_eq!(m.emphasized, 400.0);
        assert!(m.emphasized > m.effects);
    }

    #[test]
    fn motion_tokens_disabled_zeroes_emphasized_too() {
        assert_eq!(motion_tokens(false).emphasized, 0.0);
    }

    #[test]
    fn motion_tokens_reveal_is_the_400ms_carve_out() {
        assert_eq!(motion_tokens(true).reveal, 400.0);
    }

    #[test]
    fn motion_tokens_disabled_zeroes_reveal_too() {
        assert_eq!(motion_tokens(false).reveal, 0.0);
    }

    #[test]
    fn motion_tokens_marquee_pace_matches_the_owner_brief() {
        let m = motion_tokens(true);
        assert_eq!(m.marquee_px_per_sec, 30.0);
        assert_eq!(m.marquee_hold_ms, 2000.0);
    }

    #[test]
    fn motion_tokens_marquee_is_not_zeroed_when_disabled() {
        let (on, off) = (motion_tokens(true), motion_tokens(false));
        assert_eq!(off.marquee_px_per_sec, on.marquee_px_per_sec);
        assert_eq!(off.marquee_hold_ms, on.marquee_hold_ms);
    }

    #[test]
    fn motion_tokens_pin_the_expressive_durations() {
        let m = motion_tokens(true);
        assert_eq!(
            [m.spatial_fast, m.spatial, m.spatial_slow, m.effects_fast, m.effects, m.effects_slow, m.emphasized, m.reveal],
            [350.0, 500.0, 650.0, 150.0, 200.0, 300.0, 400.0, 400.0]
        );
    }

    #[test]
    fn motion_tokens_disabled_zeroes_every_expressive_duration() {
        let m = motion_tokens(false);
        assert_eq!(
            [m.spatial_fast, m.spatial, m.spatial_slow, m.effects_fast, m.effects, m.effects_slow, m.emphasized, m.reveal],
            [0.0; 8]
        );
    }

    #[test]
    fn motion_curves_end_at_one_one() {
        for (kind, c) in MOTION_CURVES {
            assert_eq!(c.len() % 6, 0, "{kind} is not whole segments");
            assert_eq!(c[c.len() - 2], 1.0, "{kind} does not end at x 1");
            assert_eq!(c[c.len() - 1], 1.0, "{kind} does not end at y 1");
        }
        assert_eq!(curve("emphasized").unwrap().len(), 12);
        assert_eq!(curve("spatial").unwrap().len(), 6);
    }

    #[test]
    fn spatial_curves_overshoot_and_effects_curves_do_not() {
        for kind in ["spatialFast", "spatial", "spatialSlow"] {
            let c = curve(kind).unwrap();
            assert!(c[1] > 1.0 || c[3] > 1.0, "{kind} has no control point above 1");
        }
        for kind in ["effectsFast", "effects", "effectsSlow"] {
            let e = curve(kind).unwrap();
            assert!(e[1] <= 1.0 && e[3] <= 1.0, "{kind} overshoots");
        }
    }

    #[test]
    fn motion_curve_resolves_reveal_and_falls_back() {
        assert_eq!(motion_curve("reveal"), curve("effectsSlow").unwrap());
        assert_eq!(motion_curve("spatialSlow"), curve("spatialSlow").unwrap());
        assert_eq!(motion_curve("nonsense"), curve("spatial").unwrap());
    }

    #[test]
    fn deform_constants_match_the_ported_mechanics() {
        assert_eq!(DEFORM.max_stretch, 0.35);
        assert_eq!(DEFORM.dead_band, 5.0);
        assert_eq!(DEFORM.stiffness, 200.0);
        assert_eq!(DEFORM.damping, 16.0);
        assert_eq!(DEFORM.epsilon, 0.002);
    }
}
