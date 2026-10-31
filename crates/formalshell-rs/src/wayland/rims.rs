//! The field every card's rim reads (`docs/superpowers/specs/2026-10-30-reactive-rims.md`):
//! the live bar's output's wallpaper, downsampled once per picture, and the
//! windows on it as lights. Built again on the topics that can move it and
//! kept when it comes out the same, so a card at rest is never aimed anew.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use fs_chrome::rim::{Field, Glint, Sky, UNFOCUSED};
use fs_chrome::types::Edge;

use super::App;
use crate::services::{compositor, wallpaper};
use crate::surfaces::rim::{self, Lit};

/// Hyprland raises no event for a window moved by pixels; this is how often
/// the model is asked again while a card is open.
const POLL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub(super) struct Rims {
    pub lit: Option<Lit>,
    /// The picture the sky was made from, by address, and the sky.
    sky: Option<(usize, Arc<Sky>)>,
    polling: bool,
    /// What the sidebar's cards were last handed.
    sidebar: Option<(u64, u64, u64)>,
}

impl App {
    /// The field again; a new generation only when it moved.
    pub(super) fn sync_rims(&mut self) {
        let field = self.store.theme.theme.reactive_rims.then(|| self.rim_field());
        let same = match (&self.rims.lit, &field) {
            (Some(lit), Some(f)) => *lit.field == *f,
            (None, None) => true,
            _ => false,
        };
        if !same {
            let generation = self.rims.lit.as_ref().map_or(0, |l| l.generation) + 1;
            self.rims.lit = field.map(|f| Lit { generation, field: Rc::new(f) });
        }
        let origin = self.bar_origin();
        let lit = self.rims.lit.clone().map(|l| (l, origin));
        let key = lit.as_ref().map(|(l, o)| (l.generation, o.0.to_bits(), o.1.to_bits()));
        if let Some(sb) = &mut self.bar.sidebar
            && self.rims.sidebar != key
        {
            self.rims.sidebar = key;
            sb.set_lit(lit);
            self.bar_dirty = true;
        }
        if self.bar.sidebar.is_none() {
            self.rims.sidebar = None;
        }
    }

    /// The bar window's top left on its output: the whole output under a
    /// frame, else its edge's strip.
    pub(super) fn bar_origin(&self) -> (f64, f64) {
        let (out_w, _) = self.output_size();
        if self.bar.framed() || self.bar.edge() != Edge::Right { (0.0, 0.0) } else { (out_w - f64::from(self.bar.scene.size.w), 0.0) }
    }

    fn rim_field(&mut self) -> Field {
        let (w, h) = self.output_size();
        Field { sky: self.rim_sky((w, h)), glints: self.rim_glints() }
    }

    fn rim_sky(&mut self, logical: (f64, f64)) -> Option<Arc<Sky>> {
        let wanted = self.store.desktop();
        if let Some(rgb) = wallpaper::flat(&wanted) {
            return Some(Arc::new(Sky::flat(rgb.map(|c| f32::from(c) / 255.0), logical)));
        }
        let bar = self.bar_wl.as_ref()?;
        let (pw, ph) = self.heads.iter().find(|h| &h.output == bar)?.backdrop.as_ref()?.pixels()?;
        let picture = self.store.wallpaper.sized(pw as u32, ph as u32).filter(|p| p.path == wanted)?.clone();
        let at = Arc::as_ptr(&picture) as usize;
        if let Some((seen, sky)) = &self.rims.sky
            && *seen == at
        {
            return Some(sky.clone());
        }
        let sky = Arc::new(Sky::from_bgra(&picture.bgra, picture.width as usize, picture.height as usize, logical)?);
        self.rims.sky = Some((at, sky.clone()));
        Some(sky)
    }

    /// The windows on the live bar's output's shown workspace, in its
    /// coordinates.
    fn rim_glints(&self) -> Vec<Glint> {
        let c = &self.store.compositor;
        let output = self.bar_output_name();
        let Some(origin) = c.outputs.iter().find(|o| o.name == output).map(|o| (o.x as f64, o.y as f64)) else { return Vec::new() };
        let snap = &c.snapshot;
        if snap.fullscreen_outputs.contains(&output) {
            return Vec::new();
        }
        let shown: Vec<&str> = snap.workspaces.iter().filter(|w| w.output == output && w.is_active).map(|w| w.id.as_str()).collect();
        let border = c.active_border.unwrap_or([1.0, 1.0, 1.0]);
        snap.windows
            .iter()
            .filter(|w| shown.contains(&w.workspace_id.as_str()) || snap.overlaid.contains(&w.id))
            .filter(|w| !snap.parked.contains(&w.id))
            .filter_map(|w| {
                let r = w.rect?;
                let (x, y) = (r.x as f64 - origin.0, r.y as f64 - origin.1);
                let focused = w.id == snap.focused_window_id;
                Some(Glint {
                    rect: (x, y, x + r.width as f64, y + r.height as f64),
                    rgb: if focused { border } else { [1.0, 1.0, 1.0] },
                    strength: if focused { 1.0 } else { UNFOCUSED },
                })
            })
            .collect()
    }

