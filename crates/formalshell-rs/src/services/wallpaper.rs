//! The desktop's picture: the wallpaper decoded and
//! cover-cropped to each output's size on the pool, handed to the UI thread
//! as ready pixels, through the retro dither pass when
//! `wallpaper.dither` asks for it and under the grid when `wallpaper.grid`
//! does. Each output's backdrop crossfades between two of these.
//!
//! What the desktop shows is a source, [`source`]'s one answer: the
//! picture's path, or `color:#rrggbb` where `wallpaper.color` or the picker
//! made it a flat field. Every reader of the wallpaper (the backdrops, the
//! lock, the band's paint, matugen, the greeter) takes the source.

use std::path::{Path, PathBuf};
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

const FLAT: &str = "color:";

/// What the desktop shows: the picker's last colour (`wallpaperColor` in
/// state.json a hex), the picture when it last chose one (`false`), and with
/// no choice `wallpaper.color` or the preset's colour over the picture.
pub fn source(color: Option<&str>, picked: &serde_json::Value, picture: &str) -> String {
    use serde_json::Value;
    let color = match picked {
        Value::String(hex) => Some(hex.as_str()),
        Value::Bool(false) => None,
        _ => color,
    };
    color.map_or_else(|| picture.to_owned(), flat_source)
}

/// The source for a flat field of `hex`.
pub fn flat_source(hex: &str) -> String {
    format!("{FLAT}{hex}")
}

/// A flat source's `#rrggbb`, none for a picture.
pub fn flat_hex(source: &str) -> Option<&str> {
    source.strip_prefix(FLAT)
}

/// A flat source's colour.
pub fn flat(source: &str) -> Option<[u8; 3]> {
    let hex = flat_hex(source)?.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    (hex.len() == 6).then_some([byte(0)?, byte(2)?, byte(4)?])
}

/// The source decoded and cover-cropped to `width` by `height`, or the flat
/// field filling it.
pub fn image(source: &str, width: u32, height: u32) -> Option<image::RgbaImage> {
    if let Some([r, g, b]) = flat(source) {
        return Some(image::RgbaImage::from_pixel(width.max(1), height.max(1), image::Rgba([r, g, b, 255])));
    }
    let img = image::ImageReader::open(source).ok()?.with_guessed_format().ok()?.decode().ok()?.to_rgba8();
    let (cx, cy, cw, ch) = cover_rect((img.width(), img.height()), (width, height));
    let crop = imageops::crop_imm(&img, cx, cy, cw, ch).to_image();
    Some(imageops::resize(&crop, width, height, FilterType::Triangle))
}

/// The centred box of an `iw` by `ih` picture that scaled covers `width` by
/// `height`: x, y, width, height.
fn cover_rect((iw, ih): (u32, u32), (width, height): (u32, u32)) -> (u32, u32, u32, u32) {
    let scale = (width as f64 / iw as f64).max(height as f64 / ih as f64);
    let (cw, ch) = ((width as f64 / scale).round().max(1.0) as u32, (height as f64 / scale).round().max(1.0) as u32);
    ((iw.saturating_sub(cw)) / 2, (ih.saturating_sub(ch)) / 2, cw.min(iw), ch.min(ih))
}

/// Where `formalshell-depth` keeps a picture's depth map, none for a flat
/// field.
pub fn depth_path(source: &str) -> Option<PathBuf> {
    if source.is_empty() || flat_hex(source).is_some() {
        return None;
    }
    let path = Path::new(source);
    let mut name = path.file_name()?.to_os_string();
    name.push(".png");
    Some(path.parent()?.join(".depth").join(name))
}

/// `<prefix>.depthThreshold` and `<prefix>.depthFeather`, on the map's
/// 0..100 (100 nearest): what passes in front of the clock. No threshold
/// cuts just past the map's far plane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DepthCut {
    pub threshold: Option<f64>,
    pub feather: f64,
}

impl DepthCut {
    pub const THRESHOLD: f64 = 50.0;
    pub const FEATHER: f64 = 8.0;

