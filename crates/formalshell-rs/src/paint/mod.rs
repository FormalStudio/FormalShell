//! The drawing kit every surface paints through: the [`Painter`] that writes
//! a scene's nodes, the [`Kit`] that shapes text for a theme's [`Look`], and
//! the rim and join geometry cards are drawn with. It sits below the
//! surfaces and the ui kit, which both import it.

pub mod kit;
pub mod rim;
pub mod shoulders;

pub use kit::{Kit, Look, face};

use fs_theme::color::Rgba;
use fs_theme::style::Glow;
use vello_cpu::kurbo::Affine;

use crate::scene::{Bitmap, Fade, IRect, NodeId, Paint, Scene};
use crate::text::{self, ShapedText};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Rest,
    Destructive,
    Warning,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Rest => "rest",
            Tone::Destructive => "destructive",
            Tone::Warning => "warning",
        }
    }
}

/// The ink a cell's content resolves to, and the band's glow under it.
#[derive(Clone, Debug, PartialEq)]
pub struct Ink {
    pub fg: Rgba,
    pub dim: Rgba,
    pub glow: Vec<Glow<Rgba>>,
    /// On a band: labels go semibold.
    pub band: bool,
    pub alpha: f32,
}

impl Ink {
    pub fn resolve(look: &Look, tone: Tone, band: Option<(Rgba, &[Glow<Rgba>])>, alpha: f32) -> Self {
        let band_ink = band.filter(|(c, _)| c.a > 0.0);
        let fg = match tone {
            Tone::Destructive => look.destructive,
            Tone::Warning => look.warning,
            Tone::Rest => band_ink.map_or(look.foreground, |(c, _)| c),
        };
        let dim = if band_ink.is_some() { fg } else { look.muted };
        let glow = match (tone, band_ink) {
            (Tone::Rest, Some((_, g))) => g.to_vec(),
            _ => Vec::new(),
        };
        Self { fg, dim, glow, band: band_ink.is_some(), alpha }
    }

    pub fn of(&self, c: Rgba) -> Rgba {
        c.with_alpha(c.a * self.alpha)
    }
}

/// Writes one owner's nodes in call order, reusing what it wrote last time,
/// so an unchanged frame damages nothing. `finish` hides whatever was not
/// written this time.
pub struct Painter<'a> {
    scene: &'a mut Scene,
    nodes: &'a mut Vec<NodeId>,
    used: usize,
    pub clip: Option<IRect>,
    /// Where a node this painter has not drawn before goes in paint order:
    /// after its own last one, or after this.
    anchor: Option<NodeId>,
    /// While set, every node drawn fades out toward its view's ends.
    pub fade: Option<Fade>,
    /// Applied over every node's own transform (a zoom over a whole
    /// layout).
    pub transform: Affine,
}

impl<'a> Painter<'a> {
    pub fn new(scene: &'a mut Scene, nodes: &'a mut Vec<NodeId>, clip: Option<IRect>) -> Self {
        Self { scene, nodes, used: 0, clip, anchor: None, fade: None, transform: Affine::IDENTITY }
    }

    /// New nodes start right after `anchor` rather than on top of the scene.
    pub fn after(mut self, anchor: Option<NodeId>) -> Self {
        self.anchor = anchor;
        self
    }

    /// The last node this painter drew, for the next group to follow.
    pub fn last(&self) -> Option<NodeId> {
        self.nodes.last().copied().or(self.anchor)
    }

    pub fn scene(&mut self) -> &mut Scene {
        self.scene
    }

