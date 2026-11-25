//! Everything over the picture, drawn on the CPU whenever it changes (it
//! does not change with the video): the hover strip and the state screens
//! in the phone's own rounded shape. Lucide glyphs come from the package's
//! icon font, text from the system's sans face through parley.

use std::path::PathBuf;
use std::sync::Arc;

use parley::fontique::Blob;
use parley::{FontContext, FontFamily, FontFamilyName, FontWeight, GenericFamily, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Rect, RoundedRect, Shape};
use vello_cpu::{Glyph, Pixmap, RenderContext, Resources};

/// A button on the hover strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Home,
    AppSwitcher,
    Mute,
    Close,
}

pub const CONTROLS: [Control; 4] = [Control::Home, Control::AppSwitcher, Control::Mute, Control::Close];

/// What the phone's shape holds when it is not the live picture.
#[derive(Clone, Debug, PartialEq)]
pub enum Screen {
    Live,
    /// No picture yet: a title and the step under way.
    Waiting { title: String, detail: String },
    Locked,
    /// The last picture stays; a quiet badge says it is not live.
    Stalled,
}

/// One state of the overlay, in logical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub size: (u32, u32),
    pub scale: f64,
    pub phone: Rect,
    pub radius: f64,
    pub screen: Screen,
    pub strip: bool,
    pub hover: Option<Control>,
    pub muted: bool,
}

/// The strip's geometry: one pill above the phone, centred.
pub fn strip_buttons(phone: Rect) -> Vec<(Control, Rect)> {
    let size = 28.0;
    let gap = 2.0;
    let pad = 4.0;
    let width = CONTROLS.len() as f64 * size + (CONTROLS.len() - 1) as f64 * gap + 2.0 * pad;
    let x0 = phone.center().x - width / 2.0 + pad;
    let y0 = phone.y0 - STRIP_GAP - size - pad;
    CONTROLS
        .iter()
        .enumerate()
        .map(|(i, c)| (*c, Rect::new(x0 + i as f64 * (size + gap), y0, x0 + i as f64 * (size + gap) + size, y0 + size)))
        .collect()
}

pub fn strip_rect(phone: Rect) -> Rect {
    let buttons = strip_buttons(phone);
    let (first, last) = (buttons[0].1, buttons[buttons.len() - 1].1);
    Rect::new(first.x0 - 4.0, first.y0 - 4.0, last.x1 + 4.0, last.y1 + 4.0)
}

/// Room above the phone for the strip.
pub const STRIP_GAP: f64 = 8.0;
pub const TOP_MARGIN: f64 = 52.0;
/// Room on the other sides for the shadow.
pub const MARGIN: f64 = 32.0;

pub struct Overlay {
    fcx: FontContext,
    lcx: LayoutContext<()>,
    resources: Resources,
    icons: bool,
}

fn icon_font() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FS_RS_ICON_FONT") {
        return Some(p.into());
    }
    // Installed as <prefix>/bin/formalshell-iphone-mirror.
    let exe = std::env::current_exe().ok()?;
    let p = exe.parent()?.parent()?.join("share/formalshell/fonts/lucide.ttf");
    p.exists().then_some(p)
}

/// Must match the shader's: see `rrect` in gpu.rs.
const CORNER_EXTENT: f64 = 1.52866;
const CORNER_EXPONENT: f64 = 3.26;

