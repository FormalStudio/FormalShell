//! One output's lock screen: the
//! wallpaper under the modal scrim, or the flat background without one,
//! and the login card the greeter draws (`surfaces::login`) in its locked
//! state, now playing under the clock in place of the greeter's controls.
//! `lock status` still reports the ink the wallpaper under the card asks
//! for (Lock/model.js's `ink`).

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Local};
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use image::imageops::{self, FilterType};
use serde_json::{Value, json};

use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::services::wallpaper::{self, DepthCut, Picture};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::surfaces::login;
use crate::ui::el::{Opt, Variant};
use crate::ui::{El, Ink, Type, Ui, w};

/// The modal scrim's opacity over the wallpaper.
pub const SCRIM: f64 = 0.5;

/// What every output shows the same.
pub struct Shared<'a> {
    pub now: DateTime<Local>,
    /// The user's full name over the field.
    pub name: &'a str,
    pub prompt: Prompt<'a>,
    pub error: &'a str,
    /// `state.json`'s wallpaper is set.
    pub has_wallpaper: bool,
    /// The decoded wallpaper, scrim baked in, at its own size.
    pub backdrop: Option<&'a Bitmap>,
    /// The backdrop's pixels where the wallpaper's subject stands, the rest
    /// clear: drawn over the clock.
    pub foreground: Option<&'a Bitmap>,
    /// The desktop's sculpture held still, darkened as the scrim darkens
    /// the wallpaper, stretched over the whole output.
    pub shards: Option<&'a Bitmap>,
    /// The wallpaper as the desktop shows it, for the ink sampler.
    pub picture: Option<&'a Arc<Picture>>,
    pub avatar: Option<&'a Bitmap>,
    pub media: Option<NowPlaying<'a>>,
    /// The column's entrance: its opacity and how far it still has to rise.
    pub enter: (f32, f64),
    /// Idle-blanked: nothing but black once the wake fade has run out.
    pub blanked: bool,
    /// The blank and wake crossfade: 0 is
    /// black, 1 the whole screen.
    pub wake: f32,
    pub buttons: Buttons,
}

/// The buttons under the field, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    PowerOff,
    Reboot,
    /// Ends this session for the greeter: the boot's lock only.
    OtherUser,
}

impl Button {
    pub fn id(self) -> &'static str {
        match self {
            Self::PowerOff => "poweroff",
            Self::Reboot => "reboot",
            Self::OtherUser => "otheruser",
        }
    }

    /// The launcher's row names, armed as `Confirm <label>?` the same way.
    pub fn label(self) -> &'static str {
        match self {
            Self::PowerOff => "Shutdown",
            Self::Reboot => "Reboot",
            Self::OtherUser => "Other user",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::PowerOff => "power",
            Self::Reboot => "rotate-cw",
            Self::OtherUser => "users",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub shown: Vec<Button>,
    /// The keyboard's cursor among them, -1 on the field or the transport.
    pub cursor: i32,
    /// Asked once: the next Return on it goes ahead.
    pub armed: Option<Button>,
}

impl Buttons {
    /// Icons alone, the armed one carrying its question in the destructive
    /// variant, centred under the field.
    fn el(&self) -> Option<El> {
        if self.shown.is_empty() {
            return None;
        }
        let row = self
            .shown
            .iter()
            .map(|b| {
                let el = if self.armed == Some(*b) {
                    w::icon_text_button(b.icon(), format!("Confirm {}?", b.label())).variant(Variant::Destructive)
                } else {
                    w::icon_button(b.icon())
                };
                el.stop(stop(*b)).on(stop(*b))
            })
            .collect();
        Some(w::row(0.0, row).centred())
    }
}

/// A button's cursor stop and action.
pub fn stop(b: Button) -> String {
    format!("lock:{}", b.id())
}

/// The password field and what sits over it.
pub struct Prompt<'a> {
    /// What the field shows: a password's dots, or a username as typed.
    pub text: String,
    pub placeholder: &'a str,
    /// The section label over the field; empty draws none.
    pub label: &'a str,
    /// Non-empty replaces the field with this message.
    pub unavailable: &'a str,
    pub enabled: bool,
    /// A check is out: the field breathes on the pulse clock until it answers.
    pub busy: bool,
    /// The caret and the selection's other end, as char counts.
    pub caret: (usize, usize),
}

