//! The desktop clock's grid faces (`desktopClock.face`): a time string
//! turned into an 8-bit coverage mask at a given digit height. `serif` is
//! shaped by the shell through parley, so only its name lives here.
//!
//! Glyphs are rectangles on a 28x50 unit grid, measured off the owner's
//! phone clock (docs/superpowers/refs/desktop-clock-pixel.png, where a
//! digit is 234px tall and one unit is 4.68px). A stroke is 8 units. The
//! caps are inset so the round digits lose their corner cells, and every
//! vertical stroke breaks at the waist (`SEAM`), which only a horizontal
//! bar crosses.

/// The grid a glyph is drawn on, in units.
const GRID_W: usize = 28;
const GRID_H: usize = 50;

/// The waist rows every vertical stroke stops short of.
pub const SEAM: (u32, u32) = (23, 28);

/// The columns a middle bar spans, the only ink the seam rows may carry.
pub const BAR_SPAN: (u32, u32) = (6, 22);

/// Convex corners round at this many units, concave ones fill at `FILLET`.
const RADIUS: f64 = 2.2;
const FILLET: f64 = 1.0;

type Rect = [u8; 4];

const CAP_T: Rect = [6, 0, 22, 8];
const CAP_B: Rect = [6, 42, 22, 50];
const UL: Rect = [0, 6, 8, 23];
const UR: Rect = [20, 6, 28, 23];
const LL: Rect = [0, 28, 8, 44];
const LR: Rect = [20, 28, 28, 44];
const MID: Rect = [6, 21, 22, 29];
// The 9's tail runs further left than its cap, as in the reference.
const TAIL_B: Rect = [3, 42, 22, 50];
const TAIL_T: Rect = [6, 0, 25, 8];

const GLYPHS: [&[Rect]; 10] = [
    &[CAP_T, UL, UR, LL, LR, CAP_B],
    &[[14, 2, 22, 23], [14, 28, 22, 48]],
    &[[3, 0, 22, 8], UR, MID, LL, [0, 42, 28, 50]],
    &[[3, 0, 22, 8], UR, [9, 21, 22, 29], LR, TAIL_B],
    &[[0, 2, 8, 23], [20, 2, 28, 23], MID, [20, 28, 28, 48]],
    &[[0, 0, 28, 8], UL, MID, LR, TAIL_B],
    &[TAIL_T, UL, MID, LL, LR, CAP_B],
    &[[0, 0, 22, 8], UR, [12, 21, 22, 29], [12, 28, 20, 48]],
    &[CAP_T, UL, UR, MID, LL, LR, CAP_B],
    &[CAP_T, UL, UR, MID, LR, TAIL_B],
];

const COLON: &[Rect] = &[[0, 11, 8, 19], [0, 32, 8, 40]];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Pixel,
    Gooey,
    Serif,
}

impl Face {
    pub fn parse(s: &str) -> Option<Face> {
        match s {
            "pixel" => Some(Face::Pixel),
            "gooey" => Some(Face::Gooey),
            "serif" => Some(Face::Serif),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Face::Pixel => "pixel",
            Face::Gooey => "gooey",
            Face::Serif => "serif",
        }
    }
}

/// Nothing's two width buttons: the cell's aspect and the gap between
/// glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Width {
    Narrow,
    Wide,
}

impl Width {
    pub fn parse(s: &str) -> Option<Width> {
        match s {
            "narrow" => Some(Width::Narrow),
            "wide" => Some(Width::Wide),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Width::Narrow => "narrow",
            Width::Wide => "wide",
        }
    }

    /// A horizontal unit against a vertical one.
    fn aspect(self) -> f64 {
        match self {
            Width::Narrow => 0.78,
            Width::Wide => 1.0,
        }
    }

    /// The gap between glyphs, in vertical units.
    fn tracking(self) -> f64 {
        match self {
            Width::Narrow => 5.0,
            Width::Wide => 8.0,
        }
    }
}

