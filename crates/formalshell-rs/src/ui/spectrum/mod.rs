//! The media panel's spectrum: a style draws its shape list for the
//! frame, and each shape becomes one node. A style keeps its own state (its
//! caps, particles and traces) per element, thrown away when the style
//! changes or the live state flips, so each style's resting frame is drawn
//! over the all-zero baseline.

use std::time::Instant;

use fs_media::visualizer::model;
use fs_media::visualizer::style::Style;
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use vello_cpu::kurbo::{self, Affine, BezPath, Rect, Shape as _};

use crate::scene::{Paint, Scene};
use fs_types::IRect;
use crate::services::visualizer;
use crate::text::TextStyle;
use crate::ui::{Cx, State, Widget, irect};

pub mod band;
pub mod scene;
pub mod styles;

use self::scene::{self as vis, PathEl, Shape, TextAlign};

/// The spectrum in one style, `columns` wide, redrawn every frame off
/// `feed` while it has one and at rest otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectrum {
    pub style: Style,
    pub columns: usize,
    pub feed: Option<visualizer::Feed>,
}

struct Live {
    style: Option<Style>,
    live: bool,
    state: styles::State,
    shown: Vec<f64>,
    last: Option<Instant>,
}

impl State for Live {}

impl Widget for Spectrum {
    fn measure(&self, cx: &mut Cx, _avail: f64) -> (f64, f64) {
        let s = &cx.theme.space;
        let n = self.columns as f64;
        (n * s.track_thickness + (n - 1.0).max(0.0) * s.xxs, s.control_height)
    }

    fn paint(&self, cx: &mut Cx, inner: Rect, path: &str) {
        paint(cx, inner, self.style, self.columns, self.feed, path);
    }
}

fn faded(c: Rgba, alpha: f64, outer: f32) -> Rgba {
    Rgba { a: c.a * alpha as f32 * outer, ..c }
}

fn path(els: &[PathEl], at: Affine) -> BezPath {
    let mut p = BezPath::new();
    for el in els {
        match *el {
            PathEl::MoveTo(x, y) => p.move_to((x, y)),
            PathEl::LineTo(x, y) => p.line_to((x, y)),
            PathEl::QuadTo(cx, cy, x, y) => p.quad_to((cx, cy), (x, y)),
            PathEl::Arc { cx, cy, r, start, end } => {
                let arc = kurbo::Arc::new((cx, cy), (r, r), start, end - start, 0.0);
                p.extend(arc.path_elements(0.1));
            }
            PathEl::RoundedRect { x, y, w, h, r } => {
                p.extend(kurbo::RoundedRect::new(x, y, x + w, y + h, r).path_elements(0.1));
            }
            PathEl::Close => p.close_path(),
        }
    }
    p.apply_affine(at);
    p
}

fn paint(cx: &mut Cx, r: Rect, style: Style, columns: usize, feed: Option<visualizer::Feed>, key: &str) {
    let t = cx.theme;
    let ink = styles::Ink {
        groove: t.colors.get(ColorRole::Muted),
        dim: t.colors.get(ColorRole::MutedForeground),
        content: t.colors.get(ColorRole::Foreground),
        accent: t.colors.get(ColorRole::Primary),
        mono: String::new(),
        columns,
        gap: t.space.xxs,
        radius: t.radii.sm,
    };
    let live = feed.is_some();
    let frame = feed.map_or_else(visualizer::Frame::baseline, |f| f.frame(cx.now));
    let now = cx.now;
    let mut held = cx.take(key, || Live {
        style: None,
        live: false,
        state: styles::State::default(),
        shown: model::baseline_levels(),
        last: None,
    });
    let st = &mut *held;
    if st.style != Some(style) || st.live != live {
        st.style = Some(style);
        st.live = live;
        st.state = styles::State::default();
        st.shown = model::baseline_levels();
        st.last = None;
    }
    let dt = match (live, st.last) {
        (true, Some(l)) => now.saturating_duration_since(l).as_secs_f64().min(styles::MAX_DT),
        _ => 0.0,
    };
    if live {
        st.last = Some(now);
        st.shown = model::smooth_levels(&st.shown, &frame.mono, dt);
    }
    let mut scene = vis::Scene::new();
    styles::draw(style, &mut scene, r.width(), r.height(), &st.shown, &mut st.state, &ink, dt, Some(&frame.left), Some(&frame.right));
    cx.keep(held);
    if live {
        cx.animate();
    }

    let outer = cx.alpha;
    let at = Affine::translate((r.x0, r.y0));
    let mono = cx.kit.look.mono;
    let clip = irect(r);
    let mut shaped = Vec::new();
    for s in &scene.shapes {
        if let Shape::Text { text, size, .. } = s {
            shaped.push(Some(cx.kit.shape(text, TextStyle { family: mono, size: *size as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None })));
        } else {
            shaped.push(None);
        }
    }
    let mut p = cx.painter(key);
    for (s, text) in scene.shapes.iter().zip(shaped) {
        match s {
            Shape::Rect { x, y, w, h, color, alpha } => {
                let mut b = BezPath::new();
                b.extend(Rect::new(*x, *y, x + w, y + h).path_elements(0.1));
                b.apply_affine(at);
                let bounds = Scene::cover(b.bounding_box(), Affine::IDENTITY, 1.0);
                p.shape_in(bounds, Paint::Shape { fill: Some((b, faded(*color, *alpha, outer))), strokes: Vec::new() }, clip);
            }
            Shape::Fill { path: els, color, alpha } => {
                let b = path(els, at);
                let bounds = Scene::cover(b.bounding_box(), Affine::IDENTITY, 1.0);
                p.shape_in(bounds, Paint::Shape { fill: Some((b, faded(*color, *alpha, outer))), strokes: Vec::new() }, clip);
            }
            Shape::Stroke { path: els, color, alpha, width } => {
                let b = path(els, at);
                let bounds = Scene::cover(b.bounding_box(), Affine::IDENTITY, width + 1.0);
                p.shape_in(bounds, Paint::Shape { fill: None, strokes: vec![(b, faded(*color, *alpha, outer), *width)] }, clip);
            }
            Shape::Text { x, y, align, color, alpha, .. } => {
                let Some(t) = text else { continue };
                let w = t.width as f64;
                let x = r.x0 + x - if *align == TextAlign::Center { w / 2.0 } else { 0.0 };
                let y = r.y0 + y - t.line_height() as f64 / 2.0;
                let (bw, bh) = t.box_size();
                let bounds = IRect::new(x.round() as i32 - crate::text::PAD, y.round() as i32 - crate::text::PAD, bw, bh);
                p.text_in(&t, bounds, Affine::IDENTITY, Some(clip), faded(*color, *alpha, outer), &[]);
            }
        }
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}
