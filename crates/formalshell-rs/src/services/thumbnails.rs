//! Thumbnail warming for the wallpaper picker (`wide`) and the clipboard
//! (`fit`): ffmpeg draws each source into the cache directory, a few at a
//! time. Blocking, so it runs on the pool.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use fs_menu::thumbnails::Mode;

use super::proc;

/// Cached files untouched this long are swept; a hit touches its file, so
/// the sweep is an LRU.
const KEEP: Duration = Duration::from_secs(31 * 24 * 3600);
const POLL: Duration = Duration::from_millis(50);

/// Draws each `(source, destination)` pair at `size`, `concurrency` at a
/// time, and answers the sources whose thumbnail is now in the cache, a hit
/// or freshly drawn, in the order they landed. A source not newer than its
/// thumbnail is a hit. A failed ffmpeg leaves nothing behind and is not
/// answered. Past `budget` no new one starts and a running one is killed.
pub fn warm(dir: &str, size: u32, mode: Mode, pairs: &[(String, String)], concurrency: usize, budget: Duration) -> Vec<String> {
    if std::fs::create_dir_all(dir).is_err() {
        return Vec::new();
    }
    sweep(Path::new(dir));
    let filter = mode.filter(size);
    let deadline = Instant::now() + budget;
    let next = AtomicUsize::new(0);
    let landed = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..concurrency.clamp(1, pairs.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((src, dst)) = pairs.get(i) else { return };
                    if Instant::now() >= deadline {
                        return;
                    }
                    if draw(src, dst, &filter, deadline)
                        && let Ok(mut landed) = landed.lock()
                    {
                        landed.push(src.clone());
                    }
                }
            });
        }
    });
    landed.into_inner().unwrap_or_default()
}

fn sweep(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        let partial = entry.file_name().to_string_lossy().ends_with(".part.jpg");
        let old = meta.modified().ok().and_then(|m| SystemTime::now().duration_since(m).ok()).is_some_and(|age| age >= KEEP);
        if meta.is_file() && (partial || old) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn draw(src: &str, dst: &str, filter: &str, deadline: Instant) -> bool {
    let modified = |p: &str| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if let Some(cached) = modified(dst).filter(|_| Path::new(dst).is_file()) {
        // `! src -nt dst`: a missing source counts as not newer.
        if modified(src).is_none_or(|s| s <= cached) {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dst) {
                let _ = f.set_modified(SystemTime::now());
            }
            return true;
        }
    }
    let tmp = format!("{dst}.part.jpg");
    let child = proc::std_command("ffmpeg")
        .args(["-v", "error", "-nostdin", "-y", "-i", src, "-frames:v", "1", "-q:v", "3", "-vf", filter, &tmp])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let ok = child.is_ok_and(|mut child| loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => std::thread::sleep(POLL),
            Err(_) => break false,
        }
    });
    if ok && std::fs::rename(&tmp, dst).is_ok() {
        return true;
    }
    let _ = std::fs::remove_file(&tmp);
    false
}