/// Coverage, one byte per pixel, row-major.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Mask {
    fn new(width: u32, height: u32) -> Self {
        Mask { width, height, data: vec![0; width as usize * height as usize] }
    }

    pub fn at(&self, x: u32, y: u32) -> u8 {
        self.data[(y * self.width + x) as usize]
    }
}

fn glyph(c: char) -> Option<&'static [Rect]> {
    match c {
        '0'..='9' => Some(GLYPHS[c as usize - '0' as usize]),
        ':' => Some(COLON),
        _ => None,
    }
}

/// A glyph's advance in grid units, before tracking. A space is half a
/// digit; anything else without a glyph takes no room.
fn advance(c: char) -> f64 {
    match c {
        ':' => 8.0,
        ' ' => GRID_W as f64 / 2.0,
        _ if glyph(c).is_some() => GRID_W as f64,
        _ => 0.0,
    }
}

/// The pixel origin of each drawable glyph and the mask's width.
fn layout(text: &str, height: u32, width: Width) -> (Vec<(char, u32)>, u32) {
    let hy = height as f64 / GRID_H as f64;
    let hx = hy * width.aspect();
    let gap = (width.tracking() * hy).round() as u32;
    let mut x = 0u32;
    let mut out = Vec::new();
    for c in text.chars() {
        let adv = advance(c);
        if adv == 0.0 {
            continue;
        }
        if !out.is_empty() || x > 0 {
            x += gap;
        }
        if glyph(c).is_some() {
            out.push((c, x));
        }
        x += (adv * hx).ceil() as u32;
    }
    (out, x)
}

/// The mask's width for `text` without drawing it.
pub fn measure(text: &str, height: u32, width: Width) -> u32 {
    layout(text, height, width).1
}

/// `text` drawn in `face` with digits `height` pixels tall. `None` for
/// `serif`, which the shell shapes itself.
pub fn render(face: Face, text: &str, height: u32, width: Width) -> Option<Mask> {
    if face == Face::Serif {
        return None;
    }
    let (glyphs, w) = layout(text, height, width);
    let mut mask = Mask::new(w, height);
    for (c, x) in glyphs {
        let rects = glyph(c).unwrap_or(&[]);
        match face {
            Face::Pixel => draw_pixel(&mut mask, x, &Cells::of(rects), width),
            _ => draw_gooey(&mut mask, x, rects, width, true),
        }
    }
    Some(mask)
}

/// A glyph's rectangles filled in on the unit grid.
struct Cells([[bool; GRID_W]; GRID_H]);

impl Cells {
    fn of(rects: &[Rect]) -> Self {
        let mut g = [[false; GRID_W]; GRID_H];
        for &[x0, y0, x1, y1] in rects {
            for row in &mut g[y0 as usize..y1 as usize] {
                row[x0 as usize..x1 as usize].fill(true);
            }
        }
        Cells(g)
    }

    fn get(&self, x: i64, y: i64) -> bool {
        x >= 0 && y >= 0 && (x as usize) < GRID_W && (y as usize) < GRID_H && self.0[y as usize][x as usize]
    }

    /// Every outline corner: the grid vertex, the quadrant its arc bulges
    /// into, and whether it is convex.
    fn corners(&self) -> Vec<(i64, i64, i64, i64, bool)> {
        let mut out = Vec::new();
        for vy in 0..=GRID_H as i64 {
            for vx in 0..=GRID_W as i64 {
                let q = [
                    (-1, -1, self.get(vx - 1, vy - 1)),
                    (1, -1, self.get(vx, vy - 1)),
                    (-1, 1, self.get(vx - 1, vy)),
                    (1, 1, self.get(vx, vy)),
                ];
                let filled = q.iter().filter(|c| c.2).count();
                if filled == 1 || filled == 3 {
                    let want = filled == 1;
                    let &(dx, dy, _) = q.iter().find(|c| c.2 == want).unwrap();
                    out.push((vx, vy, dx, dy, want));
                }
            }
        }
        out
    }
}

