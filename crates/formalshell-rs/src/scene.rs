//! A retained scene: nodes in paint order, each with the device-pixel bounds
//! it covers. Every change marks the old and the new bounds dirty, and the
//! renderer redraws only the dirty rects.

use std::sync::Arc;

use vello_cpu::kurbo::{Affine, BezPath, Rect, Shape};
use vello_cpu::{PixelMetadata, Pixmap};

use crate::text::ShapedText;
use fs_theme::color::Rgba;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    pub fn to_json(self) -> serde_json::Value {
        serde_json::json!({"x": self.x, "y": self.y, "width": self.w, "height": self.h})
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

/// One of a Box's casts: CSS's offset, blur radius
/// and spread, drawn analytically.
#[derive(Clone, Copy, Debug)]
pub struct Cast {
    pub x: f64,
    pub y: f64,
    pub blur: f64,
    pub spread: f64,
    pub color: Rgba,
}

/// A decoded picture ready to draw at its own size (a tray icon, a menu row's
/// icon): premultiplied, shared between the nodes that show it. `density`
/// is its pixels to a logical pixel: a picture made for a surface at 1.25
/// is 1.25 times its logical box, and maps 1:1 onto that surface's buffer.
#[derive(Clone)]
pub struct Bitmap {
    pub pixmap: Arc<Pixmap>,
    pub density: f32,
}

impl Bitmap {
    /// `rgba` is straight alpha, `width * height * 4` bytes.
    pub fn from_rgba(width: u16, height: u16, mut rgba: Vec<u8>) -> Self {
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3] as u32;
            for c in &mut px[..3] {
                *c = ((*c as u32 * a + 127) / 255) as u8;
            }
        }
        Self::from_premultiplied(width, height, rgba)
    }

    /// `rgba` already premultiplied, `width * height * 4` bytes.
    pub fn from_premultiplied(width: u16, height: u16, rgba: Vec<u8>) -> Self {
        Self { pixmap: Arc::new(Pixmap::from_parts(rgba, width, height, PixelMetadata::default())), density: 1.0 }
    }

    /// The same pixels made for a surface at `scale`.
    pub fn at(mut self, scale: f64) -> Self {
        self.density = scale as f32;
        self
    }

    /// The box it covers in logical pixels.
    pub fn logical(&self) -> (f64, f64) {
        let d = f64::from(self.density);
        (f64::from(self.pixmap.width()) / d, f64::from(self.pixmap.height()) / d)
    }

    /// [`Bitmap::logical`] in whole pixels.
    pub fn size(&self) -> (i32, i32) {
        let (w, h) = self.logical();
        (w.round() as i32, h.round() as i32)
    }

    pub fn same_as(&self, other: &Bitmap) -> bool {
        Arc::ptr_eq(&self.pixmap, &other.pixmap) && self.density == other.density
    }
}

/// Of the same picture made at several scales, the one made at `scale`,
/// else the nearest above it, else the largest.
pub fn nearest(made: &[Bitmap], scale: f64) -> Option<&Bitmap> {
    let at = |b: &Bitmap| f64::from(b.density);
    made.iter()
        .filter(|b| at(b) >= scale - 1e-3)
        .min_by(|a, b| at(a).total_cmp(&at(b)))
        .or_else(|| made.iter().max_by(|a, b| at(a).total_cmp(&at(b))))
}

/// `made` with `b` in it, in place of one at the same density.
pub fn keep(made: &mut Vec<Bitmap>, b: Bitmap) {
    made.retain(|m| m.density != b.density);
    made.push(b);
}

/// A logical length as the pixels a picture of it is made with at `scale`.
pub fn pixels_at(logical: f64, scale: f64) -> u32 {
    (logical * scale).round().max(1.0) as u32
}

impl PartialEq for Bitmap {
    fn eq(&self, other: &Self) -> bool {
        self.same_as(other)
    }
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bitmap({}x{}@{})", self.pixmap.width(), self.pixmap.height(), self.density)
    }
}

/// An alpha ramp at either end of `view` along its axis: `low` px of it at
/// the start (left or top), `high` px at the end, none at 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub view: IRect,
    pub vertical: bool,
    pub low: f64,
    pub high: f64,
}

impl Cast {
    fn same(&self, o: &Cast) -> bool {
        (self.x, self.y, self.blur, self.spread) == (o.x, o.y, o.blur, o.spread) && self.color == o.color
    }
}

