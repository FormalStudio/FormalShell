//! The notification slice's clock and the toast stack's surface. Whatever
//! came due (a toast timing out, a reminder) runs before each frame, and
//! `arm_wake` sleeps until the next deadline rather than ticking.

use std::time::Instant;

use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::motion::Kind;
use crate::scroll::Travel;
use crate::services::notifications::now_ms;
use crate::store::Topic;
use crate::surface::Surface;
use crate::surfaces;
use crate::surfaces::panel::center;
use crate::surfaces::toasts::{Act, Toasts};

impl App {
    pub(crate) fn tick_notifications(&mut self) {
        let open = self.center_open();
        let n = &mut self.store.notifications;
        let dnd = n.model.dnd;
        let mut moved = false;
        // Closing the centre files everything pending as seen, however
        // the card was closed (Escape, a click outside, showHistory).
        if n.center_open != open {
            n.center_open = open;
            if !open {
                n.mark_all_seen();
            }
            moved = true;
        }
        n.sync(&self.store.state.data);
        n.sync_config(self.store.config.get("iphone.notifications.dedupe"));
        if n.tick(now_ms()) || moved || dnd != self.store.notifications.model.dnd {
            surfaces::changed(self, Topic::Notifications);
        }
    }

    fn center_open(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.is_open() && p.id() == center::ID)
    }

    /// The centre's height, the most the output leaves it, and whether it
    /// is capped: what `notifications status` reports.
    pub fn center_numbers(&self) -> (f64, f64, bool) {
        match &self.panel {
            Some(p) if p.id() == center::ID && p.is_open() => p.fit.get(),
            _ => (0.0, 0.0, false),
        }
    }

    /// The centre is a panel: it hangs off the line at `anchor` (the cell
    /// or card that opened it), else at its own cell, else at the line's end.
    pub fn set_center(&mut self, open: bool, anchor: Option<f64>) -> &'static str {
        let now = Instant::now();
        if !open {
            if self.center_open() {
                self.close_panels();
            }
        } else if !self.center_open() {
            let anchor = anchor.or_else(|| self.bar.panel_anchor(center::ID));
            self.open_host(Box::new(center::Center::default()), anchor, false, now);
        }
        self.tick_notifications();
        "ok"
    }

    pub(crate) fn toasts_changed(&mut self) {
        self.toasts_dirty = true;
    }

    /// Mapped for as long as the centre is shut, passive and empty when
    /// nothing is up: the centre suppresses the stack while it is open.
    /// Every layer unmap ends in a synthetic pointer motion to the window
    /// under the cursor (`CLayerSurface::onUnmap`), which wakes a video's
    /// hidden cursor and controls, fullscreen or not, and Hyprland 0.56's
    /// scrolling layout recalculates the workspace on every layer map,
    /// unmap and layer-state commit and, with no tiled fullscreen window on
    /// it (`SScrollingData::recalculate` counts no other), deactivates every
    /// pointer constraint (`CScrollingFullscreenHandler::updateFullscreenFade`).
    /// Buffer and input region commits reach none of that. Over a tiled
    /// fullscreen window an empty column parks on the top layer, which
    /// fullscreen hides and direct scanout ignores, where any overlay layer
    /// would keep Hyprland off scanout and tearing
    /// (`CMonitor::isSolitaryBlocked`); a layer change sends no motion to a
    /// surface the pointer is not on, and that recalc keeps the lock.
    pub(crate) fn present_toasts(&mut self, now: Instant) {
        let covered = self.chrome_hidden();
        let floating = self.store.compositor.snapshot.floating_outputs.contains(&self.bar.output);
        let n = &self.store.notifications;
        let fading = self.toasts.as_ref().is_some_and(|t| t.holds_cards());
        let up = !n.model.popups.is_empty() || !surfaces::toasts::activity_groups(&self.store).is_empty() || fading;
        let want = !n.center_open;
        let layer = if covered && !floating && !up { Layer::Top } else { Layer::Overlay };
        // Placed off the same line as every panel: its insets, and beside a
        // side line, that line's side.
        let place = self.panel_place(None, false);
        let insets = place.insets();
        let spec = place.corner(fs_info::notifications::position_spec(self.store.config.str("notifications.position")));
        let right = spec.right;
        let theme = &self.store.theme.theme;
        let cast = *self.toast_cast.get_or_insert_with(|| surfaces::toasts::column_cast(theme));
        let column = (right, surfaces::toasts::column_width(theme, cast, &insets, right));
        // The column stays mapped between toasts (see above), so it is moved
        // to the live bar's output only when a toast arrives on an empty one.
        let stray = up && self.toasts.as_ref().is_some_and(|t| !t.holds_cards() && t.output.as_ref() != self.bar_wl.as_ref());
        // The column is as tall as the output, which is unknown until its
        // first mode or logical size arrives; a scene made before that is
        // zero rows tall and never commits.
        let (_, out_h) = self.output_size();
        let resized = self.toasts.as_ref().is_some_and(|t| t.scene.size.h != out_h as i32);
        if !want || stray || resized || self.toasts.as_ref().is_some_and(|t| t.column != column) {
            if self.toasts.take().is_some() {
                self.log("toasts unmapped");
            }
            if !want {
                return;
            }
        }
        if self.toasts.is_none() {
            if out_h <= 0.0 {
                return;
            }
            let side = if right { Anchor::RIGHT } else { Anchor::LEFT };
            let ls = self.overlay("formalshell:notifications", layer, Anchor::TOP | Anchor::BOTTOM | side, (column.1, 0), -1);
            let mut surface = Surface::new("toasts", ls, &self.shm, self.started);
            surface.wait_map = true;
            self.toasts = Some(Toasts::new(surface, (column.1 as i32, out_h as i32), column, layer, self.bar_wl.clone()));
            self.toasts_dirty = true;
        }
        let qh = self.qh.clone();
        let (out_w, _) = self.output_size();
        let theme = &self.store.theme.theme;
        let Some(t) = &mut self.toasts else { return };
        let generation = |l: &Option<crate::surfaces::rim::Lit>| l.as_ref().map(|l| l.generation);
        if generation(&t.lit) != generation(&self.rims.lit) {
            t.lit = self.rims.lit.clone();
            self.toasts_dirty = true;
        }
        t.origin_x = if t.column.0 { out_w - f64::from(t.column.1) } else { 0.0 };
        if now >= t.rel_at + surfaces::toasts::REL_EVERY {
            t.rel_at = now;
            self.toasts_dirty |= up;
        }
        // One draw past the last running frame, so what rests on screen is
        // every clock's end value rather than its last sample.
        let animating = t.animating(now);
        if (self.toasts_dirty || animating || t.was_animating) && t.surface.mapped {
            let _raster = crate::scene::raster_at(t.surface.scale());
            t.draw(&self.store, theme, &mut self.bar.kit, &insets, spec, self.motion_scale, now);
            self.toasts_dirty = false;
            t.was_animating = animating;
        }
        let lifted = t.sync_layer(layer);
        let moved = t.sync_region(&self.compositor, covered);
        if moved || lifted {
            t.scene.touch(crate::scene::IRect::new(0, 0, 1, 1));
        }
        let passive = moved && covered;
        let animating = t.animating(now);
        t.surface.present(&mut t.scene, animating, &qh);
        if passive {
            self.log("toasts input empty");
        }
        if lifted {
            self.log(&format!("toasts layer {}", if layer == Layer::Top { "top" } else { "overlay" }));
        }
    }

    pub(crate) fn hover_toasts(&mut self, at: Option<(f64, f64)>) {
        let Some(t) = &mut self.toasts else { return };
        let was = t.expanded(&self.store);
        if !t.pointer(at) {
            return;
        }
        self.toasts_dirty = true;
        // An expanded stack holds every toast in it, as a hovered card does.
        let expanded = t.expanded(&self.store);
        if expanded != was {
            let ids: Vec<String> = t.members().into_iter().collect();
            self.store.notifications.set_hovered(&ids, expanded);
        }
    }

    pub(crate) fn release_toasts(&mut self, x: f64, y: f64) {
        let Some(t) = &mut self.toasts else { return };
        let act = t.release(x, y);
        self.toast_act(act);
    }

    /// One axis frame over the stack: a finger's horizontal travel swipes
    /// the card under it, and a wheel does nothing here.
    pub(crate) fn swipe_toasts(&mut self, x: f64, y: f64, along: Travel, lift: Option<u32>) {
        let ms = Kind::Spatial.ms(&self.store.theme.theme) * self.motion_scale;
        let Some(t) = &mut self.toasts else { return };
        let (moved, act) = t.swipe(x, y, along, lift, ms, Instant::now());
        if moved {
            self.toasts_dirty = true;
        }
        self.toast_act(act);
    }

    fn toast_act(&mut self, act: Act) {
        if let Act::Activity(verb, source) = act {
            if let Ok(surfaces::activity::Done::Panel) = surfaces::activity::act(&self.store, self.runtime.as_ref(), verb, &source) {
                self.set_panel("activity", true, None);
                self.panel_call("activity", "select", &source);
            }
            return;
        }
        let n = &mut self.store.notifications;
        match act {
            Act::None | Act::Activity(..) => return,
            Act::Dismiss(ids) => n.dismiss_popup_group(&ids),
            Act::Action(id, key) => {
                n.invoke_action(&id, &key);
                n.dismiss_popup(&id);
            }
            Act::Body(ids) => {
                if let Some(id) = ids.last() {
                    if n.find(id).is_some_and(|e| e.actions.iter().any(|a| a.key == "default")) {
                        n.invoke_action(id, "default");
                    }
                }
                n.dismiss_popup_group(&ids);
            }
        }
        surfaces::changed(self, Topic::Notifications);
    }
}
