//! Pictures other processes hand the shell: an icon by freedesktop name, a
//! raw pixmap or a PNG, decoded to straight-alpha RGBA on the pool and
//! scaled to a surface's own size (a few hundred pixels). Names resolve
//! for the pixel size they are drawn at, through the configured icon
//! theme's inheritance chain, then any other installed theme, then loose
//! pixmaps.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use image::imageops::{self, FilterType};
use image::RgbaImage;

use crate::scene::Bitmap;

/// The longest side a picture decoded without a target size keeps, so the
/// one resize to its drawn size starts from enough pixels.
const KEEP: u32 = 256;

#[derive(Clone)]
pub struct Raw {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<Vec<u8>>,
}

impl PartialEq for Raw {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rgba, &other.rgba)
    }
}

impl fmt::Debug for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Raw({}x{})", self.width, self.height)
    }
}

/// `img` with its longest side at `longest`, resampled once.
fn fit(img: RgbaImage, longest: u32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let side = w.max(h);
    if side == longest || side == 0 {
        return img;
    }
    let s = longest as f64 / side as f64;
    let (nw, nh) = (((w as f64 * s).round() as u32).clamp(1, longest), ((h as f64 * s).round() as u32).clamp(1, longest));
    let filter = if side > longest { FilterType::Lanczos3 } else { FilterType::CatmullRom };
    imageops::resize(&img, nw, nh, filter)
}

impl Raw {
    fn from_image(img: RgbaImage) -> Option<Self> {
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let img = if w.max(h) > KEEP { fit(img, KEEP) } else { img };
        let (width, height) = img.dimensions();
        Some(Self { width, height, rgba: Arc::new(img.into_raw()) })
    }

    /// Fitted inside a `size` square and centred in it, ready to draw on a
    /// surface at `scale`.
    pub fn bitmap(&self, size: u32, scale: f64) -> Option<Bitmap> {
        let size = crate::scene::pixels_at(f64::from(size.max(1)), scale);
        let src = RgbaImage::from_raw(self.width, self.height, self.rgba.as_ref().clone())?;
        let fitted = fit(src, size);
        let (w, h) = fitted.dimensions();
        let mut canvas = RgbaImage::new(size, size);
        imageops::replace(&mut canvas, &fitted, ((size - w.min(size)) / 2) as i64, ((size - h.min(size)) / 2) as i64);
        Some(Bitmap::from_rgba(size as u16, size as u16, canvas.into_raw()).at(scale))
    }
}

pub fn from_rgba(width: i32, height: i32, rgba: &[u8]) -> Option<Raw> {
    let img = RgbaImage::from_raw(u32::try_from(width).ok()?, u32::try_from(height).ok()?, rgba.to_vec())?;
    Raw::from_image(img)
}

pub fn from_bytes(bytes: &[u8]) -> Option<Raw> {
    Raw::from_image(image::load_from_memory(bytes).ok()?.to_rgba8())
}

