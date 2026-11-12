//! What every output carries: its own wallpaper, and a bar of its own.
//!
//! One of those bars is live: it is `Bars::live`, the one panels, menus,
//! joins and tooltips hang off, and every other bar is a spare drawn from the
//! same store at its own output's size. The live bar follows the focused
//! output, moving to that output's window, but only while nothing hangs
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

use fs_theme::palette::ColorRole;
use std::time::Instant;

use fs_types::{Edge, IRect};
use smithay_client_toolkit::compositor::Region;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use super::backdrop::{Backdrop, Shards};
use crate::store::Topic;
use crate::surface::{PixelSurface, Surface};
use crate::surfaces::bar::Bar;

pub(super) struct Head {
    pub output: WlOutput,
    pub bar: BarWindow,
    pub backdrop: Option<Backdrop>,
    pub catcher: Option<PixelSurface>,
    pub clock: Option<super::desktop_clock::Clock>,
}

/// A bar with the layer surface and frame zones it is drawn on.
pub struct BarWindow {
    pub bar: Bar,
    pub surface: Option<Surface>,
    pub zones: Vec<(Edge, PixelSurface)>,
    pub dirty: bool,
}

impl BarWindow {
    fn new(bar: Bar) -> Self {
        Self { bar, surface: None, zones: Vec::new(), dirty: true }
    }

    /// A freshly built window in place: the reserved zone and the input
    /// region start over, as the compositor has seen neither.
    pub fn mount(&mut self, surface: Surface, zones: Vec<(Edge, PixelSurface)>) {
        self.bar.reserved = self.bar.zone();
        self.bar.input_sent = None;
        self.surface = Some(surface);
        self.zones = zones;
        self.dirty = true;
    }

    /// The window gone, its bar kept.
    fn unmount(&mut self) {
        self.surface = None;
        self.zones.clear();
        self.dirty = true;
    }
}

/// Every head with its bar window, and which of them is live. Before the
/// first output, and after the last is gone, the one window has no head
/// and waits in `lone`.
pub struct Bars {
    pub(super) heads: Vec<Head>,
    lone: Option<BarWindow>,
    pub(super) live: usize,
}

/// The key `bars[Live]` reads the live window by.
pub struct Live;

impl std::ops::Index<Live> for Bars {
    type Output = BarWindow;

    fn index(&self, _: Live) -> &BarWindow {
        match self.heads.get(self.live) {
            Some(h) => &h.bar,
            None => self.lone.as_ref().expect("a headless bar window"),
        }
    }
}

impl std::ops::IndexMut<Live> for Bars {
    fn index_mut(&mut self, _: Live) -> &mut BarWindow {
        match self.heads.get_mut(self.live) {
            Some(h) => &mut h.bar,
            None => self.lone.as_mut().expect("a headless bar window"),
        }
    }
}

impl Bars {
    pub fn new(bar: Bar) -> Self {
        Self { heads: Vec::new(), lone: Some(BarWindow::new(bar)), live: 0 }
    }

    pub(super) fn wins_mut(&mut self) -> impl Iterator<Item = &mut BarWindow> {
        self.heads.iter_mut().map(|h| &mut h.bar).chain(self.lone.as_mut())
    }

    /// `f` on every bar, whether any answered true.
    pub fn apply(&mut self, mut f: impl FnMut(&mut Bar) -> bool) -> bool {
        self.wins_mut().fold(false, |any, w| f(&mut w.bar) | any)
    }

    pub fn each(&mut self, mut f: impl FnMut(&mut Bar)) {
        self.wins_mut().for_each(|w| f(&mut w.bar));
    }

    /// Head `i`'s window when it is a spare.
    fn spare(&self, i: usize) -> Option<&BarWindow> {
        self.heads.get(i).filter(|_| i != self.live).map(|h| &h.bar)
    }

    fn spare_mut(&mut self, i: usize) -> Option<&mut BarWindow> {
        let live = self.live;
        self.heads.get_mut(i).filter(|_| i != live).map(|h| &mut h.bar)
    }

    /// Every window but the live one, with its head's index.
    fn spares(&self) -> impl Iterator<Item = (usize, &BarWindow)> {
        let live = self.live;
        self.heads.iter().enumerate().filter(move |(i, _)| *i != live).map(|(i, h)| (i, &h.bar))
    }

