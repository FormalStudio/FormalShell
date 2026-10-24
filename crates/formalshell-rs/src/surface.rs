//! One layer surface and its wl_shm buffers. A commit carries only the
//! rects the scene dirtied, copied out of the renderer's canvas into
//! whichever buffer the compositor has released, plus whatever that buffer
//! missed while the other one was on screen. A frame callback is asked for
//! only on a commit made while something on the surface animates.
//! A flat full-output fill skips all of that: see [`PixelSurface`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use fs_chrome::grid::Grid;
use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::dispatch2::Dispatch2;
use smithay_client_toolkit::reexports::client::globals::{BindError, GlobalList};
use smithay_client_toolkit::reexports::client::protocol::wl_buffer::WlBuffer;
use smithay_client_toolkit::reexports::client::protocol::wl_callback::WlCallback;
use smithay_client_toolkit::reexports::client::protocol::wl_output::WlOutput;
use smithay_client_toolkit::reexports::client::protocol::wl_shm;
use smithay_client_toolkit::reexports::client::{Connection, Dispatch, Proxy, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_surface_v1::WpAlphaModifierSurfaceV1;
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use smithay_client_toolkit::reexports::protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{self, WpFractionalScaleV1};
use smithay_client_toolkit::reexports::protocols::wp::alpha_modifier::v1::client::wp_alpha_modifier_v1::WpAlphaModifierV1;
use smithay_client_toolkit::reexports::protocols::wp::single_pixel_buffer::v1::client::wp_single_pixel_buffer_manager_v1::WpSinglePixelBufferManagerV1;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use smithay_client_toolkit::reexports::client::protocol::wl_subcompositor::WlSubcompositor;
use smithay_client_toolkit::reexports::client::protocol::wl_subsurface::WlSubsurface;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::session_lock::SessionLockSurface;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};

use crate::render::{Renderer, scaled};
use crate::scene::{IRect, Scene};
use crate::wayland::App;

/// Past this many pending rects a buffer just takes their union.
const STALE_LIMIT: usize = 8;

struct ShmBuffer {
    buffer: Buffer,
    stale: Vec<IRect>,
}

/// What a [`Surface`] is to the compositor: a layer surface, or a lock
/// surface on one output.
pub trait Role {
    fn role_surface(&self) -> &WlSurface;
    fn role_commit(&self);
}

impl Role for LayerSurface {
    fn role_surface(&self) -> &WlSurface {
        WaylandSurface::wl_surface(self)
    }

    fn role_commit(&self) {
        WaylandSurface::commit(self);
    }
}

impl Role for SessionLockSurface {
    fn role_surface(&self) -> &WlSurface {
        SessionLockSurface::wl_surface(self)
    }

    fn role_commit(&self) {
        SessionLockSurface::wl_surface(self).commit();
    }
}

/// A synchronized subsurface: its commits wait for its parent's.
pub struct Sub {
    pub surface: WlSurface,
    pub sub: WlSubsurface,
}

impl Role for Sub {
    fn role_surface(&self) -> &WlSurface {
        &self.surface
    }

    fn role_commit(&self) {
        self.surface.commit();
    }
}

impl Drop for Sub {
    fn drop(&mut self) {
        self.sub.destroy();
        self.surface.destroy();
    }
}

/// The globals a surface needs to draw at its output's scale, bound once
/// per process: every [`Surface`] reaches them from its own present.
struct Scaling {
    fractional: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
}

static SCALING: std::sync::OnceLock<Scaling> = std::sync::OnceLock::new();

/// The bar's output's scale in 120ths: what a surface with no output of
/// its own named starts drawing at, until its preferred scale arrives.
static HINT: AtomicU32 = AtomicU32::new(120);

/// Each output's scale in 120ths by its wl_output's protocol id, read off
/// its mode and logical size, for a surface made for one output to draw its
/// first frame at.
static OUTPUTS: std::sync::Mutex<Vec<(u32, u32)>> = std::sync::Mutex::new(Vec::new());

/// An output's scale off its current mode and its logical size, in
/// 120ths; the mode is in the output's own orientation, so the longer
/// sides are compared.
pub fn scale_of(mode: (i32, i32), logical: (i32, i32)) -> Option<u32> {
    let (m, l) = (mode.0.max(mode.1), logical.0.max(logical.1));
    (m > 0 && l > 0).then(|| ((f64::from(m) / f64::from(l)) * 120.0).round() as u32).filter(|s| *s > 0)
}

/// The outputs as they are now: each one's scale where its info says it,
/// and `primary` the one a surface with no output named is made on.
pub fn note_outputs(outputs: &smithay_client_toolkit::output::OutputState, primary: Option<&WlOutput>) {
    let mut table = OUTPUTS.lock().unwrap();
    table.clear();
    for o in outputs.outputs() {
        let Some(info) = outputs.info(&o) else { continue };
        let mode = info.modes.iter().find(|m| m.current).map(|m| m.dimensions);
        if let Some(scale) = mode.zip(info.logical_size).and_then(|(m, l)| scale_of(m, l)) {
            table.push((o.id().protocol_id(), scale));
        }
    }
    let primary = primary.cloned().or_else(|| outputs.outputs().next()).map(|o| o.id().protocol_id());
    if let Some(s) = primary.and_then(|p| table.iter().find(|(id, _)| *id == p)).map(|(_, s)| *s) {
        HINT.store(s, Ordering::Relaxed);
    }
}

/// Every scale an output draws at, once each: what a picture shown on
/// every output (the lock's) is made at.
pub fn output_scales() -> Vec<f64> {
    let table = OUTPUTS.lock().unwrap();
    let mut scales: Vec<u32> = table.iter().map(|(_, s)| *s).collect();
    scales.sort_unstable();
    scales.dedup();
    if scales.is_empty() {
        scales.push(HINT.load(Ordering::Relaxed));
    }
    scales.into_iter().map(|s| f64::from(s) / 120.0).collect()
}

/// The largest scale any output draws at.
pub fn largest_scale() -> f64 {
    let table = OUTPUTS.lock().unwrap();
    f64::from(table.iter().map(|(_, s)| *s).max().unwrap_or(120).max(120)) / 120.0
}

/// The scale a surface made on `output` starts at.
pub fn scale_on(output: &WlOutput) -> f64 {
    let id = output.id().protocol_id();
    let table = OUTPUTS.lock().unwrap();
    table.iter().find(|(o, _)| *o == id).map_or_else(primary_scale, |(_, s)| f64::from(*s) / 120.0)
}

pub fn bind_scaling<D>(globals: &GlobalList, qh: &QueueHandle<D>)
where
    D: Dispatch<WpFractionalScaleManagerV1, Inert> + Dispatch<WpViewporter, Inert> + 'static,
{
    let _ = SCALING.set(Scaling { fractional: globals.bind(qh, 1..=1, Inert).ok(), viewporter: globals.bind(qh, 1..=1, Inert).ok() });
}

/// The bar's output's scale, else the first output's.
pub fn primary_scale() -> f64 {
    f64::from(HINT.load(Ordering::Relaxed)) / 120.0
}

