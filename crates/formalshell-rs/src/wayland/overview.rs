//! The overview's window, clocks, captures and input; the model and the
//! drawing are `surfaces::overview`.
//!
//! One `formalshell:overview` layer on the Top layer of the focused output,
//! with exclusive zone 0, so the bar keeps its band (and the hot corners in
//! it) above and beside it. It takes the keyboard Exclusive while open.
//! The surface maps clear and the zoom waits on the current workspace's
//! frames (or `WAIT`), so it starts on the live desktop; the close focuses
//! the selection first and unmaps once it is back on it.

use std::cell::Cell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use fs_chrome::types::Edge;
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
use crate::motion::{Animated, Curve};
use crate::scene::{Bitmap, Scene};
use crate::services::{appicon, compositor, wallpaper};
use crate::surface::Surface;
use crate::surfaces::overview::{self as ov, Drag, Geometry, Overview, Phase, Sel};

/// The longest the zoom waits for the current workspace's frames.
const WAIT: Duration = Duration::from_millis(150);
/// Scroll travel (wl_pointer axis units) that moves the selection one step.
const SCROLL_STEP: f64 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open,
    Close,
    Toggle,
}

thread_local! {
    /// An action asked for from inside another surface's own pass (the
    /// minimap's long press), run on the overview's next present.
    static ASKED: Cell<Option<Action>> = const { Cell::new(None) };
}

/// Asks for `action` from code that has no hold on the App.
pub fn ask(action: Action) {
    ASKED.with(|a| a.set(Some(action)));
}

#[derive(Default)]
pub struct State {
    pub model: Overview,
    win: Option<(Surface, Scene, WlOutput)>,
    pub dirty: bool,
    /// The wallpaper picture's address and the bitmap made of it.
    wallpaper: Option<(usize, Bitmap)>,
    scroll: f64,
    /// The press, before it lifts or lets go: the row and window under it.
    pressed: Option<(usize, Option<usize>)>,
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

fn icon_key(id: &str) -> String {
    format!("overview:{id}")
}

impl App {
    fn overview_curve(&self) -> (f64, Curve) {
        let m = self.store.theme.theme.motion();
        (m.arrive * self.motion_scale, Curve::from_table(&m.arrive_curve))
    }

    pub fn overview_action(&mut self, action: Action) {
        let open = matches!(self.overview.model.phase, Phase::Opening | Phase::Open);
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
        let output = self.outputs.outputs().find(|o| self.output_name(o) == name).or_else(|| self.bar_wl.clone())?;
        let info = self.outputs.info(&output)?;
        let origin = info.logical_position.map_or((0.0, 0.0), |(x, y)| (f64::from(x), f64::from(y)));
        let size = info.logical_size.map(|(w, h)| (f64::from(w), f64::from(h)))?;
        Some((output, name, origin, size))
    }

    /// The rows of the output the overview opened on, wherever focus is.
    fn overview_rows(&self) -> Vec<ov::Row> {
        let name = &self.overview.model.output;
        let output = self.outputs.outputs().find(|o| self.output_name(o) == *name);
        let origin = output.and_then(|o| self.outputs.info(&o)?.logical_position).map_or((0.0, 0.0), |(x, y)| (f64::from(x), f64::from(y)));
        ov::rows(&self.store.compositor.snapshot, &self.overview.model.output, origin)
    }

