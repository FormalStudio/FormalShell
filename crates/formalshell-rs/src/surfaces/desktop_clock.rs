//! The clock face the desktop clock (`wayland/desktop_clock.rs`) and the
//! sidebar's time card both draw: the format every clock reads, the
//! `desktopClock.*` keys the digits are drawn by, the digits laid out in
//! their face, and the glass they are drawn in, frosted over the picture
//! behind with the wallpaper's subject laid back over them.

use std::sync::Arc;

use fs_chrome::clock_face::{self, Face, Width};
use fs_info::clock::CLOCK_FORMATS;
use fs_theme::color::Rgba;
use fs_theme::palette::ColorRole;

use crate::paint::{Kit, Painter};
use crate::scene::Bitmap;
use fs_types::IRect;
use crate::services::wallpaper::{DepthCut, Picture};
use crate::settings;
use crate::store::Store;
use crate::text::{Family, ShapedText, TextStyle};

/// The clock format every clock reads: the one state.json saved, else the
/// locale's own 12 or 24 hour preset.
pub fn clock_format(store: &Store) -> String {
    let saved = &store.state.data.clock_format;
    match saved.is_empty() {
        false => saved.clone(),
        true if twelve_hour() => CLOCK_FORMATS[1].to_owned(),
        true => CLOCK_FORMATS[0].to_owned(),
    }
}

/// Whether the locale's LC_TIME writes a 12 hour clock: its own time
/// format (`nl_langinfo(T_FMT)`) carrying an AM/PM or a 12 hour field.
fn twelve_hour() -> bool {
    static TWELVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TWELVE.get_or_init(|| {
        // A locale object of its own: setlocale would change the process's.
        let loc = unsafe { libc::newlocale(libc::LC_TIME_MASK, c"".as_ptr(), std::ptr::null_mut()) };
        if loc.is_null() {
            return false;
        }
        let fmt = unsafe { std::ffi::CStr::from_ptr(libc::nl_langinfo_l(libc::T_FMT, loc)) }.to_string_lossy().into_owned();
        unsafe { libc::freelocale(loc) };
        ["%p", "%P", "%r", "%I", "%l"].iter().any(|f| fmt.contains(f))
    })
}

/// A subject's coverage, one byte per picture pixel, and each column's
/// first row at least half covered (the picture's height for none).
pub(crate) struct Subject {
    mask: Vec<u8>,
    top: Vec<u32>,
}

impl Subject {
    pub(crate) fn new(mask: Vec<u8>, width: u32, height: u32) -> Self {
        let w = width as usize;
        let top = (0..w).map(|x| (0..height).find(|&y| mask.get(y as usize * w + x).is_some_and(|&a| a >= 128)).unwrap_or(height)).collect();
        Self { mask, top }
    }

    /// The median top edge over columns `x0..x1`, none where most of them
    /// carry no subject.
    pub(crate) fn edge(&self, x0: usize, x1: usize, height: u32) -> Option<u32> {
        let mut tops: Vec<u32> = self.top.get(x0..x1.min(self.top.len()))?.to_vec();
        if tops.is_empty() {
            return None;
        }
        let mid = tops.len() / 2;
        let (_, m, _) = tops.select_nth_unstable(mid);
        (*m < height).then_some(*m)
    }
}

/// The keys both clocks draw their digits by, read whether or not
/// `desktopClock.enabled` is on: the sidebar's clock card wears them too.
#[derive(Clone, PartialEq)]
pub(crate) struct FaceSettings {
    pub face: Face,
    pub width: Width,
    pub color: String,
    pub cut: Option<DepthCut>,
}

impl FaceSettings {
    pub(crate) fn read(config: &crate::services::config::State) -> Self {
        Self {
            face: Face::parse(settings::CLOCK_FACE.get(config)).unwrap_or(Face::Pixel),
            width: Width::parse(settings::CLOCK_WIDTH.get(config)).unwrap_or(Width::Wide),
            color: settings::CLOCK_COLOR.get(config).to_owned(),
            cut: DepthCut::read(config, "desktopClock", None),
        }
    }

    /// The ink `color` names; none for `auto`, which reads what is behind.
    pub(crate) fn ink(&self, store: &Store) -> Option<Rgba> {
        match self.color.as_str() {
            "auto" => None,
            "primary" => Some(store.theme.theme.colors.get(ColorRole::Primary)),
            hex => Some(Rgba::parse(hex).unwrap_or(Rgba::WHITE)),
        }
    }
}

