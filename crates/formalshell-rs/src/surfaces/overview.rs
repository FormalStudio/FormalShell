//! The workspace overview (`docs/superpowers/specs/2026-10-31-overview.md`):
//! niri's, drawn by the shell. The focused output's workspaces zoomed out
//! and stacked top to bottom, each a card of the output holding its windows
//! at their own places in the layout, the selected one centred.
//!
//! Everything is laid out once at rest, in "stack space": cards at the zoom,
//! one under the other from the top. A frame of the zoom or of the stack's
//! glide moves all of it through one transform, so nothing is laid out
//! again and nothing snaps to whole pixels while it moves.
//!
//! This is the model and the drawing; `wayland::overview` owns the window,
//! the clocks, the captures and the input.

use std::collections::HashMap;
use std::time::Instant;

use fs_chrome::bar::workspaces::{self, SlotOpts};
use fs_theme::color::Rgba;
use fs_theme::theme::Theme;
use vello_cpu::kurbo::{self, Affine};
use serde_json::{Value, json};

use crate::motion::{Animated, Curve};
use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::services::compositor::model::{Snapshot, Window};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::surfaces::bar::cells::workspaces::{window, workspace};
use crate::text::{Family, TextStyle};

/// niri's default zoom.
pub const ZOOM: f64 = 0.5;
/// The gap between two workspaces at rest, a share of the output's height
/// (niri's 16px over a 1080p output at its 0.5 zoom).
pub const GAP: f64 = 0.05;
/// The black over the backdrop at rest. Darker than the modal scrim: at niri's the
/// backdrop reads at about a quarter of the cards' brightness, which is what lifts them.
pub const DIM: f64 = 0.75;
/// How far a press travels before it lifts the window.
pub const LIFT_PX: f64 = 6.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Closed,
    /// Mapped and clear, waiting on the current workspace's frames.
    Opening,
    Open,
    Closing,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Phase::Closed => "closed",
            Phase::Opening => "opening",
            Phase::Open => "open",
            Phase::Closing => "closing",
        }
    }
}

/// One window in a row, its rect relative to the output.
#[derive(Clone, Debug, PartialEq)]
pub struct Win {
    pub id: String,
    pub rect: (f64, f64, f64, f64),
    pub floating: bool,
}

/// One workspace of the output, or the empty slot under the last.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Empty for the slot.
    pub workspace_id: String,
    pub idx: i64,
    pub name: String,
    /// In reading order (x, then y): what Left and Right walk.
    pub windows: Vec<Win>,
}

/// The output's workspaces as the bar and the minimap list them, each
/// with its windows in screen order, and the empty slot after them.
/// `origin` is the output's logical top left.
pub fn rows(snapshot: &Snapshot, output: &str, origin: (f64, f64)) -> Vec<Row> {
    let ws: Vec<workspaces::Workspace> = snapshot.workspaces.iter().map(workspace).collect();
    let wins: Vec<workspaces::Window> = snapshot.windows.iter().map(window).collect();
    let by_id: HashMap<&str, &Window> = snapshot.windows.iter().map(|w| (w.id.as_str(), w)).collect();
    let mut rows: Vec<Row> = workspaces::slots(&ws, &wins, output, SlotOpts { persistent: 0, max_icons: usize::MAX })
        .into_iter()
        .filter(|s| !s.placeholder && s.idx >= 1)
        .map(|s| {
            let windows = s.windows.iter().filter(|w| !snapshot.parked.contains(&w.id)).filter_map(|w| win_of(by_id.get(w.id.as_str())?, origin)).collect();
            Row { workspace_id: s.id, idx: s.idx, name: s.name, windows }
        })
        .collect();
    let next = rows.last().map_or(1, |r| r.idx + 1);
    rows.push(Row { workspace_id: String::new(), idx: next, name: String::new(), windows: Vec::new() });
    rows
}

fn win_of(w: &Window, origin: (f64, f64)) -> Option<Win> {
    let r = w.rect?;
    (r.width > 0 && r.height > 0).then(|| Win {
        id: w.id.clone(),
        rect: (r.x as f64 - origin.0, r.y as f64 - origin.1, r.width as f64, r.height as f64),
        floating: w.is_floating,
    })
}

/// The selection: a row, and a window in it when it has any.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sel {
    pub row: usize,
    pub win: Option<usize>,
}

