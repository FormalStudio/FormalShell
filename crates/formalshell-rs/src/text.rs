//! Shaping through parley, outlines through skrifa hinted at the pixel size
//! they are drawn at. Layout advances stay linear; what hinting moves is the
//! outline inside each glyph's box, and every glyph origin and baseline is
//! snapped to a whole device pixel so the hinted stems land on the grid.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use parley::fontique::Blob;
use parley::setting::Tag;
use parley::{
    FontContext, FontFamily, FontFamilyName, FontVariation, FontVariations, FontWeight, FontWidth, GenericFamily, Layout,
    LayoutContext, PositionedLayoutItem, RangedBuilder, StyleProperty,
};
use skrifa::bitmap::{BitmapData, BitmapStrikes};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{
    DrawSettings, Engine, GlyphStyles, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
};
use skrifa::raw::types::F2Dot14;
use skrifa::string::StringId;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use vello_cpu::kurbo::BezPath;

use crate::fontconfig::{self, HintStyle, Rendering};

/// A font file mapped rather than read: the emoji face alone is ~10 MB,
/// and only the tables a glyph needs are ever paged in.
fn map_font(path: impl AsRef<std::path::Path>) -> std::io::Result<Blob<u8>> {
    let file = std::fs::File::open(path)?;
    // SAFETY: the font files come from the read-only nix store or a
    // package's share dir; nothing truncates them under a running shell.
    let map = unsafe { memmap2::Mmap::map(&file)? };
    Ok(Blob::new(Arc::new(map)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Generic(GenericFamily),
    /// A family registered by name, the icon font.
    Named(&'static str),
    /// A theme's family by name, with the alias behind it for glyphs the
    /// face lacks and for a face that is not installed at all.
    Face(&'static str, GenericFamily),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub family: Family,
    pub size: f32,
    /// What picks the face; on a face with a `wght` axis it also sets it.
    pub weight: f32,
    /// Extra advance after every glyph, in pixels (Qt's `letterSpacing`).
    pub tracking: f32,
    /// The `wdth` axis in percent, 100 for the face's normal width.
    pub wdth: f32,
    /// The `wght` axis set outright, past whatever `weight` picked.
    pub wght: Option<f32>,
}

impl TextStyle {
    pub const NORMAL_WIDTH: f32 = 100.0;
}

/// Room left round the line box for ink that overhangs it (side bearings,
/// overshoot), so a text node's bounds cover everything it paints.
pub const PAD: i32 = 2;

#[derive(Clone)]
pub struct PlacedGlyph {
    pub path: Arc<BezPath>,
    pub x: i32,
    pub y: i32,
    /// A colour bitmap (CBDT, sbix) drawn instead of the outline, already
    /// at the pixel size, its top-left at `x, y`.
    pub image: Option<crate::scene::Bitmap>,
    /// Where `image` came from, for a surface past scale 1 to make it again
    /// at its own pixel size.
    pub color: Option<Arc<ColorGlyph>>,
    /// The outline rasterised once on the pixel grid, what the renderer
    /// draws text from; `None` for a glyph with no ink.
    pub coverage: Option<Arc<Coverage>>,
}

/// A colour glyph's face, id and size, and the offset its 1x bitmap has
/// from the pen.
pub struct ColorGlyph {
    blob: Blob<u8>,
    index: u32,
    glyph: u32,
    pub size: f32,
    pub dx: i32,
    pub dy: i32,
}

impl ColorGlyph {
    /// The bitmap at `size` pixels, from the strike nearest it, and its
    /// offset from the pen.
    pub fn at(&self, size: f32) -> Option<(crate::scene::Bitmap, i32, i32)> {
        color_bitmap(&FontRef::from_index(self.blob.as_ref(), self.index).ok()?, self.glyph, size)
    }
}

/// A glyph's colour bitmap scaled to `size`, and its top-left against the
/// pen position on the baseline. Only a 32-bit strike is colour; a mask
/// strike is a monochrome bitmap the outline covers.
fn color_bitmap(font: &FontRef, glyph: u32, size: f32) -> Option<(crate::scene::Bitmap, i32, i32)> {
    let strikes = BitmapStrikes::new(font);
    if strikes.is_empty() {
        return None;
    }
    let g = strikes.glyph_for_size(Size::new(size), GlyphId::new(glyph))?;
    let rgba = match g.data {
        BitmapData::Png(bytes) => image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?.into_rgba8(),
        BitmapData::Bgra(bytes) => {
            let mut px = bytes.to_vec();
            for p in px.chunks_exact_mut(4) {
                p.swap(0, 2);
                // Premultiplied in the table; from_rgba wants it straight.
                let a = u32::from(p[3]);
                if a > 0 {
                    for c in &mut p[..3] {
                        *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            image::RgbaImage::from_raw(g.width, g.height, px)?
        }
        BitmapData::Mask(_) => return None,
    };
    let scale = size / g.ppem_y.max(1.0);
    let w = ((rgba.width() as f32 * scale).round() as u32).max(1);
    let h = ((rgba.height() as f32 * scale).round() as u32).max(1);
    let filter = if scale < 1.0 { image::imageops::FilterType::Triangle } else { image::imageops::FilterType::CatmullRom };
    let scaled = if (w, h) == rgba.dimensions() { rgba } else { image::imageops::resize(&rgba, w, h, filter) };
    let dx = (g.inner_bearing_x * scale).round() as i32;
    let dy = -(g.inner_bearing_y * scale).round() as i32;
    Some((crate::scene::Bitmap::from_rgba(w as u16, h as u16, scaled.into_raw()), dx, dy))
}

/// A glyph's antialiased coverage, one byte a pixel, its top-left at `x, y`
/// against the glyph's own origin.
pub struct Coverage {
    pub x: i32,
    pub y: i32,
    pub w: u16,
    pub h: u16,
    pub alpha: Vec<u8>,
}

impl Coverage {
    /// The outline at `scale`, its origin `dx` of a pixel right of a whole one.
    pub fn scaled(path: &BezPath, scale: f64, dx: f64) -> Option<Self> {
        let mut path = path.clone();
        path.apply_affine(vello_cpu::kurbo::Affine::new([scale, 0.0, 0.0, scale, dx, 0.0]));
        Self::of(&path)
    }

    fn of(path: &BezPath) -> Option<Self> {
        use vello_cpu::kurbo::Shape;
        if path.elements().is_empty() {
            return None;
        }
        let b = path.bounding_box();
        let (x, y) = (b.x0.floor() as i32, b.y0.floor() as i32);
        let w = u16::try_from((b.x1.ceil() as i32 - x).max(1)).ok()?;
        let h = u16::try_from((b.y1.ceil() as i32 - y).max(1)).ok()?;
        let mut ctx = vello_cpu::RenderContext::new(w, h);
        ctx.set_transform(vello_cpu::kurbo::Affine::translate((-f64::from(x), -f64::from(y))));
        ctx.set_paint(vello_cpu::color::AlphaColor::<vello_cpu::color::Srgb>::from_rgba8(255, 255, 255, 255));
        ctx.fill_path(path);
        ctx.flush();
        let mut target = vello_cpu::Pixmap::new(w, h);
        ctx.render(&mut target, &mut vello_cpu::Resources::new());
        let alpha: Vec<u8> = target.data().iter().map(|p| p.a).collect();
        alpha.iter().any(|a| *a > 0).then_some(Self { x, y, w, h, alpha })
    }
}

/// The colour emoji face the package ships beside the icon fonts, named so
/// a family that lacks a glyph falls through to it before fontconfig's own
/// `emoji` alias.
const EMOJI_FAMILY: &str = "Noto Color Emoji";

/// A shaped single line, glyphs placed relative to the line's top-left with
/// the baseline already on a whole pixel.
#[derive(Clone)]
pub struct ShapedText {
    source: String,
    style: TextStyle,
    /// The advance of the line, unpadded.
    pub width: i32,
    pub ascent: f32,
    pub descent: f32,
    pub glyphs: Vec<PlacedGlyph>,
}

impl ShapedText {
    pub fn line_height(&self) -> i32 {
        (self.ascent + self.descent).ceil() as i32
    }

    /// The node box this line paints into, padding included.
    pub fn box_size(&self) -> (i32, i32) {
        (self.width + PAD * 2, self.line_height() + PAD * 2)
    }

    pub fn same_as(&self, other: &ShapedText) -> bool {
        self.source == other.source && self.style == other.style
    }
}

#[derive(Hash, PartialEq, Eq)]
struct InstanceKey {
    blob: u64,
    index: u32,
    size_bits: u32,
    coords: Vec<i16>,
}

#[derive(Hash, PartialEq, Eq)]
struct GlyphKey {
    instance: usize,
    glyph: u32,
}

/// The shaper, shared: the UI thread shapes through it, and a pool job
/// warms it ahead of a surface (faces, outlines, whole strings) so the UI
/// thread finds them cached. The lock is taken per string, so the UI waits
/// on at most one shape.
#[derive(Clone)]
pub struct Text(Arc<Shared>);

struct Shared {
    inner: Mutex<Option<Inner>>,
    /// fontconfig and the font collection, loaded on a thread of their own
    /// while the compositor answers the bar's first configure.
    loading: Mutex<Option<std::thread::JoinHandle<Inner>>>,
    /// Draws waiting on the lock; warming stands aside while there are any.
    urgent: AtomicUsize,
}

/// A style as a cache key, built without allocating: a frame's lookups of
/// strings already shaped cost a hash each.
#[derive(Hash, PartialEq, Eq, Clone, Copy)]
struct StyleKey {
    family: Family,
    size: u32,
    weight: u32,
    tracking: u32,
    wdth: u32,
    wght: Option<u32>,
}

impl StyleKey {
    fn new(style: TextStyle) -> Self {
        Self {
            family: style.family,
            size: style.size.to_bits(),
            weight: style.weight.to_bits(),
            tracking: style.tracking.to_bits(),
            wdth: style.wdth.to_bits(),
            wght: style.wght.map(f32::to_bits),
        }
    }
}

const SHAPED_LIMIT: usize = 4096;
const OUTLINE_LIMIT: usize = 8192;

/// The loader `preload` started, for the first `Text` to take.
static PRELOAD: Mutex<Option<std::thread::JoinHandle<Inner>>> = Mutex::new(None);

fn load() -> std::thread::JoinHandle<Inner> {
    std::thread::Builder::new().name("fs-fonts".into()).spawn(Inner::new).expect("spawn the font loader")
}

/// Starts loading the fonts before the compositor connection: the bar's
/// first frame waits on them, and the connection's own wait is as long.
pub fn preload() {
    *PRELOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(load());
}

impl Text {
    pub fn new() -> Self {
        let loading = PRELOAD.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_else(load);
        Self(Arc::new(Shared { inner: Mutex::new(None), loading: Mutex::new(Some(loading)), urgent: AtomicUsize::new(0) }))
    }

    /// A shape a draw is waiting on, ahead of any warming.
    pub fn shape(&mut self, source: &str, style: TextStyle) -> ShapedText {
        self.0.urgent.fetch_add(1, Ordering::SeqCst);
        self.shape_now(source, style, true)
    }

    /// Shapes each string ahead of its first draw, one lock per string,
    /// standing aside before each while a draw is waiting: a mutex lets the
    /// thread that just let go take it straight back, and this loop would
    /// otherwise starve the UI thread for its whole list.
    pub fn warm(&self, jobs: &[(String, TextStyle)]) {
        for (source, style) in jobs {
            while self.0.urgent.load(Ordering::SeqCst) > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            self.shape_now(source, *style, false);
        }
    }

    fn shape_now(&self, source: &str, style: TextStyle, urgent: bool) -> ShapedText {
        let asked = std::time::Instant::now();
        let mut guard = self.0.inner.lock().unwrap_or_else(|e| e.into_inner());
        if urgent {
            self.0.urgent.fetch_sub(1, Ordering::SeqCst);
        }
        if guard.is_none() {
            let loading = self.0.loading.lock().unwrap_or_else(|e| e.into_inner()).take();
            *guard = Some(loading.and_then(|h| h.join().ok()).unwrap_or_else(Inner::new));
        }
        if asked.elapsed().as_millis() >= 4 {
            eprintln!("text: waited {}us on the shaper", asked.elapsed().as_micros());
        }
        let inner = guard.as_mut().expect("loaded above");
        let key = StyleKey::new(style);
        if let Some(hit) = inner.shaped.get(&key).and_then(|m| m.get(source)) {
            return hit.clone();
        }
        let shaped = inner.shape(source, style);
        if inner.shaped_count >= SHAPED_LIMIT {
            inner.shaped.clear();
            inner.shaped_count = 0;
        }
        inner.shaped.entry(key).or_default().insert(source.to_owned(), shaped.clone());
        inner.shaped_count += 1;
        shaped
    }
}

struct Inner {
    fcx: FontContext,
    lcx: LayoutContext<()>,
    rendering: Rendering,
    instance_ids: HashMap<InstanceKey, usize>,
    instances: Vec<Option<HintingInstance>>,
    outlines: HashMap<GlyphKey, (Arc<BezPath>, Option<Arc<Coverage>>)>,
    /// Colour bitmaps by face, glyph and pixel size; `None` where the face
    /// has no bitmap for the glyph.
    bitmaps: HashMap<(u64, u32, u32, u32), Option<(crate::scene::Bitmap, Arc<ColorGlyph>)>>,
    /// The autohinter's glyph classes per face, the costly half of a hinting
    /// instance, computed once and shared by every size.
    styles: HashMap<(u64, u32), GlyphStyles>,
    shaped: HashMap<StyleKey, HashMap<String, ShapedText>>,
    shaped_count: usize,
    /// Theme families the collection has no face for, logged once each.
    missing: HashSet<&'static str>,
}

impl Inner {
    fn new() -> Self {
        let rendering = fontconfig::rendering();
        eprintln!("text: fontconfig {rendering:?}");
        crate::phase("fontconfig");
        let mut fcx = FontContext::new();
        crate::phase("font collection");
        // The package wraps the binary with the lucide font's path, the way
        // nix/package.nix puts it on XDG_DATA_DIRS for Qt.
        match std::env::var("FS_RS_ICON_FONT").map(map_font) {
            Ok(Ok(data)) => {
                let families = fcx.collection.register_fonts(data, None);
                eprintln!("text: icon font registered ({} families)", families.len());
            }
            Ok(Err(err)) => eprintln!("text: icon font unreadable: {err}"),
            Err(_) => eprintln!("text: FS_RS_ICON_FONT unset, no icons"),
        }
        // font-logos and the Nerd set, by directory: the package does not
        // name one file of it.
        for dir in std::env::var("FS_RS_FONT_DIRS").unwrap_or_default().split(':').filter(|d| !d.is_empty()) {
            let mut stack = vec![std::path::PathBuf::from(dir)];
            while let Some(path) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(&path) else { continue };
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else if let Some(data) =
                        p.extension().filter(|e| *e == "ttf" || *e == "otf").and_then(|_| map_font(&p).ok())
                    {
                        fcx.collection.register_fonts(data, None);
                        eprintln!("text: registered {}", p.display());
                    }
                }
            }
        }
        crate::phase("font files");
        Self {
            fcx,
            lcx: LayoutContext::new(),
            rendering,
            instance_ids: HashMap::new(),
            instances: Vec::new(),
            outlines: HashMap::new(),
            bitmaps: HashMap::new(),
            styles: HashMap::new(),
            shaped: HashMap::new(),
            shaped_count: 0,
            missing: HashSet::new(),
        }
    }

    fn shape(&mut self, source: &str, style: TextStyle) -> ShapedText {
        let face = match style.family {
            Family::Face(name, g) if self.fcx.collection.family_id(name).is_none() => {
                if self.missing.insert(name) {
                    eprintln!("text: no face for \"{name}\", drawing {g:?}");
                }
                Family::Generic(g)
            }
            family => family,
        };
        let mut builder = self.lcx.ranged_builder(&mut self.fcx, source, 1.0, true);
        match face {
            Family::Generic(g) => text_stack(&mut builder, source, None, g),
            Family::Face(name, g) => text_stack(&mut builder, source, Some(name), g),
            Family::Named(name) => builder.push_default(StyleProperty::from(FontFamily::Source(Cow::Borrowed(name)))),
        }
        builder.push_default(StyleProperty::FontSize(style.size));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight)));
        if style.tracking != 0.0 {
            builder.push_default(StyleProperty::LetterSpacing(style.tracking));
        }
        let mut axes = Vec::new();
        if style.wdth != TextStyle::NORMAL_WIDTH {
            builder.push_default(StyleProperty::FontWidth(FontWidth::from_percentage(style.wdth)));
            axes.push(FontVariation::new(Tag::new(b"wdth"), style.wdth));
        }
        if let Some(wght) = style.wght {
            axes.push(FontVariation::new(Tag::new(b"wght"), wght));
        }
        if !axes.is_empty() {
            builder.push_default(StyleProperty::FontVariations(FontVariations::List(Cow::Owned(axes))));
        }
        let mut layout: Layout<()> = builder.build(source);
        layout.break_all_lines(None);

        let mut shaped = ShapedText {
            source: source.to_owned(),
            style,
            width: layout.width().ceil() as i32,
            ascent: 0.0,
            descent: 0.0,
            glyphs: Vec::new(),
        };
        let Some(line) = layout.lines().next() else {
            return shaped;
        };
        let metrics = line.metrics();
        shaped.ascent = metrics.ascent;
        shaped.descent = metrics.descent;
        let baseline = metrics.ascent.round();
        let line_baseline = metrics.baseline;

        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let size = run.font_size();
            let coords: Vec<i16> = run.normalized_coords().to_vec();
            let Ok(font_ref) = FontRef::from_index(font.data.as_ref(), font.index) else {
                continue;
            };
            let instance = self.instance(&font_ref, font.data.id(), font.index, size, &coords);
            for glyph in glyph_run.positioned_glyphs() {
                let x = PAD + glyph.x.round() as i32;
                let y = PAD + (baseline + glyph.y - line_baseline).round() as i32;
                if let Some((image, color)) = self.bitmap(&font_ref, &font.data, font.index, glyph.id, size) {
                    let (dx, dy) = (color.dx, color.dy);
                    shaped.glyphs.push(PlacedGlyph { path: Arc::new(BezPath::new()), x: x + dx, y: y + dy, image: Some(image), color: Some(color), coverage: None });
                    continue;
                }
                let (path, coverage) = self.outline(&font_ref, instance, glyph.id, size, &coords);
                shaped.glyphs.push(PlacedGlyph { path, x, y, image: None, color: None, coverage });
            }
        }
        shaped
    }

    fn instance(&mut self, font: &FontRef, blob: u64, index: u32, size: f32, coords: &[i16]) -> usize {
        let key = InstanceKey { blob, index, size_bits: size.to_bits(), coords: coords.to_vec() };
        if let Some(id) = self.instance_ids.get(&key) {
            return *id;
        }
        let outlines = font.outline_glyphs();
        let location = location(coords);
        let hinting = hinting_options(self.rendering).and_then(|mut options| {
            if let Engine::Auto(None) = options.engine {
                let styles = self.styles.entry((blob, index)).or_insert_with(|| GlyphStyles::new(&outlines));
                options.engine = Engine::Auto(Some(styles.clone()));
            }
            HintingInstance::new(&outlines, Size::new(size), LocationRef::new(&location), options).ok()
        });
        let family = font
            .localized_strings(StringId::FAMILY_NAME)
            .english_or_first()
            .map(|n| n.to_string())
            .unwrap_or_default();
        let subfamily = font
            .localized_strings(StringId::SUBFAMILY_NAME)
            .english_or_first()
            .map(|n| n.to_string())
            .unwrap_or_default();
        eprintln!(
            "text: face \"{family} {subfamily}\" {size}px coords={coords:?} hinted={}",
            hinting.is_some()
        );
        self.instances.push(hinting);
        let id = self.instances.len() - 1;
        self.instance_ids.insert(key, id);
        id
    }

    /// The glyph's colour bitmap at `size` once, and where it came from.
    fn bitmap(&mut self, font: &FontRef, data: &Blob<u8>, index: u32, glyph: u32, size: f32) -> Option<(crate::scene::Bitmap, Arc<ColorGlyph>)> {
        let key = (data.id(), index, glyph, size.to_bits());
        if let Some(hit) = self.bitmaps.get(&key) {
            return hit.clone();
        }
        let placed = color_bitmap(font, glyph, size)
            .map(|(image, dx, dy)| (image, Arc::new(ColorGlyph { blob: data.clone(), index, glyph, size, dx, dy })));
        self.bitmaps.insert(key, placed.clone());
        placed
    }

    fn outline(&mut self, font: &FontRef, instance: usize, glyph: u32, size: f32, coords: &[i16]) -> (Arc<BezPath>, Option<Arc<Coverage>>) {
        let key = GlyphKey { instance, glyph };
        if let Some(hit) = self.outlines.get(&key) {
            return hit.clone();
        }
        let hinting = self.instances[instance].as_ref();
        let mut pen = FlipPen(BezPath::new());
        if let Some(outline) = font.outline_glyphs().get(GlyphId::new(glyph)) {
            let location = location(coords);
            let drawn = match hinting {
                Some(h) => outline.draw(DrawSettings::hinted(h, false), &mut pen),
                None => outline.draw(DrawSettings::unhinted(Size::new(size), LocationRef::new(&location)), &mut pen),
            };
            if drawn.is_err() {
                pen.0 = BezPath::new();
            }
        }
        let coverage = Coverage::of(&pen.0).map(Arc::new);
        let path = Arc::new(pen.0);
        // The screensaver shapes glyphs no surface draws again; the paths
        // live on in the strings that hold them.
        if self.outlines.len() >= OUTLINE_LIMIT {
            self.outlines.clear();
        }
        self.outlines.insert(key, (path.clone(), coverage.clone()));
        (path, coverage)
    }
}

/// A text family's stack: the named face if any, the alias, then the
/// colour emoji face. DejaVu, behind every sans-serif, draws many emoji as
/// outlines, so an emoji run asks the colour face first, the way a browser
/// honours emoji presentation.
fn text_stack(builder: &mut RangedBuilder<'_, ()>, source: &str, name: Option<&'static str>, g: GenericFamily) {
    let lead = name.map(|n| FontFamilyName::Named(Cow::Borrowed(n)));
    let stack: Vec<_> = lead
        .clone()
        .into_iter()
        .chain([
            FontFamilyName::Generic(g),
            FontFamilyName::Named(Cow::Borrowed(EMOJI_FAMILY)),
            FontFamilyName::Generic(GenericFamily::Emoji),
        ])
        .collect();
    builder.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(stack))));
    for range in emoji_runs(source) {
        let stack = [FontFamilyName::Named(Cow::Borrowed(EMOJI_FAMILY)), FontFamilyName::Generic(GenericFamily::Emoji)]
            .into_iter()
            .chain(lead.clone())
            .chain([FontFamilyName::Generic(g)])
            .collect();
        builder.push(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(stack))), range);
    }
}

