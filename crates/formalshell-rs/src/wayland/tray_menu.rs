//! The menu card: a tray item's menu, hung off the strip or off the tray's
//! own panel, or one menu of the focused Mac app, hung off its title; and
//! the `tray` and `appmenu` verbs that open, step and activate it.

use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_types::{Edge, IRect};
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::App;
use crate::paint::Kit;
use crate::services::{appmenu, tray};
use crate::surfaces::bar::cells::tray as tray_cell;
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::card_window::CardWindow;
use crate::surfaces::tray_menu::{Hit, Menu, Outcome};

/// Whose menu the card holds.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// A tray item, by its key.
    Tray(String),
    /// One top-level menu of the focused Mac app, by its dbusmenu id.
    App(i32),
    /// An app's menu in the dock, by the dock's id for it.
    Dock(String),
}

/// The line a menu card hangs off: its edge, how far in it lies, how long
/// it runs, the room past it for the card and the output's extent away
/// from it.
#[derive(Clone, Copy)]
pub(super) struct Hang {
    pub edge: Edge,
    pub line_at: f64,
    pub length: i32,
    pub cap: f64,
    pub room: i32,
}

/// The menu card.
pub struct Popout {
    pub win: CardWindow,
    /// The dbusmenu it draws (`traymenu`).
    pub(super) menu: Option<Menu>,
    pub(super) source: Source,
    /// The line it hangs off, end to end.
    length: i32,
}

impl Popout {
    /// Lays its content out at the card's content rect this frame.
    pub(super) fn layout(&mut self, kit: &mut Kit) {
        let card = &mut self.win.card;
        let (rect, alpha) = card.content;
        let clip = card.content_clip();
        if let Some(menu) = &mut self.menu {
            menu.layout(kit, &mut card.scene, rect, alpha, clip, self.win.surface.scale());
        }
    }

    pub(super) fn menu_hit(&self, x: f64, y: f64) -> Option<Hit> {
        self.menu.as_ref()?.hit(x.floor() as i32, y.floor() as i32)
    }
}

impl App {
    /// The input region over a popout's resting rect, or its whole surface
    /// while a menu is open, or nothing once it is on its way out.
    pub(super) fn set_popout_input(compositor: &CompositorState, p: &mut Popout) {
        // A menu takes input everywhere on its surface: a click off the
        // card dismisses it. One that is leaving would swallow the click
        // meant for what it covers.
        let size = p.win.card.scene.size;
        let r = match &p.menu {
            Some(_) if !p.win.card.is_open() => IRect::default(),
            Some(_) => IRect::new(0, 0, size.w, size.h),
            None => p.win.card.rest_rect(),
        };
        p.win.sync_region(compositor, r, &[]);
    }

    /// A tray item's menu, hung off `anchor` along the line (its icon's
    /// centre) or at the strip's end, and off the tray's panel when
    /// `over_panel`, which it then stands clear of.
    pub(super) fn open_tray_menu(&mut self, key: &str, anchor: Option<f64>, over_panel: bool) -> Result<(), String> {
        let Some(item) = self.store.tray.by_key(key).cloned() else { return Ok(()) };
        if !item.has_menu {
            return Err(format!("tray item '{}' has no menu", item.id));
        }
        self.open_menu_card(Source::Tray(item.key.clone()), item.key.clone(), item.tooltip.clone(), anchor, over_panel);
        let (key, largest) = (item.key, self.store.display.scales.largest());
        self.runtime.service(move |ctx| tray::open_menu(ctx, key, largest));
        Ok(())
    }

    /// The Mac app's top-level menu `id`, hung off `anchor` (its title's
    /// centre) or off where its title sits. The one already up shuts.
    pub fn open_app_menu(&mut self, id: i32, anchor: Option<f64>) -> Result<(), String> {
        let Some(node) = self.app_menu_node(id).cloned() else {
            return Err(format!("no menu with id {id}"));
        };
        if self.app_menu_open() == Some(id) {
            self.close_tray_menu();
            return Ok(());
        }
        let anchor = anchor.or_else(|| live!(self).bar.app_menu_anchor(id));
        let title = self.store.appmenu.titles().first().filter(|_| id == 0).map_or(node.label.clone(), |app| app.label.clone());
        self.open_menu_card(Source::App(id), format!("appmenu:{id}"), title, anchor, false);
        // The tree is already here; the app's answer to AboutToShow lands
        // on it as a change.
        if let Some(menu) = self.tray_menu.as_mut().and_then(|p| p.menu.as_mut()) {
            menu.set_tree(Some(&node));
        }
        self.resize_menu();
        appmenu::about_to_show(id);
        Ok(())
    }