/// Told when a surface's preferred scale moved, so an owner that draws only
/// when something changed draws again.
pub trait ScaleHandler {
    fn rescaled(&mut self);
}

/// A surface's preferred scale in 120ths, as its wp_fractional_scale_v1
/// last said.
#[derive(Clone)]
pub struct Preferred(Arc<AtomicU32>);

impl<D: ScaleHandler> Dispatch2<WpFractionalScaleV1, D> for Preferred {
    fn event(&self, state: &mut D, _: &WpFractionalScaleV1, event: wp_fractional_scale_v1::Event, _: &Connection, _: &QueueHandle<D>) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            if self.0.swap(scale, Ordering::Relaxed) != scale {
                state.rescaled();
            }
        }
    }
}

/// User data for the scaling objects no event of which matters, for any
/// state type (the shell's and the greeter's).
pub struct Inert;

impl<D> Dispatch2<WpFractionalScaleManagerV1, D> for Inert {
    fn event(&self, _: &mut D, _: &WpFractionalScaleManagerV1, _: <WpFractionalScaleManagerV1 as Proxy>::Event, _: &Connection, _: &QueueHandle<D>) {}
}

impl<D> Dispatch2<WpViewporter, D> for Inert {
    fn event(&self, _: &mut D, _: &WpViewporter, _: <WpViewporter as Proxy>::Event, _: &Connection, _: &QueueHandle<D>) {}
}

impl<D> Dispatch2<WpViewport, D> for Inert {
    fn event(&self, _: &mut D, _: &WpViewport, _: <WpViewport as Proxy>::Event, _: &Connection, _: &QueueHandle<D>) {}
}

/// What a viewport is sent: a source in buffer pixels, and a destination.
type View = (Option<[f64; 4]>, (i32, i32));

/// A surface's scale and its viewport: a buffer drawn past scale 1 is
/// mapped back onto the surface's logical size by the viewport's
/// destination, and a crop the owner asks for is a source in buffer pixels.
struct Scaler {
    preferred: Preferred,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    /// The owner's cut of the surface, in logical pixels.
    crop: Option<IRect>,
    sent: Option<View>,
}

impl Scaler {
    fn new(scale: f64) -> Self {
        let preferred = Preferred(Arc::new(AtomicU32::new((scale * 120.0).round() as u32)));
        Self { preferred, fractional: None, viewport: None, crop: None, sent: None }
    }

    /// The scale the surface should be drawn at: the fractional scale it
    /// was given, else the integer one, else 1 where no viewport can map it.
    fn wanted(&self, surface: &WlSurface) -> f64 {
        let Some(scaling) = SCALING.get().filter(|s| s.viewporter.is_some()) else { return 1.0 };
        if scaling.fractional.is_some() {
            return f64::from(self.preferred.0.load(Ordering::Relaxed).max(1)) / 120.0;
        }
        let factor = surface.data::<smithay_client_toolkit::compositor::SurfaceData<()>>().map_or(1, |d| d.scale_factor());
        f64::from(factor.max(1))
    }

    /// Asks for the surface's preferred scale, once.
    fn follow<D>(&mut self, surface: &WlSurface, qh: &QueueHandle<D>)
    where
        D: Dispatch<WpFractionalScaleV1, Preferred> + 'static,
    {
        if self.fractional.is_some() {
            return;
        }
        if let Some(manager) = SCALING.get().and_then(|s| s.fractional.as_ref()) {
            self.fractional = Some(manager.get_fractional_scale(surface, qh, self.preferred.clone()));
        }
    }

    /// The viewport for a buffer drawn at `scale` for a surface `size`
    /// big, sent when it changed; with no `qh` only a viewport already made
    /// is used.
    fn sync<D>(&mut self, surface: &WlSurface, scale: f64, size: (i32, i32), qh: Option<&QueueHandle<D>>)
    where
        D: Dispatch<WpViewport, Inert> + 'static,
    {
        let want: View = match self.crop {
            Some(c) if scale == 1.0 => (Some([f64::from(c.x), f64::from(c.y), f64::from(c.w), f64::from(c.h)]), (c.w, c.h)),
            Some(c) => {
                // A source has to lie inside the buffer, which is the
                // logical size scaled and rounded; wl_fixed keeps 1/256.
                let (bw, bh) = (f64::from(scaled(size.0, scale)), f64::from(scaled(size.1, scale)));
                let fixed = |v: f64| (v * 256.0).floor() / 256.0;
                let (x, y) = (fixed(f64::from(c.x) * scale).min(bw), fixed(f64::from(c.y) * scale).min(bh));
                let (w, h) = (fixed((f64::from(c.w) * scale).min(bw - x)), fixed((f64::from(c.h) * scale).min(bh - y)));
                (Some([x, y, w, h]), (c.w, c.h))
            }
            None if scale != 1.0 || self.sent.is_some() => (None, size),
            None => return,
        };
        if self.sent == Some(want) {
            return;
        }
        if self.viewport.is_none() {
            let viewporter = SCALING.get().and_then(|s| s.viewporter.as_ref());
            if let (Some(viewporter), Some(qh)) = (viewporter, qh) {
                self.viewport = Some(viewporter.get_viewport(surface, qh, Inert));
            }
        }
        let Some(viewport) = &self.viewport else { return };
        if let Some([x, y, w, h]) = want.0 {
            viewport.set_source(x, y, w, h);
        }
        viewport.set_destination(want.1.0, want.1.1);
        self.sent = Some(want);
    }
}

impl Drop for Scaler {
    fn drop(&mut self) {
        if let Some(f) = self.fractional.take() {
            f.destroy();
        }
        if let Some(v) = self.viewport.take() {
            v.destroy();
        }
    }
}

pub struct Surface<R: Role = LayerSurface> {
    pub name: &'static str,
    /// Ahead of `layer`, so its objects go before the surface they are on.
    scaler: Scaler,
    pub layer: R,
    /// ABGR8888 where the compositor takes it: the canvas's own byte order,
    /// copied without a swizzle.
    format: wl_shm::Format,
    pool: SlotPool,
    buffers: Vec<ShmBuffer>,
    renderer: Renderer,
    pub configured: bool,
    /// A frame callback is outstanding: nothing is drawn until it lands.
    pub frame_pending: bool,
    /// The first callback after the first commit, which is when the
    /// surface is on screen.
    pub mapped: bool,
    /// Whether the owner waits on `mapped` before it animates.
    pub wait_map: bool,
    started: Instant,
    commits: u64,
    pub callbacks: u64,
    /// When the outstanding frame callback was asked for, and how long the
    /// last one took to land: a slow compositor reads here, a slow shell
    /// in `event loop:` lines.
    asked: Option<Instant>,
    waited_us: u128,
    landed_at: Option<Instant>,
    /// No buffer has been dropped out of the pool, so every byte of it a
    /// new buffer gets is still the zero (clear) memfd fill.
    pool_clean: bool,
    /// Everything the renderer has drawn since its canvas was cleared: all
    /// a buffer cut from a clean pool lacks.
    drawn: Option<IRect>,
    /// Set, a frame is drawn in bands across loop turns, each turn stopping
    /// once it has run this long, so a large first frame never holds
    /// another surface's due frame (the bar's) behind it.
    pub raster_budget: Option<std::time::Duration>,
    /// The frame being drawn: all its damage, and the bands still to draw.
    raster: Vec<IRect>,
    left: std::collections::VecDeque<IRect>,
    react_us: u128,
    render_us: u128,
    slices: u32,
    /// What the canvas is multiplied by on its way into a buffer, out of
    /// 256. Hyprland draws no `wp_alpha_modifier` on a layer surface's
    /// subsurfaces (0.56 wraps only a window's and a popup's in the
    /// `CWLSurface` its renderer reads the multiplier off), so a
    /// subsurface fades in its own pixels.
    opacity: u16,
    /// The opacity changed: everything drawn goes to the compositor again.
    refade: bool,
    /// The buffer the compositor last got, and the one it got before a
    /// resize dropped the rest: what [`Surface::recommit`] attaches again.
    front: Option<usize>,
    retired: Option<Buffer>,
    /// The scale and logical size that buffer was drawn at.
    front_view: Option<(f64, (i32, i32))>,
}

