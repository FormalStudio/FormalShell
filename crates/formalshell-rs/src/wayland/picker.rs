//! The region picker's Wayland side: one Overlay surface per screen with
//! Exclusive keyboard focus while the picker is open, its pointer and keys,
//! the freeze decode, and the timers the capture family's watchdogs and
//! settles run on.

use std::time::{Duration, Instant};

use calloop::timer::{TimeoutAction, Timer};
use smithay_client_toolkit::reexports::client::protocol::wl_surface;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::App;
use crate::scene::Bitmap;
use crate::services::capture::Event;
use crate::store::Diff;
use crate::surface::Surface;
use crate::surfaces::capture::draw::Sheet;
use crate::surfaces::capture::picker::{R, Screen};
use crate::surfaces::capture::{self as cap};

const NAMESPACE: &str = "formalshell:capture";

pub struct PickerSurface {
    pub output: String,
    pub origin: (f64, f64),
    pub surface: Surface,
    pub sheet: Sheet,
    pub(super) dirty: bool,
}

#[derive(Default)]
pub struct Pickers {
    pub surfaces: Vec<PickerSurface>,
    /// Where the left button went down, in global coordinates, and whether
    /// it went down on a toolbar cell.
    press: Option<(f64, f64, Option<String>)>,
}

impl App {
    /// Runs `f` on the UI loop after `ms`.
    pub fn capture_after(&self, ms: u64, f: impl FnOnce(&mut App) + 'static) {
        let Some(handle) = &self.handle else { return };
        let mut f = Some(f);
        let _ = handle.insert_source(Timer::from_duration(Duration::from_millis(ms)), move |_, _, app: &mut App| {
            if let Some(f) = f.take() {
                f(app);
            }
            TimeoutAction::Drop
        });
    }

    /// Every output's logical box, in announce order.
    pub fn capture_screens(&self) -> Vec<Screen> {
        self.screen_outputs().into_iter().map(|(_, s)| s).collect()
    }

    /// [`App::capture_screens`] beside the output each one is.
    fn screen_outputs(&self) -> Vec<(smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput, Screen)> {
        self.outputs
            .outputs()
            .filter_map(|o| {
                let i = self.outputs.info(&o)?;
                let (x, y) = i.logical_position.unwrap_or((0, 0));
                let (w, h) = i.logical_size.or_else(|| i.modes.iter().find(|m| m.current).map(|m| m.dimensions))?;
                Some((o, Screen { name: i.name.clone().unwrap_or_default(), rect: R { x: x as f64, y: y as f64, w: w as f64, h: h as f64 } }))
            })
            .collect()
    }

    /// A freeze's PNG decoded on the pool, landing as an event.
    pub fn capture_decode(&self, generation: u64, output: String, path: String) {
        let Some(rt) = &self.runtime else { return };
        rt.service(move |ctx| {
            let job = ctx.pool().run(move || {
                let img = image::open(&path).ok()?.to_rgba8();
                let (w, h) = img.dimensions();
                Some(Bitmap::from_rgba(u16::try_from(w).ok()?, u16::try_from(h).ok()?, img.into_raw()))
            });
            let publisher = ctx.publisher().clone();
            ctx.spawn(async move {
                let frame = job.await.flatten();
                publisher.publish(Diff::Capture(Event::Frame { generation, output, frame }));
            });
        });
    }