pub enum Paint {
    Rect { fill: Rgba, radius: f32 },
    /// Another paint faded out toward the ends of `fade`'s view (a
    /// marquee's cut edges), in a layer of its own so nothing under it fades.
    Faded { paint: Box<Paint>, fade: Fade },
    /// A bitmap drawn at the node's bounds origin, one pixel to a pixel.
    Image { image: Bitmap, alpha: f32 },
    /// A bitmap stretched over the node's bounds inside a rounded rect (a
    /// window thumbnail).
    Picture { image: Bitmap, alpha: f32, radius: f32 },
    /// A rounded rect's fill under a border drawn inside its edge.
    Framed { fill: Rgba, radius: f32, border: Rgba, width: f32 },
    /// A box's inset lines (`inset-shadow` with no blur), each an offset
    /// and its colour, inside the border `border` wide of the rounded rect
    /// on the node's bounds.
    Lips { radius: f32, border: f32, lips: Vec<(f64, f64, Rgba)> },
    Text { text: ShapedText, color: Rgba },
    /// A line under a real Gaussian blur of `blur` px (the lyrics pane's
    /// depth of field), placed the way `Text` is.
    Blur { text: ShapedText, color: Rgba, blur: f32 },
    /// A blurred copy of a line under its crisp one:
    /// the glyphs offset by `x`, `y` and smeared over `blur` pixels.
    Glow { text: ShapedText, color: Rgba, x: f32, y: f32, blur: f32 },
    /// A filled outline and its strokes, in the node's own coordinates.
    Shape { fill: Option<(BezPath, Rgba)>, strokes: Vec<(BezPath, Rgba, f64)> },
    /// A rounded rect filled top to bottom from one colour to another
    /// (a Box's `face`), inside a border `border` wide of the node's bounds
    /// with the outer corner `radius`.
    Face { from: Rgba, to: Rgba, radius: f32, border: f32 },
    /// Casts under a rounded rect, cut out of the rect's own shape so a
    /// translucent fill over them shows the desktop and not the shadow.
    Casts { rect: Rect, radius: f64, layers: Vec<Cast>, cutout: Fit },
    /// A row of terminal cells (the screensaver's): plain rects and glyph
    /// outlines placed at their own origins, each in its own colour.
    Cells { rects: Vec<(Rect, Rgba)>, glyphs: Vec<(std::sync::Arc<BezPath>, f64, f64, Rgba)> },
    /// A canvas's draw list (Radio Atlas's globe), in device pixels; one
    /// list is one picture, so a node keeps the same `Arc` while nothing on
    /// it moved and damages nothing.
    Vector(Arc<Vec<VOp>>),
    /// A card's reactive rim (`surfaces/rim.rs`): lines stroked with a sweep
    /// gradient held in surface pixels, whatever the node's transform.
    /// A rim drawn off `Fit`'s outline on whole pixels, or off its own
    /// paths with none.
    Rim(Vec<RimLine>, Option<Fit>),
    /// A card's fill under its border, the near edge a joined card fades
    /// out as it lets go of its line, and the lit line inside the border.
    Outlined { fit: Fit, fill: Rgba, border: Rgba, near: Rgba, lit: Option<LitLine> },
}

/// An inset ring: a line `width` wide just inside a box's border, its
/// alpha at the bottom `fade` of the top's where it fades.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LitLine {
    pub color: Rgba,
    pub width: f64,
    pub fade: Option<f64>,
}

impl LitLine {
    pub fn of(ring: &fs_theme::style::Ring<Rgba>) -> Self {
        Self { color: ring.color, width: ring.spread, fade: ring.fade }
    }

    pub fn with_alpha(self, alpha: f32) -> Self {
        Self { color: self.color.with_alpha(self.color.a * alpha), ..self }
    }
}

/// How a fitted outline meets the canvas's pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Snap {
    /// Its edges and lines on whole pixels in the node's own space, the
    /// node's transform still moving it by fractions.
    Edges,
    /// At rest, the transform's own offset rounded too: for a node whose
    /// transform is where it stands (a card hanging off its line).
    Placed,
}

/// The outline a card's paints are built off, in the node's coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outline {
    /// A rounded rect with a border `border` wide inside its edge.
    Box { rect: Rect, radius: f64, border: f64 },
    /// A card hanging off its line (`surfaces/shoulders.rs`).
    Join(Joined),
}

/// `shoulders::paths_with`'s arguments: the item `size`, its origin at
/// the top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joined {
    pub size: (f64, f64),
    pub radius: f64,
    pub border: f64,
    pub attach: f64,
    pub near_inset: f64,
    pub walls: crate::paint::shoulders::Walls,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub outline: Outline,
    pub snap: Snap,
}

