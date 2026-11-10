//! One layer surface and its wl_shm buffers. A commit carries only the
//! rects the scene dirtied, copied out of the renderer's canvas into
//! whichever buffer the compositor has released, plus whatever that buffer
//! missed while the other one was on screen. A frame callback is asked for
//! only on a commit made while something on the surface animates.
//! A flat full-output fill skips all of that: see [`PixelSurface`].

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

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
use crate::trace;

pub mod audit;

/// Past this many pending rects a buffer just takes their union.
const STALE_LIMIT: usize = 8;

struct ShmBuffer {
    buffer: Buffer,
    stale: Vec<IRect>,
}

/// Where a surface draws: `Gpu` on the GPU while `render.backend` resolves
/// to it and the device is up, on the CPU otherwise; `Cpu` always on the
/// CPU.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Gpu,
    Cpu,
}

impl Backend {
    /// The GPU is the one to draw on now.
    fn on_gpu(self) -> bool {
        self == Self::Gpu && crate::gpu::active()
    }

    fn renderer(self) -> Renderer {
        let gpu = self.on_gpu().then(|| Renderer::gpu(1, 1)).flatten();
        gpu.unwrap_or_else(|| Renderer::new(1, 1))
    }
}

/// The byte order a CPU canvas goes into a wl_shm buffer in: the canvas's
/// own RGBA (ABGR8888) where the compositor takes it, else BGRA (ARGB8888,
/// premultiplied BGRA in memory on little endian).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Rgba,
    Bgra,
}

impl Format {
    /// RGBA where `shm` offers it.
    pub fn of(shm: &Shm) -> Self {
        if shm.formats().contains(&wl_shm::Format::Abgr8888) { Self::Rgba } else { Self::Bgra }
    }

    pub fn wl(self) -> wl_shm::Format {
        match self {
            Self::Rgba => wl_shm::Format::Abgr8888,
            Self::Bgra => wl_shm::Format::Argb8888,
        }
    }

    /// An RGBA pixel in this byte order, or one in this order back to RGBA.
    pub fn pixel(self, [r, g, b, a]: [u8; 4]) -> [u8; 4] {
        match self {
            Self::Rgba => [r, g, b, a],
            Self::Bgra => [b, g, r, a],
        }
    }

    /// RGBA bytes put in this byte order in place.
    pub fn swizzle(self, bytes: &mut [u8]) {
        if self == Self::Bgra {
            for px in bytes.as_chunks_mut::<4>().0 {
                px.swap(0, 2);
            }
        }
    }
}

