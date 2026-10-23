//! The satellite picture the globe wraps itself in, and the sampler that
//! draws it. The picture is a pyramid of equirectangular levels cut into
//! square tiles (`FS_EARTH_TILES`, nix/earth-tiles.py): the coarsest level
//! is decoded whole on open, every finer tile only once a frame needs it,
//! on a pool thread, into a cache with a byte cap. A frame samples each run
//! of pixels from the finest level that has its tiles, so a tile still
//! decoding shows the level under it rather than a hole. Without the tiles
//! the single picture `FS_EARTH_IMAGE` names is a pyramid of one level.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vello_cpu::Pixmap;

use super::globe::{Rot, light};
use crate::scene::Bitmap;

/// Decoded tiles past the coarsest level held at once; the least recently
/// drawn go first. A view at the finest level needs some 40 of them.
const CACHE_BYTES: usize = 96 << 20;
/// Tiles landing faster than this are answered with one redraw.
const WAKE_EVERY: Duration = Duration::from_millis(33);

/// Runs a decode job off the UI thread.
pub type Spawn = Box<dyn Fn(Box<dyn FnOnce() + Send>) + Send + Sync>;
/// Asks the UI thread to draw again, once finer tiles landed.
pub type Wake = Box<dyn Fn() + Send + Sync>;

#[derive(Clone, Copy, Debug)]
struct Level {
    w: usize,
    h: usize,
    cols: usize,
    rows: usize,
}

impl Level {
    fn new(w: usize, h: usize, tile: usize) -> Self {
        Self { w, h, cols: w.div_ceil(tile), rows: h.div_ceil(tile) }
    }
}

type Id = (usize, usize);

/// One tile's RGB texels, three bytes each and one past the end, so any
/// texel reads as one little-endian u32.
pub struct Tile {
    w: usize,
    h: usize,
    rgb: Vec<u8>,
}

impl Tile {
    fn new(w: usize, h: usize, mut rgb: Vec<u8>) -> Self {
        rgb.reserve_exact(1);
        rgb.push(0);
        Self { w, h, rgb }
    }

    #[inline(always)]
    fn texel(&self, x: usize, y: usize) -> u32 {
        let i = (y * self.w + x) * 3;
        u32::from_le_bytes(self.rgb[i..i + 4].try_into().unwrap_or([0; 4]))
    }
}

struct Shared {
    dir: PathBuf,
    tile: usize,
    levels: Vec<Level>,
    state: Mutex<State>,
    version: AtomicU64,
    wake: Wake,
}

#[derive(Default)]
struct State {
    /// Tiles past the coarsest level, each with the frame that last drew it.
    tiles: HashMap<Id, (Arc<Tile>, u64)>,
    bytes: usize,
    clock: u64,
    /// The last frame's missing tiles at its level, in the order it met them.
    wanted: Vec<Id>,
    failed: HashSet<Id>,
    busy: bool,
    closed: bool,
}

pub struct Earth {
    tile: usize,
    levels: Vec<Level>,
    /// The coarsest level, every tile, row by row.
    base: Vec<Arc<Tile>>,
    /// The finer levels, with no tiles for a single picture.
    shared: Option<Arc<Shared>>,
    spawn: Option<Spawn>,
}

impl std::fmt::Debug for Earth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let widths: Vec<_> = self.levels.iter().map(|l| l.w).collect();
        write!(f, "Earth({widths:?} by {})", self.tile)
    }
}

impl Drop for Earth {
    fn drop(&mut self) {
        if let Some(shared) = &self.shared {
            let mut st = shared.state.lock().unwrap();
            st.closed = true;
            st.tiles.clear();
            st.wanted.clear();
            st.bytes = 0;
        }
    }
}

/// What one frame drew from.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    /// The level the frame asked for, by width.
    pub level: usize,
    /// Its tiles the frame met, and how many of those were not decoded yet.
    pub tiles: usize,
    pub missing: usize,
    /// Decoded tiles held past the coarsest level, and their bytes.
    pub cached: usize,
    pub cached_bytes: usize,
}

