//! One output's lock screen: the
//! wallpaper under the modal scrim, or the flat background without one,
//! and the login card the greeter draws (`surfaces::login`) in its locked
//! state, now playing under the clock in place of the greeter's controls.
//! `lock status` still reports the ink the wallpaper under the card asks
//! for.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Local};
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use serde_json::{Value, json};

use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::power::Action;
use crate::services::wallpaper::{self, DepthCut, Picture};
use crate::paint::{Kit, Painter};
use crate::surfaces::login::{self, Backdrop, Prompt, Under, View};
use crate::ui::el::{Opt, Variant};
use crate::ui::{El, Ink, Type, w};

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
    /// Where the keyboard's cursor rests past the field.
    pub cursor: Option<Control>,
}

impl<'a> Shared<'a> {
    /// `name` over `prompt` on the flat background, settled and awake,
    /// with nothing else on the card.
    pub fn new(name: &'a str, prompt: Prompt<'a>) -> Self {
        Self {
            now: Local::now(),
            name,
            prompt,
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
            cursor: None,
        }
    }
}

/// The buttons under the field, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Power(Action),
    /// Ends this session for the greeter: the boot's lock only.
    OtherUser,
}

impl Button {
    pub const SHUTDOWN: Self = Self::Power(Action::PowerOff);
    pub const REBOOT: Self = Self::Power(Action::Reboot);

    pub fn id(self) -> &'static str {
        match self {
            Self::Power(a) => a.id(),
            Self::OtherUser => "otheruser",
        }
    }

    /// The launcher's row names, armed as the launcher's are
    /// (`confirm::ask`).
    pub fn label(self) -> &'static str {
        match self {
            Self::Power(Action::Suspend) => "Suspend",
            Self::Power(Action::Reboot) => "Reboot",
            Self::Power(Action::PowerOff) => "Shutdown",
            Self::OtherUser => "Other user",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::Power(a) => login::power_icon(a),
            Self::OtherUser => "users",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Buttons {
    pub shown: Vec<Button>,
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
                    w::icon_text_button(b.icon(), crate::ui::confirm::ask(b.label())).variant(Variant::Destructive)
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

/// The lock's field: a masked password and nothing over it, or, while PAM
/// has the password, closed and saying so.
pub fn prompt(typed: &str, caret: (usize, usize), checking: bool) -> Prompt<'static> {
    let p = Prompt::new(typed, true, caret, "Password");
    if checking { p.checking("Unlocking\u{2026}") } else { p }
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
}

/// A control the keyboard's cursor rests on past the field: the
/// transport's, then the buttons under the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Previous,
    PlayPause,
    Next,
    Button(Button),
}

pub const TRANSPORT: [Control; 3] = [Control::Previous, Control::PlayPause, Control::Next];

/// The controls in the cursor's order: the transport while a track
/// shows, then `buttons`.
pub fn controls(media: bool, buttons: &[Button]) -> Vec<Control> {
    let transport: &[Control] = if media { &TRANSPORT } else { &[] };
    transport.iter().copied().chain(buttons.iter().map(|&b| Control::Button(b))).collect()
}

/// The keyboard's place in a row of controls, or the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor(i32);

impl Default for Cursor {
    fn default() -> Self {
        Self(-1)
    }
}

impl Cursor {
    /// Back on the field.
    pub fn clear(&mut self) {
        self.0 = -1;
    }

    /// The control it rests on.
    pub fn on(self, controls: &[Control]) -> Option<Control> {
        usize::try_from(self.0).ok().and_then(|i| controls.get(i).copied())
    }

    /// A key over `controls`: whether the field must not see it, and the
    /// control it pressed.
    pub fn key(&mut self, controls: &[Control], key: TKey) -> (bool, Option<Control>) {
        let step = transport_key(self.0, controls.len() as i32, key);
        self.0 = step.index;
        (step.taken, if step.press { self.on(controls) } else { None })
    }
}

/// Where a key moves the transport cursor,
/// whether the field must not see it, and whether it pressed the button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    index: i32,
    taken: bool,
    press: bool,
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

