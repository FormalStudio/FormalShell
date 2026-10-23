//! The notification slice's clock and the toast stack's surface. Whatever
//! came due (a toast timing out, a reminder) runs before each frame, and
//! `arm_wake` sleeps until the next deadline rather than ticking.

use std::time::Instant;

use fs_chrome::types::Edge;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::App;
use crate::motion::Kind;
use crate::scroll::Travel;
use crate::services::notifications::now_ms;
use crate::store::Topic;
use crate::surface::Surface;
use crate::surfaces;
use crate::surfaces::card::Ends;
use crate::surfaces::panel::center;
use crate::surfaces::panel::host::Place;
use crate::surfaces::toasts::{Act, Insets, Toasts};

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

    /// The centre hangs off the output's right edge, clear of a bar on any
    /// edge, at the top of the room it leaves.
    pub fn set_center(&mut self, open: bool) -> &'static str {
        let now = Instant::now();
        if !open {
            if self.center_open() {
                self.close_panels();
            }
        } else if !self.center_open() {
            let insets = self.toast_insets();
            let output = self.output_size();
            let place = Place {
                edge: Edge::Right,
                output,
                line_at: insets.right,
                ends: Ends { along: output.1, inset_start: insets.top, inset_end: insets.bottom, radius: 0.0 },
                far_inset: insets.left,
                anchor: Some(0.0),
                target: None,
            };
            self.open_host_at(Box::new(center::Center::default()), place, now);
        }
        self.tick_notifications();
        "ok"
    }

    pub(crate) fn toasts_changed(&mut self) {
        self.toasts_dirty = true;
    }

    /// Theme.edgeInset: the bar's thickness on its own edge.
    fn toast_insets(&self) -> Insets {
        let t = if self.bar.framed() { 0.0 } else { f64::from(self.bar.thickness()) };
        let edge = self.bar.edge();
        Insets {
            top: if edge == Edge::Top { t } else { 0.0 },
            bottom: if edge == Edge::Bottom { t } else { 0.0 },
            left: if edge == Edge::Left { t } else { 0.0 },
            right: if edge == Edge::Right { t } else { 0.0 },
        }
    }

    /// Mapped while anything is popped up or still fading out, and the
    /// centre is shut: the centre suppresses the stack for as long as it is
    /// open.
    pub(crate) fn present_toasts(&mut self, now: Instant) {
        let n = &self.store.notifications;
        let fading = self.toasts.as_ref().is_some_and(|t| t.holds_cards());
        let want = (!n.model.popups.is_empty() || fading) && !n.center_open;
        if !want {
            if self.toasts.take().is_some() {
                self.log("toasts unmapped");
            }
            return;
        }
        if self.toasts.is_none() {
            let size = self.output_size();
            let layer = self.overlay("formalshell:notifications", Layer::Overlay, Anchor::all(), (0, 0), -1);
            let mut surface = Surface::new("toasts", layer, &self.shm, self.started);
            surface.wait_map = true;
            self.toasts = Some(Toasts::new(surface, (size.0 as i32, size.1 as i32)));
            self.toasts_dirty = true;
        }
        let insets = self.toast_insets();
        let qh = self.qh.clone();
        let theme = &self.store.theme.theme;
        let Some(t) = &mut self.toasts else { return };
        if now >= t.rel_at + surfaces::toasts::REL_EVERY {
            t.rel_at = now;
            self.toasts_dirty = true;
        }
        // One draw past the last running frame, so what rests on screen is
        // every clock's end value rather than its last sample.
        let animating = t.animating(now);
        if (self.toasts_dirty || animating || t.was_animating) && t.surface.mapped {
            let _raster = crate::scene::raster_at(t.surface.scale());
            t.draw(&self.store, theme, &mut self.bar.kit, &insets, self.motion_scale, now);
            t.sync_region(&self.compositor);
            self.toasts_dirty = false;
            t.was_animating = animating;
        }
        let animating = t.animating(now);
        t.surface.present(&mut t.scene, animating, &qh);
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
        let n = &mut self.store.notifications;
        match act {
            Act::None => return,
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
