//! The camera mirror: the cameras
//! listed when the view opens, and the current one streamed in process on a
//! thread of its own, each frame mirrored and fitted to the feed box before
//! it reaches the UI. A node is held open only while the view shows it.
//!
//! An IR camera's emitter lights every other frame, so its feed goes
//! through the IR filter: a frame darker than the one before is
//! dropped, and the lit one is levelled to a mean of `IR_TARGET` with at
//! most `IR_MAX_GAIN` and a gamma, so the face reads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};

use async_channel::{Receiver, Sender};
use fs_system::camera::{self, Device, Row};

use super::v4l2;
use crate::runtime::Ctx;
use crate::scene::Bitmap;
use crate::store;

const LIT_RATIO: f64 = 1.03;
const IR_TARGET: f64 = 0.45;
const IR_MAX_GAIN: f64 = 6.0;
const IR_GAMMA: f64 = 0.8;

#[derive(Default)]
pub struct State {
    pub cameras: Vec<Row>,
    pub listed: bool,
    pub streaming: Option<String>,
    pub frame: Option<(String, Bitmap)>,
    pub error: String,
}

pub enum Diff {
    Cameras(Vec<Row>),
    Streaming(Option<String>),
    Frame(String, Bitmap),
    Error(String),
}

impl State {
    pub fn apply(&mut self, diff: Diff) -> bool {
        match diff {
            Diff::Cameras(list) => {
                self.cameras = list;
                self.listed = true;
            }
            Diff::Streaming(id) => {
                if id.is_none() {
                    self.frame = None;
                }
                self.streaming = id;
            }
            Diff::Frame(id, b) => {
                if self.streaming.as_deref() != Some(id.as_str()) {
                    return false;
                }
                self.frame = Some((id, b));
            }
            Diff::Error(e) => self.error = e,
        }
        true
    }

    pub fn has_frame(&self, id: &str) -> bool {
        self.frame.as_ref().is_some_and(|(f, _)| f == id)
    }
}

pub enum Cmd {
    /// List the cameras again.
    List,
    /// Stream this camera into a `(w, h)` box, or stop.
    Stream(Option<(String, bool)>),
}

static CHANNEL: LazyLock<(Sender<Cmd>, Receiver<Cmd>)> = LazyLock::new(async_channel::unbounded);
/// The feed box the frames are fitted to, `w << 32 | h`, in the pixels of
/// the surface at `SCALE` (in 120ths) they are drawn on.
static BOX: AtomicU64 = AtomicU64::new((640 << 32) | 480);
static SCALE: AtomicU64 = AtomicU64::new(120);

pub fn command(cmd: Cmd) {
    let _ = CHANNEL.0.try_send(cmd);
}

