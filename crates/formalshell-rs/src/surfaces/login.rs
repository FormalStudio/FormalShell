//! The card the greeter and the lock screen share: the date and clock on
//! its leading side over whatever that state adds (the greeter's session
//! and power controls, the lock's now playing), the user's picture, name
//! and password field on its trailing side. The wallpaper carries the rest
//! of the screen: one [`View`] per output draws both, over the
//! [`Backdrop`] each hands it.

use std::sync::Arc;
use std::time::Instant;

use chrono::{DateTime, Local};
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use fs_theme::tokens::WEIGHTS;

use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::services::power;
use crate::paint::{Kit, Painter};
use crate::text::{ShapedText, TextStyle};
use crate::ui::{self, El, Ink, Size, Type, Ui, w};

/// A press on a row of the session or user menu, by its index.
pub enum Press {
    Session(usize),
    User(usize),
}

impl crate::ui::on::HitId for Press {}

impl std::fmt::Display for Press {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Press::Session(i) => write!(f, "session:{i}"),
            Press::User(i) => write!(f, "user:{i}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Closed,
    Sessions,
    Users,
}

/// The password field and what sits over it.
#[derive(Clone)]
pub struct Prompt<'a> {
    /// What the field shows: a secret's dots, or a username as typed.
    pub text: String,
    pub placeholder: &'a str,
    /// A question that is not the password (an OTP prompt); empty draws none.
    pub label: &'a str,
    /// Non-empty replaces the field with this message.
    pub unavailable: &'a str,
    pub enabled: bool,
    /// A check is out: the field breathes on the pulse clock until it answers.
    pub busy: bool,
    /// The caret and the selection's other end, as char counts.
    pub caret: (usize, usize),
}

impl<'a> Prompt<'a> {
    /// The field showing `typed`, one dot a character when `masked`.
    pub fn new(typed: &str, masked: bool, caret: (usize, usize), placeholder: &'a str) -> Self {
        let text = if masked { "\u{2022}".repeat(typed.chars().count()) } else { typed.to_owned() };
        Self { text, placeholder, label: "", unavailable: "", enabled: true, busy: false, caret }
    }

    /// While a check is out: the field closed, saying `busy` until it answers.
    pub fn checking(self, busy: &'a str) -> Self {
        Self { placeholder: busy, enabled: false, busy: true, ..self }
    }
}

/// What the card shows, the same on every output.
pub struct Card<'a> {
    pub now: DateTime<Local>,
    pub name: &'a str,
    pub avatar: Option<&'a Bitmap>,
    pub prompt: Prompt<'a>,
    pub error: &'a str,
    /// Under the clock: the greeter's controls or the lock's now playing.
    pub below: Option<El>,
    /// Under the field: the lock's buttons.
    pub under_field: Option<El>,
}