/// An outline built on the canvas's pixels: the fill, the faces the border
/// strokes, the near edge where a joined card has one, the border's
/// width, and a box's rect and corner radius.
pub struct Drawn {
    pub fill: BezPath,
    pub outer: BezPath,
    pub near: Option<BezPath>,
    pub width: f64,
    pub rounded: Option<(Rect, f64)>,
}

impl Paint {
    /// A rim drawn off `fit`.
    pub fn fit(self, fit: Fit) -> Paint {
        match self {
            Paint::Rim(lines, _) => Paint::Rim(lines, Some(fit)),
            other => other,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RimLine {
    pub path: BezPath,
    pub width: f64,
    /// Cut to this shape, in the node's own coordinates.
    pub clip: Option<BezPath>,
    /// The sweep's centre in surface pixels, its stops from its +x
    /// clockwise, the last one back on the first.
    pub centre: (f64, f64),
    pub stops: Vec<(f32, Rgba)>,
    pub mask: Option<RimMask>,
    /// A stroke's alpha at the bottom of its path as a share of the top.
    pub fade: Option<f64>,
}

impl RimLine {
    /// This line rebuilt off `drawn`: a fill (wash, spill) over the
    /// outline, the specular line one border deep inside it, the border
    /// stroked along its faces.
    pub fn fitted(&self, drawn: &Drawn) -> RimLine {
        let mut out = self.clone();
        if let Some(mask) = &mut out.mask {
            out.path = drawn.fill.clone();
            mask.rect = drawn.fill.bounding_box();
        } else if out.clip.is_some() {
            out.width = drawn.width * 2.0;
            out.path = drawn.fill.clone();
            out.clip = Some(drawn.fill.clone());
        } else {
            out.path = drawn.outer.clone();
            out.width = drawn.width;
        }
        out
    }
}

/// A rim line that fills its path instead of stroking it, the sweep
/// weighted by `image`'s alpha stretched over `rect` (node coordinates).
#[derive(Clone)]
pub struct RimMask {
    pub image: Bitmap,
    pub rect: Rect,
}

impl PartialEq for RimMask {
    fn eq(&self, other: &Self) -> bool {
        self.rect == other.rect && self.image.same_as(&other.image)
    }
}

impl std::fmt::Debug for RimMask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RimMask({}x{} over {:?})", self.image.pixmap.width(), self.image.pixmap.height(), self.rect)
    }
}

/// What a vector op fills or strokes with.
#[derive(Clone, Debug, PartialEq)]
pub enum Brush {
    Solid(Rgba),
    /// Canvas2D's `createRadialGradient` with one centre: stops between
    /// radius `r0` and `r1`.
    Radial { centre: (f64, f64), r0: f64, r1: f64, stops: Vec<(f32, Rgba)> },
}

#[derive(Clone, Debug, PartialEq)]
pub enum VOp {
    Fill(BezPath, Brush),
    Stroke(BezPath, Brush, f64),
    /// Everything up to the matching `Pop` is cut to the path.
    Clip(BezPath),
    Pop,
    /// A bitmap one pixel to a pixel with its top left at the point, at
    /// an alpha.
    Image(Bitmap, (f64, f64), f32),
}

pub struct Node {
    pub bounds: IRect,
    pub paint: Paint,
    pub visible: bool,
    /// From the node's own coordinates to device pixels, after the bounds'
    /// origin for text and before it for everything else.
    pub transform: Affine,
    pub clip: Option<IRect>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(usize);

/// Nodes live in a slab; `order` is paint order, so a node can be inserted
/// beside another (a widget's wash between its fill and its text) without
/// moving any id.
pub struct Scene {
    pub size: IRect,
    nodes: Vec<Node>,
    order: Vec<NodeId>,
    free: Vec<NodeId>,
    damage: Vec<IRect>,
}

impl Scene {
    pub fn new(width: i32, height: i32) -> Self {
        let size = IRect::new(0, 0, width, height);
        Self { size, nodes: Vec::new(), order: Vec::new(), free: Vec::new(), damage: vec![size] }
    }

    /// A scene whose buffers start out transparent and so owe the
    /// compositor nothing until a node is drawn.
    pub fn clear(width: i32, height: i32) -> Self {
        Self { damage: Vec::new(), ..Self::new(width, height) }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.size = IRect::new(0, 0, width, height);
        self.damage = vec![self.size];
    }

