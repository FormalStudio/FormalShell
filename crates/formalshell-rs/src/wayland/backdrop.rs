//! The desktop's own layers: the wallpaper's [`Backdrop`] and the
//! sculpture's [`Shards`] over it.

use std::time::Instant;

use fs_chrome::grid::Grid;
use smithay_client_toolkit::compositor::FrameCallbackData;
use smithay_client_toolkit::reexports::client::QueueHandle;
use smithay_client_toolkit::reexports::client::protocol::wl_surface::WlSurface;
use smithay_client_toolkit::reexports::protocols::wp::viewporter::client::wp_viewport::WpViewport;
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::LayerSurface;
use smithay_client_toolkit::shm::Shm;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};

use crate::render::scaled;
use fs_types::IRect;
use crate::services::wallpaper::Picture;
use crate::shards::{Look, Painted, Raster};
use crate::surface::{Dispatcher, Format, Ignore, Pixels, Scaler};
use crate::trace;

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
    /// While the GPU draws: the pictures a frame blends, in device memory,
    /// and the dmabufs it blends them into, with their size.
    textures: Vec<(PictureKey, crate::gpu::Picture)>,
    dmabufs: Vec<crate::gpu::Buffer>,
    dmabuf_size: (i32, i32),
    size: Option<(i32, i32)>,
    drawn: Option<(String, Option<usize>, i32, i32, [u8; 4], Option<Grid>)>,
    /// What is on screen once any fade lands.
    shown: Option<std::sync::Arc<Picture>>,
    fade: Option<Fade>,
    /// The logical size the buffer on screen was painted for.
    painted: Option<(i32, i32)>,
    /// The sculpture over the picture, while `wallpaper.animate` is on.
    pub shards: Option<Shards>,
    /// A first frame and a first whole picture committed, and then shown:
    /// what the session's arrival waits on.
    committed: (bool, bool),
    pub landed: (bool, bool),
}

/// What a backdrop picture holds: a wallpaper by its path, dither and
/// grid, or the plain colour under its grid. A size change drops them all.
type PictureKey = (String, Option<usize>, Option<Grid>, [u8; 4], Option<Grid>);

struct Fade {
    from: Option<std::sync::Arc<Picture>>,
    to: Option<std::sync::Arc<Picture>>,
    color: [u8; 4],
    grid: Option<Grid>,
    start: Instant,
    ms: f64,
}

impl Backdrop {
    pub fn new(layer: LayerSurface, shm: &Shm, scale: f64) -> Self {
        Self {
            scaler: Scaler::new(scale),
            layer,
            pool: SlotPool::new(4096, shm).expect("wl_shm pool"),
            buffer: None,
            textures: Vec::new(),
            dmabufs: Vec::new(),
            dmabuf_size: (0, 0),
            size: None,
            drawn: None,
            shown: None,
            fade: None,
            painted: None,
            shards: None,
            committed: (false, false),
            landed: (false, false),
        }
    }