impl Card<'_> {
    /// The clock, bigger than the type scale goes, so it is painted rather
    /// than laid out; the body keeps a slot its height.
    pub fn clock(&self, theme: &Theme, kit: &mut Kit) -> ShapedText {
        let size = (theme.font_size.display_large * 2.0).round() as f32;
        let text = self.now.format("%H:%M").to_string();
        let style = TextStyle { family: kit.look.mono, size, weight: WEIGHTS.semibold as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        kit.shape(&text, style)
    }

    fn date(&self) -> El {
        w::text(self.now.format("%A, %B %-d").to_string()).size(Type::Subtitle).ink(Ink::Primary)
    }

    /// The date, the clock's slot and whatever sits under them.
    fn leading(&self, clock: &ShapedText, theme: &Theme) -> El {
        let s = &theme.space;
        let mut leading = vec![self.date(), w::space(f64::from(clock_slot(clock).1))];
        if let Some(below) = &self.below {
            leading.push(w::space(s.sm));
            leading.push(below.clone());
        }
        w::column(s.xs, leading).width(Size::Px(leading_width(theme)))
    }

    pub fn body(&self, clock: &ShapedText, theme: &Theme) -> El {
        let s = &theme.space;
        let field_w = s.popup_width_narrow;

        let size = picture_size(theme);
        let picture = match self.avatar {
            Some(b) => w::picture(Some(b.clone()), size),
            None => w::icon("user").size(Type::DisplayLarge).ink(Ink::Muted).width(Size::Px(size)).height(size),
        };
        let mut trailing = vec![picture.centred()];
        if !self.name.is_empty() {
            trailing.push(w::label(self.name).size(Type::Subtitle).centred());
        }
        let pr = &self.prompt;
        if !pr.label.is_empty() {
            trailing.push(w::caption(pr.label).centred());
        }
        if pr.unavailable.is_empty() {
            let error = (!self.error.is_empty()).then_some(self.error);
            trailing.push(w::input(&pr.text, pr.placeholder, pr.enabled, error).caret(pr.caret).width(Size::Px(field_w)).enabled(pr.enabled).pulse(pr.busy));
        } else {
            trailing.push(w::text(pr.unavailable).size(Type::BodySmall).ink(Ink::Muted).width(Size::Px(field_w)).centred());
        }
        if let Some(el) = &self.under_field {
            trailing.push(el.clone());
        }

        w::row(gap(theme), vec![self.leading(clock, theme), w::column(s.sm, trailing).width(Size::Px(field_w))]).width(Size::Px(width(theme)))
    }

    /// Paints the clock into its slot in a body laid out at `inner`.
    pub fn paint_clock(&self, p: &mut Painter, clock: &ShapedText, inner: Rect, theme: &Theme, kit: &mut Kit, alpha: f32) -> IRect {
        let (_, date_h) = ui::measure(&self.date(), width(theme), theme, kit);
        // The body's row centres the shorter column, so the date sits that far down.
        let (_, leading_h) = ui::measure(&self.leading(clock, theme), width(theme), theme, kit);
        let top = inner.y0 + ((inner.height() - leading_h) / 2.0).max(0.0);
        let (over, slot) = clock_slot(clock);
        let at = (inner.x0.round() as i32, (top + date_h + theme.space.xs).round() as i32);
        let ink = theme.colors.get(ColorRole::Foreground);
        p.text(clock, (at.0, at.1 + over), ink.with_alpha(ink.a * alpha), &[]);
        IRect::new(at.0, at.1, clock.width, slot)
    }
}

fn gap(theme: &Theme) -> f64 {
    theme.space.lg * 2.0
}

/// Wider than the field, so a session chip and a user chip share a row.
fn leading_width(theme: &Theme) -> f64 {
    (theme.space.popup_width_narrow * 1.25).round()
}

/// The body's width: the clock side, the field side and the gap between.
pub fn width(theme: &Theme) -> f64 {
    leading_width(theme) + theme.space.popup_width_narrow + gap(theme)
}

/// The side of the card's pictures, the avatar and the lock's cover, which
/// they are decoded at.
pub fn picture_size(theme: &Theme) -> f64 {
    (theme.space.control_height * 2.0).round()
}

/// The icon a power action's button carries, on the greeter and the lock.
pub fn power_icon(action: power::Action) -> &'static str {
    match action {
        power::Action::Suspend => "moon",
        power::Action::Reboot => "rotate-cw",
        power::Action::PowerOff => "power",
    }
}

/// A decoded wallpaper's BGRA as a bitmap's RGBA, opaque.
pub fn rgba(bgra: &[u8]) -> Vec<u8> {
    bgra.chunks_exact(4).flat_map(|px| [px[2], px[1], px[0], 255]).collect()
}