impl Prompt<'_> {
    /// The lock's: a masked password and nothing over it, or, while PAM
    /// has the password, an empty field saying so.
    pub fn password(dots: usize, caret: (usize, usize), checking: bool) -> Self {
        let placeholder = if checking { "Unlocking\u{2026}" } else { "Password" };
        Self { text: "\u{2022}".repeat(dots), placeholder, label: "", unavailable: "", enabled: !checking, busy: checking, caret }
    }
}

/// The now-playing block, shown while a player that is not a stream
/// has a track.
pub struct NowPlaying<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub playing: bool,
    pub can_previous: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    /// Position over length, when the player has a timeline.
    pub progress: Option<f64>,
    pub art: Option<&'a Bitmap>,
    /// The transport cursor, -1 while the field has every key.
    pub cursor: i32,
}

/// The transport's three buttons, in order.
pub const TRANSPORT: usize = 3;

/// Lock/model.js's `transportKey`: where a key moves the transport cursor,
/// whether the field must not see it, and whether it pressed the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub index: i32,
    pub taken: bool,
    pub press: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TKey {
    Tab,
    Backtab,
    Left,
    Right,
    Enter,
    Escape,
    Modifier,
    Other,
}

pub fn transport_key(index: i32, count: i32, key: TKey) -> Step {
    let mut out = Step { index, taken: false, press: false };
    if count <= 0 {
        out.index = -1;
        return out;
    }
    if index >= count {
        out.index = count - 1;
    }
    match key {
        TKey::Modifier => {}
        TKey::Tab => (out.index, out.taken) = (if out.index + 1 >= count { -1 } else { out.index + 1 }, true),
        TKey::Backtab => (out.index, out.taken) = (if out.index < 0 { count - 1 } else { out.index - 1 }, true),
        _ if out.index < 0 => {}
        TKey::Left => (out.index, out.taken) = ((out.index - 1).max(0), true),
        TKey::Right => (out.index, out.taken) = ((out.index + 1).min(count - 1), true),
        TKey::Enter => (out.press, out.taken) = (true, true),
        TKey::Escape => (out.index, out.taken) = (-1, true),
        TKey::Other => out.index = -1,
    }
    out
}

pub struct View {
    pub scene: Scene,
    pub ui: Ui,
    nodes: Vec<NodeId>,
    /// `lock status`'s per-output report.
    pub report: Value,
    over_clock: Option<OverClock>,
}

/// The foreground's crop over the clock and the card's own pixels over the
/// same box, both cut by the subject's mask, with what they were cut from.
#[derive(Clone)]
struct OverClock {
    pixels: IRect,
    card_rect: IRect,
    card: fs_theme::style::BoxStyle<Rgba>,
    from: Arc<vello_cpu::Pixmap>,
    front: Bitmap,
    over: Bitmap,
}

impl View {
    pub fn new(width: i32, height: i32) -> Self {
        Self { scene: Scene::new(width, height), ui: Ui::new(None), nodes: Vec::new(), report: Value::Null, over_clock: None }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        if (self.scene.size.w, self.scene.size.h) != (width, height) {
            self.scene.resize(width, height);
            self.scene.touch(self.scene.size);
        }
    }

