//! The power flow diagram as one element. The nodes
//! sit in thirds of the width with a link between each pair, and under the
//! laptop a trunk drops one branch per USB-C port. A link carries a train
//! of chevrons only while power really crosses it, on the charging pulse's
//! clock, and holds still under `motion.enabled: false`.

use vello_cpu::kurbo::{Affine, Rect};

use super::super::el::{Flow, FlowNode, Font, Type, Weight};
use super::super::{Cx, Ink, irect, w};
use super::{elided, icon, paint as paint_el, resolve, shape};
use crate::scene::IRect;
use crate::text;

const CAPTION: Font = Font { size: Type::Caption, weight: Weight::Normal, mono: false };
const BODY: Font = Font { size: Type::Body, weight: Weight::Normal, mono: false };
const MONO: Font = Font { size: Type::Body, weight: Weight::Normal, mono: true };

pub struct Metrics {
    pub total: f64,
    node_h: f64,
    rows: Vec<f64>,
}

fn node_height(cx: &mut Cx, n: &FlowNode) -> f64 {
    let xxs = cx.theme.space.xxs;
    let mut h = icon(cx, &n.icon, Type::Heading).line_height() as f64;
    h += xxs + shape(cx, "A", &CAPTION).line_height() as f64;
    if !n.value.is_empty() {
        h += xxs + shape(cx, "A", &MONO).line_height() as f64;
    }
    if !n.detail.is_empty() {
        h += xxs + shape(cx, "A", &CAPTION).line_height() as f64;
    }
    h
}

pub fn metrics(cx: &mut Cx, f: &Flow, _width: f64) -> Metrics {
    let row_gap = cx.theme.space.row_gap;
    let node_h = f.nodes.iter().map(|n| node_height(cx, n)).fold(0.0, f64::max);
    let body = shape(cx, "A", &BODY).line_height() as f64;
    let cap = shape(cx, "A", &CAPTION).line_height() as f64;
    let rows: Vec<f64> = f
        .ports
        .iter()
        .map(|p| (if p.label.is_empty() { 0.0 } else { body }) + cap + row_gap * 2.0)
        .collect();
    let total = node_h + rows.iter().map(|h| row_gap + h).sum::<f64>();
    Metrics { total, node_h, rows }
}

/// One link: the hairline, and the chevrons while `direction` is not 0.
fn link(cx: &mut Cx, r: Rect, direction: i8, animate: bool, path: &str) {
    paint_el(cx, &w::separator(), Rect::new(r.x0, r.y0 + r.height() / 2.0, r.x1, r.y0 + r.height() / 2.0 + 1.0), &format!("{path}/rule"));
    if direction == 0 {
        return;
    }
    let caption = cx.theme.font_size.caption;
    let spacing = caption * 2.0;
    let name = if direction > 0 { "chevron-right" } else { "chevron-left" };
    let glyph = icon(cx, name, Type::Caption);
    let pulse = crate::motion::PULSE_MS;
    let moving = animate && cx.theme.motion_enabled;
    let phase = if moving {
        let born = cx.born(path);
        cx.animate();
        (cx.now.saturating_duration_since(born).as_secs_f64() * 1000.0 / pulse).fract()
    } else {
        0.0
    };
    let fg = cx.a(resolve(cx, Ink::Fg));
    let width = r.width();
    let count = (width / spacing).ceil() as i32 + 1;
    let clip = irect(r);
    let y = (r.y0 + (r.height() - glyph.line_height() as f64) / 2.0).round() as i32;
    let (gw, gh) = glyph.box_size();
    let mut p = cx.painter(&format!("{path}/chevrons"));
    for i in 0..count {
        let along = (i as f64 - 1.0 + phase) * spacing;
        let fade = ((along + caption) / spacing).min((width - along - caption) / spacing).clamp(0.0, 1.0);
        if fade <= 0.0 {
            continue;
        }
        let x = if direction > 0 { along } else { width - along - caption };
        let bounds = IRect::new((r.x0 + x).round() as i32 - text::PAD, y - text::PAD, gw, gh);
        let clipped = Some(p.clip.map_or(clip, |c| c.intersect(&clip)));
        p.text_in(&glyph, bounds, Affine::IDENTITY, clipped, fg.with_alpha(fg.a * fade as f32), &[]);
    }
    let last = p.last();
    p.finish();
    cx.done(last);
}