impl Sel {
    /// The focused window's row and place, else the focused workspace's.
    pub fn of(rows: &[Row], workspace: &str, window: &str) -> Sel {
        for (i, r) in rows.iter().enumerate() {
            if let Some(j) = r.windows.iter().position(|w| w.id == window) {
                return Sel { row: i, win: Some(j) };
            }
        }
        let row = rows.iter().position(|r| !r.workspace_id.is_empty() && r.workspace_id == workspace).unwrap_or(0);
        Sel { row, win: (!rows.get(row).is_none_or(|r| r.windows.is_empty())).then_some(0) }
    }

    /// One row up or down, landing on the window nearest the old one's
    /// middle.
    pub fn step_row(self, rows: &[Row], by: i32) -> Sel {
        let row = (self.row as i64 + i64::from(by)).clamp(0, rows.len() as i64 - 1) as usize;
        if row == self.row {
            return self;
        }
        let mid = self.win.and_then(|j| rows.get(self.row)?.windows.get(j)).map(|w| w.rect.0 + w.rect.2 / 2.0);
        let target = &rows[row].windows;
        let win = match mid {
            Some(m) => target
                .iter()
                .enumerate()
                .min_by(|a, b| (a.1.rect.0 + a.1.rect.2 / 2.0 - m).abs().total_cmp(&(b.1.rect.0 + b.1.rect.2 / 2.0 - m).abs()))
                .map(|(j, _)| j),
            None => (!target.is_empty()).then_some(0),
        };
        Sel { row, win }
    }

    pub fn step_win(self, rows: &[Row], by: i32) -> Sel {
        let n = rows.get(self.row).map_or(0, |r| r.windows.len());
        if n == 0 {
            return self;
        }
        let j = self.win.map_or(0, |j| (j as i64 + i64::from(by)).clamp(0, n as i64 - 1) as usize);
        Sel { win: Some(j), ..self }
    }

    pub fn edge(self, rows: &[Row], last: bool) -> Sel {
        let n = rows.get(self.row).map_or(0, |r| r.windows.len());
        if n == 0 { self } else { Sel { win: Some(if last { n - 1 } else { 0 }), ..self } }
    }

    /// The same window or workspace after the rows were read again.
    pub fn follow(self, old: &[Row], new: &[Row]) -> Sel {
        let id = self.win.and_then(|j| old.get(self.row)?.windows.get(j)).map(|w| w.id.clone());
        if let Some(id) = id {
            for (i, r) in new.iter().enumerate() {
                if let Some(j) = r.windows.iter().position(|w| w.id == id) {
                    return Sel { row: i, win: Some(j) };
                }
            }
        }
        let ws = old.get(self.row).map(|r| r.workspace_id.clone()).unwrap_or_default();
        let row = new.iter().position(|r| r.workspace_id == ws && !ws.is_empty()).unwrap_or(self.row.min(new.len().saturating_sub(1)));
        let n = new.get(row).map_or(0, |r| r.windows.len());
        Sel { row, win: (n > 0).then(|| self.win.unwrap_or(0).min(n - 1)) }
    }
}

/// The rest layout's numbers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geometry {
    /// The surface's size.
    pub area: (f64, f64),
    /// The output's logical size and where the surface sits on it.
    pub output: (f64, f64),
    pub offset: (f64, f64),
    pub card: (f64, f64),
    pub gap: f64,
    /// The cards' left edge in the surface.
    pub left: f64,
}

impl Geometry {
    pub fn new(area: (f64, f64), output: (f64, f64), offset: (f64, f64)) -> Self {
        let card = (output.0 * ZOOM, output.1 * ZOOM);
        Self { area, output, offset, card, gap: (output.1 * GAP).round(), left: ((area.0 - card.0) / 2.0).round() }
    }

    /// Row `i`'s card in stack space.
    pub fn card_at(&self, i: usize) -> (f64, f64, f64, f64) {
        (self.left, i as f64 * (self.card.1 + self.gap), self.card.0, self.card.1)
    }

    /// Stack space to the surface at zoom progress `p` (0 the live desktop,
    /// 1 at rest) with the stack's glide on row `glide`.
    pub fn transform(&self, p: f64, glide: f64) -> Affine {
        let s = (self.left, glide * (self.card.1 + self.gap));
        let rest = (self.left, ((self.area.1 - self.card.1) / 2.0).round());
        let live = (-self.offset.0, -self.offset.1);
        let k = 1.0 / ZOOM + (1.0 - 1.0 / ZOOM) * p;
        let a = (live.0 + (rest.0 - live.0) * p, live.1 + (rest.1 - live.1) * p);
        Affine::translate(a) * Affine::scale(k) * Affine::translate((-s.0, -s.1))
    }

