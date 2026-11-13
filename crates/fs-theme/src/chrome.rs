//! `~/.config/hypr/formalshell-chrome.lua`, or the same values as data for a
//! compositor that takes them over its socket ([`wm_chrome`]): the
//! window rounding, whether the compositor blurs
//! behind the shell, the gaps, frame and cast the table's `window` role
//! asks for, and the shell's two clock families as the
//! curves and speeds its window animations take. Not a matugen template,
//! since none of it comes from the wallpaper.
//!
//! Every value is clamped to the range Hyprland's own option carries
//! (src/config/values/ConfigValues.cpp, 0.56): a value outside one fails the
//! whole `hl.config` call rather than one key.

use std::borrow::Cow;

use serde_json::{Map, Value};

use crate::style;
use crate::tokens;

/// What ThemeEngine hands the renderer: the two settings scalars as written
/// and the `window` role's two states straight off the live table.
#[derive(Clone, Debug, Default)]
pub struct Chrome {
    pub rounding: Option<Value>,
    pub blur: Option<Value>,
    pub window: Option<Map<String, Value>>,
    pub window_inactive: Option<Map<String, Value>>,
    /// `motion.enabled`.
    pub motion: bool,
    /// The panel edge, resolved for the palette and mode the table was read
    /// under. Only formalwm draws it.
    pub edge: fs_wm_proto::Edge,
}

impl Chrome {
    pub fn from_table(table: &Value, rounding: Value, blur: Value, motion: bool) -> Self {
        Self {
            rounding: Some(rounding),
            blur: Some(blur),
            motion,
            window: style::entry(table, style::Role::Window, Some("rest")).map(Cow::into_owned),
            window_inactive: style::entry(table, style::Role::Window, Some("inactive")).map(Cow::into_owned),
            edge: fs_wm_proto::Edge::default(),
        }
    }
}

/// Hyprland's `rounding` is a non-negative int. NaN falls through to 0.
fn rounding(value: Option<&Value>) -> i64 {
    let n = fs_js::round(fs_js::to_number(value));
    if n > 0.0 { n as i64 } else { 0 }
}

