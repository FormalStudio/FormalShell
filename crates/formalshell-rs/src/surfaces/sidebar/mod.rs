// Portions from Pond (BSD-3-Clause, Copyright 2026 Butterfly Interaction Inc.)
//! The sidebar (docs/superpowers/specs/2026-10-30-sidebar.md): the bar's
//! body under `layout: sidebar`. The bar keeps its window, its edge, its
//! zone and its joins; instead of cells it draws a column of cards through
//! the ui kit, the status card pinned to its foot, or the rail while
//! collapsed. Each control carries an [`Intent`] (or an Activity row's or
//! the minimap's own), and one that opens something lands as a cell
//! [`Action`], so the bar's own routing opens what a cell would; folding
//! the column and the Software card's buttons run here.
//!
//! The composition and its lengths are Pond's sidebar; [`Geo`] carries
//! them. What the kit's type scale cannot draw (the clock's digits, its period and the
//! date box) is painted over the time card's reserved box, the digits in
//! the desktop clock's own face and glass (`desktopClock.*`).

pub mod minimap;

use std::collections::HashMap;
use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_types::{Edge, NotificationId};
use fs_info::clock::format_qt;
use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::Role;
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::scene::{Bitmap, IRect, NodeId, Scene};
use crate::services::compositor::Command;
use crate::services::activity::Verb as ActivityVerb;
use crate::services::{motion_art, packages};
use crate::services::state::{self, Field};
use crate::store::Store;
use crate::surfaces::activity;
use crate::surfaces::bar::cell::Action;
use crate::paint::{Kit, Painter};
use crate::surfaces::panel::center;
use crate::text::TextStyle;
use crate::ui::{El, Ink, Type, Ui, Weight, w};
use crate::surfaces::desktop_clock::{self as glass, Digits, FaceSettings, Glass, Under, clock_format};

/// The cards in the order they give up their place when the column runs
/// short: the first is the first to go.
const DROPS: [&str; 4] = ["media", "software", "activity", "notifications"];

/// Pond's sidebar lengths at our card: Pond's 156 wide card is 136 here, so
/// each is Pond's times 136/156, rounded (Pond's in the comments). They
/// grow with `sidebarWidth` (154: the card and a margin either side).
pub struct Geo {
    /// Screen edge to card, and card to the windows [10].
    pub margin: f64,
    /// Between cards [6].
    pub gap: f64,
    /// A simple row's inset from the card's side [16] and its height [48].
    pub row_pad: f64,
    pub row_h: f64,
    /// The time card [58]: the digits [39] at x [12], the period [11], and
    /// the date box [40] inset [12] with its radius [6] and words [13],
    /// whose baselines sit [18] and [31] down the box.
    pub time_h: f64,
    /// Under the time with a laptop battery, Pond's level line [17 more],
    /// [2] thick.
    pub battery_h: f64,
    pub battery_line: f64,
    pub time_x: f64,
    pub digits: f64,
    pub period: f64,
    pub date_box: f64,
    pub date_inset: f64,
    pub date_radius: f64,
    pub date_words: f64,
    pub date_lines: (f64, f64),
    /// The media card [62] and its art [38].
    pub media_h: f64,
    pub art: f64,
    /// Between neighbours in a row [8].
    pub spacing: f64,
    /// A notification's app icon [14] and the rule before them [14].
    pub glyph: f64,
    /// A window's circle on the minimap [40], its icon [16], the gap
    /// between circles and rows [8], an empty slot's dot [4], and the
    /// current row's plate inset from the card's side [4].
    pub slot: f64,
    pub icon: f64,
    pub slot_gap: f64,
    pub dot: f64,
    pub plate_inset: f64,
    /// A row's number pill [14 x 20] and its digits [11].
    pub pill: (f64, f64),
    pub pill_words: f64,
    pub rail: Rail,
}

/// Pond's collapsed column (its 48 wide card) at our scale.
pub struct Rail {
    /// Its words [13] in the mono face, a line apart [13].
    pub words: f64,
    pub line: f64,
    /// The time card [113]: the hour's cap top [16], the divider [16 x 1]
    /// centred at [63], and the weekday's cap top [75].
    pub time_h: f64,
    pub cap_top: f64,
    pub divider: f64,
    pub divider_y: f64,
    pub date_top: f64,
    /// A workspace row [41] and the gap under it [8], the current row's
    /// header [above its windows, 37], its windows' circles [32] at a
    /// pitch [40], the plate's inset [4], and its reach past the last
    /// circle and the card's padding above and below the rows [4].
    pub row: f64,
    pub row_gap: f64,
    pub header: f64,
    pub circle: f64,
    pub pitch: f64,
    pub plate_inset: f64,
    pub plate_pad: f64,
    /// A row's window dots [4], apart [2], under the number's baseline
    /// [4], at most [4].
    pub dot: f64,
    pub dot_gap: f64,
    pub dot_drop: f64,
    /// A square card [48], the art in it [32].
    pub square: f64,
    pub art: f64,
    /// The installs and the foot cards' padding [4] and their rows [40].
    pub pad: f64,
    pub button: f64,
    /// Bell to count in the notifications card [2].
    pub tight: f64,
}

impl Geo {
    pub fn of() -> Self {
        let p = |v: f64| v.round();
        Self {
            margin: p(9.0),
            gap: p(5.0),
            row_pad: p(14.0),
            row_h: p(42.0),
            time_h: p(50.0),
            battery_h: p(15.0),
            battery_line: p(2.0),
            time_x: p(10.0),
            digits: p(34.0),
            period: p(10.0),
            date_box: p(35.0),
            date_inset: p(10.0),
            date_radius: p(5.0),
            date_words: p(11.0),
            date_lines: (p(16.0), p(27.0)),
            media_h: p(54.0),
            art: p(33.0),
            spacing: p(7.0),
            glyph: p(12.0),
            slot: p(35.0),
            icon: p(14.0),
            slot_gap: p(6.0),
            dot: p(4.0),
            plate_inset: p(4.0),
            pill: (p(12.0), p(17.0)),
            pill_words: p(10.0),
            rail: Rail::of(&|v| (v * 0.875).round()),
        }
    }
}