    /// A window's box in stack space, on row `i`.
    pub fn win_at(&self, i: usize, w: &Win) -> (f64, f64, f64, f64) {
        let (cx, cy, _, _) = self.card_at(i);
        (cx + w.rect.0 * ZOOM, cy + w.rect.1 * ZOOM, w.rect.2 * ZOOM, w.rect.3 * ZOOM)
    }

    /// How far row `i` slides sideways so window `w` is whole on screen
    /// (inside the card when it fits, else its left edge on the card's).
    pub fn slide_for(&self, i: usize, w: &Win) -> f64 {
        let (x, _, ww, _) = self.win_at(i, w);
        let (lo, hi) = (self.left, self.left + self.card.0);
        if ww >= hi - lo || x < lo {
            lo - x
        } else if x + ww > hi {
            hi - (x + ww)
        } else {
            0.0
        }
    }
}

/// A window being dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub id: String,
    pub from_row: usize,
    /// Where the press landed, in the surface.
    pub from: (f64, f64),
    pub at: (f64, f64),
    /// The press's offset inside the window's box at rest.
    pub grab: (f64, f64),
    pub lifted: bool,
    /// The row under the pointer.
    pub over: Option<usize>,
}

#[derive(Default)]
pub struct Overview {
    pub phase: Phase,
    pub rows: Vec<Row>,
    pub sel: Sel,
    pub geo: Geometry,
    pub output: String,
    /// 0 is the live desktop, 1 the overview at rest.
    pub zoom: Option<Animated>,
    /// The stack's centre row.
    pub glide: Option<Animated>,
    /// Each row's sideways slide, by workspace id.
    pub slides: HashMap<String, Animated>,
    pub opened_at: Option<Instant>,
    pub drag: Option<Drag>,
    pub opens: u64,
    pub nodes: Vec<NodeId>,
}

/// What one draw reads.
pub struct Input<'a> {
    pub theme: &'a Theme,
    /// The output's wallpaper as drawn, or a flat colour.
    pub wallpaper: Option<&'a Bitmap>,
    /// The wallpaper averaged down, drawn back up as the backdrop.
    pub backdrop: Option<&'a Bitmap>,
    pub flat: Option<Rgba>,
    pub thumbs: &'a HashMap<String, Bitmap>,
    pub icons: &'a HashMap<String, Bitmap>,
    pub now: Instant,
}

impl Overview {
    pub fn open(&self) -> bool {
        self.phase != Phase::Closed
    }

    pub fn progress(&self, now: Instant) -> f64 {
        self.zoom.as_ref().map_or(0.0, |z| z.value(now).clamp(0.0, 1.0))
    }

    pub fn glide_at(&self, now: Instant) -> f64 {
        self.glide.as_ref().map_or(self.sel.row as f64, |g| g.value(now))
    }

    pub fn animating(&self, now: Instant) -> bool {
        self.zoom.as_ref().is_some_and(|z| z.running(now))
            || self.glide.as_ref().is_some_and(|g| g.running(now))
            || self.slides.values().any(|s| s.running(now))
    }

    /// The selection moved: the stack and the row follow on `ms`.
    pub fn select(&mut self, sel: Sel, now: Instant, ms: f64, curve: Curve) {
        self.sel = sel;
        let g = self.glide.get_or_insert_with(|| Animated::new(sel.row as f64, curve));
        if g.target() != sel.row as f64 {
            g.set(now, sel.row as f64, ms);
        }
        let Some(row) = self.rows.get(sel.row) else { return };
        let target = sel.win.and_then(|j| row.windows.get(j)).map_or(0.0, |w| self.geo.slide_for(sel.row, w));
        let s = self.slides.entry(row.workspace_id.clone()).or_insert_with(|| Animated::new(target, curve));
        if s.target() != target {
            s.set(now, target, ms);
        }
    }

    fn slide(&self, i: usize, now: Instant) -> f64 {
        self.rows.get(i).and_then(|r| self.slides.get(&r.workspace_id)).map_or(0.0, |s| s.value(now))
    }

    /// Row `i`'s card on the surface now.
    fn card_rect(&self, i: usize, now: Instant) -> (f64, f64, f64, f64) {
        let t = self.geo.transform(self.progress(now), self.glide_at(now));
        bbox(t, self.geo.card_at(i))
    }