/// Anything but the boolean true is false: a stray string on a bool option
/// is rejected.
fn blur(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

fn int(value: Option<&Value>, min: i64, max: i64, fallback: i64) -> i64 {
    let n = fs_js::round(fs_js::to_number(value));
    if !n.is_finite() {
        return fallback;
    }
    (n as i64).clamp(min, max)
}

fn byte(alpha: Option<&Value>) -> String {
    let a = fs_js::to_number(alpha);
    if !a.is_finite() {
        return "ff".to_owned();
    }
    format!("{:02x}", fs_js::round(a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A colour as the table names it: a literal (`RRGGBBAA`), or a palette
/// role, which the compositor resolves against the colours it was given so
/// the wallpaper keeps moving it. A role therefore arrives at whatever alpha
/// the palette gave it.
enum Paint {
    Literal(String),
    Role(String),
}

fn paint(name: Option<&Value>, alpha: Option<&Value>) -> Paint {
    let name = match name {
        None | Some(Value::Null) => return Paint::Literal("00000000".to_owned()),
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Paint::Role(other.to_string()),
    };
    if name == "transparent" {
        return Paint::Literal("00000000".to_owned());
    }
    match style::literal(name) {
        Some(lit) => Paint::Literal(format!("{}{}", lit.trim_start_matches('#'), byte(alpha))),
        None => Paint::Role(name.to_owned()),
    }
}

/// Hyprland's `rgba(RRGGBBAA)`, or a role's name, a key into
/// formalshell-colors.lua's table.
fn color(name: Option<&Value>, alpha: Option<&Value>) -> String {
    match paint(name, alpha) {
        Paint::Literal(hex) => format!("rgba({hex})"),
        Paint::Role(role) => role,
    }
}

/// `#rrggbbaa`, or a role's name, a key into the colours sent beside it.
fn hex_color(name: Option<&Value>, alpha: Option<&Value>) -> String {
    match paint(name, alpha) {
        Paint::Literal(hex) => format!("#{hex}"),
        Paint::Role(role) => role,
    }
}

fn field<'a>(map: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    map.and_then(|m| m.get(key))
}

/// The window role's values, each clamped to Hyprland's range, with the
/// colours still as the table names them.
struct Window {
    rounding: i64,
    blur: bool,
    gaps_in: i64,
    gaps_out: i64,
    border_size: i64,
    border: (Option<Value>, Option<Value>),
    shadow: bool,
    range: i64,
    power: i64,
    offset: (i64, i64),
    shadow_color: (Option<Value>, Option<Value>),
    backdrop_color: (Option<Value>, Option<Value>),
}

fn window(chrome: &Chrome) -> Window {
    let focused = chrome.window.clone().map(Value::Object);
    let backdrop = chrome.window_inactive.clone().map(Value::Object);
    let border = field(focused.as_ref(), "border");
    let cast = field(focused.as_ref(), "shadow");
    let backdrop_cast = field(backdrop.as_ref(), "shadow");
    let offset = field(cast, "offset");
    let pair = |m: Option<&Value>| (field(m, "color").cloned(), field(m, "alpha").cloned());
    Window {
        rounding: rounding(chrome.rounding.as_ref()),
        blur: blur(chrome.blur.as_ref()),
        gaps_in: int(field(focused.as_ref(), "gapsIn"), 0, 100, 4),
        gaps_out: int(field(focused.as_ref(), "gapsOut"), 0, 100, 8),
        border_size: int(field(border, "width"), 0, 20, 1),
        border: pair(border),
        shadow: blur(field(cast, "enabled")),
        range: int(field(cast, "range"), 0, 100, 4),
        power: int(field(cast, "renderPower"), 1, 4, 3),
        offset: (int(offset.and_then(|o| o.get(0)), -250, 250, 0), int(offset.and_then(|o| o.get(1)), -250, 250, 0)),
        shadow_color: pair(cast),
        backdrop_color: pair(backdrop_cast),
    }
}

/// The six families as `motion` in formalshell-chrome.lua: each its own
/// `MOTION_CURVES` bezier, which Hyprland takes as one segment's two
/// control points (the end points are fixed at 0,0 and 1,1, the overshoot
/// in the control points' y as the shell's own), and its duration as
/// Hyprland's `speed`, in tenths of a second. `enabled` is
/// `motion.enabled`; the speeds stay the families' own with it off, since
/// Hyprland refuses a zero speed.
pub fn hyprland_motion(enabled: bool) -> String {
    let m = tokens::motion_tokens(true);
    let mut out = format!("  motion = {{\n    enabled = {enabled},\n");
    for name in FAMILIES {
        let c = tokens::curve(name).expect("every family is in MOTION_CURVES");
        let speed = m.family(name).expect("every family has a duration") / 100.0;
        out.push_str(&format!(
            "    {name} = {{ curve = {{ {{ {}, {} }}, {{ {}, {} }} }}, speed = {} }},\n",
            c[0], c[1], c[2], c[3], speed
        ));
    }
    out.push_str("  },\n");
    out
}

const FAMILIES: [&str; 6] = ["spatialFast", "spatial", "spatialSlow", "effectsFast", "effects", "effectsSlow"];

/// Every value [`hyprland_chrome`] writes, as the `chrome` of formalwm's
/// `theme.set`: the same clamps, a colour as [`hex_color`], and each motion
/// family its bezier's control points and its duration in milliseconds.
pub fn wm_chrome(chrome: &Chrome) -> fs_wm_proto::Chrome {
    let w = window(chrome);
    let hex = |(name, alpha): &(Option<Value>, Option<Value>)| hex_color(name.as_ref(), alpha.as_ref());
    let m = tokens::motion_tokens(true);
    let family = |name: &str| {
        let c = tokens::curve(name).expect("every family is in MOTION_CURVES");
        let ms = m.family(name).expect("every family has a duration");
        fs_wm_proto::Family { curve: [c[0], c[1], c[2], c[3]], ms }
    };
    let [spatial_fast, spatial, spatial_slow, effects_fast, effects, effects_slow] = FAMILIES.map(family);
    // Each is clamped to Hyprland's range above, well inside an i32.
    let int = |v: i64| v as i32;
    fs_wm_proto::Chrome {
        rounding: int(w.rounding),
        blur: w.blur,
        gaps_in: int(w.gaps_in),
        gaps_out: int(w.gaps_out),
        border_size: int(w.border_size),
        border_color: hex(&w.border),
        shadow: w.shadow,
        shadow_range: int(w.range),
        shadow_power: int(w.power),
        shadow_offset: [int(w.offset.0), int(w.offset.1)],
        shadow_color: hex(&w.shadow_color),
        shadow_inactive_color: hex(&w.backdrop_color),
        motion: fs_wm_proto::Motion {
            enabled: chrome.motion,
            families: fs_wm_proto::Families { spatial_fast, spatial, spatial_slow, effects_fast, effects, effects_slow },
        },
        edge: chrome.edge.clone(),
    }
}

pub fn hyprland_chrome(chrome: &Chrome) -> String {
    let w = window(chrome);
    let hypr = |(name, alpha): &(Option<Value>, Option<Value>)| color(name.as_ref(), alpha.as_ref());
    let (gaps_in, gaps_out, border_size, enabled, range, power) = (w.gaps_in, w.gaps_out, w.border_size, w.shadow, w.range, w.power);
    let (ox, oy) = w.offset;
    let (border_color, shadow_color, backdrop_color) = (hypr(&w.border), hypr(&w.shadow_color), hypr(&w.backdrop_color));

    format!(
        "-- Written by the shell (ThemeEngine) into ~/.config/hypr/formalshell-chrome.lua\n\
         -- on every theme.blur change, for a hyprland.lua\n\
         -- that reads it back with `dofile`. The shell asks Hyprland to reload after\n\
         -- each write.\n\
         return {{\n  \
         rounding = {},\n  \
         blur = {},\n  \
         gapsIn = {gaps_in},\n  \
         gapsOut = {gaps_out},\n  \
         borderSize = {border_size},\n  \
         borderColor = \"{border_color}\",\n  \
         shadow = {enabled},\n  \
         shadowRange = {range},\n  \
         shadowPower = {power},\n  \
         shadowOffset = {{ {ox}, {oy} }},\n  \
         shadowColor = \"{shadow_color}\",\n  \
         shadowInactiveColor = \"{backdrop_color}\",\n\
         {motion}\
         }}\n",
        w.rounding, w.blur,
        motion = hyprland_motion(chrome.motion),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chrome_for(table: &Value, rounding: Value, blur: Value) -> Chrome {
        Chrome::from_table(table, rounding, blur, true)
    }

    fn raw(v: Value) -> Chrome {
        let o = |k: &str| v.get(k).and_then(Value::as_object).cloned();
        Chrome { rounding: v.get("rounding").cloned(), blur: v.get("blur").cloned(), window: o("window"), window_inactive: o("windowInactive"), motion: true, edge: Default::default() }
    }

    /// The table alone, with the file's own header dropped.
    fn body(text: &str) -> String {
        text.trim().lines().filter(|l| !l.starts_with("--")).collect::<Vec<_>>().join("\n")
    }

    #[allow(clippy::too_many_arguments)]
    fn expected(
        rounding: i64,
        blur: &str,
        gaps: (i64, i64),
        border_size: i64,
        border_color: &str,
        shadow: &str,
        range: i64,
        power: i64,
        offset: &str,
        shadow_color: &str,
        inactive: &str,
    ) -> String {
        format!(
            "return {{\n  rounding = {rounding},\n  blur = {blur},\n  gapsIn = {},\n  gapsOut = {},\n  borderSize = {border_size},\n  borderColor = {border_color},\n  shadow = {shadow},\n  shadowRange = {range},\n  shadowPower = {power},\n  shadowOffset = {offset},\n  shadowColor = {shadow_color},\n  shadowInactiveColor = {inactive},\n{}}}",
            gaps.0, gaps.1, hyprland_motion(true)
        )
    }

    #[test]
    fn the_shipped_table_renders_the_shipped_chrome() {
        let out = hyprland_chrome(&chrome_for(crate::table(), json!(6), json!(true)));
        let lines: Vec<&str> = out.trim().lines().collect();
        for line in &lines[..4] {
            assert!(line.starts_with("--"));
        }
        assert_eq!(
            body(&out),
            expected(6, "true", (4, 6), 1, "\"border\"", "true", 24, 3, "{ 0, 6 }", "\"rgba(00000059)\"", "\"rgba(00000040)\"")
        );
    }

    #[test]
    fn rounding_is_a_non_negative_int() {
        let t = crate::table();
        assert!(hyprland_chrome(&chrome_for(t, json!(10.6), json!(false))).contains("rounding = 11,"));
        assert!(hyprland_chrome(&chrome_for(t, json!(-4), json!(false))).contains("rounding = 0,"));
        assert!(hyprland_chrome(&chrome_for(t, json!("square"), json!(false))).contains("rounding = 0,"));
    }

    #[test]
    fn blur_is_a_real_bool() {
        let t = crate::table();
        assert!(hyprland_chrome(&chrome_for(t, json!(0), json!("yes"))).contains("blur = false,"));
        let mut absent = chrome_for(t, json!(0), Value::Null);
        absent.blur = None;
        assert!(hyprland_chrome(&absent).contains("blur = false,"));
    }

    #[test]
    fn the_window_values_are_clamped_to_hyprlands_own_ranges() {
        let wild = raw(json!({
            "rounding": 0,
            "blur": false,
            "window": {
                "border": { "color": "primary", "width": 99 },
                "shadow": { "enabled": "on", "range": 4000, "renderPower": 9, "offset": [-999, "down"], "color": "black", "alpha": 4 }
            },
            "windowInactive": { "shadow": { "color": "black", "alpha": -1 } }
        }));
        assert_eq!(
            body(&hyprland_chrome(&wild)),
            expected(0, "false", (4, 8), 20, "\"primary\"", "false", 100, 4, "{ -250, 0 }", "\"rgba(000000ff)\"", "\"rgba(00000000)\"")
        );

        let thin = raw(json!({
            "rounding": 0,
            "blur": false,
            "window": {
                "border": { "color": "border", "width": -3 },
                "shadow": { "enabled": true, "range": "wide", "renderPower": 0, "offset": [], "color": "white", "alpha": 0.5 }
            },
            "windowInactive": {}
        }));
        assert_eq!(
            body(&hyprland_chrome(&thin)),
            expected(0, "false", (4, 8), 0, "\"border\"", "true", 4, 1, "{ 0, 0 }", "\"rgba(ffffff80)\"", "\"rgba(00000000)\"")
        );
    }

    #[test]
    fn a_table_with_no_window_role_still_renders_every_key() {
        assert_eq!(
            body(&hyprland_chrome(&raw(json!({ "rounding": 10, "blur": true })))),
            expected(10, "true", (4, 8), 1, "\"rgba(00000000)\"", "false", 4, 3, "{ 0, 0 }", "\"rgba(00000000)\"", "\"rgba(00000000)\"")
        );
    }

    #[test]
    fn the_gaps_are_clamped_and_fall_back() {
        let bare = raw(json!({ "rounding": 0, "blur": false, "window": {}, "windowInactive": {} }));
        assert!(hyprland_chrome(&bare).contains("gapsIn = 4,\n"));
        assert!(hyprland_chrome(&bare).contains("gapsOut = 8,\n"));

        let wild = raw(json!({ "rounding": 0, "blur": false, "window": { "gapsIn": -5, "gapsOut": 4000 }, "windowInactive": {} }));
        assert!(hyprland_chrome(&wild).contains("gapsIn = 0,\n"));
        assert!(hyprland_chrome(&wild).contains("gapsOut = 100,\n"));
    }

    #[test]
    fn the_json_chrome_carries_what_the_lua_one_does() {
        let chrome = chrome_for(crate::table(), json!(6), json!(true));
        let lua = hyprland_chrome(&chrome);
        let v = serde_json::to_value(wm_chrome(&chrome)).unwrap();
        for key in ["rounding", "gapsIn", "gapsOut", "borderSize", "shadowRange", "shadowPower"] {
            assert!(lua.contains(&format!("{key} = {},", v[key])), "{key}: {} not in {lua}", v[key]);
        }
        assert_eq!(v["blur"], json!(true));
        for key in ["borderColor", "shadowColor", "shadowInactiveColor"] {
            let c = v[key].as_str().unwrap();
            let lua_form = c.strip_prefix('#').map_or_else(|| c.to_owned(), |hex| format!("rgba({hex})"));
            assert!(lua.contains(&format!("{key} = \"{lua_form}\"")), "{key}: {c} not in {lua}");
            assert!(!c.starts_with('#') || c.len() == 9, "{key}: {c}");
        }
        assert_eq!(v["motion"]["families"]["spatial"], json!({ "curve": [0.38, 1.21, 0.22, 1.0], "ms": 500.0 }));
        assert_eq!(v["motion"]["families"].as_object().unwrap().len(), 6);
        assert_eq!(hex_color(None, None), "#00000000");
        assert_eq!(hex_color(Some(&json!("primary")), None), "primary");
    }

    /// The curves are motion.rs's own, the speeds the families' durations
    /// in tenths of a second, and motion off keeps them while turning the
    /// animations off.
    #[test]
    fn the_motion_block_carries_the_two_clock_families() {
        assert_eq!(
            hyprland_motion(true),
            "  motion = {\n    enabled = true,\n\
             \x20   spatialFast = { curve = { { 0.42, 1.67 }, { 0.21, 0.9 } }, speed = 3.5 },\n\
             \x20   spatial = { curve = { { 0.38, 1.21 }, { 0.22, 1 } }, speed = 5 },\n\
             \x20   spatialSlow = { curve = { { 0.39, 1.29 }, { 0.35, 0.98 } }, speed = 6.5 },\n\
             \x20   effectsFast = { curve = { { 0.31, 0.94 }, { 0.34, 1 } }, speed = 1.5 },\n\
             \x20   effects = { curve = { { 0.34, 0.8 }, { 0.34, 1 } }, speed = 2 },\n\
             \x20   effectsSlow = { curve = { { 0.34, 0.88 }, { 0.34, 1 } }, speed = 3 },\n\
             \x20 },\n"
        );
        let mut still = chrome_for(crate::table(), json!(6), json!(true));
        still.motion = false;
        let off = hyprland_chrome(&still);
        assert!(off.contains("    enabled = false,\n    spatialFast = { curve = { { 0.42, 1.67 }, { 0.21, 0.9 } }, speed = 3.5 },"), "{off}");
    }
}
