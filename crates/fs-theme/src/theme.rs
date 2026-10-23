//! What the theme resolves: the preset against settings.json,
//! the palette theme.json carries, the tokens at the live scale, and
//! `box()` against all three. Settings come in through `get` (one dotted
//! path, `None` when absent) so this holds no config of its own.

use serde_json::{Map, Value};

use crate::chrome;
use crate::color::Rgba;
use crate::palette::{self, COLOR_KEYS};
use crate::presets;
use crate::style::{self, BoxStyle, Clock, Radius, Ring};
use crate::tokens::{self, Easing, FontTokens, LetterSpacing, Radii, Space};

/// theme.json's colour roles, parsed. Built from a palette that already
/// went through `palette::merge_with_fallback`, so every key is valid hex.
#[derive(Clone, Debug, PartialEq)]
pub struct Colors {
    pub mode: String,
    values: [Rgba; 26],
}

impl Colors {
    pub fn from_palette(palette: &Map<String, Value>) -> Self {
        let mode = palette.get("mode").and_then(Value::as_str).unwrap_or("dark").to_owned();
        let values = COLOR_KEYS.map(|key| {
            palette.get(key).and_then(Value::as_str).and_then(Rgba::parse).unwrap_or(Rgba::TRANSPARENT)
        });
        Self { mode, values }
    }

    /// A role by its theme.json name; an unknown one draws nothing.
    pub fn get(&self, name: &str) -> Rgba {
        COLOR_KEYS.iter().position(|k| *k == name).map_or(Rgba::TRANSPARENT, |i| self.values[i])
    }

    /// The same roles with `primary` taken from `role`, for a surface drawn
    /// in its own accent. A role too close to the background to read as one
    /// (the static fallback's grey chart steps) leaves `primary` alone.
    pub fn accented(&self, role: &str) -> Self {
        let mut out = self.clone();
        let accent = self.get(role);
        let (hi, lo) = {
            let (a, b) = (luminance(accent), luminance(self.get("background")));
            (a.max(b), a.min(b))
        };
        if let Some(p) = COLOR_KEYS.iter().position(|k| *k == "primary")
            && COLOR_KEYS.contains(&role)
            && (hi + 0.05) / (lo + 0.05) >= 3.0
        {
            out.values[p] = accent;
        }
        out
    }
}

/// WCAG's relative luminance of an sRGB colour.
fn luminance(c: Rgba) -> f64 {
    let lin = |v: f32| {
        let v = f64::from(v);
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

impl Default for Colors {
    /// `Palette.fallback()`'s dark zinc, the placeholder before theme.json.
    fn default() -> Self {
        Self::from_palette(&palette::fallback("dark"))
    }
}

/// Every clock a surface runs on, with `motion.enabled` and the rig's
/// `debug motionScale` already applied.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeMotion {
    pub emerge: f64,
    pub arrive: f64,
    pub restack: f64,
    pub switcher: f64,
    pub restack_stagger: f64,
    pub families: tokens::Motion,
    pub emerge_curve: Easing,
    pub arrive_curve: Easing,
    pub restack_curve: Easing,
    pub switcher_curve: Easing,
    pub pulse_duration: f64,
    /// The table's effects past its clocks, durations scaled the same way.
    pub catalogue: style::Catalogue,
}

/// One text role's family: the name a table gives it, and the fontconfig
/// alias that stands in when the table names none or the face is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub name: Option<&'static str>,
    pub generic: &'static str,
}

/// The four text roles a table may name: `words` for labels, `values` for
/// numbers and status, `display` for headings and titles, `body` for
/// secondary lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faces {
    pub words: Face,
    pub values: Face,
    pub display: Face,
    pub body: Face,
}

