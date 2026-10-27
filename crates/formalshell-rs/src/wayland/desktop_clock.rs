//! The desktop clock (`desktopClock.*`): a Background layer on every output,
//! stacked over the wallpaper and the sculpture, clear but for the clock's
//! box. The time and the date line are drawn there and the wallpaper's
//! subject laid back over them, cut out of the picture by its depth map, so
//! the clock sits behind whatever the map puts in front. The layer takes
//! the pointer over bare desktop, and the clock leans toward it.

use std::sync::Arc;
use std::time::Instant;

use calloop::channel::{self, Sender};
use fs_chrome::clock_face::{self, Face, Width};
use fs_theme::color::Rgba;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::motion::{Animated, Kind};
use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::wallpaper::{DepthCut, Picture};
use crate::store::{Store, Topic};
use crate::surface::Surface;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{Family, ShapedText, TextStyle};

/// The App's share: the pool's channel back, and a count that moves when a
/// depth map is written, so every subject cut before it is cut again.
#[derive(Default)]
pub(super) struct State {
    tx: Option<Sender<Msg>>,
    generation: u64,
}

struct Msg {
    key: Key,
    mask: Option<Arc<Vec<u8>>>,
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

#[derive(Clone, PartialEq)]
struct Settings {
    face: Face,
    size: f64,
    width: Width,
    x: f64,
    y: f64,
    date: bool,
    hour12: bool,
    color: String,
    parallax: f64,
    cut: Option<DepthCut>,
}

impl Settings {
    /// None while `desktopClock.enabled` is off.
    fn read(config: &crate::services::config::State) -> Option<Self> {
        if config.bool("desktopClock.enabled") != Some(true) {
            return None;
        }
        let num = |path: &str, fallback: f64, lo: f64, hi: f64| config.f64(path).filter(|n| n.is_finite()).map_or(fallback, |n| n.clamp(lo, hi));
        Some(Self {
            face: config.str("desktopClock.face").and_then(Face::parse).unwrap_or(Face::Pixel),
            size: num("desktopClock.size", 0.18, 0.02, 0.9),
            width: config.str("desktopClock.width").and_then(Width::parse).unwrap_or(Width::Wide),
            x: num("desktopClock.x", 0.5, 0.0, 1.0),
            y: num("desktopClock.y", 0.3, 0.0, 1.0),
            date: config.bool("desktopClock.date").unwrap_or(true),
            hour12: config.bool("desktopClock.hour12").unwrap_or(false),
            color: config.str("desktopClock.color").unwrap_or("auto").to_owned(),
            parallax: num("desktopClock.parallax", 12.0, 0.0, 200.0),
            cut: DepthCut::read(config, "desktopClock.depth"),
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
}

/// One output's clock.
pub(super) struct Clock {
    pub surface: Surface,
    scene: Scene,
    nodes: Vec<NodeId>,
    /// The output's sculpture was up when this layer was made. One made
    /// later stacks over it, so the layer is made again then.
    pub over_shards: bool,
    /// The layer takes the pointer (`desktopClock.parallax` above 0).
    input: bool,
    face: Face,
    /// The box the clock draws in, at rest.
    rect: IRect,
    /// Where the clock sits off `rect`, x and y, on the spatial family.
    glide: [Animated; 2],
    /// The offset the scene was last drawn at.
    offset: (f64, f64),
    ink: Rgba,
    /// What the ink was read off: the box in the picture's pixels, and the
    /// picture.
    ink_from: Option<(IRect, usize)>,
    digits: Option<(Look, Bitmap)>,
    subject: Option<(Key, Option<Arc<Vec<u8>>>)>,
    asked: Option<Key>,
    /// The subject over the box, in the picture's pixels.
    crop: Option<(IRect, usize, Bitmap)>,
    /// The time last drawn, and what that draw cost: the minute's redraw.
    time: String,
    minute_us: u64,
}

impl Clock {
    fn new(surface: Surface, over_shards: bool, input: bool) -> Self {
        Self {
            surface,
            scene: Scene::clear(1, 1),
            nodes: Vec::new(),
            over_shards,
            input,
            face: Face::Pixel,
            rect: IRect::default(),
            glide: [Animated::new(0.0, Kind::Spatial.curve()), Animated::new(0.0, Kind::Spatial.curve())],
            offset: (0.0, 0.0),
            ink: Rgba::WHITE,
            ink_from: None,
            digits: None,
            subject: None,
            asked: None,
            crop: None,
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

    fn at(&self, now: Instant) -> (f64, f64) {
        (self.glide[0].value(now), self.glide[1].value(now))
    }

    /// The glide has somewhere left to go from what was drawn.
    fn moving(&self, now: Instant) -> bool {
        self.glide.iter().any(|g| g.running(now)) || self.at(now) != self.offset
    }

    /// The subject's coverage when depth is on and its map was found.
    fn mask(&self, cut: Option<DepthCut>) -> Option<&Arc<Vec<u8>>> {
        let cut = cut?;
        self.subject.as_ref().filter(|(k, _)| k.cut == cut).and_then(|(_, m)| m.as_ref())
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
        let mut landed = false;
        for c in self.heads.iter_mut().filter_map(|h| h.clock.as_mut()) {
            if c.asked.as_ref().is_some_and(|k| k.same(&msg.key)) {
                c.asked = None;
                c.subject = Some((msg.key.clone(), msg.mask.clone()));
                c.crop = None;
                landed = true;
            }
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
    /// state ask, and drawn.
    pub(super) fn sync_desktop_clocks(&mut self) {
        let settings = Settings::read(&self.store.config);
        let mut asks = Vec::new();
        for i in 0..self.heads.len() {
            let output = self.heads[i].output.clone();
            let name = self.output_name(&output);
            let shards = self.heads[i].backdrop.as_ref().is_some_and(|b| b.shards.is_some());
            let want = settings.is_some() && self.heads[i].backdrop.is_some() && !self.hidden_on(&name);
            let input = settings.as_ref().is_some_and(|s| s.parallax > 0.0);
            let stale = self.heads[i].clock.as_ref().is_some_and(|c| (shards && !c.over_shards) || c.input != input);
            if (!want || stale) && self.heads[i].clock.take().is_some() {
                self.log(&format!("desktop clock off {name}"));
            }
            let Some(settings) = &settings else { continue };
            if !want {
                continue;
            }
            if self.heads[i].clock.is_none() {
                let layer = self.overlay_on("formalshell:desktop-clock", Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
                // The whole output: the pointer reaches it wherever no
                // window or other layer is over the desktop.
                if input {
                    layer.set_input_region(None);
                }
                let surface = Surface::on("desktop-clock", layer, &self.shm, self.started, &output);
                self.heads[i].clock = Some(Clock::new(surface, shards, input));
                self.log(&format!("desktop clock on {name}"));
            }
            let picture = self.desktop_clock_picture(i);
            let Some(clock) = self.heads[i].clock.as_mut() else { continue };
            draw(clock, &mut self.bar.kit, &self.store, settings, picture.as_ref(), Instant::now());
            if let (Some(cut), Some(picture)) = (settings.cut, picture) {
                let key = Key { picture, cut, generation: self.desktop_clock.generation };
                let held = clock.subject.as_ref().is_some_and(|(k, _)| k.same(&key)) || clock.asked.as_ref().is_some_and(|k| k.same(&key));
                if !held {
                    clock.asked = Some(key.clone());
                    asks.push(key);
                }
            }
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
                let mask = crate::services::wallpaper::depth_mask(&p.path, p.width, p.height, key.cut).map(Arc::new);
                let _ = tx.send(Msg { key, mask });
            });
        }
    }

    fn desktop_clock_picture(&self, i: usize) -> Option<Arc<Picture>> {
        let (w, h) = self.heads[i].backdrop.as_ref().and_then(|b| b.pixels())?;
        let wanted = self.store.desktop();
        self.store.wallpaper.sized(w as u32, h as u32).filter(|p| p.path == wanted).cloned()
    }

    /// The pointer over head `i`'s bare desktop: the clock heads toward it,
    /// `desktopClock.parallax` px at the output's edge. Leave holds it there.
    pub(super) fn desktop_clock_pointer(&mut self, i: usize, e: &PointerEvent) {
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.cursor = Some((serial, Shape::Default));
                self.pointer_on = None;
                self.hover(None, e.position);
                self.set_cursor(true);
            }
            PointerEventKind::Motion { .. } => {}
            _ => return,
        }
        let Some(s) = Settings::read(&self.store.config) else { return };
        let ms = Kind::Spatial.ms(&self.store.theme.theme);
        let Some(c) = self.heads.get_mut(i).and_then(|h| h.clock.as_mut()) else { return };
        let (hw, hh) = (f64::from(c.scene.size.w) / 2.0, f64::from(c.scene.size.h) / 2.0);
        if hw < 1.0 || hh < 1.0 {
            return;
        }
        let lean = |at: f64, half: f64| ((at - half) / half).clamp(-1.0, 1.0) * s.parallax;
        let now = Instant::now();
        c.glide[0].set(now, lean(e.position.0, hw), ms);
        c.glide[1].set(now, lean(e.position.1, hh), ms);
    }

    /// Head `i`'s clock holds its frame: its output fullscreen, the session
    /// locked or under the screensaver.
    pub(super) fn desktop_clock_held(&self, i: usize) -> bool {
        let name = self.output_name(&self.heads[i].output);
        self.lock.locked || self.saver.active || self.store.compositor.snapshot.fullscreen_outputs.contains(&name)
    }

    /// A gliding clock is drawn again once per frame callback, and asks
    /// for none once it lands.
    pub(super) fn present_desktop_clocks(&mut self) {
        let qh = self.qh.clone();
        let now = Instant::now();
        let mut settings = None;
        for i in 0..self.heads.len() {
            if self.heads[i].clock.is_none() || self.desktop_clock_held(i) {
                continue;
            }
            let moving = self.heads[i].clock.as_ref().is_some_and(|c| c.moving(now));
            if moving && self.heads[i].clock.as_ref().is_some_and(|c| !c.surface.frame_pending) {
                let Some(s) = settings.get_or_insert_with(|| Settings::read(&self.store.config)).clone() else { return };
                let picture = self.desktop_clock_picture(i);
                if let Some(c) = self.heads[i].clock.as_mut() {
                    draw(c, &mut self.bar.kit, &self.store, &s, picture.as_ref(), now);
                }
            }
            if let Some(c) = self.heads[i].clock.as_mut() {
                c.surface.present(&mut c.scene, moving, &qh);
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
                let hex = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                serde_json::json!({
                    "output": self.output_name(&h.output),
                    "enabled": c.is_some(),
                    "face": c.map_or("", |c| c.face.as_str()),
                    "rect": [r.x, r.y, r.w, r.h],
                    "offset": c.map_or([0.0, 0.0], |c| [c.offset.0, c.offset.1]),
                    "target": c.map_or([0.0, 0.0], |c| [c.glide[0].target(), c.glide[1].target()]),
                    "input": c.is_some_and(|c| c.input),
                    "ink": format!("#{:02x}{:02x}{:02x}", hex(ink.r), hex(ink.g), hex(ink.b)),
                    "depth": c.is_some_and(|c| c.mask(settings.as_ref().and_then(|s| s.cut)).is_some()),
                    "commits": c.map_or(0, |c| c.surface.commits()),
                    "minuteUs": c.map_or(0, |c| c.minute_us),
                })
            })
            .collect();
        serde_json::Value::Array(heads)
    }
}

/// The digits laid out: their logical size, and what draws them.
enum Digits {
    Grid { w: f64, text: String, device: u32 },
    Serif { hours: ShapedText, colon: ShapedText, minutes: ShapedText, gap: f64 },
}

fn draw(clock: &mut Clock, kit: &mut Kit, store: &Store, s: &Settings, picture: Option<&Arc<Picture>>, now: Instant) {
    let size = clock.scene.size;
    if size.w <= 1 || size.h <= 1 {
        return;
    }
    let started = Instant::now();
    let scale = clock.surface.scale();
    let (sw, sh) = (f64::from(size.w), f64::from(size.h));
    let digit_h = (s.size * sh).max(8.0);
    let offset = clock.at(now);
    clock.offset = offset;
    let (ox, oy) = (offset.0.round() as i32, offset.1.round() as i32);
    let now = store.clock.now;
    let time = now.format(if s.hour12 { "%I%M" } else { "%H%M" }).to_string();
    let new_minute = time != clock.time;
    if new_minute {
        clock.time.clone_from(&time);
    }
    let digits = if s.face == Face::Serif {
        let font = (digit_h / SERIF_FIGURE) as f32;
        let tracking = if s.width == Width::Wide { (digit_h * 0.03) as f32 } else { 0.0 };
        let style = |size: f32| TextStyle {
            family: Family::Generic(parley::GenericFamily::Serif),
            size,
            weight: 400.0,
            tracking,
            wdth: TextStyle::NORMAL_WIDTH,
            wght: None,
        };
        Digits::Serif {
            hours: kit.shape(&time[..2], style(font)),
            colon: kit.shape(":", style(font * 0.6)),
            minutes: kit.shape(&time[2..], style(font)),
            gap: digit_h * 0.06,
        }
    } else {
        let device = (digit_h * scale).round().max(1.0) as u32;
        Digits::Grid { w: f64::from(clock_face::measure(&time, device, s.width)) / scale, text: time, device }
    };
    let digits_w = match &digits {
        Digits::Grid { w, .. } => *w,
        Digits::Serif { hours, colon, minutes, gap } => f64::from(hours.width + colon.width + minutes.width) + gap * 2.0,
    };
    let date = s.date.then(|| {
        let size = (digit_h * 0.16).clamp(12.0, 72.0) as f32;
        let style = TextStyle { family: kit.look.sans, size, weight: 500.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        kit.shape(&now.format("%a %-d %b").to_string(), style)
    });
    let date_h = date.as_ref().map_or(0.0, |d| f64::from(d.line_height()) + digit_h * 0.08);
    let box_w = digits_w.max(date.as_ref().map_or(0.0, |d| f64::from(d.width)));
    let box_h = date_h + digit_h;
    let (cx, cy) = (s.x * sw, s.y * sh);
    let rect = IRect::new((cx - box_w / 2.0).round() as i32, (cy - box_h / 2.0).round() as i32, box_w.ceil() as i32, box_h.ceil() as i32);
    clock.face = s.face;
    clock.rect = rect;

    // The picture's own pixels over a logical rect, the way the lock crops
    // its foreground.
    let k = picture.map_or(scale, |p| f64::from(p.width) / sw);
    let device = |r: IRect, p: &Picture| {
        let at = |v: i32| (f64::from(v) * k).round() as i32;
        IRect::new(at(r.x), at(r.y), at(r.right()) - at(r.x), at(r.bottom()) - at(r.y)).intersect(&IRect::new(0, 0, p.width as i32, p.height as i32))
    };
    let ptr = |p: &Arc<Picture>| Arc::as_ptr(p) as usize;
    clock.ink = match s.color.as_str() {
        "primary" => store.theme.theme.colors.get("primary"),
        "auto" => {
            let from = picture.map(|p| (device(rect, p), ptr(p)));
            if from.is_none() || from != clock.ink_from {
                let luma = match picture {
                    Some(p) => mean_luma(p, device(rect, p)),
                    None => luma(store.theme.theme.colors.get("background")),
                };
                clock.ink_from = from;
                clock.ink = if luma > fs_theme::barpaint::LUMA_THRESHOLD { Rgba::BLACK } else { Rgba::WHITE };
            }
            clock.ink
        }
        hex => Rgba::parse(hex).unwrap_or(Rgba::WHITE),
    };
    let ink = clock.ink;

    let digits_top = f64::from(rect.y) + date_h;
    let digits_x = cx - digits_w / 2.0;
    let mut p = Painter::new(&mut clock.scene, &mut clock.nodes, None);
    if let Some(d) = &date {
        p.text(d, ((cx - f64::from(d.width) / 2.0).round() as i32 + ox, rect.y + oy), ink.with_alpha(ink.a * DATE_ALPHA), &[]);
    }
    match digits {
        Digits::Grid { text, device, .. } => {
            let look = Look { face: s.face, text, device, width: s.width, ink, scale };
            if clock.digits.as_ref().is_none_or(|(l, _)| *l != look)
                && let Some(mask) = clock_face::render(look.face, &look.text, look.device, look.width)
            {
                clock.digits = Some((look, tint(&mask, ink).at(scale)));
            }
            if let Some((_, bitmap)) = &clock.digits {
                p.image(bitmap, (digits_x.round() as i32 + ox, digits_top.round() as i32 + oy), 1.0);
            }
        }
        Digits::Serif { hours, colon, minutes, gap } => {
            let baseline = digits_top + digit_h + f64::from(oy);
            let mut x = digits_x + f64::from(ox);
            let put = |p: &mut Painter, t: &ShapedText, base: f64, x: &mut f64, after: f64| {
                p.text(t, (x.round() as i32, (base - f64::from(t.ascent)).round() as i32), ink, &[]);
                *x += f64::from(t.width) + after;
            };
            put(&mut p, &hours, baseline, &mut x, gap);
            put(&mut p, &colon, baseline - digit_h * COLON_RAISE, &mut x, gap);
            put(&mut p, &minutes, baseline, &mut x, 0.0);
        }
    }
    // The subject over the box and the parallax's reach round it, so the
    // clock moving under it never needs a new crop.
    if let (Some(pic), Some(mask)) = (picture, clock.subject.as_ref().filter(|(k, _)| Some(k.cut) == s.cut).and_then(|(_, m)| m.clone())) {
        let reach = s.parallax.ceil() as i32 + 1;
        let over = IRect::new(rect.x - reach, rect.y - reach, rect.w + reach * 2, rect.h + reach * 2).intersect(&IRect::new(0, 0, size.w, size.h));
        let px = device(over, pic);
        if !px.is_empty() {
            if clock.crop.as_ref().is_none_or(|(r, from, _)| *r != px || *from != ptr(pic)) {
                clock.crop = Some((px, ptr(pic), cut_out(pic, &mask, px).at(k)));
            }
            if let Some((_, _, b)) = &clock.crop {
                p.image(b, (over.x, over.y), 1.0);
            }
        }
    }
    p.finish();
    if new_minute {
        clock.minute_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    }
}

/// A serif face's figures stand this share of its size.
const SERIF_FIGURE: f64 = 0.7;
/// How far the serif colon sits over the figures' baseline, of their height.
const COLON_RAISE: f64 = 0.16;
const DATE_ALPHA: f32 = 0.8;

fn luma(c: Rgba) -> f64 {
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

/// A coverage mask filled with `ink`, premultiplied.
fn tint(mask: &clock_face::Mask, ink: Rgba) -> Bitmap {
    let [r, g, b, a] = [ink.r, ink.g, ink.b, ink.a].map(|v| v.clamp(0.0, 1.0));
    let mut rgba = Vec::with_capacity(mask.data.len() * 4);
    for &c in &mask.data {
        let k = f32::from(c) / 255.0 * a;
        rgba.extend([r * k, g * k, b * k, k].map(|v| (v * 255.0).round() as u8));
    }
    Bitmap::from_premultiplied(mask.width as u16, mask.height as u16, rgba)
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
