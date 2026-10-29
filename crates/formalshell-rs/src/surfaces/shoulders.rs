//! The outline and its three paths, built
//! for a top line in the item's own coordinates. Every other edge is the
//! same outline under the card's edge map, which mirrors the arcs with it.

use vello_cpu::kurbo::{Arc, BezPath, Point, SvgArc, Vec2};

pub fn fillet_radius(radius: f64, depth: f64) -> f64 {
    radius.min(depth).max(0.0)
}

/// How far the item runs past the card along the line at either end.
pub fn overhang(radius: f64, wall_start: f64, wall_end: f64) -> f64 {
    radius.max(0.0).max(wall_start.max(0.0)).max(wall_end.max(0.0))
}

/// And past its far edge: one radius while a side is walled.
pub fn far_overhang(radius: f64, wall_start: f64, wall_end: f64) -> f64 {
    if wall_start >= 0.0 || wall_end >= 0.0 { radius.max(0.0) } else { 0.0 }
}

/// A side running out to where its line ends (M57 D2): how far past the
/// card each end reaches (negative for a side with room), the attach it
/// rides and the corner the two lines meet in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Walls {
    pub start: f64,
    pub end: f64,
    pub attach: f64,
    pub radius: f64,
}

impl Walls {
    pub const NONE: Walls = Walls { start: -1.0, end: -1.0, attach: 0.0, radius: 0.0 };
}

pub struct Outline {
    pub p: [Point; 8],
    /// Where the far edge's stroke stops and resumes around a child's gap.
    pub g: [Point; 2],
    /// Per corner arc in path order: radius and whether it is concave.
    pub arcs: [(f64, bool); 4],
    pub walled: [bool; 2],
}

#[allow(clippy::too_many_arguments)]
pub fn outline(width: f64, height: f64, radius: f64, inset: f64, near: f64, corner: f64, far_gap: Option<(f64, f64)>, lip: f64, walls: Walls) -> Outline {
    let r = radius.max(0.0);
    let i = inset.max(0.0);
    let n = near.max(0.0);
    let ws = if walls.start >= 0.0 { walls.start } else { -1.0 };
    let we = if walls.end >= 0.0 { walls.end } else { -1.0 };
    let at = walls.attach.clamp(0.0, 1.0);
    let over = overhang(r, ws, we);
    let lift = far_overhang(r, ws, we);
    let depth = height - lift;
    let length = (width - over * 2.0).max(0.0);
    let body = (depth - n).max(0.0);
    let l = lip.max(0.0).min(body);
    let rc = (r.min(length.min(body) / 2.0) - i).max(0.0);
    let concave = corner >= 0.0;
    let rn = if concave {
        if corner > 0.0 { fillet_radius(corner, body) + i } else { 0.0 }
    } else {
        ((-corner).min(length.min(body) / 2.0) - i).max(0.0)
    };
    let signed = if concave { rn } else { -rn };
    let wr = walls.radius.max(0.0);
    let rq = if wr > 0.0 {
        ((wr * at + r * (1.0 - at)).min(length.min(body) / 2.0) - i).max(0.0)
    } else if concave {
        0.0
    } else {
        rn
    };
    let far = depth - i;
    let top = n + i + l;
    let u_start = if ws >= 0.0 { i - ws * at + l } else { i };
    let u_end = if we >= 0.0 { length - i + we * at - l } else { length - i };
    let run_start = if ws >= 0.0 { u_start + rn } else { i + rc };
    let run_end = if we >= 0.0 { u_end - rn } else { length - i - rc };
    let (mut ga, mut gb) = (run_end, run_end);
    if let Some((a, b)) = far_gap {
        ga = run_start.max(run_end.min(a - over));
        gb = ga.max(run_end.min(b - over));
    }
    let head = if ws >= 0.0 {
        [(u_start + rq, top), (u_start, top + rq), (u_start, far + signed), (run_start, far)]
    } else {
        [(i - signed, top), (i, top + rn), (i, far - rc), (run_start, far)]
    };
    let tail = if we >= 0.0 {
        [(run_end, far), (u_end, far + signed), (u_end, top + rq), (u_end - rq, top)]
    } else {
        [(run_end, far), (length - i, far - rc), (length - i, top + rn), (length - i + signed, top)]
    };
    let map = |(u, v): (f64, f64)| Point::new(over + u, v);
    let p = [map(head[0]), map(head[1]), map(head[2]), map(head[3]), map(tail[0]), map(tail[1]), map(tail[2]), map(tail[3])];
    Outline {
        p,
        g: [map((ga, far)), map((gb, far))],
        arcs: [
            if ws >= 0.0 { (rq, false) } else { (rn, concave) },
            if ws >= 0.0 { (rn, concave) } else { (rc, false) },
            if we >= 0.0 { (rn, concave) } else { (rc, false) },
            if we >= 0.0 { (rq, false) } else { (rn, concave) },
        ],
        walled: [ws >= 0.0, we >= 0.0],
    }
}