/// A corner in pixels: the box it owns and whether a sample in that box
/// is inked.
struct Arc {
    vx: f64,
    vy: f64,
    dx: f64,
    dy: f64,
    r: f64,
    convex: bool,
}

impl Arc {
    /// `None` outside the corner's box, else whether the sample is inked.
    fn test(&self, sx: f64, sy: f64) -> Option<bool> {
        let (u, v) = ((sx - self.vx) * self.dx, (sy - self.vy) * self.dy);
        if !(0.0..self.r).contains(&u) || !(0.0..self.r).contains(&v) {
            return None;
        }
        let d = (self.r - u).hypot(self.r - v);
        Some(if self.convex { d <= self.r } else { d >= self.r })
    }

    /// The top-left of the box the corner owns.
    fn origin(&self) -> (f64, f64) {
        (self.vx + self.r * self.dx.min(0.0), self.vy + self.r * self.dy.min(0.0))
    }

    /// Whether this box comes within `pad` pixels of `other`'s.
    fn within(&self, other: &Arc, pad: f64) -> bool {
        let ((ax, ay), (bx, by)) = (self.origin(), other.origin());
        ax < bx + other.r + pad && bx < ax + self.r + pad && ay < by + other.r + pad && by < ay + self.r + pad
    }

}

const SS: usize = 4;

/// The most a dot spacing may be against the stroke. Past about 1.3 the
/// fields of two dots no longer reach the threshold between them.
const GOO_SPACING: f64 = 1.2;

fn draw_pixel(mask: &mut Mask, ox: u32, cells: &Cells, width: Width) {
    let hy = mask.height as f64 / GRID_H as f64;
    let hx = hy * width.aspect();
    let arcs: Vec<Arc> = cells
        .corners()
        .into_iter()
        .map(|(vx, vy, dx, dy, convex)| Arc {
            vx: vx as f64 * hx,
            vy: vy as f64 * hy,
            dx: dx as f64,
            dy: dy as f64,
            r: if convex { RADIUS } else { FILLET } * hy,
            convex,
        })
        .collect();
    let inked = |near: &[&Arc], sx: f64, sy: f64| -> bool {
        for a in near {
            if let Some(v) = a.test(sx, sy) {
                return v;
            }
        }
        cells.get((sx / hx).floor() as i64, (sy / hy).floor() as i64)
    };
    let gw = (GRID_W as f64 * hx).ceil() as u32;
    let cols = gw.min(mask.width.saturating_sub(ox));
    // Away from the corners the outline is the cells' own edges, so a pixel
    // takes its exact overlap with the filled cells.
    let span = |p: u32, unit: f64| -> (i64, i64) {
        ((p as f64 / unit).floor() as i64, ((p + 1) as f64 / unit).ceil() as i64)
    };
    let overlap = |p: u32, cell: i64, unit: f64| -> f64 {
        let (a, b) = (cell as f64 * unit, (cell + 1) as f64 * unit);
        ((p + 1) as f64).min(b) - (p as f64).max(a)
    };
    for py in 0..mask.height {
        let (cy0, cy1) = span(py, hy);
        for px in 0..cols {
            let (cx0, cx1) = span(px, hx);
            let mut cov = 0.0;
            for cy in cy0..cy1 {
                for cx in cx0..cx1 {
                    if cells.get(cx, cy) {
                        cov += overlap(px, cx, hx).max(0.0) * overlap(py, cy, hy).max(0.0);
                    }
                }
            }
            let i = (py * mask.width + ox + px) as usize;
            mask.data[i] = mask.data[i].max((cov * 255.0).round() as u8);
        }
    }
    // The corners' boxes, supersampled; a pixel two boxes touch is drawn once.
    let mut done = vec![false; cols as usize * mask.height as usize];
    for a in &arcs {
        let (bx0, by0) = a.origin();
        let near: Vec<&Arc> = arcs.iter().filter(|b| b.within(a, 1.0)).collect();
        let xs = bx0.floor().max(0.0) as u32..((bx0 + a.r).ceil() as u32).min(cols);
        let ys = by0.floor().max(0.0) as u32..((by0 + a.r).ceil() as u32).min(mask.height);
        for py in ys {
            for px in xs.clone() {
                let seen = &mut done[(py * cols + px) as usize];
                if std::mem::replace(seen, true) {
                    continue;
                }
                let mut n = 0;
                for j in 0..SS {
                    for i in 0..SS {
                        let sx = px as f64 + (i as f64 + 0.5) / SS as f64;
                        let sy = py as f64 + (j as f64 + 0.5) / SS as f64;
                        n += inked(&near, sx, sy) as u32;
                    }
                }
                let i = (py * mask.width + ox + px) as usize;
                mask.data[i] = (n * 255 / (SS * SS) as u32) as u8;
            }
        }
    }
}