    pub fn draw(&mut self, s: &Shared, theme: &Theme, kit: &mut Kit, now: Instant) -> bool {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let full = IRect::new(0, 0, w, h);
        if s.blanked && s.wake <= 0.0 {
            let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
            p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
            p.finish();
            self.ui.hide(&mut self.scene);
            return false;
        }
        let (enter, rise) = s.enter;
        let fade = enter * s.wake;
        let pr = &s.prompt;
        let media = s.media.as_ref().map(|m| now_playing(m, theme));
        let card = login::Card {
            now: s.now,
            name: s.name,
            avatar: s.avatar,
            text: pr.text.clone(),
            placeholder: pr.placeholder,
            label: pr.label,
            error: s.error,
            unavailable: pr.unavailable,
            enabled: pr.enabled,
            busy: pr.busy,
            caret: pr.caret,
            below: media,
            under_field: s.buttons.el(),
        };
        let clock = card.clock(theme, kit);
        let body = card.body(&clock, theme);
        let (card_rect, inner) = login::place(&body, (w, h), rise.round(), theme, kit);

        let rest = IRect::new(card_rect.x, card_rect.y - rise.round() as i32, card_rect.w, card_rect.h);
        let (ink_name, luma, sampled) = ink(s, theme, rest, (w, h));

        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        // Black under everything, the screen faded in over it: a light
        // theme's `background` under a part-faded backdrop is a white flash
        // in a dark room on every blank and wake.
        let black = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
        p.rect(full, black, 0.0);
        paint_backdrop(&mut p, s, theme, (w, h));
        login::paint(&mut p, card_rect, theme, fade);
        let clock_rect = card.paint_clock(&mut p, &clock, inner, theme, kit, fade);
        let front = s.foreground.and_then(|fg| over_clock(&mut self.over_clock, fg, clock_rect, card_rect, theme, (w, h)));
        if let Some((rect, front, over)) = &front {
            p.shape(*rect, Paint::Picture { image: front.clone(), alpha: s.wake, radius: 0.0 });
            p.shape(*rect, Paint::Picture { image: over.clone(), alpha: fade, radius: 0.0 });
        }
        let last = p.last();
        p.finish();

        self.ui.anchor = last;
        let cursor = s.media.as_ref().map_or(-1, |m| m.cursor);
        let button = usize::try_from(s.buttons.cursor).ok().and_then(|i| s.buttons.shown.get(i).copied());
        self.ui.cursor = match button {
            Some(b) => Some(stop(b)),
            None => (cursor >= 0).then(|| "transport".to_owned()),
        };
        self.ui.ring = self.ui.cursor.is_some();
        let drawn = self.ui.draw(&body, inner, None, fade, theme, kit, &mut self.scene, now);
        let card_json = json!({"x": rest.x, "y": rest.y, "width": rest.w, "height": rest.h});
        self.report = json!({
            "clockInk": ink_name,
            "clockLuma": js(luma),
            "clockSampled": sampled,
            "clockRect": card_json,
            "depth": front.is_some(),
            "depthRect": front.as_ref().map(|(r, _, _)| json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h})),
            "mediaCard": s.media.is_some().then(|| card_json.clone()),
            "nowPlaying": s.media.is_some(),
            "transportCursor": cursor,
        });
        drawn.animating
    }

    /// The boot lock's first frame on this output: the splash's picture on
    /// black and nothing else, so no text is shaped before it is shown.
    pub fn draw_splash(&mut self, splash: Option<&Bitmap>) {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        p.rect(IRect::new(0, 0, w, h), Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
        if let Some(b) = splash {
            p.image(b, splash_at(b, (w, h)), 1.0);
        }
        p.finish();
        self.ui.hide(&mut self.scene);
    }
}

/// The lock under its card, as it stands with nothing over it: what the
/// boot's splash dissolves into.
pub fn backdrop_scene(s: &Shared, theme: &Theme, (w, h): (i32, i32)) -> Scene {
    let mut scene = Scene::new(w, h);
    let mut nodes = Vec::new();
    let mut p = Painter::new(&mut scene, &mut nodes, None);
    p.rect(IRect::new(0, 0, w, h), Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
    paint_backdrop(&mut p, s, theme, (w, h));
    p.finish();
    scene
}

/// The wallpaper, the sculpture and the grid over the black under them.
fn paint_backdrop(p: &mut Painter, s: &Shared, theme: &Theme, (w, h): (i32, i32)) {
    let full = IRect::new(0, 0, w, h);
    match s.backdrop {
        Some(b) if (b.pixmap.width() as i32, b.pixmap.height() as i32) == (w, h) => p.image(b, (0, 0), s.wake),
        // The desktop's picture made at the output's own scale.
        Some(b) if fills(b, (w, h)) => {
            p.shape(full, Paint::Picture { image: b.clone(), alpha: s.wake, radius: 0.0 });
        }
        // The wallpaper still decoding: dark, never the theme's background.
        None if s.has_wallpaper => {}
        _ => {
            let bg = theme.colors.get("background");
            p.rect(full, bg.with_alpha(bg.a * s.wake), 0.0);
            if s.has_wallpaper {
                p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: SCRIM as f32 }, 0.0);
            }
        }
    }
    if let Some(b) = s.shards {
        p.shape(full, Paint::Picture { image: b.clone(), alpha: s.wake, radius: 0.0 });
    }
    login::paint_grid(p, theme, (w, h), s.wake);
}

