//! Rasterises dirty rects of the scene into one persistent premultiplied
//! RGBA canvas. Only nodes crossing a rect are encoded, and only that rect is
//! rasterised; the rest of the canvas keeps what it last held.

use std::collections::HashMap;
use std::sync::Arc;

use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Join, Rect, RoundedRect, Shape, Stroke, Vec2};
use vello_cpu::peniko::{BlendMode, Compose, Fill, Mix};
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::scene::{Bitmap, Brush, Fade, IRect, Paint, Scene, VOp};
use crate::text::{ColorGlyph, Coverage, ShapedText};
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
    /// Colour glyphs made at the canvas's pixel size, by source and size.
    colors: HashMap<(usize, u32), (Arc<ColorGlyph>, Option<(Bitmap, i32, i32)>)>,
    /// A line's glow drawn once into a pixmap of its own (a band's ink
    /// shadow is 9 taps of every glyph's outline per layer), by the glyphs,
    /// the glow, the scale and where the node's origin falls inside a
    /// canvas pixel; the entry holds the line so its glyphs' addresses are
    /// never reused under it. A scrolling marquee only moves it.
    glows: HashMap<GlowKey, (ShapedText, Arc<Pixmap>)>,
    #[cfg(test)]
    no_glow_cache: bool,
}

/// The glyphs' outline addresses and places, the glow's colour, offset and
/// blur, the scale, and the origin's eighth of a canvas pixel down and across.
type GlowKey = (u64, [u8; 4], [u32; 3], u64, (u8, u8));

/// Glows are a few lines' worth on a strip; a crossfade's every alpha
/// step is its own entry, so the table is cleared rather than kept.
const GLOWS_LIMIT: usize = 64;