/// Dot centres along each rectangle's long axis, a little more than a
/// stroke apart so the necks between them pinch, in grid units, with the
/// stroke width.
fn dots(rects: &[Rect]) -> Vec<(f64, f64, f64)> {
    let mut out: Vec<(f64, f64, f64)> = Vec::new();
    for &[x0, y0, x1, y1] in rects {
        let (x0, y0, x1, y1) = (x0 as f64, y0 as f64, x1 as f64, y1 as f64);
        let t = (x1 - x0).min(y1 - y0);
        let (a, b, horizontal) = if x1 - x0 >= y1 - y0 { (x0, x1, true) } else { (y0, y1, false) };
        let span = b - a - t;
        let n = (span / (t * GOO_SPACING)).ceil().max(0.0) as usize;
        for k in 0..=n {
            let along = a + t / 2.0 + if n == 0 { 0.0 } else { span * k as f64 / n as f64 };
            let p = if horizontal { (along, (y0 + y1) / 2.0) } else { ((x0 + x1) / 2.0, along) };
            if !out.iter().any(|d| (d.0 - p.0).hypot(d.1 - p.1) < t / 2.0) {
                out.push((p.0, p.1, t));
            }
        }
    }
    out
}

/// A dot's field at `d2`, the squared distance over the squared radius:
/// Wyvill's falloff, zero at twice the radius.
fn falloff(d2: f64) -> f64 {
    let t = d2 / 4.0;
    if t >= 1.0 { 0.0 } else { (1.0 - t).powi(3) }
}

/// The level a lone dot's field crosses at its own radius, so a single dot
/// draws at its own size and neighbours swell into each other.
fn threshold() -> f64 {
    falloff(1.0)
}

/// The steepest `falloff` gets per pixel of distance, times the radius:
/// 3(1-t)^2 sqrt(t) at t = 0.2.
const FALLOFF_SLOPE: f64 = 0.86;