    fn put(&mut self, bounds: IRect, paint: Paint, transform: Affine, clip: Option<IRect>) {
        if self.used == self.nodes.len() {
            let prev = self.nodes.last().copied().or(self.anchor);
            let id = self.scene.add_after(prev, IRect::default(), Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
            self.scene.set_visible(id, false);
            self.nodes.push(id);
        }
        let id = self.nodes[self.used];
        self.used += 1;
        let paint = match self.fade {
            Some(fade) => Paint::Faded { paint: Box::new(paint), fade },
            None => paint,
        };
        self.scene.update_with(id, bounds, paint, true, self.transform * transform, clip);
    }

    pub fn rect(&mut self, r: IRect, fill: Rgba, radius: f32) {
        if fill.a > 0.0 && !r.is_empty() {
            self.put(r, Paint::Rect { fill, radius }, Affine::IDENTITY, self.clip);
        }
    }

    pub fn framed(&mut self, r: IRect, fill: Rgba, radius: f32, border: Rgba, width: f32) {
        if (fill.a > 0.0 || (border.a > 0.0 && width > 0.0)) && !r.is_empty() {
            self.put(r, Paint::Framed { fill, radius, border, width }, Affine::IDENTITY, self.clip);
        }
    }

    /// Pixels drawn with their top-left on `at`.
    pub fn image(&mut self, image: &Bitmap, at: (i32, i32), alpha: f32) {
        if alpha > 0.0 {
            let (w, h) = image.size();
            let bounds = IRect::new(at.0, at.1, w, h);
            self.put(bounds, Paint::Image { image: image.clone(), alpha }, Affine::IDENTITY, self.clip);
        }
    }

    pub fn shape(&mut self, bounds: IRect, paint: Paint) {
        self.put(bounds, paint, Affine::IDENTITY, self.clip);
    }

    /// A node under its own clip, inside the painter's.
    pub fn shape_in(&mut self, bounds: IRect, paint: Paint, clip: IRect) {
        let clip = self.clip.map_or(clip, |c| c.intersect(&clip));
        self.put(bounds, paint, Affine::IDENTITY, Some(clip));
    }

    /// A line whose line box's top-left lands on `at`.
    pub fn text(&mut self, shaped: &ShapedText, at: (i32, i32), color: Rgba, glow: &[Glow<Rgba>]) {
        let (w, h) = shaped.box_size();
        let bounds = IRect::new(at.0 - text::PAD, at.1 - text::PAD, w, h);
        self.text_in(shaped, bounds, Affine::IDENTITY, self.clip, color, glow);
    }

    /// A line drawn through `transform` (rotated, scrolled), covering
    /// `bounds` in device pixels.
    pub fn text_in(&mut self, shaped: &ShapedText, bounds: IRect, transform: Affine, clip: Option<IRect>, color: Rgba, glow: &[Glow<Rgba>]) {
        if color.a <= 0.0 || shaped.glyphs.is_empty() {
            return;
        }
        for g in glow.iter().rev() {
            let reach = (g.blur + g.x.abs().max(g.y.abs())).ceil() as i32 + 1;
            let wide = IRect::new(bounds.x - reach, bounds.y - reach, bounds.w + reach * 2, bounds.h + reach * 2);
            let shift = Affine::translate((reach as f64, reach as f64));
            let ink = g.color.with_alpha(g.color.a * color.a);
            let paint = Paint::Glow { text: shaped.clone(), color: ink, x: g.x as f32, y: g.y as f32, blur: g.blur as f32 };
            self.put(wide, paint, transform * shift, clip);
        }
        self.put(bounds, Paint::Text { text: shaped.clone(), color }, transform, clip);
    }

    /// A line under a Gaussian blur, its bounds grown by the blur's reach and
    /// cut to `clip`, under a transform about the surface's origin; it must
    /// not carry the box past its grown bounds.
    pub fn blurred_with(&mut self, shaped: &ShapedText, at: (i32, i32), color: Rgba, blur: f32, clip: Option<IRect>, about: Affine) {
        if color.a <= 0.0 || shaped.glyphs.is_empty() {
            return;
        }
        let (w, h) = shaped.box_size();
        let reach = (blur * 3.0).ceil() as i32 + 1;
        let bounds = IRect::new(at.0 - text::PAD - reach, at.1 - text::PAD - reach, w + reach * 2, h + reach * 2);
        let shift = Affine::translate((reach as f64, reach as f64));
        self.put(bounds, Paint::Blur { text: shaped.clone(), color, blur }, about * shift, clip);
    }

    pub fn finish(self) {
        for id in &self.nodes[self.used..] {
            self.scene.set_visible(*id, false);
        }
    }
}

