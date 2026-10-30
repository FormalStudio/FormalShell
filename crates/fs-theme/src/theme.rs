//! What the theme resolves: the one table against settings.json, the
//! palette theme.json carries, the tokens at the live scale, and `box()`
//! against all three. Settings come in through `get` (one dotted path,
//! `None` when absent) so this holds no config of its own.

use serde_json::{Map, Value};

use crate::barpaint::{Paint, Pin};
use crate::chrome;
use crate::color::Rgba;
use crate::palette::{self, COLOR_KEYS};
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
}

/// One text role's family: a face's name, and the fontconfig alias that
/// stands in when it names none or the face is missing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub name: Option<&'static str>,
    pub generic: &'static str,
}

/// The four text roles: `words` for labels, `values` for numbers and
/// status, `display` for headings and titles, `body` for secondary lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faces {
    pub words: Face,
    pub values: Face,
    pub display: Face,
    pub body: Face,
}

impl Faces {
    /// Every role on its fontconfig alias: `values` on `monospace`, the
    /// rest on `sans-serif`.
    pub const ALIASES: Self = {
        let sans = Face { name: None, generic: "sans-serif" };
        Self { words: sans, values: Face { name: None, generic: "monospace" }, display: sans, body: sans }
    };
}

/// `layout`: where the shell's chrome sits. `sidebar` is read ahead of the
/// sidebar itself and draws the bar until it exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Bar,
    Sidebar,
}

impl Layout {
    /// An unknown value reads as the default.
    fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("sidebar") => Self::Sidebar,
            _ => Self::Bar,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bar => "bar",
            Self::Sidebar => "sidebar",
        }
    }
}

/// `bar.style`: the band's paint. `corners` is `solid` with a fillet at
/// each end of the band's inner edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarStyle {
    Transparent,
    Solid,
    Corners,
}

impl BarStyle {
    /// The band's paint rule: wingpanel's without the busy branch, or the
    /// `maximized` paint whatever is under the band.
    pub fn pin(self) -> Pin {
        match self {
            Self::Transparent => Pin::Transparent,
            Self::Solid | Self::Corners => Pin::Paint(Paint::Maximized),
        }
    }

    /// An unknown value reads as the default.
    fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("solid") => Self::Solid,
            Some("corners") => Self::Corners,
            _ => Self::Transparent,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transparent => "transparent",
            Self::Solid => "solid",
            Self::Corners => "corners",
        }
    }
}

/// `render.backend` as asked. Reserved: every value draws with `vello_cpu`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Auto,
    Gpu,
    Cpu,
}

impl Backend {
    /// An unknown value reads as the default.
    fn parse(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_str) {
            Some("gpu") => Self::Gpu,
            Some("cpu") => Self::Cpu,
            _ => Self::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Gpu => "gpu",
            Self::Cpu => "cpu",
        }
    }
}

/// Settings keys earlier versions read and this one ignores, each logged
/// once per start where it is still written.
pub const REMOVED_KEYS: [&str; 10] = [
    "theme.preset",
    "theme.radius",
    "theme.icons",
    "theme.fonts",
    "theme.dither",
    "theme.grid",
    "theme.boot",
    "theme.sounds",
    "theme.soundVolume",
    "bar.paint",
];

/// The [`REMOVED_KEYS`] settings.json still writes.
pub fn removed_keys(get: impl Fn(&str) -> Option<Value>) -> Vec<&'static str> {
    REMOVED_KEYS.into_iter().filter(|k| get(k).is_some()).collect()
}

/// tsubi.club's five swatches (`--color-1` to `--color-5`), the names
/// `wallpaper.color` takes besides a hex.
pub const WALLPAPER_COLORS: [(&str, &str); 5] =
    [("sage", "#a1c8bc"), ("purple", "#402484"), ("green", "#68bc64"), ("silver", "#bdc3c9"), ("orange", "#ff8000")];

/// `wallpaper.color` as written: a `#rrggbb` hex (lowercased) or one of
/// [`WALLPAPER_COLORS`]' names gives that colour, `false` or `null` the
/// picture, and anything else is malformed, `None`.
pub fn wallpaper_color(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Bool(false) | Value::Null => Some(None),
        Value::String(s) => {
            let s = s.trim().to_lowercase();
            if let Some((_, hex)) = WALLPAPER_COLORS.iter().find(|(name, _)| *name == s) {
                return Some(Some((*hex).to_owned()));
            }
            let digits = s.strip_prefix('#')?;
            (digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_hexdigit())).then(|| Some(s.clone()))
        }
        _ => None,
    }
}

