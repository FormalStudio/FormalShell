//! The overview's window, captures and input; the model, its phases and
//! the drawing are `surfaces::overview`.
//!
//! One `formalshell:overview` layer on the Top layer of the focused output,
//! with exclusive zone 0, so the bar keeps its band (and the hot corners in
//! it) above and beside it. It takes the keyboard Exclusive while open.
//! The surface maps clear and the zoom waits on the current workspace's
//! frames (or `ov::WAIT`), so it starts on the live desktop; the close focuses
//! the selection first and unmaps once it is back on it.

use std::collections::HashMap;
use std::time::Instant;

use fs_types::{WindowId, WorkspaceId};
use fs_theme::color::Rgba;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape;
use smithay_client_toolkit::seat::keyboard::{KeyEvent, Keysym};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{Anchor, KeyboardInteractivity, Layer};

use super::App;
use super::capture::{self, Owner};
use crate::motion::Curve;
use crate::scene::{Bitmap, Scene};
use crate::services::{appicon, compositor, wallpaper};
use crate::surface::{Backend, Surface};
use crate::surfaces::overview::{self as ov, Focus, Geometry, Overview, Pace, Phase, Release, Sel, Step};

/// The backdrop's picture is the wallpaper at this fraction of its size.
const BACKDROP_DOWN: u32 = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Close,
    Toggle,
}

#[derive(Default)]
pub struct State {
    pub model: Overview,
    pub(super) win: Option<(Surface, Scene, WlOutput)>,
    pub dirty: bool,
    /// The wallpaper picture's address and the bitmap made of it.
    wallpaper: Option<(usize, Bitmap, Bitmap)>,
    /// The keyboard is held Exclusive until the zoom starts, then
    /// OnDemand: Hyprland sends the pointer to nothing but an Exclusive
    /// layer, which would keep it off the bar and the hot corners.
    primed: bool,
}

impl State {
    pub fn owns(&self, surface: &WlSurface) -> bool {
        self.win.as_ref().is_some_and(|(s, _, _)| s.layer.wl_surface() == surface)
    }
}

fn icon_key(id: &WindowId) -> String {
    format!("overview:{id}")
}

impl App {
    fn overview_pace(&self) -> Pace {
        let m = self.store.theme.theme.motion();
        Pace { ms: m.arrive * self.motion_scale, curve: Curve::from_table(&m.arrive_curve) }
    }

    pub fn overview_action(&mut self, action: Action) {
        let open = self.overview.model.holding();
        match action {
            Action::Open if !open => self.overview_open(),
            Action::Close if open => self.overview_close(),
            Action::Toggle if open => self.overview_close(),
            Action::Toggle => self.overview_open(),
            _ => {}
        }
    }

    fn overview_output(&self) -> Option<(WlOutput, String, (f64, f64), (f64, f64))> {
        let focused = &self.store.compositor.snapshot.focused_output_name;
        let name = if focused.is_empty() { self.bar_output_name() } else { focused.clone() };
        let output = self.outputs.outputs().find(|o| self.output_name(o) == name).or_else(|| self.bar_wl().cloned())?;
        let info = self.outputs.info(&output)?;
        let origin = info.logical_position.map_or((0.0, 0.0), |(x, y)| (f64::from(x), f64::from(y)));
        let size = info.logical_size.map(|(w, h)| (f64::from(w), f64::from(h)))?;
        Some((output, name, origin, size))
    }

    /// The rows of output `name`, wherever focus is.
    fn overview_rows(&self, name: &str) -> Vec<ov::Row> {
        let output = self.outputs.outputs().find(|o| self.output_name(o) == name);
        let origin = output.and_then(|o| self.outputs.info(&o)?.logical_position).map_or((0.0, 0.0), |(x, y)| (f64::from(x), f64::from(y)));
        ov::rows(&self.store.compositor.snapshot, name, origin)
    }

