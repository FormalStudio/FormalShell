/// An output edge: where the bar, a drawer's line or a card sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn parse(s: &str) -> Option<Edge> {
        match s {
            "top" => Some(Edge::Top),
            "bottom" => Some(Edge::Bottom),
            "left" => Some(Edge::Left),
            "right" => Some(Edge::Right),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left",
            Edge::Right => "right",
        }
    }

    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

/// One of the bar's three regions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Left,
    Center,
    Right,
}

impl Region {
    pub const ALL: [Region; 3] = [Region::Left, Region::Center, Region::Right];

    pub fn parse(s: &str) -> Option<Region> {
        match s {
            "left" => Some(Region::Left),
            "center" => Some(Region::Center),
            "right" => Some(Region::Right),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Region::Left => "left",
            Region::Center => "center",
            Region::Right => "right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Half-open: the right and bottom edges are outside.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// The overlap, none when the two only touch or miss.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = ((self.x + self.width).min(o.x + o.width), (self.y + self.height).min(o.y + o.height));
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }
}

/// How much of each output edge is spoken for.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// A rectangle in whole pixels. Its wire form names the size `width` and
/// `height`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    #[serde(rename = "width")]
    pub w: i32,
    #[serde(rename = "height")]
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    /// Empty, never negative, where the two miss: a clip of negative
    /// height is an inverted rect, which the rasteriser fills.
    pub fn intersect(&self, o: &IRect) -> IRect {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        IRect::new(x, y, (self.right().min(o.right()) - x).max(0), (self.bottom().min(o.bottom()) - y).max(0))
    }

    pub fn intersects(&self, o: &IRect) -> bool {
        !self.intersect(o).is_empty()
    }

    pub fn contains(&self, o: &IRect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    pub fn union(&self, o: &IRect) -> IRect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        IRect::new(x, y, self.right().max(o.right()) - x, self.bottom().max(o.bottom()) - y)
    }

    pub fn area(&self) -> i64 {
        if self.is_empty() { 0 } else { self.w as i64 * self.h as i64 }
    }
}

impl From<IRect> for Rect {
    fn from(r: IRect) -> Rect {
        Rect::new(f64::from(r.x), f64::from(r.y), f64::from(r.w), f64::from(r.h))
    }
}
