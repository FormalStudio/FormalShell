//! `Kind::Strip`: a framed viewport over tiles at their own places
//! (the Spaces preview's miniature), each tile a window's picture or its
//! schematic until a frame lands.

use fs_theme::color::Rgba;
use vello_cpu::kurbo::Rect;

use super::draw::elided;
use super::el::{Font, Tile, Type, Weight};
use super::{Cx, El, Hit, HitWhat, Stop, boxes, irect};
use crate::scene::{IRect, Paint};
use crate::text::{Family, TextStyle};

/// The frame is the `cell` box; the tiles are clipped to the viewport
/// `inset` inside it, scrolled by `scroll`, and each one is a stop and a
/// click. The viewport itself takes the wheel when the element has an `on`.
pub(super) fn paint(cx: &mut Cx, el: &El, r: Rect, inset: f64, scroll: (f64, f64), tiles: &[Tile], path: &str) {
    let t = cx.theme;
    let frame = irect(r);
    let b = t.box_style("cell", Some("rest"));
    let radius = t.box_radius(&b, frame.h as f64);
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, frame, &b, radius, alpha, 0.0);
    let last = p.last();
    p.finish();
    cx.done(last);

    let view = Rect::new(r.x0 + inset, r.y0 + inset, r.x1 - inset, r.y1 - inset);
    let vi = irect(view);
    if let Some(on) = &el.on {
        cx.hit(Hit { rect: vi, path: path.into(), on: Some(on.clone()), tip: None, stop: None, what: HitWhat::Scroll });
    }
    let saved = cx.clip;
    cx.clip = Some(saved.map_or(vi, |c| c.intersect(&vi)));
    let mut order: Vec<&Tile> = tiles.iter().collect();
    order.sort_by_key(|tile| tile.raised);
    for tile in order {
        let (x, y, w, h) = tile.rect;
        let (x0, y0) = (view.x0 + x - scroll.0, view.y0 + y - scroll.1);
        let tr = irect(Rect::new(x0, y0, x0 + w, y0 + h));
        let tile_path = format!("{path}/{}", tile.key);
        let (on, ring) = cx.cursor(Some(tile.key.as_str()));
        let lit = on || cx.hover(&tile_path);
        if tr.intersects(&vi) {
            thumb(cx, tile, tr, lit, ring, &tile_path);
        } else {
            cx.painter(&tile_path).finish();
        }
        let shown = cx.clip.map_or(tr, |c| c.intersect(&tr));
        if !shown.is_empty() {
            let radius = cx.theme.cover_radius(tr.w.min(tr.h) as f64);
            cx.stop(Stop { key: tile.key.clone(), rect: shown, radius });
        }
        cx.hit(Hit { rect: tr, path: tile_path, on: Some(tile.on.clone()), tip: None, stop: Some(tile.key.clone()), what: HitWhat::Click });
    }
    cx.clip = saved;
}

/// The picture rounded and bordered, the border in `ring` while lit, with
/// the app's icon in its corner when asked; or the schematic box carrying
/// the icon and, where it fits, the title.
fn thumb(cx: &mut Cx, tile: &Tile, r: IRect, lit: bool, ring: bool, path: &str) {
    let t = cx.theme;
    let s = t.space.clone();
    let alpha = cx.alpha;
    let a = |c: Rgba| c.with_alpha(c.a * alpha);
    let cover = t.cover_radius(r.w.min(r.h) as f64);
    match &tile.picture.0 {
        Some(image) => {
            let roomy = r.w as f64 > s.control_height * 2.0 && r.h as f64 > s.control_height * 1.5;
            let badge = tile.icon.0.clone().filter(|_| tile.badge && roomy);
            let border = if lit { t.colors.get("ring") } else { t.colors.get("border") };
            let cell = t.box_style("cell", Some("rest"));
            let mut p = cx.painter(path);
            p.shape(r, Paint::Picture { image: image.clone(), alpha, radius: cover as f32 });
            p.framed(r, Rgba::TRANSPARENT, cover as f32, a(border), t.border_width as f32);
            if let Some(icon) = badge {
                let size = (s.control_height - s.sm).round() as i32;
                let b = IRect::new(r.x + s.sm as i32, r.bottom() - s.sm as i32 - size, size, size);
                boxes::paint(&mut p, b, &cell, t.box_radius(&cell, size as f64), alpha, 0.0);
                let inner = (size - (s.sm * 2.0) as i32).max(1);
                p.shape(IRect::new(b.x + (size - inner) / 2, b.y + (size - inner) / 2, inner, inner), Paint::Picture { image: icon, alpha, radius: 0.0 });
            }
            let last = p.last();
            p.finish();
            cx.done(last);
        }
        None => {
            let mut b = t.box_style("cell", Some(if tile.selected { "selected" } else { "rest" }));
            b.border = t.box_style("cell", None).border;
            let b = t.with_cursor(b, ring, false);
            let radius = t.box_radius(&b, r.h as f64);
            let size = (s.huge * 2.0).min(r.w as f64 / 2.0).min(r.h as f64 / 2.0).round().max(1.0) as i32;
            let font = Font { size: Type::Caption, weight: Weight::Normal, mono: false };
            let title = (!tile.title.is_empty()).then(|| elided(cx, &tile.title, &font, (r.w as f64 - s.md * 2.0).max(0.0)));
            let title = title.filter(|l| r.h as f64 >= size as f64 + s.xs + l.line_height() as f64 + s.md * 2.0);
            let glyph = fs_theme::icons::glyph(&cx.kit.look.icon_set, "app-window");
            let st = TextStyle { family: Family::Named(glyph.family), size: (size as f32 * 0.75).max(1.0), weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
            let mark = cx.kit.shape(glyph.text, st);
            let total = size + title.as_ref().map_or(0, |l| s.xs as i32 + l.line_height());
            let at = IRect::new(r.x + (r.w - size) / 2, r.y + (r.h - total) / 2, size, size);
            let ink = if tile.selected { t.colors.get("accentForeground") } else { t.colors.get("foreground") };
            let dim = if tile.selected { ink } else { t.colors.get("mutedForeground") };
            let mut p = cx.painter(path);
            boxes::paint(&mut p, r, &b, radius, alpha, 0.0);
            match &tile.icon.0 {
                Some(icon) => p.shape(at, Paint::Picture { image: icon.clone(), alpha, radius: 0.0 }),
                None => p.text(&mark, (at.x + (at.w - mark.width) / 2, at.y + (at.h - mark.line_height()) / 2), a(dim), &[]),
            }
            if let Some(l) = &title {
                p.text(l, (r.x + (r.w - l.width) / 2, at.bottom() + s.xs as i32), a(ink), &[]);
            }
            let last = p.last();
            p.finish();
            cx.done(last);
        }
    }
}
