//! The Spaces preview's miniature: a framed viewport over tiles at their
//! own places, each tile a window's picture or its schematic until a frame
//! lands.

use fs_theme::style::Role;
use vello_cpu::kurbo::Rect;

use fs_types::IRect;
use crate::ui::el::Pic;
use crate::ui::{Cx, Hit, HitWhat, On, Stop, Widget, boxes, irect, thumb};

/// One tile of a `Strip`, placed freely in the strip's content: a picture
/// once one has landed, a schematic box with its icon and title until then.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    /// Its cursor stop and its identity across draws.
    pub key: On,
    /// What a click on it fires.
    pub on: On,
    /// Its box in the content, before the scroll.
    pub rect: (f64, f64, f64, f64),
    pub picture: Pic,
    pub icon: Pic,
    pub title: String,
    /// The schematic's selected state (the window that has focus).
    pub selected: bool,
    /// The icon in the picture's corner, for a tile too small to name itself.
    pub badge: bool,
    /// Drawn above tiles that do not raise (a floating window).
    pub raised: bool,
}

/// A framed viewport `size` big, `inset` inside its frame, scrolled to
/// `scroll`, holding tiles at their own places. The wheel over it fires
/// `on` with `What::Scroll`.
#[derive(Clone, Debug, PartialEq)]
pub struct Strip {
    pub size: (f64, f64),
    pub inset: f64,
    pub scroll: (f64, f64),
    pub tiles: Vec<Tile>,
    pub on: On,
}

impl Widget for Strip {
    fn measure(&self, _cx: &mut Cx, _avail: f64) -> (f64, f64) {
        self.size
    }

    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str) {
        paint(cx, &self.on, inner, self.inset, self.scroll, &self.tiles, path);
    }
}

/// The frame is the `cell` box; the tiles are clipped to the viewport
/// `inset` inside it, scrolled by `scroll`, and each one is a stop and a
/// click. The viewport itself takes the wheel.
fn paint(cx: &mut Cx, on: &On, r: Rect, inset: f64, scroll: (f64, f64), tiles: &[Tile], path: &str) {
    let t = cx.theme;
    let frame = irect(r);
    let b = t.box_style(Role::Cell, Some("rest"));
    let radius = t.box_radius(&b, frame.h as f64);
    let alpha = cx.alpha;
    let mut p = cx.painter(path);
    boxes::paint(&mut p, frame, &b, radius, alpha, 0.0);
    let last = p.last();
    p.finish();
    cx.done(last);

    let view = Rect::new(r.x0 + inset, r.y0 + inset, r.x1 - inset, r.y1 - inset);
    let vi = irect(view);
    cx.hit(Hit { rect: vi, path: path.into(), on: Some(on.clone()), tip: None, stop: None, what: HitWhat::Scroll });
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
    let th = thumb::Thumb { picture: tile.picture.0.as_ref(), icon: tile.icon.0.as_ref(), title: &tile.title, selected: tile.selected, badge: tile.badge, edge: 1.0 };
    let (theme, alpha) = (cx.theme, cx.alpha);
    let (mut p, kit) = cx.painter_kit(path);
    thumb::paint(&mut p, kit, theme, &th, r, lit, ring, alpha);
    let last = p.last();
    p.finish();
    cx.done(last);
}
