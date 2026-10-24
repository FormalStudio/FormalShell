//! One frame of the boot (`crate::boot`) drawn into a scene: the black,
//! the grid's lines as far as they have grown, and the emblem, its two
//! rings and the stacked wordmark, all in the table's `boot` role. The
//! greeter draws it under its card, the session over its desktop.

use std::sync::Arc;

use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::{Affine, BezPath, Shape};

use crate::boot::{self, Plan, RINGS, ROW_LEADING, ROW_SIZES, STAGE, WORD_ROOM};
use crate::scene::{Brush, IRect, Paint, VOp};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{PAD, TextStyle};

/// The wordmark shaped once: every glyph's outline with its place about
/// the emblem's centre, row by row, in stage pixels.
pub struct Emblem {
    rows: Vec<Vec<(Arc<BezPath>, f64, f64)>>,
}

impl Emblem {
    /// `words` stacked, the first the larger, each set wide and black in
    /// the table's display face and narrowed to fit the inner ring.
    pub fn new(kit: &mut Kit, theme: &Theme, words: [&str; 2]) -> Self {
        let family = crate::surfaces::bar::cell::face(theme.faces.display);
        let style = |size: f64| TextStyle { family, size: size as f32, weight: 900.0, tracking: 0.0, wdth: 125.0, wght: Some(900.0) };
        let mut shaped = Vec::new();
        for (word, size) in words.iter().zip(ROW_SIZES) {
            let mut s = kit.shape(word, style(size));
            let mut size = size;
            if f64::from(s.width) > WORD_ROOM {
                size = (size * WORD_ROOM / f64::from(s.width)).floor();
                s = kit.shape(word, style(size));
            }
            shaped.push((s, size));
        }
        let total: f64 = shaped.iter().map(|(_, size)| size * ROW_LEADING).sum();
        let mut top = -total / 2.0;
        let pad = f64::from(PAD);
        let rows = shaped
            .iter()
            .map(|(s, size)| {
                let centre = top + size * ROW_LEADING / 2.0;
                top += size * ROW_LEADING;
                let (cx, cy) = (f64::from(s.width) / 2.0, f64::from(s.line_height()) / 2.0);
                s.glyphs.iter().filter(|g| g.image.is_none()).map(|g| (g.path.clone(), f64::from(g.x) - pad - cx, f64::from(g.y) - pad - cy + centre)).collect()
            })
            .collect();
        Self { rows }
    }

    /// No wordmark at all: the greeter's handoff draws none.
    pub fn empty() -> Self {
        Self { rows: Vec::new() }
    }

    /// How many letters each row carries, for the plan's dice.
    pub fn counts(&self) -> Vec<usize> {
        self.rows.iter().map(Vec::len).collect()
    }
}

/// Where the boot stands in one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// ms into the way in, held where it was once the way out starts.
    pub t: f64,
    /// ms into the crumple, once it starts.
    pub out: Option<f64>,
    /// The emblem's centre on screen and its size against the stage's.
    pub at: (f64, f64),
    pub scale: f64,
    /// The black under everything, the lines and the emblem, 0 to 1.
    pub black: f32,
    pub lines: f32,
    pub emblem: f32,
}

/// How big the stage is on a `size` screen.
pub fn stage_scale(size: (i32, i32)) -> f64 {
    (f64::from(size.0) / STAGE.0).min(f64::from(size.1) / STAGE.1)
}

/// The grid the boot draws its lines along: the desktop's own, so they
/// land where the wallpaper's lines will be. Each as `(x0, y0, x1, y1)`.
pub fn grid_lines(theme: &Theme, size: (i32, i32)) -> Vec<(f64, f64, f64, f64)> {
    let g = crate::services::wallpaper::grid(theme, 1.0);
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    g.lines(w, h).into_iter().map(|l| (l.x0, l.y0, l.x1, l.y1)).collect()
}

/// Which of `lines` are columns.
pub fn columns(lines: &[(f64, f64, f64, f64)], size: (i32, i32)) -> Vec<bool> {
    lines.iter().map(|l| l.2 - l.0 < f64::from(size.0)).collect()
}

