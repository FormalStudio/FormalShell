//! Apple Music's animated cover:
//! behind `media.appleMusicArt`, the track's album is looked up over the
//! undocumented chain fs-media's `applemusic` parses (iTunes artist search,
//! that artist's albums, a scraped web-player token, amp-api's editorial
//! video, the HLS playlists, one progressive mp4) and the mp4 kept under
//! `$XDG_CACHE_HOME/formalshell/applemusic-art`. While a slot shows the
//! cover (the media panel's, and each bar's now-playing cell behind
//! `media.animatedBarCover`), one ffmpeg child decodes that mp4 to raw
//! frames at ~8 fps, cropped to the largest slot; each frame is scaled to
//! every slot and rounded on the pool, and published to the store. A paused
//! track stops the child where it is and keeps its last frame; the last
//! slot going kills it.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use async_io::Timer;
use fs_media::applemusic as am;
use futures_lite::AsyncReadExt;

use super::media;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

/// One frame per 120 ms.
const FPS: &str = "25/3";
/// The lookup waits out a track skipped straight past (`_debounce`).
const DEBOUNCE: Duration = Duration::from_millis(1500);
/// Cached artwork older than this is pruned (`_prune`).
const MAX_AGE_DAYS: f64 = 30.0;
const CURL: [&str; 10] = ["-sS", "--fail", "-L", "--max-redirs", "5", "--connect-timeout", "5", "--max-time", "20", "--compressed"];

/// What a cover slot asks for while it is on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    pub artist: String,
    pub album: String,
    pub size: u32,
    pub radius: f64,
    pub playing: bool,
}

impl Want {
    pub fn key(&self) -> String {
        am::cache_key(&self.artist, &self.album)
    }

    /// The same slot on a surface at `scale`, in its pixels.
    pub fn at(mut self, scale: f64) -> Self {
        self.size = crate::scene::pixels_at(f64::from(self.size), scale);
        self.radius *= scale;
        self
    }
}

/// One decode for every slot showing the same album: the slots by size,
/// largest first, and whether any of them is playing.
#[derive(Clone, Debug, PartialEq)]
struct Job {
    artist: String,
    album: String,
    slots: Vec<(u32, f64)>,
    playing: bool,
}

impl Job {
    fn key(&self) -> String {
        am::cache_key(&self.artist, &self.album)
    }
}

/// The media panel's slot id; a bar cell takes one of its own off `slot()`.
pub const PANEL: u64 = 0;

pub fn slot() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(PANEL + 1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

static WANT: OnceLock<async_channel::Sender<Option<Job>>> = OnceLock::new();
static WANTS: Mutex<(BTreeMap<u64, Want>, Option<Job>)> = Mutex::new((BTreeMap::new(), None));

/// The slots folded into one decode: the album of the lowest id (the
/// panel, when it is up), every slot showing that album.
fn job(wants: &BTreeMap<u64, Want>) -> Option<Job> {
    let first = wants.values().next()?;
    let key = first.key();
    let same: Vec<&Want> = wants.values().filter(|w| w.key() == key).collect();
    let mut slots: Vec<(u32, f64)> = same.iter().map(|w| (w.size, w.radius)).collect();
    slots.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.total_cmp(&b.1)));
    slots.dedup_by_key(|s| s.0);
    Some(Job { artist: first.artist.clone(), album: first.album.clone(), slots, playing: same.iter().any(|w| w.playing) })
}

/// Slot `id`'s want, or none once it is off screen. Repeats are dropped
/// here, so a slot may say it on every draw.
pub fn want(id: u64, w: Option<Want>) {
    let Ok(mut guard) = WANTS.lock() else { return };
    let (wants, last) = &mut *guard;
    if wants.get(&id) == w.as_ref() {
        return;
    }
    match w {
        Some(w) => wants.insert(id, w),
        None => wants.remove(&id),
    };
    let next = job(wants);
    if *last == next {
        return;
    }
    *last = next.clone();
    if let Some(tx) = WANT.get() {
        let _ = tx.try_send(next);
    }
}