const INKED_LIMIT: usize = 8192;
/// An emoji at 20 px is 1.6 KB, so a full table stays under a megabyte.
const COLORS_LIMIT: usize = 512;

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
            colors: HashMap::new(),
            glows: HashMap::new(),
            #[cfg(test)]
            no_glow_cache: false,
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
            self.colors.clear();
            self.glows.clear();
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
        let shift = scene.shift();
        if shift != 0.0 {
            origin = origin * Affine::translate((shift, 0.0));
            // The nodes landing in the rect sit `shift` short of it.
            let dx = shift.floor() as i32;
            rect = IRect::new(rect.x - dx - 1, rect.y, rect.w + 2, rect.h);
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
            let (paint, fade) = match &node.paint {
                Paint::Faded { paint, fade } => (&**paint, Some(*fade)),
                paint => (paint, None),
            };
            let spills = matches!(paint, Paint::Casts { .. } | Paint::Glow { .. });
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
            if fade.is_some() {
                self.ctx.set_transform(origin);
                self.ctx.push_layer(None, None, None, None, None);
            }
            match paint {
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
                Paint::Glow { text, color: ink, x, y, blur } if self.cached_glow(node.bounds, at, text, *ink, (*x, *y, *blur)) => {}
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
                    // The layer's transform scales the radius to the canvas.
                    self.ctx.set_transform(origin);
                    let f = vello_cpu::filter_effects::Filter::from_function(vello_cpu::filter_effects::FilterFunction::Blur { radius: *blur });
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
                                let sampler = vello_cpu::peniko::ImageSampler::default().with_quality(vello_cpu::peniko::ImageQuality::Low).with_alpha(*alpha);
                                self.image(image, at * Affine::translate((*x, *y)), sampler);
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
                            let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(ink.a);
                            let pen = at * Affine::translate((p.0 as f64, p.1 as f64));
                            match glyph.color.as_ref().filter(|_| s != 1.0).and_then(|c| self.color_glyph(c, pen)) {
                                Some((image, x, y)) => {
                                    self.ctx.set_transform(Affine::translate((x, y)));
                                    self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                                    self.ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(image.pixmap.width()), f64::from(image.pixmap.height())));
                                }
                                None => self.image(image, pen, sampler),
                            }
                        } else {
                            self.ctx.set_paint(color(*ink));
                            self.ctx.fill_path(&glyph.path);
                        }
                    }
                }
                Paint::Image { image, alpha } => {
                    let b = node.bounds;
                    let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                    self.image(image, at * Affine::translate((b.x as f64, b.y as f64)), sampler);
                }
                Paint::Picture { image, alpha, radius } => {
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    let (iw, ih) = (image.pixmap.width() as f64, image.pixmap.height() as f64);
                    // A picture made at the canvas's own size for the box
                    // goes on one pixel to a pixel, its corner on a whole one.
                    if let Some((x, y)) = (s != 1.0).then(|| fits(at, r, (iw, ih))).flatten() {
                        self.ctx.set_transform(Affine::translate((x, y)));
                        let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                        self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                        let k = at.as_coeffs()[0];
                        self.ctx.fill_path(&RoundedRect::new(0.0, 0.0, iw, ih, (f64::from(*radius) * k).max(0.0)).to_path(0.1));
                        continue;
                    }
                    self.ctx.set_transform(at);
                    self.ctx.set_paint_transform(
                        Affine::translate((r.x0, r.y0)) * Affine::scale_non_uniform(r.width() / iw.max(1.0), r.height() / ih.max(1.0)),
                    );
                    let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                    self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                    self.ctx.fill_path(&RoundedRect::from_rect(r, (*radius as f64).max(0.0)).to_path(0.1));
                    self.ctx.reset_paint_transform();
                }
                Paint::Cut { image, alpha, cut } => {
                    let b = node.bounds;
                    let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                    let (iw, ih) = (image.pixmap.width() as f64, image.pixmap.height() as f64);
                    let outline = match cut {
                        crate::scene::Cut::Corners(c) => crate::ui::boxes::chamfered(r, [f64::from(*c); 4]),
                        crate::scene::Cut::Hex => crate::ui::boxes::hexagon(r),
                    };
                    self.ctx.set_transform(at);
                    self.ctx.set_paint_transform(
                        Affine::translate((r.x0, r.y0)) * Affine::scale_non_uniform(r.width() / iw.max(1.0), r.height() / ih.max(1.0)),
                    );
                    let sampler = vello_cpu::peniko::ImageSampler::default().with_alpha(*alpha);
                    self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
                    self.ctx.fill_path(&outline);
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
                Paint::Faded { .. } => {}
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
            if let Some(fade) = fade {
                self.fade_out(origin, fade);
                self.ctx.pop_layer();
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
    /// A bitmap with its top-left at `at`'s origin, over its logical box:
    /// one of its pixels to a canvas pixel on a whole one where `at` scales
    /// by exactly its density, stretched by the difference otherwise.
    fn image(&mut self, image: &Bitmap, at: Affine, sampler: vello_cpu::peniko::ImageSampler) {
        let (w, h) = (f64::from(image.pixmap.width()), f64::from(image.pixmap.height()));
        let d = f64::from(image.density);
        match one_to_one(at, d).filter(|_| self.scale != 1.0 || d != 1.0) {
            Some((x, y)) => self.ctx.set_transform(Affine::translate((x, y))),
            None if d == 1.0 => self.ctx.set_transform(at),
            None => self.ctx.set_transform(at * Affine::scale(1.0 / d)),
        }
        self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(image.pixmap.clone()), sampler });
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, w, h));
    }

    /// A colour glyph made again at the canvas's pixel size for its pen at
    /// `pen`, and the whole canvas pixel its top-left lands on; `None` when
    /// `pen` rotates or squeezes it, or the face has nothing at that size.
    fn color_glyph(&mut self, color: &Arc<ColorGlyph>, pen: Affine) -> Option<(Bitmap, f64, f64)> {
        let [a, b, c, d, e, f] = pen.as_coeffs();
        if b != 0.0 || c != 0.0 || a != d || a <= 0.0 {
            return None;
        }
        let size = color.size * a as f32;
        let key = (Arc::as_ptr(color) as usize, size.to_bits());
        let made = match self.colors.get(&key) {
            Some((_, hit)) => hit.clone(),
            None => {
                if self.colors.len() >= COLORS_LIMIT {
                    self.colors.clear();
                }
                let made = color.at(size);
                self.colors.insert(key, (color.clone(), made.clone()));
                made
            }
        };
        let (image, dx, dy) = made?;
        // The 1x bitmap sits `dx`, `dy` off the pen; the pen goes on a
        // whole canvas pixel, as a 1x glyph's does.
        let pen = (e - f64::from(color.dx) * a, f - f64::from(color.dy) * a);
        Some((image, pen.0.round() + f64::from(dx), pen.1.round() + f64::from(dy)))
    }

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

    /// Erases the open layer toward `fade`'s ends: opaque at each end,
    /// nothing `low` or `high` px in.
    fn fade_out(&mut self, origin: Affine, fade: Fade) {
        let v = fade.view;
        let (x0, y0, x1, y1) = (f64::from(v.x), f64::from(v.y), f64::from(v.right()), f64::from(v.bottom()));
        self.ctx.set_transform(origin);
        self.ctx.push_layer(None, Some(BlendMode::new(Mix::Normal, Compose::DestOut)), None, None, None);
        let ramps = if fade.vertical {
            [(fade.low, (x0, y0), (x0, y0 + fade.low), Rect::new(x0, y0, x1, y0 + fade.low)), (fade.high, (x0, y1), (x0, y1 - fade.high), Rect::new(x0, y1 - fade.high, x1, y1))]
        } else {
            [(fade.low, (x0, y0), (x0 + fade.low, y0), Rect::new(x0, y0, x0 + fade.low, y1)), (fade.high, (x1, y0), (x1 - fade.high, y0), Rect::new(x1 - fade.high, y0, x1, y1))]
        };
        for (width, from, to, rect) in ramps {
            if width < 0.5 {
                continue;
            }
            let gradient = vello_cpu::peniko::Gradient::new_linear(from, to).with_stops([color(Rgba::BLACK), color(Rgba::TRANSPARENT)]);
            self.ctx.set_paint(gradient);
            self.ctx.fill_rect(&rect);
        }
        self.ctx.pop_layer();
    }

    /// A glow node drawn from its cached pixmap, made on a miss; false when
    /// `at` rotates or squeezes it, for the caller to fill the outlines.
    fn cached_glow(&mut self, bounds: IRect, at: Affine, text: &ShapedText, ink: Rgba, (x, y, blur): (f32, f32, f32)) -> bool {
        // The pixmap holds the whole line and its reach, not the node's
        // bounds: a marquee's node is cut to its viewport, and the same
        // line is drawn again at every scroll with other bounds.
        let reach = (blur / 2.0).max(0.0);
        let margin = (x.abs() + y.abs() + reach).ceil() as i32 + 2;
        let (tw, th) = text.box_size();
        let origin = at * Affine::translate((f64::from(bounds.x - margin), f64::from(bounds.y - margin)));
        let [a, b, c, d, e, f] = origin.as_coeffs();
        if b != 0.0 || c != 0.0 || a != d || a <= 0.0 || bounds.is_empty() {
            return false;
        }
        #[cfg(test)]
        if self.no_glow_cache {
            return false;
        }
        let eighth = |v: f64| {
            let q = ((v - v.floor()) * 8.0).round();
            if q >= 8.0 { (v.floor() + 1.0, 0u8) } else { (v.floor(), q as u8) }
        };
        let ((px, qx), (py, qy)) = (eighth(e), eighth(f));
        let glyphs = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for g in &text.glyphs {
                (Arc::as_ptr(&g.path) as usize, g.x, g.y).hash(&mut h);
            }
            h.finish()
        };
        let key = (glyphs, ink.to_u8(), [x.to_bits(), y.to_bits(), blur.to_bits()], a.to_bits(), (qx, qy));
        let pixmap = match self.glows.get(&key) {
            Some((_, p)) => p.clone(),
            None => {
                if self.glows.len() >= GLOWS_LIMIT {
                    self.glows.clear();
                }
                let w = (f64::from(tw + margin * 2) * a).ceil() as u16 + 2;
                let h = (f64::from(th + margin * 2) * a).ceil() as u16 + 2;
                let mut ctx = RenderContext::new(w, h);
                let taps: &[(f32, f32)] = if reach < 0.25 {
                    &[(0.0, 0.0)]
                } else {
                    &[(0.0, 0.0), (-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-0.7, -0.7), (0.7, -0.7), (-0.7, 0.7), (0.7, 0.7)]
                };
                let share = ink.with_alpha(ink.a / taps.len() as f32 * if taps.len() > 1 { 2.0 } else { 1.0 });
                ctx.set_paint(color(share));
                let base = Affine::translate((f64::from(qx) / 8.0, f64::from(qy) / 8.0)) * Affine::scale(a) * Affine::translate((f64::from(margin), f64::from(margin)));
                for (dx, dy) in taps {
                    for glyph in &text.glyphs {
                        let p = (glyph.x as f32 + x + dx * reach, glyph.y as f32 + y + dy * reach);
                        ctx.set_transform(base * Affine::translate((f64::from(p.0), f64::from(p.1))));
                        ctx.fill_path(&glyph.path);
                    }
                }
                ctx.flush();
                let mut pixmap = Pixmap::new(w, h);
                ctx.render(&mut pixmap, &mut self.resources);
                let pixmap = Arc::new(pixmap);
                self.glows.insert(key, (text.clone(), pixmap.clone()));
                pixmap
            }
        };
        let (w, h) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
        self.ctx.set_transform(Affine::translate((px, py)));
        self.ctx.set_paint(vello_cpu::Image { image: vello_cpu::ImageSource::Pixmap(pixmap), sampler: vello_cpu::peniko::ImageSampler::default() });
        self.ctx.fill_rect(&Rect::new(0.0, 0.0, w, h));
        true
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