/// Centred on its own pixels, as sway's `bg <png> center` puts the splash's
/// frame: the offset whole in device pixels, rounded down.
fn splash_at(b: &Bitmap, (w, h): (i32, i32)) -> (i32, i32) {
    let k = f64::from(b.density);
    let at = |screen: i32, px: u16| ((f64::from(screen) * k - f64::from(px)) / 2.0).floor() / k;
    (at(w, b.pixmap.width()).round() as i32, at(h, b.pixmap.height()).round() as i32)
}

/// The card's cover, words and controls.
fn now_playing(m: &NowPlaying, theme: &Theme) -> El {
    let s = &theme.space;
    let title = if m.title.is_empty() { "Unknown title" } else { m.title };
    let mut words = vec![w::label(title).elide()];
    if !m.artist.is_empty() {
        words.push(w::text(m.artist).size(Type::BodySmall).ink(Ink::Muted).elide());
    }
    let mut info = Vec::new();
    if let Some(art) = m.art {
        info.push(w::picture(Some(art.clone()), s.control_height * 2.0));
    }
    info.push(w::column(s.xxs, words).fill());
    let mut rows = vec![w::row(s.md, info).fill()];
    if let Some(p) = m.progress {
        rows.push(w::track(p.clamp(0.0, 1.0)).fill());
    }
    let icon = |id: &str| match id {
        "previous" => "skip-back",
        "next" => "skip-forward",
        _ if m.playing => "pause",
        _ => "play",
    };
    let enabled = [m.can_previous, m.can_toggle, m.can_next];
    let options: Vec<Opt> = ["previous", "playpause", "next"]
        .iter()
        .zip(enabled)
        .map(|(id, on)| {
            let mut o = Opt::new("").icon(icon(id));
            o.enabled = on;
            o
        })
        .collect();
    let group = w::group(options, usize::MAX, false).stop("transport").ring(m.cursor.max(0) as usize);
    rows.push(group.centred());
    w::column(s.sm, rows)
}

/// How `JSON.stringify` prints a number: whole values without a fraction.
fn js(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 1e15 { json!(v as i64) } else { json!(v) }
}

/// Lock/model.js's `lumaOf`, on 0..1 channels.
fn luma_of(c: Rgba) -> f64 {
    255.0 * (0.299 * c.r as f64 + 0.587 * c.g as f64 + 0.114 * c.b as f64)
}

