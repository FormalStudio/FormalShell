//! The wallpaper picker's scan and its tiles' pictures: a directory listed
//! (itself and its Dark/Light subdirectories) by a `find` child and read as
//! pairs and singles, the built-ins out of `FS_WALLPAPERS`'s catalog, every
//! picture prerendered to a 640x360 `wide` crop in the shared thumbnail
//! cache, and the tiles decoded from that cache on the pool. A directory is
//! scanned when the route is entered, and once when `picker.directory` is
//! first read so the grid opens warm.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::LazyLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_menu::providers::{self, Item, ItemKind};
use fs_menu::thumbnails::{self, Mode};

use super::clipboard::thumb_dir;
use super::proc;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

/// The `wide` thumbnail's width; its height is 9/16 of it.
const WIDE: u32 = 640;

/// A pair's two halves in one tile key.
const HALVES: char = '\u{1}';

#[derive(Default)]
pub struct State {
    /// The directory the listing belongs to.
    pub dir: String,
    pub scanned: Vec<String>,
    /// `scanned` read as pairs and singles.
    pub items: Vec<Item>,
    /// The built-ins' directory, empty with none.
    pub included_dir: String,
    pub included: Vec<Item>,
    /// Pictures whose `wide` thumbnail is in the cache.
    pub cached: HashSet<String>,
    /// Each tile's picture by [`tile_key`].
    pub thumbs: HashMap<String, Option<Bitmap>>,
    /// What the pictures were made for.
    pub thumb_px: Px,
}

/// A tile's picture size in logical pixels, its corner radius, and the
/// surface's scale.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Px {
    pub w: f64,
    pub h: f64,
    pub radius: f64,
    pub scale: f64,
}

pub struct Scan {
    pub dir: String,
    pub list: Vec<String>,
    pub items: Vec<Item>,
    pub included_dir: String,
    pub included: Vec<Item>,
}

pub enum Diff {
    Scanned(Scan),
    Cached(Vec<String>),
    Thumbs(Px, Vec<(String, Option<Bitmap>)>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Scanned(scan) => {
                if self.dir == scan.dir && self.scanned == scan.list && self.included == scan.included {
                    return false;
                }
                self.dir = scan.dir;
                self.scanned = scan.list;
                self.items = scan.items;
                self.included_dir = scan.included_dir;
                self.included = scan.included;
            }
            Diff::Cached(list) => self.cached.extend(list),
            Diff::Thumbs(px, list) => {
                if px != self.thumb_px {
                    self.thumbs.clear();
                    self.thumb_px = px;
                }
                self.thumbs.extend(list);
            }
        }
        true
    }

    /// Every file of the tile is in the cache.
    pub fn warm(&self, item: &Item) -> bool {
        let files = item.files();
        !files.is_empty() && files.iter().all(|f| self.cached.contains(*f))
    }
}

/// The key a tile's picture is kept under: a pair's two halves, a single's
/// file, the Sky's source now, a Style preview's slot.
pub fn tile_key(item: &Item, sky: &str) -> String {
    match &item.kind {
        ItemKind::Pair { light, dark } => format!("{light}{HALVES}{dark}"),
        ItemKind::Single { path, .. } | ItemKind::Mode { path, .. } => path.clone(),
        ItemKind::Sky => sky.to_owned(),
    }
}

/// What `source` (the desktop's) is called: the title of the tile holding
/// it, else one read off its file name; the Sky and a flat field by name.
pub fn title(state: &State, source: &str) -> String {
    if super::wallpaper::sky(source).is_some() {
        return "Sky".into();
    }
    if super::wallpaper::generated(source) {
        return "Colour".into();
    }
    match state.items.iter().chain(&state.included).find(|i| i.files().contains(&source)) {
        Some(item) => item.title.clone(),
        None if source.is_empty() => String::new(),
        None => providers::title_of(providers::image_basename(source)),
    }
}

/// The directory Your wallpapers lists: `picker.directory`, else
/// `Wallpapers` in the user's pictures directory.
pub fn directory(configured: &str) -> String {
    if !configured.is_empty() {
        return configured.to_owned();
    }
    let home = super::menu::home();
    let dirs = std::fs::read_to_string(super::menu::xdg_dir("XDG_CONFIG_HOME", ".config").join("user-dirs.dirs")).unwrap_or_default();
    let line = dirs.lines().find(|l| l.trim_start().starts_with("XDG_PICTURES_DIR=")).unwrap_or("");
    let pictures = providers::user_dirs(line, &home).into_iter().next().unwrap_or_else(|| format!("{}/Pictures", home.trim_end_matches('/')));
    format!("{pictures}/Wallpapers")
}

