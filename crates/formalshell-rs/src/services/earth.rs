//! The satellite picture the radio atlas's globe wraps itself in, and the
//! cache its frames read it through. The picture is a pyramid of equirectangular levels cut into
//! square tiles (`FS_EARTH_TILES`, packaging/earth-tiles.py): the coarsest level
//! is decoded whole on open, every finer tile only once a frame needs it,
//! on a pool thread, into a cache with a byte cap. A frame reads each run
//! of pixels from the finest level that has its tiles ([`View`]), so a tile
//! still decoding shows the level under it rather than a hole. Without the
//! tiles the single picture `FS_EARTH_IMAGE` names is a pyramid of one
//! level. Dropping the [`Earth`] drops every tile past the coarsest level.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<u8>,
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
    pub fn level_for(&self, texels: f64) -> usize {
        self.levels.iter().position(|l| l.w as f64 >= texels).unwrap_or(self.levels.len() - 1)
    }

    /// One frame's hold on the pyramid up to level `top`.
    pub fn view(&self, top: usize) -> View {
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
    pub fn finish(&self, view: View) -> Stats {
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

pub struct LevelView {
    pub w: usize,
    pub h: usize,
    pub cols: usize,
    /// This level's texels per texel of the frame's level.
    pub sx: f32,
    pub sy: f32,
    pub tiles: Vec<Option<Arc<Tile>>>,
    touched: Vec<bool>,
}

/// One frame's hold on the pyramid, up to the level it draws at.
pub struct View {
    pub tile: usize,
    pub top: usize,
    pub levels: Vec<LevelView>,
    /// The frame level's tiles it found missing, in the order it met them.
    missing: Vec<usize>,
    seen: Vec<bool>,
}

/// Where a run of pixels reads from.
pub enum Source {
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
    pub fn resolve(&mut self, x0: f32, x1: f32, y0: f32, y1: f32) -> Source {
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
    pub fn bilinear(&mut self, from: usize, fx: f32, fy: f32) -> (u32, u32) {
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
pub fn lerp(a: u32, b: u32, t: u32) -> u32 {
    ((a * (256 - t) + b * t) >> 8) & 0x00ff_00ff
}

/// Four texels mixed by the weights across (`wx`) and down (`wy`), out of
/// 256: red and blue side by side in one word, green on its own.
#[inline(always)]
pub fn mix4([p00, p01, p10, p11]: [u32; 4], wx: u32, wy: u32) -> (u32, u32) {
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
        let y = fs_theme::color::luma601(r, g, b);
        for (out, c) in px.iter_mut().zip([r, g, b]) {
            *out = (y + (c - y) * TONE_SATURATION).round().clamp(0.0, 255.0) as u8;
        }
    }
}

/// Pyramids built in memory, for the sampler's tests too.
#[cfg(test)]
pub(crate) mod pictures {
    use super::*;

    /// A picture whose texel says where it is, toned by nothing: red the
    /// longitude, green the latitude.
    pub(crate) fn coded(width: usize, height: usize) -> Vec<u8> {
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
    pub(crate) fn pyramid(sizes: &[(usize, usize)], tile: usize, resident: bool) -> Earth {
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

    /// The same pyramid written under `dir` as `packaging/earth-tiles.py`
    /// lays it out, loaded the way the atlas loads it, `spawn` running the
    /// finer tiles' decodes.
    pub(crate) fn on_disk(dir: &Path, sizes: &[(usize, usize)], tile: usize, spawn: Spawn) -> Earth {
        let mut manifest = format!("tile {tile}\n");
        for &(w, h) in sizes {
            manifest += &format!("level {w} {h}\n");
            let cols = Level::new(w, h, tile).cols;
            std::fs::create_dir_all(dir.join(w.to_string())).unwrap();
            for (i, t) in cut(&coded(w, h), w, h, tile).iter().enumerate() {
                let picture = image::RgbImage::from_raw(t.w as u32, t.h as u32, t.rgb[..t.w * t.h * 3].to_vec()).unwrap();
                picture.save(dir.join(w.to_string()).join(format!("{}_{}.jpg", i % cols, i / cols))).unwrap();
            }
        }
        std::fs::write(dir.join("pyramid.txt"), manifest).unwrap();
        Earth::tiled(dir, spawn, Box::new(|| {})).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::pictures::pyramid;

    #[test]
    fn the_level_follows_the_zoom() {
        let e = pyramid(&[(2700, 1350), (5400, 2700), (10800, 5400), (21600, 10800)], 512, false);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 400.0)].w, 2700);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 650.0)].w, 5400);
        assert_eq!(e.levels[e.level_for(std::f64::consts::TAU * 10000.0)].w, 21600);
    }
}
