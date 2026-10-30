//! The sidebar (docs/superpowers/specs/2026-10-30-sidebar.md): the bar's
//! body under `layout: sidebar`. The bar keeps its window, its edge, its
//! zone and its joins; instead of cells it draws a column of cards through
//! the ui kit, the status row pinned to its foot, or the rail while
//! collapsed. Every input lands as a cell [`Action`], so the bar's own
//! routing opens what a cell would.

pub mod minimap;

use std::time::Instant;

use fs_chrome::types::Edge;
use fs_info::clock::{CLOCK_FORMATS, format_qt, substitute_iso_week};
use fs_theme::theme::Theme;
use serde_json::{Value, json};
use vello_cpu::kurbo::Rect;

use crate::scene::{IRect, NodeId, Scene};
use crate::services::compositor::Command;
use crate::services::state::{self, Field};
use crate::store::Store;
use crate::surfaces::activity;
use crate::surfaces::bar::cell::{Action, Kit};
use crate::ui::{El, Ink, Type, Ui, Variant, Weight, w};

/// The cards in the order they give up their place when the column runs
/// short: the first is the first to go.
const DROPS: [&str; 3] = ["media", "notifications", "activity"];

pub struct Sidebar {
    /// The cards from the top, and the status row at the foot.
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
    pressed: Option<String>,
}