impl Rail {
    fn of(p: &dyn Fn(f64) -> f64) -> Self {
        Self {
            words: p(13.0),
            line: p(13.0),
            time_h: p(113.0),
            cap_top: p(16.0),
            divider: p(16.0),
            divider_y: p(63.0),
            date_top: p(75.0),
            row: p(41.0),
            row_gap: p(8.0),
            header: p(37.0),
            circle: p(32.0),
            pitch: p(40.0),
            plate_inset: p(4.0),
            plate_pad: p(4.0),
            dot: p(4.0),
            dot_gap: p(2.0),
            dot_drop: p(4.0),
            square: p(48.0),
            art: p(32.0),
            pad: p(4.0),
            button: p(40.0),
            tight: p(2.0),
        }
    }
}

/// The hairline round a window's circle, the date box and a row's number:
/// Pond's inner outline, a tenth of the ink over the card.
pub fn outline(theme: &Theme) -> Rgba {
    let fg = theme.colors.get(ColorRole::Foreground);
    fg.with_alpha(fg.a * 0.14)
}

pub struct Sidebar {
    /// The cards from the top, and the status card at the foot.
    top: Ui,
    foot: Ui,
    pub map: minimap::Map,
    /// The cards the last draw kept, for `sidebar status`.
    cards: Vec<&'static str>,
    pub animating: bool,
    pub wake: Option<Instant>,
    /// The column's box in the window, as last drawn.
    strip: IRect,
    /// The control a press landed on, by its `on`.
    pressed: Option<crate::ui::On>,
    /// The clock's face over the time card.
    face: Vec<NodeId>,
    /// Notification app icons fitted at `Geo::glyph`, by entry id.
    icons: HashMap<NotificationId, Option<Bitmap>>,
    clock: ClockCard,
    /// Folded to the rail, as last drawn, and the rail clock's lines.
    collapsed: bool,
    rail_lines: Vec<String>,
    /// The media card's `motion_art` slot, what it asks for there, and
    /// whether the column is on screen to show it.
    motion_slot: u64,
    motion: Option<motion_art::Want>,
    seen: bool,
}

impl Drop for Sidebar {
    fn drop(&mut self) {
        motion_art::want(self.motion_slot, None);
    }
}

impl Sidebar {
    /// The field the cards' rims read and the bar window's top left on
    /// the output.
    pub fn set_lit(&mut self, lit: Option<(crate::paint::rim::Lit, (f64, f64))>) {
        self.top.lit = lit.clone();
        self.foot.lit = lit;
    }

    pub fn new() -> Self {
        Self {
            top: Ui::new(None),
            foot: Ui::new(None),
            map: minimap::Map::default(),
            cards: Vec::new(),
            animating: false,
            wake: None,
            strip: IRect::new(0, 0, 0, 0),
            pressed: None,
            face: Vec::new(),
            icons: HashMap::new(),
            clock: ClockCard::default(),
            collapsed: false,
            rail_lines: Vec::new(),
            motion_slot: motion_art::slot(),
            motion: None,
            seen: true,
        }
    }

    /// Off screen under a fullscreen window and back.
    pub fn visible(&mut self, on: bool) {
        if on != self.seen {
            self.seen = on;
            motion_art::want(self.motion_slot, self.motion.clone().filter(|_| self.seen));
        }
    }

    /// What the clock card's digits are frosted over; true when it moved.
    pub fn set_clock(&mut self, under: Option<Under>) -> bool {
        let same = match (&self.clock.under, &under) {
            (Some(a), Some(b)) => a.same(b),
            (None, None) => true,
            _ => false,
        };
        self.clock.under = under;
        !same
    }

    /// The clock card for `debug dump`.
    pub fn clock_state(&self, store: &Store) -> Value {
        self.clock.state(store)
    }

