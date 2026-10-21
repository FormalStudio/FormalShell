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

use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::wallpaper::Picture;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::surfaces::login;
use crate::ui::el::Opt;
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
    ui: Ui,
    nodes: Vec<NodeId>,
    /// `lock status`'s per-output report.
    pub report: Value,
}

impl View {
    pub fn new(width: i32, height: i32) -> Self {
        Self { scene: Scene::new(width, height), ui: Ui::new(None), nodes: Vec::new(), report: Value::Null }
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
        match s.backdrop {
            Some(b) if (b.pixmap.width() as i32, b.pixmap.height() as i32) == (w, h) => p.image(b, (0, 0), s.wake),
            // The wallpaper still decoding: dark, never the theme's background.
            None if s.has_wallpaper => {}
            _ => {
                let bg = theme.colors.get("background");
                p.rect(full, bg.with_alpha(bg.a * s.wake), 0.0);
                if s.has_wallpaper {
                    p.rect(full, black.with_alpha(SCRIM as f32), 0.0);
                }
            }
        }
        login::paint(&mut p, card_rect, theme, fade);
        card.paint_clock(&mut p, &clock, inner, theme, kit, fade);
        let last = p.last();
        p.finish();

        self.ui.anchor = last;
        let cursor = s.media.as_ref().map_or(-1, |m| m.cursor);
        self.ui.cursor = (cursor >= 0).then(|| "transport".to_owned());
        self.ui.ring = cursor >= 0;
        let drawn = self.ui.draw(&body, inner, None, fade, theme, kit, &mut self.scene, now);
        let card_json = json!({"x": rest.x, "y": rest.y, "width": rest.w, "height": rest.h});
        self.report = json!({
            "clockInk": ink_name,
            "clockLuma": js(luma),
            "clockSampled": sampled,
            "clockRect": card_json,
            "mediaCard": s.media.is_some().then(|| card_json.clone()),
            "nowPlaying": s.media.is_some(),
            "transportCursor": cursor,
        });
        drawn.animating
    }
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
    let mut rows = vec![w::row(s.lg, info).fill()];
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
    w::column(s.md, rows)
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
    let mean = s.picture.filter(|p| (p.width as i32, p.height as i32) == size).and_then(|p| mean_luma(p, rect));
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

/// The desktop's picture with the scrim baked in, for the pool.
pub fn backdrop(p: &Picture) -> Bitmap {
    let keep = 1.0 - SCRIM;
    let mut rgba = Vec::with_capacity(p.bgra.len());
    for px in p.bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[(px[2] as f64 * keep) as u8, (px[1] as f64 * keep) as u8, (px[0] as f64 * keep) as u8, 255]);
    }
    Bitmap::from_rgba(p.width as u16, p.height as u16, rgba)
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
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: true,
                wake,
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
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: true,
                wake,
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
                picture: None,
                avatar: None,
                media: None,
                enter: (1.0, 0.0),
                blanked: false,
                wake: 1.0,
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