/// Rows per band of a sliced frame.
const BAND: i32 = 96;

fn bands(rect: IRect) -> impl Iterator<Item = IRect> {
    (rect.y..rect.bottom()).step_by(BAND as usize).map(move |y| IRect::new(rect.x, y, rect.w, BAND.min(rect.bottom() - y)))
}

/// A closed window's pool, buffers and canvas, kept for the next window of
/// the same kind so its first frames write into pages already faulted in
/// rather than fresh memfd and heap pages, a fault per 4 KiB.
pub struct Kept {
    pool: SlotPool,
    buffers: Vec<ShmBuffer>,
    renderer: Renderer,
    pool_clean: bool,
    drawn: Option<IRect>,
}

/// linux/mman.h, Linux 5.14: fault the range in writable without touching
/// its contents, so it can run beside a writer.
const MADV_POPULATE_WRITE: libc::c_int = 23;

impl Kept {
    /// A clean pool two `size` buffers deep and a canvas of `size`, both
    /// faulted in on the blocking pool while nothing draws from them.
    pub fn prefaulted(shm: &Shm, size: (i32, i32), runner: &crate::runtime::Pool) -> Option<Self> {
        let scale = primary_scale();
        let bytes = scaled(size.0.max(1), scale) as usize * scaled(size.1.max(1), scale) as usize * 4 * 2;
        let mut pool = SlotPool::new(bytes, shm).ok()?;
        let slot = pool.new_slot(bytes).ok()?;
        let data = pool.raw_data_mut(&slot);
        let (at, len) = (data.as_mut_ptr() as usize, data.len());
        drop(slot);
        let mut renderer = Renderer::new(1, 1);
        renderer.set_scale(scale);
        renderer.resize(size.0.max(1) as u16, size.1.max(1) as u16);
        let canvas = renderer.canvas_bytes_mut();
        let (canvas_at, canvas_len) = (canvas.as_mut_ptr() as usize, canvas.len());
        runner.submit(move || {
            // SAFETY: madvise only faults the pages in; a range unmapped
            // meanwhile (a resized pool) answers ENOMEM and nothing else.
            unsafe {
                libc::madvise(at as *mut libc::c_void, len, MADV_POPULATE_WRITE);
                libc::madvise(canvas_at as *mut libc::c_void, canvas_len, MADV_POPULATE_WRITE);
            }
        });
        Some(Self { pool, buffers: Vec::new(), renderer, pool_clean: true, drawn: None })
    }
}

impl<R: Role> Surface<R> {
    pub fn commits(&self) -> u64 {
        self.commits
    }

    /// The pool, buffers and canvas, for [`Surface::adopt`] on the next
    /// window of this kind.
    pub fn keep(self) -> Kept {
        Kept { pool: self.pool, buffers: self.buffers, renderer: self.renderer, pool_clean: self.pool_clean, drawn: self.drawn }
    }

    /// Takes over a kept pool, buffers and canvas. What the last window
    /// drew is cleared off the canvas and stale in every buffer, so each
    /// takes the clear canvas there on its first use.
    pub fn adopt(&mut self, mut kept: Kept) {
        if let Some(old) = kept.drawn {
            kept.renderer.clear(old);
            for b in &mut kept.buffers {
                b.stale.push(old);
            }
        }
        self.pool = kept.pool;
        self.buffers = kept.buffers;
        self.renderer = kept.renderer;
        self.pool_clean = kept.pool_clean;
        self.front = None;
    }

    pub fn new(name: &'static str, layer: R, shm: &Shm, started: Instant) -> Self {
        Self::at(name, layer, shm, started, primary_scale())
    }

    /// A surface made for `output`, drawing its first frame at that
    /// output's scale.
    pub fn on(name: &'static str, layer: R, shm: &Shm, started: Instant, output: &WlOutput) -> Self {
        Self::at(name, layer, shm, started, scale_on(output))
    }

    fn at(name: &'static str, layer: R, shm: &Shm, started: Instant, scale: f64) -> Self {
        let mut renderer = Renderer::new(1, 1);
        renderer.set_scale(scale);
        Self {
            name,
            scaler: Scaler::new(scale),
            layer,
            format: if shm.formats().contains(&wl_shm::Format::Abgr8888) { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 },
            pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            buffers: Vec::new(),
            renderer,
            configured: false,
            frame_pending: false,
            mapped: false,
            wait_map: false,
            started,
            commits: 0,
            callbacks: 0,
            asked: None,
            waited_us: 0,
            landed_at: None,
            pool_clean: true,
            drawn: None,
            raster_budget: None,
            raster: Vec::new(),
            left: std::collections::VecDeque::new(),
            react_us: 0,
            render_us: 0,
            slices: 0,
            opacity: 256,
            refade: false,
            front: None,
            retired: None,
            front_view: None,
        }
    }

    /// The scale this surface draws at from its next present on: what the
    /// pictures its scene asks for are made at.
    pub fn scale(&self) -> f64 {
        self.scaler.wanted(self.layer.role_surface())
    }

    /// Takes over a viewport made for this surface, which a crop is sent on.
    pub fn with_viewport(mut self, viewport: WpViewport) -> Self {
        self.scaler.viewport = Some(viewport);
        self
    }

    /// Shows only `crop` of the surface, at its own size: sent with the
    /// next commit, a present's or [`Surface::recommit`]'s.
    pub fn set_crop(&mut self, crop: IRect) {
        self.scaler.crop = Some(crop);
    }

    /// Draws again at the scale the compositor last asked for, if it moved:
    /// the canvas and buffers at the new size and the whole scene damaged.
    fn follow_scale(&mut self, scene: &mut Scene) {
        let want = self.scaler.wanted(self.layer.role_surface());
        if want == self.renderer.scale() {
            return;
        }
        self.renderer.set_scale(want);
        self.drop_buffers();
        scene.touch(IRect::new(0, 0, scene.size.w, scene.size.h));
    }

