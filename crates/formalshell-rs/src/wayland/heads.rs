//! What every output carries: its own wallpaper, and a bar of its own.
//!
//! One of those bars is live: it is `App::bar`, the one panels, second bars,
//! joins and tooltips hang off, and every other bar is a spare drawn from the
//! same store at its own output's size. The live bar follows the focused
//! output, swapping with that output's spare, but only while nothing hangs
//! off it, so a card is never left joined to a line that moved away. A pointer
//! entering a spare promotes it on the spot, so the first click lands without
//! waiting for the compositor's focus event.
//!
//! While something on the live bar's output closes on a click outside it
//! (a panel, a tray menu, the launcher, the atlas, a plugin's overlay), every
//! other output carries a catcher: a clear full-output surface whose press
//! closes it, as a press on the panel's own window off its card does. The
//! catcher leaves that output's bar strip out of its input, so a press there
//! still reaches its cell, once the live bar's cards are gone.

use std::time::Instant;

use fs_chrome::types::Edge;
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::store::Topic;
use crate::scene::IRect;
use crate::surface::{Backdrop, PixelSurface, Surface};
use crate::surfaces::bar::Bar;

pub(super) struct Head {
    pub output: WlOutput,
    pub backdrop: Option<Backdrop>,
    /// None on the live bar's output, whose bar is `App::bar`.
    pub spare: Option<Spare>,
    pub catcher: Option<PixelSurface>,
}

pub(super) struct Spare {
    pub bar: Bar,
    pub surface: Option<Surface>,
    pub zones: Vec<(Edge, PixelSurface)>,
    pub dirty: bool,
}

/// Which part of a head a surface is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
    Backdrop,
    Bar,
    Zone(usize),
    Catcher,
}

impl App {
    pub(super) fn output_name(&self, output: &WlOutput) -> String {
        self.outputs.info(output).and_then(|i| i.name).unwrap_or_default()
    }

