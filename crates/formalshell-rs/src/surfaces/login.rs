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
        kit.shape(&self.now.format("%H:%M").to_string(), TextStyle { family: kit.look.mono, size, weight: WEIGHTS.semibold as f32, tracking: 0.0 })
    }

    fn date(&self) -> El {
        w::text(self.now.format("%A, %B %-d").to_string()).size(Type::Subtitle).ink(Ink::Primary)
    }

    pub fn body(&self, clock: &ShapedText, theme: &Theme) -> El {
        let s = &theme.space;
        let field_w = s.popup_width_narrow;
        let mut leading = vec![self.date(), w::space(clock.line_height() as f64)];
        if let Some(below) = &self.below {
            leading.push(w::space(s.md));
            leading.push(below.clone());
        }

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

        w::row(gap(theme), vec![w::column(s.xs, leading).width(Size::Px(leading_width(theme))), w::column(s.md, trailing).width(Size::Px(field_w))]).width(Size::Px(width(theme)))
    }

    /// Paints the clock into its slot in a body laid out at `inner`.
    pub fn paint_clock(&self, p: &mut Painter, clock: &ShapedText, inner: Rect, theme: &Theme, kit: &mut Kit, alpha: f32) -> IRect {
        let (_, date_h) = ui::measure(&self.date(), width(theme), theme, kit);
        let at = (inner.x0.round() as i32, (inner.y0 + date_h + theme.space.xs).round() as i32);
        let ink = theme.colors.get("foreground");
        p.text(clock, at, ink.with_alpha(ink.a * alpha), &[]);
        IRect::new(at.0, at.1, clock.width, clock.line_height())
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
    pub fn draw(&mut self, c: &Card, backdrop: Option<&Bitmap>, theme: &Theme, kit: &mut Kit, now: Instant) -> bool {
        let (w, h) = (self.scene.size.w, self.scene.size.h);
        let full = IRect::new(0, 0, w, h);
        let clock = c.clock(theme, kit);
        let body = c.body(&clock, theme);
        let (card, inner) = place(&body, (w, h), 0.0, theme, kit);
        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        p.rect(full, Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }, 0.0);
        match backdrop {
            Some(b) if (b.pixmap.width() as i32, b.pixmap.height() as i32) == (w, h) => p.image(b, (0, 0), 1.0),
            // Made at the output's own scale, or at its last one while the
            // new size decodes.
            Some(b) => p.shape(full, Paint::Picture { image: b.clone(), alpha: 1.0, radius: 0.0 }),
            None => p.rect(full, theme.colors.get("background"), 0.0),
        }
        paint(&mut p, card, theme, 1.0);
        c.paint_clock(&mut p, &clock, inner, theme, kit, 1.0);
        let last = p.last();
        p.finish();
        self.ui.anchor = last;
        self.ui.draw(&body, inner, None, 1.0, theme, kit, &mut self.scene, now).animating
    }
}
