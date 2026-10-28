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
    pub clock: Option<super::desktop_clock::Clock>,
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
            self.heads.push(Head { output, backdrop, spare, catcher: None, clock: None });
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
            if self.bar_surface.is_none() && !self.bar.hidden() {
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
            if let Some(n) = h.backdrop.as_ref().and_then(|b| b.shards.as_ref()).and_then(|s| s.owns(surface)) {
                return Some((i, Part::Shards(n)));
            }
            if h.catcher.as_ref().is_some_and(|c| c.layer.wl_surface() == surface) {
                return Some((i, Part::Catcher));
            }
            if h.clock.as_ref().is_some_and(|c| c.surface.layer.wl_surface() == surface) {
                return Some((i, Part::Clock));
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
        spare.bar.set_hidden(self.hidden_on(&spare.bar.output));
        (spare.surface, spare.zones) = if spare.bar.hidden() {
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
            if self.hidden_on(&s.bar.output) != s.bar.hidden() {
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

    pub(super) fn spares_marquee_wake(&self, now: Instant) -> Option<Instant> {
        self.heads.iter().filter_map(|h| h.spare.as_ref()).filter_map(|s| s.bar.marquee_wake(now)).min()
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
            let stepping = animating || s.bar.marquee_due(now);
            if all || s.dirty || (stepping && !s.bar.step_in_place(now)) {
                s.bar.layout(&self.store, now);
                s.dirty = false;
            }
            surface.present(&mut s.bar.scene, animating, &qh);
            for (_, z) in &mut s.zones {
                z.present(0.0, false, &qh);
            }
        }
        self.present_desktop_clocks();
    }

    /// The wallpaper for each output's size, asked for again whenever either
    /// moves, and drawn once it is ready; the sculpture over it where
    /// `wallpaper.animate` asks for one.
    pub(super) fn update_backdrops(&mut self) {
        let theme = &self.store.theme.theme;
        let dither = theme.wallpaper_dither.then(|| self.store.config.f64("wallpaper.ditherColors").unwrap_or(6.0).max(1.0) as usize);
        let background = theme.colors.get("background");
        let wanted = &self.store.desktop();
        let reveal = theme.motion().families.reveal * self.motion_scale;
        let animate = theme.wallpaper_animate;
        for b in self.heads.iter_mut().filter_map(|h| h.backdrop.as_mut()) {
            let Some((w, h)) = b.pixels() else { continue };
            let grid = theme.wallpaper_grid.then(|| crate::services::wallpaper::grid(theme, b.scale()));
            // Under the sculpture the grid is a layer of its own, over it.
            let under = grid.filter(|_| !animate);
            crate::services::wallpaper::show(wanted, w as u32, h as u32, dither, under);
            let picture = self.store.wallpaper.sized(w as u32, h as u32).cloned();
            if picture.is_none() && b.holds(wanted) {
                continue;
            }
            b.draw(picture.filter(|p| p.path == *wanted && p.dither == dither && p.grid == under), background, under, reveal, &self.qh);
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
        let animate = self.store.theme.theme.wallpaper_animate && self.faces_warmed && !self.boot_holds();
        let word = if animate { Some(self.shard_word()) } else { None };
        let now = Instant::now();
        for i in 0..self.heads.len() {
            let run = self.shard_run(i);
            let theme = &self.store.theme.theme;
            let wanted = &self.store.desktop();
            let output = self.heads[i].output.clone();
            let has = self.heads[i].backdrop.as_ref().map(|b| b.shards.is_some());
            let made = (animate && has == Some(false)).then(|| {
                let layer = |ns| self.overlay_on(ns, Layer::Background, Anchor::all(), (0, 0), -1, Some(&output));
                let (shards, grid) = (layer("formalshell:backdrop-shards"), layer("formalshell:backdrop-grid"));
                crate::surface::Shards::new(shards, grid, &self.pixels, &self.shm, &self.qh, self.started)
            });
            let qh = &self.qh;
            let Some(b) = self.heads[i].backdrop.as_mut() else { continue };
            if made.is_some() || (!animate && has == Some(true)) {
                if let Some(token) = b.shards.as_mut().and_then(|s| s.timer.take())
                    && let Some(h) = &self.handle
                {
                    h.remove(token);
                }
                b.animate(made);
            }
            let Some(word) = &word else { continue };
            let Some((w, h)) = b.pixels() else { continue };
            let picture = self.store.wallpaper.sized(w as u32, h as u32).filter(|p| p.path == *wanted);
            let base = picture.and_then(|p| crate::shards::mean(&p.bgra)).unwrap_or(theme.colors.get("background"));
            let look = crate::shards::Look::new(theme, base, word.clone(), 1.0);
            let grid = theme.wallpaper_grid.then(|| crate::services::wallpaper::grid(theme, b.scale()));
            let running = b.sync_shards(look, grid, run, now, qh);
            let Some(s) = b.shards.as_mut() else { continue };
            match (running, s.timer.take()) {
                (true, Some(token)) => s.timer = Some(token),
                (true, None) => s.timer = self.handle.as_ref().and_then(|h| shard_beat(h, output)),
                (false, Some(token)) => {
                    if let Some(h) = &self.handle {
                        h.remove(token);
                    }
                }
                (false, None) => {}
            }
        }
    }

    /// Why head `i`'s sculpture holds still, if it does: the spec's pause
    /// rules, the first that applies.
    fn shard_run(&self, i: usize) -> Result<(), &'static str> {
        let theme = &self.store.theme.theme;
        let name = self.output_name(&self.heads[i].output);
        let snapshot = &self.store.compositor.snapshot;
        let power = &self.store.devices.power;
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
        if snapshot.fullscreen_outputs.iter().any(|o| *o == name) {
            return Err("fullscreen");
        }
        if self.store.devices.profiles.as_ref().is_some_and(|p| p.active == fs_upower::Profile::PowerSaver) {
            return Err("powerSaver");
        }
        if power.on_battery && power.battery.as_ref().is_some_and(|b| b.percent < SHARD_BATTERY_FLOOR) {
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
        let output = &self.heads[i].output;
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
            .map(|r| (r.x as f64, r.y as f64, r.width as f64, r.height as f64))
            .collect();
        fs_chrome::shards::covered((f64::from(x), f64::from(y), f64::from(w), f64::from(h)), &rects)
    }

    /// Each output's sculpture for `debug dump`: running or why not, its
    /// clock and what it has committed.
    pub(crate) fn backdrop_state(&self) -> serde_json::Value {
        let heads: Vec<serde_json::Value> = (0..self.heads.len())
            .map(|i| {
                let s = self.heads[i].backdrop.as_ref().and_then(|b| b.shards.as_ref());
                serde_json::json!({
                    "output": self.output_name(&self.heads[i].output),
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
    pub(super) fn shard_word(&mut self) -> std::sync::Arc<Vec<(std::sync::Arc<vello_cpu::kurbo::BezPath>, f64, f64)>> {
        let theme = &self.store.theme.theme;
        let style = crate::text::TextStyle {
            family: crate::surfaces::bar::cell::face(theme.faces.display),
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
        let shaped = self.bar.kit.shape(crate::shards::WORD, style);
        let (cx, cy) = (f64::from(shaped.width) / 2.0, f64::from(shaped.line_height()) / 2.0);
        let pad = f64::from(crate::text::PAD);
        let word: Vec<_> = shaped.glyphs.iter().map(|g| (g.path.clone(), f64::from(g.x) - pad - cx, f64::from(g.y) - pad - cy)).collect();
        let word = std::sync::Arc::new(word);
        self.shard_word = Some((style, word.clone()));
        word
    }

    pub(super) fn head_frame(&mut self, i: usize, part: Part, now: Instant) {
        let Some(head) = self.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => {
                if let Some(b) = &mut head.backdrop {
                    b.step(now, &self.qh);
                }
            }
            Part::Shards(0) => {
                if let Some(s) = head.backdrop.as_mut().and_then(|b| b.shards.as_mut()) {
                    s.frame_done();
                }
            }
            Part::Shards(_) => {}
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
            Part::Clock => {
                let Some(c) = &mut head.clock else { return };
                c.surface.landed(now);
                c.surface.mapped = true;
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
            Part::Shards(n) => {
                if let Some(c) = head.backdrop.as_mut().and_then(|b| b.shards.as_mut()).and_then(|s| s.configured.get_mut(n)) {
                    *c = true;
                }
                self.sync_shards();
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
            Part::Clock => {
                if let Some(c) = &mut head.clock {
                    c.configure(width, height);
                }
                self.sync_desktop_clocks();
            }
        }
    }

    pub(super) fn head_closed(&mut self, i: usize, part: Part) {
        let Some(head) = self.heads.get_mut(i) else { return };
        match part {
            Part::Backdrop => head.backdrop = None,
            Part::Shards(_) => {
                if let Some(b) = &mut head.backdrop {
                    b.animate(None);
                }
            }
            Part::Bar => {
                if let Some(s) = &mut head.spare {
                    s.surface = None;
                }
            }
            Part::Zone(_) => {}
            Part::Catcher => head.catcher = None,
            Part::Clock => head.clock = None,
        }
    }
}

/// Below this battery share, discharging, the sculpture holds still.
const SHARD_BATTERY_FLOOR: f64 = 20.0;
/// Past this share of the output under tiled windows it holds still.
const SHARD_COVER_LIMIT: f64 = 0.85;

/// The beat that draws `output`'s sculpture at [`crate::surface::SHARD_FPS`]
/// while it runs; it drops itself once the sculpture stops or goes.
fn shard_beat(handle: &calloop::LoopHandle<'static, App>, output: WlOutput) -> Option<calloop::RegistrationToken> {
    use calloop::timer::{TimeoutAction, Timer};
    let period = std::time::Duration::from_secs_f64(1.0 / crate::surface::SHARD_FPS);
    handle
        .insert_source(Timer::from_duration(period), move |deadline, _, app| {
            let now = Instant::now();
            let (scale, qh) = (app.motion_scale, app.qh.clone());
            let Some(b) = app.heads.iter_mut().find(|h| h.output == output).and_then(|h| h.backdrop.as_mut()) else {
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