    /// A head for every output the registry holds, the live bar placed on
    /// the first when it has none yet.
    pub(super) fn sync_heads(&mut self) {
        let outputs: Vec<WlOutput> = self.outputs.outputs().collect();
        for output in outputs {
            if self.heads.iter().any(|h| h.output == output) {
                continue;
            }
            let layer = self.overlay_on("formalshell:wallpaper", Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
            let backdrop = Some(Backdrop::new(layer, &self.shm, &output));
            let spare = if self.bar_wl.is_none() {
                self.bar_wl = Some(output.clone());
                None
            } else {
                Some(Spare { bar: self.spare_bar(), surface: None, zones: Vec::new(), dirty: true })
            };
            self.heads.push(Head { output, backdrop, spare, catcher: None });
            if self.heads.last().is_some_and(|h| h.spare.is_some()) {
                self.place_spare(self.heads.len() - 1);
            } else {
                self.place_chrome();
            }
        }
        let names: Vec<(usize, String)> = self.heads.iter().enumerate().map(|(i, h)| (i, self.output_name(&h.output))).collect();
        for (i, name) in names {
            if let Some(s) = &mut self.heads[i].spare
                && s.bar.output != name
            {
                s.bar.output = name;
                s.bar.read(&self.store, None, Instant::now());
                s.dirty = true;
            }
        }
    }

    /// A bar built the way the live one is: its edge, its layout and theme.
    fn spare_bar(&self) -> Bar {
        let mut bar = Bar::new(self.bar.edge(), &self.store.theme.theme);
        bar.set_layout(self.bar.resolved.clone());
        bar.update_paint(&self.store);
        bar
    }

    /// The output gone: its head with it, and the live bar handed to another
    /// output's spare when it was the live one's.
    pub(super) fn drop_head(&mut self, output: &WlOutput) {
        let Some(i) = self.heads.iter().position(|h| h.output == *output) else { return };
        let head = self.heads.remove(i);
        if self.bar_wl.as_ref() != Some(output) {
            return;
        }
        self.close_bar_cards();
        self.bar_surface = None;
        self.zones.clear();
        self.bar_wl = None;
        drop(head);
        if let Some(j) = self.heads.iter().position(|h| h.spare.is_some()) {
            let spare = self.heads[j].spare.take().expect("checked");
            self.bar = spare.bar;
            self.bar_surface = spare.surface;
            self.zones = spare.zones;
            self.bar_wl = Some(self.heads[j].output.clone());
            if self.bar_surface.is_none() && !self.bar.hidden {
                self.place_chrome();
            }
        }
        self.refresh_bar(None);
    }

    /// Everything hanging off the live bar, shut at once.
    fn close_bar_cards(&mut self) {
        self.overflow = None;
        self.panel = None;
        self.drop_tray_menu();
        self.outgoing = None;
        self.tip_card = None;
    }

    /// The spare on head `i` made the live bar, when nothing hangs off the
    /// live one.
    pub(super) fn promote(&mut self, i: usize) -> bool {
        if self.heads.get(i).is_none_or(|h| h.spare.is_none()) || self.bar_busy() {
            return false;
        }
        let mut spare = self.heads[i].spare.take().expect("checked");
        std::mem::swap(&mut self.bar, &mut spare.bar);
        std::mem::swap(&mut self.bar_surface, &mut spare.surface);
        std::mem::swap(&mut self.zones, &mut spare.zones);
        let old = self.bar_wl.replace(self.heads[i].output.clone());
        self.tip_card = None;
        self.bar.hover = None;
        spare.bar.hover = None;
        spare.dirty = true;
        match old.and_then(|o| self.heads.iter_mut().find(|h| h.output == o)) {
            Some(h) => h.spare = Some(spare),
            None => drop(spare),
        }
        self.log(&format!("bar live on {}", self.bar.output));
        self.refresh_bar(None);
        true
    }

    /// The live bar onto the focused output, when it is elsewhere and free.
    pub(super) fn follow_focus(&mut self) {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        if focused.is_empty() || *focused == self.bar.output {
            return;
        }
        let Some(i) = self.heads.iter().position(|h| h.spare.as_ref().is_some_and(|s| s.bar.output == *focused)) else { return };
        self.promote(i);
    }

    /// Something on the live bar's output that a click outside closes.
    pub(super) fn catches(&self) -> bool {
        self.panel.as_ref().is_some_and(|h| h.is_open() && h.module.takes_keyboard())
            || self.menu.as_ref().is_some_and(|p| p.card.is_open())
            || self.overflow_open_any()
            || self.launcher.open
            || self.atlas_open()
            || self.overlay_open().is_some()
    }

    /// A click outside, landed on another output.
    pub(super) fn dismiss(&mut self) {
        if self.launcher.open {
            self.menu_close();
        }
        self.atlas_close();
        self.overlay_close();
        self.close_panels();
        self.close_overflow(Instant::now());
    }

    /// A press on head `i`'s spare bar: whatever a click outside closes is
    /// closed, the live bar's cards go at once rather than over their close,
    /// and the spare is promoted so the press lands on its cell. A modal
    /// still joined to the old line while it closes keeps the press.
    pub(super) fn press_spare(&mut self, i: usize) -> bool {
        if self.heads.get(i).is_none_or(|h| h.spare.is_none()) {
            return false;
        }
        if self.catches() {
            self.dismiss();
        }
        if self.panel.is_some() || self.outgoing.is_some() || self.overflow.is_some() || self.menu.is_some() {
            self.close_bar_cards();
            self.panel_dirty = true;
            self.sync_join();
            self.sync_open(Instant::now());
        }
        self.promote(i)
    }

    /// A catcher on every output but the live bar's while `catches`, none
    /// otherwise.
    pub(super) fn sync_catchers(&mut self) {
        let want = self.catches();
        for i in 0..self.heads.len() {
            let live = self.bar_wl.as_ref() == Some(&self.heads[i].output);
            if want && !live {
                if self.heads[i].catcher.is_none() {
                    let output = self.heads[i].output.clone();
                    let layer = self.overlay_on("formalshell:catcher", Layer::Overlay, Anchor::all(), (0, 0), -1, Some(&output));
                    self.heads[i].catcher = Some(PixelSurface::new("catcher", layer, 0.0, &self.pixels, &self.qh, self.started));
                    self.log(&format!("catcher on {}", self.output_name(&output)));
                }
            } else if self.heads[i].catcher.take().is_some() {
                self.log(&format!("catcher off {}", self.output_name(&self.heads[i].output)));
            }
        }
    }

    /// Head `i`'s catcher taking input over its whole output but the strip
    /// its spare bar draws on.
    fn set_catcher_input(&self, i: usize) {
        let head = &self.heads[i];
        let Some((w, h)) = head.catcher.as_ref().and_then(|c| c.size()) else { return };
        let Ok(region) = Region::new(&self.compositor) else { return };
        region.add(0, 0, w, h);
        if let Some(s) = head.spare.as_ref().filter(|s| s.surface.is_some()) {
            let t = s.bar.thickness();
            let band = match s.bar.edge() {
                Edge::Top => IRect::new(0, 0, w, t),
                Edge::Bottom => IRect::new(0, h - t, w, t),
                Edge::Left => IRect::new(0, 0, t, h),
                Edge::Right => IRect::new(w - t, 0, t, h),
            };
            region.subtract(band.x, band.y, band.w, band.h);
        }
        if let Some(c) = &head.catcher {
            c.layer.set_input_region(Some(region.wl_region()));
        }
    }

    /// A card, a second bar or a join hangs off the live bar.
    fn bar_busy(&self) -> bool {
        self.panel.is_some() || self.outgoing.is_some() || self.overflow.is_some() || self.menu.is_some() || !self.joins().is_empty()
    }

    pub(super) fn head_part(&self, surface: &WlSurface) -> Option<(usize, Part)> {
        self.heads.iter().enumerate().find_map(|(i, h)| {
            if h.backdrop.as_ref().is_some_and(|b| b.layer.wl_surface() == surface) {
                return Some((i, Part::Backdrop));
            }
            if h.catcher.as_ref().is_some_and(|c| c.layer.wl_surface() == surface) {
                return Some((i, Part::Catcher));
            }
            let s = h.spare.as_ref()?;
            if s.surface.as_ref().is_some_and(|b| b.layer.wl_surface() == surface) {
                return Some((i, Part::Bar));
            }
            s.zones.iter().position(|(_, z)| z.layer.wl_surface() == surface).map(|z| (i, Part::Zone(z)))
        })
    }

    /// Head `i`'s spare window and zones as its output's fullscreen state
    /// wants them.
    pub(super) fn place_spare(&mut self, i: usize) {
        let Some(mut spare) = self.heads.get_mut(i).and_then(|h| h.spare.take()) else { return };
        let output = self.heads[i].output.clone();
        spare.bar.hidden = self.hidden_on(&spare.bar.output);
        (spare.surface, spare.zones) = if spare.bar.hidden {
            (None, Vec::new())
        } else {
            let (surface, zones) = self.build_chrome(&spare.bar, &output);
            (Some(surface), zones)
        };
        spare.dirty = true;
        self.heads[i].spare = Some(spare);
    }

    pub(super) fn place_spares(&mut self) {
        for i in 0..self.heads.len() {
            self.place_spare(i);
        }
    }

    /// The spares' share of `refresh_bar`.
    pub(super) fn refresh_spares(&mut self, topic: Option<Topic>, now: Instant) {
        for i in 0..self.heads.len() {
            let Some(s) = &self.heads[i].spare else { continue };
            if self.hidden_on(&s.bar.output) != s.bar.hidden {
                self.place_spare(i);
            }
            let Some(s) = &mut self.heads[i].spare else { continue };
            s.bar.update_paint(&self.store);
            s.bar.read(&self.store, topic, now);
            s.dirty = true;
        }
    }

    pub(super) fn tick_spares(&mut self, now: Instant) {
        for s in self.heads.iter_mut().filter_map(|h| h.spare.as_mut()) {
            s.bar.tick(&self.store, now);
            s.dirty = true;
        }
    }

    pub(super) fn spares_wake(&self, now: Instant) -> Option<Instant> {
        self.heads.iter().filter_map(|h| h.spare.as_ref()).filter_map(|s| s.bar.wake(now)).min()
    }

    pub(super) fn present_heads(&mut self, all: bool, now: Instant) {
        self.sync_catchers();
        let qh = self.qh.clone();
        for c in self.heads.iter_mut().filter_map(|h| h.catcher.as_mut()) {
            c.present(0.0, false, &qh);
        }
        for s in self.heads.iter_mut().filter_map(|h| h.spare.as_mut()) {
            let Some(surface) = &mut s.surface else { continue };
            let animating = s.bar.animating(now);
            if all || s.dirty || (animating && !s.bar.step_in_place(now)) {
                s.bar.layout(&self.store, now);
                s.dirty = false;
            }
            surface.present(&mut s.bar.scene, animating, &qh);
            for (_, z) in &mut s.zones {
                z.present(0.0, false, &qh);
            }
        }
    }

    /// The wallpaper for each output's size, asked for again whenever either
    /// moves, and drawn once it is ready.
    pub(super) fn update_backdrops(&mut self) {
        let theme = &self.store.theme.theme;
        let dither = theme.wallpaper_dither.then(|| self.store.config.f64("wallpaper.ditherColors").unwrap_or(6.0).max(1.0) as usize);
        let background = theme.colors.get("background");
        let wanted = &self.store.state.data.wallpaper;
        let reveal = theme.motion().families.reveal * self.motion_scale;
        for b in self.heads.iter_mut().filter_map(|h| h.backdrop.as_mut()) {
            let Some((w, h)) = b.pixels() else { continue };
            crate::services::wallpaper::show(wanted, w as u32, h as u32, dither);
            let picture = self.store.wallpaper.sized(w as u32, h as u32).cloned();
            if picture.is_none() && b.holds(wanted) {
                continue;
            }
            b.draw(picture.filter(|p| p.path == *wanted && p.dither == dither), background, reveal, &self.qh);
        }
    }

    pub(super) fn head_frame(&mut self, i: usize, part: Part, now: Instant) {
        let Some(head) = self.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => {
                if let Some(b) = &mut head.backdrop {
                    b.step(now, &self.qh);
                }
            }
            Part::Bar => {
                let Some(s) = &mut head.spare else { return };
                let Some(surface) = &mut s.surface else { return };
                surface.landed(now);
                if !surface.mapped {
                    surface.mapped = true;
                    s.dirty = true;
                }
            }
            Part::Zone(z) => {
                let Some((_, z)) = head.spare.as_mut().and_then(|s| s.zones.get_mut(z)) else { return };
                (z.frame_pending, z.mapped, z.callbacks) = (false, true, z.callbacks + 1);
            }
            Part::Catcher => {
                let Some(c) = &mut head.catcher else { return };
                (c.frame_pending, c.mapped, c.callbacks) = (false, true, c.callbacks + 1);
            }
        }
    }

