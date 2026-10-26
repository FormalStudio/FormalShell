//! The bar: three regions out of `bar.layout`
//! (fs-chrome's resolver), the room rule that shares the strip between them
//! (free labels give ground first, then whole cells hide from an end
//! region's inner edge), on whichever edge `bar.position` names, painted as
//! the theme's `bar` habit: the metamorphosis strip or
//! wingpanel's band, whose paint is read off the
//! wallpaper. With the screen frame on, the window is the whole output and
//! paints the ring, strip included, under its cells.
//!
//! Cells are [`cell::Cell`]s in `cells/`, placed as [`slot::Slot`]s.

pub mod cell;
pub mod cells;
pub mod slot;

use std::time::{Duration, Instant};

use fs_chrome::bar::layout::{self, Entry, Label, Rails, Resolved};
use fs_chrome::frame::{self, Gaps};
use fs_chrome::types::{Edge, Insets, Region};
use fs_theme::barpaint;
use fs_theme::color::Rgba;
use fs_theme::style::Glow;
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::{Arc, BezPath, Point, Rect, RoundedRect, Shape, SvgArc, Vec2};

use crate::scene::{IRect, NodeId, Paint, Scene};
use crate::store::{Store, Topic};
use cell::{Action, Button, Env, Kit, Painter};
use slot::Slot;

/// The two cells whose text runs free, in the order they keep their room.
const LABELS: [&str; 2] = ["nowPlaying", "activeWindow"];
/// Nodes held behind every cell for the strip's own paint.
const STRIP_NODES: usize = 6;
/// How long after it maps the strip holds its cells' size clocks off, so a
/// session's first answers land as one layout.
const REVEAL: Duration = Duration::from_millis(1500);

/// The theme's paint for the strip, the band and the ring.
struct Style {
    wingpanel: bool,
    fill: Rgba,
    edge: Option<(Rgba, f64)>,
    thickness: f64,
    margin: f64,
    inset: f64,
    gap: f64,
    frame: (bool, f64, f64, f64, bool),
}

impl Style {
    fn new(theme: &Theme) -> Self {
        let bar = theme.box_style("bar", None);
        let s = &theme.space;
        Self {
            wingpanel: theme.habit("bar").and_then(Value::as_str) == Some("wingpanel"),
            fill: bar.fill,
            edge: bar.edge.map(|l| (l.color, l.width)),
            thickness: s.bar_cell_height,
            margin: s.bar_margin,
            inset: s.md,
            gap: s.sm,
            frame: (theme.frame_enabled(), theme.frame_thickness, theme.frame_radius, theme.frame_requested, theme.frame_habit),
        }
    }
}

/// What the band settled on and why (`bar paint`).
#[derive(Clone, Debug, PartialEq)]
pub struct PaintState {
    pub paint: barpaint::Paint,
    pub pin: &'static str,
    pub fullscreen: bool,
}

pub struct Bar {
    pub scene: Scene,
    pub kit: Kit,
    style: Style,
    edge: Edge,
    /// The wingpanel band's ink and glow for its cells, and its fill.
    band_box: Option<(Rgba, Vec<Glow<Rgba>>)>,
    /// The two inks a cell on a bare band picks between off its own stretch
    /// of wallpaper: the `light` paint's and the `dark` paint's.
    bare_inks: Option<[(Rgba, Vec<Glow<Rgba>>); 2]>,
    band_fill: Option<Rgba>,
    /// The ring's fill, line width and line colour.
    ring_box: (Rgba, f64, Rgba),
    pub resolved: Resolved,
    pub slots: Vec<Slot>,
    strip_nodes: Vec<NodeId>,
    strip_key: Option<String>,
    /// The gaps joined cards open in the lines, `[start, end)` along each:
    /// the strip's own, and a frame ring's side a walled card runs out to.
    gaps: Vec<(Edge, (i32, i32))>,
    line_rects: Vec<IRect>,
    pub output: String,
    /// The scale the bar's surface draws at, which its cells' pictures are
    /// made at.
    pub raster: f64,
    pub paint: Option<PaintState>,
    pub hover: Option<usize>,
    /// The bar is off screen (a fullscreen window over it): nothing runs a
    /// clock.
    hidden: bool,
    born: Instant,
    /// Cells rebuilt (a new theme or layout) and not yet read in full: that
    /// read places them where they were rather than growing every one of
    /// them out of nothing, which blanked the strip on each wallpaper change.
    rebuilt: bool,
    /// Strip room as last laid out, for `bar room`.
    room: Room,
}

#[derive(Default, Clone)]
struct Room {
    slack: f64,
    cells: [usize; 3],
    hidden: [usize; 3],
}

