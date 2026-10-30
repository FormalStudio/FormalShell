//! The ledger at `$XDG_STATE_HOME/formalshell/
//! clipboard.json`, fed by three `wl-paste --watch` children started with
//! the shell, never by a first reader.
//!
//! - `text`: each change forked into `cat; printf '\0'`, so entries are NUL
//!   delimited. A capture settles [`SETTLE`] before it is recorded and a
//!   newer one inside that window replaces it: one app rewriting the
//!   clipboard per keystroke otherwise turns the whole ledger over.
//! - `image/png`: the bytes stored content-addressed as `<sha256>.png`
//!   under `clipboard-images/`, the path printed NUL delimited.
//! - `text/uri-list`: how a GTK4 app copies an image (the file, never its
//!   pixels). One local picture is imported into the same store, through
//!   ffmpeg when it is not a png, and the text watcher's echo of the same
//!   copy is dropped.
//!
//! `CLIPBOARD_STATE=sensitive` (a password manager's hint) is never
//! forwarded. Each watcher dies with the shell and restarts 3s after it
//! exits. Every eviction's orphaned images are removed here, and only
//! under the image store.

use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use fs_menu::clipboard::history::{self, Entry, EntryKind, NewEntry};
use fs_menu::thumbnails::{self, Mode};
use futures_lite::io::BufReader;
use futures_lite::{AsyncBufReadExt, FutureExt, future};

use super::proc;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

const SETTLE: Duration = Duration::from_millis(300);
const ECHO_MS: i64 = 5000;
const RESTART: Duration = Duration::from_secs(3);
/// The thumbnail edge, shared with the picker's cache.
pub const THUMB_SIZE: u32 = 512;

#[derive(Default)]
pub struct State {
    pub items: Vec<Entry>,
    /// An image just captured, for `clipssh.autoSendImages`, taken by the
    /// one reader. A re-copied image only moves its entry, so the list
    /// alone cannot say a capture happened.
    pub captured: bool,
    /// The launcher pane's picture: a capture or an image file.
    pub preview: Option<Preview>,
    /// What [`Cmd::Paste`] read, for the field holding the keyboard to take.
    pub pasted: Option<String>,
}

/// One picture fitted to the launcher's preview pane.
pub struct Preview {
    pub path: String,
    /// The box it was fitted to, and the scale.
    pub size: ((u32, u32), f64),
    pub bitmap: Option<Bitmap>,
    /// Its own width and height, read off the file's header.
    pub dims: Option<(u32, u32)>,
}

pub enum Diff {
    Items(Vec<Entry>),
    Captured,
    Preview(Preview),
    Pasted(String),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Items(items) => {
                if self.items == items {
                    return false;
                }
                self.items = items;
            }
            Diff::Captured => self.captured = true,
            Diff::Pasted(text) => self.pasted = Some(text),
            Diff::Preview(preview) => self.preview = Some(preview),
        }
        true
    }

    /// The IPC `list` reply.
    pub fn list(&self) -> String {
        serde_json::to_string(&self.items).unwrap_or_else(|_| "[]".into())
    }
}

pub enum Cmd {
    /// A history entry, by id, back on the clipboard.
    Copy(String),
    /// Text a field copied or cut.
    SetText(String),
    /// The clipboard's text, read for a field, lands as [`Diff::Pasted`].
    Paste,
    Remove(String),
    Clear,
    /// The launcher pane's picture of a capture or an image file, fitted to
    /// `(w, h)`.
    Preview(String, ((u32, u32), f64)),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);

/// Callable from any thread.
pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

fn state_dir() -> PathBuf {
    super::state::state_path().parent().map(PathBuf::from).unwrap_or_default()
}

pub fn images_dir() -> PathBuf {
    state_dir().join("clipboard-images")
}

pub fn thumb_dir() -> String {
    let base = match std::env::var("XDG_CACHE_HOME") {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"),
    };
    base.join("formalshell/thumbnails").to_string_lossy().into_owned()
}