    /// The column inside `strip`, after `anchor` in paint order, its
    /// pictures made at `raster`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        store: &Store,
        output: &str,
        collapsed: bool,
        edge: Edge,
        strip: IRect,
        raster: f64,
        kit: &mut Kit,
        scene: &mut Scene,
        anchor: Option<NodeId>,
        now: Instant,
    ) {
        let theme = &store.theme.theme;
        let g = Geo::of();
        let pad = g.margin;
        let width = (strip.w as f64 - pad * 2.0).max(1.0);
        let (x0, y0) = (strip.x as f64 + pad, strip.y as f64 + pad);
        let bottom = (strip.y + strip.h) as f64 - pad;
        self.top.motion_scale = kit.motion_scale;
        self.foot.motion_scale = kit.motion_scale;
        self.collapsed = collapsed;
        self.map.rail = collapsed;
        self.map.read(store, output, raster);
        self.strip = strip;
        self.fit_icons(store, &g, raster);

        let foot = if collapsed { rail_foot(edge, &g) } else { foot_card(store, edge, &g) };
        let (_, foot_h) = crate::ui::measure(&foot, width, theme, kit);
        let gap = g.gap;
        let room = (bottom - foot_h - gap - y0).max(0.0);
        let art = if collapsed { g.rail.art } else { g.art };
        let (cover, motion) = store.media.active().map(|a| motion_art::chrome_cover(store, &a, art, raster)).unwrap_or_default();
        let build = |map: &minimap::Map, kit: &mut Kit| {
            if collapsed { rail(store, map, &g, &cover) } else { cards(theme, store, map, &g, &cover, &self.icons, width, kit, now) }
        };
        // Measured whole first: every row, from the top.
        let want = self.map.first;
        self.map.fits = usize::MAX;
        self.map.circles = usize::MAX;
        self.map.first = 0;
        let fits = |cards: &[(&str, El)], kit: &mut Kit| {
            let col = w::column(gap, cards.iter().map(|(_, c)| c.clone()).collect());
            crate::ui::measure(&col, width, theme, kit).1 <= room
        };
        let mut cards = build(&self.map, kit);
        for drop in DROPS {
            if fits(&cards, kit) {
                break;
            }
            cards.retain(|(name, _)| *name != drop);
        }
        // Time and Spaces alone still too tall: the minimap shows the
        // rows that fit and scrolls through the rest. Each pass measures
        // the rows it would show (`first_for`), so what settles once
        // after it is what was measured.
        // On the rail the current row's circles go first, down to one.
        let total = self.map.len();
        let mut n = total;
        let mut circles = if collapsed { self.map.current_windows() } else { 0 };
        while !fits(&cards, kit) && (n > 1 || circles > 1) {
            if circles > 1 {
                circles -= 1;
                self.map.circles = circles;
            } else {
                n -= 1;
                self.map.fits = n;
                self.map.first = self.map.first_for(want, n);
            }
            let names: Vec<&str> = cards.iter().map(|(name, _)| *name).collect();
            cards = build(&self.map, kit).into_iter().filter(|(name, _)| names.contains(name)).collect();
        }
        if n < total {
            self.map.first = want;
            self.map.keep_in_view();
        }
        if self.map.fits == usize::MAX {
            self.map.first = 0;
        }
        let kept: Vec<&'static str> = cards.iter().map(|(n, _)| *n).collect();
        let top = w::column(gap, cards.into_iter().map(|(_, c)| c).collect());
        self.cards = kept;
        self.motion = motion.filter(|_| self.cards.contains(&"media"));
        motion_art::want(self.motion_slot, self.motion.clone().filter(|_| self.seen));
        let clip = Some(strip);
        self.top.anchor = anchor;
        self.top.cursor = self.map.cursor_key();
        self.top.ring = self.top.cursor.is_some();
        let drawn = self.top.draw(&top, Rect::new(x0, y0, x0 + width, y0 + room), clip, 1.0, theme, kit, scene, now);
        self.foot.anchor = drawn.last.or(anchor);
        let at = bottom - foot_h;
        let feet = self.foot.draw(&foot, Rect::new(x0, at, x0 + width, bottom), clip, 1.0, theme, kit, scene, now);
        let face = self.paint_face(collapsed, store, &g, raster, kit, scene, feet.last.or(self.foot.anchor), strip);
        let card = (x0, width);
        let gliding = self.map.paint(&self.top.hits, theme, kit, scene, face, strip, card, edge == Edge::Right, now);
        self.animating = drawn.animating || feet.animating || gliding;
        self.wake = [drawn.wake, feet.wake, self.map.wake()].into_iter().flatten().min();
    }

    /// Fits each notification's picture at the row's icon size once, and
    /// lets go of the ones whose entries are gone.
    fn fit_icons(&mut self, store: &Store, g: &Geo, scale: f64) {
        let n = &store.notifications;
        self.icons.retain(|id, _| n.icons.contains_key(id));
        for (id, raw) in &n.icons {
            self.icons.entry(*id).or_insert_with(|| raw.bitmap(g.glyph as u32, scale));
        }
    }

    /// The clock's digits, its period and the date box over the time
    /// card's reserved box, or the rail's lines; the last node drawn.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn paint_face(&mut self, collapsed: bool, store: &Store, g: &Geo, raster: f64, kit: &mut Kit, scene: &mut Scene, anchor: Option<NodeId>, clip: IRect) -> Option<NodeId> {
        let card = self.top.hits.iter().find(|h| h.on.as_ref().and_then(|on| on.get::<Intent>()) == Some(&Intent::Panel(PanelId::Calendar))).map(|h| h.rect);
        let mut nodes = std::mem::take(&mut self.face);
        let mut p = Painter::new(scene, &mut nodes, Some(clip)).after(anchor);
        self.rail_lines.clear();
        match card {
            Some(card) if collapsed => self.rail_lines = rail_clock(&mut p, kit, store, g, card),
            Some(card) => {
                // The time keeps its own box; the battery's line runs under it.
                let line = store.power.battery.as_ref().map(|b| (b, card.h - g.time_h as i32));
                let face = IRect::new(card.x, card.y, card.w, card.h - line.map_or(0, |(_, h)| h));
                self.clock.paint(&mut p, kit, store, g, face, raster);
                if let Some((b, _)) = line {
                    battery_line(&mut p, &store.theme.theme, g, b, IRect::new(card.x, face.bottom(), card.w, card.h - face.h));
                }
            }
            None => {}
        }
        let last = p.last();
        p.finish();
        self.face = nodes;
        last
    }

    /// The boxes that take the pointer: every card and every control (the
    /// rail's buttons), so a press anywhere else in the column reaches
    /// what is under it.
    pub fn input_rects(&self) -> Vec<IRect> {
        let controls = self.top.hits.iter().chain(&self.foot.hits).filter(|h| h.on.is_some()).map(|h| h.rect);
        self.top.cards.iter().chain(&self.foot.cards).copied().chain(controls).filter(|r| !r.is_empty()).collect()
    }

    /// The clickable rect under a point, as an index past the top column's
    /// hits into the foot's.
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        let at = |ui: &Ui| {
            let (x, y) = (x.floor() as i32, y.floor() as i32);
            ui.hits.iter().rposition(|h| h.on.is_some() && x >= h.rect.x && x < h.rect.right() && y >= h.rect.y && y < h.rect.bottom())
        };
        at(&self.foot).map(|i| i + self.top.hits.len()).or_else(|| at(&self.top))
    }

    fn hit_at(&self, i: usize) -> Option<&crate::ui::Hit> {
        let n = self.top.hits.len();
        if i < n { self.top.hits.get(i) } else { self.foot.hits.get(i - n) }
    }

    /// Hit `i`'s tooltip and rect.
    pub fn tip(&self, i: usize) -> Option<(String, IRect)> {
        self.hit_at(i).and_then(|h| h.tip.clone().map(|t| (t, h.rect)))
    }

    /// The pointer over hit `i`; true when the column needs a pass.
    pub fn hover(&mut self, i: Option<usize>) -> bool {
        let hit = i.and_then(|i| self.hit_at(i));
        let path = hit.map(|h| h.path.clone());
        let over = hit.and_then(|h| h.on.as_ref()).is_some_and(minimap::on_map);
        let n = self.top.hits.len();
        let (top, foot) = match i {
            Some(i) if i >= n => (None, path),
            _ => (path, None),
        };
        let moved = top != self.top.hover || foot != self.foot.hover || over != self.map.over;
        self.top.hover = top;
        self.foot.hover = foot;
        self.map.over = over;
        moved
    }

    /// The centre of hit `i` along a vertical strip.
    pub fn anchor(&self, i: usize) -> f64 {
        self.hit_at(i).map_or(0.0, |h| h.rect.y as f64 + h.rect.h as f64 / 2.0)
    }

    /// Where the control opening `id` sits, for an open with no click.
    pub fn panel_anchor(&self, id: PanelId) -> Option<f64> {
        self.anchor_of(Intent::Panel(id))
    }

    /// Where the notifications card sits.
    pub fn center_anchor(&self) -> Option<f64> {
        self.anchor_of(Intent::Center)
    }

    /// Where the control carrying `intent` sits.
    pub fn anchor_of(&self, intent: Intent) -> Option<f64> {
        let n = self.top.hits.len() + self.foot.hits.len();
        (0..n).find(|i| self.hit_at(*i).and_then(|h| h.on.as_ref()?.get::<Intent>()) == Some(&intent)).map(|i| self.anchor(i))
    }

    /// A wheel over hit `i`: on the minimap it scrolls its rows.
    pub fn wheel(&mut self, i: usize, notches: f64) -> bool {
        let on_map = self.hit_at(i).and_then(|h| h.on.as_ref()).is_some_and(minimap::on_map);
        on_map && notches != 0.0 && self.map.scroll(notches.signum() as i32)
    }

    /// A press on hit `i`, which may start a drag on the minimap. The
    /// control is kept by what it does, since a redraw before the release
    /// can renumber the hits.
    pub fn press(&mut self, i: usize, at: (f64, f64)) {
        let on = self.hit_at(i).and_then(|h| h.on.clone());
        // Pond lifts no icon off its collapsed column.
        let lift = on.as_ref().filter(|_| !self.collapsed).and_then(|on| on.get::<minimap::Press>());
        self.map.press(lift, at);
        self.pressed = on;
    }

    /// The button let go: the drop's commands, or `None` for a click.
    pub fn release(&mut self, at: (f64, f64), now: Instant) -> Option<Vec<Command>> {
        self.map.release(at, self.strip, now)
    }

    /// A click let go at `(x, y)`: what the pressed control asks for, when
    /// the pointer is still on that control.
    pub fn clicked(&mut self, x: f64, y: f64) -> Action {
        let pressed = self.pressed.take();
        let here = self.hit(x, y).and_then(|i| self.hit_at(i)).and_then(|h| h.on.clone());
        match pressed.filter(|on| Some(on) == here.as_ref()) {
            Some(on) => action(&on, &self.map),
            None => Action::None,
        }
    }

    /// `sidebar status`'s answer; `hits` are every control's rect in the
    /// window's own coordinates.
    pub fn status(&self, collapsed: bool, edge: Edge, width: i32) -> Value {
        json!({
            "collapsed": collapsed,
            "edge": edge.as_str(),
            "width": width,
            "cards": self.cards,
            "rail": collapsed.then(|| json!({"time": self.rail_lines.get(..self.rail_lines.len().saturating_sub(2)), "date": self.rail_lines.get(self.rail_lines.len().saturating_sub(2)..)})),
            "input": self.input_rects().iter().map(|r| json!({"x": r.x, "y": r.y, "w": r.w, "h": r.h})).collect::<Vec<_>>(),
            "rows": self.map.status(),
            "cursor": self.map.cursor_status(),
            "minimap": self.map.drag_status(),
            "shown": {"first": self.map.first, "count": self.map.fits.min(self.map.len())},
            "hits": self
                .top
                .hits
                .iter()
                .chain(&self.foot.hits)
                .filter_map(|h| h.on.as_ref().map(|on| json!({"on": on, "x": h.rect.x, "y": h.rect.y, "w": h.rect.w, "h": h.rect.h})))
                .collect::<Vec<_>>(),
        })
    }
}