impl Faces {
    /// The table's `fonts` block, `display` and `body` falling back to
    /// `words`, `values` to `monospace` and `words` to the `theme.fonts`
    /// pair's sans. `from_table` false leaves every role on its alias.
    pub fn resolve(style: &'static Value, fonts: &str, from_table: bool) -> Self {
        let sans = if fonts == "mono" { "monospace" } else { "sans-serif" };
        let name = |key: &str| -> Option<&'static str> {
            if !from_table {
                return None;
            }
            style.get("fonts")?.get(key)?.as_str().filter(|n| !n.is_empty())
        };
        let words = Face { name: name("words"), generic: sans };
        let or_words = |key: &str| name(key).map_or(words, |n| Face { name: Some(n), generic: sans });
        Self {
            words,
            values: Face { name: name("values"), generic: "monospace" },
            display: or_words("display"),
            body: or_words("body"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub preset: &'static str,
    pub style: &'static Value,
    pub colors: Colors,
    pub radius: f64,
    pub radii: Radii,
    pub border_width: f64,
    pub surface_opacity: f64,
    pub blur_behind: bool,
    pub fonts: &'static str,
    pub faces: Faces,
    pub icon_set: String,
    pub dither: bool,
    pub wallpaper_dither: bool,
    pub lock_dither: bool,
    pub font_base_size: f64,
    pub font_scale: f64,
    pub font_size: FontTokens,
    pub spacing_scale: f64,
    pub space: Space,
    pub letter_spacing: LetterSpacing,
    pub motion_enabled: bool,
    pub motion_scale: f64,
    pub frame_habit: bool,
    pub frame_requested: f64,
    pub frame_thickness: f64,
    pub frame_radius: f64,
}

struct PaletteCtx<'a> {
    theme: &'a Theme,
}

impl style::Ctx for PaletteCtx<'_> {
    type Color = Rgba;
    fn mode(&self) -> &str {
        &self.theme.colors.mode
    }
    fn surface_opacity(&self) -> f64 {
        self.theme.surface_opacity
    }
    fn radius(&self, step: &str) -> Option<f64> {
        self.theme.radii.step(step)
    }
    fn color(&self, name: &str) -> Rgba {
        self.theme.colors.get(name)
    }
    fn literal(&self, value: &'static str) -> Rgba {
        Rgba::parse(value).unwrap_or(Rgba::TRANSPARENT)
    }
    fn alpha(&self, c: &Rgba, a: f64) -> Rgba {
        c.with_alpha(a as f32)
    }
    fn tint(&self, c: &Rgba, over: &Rgba) -> Rgba {
        c.tint(*over)
    }
}

impl Theme {
    pub fn resolve(get: impl Fn(&str) -> Option<Value>, palette: &Map<String, Value>) -> Self {
        let preset = presets::resolve(get("theme.preset").as_ref(), &get);
        let preset_defaults = presets::defaults(Some(&Value::from(preset.preset)));
        let style = preset.style;

        let radius = tokens::js_round(tokens::clamp(Some(&preset.radius), 0.0, f64::INFINITY, preset_defaults.radius));
        let surface_opacity = tokens::clamp(Some(&preset.surface_opacity), 0.0, 1.0, preset_defaults.surface_opacity);

        let font_base_size = 13.0;
        let font_scale = tokens::font_scale(font_base_size);
        let spacing_scale = font_scale;

        // A table that wears no frame reads `frame.thickness` as 0, ring and
        // reservation both.
        let frame_habit = style["habits"].get("frame") != Some(&Value::Bool(false));
        let frame_requested =
            tokens::js_round(tokens::clamp(get("frame.thickness").as_ref(), 0.0, f64::INFINITY, 0.0));
        let frame_radius_default = Value::from(if radius > 0.0 { 20 } else { 0 });
        let frame_radius = tokens::js_round(tokens::clamp(
            Some(&get("frame.radius").unwrap_or(frame_radius_default)),
            0.0,
            f64::INFINITY,
            0.0,
        ));

        let icon_set = match &preset.icons {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };

        Self {
            preset: preset.preset,
            style,
            colors: Colors::from_palette(palette),
            radius,
            radii: tokens::radius_tokens(Some(radius)),
            border_width: 1.0,
            surface_opacity,
            blur_behind: preset.blur,
            fonts: preset.fonts,
            faces: Faces::resolve(style, preset.fonts, preset.table_fonts),
            icon_set,
            dither: preset.dither,
            wallpaper_dither: preset.wallpaper_dither,
            lock_dither: preset.lock_dither,
            font_base_size,
            font_scale,
            font_size: tokens::font_tokens(font_base_size),
            spacing_scale,
            space: tokens::spacing_tokens(spacing_scale),
            letter_spacing: tokens::letter_spacing_tokens(font_scale),
            motion_enabled: get("motion.enabled").unwrap_or(Value::Bool(true)) == Value::Bool(true),
            motion_scale: 1.0,
            frame_habit,
            frame_requested,
            frame_thickness: if frame_habit { frame_requested } else { 0.0 },
            frame_radius,
        }
    }

