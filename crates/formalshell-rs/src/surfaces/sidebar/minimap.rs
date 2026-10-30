//! The Spaces minimap: a row per workspace on the sidebar's output, its
//! ordinal and its windows' icons three across in screen order, one empty
//! row under the last for a new workspace. Icons come off compositor
//! metadata through `appicon`; nothing is captured.
//!
//! The ui kit lays each row out with an empty slot per icon; the icons are
//! painted over those slots here, each gliding to its slot on the spatial
//! clock, so a reorder (a drag, a key, the compositor's own) moves them
//! rather than cutting. A drag lifts its icon under the pointer, the rows
//! open a gap where it would land, and once dropped the map holds the
//! order it asked for until the compositor agrees or `HOLD_MS` passes,
//! after which every icon glides to where the compositor has it: a refused
//! move springs back.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use fs_chrome::bar::workspaces::{self, SlotOpts};
use fs_theme::theme::Theme;
use serde_json::{Value, json};

use vello_cpu::kurbo::Affine;

use crate::motion::{Animated, Kind as Clock};
use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::compositor::Command;
use crate::services::appicon;
use crate::store::Store;
use crate::surfaces::bar::cell::{Action, Kit, Painter};
use crate::surfaces::bar::cells::workspaces::{window, workspace};
use crate::ui::{El, Ink, Variant, w};

/// Icons a row lays across before it wraps.
pub const ACROSS: usize = 3;

/// Icons a row's grid holds across `width` (the column inside its
/// padding), past a label `label` wide.
pub fn across(theme: &Theme, width: f64, label: f64) -> usize {
    let s = &theme.space;
    let room = width - 2.0 * s.md - 2.0 * s.control_padding_x - s.sm - label;
    let cell = icon_size(theme) + 2.0 * s.xs;
    (((room + s.xxs) / (cell + s.xxs)).floor() as usize).clamp(1, ACROSS)
}
/// The cap past which a row stops listing windows.
const MAX: usize = 64;
/// How long a dropped order holds against a compositor that disagrees.
pub const HOLD_MS: u64 = 600;
/// Pointer travel past which a press on an icon lifts it.
const LIFT_PX: f64 = 6.0;
/// The lifted icon's size step.
const LIFT_SCALE: f64 = 1.25;
/// The other icons' alpha while one is lifted.
const DIMMED: f32 = 0.7;

#[derive(Clone)]
pub struct Win {
    pub id: String,
    pub focused: bool,
    pub icon: Option<Bitmap>,
}

#[derive(Clone)]
pub struct Row {
    /// Empty for a persistent slot the compositor has no workspace for.
    pub id: String,
    pub idx: i64,
    pub label: String,
    pub current: bool,
    /// Its windows reorder by column (`Workspace::columns`).
    pub columns: bool,
    pub windows: Vec<Win>,
}

/// The keyboard's place on the map: the window it is on, or a row with no
/// window under it (the empty row is `rows.len()`). It follows its window
/// to wherever the compositor puts it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cursor {
    pub window: Option<String>,
    pub row: usize,
}

/// A carry the compositor is asked to make.
#[derive(Clone, Debug, PartialEq)]

/// A press on an icon, lifted once it has travelled `LIFT_PX`.
pub struct Drag {
    pub id: String,
    from: (f64, f64),
    pub at: (f64, f64),
    pub lifted: bool,
    /// The row and the slot in it the icon would land on.
    pub target: Option<(usize, usize)>,
}

/// Window ids by workspace index.
type Held = Vec<(i64, Vec<String>)>;

#[derive(Default)]
pub struct Map {
    pub rows: Vec<Row>,
    probed: Option<(u32, Vec<appicon::Query>)>,
    pub cursor: Option<Cursor>,
    pub drag: Option<Drag>,
    /// The order a drop asked for, held until the compositor agrees or the
    /// deadline: window ids by workspace index, so a row the compositor
    /// drops or adds meanwhile does not shift the rest (`align`).
    hold: Option<(Held, Instant)>,
    /// Each painted icon's place, gliding to its slot.
    glides: HashMap<String, (Animated, Animated)>,
    /// Each slot as last laid out, by window id, and each row's box.
    slots: HashMap<String, IRect>,
    row_boxes: Vec<(usize, IRect)>,
    /// The rows shown when not all fit the column: from `first`, `fits`
    /// of them (the empty row counted). `seen` is the current row last
    /// brought into view, so a wheel scroll is not undone every pass.
    pub first: usize,
    pub fits: usize,
    seen: Option<usize>,
    /// Icons across a row's grid, set for the column's width.
    pub across: usize,
    nodes: Vec<NodeId>,
    /// Each tiled window's column (its x) on a workspace that reorders by
    /// column: a column moves whole.
    cols: HashMap<String, i64>,
}