/// Where the card sits on a `size` screen, `rise` px below its rest, and
/// the rect its body is laid out in.
pub fn place(body: &El, size: (i32, i32), rise: f64, theme: &Theme, kit: &mut Kit) -> (IRect, Rect) {
    let (_, bh) = ui::measure(body, width(theme), theme, kit);
    let bw = width(theme);
    let pad = theme.space.panel_padding * 1.5;
    let (cw, ch) = (bw + pad * 2.0, bh + pad * 2.0);
    // A little above centre, where the eye lands first.
    let x = ((size.0 as f64 - cw) / 2.0).round();
    let y = (((size.1 as f64 - ch) * 0.42).round().max(0.0) + rise).round();
    (IRect::new(x as i32, y as i32, cw.round() as i32, ch.round() as i32), Rect::new(x + pad, y + pad, x + pad + bw, y + pad + bh))
}

/// The backdrop grid over a screen `size` across, at `alpha` of its ink,
/// where `lock.grid` asks for it: over the lock's scrim, so it reads at the
/// desktop's strength, and over the greeter's wallpaper.
pub fn paint_grid(p: &mut Painter, theme: &Theme, size: (i32, i32), alpha: f32) {
    if !theme.lock_grid || alpha <= 0.0 {
        return;
    }
    let g = crate::services::wallpaper::grid(theme, 1.0);
    let ink = theme.box_style(Role::BackdropGrid, None).fill;
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let from = g.fade_from(h);
    let at = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let (x0, y0) = (x0.round() as i32, y0.round() as i32);
        IRect::new(x0, y0, x1.round() as i32 - x0, y1.round() as i32 - y0)
    };
    for l in g.lines(w, h) {
        if l.x1 - l.x0 >= w {
            let a = ink.a * alpha * g.strength((l.y0 + l.y1) / 2.0, h) as f32;
            p.rect(at(l.x0, l.y0, l.x1, l.y1), ink.with_alpha(a), 0.0);
        } else {
            let top = ink.with_alpha(ink.a * alpha);
            p.rect(at(l.x0, l.y0, l.x1, from), top, 0.0);
            p.shape(at(l.x0, from, l.x1, h), Paint::Face { from: top, to: top.with_alpha(0.0), radius: 0.0, border: 0.0 });
        }
    }
}

pub fn paint(p: &mut Painter, rect: IRect, theme: &Theme, alpha: f32) {
    let card = theme.box_style(Role::Card, Some("dialog"));
    ui::boxes::paint(p, rect, &card, theme.box_radius(&card, rect.h as f64), alpha, 0.0);
}

/// The greeter's controls: the session chip, the user chip with more than
/// one user, and power. A chip's menu floats under it.
pub struct Controls<'a> {
    pub sessions: &'a [String],
    pub session: usize,
    pub users: &'a [String],
    pub user: usize,
    pub menu: Menu,
}

impl Controls<'_> {
    pub fn el(&self, theme: &Theme) -> El {
        let s = &theme.space;
        // Power on its own row: beside two chips it runs out of the column.
        let (items, current, press): (&[String], usize, fn(usize) -> Press) = match self.menu {
            Menu::Sessions => (self.sessions, self.session, Press::Session),
            Menu::Users => (self.users, self.user, Press::User),
            Menu::Closed => (&[][..], 0, Press::Session),
        };
        let list = || {
            let rows = items
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let check = w::icon("check").size(Type::BodySmall).ink(if i == current { Ink::Fg } else { Ink::Color(Rgba::TRANSPARENT) });
                    w::cell(w::row(s.icon_gap, vec![check, w::text(label.clone()).size(Type::BodySmall).elide()])).selected(i == current).interactive().on(press(i))
                })
                .collect();
            w::column(0.0, rows)
        };
        let mut chips = Vec::new();
        if let Some(name) = self.sessions.get(self.session).filter(|_| self.sessions.len() > 1) {
            let chip = w::trigger(s, "monitor", name, self.menu == Menu::Sessions).on("sessions");
            chips.push(if self.menu == Menu::Sessions { chip.float(list()) } else { chip });
        }
        if let Some(name) = self.users.get(self.user).filter(|_| self.users.len() > 1) {
            let chip = w::trigger(s, "users", name, self.menu == Menu::Users).on("users");
            chips.push(if self.menu == Menu::Users { chip.float(list()) } else { chip });
        }
        let power = power::Action::ALL.map(|a| w::icon_button(power_icon(a)).on(a.id())).to_vec();
        let mut col = Vec::new();
        if !chips.is_empty() {
            col.push(w::row(s.sm, chips));
        }
        col.push(w::row(s.xs, power));
        w::column(s.sm, col).fill()
    }
}

