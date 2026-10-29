//! The region picker's surface, one per screen: the frozen frame under a
//! scrim with a hole for the selection, the selection's border and size
//! readout, the key legend, the toolbar and the card naming windows the
//! compositor gave no box.

use std::time::Instant;

use fs_theme::theme::Theme;
use vello_cpu::kurbo::Rect;

use super::picker::{Cands, Picker, R, TOOLS};
use crate::motion::{Animated, Kind as Clock};
use crate::scene::{IRect, NodeId, Paint, Scene};
use crate::surfaces::bar::cell::{Kit, Painter};
use crate::ui::el::CellState;
use crate::ui::{El, Ink, Ui, w};

pub struct Sheet {
    pub scene: Scene,
    nodes: Vec<NodeId>,
    readout: Ui,
    legend: Ui,
    pub toolbar: Ui,
    names: Ui,
    /// The selection box on the move clock, in surface coordinates.
    sel: [Animated; 4],
    placed: bool,
}

impl Sheet {
    pub fn new(width: i32, height: i32) -> Self {
        let c = Clock::Spatial.curve();
        Self {
            scene: Scene::clear(width, height),
            nodes: Vec::new(),
            readout: Ui::new(None),
            legend: Ui::new(None),
            toolbar: Ui::new(None),
            names: Ui::new(None),
            sel: [Animated::new(0.0, c), Animated::new(0.0, c), Animated::new(0.0, c), Animated::new(0.0, c)],
            placed: false,
        }
    }

    /// Lays out and paints; true while the selection box is travelling.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&mut self, picker: &Picker, c: &Cands, origin: (f64, f64), frame: Option<&crate::scene::Bitmap>, theme: &Theme, kit: &mut Kit, scale: f64, now: Instant) -> bool {
        let (sw, sh) = (self.scene.size.w, self.scene.size.h);
        let s = &theme.space;
        let current = picker.current(c);
        let target = current.as_ref().and_then(|e| e.rect).map(|r| R { x: r.x - origin.0, y: r.y - origin.1, w: r.w, h: r.h });
        if let Some(t) = target {
            let travel = self.placed && picker.drag.is_none();
            let ms = Clock::Spatial.ms(theme) * scale;
            for (a, v) in self.sel.iter_mut().zip([t.x, t.y, t.w, t.h]) {
                if travel {
                    if a.target() != v {
                        a.set(now, v, ms);
                    }
                } else {
                    a.jump(v);
                }
            }
            self.placed = true;
        }
        let [x, y, bw, bh] = [0, 1, 2, 3].map(|i| self.sel[i].value(now));
        let animating = self.sel.iter().any(|a| a.running(now));

        let mut p = Painter::new(&mut self.scene, &mut self.nodes, None);
        // The freeze is the output's native pixels, which the surface draws
        // at: a scaled output's frame lands one to one.
        if let Some(f) = frame {
            p.shape(IRect::new(0, 0, sw, sh), Paint::Picture { image: f.clone(), alpha: 1.0, radius: 0.0 });
        }
        let bg = theme.colors.get("background");
        let scrim = bg.with_alpha(0.6);
        let toolbar_box = theme.box_style("card", Some("opaque"));
        let pad = s.panel_padding;
        let toolbar_el = toolbar(picker, theme);
        let (tw, th) = crate::ui::measure(&toolbar_el, sw as f64, theme, kit);
        let card = IRect::new(((sw as f64 - (tw + pad * 2.0)) / 2.0).round() as i32, (sh as f64 - (th + pad * 2.0) - s.md).round() as i32, (tw + pad * 2.0).round() as i32, (th + pad * 2.0).round() as i32);
        let names = picker.named_shown(c);
        let names_el = names.then(|| names_list(c, theme));
        let names_card = names_el.as_ref().map(|el| {
            let w = s.popup_width_wide;
            let (_, h) = crate::ui::measure(el, w - pad * 2.0, theme, kit);
            IRect::new(((sw as f64 - w) / 2.0).round() as i32, ((sh as f64 - h - pad * 2.0) / 2.0).round() as i32, w as i32, (h + pad * 2.0).round() as i32)
        });
        let (xi, yi, wi, hi) = (x.round() as i32, y.round() as i32, bw.round() as i32, bh.round() as i32);
        if target.is_some() || self.placed && current.is_some() {
            p.rect(IRect::new(0, 0, sw, yi.max(0)), scrim, 0.0);
            p.rect(IRect::new(0, yi + hi, sw, (sh - (yi + hi)).max(0)), scrim, 0.0);
            p.rect(IRect::new(0, yi, xi.max(0), hi), scrim, 0.0);
            p.rect(IRect::new(xi + wi, yi, (sw - (xi + wi)).max(0), hi), scrim, 0.0);
            let ink = theme.colors.get(if picker.recording() { "destructive" } else { "primary" });
            let width = (theme.border_width * 2.0) as f32;
            p.framed(IRect::new(xi, yi, wi, hi), fs_theme::color::Rgba::TRANSPARENT, 0.0, ink, width);
        } else {
            p.rect(IRect::new(0, 0, sw, sh), scrim, 0.0);
        }
        let radius = theme.box_radius(&toolbar_box, card.h as f64);
        crate::ui::boxes::paint(&mut p, card, &toolbar_box, radius, 1.0, 0.0);
        if let Some(r) = names_card {
            let radius = theme.box_radius(&toolbar_box, r.h as f64);
            crate::ui::boxes::paint(&mut p, r, &toolbar_box, radius, 1.0, 0.0);
        }
        let anchor = p.last();
        p.finish();