/// An SVG rasterised with its longest side at `longest`, straight alpha.
fn from_svg(data: &[u8], longest: u32) -> Option<RgbaImage> {
    let tree = resvg::usvg::Tree::from_data(data, &resvg::usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = longest as f32 / size.width().max(size.height());
    let (w, h) = ((size.width() * scale).round().max(1.0) as u32, (size.height() * scale).round().max(1.0) as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut rgba = pixmap.take();
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(w, h, rgba)
}

fn is_svg(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "svg")
}

/// The file's pixels: an SVG rendered at `longest` (or [`KEEP`]), a raster
/// resampled once to `longest` when one is given.
fn decode(path: &Path, longest: Option<u32>) -> Option<RgbaImage> {
    let data = std::fs::read(path).ok()?;
    if is_svg(path) {
        return from_svg(&data, longest.unwrap_or(KEEP));
    }
    let img = image::load_from_memory(&data).ok()?.to_rgba8();
    Some(match longest {
        Some(n) => fit(img, n),
        None => img,
    })
}

pub fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    match std::env::var_os("XDG_DATA_HOME") {
        Some(d) if !d.is_empty() => dirs.push(PathBuf::from(d)),
        _ => dirs.extend(home.iter().map(|h| h.join(".local/share"))),
    }
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
    if system.is_empty() {
        dirs.extend(["/usr/local/share", "/usr/share"].map(PathBuf::from));
    } else {
        dirs.extend(system.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    }
    dirs.extend(home.iter().map(|h| h.join(".nix-profile/share")));
    dirs.push(PathBuf::from("/run/current-system/sw/share"));
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// One file a name resolved to, and what its directory says about it.
struct Candidate {
    path: PathBuf,
    /// The directory matches the size at scale 1 (the spec's
    /// DirectoryMatchesSize).
    exact: bool,
    /// The spec's DirectorySizeDistance, or the gap to a size parsed off a
    /// directory name.
    distance: u32,
    /// The pixel size the directory claims.
    nominal: u32,
}

/// Lower is better. A raster is judged by its own pixels rather than its
/// directory (elementary-colloid ships 64px files in `apps@2x/64`, and
/// 48px ones in `apps@3x/16`, which claims 24 to 69), and one that would be
/// upscaled loses to anything that would not; then the spec's exact match,
/// the nearest size, the larger of two equally near, a raster drawn for
/// the size over an SVG that renders at it.
fn score(c: &Candidate, size: u32) -> (bool, bool, u32, Reverse<u32>, bool) {
    if is_svg(&c.path) {
        return (false, !c.exact, c.distance, Reverse(size), true);
    }
    let pixels = image::image_dimensions(&c.path).map(|(w, h)| w.max(h)).unwrap_or(c.nominal);
    (pixels < size, !c.exact, pixels.abs_diff(size), Reverse(pixels), false)
}

/// The best candidate, and whether drawing it at `size` upscales it.
fn best(candidates: Vec<Candidate>, size: u32) -> Option<(bool, PathBuf)> {
    candidates.into_iter().map(|c| (score(&c, size), c.path)).min_by(|a, b| a.0.cmp(&b.0)).map(|(s, p)| (s.0, p))
}

/// A size directory's name, `64x64`, `64`, `64x64@2` or `48@2x`, as the
/// pixel size it holds.
fn dir_size(dir: &str) -> Option<u32> {
    let (n, scale) = dir.split_once('@').map_or((dir, 1), |(n, s)| (n, s.trim_end_matches('x').parse().unwrap_or(1)));
    n.split('x').next()?.parse::<u32>().ok().map(|n| n * scale)
}

fn subdirs(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(read) = std::fs::read_dir(dir) else { return Vec::new() };
    read.filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| Some((e.file_name().into_string().ok()?, e.path())))
        .collect()
}

fn files_in(dir: &Path, name: &str) -> Vec<PathBuf> {
    ["svg", "png"].map(|ext| dir.join(format!("{name}.{ext}"))).into_iter().filter(|p| p.is_file()).collect()
}

fn loose(path: PathBuf) -> Candidate {
    Candidate { path, exact: false, distance: u32::MAX, nominal: 0 }
}

/// Every file for `name` inside one theme directory with no index read, in
/// either layout: `<size>/<context>/` or `<context>/<size>/`.
fn in_theme(theme: &Path, name: &str, size: u32) -> Vec<Candidate> {
    let sized = |dir: &str, path: PathBuf| match (dir, dir_size(dir)) {
        ("scalable", _) => Candidate { path, exact: true, distance: 0, nominal: size },
        (_, Some(n)) => Candidate { path, exact: n == size, distance: n.abs_diff(size), nominal: n },
        _ => loose(path),
    };
    let mut found: Vec<Candidate> = files_in(theme, name).into_iter().map(loose).collect();
    for (first, first_path) in subdirs(theme) {
        found.extend(files_in(&first_path, name).into_iter().map(|p| sized(&first, p)));
        for (second, second_path) in subdirs(&first_path) {
            let dir = if first == "scalable" || dir_size(&first).is_some() { &first } else { &second };
            found.extend(files_in(&second_path, name).into_iter().map(|p| sized(dir, p)));
        }
    }
    found
}

/// An icon name (or an absolute path, or a `file://` URL) as the file that
/// draws best at `size` pixels: the app's own theme path first, then the
/// theme chain, then every other installed theme, then the loose pixmaps.
pub fn lookup(name: &str, theme_path: &str, size: u32) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    if let Some(path) = name.strip_prefix("file://").or_else(|| Path::new(name).is_absolute().then_some(name)) {
        return Path::new(path).is_file().then(|| PathBuf::from(path));
    }
    static FOUND: OnceLock<Mutex<HashMap<(String, String, u32), PathBuf>>> = OnceLock::new();
    let found = FOUND.get_or_init(Default::default);
    let key = (name.to_owned(), theme_path.to_owned(), size);
    if let Some(hit) = found.lock().unwrap().get(&key) {
        return Some(hit.clone());
    }
    let path = search(name, theme_path, size)?;
    found.lock().unwrap().insert(key, path.clone());
    Some(path)
}