/// What a control of the column's own does. Its `on` key, which `sidebar
/// status` lists among its hits, is built and read here alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Panel(PanelId),
    /// The notification centre.
    Center,
    /// The Software card's Apply or Undo.
    Software(packages::Change),
    /// The column's chevron: the quick settings card the top bar's chevron
    /// opens, the right region's cells it has no card for as info tiles.
    Overflow,
    /// Folded to the rail (true) or open again.
    Collapse(bool),
}

impl crate::ui::on::HitId for Intent {}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Intent::Panel(id) => write!(f, "panel:{id}"),
            Intent::Center => write!(f, "panel:{}", center::ID),
            Intent::Software(packages::Change::Apply) => f.write_str("software:apply"),
            Intent::Software(packages::Change::Undo) => f.write_str("software:undo"),
            Intent::Overflow => f.write_str("overflow"),
            Intent::Collapse(true) => f.write_str("collapse"),
            Intent::Collapse(false) => f.write_str("expand"),
        }
    }
}

impl Intent {
    /// Runs it: what the bar opens, or nothing once the column did it.
    fn run(self) -> Action {
        match self {
            Intent::Panel(id) => Action::Panel(id),
            Intent::Center => Action::Center,
            Intent::Software(change) => {
                packages::send(packages::Cmd::Change(change));
                Action::None
            }
            Intent::Overflow => Action::Overflow(fs_types::Region::Right),
            Intent::Collapse(collapsed) => {
                set_collapsed(collapsed);
                Action::None
            }
        }
    }
}