/// `auto`'s ink over what reads `luma` (0..255) behind the digits.
pub(crate) fn auto_ink(luma: f64) -> Rgba {
    if luma > fs_theme::barpaint::LUMA_THRESHOLD { Rgba::BLACK } else { Rgba::WHITE }
}

/// What a grid face's bitmap was drawn for.
#[derive(Clone, PartialEq)]
struct Look {
    face: Face,
    text: String,
    device: u32,
    width: Width,
    ink: Rgba,
    scale: f64,
    /// The picture the digits are frosted over, and their box in its pixels.
    under: Option<(usize, IRect)>,
}

/// What a clock's digits are frosted over: its output's picture, the
/// picture's pixels per logical pixel, the surface's top left on the
/// output, and the subject cut from the picture.
#[derive(Clone)]
pub(crate) struct Under {
    pub picture: Arc<Picture>,
    pub k: f64,
    pub origin: (f64, f64),
    pub subject: Option<Arc<Subject>>,
}

impl Under {
    /// The picture's own pixels under a rect in the surface's logical
    /// pixels, the way the lock crops its foreground.
    pub(crate) fn device(&self, r: IRect) -> IRect {
        let (k, (ox, oy)) = (self.k, self.origin);
        let at = |v: i32, o: f64| ((f64::from(v) + o) * k).round() as i32;
        let (x0, y0, x1, y1) = (at(r.x, ox), at(r.y, oy), at(r.right(), ox), at(r.bottom(), oy));
        IRect::new(x0, y0, x1 - x0, y1 - y0).intersect(&IRect::new(0, 0, self.picture.width as i32, self.picture.height as i32))
    }

    pub(crate) fn ptr(&self) -> usize {
        Arc::as_ptr(&self.picture) as usize
    }

    /// The mean luma of the picture under `r`.
    pub(crate) fn luma(&self, r: IRect) -> f64 {
        self.picture.mean_luma(self.device(r), 2).unwrap_or(0.0)
    }

    pub(crate) fn same(&self, o: &Under) -> bool {
        let subject = match (&self.subject, &o.subject) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        Arc::ptr_eq(&self.picture, &o.picture) && self.k == o.k && self.origin == o.origin && subject
    }
}

/// One clock's digits as last drawn: the grid face's bitmap on its look,
/// and the subject's crop over them.
#[derive(Default)]
pub(crate) struct Glass {
    digits: Option<(Look, Bitmap)>,
    crop: Option<(IRect, (usize, usize), Bitmap)>,
}

impl Glass {
    /// Whether the digits last drawn were filled with the picture.
    pub(crate) fn frosted(&self) -> bool {
        self.digits.as_ref().is_some_and(|(l, _)| l.under.is_some())
    }

    /// Whether the subject was last laid over the digits.
    pub(crate) fn cut(&self) -> bool {
        self.crop.is_some()
    }

