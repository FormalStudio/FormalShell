//! The card the greeter and the lock screen share: the date and clock on
//! its leading side over whatever that state adds (the greeter's session
//! and power controls, the lock's now playing), the user's picture, name
//! and password field on its trailing side. The wallpaper carries the rest
//! of the screen.

use std::time::Instant;

use chrono::{DateTime, Local};
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use fs_theme::tokens::WEIGHTS;

use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{ShapedText, TextStyle};
use crate::ui::{self, El, Ink, Size, Type, Ui, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    Closed,
    Sessions,
    Users,
}

/// What the card shows, the same on every output.
pub struct Card<'a> {
    pub now: DateTime<Local>,
    pub name: &'a str,
    pub avatar: Option<&'a Bitmap>,
    /// What the field shows: dots, or a username as typed.
    pub text: String,
    pub placeholder: &'a str,
    /// A question that is not the password (an OTP prompt); empty draws none.
    pub label: &'a str,
    pub error: &'a str,
    /// Non-empty replaces the field with this message.
    pub unavailable: &'a str,
    pub enabled: bool,
    pub busy: bool,
    pub caret: (usize, usize),
    /// Under the clock: the greeter's controls or the lock's now playing.
    pub below: Option<El>,
}

impl Card<'_> {
    /// The clock, bigger than the type scale goes, so it is painted rather
    /// than laid out; the body keeps a slot its height.
    pub fn clock(&self, theme: &Theme, kit: &mut Kit) -> ShapedText {
        let size = (theme.font_size.display_large * 2.0).round() as f32;
        let text = self.now.format("%H:%M").to_string();
        // A table that sets the clock's axes sets it in its display face.
        let wght = theme.metric("lock.clock", "wght") as f32;
        let style = if wght > 0.0 {
            let wdth = theme.metric("lock.clock", "wdth") as f32;
            let family = crate::surfaces::bar::cell::face(theme.faces.display);
            TextStyle { family, size, weight: wght, tracking: 0.0, wdth: if wdth > 0.0 { wdth } else { TextStyle::NORMAL_WIDTH }, wght: Some(wght) }
        } else {
            TextStyle { family: kit.look.mono, size, weight: WEIGHTS.semibold as f32, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
        };
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
            leading.push(w::space(s.md));
            leading.push(below.clone());
        }
        w::column(s.xs, leading).width(Size::Px(leading_width(theme)))
    }

    pub fn body(&self, clock: &ShapedText, theme: &Theme) -> El {
        let s = &theme.space;
        let field_w = s.popup_width_narrow;

        let size = avatar_size(theme);
        let picture = match self.avatar {
            Some(b) => w::picture(Some(b.clone()), size),
            None => w::icon("user").size(Type::DisplayLarge).ink(Ink::Muted).width(Size::Px(size)).height(size),
        };
        let mut trailing = vec![picture.centred()];
        if !self.name.is_empty() {
            trailing.push(w::label(self.name).size(Type::Subtitle).centred());
        }
        if !self.label.is_empty() {
            trailing.push(w::caption(self.label).centred());
        }
        if self.unavailable.is_empty() {
            let error = (!self.error.is_empty()).then_some(self.error);
            trailing.push(w::input(&self.text, self.placeholder, self.enabled, error).caret(self.caret).width(Size::Px(field_w)).enabled(self.enabled).pulse(self.busy));
        } else {
            trailing.push(w::text(self.unavailable).size(Type::BodySmall).ink(Ink::Muted).width(Size::Px(field_w)).centred());
        }

        w::row(gap(theme), vec![self.leading(clock, theme), w::column(s.md, trailing).width(Size::Px(field_w))]).width(Size::Px(width(theme)))
    }

    /// Paints the clock into its slot in a body laid out at `inner`.
    pub fn paint_clock(&self, p: &mut Painter, clock: &ShapedText, inner: Rect, theme: &Theme, kit: &mut Kit, alpha: f32) -> IRect {
        let (_, date_h) = ui::measure(&self.date(), width(theme), theme, kit);
        // The body's row centres the shorter column, so the date sits that far down.
        let (_, leading_h) = ui::measure(&self.leading(clock, theme), width(theme), theme, kit);
        let top = inner.y0 + ((inner.height() - leading_h) / 2.0).max(0.0);
        let (over, slot) = clock_slot(clock);
        let at = (inner.x0.round() as i32, (top + date_h + theme.space.xs).round() as i32);
        let ink = theme.colors.get("foreground");
        p.text(clock, (at.0, at.1 + over), ink.with_alpha(ink.a * alpha), &[]);
        IRect::new(at.0, at.1, clock.width, slot)
    }
}

fn gap(theme: &Theme) -> f64 {
    theme.space.xxl * 2.0
}

/// Wider than the field, so a session chip and a user chip share a row.
fn leading_width(theme: &Theme) -> f64 {
    (theme.space.popup_width_narrow * 1.25).round()
}

/// The body's width: the clock side, the field side and the gap between.
pub fn width(theme: &Theme) -> f64 {
    leading_width(theme) + theme.space.popup_width_narrow + gap(theme)
}