/// A value that is not a real boolean is a malformed key rather than a
/// falsy one, so `"true"` and `1` take the default.
fn boolean(value: Option<Value>, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(b)) => b,
        _ => fallback,
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub style: &'static Value,
    pub colors: Colors,
    pub radii: Radii,
    pub border_width: f64,
    pub surface_opacity: f64,
    pub blur_behind: bool,
    pub faces: Faces,
    pub wallpaper_dither: bool,
    pub lock_dither: bool,
    pub wallpaper_grid: bool,
    pub lock_grid: bool,
    pub wallpaper_animate: bool,
    pub wallpaper_color: Option<String>,
    pub font_base_size: f64,
    pub font_scale: f64,
    pub font_size: FontTokens,
    pub spacing_scale: f64,
    pub space: Space,
    pub letter_spacing: LetterSpacing,
    pub motion_enabled: bool,
    pub motion_scale: f64,
    pub layout: Layout,
    pub bar_style: BarStyle,
    /// `motion.morph`: a card buds off the bar's inner edge and morphs
    /// between sizes, or drops from its cell as a popover.
    pub morph: bool,
    pub render_backend: Backend,
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
        let style = crate::table();
        let surface_opacity = tokens::clamp(get("theme.surfaceOpacity").as_ref(), 0.0, 1.0, 0.85);

        let font_base_size = 13.0;
        let font_scale = tokens::font_scale(font_base_size);
        let spacing_scale = font_scale;

        let frame_thickness =
            tokens::js_round(tokens::clamp(get("frame.thickness").as_ref(), 0.0, f64::INFINITY, 0.0));
        let frame_radius = tokens::js_round(tokens::clamp(get("frame.radius").as_ref(), 0.0, f64::INFINITY, 15.0));

        Self {
            style,
            colors: Colors::from_palette(palette),
            radii: tokens::RADII,
            border_width: 1.0,
            surface_opacity,
            blur_behind: boolean(get("theme.blur"), true),
            faces: Faces::ALIASES,
            wallpaper_dither: boolean(get("wallpaper.dither"), false),
            lock_dither: boolean(get("lock.dither"), false),
            wallpaper_grid: boolean(get("wallpaper.grid"), false),
            lock_grid: boolean(get("lock.grid"), false),
            wallpaper_animate: boolean(get("wallpaper.animate"), false),
            wallpaper_color: get("wallpaper.color").as_ref().and_then(wallpaper_color).flatten(),
            font_base_size,
            font_scale,
            font_size: tokens::font_tokens(font_base_size),
            spacing_scale,
            space: tokens::spacing_tokens(spacing_scale),
            letter_spacing: tokens::letter_spacing_tokens(font_scale),
            motion_enabled: get("motion.enabled").unwrap_or(Value::Bool(true)) == Value::Bool(true),
            motion_scale: 1.0,
            layout: Layout::parse(get("layout").as_ref()),
            bar_style: BarStyle::parse(get("bar.style").as_ref()),
            morph: boolean(get("motion.morph"), true),
            render_backend: Backend::parse(get("render.backend").as_ref()),
            frame_thickness,
            frame_radius,
        }
    }

    pub fn frame_enabled(&self) -> bool {
        self.frame_thickness > 0.0
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
        if on { style::with_cursor(&b, &self.box_style("cursor", None), halo) } else { b }
    }

    /// A number a role carries beside its box.
    pub fn metric(&self, role: &str, key: &str) -> f64 {
        style::metric(self.style, role, key)
    }

    pub fn pill_radius(&self, extent: f64) -> f64 {
        extent / 2.0
    }

    pub fn box_radius(&self, b: &BoxStyle<Rgba>, extent: f64) -> f64 {
        match b.radius {
            Radius::Pill => self.pill_radius(extent),
            Radius::Px(px) => px,
        }
    }

    /// The `card` role's radius, which a card joined to the bar also opens
    /// its fillets at.
    pub fn card_radius(&self) -> f64 {
        match self.box_style("card", None).radius {
            Radius::Pill => self.radii.xl,
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

    /// What formalshell-chrome.lua carries: the window radius (`xl`, libadwaita's `--window-radius`), the blur switch and
    /// the `window` role's two states unresolved, since Hyprland draws them
    /// off a palette role's name rather than a colour.
    pub fn window_chrome(&self) -> chrome::Chrome {
        chrome::Chrome::from_table(self.style, Value::from(self.radii.xl), Value::Bool(self.blur_behind))
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(border.color, Rgba::BLACK.with_alpha(0.75));
        assert_eq!(border.width, t.border_width);
        assert_eq!(t.box_radius(&b, 40.0), 12.0);
        assert_eq!(b.casts.len(), 2);
        assert!(b.rings.is_empty());
    }

    #[test]
    fn a_role_with_no_border_draws_none() {
        let t = theme();
        let b = t.box_style("cell.mark", None);
        assert_eq!(b.fill, hex("#1133ff"));
        assert_eq!(b.border, None);
        assert_eq!(t.box_radius(&b, 40.0), t.radii.sm);
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
        assert_eq!(b.fill, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 125.0 / 255.0 });
        assert_eq!(t.box_radius(&b, 40.0), 0.0);
        assert_eq!(b.border, None);
    }

    #[test]
    fn the_consumer_can_override_the_radius() {
        // Box's own `radius` property wins over the table; the table's value
        // is what it overrides.
        assert_eq!(theme().box_style("card", None).radius, Radius::Px(12.0));
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
        let ring = geometry::ring(100.0, 40.0, 8.0, 2.0);
        assert_eq!(ring.x, -2.0);
        assert_eq!(ring.width, 104.0);
        assert_eq!(ring.radius, 10.0);
        let cursor = theme().box_style("cursor", None);
        assert!(cursor.rings.is_empty());
        assert_eq!(cursor.border.unwrap().color, hex("#00ccff").with_alpha(0.5));
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
        assert!(theme().box_style("scrim", None).hairlines.is_empty());
        assert_eq!(theme().box_style("card", None).hairlines.len(), 4);
    }

    #[test]
    fn a_face_draws_a_gradient_only_when_the_table_sets_one() {
        assert_eq!(theme().box_style("card", None).face, None);
        assert_eq!(geometry::inner_radius(8.0, 1.0, 100.0, 40.0), 8.0 - 1.0);
    }

    #[test]
    fn a_cast_layers_an_effect_only_when_a_layer_blurs() {
        let t = theme();
        assert_eq!(t.box_style("card", None).casts.len(), 2);
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
        assert_eq!(t.box_radius(&b, 40.0), 9.0);
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
    fn the_defaults_are_the_one_look() {
        let t = theme();
        assert_eq!((t.radii.md, t.radii.xl, t.surface_opacity), (9.0, 15.0, 0.85));
        assert!(t.blur_behind);
        assert_eq!(t.faces, Faces::ALIASES);
        assert_eq!(t.faces.words, Face { name: None, generic: "sans-serif" });
        assert!(!t.wallpaper_dither && !t.lock_dither && !t.wallpaper_grid && !t.lock_grid && !t.wallpaper_animate);
        assert_eq!(t.wallpaper_color, None);
        assert_eq!((t.layout, t.bar_style, t.morph, t.render_backend), (Layout::Bar, BarStyle::Transparent, true, Backend::Auto));
        assert_eq!((t.frame_thickness, t.frame_radius), (0.0, 15.0));
        assert!(!t.frame_enabled());
    }

    #[test]
    fn a_removed_key_is_ignored_and_reported() {
        let written = json!({ "theme": { "preset": "square", "radius": 0, "icons": "nerd", "dither": true, "grid": true }, "bar": { "paint": "dark" } });
        let t = Theme::resolve(make_get(written.clone()), &sentinel());
        assert_eq!((t.radii.sm, t.surface_opacity), (6.0, 0.85));
        assert!(!t.wallpaper_dither && !t.lock_dither && !t.wallpaper_grid && !t.lock_grid);
        assert_eq!(t.bar_style, BarStyle::Transparent);
        assert_eq!(removed_keys(make_get(written)), ["theme.preset", "theme.radius", "theme.icons", "theme.dither", "theme.grid", "bar.paint"]);
        assert!(removed_keys(make_get(json!({ "theme": { "mode": "dark" } }))).is_empty());
    }

    #[test]
    fn the_image_keys_are_read_on_their_own() {
        let t = Theme::resolve(make_get(json!({ "wallpaper": { "dither": true, "grid": true, "animate": true }, "lock": { "grid": true } })), &sentinel());
        assert!(t.wallpaper_dither && !t.lock_dither && t.wallpaper_grid && t.lock_grid && t.wallpaper_animate);
        let t = Theme::resolve(make_get(json!({ "wallpaper": { "dither": "true", "animate": 1 } })), &sentinel());
        assert!(!t.wallpaper_dither && !t.wallpaper_animate);
    }

    #[test]
    fn wallpaper_color_takes_a_hex_or_a_name_and_false_gives_the_picture_back() {
        let color = |v: Value| Theme::resolve(make_get(json!({ "wallpaper": { "color": v } })), &sentinel()).wallpaper_color;
        assert_eq!(color(json!("#FF8000")).as_deref(), Some("#ff8000"));
        assert_eq!(color(json!("sage")).as_deref(), Some("#a1c8bc"));
        assert_eq!(color(json!(false)), None);
        assert_eq!(color(json!(null)), None);
        for junk in [json!("#40248"), json!("402484"), json!("violet"), json!(true), json!(4204164)] {
            assert_eq!(color(junk.clone()), None, "{junk}");
            assert_eq!(wallpaper_color(&junk), None, "{junk}");
        }
        assert_eq!(wallpaper_color(&json!(false)), Some(None));
    }

    #[test]
    fn a_glass_fill_goes_opaque_with_the_cards() {
        let mut t = theme();
        assert_eq!(t.box_style("switcher", None).fill.a, 0.6);
        assert_eq!(t.box_style("popover", None).fill.a, 0.9);
        t.surface_opacity = 1.0;
        assert_eq!(t.box_style("switcher", None).fill.a, 1.0);
        assert_eq!(t.box_style("popover", None).fill.a, 1.0);
    }

    #[test]
    fn a_malformed_scalar_falls_back_to_the_default() {
        let t = Theme::resolve(make_get(json!({ "theme": { "surfaceOpacity": 4, "blur": "false" } })), &sentinel());
        assert_eq!(t.surface_opacity, 1.0);
        assert!(t.blur_behind);
        let t = Theme::resolve(make_get(json!({ "theme": { "surfaceOpacity": -1, "blur": false } })), &sentinel());
        assert_eq!(t.surface_opacity, 0.0);
        assert!(!t.blur_behind);
    }

    #[test]
    fn the_frame_thickness_alone_draws_the_ring() {
        let t = Theme::resolve(make_get(json!({ "frame": { "thickness": 10 } })), &sentinel());
        assert_eq!((t.frame_thickness, t.frame_radius), (10.0, 15.0));
        assert!(t.frame_enabled());
        let t = Theme::resolve(make_get(json!({ "frame": { "thickness": 6, "radius": 20 } })), &sentinel());
        assert_eq!(t.frame_radius, 20.0);
    }

    #[test]
    fn a_written_key_wins_and_an_unknown_or_malformed_one_reads_as_the_default() {
        let written = json!({
            "layout": "sidebar",
            "bar": { "style": "corners" },
            "motion": { "morph": false },
            "render": { "backend": "gpu" },
        });
        let t = Theme::resolve(make_get(written), &sentinel());
        assert_eq!((t.layout, t.bar_style, t.morph, t.render_backend), (Layout::Sidebar, BarStyle::Corners, false, Backend::Gpu));
        let get = make_get(json!({
            "layout": "dock",
            "bar": { "style": "auto" },
            "motion": { "morph": "false" },
            "render": { "backend": "vulkan" },
        }));
        let t = Theme::resolve(get, &sentinel());
        assert_eq!((t.layout, t.bar_style, t.morph, t.render_backend), (Layout::Bar, BarStyle::Transparent, true, Backend::Auto));
        assert_eq!(BarStyle::Transparent.pin(), Pin::Transparent);
        assert_eq!(BarStyle::Corners.pin(), Pin::Paint(Paint::Maximized));
    }

    #[test]
    fn motion_follows_the_table_the_switch_and_the_scale() {
        let mut t = theme();
        let m = t.motion();
        assert_eq!(m.emerge, 150.0);
        assert_eq!(m.restack_stagger, 150.0);
        assert_eq!(m.restack_curve, Easing::Bezier(tokens::curve("emphasizedDecel").unwrap().to_vec()));

        t.motion_scale = 10.0;
        assert_eq!(t.motion().emerge, 1500.0);
        assert_eq!(t.motion().families.spatial, 5000.0);
        let off = Theme::resolve(make_get(json!({ "motion": { "enabled": "yes" } })), &sentinel());
        assert!(!off.motion_enabled);
        assert_eq!(off.motion().emerge, 0.0);
        assert_eq!(off.motion().families.marquee_px_per_sec, 30.0);
    }

    #[test]
    fn the_cursor_ring_draws_inside_the_box() {
        assert_eq!(theme().ring_width(), 0.0);
    }

    #[test]
    fn an_unknown_palette_role_draws_nothing() {
        assert_eq!(theme().colors.get("nonsense"), Rgba::TRANSPARENT);
        assert_eq!(Colors::default().get("card"), Rgba::hex(0x18181b));
    }
}