    /// `digits` painted with their top left at `at`, `digit_h` tall, in
    /// the surface's logical pixels; the subject laid back over them
    /// inside `bounds`. Returns what the digits cover, past their box where
    /// a serif figure overshoots its baseline.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(&mut self, p: &mut Painter, digits: Digits, f: &FaceSettings, at: (f64, f64), digit_h: f64, scale: f64, ink: Rgba, under: Option<&Under>, bounds: IRect) -> IRect {
        let (x, y) = (at.0.round() as i32, at.1.round() as i32);
        let mut drawn = IRect::new(x, y, digits.width().ceil() as i32, digit_h.ceil() as i32);
        match digits {
            Digits::Grid { text, device: px, w } => {
                let rect = IRect::new(x, y, w.ceil() as i32, digit_h.ceil() as i32);
                let look = Look { face: f.face, text, device: px, width: f.width, ink, scale, under: under.map(|u| (u.ptr(), u.device(rect))) };
                if self.digits.as_ref().is_none_or(|(l, _)| *l != look) {
                    self.digits = clock_face::render(look.face, &look.text, look.device, look.width).map(|mask| {
                        let filled = match under {
                            Some(u) => frost(&mask, ink, &u.picture, ((f64::from(x) + u.origin.0) * u.k, (f64::from(y) + u.origin.1) * u.k), u.k / scale),
                            None => tint(&mask, ink, FROST_ALPHA),
                        };
                        (look, filled.at(scale))
                    });
                }
                if let Some((_, bitmap)) = &self.digits {
                    p.image(bitmap, (x, y), 1.0);
                }
            }
            Digits::Serif { hours, colon, minutes, gap } => {
                self.digits = None;
                let baseline = at.1 + digit_h;
                let mut x = at.0;
                let mut put = |p: &mut Painter, t: &ShapedText, base: f64, x: &mut f64, after: f64| {
                    let at = (x.round() as i32, (base - f64::from(t.ascent)).round() as i32);
                    p.text(t, at, ink.with_alpha(FROST_ALPHA), &[]);
                    drawn = drawn.union(&IRect::new(at.0, at.1, t.width, t.line_height()));
                    *x += f64::from(t.width) + after;
                };
                put(p, &hours, baseline, &mut x, gap);
                put(p, &colon, baseline - digit_h * COLON_RAISE, &mut x, gap);
                put(p, &minutes, baseline, &mut x, 0.0);
            }
        }
        // The subject laid back over the box.
        let over = drawn.intersect(&bounds);
        let px = under.filter(|u| u.subject.is_some()).map_or(IRect::default(), |u| u.device(over));
        match under.and_then(|u| u.subject.as_ref().map(|s| (u, s))) {
            Some((u, sub)) if !px.is_empty() => {
                // A new depth map or cut over the same picture is a new subject.
                let from = (u.ptr(), Arc::as_ptr(sub) as usize);
                if self.crop.as_ref().is_none_or(|(r, f, _)| *r != px || *f != from) {
                    self.crop = Some((px, from, cut_out(&u.picture, &sub.mask, px).at(u.k)));
                }
                if let Some((_, _, b)) = &self.crop {
                    p.image(b, (over.x, over.y), 1.0);
                }
            }
            _ => self.crop = None,
        }
        drawn
    }
}

/// The digits laid out: their logical size, and what draws them.
pub(crate) enum Digits {
    Grid { w: f64, text: String, device: u32 },
    Serif { hours: ShapedText, colon: ShapedText, minutes: ShapedText, gap: f64 },
}

impl Digits {
    /// `hours` and `minutes` in `f`'s face, `digit_h` tall at `scale`. The
    /// grid faces draw the figures alone, set close as one run.
    pub(crate) fn lay(kit: &mut Kit, f: &FaceSettings, hours: &str, minutes: &str, digit_h: f64, scale: f64) -> Self {
        if f.face == Face::Serif {
            let font = (digit_h / SERIF_FIGURE) as f32;
            let tracking = if f.width == Width::Wide { (digit_h * 0.03) as f32 } else { 0.0 };
            let style = |size: f32| TextStyle {
                family: Family::Generic(parley::GenericFamily::Serif),
                size,
                weight: 400.0,
                tracking,
                wdth: TextStyle::NORMAL_WIDTH,
                wght: None,
            };
            Digits::Serif {
                hours: kit.shape(hours, style(font)),
                colon: kit.shape(":", style(font * 0.6)),
                minutes: kit.shape(minutes, style(font)),
                gap: digit_h * 0.06,
            }
        } else {
            let text = format!("{hours}{minutes}");
            let device = (digit_h * scale).round().max(1.0) as u32;
            Digits::Grid { w: f64::from(clock_face::measure(&text, device, f.width)) / scale, text, device }
        }
    }

    pub(crate) fn width(&self) -> f64 {
        match self {
            Digits::Grid { w, .. } => *w,
            Digits::Serif { hours, colon, minutes, gap } => f64::from(hours.width + colon.width + minutes.width) + gap * 2.0,
        }
    }
}

/// A serif face's figures stand this share of its size.
const SERIF_FIGURE: f64 = 0.7;
/// How far the serif colon sits over the figures' baseline, of their height.
const COLON_RAISE: f64 = 0.16;

/// How opaque the digits are, and how far their fill is drawn from the
/// blurred picture toward the ink.
const FROST_ALPHA: f32 = 0.94;
const FROST_INK: f32 = 0.5;
/// The blur's radius, of the digits' height.
const FROST_BLUR: f64 = 0.14;

