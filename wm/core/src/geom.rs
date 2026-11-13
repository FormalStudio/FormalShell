//! Logical-pixel rectangles.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub fn inset(&self, by: f64) -> Rect {
        Rect::new(
            self.x + by,
            self.y + by,
            (self.w - 2.0 * by).max(0.0),
            (self.h - 2.0 * by).max(0.0),
        )
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// Scaled by `sx` and `sy` with `origin` held where it is.
    pub fn scaled_about(&self, origin: (f64, f64), sx: f64, sy: f64) -> Rect {
        Rect::new(
            origin.0 + (self.x - origin.0) * sx,
            origin.1 + (self.y - origin.1) * sy,
            self.w * sx,
            self.h * sy,
        )
    }

    /// Edges on whole pixels, so neighbours that share an edge still share it.
    pub fn snapped(&self) -> Rect {
        let x = self.x.round();
        let y = self.y.round();
        Rect::new(x, y, self.right().round() - x, self.bottom().round() - y)
    }

    pub fn lerp(&self, to: &Rect, t: f64) -> Rect {
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Rect::new(
            mix(self.x, to.x),
            mix(self.y, to.y),
            mix(self.w, to.w),
            mix(self.h, to.h),
        )
    }
}