pub fn paint(p: &mut Painter, theme: &Theme, size: (i32, i32), lines: &[(f64, f64, f64, f64)], plan: &Plan, emblem: &Emblem, f: &Frame) {
    let role = theme.box_style("boot", None);
    let ink = role.fill;
    let line_ink = role.border.as_ref().map_or(ink.with_alpha(0.22), |b| b.color);
    let full = IRect::new(0, 0, size.0, size.1);
    if f.black > 0.0 {
        p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: f.black }, 0.0);
    }
    if f.lines > 0.0 {
        let ink = line_ink.with_alpha(line_ink.a * f.lines);
        for (i, &(x0, y0, x1, y1)) in lines.iter().enumerate() {
            let g = plan.grown(i, f.t);
            if g <= 0.0 {
                continue;
            }
            let column = x1 - x0 < f64::from(size.0);
            let from_end = plan.lines.get(i).is_some_and(|l| l.from_end);
            let (x0, y0, x1, y1) = match (column, from_end) {
                (true, false) => (x0, y0, x1, y0 + (y1 - y0) * g),
                (true, true) => (x0, y1 - (y1 - y0) * g, x1, y1),
                _ => (x0, y0, x0 + (x1 - x0) * g, y1),
            };
            let (ix, iy) = (x0.round() as i32, y0.round() as i32);
            p.rect(IRect::new(ix, iy, (x1.round() as i32 - ix).max(1), (y1.round() as i32 - iy).max(1)), ink, 0.0);
        }
    }
    if f.emblem <= 0.0 {
        return;
    }
    let crumple = f.out.map_or(boot::crumple(0.0), boot::crumple);
    let alpha = f.emblem * crumple.alpha;
    if alpha <= 0.0 {
        return;
    }
    let k = stage_scale(size) * f.scale;
    // The stage's emblem onto the screen: scaled about its centre, the
    // crumple's scale over its skew (`scale() skewX()`), then placed.
    let place = Affine::translate(f.at) * Affine::scale_non_uniform(crumple.sx, crumple.sy) * Affine::skew(crumple.skew, 0.0) * Affine::scale(k);
    let mut ops = Vec::new();
    for r in &RINGS {
        let pose = boot::ring(r, f.t);
        if pose.alpha <= 0.0 {
            continue;
        }
        let (sin, cos) = pose.spin.sin_cos();
        let mut path = BezPath::new();
        const N: usize = 96;
        for i in 0..=N {
            let a = std::f64::consts::TAU * i as f64 / N as f64;
            let (x, y) = (r.rx * a.cos(), r.ry * a.sin());
            let pt = place * vello_cpu::kurbo::Point::new(x * cos - y * sin, (x * sin + y * cos) * pose.squash);
            if i == 0 { path.move_to(pt) } else { path.line_to(pt) }
        }
        path.close_path();
        let a = alpha * pose.alpha;
        ops.push(VOp::Stroke(path, Brush::Solid(ink.with_alpha(ink.a * a)), r.stroke * k));
    }
    for (row, glyphs) in emblem.rows.iter().enumerate() {
        for (i, (outline, x, y)) in glyphs.iter().enumerate() {
            let (a, dx, dy) = plan.letter(row, i, f.t);
            if a <= 0.0 || outline.elements().is_empty() {
                continue;
            }
            let mut path = (**outline).clone();
            path.apply_affine(place * Affine::translate((x + dx, y + dy)));
            ops.push(VOp::Fill(path, Brush::Solid(ink.with_alpha(ink.a * alpha * a))));
        }
    }
    if ops.is_empty() {
        return;
    }
    let bounds = ops.iter().fold(None::<vello_cpu::kurbo::Rect>, |u, op| {
        let b = match op {
            VOp::Fill(path, _) | VOp::Stroke(path, _, _) => path.bounding_box(),
            _ => return u,
        };
        Some(u.map_or(b, |u| u.union(b)))
    });
    let Some(b) = bounds else { return };
    let reach = (RINGS[0].stroke * k).ceil() + 2.0;
    let (x0, y0) = ((b.x0 - reach).floor() as i32, (b.y0 - reach).floor() as i32);
    let bounds = IRect::new(x0, y0, (b.x1 + reach).ceil() as i32 - x0, (b.y1 + reach).ceil() as i32 - y0).intersect(&full);
    if !bounds.is_empty() {
        p.shape(bounds, Paint::Vector(Arc::new(ops)));
    }
}