    fn win_rect(&self, i: usize, w: &Win, now: Instant) -> (f64, f64, f64, f64) {
        let t = self.geo.transform(self.progress(now), self.glide_at(now)) * Affine::translate((self.slide(i, now), 0.0));
        bbox(t, self.geo.win_at(i, w))
    }

    /// What is under a point on the surface: a row, and a window in it.
    pub fn hit(&self, at: (f64, f64), now: Instant) -> Option<(usize, Option<usize>)> {
        let inside = |r: (f64, f64, f64, f64)| at.0 >= r.0 && at.0 < r.0 + r.2 && at.1 >= r.1 && at.1 < r.1 + r.3;
        for (i, row) in self.rows.iter().enumerate() {
            // Floating windows are drawn last, so they are hit first.
            let mut order: Vec<usize> = (0..row.windows.len()).collect();
            order.sort_by_key(|&j| !row.windows[j].floating);
            if let Some(j) = order.into_iter().find(|&j| inside(self.win_rect(i, &row.windows[j], now))) {
                return Some((i, Some(j)));
            }
            if inside(self.card_rect(i, now)) {
                return Some((i, None));
            }
        }
        None
    }

    /// Each window's id and the logical size its frame is asked at: the
    /// current workspace's at full size, since the zoom starts there.
    pub fn thumb_sizes(&self, current: &str) -> Vec<(String, (u32, u32))> {
        self.rows
            .iter()
            .flat_map(|r| {
                let k = if r.workspace_id == current { 1.0 } else { ZOOM };
                r.windows.iter().map(move |w| (w.id.clone(), ((w.rect.2 * k).round().max(1.0) as u32, (w.rect.3 * k).round().max(1.0) as u32)))
            })
            .collect()
    }

    /// The row the zoom starts and ends on.
    pub fn current_row(&self) -> usize {
        self.sel.row
    }

