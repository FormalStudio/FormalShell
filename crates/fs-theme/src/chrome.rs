//! `~/.config/hypr/formalshell-chrome.lua` (`shell/Theme/chrome.js`) and
//! its mango twins: the window rounding, whether the compositor blurs
//! behind the shell, and the gaps, frame and cast the table's `window` role
//! asks for. Not a matugen template, since none of it comes from the
//! wallpaper.
//!
//! Every value is clamped to the range Hyprland's own option carries
//! (src/config/values/ConfigValues.cpp, 0.56): a value outside one fails the
//! whole `hl.config` call rather than one key.

use std::borrow::Cow;

use serde_json::{Map, Value};

use crate::style;
use crate::tokens::{js_number, js_round};

/// What ThemeEngine hands the renderer: the two settings scalars as written
/// and the `window` role's two states straight off the live table.
#[derive(Clone, Debug, Default)]
pub struct Chrome {
    pub rounding: Option<Value>,
    pub blur: Option<Value>,
    pub window: Option<Map<String, Value>>,
    pub window_inactive: Option<Map<String, Value>>,
}

impl Chrome {
    pub fn from_table(table: &Value, rounding: Value, blur: Value) -> Self {
        Self {
            rounding: Some(rounding),
            blur: Some(blur),
            window: style::entry(table, "window", Some("rest")).map(Cow::into_owned),
            window_inactive: style::entry(table, "window", Some("inactive")).map(Cow::into_owned),
        }
    }
}

/// Hyprland's `rounding` is a non-negative int. NaN falls through to 0.
fn rounding(value: Option<&Value>) -> i64 {
    let n = js_round(js_number(value));
    if n > 0.0 { n as i64 } else { 0 }
}

/// Anything but the boolean true is false: a stray string on a bool option
/// is rejected.
fn blur(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Bool(true)))
}

fn int(value: Option<&Value>, min: i64, max: i64, fallback: i64) -> i64 {
    let n = js_round(js_number(value));
    if !n.is_finite() {
        return fallback;
    }
    (n as i64).clamp(min, max)
}

