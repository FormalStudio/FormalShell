//! The desktop clock (`desktopClock.*`): a Background layer on every output,
//! stacked over the wallpaper and the sculpture, clear but for the clock's
//! box. The time is drawn there and the wallpaper's subject laid back over
//! it, cut out of the picture by its depth map, so the clock sits behind
//! whatever the map puts in front; with no `desktopClock.y` the clock is
//! placed so the subject's top edge crosses its lower part.

use std::sync::Arc;
use std::time::Instant;

use calloop::channel::{self, Sender};
use fs_chrome::clock_face::{self, Face, Width};
use fs_theme::color::Rgba;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::wallpaper::{DepthCut, Picture};
use crate::store::{Store, Topic};
use crate::surface::Surface;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{Family, ShapedText, TextStyle};

/// The App's share: the pool's channel back, a count that moves when a
/// depth map is written, so every subject cut before it is cut again, and
/// the subject the sidebar's clock card is cut by.
#[derive(Default)]
pub(super) struct State {
    tx: Option<Sender<Msg>>,
    generation: u64,
    sidebar: Slot,
}

struct Msg {
    key: Key,
    subject: Option<Arc<Subject>>,
}

/// A subject's coverage, one byte per picture pixel, and each column's
/// first row at least half covered (the picture's height for none).
pub(crate) struct Subject {
    mask: Vec<u8>,
    top: Vec<u32>,
}

impl Subject {
    fn new(mask: Vec<u8>, width: u32, height: u32) -> Self {
        let w = width as usize;
        let top = (0..w).map(|x| (0..height).find(|&y| mask.get(y as usize * w + x).is_some_and(|&a| a >= 128)).unwrap_or(height)).collect();
        Self { mask, top }
    }

    /// The median top edge over columns `x0..x1`, none where most of them
    /// carry no subject.
    fn edge(&self, x0: usize, x1: usize, height: u32) -> Option<u32> {
        let mut tops: Vec<u32> = self.top.get(x0..x1.min(self.top.len()))?.to_vec();
        if tops.is_empty() {
            return None;
        }
        let mid = tops.len() / 2;
        let (_, m, _) = tops.select_nth_unstable(mid);
        (*m < height).then_some(*m)
    }
}

/// What a subject is cut from.
#[derive(Clone)]
struct Key {
    picture: Arc<Picture>,
    cut: DepthCut,
    generation: u64,
}

impl Key {
    fn same(&self, o: &Key) -> bool {
        Arc::ptr_eq(&self.picture, &o.picture) && self.cut == o.cut && self.generation == o.generation
    }
}

/// One clock's subject: the last one cut, and the one asked of the pool.
#[derive(Default)]
struct Slot {
    subject: Option<(Key, Option<Arc<Subject>>)>,
    asked: Option<Key>,
}

impl Slot {
    /// The subject when depth is on and its map was found.
    fn mask(&self, cut: Option<DepthCut>) -> Option<&Arc<Subject>> {
        let cut = cut?;
        self.subject.as_ref().filter(|(k, _)| k.cut == cut).and_then(|(_, m)| m.as_ref())
    }

    /// True when `key` is neither held nor asked for, which asks for it.
    fn want(&mut self, key: &Key) -> bool {
        let held = self.subject.as_ref().is_some_and(|(k, _)| k.same(key)) || self.asked.as_ref().is_some_and(|k| k.same(key));
        if !held {
            self.asked = Some(key.clone());
        }
        !held
    }