    /// Maps or unmaps the picker's surfaces to match its state, and redraws.
    pub fn picker_sync(&mut self) {
        if !self.capture.picker.open {
            if !self.pickers.surfaces.is_empty() {
                self.pickers.surfaces.clear();
                self.log("capture picker unmapped");
            }
            return;
        }
        if self.pickers.surfaces.is_empty() {
            for (o, s) in self.screen_outputs() {
                let surface = self.compositor.create_surface(&self.qh);
                let ls = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Overlay, Some(NAMESPACE), Some(&o));
                ls.set_anchor(Anchor::all());
                ls.set_size(0, 0);
                ls.set_exclusive_zone(-1);
                ls.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
                ls.commit();
                let (w, h) = (s.rect.w as i32, s.rect.h as i32);
                self.pickers.surfaces.push(PickerSurface {
                    output: s.name,
                    origin: (s.rect.x, s.rect.y),
                    surface: Surface::on("capture", ls, &self.shm, self.started, &o),
                    sheet: Sheet::new(w, h),
                    dirty: true,
                });
            }
            self.log("capture picker mapped");
        }
        for p in &mut self.pickers.surfaces {
            p.dirty = true;
        }
    }

    pub(super) fn picker_owner(&self, surface: &wl_surface::WlSurface) -> Option<usize> {
        self.pickers.surfaces.iter().position(|p| p.surface.layer.wl_surface() == surface)
    }

    pub(super) fn picker_configure(&mut self, i: usize, width: i32, height: i32) {
        let Some(p) = self.pickers.surfaces.get_mut(i) else { return };
        let (w, h) = if width > 0 && height > 0 { (width, height) } else { (p.sheet.scene.size.w, p.sheet.scene.size.h) };
        if (p.sheet.scene.size.w, p.sheet.scene.size.h) != (w, h) {
            p.sheet = Sheet::new(w, h);
        }
        p.surface.configure(w, h);
        p.dirty = true;
    }

    pub(super) fn picker_frame(&mut self, i: usize) {
        let Some(p) = self.pickers.surfaces.get_mut(i) else { return };
        let s = &mut p.surface;
        (s.frame_pending, s.mapped, s.callbacks) = (false, true, s.callbacks + 1);
        p.dirty = true;
    }

    pub(super) fn picker_closed(&mut self, i: usize) {
        if i < self.pickers.surfaces.len() {
            self.pickers.surfaces.remove(i);
        }
    }

    pub(super) fn present_picker(&mut self, now: Instant) {
        if self.pickers.surfaces.is_empty() {
            return;
        }
        let c = cap::cands(self);
        let qh = self.qh.clone();
        let theme = &self.store.theme.theme;
        let kit = &mut self.bar.kit;
        for p in &mut self.pickers.surfaces {
            if !p.dirty {
                continue;
            }
            let _raster = crate::scene::raster_at(p.surface.scale());
            let frame = self.capture.picker.frames.get(&p.output).and_then(|f| f.as_ref());
            let animating = p.sheet.draw(&self.capture.picker, &c, p.origin, frame, theme, kit, self.motion_scale, now);
            p.dirty = animating;
            p.surface.present(&mut p.sheet.scene, animating, &qh);
        }
    }

    pub(super) fn picker_pointer(&mut self, i: usize, e: &PointerEvent) {
        let Some(p) = self.pickers.surfaces.get(i) else { return };
        let (ox, oy) = p.origin;
        let (lx, ly) = e.position;
        let (gx, gy) = (lx + ox, ly + oy);
        let hit = p.sheet.toolbar.hit(lx, ly).and_then(|h| h.on.clone());
        match e.kind {
            PointerEventKind::Enter { serial } => {
                if let Some(device) = &self.cursor_device {
                    device.set_shape(serial, Shape::Crosshair);
                }
                self.cursor = Some((serial, Shape::Crosshair));
                self.picker_motion(i, lx, ly, gx, gy, hit);
            }
            PointerEventKind::Motion { .. } => self.picker_motion(i, lx, ly, gx, gy, hit),
            PointerEventKind::Press { button, .. } => {
                if button == 0x111 {
                    cap::picker_close(self, "cancelled by right-click");
                    return;
                }
                self.pickers.press = Some((gx, gy, hit.clone()));
                if hit.is_none() && (self.capture.picker.mode == "smart" || self.capture.picker.mode == "region") {
                    self.capture.picker.drag = Some(R { x: gx, y: gy, w: 0.0, h: 0.0 });
                }
                self.picker_sync();
            }
            PointerEventKind::Release { .. } => {
                let Some((_, _, on)) = self.pickers.press.take() else { return };
                if let Some(on) = on {
                    if hit.as_deref() == Some(on.as_str()) {
                        self.picker_action(&on);
                    }
                    return;
                }
                let drag = self.capture.picker.drag.take();
                match drag {
                    Some(d) if d.w * d.h >= 20.0 => cap::finish_rect(self, d),
                    _ => {
                        let c = cap::cands(self);
                        self.capture.picker.hover = self.capture.picker.resolve_at(&c, gx, gy);
                        if self.capture.picker.hover.is_some() {
                            cap::picker_commit(self, false);
                        }
                    }
                }
                self.picker_sync();
            }
            _ => {}
        }
    }

    fn picker_motion(&mut self, i: usize, lx: f64, ly: f64, gx: f64, gy: f64, hit: Option<String>) {
        if let Some(p) = self.pickers.surfaces.get_mut(i) {
            let path = p.sheet.toolbar.hit(lx, ly).map(|h| h.path.clone());
            if p.sheet.toolbar.hover != path {
                p.sheet.toolbar.hover = path;
            }
        }
        if let Some((px, py, None)) = self.pickers.press.clone()
            && self.capture.picker.drag.is_some()
        {
            self.capture.picker.drag = Some(R { x: px.min(gx), y: py.min(gy), w: (gx - px).abs(), h: (gy - py).abs() });
        } else if hit.is_none() && self.pickers.press.is_none() {
            let c = cap::cands(self);
            self.capture.picker.cursor = -1;
            self.capture.picker.hover = self.capture.picker.resolve_at(&c, gx, gy);
        }
        self.picker_sync();
    }

    fn picker_action(&mut self, on: &str) {
        if on == "commit" {
            cap::picker_commit(self, false);
        } else if let Some(i) = on.strip_prefix("tool:").and_then(|i| i.parse::<i32>().ok()) {
            let c = cap::cands(self);
            self.capture.picker.set_tool(&c, i);
        }
        self.picker_sync();
    }

    /// True when the picker took the key.
    pub(super) fn picker_key_event(&mut self, event: &KeyEvent) -> bool {
        if !self.capture.picker.open {
            return false;
        }
        let name = match event.keysym {
            Keysym::Escape => "escape",
            Keysym::Return | Keysym::KP_Enter => {
                if self.mods.ctrl {
                    "ctrl-return"
                } else {
                    "return"
                }
            }
            Keysym::Tab => "tab",
            Keysym::ISO_Left_Tab => "shift-tab",
            Keysym::Left => "left",
            Keysym::Right => "right",
            Keysym::Up => "up",
            Keysym::Down => "down",
            Keysym::_1 => "1",
            Keysym::_2 => "2",
            Keysym::_3 => "3",
            Keysym::_4 => "4",
            Keysym::_5 => "5",
            Keysym::_6 => "6",
            _ => return true,
        };
        cap::picker_key(self, name);
        true
    }
}
