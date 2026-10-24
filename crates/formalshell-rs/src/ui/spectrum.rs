//! The spectrum element: fs-media's style draws its shape list for the
//! frame, and each shape becomes one node. A style keeps its own state (its
//! caps, particles and traces) per element, thrown away when the style
//! changes or the live state flips, so each style's resting frame is drawn
//! over the all-zero baseline.

use std::time::Instant;

use fs_media::visualizer::model;
use fs_media::visualizer::scene::{self as vis, PathEl, Shape, TextAlign};
use fs_media::visualizer::styles;
use fs_theme::color::Rgba;
use vello_cpu::kurbo::{self, Affine, BezPath, Rect, Shape as _};

use super::Cx;
use crate::scene::{IRect, Paint, Scene};
use crate::services::visualizer;
use crate::text::TextStyle;

pub struct Live {
    style: String,
    live: bool,
    state: styles::State,
    shown: Vec<f64>,
    last: Option<Instant>,
    pub seen: u64,
}

fn to_vis(c: Rgba) -> vis::Color {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    vis::Color { r: b(c.r), g: b(c.g), b: b(c.b), a: b(c.a) }
}

fn from_vis(c: vis::Color, alpha: f64, outer: f32) -> Rgba {
    Rgba { r: c.r as f32 / 255.0, g: c.g as f32 / 255.0, b: c.b as f32 / 255.0, a: c.a as f32 / 255.0 * alpha as f32 * outer }
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

pub fn paint(cx: &mut Cx, r: Rect, style: &str, columns: usize, live: bool, key: &str) {
    let t = cx.theme;
    let ink = styles::Ink {
        groove: to_vis(t.colors.get("muted")),
        dim: to_vis(t.colors.get("mutedForeground")),
        content: to_vis(t.colors.get("foreground")),
        accent: to_vis(t.colors.get("primary")),
        mono: String::new(),
        columns,
        gap: t.space.xxs,
        radius: t.radii.sm,
    };
    let frame = if live { visualizer::frame() } else { visualizer::Frame::baseline() };
    let (now, seen) = (cx.now, cx.ui.frame);
    let st = cx.ui.spectra.entry(key.to_owned()).or_insert_with(|| Live {
        style: String::new(),
        live: false,
        state: styles::State::default(),
        shown: model::baseline_levels(),
        last: None,
        seen,
    });
    st.seen = seen;
    if st.style != style || st.live != live {
        st.style = style.to_owned();
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
    if live {
        cx.animate();
    }

    let outer = cx.alpha;
    let at = Affine::translate((r.x0, r.y0));
    let mono = cx.kit.look.mono;
    let clip = super::irect(r);
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
                p.shape_in(bounds, Paint::Shape { fill: Some((b, from_vis(*color, *alpha, outer))), strokes: Vec::new() }, clip);
            }
            Shape::Fill { path: els, color, alpha } => {
                let b = path(els, at);
                let bounds = Scene::cover(b.bounding_box(), Affine::IDENTITY, 1.0);
                p.shape_in(bounds, Paint::Shape { fill: Some((b, from_vis(*color, *alpha, outer))), strokes: Vec::new() }, clip);
            }
            Shape::Stroke { path: els, color, alpha, width } => {
                let b = path(els, at);
                let bounds = Scene::cover(b.bounding_box(), Affine::IDENTITY, width + 1.0);
                p.shape_in(bounds, Paint::Shape { fill: None, strokes: vec![(b, from_vis(*color, *alpha, outer), *width)] }, clip);
            }
            Shape::Text { x, y, align, color, alpha, .. } => {
                let Some(t) = text else { continue };
                let w = t.width as f64;
                let x = r.x0 + x - if *align == TextAlign::Center { w / 2.0 } else { 0.0 };
                let y = r.y0 + y - t.line_height() as f64 / 2.0;
                let (bw, bh) = t.box_size();
                let bounds = IRect::new(x.round() as i32 - crate::text::PAD, y.round() as i32 - crate::text::PAD, bw, bh);
                p.text_in(&t, bounds, Affine::IDENTITY, Some(clip), from_vis(*color, *alpha, outer), &[]);
            }
        }
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}