    /// Every buffer and what the canvas held, for a new canvas size; the
    /// one on screen is kept for [`Surface::recommit`].
    fn drop_buffers(&mut self) {
        self.pool_clean &= self.buffers.is_empty();
        if let Some(at) = self.front.take() {
            self.retired = Some(self.buffers.swap_remove(at).buffer);
        }
        self.buffers.clear();
        self.drawn = None;
        self.raster.clear();
        self.left.clear();
    }

    /// The opacity everything this surface shows is drawn at, from 0 to 1,
    /// applied on the next present without drawing the scene again.
    pub fn set_opacity(&mut self, opacity: f32) {
        let opacity = (opacity.clamp(0.0, 1.0) * 256.0).round() as u16;
        if opacity == self.opacity {
            return;
        }
        self.opacity = opacity;
        if let Some(drawn) = self.drawn {
            self.refade = true;
            for b in &mut self.buffers {
                b.stale.push(drawn);
            }
        }
    }

    /// Commits state set beside the buffer (a subsurface's place, its
    /// viewport) with the buffer the compositor already shows attached
    /// again. Hyprland 0.56 takes a surface's size off its viewport only on
    /// a commit carrying a buffer (`SSurfaceState::updateFrom` copies `size`
    /// under the buffer bit), so a viewport changed on a bare commit draws
    /// the new crop squeezed into the old size.
    pub fn recommit(&mut self) {
        if let Some((scale, size)) = self.front_view {
            self.scaler.sync::<App>(self.layer.role_surface(), scale, size, None);
        }
        let buffer = self.front.map(|at| &self.buffers[at].buffer).or(self.retired.as_ref());
        if let Some(b) = buffer {
            // Hyprland copies an shm buffer on commit and releases it at
            // once, so it may be back already: in use again either way.
            let _ = b.activate();
            self.layer.role_surface().attach(Some(b.wl_buffer()), 0, 0);
        }
        self.layer.role_commit();
    }

    /// Sizes the canvas ahead of the compositor's first configure, so the
    /// first frame is drawn while it is on its way.
    pub fn presize(&mut self, width: i32, height: i32) {
        self.renderer.resize(width.max(1) as u16, height.max(1) as u16);
    }

    /// A whole frame is drawn and waits only to be committed.
    pub fn drawn_ahead(&self) -> bool {
        !self.raster.is_empty() && self.left.is_empty()
    }

    /// Draws the scene's damage into the canvas, as part of the frame in
    /// progress. False while a sliced frame still has bands left.
    fn draw(&mut self, scene: &mut Scene) -> bool {
        if self.raster.is_empty() {
            // From the callback that let this frame go to the start of drawing it.
            self.react_us = self.landed_at.take().map_or(0, |at| at.elapsed().as_micros());
            self.render_us = 0;
            self.slices = 0;
        }
        // Damage that came in while a sliced frame was being drawn joins it.
        for rect in scene.take_damage() {
            self.raster.push(rect);
            match self.raster_budget {
                Some(_) => self.left.extend(bands(rect)),
                None => self.left.push_back(rect),
            }
        }
        let t0 = Instant::now();
        while let Some(rect) = self.left.pop_front() {
            self.renderer.render(scene, rect);
            if self.raster_budget.is_some_and(|budget| t0.elapsed() >= budget) && !self.left.is_empty() {
                self.render_us += t0.elapsed().as_micros();
                self.slices += 1;
                return false;
            }
        }
        self.render_us += t0.elapsed().as_micros();
        self.slices += 1;
        true
    }

    /// Draws the scene's damage without committing it, so a frame can wait
    /// on another surface's: true once the whole frame is drawn, or when
    /// there is nothing to draw or nowhere to draw it yet.
    pub fn prepare(&mut self, scene: &mut Scene) -> bool {
        let sized = (self.renderer.width() as i32, self.renderer.height() as i32) == (scene.size.w, scene.size.h);
        if !self.configured || !sized {
            return true;
        }
        self.follow_scale(scene);
        if !scene.has_damage() && self.raster.is_empty() {
            return true;
        }
        self.draw(scene)
    }

    /// A sliced frame is part drawn: the owner holds its scene still and
    /// presents again on the next loop turn.
    pub fn rastering(&self) -> bool {
        !self.left.is_empty()
    }

    /// A frame callback landed.
    pub fn landed(&mut self, now: Instant) {
        self.frame_pending = false;
        self.callbacks += 1;
        if let Some(asked) = self.asked.take() {
            self.waited_us = now.duration_since(asked).as_micros();
        }
        self.landed_at = Some(now);
    }

    /// The compositor's size, which the owner has resized its scene to.
    pub fn configure(&mut self, width: i32, height: i32) {
        self.configured = true;
        if (self.renderer.width() as i32, self.renderer.height() as i32) != (width, height) {
            self.renderer.resize(width as u16, height as u16);
            self.drop_buffers();
        }
    }