impl Map {
    /// The rows for `output` off the compositor's snapshot.
    pub fn read(&mut self, store: &Store, output: &str, theme: &Theme) {
        let c = &store.compositor.snapshot;
        let ws: Vec<workspaces::Workspace> = c.workspaces.iter().map(workspace).collect();
        let wins: Vec<workspaces::Window> = c.windows.iter().map(window).collect();
        let slots = workspaces::slots(&ws, &wins, output, SlotOpts { persistent: 0, max_icons: MAX });
        self.cols = wins
            .iter()
            .filter(|w| !w.is_floating && c.workspaces.iter().any(|ws| ws.id == w.workspace_id && ws.columns))
            .filter_map(|w| Some((w.id.clone(), w.rect.as_ref()?.x.round() as i64)))
            .collect();
        let size = icon_size(theme).round() as u32;
        let scale = crate::scene::raster_scale();
        let queries: Vec<appicon::Query> = slots
            .iter()
            .flat_map(|s| &s.windows)
            .map(|w| appicon::Query {
                id: w.id.clone(),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .collect();
        let px = crate::scene::pixels_at(f64::from(size), scale);
        if self.probed.as_ref().is_none_or(|(s, q)| *s != px || *q != queries) {
            self.probed = Some((px, queries.clone()));
            appicon::probe(size, scale, queries);
        }
        self.rows = slots
            .into_iter()
            .map(|s| Row {
                windows: s
                    .windows
                    .iter()
                    .map(|w| Win { id: w.id.clone(), focused: w.is_focused, icon: store.appicon.get(&w.id, size, scale).and_then(|i| i.image) })
                    .collect(),
                columns: c.workspaces.iter().any(|w| w.id == s.id && w.columns),
                id: s.id,
                idx: s.idx,
                label: s.label,
                current: s.current,
            })
            .collect();
        let now = Instant::now();
        if self.hold.as_ref().is_some_and(|(_, until)| now >= *until) || self.held() == self.base() {
            self.hold = None;
        }
        if let Some(c) = &mut self.cursor {
            match c.window.as_ref().and_then(|id| self.rows.iter().position(|r| r.windows.iter().any(|w| &w.id == id))) {
                Some(row) => c.row = row,
                None => {
                    c.window = None;
                    c.row = c.row.min(self.rows.len());
                }
            }
        }
    }

    /// The cursor's row and its column in that row.
    fn at(&self) -> Option<(usize, usize)> {
        let c = self.cursor.as_ref()?;
        let col = c.window.as_ref().and_then(|id| self.rows.get(c.row)?.windows.iter().position(|w| &w.id == id)).unwrap_or(0);
        Some((c.row, col))
    }

    /// The cursor on the focused window, else on the current row.
    pub fn take_keyboard(&mut self) {
        let row = self.rows.iter().position(|r| r.current).unwrap_or(0);
        let window = self.rows.get(row).and_then(|r| r.windows.iter().find(|w| w.focused).or(r.windows.first())).map(|w| w.id.clone());
        self.cursor = Some(Cursor { window, row });
    }

    /// An arrow: the cursor moves, or with `carry` the window under it does
    /// (along its row a swap with its neighbour, across rows onto that
    /// workspace, past the last onto a new one).
    pub fn step(&mut self, dx: i32, dy: i32, carry: bool) -> Option<Command> {
        let (row, col) = self.at()?;
        let windows = |r: usize| self.rows.get(r).map_or(&[][..], |r| r.windows.as_slice());
        if carry {
            let id = self.cursor.as_ref()?.window.clone()?;
            if dx != 0 {
                let ids: Vec<String> = windows(row).iter().map(|w| w.id.clone()).collect();
                let col_of = |w: &str| self.cols.get(w).copied();
                let j = if self.rows[row].columns {
                    column_step(&ids, &id, dx, &col_of)?
                } else {
                    col.checked_add_signed(dx as isize).filter(|j| *j < ids.len())?
                };
                return Some(reorder(&ids, &id, j, self.rows[row].columns, &col_of));
            }
            let to = row.checked_add_signed(dy as isize)?;
            let idx = match self.rows.get(to) {
                Some(r) => r.idx,
                None if to == self.rows.len() && windows(row).len() > 1 => self.next_idx(),
                None => return None,
            };
            if let Some(c) = &mut self.cursor {
                c.row = to;
            }
            return Some(Command::MoveWindowTo { id, idx, output: String::new() });
        }
        let (row, col) = if dx != 0 {
            (row, col.saturating_add_signed(dx as isize).min(windows(row).len().saturating_sub(1)))
        } else {
            (row.saturating_add_signed(dy as isize).min(self.rows.len()), col)
        };
        let window = windows(row).get(col.min(windows(row).len().saturating_sub(1))).map(|w| w.id.clone());
        self.cursor = Some(Cursor { window, row });
        None
    }

    /// Enter on the cursor: its window, its row's workspace, or a new one.
    pub fn enter(&self, output: &str) -> Action {
        let Some(c) = &self.cursor else { return Action::None };
        match (&c.window, self.rows.get(c.row)) {
            (Some(id), _) => Action::Window(id.clone()),
            (None, Some(_)) => self.action(&format!("row:{}", c.row), output),
            (None, None) => self.action("new", output),
        }
    }

    /// The cursor's stop, for the ring.
    pub fn cursor_key(&self) -> Option<String> {
        let c = self.cursor.as_ref()?;
        Some(match &c.window {
            Some(id) => format!("w:{id}"),
            None if c.row < self.rows.len() => format!("r:{}", c.row),
            None => "new".into(),
        })
    }

    /// The ordinal a window dropped on the empty row gets.
    pub fn next_idx(&self) -> i64 {
        self.rows.iter().map(|r| r.idx).max().unwrap_or(0) + 1
    }

    /// The held order laid over the rows the compositor has now, or
    /// `base` when nothing holds.
    fn held(&self) -> Vec<Vec<String>> {
        match &self.hold {
            Some((held, _)) => align(held, &self.rows),
            None => self.base(),
        }
    }

    /// Every row's window ids as the compositor has them, the empty row's
    /// last.
    fn base(&self) -> Vec<Vec<String>> {
        let mut order: Vec<Vec<String>> = self.rows.iter().map(|r| r.windows.iter().map(|w| w.id.clone()).collect()).collect();
        order.push(Vec::new());
        order
    }

    /// What the map shows: a dropped order while it holds, with a lifted
    /// icon taken out and an empty slot (`""`) where it would land.
    fn shown(&self) -> Vec<Vec<String>> {
        let mut order = self.held();
        if let Some(d) = self.drag.as_ref().filter(|d| d.lifted) {
            for row in &mut order {
                row.retain(|id| *id != d.id);
            }
            if let Some((t, j)) = d.target
                && let Some(row) = order.get_mut(t)
            {
                row.insert(j.min(row.len()), String::new());
            }
        }
        order
    }

    fn win(&self, id: &str) -> Option<&Win> {
        self.rows.iter().flat_map(|r| &r.windows).find(|w| w.id == id)
    }

    pub fn view(&self, theme: &Theme, _now: Instant) -> El {
        let s = &theme.space;
        let size = icon_size(theme);
        let target = self.drag.as_ref().filter(|d| d.lifted).and_then(|d| d.target).map(|t| t.0);
        let shown = self.shown();
        let last = shown.len() - 1;
        let col: Vec<El> = shown
            .iter()
            .enumerate()
            .skip(self.first)
            .take(self.fits)
            .map(|(i, ids)| {
                let (label, current) = match self.rows.get(i) {
                    Some(r) => (r.label.clone(), r.current),
                    None => (format!("{}", self.next_idx() + (i - self.rows.len()) as i64), false),
                };
                let on = if i == last { "new".to_owned() } else { format!("row:{i}") };
                let stop = if i == last { "new".to_owned() } else { format!("r:{i}") };
                row(theme, &label, current, target == Some(i), ids, |id| self.win(id), size, self.across.max(1)).on(on).stop(stop)
            })
            .collect();
        w::column(s.xxs, col).fill()
    }

    /// The widest ordinal a row's label carries.
    pub fn widest_label(&self) -> String {
        let mut labels: Vec<String> = self.rows.iter().map(|r| r.label.clone()).collect();
        labels.push(self.next_idx().to_string());
        labels.into_iter().max_by_key(|l| l.chars().count()).unwrap_or_default()
    }

    /// The rows in the list, the empty one counted.
    pub fn len(&self) -> usize {
        self.shown().len()
    }

    /// `first` moved so the row that matters is shown: the cursor's or a
    /// drag's target always, the current row when it changes.
    pub fn keep_in_view(&mut self) {
        let total = self.len();
        let fits = self.fits.clamp(1, total.max(1));
        let current = self.rows.iter().position(|r| r.current);
        let held = self.cursor.as_ref().map(|c| c.row).or_else(|| self.drag.as_ref().and_then(|d| d.target).map(|t| t.0));
        let row = held.or(current.filter(|c| self.seen != Some(*c)));
        self.seen = current;
        if let Some(r) = row {
            if r < self.first {
                self.first = r;
            } else if r >= self.first + fits {
                self.first = r + 1 - fits;
            }
        }
        self.first = self.first.min(total.saturating_sub(fits));
    }

    /// A wheel notch over the map, `dy` rows on.
    pub fn scroll(&mut self, dy: i32) -> bool {
        let before = self.first;
        self.first = self.first.saturating_add_signed(dy as isize).min(self.len().saturating_sub(self.fits.max(1)));
        before != self.first
    }

    /// A press on hit `on` at `at`: a window's icon may be lifted.
    pub fn press(&mut self, on: &str, at: (f64, f64)) {
        self.drag = on.strip_prefix("window:").map(|id| Drag { id: id.to_owned(), from: at, at, lifted: false, target: None });
    }

    /// The pointer moved with a button held; true when the map needs a
    /// pass.
    pub fn motion(&mut self, at: (f64, f64)) -> bool {
        let Some(d) = &mut self.drag else { return false };
        d.at = at;
        if !d.lifted && (at.0 - d.from.0).hypot(at.1 - d.from.1) < LIFT_PX {
            return false;
        }
        d.lifted = true;
        let target = self.target(at);
        if let Some(d) = &mut self.drag {
            d.target = target;
        }
        true
    }

    /// The row under `at` (above the first is the first, below the empty
    /// row is the empty row) and the slot in it: the icons of that row
    /// ahead of the pointer in reading order.
    fn target(&self, at: (f64, f64)) -> Option<(usize, usize)> {
        let d = self.drag.as_ref()?;
        let (x, y) = at;
        let last = self.row_boxes.last()?.0;
        let t = self.row_boxes.iter().find(|(_, b)| (y as i32) < b.bottom()).map_or(last, |(i, _)| *i);
        let mut order = self.held();
        let row = order.get_mut(t)?;
        let along = row.contains(&d.id) && self.rows.get(t).is_some_and(|r| r.columns);
        let mates = if along { mates(row, &d.id, &|w| self.cols.get(w).copied()) } else { vec![d.id.clone()] };
        row.retain(|id| !mates.contains(id));
        let slot = row
            .iter()
            .filter_map(|id| self.slots.get(id))
            .filter(|r| {
                let (cx, cy, half) = (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0, r.h as f64 / 2.0);
                cy < y - half || ((cy - y).abs() <= half && cx < x)
            })
            .count();
        Some((t, slot))
    }

    /// The button let go. `None` when no icon was lifted (the press is a
    /// click); otherwise the commands the drop asks for, none when it lands
    /// where it started or outside `strip`.
    pub fn release(&mut self, at: (f64, f64), strip: IRect, now: Instant) -> Option<Vec<Command>> {
        let d = self.drag.take()?;
        if !d.lifted {
            return None;
        }
        let inside = at.0 >= strip.x as f64 && at.0 < strip.right() as f64 && at.1 >= strip.y as f64 && at.1 < strip.bottom() as f64;
        let Some((t, j)) = d.target.filter(|_| inside) else { return Some(Vec::new()) };
        let order = self.held();
        let Some(from) = order.iter().position(|row| row.contains(&d.id)) else { return Some(Vec::new()) };
        let columns = self.rows.get(t).is_some_and(|r| r.columns);
        let idx = self.rows.get(t).map(|r| r.idx).unwrap_or_else(|| self.next_idx());
        let (commands, predicted) = drop_on(&order, from, &d.id, t, j, idx, columns, &|w| self.cols.get(w).copied());
        if !commands.is_empty() {
            let next = self.next_idx();
            let keyed = predicted.into_iter().enumerate().map(|(i, ids)| (self.rows.get(i).map_or_else(|| next + (i - self.rows.len()) as i64, |r| r.idx), ids)).collect();
            self.hold = Some((keyed, now + Duration::from_millis(HOLD_MS)));
        }
        Some(commands)
    }

    /// When the held order lets go.
    pub fn wake(&self) -> Option<Instant> {
        self.hold.as_ref().map(|h| h.1)
    }

    /// The icons over the slots `hits` laid out, after `anchor` in paint
    /// order; true while one is still gliding.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, hits: &[crate::ui::Hit], theme: &Theme, kit: &mut Kit, scene: &mut Scene, anchor: Option<NodeId>, clip: IRect, now: Instant) -> bool {
        self.slots = hits.iter().filter_map(|h| Some((h.on.as_ref()?.strip_prefix("window:")?.to_owned(), h.rect))).collect();
        let last = self.rows.len();
        self.row_boxes = hits
            .iter()
            .filter_map(|h| {
                let on = h.on.as_deref()?;
                let i = if on == "new" { last } else { on.strip_prefix("row:")?.parse().ok()? };
                Some((i, h.rect))
            })
            .collect();
        let ms = Clock::Spatial.ms(theme) * kit.motion_scale;
        let lifted = self.drag.as_ref().filter(|d| d.lifted).map(|d| (d.id.clone(), d.at));
        self.glides.retain(|id, _| self.slots.contains_key(id) || lifted.as_ref().is_some_and(|l| &l.0 == id));
        let fg = theme.colors.get("foreground");
        let mut nodes = std::mem::take(&mut self.nodes);
        let mut p = Painter::new(scene, &mut nodes, Some(clip)).after(anchor);
        let mut moving = false;
        let ids: Vec<String> = self.slots.keys().cloned().collect();
        for id in &ids {
            let r = self.slots[id];
            let (x, y) = (r.x as f64 + r.w as f64 / 2.0, r.y as f64 + r.h as f64 / 2.0);
            let g = self.glides.entry(id.clone()).or_insert_with(|| (Animated::new(x, Clock::Spatial.curve()), Animated::new(y, Clock::Spatial.curve())));
            g.0.set(now, x, ms);
            g.1.set(now, y, ms);
            moving |= g.0.running(now) || g.1.running(now);
            let at = (g.0.value(now), g.1.value(now));
            let alpha = if lifted.is_some() { DIMMED } else { 1.0 };
            let icon = self.win(id).and_then(|w| w.icon.clone());
            draw_icon(&mut p, kit, icon.as_ref(), at, 1.0, alpha, fg);
        }
        if let Some((id, at)) = lifted {
            let size = icon_size(theme) * LIFT_SCALE;
            let halo = theme.colors.get("foreground").with_alpha(0.12);
            let card = theme.colors.get("card");
            let reach = (size * 1.6).round() as i32;
            p.rect(IRect::new(at.0 as i32 - reach / 2, at.1 as i32 - reach / 2, reach, reach), halo, reach as f32 / 2.0);
            let plate = (size + theme.space.sm).round() as i32;
            p.framed(IRect::new(at.0 as i32 - plate / 2, at.1 as i32 - plate / 2, plate, plate), card, theme.radii.md as f32, theme.colors.get("border"), 1.0);
            let icon = self.win(&id).and_then(|w| w.icon.clone());
            draw_icon(&mut p, kit, icon.as_ref(), at, LIFT_SCALE, 1.0, fg);
            // It lands from where it was let go.
            let g = self.glides.entry(id).or_insert_with(|| (Animated::new(at.0, Clock::Spatial.curve()), Animated::new(at.1, Clock::Spatial.curve())));
            g.0.jump(at.0);
            g.1.jump(at.1);
        }
        p.finish();
        self.nodes = nodes;
        moving
    }

    /// The rail's chips: one ordinal per workspace, the focused one filled.
    pub fn rail(&self, theme: &Theme) -> El {
        let chips = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let v = if r.current { Variant::Default } else { Variant::Ghost };
                w::button(r.label.clone()).variant(v).on(format!("row:{i}")).centred()
            })
            .collect();
        w::column(theme.space.xxs, chips).fill()
    }

    pub fn action(&self, on: &str, output: &str) -> Action {
        let (verb, arg) = on.split_once(':').unwrap_or((on, ""));
        match verb {
            "window" => Action::Window(arg.to_owned()),
            "row" => match arg.parse::<usize>().ok().and_then(|i| self.rows.get(i)) {
                Some(r) if r.id.is_empty() => Action::WorkspaceAt(r.idx, output.to_owned()),
                Some(r) => Action::Workspace(r.id.clone()),
                None => Action::None,
            },
            "new" => Action::WorkspaceAt(self.next_idx(), output.to_owned()),
            _ => Action::None,
        }
    }

    pub fn status(&self) -> Value {
        Value::Array(
            self.rows
                .iter()
                .map(|r| json!({"id": r.id, "idx": r.idx, "current": r.current, "windows": r.windows.iter().map(|w| &w.id).collect::<Vec<_>>()}))
                .collect(),
        )
    }

    pub fn cursor_status(&self) -> Value {
        self.cursor.as_ref().map_or(Value::Null, |c| json!({"window": c.window, "row": c.row}))
    }

    /// The press or lifted icon, and the order a drop holds.
    pub fn drag_status(&self) -> Value {
        json!({
            "drag": self.drag.as_ref().map(|d| json!({"window": d.id, "lifted": d.lifted, "target": d.target.map(|(t, j)| [t, j])})),
            "held": self.hold.as_ref().map(|h| &h.0),
        })
    }
}