/// What the card stands on, over black.
pub struct Backdrop<'a> {
    pub under: Under<'a>,
    /// The desktop's sculpture held still, stretched over the whole output.
    pub shards: Option<&'a Bitmap>,
    /// The backdrop's pixels where the wallpaper's subject stands, the rest
    /// clear: drawn over the clock.
    pub foreground: Option<&'a Bitmap>,
    /// The blank and wake crossfade: 0 is black, 1 the whole screen.
    pub wake: f32,
}

pub enum Under<'a> {
    /// A picture at the output's own pixels, or stretched over it.
    Picture(&'a Bitmap),
    /// A wallpaper still decoding: dark, never the theme's background.
    Dark,
    /// The theme's background, under a black scrim at this opacity.
    Flat { scrim: f32 },
}

impl<'a> Backdrop<'a> {
    /// `under` alone, awake.
    pub fn plain(under: Under<'a>) -> Self {
        Self { under, shards: None, foreground: None, wake: 1.0 }
    }
}

/// The picture, the sculpture and the grid over the black under them.
pub fn paint_backdrop(p: &mut Painter, b: &Backdrop, theme: &Theme, (w, h): (i32, i32)) {
    let full = IRect::new(0, 0, w, h);
    match b.under {
        Under::Picture(pic) if (pic.pixmap.width() as i32, pic.pixmap.height() as i32) == (w, h) => p.image(pic, (0, 0), b.wake),
        Under::Picture(pic) => p.shape(full, Paint::Picture { image: pic.clone(), alpha: b.wake, radius: 0.0 }),
        Under::Dark => {}
        Under::Flat { scrim } => {
            let bg = theme.colors.get(ColorRole::Background);
            p.rect(full, bg.with_alpha(bg.a * b.wake), 0.0);
            if scrim > 0.0 {
                p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: scrim }, 0.0);
            }
        }
    }
    if let Some(s) = b.shards {
        p.shape(full, Paint::Picture { image: s.clone(), alpha: b.wake, radius: 0.0 });
    }
    paint_grid(p, theme, (w, h), b.wake);
}

pub struct View {
    pub scene: Scene,
    pub ui: Ui,
    nodes: Vec<NodeId>,
    over_clock: Option<OverClock>,
}

pub struct Drawn {
    /// Something on the card is still moving.
    pub animating: bool,
    /// The card's rect at rest.
    pub card: IRect,
    /// Where the foreground covers the clock.
    pub depth: Option<IRect>,
}

impl View {
    pub fn new(width: i32, height: i32) -> Self {
        Self { scene: Scene::new(width, height), ui: Ui::new(None), nodes: Vec::new(), over_clock: None }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        if (self.scene.size.w, self.scene.size.h) != (width, height) {
            self.scene.resize(width, height);
            self.scene.touch(self.scene.size);
        }
    }

