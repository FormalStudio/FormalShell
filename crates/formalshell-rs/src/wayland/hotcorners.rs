//! Hot corners: a transparent `hotCorners.size` square on
//! the top layer in every corner whose action is not "none", firing after a
//! dwell or on a click. Arming is fs-chrome's `arm` rule: an action that
//! reports its end (the lock, the screensaver) ends when its covering
//! surface unmaps, and the corner is told whether the compositor's cursor
//! sits on it at that moment, so a cursor parked in the corner through an
//! unlock never fires it again.
//!
//! The screensaver corner locks the session and shows the screensaver inside
//! the lock, so like the lock corner it ends at the unlock.

use std::collections::HashMap;
use std::time::Duration;

use calloop::RegistrationToken;
use calloop::timer::{TimeoutAction, Timer};
use fs_chrome::hot_corners::arm::{self, ArmState};
use fs_chrome::hot_corners::corners::{self, Action, Config, Corner};
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerSurface};

use super::App;
use super::lock::LockMsg;
use crate::services::compositor;
use crate::surface::Ignore;

const NAMESPACE: &str = "formalshell:hotcorner";

struct Win {
    output: WlOutput,
    corner: Corner,
    action: Action,
    layer: LayerSurface,
    viewport: WpViewport,
    buffer: Option<WlBuffer>,
    arm: ArmState,
    dwell: Option<RegistrationToken>,
}

impl Drop for Win {
    fn drop(&mut self) {
        self.viewport.destroy();
        if let Some(b) = self.buffer.take() {
            b.destroy();
        }
    }
}

#[derive(Default)]
pub struct HotCorners {
    config: Option<Config>,
    /// What the windows were built from, so an unchanged model keeps them.
    key: Vec<(String, Corner, String, i64)>,
    wins: Vec<Win>,
    /// When each action last ended, on [`App::corner_ms`]'s clock.
    ended_at: HashMap<String, i64>,
}

impl App {
    /// A monotonic millisecond clock that never reads 0, which `arm` takes
    /// to mean "never".
    fn corner_ms(&self) -> i64 {
        self.started.elapsed().as_millis() as i64 + 1_000_000
    }

    fn action_active(&self, action: &Action) -> bool {
        match action {
            Action::Lock => self.lock.locked && !self.lock_external(),
            Action::Screensaver => self.saver.active || self.saver_mapped() || (self.lock.locked && !self.lock_external()),
            _ => false,
        }
    }