/// A held order (window ids by workspace index) over `rows`: each row
/// takes its workspace's held ids, the empty row the next index's (and a
/// fresh empty row after it when it holds any), windows gone are dropped,
/// and a window the hold places nowhere stays where the compositor has it.
fn align(held: &[(i64, Vec<String>)], rows: &[Row]) -> Vec<Vec<String>> {
    let live = |id: &String| rows.iter().any(|r| r.windows.iter().any(|w| &w.id == id));
    let take = |idx: i64| held.iter().find(|(k, _)| *k == idx).map(|(_, ids)| ids.iter().filter(|id| live(id)).cloned().collect::<Vec<_>>());
    let mut out: Vec<Vec<String>> = rows
        .iter()
        .map(|r| take(r.idx).unwrap_or_else(|| r.windows.iter().map(|w| w.id.clone()).collect()))
        .collect();
    let next = take(rows.iter().map(|r| r.idx).max().unwrap_or(0) + 1).unwrap_or_default();
    let fresh = !next.is_empty();
    out.push(next);
    if fresh {
        out.push(Vec::new());
    }
    for (i, r) in rows.iter().enumerate() {
        for w in &r.windows {
            if !out.iter().any(|row| row.contains(&w.id)) {
                out[i].push(w.id.clone());
            }
        }
    }
    out
}