    /// Every card on the live bar's output aimed at the field, before the
    /// turn decides what animates.
    pub(super) fn light_cards(&mut self, now: Instant) {
        let lit = self.rims.lit.clone();
        let lit = lit.as_ref();
        let ms = rim::blend_ms(&self.store.theme.theme, self.motion_scale);
        let (out_w, out_h) = self.output_size();
        for h in [&mut self.outgoing, &mut self.panel].into_iter().flatten() {
            h.card.light(now, lit, (0.0, 0.0), ms);
        }
        for p in [&mut self.overflow, &mut self.menu].into_iter().flatten() {
            let d = p.card.depth() as f64;
            let origin = match p.card.edge {
                Edge::Top | Edge::Left => (0.0, 0.0),
                Edge::Bottom => (0.0, out_h - d),
                Edge::Right => (out_w - d, 0.0),
            };
            p.card.light(now, lit, origin, ms);
        }
        if let Some(w) = &mut self.launch {
            w.shown.modal.card.light(now, lit, (0.0, 0.0), ms);
        }
        if let Some(w) = &mut self.atlas.win {
            w.shown.modal.card.light(now, lit, (0.0, 0.0), ms);
        }
        if let Some(d) = &mut self.polkit {
            d.modal.card.light(now, lit, (0.0, 0.0), ms);
        }
        if let Some(w) = &mut self.plugin_overlay {
            w.modal.card.light(now, lit, (0.0, 0.0), ms);
        }
        if let Some(p) = &mut self.osd.pill {
            let d = p.card.depth() as f64;
            p.card.light(now, lit, (0.0, out_h - d), ms);
        }
        self.arm_rim_poll();
    }

    /// Whether a transient card that reads the windows is up. The sidebar's
    /// cards never close, so they take the model's own events alone.
    fn rim_card_open(&self) -> bool {
        let toasts = self.toasts.as_ref().is_some_and(|t| t.holds_cards());
        self.rims.lit.is_some() && (self.panel.is_some() || self.overflow.is_some() || self.menu.is_some() || self.modal_shown() || self.osd.pill.is_some() || toasts)
    }

    fn arm_rim_poll(&mut self) {
        if self.rims.polling || !self.rim_card_open() {
            return;
        }
        let Some(handle) = &self.handle else { return };
        self.rims.polling = true;
        let _ = handle.insert_source(Timer::from_duration(POLL), |_, _, app: &mut App| {
            if !app.rim_card_open() {
                app.rims.polling = false;
                return TimeoutAction::Drop;
            }
            compositor::send(compositor::Command::RefreshWindows);
            TimeoutAction::ToDuration(POLL)
        });
    }

    /// The open cards' rims, for `debug dump`.
    pub fn rims_report(&self) -> serde_json::Value {
        let report = |card: &crate::surfaces::card::Card, origin: (f64, f64)| {
            let r = card.rest_rect();
            let rect = vello_cpu::kurbo::Rect::new(r.x as f64 + origin.0, r.y as f64 + origin.1, r.right() as f64 + origin.0, r.bottom() as f64 + origin.1);
            card.rim.report(rect)
        };
        let (_, out_h) = self.output_size();
        serde_json::json!({
            "enabled": self.rims.lit.is_some(),
            "generation": self.rims.lit.as_ref().map_or(0, |l| l.generation),
            "windows": self.rims.lit.as_ref().map_or(0, |l| l.field.glints.len()),
            "panel": self.panel.as_ref().and_then(|h| report(&h.card, (0.0, 0.0))),
            "launcher": self.launch.as_ref().and_then(|w| report(&w.shown.modal.card, (0.0, 0.0))),
            "osd": self.osd.pill.as_ref().and_then(|p| report(&p.card, (0.0, out_h - p.card.depth() as f64))),
        })
    }
}