    fn overview_open(&mut self) {
        let now = Instant::now();
        let (ms, curve) = self.overview_curve();
        if self.overview.model.phase == Phase::Closing {
            // Back out of a close still running: the stack zooms out again.
            self.overview.model.phase = Phase::Open;
            if let Some(z) = &mut self.overview.model.zoom {
                z.set(now, 1.0, ms);
            }
            self.overview_keyboard(KeyboardInteractivity::Exclusive);
            self.overview.primed = false;
            self.overview.dirty = true;
            return;
        }
        let Some((output, name, _, size)) = self.overview_output() else {
            eprintln!("overview: no output to open on");
            return;
        };
        let m = &mut self.overview.model;
        m.output = name;
        m.rows = Vec::new();
        let rows = self.overview_rows();
        let snap = &self.store.compositor.snapshot;
        let sel = Sel::of(&rows, &snap.focused_workspace_id, &snap.focused_window_id);
        let m = &mut self.overview.model;
        m.rows = rows;
        m.sel = sel;
        m.phase = Phase::Opening;
        m.zoom = Some(Animated::new(0.0, curve));
        m.glide = Some(Animated::new(sel.row as f64, curve));
        m.slides.clear();
        m.drag = None;
        m.opened_at = Some(now);
        m.opens += 1;
        self.overview.pressed = None;
        self.overview.scroll = 0.0;

        let surface = self.compositor.create_surface(&self.qh);
        let layer = self.layer_shell.create_layer_surface(&self.qh, surface, Layer::Top, Some("formalshell:overview"), Some(&output));
        layer.set_anchor(Anchor::all());
        layer.set_size(0, 0);
        layer.set_exclusive_zone(0);
        layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
        layer.commit();
        let mut surface = Surface::on("overview", layer, &self.shm, self.started, &output);
        surface.wait_map = true;
        self.overview.win = Some((surface, Scene::clear(size.0 as i32, size.1 as i32), output));
        compositor::send(compositor::Command::RefreshWindows);
        self.overview_capture();
        self.overview_probe();
        self.overview.dirty = true;
        self.log("overview opening");
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
        let (ms, _) = self.overview_curve();
        let m = &mut self.overview.model;
        let Some(row) = m.rows.get(m.sel.row) else {
            self.overview_unmap();
            return;
        };
        match m.sel.win.and_then(|j| row.windows.get(j)) {
            Some(w) => compositor::focus_window(&w.id),
            None if row.workspace_id.is_empty() => compositor::focus_workspace_at(row.idx, &m.output),
            None => compositor::focus_workspace(&row.workspace_id),
        }
        m.drag = None;
        let was = std::mem::replace(&mut m.phase, Phase::Closing);
        if was == Phase::Opening {
            // The zoom never left the desktop: nothing to run back.
            self.overview_unmap();
            return;
        }
        if let Some(z) = &mut m.zoom {
            z.set(now, 0.0, ms);
        }
        self.overview_keyboard(KeyboardInteractivity::None);
        self.overview.dirty = true;
        self.log("overview closing");
    }

    fn overview_unmap(&mut self) {
        self.overview.win = None;
        self.overview.primed = false;
        let m = &mut self.overview.model;
        m.phase = Phase::Closed;
        m.zoom = None;
        m.glide = None;
        m.slides.clear();
        m.drag = None;
        m.nodes.clear();
        m.opened_at = None;
        self.overview.pressed = None;
        self.capture_clear(Owner::Overview);
        self.log("overview unmapped");
    }

    fn overview_current(&self) -> String {
        let m = &self.overview.model;
        m.rows.get(m.current_row()).map(|r| r.workspace_id.clone()).unwrap_or_default()
    }

    fn overview_capture(&mut self) {
        let wants = self.overview.model.thumb_sizes(&self.overview_current());
        let scale = self.overview.win.as_ref().map_or_else(crate::scene::raster_scale, |(s, _, _)| s.scale());
        self.capture_set(Owner::Overview, &wants, false, scale);
    }

    fn overview_icon_edge(&self) -> u32 {
        (self.store.theme.theme.space.xl * 2.0).round() as u32
    }