/// An icon three across a sidebar card, past the ordinal.
fn icon_size(theme: &Theme) -> f64 {
    theme.space.control_height * 0.75
}

/// One icon centred on `at`, `scale` about its centre.
fn draw_icon(p: &mut Painter, kit: &mut Kit, icon: Option<&Bitmap>, at: (f64, f64), scale: f64, alpha: f32, fg: fs_theme::color::Rgba) {
    let about = Affine::scale_about(scale, (at.0, at.1));
    match icon {
        Some(b) => {
            let (w, h) = b.size();
            p.image(b, ((at.0 - w as f64 / 2.0).round() as i32, (at.1 - h as f64 / 2.0).round() as i32), alpha);
        }
        None => {
            let glyph = kit.icon("app-window");
            let (w, h) = glyph.box_size();
            p.text(&glyph, ((at.0 - w as f64 / 2.0).round() as i32 + crate::text::PAD, (at.1 - h as f64 / 2.0).round() as i32 + crate::text::PAD), fg.with_alpha(fg.a * alpha), &[]);
        }
    }
    if scale != 1.0
        && let Some(id) = p.last()
    {
        p.scene().set_transform(id, about);
    }
}

/// `id` to slot `j` of its own row `ids` (counted with `id` taken out),
/// and the row it leaves. Under columns its column moves there; elsewhere
/// it trades places with the window that holds the slot now.
/// The windows of `row` in `id`'s column, `id` alone where `col` knows
/// no column for it.
fn mates(row: &[String], id: &str, col: &dyn Fn(&str) -> Option<i64>) -> Vec<String> {
    match col(id) {
        Some(x) => row.iter().filter(|w| col(w) == Some(x)).cloned().collect(),
        None => vec![id.to_owned()],
    }
}

