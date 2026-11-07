//! Where the screen frame paints. Pure, so the band and its corners are
//! checkable without a compositor.
//!
//! The frame is the whole output (`outer`) with a rounded rectangle cut out of
//! it (`inner`): `insets` in from every edge, which is the bar's own
//! thickness on its edge and the band's `thickness` on the other three. The
//! ring between the two is one fill, strip included: the bar draws only its
//! cells over it, so the strip and the band are one blurred surface with no
//! seam where one would hand over to the other. `radius` is capped so the
//! cut-out is never asked for corners it cannot hold.

use fs_types::{Insets, Rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameGeometry {
    pub outer: Rect,
    pub inner: Rect,
    pub radius: f64,
}

pub fn frame_geometry(width: f64, height: f64, insets: Insets, radius: f64) -> FrameGeometry {
    let outer = Rect::new(0.0, 0.0, width.max(0.0), height.max(0.0));
    let inner = Rect::new(
        insets.left,
        insets.top,
        (width - insets.left - insets.right).max(0.0),
        (height - insets.top - insets.bottom).max(0.0),
    );
    let r = (if radius > 0.0 { radius } else { 0.0 })
        .min(inner.width.min(inner.height) / 2.0)
        .max(0.0);
    FrameGeometry {
        outer,
        inner,
        radius: r,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
}

/// The cut-out shrunk by half a stroke, so a line drawn along it lies inside
/// the band rather than straddling its edge.
pub fn stroke_rect(inner: Rect, radius: f64, stroke_width: f64) -> StrokeRect {
    let half = stroke_width / 2.0;
    StrokeRect {
        x: inner.x + half,
        y: inner.y + half,
        width: (inner.width - stroke_width).max(0.0),
        height: (inner.height - stroke_width).max(0.0),
        radius: (radius - half).max(0.0),
    }
}

/// The gap a joined card's shoulders draw into one side, `(start, end)` in
/// window coordinates along that side.
pub type Gap = (f64, f64);

/// The gap on each side that has a card on it; absent for a side with nothing
/// on it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gaps {
    pub top: Option<Gap>,
    pub right: Option<Gap>,
    pub bottom: Option<Gap>,
    pub left: Option<Gap>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Corner {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    pub gone: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sides {
    pub top: [Segment; 2],
    pub right: [Segment; 2],
    pub bottom: [Segment; 2],
    pub left: [Segment; 2],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingLine {
    pub sides: Sides,
    /// Clockwise from the top right.
    pub corners: [Corner; 4],
}

/// The ring's hairline as twelve strokes: each side of the cut-out in two
/// runs, split at the gap a joined card's shoulders draw into on that side,
/// and the four corner arcs, clockwise. Sides are cut only on their straight
/// run; a gap outside it or empty gives a second run of nothing and a first
/// that is the whole side. Strokes are separate so any side can open on any
/// frame, which one walk cannot do once two cards are up on two sides at
/// once.
///
/// A corner is `gone` when the gaps on both of its sides run into it: a card
/// that comes out of one of those lines and runs out to the other covers the
/// arc between them with its own silhouette, and an arc drawn there would
/// read as a line across the card's fill. Both sides, so a card that merely
/// rests near the end of one line keeps the corner it never reaches.
pub fn ring_line(inner: Rect, radius: f64, gaps: Gaps) -> RingLine {
    let (x, y, w, h, r) = (inner.x, inner.y, inner.width, inner.height, radius);

    // The two runs of one side, as offsets along it.
    let runs = |gap: Option<Gap>, origin: f64, extent: f64| -> [(f64, f64); 2] {
        let lo = gap.map_or(extent - r, |g| r.max((extent - r).min(g.0 - origin)));
        let hi = gap.map_or(extent - r, |g| lo.max((extent - r).min(g.1 - origin)));
        [(r, lo), (hi, extent - r)]
    };
    // Whether the gap on one side covers the whole corner at one of its ends,
    // which is the last `radius` of that side. The whole of it, not just its
    // start: a card still coming out from under the line has not reached the
    // arc yet, and the frame keeps a corner until something is there to draw
    // that stretch of the cut-out's edge instead.
    let reaches = |gap: Option<Gap>, origin: f64, extent: f64, far: bool| -> bool {
        let Some(g) = gap else { return false };
        if !(g.1 > g.0) {
            return false;
        }
        if far {
            g.0 - origin <= extent - r && g.1 - origin >= extent
        } else {
            g.0 - origin <= 0.0 && g.1 - origin >= r
        }
    };

    let across_x = |s: [(f64, f64); 2], at: f64| {
        s.map(|(a, b)| Segment {
            x1: x + a,
            y1: at,
            x2: x + b,
            y2: at,
        })
    };
    let across_y = |s: [(f64, f64); 2], at: f64| {
        s.map(|(a, b)| Segment {
            x1: at,
            y1: y + a,
            x2: at,
            y2: y + b,
        })
    };

    RingLine {
        sides: Sides {
            top: across_x(runs(gaps.top, x, w), y),
            right: across_y(runs(gaps.right, y, h), x + w),
            bottom: across_x(runs(gaps.bottom, x, w), y + h),
            left: across_y(runs(gaps.left, y, h), x),
        },
        corners: [
            Corner {
                x1: x + w - r,
                y1: y,
                x2: x + w,
                y2: y + r,
                gone: reaches(gaps.top, x, w, true) && reaches(gaps.right, y, h, false),
            },
            Corner {
                x1: x + w,
                y1: y + h - r,
                x2: x + w - r,
                y2: y + h,
                gone: reaches(gaps.right, y, h, true) && reaches(gaps.bottom, x, w, true),
            },
            Corner {
                x1: x + r,
                y1: y + h,
                x2: x,
                y2: y + h - r,
                gone: reaches(gaps.bottom, x, w, false) && reaches(gaps.left, y, h, true),
            },
            Corner {
                x1: x,
                y1: y + r,
                x2: x + r,
                y2: y,
                gone: reaches(gaps.left, y, h, false) && reaches(gaps.top, x, w, false),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A 1920x1080 output with a 40px bar, a 10px band and a 20px corner.

    fn inset(position: &str) -> Insets {
        let on = |p: &str| if position == p { 40.0 } else { 0.0 };
        Insets {
            top: on("top"),
            bottom: on("bottom"),
            left: on("left"),
            right: on("right"),
        }
    }

    fn edge(position: &str, thickness: f64) -> Insets {
        let i = inset(position);
        let pick = |v: f64| if v > 0.0 { v } else { thickness };
        Insets {
            top: pick(i.top),
            bottom: pick(i.bottom),
            left: pick(i.left),
            right: pick(i.right),
        }
    }

    fn frame(position: &str, thickness: f64, radius: f64) -> FrameGeometry {
        frame_geometry(1920.0, 1080.0, edge(position, thickness), radius)
    }

    // The ring is the whole output: the strip is painted here, not by the
    // bar, so the two never meet at a seam.
    #[test]
    fn the_outer_rectangle_is_the_whole_output() {
        assert_eq!(
            frame("top", 10.0, 20.0).outer,
            Rect::new(0.0, 0.0, 1920.0, 1080.0)
        );
    }

    // The cut-out is the band in from every edge but the bar's, where it is
    // the bar's own thickness in.
    #[test]
    fn the_cut_out_clears_the_bar_and_the_band() {
        let g = frame("top", 10.0, 20.0);
        assert_eq!(g.inner, Rect::new(10.0, 40.0, 1900.0, 1030.0));
        let l = frame("left", 10.0, 20.0);
        assert_eq!(l.inner, Rect::new(40.0, 10.0, 1870.0, 1060.0));
        assert_eq!(g.radius, 20.0);
    }

    #[test]
    fn a_radius_too_big_for_the_cut_out_is_capped() {
        assert_eq!(
            frame_geometry(100.0, 60.0, edge("top", 10.0), 500.0).radius,
            5.0
        );
        assert_eq!(frame("top", 10.0, -4.0).radius, 0.0);
    }

    #[test]
    fn no_band_leaves_only_the_bar_cut_in() {
        let g = frame("top", 0.0, 20.0);
        assert_eq!(g.inner, Rect::new(0.0, 40.0, 1920.0, 1040.0));
    }

    // The hairline sits half a stroke inside the cut-out, so it lies in the
    // band rather than straddling its edge.
    #[test]
    fn the_stroke_rect_is_half_a_stroke_inside() {
        let g = frame("top", 10.0, 20.0);
        let s = stroke_rect(g.inner, g.radius, 1.0);
        assert_eq!(
            s,
            StrokeRect {
                x: 10.5,
                y: 40.5,
                width: 1899.0,
                height: 1029.0,
                radius: 19.5
            }
        );
    }

    // The hairline's strokes (the joined card on a framed screen)

    fn line(position: &str, gaps: Gaps) -> RingLine {
        let g = frame(position, 10.0, 20.0);
        ring_line(g.inner, g.radius, gaps)
    }

    fn seg(x1: f64, y1: f64, x2: f64, y2: f64) -> Segment {
        Segment { x1, y1, x2, y2 }
    }

    #[test]
    fn no_gap_leaves_every_side_one_whole_run() {
        let l = line("top", Gaps::default());
        assert_eq!(l.sides.top[0], seg(30.0, 40.0, 1890.0, 40.0));
        assert_eq!(l.sides.top[1].x1, l.sides.top[1].x2);
        assert_eq!(l.sides.left[0], seg(10.0, 60.0, 10.0, 1050.0));
        assert_eq!(l.corners.len(), 4);
        assert_eq!(
            l.corners[3],
            Corner {
                x1: 10.0,
                y1: 60.0,
                x2: 30.0,
                y2: 40.0,
                gone: false
            }
        );
    }

    // The corner a walled card covers. A card out of a left bar's line that
    // runs out to the ring's bottom line draws that corner's own stretch of
    // the cut-out itself, so the ring gives the arc up: the card's gap covers
    // the last radius of the left line and its wall's gap the first radius of
    // the bottom one.
    #[test]
    fn a_corner_both_gaps_cover_is_given_up() {
        let l = line(
            "left",
            Gaps {
                left: Some((539.0, 1085.0)),
                bottom: Some((26.0, 426.0)),
                ..Gaps::default()
            },
        );
        assert!(l.corners[2].gone);
        assert!(!l.corners[0].gone);
        assert!(!l.corners[1].gone);
        assert!(!l.corners[3].gone);
    }

    // One side alone is not enough: a card resting near the end of a line
    // without running out to the other one never reaches the arc.
    #[test]
    fn one_gap_alone_keeps_the_corner() {
        let l = line(
            "left",
            Gaps {
                left: Some((539.0, 1085.0)),
                ..Gaps::default()
            },
        );
        assert!(!l.corners[2].gone);
        let m = line(
            "left",
            Gaps {
                bottom: Some((26.0, 426.0)),
                ..Gaps::default()
            },
        );
        assert!(!m.corners[2].gone);
    }

    // And a shape still coming out from under the line covers only the first
    // pixels of the wall, not the arc: the corner stands until it does.
    #[test]
    fn a_shallow_run_out_keeps_the_corner() {
        let l = line(
            "left",
            Gaps {
                left: Some((539.0, 1085.0)),
                bottom: Some((26.0, 45.0)),
                ..Gaps::default()
            },
        );
        assert!(!l.corners[2].gone);
    }

    #[test]
    fn a_gap_on_a_left_bar_splits_its_side_at_the_gap() {
        let l = line(
            "left",
            Gaps {
                left: Some((300.0, 500.0)),
                ..Gaps::default()
            },
        );
        assert_eq!(l.sides.left[0], seg(40.0, 30.0, 40.0, 300.0));
        assert_eq!(l.sides.left[1], seg(40.0, 500.0, 40.0, 1050.0));
        assert_eq!(l.sides.right[1].y1, l.sides.right[1].y2);
    }

    #[test]
    fn two_sides_can_open_at_once() {
        let l = line(
            "left",
            Gaps {
                left: Some((300.0, 500.0)),
                right: Some((700.0, 900.0)),
                ..Gaps::default()
            },
        );
        assert_eq!(l.sides.left[1].y1, 500.0);
        assert_eq!(l.sides.right[0].y2, 700.0);
        assert_eq!(l.sides.right[1].y1, 900.0);
    }

    #[test]
    fn the_gap_is_held_to_the_straight_run() {
        let l = line(
            "top",
            Gaps {
                top: Some((0.0, 5.0)),
                ..Gaps::default()
            },
        );
        assert_eq!(l.sides.top[0].x2, 30.0);
        assert_eq!(l.sides.top[1].x1, 30.0);
        let m = line(
            "right",
            Gaps {
                right: Some((1070.0, 1090.0)),
                ..Gaps::default()
            },
        );
        assert_eq!(m.sides.right[0].y2, 1050.0);
        assert_eq!(m.sides.right[1].y1, 1050.0);
    }
}