/// What a click on `on` asks for: the column's own, an Activity row's (the
/// panel holds its actions), or the minimap's.
fn action(on: &crate::ui::On, map: &minimap::Map) -> Action {
    if let Some(&intent) = on.get::<Intent>() {
        return intent.run();
    }
    if let Some(press) = on.get::<minimap::Press>() {
        return map.action(press);
    }
    match ActivityVerb::parse(on) {
        Some((ActivityVerb::Open, _)) => Action::Panel(PanelId::Activity),
        _ => Action::None,
    }
}

/// Collapse state lives in state.json, never in settings.json.
pub fn set_collapsed(collapsed: bool) {
    state::set(vec![Field::SidebarCollapsed(collapsed)]);
}

/// The time the card's digits read and the period after them, on the
/// same format as the bar's clock: its hour (padded or not) with its
/// minutes, and its AM/PM where it carries one. Its date is the box's.
fn time_parts(store: &Store) -> (String, Option<String>) {
    let format = clock_format(store);
    // Quoted runs are literal text, never a marker.
    let bare: String = format.split('\'').step_by(2).collect();
    let hour = if bare.contains("hh") || bare.contains("HH") { "hh" } else if bare.contains('h') || bare.contains('H') { "h" } else { "hh" };
    let period = if bare.contains('A') { Some("AP") } else if bare.contains('a') { Some("ap") } else { None };
    let clock = store.clock.now;
    match period {
        Some(ap) => {
            let both = format_qt(clock, &format!("{hour}:mm {ap}"));
            match both.split_once(' ') {
                Some((time, ap)) => (time.to_owned(), Some(ap.to_owned())),
                None => (both, None),
            }
        }
        None => (format_qt(clock, &format!("{hour}:mm")), None),
    }
}

fn style(kit: &Kit, size: f64, weight: f32, tracking: f32) -> TextStyle {
    TextStyle { family: kit.look.sans, size: size as f32, weight, tracking, wdth: TextStyle::NORMAL_WIDTH, wght: None }
}

/// Pond's clock card: the digits with the period on their baseline, and
/// the weekday over the day in an outlined box at the far end.
struct ClockCard {
    under: Option<Under>,
    glass: Glass,
    face: Option<fs_chrome::clock_face::Face>,
    /// The digits' box, their ink, the time and period they read, and what
    /// the draw that first read that time cost.
    rect: IRect,
    ink: Rgba,
    time: String,
    period: Option<String>,
    minute_us: u64,
}

impl Default for ClockCard {
    fn default() -> Self {
        Self { under: None, glass: Glass::default(), face: None, rect: IRect::default(), ink: Rgba::TRANSPARENT, time: String::new(), period: None, minute_us: 0 }
    }
}

/// The share of the type size Pond's digits' figures stand.
const FIGURE: f64 = 0.7;

/// Pond's battery line under the clock: a trough the
/// digits' width in the foreground at a fifth, filled to the level, the
/// fill in the foreground, `primary` while charging and `destructive` low
/// on battery, centred in `band`.
fn battery_line(p: &mut Painter, theme: &Theme, g: &Geo, b: &crate::services::devices::power::Battery, band: IRect) {
    use fs_upower::DeviceState;
    let fg = theme.colors.get(ColorRole::Foreground);
    let charging = matches!(b.state, DeviceState::Charging | DeviceState::FullyCharged | DeviceState::PendingCharge);
    let low = !charging && fs_system::power::model::Thresholds::LAPTOP.low(b.level).is_some();
    let fill = if charging {
        theme.colors.get(ColorRole::Primary)
    } else if low {
        theme.colors.get(ColorRole::Destructive)
    } else {
        fg.with_alpha(fg.a * 0.7)
    };
    let (x, w) = (band.x + g.time_x as i32, band.w - g.time_x as i32 * 2);
    let h = g.battery_line.max(1.0) as i32;
    let y = band.y + (band.h - h) / 2;
    let r = h as f32 / 2.0;
    p.rect(IRect::new(x, y, w, h), fg.with_alpha(fg.a * 0.2), r);
    let filled = (w as f64 * b.level.as_fraction()).round() as i32;
    if filled > 0 {
        p.rect(IRect::new(x, y, filled, h), fill, r);
    }
}