/// Where `at` puts a bitmap's top-left when it maps one bitmap pixel at
/// `density` to one canvas pixel, rounded to the whole pixel.
fn one_to_one(at: Affine, density: f64) -> Option<(f64, f64)> {
    let [a, b, c, d, e, f] = at.as_coeffs();
    (b == 0.0 && c == 0.0 && a == d && (a - density).abs() < 1e-3).then(|| (e.round(), f.round()))
}

/// Where `rect`'s top-left lands on the canvas under `at`, rounded, when a
/// picture `size` pixels big is `rect` at `at`'s scale give or take the
/// rounding of a scaled length.
fn fits(at: Affine, rect: Rect, size: (f64, f64)) -> Option<(f64, f64)> {
    let [a, b, c, d, e, f] = at.as_coeffs();
    if b != 0.0 || c != 0.0 || a != d || a <= 0.0 {
        return None;
    }
    let near = |logical: f64, px: f64| (logical * a - px).abs() <= 1.0;
    (near(rect.width(), size.0) && near(rect.height(), size.1)).then(|| ((rect.x0 * a + e).round(), (rect.y0 * a + f).round()))
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
    use super::{Renderer, coverage_lift, fits, one_to_one};
    use crate::scene::{IRect, Paint, Scene};
    use fs_theme::color::Rgba;
    use vello_cpu::kurbo::{Affine, Rect};

    /// A glow drawn from its cached pixmap lands within a step of the
    /// outlines filled in place, at 1x and at a fractional scale, and a
    /// scroll by whole pixels reuses the pixmap.
    #[test]
    fn a_cached_glow_draws_what_the_outlines_do() {
        let mut text = crate::text::Text::new();
        let style = crate::text::TextStyle {
            family: crate::text::Family::Generic(parley::GenericFamily::SansSerif),
            size: 16.0,
            weight: 600.0,
            tracking: 0.0,
            wdth: 100.0,
            wght: None,
        };
        let shaped = text.shape("Hide & Seek, But We STOLE", style);
        assert!(!shaped.glyphs.is_empty());
        for scale in [1.0, 1.25] {
            let draw = |cache: bool, shift: f64| {
                let mut scene = Scene::new(400, 40);
                let (w, h) = shaped.box_size();
                let paint = Paint::Glow { text: shaped.clone(), color: Rgba::BLACK.with_alpha(0.6), x: 0.0, y: 1.0, blur: 2.0 };
                let id = scene.add(IRect::new(20, 5, w + 6, h + 6), paint);
                scene.set_transform(id, Affine::translate((shift + 3.0, 3.0)));
                let mut r = Renderer::new(400, 40);
                r.set_scale(scale);
                r.no_glow_cache = !cache;
                r.render(&scene, IRect::new(0, 0, 400, 40));
                (r.canvas().data_as_u8_slice().to_vec(), r.glows.len())
            };
            for shift in [0.0, 7.0 / scale] {
                let (direct, _) = draw(false, shift);
                let (cached, entries) = draw(true, shift);
                assert_eq!(entries, 1);
                let worst = direct.iter().zip(&cached).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
                assert!(worst <= 2, "scale {scale} shift {shift}: a channel off by {worst}");
            }
            // A marquee: the node cut to a viewport narrower than the line
            // and the line scrolled under it, drawn first at a narrower cut
            // so a pixmap sized to the bounds would be reused short.
            let (w, h) = shaped.box_size();
            let marquee = |r: &mut Renderer, cut: i32, scroll: f64| {
                let mut scene = Scene::new(400, 40);
                let paint = Paint::Glow { text: shaped.clone(), color: Rgba::BLACK.with_alpha(0.6), x: 0.0, y: 1.0, blur: 2.0 };
                let (bounds, clip) = (IRect::new(20, 5, cut, h + 6), IRect::new(20, 0, cut, 40));
                let id = scene.add(bounds, Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
                scene.update_with(id, bounds, paint, true, Affine::translate((3.0 - scroll, 3.0)), Some(clip));
                r.render(&scene, IRect::new(0, 0, 400, 40));
                r.canvas().data_as_u8_slice().to_vec()
            };
            // Half a pixel, on the cache's eighth-pixel grid at 1x and 1.25x.
            let (cut, scroll) = (w / 2, f64::from(w / 3) + 0.5);
            let mut direct = Renderer::new(400, 40);
            direct.set_scale(scale);
            direct.no_glow_cache = true;
            let want = marquee(&mut direct, cut, scroll);
            let mut cached = Renderer::new(400, 40);
            cached.set_scale(scale);
            marquee(&mut cached, cut / 3, scroll);
            let got = marquee(&mut cached, cut, scroll);
            assert!(want.iter().any(|v| *v > 0), "the marquee drew nothing");
            let worst = want.iter().zip(&got).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
            assert!(worst <= 2, "scale {scale} marquee: a channel off by {worst}");
        }
    }

    #[test]
    fn a_picture_made_at_the_canvas_scale_lands_on_whole_pixels() {
        let at = Affine::scale(1.25).then_translate((10.0, 4.0).into()) * Affine::translate((3.1, 2.0));
        assert_eq!(one_to_one(at, 1.25), Some((14.0, 7.0)));
        assert_eq!(one_to_one(at, 1.0), None);
        assert_eq!(one_to_one(Affine::rotate(0.1), 1.0), None);
    }

    #[test]
    fn a_thumbnail_fits_its_box_give_or_take_a_rounded_pixel() {
        let rect = Rect::new(10.0, 10.0, 110.0, 66.0);
        assert_eq!(fits(Affine::scale(1.25), rect, (125.0, 70.0)), Some((13.0, 13.0)));
        assert_eq!(fits(Affine::scale(1.25), rect, (124.0, 71.0)), Some((13.0, 13.0)));
        assert_eq!(fits(Affine::scale(1.25), rect, (100.0, 56.0)), None);
    }

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