impl Sidebar {
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
        }
    }

    /// The column inside `strip`, after `anchor` in paint order.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        store: &Store,
        output: &str,
        collapsed: bool,
        edge: Edge,
        strip: IRect,
        kit: &mut Kit,
        scene: &mut Scene,
        anchor: Option<NodeId>,
        now: Instant,
    ) {
        let theme = &store.theme.theme;
        let s = &theme.space;
        let pad = if collapsed { s.xs } else { s.screen_padding };
        let width = (strip.w as f64 - pad * 2.0).max(1.0);
        let (x0, y0) = (strip.x as f64 + pad, strip.y as f64 + pad);
        let bottom = (strip.y + strip.h) as f64 - pad;
        self.top.motion_scale = kit.motion_scale;
        self.foot.motion_scale = kit.motion_scale;
        self.map.read(store, output, theme);
        self.strip = strip;

        let foot = if collapsed { rail_foot(theme, edge) } else { foot_row(theme, edge) };
        let (_, foot_h) = crate::ui::measure(&foot, width, theme, kit);
        let room = (bottom - foot_h - s.section_gap - y0).max(0.0);
        let mut kept: Vec<&'static str> = Vec::new();
        let top = if collapsed {
            rail(theme, store, &self.map)
        } else {
            self.map.fits = usize::MAX;
            let label = crate::ui::measure(&w::caption(self.map.widest_label()).mono(), width, theme, kit).0;
            self.map.across = minimap::across(theme, width, label);
            let fits = |cards: &[(&str, El)], kit: &mut Kit| {
                let col = w::column(s.section_gap, cards.iter().map(|(_, c)| c.clone()).collect());
                crate::ui::measure(&col, width, theme, kit).1 <= room
            };
            let mut cards = cards(theme, store, &self.map, now);
            for drop in DROPS {
                if fits(&cards, kit) {
                    break;
                }
                cards.retain(|(name, _)| *name != drop);
            }
            // Time and Spaces alone still too tall: the minimap shows the
            // rows that fit and scrolls through the rest.
            let mut n = self.map.len();
            while !fits(&cards, kit) && n > 1 {
                n -= 1;
                self.map.fits = n;
                self.map.keep_in_view();
                let names: Vec<&str> = cards.iter().map(|(name, _)| *name).collect();
                cards = self::cards(theme, store, &self.map, now).into_iter().filter(|(name, _)| names.contains(name)).collect();
            }
            if self.map.fits == usize::MAX {
                self.map.first = 0;
            }
            kept = cards.iter().map(|(n, _)| *n).collect();
            w::column(s.section_gap, cards.into_iter().map(|(_, c)| c).collect())
        };
        self.cards = kept;
        let clip = Some(strip);
        self.top.anchor = anchor;
        self.top.cursor = self.map.cursor_key();
        self.top.ring = self.top.cursor.is_some();
        let drawn = self.top.draw(&top, Rect::new(x0, y0, x0 + width, y0 + room), clip, 1.0, theme, kit, scene, now);
        self.foot.anchor = drawn.last.or(anchor);
        let at = bottom - foot_h;
        let feet = self.foot.draw(&foot, Rect::new(x0, at, x0 + width, bottom), clip, 1.0, theme, kit, scene, now);
        let hits = if collapsed { &[][..] } else { &self.top.hits[..] };
        let gliding = self.map.paint(hits, theme, kit, scene, feet.last.or(self.foot.anchor), strip, now);
        self.animating = drawn.animating || feet.animating || gliding;
        self.wake = [drawn.wake, feet.wake, self.map.wake()].into_iter().flatten().min();
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
        let path = i.and_then(|i| self.hit_at(i)).map(|h| h.path.clone());
        let n = self.top.hits.len();
        let (top, foot) = match i {
            Some(i) if i >= n => (None, path),
            _ => (path, None),
        };
        let moved = top != self.top.hover || foot != self.foot.hover;
        self.top.hover = top;
        self.foot.hover = foot;
        moved
    }

    /// The centre of hit `i` along a vertical strip.
    pub fn anchor(&self, i: usize) -> f64 {
        self.hit_at(i).map_or(0.0, |h| h.rect.y as f64 + h.rect.h as f64 / 2.0)
    }

    /// Where the control opening `name` sits, for an open with no click.
    pub fn panel_anchor(&self, name: &str) -> Option<f64> {
        let on = format!("panel:{name}");
        let n = self.top.hits.len() + self.foot.hits.len();
        (0..n).find(|i| self.hit_at(*i).is_some_and(|h| h.on.as_deref() == Some(on.as_str()))).map(|i| self.anchor(i))
    }

    /// A wheel over hit `i`: on the minimap it scrolls its rows.
    pub fn wheel(&mut self, i: usize, notches: f64) -> bool {
        let on_map = self.hit_at(i).and_then(|h| h.on.as_deref()).is_some_and(|on| on == "new" || on.starts_with("row:") || on.starts_with("window:"));
        on_map && notches != 0.0 && self.map.scroll(notches.signum() as i32)
    }

    /// A press on hit `i`, which may start a drag on the minimap. The
    /// control is kept by what it does, since a redraw before the release
    /// can renumber the hits.
    pub fn press(&mut self, i: usize, at: (f64, f64)) {
        let on = self.hit_at(i).and_then(|h| h.on.clone());
        self.map.press(on.as_deref().unwrap_or_default(), at);
        self.pressed = on;
    }

    /// The button let go: the drop's commands, or `None` for a click.
    pub fn release(&mut self, at: (f64, f64), now: Instant) -> Option<Vec<Command>> {
        self.map.release(at, self.strip, now)
    }

    /// A click let go at `(x, y)`: what the pressed control asks for, when
    /// the pointer is still on that control.
    pub fn clicked(&mut self, x: f64, y: f64, output: &str) -> Action {
        let pressed = self.pressed.take();
        let here = self.hit(x, y).and_then(|i| self.hit_at(i)).and_then(|h| h.on.clone());
        match pressed.filter(|on| Some(on) == here.as_ref()) {
            Some(on) => action(&on, output, &self.map),
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

/// What a click on `on` asks for.
fn action(on: &str, output: &str, map: &minimap::Map) -> Action {
    let (verb, arg) = on.split_once(':').unwrap_or((on, ""));
    match verb {
        "panel" => match arg {
            "calendar" => Action::Panel("calendar"),
            "network" => Action::Panel("network"),
            "audio" => Action::Panel("audio"),
            "media" => Action::Panel("media"),
            "activity" => Action::Panel("activity"),
            _ => Action::None,
        },
        // An Activity row: the panel holds its actions.
        "open" => Action::Panel("activity"),
        "center" => Action::Center,
        "summon" => Action::Summon("system"),
        "media" => Action::MediaPlayPause,
        "collapse" => Action::Sidebar(true),
        "expand" => Action::Sidebar(false),
        _ => map.action(on, output),
    }
}

/// Collapse state lives in state.json, never in settings.json.
pub fn set_collapsed(collapsed: bool) {
    state::set(vec![Field::SidebarCollapsed(collapsed)]);
}

fn clock_format(store: &Store) -> String {
    let saved = &store.state.data.clock_format;
    if saved.is_empty() { CLOCK_FORMATS[0].to_owned() } else { saved.clone() }
}

/// Every card that has something to show, top to bottom, by name.
fn cards(theme: &Theme, store: &Store, map: &minimap::Map, now: Instant) -> Vec<(&'static str, El)> {
    let s = &theme.space;
    let clock = store.clock.now;
    let time = format_qt(clock, &substitute_iso_week(&clock_format(store), clock.date()));
    let mut out = vec![(
        "time",
        w::card(w::column(
            s.xxs,
            vec![
                w::para(time, Type::Title, Weight::Semibold, Ink::Fg, 1).mono(),
                w::caption(format_qt(clock, "dddd d MMMM")).ink(Ink::Muted).elide(),
            ],
        ))
        .interactive()
        .on("panel:calendar")
        .fill(),
    )];
    out.push(("spaces", w::card(map.view(theme, now)).fill()));
    let rows = activity::list(theme, store, 3);
    if !rows.is_empty() {
        out.push(("activity", w::card(w::column(s.row_gap, rows)).fill()));
    }
    out.push(("notifications", w::card(notifications(theme, store)).interactive().on("center").fill()));
    if let Some(media) = media(theme, store) {
        out.push(("media", w::card(media).fill()));
    }
    out
}

fn notifications(theme: &Theme, store: &Store) -> El {
    let s = &theme.space;
    let m = &store.notifications.model;
    let icon = if m.dnd { "bell-off" } else { "bell" };
    // A toast still up is as new as one filed unseen.
    let new = m.popups.len() + m.pending.len();
    let Some(newest) = m.popups.iter().chain(&m.pending).chain(&m.past).max_by_key(|e| e.arrived_at) else {
        return w::row(s.icon_gap, vec![w::icon(icon).ink(Ink::Muted), w::caption("No notifications").ink(Ink::Muted).fill()]);
    };
    let count = if new == 0 { format!("{} seen", m.past.len()) } else { format!("{new} new") };
    let head = w::row(s.icon_gap, vec![w::icon(icon), w::label(count).fill()]);
    w::column(s.xxs, vec![head, w::caption(newest.summary.clone()).ink(Ink::Muted).elide()])
}

fn media(theme: &Theme, store: &Store) -> Option<El> {
    let s = &theme.space;
    let a = store.media.active()?;
    let size = s.control_height;
    let (px, radius) = w::cover_inner(theme, size);
    let cover = (!a.art_url.is_empty()).then(|| store.media.cover(&a.art_url, px, radius, crate::scene::raster_scale())).flatten();
    let words = w::column(
        0.0,
        vec![w::label(a.title.clone()).elide(), w::caption(a.artist.clone()).ink(Ink::Muted).elide()],
    )
    .fill();
    let play = w::icon_button(if a.playing { "pause" } else { "play" }).variant(Variant::Ghost).on("media:toggle");
    let row = w::row(s.sm, vec![w::cover(cover, size).on("panel:media"), words, play]);
    Some(row)
}

/// The status row: network, audio, settings and collapse.
fn foot_row(theme: &Theme, edge: Edge) -> El {
    let s = &theme.space;
    let mut b = foot_buttons(edge, false);
    b.insert(3, w::spacer());
    w::row(s.xs, b).fill()
}

fn foot_buttons(edge: Edge, collapsed: bool) -> Vec<El> {
    let right = edge == Edge::Right;
    let fold = match (collapsed, right) {
        (false, false) => "panel-left-close",
        (false, true) => "panel-right-close",
        (true, false) => "panel-left-open",
        (true, true) => "panel-right-open",
    };
    let button = |glyph: &str, tip: &str, on: &str| w::icon_button(glyph).variant(Variant::Ghost).tip(tip).on(on);
    vec![
        button("wifi", "Network", "panel:network"),
        button("volume-2", "Audio", "panel:audio"),
        button("settings", "System", "summon:system"),
        button(fold, if collapsed { "Expand" } else { "Collapse" }, if collapsed { "expand" } else { "collapse" }),
    ]
}

/// The rail's foot: the same four buttons stacked.
fn rail_foot(theme: &Theme, edge: Edge) -> El {
    w::column(theme.space.xs, foot_buttons(edge, true).into_iter().map(El::centred).collect()).fill()
}

/// The rail: the clock as hours over minutes, the workspace chips and the
/// Activity ring while something runs.
fn rail(theme: &Theme, store: &Store, map: &minimap::Map) -> El {
    let s = &theme.space;
    let clock = store.clock.now;
    let mut col = vec![
        w::column(
            0.0,
            vec![w::value(format_qt(clock, "hh")).centred(), w::value(format_qt(clock, "mm")).centred()],
        )
        .fill()
        .on("panel:calendar"),
    ];
    col.push(map.rail(theme));
    if let Some(g) = store.activity.activity.groups().first() {
        col.push(w::ring(activity::ring(g.lead())).centred().on("panel:activity"));
    }
    w::column(s.section_gap, col).fill()
}