/// The picture: the tile pyramid `FS_EARTH_TILES` names, else the single
/// picture `FS_EARTH_IMAGE` names. Run off the UI thread; `spawn` runs the
/// finer tiles' decodes and `wake` reports them landed.
pub fn load(spawn: Spawn, wake: Wake) -> Result<Earth, String> {
    if let Some(dir) = std::env::var_os("FS_EARTH_TILES").filter(|d| !d.is_empty()) {
        let dir = PathBuf::from(dir);
        match Earth::tiled(&dir, spawn, wake) {
            Ok(e) => return Ok(e),
            Err(e) => eprintln!("radio atlas: no satellite tiles, trying the single picture: {}: {e}", dir.display()),
        }
    }
    let path = std::env::var_os("FS_EARTH_IMAGE").ok_or("neither FS_EARTH_TILES nor FS_EARTH_IMAGE is usable")?;
    let shown = path.to_string_lossy().into_owned();
    let image = image::ImageReader::open(&path)
        .map_err(|e| format!("{shown}: {e}"))?
        .decode()
        .map_err(|e| format!("{shown}: {e}"))?
        .into_rgb8();
    let (width, height) = (image.width() as usize, image.height() as usize);
    if width < 2 || height < 2 {
        return Err(format!("{shown}: {width}x{height} is too small"));
    }
    let mut rgb = image.into_raw();
    tone(&mut rgb);
    Ok(Earth::single(width, height, rgb))
}

fn decode(path: &Path, w: usize, h: usize) -> Result<Tile, String> {
    let shown = || path.display().to_string();
    let image = image::ImageReader::open(path)
        .map_err(|e| format!("{}: {e}", shown()))?
        .decode()
        .map_err(|e| format!("{}: {e}", shown()))?
        .into_rgb8();
    if (image.width() as usize, image.height() as usize) != (w, h) {
        return Err(format!("{}: {}x{}, not {w}x{h}", shown(), image.width(), image.height()));
    }
    let mut rgb = image.into_raw();
    tone(&mut rgb);
    Ok(Tile::new(w, h, rgb))
}

impl Shared {
    fn path(&self, (l, i): Id) -> PathBuf {
        let level = self.levels[l];
        self.dir.join(level.w.to_string()).join(format!("{}_{}.jpg", i % level.cols, i / level.cols))
    }

    fn decode(&self, (l, i): Id) -> Result<Tile, String> {
        let level = self.levels[l];
        let (c, r) = (i % level.cols, i / level.cols);
        let w = self.tile.min(level.w - c * self.tile);
        let h = self.tile.min(level.h - r * self.tile);
        decode(&self.path((l, i)), w, h)
    }
}

/// The pool job: decodes what the latest frame still misses, newest wishes
/// first, until nothing is left or the atlas closed.
fn work(shared: &Shared) {
    let mut woke = Instant::now();
    let mut unseen = false;
    loop {
        let id = {
            let mut st = shared.state.lock().unwrap();
            loop {
                if st.closed || st.wanted.is_empty() {
                    st.busy = false;
                    drop(st);
                    if unseen {
                        (shared.wake)();
                    }
                    return;
                }
                let id = st.wanted.remove(0);
                if !st.tiles.contains_key(&id) && !st.failed.contains(&id) {
                    break id;
                }
            }
        };
        let decoded = shared.decode(id);
        let mut st = shared.state.lock().unwrap();
        if st.closed {
            st.busy = false;
            return;
        }
        match decoded {
            Ok(tile) => {
                st.bytes += tile.rgb.capacity();
                let clock = st.clock;
                st.tiles.insert(id, (Arc::new(tile), clock));
                while st.bytes > CACHE_BYTES {
                    let Some(&old) = st.tiles.iter().filter(|(k, _)| **k != id).min_by_key(|(_, (_, at))| *at).map(|(k, _)| k) else { break };
                    if let Some((t, _)) = st.tiles.remove(&old) {
                        st.bytes -= t.rgb.capacity();
                    }
                }
                shared.version.fetch_add(1, Ordering::Relaxed);
                unseen = true;
            }
            Err(e) => {
                eprintln!("radio atlas: {e}");
                st.failed.insert(id);
            }
        }
        drop(st);
        if unseen && woke.elapsed() >= WAKE_EVERY {
            (shared.wake)();
            woke = Instant::now();
            unseen = false;
        }
    }
}

impl Earth {
    /// A pyramid of one level, `rgb` the whole picture as its one tile.
    pub fn single(width: usize, height: usize, rgb: Vec<u8>) -> Self {
        let tile = width.max(height);
        Self { tile, levels: vec![Level::new(width, height, tile)], base: vec![Arc::new(Tile::new(width, height, rgb))], shared: None, spawn: None }
    }