/// The slot (counted with `id`'s column taken out) one column along from
/// `id`'s, `dx` either way; `None` past either end.
fn column_step(ids: &[String], id: &str, dx: i32, col: &dyn Fn(&str) -> Option<i64>) -> Option<usize> {
    let own = mates(ids, id, col);
    let rest: Vec<&String> = ids.iter().filter(|w| !own.contains(w)).collect();
    let at = ids.iter().take_while(|w| !own.contains(w)).count();
    // Each column's first slot in `rest`.
    let starts: Vec<usize> = (0..rest.len()).filter(|&i| i == 0 || col(rest[i]).is_none() || col(rest[i]) != col(rest[i - 1])).collect();
    if dx < 0 {
        starts.iter().rev().find(|&&i| i < at).copied()
    } else {
        let next = starts.iter().position(|&i| i >= at)?;
        Some(starts.get(next + 1).copied().unwrap_or(rest.len()))
    }
}

/// The command that puts `id` at slot `j` of its row (`ids`): under
/// columns its column moves before the window holding slot `j` with that
/// column taken out; otherwise it trades places with the window at `j`.
fn reorder(ids: &[String], id: &str, j: usize, columns: bool, col: &dyn Fn(&str) -> Option<i64>) -> Command {
    if columns {
        let own = mates(ids, id, col);
        let rest: Vec<&String> = ids.iter().filter(|w| !own.contains(w)).collect();
        return Command::MoveColumn { id: id.to_owned(), before: rest.get(j).map(|w| (*w).clone()) };
    }
    Command::SwapWindows { a: id.to_owned(), b: ids[j.min(ids.len() - 1)].clone() }
}