fn node(cx: &mut Cx, n: &FlowNode, x: f64, y: f64, col: f64, path: &str) -> (f64, f64) {
    let xxs = cx.theme.space.xxs;
    let glyph = icon(cx, &n.icon, Type::Heading);
    let glyph_h = glyph.line_height() as f64;
    let muted = cx.a(resolve(cx, Ink::Muted));
    let fg = cx.a(resolve(cx, Ink::Fg));
    let icon_ink = if n.dim { muted } else { fg };
    let mut lines = vec![(elided(cx, &n.caption, &CAPTION, col), muted)];
    if !n.value.is_empty() {
        lines.push((elided(cx, &n.value, &MONO, col), fg));
    }
    if !n.detail.is_empty() {
        lines.push((elided(cx, &n.detail, &CAPTION, col), muted));
    }
    let mut p = cx.painter(path);
    let gx = (x + (col - glyph.width as f64) / 2.0).round() as i32;
    p.text(&glyph, (gx, y.round() as i32), icon_ink, &[]);
    let mut at = y + glyph_h + xxs;
    for (t, ink) in &lines {
        let lx = (x + (col - t.width as f64) / 2.0).round() as i32;
        p.text(t, (lx, at.round() as i32), *ink, &[]);
        at += t.line_height() as f64 + xxs;
    }
    let last = p.last();
    p.finish();
    cx.done(last);
    (glyph.width as f64, glyph_h)
}

pub fn paint(cx: &mut Cx, r: Rect, f: &Flow, path: &str) {
    let s = cx.theme.space.clone();
    let m = metrics(cx, f, r.width());
    let col = r.width() / 3.0;
    let gap = s.icon_gap;
    let caption = cx.theme.font_size.caption;
    let icon_size = cx.theme.font_size.heading;
    let mut centre_y = r.y0;
    for (i, n) in f.nodes.iter().enumerate() {
        let (_, gh) = node(cx, n, r.x0 + col * i as f64, r.y0, col, &format!("{path}/node{i}"));
        centre_y = r.y0 + gh / 2.0;
    }
    for (i, direction) in f.links.iter().enumerate() {
        let x = r.x0 + col * (i as f64 + 0.5) + icon_size / 2.0 + gap;
        let width = col - icon_size - gap * 2.0;
        let rect = Rect::new(x, centre_y - caption / 2.0, x + width, centre_y + caption / 2.0);
        link(cx, rect, *direction, f.animate, &format!("{path}/link{i}"));
    }
    let trunk = r.x0 + r.width() / 2.0;
    let branch = s.xl + s.lg;
    let mut y = r.y0 + m.node_h + s.row_gap;
    let body = shape(cx, "A", &BODY).line_height() as f64;
    let cap = shape(cx, "A", &CAPTION).line_height() as f64;
    for (i, port) in f.ports.iter().enumerate() {
        let h = m.rows[i];
        let last = i + 1 == f.ports.len();
        let base = format!("{path}/port{i}");
        let name = elided(cx, &port.name, &CAPTION, (trunk - gap - r.x0).max(0.0));
        let muted = cx.a(resolve(cx, Ink::Muted));
        let fg = cx.a(resolve(cx, Ink::Fg));
        let text_x = trunk + branch + gap;
        let text_w = (r.x1 - text_x).max(0.0);
        let label = (!port.label.is_empty()).then(|| elided(cx, &port.label, &BODY, text_w));
        let detail = elided(cx, &port.detail, &CAPTION, text_w);
        let block = if label.is_some() { body } else { 0.0 } + cap;
        let top = y + (h - block) / 2.0;
        let mut p = cx.painter(&format!("{base}/text"));
        let nx = (trunk - gap - name.width as f64).round() as i32;
        p.text(&name, (nx, (y + (h - cap) / 2.0).round() as i32), muted, &[]);
        let mut at = top;
        if let Some(l) = &label {
            p.text(l, (text_x.round() as i32, at.round() as i32), fg, &[]);
            at += body;
        }
        p.text(&detail, (text_x.round() as i32, at.round() as i32), muted, &[]);
        let last_node = p.last();
        p.finish();
        cx.done(last_node);
        let trunk_h = if last { h / 2.0 } else { h };
        paint_el(cx, &w::separator_vertical(), Rect::new(trunk, y, trunk + 1.0, y + trunk_h), &format!("{base}/trunk"));
        let rect = Rect::new(trunk, y + h / 2.0 - caption / 2.0, trunk + branch, y + h / 2.0 + caption / 2.0);
        link(cx, rect, port.direction, f.animate, &format!("{base}/branch"));
        y += h + s.row_gap;
    }
}
