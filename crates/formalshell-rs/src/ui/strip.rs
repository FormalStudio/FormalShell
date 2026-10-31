//! `Kind::Strip`: a framed viewport over tiles at their own places
//! (the Spaces preview's miniature), each tile a window's picture or its
//! schematic until a frame lands.

use vello_cpu::kurbo::Rect;

use super::el::Tile;
use super::{Cx, El, Hit, HitWhat, Stop, boxes, irect};
use crate::scene::IRect;

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

fn thumb(cx: &mut Cx, tile: &Tile, r: IRect, lit: bool, ring: bool, path: &str) {
    let th = super::thumb::Thumb { picture: tile.picture.0.as_ref(), icon: tile.icon.0.as_ref(), title: &tile.title, selected: tile.selected, badge: tile.badge, edge: 1.0 };
    let (theme, alpha) = (cx.theme, cx.alpha);
    let (mut p, kit) = cx.painter_kit(path);
    super::thumb::paint(&mut p, kit, theme, &th, r, lit, ring, alpha);
    let last = p.last();
    p.finish();
    cx.done(last);
}
