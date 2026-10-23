//! One role's box, in paint order: casts under
//! everything, rings, the fill with its border, the face, the inset
//! hairlines and rings, the pointer's wash, then the cursor's leading bar
//! and its brackets. A hexagonal box draws its fill, border, wash and
//! brackets alone.

use fs_theme::color::Rgba;
use fs_theme::style::{BoxStyle, Brackets};
use vello_cpu::kurbo::{BezPath, Point, Rect, RoundedRect, Shape, Vec2};

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

/// The pointy-topped regular hexagon `r` holds, centred in it, clockwise
/// from the top vertex.
pub fn hex_points(r: Rect) -> [Point; 6] {
    let c = r.center();
    let radius = (r.height() / 2.0).min(r.width() / 3f64.sqrt()).max(0.0);
    let (dx, dy) = (radius * 3f64.sqrt() / 2.0, radius / 2.0);
    [
        Point::new(c.x, c.y - radius),
        Point::new(c.x + dx, c.y - dy),
        Point::new(c.x + dx, c.y + dy),
        Point::new(c.x, c.y + radius),
        Point::new(c.x - dx, c.y + dy),
        Point::new(c.x - dx, c.y - dy),
    ]
}

/// The rect that hexagon spans.
pub fn hex_bounds(r: Rect) -> Rect {
    let p = hex_points(r);
    Rect::new(p[5].x, p[0].y, p[1].x, p[3].y)
}

pub fn hexagon(r: Rect) -> BezPath {
    let p = hex_points(r);
    let mut path = BezPath::new();
    path.move_to(p[0]);
    for q in &p[1..] {
        path.line_to(*q);
    }
    path.close_path();
    path
}

/// A box's outline as a polygon, `out` pixels outside it: its hexagon, its
/// chamfered rect or its rect (corners square: a bracket reads as a line).
fn outline(rect: Rect, b: &BoxStyle<Rgba>, out: f64) -> Vec<Point> {
    if b.hex {
        let p = hex_points(rect);
        let c = rect.center();
        // Each vertex pushed out along its own bisector so every edge
        // moves `out` along its normal.
        let k = out / (std::f64::consts::PI / 6.0).cos();
        return p.iter().map(|q| { let v = (*q - c).normalize(); *q + v * k }).collect();
    }
    let r = rect.inflate(out, out);
    let most = (r.width().min(r.height()) / 2.0).max(0.0);
    let cut = |c: f64| if c > 0.0 { (c + out * (2.0 - std::f64::consts::SQRT_2)).clamp(0.0, most) } else { 0.0 };
    let [tl, tr, br, bl] = b.chamfer.map(cut);
    vec![
        Point::new(r.x0 + tl, r.y0),
        Point::new(r.x1 - tr, r.y0),
        Point::new(r.x1, r.y0 + tr),
        Point::new(r.x1, r.y1 - br),
        Point::new(r.x1 - br, r.y1),
        Point::new(r.x0 + bl, r.y1),
        Point::new(r.x0, r.y1 - bl),
        Point::new(r.x0, r.y0 + tl),
    ]
}

/// The stretches of `poly` within `sweep` degrees of straight up and of
/// straight down from `centre`, the angle read with the box squared up to
/// `half` so the sectors keep their shape on a wide box.
fn sectors(poly: &[Point], centre: Point, half: Vec2, sweep: f64) -> BezPath {
    let limit = (sweep / 2.0).to_radians();
    let inside = |q: Point| {
        let (x, y) = ((q.x - centre.x) / half.x.max(1e-6), (q.y - centre.y) / half.y.max(1e-6));
        // Off the vertical axis, in radians, either way up.
        x.abs().atan2(y.abs()) <= limit
    };
    let mut path = BezPath::new();
    let mut drawing = false;
    let n = poly.len();
    // Starting on a vertex outside every sector, so no run is split at the seam.
    let start = (0..n).find(|i| !inside(poly[*i])).unwrap_or(0);
    for k in 0..n {
        let (a, b) = (poly[(start + k) % n], poly[(start + k + 1) % n]);
        let steps = ((b - a).hypot() / 0.5).ceil().max(1.0) as usize;
        for j in 0..=steps {
            let q = a.lerp(b, j as f64 / steps as f64);
            match (inside(q), drawing) {
                (true, false) => {
                    path.move_to(q);
                    drawing = true;
                }
                (true, true) => path.line_to(q),
                (false, true) => drawing = false,
                (false, false) => {}
            }
        }
    }
    path
}

/// The brackets along `b`'s outline, `gap` out from it, each stroke under
/// wider fainter copies of itself for the role's glow.
fn brackets(rect: Rect, b: &BoxStyle<Rgba>, br: &Brackets<Rgba>, alpha: f32) -> (IRect, Paint) {
    let half_line = br.line.width / 2.0;
    let out = br.gap + half_line;
    let bounds = if b.hex { hex_bounds(rect) } else { rect };
    let poly = outline(rect, b, out);
    let centre = bounds.center();
    let half = Vec2::new(bounds.width() / 2.0 + out, bounds.height() / 2.0 + out);
    let path = sectors(&poly, centre, half, br.sweep);
    let mut strokes = Vec::new();
    let mut reach = br.line.width;
    for g in &br.glow {
        for (width, share) in [(g.blur * 1.5, 0.15), (g.blur * 0.75, 0.3)] {
            let c = g.color;
            strokes.push((path.clone(), c.with_alpha(c.a * share * alpha), br.line.width + width));
            reach = reach.max(br.line.width + width);
        }
    }
    strokes.push((path, br.line.color.with_alpha(br.line.color.a * alpha), br.line.width));
    let r = bounds.inflate(out + reach, out + reach);
    (IRect::new(r.x0.floor() as i32, r.y0.floor() as i32, r.width().ceil() as i32 + 2, r.height().ceil() as i32 + 2), Paint::Shape { fill: None, strokes })
}

/// `b`'s fill and border already resolved (and crossfaded) by the caller;
/// `wash` is the wash's own fade, 0 to 1.
pub fn paint(p: &mut Painter, r: IRect, b: &BoxStyle<Rgba>, radius: f64, alpha: f32, wash: f32) {
    if r.is_empty() {
        return;
    }
    let a = |c: Rgba| c.with_alpha(c.a * alpha);
    let rect = frect(r);
    if b.hex {
        let (bc, bw) = b.border.as_ref().map_or((Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width));
        let shape = hexagon(rect);
        let strokes = if bw > 0.0 && bc.a > 0.0 { vec![(hexagon(hex_bounds(rect).inflate(-bw / 2.0, -bw / 2.0)), a(bc), bw)] } else { Vec::new() };
        p.shape(r, Paint::Shape { fill: (b.fill.a > 0.0).then(|| (shape.clone(), a(b.fill))), strokes });
        if let Some(w) = b.wash.filter(|_| wash > 0.0) {
            p.shape(r, Paint::Shape { fill: Some((shape, w.with_alpha(w.a * alpha * wash))), strokes: Vec::new() });
        }
        if let Some(br) = &b.brackets {
            let (bounds, shape) = brackets(rect, b, br, alpha);
            p.shape(bounds, shape);
        }
        return;
    }
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
        let (bounds, shape) = brackets(rect, b, br, alpha);
        p.shape(bounds, shape);
    }
}