fn search(name: &str, theme_path: &str, size: u32) -> Option<PathBuf> {
    if !theme_path.is_empty() {
        if let Some((_, p)) = best(in_theme(Path::new(theme_path), name, size), size) {
            return Some(p);
        }
    }
    static THEMES: OnceLock<Mutex<Themes>> = OnceLock::new();
    let chained = THEMES.get_or_init(|| Mutex::new(Themes::new(icon_bases(), configured_theme()))).lock().unwrap().find(name, size);
    if chained.is_some() {
        return chained;
    }
    let dirs = data_dirs();
    let mut themes: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        let mut found: Vec<PathBuf> = subdirs(&d.join("icons")).into_iter().map(|(_, p)| p).collect();
        found.sort();
        themes.extend(found);
    }
    let pixmaps = || best(dirs.iter().flat_map(|d| files_in(&d.join("pixmaps"), name)).map(loose).collect(), size).map(|(_, p)| p);
    themes.iter().find_map(|t| best(in_theme(t, name, size), size).map(|(_, p)| p)).or_else(pixmaps)
}

/// A file decoded once per process, at up to [`KEEP`] pixels, for a
/// consumer that only learns its drawn size later.
pub fn load(path: &Path) -> Option<Raw> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<Raw>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().unwrap().get(path) {
        return hit.clone();
    }
    let raw = decode(path, None).and_then(Raw::from_image);
    cache.lock().unwrap().insert(path.to_owned(), raw.clone());
    raw
}

/// How many decoded (file, pixel size) pairs [`load_at`] keeps: every app
/// in the launcher at two scales, with room, at 25 KB an 80 px icon.
const LOADED: usize = 512;

/// A file drawn in a `size` square on a surface at `scale`: an SVG
/// rendered at its pixel size, a raster resampled to it once. Decoded once
/// per (file, pixel size), so a surface at another scale has its own.
pub fn load_at(path: &Path, size: u32, scale: f64) -> Option<Bitmap> {
    static CACHE: OnceLock<Mutex<HashMap<(PathBuf, u32), Option<Bitmap>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let px = crate::scene::pixels_at(f64::from(size.max(1)), scale);
    let key = (path.to_owned(), px);
    if let Some(hit) = cache.lock().unwrap().get(&key) {
        return hit.clone().map(|b| b.at(scale));
    }
    let bitmap = decode(path, Some(px)).and_then(|img| Raw { width: img.width(), height: img.height(), rgba: Arc::new(img.into_raw()) }.bitmap(px, 1.0));
    let mut cache = cache.lock().unwrap();
    if cache.len() >= LOADED {
        cache.clear();
    }
    cache.insert(key, bitmap.clone());
    bitmap.map(|b| b.at(scale))
}