impl Bar {
    pub fn new(edge: Edge, theme: &Theme) -> Self {
        let style = Style::new(theme);
        let mut bar = Self {
            scene: Scene::new(1, 1),
            kit: Kit::new(theme),
            style,
            edge,
            band_box: None,
            bare_inks: None,
            band_fill: None,
            ring_box: (Rgba::TRANSPARENT, 0.0, Rgba::TRANSPARENT),
            resolved: layout::resolve(None, &[]),
            slots: Vec::new(),
            strip_nodes: Vec::new(),
            strip_key: None,
            gaps: Vec::new(),
            line_rects: Vec::new(),
            output: String::new(),
            raster: 1.0,
            paint: None,
            hover: None,
            hidden: false,
            born: Instant::now(),
            rebuilt: false,
            room: Room::default(),
        };
        let (w, h) = bar.placed_size(true);
        bar.scene = Scene::new(w, h);
        bar.rebuild();
        bar
    }

    pub fn edge(&self) -> Edge {
        self.edge
    }

    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// Off screen under a fullscreen window and back: the cells hear it at
    /// once, since no paint reaches them while the bar has no window.
    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        for slot in &mut self.slots {
            slot.cell.visible(!hidden);
        }
    }

    pub fn framed(&self) -> bool {
        self.style.frame.0
    }

    /// `frame` in `debug dump`.
    pub fn frame_state(&self) -> Value {
        let (enabled, thickness, _, requested, habit) = self.style.frame;
        json!({"enabled": enabled, "thickness": number(thickness), "requested": number(requested), "habit": habit})
    }

    pub fn frame_thickness(&self) -> f64 {
        self.style.frame.1
    }

    /// The strip's extent across its edge, which is also its exclusive zone.
    pub fn thickness(&self) -> i32 {
        let across = if self.edge.is_vertical() { self.kit.look.cell_width } else { self.style.thickness };
        (across + self.style.margin * 2.0) as i32
    }

    fn vertical(&self) -> bool {
        self.edge.is_vertical()
    }

    /// The strip's own box in the window: the whole window, or the bar's
    /// edge of a framed one.
    pub fn strip(&self) -> IRect {
        let IRect { w, h, .. } = self.scene.size;
        let t = self.thickness();
        if !self.framed() {
            return IRect::new(0, 0, w, h);
        }
        match self.edge {
            Edge::Top => IRect::new(0, 0, w, t),
            Edge::Bottom => IRect::new(0, h - t, w, t),
            Edge::Left => IRect::new(0, 0, t, h),
            Edge::Right => IRect::new(w - t, 0, t, h),
        }
    }

    /// The strip's length along its own edge.
    pub fn length(&self) -> i32 {
        let s = self.strip();
        if self.vertical() { s.h } else { s.w }
    }

    /// The scene size before the compositor has sent one: the thickness
    /// across and the current length along, or a placeholder.
    pub fn placed_size(&self, turned: bool) -> (i32, i32) {
        let IRect { w, h, .. } = self.scene.size;
        let t = self.thickness();
        if self.framed() {
            return (w.max(1), h.max(1));
        }
        match (turned, self.vertical()) {
            (false, true) => (t, h),
            (false, false) => (w, t),
            (true, true) => (t, 1),
            (true, false) => (1, t),
        }
    }

    pub fn set_edge(&mut self, edge: Edge) -> bool {
        if edge == self.edge {
            return false;
        }
        let turned = edge.is_vertical() != self.edge.is_vertical();
        self.edge = edge;
        self.gaps.clear();
        let (w, h) = self.placed_size(turned);
        self.scene = Scene::new(w, h);
        self.rebuild();
        true
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        if (width, height) == (self.scene.size.w, self.scene.size.h) {
            return;
        }
        self.scene.resize(width, height);
        self.strip_key = None;
    }

    /// A new theme: every metric, ink and the habit. True when the strip's
    /// framing changed with it, which re-places the window.
    pub fn set_theme(&mut self, theme: &Theme) -> bool {
        let framed = self.framed();
        self.kit.set_theme(theme);
        self.style = Style::new(theme);
        let (w, h) = self.placed_size(false);
        self.scene = Scene::new(w, h);
        self.rebuild();
        framed != self.framed()
    }

    /// `bar.layout` and `bar.modules` resolved again; a changed layout
    /// builds its cells afresh.
    pub fn set_layout(&mut self, resolved: Resolved) -> bool {
        if resolved.regions == self.resolved.regions {
            return false;
        }
        for w in &resolved.warnings {
            eprintln!("bar: {w}");
        }
        self.resolved = resolved;
        let size = self.scene.size;
        self.scene = Scene::new(size.w, size.h);
        self.rebuild();
        true
    }

    fn rebuild(&mut self) {
        self.strip_nodes.clear();
        self.strip_key = None;
        for _ in 0..STRIP_NODES {
            let id = self.scene.add(IRect::default(), Paint::Rect { fill: Rgba::TRANSPARENT, radius: 0.0 });
            self.scene.set_visible(id, false);
            self.strip_nodes.push(id);
        }
        self.hover = None;
        self.rebuilt = true;
        self.slots.clear();
        for region in Region::ALL {
            let entries = self.resolved.regions.get(region).to_vec();
            for e in entries.iter().filter(|e| !e.collapsible) {
                let unit = self.slots.last().map_or(0, |s| s.unit + 1);
                self.slots.extend(Slot::rail(e, &entries, unit));
            }
        }
        if self.hidden {
            self.set_hidden(true);
        }
    }

    /// The governed entries of a region, as the second bar holds them.
    pub fn overflow_entries(&self, region: Region) -> Vec<Entry> {
        layout::overflow_entries(self.resolved.regions.get(region))
    }

    pub fn has_chevron(&self, region: Region) -> bool {
        layout::has_chevron(self.resolved.regions.get(region))
    }

    fn animate(&self, now: Instant) -> bool {
        !self.rebuilt && now.saturating_duration_since(self.born) > REVEAL
    }

    /// Re-reads the cells that read `topic` (all of them with `None`).
    pub fn read(&mut self, store: &Store, topic: Option<Topic>, now: Instant) {
        let _raster = crate::scene::raster_at(self.raster);
        let env = Env { store, edge: self.edge, output: &self.output };
        let band = self.band().is_some();
        let animate = self.animate(now);
        let along = self.length() as f64;
        for slot in &mut self.slots {
            if topic.is_none_or(|t| slot.cell.reads().contains(&t)) {
                slot.refresh(&mut self.kit, &env, band, animate, along, now);
            }
        }
        if topic.is_none() {
            self.rebuilt = false;
        }
    }

    /// Reads the cells whose own timer ran out.
    pub fn tick(&mut self, store: &Store, now: Instant) {
        let _raster = crate::scene::raster_at(self.raster);
        let env = Env { store, edge: self.edge, output: &self.output };
        let band = self.band().is_some();
        let animate = self.animate(now);
        let along = self.length() as f64;
        for slot in &mut self.slots {
            if slot.cell.wake().is_some_and(|t| t <= now) {
                slot.refresh(&mut self.kit, &env, band, animate, along, now);
            }
        }
    }

    /// The pointer over the strip, handed to each cell that washes its own
    /// parts in the cell's own coordinates; true when the strip needs a pass.
    pub fn pointer(&mut self, over: Option<usize>, at: (f64, f64), store: &Store, now: Instant) -> bool {
        let (mut redraw, mut relayout) = (false, false);
        for (i, s) in self.slots.iter_mut().enumerate() {
            let local = (Some(i) == over).then(|| (at.0 - s.rect.x as f64, at.1 - s.rect.y as f64));
            if let Some(c) = s.cell.custom() {
                let (r, l) = c.pointer(local);
                s.dirty |= r;
                (redraw, relayout) = (redraw | r, relayout | l);
            }
        }
        if relayout {
            self.read(store, Some(Topic::Compositor), now);
        }
        redraw
    }

    /// `workspaces status`'s cell, when the strip carries one.
    pub fn workspaces_status(&self) -> Option<Value> {
        self.slots.iter().find_map(|s| s.cell.status())
    }

    /// Which panel or second bar each cell has open.
    pub fn set_open(&mut self, panel: Option<&str>, overflow: Option<Region>, now: Instant) {
        for slot in &mut self.slots {
            let open = match &slot.view.panel {
                Some(p) => panel == Some(*p),
                None => slot.name == "chevron" && overflow == Some(slot.region),
            };
            slot.set_open(open, &self.kit, now);
        }
    }

    /// The band's ink and glow under the wingpanel habit, none for a strip.
    fn band(&self) -> Option<(Rgba, Vec<Glow<Rgba>>)> {
        self.band_box.clone()
    }

    /// The wingpanel band's paint off the shared wallpaper reading, the
    /// output's fullscreen state and `bar.paint`; true when it moved. The
    /// ring wears the same paint.
    pub fn update_paint(&mut self, store: &Store) -> bool {
        let theme = &store.theme.theme;
        let paint = self.style.wingpanel.then(|| {
            let pin_raw = store.config.str("bar.paint").map(str::to_owned).or_else(|| theme.habit("paint").and_then(Value::as_str).map(str::to_owned));
            let pin = barpaint::pin(pin_raw.as_deref()).as_str();
            let fullscreen = store.compositor.snapshot.fullscreen_outputs.contains(&self.output);
            let stats = store.bar_paint.stats();
            let paint = barpaint::decide(stats.as_ref(), &theme.colors.mode, fullscreen, Some(pin));
            PaintState { paint, pin, fullscreen }
        });
        let state = paint.as_ref().map(|p| p.paint.as_str());
        let bar = theme.box_style("bar", state);
        let ring = theme.box_style("frame", state);
        let line = ring.border.clone().unwrap_or(fs_theme::style::Line { color: Rgba::TRANSPARENT, width: 0.0 });
        let band_box = paint.as_ref().map(|_| (bar.ink, bar.ink_shadow.clone()));
        let bare_inks = paint.as_ref().map(|_| {
            [barpaint::Paint::Light, barpaint::Paint::Dark].map(|p| {
                let b = theme.box_style("bar", Some(p.as_str()));
                (b.ink, b.ink_shadow)
            })
        });
        let band_fill = paint.as_ref().map(|_| bar.fill);
        let ring_box = (ring.fill, line.width, line.color);
        let changed = paint != self.paint
            || band_box != self.band_box
            || bare_inks != self.bare_inks
            || band_fill != self.band_fill
            || ring_box != self.ring_box;
        if changed {
            let ink_changed = band_box != self.band_box;
            self.paint = paint;
            self.band_box = band_box;
            self.bare_inks = bare_inks;
            self.band_fill = band_fill;
            self.ring_box = ring_box;
            self.strip_key = None;
            if ink_changed {
                // Labels go semibold on a band.
                self.read(store, None, Instant::now());
            }
        }
        changed
    }

    /// `bar paint`'s answer for this bar, null under a strip. `cells` is the
    /// ink each placed cell wears, the band's own unless its stretch of a
    /// bare band read otherwise.
    pub fn paint_state(&self, store: &Store) -> Value {
        let Some(p) = &self.paint else { return Value::Null };
        let s = store.bar_paint.numbers();
        let cells: Vec<Value> = self
            .slots
            .iter()
            .filter(|s| !s.rect.is_empty())
            .map(|s| json!({"name": s.name, "paint": s.ink.unwrap_or(p.paint).as_str()}))
            .collect();
        json!({
            "paint": p.paint.as_str(),
            "cells": cells,
            "source": store.bar_paint.source,
            "pin": p.pin,
            "pinned": p.pin != "auto",
            "mean": s.mean,
            "std": s.std,
            "acutance": s.acutance,
            "sampled": s.sampled,
            "fullscreen": p.fullscreen,
        })
    }

    pub fn wingpanel(&self) -> bool {
        self.style.wingpanel
    }

    pub fn line_rects(&self) -> &[IRect] {
        &self.line_rects
    }

    /// The joins cards publish (`join`): each one's edge, its
    /// start and width along that line and the fillets' reach.
    /// Every gap open this frame, one per edge at most.
    pub fn set_joins(&mut self, joins: &[(Edge, f64, f64, f64)]) {
        let gaps: Vec<(Edge, (i32, i32))> =
            joins.iter().map(|(e, x, width, reach)| (*e, ((x - reach).round() as i32, (x + width + reach).round() as i32))).collect();
        if gaps != self.gaps {
            self.gaps = gaps;
            self.strip_key = None;
        }
    }

    /// The gap in the strip's own line.
    fn gap(&self) -> Option<(i32, i32)> {
        self.gaps.iter().find(|(e, _)| *e == self.edge).map(|(_, g)| *g)
    }

    /// One pass: budgets, positions and paint. Cheap enough per frame;
    /// unchanged nodes damage nothing.
    pub fn layout(&mut self, store: &Store, now: Instant) {
        let _raster = crate::scene::raster_at(self.raster);
        self.refit(store, now);
        self.place(now);
        if self.fit_rooms(store, now) {
            self.place(now);
        }
        self.ink_cells(store);
        self.paint_strip();
        let visible = !self.hidden;
        let edge = self.edge;
        let (band, bare) = (&self.band_box, &self.bare_inks);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.rect.is_empty() {
                slot.hide(&mut self.scene);
                continue;
            }
            let ink = slot_ink(band, bare, slot.ink);
            slot.paint(&mut self.kit, &mut self.scene, edge, self.hover == Some(i), ink, 1.0, visible, now);
        }
    }

    /// Each placed cell's ink off the stretch of wallpaper under it
    /// (`barpaint::cell`), read where the cell stands now, so a cell
    /// gliding across a bright-to-dark seam flips as it crosses.
    fn ink_cells(&mut self, store: &Store) {
        let Some(p) = &self.paint else {
            for slot in &mut self.slots {
                slot.ink = None;
            }
            return;
        };
        let strip = self.strip();
        let vertical = self.edge.is_vertical();
        let profile = store.bar_paint.profile.as_ref();
        for slot in &mut self.slots {
            let r = slot.rect;
            slot.ink = match profile {
                Some(profile) if !r.is_empty() => {
                    let (from, to) = if vertical {
                        ((r.y - strip.y) as f64 / strip.h.max(1) as f64, (r.bottom() - strip.y) as f64 / strip.h.max(1) as f64)
                    } else {
                        ((r.x - strip.x) as f64 / strip.w.max(1) as f64, (r.right() - strip.x) as f64 / strip.w.max(1) as f64)
                    };
                    Some(barpaint::cell(p.paint, Some(p.pin), &profile.span(from, to)))
                }
                _ => Some(p.paint),
            };
        }
    }

    /// Tells each self-drawn cell the room it has (its own extent plus the
    /// strip's slack, which the cell's own change cannot move); true when
    /// one measured again.
    fn fit_rooms(&mut self, store: &Store, now: Instant) -> bool {
        if self.length() <= 1 {
            return false;
        }
        let slack = self.room.slack;
        let env = Env { store, edge: self.edge, output: &self.output };
        let band = self.band_box.is_some();
        let animate = self.animate(now);
        let along = self.length() as f64;
        let mut moved = false;
        for slot in &mut self.slots {
            let own = slot.extent(now);
            if slot.cell.custom().is_some_and(|c| c.room(own + slack)) {
                slot.refresh(&mut self.kit, &env, band, animate, along, now);
                moved = true;
            }
        }
        moved
    }

    /// The two free labels share the room the cells leave
    /// (layout.js's labelBudgets); a moved budget measures its cell again.
    fn refit(&mut self, store: &Store, now: Instant) {
        let along = self.length() as f64;
        let rail = |slots: &[Slot], r: Region, gap: f64| {
            let exts: Vec<f64> = slots.iter().filter(|s| s.region == r && s.present()).map(|s| s.natural).collect();
            exts.iter().sum::<f64>() + gap * exts.len().saturating_sub(1) as f64
        };
        let gap = self.style.gap;
        let rails = Rails {
            left: rail(&self.slots, Region::Left, gap),
            center: rail(&self.slots, Region::Center, gap),
            right: rail(&self.slots, Region::Right, gap),
        };
        let mut at = Vec::new();
        let mut labels = Vec::new();
        for name in LABELS {
            let Some(i) = self.slots.iter().position(|s| s.name == name && s.present()) else { continue };
            let s = &self.slots[i];
            let Some(limits) = s.limits else { continue };
            labels.push(Label { region: s.region, extent: s.free_extent(), natural: s.free_natural(), cap: limits.cap, min: limits.min });
            at.push(i);
        }
        if labels.is_empty() {
            return;
        }
        let budgets = layout::label_budgets(along, self.style.inset, gap, rails, &labels);
        let env = Env { store, edge: self.edge, output: &self.output };
        let band = self.band_box.is_some();
        let animate = self.animate(now);
        for (i, budget) in at.into_iter().zip(budgets) {
            if (self.slots[i].budget - budget).abs() > 0.25 {
                self.slots[i].budget = budget;
                self.slots[i].refresh(&mut self.kit, &env, band, animate, along, now);
            }
        }
    }

    /// The regions: the centre held at the middle between the two end
    /// rails, each end region fitting whole cells from its own anchored edge.
    fn place(&mut self, now: Instant) {
        let along = self.length() as f64;
        let (inset, gap) = (self.style.inset, self.style.gap);
        let exts = |slots: &[Slot], r: Region| -> Vec<(usize, f64)> {
            slots.iter().enumerate().filter(|(_, s)| s.region == r).map(|(i, s)| (i, s.extent(now))).filter(|(_, e)| *e > 0.0).collect()
        };
        let implicit = |e: &[(usize, f64)]| e.iter().map(|x| x.1).sum::<f64>() + gap * e.len().saturating_sub(1) as f64;
        let (left, center, right) = (exts(&self.slots, Region::Left), exts(&self.slots, Region::Center), exts(&self.slots, Region::Right));
        let (li, ci, ri) = (implicit(&left), implicit(&center), implicit(&right));
        let floor = inset + gap + li;
        let ceiling = along - inset - gap - ri - ci;
        let middle = (along - ci) / 2.0;
        let c_at = floor.max(middle.min(ceiling));
        let left_room = (c_at - inset - gap).max(0.0);
        let right_room = (along - inset - gap - c_at - ci).max(0.0);
        // A rail's cells are one delegate to the region: kept or hidden whole.
        let units = |items: &[(usize, f64)], slots: &[Slot]| -> Vec<f64> {
            let mut out: Vec<(usize, f64)> = Vec::new();
            for (i, e) in items {
                match out.last_mut() {
                    Some((unit, ext)) if *unit == slots[*i].unit => *ext += gap + e,
                    _ => out.push((slots[*i].unit, *e)),
                }
            }
            out.into_iter().map(|u| u.1).collect()
        };
        let (left_units, center_units, mut right_rev) = (units(&left, &self.slots), units(&center, &self.slots), units(&right, &self.slots));
        right_rev.reverse();
        let left_fit = layout::fit_extent(&left_units, gap, left_room);
        let right_fit = layout::fit_extent(&right_rev, gap, right_room);
        self.room = Room {
            slack: along - inset * 2.0 - gap * 2.0 - li - ci - ri,
            cells: [left_units.len(), center_units.len(), right_rev.len()],
            hidden: [left_units.len() - left_fit.count, 0, right_rev.len() - right_fit.count],
        };

        let strip = self.strip();
        let across = if self.vertical() { self.kit.look.cell_width } else { self.style.thickness };
        let margin = self.style.margin;
        let vertical = self.vertical();
        let rect = |from: f64, extent: f64| -> IRect {
            let (a0, a1) = (from.round() as i32, (from + extent).round() as i32);
            if vertical {
                IRect::new(strip.x + margin as i32, strip.y + a0, across as i32, a1 - a0)
            } else {
                IRect::new(strip.x + a0, strip.y + margin as i32, a1 - a0, across as i32)
            }
        };
        let whole_strip = strip;
        for s in &mut self.slots {
            s.rect = IRect::default();
        }
        let lay = |items: &[(usize, f64)], start: f64, clip: IRect, slots: &mut [Slot]| {
            let mut at = start;
            for (i, e) in items {
                slots[*i].rect = rect(at, *e);
                slots[*i].clip = clip;
                at += e + gap;
            }
        };
        let left_box = rect(inset, left_fit.extent);
        let right_box = rect(along - inset - right_fit.extent, right_fit.extent);
        lay(&left, inset, left_box, &mut self.slots);
        lay(&center, c_at, whole_strip, &mut self.slots);
        lay(&right, along - inset - ri, right_box, &mut self.slots);
        for s in &mut self.slots {
            if !s.rect.intersects(&s.clip) {
                s.rect = IRect::default();
            }
        }
    }

    /// The strip's own paint: the strip habit's fill and hairline, the
    /// band's fill, or the frame's ring with its line.
    fn paint_strip(&mut self) {
        let key = format!("{:?} {:?} {:?} {:?} {:?}", self.scene.size, self.gaps, self.edge, self.paint, self.ring_box);
        if self.strip_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.strip_key = Some(key);
        let size = self.scene.size;
        let strip = self.strip();
        let len = self.length();
        let mut lines = Vec::new();
        let t = self.thickness() as f64;
        let edge = self.edge;
        let ring = self.ring_box;
        let gap = self.gap();
        let all_gaps = self.gaps.clone();
        let band_fill = self.band_fill;
        let mut p = Painter::new(&mut self.scene, &mut self.strip_nodes, None);
        if self.style.frame.0 {
            let (_, ft, fr, _, _) = self.style.frame;
            let on = |e: Edge| if e == edge { t } else { ft };
            let insets = Insets { top: on(Edge::Top), bottom: on(Edge::Bottom), left: on(Edge::Left), right: on(Edge::Right) };
            let g = frame::frame_geometry(size.w as f64, size.h as f64, insets, fr);
            let mut band = BezPath::new();
            band.extend(Rect::new(g.outer.x, g.outer.y, g.outer.x + g.outer.width, g.outer.y + g.outer.height).path_elements(0.1));
            let inner = Rect::new(g.inner.x, g.inner.y, g.inner.x + g.inner.width, g.inner.y + g.inner.height);
            band.extend(RoundedRect::from_rect(inner, g.radius).path_elements(0.1));
            let mut gaps = Gaps::default();
            for (e, (a, b)) in &all_gaps {
                let gap = Some((*a as f64, *b as f64));
                match e {
                    Edge::Top => gaps.top = gap,
                    Edge::Bottom => gaps.bottom = gap,
                    Edge::Left => gaps.left = gap,
                    Edge::Right => gaps.right = gap,
                }
            }
            let sr = frame::stroke_rect(g.inner, g.radius, ring.1);
            let line = frame::ring_line(fs_chrome::types::Rect::new(sr.x, sr.y, sr.width, sr.height), sr.radius, gaps);
            let mut path = BezPath::new();
            let s = &line.sides;
            for seg in [s.top, s.right, s.bottom, s.left].iter().flatten() {
                if (seg.x2 - seg.x1).abs() + (seg.y2 - seg.y1).abs() > 0.01 {
                    path.move_to((seg.x1, seg.y1));
                    path.line_to((seg.x2, seg.y2));
                }
            }
            for c in line.corners.iter().filter(|c| !c.gone) {
                let from = Point::new(c.x1, c.y1);
                path.move_to(from);
                let arc = SvgArc { from, to: Point::new(c.x2, c.y2), radii: Vec2::new(sr.radius, sr.radius), x_rotation: 0.0, large_arc: false, sweep: true };
                match Arc::from_svg_arc(&arc) {
                    Some(a) => path.extend(a.append_iter(0.05)),
                    None => path.line_to((c.x2, c.y2)),
                }
            }
            p.shape(size, Paint::Shape { fill: Some((band, ring.0)), strokes: vec![(path, ring.2, ring.1)] });
        } else if let Some(fill) = band_fill {
            p.rect(strip, fill, 0.0);
        } else {
            p.rect(strip, self.style.fill, 0.0);
            if let Some((color, width)) = self.style.edge.filter(|(c, w)| c.a > 0.0 && *w > 0.0) {
                let t = width.round().max(1.0) as i32;
                let (start, end) = gap.unwrap_or((0, 0));
                let (start, end) = (start.clamp(0, len), end.clamp(start.clamp(0, len), len));
                let IRect { w, h, .. } = strip;
                let segment = |a: i32, b: i32| match edge {
                    Edge::Top => IRect::new(a, h - t, b - a, t),
                    Edge::Bottom => IRect::new(a, 0, b - a, t),
                    Edge::Left => IRect::new(w - t, a, t, b - a),
                    Edge::Right => IRect::new(0, a, t, b - a),
                };
                let (first, second) = (segment(0, start), segment(end, len));
                lines = vec![first, second];
                if start > 0 {
                    p.rect(first, color, 0.0);
                }
                if end < len {
                    p.rect(second, color, 0.0);
                }
            }
        }
        p.finish();
        self.line_rects = lines;
    }

    /// A frame in which only cells' own drawing moves (a badge's spinner):
    /// those slots repainted where they are, with no layout of the strip.
    /// False when anything along it changes, for a whole layout instead.
    pub fn step_in_place(&mut self, now: Instant) -> bool {
        let visible = !self.hidden;
        let vertical = self.edge.is_vertical();
        let band = self.band_box.is_some();
        if !self.slots.iter_mut().all(|s| s.moves_in_place(&mut self.kit, vertical, band, visible, now)) {
            return false;
        }
        let edge = self.edge;
        let (band, bare) = (&self.band_box, &self.bare_inks);
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.rect.is_empty() || !slot.drawing(now) {
                continue;
            }
            let ink = slot_ink(band, bare, slot.ink);
            slot.paint(&mut self.kit, &mut self.scene, edge, self.hover == Some(i), ink, 1.0, visible, now);
        }
        true
    }

    pub fn animating(&mut self, now: Instant) -> bool {
        let visible = !self.hidden;
        let kit = &self.kit;
        self.slots.iter_mut().any(|s| s.animating(kit, visible, now))
    }

    /// The next time something holding still starts to move again (a
    /// marquee ending its hold).
    pub fn wake(&self, now: Instant) -> Option<Instant> {
        let visible = !self.hidden;
        self.slots.iter().filter_map(|s| s.wake(&self.kit, visible, now)).min()
    }

    /// The slot under a point of the window, if it is the strip's.
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        let (x, y) = (x.floor() as i32, y.floor() as i32);
        self.slots.iter().position(|s| {
            let r = s.rect.intersect(&s.clip);
            !r.is_empty() && x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
        })
    }

    pub fn click(&mut self, i: usize, button: Button, x: f64, y: f64, store: &Store) -> Action {
        let env = Env { store, edge: self.edge, output: &self.output };
        let s = &mut self.slots[i];
        if !s.view.interactive {
            return Action::None;
        }
        s.cell.click(button, (x - s.rect.x as f64, y - s.rect.y as f64), &env)
    }

    pub fn wheel(&mut self, i: usize, vertical: f64, sideways: f64, store: &Store) -> Action {
        let env = Env { store, edge: self.edge, output: &self.output };
        self.slots[i].cell.wheel_by(vertical, sideways, &env)
    }

    /// The anchor a tooltip hangs off (the tooltip surface is the
    /// panels'): the hovered cell's text, its rect in the window and the
    /// edge the card opens away from.
    #[allow(dead_code)]
    pub fn tooltip(&self) -> Option<(String, IRect, Edge)> {
        let s = &self.slots[self.hover?];
        if let Some((text, start, extent)) = s.cell.tip_at() {
            let (start, extent) = (start.round() as i32, extent.round() as i32);
            let r = s.rect;
            let part = if self.vertical() { IRect::new(r.x, r.y + start, r.w, extent) } else { IRect::new(r.x + start, r.y, extent, r.h) };
            return Some((text, part, self.edge));
        }
        (!s.view.tooltip.is_empty()).then(|| (s.view.tooltip.clone(), s.rect, self.edge))
    }

    /// Where a panel's own cell sits along the strip, for an open with no
    /// click to read one off.
    pub fn panel_anchor(&self, name: &str) -> Option<f64> {
        let s = self.slots.iter().find(|s| s.view.panel == Some(name) && !s.rect.is_empty())?;
        let r = s.rect;
        Some(if self.vertical() { r.y as f64 + r.h as f64 / 2.0 } else { r.x as f64 + r.w as f64 / 2.0 })
    }

    pub fn slot_anchor(&self, i: usize) -> f64 {
        let r = self.slots[i].rect;
        if self.vertical() { r.y as f64 + r.h as f64 / 2.0 } else { r.x as f64 + r.w as f64 / 2.0 }
    }

    /// `bar room`'s answer for this bar.
    pub fn room(&self) -> Value {
        let strip = self.strip();
        let label = |name: &str| {
            let Some(s) = self.slots.iter().find(|s| s.name == name && s.present()) else {
                return json!({"budget": -1, "natural": 0, "extent": 0, "cap": 0, "min": 0, "scrolling": false});
            };
            let limits = s.limits.unwrap_or(cell::Limits { cap: 0.0, min: 0.0 });
            json!({
                "budget": if s.budget.is_finite() { number(s.budget) } else { json!(-1) },
                "cap": number(limits.cap),
                "min": number(limits.min),
                "natural": number(s.free_natural()),
                "extent": number(s.free_extent()),
                "scrolling": s.scrolling(&self.kit, !self.hidden),
            })
        };
        let cells: Vec<Value> = self
            .slots
            .iter()
            .filter_map(|s| {
                let r = s.rect.intersect(&s.clip).intersect(&strip);
                (!r.is_empty()).then(|| {
                    json!({
                        "region": s.region.as_str(),
                        "name": s.name,
                        "x": r.x - strip.x,
                        "y": r.y - strip.y,
                        "width": r.w,
                        "height": r.h,
                        "whole": r == s.rect,
                    })
                })
            })
            .collect();
        let room = &self.room;
        json!({
            "screen": self.output,
            "edge": self.edge.as_str(),
            "along": self.length(),
            "edgeInset": number(self.style.inset),
            "gap": number(self.style.gap),
            "slack": number(room.slack),
            "regions": {
                "left": {"cells": room.cells[0], "hidden": room.hidden[0]},
                "center": {"cells": room.cells[1], "hidden": 0},
                "right": {"cells": room.cells[2], "hidden": room.hidden[2]},
            },
            "nowPlaying": label("nowPlaying"),
            "activeWindow": label("activeWindow"),
            "cells": cells,
        })
    }
}

/// A number as JSON.stringify prints it: no `.0` on a whole one.
pub fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 { json!(n as i64) } else { json!(n) }
}

/// The band ink one slot draws with: the `light` or `dark` paint's when its
/// own stretch read one, the band's otherwise, none under a strip.
fn slot_ink<'a>(
    band: &'a Option<(Rgba, Vec<Glow<Rgba>>)>,
    bare: &'a Option<[(Rgba, Vec<Glow<Rgba>>); 2]>,
    ink: Option<barpaint::Paint>,
) -> Option<(Rgba, &'a [Glow<Rgba>])> {
    let own = match (ink, bare) {
        (Some(barpaint::Paint::Light), Some(b)) => Some(&b[0]),
        (Some(barpaint::Paint::Dark), Some(b)) => Some(&b[1]),
        _ => None,
    };
    own.or(band.as_ref()).map(|(c, g)| (*c, g.as_slice()))
}