    pub(super) fn head_configure(&mut self, i: usize, part: Part, width: i32, height: i32) {
        let Some(head) = self.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => {
                if let Some(b) = &mut head.backdrop {
                    b.configure(width, height);
                }
                self.update_backdrops();
            }
            Part::Bar => {
                let Some(s) = &mut head.spare else { return };
                let current = s.bar.scene.size;
                let w = if width > 0 { width } else { current.w };
                let h = if height > 0 { height } else { current.h };
                s.bar.resize(w, h);
                let size = s.bar.scene.size;
                if let Some(surface) = &mut s.surface {
                    surface.configure(size.w, size.h);
                }
                s.bar.read(&self.store, None, Instant::now());
                s.dirty = true;
                if let Some(s) = &self.heads[i].spare
                    && let Some(surface) = &s.surface
                {
                    self.set_input_region_for(&s.bar, surface);
                }
            }
            Part::Zone(z) => {
                if let Some((_, z)) = head.spare.as_mut().and_then(|s| s.zones.get_mut(z)) {
                    z.configure(width.max(1), height.max(1));
                }
            }
            Part::Catcher => {
                if let Some(c) = &mut head.catcher {
                    c.configure(width.max(1), height.max(1));
                }
                self.set_catcher_input(i);
            }
        }
    }

    pub(super) fn head_closed(&mut self, i: usize, part: Part) {
        let Some(head) = self.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => head.backdrop = None,
            Part::Bar => {
                if let Some(s) = &mut head.spare {
                    s.surface = None;
                }
            }
            Part::Zone(_) => {}
            Part::Catcher => head.catcher = None,
        }
    }
}