        let mut drawn_animating = false;
        for ui in [&mut self.readout, &mut self.legend, &mut self.toolbar, &mut self.names] {
            ui.anchor = anchor;
            ui.motion_scale = scale;
        }
        match current.as_ref().and_then(|e| e.rect.map(|r| (r, e))) {
            Some((r, e)) => {
                let label = if e.kind == super::picker::EntryKind::Drag { String::new() } else { e.label.clone() };
                let el = readout(r, &label, theme);
                let (rw, rh) = crate::ui::measure(&el, sw as f64, theme, kit);
                let above = y - rh - s.xs;
                let ry = if above >= 0.0 { above } else { y + s.xs };
                let rect = Rect::new(x.max(0.0), ry, x.max(0.0) + rw, ry + rh);
                drawn_animating |= self.readout.draw(&el, rect, None, 1.0, theme, kit, &mut self.scene, now).animating;
            }
            None => self.readout.hide(&mut self.scene),
        }
        let legend_el = legend(picker, theme);
        let (lw, lh) = crate::ui::measure(&legend_el, sw as f64, theme, kit);
        let ly = card.y as f64 - lh - s.sm;
        let lx = ((sw as f64 - lw) / 2.0).round();
        drawn_animating |= self.legend.draw(&legend_el, Rect::new(lx, ly, lx + lw, ly + lh), None, 1.0, theme, kit, &mut self.scene, now).animating;
        let (cx, cy) = (card.x as f64 + pad, card.y as f64 + pad);
        drawn_animating |= self.toolbar.draw(&toolbar_el, Rect::new(cx, cy, cx + tw, cy + th), None, 1.0, theme, kit, &mut self.scene, now).animating;
        match (names_el, names_card) {
            (Some(el), Some(r)) => {
                let (nx, ny) = (r.x as f64 + pad, r.y as f64 + pad);
                drawn_animating |= self.names.draw(&el, Rect::new(nx, ny, r.right() as f64 - pad, r.bottom() as f64 - pad), None, 1.0, theme, kit, &mut self.scene, now).animating;
            }
            _ => self.names.hide(&mut self.scene),
        }
        animating || drawn_animating
    }
}

fn chip(child: El) -> El {
    w::cell(child).cell_state(|c: &mut CellState| {
        c.chip = true;
        c.small = true;
    })
}

fn readout(r: R, label: &str, theme: &Theme) -> El {
    let s = &theme.space;
    let mut parts = vec![w::caption(format!("{}\u{d7}{}", r.w.round(), r.h.round())).mono().ink(Ink::Fg)];
    if !label.is_empty() {
        parts.push(w::section_label(s, label, None, false).ink(Ink::Dim));
    }
    chip(w::row(s.sm, parts))
}

fn legend(picker: &Picker, theme: &Theme) -> El {
    let s = &theme.space;
    let verb = if picker.recording() { "record" } else { "capture" };
    let text = format!("Return {verb}  \u{b7}  Ctrl+Return display  \u{b7}  Tab cycle  \u{b7}  1-6 tool  \u{b7}  Esc cancel");
    chip(w::section_label(s, &text, None, false).ink(Ink::Dim))
}

fn toolbar(picker: &Picker, theme: &Theme) -> El {
    let s = &theme.space;
    let tool = picker.tool_index();
    let cell = |i: usize| {
        let t = &TOOLS[i];
        w::cell(w::row(s.sm, vec![w::icon(t.icon), w::section_label(s, t.label, None, false).ink(Ink::Dim)]))
            .cell_state(|c: &mut CellState| {
                c.chip = true;
                c.ghost = true;
                c.selected = tool == i as i32;
            })
            .interactive()
            .on(format!("tool:{i}"))
    };
    let label = |text: &str| w::section_label(s, text, None, false);
    let rec = picker.recording();
    let commit = w::cell(w::row(
        s.sm,
        vec![w::icon(if rec { "video" } else { "camera" }), w::section_label(s, if rec { "Record" } else { "Capture" }, None, false).ink(Ink::Dim)],
    ))
    .cell_state(|c: &mut CellState| {
        c.chip = true;
        c.ghost = true;
        c.active = true;
    })
    .interactive()
    .on("commit");
    w::row(
        s.sm,
        vec![
            label("Shot"),
            w::row(0.0, (0..3).map(cell).collect()),
            label("Rec"),
            w::row(0.0, (3..6).map(cell).collect()),
            commit,
        ],
    )
}

fn names_list(c: &Cands, theme: &Theme) -> El {
    let s = &theme.space;
    let mut rows = vec![w::section_label(s, "Cannot capture: no compositor geometry", None, true)];
    for e in &c.without_rect {
        let mut col = vec![w::text(e.label.clone()).ink(Ink::Muted).elide()];
        if !e.sublabel.is_empty() {
            col.push(w::section_label(s, &e.sublabel, None, false));
        }
        rows.push(w::cell(w::column(0.0, col)).ghost());
    }
    w::column(0.0, rows)
}