    pub fn present<D>(&mut self, scene: &mut Scene, animating: bool, qh: &QueueHandle<D>)
    where
        D: Dispatch<WlCallback, FrameCallbackData> + Dispatch<WpFractionalScaleV1, Preferred> + Dispatch<WpViewport, Inert> + 'static,
    {
        self.scaler.follow(self.layer.role_surface(), qh);
        if self.frame_pending {
            return;
        }
        // A scene resized ahead of the compositor's configure (the bar
        // moving edge) keeps its damage until the buffers match it.
        if (self.renderer.width() as i32, self.renderer.height() as i32) != (scene.size.w, scene.size.h) {
            return;
        }
        self.follow_scale(scene);
        // A presized canvas draws the first frame while the configure is on
        // its way; the configure then only has it copied and committed.
        if !self.configured {
            if self.commits == 0 && scene.has_damage() {
                self.draw(scene);
            }
            return;
        }
        // An animation whose frame changed nothing (a card still wholly
        // behind its line) still needs the next callback to carry on.
        let request = animating || (self.wait_map && !self.mapped);
        // A scene that starts out clear still owes the compositor one
        // buffer before the surface can map.
        if self.commits == 0 && !scene.has_damage() && self.raster.is_empty() {
            scene.touch(IRect::new(0, 0, 1, 1));
        }
        if !scene.has_damage() && self.raster.is_empty() && !self.refade {
            if request && self.commits > 0 {
                let surface = self.layer.role_surface();
                surface.frame(qh, FrameCallbackData(surface.clone()));
                self.frame_pending = true;
                self.asked = Some(Instant::now());
                self.layer.role_commit();
            }
            return;
        }
        if !self.draw(scene) {
            return;
        }
        let mut damage = std::mem::take(&mut self.raster);
        let (react_us, render_us) = (self.react_us, self.render_us);
        if std::mem::take(&mut self.refade) {
            damage.extend(self.drawn);
        }
        self.drawn = damage.iter().fold(self.drawn, |u, r| Some(u.map_or(*r, |u| u.union(r))));

        let t1 = Instant::now();
        let size = scene.size;
        let (pw, ph) = self.renderer.pixels();
        let at = match self.buffers.iter().position(|b| b.buffer.canvas(&mut self.pool).is_some()) {
            Some(at) => at,
            None => {
                let (buffer, _) = self
                    .pool
                    .create_buffer(pw, ph, pw * 4, self.format)
                    .expect("wl_shm buffer");
                let stale = if self.pool_clean { self.drawn.into_iter().collect() } else { vec![size] };
                self.buffers.push(ShmBuffer { buffer, stale });
                self.buffers.len() - 1
            }
        };

        for (i, b) in self.buffers.iter_mut().enumerate() {
            if i != at {
                b.stale.extend_from_slice(&damage);
                if b.stale.len() > STALE_LIMIT {
                    let union = b.stale.iter().fold(b.stale[0], |u, r| u.union(r));
                    b.stale = vec![union];
                }
            }
        }

        let target = &mut self.buffers[at];
        let mut copy = std::mem::take(&mut target.stale);
        copy.extend_from_slice(&damage);
        // A rect inside another is copied with it.
        let copy: Vec<IRect> = copy
            .iter()
            .enumerate()
            .filter(|(i, r)| !copy.iter().enumerate().any(|(j, o)| j != *i && o.contains(r) && (o != *r || j < *i)))
            .map(|(_, r)| self.renderer.device(r.intersect(&size)))
            .filter(|r| !r.is_empty())
            .collect();
        let canvas = target.buffer.canvas(&mut self.pool).expect("released buffer");
        let stride = pw as usize;
        let k = self.opacity;
        // Premultiplied, so all four channels scale alike.
        let fade = |c: u8| ((u16::from(c) * k + 128) >> 8) as u8;
        if k < 256 {
            let src = self.renderer.canvas().data_as_u8_slice();
            let bgra = self.format != wl_shm::Format::Abgr8888;
            for rect in &copy {
                for row in rect.y..rect.bottom() {
                    let start = (row as usize * stride + rect.x as usize) * 4;
                    let end = start + rect.w as usize * 4;
                    for (px, dst) in src[start..end].chunks_exact(4).zip(canvas[start..end].chunks_exact_mut(4)) {
                        let (r, g, b, a) = (fade(px[0]), fade(px[1]), fade(px[2]), fade(px[3]));
                        dst.copy_from_slice(&if bgra { [b, g, r, a] } else { [r, g, b, a] });
                    }
                }
            }
        } else if self.format == wl_shm::Format::Abgr8888 {
            // ABGR8888 is the canvas's own RGBA byte order: rows copy whole.
            let src = self.renderer.canvas().data_as_u8_slice();
            for rect in &copy {
                for row in rect.y..rect.bottom() {
                    let start = (row as usize * stride + rect.x as usize) * 4;
                    let end = start + rect.w as usize * 4;
                    canvas[start..end].copy_from_slice(&src[start..end]);
                }
            }
        } else {
            let src = self.renderer.canvas().data();
            for rect in &copy {
                for row in rect.y..rect.bottom() {
                    let start = row as usize * stride + rect.x as usize;
                    let pixels = &src[start..start + rect.w as usize];
                    let out = &mut canvas[start * 4..(start + rect.w as usize) * 4];
                    // wl_shm ARGB8888 is premultiplied BGRA in memory on little endian.
                    for (px, dst) in pixels.iter().zip(out.chunks_exact_mut(4)) {
                        dst.copy_from_slice(&[px.b, px.g, px.r, px.a]);
                    }
                }
            }
        }
        let copy_us = t1.elapsed().as_micros();

        let scale = self.renderer.scale();
        let surface = self.layer.role_surface();
        for rect in &damage {
            let rect = self.renderer.device(*rect);
            surface.damage_buffer(rect.x, rect.y, rect.w, rect.h);
        }
        self.scaler.sync(surface, scale, (size.w, size.h), Some(qh));
        // Until the first callback the surface may not be on screen yet,
        // and an enter waits for it.
        if request {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
            self.asked = Some(Instant::now());
        }
        target.buffer.attach_to(surface).expect("attach released buffer");
        self.layer.role_commit();
        self.front = Some(at);
        self.retired = None;
        self.front_view = Some((scale, (size.w, size.h)));

        self.commits += 1;
        if !crate::tracing() {
            return;
        }
        let area: i64 = damage.iter().map(IRect::area).sum();
        let rects: Vec<String> = damage.iter().map(|r| format!("{},{},{}x{}", r.x, r.y, r.w, r.h)).collect();
        crate::trace(format!(
            "commit surface={} n={} t={}ms render_us={} copy_us={} rects={} px={} damage=[{}] buffer={} frame_callbacks={} request={} last_wait_us={} react_us={} slices={}",
            self.name,
            self.commits,
            self.started.elapsed().as_millis(),
            render_us,
            copy_us,
            damage.len(),
            area,
            rects.join(" "),
            at,
            self.callbacks,
            request as u8,
            self.waited_us,
            react_us,
            self.slices,
        ) + &if scale == 1.0 { String::new() } else { format!(" scale={scale} buffer_px={pw}x{ph}") });
    }

    pub fn report(&self) -> String {
        format!("{}: commits={} frame_callbacks={}", self.name, self.commits, self.callbacks)
    }
}

/// A layer surface showing one black pixel (`wp_single_pixel_buffer_v1`)
/// stretched over its whole size by `wp_viewporter` and faded by
/// `wp_alpha_modifier_v1`, so the compositor does all of the drawing and a
/// step of a fade is one multiplier and one commit.
///
/// The pixel carries the surface's own alpha and the multiplier only the
/// fade: Hyprland's `ignore_alpha` reads the buffer's alpha, never the
/// modifier's, so an opaque pixel faded to 0.5 is still blurred behind, on
/// every frame and over the whole output for a scrim.
pub struct PixelSurface {
    pub name: &'static str,
    pub layer: LayerSurface,
    viewport: WpViewport,
    fade: WpAlphaModifierSurfaceV1,
    buffer: WlBuffer,
    /// The pixel's own alpha, what the compositor's `ignore_alpha` reads.
    pub alpha: f64,
    size: Option<(i32, i32)>,
    /// A size the next commit has to carry.
    resized: bool,
    multiplier: Option<u32>,
    pub frame_pending: bool,
    pub mapped: bool,
    started: Instant,
    commits: u64,
    pub callbacks: u64,
}

impl PixelSurface {
    pub fn new(name: &'static str, layer: LayerSurface, alpha: f64, pixels: &Pixels, qh: &QueueHandle<App>, started: Instant) -> Self {
        let surface = layer.wl_surface();
        let viewport = pixels.viewporter.get_viewport(surface, qh, Ignore);
        let fade = pixels.alpha.get_surface(surface, qh, Ignore);
        let alpha = alpha.clamp(0.0, 1.0);
        // Premultiplied black is zero in every channel but the alpha.
        let a = (alpha * f64::from(u32::MAX)).round() as u32;
        let buffer = pixels.single_pixel.create_u32_rgba_buffer(0, 0, 0, a, qh, Ignore);
        Self {
            name,
            layer,
            viewport,
            fade,
            buffer,
            alpha,
            size: None,
            resized: false,
            multiplier: None,
            frame_pending: false,
            mapped: false,
            started,
            commits: 0,
            callbacks: 0,
        }
    }

    pub fn configure(&mut self, width: i32, height: i32) {
        if width > 0 && height > 0 && self.size != Some((width, height)) {
            self.size = Some((width, height));
            self.resized = true;
        }
    }