impl ClockCard {
    fn paint(&mut self, p: &mut Painter, kit: &mut Kit, store: &Store, g: &Geo, card: IRect, scale: f64) {
        let started = Instant::now();
        let theme = &store.theme.theme;
        let fg = theme.colors.get(ColorRole::Foreground);
        let clock = store.clock.now;
        let b = g.date_box as i32;
        let (bx, by) = (card.right() - g.date_inset as i32 - b, card.y + (card.h - b) / 2);
        p.framed(IRect::new(bx, by, b, b), Rgba::TRANSPARENT, g.date_radius as f32, outline(theme), theme.border_width as f32);
        for (words, line) in [(format_qt(clock, "ddd"), g.date_lines.0), (format_qt(clock, "d"), g.date_lines.1)] {
            let t = kit.shape(&words, style(kit, g.date_words, 400.0, 0.0));
            p.text(&t, (bx + (b - t.width) / 2, by + line as i32 - t.ascent.round() as i32), fg, &[]);
        }
        let (time, period) = time_parts(store);
        let shaped = period.as_ref().map(|ap| kit.shape(ap, style(kit, g.period, 500.0, 0.0)));
        let after = shaped.as_ref().map_or(0.0, |t| f64::from(t.width) + g.dot);
        let room = f64::from(bx - (card.x + g.time_x as i32)) - g.dot;
        let face = FaceSettings::read(&store.config);
        let (hours, minutes) = time.split_once(':').unwrap_or((&time, ""));
        // A time wider than the room shrinks rather than cut.
        let mut digit_h = (g.digits * FIGURE).round();
        let mut digits = Digits::lay(kit, &face, hours, minutes, digit_h, scale);
        while digits.width() + after > room && digit_h > g.date_words * FIGURE {
            digit_h -= 1.0;
            digits = Digits::lay(kit, &face, hours, minutes, digit_h, scale);
        }
        let digits_w = digits.width();
        let top = (f64::from(card.y) + (f64::from(card.h) - digit_h) / 2.0).round();
        let x = f64::from(card.x) + g.time_x;
        // `auto` reads what is behind the digits, which here is the card.
        let ink = face.ink(store).unwrap_or_else(|| glass::auto_ink(theme.box_style(Role::Card, None).fill.luma601()));
        self.rect = self.glass.paint(p, digits, &face, (x, top), digit_h, scale, ink, self.under.as_ref(), card);
        if let Some(t) = shaped {
            let baseline = (top + digit_h) as i32;
            let x = (x + digits_w).ceil() as i32 + g.dot as i32 - 1;
            p.text(&t, (x, baseline - t.ascent.round() as i32), fg.with_alpha(fg.a * 0.7), &[]);
        }
        self.face = Some(face.face);
        self.ink = ink;
        self.period = period;
        if time != self.time {
            self.time = time;
            self.minute_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        }
    }

    fn state(&self, store: &Store) -> Value {
        let r = self.rect;
        let format = clock_format(store);
        json!({
            "face": self.face.map_or("", |f| f.as_str()),
            "rect": [r.x, r.y, r.w, r.h],
            "ink": self.ink.to_hex(),
            "frosted": self.glass.frosted(),
            "depth": self.under.as_ref().is_some_and(|u| u.subject.is_some()),
            "cut": self.glass.cut(),
            "text": self.time,
            "period": self.period,
            "format": format,
            "formatted": format_qt(store.clock.now, &format),
            "minuteUs": self.minute_us,
            "battery": store.power.battery.as_ref().map(|b| b.level.as_percent()),
        })
    }
}

/// Every card that has something to show, top to bottom, by name.
#[allow(clippy::too_many_arguments)]
fn cards(theme: &Theme, store: &Store, map: &minimap::Map, g: &Geo, cover: &Option<Bitmap>, icons: &HashMap<NotificationId, Option<Bitmap>>, width: f64, kit: &mut Kit, now: Instant) -> Vec<(&'static str, El)> {
    let s = &theme.space;
    let inner = |h: f64| (h - s.md * 2.0).max(0.0);
    let battery = if store.power.battery.is_some() { g.battery_h } else { 0.0 };
    let mut out = vec![(
        "time",
        w::action_card(w::column(0.0, vec![w::space(inner(g.time_h) + battery)]), Intent::Panel(PanelId::Calendar)).fill(),
    )];
    out.push(("spaces", w::card(map.view(theme, now)).fill()));
    if let Some(media) = media(store, g, cover) {
        out.push(("media", w::action_card(media.height(inner(g.media_h)), Intent::Panel(PanelId::Media)).fill()));
    }
    let row = notifications(theme, store, g, icons, width - g.row_pad * 2.0, kit);
    out.push(("notifications", w::action_card(row.height(inner(g.row_h)).pad_start(g.row_pad - s.md), Intent::Center).fill()));
    if let Some(card) = activity::installs_card(store) {
        out.push(("activity", card));
    }
    let software = software_rows(theme, store);
    if !software.is_empty() {
        out.push(("software", w::card(w::column(s.row_gap, software)).fill()));
    }
    out
}

/// The Software card's rows: the upstream's commits the spec
/// checkout lacks, with Apply, and the newest app change, with Undo. Each
/// is a label over a caption so the words keep the sidebar's narrow width.
/// The packages service checks the checkout while the column is open
/// (`wayland/sidebar.rs`).
fn software_rows(theme: &Theme, store: &Store) -> Vec<El> {
    use crate::services::packages::{Change, NO_SPEC, Verb};
    use crate::ui::Variant;
    use fs_activity::{Origin, State as Run};
    let s = &theme.space;
    let row = |icon: &str, words: Vec<El>, button: Option<El>| {
        let mut r = vec![w::icon(icon).ink(Ink::Muted), w::column(0.0, words).fill()];
        r.extend(button);
        w::row(s.icon_gap, r)
    };
    let dim = |text: String| w::para(text, Type::Caption, Weight::Normal, Ink::Muted, 3);
    let button = |glyph: &str, tip: &str, on: Intent| w::icon_button(glyph).variant(Variant::Ghost).tip(tip).on(on);
    if !store.activity.fspec.installed {
        return vec![row("refresh-cw", vec![dim("Updates need fspec".into())], None)];
    }
    let sw = &store.packages.software;
    // No spec is nothing to track rather than a failure.
    if matches!(&sw.behind, Some(Err(why)) if why == NO_SPEC) {
        return Vec::new();
    }
    let retry = !sw.applying && matches!(sw.failed, Some((Change::Apply, _)));
    let updates = match &sw.behind {
        None => row("refresh-cw", vec![dim("Checking for updates\u{2026}".into())], None),
        Some(Err(why)) => row("refresh-cw", vec![dim(why.clone())], None),
        // The checkout already fast-forwarded, so only the apply failed:
        // the row keeps its retry.
        Some(Ok((0, _))) if retry => row("refresh-cw", vec![w::label("Failed").elide(), dim("Not applied".into())], Some(button("rotate-ccw", "Apply again", Intent::Software(Change::Apply)))),
        Some(Ok((0, _))) => row("refresh-cw", vec![dim("Up to date".into())], None),
        Some(Ok((n, upstream))) => {
            let label = if *n == 1 { "1 update".to_owned() } else { format!("{n} updates") };
            let (caption, apply) = if sw.applying { ("Applying\u{2026}".to_owned(), None) } else { (upstream.clone(), Some(button("download", "Apply", Intent::Software(Change::Apply)))) };
            row("refresh-cw", vec![w::label(label).elide(), dim(caption)], apply)
        }
    };
    let mut out = vec![updates];
    if let Some((verb, name)) = &sw.last {
        let app = store.menu.nodes().get(&format!("apps.{name}")).map_or(name.as_str(), |n| n.label.as_str());
        let (icon, words) = match verb {
            Verb::Install => ("package-plus", "Installed"),
            Verb::Uninstall => ("package-minus", "Removed"),
        };
        let undoing = store.activity.activity.live().iter().any(|i| i.origin == Origin::Fspec && i.kind == "undo" && i.state == Run::Running);
        let undo = (!undoing).then(|| button("undo-2", "Undo", Intent::Software(Change::Undo)));
        out.push(row(icon, vec![w::label(words), dim(app.to_owned())], undo));
    }
    if let Some((_, why)) = &sw.failed {
        out.push(w::para(why.clone(), Type::Caption, Weight::Normal, Ink::Destructive, 3));
    }
    out
}

