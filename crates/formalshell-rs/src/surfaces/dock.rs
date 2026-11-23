//! The dock's row (docs/superpowers/specs/2026-11-11-mac-apps.md, section
//! 4): one cell per app, the pinned ones, a rule, then the open ones. A
//! cell is the app's icon over the dot that says it has a window, with the
//! marks a Mac app's dock tile carries on top: its badge, its progress
//! along the icon's foot, and the hops of one that asks for attention.

use std::time::Instant;

use fs_theme::palette::ColorRole;
use fs_theme::theme::Theme;
use fs_types::IRect;
use vello_cpu::kurbo::Rect;

use crate::ui::el::{Font, Pic};
use crate::ui::{self, Cx, El, State, Type, Weight, Widget, irect, w};

/// An icon's side, and the room between two.
pub const ICON: f64 = 48.0;
pub const GAP: f64 = 6.0;
/// The running dot's diameter.
const DOT: f64 = 4.0;
/// A rounded square icon's corner as a share of its side, which the card's
/// own corner follows a padding out, so the two curves are concentric.
const ICON_CORNER: f64 = 0.225;
/// A hop off a launch, measured off macOS's Dock frame by frame: half the
/// icon high, 0.65 s up and down on a gravity parabola, landing for good
/// on the first touchdown once the window is up.
const LAUNCH_HOP: f64 = 0.65;
const LAUNCH_RISE: f64 = 0.5;
/// A hop asking for attention: macOS's critical request, taller and
/// slower, a second each (its informational request is one such hop), over
/// and over until the app is looked at.
const ATTENTION_HOP: f64 = 1.0;
const ATTENTION_RISE: f64 = 0.75;
/// A launching icon's opacity with motion off, which takes the hops away.
const LAUNCHING_STILL: f32 = 0.6;
/// A pressed icon's opacity, macOS's darker icon under the button.
const PRESSED: f32 = 0.7;

/// A press on an app's cell.
pub struct Press {
    pub index: usize,
    pub app: String,
}

impl ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dock:{}", self.app)
    }
}

/// One app as the dock draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub app: String,
    pub name: String,
    pub image: Pic,
    pub running: bool,
    pub focused: bool,
    pub badge: Option<String>,
    pub urgent: bool,
    /// 0 to 1 while the app shows progress.
    pub progress: Option<f64>,
    /// Started from here, its window not mapped yet.
    pub launching: bool,
    /// Which way is away from the dock's edge: -1 up off the bottom, 1
    /// down off the top.
    pub away: f64,
}

/// The card's padding round the icons. The running dot sits in the
/// padding under its icon, as macOS's does.
pub fn padding(theme: &Theme) -> f64 {
    theme.space.md
}

/// The card's corner, concentric with the icons' own a padding in.
pub fn radius(theme: &Theme) -> f64 {
    padding(theme) + ICON * ICON_CORNER
}

/// How far past the card's far side an icon may stand mid hop.
pub fn reach(theme: &Theme) -> i32 {
    (ICON * LAUNCH_RISE.max(ATTENTION_RISE) - padding(theme)).ceil().max(0.0) as i32
}

/// The row's (width, height) for `count` apps, with the rule or without.
pub fn size(theme: &Theme, count: usize, rule: bool) -> (f64, f64) {
    let n = count.max(1) as f64;
    let ruled = if rule { GAP + theme.border_width } else { 0.0 };
    (n * ICON + (n - 1.0) * GAP + ruled, ICON)
}

pub fn row(marks: Vec<Mark>, rule: Option<usize>) -> El {
    let mut cells = Vec::with_capacity(marks.len() + 1);
    for (index, mark) in marks.into_iter().enumerate() {
        if rule == Some(index) {
            cells.push(w::separator_vertical().height(ICON));
        }
        let (press, tip) = (Press { index, app: mark.app.clone() }, mark.name.clone());
        cells.push(El::custom(mark).on(press).tip(tip));
    }
    w::row(GAP, cells)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    Launch,
    Attention,
}

impl Ask {
    fn hop(self) -> f64 {
        match self {
            Ask::Launch => LAUNCH_HOP,
            Ask::Attention => ATTENTION_HOP,
        }
    }

    fn rise(self) -> f64 {
        match self {
            Ask::Launch => LAUNCH_RISE,
            Ask::Attention => ATTENTION_RISE,
        }
    }
}

/// An icon's hops: what they are for, when they started, and once the
/// reason is gone, the touchdown they end on.
#[derive(Default)]
struct Hops {
    on: Option<(Ask, Instant)>,
    lands: Option<f64>,
}

impl State for Hops {}

/// How high an icon stands `t` seconds into a hop `period` long, as a
/// share of its peak: a body thrown up under gravity, fastest off the
/// ground and still at the top.
pub fn hop(t: f64, period: f64) -> f64 {
    let p = (t / period).rem_euclid(1.0);
    4.0 * p * (1.0 - p)
}

/// The touchdown at or after `t` seconds into hops `period` long.
fn touchdown(t: f64, period: f64) -> f64 {
    (t / period).ceil() * period
}