    pub fn frame_enabled(&self) -> bool {
        self.frame_thickness > 0.0
    }

    /// One habit's value, which a surface reads instead of the preset name.
    pub fn habit(&self, key: &str) -> Option<&Value> {
        self.style["habits"].get(key)
    }

    /// A string habit, `""` when the table leaves it out.
    pub fn habit_str(&self, key: &str) -> &str {
        self.habit(key).and_then(Value::as_str).unwrap_or("")
    }

    /// This theme with `primary` taken from another palette role.
    pub fn accented(&self, role: &str) -> Self {
        let mut out = self.clone();
        out.colors = self.colors.accented(role);
        out
    }

    fn ctx(&self) -> PaletteCtx<'_> {
        PaletteCtx { theme: self }
    }

    /// `Theme.box(role, state)`: one role's box, drawable.
    pub fn box_style(&self, role: &str, state: Option<&str>) -> BoxStyle<Rgba> {
        style::resolve(self.style, role, state, &self.ctx())
    }

    pub fn has_state(&self, role: &str, state: &str) -> bool {
        style::has_state(self.style, role, state)
    }

    pub fn with_cursor(&self, b: BoxStyle<Rgba>, on: bool, halo: bool) -> BoxStyle<Rgba> {
        if on { style::with_cursor(&b, &self.box_style("cursor", None), self.cursor_bar().as_ref(), halo) } else { b }
    }

    /// A grid cell's cursor: the `cursor.brackets` arcs, their glow cast
    /// and the cursor's own halo on a table that draws brackets, the plain
    /// cursor otherwise.
    pub fn with_brackets(&self, b: BoxStyle<Rgba>, on: bool, halo: bool) -> BoxStyle<Rgba> {
        match self.cursor_brackets() {
            Some(brackets) if on => {
                let mut out = style::with_cursor(&b, &self.box_style("cursor", None), None, halo);
                out.casts.extend(self.box_style("cursor.brackets", None).casts);
                out.brackets = Some(brackets);
                out
            }
            _ => self.with_cursor(b, on, halo),
        }
    }

    /// The block the cursor draws on a row's leading edge, if the table has one.
    pub fn cursor_bar(&self) -> Option<style::Bar<Rgba>> {
        style::cursor_bar(self.style, &self.ctx())
    }

    pub fn cursor_brackets(&self) -> Option<style::Brackets<Rgba>> {
        style::cursor_brackets(self.style, &self.ctx())
    }

    /// A number a role carries beside its box.
    pub fn metric(&self, role: &str, key: &str) -> f64 {
        style::metric(self.style, role, key)
    }

    /// Half the extent while the base radius is positive, square at 0.
    pub fn pill_radius(&self, extent: f64) -> f64 {
        if self.radius > 0.0 { extent / 2.0 } else { 0.0 }
    }

    pub fn box_radius(&self, b: &BoxStyle<Rgba>, extent: f64) -> f64 {
        match b.radius {
            Radius::Pill => self.pill_radius(extent),
            Radius::Px(px) => px,
        }
    }

    pub fn cover_radius(&self, extent: f64) -> f64 {
        tokens::cover_radius(self.radii.sm, extent)
    }

    /// The halo the `cursor` entry declares; nothing when it declares none.
    pub fn cursor_ring(&self) -> Ring<Rgba> {
        let cursor = self.box_style("cursor", None);
        cursor.rings.into_iter().next().unwrap_or(Ring { spread: 0.0, color: Rgba::TRANSPARENT })
    }

    /// The room the cursor's halo needs outside a row.
    pub fn ring_width(&self) -> f64 {
        self.cursor_ring().spread
    }

    pub fn surface(&self, c: Rgba) -> Rgba {
        c.with_alpha(self.surface_opacity as f32)
    }

    pub fn hover_fill(&self) -> Rgba {
        style::wash(self.style, "hover", &self.ctx())
    }

    pub fn press_fill(&self) -> Rgba {
        style::wash(self.style, "press", &self.ctx())
    }

    pub fn hover_filled(&self, c: Rgba) -> Rgba {
        c.tint(style::wash(self.style, "filledHover", &self.ctx()))
    }

    pub fn press_filled(&self, c: Rgba) -> Rgba {
        c.tint(style::wash(self.style, "filledPress", &self.ctx()))
    }

    /// What formalshell-chrome.lua carries: the radius, the blur switch and
    /// the `window` role's two states unresolved, since Hyprland draws them
    /// off a palette role's name rather than a colour.
    pub fn window_chrome(&self) -> chrome::Chrome {
        chrome::Chrome::from_table(self.style, Value::from(self.radius), Value::Bool(self.blur_behind))
    }

    fn catalogue(&self, on: &dyn Fn(f64) -> f64) -> style::Catalogue {
        let c = style::catalogue(self.style, &tokens::motion_tokens(true));
        let clock = |k: Clock| Clock { duration: on(k.duration), stagger: on(k.stagger), ..k };
        style::Catalogue {
            focus_text: c.focus_text.map(|f| style::FocusText { clock: clock(f.clock), ..f }),
            reveal: c.reveal.map(on),
            rows: c.rows.map(|r| style::Rows { clock: clock(r.clock), stagger: on(r.stagger), ..r }),
            swap: c.swap.map(|g| style::Glitch { out: on(g.out), into: on(g.into), ..g }),
            use_: c.use_.map(|u| style::Use { clock: clock(u.clock), ..u }),
            error: c.error.map(on),
            picture: c.picture.map(on),
            noise: c.noise.map(|n| style::Noise { clock: clock(n.clock), ..n }),
            progress: c.progress,
        }
    }

    pub fn motion(&self) -> ThemeMotion {
        let families_on = tokens::motion_tokens(true);
        let clock = |key: &str| -> Clock { style::motion(self.style, key, &families_on) };
        let (emerge, arrive, restack, switcher) = (clock("emerge"), clock("arrive"), clock("restack"), clock("switcher"));
        let on = |d: f64| if self.motion_enabled { d * self.motion_scale } else { 0.0 };
        let m = tokens::motion_tokens(self.motion_enabled);
        let s = self.motion_scale;
        ThemeMotion {
            emerge: on(emerge.duration),
            arrive: on(arrive.duration),
            restack: on(restack.duration),
            switcher: on(switcher.duration),
            restack_stagger: on(restack.stagger),
            families: tokens::Motion {
                spatial_fast: m.spatial_fast * s,
                spatial: m.spatial * s,
                spatial_slow: m.spatial_slow * s,
                effects_fast: m.effects_fast * s,
                effects: m.effects * s,
                effects_slow: m.effects_slow * s,
                emphasized: m.emphasized * s,
                reveal: m.reveal * s,
                ..m
            },
            emerge_curve: emerge.curve,
            arrive_curve: arrive.curve,
            restack_curve: restack.curve,
            switcher_curve: switcher.curve,
            pulse_duration: 900.0,
            catalogue: self.catalogue(&on),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::tests::make_get;
    use crate::style::{Cast, Edge, geometry};
    use serde_json::json;

    /// tst_box's sentinel palette: distinct values per role, since the zinc
    /// fallback shares hex values between roles and would hide a fill that
    /// went back to the wrong one.
    fn sentinel() -> Map<String, Value> {
        let colors = json!({
            "mode": "dark",
            "background": "#010101", "foreground": "#eeeeee", "mutedForeground": "#aaaaaa",
            "card": "#151515", "cardForeground": "#f0f0f0", "popover": "#181818",
            "popoverForeground": "#f1f1f1", "muted": "#2a2a2a", "accent": "#3b3b3b",
            "accentForeground": "#dddddd", "primary": "#1133ff", "primaryForeground": "#ffdd00",
            "destructive": "#ff2222", "destructiveForeground": "#22ff88", "warning": "#ffaa00",
            "warningForeground": "#001122", "border": "#444444", "input": "#333344", "ring": "#00ccff"
        });
        palette::merge_with_fallback(Some(&colors))
    }

    fn theme() -> Theme {
        Theme::resolve(make_get(json!({})), &sentinel())
    }

    fn hex(s: &str) -> Rgba {
        Rgba::parse(s).unwrap()
    }

    // --- the box a role resolves to, and the geometry Box
    // draws it with.

    #[test]
    fn a_fill_and_a_border_are_one_rectangle() {
        let t = theme();
        let b = t.box_style("card", None);
        assert_eq!(b.fill, hex("#151515").with_alpha(t.surface_opacity as f32));
        let border = b.border.clone().unwrap();
        assert_eq!(border.color, hex("#444444"));
        assert_eq!(border.width, t.border_width);
        assert_eq!(t.box_radius(&b, 40.0), t.radii.xl);
        assert!(b.casts.is_empty());
        assert!(b.rings.is_empty());
    }

    #[test]
    fn a_role_with_no_border_draws_none() {
        let t = theme();
        let b = t.box_style("trough", None);
        assert_eq!(b.fill, hex("#2a2a2a"));
        assert_eq!(b.border, None);
        assert_eq!(t.box_radius(&b, 40.0), t.radii.md);
    }

    #[test]
    fn a_state_changes_the_fill() {
        let t = theme();
        assert_eq!(t.box_style("cell", Some("active")).fill, hex("#1133ff"));
        assert_eq!(t.box_style("cell", Some("selected")).fill, hex("#3b3b3b"));
        let ghost = t.box_style("cell", Some("ghost"));
        assert_eq!(ghost.fill, Rgba::TRANSPARENT);
        assert_eq!(ghost.border, None);
        assert_eq!(t.box_style("cell", Some("destructive")).border.unwrap().color, hex("#ff2222"));
    }

    // Box's colour transition rides the effects clock family, which is all
    // the theme owns of it.
    #[test]
    fn the_fill_crossfades_on_a_state_change() {
        let t = theme();
        assert_ne!(t.box_style("cell", Some("rest")).fill, t.box_style("cell", Some("active")).fill);
        assert_eq!(t.motion().families.effects, 200.0);
    }

    #[test]
    fn a_literal_colour_resolves_under_its_own_alpha() {
        let t = theme();
        let b = t.box_style("scrim", None);
        assert_eq!(b.fill, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.5 });
        assert_eq!(t.box_radius(&b, 40.0), 0.0);
        assert_eq!(b.border, None);
    }

    #[test]
    fn the_consumer_can_override_the_radius() {
        // Box's own `radius` property wins over the table; the table's value
        // is what it overrides.
        assert_eq!(theme().box_style("card", None).radius, Radius::Px(14.0));
    }

    #[test]
    fn pill_resolves_against_the_box_itself() {
        let t = theme();
        let knob = t.box_style("switch.knob", None);
        assert_eq!(knob.radius, Radius::Pill);
        assert_eq!(t.box_radius(&knob, 40.0f64.min(100.0)), t.pill_radius(40.0));
    }

    #[test]
    fn a_ring_layer_draws_a_band_outside_the_box() {
        let t = theme();
        let cursor = t.box_style("cursor", None);
        assert_eq!(cursor.rings.len(), 1);
        let spread = t.ring_width();
        let ring = geometry::ring(100.0, 40.0, 8.0, spread);
        assert_eq!(ring.x, -spread);
        assert_eq!(ring.width, 100.0 + spread * 2.0);
        assert_eq!(ring.radius, 8.0 + spread);
        assert_eq!(cursor.rings[0].color, hex("#00ccff").with_alpha(0.5));
        assert_eq!(cursor.border.unwrap().color, hex("#00ccff"));
    }

    #[test]
    fn a_hairline_draws_one_line_along_its_own_edge() {
        // A 100x40 box, radius 8, a 1px border: the layer inside the border
        // is 98x38 with an inner corner of 7.
        let inner = geometry::inner_radius(8.0, 1.0, 100.0, 40.0);
        assert_eq!(inner, 7.0);
        let (band, stroke) = geometry::hairline(Edge::Right, 1.0, 98.0, 38.0, inner);
        assert_eq!(band.width, 8.0 - 1.0);
        assert_eq!(band.height, 38.0);
        assert_eq!(band.x, 98.0 - band.width);
        assert_eq!(stroke.radius, 8.0 - 1.0);
        assert_eq!(stroke.x, -band.x);
        assert_eq!(stroke.width, 98.0);
    }

    #[test]
    fn a_box_with_no_hairlines_draws_none() {
        assert!(theme().box_style("card", None).hairlines.is_empty());
    }

    #[test]
    fn a_face_draws_a_gradient_only_when_the_table_sets_one() {
        assert_eq!(theme().box_style("card", None).face, None);
        assert_eq!(geometry::inner_radius(8.0, 1.0, 100.0, 40.0), 8.0 - 1.0);
    }

    #[test]
    fn a_cast_layers_an_effect_only_when_a_layer_blurs() {
        let t = theme();
        assert!(t.box_style("card", None).casts.is_empty());
        assert!(t.box_style("cursor", None).casts.is_empty());

        let casts = [
            Cast { x: 0.0, y: 3.0, blur: 4.0, spread: 0.0, color: hex("#00000026") },
            Cast { x: 0.0, y: 3.0, blur: 3.0, spread: -3.0, color: hex("#00000059") },
        ];
        let pad = geometry::cast_pad(&casts);
        assert_eq!(100.0 + pad * 2.0, 100.0 + (4.0 + 3.0) * 2.0);
        let (first, spread) = geometry::cast(100.0, 40.0, 8.0, &casts[0]);
        assert_eq!(first.width, 100.0);
        assert_eq!(first.radius, 8.0);
        assert_eq!(spread, 0.0);
        let (second, spread) = geometry::cast(100.0, 40.0, 8.0, &casts[1]);
        assert_eq!(spread, 0.0);
        assert_eq!(second.width, 100.0 - 6.0);
        assert_eq!(second.radius, 8.0 - 3.0);
    }

    #[test]
    fn the_pointer_wash_lands_over_the_fill() {
        let t = theme();
        let b = t.box_style("cell", Some("hover"));
        assert_eq!(b.wash, Some(t.hover_fill()));
        assert_eq!(t.box_radius(&b, 40.0), t.radii.md);
    }

    #[test]
    fn a_tinted_state_carries_no_wash() {
        let t = theme();
        let b = t.box_style("button.default", Some("hover"));
        assert_eq!(b.wash, None);
        assert_eq!(b.fill, t.hover_filled(hex("#1133ff")));
    }

    // --- the theme's own resolution

    #[test]
    fn the_preset_scalars_reach_the_theme() {
        let t = Theme::resolve(make_get(json!({ "theme": { "preset": "retro" } })), &sentinel());
        assert_eq!(t.radius, 0.0);
        assert_eq!(t.radii.xl, 0.0);
        assert_eq!(t.surface_opacity, 1.0);
        assert!(!t.blur_behind);
        assert_eq!(t.faces.words, Face { name: None, generic: "monospace" });
        assert_eq!(t.icon_set, "nerd");
        assert_eq!(t.pill_radius(20.0), 0.0);
        assert_eq!(t.frame_radius, 0.0);
    }

    #[test]
    fn a_table_with_no_fonts_block_keeps_the_aliases() {
        let alias = |generic| Face { name: None, generic };
        for (preset, sans) in [("metamorphosis", "sans-serif"), ("pantheon", "sans-serif"), ("retro", "monospace")] {
            let t = Theme::resolve(make_get(json!({ "theme": { "preset": preset } })), &sentinel());
            let want = Faces { words: alias(sans), values: alias("monospace"), display: alias(sans), body: alias(sans) };
            assert_eq!(t.faces, want, "{preset}");
        }
    }

    #[test]
    fn an_accent_too_close_to_the_background_keeps_primary() {
        let c = Colors::default();
        assert_eq!(c.accented("chart2").get("primary"), c.get("primary"));
        assert_eq!(c.accented("chart3").get("primary"), c.get("chart3"));
        assert_eq!(c.accented("chart3").get("chart3"), c.get("chart3"));
    }

    #[test]
    fn a_fonts_block_falls_back_down_its_roles() {
        let full: &'static Value = Box::leak(Box::new(json!({ "fonts": {
            "words": "Archivo", "values": "DotGothic16", "display": "Archivo Narrow", "body": "BIZ UDMincho"
        } })));
        let f = Faces::resolve(full, "pair", true);
        assert_eq!(f.words, Face { name: Some("Archivo"), generic: "sans-serif" });
        assert_eq!(f.values, Face { name: Some("DotGothic16"), generic: "monospace" });
        assert_eq!(f.display, Face { name: Some("Archivo Narrow"), generic: "sans-serif" });
        assert_eq!(f.body, Face { name: Some("BIZ UDMincho"), generic: "sans-serif" });

        let words: &'static Value = Box::leak(Box::new(json!({ "fonts": { "words": "Archivo", "display": "" } })));
        let f = Faces::resolve(words, "pair", true);
        assert_eq!(f.display, f.words);
        assert_eq!(f.body, f.words);
        assert_eq!(f.values, Face { name: None, generic: "monospace" });

        let display: &'static Value = Box::leak(Box::new(json!({ "fonts": { "display": "Archivo" } })));
        let f = Faces::resolve(display, "pair", true);
        assert_eq!(f.words, Face { name: None, generic: "sans-serif" });
        assert_eq!(f.display, Face { name: Some("Archivo"), generic: "sans-serif" });
        assert_eq!(f.body, f.words);
    }

    #[test]
    fn theme_fonts_in_settings_wins_over_the_tables_block() {
        let table: &'static Value = Box::leak(Box::new(json!({ "fonts": { "words": "Archivo", "values": "DotGothic16" } })));
        let f = Faces::resolve(table, "mono", false);
        assert_eq!(f.words, Face { name: None, generic: "monospace" });
        assert_eq!(f.display, f.words);
        assert_eq!(f.values, Face { name: None, generic: "monospace" });
    }

    #[test]
    fn a_malformed_scalar_falls_back_to_the_presets_own() {
        let t = Theme::resolve(
            make_get(json!({ "theme": { "preset": "pantheon", "radius": "big", "surfaceOpacity": 4 } })),
            &sentinel(),
        );
        assert_eq!(t.radius, 6.0);
        assert_eq!(t.surface_opacity, 1.0);
        let t = Theme::resolve(make_get(json!({ "theme": { "radius": 10.6, "surfaceOpacity": -1 } })), &sentinel());
        assert_eq!(t.radius, 11.0);
        assert_eq!(t.surface_opacity, 0.0);
    }

    #[test]
    fn a_table_without_a_frame_reads_the_thickness_as_zero() {
        let get = make_get(json!({ "theme": { "preset": "pantheon" }, "frame": { "thickness": 10 } }));
        let t = Theme::resolve(get, &sentinel());
        assert!(!t.frame_habit);
        assert_eq!(t.frame_requested, 10.0);
        assert_eq!(t.frame_thickness, 0.0);
        let t = Theme::resolve(make_get(json!({ "frame": { "thickness": 10 } })), &sentinel());
        assert_eq!(t.frame_thickness, 10.0);
        assert!(t.frame_enabled());
        assert_eq!(t.frame_radius, 20.0);
    }

    #[test]
    fn the_catalogue_rides_the_switch_and_the_scale() {
        assert_eq!(theme().motion().catalogue, style::Catalogue::default());
        let mut c = Theme::resolve(make_get(json!({ "theme": { "preset": "cartridge" } })), &sentinel());
        c.motion_scale = 10.0;
        let m = c.motion().catalogue;
        assert_eq!(m.reveal, Some(6000.0));
        assert_eq!(m.rows.map(|r| (r.clock.duration, r.stagger, r.rise)), Some((9000.0, 600.0, 40.0)));
        assert_eq!(m.swap.map(|g| (g.out, g.into)), Some((1800.0, 5000.0)));
        c.motion_enabled = false;
        let off = c.motion().catalogue;
        assert_eq!((off.reveal, off.error, off.focus_text.map(|f| f.clock.duration)), (Some(0.0), Some(0.0), Some(0.0)));
    }

    #[test]
    fn motion_follows_the_table_the_switch_and_the_scale() {
        let mut t = theme();
        let m = t.motion();
        assert_eq!(m.emerge, 500.0);
        assert_eq!(m.restack_stagger, 0.0);
        assert_eq!(m.emerge_curve, Easing::Bezier(tokens::curve("spatial").unwrap().to_vec()));
        // The toast's own clock is sonner's: 400ms on CSS `ease`, never past rest.
        assert_eq!((m.arrive, m.restack), (400.0, 400.0));
        assert_eq!(m.restack_curve, Easing::Bezier(vec![0.25, 0.1, 0.25, 1.0, 1.0, 1.0]));

        let p = Theme::resolve(make_get(json!({ "theme": { "preset": "pantheon" } })), &sentinel()).motion();
        assert_eq!(p.emerge, 150.0);
        assert_eq!(p.restack_stagger, 150.0);
        assert_eq!(p.restack_curve, Easing::Bezier(tokens::curve("emphasizedDecel").unwrap().to_vec()));

        t.motion_scale = 10.0;
        assert_eq!(t.motion().emerge, 5000.0);
        assert_eq!(t.motion().families.spatial, 5000.0);
        let off = Theme::resolve(make_get(json!({ "motion": { "enabled": "yes" } })), &sentinel());
        assert!(!off.motion_enabled);
        assert_eq!(off.motion().emerge, 0.0);
        assert_eq!(off.motion().families.marquee_px_per_sec, 30.0);
    }

    #[test]
    fn the_cursor_ring_reserves_its_spread() {
        assert_eq!(theme().ring_width(), 3.0);
        let p = Theme::resolve(make_get(json!({ "theme": { "preset": "pantheon" } })), &sentinel());
        assert_eq!(p.ring_width(), 2.0);
    }

    #[test]
    fn an_unknown_palette_role_draws_nothing() {
        assert_eq!(theme().colors.get("nonsense"), Rgba::TRANSPARENT);
        assert_eq!(Colors::default().get("card"), Rgba::hex(0x18181b));
    }
}
