//! The notification slice's clock and the toast stack's surface. Whatever
//! came due (a toast timing out, a reminder) runs before each frame, and
//! `arm_wake` sleeps until the next deadline rather than ticking.

use std::time::Instant;

use fs_chrome::bar::panels::PanelId;
use fs_types::Insets;
use fs_info::notifications::PositionSpec;
use smithay_client_toolkit::reexports::client::protocol::wl_pointer;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::wlr_layer::{Anchor, Layer};

use super::{App, Press, Target};
use crate::motion::Kind;
use crate::runtime::now_ms;
use crate::services::notifications::Op;
use crate::surface::{Backend, Surface};
use crate::surfaces;
use crate::surfaces::panel::center;
use crate::surfaces::toasts::{Act, Toasts};

impl App {
    /// The centre's state as the panel host has it, which every way a card
    /// opens or shuts reaches, then the slice's clock.
    pub(crate) fn tick_notifications(&mut self) {
        let open = self.center_open();
        if open != self.store.notifications.center_open {
            self.apply(Op::CenterOpen(open));
        }
        self.apply(Op::Tick(now_ms()));
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
    pub fn set_center(&mut self, open: bool, anchor: Option<f64>) {
        let now = Instant::now();
        if !open {
            if self.center_open() {
                self.close_panels();
            }
        } else if !self.center_open() {
            let anchor = anchor.or_else(|| live!(self).bar.center_anchor());
            self.open_host(Box::new(center::Center::default()), center::ID, crate::surfaces::panel::quick::Origin::Ipc, anchor, now);
        }
        self.tick_notifications();
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
        let chrome = self.chrome();
        let covered = chrome.bar_covered();
        let floating = self.store.compositor.snapshot.floating_outputs.contains(&live!(self).bar.output);
        let n = &self.store.notifications;
        let fading = self.toasts.as_ref().is_some_and(|t| t.holds_cards());
        let up = !n.model.popups.is_empty() || !surfaces::toasts::activity_groups(&self.store).is_empty() || fading;
        let want = !n.center_open;
        let layer = if covered && !floating && !up { Layer::Top } else { Layer::Overlay };
        // Placed off the same line as every panel: its insets, and beside a
        // side line, that line's side.
        let place = chrome.place(None);
        let mut insets = place.insets();
        let spec = place.corner(fs_info::notifications::position_spec(Some(crate::settings::NOTIFICATIONS_POSITION.get(&self.store.config))));
        let right = spec.right;
        let theme = &self.store.theme.theme;
        let cast = *self.toast_cast.get_or_insert_with(|| surfaces::toasts::column_cast(theme));
        let column = (right, surfaces::toasts::column_width(theme, cast, &insets, right));
        self.clear_of_cards(&mut insets, spec);
        // The column stays mapped between toasts (see above), so it is moved
        // to the live bar's output only when a toast arrives on an empty one.
        let stray = up && self.toasts.as_ref().is_some_and(|t| !t.holds_cards() && t.output.as_ref() != self.bar_wl());
        // The column is as tall as the output, which is unknown until its
        // first mode or logical size arrives; a scene made before that is
        // zero rows tall and never commits.
        let (_, out_h) = self.output_size();
        let resized = self.toasts.as_ref().is_some_and(|t| t.scene.size.h != out_h as i32);
        if !want || stray || resized || self.toasts.as_ref().is_some_and(|t| t.column != column) {
            if self.toasts.take().is_some() {
                crate::trace::mapped(crate::trace::Surface::Toasts, crate::trace::State::Unmapped);
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
            let mut surface = Surface::new("toasts", Backend::Gpu, ls, &self.shm, self.started, self.store.display.scales.primary());
            surface.wait_map = true;
            self.toasts = Some(Toasts::new(surface, (column.1 as i32, out_h as i32), column, layer, self.bar_wl().cloned()));
            self.toasts_dirty = true;
        }
        let qh = self.qh.clone();
        let (out_w, _) = self.output_size();
        let theme = &self.store.theme.theme;
        let Some(t) = &mut self.toasts else { return };
        if t.insets != insets {
            t.insets = insets;
            self.toasts_dirty = true;
        }
        let generation = |l: &Option<crate::paint::rim::Lit>| l.as_ref().map(|l| l.generation);
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
        if (self.toasts_dirty || animating || t.was_animating) && t.surface.mapped() {
            t.draw(&self.store, theme, &mut self.kit, &insets, spec, self.motion_scale, now);
            self.toasts_dirty = false;
            t.was_animating = animating;
        }
        let lifted = t.sync_layer(layer);
        let moved = t.sync_region(&self.compositor, covered);
        let passive = moved && covered;
        let animating = t.animating(now);
        if !up && !animating && !t.was_animating {
            // An empty column is one clear pixel: its output-tall canvas and
            // buffer go until the next toast.
            t.scene.take_damage();
            t.surface.present_clear((t.scene.size.w, t.scene.size.h), moved || lifted, &self.pixels, &self.shm, &qh);
        } else {
            if moved || lifted {
                t.scene.touch(fs_types::IRect::new(0, 0, 1, 1));
            }
            t.surface.present(&mut t.scene, animating, &qh);
        }
        if passive {
            crate::trace::mapped(crate::trace::Surface::Toasts, crate::trace::State::InputEmpty);
        }
        if lifted {
            crate::trace::note(format_args!("toasts layer {}", if layer == Layer::Top { "top" } else { "overlay" }));
        }
    }

    /// Moves the stack clear of the open panel's card where it crosses the
    /// stack's column: a stack at the top drops below it, one at the bottom
    /// rises above it. A card on the column's far half is the stack's to
    /// overlap, as one that left no room either way would be. Read off the
    /// card's resting rect, so the stack moves once rather than following
    /// an entrance.
    fn clear_of_cards(&self, insets: &mut Insets, spec: PositionSpec) {
        let out = self.output_size();
        if let Some(r) = self.panel.as_ref().filter(|h| h.is_open()).map(|h| h.win.card.rest_on(out)).filter(|r| !r.is_empty()) {
            self.clear_of(r, insets, spec);
        }
    }

    fn clear_of(&self, r: fs_types::IRect, insets: &mut Insets, spec: PositionSpec) {
        let theme = &self.store.theme.theme;
        let (out_w, out_h) = self.output_size();
        let (pad, width) = (theme.space.screen_padding, surfaces::toasts::card_width(theme));
        let x0 = if spec.left { insets.left + pad } else { out_w - insets.right - pad - width };
        if f64::from(r.right()) <= x0 || f64::from(r.x) >= x0 + width {
            return;
        }
        let mid = f64::from(r.y) + f64::from(r.h) / 2.0;
        // A panel that leaves no room past it for one bubble (its picture
        // slot and padding) is the stack's to overlap: a toast squashed out
        // of sight is worse.
        let bubble = theme.space.control_height + theme.space.panel_padding * 2.0;
        let room = |top: f64, bottom: f64| out_h - top - bottom - pad * 2.0 >= bubble;
        if spec.top && mid < out_h / 2.0 {
            let top = insets.top.max(f64::from(r.bottom()));
            if room(top, insets.bottom) {
                insets.top = top;
            }
        } else if !spec.top && mid >= out_h / 2.0 {
            let bottom = insets.bottom.max(out_h - f64::from(r.y));
            if room(insets.top, bottom) {
                insets.bottom = bottom;
            }
        }
    }

    pub fn toast_cards(&self) -> Vec<(String, fs_types::IRect)> {
        self.toasts.as_ref().map(|t| t.cards()).unwrap_or_default()
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
            let ids: Vec<_> = t.members().into_iter().collect();
            self.apply(Op::Hover(ids, expanded));
        }
    }

    /// The stack takes its whole gesture: a press and release on a card,
    /// and an axis frame, where a finger's horizontal travel swipes the
    /// card under it and a wheel does nothing. Its hover goes on to the
    /// shared handling.
    pub(super) fn toasts_pointer(&mut self, e: &PointerEvent, target: Option<Target>) -> bool {
        let (x, y) = e.position;
        let act = match e.kind {
            PointerEventKind::Press { .. } if target == Some(Target::Toasts) => {
                if let Some(t) = &mut self.toasts {
                    t.press(x, y);
                }
                self.pressed = Some((Target::Toasts, Press::Whole));
                return true;
            }
            PointerEventKind::Release { .. } if matches!(self.pressed, Some((Target::Toasts, _))) => {
                self.pressed = None;
                let Some(t) = &mut self.toasts else { return true };
                t.release(x, y)
            }
            PointerEventKind::Axis { time, horizontal, vertical, source } if target == Some(Target::Toasts) => {
                let finger = source == Some(wl_pointer::AxisSource::Finger);
                let lift = ((horizontal.stop || vertical.stop) && finger).then_some(time);
                let ms = Kind::Spatial.ms(&self.store.theme.theme) * self.motion_scale;
                let Some(t) = &mut self.toasts else { return true };
                let (moved, act) = t.swipe(x, y, crate::scroll::travel(&horizontal, source, time), lift, ms, Instant::now());
                if moved {
                    self.toasts_dirty = true;
                }
                act
            }
            _ => return false,
        };
        self.toast_act(act);
        true
    }

    fn toast_act(&mut self, act: Act) {
        if let Act::Activity(verb, source) = act {
            if let Ok(crate::services::activity::Done::Panel) = crate::services::activity::act(&self.store, self.runtime.as_ref(), verb, &source) {
                self.set_panel(PanelId::Activity, true, None);
                if let Some(panel) = self.panel_mut::<crate::surfaces::panel::activity::Activity>(PanelId::Activity) {
                    panel.select(&source);
                }
            }
            return;
        }
        let op = match act {
            Act::None | Act::Activity(..) => return,
            Act::Dismiss(ids) => Op::DismissPopups(ids),
            Act::Action(id, key) => Op::Pick(id, key),
            Act::Body(ids) => Op::Open(ids),
        };
        self.apply(op);
    }
}