    pub fn add(&mut self, bounds: IRect, paint: Paint) -> NodeId {
        self.add_after(None, bounds, paint)
    }

    /// A node painted right after `prev`, or on top of everything.
    pub fn add_after(&mut self, prev: Option<NodeId>, bounds: IRect, paint: Paint) -> NodeId {
        let node = Node { bounds, paint, visible: true, transform: Affine::IDENTITY, clip: None };
        let id = match self.free.pop() {
            Some(id) => {
                self.nodes[id.0] = node;
                id
            }
            None => {
                self.nodes.push(node);
                NodeId(self.nodes.len() - 1)
            }
        };
        let at = prev.and_then(|p| self.order.iter().position(|o| *o == p)).map_or(self.order.len(), |i| i + 1);
        self.order.insert(at, id);
        self.mark(bounds);
        id
    }

    /// Drops a node for good; its id may be handed out again.
    pub fn remove(&mut self, id: NodeId) {
        self.set_visible(id, false);
        if let Some(i) = self.order.iter().position(|o| *o == id) {
            self.order.remove(i);
            self.free.push(id);
        }
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.order.iter().map(|id| &self.nodes[id.0]).filter(|n| n.visible)
    }

    /// Damage with no node behind it: the one rect a first commit carries.
    pub fn touch(&mut self, rect: IRect) {
        self.mark(rect);
    }

    /// Replaces a node's bounds and paint, damaging both rects only when
    /// something actually changed.
    pub fn update_with(
        &mut self,
        id: NodeId,
        bounds: IRect,
        paint: Paint,
        visible: bool,
        transform: Affine,
        clip: Option<IRect>,
    ) {
        let node = &self.nodes[id.0];
        let same = node.visible == visible
            && node.bounds == bounds
            && node.transform == transform
            && node.clip == clip
            && paint_eq(&node.paint, &paint);
        if same {
            return;
        }
        // A transformed node drew past its bounds (a lifted icon scaled
        // about its centre), so where it was is the bounds through that
        // transform as well.
        let old = node.visible.then(|| (node.bounds, node.transform));
        self.nodes[id.0] = Node { bounds, paint, visible, transform, clip };
        for (b, t) in old.into_iter().chain(visible.then_some((bounds, transform))) {
            self.mark(b);
            if t != Affine::IDENTITY {
                let r = Rect::new(b.x as f64, b.y as f64, b.right() as f64, b.bottom() as f64);
                self.mark(Self::cover(r, t, 1.0));
            }
        }
    }

    pub fn transform_of(&self, id: NodeId) -> Affine {
        self.nodes[id.0].transform
    }

    /// Moves a node under `transform` (a deform over what a painter laid
    /// out), damaging where it was and where it lands.
    pub fn set_transform(&mut self, id: NodeId, transform: Affine) {
        let node = &self.nodes[id.0];
        if node.transform == transform {
            return;
        }
        let (bounds, old, visible) = (node.bounds, node.transform, node.visible);
        self.nodes[id.0].transform = transform;
        if visible {
            let r = Rect::new(bounds.x as f64, bounds.y as f64, bounds.right() as f64, bounds.bottom() as f64);
            self.mark(Self::cover(r, old, 1.0));
            self.mark(Self::cover(r, transform, 1.0));
        }
    }

    pub fn set_visible(&mut self, id: NodeId, visible: bool) {
        if self.nodes[id.0].visible != visible {
            self.nodes[id.0].visible = visible;
            let b = self.nodes[id.0].bounds;
            self.mark(b);
        }
    }

    fn mark(&mut self, rect: IRect) {
        let rect = rect.intersect(&self.size);
        if rect.is_empty() {
            return;
        }
        // Overlapping or touching rects merge, so a cell that changed width
        // is one rect rather than two slivers.
        let mut merged = rect;
        loop {
            let grown = IRect::new(merged.x - 1, merged.y - 1, merged.w + 2, merged.h + 2);
            let before = self.damage.len();
            self.damage.retain(|d| {
                if d.intersects(&grown) {
                    merged = merged.union(d);
                    false
                } else {
                    true
                }
            });
            if self.damage.len() == before {
                break;
            }
        }
        self.damage.push(merged);
    }

    /// A rect's device bounds once transformed, padded for antialiasing.
    pub fn cover(rect: Rect, transform: Affine, pad: f64) -> IRect {
        let r = transform.transform_rect_bbox(rect).inflate(pad, pad);
        let (x0, y0) = (r.x0.floor() as i32, r.y0.floor() as i32);
        IRect::new(x0, y0, r.x1.ceil() as i32 - x0, r.y1.ceil() as i32 - y0)
    }