    pub fn size(&self) -> Option<(i32, i32)> {
        self.size
    }

    /// `fade` multiplies the pixel's own alpha, 0 to 1.
    pub fn present(&mut self, fade: f64, animating: bool, qh: &QueueHandle<App>) {
        let Some((width, height)) = self.size else { return };
        if self.frame_pending {
            return;
        }
        let request = animating || !self.mapped;
        let multiplier = (fade.clamp(0.0, 1.0) * u32::MAX as f64).round() as u32;
        let surface = self.layer.wl_surface();
        if self.multiplier == Some(multiplier) && !self.resized {
            if request && self.commits > 0 {
                surface.frame(qh, FrameCallbackData(surface.clone()));
                self.frame_pending = true;
                self.layer.commit();
            }
            return;
        }
        let t0 = Instant::now();
        if self.resized || self.commits == 0 {
            self.viewport.set_destination(width, height);
            surface.attach(Some(&self.buffer), 0, 0);
            self.resized = false;
        }
        self.fade.set_multiplier(multiplier);
        surface.damage_buffer(0, 0, 1, 1);
        if request {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
        }
        self.layer.commit();
        self.multiplier = Some(multiplier);
        self.commits += 1;
        if !crate::tracing() {
            return;
        }
        crate::trace(format!(
            "commit surface={} n={} t={}ms render_us={} copy_us=0 rects=1 px=0 damage=[0,0,1x1] buffer=pixel alpha={:.3} frame_callbacks={} request={}",
            self.name,
            self.commits,
            self.started.elapsed().as_millis(),
            t0.elapsed().as_micros(),
            self.alpha * fade,
            self.callbacks,
            request as u8,
        ));
    }

    pub fn report(&self) -> String {
        format!("{}: commits={} frame_callbacks={}", self.name, self.commits, self.callbacks)
    }

    /// The multiplier last committed, 0 to 1.
    pub fn fade(&self) -> f64 {
        self.multiplier.map_or(0.0, |m| f64::from(m) / f64::from(u32::MAX))
    }
}

impl Drop for PixelSurface {
    fn drop(&mut self) {
        self.fade.destroy();
        self.viewport.destroy();
        self.buffer.destroy();
    }
}

/// The three globals a [`PixelSurface`] is made of.
pub struct Pixels {
    pub subcompositor: WlSubcompositor,
    pub viewporter: WpViewporter,
    pub single_pixel: WpSinglePixelBufferManagerV1,
    pub alpha: WpAlphaModifierV1,
}

impl Pixels {
    pub fn bind(globals: &GlobalList, qh: &QueueHandle<App>) -> Result<Self, BindError> {
        Ok(Self {
            subcompositor: globals.bind(qh, 1..=1, Ignore)?,
            viewporter: globals.bind(qh, 1..=1, Ignore)?,
            single_pixel: globals.bind(qh, 1..=1, Ignore)?,
            alpha: globals.bind(qh, 1..=1, Ignore)?,
        })
    }
}

/// User data for objects whose events carry nothing this shell acts on: a
/// buffer that is never redrawn has no use for its release.
pub struct Ignore;

macro_rules! ignore_events {
    ($($iface:ty),*) => {$(
        impl Dispatch2<$iface, App> for Ignore {
            fn event(&self, _: &mut App, _: &$iface, _: <$iface as Proxy>::Event, _: &Connection, _: &QueueHandle<App>) {}
        }
    )*};
}

ignore_events!(WlSubcompositor, WlSubsurface, WpViewporter, WpViewport, WpSinglePixelBufferManagerV1, WpAlphaModifierV1, WpAlphaModifierSurfaceV1, WlBuffer);

/// The desktop's own layer: the theme's background colour under the grid
/// when `wallpaper.grid` asks for one, or the wallpaper's ready pixels over
/// the whole output. A new wallpaper
/// crossfades in over `reveal`; nothing draws at rest, and frame callbacks
/// are asked for only while a fade runs. The pixels are the output's own:
/// a wallpaper made at the scaled size, mapped back by the viewport.
pub struct Backdrop {
    scaler: Scaler,
    pub layer: LayerSurface,
    pool: SlotPool,
    buffer: Option<Buffer>,
    size: Option<(i32, i32)>,
    drawn: Option<(String, Option<usize>, i32, i32, [u8; 4], Option<Grid>)>,
    /// What is on screen once any fade lands.
    shown: Option<std::sync::Arc<crate::services::wallpaper::Picture>>,
    fade: Option<Fade>,
    /// The logical size the buffer on screen was painted for.
    painted: Option<(i32, i32)>,
    /// The sculpture over the picture, while `wallpaper.animate` is on.
    pub shards: Option<Shards>,
}

struct Fade {
    from: Option<std::sync::Arc<crate::services::wallpaper::Picture>>,
    to: Option<std::sync::Arc<crate::services::wallpaper::Picture>>,
    color: [u8; 4],
    grid: Option<Grid>,
    start: Instant,
    ms: f64,
}

impl Backdrop {
    pub fn new(layer: LayerSurface, shm: &Shm, output: &WlOutput) -> Self {
        Self {
            scaler: Scaler::new(scale_on(output)),
            layer,
            pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            buffer: None,
            size: None,
            drawn: None,
            shown: None,
            fade: None,
            painted: None,
            shards: None,
        }
    }

    pub fn configure(&mut self, width: i32, height: i32) {
        if width > 0 && height > 0 {
            self.size = Some((width, height));
        }
    }

    /// The sculpture's layers taken on, or dropped with none.
    pub fn animate(&mut self, shards: Option<Shards>) {
        self.shards = shards;
    }

    /// The sculpture as `look` and `grid` draw it, running or held: a
    /// change of look or size draws the held frame again at once. True
    /// while it runs.
    pub fn sync_shards(&mut self, look: crate::shards::Look, grid: Option<Grid>, run: Result<(), &'static str>, now: Instant, qh: &QueueHandle<App>) -> bool {
        let (Some(logical), Some(device)) = (self.size, self.pixels()) else { return false };
        let Some(s) = &mut self.shards else { return false };
        if !s.configured.iter().all(|c| *c) {
            return false;
        }
        s.set_grid(grid, device, logical);
        let started = match run {
            Ok(()) if !s.running => {
                s.running = true;
                s.why = "";
                s.last = Some(now);
                true
            }
            Err(why) => {
                s.running = false;
                s.why = why;
                false
            }
            Ok(()) => false,
        };
        let moved = s.look.as_ref() != Some(&look) || s.drawn.is_none_or(|(_, d)| d != crate::shards::half(device));
        s.look = Some(look);
        // Anything else waits for the beat, which keeps the cap.
        if moved || (started && !s.pending) {
            s.draw(device, logical, now, qh);
        }
        s.running
    }