    /// The card over `b`, `enter` its opacity and how far it still has to
    /// rise.
    pub fn draw(&mut self, c: &Card, b: &Backdrop, enter: (f32, f64), theme: &Theme, kit: &mut Kit, now: Instant) -> Drawn {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let fade = enter.0 * b.wake;
        let rise = enter.1.round();
        let clock = c.clock(theme, kit);
        let body = c.body(&clock, theme);
        let (card, inner) = place(&body, (w, h), rise, theme, kit);
        let rest = IRect::new(card.x, card.y - rise as i32, card.w, card.h);

        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        // Black under everything, the screen faded in over it: a light
        // theme's `background` under a part-faded backdrop is a white flash
        // in a dark room on every blank and wake.
        p.rect(IRect::new(0, 0, w, h), Rgba::BLACK, 0.0);
        paint_backdrop(&mut p, b, theme, (w, h));
        paint(&mut p, card, theme, fade);
        let clock_rect = c.paint_clock(&mut p, &clock, inner, theme, kit, fade);
        let front = b.foreground.and_then(|fg| over_clock(&mut self.over_clock, fg, clock_rect, card, theme, (w, h)));
        if let Some((rect, front, over)) = &front {
            p.shape(*rect, Paint::Picture { image: front.clone(), alpha: b.wake, radius: 0.0 });
            p.shape(*rect, Paint::Picture { image: over.clone(), alpha: fade, radius: 0.0 });
        }
        let last = p.last();
        p.finish();
        self.ui.anchor = last;
        let animating = self.ui.draw(&body, inner, None, fade, theme, kit, &mut self.scene, now).animating;
        Drawn { animating, card: rest, depth: front.map(|(r, _, _)| r) }
    }

    /// Nothing but black, and a picture at its place: the lock blanked, or
    /// the boot lock's splash before any text is shaped.
    pub fn bare(&mut self, picture: Option<(&Bitmap, (i32, i32))>) {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        p.rect(IRect::new(0, 0, w, h), Rgba::BLACK, 0.0);
        if let Some((b, at)) = picture {
            p.image(b, at, 1.0);
        }
        p.finish();
        self.ui.hide(&mut self.scene);
    }
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
    let card = theme.box_style(Role::Card, Some("dialog"));
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
        paint(&mut p, IRect::new(0, 0, card_rect.w, card_rect.h), theme, 1.0);
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
pub fn fills(b: &Bitmap, size: (i32, i32)) -> bool {
    fills_size((i32::from(b.pixmap.width()), i32::from(b.pixmap.height())), size)
}

pub fn fills_size((bw, bh): (i32, i32), (w, h): (i32, i32)) -> bool {
    let (bw, bh, w, h) = (i64::from(bw), i64::from(bh), i64::from(w), i64::from(h));
    bw > w && (bw * h - bh * w).abs() <= bw.max(bh)
}

/// How far the clock's ink rises past its line box, and the slot that
/// holds all of it. A heavy display cut's figures stand taller than the
/// face's own ascent says, and would cross the date over them.
fn clock_slot(clock: &ShapedText) -> (i32, i32) {
    let pad = crate::text::PAD;
    let (top, bottom) = clock.glyphs.iter().filter(|g| g.image.is_none()).fold((0, clock.line_height()), |(top, bottom), g| {
        let b = vello_cpu::kurbo::Shape::bounding_box(g.path.as_ref());
        (top.min(g.y - pad + b.y0.floor() as i32), bottom.max(g.y - pad + b.y1.ceil() as i32))
    });
    (-top, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_foreground_covers_the_clock_as_the_card_shows_it_and_is_cut_once() {
        use crate::render::Renderer;
        let theme = Theme::default();
        let mut kit = Kit::new(&theme);
        let (w, h) = (900, 700);
        let red = Bitmap::from_rgba(w as u16, h as u16, [255, 0, 0, 255].repeat((w * h) as usize));
        let card = Card { now: Local::now(), name: "", avatar: None, prompt: Prompt::new("", true, (0, 0), "Password"), error: "", below: None, under_field: None };
        let backdrop = |fg| Backdrop { foreground: fg, ..Backdrop::plain(Under::Picture(&red)) };
        let mut view = View::new(w, h);
        assert_eq!(view.draw(&card, &backdrop(None), (1.0, 0.0), &theme, &mut kit, Instant::now()).depth, None);
        assert!(view.draw(&card, &backdrop(Some(&red)), (1.0, 0.0), &theme, &mut kit, Instant::now()).depth.is_some());
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
        view.draw(&card, &backdrop(Some(&red)), (1.0, 0.0), &theme, &mut kit, Instant::now());
        assert!(Arc::ptr_eq(&view.over_clock.as_ref().unwrap().front.pixmap, &crop.pixmap), "a settled lock cut the crop again");
    }
}