fn transport_key(index: i32, count: i32, key: TKey) -> Step {
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

/// One output's lock drawn into `view`: whether it is still moving, and
/// `lock status`'s report for the output, none while blanked to black.
pub fn draw(view: &mut View, s: &Shared, theme: &Theme, kit: &mut Kit, now: Instant) -> (bool, Option<Value>) {
    let (w, h) = (view.scene.size.w, view.scene.size.h);
    if s.blanked && s.wake <= 0.0 {
        view.bare(None);
        return (false, None);
    }
    let ring = TRANSPORT.iter().position(|c| Some(*c) == s.cursor);
    let card = login::Card {
        now: s.now,
        name: s.name,
        avatar: s.avatar,
        prompt: s.prompt.clone(),
        error: s.error,
        below: s.media.as_ref().map(|m| now_playing(m, ring, theme)),
        under_field: s.buttons.el(),
    };
    view.ui.cursor = match s.cursor {
        Some(Control::Button(b)) => Some(stop(b)),
        Some(_) => Some("transport".to_owned()),
        None => None,
    };
    view.ui.ring = view.ui.cursor.is_some();
    let drawn = view.draw(&card, &backdrop_of(s, (w, h)), s.enter, theme, kit, now);
    let (ink_name, luma, sampled) = ink(s, theme, drawn.card, (w, h));
    let card_json = drawn.card.to_json();
    let report = json!({
        "clockInk": ink_name,
        "clockLuma": fs_js::json_number(luma),
        "clockSampled": sampled,
        "clockRect": card_json,
        "depth": drawn.depth.is_some(),
        "depthRect": drawn.depth.map(|r| r.to_json()),
        "mediaCard": s.media.is_some().then(|| card_json.clone()),
        "nowPlaying": s.media.is_some(),
        "transportCursor": ring.map_or(-1, |i| i as i32),
    });
    (drawn.animating, Some(report))
}

/// The boot lock's first frame on this output: the splash's picture on
/// black and nothing else, so no text is shaped before it is shown.
pub fn draw_splash(view: &mut View, splash: Option<&Bitmap>) {
    let size = (view.scene.size.w, view.scene.size.h);
    view.bare(splash.map(|b| (b, splash_at(b, size))));
}

/// What the lock stands on: the scrimmed wallpaper at this output's
/// pixels, dark while it decodes, else the background, scrimmed when a
/// wallpaper is set.
fn backdrop_of<'a>(s: &Shared<'a>, size: (i32, i32)) -> Backdrop<'a> {
    let under = match s.backdrop {
        Some(b) if (i32::from(b.pixmap.width()), i32::from(b.pixmap.height())) == size || login::fills(b, size) => Under::Picture(b),
        None if s.has_wallpaper => Under::Dark,
        _ => Under::Flat { scrim: if s.has_wallpaper { SCRIM as f32 } else { 0.0 } },
    };
    Backdrop { under, shards: s.shards, foreground: s.foreground, wake: s.wake }
}

/// The lock under its card, as it stands with nothing over it: what the
/// boot's splash dissolves into.
pub fn backdrop_scene(s: &Shared, theme: &Theme, (w, h): (i32, i32)) -> Scene {
    let mut scene = Scene::new(w, h);
    let mut nodes: Vec<NodeId> = Vec::new();
    let mut p = Painter::new(&mut scene, &mut nodes, None);
    p.rect(IRect::new(0, 0, w, h), Rgba::BLACK, 0.0);
    login::paint_backdrop(&mut p, &backdrop_of(s, (w, h)), theme, (w, h));
    p.finish();
    scene
}

/// Centred on its own pixels, as sway's `bg <png> center` puts the splash's
/// frame: the offset whole in device pixels, rounded down.
fn splash_at(b: &Bitmap, (w, h): (i32, i32)) -> (i32, i32) {
    let k = f64::from(b.density);
    let at = |screen: i32, px: u16| ((f64::from(screen) * k - f64::from(px)) / 2.0).floor() / k;
    (at(w, b.pixmap.width()).round() as i32, at(h, b.pixmap.height()).round() as i32)
}