    pub fn has_damage(&self) -> bool {
        !self.damage.is_empty()
    }

    pub fn take_damage(&mut self) -> Vec<IRect> {
        // A blurred node redrawn through a rect cutting across it has come
        // out with a hole where the rect was (a lyric line on the GPU), so
        // a rect reaching one redraws its whole box.
        if !self.damage.is_empty() {
            let blurred: Vec<IRect> = self
                .nodes()
                .filter(|n| matches!(&n.paint, Paint::Blur { .. }) || matches!(&n.paint, Paint::Faded { paint, .. } if matches!(**paint, Paint::Blur { .. })))
                .map(|n| n.bounds.intersect(&self.size))
                .filter(|b| !b.is_empty())
                .collect();
            loop {
                let cut = blurred.iter().find(|b| self.damage.iter().any(|d| d.intersects(b)) && !self.damage.iter().any(|d| d.contains(b)));
                match cut {
                    Some(b) => self.mark(*b),
                    None => break,
                }
            }
        }
        std::mem::take(&mut self.damage)
    }
}

fn paint_eq(a: &Paint, b: &Paint) -> bool {
    match (a, b) {
        (Paint::Faded { paint: p1, fade: f1 }, Paint::Faded { paint: p2, fade: f2 }) => f1 == f2 && paint_eq(p1, p2),
        (Paint::Rect { fill: f1, radius: r1 }, Paint::Rect { fill: f2, radius: r2 }) => f1 == f2 && r1 == r2,
        (Paint::Text { text: t1, color: c1 }, Paint::Text { text: t2, color: c2 }) => c1 == c2 && t1.same_as(t2),
        (Paint::Blur { text: t1, color: c1, blur: b1 }, Paint::Blur { text: t2, color: c2, blur: b2 }) => {
            c1 == c2 && b1 == b2 && t1.same_as(t2)
        }
        (
            Paint::Framed { fill: f1, radius: r1, border: b1, width: w1 },
            Paint::Framed { fill: f2, radius: r2, border: b2, width: w2 },
        ) => f1 == f2 && r1 == r2 && b1 == b2 && w1 == w2,
        (Paint::Lips { radius: r1, border: b1, lips: l1 }, Paint::Lips { radius: r2, border: b2, lips: l2 }) => r1 == r2 && b1 == b2 && l1 == l2,
        (Paint::Glow { text: t1, color: c1, x: x1, y: y1, blur: b1 }, Paint::Glow { text: t2, color: c2, x: x2, y: y2, blur: b2 }) => {
            c1 == c2 && t1.same_as(t2) && x1 == x2 && y1 == y2 && b1 == b2
        }
        (Paint::Image { image: i1, alpha: a1 }, Paint::Image { image: i2, alpha: a2 }) => a1 == a2 && i1.same_as(i2),
        (Paint::Picture { image: i1, alpha: a1, radius: r1 }, Paint::Picture { image: i2, alpha: a2, radius: r2 }) => {
            a1 == a2 && r1 == r2 && i1.same_as(i2)
        }
        (Paint::Shape { fill: f1, strokes: s1 }, Paint::Shape { fill: f2, strokes: s2 }) => f1 == f2 && s1 == s2,
        (Paint::Outlined { fit: a1, fill: f1, border: b1, near: n1, lit: l1 }, Paint::Outlined { fit: a2, fill: f2, border: b2, near: n2, lit: l2 }) => {
            a1 == a2 && f1 == f2 && b1 == b2 && n1 == n2 && l1 == l2
        }
        (
            Paint::Casts { rect: r1, radius: a1, layers: l1, cutout: c1 },
            Paint::Casts { rect: r2, radius: a2, layers: l2, cutout: c2 },
        ) => r1 == r2 && a1 == a2 && c1 == c2 && l1.len() == l2.len() && l1.iter().zip(l2).all(|(a, b)| a.same(b)),
        (Paint::Face { from: f1, to: t1, radius: r1, border: b1 }, Paint::Face { from: f2, to: t2, radius: r2, border: b2 }) => {
            f1 == f2 && t1 == t2 && r1 == r2 && b1 == b2
        }
        (Paint::Vector(a), Paint::Vector(b)) => Arc::ptr_eq(a, b),
        (Paint::Rim(a, f1), Paint::Rim(b, f2)) => a == b && f1 == f2,
        (Paint::Cells { rects: r1, glyphs: g1 }, Paint::Cells { rects: r2, glyphs: g2 }) => {
            r1 == r2
                && g1.len() == g2.len()
                && g1.iter().zip(g2).all(|(a, b)| std::sync::Arc::ptr_eq(&a.0, &b.0) && (a.1, a.2, a.3) == (b.1, b.2, b.3))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn made(w: u16, scale: f64) -> Bitmap {
        Bitmap::from_rgba(w, w, vec![0; usize::from(w) * usize::from(w) * 4]).at(scale)
    }

    #[test]
    fn damage_reaching_a_blurred_node_takes_its_whole_box() {
        let mut text = crate::text::Text::new();
        let style = crate::text::TextStyle {
            family: crate::text::Family::Generic(parley::GenericFamily::SansSerif),
            size: 16.0,
            weight: 600.0,
            tracking: 0.0,
            wdth: 100.0,
            wght: None,
        };
        let shaped = text.shape("and the crowd go crazy", style);
        let mut scene = Scene::clear(400, 100);
        let line = IRect::new(10, 10, 200, 30);
        let other = IRect::new(10, 60, 200, 30);
        scene.add(line, Paint::Blur { text: shaped.clone(), color: Rgba::BLACK, blur: 3.0 });
        scene.add(other, Paint::Text { text: shaped, color: Rgba::BLACK });
        scene.take_damage();
        scene.touch(IRect::new(150, 30, 16, 20));
        let damage = scene.take_damage();
        assert!(damage.iter().any(|d| d.contains(&line)), "{damage:?}");
        assert!(!damage.iter().any(|d| d.intersects(&IRect::new(10, 80, 200, 10))), "{damage:?}");
        scene.touch(IRect::new(300, 70, 10, 10));
        assert_eq!(scene.take_damage(), vec![IRect::new(300, 70, 10, 10)]);
    }

    #[test]
    fn rects_that_miss_intersect_empty() {
        let band = IRect::new(0, 889, 100, 7);
        let view = IRect::new(0, 0, 100, 880);
        let cut = band.intersect(&view);
        assert!(cut.is_empty());
        assert!(cut.w >= 0 && cut.h >= 0);
        assert_eq!(view.intersect(&IRect::new(10, 870, 20, 26)), IRect::new(10, 870, 20, 10));
    }

    #[test]
    fn a_surface_takes_its_own_scale_else_the_next_up_else_the_largest() {
        let all = [made(16, 1.0), made(20, 1.25), made(32, 2.0)];
        assert_eq!(nearest(&all, 1.25).map(|b| b.density), Some(1.25));
        assert_eq!(nearest(&all, 1.5).map(|b| b.density), Some(2.0));
        assert_eq!(nearest(&all, 3.0).map(|b| b.density), Some(2.0));
        assert_eq!(nearest(&all[..1], 1.25).map(|b| b.density), Some(1.0));
        assert!(nearest(&[], 1.0).is_none());
    }

    #[test]
    fn keeping_a_scale_replaces_the_one_made_at_it() {
        let mut all = vec![made(16, 1.0), made(20, 1.25)];
        keep(&mut all, made(21, 1.25));
        assert_eq!(all.len(), 2);
        assert_eq!(nearest(&all, 1.25).map(|b| b.pixmap.width()), Some(21));
    }

    #[test]
    fn a_picture_covers_its_logical_box_at_any_scale() {
        let b = made(20, 1.25);
        assert_eq!(b.logical(), (16.0, 16.0));
        assert_eq!(b.size(), (16, 16));
        assert_ne!(b, made(20, 1.0));
        assert_eq!((pixels_at(16.0, 1.25), pixels_at(16.0, 1.0), pixels_at(0.1, 1.0)), (20, 16, 1));
    }

    #[test]
    fn a_node_redrawn_untransformed_damages_where_its_transform_drew_it() {
        let mut scene = Scene::clear(200, 200);
        let paint = || Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 };
        let id = scene.add(IRect::new(80, 80, 40, 40), paint());
        scene.set_transform(id, Affine::scale_about(1.25, (100.0, 100.0)));
        scene.take_damage();
        scene.update_with(id, IRect::new(60, 60, 40, 40), paint(), true, Affine::IDENTITY, None);
        let damage = scene.take_damage();
        assert!(damage.iter().any(|d| d.x <= 75 && d.y <= 75 && d.right() >= 125 && d.bottom() >= 125), "{damage:?}");
    }
}
