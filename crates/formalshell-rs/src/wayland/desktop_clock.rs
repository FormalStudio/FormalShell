//! The desktop clock (`desktopClock.*`): a Background layer on every output,
//! stacked over the wallpaper and the sculpture, clear but for the clock's
//! box. The time is drawn there and the wallpaper's subject laid back over
//! it, cut out of the picture by its depth map, so the clock sits behind
//! whatever the map puts in front; with no `desktopClock.y` the clock is
//! placed so the subject's top edge crosses its lower part. The face and
//! its glass are `surfaces/desktop_clock.rs`, which the sidebar's time card
//! draws with too.

use std::sync::Arc;
use std::time::Instant;

use calloop::channel::{self, Sender};
use fs_chrome::clock_face::Face;
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::paint::{Kit, Painter};
use crate::scene::{NodeId, Scene};
use fs_types::IRect;
use crate::services::wallpaper::{DepthCut, Picture};
use crate::settings;
use crate::store::Store;
use crate::surface::{Backend, Surface};
use crate::surfaces::desktop_clock::{Digits, FaceSettings, Glass, Subject, Under, auto_ink};

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
        if !settings::CLOCK_ENABLED.get(config) {
            return None;
        }
        let num = |path: &str, fallback: f64, lo: f64, hi: f64| config.f64(path).filter(|n| n.is_finite()).map_or(fallback, |n| n.clamp(lo, hi));
        Some(Self {
            look: FaceSettings::read(config),
            size: num("desktopClock.size", 0.18, 0.02, 0.9),
            x: num("desktopClock.x", 0.5, 0.0, 1.0),
            y: config.f64("desktopClock.y").filter(|n| n.is_finite()).map(|n| n.clamp(0.0, 1.0)),
            hour12: settings::CLOCK_HOUR12.get(config),
        })
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
        for c in self.bars.heads.iter_mut().filter_map(|h| h.clock.as_mut()) {
            landed |= c.slot.land(&msg);
        }
        if landed {
            self.sync_desktop_clocks();
        }
    }

    pub(super) fn desktop_clock_changed(&mut self) {
        self.desktop_clock.generation += 1;
    }

    /// Each output's clock made or dropped as the settings and its output's
    /// state ask, and drawn; the sidebar's clock card handed what it
    /// frosts over.
    pub(super) fn sync_desktop_clocks(&mut self) {
        let settings = Settings::read(&self.store.config);
        let mut asks = Vec::new();
        for i in 0..self.bars.heads.len() {
            let output = self.bars.heads[i].output.clone();
            let name = self.output_name(&output);
            let shards = self.bars.heads[i].backdrop.as_ref().is_some_and(|b| b.shards.is_some());
            let want = settings.is_some() && self.bars.heads[i].backdrop.is_some() && !self.chrome().covered(&name);
            let stale = self.bars.heads[i].clock.as_ref().is_some_and(|c| shards && !c.over_shards);
            if (!want || stale) && self.bars.heads[i].clock.take().is_some() {
                crate::trace::note(format_args!("desktop clock off {name}"));
            }
            let Some(settings) = &settings else { continue };
            if !want {
                continue;
            }
            if self.bars.heads[i].clock.is_none() {
                let layer = self.overlay_on("formalshell:desktop-clock", Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
                let surface = Surface::new("desktop-clock", Backend::Gpu, layer, &self.shm, self.store.display.scales.on(&output));
                self.bars.heads[i].clock = Some(Clock::new(surface, shards));
                crate::trace::note(format_args!("desktop clock on {name}"));
            }
            let picture = self.desktop_clock_picture(i);
            let Some(clock) = self.bars.heads[i].clock.as_mut() else { continue };
            draw(clock, &mut self.kit, &self.store, settings, picture.as_ref());
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
        if live!(self).bar.sidebar().is_none() {
            self.desktop_clock.sidebar = Slot::default();
            return None;
        }
        let face = FaceSettings::read(&self.store.config);
        let head = self.bars.heads.iter().position(|h| Some(&h.output) == self.bar_wl());
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
        if let Some(sb) = live!(self).bar.sidebar_mut()
            && sb.set_clock(under)
        {
            live!(self).dirty = true;
        }
        ask
    }

    fn desktop_clock_picture(&self, i: usize) -> Option<Arc<Picture>> {
        let (w, h) = self.bars.heads[i].backdrop.as_ref().and_then(|b| b.pixels())?;
        let wanted = self.store.desktop();
        self.store.wallpaper.sized(w as u32, h as u32).filter(|p| p.path == wanted).cloned()
    }

    /// Head `i`'s clock holds its frame: its output fullscreen, the session
    /// locked or under the screensaver.
    pub(super) fn desktop_clock_held(&self, i: usize) -> bool {
        let name = self.output_name(&self.bars.heads[i].output);
        self.lock.locked || self.saver.active || self.fullscreen_on(&name)
    }

    pub(super) fn present_desktop_clocks(&mut self) {
        let qh = self.qh.clone();
        for i in 0..self.bars.heads.len() {
            if self.desktop_clock_held(i) {
                continue;
            }
            if let Some(c) = self.bars.heads[i].clock.as_mut() {
                c.surface.present(&mut c.scene, false, &qh);
            }
        }
    }

    /// Each output's clock for `debug dump`.
    pub(crate) fn desktop_clock_state(&self) -> serde_json::Value {
        let settings = Settings::read(&self.store.config);
        let heads: Vec<serde_json::Value> = self
            .bars
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
                    "ink": ink.to_hex(),
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
        let Some(sb) = live!(self).bar.sidebar() else { return serde_json::Value::Null };
        let mut v = sb.clock_state(&self.store);
        v["origin"] = serde_json::json!(self.bar_origin());
        v["commits"] = serde_json::json!(live!(self).surface.as_ref().map_or(0, |s| s.commits()));
        v
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
                    None => store.theme.theme.colors.get(ColorRole::Background).luma601(),
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

/// The share of the digits a placed clock hides behind the subject's edge.
const UNDER: f64 = 0.2;
/// Where a clock with no subject to sit behind is centred, of the height.
const AUTO_Y: f64 = 0.3;
