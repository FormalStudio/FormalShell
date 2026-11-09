//! One placed cell (a region's delegate): the cell, its measure, its
//! presence along the strip and fade, its open mark, its marquee clock and
//! the nodes it last drew. The strip holds its cells as slots.

use std::any::Any;
use std::time::{Duration, Instant};

use fs_chrome::bar::layout::{Builtin, Entry, EntryKind};
use fs_chrome::bar::panels::PanelId;
use fs_types::{Edge, Region};
use fs_theme::color::Rgba;
use fs_theme::style::Glow;

use crate::motion::{Animated, Kind};
use crate::paint::{Ink, Kit, Look, Painter};
use crate::scene::{IRect, NodeId, Scene};
use crate::surfaces::bar::cell::{Cell, Env, FreeLabel, Frame, Limits, Measured, Part, View};
use crate::surfaces::bar::cells::{self, chevron::Chevron};

/// What one pass over the strip's cells reads them against, built once
/// for the pass.
pub struct Pass<'a> {
    pub env: Env<'a>,
    /// The strip wears the wingpanel band.
    pub band: bool,
    /// The presence clocks run (held off until the strip has arrived).
    pub animate: bool,
    /// The strip's length.
    pub along: f64,
    pub now: Instant,
}

/// A slot's clocks: its extent along the strip, its presence, the open
/// mark's scale and fade, and a swapped text's crossfade.
struct Clocks {
    along: Animated,
    fade: Animated,
    mark: Animated,
    mark_fade: Animated,
    cross: Animated,
}

impl Clocks {
    fn new() -> Self {
        Self {
            along: Animated::new(0.0, Kind::Spatial.curve()),
            fade: Animated::new(0.0, Kind::Effects.curve()),
            mark: Animated::new(0.0, Kind::SpatialFast.curve()),
            mark_fade: Animated::new(0.0, Kind::Effects.curve()),
            cross: Animated::new(1.0, Kind::Effects.curve()),
        }
    }

    /// Whether any of them moves this frame; the crossfade counts only
    /// while it carries an outgoing text (`crossing`).
    fn running(&self, crossing: bool, now: Instant) -> bool {
        self.along.running(now) || self.fade.running(now) || self.mark.running(now) || self.mark_fade.running(now) || crossing && self.cross.running(now)
    }
}

/// How far into its cycle (the hold, then one loop of the text) a
/// marquee started at `start` is at `now`, in milliseconds, and the hold.
fn marquee_at(free: &FreeLabel, look: &Look, start: Instant, now: Instant) -> (f64, f64) {
    let cycle = look.marquee_hold + free.loop_width / look.marquee_px * 1000.0;
    (now.saturating_duration_since(start).as_secs_f64() * 1000.0 % cycle, look.marquee_hold)
}

pub struct Slot {
    /// The name `bar.layout` gave it.
    pub name: String,
    /// The builtin it is, none for a module or a plugin.
    pub builtin: Option<Builtin>,
    pub region: Region,
    /// The entry it came from: a rail's cells share one, and the region
    /// keeps or hides them together.
    pub unit: usize,
    pub cell: Box<dyn Cell>,
    pub view: View,
    pub measured: Measured,
    /// What the slot measures to, 0 while its cell is not shown.
    pub natural: f64,
    clocks: Clocks,
    /// The free label's budget (`labelBudget`).
    pub budget: f64,
    pub limits: Option<Limits>,
    pub open: bool,
    /// A text part's outgoing string, faded out by the crossfade clock.
    ghost: Option<(usize, Part)>,
    /// When the marquee's current cycle started, while it overflows.
    marquee: Option<Instant>,
    /// The scroll the free label was last painted at, so the frame the
    /// marquee settles back into its hold repaints it too.
    painted_scroll: Option<f64>,
    /// The output is idle (screensaver or lock up): the marquee holds still.
    pub held: bool,
    /// Laid out along the strip, in the owner's scene; empty when the
    /// region's room hid it.
    pub rect: IRect,
    pub clip: IRect,
    /// The band paint this cell's own stretch of wallpaper read, on a
    /// wingpanel band; none under a strip.
    pub ink: Option<fs_theme::barpaint::Paint>,
    nodes: Vec<NodeId>,
    pub dirty: bool,
}