/// Byte ranges of emoji sequences: pictographs, regional indicators, skin
/// tones, and anything carrying VS16, with the joiners, keycaps and tags
/// that bind them.
fn emoji_runs(source: &str) -> Vec<std::ops::Range<usize>> {
    let chars: Vec<(usize, char)> = source.char_indices().collect();
    let pictographic = |c: char| matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF);
    let binding = |c: char| matches!(c as u32, 0xFE0F | 0x200D | 0x20E3 | 0xE0020..=0xE007F);
    let mut runs: Vec<std::ops::Range<usize>> = Vec::new();
    for (i, &(at, c)) in chars.iter().enumerate() {
        let next_vs16 = chars.get(i + 1).is_some_and(|&(_, n)| n == '\u{FE0F}' || n == '\u{20E3}');
        let wide = (c as u32) >= 0x1F000 && pictographic(c);
        let continues = binding(c) && runs.last().is_some_and(|r| r.end == at);
        if wide || next_vs16 || continues {
            let end = at + c.len_utf8();
            match runs.last_mut() {
                Some(r) if r.end == at => r.end = end,
                _ => runs.push(at..end),
            }
        }
    }
    runs
}

fn location(coords: &[i16]) -> Vec<NormalizedCoord> {
    coords.iter().map(|c| F2Dot14::from_bits(*c)).collect()
}

