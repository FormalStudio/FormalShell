//! The wallpaper picker's scan and the thumbnail `cover`
//! half: a directory listed (itself and its Dark/Light subdirectories) by a
//! `find` child, every image prerendered to a 512px square in the shared
//! thumbnail cache, and the cells' pictures decoded from that cache on the
//! pool. A directory is scanned when the route is entered, and once when
//! `picker.directory` is first read so the grid opens warm.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;

use async_channel::{Receiver, Sender};
use fs_menu::providers;
use fs_menu::thumbnails::{self, Mode};

use super::clipboard::{THUMB_SIZE, thumb_dir};
use super::proc;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

#[derive(Default)]
pub struct State {
    /// The directory the listing belongs to.
    pub dir: String,
    pub scanned: Vec<String>,
    /// Images whose `cover` thumbnail is in the cache.
    pub cached: HashSet<String>,
    pub thumbs: HashMap<String, Option<Bitmap>>,
    /// The cell edge the thumbnails were made for, and the scale.
    pub thumb_px: (u32, f64),
}

pub enum Diff {
    Scanned(String, Vec<String>),
    Cached(Vec<String>),
    Thumbs((u32, f64), Vec<(String, Option<Bitmap>)>),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Scanned(dir, list) => {
                if self.dir == dir && self.scanned == list {
                    return false;
                }
                self.dir = dir;
                self.scanned = list;
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
}

pub enum Cmd {
    Scan(String),
    /// `picker.directory` as read: scanned and warmed once per value.
    Boot(String),
    /// Decode these images' cells at `px` square on a surface at a scale.
    Thumbs(Vec<String>, (u32, f64)),
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
                Cmd::Scan(dir)
            }
            c => c,
        };
        match cmd {
            Cmd::Boot(_) => {}
            Cmd::Scan(dir) => {
                if dir.is_empty() {
                    ctx.publish(store::Diff::Picker(Diff::Scanned(dir, Vec::new())));
                    continue;
                }
                let done = proc::capture(&providers::picker_scan_command(&dir), Duration::from_secs(30)).await;
                let list: Vec<String> = done.stdout.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect();
                ctx.publish(store::Diff::Picker(Diff::Scanned(dir, list.clone())));
                let c = ctx.clone();
                ctx.spawn(async move {
                    let ready = warm(&list).await;
                    c.publish(store::Diff::Picker(Diff::Cached(ready)));
                });
            }
            Cmd::Thumbs(paths, px) => {
                let c = ctx.clone();
                ctx.spawn(async move {
                    if let Some(list) = c.pool().run(move || decode(&paths, px.0, px.1)).await {
                        c.publish(store::Diff::Picker(Diff::Thumbs(px, list)));
                    }
                });
            }
        }
    }
}

/// ThumbnailService.warm in `cover` mode: the sources whose thumbnail is
/// now in the cache, a hit or freshly drawn.
async fn warm(paths: &[String]) -> Vec<String> {
    if paths.is_empty() {
        return Vec::new();
    }
    let dir = thumb_dir();
    let size = THUMB_SIZE.to_string();
    let mut argv = proc::argv(&["sh", "-c", &thumbnails::warm_script(4), "sh", &dir, &size, Mode::Cover.as_str()]);
    for p in paths {
        argv.push(p.clone());
        argv.push(thumbnails::cache_path(&dir, p, THUMB_SIZE, Mode::Cover));
    }
    let done = proc::capture(&argv, Duration::from_secs(300)).await;
    done.stdout.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// Each image as a `px` square cover crop on a surface at `scale`, from its
/// cached thumbnail when there is one; a flat colour's cell is that colour.
fn decode(paths: &[String], px: u32, scale: f64) -> Vec<(String, Option<Bitmap>)> {
    let dir = thumb_dir();
    let px = crate::scene::pixels_at(f64::from(px), scale);
    paths
        .iter()
        .map(|p| {
            let img = match super::wallpaper::flat(p) {
                Some([r, g, b]) => Some(image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(px.max(1), px.max(1), image::Rgba([r, g, b, 255])))),
                None => image::open(thumbnails::cache_path(&dir, p, THUMB_SIZE, Mode::Cover)).or_else(|_| image::open(p)).ok(),
            };
            let bitmap = img.map(|i| {
                let fitted = i.resize_to_fill(px.max(1), px.max(1), image::imageops::FilterType::Triangle).into_rgba8();
                Bitmap::from_rgba(fitted.width() as u16, fitted.height() as u16, fitted.into_raw()).at(scale)
            });
            (p.clone(), bitmap)
        })
        .collect()
}