/// The order `reorder` predicts for `row`: a column lands whole ahead of
/// the column holding slot `j`.
fn reordered(row: &mut Vec<String>, id: &str, j: usize, columns: bool, col: &dyn Fn(&str) -> Option<i64>) {
    let k = row.iter().position(|w| w == id).unwrap_or(0);
    if columns {
        let own = mates(row, id, col);
        row.retain(|w| !own.contains(w));
        let mut at = j.min(row.len());
        while at > 0 && at < row.len() && col(&row[at]).is_some() && col(&row[at - 1]) == col(&row[at]) {
            at -= 1;
        }
        row.splice(at..at, own);
    } else {
        let j = j.min(row.len() - 1);
        row.swap(k, j);
    }
}

/// The commands a drop of `id` from row `from` onto slot `j` of row `t`
/// asks for (`j` counted with `id`, or under columns its column, taken
/// out), and the order it predicts. Along its own row it is `reorder`;
/// onto another row it moves there (`idx`), a column of its own, and then
/// takes the slot, if a window holds it.
#[allow(clippy::too_many_arguments)]
fn drop_on(order: &[Vec<String>], from: usize, id: &str, t: usize, j: usize, idx: i64, columns: bool, col: &dyn Fn(&str) -> Option<i64>) -> (Vec<Command>, Vec<Vec<String>>) {
    let mut out = order.to_vec();
    if t == from {
        reordered(&mut out[from], id, j, columns, col);
        if out[from] == order[from] {
            return (Vec::new(), out);
        }
        return (vec![reorder(&order[from], id, j, columns, col)], out);
    }
    let alone = |w: &str| if w == id { None } else { col(w) };
    let mut commands = vec![Command::MoveWindowTo { id: id.to_owned(), idx, output: String::new() }];
    out[from].retain(|w| w != id);
    let row = &mut out[t];
    row.push(id.to_owned());
    if j < order[t].len() {
        commands.push(reorder(row, id, j, columns, &alone));
        reordered(row, id, j, columns, &alone);
    }
    if t == order.len() - 1 {
        out.push(Vec::new());
    }
    (commands, out)
}