fn linear(v: f64) -> f64 {
    let c = v.clamp(0.0, 255.0) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn contrast(a: f64, b: f64) -> f64 {
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Lock/model.js's `ink` on an already composited backdrop luma.
fn ink_of(backdrop: f64) -> &'static str {
    let l = linear(backdrop);
    if contrast(1.0, l) >= contrast(0.0, l) { "light" } else { "dark" }
}

/// The ink over `rect`, the backdrop luma it was chosen against, and
/// whether that luma was read off the wallpaper (BarPaint's `sampled`).
fn ink(s: &Shared, theme: &Theme, rect: IRect, size: (i32, i32)) -> (&'static str, f64, bool) {
    let fallback = if s.has_wallpaper { 0.0 } else { luma_of(theme.colors.get("background")) };
    let mean = s.picture.and_then(|p| {
        let (pw, ph) = (p.width as i32, p.height as i32);
        if (pw, ph) == size {
            return mean_luma(p, rect);
        }
        if !fills_size((pw, ph), size) {
            return None;
        }
        let k = f64::from(pw) / f64::from(size.0.max(1));
        let at = |v: i32| (f64::from(v) * k).round() as i32;
        mean_luma(p, IRect::new(at(rect.x), at(rect.y), at(rect.right()) - at(rect.x), at(rect.bottom()) - at(rect.y)))
    });
    let scrim = if s.has_wallpaper { SCRIM } else { 0.0 };
    match mean {
        Some(m) => {
            let l = m * (1.0 - scrim);
            (ink_of(l), l, true)
        }
        None => (ink_of(fallback), fallback, false),
    }
}

/// The mean luma (0..255) of the picture under `rect`, every pixel read.
fn mean_luma(p: &Picture, rect: IRect) -> Option<f64> {
    let r = rect.intersect(&IRect::new(0, 0, p.width as i32, p.height as i32));
    if r.is_empty() {
        return None;
    }
    let mut sum = 0.0;
    for y in r.y..r.bottom() {
        let row = &p.bgra[(y as usize * p.width as usize + r.x as usize) * 4..(y as usize * p.width as usize + r.right() as usize) * 4];
        for px in row.chunks_exact(4) {
            sum += 0.299 * px[2] as f64 + 0.587 * px[1] as f64 + 0.114 * px[0] as f64;
        }
    }
    Some(sum / r.area() as f64)
}

/// The foreground's pixels under `rect` of a screen `size`, the card's
/// pixels over the same box at the subject's coverage, and the box: the
/// subject reads as the card shows it, not as bare wallpaper. Cut again
/// only when the box, the card or the foreground moved, so a settled lock
/// hands the scene the same pictures every frame.
fn over_clock(cache: &mut Option<OverClock>, fg: &Bitmap, rect: IRect, card_rect: IRect, theme: &Theme, size: (i32, i32)) -> Option<(IRect, Bitmap, Bitmap)> {
    let rect = rect.intersect(&IRect::new(0, 0, size.0, size.1));
    let (fw, fh) = (i32::from(fg.pixmap.width()), i32::from(fg.pixmap.height()));
    if rect.is_empty() || card_rect.is_empty() || ((fw, fh) != size && !fills_size((fw, fh), size)) {
        return None;
    }
    let k = f64::from(fw) / f64::from(size.0.max(1));
    let at = |v: i32| (f64::from(v) * k).round() as i32;
    let px = IRect::new(at(rect.x), at(rect.y), at(rect.right()) - at(rect.x), at(rect.bottom()) - at(rect.y))
        .intersect(&IRect::new(0, 0, fw, fh));
    if px.is_empty() {
        return None;
    }
    let card = theme.box_style("card", Some("dialog"));
    let hit = matches!(&*cache, Some(c) if c.pixels == px && c.card_rect == card_rect && c.card == card && Arc::ptr_eq(&c.from, &fg.pixmap));
    if !hit {
        let data = fg.pixmap.data_as_u8_slice();
        let mut front = Vec::with_capacity((px.w * px.h * 4) as usize);
        for y in px.y..px.bottom() {
            let from = ((y * fw + px.x) * 4) as usize;
            front.extend_from_slice(&data[from..from + (px.w * 4) as usize]);
        }
        // The card alone, drawn the way the lock draws it, at the
        // foreground's pixel size.
        let mut scene = Scene::new(card_rect.w, card_rect.h);
        let mut nodes = Vec::new();
        let mut p = Painter::new(&mut scene, &mut nodes, None);
        login::paint(&mut p, IRect::new(0, 0, card_rect.w, card_rect.h), theme, 1.0);
        p.finish();
        let mut r = crate::render::Renderer::new(card_rect.w as u16, card_rect.h as u16);
        r.set_scale(k);
        r.render(&scene, IRect::new(0, 0, card_rect.w, card_rect.h));
        let (cw, ch) = r.pixels();
        let canvas = r.canvas().data_as_u8_slice();
        let (ox, oy) = (px.x - at(card_rect.x), px.y - at(card_rect.y));
        let mut over = vec![0u8; front.len()];
        for y in 0..px.h {
            for x in 0..px.w {
                let (cx, cy) = (ox + x, oy + y);
                if cx < 0 || cy < 0 || cx >= cw || cy >= ch {
                    continue;
                }
                let i = ((y * px.w + x) * 4) as usize;
                let mask = u32::from(front[i + 3]);
                let c = ((cy * cw + cx) * 4) as usize;
                for n in 0..4 {
                    over[i + n] = ((u32::from(canvas[c + n]) * mask + 127) / 255) as u8;
                }
            }
        }
        *cache = Some(OverClock {
            pixels: px,
            card_rect,
            card,
            from: fg.pixmap.clone(),
            front: Bitmap::from_premultiplied(px.w as u16, px.h as u16, front),
            over: Bitmap::from_premultiplied(px.w as u16, px.h as u16, over),
        });
    }
    cache.as_ref().map(|c| (rect, c.front.clone(), c.over.clone()))
}

/// A picture larger than `size` at its aspect, give or take the rounding
/// of a scaled size: the output's own pixels under a fractional scale.
fn fills(b: &Bitmap, size: (i32, i32)) -> bool {
    fills_size((i32::from(b.pixmap.width()), i32::from(b.pixmap.height())), size)
}

fn fills_size((bw, bh): (i32, i32), (w, h): (i32, i32)) -> bool {
    let (bw, bh, w, h) = (i64::from(bw), i64::from(bh), i64::from(w), i64::from(h));
    bw > w && (bw * h - bh * w).abs() <= bw.max(bh)
}

/// The desktop's picture with the scrim baked in, for the pool, and with
/// `cut` the same pixels over the picture's depth map, for in front of the
/// clock. Its grid comes off first: the lock draws its own over the scrim,
/// or none.
pub fn backdrop(p: &Picture, cut: Option<DepthCut>) -> (Bitmap, Option<Bitmap>) {
    let mut rgba: Vec<u8> = p.bgra.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0], 255]).collect();
    if let Some(g) = p.grid {
        g.lift(&mut rgba, p.width as usize, p.height as usize);
    }
    let keep = 1.0 - SCRIM;
    for px in rgba.chunks_exact_mut(4) {
        for c in &mut px[..3] {
            *c = (f64::from(*c) * keep) as u8;
        }
    }
    let front = cut.and_then(|cut| wallpaper::depth_mask(&p.path, p.width, p.height, cut)).map(|mask| {
        let mut front = rgba.clone();
        for (px, a) in front.chunks_exact_mut(4).zip(mask) {
            px[3] = a;
        }
        Bitmap::from_rgba(p.width as u16, p.height as u16, front)
    });
    (Bitmap::from_rgba(p.width as u16, p.height as u16, rgba), front)
}