    /// True when `msg` answers what this slot asked.
    fn land(&mut self, msg: &Msg) -> bool {
        if !self.asked.as_ref().is_some_and(|k| k.same(&msg.key)) {
            return false;
        }
        self.asked = None;
        self.subject = Some((msg.key.clone(), msg.subject.clone()));
        true
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
            face: config.str("desktopClock.face").and_then(Face::parse).unwrap_or(Face::Pixel),
            width: config.str("desktopClock.width").and_then(Width::parse).unwrap_or(Width::Wide),
            color: config.str("desktopClock.color").unwrap_or("auto").to_owned(),
            cut: DepthCut::read(config, "desktopClock", None),
        }
    }

    /// The ink `color` names; none for `auto`, which reads what is behind.
    pub(crate) fn ink(&self, store: &Store) -> Option<Rgba> {
        match self.color.as_str() {
            "auto" => None,
            "primary" => Some(store.theme.theme.colors.get("primary")),
            hex => Some(Rgba::parse(hex).unwrap_or(Rgba::WHITE)),
        }
    }
}

/// `auto`'s ink over what reads `luma` (0..255) behind the digits.
pub(crate) fn auto_ink(luma: f64) -> Rgba {
    if luma > fs_theme::barpaint::LUMA_THRESHOLD { Rgba::BLACK } else { Rgba::WHITE }
}

#[derive(Clone, PartialEq)]
struct Settings {
    look: FaceSettings,
    size: f64,
    x: f64,
    /// None places the clock off the subject.
    y: Option<f64>,
    hour12: bool,
}

impl Settings {
    /// None while `desktopClock.enabled` is off.
    fn read(config: &crate::services::config::State) -> Option<Self> {
        if config.bool("desktopClock.enabled") != Some(true) {
            return None;
        }
        let num = |path: &str, fallback: f64, lo: f64, hi: f64| config.f64(path).filter(|n| n.is_finite()).map_or(fallback, |n| n.clamp(lo, hi));
        Some(Self {
            look: FaceSettings::read(config),
            size: num("desktopClock.size", 0.18, 0.02, 0.9),
            x: num("desktopClock.x", 0.5, 0.0, 1.0),
            y: config.f64("desktopClock.y").filter(|n| n.is_finite()).map(|n| n.clamp(0.0, 1.0)),
            hour12: config.bool("desktopClock.hour12").unwrap_or(false),
        })
    }
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
    fn device(&self, r: IRect) -> IRect {
        let (k, (ox, oy)) = (self.k, self.origin);
        let at = |v: i32, o: f64| ((f64::from(v) + o) * k).round() as i32;
        let (x0, y0, x1, y1) = (at(r.x, ox), at(r.y, oy), at(r.right(), ox), at(r.bottom(), oy));
        IRect::new(x0, y0, x1 - x0, y1 - y0).intersect(&IRect::new(0, 0, self.picture.width as i32, self.picture.height as i32))
    }

    fn ptr(&self) -> usize {
        Arc::as_ptr(&self.picture) as usize
    }