    fn tiled(dir: &Path, spawn: Spawn, wake: Wake) -> Result<Self, String> {
        let manifest = std::fs::read_to_string(dir.join("pyramid.txt")).map_err(|e| format!("pyramid.txt: {e}"))?;
        let mut tile = 0;
        let mut sizes = Vec::new();
        for line in manifest.lines() {
            let words: Vec<usize> = line.split_whitespace().skip(1).filter_map(|w| w.parse().ok()).collect();
            match (line.split_whitespace().next(), words.as_slice()) {
                (Some("tile"), [t]) => tile = *t,
                (Some("level"), [w, h]) => sizes.push((*w, *h)),
                (None, _) => {}
                _ => return Err(format!("pyramid.txt: cannot read {line:?}")),
            }
        }
        if tile < 2 || sizes.is_empty() || sizes.iter().any(|&(w, h)| w < 2 || h < 2) || sizes.windows(2).any(|p| p[1].0 <= p[0].0) {
            return Err("pyramid.txt names no usable levels".into());
        }
        let levels: Vec<Level> = sizes.into_iter().map(|(w, h)| Level::new(w, h, tile)).collect();
        let shared = Arc::new(Shared { dir: dir.to_owned(), tile, levels: levels.clone(), state: Mutex::default(), version: AtomicU64::new(0), wake });
        let base = (0..levels[0].cols * levels[0].rows).map(|i| shared.decode((0, i)).map(Arc::new)).collect::<Result<Vec<_>, _>>()?;
        Ok(Self { tile, levels, base, shared: Some(shared), spawn: Some(spawn) })
    }

    /// Bumped by every finer tile landing, so a still globe draws again.
    pub fn version(&self) -> u64 {
        self.shared.as_ref().map_or(0, |s| s.version.load(Ordering::Relaxed))
    }

    /// The coarsest level at least `texels` wide, else the finest.
    fn level_for(&self, texels: f64) -> usize {
        self.levels.iter().position(|l| l.w as f64 >= texels).unwrap_or(self.levels.len() - 1)
    }

    fn view(&self, top: usize) -> View {
        let st = self.shared.as_ref().map(|s| s.state.lock().unwrap());
        let (tw, th) = (self.levels[top].w as f32, self.levels[top].h as f32);
        let levels = self.levels[..=top]
            .iter()
            .enumerate()
            .map(|(l, lv)| {
                let n = lv.cols * lv.rows;
                let tiles = if l == 0 {
                    self.base.iter().cloned().map(Some).collect()
                } else {
                    (0..n).map(|i| st.as_ref().and_then(|st| st.tiles.get(&(l, i))).map(|(t, _)| t.clone())).collect()
                };
                LevelView { w: lv.w, h: lv.h, cols: lv.cols, sx: lv.w as f32 / tw, sy: lv.h as f32 / th, tiles, touched: vec![false; n] }
            })
            .collect();
        View { tile: self.tile, top, levels, missing: Vec::new(), seen: vec![false; self.levels[top].cols * self.levels[top].rows] }
    }

    /// Marks what the frame drew as recently used and asks for what it
    /// missed.
    fn finish(&self, view: View) -> Stats {
        let mut stats = Stats {
            level: self.levels[view.top].w,
            tiles: view.levels[view.top].touched.iter().filter(|t| **t).count() + view.missing.len(),
            missing: view.missing.len(),
            ..Stats::default()
        };
        let Some(shared) = &self.shared else { return stats };
        let mut st = shared.state.lock().unwrap();
        st.clock += 1;
        let clock = st.clock;
        for (l, lv) in view.levels.iter().enumerate().skip(1) {
            for (i, _) in lv.touched.iter().enumerate().filter(|(_, t)| **t) {
                if let Some(entry) = st.tiles.get_mut(&(l, i)) {
                    entry.1 = clock;
                }
            }
        }
        let wanted: Vec<Id> = view.missing.iter().map(|&i| (view.top, i)).filter(|id| !st.failed.contains(id)).collect();
        st.wanted = wanted;
        stats.cached = st.tiles.len();
        stats.cached_bytes = st.bytes;
        if !st.wanted.is_empty() && !st.busy
            && let Some(spawn) = &self.spawn
        {
            st.busy = true;
            drop(st);
            let shared = shared.clone();
            spawn(Box::new(move || work(&shared)));
        }
        stats
    }
}

struct LevelView {
    w: usize,
    h: usize,
    cols: usize,
    /// This level's texels per texel of the frame's level.
    sx: f32,
    sy: f32,
    tiles: Vec<Option<Arc<Tile>>>,
    touched: Vec<bool>,
}

/// One frame's hold on the pyramid, up to the level it draws at.
struct View {
    tile: usize,
    top: usize,
    levels: Vec<LevelView>,
    /// The frame level's tiles it found missing, in the order it met them.
    missing: Vec<usize>,
    seen: Vec<bool>,
}

/// Where a run of pixels reads from.
enum Source {
    /// Every texel inside one decoded tile of this level.
    Tile(usize, usize),
    /// Texel by texel, from this level down.
    Each(usize),
}