    fn spares_mut(&mut self) -> impl Iterator<Item = (usize, &mut BarWindow)> {
        let live = self.live;
        self.heads.iter_mut().enumerate().filter(move |(i, _)| *i != live).map(|(i, h)| (i, &mut h.bar))
    }
}

/// Which part of a head a surface is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Part {
    Backdrop,
    /// One of the sculpture's two layers: 0 the shards, 1 the grid.
    Shards(usize),
    Bar,
    Zone(usize),
    Catcher,
    Clock,
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
            if self.bars.heads.iter().any(|h| h.output == output) {
                continue;
            }
            self.veil_output(&output);
            let layer = self.overlay_on("formalshell:wallpaper", Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
            let backdrop = Some(Backdrop::new(layer, &self.shm, self.store.display.scales.on(&output)));
            let lone = self.bars.lone.take();
            let first = lone.is_some();
            let bar = lone.unwrap_or_else(|| BarWindow::new(self.spare_bar()));
            self.bars.heads.push(Head { output, bar, backdrop, catcher: None, clock: None });
            if first {
                self.place_chrome();
            } else {
                self.place_spare(self.bars.heads.len() - 1);
            }
        }
        for i in 0..self.bars.heads.len() {
            let name = self.output_name(&self.bars.heads[i].output);
            if let Some(s) = self.bars.spare_mut(i)
                && s.bar.output != name
            {
                s.bar.output = name;
                s.bar.read(&mut self.kit, &self.store, None, Instant::now());
                s.dirty = true;
            }
        }
    }

    /// A bar built the way the live one is: its edge, its layout and theme.
    fn spare_bar(&mut self) -> Bar {
        let mut bar = Bar::new(live!(self).bar.edge(), &self.store.theme.theme, crate::settings::layout(&self.store.config));
        bar.set_layout(live!(self).bar.resolved.clone());
        bar.set_collapsed(self.store.state.data.sidebar_collapsed);
        bar.update_paint(&mut self.kit, &self.store);
        bar
    }

    /// The output gone: its head with it, and the live bar handed to another
    /// output's spare when it was the live one's.
    pub(super) fn drop_head(&mut self, output: &WlOutput) {
        let Some(i) = self.bars.heads.iter().position(|h| h.output == *output) else { return };
        let was_live = i == self.bars.live;
        let Head { bar, .. } = self.bars.heads.remove(i);
        if self.bars.heads.is_empty() {
            // The one window outlives its output, unmapped.
            self.bars.lone = Some(bar);
            self.bars.live = 0;
            self.close_bar_cards();
            live!(self).unmount();
        } else {
            drop(bar);
            if !was_live {
                if i < self.bars.live {
                    self.bars.live -= 1;
                }
                return;
            }
            self.close_bar_cards();
            self.bars.live = 0;
            if live!(self).surface.is_none() && !live!(self).bar.hidden() {
                self.place_chrome();
            }
        }
        self.refresh_bar(None);
    }

    /// Everything hanging off the live bar, shut at once.
    pub(super) fn close_bar_cards(&mut self) {
        self.panel = None;
        self.drop_tray_menu();
        self.outgoing = None;
        self.tip_card = None;
    }

    /// The spare on head `i` made the live bar, when nothing hangs off the
    /// live one.
    pub(super) fn promote(&mut self, i: usize) -> bool {
        if i >= self.bars.heads.len() || i == self.bars.live || self.bar_busy() {
            return false;
        }
        if let Some(sb) = live!(self).bar.sidebar_mut() {
            sb.map.cursor = None;
        }
        self.sync_sidebar_keys();
        live!(self).bar.hover = None;
        live!(self).dirty = true;
        self.bars.live = i;
        live!(self).bar.hover = None;
        self.tip_card = None;
        crate::trace::note(format_args!("bar live on {}", live!(self).bar.output));
        self.refresh_bar(None);
        true
    }

    /// The live bar onto the focused output, when it is elsewhere and free.
    pub(super) fn follow_focus(&mut self) {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        if focused.is_empty() || *focused == live!(self).bar.output {
            return;
        }
        let Some((i, _)) = self.bars.spares().find(|(_, s)| s.bar.output == *focused) else { return };
        self.promote(i);
    }

    /// Something on the live bar's output that a click outside closes.
    pub(super) fn catches(&self) -> bool {
        self.panel.as_ref().is_some_and(|h| h.is_open() && h.module.takes_keyboard())
            || self.tray_menu.as_ref().is_some_and(|p| p.win.card.is_open())
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
    }

    /// A press on head `i`'s spare bar: whatever a click outside closes is
    /// closed, the live bar's cards go at once rather than over their close,
    /// and the spare is promoted so the press lands on its cell. A modal
    /// still joined to the old line while it closes keeps the press.
    pub(super) fn press_spare(&mut self, i: usize) -> bool {
        if i >= self.bars.heads.len() || i == self.bars.live {
            return false;
        }
        if self.catches() {
            self.dismiss();
        }
        if self.panel.is_some() || self.outgoing.is_some() || self.tray_menu.is_some() {
            self.close_bar_cards();
            self.panel_dirty = true;
            self.sync_join();
            self.sync_bar_open(Instant::now());
        }
        self.promote(i)
    }

    /// A catcher on every output but the live bar's while `catches`, none
    /// otherwise.
    pub(super) fn sync_catchers(&mut self) {
        let want = self.catches();
        for i in 0..self.bars.heads.len() {
            if want && i != self.bars.live {
                if self.bars.heads[i].catcher.is_none() {
                    let output = self.bars.heads[i].output.clone();
                    let layer = self.overlay_on("formalshell:catcher", Layer::Overlay, Anchor::all(), (0, 0), -1, Some(&output));
                    self.bars.heads[i].catcher = Some(PixelSurface::new("catcher", layer, 0.0, &self.pixels, &self.qh));
                    crate::trace::note(format_args!("catcher on {}", self.output_name(&output)));
                }
            } else if self.bars.heads[i].catcher.take().is_some() {
                crate::trace::note(format_args!("catcher off {}", self.output_name(&self.bars.heads[i].output)));
            }
        }
    }

    /// Head `i`'s catcher taking input over its whole output but the strip
    /// its spare bar draws on.
    fn set_catcher_input(&self, i: usize) {
        let head = &self.bars.heads[i];
        let Some((w, h)) = head.catcher.as_ref().and_then(|c| c.size()) else { return };
        let Ok(region) = Region::new(&self.compositor) else { return };
        region.add(0, 0, w, h);
        if let Some(s) = self.bars.spare(i).filter(|s| s.surface.is_some()) {
            let t = s.bar.thickness();
            let band = match s.bar.edge() {
                Edge::Top => IRect::new(0, 0, w, t),
                Edge::Bottom => IRect::new(0, h - t, w, t),
                Edge::Left => IRect::new(0, 0, t, h),
                Edge::Right => IRect::new(w - t, 0, t, h),
            };
            // Under the sidebar only its cards and controls, offset by the
            // band's own place on the output.
            let holes = match s.bar.input_rects().filter(|_| s.bar.sidebar().is_some()) {
                Some(rects) => rects.into_iter().map(|r| IRect::new(r.x + band.x, r.y + band.y, r.w, r.h)).collect(),
                None => vec![band],
            };
            for r in holes {
                region.subtract(r.x, r.y, r.w, r.h);
            }
        }
        if let Some(c) = &head.catcher {
            c.layer.set_input_region(Some(region.wl_region()));
            c.layer.commit();
        }
    }

    /// A card or a join hangs off the live bar.
    fn bar_busy(&self) -> bool {
        self.panel.is_some() || self.outgoing.is_some() || self.tray_menu.is_some() || !self.joins().is_empty()
    }

    pub(super) fn head_part(&self, surface: &WlSurface) -> Option<(usize, Part)> {
        self.bars.heads.iter().enumerate().find_map(|(i, h)| {
            if h.backdrop.as_ref().is_some_and(|b| b.layer.wl_surface() == surface) {
                return Some((i, Part::Backdrop));
            }
            if let Some(n) = h.backdrop.as_ref().and_then(|b| b.shards.as_ref()).and_then(|s| s.owns(surface)) {
                return Some((i, Part::Shards(n)));
            }
            if h.catcher.as_ref().is_some_and(|c| c.layer.wl_surface() == surface) {
                return Some((i, Part::Catcher));
            }
            if h.clock.as_ref().is_some_and(|c| c.surface.layer.wl_surface() == surface) {
                return Some((i, Part::Clock));
            }
            if i == self.bars.live {
                return None;
            }
            let s = &h.bar;
            if s.surface.as_ref().is_some_and(|b| b.layer.wl_surface() == surface) {
                return Some((i, Part::Bar));
            }
            s.zones.iter().position(|(_, z)| z.layer.wl_surface() == surface).map(|z| (i, Part::Zone(z)))
        })
    }

    /// Head `i`'s spare window and zones as its output's fullscreen state
    /// wants them.
    pub(super) fn place_spare(&mut self, i: usize) {
        let Some(head) = self.bars.heads.get(i) else { return };
        let output = head.output.clone();
        let hidden = self.chrome().covered(&head.bar.bar.output);
        let win = &mut self.bars.heads[i].bar;
        win.bar.set_hidden(hidden);
        if hidden {
            win.unmount();
        } else {
            let (surface, zones) = self.build_chrome(&self.bars.heads[i].bar.bar, &output);
            self.bars.heads[i].bar.mount(surface, zones);
        }
    }

    pub(super) fn place_spares(&mut self) {
        for i in 0..self.bars.heads.len() {
            if i != self.bars.live {
                self.place_spare(i);
            }
        }
    }

    /// The spares' share of `refresh_bar`.
    pub(super) fn refresh_spares(&mut self, topic: Option<Topic>, now: Instant) {
        for i in 0..self.bars.heads.len() {
            let Some(s) = self.bars.spare(i) else { continue };
            if self.chrome().covered(&s.bar.output) != s.bar.hidden() {
                self.place_spare(i);
            }
            let s = &mut self.bars.heads[i].bar;
            s.bar.update_paint(&mut self.kit, &self.store);
            super::sync_zone(&mut s.bar, s.surface.as_mut());
            s.bar.read(&mut self.kit, &self.store, topic, now);
            s.dirty = true;
        }
    }

    pub(super) fn tick_spares(&mut self, now: Instant) {
        for (_, s) in self.bars.spares_mut() {
            s.bar.tick(&mut self.kit, &self.store, now);
            s.dirty = true;
        }
    }

    pub(super) fn spares_wake(&self, now: Instant) -> Option<Instant> {
        self.bars.spares().filter_map(|(_, s)| s.bar.wake(&self.kit, now)).min()
    }

    pub(super) fn present_heads(&mut self, all: bool, now: Instant) {
        self.sync_catchers();
        let qh = self.qh.clone();
        for c in self.bars.heads.iter_mut().filter_map(|h| h.catcher.as_mut()) {
            c.present(0.0, false, &qh);
        }
        let mut moved = Vec::new();
        for (i, s) in self.bars.spares_mut() {
            let Some(surface) = &mut s.surface else { continue };
            let animating = s.bar.animating(&self.kit, now);
            let stepping = animating || s.bar.marquee_due(&self.kit, now);
            if all || s.dirty || (stepping && !s.bar.step_in_place(&mut self.kit, now)) {
                s.bar.layout(&mut self.kit, &self.store, now);
                s.dirty = false;
                if Self::set_input_region_for(&self.compositor, &mut s.bar, surface) {
                    moved.push(i);
                }
            }
            surface.present(&mut s.bar.scene, animating, &qh);
            for (_, z) in &mut s.zones {
                z.present(0.0, false, &qh);
            }
        }
        for i in moved {
            self.set_catcher_input(i);
        }
        self.present_desktop_clocks();
    }

    /// The wallpaper for each output's size, asked for again whenever either
    /// moves, and drawn once it is ready; the sculpture over it where
    /// `wallpaper.animate` asks for one.
    pub(super) fn update_backdrops(&mut self) {
        let theme = &self.store.theme.theme;
        let dither = theme.wallpaper_dither.then(|| self.store.config.f64("wallpaper.ditherColors").unwrap_or(6.0).max(1.0) as usize);
        let background = theme.colors.get(ColorRole::Background);
        let wanted = &self.store.desktop();
        let reveal = theme.motion().families.reveal;
        let animate = crate::settings::WALLPAPER_ANIMATE.get(&self.store.config);
        for b in self.bars.heads.iter_mut().filter_map(|h| h.backdrop.as_mut()) {
            let Some((w, h)) = b.pixels() else { continue };
            let grid = theme.wallpaper_grid.then(|| crate::services::wallpaper::grid(theme, b.scale()));
            // Under the sculpture the grid is a layer of its own, over it.
            let under = grid.filter(|_| !animate);
            crate::services::wallpaper::show(wanted, w as u32, h as u32, dither, under);
            let made = self.store.wallpaper.sized(w as u32, h as u32).cloned();
            let picture = made.clone().filter(|p| p.path == *wanted && p.dither == dither && p.grid == under);
            // Another picture still at this size means the wanted one is
            // being made (a failed one empties the size): keep what is up
            // and crossfade once it lands, rather than cut to the plain
            // colour first.
            if picture.is_none() && (made.is_some() || b.holds(wanted)) {
                b.wait(true);
                continue;
            }
            let plain = picture.is_none();
            b.draw(picture, background, under, reveal, &self.qh);
            // The plain colour stands in for a first picture still being
            // made, which the session is not settled without.
            if plain {
                b.wait(!wanted.is_empty() && !self.store.wallpaper.answered(wanted, w as u32, h as u32));
            }
        }
        self.sync_shards();
        self.sync_desktop_clocks();
    }

    /// Each output's sculpture made or dropped, and run or held as its
    /// output's state asks; its beat armed while it runs.
    pub(super) fn sync_shards(&mut self) {
        // Not before the bar's first frame: the mark's word waits on the
        // fonts, and the cold start budget is the bar's. Nor under the boot,
        // which hands the desktop over once it is gone.
        let animate = crate::settings::WALLPAPER_ANIMATE.get(&self.store.config) && self.faces_warmed;
        let word = if animate { Some(self.shard_word()) } else { None };
        let now = Instant::now();
        for i in 0..self.bars.heads.len() {
            let run = self.shard_run(i);
            let theme = &self.store.theme.theme;
            let wanted = &self.store.desktop();
            let output = self.bars.heads[i].output.clone();
            let has = self.bars.heads[i].backdrop.as_ref().map(|b| b.shards.is_some());
            let made = (animate && has == Some(false)).then(|| {
                let layer = |ns| self.overlay_on(ns, Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
                let (shards, grid) = (layer("formalshell:backdrop-shards"), layer("formalshell:backdrop-grid"));
                Shards::new(shards, grid, &self.pixels, &self.shm, &self.qh)
            });
            let qh = &self.qh;
            let Some(b) = self.bars.heads[i].backdrop.as_mut() else { continue };
            if made.is_some() || (!animate && has == Some(true)) {
                if let Some(token) = b.shards.as_mut().and_then(|s| s.timer.take()) {
                    self.handle.remove(token);
                }
                b.animate(made);
            }
            let Some(word) = &word else { continue };
            let Some((w, h)) = b.pixels() else { continue };
            let picture = self.store.wallpaper.sized(w as u32, h as u32).filter(|p| p.path == *wanted);
            let base = picture.and_then(|p| crate::shards::mean(&p.bgra)).unwrap_or(theme.colors.get(ColorRole::Background));
            let look = crate::shards::Look::new(theme, base, word.clone(), 1.0);
            let grid = theme.wallpaper_grid.then(|| crate::services::wallpaper::grid(theme, b.scale()));
            let running = b.sync_shards(look, grid, run, now, qh);
            let Some(s) = b.shards.as_mut() else { continue };
            match (running, s.timer.take()) {
                (true, Some(token)) => s.timer = Some(token),
                (true, None) => s.timer = shard_beat(&self.handle, output),
                (false, Some(token)) => {
                    self.handle.remove(token);
                }
                (false, None) => {}
            }
        }
    }

    /// Why head `i`'s sculpture holds still, if it does: the spec's pause
    /// rules, the first that applies.
    fn shard_run(&self, i: usize) -> Result<(), &'static str> {
        let theme = &self.store.theme.theme;
        let name = self.output_name(&self.bars.heads[i].output);
        let power = &self.store.power;
        if !theme.motion_enabled {
            return Err("reducedMotion");
        }
        if self.lock.locked {
            return Err("locked");
        }
        if self.saver.active {
            return Err("screensaver");
        }
        // The modal's open is the launcher's budget; the scrim dims the
        // sculpture under it anyway.
        if self.modal_up {
            return Err("modal");
        }
        if self.fullscreen_on(&name) {
            return Err("fullscreen");
        }
        if self.store.profiles.as_ref().is_some_and(|p| p.active == fs_upower::Profile::PowerSaver) {
            return Err("powerSaver");
        }
        if power.on_battery && power.battery.as_ref().is_some_and(|b| b.level < SHARD_BATTERY_FLOOR) {
            return Err("lowBattery");
        }
        if self.shard_cover(i) > SHARD_COVER_LIMIT {
            return Err("covered");
        }
        Ok(())
    }

    /// The share of head `i`'s output its active workspace's tiled windows
    /// cover.
    pub(super) fn shard_cover(&self, i: usize) -> f64 {
        let output = &self.bars.heads[i].output;
        let name = self.output_name(output);
        let Some(info) = self.outputs.info(output) else { return 0.0 };
        let Some((w, h)) = info.logical_size else { return 0.0 };
        let (x, y) = info.logical_position.unwrap_or((0, 0));
        let snapshot = &self.store.compositor.snapshot;
        let active: Vec<&str> = snapshot.workspaces.iter().filter(|ws| ws.output == name && ws.is_active).map(|ws| ws.id.as_str()).collect();
        let rects: Vec<(f64, f64, f64, f64)> = snapshot
            .windows
            .iter()
            .filter(|win| !win.is_floating && active.contains(&win.workspace_id.as_str()))
            .filter_map(|win| win.rect)
            .map(|r| (f64::from(r.x), f64::from(r.y), f64::from(r.w), f64::from(r.h)))
            .collect();
        fs_chrome::shards::covered((f64::from(x), f64::from(y), f64::from(w), f64::from(h)), &rects)
    }

    /// Each output's sculpture for `debug dump`: running or why not, its
    /// clock and what it has committed.
    pub(crate) fn backdrop_state(&self) -> serde_json::Value {
        let heads: Vec<serde_json::Value> = (0..self.bars.heads.len())
            .map(|i| {
                let s = self.bars.heads[i].backdrop.as_ref().and_then(|b| b.shards.as_ref());
                serde_json::json!({
                    "output": self.output_name(&self.bars.heads[i].output),
                    "animate": s.is_some(),
                    "running": s.is_some_and(|s| s.running),
                    "paused": s.map_or("", |s| s.why),
                    "clock": s.map_or(0.0, |s| s.t.round()),
                    "commits": s.map_or(0, |s| s.commits),
                    "frameCallbacks": s.map_or(0, |s| s.callbacks),
                    "renderUs": s.map_or(0, |s| s.render_us as u64),
                    "cover": (self.shard_cover(i) * 1000.0).round() / 1000.0,
                })
            })
            .collect();
        serde_json::Value::Array(heads)
    }

    /// The mark's word in the table's display face, shaped once per face.
    pub(super) fn shard_word(&mut self) -> crate::shards::Words {
        let theme = &self.store.theme.theme;
        let style = crate::text::TextStyle {
            family: crate::paint::face(theme.faces.display),
            size: crate::shards::WORD_SIZE,
            weight: 900.0,
            tracking: 0.0,
            wdth: 125.0,
            wght: Some(900.0),
        };
        if let Some((s, word)) = &self.shard_word
            && *s == style
        {
            return word.clone();
        }
        let shaped = self.kit.shape(crate::shards::WORD, style);
        let (cx, cy) = (f64::from(shaped.width) / 2.0, f64::from(shaped.line_height()) / 2.0);
        let pad = f64::from(crate::text::PAD);
        let word: Vec<_> = shaped.glyphs.iter().map(|g| (g.path.clone(), f64::from(g.x) - pad - cx, f64::from(g.y) - pad - cy)).collect();
        let word = std::sync::Arc::new(word);
        self.shard_word = Some((style, word.clone()));
        word
    }

    pub(super) fn head_frame(&mut self, i: usize, part: Part, now: Instant) {
        let live = self.bars.live;
        let Some(head) = self.bars.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => {
                if let Some(b) = &mut head.backdrop {
                    b.landed(now, &self.qh);
                }
            }
            Part::Shards(0) => {
                if let Some(s) = head.backdrop.as_mut().and_then(|b| b.shards.as_mut()) {
                    s.landed();
                }
            }
            Part::Shards(_) => {}
            Part::Bar if i != live => {
                let s = &mut head.bar;
                let Some(surface) = &mut s.surface else { return };
                if surface.landed(now) {
                    s.dirty = true;
                }
            }
            Part::Zone(z) if i != live => {
                let Some((_, z)) = head.bar.zones.get_mut(z) else { return };
                z.landed();
            }
            Part::Bar | Part::Zone(_) => {}
            Part::Catcher => {
                let Some(c) = &mut head.catcher else { return };
                c.landed();
            }
            Part::Clock => {
                let Some(c) = &mut head.clock else { return };
                c.surface.landed(now);
            }
        }
    }

    pub(super) fn head_configure(&mut self, i: usize, part: Part, width: i32, height: i32) {
        let live = self.bars.live;
        let Some(head) = self.bars.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => {
                if let Some(b) = &mut head.backdrop {
                    b.configure(width, height);
                }
                self.update_backdrops();
            }
            Part::Shards(n) => {
                if let Some(c) = head.backdrop.as_mut().and_then(|b| b.shards.as_mut()).and_then(|s| s.configured.get_mut(n)) {
                    *c = true;
                }
                self.sync_shards();
            }
            Part::Bar if i != live => {
                let s = &mut head.bar;
                let current = s.bar.scene.size;
                let w = if width > 0 { width } else { current.w };
                let h = if height > 0 { height } else { current.h };
                s.bar.resize(w, h);
                let size = s.bar.scene.size;
                if let Some(surface) = &mut s.surface {
                    surface.configure(size.w, size.h);
                }
                s.bar.read(&mut self.kit, &self.store, None, Instant::now());
                s.dirty = true;
                if let Some(surface) = &s.surface {
                    Self::set_input_region_for(&self.compositor, &mut s.bar, surface);
                }
            }
            Part::Zone(z) if i != live => {
                if let Some((_, z)) = head.bar.zones.get_mut(z) {
                    z.configure(width.max(1), height.max(1));
                }
            }
            Part::Bar | Part::Zone(_) => {}
            Part::Catcher => {
                if let Some(c) = &mut head.catcher {
                    c.configure(width.max(1), height.max(1));
                }
                self.set_catcher_input(i);
            }
            Part::Clock => {
                if let Some(c) = &mut head.clock {
                    c.configure(width, height);
                }
                self.sync_desktop_clocks();
            }
        }
    }

    pub(super) fn head_closed(&mut self, i: usize, part: Part) {
        let live = self.bars.live;
        let Some(head) = self.bars.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => head.backdrop = None,
            Part::Shards(_) => {
                if let Some(b) = &mut head.backdrop {
                    b.animate(None);
                }
            }
            Part::Bar if i != live => head.bar.surface = None,
            Part::Bar | Part::Zone(_) => {}
            Part::Catcher => head.catcher = None,
            Part::Clock => head.clock = None,
        }
    }
}

