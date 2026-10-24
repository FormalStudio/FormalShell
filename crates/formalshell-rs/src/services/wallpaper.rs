//! The desktop's picture: the wallpaper decoded and
//! cover-cropped to each output's size on the pool, handed to the UI thread
//! as ready pixels, through the retro dither pass when
//! `wallpaper.dither` asks for it and under the grid when `wallpaper.grid`
//! does. Each output's backdrop crossfades between two of these.

use std::sync::{Arc, OnceLock};

use fs_chrome::grid::Grid;
use image::imageops::{self, FilterType};

use crate::runtime::Ctx;
use crate::store;

/// One output's picture, rows of wl_shm ARGB8888 (BGRA in memory).
pub struct Picture {
    pub path: String,
    /// The palette size the pass quantized to, none for the plain image.
    pub dither: Option<usize>,
    /// The grid laid over it, in RGBA order and the picture's own pixels.
    pub grid: Option<Grid>,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The newest picture for each output size asked for.
#[derive(Default)]
pub struct State {
    pictures: Vec<Arc<Picture>>,
}

/// The picture for one size, none when the wallpaper is unset or failed.
pub struct Diff {
    pub size: (u32, u32),
    pub picture: Option<Arc<Picture>>,
}

/// More sizes than any desk carries outputs; the oldest goes first.
const KEPT: usize = 4;

impl State {
    pub fn apply(&mut self, Diff { size, picture }: Diff) -> bool {
        let at = self.pictures.iter().position(|p| (p.width, p.height) == size);
        let old = at.map(|i| self.pictures.remove(i));
        let same = match (&old, &picture) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if let Some(p) = picture {
            self.pictures.retain(|q| q.path == p.path);
            self.pictures.push(p);
            if self.pictures.len() > KEPT {
                self.pictures.remove(0);
            }
        }
        !same
    }

    /// The picture made for an output `width` by `height`.
    pub fn sized(&self, width: u32, height: u32) -> Option<&Arc<Picture>> {
        self.pictures.iter().rev().find(|p| (p.width, p.height) == (width, height))
    }

    /// The picture at every size an output asked for.
    pub fn all(&self) -> impl Iterator<Item = &Arc<Picture>> {
        self.pictures.iter()
    }

    /// The latest picture of any size, for a surface that scales its own.
    pub fn latest(&self) -> Option<&Arc<Picture>> {
        self.pictures.last()
    }
}

type Request = (String, u32, u32, Option<usize>, Option<Grid>);

static REQUESTS: OnceLock<async_channel::Sender<Request>> = OnceLock::new();

/// The wallpaper at `path` for an output `width` by `height`, dithered to
/// `dither` colours when set, then under `grid`.
pub fn show(path: &str, width: u32, height: u32, dither: Option<usize>, grid: Option<Grid>) {
    if let Some(tx) = REQUESTS.get() {
        let _ = tx.try_send((path.to_owned(), width, height, dither, grid));
    }
}

/// The table's `backdrop.grid` for a buffer at `scale` times the logical
/// size, in RGBA order.
pub fn grid(theme: &fs_theme::theme::Theme, scale: f64) -> Grid {
    let m = |key| theme.metric("backdrop.grid", key);
    let color = theme.box_style("backdrop.grid", None).fill.to_u8();
    Grid { pitch: m("pitch"), width: m("width"), fade: m("fade"), color }.scaled(scale)
}

/// The retro dither: the output sampled on a `chunk` grid at
/// each cell's centre, a palette of the image's own colours, and every cell
/// painted as one hard-edged square of its quantized entry.
fn dither(img: &mut image::RgbaImage, colors: usize) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let chunk = ((w.max(h) as f64 / 480.0).round() as usize).max(2);
    let (gw, gh) = (w.div_ceil(chunk), h.div_ceil(chunk));
    let mut cells = Vec::with_capacity(gw * gh * 3);
    for gy in 0..gh {
        let cy = (gy * chunk + chunk / 2).min(h - 1) as u32;
        for gx in 0..gw {
            let cx = (gx * chunk + chunk / 2).min(w - 1) as u32;
            let p = img.get_pixel(cx, cy).0;
            cells.extend_from_slice(&p[..3]);
        }
    }
    let pal = fs_chrome::dither::palette(&cells, gw * gh, colors);
    let idx = fs_chrome::dither::quantize(&cells, gw * gh, gw, &pal);
    if pal.is_empty() {
        return;
    }
    for (y, row) in img.rows_mut().enumerate() {
        let gy = y / chunk;
        for (x, px) in row.enumerate() {
            let e = idx[gy * gw + x / chunk] as usize * 3;
            px.0 = [pal[e], pal[e + 1], pal[e + 2], 255];
        }
    }
}

/// Aspect-crop: scaled to cover, centred. The format comes from the bytes,
/// since the greeter's mirrored copy has no extension.
pub fn render(path: &str, width: u32, height: u32, colors: Option<usize>, grid: Option<Grid>) -> Option<Picture> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8();
    let (iw, ih) = (img.width() as f64, img.height() as f64);
    let scale = (width as f64 / iw).max(height as f64 / ih);
    let (cw, ch) = ((width as f64 / scale).round().max(1.0) as u32, (height as f64 / scale).round().max(1.0) as u32);
    let (cx, cy) = ((img.width().saturating_sub(cw)) / 2, (img.height().saturating_sub(ch)) / 2);
    let crop = imageops::crop_imm(&img, cx, cy, cw.min(img.width()), ch.min(img.height())).to_image();
    let mut out = imageops::resize(&crop, width, height, FilterType::Triangle);
    if let Some(n) = colors {
        dither(&mut out, n);
    }
    if let Some(g) = grid {
        g.paint(&mut out, width as usize, height as usize);
    }
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for px in out.pixels() {
        let [r, g, b, _] = px.0;
        bgra.extend_from_slice(&[b, g, r, 255]);
    }
    Some(Picture { path: path.to_owned(), dither: colors, grid, width, height, bgra })
}

/// Requests queued behind one another collapse to the newest per size, so
/// each output's backdrop gets its own picture and none waits on a stale one.
pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = REQUESTS.set(tx);
    let mut last: Vec<Request> = Vec::new();
    while let Ok(first) = rx.recv().await {
        let mut batch: Vec<Request> = vec![first];
        while let Ok(newer) = rx.try_recv() {
            batch.retain(|r| (r.1, r.2) != (newer.1, newer.2));
            batch.push(newer);
        }
        for request in batch {
            if last.contains(&request) || request.1 == 0 || request.2 == 0 {
                continue;
            }
            last.retain(|r| (r.1, r.2) != (request.1, request.2));
            last.push(request.clone());
            let (path, w, h, colors, grid) = request;
            let picture = if path.is_empty() {
                None
            } else {
                ctx.pool().run(move || render(&path, w, h, colors, grid)).await.flatten().map(Arc::new)
            };
            ctx.publish(store::Diff::Wallpaper(Diff { size: (w, h), picture }));
        }
    }
}