    /// One beat of the sculpture's clock: a frame, unless the last one is
    /// still waiting on the compositor. False once it has stopped.
    pub fn shard_tick(&mut self, now: Instant, motion_scale: f64, qh: &QueueHandle<App>) -> bool {
        let (Some(logical), Some(device)) = (self.size, self.pixels()) else { return false };
        let Some(s) = &mut self.shards else { return false };
        if !s.running || !s.configured.iter().all(|c| *c) {
            return false;
        }
        if let Some(last) = s.last.replace(now) {
            s.t += now.duration_since(last).as_secs_f64() * 1000.0 / motion_scale.max(0.01);
        }
        if !s.pending {
            s.draw(device, logical, now, qh);
        }
        true
    }

    /// The size in buffer pixels, what a wallpaper is made at.
    pub fn pixels(&self) -> Option<(i32, i32)> {
        let scale = self.scaler.wanted(self.layer.wl_surface());
        self.size.map(|(w, h)| (scaled(w, scale), scaled(h, scale)))
    }

    /// Buffer pixels to a logical one.
    pub fn scale(&self) -> f64 {
        self.scaler.wanted(self.layer.wl_surface())
    }

    /// Already showing `path` at this logical size, though at another
    /// scale: it stays up while the picture for the new scale is made.
    pub fn holds(&self, path: &str) -> bool {
        self.painted == self.size && self.shown.as_ref().is_some_and(|p| p.path == path)
    }

    /// `picture` when it was made for this size, else the plain colour; a
    /// change after the first paint fades over `reveal_ms`.
    pub fn draw(
        &mut self,
        picture: Option<std::sync::Arc<crate::services::wallpaper::Picture>>,
        background: fs_theme::color::Rgba,
        grid: Option<Grid>,
        reveal_ms: f64,
        qh: &QueueHandle<App>,
    ) {
        self.scaler.follow(self.layer.wl_surface(), qh);
        let Some((w, h)) = self.pixels() else { return };
        let picture = picture.filter(|p| (p.width as i32, p.height as i32) == (w, h));
        let [r, g, b, _] = background.to_u8();
        let color = [b, g, r, 255];
        let key = (picture.as_ref().map(|p| p.path.clone()).unwrap_or_default(), picture.as_ref().and_then(|p| p.dither), w, h, color, grid);
        if self.drawn.as_ref() == Some(&key) {
            return;
        }
        let first = self.drawn.is_none();
        self.drawn = Some(key);
        let from = match self.fade.take() {
            Some(f) => f.to,
            None => self.shown.clone(),
        };
        self.shown = picture.clone();
        if first || reveal_ms <= 0.0 || picture.is_none() {
            self.paint(None, picture.as_deref(), color, grid, 1.0, qh);
            return;
        }
        self.fade = Some(Fade { from, to: picture, color, grid, start: Instant::now(), ms: reveal_ms });
        self.step(Instant::now(), qh);
    }

    /// One frame of a running fade; the frame callback lands back here.
    pub fn step(&mut self, now: Instant, qh: &QueueHandle<App>) {
        let Some(f) = &self.fade else { return };
        let t = (now.duration_since(f.start).as_secs_f64() * 1000.0 / f.ms).clamp(0.0, 1.0);
        // effectsSlow's ease-out, close enough at a crossfade's length.
        let a = 1.0 - (1.0 - t).powi(3);
        let (from, to, color, grid) = (f.from.clone(), f.to.clone(), f.color, f.grid);
        if t >= 1.0 {
            self.fade = None;
            self.paint(None, to.as_deref(), color, grid, 1.0, qh);
            return;
        }
        let surface = self.layer.wl_surface().clone();
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.paint(Some(from.as_deref()), to.as_deref(), color, grid, a as f32, qh);
    }

    /// `to` over `from` at `a`; `from` none is no blend at all, `Some(None)`
    /// the plain colour under `grid`.
    fn paint(
        &mut self,
        from: Option<Option<&crate::services::wallpaper::Picture>>,
        to: Option<&crate::services::wallpaper::Picture>,
        color: [u8; 4],
        grid: Option<Grid>,
        a: f32,
        qh: &QueueHandle<App>,
    ) {
        let Some(size) = self.size else { return };
        let scale = self.scaler.wanted(self.layer.wl_surface());
        let (w, h) = (scaled(size.0, scale), scaled(size.1, scale));
        // The grid rides on its own layer over the sculpture.
        let grid = grid.filter(|_| self.shards.is_none());
        let Ok((buffer, canvas)) = self.pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        let plain = |canvas: &mut [u8]| {
            for px in canvas.chunks_exact_mut(4) {
                px.copy_from_slice(&color);
            }
            if let Some(g) = grid {
                let [r, gr, b, a] = g.color;
                Grid { color: [b, gr, r, a], ..g }.paint(canvas, w as usize, h as usize);
            }
        };
        match (from, to) {
            (None, Some(p)) => canvas.copy_from_slice(&p.bgra),
            (None, None) => plain(canvas),
            (Some(src), dst) => {
                let k = (a.clamp(0.0, 1.0) * 256.0) as u32;
                let mut flat = Vec::new();
                if src.is_none() || dst.is_none() {
                    flat.resize(canvas.len(), 0);
                    plain(&mut flat);
                }
                let pick = |p: Option<&crate::services::wallpaper::Picture>, i: usize| p.map_or_else(|| flat[i], |p| p.bgra[i]);
                for (i, c) in canvas.iter_mut().enumerate() {
                    let s0 = u32::from(pick(src, i));
                    let d0 = u32::from(pick(dst, i));
                    *c = ((s0 * (256 - k) + d0 * k) >> 8) as u8;
                }
            }
        }
        let surface = self.layer.wl_surface();
        if buffer.attach_to(surface).is_err() {
            return;
        }
        self.scaler.sync(surface, scale, size, Some(qh));
        surface.damage_buffer(0, 0, w, h);
        self.layer.commit();
        self.painted = Some(size);
        self.buffer = Some(buffer);
        if from.is_none() {
            eprintln!("commit surface=wallpaper {}x{} picture={}", w, h, to.is_some());
        }
    }
}

/// Frames a second the sculpture is drawn at, at most.
pub const SHARD_FPS: f64 = 24.0;

/// The sculpture's two layers over a [`Backdrop`]'s picture: the shards
/// at half the output's pixels, mapped back over the whole output by the
/// viewport, and the grid over them at the output's own, drawn once.
/// Layer surfaces of their own rather than the picture's subsurfaces:
/// Hyprland schedules no frame for a commit to a layer's subsurface, so a
/// subsurface's frames land only when something else redraws the output.
pub struct Shards {
    layer: LayerSurface,
    viewport: WpViewport,
    grid: LayerSurface,
    grid_viewport: WpViewport,
    /// Each layer's first configure, before which neither takes a buffer.
    pub configured: [bool; 2],
    pool: SlotPool,
    /// Each buffer and what its last draw left in it.
    buffers: Vec<(Buffer, Option<crate::shards::Painted>)>,
    grid_pool: SlotPool,
    grid_buffer: Option<Buffer>,
    grid_drawn: Option<((i32, i32), Option<Grid>)>,
    format: wl_shm::Format,
    raster: crate::shards::Raster,
    look: Option<crate::shards::Look>,
    /// The sculpture's own clock, ms: it moves only while it runs.
    pub t: f64,
    last: Option<Instant>,
    pub running: bool,
    /// Why it holds still, empty while it runs.
    pub why: &'static str,
    /// A frame is on its way to the screen: the next waits for its callback.
    pending: bool,
    damage: Option<IRect>,
    /// The clock and buffer size of the frame on screen.
    drawn: Option<(f64, (i32, i32))>,
    pub commits: u64,
    pub callbacks: u64,
    pub render_us: u128,
    started: Instant,
    /// The beat driving it, while it runs.
    pub timer: Option<calloop::RegistrationToken>,
}