    /// A top-level menu by its id, or the whole menu (0), which a vertical
    /// strip opens.
    fn app_menu_node(&self, id: i32) -> Option<&tray::MenuNode> {
        let menus = &self.store.appmenu;
        if id == 0 { menus.node(0) } else { menus.titles().iter().find(|t| t.id == id) }
    }

    /// The bar marks the title whose menu is up.
    fn mark_app_menu(&mut self) {
        let open = self.app_menu_open();
        if live!(self).bar.set_app_menu(open) {
            live!(self).dirty = true;
        }
    }

    /// The Mac menu the card holds while it is up.
    pub fn app_menu_open(&self) -> Option<i32> {
        match self.tray_menu.as_ref().filter(|p| p.win.card.is_open()).map(|p| &p.source) {
            Some(Source::App(id)) => Some(*id),
            _ => None,
        }
    }

    /// The rows of the Mac menu that is up: each one's dbusmenu id and its
    /// box in the card's surface.
    pub fn app_menu_rows(&self) -> Vec<(i32, IRect)> {
        self.app_menu_open().and(self.tray_menu.as_ref()).and_then(|p| p.menu.as_ref()).map(|m| m.row_boxes().collect()).unwrap_or_default()
    }

    /// The Mac menu title under a point of the open card's surface, which
    /// lies over the bar: both hang off the one edge of the output.
    fn app_menu_title_at(&self, x: f64, y: f64) -> Option<i32> {
        let p = self.tray_menu.as_ref()?;
        let bar = &live!(self).bar;
        let (card, strip) = (p.win.card.scene.size, bar.scene.size);
        let (x, y) = match bar.edge() {
            Edge::Bottom => (x, y - f64::from(card.h - strip.h)),
            Edge::Right => (x - f64::from(card.w - strip.w), y),
            Edge::Top | Edge::Left => (x, y),
        };
        bar.app_menu_at(x, y)
    }

    /// The pointer at a point of a Mac menu's surface that is off its
    /// card: over another title, that menu takes its place, as a pointer
    /// run along a Mac's menu bar does. True when it did.
    pub(super) fn app_menu_scrub(&mut self, x: f64, y: f64) -> bool {
        let Some(open) = self.app_menu_open() else { return false };
        match self.app_menu_title_at(x, y) {
            Some(id) if id != open => self.open_app_menu(id, None).is_ok(),
            _ => false,
        }
    }

    /// A press there: on its own title the menu shuts, on another that one
    /// opens, anywhere else it shuts.
    pub(super) fn app_menu_press_off_card(&mut self, x: f64, y: f64) {
        if !self.app_menu_scrub(x, y) {
            self.close_tray_menu();
        }
    }

    fn open_menu_card(&mut self, source: Source, key: String, title: String, anchor: Option<f64>, over_panel: bool) {
        let owner_edge = self.panel.as_ref().filter(|h| over_panel && h.is_open()).map(|h| h.win.card.rest().y1);
        let chrome = self.chrome();
        let theme = &self.store.theme.theme;
        let edge = chrome.edge;
        let line_at = owner_edge.unwrap_or(chrome.line());
        let (output_width, output_height) = self.output_size();
        let pad = theme.space.screen_padding;
        let cap = if edge.is_vertical() { output_height - pad * 2.0 } else { output_height - line_at - theme.space.bar_margin - pad };
        let length = live!(self).bar.length();
        let room = (if edge.is_vertical() { output_width } else { output_height }) as i32;
        let hang = Hang { edge, line_at, length, cap, room };
        self.open_menu_card_on(source, key, title, anchor.unwrap_or(length as f64), hang, owner_edge.is_none());
    }