/// `size` as the pixel size a named icon is looked up at: the largest any
/// output draws it at, so a surface at that scale never upscales it.
pub fn pick(size: u32) -> u32 {
    crate::scene::pixels_at(f64::from(size), crate::surface::largest_scale())
}

/// `name` looked up for `size` pixels, decoded for a later [`Raw::bitmap`].
pub fn named(name: &str, theme_path: &str, size: u32) -> Option<Raw> {
    load(&lookup(name, theme_path, size)?)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Fixed,
    Scalable,
    Threshold,
}

struct Dir {
    path: String,
    size: u32,
    scale: u32,
    min: u32,
    max: u32,
    threshold: u32,
    kind: Kind,
}

impl Dir {
    /// The freedesktop spec's DirectorySizeDistance.
    fn distance(&self, size: u32) -> u32 {
        let want = size as i64;
        let (s, k) = (self.size as i64 * self.scale as i64, self.scale as i64);
        match self.kind {
            Kind::Fixed => (s - want).unsigned_abs() as u32,
            Kind::Scalable => {
                let (lo, hi) = (self.min as i64 * k, self.max as i64 * k);
                if want < lo { (lo - want) as u32 } else if want > hi { (want - hi) as u32 } else { 0 }
            }
            Kind::Threshold => {
                let (lo, hi) = ((self.size as i64 - self.threshold as i64) * k, (self.size as i64 + self.threshold as i64) * k);
                if want < lo { (lo - want) as u32 } else if want > hi { (want - hi) as u32 } else { 0 }
            }
        }
    }

    /// The spec's DirectoryMatchesSize at scale 1.
    fn matches(&self, size: u32) -> bool {
        self.scale == 1
            && match self.kind {
                Kind::Fixed => self.size == size,
                Kind::Scalable => (self.min..=self.max).contains(&size),
                Kind::Threshold => self.size.abs_diff(size) <= self.threshold,
            }
    }
}

struct Theme {
    inherits: Vec<String>,
    dirs: Vec<Dir>,
    /// Each base directory this theme has a folder in.
    roots: Vec<PathBuf>,
}

struct Themes {
    bases: Vec<PathBuf>,
    configured: String,
    loaded: HashMap<String, Option<Theme>>,
    chain: Option<Vec<String>>,
}

fn icon_bases() -> Vec<PathBuf> {
    let mut bases = vec![home().join(".icons")];
    bases.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    bases
}

/// The theme the GTK settings name, hicolor when none does.
fn configured_theme() -> String {
    let config = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map_or_else(|| home().join(".config"), PathBuf::from);
    let settings = std::fs::read_to_string(config.join("gtk-3.0/settings.ini")).unwrap_or_default();
    settings
        .lines()
        .find_map(|l| l.trim().strip_prefix("gtk-icon-theme-name").and_then(|r| r.trim_start().strip_prefix('=')))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "hicolor".into())
}

fn load_theme(name: &str, bases: &[PathBuf]) -> Option<Theme> {
    let roots: Vec<PathBuf> = bases.iter().map(|b| b.join(name)).filter(|r| r.is_dir()).collect();
    let index = roots.iter().find_map(|r| std::fs::read_to_string(r.join("index.theme")).ok())?;
    let mut sections: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in index.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.to_owned(), HashMap::new()));
        } else if let (Some((k, v)), Some((_, fields))) = (line.split_once('='), sections.last_mut()) {
            fields.entry(k.trim().to_owned()).or_insert_with(|| v.trim().to_owned());
        }
    }
    let list = |s: &HashMap<String, String>, k: &str| -> Vec<String> {
        s.get(k).map(|v| v.split(',').map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect()).unwrap_or_default()
    };
    let head = sections.iter().find(|(n, _)| n == "Icon Theme").map(|(_, f)| f)?;
    let num = |s: &HashMap<String, String>, k: &str, default: u32| s.get(k).and_then(|v| v.parse().ok()).unwrap_or(default);
    let dirs = list(head, "Directories")
        .into_iter()
        .filter_map(|path| {
            let fields = &sections.iter().find(|(n, _)| *n == path)?.1;
            let size = num(fields, "Size", 0);
            let kind = match fields.get("Type").map(String::as_str) {
                Some("Fixed") => Kind::Fixed,
                Some("Scalable") => Kind::Scalable,
                _ => Kind::Threshold,
            };
            Some(Dir {
                size,
                scale: num(fields, "Scale", 1).max(1),
                min: num(fields, "MinSize", size),
                max: num(fields, "MaxSize", size),
                threshold: num(fields, "Threshold", 2),
                kind,
                path,
            })
        })
        .collect();
    Some(Theme { inherits: list(head, "Inherits"), dirs, roots })
}