    /// The mean luma of the picture under `r`.
    fn luma(&self, r: IRect) -> f64 {
        mean_luma(&self.picture, self.device(r))
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

/// One output's clock.
pub(super) struct Clock {
    pub surface: Surface,
    scene: Scene,
    nodes: Vec<NodeId>,
    /// The output's sculpture was up when this layer was made. One made
    /// later stacks over it, so the layer is made again then.
    pub over_shards: bool,
    face: Face,
    /// The box the clock draws in, at rest.
    rect: IRect,
    ink: Rgba,
    /// What the ink was read off: the box in the picture's pixels, and the
    /// picture.
    ink_from: Option<(IRect, usize)>,
    glass: Glass,
    slot: Slot,
    /// The time last drawn, and what that draw cost: the minute's redraw.
    time: String,
    minute_us: u64,
}

impl Clock {
    fn new(surface: Surface, over_shards: bool) -> Self {
        Self {
            surface,
            scene: Scene::clear(1, 1),
            nodes: Vec::new(),
            over_shards,
            face: Face::Pixel,
            rect: IRect::default(),
            ink: Rgba::WHITE,
            ink_from: None,
            glass: Glass::default(),
            slot: Slot::default(),
            time: String::new(),
            minute_us: 0,
        }
    }

    pub fn configure(&mut self, width: i32, height: i32) {
        if width > 0 && height > 0 {
            self.scene.resize(width, height);
            self.surface.configure(width, height);
        }
    }
}

impl App {
    fn desktop_clock_channel(&mut self) -> Option<Sender<Msg>> {
        if let Some(tx) = &self.desktop_clock.tx {
            return Some(tx.clone());
        }
        let handle = self.handle.as_ref()?;
        let (tx, rx) = channel::channel::<Msg>();
        handle
            .insert_source(rx, |event, _, app: &mut App| {
                if let channel::Event::Msg(msg) = event {
                    app.desktop_clock_subject(msg);
                }
            })
            .ok()?;
        self.desktop_clock.tx = Some(tx.clone());
        Some(tx)
    }

    fn desktop_clock_subject(&mut self, msg: Msg) {
        let mut landed = self.desktop_clock.sidebar.land(&msg);
        for c in self.heads.iter_mut().filter_map(|h| h.clock.as_mut()) {
            landed |= c.slot.land(&msg);
        }
        if landed {
            self.sync_desktop_clocks();
        }
    }

    pub fn desktop_clock_changed(&mut self, topic: Topic) {
        if topic == Topic::Depth {
            self.desktop_clock.generation += 1;
        }
    }

    /// Each output's clock made or dropped as the settings and its output's
    /// state ask, and drawn; the sidebar's clock card handed what it
    /// frosts over.
    pub(super) fn sync_desktop_clocks(&mut self) {
        let settings = Settings::read(&self.store.config);
        let mut asks = Vec::new();
        for i in 0..self.heads.len() {
            let output = self.heads[i].output.clone();
            let name = self.output_name(&output);
            let shards = self.heads[i].backdrop.as_ref().is_some_and(|b| b.shards.is_some());
            let want = settings.is_some() && self.heads[i].backdrop.is_some() && !self.hidden_on(&name);
            let stale = self.heads[i].clock.as_ref().is_some_and(|c| shards && !c.over_shards);
            if (!want || stale) && self.heads[i].clock.take().is_some() {
                self.log(&format!("desktop clock off {name}"));
            }
            let Some(settings) = &settings else { continue };
            if !want {
                continue;
            }
            if self.heads[i].clock.is_none() {
                let layer = self.overlay_on("formalshell:desktop-clock", Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
                let surface = Surface::on("desktop-clock", layer, &self.shm, self.started, &output);
                self.heads[i].clock = Some(Clock::new(surface, shards));
                self.log(&format!("desktop clock on {name}"));
            }
            let picture = self.desktop_clock_picture(i);
            let Some(clock) = self.heads[i].clock.as_mut() else { continue };
            draw(clock, &mut self.bar.kit, &self.store, settings, picture.as_ref());
            if let (Some(cut), Some(picture)) = (settings.look.cut, picture) {
                let key = Key { picture, cut, generation: self.desktop_clock.generation };
                if clock.slot.want(&key) {
                    asks.push(key);
                }
            }
        }
        if let Some(key) = self.sync_sidebar_clock() {
            asks.push(key);
        }
        if asks.is_empty() {
            return;
        }
        let Some(tx) = self.desktop_clock_channel() else { return };
        let Some(rt) = &self.runtime else { return };
        for key in asks {
            let tx = tx.clone();
            rt.pool().submit(move || {
                let p = &key.picture;
                let subject = crate::services::wallpaper::depth_mask(&p.path, p.width, p.height, key.cut).map(|m| Arc::new(Subject::new(m, p.width, p.height)));
                let _ = tx.send(Msg { key, subject });
            });
        }
    }

    /// The sidebar's clock card handed the face keys and the bar's output's
    /// picture under the bar window; the subject to ask for, if any.
    fn sync_sidebar_clock(&mut self) -> Option<Key> {
        if self.bar.sidebar.is_none() {
            self.desktop_clock.sidebar = Slot::default();
            return None;
        }
        let face = FaceSettings::read(&self.store.config);
        let head = self.heads.iter().position(|h| Some(&h.output) == self.bar_wl.as_ref());
        let picture = head.and_then(|i| self.desktop_clock_picture(i));
        let (out_w, _) = self.output_size();
        let origin = self.bar_origin();
        let mut ask = None;
        if let (Some(cut), Some(p)) = (face.cut, &picture) {
            let key = Key { picture: p.clone(), cut, generation: self.desktop_clock.generation };
            if self.desktop_clock.sidebar.want(&key) {
                ask = Some(key);
            }
        }
        let under = picture.filter(|_| out_w > 0.0).map(|picture| Under {
            k: f64::from(picture.width) / out_w,
            subject: self.desktop_clock.sidebar.mask(face.cut).cloned(),
            picture,
            origin,
        });
        if let Some(sb) = &mut self.bar.sidebar
            && sb.set_clock(under)
        {
            self.bar_dirty = true;
        }
        ask
    }

    fn desktop_clock_picture(&self, i: usize) -> Option<Arc<Picture>> {
        let (w, h) = self.heads[i].backdrop.as_ref().and_then(|b| b.pixels())?;
        let wanted = self.store.desktop();
        self.store.wallpaper.sized(w as u32, h as u32).filter(|p| p.path == wanted).cloned()
    }

    /// Head `i`'s clock holds its frame: its output fullscreen, the session
    /// locked or under the screensaver.
    pub(super) fn desktop_clock_held(&self, i: usize) -> bool {
        let name = self.output_name(&self.heads[i].output);
        self.lock.locked || self.saver.active || self.store.compositor.snapshot.fullscreen_outputs.contains(&name)
    }

    pub(super) fn present_desktop_clocks(&mut self) {
        let qh = self.qh.clone();
        for i in 0..self.heads.len() {
            if self.desktop_clock_held(i) {
                continue;
            }
            if let Some(c) = self.heads[i].clock.as_mut() {
                c.surface.present(&mut c.scene, false, &qh);
            }
        }
    }

    /// Each output's clock for `debug dump`.
    pub(crate) fn desktop_clock_state(&self) -> serde_json::Value {
        let settings = Settings::read(&self.store.config);
        let heads: Vec<serde_json::Value> = self
            .heads
            .iter()
            .map(|h| {
                let c = h.clock.as_ref();
                let r = c.map_or(IRect::default(), |c| c.rect);
                let ink = c.map_or(Rgba::TRANSPARENT, |c| c.ink);
                serde_json::json!({
                    "output": self.output_name(&h.output),
                    "enabled": c.is_some(),
                    "face": c.map_or("", |c| c.face.as_str()),
                    "rect": [r.x, r.y, r.w, r.h],
                    "ink": hex(ink),
                    "depth": c.is_some_and(|c| c.slot.mask(settings.as_ref().and_then(|s| s.look.cut)).is_some()),
                    "commits": c.map_or(0, |c| c.surface.commits()),
                    "minuteUs": c.map_or(0, |c| c.minute_us),
                })
            })
            .collect();
        serde_json::Value::Array(heads)
    }

    /// The sidebar's clock card for `debug dump`, null under the bar.
    pub(crate) fn sidebar_clock_state(&self) -> serde_json::Value {
        let Some(sb) = &self.bar.sidebar else { return serde_json::Value::Null };
        let mut v = sb.clock_state(&self.store);
        v["origin"] = serde_json::json!(self.bar_origin());
        v["commits"] = serde_json::json!(self.bar_surface.as_ref().map_or(0, |s| s.commits()));
        v
    }
}

/// `#rrggbb` for `debug dump`.
pub(crate) fn hex(c: Rgba) -> String {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c.r), b(c.g), b(c.b))
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

fn draw(clock: &mut Clock, kit: &mut Kit, store: &Store, s: &Settings, picture: Option<&Arc<Picture>>) {
    let size = clock.scene.size;
    if size.w <= 1 || size.h <= 1 {
        return;
    }
    let started = Instant::now();
    let scale = clock.surface.scale();
    let (sw, sh) = (f64::from(size.w), f64::from(size.h));
    let digit_h = (s.size * sh).max(8.0);
    let now = store.clock.now;
    let time = now.format(if s.hour12 { "%I%M" } else { "%H%M" }).to_string();
    let new_minute = time != clock.time;
    if new_minute {
        clock.time.clone_from(&time);
    }
    let digits = Digits::lay(kit, &s.look, &time[..2], &time[2..], digit_h, scale);
    let digits_w = digits.width();
    let (box_w, box_h) = (digits_w, digit_h);
    let cx = s.x * sw;
    let k = picture.map_or(scale, |p| f64::from(p.width) / sw);
    let subject = picture.and(clock.slot.mask(s.look.cut).cloned());
    let top = match (s.y, picture, &subject) {
        (Some(y), ..) => y * sh - box_h / 2.0,
        (None, Some(p), Some(sub)) => {
            let col = |x: f64| (x * k).round().clamp(0.0, f64::from(p.width)) as usize;
            sub.edge(col(cx - box_w / 2.0), col(cx + box_w / 2.0), p.height)
                .map_or(sh * AUTO_Y - box_h / 2.0, |e| f64::from(e) / k - box_h * (1.0 - UNDER))
        }
        (None, ..) => sh * AUTO_Y - box_h / 2.0,
    };
    let top = top.min(sh - box_h).max(sh * 0.06);
    let rect = IRect::new((cx - box_w / 2.0).round() as i32, top.round() as i32, box_w.ceil() as i32, box_h.ceil() as i32);
    clock.face = s.look.face;
    clock.rect = rect;

    let under = picture.map(|p| Under { picture: p.clone(), k, origin: (0.0, 0.0), subject });
    clock.ink = match s.look.ink(store) {
        Some(ink) => ink,
        None => {
            let from = under.as_ref().map(|u| (u.device(rect), u.ptr()));
            if from.is_none() || from != clock.ink_from {
                let luma = match &under {
                    Some(u) => u.luma(rect),
                    None => luma(store.theme.theme.colors.get("background")),
                };
                clock.ink_from = from;
                clock.ink = auto_ink(luma);
            }
            clock.ink
        }
    };
    let ink = clock.ink;
    let mut p = Painter::new(&mut clock.scene, &mut clock.nodes, None);
    let bounds = IRect::new(0, 0, size.w, size.h);
    clock.glass.paint(&mut p, digits, &s.look, (cx - digits_w / 2.0, f64::from(rect.y)), digit_h, scale, ink, under.as_ref(), bounds);
    p.finish();
    if new_minute {
        clock.minute_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    }
}

/// A serif face's figures stand this share of its size.
const SERIF_FIGURE: f64 = 0.7;
/// How far the serif colon sits over the figures' baseline, of their height.
const COLON_RAISE: f64 = 0.16;
/// The share of the digits a placed clock hides behind the subject's edge.
const UNDER: f64 = 0.2;
/// Where a clock with no subject to sit behind is centred, of the height.
const AUTO_Y: f64 = 0.3;

pub(crate) fn luma(c: Rgba) -> f64 {
    255.0 * (0.299 * f64::from(c.r) + 0.587 * f64::from(c.g) + 0.114 * f64::from(c.b))
}

/// The mean Rec. 601 luma of `p` over `r`, every other pixel each way.
fn mean_luma(p: &Picture, r: IRect) -> f64 {
    let (mut sum, mut n) = (0.0, 0u64);
    for y in (r.y..r.bottom()).step_by(2) {
        for x in (r.x..r.right()).step_by(2) {
            let i = ((y as usize) * p.width as usize + x as usize) * 4;
            let Some(px) = p.bgra.get(i..i + 3) else { continue };
            sum += 0.114 * f64::from(px[0]) + 0.587 * f64::from(px[1]) + 0.299 * f64::from(px[2]);
            n += 1;
        }
    }
    if n == 0 { 0.0 } else { sum / n as f64 }
}

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