impl View {
    fn want(&mut self, i: usize) {
        if !std::mem::replace(&mut self.seen[i], true) {
            self.missing.push(i);
        }
    }

    /// The finest level whose tiles cover the texels between (`x0`, `y0`)
    /// and (`x1`, `y1`), in the frame level's coordinates, and whether one
    /// tile holds them all.
    fn resolve(&mut self, x0: f32, x1: f32, y0: f32, y1: f32) -> Source {
        const SLACK: f32 = 1e-3;
        let t = self.tile as i64;
        for l in (0..=self.top).rev() {
            let lv = &self.levels[l];
            let (w, h, cols, sx, sy) = (lv.w as i64, lv.h as i64, lv.cols as i64, lv.sx, lv.sy);
            let at = |v: f32, s: f32| (v + 0.5) * s - 0.5;
            let xa = (at(x0, sx) - SLACK).floor() as i64;
            let xb = (at(x1, sx) + SLACK).floor() as i64 + 1;
            let ymax = (h - 1) as f32;
            let ya = ((at(y0, sy).clamp(0.0, ymax) - SLACK).floor() as i64).max(0);
            let yb = ((at(y1, sy).clamp(0.0, ymax) + SLACK).floor() as i64 + 1).min(h - 1);
            if xb - xa >= t || yb - ya >= t {
                return Source::Each(l);
            }
            let wraps = xa < 0 || xb >= w;
            let (ca, cb) = (xa.rem_euclid(w) / t, xb.rem_euclid(w) / t);
            let (ra, rb) = (ya / t, yb / t);
            let mut all = true;
            for r in [ra, rb] {
                for c in [ca, cb] {
                    let i = (r * cols + c) as usize;
                    if self.levels[l].tiles[i].is_some() {
                        self.levels[l].touched[i] = true;
                    } else {
                        all = false;
                        if l == self.top {
                            self.want(i);
                        }
                    }
                }
            }
            if all {
                return if !wraps && ca == cb && ra == rb { Source::Tile(l, (ra * cols + ca) as usize) } else { Source::Each(l) };
            }
        }
        Source::Each(0)
    }

    /// The bilinear texel at (`fx`, `fy`) in the frame level's coordinates,
    /// red and blue side by side in one word and green on its own, off the
    /// finest level from `from` down whose four texels are decoded.
    fn bilinear(&mut self, from: usize, fx: f32, fy: f32) -> (u32, u32) {
        let t = self.tile;
        for l in (0..=from).rev() {
            let lv = &self.levels[l];
            let (w, h, cols) = (lv.w, lv.h, lv.cols);
            let x = (fx + 0.5) * lv.sx - 0.5;
            let y = ((fy + 0.5) * lv.sy - 0.5).clamp(0.0, (h - 1) as f32);
            let (gx, gy) = (x.floor(), y.floor());
            let (wx, wy) = (((x - gx) * 256.0) as u32, ((y - gy) * 256.0) as u32);
            let x0 = (gx as i64).rem_euclid(w as i64) as usize;
            let x1 = if x0 + 1 == w { 0 } else { x0 + 1 };
            let y0 = (gy as usize).min(h - 1);
            let y1 = (y0 + 1).min(h - 1);
            let mut px = [0u32; 4];
            let mut all = true;
            for (k, (tx, ty)) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].into_iter().enumerate() {
                let i = (ty / t) * cols + tx / t;
                match &self.levels[l].tiles[i] {
                    Some(tile) => {
                        px[k] = tile.texel(tx % t, ty % t);
                        self.levels[l].touched[i] = true;
                    }
                    None => {
                        all = false;
                        if l == self.top {
                            self.want(i);
                        }
                    }
                }
            }
            if all {
                return mix4(px, wx, wy);
            }
        }
        (0, 0)
    }
}

#[inline(always)]
fn lerp(a: u32, b: u32, t: u32) -> u32 {
    ((a * (256 - t) + b * t) >> 8) & 0x00ff_00ff
}

#[inline(always)]
fn mix4([p00, p01, p10, p11]: [u32; 4], wx: u32, wy: u32) -> (u32, u32) {
    let rb = |p: u32| p & 0x00ff_00ff;
    let g = |p: u32| (p >> 8) & 0xff;
    (lerp(lerp(rb(p00), rb(p01), wx), lerp(rb(p10), rb(p11), wx), wy), lerp(lerp(g(p00), g(p01), wx), lerp(g(p10), g(p11), wx), wy))
}