    /// The menu card off `hang`, centred on `anchor` along it. The panels
    /// shut first unless the card hangs off one.
    pub(super) fn open_menu_card_on(&mut self, source: Source, key: String, title: String, anchor: f64, hang: Hang, shut_panels: bool) {
        let now = Instant::now();
        self.drop_tray_menu();
        self.tips.hide(None, now);
        if shut_panels {
            self.close_panels();
        }
        let theme = &self.store.theme.theme;
        let Hang { edge, line_at, length, cap, room } = hang;
        let vertical = edge.is_vertical();
        let pad = theme.space.screen_padding;
        let mut menu = Menu::new(theme, key, title);
        menu.cap = cap.max(menu.width());
        let width = menu.width();
        let oriented = |h: f64| if vertical { (h, width) } else { (width, h) };
        menu.anchor = anchor;
        let size = Sizing::Card { size: oriented(menu.cap), anchor, length, room };
        let mut card = Card::new(theme, CardSpec { role: fs_theme::style::Role::Menu, ..CardSpec::new(edge, line_at, size) }, self.cast);
        card.resize(now, anchor, length, pad, oriented(menu.height().min(menu.cap)));
        menu.morph.jump(menu.height().min(menu.cap));
        let surface = self.popout_surface(&card, Layer::Overlay);
        let mut popout = Popout { win: CardWindow::new(card, surface), menu: Some(menu), source, length };
        Self::set_popout_input(&self.compositor, &mut popout);
        self.tray_menu = Some(popout);
        self.mark_app_menu();
        live!(self).dirty = true;
        crate::trace::mapped(crate::trace::Surface::TrayMenu, crate::trace::State::Mapped);
    }

    /// Closes the menu through its exit, telling its owner.
    pub fn close_tray_menu(&mut self) {
        let now = Instant::now();
        let Some(p) = self.tray_menu.as_mut().filter(|p| p.win.card.is_open()) else { return };
        p.win.card.set_open(now, false);
        Self::set_popout_input(&self.compositor, p);
        match p.source.clone() {
            Source::Tray(key) => self.runtime.service(move |ctx| tray::close_menu(ctx, key)),
            Source::App(id) => appmenu::closed(id),
            Source::Dock(_) => self.dock_menu_closed(now),
        }
        self.mark_app_menu();
        live!(self).dirty = true;
    }

    /// Takes the menu off screen at once, telling the item if it was up.
    pub(super) fn drop_tray_menu(&mut self) {
        self.close_tray_menu();
        self.tray_menu = None;
    }

    /// The menu's card at the height its rows ask for, capped to the room.
    pub(super) fn resize_menu(&mut self) {
        let now = Instant::now();
        let pad = self.store.theme.theme.space.screen_padding;
        if let Some(p) = &mut self.tray_menu {
            let (vertical, length) = (p.win.card.edge.is_vertical(), p.length);
            let mapped = p.win.card.is_open() && p.win.surface.mapped();
            if let Some(m) = &mut p.menu {
                let (w, h) = (m.width(), m.height().min(m.cap));
                // The first size lands at once; every later one morphs.
                if mapped && m.morph.target() > 0.0 {
                    let ms = crate::motion::Kind::Spatial.ms(&self.store.theme.theme);
                    m.morph.set(now, h, ms);
                } else {
                    m.morph.jump(h);
                }
                let hv = m.morph.value(now);
                let anchor = m.anchor;
                p.win.card.resize(now, anchor, length, pad, if vertical { (hv, w) } else { (w, hv) });
            }
            Self::set_popout_input(&self.compositor, p);
        }
    }

    /// The menu's card on its way to the height its rows ask for, a frame at a time.
    pub(super) fn step_menu(&mut self, now: Instant) {
        let pad = self.store.theme.theme.space.screen_padding;
        let Some(p) = &mut self.tray_menu else { return };
        let (vertical, length) = (p.win.card.edge.is_vertical(), p.length);
        let Some(m) = &p.menu else { return };
        if !m.morph.running(now) {
            return;
        }
        let (w, h) = (m.width(), m.morph.value(now));
        p.win.card.resize(now, m.anchor, length, pad, if vertical { (h, w) } else { (w, h) });
    }