/// The card's cover, words and controls.
/// `ring` is the transport button the cursor is on.
fn now_playing(m: &NowPlaying, ring: Option<usize>, theme: &Theme) -> El {
    let s = &theme.space;
    let title = if m.title.is_empty() { "Unknown title" } else { m.title };
    let mut words = vec![w::label(title).elide()];
    if !m.artist.is_empty() {
        words.push(w::text(m.artist).size(Type::BodySmall).ink(Ink::Muted).elide());
    }
    let mut info = Vec::new();
    if let Some(art) = m.art {
        info.push(w::picture(Some(art.clone()), login::picture_size(theme)));
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
    let group = w::group(options, usize::MAX, false).stop("transport").ring(ring.unwrap_or(0));
    rows.push(group.centred());
    w::column(s.sm, rows)
}

/// The ink for an already composited backdrop luma.
fn ink_of(backdrop: f64) -> &'static str {
    let l = Rgba::linear(backdrop.clamp(0.0, 255.0) / 255.0);
    if Rgba::contrast(1.0, l) >= Rgba::contrast(0.0, l) { "light" } else { "dark" }
}

/// The ink over `rect`, the backdrop luma it was chosen against, and
/// whether that luma was read off the wallpaper.
fn ink(s: &Shared, theme: &Theme, rect: IRect, size: (i32, i32)) -> (&'static str, f64, bool) {
    let fallback = if s.has_wallpaper { 0.0 } else { theme.colors.get(ColorRole::Background).luma601() };
    let mean = s.picture.and_then(|p| {
        let (pw, ph) = (p.width as i32, p.height as i32);
        if (pw, ph) == size {
            return mean_luma(p, rect);
        }
        if !login::fills_size((pw, ph), size) {
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

/// The desktop's picture with the scrim baked in, for the pool, and with
/// `cut` the same pixels over the picture's depth map, for in front of the
/// clock. Its grid comes off first: the lock draws its own over the scrim,
/// or none.
pub fn backdrop(p: &Picture, cut: Option<DepthCut>) -> (Bitmap, Option<Bitmap>) {
    let mut rgba = login::rgba(&p.bgra);
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
        draw_splash(&mut view, Some(&pink));
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
    fn one_cursor_walks_the_transport_then_the_buttons() {
        let row = controls(true, &[Button::SHUTDOWN, Button::REBOOT]);
        let mut c = Cursor::default();
        for _ in 0..4 {
            assert_eq!(c.key(&row, TKey::Tab), (true, None));
        }
        assert_eq!(c.on(&row), Some(Control::Button(Button::SHUTDOWN)));
        assert_eq!(c.key(&row, TKey::Enter), (true, Some(Control::Button(Button::SHUTDOWN))));
        let bare = controls(false, &[Button::SHUTDOWN, Button::REBOOT]);
        assert_eq!(c.on(&bare), None);
        c.clear();
        assert_eq!(c.key(&bare, TKey::Tab), (true, None));
        assert_eq!(c.on(&bare), Some(Control::Button(Button::SHUTDOWN)));
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
            let shared = Shared { has_wallpaper: true, backdrop: Some(&white), blanked: true, wake, ..Shared::new("", prompt("", (0, 0), false)) };
            let mut view = View::new(w, h);
            draw(&mut view, &shared, &theme, &mut kit, Instant::now());
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
        assert!(theme.colors.get(ColorRole::Background).luma601() > 200.0);
        let mut kit = Kit::new(&theme);
        // Big enough that the corner is clear of the column.
        let (w, h) = (900, 700);
        let dim =Bitmap::from_rgba(w as u16, h as u16, [24, 24, 24, 255].repeat((w * h) as usize));
        let mut corner = |backdrop: Option<&Bitmap>, has_wallpaper: bool, wake: f32| {
            let shared = Shared { has_wallpaper, backdrop, blanked: true, wake, ..Shared::new("", prompt("", (0, 0), false)) };
            let mut view = View::new(w, h);
            draw(&mut view, &shared, &theme, &mut kit, Instant::now());
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
            let shared = Shared::new("", prompt("", (0, 0), checking));
            let mut view = View::new(900, 700);
            let now = Instant::now();
            draw(&mut view, &shared, &theme, &mut kit, now);
            draw(&mut view, &shared, &theme, &mut kit, now + std::time::Duration::from_millis(300)).0
        };
        assert!(animating(true));
        assert!(!animating(false));
        assert_eq!(prompt("", (0, 0), true).placeholder, "Unlocking\u{2026}");
    }

    #[test]
    fn whole_numbers_print_like_js() {
        assert_eq!(fs_js::json_number(0.0).to_string(), "0");
        assert_eq!(fs_js::json_number(12.5).to_string(), "12.5");
    }
}
