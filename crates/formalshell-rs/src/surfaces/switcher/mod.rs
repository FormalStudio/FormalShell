//! The Alt+Tab switcher: the windows as
//! thumbnails on one card in the middle of the output, each with its app
//! icon and title under it, the selected one under the cursor. Summoned
//! over IPC alone (`switcher next|prev|commit|cancel|state`), so the
//! compositor's own binds drive it; it maps only 150 ms into a hold, so a
//! quick Alt+Tab switches with no card at all.
//!
//! This is the switcher's state and its drawing; `wayland::switcher` owns
//! its window, its timers and its captures.

pub mod model;

use std::time::{Duration, Instant};

use fs_theme::color::Rgba;
use fs_theme::style::Line;
use fs_theme::theme::Theme;
use serde_json::{Value, json};

use crate::motion::{Animated, Curve};
use crate::scene::{Bitmap, IRect, NodeId, Paint, Scene};
use crate::services::compositor::model::Window;
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::text::{Family, TextStyle};
use crate::ui::boxes;

/// How long the card waits before it maps (macOS's Cmd+Tab).
pub const SHOW_AFTER: Duration = Duration::from_millis(150);
/// The window in which a release and its `next` may arrive out of order.
pub const COMMIT_RACE: Duration = Duration::from_millis(80);
pub const PRIME: Duration = Duration::from_millis(75);
pub const REFOCUS: Duration = Duration::from_millis(80);
/// Two frames at 60Hz: the card is on screen before the first capture.
pub const CAPTURE_AFTER: Duration = Duration::from_millis(32);
/// The selected thumbnail's refresh, five a second; the rest hold one frame.
pub const REFRESH: Duration = Duration::from_millis(200);

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
    pub open: bool,
    /// The card is mapped for it.
    pub shown: bool,
    pub shows: u64,
    pub index: usize,
    history: Vec<String>,
    pub open_history: Vec<String>,
    pub open_workspace: String,
    pub commit_pending: Option<Instant>,
    pub committed_at: Option<Instant>,
    pub show_at: Option<Instant>,
    pub primed: bool,
    pub prime_at: Option<Instant>,
    pub refocus: Option<(String, Instant)>,
    pub capturing: bool,
    pub capture_at: Option<Instant>,
    pub refresh_at: Option<Instant>,
    pub fade: Option<Animated>,
    pub scroll: f64,
    pub cells: Vec<CellRects>,
    pub nodes: Vec<NodeId>,
}

/// What one layout pass reads.
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

/// The sizes the switcher draws at.
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
    TextStyle { family: kit.look.sans, size: kit.look.caption, weight: if selected { 500.0 } else { 400.0 }, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None }
}


impl Switcher {
    /// A window took focus: it goes to the front of the history.
    pub fn focused(&mut self, id: &str) {
        if id.is_empty() || self.history.first().is_some_and(|h| h == id) {
            return;
        }
        self.history.retain(|h| h != id);
        self.history.insert(0, id.to_owned());
        self.history.truncate(32);
    }

    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// The card's opacity now, and whether it is still moving.
    pub fn alpha(&self, now: Instant) -> (f32, bool) {
        match &self.fade {
            Some(f) => (f.value(now).clamp(0.0, 1.0) as f32, f.running(now)),
            None => (0.0, false),
        }
    }

    pub fn set_fade(&mut self, theme: &Theme, now: Instant, scale: f64, open: bool) {
        let motion = theme.motion();
        let curve = Curve::from_table(&motion.switcher_curve);
        let f = self.fade.get_or_insert_with(|| Animated::new(0.0, curve));
        f.set(now, if open { 1.0 } else { 0.0 }, motion.switcher * scale);
    }

    /// Each thumbnail's id and device size for this layout.
    pub fn thumb_sizes(&self, entries: &[&Window]) -> Vec<(String, (u32, u32))> {
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
            self.scroll = model::follow(p.y, m.cell_h, self.scroll, view_h, grid_h);
        }
        let origin = (card.x as f64 + (card_w - grid_w) / 2.0, card.y as f64 + (card_h - view_h) / 2.0 - self.scroll);
        let view = IRect::new(origin.0.round() as i32, (origin.1 + self.scroll).round() as i32, grid_w.round() as i32, view_h.round() as i32);

