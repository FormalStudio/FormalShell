//! A tray item's menu, hung off the strip or off the tray's own panel,
//! and the `tray` verbs that open, step and activate it.

use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_types::IRect;
use smithay_client_toolkit::compositor::CompositorState;
use smithay_client_toolkit::shell::wlr_layer::Layer;

use super::App;
use crate::paint::Kit;
use crate::services::tray;
use crate::surfaces::bar::cells::tray as tray_cell;
use crate::surfaces::card::{Card, CardSpec, Sizing};
use crate::surfaces::card_window::CardWindow;
use crate::surfaces::tray_menu::{Hit, Menu, Outcome};

/// A tray item's menu, hung off the strip or off the tray's own panel.
pub struct Popout {
    pub win: CardWindow,
    /// The item's dbusmenu (`traymenu`).
    pub(super) menu: Option<Menu>,
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
    /// while a menu is open.
    pub(super) fn set_popout_input(compositor: &CompositorState, p: &mut Popout) {
        // A menu takes input everywhere on its surface: a click off the
        // card dismisses it.
        let size = p.win.card.scene.size;
        let r = if p.menu.is_some() { IRect::new(0, 0, size.w, size.h) } else { p.win.card.rest_rect() };
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
        let now = Instant::now();
        self.drop_tray_menu();
        self.tips.hide(None, now);
        let owner_edge = self.panel.as_ref().filter(|h| over_panel && h.is_open()).map(|h| h.win.card.rest().y1);
        if owner_edge.is_none() {
            self.close_panels();
        }
        let chrome = self.chrome();
        let theme = &self.store.theme.theme;
        let edge = chrome.edge;
        let vertical = edge.is_vertical();
        let line_at = owner_edge.unwrap_or(chrome.line());
        let (_, output_height) = self.output_size();
        let pad = theme.space.screen_padding;
        let cap = if vertical { output_height - pad * 2.0 } else { output_height - line_at - theme.space.bar_margin - pad };
        let mut menu = Menu::new(theme, item.key.clone(), item.tooltip.clone());
        menu.cap = cap.max(menu.width());
        let width = menu.width();
        let oriented = |h: f64| if vertical { (h, width) } else { (width, h) };
        let anchor = anchor.unwrap_or(live!(self).bar.length() as f64);
        menu.anchor = anchor;
        let size = Sizing::Card { size: oriented(menu.cap), anchor, length: live!(self).bar.length() };
        let mut card = Card::new(theme, CardSpec { role: fs_theme::style::Role::Menu, ..CardSpec::new(edge, line_at, size) }, self.cast);
        card.resize(now, anchor, live!(self).bar.length(), pad, oriented(menu.height().min(menu.cap)));
        menu.morph.jump(menu.height().min(menu.cap));
        let surface = self.popout_surface(&card, Layer::Overlay);
        let mut popout = Popout { win: CardWindow::new(card, surface), menu: Some(menu) };
        Self::set_popout_input(&self.compositor, &mut popout);
        self.tray_menu = Some(popout);
        let (key, largest) = (item.key, self.store.display.scales.largest());
        self.runtime.service(move |ctx| tray::open_menu(ctx, key, largest));
        crate::trace::mapped(crate::trace::Surface::TrayMenu, crate::trace::State::Mapped);
        Ok(())
    }

    /// Closes the menu through its exit, telling the item.
    pub fn close_tray_menu(&mut self) {
        let now = Instant::now();
        let Some(p) = self.tray_menu.as_mut().filter(|p| p.win.card.is_open()) else { return };
        p.win.card.set_open(now, false);
        let key = p.menu.as_ref().map(|m| m.key.clone());
        if let Some(key) = key {
            self.runtime.service(move |ctx| tray::close_menu(ctx, key));
        }
    }

    /// Takes the menu off screen at once, telling the item if it was up.
    pub(super) fn drop_tray_menu(&mut self) {
        self.close_tray_menu();
        self.tray_menu = None;
    }

    /// The menu's card at the height its rows ask for, capped to the room.
    pub(super) fn resize_menu(&mut self) {
        let now = Instant::now();
        let vertical = live!(self).bar.edge().is_vertical();
        let length = live!(self).bar.length();
        let pad = self.store.theme.theme.space.screen_padding;
        if let Some(p) = &mut self.tray_menu {
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
        let vertical = live!(self).bar.edge().is_vertical();
        let length = live!(self).bar.length();
        let pad = self.store.theme.theme.space.screen_padding;
        let Some(p) = &mut self.tray_menu else { return };
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

    pub(super) fn menu_outcome(&mut self, outcome: Outcome) {
        let Some(key) = self.tray_menu.as_ref().and_then(|p| p.menu.as_ref()).map(|m| m.key.clone()) else { return };
        match outcome {
            Outcome::None => {}
            Outcome::Click(id) => {
                self.runtime.service(move |ctx| tray::menu_click(ctx, key, id));
                self.close_tray_menu();
            }
            Outcome::Toggled(id, opened) => {
                if opened {
                    let largest = self.store.display.scales.largest();
                    self.runtime.service(move |ctx| tray::menu_about_to_show(ctx, key, id, largest));
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