#[allow(clippy::too_many_arguments)]
fn row<'a>(theme: &Theme, label: &str, current: bool, lit: bool, ids: &[String], win: impl Fn(&str) -> Option<&'a Win>, size: f64, across: usize) -> El {
    let s = &theme.space;
    let icons: Vec<El> = ids
        .iter()
        .map(|id| {
            let slot = w::cell(w::picture(None, size));
            if id.is_empty() {
                return slot.cell_state(|c| {
                    c.chip = true;
                    c.small = true;
                    c.tight = true;
                    c.ghost = true;
                });
            }
            let focused = win(id).is_some_and(|w| w.focused);
            slot.cell_state(|c| {
                c.chip = true;
                c.small = true;
                c.tight = true;
                c.ghost = !focused;
                c.selected = focused;
            })
            .interactive()
            .on(format!("window:{id}"))
            .stop(format!("w:{id}"))
        })
        .collect();
    let label = w::caption(label.to_owned()).mono().ink(if current { Ink::Fg } else { Ink::Dim });
    let body = w::row(s.sm, vec![label, w::grid(across, s.xxs, icons).fill()]);
    w::cell(body).ghost().selected(current).cell_state(|c| c.hovered = lit).interactive().fill()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<i64> {
        None
    }

    fn order(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect()
    }

    fn rows(rows: &[(i64, &[&str])]) -> Vec<Row> {
        rows.iter()
            .map(|(idx, ids)| Row {
                id: idx.to_string(),
                idx: *idx,
                label: String::new(),
                current: false,
                columns: false,
                windows: ids.iter().map(|id| Win { id: id.to_string(), focused: false, icon: None }).collect(),
            })
            .collect()
    }

    #[test]
    fn a_held_order_follows_its_workspaces() {
        // `a`, the sole window of workspace 1, dropped ahead of `b` on 2.
        let held = vec![(1, vec![]), (2, vec!["a".to_owned(), "b".to_owned()]), (3, vec![])];
        // The compositor drops workspace 1 before it reorders 2.
        assert_eq!(align(&held, &rows(&[(2, &["b", "a"])])), order(&[&["a", "b"], &[]]));
        // A drop on the empty row, before workspace 3 exists.
        let held = vec![(1, vec!["b".to_owned()]), (2, vec!["a".to_owned()]), (3, vec![])];
        assert_eq!(align(&held, &rows(&[(1, &["a", "b"])])), order(&[&["b"], &["a"], &[]]));
        // A window that went away, and one that arrived.
        let held = vec![(1, vec!["a".to_owned(), "b".to_owned()]), (2, vec![])];
        assert_eq!(align(&held, &rows(&[(1, &["b", "c"])])), order(&[&["b", "c"], &[]]));
    }

    #[test]
    fn a_drop_along_its_row_swaps() {
        let o = order(&[&["a", "b", "c"], &[]]);
        let (cmds, out) = drop_on(&o, 0, "a", 0, 2, 1, false, &none);
        assert!(matches!(&cmds[..], [Command::SwapWindows { a, b }] if a == "a" && b == "c"));
        assert_eq!(out, order(&[&["c", "b", "a"], &[]]));
        assert!(drop_on(&o, 0, "b", 0, 1, 1, false, &none).0.is_empty());
    }

    #[test]
    fn a_drop_on_another_row_moves_then_swaps_into_the_slot() {
        let o = order(&[&["a", "b"], &["c"], &[]]);
        let (cmds, out) = drop_on(&o, 0, "a", 1, 0, 2, false, &none);
        assert!(matches!(&cmds[..], [Command::MoveWindowTo { id, idx: 2, .. }, Command::SwapWindows { a, b }] if id == "a" && a == "a" && b == "c"));
        assert_eq!(out, order(&[&["b"], &["a", "c"], &[]]));
        let (cmds, out) = drop_on(&o, 0, "a", 1, 1, 2, false, &none);
        assert!(matches!(&cmds[..], [Command::MoveWindowTo { .. }]));
        assert_eq!(out, order(&[&["b"], &["c", "a"], &[]]));
    }

    #[test]
    fn a_drop_on_the_empty_row_makes_a_workspace() {
        let o = order(&[&["a", "b"], &[]]);
        let (cmds, out) = drop_on(&o, 0, "a", 1, 0, 2, false, &none);
        assert!(matches!(&cmds[..], [Command::MoveWindowTo { id, idx: 2, .. }] if id == "a"));
        assert_eq!(out, order(&[&["b"], &["a"], &[]]));
    }

    #[test]
    fn under_columns_a_drop_moves_the_column() {
        let o = order(&[&["a", "b", "c"], &["d"], &[]]);
        let (cmds, out) = drop_on(&o, 0, "a", 0, 1, 1, true, &none);
        assert!(matches!(&cmds[..], [Command::MoveColumn { id, before: Some(b) }] if id == "a" && b == "c"));
        assert_eq!(out[0], ["b", "a", "c"]);
        let (cmds, out) = drop_on(&o, 0, "c", 0, 0, 1, true, &none);
        assert!(matches!(&cmds[..], [Command::MoveColumn { id, before: Some(b) }] if id == "c" && b == "a"));
        assert_eq!(out[0], ["c", "a", "b"]);
        let (cmds, out) = drop_on(&o, 0, "a", 0, 2, 1, true, &none);
        assert!(matches!(&cmds[..], [Command::MoveColumn { before: None, .. }]));
        assert_eq!(out[0], ["b", "c", "a"]);
        let (cmds, out) = drop_on(&o, 0, "b", 1, 0, 2, true, &none);
        assert!(matches!(&cmds[..], [Command::MoveWindowTo { .. }, Command::MoveColumn { id, before: Some(b) }] if id == "b" && b == "d"));
        assert_eq!(out[1], ["b", "d"]);
    }

    #[test]
    fn a_column_moves_whole() {
        // Columns [a b] [c] [d].
        let col = |w: &str| match w {
            "a" | "b" => Some(0),
            "c" => Some(1),
            "d" => Some(2),
            _ => None,
        };
        let o = order(&[&["a", "b", "c", "d"], &[]]);
        let (cmds, out) = drop_on(&o, 0, "a", 0, 2, 1, true, &col);
        assert!(matches!(&cmds[..], [Command::MoveColumn { id, before: None }] if id == "a"));
        assert_eq!(out[0], ["c", "d", "a", "b"]);
        let (cmds, out) = drop_on(&o, 0, "d", 0, 1, 1, true, &col);
        assert!(matches!(&cmds[..], [Command::MoveColumn { id, before: Some(b) }] if id == "d" && b == "b"));
        assert_eq!(out[0], ["d", "a", "b", "c"]);
        assert!(drop_on(&o, 0, "b", 0, 0, 1, true, &col).0.is_empty());
        let ids: Vec<String> = o[0].clone();
        assert_eq!(column_step(&ids, "a", 1, &col), Some(1));
        assert_eq!(column_step(&ids, "d", -1, &col), Some(2));
        assert_eq!(column_step(&ids, "c", -1, &col), Some(0));
        assert_eq!(column_step(&ids, "a", -1, &col), None);
    }
}