impl Slot {
    /// The slots one `bar.layout` entry places: one, or a rail of cells
    /// that share the entry's place along the strip as one `unit`.
    pub fn rail(entry: &Entry, region_entries: &[Entry], unit: usize) -> Vec<Self> {
        let builtin = match entry.kind {
            EntryKind::Builtin(b) => Some(b),
            _ => None,
        };
        cells::build(entry, region_entries).into_iter().map(|cell| Self::new(entry.name(), builtin, entry.region, unit, cell)).collect()
    }

    fn new(name: String, builtin: Option<Builtin>, region: Region, unit: usize, cell: Box<dyn Cell>) -> Self {
        Self {
            name,
            builtin,
            region,
            unit,
            cell,
            view: View::hidden(),
            measured: Measured::default(),
            natural: 0.0,
            clocks: Clocks::new(),
            budget: f64::INFINITY,
            limits: None,
            open: false,
            ghost: None,
            marquee: None,
            painted_scroll: None,
            held: false,
            rect: IRect::default(),
            clip: IRect::default(),
            ink: None,
            nodes: Vec::new(),
            dirty: true,
        }
    }

    /// The cell as `T`, when it is the builtin `b`: the hooks only one
    /// cell has are reached this way.
    pub fn builtin<T: Cell>(&self, b: Builtin) -> Option<&T> {
        (self.builtin == Some(b)).then(|| (self.cell.as_ref() as &dyn Any).downcast_ref()).flatten()
    }

    pub fn builtin_mut<T: Cell>(&mut self, b: Builtin) -> Option<&mut T> {
        (self.builtin == Some(b)).then(|| (self.cell.as_mut() as &mut dyn Any).downcast_mut()).flatten()
    }

    /// Re-reads the cell and measures it again.
    pub fn refresh(&mut self, kit: &mut Kit, pass: &Pass) {
        let (band, animate, now) = (pass.band, pass.animate, pass.now);
        if self.cell.read(&pass.env) {
            self.dirty = true;
        }
        let vertical = pass.env.edge.is_vertical();
        let old_view = std::mem::replace(&mut self.view, self.cell.view(&kit.look));
        if animate && kit.look.motion {
            if let Some(changed) = swapped_text(&old_view, &self.view) {
                self.ghost = Some(changed);
                self.clocks.cross.jump(0.0);
                self.clocks.cross.set(now, 1.0, kit.look.effects * kit.motion_scale);
            }
        }
        let natural = match self.cell.custom() {
            Some(c) if self.view.shown => c.measure(kit, vertical, band),
            _ => {
                self.measured = kit.measure(&self.view, vertical, self.budget, band);
                if let Some(free) = &self.measured.free {
                    let rest = self.measured.along - free.extent;
                    self.limits = self.cell.limits(&kit.look, pass.along, rest);
                }
                if self.view.shown { self.measured.along } else { 0.0 }
            }
        };
        let look = &kit.look;
        let scale = kit.motion_scale;
        if natural != self.natural || !animate {
            self.natural = natural;
            if animate && look.motion {
                self.clocks.along.set(now, natural, look.spatial * scale);
            } else {
                self.clocks.along.jump(natural);
            }
        }
        let fade = if self.view.shown { 1.0 } else { 0.0 };
        if animate && look.motion {
            self.clocks.fade.set(now, fade, look.effects * scale);
        } else {
            self.clocks.fade.jump(fade);
        }
        self.dirty = true;
    }

    /// The open mark: on while the panel this cell opens is up, and on a
    /// chevron while the quick settings card it opened is.
    pub fn set_open(&mut self, panel: Option<PanelId>, overflow: Option<Region>, kit: &Kit, now: Instant) {
        let mut open = self.view.panel.is_some_and(|p| panel == Some(p));
        if let Some(chevron) = self.builtin_mut::<Chevron>(Builtin::Chevron) {
            let turned = chevron.set_open(overflow);
            open = chevron.open;
            if turned {
                self.view = self.cell.view(&kit.look);
                self.dirty = true;
            }
        }
        if open == self.open {
            return;
        }
        self.open = open;
        let t = if open { 1.0 } else { 0.0 };
        let scale = kit.motion_scale;
        self.clocks.mark.set(now, t, kit.look.spatial_fast * scale);
        self.clocks.mark_fade.set(now, t, kit.look.effects * scale);
        self.dirty = true;
    }