impl Widget for Mark {
    fn measure(&self, _cx: &mut Cx, _avail: f64) -> (f64, f64) {
        (ICON, ICON)
    }

    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str) {
        let t = cx.theme;
        let moving = t.motion_enabled;
        let (now, scale) = (cx.now, t.motion_scale.max(f64::EPSILON));
        let want = match () {
            () if self.urgent => Some(Ask::Attention),
            () if self.launching => Some(Ask::Launch),
            () => None,
        };
        let mut hops = cx.take(path, Hops::default);
        let lift = match (want.filter(|_| moving), hops.on) {
            (Some(ask), Some((was, _))) if ask == was => {
                hops.lands = None;
                None
            }
            (Some(ask), _) => {
                *hops = Hops { on: Some((ask, now)), lands: None };
                None
            }
            (None, None) => None,
            (None, Some((ask, since))) => {
                let t = now.saturating_duration_since(since).as_secs_f64() / scale;
                let lands = *hops.lands.get_or_insert_with(|| touchdown(t, ask.hop()));
                if t >= lands {
                    *hops = Hops::default();
                    Some(0.0)
                } else {
                    None
                }
            }
        };
        let lift = lift.unwrap_or_else(|| {
            hops.on.map_or(0.0, |(ask, since)| {
                let t = now.saturating_duration_since(since).as_secs_f64() / scale;
                hop(t, ask.hop()) * ask.rise() * ICON
            })
        });
        let airborne = hops.on.is_some();
        cx.keep(hops);
        if airborne {
            cx.animate();
        }
        let pressed = cx.pressed(path);
        let alpha = cx.alpha
            * match () {
                () if pressed => PRESSED,
                () if self.launching && !moving => LAUNCHING_STILL,
                () => 1.0,
            };

        let y = inner.y0 + lift * self.away;
        let icon = irect(Rect::new(inner.x0, y, inner.x0 + ICON, y + ICON));
        let glyph = self.image.0.is_none().then(|| ui::icon(cx, "app-window", Type::DisplayLarge));
        let badge = self.badge.as_ref().map(|text| ui::shape(cx, text, &Font { size: Type::Caption, weight: Weight::Semibold, mono: false }));
        let ink = |role: ColorRole| {
            let c = t.colors.get(role);
            c.with_alpha(c.a * alpha)
        };
        let dot_ink = match () {
            () if self.urgent && !moving => ink(ColorRole::Warning),
            () if self.focused => ink(ColorRole::Foreground),
            () => ink(ColorRole::MutedForeground),
        };
        let mut p = cx.painter(path);
        match (&self.image.0, &glyph) {
            (Some(image), _) => {
                let (w, h) = image.size();
                p.image(image, (icon.x + (icon.w - w) / 2, icon.y + (icon.h - h) / 2), alpha);
            }
            (None, Some(g)) => {
                let at = (icon.x + (icon.w - g.width) / 2, icon.y + (icon.h - g.line_height()) / 2);
                p.text(g, at, ink(ColorRole::MutedForeground), &[]);
            }
            (None, None) => {}
        }
        if let Some(value) = self.progress {
            let inset = t.space.xs.round() as i32;
            let track = IRect::new(icon.x + inset, icon.bottom() - inset - DOT as i32, icon.w - inset * 2, DOT as i32);
            let done = IRect::new(track.x, track.y, (f64::from(track.w) * value.clamp(0.0, 1.0)).round() as i32, track.h);
            let radius = DOT as f32 / 2.0;
            p.rect(track, ink(ColorRole::Muted), radius);
            p.rect(done, ink(ColorRole::Primary), radius);
        }
        if self.running {
            let d = DOT as i32;
            // Centred in the padding on the edge's side of the icon.
            let gap = (padding(t) - DOT) / 2.0;
            let y = if self.away < 0.0 { inner.y1 + gap } else { inner.y0 - gap - DOT };
            let dot = IRect::new((inner.x0 + (ICON - DOT) / 2.0).round() as i32, y.round() as i32, d, d);
            p.rect(dot, dot_ink, DOT as f32 / 2.0);
        }
        if let Some(text) = &badge {
            let h = text.line_height() + 2;
            let w = (text.width + t.space.xs.round() as i32 * 2).max(h);
            let pill = IRect::new(icon.right() - w + 2, icon.y - 2, w, h);
            p.rect(pill, ink(ColorRole::Destructive), h as f32 / 2.0);
            p.text(text, (pill.x + (pill.w - text.width) / 2, pill.y + 1), ink(ColorRole::DestructiveForeground), &[]);
        }
        let last = p.last();
        p.finish();
        cx.done(last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hop_is_a_parabola_landing_on_its_period() {
        assert_eq!(hop(0.0, LAUNCH_HOP), 0.0);
        assert!((hop(LAUNCH_HOP / 2.0, LAUNCH_HOP) - 1.0).abs() < 1e-9);
        assert!(hop(LAUNCH_HOP - 1e-6, LAUNCH_HOP) < 1e-4);
        let (rising, falling) = (hop(LAUNCH_HOP * 0.25, LAUNCH_HOP), hop(LAUNCH_HOP * 0.75, LAUNCH_HOP));
        assert!((rising - falling).abs() < 1e-9 && (rising - 0.75).abs() < 1e-9);
        assert!((hop(LAUNCH_HOP * 2.5, LAUNCH_HOP) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn hops_end_on_the_next_touchdown() {
        assert_eq!(touchdown(0.0, 1.0), 0.0);
        assert_eq!(touchdown(0.3, 1.0), 1.0);
        assert_eq!(touchdown(2.0, 1.0), 2.0);
        assert!((touchdown(0.7, LAUNCH_HOP) - 2.0 * LAUNCH_HOP).abs() < 1e-9);
    }
}
