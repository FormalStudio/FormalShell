//! The Alt+Tab switcher: the windows as
//! thumbnails on one card in the middle of the output, each with its app
//! icon and title under it, the selected one under the cursor. Summoned
//! over IPC alone (`switcher next|prev|commit|cancel|state`), so the
//! compositor's own binds drive it; it maps only 150 ms into a hold, so a
//! quick Alt+Tab switches with no card at all.
//!
//! This is the switcher's state, its clocks and its drawing;
//! `wayland::switcher` owns its window and its captures.

pub mod model;

use std::time::{Duration, Instant};

use fs_theme::palette::ColorRole;
use fs_theme::color::Rgba;
use fs_theme::style::{Line, Role};
use fs_theme::theme::Theme;
use fs_theme::tokens::WEIGHTS;
use fs_types::{IRect, Window, WindowId, WorkspaceId};
use serde_json::{Value, json};

use crate::motion::{Animated, Curve};
use crate::scene::{Bitmap, NodeId, Paint, Scene};
use crate::paint::{Kit, Painter};
use crate::text::TextStyle;
use crate::ui::boxes;

/// How long the card waits before it maps (macOS's Cmd+Tab).
const SHOW_AFTER: Duration = Duration::from_millis(150);
/// The window in which a release and its `next` may arrive out of order.
const COMMIT_RACE: Duration = Duration::from_millis(80);
const PRIME: Duration = Duration::from_millis(75);
const REFOCUS: Duration = Duration::from_millis(80);
/// Two frames at 60Hz: the card is on screen before the first capture.
const CAPTURE_AFTER: Duration = Duration::from_millis(32);
/// The selected thumbnail's refresh, five a second; the rest hold one frame.
const REFRESH: Duration = Duration::from_millis(200);

/// One cell as drawn, in output pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CellRects {
    pub cell: IRect,
    pub thumb: IRect,
    pub icon: IRect,
}

#[derive(Default)]
pub struct Switcher {
    /// A switch is under way, from the first step to the commit or cancel.
    open: bool,
    /// The card is mapped for it.
    shown: bool,
    shows: u64,
    index: usize,
    history: Vec<WindowId>,
    /// The history and the workspace the switch opened on.
    open_history: Vec<WindowId>,
    open_workspace: WorkspaceId,
    commit_pending: Option<Instant>,
    committed_at: Option<Instant>,
    show_at: Option<Instant>,
    /// The layer has let the keyboard go to OnDemand.
    primed: bool,
    prime_at: Option<Instant>,
    refocus: Option<(WindowId, Instant)>,
    capturing: bool,
    capture_at: Option<Instant>,
    refresh_at: Option<Instant>,
    fade: Option<Animated>,
    scroll: f64,
    cells: Vec<CellRects>,
    nodes: Vec<NodeId>,
}

/// What the clocks that came due ask of the host, in this order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Due {
    /// A commit's focus held for the keyboard's release.
    pub refocus: Option<WindowId>,
    /// The card shows: `fresh` when it needs a new window, else the one
    /// still fading out takes the keyboard back.
    pub show: Option<bool>,
    /// The keyboard goes to OnDemand.
    pub prime: bool,
    /// The captures start.
    pub capture: bool,
    /// The selected thumbnail is captured again.
    pub refresh: bool,
}

pub struct Input<'a> {
    pub theme: &'a Theme,
    pub output: (f64, f64),
    pub entries: &'a [&'a Window],
    /// Per entry: its app key (for the ordinals), its caption icon and the
    /// schematic's larger one.
    pub icons: &'a [(String, Option<Bitmap>, Option<Bitmap>)],
    pub thumbs: &'a [Option<Bitmap>],
    pub alpha: f32,
}

pub struct Metrics {
    pub thumb_h: f64,
    pub caption_icon: f64,
    pub caption_h: f64,
    pub cell_h: f64,
    pub pad: f64,
}