/// The phone's outline with continuous corners, as the shader cuts it.
pub fn continuous(rect: Rect, r: f64) -> BezPath {
    let l = (r * CORNER_EXTENT).min(rect.width() / 2.0).min(rect.height() / 2.0);
    let mut path = BezPath::new();
    // Corner centres, each with the quadrant its arc sweeps, clockwise from top right.
    let corners = [
        (rect.x1 - l, rect.y0 + l, 1.0, -1.0),
        (rect.x1 - l, rect.y1 - l, 1.0, 1.0),
        (rect.x0 + l, rect.y1 - l, -1.0, 1.0),
        (rect.x0 + l, rect.y0 + l, -1.0, -1.0),
    ];
    const STEPS: usize = 24;
    for (i, (cx, cy, sx, sy)) in corners.iter().enumerate() {
        for k in 0..=STEPS {
            // Top right runs from the top edge (t = 0) to the right edge
            // (t = 1); the others turn on clockwise.
            let t = k as f64 / STEPS as f64 * std::f64::consts::FRAC_PI_2;
            let (a, b) = if i % 2 == 0 { (t.sin(), t.cos()) } else { (t.cos(), t.sin()) };
            let x = cx + sx * l * a.powf(2.0 / CORNER_EXPONENT);
            let y = cy + sy * l * b.powf(2.0 / CORNER_EXPONENT);
            if i == 0 && k == 0 {
                path.move_to((x, y));
            } else {
                path.line_to((x, y));
            }
        }
    }
    path.close_path();
    path
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> AlphaColor<Srgb> {
    AlphaColor::from_rgba8(r, g, b, (a * 255.0).round() as u8)
}

impl Overlay {
    pub fn new() -> Self {
        let mut fcx = FontContext::new();
        let mut icons = false;
        match icon_font().map(std::fs::read) {
            Some(Ok(data)) => {
                fcx.collection.register_fonts(Blob::new(Arc::new(data)), None);
                icons = true;
            }
            Some(Err(e)) => tracing::warn!("icon font: {e}"),
            None => tracing::warn!("icon font: no lucide.ttf (FS_RS_ICON_FONT unset)"),
        }
        Self { fcx, lcx: LayoutContext::new(), resources: Resources::new(), icons }
    }

    /// The scene as premultiplied RGBA at device pixels.
    pub fn draw(&mut self, s: &Scene) -> Pixmap {
        let (w, h) = ((s.size.0 as f64 * s.scale).round() as u16, (s.size.1 as f64 * s.scale).round() as u16);
        let mut cx = RenderContext::new(w.max(1), h.max(1));
        cx.set_transform(Affine::scale(s.scale));
        let shape = continuous(s.phone, s.radius);
        match &s.screen {
            Screen::Live => {}
            Screen::Waiting { title, detail } => {
                cx.set_paint(rgba(18, 18, 20, 1.0));
                cx.fill_path(&shape);
                let c = s.phone.center();
                self.icon(&mut cx, "smartphone", 40.0, (c.x, c.y - 44.0), rgba(255, 255, 255, 0.85));
                self.text(&mut cx, title, 17.0, FontWeight::SEMI_BOLD, (c.x, c.y + 10.0), rgba(255, 255, 255, 0.92), s.phone.width() - 48.0);
                self.text(&mut cx, detail, 13.0, FontWeight::NORMAL, (c.x, c.y + 36.0), rgba(255, 255, 255, 0.55), s.phone.width() - 48.0);
            }
            Screen::Locked => {
                cx.set_paint(rgba(18, 18, 20, 1.0));
                cx.fill_path(&shape);
                let c = s.phone.center();
                self.icon(&mut cx, "lock", 40.0, (c.x, c.y - 44.0), rgba(255, 255, 255, 0.85));
                self.text(&mut cx, "iPhone is locked", 17.0, FontWeight::SEMI_BOLD, (c.x, c.y + 10.0), rgba(255, 255, 255, 0.92), s.phone.width() - 48.0);
                self.text(&mut cx, "Unlock your iPhone to continue", 13.0, FontWeight::NORMAL, (c.x, c.y + 36.0), rgba(255, 255, 255, 0.55), s.phone.width() - 48.0);
            }
            Screen::Stalled => {
                // A small pill near the bottom: the picture is not live.
                let c = s.phone.center();
                let pill = Rect::new(c.x - 62.0, s.phone.y1 - 64.0, c.x + 62.0, s.phone.y1 - 36.0);
                cx.set_paint(rgba(20, 20, 22, 0.78));
                cx.fill_path(&RoundedRect::from_rect(pill, 14.0).to_path(0.1));
                self.text(&mut cx, "Reconnecting", 13.0, FontWeight::MEDIUM, (c.x, pill.center().y), rgba(255, 255, 255, 0.85), 200.0);
            }
        }
        if s.strip {
            let bar = strip_rect(s.phone);
            cx.set_paint(rgba(28, 28, 30, 0.82));
            cx.fill_path(&RoundedRect::from_rect(bar, bar.height() / 2.0).to_path(0.1));
            for (control, r) in strip_buttons(s.phone) {
                if s.hover == Some(control) {
                    cx.set_paint(rgba(255, 255, 255, 0.16));
                    cx.fill_path(&RoundedRect::from_rect(r, r.height() / 2.0).to_path(0.1));
                }
                let name = match control {
                    Control::Home => "house",
                    Control::AppSwitcher => "layers",
                    Control::Mute if s.muted => "volume-x",
                    Control::Mute => "volume-2",
                    Control::Close => "x",
                };
                self.icon(&mut cx, name, 16.0, (r.center().x, r.center().y), rgba(255, 255, 255, 0.9));
            }
        }
        let mut pixmap = Pixmap::new(w.max(1), h.max(1));
        cx.flush();
        cx.render(&mut pixmap, &mut self.resources);
        pixmap
    }

    fn icon(&mut self, cx: &mut RenderContext, name: &str, size: f32, center: (f64, f64), color: AlphaColor<Srgb>) {
        if !self.icons {
            return;
        }
        let g = fs_theme::icons::glyph(name);
        self.run(cx, g.text, FontFamily::named(g.family), size, FontWeight::NORMAL, center, color, f32::MAX);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, cx: &mut RenderContext, text: &str, size: f32, weight: FontWeight, center: (f64, f64), color: AlphaColor<Srgb>, max: f64) {
        self.run(cx, text, FontFamily::Single(FontFamilyName::Generic(GenericFamily::SystemUi)), size, weight, center, color, max as f32);
    }

    /// One line of `text` centred on `center`.
    #[allow(clippy::too_many_arguments)]
    fn run(
        &mut self,
        cx: &mut RenderContext,
        text: &str,
        family: FontFamily,
        size: f32,
        weight: FontWeight,
        center: (f64, f64),
        color: AlphaColor<Srgb>,
        max: f32,
    ) {
        let mut builder = self.lcx.ranged_builder(&mut self.fcx, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(family));
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::FontWeight(weight));
        let mut layout: Layout<()> = builder.build(text);
        layout.break_all_lines(Some(max));
        let Some(line) = layout.lines().next() else { return };
        let m = line.metrics();
        let x0 = center.0 - layout.width() as f64 / 2.0;
        // The line's box centred, its baseline where that puts it.
        let y0 = center.1 - (m.ascent + m.descent) as f64 / 2.0 + m.ascent as f64 - m.baseline as f64;
        cx.set_paint(color);
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
            let run = glyph_run.run();
            let glyphs: Vec<Glyph> = glyph_run
                .positioned_glyphs()
                .map(|g| Glyph { id: g.id, x: (x0 + g.x as f64) as f32, y: (y0 + g.y as f64) as f32 })
                .collect();
            if let Err(e) = cx
                .glyph_run(&mut self.resources, run.font())
                .font_size(run.font_size())
                .normalized_coords(run.normalized_coords())
                .fill_glyphs(glyphs.into_iter())
            {
                tracing::warn!("glyphs for {text:?}: {e:?}");
            }
        }
    }
}