    /// The backdrop's frame callback.
    pub fn landed<D: Dispatcher>(&mut self, now: Instant, qh: &QueueHandle<D>) {
        self.landed = self.committed;
        self.step(now, qh);
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
    pub fn sync_shards<D: Dispatcher>(&mut self, look: Look, grid: Option<Grid>, run: Result<(), &'static str>, now: Instant, qh: &QueueHandle<D>) -> bool {
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
        let moved = s.look.as_ref() != Some(&look) || s.drawn.is_none_or(|(_, d)| d != device);
        s.look = Some(look);
        // Anything else waits for the beat, which keeps the cap.
        if moved || (started && !s.pending) {
            s.draw(device, logical, now, qh);
        }
        s.running
    }

    /// One beat of the sculpture's clock: a frame, unless the last one is
    /// still waiting on the compositor. False once it has stopped.
    pub fn shard_tick<D: Dispatcher>(&mut self, now: Instant, motion_scale: f64, qh: &QueueHandle<D>) -> bool {
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

    /// Paints what is up again, on the GPU once it has come up.
    pub fn repaint<D: Dispatcher>(&mut self, qh: &QueueHandle<D>) {
        let Some((_, _, _, _, color, grid)) = self.drawn.clone() else { return };
        if self.fade.is_some() {
            return;
        }
        let shown = self.shown.clone();
        self.paint(None, shown.as_deref(), color, grid, 1.0, qh);
    }

    /// Already showing `path` at this logical size, though at another
    /// scale: it stays up while the picture for the new scale is made.
    pub fn holds(&self, path: &str) -> bool {
        self.painted == self.size && self.shown.as_ref().is_some_and(|p| p.path == path)
    }

    /// `picture` when it was made for this size, else the plain colour; a
    /// change after the first paint fades over `reveal_ms`.
    pub fn draw<D: Dispatcher>(
        &mut self,
        picture: Option<std::sync::Arc<Picture>>,
        background: fs_theme::color::Rgba,
        grid: Option<Grid>,
        reveal_ms: f64,
        qh: &QueueHandle<D>,
    ) {
        self.scaler.follow(self.layer.wl_surface(), qh);
        let Some((w, h)) = self.pixels() else { return };
        let picture = picture.filter(|p| (p.width as i32, p.height as i32) == (w, h));
        let [r, g, b, _] = background.to_u8();
        // The pictures are BGRA, so the whole layer is.
        let color = Format::Bgra.pixel([r, g, b, 255]);
        let key = (picture.as_ref().map(|p| p.path.clone()).unwrap_or_default(), picture.as_ref().and_then(|p| p.dither), w, h, color, grid);
        if self.drawn.as_ref() == Some(&key) {
            return;
        }
        let first = self.drawn.is_none();
        self.drawn = Some(key);
        // The same picture under a new colour or grid (the palette a mode
        // change reruns lands mid-fade) carries on the running fade.
        let same = |a: &Option<std::sync::Arc<Picture>>| match (a, &picture) {
            (Some(a), Some(b)) => (&a.path, a.dither) == (&b.path, b.dither),
            (None, None) => true,
            _ => false,
        };
        if let Some(f) = self.fade.as_mut().filter(|f| same(&f.to)) {
            (f.color, f.grid) = (color, grid);
            self.shown = picture;
            self.step(Instant::now(), qh);
            return;
        }
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
    pub fn step<D: Dispatcher>(&mut self, now: Instant, qh: &QueueHandle<D>) {
        let Some(f) = &self.fade else { return };
        let t = crate::motion::progress(f.start, f.ms, now);
        let a = crate::motion::Kind::EffectsSlow.curve().ease(t);
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
    fn paint<D: Dispatcher>(
        &mut self,
        from: Option<Option<&Picture>>,
        to: Option<&Picture>,
        color: [u8; 4],
        grid: Option<Grid>,
        a: f32,
        qh: &QueueHandle<D>,
    ) {
        let Some(size) = self.size else { return };
        let scale = self.scaler.wanted(self.layer.wl_surface());
        let (w, h) = (scaled(size.0, scale), scaled(size.1, scale));
        // The grid rides on its own layer over the sculpture.
        let grid = grid.filter(|_| self.shards.is_none());
        // A picture made for the output's last scale (an output given its
        // scale just after it appeared) waits for its own size; the plain
        // colour stands in until it lands.
        let len = w as usize * h as usize * 4;
        let fits = |p: &&Picture| p.bgra.len() == len;
        let (from, to) = (from.map(|f| f.filter(fits)), to.filter(fits));
        let plain = |canvas: &mut [u8]| {
            for px in canvas.chunks_exact_mut(4) {
                px.copy_from_slice(&color);
            }
            if let Some(g) = grid {
                Grid { color: Format::Bgra.pixel(g.color), ..g }.paint(canvas, w as usize, h as usize);
            }
        };
        let surface = self.layer.wl_surface().clone();
        let on_gpu = crate::gpu::active().then(|| self.blend_on_gpu(from, to, (color, grid), a, (w, h), &plain)).flatten();
        if let Some(at) = on_gpu {
            surface.attach(Some(self.dmabufs[at].wl_buffer()), 0, 0);
            self.dmabufs[at].attached();
            self.buffer = None;
        } else {
            self.textures.clear();
            self.dmabufs.clear();
            let Ok((buffer, canvas)) = self.pool.create_buffer(w, h, w * 4, Format::Bgra.wl()) else { return };
            Self::blend_on_cpu(canvas, from, to, a, &plain);
            if buffer.attach_to(&surface).is_err() {
                return;
            }
            self.buffer = Some(buffer);
        }
        self.scaler.sync(&surface, scale, size, qh);
        surface.damage_buffer(0, 0, w, h);
        let committed = (true, self.committed.1 || (from.is_none() && to.is_some()));
        if committed != self.committed && from.is_none() {
            surface.frame(qh, FrameCallbackData(surface.clone()));
        }
        self.committed = committed;
        self.layer.commit();
        self.painted = Some(size);
        if from.is_none() {
            trace::commit_bare("wallpaper", format_args!("{}x{} picture={} gpu={}", w, h, to.is_some(), on_gpu.is_some()));
        }
    }

    /// `to` over `from` at `a` into a dmabuf, the pictures uploaded to
    /// device memory the first time a frame asks for them and dropped once
    /// no frame does; none when the GPU could not.
    fn blend_on_gpu(
        &mut self,
        from: Option<Option<&Picture>>,
        to: Option<&Picture>,
        (color, grid): ([u8; 4], Option<Grid>),
        a: f32,
        (w, h): (i32, i32),
        plain: &dyn Fn(&mut [u8]),
    ) -> Option<usize> {
        let key = |p: Option<&Picture>| -> PictureKey {
            p.map_or_else(|| (String::new(), None, None, color, grid), |p| (p.path.clone(), p.dither, p.grid, [0; 4], None))
        };
        if self.dmabuf_size != (w, h) {
            self.textures.clear();
            self.dmabufs.clear();
            self.dmabuf_size = (w, h);
        }
        let wanted: Vec<_> = std::iter::once(to).chain(from).map(|p| (key(p), p)).collect();
        self.textures.retain(|(k, _)| wanted.iter().any(|(w, _)| w == k));
        for (k, p) in &wanted {
            if self.textures.iter().any(|(have, _)| have == k) {
                continue;
            }
            let picture = match p {
                Some(p) => crate::gpu::Picture::bgra(w as u32, h as u32, &p.bgra)?,
                None => {
                    let mut flat = vec![0; w as usize * h as usize * 4];
                    plain(&mut flat);
                    crate::gpu::Picture::bgra(w as u32, h as u32, &flat)?
                }
            };
            self.textures.push((k.clone(), picture));
        }
        let at = match crate::gpu::released(&self.dmabufs) {
            Some(at) => at,
            None => {
                self.dmabufs.push(crate::gpu::Buffer::new(w as u32, h as u32)?);
                self.dmabufs.len() - 1
            }
        };
        let get = |k: &PictureKey| self.textures.iter().find(|(have, _)| have == k).map(|(_, t)| t);
        let dst = get(&wanted[0].0)?;
        let (src, a) = match wanted.get(1) {
            Some((k, _)) => (get(k)?, a),
            None => (dst, 1.0),
        };
        self.dmabufs[at].blend(src, dst, a).then_some(at)
    }

    /// The same blend into a CPU canvas, in its own BGRA bytes.
    fn blend_on_cpu(
        canvas: &mut [u8],
        from: Option<Option<&Picture>>,
        to: Option<&Picture>,
        a: f32,
        plain: &dyn Fn(&mut [u8]),
    ) {
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
                let pick = |p: Option<&Picture>, i: usize| p.map_or_else(|| flat[i], |p| p.bgra[i]);
                for (i, c) in canvas.iter_mut().enumerate() {
                    let s0 = u32::from(pick(src, i));
                    let d0 = u32::from(pick(dst, i));
                    *c = ((s0 * (256 - k) + d0 * k) >> 8) as u8;
                }
            }
        }
    }
}

/// The sculpture's two layers over a [`Backdrop`]'s picture: the shards
/// and the grid over them, both at the output's own pixels, mapped onto its
/// logical size by their viewports; the grid is drawn once.
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
    buffers: Vec<(Buffer, Option<Painted>)>,
    grid_pool: SlotPool,
    grid_buffer: Option<Buffer>,
    grid_drawn: Option<((i32, i32), Option<Grid>)>,
    format: Format,
    raster: Raster,
    look: Option<Look>,
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
    pub fn new<D: Dispatcher>(layer: LayerSurface, grid: LayerSurface, pixels: &Pixels, shm: &Shm, qh: &QueueHandle<D>, started: Instant) -> Self {
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
            format: Format::of(shm),
            raster: Raster::new(),
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

    /// The shards' frame callback.
    pub fn landed(&mut self) {
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
            let Ok((buffer, canvas)) = self.grid_pool.create_buffer(1, 1, 4, self.format.wl()) else { return };
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
        let Ok((buffer, canvas)) = self.grid_pool.create_buffer(w, h, w * 4, self.format.wl()) else { return };
        canvas.fill(0);
        Grid { color: self.format.pixel(g.color), ..g }.coat(canvas, w as usize, h as usize);
        if buffer.attach_to(surface).is_err() {
            return;
        }
        self.grid_viewport.set_destination(logical.0, logical.1);
        surface.damage_buffer(0, 0, w, h);
        self.grid.commit();
        self.grid_buffer = Some(buffer);
    }

    /// The frame at the clock's `t`, committed with a callback asked for.
    fn draw<D: Dispatcher>(&mut self, device: (i32, i32), logical: (i32, i32), now: Instant, qh: &QueueHandle<D>) {
        let Some(look) = &self.look else { return };
        let (w, h) = device;
        let (Ok(bw), Ok(bh)) = (u16::try_from(w), u16::try_from(h)) else { return };
        let t0 = Instant::now();
        // Two buffers in turn, a third only while both are on screen.
        self.buffers.retain(|(b, _)| b.height() == h && b.stride() == w * 4);
        let free = self.buffers.iter().position(|(b, _)| self.pool.canvas(b).is_some());
        let at = match free {
            Some(i) => i,
            None if self.buffers.len() >= 3 => return,
            None => {
                let Ok((buffer, _)) = self.pool.create_buffer(w, h, w * 4, self.format.wl()) else { return };
                self.buffers.push((buffer, None));
                self.buffers.len() - 1
            }
        };
        let resized = self.drawn.is_none_or(|(_, d)| d != (w, h));
        let (buffer, prior) = &mut self.buffers[at];
        let Some(canvas) = self.pool.canvas(buffer) else { return };
        // A buffer swapped to BGRA whole can only be painted whole.
        let keep = if self.format == Format::Rgba { *prior } else { None };
        let painted = self.raster.draw(canvas, bw, bh, self.t, look, keep);
        *prior = Some(painted);
        let damage = painted.shards;
        self.format.swizzle(canvas);
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
        trace::commit(
            "shards",
            self.commits,
            now.duration_since(self.started).as_millis(),
            format_args!(
                "render_us={} damage=[{},{},{}x{}] buffer={}x{} clock={:.0} running={}",
                render_us, rect.x, rect.y, rect.w, rect.h, w, h, self.t, self.running as u8,
            ),
        );
    }
}

impl Drop for Shards {
    fn drop(&mut self) {
        self.viewport.destroy();
        self.grid_viewport.destroy();
    }
}