impl Metrics {
    pub fn new(theme: &Theme, kit: &mut Kit) -> Self {
        let s = &theme.space;
        let caption_icon = s.icon_gap * 2.0;
        let style = caption_style(kit, false);
        let line = kit.shape("Ag", style).line_height() as f64;
        let caption_h = caption_icon.max(line);
        Self {
            thumb_h: s.switcher_thumb,
            caption_icon,
            caption_h,
            cell_h: s.switcher_thumb + s.icon_gap + caption_h + s.panel_padding * 2.0,
            pad: s.panel_padding,
        }
    }
}

fn caption_style(kit: &Kit, selected: bool) -> TextStyle {
    kit.weight(kit.look.sans, kit.look.caption, if selected { WEIGHTS.medium } else { WEIGHTS.normal })
}


impl Switcher {
    /// A window took focus: it goes to the front of the history.
    pub fn focused(&mut self, id: &WindowId) {
        if id.is_empty() || self.history.first() == Some(id) {
            return;
        }
        self.history.retain(|h| h != id);
        self.history.insert(0, id.clone());
        self.history.truncate(32);
    }

    pub fn open(&self) -> bool {
        self.open
    }

    pub fn shown(&self) -> bool {
        self.shown
    }

    pub fn shows(&self) -> u64 {
        self.shows
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// The thumbnails are captured.
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// The windows the switch walks, in the order it opened on.
    pub fn entries<'a>(&self, windows: &'a [Window]) -> Vec<&'a Window> {
        model::entries(windows, &self.open_history, &self.open_workspace)
    }

    /// The first `next` or `prev` opens the switch on `workspace` (empty
    /// for every one), with the history as it stands. False inside a
    /// commit's race, where a repeat sent before Alt came up lands late.
    pub fn begin(&mut self, workspace: WorkspaceId, now: Instant) -> bool {
        if self.committed_at.is_some_and(|t| now.duration_since(t) < COMMIT_RACE) {
            return false;
        }
        self.open_history = self.history.clone();
        self.open_workspace = workspace;
        self.index = 0;
        self.primed = false;
        self.open = true;
        self.scroll = 0.0;
        self.show_at = Some(now + SHOW_AFTER);
        true
    }

    /// The cursor `step` along `count` entries.
    pub fn walk(&mut self, count: usize, step: i32) {
        self.index = model::advance(self.index, count, step);
    }

    /// A commit with nothing open yet waits a moment for the `next` it
    /// may have overtaken.
    pub fn defer_commit(&mut self, now: Instant) {
        self.commit_pending = Some(now + COMMIT_RACE);
    }

    /// A commit that was waiting on this switch's opening.
    pub fn take_pending_commit(&mut self) -> bool {
        self.commit_pending.take().is_some()
    }

    /// Commits to `id`: the window to focus now, or none when the focus
    /// waits for the keyboard's release. Inside the prime the layer still
    /// holds the keyboard exclusively, and Hyprland hands focus back to
    /// where it came from once it lets go: one dispatch after that
    /// release, not one either side of it.
    pub fn commit(&mut self, id: WindowId, now: Instant) -> Option<WindowId> {
        let (primed, shown) = (self.primed, self.shown);
        self.committed_at = Some(now);
        if id.is_empty() {
            return None;
        }
        if primed || !shown {
            return Some(id);
        }
        self.refocus = Some((id, now + REFOCUS));
        None
    }

    /// The switch ends; a shown card fades out.
    pub fn close(&mut self, theme: &Theme, now: Instant, scale: f64) {
        self.show_at = None;
        self.open = false;
        if self.shown {
            self.shown = false;
            self.set_fade(theme, now, scale, false);
        }
    }

    /// The window's first frame landed.
    pub fn landed(&mut self, now: Instant) {
        self.prime_at = Some(now + PRIME);
        self.capture_at = Some(now + CAPTURE_AFTER);
    }

    /// The window went with its fade: nothing drawn or captured is kept.
    pub fn unmapped(&mut self) {
        self.capturing = false;
        self.cells.clear();
        self.nodes.clear();
    }

    /// When the next clock comes due.
    pub fn deadline(&self) -> Option<Instant> {
        let refresh = self.refresh_at.filter(|_| self.shown && self.capturing);
        [self.show_at, self.commit_pending, self.prime_at, self.capture_at, refresh, self.refocus.as_ref().map(|r| r.1)].into_iter().flatten().min()
    }

    /// Runs the clocks due at `now`; `window` says whether the card's
    /// window is still up from the last show.
    pub fn tick(&mut self, theme: &Theme, now: Instant, scale: f64, window: bool) -> Due {
        let due = |t: Option<Instant>| t.is_some_and(|t| t <= now);
        let mut out = Due::default();
        if due(self.commit_pending) {
            self.commit_pending = None;
        }
        if self.refocus.as_ref().is_some_and(|r| r.1 <= now) {
            let (id, _) = self.refocus.take().expect("refocus");
            if !self.open {
                out.refocus = Some(id);
            }
        }
        if due(self.show_at) {
            self.show_at = None;
            if self.open {
                self.show(theme, now, scale, window);
                out.show = Some(!window);
            }
        }
        if due(self.prime_at) {
            self.prime_at = None;
            if self.shown {
                self.primed = true;
                out.prime = true;
            }
        }
        if due(self.capture_at) {
            self.capture_at = None;
            if self.shown {
                self.capturing = true;
                self.refresh_at = Some(now + REFRESH);
                out.capture = true;
            }
        }
        if self.shown && self.capturing && due(self.refresh_at) {
            self.refresh_at = Some(now + REFRESH);
            out.refresh = true;
        }
        out
    }

    fn show(&mut self, theme: &Theme, now: Instant, scale: f64, window: bool) {
        self.shown = true;
        self.shows += 1;
        self.capturing = false;
        self.prime_at = None;
        self.set_fade(theme, now, scale, true);
        if window {
            self.prime_at = Some(now + PRIME);
            self.capture_at = Some(now + CAPTURE_AFTER);
        }
    }

    /// The card's opacity now, and whether it is still moving.
    pub fn alpha(&self, now: Instant) -> (f32, bool) {
        match &self.fade {
            Some(f) => (f.value(now).clamp(0.0, 1.0) as f32, f.running(now)),
            None => (0.0, false),
        }
    }

    fn set_fade(&mut self, theme: &Theme, now: Instant, scale: f64, open: bool) {
        let motion = theme.motion();
        let curve = Curve::from_table(&motion.switcher_curve);
        let f = self.fade.get_or_insert_with(|| Animated::new(0.0, curve));
        f.set(now, if open { 1.0 } else { 0.0 }, motion.switcher * scale);
    }

    /// Each thumbnail's id and device size for this layout.
    pub fn thumb_sizes(&self, entries: &[&Window]) -> Vec<(WindowId, (u32, u32))> {
        entries
            .iter()
            .zip(&self.cells)
            .map(|(w, c)| (w.id.clone(), (c.thumb.w.max(1) as u32, c.thumb.h.max(1) as u32)))
            .collect()
    }

    /// Lays the card out on `scene` and draws it.
    pub fn draw(&mut self, input: &Input, kit: &mut Kit, scene: &mut Scene) {
        let t = input.theme;
        let s = &t.space;
        let m = Metrics::new(t, kit);
        let (ow, oh) = input.output;
        let pad = m.pad;
        let count = input.entries.len();
        let max_row = (ow - (s.switcher_inset + pad) * 2.0).max(0.0);
        let widths: Vec<f64> = input
            .entries
            .iter()
            .map(|w| {
                let max = (m.thumb_h / 2.0).max((m.thumb_h * 3.0).min(max_row - pad * 2.0));
                model::thumb_width(w.rect.as_ref(), m.thumb_h, m.thumb_h / 2.0, max) + pad * 2.0
            })
            .collect();
        let grid = model::layout(&widths, 0.0, max_row);
        let places = model::cells(&grid, m.cell_h);
        let grid_w = if count > 0 { grid.width } else { 0.0 };
        let grid_h = if grid.rows.is_empty() { m.cell_h } else { grid.rows.len() as f64 * m.cell_h };
        let max_rows = ((oh - s.switcher_inset * 2.0 - pad * 2.0) / m.cell_h).floor().max(1.0);
        let view_h = grid_h.min(max_rows * m.cell_h);
        let content_w = grid_w.max(s.popup_width_narrow - pad * 2.0);
        let (card_w, card_h) = (content_w + pad * 2.0, view_h + pad * 2.0);
        let card = IRect::new(((ow - card_w) / 2.0).round() as i32, ((oh - card_h) / 2.0).round() as i32, card_w.round() as i32, card_h.round() as i32);

        if let Some(p) = places.get(self.index) {
            self.scroll = crate::scroll::follow(p.rect.y, m.cell_h, self.scroll, view_h, grid_h);
        }
        let origin = (card.x as f64 + (card_w - grid_w) / 2.0, card.y as f64 + (card_h - view_h) / 2.0 - self.scroll);
        let view = IRect::new(origin.0.round() as i32, (origin.1 + self.scroll).round() as i32, grid_w.round() as i32, view_h.round() as i32);

        self.cells = places
            .iter()
            .map(|p| {
                let (x, y) = ((origin.0 + p.rect.x).round() as i32, (origin.1 + p.rect.y).round() as i32);
                let pi = pad as i32;
                CellRects {
                    cell: IRect::new(x, y, p.rect.width.round() as i32, p.rect.height.round() as i32),
                    thumb: IRect::new(x + pi, y + pi, (p.rect.width - pad * 2.0).round() as i32, m.thumb_h.round() as i32),
                    icon: IRect::new(
                        x + pi,
                        (y as f64 + pad + m.thumb_h + s.icon_gap + (m.caption_h - m.caption_icon) / 2.0).round() as i32,
                        m.caption_icon as i32,
                        m.caption_icon as i32,
                    ),
                }
            })
            .collect();

        let alpha = input.alpha;
        let a = |c: Rgba| c.with_alpha(c.a * alpha);
        let fg = t.colors.get(ColorRole::Foreground);
        let muted = t.colors.get(ColorRole::MutedForeground);
        let mut p = Painter::new(scene, &mut self.nodes, None);

        let card_box = t.box_style(Role::Switcher, None);
        let radius = t.box_radius(&card_box, card.h as f64);
        boxes::paint(&mut p, card, &card_box, radius, alpha, 0.0);

        if count == 0 {
            let r = IRect::new(card.x + pad as i32, card.y + pad as i32, content_w.round() as i32, m.cell_h.round() as i32);
            let b = t.box_style(Role::SwitcherCell, Some("rest"));
            boxes::paint(&mut p, r, &b, t.box_radius(&b, r.h as f64), alpha, 0.0);
            let style = kit.weight(kit.look.sans, kit.look.caption, WEIGHTS.medium);
            let label = kit.shape("No windows", style);
            p.text(&label, (r.x + (r.w - label.width) / 2, r.y + (r.h - label.line_height()) / 2), a(muted), &[]);
            p.finish();
            return;
        }

        let keys: Vec<String> = input.icons.iter().map(|i| i.0.clone()).collect();
        let ordinals = model::ordinals(&keys);
        // Clipped only once the rows scroll, so the cursor's halo is whole.
        p.clip = (grid_h > view_h).then_some(view);
        for (i, rects) in self.cells.clone().iter().enumerate() {
            let selected = i == self.index;
            let win = input.entries[i];
            let icon = input.icons.get(i);

            let mut b = t.box_style(Role::SwitcherCell, Some(if selected { "selected" } else { "ghost" }));
            let rest_border = t.box_style(Role::SwitcherCell, None).border.map_or(Rgba::TRANSPARENT, |l| l.color);
            b.border = Some(t.box_style(Role::SwitcherCell, Some("ghost")).border.unwrap_or(Line { color: rest_border, width: 0.0 }));
            let b = t.with_cursor(b, selected, true);
            boxes::paint(&mut p, rects.cell, &b, t.box_radius(&b, rects.cell.h as f64), alpha, 0.0);

            let th = rects.thumb;
            let thumb = crate::ui::thumb::Thumb { picture: input.thumbs.get(i).and_then(Option::as_ref), icon: icon.and_then(|i| i.2.as_ref()), title: "", selected: false, badge: false, edge: 1.0 };
            crate::ui::thumb::paint(&mut p, kit, t, &thumb, th, selected, false, alpha);

            let (n, of) = ordinals.get(i).copied().unwrap_or((1, 1));
            if of > 1 {
                let st = kit.weight(kit.look.mono, kit.look.body, WEIGHTS.medium);
                let label = kit.shape(&n.to_string(), st);
                let h = label.line_height() + (s.control_padding_y * 2.0) as i32;
                let w = label.width + (s.control_padding_x * 2.0) as i32;
                let r = IRect::new(th.right() - s.xs as i32 - w, th.bottom() - s.xs as i32 - h, w, h);
                let chip = t.box_style(Role::Cell, Some("active"));
                boxes::paint(&mut p, r, &chip, t.radii.sm, alpha, 0.0);
                p.text(&label, (r.x + (r.w - label.width) / 2, r.y + (r.h - label.line_height()) / 2), a(t.colors.get(ColorRole::PrimaryForeground)), &[]);
            }

            let ir = rects.icon;
            match icon.and_then(|i| i.1.clone()) {
                Some(small) => p.shape(ir, Paint::Picture { image: small, alpha, radius: 0.0 }),
                None => {
                    kit.glyph(&mut p, "app-window", m.caption_icon as f32, ir, a(muted));
                }
            }
            let title_x = ir.right() + s.icon_gap as i32;
            let max = (th.right() - title_x) as f64;
            let style = caption_style(kit, selected);
            let title = kit.elided(&win.title, style, max.max(0.0));
            let cap_y = rects.thumb.bottom() as f64 + s.icon_gap;
            let ty = (cap_y + (m.caption_h - title.line_height() as f64) / 2.0).round() as i32;
            p.text(&title, (title_x, ty), a(if selected { fg } else { muted }), &[]);
        }
        p.finish();
    }

    /// `switcher state`'s `cells`, in output pixels.
    pub fn cells_json(&self) -> Value {
        let r = |r: &IRect| json!(r);
        Value::Array(self.cells.iter().map(|c| json!({"cell": r(&c.cell), "thumb": r(&c.thumb), "icon": r(&c.icon)})).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_shows_only_past_the_hold() {
        let theme = Theme::default();
        let t0 = Instant::now();
        let mut s = Switcher::default();
        assert!(s.begin(WorkspaceId::default(), t0));
        assert_eq!(s.tick(&theme, t0 + Duration::from_millis(100), 1.0, false).show, None);
        assert_eq!(s.tick(&theme, t0 + SHOW_AFTER, 1.0, false).show, Some(true));
        assert!(s.shown());
    }

    #[test]
    fn a_quick_tap_commits_with_no_card_and_a_late_step_is_dropped() {
        let theme = Theme::default();
        let t0 = Instant::now();
        let mut s = Switcher::default();
        assert!(s.begin(WorkspaceId::default(), t0));
        let at = t0 + Duration::from_millis(40);
        assert_eq!(s.commit("w".into(), at), Some("w".into()));
        s.close(&theme, at, 1.0);
        assert_eq!(s.tick(&theme, t0 + SHOW_AFTER * 2, 1.0, false), Due::default());
        assert!(!s.shown() && s.shows() == 0);
        assert!(!s.begin(WorkspaceId::default(), at + Duration::from_millis(10)));
        assert!(s.begin(WorkspaceId::default(), at + COMMIT_RACE));
    }
}
