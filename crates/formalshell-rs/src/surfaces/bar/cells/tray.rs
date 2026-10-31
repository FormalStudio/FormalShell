//! The StatusNotifier items as a rail of icon
//! cells, or, under the default `tray.maxVisible` of 0 and whenever the
//! strip has no room, one dots toggle with the whole tray in the second bar
//! (the `trayoverflow` panel). All or nothing: the strip carries every icon
//! or the toggle alone.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use crate::scene::{Bitmap, IRect, Paint};
use crate::services::tray::Item;
use crate::store::Topic;
use crate::surfaces::bar::cell::{Action, Button, Cell, Custom, Env, Ink, Kit, Look, Painter, TrayClick, View};

/// How many icons the strip kept, for `tray status`.
static INLINE: AtomicUsize = AtomicUsize::new(0);

pub fn inline_count() -> usize {
    INLINE.load(Ordering::Relaxed)
}

/// Bar/tray.js `extent`: n cells and the spacing between them.
fn extent(n: usize, cell: f64, spacing: f64) -> f64 {
    if n == 0 { 0.0 } else { n as f64 * cell + (n - 1) as f64 * spacing }
}

/// Bar/tray.js `needsRoom`: whether the answer depends on a measured strip.
/// A ceiling has already decided for a tray over it, and the default, 0, is
/// over every ceiling.
fn needs_room(total: usize, max_visible: i64) -> bool {
    total > 0 && max_visible != 0 && !(max_visible > 0 && total as i64 > max_visible)
}

/// Bar/tray.js `fit`: the whole tray inline or none of it.
fn fits(total: usize, budget: f64, cell: f64, spacing: f64, max_visible: i64) -> bool {
    needs_room(total, max_visible) && (!(cell > 0.0) || extent(total, cell, spacing) <= budget)
}

pub struct Tray {
    items: Vec<Item>,
    max_visible: i64,
    /// The strip's answer for this tray, and whether it has given one.
    fit: bool,
    decided: bool,
    vertical: bool,
    unit: f64,
    gap: f64,
    along: f64,
    hover: Option<usize>,
    bitmaps: Vec<Option<Bitmap>>,
    bitmap_size: (u32, f64),
}

impl Tray {
    pub fn strip() -> Self {
        Self {
            items: Vec::new(),
            max_visible: 0,
            fit: false,
            decided: false,
            vertical: false,
            unit: 0.0,
            gap: 0.0,
            along: 0.0,
            hover: None,
            bitmaps: Vec::new(),
            bitmap_size: (0, 0.0),
        }
    }

    fn inline(&self) -> usize {
        if self.decided && self.fit { self.items.len() } else { 0 }
    }

    /// The icons drawn on this surface: all of them, or none (the toggle).
    fn icons(&self) -> bool {
        self.inline() > 0
    }

    fn publish(&self) {
        INLINE.store(self.inline(), Ordering::Relaxed);
    }

    /// Where each box starts along the cell, and its extent.
    fn boxes(&self) -> Vec<(f64, f64)> {
        let n = if self.icons() { self.items.len() } else { usize::from(!self.items.is_empty()) };
        (0..n).map(|i| (i as f64 * (self.unit + self.gap), self.unit)).collect()
    }

    fn box_at(&self, along: f64) -> Option<usize> {
        self.boxes().iter().position(|(s, e)| along >= *s && along < s + e)
    }

    fn ensure_bitmaps(&mut self, size: u32) {
        let scale = crate::scene::raster_scale();
        if self.bitmap_size == (size, scale) && self.bitmaps.len() == self.items.len() {
            return;
        }
        self.bitmaps = self.items.iter().map(|i| i.icon.as_ref().and_then(|r| r.bitmap(size, scale))).collect();
        self.bitmap_size = (size, scale);
    }
}

impl Cell for Tray {
    fn reads(&self) -> &'static [Topic] {
        &[Topic::Tray, Topic::Config]
    }

    fn read(&mut self, env: &Env) -> bool {
        let items = env.store.tray.items.clone();
        let max_visible = env.store.config.f64("tray.maxVisible").map_or(0, |n| n as i64);
        let vertical = env.edge.is_vertical();
        let changed = items != self.items || max_visible != self.max_visible || vertical != self.vertical;
        if items.len() != self.items.len() || max_visible != self.max_visible {
            self.decided = false;
        }
        if items != self.items {
            self.bitmaps.clear();
            self.hover = None;
        }
        self.items = items;
        self.max_visible = max_visible;
        self.vertical = vertical;
        self.publish();
        changed
    }

    fn view(&self, look: &Look) -> View {
        if self.items.is_empty() {
            return View::hidden();
        }
        if self.icons() {
            return View::new(Vec::new(), 0.0);
        }
        let name = if self.vertical { "more-vertical" } else { "ellipsis" };
        View::icon(name, look).panel(crate::surfaces::panel::tray::ID).tooltip(format!("TRAY / {} ITEMS", self.items.len()))
    }

    fn click(&mut self, button: Button, at: (f64, f64), _: &Env) -> Action {
        if !self.icons() {
            return if button == Button::Left { Action::Panel(crate::surfaces::panel::tray::ID) } else { Action::None };
        }
        let along = if self.vertical { at.1 } else { at.0 };
        let Some(i) = self.box_at(along) else { return Action::None };
        let item = &self.items[i];
        let click = match button {
            Button::Left if item.only_menu && item.has_menu => TrayClick::Menu,
            Button::Left => TrayClick::Activate,
            Button::Middle => TrayClick::Secondary,
            Button::Right if item.has_menu => TrayClick::Menu,
            Button::Right => return Action::None,
        };
        let (start, e) = self.boxes()[i];
        Action::Tray { id: item.id.clone(), click, offset: start + e / 2.0 - self.along / 2.0 }
    }

    fn set_open(&mut self, _: bool) -> bool {
        false
    }

    /// The item's own words, else its title, else its id.
    fn tip_at(&self) -> Option<(String, f64, f64)> {
        let i = self.hover.filter(|_| self.icons())?;
        let item = &self.items[i];
        let text = [&item.tooltip, &item.title, &item.id].into_iter().find(|t| !t.is_empty())?.clone();
        let (start, e) = *self.boxes().get(i)?;
        Some((text, start, e))
    }

    fn custom(&mut self) -> Option<&mut dyn Custom> {
        Some(self)
    }
}