    pub fn draw(&mut self, input: &Input, kit: &mut Kit, scene: &mut Scene) {
        let t = input.theme;
        let now = input.now;
        let p = self.progress(now);
        let glide = self.glide_at(now);
        let base = self.geo.transform(p, glide);
        let current = self.current_row();
        let fade = |c: Rgba, a: f64| c.with_alpha(c.a * a as f32);
        let border = t.colors.get("border");
        let ring = t.colors.get("ring");
        let muted = t.colors.get("mutedForeground");
        let bw = t.border_width as f32;
        let area = IRect::new(0, 0, self.geo.area.0.round() as i32, self.geo.area.1.round() as i32);
        let slides: Vec<f64> = (0..self.rows.len()).map(|i| self.slide(i, now)).collect();
        let mut nodes = std::mem::take(&mut self.nodes);
        let mut painter = Painter::new(scene, &mut nodes, None);
        // The backdrop is the wallpaper softened under the modal scrim,
        // opaque from the zoom's first frame so the live desktop never
        // shows behind the shrinking workspace.
        let output = IRect::new(-self.geo.offset.0.round() as i32, -self.geo.offset.1.round() as i32, self.geo.output.0.round() as i32, self.geo.output.1.round() as i32);
        match (input.backdrop, input.flat) {
            (_, Some(c)) => painter.rect(area, c, 0.0),
            (Some(image), None) => painter.shape(output, Paint::Picture { image: image.clone(), alpha: 1.0, radius: 0.0 }),
            (None, None) => painter.rect(area, t.colors.get("background"), 0.0),
        }
        painter.rect(area, Rgba { r: 0.0, g: 0.0, b: 0.0, a: (DIM * p) as f32 }, 0.0);
        painter.transform = base;

        let over = self.drag.as_ref().filter(|d| d.lifted).and_then(|d| d.over);
        let dragged = self.drag.as_ref().filter(|d| d.lifted).map(|d| d.id.clone());
        let caption = TextStyle { family: kit.look.sans, size: kit.look.caption, weight: 500.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
        for (i, row) in self.rows.iter().enumerate() {
            // The row the zoom anchors on is the desktop itself at p 0, so
            // it is whole from the first frame; every other one fades in.
            let alpha = if i == current { 1.0 } else { p };
            if alpha <= 0.0 {
                continue;
            }
            let card = irect(self.geo.card_at(i));
            let radius = t.radii.xl as f32;
            let selected = i == self.sel.row && self.sel.win.is_none() || over == Some(i);
            let edge = if selected { ring } else { border };
            if row.workspace_id.is_empty() {
                let b = t.box_style("cell", Some("rest"));
                let fill = b.fill;
                painter.framed(card, fade(fill, alpha), radius, fade(edge, alpha * p), bw);
                let glyph = fs_theme::icons::glyph("plus");
                let st = TextStyle { family: Family::Named(glyph.family), size: 24.0, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
                let g = kit.shape(glyph.text, st);
                painter.text(&g, (card.x + (card.w - g.width) / 2, card.y + (card.h - g.line_height()) / 2), fade(muted, alpha), &[]);
                continue;
            }
            match (input.wallpaper, input.flat) {
                (_, Some(c)) => painter.rect(card, fade(c, alpha), radius * p as f32),
                (Some(image), None) => painter.shape(card, Paint::Picture { image: image.clone(), alpha: alpha as f32, radius: radius * p as f32 }),
                (None, None) => painter.rect(card, fade(t.colors.get("muted"), alpha), radius * p as f32),
            }
            painter.transform = base * Affine::translate((slides[i], 0.0));
            let mut order: Vec<&Win> = row.windows.iter().collect();
            order.sort_by_key(|w| w.floating);
            for w in order {
                if dragged.as_deref() == Some(w.id.as_str()) {
                    continue;
                }
                let r = irect(self.geo.win_at(i, w));
                let on = i == self.sel.row && self.sel.win.and_then(|j| row.windows.get(j)).is_some_and(|s| s.id == w.id);
                paint_window(&mut painter, t, kit, input, w, r, alpha, p, on);
            }
            painter.transform = base;
            painter.framed(card, Rgba::TRANSPARENT, radius, fade(edge, alpha * p), bw);
            let label = kit.shape(&row.idx.to_string(), caption);
            let pad = t.space.sm.round() as i32;
            painter.text(&label, (card.x + pad, card.y + pad), fade(t.colors.get("foreground"), alpha * p), &[]);
        }
        if let Some(d) = self.drag.as_ref().filter(|d| d.lifted)
            && let Some(w) = self.rows.get(d.from_row).and_then(|r| r.windows.iter().find(|w| w.id == d.id))
        {
            let (_, _, ww, wh) = self.geo.win_at(d.from_row, w);
            let r = IRect::new((d.at.0 - d.grab.0).round() as i32, (d.at.1 - d.grab.1).round() as i32, ww.round() as i32, wh.round() as i32);
            painter.transform = Affine::IDENTITY;
            paint_window(&mut painter, t, kit, input, w, r, 1.0, 1.0, true);
        }
        painter.finish();
        self.nodes = nodes;
    }
}

/// A window through the shared thumbnail, its edge coming in with the
/// zoom: at the live desktop the window wears the compositor's own border.
#[allow(clippy::too_many_arguments)]
fn paint_window(painter: &mut Painter, t: &Theme, kit: &mut Kit, input: &Input, w: &Win, r: IRect, alpha: f64, p: f64, on: bool) {
    let th = crate::ui::thumb::Thumb { picture: input.thumbs.get(&w.id), icon: input.icons.get(&w.id), title: "", selected: false, badge: false, edge: p as f32 };
    crate::ui::thumb::paint(painter, kit, t, &th, r, on, on, alpha as f32);
}

impl Overview {
    /// `debug dump`'s `overview`.
    pub fn status(&self, now: Instant, captured: &dyn Fn(&str) -> bool) -> Value {
        let rect = |r: (f64, f64, f64, f64)| json!({"x": r.0.round(), "y": r.1.round(), "width": r.2.round(), "height": r.3.round()});
        let rows: Vec<Value> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                json!({
                    "workspace": r.workspace_id,
                    "idx": r.idx,
                    "card": rect(self.card_rect(i, now)),
                    "windows": r.windows.iter().map(|w| json!({"id": w.id, "rect": rect(self.win_rect(i, w, now)), "captured": captured(&w.id)})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let row = self.rows.get(self.sel.row);
        json!({
            "open": self.open(),
            "phase": self.phase.as_str(),
            "zoom": self.progress(now),
            "output": self.output,
            "origin": [self.geo.offset.0, self.geo.offset.1],
            "selected": {
                "row": self.sel.row,
                "workspace": row.map_or("", |r| r.workspace_id.as_str()),
                "window": self.sel.win.and_then(|j| row?.windows.get(j)).map_or("", |w| w.id.as_str()),
            },
            "rows": rows,
            "drag": self.drag.as_ref().map(|d| json!({"id": d.id, "lifted": d.lifted, "over": d.over})),
            "opens": self.opens,
        })
    }
}

fn irect(r: (f64, f64, f64, f64)) -> IRect {
    IRect::new(r.0.round() as i32, r.1.round() as i32, r.2.round() as i32, r.3.round() as i32)
}

fn bbox(t: Affine, r: (f64, f64, f64, f64)) -> (f64, f64, f64, f64) {
    let b = t.transform_rect_bbox(kurbo::Rect::new(r.0, r.1, r.0 + r.2, r.1 + r.3));
    (b.x0, b.y0, b.width(), b.height())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::compositor::model::{Rect, Workspace};

    fn snapshot() -> Snapshot {
        let ws = |id: &str, idx: i64| Workspace { id: id.into(), idx, output: "DP-1".into(), ..Default::default() };
        let win = |id: &str, ws: &str, x: i64| Window {
            id: id.into(),
            workspace_id: ws.into(),
            rect: Some(Rect { x, y: 10, width: 800, height: 1000 }),
            ..Default::default()
        };
        Snapshot {
            workspaces: vec![ws("2", 2), ws("1", 1), Workspace { id: "9".into(), idx: 1, output: "HDMI-A-1".into(), ..Default::default() }],
            windows: vec![win("b", "1", 1000), win("a", "1", 100), win("c", "2", 2200)],
            ..Default::default()
        }
    }

    #[test]
    fn rows_are_the_outputs_workspaces_in_order_with_a_slot_after() {
        let rows = rows(&snapshot(), "DP-1", (0.0, 0.0));
        assert_eq!(rows.iter().map(|r| r.idx).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(rows[2].workspace_id.is_empty());
        assert_eq!(rows[0].windows.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn rects_are_relative_to_the_output() {
        let rows = rows(&snapshot(), "DP-1", (100.0, 0.0));
        assert_eq!(rows[0].windows[0].rect.0, 0.0);
    }

    #[test]
    fn stepping_rows_lands_on_the_nearest_window_and_stops_at_the_ends() {
        let rows = rows(&snapshot(), "DP-1", (0.0, 0.0));
        let sel = Sel::of(&rows, "1", "b");
        assert_eq!(sel, Sel { row: 0, win: Some(1) });
        assert_eq!(sel.step_row(&rows, 1), Sel { row: 1, win: Some(0) });
        assert_eq!(sel.step_row(&rows, 2), Sel { row: 2, win: None });
        assert_eq!(sel.step_row(&rows, -1), sel);
        assert_eq!(sel.step_win(&rows, 1), sel);
        assert_eq!(sel.step_win(&rows, -1), Sel { row: 0, win: Some(0) });
    }

    #[test]
    fn the_zoom_starts_on_the_output_and_rests_centred() {
        let g = Geometry::new((1766.0, 1080.0), (1920.0, 1080.0), (154.0, 0.0));
        let live = g.transform(0.0, 0.0) * kurbo::Point::new(g.left, 0.0);
        assert!((live.x + 154.0).abs() < 1e-9 && live.y.abs() < 1e-9);
        let far = g.transform(0.0, 0.0) * kurbo::Point::new(g.left + g.card.0, g.card.1);
        assert!((far.x - (1920.0 - 154.0)).abs() < 1e-9 && (far.y - 1080.0).abs() < 1e-9);
        let rest = g.transform(1.0, 1.0) * kurbo::Point::new(g.left, g.card_at(1).1);
        assert!((rest.y - ((1080.0 - 540.0) / 2.0)).abs() < 1e-9);
    }

    #[test]
    fn a_row_slides_until_its_window_is_whole() {
        let g = Geometry::new((1920.0, 1080.0), (1920.0, 1080.0), (0.0, 0.0));
        let off = Win { id: "c".into(), rect: (2200.0, 0.0, 800.0, 1000.0), floating: false };
        let s = g.slide_for(0, &off);
        let (x, _, w, _) = g.win_at(0, &off);
        assert!((x + s + w - (g.left + g.card.0)).abs() < 1e-9);
        let on = Win { id: "a".into(), rect: (100.0, 0.0, 800.0, 1000.0), floating: false };
        assert_eq!(g.slide_for(0, &on), 0.0);
    }
}