/// The bell and how many are new (or how many there are), a rule, and the
/// apps the latest came from, as many as fit `room`.
fn notifications(theme: &Theme, store: &Store, g: &Geo, icons: &HashMap<NotificationId, Option<Bitmap>>, room: f64, kit: &mut Kit) -> El {
    let s = &theme.space;
    let m = &store.notifications.model;
    let bell = w::icon(if m.dnd { "bell-off" } else { "bell" }).size(Type::Caption).ink(Ink::Muted);
    let mut all: Vec<&fs_info::notifications::Entry> = m.popups.iter().chain(&m.pending).chain(&m.past).collect();
    if all.is_empty() {
        return w::row(s.icon_gap, vec![bell, w::caption("Notifications").ink(Ink::Muted).elide()]);
    }
    // A toast still up is as new as one filed unseen.
    let count = m.badge().to_string();
    all.sort_by_key(|e| std::cmp::Reverse(e.arrived_at));
    let mut apps: Vec<(&str, Bitmap)> = Vec::new();
    for e in all {
        if let Some(Some(b)) = icons.get(&e.id)
            && !apps.iter().any(|(app, _)| *app == e.app_name)
        {
            apps.push((&e.app_name, b.clone()));
        }
    }
    let shaped = kit.shape(&count, style(kit, theme.font_size.caption, 500.0, 0.0));
    let head = theme.font_size.caption + s.xxs + shaped.width as f64;
    let fit = ((room - head - g.spacing * 2.0 - theme.border_width + g.spacing) / (g.glyph + g.spacing)).floor().max(0.0) as usize;
    let mut row = vec![w::row(s.xxs, vec![bell, w::caption(count).weight(Weight::Medium).ink(Ink::Muted)])];
    if fit > 0 && !apps.is_empty() {
        row.push(w::swatch(theme.colors.get(ColorRole::Border), theme.border_width, g.glyph, 0.0));
        for (i, (_, b)) in apps.into_iter().take(fit).enumerate() {
            let pic = w::picture(Some(b), g.glyph);
            row.push(if i == 0 { pic } else { pic.fade(0.4) });
        }
    }
    w::row(g.spacing, row)
}

/// The art, the title over the artist.
fn media(store: &Store, g: &Geo, cover: &Option<Bitmap>) -> Option<El> {
    let a = store.media.active()?;
    let words = w::column(
        0.0,
        vec![
            w::label(a.title.clone()).size(Type::BodySmall).weight(Weight::Semibold).elide(),
            w::caption(a.artist.clone()).ink(Ink::Muted).elide(),
        ],
    )
    .fill();
    Some(w::row(g.spacing, vec![w::cover(cover.clone(), g.art), words]))
}

/// The status card: network, the chevron opening quick settings, settings,
/// the tray's second bar while it has items, and collapse, sharing the
/// card's width.
fn foot_card(store: &Store, edge: Edge, g: &Geo) -> El {
    let theme = &store.theme.theme;
    let tall = (g.row_h - theme.space.md * 2.0).max(0.0);
    let button = |glyph: &str, tip: &str, on: Intent| {
        w::cell(w::column(0.0, vec![w::icon(glyph).size(Type::Caption).ink(Ink::Muted).centred()]))
            .ghost()
            .cell_state(|c| c.tight = true)
            .interactive()
            .tip(tip)
            .on(on)
            .height(tall)
            .fill()
    };
    let mut row = vec![
        button("wifi", "Network", Intent::Panel(PanelId::Network)),
        button(chevron_glyph(edge), "Quick settings", Intent::Overflow),
        button("settings", "Settings", Intent::Panel(PanelId::Settings)),
    ];
    if !store.tray.items.is_empty() {
        row.push(button("ellipsis", "Tray", Intent::Panel(PanelId::TrayOverflow)));
    }
    row.push(button(fold_glyph(edge, false), "Collapse", Intent::Collapse(true)));
    w::card(w::row(0.0, row).fill()).fill()
}

/// The chevron pointing out of the column, where quick settings opens.
fn chevron_glyph(edge: Edge) -> &'static str {
    if edge == Edge::Right { "chevron-left" } else { "chevron-right" }
}

fn fold_glyph(edge: Edge, collapsed: bool) -> &'static str {
    match (collapsed, edge == Edge::Right) {
        (false, false) => "panel-left-close",
        (false, true) => "panel-right-close",
        (true, false) => "panel-left-open",
        (true, true) => "panel-right-open",
    }
}