    /// The extent the strip lays this slot out at this frame.
    pub fn extent(&self, now: Instant) -> f64 {
        self.clocks.along.value(now).max(0.0)
    }

    pub fn present(&self) -> bool {
        self.natural > 0.0
    }

    pub fn free_natural(&self) -> f64 {
        self.measured.free.as_ref().map_or(0.0, |f| f.natural)
    }

    pub fn free_extent(&self) -> f64 {
        self.measured.free.as_ref().map_or(0.0, |f| f.extent)
    }

    pub fn scrolling(&self, kit: &Kit, visible: bool) -> bool {
        kit.look.motion && visible && !self.held && self.measured.free.as_ref().is_some_and(|f| f.overflow && f.extent > 0.0)
    }

    /// The marquee's scroll this frame off the clock, fractional, or none
    /// while it holds or fits.
    fn scroll(&mut self, kit: &Kit, visible: bool, now: Instant) -> Option<f64> {
        if !self.scrolling(kit, visible) {
            self.marquee = None;
            return None;
        }
        let start = *self.marquee.get_or_insert(now);
        let (t, hold) = marquee_at(self.measured.free.as_ref()?, &kit.look, start, now);
        (t >= hold).then(|| (t - hold) / 1000.0 * kit.look.marquee_px)
    }

    /// The marquee moved (or started or stopped) since the label was last
    /// painted.
    pub fn marquee_due(&mut self, kit: &Kit, visible: bool, now: Instant) -> bool {
        !self.rect.is_empty() && self.scroll(kit, visible, now) != self.painted_scroll
    }

    /// When the marquee next leaves its hold or the cell's own timer,
    /// whichever comes first.
    pub fn wake(&self, kit: &Kit, visible: bool, now: Instant) -> Option<Instant> {
        let marquee = self.marquee.filter(|_| self.scrolling(kit, visible)).and_then(|start| {
            let (t, hold) = marquee_at(self.measured.free.as_ref()?, &kit.look, start, now);
            (t < hold).then(|| now + Duration::from_secs_f64((hold - t) / 1000.0))
        });
        [marquee, self.cell.wake()].into_iter().flatten().min()
    }

    /// A clock that wants a frame on every vsync, the marquee included: it
    /// moves by the clock at the output's own refresh rate.
    pub fn animating(&mut self, kit: &Kit, visible: bool, now: Instant) -> bool {
        let custom = self.cell.custom().is_some_and(|c| c.animating(now));
        custom || self.view.turning(&kit.look) || self.clocks.running(self.ghost.is_some(), now) || self.scroll(kit, visible, now).is_some()
    }

    /// Whether only the cell's own drawing moves this frame: nothing about
    /// the slot's place, size or fade, and a self-drawn cell measuring what
    /// it measured. A scrolling marquee moves in place. False sends the
    /// frame to a whole layout.
    pub fn moves_in_place(&mut self, kit: &mut Kit, vertical: bool, band: bool, now: Instant) -> bool {
        let still = !self.clocks.running(self.ghost.is_some(), now);
        let shown = self.view.shown;
        let natural = self.natural;
        still
            && match self.cell.custom() {
                Some(c) if c.animating(now) => !shown || (c.measure(kit, vertical, band) - natural).abs() < 0.5,
                _ => true,
            }
    }

    /// The cell's own drawing moves this frame.
    pub fn drawing(&mut self, kit: &Kit, now: Instant) -> bool {
        self.cell.custom().is_some_and(|c| c.animating(now)) || self.view.turning(&kit.look)
    }

    pub fn hide(&mut self, scene: &mut Scene) {
        for id in &self.nodes {
            scene.set_visible(*id, false);
        }
        self.rect = IRect::default();
    }

