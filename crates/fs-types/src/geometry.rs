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
}

/// How much of each output edge is spoken for.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Insets {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// A rectangle in whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl IRect {
    pub fn new(x: i64, y: i64, width: i64, height: i64) -> IRect {
        IRect {
            x,
            y,
            width,
            height,
        }
    }
}
