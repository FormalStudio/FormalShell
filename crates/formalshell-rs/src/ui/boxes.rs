//! One role's box, in paint order: casts under
//! everything, rings, the fill with its border, the face, the inset
//! hairlines and rings, the pointer's wash, then the cursor's leading bar
//! and its brackets.

use fs_theme::color::Rgba;
use fs_theme::style::{BoxStyle, Brackets};
use vello_cpu::kurbo::{Arc, BezPath, Point, Rect, RoundedRect, Shape, Vec2};

use crate::scene::{Cast, IRect, Paint, Scene};
use crate::surfaces::bar::cell::Painter;

fn rr(r: Rect, radius: f64) -> vello_cpu::kurbo::BezPath {
    RoundedRect::from_rect(r, radius.max(0.0)).to_path(0.1)
}

fn frect(r: IRect) -> Rect {
    Rect::new(r.x as f64, r.y as f64, r.right() as f64, r.bottom() as f64)
}

/// `r` with each corner cut straight across, clockwise from the top left,
/// every cut held to half the shorter side.
pub fn chamfered(r: Rect, cuts: [f64; 4]) -> BezPath {
    let most = (r.width().min(r.height()) / 2.0).max(0.0);
    let [tl, tr, br, bl] = cuts.map(|c| c.clamp(0.0, most));
    let mut path = BezPath::new();
    path.move_to((r.x0 + tl, r.y0));
    path.line_to((r.x1 - tr, r.y0));
    path.line_to((r.x1, r.y0 + tr));
    path.line_to((r.x1, r.y1 - br));
    path.line_to((r.x1 - br, r.y1));
    path.line_to((r.x0 + bl, r.y1));
    path.line_to((r.x0, r.y1 - bl));
    path.line_to((r.x0, r.y0 + tl));
    path.close_path();
    path
}

/// The fill and border of a chamfered box: the stroke runs half a line in,
/// its cuts shortened by what that inset takes off a 45 degree edge.
pub fn chamfer_paint(rect: Rect, cuts: [f64; 4], fill: Rgba, border: Rgba, width: f64) -> Paint {
    let half = width / 2.0;
    let inset = cuts.map(|c| if c > 0.0 { (c - half * (2.0 - std::f64::consts::SQRT_2)).max(0.0) } else { 0.0 });
    let strokes = if width > 0.0 && border.a > 0.0 { vec![(chamfered(rect.inflate(-half, -half), inset), border, width)] } else { Vec::new() };
    Paint::Shape { fill: (fill.a > 0.0).then(|| (chamfered(rect, cuts), fill)), strokes }
}

/// Two arcs either side of `rect`, `gap` out from it, on the ellipse whose
/// axes meet those points.
fn brackets(rect: Rect, b: &Brackets<Rgba>, alpha: f32) -> (IRect, Paint) {
    let half = b.line.width / 2.0;
    let radii = Vec2::new(rect.width() / 2.0 + b.gap + half, rect.height() / 2.0 + b.gap + half);
    let centre = rect.center();
    let sweep = b.sweep.to_radians();
    let mut strokes = Vec::new();
    for mid in [std::f64::consts::PI, 0.0] {
        let arc = Arc { center: centre, radii, start_angle: mid - sweep / 2.0, sweep_angle: sweep, x_rotation: 0.0 };
        strokes.push((arc.to_path(0.1), b.line.color.with_alpha(b.line.color.a * alpha), b.line.width));
    }
    let reach = (radii.x + b.line.width).ceil();
    let reach_y = (radii.y + b.line.width).ceil();
    let at = Point::new(centre.x - reach, centre.y - reach_y);
    (IRect::new(at.x as i32, at.y as i32, (reach * 2.0) as i32 + 1, (reach_y * 2.0) as i32 + 1), Paint::Shape { fill: None, strokes })
}

/// `b`'s fill and border already resolved (and crossfaded) by the caller;
/// `wash` is the wash's own fade, 0 to 1.
pub fn paint(p: &mut Painter, r: IRect, b: &BoxStyle<Rgba>, radius: f64, alpha: f32, wash: f32) {
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
    if b.chamfered() {
        p.shape(r, chamfer_paint(rect, b.chamfer, a(b.fill), a(bc), bw));
    } else {
        p.framed(r, a(b.fill), radius as f32, a(bc), bw as f32);
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
    if let Some(bar) = &b.bar {
        p.rect(IRect::new(r.x, r.y, (bar.width.round() as i32).min(r.w), r.h), a(bar.color), 0.0);
    }
    if let Some(br) = &b.brackets {
        let (bounds, shape) = brackets(rect, br, alpha);
        p.shape(bounds, shape);
    }
}