/// PathArc from the pen to `to`: Qt's `Clockwise` is SVG's sweep flag on a
/// y-down canvas, and a top line is never mirrored.
fn arc_to(path: &mut BezPath, from: Point, to: Point, (r, concave): (f64, bool)) {
    if r <= 0.0 {
        path.line_to(to);
        return;
    }
    let svg = SvgArc { from, to, radii: Vec2::new(r, r), x_rotation: 0.0, large_arc: false, sweep: concave };
    match Arc::from_svg_arc(&svg) {
        Some(arc) => path.extend(arc.append_iter(0.05)),
        None => path.line_to(to),
    }
}

pub struct Paths {
    pub fill: BezPath,
    /// The faces toward the desktop, in two runs around a child's gap.
    pub outer: BezPath,
    /// The near edge (and a walled side), drawn at `1 - attach` of the
    /// border's alpha.
    pub near: BezPath,
}

/// A Shoulders item `width` x `height`, origin at its top-left.
pub fn paths(width: f64, height: f64, radius: f64, border: f64, attach: f64, near_inset: f64) -> Paths {
    paths_with(width, height, radius, border, attach, near_inset, Walls::NONE, None)
}

#[allow(clippy::too_many_arguments)]
pub fn paths_with(width: f64, height: f64, radius: f64, border: f64, attach: f64, near_inset: f64, walls: Walls, far_gap: Option<(f64, f64)>) -> Paths {
    let a = attach.clamp(0.0, 1.0);
    let corner = radius * (2.0 * a - 1.0);
    let walls = Walls { attach: a, ..walls };

    let f = outline(width, height, radius, 0.0, near_inset, corner, None, border * a, walls);
    let mut fill = BezPath::new();
    fill.move_to(f.p[0]);
    arc_to(&mut fill, f.p[0], f.p[1], f.arcs[0]);
    fill.line_to(f.p[2]);
    arc_to(&mut fill, f.p[2], f.p[3], f.arcs[1]);
    fill.line_to(f.p[4]);
    arc_to(&mut fill, f.p[4], f.p[5], f.arcs[2]);
    fill.line_to(f.p[6]);
    arc_to(&mut fill, f.p[6], f.p[7], f.arcs[3]);
    fill.close_path();

    let s = outline(width, height, radius, border / 2.0, near_inset, corner, far_gap, 0.0, walls);
    let [ws, we] = s.walled;
    let mut outer = BezPath::new();
    let start = if ws { s.p[2] } else { s.p[0] };
    outer.move_to(start);
    if !ws {
        arc_to(&mut outer, s.p[0], s.p[1], s.arcs[0]);
    }
    outer.line_to(s.p[2]);
    arc_to(&mut outer, s.p[2], s.p[3], s.arcs[1]);
    outer.line_to(s.g[0]);
    outer.move_to(s.g[1]);
    outer.line_to(s.p[4]);
    arc_to(&mut outer, s.p[4], s.p[5], s.arcs[2]);
    if !we {
        outer.line_to(s.p[6]);
        arc_to(&mut outer, s.p[6], s.p[7], s.arcs[3]);
    }

    let mut near = BezPath::new();
    near.move_to(if we { s.p[5] } else { s.p[7] });
    if we {
        near.line_to(s.p[6]);
        arc_to(&mut near, s.p[6], s.p[7], s.arcs[3]);
    }
    near.line_to(s.p[0]);
    if ws {
        arc_to(&mut near, s.p[0], s.p[1], s.arcs[0]);
        near.line_to(s.p[2]);
    }

    Paths { fill, outer, near }
}

