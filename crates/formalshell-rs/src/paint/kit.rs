//! Shaping, cached by text and style, with the look it shapes for.

use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::Role;
use fs_theme::theme::{Face, Theme};
use fs_theme::tokens::WEIGHTS;
use parley::GenericFamily;

use crate::paint::Painter;
use fs_types::IRect;
use crate::text::{Family, ShapedText, Text, TextStyle};

/// What the theme hands every cell: tokens, inks and the `cell` role's
/// states, read once per theme.
#[derive(Clone)]
pub struct Look {
    pub pad_x: f64,
    pub pad_y: f64,
    pub cell_width: f64,
    pub xxs: f64,
    pub huge: f64,
    /// How far a cut marquee label fades in from each cut end (`xxl`).
    pub fade: f64,
    pub xs: f64,
    pub sm: f64,
    pub md: f64,
    pub body: f32,
    pub caption: f32,
    /// `letterSpacing.meta`, the tracking of a meta label.
    pub meta_tracking: f32,
    pub narrow: f64,
    pub sans: Family,
    pub mono: Family,
    pub foreground: Rgba,
    pub muted: Rgba,
    pub on_active: Rgba,
    pub destructive: Rgba,
    pub warning: Rgba,
    pub primary: Rgba,
    pub background: Rgba,
    /// The flat tracks' trough, its thickness and its corner (`muted`,
    /// `trackThickness`, `radiusSm`).
    pub muted_fill: Rgba,
    pub border: Rgba,
    pub track: f64,
    pub radius_sm: f64,
    pub radius: f32,
    pub active_fill: Rgba,
    pub hover_wash: Option<Rgba>,
    pub ghost_open: Rgba,
    pub destructive_border: Option<(Rgba, f32)>,
    pub warning_border: Option<(Rgba, f32)>,
    pub border_width: f64,
    pub effects: f64,
    pub effects_slow: f64,
    pub spatial: f64,
    pub spatial_fast: f64,
    pub emphasized: f64,
    pub motion: bool,
    /// The theme's `motion_scale`, for a clock the table has no token for.
    pub motion_scale: f64,
    pub marquee_px: f64,
    pub marquee_hold: f64,
    /// The box under every cell (`bar.cell`), none where it draws nothing.
    pub chip: Option<fs_theme::style::BoxStyle<Rgba>>,
}

/// A theme text role's family: its fontconfig alias.
pub fn face(face: Face) -> Family {
    Family::Generic(match face {
        Face::SansSerif => GenericFamily::SansSerif,
        Face::Monospace => GenericFamily::Monospace,
    })
}

impl Look {
    pub fn new(theme: &Theme) -> Self {
        let s = &theme.space;
        let cell = |state| theme.box_style(Role::Cell, Some(state));
        let border = |state| cell(state).border.map(|l| (l.color, l.width as f32));
        let motion = theme.motion().families;
        Self {
            pad_x: s.control_padding_x,
            pad_y: s.control_padding_y,
            cell_width: s.bar_cell_width,
            xxs: s.xxs,
            huge: s.xl,
            fade: s.lg,
            xs: s.xs,
            sm: s.sm,
            md: s.sm,
            body: theme.font_size.body as f32,
            caption: theme.font_size.caption as f32,
            meta_tracking: theme.letter_spacing.meta as f32,
            narrow: s.popup_width_narrow,
            sans: face(theme.faces.words),
            mono: face(theme.faces.values),
            foreground: theme.colors.get(ColorRole::Foreground),
            muted: theme.colors.get(ColorRole::MutedForeground),
            destructive: theme.colors.get(ColorRole::Destructive),
            warning: theme.colors.get(ColorRole::Warning),
            primary: theme.colors.get(ColorRole::Primary),
            background: theme.colors.get(ColorRole::Background),
            muted_fill: theme.colors.get(ColorRole::Muted),
            border: theme.colors.get(ColorRole::Border),
            track: s.track_thickness,
            radius_sm: theme.radii.sm,
            radius: theme.box_radius(&theme.box_style(Role::Cell, None), s.bar_cell_height) as f32,
            on_active: theme.colors.get(ColorRole::PrimaryForeground),
            active_fill: cell("active").fill,
            hover_wash: cell("hover").wash,
            ghost_open: cell("ghostOpen").fill,
            destructive_border: border("destructive"),
            warning_border: border("warning"),
            border_width: theme.border_width,
            effects: motion.effects,
            effects_slow: motion.effects_slow,
            spatial: motion.spatial,
            spatial_fast: motion.spatial_fast,
            emphasized: motion.emphasized,
            motion: theme.motion_enabled,
            motion_scale: theme.motion_scale,
            marquee_px: motion.marquee_px_per_sec,
            marquee_hold: motion.marquee_hold_ms,
            chip: Some(theme.box_style(Role::BarCell, None)).filter(|b| b.fill.a > 0.0 || b.border.as_ref().is_some_and(|l| l.width > 0.0)),
        }
    }