/// fontconfig's hintstyle onto skrifa's targets, the way cairo maps it onto
/// FreeType's load targets: slight is FreeType's light target, which runs
/// the autohinter on the vertical axis only, medium and full are the normal
/// target. Grayscale throughout, whatever `rgba` says, so subpixel layouts
/// are not asked for.
fn hinting_options(r: Rendering) -> Option<HintingOptions> {
    if !r.antialias || !r.hinting {
        return None;
    }
    let (engine, mode) = match r.hint_style {
        HintStyle::None => return None,
        HintStyle::Slight => (Engine::Auto(None), SmoothMode::Light),
        HintStyle::Medium | HintStyle::Full => (Engine::AutoFallback, SmoothMode::Normal),
    };
    Some(HintingOptions {
        engine,
        target: Target::Smooth { mode, symmetric_rendering: true, preserve_linear_metrics: true },
    })
}

/// skrifa draws y-up in pixels; the scene is y-down.
struct FlipPen(BezPath);

impl OutlinePen for FlipPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((x as f64, -y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((x as f64, -y as f64));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to((cx0 as f64, -cy0 as f64), (x as f64, -y as f64));
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to((cx0 as f64, -cy0 as f64), (cx1 as f64, -cy1 as f64), (x as f64, -y as f64));
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(family: Family, wdth: f32, wght: Option<f32>) -> TextStyle {
        TextStyle { family, size: 20.0, weight: 400.0, tracking: 0.0, wdth, wght }
    }

    #[test]
    fn a_face_not_installed_draws_its_alias() {
        let mut inner = Inner::new();
        let missing = Family::Face("No Such Family", GenericFamily::Monospace);
        let alias = inner.shape("values 0123", style(Family::Generic(GenericFamily::Monospace), 100.0, None)).width;
        assert_eq!(inner.shape("values 0123", style(missing, 100.0, None)).width, alias);
        inner.shape("again", style(missing, 100.0, None));
        assert_eq!(inner.missing.len(), 1);
    }
}