/// A coverage mask filled with `ink` at `alpha`, premultiplied.
fn tint(mask: &clock_face::Mask, ink: Rgba, alpha: f32) -> Bitmap {
    let [r, g, b, a] = [ink.r, ink.g, ink.b, ink.a * alpha].map(|v| v.clamp(0.0, 1.0));
    let mut rgba = Vec::with_capacity(mask.data.len() * 4);
    for &c in &mask.data {
        let k = f32::from(c) / 255.0 * a;
        rgba.extend([r * k, g * k, b * k, k].map(|v| (v * 255.0).round() as u8));
    }
    Bitmap::from_premultiplied(mask.width as u16, mask.height as u16, rgba)
}

/// A coverage mask filled with the picture under it blurred and drawn
/// toward `ink`, frosted glass rather than paint. `origin` is the mask's
/// top-left in the picture's pixels and `k` the picture's pixels per mask
/// pixel. The blur runs at half the mask's density, two box passes each way.
fn frost(mask: &clock_face::Mask, ink: Rgba, p: &Picture, origin: (f64, f64), k: f64) -> Bitmap {
    let (mw, mh) = (mask.width as usize, mask.height as usize);
    let r = ((mh as f64 * FROST_BLUR) / 2.0).ceil().max(1.0) as usize;
    let (w, h) = (mw.div_ceil(2) + r * 2, mh.div_ceil(2) + r * 2);
    let mut rgb = vec![0f32; w * h * 3];
    let (pw, ph) = (p.width as i64, p.height as i64);
    for y in 0..h {
        let py = ((origin.1 + ((y as f64 - r as f64) * 2.0 + 1.0) * k) as i64).clamp(0, ph - 1);
        for x in 0..w {
            let px = ((origin.0 + ((x as f64 - r as f64) * 2.0 + 1.0) * k) as i64).clamp(0, pw - 1);
            let i = (py * pw + px) as usize * 4;
            let o = (y * w + x) * 3;
            if let Some(c) = p.bgra.get(i..i + 3) {
                rgb[o..o + 3].copy_from_slice(&[f32::from(c[2]), f32::from(c[1]), f32::from(c[0])]);
            }
        }
    }
    for _ in 0..2 {
        box_pass(&mut rgb, w, h, r, 3, w * 3);
        box_pass(&mut rgb, h, w, r, w * 3, 3);
    }
    let [ir, ig, ib] = [ink.r, ink.g, ink.b].map(|v| v.clamp(0.0, 1.0) * 255.0);
    let mut out = Vec::with_capacity(mw * mh * 4);
    for y in 0..mh {
        for x in 0..mw {
            let a = f32::from(mask.data[y * mw + x]) / 255.0 * FROST_ALPHA;
            let o = ((y / 2 + r) * w + x / 2 + r) * 3;
            let mix = |c: f32, i: f32| ((c + (i - c) * FROST_INK) * a).round() as u8;
            out.extend([mix(rgb[o], ir), mix(rgb[o + 1], ig), mix(rgb[o + 2], ib), (a * 255.0).round() as u8]);
        }
    }
    Bitmap::from_premultiplied(mw as u16, mh as u16, out)
}

/// One box blur of radius `r` along every line of `n` samples, `step`
/// apart, the lines `stride` apart, edges held.
fn box_pass(buf: &mut [f32], n: usize, lines: usize, r: usize, step: usize, stride: usize) {
    let mut line = vec![0f32; n];
    let span = (r * 2 + 1) as f32;
    for l in 0..lines {
        for c in 0..3 {
            let at = |i: usize| l * stride + i * step + c;
            for (i, v) in line.iter_mut().enumerate() {
                *v = buf[at(i)];
            }
            let get = |i: isize| line[i.clamp(0, n as isize - 1) as usize];
            let mut sum: f32 = (-(r as isize)..=r as isize).map(get).sum();
            for i in 0..n {
                buf[at(i)] = sum / span;
                sum += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
            }
        }
    }
}

/// The picture over `r` with the subject's coverage as its alpha.
fn cut_out(p: &Picture, mask: &[u8], r: IRect) -> Bitmap {
    let mut rgba = Vec::with_capacity((r.w * r.h * 4) as usize);
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            let i = y as usize * p.width as usize + x as usize;
            let a = u32::from(mask.get(i).copied().unwrap_or(0));
            let px = p.bgra.get(i * 4..i * 4 + 3).unwrap_or(&[0, 0, 0]);
            let pre = |c: u8| ((u32::from(c) * a + 127) / 255) as u8;
            rgba.extend([pre(px[2]), pre(px[1]), pre(px[0]), a as u8]);
        }
    }
    Bitmap::from_premultiplied(r.w as u16, r.h as u16, rgba)
}
