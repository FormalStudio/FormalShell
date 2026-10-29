//! `theme.preset` (`shell/Theme/presets.js`): a table of chrome defaults,
//! not a mode. Every knob a preset sets is a settings key the user can still
//! write, and an explicit key always wins; the preset's chrome table is not
//! overridable at all.

use serde_json::Value;

use crate::tables;

pub const NAMES: [&str; 3] = ["metamorphosis", "retro", "pantheon"];

/// tsubi.club's five presets (`--color-1` to `--color-5`), the names
/// `wallpaper.color` takes besides a hex.
pub const COLORS: [(&str, &str); 5] =
    [("sage", "#a1c8bc"), ("purple", "#402484"), ("green", "#68bc64"), ("silver", "#bdc3c9"), ("orange", "#ff8000")];

#[derive(Clone, Copy, Debug)]
pub struct Defaults {
    pub radius: f64,
    pub icons: &'static str,
    pub fonts: &'static str,
    pub surface_opacity: f64,
    pub blur: bool,
    pub dither: bool,
    pub grid: bool,
    pub animate: bool,
    /// The flat colour the desktop shows in place of the picture.
    pub color: Option<&'static str>,
    /// The colours the wallpaper picker offers ahead of the pictures.
    pub swatches: &'static [(&'static str, &'static str)],
    pub style: &'static Value,
}

/// Anything that is not one of [`NAMES`] resolves to metamorphosis.
fn name(value: Option<&Value>) -> &'static str {
    let wanted = value.and_then(Value::as_str);
    NAMES.into_iter().find(|n| Some(*n) == wanted).unwrap_or("metamorphosis")
}

/// A fresh copy per call, so a caller can hold and change what it gets.
pub fn defaults(preset: Option<&Value>) -> Defaults {
    match name(preset) {
        "retro" => Defaults {
            radius: 0.0,
            icons: "nerd",
            fonts: "mono",
            surface_opacity: 1.0,
            blur: false,
            dither: true,
            grid: false,
            animate: false,
            color: None,
            swatches: &[],
            style: tables::retro(),
        },
        // Opaque, unlike the other two: elementary's popovers and dialogs
        // have nothing behind them, and its one translucent surface is the
        // panel, which carries its own alpha in the table.
        "pantheon" => Defaults {
            radius: 6.0,
            icons: "lucide",
            fonts: "pair",
            surface_opacity: 1.0,
            blur: true,
            dither: false,
            grid: false,
            animate: false,
            color: None,
            swatches: &[],
            style: tables::pantheon(),
        },
        _ => Defaults {
            radius: 10.0,
            icons: "lucide",
            fonts: "pair",
            surface_opacity: 0.85,
            blur: true,
            dither: false,
            grid: false,
            animate: false,
            color: None,
            swatches: &[],
            style: tables::metamorphosis(),
        },
    }
}

/// A value that is not a real boolean is a malformed key rather than a
/// falsy one, so `"true"` and `1` take the preset's value.
fn boolean(value: Option<Value>, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(b)) => b,
        _ => fallback,
    }
}