    fn overview_open(&mut self) {
        let now = Instant::now();
        let pace = self.overview_pace();
        if self.overview.model.reopen(now, pace.ms) {
            self.overview_keyboard(KeyboardInteractivity::Exclusive);
            self.overview.primed = false;
            self.overview.dirty = true;
            return;
        }
        let Some((output, name, _, size)) = self.overview_output() else {
            eprintln!("overview: no output to open on");
            return;
        };
        let rows = self.overview_rows(&name);
        let snap = &self.store.compositor.snapshot;
        let sel = Sel::of(&rows, &snap.focused_workspace_id, &snap.focused_window_id);
        self.overview.model.begin(name, rows, sel, now, pace.curve);

        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Top, Some("formalshell:overview"), Some(&output));
        layer.set_anchor(Anchor::all());
        layer.set_size(0, 0);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();
        let mut surface = Surface::new("overview", Backend::Cpu, layer, &self.shm, self.started, self.store.display.scales.on(&output));
        surface.wait_map = true;
        self.overview.win = Some((surface, Scene::clear(size.0 as i32, size.1 as i32), output));
        compositor::send(compositor::Command::RefreshWindows);
        self.overview_capture();
        self.overview_probe();
        self.overview.dirty = true;
        crate::trace::mapped(crate::trace::Surface::Overview, crate::trace::State::Opening);
    }

    fn overview_keyboard(&self, k: KeyboardInteractivity) {
        if let Some((s, _, _)) = &self.overview.win {
            s.layer.set_keyboard_interactivity(k);
            s.layer.commit();
        }
    }

    /// Focuses the selection and zooms back onto it.
    fn overview_close(&mut self) {
        let now = Instant::now();
        let pace = self.overview_pace();
        let closing = self.overview.model.close(now, pace.ms);
        match &closing.focus {
            Some(Focus::Window(id)) => compositor::focus_window(id),
            Some(Focus::WorkspaceAt(idx)) => compositor::focus_workspace_at(*idx),
            Some(Focus::Workspace(id)) => compositor::focus_workspace(id),
            None => {}
        }
        if closing.unmap {
            self.overview_unmap();
            return;
        }
        self.overview_keyboard(KeyboardInteractivity::None);
        self.overview.dirty = true;
        crate::trace::mapped(crate::trace::Surface::Overview, crate::trace::State::Closing);
    }

    fn overview_unmap(&mut self) {
        self.overview.win = None;
        self.overview.primed = false;
        self.overview.model.unmapped();
        self.capture_clear(Owner::Overview);
        crate::trace::mapped(crate::trace::Surface::Overview, crate::trace::State::Unmapped);
    }

    fn overview_current(&self) -> WorkspaceId {
        let m = &self.overview.model;
        m.rows().get(m.current_row()).map(|r| r.workspace_id.clone()).unwrap_or_default()
    }

    fn overview_capture(&mut self) {
        let wants = self.overview.model.thumb_sizes(&self.overview_current());
        let scale = self.overview.win.as_ref().map_or_else(|| self.store.display.scales.primary(), |(s, _, _)| s.scale());
        self.capture_set(Owner::Overview, &wants, false, scale);
    }

    fn overview_icon_edge(&self) -> u32 {
        (self.store.theme.theme.space.xl * 2.0).round() as u32
    }

    fn overview_probe(&self) {
        let scale = self.overview.win.as_ref().map_or_else(|| self.store.display.scales.primary(), |(s, _, _)| s.scale());
        let ids: Vec<&WindowId> = self.overview.model.rows().iter().flat_map(|r| r.windows.iter().map(|w| &w.id)).collect();
        let queries = ids
            .iter()
            .filter_map(|id| self.store.compositor.window(id))
            .map(|w| appicon::Query {
                id: icon_key(&w.id),
                app_id: w.app_id.clone(),
                initial_class: w.initial_class.clone(),
                initial_title: w.initial_title.clone(),
                pid: w.pid,
            })
            .collect();
        appicon::probe(self.overview_icon_edge(), scale, queries);
    }

    /// The compositor or an icon moved under it.
    pub(super) fn overview_changed(&mut self, topic: crate::store::Topic) {
        use crate::store::Topic;
        if topic == Topic::Compositor {
            let rows = self.overview_rows(self.overview.model.output());
            let pace = self.overview_pace();
            if let Some(stale) = self.overview.model.reread(rows, Instant::now(), pace)
                && self.overview.model.phase() != Phase::Closing
            {
                self.overview_capture();
                for id in &stale {
                    self.capture_refresh(Owner::Overview, id);
                }
                self.overview_probe();
            }
        }
        self.overview.dirty = true;
    }

    pub fn overview_thumbs_changed(&mut self) {
        self.overview.dirty = true;
    }

    pub fn overview_status(&self) -> serde_json::Value {
        let captured = |id: &WindowId| self.capture_image(Owner::Overview, id).is_some();
        let mut v = self.overview.model.status(Instant::now(), &captured);
        v["capturing"] = self.capture_sourced(Owner::Overview).into();
        v
    }

    pub fn overview_deadline(&self) -> Option<Instant> {
        self.overview.model.deadline()
    }

    pub fn overview_frame(&mut self) {
        let Some((s, _, _)) = &mut self.overview.win else { return };
        if s.landed(Instant::now()) {
            crate::trace::mapped(crate::trace::Surface::Overview, crate::trace::State::Mapped);
        }
        self.overview.dirty = true;
    }

    pub fn overview_configure(&mut self, width: i32, height: i32) {
        let Some((s, scene, _)) = &mut self.overview.win else { return };
        if (scene.size.w, scene.size.h) != (width, height) && width > 0 && height > 0 {
            scene.resize(width, height);
        }
        s.configure(scene.size.w, scene.size.h);
        let area = (f64::from(scene.size.w), f64::from(scene.size.h));
        let output = self.overview_output().map_or(area, |o| o.3);
        let offset = self.chrome().offset(output, area);
        let pace = self.overview_pace();
        self.overview.model.lay_out(Geometry::new(area, output, offset), Instant::now(), pace);
        self.overview.dirty = true;
    }

    pub fn overview_closed(&mut self) {
        self.overview_unmap();
    }

    /// The output's wallpaper as one bitmap for the cards and a small one
    /// for the backdrop, both made once per picture off the one the
    /// wallpaper surface already holds; or its flat colour.
    fn overview_wallpaper(&mut self) -> (Option<(Bitmap, Bitmap)>, Option<Rgba>) {
        let wanted = self.store.desktop();
        if let Some([r, g, b]) = wallpaper::flat(&wanted) {
            return (None, Some(Rgba::hex(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))));
        }
        let Some((_, _, output)) = &self.overview.win else { return (None, None) };
        let Some((pw, ph)) = self.bars.heads.iter().find(|h| &h.output == output).and_then(|h| h.backdrop.as_ref()?.pixels()) else {
            return (None, None);
        };
        let Some(picture) = self.store.wallpaper.sized(pw as u32, ph as u32).filter(|p| p.path == wanted).cloned() else {
            return (None, None);
        };
        let at = std::sync::Arc::as_ptr(&picture) as usize;
        if let Some((seen, card, back)) = &self.overview.wallpaper
            && *seen == at
        {
            return (Some((card.clone(), back.clone())), None);
        }
        let size = (picture.width, picture.height);
        let card = capture::fit(&picture.bgra, size, true, size);
        // Averaged down this far and drawn back up at the output's size,
        // the picture reads as blurred without a blur pass.
        let back = capture::fit(&picture.bgra, size, true, ((size.0 / BACKDROP_DOWN).max(1), (size.1 / BACKDROP_DOWN).max(1)));
        self.overview.wallpaper = Some((at, card.clone(), back.clone()));
        (Some((card, back)), None)
    }

    pub fn overview_present(&mut self) {
        let now = Instant::now();
        let ready = self.overview.win.as_ref().is_some_and(|(s, _, _)| s.configured && s.mapped());
        if ready && self.overview.model.phase() == Phase::Opening {
            let landed = self.overview.model.current_windows().all(|id| self.capture_image(Owner::Overview, id).is_some());
            let ms = self.overview_pace().ms;
            if self.overview.model.start_zoom(now, ms, landed) {
                self.overview.dirty = true;
                crate::trace::mapped(crate::trace::Surface::Overview, crate::trace::State::Zooming);
            }
        }
        if self.overview.model.phase() == Phase::Open && ready && !self.overview.primed {
            self.overview.primed = true;
            self.overview_keyboard(KeyboardInteractivity::OnDemand);
        }
        if self.overview.model.closed_out(now) {
            self.overview_unmap();
            return;
        }
        let animating = self.overview.model.animating(now);
        let drawable = self.overview.win.as_ref().is_some_and(|(s, _, _)| s.configured && !s.frame_pending());
        if !drawable || !(self.overview.dirty || animating) {
            return;
        }
        self.overview.dirty = false;
        let (picture, flat) = self.overview_wallpaper();
        let scale = self.overview.win.as_ref().map_or(1.0, |(s, _, _)| s.scale());
        let edge = self.overview_icon_edge();
        let mut thumbs = HashMap::new();
        let mut icons = HashMap::new();
        for w in self.overview.model.rows().iter().flat_map(|r| r.windows.iter()) {
            if let Some(b) = self.capture_image(Owner::Overview, &w.id) {
                thumbs.insert(w.id.clone(), b.clone());
            }
            if let Some(b) = self.store.appicon.get(&icon_key(&w.id), edge, scale).and_then(|i| i.image) {
                icons.insert(w.id.clone(), b);
            }
        }
        let input = ov::Input { theme: &self.store.theme.theme, wallpaper: picture.as_ref().map(|p| &p.0), backdrop: picture.as_ref().map(|p| &p.1), flat, thumbs: &thumbs, icons: &icons, now };
        let Some((surface, scene, _)) = &mut self.overview.win else { return };
        self.overview.model.draw(&input, &mut self.kit, scene);
        let qh = self.qh.clone();
        surface.present(scene, animating, &qh);
    }

    /// A key while it holds the keyboard.
    pub fn overview_key(&mut self, event: &KeyEvent) -> bool {
        if !self.overview.model.holding() {
            return false;
        }
        let step = match event.keysym {
            Keysym::Up | Keysym::k => Step::Row(-1),
            Keysym::Down | Keysym::j => Step::Row(1),
            Keysym::Left | Keysym::h => Step::Win(-1),
            Keysym::Right | Keysym::l => Step::Win(1),
            Keysym::Home => Step::Edge(false),
            Keysym::End => Step::Edge(true),
            Keysym::Return | Keysym::KP_Enter | Keysym::space | Keysym::Escape => {
                self.overview_close();
                return true;
            }
            _ => return true,
        };
        let pace = self.overview_pace();
        self.overview.model.step(step, Instant::now(), pace);
        self.overview.dirty = true;
        true
    }

    /// Pointer events on its surface; false for any other surface's.
    pub fn overview_pointer(&mut self, e: &PointerEvent) -> bool {
        if !self.overview.owns(&e.surface) {
            return false;
        }
        let now = Instant::now();
        let at = e.position;
        let pace = self.overview_pace();
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.cursor = Some((serial, Shape::Default));
                if let Some(device) = &self.cursor_device {
                    device.set_shape(serial, Shape::Default);
                }
            }
            PointerEventKind::Leave { .. } => {}
            PointerEventKind::Motion { .. } => {
                if self.overview.model.drag_to(at, now) {
                    self.overview.dirty = true;
                }
            }
            PointerEventKind::Press { button, .. } if button == crate::input::BTN_LEFT => self.overview.model.press(at, now),
            PointerEventKind::Release { button: crate::input::BTN_LEFT, .. } => match self.overview.model.release(at, now, pace) {
                Release::Nothing => {}
                Release::Dropped(moved) => {
                    if let Some((id, idx)) = moved {
                        let output = self.overview.model.output().to_owned();
                        compositor::send(compositor::Command::MoveWindowTo { id: id.clone(), idx, output });
                        compositor::send(compositor::Command::RefreshWindows);
                        crate::trace::note(format_args!("overview drop {id} on {idx}"));
                    }
                    self.overview.dirty = true;
                }
                Release::Close => {
                    self.overview.dirty = true;
                    self.overview_close();
                }
            },
            PointerEventKind::Axis { vertical, horizontal, .. } => {
                let (along, by) = if horizontal.absolute != 0.0 || horizontal.discrete != 0 {
                    (true, if horizontal.discrete != 0 { f64::from(horizontal.discrete) * ov::SCROLL_STEP } else { horizontal.absolute })
                } else {
                    (self.mods.shift, if vertical.discrete != 0 { f64::from(vertical.discrete) * ov::SCROLL_STEP } else { vertical.absolute })
                };
                if self.overview.model.wheel(along, by, now, pace) {
                    self.overview.dirty = true;
                }
            }
            _ => {}
        }
        true
    }
}