/// By its bytes, not its name: `~/.face` has no extension.
fn decode(path: &str) -> Option<image::RgbaImage> {
    Some(image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8())
}

/// The cover at `size`: the track's art (a file or a data URL) cover-cropped
/// square, for the pool.
pub fn cover(url: &str, size: u32) -> Option<Bitmap> {
    let img = match url.strip_prefix("data:") {
        Some(data) => {
            use base64::Engine;
            let (_, payload) = data.split_once(";base64,")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(payload.trim()).ok()?;
            image::load_from_memory(&bytes).ok()?.to_rgba8()
        }
        None => decode(url.strip_prefix("file://")?)?,
    };
    let side = img.width().min(img.height());
    let crop = imageops::crop_imm(&img, (img.width() - side) / 2, (img.height() - side) / 2, side, side).to_image();
    let out = imageops::resize(&crop, size, size, FilterType::Triangle);
    Some(Bitmap::from_rgba(size as u16, size as u16, out.into_raw()))
}

/// The picture cover-cropped into a `size` disc, for the pool.
/// None when there is no readable picture, which hides the avatar.
pub fn avatar(path: &str, size: u32) -> Option<Bitmap> {
    let img = decode(path)?;
    let side = img.width().min(img.height());
    let crop = imageops::crop_imm(&img, (img.width() - side) / 2, (img.height() - side) / 2, side, side).to_image();
    let mut out = imageops::resize(&crop, size, size, FilterType::Triangle);
    let r = size as f64 / 2.0;
    for (x, y, px) in out.enumerate_pixels_mut() {
        let d = ((x as f64 + 0.5 - r).powi(2) + (y as f64 + 0.5 - r).powi(2)).sqrt();
        let cover = (r - d + 0.5).clamp(0.0, 1.0);
        px.0[3] = (px.0[3] as f64 * cover).round() as u8;
    }
    Some(Bitmap::from_rgba(size as u16, size as u16, out.into_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ink_follows_the_backdrop() {
        assert_eq!(ink_of(0.0), "light");
        assert_eq!(ink_of(255.0), "dark");
    }

    #[test]
    fn the_splash_frame_is_the_picture_centred_on_black() {
        use crate::render::Renderer;
        let (w, h) = (64, 48);
        let pink = Bitmap::from_rgba(16, 8, [224u8, 69, 123, 255].repeat(16 * 8));
        let mut view = View::new(w, h);
        view.draw_splash(Some(&pink));
        let mut r = Renderer::new(w as u16, h as u16);
        r.render(&view.scene, IRect::new(0, 0, w, h));
        let px = |x: i32, y: i32| r.canvas().data()[(y * w + x) as usize];
        assert_eq!((px(0, 0).r, px(0, 0).a), (0, 255));
        assert_eq!(px(23, 19).r, 0);
        assert_eq!(px(24, 20).r, 224);
        assert_eq!(px(39, 27).r, 224);
        assert_eq!(px(40, 28).r, 0);
    }

    #[test]
    fn transport_keys_follow_the_model() {
        let t = |i, k| transport_key(i, 3, k);
        assert_eq!(t(-1, TKey::Tab), Step { index: 0, taken: true, press: false });
        assert_eq!(t(0, TKey::Right), Step { index: 1, taken: true, press: false });
        assert_eq!(t(1, TKey::Enter), Step { index: 1, taken: true, press: true });
        assert_eq!(t(2, TKey::Tab), Step { index: -1, taken: true, press: false });
        assert_eq!(t(-1, TKey::Backtab).index, 2);
        assert_eq!(t(1, TKey::Other), Step { index: -1, taken: false, press: false });
        assert_eq!(t(-1, TKey::Enter), Step { index: -1, taken: false, press: false });
        assert_eq!(transport_key(1, 0, TKey::Tab).index, -1);
    }

    #[test]
    fn blank_and_wake_crossfade_the_backdrop() {
        use crate::render::Renderer;
        let theme = Theme::resolve(|_| None, &Default::default());
        let mut kit = Kit::new(&theme);
        // Big enough that the corner is clear of the card.
        let (w, h) = (900, 700);
        let white = Bitmap::from_rgba(w as u16, h as u16, vec![255; (w * h * 4) as usize]);
        let mut corner = |wake: f32| {
            let shared = Shared {
                now: Local::now(),
                name: "",
                prompt: Prompt::password(0, (0, 0), false),
                error: "",
                has_wallpaper: true,
                backdrop: Some(&white),
                foreground: None,
                shards: None,
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: true,
                wake,
                buttons: Buttons::default(),
            };
            let mut view = View::new(w, h);
            view.draw(&shared, &theme, &mut kit, Instant::now());
            let mut r = Renderer::new(w as u16, h as u16);
            r.render(&view.scene, IRect::new(0, 0, w, h));
            r.canvas().data()[0].r as i32
        };
        let (off, half, on) = (corner(0.0), corner(0.5), corner(1.0));
        assert_eq!(off, 0);
        assert!(half > off + 40 && half < on - 40, "{off} {half} {on}");
    }

    #[test]
    fn the_foreground_covers_the_clock_as_the_card_shows_it_and_is_cut_once() {
        use crate::render::Renderer;
        let theme = Theme::resolve(|_| None, &fs_theme::palette::fallback("dark"));
        let mut kit = Kit::new(&theme);
        let (w, h) = (900, 700);
        let red = Bitmap::from_rgba(w as u16, h as u16, [255, 0, 0, 255].repeat((w * h) as usize));
        let shared = |fg| Shared {
            now: Local::now(),
            name: "",
            prompt: Prompt::password(0, (0, 0), false),
            error: "",
            has_wallpaper: true,
            backdrop: Some(&red),
            foreground: fg,
            shards: None,
            picture: None,
            avatar: None,
            media: None,
            enter: (1.0, 0.0),
            blanked: false,
            wake: 1.0,
            buttons: Buttons::default(),
        };
        let mut view = View::new(w, h);
        view.draw(&shared(None), &theme, &mut kit, Instant::now());
        assert_eq!(view.report["depth"], false);
        view.draw(&shared(Some(&red)), &theme, &mut kit, Instant::now());
        assert_eq!(view.report["depth"], true);
        let OverClock { pixels: cut, front: crop, .. } = view.over_clock.clone().unwrap();
        assert!(cut.w > 0 && cut.h > 0 && cut.w < w);
        let mut r = Renderer::new(w as u16, h as u16);
        r.render(&view.scene, IRect::new(0, 0, w, h));
        let at = |x: i32, y: i32| {
            let p = r.canvas().data()[(y * w + x) as usize];
            [i32::from(p.r), i32::from(p.g), i32::from(p.b)]
        };
        // The subject over the whole screen: inside the clock's box reads
        // exactly as the card over it just above the box, digits hidden.
        let (inside, above) = (at(cut.x + cut.w / 2, cut.y + cut.h / 2), at(cut.x + cut.w / 2, cut.y - 3));
        assert!(inside.iter().zip(above).all(|(a, b)| (a - b).abs() <= 2), "a step at the clock's edge: {inside:?} {above:?}");
        assert_ne!(inside, [255, 0, 0], "the card's fill is missing over the subject");
        let ink = (cut.y..cut.bottom()).flat_map(|y| (cut.x..cut.right()).map(move |x| (x, y))).filter(|&(x, y)| at(x, y).iter().zip(above).any(|(a, b)| (a - b).abs() > 2)).count();
        assert_eq!(ink, 0, "clock ink showing through the subject");
        view.draw(&shared(Some(&red)), &theme, &mut kit, Instant::now());
        assert!(Arc::ptr_eq(&view.over_clock.as_ref().unwrap().front.pixmap, &crop.pixmap), "a settled lock cut the crop again");
    }

    #[test]
    fn wake_never_passes_a_light_background() {
        use crate::render::Renderer;
        let theme = Theme::resolve(|_| None, &fs_theme::palette::fallback("light"));
        assert!(luma_of(theme.colors.get("background")) > 200.0);
        let mut kit = Kit::new(&theme);
        // Big enough that the corner is clear of the column.
        let (w, h) = (900, 700);
        let dim =Bitmap::from_rgba(w as u16, h as u16, [24, 24, 24, 255].repeat((w * h) as usize));
        let mut corner = |backdrop: Option<&Bitmap>, has_wallpaper: bool, wake: f32| {
            let shared = Shared {
                now: Local::now(),
                name: "",
                prompt: Prompt::password(0, (0, 0), false),
                error: "",
                has_wallpaper,
                backdrop,
                foreground: None,
                shards: None,
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: true,
                wake,
                buttons: Buttons::default(),
            };
            let mut view = View::new(w, h);
            view.draw(&shared, &theme, &mut kit, Instant::now());
            let mut r = Renderer::new(w as u16, h as u16);
            r.render(&view.scene, IRect::new(0, 0, w, h));
            r.canvas().data()[0].r as i32
        };
        for (backdrop, has) in [(Some(&dim), true), (None, true), (None, false)] {
            let rest = corner(backdrop, has, 1.0);
            for wake in [0.05, 0.25, 0.5, 0.75] {
                let at = corner(backdrop, has, wake);
                assert!(at <= rest, "wake {wake} drew {at}, brighter than the settled {rest}");
            }
        }
        assert_eq!(corner(None, true, 1.0), 0, "a wallpaper still decoding draws dark");
    }

    /// The check's pulse is what keeps the lock asking for frames while PAM
    /// has the password; a settled field asks for none.
    #[test]
    fn a_pending_check_keeps_the_field_breathing() {
        let theme = Theme::resolve(|_| None, &Default::default());
        assert!(theme.motion_enabled);
        let mut kit = Kit::new(&theme);
        let mut animating = |checking: bool| {
            let shared = Shared {
                now: Local::now(),
                name: "",
                prompt: Prompt::password(0, (0, 0), checking),
                error: "",
                has_wallpaper: false,
                backdrop: None,
                foreground: None,
                shards: None,
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: false,
                wake: 1.0,
                buttons: Buttons::default(),
            };
            let mut view = View::new(900, 700);
            let now = Instant::now();
            view.draw(&shared, &theme, &mut kit, now);
            view.draw(&shared, &theme, &mut kit, now + std::time::Duration::from_millis(300))
        };
        assert!(animating(true));
        assert!(!animating(false));
        assert_eq!(Prompt::password(0, (0, 0), true).placeholder, "Unlocking\u{2026}");
    }

    #[test]
    fn whole_numbers_print_like_js() {
        assert_eq!(js(0.0).to_string(), "0");
        assert_eq!(js(12.5).to_string(), "12.5");
    }
}