/// `wallpaper.color` as written: a `#rrggbb` hex (lowercased) or one of
/// [`COLORS`]' names gives that colour, `false` or `null` the picture, and
/// anything else is malformed, `None`, so the preset's own stands.
pub fn color(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Bool(false) | Value::Null => Some(None),
        Value::String(s) => {
            let s = s.trim().to_lowercase();
            if let Some((_, hex)) = COLORS.iter().find(|(name, _)| *name == s) {
                return Some(Some((*hex).to_owned()));
            }
            let digits = s.strip_prefix('#')?;
            (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Some(s.clone()))
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct Resolved {
    pub preset: &'static str,
    pub style: &'static Value,
    /// As written in settings.json, or the preset's number. `theme::Theme`
    /// range-checks it, falling back to [`defaults`] for a non-number.
    pub radius: Value,
    pub icons: Value,
    pub fonts: &'static str,
    /// As written, or the preset's alpha; range-checked by `theme::Theme`.
    pub surface_opacity: Value,
    pub blur: bool,
    pub dither: bool,
    /// The two full-screen image passes follow `theme.dither` unless they
    /// say otherwise.
    pub wallpaper_dither: bool,
    pub lock_dither: bool,
    /// `theme.grid` the same way: the desktop's lines, and the lock's and
    /// the greeter's.
    pub grid: bool,
    pub wallpaper_grid: bool,
    pub lock_grid: bool,
    /// `wallpaper.animate`: the shard sculpture moving over the desktop.
    pub wallpaper_animate: bool,
    /// `wallpaper.color`, or the preset's: a `#rrggbb` the desktop shows
    /// flat unless the picker chose otherwise.
    pub wallpaper_color: Option<String>,
    pub wallpaper_swatches: &'static [(&'static str, &'static str)],
}

/// `get` reads one dotted settings path, `None` when the key is absent: the
/// preset's own value stands in exactly where `Config.get` would hand back
/// its fallback.
pub fn resolve(preset: Option<&Value>, get: impl Fn(&str) -> Option<Value>) -> Resolved {
    let preset = name(preset);
    let d = defaults(Some(&Value::from(preset)));

    let fonts = match get("theme.fonts").as_ref().and_then(Value::as_str) {
        Some("pair") => "pair",
        Some("mono") => "mono",
        _ => d.fonts,
    };
    let dither = boolean(get("theme.dither"), d.dither);
    let grid = boolean(get("theme.grid"), d.grid);

    Resolved {
        preset,
        style: d.style,
        radius: get("theme.radius").unwrap_or(Value::from(d.radius)),
        icons: get("theme.icons").unwrap_or(Value::from(d.icons)),
        fonts,
        surface_opacity: get("theme.surfaceOpacity").unwrap_or(Value::from(d.surface_opacity)),
        blur: boolean(get("theme.blur"), d.blur),
        dither,
        wallpaper_dither: boolean(get("wallpaper.dither"), dither),
        lock_dither: boolean(get("lock.dither"), dither),
        grid,
        wallpaper_grid: boolean(get("wallpaper.grid"), grid),
        lock_grid: boolean(get("lock.grid"), grid),
        wallpaper_animate: boolean(get("wallpaper.animate"), d.animate),
        wallpaper_color: get("wallpaper.color").as_ref().and_then(color).unwrap_or(d.color.map(str::to_owned)),
        wallpaper_swatches: d.swatches,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// `Config.get`'s own walk over a plain settings object.
    pub(crate) fn make_get(settings: Value) -> impl Fn(&str) -> Option<Value> {
        move |path| {
            let mut node = &settings;
            for part in path.split('.') {
                node = node.as_object()?.get(part)?;
            }
            Some(node.clone())
        }
    }

    fn p(name: &str, settings: Value) -> Resolved {
        resolve(Some(&json!(name)), make_get(settings))
    }

    #[test]
    fn names_are_the_three_presets() {
        assert_eq!(NAMES, ["metamorphosis", "retro", "pantheon"]);
    }

    #[test]
    fn unknown_name_resolves_to_the_metamorphosis_table() {
        let t = p("brutalist", json!({}));
        assert_eq!(t.preset, "metamorphosis");
        assert_eq!(t.radius, json!(10.0));
        assert_eq!(t.icons, "lucide");
        assert_eq!(t.fonts, "pair");
        assert_eq!(t.surface_opacity, json!(0.85));
        assert!(t.blur);
        assert!(!t.dither);
        assert!(!t.wallpaper_dither);
        assert!(!t.lock_dither);
    }

    #[test]
    fn a_name_that_is_not_a_string_resolves_to_metamorphosis() {
        assert_eq!(defaults(None).radius, 10.0);
        assert_eq!(defaults(Some(&json!(42))).icons, "lucide");
        assert_eq!(resolve(Some(&Value::Null), make_get(json!({}))).preset, "metamorphosis");
    }

    #[test]
    fn retro_defaults_match_the_table() {
        let t = p("retro", json!({}));
        assert_eq!(t.preset, "retro");
        assert_eq!(t.radius, json!(0.0));
        assert_eq!(t.icons, "nerd");
        assert_eq!(t.fonts, "mono");
        assert_eq!(t.surface_opacity, json!(1.0));
        assert!(!t.blur);
        assert!(t.dither);
        assert!(t.wallpaper_dither);
        assert!(t.lock_dither);
    }

    #[test]
    fn pantheon_defaults_match_the_table() {
        let t = p("pantheon", json!({}));
        assert_eq!(t.preset, "pantheon");
        assert_eq!(t.radius, json!(6.0));
        assert_eq!(t.icons, "lucide");
        assert_eq!(t.fonts, "pair");
        assert_eq!(t.surface_opacity, json!(1.0));
        assert!(t.blur);
        assert!(!t.dither);
        assert!(!t.wallpaper_dither);
        assert!(!t.lock_dither);
    }

    #[test]
    fn no_preset_takes_the_grid_the_backdrop_or_a_flat_colour_unless_asked() {
        for name in NAMES {
            let t = p(name, json!({}));
            assert!(!t.grid && !t.wallpaper_grid && !t.lock_grid, "{name}");
            assert!(!t.wallpaper_animate, "{name}");
            assert_eq!(t.wallpaper_color, None, "{name}");
            assert!(t.wallpaper_swatches.is_empty(), "{name}");
        }
        assert!(p("pantheon", json!({ "wallpaper": { "animate": true } })).wallpaper_animate);
        assert_eq!(p("pantheon", json!({ "wallpaper": { "color": "#FF8000" } })).wallpaper_color.as_deref(), Some("#ff8000"));
        assert_eq!(p("retro", json!({ "wallpaper": { "color": "sage" } })).wallpaper_color.as_deref(), Some("#a1c8bc"));
    }

    #[test]
    fn false_or_null_gives_the_picture_back_and_junk_keeps_the_preset() {
        let flat = json!({ "wallpaper": { "color": "#402484" } });
        assert_eq!(p("pantheon", flat).wallpaper_color.as_deref(), Some("#402484"));
        assert_eq!(p("pantheon", json!({ "wallpaper": { "color": false } })).wallpaper_color, None);
        assert_eq!(p("pantheon", json!({ "wallpaper": { "color": null } })).wallpaper_color, None);
        for junk in [json!("#40248"), json!("402484"), json!("violet"), json!(true), json!(4204164)] {
            assert_eq!(p("pantheon", json!({ "wallpaper": { "color": junk } })).wallpaper_color, None, "{junk}");
        }
    }

    #[test]
    fn the_grid_overrides_like_dither() {
        let t = p("metamorphosis", json!({ "theme": { "grid": true }, "lock": { "grid": false } }));
        assert!(t.wallpaper_grid);
        assert!(!t.lock_grid);
        let t = p("metamorphosis", json!({ "theme": { "grid": true } }));
        assert!(t.wallpaper_grid && t.lock_grid);
        assert!(!p("pantheon", json!({ "theme": { "grid": "true" } })).grid);
    }

    #[test]
    fn an_explicit_key_wins_over_the_preset() {
        let t = p("retro", json!({ "theme": { "radius": 4 } }));
        assert_eq!(t.radius, json!(4));
        assert_eq!(t.icons, "nerd");
        assert_eq!(t.fonts, "mono");
    }

    #[test]
    fn wallpaper_dither_can_opt_out_of_the_preset() {
        let t = p("retro", json!({ "wallpaper": { "dither": false } }));
        assert!(t.dither);
        assert!(!t.wallpaper_dither);
        assert!(t.lock_dither);
    }

    #[test]
    fn theme_dither_carries_both_image_passes() {
        let t = p("metamorphosis", json!({ "theme": { "dither": true } }));
        assert!(t.dither);
        assert!(t.wallpaper_dither);
        assert!(t.lock_dither);
    }

    #[test]
    fn an_unknown_fonts_value_takes_the_preset_default() {
        assert_eq!(p("metamorphosis", json!({ "theme": { "fonts": "comic" } })).fonts, "pair");
        assert_eq!(p("retro", json!({ "theme": { "fonts": "comic" } })).fonts, "mono");
        assert_eq!(p("metamorphosis", json!({ "theme": { "fonts": "mono" } })).fonts, "mono");
    }

    #[test]
    fn a_non_boolean_takes_the_preset_default() {
        assert!(p("metamorphosis", json!({ "theme": { "blur": "false" } })).blur);
        assert!(p("metamorphosis", json!({ "theme": { "blur": 0 } })).blur);
        assert!(!p("retro", json!({ "theme": { "blur": 1 } })).blur);
        assert!(!p("metamorphosis", json!({ "theme": { "dither": "true" } })).dither);
        assert!(!p("metamorphosis", json!({ "theme": { "blur": false } })).blur);
    }

    #[test]
    fn defaults_hands_back_a_copy() {
        let mut first = defaults(Some(&json!("retro")));
        first.radius = 99.0;
        assert_eq!(first.radius, 99.0);
        assert_eq!(defaults(Some(&json!("retro"))).radius, 0.0);
    }
}
