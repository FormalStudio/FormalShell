//! `debug staleAudit`: every frame a surface commits checked against the
//! same scene drawn whole into a fresh canvas. A partial redraw that leaves
//! a pixel behind (a node drawing past the damage its bounds give, a
//! buffer reused with a rect it missed) shows up as the committed buffer
//! differing from that full draw. Off unless a leg turns it on: the full
//! draw costs a whole frame per commit.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::json;
use vello_cpu::Pixmap;

use crate::render::Renderer;
use crate::scene::{IRect, Scene};

static ON: AtomicBool = AtomicBool::new(false);
static SEEN: Mutex<BTreeMap<&'static str, Seen>> = Mutex::new(BTreeMap::new());

/// A channel further off than this is a stale pixel rather than the
/// antialiasing of a tile drawn at another origin.
const TOLERANCE: u8 = 24;

#[derive(Default)]
struct Seen {
    commits: u64,
    stale: u64,
    worst: u64,
    first: Option<(u64, IRect, String)>,
    last: Option<(u64, IRect)>,
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn start() {
    SEEN.lock().unwrap().clear();
    ON.store(true, Ordering::Relaxed);
}

pub fn stop() {
    ON.store(false, Ordering::Relaxed);
}

pub fn report() -> serde_json::Value {
    let seen = SEEN.lock().unwrap();
    let rect = |r: &IRect| json!({"x": r.x, "y": r.y, "width": r.w, "height": r.h});
    let surfaces: serde_json::Map<String, serde_json::Value> = seen
        .iter()
        .map(|(name, s)| {
            let first = s.first.as_ref().map(|(n, r, png)| json!({"commit": n, "box": rect(r), "png": png}));
            let last = s.last.as_ref().map(|(n, r)| json!({"commit": n, "box": rect(r)}));
            (name.to_string(), json!({"commits": s.commits, "stale": s.stale, "worstPx": s.worst, "first": first, "last": last}))
        })
        .collect();
    json!({"on": on(), "surfaces": surfaces})
}

/// `scene` drawn whole at `scale` into a fresh canvas.
pub fn reference(scene: &Scene, scale: f64) -> Pixmap {
    let mut r = Renderer::new(1, 1);
    r.set_scale(scale);
    r.resize(scene.size.w.max(1) as u16, scene.size.h.max(1) as u16);
    r.render(scene, scene.size);
    r.canvas().clone()
}

/// The buffer a surface is about to commit (`bytes`, in its shm `format`
/// and faded by `opacity` out of 256) against `reference`.
pub fn compare(name: &'static str, commit: u64, reference: &Pixmap, bytes: &[u8], bgra: bool, opacity: u16) {
    let w = reference.width() as usize;
    let fade = |c: u8| if opacity < 256 { ((u16::from(c) * opacity + 128) >> 8) as u8 } else { c };
    let (mut count, mut x0, mut y0, mut x1, mut y1) = (0u64, usize::MAX, usize::MAX, 0, 0);
    for (i, (px, got)) in reference.data().iter().zip(bytes.chunks_exact(4)).enumerate() {
        let want = if bgra { [px.b, px.g, px.r, px.a] } else { [px.r, px.g, px.b, px.a] }.map(fade);
        if want.iter().zip(got).any(|(a, b)| a.abs_diff(*b) > TOLERANCE) {
            let (x, y) = (i % w, i / w);
            count += 1;
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    let mut seen = SEEN.lock().unwrap();
    let s = seen.entry(name).or_default();
    s.commits += 1;
    if count == 0 {
        return;
    }
    let r = IRect::new(x0 as i32, y0 as i32, (x1 - x0 + 1) as i32, (y1 - y0 + 1) as i32);
    s.stale += 1;
    s.worst = s.worst.max(count);
    s.last = Some((commit, r));
    if s.first.is_none() {
        let png = save(name, commit, reference, bytes, bgra, r).unwrap_or_default();
        eprintln!("stale surface={name} commit={commit} px={count} box=[{},{},{}x{}] png={png}", r.x, r.y, r.w, r.h);
        s.first = Some((commit, r, png));
    }
}

/// The stale box and a margin round it, the buffer on top and the full
/// draw under it, for reading by eye.
fn save(name: &str, commit: u64, reference: &Pixmap, bytes: &[u8], bgra: bool, r: IRect) -> Option<String> {
    let dir = match std::env::var_os("FS_STALE_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => crate::paths::session_runtime()?.join("formalshell-stale"),
    };
    std::fs::create_dir_all(&dir).ok()?;
    let (w, h) = (reference.width() as i32, reference.height() as i32);
    let crop = IRect::new(r.x - 24, r.y - 24, r.w + 48, r.h + 48).intersect(&IRect::new(0, 0, w, h));
    let mut out = image::RgbaImage::new(crop.w as u32, crop.h as u32 * 2);
    for y in 0..crop.h {
        for x in 0..crop.w {
            let i = ((crop.y + y) * w + crop.x + x) as usize;
            let g = &bytes[i * 4..i * 4 + 4];
            let got = if bgra { [g[2], g[1], g[0], g[3]] } else { [g[0], g[1], g[2], g[3]] };
            let px = reference.data()[i];
            out.put_pixel(x as u32, y as u32, image::Rgba(got));
            out.put_pixel(x as u32, (y + crop.h) as u32, image::Rgba([px.r, px.g, px.b, px.a]));
        }
    }
    let path = dir.join(format!("{name}-{commit}.png"));
    out.save(&path).ok()?;
    Some(path.display().to_string())
}