fn cache_dir() -> String {
    let base = std::env::var("XDG_CACHE_HOME").ok().filter(|d| !d.is_empty()).unwrap_or_else(|| format!("{}/.cache", std::env::var("HOME").unwrap_or_default()));
    format!("{base}/formalshell/applemusic-art")
}

/// The decode running for one slot: the child's pid, the flag that holds
/// it after the frame in flight (a pause) and the one that ends its reader.
struct Decode {
    key: String,
    slots: Vec<(u32, f64)>,
    pid: Arc<AtomicI32>,
    hold: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

fn signal(pid: &AtomicI32, sig: i32) {
    let pid = pid.load(Ordering::Relaxed);
    if pid > 0 {
        // SAFETY: kill(2) on our own child's pid, which the reader keeps
        // unreaped until it has cleared it here.
        unsafe { libc::kill(pid, sig) };
    }
}

impl Decode {
    fn set_playing(&self, playing: bool) {
        if self.hold.swap(!playing, Ordering::Relaxed) && playing {
            signal(&self.pid, libc::SIGCONT);
        }
    }

    fn end(self) {
        self.stop.store(true, Ordering::Relaxed);
        signal(&self.pid, libc::SIGKILL);
    }
}

pub async fn run(ctx: Ctx) {
    let (tx, rx) = async_channel::unbounded();
    let _ = WANT.set(tx);
    let dir = cache_dir();
    prune(&dir).await;
    // A key's mp4, or none for an album Apple has no motion art for.
    let mut found: HashMap<String, Option<String>> = HashMap::new();
    let mut token = String::new();
    let mut decode: Option<Decode> = None;
    while let Ok(mut w) = rx.recv().await {
        while let Ok(next) = rx.try_recv() {
            w = next;
        }
        let Some(w) = w else {
            if let Some(d) = decode.take() {
                d.end();
            }
            ctx.publish(store::Diff::Media(media::Diff::Motion(None)));
            continue;
        };
        let key = w.key();
        if let Some(d) = decode.as_ref().filter(|d| d.key == key && d.slots == w.slots) {
            d.set_playing(w.playing);
            continue;
        }
        if let Some(d) = decode.take() {
            d.end();
        }
        ctx.publish(store::Diff::Media(media::Diff::Motion(None)));
        let path = match found.get(&key) {
            Some(p) => p.clone(),
            None => {
                let path = format!("{dir}/{key}.mp4");
                let on_disk = std::fs::metadata(&path).is_ok_and(|m| m.len() > 0);
                if on_disk {
                    Some(path)
                } else {
                    Timer::after(DEBOUNCE).await;
                    if !rx.is_empty() {
                        // Something newer is waiting; it decides.
                        continue;
                    }
                    match lookup(&w, &dir, &key, &mut token).await {
                        Ok(p) => p,
                        Err(why) => {
                            eprintln!("AppleMusicArtService: {why}");
                            continue;
                        }
                    }
                }
            }
        };
        found.insert(key.clone(), path.clone());
        let Some(path) = path.filter(|_| rx.is_empty()) else { continue };
        // A paused track still shows its first frame, then holds it.
        let d = Decode {
            key: key.clone(),
            slots: w.slots.clone(),
            pid: Arc::new(AtomicI32::new(0)),
            hold: Arc::new(AtomicBool::new(!w.playing)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        ctx.spawn(frames(ctx.clone(), key, path, w.slots.clone(), d.pid.clone(), d.hold.clone(), d.stop.clone()));
        decode = Some(d);
    }
}

/// The ffmpeg argv for one slot: decoded on the CPU, looped, paced to
/// real time, cropped to a `size` square, raw RGBA on stdout.
fn ffmpeg_args(path: &str, size: u32) -> Vec<String> {
    let filter = format!("fps={FPS},scale={size}:{size}:force_original_aspect_ratio=increase,crop={size}:{size}");
    ["-nostdin", "-hide_banner", "-loglevel", "error", "-re", "-stream_loop", "-1", "-i", path, "-an", "-vf", &filter, "-f", "rawvideo", "-pix_fmt", "rgba", "-"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

async fn frames(ctx: Ctx, key: String, path: String, slots: Vec<(u32, f64)>, pid: Arc<AtomicI32>, hold: Arc<AtomicBool>, stop: Arc<AtomicBool>) {
    let Some(&(size, _)) = slots.first() else { return };
    let child = crate::services::proc::command("ffmpeg")
        .args(ffmpeg_args(&path, size))
        .stdin(async_process::Stdio::null())
        .stdout(async_process::Stdio::piped())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            eprintln!("AppleMusicArtService: ffmpeg did not start: {e}");
            return;
        }
    };
    pid.store(child.id() as i32, Ordering::Relaxed);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let Some(mut out) = child.stdout.take() else { return };
    let mut buf = vec![0u8; (size * size * 4) as usize];
    while out.read_exact(&mut buf).await.is_ok() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let raw = buf.clone();
        let slots = slots.clone();
        let bitmaps = ctx.pool().run(move || frames_for(raw, size, &slots)).await.flatten();
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(b) = bitmaps {
            ctx.publish(store::Diff::Media(media::Diff::Motion(Some((key.clone(), b)))));
        }
        if hold.load(Ordering::Relaxed) {
            signal(&pid, libc::SIGSTOP);
        }
    }
    pid.store(0, Ordering::Relaxed);
    let _ = child.kill();
    let _ = child.status().await;
}

/// One raw frame, decoded at the largest slot's `size`, as each slot draws
/// it: scaled to the slot and its corners cut to the slot's radius.
fn frames_for(raw: Vec<u8>, size: u32, slots: &[(u32, f64)]) -> Option<Vec<Bitmap>> {
    let img = image::RgbaImage::from_raw(size, size, raw)?;
    slots
        .iter()
        .map(|&(side, radius)| {
            let mut img = if side == size { img.clone() } else { image::imageops::resize(&img, side, side, image::imageops::FilterType::Triangle) };
            super::cover::round_corners(&mut img, radius);
            let px = u16::try_from(side).ok()?;
            Some(Bitmap::from_rgba(px, px, img.into_raw()))
        })
        .collect()
}

async fn curl(args: &[&str]) -> (i32, String) {
    let out = crate::services::proc::command("curl")
        .args(CURL)
        .args(args)
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await;
    match out {
        Ok(o) => (o.status.code().unwrap_or(-1), String::from_utf8_lossy(&o.stdout).into_owned()),
        Err(_) => (-1, String::new()),
    }
}

/// The chain for one album: its mp4 on disk, `Ok(None)` for an album with
/// no motion art (cached as such), `Err` for a failure worth retrying.
async fn lookup(w: &Job, dir: &str, key: &str, token: &mut String) -> Result<Option<String>, &'static str> {
    let Some(url) = am::artist_search_url(&w.artist) else { return Ok(None) };
    let (code, out) = curl(&[&url]).await;
    let Some(artist) = am::parse_artist_search_result(code, &out, &w.artist).map_err(|_| "itunes artist search failed")? else { return Ok(None) };
    let (code, out) = curl(&[&am::artist_albums_url(artist)]).await;
    let Some(collection) = am::parse_artist_albums_result(code, &out, &w.album).map_err(|_| "itunes album lookup failed")? else { return Ok(None) };
    if token.is_empty() {
        let (code, html) = curl(&[&am::album_page_url(collection)]).await;
        if code != 0 {
            return Err("web-player page fetch failed");
        }
        let asset = am::extract_asset_path(&html).ok_or("no asset bundle referenced by album page")?;
        let (code, bundle) = curl(&[&format!("https://music.apple.com{asset}")]).await;
        if code != 0 {
            return Err("asset bundle fetch failed");
        }
        *token = am::extract_token(&bundle).ok_or("no web-player token found in asset bundle")?;
    }
    let auth = format!("Authorization: Bearer {token}");
    let (code, out) = curl(&["-H", &auth, "-H", "Origin: https://music.apple.com", &am::amp_api_url(collection)]).await;
    let video = match am::parse_editorial_video(code, &out) {
        Ok(v) => v,
        Err(e) => {
            if e == am::ParseError::HttpError {
                token.clear();
            }
            return Err("editorial video lookup failed");
        }
    };
    if video.is_empty() {
        return Ok(None);
    }
    let (code, master) = curl(&[&video]).await;
    if code != 0 {
        return Err("master playlist fetch failed");
    }
    let Some(variant) = am::pick_variant(&master) else { return Ok(None) };
    let variant = am::resolve_url(&variant, &video);
    let (code, rendition) = curl(&[&variant]).await;
    if code != 0 {
        return Err("rendition playlist fetch failed");
    }
    let Some(mp4) = am::extract_mp4_url(&rendition, &variant) else { return Ok(None) };
    std::fs::create_dir_all(dir).map_err(|_| "could not create cache dir")?;
    let path = format!("{dir}/{key}.mp4");
    let part = format!("{path}.{}.part", std::process::id());
    let ok = crate::services::proc::command("curl")
        .args(["-sSf", "-L", "--max-redirs", "5", "--connect-timeout", "5", "--max-time", "60", "-o", &part, "--", &mp4])
        .stdin(async_process::Stdio::null())
        .stderr(async_process::Stdio::null())
        .kill_on_drop(true)
        .status()
        .await
        .is_ok_and(|s| s.success());
    if !ok {
        let _ = std::fs::remove_file(&part);
        return Err("artwork download failed");
    }
    std::fs::rename(&part, &path).map_err(|_| "could not install downloaded artwork")?;
    Ok(Some(path))
}

/// Drops cached artwork older than a month.
async fn prune(dir: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let now = crate::services::notifications::now_ms() as f64;
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        let Some(mtime) = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) else { continue };
        if meta.is_file() && am::is_stale(mtime.as_secs_f64() * 1000.0, now, MAX_AGE_DAYS) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decode_is_looped_paced_and_cropped_to_the_slot() {
        let args = ffmpeg_args("/c/a.mp4", 96).join(" ");
        assert!(args.contains("-re -stream_loop -1 -i /c/a.mp4"), "{args}");
        assert!(args.contains("fps=25/3,scale=96:96:force_original_aspect_ratio=increase,crop=96:96"), "{args}");
        assert!(args.ends_with("-f rawvideo -pix_fmt rgba -"), "{args}");
    }