    /// Draws the slot at `rect` under `clip`.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(
        &mut self,
        kit: &mut Kit,
        scene: &mut Scene,
        edge: Edge,
        hovered: bool,
        band: Option<(Rgba, &[Glow<Rgba>])>,
        alpha: f32,
        visible: bool,
        now: Instant,
    ) {
        self.cell.visible(visible);
        let rect = self.rect;
        let fade = self.clocks.fade.value(now).clamp(0.0, 1.0) as f32 * alpha * self.view.opacity;
        let scroll = self.scroll(kit, visible, now);
        if !self.clocks.cross.running(now) {
            self.ghost = None;
        }
        let own_hover = self.cell.custom().is_some_and(|c| c.own_hover());
        self.painted_scroll = scroll;
        let mut p = Painter::new(scene, &mut self.nodes, Some(self.clip.intersect(&rect)));
        if rect.is_empty() || fade <= 0.0 {
            p.finish();
            return;
        }
        let frame = Frame {
            rect,
            edge,
            hovered: hovered && self.view.interactive && !own_hover,
            open: (self.clocks.mark.value(now), self.clocks.mark_fade.value(now).clamp(0.0, 1.0)),
            alpha: fade,
            band,
            scroll,
            ghost: self.ghost.as_ref().map(|(i, part)| (*i, part, self.clocks.cross.value(now).clamp(0.0, 1.0) as f32)),
            now,
        };
        match self.cell.custom() {
            Some(custom) => {
                kit.draw_box(&mut p, self.view.tone, &frame);
                let ink = Ink::resolve(&kit.look, self.view.tone, band, fade);
                custom.draw(kit, &mut p, rect, &ink, now);
            }
            None => kit.draw(&self.view, &self.measured, &mut p, &frame),
        }
        p.finish();
        self.dirty = false;
    }
}

/// The one text part whose string changed between two views of a cell that
/// crossfades it (an app name, a track title), with the part it replaced.
fn swapped_text(old: &View, new: &View) -> Option<(usize, Part)> {
    if old.parts.len() != new.parts.len() {
        return None;
    }
    let mut changed = old.parts.iter().zip(&new.parts).enumerate().filter(|(_, (a, b))| a != b);
    let (i, (a, b)) = changed.next()?;
    if changed.next().is_some() {
        return None;
    }
    let fades = match (a, b) {
        (Part::Name { text: x, .. }, Part::Name { text: y, .. }) => !x.is_empty() && !y.is_empty() && x != y,
        (Part::Free { text: x, cross: true, .. }, Part::Free { text: y, cross: true, .. }) => !x.is_empty() && !y.is_empty() && x != y,
        _ => false,
    };
    fades.then(|| (i, a.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> Part {
        Part::Name { text: text.into(), max: 100.0, dim: false }
    }

    fn title(text: &str, cross: bool) -> Part {
        Part::Free { text: text.into(), dim: false, lead: 0.0, ceiling: 100.0, cross }
    }

    fn view(parts: Vec<Part>) -> View {
        View::new(parts, 0.0)
    }

    #[test]
    fn a_renamed_app_crossfades_and_the_old_name_is_kept() {
        let old = view(vec![name("Foot"), title("a", false)]);
        let new = view(vec![name("Firefox"), title("a", false)]);
        assert_eq!(swapped_text(&old, &new), Some((0, name("Foot"))));
    }

    #[test]
    fn only_a_title_that_asks_to_crossfade_does() {
        let (old, new) = (view(vec![title("one", false)]), view(vec![title("two", false)]));
        assert_eq!(swapped_text(&old, &new), None);
        let (old, new) = (view(vec![title("one", true)]), view(vec![title("two", true)]));
        assert_eq!(swapped_text(&old, &new), Some((0, title("one", true))));
    }

    #[test]
    fn two_parts_changing_at_once_is_a_cut() {
        let (old, new) = (view(vec![name("a"), title("x", true)]), view(vec![name("b"), title("y", true)]));
        assert_eq!(swapped_text(&old, &new), None);
    }

    #[test]
    fn a_name_arriving_from_nothing_does_not_fade() {
        let (old, new) = (view(vec![name("")]), view(vec![name("Foot")]));
        assert_eq!(swapped_text(&old, &new), None);
    }
}
