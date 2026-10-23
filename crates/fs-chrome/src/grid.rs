//! The backdrop grid (`theme.grid`): lines on a fixed pitch over the whole
//! output, fading out over its bottom stretch. One geometry for every place
//! that draws it, so the lock and the greeter put their lines exactly where
//! the desktop's are.
//!
//! Each line ends on its position rather than starting there, the way the
//! reference's `background-position: -2px` puts it: horizontal lines end on
//! every multiple of the pitch from the top, vertical ones on a column
//! centred on the output, so the middle of the screen is the middle of a cell.

/// In the units of the buffer it is drawn into.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub pitch: f64,
    pub width: f64,
    /// The bottom fraction of the height the lines fade out over.
    pub fade: f64,
    /// The line colour in the buffer's own channel order, alpha last.
    pub color: [u8; 4],
}

/// A horizontal or vertical stretch of line, `x0..x1` by `y0..y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl Grid {
    /// The same grid at `scale` times its size, for a buffer at an output's
    /// own scale.
    pub fn scaled(self, scale: f64) -> Self {
        Self { pitch: self.pitch * scale, width: self.width * scale, ..self }
    }

    fn usable(&self) -> bool {
        self.pitch > self.width && self.width > 0.0 && self.color[3] > 0
    }

    /// Every line over a `w` by `h` area: the horizontals first, then the
    /// verticals, each the full length of the area.
    pub fn lines(&self, w: f64, h: f64) -> Vec<Line> {
        let mut out = Vec::new();
        if !self.usable() || w <= 0.0 || h <= 0.0 {
            return out;
        }
        let mut y = self.pitch;
        while y - self.width < h {
            out.push(Line { x0: 0.0, y0: y - self.width, x1: w, y1: y.min(h) });
            y += self.pitch;
        }
        let mut x = (w / 2.0 - self.pitch / 2.0).rem_euclid(self.pitch);
        if x <= 0.0 {
            x += self.pitch;
        }
        while x - self.width < w {
            out.push(Line { x0: (x - self.width).max(0.0), y0: 0.0, x1: x.min(w), y1: h });
            x += self.pitch;
        }
        out
    }

    /// Where the fade starts, down an area `h` tall.
    pub fn fade_from(&self, h: f64) -> f64 {
        h * (1.0 - self.fade.clamp(0.0, 1.0))
    }

    /// The share of the line's alpha left at `y`: whole above the fade,
    /// none at the bottom edge.
    pub fn strength(&self, y: f64, h: f64) -> f64 {
        let from = self.fade_from(h);
        if y <= from || h <= from { 1.0 } else { ((h - y) / (h - from)).clamp(0.0, 1.0) }
    }

    /// The alpha each pixel of a `w` by `h` buffer takes, 0 to 255, or none
    /// when the grid draws nothing. A crossing is one line, never two.
    fn coverage(&self, w: usize, h: usize) -> Option<(Vec<bool>, Vec<bool>, Vec<u8>)> {
        if !self.usable() || w == 0 || h == 0 {
            return None;
        }
        let (mut cols, mut rows) = (vec![false; w], vec![false; h]);
        for l in self.lines(w as f64, h as f64) {
            let (on, a, b) = if l.x1 - l.x0 >= w as f64 { (&mut rows, l.y0, l.y1) } else { (&mut cols, l.x0, l.x1) };
            let (a, b) = (a.round().max(0.0) as usize, b.round().max(0.0) as usize);
            // A line thinner than a pixel still draws one.
            let b = b.max(a + 1).min(on.len());
            on[a.min(b)..b].fill(true);
        }
        let alpha = (0..h).map(|y| (f64::from(self.color[3]) * self.strength(y as f64 + 0.5, h as f64)).round() as u8).collect();
        Some((cols, rows, alpha))
    }

    /// The lines laid over a buffer of 4-byte pixels, `w` by `h`, in the
    /// order [`Grid::color`] is given in. The fourth byte is left alone.
    pub fn paint(&self, buf: &mut [u8], w: usize, h: usize) {
        self.each(buf, w, h, |c, ink, a| {
            let a = u32::from(a);
            ((u32::from(c) * (255 - a) + u32::from(ink) * a + 127) / 255) as u8
        });
    }

    /// [`Grid::paint`] undone, to within a level of rounding: a picture
    /// made with one grid handed to a surface that wants another, or none.
    pub fn lift(&self, buf: &mut [u8], w: usize, h: usize) {
        self.each(buf, w, h, |c, ink, a| {
            if a == 255 {
                return c;
            }
            let a = f64::from(a) / 255.0;
            ((f64::from(c) - f64::from(ink) * a) / (1.0 - a)).round().clamp(0.0, 255.0) as u8
        });
    }

    fn each(&self, buf: &mut [u8], w: usize, h: usize, f: impl Fn(u8, u8, u8) -> u8) {
        let Some((cols, rows, alpha)) = self.coverage(w, h) else { return };
        if buf.len() < w * h * 4 {
            return;
        }
        for (y, row) in buf.chunks_exact_mut(w * 4).take(h).enumerate() {
            let a = alpha[y];
            if a == 0 {
                continue;
            }
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                if rows[y] || cols[x] {
                    for i in 0..3 {
                        px[i] = f(px[i], self.color[i], a);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference's own numbers at scale 1.
    fn grid() -> Grid {
        Grid { pitch: 120.0, width: 2.0, fade: 0.2, color: [232, 230, 227, 36] }
    }

    fn flat(w: usize, h: usize, c: [u8; 4]) -> Vec<u8> {
        c.iter().copied().cycle().take(w * h * 4).collect()
    }

    fn px(buf: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        let i = (y * w + x) * 4;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    }

    #[test]
    fn lines_fall_where_the_reference_draws_them() {
        let lines = grid().lines(1440.0, 900.0);
        let horizontal: Vec<f64> = lines.iter().filter(|l| l.x1 - l.x0 == 1440.0).map(|l| l.y1).collect();
        let vertical: Vec<f64> = lines.iter().filter(|l| l.y1 - l.y0 == 900.0).map(|l| l.x1).collect();
        assert_eq!(horizontal, [120.0, 240.0, 360.0, 480.0, 600.0, 720.0, 840.0]);
        assert_eq!(vertical.first(), Some(&60.0));
        assert_eq!(vertical.len(), 12);
        // The middle of the screen is the middle of a cell.
        assert!(vertical.contains(&660.0) && vertical.contains(&780.0));
    }

    #[test]
    fn a_flat_buffer_takes_lines_and_nothing_between_them() {
        let (w, h) = (480, 300);
        let bg = [24, 26, 27, 255];
        let mut buf = flat(w, h, bg);
        grid().paint(&mut buf, w, h);
        assert_eq!(px(&buf, w, 10, 10), bg, "inside a cell");
        let on = px(&buf, w, 10, 119);
        assert!(on[0] > bg[0] + 20 && on[3] == 255, "on a line: {on:?}");
        assert_eq!(px(&buf, w, 10, 120), bg, "the line ends on the pitch");
        // Vertical lines end on 240 +- 120 for a 480 wide buffer: 60, 180...
        assert_eq!(px(&buf, w, 59, 10), on);
        assert_eq!(px(&buf, w, 60, 10), bg);
        // A crossing is one line, not two laid over each other.
        assert_eq!(px(&buf, w, 59, 119), on);
    }

    #[test]
    fn the_bottom_fifth_fades_out() {
        let (w, h) = (100, 1000);
        let g = Grid { pitch: 50.0, width: 2.0, fade: 0.2, color: [255, 255, 255, 255] };
        let mut buf = flat(w, h, [0, 0, 0, 255]);
        g.paint(&mut buf, w, h);
        let at = |y| px(&buf, w, 3, y)[0];
        assert_eq!(at(749), 255);
        assert_eq!(at(799), 255);
        assert!((120..135).contains(&at(899)), "half way down the fade: {}", at(899));
        assert!(at(999) < 3);
    }

    #[test]
    fn lift_undoes_paint() {
        let (w, h) = (360, 240);
        let mut buf: Vec<u8> = (0..w * h * 4).map(|i| if i % 4 == 3 { 255 } else { (i * 7 % 251) as u8 }).collect();
        let before = buf.clone();
        let g = grid();
        g.paint(&mut buf, w, h);
        assert_ne!(buf, before);
        g.lift(&mut buf, w, h);
        let worst = buf.iter().zip(&before).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
        assert!(worst <= 1, "{worst}");
    }

    #[test]
    fn scaled_lines_keep_their_cells() {
        let g = grid().scaled(1.5);
        assert_eq!((g.pitch, g.width), (180.0, 3.0));
        let lines = g.lines(2160.0, 1350.0);
        assert!(lines.iter().any(|l| l.x1 == 990.0), "the centre cell is centred");
    }

    #[test]
    fn a_grid_with_no_ink_or_room_draws_nothing() {
        let mut buf = flat(64, 64, [9, 9, 9, 255]);
        let before = buf.clone();
        Grid { color: [255, 255, 255, 0], ..grid() }.paint(&mut buf, 64, 64);
        Grid { width: 200.0, ..grid() }.paint(&mut buf, 64, 64);
        assert_eq!(buf, before);
        assert!(Grid { pitch: 0.0, ..grid() }.lines(100.0, 100.0).is_empty());
    }
}