    #[test]
    fn a_raw_frame_becomes_a_rounded_square_per_slot() {
        let b = frames_for(vec![255; 16 * 16 * 4], 16, &[(16, 4.0), (8, 4.0)]).unwrap();
        let px = b[0].pixmap.data_as_u8_slice();
        assert_eq!(px[3], 0, "the corner is cut");
        assert_eq!(px[((8 * 16) + 8) * 4 + 3], 255);
        assert_eq!((b[1].pixmap.width(), b[1].pixmap.height()), (8, 8), "the smaller slot is scaled");
        assert_eq!(b[1].pixmap.data_as_u8_slice()[3], 0);
        assert!(frames_for(vec![0; 10], 16, &[(16, 0.0)]).is_none(), "a short read is no frame");
    }

    fn w(album: &str, size: u32, playing: bool) -> Want {
        Want { artist: "a".into(), album: album.into(), size, radius: 2.0, playing }
    }

    #[test]
    fn one_decode_serves_every_slot_on_the_album() {
        let mut wants = BTreeMap::new();
        assert_eq!(job(&wants), None);
        wants.insert(5, w("x", 17, false));
        wants.insert(6, w("x", 17, false));
        let bar = job(&wants).unwrap();
        assert_eq!((bar.slots.len(), bar.playing), (1, false), "two bars of one size share a slot");
        wants.insert(PANEL, w("x", 96, true));
        let both = job(&wants).unwrap();
        assert_eq!(both.slots.iter().map(|s| s.0).collect::<Vec<_>>(), vec![96, 17], "decoded at the panel's size");
        assert!(both.playing);
        wants.insert(PANEL, w("y", 96, true));
        assert_eq!(job(&wants).unwrap().slots.len(), 1, "the panel's album wins");
    }
}