    /// None while `<prefix>.depth` is off. `threshold` is the default
    /// cut, None for the far plane's.
    pub fn read(config: &super::config::State, prefix: &str, threshold: Option<f64>) -> Option<Self> {
        if config.bool(&format!("{prefix}.depth")) == Some(false) {
            return None;
        }
        let num = |key: &str, hi: f64| config.f64(&format!("{prefix}.{key}")).filter(|n| n.is_finite()).map(|n| n.clamp(0.0, hi));
        Some(Self { threshold: num("depthThreshold", 100.0).or(threshold), feather: num("depthFeather", 50.0).unwrap_or(Self::FEATHER) })
    }

    /// A map value's coverage: a smoothstep across `threshold +- feather/2`.
    pub fn alpha(&self, value: u8) -> u8 {
        let threshold = self.threshold.unwrap_or(Self::THRESHOLD);
        let x = f64::from(value) * 100.0 / 255.0;
        let half = self.feather / 2.0;
        let t = if half <= 0.0 {
            if x >= threshold { 1.0 } else { 0.0 }
        } else {
            ((x - (threshold - half)) / self.feather).clamp(0.0, 1.0)
        };
        (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8
    }
}

/// The depth map for `source` cover-cropped as [`image`] crops the picture
/// to `width` by `height`, as one coverage byte per pixel. None for a flat
/// field, and for a map that is missing or older than its picture.
pub fn depth_mask(source: &str, width: u32, height: u32, cut: DepthCut) -> Option<Vec<u8>> {
    let map = depth_path(source)?;
    let made = std::fs::metadata(&map).ok()?.modified().ok()?;
    if made < std::fs::metadata(source).ok()?.modified().ok()? {
        return None;
    }
    let img = image::ImageReader::open(&map).ok()?.with_guessed_format().ok()?.decode().ok()?.to_luma8();
    let (cx, cy, cw, ch) = cover_rect((img.width(), img.height()), (width, height));
    let crop = imageops::crop_imm(&img, cx, cy, cw, ch).to_image();
    let fitted = imageops::resize(&crop, width, height, FilterType::Triangle);
    let map = fitted.into_raw();
    // The far plane's cut eases in over a narrow feather starting at its
    // band's edge, so none of the sky comes forward.
    let cut = match (cut.threshold, far_plane(&map)) {
        (None, Some(edge)) => {
            let feather = cut.feather.min(FAR_FEATHER);
            DepthCut { threshold: Some(edge + feather / 2.0), feather }
        }
        _ => cut,
    };
    Some(map.into_iter().map(|v| cut.alpha(v)).collect())
}

/// The near edge of the map's far plane, on 0..100: the band holding its
/// farthest values, when that band is a sky or a backdrop's worth of the
/// picture. Anything nearer, a mountain range included, is subject. None
/// for a map with no such band, which is cut at [`DepthCut::THRESHOLD`].
pub fn far_plane(map: &[u8]) -> Option<f64> {
    let mut hist = [0usize; 256];
    for &v in map {
        hist[usize::from(v)] += 1;
    }
    let far = hist.iter().position(|&n| n * 1000 >= map.len())?;
    let band: usize = hist[far..=(far + FAR_BAND).min(255)].iter().sum();
    (band * 100 >= map.len() * FAR_SHARE).then(|| (far + FAR_BAND) as f64 * 100.0 / 255.0)
}

/// How wide the far plane's band is, in map values, and the share of the
/// picture in percent it has to hold to count as one.
const FAR_BAND: usize = 4;
const FAR_SHARE: usize = 15;
/// The far plane's cut eases over no more than this, on 0..100.
const FAR_FEATHER: f64 = 3.0;

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
/// since the greeter's mirrored copy has no extension. A flat field skips
/// the dither, which would only hand it back.
pub fn render(path: &str, width: u32, height: u32, colors: Option<usize>, grid: Option<Grid>) -> Option<Picture> {
    let mut out = image(path, width, height)?;
    if let Some(n) = colors.filter(|_| flat(path).is_none()) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_preset_colour_covers_the_picture_until_the_picker_says_otherwise() {
        assert_eq!(source(Some("#402484"), &json!(null), "/w.png"), "color:#402484");
        assert_eq!(source(None, &json!(null), "/w.png"), "/w.png");
        assert_eq!(source(Some("#402484"), &json!(false), "/w.png"), "/w.png");
        assert_eq!(source(None, &json!("#ff8000"), "/w.png"), "color:#ff8000");
        assert_eq!(source(Some("#402484"), &json!("#a1c8bc"), ""), "color:#a1c8bc");
        assert_eq!(source(Some("#402484"), &json!(3), "/w.png"), "color:#402484");
    }

    #[test]
    fn a_flat_source_renders_one_colour_under_its_grid() {
        assert_eq!(flat("color:#402484"), Some([0x40, 0x24, 0x84]));
        assert_eq!(flat("/w/color.png"), None);
        assert_eq!(flat("color:#40248"), None);
        let p = render("color:#402484", 8, 4, Some(6), None).unwrap();
        assert_eq!(p.path, "color:#402484");
        assert!(p.bgra.chunks_exact(4).all(|px| px == [0x84, 0x24, 0x40, 255]));
        assert_eq!(crate::shards::mean(&p.bgra).map(|c| c.to_u8()), Some([0x40, 0x24, 0x84, 255]));
    }

    #[test]
    fn a_picture_keeps_its_depth_map_beside_it() {
        assert_eq!(depth_path("/w/dark/sea.jpg"), Some(PathBuf::from("/w/dark/.depth/sea.jpg.png")));
        assert_eq!(depth_path("color:#402484"), None);
        assert_eq!(depth_path(""), None);
    }

    #[test]
    fn the_cut_eases_across_its_feather() {
        let cut = DepthCut { threshold: Some(50.0), feather: 20.0 };
        let at = |pct: f64| cut.alpha((pct * 255.0 / 100.0).round() as u8);
        assert_eq!(at(0.0), 0);
        assert_eq!(at(39.0), 0);
        assert!((120..=135).contains(&at(50.0)), "{}", at(50.0));
        assert_eq!(at(61.0), 255);
        assert!(at(42.0) < at(45.0) && at(45.0) < at(55.0) && at(55.0) < at(58.0));
        let hard = DepthCut { threshold: Some(50.0), feather: 0.0 };
        assert_eq!((hard.alpha(126), hard.alpha(128)), (0, 255));
    }

    /// A sky at 0 with a far range at 7..36 and dunes past 117, the shape of
    /// a desert under mountains: the range is subject, the sky is not. A
    /// map with no far band keeps the fixed cut.
    #[test]
    fn the_far_plane_leaves_the_mountains_in_front() {
        let mut map = vec![0u8; 440];
        map.extend((7..37).flat_map(|v| [v; 5]));
        map.extend((117..=255).flat_map(|v| [v; 3]));
        let edge = far_plane(&map).unwrap();
        assert!(edge < 7.0 * 100.0 / 255.0, "{edge}");
        let ramp: Vec<u8> = (0..=255).collect();
        assert_eq!(far_plane(&ramp), None);
    }

    /// A map at a different size from its picture lands on the same pixels:
    /// both carry their column across the red channel and their row across
    /// the green, so the crops agree wherever they are read.
    #[test]
    fn the_map_is_cropped_as_the_picture_is() {
        let dir = tempfile::tempdir().unwrap();
        let picture = dir.path().join("wide.png");
        let ramp = |w: u32, h: u32| image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 0, 255]));
        ramp(600, 200).save(&picture).unwrap();
        std::fs::create_dir(dir.path().join(".depth")).unwrap();
        let map = dir.path().join(".depth/wide.png.png");
        let small = ramp(300, 100);
        image::GrayImage::from_fn(300, 100, |x, y| image::Luma([small.get_pixel(x, y).0[0]])).save(&map).unwrap();
        let identity = DepthCut { threshold: Some(50.0), feather: 100.0 };
        let (w, h) = (160, 120);
        let shown = image(picture.to_str().unwrap(), w, h).unwrap();
        let mask = depth_mask(picture.to_str().unwrap(), w, h, identity).unwrap();
        for (x, y) in [(0, 0), (40, 60), (80, 30), (159, 119)] {
            let want = identity.alpha(shown.get_pixel(x, y).0[0]);
            let got = mask[(y * w + x) as usize];
            assert!(want.abs_diff(got) <= 6, "({x}, {y}): picture says {want}, map says {got}");
        }
        let past = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options().write(true).open(&map).unwrap().set_modified(past).unwrap();
        assert!(depth_mask(picture.to_str().unwrap(), w, h, identity).is_none(), "a map older than its picture is stale");
    }
}
