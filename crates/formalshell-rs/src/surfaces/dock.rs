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
use crate::ui::{self, Cx, El, Size, State, Type, Weight, Widget, irect, w};

/// An icon's side, and the room between two.
pub const ICON: f64 = 48.0;
pub const GAP: f64 = 9.0;
/// The running dot's diameter.
const DOT: f64 = 4.0;
/// One hop of an icon asking for attention, in seconds, how many it takes
/// and how often it takes them again.
const HOP: f64 = 0.4;
const HOPS: f64 = 3.0;
const AGAIN: f64 = 4.0;
/// A launching icon's breath, in seconds.
const BREATH: f64 = 1.2;

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
}

/// The room under an icon for its dot.
fn foot(theme: &Theme) -> f64 {
    theme.space.xs + DOT
}

/// A cell's (width, height): the icon and its dot inside the cell's own
/// padding.
pub fn cell(theme: &Theme) -> (f64, f64) {
    let pad = theme.space.xs;
    (ICON + pad * 2.0, ICON + foot(theme) + pad * 2.0)
}

/// The row's (width, height) for `count` apps, with the rule or without.
pub fn size(theme: &Theme, count: usize, rule: bool) -> (f64, f64) {
    let (cw, ch) = cell(theme);
    let n = count.max(1) as f64;
    let ruled = if rule { GAP + theme.border_width } else { 0.0 };
    (n * cw + (n - 1.0) * GAP + ruled, ch)
}

pub fn row(theme: &Theme, marks: Vec<Mark>, rule: Option<usize>) -> El {
    let s = &theme.space;
    let (cw, ch) = cell(theme);
    let mut cells = Vec::with_capacity(marks.len() + 1);
    for (index, mark) in marks.into_iter().enumerate() {
        if rule == Some(index) {
            cells.push(w::separator_vertical().height(ch));
        }
        let (press, tip) = (Press { index, app: mark.app.clone() }, mark.name.clone());
        let pad = s.xs;
        cells.push(
            w::cell(El::custom(mark))
                .cell_state(|c| {
                    c.ghost = true;
                    c.inset = Some((pad, pad));
                })
                .width(Size::Px(cw))
                .height(ch)
                .interactive()
                .on(press)
                .tip(tip),
        );
    }
    w::row(GAP, cells)
}

/// When the icon started asking for attention.
struct Asking(Option<Instant>);

impl State for Asking {}

/// How high an icon stands `t` seconds into asking, as a share of its
/// side: three hops, each lower than the one before, then still until
/// the next round.
pub fn hop(t: f64) -> f64 {
    let t = t.rem_euclid(AGAIN);
    let n = (t / HOP).floor();
    if n >= HOPS {
        return 0.0;
    }
    let through = t / HOP - n;
    (std::f64::consts::PI * through).sin() * 0.3 * (1.0 - n / HOPS)
}

/// A launching icon's opacity `t` seconds in.
pub fn breath(t: f64) -> f32 {
    (0.7 + 0.3 * (std::f64::consts::TAU * t / BREATH).cos()) as f32
}

impl Widget for Mark {
    fn measure(&self, cx: &mut Cx, _avail: f64) -> (f64, f64) {
        (ICON, ICON + foot(cx.theme))
    }

    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str) {
        let t = cx.theme;
        let moving = t.motion_enabled;
        let (now, scale) = (cx.now, t.motion_scale.max(f64::EPSILON));
        let mut held = cx.take(path, || Asking(None));
        let asked = match (self.urgent || self.launching, held.0) {
            (true, None) => {
                held.0 = Some(now);
                0.0
            }
            (true, Some(since)) => now.saturating_duration_since(since).as_secs_f64() / scale,
            (false, _) => {
                held.0 = None;
                0.0
            }
        };
        cx.keep(held);
        let lift = if self.urgent && moving { hop(asked) * ICON } else { 0.0 };
        let alpha = cx.alpha * if self.launching && moving { breath(asked) } else { 1.0 };
        if (self.urgent || self.launching) && moving {
            cx.animate();
        }

        let icon = irect(Rect::new(inner.x0, inner.y0 - lift, inner.x0 + ICON, inner.y0 - lift + ICON));
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
            let dot = IRect::new((inner.x0 + (ICON - DOT) / 2.0).round() as i32, (inner.y1 - DOT).round() as i32, d, d);
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
    fn three_hops_then_still_until_the_next_round() {
        assert_eq!(hop(0.0), 0.0);
        let (first, second, third) = (hop(HOP * 0.5), hop(HOP * 1.5), hop(HOP * 2.5));
        assert!(first > second && second > third && third > 0.0, "{first} {second} {third}");
        assert_eq!(hop(HOP * 3.5), 0.0);
        assert_eq!(hop(AGAIN - 0.1), 0.0);
        assert!((hop(AGAIN + HOP * 0.5) - first).abs() < 1e-9);
    }

    #[test]
    fn a_breath_never_takes_the_icon_away() {
        for i in 0..24 {
            let a = breath(f64::from(i) * 0.1);
            assert!((0.39..=1.0).contains(&a), "{a}");
        }
    }
}