/// The rail's foot, Pond's collapsed controls: network, the chevron,
/// settings and expand, stacked.
fn rail_foot(edge: Edge, g: &Geo) -> El {
    let r = &g.rail;
    let button = |glyph: &str, tip: &str, on: Intent| {
        w::cell(w::column(0.0, vec![w::icon(glyph).size(Type::Caption).ink(Ink::Muted).centred()]).fill())
            .ghost()
            .cell_state(|c| c.inset = Some((0.0, 0.0)))
            .interactive()
            .tip(tip)
            .on(on)
            .height(r.button)
            .fill()
    };
    let buttons = vec![
        button("wifi", "Network", Intent::Panel(PanelId::Network)),
        button(chevron_glyph(edge), "Quick settings", Intent::Overflow),
        button("settings", "Settings", Intent::Panel(PanelId::Settings)),
        button(fold_glyph(edge, true), "Expand", Intent::Collapse(false)),
    ];
    w::card(w::column(0.0, buttons).fill()).cell_state(|c| c.inset = Some((r.pad, r.pad))).fill()
}

/// The rail's cards, Pond's collapsed column: the time card (painted over
/// by `rail_clock`), the workspace rows, the art while a player plays, the
/// bell, and a ring per running group.
fn rail(store: &Store, map: &minimap::Map, g: &Geo, cover: &Option<Bitmap>) -> Vec<(&'static str, El)> {
    let r = &g.rail;
    let bare = |c: &mut crate::ui::el::CellState| c.inset = Some((0.0, 0.0));
    let square = |child: El| w::cell(w::column(0.0, vec![child.centred()]).fill()).ghost().cell_state(bare).height(r.square).fill();
    let mut out = vec![("time", w::action_card(w::space(r.time_h), Intent::Panel(PanelId::Calendar)).cell_state(bare).fill())];
    out.push(("spaces", w::card(map.rail_view()).cell_state(bare).fill()));
    if store.media.active().is_some() {
        out.push(("media", w::action_card(square(w::cover(cover.clone(), r.art)), Intent::Panel(PanelId::Media)).cell_state(bare).fill()));
    }
    let m = &store.notifications.model;
    let bell = w::icon(if m.dnd { "bell-off" } else { "bell" }).size(Type::Caption).ink(Ink::Muted);
    // A toast still up is as new as one filed unseen.
    let bell = match m.badge() {
        0 => bell,
        n => w::row(r.tight, vec![bell, w::caption(n.to_string()).mono().weight(Weight::Medium).ink(Ink::Muted)]),
    };
    out.push(("notifications", w::action_card(square(bell), Intent::Center).cell_state(bare).fill()));
    let groups = store.activity.activity.groups();
    if !groups.is_empty() {
        let rows = groups
            .iter()
            .take(3)
            .map(|grp| {
                let lead = grp.lead();
                w::cell(w::column(0.0, vec![w::indicator(activity::ring(lead)).centred()]).fill())
                    .ghost()
                    .cell_state(bare)
                    .interactive()
                    .tip(lead.subject().to_owned())
                    .on(ActivityVerb::Open.on(grp.source))
                    .height(r.button)
                    .fill()
            })
            .collect();
        out.push(("activity", w::action_card(w::column(0.0, rows).fill(), Intent::Panel(PanelId::Activity)).cell_state(|c| c.inset = Some((r.pad, r.pad))).fill()));
    }
    out
}

/// The rail clock's lines, Pond's: the two-digit hour, the minutes, and
/// the period at 0.3 where the format is 12 hour; then the weekday's first
/// two letters at 0.5 and the two-digit day.
fn rail_lines(store: &Store) -> [Vec<Line>; 2] {
    let format = clock_format(store);
    // Quoted runs are literal text, never a marker.
    let bare: String = format.split('\'').step_by(2).collect();
    let period = if bare.contains('A') { Some("AP") } else if bare.contains('a') { Some("ap") } else { None };
    let clock = store.clock.now;
    let hour = match period {
        Some(_) => format_qt(clock, "hh AP").split(' ').next().unwrap_or_default().to_owned(),
        None => format_qt(clock, "HH"),
    };
    let mut time = vec![(hour, 1.0), (format_qt(clock, "mm"), 1.0)];
    if let Some(ap) = period {
        time.push((format_qt(clock, ap), 0.3));
    }
    let day = format_qt(clock, "ddd").chars().take(2).collect::<String>().to_uppercase();
    [time, vec![(day, 0.5), (format_qt(clock, "dd"), 1.0)]]
}

/// A rail clock line and its opacity.
type Line = (String, f32);

/// The rail clock over the time card's box; the lines it drew.
fn rail_clock(p: &mut Painter, kit: &mut Kit, store: &Store, g: &Geo, card: IRect) -> Vec<String> {
    let theme = &store.theme.theme;
    let r = &g.rail;
    let fg = theme.colors.get(ColorRole::Foreground);
    let style = TextStyle { family: kit.look.mono, size: r.words as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
    let cap = cap_height(r.words);
    let [time, date] = rail_lines(store);
    let mut drawn = Vec::new();
    for (lines, top) in [(&time, r.cap_top), (&date, r.date_top)] {
        for (i, (words, alpha)) in lines.iter().enumerate() {
            let t = kit.shape(words, style);
            let baseline = card.y as f64 + top + i as f64 * r.line + cap;
            p.text(&t, (card.x + (card.w - t.width) / 2, (baseline - f64::from(t.ascent)).round() as i32), fg.with_alpha(fg.a * alpha), &[]);
            drawn.push(words.clone());
        }
    }
    let d = r.divider as i32;
    p.rect(IRect::new(card.x + (card.w - d) / 2, card.y + r.divider_y as i32, d, theme.border_width.max(1.0) as i32), outline(theme), 0.0);
    drawn
}

/// A face's cap height at `size`: the mono faces we ask for stand theirs
/// at about 0.7 of the size.
pub fn cap_height(size: f64) -> f64 {
    (size * 0.7).round()
}