/// The picture's side, which the lock and the greeter decode avatars at.
pub fn avatar_size(theme: &Theme) -> f64 {
    (theme.space.control_height * 2.0).round()
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
    let ink = theme.box_style("backdrop.grid", None).fill;
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
            p.shape(at(l.x0, from, l.x1, h), Paint::Face { from: top, to: top.with_alpha(0.0), radius: 0.0 });
        }
    }
}

pub fn paint(p: &mut Painter, rect: IRect, theme: &Theme, alpha: f32) {
    let card = theme.box_style("card", None);
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
        let (items, current, key) = match self.menu {
            Menu::Sessions => (self.sessions, self.session, "session"),
            Menu::Users => (self.users, self.user, "user"),
            Menu::Closed => (&[][..], 0, ""),
        };
        let list = || {
            let rows = items
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    let check = w::icon("check").size(Type::BodySmall).ink(if i == current { Ink::Fg } else { Ink::Color(Rgba::TRANSPARENT) });
                    w::cell(w::row(s.icon_gap, vec![check, w::text(label.clone()).size(Type::BodySmall).elide()])).selected(i == current).interactive().on(format!("{key}:{i}"))
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
        let power = [("moon", "suspend"), ("rotate-cw", "reboot"), ("power", "poweroff")].map(|(glyph, on)| w::icon_button(glyph).on(on)).to_vec();
        let mut col = Vec::new();
        if !chips.is_empty() {
            col.push(w::row(s.sm, chips));
        }
        col.push(w::row(s.xs, power));
        w::column(s.sm, col).fill()
    }
}

/// One greeter output.
pub struct View {
    pub scene: Scene,
    pub ui: Ui,
    nodes: Vec<NodeId>,
}

impl View {
    pub fn new(width: i32, height: i32) -> Self {
        Self { scene: Scene::new(width, height), ui: Ui::new(None), nodes: Vec::new() }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        if (self.scene.size.w, self.scene.size.h) != (width, height) {
            self.scene.resize(width, height);
            self.scene.touch(self.scene.size);
        }
    }

    /// True while something on the card is still moving.
    pub fn draw(&mut self, c: &Card, backdrop: Option<&Bitmap>, theme: &Theme, kit: &mut Kit, now: Instant, boot: Option<&Stage>) -> bool {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let full = IRect::new(0, 0, w, h);
        let clock = c.clock(theme, kit);
        let body = c.body(&clock, theme);
        let (mut card, mut inner) = place(&body, (w, h), 0.0, theme, kit);
        // Under the boot the emblem perches over the card, the two centred
        // together a little above the middle.
        let mut perch = (f64::from(w) / 2.0, f64::from(h) / 2.0);
        if let Some(s) = boot {
            let k = crate::surfaces::boot::stage_scale((w, h));
            let tall = 2.0 * crate::boot::RINGS[0].ry * k * crate::boot::PERCH;
            let gap = theme.space.xxl;
            let top = ((f64::from(h) - tall - gap - f64::from(card.h)) * 0.42).max(0.0);
            perch.1 = top + tall / 2.0;
            let rise = top + tall + gap - f64::from(card.y) + s.greet.rise;
            (card, inner) = place(&body, (w, h), rise, theme, kit);
        }
        let (world, alpha) = boot.map_or((1.0, 1.0), |s| (s.greet.world, s.greet.card));
        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
        match backdrop {
            Some(b) if (b.pixmap.width() as i32, b.pixmap.height() as i32) == (w, h) => p.image(b, (0, 0), world),
            // Made at the output's own scale, or at its last one while the
            // new size decodes.
            Some(b) if world > 0.0 => p.shape(full, Paint::Picture { image: b.clone(), alpha: world, radius: 0.0 }),
            Some(_) => {}
            None => {
                let bg = theme.colors.get("background");
                p.rect(full, bg.with_alpha(bg.a * world), 0.0);
            }
        }
        paint_grid(&mut p, theme, (w, h), world);
        if let Some(s) = boot {
            let (cx, cy) = (f64::from(w) / 2.0, f64::from(h) / 2.0);
            let e = s.greet.settle;
            let frame = crate::surfaces::boot::Frame {
                t: s.t,
                out: s.out,
                at: (cx, cy + (perch.1 - cy) * e),
                scale: 1.0 + (crate::boot::PERCH - 1.0) * e,
                black: 0.0,
                lines: s.greet.lines,
                emblem: 1.0,
            };
            crate::surfaces::boot::paint(&mut p, theme, (w, h), s.lines, s.plan, s.emblem, &frame);
        }
        if alpha > 0.0 {
            paint(&mut p, card, theme, alpha);
            c.paint_clock(&mut p, &clock, inner, theme, kit, alpha);
        }
        let last = p.last();
        p.finish();
        self.ui.anchor = last;
        if alpha <= 0.0 {
            self.ui.hide(&mut self.scene);
            return false;
        }
        self.ui.draw(&body, inner, None, alpha, theme, kit, &mut self.scene, now).animating
    }
}

/// The boot behind the greeter's card, one frame of it.
pub struct Stage<'a> {
    pub plan: &'a crate::boot::Plan,
    pub lines: &'a [(f64, f64, f64, f64)],
    pub emblem: &'a crate::surfaces::boot::Emblem,
    /// ms into the way in, held once the emblem starts up.
    pub t: f64,
    /// ms into the crumple, once the session is on its way.
    pub out: Option<f64>,
    pub greet: crate::boot::Greet,
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