impl Custom for Tray {
    fn measure(&mut self, kit: &mut Kit, vertical: bool, _band: bool) -> f64 {
        let look = &kit.look;
        self.vertical = vertical;
        self.unit = look.body as f64 + look.pad_x * 2.0;
        self.gap = look.sm;
        self.along = if self.icons() { extent(self.items.len(), self.unit, self.gap) } else { self.unit };
        self.along
    }

    fn draw(&mut self, kit: &mut Kit, p: &mut Painter, rect: IRect, ink: &Ink, _: Instant) {
        let look = kit.look.clone();
        let size = look.body.round().max(1.0) as u32;
        let boxes = self.boxes();
        if !self.icons() {
            let Some((start, e)) = boxes.first().copied() else { return };
            let name = if self.vertical { "more-vertical" } else { "ellipsis" };
            let shaped = kit.icon(name);
            let c = centre(rect, self.vertical, start + e / 2.0);
            p.text(&shaped, (c.0 - shaped.width / 2, c.1 - shaped.line_height() / 2), ink.of(ink.fg), &ink.glow);
            return;
        }
        self.ensure_bitmaps(size);
        for (i, (start, e)) in boxes.iter().enumerate() {
            let c = centre(rect, self.vertical, start + e / 2.0);
            if self.hover == Some(i) {
                if let Some(wash) = look.hover_wash {
                    let cell = if self.vertical {
                        IRect::new(rect.x, rect.y + start.round() as i32, rect.w, e.round() as i32)
                    } else {
                        IRect::new(rect.x + start.round() as i32, rect.y, e.round() as i32, rect.h)
                    };
                    p.rect(cell, ink.of(wash), look.radius);
                }
            }
            let at = IRect::new(c.0 - size as i32 / 2, c.1 - size as i32 / 2, size as i32, size as i32);
            match &self.bitmaps[i] {
                Some(image) => p.shape(at, Paint::Image { image: image.clone(), alpha: ink.alpha }),
                None => {
                    // No picture and no way to draw one: the shell's own
                    // unknown mark, never a stand-in from another icon set.
                    let shaped = kit.icon("circle-help");
                    p.text(&shaped, (c.0 - shaped.width / 2, c.1 - shaped.line_height() / 2), ink.of(ink.dim), &[]);
                }
            }
        }
    }

    fn animating(&self, _: Instant) -> bool {
        false
    }

    fn own_hover(&self) -> bool {
        self.icons()
    }

    fn pointer(&mut self, at: Option<(f64, f64)>) -> (bool, bool) {
        let hover = at.and_then(|(x, y)| self.box_at(if self.vertical { y } else { x }));
        let changed = hover != self.hover;
        self.hover = hover;
        (changed, false)
    }

    fn room(&mut self, budget: f64) -> bool {
        let fit = fits(self.items.len(), budget, self.unit, self.gap, self.max_visible);
        let was = self.inline();
        self.fit = fit;
        self.decided = true;
        self.publish();
        self.inline() != was
    }
}

/// The centre of the box starting `mid` along the cell, in the window.
fn centre(rect: IRect, vertical: bool, mid: f64) -> (i32, i32) {
    if vertical {
        (rect.x + rect.w / 2, rect.y + mid.round() as i32)
    } else {
        (rect.x + mid.round() as i32, rect.y + rect.h / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_ceiling_is_the_second_bar() {
        assert!(!needs_room(6, 0));
        assert!(!fits(6, f64::INFINITY, 30.0, 8.0, 0));
    }

    #[test]
    fn a_ceiling_over_the_count_hands_the_tray_over() {
        assert!(!needs_room(6, 4));
        assert!(needs_room(4, 4));
        assert!(needs_room(6, -1));
    }

    #[test]
    fn room_has_the_last_word() {
        let need = extent(6, 30.0, 8.0);
        assert_eq!(need, 6.0 * 30.0 + 5.0 * 8.0);
        assert!(fits(6, need, 30.0, 8.0, -1));
        assert!(!fits(6, need - 1.0, 30.0, 8.0, -1));
        assert!(!fits(0, 100.0, 30.0, 8.0, -1));
    }
}