/// Below this battery share, discharging, the sculpture holds still.
const SHARD_BATTERY_FLOOR: fs_types::Percent = fs_types::Percent::whole(20);
/// Past this share of the output under tiled windows it holds still.
const SHARD_COVER_LIMIT: f64 = 0.85;

/// Frames a second the sculpture is drawn at, at most.
const SHARD_FPS: f64 = 24.0;

/// The beat that draws `output`'s sculpture at [`SHARD_FPS`]
/// while it runs; it drops itself once the sculpture stops or goes.
fn shard_beat(handle: &calloop::LoopHandle<'static, App>, output: WlOutput) -> Option<calloop::RegistrationToken> {
    use calloop::timer::{TimeoutAction, Timer};
    let period = std::time::Duration::from_secs_f64(1.0 / SHARD_FPS);
    handle
        .insert_source(Timer::from_duration(period), move |deadline, _, app| {
            let now = Instant::now();
            let (scale, qh) = (app.store.theme.theme.motion_scale, app.qh.clone());
            let Some(b) = app.bars.heads.iter_mut().find(|h| h.output == output).and_then(|h| h.backdrop.as_mut()) else {
                return TimeoutAction::Drop;
            };
            if b.shard_tick(now, scale, &qh) {
                // A late beat starts the cadence over rather than firing a
                // burst to catch up.
                TimeoutAction::ToInstant((deadline + period).max(now))
            } else {
                if let Some(s) = &mut b.shards {
                    s.timer = None;
                }
                TimeoutAction::Drop
            }
        })
        .ok()
}