impl Themes {
    fn new(bases: Vec<PathBuf>, configured: String) -> Self {
        Self { bases, configured, loaded: HashMap::new(), chain: None }
    }

    fn theme(&mut self, name: &str) -> Option<&Theme> {
        let bases = &self.bases;
        self.loaded.entry(name.to_owned()).or_insert_with(|| load_theme(name, bases)).as_ref()
    }

    /// The configured theme, what it inherits depth first, then hicolor.
    fn order(&mut self) -> Vec<String> {
        if let Some(c) = &self.chain {
            return c.clone();
        }
        let mut order: Vec<String> = Vec::new();
        let mut stack = vec![self.configured.clone()];
        while let Some(name) = stack.pop() {
            if order.contains(&name) {
                continue;
            }
            if let Some(t) = self.theme(&name) {
                stack.extend(t.inherits.iter().rev().cloned());
            }
            order.push(name);
        }
        if !order.iter().any(|n| n == "hicolor") {
            order.push("hicolor".into());
        }
        self.chain = Some(order.clone());
        order
    }

    /// The first theme in the chain carrying `name`, its best file for
    /// `size`; a theme whose best would be upscaled yields to a later one
    /// that has the pixels, and is the answer only when none does.
    fn find(&mut self, name: &str, size: u32) -> Option<PathBuf> {
        let mut fallback = None;
        for theme in self.order() {
            let Some(t) = self.theme(&theme) else { continue };
            let mut found = Vec::new();
            for dir in &t.dirs {
                for root in &t.roots {
                    for path in files_in(&root.join(&dir.path), name) {
                        found.push(Candidate { path, exact: dir.matches(size), distance: dir.distance(size), nominal: dir.size * dir.scale });
                    }
                }
            }
            match best(found, size) {
                Some((false, path)) => return Some(path),
                Some((true, path)) => {
                    fallback.get_or_insert(path);
                }
                None => {}
            }
        }
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(path: &Path, side: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        RgbaImage::from_pixel(side, side, image::Rgba([255, 0, 0, 255])).save(path).unwrap();
    }

    fn svg(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16" fill="#f00"/></svg>"##).unwrap();
    }

    /// elementary-colloid's shape: per-size app directories, every one
    /// Scalable, the `@3x/16` one claiming 24 to 69 pixels.
    fn fixture(base: &Path) {
        let theme = base.join("fx");
        std::fs::create_dir_all(&theme).unwrap();
        std::fs::write(
            theme.join("index.theme"),
            "[Icon Theme]\nName=fx\nInherits=hicolor\nDirectories=apps/16,apps@3x/16,apps/48,apps/64,apps/128,apps/scalable\n\n\
             [apps/16]\nSize=16\nMinSize=8\nMaxSize=23\nType=Scalable\n\n\
             [apps@3x/16]\nSize=16\nScale=3\nMinSize=8\nMaxSize=23\nType=Scalable\n\n\
             [apps/48]\nSize=48\nMinSize=8\nMaxSize=63\nType=Scalable\n\n\
             [apps/64]\nSize=64\nMinSize=8\nMaxSize=127\nType=Scalable\n\n\
             [apps/128]\nSize=128\nMinSize=8\nMaxSize=512\nType=Scalable\n\n\
             [apps/scalable]\nSize=64\nMinSize=8\nMaxSize=512\nType=Scalable\n",
        )
        .unwrap();
        for (dir, side) in [("apps/16", 16), ("apps@3x/16", 48), ("apps/48", 48), ("apps/64", 64), ("apps/128", 128)] {
            png(&theme.join(dir).join("app.png"), side);
        }
        png(&theme.join("apps/16/tiny.png"), 16);
        svg(&theme.join("apps/scalable/vector.svg"));
        png(&theme.join("apps/48/vector.png"), 48);
        let hicolor = base.join("hicolor");
        std::fs::create_dir_all(&hicolor).unwrap();
        std::fs::write(hicolor.join("index.theme"), "[Icon Theme]\nName=hicolor\nDirectories=256x256/apps\n\n[256x256/apps]\nSize=256\nType=Threshold\n").unwrap();
        png(&hicolor.join("256x256/apps/tiny.png"), 256);
        png(&hicolor.join("256x256/apps/only.png"), 256);
    }

    fn found(themes: &mut Themes, name: &str, size: u32) -> String {
        let path = themes.find(name, size).unwrap();
        let base = themes.bases[0].clone();
        path.strip_prefix(base).unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn a_theme_picks_the_file_for_the_drawn_size() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let mut themes = Themes::new(vec![dir.path().to_owned()], "fx".into());
        assert_eq!(found(&mut themes, "app", 64), "fx/apps/64/app.png");
        assert_eq!(found(&mut themes, "app", 16), "fx/apps/16/app.png");
        assert_eq!(found(&mut themes, "app", 48), "fx/apps/48/app.png");
        assert_eq!(found(&mut themes, "app", 100), "fx/apps/128/app.png");
        assert_eq!(found(&mut themes, "app", 300), "fx/apps/128/app.png");
        assert_eq!(found(&mut themes, "vector", 64), "fx/apps/scalable/vector.svg");
        assert_eq!(found(&mut themes, "vector", 48), "fx/apps/48/vector.png");
        assert_eq!(found(&mut themes, "only", 64), "hicolor/256x256/apps/only.png");
    }

    #[test]
    fn a_small_raster_yields_to_an_inherited_larger_one() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let mut themes = Themes::new(vec![dir.path().to_owned()], "fx".into());
        assert_eq!(found(&mut themes, "tiny", 64), "hicolor/256x256/apps/tiny.png");
        assert_eq!(found(&mut themes, "tiny", 16), "fx/apps/16/tiny.png");
    }