/// `merge` false draws the dots as plain disks, what the field adds to.
fn draw_gooey(mask: &mut Mask, ox: u32, rects: &[Rect], width: Width, merge: bool) {
    let hy = mask.height as f64 / GRID_H as f64;
    let hx = hy * width.aspect();
    // Dots stay round: their radius follows the narrower unit.
    let unit = hx.min(hy);
    let dots: Vec<(f64, f64, f64)> =
        dots(rects).into_iter().map(|(x, y, t)| (x * hx, y * hy, t * unit * 0.5)).collect();
    let level = if merge { threshold() } else { 0.5 };
    let gw = (GRID_W as f64 * hx).ceil() as u32;
    let mut near = Vec::with_capacity(dots.len());
    for py in 0..mask.height {
        for px in 0..gw.min(mask.width.saturating_sub(ox)) {
            let (x0, y0) = (px as f64, py as f64);
            near.clear();
            near.extend(dots.iter().filter(|&&(cx, cy, r)| {
                let reach = if merge { 2.0 * r } else { r } + 1.0;
                (cx - x0 - 0.5).abs() < reach && (cy - y0 - 0.5).abs() < reach
            }));
            if near.is_empty() {
                continue;
            }
            let field = |x: f64, y: f64| -> f64 {
                if merge {
                    near.iter().map(|&(cx, cy, r)| falloff(((x - cx).powi(2) + (y - cy).powi(2)) / (r * r))).sum()
                } else {
                    near.iter().any(|&(cx, cy, r)| (x - cx).hypot(y - cy) <= r) as u8 as f64
                }
            };
            // The field moves no faster than its slope, so a centre far enough
            // from the level decides the whole pixel.
            if merge {
                let slope: f64 = near.iter().map(|d| FALLOFF_SLOPE / d.2).sum();
                let f = field(x0 + 0.5, y0 + 0.5);
                if (f - level).abs() > slope * std::f64::consts::FRAC_1_SQRT_2 {
                    if f > level {
                        mask.data[(py * mask.width + ox + px) as usize] = 255;
                    }
                    continue;
                }
            }
            let mut n = 0;
            for j in 0..SS {
                for i in 0..SS {
                    let sx = x0 + (i as f64 + 0.5) / SS as f64;
                    let sy = y0 + (j as f64 + 0.5) / SS as f64;
                    n += (field(sx, sy) >= level) as u32;
                }
            }
            let cov = (n * 255 / (SS * SS) as u32) as u8;
            let i = (py * mask.width + ox + px) as usize;
            mask.data[i] = mask.data[i].max(cov);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: u32 = 240;

    fn ink(m: &Mask) -> u64 {
        m.data.iter().map(|&v| v as u64).sum()
    }

    #[test]
    fn every_digit_is_inked_and_stays_in_its_box() {
        for face in [Face::Pixel, Face::Gooey] {
            for width in [Width::Narrow, Width::Wide] {
                for c in '0'..='9' {
                    // A digit drawn on a wider canvas leaves the rest of it blank.
                    let gw = measure(&c.to_string(), H, width);
                    let mut m = Mask::new(gw + 40, H);
                    let rects = glyph(c).unwrap();
                    match face {
                        Face::Pixel => draw_pixel(&mut m, 20, &Cells::of(rects), width),
                        _ => draw_gooey(&mut m, 20, rects, width, true),
                    }
                    assert!(ink(&m) > 0, "{c} {face:?} {width:?} is empty");
                    for y in 0..H {
                        for x in (0..20).chain(20 + gw..m.width) {
                            assert_eq!(m.at(x, y), 0, "{c} {face:?} {width:?} inked at {x},{y}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn the_seam_rows_carry_only_a_middle_bar() {
        let hy = H as f64 / GRID_H as f64;
        let y = ((SEAM.0 + SEAM.1) as f64 / 2.0 * hy) as u32;
        for c in '0'..='9' {
            let m = render(Face::Pixel, &c.to_string(), H, Width::Wide).unwrap();
            let bar = (BAR_SPAN.0 as f64 * hy) as u32..(BAR_SPAN.1 as f64 * hy).ceil() as u32;
            for x in 0..m.width {
                if !bar.contains(&x) {
                    assert_eq!(m.at(x, y), 0, "{c} inked at {x} in the seam");
                }
            }
        }
        for c in ['0', '1'] {
            let m = render(Face::Pixel, &c.to_string(), H, Width::Wide).unwrap();
            assert!((0..m.width).all(|x| m.at(x, y) == 0), "{c} crosses the seam");
        }
    }

    #[test]
    fn a_convex_corner_is_rounded_and_a_concave_one_filled() {
        let m = render(Face::Pixel, "0", H, Width::Wide).unwrap();
        let hy = H as f64 / GRID_H as f64;
        // The cap's top-left corner pixel is partly inked, a pixel further in
        // fully.
        let (cx, cy) = ((6.0 * hy) as u32, 0);
        assert_eq!(m.at(cx, cy), 0);
        let edge = (0..m.width).find(|&x| m.at(x, cy) > 0).unwrap();
        assert!(edge > cx + 2, "the cap's corner is square");
        assert!((1..255).contains(&m.at(edge, cy)) || (1..255).contains(&m.at(edge - 1, cy)));
        assert_eq!(m.at((10.0 * hy) as u32, (4.0 * hy) as u32), 255);
        // The corner cell of the glyph's own box is empty.
        assert_eq!(m.at(2, 2), 0);
        // Just outside the concave corner where the cap meets the left
        // stroke, the fillet puts ink.
        let (fx, fy) = ((6.0 * hy) as u32 - 1, (6.0 * hy) as u32 - 1);
        assert!(m.at(fx, fy) > 0, "no fillet at the cap's inner corner");
    }

    #[test]
    fn gooey_runs_its_dots_together() {
        for c in '0'..='9' {
            let rects = glyph(c).unwrap();
            let w = measure(&c.to_string(), H, Width::Wide);
            let (mut goo, mut disks) = (Mask::new(w, H), Mask::new(w, H));
            draw_gooey(&mut goo, 0, rects, Width::Wide, true);
            draw_gooey(&mut disks, 0, rects, Width::Wide, false);
            assert!(ink(&goo) > ink(&disks), "{c}: the field adds nothing");
        }
    }

    #[test]
    fn the_same_text_draws_the_same_mask() {
        for face in [Face::Pixel, Face::Gooey] {
            for width in [Width::Narrow, Width::Wide] {
                assert_eq!(render(face, "09:19", 180, width), render(face, "09:19", 180, width));
            }
        }
        assert_eq!(render(Face::Serif, "09:19", 180, Width::Wide), None);
    }

    #[test]
    fn layout_is_tabular_and_measured() {
        let w = measure("0", H, Width::Wide);
        assert_eq!(measure("00", H, Width::Wide), 2 * w + (8.0 * H as f64 / 50.0).round() as u32);
        assert_eq!(measure("11", H, Width::Wide), measure("00", H, Width::Wide));
        assert!(measure("09:19", H, Width::Narrow) < measure("09:19", H, Width::Wide));
        assert_eq!(render(Face::Pixel, "09:19", H, Width::Wide).unwrap().width, measure("09:19", H, Width::Wide));
        for s in ["pixel", "gooey", "serif"] {
            assert_eq!(Face::parse(s).unwrap().as_str(), s);
        }
        for s in ["narrow", "wide"] {
            assert_eq!(Width::parse(s).unwrap().as_str(), s);
        }
    }

    /// Writes every face and width for 09:19 at 240px into
    /// `artifacts/desktop-clock/` for reading beside the reference.
    #[test]
    #[ignore]
    fn write_faces() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/desktop-clock");
        std::fs::create_dir_all(&dir).unwrap();
        for (text, suffix) in [("09:19", ""), ("0123456789", "-digits")] {
            for face in [Face::Pixel, Face::Gooey] {
                for width in [Width::Narrow, Width::Wide] {
                    let m = render(face, text, 240, width).unwrap();
                let pad = 40;
                let mut img = image::GrayImage::from_pixel(m.width + 2 * pad, m.height + 2 * pad, image::Luma([96]));
                for y in 0..m.height {
                    for x in 0..m.width {
                        let a = m.at(x, y) as u32;
                        img.put_pixel(x + pad, y + pad, image::Luma([(96 + (255 - 96) * a / 255) as u8]));
                    }
                }
                let path = dir.join(format!("{}-{}{suffix}.png", face.as_str(), width.as_str()));
                img.save(&path).unwrap();
                println!("{}", path.display());
                }
            }
        }
    }
}