/// The feed box, `w` by `h` logical pixels on a surface at `scale`.
pub fn set_box(w: u32, h: u32, scale: f64) {
    let (w, h) = (crate::scene::pixels_at(f64::from(w), scale), crate::scene::pixels_at(f64::from(h), scale));
    BOX.store((u64::from(w) << 32) | u64::from(h), Ordering::Relaxed);
    SCALE.store((scale * 120.0).round() as u64, Ordering::Relaxed);
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

pub async fn run(ctx: Ctx) {
    let (ftx, frx) = async_channel::bounded::<(String, Result<Bitmap, String>)>(2);
    let mut running: Option<(String, Running)> = None;
    let c = ctx.clone();
    ctx.spawn(async move {
        while let Ok((id, frame)) = frx.recv().await {
            match frame {
                Ok(b) => c.publish(store::Diff::Mirror(Diff::Frame(id, b))),
                Err(e) => c.publish(store::Diff::Mirror(Diff::Error(e))),
            }
        }
    });
    while let Ok(cmd) = CHANNEL.1.recv().await {
        match cmd {
            Cmd::List => {
                let list = ctx.pool().run(list).await.unwrap_or_default();
                ctx.publish(store::Diff::Mirror(Diff::Cameras(list)));
            }
            Cmd::Stream(want) => {
                if running.as_ref().map(|(id, _)| id) == want.as_ref().map(|(id, _)| id) {
                    continue;
                }
                if let Some((_, r)) = running.take() {
                    r.stop.store(true, Ordering::Relaxed);
                    let t = r.thread;
                    // The node has to be released before the next opens.
                    ctx.pool().run(move || drop(t.join())).await;
                }
                ctx.publish(store::Diff::Mirror(Diff::Error(String::new())));
                if let Some((id, ir)) = want {
                    let stop = Arc::new(AtomicBool::new(false));
                    let (s, tx, path) = (stop.clone(), ftx.clone(), id.clone());
                    let thread = std::thread::Builder::new().name("mirror".into()).spawn(move || capture(&path, ir, &s, &tx));
                    if let Ok(thread) = thread {
                        running = Some((id.clone(), Running { stop, thread }));
                    }
                    ctx.publish(store::Diff::Mirror(Diff::Streaming(Some(id))));
                } else {
                    ctx.publish(store::Diff::Mirror(Diff::Streaming(None)));
                }
            }
        }
    }
}

fn list() -> Vec<Row> {
    let devices: Vec<Device> = v4l2::list()
        .into_iter()
        .map(|n| {
            let grey_only = !n.formats.is_empty() && n.formats.iter().all(|f| *f == v4l2::GREY || *f == v4l2::Y16);
            Device { id: n.path, description: n.card, grey_only }
        })
        .collect();
    camera::rows(&devices)
}

fn capture(path: &str, ir: bool, stop: &AtomicBool, tx: &Sender<(String, Result<Bitmap, String>)>) {
    let prefer = [v4l2::YUYV, v4l2::MJPG, v4l2::GREY, v4l2::Y16];
    let mut stream = match v4l2::Stream::open(path, &prefer) {
        Ok(s) => s,
        Err(e) => {
            let _ = tx.send_blocking((path.to_owned(), Err(format!("Camera unavailable: {e}"))));
            return;
        }
    };
    if !prefer.contains(&stream.format()) {
        let _ = tx.send_blocking((path.to_owned(), Err("Camera format not supported".into())));
        return;
    }
    let mut last_mean = 0.0;
    while !stop.load(Ordering::Relaxed) {
        let mut out = None;
        let got = stream.next(200, &mut |f| out = rgb(&f));
        match got {
            Err(e) => {
                let _ = tx.send_blocking((path.to_owned(), Err(format!("Camera stopped: {e}"))));
                return;
            }
            Ok(false) => continue,
            Ok(true) => {}
        }
        let Some((mut px, w, h)) = out else { continue };
        if ir {
            let mean = px.iter().step_by(3).map(|v| f64::from(*v)).sum::<f64>() / (px.len() / 3).max(1) as f64;
            let dark = mean * LIT_RATIO < last_mean;
            last_mean = mean;
            if dark {
                continue;
            }
            level(&mut px, mean);
        }
        let b = BOX.load(Ordering::Relaxed);
        let (bw, bh) = ((b >> 32) as u32, (b & 0xffff_ffff) as u32);
        let Some(img) = image::RgbImage::from_raw(w, h, px) else { continue };
        let scale = (f64::from(bw) / f64::from(w)).min(f64::from(bh) / f64::from(h));
        let (tw, th) = (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1));
        let fitted = image::imageops::flip_horizontal(&image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle));
        let rgba = image::DynamicImage::ImageRgb8(fitted).into_rgba8();
        let density = SCALE.load(Ordering::Relaxed) as f64 / 120.0;
        let bitmap = Bitmap::from_rgba(rgba.width() as u16, rgba.height() as u16, rgba.into_raw()).at(density);
        if tx.send_blocking((path.to_owned(), Ok(bitmap))).is_err() {
            return;
        }
    }
}

/// The IR level pass on a grey frame.
fn level(px: &mut [u8], mean: f64) {
    let gain = (IR_TARGET * 255.0 / mean.max(1.0)).min(IR_MAX_GAIN);
    for v in px.iter_mut() {
        let x = (f64::from(*v) * gain / 255.0).clamp(0.0, 1.0);
        *v = (x.powf(IR_GAMMA) * 255.0).round() as u8;
    }
}

/// A frame as RGB triples.
fn rgb(f: &v4l2::Frame) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h) = (f.width as usize, f.height as usize);
    let stride = (f.stride as usize).max(1);
    let mut out = Vec::with_capacity(w * h * 3);
    match f.format {
        v4l2::YUYV => {
            for y in 0..h {
                let row = f.data.get(y * stride..y * stride + w * 2)?;
                for p in row.chunks_exact(4) {
                    let (y0, u, y1, v) = (f64::from(p[0]), f64::from(p[1]) - 128.0, f64::from(p[2]), f64::from(p[3]) - 128.0);
                    for yy in [y0, y1] {
                        out.push((yy + 1.402 * v).clamp(0.0, 255.0) as u8);
                        out.push((yy - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8);
                        out.push((yy + 1.772 * u).clamp(0.0, 255.0) as u8);
                    }
                }
            }
        }
        v4l2::GREY => {
            for y in 0..h {
                for v in f.data.get(y * stride..y * stride + w)? {
                    out.extend_from_slice(&[*v, *v, *v]);
                }
            }
        }
        v4l2::Y16 => {
            for y in 0..h {
                for p in f.data.get(y * stride..y * stride + w * 2)?.chunks_exact(2) {
                    let v = p[1];
                    out.extend_from_slice(&[v, v, v]);
                }
            }
        }
        v4l2::MJPG => {
            let img = image::load_from_memory_with_format(f.data, image::ImageFormat::Jpeg).ok()?.into_rgb8();
            let (w, h) = (img.width(), img.height());
            return Some((img.into_raw(), w, h));
        }
        _ => return None,
    }
    Some((out, w as u32, h as u32))
}
