//! The token maths from `shell/Theme/tokens.js` (DESIGN.md §1): type and
//! spacing scales, the radius ladder, the motion families and the deform
//! constants. `docs/DESIGN.md` and tokens.js carry the reasoning behind each
//! number; this file only has to agree with them.

use serde_json::Value;

/// `Math.round`: halves go up, toward positive infinity, where Rust's
/// `round` takes them away from zero.
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// JavaScript's `Number(value)` for a JSON value: a numeric string reads as
/// its number, `null` and `""` as 0, a bool as 0 or 1, anything else NaN.
pub fn js_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                return 0.0;
            }
            match t {
                "Infinity" | "+Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                _ if t.chars().any(|c| c.is_ascii_alphabetic() && c != 'e' && c != 'E') => f64::NAN,
                _ => t.parse().unwrap_or(f64::NAN),
            }
        }
        Some(Value::Array(_) | Value::Object(_)) => f64::NAN,
    }
}

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

pub fn font_scale(base_size: f64) -> f64 {
    base_size / 13.0
}

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

pub fn font_tokens(base_size: f64) -> FontTokens {
    let at = |name: &str| {
        let (_, m) = FONT_MULTIPLIERS.iter().find(|(k, _)| *k == name).expect("every FontTokens field has a ratio");
        js_round(base_size * m)
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

/// libadwaita's grid of 3 and 6, `xxs` aside.
pub const SPACING_BASE: [(&str, f64); 8] = [
    ("xxs", 2.0),
    ("xs", 3.0),
    ("sm", 6.0),
    ("md", 9.0),
    ("lg", 12.0),
    ("xl", 18.0),
    ("xxl", 24.0),
    ("huge", 36.0),
];

pub const SEMANTIC_SPACING_BASE: [(&str, f64); 39] = [
    ("controlGap", 6.0),
    ("controlPaddingX", 10.0),
    ("controlPaddingY", 5.0),
    ("controlHeight", 34.0),
    ("barCellHeight", 28.0),
    ("barCellWidth", 44.0),
    ("barMargin", 6.0),
    ("popupRowHeight", 32.0),
    ("rowGap", 4.0),
    ("iconGap", 6.0),
    ("panelPadding", 12.0),
    ("sectionGap", 18.0),
    ("screenPadding", 12.0),
    ("trackThickness", 6.0),
    ("popupWidthNarrow", 320.0),
    ("popupWidthDefault", 380.0),
    ("popupWidthWide", 480.0),
    ("popupWidthMenu", 560.0),
    ("popupWidthMenuSplit", 840.0),
    ("popupWidthMenuApp", 900.0),
    ("popupWidthBubble", 332.0),
    ("popupHeightMenu", 520.0),
    ("popupHeightMenuSplit", 462.0),
    ("popupHeightMenuApp", 720.0),
    ("switcherThumb", 128.0),
    ("switcherInset", 64.0),
    ("keycapHeight", 20.0),
    ("sidebarWidth", 154.0),
    ("sidebarRail", 60.0),
    // The panel language's 4px grid (specs/2026-10-30-panel-language.md §2),
    // Pond's numbers rounded onto it.
    ("panelInset", 20.0),
    ("panelRowInset", 8.0),
    ("panelRow", 44.0),
    ("panelRowCompact", 40.0),
    ("panelGap", 8.0),
    ("panelSection", 16.0),
    ("panelIcon", 20.0),
    ("panelMark", 16.0),
    ("panelWidth", 320.0),
    ("panelWidthWide", 400.0),
];

/// `spacingTokens(scale)`: every scale step and semantic spacing token,
/// rounded at the given scale.
#[derive(Clone, Debug, PartialEq)]
pub struct Space {
    pub xxs: f64,
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub lg: f64,
    pub xl: f64,
    pub xxl: f64,
    pub huge: f64,
    pub control_gap: f64,
    pub control_padding_x: f64,
    pub control_padding_y: f64,
    pub control_height: f64,
    pub bar_cell_height: f64,
    pub bar_cell_width: f64,
    pub bar_margin: f64,
    pub popup_row_height: f64,
    pub row_gap: f64,
    pub icon_gap: f64,
    pub panel_padding: f64,
    pub section_gap: f64,
    pub screen_padding: f64,
    pub track_thickness: f64,
    pub popup_width_narrow: f64,
    pub popup_width_default: f64,
    pub popup_width_wide: f64,
    pub popup_width_menu: f64,
    pub popup_width_menu_split: f64,
    pub popup_width_menu_app: f64,
    pub popup_width_bubble: f64,
    pub popup_height_menu: f64,
    pub popup_height_menu_split: f64,
    pub popup_height_menu_app: f64,
    pub switcher_thumb: f64,
    pub switcher_inset: f64,
    pub keycap_height: f64,
    pub sidebar_width: f64,
    pub sidebar_rail: f64,
    pub panel_inset: f64,
    pub panel_row_inset: f64,
    pub panel_row: f64,
    pub panel_row_compact: f64,
    pub panel_gap: f64,
    pub panel_section: f64,
    pub panel_icon: f64,
    pub panel_mark: f64,
    pub panel_width: f64,
    pub panel_width_wide: f64,
}

impl Space {
    /// A token by its tokens.js name, for the readers (IPC dumps, tests)
    /// that address them by string.
    pub fn get(&self, name: &str) -> Option<f64> {
        Some(match name {
            "xxs" => self.xxs,
            "xs" => self.xs,
            "sm" => self.sm,
            "md" => self.md,
            "lg" => self.lg,
            "xl" => self.xl,
            "xxl" => self.xxl,
            "huge" => self.huge,
            "controlGap" => self.control_gap,
            "controlPaddingX" => self.control_padding_x,
            "controlPaddingY" => self.control_padding_y,
            "controlHeight" => self.control_height,
            "barCellHeight" => self.bar_cell_height,
            "barCellWidth" => self.bar_cell_width,
            "barMargin" => self.bar_margin,
            "popupRowHeight" => self.popup_row_height,
            "rowGap" => self.row_gap,
            "iconGap" => self.icon_gap,
            "panelPadding" => self.panel_padding,
            "sectionGap" => self.section_gap,
            "screenPadding" => self.screen_padding,
            "trackThickness" => self.track_thickness,
            "popupWidthNarrow" => self.popup_width_narrow,
            "popupWidthDefault" => self.popup_width_default,
            "popupWidthWide" => self.popup_width_wide,
            "popupWidthMenu" => self.popup_width_menu,
            "popupWidthMenuSplit" => self.popup_width_menu_split,
            "popupWidthMenuApp" => self.popup_width_menu_app,
            "popupWidthBubble" => self.popup_width_bubble,
            "popupHeightMenu" => self.popup_height_menu,
            "popupHeightMenuSplit" => self.popup_height_menu_split,
            "popupHeightMenuApp" => self.popup_height_menu_app,
            "switcherThumb" => self.switcher_thumb,
            "switcherInset" => self.switcher_inset,
            "keycapHeight" => self.keycap_height,
            "sidebarWidth" => self.sidebar_width,
            "sidebarRail" => self.sidebar_rail,
            "panelInset" => self.panel_inset,
            "panelRowInset" => self.panel_row_inset,
            "panelRow" => self.panel_row,
            "panelRowCompact" => self.panel_row_compact,
            "panelGap" => self.panel_gap,
            "panelSection" => self.panel_section,
            "panelIcon" => self.panel_icon,
            "panelMark" => self.panel_mark,
            "panelWidth" => self.panel_width,
            "panelWidthWide" => self.panel_width_wide,
            _ => return None,
        })
    }
}

pub fn spacing_tokens(scale: f64) -> Space {
    let s = |name: &str| {
        let (_, v) = SPACING_BASE
            .iter()
            .chain(SEMANTIC_SPACING_BASE.iter())
            .find(|(k, _)| *k == name)
            .expect("every Space field has a base");
        js_round(v * scale)
    };
    Space {
        xxs: s("xxs"),
        xs: s("xs"),
        sm: s("sm"),
        md: s("md"),
        lg: s("lg"),
        xl: s("xl"),
        xxl: s("xxl"),
        huge: s("huge"),
        control_gap: s("controlGap"),
        control_padding_x: s("controlPaddingX"),
        control_padding_y: s("controlPaddingY"),
        control_height: s("controlHeight"),
        bar_cell_height: s("barCellHeight"),
        bar_cell_width: s("barCellWidth"),
        bar_margin: s("barMargin"),
        popup_row_height: s("popupRowHeight"),
        row_gap: s("rowGap"),
        icon_gap: s("iconGap"),
        panel_padding: s("panelPadding"),
        section_gap: s("sectionGap"),
        screen_padding: s("screenPadding"),
        track_thickness: s("trackThickness"),
        popup_width_narrow: s("popupWidthNarrow"),
        popup_width_default: s("popupWidthDefault"),
        popup_width_wide: s("popupWidthWide"),
        popup_width_menu: s("popupWidthMenu"),
        popup_width_menu_split: s("popupWidthMenuSplit"),
        popup_width_menu_app: s("popupWidthMenuApp"),
        popup_width_bubble: s("popupWidthBubble"),
        popup_height_menu: s("popupHeightMenu"),
        popup_height_menu_split: s("popupHeightMenuSplit"),
        popup_height_menu_app: s("popupHeightMenuApp"),
        switcher_thumb: s("switcherThumb"),
        switcher_inset: s("switcherInset"),
        keycap_height: s("keycapHeight"),
        sidebar_width: s("sidebarWidth"),
        sidebar_rail: s("sidebarRail"),
        panel_inset: s("panelInset"),
        panel_row_inset: s("panelRowInset"),
        panel_row: s("panelRow"),
        panel_row_compact: s("panelRowCompact"),
        panel_gap: s("panelGap"),
        panel_section: s("panelSection"),
        panel_icon: s("panelIcon"),
        panel_mark: s("panelMark"),
        panel_width: s("panelWidth"),
        panel_width_wide: s("panelWidthWide"),
    }
}

/// The launcher's counts and shares, unscaled.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launcher {
    pub height_share: f64,
    pub app_height_share: f64,
    pub picker_columns: u32,
    pub emoji_columns: u32,
    /// The most rows `Recent` lists at rest.
    pub recent: u32,
    /// The field's type, a ratio on the base size, past the ramp's
    /// `display`: Pond's field size in the shell's sans.
    pub field_size: f64,
    pub row_height: f64,
    pub row_icon: f64,
    /// The card's one width: a preview pane or the actions column splits
    /// it rather than widening it.
    pub width: f64,
    /// The field's band, whether a mode token stands in it or not.
    pub field_band: f64,
    /// The tiles the opened state shows, and their height.
    pub tiles: u32,
    pub tile_height: f64,
}

pub const LAUNCHER: Launcher = Launcher {
    height_share: 0.6,
    app_height_share: 0.82,
    picker_columns: 4,
    emoji_columns: 8,
    recent: 8,
    field_size: 2.0,
    row_height: 48.0,
    row_icon: 24.0,
    width: 640.0,
    field_band: 64.0,
    tiles: 3,
    tile_height: 144.0,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LetterSpacing {
    pub meta: f64,
    pub wide: f64,
    pub display: f64,
}

pub fn letter_spacing_tokens(scale: f64) -> LetterSpacing {
    LetterSpacing { meta: js_round(scale), wide: js_round(2.0 * scale), display: js_round(6.0 * scale) }
}

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
            let n = js_number(Some(v));
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
    sm.min(js_round(e / 4.0)).max(2.0)
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
    fn font_scale_is_base_over_thirteen() {
        assert_eq!(font_scale(13.0), 1.0);
        assert_eq!(font_scale(26.0), 2.0);
    }

    #[test]
    fn font_tokens_at_default_base_match_design_doc_table() {
        let f = font_tokens(13.0);
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
    fn font_tokens_rescale_proportionally_with_base_size() {
        let f = font_tokens(16.0);
        assert_eq!(f.base_size, 16.0);
        assert_eq!(f.body, 16.0);
        assert_eq!(f.caption, js_round(16.0 * 0.82));
        assert_eq!(f.display, js_round(16.0 * 1.81));
    }

    #[test]
    fn spacing_tokens_at_scale_one_are_the_adwaita_grid() {
        let s = spacing_tokens(1.0);
        assert_eq!(
            [s.xxs, s.xs, s.sm, s.md, s.lg, s.xl, s.xxl, s.huge],
            [2.0, 3.0, 6.0, 9.0, 12.0, 18.0, 24.0, 36.0]
        );
        assert_eq!(s.track_thickness, 6.0);
    }

    #[test]
    fn semantic_spacing_tokens_match_the_adwaita_table() {
        let s = spacing_tokens(1.0);
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
    fn spacing_tokens_rescale_with_scale_factor() {
        let s = spacing_tokens(2.0);
        assert_eq!(s.sm, 12.0);
        assert_eq!(s.panel_padding, 24.0);
        assert_eq!(s.track_thickness, 12.0);
        assert_eq!(s.control_height, 68.0);
        assert_eq!(s.bar_margin, 12.0);
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
    fn letter_spacing_tokens_at_scale_one() {
        let l = letter_spacing_tokens(1.0);
        assert_eq!((l.meta, l.wide, l.display), (1.0, 2.0, 6.0));
    }

    #[test]
    fn letter_spacing_tokens_rescale_with_font_scale() {
        let l = letter_spacing_tokens(2.0);
        assert_eq!((l.meta, l.wide, l.display), (2.0, 4.0, 12.0));
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

    #[test]
    fn js_number_reads_like_javascript() {
        assert_eq!(js_number(Some(&json!(""))), 0.0);
        assert_eq!(js_number(Some(&json!(true))), 1.0);
        assert!(js_number(Some(&json!("down"))).is_nan());
        assert!(js_number(None).is_nan());
        assert_eq!(js_number(Some(&json!(" 2.5 "))), 2.5);
        assert_eq!(js_round(10.5), 11.0);
        assert_eq!(js_round(-2.5), -2.0);
    }
}