pub enum Cmd {
    Scan(String),
    /// `picker.directory` as read: scanned and warmed once per value.
    Boot(String),
    /// Decode these tiles' pictures (by [`tile_key`]) at `Px`.
    Thumbs(Vec<String>, Px),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

pub async fn run(ctx: Ctx) {
    let mut booted: Option<String> = None;
    while let Ok(cmd) = CHANNEL.1.recv().await {
        let cmd = match cmd {
            Cmd::Boot(dir) if booted.as_ref() == Some(&dir) => continue,
            Cmd::Boot(dir) => {
                booted = Some(dir.clone());
                Cmd::Scan(directory(&dir))
            }
            c => c,
        };
        match cmd {
            Cmd::Boot(_) => {}
            Cmd::Scan(dir) => {
                let list: Vec<String> = if dir.is_empty() {
                    Vec::new()
                } else {
                    let done = proc::capture(&providers::picker_scan_command(&dir), Duration::from_secs(30)).await;
                    done.stdout.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
                };
                let (d, l) = (dir.clone(), list.clone());
                let Some(scan) = ctx.pool().run(move || read(d, l)).await else { continue };
                let files: Vec<String> = scan.items.iter().chain(&scan.included).flat_map(|i| i.files()).map(str::to_owned).collect();
                ctx.publish(store::Diff::Picker(Diff::Scanned(scan)));
                let c = ctx.clone();
                ctx.spawn(async move {
                    let ready = warm(&files).await;
                    c.publish(store::Diff::Picker(Diff::Cached(ready)));
                });
            }
            Cmd::Thumbs(keys, px) => {
                let c = ctx.clone();
                ctx.spawn(async move {
                    if let Some(list) = c.pool().run(move || decode(&keys, px)).await {
                        c.publish(store::Diff::Picker(Diff::Thumbs(px, list)));
                    }
                });
            }
        }
    }
}

/// The listing read as pairs and singles, and the built-ins.
fn read(dir: String, list: Vec<String>) -> Scan {
    let items = providers::items(&list, &dir, identical);
    let included_dir = std::env::var("FS_WALLPAPERS").unwrap_or_default();
    let included = match std::fs::read_to_string(format!("{}/catalog.json", included_dir.trim_end_matches('/'))) {
        Ok(json) if !included_dir.is_empty() => providers::catalog(&json, &included_dir, |p| std::path::Path::new(p).is_file()),
        _ => Vec::new(),
    };
    Scan { dir, list, items, included_dir, included }
}

/// The two files hold the same bytes: their lengths, then their contents.
fn identical(a: &str, b: &str) -> bool {
    let len = |p: &str| std::fs::metadata(p).map(|m| m.len()).ok();
    if len(a).is_none() || len(a) != len(b) {
        return false;
    }
    let (Ok(mut fa), Ok(mut fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let Ok(n) = fa.read(&mut ba) else { return false };
        if n == 0 {
            return true;
        }
        if fb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

/// ThumbnailService.warm in `wide` mode: the sources whose thumbnail is
/// now in the cache, a hit or freshly drawn.
async fn warm(paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return Vec::new();
    }
    let dir = thumb_dir();
    let size = WIDE.to_string();
    let mut argv = proc::argv(&["sh", "-c", &thumbnails::warm_script(4), "sh", &dir, &size, Mode::Wide.as_str()]);
    for p in paths {
        argv.push(p.clone());
        argv.push(thumbnails::cache_path(&dir, p, WIDE, Mode::Wide));
    }
    let done = proc::capture(&argv, Duration::from_secs(300)).await;
    done.stdout.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// Each tile as a cover crop at `px`, from its files' cached thumbnails
/// when there are some; a pair's light half above the diagonal from the
/// bottom-left corner to the top-right and its dark half below, every
/// corner cut round. The sky's tile is drawn.
fn decode(keys: &[String], px: Px) -> Vec<(String, Option<Bitmap>)> {
    let dir = thumb_dir();
    // Whole device pixels inside the box, never one past its edge.
    let fit = |logical: f64| (logical * px.scale).floor().max(1.0) as u32;
    let (w, h) = (fit(px.w), fit(px.h));
    let radius = px.radius * px.scale;
    let crop = |p: &str| -> Option<image::RgbaImage> {
        let img = if super::wallpaper::generated(p) {
            super::wallpaper::image(p, w, h).map(image::DynamicImage::ImageRgba8)
        } else {
            image::open(thumbnails::cache_path(&dir, p, WIDE, Mode::Wide)).or_else(|_| image::open(p)).ok()
        };
        img.map(|i| i.resize_to_fill(w, h, image::imageops::FilterType::Triangle).into_rgba8())
    };
    keys.iter()
        .map(|key| {
            let picture = match key.split_once(HALVES) {
                Some((light, dark)) => crop(light).zip(crop(dark)).map(|(l, d)| diagonal(l, &d)),
                None => crop(key),
            };
            let bitmap = picture.map(|mut img| {
                round_corners(&mut img, radius);
                Bitmap::from_rgba(img.width() as u16, img.height() as u16, img.into_raw()).at(px.scale)
            });
            (key.clone(), bitmap)
        })
        .collect()
}

/// `light` above the line from the bottom-left corner to the top-right,
/// `dark` below it, blended across the pixel the line runs through.
fn diagonal(mut light: image::RgbaImage, dark: &image::RgbaImage) -> image::RgbaImage {
    let (w, h) = (f64::from(light.width()), f64::from(light.height()));
    let norm = w.hypot(h);
    for (x, y, p) in light.enumerate_pixels_mut() {
        let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        // Distance above the line, in pixels.
        let above = (w * h - cx * h - cy * w) / norm;
        let t = (0.5 - above).clamp(0.0, 1.0);
        if t > 0.0 {
            let q = dark.get_pixel(x, y);
            for c in 0..4 {
                p[c] = (f64::from(p[c]) * (1.0 - t) + f64::from(q[c]) * t).round() as u8;
            }
        }
    }
    light
}

/// Each corner cut to a quarter circle of `radius` device pixels, the edge
/// pixels covered by the share of them inside it.
fn round_corners(img: &mut image::RgbaImage, radius: f64) {
    let (w, h) = (img.width(), img.height());
    let r = radius.min(f64::from(w.min(h)) / 2.0);
    if r <= 0.0 {
        return;
    }
    let n = r.ceil() as u32;
    for y in 0..n.min(h) {
        for x in 0..n.min(w) {
            let (dx, dy) = (r - (f64::from(x) + 0.5), r - (f64::from(y) + 0.5));
            if dx <= 0.0 || dy <= 0.0 {
                continue;
            }
            let cover = (r - dx.hypot(dy) + 0.5).clamp(0.0, 1.0);
            for (px, py) in [(x, y), (w - 1 - x, y), (x, h - 1 - y), (w - 1 - x, h - 1 - y)] {
                let p = img.get_pixel_mut(px, py);
                p[3] = (f64::from(p[3]) * cover).round() as u8;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_splits_on_the_diagonal() {
        let light = image::RgbaImage::from_pixel(160, 90, image::Rgba([255, 255, 255, 255]));
        let dark = image::RgbaImage::from_pixel(160, 90, image::Rgba([0, 0, 0, 255]));
        let tile = diagonal(light, &dark);
        assert_eq!(tile.get_pixel(5, 5)[0], 255);
        assert_eq!(tile.get_pixel(154, 84)[0], 0);
        assert_eq!(tile.get_pixel(80, 20)[0], 255);
        assert_eq!(tile.get_pixel(80, 70)[0], 0);
    }

    #[test]
    fn corners_are_cut_round() {
        let mut img = image::RgbaImage::from_pixel(40, 30, image::Rgba([10, 20, 30, 255]));
        round_corners(&mut img, 8.0);
        for (x, y) in [(0, 0), (39, 0), (0, 29), (39, 29)] {
            assert_eq!(img.get_pixel(x, y)[3], 0, "corner {x},{y}");
        }
        assert_eq!(img.get_pixel(8, 0)[3], 255);
        assert_eq!(img.get_pixel(20, 15)[3], 255);
    }

    #[test]
    fn the_desktop_is_named_by_its_tile() {
        let state = State {
            items: vec![Item { id: "/w/dusk-l.png".into(), kind: ItemKind::Pair { light: "/w/dusk-l.png".into(), dark: "/w/dusk-d.png".into() }, title: "Dusk".into(), credit: None }],
            ..State::default()
        };
        assert_eq!(title(&state, "/w/dusk-d.png"), "Dusk");
        assert_eq!(title(&state, "/elsewhere/3840x2160_lake-dark.jpg"), "Lake");
        assert_eq!(title(&state, &super::super::wallpaper::sky_source(4)), "Sky");
        assert_eq!(title(&state, ""), "");
    }

    #[test]
    fn identical_reads_bytes() {
        let dir = std::env::temp_dir().join(format!("fs-picker-identical-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (a, b, c) = (dir.join("a"), dir.join("b"), dir.join("c"));
        std::fs::write(&a, b"same").unwrap();
        std::fs::write(&b, b"same").unwrap();
        std::fs::write(&c, b"sane").unwrap();
        let s = |p: &std::path::Path| p.to_string_lossy().into_owned();
        assert!(identical(&s(&a), &s(&b)));
        assert!(!identical(&s(&a), &s(&c)));
        assert!(!identical(&s(&a), &s(&dir.join("missing"))));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
