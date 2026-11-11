//! The focused Mac app's menu bar: its top-level titles, each a hit that
//! opens its menu under it, the application menu first under the app's
//! name. Off the strip unless a Mac window holds focus and its app
//! registered a menu. A vertical strip has no room for titles: one `menu`
//! icon there opens the whole menu, its titles as rows.

use std::time::Instant;

use fs_theme::tokens::WEIGHTS;
use fs_types::IRect;

use crate::paint::{Ink, Kit, Look, Painter};
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Custom, Env, Summary, View};

#[derive(Default)]
pub struct AppMenu {
    /// Each title's dbusmenu id and label.
    titles: Vec<(i32, String)>,
    vertical: bool,
    /// Where each title starts along the cell, and its extent.
    boxes: Vec<(f64, f64)>,
    along: f64,
    hover: Option<usize>,
    open: Option<i32>,
}

impl AppMenu {
    /// The menu whose card is up; true when it changed.
    pub fn set_open(&mut self, open: Option<i32>) -> bool {
        std::mem::replace(&mut self.open, open) != open
    }

    /// The centre of the title `id` from the cell's start.
    pub fn title_mid(&self, id: i32) -> Option<f64> {
        if self.vertical {
            return Some(self.along / 2.0);
        }
        let i = self.titles.iter().position(|(t, _)| *t == id)?;
        self.boxes.get(i).map(|(start, extent)| start + extent / 2.0)
    }

    /// Each title's id, where it starts along the cell and its extent.
    pub fn title_boxes(&self) -> impl Iterator<Item = (i32, f64, f64)> + '_ {
        self.titles.iter().zip(&self.boxes).map(|((id, _), (start, extent))| (*id, *start, *extent))
    }

    /// The title under a point `along` the cell: the whole menu's on a
    /// vertical strip.
    pub fn title_at(&self, along: f64) -> Option<i32> {
        if self.vertical {
            return self.titles.first().map(|_| 0);
        }
        self.box_at(along).map(|i| self.titles[i].0)
    }

    fn box_at(&self, along: f64) -> Option<usize> {
        self.boxes.iter().position(|(s, e)| along >= *s && along < s + e)
    }
}

impl Cell for AppMenu {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::AppMenu]
    }

    fn read(&mut self, env: &Env) -> bool {
        let titles: Vec<(i32, String)> = env.store.appmenu.titles().iter().map(|t| (t.id, t.label.clone())).collect();
        let vertical = env.edge.is_vertical();
        let changed = titles != self.titles || vertical != self.vertical;
        if titles != self.titles {
            self.hover = None;
        }
        self.titles = titles;
        self.vertical = vertical;
        changed
    }

    fn view(&self, look: &Look) -> View {
        match self.titles.first() {
            None => View::hidden(),
            Some((_, app)) if self.vertical => View::icon("menu", look).tooltip(format!("{app} menu")),
            Some(_) => View::new(Vec::new(), 0.0),
        }
    }

    fn summary(&self) -> Option<Summary> {
        let (_, app) = self.titles.first()?;
        Some(Summary::new("menu", "Menu", app.clone()))
    }

    fn click(&mut self, button: Button, at: (f64, f64), _: &Env) -> Action {
        if button != Button::Left {
            return Action::None;
        }
        if self.vertical {
            return Action::AppMenu { id: 0, offset: 0.0 };
        }
        let Some(i) = self.box_at(at.0) else { return Action::None };
        let (start, extent) = self.boxes[i];
        Action::AppMenu { id: self.titles[i].0, offset: start + extent / 2.0 - self.along / 2.0 }
    }

    fn custom(&mut self) -> Option<&mut dyn Custom> {
        (!self.vertical).then_some(self)
    }
}

/// The application menu's title is the app's name, heavier as on the Mac.
fn weight(i: usize) -> f64 {
    if i == 0 { WEIGHTS.semibold } else { WEIGHTS.medium }
}

impl Custom for AppMenu {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, _band: bool) -> f64 {
        self.vertical = vertical;
        let look = kit.look.clone();
        let mut at = 0.0;
        self.boxes.clear();
        for (i, (_, label)) in self.titles.iter().enumerate() {
            let style = kit.weight(look.sans, look.body, weight(i));
            let extent = f64::from(kit.shape(label, style).width) + look.pad_x * 2.0;
            self.boxes.push((at, extent));
            at += extent;
        }
        self.along = at;
        at
    }

    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, _: Instant) {
        let look = kit.look.clone();
        for (i, ((id, label), (start, extent))) in self.titles.iter().zip(&self.boxes).enumerate() {
            let cell = IRect::new(rect.x + start.round() as i32, rect.y, extent.round() as i32, rect.h);
            let open = self.open == Some(*id);
            if open {
                p.rect(cell, ink.of(look.ghost_open), look.radius);
            } else if self.hover == Some(i)
                && let Some(wash) = look.hover_wash
            {
                p.rect(cell, ink.of(wash), look.radius);
            }
            let style = kit.weight(look.sans, look.body, weight(i));
            let shaped = kit.shape(label, style);
            p.text(&shaped, (cell.x + (cell.w - shaped.width) / 2, cell.y + (cell.h - shaped.line_height()) / 2), ink.of(ink.fg), &ink.glow);
        }
    }

    fn animating(&self, _: Instant) -> bool {
        false
    }

    fn own_hover(&self) -> bool {
        true
    }

    fn pointer(&mut self, at: Option<(f64, f64)>) -> (bool, bool) {
        let hover = at.and_then(|(x, _)| self.box_at(x));
        let changed = hover != self.hover;
        self.hover = hover;
        (changed, false)
    }
}
