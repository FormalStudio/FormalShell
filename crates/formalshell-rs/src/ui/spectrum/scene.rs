//! What the visualizer styles draw: a flat list of shapes with their colour
//! and alpha resolved. The styles issue the same calls a 2D canvas takes
//! (`fill_rect`, a path then `fill`, ...) and `Scene` records each as a
//! `Shape`; a renderer walks `Scene::shapes` and never sees the canvas state.

use fs_theme::color::Rgba;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathEl {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    /// Control point, then end point.
    QuadTo(f64, f64, f64, f64),
    /// A full circle: centre, radius, start and end angle in radians.
    Arc {
        cx: f64,
        cy: f64,
        r: f64,
        start: f64,
        end: f64,
    },
    RoundedRect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        r: f64,
    },
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    Middle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Rect {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        color: Rgba,
        alpha: f64,
    },
    Fill {
        path: Vec<PathEl>,
        color: Rgba,
        alpha: f64,
    },
    Stroke {
        path: Vec<PathEl>,
        color: Rgba,
        alpha: f64,
        width: f64,
    },
    /// `size` is the font's pixel size, `family` the mono family name.
    Text {
        text: String,
        x: f64,
        y: f64,
        size: f64,
        family: String,
        align: TextAlign,
        baseline: TextBaseline,
        color: Rgba,
        alpha: f64,
    },
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub shapes: Vec<Shape>,
    pub fill_style: Rgba,
    pub stroke_style: Rgba,
    pub line_width: f64,
    pub global_alpha: f64,
    pub font_size: f64,
    pub font_family: String,
    pub text_align: TextAlign,
    pub text_baseline: TextBaseline,
    path: Vec<PathEl>,
}

impl Default for Scene {
    fn default() -> Self {
        Scene {
            shapes: Vec::new(),
            fill_style: Rgba::BLACK,
            stroke_style: Rgba::BLACK,
            line_width: 1.0,
            global_alpha: 1.0,
            font_size: 10.0,
            font_family: String::new(),
            text_align: TextAlign::Left,
            text_baseline: TextBaseline::Middle,
            path: Vec::new(),
        }
    }
}

impl Scene {
    pub fn new() -> Scene {
        Scene::default()
    }

    pub(crate) fn begin_path(&mut self) {
        self.path.clear();
    }

    pub(crate) fn close_path(&mut self) {
        self.path.push(PathEl::Close);
    }

    pub(crate) fn move_to(&mut self, x: f64, y: f64) {
        self.path.push(PathEl::MoveTo(x, y));
    }

    pub(crate) fn line_to(&mut self, x: f64, y: f64) {
        self.path.push(PathEl::LineTo(x, y));
    }

    pub(crate) fn quad_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.path.push(PathEl::QuadTo(cx, cy, x, y));
    }

    pub(crate) fn arc(&mut self, cx: f64, cy: f64, r: f64, start: f64, end: f64) {
        self.path.push(PathEl::Arc {
            cx,
            cy,
            r,
            start,
            end,
        });
    }

    pub(crate) fn rounded_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        self.path.push(PathEl::RoundedRect { x, y, w, h, r });
    }

    pub(crate) fn fill(&mut self) {
        self.shapes.push(Shape::Fill {
            path: self.path.clone(),
            color: self.fill_style,
            alpha: self.global_alpha,
        });
    }

    pub(crate) fn stroke(&mut self) {
        self.shapes.push(Shape::Stroke {
            path: self.path.clone(),
            color: self.stroke_style,
            alpha: self.global_alpha,
            width: self.line_width,
        });
    }

    pub(crate) fn fill_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.shapes.push(Shape::Rect {
            x,
            y,
            w,
            h,
            color: self.fill_style,
            alpha: self.global_alpha,
        });
    }

    pub(crate) fn set_font(&mut self, size: f64, family: &str) {
        self.font_size = size;
        self.font_family = family.to_string();
    }

    pub(crate) fn fill_text(&mut self, text: &str, x: f64, y: f64) {
        self.shapes.push(Shape::Text {
            text: text.to_string(),
            x,
            y,
            size: self.font_size,
            family: self.font_family.clone(),
            align: self.text_align,
            baseline: self.text_baseline,
            color: self.fill_style,
            alpha: self.global_alpha,
        });
    }
}
