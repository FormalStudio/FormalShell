//! A window's thumbnail: its captured frame, or its schematic until one
//! lands. The Spaces preview's strip, the switcher and the overview all
//! draw windows through this.

use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use fs_theme::tokens::WEIGHTS;

use super::boxes;
use crate::scene::{Bitmap, IRect, Paint};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{Family, TextStyle};

pub struct Thumb<'a> {
    pub picture: Option<&'a Bitmap>,
    pub icon: Option<&'a Bitmap>,
    /// Written under the schematic's icon where it fits.
    pub title: &'a str,
    /// The schematic's selected state (the window that has focus).
    pub selected: bool,
    /// The icon in the picture's corner, for a tile too small to name itself.
    pub badge: bool,
    /// The share of the picture's rounding and border drawn: the overview
    /// starts its zoom at 0, where the frame sits on the live window.
    pub edge: f32,
}

/// `lit` puts the picture's border in `ring`; `ring` is the schematic's
/// keyboard cursor.
#[allow(clippy::too_many_arguments)]
pub fn paint(p: &mut Painter, kit: &mut Kit, t: &Theme, th: &Thumb, r: IRect, lit: bool, ring: bool, alpha: f32) {
    let s = &t.space;
    let a = |c: Rgba, k: f32| c.with_alpha(c.a * alpha * k);
    match th.picture {
        Some(image) => {
            let cover = t.cover_radius(r.w.min(r.h) as f64) as f32 * th.edge;
            let border = if lit { t.colors.get("ring") } else { t.colors.get("border") };
            p.shape(r, Paint::Picture { image: image.clone(), alpha, radius: cover });
            p.framed(r, Rgba::TRANSPARENT, cover, a(border, th.edge), t.border_width as f32);
            let roomy = r.w as f64 > s.control_height * 2.0 && r.h as f64 > s.control_height * 1.5;
            if let Some(icon) = th.icon.filter(|_| th.badge && roomy) {
                let cell = t.box_style("cell", Some("rest"));
                let size = (s.control_height - s.sm).round() as i32;
                let b = IRect::new(r.x + s.sm as i32, r.bottom() - s.sm as i32 - size, size, size);
                boxes::paint(p, b, &cell, t.box_radius(&cell, size as f64), alpha, 0.0);
                let inner = (size - (s.sm * 2.0) as i32).max(1);
                p.shape(IRect::new(b.x + (size - inner) / 2, b.y + (size - inner) / 2, inner, inner), Paint::Picture { image: icon.clone(), alpha, radius: 0.0 });
            }
        }
        None => {
            let mut b = t.box_style("cell", Some(if th.selected { "selected" } else { "rest" }));
            b.border = t.box_style("cell", None).border;
            let b = t.with_cursor(b, ring, false);
            let radius = t.box_radius(&b, r.h as f64);
            let size = (s.xl * 2.0).min(r.w as f64 / 2.0).min(r.h as f64 / 2.0).round().max(1.0) as i32;
            let caption = TextStyle { family: kit.look.sans, size: t.font_size.caption as f32, weight: WEIGHTS.normal as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
            let title = (!th.title.is_empty()).then(|| kit.elided(th.title, caption, (r.w as f64 - s.sm * 2.0).max(0.0)));
            let title = title.filter(|l| r.h as f64 >= size as f64 + s.xs + l.line_height() as f64 + s.sm * 2.0);
            let total = size + title.as_ref().map_or(0, |l| s.xs as i32 + l.line_height());
            let at = IRect::new(r.x + (r.w - size) / 2, r.y + (r.h - total) / 2, size, size);
            let ink = if th.selected { t.colors.get("accentForeground") } else { t.colors.get("foreground") };
            let dim = if th.selected { ink } else { t.colors.get("mutedForeground") };
            boxes::paint(p, r, &b, radius, alpha, 0.0);
            match th.icon {
                Some(icon) => p.shape(at, Paint::Picture { image: icon.clone(), alpha, radius: 0.0 }),
                None => {
                    let glyph = fs_theme::icons::glyph("app-window");
                    let st = TextStyle { family: Family::Named(glyph.family), size: (size as f32 * 0.75).max(1.0), weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
                    let mark = kit.shape(glyph.text, st);
                    p.text(&mark, (at.x + (at.w - mark.width) / 2, at.y + (at.h - mark.line_height()) / 2), a(dim, 1.0), &[]);
                }
            }
            if let Some(l) = &title {
                p.text(l, (r.x + (r.w - l.width) / 2, at.bottom() + s.xs as i32), a(ink, 1.0), &[]);
            }
        }
    }
}