fn byte(alpha: Option<&Value>) -> String {
    let a = js_number(alpha);
    if !a.is_finite() {
        return "ff".to_owned();
    }
    format!("{:02x}", js_round(a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// A literal renders as Hyprland's `rgba(RRGGBBAA)`; anything else is a
/// palette role and renders as its name, a key into
/// formalshell-colors.lua's table, so the wallpaper keeps moving it. A role
/// therefore arrives at whatever alpha the palette gave it.
fn color(name: Option<&Value>, alpha: Option<&Value>) -> String {
    let name = match name {
        None | Some(Value::Null) => return "rgba(00000000)".to_owned(),
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return other.to_string(),
    };
    if name == "transparent" {
        return "rgba(00000000)".to_owned();
    }
    match style::literal(name) {
        Some(lit) => format!("rgba({}{})", lit.trim_start_matches('#'), byte(alpha)),
        None => name.to_owned(),
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
         }}\n",
        w.rounding, w.blur,
    )
}

/// `~/.config/mango/formalshell-chrome.conf`: the same window role as mango
/// 0.18 options (src/config/option_defs.c), numbers only, so it needs no
/// palette. Hyprland's `gaps_in` is per side where mango's inner gap is the
/// whole space between two windows, hence twice it. mango has no shadow
/// falloff, so the blur spans the whole range Hyprland draws over.
pub fn mango_chrome(chrome: &Chrome) -> String {
    let w = window(chrome);
    let flag = |b: bool| u8::from(b);
    format!(
        "# Written by the shell (ThemeEngine) into ~/.config/mango/formalshell-chrome.conf\n\
         # on every theme.blur change, for a mango config\n\
         # that sources it. The shell asks mango to reload after each write.\n\
         border_radius={}\n\
         border_px={}\n\
         gap_inner_horizontal={inner}\n\
         gap_inner_vertical={inner}\n\
         gap_outer_horizontal={outer}\n\
         gap_outer_vertical={outer}\n\
         blur={blur}\n\
         blur_layer={blur}\n\
         shadows={}\n\
         shadow_only_floating=0\n\
         shadows_size={range}\n\
         shadows_blur={range}\n\
         shadows_position_x={}\n\
         shadows_position_y={}\n",
        w.rounding.min(100),
        w.border_size,
        flag(w.shadow),
        w.offset.0,
        w.offset.1,
        inner = w.gaps_in * 2,
        outer = w.gaps_out,
        blur = flag(w.blur),
        range = w.range,
    )
}

/// `#rrggbb` or `#rrggbbaa` as mango's `0xRRGGBBAA`.
fn mango_hex(hex: &str, alpha: Option<String>) -> Option<String> {
    let digits = hex.strip_prefix('#')?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match digits.len() {
        6 => Some(format!("0x{digits}{}", alpha.unwrap_or_else(|| "ff".into()))),
        8 => Some(format!("0x{digits}")),
        _ => None,
    }
}

/// A literal at its own alpha, a palette role at the palette's; `None` for
/// a role the palette does not carry, which leaves mango's own value.
fn mango_color((name, alpha): &(Option<Value>, Option<Value>), palette: &Map<String, Value>) -> Option<String> {
    let name = match name {
        None | Some(Value::Null) => return Some("0x00000000".into()),
        Some(Value::String(s)) => s.as_str(),
        Some(_) => return None,
    };
    match style::literal(name) {
        Some("transparent") => Some("0x00000000".into()),
        Some(lit) => mango_hex(lit, Some(byte(alpha.as_ref()))),
        None => mango_hex(palette.get(name)?.as_str()?, None),
    }
}

/// `~/.config/mango/formalshell-colors.conf`: every colour the window role
/// and the palette give mango, resolved here because a mango config has no
/// table to look a role up in.
pub fn mango_colors(chrome: &Chrome, palette: &Map<String, Value>) -> String {
    let w = window(chrome);
    let role = |name: &str| (Some(Value::String(name.into())), None);
    let mut out = String::from(
        "# Written by the shell (ThemeEngine) into ~/.config/mango/formalshell-colors.conf\n\
         # on every wallpaper, mode or theme change, for a mango config that sources\n\
         # it. The shell asks mango to reload after each write.\n",
    );
    let lines = [
        ("focus_color", &w.border),
        ("border_color", &role("border")),
        ("urgent_color", &role("destructive")),
        ("shadows_color", &w.shadow_color),
    ];
    for (key, value) in lines {
        if let Some(c) = mango_color(value, palette) {
            out.push_str(&format!("{key}={c}\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chrome_for(table: &Value, rounding: Value, blur: Value) -> Chrome {
        Chrome::from_table(table, rounding, blur)
    }

    fn raw(v: Value) -> Chrome {
        let o = |k: &str| v.get(k).and_then(Value::as_object).cloned();
        Chrome { rounding: v.get("rounding").cloned(), blur: v.get("blur").cloned(), window: o("window"), window_inactive: o("windowInactive") }
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
            "return {{\n  rounding = {rounding},\n  blur = {blur},\n  gapsIn = {},\n  gapsOut = {},\n  borderSize = {border_size},\n  borderColor = {border_color},\n  shadow = {shadow},\n  shadowRange = {range},\n  shadowPower = {power},\n  shadowOffset = {offset},\n  shadowColor = {shadow_color},\n  shadowInactiveColor = {inactive},\n}}",
            gaps.0, gaps.1
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
    fn mango_takes_the_same_window_as_its_own_options() {
        let out = mango_chrome(&chrome_for(crate::table(), json!(6), json!(true)));
        let body: Vec<&str> = out.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(
            body,
            [
                "border_radius=6",
                "border_px=1",
                "gap_inner_horizontal=8",
                "gap_inner_vertical=8",
                "gap_outer_horizontal=6",
                "gap_outer_vertical=6",
                "blur=1",
                "blur_layer=1",
                "shadows=1",
                "shadow_only_floating=0",
                "shadows_size=24",
                "shadows_blur=24",
                "shadows_position_x=0",
                "shadows_position_y=6",
            ]
        );
        assert!(mango_chrome(&chrome_for(crate::table(), json!(400), json!(false))).contains("border_radius=100\nborder_px=1\n"));
    }

    #[test]
    fn mango_colours_resolve_roles_against_the_palette() {
        let palette: Map<String, Value> = serde_json::from_value(json!({
            "primary": "#9ecafc", "border": "#42474e80", "destructive": "#ffb4ab"
        }))
        .unwrap();
        let out = mango_colors(&chrome_for(crate::table(), json!(6), json!(true)), &palette);
        let body: Vec<&str> = out.lines().filter(|l| !l.starts_with('#')).collect();
        assert_eq!(body, ["focus_color=0x42474e80", "border_color=0x42474e80", "urgent_color=0xffb4abff", "shadows_color=0x00000059"]);
        let none = mango_colors(&raw(json!({ "window": { "border": { "color": "nope" } } })), &Map::new());
        assert_eq!(none.lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(), ["shadows_color=0x00000000"]);
    }
}
