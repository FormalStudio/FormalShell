//! Rasterises dirty rects of the scene into one persistent premultiplied
//! RGBA canvas. Only nodes crossing a rect are encoded, and only that rect is
//! rasterised; the rest of the canvas keeps what it last held.

use std::collections::HashMap;
use std::sync::Arc;

use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Join, Rect, RoundedRect, Shape, Stroke, Vec2};
use vello_cpu::peniko::{BlendMode, Compose, Fill, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::scene::{Bitmap, Brush, IRect, Paint, Scene, VOp};
use crate::text::Coverage;
use fs_theme::color::Rgba;

pub struct Renderer {
    ctx: RenderContext,
    resources: Resources,
    /// `size` times `scale`, rounded: the scene stays in logical pixels and
    /// only the canvas is in the output's own.
    canvas: Pixmap,
    size: (u16, u16),
    scale: f64,
    /// Glyph coverage tinted for an ink, by the coverage's address; the
    /// entry holds the coverage so the address is never reused under it.
    inked: HashMap<(usize, [u8; 3]), (Arc<Coverage>, Arc<Pixmap>)>,
    /// Glyph outlines rasterised at `scale`, by the outline's address, the
    /// scale and the origin's quarter pixel across.
    scaled: HashMap<(usize, u64, u8), (Arc<BezPath>, Option<Arc<Coverage>>)>,
}

const INKED_LIMIT: usize = 8192;

/// A logical length at `scale`, rounded half away from zero the way
/// wp_fractional_scale_v1 asks a buffer to be.
pub fn scaled(n: i32, scale: f64) -> i32 {
    if scale == 1.0 { n } else { (f64::from(n) * scale).round() as i32 }
}

impl Renderer {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            ctx: RenderContext::new(1, 1),
            resources: Resources::new(),
            canvas: Pixmap::new(width, height),
            size: (width, height),
            scale: 1.0,
            inked: HashMap::new(),
            scaled: HashMap::new(),
        }
    }

    pub fn resize(&mut self, width: u16, height: u16) {
        self.size = (width, height);
        let (w, h) = (scaled(i32::from(width), self.scale), scaled(i32::from(height), self.scale));
        self.canvas = Pixmap::new(w.clamp(1, i32::from(u16::MAX)) as u16, h.clamp(1, i32::from(u16::MAX)) as u16);
    }

    pub fn set_scale(&mut self, scale: f64) {
        if scale != self.scale {
            self.scale = scale;
            self.scaled.clear();
            self.resize(self.size.0, self.size.1);
        }
    }

    pub fn scale(&self) -> f64 {
        self.scale
    }

    /// The logical size the canvas stands for.
    pub fn width(&self) -> u16 {
        self.size.0
    }

    pub fn height(&self) -> u16 {
        self.size.1
    }

    /// The canvas in its own pixels.
    pub fn pixels(&self) -> (i32, i32) {
        (i32::from(self.canvas.width()), i32::from(self.canvas.height()))
    }

    /// The canvas pixels a logical rect touches.
    pub fn device(&self, rect: IRect) -> IRect {
        if self.scale == 1.0 {
            return rect;
        }
        let s = self.scale;
        let (x0, y0) = ((f64::from(rect.x) * s).floor() as i32, (f64::from(rect.y) * s).floor() as i32);
        let (x1, y1) = ((f64::from(rect.right()) * s).ceil() as i32, (f64::from(rect.bottom()) * s).ceil() as i32);
        let (w, h) = self.pixels();
        IRect::new(x0, y0, x1 - x0, y1 - y0).intersect(&IRect::new(0, 0, w, h))
    }

    pub fn canvas(&self) -> &Pixmap {
        &self.canvas
    }

    pub fn canvas_bytes_mut(&mut self) -> &mut [u8] {
        self.canvas.data_as_u8_slice_mut()
    }

    /// `rect` back to transparent, without drawing anything.
    pub fn clear(&mut self, rect: IRect) {
        let size = IRect::new(0, 0, self.width() as i32, self.height() as i32);
        let rect = self.device(rect.intersect(&size));
        if rect.is_empty() {
            return;
        }
        let stride = self.canvas.width() as usize * 4;
        let bytes = self.canvas.data_as_u8_slice_mut();
        for row in rect.y..rect.bottom() {
            let start = row as usize * stride + rect.x as usize * 4;
            bytes[start..start + rect.w as usize * 4].fill(0);
        }
    }

    pub fn render(&mut self, scene: &Scene, rect: IRect) {
        let mut rect = rect.intersect(&scene.size);
        if rect.is_empty() {
            return;
        }
        let device = self.device(rect);
        if device.is_empty() {
            return;
        }
        let s = self.scale;
        let mut origin = Affine::translate((-device.x as f64, -device.y as f64));
        if s != 1.0 {
            origin = origin * Affine::scale(s);
            // Every node reaching a canvas pixel the rect only partly covers
            // is drawn into it, or that pixel loses whatever it misses.
            let (x0, y0) = ((f64::from(device.x) / s).floor() as i32, (f64::from(device.y) / s).floor() as i32);
            let (x1, y1) = ((f64::from(device.right()) / s).ceil() as i32, (f64::from(device.bottom()) / s).ceil() as i32);
            rect = IRect::new(x0, y0, x1 - x0, y1 - y0);
        }
        let (w, h) = (device.w as u16, device.h as u16);
        self.ctx.reset_and_resize(w, h);
        // A clip is a layer composited on its own, so a run of nodes under
        // one clip shares a layer, and a node the clip cannot cut (its box
        // inside the clip, moved by a plain translation at most) needs none.
        // A launcher frame is a few hundred rows under the body's one clip.
        let mut active: Option<IRect> = None;
        for node in scene.nodes().filter(|n| n.bounds.intersects(&rect)) {
            // Casts and glows paint past their own box.
            let spills = matches!(node.paint, Paint::Casts { .. } | Paint::Glow { .. });
            let drawn = drawn_box(node.bounds, node.transform).filter(|_| !spills);
            let inside = |c: &IRect| drawn.is_some_and(|d| c.contains(&d));
            let want = node.clip.filter(|c| !inside(c));
            match want {
                Some(c) if active != Some(c) => {
                    if active.is_some() {
                        self.ctx.pop_layer();
                    }
                    self.ctx.set_transform(origin);
                    let r = Rect::new(c.x as f64, c.y as f64, c.right() as f64, c.bottom() as f64);
                    self.ctx.push_clip_layer(&r.to_path(0.1));
                    active = Some(c);
                }
                None if active.is_some_and(|a| !inside(&a)) => {
                    self.ctx.pop_layer();
                    active = None;
                }
                _ => {}
            }
            let at = origin * node.transform;
            match &node.paint {
                Paint::Rect { fill, radius } => {
                    self.ctx.set_transform(at);
                    self.ctx.set_paint(color(*fill));
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    if *radius > 0.0 {
                        self.ctx.fill_path(&RoundedRect::from_rect(r, *radius as f64).to_path(0.1));
                    } else {
                        self.ctx.fill_rect(&r);
                    }
                }
                Paint::Framed { fill, radius, border, width } => {
                    self.ctx.set_transform(at);
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    let rr = |r: Rect, radius: f64| RoundedRect::from_rect(r, radius.max(0.0)).to_path(0.1);
                    if fill.to_u8()[3] > 0 {
                        self.ctx.set_paint(color(*fill));
                        self.ctx.fill_path(&rr(r, *radius as f64));
                    }
                    if *width > 0.0 && border.to_u8()[3] > 0 {
                        let half = *width as f64 / 2.0;
                        self.ctx.set_stroke(Stroke::new(*width as f64));
                        self.ctx.set_paint(color(*border));
                        self.ctx.stroke_path(&rr(r.inflate(-half, -half), *radius as f64 - half));
                    }
                }
                Paint::Face { from, to, radius } => {
                    self.ctx.set_transform(at);
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    let gradient = vello_cpu::peniko::Gradient::new_linear((r.x0, r.y0), (r.x0, r.y1))
                        .with_stops([color(*from), color(*to)]);
                    self.ctx.set_paint(gradient);
                    self.ctx.fill_path(&RoundedRect::from_rect(r, (*radius as f64).max(0.0)).to_path(0.1));
                }
                Paint::Glow { text, color: ink, x, y, blur } => {
                    // A box of taps over the blur's reach, each carrying its
                    // share of the alpha: cheap, and close enough at the 2px
                    // the tables ask for.
                    let reach = (*blur / 2.0).max(0.0);
                    let taps: &[(f32, f32)] = if reach < 0.25 {
                        &[(0.0, 0.0)]
                    } else {
                        &[(0.0, 0.0), (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)]
                    };
                    let share = ink.with_alpha(ink.a / taps.len() as f32 * if taps.len() > 1 { 2.0 } else { 1.0 });
                    self.ctx.set_paint(color(share));
                    for (dx, dy) in taps {
                        for glyph in &text.glyphs {
                            let p = (
                                node.bounds.x as f32 + glyph.x as f32 + x + dx * reach,
                                node.bounds.y as f32 + glyph.y as f32 + y + dy * reach,
                            );
                            self.ctx.set_transform(at * Affine::translate((p.0 as f64, p.1 as f64)));
                            self.ctx.fill_path(&glyph.path);
                        }
                    }
                }
                Paint::Blur { text, color: ink, blur } => {
                    self.ctx.set_transform(origin);
                    let radius = *blur * s as f32;
                    let f = vello_cpu::filter_effects::Filter::from_function(vello_cpu::filter_effects::FilterFunction::Blur { radius });
                    self.ctx.push_filter_layer(f);
                    self.ctx.set_paint(color(*ink));
                    for glyph in &text.glyphs {
                        let p = (node.bounds.x + glyph.x, node.bounds.y + glyph.y);
                        self.ctx.set_transform(at * Affine::translate((p.0 as f64, p.1 as f64)));
                        self.ctx.fill_path(&glyph.path);
                    }
                    self.ctx.pop_layer();
                }
                Paint::Vector(ops) => {
                    self.ctx.set_transform(at);
                    let mut depth = 0;
                    for op in ops.iter() {
                        match op {
                            VOp::Fill(path, brush) => {
                                self.set_brush(brush);
                                self.ctx.fill_path(path);
                            }
                            VOp::Stroke(path, brush, width) => {
                                self.set_brush(brush);
                                self.ctx.set_stroke(Stroke::new(*width));
                                self.ctx.stroke_path(path);
                            }
                            VOp::Clip(path) => {
                                self.ctx.push_clip_layer(path);
                                depth += 1;
                            }
                            VOp::Pop => {
                                if depth > 0 {
                                    self.ctx.pop_layer();
                                    depth -= 1;
                                }
                            }
                            VOp::Image(image, (x, y), alpha) => {
                                self.ctx.set_transform(at * Affine::translate((*x, *y)));
                                let sampler = vello_cpu::peniko::ImageSampler::default().with_quality(vello_cpu::peniko::ImageQuality::Low).with_alpha(*alpha);
                                self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                                let (w, h) = (image.pixmap.width() as f64, image.pixmap.height() as f64);
                                self.ctx.fill_rect(&Rect::new(0.0, 0.0, w, h));
                                self.ctx.set_transform(at);
                            }
                        }
                    }
                    for _ in 0..depth {
                        self.ctx.pop_layer();
                    }
                }
                Paint::Cells { rects, glyphs } => {
                    self.ctx.set_transform(at);
                    for (r, ink) in rects {
                        self.ctx.set_paint(color(*ink));
                        self.ctx.fill_rect(r);
                    }
                    for (path, x, y, ink) in glyphs {
                        self.ctx.set_paint(color(*ink));
                        self.ctx.set_transform(at * Affine::translate((*x, *y)));
                        self.ctx.fill_path(path);
                    }
                }
                Paint::Text { text, color: ink } => {
                    for glyph in &text.glyphs {
                        let p = (node.bounds.x + glyph.x, node.bounds.y + glyph.y);
                        self.ctx.set_transform(at * Affine::translate((p.0 as f64, p.1 as f64)));
                        if s != 1.0 && glyph.image.is_none() && glyph.coverage.is_some() {
                            self.scaled_glyph(&glyph.path, at * Affine::translate((p.0 as f64, p.1 as f64)), *ink);
                        } else if let (None, Some(coverage)) = (&glyph.image, &glyph.coverage) {
                            let pixmap = self.inked(coverage, *ink);
                            let shift = Affine::translate(((p.0 + coverage.x) as f64, (p.1 + coverage.y) as f64));
                            self.ctx.set_transform(at * shift);
                            self.ctx.set_paint(vello_cpu::Image {
                                image: vello_cpu::ImageSource::Pixmap(pixmap),
                                sampler: vello_cpu::peniko::ImageSampler::default().with_alpha(ink.a),
                            });
                            self.ctx.fill_rect(&Rect::new(0.0, 0.0, coverage.w as f64, coverage.h as f64));
                        } else if let Some(image) = &glyph.image {
                            // A colour glyph keeps its own colours and takes only the ink's alpha.
                            self.ctx.set_paint(vello_cpu::Image {
                                image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()),
                                sampler: vello_cpu::peniko::ImageSampler::default().with_alpha(ink.a),
                            });
                            self.ctx.fill_rect(&Rect::new(0.0, 0.0, image.pixmap.width() as f64, image.pixmap.height() as f64));
                        } else {
                            self.ctx.set_paint(color(*ink));
                            self.ctx.fill_path(&glyph.path);
                        }
                    }
                }
                Paint::Image { image, alpha } => {
                    let b = node.bounds;
                    self.ctx.set_transform(at * Affine::translate((b.x as f64, b.y as f64)));
                    let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                    self.ctx.set_paint(vello_cpu::Image {
                        image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()),
                        sampler,
                    });
                    let (w, h) = (image.pixmap.width() as f64, image.pixmap.height() as f64);
                    self.ctx.fill_rect(&Rect::new(0.0, 0.0, w, h));
                }
                Paint::Picture { image, alpha, radius } => {
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    let (iw, ih) = (image.pixmap.width() as f64, image.pixmap.height() as f64);
                    self.ctx.set_transform(at);
                    self.ctx.set_paint_transform(
                        Affine::translate((r.x0, r.y0)) * Affine::scale_non_uniform(r.width() / iw.max(1.0), r.height() / ih.max(1.0)),
                    );
                    let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                    self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                    self.ctx.fill_path(&RoundedRect::from_rect(r, (*radius as f64).max(0.0)).to_path(0.1));
                    self.ctx.reset_paint_transform();
                }
                Paint::Shape { fill, strokes } => {
                    self.ctx.set_transform(at);
                    if let Some((path, ink)) = fill {
                        // Even-odd, so a band carries its cut-out as a second subpath.
                        self.ctx.set_fill_rule(Fill::EvenOdd);
                        self.ctx.set_paint(color(*ink));
                        self.ctx.fill_path(path);
                        self.ctx.set_fill_rule(Fill::NonZero);
                    }
                    for (path, ink, width) in strokes {
                        if ink.to_u8()[3] == 0 {
                            continue;
                        }
                        // ShapePath's own defaults.
                        self.ctx.set_stroke(Stroke::new(*width).with_caps(Cap::Square).with_join(Join::Bevel));
                        self.ctx.set_paint(color(*ink));
                        self.ctx.stroke_path(path);
                    }
                }
                Paint::Casts { rect: card, radius, layers, cutout } => {
                    self.ctx.set_transform(at);
                    self.ctx.push_layer(None, None, None, None, None);
                    for cast in layers {
                        let shrink = (-cast.spread).max(0.0);
                        let grow = cast.spread.max(0.0);
                        let r = card.inflate(grow - shrink, grow - shrink) + Vec2::new(cast.x, cast.y);
                        self.ctx.set_paint(color(cast.color));
                        // RectangularShadow's blur is CSS's blur radius, twice the deviation.
                        self.ctx.fill_blurred_rounded_rect(&r, (radius - shrink).max(0.0) as f32, (cast.blur / 2.0) as f32, false);
                    }
                    self.ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
                    self.ctx.set_paint(color(Rgba::hex(0)));
                    self.ctx.fill_path(cutout);
                    self.ctx.pop_layer();
                    self.ctx.pop_layer();
                }
            }
        }
        if active.is_some() {
            self.ctx.pop_layer();
        }
        self.ctx.flush();
        let mut tile = Pixmap::new(w, h);
        self.ctx.render(&mut tile, &mut self.resources);

        let stride = self.canvas.width() as usize;
        let src = tile.data();
        let dst = self.canvas.data_mut();
        for row in 0..device.h as usize {
            let d = (device.y as usize + row) * stride + device.x as usize;
            let s = row * device.w as usize;
            dst[d..d + device.w as usize].copy_from_slice(&src[s..s + device.w as usize]);
        }
    }
}