    fn overview_probe(&self) {
        let scale = self.overview.win.as_ref().map_or_else(crate::scene::raster_scale, |(s, _, _)| s.scale());
        let ids: Vec<&str> = self.overview.model.rows.iter().flat_map(|r| r.windows.iter().map(|w| w.id.as_str())).collect();
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
    pub fn overview_changed(&mut self, topic: crate::store::Topic) {
        use crate::store::Topic;
        if !self.overview.model.open() {
            return;
        }
        if topic == Topic::Compositor {
            let rows = self.overview_rows();
            let m = &mut self.overview.model;
            if rows != m.rows {
                let sel = m.sel.follow(&m.rows, &rows);
                // A window that moved workspace or changed size (a drop
                // re-tiles both rows) has a frame of its old shape.
                let shape = |rows: &[ov::Row]| -> HashMap<String, (String, i64, i64)> {
                    rows.iter().flat_map(|r| r.windows.iter().map(|w| (w.id.clone(), (r.workspace_id.clone(), w.rect.2.round() as i64, w.rect.3.round() as i64)))).collect()
                };
                let before = shape(&m.rows);
                let stale: Vec<String> = shape(&rows).into_iter().filter(|(id, now)| before.get(id).is_some_and(|b| b != now)).map(|(id, _)| id).collect();
                m.rows = rows;
                let (ms, curve) = self.overview_curve();
                self.overview.model.select(sel, Instant::now(), ms, curve);
                if self.overview.model.phase != Phase::Closing {
                    self.overview_capture();
                    for id in &stale {
                        self.capture_refresh(Owner::Overview, id);
                    }
                    self.overview_probe();
                }
            }
        }
        self.overview.dirty = true;
    }

    pub fn overview_thumbs_changed(&mut self) {
        self.overview.dirty = true;
    }

    pub fn overview_status(&self) -> serde_json::Value {
        let captured = |id: &str| self.capture_image(Owner::Overview, id).is_some();
        let mut v = self.overview.model.status(Instant::now(), &captured);
        v["capturing"] = self.capture_sourced(Owner::Overview).into();
        v
    }

    /// An ask lands during a paint that runs after this surface's present,
    /// so it takes the loop round once more straight away.
    pub fn overview_deadline(&self) -> Option<Instant> {
        if ASKED.with(Cell::get).is_some() {
            return Some(Instant::now());
        }
        let m = &self.overview.model;
        (m.phase == Phase::Opening).then(|| m.opened_at.map(|t| t + WAIT)).flatten()
    }

    pub fn overview_frame(&mut self) {
        let Some((s, _, _)) = &mut self.overview.win else { return };
        (s.frame_pending, s.callbacks) = (false, s.callbacks + 1);
        if !s.mapped {
            s.mapped = true;
            self.log("overview mapped");
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
        let offset = band_offset(self.bar.edge(), output, area);
        self.overview.model.geo = Geometry::new(area, output, offset, self.store.theme.theme.space.xl);
        let (ms, curve) = self.overview_curve();
        let sel = self.overview.model.sel;
        let m = &mut self.overview.model;
        m.slides.clear();
        m.select(sel, Instant::now(), ms, curve);
        // A fresh layout has nothing to glide from.
        for s in m.slides.values_mut() {
            let t = s.target();
            s.jump(t);
        }
        self.overview.dirty = true;
    }

    pub fn overview_closed(&mut self) {
        self.overview_unmap();
    }

    /// The output's wallpaper as one bitmap, made once per picture.
    fn overview_wallpaper(&mut self) -> (Option<Bitmap>, Option<Rgba>) {
        let wanted = self.store.desktop();
        if let Some([r, g, b]) = wallpaper::flat(&wanted) {
            return (None, Some(Rgba::hex(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))));
        }
        let Some((_, _, output)) = &self.overview.win else { return (None, None) };
        let Some((pw, ph)) = self.heads.iter().find(|h| &h.output == output).and_then(|h| h.backdrop.as_ref()?.pixels()) else {
            return (None, None);
        };
        let Some(picture) = self.store.wallpaper.sized(pw as u32, ph as u32).filter(|p| p.path == wanted).cloned() else {
            return (None, None);
        };
        let at = std::sync::Arc::as_ptr(&picture) as usize;
        if let Some((seen, bitmap)) = &self.overview.wallpaper
            && *seen == at
        {
            return (Some(bitmap.clone()), None);
        }
        let bitmap = capture::fit(&picture.bgra, (picture.width, picture.height), true, (picture.width, picture.height));
        self.overview.wallpaper = Some((at, bitmap.clone()));
        (Some(bitmap), None)
    }

    pub fn overview_present(&mut self) {
        if let Some(action) = ASKED.with(Cell::take) {
            self.overview_action(action);
        }
        let now = Instant::now();
        let (ms, _) = self.overview_curve();
        let ready = self.overview.win.as_ref().is_some_and(|(s, _, _)| s.configured && s.mapped);
        let m = &mut self.overview.model;
        if m.phase == Phase::Opening && ready {
            let current = m.rows.get(m.current_row()).map(|r| r.windows.iter().map(|w| w.id.clone()).collect::<Vec<_>>()).unwrap_or_default();
            let landed = current.iter().all(|id| self.capture_image(Owner::Overview, id).is_some());
            let m = &mut self.overview.model;
            if landed || m.opened_at.is_some_and(|t| now >= t + WAIT) {
                m.phase = Phase::Open;
                if let Some(z) = &mut m.zoom {
                    z.set(now, 1.0, ms);
                }
                self.overview.dirty = true;
                self.log("overview zooming");
            }
        }
        if self.overview.model.phase == Phase::Open && ready && !self.overview.primed {
            self.overview.primed = true;
            self.overview_keyboard(KeyboardInteractivity::OnDemand);
        }
        let m = &self.overview.model;
        if m.phase == Phase::Closing && !m.animating(now) {
            self.overview_unmap();
            return;
        }
        let animating = self.overview.model.animating(now);
        let drawable = self.overview.win.as_ref().is_some_and(|(s, _, _)| s.configured && !s.frame_pending);
        if !drawable || !(self.overview.dirty || animating) {
            return;
        }
        self.overview.dirty = false;
        let (picture, flat) = self.overview_wallpaper();
        let scale = self.overview.win.as_ref().map_or(1.0, |(s, _, _)| s.scale());
        let edge = self.overview_icon_edge();
        let mut thumbs = HashMap::new();
        let mut icons = HashMap::new();
        for w in self.overview.model.rows.iter().flat_map(|r| r.windows.iter()) {
            if let Some(b) = self.capture_image(Owner::Overview, &w.id) {
                thumbs.insert(w.id.clone(), b.clone());
            }
            if let Some(b) = self.store.appicon.get(&icon_key(&w.id), edge, scale).and_then(|i| i.image) {
                icons.insert(w.id.clone(), b);
            }
        }
        // Until the zoom starts the surface is clear over the desktop.
        let clear = self.overview.model.phase == Phase::Opening;
        let input = ov::Input { theme: &self.store.theme.theme, wallpaper: picture.as_ref(), flat, thumbs: &thumbs, icons: &icons, now };
        let Some((surface, scene, _)) = &mut self.overview.win else { return };
        let _raster = crate::scene::raster_at(scale);
        if clear {
            let mut p = crate::surfaces::bar::cell::Painter::new(scene, &mut self.overview.model.nodes, None);
            p.rect(crate::scene::IRect::new(0, 0, 0, 0), Rgba::TRANSPARENT, 0.0);
            p.finish();
        } else {
            self.overview.model.draw(&input, &mut self.bar.kit, scene);
        }
        let qh = self.qh.clone();
        surface.present(scene, animating, &qh);
    }

    fn overview_select(&mut self, sel: Sel) {
        let (ms, curve) = self.overview_curve();
        self.overview.model.select(sel, Instant::now(), ms, curve);
        self.overview.dirty = true;
    }

    /// A key while it holds the keyboard.
    pub fn overview_key(&mut self, event: &KeyEvent) -> bool {
        if !matches!(self.overview.model.phase, Phase::Opening | Phase::Open) {
            return false;
        }
        let m = &self.overview.model;
        let (rows, sel) = (&m.rows, m.sel);
        let next = match event.keysym {
            Keysym::Up | Keysym::k => Some(sel.step_row(rows, -1)),
            Keysym::Down | Keysym::j => Some(sel.step_row(rows, 1)),
            Keysym::Left | Keysym::h => Some(sel.step_win(rows, -1)),
            Keysym::Right | Keysym::l => Some(sel.step_win(rows, 1)),
            Keysym::Home => Some(sel.edge(rows, false)),
            Keysym::End => Some(sel.edge(rows, true)),
            Keysym::Return | Keysym::KP_Enter | Keysym::space | Keysym::Escape => {
                self.overview_close();
                return true;
            }
            _ => None,
        };
        if let Some(sel) = next {
            self.overview_select(sel);
        }
        true
    }

    /// Pointer events on its surface; false for any other surface's.
    pub fn overview_pointer(&mut self, e: &PointerEvent) -> bool {
        if !self.overview.owns(&e.surface) {
            return false;
        }
        let now = Instant::now();
        let at = e.position;
        let live = matches!(self.overview.model.phase, Phase::Open);
        match e.kind {
            PointerEventKind::Enter { serial } => {
                self.cursor = Some((serial, Shape::Default));
                if let Some(device) = &self.cursor_device {
                    device.set_shape(serial, Shape::Default);
                }
            }
            PointerEventKind::Leave { .. } => {}
            PointerEventKind::Motion { .. } => {
                let m = &mut self.overview.model;
                if let Some(d) = &mut m.drag {
                    d.at = at;
                    if !d.lifted && (at.0 - d.from.0).hypot(at.1 - d.from.1) >= ov::LIFT_PX {
                        d.lifted = true;
                    }
                    if d.lifted {
                        // Over a card or one of its windows; over the
                        // backdrop the drop moves nothing.
                        let over = m.hit(at, now).map(|(i, _)| i);
                        if let Some(d) = &mut m.drag {
                            d.over = over;
                        }
                        self.overview.dirty = true;
                    }
                }
            }
            PointerEventKind::Press { button, .. } if button == 0x110 && live => {
                let hit = self.overview.model.hit(at, now);
                self.overview.pressed = hit;
                if let Some((i, Some(j))) = hit {
                    let m = &mut self.overview.model;
                    let w = m.rows[i].windows[j].clone();
                    let t = m.geo.transform(m.progress(now), m.glide_at(now));
                    let (x, y, _, _) = m.geo.win_at(i, &w);
                    let origin = t * vello_cpu::kurbo::Point::new(x, y);
                    let slide = m.rows.get(i).and_then(|r| m.slides.get(&r.workspace_id)).map_or(0.0, |s| s.value(now));
                    m.drag = Some(Drag { id: w.id, from_row: i, from: at, at, grab: (at.0 - origin.x - slide, at.1 - origin.y), lifted: false, over: None });
                }
            }
            PointerEventKind::Release { button, .. } if button == 0x110 => {
                let pressed = self.overview.pressed.take();
                let drag = self.overview.model.drag.take();
                if !live {
                    return true;
                }
                if let Some(d) = drag.filter(|d| d.lifted) {
                    let m = &self.overview.model;
                    if let Some(row) = d.over.and_then(|i| m.rows.get(i)).filter(|_| d.over != Some(d.from_row)) {
                        compositor::send(compositor::Command::MoveWindowTo { id: d.id.clone(), idx: row.idx, output: m.output.clone() });
                        compositor::send(compositor::Command::RefreshWindows);
                        self.log(&format!("overview drop {} on {}", d.id, row.idx));
                    }
                    self.overview.dirty = true;
                    return true;
                }
                let hit = self.overview.model.hit(at, now);
                if hit != pressed {
                    return true;
                }
                if let Some((row, win)) = hit {
                    self.overview_select(Sel { row, win });
                }
                self.overview_close();
            }
            PointerEventKind::Axis { vertical, horizontal, .. } if live => {
                let shift = self.mods.shift;
                let (along, by) = if horizontal.absolute != 0.0 || horizontal.discrete != 0 {
                    (true, if horizontal.discrete != 0 { f64::from(horizontal.discrete) * SCROLL_STEP } else { horizontal.absolute })
                } else {
                    (shift, if vertical.discrete != 0 { f64::from(vertical.discrete) * SCROLL_STEP } else { vertical.absolute })
                };
                self.overview.scroll += by;
                let steps = (self.overview.scroll / SCROLL_STEP).trunc();
                if steps != 0.0 {
                    self.overview.scroll -= steps * SCROLL_STEP;
                    let m = &self.overview.model;
                    let sel = if along { m.sel.step_win(&m.rows, steps as i32) } else { m.sel.step_row(&m.rows, steps as i32) };
                    self.overview_select(sel);
                }
            }
            _ => {}
        }
        true
    }
}

/// Where a surface of `area` sits on an output of `output` with the bar's
/// band on `edge` taken out: the band's whole extent on its own side, any
/// other inset split evenly.
fn band_offset(edge: Edge, output: (f64, f64), area: (f64, f64)) -> (f64, f64) {
    let (dx, dy) = ((output.0 - area.0).max(0.0), (output.1 - area.1).max(0.0));
    let x = match edge {
        Edge::Left => dx,
        Edge::Right => 0.0,
        _ => dx / 2.0,
    };
    let y = match edge {
        Edge::Top => dy,
        Edge::Bottom => 0.0,
        _ => dy / 2.0,
    };
    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_offset_follows_the_bars_edge() {
        assert_eq!(band_offset(Edge::Left, (1920.0, 1080.0), (1766.0, 1080.0)), (154.0, 0.0));
        assert_eq!(band_offset(Edge::Top, (1920.0, 1080.0), (1920.0, 1040.0)), (0.0, 40.0));
        assert_eq!(band_offset(Edge::Bottom, (1920.0, 1080.0), (1920.0, 1040.0)), (0.0, 0.0));
    }
}