    /// The tray changed: a menu waiting on its tree gets it, one whose item
    /// went away shuts.
    pub(super) fn tray_changed(&mut self) {
        let Some(p) = &mut self.tray_menu else { return };
        if !matches!(p.source, Source::Tray(_)) {
            return;
        }
        let Some(menu) = &mut p.menu else { return };
        if self.store.tray.by_key(&menu.key).is_none() {
            self.close_tray_menu();
            return;
        }
        if let Some(tree) = self.store.tray.menu.as_ref().filter(|m| m.key == menu.key) {
            menu.set_tree(Some(&tree.root));
        }
        self.resize_menu();
    }

    /// The Mac app's menu changed, or its window lost focus: the open card
    /// takes the new rows, and shuts when its menu is gone.
    pub(super) fn appmenu_changed(&mut self) {
        let Some(Source::App(id)) = self.tray_menu.as_ref().map(|p| p.source.clone()) else { return };
        let node = self.app_menu_node(id).cloned();
        match (node, self.tray_menu.as_mut().and_then(|p| p.menu.as_mut())) {
            (Some(node), Some(menu)) => menu.set_tree(Some(&node)),
            _ => return self.close_tray_menu(),
        }
        self.resize_menu();
    }

    pub(super) fn menu_outcome(&mut self, outcome: Outcome) {
        let Some(source) = self.tray_menu.as_ref().map(|p| p.source.clone()) else { return };
        match (outcome, source) {
            (Outcome::None, _) => {}
            (Outcome::Click(id), Source::Tray(key)) => {
                self.runtime.service(move |ctx| tray::menu_click(ctx, key, id));
                self.close_tray_menu();
            }
            (Outcome::Click(id), Source::App(_)) => {
                appmenu::click(id);
                self.close_tray_menu();
            }
            (Outcome::Click(id), Source::Dock(_)) => {
                self.dock_menu_click(id);
                self.close_tray_menu();
            }
            (Outcome::Toggled(id, opened), source) => {
                match source {
                    Source::Tray(key) if opened => {
                        let largest = self.store.display.scales.largest();
                        self.runtime.service(move |ctx| tray::menu_about_to_show(ctx, key, id, largest));
                    }
                    Source::App(_) if opened => appmenu::about_to_show(id),
                    _ => {}
                }
                self.resize_menu();
            }
        }
    }

    pub(super) fn open_menu_mut(&mut self) -> Option<&mut Menu> {
        self.tray_menu.as_mut().filter(|p| p.win.card.is_open()).and_then(|p| p.menu.as_mut())
    }

    /// `tray activate`: the item's own Activate, as its cell's click does.
    pub fn tray_activate(&self, id: &str) -> Result<(), String> {
        let Some(item) = self.store.tray.by_id(id) else { return Err(format!("no tray item with id '{id}'")) };
        let key = item.key.clone();
        self.runtime.service(move |ctx| tray::activate(ctx, key));
        Ok(())
    }

    /// `tray menu`: opens over the second bar while that is up, as a right
    /// click on its cell does.
    pub fn tray_menu(&mut self, id: &str) -> Result<(), String> {
        let Some(key) = self.store.tray.by_id(id).map(|i| i.key.clone()) else {
            return Err(format!("no tray item with id '{id}'"));
        };
        let over = self.shows(PanelId::TrayOverflow);
        self.open_tray_menu(&key, None, over)
    }

    pub fn tray_menu_cursor(&mut self, delta: i32) -> Result<(), String> {
        let m = self.open_menu_mut().ok_or("no tray menu open")?;
        m.move_cursor(delta);
        Ok(())
    }

    pub fn tray_menu_activate(&mut self) -> Result<(), String> {
        let m = self.open_menu_mut().ok_or("no tray menu open")?;
        let outcome = m.activate_cursor();
        self.menu_outcome(outcome);
        Ok(())
    }

    /// `tray status`'s `overflow` block: whether the second bar is up and how
    /// many icons the strip kept.
    pub fn tray_inline(&self) -> usize {
        tray_cell::inline_count().min(self.store.tray.items.len())
    }
}