/// An attached card `depth` deep hung at
/// a published join, so `debug join` has a card in the gap it opens. Drawn
/// into a full-output scene; `line` is the line's row across from `edge`.
pub fn preview(scene: &mut crate::scene::Scene, nodes: &mut Vec<crate::scene::NodeId>, theme: &fs_theme::theme::Theme, edge: fs_chrome::types::Edge, line: f64, join: Option<(f64, f64)>) {
    use fs_chrome::types::Edge;
    use vello_cpu::kurbo::{Affine, Rect};
    let mut p = crate::surfaces::bar::cell::Painter::new(scene, nodes, None);
    if let Some((x, width)) = join {
        const DEPTH: f64 = 140.0;
        let card = theme.box_style("card", None);
        let radius = theme.card_radius();
        let (color, bw) = card.border.map_or((fs_theme::color::Rgba::TRANSPARENT, 0.0), |l| (l.color, l.width));
        let paths = paths(width + radius * 2.0, DEPTH, radius, bw, 1.0, 0.0);
        let (sw, sh) = (p.scene().size.w as f64, p.scene().size.h as f64);
        let top = line - theme.border_width;
        // The top-line outline turned onto the join's edge.
        let at = match edge {
            Edge::Top => Affine::translate((x - radius, top)),
            Edge::Bottom => Affine::new([1.0, 0.0, 0.0, -1.0, x - radius, sh - top]),
            Edge::Left => Affine::new([0.0, 1.0, 1.0, 0.0, top, x - radius]),
            Edge::Right => Affine::new([0.0, 1.0, -1.0, 0.0, sw - top, x - radius]),
        };
        let bounds = crate::scene::Scene::cover(Rect::new(0.0, 0.0, width + radius * 2.0, DEPTH), at, 2.0);
        let mut fill = paths.fill;
        fill.apply_affine(at);
        let mut outer = paths.outer;
        outer.apply_affine(at);
        p.shape(bounds, crate::scene::Paint::Shape { fill: Some((fill, card.fill)), strokes: vec![(outer, color, bw)] });
    }
    p.finish();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A walled end runs the near edge out past the card to the wall while
    /// attached, and the item grows a radius on its far side for the fillet.
    #[test]
    fn a_walled_end_reaches_the_wall() {
        let walls = Walls { start: -1.0, end: 13.0, attach: 1.0, radius: 0.0 };
        let over = overhang(14.0, walls.start, walls.end);
        assert_eq!(over, 14.0);
        let o = outline(200.0 + over * 2.0, 100.0 + 14.0, 14.0, 0.0, 0.0, 14.0, None, 0.0, walls);
        assert!(o.walled[1] && !o.walled[0]);
        assert_eq!(o.p[7].x, over + 200.0 + 13.0);
        assert_eq!(o.p[0].x, over - 14.0);
    }

    #[test]
    fn a_far_gap_splits_the_far_edge() {
        let o = outline(228.0, 100.0, 14.0, 0.5, 0.0, 14.0, Some((80.0, 120.0)), 0.0, Walls::NONE);
        assert_eq!(o.g[0].x, 80.0);
        assert_eq!(o.g[1].x, 120.0);
    }
}