/// Blue Marble is mastered dark for print: open ocean sits near (7, 21, 52)
/// and reads as black on a screen. A gamma of 1/1.6 lifts the shadows and
/// midtones while white stays white, and a 1.15 saturation about the luma
/// gives back the colour the lift washes out. Run once a tile on decode, so
/// the sampler's per-pixel work is unchanged.
const TONE_GAMMA: f32 = 1.0 / 1.6;
const TONE_SATURATION: f32 = 1.15;

fn tone(rgb: &mut [u8]) {
    let lut: [f32; 256] = std::array::from_fn(|i| (i as f32 / 255.0).powf(TONE_GAMMA) * 255.0);
    for px in rgb.chunks_exact_mut(3) {
        let [r, g, b] = [lut[px[0] as usize], lut[px[1] as usize], lut[px[2] as usize]];
        let y = 0.299 * r + 0.587 * g + 0.114 * b;
        for (out, c) in px.iter_mut().zip([r, g, b]) {
            *out = (y + (c - y) * TONE_SATURATION).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// The run of pixels the sampler draws between two exactly projected
/// ones, and the shorter run it takes over the outer rim of the disc, where
/// the projection bends hardest.
const SPAN: i32 = 16;
const RIM_SPAN: i32 = 4;

/// The atmosphere seen edge on: its blue laid over the picture from
/// `HAZE_FROM` of the way out, rising with the cube of the distance into
/// it to `HAZE_MAX` at the limb, the way the air thickens along a grazing
/// line of sight.
pub const ATMOSPHERE: [u8; 3] = [118, 170, 255];
const HAZE_FROM: f32 = 0.55;
const HAZE_MAX: f32 = 0.55;
const HAZE_RB: u32 = (ATMOSPHERE[2] as u32) << 16 | ATMOSPHERE[0] as u32;
const HAZE_G: u32 = ATMOSPHERE[1] as u32;

/// atan2 to within 1e-5 rad, a thirtieth of a texel on the 21600 wide
/// level, without the libm call per pixel.
#[inline(always)]
pub(super) fn fast_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let (lo, hi) = if ax > ay { (ay, ax) } else { (ax, ay) };
    if hi == 0.0 {
        return 0.0;
    }
    let a = lo / hi;
    let s = a * a;
    let mut r = a * (0.999_977_26 + s * (-0.332_623_47 + s * (0.193_543_46 + s * (-0.116_432_87 + s * (0.052_653_32 + s * -0.011_721_2)))));
    if ay > ax {
        r = std::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = std::f32::consts::PI - r;
    }
    if y < 0.0 { -r } else { r }
}

impl Earth {
    /// The near-side view of the picture over `area` (the disc's box cut to
    /// the pane, in buffer pixels), centred at `centre` with `r` pixels to
    /// the unit and the camera `distance` sphere radii out, turned by
    /// `rot`, and lit by `light` the way the flat globe's land is. Pixels
    /// off the sphere stay clear; the caller clips the edge.
    ///
    /// The level is the coarsest with a texel or more per pixel at the
    /// disc's centre, where a pixel spans 1/`r` radians.
    ///
    /// The pixels land in a buffer out of `pool` nothing else holds any
    /// more (the frame before last's), so a drag maps no fresh pages a
    /// frame. `coarse` samples every other pixel of every other row and
    /// doubles each, for a globe in motion.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn sample(&self, pool: &mut Vec<Arc<Pixmap>>, coarse: bool, area: (i32, i32, i32, i32), centre: (f64, f64), r: f64, horizon_px: f64, distance: f64, rot: &Rot) -> Option<(Bitmap, Stats)> {
        let (x0, y0, x1, y1) = area;
        let (bw, bh) = ((x1 - x0).max(0) as usize, (y1 - y0).max(0) as usize);
        if bw == 0 || bh == 0 || bw > u16::MAX as usize || bh > u16::MAX as usize {
            return None;
        }
        let top = self.level_for(std::f64::consts::TAU * r);
        let mut view = self.view(top);
        let i = match pool.iter_mut().position(|p| Arc::get_mut(p).is_some()) {
            Some(i) => i,
            None => {
                pool.push(Arc::new(Pixmap::new(bw as u16, bh as u16)));
                pool.len() - 1
            }
        };
        let pixmap = Arc::get_mut(&mut pool[i])?;
        pixmap.resize(bw as u16, bh as u16);
        pixmap.set_may_have_transparency(true);
        let out = pixmap.data_as_u8_slice_mut();
        let (cx, cy) = (centre.0 as f32, centre.1 as f32);
        let inv_r = 1.0 / r as f32;
        let d = distance as f32;
        let dm1 = d - 1.0;
        let dd = d * dm1;
        let k2 = d * d - 1.0;
        let reach = horizon_px as f32 + 1.0;
        let inv_horizon = 1.0 / (horizon_px as f32).max(1.0);
        let (cl, sl) = (rot.cos_lat as f32, rot.sin_lat as f32);
        let (co, so) = (rot.cos_lon as f32, rot.sin_lon as f32);
        let l = light();
        let (l0, l1, l2) = (l[0] as f32, l[1] as f32, l[2] as f32);
        let (wf, hf) = (self.levels[top].w as f32, self.levels[top].h as f32);
        let to_x = wf / std::f32::consts::TAU;
        let to_y = hf / std::f32::consts::PI;
        let step = if coarse { 2 } else { 1 };
        for j in 0..bh {
            if coarse && j % 2 == 1 {
                out.copy_within((j - 1) * bw * 4..j * bw * 4, j * bw * 4);
                continue;
            }
            let py = (y0 + j as i32) as f32 + 0.5;
            let dy = py - cy;
            let half2 = reach * reach - dy * dy;
            let row = &mut out[j * bw * 4..(j + 1) * bw * 4];
            if half2 <= 0.0 {
                row.fill(0);
                continue;
            }
            let half = half2.sqrt();
            let xs = ((cx - half).floor() as i32).clamp(x0, x1);
            let xe = ((cx + half).ceil() as i32).clamp(xs, x1);
            row[..(xs - x0) as usize * 4].fill(0);
            row[(xe - x0) as usize * 4..].fill(0);
            let v = -dy * inv_r;
            let vv = v * v + dm1 * dm1;
            // The picture's coordinates at the frame's level and the light
            // at a pixel centre, fx unwrapped (-0.5 up to the width).
            let exact = |x: i32| {
                let u = ((x as f32) + 0.5 - cx) * inv_r;
                let a = u * u + vv;
                let disc = (dd * dd - a * k2).max(0.0);
                let t = (dd - disc.sqrt()) / a;
                let (vx, vy, vz) = (t * u, t * v, d - t * dm1);
                // The view frame back to the world: Rot::apply inverted.
                let p2 = cl * vy + sl * vz;
                let hz = cl * vz - sl * vy;
                let p0 = hz * co - vx * so;
                let p1 = hz * so + vx * co;
                let lon = fast_atan2(p1, p0);
                let lat = fast_atan2(p2, (1.0 - p2 * p2).max(0.0).sqrt());
                let shade = (0.7 + 0.4 * (vx * l0 + vy * l1 + vz * l2).max(0.0)).min(1.0);
                // A narrow limb darkening and the atmosphere's haze over
                // it, both off the distance from the disc's centre, folded
                // in here rather than painted as passes over the disc.
                let dx = x as f32 + 0.5 - cx;
                let rho = (dx * dx + dy * dy).sqrt() * inv_horizon;
                let edge = ((rho - 0.94) / 0.06).clamp(0.0, 1.0);
                let shade = shade * (1.0 - 0.3 * edge);
                let haze = ((rho - HAZE_FROM) / (1.0 - HAZE_FROM)).clamp(0.0, 1.0);
                (lon * to_x + wf * 0.5 - 0.5, hf * 0.5 - lat * to_y - 0.5, shade * 256.0, haze * haze * haze * HAZE_MAX * 256.0)
            };
            // Exact every SPAN pixels and straight lines between: the
            // mapping bends slowly enough that the error stays under a
            // texel everywhere but the last pixels before the limb and
            // round a pole, where the picture's rows are one colour.
            let rim = (0.8 * horizon_px as f32).powi(2) - dy * dy;
            let mut a = xs;
            let mut at = exact(a);
            while a < xe {
                let off = (a as f32 + 0.5 - cx).abs().min((a as f32 + SPAN as f32 + 0.5 - cx).abs());
                let b = (a + if off * off < rim { SPAN } else { RIM_SPAN }).min(xe);
                let bt = exact(b);
                let mut dfx = bt.0 - at.0;
                if dfx > wf * 0.5 {
                    dfx -= wf;
                } else if dfx < -wf * 0.5 {
                    dfx += wf;
                }
                let n = 1.0 / (b - a) as f32;
                let (sx, sy, ss) = (dfx * n * step as f32, (bt.1 - at.1) * n * step as f32, (bt.2 - at.2) * n * step as f32);
                let sh = (bt.3 - at.3) * n * step as f32;
                // Only the runs out near the rim carry any haze.
                let hazy = at.3 > 0.0 || bt.3 > 0.0;
                let (fx, fy, mut shade, mut haze) = at;
                let mut put = |x: i32, rb: u32, g: u32, shade: f32, haze: f32| {
                    let k = shade as u32;
                    let (mut rb, mut g) = (((rb * k) >> 8) & 0x00ff_00ff, (g * k) >> 8);
                    if hazy {
                        let t = haze as u32;
                        rb = lerp(rb, HAZE_RB, t);
                        g = lerp(g, HAZE_G, t);
                    }
                    let px = rb | (g << 8) | 0xff00_0000;
                    let o = (x - x0) as usize * 4;
                    row[o..o + 4].copy_from_slice(&px.to_le_bytes());
                    if coarse && x + 1 < b {
                        row[o + 4..o + 8].copy_from_slice(&px.to_le_bytes());
                    }
                };
                match view.resolve(fx.min(fx + dfx), fx.max(fx + dfx), at.1.min(bt.1), at.1.max(bt.1)) {
                    Source::Tile(l, ti) => {
                        // The whole run inside one tile: today's bilinear
                        // with no wrap or tile lookup per pixel.
                        let lv = &view.levels[l];
                        let tile = lv.tiles[ti].as_deref().expect("a resolved tile");
                        let (ox, oy) = (((ti % lv.cols) * view.tile) as f32, ((ti / lv.cols) * view.tile) as f32);
                        let ymax = (lv.h - 1) as f32;
                        let (tw, th) = (tile.w, tile.h);
                        let stride = tw * 3;
                        let tex = &tile.rgb[..];
                        let (mut lx, mut wy_) = ((fx + 0.5) * lv.sx - 0.5 - ox, (fy + 0.5) * lv.sy - 0.5);
                        let (dx, dy) = (sx * lv.sx, sy * lv.sy);
                        for x in (a..b).step_by(step) {
                            let ly = wy_.clamp(0.0, ymax) - oy;
                            let (gx, gy) = (lx.max(0.0).floor(), ly.max(0.0).floor());
                            let (wx, wy) = (((lx - gx) * 256.0) as u32, ((ly - gy) * 256.0) as u32);
                            let tx0 = (gx as usize).min(tw - 1);
                            let tx1 = (tx0 + 1).min(tw - 1);
                            let ty0 = (gy as usize).min(th - 1);
                            let ty1 = (ty0 + 1).min(th - 1);
                            let (r0, r1) = (ty0 * stride, ty1 * stride);
                            let texel = |i: usize| u32::from_le_bytes(tex[i..i + 4].try_into().unwrap_or([0; 4]));
                            let (rb, g) = mix4([texel(r0 + tx0 * 3), texel(r0 + tx1 * 3), texel(r1 + tx0 * 3), texel(r1 + tx1 * 3)], wx, wy);
                            put(x, rb, g, shade, haze);
                            lx += dx;
                            wy_ += dy;
                            shade += ss;
                            haze += sh;
                        }
                    }
                    Source::Each(l) => {
                        let (mut fx, mut fy) = (fx, fy);
                        for x in (a..b).step_by(step) {
                            let (rb, g) = view.bilinear(l, fx, fy);
                            put(x, rb, g, shade, haze);
                            fx += sx;
                            fy += sy;
                            shade += ss;
                            haze += sh;
                        }
                    }
                }
                a = b;
                at = bt;
            }
        }
        let pixmap = pool[i].clone();
        pool.truncate(3);
        Some((Bitmap { pixmap, density: 1.0 }, self.finish(view)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture whose texel says where it is, toned by nothing: red the
    /// longitude, green the latitude.
    fn coded(width: usize, height: usize) -> Vec<u8> {
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.extend([(x * 256 / width) as u8, (y * 256 / height) as u8, ((x * 7 + y * 13) % 256) as u8]);
            }
        }
        rgb
    }

    fn cut(rgb: &[u8], w: usize, h: usize, tile: usize) -> Vec<Arc<Tile>> {
        let level = Level::new(w, h, tile);
        (0..level.rows)
            .flat_map(|r| (0..level.cols).map(move |c| (c, r)))
            .map(|(c, r)| {
                let (tw, th) = (tile.min(w - c * tile), tile.min(h - r * tile));
                let mut t = Vec::with_capacity(tw * th * 3);
                for y in r * tile..r * tile + th {
                    t.extend_from_slice(&rgb[(y * w + c * tile) * 3..(y * w + c * tile + tw) * 3]);
                }
                Arc::new(Tile::new(tw, th, t))
            })
            .collect()
    }

    /// A pyramid over `sizes`, coarsest first, cut into `tile` squares,
    /// with the finer levels' tiles decoded when `resident`.
    fn pyramid(sizes: &[(usize, usize)], tile: usize, resident: bool) -> Earth {
        let levels: Vec<Level> = sizes.iter().map(|&(w, h)| Level::new(w, h, tile)).collect();
        let shared = Arc::new(Shared { dir: PathBuf::new(), tile, levels: levels.clone(), state: Mutex::default(), version: AtomicU64::new(0), wake: Box::new(|| {}) });
        if resident {
            let mut st = shared.state.lock().unwrap();
            for (l, &(w, h)) in sizes.iter().enumerate().skip(1) {
                for (i, t) in cut(&coded(w, h), w, h, tile).into_iter().enumerate() {
                    st.tiles.insert((l, i), (t, 0));
                }
            }
        }
        let (w, h) = sizes[0];
        let base = cut(&coded(w, h), w, h, tile);
        Earth { tile, levels, base, shared: Some(shared), spawn: None }
    }

    const ROT: Rot = Rot { sin_lat: 0.3, cos_lat: 0.953_939_2, sin_lon: 0.5, cos_lon: 0.866_025_4 };

    fn draw(e: &Earth, r: f64) -> (Pixmap, Stats) {
        let mut pool = Vec::new();
        let (b, stats) = e.sample(&mut pool, false, (100, 50, 900, 850), (500.0, 450.0), r, 396.0, fs_media::radio::model::view_distance(1.0), &ROT).unwrap();
        ((*b.pixmap).clone(), stats)
    }

    fn worst(a: &Pixmap, b: &Pixmap) -> u8 {
        a.data_as_u8_slice().iter().zip(b.data_as_u8_slice()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    #[test]
    fn tiles_draw_what_the_whole_picture_draws() {
        // 2*pi*547 asks for 3437 texels: the 4096 level, cut into 512 px
        // tiles and every one decoded, against the same 4096 picture whole.
        let tiled = pyramid(&[(1024, 512), (4096, 2048)], 512, true);
        let whole = Earth::single(4096, 2048, coded(4096, 2048));
        let (a, stats) = draw(&tiled, 547.0);
        let (b, _) = draw(&whole, 547.0);
        assert_eq!((stats.level, stats.missing), (4096, 0));
        assert!(stats.tiles > 1, "{stats:?}");
        assert!(worst(&a, &b) <= 1, "tiles and the whole picture differ by {}", worst(&a, &b));
    }

    #[test]
    fn a_missing_level_draws_the_one_under_it_and_asks_for_its_tiles() {
        let empty = pyramid(&[(1024, 512), (4096, 2048)], 512, false);
        let coarse = Earth::single(1024, 512, coded(1024, 512));
        let (a, stats) = draw(&empty, 547.0);
        let (b, _) = draw(&coarse, 547.0);
        assert_eq!(stats.level, 4096);
        assert!(stats.missing > 4 && stats.missing == stats.tiles, "{stats:?}");
        assert!(worst(&a, &b) <= 1, "the fallback differs from the coarse level by {}", worst(&a, &b));
        let st = empty.shared.as_ref().unwrap().state.lock().unwrap();
        assert_eq!(st.wanted.len(), stats.missing);
        assert!(st.wanted.iter().all(|&(l, _)| l == 1));
    }

    #[test]
    fn the_level_follows_the_zoom() {
        let e = pyramid(&[(2700, 1350), (5400, 2700), (10800, 5400), (21600, 10800)], 512, false);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 400.0)].w, 2700);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 650.0)].w, 5400);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 10000.0)].w, 21600);
    }

    #[test]
    fn sampling_tiles_costs_what_the_whole_picture_does() {
        let tiled = pyramid(&[(1024, 512), (4096, 2048)], 512, true);
        let whole = Earth::single(4096, 2048, coded(4096, 2048));
        for (name, e) in [("whole", &whole), ("tiled", &tiled), ("whole", &whole), ("tiled", &tiled)] {
            let mut pool = Vec::new();
            let t = Instant::now();
            for _ in 0..20 {
                e.sample(&mut pool, false, (100, 50, 900, 850), (500.0, 450.0), 547.0, 396.0, fs_media::radio::model::view_distance(1.0), &ROT).unwrap();
            }
            eprintln!("{name}: {} us a frame", t.elapsed().as_micros() / 20);
        }
    }

    #[test]
    fn fast_atan2_holds_a_fraction_of_a_texel() {
        let mut worst = 0.0f32;
        for i in 0..3600 {
            let a = (i as f32 / 10.0).to_radians() - std::f32::consts::PI;
            for m in [0.3f32, 1.0, 7.0] {
                let (y, x) = (a.sin() * m, a.cos() * m);
                let e = (fast_atan2(y, x) - y.atan2(x)).abs();
                worst = worst.max(e.min((e - std::f32::consts::TAU).abs()));
            }
        }
        assert!(worst < 1.5e-4, "{worst}");
    }
}