/// The store's half of a capture, with the bytes already in `$tmp` under
/// `$dir`: an empty read leaves nothing, the file is named by its sha256,
/// and the final path goes out NUL delimited.
const STORE_SCRIPT: &str = "if [ ! -s \"$tmp\" ]; then rm -f \"$tmp\"; exit 0; fi; \
    hash=$(sha256sum \"$tmp\" | cut -d ' ' -f1); file=\"$dir/$hash.png\"; \
    if [ -e \"$file\" ]; then rm -f \"$tmp\"; else mv \"$tmp\" \"$file\"; fi; \
    printf '%s\\0' \"$file\"";

enum Event {
    Text(String),
    Image(String),
    Uri(String),
    Imported(u64, String, fs_screensaver::urilist::ImageFile),
    Cmd(Cmd),
    Settled,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

pub async fn run(ctx: Ctx) {
    let path = state_dir().join("clipboard.json");
    let images = images_dir();
    let loaded = {
        let path = path.clone();
        ctx.pool().run(move || load(&path)).await.unwrap_or_default()
    };
    let mut items = loaded;
    ctx.publish(store::Diff::Clipboard(Diff::Items(items.clone())));
    warm(&ctx, items.iter().filter_map(image_path).collect());

    let (tx, rx) = async_channel::unbounded::<Event>();
    let sensitive = "[ \"$CLIPBOARD_STATE\" = sensitive ] && exit 0; ";
    let text = format!("{sensitive}cat; printf '\\0'");
    let image = format!("{sensitive}dir=\"$0\"; mkdir -p \"$dir\" || exit 0; tmp=$(mktemp \"$dir/tmp.XXXXXX\") || exit 0; cat > \"$tmp\"; {STORE_SCRIPT}");
    let dir = images.to_string_lossy().into_owned();
    ctx.spawn(watch(vec!["--type".into(), "text".into(), "--watch".into(), "sh".into(), "-c".into(), text.clone()], tx.clone(), Event::Text));
    ctx.spawn(watch(vec!["--type".into(), "image/png".into(), "--watch".into(), "sh".into(), "-c".into(), image, dir.clone()], tx.clone(), Event::Image));
    ctx.spawn(watch(vec!["--type".into(), "text/uri-list".into(), "--watch".into(), "sh".into(), "-c".into(), text], tx.clone(), Event::Uri));

    let mut seq: u64 = 0;
    let mut next_id = move || {
        seq += 1;
        format!("{}-{seq}", now_ms())
    };
    let mut pending: Option<(String, Instant)> = None;
    let mut import_gen: u64 = 0;
    loop {
        let settle = async {
            match &pending {
                Some((_, at)) => {
                    async_io::Timer::at(*at).await;
                    Some(Event::Settled)
                }
                None => future::pending().await,
            }
        };
        let event = async { rx.recv().await.ok() }.or(async { CHANNEL.1.recv().await.ok().map(Event::Cmd) }).or(settle).await;
        let Some(event) = event else { return };
        let before = items.clone();
        let mut removed = Vec::new();
        let state = |items: &Vec<Entry>| history::State { items: items.clone() };
        match event {
            Event::Text(t) => pending = Some((t, Instant::now() + SETTLE)),
            Event::Settled => {
                if let Some((t, _)) = pending.take() {
                    let r = history::add(&state(&items), &NewEntry { id: next_id(), text: Some(t), ..Default::default() }, now_ms());
                    items = r.state.items;
                    removed = r.removed_paths;
                }
            }
            Event::Image(p) => {
                let r = history::add(&state(&items), &NewEntry { id: next_id(), kind: EntryKind::Image, path: Some(p.clone()), ..Default::default() }, now_ms());
                items = r.state.items;
                removed = r.removed_paths;
                warm(&ctx, vec![p]);
                ctx.publish(store::Diff::Clipboard(Diff::Captured));
            }
            Event::Uri(data) => {
                if let Some(file) = fs_screensaver::urilist::image_file(Some(&data)) {
                    import_gen += 1;
                    let (g, tx, dir) = (import_gen, tx.clone(), dir.clone());
                    ctx.spawn(async move {
                        let script = format!(
                            "dir=\"$0\"; src=\"$1\"; [ -f \"$src\" ] || exit 0; mkdir -p \"$dir\" || exit 0; \
                             tmp=$(mktemp \"$dir/tmp.XXXXXX\") || exit 0; \
                             if [ \"$2\" = png ]; then cp -- \"$src\" \"$tmp\"; \
                             else ffmpeg -v error -y -i \"$src\" -frames:v 1 -update 1 -c:v png -f image2 \"$tmp\"; fi \
                             || {{ rm -f \"$tmp\"; exit 0; }}; {STORE_SCRIPT}"
                        );
                        let kind = if file.png { "png" } else { "convert" };
                        let done = proc::capture(&proc::argv(&["sh", "-c", &script, &dir, &file.path, kind]), Duration::from_secs(30)).await;
                        if let Some(p) = done.stdout.split('\0').find(|p| !p.is_empty()) {
                            let _ = tx.send(Event::Imported(g, p.to_owned(), file)).await;
                        }
                    });
                }
            }
            Event::Imported(g, p, source) => {
                if g == import_gen {
                    let echoes = [source.path.as_str(), source.uri.as_str()];
                    if pending.as_ref().is_some_and(|(t, _)| echoes.contains(&fs_js::trim(&t.replace('\0', "")))) {
                        pending = None;
                    }
                    items = history::drop_echo(&state(&items), &echoes, now_ms(), ECHO_MS).items;
                    let r = history::add(&state(&items), &NewEntry { id: next_id(), kind: EntryKind::Image, path: Some(p.clone()), ..Default::default() }, now_ms());
                    items = r.state.items;
                    removed = r.removed_paths;
                    warm(&ctx, vec![p]);
                    ctx.publish(store::Diff::Clipboard(Diff::Captured));
                }
            }
            Event::Cmd(Cmd::Copy(id)) => {
                if let Some(e) = items.iter().find(|i| i.id == id) {
                    let argv = match e.kind {
                        EntryKind::Image => proc::argv(&["sh", "-c", "exec wl-copy --type image/png < \"$0\"", e.path.as_deref().unwrap_or("")]),
                        EntryKind::Text => proc::argv(&["wl-copy", "--", e.text.as_deref().unwrap_or("")]),
                    };
                    ctx.spawn(async move {
                        proc::capture(&argv, Duration::from_secs(10)).await;
                    });
                }
            }
            Event::Cmd(Cmd::SetText(text)) => {
                ctx.spawn(async move {
                    proc::capture_exit(&proc::argv(&["wl-copy", "--", &text]), Duration::from_secs(10)).await;
                });
            }
            Event::Cmd(Cmd::Paste) => {
                let c = ctx.clone();
                ctx.spawn(async move {
                    let done = proc::capture(&proc::argv(&["wl-paste", "--no-newline", "--type", "text"]), Duration::from_secs(5)).await;
                    if done.code == 0 && !done.stdout.is_empty() {
                        c.publish(store::Diff::Clipboard(Diff::Pasted(done.stdout)));
                    }
                });
            }
            Event::Cmd(Cmd::Remove(id)) => {
                let r = history::remove(&state(&items), &id);
                items = r.state.items;
                removed = r.removed_paths;
            }
            Event::Cmd(Cmd::Clear) => {
                // A capture still settling belongs to the history being cleared.
                pending = None;
                let r = history::clear(&state(&items));
                items = r.state.items;
                removed = r.removed_paths;
            }
            Event::Cmd(Cmd::Preview(path, size)) => {
                let c = ctx.clone();
                ctx.spawn(async move {
                    let p = path.clone();
                    let decoded = c.pool().run(move || (image::image_dimensions(&p).ok(), decode_thumbs(&[p], size.0, size.1))).await;
                    if let Some((dims, mut list)) = decoded {
                        let bitmap = list.pop().and_then(|(_, b)| b);
                        c.publish(store::Diff::Clipboard(Diff::Preview(Preview { path, size, bitmap, dims })));
                    }
                });
            }
        }
        let prefix = format!("{}/", images.to_string_lossy());
        let safe: Vec<String> = removed.into_iter().filter(|p| p.starts_with(&prefix)).collect();
        if !safe.is_empty() {
            ctx.pool().run(move || safe.iter().for_each(|p| drop(std::fs::remove_file(p)))).await;
        }
        if items != before {
            ctx.publish(store::Diff::Clipboard(Diff::Items(items.clone())));
            let (path, text) = (path.clone(), serde_json::json!({ "items": items }).to_string());
            ctx.pool().run(move || {
                if let Err(err) = super::state::write_atomic(&path, &text) {
                    eprintln!("clipboard: {}: {err}", path.display());
                }
            })
            .await;
        }
    }
}

fn image_path(e: &Entry) -> Option<String> {
    (e.kind == EntryKind::Image).then(|| e.path.clone()).flatten()
}

fn load(path: &std::path::Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    v.get("items").and_then(|i| i.as_array()).map_or_else(Vec::new, |a| a.iter().filter_map(history::normalize_entry).collect())
}

/// One `wl-paste --watch` child, forever: each NUL-delimited record sent
/// as `wrap(record)`, restarted [`RESTART`] after it exits (no
/// data-control protocol, or no wl-paste at all).
async fn watch(args: Vec<String>, tx: Sender<Event>, wrap: fn(String) -> Event) {
    loop {
        let child = crate::services::proc::command("setpriv")
            .args(["--pdeathsig", "TERM", "--", "wl-paste"])
            .args(&args)
            .stdin(async_process::Stdio::null())
            .stdout(async_process::Stdio::piped())
            .stderr(async_process::Stdio::null())
            .kill_on_drop(true)
            .spawn();
        if let Ok(mut child) = child {
            if let Some(out) = child.stdout.take() {
                let mut reader = BufReader::new(out);
                let mut buf = Vec::new();
                loop {
                    buf.clear();
                    match reader.read_until(0, &mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            if buf.last() == Some(&0) {
                                buf.pop();
                            }
                            if tx.send(wrap(String::from_utf8_lossy(&buf).into_owned())).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            }
            let _ = child.status().await;
        }
        async_io::Timer::after(RESTART).await;
    }
}

/// ThumbnailService.warm in `fit` mode for these captures, off the loop.
fn warm(ctx: &Ctx, paths: Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let dir = thumb_dir();
    let size = THUMB_SIZE.to_string();
    let mut argv = proc::argv(&["sh", "-c", &thumbnails::warm_script(4), "sh", &dir, &size, Mode::Fit.as_str()]);
    for p in &paths {
        argv.push(p.clone());
        argv.push(thumbnails::cache_path(&dir, p, THUMB_SIZE, Mode::Fit));
    }
    ctx.spawn(async move {
        proc::capture(&argv, Duration::from_secs(60)).await;
    });
}

/// Each capture fitted inside `size` on a surface at `scale`, from its
/// `fit` thumbnail when the cache has one, else from the capture itself.
fn decode_thumbs(paths: &[String], size: (u32, u32), scale: f64) -> Vec<(String, Option<Bitmap>)> {
    let dir = thumb_dir();
    let size = (crate::scene::pixels_at(f64::from(size.0), scale), crate::scene::pixels_at(f64::from(size.1), scale));
    paths
        .iter()
        .map(|p| {
            let cached = thumbnails::cache_path(&dir, p, THUMB_SIZE, Mode::Fit);
            let img = image::open(&cached).or_else(|_| image::open(p)).ok();
            let bitmap = img.map(|i| {
                let fitted = i.resize(size.0.max(1), size.1.max(1), image::imageops::FilterType::Triangle).into_rgba8();
                Bitmap::from_rgba(fitted.width() as u16, fitted.height() as u16, fitted.into_raw()).at(scale)
            });
            (p.clone(), bitmap)
        })
        .collect()
}