impl Renderer {
    /// A glyph at a scale past 1, rasterised at that scale rather than its
    /// 1x coverage stretched: its origin on a whole canvas pixel down and a
    /// quarter one across, the way the 1x coverage sits on whole pixels.
    /// A transform that rotates or squeezes it fills the outline instead.
    fn scaled_glyph(&mut self, path: &Arc<BezPath>, at: Affine, ink: Rgba) {
        let [a, b, c, d, e, f] = at.as_coeffs();
        if b != 0.0 || c != 0.0 || a != d || a <= 0.0 {
            self.ctx.set_paint(color(ink));
            self.ctx.fill_path(path);
            return;
        }
        let quarter = ((e - e.floor()) * 4.0).round();
        let (x, across) = if quarter >= 4.0 { (e.floor() + 1.0, 0) } else { (e.floor(), quarter as u8) };
        let key = (Arc::as_ptr(path) as usize, a.to_bits(), across);
        let coverage = match self.scaled.get(&key) {
            Some((_, c)) => c.clone(),
            None => {
                if self.scaled.len() >= INKED_LIMIT {
                    self.scaled.clear();
                }
                let c = Coverage::scaled(path, a, f64::from(across) / 4.0).map(Arc::new);
                self.scaled.insert(key, (path.clone(), c.clone()));
                c
            }
        };
        let Some(coverage) = coverage else { return };
        let pixmap = self.inked(&coverage, ink);
        self.ctx.set_transform(Affine::translate((x + f64::from(coverage.x), f.round() + f64::from(coverage.y))));
        self.ctx.set_paint(vello_cpu::Image {
            image: vello_cpu::ImageSource::Pixmap(pixmap),
            sampler: vello_cpu::peniko::ImageSampler::default().with_alpha(ink.a),
        });
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(coverage.w), f64::from(coverage.h)));
    }

    /// `coverage` in `ink`'s colour, opaque: the ink's alpha is the
    /// sampler's, so a fade reuses the same pixmap.
    fn inked(&mut self, coverage: &Arc<Coverage>, ink: Rgba) -> Arc<Pixmap> {
        let [r, g, b, _] = ink.to_u8();
        let key = (Arc::as_ptr(coverage) as usize, [r, g, b]);
        if let Some((_, pixmap)) = self.inked.get(&key) {
            return pixmap.clone();
        }
        if self.inked.len() >= INKED_LIMIT {
            self.inked.clear();
        }
        let lift = coverage_lift([r, g, b]);
        let mut px = Vec::with_capacity(coverage.alpha.len() * 4);
        for a in &coverage.alpha {
            let a = u32::from(lift[*a as usize]);
            for c in [r, g, b] {
                px.push(((u32::from(c) * a + 127) / 255) as u8);
            }
            px.push(a as u8);
        }
        let pixmap = Bitmap::from_premultiplied(coverage.w, coverage.h, px).pixmap;
        self.inked.insert(key, (coverage.clone(), pixmap.clone()));
        pixmap
    }

    fn set_brush(&mut self, brush: &Brush) {
        match brush {
            Brush::Solid(c) => self.ctx.set_paint(color(*c)),
            Brush::Radial { centre, r0, r1, stops } => {
                let stops: Vec<(f32, AlphaColor<Srgb>)> = stops.iter().map(|(o, c)| (*o, color(*c))).collect();
                let g = vello_cpu::peniko::Gradient::new_two_point_radial(*centre, *r0 as f32, *centre, *r1 as f32).with_stops(stops.as_slice());
                self.ctx.set_paint(g);
            }
        }
    }
}