/// A buffer's stale list after `damage` went into another one: past
/// [`STALE_LIMIT`] rects it just takes their union.
fn go_stale(stale: &mut Vec<IRect>, damage: &[IRect]) {
    stale.extend_from_slice(damage);
    if stale.len() > STALE_LIMIT {
        let union = stale.iter().fold(stale[0], |u, r| u.union(r));
        *stale = vec![union];
    }
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

/// An output's scale off its current mode and its logical size, in
/// 120ths; the mode is in the output's own orientation, so the longer
/// sides are compared.
pub fn scale_of(mode: (i32, i32), logical: (i32, i32)) -> Option<u32> {
    let (m, l) = (mode.0.max(mode.1), logical.0.max(logical.1));
    (m > 0 && l > 0).then(|| ((f64::from(m) / f64::from(l)) * 120.0).round() as u32).filter(|s| *s > 0)
}

/// Each output's scale in 120ths by its wl_output's protocol id, read off
/// its mode and logical size, for a surface made for one output to draw
/// its first frame at; and the primary output's (the bar's), where a
/// surface with no output named opens.
#[derive(Clone, Debug, PartialEq)]
pub struct Scales {
    outputs: Vec<(u32, u32)>,
    primary: u32,
}

impl Default for Scales {
    fn default() -> Self {
        Self { outputs: Vec::new(), primary: 120 }
    }
}

impl Scales {
    /// The outputs as they are now, `primary` the one a surface with no
    /// output named is made on. A primary whose scale is not known yet
    /// keeps this one's.
    pub fn read(&self, outputs: &smithay_client_toolkit::output::OutputState, primary: Option<&WlOutput>) -> Self {
        let mut table = Vec::new();
        for o in outputs.outputs() {
            let Some(info) = outputs.info(&o) else { continue };
            let mode = info.modes.iter().find(|m| m.current).map(|m| m.dimensions);
            if let Some(scale) = mode.zip(info.logical_size).and_then(|(m, l)| scale_of(m, l)) {
                table.push((o.id().protocol_id(), scale));
            }
        }
        let id = primary.cloned().or_else(|| outputs.outputs().next()).map(|o| o.id().protocol_id());
        let primary = id.and_then(|p| table.iter().find(|(o, _)| *o == p)).map_or(self.primary, |(_, s)| *s);
        Self { outputs: table, primary }
    }

    /// The bar's output's scale, else the first output's.
    pub fn primary(&self) -> f64 {
        f64::from(self.primary) / 120.0
    }

    /// Every scale an output draws at, once each: what a picture shown on
    /// every output (the lock's) is made at.
    pub fn all(&self) -> Vec<f64> {
        let mut scales: Vec<u32> = self.outputs.iter().map(|(_, s)| *s).collect();
        scales.sort_unstable();
        scales.dedup();
        if scales.is_empty() {
            scales.push(self.primary);
        }
        scales.into_iter().map(|s| f64::from(s) / 120.0).collect()
    }

    /// The largest scale any output draws at, never under 1.
    pub fn largest(&self) -> f64 {
        f64::from(self.outputs.iter().map(|(_, s)| *s).max().unwrap_or(120).max(120)) / 120.0
    }

    /// `output`'s scale where its info says it.
    pub fn of(&self, output: &WlOutput) -> Option<f64> {
        let id = output.id().protocol_id();
        self.outputs.iter().find(|(o, _)| *o == id).map(|(_, s)| f64::from(*s) / 120.0)
    }

    /// The scale a surface made on `output` starts at.
    pub fn on(&self, output: &WlOutput) -> f64 {
        self.of(output).unwrap_or_else(|| self.primary())
    }
}

/// Binds the scaling globals, once per process; later calls find them bound.
pub fn bind_scaling<D: Dispatcher>(globals: &GlobalList, qh: &QueueHandle<D>) {
    SCALING.get_or_init(|| Scaling { fractional: globals.bind(qh, 1..=1, Ignore).ok(), viewporter: globals.bind(qh, 1..=1, Ignore).ok() });
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

/// User data for objects whose events carry nothing this shell acts on (a
/// buffer that is never redrawn has no use for its release), for any state
/// type: the shell's and the greeter's.
pub struct Ignore;

/// [`Ignore`] for each interface named.
macro_rules! ignore_events {
    ($($iface:ty),* $(,)?) => {$(
        impl<D> smithay_client_toolkit::dispatch2::Dispatch2<$iface, D> for $crate::surface::Ignore {
            fn event(
                &self,
                _: &mut D,
                _: &$iface,
                _: <$iface as smithay_client_toolkit::reexports::client::Proxy>::Event,
                _: &smithay_client_toolkit::reexports::client::Connection,
                _: &smithay_client_toolkit::reexports::client::QueueHandle<D>,
            ) {
            }
        }
    )*};
}
pub(crate) use ignore_events;

ignore_events!(
    WpFractionalScaleManagerV1,
    WlSubcompositor,
    WlSubsurface,
    WpViewporter,
    WpViewport,
    WpSinglePixelBufferManagerV1,
    WpAlphaModifierV1,
    WpAlphaModifierSurfaceV1,
    WlBuffer
);

/// The state a surface's objects dispatch to: the shell's or the greeter's.
pub trait Dispatcher:
    Dispatch<WlCallback, FrameCallbackData>
    + Dispatch<WpFractionalScaleV1, Preferred>
    + Dispatch<WpFractionalScaleManagerV1, Ignore>
    + Dispatch<WpViewporter, Ignore>
    + Dispatch<WpViewport, Ignore>
    + Dispatch<WlSubcompositor, Ignore>
    + Dispatch<WpSinglePixelBufferManagerV1, Ignore>
    + Dispatch<WpAlphaModifierV1, Ignore>
    + Dispatch<WpAlphaModifierSurfaceV1, Ignore>
    + Dispatch<WlBuffer, Ignore>
    + 'static
{
}

impl<D> Dispatcher for D where
    D: Dispatch<WlCallback, FrameCallbackData>
        + Dispatch<WpFractionalScaleV1, Preferred>
        + Dispatch<WpFractionalScaleManagerV1, Ignore>
        + Dispatch<WpViewporter, Ignore>
        + Dispatch<WpViewport, Ignore>
        + Dispatch<WlSubcompositor, Ignore>
        + Dispatch<WpSinglePixelBufferManagerV1, Ignore>
        + Dispatch<WpAlphaModifierV1, Ignore>
        + Dispatch<WpAlphaModifierSurfaceV1, Ignore>
        + Dispatch<WlBuffer, Ignore>
        + 'static
{
}

/// What a viewport is sent: a source in buffer pixels, and a destination.
type View = (Option<[f64; 4]>, (i32, i32));

/// A surface's scale and its viewport: a buffer drawn past scale 1 is
/// mapped back onto the surface's logical size by the viewport's
/// destination, and a crop the owner asks for is a source in buffer pixels.
pub(crate) struct Scaler {
    preferred: Preferred,
    fractional: Option<WpFractionalScaleV1>,
    viewport: Option<WpViewport>,
    /// The owner's cut of the surface, in logical pixels.
    crop: Option<IRect>,
    sent: Option<View>,
    /// `preferred` is a parent's: Hyprland sends no preferred scale to a
    /// subsurface, which would otherwise keep the scale it was made at.
    inherited: bool,
}

impl Scaler {
    pub(crate) fn new(scale: f64) -> Self {
        let preferred = Preferred(Arc::new(AtomicU32::new((scale * 120.0).round() as u32)));
        Self { preferred, fractional: None, viewport: None, crop: None, sent: None, inherited: false }
    }

    /// The scale the surface should be drawn at: the fractional scale it
    /// was given, else the integer one, else 1 where no viewport can map it.
    pub(crate) fn wanted(&self, surface: &WlSurface) -> f64 {
        let Some(scaling) = SCALING.get().filter(|s| s.viewporter.is_some()) else { return 1.0 };
        if scaling.fractional.is_some() {
            return f64::from(self.preferred.0.load(Ordering::Relaxed).max(1)) / 120.0;
        }
        let factor = surface.data::<smithay_client_toolkit::compositor::SurfaceData<()>>().map_or(1, |d| d.scale_factor());
        f64::from(factor.max(1))
    }

    /// Asks for the surface's preferred scale, once.
    pub(crate) fn follow<D: Dispatcher>(&mut self, surface: &WlSurface, qh: &QueueHandle<D>) {
        if self.fractional.is_some() || self.inherited {
            return;
        }
        if let Some(manager) = SCALING.get().and_then(|s| s.fractional.as_ref()) {
            self.fractional = Some(manager.get_fractional_scale(surface, qh, self.preferred.clone()));
        }
    }

    /// What the viewport is to be sent for a buffer drawn at `scale` for a
    /// surface `size` big; none when that is already sent or no viewport
    /// is needed.
    fn want(&self, scale: f64, size: (i32, i32)) -> Option<View> {
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
            None => return None,
        };
        (self.sent != Some(want)).then_some(want)
    }

    /// `want` sent on the viewport already made, if there is one.
    fn send(&mut self, want: View) {
        let Some(viewport) = &self.viewport else { return };
        if let Some([x, y, w, h]) = want.0 {
            viewport.set_source(x, y, w, h);
        }
        viewport.set_destination(want.1.0, want.1.1);
        self.sent = Some(want);
    }

    /// The surface's viewport, made the first time one is needed.
    fn make_viewport<D: Dispatcher>(&mut self, surface: &WlSurface, qh: &QueueHandle<D>) {
        if self.viewport.is_none()
            && let Some(viewporter) = SCALING.get().and_then(|s| s.viewporter.as_ref())
        {
            self.viewport = Some(viewporter.get_viewport(surface, qh, Ignore));
        }
    }

    /// The viewport for a buffer drawn at `scale` for a surface `size`
    /// big, sent when it changed.
    pub(crate) fn sync<D: Dispatcher>(&mut self, surface: &WlSurface, scale: f64, size: (i32, i32), qh: &QueueHandle<D>) {
        let Some(want) = self.want(scale, size) else { return };
        self.make_viewport(surface, qh);
        self.send(want);
    }

    /// The viewport for a one-pixel buffer over the whole surface: no
    /// source, `size` as the destination.
    fn stretch<D: Dispatcher>(&mut self, surface: &WlSurface, size: (i32, i32), qh: &QueueHandle<D>) {
        self.make_viewport(surface, qh);
        let Some(viewport) = &self.viewport else { return };
        viewport.set_source(-1.0, -1.0, -1.0, -1.0);
        viewport.set_destination(size.0.max(1), size.1.max(1));
        // The next frame's viewport is sent again whatever it is.
        self.sent = Some((Some([-1.0; 4]), size));
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
    /// The surface's id in the `commit surface=` lines.
    pub name: &'static str,
    backend: Backend,
    /// Ahead of `layer`, so its objects go before the surface they are on.
    scaler: Scaler,
    pub layer: R,
    format: Format,
    pool: SlotPool,
    buffers: Vec<ShmBuffer>,
    /// The GPU renderer's buffers, which its canvas is copied into.
    dmabufs: Vec<crate::gpu::Buffer>,
    renderer: Renderer,
    pub configured: bool,
    /// A frame callback is outstanding: nothing is drawn until it lands.
    frame_pending: bool,
    /// The first callback after the first commit, which is when the
    /// surface is on screen.
    mapped: bool,
    /// Whether the owner waits on `mapped` before it animates.
    pub wait_map: bool,
    started: Instant,
    commits: u64,
    callbacks: u64,
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
    retired_dmabuf: Option<crate::gpu::Buffer>,
    /// `debug staleAudit`'s full draw of the frame being committed.
    audit: Option<vello_cpu::Pixmap>,
    /// The scale and logical size that buffer was drawn at.
    front_view: Option<(f64, (i32, i32))>,
    /// [`Pixels::clear`] is attached: the surface shows nothing and holds
    /// no canvas or buffers ([`Surface::present_clear`]).
    cleared: bool,
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
    dmabufs: Vec<crate::gpu::Buffer>,
    renderer: Renderer,
    pool_clean: bool,
    drawn: Option<IRect>,
}

/// linux/mman.h, Linux 5.14: fault the range in writable without touching
/// its contents, so it can run beside a writer.
const MADV_POPULATE_WRITE: libc::c_int = 23;

impl Kept {
    /// A clean pool two `size` buffers deep and a canvas of `size` at
    /// `scale`, both faulted in on the blocking pool while nothing draws
    /// from them.
    pub fn prefaulted(shm: &Shm, size: (i32, i32), scale: f64, runner: &crate::runtime::Pool) -> Option<Self> {
        // The launcher's card draws on the GPU there: nothing to fault in.
        if crate::gpu::active() {
            return None;
        }
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
        Some(Self { pool, buffers: Vec::new(), dmabufs: Vec::new(), renderer, pool_clean: true, drawn: None })
    }
}

impl<R: Role> Surface<R> {
    pub fn commits(&self) -> u64 {
        self.commits
    }

    /// The pool, buffers and canvas, for [`Surface::adopt`] on the next
    /// window of this kind.
    pub fn keep(self) -> Kept {
        Kept { pool: self.pool, buffers: self.buffers, dmabufs: self.dmabufs, renderer: self.renderer, pool_clean: self.pool_clean, drawn: self.drawn }
    }

    /// Takes over a kept pool, buffers and canvas. What the last window
    /// drew is cleared off the canvas and stale in every buffer, so each
    /// takes the clear canvas there on its first use.
    pub fn adopt(&mut self, mut kept: Kept) {
        // Kept on the other backend (the GPU lost since) is no use here.
        if kept.renderer.is_gpu() != self.renderer.is_gpu() {
            return;
        }
        if let Some(old) = kept.drawn {
            kept.renderer.clear(old);
            for b in &mut kept.buffers {
                b.stale.push(old);
            }
            for b in &mut kept.dmabufs {
                b.stale.push(old);
            }
        }
        self.pool = kept.pool;
        self.buffers = kept.buffers;
        self.dmabufs = kept.dmabufs;
        self.renderer = kept.renderer;
        self.pool_clean = kept.pool_clean;
        self.front = None;
    }

    /// A surface drawing its first frame at `scale`, until its preferred
    /// scale arrives.
    pub fn new(name: &'static str, backend: Backend, layer: R, shm: &Shm, started: Instant, scale: f64) -> Self {
        let mut renderer = backend.renderer();
        renderer.set_scale(scale);
        Self {
            name,
            backend,
            scaler: Scaler::new(scale),
            layer,
            format: Format::of(shm),
            pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            buffers: Vec::new(),
            dmabufs: Vec::new(),
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
            retired_dmabuf: None,
            front_view: None,
            audit: None,
            cleared: false,
        }
    }

    /// The scale this surface draws at from its next present on: what the
    /// pictures its scene asks for are made at.
    pub fn scale(&self) -> f64 {
        self.scaler.wanted(self.layer.role_surface())
    }

    /// Takes `scale` as the preferred scale, for a surface whose output's
    /// scale changes with no new one sent: Hyprland sends a session lock
    /// surface its fractional scale once, as it is made
    /// (`src/protocols/SessionLock.cpp`). True when it moved.
    pub fn take_scale(&mut self, scale: f64) -> bool {
        let s = (scale * 120.0).round() as u32;
        self.scaler.preferred.0.swap(s, Ordering::Relaxed) != s
    }

    /// Draws at `parent`'s scale, for a subsurface of it.
    pub fn under<P: Role>(mut self, parent: &Surface<P>) -> Self {
        self.scaler.preferred = parent.scaler.preferred.clone();
        self.scaler.inherited = true;
        self
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
            if self.renderer.is_gpu() {
                self.retired_dmabuf = (at < self.dmabufs.len()).then(|| self.dmabufs.swap_remove(at));
            } else {
                self.retired = Some(self.buffers.swap_remove(at).buffer);
            }
        }
        self.buffers.clear();
        self.dmabufs.clear();
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
            for b in &mut self.dmabufs {
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
        if let Some(want) = self.front_view.and_then(|(scale, size)| self.scaler.want(scale, size)) {
            self.scaler.send(want);
        }
        if self.renderer.is_gpu() {
            let buffer = self.front.and_then(|at| self.dmabufs.get(at)).or(self.retired_dmabuf.as_ref());
            if let Some(b) = buffer {
                b.attached();
                self.layer.role_surface().attach(Some(b.wl_buffer()), 0, 0);
            }
            self.layer.role_commit();
            return;
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

    /// Shows nothing until the next [`Surface::present`]: a clear single
    /// pixel stretched over the surface's `size`, with the pool, buffers and
    /// canvas let go, so a surface kept mapped while it is empty (the toast
    /// column, output tall) holds none of their memory. Once clear, it
    /// commits again only when `commit` asks, for state set beside the
    /// buffer (its layer, its input region).
    pub fn present_clear<D: Dispatcher>(&mut self, size: (i32, i32), commit: bool, pixels: &Pixels, shm: &Shm, qh: &QueueHandle<D>) {
        self.scaler.follow(self.layer.role_surface(), qh);
        if self.frame_pending || !self.configured {
            return;
        }
        let request = self.wait_map && !self.mapped;
        let surface = self.layer.role_surface();
        if self.cleared {
            if commit || (request && self.commits > 0) {
                if request {
                    surface.frame(qh, FrameCallbackData(surface.clone()));
                    self.frame_pending = true;
                }
                self.layer.role_commit();
            }
            return;
        }
        self.scaler.stretch(surface, size, qh);
        surface.attach(Some(&pixels.clear), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        if request {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
            self.asked = Some(Instant::now());
        }
        self.layer.role_commit();
        self.cleared = true;
        self.commits += 1;
        self.front = None;
        self.retired = None;
        self.retired_dmabuf = None;
        self.front_view = None;
        self.buffers.clear();
        self.dmabufs.clear();
        if let Ok(pool) = SlotPool::new(4096, shm) {
            self.pool = pool;
            self.pool_clean = true;
        }
        self.drawn = None;
        self.raster.clear();
        self.left.clear();
        self.renderer.resize(1, 1);
        trace::commit(
            self.name,
            self.commits,
            trace::since(self.started),
            format_args!("render_us=0 copy_us=0 rects=1 px=0 damage=[0,0,1x1] buffer=clear frame_callbacks={} request={}", self.callbacks, request as u8),
        );
    }

    /// One single-pixel buffer stretched over the surface's `size` with a
    /// callback asked for: a frame that costs no drawing and no copy. The
    /// next [`Surface::present`] draws the whole scene again.
    pub fn present_pixel<D: Dispatcher>(&mut self, size: (i32, i32), pixel: &WlBuffer, qh: &QueueHandle<D>) {
        self.scaler.follow(self.layer.role_surface(), qh);
        if self.frame_pending || !self.configured {
            return;
        }
        let surface = self.layer.role_surface();
        self.scaler.stretch(surface, size, qh);
        surface.attach(Some(pixel), 0, 0);
        surface.damage_buffer(0, 0, 1, 1);
        surface.frame(qh, FrameCallbackData(surface.clone()));
        self.frame_pending = true;
        self.asked = Some(Instant::now());
        self.layer.role_commit();
        self.cleared = true;
        self.commits += 1;
        self.front = None;
        self.front_view = None;
        trace::commit(
            self.name,
            self.commits,
            trace::since(self.started),
            format_args!("render_us=0 copy_us=0 rects=1 px=0 damage=[0,0,1x1] buffer=pixel frame_callbacks={} request=1", self.callbacks),
        );
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
            // A surface made again for a scene that has drawn before (the
            // bar re-placed when its framing moves, a spare bar on another
            // output) starts on a blank canvas: the scene's damage covers
            // only what changed since its last surface drew it.
            if self.commits == 0 {
                scene.touch(IRect::new(0, 0, scene.size.w, scene.size.h));
            }
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

    /// True for the first callback, which is when the surface is on screen.
    pub fn landed(&mut self, now: Instant) -> bool {
        self.frame_pending = false;
        self.callbacks += 1;
        if let Some(asked) = self.asked.take() {
            self.waited_us = now.duration_since(asked).as_micros();
        }
        self.landed_at = Some(now);
        !std::mem::replace(&mut self.mapped, true)
    }

    /// The first frame callback has landed.
    pub fn mapped(&self) -> bool {
        self.mapped
    }

    /// A frame callback is outstanding.
    pub fn frame_pending(&self) -> bool {
        self.frame_pending
    }

    pub fn callbacks(&self) -> u64 {
        self.callbacks
    }

    /// The compositor's size, which the owner has resized its scene to.
    pub fn configure(&mut self, width: i32, height: i32) {
        self.configured = true;
        // The canvas comes back at the scene's size on the next frame drawn.
        if self.cleared {
            return;
        }
        if (self.renderer.width() as i32, self.renderer.height() as i32) != (width, height) {
            self.renderer.resize(width as u16, height as u16);
            self.drop_buffers();
        }
    }

    pub fn present<D: Dispatcher>(&mut self, scene: &mut Scene, animating: bool, qh: &QueueHandle<D>) {
        self.scaler.follow(self.layer.role_surface(), qh);
        if self.frame_pending {
            return;
        }
        if self.renderer.is_gpu() && !crate::gpu::active() {
            self.leave_gpu(scene);
        } else if !self.renderer.is_gpu() && self.backend.on_gpu() {
            self.enter_gpu(scene);
        }
        if std::mem::take(&mut self.cleared) {
            self.renderer.resize(scene.size.w.max(1) as u16, scene.size.h.max(1) as u16);
            scene.touch(IRect::new(0, 0, scene.size.w, scene.size.h));
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
        let damage = std::mem::take(&mut self.raster);
        if audit::on() && !self.renderer.is_gpu() {
            self.audit = Some(audit::reference(scene, self.renderer.scale()));
        }
        self.commit_frame(damage, scene.size, request, qh);
    }

    pub fn is_gpu(&self) -> bool {
        self.renderer.is_gpu()
    }

    /// The canvas's size in device pixels.
    pub fn pixels(&self) -> (i32, i32) {
        self.renderer.pixels()
    }

    /// The CPU canvas as it stands: what the last frame drew.
    pub fn canvas_pixmap(&self) -> Option<vello_cpu::Pixmap> {
        (!self.renderer.is_gpu()).then(|| self.renderer.canvas().clone())
    }

    /// A whole frame written straight into the CPU canvas by `fill` (its
    /// bytes and size in device pixels) rather than drawn from a scene,
    /// then committed with a callback asked for. Skipped while a callback
    /// is outstanding; false when `fill` refused the canvas. The scene
    /// drawn next has to be touched whole, since the canvas no longer
    /// holds what it last drew.
    pub fn present_pixels<D: Dispatcher>(&mut self, size: IRect, fill: impl FnOnce(&mut [u8], (i32, i32)) -> bool, qh: &QueueHandle<D>) -> bool {
        self.scaler.follow(self.layer.role_surface(), qh);
        if self.frame_pending || !self.configured || self.renderer.is_gpu() {
            return true;
        }
        if (self.renderer.width() as i32, self.renderer.height() as i32) != (size.w, size.h) {
            return false;
        }
        let t0 = Instant::now();
        let pixels = self.renderer.pixels();
        if !fill(self.renderer.canvas_bytes_mut(), pixels) {
            return false;
        }
        (self.react_us, self.render_us, self.slices) = (self.landed_at.take().map_or(0, |at| at.elapsed().as_micros()), t0.elapsed().as_micros(), 1);
        self.left.clear();
        self.raster.clear();
        self.commit_frame(vec![size], size, true, qh);
        true
    }

    /// The canvas's `damage` copied into a buffer and committed.
    fn commit_frame<D: Dispatcher>(&mut self, mut damage: Vec<IRect>, size: IRect, request: bool, qh: &QueueHandle<D>) {
        let (react_us, render_us) = (self.react_us, self.render_us);
        if std::mem::take(&mut self.refade) {
            damage.extend(self.drawn);
        }
        self.drawn = damage.iter().fold(self.drawn, |u, r| Some(u.map_or(*r, |u| u.union(r))));

        let t1 = Instant::now();
        let (pw, ph) = self.renderer.pixels();
        let at = if self.renderer.is_gpu() { self.fill_dmabuf(&damage, size) } else { Some(self.fill_shm(&damage, size)) };
        let Some(at) = at else {
            // No buffer to draw into: the frame waits for the next present.
            self.raster = damage;
            return;
        };
        let copy_us = t1.elapsed().as_micros();

        let scale = self.renderer.scale();
        let surface = self.layer.role_surface();
        for rect in &damage {
            let rect = self.renderer.device(*rect);
            surface.damage_buffer(rect.x, rect.y, rect.w, rect.h);
        }
        self.scaler.sync(surface, scale, (size.w, size.h), qh);
        // Until the first callback the surface may not be on screen yet,
        // and an enter waits for it.
        if request {
            surface.frame(qh, FrameCallbackData(surface.clone()));
            self.frame_pending = true;
            self.asked = Some(Instant::now());
        }
        if self.renderer.is_gpu() {
            let b = &self.dmabufs[at];
            b.attached();
            surface.attach(Some(b.wl_buffer()), 0, 0);
        } else {
            self.buffers[at].buffer.attach_to(surface).expect("attach released buffer");
        }
        self.layer.role_commit();
        self.front = Some(at);
        self.retired = None;
        self.retired_dmabuf = None;
        self.front_view = Some((scale, (size.w, size.h)));

        self.commits += 1;
        if !trace::enabled() {
            return;
        }
        let area: i64 = damage.iter().map(IRect::area).sum();
        let rects: Vec<String> = damage.iter().map(|r| format!("{},{},{}x{}", r.x, r.y, r.w, r.h)).collect();
        let scaled = if scale == 1.0 { String::new() } else { format!(" scale={scale} buffer_px={pw}x{ph}") };
        trace::commit(
            self.name,
            self.commits,
            trace::since(self.started),
            format_args!(
                "render_us={} copy_us={} rects={} px={} damage=[{}] buffer={} frame_callbacks={} request={} last_wait_us={} react_us={} slices={}{scaled}",
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
            ),
        );
    }

    /// The canvas copied into a released shm buffer, made if none is: the
    /// frame's damage plus whatever that buffer missed meanwhile.
    fn fill_shm(&mut self, damage: &[IRect], size: IRect) -> usize {
        let (pw, ph) = self.renderer.pixels();
        let at = match self.buffers.iter().position(|b| b.buffer.canvas(&mut self.pool).is_some()) {
            Some(at) => at,
            None => {
                let (buffer, _) = self
                    .pool
                    .create_buffer(pw, ph, pw * 4, self.format.wl())
                    .expect("wl_shm buffer");
                let stale = if self.pool_clean { self.drawn.into_iter().collect() } else { vec![size] };
                self.buffers.push(ShmBuffer { buffer, stale });
                self.buffers.len() - 1
            }
        };
        for (i, b) in self.buffers.iter_mut().enumerate() {
            if i != at {
                go_stale(&mut b.stale, damage);
            }
        }
        let stale = std::mem::take(&mut self.buffers[at].stale);
        let copy = self.copy_list(stale, damage, size);
        let target = &mut self.buffers[at];
        let canvas = target.buffer.canvas(&mut self.pool).expect("released buffer");
        let stride = pw as usize;
        let k = self.opacity;
        // Premultiplied, so all four channels scale alike.
        let fade = |c: u8| ((u16::from(c) * k + 128) >> 8) as u8;
        let format = self.format;
        let src = self.renderer.canvas().data_as_u8_slice();
        for rect in &copy {
            for row in rect.y..rect.bottom() {
                let start = (row as usize * stride + rect.x as usize) * 4;
                let end = start + rect.w as usize * 4;
                let (from, to) = (&src[start..end], &mut canvas[start..end]);
                if k == 256 && format == Format::Rgba {
                    to.copy_from_slice(from);
                    continue;
                }
                for (px, dst) in from.as_chunks::<4>().0.iter().zip(to.as_chunks_mut::<4>().0) {
                    *dst = format.pixel(if k < 256 { px.map(fade) } else { *px });
                }
            }
        }
        if let Some(reference) = self.audit.take() {
            audit::compare(self.name, self.commits + 1, &reference, canvas, format, k);
        }
        at
    }

    /// The GPU canvas copied into a dmabuf the compositor has given back,
    /// made if none is; None when the device can no longer make one.
    fn fill_dmabuf(&mut self, damage: &[IRect], size: IRect) -> Option<usize> {
        let (pw, ph) = self.renderer.pixels();
        let at = match crate::gpu::released(&self.dmabufs) {
            Some(at) => at,
            None => {
                let mut b = crate::gpu::Buffer::new(pw as u32, ph as u32)?;
                // Fresh device memory holds nothing known.
                b.stale = vec![size];
                self.dmabufs.push(b);
                self.dmabufs.len() - 1
            }
        };
        for (i, b) in self.dmabufs.iter_mut().enumerate() {
            if i != at {
                go_stale(&mut b.stale, damage);
            }
        }
        let stale = std::mem::take(&mut self.dmabufs[at].stale);
        let copy = self.copy_list(stale, damage, size);
        let opacity = self.opacity;
        let canvas = self.renderer.gpu_canvas()?;
        self.dmabufs[at].fill(canvas, &copy, opacity).then_some(at)
    }

    /// `stale` and `damage` as canvas pixels, a rect inside another dropped.
    fn copy_list(&self, mut copy: Vec<IRect>, damage: &[IRect], size: IRect) -> Vec<IRect> {
        copy.extend_from_slice(damage);
        copy.iter()
            .enumerate()
            .filter(|(i, r)| !copy.iter().enumerate().any(|(j, o)| j != *i && o.contains(r) && (o != *r || j < *i)))
            .map(|(_, r)| self.renderer.device(r.intersect(&size)))
            .filter(|r| !r.is_empty())
            .collect()
    }

    /// A surface drawn on the CPU while the device was still opening.
    fn enter_gpu(&mut self, scene: &mut Scene) {
        let (w, h, scale) = (self.renderer.width(), self.renderer.height(), self.renderer.scale());
        let Some(mut renderer) = Renderer::gpu(1, 1) else { return };
        self.drop_buffers();
        self.retired_dmabuf = None;
        renderer.set_scale(scale);
        renderer.resize(w, h);
        self.renderer = renderer;
        scene.touch(IRect::new(0, 0, scene.size.w, scene.size.h));
    }

    /// The GPU is gone: a CPU canvas of the same size and scale, and the
    /// whole scene drawn into it again.
    fn leave_gpu(&mut self, scene: &mut Scene) {
        let (w, h, scale) = (self.renderer.width(), self.renderer.height(), self.renderer.scale());
        self.drop_buffers();
        self.retired_dmabuf = None;
        let mut renderer = Renderer::new(1, 1);
        renderer.set_scale(scale);
        renderer.resize(w, h);
        self.renderer = renderer;
        self.pool_clean = false;
        scene.touch(IRect::new(0, 0, scene.size.w, scene.size.h));
    }

    pub fn report(&self) -> String {
        report_line(&self.name, self.commits, self.callbacks)
    }
}

fn report_line(name: &str, commits: impl std::fmt::Display, callbacks: impl std::fmt::Display) -> String {
    format!("{name}: commits={commits} frame_callbacks={callbacks}")
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
    frame_pending: bool,
    mapped: bool,
    started: Instant,
    commits: u64,
    callbacks: u64,
}

impl PixelSurface {
    pub fn new<D: Dispatcher>(name: &'static str, layer: LayerSurface, alpha: f64, pixels: &Pixels, qh: &QueueHandle<D>, started: Instant) -> Self {
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

    /// True for the first callback, which is when the surface is on screen.
    pub fn landed(&mut self) -> bool {
        self.frame_pending = false;
        self.callbacks += 1;
        !std::mem::replace(&mut self.mapped, true)
    }

    /// The first frame callback has landed.
    pub fn mapped(&self) -> bool {
        self.mapped
    }

    /// A frame callback is outstanding.
    pub fn frame_pending(&self) -> bool {
        self.frame_pending
    }

    /// `fade` multiplies the pixel's own alpha, 0 to 1.
    pub fn present<D: Dispatcher>(&mut self, fade: f64, animating: bool, qh: &QueueHandle<D>) {
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
        trace::commit(
            self.name,
            self.commits,
            trace::since(self.started),
            format_args!(
                "render_us={} copy_us=0 rects=1 px=0 damage=[0,0,1x1] buffer=pixel alpha={:.3} frame_callbacks={} request={}",
                t0.elapsed().as_micros(),
                self.alpha * fade,
                self.callbacks,
                request as u8,
            ),
        );
    }

    pub fn report(&self) -> String {
        report_line(&self.name, self.commits, self.callbacks)
    }

    /// The multiplier last committed, 0 to 1.
    pub fn fade(&self) -> f64 {
        self.multiplier.map_or(0.0, |m| f64::from(m) / f64::from(u32::MAX))
    }

    pub fn commits(&self) -> u64 {
        self.commits
    }
}

impl Drop for PixelSurface {
    fn drop(&mut self) {
        self.fade.destroy();
        self.viewport.destroy();
        self.buffer.destroy();
    }
}

/// The globals a [`PixelSurface`] and a subsurface are made of.
pub struct Pixels {
    pub subcompositor: WlSubcompositor,
    pub viewporter: WpViewporter,
    pub single_pixel: WpSinglePixelBufferManagerV1,
    pub alpha: WpAlphaModifierV1,
    /// One clear pixel, attached to any surface showing nothing.
    pub clear: WlBuffer,
    /// One opaque black pixel.
    pub black: WlBuffer,
}

impl Pixels {
    /// The viewporter is the scaling one ([`bind_scaling`]), bound once.
    pub fn bind<D: Dispatcher>(globals: &GlobalList, qh: &QueueHandle<D>) -> Result<Self, BindError> {
        bind_scaling(globals, qh);
        let viewporter = SCALING.get().and_then(|s| s.viewporter.clone()).ok_or(BindError::NotPresent)?;
        let single_pixel: WpSinglePixelBufferManagerV1 = globals.bind(qh, 1..=1, Ignore)?;
        let clear = single_pixel.create_u32_rgba_buffer(0, 0, 0, 0, qh, Ignore);
        let black = single_pixel.create_u32_rgba_buffer(0, 0, 0, u32::MAX, qh, Ignore);
        Ok(Self { subcompositor: globals.bind(qh, 1..=1, Ignore)?, viewporter, single_pixel, alpha: globals.bind(qh, 1..=1, Ignore)?, clear, black })
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