impl Shards {
    /// `layer` and `grid` are made in that order on the Background level
    /// after the picture's own, which stacks them over it.
    pub fn new(layer: LayerSurface, grid: LayerSurface, pixels: &Pixels, shm: &Shm, qh: &QueueHandle<App>, started: Instant) -> Self {
        let viewport = pixels.viewporter.get_viewport(layer.wl_surface(), qh, Ignore);
        let grid_viewport = pixels.viewporter.get_viewport(grid.wl_surface(), qh, Ignore);
        Self {
            layer,
            viewport,
            grid,
            grid_viewport,
            configured: [false; 2],
            pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            buffers: Vec::new(),
            grid_pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            grid_buffer: None,
            grid_drawn: None,
            format: if shm.formats().contains(&wl_shm::Format::Abgr8888) { wl_shm::Format::Abgr8888 } else { wl_shm::Format::Argb8888 },
            raster: crate::shards::Raster::new(),
            look: None,
            t: 0.0,
            last: None,
            running: false,
            why: "",
            pending: false,
            damage: None,
            drawn: None,
            commits: 0,
            callbacks: 0,
            render_us: 0,
            started,
            timer: None,
        }
    }

    /// Which of the two layers `surface` is: 0 the shards, 1 the grid.
    pub fn owns(&self, surface: &WlSurface) -> Option<usize> {
        if self.layer.wl_surface() == surface {
            Some(0)
        } else if self.grid.wl_surface() == surface {
            Some(1)
        } else {
            None
        }
    }

    pub fn frame_done(&mut self) {
        self.pending = false;
        self.callbacks += 1;
    }

    /// The grid's layer at the output's own pixels, drawn again only when
    /// its size or lines change; none clears it.
    fn set_grid(&mut self, grid: Option<Grid>, device: (i32, i32), logical: (i32, i32)) {
        let key = (device, grid);
        if self.grid_drawn == Some(key) {
            return;
        }
        self.grid_drawn = Some(key);
        let surface = self.grid.wl_surface();
        let Some(g) = grid else {
            // A clear pixel keeps the layer mapped: a null buffer unmaps it,
            // and a remapped layer surface needs a fresh configure.
            let Ok((buffer, canvas)) = self.grid_pool.create_buffer(1, 1, 4, self.format) else { return };
            canvas.fill(0);
            if buffer.attach_to(surface).is_ok() {
                self.grid_viewport.set_destination(logical.0, logical.1);
                surface.damage_buffer(0, 0, 1, 1);
                self.grid.commit();
                self.grid_buffer = Some(buffer);
            }
            return;
        };
        let (w, h) = device;
        let Ok((buffer, canvas)) = self.grid_pool.create_buffer(w, h, w * 4, self.format) else { return };
        canvas.fill(0);
        let [r, gr, b, a] = g.color;
        let ink = if self.format == wl_shm::Format::Abgr8888 { [r, gr, b, a] } else { [b, gr, r, a] };
        Grid { color: ink, ..g }.coat(canvas, w as usize, h as usize);
        if buffer.attach_to(surface).is_err() {
            return;
        }
        self.grid_viewport.set_destination(logical.0, logical.1);
        surface.damage_buffer(0, 0, w, h);
        self.grid.commit();
        self.grid_buffer = Some(buffer);
    }

    /// The frame at the clock's `t`, committed with a callback asked for.
    fn draw(&mut self, device: (i32, i32), logical: (i32, i32), now: Instant, qh: &QueueHandle<App>) {
        let Some(look) = &self.look else { return };
        let (w, h) = crate::shards::half(device);
        let (Ok(bw), Ok(bh)) = (u16::try_from(w), u16::try_from(h)) else { return };
        let t0 = Instant::now();
        // Two buffers in turn, a third only while both are on screen.
        self.buffers.retain(|(b, _)| b.height() == h && b.stride() == w * 4);
        let free = self.buffers.iter().position(|(b, _)| self.pool.canvas(b).is_some());
        let at = match free {
            Some(i) => i,
            None if self.buffers.len() >= 3 => return,
            None => {
                let Ok((buffer, _)) = self.pool.create_buffer(w, h, w * 4, self.format) else { return };
                self.buffers.push((buffer, None));
                self.buffers.len() - 1
            }
        };
        let resized = self.drawn.is_none_or(|(_, d)| d != (w, h));
        let (buffer, prior) = &mut self.buffers[at];
        let Some(canvas) = self.pool.canvas(buffer) else { return };
        // A buffer swapped to BGRA whole can only be painted whole.
        let keep = if self.format == wl_shm::Format::Abgr8888 { *prior } else { None };
        let painted = self.raster.draw(canvas, bw, bh, self.t, look, keep);
        *prior = Some(painted);
        let damage = painted.shards;
        if self.format != wl_shm::Format::Abgr8888 {
            for px in canvas.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
        }
        let render_us = t0.elapsed().as_micros();
        let surface = self.layer.wl_surface();
        if self.buffers[at].0.attach_to(surface).is_err() {
            return;
        }
        let full = IRect::new(0, 0, w, h);
        let rect = match (resized, self.damage, damage) {
            (true, ..) => full,
            (false, Some(a), Some(b)) => a.union(&b),
            (false, a, b) => a.or(b).unwrap_or(full),
        };
        self.viewport.set_destination(logical.0, logical.1);
        surface.damage_buffer(rect.x, rect.y, rect.w, rect.h);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.layer.commit();
        self.pending = true;
        self.damage = damage;
        self.drawn = Some((self.t, (w, h)));
        self.commits += 1;
        self.render_us = render_us;
        if crate::tracing() {
            crate::trace(format!(
                "commit surface=shards n={} t={}ms render_us={} damage=[{},{},{}x{}] buffer={}x{} clock={:.0} running={}",
                self.commits,
                now.duration_since(self.started).as_millis(),
                render_us,
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                w,
                h,
                self.t,
                self.running as u8,
            ));
        }
    }
}

impl Drop for Shards {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.grid_viewport.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::scale_of;

    #[test]
    fn an_outputs_scale_comes_off_its_mode_and_logical_size() {
        assert_eq!(scale_of((2560, 1600), (2048, 1280)), Some(150));
        assert_eq!(scale_of((1920, 1200), (1536, 960)), Some(150));
        assert_eq!(scale_of((2560, 1440), (2048, 1152)), Some(150));
        assert_eq!(scale_of((1920, 1080), (1536, 864)), Some(150));
        assert_eq!(scale_of((1920, 1080), (1920, 1080)), Some(120));
        assert_eq!(scale_of((1600, 2560), (1280, 2048)), Some(150));
        assert_eq!(scale_of((0, 0), (1920, 1080)), None);
    }
}