/// A node's box where it lands, when its transform is a translation (or
/// nothing); `None` for anything that scales, rotates or skews it.
fn drawn_box(bounds: IRect, transform: Affine) -> Option<IRect> {
    let [a, b, c, d, e, f] = transform.as_coeffs();
    if a != 1.0 || b != 0.0 || c != 0.0 || d != 1.0 {
        return None;
    }
    let (dx, dy) = (e.floor() as i32, f.floor() as i32);
    let (ex, ey) = (i32::from(e.fract() != 0.0), i32::from(f.fract() != 0.0));
    Some(IRect::new(bounds.x + dx, bounds.y + dy, bounds.w + ex, bounds.h + ey))
}

/// The coverage curve for an ink. vello_cpu blends in sRGB-encoded values,
/// as cairo does, so a pixel half covered by light ink on a dark card gives
/// off far less than half the light, and the antialiased edges of light
/// text fall away: stems read a pixel thin. The edges are lifted by a gamma
/// that grows with how light the ink looks (its luma), up to 1.8 for white,
/// the value macOS and Skia settle on, so muted words on a dark card keep
/// their weight too; black ink, which that blend already renders full, keeps
/// its coverage as drawn. A fully covered pixel stays fully covered either
/// way, so hinted stems stay as sharp.
fn coverage_lift([r, g, b]: [u8; 3]) -> [u8; 256] {
    let luma = (0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b)) / 255.0;
    let exponent = 1.0 / (1.0 + 0.8 * luma);
    std::array::from_fn(|a| ((a as f32 / 255.0).powf(exponent) * 255.0).round() as u8)
}

fn color(c: Rgba) -> AlphaColor<Srgb> {
    let [r, g, b, a] = c.to_u8();
    AlphaColor::from_rgba8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use super::coverage_lift;

    #[test]
    fn light_ink_lifts_the_edges_and_dark_ink_keeps_them() {
        let white = coverage_lift([255, 255, 255]);
        let black = coverage_lift([0, 0, 0]);
        assert_eq!((white[0], white[255]), (0, 255));
        assert!(white[64] > 100, "a quarter covered pixel under white ink: {}", white[64]);
        assert!((0..256).all(|a| black[a] as usize == a));
        assert!(white.windows(2).all(|w| w[0] <= w[1]));
    }
}