    /// Builds the corners the config and the output list ask for.
    pub(super) fn sync_hot_corners(&mut self) {
        if !self.store.config.loaded {
            return;
        }
        let config = corners::resolve(self.store.config.get("hotCorners"));
        if self.hot.config.as_ref().map(|c| &c.warnings) != Some(&config.warnings) {
            for w in &config.warnings {
                eprintln!("HotCorners: {w}");
            }
        }
        let outputs: Vec<(WlOutput, String)> =
            self.outputs.outputs().map(|o| (o.clone(), self.outputs.info(&o).and_then(|i| i.name).unwrap_or_default())).collect();
        let names: Vec<String> = outputs.iter().map(|(_, n)| n.clone()).collect();
        // Down while a fullscreen window covers the output, so its scanout
        // is free and a corner cannot fire mid-game.
        let hide = self.store.config.bool("fullscreen.hideChrome").unwrap_or(true);
        let covered = &self.store.compositor.snapshot.fullscreen_outputs;
        let windows: Vec<_> =
            corners::windows(&config, &names).into_iter().filter(|w| !(hide && covered.contains(&w.screen))).collect();
        let key: Vec<(String, Corner, String, i64)> =
            windows.iter().map(|w| (w.screen.clone(), w.corner, w.action.as_str().to_owned(), config.size)).collect();
        self.hot.config = Some(config.clone());
        if key == self.hot.key {
            return;
        }
        self.hot.key = key;
        self.hot.wins.clear();
        let now = self.corner_ms();
        for w in windows {
            let Some((output, _)) = outputs.iter().find(|(_, n)| *n == w.screen) else { continue };
            let edges = corners::edges(w.corner);
            let mut anchor = Anchor::empty();
            for (on, a) in [(edges.top, Anchor::TOP), (edges.bottom, Anchor::BOTTOM), (edges.left, Anchor::LEFT), (edges.right, Anchor::RIGHT)] {
                if on {
                    anchor |= a;
                }
            }
            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Top, Some(NAMESPACE), Some(output));
            layer.set_anchor(anchor);
            layer.set_size(config.size as u32, config.size as u32);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.commit();
            let viewport = self.pixels.viewporter.get_viewport(layer.wl_surface(), &self.qh, Ignore);
            let ended = self.hot.ended_at.get(w.action.as_str()).copied().unwrap_or(0);
            let state = arm::adopt(now, self.action_active(&w.action), ended);
            self.hot.wins.push(Win { output: output.clone(), corner: w.corner, action: w.action, layer, viewport, buffer: None, arm: state, dwell: None });
        }
    }

    pub(super) fn hot_corner_owns(&self, layer: &LayerSurface) -> bool {
        self.hot.wins.iter().any(|w| w.layer.wl_surface() == layer.wl_surface())
    }

    /// Mapped with one transparent pixel stretched over the square; its
    /// default input region is the whole square.
    pub(super) fn hot_corner_configure(&mut self, layer: &LayerSurface) {
        let size = self.hot.config.as_ref().map_or(corners::DEFAULT_SIZE, |c| c.size) as i32;
        let Some(w) = self.hot.wins.iter_mut().find(|w| w.layer.wl_surface() == layer.wl_surface()) else { return };
        if w.buffer.is_some() {
            return;
        }
        let buffer = self.pixels.single_pixel.create_u32_rgba_buffer(0, 0, 0, 0, &self.qh, Ignore);
        w.viewport.set_destination(size, size);
        let surface = w.layer.wl_surface();
        surface.attach(Some(&buffer), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        w.layer.commit();
        w.buffer = Some(buffer);
    }

    /// Takes the events on a corner and hands back the rest.
    pub(super) fn hot_corner_pointer(&mut self, events: &[PointerEvent]) -> Vec<PointerEvent> {
        let mut rest = Vec::new();
        for e in events {
            let Some(i) = self.corner_of(&e.surface) else {
                rest.push(e.clone());
                continue;
            };
            let now = self.corner_ms();
            match e.kind {
                PointerEventKind::Enter { .. } => {
                    let w = &self.hot.wins[i];
                    let armed = arm::is_armed(w.arm, now);
                    let active = self.action_active(&w.action);
                    self.hot.wins[i].arm = arm::on_enter(w.arm, now);
                    if armed && !active {
                        self.start_dwell(i);
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.stop_dwell(i);
                    let w = &mut self.hot.wins[i];
                    w.arm = arm::on_exit(w.arm);
                }
                PointerEventKind::Release { .. } => self.fire_corner(i),
                _ => {}
            }
        }
        rest
    }

    fn corner_of(&self, surface: &WlSurface) -> Option<usize> {
        self.hot.wins.iter().position(|w| w.layer.wl_surface() == surface)
    }

    fn start_dwell(&mut self, i: usize) {
        self.stop_dwell(i);
        let delay = self.hot.config.as_ref().map_or(corners::DEFAULT_DELAY_MS, |c| c.delay_ms);
        let Some(handle) = &self.handle else { return };
        let surface = self.hot.wins[i].layer.wl_surface().clone();
        let token = handle.insert_source(Timer::from_duration(Duration::from_millis(delay as u64)), move |_, _, app: &mut App| {
            if let Some(i) = app.corner_of(&surface) {
                app.hot.wins[i].dwell = None;
                app.fire_corner(i);
            }
            TimeoutAction::Drop
        });
        self.hot.wins[i].dwell = token.ok();
    }

    fn stop_dwell(&mut self, i: usize) {
        if let (Some(handle), Some(t)) = (&self.handle, self.hot.wins[i].dwell.take()) {
            handle.remove(t);
        }
    }

    fn fire_corner(&mut self, i: usize) {
        self.stop_dwell(i);
        let now = self.corner_ms();
        let external = self.lock_external();
        let action = self.hot.wins[i].action.clone();
        let reports = arm::reports_end(&action, external);
        self.hot.wins[i].arm = arm::on_fire(self.hot.wins[i].arm, now, reports);
        if self.action_active(&action) {
            return;
        }
        if !reports {
            self.hot.ended_at.insert(action.as_str().to_owned(), now);
        }
        match &action {
            Action::Lock => {
                let _ = self.lock();
            }
            // The screensaver inside a lock of this shell's own: away from
            // the machine with it left awake.
            Action::Screensaver => {
                let _ = self.lock();
                if !external {
                    self.saver_start();
                }
            }
            other => eprintln!("HotCorners: {} has no surface in this shell yet", other.as_str()),
        }
    }

    /// The covering surface of `action` has unmapped: every corner running
    /// it hears whether the compositor's cursor is on it now. The screensaver
    /// calls this from its own unmap.
    pub fn hot_corner_action_ended(&mut self, action: &str) {
        let at = self.corner_ms();
        self.hot.ended_at.insert(action.to_owned(), at);
        let (Some(rt), Some(tx)) = (&self.runtime, self.lock.tx.clone()) else { return };
        let action = action.to_owned();
        rt.service(move |ctx| {
            ctx.spawn(async move {
                let pos = compositor::cursor_pos().await;
                let _ = tx.send(LockMsg::ActionEnded { action, at, cursor: pos });
            });
        });
    }

    /// The cursor position the end was judged on. With none (the socket
    /// failed), a corner is taken to still hold the cursor, which asks for
    /// a real leave rather than inventing one.
    pub(super) fn hot_corner_ended(&mut self, action: &str, at: i64, cursor: Option<(f64, f64)>) {
        let size = self.hot.config.as_ref().map_or(corners::DEFAULT_SIZE, |c| c.size) as f64;
        for i in 0..self.hot.wins.len() {
            if self.hot.wins[i].action.as_str() != action {
                continue;
            }
            let inside = match (cursor, self.outputs.info(&self.hot.wins[i].output)) {
                (Some((x, y)), Some(info)) => {
                    let (ox, oy) = (info.logical_position.unwrap_or((0, 0)).0 as f64, info.logical_position.unwrap_or((0, 0)).1 as f64);
                    let (ow, oh) = info.logical_size.map_or((0.0, 0.0), |(w, h)| (w as f64, h as f64));
                    let edges = corners::edges(self.hot.wins[i].corner);
                    let x0 = if edges.right { ox + ow - size } else { ox };
                    let y0 = if edges.bottom { oy + oh - size } else { oy };
                    x >= x0 && x < x0 + size && y >= y0 && y < y0 + size
                }
                _ => true,
            };
            let w = &mut self.hot.wins[i];
            w.arm = arm::on_action_end(w.arm, at, inside);
        }
    }
}
