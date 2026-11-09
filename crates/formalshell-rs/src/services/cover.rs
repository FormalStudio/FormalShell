//! Album art: an `artUrl` (a file, a `data:` URL as
//! mpv's mpris.lua hands out embedded art, or http art) fetched off the UI
//! thread, decoded on the pool, cropped to a square the way
//! `PreserveAspectCrop` fills a slot, scaled to the slot that asked and
//! rounded to its cover radius. A surface only ever draws the finished
//! bitmap off the store; a url and size already on its way is not asked
//! for twice.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use base64::Engine;
use image::RgbaImage;
use image::imageops::{self, FilterType};

use super::media;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

/// http art bigger than this is not album art.
const MAX_BYTES: &str = "8388608";

struct Ask {
    url: String,
    size: u32,
    radius: f64,
}

static ASK: OnceLock<async_channel::Sender<Ask>> = OnceLock::new();
static ASKED: Mutex<Option<HashSet<(String, u32)>>> = Mutex::new(None);

/// Queues `url` at `size` pixels square, unless it is already queued.
pub fn request(url: &str, size: u32, radius: f64) {
    if url.is_empty() || size == 0 {
        return;
    }
    let Ok(mut asked) = ASKED.lock() else { return };
    let asked = asked.get_or_insert_with(HashSet::new);
    let key = (url.to_owned(), size);
    if asked.contains(&key) {
        return;
    }
    asked.insert(key);
    if let Some(tx) = ASK.get() {
        let _ = tx.try_send(Ask { url: url.to_owned(), size, radius });
    }
}

pub fn start(ctx: &Ctx) {
    ctx.spawn(run(ctx.clone()));
}

async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = ASK.set(tx);
    let mut last: Option<(String, std::sync::Arc<Vec<u8>>)> = None;
    while let Ok(ask) = rx.recv().await {
        let bytes = match &last {
            Some((url, bytes)) if *url == ask.url => Some(bytes.clone()),
            _ => fetch(&ctx, &ask.url).await.map(std::sync::Arc::new),
        };
        let bitmap = match &bytes {
            Some(b) => {
                last = Some((ask.url.clone(), b.clone()));
                let b = b.clone();
                let (size, radius) = (ask.size, ask.radius);
                ctx.pool().run(move || decode(&b, size, radius)).await.flatten()
            }
            None => None,
        };
        if let Ok(mut asked) = ASKED.lock()
            && let Some(asked) = asked.as_mut()
        {
            asked.remove(&(ask.url.clone(), ask.size));
        }
        ctx.publish(store::Diff::Media(media::Diff::Cover(ask.url, ask.size, bitmap)));
    }
}

async fn fetch(ctx: &Ctx, url: &str) -> Option<Vec<u8>> {
    if let Some(rest) = url.strip_prefix("data:") {
        let (meta, body) = rest.split_once(',')?;
        return if meta.ends_with(";base64") {
            base64::engine::general_purpose::STANDARD.decode(body.trim()).ok()
        } else {
            Some(percent_decode(body).into_bytes())
        };
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        let out = crate::services::proc::command("curl")
            .args(["--fail", "--silent", "--location", "--connect-timeout", "4", "--max-time", "10", "--max-filesize", MAX_BYTES, url])
            .stdin(async_process::Stdio::null())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .ok()?;
        return (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout);
    }
    let file = url.strip_prefix("file://").unwrap_or(url);
    let path = percent_decode(file.split_once('?').map_or(file, |(p, _)| p));
    ctx.pool().run(move || std::fs::read(path).ok()).await.flatten()
}

pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The picture filling a `size` square, its overflow cropped evenly off
/// both sides, its corners cut to `radius`.
pub fn decode(bytes: &[u8], size: u32, radius: f64) -> Option<Bitmap> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || size == 0 {
        return None;
    }
    let side = w.min(h);
    let square = imageops::crop_imm(&img, (w - side) / 2, (h - side) / 2, side, side).to_image();
    let mut out: RgbaImage = if side == size { square } else { imageops::resize(&square, size, size, FilterType::Triangle) };
    round_corners(&mut out, radius);
    let size = u16::try_from(size).ok()?;
    Some(Bitmap::from_rgba(size, size, out.into_raw()))
}

pub fn round_corners(img: &mut RgbaImage, radius: f64) {
    let (w, h) = img.dimensions();
    let r = radius.min(w.min(h) as f64 / 2.0);
    if r <= 0.0 {
        return;
    }
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
            let cx = if px < r { r } else if px > w as f64 - r { w as f64 - r } else { continue };
            let cy = if py < r { r } else if py > h as f64 - r { h as f64 - r } else { continue };
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
            let cover = (r - d + 0.5).clamp(0.0, 1.0);
            let a = &mut img.get_pixel_mut(x, y).0[3];
            *a = (*a as f64 * cover).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = RgbaImage::from_fn(w, h, |x, _| if x < w / 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn a_wide_picture_is_cropped_to_its_middle_square() {
        let b = decode(&png(64, 32), 16, 0.0).unwrap();
        assert_eq!((b.pixmap.width(), b.pixmap.height()), (16, 16));
    }

    #[test]
    fn corners_are_cut_and_the_middle_kept() {
        let b = decode(&png(32, 32), 32, 8.0).unwrap();
        let px = b.pixmap.data_as_u8_slice();
        assert_eq!(px[3], 0, "the corner pixel is transparent");
        let mid = ((16 * 32) + 16) * 4;
        assert_eq!(px[mid + 3], 255);
    }

    #[test]
    fn a_file_url_is_percent_decoded() {
        assert_eq!(percent_decode("/tmp/a%20b.png"), "/tmp/a b.png");
    }

    #[test]
    fn garbage_decodes_to_nothing() {
        assert!(decode(b"nope", 16, 0.0).is_none());
    }
}