    /// The strip's room across a vertical cell's content.
    pub fn content_across(&self) -> f64 {
        (self.cell_width - self.pad_y * 2.0).max(0.0)
    }

    pub fn label(&self, weight: f32) -> TextStyle {
        TextStyle { family: self.mono, size: self.body, weight, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }

    pub fn sans(&self, weight: f32) -> TextStyle {
        TextStyle { family: self.sans, size: self.body, weight, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }
}

/// Shaping, cached by text and style, with the look it shapes for.
pub struct Kit {
    pub text: Text,
    pub look: Look,
    /// While set, every style shaped in, so a surface's styles can be
    /// warmed ahead of its next open.
    pub seen: Option<Vec<TextStyle>>,
}

impl Kit {
    pub fn new(theme: &Theme) -> Self {
        Self { text: Text::new(), look: Look::new(theme), seen: None }
    }

    pub fn set_theme(&mut self, theme: &Theme) {
        self.look = Look::new(theme);
    }

    pub fn shape(&mut self, source: &str, style: TextStyle) -> ShapedText {
        if let Some(seen) = &mut self.seen
            && !seen.contains(&style)
        {
            seen.push(style);
        }
        self.text.shape(source, style)
    }

    /// `source` cut to `max` with an ellipsis, or whole.
    pub fn elided(&mut self, source: &str, style: TextStyle, max: f64) -> ShapedText {
        let whole = self.shape(source, style);
        if whole.width as f64 <= max + 0.5 || source.is_empty() {
            return whole;
        }
        let chars: Vec<char> = source.chars().collect();
        let cut = |n: usize| chars[..n].iter().collect::<String>().trim_end().to_owned() + "\u{2026}";
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi {
            let mid = (lo + hi).div_ceil(2);
            if self.shape(&cut(mid), style).width as f64 <= max { lo = mid } else { hi = mid - 1 }
        }
        self.shape(&cut(lo), style)
    }

    /// A style in `family` at `size`, with no tracking and the normal width.
    pub fn weight(&self, family: Family, size: f32, weight: f64) -> TextStyle {
        TextStyle { family, size, weight: weight as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
    }

    /// The icon `name` at `size`, centred in `within`.
    pub fn glyph(&mut self, p: &mut Painter, name: &str, size: f32, within: IRect, color: Rgba) {
        let g = fs_theme::icons::glyph(name);
        let shaped = self.shape(g.text, self.weight(Family::Named(g.family), size, WEIGHTS.normal));
        p.text(&shaped, (within.x + (within.w - shaped.width) / 2, within.y + (within.h - shaped.line_height()) / 2), color, &[]);
    }

    pub fn icon(&mut self, name: &str) -> ShapedText {
        let g = fs_theme::icons::glyph(name);
        let style = TextStyle { family: Family::Named(g.family), size: self.look.body, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        self.shape(g.text, style)
    }
}
