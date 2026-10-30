//! One role's box, in paint order: casts under
//! everything, rings, the fill with its border, the face, the inset
//! hairlines and rings, then the pointer's wash.

use std::time::Instant;

use fs_theme::color::Rgba;
use fs_theme::style::BoxStyle;
use vello_cpu::kurbo::{Affine, Rect, RoundedRect, Shape};

use crate::scene::{Cast, IRect, Paint, Scene};
use crate::surfaces::bar::cell::Painter;
use crate::surfaces::rim::Rim;

fn rr(r: Rect, radius: f64) -> vello_cpu::kurbo::BezPath {
    RoundedRect::from_rect(r, radius.max(0.0)).to_path(0.1)
}

fn frect(r: IRect) -> Rect {
    Rect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

/// `b`'s fill and border already resolved (and crossfaded) by the caller;
/// `wash` is the wash's own fade, 0 to 1.
pub fn paint(p: &mut Painter, r: IRect, b: &BoxStyle<Rgba>, radius: f64, alpha: f32, wash: f32) {
    paint_rimmed(p, r, b, radius, alpha, wash, None);
}

/// [`paint`] with the border drawn by a reactive rim where it has stops.
/// `rim` carries the transform the box's nodes are drawn under, which the
/// sweep's centre follows.
pub fn paint_rimmed(p: &mut Painter, r: IRect, b: &BoxStyle<Rgba>, radius: f64, alpha: f32, wash: f32, rim: Option<(&mut Rim, Instant, Affine)>) {
    if r.is_empty() {
        return;
    }
    let a = |c: Rgba| c.with_alpha(c.a * alpha);
    let rect = frect(r);
    if !b.casts.is_empty() {
        let layers: Vec<Cast> = b
            .casts
            .iter()
            .map(|c| Cast { x: c.x, y: c.y, blur: c.blur, spread: c.spread, color: a(c.color) })
            .collect();
        let reach = fs_theme::style::geometry::cast_pad(&b.casts);
        p.shape(
            Scene::cover(rect, vello_cpu::kurbo::Affine::IDENTITY, reach + 2.0),
            Paint::Casts { rect, radius, layers, cutout: rr(rect, radius) },
        );
    }
    for ring in &b.rings {
        let s = ring.spread.round() as i32;
        p.rect(IRect::new(r.x - s, r.y - s, r.w + s * 2, r.h + s * 2), a(ring.color), (radius + ring.spread) as f32);
    }
    let (bc, bw) = b.border.as_ref().map_or((Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width));
    let half = bw / 2.0;
    let lit = rim.filter(|_| bw > 0.0).and_then(|(rim, now, at)| {
        let mut line = rr(rect.inflate(-half, -half), radius - half);
        line.close_path();
        rim.paint(now, line, bw, rr(rect, radius), at.transform_rect_bbox(rect), alpha, None)
    });
    p.framed(r, a(b.fill), radius as f32, if lit.is_some() { Rgba::TRANSPARENT } else { a(bc) }, bw as f32);
    if let Some(lit) = lit {
        p.shape(IRect::new(r.x - 1, r.y - 1, r.w + 2, r.h + 2), lit);
    }
    let inner = fs_theme::style::geometry::inner_radius(radius, bw, r.w as f64, r.h as f64);
    let bwi = bw.round() as i32;
    let body = IRect::new(r.x + bwi, r.y + bwi, r.w - bwi * 2, r.h - bwi * 2);
    if let Some(face) = &b.face {
        p.shape(body, Paint::Face { from: a(face.from), to: a(face.to), radius: inner as f32 });
    }
    for h in b.hairlines.iter().filter(|h| h.inset) {
        let (band, _) = fs_theme::style::geometry::hairline(h.edge, h.thickness, body.w as f64, body.h as f64, inner);
        let clip = IRect::new(body.x + band.x as i32, body.y + band.y as i32, band.width.ceil() as i32, band.height.ceil() as i32);
        let half = h.thickness / 2.0;
        let line = rr(frect(body).inflate(-half, -half), (inner - half).max(0.0));
        p.shape_in(body, Paint::Shape { fill: None, strokes: vec![(line, a(h.color), h.thickness)] }, clip);
    }
    for ring in &b.inset_rings {
        let half = ring.spread / 2.0;
        let line = rr(rect.inflate(-half, -half), (radius - half).max(0.0));
        p.shape(r, Paint::Shape { fill: None, strokes: vec![(line, a(ring.color), ring.spread)] });
    }
    if let Some(w) = b.wash.filter(|_| wash > 0.0) {
        p.rect(r, w.with_alpha(w.a * alpha * wash), radius as f32);
    }
}