        self.cells = places
            .iter()
            .map(|p| {
                let (x, y) = ((origin.0 + p.x).round() as i32, (origin.1 + p.y).round() as i32);
                let pi = pad as i32;
                CellRects {
                    cell: IRect::new(x, y, p.width.round() as i32, p.height.round() as i32),
                    thumb: IRect::new(x + pi, y + pi, (p.width - pad * 2.0).round() as i32, m.thumb_h.round() as i32),
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
        let fg = t.colors.get("foreground");
        let muted = t.colors.get("mutedForeground");
        let mut p = Painter::new(scene, &mut self.nodes, None);

        let card_box = t.box_style("switcher", None);
        let radius = t.box_radius(&card_box, card.h as f64);
        boxes::paint(&mut p, card, &card_box, radius, alpha, 0.0);

        if count == 0 {
            let r = IRect::new(card.x + pad as i32, card.y + pad as i32, content_w.round() as i32, m.cell_h.round() as i32);
            let b = t.box_style("switcher.cell", Some("rest"));
            boxes::paint(&mut p, r, &b, t.box_radius(&b, r.h as f64), alpha, 0.0);
            let style = TextStyle { family: kit.look.sans, size: kit.look.caption, weight: 500.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
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

            let mut b = t.box_style("switcher.cell", Some(if selected { "selected" } else { "ghost" }));
            let rest_border = t.box_style("switcher.cell", None).border.map_or(Rgba::TRANSPARENT, |l| l.color);
            b.border = Some(t.box_style("switcher.cell", Some("ghost")).border.unwrap_or(Line { color: rest_border, width: 0.0 }));
            let b = t.with_cursor(b, selected, true);
            boxes::paint(&mut p, rects.cell, &b, t.box_radius(&b, rects.cell.h as f64), alpha, 0.0);

            let th = rects.thumb;
            let thumb = crate::ui::thumb::Thumb { picture: input.thumbs.get(i).and_then(Option::as_ref), icon: icon.and_then(|i| i.2.as_ref()), title: "", selected: false, badge: false, edge: 1.0 };
            crate::ui::thumb::paint(&mut p, kit, t, &thumb, th, selected, false, alpha);

            let (n, of) = ordinals.get(i).copied().unwrap_or((1, 1));
            if of > 1 {
                let st = TextStyle { family: kit.look.mono, size: kit.look.body, weight: 500.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
                let label = kit.shape(&n.to_string(), st);
                let h = label.line_height() + (s.control_padding_y * 2.0) as i32;
                let w = label.width + (s.control_padding_x * 2.0) as i32;
                let r = IRect::new(th.right() - s.xs as i32 - w, th.bottom() - s.xs as i32 - h, w, h);
                let chip = t.box_style("cell", Some("active"));
                boxes::paint(&mut p, r, &chip, t.radii.sm, alpha, 0.0);
                p.text(&label, (r.x + (r.w - label.width) / 2, r.y + (r.h - label.line_height()) / 2), a(t.colors.get("primaryForeground")), &[]);
            }

            let ir = rects.icon;
            match icon.and_then(|i| i.1.clone()) {
                Some(small) => p.shape(ir, Paint::Picture { image: small, alpha, radius: 0.0 }),
                None => {
                    let glyph = fs_theme::icons::glyph("app-window");
                    let st = TextStyle { family: Family::Named(glyph.family), size: m.caption_icon as f32, weight: 400.0, tracking: 0.0, wdth: TextStyle::NORMAL_WIDTH, wght: None };
                    let g = kit.shape(glyph.text, st);
                    p.text(&g, (ir.x + (ir.w - g.width) / 2, ir.y + (ir.h - g.line_height()) / 2), a(muted), &[]);
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
        let r = |r: &IRect| json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h});
        Value::Array(self.cells.iter().map(|c| json!({"cell": r(&c.cell), "thumb": r(&c.thumb), "icon": r(&c.icon)})).collect())
    }
}