    #[test]
    fn an_unindexed_theme_and_loose_pixmaps_rank_by_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let t = dir.path().join("t");
        png(&t.join("32x32/apps/a.png"), 32);
        png(&t.join("apps/128/a.png"), 128);
        png(&t.join("256x256@2/apps/a.png"), 512);
        svg(&t.join("scalable/apps/b.svg"));
        png(&t.join("64x64/apps/b.png"), 64);
        let pick = |name: &str, size: u32| best(in_theme(&t, name, size), size).unwrap().1.strip_prefix(&t).unwrap().to_string_lossy().into_owned();
        assert_eq!(pick("a", 64), "apps/128/a.png");
        assert_eq!(pick("a", 32), "32x32/apps/a.png");
        assert_eq!(pick("a", 300), "256x256@2/apps/a.png");
        assert_eq!(pick("b", 64), "64x64/apps/b.png");
        assert_eq!(pick("b", 96), "scalable/apps/b.svg");
        let pixmaps = dir.path().join("pixmaps");
        png(&pixmaps.join("small/p.png"), 24);
        png(&pixmaps.join("large/p.png"), 96);
        let loose_pick = best(vec![loose(pixmaps.join("small/p.png")), loose(pixmaps.join("large/p.png"))], 64).unwrap();
        assert_eq!(loose_pick, (false, pixmaps.join("large/p.png")));
        assert_eq!(dir_size("48@2x"), Some(96));
    }

    #[test]
    fn an_absolute_path_is_its_own_answer() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("icon.png");
        png(&p, 24);
        let s = p.to_string_lossy().into_owned();
        assert_eq!(lookup(&s, "", 64), Some(p.clone()));
        assert_eq!(lookup(&format!("file://{s}"), "", 64), Some(p.clone()));
        assert_eq!(lookup(&dir.path().join("missing.png").to_string_lossy(), "", 64), None);
    }

    #[test]
    fn load_at_draws_at_the_asked_size() {
        let dir = tempfile::tempdir().unwrap();
        for (name, side) in [("small.png", 16), ("large.png", 512)] {
            png(&dir.path().join(name), side);
            let b = load_at(&dir.path().join(name), 64, 1.0).unwrap();
            assert_eq!((b.pixmap.width(), b.pixmap.height()), (64, 64));
        }
        let v = dir.path().join("v.svg");
        svg(&v);
        let b = load_at(&v, 48, 1.0).unwrap();
        assert_eq!((b.pixmap.width(), b.pixmap.height()), (48, 48));
        assert_eq!(b.pixmap.data_as_u8_slice()[24 * 48 * 4..][..4], [255, 0, 0, 255]);
    }

    #[test]
    fn load_at_makes_a_scaled_surface_its_own_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let v = dir.path().join("v.svg");
        svg(&v);
        let one = load_at(&v, 48, 1.0).unwrap();
        let more = load_at(&v, 48, 1.25).unwrap();
        assert_eq!((more.pixmap.width(), more.pixmap.height(), more.density), (60, 60, 1.25));
        assert_eq!(more.size(), (48, 48));
        assert!(!one.same_as(&more));
        assert_eq!((one.pixmap.width(), one.density), (48, 1.0));
        // The same pixels asked for at another logical size and scale are
        // the cache's, stamped for the asker.
        let again = load_at(&v, 60, 1.0).unwrap();
        assert!(std::sync::Arc::ptr_eq(&again.pixmap, &more.pixmap));
        assert_eq!(again.density, 1.0);
    }

    #[test]
    fn size_distance() {
        let fixed = Dir { path: "48x48/apps".into(), size: 48, scale: 1, min: 48, max: 48, threshold: 2, kind: Kind::Fixed };
        assert_eq!(fixed.distance(20), 28);
        let svg = Dir { path: "scalable/apps".into(), size: 128, scale: 1, min: 8, max: 512, threshold: 2, kind: Kind::Scalable };
        assert_eq!(svg.distance(20), 0);
        let thr = Dir { path: "32x32".into(), size: 32, scale: 1, min: 32, max: 32, threshold: 2, kind: Kind::Threshold };
        assert_eq!(thr.distance(33), 0);
        assert_eq!(thr.distance(40), 6);
        let at3 = Dir { path: "apps@3x/16".into(), size: 16, scale: 3, min: 8, max: 23, threshold: 2, kind: Kind::Scalable };
        assert_eq!(at3.distance(64), 0);
        assert!(!at3.matches(64));
    }

    #[test]
    fn a_pixmap_fits_its_square() {
        let raw = from_rgba(2, 1, &[255, 0, 0, 255, 0, 255, 0, 255]).unwrap();
        let b = raw.bitmap(4, 1.0).unwrap();
        assert_eq!((b.pixmap.width(), b.pixmap.height()), (4, 4));
        let b = raw.bitmap(4, 1.5).unwrap();
        assert_eq!((b.pixmap.width(), b.pixmap.height(), b.size()), (6, 6, (4, 4)));
    }

    #[test]
    fn svg_renders_at_the_longest_side() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="8"><rect width="16" height="8" fill="#f00"/></svg>"##;
        let img = from_svg(svg, 64).unwrap();
        assert_eq!(img.dimensions(), (64, 32));
        assert_eq!(&img.as_raw()[..4], &[255, 0, 0, 255]);
    }
}
